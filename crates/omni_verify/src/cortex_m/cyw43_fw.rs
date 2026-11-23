use super::chip::RpChipProfile;
use super::thumb_enc::{Cond, ThumbAssembler};

/// Official Infineon CYW43439 A0 Wi-Fi SoC firmware (`7.95.49`, 224,190 B padded to 224,192 B = 3,503 x 64 B blocks).
/// Lazily linked into XIP Flash `.rodata` ONLY when a bare-metal program references `cyw43_*` symbols.
pub const CYW43_43439_FW_BLOB: &[u8] = include_bytes!("cyw43_43439_fw.bin");

/// Raspberry Pi Pico W / Pico 2 W (`cyw943439wlpth_rev1_0`) NVRAM configuration image (743 B padded to 744 B).
pub const CYW43_43439_NVRAM_BLOB: &[u8] = include_bytes!("cyw43_43439_nvram.bin");

/// Official CYW43439 Raspberry Pi Pico W / Pico 2 W CLM regulatory blob (`984` B).
pub const CYW43_43439_CLM_BLOB: &[u8] = include_bytes!("cyw43_43439_clm.bin");

/// Compiled LLVM IR for the bare-metal OmniNet Wi-Fi + DHCPv4 + TCP + RFC 6455 WebSocket stack.
pub const CYW43_WS_STACK_IR: &str = include_str!("cyw43_ws_ir.ll");

pub const CYW43_SYMBOL_NAMES: &[&str] = &[
    "cyw43_init",
    "cyw43_gpio_put",
    "cyw43_led_init",
    "cyw43_led_put",
    "cyw_ll_clm_ptr",
    "cyw_ll_sram_scratch",
    "cyw_ll_pack_cmd",
    "cyw_ll_read_reg",
    "cyw_ll_write_reg",
    "cyw_ll_write_bp",
    "cyw_ll_gspi_write_u32",
    "cyw_ll_gspi_write_bytes",
    "cyw_ll_gspi_read_bytes",
    "wifi_connect",
    "wifi_ip",
    "ws_listen",
    "ws_poll",
    "ws_send",
    "ws_is_open",
    "tcp_listen",
    "tcp_poll",
    "tcp_send",
    "tcp_is_open",
    "tcp_close",
];

pub const CYW43_NET_SYMBOL_NAMES: &[&str] = &[
    "wifi_connect",
    "wifi_ip",
    "ws_listen",
    "ws_poll",
    "ws_send",
    "ws_is_open",
    "tcp_listen",
    "tcp_poll",
    "tcp_send",
    "tcp_is_open",
    "tcp_close",
];

