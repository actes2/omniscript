#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpChipKind {
    Rp2040,
    Rp2350,
}

#[derive(Debug, Clone, Copy)]
pub struct RpChipProfile {
    pub kind: RpChipKind,
    pub has_thumb2: bool,
    pub flash_base: u32,
    pub vector_table_vma: u32,
    pub sram_base: u32,
    pub sram_top_msp: u32,
    pub heap_start: u32,
    pub heap_limit: u32,
    pub xosc_base: u32,
    pub clocks_base: u32,
    pub resets_base: u32,
    pub resets_unreset_mask: u32,
    pub io_bank0_base: u32,
    pub pads_bank0_base: u32,
    pub pads_clear_iso: bool,
    pub uart0_base: u32,
    pub timer_base: u32,
    pub watchdog_base: u32,
    pub ticks_base: u32,
    pub pio0_base: u32,
    pub pio1_base: u32,
    pub pio2_base: u32,
    pub sio_base: u32,
    pub sio_gpio_in: u32,
    pub sio_gpio_out: u32,
    pub sio_gpio_out_set: u32,
    pub sio_gpio_out_clr: u32,
    pub sio_gpio_out_xor: u32,
    pub sio_gpio_oe_set: u32,
    pub sio_gpio_oe_clr: u32,
    pub uf2_family_id: u32,
}

impl RpChipProfile {
    pub const fn rp2040() -> Self {
        Self {
            kind: RpChipKind::Rp2040,
            has_thumb2: false,
            flash_base: 0x1000_0000,
            vector_table_vma: 0x1000_0100, // Immediately after 256-byte CRC32 boot2
            sram_base: 0x2000_0000,
            sram_top_msp: 0x2004_2000, // 264 KB SRAM top
            heap_start: 0x2000_4000,
            heap_limit: 0x2003_8000,
            xosc_base: 0x4002_4000,
            clocks_base: 0x4000_8000,
            resets_base: 0x4000_C000,
            // IO_BANK0 (1<<5) | PADS_BANK0 (1<<8) | PIO0 (1<<10) | PIO1 (1<<11) | TIMER (1<<21) | UART0 (1<<22)
            resets_unreset_mask: 0x0060_0D20,
            io_bank0_base: 0x4001_4000,
            pads_bank0_base: 0x4001_C000,
            pads_clear_iso: false,
            uart0_base: 0x4003_4000,
            timer_base: 0x4005_4000,
            watchdog_base: 0x4005_8000,
            ticks_base: 0,
            pio0_base: 0x5020_0000,
            pio1_base: 0x5030_0000,
            pio2_base: 0x5020_0000,
            sio_base: 0xD000_0000,
            sio_gpio_in: 0x04,
            sio_gpio_out: 0x10,
            sio_gpio_out_set: 0x14,
            sio_gpio_out_clr: 0x18,
            sio_gpio_out_xor: 0x1C,
            sio_gpio_oe_set: 0x24,
            sio_gpio_oe_clr: 0x28,
            uf2_family_id: 0xE48B_FF56, // RP2040
        }
    }

    pub const fn rp2350() -> Self {
        Self {
            kind: RpChipKind::Rp2350,
            has_thumb2: true,
            flash_base: 0x1000_0000,
            vector_table_vma: 0x1000_0000, // Mask ROM configures XIP directly via PICOBIN IMAGE_DEF
            sram_base: 0x2000_0000,
            sram_top_msp: 0x2008_2000, // 520 KB SRAM top
            heap_start: 0x2000_4000,
            heap_limit: 0x2007_8000,
            xosc_base: 0x4004_8000,
            clocks_base: 0x4001_0000,
            resets_base: 0x4002_0000,
            // IO_BANK0 (1<<6) | PADS_BANK0 (1<<9) | PIO0 (1<<11) | PIO1 (1<<12) | PIO2 (1<<13) | TIMER0 (1<<23) | UART0 (1<<26)
            resets_unreset_mask: 0x0480_3A40,
            io_bank0_base: 0x4002_8000,
            pads_bank0_base: 0x4003_8000,
            pads_clear_iso: true, // Clear bit 8 ISO pad isolation on RP2350!
            uart0_base: 0x4007_0000,
            timer_base: 0x400B_0000,
            watchdog_base: 0,
            ticks_base: 0x4010_8000,
            pio0_base: 0x5020_0000,
            pio1_base: 0x5030_0000,
            pio2_base: 0x5040_0000,
            sio_base: 0xD000_0000,
            sio_gpio_in: 0x04,
            sio_gpio_out: 0x10,
            sio_gpio_out_set: 0x18,
            sio_gpio_out_clr: 0x20,
            sio_gpio_out_xor: 0x28,
            sio_gpio_oe_set: 0x38,
            sio_gpio_oe_clr: 0x40,
            uf2_family_id: 0xE48B_FF59, // RP2350_ARM_S
        }
    }

    pub fn from_target_triple(triple: &str) -> Option<Self> {
        let low = triple.trim().to_ascii_lowercase();
        if low.starts_with("thumbv6m") || low.contains("rp2040") || low.contains("cortex-m0") {
            Some(Self::rp2040())
        } else if low.starts_with("thumbv8m")
            || low.contains("rp2350")
            || low.contains("cortex-m33")
        {
            Some(Self::rp2350())
        } else {
            None
        }
    }
}

