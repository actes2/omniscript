use super::chip::{
    build_rp2040_boot2_256b, build_rp2350_picobin_block, RpChipKind, RpChipProfile,
};
use super::thumb_enc::{Cond, GlobalReloc, GlobalRelocKind, ThumbAssembler};

pub const SRAM_G_HEAP_PTR_OFF: u32 = 0x00;
pub const SRAM_G_RET_PTR_OFF: u32 = 0x04;
pub const SRAM_G_RNG_STATE_OFF: u32 = 0x08;
pub const SRAM_USER_GLOBALS_OFF: u32 = 0x20;

/// 32-bit Cortex-M `OmniSlice` header magic (`[0xFF, b'N', b'M', b'O']` in little-endian -> byte 0 == `0xFF`).
pub const OMNI_SLICE_MAGIC32: u32 = 0x4F4D_4EFF;
/// 32-bit Cortex-M `OmniClosure` header magic (`[0xFF, b'N', b'L', b'C']` in little-endian -> byte 0 == `0xFF`).
pub const OMNI_CLOSURE_MAGIC32: u32 = 0x434C_4EFF;
/// 32-bit Cortex-M `OmniMap` header magic (`[0xFF, b'P', b'A', b'M']` in little-endian -> byte 0 == `0xFF`).
pub const OMNI_MAP_MAGIC32: u32 = 0x4D41_50FF;
/// 32-bit Cortex-M Heap Object header magic (`[0xFF, b'J', b'B', b'O']` in little-endian -> byte 0 == `0xFF`).
pub const OMNI_OBJ_MAGIC32: u32 = 0x4F42_4AFF;
/// 32-bit Cortex-M Heap Object header live canary (`[payload_ptr - 4]`).
pub const OMNI_OBJ_CANARY32: u32 = 0x5A5A_5A5A;

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
    // 2. Reset_Handler: XOSC, PLL_SYS (125/150 MHz), Clocks, Ticks, Resets, SRAM, UART0, @main
    // -------------------------------------------------------------------------
    asm.begin_function("Reset_Handler");
    {
        // A) Enable 12 MHz XOSC (`STARTUP = 47`, `CTRL = 0x00FABAA0`), poll `STATUS` bit 31 (bounded timeout)
        asm.emit_load_u32(0, chip.xosc_base);
        asm.emit_movs_imm8(1, 47);
        asm.emit_str_imm(1, 0, 0x0C);
        asm.emit_load_u32(1, 0x00FA_BAA0);
        asm.emit_str_imm(1, 0, 0x00);
        asm.emit_load_u32(3, 0x0001_0000);
        let xosc_wait = asm.new_label();
        let xosc_done = asm.new_label();
        asm.bind_label(xosc_wait);
        asm.emit_ldr_imm(1, 0, 0x04);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Mi, xosc_done); // bit 31 == 1 -> XOSC stable
        asm.emit_subs_imm8(3, 1);
        asm.emit_bcond_label_short(Cond::Ne, xosc_wait);
        asm.bind_label(xosc_done);

        // B) Glitchless park `CLK_REF` -> `XOSC` (2), `CLK_SYS` -> `CLK_REF` (0) + poll `CLK_SYS_SELECTED` (`+0x44`),
        //    `CLK_SYS_DIV` = 1.0, `CLK_PERI` -> `ENABLE | XOSC` (`0x0880` = 12 MHz fixed reference for UART0 115200 baud)
        asm.emit_load_u32(0, chip.clocks_base);
        asm.emit_movs_imm8(1, 2);
        asm.emit_str_imm(1, 0, 0x30); // CLK_REF_CTRL = XOSC
        asm.emit_ldr_imm(1, 0, 0x3C);
        asm.emit_movs_imm8(2, 1);
        asm.emit_bics(1, 2);
        asm.emit_str_imm(1, 0, 0x3C); // Clear SRC bit 0 first (glitchless mux -> CLK_REF)
        asm.emit_load_u32(3, 0x0001_0000);
        let ref_sel_wait = asm.new_label();
        let ref_sel_done = asm.new_label();
        asm.bind_label(ref_sel_wait);
        asm.emit_ldr_imm(1, 0, 0x44); // CLK_SYS_SELECTED
        asm.emit_tst_reg(1, 2);       // bit 0 == 1 -> CLK_REF active
        asm.emit_bcond_label_short(Cond::Ne, ref_sel_done);
        asm.emit_subs_imm8(3, 1);
        asm.emit_bcond_label_short(Cond::Ne, ref_sel_wait);
        asm.bind_label(ref_sel_done);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 0, 0x3C); // CLK_SYS_CTRL = 0 (AUXSRC = PLL_SYS, SRC = CLK_REF)
        asm.emit_load_u32(1, chip.clk_sys_div_1);
        asm.emit_str_imm(1, 0, 0x40); // CLK_SYS_DIV = 1.0
        asm.emit_load_u32(1, 0x0000_0880);
        asm.emit_str_imm(1, 0, 0x48); // CLK_PERI_CTRL = ENABLE | XOSC

        // C) Configure 1 MHz microsecond tick generator (`12 MHz CLK_REF / 12 = 1 MHz`)
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

        // D) Pulse reset on `PLL_SYS` (`RESETS + 0x2000`), then unreset `PLL_SYS` + IO_BANK0, PADS_BANK0,
        //    PIO0..2, TIMER, UART0 via Atomic Clear (`RESETS + 0x3000`) and poll `RESET_DONE` (bounded)
        asm.emit_load_u32(0, chip.resets_base + 0x2000);
        asm.emit_load_u32(1, chip.resets_pll_sys_mask);
        asm.emit_str_imm(1, 0, 0x00);

        asm.emit_load_u32(0, chip.resets_base + 0x3000);
        asm.emit_load_u32(2, chip.resets_unreset_mask);
        asm.emit_str_imm(2, 0, 0x00);
        asm.emit_load_u32(0, chip.resets_base);
        asm.emit_load_u32(3, 0x0001_0000);
        let rst_wait = asm.new_label();
        let rst_done = asm.new_label();
        asm.bind_label(rst_wait);
        asm.emit_ldr_imm(1, 0, 0x08); // RESET_DONE
        asm.emit_ands(1, 2);
        asm.emit_cmp_reg(1, 2);
        asm.emit_bcond_label_short(Cond::Eq, rst_done);
        asm.emit_subs_imm8(3, 1);
        asm.emit_bcond_label_short(Cond::Ne, rst_wait);
        asm.bind_label(rst_done);

        // D2) Configure & Lock `PLL_SYS` (`125 MHz` on RP2040: FBDIV=125, 1500/6/2; `150 MHz` on RP2350: FBDIV=125, 1500/5/2)
        asm.emit_load_u32(0, chip.pll_sys_base);
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 0x00); // CS: REFDIV = 1
        asm.emit_load_u32(1, chip.pll_fbdiv_int);
        asm.emit_str_imm(1, 0, 0x08); // FBDIV_INT = 125
        // Power up VCO + main PLL (`VCOPD` bit 5 = 0, `PD` bit 0 = 0; keep `POSTDIVPD` bit 3 = 1, `DSMPD` bit 2 = 1 -> 0x0C)
        asm.emit_movs_imm8(1, 0x0C);
        asm.emit_str_imm(1, 0, 0x04); // PWR = POSTDIVPD | DSMPD
        // Poll `CS.LOCK` (bit 31) with bounded timeout, tracking `r4 = r_locked`
        asm.emit_movs_imm8(4, 0); // r_locked = 0
        asm.emit_load_u32(3, 0x0001_0000);
        let pll_wait = asm.new_label();
        let pll_got_lock = asm.new_label();
        let pll_after_wait = asm.new_label();
        asm.bind_label(pll_wait);
        asm.emit_ldr_imm(1, 0, 0x00); // CS
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Mi, pll_got_lock); // bit 31 == 1 -> LOCKED
        asm.emit_subs_imm8(3, 1);
        asm.emit_bcond_label_short(Cond::Ne, pll_wait);
        asm.emit_b_label_short(pll_after_wait);
        asm.bind_label(pll_got_lock);
        asm.emit_movs_imm8(4, 1); // r_locked = 1
        asm.bind_label(pll_after_wait);
        // Set `PRIM` (`POSTDIV1 | POSTDIV2`) and clear `POSTDIVPD` (`PWR = DSMPD = 0x04`)
        asm.emit_load_u32(1, chip.pll_prim_postdiv);
        asm.emit_str_imm(1, 0, 0x0C); // PRIM
        asm.emit_movs_imm8(1, 0x04);
        asm.emit_str_imm(1, 0, 0x04); // PWR: clear POSTDIVPD

        // ONLY switch `CLK_SYS_CTRL` to `PLL_SYS` (`1`) if `CS.LOCK` (`r4 != 0`) was observed;
        // otherwise keep `CLK_SYS_CTRL` safely on `CLK_REF` (`0`)!
        let sys_sel_done = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, sys_sel_done);

        asm.emit_load_u32(0, chip.clocks_base);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 0, 0x3C); // CLK_SYS_CTRL: AUXSRC=0 (PLL_SYS), SRC=0 (CLK_REF)
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 0x3C); // CLK_SYS_CTRL: AUXSRC=0 (PLL_SYS), SRC=1 (AUX)
        asm.emit_movs_imm8(2, 2);     // CLK_SYS_SELECTED bit 1 (SRC == 1)
        asm.emit_load_u32(3, 0x0001_0000);
        let sys_sel_wait = asm.new_label();
        asm.bind_label(sys_sel_wait);
        asm.emit_ldr_imm(1, 0, 0x44); // CLK_SYS_SELECTED
        asm.emit_tst_reg(1, 2);
        asm.emit_bcond_label_short(Cond::Ne, sys_sel_done);
        asm.emit_subs_imm8(3, 1);
        asm.emit_bcond_label_short(Cond::Ne, sys_sel_wait);
        asm.bind_label(sys_sel_done);

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

    // `malloc(i64 size)` (`r0` = size) -> `r0` = 8-byte aligned zeroed SRAM ptr (or `0` NULL on OOM!), `r1` = 0
    asm.begin_function("malloc");
    {
        asm.emit_push((1 << 4) | (1 << 5), false);
        let oom_lbl = asm.new_label();
        // Align size up to 8 bytes (minimum 8 bytes), guarding against 32-bit unsigned wrap
        asm.emit_cmp_imm8(0, 0);
        let non_zero = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, non_zero);
        asm.emit_movs_imm8(0, 8);
        asm.bind_label(non_zero);
        asm.emit_adds_imm8(0, 7);
        asm.emit_bcond_label_short(Cond::Cs, oom_lbl);
        asm.emit_movs_imm8(2, 7);
        asm.emit_bics(0, 2); // r0 = aligned_size

        asm.emit_load_u32(3, chip.sram_base);
        asm.emit_ldr_imm(4, 3, SRAM_G_HEAP_PTR_OFF); // r4 = old_heap
        asm.emit_adds_reg(5, 4, 0);                  // r5 = next_ptr
        asm.emit_bcond_label_short(Cond::Cs, oom_lbl);
        asm.emit_load_u32(2, chip.heap_limit);
        asm.emit_cmp_reg(5, 2);
        asm.emit_bcond_label_short(Cond::Hi, oom_lbl); // next_ptr > heap_limit -> return NULL without advancing heap_ptr!
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

        asm.bind_label(oom_lbl);
        asm.emit_movs_imm8(0, 0);
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
        let skip = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, skip);
        asm.emit_cmp_imm8(4, 0);
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

    // `__omni_str_len(i8* s=r0) -> i64 r0:r1`
    // Discriminates `OmniSlice*` (`0x4F4D4EFF`), `OmniMap*` (`0x4D4150FF`), and C `char*` identically to host!
    asm.begin_function("__omni_str_len");
    {
        let null_lbl = asm.new_label();
        let cstr_lbl = asm.new_label();
        let chk_map = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, null_lbl);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, cstr_lbl);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_SLICE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, chk_map);
        asm.emit_ldr_imm(0, 0, 12); // OmniSlice.len at offset +12
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);

        asm.bind_label(chk_map);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, cstr_lbl);
        asm.emit_ldr_imm(0, 0, 16); // OmniMap.len at offset +16
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);

        asm.bind_label(cstr_lbl);
        asm.emit_push(1 << 4, true);
        asm.emit_bl_sym("strlen");
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(1 << 4, true);

        asm.bind_label(null_lbl);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_str_ord(i8* s=r0) -> i64 r0:r1`
    asm.begin_function("__omni_str_ord");
    {
        let null_lbl = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, null_lbl);
        asm.emit_ldrb_imm(0, 0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.bind_label(null_lbl);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_str_chr(i64 code=r0:r1) -> i8* r0`
    asm.begin_function("__omni_str_chr");
    {
        asm.emit_push(1 << 4, true);
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_imm8(0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_strb_imm(4, 0, 0);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(1 << 4, true);
    }
    asm.end_function(base_vma)?;

    // `__omni_str_char_at(i8* s=r0, i64 idx=r2:r3) -> i8* r0`
    asm.begin_function("__omni_str_char_at");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 2);
        asm.emit_movs_reg(6, 3);
        asm.emit_bl_sym("strlen");
        asm.emit_cmp_imm8(6, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(5, 5, 0);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(5, 0);
        let oob = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_adds_reg(4, 4, 5);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_str_chr");
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.bind_label(oob);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_str_chr");
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_str_byte_at(i8* s=r0, i64 idx=r2:r3) -> i64 r0:r1`
    asm.begin_function("__omni_str_byte_at");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 2);
        asm.emit_movs_reg(6, 3);
        asm.emit_bl_sym("strlen");
        asm.emit_cmp_imm8(6, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(5, 5, 0);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(5, 0);
        let oob = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_adds_reg(4, 4, 5);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.bind_label(oob);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
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
    for consumer in ["__omni_c_consume_returned_ptr", "__omni_c_consume_returned_str"] {
        asm.begin_function(consumer);
        asm.emit_load_u32(2, chip.sram_base);
        asm.emit_ldr_imm(0, 2, SRAM_G_RET_PTR_OFF);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 2, SRAM_G_RET_PTR_OFF);
        asm.emit_bx(14);
        asm.end_function(base_vma)?;
    }

    // -------------------------------------------------------------------------
    // 4. 64-bit Integer & IEEE-754 Soft-Float Math Runtime
    // -------------------------------------------------------------------------
    emit_math64_helpers(asm, base_vma)?;
    emit_soft_float64_helpers(asm, base_vma)?;

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

    // `__omni_float_to_str(f64 val = r0:r1) -> i8* r0` (IEEE-754 finite + 6-digit fractional + nan/inf)
    emit_float_to_str_helper(asm, base_vma)?;

    // `__omni_print_float(f64 val = r0:r1)`
    asm.begin_function("__omni_print_float");
    {
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_float_to_str");
        asm.emit_bl_sym("__omni_print_str");
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `printf(i8* fmt = r0, ...)` -> freestanding UART0 formatter for `%s`, `%d`, `%ld`, `%lld`, `%u`, `%f`, `%%`
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
        asm.emit_bcond_near(Cond::Eq, done);

        let lp = asm.new_label();
        let fmt_spec = asm.new_label();
        asm.bind_label(lp);
        asm.emit_ldrb_imm(0, 4, 0);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, done);
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
        asm.emit_bcond_near(Cond::Eq, done);
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

        // `%f` / `%g`: 8-byte AAPCS-aligned `f64` argument
        let do_flt = asm.new_label();
        let not_flt = asm.new_label();
        asm.emit_cmp_imm8(0, b'f');
        asm.emit_bcond_label_short(Cond::Eq, do_flt);
        asm.emit_cmp_imm8(0, b'g');
        asm.emit_bcond_label_short(Cond::Ne, not_flt);
        asm.bind_label(do_flt);
        asm.emit_adds_imm8(5, 7);
        asm.emit_movs_imm8(2, 7);
        asm.emit_bics(5, 2);
        asm.emit_ldr_imm(0, 5, 0);
        asm.emit_ldr_imm(1, 5, 4);
        asm.emit_adds_imm8(5, 8);
        asm.emit_bl_sym("__omni_float_to_str");
        asm.emit_bl_sym("uart_puts");
        asm.emit_b_label_short(lp);
        asm.bind_label(not_flt);

        // Integer specifiers (`%d`, `%u`, `%lld`, `%ld`):
        asm.emit_cmp_imm8(6, 0);
        let i32_arg = asm.new_label();
        let print_num = asm.new_label();
        asm.emit_bcond_label_short(Cond::Eq, i32_arg);
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

/// Pure ARMv6-M Thumb-1 IEEE-754 `binary64` (`f64`, `r0:r1` = `lo32:hi32`) soft-float engine:
/// arithmetic (`fadd64`, `fsub64`, `fmul64`, `fdiv64`, `frem64`, `fpow64`, `fabs64`),
/// 3-way + NaN comparisons (`fcmp64`), and integer/single-precision conversions.
fn emit_soft_float64_helpers(asm: &mut ThumbAssembler, base_vma: u32) -> Result<(), String> {
    // 1. Internal shared GRS round-to-nearest-even + subnormal/Inf packer:
    //    `__omni_fpack64(m_lo=r0, m_hi=r1, exp=r2, sign_bit31=r3) -> f64 r0:r1`
    //    Normal MSB of `m` is at bit 55 (`0x0080_0000` in `r1`), bits `[2:0]` of `r0` = GRS.
    asm.begin_function("__omni_fpack64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        let non_zero_m = asm.new_label();
        asm.emit_movs_reg(4, 0);
        asm.emit_orrs(4, 1);
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, non_zero_m);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_reg(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(non_zero_m);
        let exp_pos = asm.new_label();
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Gt, exp_pos);

        // `exp <= 0`: shift right into subnormal range (or underflow to signed zero if `exp < -56`)
        asm.emit_movs_imm8(4, 56);
        asm.emit_rsbs_zero(4, 4); // r4 = -56
        let sub_shift_lp = asm.new_label();
        asm.emit_cmp_reg(2, 4);
        asm.emit_bcond_label_short(Cond::Ge, sub_shift_lp);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_reg(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(sub_shift_lp);
        asm.emit_cmp_imm8(2, 1);
        asm.emit_bcond_label_short(Cond::Ge, exp_pos);
        asm.emit_movs_imm8(4, 1);
        asm.emit_ands(4, 0); // sticky = m_lo & 1
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(5, 1, 31);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_orrs(0, 4);
        asm.emit_adds_imm8(2, 1);
        asm.emit_b_label_short(sub_shift_lp);

        asm.bind_label(exp_pos);
        // If bit 55 (`0x0080_0000` in `r1`) is 0 (subnormal), `exp_field = 0`, else `exp_field = r2`
        asm.emit_load_u32(4, 0x0080_0000);
        asm.emit_tst_reg(1, 4);
        let is_norm_m = asm.new_label();
        let do_grs = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, is_norm_m);
        asm.emit_movs_imm8(6, 0);
        asm.emit_b_label_short(do_grs);
        asm.bind_label(is_norm_m);
        asm.emit_movs_reg(6, 2);

        asm.bind_label(do_grs);
        asm.emit_movs_imm8(4, 7);
        asm.emit_ands(4, 0); // r4 = GRS
        asm.emit_lsrs_imm(0, 0, 3);
        asm.emit_lsls_imm(5, 1, 29);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 3);

        // Round to nearest, ties to even: `grs > 4 || (grs == 4 && (m_lo & 1) != 0)`
        let no_round = asm.new_label();
        let do_round = asm.new_label();
        asm.emit_cmp_imm8(4, 4);
        asm.emit_bcond_label_short(Cond::Cc, no_round);
        asm.emit_bcond_label_short(Cond::Hi, do_round);
        asm.emit_movs_imm8(5, 1);
        asm.emit_tst_reg(0, 5);
        asm.emit_bcond_label_short(Cond::Eq, no_round);

        asm.bind_label(do_round);
        asm.emit_adds_imm8(0, 1);
        asm.emit_movs_imm8(5, 0);
        asm.emit_adcs(1, 5);
        let chk_norm_carry = asm.new_label();
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Ne, chk_norm_carry);
        // Subnormal rounded up into normal (`0x0010_0000`)?
        asm.emit_load_u32(5, 0x0010_0000);
        asm.emit_tst_reg(1, 5);
        asm.emit_bcond_label_short(Cond::Eq, no_round);
        asm.emit_movs_imm8(6, 1);
        asm.emit_b_label_short(no_round);

        asm.bind_label(chk_norm_carry);
        asm.emit_load_u32(5, 0x0020_0000);
        asm.emit_tst_reg(1, 5);
        asm.emit_bcond_label_short(Cond::Eq, no_round);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(5, 1, 31);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_adds_imm8(6, 1);

        asm.bind_label(no_round);
        let ret_inf = asm.new_label();
        asm.emit_load_u32(4, 0x07FF);
        asm.emit_cmp_reg(6, 4);
        asm.emit_bcond_label_short(Cond::Ge, ret_inf);

        asm.emit_load_u32(4, 0x0010_0000);
        asm.emit_bics(1, 4); // clear implicit bit 52
        asm.emit_lsls_imm(6, 6, 20);
        asm.emit_orrs(1, 6);
        asm.emit_orrs(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(ret_inf);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF0_0000);
        asm.emit_orrs(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // 2. Internal 53-bit mantissa normalizer for non-zero finite `(lo=r0, abs_hi=r1)` -> `M=r0:r1`, `eff_exp=r2`
    asm.begin_function("__omni_fnorm53");
    {
        asm.emit_push(1 << 4, false);
        asm.emit_lsrs_imm(2, 1, 20);
        asm.emit_lsls_imm(1, 1, 12);
        asm.emit_lsrs_imm(1, 1, 12);
        asm.emit_load_u32(4, 0x0010_0000);
        let sub_init = asm.new_label();
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, sub_init);
        asm.emit_orrs(1, 4);
        asm.emit_pop(1 << 4, false);
        asm.emit_bx(14);

        asm.bind_label(sub_init);
        asm.emit_movs_imm8(2, 1);
        let sub_lp = asm.new_label();
        let sub_dn = asm.new_label();
        asm.bind_label(sub_lp);
        asm.emit_tst_reg(1, 4);
        asm.emit_bcond_label_short(Cond::Ne, sub_dn);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(sub_lp);
        asm.bind_label(sub_dn);
        asm.emit_pop(1 << 4, false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // 3. `__omni_fabs64(f64=r0:r1) -> f64 r0:r1`
    asm.begin_function("__omni_fabs64");
    asm.emit_lsls_imm(1, 1, 1);
    asm.emit_lsrs_imm(1, 1, 1);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // 4. `__omni_fcmp64(a=r0:r1, b=r2:r3) -> i32 r0`
    //    Returns `-1` (`a < b`), `0` (`a == b`, including `+0.0 == -0.0`), `1` (`a > b`), `2` (unordered / NaN).
    asm.begin_function("__omni_fcmp64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        let ret_lt = asm.new_label();
        let ret_eq = asm.new_label();
        let ret_gt = asm.new_label();
        let ret_nan = asm.new_label();

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // r4 = a_abs_hi
        asm.emit_lsls_imm(5, 3, 1);
        asm.emit_lsrs_imm(5, 5, 1); // r5 = b_abs_hi

        // NaN iff `(0x7FF0_0000 : 0) <u (abs_hi : lo)`
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        asm.emit_bcond_label_short(Cond::Cc, ret_nan);

        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 2);
        asm.emit_sbcs(7, 5);
        asm.emit_bcond_label_short(Cond::Cc, ret_nan);

        // `+0.0 == -0.0`
        asm.emit_movs_reg(6, 4);
        asm.emit_orrs(6, 0);
        asm.emit_orrs(6, 5);
        asm.emit_orrs(6, 2);
        asm.emit_bcond_label_short(Cond::Eq, ret_eq);

        // Exact bitwise equality
        asm.emit_movs_reg(6, 0);
        asm.emit_eors(6, 2);
        asm.emit_movs_reg(7, 1);
        asm.emit_eors(7, 3);
        asm.emit_orrs(6, 7);
        asm.emit_bcond_label_short(Cond::Eq, ret_eq);

        // Compare signs
        asm.emit_lsrs_imm(6, 1, 31); // sign_a
        asm.emit_lsrs_imm(7, 3, 31); // sign_b
        let same_sign = asm.new_label();
        asm.emit_cmp_reg(6, 7);
        asm.emit_bcond_label_short(Cond::Eq, same_sign);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Ne, ret_lt);
        asm.emit_b_label_short(ret_gt);

        asm.bind_label(same_sign);
        let mag_lt = asm.new_label();
        asm.emit_subs_reg(7, 0, 2);
        asm.emit_sbcs(4, 5);
        asm.emit_bcond_label_short(Cond::Cc, mag_lt);
        // `|a| > |b|`
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, ret_gt);
        asm.emit_b_label_short(ret_lt);

        asm.bind_label(mag_lt);
        // `|a| < |b|`
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, ret_lt);
        asm.emit_b_label_short(ret_gt);

        asm.bind_label(ret_lt);
        asm.emit_movs_imm8(0, 1);
        asm.emit_rsbs_zero(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(ret_eq);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(ret_gt);
        asm.emit_movs_imm8(0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);

        asm.bind_label(ret_nan);
        asm.emit_movs_imm8(0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // 5. `__omni_fadd64(a=r0:r1, b=r2:r3) -> f64 r0:r1`
    asm.begin_function("__omni_fadd64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(16);

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // r4 = a_abs_hi
        asm.emit_lsls_imm(5, 3, 1);
        asm.emit_lsrs_imm(5, 5, 1); // r5 = b_abs_hi

        // Swap so `|a| >= |b|` always
        let no_swap = asm.new_label();
        asm.emit_subs_reg(6, 0, 2);
        asm.emit_movs_reg(7, 4);
        asm.emit_sbcs(7, 5);
        asm.emit_bcond_label_short(Cond::Cs, no_swap);
        asm.emit_movs_reg(6, 0);
        asm.emit_movs_reg(0, 2);
        asm.emit_movs_reg(2, 6);
        asm.emit_movs_reg(6, 1);
        asm.emit_movs_reg(1, 3);
        asm.emit_movs_reg(3, 6);
        asm.emit_movs_reg(6, 4);
        asm.emit_movs_reg(4, 5);
        asm.emit_movs_reg(5, 6);
        asm.bind_label(no_swap);

        asm.emit_lsrs_imm(6, 1, 31);
        asm.emit_lsls_imm(6, 6, 31);
        asm.emit_str_sp(6, 0); // [sp+0] = sign_a
        asm.emit_lsrs_imm(7, 3, 31);
        asm.emit_lsls_imm(7, 7, 31);
        asm.emit_str_sp(7, 4); // [sp+4] = sign_b

        asm.emit_lsrs_imm(6, 4, 20); // exp_a
        asm.emit_lsrs_imm(7, 5, 20); // exp_b

        let finite_a = asm.new_label();
        let ret_qnan = asm.new_label();
        let ret_a_done = asm.new_label();
        asm.emit_load_u32(3, 0x07FF);
        asm.emit_cmp_reg(6, 3);
        asm.emit_bcond_label_short(Cond::Ne, finite_a);

        // `exp_a == 0x7FF`: NaN or Inf
        asm.emit_load_u32(3, 0x7FF0_0000);
        asm.emit_cmp_reg(4, 3);
        asm.emit_bcond_near(Cond::Hi, ret_qnan);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Ne, ret_qnan);
        // `a` is Inf; if `b` is also Inf with opposite sign -> qNaN
        asm.emit_load_u32(3, 0x07FF);
        asm.emit_cmp_reg(7, 3);
        asm.emit_bcond_near(Cond::Ne, ret_a_done);
        asm.emit_ldr_sp(6, 0);
        asm.emit_ldr_sp(7, 4);
        asm.emit_cmp_reg(6, 7);
        asm.emit_bcond_near(Cond::Ne, ret_qnan);
        asm.emit_b_label_short(ret_a_done);

        asm.bind_label(finite_a);
        let nonzero_b = asm.new_label();
        asm.emit_movs_reg(3, 5);
        asm.emit_orrs(3, 2);
        asm.emit_cmp_imm8(3, 0);
        asm.emit_bcond_label_short(Cond::Ne, nonzero_b);
        // `b == ±0.0`: if `a == ±0.0`, IEEE-754 says `-0 + -0 = -0`, else `+0`
        asm.emit_movs_reg(3, 4);
        asm.emit_orrs(3, 0);
        asm.emit_cmp_imm8(3, 0);
        asm.emit_bcond_near(Cond::Ne, ret_a_done);
        asm.emit_ldr_sp(0, 0);
        asm.emit_ldr_sp(1, 4);
        asm.emit_ands(1, 0);
        asm.emit_movs_imm8(0, 0);
        asm.emit_b_label_short(ret_a_done);

        asm.bind_label(nonzero_b);
        // Build `m_a = (frac_a | implicit) << 3` in `[sp+8..16]`, `eff_a` in `r6`
        asm.emit_lsls_imm(4, 4, 12);
        asm.emit_lsrs_imm(4, 4, 12);
        let norm_a = asm.new_label();
        let sh3_a = asm.new_label();
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Ne, norm_a);
        asm.emit_movs_imm8(6, 1);
        asm.emit_b_label_short(sh3_a);
        asm.bind_label(norm_a);
        asm.emit_load_u32(3, 0x0010_0000);
        asm.emit_orrs(4, 3);
        asm.bind_label(sh3_a);
        asm.emit_lsls_imm(4, 4, 3);
        asm.emit_lsrs_imm(3, 0, 29);
        asm.emit_orrs(4, 3);
        asm.emit_lsls_imm(0, 0, 3);
        asm.emit_str_sp(0, 8);
        asm.emit_str_sp(4, 12);

        // Build `m_b = (frac_b | implicit) << 3` in `r2:r5`, `eff_b` in `r7`
        asm.emit_lsls_imm(5, 5, 12);
        asm.emit_lsrs_imm(5, 5, 12);
        let norm_b = asm.new_label();
        let sh3_b = asm.new_label();
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Ne, norm_b);
        asm.emit_movs_imm8(7, 1);
        asm.emit_b_label_short(sh3_b);
        asm.bind_label(norm_b);
        asm.emit_load_u32(3, 0x0010_0000);
        asm.emit_orrs(5, 3);
        asm.bind_label(sh3_b);
        asm.emit_lsls_imm(5, 5, 3);
        asm.emit_lsrs_imm(3, 2, 29);
        asm.emit_orrs(5, 3);
        asm.emit_lsls_imm(2, 2, 3);

        // Sticky right-shift `m_b` by `diff = eff_a - eff_b`
        asm.emit_subs_reg(7, 6, 7);
        let align_lp = asm.new_label();
        let aligned_dn = asm.new_label();
        asm.emit_cmp_imm8(7, 57);
        asm.emit_bcond_label_short(Cond::Cc, align_lp);
        asm.emit_movs_imm8(2, 1);
        asm.emit_movs_imm8(5, 0);
        asm.emit_b_label_short(aligned_dn);

        asm.bind_label(align_lp);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, aligned_dn);
        asm.emit_movs_imm8(3, 1);
        asm.emit_ands(3, 2);
        asm.emit_lsrs_imm(2, 2, 1);
        asm.emit_lsls_imm(4, 5, 31);
        asm.emit_orrs(2, 4);
        asm.emit_lsrs_imm(5, 5, 1);
        asm.emit_orrs(2, 3);
        asm.emit_subs_imm8(7, 1);
        asm.emit_b_label_short(align_lp);

        asm.bind_label(aligned_dn);
        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_ldr_sp(3, 0);
        asm.emit_ldr_sp(4, 4);
        let do_sub_m = asm.new_label();
        let pack_res = asm.new_label();
        asm.emit_cmp_reg(3, 4);
        asm.emit_bcond_label_short(Cond::Ne, do_sub_m);

        // Same sign: `m_res = m_a + m_b`
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_adcs(1, 5);
        asm.emit_load_u32(4, 0x0100_0000);
        asm.emit_tst_reg(1, 4);
        asm.emit_bcond_label_short(Cond::Eq, pack_res);
        asm.emit_movs_imm8(4, 1);
        asm.emit_ands(4, 0);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(5, 1, 31);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_orrs(0, 4);
        asm.emit_adds_imm8(6, 1);
        asm.emit_b_label_short(pack_res);

        // Opposite sign: `m_res = m_a - m_b`
        asm.bind_label(do_sub_m);
        asm.emit_subs_reg(0, 0, 2);
        asm.emit_sbcs(1, 5);
        let norm_sub_init = asm.new_label();
        asm.emit_movs_reg(4, 0);
        asm.emit_orrs(4, 1);
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, norm_sub_init);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_b_label_short(ret_a_done);

        asm.bind_label(norm_sub_init);
        asm.emit_load_u32(4, 0x0080_0000);
        let norm_sub_lp = asm.new_label();
        asm.bind_label(norm_sub_lp);
        asm.emit_tst_reg(1, 4);
        asm.emit_bcond_label_short(Cond::Ne, pack_res);
        asm.emit_cmp_imm8(6, 1);
        asm.emit_bcond_label_short(Cond::Le, pack_res);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        asm.emit_subs_imm8(6, 1);
        asm.emit_b_label_short(norm_sub_lp);

        asm.bind_label(pack_res);
        asm.emit_movs_reg(2, 6);
        asm.emit_ldr_sp(3, 0);
        asm.emit_bl_sym("__omni_fpack64");
        asm.emit_b_label_short(ret_a_done);

        asm.bind_label(ret_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF8_0000);

        asm.bind_label(ret_a_done);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 6. `__omni_fsub64(a=r0:r1, b=r2:r3) -> f64 r0:r1`
    //    Flips sign bit `0x8000_0000` of `r3` and branches into `__omni_fadd64`!
    asm.begin_function("__omni_fsub64");
    {
        asm.emit_push(1 << 4, true);
        asm.emit_load_u32(4, 0x8000_0000);
        asm.emit_eors(3, 4);
        asm.emit_bl_sym("__omni_fadd64");
        asm.emit_pop(1 << 4, true);
    }
    asm.end_function(base_vma)?;

    // 7. `__omni_fmul64(a=r0:r1, b=r2:r3) -> f64 r0:r1`
    asm.begin_function("__omni_fmul64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(16);

        let mul_qnan = asm.new_label();
        let mul_has_inf = asm.new_label();
        let mul_zero = asm.new_label();
        let mul_exit = asm.new_label();

        asm.emit_movs_reg(4, 1);
        asm.emit_eors(4, 3);
        asm.emit_lsrs_imm(4, 4, 31);
        asm.emit_lsls_imm(4, 4, 31);
        asm.emit_str_sp(4, 0); // [sp+0] = sign_res

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // a_abs_hi
        asm.emit_lsls_imm(5, 3, 1);
        asm.emit_lsrs_imm(5, 5, 1); // b_abs_hi

        // NaN checks
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        asm.emit_bcond_near(Cond::Cc, mul_qnan);

        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 2);
        asm.emit_sbcs(7, 5);
        asm.emit_bcond_near(Cond::Cc, mul_qnan);

        // Inf checks
        asm.emit_lsrs_imm(6, 4, 20);
        asm.emit_lsrs_imm(7, 5, 20);
        asm.emit_load_u32(3, 0x07FF);
        asm.emit_cmp_reg(6, 3);
        asm.emit_bcond_near(Cond::Eq, mul_has_inf);
        asm.emit_cmp_reg(7, 3);
        asm.emit_bcond_near(Cond::Eq, mul_has_inf);

        // Zero checks
        asm.emit_movs_reg(6, 4);
        asm.emit_orrs(6, 0);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Eq, mul_zero);
        asm.emit_movs_reg(6, 5);
        asm.emit_orrs(6, 2);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Eq, mul_zero);

        // Normalize `a` and `b`
        asm.emit_str_sp(2, 8);
        asm.emit_str_sp(5, 12);
        asm.emit_movs_reg(1, 4);
        asm.emit_bl_sym("__omni_fnorm53");
        asm.emit_str_sp(2, 4); // eff_a
        asm.emit_lsls_imm(5, 1, 3);
        asm.emit_lsrs_imm(3, 0, 29);
        asm.emit_orrs(5, 3);
        asm.emit_lsls_imm(4, 0, 3); // r4:r5 = M_a << 3

        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_bl_sym("__omni_fnorm53"); // r0:r1 = M_b, r2 = eff_b
        asm.emit_ldr_sp(3, 4);
        asm.emit_adds_reg(2, 2, 3);
        asm.emit_load_u32(3, 1023);
        asm.emit_subs_reg(2, 2, 3);
        asm.emit_str_sp(2, 4); // exp_res

        // 53-step sticky right-shift mantissa multiplication
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(3, 1);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_movs_imm8(6, 52);

        let mul_lp = asm.new_label();
        let mul_no_add = asm.new_label();
        let mul_lp_done = asm.new_label();
        asm.bind_label(mul_lp);
        asm.emit_movs_imm8(7, 1);
        asm.emit_tst_reg(2, 7);
        asm.emit_bcond_label_short(Cond::Eq, mul_no_add);
        asm.emit_adds_reg(0, 0, 4);
        asm.emit_adcs(1, 5);
        asm.bind_label(mul_no_add);

        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, mul_lp_done);

        // Sticky `acc >>= 1`
        asm.emit_movs_imm8(7, 1);
        asm.emit_ands(7, 0);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_push(1 << 6, false);
        asm.emit_lsls_imm(6, 1, 31);
        asm.emit_orrs(0, 6);
        asm.emit_pop(1 << 6, false);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_orrs(0, 7);

        // `M_b >>= 1`
        asm.emit_lsrs_imm(2, 2, 1);
        asm.emit_lsls_imm(7, 3, 31);
        asm.emit_orrs(2, 7);
        asm.emit_lsrs_imm(3, 3, 1);

        asm.emit_subs_imm8(6, 1);
        asm.emit_b_label_short(mul_lp);

        asm.bind_label(mul_lp_done);
        asm.emit_ldr_sp(2, 4);
        let mul_pack = asm.new_label();
        asm.emit_load_u32(4, 0x0100_0000);
        asm.emit_tst_reg(1, 4);
        asm.emit_bcond_label_short(Cond::Eq, mul_pack);
        asm.emit_movs_imm8(4, 1);
        asm.emit_ands(4, 0);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(5, 1, 31);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_orrs(0, 4);
        asm.emit_adds_imm8(2, 1);

        asm.bind_label(mul_pack);
        asm.emit_ldr_sp(3, 0);
        asm.emit_bl_sym("__omni_fpack64");
        asm.emit_b_label_short(mul_exit);

        asm.bind_label(mul_has_inf);
        asm.emit_movs_reg(6, 4);
        asm.emit_orrs(6, 0);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, mul_qnan);
        asm.emit_movs_reg(6, 5);
        asm.emit_orrs(6, 2);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, mul_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF0_0000);
        asm.emit_ldr_sp(3, 0);
        asm.emit_orrs(1, 3);
        asm.emit_b_label_short(mul_exit);

        asm.bind_label(mul_zero);
        asm.emit_movs_imm8(0, 0);
        asm.emit_ldr_sp(1, 0);
        asm.emit_b_label_short(mul_exit);

        asm.bind_label(mul_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF8_0000);

        asm.bind_label(mul_exit);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 8. `__omni_fdiv64(a=r0:r1, b=r2:r3) -> f64 r0:r1`
    asm.begin_function("__omni_fdiv64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(16);

        let div_qnan = asm.new_label();
        let div_inf = asm.new_label();
        let div_zero = asm.new_label();
        let div_exit = asm.new_label();

        asm.emit_movs_reg(4, 1);
        asm.emit_eors(4, 3);
        asm.emit_lsrs_imm(4, 4, 31);
        asm.emit_lsls_imm(4, 4, 31);
        asm.emit_str_sp(4, 0); // [sp+0] = sign_res

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // a_abs_hi
        asm.emit_lsls_imm(5, 3, 1);
        asm.emit_lsrs_imm(5, 5, 1); // b_abs_hi

        // NaN checks
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        asm.emit_bcond_near(Cond::Cc, div_qnan);

        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 2);
        asm.emit_sbcs(7, 5);
        asm.emit_bcond_near(Cond::Cc, div_qnan);

        asm.emit_lsrs_imm(6, 4, 20); // exp_a
        asm.emit_lsrs_imm(7, 5, 20); // exp_b
        asm.emit_load_u32(3, 0x07FF);
        let div_a_fin = asm.new_label();
        asm.emit_cmp_reg(6, 3);
        asm.emit_bcond_label_short(Cond::Ne, div_a_fin);
        asm.emit_cmp_reg(7, 3);
        asm.emit_bcond_near(Cond::Eq, div_qnan);
        asm.emit_b_label_short(div_inf);

        asm.bind_label(div_a_fin);
        asm.emit_cmp_reg(7, 3);
        asm.emit_bcond_near(Cond::Eq, div_zero);

        // Check `b == 0`
        let div_b_nz = asm.new_label();
        asm.emit_movs_reg(6, 5);
        asm.emit_orrs(6, 2);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Ne, div_b_nz);
        asm.emit_movs_reg(6, 4);
        asm.emit_orrs(6, 0);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Eq, div_qnan);
        asm.emit_b_label_short(div_inf);

        asm.bind_label(div_b_nz);
        asm.emit_movs_reg(6, 4);
        asm.emit_orrs(6, 0);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Eq, div_zero);

        // Normalize `a` and `b`
        asm.emit_str_sp(2, 8);
        asm.emit_str_sp(5, 12);
        asm.emit_movs_reg(1, 4);
        asm.emit_bl_sym("__omni_fnorm53");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_str_sp(2, 4); // eff_a

        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_bl_sym("__omni_fnorm53"); // r0:r1 = M_b, r2 = eff_b
        asm.emit_str_sp(0, 8);
        asm.emit_str_sp(1, 12);

        asm.emit_ldr_sp(3, 4);
        asm.emit_subs_reg(3, 3, 2);
        asm.emit_load_u32(2, 1023);
        asm.emit_adds_reg(3, 3, 2); // exp_res

        let div_ge = asm.new_label();
        asm.emit_subs_reg(6, 4, 0);
        asm.emit_movs_reg(7, 5);
        asm.emit_sbcs(7, 1);
        asm.emit_bcond_label_short(Cond::Cs, div_ge);
        asm.emit_adds_reg(4, 4, 4);
        asm.emit_adcs(5, 5);
        asm.emit_subs_imm8(3, 1);
        asm.bind_label(div_ge);
        asm.emit_str_sp(3, 4);

        // 55-step restoring mantissa division
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_movs_imm8(6, 55);
        let div_lp = asm.new_label();
        let div_no_sub = asm.new_label();
        let div_lp_done = asm.new_label();
        asm.bind_label(div_lp);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        asm.emit_ldr_sp(2, 8);
        asm.emit_ldr_sp(3, 12);
        asm.emit_subs_reg(2, 4, 2);
        asm.emit_movs_reg(7, 5);
        asm.emit_sbcs(7, 3);
        asm.emit_bcond_label_short(Cond::Cc, div_no_sub);
        asm.emit_movs_reg(4, 2);
        asm.emit_movs_reg(5, 7);
        asm.emit_adds_imm8(0, 1);
        asm.bind_label(div_no_sub);
        asm.emit_subs_imm8(6, 1);
        asm.emit_bcond_label_short(Cond::Eq, div_lp_done);
        asm.emit_adds_reg(4, 4, 4);
        asm.emit_adcs(5, 5);
        asm.emit_b_label_short(div_lp);

        asm.bind_label(div_lp_done);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        let div_pack = asm.new_label();
        asm.emit_orrs(4, 5);
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, div_pack);
        asm.emit_movs_imm8(2, 1);
        asm.emit_orrs(0, 2);

        asm.bind_label(div_pack);
        asm.emit_ldr_sp(2, 4);
        asm.emit_ldr_sp(3, 0);
        asm.emit_bl_sym("__omni_fpack64");
        asm.emit_b_label_short(div_exit);

        asm.bind_label(div_inf);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF0_0000);
        asm.emit_ldr_sp(3, 0);
        asm.emit_orrs(1, 3);
        asm.emit_b_label_short(div_exit);

        asm.bind_label(div_zero);
        asm.emit_movs_imm8(0, 0);
        asm.emit_ldr_sp(1, 0);
        asm.emit_b_label_short(div_exit);

        asm.bind_label(div_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF8_0000);

        asm.bind_label(div_exit);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 9. `__omni_u64_to_f64(u64=r0:r1) -> f64 r0:r1` (`uitofp`)
    asm.begin_function("__omni_u64_to_f64");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let nz = asm.new_label();
        asm.emit_movs_reg(4, 0);
        asm.emit_orrs(4, 1);
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, nz);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(nz);
        asm.emit_load_u32(2, 1078); // bit 55 corresponds to 2^0 when exp = 1023 + 55 = 1078
        let shr_lp = asm.new_label();
        let shl_chk = asm.new_label();
        let shl_lp = asm.new_label();
        let pack_it = asm.new_label();

        asm.bind_label(shr_lp);
        asm.emit_lsrs_imm(4, 1, 24);
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, shl_chk);
        asm.emit_movs_imm8(4, 1);
        asm.emit_ands(4, 0);
        asm.emit_lsrs_imm(0, 0, 1);
        asm.emit_lsls_imm(5, 1, 31);
        asm.emit_orrs(0, 5);
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_orrs(0, 4);
        asm.emit_adds_imm8(2, 1);
        asm.emit_b_label_short(shr_lp);

        asm.bind_label(shl_chk);
        asm.emit_load_u32(4, 0x0080_0000);
        asm.bind_label(shl_lp);
        asm.emit_tst_reg(1, 4);
        asm.emit_bcond_label_short(Cond::Ne, pack_it);
        asm.emit_adds_reg(0, 0, 0);
        asm.emit_adcs(1, 1);
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(shl_lp);

        asm.bind_label(pack_it);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_fpack64");
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // 10. `__omni_i64_to_f64(i64=r0:r1) -> f64 r0:r1` (`sitofp`)
    asm.begin_function("__omni_i64_to_f64");
    {
        asm.emit_push(1 << 4, true);
        let pos = asm.new_label();
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Pl, pos);
        asm.emit_movs_imm8(2, 0);
        asm.emit_movs_imm8(3, 0);
        asm.emit_subs_reg(0, 2, 0);
        asm.emit_sbcs(3, 1);
        asm.emit_movs_reg(1, 3);
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_load_u32(2, 0x8000_0000);
        asm.emit_orrs(1, 2);
        asm.emit_pop(1 << 4, true);

        asm.bind_label(pos);
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_pop(1 << 4, true);
    }
    asm.end_function(base_vma)?;

    // 11. `__omni_f64_to_u64(f64=r0:r1) -> u64 r0:r1` (`fptoui`)
    asm.begin_function("__omni_f64_to_u64");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let ret_u0 = asm.new_label();
        let ovf_u64 = asm.new_label();
        let shr_u64 = asm.new_label();
        let sat_u64 = asm.new_label();

        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Mi, ret_u0);
        asm.emit_lsrs_imm(4, 1, 20);
        asm.emit_load_u32(5, 1023);
        asm.emit_cmp_reg(4, 5);
        asm.emit_bcond_label_short(Cond::Cc, ret_u0);
        asm.emit_load_u32(5, 1087);
        asm.emit_cmp_reg(4, 5);
        asm.emit_bcond_label_short(Cond::Cs, ovf_u64);

        asm.emit_lsls_imm(1, 1, 12);
        asm.emit_lsrs_imm(1, 1, 12);
        asm.emit_load_u32(5, 0x0010_0000);
        asm.emit_orrs(1, 5);

        asm.emit_load_u32(5, 1075);
        asm.emit_cmp_reg(4, 5);
        asm.emit_bcond_label_short(Cond::Cc, shr_u64);
        asm.emit_subs_reg(2, 4, 5);
        asm.emit_bl_sym("__omni_shl64");
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(shr_u64);
        asm.emit_subs_reg(2, 5, 4);
        asm.emit_bl_sym("__omni_lshr64");
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(ovf_u64);
        asm.emit_load_u32(5, 0x7FF0_0000);
        asm.emit_cmp_reg(1, 5);
        asm.emit_bcond_label_short(Cond::Hi, ret_u0);
        asm.emit_bcond_label_short(Cond::Ne, sat_u64);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Ne, ret_u0);

        asm.bind_label(sat_u64);
        asm.emit_movs_imm8(0, 0);
        asm.emit_mvns(0, 0);
        asm.emit_movs_reg(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(ret_u0);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // 12. `__omni_f64_to_i64(f64=r0:r1) -> i64 r0:r1` (`fptosi`)
    asm.begin_function("__omni_f64_to_i64");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let ovf_i64 = asm.new_label();
        let ret_i0 = asm.new_label();
        let sat_i64 = asm.new_label();
        let done_i64 = asm.new_label();

        asm.emit_lsrs_imm(4, 1, 31); // sign
        asm.emit_lsls_imm(1, 1, 1);
        asm.emit_lsrs_imm(1, 1, 1);  // abs_hi
        asm.emit_lsrs_imm(5, 1, 20); // exp
        asm.emit_load_u32(2, 1086);
        asm.emit_cmp_reg(5, 2);
        asm.emit_bcond_label_short(Cond::Cs, ovf_i64);

        asm.emit_bl_sym("__omni_f64_to_u64");
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, done_i64);
        asm.emit_movs_imm8(2, 0);
        asm.emit_movs_imm8(3, 0);
        asm.emit_subs_reg(0, 2, 0);
        asm.emit_sbcs(3, 1);
        asm.emit_movs_reg(1, 3);
        asm.bind_label(done_i64);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(ovf_i64);
        asm.emit_load_u32(2, 0x7FF0_0000);
        asm.emit_cmp_reg(1, 2);
        asm.emit_bcond_label_short(Cond::Hi, ret_i0);
        asm.emit_bcond_label_short(Cond::Ne, sat_i64);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Ne, ret_i0);

        asm.bind_label(sat_i64);
        let sat_neg = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, sat_neg);
        asm.emit_movs_imm8(0, 0);
        asm.emit_mvns(0, 0);
        asm.emit_lsrs_imm(1, 0, 1); // 0x7FFF_FFFF : 0xFFFF_FFFF
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(sat_neg);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x8000_0000); // i64::MIN
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(ret_i0);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // 13. `__omni_frem64(a=r0:r1, b=r2:r3) -> f64 r0:r1` (`a - trunc(a / b) * b`)
    asm.begin_function("__omni_frem64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_movs_reg(6, 2);
        asm.emit_movs_reg(7, 3);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_bl_sym("__omni_f64_to_i64");
        asm.emit_bl_sym("__omni_i64_to_f64");
        asm.emit_movs_reg(2, 6);
        asm.emit_movs_reg(3, 7);
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(3, 1);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("__omni_fsub64");
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 14a. `__omni_fsqrt64(x=r0:r1) -> f64 r0:r1` (IEEE-754 square root via 8-step Newton-Raphson)
    asm.begin_function("__omni_fsqrt64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(8);
        let sqrt_ret_self = asm.new_label();
        let sqrt_ret_qnan = asm.new_label();

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // r4 = x_abs_hi

        // `±0.0` -> return `±0.0` unchanged
        asm.emit_movs_reg(5, 4);
        asm.emit_orrs(5, 0);
        asm.emit_cmp_imm8(5, 0);
        asm.emit_bcond_near(Cond::Eq, sqrt_ret_self);

        // `NaN` -> return canonical `qNaN`
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        asm.emit_bcond_near(Cond::Cc, sqrt_ret_qnan);

        // `x < 0.0` (non-zero negative) -> return canonical `qNaN`
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_near(Cond::Mi, sqrt_ret_qnan);

        // `+Inf` -> return `+Inf`
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_cmp_reg(4, 7);
        asm.emit_bcond_near(Cond::Eq, sqrt_ret_self);

        // Positive finite `x`: save `x` at `[sp+0..8]`, normalize via `__omni_fnorm53`
        asm.emit_str_sp(0, 0);
        asm.emit_str_sp(1, 4);
        asm.emit_movs_reg(1, 4);
        asm.emit_bl_sym("__omni_fnorm53"); // r2 = eff_exp (biased by 1023)
        asm.emit_load_u32(3, 1023);
        asm.emit_adds_reg(2, 2, 3);
        asm.emit_asrs_imm(2, 2, 1); // seed_exp = (eff_exp + 1023) >> 1
        asm.emit_lsls_imm(5, 2, 20);
        asm.emit_movs_imm8(4, 0);   // g = 2^(seed_exp - 1023) in r4:r5

        // 8 iterations of `g = 0.5 * (g + x / g)`
        asm.emit_movs_imm8(6, 8);
        let nr_lp = asm.new_label();
        asm.bind_label(nr_lp);
        asm.emit_ldr_sp(0, 0);
        asm.emit_ldr_sp(1, 4);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_fadd64");
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x3FE0_0000); // 0.5
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_subs_imm8(6, 1);
        asm.emit_bcond_label_short(Cond::Ne, nr_lp);
        asm.emit_b_label_short(sqrt_ret_self);

        asm.bind_label(sqrt_ret_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF8_0000);

        asm.bind_label(sqrt_ret_self);
        asm.emit_add_sp(8);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 14b. `__omni_fpow64(base=r0:r1, exp=r2:r3) -> f64 r0:r1`
    //      IEEE-754 special cases (`exp==±0`, `base==1`, `NaN`, `exp==±Inf`) + fast binary squaring + half-integer `sqrt`!
    asm.begin_function("__omni_fpow64");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(24);
        // [sp+0..8]=base, [sp+8..16]=abs_exp, [sp+16]=exp_neg, [sp+20]=int_n

        let ret_one = asm.new_label();
        let ret_qnan = asm.new_label();
        let ret_pos_inf = asm.new_label();
        let ret_pos_zero = asm.new_label();
        let pow_exit = asm.new_label();

        asm.emit_lsls_imm(4, 1, 1);
        asm.emit_lsrs_imm(4, 4, 1); // r4 = base_abs_hi
        asm.emit_lsls_imm(5, 3, 1);
        asm.emit_lsrs_imm(5, 5, 1); // r5 = exp_abs_hi

        // 1) `exp == ±0.0` -> `1.0` (even if `base` is `NaN` or `±Inf`)
        asm.emit_movs_reg(6, 5);
        asm.emit_orrs(6, 2);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Eq, ret_one);

        // 2) `base == +1.0` -> `1.0` (even if `exp` is `NaN` or `±Inf`)
        let not_base_one = asm.new_label();
        asm.emit_load_u32(6, 0x3FF0_0000);
        asm.emit_cmp_reg(1, 6);
        asm.emit_bcond_label_short(Cond::Ne, not_base_one);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, ret_one);
        asm.bind_label(not_base_one);

        // 3) Either `base` or `exp` is `NaN` -> canonical `qNaN`
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        asm.emit_bcond_near(Cond::Cc, ret_qnan);

        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x7FF0_0000);
        asm.emit_subs_reg(6, 6, 2);
        asm.emit_sbcs(7, 5);
        asm.emit_bcond_near(Cond::Cc, ret_qnan);

        // 4) `exp == ±Inf` or huge `|exp| >= 2^31` (`exp_abs_hi >= 0x41E0_0000`)
        let finite_mod_exp = asm.new_label();
        asm.emit_load_u32(7, 0x41E0_0000);
        asm.emit_cmp_reg(5, 7);
        asm.emit_bcond_label_short(Cond::Cc, finite_mod_exp);

        // If `|base| == 1.0` (`base == -1.0` with `±Inf` or even `>= 2^53`) -> `1.0`
        let inf_cmp_mag = asm.new_label();
        asm.emit_load_u32(6, 0x3FF0_0000);
        asm.emit_cmp_reg(4, 6);
        asm.emit_bcond_label_short(Cond::Ne, inf_cmp_mag);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, ret_one);

        asm.bind_label(inf_cmp_mag);
        // `gt1 = ((0x3FF0_0000 : 0) <u (base_abs_hi : base_lo))`
        asm.emit_movs_imm8(6, 0);
        asm.emit_load_u32(7, 0x3FF0_0000);
        asm.emit_subs_reg(6, 6, 0);
        asm.emit_sbcs(7, 4);
        let mag_lt_1 = asm.new_label();
        let mag_done = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, mag_lt_1);
        asm.emit_movs_imm8(6, 1); // |base| > 1.0
        asm.emit_b_label_short(mag_done);
        asm.bind_label(mag_lt_1);
        asm.emit_movs_imm8(6, 0); // |base| < 1.0
        asm.bind_label(mag_done);
        asm.emit_lsrs_imm(7, 3, 31); // exp_neg (0 for +Inf, 1 for -Inf)
        asm.emit_eors(6, 7);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_near(Cond::Ne, ret_pos_inf); // (+Inf & >1) or (-Inf & <1) -> +Inf
        asm.emit_b_label_short(ret_pos_zero);       // (+Inf & <1) or (-Inf & >1) -> +0.0

        // 5) Finite `0 < |exp| < 2^31`: decompose `|exp| = int_n + rem`
        asm.bind_label(finite_mod_exp);
        asm.emit_str_sp(0, 0);  // base_lo
        asm.emit_str_sp(1, 4);  // base_hi
        asm.emit_str_sp(2, 8);  // abs_exp_lo
        asm.emit_str_sp(5, 12); // abs_exp_hi
        asm.emit_lsrs_imm(7, 3, 31);
        asm.emit_str_sp(7, 16); // exp_neg

        asm.emit_movs_reg(0, 2);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("__omni_f64_to_u64"); // r0 = int_n (<= 0x7FFF_FFFF), r1 = 0
        asm.emit_str_sp(0, 20);
        asm.emit_bl_sym("__omni_u64_to_f64"); // nf = r0:r1
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(3, 1);
        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_bl_sym("__omni_fsub64"); // rem = abs_exp - nf in r0:r1

        let non_int_exp = asm.new_label();
        let start_sq_loop = asm.new_label();
        asm.emit_lsls_imm(2, 1, 1);
        asm.emit_orrs(2, 0);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Ne, non_int_exp);

        // Exact integer `exp`: `acc = 1.0`
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x3FF0_0000);
        asm.emit_b_label_short(start_sq_loop);

        // Non-integer `exp` (e.g. `k + 0.5`): check negative `base` & initialize `acc = sqrt(base)`
        asm.bind_label(non_int_exp);
        asm.emit_movs_reg(6, 1); // save rem_hi
        asm.emit_ldr_sp(0, 0);
        asm.emit_ldr_sp(1, 4);
        let base_non_neg = asm.new_label();
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Pl, base_non_neg);
        // If `base == -0.0`, normalize to `+0.0`; if `base == -Inf`, normalize to `+Inf`; else finite negative -> `qNaN`!
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Ne, ret_qnan);
        asm.emit_lsls_imm(2, 1, 1);
        asm.emit_lsrs_imm(2, 2, 1);
        let chk_neg_inf = asm.new_label();
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Ne, chk_neg_inf);
        asm.emit_movs_imm8(1, 0); // -0.0 -> +0.0
        asm.emit_str_sp(1, 4);
        asm.emit_b_label_short(base_non_neg);
        asm.bind_label(chk_neg_inf);
        asm.emit_load_u32(3, 0x7FF0_0000);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_near(Cond::Ne, ret_qnan);
        asm.emit_movs_reg(1, 3); // -Inf -> +Inf for non-integer power
        asm.emit_str_sp(1, 4);

        asm.bind_label(base_non_neg);
        asm.emit_bl_sym("__omni_fsqrt64"); // acc = sqrt(base)
        // Bonus: if `rem == 0.25` (`0x3FD0_0000`), apply second `sqrt` for quarter powers!
        let sq_init_done = asm.new_label();
        asm.emit_load_u32(3, 0x3FD0_0000);
        asm.emit_cmp_reg(6, 3);
        asm.emit_bcond_label_short(Cond::Ne, sq_init_done);
        asm.emit_bl_sym("__omni_fsqrt64");
        asm.bind_label(sq_init_done);

        // Fast binary squaring loop for `acc * base^int_n`
        asm.bind_label(start_sq_loop);
        asm.emit_ldr_sp(4, 0);  // cur_lo = base_lo
        asm.emit_ldr_sp(5, 4);  // cur_hi = base_hi
        asm.emit_ldr_sp(6, 20); // int_n
        let lp = asm.new_label();
        let skip = asm.new_label();
        let lp_dn = asm.new_label();
        asm.bind_label(lp);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, lp_dn);
        asm.emit_movs_imm8(2, 1);
        asm.emit_tst_reg(6, 2);
        asm.emit_bcond_label_short(Cond::Eq, skip);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_fmul64");
        asm.bind_label(skip);
        asm.emit_lsrs_imm(6, 6, 1);
        asm.emit_bcond_label_short(Cond::Eq, lp_dn);
        asm.emit_push((1 << 0) | (1 << 1), false);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_pop((1 << 0) | (1 << 1), false);
        asm.emit_b_label_short(lp);

        asm.bind_label(lp_dn);
        asm.emit_ldr_sp(7, 16); // exp_neg
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_near(Cond::Eq, pow_exit);
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(3, 1);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x3FF0_0000);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_b_label_short(pow_exit);

        asm.bind_label(ret_one);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x3FF0_0000);
        asm.emit_b_label_short(pow_exit);

        asm.bind_label(ret_pos_inf);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF0_0000);
        asm.emit_b_label_short(pow_exit);

        asm.bind_label(ret_pos_zero);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_b_label_short(pow_exit);

        asm.bind_label(ret_qnan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF8_0000);

        asm.bind_label(pow_exit);
        asm.emit_add_sp(24);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // 15. `__omni_f32_to_f64(f32=r0) -> f64 r0:r1` (`fpext`)
    asm.begin_function("__omni_f32_to_f64");
    {
        asm.emit_push((1 << 4) | (1 << 5), false);
        asm.emit_lsrs_imm(3, 0, 31);
        asm.emit_lsls_imm(3, 3, 31); // sign_bit31
        asm.emit_lsls_imm(2, 0, 1);
        asm.emit_lsrs_imm(2, 2, 24); // exp8
        asm.emit_lsls_imm(4, 0, 9);
        asm.emit_lsrs_imm(4, 4, 9);  // frac23

        let not_zero_exp = asm.new_label();
        let inf_nan = asm.new_label();
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Ne, not_zero_exp);
        let sub_f32 = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Ne, sub_f32);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_reg(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(sub_f32);
        asm.emit_load_u32(2, 897); // 1 - 127 + 1023
        asm.emit_load_u32(5, 0x0080_0000);
        let sub_lp = asm.new_label();
        let pack_f32 = asm.new_label();
        asm.bind_label(sub_lp);
        asm.emit_tst_reg(4, 5);
        asm.emit_bcond_label_short(Cond::Ne, pack_f32);
        asm.emit_lsls_imm(4, 4, 1);
        asm.emit_subs_imm8(2, 1);
        asm.emit_b_label_short(sub_lp);
        asm.bind_label(pack_f32);
        asm.emit_bics(4, 5);
        asm.emit_lsls_imm(0, 4, 29);
        asm.emit_lsrs_imm(1, 4, 3);
        asm.emit_lsls_imm(2, 2, 20);
        asm.emit_orrs(1, 2);
        asm.emit_orrs(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(not_zero_exp);
        asm.emit_cmp_imm8(2, 0xFF);
        asm.emit_bcond_label_short(Cond::Eq, inf_nan);
        asm.emit_load_u32(5, 896); // 1023 - 127
        asm.emit_adds_reg(2, 2, 5);
        asm.emit_lsls_imm(0, 4, 29);
        asm.emit_lsrs_imm(1, 4, 3);
        asm.emit_lsls_imm(2, 2, 20);
        asm.emit_orrs(1, 2);
        asm.emit_orrs(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(inf_nan);
        asm.emit_movs_imm8(0, 0);
        asm.emit_load_u32(1, 0x7FF0_0000);
        let done_inf = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, done_inf);
        asm.emit_load_u32(5, 0x0008_0000);
        asm.emit_orrs(1, 5);
        asm.bind_label(done_inf);
        asm.emit_orrs(1, 3);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // 16. `__omni_f64_to_f32(f64=r0:r1) -> f32 r0` (`fptrunc`)
    asm.begin_function("__omni_f64_to_f32");
    {
        asm.emit_push((1 << 4) | (1 << 5), false);
        asm.emit_lsrs_imm(3, 1, 31);
        asm.emit_lsls_imm(3, 3, 31); // sign_bit31
        asm.emit_lsls_imm(2, 1, 1);
        asm.emit_lsrs_imm(2, 2, 21); // exp11
        asm.emit_lsls_imm(4, 1, 12);
        asm.emit_lsrs_imm(4, 4, 12); // frac_hi20

        let inf_nan = asm.new_label();
        let underflow = asm.new_label();
        let overflow = asm.new_label();
        asm.emit_load_u32(5, 0x07FF);
        asm.emit_cmp_reg(2, 5);
        asm.emit_bcond_label_short(Cond::Eq, inf_nan);
        asm.emit_load_u32(5, 896);
        asm.emit_cmp_reg(2, 5);
        asm.emit_bcond_label_short(Cond::Le, underflow);
        asm.emit_load_u32(5, 1151);
        asm.emit_cmp_reg(2, 5);
        asm.emit_bcond_label_short(Cond::Ge, overflow);

        asm.emit_load_u32(5, 896);
        asm.emit_subs_reg(2, 2, 5); // exp8
        asm.emit_lsls_imm(4, 4, 3);
        asm.emit_lsrs_imm(5, 0, 29);
        asm.emit_orrs(4, 5); // frac23
        // Round bit `(r0 >> 28) & 1`
        asm.emit_lsrs_imm(5, 0, 28);
        asm.emit_movs_imm8(1, 1);
        asm.emit_ands(5, 1);
        asm.emit_lsls_imm(2, 2, 23);
        asm.emit_orrs(2, 4);
        asm.emit_adds_reg(0, 2, 5);
        asm.emit_orrs(0, 3);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(underflow);
        asm.emit_movs_reg(0, 3);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(overflow);
        asm.emit_load_u32(0, 0x7F80_0000);
        asm.emit_orrs(0, 3);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);

        asm.bind_label(inf_nan);
        asm.emit_load_u32(2, 0x7F80_0000);
        asm.emit_orrs(4, 0);
        let done_in = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_label_short(Cond::Eq, done_in);
        asm.emit_load_u32(5, 0x0040_0000);
        asm.emit_orrs(2, 5);
        asm.bind_label(done_in);
        asm.emit_orrs(2, 3);
        asm.emit_movs_reg(0, 2);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), false);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    Ok(())
}

/// Format an IEEE-754 `f64` (`r0:r1`) into a heap-allocated decimal C string
/// (`"3.14"`, `"-0.5"`, `"42.0"`, `"1.0e+20"`, `"1.0e-5"`, `"nan"`, `"inf"`, `"-inf"`).
fn emit_float_to_str_helper(asm: &mut ThumbAssembler, base_vma: u32) -> Result<(), String> {
    asm.begin_function("__omni_float_to_str");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(32);
        // [sp+0]=out_buf, [sp+4]=cursor, [sp+8]=val_lo, [sp+12]=val_abs_hi,
        // [sp+16]=sci_mode, [sp+20]=exp10, [sp+24..32]=frac6 digits
        asm.emit_lsrs_imm(6, 1, 31); // sign
        asm.emit_lsls_imm(5, 1, 1);
        asm.emit_lsrs_imm(5, 5, 1);  // val_abs_hi
        asm.emit_str_sp(0, 8);
        asm.emit_str_sp(5, 12);
        asm.emit_movs_imm8(2, 0);
        asm.emit_str_sp(2, 16); // sci_mode = 0
        asm.emit_str_sp(2, 20); // exp10 = 0

        let fts_ret = asm.new_label();
        asm.emit_movs_imm8(0, 48);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_str_sp(0, 0);
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, fts_ret);
        asm.emit_movs_reg(4, 0); // cursor

        // Check NaN / Inf (`exp == 0x7FF`)
        let fts_finite = asm.new_label();
        let fts_nan = asm.new_label();
        asm.emit_ldr_sp(5, 12);
        asm.emit_lsrs_imm(2, 5, 20);
        asm.emit_load_u32(3, 0x07FF);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, fts_finite);

        asm.emit_load_u32(3, 0x7FF0_0000);
        asm.emit_cmp_reg(5, 3);
        asm.emit_bcond_label_short(Cond::Hi, fts_nan);
        asm.emit_ldr_sp(2, 8);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Ne, fts_nan);

        let inf_pos = asm.new_label();
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, inf_pos);
        asm.emit_movs_imm8(2, b'-');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.bind_label(inf_pos);
        asm.emit_movs_imm8(2, b'i');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_movs_imm8(2, b'n');
        asm.emit_strb_imm(2, 4, 1);
        asm.emit_movs_imm8(2, b'f');
        asm.emit_strb_imm(2, 4, 2);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 4, 3);
        asm.emit_b_label_short(fts_ret);

        asm.bind_label(fts_nan);
        asm.emit_movs_imm8(2, b'n');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_movs_imm8(2, b'a');
        asm.emit_strb_imm(2, 4, 1);
        asm.emit_movs_imm8(2, b'n');
        asm.emit_strb_imm(2, 4, 2);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 4, 3);
        asm.emit_b_label_short(fts_ret);

        asm.bind_label(fts_finite);
        let fts_pos = asm.new_label();
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, fts_pos);
        asm.emit_ldr_sp(2, 8);
        asm.emit_orrs(2, 5);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, fts_pos);
        asm.emit_movs_imm8(2, b'-');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);

        asm.bind_label(fts_pos);
        asm.emit_str_sp(4, 4); // save cursor

        // Check scientific notation thresholds on non-zero `|val|`:
        // `|val| >= 1e18` (`0x43AB_C16D : 0x674E_C800`) or `0 < |val| < 1e-4` (`0x3F1A_36E2 : 0xEB1C_432D`)
        let fts_scaled = asm.new_label();
        let chk_small_sci = asm.new_label();
        asm.emit_ldr_sp(4, 8);  // val_lo
        asm.emit_ldr_sp(5, 12); // val_abs_hi
        asm.emit_movs_reg(2, 4);
        asm.emit_orrs(2, 5);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_near(Cond::Eq, fts_scaled);

        asm.emit_load_u32(2, 0x674E_C800);
        asm.emit_load_u32(3, 0x43AB_C16D);
        asm.emit_subs_reg(2, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cc, chk_small_sci);

        // `|val| >= 1e18`: scale down into `[1.0, 10.0)`
        asm.emit_movs_imm8(2, 1);
        asm.emit_str_sp(2, 16); // sci_mode = 1
        asm.emit_movs_imm8(7, 0); // exp10 = 0
        let big_16_lp = asm.new_label();
        let big_1_lp = asm.new_label();
        let store_sci = asm.new_label();
        asm.bind_label(big_16_lp);
        asm.emit_load_u32(2, 0x37E0_8000);
        asm.emit_load_u32(3, 0x4341_C379); // 1e16
        asm.emit_subs_reg(6, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cc, big_1_lp);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_adds_imm8(7, 16);
        asm.emit_b_label_short(big_16_lp);

        asm.bind_label(big_1_lp);
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x4024_0000); // 10.0
        asm.emit_subs_reg(6, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cc, store_sci);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_adds_imm8(7, 1);
        asm.emit_b_label_short(big_1_lp);

        asm.bind_label(chk_small_sci);
        asm.emit_load_u32(2, 0xEB1C_432D);
        asm.emit_load_u32(3, 0x3F1A_36E2); // 1e-4
        asm.emit_subs_reg(2, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_near(Cond::Cs, fts_scaled);

        // `0 < |val| < 1e-4`: scale up into `[1.0, 10.0)`
        asm.emit_movs_imm8(2, 1);
        asm.emit_str_sp(2, 16); // sci_mode = 1
        asm.emit_movs_imm8(7, 0); // exp10 = 0
        let sml_16_lp = asm.new_label();
        let sml_1_lp = asm.new_label();
        let sml_chk10 = asm.new_label();
        asm.bind_label(sml_16_lp);
        asm.emit_load_u32(2, 0x97D8_89BC);
        asm.emit_load_u32(3, 0x3C9C_D2B2); // 1e-16
        asm.emit_subs_reg(6, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cs, sml_1_lp);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_load_u32(2, 0x37E0_8000);
        asm.emit_load_u32(3, 0x4341_C379); // 1e16
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_subs_imm8(7, 16);
        asm.emit_b_label_short(sml_16_lp);

        asm.bind_label(sml_1_lp);
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x3FF0_0000); // 1.0
        asm.emit_subs_reg(6, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cs, sml_chk10);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x4024_0000); // 10.0
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_subs_imm8(7, 1);
        asm.emit_b_label_short(sml_1_lp);

        asm.bind_label(sml_chk10);
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x4024_0000); // 10.0
        asm.emit_subs_reg(6, 4, 2);
        asm.emit_movs_reg(6, 5);
        asm.emit_sbcs(6, 3);
        asm.emit_bcond_label_short(Cond::Cc, store_sci);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1);
        asm.emit_adds_imm8(7, 1);

        asm.bind_label(store_sci);
        asm.emit_str_sp(4, 8);
        asm.emit_str_sp(5, 12);
        asm.emit_str_sp(7, 20);

        asm.bind_label(fts_scaled);
        // `int_part = __omni_f64_to_u64(|val|)` -> `r6:r7` (strictly `< 1e18 < 2^60`, so `r7 >= 0` always!)
        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_bl_sym("__omni_f64_to_u64");
        asm.emit_movs_reg(6, 0);
        asm.emit_movs_reg(7, 1);

        // `rem = |val| - (f64)int_part`
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_reg(3, 1);
        asm.emit_ldr_sp(0, 8);
        asm.emit_ldr_sp(1, 12);
        asm.emit_bl_sym("__omni_fsub64");
        let rem_ok = asm.new_label();
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Pl, rem_ok);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.bind_label(rem_ok);

        // `frac6 = (u64)(rem * 1_000_000.0 + 0.5)`
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x412E_8480);
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x3FE0_0000);
        asm.emit_bl_sym("__omni_fadd64");
        asm.emit_bl_sym("__omni_f64_to_u64");
        asm.emit_movs_reg(5, 0); // frac6

        let frac_ok = asm.new_label();
        asm.emit_load_u32(2, 1_000_000);
        asm.emit_cmp_reg(5, 2);
        asm.emit_bcond_label_short(Cond::Cc, frac_ok);
        asm.emit_subs_reg(5, 5, 2);
        asm.emit_adds_imm8(6, 1);
        asm.emit_movs_imm8(2, 0);
        asm.emit_adcs(7, 2);
        asm.bind_label(frac_ok);

        // If `sci_mode == 1` and rounding bumped `int_part` to `10`, normalize to `1.0` and `exp10 += 1`
        let sci_norm_ok = asm.new_label();
        asm.emit_ldr_sp(2, 16);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, sci_norm_ok);
        asm.emit_cmp_imm8(6, 10);
        asm.emit_bcond_label_short(Cond::Cc, sci_norm_ok);
        asm.emit_movs_imm8(6, 1);
        asm.emit_movs_imm8(7, 0);
        asm.emit_movs_imm8(5, 0);
        asm.emit_ldr_sp(2, 20);
        asm.emit_adds_imm8(2, 1);
        asm.emit_str_sp(2, 20);
        asm.bind_label(sci_norm_ok);

        // Write integer digits
        asm.emit_movs_reg(0, 6);
        asm.emit_movs_reg(1, 7);
        asm.emit_bl_sym("__omni_int_to_str");
        asm.emit_ldr_sp(4, 4);
        let cp_int_lp = asm.new_label();
        let cp_int_dn = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, cp_int_dn);
        asm.emit_movs_reg(1, 0);
        asm.bind_label(cp_int_lp);
        asm.emit_ldrb_imm(2, 1, 0);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, cp_int_dn);
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(1, 1);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(cp_int_lp);
        asm.bind_label(cp_int_dn);

        // Decimal point `'.'`
        asm.emit_movs_imm8(2, b'.');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);

        // Extract 6 decimal digits of `frac6` (`r5`) into `[sp+24..30]`
        asm.emit_movs_imm8(6, 6);
        let frac_dig_lp = asm.new_label();
        asm.bind_label(frac_dig_lp);
        asm.emit_subs_imm8(6, 1);
        asm.emit_movs_reg(0, 5);
        asm.emit_movs_imm8(1, 0);
        asm.emit_movs_imm8(2, 10);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_udivmod64");
        asm.emit_movs_reg(5, 0);
        asm.emit_adds_imm8(2, b'0');
        asm.emit_add_reg_sp(3, 24);
        asm.emit_adds_reg(3, 3, 6);
        asm.emit_strb_imm(2, 3, 0);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Ne, frac_dig_lp);

        // Trim trailing `'0'`s down to at least 1 digit (`3.0`, `3.14`, `0.125`)
        asm.emit_movs_imm8(7, 6);
        let trim_lp = asm.new_label();
        let trim_dn = asm.new_label();
        asm.bind_label(trim_lp);
        asm.emit_cmp_imm8(7, 1);
        asm.emit_bcond_label_short(Cond::Le, trim_dn);
        asm.emit_movs_reg(2, 7);
        asm.emit_subs_imm8(2, 1);
        asm.emit_add_reg_sp(3, 24);
        asm.emit_adds_reg(3, 3, 2);
        asm.emit_ldrb_imm(2, 3, 0);
        asm.emit_cmp_imm8(2, b'0');
        asm.emit_bcond_label_short(Cond::Ne, trim_dn);
        asm.emit_subs_imm8(7, 1);
        asm.emit_b_label_short(trim_lp);
        asm.bind_label(trim_dn);

        asm.emit_movs_imm8(6, 0);
        let cp_frac_lp = asm.new_label();
        let cp_frac_dn = asm.new_label();
        asm.bind_label(cp_frac_lp);
        asm.emit_cmp_reg(6, 7);
        asm.emit_bcond_label_short(Cond::Cs, cp_frac_dn);
        asm.emit_add_reg_sp(3, 24);
        asm.emit_adds_reg(3, 3, 6);
        asm.emit_ldrb_imm(2, 3, 0);
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.emit_adds_imm8(6, 1);
        asm.emit_b_label_short(cp_frac_lp);
        asm.bind_label(cp_frac_dn);

        // Optional `e+<exp>` / `e-<exp>` suffix when `sci_mode == 1`
        let fts_nul = asm.new_label();
        asm.emit_ldr_sp(2, 16);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, fts_nul);
        asm.emit_movs_imm8(2, b'e');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.emit_ldr_sp(0, 20); // exp10 (signed i32)
        let exp_pos_sign = asm.new_label();
        let exp_fmt = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Pl, exp_pos_sign);
        asm.emit_movs_imm8(2, b'-');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.emit_rsbs_zero(0, 0);
        asm.emit_b_label_short(exp_fmt);
        asm.bind_label(exp_pos_sign);
        asm.emit_movs_imm8(2, b'+');
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.bind_label(exp_fmt);
        asm.emit_str_sp(4, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_int_to_str");
        asm.emit_ldr_sp(4, 4);
        let cp_exp_lp = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, fts_nul);
        asm.emit_movs_reg(1, 0);
        asm.bind_label(cp_exp_lp);
        asm.emit_ldrb_imm(2, 1, 0);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, fts_nul);
        asm.emit_strb_imm(2, 4, 0);
        asm.emit_adds_imm8(1, 1);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(cp_exp_lp);

        asm.bind_label(fts_nul);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 4, 0);

        asm.bind_label(fts_ret);
        asm.emit_ldr_sp(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_add_sp(32);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_c_str_to_f64` / `atof` (`i8* s = r0`) -> `f64 r0:r1`
    // Skips leading ASCII whitespace and parses `[+|-]<digits>[.<digits>][e|E[+|-]<digits>]`!
    for parse_flt_fn in ["__omni_c_str_to_f64", "atof"] {
        asm.begin_function(parse_flt_fn);
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(16);
        asm.emit_movs_reg(4, 0); // ptr
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        let stf_done = asm.new_label();
        asm.emit_cmp_imm8(4, 0);
        asm.emit_bcond_near(Cond::Eq, stf_done);

        // Skip leading ASCII whitespace (`' '`, `'\t'..='\r'`)
        let ws_lp = asm.new_label();
        let ws_adv = asm.new_label();
        let ws_dn = asm.new_label();
        asm.bind_label(ws_lp);
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b' ');
        asm.emit_bcond_label_short(Cond::Eq, ws_adv);
        asm.emit_movs_reg(2, 5);
        asm.emit_subs_imm8(2, 9);
        asm.emit_cmp_imm8(2, 5);
        asm.emit_bcond_label_short(Cond::Cs, ws_dn);
        asm.bind_label(ws_adv);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(ws_lp);
        asm.bind_label(ws_dn);

        asm.emit_movs_imm8(7, 0); // neg = 0
        asm.emit_ldrb_imm(5, 4, 0);
        let chk_plus = asm.new_label();
        let sign_dn = asm.new_label();
        asm.emit_cmp_imm8(5, b'-');
        asm.emit_bcond_label_short(Cond::Ne, chk_plus);
        asm.emit_movs_imm8(7, 1);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(sign_dn);
        asm.bind_label(chk_plus);
        asm.emit_cmp_imm8(5, b'+');
        asm.emit_bcond_label_short(Cond::Ne, sign_dn);
        asm.emit_adds_imm8(4, 1);
        asm.bind_label(sign_dn);
        asm.emit_str_sp(7, 0); // [sp+0] = neg

        // Integer part accumulation (`r0:r1 = acc * 10.0 + digit`)
        let int_lp = asm.new_label();
        let int_dn = asm.new_label();
        asm.bind_label(int_lp);
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'0');
        asm.emit_bcond_label_short(Cond::Cc, int_dn);
        asm.emit_cmp_imm8(5, b'9' + 1);
        asm.emit_bcond_label_short(Cond::Cs, int_dn);
        asm.emit_subs_imm8(5, b'0');
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x4024_0000); // 10.0
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_str_sp(0, 8);
        asm.emit_str_sp(1, 12);
        asm.emit_movs_reg(0, 5);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_ldr_sp(2, 8);
        asm.emit_ldr_sp(3, 12);
        asm.emit_bl_sym("__omni_fadd64");
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(int_lp);

        asm.bind_label(int_dn);
        asm.emit_ldrb_imm(5, 4, 0);
        let chk_exp = asm.new_label();
        asm.emit_cmp_imm8(5, b'.');
        asm.emit_bcond_label_short(Cond::Ne, chk_exp);
        asm.emit_adds_imm8(4, 1);
        asm.emit_str_sp(0, 8);
        asm.emit_str_sp(1, 12); // save int_f64

        // Fractional digits: `frac_num` (`r6`), `frac_den` (`r7`, up to 10^9), advancing `r4` past all digits
        asm.emit_movs_imm8(6, 0);
        asm.emit_movs_imm8(7, 1);
        let frac_lp = asm.new_label();
        let frac_skip_acc = asm.new_label();
        let frac_dn = asm.new_label();
        asm.bind_label(frac_lp);
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'0');
        asm.emit_bcond_label_short(Cond::Cc, frac_dn);
        asm.emit_cmp_imm8(5, b'9' + 1);
        asm.emit_bcond_label_short(Cond::Cs, frac_dn);
        asm.emit_load_u32(2, 100_000_000);
        asm.emit_cmp_reg(7, 2);
        asm.emit_bcond_label_short(Cond::Cs, frac_skip_acc);
        asm.emit_subs_imm8(5, b'0');
        asm.emit_movs_imm8(2, 10);
        asm.emit_muls(6, 2);
        asm.emit_adds_reg(6, 6, 5);
        asm.emit_muls(7, 2);
        asm.bind_label(frac_skip_acc);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(frac_lp);

        asm.bind_label(frac_dn);
        asm.emit_str_sp(4, 4); // preserve `ptr` across `fdiv64`!
        asm.emit_movs_reg(0, 7);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_movs_reg(4, 0);
        asm.emit_movs_reg(5, 1); // den_f64
        asm.emit_movs_reg(0, 6);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("__omni_u64_to_f64");
        asm.emit_movs_reg(2, 4);
        asm.emit_movs_reg(3, 5);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.emit_ldr_sp(2, 8);
        asm.emit_ldr_sp(3, 12);
        asm.emit_bl_sym("__omni_fadd64");
        asm.emit_ldr_sp(4, 4); // restore `ptr`

        // Optional scientific exponent: `('e' | 'E') ['+' | '-'] <digits>`
        asm.bind_label(chk_exp);
        let apply_sign = asm.new_label();
        let has_exp = asm.new_label();
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'e');
        asm.emit_bcond_label_short(Cond::Eq, has_exp);
        asm.emit_cmp_imm8(5, b'E');
        asm.emit_bcond_near(Cond::Ne, apply_sign);

        asm.bind_label(has_exp);
        asm.emit_adds_imm8(4, 1);
        asm.emit_movs_imm8(7, 0); // exp_neg = 0
        asm.emit_ldrb_imm(5, 4, 0);
        let exp_chk_plus = asm.new_label();
        let exp_sign_dn = asm.new_label();
        asm.emit_cmp_imm8(5, b'-');
        asm.emit_bcond_label_short(Cond::Ne, exp_chk_plus);
        asm.emit_movs_imm8(7, 1);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(exp_sign_dn);
        asm.bind_label(exp_chk_plus);
        asm.emit_cmp_imm8(5, b'+');
        asm.emit_bcond_label_short(Cond::Ne, exp_sign_dn);
        asm.emit_adds_imm8(4, 1);
        asm.bind_label(exp_sign_dn);

        asm.emit_movs_imm8(6, 0); // exp_val
        let exp_dig_lp = asm.new_label();
        let exp_dig_skip = asm.new_label();
        let exp_dig_dn = asm.new_label();
        asm.bind_label(exp_dig_lp);
        asm.emit_ldrb_imm(5, 4, 0);
        asm.emit_cmp_imm8(5, b'0');
        asm.emit_bcond_label_short(Cond::Cc, exp_dig_dn);
        asm.emit_cmp_imm8(5, b'9' + 1);
        asm.emit_bcond_label_short(Cond::Cs, exp_dig_dn);
        asm.emit_load_u32(2, 400);
        asm.emit_cmp_reg(6, 2);
        asm.emit_bcond_label_short(Cond::Cs, exp_dig_skip);
        asm.emit_subs_imm8(5, b'0');
        asm.emit_movs_imm8(2, 10);
        asm.emit_muls(6, 2);
        asm.emit_adds_reg(6, 6, 5);
        asm.bind_label(exp_dig_skip);
        asm.emit_adds_imm8(4, 1);
        asm.emit_b_label_short(exp_dig_lp);
        asm.bind_label(exp_dig_dn);

        // Scale `r0:r1` by `10^(±exp_val)` using exact `1e16` chunks then `10.0` steps
        let exp16_lp = asm.new_label();
        let exp16_div = asm.new_label();
        let exp16_next = asm.new_label();
        let exp1_lp = asm.new_label();
        let exp1_div = asm.new_label();
        let exp1_next = asm.new_label();

        asm.bind_label(exp16_lp);
        asm.emit_cmp_imm8(6, 16);
        asm.emit_bcond_label_short(Cond::Cc, exp1_lp);
        asm.emit_subs_imm8(6, 16);
        asm.emit_load_u32(2, 0x37E0_8000);
        asm.emit_load_u32(3, 0x4341_C379); // 1e16
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Ne, exp16_div);
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_b_label_short(exp16_next);
        asm.bind_label(exp16_div);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.bind_label(exp16_next);
        asm.emit_b_label_short(exp16_lp);

        asm.bind_label(exp1_lp);
        asm.emit_cmp_imm8(6, 0);
        asm.emit_bcond_label_short(Cond::Eq, apply_sign);
        asm.emit_subs_imm8(6, 1);
        asm.emit_movs_imm8(2, 0);
        asm.emit_load_u32(3, 0x4024_0000); // 10.0
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Ne, exp1_div);
        asm.emit_bl_sym("__omni_fmul64");
        asm.emit_b_label_short(exp1_next);
        asm.bind_label(exp1_div);
        asm.emit_bl_sym("__omni_fdiv64");
        asm.bind_label(exp1_next);
        asm.emit_b_label_short(exp1_lp);

        asm.bind_label(apply_sign);
        asm.emit_ldr_sp(7, 0);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, stf_done);
        asm.emit_load_u32(2, 0x8000_0000);
        asm.emit_eors(1, 2);

        asm.bind_label(stf_done);
        asm.emit_add_sp(16);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.end_function(base_vma)?;
    }

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

    // --- Slices (`[0]=0x4F4D4EFF ('O','M','N',0xFF), [4]=rc(1), [8]=data_ptr, [12]=len, [16]=cap, [20]=0`) ---
    asm.begin_function("__omni_slice_create");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let oom_sl = asm.new_label();
        asm.emit_cmp_imm8(0, 4);
        let cap_ok = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cs, cap_ok);
        asm.emit_movs_imm8(0, 4);
        asm.bind_label(cap_ok);
        asm.emit_movs_reg(4, 0); // cap
        asm.emit_movs_imm8(0, 24);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oom_sl);
        asm.emit_movs_reg(5, 0); // hdr
        asm.emit_lsls_imm(0, 4, 3); // cap * 8
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oom_sl);
        asm.emit_load_u32(1, OMNI_SLICE_MAGIC32);
        asm.emit_str_imm(1, 5, 0);  // [hdr + 0] = 0x4F4D4EFF (byte 0 == 0xFF)
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 5, 4);  // [hdr + 4] = rc = 1
        asm.emit_str_imm(0, 5, 8);  // [hdr + 8] = data_ptr
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 5, 12); // [hdr + 12] = len = 0
        asm.emit_str_imm(4, 5, 16); // [hdr + 16] = cap
        asm.emit_movs_reg(0, 5);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(oom_sl);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_append(i8* s=r0, i64 val=r2:r3)`
    asm.begin_function("__omni_slice_append");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        let app_done = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, app_done);
        asm.emit_movs_reg(4, 0); // s
        asm.emit_movs_reg(5, 2); // val_lo
        asm.emit_movs_reg(6, 3); // val_hi
        asm.emit_ldr_imm(1, 4, 12); // len
        asm.emit_ldr_imm(2, 4, 16); // cap
        asm.emit_cmp_reg(1, 2);
        let has_room = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cc, has_room);
        // Grow `new_cap = cap * 2`
        asm.emit_lsls_imm(7, 2, 1);
        asm.emit_lsls_imm(0, 7, 3);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, app_done);
        asm.emit_push(1 << 0, false); // new_data
        asm.emit_ldr_imm(1, 4, 8);    // old_data
        asm.emit_ldr_imm(2, 4, 12);
        asm.emit_lsls_imm(2, 2, 3);   // len * 8
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 0, false);
        asm.emit_str_imm(0, 4, 8);
        asm.emit_str_imm(7, 4, 16);

        asm.bind_label(has_room);
        asm.emit_ldr_imm(0, 4, 8);  // data
        asm.emit_ldr_imm(1, 4, 12); // len
        asm.emit_lsls_imm(2, 1, 3);
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_str_imm(5, 0, 0);
        asm.emit_str_imm(6, 0, 4);
        asm.emit_adds_imm8(1, 1);
        asm.emit_str_imm(1, 4, 12);

        asm.bind_label(app_done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_get(i8* s=r0, i64 idx=r2:r3) -> i64 r0:r1`
    // Discriminates `OmniSlice*` (`0x4F4D4EFF`) from C `char*` identically to host!
    asm.begin_function("__omni_slice_get");
    {
        let oob = asm.new_label();
        let is_cstr = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oob);
        asm.emit_movs_imm8(1, 3);
        asm.emit_tst_reg(0, 1);
        asm.emit_bcond_label_short(Cond::Ne, is_cstr);
        asm.emit_ldr_imm(1, 0, 0);
        asm.emit_push(1 << 4, false);
        asm.emit_load_u32(4, OMNI_SLICE_MAGIC32);
        asm.emit_cmp_reg(1, 4);
        asm.emit_pop(1 << 4, false);
        asm.emit_bcond_label_short(Cond::Ne, is_cstr);

        asm.emit_ldr_imm(1, 0, 12); // len
        asm.emit_cmp_imm8(3, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(2, 2, 1);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(2, 1);
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_ldr_imm(0, 0, 8); // data
        asm.emit_lsls_imm(2, 2, 3);
        asm.emit_adds_reg(0, 0, 2);
        asm.emit_ldr_imm(1, 0, 4);
        asm.emit_ldr_imm(0, 0, 0);
        asm.emit_bx(14);

        asm.bind_label(is_cstr);
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_str_char_at");
        asm.emit_load_u32(2, chip.sram_base);
        asm.emit_str_imm(0, 2, SRAM_G_RET_PTR_OFF);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop(0, true);

        asm.bind_label(oob);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_set(i8* s=r0, i64 idx=r2:r3, i64 val=[entry_sp+0..8])`
    asm.begin_function("__omni_slice_set");
    {
        let oob = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oob);
        asm.emit_ldr_imm(1, 0, 12); // len
        asm.emit_cmp_imm8(3, 0);
        let non_neg = asm.new_label();
        asm.emit_bcond_label_short(Cond::Pl, non_neg);
        asm.emit_adds_reg(2, 2, 1);
        asm.bind_label(non_neg);
        asm.emit_cmp_reg(2, 1);
        asm.emit_bcond_label_short(Cond::Cs, oob);
        asm.emit_ldr_imm(0, 0, 8); // data
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

    // `__omni_slice_len(i8* s=r0) -> i64 r0:r1`
    // Discriminates `OmniSlice*` (`((uint8_t*)s)[0] == 0xFF`) from C `char*` identically to host!
    asm.begin_function("__omni_slice_len");
    {
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_str_len");
        asm.emit_pop(0, true);
    }
    asm.end_function(base_vma)?;

    // `__omni_slice_pop(i8* s=r0) -> i64`
    asm.begin_function("__omni_slice_pop");
    {
        let empty = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, empty);
        asm.emit_ldr_imm(1, 0, 12);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, empty);
        asm.emit_subs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 12);
        asm.emit_ldr_imm(0, 0, 8);
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

    // --- Reference Counting (`__omni_rc_retain`, `__omni_rc_release`), Heap Object Headers & Closures ---
    // `__omni_rc_retain(i8* p=r0) -> i8* r0`:
    // Retains `[r0, #4]` for `OmniSlice`/`OmniClosure`/`OmniMap` (`0xFF` magic) OR `[r0 - 12]` for `__omni_obj_alloc`!
    asm.begin_function("__omni_rc_retain");
    {
        let done = asm.new_label();
        let do_hdr_inc = asm.new_label();
        let chk_obj = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_lsrs_imm(2, 0, 28);
        asm.emit_cmp_imm8(2, 2); // SRAM `0x2000_0000..`
        asm.emit_bcond_label_short(Cond::Ne, done);

        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_SLICE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Eq, do_hdr_inc);
        asm.emit_load_u32(3, OMNI_CLOSURE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Eq, do_hdr_inc);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, chk_obj);

        asm.bind_label(do_hdr_inc);
        asm.emit_ldr_imm(2, 0, 4);
        asm.emit_adds_imm8(2, 1);
        asm.emit_str_imm(2, 0, 4);
        asm.emit_b_label_short(done);

        // Check `#[repr(C)]` Heap Object Header at `r0 - 16` (`magic == 0x4F424AFF && canary == 0x5A5A5A5A`)
        asm.bind_label(chk_obj);
        asm.emit_load_u32(3, chip.heap_start + 16);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cc, done);
        asm.emit_load_u32(3, chip.heap_limit);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cs, done);
        asm.emit_movs_reg(1, 0);
        asm.emit_subs_imm8(1, 16);
        asm.emit_ldr_imm(2, 1, 0);
        asm.emit_load_u32(3, OMNI_OBJ_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_ldr_imm(2, 1, 12);
        asm.emit_load_u32(3, OMNI_OBJ_CANARY32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_ldr_imm(2, 1, 4);
        asm.emit_adds_imm8(2, 1);
        asm.emit_str_imm(2, 1, 4);

        asm.bind_label(done);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_rc_release(i8* p=r0)`:
    // Decrements `[r0, #4]` for `OmniSlice`/`OmniClosure`/`OmniMap` OR `[r0 - 12]` for `__omni_obj_alloc` (clearing canary on 0)!
    asm.begin_function("__omni_rc_release");
    {
        let done = asm.new_label();
        let do_hdr_dec = asm.new_label();
        let chk_obj = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_lsrs_imm(2, 0, 28);
        asm.emit_cmp_imm8(2, 2);
        asm.emit_bcond_label_short(Cond::Ne, done);

        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_SLICE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Eq, do_hdr_dec);
        asm.emit_load_u32(3, OMNI_CLOSURE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Eq, do_hdr_dec);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, chk_obj);

        asm.bind_label(do_hdr_dec);
        asm.emit_ldr_imm(1, 0, 4);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_subs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 4);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_str_imm(1, 0, 0); // invalidate OmniMap magic when rc reaches 0
        asm.emit_b_label_short(done);

        // Check `#[repr(C)]` Heap Object Header at `r0 - 16`
        asm.bind_label(chk_obj);
        asm.emit_load_u32(3, chip.heap_start + 16);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cc, done);
        asm.emit_load_u32(3, chip.heap_limit);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cs, done);
        asm.emit_movs_reg(1, 0);
        asm.emit_subs_imm8(1, 16);
        asm.emit_ldr_imm(2, 1, 0);
        asm.emit_load_u32(3, OMNI_OBJ_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_ldr_imm(2, 1, 12);
        asm.emit_load_u32(3, OMNI_OBJ_CANARY32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_ldr_imm(2, 1, 4);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_subs_imm8(2, 1);
        asm.emit_str_imm(2, 1, 4);
        asm.emit_cmp_imm8(2, 0);
        asm.emit_bcond_label_short(Cond::Ne, done);
        asm.emit_str_imm(2, 1, 12); // clear canary `[r0 - 4] = 0` when `rc` reaches 0!

        asm.bind_label(done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_obj_alloc(i64 payload_bytes=r0:r1, i64 type_id=r2:r3) -> i8* r0`
    // Allocates `16 + payload_bytes` zeroed bytes, writes `[0]=0x4F424AFF, [4]=1, [8]=type_id, [12]=0x5A5A5A5A`, returns `hdr + 16`!
    asm.begin_function("__omni_obj_alloc");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let obj_oom = asm.new_label();
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Ne, obj_oom);
        asm.emit_load_u32(5, 0x0FFF_FFFF);
        asm.emit_cmp_reg(0, 5);
        asm.emit_bcond_label_short(Cond::Hi, obj_oom);
        asm.emit_movs_reg(4, 2); // type_id_lo
        asm.emit_adds_imm8(0, 16);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, obj_oom);

        asm.emit_load_u32(1, OMNI_OBJ_MAGIC32);
        asm.emit_str_imm(1, 0, 0);  // [hdr + 0] = 0x4F424AFF
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 4);  // [hdr + 4] = rc = 1
        asm.emit_str_imm(4, 0, 8);  // [hdr + 8] = type_id
        asm.emit_load_u32(1, OMNI_OBJ_CANARY32);
        asm.emit_str_imm(1, 0, 12); // [hdr + 12] = 0x5A5A5A5A
        asm.emit_adds_imm8(0, 16);  // payload_ptr = hdr + 16
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(obj_oom);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_obj_set_type(i8* ptr=r0, i64 type_id=r2:r3)`
    asm.begin_function("__omni_obj_set_type");
    {
        let inv = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, inv);
        asm.emit_movs_imm8(1, 3);
        asm.emit_tst_reg(0, 1);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_load_u32(3, chip.heap_start + 16);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cc, inv);
        asm.emit_load_u32(3, chip.heap_limit);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cs, inv);
        asm.emit_movs_reg(1, 0);
        asm.emit_subs_imm8(1, 16);
        asm.emit_ldr_imm(0, 1, 0);
        asm.emit_load_u32(3, OMNI_OBJ_MAGIC32);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_ldr_imm(0, 1, 12);
        asm.emit_load_u32(3, OMNI_OBJ_CANARY32);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_str_imm(2, 1, 8); // [ptr - 8] = type_id
        asm.bind_label(inv);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_obj_get_type(i8* ptr=r0) -> i64 r0:r1`
    asm.begin_function("__omni_obj_get_type");
    {
        let inv = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, inv);
        asm.emit_movs_imm8(1, 3);
        asm.emit_tst_reg(0, 1);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_load_u32(3, chip.heap_start + 16);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cc, inv);
        asm.emit_load_u32(3, chip.heap_limit);
        asm.emit_cmp_reg(0, 3);
        asm.emit_bcond_label_short(Cond::Cs, inv);
        asm.emit_movs_reg(1, 0);
        asm.emit_subs_imm8(1, 16);
        asm.emit_ldr_imm(2, 1, 0);
        asm.emit_load_u32(3, OMNI_OBJ_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_ldr_imm(2, 1, 12);
        asm.emit_load_u32(3, OMNI_OBJ_CANARY32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, inv);
        asm.emit_ldr_imm(0, 1, 8); // [ptr - 8]
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.bind_label(inv);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_closure_create(i8* fn_ptr=r0, i8* env_ptr=r1) -> i8* r0`
    // Layout: `malloc(16)` -> `[0]=0x434C4EFF, [4]=1 (rc), [8]=fn_ptr, [12]=env_ptr`
    asm.begin_function("__omni_closure_create");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        asm.emit_movs_reg(4, 0); // fn_ptr
        asm.emit_movs_reg(5, 1); // env_ptr
        asm.emit_movs_imm8(0, 16);
        asm.emit_bl_sym("malloc");
        let oom_cl = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oom_cl);
        asm.emit_load_u32(1, OMNI_CLOSURE_MAGIC32);
        asm.emit_str_imm(1, 0, 0);
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 0, 4);
        asm.emit_str_imm(4, 0, 8);
        asm.emit_str_imm(5, 0, 12);
        asm.bind_label(oom_cl);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_closure_is(i8* p=r0) -> i32/i64 r0:r1` (`1` if valid OmniClosure*, else `0`)
    asm.begin_function("__omni_closure_is");
    {
        let no_cl = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, no_cl);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, no_cl); // safely rejects odd Thumb function pointers (`addr | 1`)!
        asm.emit_ldrb_imm(2, 0, 0);
        asm.emit_cmp_imm8(2, 0xFF);
        asm.emit_bcond_label_short(Cond::Ne, no_cl);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_CLOSURE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, no_cl);
        asm.emit_movs_imm8(0, 1);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.bind_label(no_cl);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_closure_fn(i8* p=r0) -> i8* r0`: returns `[r0, #8]` if closure, else `r0` unchanged!
    asm.begin_function("__omni_closure_fn");
    {
        let raw_fn = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, raw_fn);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, raw_fn);
        asm.emit_ldrb_imm(2, 0, 0);
        asm.emit_cmp_imm8(2, 0xFF);
        asm.emit_bcond_label_short(Cond::Ne, raw_fn);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_CLOSURE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, raw_fn);
        asm.emit_ldr_imm(0, 0, 8);
        asm.bind_label(raw_fn);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_closure_env(i8* p=r0) -> i8* r0`: returns `[r0, #12]` if closure, else `0`!
    asm.begin_function("__omni_closure_env");
    {
        let no_env = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, no_env);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, no_env);
        asm.emit_ldrb_imm(2, 0, 0);
        asm.emit_cmp_imm8(2, 0xFF);
        asm.emit_bcond_label_short(Cond::Ne, no_env);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_CLOSURE_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, no_env);
        asm.emit_ldr_imm(0, 0, 12);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.bind_label(no_env);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_closure_destroy` / `__omni_slice_destroy` / `__omni_map_destroy` -> release reference count
    for destroy_fn in ["__omni_closure_destroy", "__omni_slice_destroy", "__omni_map_destroy"] {
        asm.begin_function(destroy_fn);
        asm.emit_push(0, true);
        asm.emit_bl_sym("__omni_rc_release");
        asm.emit_pop(0, true);
        asm.end_function(base_vma)?;
    }

    // --- `OmniMap` (`[0]=0x4D4150FF, [4]=rc(1), [8]=entries_ptr, [12]=cap, [16]=len` — 20B header, 16B entries) ---
    asm.begin_function("__omni_map_create");
    {
        asm.emit_push((1 << 4) | (1 << 5), true);
        let oom_map = asm.new_label();
        asm.emit_movs_imm8(4, 32); // 32 entries default capacity
        asm.emit_cmp_imm8(1, 0);
        let use_def_cap = asm.new_label();
        let cap_ready = asm.new_label();
        asm.emit_bcond_label_short(Cond::Ne, use_def_cap);
        asm.emit_cmp_imm8(0, 16);
        asm.emit_bcond_label_short(Cond::Cc, use_def_cap);
        asm.emit_load_u32(5, 4096);
        asm.emit_cmp_reg(0, 5);
        asm.emit_bcond_label_short(Cond::Hi, use_def_cap);
        asm.emit_movs_reg(4, 0);
        asm.emit_b_label_short(cap_ready);
        asm.bind_label(use_def_cap);
        asm.bind_label(cap_ready);

        asm.emit_movs_imm8(0, 24); // 20B header rounded to 8B alignment
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oom_map);
        asm.emit_movs_reg(5, 0); // hdr
        asm.emit_lsls_imm(0, 4, 4); // cap * 16B
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, oom_map);

        asm.emit_load_u32(1, OMNI_MAP_MAGIC32);
        asm.emit_str_imm(1, 5, 0);  // [hdr + 0]  = 0x4D4150FF (byte 0 == 0xFF)
        asm.emit_movs_imm8(1, 1);
        asm.emit_str_imm(1, 5, 4);  // [hdr + 4]  = rc = 1
        asm.emit_str_imm(0, 5, 8);  // [hdr + 8]  = entries_ptr
        asm.emit_str_imm(4, 5, 12); // [hdr + 12] = cap
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 5, 16); // [hdr + 16] = len = 0
        asm.emit_movs_reg(0, 5);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);

        asm.bind_label(oom_map);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_set(i8* m=r0, i8* key=r1, i64 val=r2:r3)`
    asm.begin_function("__omni_map_set");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_push((1 << 2) | (1 << 3), false); // save val_lo, val_hi at [sp+0], [sp+4]
        let map_set_exit = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, map_set_exit);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_near(Cond::Eq, map_set_exit);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_near(Cond::Ne, map_set_exit);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_near(Cond::Ne, map_set_exit);

        asm.emit_movs_reg(4, 0); // m
        asm.emit_movs_reg(5, 1); // key
        asm.emit_ldr_imm(6, 4, 8);  // entries
        asm.emit_ldr_imm(7, 4, 16); // len
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
        // Duplicate `key` onto SRAM heap so stack-allocated `_int` key buffers never dangle!
        asm.emit_movs_reg(0, 5);
        asm.emit_bl_sym("strlen");
        asm.emit_adds_imm8(0, 1);
        asm.emit_movs_reg(6, 0); // key_bytes
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_near(Cond::Eq, map_set_exit);
        asm.emit_push(1 << 0, false);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_reg(2, 6);
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 5, false); // r5 = dup_key

        asm.emit_ldr_imm(7, 4, 16); // len
        asm.emit_ldr_imm(0, 4, 12); // cap
        asm.emit_cmp_reg(7, 0);
        let has_cap = asm.new_label();
        asm.emit_bcond_label_short(Cond::Cc, has_cap);
        // Grow map entries x2
        asm.emit_lsls_imm(6, 0, 1); // new_cap
        asm.emit_lsls_imm(0, 6, 4); // new_cap * 16
        asm.emit_movs_imm8(1, 0);
        asm.emit_bl_sym("malloc");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, map_set_exit);
        asm.emit_str_imm(6, 4, 12); // update cap
        asm.emit_push(1 << 0, false);
        asm.emit_ldr_imm(1, 4, 8);  // old entries
        asm.emit_lsls_imm(2, 7, 4); // len * 16
        asm.emit_bl_sym("__omni_memcpy32");
        asm.emit_pop(1 << 0, false);
        asm.emit_str_imm(0, 4, 8);  // update entries_ptr

        asm.bind_label(has_cap);
        asm.emit_ldr_imm(0, 4, 8);
        asm.emit_lsls_imm(1, 7, 4);
        asm.emit_adds_reg(6, 0, 1); // target slot
        asm.emit_adds_imm8(7, 1);
        asm.emit_str_imm(7, 4, 16); // len + 1
        asm.emit_str_imm(5, 6, 0);  // slot.key = dup_key

        asm.bind_label(store_slot);
        asm.emit_ldr_sp(2, 0);
        asm.emit_ldr_sp(3, 4);
        asm.emit_str_imm(2, 6, 8);
        asm.emit_str_imm(3, 6, 12);

        asm.bind_label(map_set_exit);
        asm.emit_add_sp(8);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_get(i8* m=r0, i8* key=r1) -> i64 r0:r1`
    asm.begin_function("__omni_map_get");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true);
        let lp = asm.new_label();
        let found = asm.new_label();
        let miss = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, miss);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, miss);

        asm.emit_movs_reg(4, 1);    // key
        asm.emit_ldr_imm(5, 0, 8);  // entries
        asm.emit_ldr_imm(6, 0, 16); // len
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
        let lp = asm.new_label();
        let hit = asm.new_label();
        let miss = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, miss);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, miss);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, miss);

        asm.emit_movs_reg(4, 1);
        asm.emit_ldr_imm(5, 0, 8);
        asm.emit_ldr_imm(6, 0, 16);
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
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
        asm.bind_label(miss);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_delete(i8* m=r0, i8* key=r1)` (O(1) swap-remove on matching entry!)
    asm.begin_function("__omni_map_delete");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        let del_done = asm.new_label();
        let lp = asm.new_label();
        let del_found = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, del_done);
        asm.emit_cmp_imm8(1, 0);
        asm.emit_bcond_label_short(Cond::Eq, del_done);
        asm.emit_movs_imm8(2, 3);
        asm.emit_tst_reg(0, 2);
        asm.emit_bcond_label_short(Cond::Ne, del_done);
        asm.emit_ldr_imm(2, 0, 0);
        asm.emit_load_u32(3, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(2, 3);
        asm.emit_bcond_label_short(Cond::Ne, del_done);

        asm.emit_movs_reg(4, 0);    // m
        asm.emit_movs_reg(5, 1);    // key
        asm.emit_ldr_imm(6, 4, 8);  // cur entry
        asm.emit_ldr_imm(7, 4, 16); // remaining
        asm.bind_label(lp);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, del_done);
        asm.emit_ldr_imm(0, 6, 0);
        asm.emit_movs_reg(1, 5);
        asm.emit_bl_sym("strcmp");
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, del_found);
        asm.emit_adds_imm8(6, 16);
        asm.emit_subs_imm8(7, 1);
        asm.emit_b_label_short(lp);

        asm.bind_label(del_found);
        asm.emit_ldr_imm(1, 4, 16);
        asm.emit_subs_imm8(1, 1);
        asm.emit_str_imm(1, 4, 16); // len - 1
        asm.emit_ldr_imm(2, 4, 8);
        asm.emit_lsls_imm(1, 1, 4);
        asm.emit_adds_reg(2, 2, 1); // last_entry = entries + (len - 1) * 16
        asm.emit_ldr_imm(0, 2, 0);
        asm.emit_str_imm(0, 6, 0);
        asm.emit_ldr_imm(0, 2, 4);
        asm.emit_str_imm(0, 6, 4);
        asm.emit_ldr_imm(0, 2, 8);
        asm.emit_str_imm(0, 6, 8);
        asm.emit_ldr_imm(0, 2, 12);
        asm.emit_str_imm(0, 6, 12);

        asm.bind_label(del_done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_len(i8* m=r0) -> i64 r0:r1`
    asm.begin_function("__omni_map_len");
    {
        let map_len_zero = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, map_len_zero);
        asm.emit_movs_imm8(1, 3);
        asm.emit_tst_reg(0, 1);
        asm.emit_bcond_label_short(Cond::Ne, map_len_zero);
        asm.emit_ldr_imm(1, 0, 0);
        asm.emit_load_u32(2, OMNI_MAP_MAGIC32);
        asm.emit_cmp_reg(1, 2);
        asm.emit_bcond_label_short(Cond::Ne, map_len_zero);
        asm.emit_ldr_imm(0, 0, 16);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
        asm.bind_label(map_len_zero);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // Internal zero-heap helper `__omni_fmt_int_key(i8* buf=r0, i64 key=r2:r3)`:
    // Formats `"\x01i:<signed_i64>\0"` into caller's 32-byte stack buffer (`m[42] != m["42"]` domain separation!).
    asm.begin_function("__omni_fmt_int_key");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit_sub_sp(24);
        asm.emit_movs_imm8(1, 0x01);
        asm.emit_strb_imm(1, 0, 0);
        asm.emit_movs_imm8(1, b'i');
        asm.emit_strb_imm(1, 0, 1);
        asm.emit_movs_imm8(1, b':');
        asm.emit_strb_imm(1, 0, 2);
        asm.emit_adds_imm8(0, 3);
        asm.emit_movs_reg(6, 0); // write_cursor = buf + 3
        asm.emit_movs_reg(4, 2); // key_lo
        asm.emit_movs_reg(5, 3); // key_hi

        let key_pos = asm.new_label();
        asm.emit_cmp_imm8(5, 0);
        asm.emit_bcond_label_short(Cond::Pl, key_pos);
        asm.emit_movs_imm8(1, b'-');
        asm.emit_strb_imm(1, 6, 0);
        asm.emit_adds_imm8(6, 1);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_subs_reg(4, 0, 4);
        asm.emit_sbcs(1, 5);
        asm.emit_movs_reg(5, 1);
        asm.bind_label(key_pos);

        asm.emit_movs_imm8(7, 0); // digit_count
        let dig_lp = asm.new_label();
        asm.bind_label(dig_lp);
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_reg(1, 5);
        asm.emit_movs_imm8(2, 10);
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_udivmod64");
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
        asm.emit_bcond_label_short(Cond::Ne, dig_lp);

        let rev_lp = asm.new_label();
        let rev_dn = asm.new_label();
        asm.bind_label(rev_lp);
        asm.emit_cmp_imm8(7, 0);
        asm.emit_bcond_label_short(Cond::Eq, rev_dn);
        asm.emit_subs_imm8(7, 1);
        asm.emit_add_reg_sp(3, 0);
        asm.emit_adds_reg(3, 3, 7);
        asm.emit_ldrb_imm(2, 3, 0);
        asm.emit_strb_imm(2, 6, 0);
        asm.emit_adds_imm8(6, 1);
        asm.emit_b_label_short(rev_lp);
        asm.bind_label(rev_dn);
        asm.emit_movs_imm8(2, 0);
        asm.emit_strb_imm(2, 6, 0);

        asm.emit_add_sp(24);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_set_int(i8* m=r0, i64 key=r2:r3, i64 val=[entry_sp+0..8])`
    asm.begin_function("__omni_map_set_int");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6), true); // 16 bytes pushed
        asm.emit_sub_sp(32);                                 // 32B key buf -> entry_sp is at `[sp + 48]`!
        let msi_done = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, msi_done);
        asm.emit_movs_reg(4, 0); // m
        asm.emit_add_reg_sp(0, 0);
        asm.emit_bl_sym("__omni_fmt_int_key");
        asm.emit_movs_reg(0, 4);
        asm.emit_add_reg_sp(1, 0);
        asm.emit_ldr_sp(2, 48); // val_lo
        asm.emit_ldr_sp(3, 52); // val_hi
        asm.emit_bl_sym("__omni_map_set");
        asm.bind_label(msi_done);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_add_sp(32);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_map_get_int`, `__omni_map_contains_int`, `__omni_map_delete_int` (`m=r0`, `key=r2:r3`)
    for (int_fn, str_target) in [
        ("__omni_map_get_int", "__omni_map_get"),
        ("__omni_map_contains_int", "__omni_map_contains"),
        ("__omni_map_delete_int", "__omni_map_delete"),
    ] {
        asm.begin_function(int_fn);
        asm.emit_push(1 << 4, true);
        asm.emit_sub_sp(32);
        let mi_null = asm.new_label();
        let mi_done = asm.new_label();
        asm.emit_cmp_imm8(0, 0);
        asm.emit_bcond_label_short(Cond::Eq, mi_null);
        asm.emit_movs_reg(4, 0);
        asm.emit_add_reg_sp(0, 0);
        asm.emit_bl_sym("__omni_fmt_int_key");
        asm.emit_movs_reg(0, 4);
        asm.emit_add_reg_sp(1, 0);
        asm.emit_bl_sym(str_target);
        asm.emit_b_label_short(mi_done);
        asm.bind_label(mi_null);
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.bind_label(mi_done);
        asm.emit_add_sp(32);
        asm.emit_pop(1 << 4, true);
        asm.end_function(base_vma)?;
    }

    // Cooperative task launch (`__omni_go_spawn`, `__omni_thread_spawn`: `fn_ptr=r0, arg=r1`)
    for spawn_fn in ["__omni_go_spawn", "__omni_thread_spawn"] {
        asm.begin_function(spawn_fn);
        asm.emit_push(0, true);
        asm.emit_movs_reg(2, 0);
        asm.emit_movs_imm8(3, 1);
        asm.emit_orrs(2, 3); // ensure Thumb bit 0 is set for BLX
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
        "__omni_fiber_join_all",
        "__omni_slice_print",
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
        "__omni_input",
        "__omni_str_slice_step",
        "__omni_str_contains",
        "__omni_str_starts_with",
        "__omni_str_ends_with",
        "__omni_str_find",
        "__omni_str_upper",
        "__omni_str_lower",
        "__omni_str_trim",
        "__omni_str_replace",
        "__omni_str_split",
        "__omni_str_to_chars",
        "__omni_slice_subslice",
        "__omni_slice_contains",
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
