use super::chip::{
    build_rp2040_boot2_256b, build_rp2350_picobin_block, RpChipKind, RpChipProfile,
};
use super::thumb_enc::{Cond, GlobalReloc, GlobalRelocKind, ThumbAssembler};

pub const SRAM_G_HEAP_PTR_OFF: u32 = 0x00;
pub const SRAM_G_RET_PTR_OFF: u32 = 0x04;
pub const SRAM_G_RNG_STATE_OFF: u32 = 0x08;
pub const SRAM_USER_GLOBALS_OFF: u32 = 0x20;

#[derive(Debug, Clone)]
pub struct SramDataInit {
    pub flash_lma_sym: String,
    pub sram_vma: u32,
    pub size_bytes: u32,
}

/// Emit the chip-specific XIP boot header (`boot2` + Vector Table on RP2040; Vector Table + `PICOBIN` on RP2350).
pub fn emit_boot_header(asm: &mut ThumbAssembler, chip: &RpChipProfile) {
    match chip.kind {
        RpChipKind::Rp2040 => {
            // 0x0000..0x0100: 256-byte CRC-32 verified `boot2` XIP flash setup
            let boot2 = build_rp2040_boot2_256b();
            asm.code.extend_from_slice(&boot2);
            debug_assert_eq!(asm.pos(), 0x100);

            // 0x0100..0x01C0: 48-entry ARMv6-M Vector Table (16 system + 32 IRQ)
            asm.symbols.insert("__vector_table".to_string(), asm.pos());
            asm.emit32_le(chip.sram_top_msp);
            emit_vector_entry(asm, "Reset_Handler");
            for _ in 2..48 {
                emit_vector_entry(asm, "Default_ISR");
            }
        }
        RpChipKind::Rp2350 => {
            // 0x0000..0x0110: 68-entry ARMv8-M.main Vector Table (16 system + 52 IRQ)
            asm.symbols.insert("__vector_table".to_string(), asm.pos());
            asm.emit32_le(chip.sram_top_msp);
            emit_vector_entry(asm, "Reset_Handler");
            for _ in 2..68 {
                emit_vector_entry(asm, "Default_ISR");
            }

            // 0x0110..0x0124: Mandatory 5-word RP2350 `PICOBIN` `IMAGE_DEF` block (within first 4 KB)
            asm.symbols.insert("__picobin_block".to_string(), asm.pos());
            for w in build_rp2350_picobin_block() {
                asm.emit32_le(w);
            }
        }
    }
}

fn emit_vector_entry(asm: &mut ThumbAssembler, target_sym: &str) {
    let off = asm.pos();
    asm.emit32_le(0);
    asm.global_relocs.push(GlobalReloc {
        offset: off,
        symbol: target_sym.to_string(),
        kind: GlobalRelocKind::Abs32 {
            thumb_bit: true,
            addend: 0,
        },
    });
}

