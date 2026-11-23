use super::chip::{RpChipKind, RpChipProfile};
use super::thumb_enc::{Cond, ThumbAssembler};

/// USB 2.0 Device Descriptor (`18` bytes): Raspberry Pi VID `0x2E8A`, CDC Serial PID `0x000A`.
pub const USB_DEV_DESC: [u8; 18] = [
    18,   // bLength
    0x01, // bDescriptorType = DEVICE
    0x00, 0x02, // bcdUSB = 2.00
    0x02, // bDeviceClass = CDC Communications
    0x00, // bDeviceSubClass
    0x00, // bDeviceProtocol
    64,   // bMaxPacketSize0 = 64
    0x8A, 0x2E, // idVendor = 0x2E8A (Raspberry Pi — matches rootless udev rule!)
    0x0A, 0x00, // idProduct = 0x000A (Pico CDC Serial)
    0x00, 0x01, // bcdDevice = 1.00
    1,    // iManufacturer
    2,    // iProduct
    3,    // iSerialNumber
    1,    // bNumConfigurations
];

/// USB 2.0 CDC-ACM Configuration Descriptor (`67` bytes: Control IF 0 + Data IF 1).
pub const USB_CFG_DESC: [u8; 67] = [
    // Configuration Descriptor (9 B)
    9, 0x02, 67, 0, 2, 1, 0, 0x80, 250,
    // Interface 0: CDC Communication Control (9 B)
    9, 0x04, 0, 0, 1, 0x02, 0x02, 0x01, 0,
    // CDC Header Functional Descriptor (5 B)
    5, 0x24, 0x00, 0x10, 0x01,
    // CDC Call Management Functional Descriptor (5 B)
    5, 0x24, 0x01, 0x00, 0x01,
    // CDC ACM Functional Descriptor (4 B): supports Line_Coding + Control_Line_State
    4, 0x24, 0x02, 0x02,
    // CDC Union Functional Descriptor (5 B): Control IF 0 -> Data IF 1
    5, 0x24, 0x06, 0x00, 0x01,
    // Endpoint 1 IN: Interrupt Notification (7 B)
    7, 0x05, 0x81, 0x03, 16, 0, 64,
    // Interface 1: CDC Data (9 B)
    9, 0x04, 1, 0, 2, 0x0A, 0x00, 0x00, 0,
    // Endpoint 2 OUT: Bulk RX (7 B)
    7, 0x05, 0x02, 0x02, 64, 0, 0,
    // Endpoint 2 IN: Bulk TX (7 B)
    7, 0x05, 0x82, 0x02, 64, 0, 0,
];

/// String 0: LANGID `0x0409` (`4` bytes)
pub const USB_STR0: [u8; 4] = [4, 0x03, 0x09, 0x04];

/// String 1: `"OmniScript"` (`22` bytes UTF-16LE)
pub const USB_STR1: [u8; 22] = [
    22, 0x03, b'O', 0, b'm', 0, b'n', 0, b'i', 0, b'S', 0, b'c', 0, b'r', 0, b'i', 0, b'p', 0,
    b't', 0,
];

/// String 2: `"Omni Pico CDC"` (`28` bytes UTF-16LE)
pub const USB_STR2: [u8; 28] = [
    28, 0x03, b'O', 0, b'm', 0, b'n', 0, b'i', 0, b' ', 0, b'P', 0, b'i', 0, b'c', 0, b'o', 0,
    b' ', 0, b'C', 0, b'D', 0, b'C', 0,
];

/// String 3: `"OMNI2350"` (`18` bytes UTF-16LE)
pub const USB_STR3: [u8; 18] = [
    18, 0x03, b'O', 0, b'M', 0, b'N', 0, b'I', 0, b'2', 0, b'3', 0, b'5', 0, b'0', 0,
];