/// Non-reflected IEEE 802.3 / MPEG-2 CRC-32 (`poly = 0x04C11DB7`, `init = 0xFFFFFFFF`, `xorout = 0`)
/// required by the RP2040 mask ROM at byte offset `0xFC` of the 256-byte `boot2` flash sector.
pub fn rp2040_boot2_crc32(data_252: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data_252 {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Synthesize a pure-Rust 256-byte RP2040 `boot2` XIP flash stage (ARMv6-M 16-bit Thumb + `MSR MSP`)
/// configuring `XIP_SSI` (`0x18000000`) for `03h` linear XIP read, setting `SCB->VTOR = 0x10000100`,
/// loading `MSP` and `Reset_Handler`, and appending the exact RP2040 ROM CRC-32 at `0xFC..0x100`.
pub fn build_rp2040_boot2_256b() -> [u8; 256] {
    let mut b = [0u8; 256];
    let mut off = 0usize;
    let mut put16 = |hw: u16| {
        b[off] = (hw & 0xFF) as u8;
        b[off + 1] = (hw >> 8) as u8;
        off += 2;
    };

    // Literal pool will sit at byte offset `0x30` (48, 4-byte aligned):
    //   [0x30] = 0x1800_0000 (XIP_SSI_BASE)
    //   [0x34] = 0x001F_0300 (CTRLR0: DFS_32=31<<16, TMOD=EEPROM_READ=3<<8)
    //   [0x38] = 0x0300_0218 (SPI_CTRLR0: XIP_CMD=0x03<<24, INST_L=2<<8, ADDR_L=6<<2)
    //   [0x3C] = 0x1000_0100 (App Vector Table VMA)
    //   [0x40] = 0xE000_ED08 (SCB->VTOR)
    // Note: In Thumb, `LDR Rd, [PC, #imm8*4]` uses `Align(PC, 4) = Align(cur_off + 4, 4)`!

    // 0x00: ldr r3, [pc, #44]  -> Align(0x04,4)=0x04 + 44 = 0x30 (XIP_SSI_BASE)
    put16(0x4B0B);
    // 0x02: movs r0, #0
    put16(0x2000);
    // 0x04: str r0, [r3, #0x08] (SSIENR = 0)
    put16(0x6098);
    // 0x06: movs r1, #4
    put16(0x2104);
    // 0x08: str r1, [r3, #0x14] (BAUDR = 4)
    put16(0x6159);
    // 0x0A: ldr r1, [pc, #36]  -> Align(0x0E,4)=0x0C + 40 = 0x34 -> imm8=10 (0x490A)
    put16(0x490A);
    // 0x0C: str r1, [r3, #0x00] (CTRLR0)
    put16(0x6019);
    // 0x0E: str r0, [r3, #0x04] (CTRLR1 = 0)
    put16(0x6058);
    // 0x10: ldr r1, [pc, #36]  -> Align(0x14,4)=0x14 + 36 = 0x38 -> imm8=9 (0x4909)
    put16(0x4909);
    // 0x12: movs r2, #0xF4
    put16(0x22F4);
    // 0x14: str r1, [r3, r2]    (SPI_CTRLR0 at XIP_SSI_BASE + 0xF4 -> `0x5099`: STR Rt=1, Rn=3, Rm=2)
    put16(0x5099);
    // 0x16: movs r1, #1
    put16(0x2101);
    // 0x18: str r1, [r3, #0x08] (SSIENR = 1)
    put16(0x6099);
    // 0x1A: ldr r0, [pc, #32]  -> Align(0x1E,4)=0x1C + 32 = 0x3C -> imm8=8 (0x4808: 0x10000100)
    put16(0x4808);
    // 0x1C: ldr r1, [pc, #32]  -> Align(0x20,4)=0x20 + 32 = 0x40 -> imm8=8 (0x4908: 0xE000ED08)
    put16(0x4908);
    // 0x1E: str r0, [r1, #0]    (SCB->VTOR = 0x10000100)
    put16(0x6008);
    // 0x20: ldr r1, [r0, #0]    (r1 = initial MSP)
    put16(0x6801);
    // 0x22..0x26: msr msp, r1   (32-bit Thumb `0xF381 0x8808`)
    put16(0xF381);
    put16(0x8808);
    // 0x26: ldr r1, [r0, #4]    (r1 = Reset_Handler | 1)
    put16(0x6841);
    // 0x28: bx r1
    put16(0x4708);
    // 0x2A..0x30: nop padding to align literal pool at 0x30 (3 halfwords: 0x2A, 0x2C, 0x2E)
    put16(0x46C0);
    put16(0x46C0);
    put16(0x46C0);
    let _ = put16;
    debug_assert_eq!(off, 0x30);

    let lits: [u32; 5] = [
        0x1800_0000,
        0x001F_0300,
        0x0300_0218,
        0x1000_0100,
        0xE000_ED08,
    ];
    for (i, &w) in lits.iter().enumerate() {
        let base = 0x30 + i * 4;
        b[base..base + 4].copy_from_slice(&w.to_le_bytes());
    }

    let crc = rp2040_boot2_crc32(&b[..252]);
    b[252..256].copy_from_slice(&crc.to_le_bytes());
    b
}

/// Mandatory 5-word RP2350 `PICOBIN` `IMAGE_DEF` block (`IMAGE_TYPE_EXE | SECURE | ARM | RP2350`).
pub const fn build_rp2350_picobin_block() -> [u32; 5] {
    [
        0xFFFF_DED3, // PICOBIN_BLOCK_MARKER_START
        0x1021_0142, // ITEM_1BS_IMAGE_TYPE (size=1): EXE | SECURE | CPU_ARM | CHIP_RP2350
        0x0000_01FF, // ITEM_2BS_LAST (total items size = 1 word)
        0x0000_0000, // Self-referencing 0-offset block loop
        0xAB12_3579, // PICOBIN_BLOCK_MARKER_END
    ]
}