/// Emit `Reset_Handler`, `Default_ISR`, and the complete freestanding bare-metal OmniScript runtime
/// in pure ARMv6-M / ARMv8-M.main Thumb machine code (zero `pico-sdk`, zero `newlib`).
pub fn emit_bare_metal_runtime(
    asm: &mut ThumbAssembler,
    chip: &RpChipProfile,
    data_init: Option<&SramDataInit>,
) -> Result<(), String> {
    let base_vma = chip.flash_base;

    // -------------------------------------------------------------------------
    // 1. Default_ISR / exit / abort
    // -------------------------------------------------------------------------
    for trap_name in ["Default_ISR", "exit", "abort"] {
        asm.begin_function(trap_name);
        let spin = asm.new_label();
        asm.bind_label(spin);
        asm.emit_wfi();
        asm.emit_b_label_short(spin);
        asm.end_function(base_vma)?;
    }

    // -------------------------------------------------------------------------
    // 2. Reset_Handler: Clocks, Ticks, Resets, SRAM Init, UART0, call @main
    // -------------------------------------------------------------------------
    asm.begin_function("Reset_Handler");
    {
        // A) Enable 12 MHz XOSC (`STARTUP = 47`, `CTRL = 0x00FABAA0`), poll `STATUS` bit 31
        asm.emit_load_u32(0, chip.xosc_base);
        asm.emit_movs_imm8(1, 47);
        asm.emit_str_imm(1, 0, 0x0C);
        asm.emit_load_u32(1, 0x00FA_BAA0);
        asm.emit_str_imm(1, 0, 0x00);
        let xosc_wait = asm.new_label();
        asm.bind_label(xosc_wait);
        asm.emit_ldr_imm(1, 0, 0x04);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Pl, xosc_wait); // loop while bit 31 == 0

        // B) Switch `CLK_REF` -> `XOSC` (2), `CLK_SYS` -> `CLK_REF` (0), `CLK_PERI` -> `ENABLE | XOSC` (0x880)
        asm.emit_load_u32(0, chip.clocks_base);
        asm.emit_movs_imm8(1, 2);
        asm.emit_str_imm(1, 0, 0x30); // CLK_REF_CTRL
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 0, 0x3C); // CLK_SYS_CTRL
        asm.emit_load_u32(1, 0x0000_0880);
        asm.emit_str_imm(1, 0, 0x48); // CLK_PERI_CTRL

        // C) Configure 1 MHz microsecond tick generator (`12 MHz / 12 = 1 MHz`)
        match chip.kind {
            RpChipKind::Rp2040 => {
                asm.emit_load_u32(0, chip.watchdog_base);
                asm.emit_load_u32(1, 12 | (1 << 9)); // WATCHDOG_TICK_ENABLE | 12
                asm.emit_str_imm(1, 0, 0x2C);
            }
            RpChipKind::Rp2350 => {
                asm.emit_load_u32(0, chip.ticks_base);
                asm.emit_movs_imm8(1, 12);
                asm.emit_movs_imm8(2, 1);
                asm.emit_str_imm(1, 0, 0x04); // PROC0_CYCLES = 12
                asm.emit_str_imm(2, 0, 0x00); // PROC0_CTRL = ENABLE
                asm.emit_str_imm(1, 0, 0x1C); // TIMER0_CYCLES = 12
                asm.emit_str_imm(2, 0, 0x18); // TIMER0_CTRL = ENABLE
            }
        }

        // D) Unreset IO_BANK0, PADS_BANK0, PIO0..2, TIMER, UART0 via Atomic Clear (`RESETS_BASE + 0x3000`)
        asm.emit_load_u32(0, chip.resets_base + 0x3000);
        asm.emit_load_u32(2, chip.resets_unreset_mask);
        asm.emit_str_imm(2, 0, 0x00);
        asm.emit_load_u32(0, chip.resets_base);
        let rst_wait = asm.new_label();
        asm.bind_label(rst_wait);
        asm.emit_ldr_imm(1, 0, 0x08); // RESET_DONE
        asm.emit_ands(1, 2);
        asm.emit_cmp_reg(1, 2);
        asm.emit_bcond_label_short(Cond::Ne, rst_wait);

        // E) Initialize SRAM bump heap pointer & runtime globals at `0x2000_0000`
        asm.emit_load_u32(0, chip.sram_base);
        asm.emit_load_u32(1, chip.heap_start);
        asm.emit_str_imm(1, 0, SRAM_G_HEAP_PTR_OFF);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 0, SRAM_G_RET_PTR_OFF);
        asm.emit_load_u32(1, 0x6D2B_79F5);
        asm.emit_str_imm(1, 0, SRAM_G_RNG_STATE_OFF);

        // Copy initialized mutable `.data` globals from Flash LMA to SRAM if present
        if let Some(di) = data_init {
            if di.size_bytes > 0 {
                asm.emit_load_u32(0, di.sram_vma);
                asm.emit_load_sym_addr(1, &di.flash_lma_sym, false, 0);
                asm.emit_load_u32(2, di.size_bytes);
                asm.emit_bl_sym("__omni_memcpy32");
            }
        }

        // F) Configure UART0 (`115200` 8N1 @ 12 MHz `CLK_PERI`: `IBRD=6`, `FBRD=33`) + GP0/GP1 mux
        asm.emit_load_u32(0, chip.pads_bank0_base);
        asm.emit_movs_imm8(1, 0x56); // IE | DRIVE_4MA | PDE | SCHMITT, ISO=0 (un-isolated on RP2350!)
        asm.emit_str_imm(1, 0, 0x04); // GPIO0 pad
        asm.emit_str_imm(1, 0, 0x08); // GPIO1 pad

        asm.emit_load_u32(0, chip.io_bank0_base);
        asm.emit_movs_imm8(1, 2); // FUNCSEL = 2 (UART0)
        asm.emit_str_imm(1, 0, 0x04); // GPIO0_CTRL (TX)
        asm.emit_str_imm(1, 0, 0x0C); // GPIO1_CTRL (RX)

        asm.emit_load_u32(0, chip.uart0_base);
        asm.emit_movs_imm8(1, 6);
        asm.emit_str_imm(1, 0, 0x24); // UARTIBRD = 6
        asm.emit_movs_imm8(1, 33);
        asm.emit_str_imm(1, 0, 0x28); // UARTFBRD = 33
        asm.emit_movs_imm8(1, 0x70);  // WLEN=8 (3<<5) | FEN (1<<4)
        asm.emit_str_imm(1, 0, 0x2C); // UARTLCR_H
        asm.emit_load_u32(1, 0x0301); // RXE (1<<9) | TXE (1<<8) | UARTEN (1)
        asm.emit_str_imm(1, 0, 0x30); // UARTCR

        // G) Invoke `@main(0, null)` and halt cleanly in low-power WFI loop on return
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("main");

        let done_spin = asm.new_label();
        asm.bind_label(done_spin);
        asm.emit_wfi();
        asm.emit_b_label_short(done_spin);
    }
    asm.end_function(base_vma)?;

    // -------------------------------------------------------------------------
    // 3. Internal Fast Copy / Zero Helpers + C Memory / String Primitives
    // -------------------------------------------------------------------------
    // `__omni_memcpy32(dst=r0, src=r1, len=r2) -> r0=dst`
    asm.begin_function("__omni_memcpy32");
    {
        asm.emit_movs_reg(3, 0); // r3 = cur_dst
        let loop_lbl = asm.new_label();
        let done_lbl = asm.new_label();
        asm.bind_label(loop_lbl);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, done_lbl);
        asm.emit_push(1 << 4, false);
        asm.emit_ldrb_imm(4, 1, 0);
        asm.emit_strb_imm(4, 3, 0);
        asm.emit_pop(1 << 4, false);
        asm.emit_adds_imm8(1, 1);
        asm.emit_adds_imm8(3, 1);
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(loop_lbl);
        asm.bind_label(done_lbl);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `malloc(i64 size)` (`r0` = size) -> `r0` = 8-byte aligned zeroed SRAM ptr, `r1` = 0
    asm.begin_function("malloc");
    {
        asm.emit_push((1 << 4) | (1 << 5), false);
        // Align size up to 8 bytes (minimum 8 bytes)
        asm.emit_cmp_imm8(0, 0);
        let non_zero = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, non_zero);
        asm.emit_movs_imm8(0, 8);
        asm.bind_label(non_zero);
        asm.emit_adds_imm8(0, 7);
        asm.emit_movs_imm8(2, 7);
        asm.emit_bics(0, 2); // r0 = aligned_size

        asm.emit_load_u32(3, chip.sram_base);
        asm.emit_ldr_imm(4, 3, SRAM_G_HEAP_PTR_OFF); // r4 = old_heap
        asm.emit_adds_reg(5, 4, 0);                  // r5 = new_heap
        asm.emit_load_u32(2, chip.heap_limit);
        asm.emit_cmp_reg(5, 2);
        let ok_lbl = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ls, ok_lbl);
        // Out of SRAM: clamp / wrap to heap_start safely rather than faulting
        asm.emit_load_u32(4, chip.heap_start);
        asm.emit_adds_reg(5, 4, 0);
        asm.bind_label(ok_lbl);
        asm.emit_str_imm(5, 3, SRAM_G_HEAP_PTR_OFF);

        // Zero allocated words `[r4 .. r5)`
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_imm8(1, 0);
        let zloop = asm.new_label();
        let zdone = asm.new_label();
        asm.bind_label(zloop);
        asm.emit_cmp_reg(2, 5);
        asm.emit_bcond_label_short(Cond::Cs, zdone);
        asm.emit_str_imm(1, 2, 0);
        asm.emit_adds_imm8(2, 4);
        asm.emit_b_label_short(zloop);
        asm.bind_label(zdone);

        asm.emit_movs_reg(0, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `calloc(i64 nmemb, i64 size)` (`r0` = nmemb, `r2` = size)
    asm.begin_function("calloc");
    {
        asm.emit_muls(0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_push(0, true);
        asm.emit_bl_sym("malloc");
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `realloc(i64 ptr, i64 new_size)` (`r0` = old_ptr, `r2` = new_size)
    asm.begin_function("realloc");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_movs_reg(4, 0); // old_ptr
        asm.emit_movs_reg(5, 2); // new_size
        asm.emit_movs_reg(0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(4, 0);
        let skip = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, skip);
        asm.emit_movs_reg(1, 4);
        asm.emit_movs_reg(2, 5);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.bind_label(skip);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `free(i8* ptr)` -> no-op on bump/arena SRAM
    asm.begin_function("free");
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `memcpy` / `memmove` (`dst=r0`, `src=r2`, `n=[sp+0]`)
    for fn_name in ["memcpy", "memmove"] {
        asm.begin_function(fn_name);
        asm.emit_movs_reg(1, 2);
        asm.emit_ldr_sp(2, 0);
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // `memset(dst=r0, c=r2, n=[sp+0])`
    asm.begin_function("memset");
    {
        asm.emit_ldr_sp(3, 0); // n
        asm.emit_movs_reg(1, 0);
        let lp = asm.new_label();
        let dn = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(3, 0);
        asm.emit_bcond_label_short(Cond::Eq, dn);
        asm.emit_strb_imm(2, 1, 0);
        asm.emit_adds_imm8(1, 1);
        asm.emit_subs_imm8(3, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(dn);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `strlen(s=r0)` -> `r0:r1` length
    asm.begin_function("strlen");
    {
        asm.emit_movs_reg(1, 0);
        asm.emit_movs_imm8(0, 0);
        asm.emit_cmp_imm8(1, 0);
        let dn = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, dn);
        let lp = asm.new_label();
        asm.bind_label(lp);
        asm.emit_ldrb_imm(2, 1, 0);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, dn);
        asm.emit_adds_imm8(0, 1);
        asm.emit_adds_imm8(1, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(dn);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `strcmp(i8* a=r0, i8* b=r1) -> i32 r0`
    asm.begin_function("strcmp");
    {
        let lp = asm.new_label();
        let diff = asm.new_label();
        let eq = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, eq);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, eq);
        asm.bind_label(lp);
        asm.emit_ldrb_imm(2, 0, 0);
        asm.emit_ldrb_imm(3, 1, 0);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, diff);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, eq);
        asm.emit_adds_imm8(0, 1);
        asm.emit_adds_imm8(1, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(diff);
        asm.emit_subs_reg(0, 2, 3);
        asm.emit_bx(14);
        asm.bind_label(eq);
        asm.emit_movs_imm8(0, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_str_concat(i8* a=r0, i8* b=r1) -> i8* r0`
    asm.begin_function("__omni_str_concat");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_movs_reg(4, 0); // a
        asm.emit_movs_reg(5, 1); // b
        asm.emit_bl_sym("strlen");
        asm.emit_movs_reg(6, 0); // la
        asm.emit_movs_reg(0, 5);
        asm.emit_bl_sym("strlen");
        asm.emit_movs_reg(7, 0); // lb
        asm.emit_adds_reg(0, 6, 7);
        asm.emit_adds_imm8(0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_push(1 << 0, false); // save dst
        asm.emit_movs_reg(1, 4);
        asm.emit_movs_reg(2, 6);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_ldr_sp(0, 0);
        asm.emit_adds_reg(0, 0, 6);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_reg(2, 7);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 0, false); // restore dst
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_c_return_ptr` / `__omni_c_return_str` / `__omni_c_get_returned_ptr` / `__omni_c_get_returned_str`
    for setter in ["__omni_c_return_ptr", "__omni_c_return_str"] {
        asm.begin_function(setter);
        asm.emit_load_u32(2, chip.sram_base);
        asm.emit_str_imm(0, 2, SRAM_G_RET_PTR_OFF);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }
    for getter in ["__omni_c_get_returned_ptr", "__omni_c_get_returned_str"] {
        asm.begin_function(getter);
        asm.emit_load_u32(2, chip.sram_base);
        asm.emit_ldr_imm(0, 2, SRAM_G_RET_PTR_OFF);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // -------------------------------------------------------------------------
    // 4. 64-bit Integer Math Runtime (`(r0:r1) op (r2:r3) -> (r0:r1)`)
    // -------------------------------------------------------------------------
    emit_math64_helpers(asm, base_vma)?;

    // `__omni_int_to_str(i64 val = r0:r1) -> i8* r0` (full 64-bit signed decimal!)
    asm.begin_function("__omni_int_to_str");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(32); // [sp + 0..28] scratch reverse digits buffer
        asm.emit_movs_reg(4, 0); // lo
        asm.emit_movs_reg(5, 1); // hi
        asm.emit_movs_imm8(6, 0); // is_neg = 0

        // Check sign (`hi < 0`)
        asm.emit_cmp_imm8(5, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_movs_imm8(6, 1);
        // 64-bit negate `r4:r5 = 0 - r4:r5`
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_subs_reg(4, 0, 4);
        asm.emit_sbcs(1, 5);
        asm.emit_movs_reg(5, 1);
        asm.bind_label(non_neg);

        asm.emit_movs_imm8(7, 0); // digit_count = 0
        let dig_loop = asm.new_label();
        asm.bind_label(dig_loop);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_imm8(2, 10);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_udivmod64"); // quot -> r0:r1, rem -> r2:r3
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_adds_imm8(2, b'0');
        asm.emit_add_reg_sp(3, 0);
        asm.emit_adds_reg(3, 3, 7);
        asm.emit_strb_imm(2, 3, 0);
        asm.emit_adds_imm8(7, 1);

        asm.emit_movs_reg(0, 4);
        asm.emit_orrs(0, 5);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Ne, dig_loop);

        asm.emit_cmp_imm8(6, 0);
        let no_minus = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, no_minus);
        asm.emit_movs_imm8(2, b'-');
        asm.emit_add_reg_sp(3, 0);
        asm.emit_adds_reg(3, 3, 7);
        asm.emit_strb_imm(2, 3, 0);
        asm.emit_adds_imm8(7, 1);
        asm.bind_label(no_minus);

        // Allocate 24-byte output string and reverse `[sp .. sp + r7)` into it
        asm.emit_movs_imm8(0, 24);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc"); // r0 = out_buf
        asm.emit_movs_reg(4, 0);   // save out_buf
        asm.emit_movs_reg(5, 0);   // write_cursor
        let rev_loop = asm.new_label();
        let rev_done = asm.new_label();
        asm.bind_label(rev_loop);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, rev_done);
        asm.emit_subs_imm8(7, 1);
        asm.emit_add_reg_sp(3, 0);
        asm.emit_adds_reg(3, 3, 7);
        asm.emit_ldrb_imm(2, 3, 0);
        asm.emit_strb_imm(2, 5, 0);
        asm.emit_adds_imm8(5, 1);
        asm.emit_b_label_short(rev_loop);
        asm.bind_label(rev_done);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 5, 0);

        asm.emit_movs_reg(0, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_add_sp(32);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_c_str_to_i64` / `atoi` / `atol` (`s` = `r0`) -> `r0:r1`
    for parse_int_fn in ["__omni_c_str_to_i64", "atoi", "atol"] {
        asm.begin_function(parse_int_fn);
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.emit_movs_reg(4, 0); // ptr
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_cmp_imm8(4, 0);
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(6, 0); // neg
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'-');
        let not_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_neg);
        asm.emit_movs_imm8(6, 1);
        asm.emit_adds_imm8(4, 1);
        asm.bind_label(not_neg);

        let lp = asm.new_label();
        let lp_end = asm.new_label();
        asm.bind_label(lp);
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'0');
        asm.emit_bcond_label_short(Cond::Cc, lp_end);
        asm.emit_cmp_imm8(5, b'9' + 1);
        asm.emit_bcond_label_short(Cond::Cs, lp_end);
        asm.emit_subs_imm8(5, b'0');
        asm.emit_movs_imm8(2, 10);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_mul64");
        asm.emit_adds_reg(0, 0, 5);
        asm.emit_movs_imm8(2, 0);
        asm.emit_adcs(1, 2);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(lp_end);

        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(2, 0);
        asm.emit_movs_imm8(3, 0);
        asm.emit_subs_reg(0, 2, 0);
        asm.emit_sbcs(3, 1);
        asm.emit_movs_reg(1, 3);
        asm.bind_label(done);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.end_function(base_vma)?;
    }

    // -------------------------------------------------------------------------
    // 5. UART0 Serial Output (`uart_putc`, `uart_puts`, `__omni_print_*`, `printf`)
    // -------------------------------------------------------------------------
    for putc_name in ["uart_putc", "putchar"] {
        asm.begin_function(putc_name);
        asm.emit_load_u32(2, chip.uart0_base);
        asm.emit_movs_imm8(3, 0x20); // UARTFR_TXFF (bit 5)
        let wait = asm.new_label();
        asm.bind_label(wait);
        asm.emit_ldr_imm(1, 2, 0x18); // UARTFR
        asm.emit_tst_reg(1, 3);
        asm.emit_bcond_label_short(Cond::Ne, wait);
        asm.emit_uxtb(0, 0);
        asm.emit_str_imm(0, 2, 0x00); // UARTDR
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    for puts_inline in [
        "uart_puts",
        "__omni_print_str_inline",
        "__omni_print_str_raw",
    ] {
        asm.begin_function(puts_inline);
        asm.emit_push(1 << 4, true);
        asm.emit_movs_reg(4, 0);
        asm.emit_cmp_imm8(4, 0);
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, done);
        let lp = asm.new_label();
        asm.bind_label(lp);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_bl_sym("uart_putc");
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(1 << 4, true);
        asm.end_function(base_vma)?;
    }

    for puts_nl in ["__omni_print_str", "puts"] {
        asm.begin_function(puts_nl);
        asm.emit_push(0, true);
        asm.emit_bl_sym("uart_puts");
        asm.emit_movs_imm8(0, b'\n');
        asm.emit_bl_sym("uart_putc");
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // `__omni_print_int(i64 val = r0:r1)`
    asm.begin_function("__omni_print_int");
    {
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_int_to_str");
        asm.emit_bl_sym("__omni_print_str");
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `printf(i8* fmt = r0, ...)` -> freestanding UART0 formatter for `%s`, `%d`, `%ld`, `%lld`, `%u`, `%%`
    asm.begin_function("printf");
    {
        // Push r1, r2, r3 right below caller stack args so variadic cursor walks `r1, r2, r3, caller_sp...` seamlessly!
        asm.emit_push((1 << 1) | (1 << 2) | (1 << 3), false);
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        // At this point: `[sp + 16]` = pushed `r1`, `[sp + 20]` = `r2`, `[sp + 24]` = `r3`, `[sp + 28]` = `caller_sp[0]`!
        asm.emit_movs_reg(4, 0);      // fmt cursor
        asm.emit_add_reg_sp(5, 16);   // vararg byte cursor
        asm.emit_cmp_imm8(4, 0);
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, done);

        let lp = asm.new_label();
        let fmt_spec = asm.new_label();
        asm.bind_label(lp);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_adds_imm8(4, 1);
        asm.emit_cmp_imm8(0, b'%');
        asm.emit_bcond_label_short(Cond::Eq, fmt_spec);
        asm.emit_bl_sym("uart_putc");
        asm.emit_b_label_short(lp);

        asm.bind_label(fmt_spec);
        asm.emit_movs_imm8(6, 0); // is_64 = 0
        let spec_chk = asm.new_label();
        asm.bind_label(spec_chk);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_adds_imm8(4, 1);
        asm.emit_cmp_imm8(0, b'l');
        let not_l = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_l);
        asm.emit_adds_imm8(6, 1);
        asm.emit_b_label_short(spec_chk);
        asm.bind_label(not_l);

        asm.emit_cmp_imm8(0, b'%');
        let not_pct = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_pct);
        asm.emit_bl_sym("uart_putc");
        asm.emit_b_label_short(lp);
        asm.bind_label(not_pct);

        asm.emit_cmp_imm8(0, b's');
        let not_s = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_s);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_adds_imm8(5, 4);
        asm.emit_bl_sym("uart_puts");
        asm.emit_b_label_short(lp);
        asm.bind_label(not_s);

        // Integer specifiers (`%d`, `%u`, `%lld`, `%ld`):
        // Note: in `omni_codegen`, `printf` args may be passed as `i64` (`%lld` or raw i64) or `i32`.
        asm.emit_cmp_imm8(6, 0);
        let i32_arg = asm.new_label();
        let print_num = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, i32_arg);
        // 8-byte AAPCS align vararg cursor relative to `sp + 12` (where `r0` would have been)
        // `r1` was at `sp+16` (word 1), `r2` at `sp+20` (word 2, even!). So if `(r5 & 4) != 0`?
        // Note: `sp` after pushing 3 regs (12B) + 4 regs (16B) = `entry_sp - 28`.
        // `entry_sp` is 8-byte aligned! So `r2` is at `entry_sp - 8` (8-byte aligned!).
        // Thus 8-byte AAPCS alignment is simply `r5 = (r5 + 7) & !7`!
        asm.emit_adds_imm8(5, 7);
        asm.emit_movs_imm8(2, 7);
        asm.emit_bics(5, 2);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_ldr_imm(1, 5, 4);
        asm.emit_adds_imm8(5, 8);
        asm.emit_b_label_short(print_num);

        asm.bind_label(i32_arg);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_adds_imm8(5, 4);
        asm.emit_asrs_imm(1, 0, 31);

        asm.bind_label(print_num);
        asm.emit_bl_sym("__omni_int_to_str");
        asm.emit_bl_sym("uart_puts");
        asm.emit_b_label_short(lp);

        asm.bind_label(done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), false);
        asm.emit_pop(1 << 2, false); // pop saved lr into r2
        asm.emit_add_sp(12);         // discard pushed r1..r3
        asm.emit_bx(2);
    }
    asm.end_function(base_vma)?;

    // -------------------------------------------------------------------------
    // 6. Hardware Timing, MMIO, GPIO, and PIO Runtime Intrinsics
    // -------------------------------------------------------------------------
    emit_hardware_peripherals(asm, chip, base_vma)?;

    // -------------------------------------------------------------------------
    // 7. Freestanding Arena, Slices, and Maps in SRAM
    // -------------------------------------------------------------------------
    emit_collections_and_arenas(asm, chip, base_vma)?;

    Ok(())
}

fn emit_math64_helpers(asm: &mut ThumbAssembler, base_vma: u32) -> Result<(), String> {
    // `__omni_mul64(a=r0:r1, b=r2:r3) -> r0:r1` (early-exit shift-and-add, 100% ARMv6-M & ARMv8-M clean)
    asm.begin_function("__omni_mul64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_movs_reg(4, 0); // a_lo
        asm.emit_movs_reg(5, 1); // a_hi
        asm.emit_movs_imm8(0, 0); // acc_lo
        asm.emit_movs_imm8(1, 0); // acc_hi
        asm.emit_movs_imm8(6, 1);

        let lp = asm.new_label();
        let skip_add = asm.new_label();
        let done = asm.new_label();
        asm.bind_label(lp);
        asm.emit_movs_reg(7, 2);
        asm.emit_orrs(7, 3);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);

        asm.emit_tst_reg(2, 6);
        asm.emit_bcond_label_short(Cond::Eq, skip_add);
        asm.emit_adds_reg(0, 0, 4);
        asm.emit_adcs(1, 5);
        asm.bind_label(skip_add);

        // `a <<= 1`
        asm.emit_adds_reg(4, 4, 4);
        asm.emit_adcs(5, 5);

        // `b >>= 1` (`r2 = (r2 >> 1) | (r3 << 31); r3 >>= 1`)
        asm.emit_lsrs_imm(2, 2, 1);
        asm.emit_lsls_imm(7, 3, 31);
        asm.emit_orrs(2, 7);
        asm.emit_lsrs_imm(3, 3, 1);
        asm.emit_b_label_short(lp);

        asm.bind_label(done);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_udivmod64(num=r0:r1, den=r2:r3) -> quot=r0:r1, rem=r2:r3`
    asm.begin_function("__omni_udivmod64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_sub_sp(16);
        // Check `den == 0`
        asm.emit_movs_reg(4, 2);
        asm.emit_orrs(4, 3);
        let nz = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, nz);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(nz);
        // [sp+0]=den_lo, [sp+4]=den_hi, [sp+8]=quot_lo, [sp+12]=quot_hi
        asm.emit_str_sp(2, 0);
        asm.emit_str_sp(3, 4);
        asm.emit_movs_imm8(4, 0);
        asm.emit_str_sp(4, 8);
        asm.emit_str_sp(4, 12);
        // r0:r1 = num (shifted out MSB-first), r4:r5 = rem, r6 = loop count (64)
        asm.emit_movs_imm8(5, 0);
        asm.emit_movs_imm8(6, 64);

        let bit_lp = asm.new_label();
        let no_sub = asm.new_label();
        asm.bind_label(bit_lp);
        // `carry_bit = r1 >> 31`
        asm.emit_lsrs_imm(7, 1, 31);
        // `num <<= 1`
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        // `rem = (rem << 1) | carry_bit`
        asm.emit_adds_reg(4, 4, 4);
        asm.emit_adcs(5, 5);
        asm.emit_orrs(4, 7);

        // `quot <<= 1`
        asm.emit_ldr_sp(2, 8);
        asm.emit_ldr_sp(3, 12);
        asm.emit_adds_reg(2, 2, 2);
        asm.emit_adcs(3, 3);
        asm.emit_str_sp(2, 8);
        asm.emit_str_sp(3, 12);

        // Compare `rem (r4:r5) >= den` using trial 64-bit subtraction `t_lo:t_hi = rem - den`
        asm.emit_ldr_sp(2, 0);
        asm.emit_ldr_sp(3, 4);
        asm.emit_subs_reg(2, 4, 2); // r2 = rem_lo - den_lo
        asm.emit_movs_reg(7, 5);
        asm.emit_sbcs(7, 3);        // r7 = rem_hi - den_hi - borrow; CS == 1 iff rem >= den!
        asm.emit_bcond_label_short(Cond::Cc, no_sub);
        asm.emit_movs_reg(4, 2);
        asm.emit_movs_reg(5, 7);
        asm.emit_ldr_sp(2, 8);
        asm.emit_adds_imm8(2, 1);
        asm.emit_str_sp(2, 8);
        asm.bind_label(no_sub);

        asm.emit_subs_imm8(6, 1);
        asm.emit_bcond_label_short(Cond::Ne, bit_lp);

        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_udiv64` / `__omni_urem64`
    asm.begin_function("__omni_udiv64");
    asm.emit_push(0, true);
    asm.emit_bl_sym("__omni_udivmod64");
    asm.emit_pop(0, true);
    asm.end_function(base_vma)?;

    asm.begin_function("__omni_urem64");
    asm.emit_push(0, true);
    asm.emit_bl_sym("__omni_udivmod64");
    asm.emit_movs_reg(0, 2);
    asm.emit_movs_reg(1, 3);
    asm.emit_pop(0, true);
    asm.end_function(base_vma)?;

    // `__omni_sdiv64` / `__omni_srem64`
    for (fn_name, want_rem) in [("__omni_sdiv64", false), ("__omni_srem64", true)] {
        asm.begin_function(fn_name);
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_lsrs_imm(4, 1, 31); // sign_num (0 or 1)
        asm.emit_lsrs_imm(5, 3, 31); // sign_den (0 or 1)
        if !want_rem {
            asm.emit_eors(4, 5); // sign_quot = sign_num ^ sign_den
        }

        // abs(num)
        asm.emit_cmp_imm8(1, 0);
        let num_pos = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, num_pos);
        asm.emit_push(1 << 2, false);
        asm.emit_movs_imm8(2, 0);
        asm.emit_subs_reg(0, 2, 0);
        asm.emit_sbcs(2, 1);
        asm.emit_movs_reg(1, 2);
        asm.emit_pop(1 << 2, false);
        asm.bind_label(num_pos);

        // abs(den)
        asm.emit_cmp_imm8(3, 0);
        let den_pos = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, den_pos);
        asm.emit_movs_imm8(5, 0);
        asm.emit_subs_reg(2, 5, 2);
        asm.emit_sbcs(5, 3);
        asm.emit_movs_reg(3, 5);
        asm.bind_label(den_pos);

        asm.emit_bl_sym("__omni_udivmod64");
        if want_rem {
            asm.emit_movs_reg(0, 2);
            asm.emit_movs_reg(1, 3);
        }
        asm.emit_cmp_imm8(4, 0);
        let res_pos = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, res_pos);
        asm.emit_movs_imm8(2, 0);
        asm.emit_subs_reg(0, 2, 0);
        asm.emit_sbcs(2, 1);
        asm.emit_movs_reg(1, 2);
        asm.bind_label(res_pos);
        asm.emit_pop((1 << 4) | (1 << 5), true);
        asm.end_function(base_vma)?;
    }

    // 64-bit shifts: `__omni_shl64`, `__omni_lshr64`, `__omni_ashr64` (`val=r0:r1`, `cnt=r2`)
    asm.begin_function("__omni_shl64");
    {
        asm.emit_movs_imm8(3, 63);
        asm.emit_ands(2, 3);
        let lp = asm.new_label();
        let dn = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, dn);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(dn);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    for (shr_name, arith) in [("__omni_lshr64", false), ("__omni_ashr64", true)] {
        asm.begin_function(shr_name);
        asm.emit_movs_imm8(3, 63);
        asm.emit_ands(2, 3);
        let lp = asm.new_label();
        let dn = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, dn);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(3, 1, 31);
        asm.emit_orrs(0, 3);
        if arith {
            asm.emit_asrs_imm(1, 1, 1);
        } else {
            asm.emit_lsrs_imm(1, 1, 1);
        }
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(dn);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // `__omni_ipow64(base=r0:r1, exp=r2:r3) -> r0:r1`
    asm.begin_function("__omni_ipow64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_movs_reg(4, 0); // base_lo
        asm.emit_movs_reg(5, 1); // base_hi
        asm.emit_movs_reg(6, 2); // exp_lo
        asm.emit_movs_imm8(0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_cmp_imm8(3, 0);
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Mi, done); // exp < 0 -> 0/1
        let lp = asm.new_label();
        let skip = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(7, 1);
        asm.emit_tst_reg(6, 7);
        asm.emit_bcond_label_short(Cond::Eq, skip);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_mul64");
        asm.bind_label(skip);
        asm.emit_push((1 << 0) | (1 << 1), false);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_mul64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_pop((1 << 0) | (1 << 1), false);
        asm.emit_lsrs_imm(6, 6, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(done);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    Ok(())
}

fn emit_hardware_peripherals(
    asm: &mut ThumbAssembler,
    chip: &RpChipProfile,
    base_vma: u32,
) -> Result<(), String> {
    // `mmio_read32(i64 addr = r0:r1) -> i64`
    asm.begin_function("mmio_read32");
    asm.emit_ldr_imm(0, 0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `mmio_write32(i64 addr = r0:r1, i64 val = r2:r3) -> i64`
    asm.begin_function("mmio_write32");
    asm.emit_str_imm(2, 0, 0);
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `time_us` / `time_us_64` -> `r0 = TIMERAWL, r1 = TIMERAWH`
    for tf in ["time_us", "time_us_64"] {
        asm.begin_function(tf);
        asm.emit_load_u32(2, chip.timer_base);
        asm.emit_ldr_imm(0, 2, 0x28); // TIMERAWL
        asm.emit_ldr_imm(1, 2, 0x24); // TIMERAWH
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // `sleep_us` / `usleep` (`r0` = microseconds)
    for us_fn in ["sleep_us", "usleep"] {
        asm.begin_function(us_fn);
        asm.emit_load_u32(2, chip.timer_base);
        asm.emit_ldr_imm(3, 2, 0x28); // start = TIMERAWL
        let wait = asm.new_label();
        asm.bind_label(wait);
        asm.emit_ldr_imm(1, 2, 0x28); // now = TIMERAWL
        asm.emit_subs_reg(1, 1, 3);   // elapsed = now - start
        asm.emit_cmp_reg(1, 0);
        asm.emit_bcond_label_short(Cond::Cc, wait);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // `sleep_ms` / `__omni_sleep_ms` (`r0` = ms)
    for ms_fn in ["sleep_ms", "__omni_sleep_ms"] {
        asm.begin_function(ms_fn);
        asm.emit_load_u32(2, 1000);
        asm.emit_muls(0, 2);
        asm.emit_push(0, true);
        asm.emit_bl_sym("sleep_us");
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // `__omni_sleep_s` / `sleep` (`r0` = seconds)
    for s_fn in ["__omni_sleep_s", "sleep"] {
        asm.begin_function(s_fn);
        asm.emit_load_u32(2, 1_000_000);
        asm.emit_muls(0, 2);
        asm.emit_push(0, true);
        asm.emit_bl_sym("sleep_us");
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // `gpio_init(i64 pin = r0:r1)`
    asm.begin_function("gpio_init");
    {
        asm.emit_movs_imm8(3, 0x1F);
        asm.emit_ands(0, 3); // pin &= 31
        // 1) Clear SIO OE and OUT for `1 << pin`
        asm.emit_movs_imm8(1, 1);
        asm.emit_lsls_reg(1, 0);
        asm.emit_load_u32(2, chip.sio_base);
        asm.emit_str_imm(1, 2, chip.sio_gpio_oe_clr);
        asm.emit_str_imm(1, 2, chip.sio_gpio_out_clr);

        // 2) Configure `PADS_BANK0 + 0x04 + pin * 4` = `0x56` (clears bit 8 `ISO` on RP2350!)
        asm.emit_load_u32(2, chip.pads_bank0_base + 0x04);
        asm.emit_lsls_imm(3, 0, 2);
        asm.emit_movs_imm8(1, 0x56);
        asm.emit_str_reg(1, 2, 3);

        // 3) Mux `IO_BANK0 + 0x04 + pin * 8` = `5` (`FUNCSEL_SIO`)
        asm.emit_load_u32(2, chip.io_bank0_base + 0x04);
        asm.emit_lsls_imm(3, 0, 3);
        asm.emit_movs_imm8(1, 5);
        asm.emit_str_reg(1, 2, 3);

        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `gpio_set_dir(i64 pin = r0:r1, i64 out = r2:r3)`
    asm.begin_function("gpio_set_dir");
    {
        asm.emit_movs_imm8(1, 1);
        asm.emit_lsls_reg(1, 0);
        asm.emit_load_u32(3, chip.sio_base);
        asm.emit_cmp_imm8(2, 0);
        let is_in = asm.new_label();
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, is_in);
        asm.emit_str_imm(1, 3, chip.sio_gpio_oe_set);
        asm.emit_b_label_short(done);
        asm.bind_label(is_in);
        asm.emit_str_imm(1, 3, chip.sio_gpio_oe_clr);
        asm.bind_label(done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `gpio_put(i64 pin = r0:r1, i64 val = r2:r3)`
    asm.begin_function("gpio_put");
    {
        asm.emit_movs_imm8(1, 1);
        asm.emit_lsls_reg(1, 0);
        asm.emit_load_u32(3, chip.sio_base);
        asm.emit_cmp_imm8(2, 0);
        let clr = asm.new_label();
        let done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, clr);
        asm.emit_str_imm(1, 3, chip.sio_gpio_out_set);
        asm.emit_b_label_short(done);
        asm.bind_label(clr);
        asm.emit_str_imm(1, 3, chip.sio_gpio_out_clr);
        asm.bind_label(done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `gpio_toggle(i64 pin = r0:r1)`
    asm.begin_function("gpio_toggle");
    {
        asm.emit_movs_imm8(1, 1);
        asm.emit_lsls_reg(1, 0);
        asm.emit_load_u32(3, chip.sio_base);
        asm.emit_str_imm(1, 3, chip.sio_gpio_out_xor);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `gpio_get(i64 pin = r0:r1) -> i64`
    asm.begin_function("gpio_get");
    {
        asm.emit_load_u32(3, chip.sio_base);
        asm.emit_ldr_imm(1, 3, chip.sio_gpio_in);
        asm.emit_lsrs_reg(1, 0);
        asm.emit_movs_imm8(0, 1);
        asm.emit_ands(0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // Internal helper `__omni_pio_base(pio_idx = r0) -> r0 = pio_mmio_base` (preserves r2, r3)
    asm.begin_function("__omni_pio_base");
    {
        asm.emit_cmp_imm8(0, 1);
        let not_1 = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_1);
        asm.emit_load_u32(0, chip.pio1_base);
        asm.emit_bx(14);
        asm.bind_label(not_1);
        asm.emit_cmp_imm8(0, 2);
        let not_2 = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, not_2);
        asm.emit_load_u32(0, chip.pio2_base);
        asm.emit_bx(14);
        asm.bind_label(not_2);
        asm.emit_load_u32(0, chip.pio0_base);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `pio_instr_write(pio_idx=r0:r1, slot=r2:r3, instr=[entry_sp+0])`
    asm.begin_function("pio_instr_write");
    {
        asm.emit_ldr_sp(3, 0); // instr (read before push!)
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_pio_base"); // r0 = pio_base
        asm.emit_movs_imm8(1, 0x1F);
        asm.emit_ands(2, 1);
        asm.emit_lsls_imm(2, 2, 2);
        asm.emit_adds_imm8(2, 0x48); // INSTR_MEM0 offset
        asm.emit_uxth(3, 3);
        asm.emit_str_reg(3, 0, 2);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `pio_sm_config_and_enable(pio_idx=r0, sm=r2, base_offset=[entry_sp+0], cfg_pack=[entry_sp+8], pin=[entry_sp+16], clkdiv_int=[entry_sp+24])`
    asm.begin_function("pio_sm_config_and_enable");
    {
        // Save r4..r7 + lr (5 words = 20 bytes -> entry_sp args shift by +20!)
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_movs_reg(4, 0); // pio_idx (0, 1, 2)
        asm.emit_movs_imm8(1, 3);
        asm.emit_ands(2, 1);
        asm.emit_movs_reg(5, 2); // sm (0..3)

        // 1) Configure Pad & GPIO `FUNCSEL = 6 + pio_idx` (`PIO0=6, PIO1=7, PIO2=8`)
        asm.emit_ldr_sp(6, 20 + 16); // pin
        asm.emit_movs_imm8(1, 0x1F);
        asm.emit_ands(6, 1);

        asm.emit_load_u32(0, chip.pads_bank0_base + 0x04);
        asm.emit_lsls_imm(1, 6, 2);
        asm.emit_movs_imm8(2, 0x56); // clears RP2350 ISO bit 8!
        asm.emit_str_reg(2, 0, 1);

        asm.emit_load_u32(0, chip.io_bank0_base + 0x04);
        asm.emit_lsls_imm(1, 6, 3);
        asm.emit_movs_reg(2, 4);
        asm.emit_adds_imm8(2, 6);    // 6 + pio_idx
        asm.emit_str_reg(2, 0, 1);

        // 2) Compute `r4 = pio_base`, `r7 = sm_base = pio_base + 0xC8 + sm * 0x18`
        asm.emit_movs_reg(0, 4);
        asm.emit_bl_sym("__omni_pio_base");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_imm8(1, 0x18);
        asm.emit_muls(1, 5);
        asm.emit_adds_imm8(1, 0xC8);
        asm.emit_adds_reg(7, 4, 1); // r7 = sm_base

        // 3) `SM_CLKDIV` (`+0x00`) = `max(clkdiv_int, 1) << 16`
        asm.emit_ldr_sp(0, 20 + 24);
        asm.emit_cmp_imm8(0, 0);
        let div_ok = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, div_ok);
        asm.emit_movs_imm8(0, 1);
        asm.bind_label(div_ok);
        asm.emit_lsls_imm(0, 0, 16);
        asm.emit_str_imm(0, 7, 0x00);

        // 4) Unpack `cfg_pack` (`[sp + 20 + 8]`) & `base_offset` (`[sp + 20 + 0]`) for `SM_EXECCTRL` (`+0x04`)
        asm.emit_ldr_sp(0, 20 + 0); // base_offset
        asm.emit_ldr_sp(1, 20 + 8); // cfg_pack
        asm.emit_movs_imm8(3, 0x1F);

        // wrap_bottom = (base_offset + (cfg_pack & 0x1F)) << 7
        asm.emit_movs_reg(2, 1);
        asm.emit_ands(2, 3);
        asm.emit_adds_reg(2, 2, 0);
        asm.emit_lsls_imm(2, 2, 7);

        // wrap_top = (base_offset + ((cfg_pack >> 5) & 0x1F)) << 12
        asm.emit_lsrs_imm(3, 1, 5);
        asm.emit_push(1 << 0, false); // save base_offset (+4 bytes on stack!)
        asm.emit_movs_imm8(0, 0x1F);
        asm.emit_ands(3, 0);
        asm.emit_ldr_sp(0, 0);
        asm.emit_adds_reg(3, 3, 0);
        asm.emit_lsls_imm(3, 3, 12);
        asm.emit_orrs(2, 3);

        // sideset_opt (`bit 13` -> `SIDE_EN bit 30`), sideset_pindirs (`bit 14` -> `SIDE_PINDIR bit 29`)
        asm.emit_lsrs_imm(3, 1, 13);
        asm.emit_movs_imm8(0, 1);
        asm.emit_ands(3, 0);
        asm.emit_lsls_imm(3, 3, 30);
        asm.emit_orrs(2, 3);

        asm.emit_lsrs_imm(3, 1, 14);
        asm.emit_ands(3, 0);
        asm.emit_lsls_imm(3, 3, 29);
        asm.emit_orrs(2, 3);
        asm.emit_str_imm(2, 7, 0x04); // SM_EXECCTRL

        // 5) `SM_SHIFTCTRL` (`+0x08`) = `0x0006_0000` (IN_SHIFTDIR=1 | AUTOPULL=1 | OUT_SHIFTDIR=0 MSB-first)
        asm.emit_load_u32(2, 0x0006_0000);
        asm.emit_str_imm(2, 7, 0x08);

        // 6) `SM_PINCTRL` (`+0x14`):
        // total_sideset = ((cfg_pack >> 10) & 7) + ((cfg_pack >> 13) & 1)
        asm.emit_lsrs_imm(2, 1, 10);
        asm.emit_movs_imm8(3, 7);
        asm.emit_ands(2, 3);
        asm.emit_lsrs_imm(3, 1, 13);
        asm.emit_ands(3, 0); // r0 is still 1
        asm.emit_adds_reg(2, 2, 3);
        asm.emit_lsls_imm(2, 2, 29); // SIDESET_COUNT [31:29]
        asm.emit_load_u32(3, (1 << 26) | (1 << 20)); // SET_COUNT=1, OUT_COUNT=1
        asm.emit_orrs(2, 3);
        // OUT_BASE=pin, SET_BASE=pin<<5, SIDESET_BASE=pin<<10 (r6 = pin)
        asm.emit_orrs(2, 6);
        asm.emit_lsls_imm(3, 6, 5);
        asm.emit_orrs(2, 3);
        asm.emit_lsls_imm(3, 6, 10);
        asm.emit_orrs(2, 3);
        asm.emit_str_imm(2, 7, 0x14); // SM_PINCTRL

        // 7) Force `SET PINDIRS, 1` (`0xE081`) and `JMP base_offset` via `SM_INSTR` (`+0x10`)
        asm.emit_load_u32(2, 0xE081);
        asm.emit_str_imm(2, 7, 0x10);
        asm.emit_pop(1 << 0, false); // restore base_offset into r0
        asm.emit_movs_imm8(2, 0x1F);
        asm.emit_ands(0, 2);         // JMP base_offset opcode is `0x0000 | (base_offset & 0x1F)`!
        asm.emit_str_imm(0, 7, 0x10);

        // 8) Enable SM in `PIO_CTRL` (`[pio_base + 0x00] |= (1 << sm) | (1 << (8 + sm))`)
        asm.emit_ldr_imm(2, 4, 0x00);
        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_reg(3, 5);
        asm.emit_orrs(2, 3);
        asm.emit_lsls_imm(3, 3, 8); // CLKDIV_RESTART
        asm.emit_orrs(2, 3);
        asm.emit_str_imm(2, 4, 0x00);

        asm.emit_movs_reg(0, 5);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `pio_sm_put(pio_idx=r0, sm=r2, data=[entry_sp+0])`
    asm.begin_function("pio_sm_put");
    {
        asm.emit_ldr_sp(3, 0); // data
        asm.emit_push(1 << 4, true);
        asm.emit_movs_reg(4, 3);
        asm.emit_bl_sym("__omni_pio_base"); // r0 = pio_base
        asm.emit_movs_imm8(1, 3);
        asm.emit_ands(2, 1);
        // Poll `FSTAT` (`+0x04`) `TXFULL` bit `16 + sm` == 0
        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_imm(3, 3, 16);
        asm.emit_lsls_reg(3, 2);
        let wait = asm.new_label();
        asm.bind_label(wait);
        asm.emit_ldr_imm(1, 0, 0x04);
        asm.emit_tst_reg(1, 3);
        asm.emit_bcond_label_short(Cond::Ne, wait);

        // Write `data` to `TXF0 + sm * 4` (`0x10 + sm * 4`)
        asm.emit_lsls_imm(2, 2, 2);
        asm.emit_adds_imm8(2, 0x10);
        asm.emit_str_reg(4, 0, 2);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(1 << 4, true);
    }
    asm.end_function(base_vma)?;

    // `pio_sm_get(pio_idx=r0, sm=r2) -> i64`
    asm.begin_function("pio_sm_get");
    {
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_pio_base");
        asm.emit_movs_imm8(1, 3);
        asm.emit_ands(2, 1);
        // Poll `FSTAT` (`+0x04`) `RXEMPTY` bit `8 + sm` == 0
        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_imm(3, 3, 8);
        asm.emit_lsls_reg(3, 2);
        let wait = asm.new_label();
        asm.bind_label(wait);
        asm.emit_ldr_imm(1, 0, 0x04);
        asm.emit_tst_reg(1, 3);
        asm.emit_bcond_label_short(Cond::Ne, wait);

        asm.emit_lsls_imm(2, 2, 2);
        asm.emit_adds_imm8(2, 0x20); // RXF0
        asm.emit_ldr_reg(0, 0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `pio_sm_exec(pio_idx=r0, sm=r2, instr=[entry_sp+0])`
    asm.begin_function("pio_sm_exec");
    {
        asm.emit_ldr_sp(3, 0);
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_pio_base");
        asm.emit_movs_imm8(1, 3);
        asm.emit_ands(2, 1);
        asm.emit_movs_imm8(1, 0x18);
        asm.emit_muls(1, 2);
        asm.emit_adds_imm8(1, 0xD8); // 0xC8 + 0x10 = SM0_INSTR
        asm.emit_uxth(3, 3);
        asm.emit_str_reg(3, 0, 1);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    Ok(())
}

fn emit_collections_and_arenas(
    asm: &mut ThumbAssembler,
    chip: &RpChipProfile,
    base_vma: u32,
) -> Result<(), String> {
    // O(1) SRAM Scope Arena (`__omni_arena_*`)
    asm.begin_function("__omni_arena_create");
    asm.emit_load_u32(0, chip.sram_base);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_arena_alloc(i8* arena=r0, i64 size=r2:r3, i64 align=[sp+0]) -> i8*`
    asm.begin_function("__omni_arena_alloc");
    asm.emit_movs_reg(0, 2);
    asm.emit_movs_imm8(1, 0);
    asm.emit_push(0, true);
    asm.emit_bl_sym("malloc");
    asm.emit_pop(0, true);
    asm.end_function(base_vma)?;

    // `__omni_arena_mark(i8* arena=r0) -> i64`
    asm.begin_function("__omni_arena_mark");
    asm.emit_load_u32(2, chip.sram_base);
    asm.emit_ldr_imm(0, 2, SRAM_G_HEAP_PTR_OFF);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_arena_rewind(i8* arena=r0, i64 mark=r2:r3)`
    asm.begin_function("__omni_arena_rewind");
    asm.emit_load_u32(0, chip.sram_base);
    asm.emit_str_imm(2, 0, SRAM_G_HEAP_PTR_OFF);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    for arena_noop in ["__omni_arena_reset", "__omni_arena_destroy"] {
        asm.begin_function(arena_noop);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // --- Slices (`[data_ptr: u32, len: u32, cap: u32, pad: u32]`) ---
    asm.begin_function("__omni_slice_create");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_cmp_imm8(0, 4);
        let cap_ok = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, cap_ok);
        asm.emit_movs_imm8(0, 4);
        asm.bind_label(cap_ok);
        asm.emit_movs_reg(4, 0); // cap
        asm.emit_movs_imm8(0, 16);
        asm.emit_bl_sym("malloc");
        asm.emit_movs_reg(5, 0); // hdr
        asm.emit_lsls_imm(0, 4, 3); // cap * 8
        asm.emit_bl_sym("malloc");
        asm.emit_str_imm(0, 5, 0); // data_ptr
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 5, 4); // len = 0
        asm.emit_str_imm(4, 5, 8); // cap
        asm.emit_movs_reg(0, 5);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_append(i8* s=r0, i64 val=r2:r3)`
    asm.begin_function("__omni_slice_append");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_movs_reg(4, 0); // s
        asm.emit_movs_reg(5, 2); // val_lo
        asm.emit_movs_reg(6, 3); // val_hi
        asm.emit_ldr_imm(1, 4, 4); // len
        asm.emit_ldr_imm(2, 4, 8); // cap
        asm.emit_cmp_reg(1, 2);
        let has_room = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cc, has_room);
        // Grow `new_cap = cap * 2`
        asm.emit_lsls_imm(7, 2, 1);
        asm.emit_lsls_imm(0, 7, 3);
        asm.emit_bl_sym("malloc");
        asm.emit_push(1 << 0, false); // new_data
        asm.emit_ldr_imm(1, 4, 0);    // old_data
        asm.emit_ldr_imm(2, 4, 4);
        asm.emit_lsls_imm(2, 2, 3);   // len * 8
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 0, false);
        asm.emit_str_imm(0, 4, 0);
        asm.emit_str_imm(7, 4, 8);

        asm.bind_label(has_room);
        asm.emit_ldr_imm(0, 4, 0); // data
        asm.emit_ldr_imm(1, 4, 4); // len
        asm.emit_lsls_imm(2, 1, 3);
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_str_imm(5, 0, 0);
        asm.emit_str_imm(6, 0, 4);
        asm.emit_adds_imm8(1, 1);
        asm.emit_str_imm(1, 4, 4);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_get(i8* s=r0, i64 idx=r2:r3) -> i64 r0:r1`
    asm.begin_function("__omni_slice_get");
    {
        asm.emit_ldr_imm(1, 0, 4); // len
        asm.emit_cmp_imm8(3, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(2, 2, 1);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(2, 1);
        let oob = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_ldr_imm(0, 0, 0); // data
        asm.emit_lsls_imm(2, 2, 3);
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_ldr_imm(1, 0, 4);
        asm.emit_ldr_imm(0, 0, 0);
        asm.emit_bx(14);
        asm.bind_label(oob);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_set(i8* s=r0, i64 idx=r2:r3, i64 val=[entry_sp+0..8])`
    asm.begin_function("__omni_slice_set");
    {
        asm.emit_ldr_imm(1, 0, 4); // len
        asm.emit_cmp_imm8(3, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(2, 2, 1);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(2, 1);
        let oob = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_ldr_imm(0, 0, 0); // data
        asm.emit_lsls_imm(2, 2, 3);
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_ldr_sp(1, 0);
        asm.emit_ldr_sp(3, 4);
        asm.emit_str_imm(1, 0, 0);
        asm.emit_str_imm(3, 0, 4);
        asm.bind_label(oob);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_len(i8* s=r0) -> i64`
    asm.begin_function("__omni_slice_len");
    asm.emit_ldr_imm(0, 0, 4);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_slice_pop(i8* s=r0) -> i64`
    asm.begin_function("__omni_slice_pop");
    {
        asm.emit_ldr_imm(1, 0, 4);
        asm.emit_cmp_imm8(1, 0);
        let empty = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, empty);
        asm.emit_subs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 4);
        asm.emit_ldr_imm(0, 0, 0);
        asm.emit_lsls_imm(1, 1, 3);
        asm.emit_adds_reg(0, 0, 1);
        asm.emit_ldr_imm(1, 0, 4);
        asm.emit_ldr_imm(0, 0, 0);
        asm.emit_bx(14);
        asm.bind_label(empty);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // --- Compact Linear Map (`[entries_ptr: u32, len: u32, cap: u32, 0]`, entry = 16B `[k_lo, k_hi, v_lo, v_hi]`) ---
    asm.begin_function("__omni_map_create");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_movs_imm8(4, 32); // 32 entries initial capacity
        asm.emit_movs_imm8(0, 16);
        asm.emit_bl_sym("malloc");
        asm.emit_movs_reg(5, 0);
        asm.emit_lsls_imm(0, 4, 4); // 32 * 16 = 512B
        asm.emit_bl_sym("malloc");
        asm.emit_str_imm(0, 5, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 5, 4);
        asm.emit_str_imm(4, 5, 8);
        asm.emit_movs_reg(0, 5);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_set(i8* m=r0, i8* key=r1, i64 val=r2:r3)`
    asm.begin_function("__omni_map_set");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_push((1 << 2) | (1 << 3), false); // save val_lo, val_hi at [sp+0], [sp+4]
        asm.emit_movs_reg(4, 0); // m
        asm.emit_movs_reg(5, 1); // key
        asm.emit_ldr_imm(6, 4, 0); // entries
        asm.emit_ldr_imm(7, 4, 4); // len
        let lp = asm.new_label();
        let append = asm.new_label();
        let store_slot = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, append);
        asm.emit_ldr_imm(0, 6, 0);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("strcmp");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, store_slot);
        asm.emit_adds_imm8(6, 16);
        asm.emit_subs_imm8(7, 1);
        asm.emit_b_label_short(lp);

        asm.bind_label(append);
        asm.emit_ldr_imm(7, 4, 4);
        asm.emit_ldr_imm(0, 4, 8);
        asm.emit_cmp_reg(7, 0);
        let has_cap = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cc, has_cap);
        // Grow map entries x2
        asm.emit_lsls_imm(0, 0, 1);
        asm.emit_str_imm(0, 4, 8);
        asm.emit_lsls_imm(0, 0, 4);
        asm.emit_bl_sym("malloc");
        asm.emit_push(1 << 0, false);
        asm.emit_ldr_imm(1, 4, 0);
        asm.emit_lsls_imm(2, 7, 4);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 0, false);
        asm.emit_str_imm(0, 4, 0);
        asm.emit_lsls_imm(1, 7, 4);
        asm.emit_adds_reg(6, 0, 1);
        asm.bind_label(has_cap);
        asm.emit_adds_imm8(7, 1);
        asm.emit_str_imm(7, 4, 4);
        asm.emit_str_imm(5, 6, 0);

        asm.bind_label(store_slot);
        asm.emit_pop((1 << 2) | (1 << 3), false);
        asm.emit_str_imm(2, 6, 8);
        asm.emit_str_imm(3, 6, 12);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_get(i8* m=r0, i8* key=r1) -> i64 r0:r1`
    asm.begin_function("__omni_map_get");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.emit_movs_reg(4, 1); // key
        asm.emit_ldr_imm(5, 0, 0); // entries
        asm.emit_ldr_imm(6, 0, 4); // len
        let lp = asm.new_label();
        let found = asm.new_label();
        let miss = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_movs_reg(1, 4);
        asm.emit_bl_sym("strcmp");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, found);
        asm.emit_adds_imm8(5, 16);
        asm.emit_subs_imm8(6, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(found);
        asm.emit_ldr_imm(0, 5, 8);
        asm.emit_ldr_imm(1, 5, 12);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.bind_label(miss);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_contains(i8* m=r0, i8* key=r1) -> i1 r0`
    asm.begin_function("__omni_map_contains");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.emit_movs_reg(4, 1);
        asm.emit_ldr_imm(5, 0, 0);
        asm.emit_ldr_imm(6, 0, 4);
        let lp = asm.new_label();
        let hit = asm.new_label();
        let miss = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_movs_reg(1, 4);
        asm.emit_bl_sym("strcmp");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, hit);
        asm.emit_adds_imm8(5, 16);
        asm.emit_subs_imm8(6, 1);
        asm.emit_b_label_short(lp);
        asm.bind_label(hit);
        asm.emit_movs_imm8(0, 1);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.bind_label(miss);
        asm.emit_movs_imm8(0, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
    }
    asm.end_function(base_vma)?;

    asm.begin_function("__omni_map_len");
    asm.emit_ldr_imm(0, 0, 4);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // Cooperative task launch (`__omni_go_spawn`, `__omni_thread_spawn`: `fn_ptr=r0, arg=r1`)
    for spawn_fn in ["__omni_go_spawn", "__omni_thread_spawn"] {
        asm.begin_function(spawn_fn);
        asm.emit_push(0, true);
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(0, 1);
        asm.emit_blx_reg(2);
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // Deterministic Xorshift32 RNG (`__omni_random_int`, `rand`, `__omni_random_range`)
    for rng_fn in ["__omni_random_int", "rand"] {
        asm.begin_function(rng_fn);
        asm.emit_load_u32(2, chip.sram_base);
        asm.emit_ldr_imm(0, 2, SRAM_G_RNG_STATE_OFF);
        asm.emit_lsls_imm(1, 0, 13);
        asm.emit_eors(0, 1);
        asm.emit_lsrs_imm(1, 0, 17);
        asm.emit_eors(0, 1);
        asm.emit_lsls_imm(1, 0, 5);
        asm.emit_eors(0, 1);
        asm.emit_str_imm(0, 2, SRAM_G_RNG_STATE_OFF);
        asm.emit_lsrs_imm(0, 0, 1); // positive 31-bit
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    asm.begin_function("__omni_random_range");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_movs_reg(4, 0); // lo
        asm.emit_subs_reg(5, 2, 0); // span = hi - lo
        asm.emit_cmp_imm8(5, 0);
        let non_zero = asm.new_label();
        asm.emit_bcond_label_short(Cond::Gt, non_zero);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
        asm.bind_label(non_zero);
        asm.emit_bl_sym("__omni_random_int");
        asm.emit_movs_reg(2, 5);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_urem64");
        asm.emit_adds_reg(0, 0, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // Clean zero-returning stubs for host-only OS / exception / fiber symbols if declared in module header
    for stub_sym in [
        "__omni_thread_join_all",
        "__omni_slice_destroy",
        "__omni_slice_print",
        "__omni_map_destroy",
        "__omni_map_delete",
        "__omni_map_set_int",
        "__omni_map_get_int",
        "__omni_map_contains_int",
        "_setjmp",
        "__omni_try_push",
        "__omni_try_pop",
        "__omni_throw",
        "__omni_rethrow",
        "__omni_catch_msg",
        "__omni_catch_type",
        "__omni_catch_code",
        "__omni_catch_matches",
        "__omni_chan_make",
        "__omni_chan_send",
        "__omni_chan_recv",
        "__omni_chan_send_int",
        "__omni_chan_recv_int",
        "__omni_chan_len",
        "__omni_chan_close",
        "__omni_chan_destroy",
        "__omni_fiber_init",
        "__omni_fiber_create",
        "__omni_fiber_yield",
        "__omni_fiber_destroy",
        "__omni_fiber_run_on_main_stack",
        "__omni_print_float",
        "__omni_float_to_str",
        "__omni_c_str_to_f64",
        "__omni_input",
        "fast_hash_block",
    ] {
        asm.begin_function(stub_sym);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    Ok(())
}