/// Emit low-level Thumb ISR wrapper, DPRAM pipeline-safe buffer arming, descriptor table lookup,
/// and Bootrom `1200`-baud hands-free `BOOTSEL` reboot helper.
pub fn emit_usb_cdc_lowlevel(asm: &mut ThumbAssembler, chip: &RpChipProfile) -> Result<(), String> {
    let base_vma = chip.flash_base;

    // `USBCTRL_ISR`: Vector Table hardware IRQ entry (`IRQ 5` on RP2040, `IRQ 14` on RP2350)
    asm.begin_function("USBCTRL_ISR");
    asm.emit_push(1 << 4, true); // Save r4 + EXC_RETURN (`lr`), keeping SP 8-byte aligned
    asm.emit_bl_sym("__omni_usb_irq_handler");
    asm.emit_pop(1 << 4, true);  // Return from exception via `EXC_RETURN` -> `pc`
    asm.end_function(base_vma)?;

    // `__omni_usb_irq_mask() -> i64`
    asm.begin_function("__omni_usb_irq_mask");
    asm.emit_load_u32(0, 1u32 << chip.usbctrl_irq_num);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_usb_crit_enter() -> i64`: single-cycle `cpsid i` + pipeline drain
    asm.begin_function("__omni_usb_crit_enter");
    asm.emit16(0xB672); // cpsid i
    asm.emit_nop();
    asm.emit_nop();
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_usb_crit_exit() -> i64`: `cpsie i`
    asm.begin_function("__omni_usb_crit_exit");
    asm.emit16(0xB662); // cpsie i
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_usb_dpram_arm(buf_ctrl_addr = r0:r1, val_no_avail = r2:r3) -> i64`
    // Enforces >= 12 CPU cycles (3x 48 MHz USB clocks @ 150 MHz) between writing buffer metadata
    // and asserting `AVAILABLE_0 (0x0400)` per RP2040/RP2350 USB DPRAM concurrent access spec.
    asm.begin_function("__omni_usb_dpram_arm");
    asm.emit_str_imm(2, 0, 0);
    asm.emit_movs_imm8(3, 6);
    let arm_dly = asm.new_label();
    asm.bind_label(arm_dly);
    asm.emit_subs_imm8(3, 1);
    asm.emit_bcond_label_short(Cond::Ne, arm_dly);
    asm.emit_load_u32(3, 0x0400);
    asm.emit_orrs(2, 3);
    asm.emit_str_imm(2, 0, 0);
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_usb_zero_dpram() -> i64`: tight 4-cycle/word Thumb zeroing of all 4 KB `USBCTRL_DPRAM`
    asm.begin_function("__omni_usb_zero_dpram");
    asm.emit_load_u32(0, chip.usb_dpram_base);
    asm.emit_movs_imm8(1, 0);
    asm.emit_load_u32(2, 1024);
    let zlp = asm.new_label();
    asm.bind_label(zlp);
    asm.emit_str_imm(1, 0, 0);
    asm.emit_adds_imm8(0, 4);
    asm.emit_subs_imm8(2, 1);
    asm.emit_bcond_label_short(Cond::Ne, zlp);
    asm.emit_movs_imm8(0, 0);
    asm.emit_movs_imm8(1, 0);
    asm.emit_bx(14);
    asm.end_function(base_vma)?;

    // `__omni_usb_desc_ptr(idx = r0:r1) -> i64`
    // 0=dev(18), 1=cfg(67), 2=str0(4), 3=str1(22), 4=str2(28), 5=str3(18)
    asm.begin_function("__omni_usb_desc_ptr");
    {
        for (i, sym) in [
            (0u8, "__usb_dev_desc"),
            (1u8, "__usb_cfg_desc"),
            (2u8, "__usb_str0"),
            (3u8, "__usb_str1"),
            (4u8, "__usb_str2"),
            (5u8, "__usb_str3"),
        ] {
            let nxt = asm.new_label();
            asm.emit_cmp_imm8(0, i);
            asm.emit_bcond_label_short(Cond::Ne, nxt);
            asm.emit_load_sym_addr(0, sym, false, 0);
            asm.emit_movs_imm8(1, 0);
            asm.emit_bx(14);
            asm.bind_label(nxt);
        }
        asm.emit_movs_imm8(0, 0);
        asm.emit_movs_imm8(1, 0);
        asm.emit_bx(14);
    }
    asm.end_function(base_vma)?;

    // `__omni_usb_rom_bootsel() -> i64`
    // Hands-free `1200`-baud USB reboot directly into RP2040 / RP2350 `BOOTSEL` mode!
    asm.begin_function("__omni_usb_rom_bootsel");
    {
        // 1) Disable USBCTRL IRQ in NVIC_ICER0 and drop D+ pullup cleanly
        asm.emit_load_u32(0, 0xE000_E180);
        asm.emit_load_u32(1, 1u32 << chip.usbctrl_irq_num);
        asm.emit_str_imm(1, 0, 0);
        asm.emit_load_u32(0, chip.usb_regs_base);
        asm.emit_movs_imm8(1, 0);
        asm.emit_str_imm(1, 0, 0x4C); // SIE_CTRL = 0 (disconnect pullup)

        // Brief ~2 ms settle loop so host sees USB disconnect before ROM re-enumerates
        asm.emit_load_u32(2, 60_000);
        let dly = asm.new_label();
        asm.bind_label(dly);
        asm.emit_subs_imm8(2, 1);
        asm.emit_bcond_label_short(Cond::Ne, dly);

        match chip.kind {
            RpChipKind::Rp2040 => {
                // `func_table = *(u16*)0x14`, `lookup = *(u16*)0x18`, `fn = lookup(func_table, 'UB'=0x4255)`, `fn(0, 0)`
                asm.emit_movs_imm8(3, 0x14);
                asm.emit16(0x8818); // ldrh r0, [r3, #0] (0x14)
                asm.emit16(0x889A); // ldrh r2, [r3, #4] (0x18)
                asm.emit_load_u32(1, 0x4255); // 'U' | ('B' << 8)
                asm.emit_blx_reg(2);
                asm.emit_movs_reg(2, 0);
                asm.emit_movs_imm8(0, 0);
                asm.emit_movs_imm8(1, 0);
                asm.emit_blx_reg(2);
            }
            RpChipKind::Rp2350 => {
                // `lookup = *(u16*)0x16`, `fn = lookup('RB'=0x4252, RT_FLAG_FUNC_ARM_SEC=0x04)`, `fn(0x0102, 10, 0, 0)`
                asm.emit_movs_imm8(3, 0x16);
                asm.emit16(0x881A); // ldrh r2, [r3, #0] (0x16)
                asm.emit_load_u32(0, 0x4252); // 'R' | ('B' << 8)
                asm.emit_movs_imm8(1, 0x04);  // RT_FLAG_FUNC_ARM_SEC
                asm.emit_blx_reg(2);
                asm.emit_movs_reg(4, 0);
                asm.emit_load_u32(0, 0x0102); // REBOOT2_FLAG_NO_RETURN_ON_SUCCESS | REBOOT_TYPE_BOOTSEL
                asm.emit_movs_imm8(1, 10);    // 10 ms delay
                asm.emit_movs_imm8(2, 0);
                asm.emit_movs_imm8(3, 0);
                asm.emit_blx_reg(4);
            }
        }

        let spin = asm.new_label();
        asm.bind_label(spin);
        asm.emit_wfi();
        asm.emit_b_label_short(spin);
    }
    asm.end_function(base_vma)?;

    Ok(())
}