/// Emit low-level hardware-speed Thumb gSPI primitives + Flash blob pointer accessors for CYW43439
/// (`GP23 = WL_ON`, `GP24 = WL_DIO`, `GP25 = WL_CS`, `GP29 = WL_CLK` on Pico W & Pico 2 W).
pub fn emit_cyw43_lowlevel(asm: &mut ThumbAssembler, chip: &RpChipProfile) -> Result<(), String> {
    let base_vma = chip.flash_base;

    // `__omni_cyw43_fw_ptr() -> i64`
    asm.begin_function("__omni_cyw43_fw_ptr");
    asm.emit_load_sym_addr(0, "__cyw43_fw_blob", false, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_cyw43_nvram_ptr() -> i64`
    asm.begin_function("__omni_cyw43_nvram_ptr");
    asm.emit_load_sym_addr(0, "__cyw43_nvram_blob", false, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_cyw43_clm_ptr() -> i64`
    asm.begin_function("__omni_cyw43_clm_ptr");
    asm.emit_load_sym_addr(0, "__cyw43_clm_blob", false, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_cyw43_pads_base() -> i64`
    asm.begin_function("__omni_cyw43_pads_base");
    asm.emit_load_u32(0, chip.pads_bank0_base);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_cyw43_sram_scratch() -> i64` (`0x2000_000C..0x2000_0020`, 20 bytes reserved runtime scratch)
    asm.begin_function("__omni_cyw43_sram_scratch");
    asm.emit_load_u32(0, chip.sram_base + 0x0C);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `str_ptr_noop(ptr_as_i64 = r0:r1) -> i64`
    asm.begin_function("str_ptr_noop");
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_cyw43_gspi_write_bytes(ptr = r0:r1, nbytes = r2:r3) -> i64`
    // Streams `nbytes` bytes from `ptr` MSB-first per byte over 3-wire gSPI (`GP24` = DIO, `GP29` = CLK)
    // using direct single-cycle `SIO` atomic SET/CLR registers (~12 MHz SPI @ 150 MHz CLK_SYS).
    asm.begin_function("__omni_cyw43_gspi_write_bytes");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit16(0xB672); // cpsid i (atomic <= 42 us gSPI burst)
        asm.emit_movs_reg(4, 0); // r4 = ptr
        asm.emit_movs_reg(5, 2); // r5 = nbytes
        asm.emit_load_u32(6, chip.sio_base); // r6 = SIO_BASE
        asm.emit_movs_imm8(7, 1);
        asm.emit_lsls_imm(7, 7, 24); // r7 = 1 << 24 (WL_DIO)
        asm.emit_str_imm(7, 6, chip.sio_gpio_oe_set); // GP24 output enable

        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_imm(3, 3, 29); // r3 = 1 << 29 (WL_CLK)

        let byte_loop = asm.new_label();
        let bit_loop = asm.new_label();
        let drive_zero = asm.new_label();
        let clk_pulse = asm.new_label();
        let done = asm.new_label();

        asm.bind_label(byte_loop);
        asm.emit_cmp_imm8(5, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_ldrb_imm(0, 4, 0); // r0 = *ptr
        asm.emit_adds_imm8(4, 1);
        asm.emit_subs_imm8(5, 1);
        asm.emit_movs_imm8(1, 0x80); // r1 = bit mask (MSB first)

        asm.bind_label(bit_loop);
        asm.emit_movs_reg(2, 0);
        asm.emit_ands(2, 1);
        asm.emit_bcond_label_short(Cond::Eq, drive_zero);
        asm.emit_str_imm(7, 6, chip.sio_gpio_out_set);
        asm.emit_b_label_short(clk_pulse);

        asm.bind_label(drive_zero);
        asm.emit_str_imm(7, 6, chip.sio_gpio_out_clr);

        asm.bind_label(clk_pulse);
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_set); // WL_CLK = 1 (rising edge)
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_clr); // WL_CLK = 0 (falling edge)
        asm.emit_lsrs_imm(1, 1, 1);
        asm.emit_bcond_label_short(Cond::Ne, bit_loop);
        asm.emit_b_label_short(byte_loop);

        asm.bind_label(done);
        asm.emit_str_imm(7, 6, chip.sio_gpio_oe_clr); // Return GP24 (WL_DIO) to input!
        asm.emit16(0xB662); // cpsie i
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_cyw43_gspi_write_u32(word = r0:r1) -> i64`
    // Stores little-endian `u32` `[b0, b1, b2, b3]` onto 8-byte aligned stack and streams 4 bytes.
    asm.begin_function("__omni_cyw43_gspi_write_u32");
    {
        asm.emit_push(1 << 0, true); // [sp + 0] = r0 (`word`), [sp + 4] = lr (8-byte AAPCS32 aligned)
        asm.emit_mov_reg(0, 13);     // r0 = sp
        asm.emit_movs_imm8(1, 0);
        asm.emit_movs_imm8(2, 4);    // nbytes = 4
        asm.emit_movs_imm8(3, 0);
        asm.emit_bl_sym("__omni_cyw43_gspi_write_bytes");
        asm.emit_pop(1 << 2, true);  // discard stacked word into caller-saved r2, return via pc
    }
    asm.end_function(base_vma)?;

    // `__omni_cyw43_gspi_read_u32() -> i64`
    // Reads 4 little-endian bytes (each byte MSB-first) from `GP24` (`WL_DIO`) in normal-speed falling-edge mode.
    asm.begin_function("__omni_cyw43_gspi_read_u32");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit16(0xB672); // cpsid i
        asm.emit_load_u32(6, chip.sio_base);
        asm.emit_movs_imm8(7, 1);
        asm.emit_lsls_imm(7, 7, 24);
        asm.emit_str_imm(7, 6, chip.sio_gpio_oe_clr); // Ensure GP24 is input
        for _ in 0..6 {
            asm.emit_nop(); // Bus turnaround settle (>40 ns @ 150 MHz)
        }

        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_imm(3, 3, 29); // r3 = 1 << 29 (WL_CLK)
        asm.emit_movs_imm8(4, 0);    // r4 = result
        asm.emit_movs_imm8(5, 0);    // r5 = byte_shift (0, 8, 16, 24)
        asm.emit_movs_imm8(7, 1);    // r7 = 1 bit mask

        let byte_loop = asm.new_label();
        let bit_loop = asm.new_label();

        asm.bind_label(byte_loop);
        asm.emit_movs_imm8(0, 0); // cur_byte = 0
        asm.emit_movs_imm8(1, 8); // bits_left = 8

        asm.bind_label(bit_loop);
        for _ in 0..6 {
            asm.emit_nop(); // >60 ns total falling-edge -> sample margin (CYW43439 t_CO + RP2350 2-FF sync)
        }
        asm.emit_ldr_imm(2, 6, chip.sio_gpio_in);      // Sample WL_DIO before rising clock edge
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_set); // WL_CLK = 1
        asm.emit_lsrs_imm(2, 2, 24);
        asm.emit_ands(2, 7);
        asm.emit_lsls_imm(0, 0, 1);
        asm.emit_orrs(0, 2);
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_clr); // WL_CLK = 0 (CYW43439 shifts next bit on falling edge)
        asm.emit_subs_imm8(1, 1);
        asm.emit_bcond_label_short(Cond::Ne, bit_loop);

        asm.emit_lsls_reg(0, 5);
        asm.emit_orrs(4, 0);
        asm.emit_adds_imm8(5, 8);
        asm.emit_cmp_imm8(5, 32);
        asm.emit_bcond_label_short(Cond::Ne, byte_loop);

        asm.emit16(0xB662); // cpsie i
        asm.emit_movs_reg(0, 4);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    // `__omni_cyw43_gspi_read_bytes(dst_ptr = r0:r1, nbytes = r2:r3) -> i64`
    // Streams `nbytes` bytes from `GP24` (`WL_DIO`) into `dst_ptr[0..nbytes]` in normal falling-edge mode.
    asm.begin_function("__omni_cyw43_gspi_read_bytes");
    {
        asm.emit_push((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
        asm.emit16(0xB672); // cpsid i
        asm.emit_movs_reg(4, 0); // r4 = dst_ptr
        asm.emit_movs_reg(5, 2); // r5 = nbytes
        asm.emit_load_u32(6, chip.sio_base);
        asm.emit_movs_imm8(7, 1);
        asm.emit_lsls_imm(7, 7, 24);
        asm.emit_str_imm(7, 6, chip.sio_gpio_oe_clr); // Ensure GP24 is input
        for _ in 0..6 {
            asm.emit_nop();
        }

        asm.emit_movs_imm8(3, 1);
        asm.emit_lsls_imm(3, 3, 29); // r3 = 1 << 29 (WL_CLK)
        asm.emit_movs_imm8(7, 1);    // r7 = 1 bit mask

        let byte_loop = asm.new_label();
        let bit_loop = asm.new_label();
        let done = asm.new_label();

        asm.bind_label(byte_loop);
        asm.emit_cmp_imm8(5, 0);
        asm.emit_bcond_label_short(Cond::Eq, done);
        asm.emit_movs_imm8(0, 0); // cur_byte = 0
        asm.emit_movs_imm8(1, 8); // bits_left = 8

        asm.bind_label(bit_loop);
        for _ in 0..6 {
            asm.emit_nop();
        }
        asm.emit_ldr_imm(2, 6, chip.sio_gpio_in);
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_set); // WL_CLK = 1
        asm.emit_lsrs_imm(2, 2, 24);
        asm.emit_ands(2, 7);
        asm.emit_lsls_imm(0, 0, 1);
        asm.emit_orrs(0, 2);
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_nop();
        asm.emit_str_imm(3, 6, chip.sio_gpio_out_clr); // WL_CLK = 0
        asm.emit_subs_imm8(1, 1);
        asm.emit_bcond_label_short(Cond::Ne, bit_loop);

        asm.emit_strb_imm(0, 4, 0);
        asm.emit_adds_imm8(4, 1);
        asm.emit_subs_imm8(5, 1);
        asm.emit_b_label_short(byte_loop);

        asm.bind_label(done);
        asm.emit16(0xB662); // cpsie i
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_pop((1 << 4) | (1 << 5) | (1 << 6) | (1 << 7), true);
    }
    asm.end_function(base_vma)?;

    Ok(())
}

/// Emit thin Thumb entrypoints for `wifi_connect`, `wifi_ip`, `ws_listen`, `ws_poll`, `ws_send`, `ws_is_open`
/// bridging user `.omni` calls to the compiled bare-metal OmniNet WebSocket stack (`cyw43_ws_ir.ll`).
pub fn emit_cyw43_net_entrypoints(
    asm: &mut ThumbAssembler,
    chip: &RpChipProfile,
) -> Result<(), String> {
    let base_vma = chip.flash_base;
    for (pub_sym, impl_sym) in [
        ("wifi_connect", "__omni_fn_cyw_wifi_connect"),
        ("wifi_ip", "__omni_fn_cyw_wifi_ip_ptr"),
        ("ws_listen", "__omni_fn_cyw_ws_listen"),
        ("ws_poll", "__omni_fn_cyw_ws_poll_ptr"),
        ("ws_send", "__omni_fn_cyw_ws_send"),
        ("ws_is_open", "__omni_fn_cyw_ws_is_open"),
        ("tcp_listen", "__omni_fn_cyw_tcp_listen"),
        ("tcp_poll", "__omni_fn_cyw_tcp_poll_ptr"),
        ("tcp_send", "__omni_fn_cyw_tcp_send"),
        ("tcp_is_open", "__omni_fn_cyw_tcp_is_open"),
        ("tcp_close", "__omni_fn_cyw_tcp_close"),
    ] {
        if !asm.symbols.contains_key(pub_sym) {
            asm.begin_function(pub_sym);
            asm.emit_push(1 << 4, true); // keep SP 8-byte AAPCS32 aligned
            asm.emit_sub_sp(32);         // 32-byte pad for 6-arg `@__omni_fn_*` prologue loads
            asm.emit_bl_sym(impl_sym);
            asm.emit_add_sp(32);
            asm.emit_pop(1 << 4, true);
            asm.end_function(base_vma)?;
        }
    }
    Ok(())
}

/// Complete bare-metal CYW43439 gSPI + Silicon Backplane + Firmware/NVRAM Boot + GPIO/LED Driver in LLVM IR,
/// lowered through the target-matched ARMv6-M (RP2040 Pico W) or ARMv8-M.main (RP2350 Pico 2 W) backend.
pub const CYW43_DRIVER_IR: &str = r#"
declare i64 @gpio_init(i64)
declare i64 @gpio_set_dir(i64, i64)
declare i64 @gpio_put(i64, i64)
declare i64 @mmio_read32(i64)
declare i64 @mmio_write32(i64, i64)
declare i64 @sleep_ms(i64)
declare i64 @sleep_us(i64)
declare i64 @__omni_cyw43_fw_ptr()
declare i64 @__omni_cyw43_nvram_ptr()
declare i64 @__omni_cyw43_pads_base()
declare i64 @__omni_cyw43_sram_scratch()
declare i64 @__omni_cyw43_gspi_write_bytes(i64, i64)
declare i64 @__omni_cyw43_gspi_write_u32(i64)
declare i64 @__omni_cyw43_gspi_read_u32()
declare i64 @__omni_fn_cyw_net_gpio_put(i64, i64, i64, i64, i64, i64)

define i64 @__omni_cyw43_swap16x2(i64 %x) {
entry:
  %s0 = lshr i64 %x, 8
  %b0 = and i64 %s0, 255
  %m1 = and i64 %x, 255
  %b1 = shl i64 %m1, 8
  %s2 = lshr i64 %x, 24
  %m2 = and i64 %s2, 255
  %b2 = shl i64 %m2, 16
  %s3 = lshr i64 %x, 16
  %m3 = and i64 %s3, 255
  %b3 = shl i64 %m3, 24
  %o01 = or i64 %b0, %b1
  %o23 = or i64 %b2, %b3
  %res = or i64 %o01, %o23
  ret i64 %res
}

define i64 @__omni_cyw43_pack_cmd(i64 %wr, i64 %inc, i64 %fn_num, i64 %addr, i64 %sz) {
entry:
  %wm = and i64 %wr, 1
  %w = shl i64 %wm, 31
  %im = and i64 %inc, 1
  %i = shl i64 %im, 30
  %fm = and i64 %fn_num, 3
  %f = shl i64 %fm, 28
  %am = and i64 %addr, 131071
  %a = shl i64 %am, 11
  %s = and i64 %sz, 2047
  %t0 = or i64 %w, %i
  %t1 = or i64 %t0, %f
  %t2 = or i64 %t1, %a
  %t3 = or i64 %t2, %s
  %res = and i64 %t3, 4294967295
  ret i64 %res
}

define i64 @__omni_cyw43_read_swapped(i64 %fn_num, i64 %addr, i64 %sz) {
entry:
  %raw_cmd = call i64 @__omni_cyw43_pack_cmd(i64 0, i64 1, i64 %fn_num, i64 %addr, i64 %sz)
  %cmd = call i64 @__omni_cyw43_swap16x2(i64 %raw_cmd)
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %w0 = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %raw_val = call i64 @__omni_cyw43_gspi_read_u32()
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  %val = call i64 @__omni_cyw43_swap16x2(i64 %raw_val)
  ret i64 %val
}

define i64 @__omni_cyw43_write_swapped(i64 %fn_num, i64 %addr, i64 %val, i64 %sz) {
entry:
  %raw_cmd = call i64 @__omni_cyw43_pack_cmd(i64 1, i64 1, i64 %fn_num, i64 %addr, i64 %sz)
  %cmd = call i64 @__omni_cyw43_swap16x2(i64 %raw_cmd)
  %pay = call i64 @__omni_cyw43_swap16x2(i64 %val)
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %w0 = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %w1 = call i64 @__omni_cyw43_gspi_write_u32(i64 %pay)
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  ret i64 0
}

define i64 @__omni_cyw43_write_reg(i64 %fn_num, i64 %addr, i64 %val, i64 %sz) {
entry:
  %cmd = call i64 @__omni_cyw43_pack_cmd(i64 1, i64 1, i64 %fn_num, i64 %addr, i64 %sz)
  %pay = and i64 %val, 4294967295
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %w0 = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %w1 = call i64 @__omni_cyw43_gspi_write_u32(i64 %pay)
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  ret i64 0
}

define i64 @__omni_cyw43_read_reg(i64 %fn_num, i64 %addr, i64 %sz) {
entry:
  %cmd = call i64 @__omni_cyw43_pack_cmd(i64 0, i64 1, i64 %fn_num, i64 %addr, i64 %sz)
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %w0 = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %is_f1 = icmp eq i64 %fn_num, 1
  br i1 %is_f1, label %pad_f1, label %read_val

pad_f1:
  %pad = call i64 @__omni_cyw43_gspi_read_u32()
  br label %read_val

read_val:
  %val = call i64 @__omni_cyw43_gspi_read_u32()
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  %is_1 = icmp eq i64 %sz, 1
  br i1 %is_1, label %ret_1, label %chk_2

ret_1:
  %v1 = and i64 %val, 255
  ret i64 %v1

chk_2:
  %is_2 = icmp eq i64 %sz, 2
  br i1 %is_2, label %ret_2, label %ret_4

ret_2:
  %v2 = and i64 %val, 65535
  ret i64 %v2

ret_4:
  ret i64 %val
}

define i64 @__omni_cyw43_set_window(i64 %addr) {
entry:
  %win = and i64 %addr, 4294934528
  %scr = call i64 @__omni_cyw43_sram_scratch()
  %cur = call i64 @mmio_read32(i64 %scr)
  %same = icmp eq i64 %win, %cur
  br i1 %same, label %done, label %upd

upd:
  %b_low_s = lshr i64 %win, 8
  %b_low = and i64 %b_low_s, 255
  %b_mid_s = lshr i64 %win, 16
  %b_mid = and i64 %b_mid_s, 255
  %b_hi_s = lshr i64 %win, 24
  %b_hi = and i64 %b_hi_s, 255
  %w0 = call i64 @__omni_cyw43_write_reg(i64 1, i64 65546, i64 %b_low, i64 1)
  %w1 = call i64 @__omni_cyw43_write_reg(i64 1, i64 65547, i64 %b_mid, i64 1)
  %w2 = call i64 @__omni_cyw43_write_reg(i64 1, i64 65548, i64 %b_hi, i64 1)
  %st = call i64 @mmio_write32(i64 %scr, i64 %win)
  br label %done

done:
  ret i64 0
}

define i64 @__omni_cyw43_write_bp(i64 %addr, i64 %val, i64 %sz) {
entry:
  %sw = call i64 @__omni_cyw43_set_window(i64 %addr)
  %off = and i64 %addr, 32767
  %a17 = or i64 %off, 32768
  %r = call i64 @__omni_cyw43_write_reg(i64 1, i64 %a17, i64 %val, i64 %sz)
  ret i64 %r
}

define i64 @__omni_cyw43_read_bp(i64 %addr, i64 %sz) {
entry:
  %sw = call i64 @__omni_cyw43_set_window(i64 %addr)
  %off = and i64 %addr, 32767
  %a17 = or i64 %off, 32768
  %r = call i64 @__omni_cyw43_read_reg(i64 1, i64 %a17, i64 %sz)
  ret i64 %r
}

define i64 @__omni_cyw43_reset_core(i64 %wrap_base) {
entry:
  %ioctrl = add i64 %wrap_base, 1032
  %rstctrl = add i64 %wrap_base, 2048
  %d0 = call i64 @__omni_cyw43_read_bp(i64 %rstctrl, i64 1)
  %r0 = call i64 @__omni_cyw43_read_bp(i64 %rstctrl, i64 1)
  %in_rst = and i64 %r0, 1
  %already = icmp ne i64 %in_rst, 0
  br i1 %already, label %do_up, label %put_rst

put_rst:
  %p0 = call i64 @__omni_cyw43_write_bp(i64 %ioctrl, i64 3, i64 1)
  %p1 = call i64 @__omni_cyw43_read_bp(i64 %ioctrl, i64 1)
  %s0 = call i64 @sleep_ms(i64 1)
  %p2 = call i64 @__omni_cyw43_write_bp(i64 %rstctrl, i64 1, i64 1)
  %s1 = call i64 @sleep_ms(i64 1)
  br label %do_up

do_up:
  %u0 = call i64 @__omni_cyw43_write_bp(i64 %ioctrl, i64 3, i64 1)
  %u1 = call i64 @__omni_cyw43_read_bp(i64 %ioctrl, i64 1)
  %u2 = call i64 @__omni_cyw43_write_bp(i64 %rstctrl, i64 0, i64 1)
  %s2 = call i64 @sleep_ms(i64 1)
  %u3 = call i64 @__omni_cyw43_write_bp(i64 %ioctrl, i64 1, i64 1)
  %u4 = call i64 @__omni_cyw43_read_bp(i64 %ioctrl, i64 1)
  %s3 = call i64 @sleep_ms(i64 1)
  ret i64 0
}

define i64 @__omni_cyw43_upload_blob(i64 %dest_base, i64 %src_ptr, i64 %total_len) {
entry:
  %off_slot = alloca i64
  store i64 0, i64* %off_slot
  br label %loop_cond

loop_cond:
  %off = load i64, i64* %off_slot
  %more = icmp slt i64 %off, %total_len
  br i1 %more, label %loop_body, label %done

loop_body:
  %rem = sub i64 %total_len, %off
  %gt64 = icmp sgt i64 %rem, 64
  br i1 %gt64, label %sz_64, label %sz_rem

sz_64:
  br label %send_blk

sz_rem:
  br label %send_blk

send_blk:
  %blk = phi i64 [ 64, %sz_64 ], [ %rem, %sz_rem ]
  %dest = add i64 %dest_base, %off
  %sw = call i64 @__omni_cyw43_set_window(i64 %dest)
  %doff = and i64 %dest, 32767
  %a17 = or i64 %doff, 32768
  %cmd = call i64 @__omni_cyw43_pack_cmd(i64 1, i64 1, i64 1, i64 %a17, i64 %blk)
  %sptr = add i64 %src_ptr, %off
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %wc = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %wb = call i64 @__omni_cyw43_gspi_write_bytes(i64 %sptr, i64 %blk)
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  %next_off = add i64 %off, %blk
  store i64 %next_off, i64* %off_slot
  br label %loop_cond

done:
  ret i64 0
}

define i64 @cyw43_init() {
entry:
  %i_slot = alloca i64

  ; 1. Initialize pins: GP23 (WL_ON=0), GP25 (WL_CS=1), GP29 (WL_CLK=0), GP24 (WL_DIO=0 strap)
  %g23 = call i64 @gpio_init(i64 23)
  %d23 = call i64 @gpio_set_dir(i64 23, i64 1)
  %p23 = call i64 @gpio_put(i64 23, i64 0)

  %g25 = call i64 @gpio_init(i64 25)
  %d25 = call i64 @gpio_set_dir(i64 25, i64 1)
  %p25 = call i64 @gpio_put(i64 25, i64 1)

  %g29 = call i64 @gpio_init(i64 29)
  %d29 = call i64 @gpio_set_dir(i64 29, i64 1)
  %p29 = call i64 @gpio_put(i64 29, i64 0)

  %g24 = call i64 @gpio_init(i64 24)
  %d24 = call i64 @gpio_set_dir(i64 24, i64 1)
  %p24 = call i64 @gpio_put(i64 24, i64 0)

  ; Apply 12mA pads + CRITICAL pull-down (0x76) on GP24 (WL_DIO) so CYW43439 straps into gSPI mode!
  %pads = call i64 @__omni_cyw43_pads_base()
  %pad23 = add i64 %pads, 96
  %pad24 = add i64 %pads, 100
  %pad25 = add i64 %pads, 104
  %pad29 = add i64 %pads, 120
  %mp23 = call i64 @mmio_write32(i64 %pad23, i64 122)
  %mp24 = call i64 @mmio_write32(i64 %pad24, i64 118)
  %mp25 = call i64 @mmio_write32(i64 %pad25, i64 122)
  %mp29 = call i64 @mmio_write32(i64 %pad29, i64 119)

  ; Reset runtime scratch state: [0]=cur_window=0xFFFFFFFF, [4]=seq=1, [8]=reqid=1, [12]=gpioout(0)|(gpioouten(1)<<8)=256
  ; Note: [16] is reserved for `net_ctx_ptr` (left untouched here so `wifi_connect` can pre-allocate `net_ctx`)
  %scr = call i64 @__omni_cyw43_sram_scratch()
  %scr4 = add i64 %scr, 4
  %scr8 = add i64 %scr, 8
  %scr12 = add i64 %scr, 12
  %ms0 = call i64 @mmio_write32(i64 %scr, i64 4294967295)
  %ms1 = call i64 @mmio_write32(i64 %scr4, i64 1)
  %ms2 = call i64 @mmio_write32(i64 %scr8, i64 1)
  %ms3 = call i64 @mmio_write32(i64 %scr12, i64 256)

  ; Power-on reset pulse (20 ms low, 250 ms settle high)
  %sl0 = call i64 @sleep_ms(i64 20)
  %pon = call i64 @gpio_put(i64 23, i64 1)
  %sl1 = call i64 @sleep_ms(i64 250)
  %din24 = call i64 @gpio_set_dir(i64 24, i64 0)

  ; 2. Poll SPI_READ_TEST_REGISTER (0x14) == 0xFEEDBEAD (4276993709) in initial 16-bit big-endian mode
  store i64 0, i64* %i_slot
  br label %poll_test

poll_test:
  %t_idx = load i64, i64* %i_slot
  %t_ok = icmp slt i64 %t_idx, 50
  br i1 %t_ok, label %poll_test_body, label %err_test

poll_test_body:
  %tv = call i64 @__omni_cyw43_read_swapped(i64 0, i64 20, i64 4)
  %t_match = icmp eq i64 %tv, 4276993709
  br i1 %t_match, label %bus_switch, label %poll_test_next

poll_test_next:
  %tsl = call i64 @sleep_ms(i64 2)
  %t_nxt = add i64 %t_idx, 1
  store i64 %t_nxt, i64* %i_slot
  br label %poll_test

err_test:
  ret i64 -1

bus_switch:
  ; 3. Switch gSPI to 32-bit word mode, normal falling-edge MISO, ENDIAN_BIG (0x000200A3 = 131235)
  %ws = call i64 @__omni_cyw43_write_swapped(i64 0, i64 0, i64 131235, i64 4)
  %v32 = call i64 @__omni_cyw43_read_reg(i64 0, i64 20, i64 4)
  %v32_ok = icmp eq i64 %v32, 4276993709
  br i1 %v32_ok, label %bus_cfg, label %err_swap

err_swap:
  ret i64 -2

bus_cfg:
  %bc0 = call i64 @__omni_cyw43_read_reg(i64 0, i64 0, i64 4)
  %bc1 = call i64 @__omni_cyw43_write_reg(i64 0, i64 29, i64 4, i64 1)
  %bc2 = call i64 @__omni_cyw43_write_reg(i64 0, i64 4, i64 153, i64 2)
  %bc3 = call i64 @__omni_cyw43_write_reg(i64 0, i64 6, i64 190, i64 2)

  ; 4. Enable Silicon Backplane ALP clock (0x1000E = 65550 <- 0x08, wait 0x40)
  %alp_req = call i64 @__omni_cyw43_write_reg(i64 1, i64 65550, i64 8, i64 1)
  store i64 0, i64* %i_slot
  br label %poll_alp

poll_alp:
  %a_idx = load i64, i64* %i_slot
  %a_ok = icmp slt i64 %a_idx, 100
  br i1 %a_ok, label %poll_alp_body, label %err_alp

poll_alp_body:
  %acsr = call i64 @__omni_cyw43_read_reg(i64 1, i64 65550, i64 1)
  %amask = and i64 %acsr, 64
  %a_rdy = icmp ne i64 %amask, 0
  br i1 %a_rdy, label %alp_up, label %poll_alp_next

poll_alp_next:
  %asl = call i64 @sleep_ms(i64 1)
  %a_nxt = add i64 %a_idx, 1
  store i64 %a_nxt, i64* %i_slot
  br label %poll_alp

err_alp:
  ret i64 -3

alp_up:
  %alp_clr = call i64 @__omni_cyw43_write_reg(i64 1, i64 65550, i64 0, i64 1)

  ; 5. Hold WLAN ARM CM3 (0x18103000 = 403714048) in reset if running, then reset SOCSRAM (0x18104000 = 403718144)
  %arm_rst = call i64 @__omni_cyw43_read_bp(i64 403716096, i64 1)
  %arm_in = and i64 %arm_rst, 1
  %arm_ok = icmp ne i64 %arm_in, 0
  br i1 %arm_ok, label %reset_sram, label %halt_arm

halt_arm:
  %ha0 = call i64 @__omni_cyw43_write_bp(i64 403715080, i64 3, i64 1)
  %ha1 = call i64 @__omni_cyw43_read_bp(i64 403715080, i64 1)
  %ha2 = call i64 @sleep_ms(i64 1)
  %ha3 = call i64 @__omni_cyw43_write_bp(i64 403716096, i64 1, i64 1)
  %ha4 = call i64 @sleep_ms(i64 1)
  br label %reset_sram

reset_sram:
  %rs = call i64 @__omni_cyw43_reset_core(i64 403718144)

  ; Disable remap for SRAM_3: SOCSRAM_BANKX_INDEX (0x18004010 = 402669584) = 3, PDA (0x18004044 = 402669636) = 0
  %sb0 = call i64 @__omni_cyw43_write_bp(i64 402669584, i64 3, i64 4)
  %sb1 = call i64 @__omni_cyw43_write_bp(i64 402669636, i64 0, i64 4)

  ; 6. Download CYW43439 A0 firmware (224,192 B -> 0x00000000) & NVRAM (744 B -> 0x7FD14 = 523540)
  %fw_p = call i64 @__omni_cyw43_fw_ptr()
  %ul_fw = call i64 @__omni_cyw43_upload_blob(i64 0, i64 %fw_p, i64 224192)
  %fw_settle = call i64 @sleep_ms(i64 5)

  %nv_p = call i64 @__omni_cyw43_nvram_ptr()
  %ul_nv = call i64 @__omni_cyw43_upload_blob(i64 523540, i64 %nv_p, i64 744)
  ; Write NVRAM size token 0xFF4500BA (4282712250) to top of 512 KB SRAM - 4 (0x7FFFC = 524284)
  %nv_tok = call i64 @__omni_cyw43_write_bp(i64 524284, i64 4282712250, i64 4)

  ; 7. Release WLAN ARM CM3 core (0x18103000 = 403714048) and wait for HT clock (0x80)
  %ra = call i64 @__omni_cyw43_reset_core(i64 403714048)
  store i64 0, i64* %i_slot
  br label %poll_ht

poll_ht:
  %h_idx = load i64, i64* %i_slot
  %h_ok = icmp slt i64 %h_idx, 500
  br i1 %h_ok, label %poll_ht_body, label %err_ht

poll_ht_body:
  %hcsr = call i64 @__omni_cyw43_read_reg(i64 1, i64 65550, i64 1)
  %hmask = and i64 %hcsr, 128
  %h_rdy = icmp ne i64 %hmask, 0
  br i1 %h_rdy, label %ht_up, label %poll_ht_next

poll_ht_next:
  %hsl = call i64 @sleep_ms(i64 1)
  %h_nxt = add i64 %h_idx, 1
  store i64 %h_nxt, i64* %i_slot
  br label %poll_ht

err_ht:
  ret i64 -4

ht_up:
  ; Set SDIO_INT_HOST_MASK (0x18002024 = 402661412) = 0xF0 and SDIO_FUNCTION2_WATERMARK (0x10008 = 65544) = 0x20
  %im = call i64 @__omni_cyw43_write_bp(i64 402661412, i64 240, i64 4)
  %wm = call i64 @__omni_cyw43_write_reg(i64 1, i64 65544, i64 32, i64 1)

  ; 8. Poll SPI_STATUS_REGISTER (0x0008) for STATUS_F2_RX_READY (0x20 = 32)
  store i64 0, i64* %i_slot
  br label %poll_f2

poll_f2:
  %f_idx = load i64, i64* %i_slot
  %f_ok = icmp slt i64 %f_idx, 500
  br i1 %f_ok, label %poll_f2_body, label %err_f2

poll_f2_body:
  %fstat = call i64 @__omni_cyw43_read_reg(i64 0, i64 8, i64 4)
  %fmask = and i64 %fstat, 32
  %f_rdy = icmp ne i64 %fmask, 0
  br i1 %f_rdy, label %f2_up, label %poll_f2_next

poll_f2_next:
  %fsl = call i64 @sleep_ms(i64 1)
  %f_nxt = add i64 %f_idx, 1
  store i64 %f_nxt, i64* %i_slot
  br label %poll_f2

err_f2:
  ret i64 -5

f2_up:
  ; Keep HT backplane clock awake (SBSDIO_FORCE_HT = 0x02), KEEP_SDIO_ON (0x1001F = 65567), clear pulls (0x1000F = 65551)
  %fht = call i64 @__omni_cyw43_write_reg(i64 1, i64 65550, i64 2, i64 1)
  %slp0 = call i64 @__omni_cyw43_read_reg(i64 1, i64 65567, i64 1)
  %slp1 = or i64 %slp0, 1
  %slpw = call i64 @__omni_cyw43_write_reg(i64 1, i64 65567, i64 %slp1, i64 1)
  %pup0 = call i64 @__omni_cyw43_write_reg(i64 1, i64 65551, i64 0, i64 1)
  %pup1 = call i64 @__omni_cyw43_read_reg(i64 1, i64 65551, i64 1)
  %irq0 = call i64 @__omni_cyw43_read_reg(i64 0, i64 4, i64 2)
  %irq1 = call i64 @__omni_cyw43_write_reg(i64 0, i64 4, i64 %irq0, i64 2)

  ; Enable ChipCommon WL_GPIO0 output: GPIOCONTROL (0x1800006C = 402653292) = 0, GPIOOUTEN (0x18000068 = 402653288) = 1
  %cc0 = call i64 @__omni_cyw43_write_bp(i64 402653292, i64 0, i64 4)
  %cc1 = call i64 @__omni_cyw43_write_bp(i64 402653288, i64 1, i64 4)
  ret i64 0
}

define i64 @cyw43_led_init() {
entry:
  %r = call i64 @cyw43_init()
  ret i64 %r
}

define i64 @__omni_cyw43_drain_f2() {
entry:
  %p_slot = alloca i64
  %w_slot = alloca i64
  store i64 0, i64* %p_slot
  br label %poll_loop

poll_loop:
  %pi = load i64, i64* %p_slot
  %pmore = icmp slt i64 %pi, 12
  br i1 %pmore, label %poll_body, label %done

poll_body:
  %st = call i64 @__omni_cyw43_read_reg(i64 0, i64 8, i64 4)
  %avail = and i64 %st, 256
  %has_pkt = icmp ne i64 %avail, 0
  br i1 %has_pkt, label %read_pkt, label %wait_more

wait_more:
  %wu = call i64 @sleep_us(i64 250)
  %pnxt = add i64 %pi, 1
  store i64 %pnxt, i64* %p_slot
  br label %poll_loop

read_pkt:
  %lsh = lshr i64 %st, 9
  %plen = and i64 %lsh, 2047
  %p3 = add i64 %plen, 3
  %aligned = and i64 %p3, 2044
  %gt0 = icmp sgt i64 %aligned, 0
  br i1 %gt0, label %do_drain, label %done

do_drain:
  %cmd = call i64 @__omni_cyw43_pack_cmd(i64 0, i64 1, i64 2, i64 0, i64 %aligned)
  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %wc = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %nwords = lshr i64 %aligned, 2
  store i64 0, i64* %w_slot
  br label %w_loop

w_loop:
  %wi = load i64, i64* %w_slot
  %wmore = icmp slt i64 %wi, %nwords
  br i1 %wmore, label %w_body, label %w_done

w_body:
  %d = call i64 @__omni_cyw43_gspi_read_u32()
  %wnxt = add i64 %wi, 1
  store i64 %wnxt, i64* %w_slot
  br label %w_loop

w_done:
  %cs1 = call i64 @gpio_put(i64 25, i64 1)
  %pn2 = add i64 %pi, 1
  store i64 %pn2, i64* %p_slot
  br label %poll_loop

done:
  ret i64 0
}

define i64 @cyw43_gpio_put(i64 %pin, i64 %on) {
entry:
  %p = and i64 %pin, 7
  %mask = shl i64 1, %p
  %is_on = icmp ne i64 %on, 0
  br i1 %is_on, label %val_hi, label %val_lo

val_hi:
  br label %send_io

val_lo:
  br label %send_io

send_io:
  %vbit = phi i64 [ %mask, %val_hi ], [ 0, %val_lo ]
  %scr = call i64 @__omni_cyw43_sram_scratch()
  %scr16 = add i64 %scr, 16
  %nctx = call i64 @mmio_read32(i64 %scr16)
  %has_net = icmp ne i64 %nctx, 0
  br i1 %has_net, label %via_net, label %via_raw

via_net:
  %rn = call i64 @__omni_fn_cyw_net_gpio_put(i64 %pin, i64 %on, i64 0, i64 0, i64 0, i64 0)
  ret i64 %rn

via_raw:
  %scr4 = add i64 %scr, 4
  %scr8 = add i64 %scr, 8
  %scr12 = add i64 %scr, 12

  %seq0 = call i64 @mmio_read32(i64 %scr4)
  %seq = and i64 %seq0, 255
  %seq_nxt0 = add i64 %seq, 1
  %seq_nxt = and i64 %seq_nxt0, 255
  %ws = call i64 @mmio_write32(i64 %scr4, i64 %seq_nxt)

  %req0 = call i64 @mmio_read32(i64 %scr8)
  %req = and i64 %req0, 65535
  %req_nxt0 = add i64 %req, 1
  %req_nxt = and i64 %req_nxt0, 65535
  %wrq = call i64 @mmio_write32(i64 %scr8, i64 %req_nxt)

  ; Update packed GPIO shadow state at scr+12: bits[7:0]=out, bits[15:8]=outen
  %pk_old = call i64 @mmio_read32(i64 %scr12)
  %out_old = and i64 %pk_old, 255
  %en_old0 = lshr i64 %pk_old, 8
  %en_old = and i64 %en_old0, 255
  %not_mask = xor i64 %mask, 255
  %out_clr = and i64 %out_old, %not_mask
  %out_new = or i64 %out_clr, %vbit
  %en_new = or i64 %en_old, %mask
  %en_sh = shl i64 %en_new, 8
  %pk_new = or i64 %out_new, %en_sh
  %wpk = call i64 @mmio_write32(i64 %scr12, i64 %pk_new)

  ; 1) Send 44-byte SDPCM + BCDC WLC_SET_VAR (263) "gpioout\0" [mask, vbit] on Function 2
  %cmd = call i64 @__omni_cyw43_pack_cmd(i64 1, i64 1, i64 2, i64 0, i64 44)
  %sw1 = or i64 201326592, %seq
  %rsh = shl i64 %req, 16
  %flags = or i64 %rsh, 2

  %cs0 = call i64 @gpio_put(i64 25, i64 0)
  %p0 = call i64 @__omni_cyw43_gspi_write_u32(i64 %cmd)
  %p1 = call i64 @__omni_cyw43_gspi_write_u32(i64 4292018220)
  %p2 = call i64 @__omni_cyw43_gspi_write_u32(i64 %sw1)
  %p3 = call i64 @__omni_cyw43_gspi_write_u32(i64 0)
  %p4 = call i64 @__omni_cyw43_gspi_write_u32(i64 263)
  %p5 = call i64 @__omni_cyw43_gspi_write_u32(i64 16)
  %p6 = call i64 @__omni_cyw43_gspi_write_u32(i64 %flags)
  %p7 = call i64 @__omni_cyw43_gspi_write_u32(i64 0)
  %p8 = call i64 @__omni_cyw43_gspi_write_u32(i64 1869181031)
  %p9 = call i64 @__omni_cyw43_gspi_write_u32(i64 7632239)
  %p10 = call i64 @__omni_cyw43_gspi_write_u32(i64 %mask)
  %p11 = call i64 @__omni_cyw43_gspi_write_u32(i64 %vbit)
  %cs1 = call i64 @gpio_put(i64 25, i64 1)

  %dr = call i64 @__omni_cyw43_drain_f2()

  ; 2) Also mirror to ChipCommon GPIOOUTEN (0x18000068 = 402653288) & GPIOOUT (0x18000064 = 402653284)
  %b0 = call i64 @__omni_cyw43_write_bp(i64 402653288, i64 %en_new, i64 4)
  %b1 = call i64 @__omni_cyw43_write_bp(i64 402653284, i64 %out_new, i64 4)
  ret i64 0
}

define i64 @cyw43_led_put(i64 %on) {
entry:
  %r = call i64 @cyw43_gpio_put(i64 0, i64 %on)
  ret i64 %r
}
"#;