/// Append the 6 read-only USB CDC-ACM descriptors into Flash `.rodata`.
pub fn emit_usb_cdc_rodata(asm: &mut ThumbAssembler) {
    for (sym, bytes) in [
        ("__usb_dev_desc", &USB_DEV_DESC[..]),
        ("__usb_cfg_desc", &USB_CFG_DESC[..]),
        ("__usb_str0", &USB_STR0[..]),
        ("__usb_str1", &USB_STR1[..]),
        ("__usb_str2", &USB_STR2[..]),
        ("__usb_str3", &USB_STR3[..]),
    ] {
        asm.align4_zero();
        asm.symbols.insert(sym.to_string(), asm.pos());
        asm.code.extend_from_slice(bytes);
    }
}

/// Complete freestanding USB 2.0 Full-Speed CDC-ACM (`/dev/ttyACM0`) state machine in pure ISR-safe LLVM IR.
/// Uses `USBCTRL_DPRAM` (`0x5010_0000..0x5010_1000`) exclusively for buffers, state, and a 2 KB boot-preserving TX ring!
pub const USB_CDC_DRIVER_IR: &str = r#"
declare i64 @mmio_read32(i64)
declare i64 @mmio_write32(i64, i64)
declare i64 @mem_read8(i64)
declare i64 @mem_write8(i64, i64)
declare i64 @__omni_usb_irq_mask()
declare i64 @__omni_usb_crit_enter()
declare i64 @__omni_usb_crit_exit()
declare i64 @__omni_usb_dpram_arm(i64, i64)
declare i64 @__omni_usb_zero_dpram()
declare i64 @__omni_usb_desc_ptr(i64)
declare i64 @__omni_usb_rom_bootsel()

define i64 @__omni_usb_copy_bytes(i64 %dst, i64 %src, i64 %len) {
entry:
  %i_slot = alloca i64
  store i64 0, i64* %i_slot
  br label %lp

lp:
  %i = load i64, i64* %i_slot
  %more = icmp slt i64 %i, %len
  br i1 %more, label %body, label %done

body:
  %sp = add i64 %src, %i
  %dp = add i64 %dst, %i
  %b = call i64 @mem_read8(i64 %sp)
  %w = call i64 @mem_write8(i64 %dp, i64 %b)
  %nxt = add i64 %i, 1
  store i64 %nxt, i64* %i_slot
  br label %lp

done:
  ret i64 %len
}

define i64 @__omni_usb_try_pump_tx() {
entry:
  ; Only pump EP2_IN when USB is configured AND host terminal has asserted DTR (or opened ttyACM0)
  %cfg = call i64 @mmio_read32(i64 1343226496)
  %c_ok = icmp ne i64 %cfg, 0
  br i1 %c_ok, label %chk_dtr, label %done

chk_dtr:
  %dtr = call i64 @mmio_read32(i64 1343226504)
  %d_ok = icmp ne i64 %dtr, 0
  br i1 %d_ok, label %chk_busy, label %done

chk_busy:
  ; Check EP2_IN_BUF_CTRL (0x50100090 = 1343226000) AVAILABLE_0 (0x0400 = 1024)
  %bc = call i64 @mmio_read32(i64 1343226000)
  %av = and i64 %bc, 1024
  %idle = icmp eq i64 %av, 0
  br i1 %idle, label %chk_ring, label %done

chk_ring:
  %head = call i64 @mmio_read32(i64 1343226544)
  %tail0 = call i64 @mmio_read32(i64 1343226548)
  %non_empty = icmp ne i64 %head, %tail0
  br i1 %non_empty, label %fill_pkt, label %done

fill_pkt:
  %cnt_slot = alloca i64
  %tl_slot = alloca i64
  store i64 0, i64* %cnt_slot
  store i64 %tail0, i64* %tl_slot
  br label %plp

plp:
  ; Cap at 63 bytes (< 64 wMaxPacketSize) so every packet is a USB short packet -> instant Linux ttyACM flush!
  %cnt = load i64, i64* %cnt_slot
  %tl = load i64, i64* %tl_slot
  %lt63 = icmp slt i64 %cnt, 63
  br i1 %lt63, label %chk_more, label %send_it

chk_more:
  %has_b = icmp ne i64 %tl, %head
  br i1 %has_b, label %copy_one, label %send_it

copy_one:
  %rptr = add i64 1343226880, %tl
  %b = call i64 @mem_read8(i64 %rptr)
  %ep_ptr = add i64 1343226368, %cnt
  %wb = call i64 @mem_write8(i64 %ep_ptr, i64 %b)
  %tl1 = add i64 %tl, 1
  %tl_m = and i64 %tl1, 2047
  %cnt1 = add i64 %cnt, 1
  store i64 %tl_m, i64* %tl_slot
  store i64 %cnt1, i64* %cnt_slot
  br label %plp

send_it:
  %final_cnt = load i64, i64* %cnt_slot
  %final_tl = load i64, i64* %tl_slot
  %wt = call i64 @mmio_write32(i64 1343226548, i64 %final_tl)
  %pid = call i64 @mmio_read32(i64 1343226524)
  %npid = xor i64 %pid, 8192
  %wp = call i64 @mmio_write32(i64 1343226524, i64 %npid)
  %ctrl0 = or i64 32768, %pid
  %ctrl1 = or i64 %ctrl0, %final_cnt
  %arm = call i64 @__omni_usb_dpram_arm(i64 1343226000, i64 %ctrl1)
  ret i64 %final_cnt

done:
  ret i64 0
}

define i64 @__omni_usb_ring_push_byte(i64 %b) {
entry:
  %head = call i64 @mmio_read32(i64 1343226544)
  %tail = call i64 @mmio_read32(i64 1343226548)
  %wptr = add i64 1343226880, %head
  %wb = call i64 @mem_write8(i64 %wptr, i64 %b)
  %nxt0 = add i64 %head, 1
  %nxt = and i64 %nxt0, 2047
  %full = icmp eq i64 %nxt, %tail
  br i1 %full, label %drop_old, label %save_hd

drop_old:
  %t1 = add i64 %tail, 1
  %tm = and i64 %t1, 2047
  %wt = call i64 @mmio_write32(i64 1343226548, i64 %tm)
  br label %save_hd

save_hd:
  %wh = call i64 @mmio_write32(i64 1343226544, i64 %nxt)
  ret i64 0
}

define i64 @__omni_usb_putc(i64 %ch) {
entry:
  %inited = call i64 @mmio_read32(i64 1343226560)
  %ok = icmp ne i64 %inited, 0
  br i1 %ok, label %do_push, label %done

do_push:
  %di = call i64 @__omni_usb_crit_enter()
  %c8 = and i64 %ch, 255
  %is_nl = icmp eq i64 %c8, 10
  br i1 %is_nl, label %add_cr, label %push_c

add_cr:
  %pcr = call i64 @__omni_usb_ring_push_byte(i64 13)
  br label %push_c

push_c:
  %pc = call i64 @__omni_usb_ring_push_byte(i64 %c8)
  %pmp = call i64 @__omni_usb_try_pump_tx()
  %ei = call i64 @__omni_usb_crit_exit()
  br label %done

done:
  ret i64 0
}

define i64 @__omni_usb_ep0_start_in(i64 %src_ptr, i64 %desc_len, i64 %wlen) {
entry:
  %lt = icmp slt i64 %wlen, %desc_len
  br i1 %lt, label %use_wlen, label %use_dlen

use_wlen:
  br label %calc_chk

use_dlen:
  br label %calc_chk

calc_chk:
  %tot = phi i64 [ %wlen, %use_wlen ], [ %desc_len, %use_dlen ]
  %gt64 = icmp sgt i64 %tot, 64
  br i1 %gt64, label %c64, label %ctot

c64:
  br label %do_arm

ctot:
  br label %do_arm

do_arm:
  %chk = phi i64 [ 64, %c64 ], [ %tot, %ctot ]
  %cp = call i64 @__omni_usb_copy_bytes(i64 1343226112, i64 %src_ptr, i64 %chk)
  %nptr = add i64 %src_ptr, %chk
  %nrem = sub i64 %tot, %chk
  %s0 = call i64 @mmio_write32(i64 1343226508, i64 %nptr)
  %s1 = call i64 @mmio_write32(i64 1343226512, i64 %nrem)
  ; Second packet (if nrem > 0) uses DATA0 (0), since first packet uses DATA1 (8192)
  %s2 = call i64 @mmio_write32(i64 1343226516, i64 0)
  %s3 = call i64 @mmio_write32(i64 1343226520, i64 0)
  ; Arm EP0_OUT (0x50100084) for Status ZLP OUT (DATA1 | 64 = 8256)
  %ao = call i64 @__omni_usb_dpram_arm(i64 1343225988, i64 8256)
  ; Arm EP0_IN (0x50100080) with FULL(32768) | DATA1(8192) | chk = 40960 | chk
  %in_ctrl = or i64 40960, %chk
  %ai = call i64 @__omni_usb_dpram_arm(i64 1343225984, i64 %in_ctrl)
  ret i64 0
}

define i64 @__omni_usb_ep0_send_zlp_status() {
entry:
  %s0 = call i64 @mmio_write32(i64 1343226512, i64 0)
  ; FULL(32768) | DATA1(8192) | len(0) = 40960
  %ai = call i64 @__omni_usb_dpram_arm(i64 1343225984, i64 40960)
  ret i64 0
}

define i64 @__omni_usb_ep0_stall() {
entry:
  %sa = call i64 @mmio_write32(i64 1343291496, i64 3)
  %si = call i64 @mmio_write32(i64 1343225984, i64 2048)
  %so = call i64 @mmio_write32(i64 1343225988, i64 2048)
  ret i64 0
}

define i64 @__omni_usb_handle_bus_reset() {
entry:
  ; Clear BUS_RESET (0x80000 = 524288) via usb_hw_clear->sie_status (0x50113050 = 1343303760)
  %c0 = call i64 @mmio_write32(i64 1343303760, i64 524288)
  ; Reset device address = 0 (0x50110000 = 1343291392)
  %a0 = call i64 @mmio_write32(i64 1343291392, i64 0)
  ; Reset control/endpoint state WITHOUT touching tx_head/tx_tail (preserves early boot print output!)
  %z0 = call i64 @mmio_write32(i64 1343226496, i64 0)
  %z1 = call i64 @mmio_write32(i64 1343226500, i64 0)
  %z2 = call i64 @mmio_write32(i64 1343226504, i64 0)
  %z3 = call i64 @mmio_write32(i64 1343226512, i64 0)
  %z4 = call i64 @mmio_write32(i64 1343226520, i64 0)
  %z5 = call i64 @mmio_write32(i64 1343226524, i64 0)
  %z6 = call i64 @mmio_write32(i64 1343226528, i64 0)
  ; Clear buffer controls EP0..EP2
  %b0 = call i64 @mmio_write32(i64 1343225984, i64 0)
  %b1 = call i64 @mmio_write32(i64 1343225988, i64 0)
  %b2 = call i64 @mmio_write32(i64 1343225992, i64 0)
  %b3 = call i64 @mmio_write32(i64 1343226000, i64 0)
  %b4 = call i64 @mmio_write32(i64 1343226004, i64 0)
  ; Configure EP1_IN (0x50100008), EP2_IN (0x50100010), EP2_OUT (0x50100014)
  %e1 = call i64 @mmio_write32(i64 1343225864, i64 2885681536)
  %e2 = call i64 @mmio_write32(i64 1343225872, i64 2818572800)
  %e3 = call i64 @mmio_write32(i64 1343225876, i64 2818572864)
  ret i64 0
}

define i64 @__omni_usb_handle_setup() {
entry:
  ; Disarm any stale EP0 buffer controls first
  %d0 = call i64 @mmio_write32(i64 1343225984, i64 0)
  %d1 = call i64 @mmio_write32(i64 1343225988, i64 0)
  %cb = call i64 @mmio_write32(i64 1343303768, i64 3)

  %bm = call i64 @mem_read8(i64 1343225856)
  %breq = call i64 @mem_read8(i64 1343225857)
  %wv_lo = call i64 @mem_read8(i64 1343225858)
  %wv_hi = call i64 @mem_read8(i64 1343225859)
  %wv_hsh = shl i64 %wv_hi, 8
  %wval = or i64 %wv_lo, %wv_hsh

  %wi_lo = call i64 @mem_read8(i64 1343225860)
  %wi_hi = call i64 @mem_read8(i64 1343225861)
  %wi_hsh = shl i64 %wi_hi, 8
  %widx = or i64 %wi_lo, %wi_hsh

  %wl_lo = call i64 @mem_read8(i64 1343225862)
  %wl_hi = call i64 @mem_read8(i64 1343225863)
  %wl_hsh = shl i64 %wl_hi, 8
  %wlen = or i64 %wl_lo, %wl_hsh

  ; Clear SETUP_REC (0x20000 = 131072) AFTER latching all 8 setup packet bytes from DPRAM!
  %cs = call i64 @mmio_write32(i64 1343303760, i64 131072)

  %dir_in = and i64 %bm, 128
  %is_in = icmp ne i64 %dir_in, 0
  br i1 %is_in, label %req_in, label %req_out

req_in:
  ; Standard GET_DESCRIPTOR (bm == 0x80, breq == 6)
  %is_gdesc0 = icmp eq i64 %bm, 128
  %is_gdesc1 = icmp eq i64 %breq, 6
  %is_gdesc = and i1 %is_gdesc0, %is_gdesc1
  br i1 %is_gdesc, label %handle_gdesc, label %chk_glc

handle_gdesc:
  %is_dev = icmp eq i64 %wv_hi, 1
  br i1 %is_dev, label %send_dev, label %chk_cfg_d

send_dev:
  %p_dev = call i64 @__omni_usb_desc_ptr(i64 0)
  %r_dev = call i64 @__omni_usb_ep0_start_in(i64 %p_dev, i64 18, i64 %wlen)
  ret i64 0

chk_cfg_d:
  %is_cfg = icmp eq i64 %wv_hi, 2
  br i1 %is_cfg, label %send_cfg, label %chk_str_d

send_cfg:
  %p_cfg = call i64 @__omni_usb_desc_ptr(i64 1)
  %r_cfg = call i64 @__omni_usb_ep0_start_in(i64 %p_cfg, i64 67, i64 %wlen)
  ret i64 0

chk_str_d:
  %is_str = icmp eq i64 %wv_hi, 3
  br i1 %is_str, label %match_str, label %stall_ep0

match_str:
  %s_is0 = icmp eq i64 %wv_lo, 0
  br i1 %s_is0, label %send_s0, label %chk_s1

send_s0:
  %ps0 = call i64 @__omni_usb_desc_ptr(i64 2)
  %rs0 = call i64 @__omni_usb_ep0_start_in(i64 %ps0, i64 4, i64 %wlen)
  ret i64 0

chk_s1:
  %s_is1 = icmp eq i64 %wv_lo, 1
  br i1 %s_is1, label %send_s1, label %chk_s2

send_s1:
  %ps1 = call i64 @__omni_usb_desc_ptr(i64 3)
  %rs1 = call i64 @__omni_usb_ep0_start_in(i64 %ps1, i64 22, i64 %wlen)
  ret i64 0

chk_s2:
  %s_is2 = icmp eq i64 %wv_lo, 2
  br i1 %s_is2, label %send_s2, label %chk_s3

send_s2:
  %ps2 = call i64 @__omni_usb_desc_ptr(i64 4)
  %rs2 = call i64 @__omni_usb_ep0_start_in(i64 %ps2, i64 28, i64 %wlen)
  ret i64 0

chk_s3:
  %s_is3 = icmp eq i64 %wv_lo, 3
  br i1 %s_is3, label %send_s3, label %stall_ep0

send_s3:
  %ps3 = call i64 @__omni_usb_desc_ptr(i64 5)
  %rs3 = call i64 @__omni_usb_ep0_start_in(i64 %ps3, i64 18, i64 %wlen)
  ret i64 0

chk_glc:
  ; CDC_GET_LINE_CODING (bm == 0xA1 = 161, breq == 0x21 = 33) -> 7 bytes from 0x501002A8 (1343226536)
  %is_glc0 = icmp eq i64 %bm, 161
  %is_glc1 = icmp eq i64 %breq, 33
  %is_glc = and i1 %is_glc0, %is_glc1
  br i1 %is_glc, label %send_lc, label %chk_gstat

send_lc:
  %rlc = call i64 @__omni_usb_ep0_start_in(i64 1343226536, i64 7, i64 %wlen)
  ret i64 0

chk_gstat:
  ; GET_STATUS (breq == 0) -> 2 zero bytes
  %is_gs = icmp eq i64 %breq, 0
  br i1 %is_gs, label %send_gs, label %chk_gcfg

send_gs:
  %zt = call i64 @mmio_write32(i64 1343226564, i64 0)
  %rgs = call i64 @__omni_usb_ep0_start_in(i64 1343226564, i64 2, i64 %wlen)
  ret i64 0

chk_gcfg:
  ; GET_CONFIGURATION (bm == 128, breq == 8) -> 1 byte
  %is_gc = icmp eq i64 %breq, 8
  br i1 %is_gc, label %send_gc, label %stall_ep0

send_gc:
  %cv = call i64 @mmio_read32(i64 1343226496)
  %wt = call i64 @mmio_write32(i64 1343226564, i64 %cv)
  %rgc = call i64 @__omni_usb_ep0_start_in(i64 1343226564, i64 1, i64 %wlen)
  ret i64 0

req_out:
  ; Check if OUT transfer has a DATA stage (wlen > 0), e.g. CDC_SET_LINE_CODING (bm=0x21=33, breq=0x20=32)
  %has_data = icmp sgt i64 %wlen, 0
  br i1 %has_data, label %out_with_data, label %out_no_data

out_with_data:
  %is_slc0 = icmp eq i64 %bm, 33
  %is_slc1 = icmp eq i64 %breq, 32
  %is_slc = and i1 %is_slc0, %is_slc1
  br i1 %is_slc, label %arm_slc, label %arm_gen_out

arm_slc:
  %m1 = call i64 @mmio_write32(i64 1343226520, i64 1)
  %ao1 = call i64 @__omni_usb_dpram_arm(i64 1343225988, i64 8256)
  ret i64 0

arm_gen_out:
  %m2 = call i64 @mmio_write32(i64 1343226520, i64 2)
  %ao2 = call i64 @__omni_usb_dpram_arm(i64 1343225988, i64 8256)
  ret i64 0

out_no_data:
  ; 1) SET_ADDRESS (bm == 0, breq == 5)
  %is_sa0 = icmp eq i64 %bm, 0
  %is_sa1 = icmp eq i64 %breq, 5
  %is_sa = and i1 %is_sa0, %is_sa1
  br i1 %is_sa, label %do_set_addr, label %chk_set_cfg

do_set_addr:
  %addr7 = and i64 %wval, 127
  %wa = call i64 @mmio_write32(i64 1343226500, i64 %addr7)
  %za = call i64 @__omni_usb_ep0_send_zlp_status()
  ret i64 0

chk_set_cfg:
  ; 2) SET_CONFIGURATION (bm == 0, breq == 9)
  %is_sc1 = icmp eq i64 %breq, 9
  %is_sc = and i1 %is_sa0, %is_sc1
  br i1 %is_sc, label %do_set_cfg, label %chk_scls

do_set_cfg:
  %cfg_v = and i64 %wval, 255
  %wc = call i64 @mmio_write32(i64 1343226496, i64 %cfg_v)
  ; Reset EP2 IN/OUT PIDs to DATA0 and arm EP2_OUT (0x50100094 = 1343226004) for 64B host serial RX!
  %rp0 = call i64 @mmio_write32(i64 1343226524, i64 0)
  %rp1 = call i64 @mmio_write32(i64 1343226528, i64 0)
  %ae2 = call i64 @__omni_usb_dpram_arm(i64 1343226004, i64 64)
  %zc = call i64 @__omni_usb_ep0_send_zlp_status()
  ret i64 0

chk_scls:
  ; 3) CDC_SET_CONTROL_LINE_STATE (bm == 0x21 = 33, breq == 0x22 = 34): wValue bit 0 = DTR
  %is_cls0 = icmp eq i64 %bm, 33
  %is_cls1 = icmp eq i64 %breq, 34
  %is_cls = and i1 %is_cls0, %is_cls1
  br i1 %is_cls, label %do_scls, label %chk_clr_feat

do_scls:
  %dtr_bit = and i64 %wval, 1
  %wd = call i64 @mmio_write32(i64 1343226504, i64 %dtr_bit)
  %zs = call i64 @__omni_usb_ep0_send_zlp_status()
  ; Immediately kick EP2_IN so buffered boot logs flush as soon as host opens /dev/ttyACM0!
  %kp = call i64 @__omni_usb_try_pump_tx()
  ret i64 0

chk_clr_feat:
  ; 4) CLEAR_FEATURE (bm == 2, breq == 1) or CDC_SEND_BREAK (bm == 33, breq == 35)
  %is_cf0 = icmp eq i64 %bm, 2
  %is_cf1 = icmp eq i64 %breq, 1
  %is_cf = and i1 %is_cf0, %is_cf1
  br i1 %is_cf, label %do_clr_feat, label %chk_brk

do_clr_feat:
  %is_ep2in = icmp eq i64 %widx, 130
  br i1 %is_ep2in, label %rst_ep2in, label %chk_ep2out

rst_ep2in:
  %ri = call i64 @mmio_write32(i64 1343226524, i64 0)
  br label %cf_ack

chk_ep2out:
  %is_ep2out = icmp eq i64 %widx, 2
  br i1 %is_ep2out, label %rst_ep2out, label %cf_ack

rst_ep2out:
  %ro = call i64 @mmio_write32(i64 1343226528, i64 0)
  br label %cf_ack

cf_ack:
  %zcf = call i64 @__omni_usb_ep0_send_zlp_status()
  ret i64 0

chk_brk:
  %is_brk1 = icmp eq i64 %breq, 35
  %is_brk = and i1 %is_cls0, %is_brk1
  br i1 %is_brk, label %cf_ack, label %stall_ep0

stall_ep0:
  %st = call i64 @__omni_usb_ep0_stall()
  ret i64 0
}

define i64 @__omni_usb_handle_buff_status() {
entry:
  %bs = call i64 @mmio_read32(i64 1343291480)
  %has_any = icmp ne i64 %bs, 0
  br i1 %has_any, label %chk_ep0_in, label %done

chk_ep0_in:
  %b_ep0in = and i64 %bs, 1
  %is_ep0in = icmp ne i64 %b_ep0in, 0
  br i1 %is_ep0in, label %do_ep0_in, label %chk_ep0_out

do_ep0_in:
  %c0 = call i64 @mmio_write32(i64 1343303768, i64 1)
  ; Apply deferred SET_ADDRESS once Status ZLP IN finishes!
  %paddr = call i64 @mmio_read32(i64 1343226500)
  %has_pa = icmp ne i64 %paddr, 0
  br i1 %has_pa, label %apply_addr, label %chk_bootsel

apply_addr:
  %wa = call i64 @mmio_write32(i64 1343291392, i64 %paddr)
  %ca = call i64 @mmio_write32(i64 1343226500, i64 0)
  br label %chk_bootsel

chk_bootsel:
  ; If 1200-baud SET_LINE_CODING Status ZLP IN just completed, jump into ROM BOOTSEL!
  %rarm = call i64 @mmio_read32(i64 1343226532)
  %do_rb = icmp ne i64 %rarm, 0
  br i1 %do_rb, label %go_bootsel, label %chk_ep0_more

go_bootsel:
  %crb = call i64 @mmio_write32(i64 1343226532, i64 0)
  %jb = call i64 @__omni_usb_rom_bootsel()
  br label %chk_ep0_more

chk_ep0_more:
  ; Continue multi-packet EP0_IN transfer (e.g. 67-byte Configuration Descriptor packet #2!)
  %rem = call i64 @mmio_read32(i64 1343226512)
  %has_rem = icmp sgt i64 %rem, 0
  br i1 %has_rem, label %send_ep0_more, label %chk_ep0_out

send_ep0_more:
  %sptr = call i64 @mmio_read32(i64 1343226508)
  %pid = call i64 @mmio_read32(i64 1343226516)
  %gt64 = icmp sgt i64 %rem, 64
  br i1 %gt64, label %m64, label %mrem

m64:
  br label %arm_more

mrem:
  br label %arm_more

arm_more:
  %mchk = phi i64 [ 64, %m64 ], [ %rem, %mrem ]
  %mcp = call i64 @__omni_usb_copy_bytes(i64 1343226112, i64 %sptr, i64 %mchk)
  %nsptr = add i64 %sptr, %mchk
  %nrem = sub i64 %rem, %mchk
  %npid = xor i64 %pid, 8192
  %u0 = call i64 @mmio_write32(i64 1343226508, i64 %nsptr)
  %u1 = call i64 @mmio_write32(i64 1343226512, i64 %nrem)
  %u2 = call i64 @mmio_write32(i64 1343226516, i64 %npid)
  %mctrl0 = or i64 32768, %pid
  %mctrl1 = or i64 %mctrl0, %mchk
  %marm = call i64 @__omni_usb_dpram_arm(i64 1343225984, i64 %mctrl1)
  br label %chk_ep0_out

chk_ep0_out:
  %b_ep0out = and i64 %bs, 2
  %is_ep0out = icmp ne i64 %b_ep0out, 0
  br i1 %is_ep0out, label %do_ep0_out, label %chk_ep2_in

do_ep0_out:
  %c1 = call i64 @mmio_write32(i64 1343303768, i64 2)
  %omode = call i64 @mmio_read32(i64 1343226520)
  %has_om = icmp ne i64 %omode, 0
  br i1 %has_om, label %finish_out_data, label %chk_ep2_in

finish_out_data:
  %cm = call i64 @mmio_write32(i64 1343226520, i64 0)
  %is_lc = icmp eq i64 %omode, 1
  br i1 %is_lc, label %save_lc, label %send_out_zlp

save_lc:
  %clc = call i64 @__omni_usb_copy_bytes(i64 1343226536, i64 1343226112, i64 7)
  %baud = call i64 @mmio_read32(i64 1343226536)
  %is_1200 = icmp eq i64 %baud, 1200
  br i1 %is_1200, label %arm_rb, label %send_out_zlp

arm_rb:
  %wrb = call i64 @mmio_write32(i64 1343226532, i64 1)
  br label %send_out_zlp

send_out_zlp:
  %zo = call i64 @__omni_usb_ep0_send_zlp_status()
  br label %chk_ep2_in

chk_ep2_in:
  ; bit 4 (16) = EP2_IN bulk TX complete -> pump next 63B chunk from 2KB tx_ring!
  %b_ep2in = and i64 %bs, 16
  %is_ep2in = icmp ne i64 %b_ep2in, 0
  br i1 %is_ep2in, label %do_ep2_in, label %chk_ep2_out

do_ep2_in:
  %c4 = call i64 @mmio_write32(i64 1343303768, i64 16)
  %pm = call i64 @__omni_usb_try_pump_tx()
  br label %chk_ep2_out

chk_ep2_out:
  ; bit 5 (32) = EP2_OUT bulk RX from host -> push into rx_ring[256] and re-arm!
  %b_ep2out = and i64 %bs, 32
  %is_ep2out = icmp ne i64 %b_ep2out, 0
  br i1 %is_ep2out, label %do_ep2_out, label %done

do_ep2_out:
  %c5 = call i64 @mmio_write32(i64 1343303768, i64 32)
  %obc = call i64 @mmio_read32(i64 1343226004)
  %rlen = and i64 %obc, 1023
  %ri_slot = alloca i64
  store i64 0, i64* %ri_slot
  br label %rx_lp

rx_lp:
  %ri = load i64, i64* %ri_slot
  %rmore = icmp slt i64 %ri, %rlen
  br i1 %rmore, label %rx_body, label %rx_rearm

rx_body:
  %src_b = add i64 1343226432, %ri
  %ch = call i64 @mem_read8(i64 %src_b)
  %rhd = call i64 @mmio_read32(i64 1343226552)
  %rtl = call i64 @mmio_read32(i64 1343226556)
  %rnxt0 = add i64 %rhd, 1
  %rnxt = and i64 %rnxt0, 255
  %rfull = icmp eq i64 %rnxt, %rtl
  br i1 %rfull, label %rx_next, label %rx_store

rx_store:
  %dst_b = add i64 1343226624, %rhd
  %wch = call i64 @mem_write8(i64 %dst_b, i64 %ch)
  %wrh = call i64 @mmio_write32(i64 1343226552, i64 %rnxt)
  br label %rx_next

rx_next:
  %ri1 = add i64 %ri, 1
  store i64 %ri1, i64* %ri_slot
  br label %rx_lp

rx_rearm:
  %opid = call i64 @mmio_read32(i64 1343226528)
  %nopid = xor i64 %opid, 8192
  %wop = call i64 @mmio_write32(i64 1343226528, i64 %nopid)
  %octrl = or i64 %nopid, 64
  %oa = call i64 @__omni_usb_dpram_arm(i64 1343226004, i64 %octrl)
  br label %done

done:
  ret i64 0
}

define i64 @__omni_usb_irq_handler() {
entry:
  ; Read USB_INTS (0x50110098 = 1343291544): BUS_RESET=0x1000(4096), BUFF_STATUS=0x10(16), SETUP_REQ=0x10000(65536)
  %ints = call i64 @mmio_read32(i64 1343291544)
  %b_rst = and i64 %ints, 4096
  %is_rst = icmp ne i64 %b_rst, 0
  br i1 %is_rst, label %do_rst, label %chk_buf

do_rst:
  %hr = call i64 @__omni_usb_handle_bus_reset()
  br label %chk_buf

chk_buf:
  %b_buf = and i64 %ints, 16
  %is_buf = icmp ne i64 %b_buf, 0
  br i1 %is_buf, label %do_buf, label %chk_setup

do_buf:
  %hb = call i64 @__omni_usb_handle_buff_status()
  br label %chk_setup

chk_setup:
  %b_set = and i64 %ints, 65536
  %is_set = icmp ne i64 %b_set, 0
  br i1 %is_set, label %do_set, label %done

do_set:
  %hs = call i64 @__omni_usb_handle_setup()
  br label %done

done:
  ret i64 0
}

define i64 @__omni_usb_cdc_init() {
entry:
  ; 1. Zero all 4 KB of USBCTRL_DPRAM (0x50100000..0x50101000) in native Thumb
  %zd = call i64 @__omni_usb_zero_dpram()

  ; 2. Initialize default line coding 115200 8N1 at 0x501002A8 (1343226536)
  %lc0 = call i64 @mmio_write32(i64 1343226536, i64 115200)
  %lc1 = call i64 @mmio_write32(i64 1343226540, i64 524288)

  ; 3. Configure Endpoint Controls: EP1_IN, EP2_IN, EP2_OUT
  %e1 = call i64 @mmio_write32(i64 1343225864, i64 2885681536)
  %e2 = call i64 @mmio_write32(i64 1343225872, i64 2818572800)
  %e3 = call i64 @mmio_write32(i64 1343225876, i64 2818572864)

  ; 4. Mux USB PHY (0x50110074 = 1343291508 <- 9), force VBUS detect (0x50110078 = 1343291512 <- 12)
  %mx = call i64 @mmio_write32(i64 1343291508, i64 9)
  %pw = call i64 @mmio_write32(i64 1343291512, i64 12)
  ; Enable USB controller (0x50110040 = 1343291456 <- 1)
  %mc = call i64 @mmio_write32(i64 1343291456, i64 1)
  ; SIE_CTRL (0x5011004C = 1343291468) = EP0_INT_1BUF (0x20000000 = 536870912)
  %sc0 = call i64 @mmio_write32(i64 1343291468, i64 536870912)
  ; INTE (0x50110090 = 1343291536) = SETUP_REQ(65536) | BUS_RESET(4096) | BUFF_STATUS(16) = 69648
  %ie = call i64 @mmio_write32(i64 1343291536, i64 69648)
  ; Assert D+ Full-Speed pullup: SIE_CTRL = EP0_INT_1BUF | PULLUP_EN = 0x20010000 (536936448)
  %sc1 = call i64 @mmio_write32(i64 1343291468, i64 536936448)

  ; Mark usb_hw_inited = 1 (0x501002C0 = 1343226560) & enable USBCTRL_IRQ in NVIC_ISER0 (0xE000E100 = 3758153984)
  %inited = call i64 @mmio_write32(i64 1343226560, i64 1)
  %msk = call i64 @__omni_usb_irq_mask()
  %nv = call i64 @mmio_write32(i64 3758153984, i64 %msk)
  ret i64 0
}

define i64 @usb_cdc_getc() {
entry:
  %inited = call i64 @mmio_read32(i64 1343226560)
  %ok = icmp ne i64 %inited, 0
  br i1 %ok, label %chk_rx, label %empty

chk_rx:
  %rhd = call i64 @mmio_read32(i64 1343226552)
  %rtl = call i64 @mmio_read32(i64 1343226556)
  %has = icmp ne i64 %rhd, %rtl
  br i1 %has, label %pop_b, label %empty

pop_b:
  %ptr = add i64 1343226624, %rtl
  %b = call i64 @mem_read8(i64 %ptr)
  %ntl0 = add i64 %rtl, 1
  %ntl = and i64 %ntl0, 255
  %wt = call i64 @mmio_write32(i64 1343226556, i64 %ntl)
  ret i64 %b

empty:
  ret i64 -1
}
"#;

pub const USB_CDC_RODATA_SYMS: &[&str] = &[
    "__usb_dev_desc",
    "__usb_cfg_desc",
    "__usb_str0",
    "__usb_str1",
    "__usb_str2",
    "__usb_str3",
];
