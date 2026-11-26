pub mod chip;
pub mod cyw43_fw;
pub mod elf_uf2;
pub mod ir_lower;
pub mod runtime_fw;
pub mod thumb_enc;
pub mod usb_cdc_fw;

use std::fs;
use std::path::Path;

use crate::VerifyError;
use chip::RpChipProfile;
use elf_uf2::{build_elf32_arm_image, build_uf2_image};
use ir_lower::{compile_llvm_ir_to_flash_image, validate_ir_for_cortex_m};

/// Pure-Rust Cortex-M SSA IR verification (zero external `clang` dependency).
/// Validates CFG structure AND dry-runs full SSA register/instruction lowering so any
/// undefined `%reg` from local LLM synthesis triggers `omni_codegen` self-repair immediately.
pub fn verify_cortex_m_ir(ir: &str) -> Result<(), VerifyError> {
    validate_ir_for_cortex_m(ir).map_err(|(line, message)| VerifyError::SyntaxError {
        file: "<cortex-m-ir>".to_string(),
        line,
        column: 1,
        message: message.clone(),
        raw_stderr: message,
    })?;

    compile_llvm_ir_to_flash_image(ir, &RpChipProfile::rp2040()).map_err(|message| {
        VerifyError::SyntaxError {
            file: "<cortex-m-ir>".to_string(),
            line: 1,
            column: 1,
            message: message.clone(),
            raw_stderr: message,
        }
    })?;

    Ok(())
}

/// Pure-Rust LLVM IR -> Bare-Metal `RP2040` / `RP2350` Firmware Compiler & UF2/ELF Packager.
/// Emits both `.uf2` (for drag-and-drop `BOOTSEL` / `picotool load`) and `.elf` (for `openocd` / `objdump` / Ghidra).
pub fn compile_ir_to_rp_firmware(
    ir: &str,
    out_path: &Path,
    chip: &RpChipProfile,
) -> Result<(), VerifyError> {
    let (flash_bytes, asm) =
        compile_llvm_ir_to_flash_image(ir, chip).map_err(VerifyError::LinkError)?;

    let uf2_bytes = build_uf2_image(&flash_bytes, chip);
    let elf_bytes = build_elf32_arm_image(&flash_bytes, &asm, chip);

    let ext = out_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "elf" {
        fs::write(out_path, &elf_bytes).map_err(|e| VerifyError::Io(e.to_string()))?;
        let companion_uf2 = out_path.with_extension("uf2");
        fs::write(&companion_uf2, &uf2_bytes).map_err(|e| VerifyError::Io(e.to_string()))?;
    } else if ext == "bin" {
        fs::write(out_path, &flash_bytes).map_err(|e| VerifyError::Io(e.to_string()))?;
    } else {
        fs::write(out_path, &uf2_bytes).map_err(|e| VerifyError::Io(e.to_string()))?;
        let companion_elf = out_path.with_extension("elf");
        fs::write(&companion_elf, &elf_bytes).map_err(|e| VerifyError::Io(e.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chip::{build_rp2040_boot2_256b, build_rp2350_picobin_block, rp2040_boot2_crc32};

    #[test]
    fn test_rp2040_boot2_crc32_and_rp2350_picobin() {
        let boot2 = build_rp2040_boot2_256b();
        let expected_crc = rp2040_boot2_crc32(&boot2[..252]);
        let stored_crc = u32::from_le_bytes(boot2[252..256].try_into().unwrap());
        assert_eq!(expected_crc, stored_crc);
        assert_ne!(stored_crc, 0);

        let picobin = build_rp2350_picobin_block();
        assert_eq!(picobin[0], 0xFFFF_DED3);
        assert_eq!(picobin[1], 0x1021_0142);
        assert_eq!(picobin[4], 0xAB12_3579);
    }

    #[test]
    fn test_zero_dep_compile_rp2040_and_rp2350_uf2_and_elf() {
        let sample_ir = r#"
; ModuleID = 'blink_test'
target triple = "thumbv6m-none-eabi"

@.str.0 = private unnamed_addr constant [12 x i8] c"PICO_READY!\00", align 1

declare void @__omni_print_str(i8*)
declare i64 @gpio_init(i64)
declare i64 @gpio_set_dir(i64, i64)
declare i64 @gpio_put(i64, i64)

define i64 @__omni_fn_blink(i64 %pin) {
entry:
  %c0 = call i64 @gpio_init(i64 %pin)
  %c1 = call i64 @gpio_set_dir(i64 %pin, i64 1)
  %c2 = call i64 @gpio_put(i64 %pin, i64 1)
  call void @__omni_print_str(i8* getelementptr inbounds ([12 x i8], [12 x i8]* @.str.0, i64 0, i64 0))
  ret i64 42
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %r = call i64 @__omni_fn_blink(i64 25)
  %tr = trunc i64 %r to i32
  ret i32 %tr
}
"#;
        verify_cortex_m_ir(sample_ir).expect("pure-Rust Cortex-M IR validation failed");

        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(sample_ir, &chip)
                .expect("failed to lower IR to Cortex-M flash image");
            assert!(flash.len() > 512);
            assert!(asm.symbols.contains_key("Reset_Handler"));
            assert!(asm.symbols.contains_key("main"));
            assert!(asm.symbols.contains_key("__omni_fn_blink"));

            let uf2 = build_uf2_image(&flash, &chip);
            assert_eq!(uf2.len() % 512, 0);
            assert_eq!(&uf2[0..4], b"UF2\n");
            let fam = u32::from_le_bytes(uf2[28..32].try_into().unwrap());
            assert_eq!(fam, chip.uf2_family_id);

            let elf = build_elf32_arm_image(&flash, &asm, &chip);
            assert_eq!(&elf[0..4], b"\x7FELF");
            assert_eq!(elf[4], 1); // ELFCLASS32
            assert_eq!(u16::from_le_bytes(elf[18..20].try_into().unwrap()), 40); // EM_ARM
        }
    }

    // =========================================================================
    // Pure-Rust ARMv6-M / ARMv8-M.main Thumb Emulator for Bare-Metal Verification
    // =========================================================================
    use std::collections::HashMap;
    use runtime_fw::{
        OMNI_CLOSURE_MAGIC32, OMNI_MAP_MAGIC32, OMNI_OBJ_CANARY32, OMNI_OBJ_MAGIC32,
        OMNI_SLICE_MAGIC32, SRAM_G_HEAP_PTR_OFF, SRAM_G_RET_PTR_OFF,
    };

    const SENTINEL_EXIT_PC: u32 = 0xFFFF_FFFE;

    struct ThumbCpuEmu {
        chip: RpChipProfile,
        flash: Vec<u8>,
        sram: Vec<u8>,
        usb_dpram: Vec<u8>,
        r: [u32; 16],
        n: bool,
        z: bool,
        c: bool,
        v: bool,
        simulate_hw_locks: bool,
        sim_timer_us: std::cell::Cell<u64>,
        sio_out: u32,
        sio_oe: u32,
        cyw43_slave: Option<SimCyw43439>,
        mmio_regs: HashMap<u32, u32>,
        mmio_write_log: Vec<(u32, u32)>,
        uart_tx: Vec<u8>,
    }

    impl ThumbCpuEmu {
        fn new(chip: RpChipProfile, flash: Vec<u8>, simulate_hw_locks: bool) -> Self {
            let sram_len = (chip.sram_top_msp - chip.sram_base) as usize;
            let mut r = [0u32; 16];
            r[13] = chip.sram_top_msp;
            Self {
                chip,
                flash,
                sram: vec![0u8; sram_len],
                usb_dpram: vec![0u8; 4096],
                r,
                n: false,
                z: false,
                c: false,
                v: false,
                simulate_hw_locks,
                sim_timer_us: std::cell::Cell::new(1_000),
                sio_out: 0,
                sio_oe: 0,
                cyw43_slave: None,
                mmio_regs: HashMap::new(),
                mmio_write_log: Vec::new(),
                uart_tx: Vec::new(),
            }
        }

        fn init_heap_only(&mut self) {
            self.write32(self.chip.sram_base + SRAM_G_HEAP_PTR_OFF, self.chip.heap_start);
            self.write32(self.chip.sram_base + SRAM_G_RET_PTR_OFF, 0);
        }

        fn read8(&self, addr: u32) -> u8 {
            if addr >= self.chip.usb_dpram_base && addr < self.chip.usb_dpram_base + 0x1000 {
                return self.usb_dpram[(addr - self.chip.usb_dpram_base) as usize];
            }
            if addr >= self.chip.flash_base {
                let off = (addr - self.chip.flash_base) as usize;
                if off < self.flash.len() {
                    return self.flash[off];
                }
            }
            if addr >= self.chip.sram_base {
                let off = (addr - self.chip.sram_base) as usize;
                if off < self.sram.len() {
                    return self.sram[off];
                }
            }
            panic!("invalid read8 at 0x{:08X}", addr);
        }

        fn read16(&self, addr: u32) -> u16 {
            assert_eq!(addr & 1, 0, "unaligned read16 at 0x{:08X}", addr);
            (self.read8(addr) as u16) | ((self.read8(addr + 1) as u16) << 8)
        }

        fn read32(&self, addr: u32) -> u32 {
            assert_eq!(addr & 3, 0, "unaligned read32 at 0x{:08X} (PC=0x{:08X})", addr, self.r[15]);
            let is_usb_dpram =
                addr >= self.chip.usb_dpram_base && addr < self.chip.usb_dpram_base + 0x1000;
            if addr >= 0x4000_0000 && !is_usb_dpram {
                if self.simulate_hw_locks {
                    if addr == self.chip.xosc_base + 0x04 {
                        return 0x8000_0000; // XOSC.STATUS.STABLE
                    }
                    if addr == self.chip.resets_base + 0x08 {
                        let rst = self.mmio_regs.get(&self.chip.resets_base).copied().unwrap_or(0xFFFF_FFFF);
                        return !rst;
                    }
                    if addr == self.chip.pll_sys_base || addr == self.chip.pll_usb_base {
                        let cs = self.mmio_regs.get(&addr).copied().unwrap_or(1);
                        let pwr = self.mmio_regs.get(&(addr + 0x04)).copied().unwrap_or(0x2D);
                        if (pwr & 0x21) == 0 {
                            return cs | 0x8000_0000; // CS.LOCK
                        }
                        return cs;
                    }
                    if addr == self.chip.clocks_base + 0x38 {
                        let ctrl = self.mmio_regs.get(&(self.chip.clocks_base + 0x30)).copied().unwrap_or(0);
                        return 1 << (ctrl & 3); // CLK_REF_SELECTED
                    }
                    if addr == self.chip.clocks_base + 0x44 {
                        let ctrl = self.mmio_regs.get(&(self.chip.clocks_base + 0x3C)).copied().unwrap_or(0);
                        return 1 << (ctrl & 1); // CLK_SYS_SELECTED
                    }
                    if addr == self.chip.timer_base + 0x28 {
                        let next = self.sim_timer_us.get().wrapping_add(50_000);
                        self.sim_timer_us.set(next);
                        return next as u32;
                    }
                    if addr == self.chip.timer_base + 0x24 {
                        return (self.sim_timer_us.get() >> 32) as u32;
                    }
                }
                if addr == self.chip.sio_base + self.chip.sio_gpio_in {
                    if let Some(ref cyw) = self.cyw43_slave {
                        return cyw.gpio_in_word(self.sio_out, self.sio_oe);
                    }
                    return self.sio_out & self.sio_oe;
                }
                if addr == self.chip.uart0_base + 0x18 {
                    return 0; // UARTFR: TXFF == 0 (ready)
                }
                return self.mmio_regs.get(&addr).copied().unwrap_or(0);
            }
            (self.read8(addr) as u32)
                | ((self.read8(addr + 1) as u32) << 8)
                | ((self.read8(addr + 2) as u32) << 16)
                | ((self.read8(addr + 3) as u32) << 24)
        }

        fn write8(&mut self, addr: u32, val: u8) {
            if addr >= self.chip.usb_dpram_base && addr < self.chip.usb_dpram_base + 0x1000 {
                self.usb_dpram[(addr - self.chip.usb_dpram_base) as usize] = val;
                return;
            }
            if addr >= self.chip.sram_base {
                let off = (addr - self.chip.sram_base) as usize;
                if off < self.sram.len() {
                    self.sram[off] = val;
                    return;
                }
            }
            panic!("invalid write8 at 0x{:08X}", addr);
        }

        fn write16(&mut self, addr: u32, val: u16) {
            assert_eq!(addr & 1, 0, "unaligned write16 at 0x{:08X}", addr);
            self.write8(addr, (val & 0xFF) as u8);
            self.write8(addr + 1, (val >> 8) as u8);
        }

        fn write32(&mut self, addr: u32, val: u32) {
            assert_eq!(addr & 3, 0, "unaligned write32 at 0x{:08X}", addr);
            let is_usb_dpram =
                addr >= self.chip.usb_dpram_base && addr < self.chip.usb_dpram_base + 0x1000;
            if addr >= 0x4000_0000 && !is_usb_dpram {
                if addr >= self.chip.sio_base && addr < self.chip.sio_base + 0x100 {
                    let prev_out = self.sio_out;
                    let prev_oe = self.sio_oe;
                    let off = addr - self.chip.sio_base;
                    if off == self.chip.sio_gpio_out {
                        self.sio_out = val;
                    } else if off == self.chip.sio_gpio_out_set {
                        self.sio_out |= val;
                    } else if off == self.chip.sio_gpio_out_clr {
                        self.sio_out &= !val;
                    } else if off == self.chip.sio_gpio_out_xor {
                        self.sio_out ^= val;
                    } else if off == self.chip.sio_gpio_oe_set {
                        self.sio_oe |= val;
                    } else if off == self.chip.sio_gpio_oe_clr {
                        self.sio_oe &= !val;
                    }
                    if let Some(ref mut cyw) = self.cyw43_slave {
                        cyw.on_sio_transition(prev_out, prev_oe, self.sio_out, self.sio_oe);
                    } else {
                        self.mmio_write_log.push((addr, val));
                    }
                    return;
                }
                self.mmio_write_log.push((addr, val));
                if addr == self.chip.uart0_base {
                    self.uart_tx.push((val & 0xFF) as u8);
                    return;
                }
                // Handle RP2040/RP2350 Atomic Set (+0x2000) / Clear (+0x3000) aliases on RESETS & USBCTRL_REGS
                if addr == self.chip.resets_base + 0x2000 {
                    let cur = self.mmio_regs.get(&self.chip.resets_base).copied().unwrap_or(0xFFFF_FFFF);
                    self.mmio_regs.insert(self.chip.resets_base, cur | val);
                    return;
                }
                if addr == self.chip.resets_base + 0x3000 {
                    let cur = self.mmio_regs.get(&self.chip.resets_base).copied().unwrap_or(0xFFFF_FFFF);
                    self.mmio_regs.insert(self.chip.resets_base, cur & !val);
                    return;
                }
                if addr >= self.chip.usb_regs_base + 0x2000 && addr < self.chip.usb_regs_base + 0x2100 {
                    let base = addr - 0x2000;
                    let cur = self.mmio_regs.get(&base).copied().unwrap_or(0);
                    self.mmio_regs.insert(base, cur | val);
                    return;
                }
                if addr >= self.chip.usb_regs_base + 0x3000 && addr < self.chip.usb_regs_base + 0x3100 {
                    let base = addr - 0x3000;
                    let cur = self.mmio_regs.get(&base).copied().unwrap_or(0);
                    self.mmio_regs.insert(base, cur & !val);
                    return;
                }
                self.mmio_regs.insert(addr, val);
                return;
            }
            self.write8(addr, (val & 0xFF) as u8);
            self.write8(addr + 1, ((val >> 8) & 0xFF) as u8);
            self.write8(addr + 2, ((val >> 16) & 0xFF) as u8);
            self.write8(addr + 3, ((val >> 24) & 0xFF) as u8);
        }

        fn read_cstr(&self, ptr: u32) -> String {
            let mut out = Vec::new();
            let mut cur = ptr;
            loop {
                let b = self.read8(cur);
                if b == 0 {
                    break;
                }
                out.push(b);
                cur += 1;
            }
            String::from_utf8_lossy(&out).to_string()
        }

        fn set_nz(&mut self, res: u32) {
            self.n = (res & 0x8000_0000) != 0;
            self.z = res == 0;
        }

        fn add_with_carry(&mut self, a: u32, b: u32, cin: bool) -> u32 {
            let sum64 = (a as u64) + (b as u64) + (cin as u64);
            let res = sum64 as u32;
            self.set_nz(res);
            self.c = (sum64 >> 32) != 0;
            self.v = (((a ^ !b) & (a ^ res)) & 0x8000_0000) != 0;
            res
        }

        fn cond_holds(&self, cond: u8) -> bool {
            match cond {
                0x0 => self.z,
                0x1 => !self.z,
                0x2 => self.c,
                0x3 => !self.c,
                0x4 => self.n,
                0x5 => !self.n,
                0x6 => self.v,
                0x7 => !self.v,
                0x8 => self.c && !self.z,
                0x9 => !self.c || self.z,
                0xA => self.n == self.v,
                0xB => self.n != self.v,
                0xC => !self.z && (self.n == self.v),
                0xD => self.z || (self.n != self.v),
                0xE => true,
                _ => false,
            }
        }

        fn call_fn(&mut self, asm: & thumb_enc::ThumbAssembler, name: &str, args: [u32; 4], max_steps: usize) -> u64 {
            let off = *asm.symbols.get(name).unwrap_or_else(|| panic!("missing symbol {}", name));
            self.r[0] = args[0];
            self.r[1] = args[1];
            self.r[2] = args[2];
            self.r[3] = args[3];
            self.r[14] = SENTINEL_EXIT_PC | 1;
            self.r[15] = self.chip.flash_base + (off as u32);
            self.run(max_steps);
            (self.r[0] as u64) | ((self.r[1] as u64) << 32)
        }

        fn run(&mut self, max_steps: usize) {
            for _ in 0..max_steps {
                let pc = self.r[15] & !1;
                if pc == SENTINEL_EXIT_PC {
                    return;
                }
                let hw1 = self.read16(pc);
                if hw1 == 0xBF30 {
                    // WFI -> clean halt (e.g. end of Reset_Handler after main returns)
                    return;
                }
                if hw1 == 0xBF00 || hw1 == 0x46C0 {
                    self.r[15] = pc + 2;
                    continue;
                }

                // 32-bit Thumb-2 encodings (`hw1 >= 0xE800`)
                if hw1 >= 0xE800 {
                    let hw2 = self.read16(pc + 2);
                    // BL (`0xF000..0xF7FF`, `hw2 & 0xD000 == 0xD000`) or B.W (`hw2 & 0xD000 == 0x9000`)
                    if (hw1 & 0xF800) == 0xF000 && ((hw2 & 0xD000) == 0xD000 || (hw2 & 0xD000) == 0x9000) {
                        let s = ((hw1 >> 10) & 1) as i32;
                        let imm10 = (hw1 & 0x3FF) as i32;
                        let j1 = ((hw2 >> 13) & 1) as i32;
                        let j2 = ((hw2 >> 11) & 1) as i32;
                        let imm11 = (hw2 & 0x7FF) as i32;
                        let i1 = (!(j1 ^ s)) & 1;
                        let i2 = (!(j2 ^ s)) & 1;
                        let mut rel = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
                        if s != 0 {
                            rel |= !0x01FF_FFFF;
                        }
                        if (hw2 & 0xD000) == 0xD000 {
                            self.r[14] = (pc + 4) | 1;
                        }
                        self.r[15] = ((pc as i64) + 4 + (rel as i64)) as u32;
                        continue;
                    }
                    // MOVW (`0xF240`) / MOVT (`0xF2C0`)
                    if (hw1 & 0xFBF0) == 0xF240 || (hw1 & 0xFBF0) == 0xF2C0 {
                        let i = ((hw1 >> 10) & 1) as u32;
                        let imm4 = (hw1 & 0xF) as u32;
                        let imm3 = ((hw2 >> 12) & 7) as u32;
                        let rd = ((hw2 >> 8) & 0xF) as usize;
                        let imm8 = (hw2 & 0xFF) as u32;
                        let imm16 = (imm4 << 12) | (i << 11) | (imm3 << 8) | imm8;
                        if (hw1 & 0x0080) == 0 {
                            self.r[rd] = imm16;
                        } else {
                            self.r[rd] = (self.r[rd] & 0xFFFF) | (imm16 << 16);
                        }
                        self.r[15] = pc + 4;
                        continue;
                    }
                    // SDIV (`0xFB90`) / UDIV (`0xFBB0`)
                    if ((hw1 & 0xFFF0) == 0xFB90 || (hw1 & 0xFFF0) == 0xFBB0) && (hw2 & 0xF0F0) == 0xF0F0 {
                        let rn = (hw1 & 0xF) as usize;
                        let rd = ((hw2 >> 8) & 0xF) as usize;
                        let rm = (hw2 & 0xF) as usize;
                        let b = self.r[rm];
                        self.r[rd] = if b == 0 {
                            0
                        } else if (hw1 & 0x0020) == 0 {
                            (self.r[rn] as i32).wrapping_div(b as i32) as u32
                        } else {
                            self.r[rn] / b
                        };
                        self.r[15] = pc + 4;
                        continue;
                    }
                    panic!("unhandled Thumb-2 instruction 0x{:04X} 0x{:04X} at 0x{:08X}", hw1, hw2, pc);
                }

                // 16-bit Thumb-1 decode
                let top = hw1 >> 11;
                match top {
                    0x00 => {
                        // LSLS Rd, Rm, #imm5
                        let imm5 = ((hw1 >> 6) & 0x1F) as u32;
                        let rm = ((hw1 >> 3) & 7) as usize;
                        let rd = (hw1 & 7) as usize;
                        let v = self.r[rm];
                        let res = if imm5 == 0 {
                            v
                        } else {
                            self.c = ((v >> (32 - imm5)) & 1) != 0;
                            v << imm5
                        };
                        self.r[rd] = res;
                        self.set_nz(res);
                        self.r[15] = pc + 2;
                    }
                    0x01 => {
                        // LSRS Rd, Rm, #imm5
                        let imm5 = ((hw1 >> 6) & 0x1F) as u32;
                        let rm = ((hw1 >> 3) & 7) as usize;
                        let rd = (hw1 & 7) as usize;
                        let v = self.r[rm];
                        let res = if imm5 == 0 {
                            self.c = (v >> 31) != 0;
                            0
                        } else {
                            self.c = ((v >> (imm5 - 1)) & 1) != 0;
                            v >> imm5
                        };
                        self.r[rd] = res;
                        self.set_nz(res);
                        self.r[15] = pc + 2;
                    }
                    0x02 => {
                        // ASRS Rd, Rm, #imm5
                        let imm5 = ((hw1 >> 6) & 0x1F) as u32;
                        let rm = ((hw1 >> 3) & 7) as usize;
                        let rd = (hw1 & 7) as usize;
                        let v = self.r[rm];
                        let res = if imm5 == 0 {
                            self.c = (v >> 31) != 0;
                            ((v as i32) >> 31) as u32
                        } else {
                            self.c = ((v >> (imm5 - 1)) & 1) != 0;
                            ((v as i32) >> imm5) as u32
                        };
                        self.r[rd] = res;
                        self.set_nz(res);
                        self.r[15] = pc + 2;
                    }
                    0x03 => {
                        let op = (hw1 >> 9) & 3;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rd = (hw1 & 7) as usize;
                        let rm_or_imm = ((hw1 >> 6) & 7) as u32;
                        let res = match op {
                            0 => self.add_with_carry(self.r[rn], self.r[rm_or_imm as usize], false),
                            1 => self.add_with_carry(self.r[rn], !self.r[rm_or_imm as usize], true),
                            2 => self.add_with_carry(self.r[rn], rm_or_imm, false),
                            _ => self.add_with_carry(self.r[rn], !rm_or_imm, true),
                        };
                        self.r[rd] = res;
                        self.r[15] = pc + 2;
                    }
                    0x04 => {
                        let rd = ((hw1 >> 8) & 7) as usize;
                        let imm8 = (hw1 & 0xFF) as u32;
                        self.r[rd] = imm8;
                        self.set_nz(imm8);
                        self.r[15] = pc + 2;
                    }
                    0x05 => {
                        let rn = ((hw1 >> 8) & 7) as usize;
                        let imm8 = (hw1 & 0xFF) as u32;
                        let _ = self.add_with_carry(self.r[rn], !imm8, true);
                        self.r[15] = pc + 2;
                    }
                    0x06 => {
                        let rdn = ((hw1 >> 8) & 7) as usize;
                        let imm8 = (hw1 & 0xFF) as u32;
                        let res = self.add_with_carry(self.r[rdn], imm8, false);
                        self.r[rdn] = res;
                        self.r[15] = pc + 2;
                    }
                    0x07 => {
                        let rdn = ((hw1 >> 8) & 7) as usize;
                        let imm8 = (hw1 & 0xFF) as u32;
                        let res = self.add_with_carry(self.r[rdn], !imm8, true);
                        self.r[rdn] = res;
                        self.r[15] = pc + 2;
                    }
                    0x08 => {
                        if (hw1 & 0x0400) == 0 {
                            // Data-processing register (`0x4000..0x43FF`)
                            let subop = (hw1 >> 6) & 0xF;
                            let rm = ((hw1 >> 3) & 7) as usize;
                            let rdn = (hw1 & 7) as usize;
                            let a = self.r[rdn];
                            let b = self.r[rm];
                            match subop {
                                0x0 => {
                                    let res = a & b;
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0x1 => {
                                    let res = a ^ b;
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0x2 => {
                                    let sh = b & 0xFF;
                                    let res = if sh == 0 {
                                        a
                                    } else if sh < 32 {
                                        self.c = ((a >> (32 - sh)) & 1) != 0;
                                        a << sh
                                    } else if sh == 32 {
                                        self.c = (a & 1) != 0;
                                        0
                                    } else {
                                        self.c = false;
                                        0
                                    };
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0x3 => {
                                    let sh = b & 0xFF;
                                    let res = if sh == 0 {
                                        a
                                    } else if sh < 32 {
                                        self.c = ((a >> (sh - 1)) & 1) != 0;
                                        a >> sh
                                    } else if sh == 32 {
                                        self.c = (a >> 31) != 0;
                                        0
                                    } else {
                                        self.c = false;
                                        0
                                    };
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0x4 => {
                                    let sh = b & 0xFF;
                                    let res = if sh == 0 {
                                        a
                                    } else if sh < 32 {
                                        self.c = ((a >> (sh - 1)) & 1) != 0;
                                        ((a as i32) >> sh) as u32
                                    } else {
                                        self.c = (a >> 31) != 0;
                                        ((a as i32) >> 31) as u32
                                    };
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0x5 => {
                                    let res = self.add_with_carry(a, b, self.c);
                                    self.r[rdn] = res;
                                }
                                0x6 => {
                                    let res = self.add_with_carry(a, !b, self.c);
                                    self.r[rdn] = res;
                                }
                                0x8 => {
                                    self.set_nz(a & b);
                                }
                                0x9 => {
                                    let res = self.add_with_carry(0, !b, true);
                                    self.r[rdn] = res;
                                }
                                0xA => {
                                    let _ = self.add_with_carry(a, !b, true);
                                }
                                0xC => {
                                    let res = a | b;
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0xD => {
                                    let res = a.wrapping_mul(b);
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0xE => {
                                    let res = a & !b;
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                0xF => {
                                    let res = !b;
                                    self.r[rdn] = res;
                                    self.set_nz(res);
                                }
                                _ => panic!("unhandled ALU subop {} at 0x{:08X}", subop, pc),
                            }
                            self.r[15] = pc + 2;
                        } else {
                            // Special data / BX / BLX (`0x4400..0x47FF`)
                            let sub = (hw1 >> 8) & 3;
                            let d_hi = ((hw1 >> 7) & 1) as usize;
                            let rm = ((hw1 >> 3) & 0xF) as usize;
                            let rd = ((d_hi << 3) | ((hw1 & 7) as usize)) & 0xF;
                            match sub {
                                0 => {
                                    self.r[rd] = self.r[rd].wrapping_add(self.r[rm]);
                                    self.r[15] = pc + 2;
                                }
                                1 => {
                                    let _ = self.add_with_carry(self.r[rd], !self.r[rm], true);
                                    self.r[15] = pc + 2;
                                }
                                2 => {
                                    self.r[rd] = self.r[rm];
                                    self.r[15] = pc + 2;
                                }
                                _ => {
                                    let target = self.r[rm];
                                    assert_eq!(target & 1, 1, "BX/BLX without Thumb bit 0 at 0x{:08X}: target=0x{:08X}", pc, target);
                                    if (hw1 & 0x0080) != 0 {
                                        self.r[14] = (pc + 2) | 1;
                                    }
                                    self.r[15] = target & !1;
                                }
                            }
                        }
                    }
                    0x09 => {
                        // LDR Rt, [PC, #imm8*4]
                        let rt = ((hw1 >> 8) & 7) as usize;
                        let imm = ((hw1 & 0xFF) as u32) * 4;
                        let addr = ((pc + 4) & !3) + imm;
                        self.r[rt] = self.read32(addr);
                        self.r[15] = pc + 2;
                    }
                    0x0A | 0x0B => {
                        let op = (hw1 >> 9) & 7;
                        let rm = ((hw1 >> 6) & 7) as usize;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        let addr = self.r[rn].wrapping_add(self.r[rm]);
                        match op {
                            0 => self.write32(addr, self.r[rt]),
                            4 => self.r[rt] = self.read32(addr),
                            _ => panic!("unhandled reg mem op {} at 0x{:08X}", op, pc),
                        }
                        self.r[15] = pc + 2;
                    }
                    0x0C => {
                        let imm = (((hw1 >> 6) & 0x1F) as u32) * 4;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.write32(self.r[rn].wrapping_add(imm), self.r[rt]);
                        self.r[15] = pc + 2;
                    }
                    0x0D => {
                        let imm = (((hw1 >> 6) & 0x1F) as u32) * 4;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.r[rt] = self.read32(self.r[rn].wrapping_add(imm));
                        self.r[15] = pc + 2;
                    }
                    0x0E => {
                        let imm = ((hw1 >> 6) & 0x1F) as u32;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.write8(self.r[rn].wrapping_add(imm), (self.r[rt] & 0xFF) as u8);
                        self.r[15] = pc + 2;
                    }
                    0x0F => {
                        let imm = ((hw1 >> 6) & 0x1F) as u32;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.r[rt] = self.read8(self.r[rn].wrapping_add(imm)) as u32;
                        self.r[15] = pc + 2;
                    }
                    0x10 => {
                        let imm = (((hw1 >> 6) & 0x1F) as u32) * 2;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.write16(self.r[rn].wrapping_add(imm), (self.r[rt] & 0xFFFF) as u16);
                        self.r[15] = pc + 2;
                    }
                    0x11 => {
                        let imm = (((hw1 >> 6) & 0x1F) as u32) * 2;
                        let rn = ((hw1 >> 3) & 7) as usize;
                        let rt = (hw1 & 7) as usize;
                        self.r[rt] = self.read16(self.r[rn].wrapping_add(imm)) as u32;
                        self.r[15] = pc + 2;
                    }
                    0x12 => {
                        let rt = ((hw1 >> 8) & 7) as usize;
                        let imm = ((hw1 & 0xFF) as u32) * 4;
                        self.write32(self.r[13].wrapping_add(imm), self.r[rt]);
                        self.r[15] = pc + 2;
                    }
                    0x13 => {
                        let rt = ((hw1 >> 8) & 7) as usize;
                        let imm = ((hw1 & 0xFF) as u32) * 4;
                        self.r[rt] = self.read32(self.r[13].wrapping_add(imm));
                        self.r[15] = pc + 2;
                    }
                    0x15 => {
                        let rd = ((hw1 >> 8) & 7) as usize;
                        let imm = ((hw1 & 0xFF) as u32) * 4;
                        self.r[rd] = self.r[13].wrapping_add(imm);
                        self.r[15] = pc + 2;
                    }
                    0x16 | 0x17 => {
                        if (hw1 & 0xFF00) == 0xB000 {
                            let imm = ((hw1 & 0x7F) as u32) * 4;
                            if (hw1 & 0x80) == 0 {
                                self.r[13] = self.r[13].wrapping_add(imm);
                            } else {
                                self.r[13] = self.r[13].wrapping_sub(imm);
                            }
                            self.r[15] = pc + 2;
                        } else if (hw1 & 0xFF00) == 0xB200 {
                            let op = (hw1 >> 6) & 3;
                            let rm = ((hw1 >> 3) & 7) as usize;
                            let rd = (hw1 & 7) as usize;
                            let v = self.r[rm];
                            self.r[rd] = match op {
                                0 => ((v as i16) as i32) as u32,
                                1 => ((v as i8) as i32) as u32,
                                2 => v & 0xFFFF,
                                _ => v & 0xFF,
                            };
                            self.r[15] = pc + 2;
                        } else if (hw1 & 0xFE00) == 0xB400 {
                            // PUSH
                            let push_lr = (hw1 & 0x0100) != 0;
                            let count = (hw1 & 0xFF).count_ones() + (push_lr as u32);
                            let mut addr = self.r[13].wrapping_sub(count * 4);
                            self.r[13] = addr;
                            for reg in 0..8 {
                                if (hw1 & (1 << reg)) != 0 {
                                    self.write32(addr, self.r[reg]);
                                    addr += 4;
                                }
                            }
                            if push_lr {
                                self.write32(addr, self.r[14]);
                            }
                            self.r[15] = pc + 2;
                        } else if (hw1 & 0xFE00) == 0xBC00 {
                            // POP
                            let pop_pc = (hw1 & 0x0100) != 0;
                            let mut addr = self.r[13];
                            for reg in 0..8 {
                                if (hw1 & (1 << reg)) != 0 {
                                    self.r[reg] = self.read32(addr);
                                    addr += 4;
                                }
                            }
                            if pop_pc {
                                let target = self.read32(addr);
                                addr += 4;
                                self.r[13] = addr;
                                self.r[15] = target & !1;
                            } else {
                                self.r[13] = addr;
                                self.r[15] = pc + 2;
                            }
                        } else if (hw1 & 0xFFE0) == 0xB660 {
                            // CPSIE / CPSID (`0xB662` / `0xB672`)
                            self.r[15] = pc + 2;
                        } else {
                            panic!("unhandled misc 0xBxxx instruction 0x{:04X} at 0x{:08X}", hw1, pc);
                        }
                    }
                    0x1A | 0x1B => {
                        let cond = ((hw1 >> 8) & 0xF) as u8;
                        let imm8 = (hw1 & 0xFF) as i8 as i32;
                        if self.cond_holds(cond) {
                            self.r[15] = ((pc as i32) + 4 + (imm8 << 1)) as u32;
                        } else {
                            self.r[15] = pc + 2;
                        }
                    }
                    0x1C => {
                        let mut imm11 = (hw1 & 0x7FF) as i32;
                        if (imm11 & 0x400) != 0 {
                            imm11 |= !0x7FF;
                        }
                        self.r[15] = ((pc as i32) + 4 + (imm11 << 1)) as u32;
                    }
                    _ => panic!("unhandled Thumb-1 instruction 0x{:04X} at 0x{:08X}", hw1, pc),
                }
            }
            panic!("ThumbCpuEmu exceeded max_steps ({}) at PC=0x{:08X}", max_steps, self.r[15]);
        }
    }

    #[test]
    fn test_ieee754_f64_arithmetic_fcmp_casts_and_uart_printing() {
        let ir = r#"
target triple = "thumbv6m-none-eabi"

declare void @__omni_print_float(double)
declare i8* @__omni_float_to_str(double)
declare double @__omni_c_str_to_f64(i8*)
declare void @__omni_print_str(i8*)
declare i32 @printf(i8*, ...)
declare double @llvm.pow.f64(double, double)
declare double @llvm.fabs.f64(double)

@.fmt_f = private unnamed_addr constant [7 x i8] c"v=%f!\0A\00", align 1
@.str_num = private unnamed_addr constant [8 x i8] c"-12.375\00", align 1

define i64 @test_f64_pipeline(i64 %in_int) {
entry:
  ; sitofp (-25 -> -25.0), fabs -> 25.0, fadd + 6.5 -> 31.5, fsub - 1.5 -> 30.0
  %f0 = sitofp i64 %in_int to double
  %f1 = call double @llvm.fabs.f64(double %f0)
  %f2 = fadd double %f1, 6.5
  %f3 = fsub double %f2, 1.5
  ; fmul * 3.5 -> 105.0, fdiv / 2.0 -> 52.5
  %f4 = fmul double %f3, 3.5
  %f5 = fdiv double %f4, 2.0
  ; pow(2.0, 3.0) = 8.0 -> 52.5 + 8.0 = 60.5
  %pw = call double @llvm.pow.f64(double 2.0, double 3.0)
  %f6 = fadd double %f5, %pw
  ; fpext / fptrunc round-trip of 0.25 -> 60.75
  %tr32 = fptrunc double 0.25 to float
  %ex64 = fpext float %tr32 to double
  %f7 = fadd double %f6, %ex64
  ; uitofp / frem / fneg / fptoui / __omni_c_str_to_f64 check:
  ; uitofp(10) = 10.0, frem(10.0, 4.0) = 2.0, parse("-12.375") -> fneg -> +12.375 -> fptoui -> 12 -> +2.0 -> 60.75 + 14.0 = 74.75 -> fptosi = 74
  %u10 = uitofp i32 10 to double
  %rm2 = frem double %u10, 4.0
  %parsed = call double @__omni_c_str_to_f64(i8* getelementptr inbounds ([8 x i8], [8 x i8]* @.str_num, i64 0, i64 0))
  %pos_p = fneg double %parsed
  %u12 = fptoui double %pos_p to i64
  %u12_f = uitofp i64 %u12 to double
  %f8 = fadd double %f7, %rm2
  %f9 = fadd double %f8, %u12_f
  %as_i64 = fptosi double %f9 to i64
  ret i64 %as_i64
}

define i32 @test_all_16_fcmp_predicates(double %a, double %b) {
entry:
  %c0  = fcmp false double %a, %b
  %c1  = fcmp oeq   double %a, %b
  %c2  = fcmp ogt   double %a, %b
  %c3  = fcmp oge   double %a, %b
  %c4  = fcmp olt   double %a, %b
  %c5  = fcmp ole   double %a, %b
  %c6  = fcmp one   double %a, %b
  %c7  = fcmp ord   double %a, %b
  %c8  = fcmp uno   double %a, %b
  %c9  = fcmp ueq   double %a, %b
  %c10 = fcmp ugt   double %a, %b
  %c11 = fcmp uge   double %a, %b
  %c12 = fcmp ult   double %a, %b
  %c13 = fcmp ule   double %a, %b
  %c14 = fcmp une   double %a, %b
  %c15 = fcmp true  double %a, %b

  %b0  = zext i1 %c0  to i32
  %b1  = zext i1 %c1  to i32
  %s1  = shl i32 %b1, 1
  %m1  = or i32 %b0, %s1
  %b2  = zext i1 %c2  to i32
  %s2  = shl i32 %b2, 2
  %m2  = or i32 %m1, %s2
  %b3  = zext i1 %c3  to i32
  %s3  = shl i32 %b3, 3
  %m3  = or i32 %m2, %s3
  %b4  = zext i1 %c4  to i32
  %s4  = shl i32 %b4, 4
  %m4  = or i32 %m3, %s4
  %b5  = zext i1 %c5  to i32
  %s5  = shl i32 %b5, 5
  %m5  = or i32 %m4, %s5
  %b6  = zext i1 %c6  to i32
  %s6  = shl i32 %b6, 6
  %m6  = or i32 %m5, %s6
  %b7  = zext i1 %c7  to i32
  %s7  = shl i32 %b7, 7
  %m7  = or i32 %m6, %s7
  %b8  = zext i1 %c8  to i32
  %s8  = shl i32 %b8, 8
  %m8  = or i32 %m7, %s8
  %b9  = zext i1 %c9  to i32
  %s9  = shl i32 %b9, 9
  %m9  = or i32 %m8, %s9
  %b10 = zext i1 %c10 to i32
  %s10 = shl i32 %b10, 10
  %m10 = or i32 %m9, %s10
  %b11 = zext i1 %c11 to i32
  %s11 = shl i32 %b11, 11
  %m11 = or i32 %m10, %s11
  %b12 = zext i1 %c12 to i32
  %s12 = shl i32 %b12, 12
  %m12 = or i32 %m11, %s12
  %b13 = zext i1 %c13 to i32
  %s13 = shl i32 %b13, 13
  %m13 = or i32 %m12, %s13
  %b14 = zext i1 %c14 to i32
  %s14 = shl i32 %b14, 14
  %m14 = or i32 %m13, %s14
  %b15 = zext i1 %c15 to i32
  %s15 = shl i32 %b15, 15
  %m15 = or i32 %m14, %s15
  ret i32 %m15
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  call void @__omni_print_float(double 3.14)
  %s = fadd double 0.1, 0.2
  call void @__omni_print_float(double %s)
  call void @__omni_print_float(double -273.15)
  call void @__omni_print_float(double 42.0)
  call void @__omni_print_float(double 0.0)
  call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([7 x i8], [7 x i8]* @.fmt_f, i64 0, i64 0), double 6.25)
  ret i32 0
}
"#;
        verify_cortex_m_ir(ir).expect("IEEE-754 Cortex-M IR verification failed");

        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);

            // 1) Run full firmware from `Reset_Handler` -> `main` -> UART0 float output!
            emu.call_fn(&asm, "Reset_Handler", [0; 4], 500_000);
            let uart_out = String::from_utf8_lossy(&emu.uart_tx).to_string();
            assert_eq!(uart_out, "3.14\n0.3\n-273.15\n42.0\n0.0\nv=6.25!\n");

            // 2) Run `test_f64_pipeline(-25)` -> expected `74`
            let neg25 = (-25i64) as u64;
            let got = emu.call_fn(
                &asm,
                "test_f64_pipeline",
                [neg25 as u32, (neg25 >> 32) as u32, 0, 0],
                200_000,
            ) as i64;
            assert_eq!(got, 74);

            // 3) Verify ALL 16 LLVM `fcmp` predicates across `<`, `==` (`+0.0 == -0.0`), `>`, and `NaN`!
            let call_fcmp16 = |emu: &mut ThumbCpuEmu, a: f64, b: f64| -> u16 {
                let ab = a.to_bits();
                let bb = b.to_bits();
                emu.call_fn(
                    &asm,
                    "test_all_16_fcmp_predicates",
                    [ab as u32, (ab >> 32) as u32, bb as u32, (bb >> 32) as u32],
                    200_000,
                ) as u16
            };

            // Case `a < b` (-2.5 < 1.25): ogt=0,oge=0,olt=1(4),ole=1(5),one=1(6),ord=1(7),ult=1(12),ule=1(13),une=1(14),true=1(15)
            let m_lt = call_fcmp16(&mut emu, -2.5, 1.25);
            assert_eq!(m_lt, (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 12) | (1 << 13) | (1 << 14) | (1 << 15));

            // Case `+0.0 == -0.0`: oeq=1(1),oge=1(3),ole=1(5),ord=1(7),ueq=1(9),uge=1(11),ule=1(13),true=1(15)
            let m_eq = call_fcmp16(&mut emu, 0.0, -0.0);
            assert_eq!(m_eq, (1 << 1) | (1 << 3) | (1 << 5) | (1 << 7) | (1 << 9) | (1 << 11) | (1 << 13) | (1 << 15));

            // Case `a > b` (9.5 > -100.0): ogt=1(2),oge=1(3),one=1(6),ord=1(7),ugt=1(10),uge=1(11),une=1(14),true=1(15)
            let m_gt = call_fcmp16(&mut emu, 9.5, -100.0);
            assert_eq!(m_gt, (1 << 2) | (1 << 3) | (1 << 6) | (1 << 7) | (1 << 10) | (1 << 11) | (1 << 14) | (1 << 15));

            // Case `NaN` (unordered): uno=1(8),ueq=1(9),ugt=1(10),uge=1(11),ult=1(12),ule=1(13),une=1(14),true=1(15)
            let nan = f64::from_bits(0x7FF8_0000_0000_0000);
            let m_nan = call_fcmp16(&mut emu, nan, 1.0);
            assert_eq!(m_nan, 0xFF00);

            // 4) Direct 0-ULP IEEE-754 soft-float helper checks (normals, subnormals, rounding, Inf/NaN)
            let pairs: &[(f64, f64)] = &[
                (1.5, 2.25),
                (0.1, 0.2),
                (3.141592653589793, 2.718281828459045),
                (-12345.6789, 12345.6789),
                (1.0, 3.0),
                (f64::from_bits(1), f64::from_bits(2)), // smallest subnormals!
                (2.2250738585072014e-308, 0.5),         // normal -> subnormal underflow!
                (f64::INFINITY, -1.0),
            ];
            for &(x, y) in pairs {
                let xb = x.to_bits();
                let yb = y.to_bits();
                let args = [xb as u32, (xb >> 32) as u32, yb as u32, (yb >> 32) as u32];
                assert_eq!(emu.call_fn(&asm, "__omni_fadd64", args, 50_000), (x + y).to_bits());
                assert_eq!(emu.call_fn(&asm, "__omni_fsub64", args, 50_000), (x - y).to_bits());
                assert_eq!(emu.call_fn(&asm, "__omni_fmul64", args, 50_000), (x * y).to_bits());
                assert_eq!(emu.call_fn(&asm, "__omni_fdiv64", args, 50_000), (x / y).to_bits());
            }

            // Format `inf`, `-inf`, `nan`
            let inf_ptr = emu.call_fn(&asm, "__omni_float_to_str", [0, 0x7FF0_0000, 0, 0], 50_000) as u32;
            assert_eq!(emu.read_cstr(inf_ptr), "inf");
            let ninf_ptr = emu.call_fn(&asm, "__omni_float_to_str", [0, 0xFFF0_0000, 0, 0], 50_000) as u32;
            assert_eq!(emu.read_cstr(ninf_ptr), "-inf");
            let nan_ptr = emu.call_fn(&asm, "__omni_float_to_str", [1, 0x7FF0_0000, 0, 0], 50_000) as u32;
            assert_eq!(emu.read_cstr(nan_ptr), "nan");
        }
    }

    #[test]
    fn test_pll_sys_clock_init_rp2040_125mhz_and_rp2350_150mhz() {
        let minimal_ir = r#"
define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        for (chip, expected_pll_base, expected_fbdiv, expected_prim, expected_sys_div1, expected_pll_rst_bit) in [
            (RpChipProfile::rp2040(), 0x4002_8000u32, 125u32, 0x0006_2000u32, 0x0000_0100u32, 1u32 << 12),
            (RpChipProfile::rp2350(), 0x4005_0000u32, 125u32, 0x0005_2000u32, 0x0001_0000u32, 1u32 << 14),
        ] {
            assert_eq!(chip.pll_sys_base, expected_pll_base);
            assert_eq!(chip.pll_fbdiv_int, expected_fbdiv);
            assert_eq!(chip.pll_prim_postdiv, expected_prim);
            assert!(12 * chip.pll_fbdiv_int <= 1600, "VCO must not exceed 1600 MHz silicon max");
            assert_ne!(chip.resets_unreset_mask & expected_pll_rst_bit, 0);

            let (flash, asm) = compile_llvm_ir_to_flash_image(minimal_ir, &chip).unwrap();

            // A) Hardware lock mode: verify exact register sequence on PLL_SYS, RESETS, and CLOCKS
            let mut hw_emu = ThumbCpuEmu::new(chip, flash.clone(), true);
            hw_emu.call_fn(&asm, "Reset_Handler", [0; 4], 50_000);

            assert_eq!(hw_emu.mmio_regs.get(&chip.pll_sys_base), Some(&1)); // CS: REFDIV = 1
            assert_eq!(hw_emu.mmio_regs.get(&(chip.pll_sys_base + 0x08)), Some(&expected_fbdiv)); // FBDIV_INT = 125
            assert_eq!(hw_emu.mmio_regs.get(&(chip.pll_sys_base + 0x0C)), Some(&expected_prim)); // PRIM: (6,2) on RP2040, (5,2) on RP2350
            assert_eq!(hw_emu.mmio_regs.get(&(chip.pll_sys_base + 0x04)), Some(&0x04)); // PWR: VCOPD=0, PD=0, POSTDIVPD=0

            // Verify CLK_SYS_CTRL was parked to 0 BEFORE asserting RESETS.PLL_SYS (+0x2000)
            let first_sys_park_idx = hw_emu
                .mmio_write_log
                .iter()
                .position(|&(a, v)| a == chip.clocks_base + 0x3C && v == 0)
                .expect("CLK_SYS_CTRL must be parked to 0");
            let pll_rst_idx = hw_emu
                .mmio_write_log
                .iter()
                .position(|&(a, _)| a == chip.resets_base + 0x2000)
                .expect("RESETS.PLL_SYS must be asserted");
            assert!(first_sys_park_idx < pll_rst_idx, "CLK_SYS must park on CLK_REF before resetting PLL_SYS");

            // Verify PWR transitioned `0x0C` (VCOPD+PD clear) -> `0x04` (POSTDIVPD clear)
            let pwr_writes: Vec<u32> = hw_emu
                .mmio_write_log
                .iter()
                .filter(|&&(a, _)| a == chip.pll_sys_base + 0x04)
                .map(|&(_, v)| v)
                .collect();
            assert_eq!(pwr_writes, vec![0x0C, 0x04]);

            // Verify CLOCKS: CLK_REF_CTRL = 2 (XOSC), CLK_SYS_DIV = 1.0, CLK_SYS_CTRL -> 1 (AUX=PLL_SYS), CLK_PERI_CTRL = 0x880
            assert_eq!(hw_emu.mmio_regs.get(&(chip.clocks_base + 0x30)), Some(&2));
            assert_eq!(hw_emu.mmio_regs.get(&(chip.clocks_base + 0x40)), Some(&expected_sys_div1));
            assert_eq!(hw_emu.mmio_regs.get(&(chip.clocks_base + 0x3C)), Some(&1));
            assert_eq!(hw_emu.mmio_regs.get(&(chip.clocks_base + 0x48)), Some(&0x0880));

            // Verify USBCTRL is ONLY unreset AFTER 48 MHz CLK_USB is enabled (required on cold POR!)
            let clk_usb_en_idx = hw_emu
                .mmio_write_log
                .iter()
                .position(|&(a, v)| a == chip.clocks_base + chip.clk_usb_ctrl_off && v == 0x0800)
                .expect("CLK_USB_CTRL must be enabled (0x0800)");
            let usb_unrst_idx = hw_emu
                .mmio_write_log
                .iter()
                .position(|&(a, v)| {
                    a == chip.resets_base + 0x3000 && (v & chip.resets_usbctrl_mask) != 0
                })
                .expect("USBCTRL must be unreset via RESETS+0x3000");
            assert!(
                clk_usb_en_idx < usb_unrst_idx,
                "CLK_USB (48 MHz) must be enabled BEFORE unresetting USBCTRL for cold power-on boot"
            );

            // B) QEMU / unmodeled-MMIO mode (`CS.LOCK == 0` forever): bounded timeout must NEVER hang
            //    AND must keep `CLK_SYS_CTRL` safely on `CLK_REF` (`0`) rather than switching to an unlocked PLL!
            let mut qemu_emu = ThumbCpuEmu::new(chip, flash, false);
            qemu_emu.call_fn(&asm, "Reset_Handler", [0; 4], 5_000_000);
            assert_eq!(qemu_emu.read32(chip.sram_base + SRAM_G_HEAP_PTR_OFF), chip.heap_start);
            assert_eq!(qemu_emu.mmio_regs.get(&(chip.clocks_base + 0x3C)), Some(&0));
        }
    }

    #[test]
    fn test_sram_malloc_safe_oom_handling() {
        let minimal_ir = r#"
define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(minimal_ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);
            emu.init_heap_only();

            // Allocate 64 bytes and write live sentinel data
            let p1 = emu.call_fn(&asm, "malloc", [64, 0, 0, 0], 10_000) as u32;
            assert_eq!(p1, chip.heap_start);
            emu.write32(p1, 0xCAFE_BABE);
            emu.write32(p1 + 60, 0xDEAD_BEEF);

            let heap_after_p1 = emu.read32(chip.sram_base + SRAM_G_HEAP_PTR_OFF);
            assert_eq!(heap_after_p1, chip.heap_start + 64);

            // Request more bytes than remaining SRAM (`heap_limit - heap_after_p1 + 16`)
            let too_big = (chip.heap_limit - heap_after_p1) + 16;
            let oom1 = emu.call_fn(&asm, "malloc", [too_big, 0, 0, 0], 10_000);
            assert_eq!(oom1, 0, "OOM allocation must return NULL (0)");
            assert_eq!(
                emu.read32(chip.sram_base + SRAM_G_HEAP_PTR_OFF),
                heap_after_p1,
                "heap_ptr must NOT advance or wrap on OOM"
            );

            // Request near-u32::MAX wraparound size (`0xFFFF_FFFC`)
            let oom2 = emu.call_fn(&asm, "malloc", [0xFFFF_FFFC, 0, 0, 0], 10_000);
            assert_eq!(oom2, 0);
            assert_eq!(emu.read32(chip.sram_base + SRAM_G_HEAP_PTR_OFF), heap_after_p1);

            // Live data at `p1` must be 100% intact!
            assert_eq!(emu.read32(p1), 0xCAFE_BABE);
            assert_eq!(emu.read32(p1 + 60), 0xDEAD_BEEF);

            // Subsequent small allocation that fits still succeeds right at `heap_after_p1`
            let p2 = emu.call_fn(&asm, "malloc", [32, 0, 0, 0], 10_000) as u32;
            assert_eq!(p2, heap_after_p1);
        }
    }

    #[test]
    fn test_omni_slice_0xff_magic_closures_rc_and_new_runtime_symbols() {
        let ir = r#"
@.hello = private unnamed_addr constant [6 x i8] c"HELLO\00", align 1

declare i8* @__omni_slice_create(i64)
declare void @__omni_slice_append(i8*, i64)
declare i64 @__omni_slice_get(i8*, i64)
declare i64 @__omni_slice_len(i8*)
declare i64 @__omni_str_len(i8*)
declare i8* @__omni_rc_retain(i8*)
declare void @__omni_rc_release(i8*)
declare i8* @__omni_closure_create(i8*, i8*)
declare i32 @__omni_closure_is(i8*)
declare i8* @__omni_closure_fn(i8*)
declare i8* @__omni_closure_env(i8*)
declare void @__omni_closure_destroy(i8*)
declare void @__omni_c_return_ptr(i8*)
declare i8* @__omni_c_consume_returned_ptr()
declare void @__omni_fiber_join_all()
declare void @__omni_map_delete_int(i8*, i64)

define i64 @adder_fn(i8* %env, i64 %x) {
entry:
  %ep = bitcast i8* %env to i64*
  %base = load i64, i64* %ep
  %sum = add i64 %base, %x
  ret i64 %sum
}

define i64 @exercise_runtime() {
entry:
  ; 1. Create slice, append 3 items, verify polymorphic slice_len & str_len on both OmniSlice* and char*!
  %sl = call i8* @__omni_slice_create(i64 4)
  call void @__omni_slice_append(i8* %sl, i64 100)
  call void @__omni_slice_append(i8* %sl, i64 200)
  call void @__omni_slice_append(i8* %sl, i64 300)

  %sl_len1 = call i64 @__omni_slice_len(i8* %sl)
  %sl_len2 = call i64 @__omni_str_len(i8* %sl)
  %cstr = getelementptr inbounds [6 x i8], [6 x i8]* @.hello, i64 0, i64 0
  %cs_len1 = call i64 @__omni_str_len(i8* %cstr)
  %cs_len2 = call i64 @__omni_slice_len(i8* %cstr)

  %elem1 = call i64 @__omni_slice_get(i8* %sl, i64 1)

  ; 2. Retain / release on OmniSlice* and Flash char* (must not fault on Flash .rodata!)
  %sl_ret = call i8* @__omni_rc_retain(i8* %sl)
  %cs_ret = call i8* @__omni_rc_retain(i8* %cstr)
  call void @__omni_rc_release(i8* %cstr)
  call void @__omni_rc_release(i8* %sl_ret)

  ; 3. First-class closure create, closure_is, closure_fn, closure_env, indirect call, closure_destroy
  %env_box = alloca i64, align 8
  store i64 77, i64* %env_box
  %env_i8 = bitcast i64* %env_box to i8*
  %fn_i8 = bitcast i64 (i8*, i64)* @adder_fn to i8*
  %cl = call i8* @__omni_closure_create(i8* %fn_i8, i8* %env_i8)
  %is_cl = call i32 @__omni_closure_is(i8* %cl)
  %is_raw = call i32 @__omni_closure_is(i8* %fn_i8)
  %extr_fn_i8 = call i8* @__omni_closure_fn(i8* %cl)
  %extr_env = call i8* @__omni_closure_env(i8* %cl)
  %raw_pass_fn = call i8* @__omni_closure_fn(i8* %fn_i8)
  %raw_env = call i8* @__omni_closure_env(i8* %fn_i8)
  %callee = bitcast i8* %extr_fn_i8 to i64 (i8*, i64)*
  %cl_res = call i64 %callee(i8* %extr_env, i64 23)
  call void @__omni_closure_destroy(i8* %cl)

  ; 4. __omni_c_consume_returned_ptr + __omni_fiber_join_all + __omni_map_delete_int
  call void @__omni_c_return_ptr(i8* %sl)
  %c1 = call i8* @__omni_c_consume_returned_ptr()
  %c2 = call i8* @__omni_c_consume_returned_ptr()
  call void @__omni_fiber_join_all()
  call void @__omni_map_delete_int(i8* null, i64 0)

  ; Check invariants: sl_len1==3, sl_len2==3, cs_len1==5, cs_len2==5, elem1==200,
  ; is_cl==1, is_raw==0, raw_env==null, cl_res==100, c1==%sl, c2==null
  %t0 = add i64 %sl_len1, %sl_len2       ; 6
  %t1 = add i64 %cs_len1, %cs_len2       ; 10
  %t2 = add i64 %t0, %t1                 ; 16
  %t3 = add i64 %t2, %elem1              ; 216
  %t4 = add i64 %t3, %cl_res             ; 316
  %ic64 = zext i32 %is_cl to i64         ; 1
  %ir64 = zext i32 %is_raw to i64        ; 0
  %c2_i = ptrtoint i8* %c2 to i64        ; 0
  %re_i = ptrtoint i8* %raw_env to i64   ; 0
  %t5 = add i64 %t4, %ic64               ; 317
  %t6 = add i64 %t5, %ir64
  %t7 = add i64 %t6, %c2_i
  %t8 = add i64 %t7, %re_i
  ret i64 %t8
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        verify_cortex_m_ir(ir).expect("new runtime symbols IR validation failed");

        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);
            emu.init_heap_only();

            // Direct header layout verification on `__omni_slice_create` & `__omni_closure_create`
            let sl_ptr = emu.call_fn(&asm, "__omni_slice_create", [8, 0, 0, 0], 20_000) as u32;
            assert_eq!(emu.read8(sl_ptr), 0xFF, "OmniSlice byte 0 must be 0xFF");
            assert_eq!(emu.read32(sl_ptr), OMNI_SLICE_MAGIC32);
            assert_eq!(emu.read32(sl_ptr + 4), 1); // rc == 1
            assert_ne!(emu.read32(sl_ptr + 8), 0); // data_ptr
            assert_eq!(emu.read32(sl_ptr + 12), 0); // len == 0
            assert_eq!(emu.read32(sl_ptr + 16), 8); // cap == 8

            let retained = emu.call_fn(&asm, "__omni_rc_retain", [sl_ptr, 0, 0, 0], 10_000) as u32;
            assert_eq!(retained, sl_ptr);
            assert_eq!(emu.read32(sl_ptr + 4), 2);
            emu.call_fn(&asm, "__omni_rc_release", [sl_ptr, 0, 0, 0], 10_000);
            assert_eq!(emu.read32(sl_ptr + 4), 1);

            let cl_ptr = emu.call_fn(&asm, "__omni_closure_create", [0x1000_0501, 0x2000_5000, 0, 0], 20_000) as u32;
            assert_eq!(emu.read8(cl_ptr), 0xFF);
            assert_eq!(emu.read32(cl_ptr), OMNI_CLOSURE_MAGIC32);
            assert_eq!(emu.read32(cl_ptr + 4), 1);
            assert_eq!(emu.read32(cl_ptr + 8), 0x1000_0501);
            assert_eq!(emu.read32(cl_ptr + 12), 0x2000_5000);

            // Polymorphic `__omni_slice_get` on C `char*` ("HELLO"[1] == "E")
            let hello_off = *asm.symbols.get(".hello").unwrap() as u32;
            let hello_vma = chip.flash_base + hello_off;
            let ch_ptr = emu.call_fn(&asm, "__omni_slice_get", [hello_vma, 0, 1, 0], 20_000) as u32;
            assert_eq!(emu.read_cstr(ch_ptr), "E");

            // Full integrated end-to-end execution of `@exercise_runtime`
            let total = emu.call_fn(&asm, "exercise_runtime", [0; 4], 200_000) as i64;
            assert_eq!(total, 317);
        }
    }

    #[test]
    fn test_int_pow_not_hijacked_fpow64_ieee754_sqrt_and_sci_float_str() {
        let ir = r#"
@.sci1 = private unnamed_addr constant [14 x i8] c"  \09\0A -1.25e+3\00", align 1
@.sci2 = private unnamed_addr constant [9 x i8] c"  3.5E-2\00", align 1

declare i64 @__omni_ipow64(i64, i64)
declare double @llvm.pow.f64(double, double)
declare i8* @__omni_float_to_str(double)
declare double @__omni_c_str_to_f64(i8*)
declare double @atof(i8*)

define weak i64 @pow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {
entry:
  %raw = call i64 @__omni_ipow64(i64 %a0, i64 %a1)
  %has_mod = icmp sgt i64 %a2, 0
  br i1 %has_mod, label %do_mod, label %done
do_mod:
  %m = srem i64 %raw, %a2
  br label %done
done:
  %res = phi i64 [ %m, %do_mod ], [ %raw, %entry ]
  ret i64 %res
}

define i64 @run_pow_and_atof_checks() {
entry:
  ; 1) Integer `@pow` (3^5 % 100 = 243 % 100 = 43) MUST NOT be hijacked by `__omni_fpow64`!
  %ip = call i64 @pow(i64 3, i64 5, i64 100, i64 0, i64 0, i64 0)
  ; 2) `9.0 ** 0.5 == 3.0`, `4.0 ** 1.5 == 8.0`
  %rt9 = call double @llvm.pow.f64(double 9.0, double 0.5)
  %p4  = call double @llvm.pow.f64(double 4.0, double 1.5)
  ; 3) Scientific `atof` with leading whitespace (`-1250.0` + `0.035 * 1000 = 35.0`)
  %v1 = call double @__omni_c_str_to_f64(i8* getelementptr inbounds ([14 x i8], [14 x i8]* @.sci1, i64 0, i64 0))
  %v2 = call double @atof(i8* getelementptr inbounds ([9 x i8], [9 x i8]* @.sci2, i64 0, i64 0))
  %v2s = fmul double %v2, 1000.0
  %sumf = fadd double %rt9, %p4    ; 11.0
  %sumf2 = fadd double %sumf, %v2s ; 46.0
  %sumf3 = fsub double %sumf2, %v1 ; 46.0 - (-1250.0) = 1296.0
  %fi = fptosi double %sumf3 to i64
  %tot = add i64 %ip, %fi          ; 43 + 1296 = 1339
  ret i64 %tot
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        verify_cortex_m_ir(ir).expect("pow/sqrt/sci IR validation failed");

        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);
            emu.init_heap_only();

            let res = emu.call_fn(&asm, "run_pow_and_atof_checks", [0; 4], 400_000) as i64;
            assert_eq!(res, 1339);

            // Direct IEEE-754 `__omni_fpow64` edge-case & fractional checks
            let mut call_fpow = |b: f64, e: f64| -> u64 {
                let bb = b.to_bits();
                let eb = e.to_bits();
                emu.call_fn(
                    &asm,
                    "__omni_fpow64",
                    [bb as u32, (bb >> 32) as u32, eb as u32, (eb >> 32) as u32],
                    200_000,
                )
            };
            let nan = f64::from_bits(0x7FF8_0000_0000_0000);
            assert_eq!(call_fpow(9.0, 0.5), 3.0f64.to_bits());
            assert_eq!(call_fpow(4.0, -0.5), 0.5f64.to_bits());
            assert_eq!(call_fpow(4.0, -1.5), 0.125f64.to_bits());
            assert_eq!(call_fpow(-2.0, -3.0), (-0.125f64).to_bits());
            assert_eq!(call_fpow(nan, 0.0), 1.0f64.to_bits());
            assert_eq!(call_fpow(f64::INFINITY, -0.0), 1.0f64.to_bits());
            assert_eq!(call_fpow(1.0, nan), 1.0f64.to_bits());
            assert_eq!(call_fpow(1.0, f64::INFINITY), 1.0f64.to_bits());
            assert_eq!(call_fpow(-1.0, f64::INFINITY), 1.0f64.to_bits());
            assert_eq!(call_fpow(-1.0, f64::NEG_INFINITY), 1.0f64.to_bits());
            assert_eq!(call_fpow(2.0, f64::INFINITY), f64::INFINITY.to_bits());
            assert_eq!(call_fpow(0.5, f64::INFINITY), 0.0f64.to_bits());
            assert_eq!(call_fpow(2.0, f64::NEG_INFINITY), 0.0f64.to_bits());
            assert_eq!(call_fpow(0.5, f64::NEG_INFINITY), f64::INFINITY.to_bits());
            assert_eq!(call_fpow(-9.0, 0.5), 0x7FF8_0000_0000_0000);
            assert_eq!(call_fpow(nan, 2.0), 0x7FF8_0000_0000_0000);
            assert_eq!(call_fpow(2.0, nan), 0x7FF8_0000_0000_0000);

            // Large/small magnitude `__omni_float_to_str` (`1e20` must format `"1.0e+20"`, NEVER `"-1.0"`!)
            let mut fmt_f64 = |v: f64| -> String {
                let vb = v.to_bits();
                let ptr = emu.call_fn(
                    &asm,
                    "__omni_float_to_str",
                    [vb as u32, (vb >> 32) as u32, 0, 0],
                    200_000,
                ) as u32;
                emu.read_cstr(ptr)
            };
            assert_eq!(fmt_f64(1e20), "1.0e+20");
            assert_eq!(fmt_f64(-2.5e19), "-2.5e+19");
            assert_eq!(fmt_f64(1.25e-5), "1.25e-5");
        }
    }

    #[test]
    fn test_cortex_m_omni_map_0xff_int_keys_and_heap_obj_header_rc() {
        let ir = r#"
%struct.Sensor = type { i64, i64 }

@.k_str42 = private unnamed_addr constant [3 x i8] c"42\00", align 1

declare i8* @__omni_map_create(i64)
declare void @__omni_map_set(i8*, i8*, i64)
declare i64 @__omni_map_get(i8*, i8*)
declare i1 @__omni_map_contains(i8*, i8*)
declare void @__omni_map_delete(i8*, i8*)
declare void @__omni_map_set_int(i8*, i64, i64)
declare i64 @__omni_map_get_int(i8*, i64)
declare i1 @__omni_map_contains_int(i8*, i64)
declare void @__omni_map_delete_int(i8*, i64)
declare i64 @__omni_map_len(i8*)
declare i64 @__omni_str_len(i8*)
declare void @__omni_map_destroy(i8*)

declare i8* @__omni_obj_alloc(i64, i64)
declare void @__omni_obj_set_type(i8*, i64)
declare i64 @__omni_obj_get_type(i8*)
declare i8* @__omni_rc_retain(i8*)
declare void @__omni_rc_release(i8*)

define i64 @exercise_map_and_obj() {
entry:
  ; 1) OmniMap `_int` vs string `"42"` domain separation (`m[42] != m["42"]`)
  %m = call i8* @__omni_map_create(i64 8)
  %k42 = getelementptr inbounds [3 x i8], [3 x i8]* @.k_str42, i64 0, i64 0
  call void @__omni_map_set_int(i8* %m, i64 42, i64 1000)
  call void @__omni_map_set(i8* %m, i8* %k42, i64 2000)
  call void @__omni_map_set_int(i8* %m, i64 -5000000000, i64 300)

  %v_int = call i64 @__omni_map_get_int(i8* %m, i64 42)        ; 1000
  %v_str = call i64 @__omni_map_get(i8* %m, i8* %k42)          ; 2000
  %v_neg = call i64 @__omni_map_get_int(i8* %m, i64 -5000000000) ; 300
  %has42 = call i1 @__omni_map_contains_int(i8* %m, i64 42)    ; 1
  %len3  = call i64 @__omni_str_len(i8* %m)                    ; 3 (polymorphic len!)

  call void @__omni_map_delete_int(i8* %m, i64 42)
  %has42_after = call i1 @__omni_map_contains_int(i8* %m, i64 42) ; 0
  %v_str_after = call i64 @__omni_map_get(i8* %m, i8* %k42)       ; still 2000!
  %len2 = call i64 @__omni_map_len(i8* %m)                        ; 2

  %m_ret = call i8* @__omni_rc_retain(i8* %m)
  call void @__omni_map_destroy(i8* %m_ret) ; rc 2 -> 1 (still live)
  %len2_still = call i64 @__omni_map_len(i8* %m)                  ; 2
  call void @__omni_map_destroy(i8* %m)     ; rc 1 -> 0 (invalidated)
  %len_dead = call i64 @__omni_map_len(i8* %m)                    ; 0

  ; 2) Heap Object Header (`__omni_obj_alloc`, `__omni_obj_get_type`, `__omni_obj_set_type`, RC)
  %raw = call i8* @__omni_obj_alloc(i64 16, i64 7)
  %sp = bitcast i8* %raw to %struct.Sensor*
  ; Store `-1` (`0xFFFFFFFFFFFFFFFF`, byte 0 == `0xFF`!) at field 0 to prove RC never confuses payload with 0xFF container!
  %f0 = getelementptr inbounds %struct.Sensor, %struct.Sensor* %sp, i32 0, i32 0
  %f1 = getelementptr inbounds %struct.Sensor, %struct.Sensor* %sp, i32 0, i32 1
  store i64 -1, i64* %f0
  store i64 55, i64* %f1

  %t_init = call i64 @__omni_obj_get_type(i8* %raw)            ; 7
  call void @__omni_obj_set_type(i8* %raw, i64 19)
  %t_upd = call i64 @__omni_obj_get_type(i8* %raw)             ; 19

  %r_keep = call i8* @__omni_rc_retain(i8* %raw)               ; rc 1 -> 2
  %f0_val = load i64, i64* %f0                                 ; must still be -1!
  %f1_val = load i64, i64* %f1                                 ; 55
  call void @__omni_rc_release(i8* %r_keep)                    ; rc 2 -> 1
  %t_live = call i64 @__omni_obj_get_type(i8* %raw)            ; 19
  call void @__omni_rc_release(i8* %raw)                       ; rc 1 -> 0 (clears canary!)
  %t_freed = call i64 @__omni_obj_get_type(i8* %raw)           ; 0

  ; Sum: 1000 + 2000 + 300 + 1 + 3 + 0 + 2000 + 2 + 2 + 0 + 7 + 19 + (-1) + 55 + 19 + 0 = 5407
  %h1 = zext i1 %has42 to i64
  %h2 = zext i1 %has42_after to i64
  %a0 = add i64 %v_int, %v_str
  %a1 = add i64 %a0, %v_neg
  %a2 = add i64 %a1, %h1
  %a3 = add i64 %a2, %len3
  %a4 = add i64 %a3, %h2
  %a5 = add i64 %a4, %v_str_after
  %a6 = add i64 %a5, %len2
  %a7 = add i64 %a6, %len2_still
  %a8 = add i64 %a7, %len_dead
  %a9 = add i64 %a8, %t_init
  %a10 = add i64 %a9, %t_upd
  %a11 = add i64 %a10, %f0_val
  %a12 = add i64 %a11, %f1_val
  %a13 = add i64 %a12, %t_live
  %a14 = add i64 %a13, %t_freed
  ret i64 %a14
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        verify_cortex_m_ir(ir).expect("OmniMap + HeapObj IR validation failed");

        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);
            emu.init_heap_only();

            // Direct structural header checks for `OmniMap` & `__omni_obj_alloc`
            let mp = emu.call_fn(&asm, "__omni_map_create", [16, 0, 0, 0], 20_000) as u32;
            assert_eq!(emu.read8(mp), 0xFF);
            assert_eq!(emu.read32(mp), OMNI_MAP_MAGIC32);
            assert_eq!(emu.read32(mp + 4), 1);  // rc
            assert_ne!(emu.read32(mp + 8), 0);  // entries_ptr
            assert_eq!(emu.read32(mp + 12), 16); // cap
            assert_eq!(emu.read32(mp + 16), 0); // len

            let obj = emu.call_fn(&asm, "__omni_obj_alloc", [24, 0, 77, 0], 20_000) as u32;
            assert_eq!(emu.read32(obj - 16), OMNI_OBJ_MAGIC32);
            assert_eq!(emu.read32(obj - 12), 1);
            assert_eq!(emu.read32(obj - 8), 77);
            assert_eq!(emu.read32(obj - 4), OMNI_OBJ_CANARY32);

            let got = emu.call_fn(&asm, "exercise_map_and_obj", [0; 4], 400_000) as i64;
            assert_eq!(got, 5407);
        }
    }

    // =========================================================================
    // Wire-Accurate CYW43439 3-Wire gSPI + Backplane + Firmware/LED Slave Model
    // =========================================================================
    struct SimCyw43439 {
        powered: bool,
        gspi_strapped: bool,
        bus_swapped: bool,
        bus_ctrl: u32,
        f1_resp_delay: usize,
        spi_int: u16,
        spi_int_en: u16,
        backplane_window: u32,
        clock_csr: u8,
        sleep_csr: u8,
        pull_up: u8,
        f2_watermark: u8,
        socram_ioctrl: u8,
        socram_resetctrl: u8,
        arm_ioctrl: u8,
        arm_resetctrl: u8,
        sram_bankx_idx: u32,
        sram_bankx_pda: u32,
        socsram: Vec<u8>,
        cc_gpiocontrol: u32,
        cc_gpioouten: u32,
        cc_gpioout: u32,
        cc_gpioout_history: Vec<u32>,
        f2_gpio_history: Vec<(u32, u32)>,
        f2_rx_packets: std::collections::VecDeque<Vec<u8>>,
        tx_eth_frames: Vec<Vec<u8>>,
        clm_received: Vec<u8>,
        joined_ssid: String,
        joined_psk: String,
        rx_bits: Vec<u8>,
        tx_bits: Vec<u8>,
        tx_pos: usize,
        cur_miso_bit: u8,
        cmd_parsed: bool,
        cur_cmd_wr: bool,
        cur_cmd_fn: u32,
        cur_cmd_addr: u32,
        cur_cmd_sz: usize,
    }

    impl SimCyw43439 {
        fn new() -> Self {
            Self {
                powered: false,
                gspi_strapped: false,
                bus_swapped: true,
                bus_ctrl: 0,
                f1_resp_delay: 0,
                spi_int: 0x0001, // DATA_UNAVAILABLE at POR
                spi_int_en: 0,
                backplane_window: 0x1800_0000,
                clock_csr: 0,
                sleep_csr: 0,
                pull_up: 0x0F,
                f2_watermark: 0,
                socram_ioctrl: 0,
                socram_resetctrl: 1,
                arm_ioctrl: 0,
                arm_resetctrl: 0, // force host to exercise put-in-reset path too!
                sram_bankx_idx: 0,
                sram_bankx_pda: 0xFFFF_FFFF,
                socsram: vec![0u8; 512 * 1024],
                cc_gpiocontrol: 0xFFFF_FFFF,
                cc_gpioouten: 0,
                cc_gpioout: 0,
                cc_gpioout_history: Vec::new(),
                f2_gpio_history: Vec::new(),
                f2_rx_packets: std::collections::VecDeque::new(),
                tx_eth_frames: Vec::new(),
                clm_received: Vec::new(),
                joined_ssid: String::new(),
                joined_psk: String::new(),
                rx_bits: Vec::new(),
                tx_bits: Vec::new(),
                tx_pos: 0,
                cur_miso_bit: 0,
                cmd_parsed: false,
                cur_cmd_wr: false,
                cur_cmd_fn: 0,
                cur_cmd_addr: 0,
                cur_cmd_sz: 0,
            }
        }

        fn swap16x2(x: u32) -> u32 {
            ((x >> 8) & 0x00FF_00FF) | ((x & 0x00FF_00FF) << 8)
        }

        fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(bits.len() / 8);
            for chunk in bits.chunks_exact(8) {
                let mut b = 0u8;
                for &bit in chunk {
                    b = (b << 1) | (bit & 1);
                }
                out.push(b);
            }
            out
        }

        fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(bytes.len() * 8);
            for &b in bytes {
                for k in (0..8).rev() {
                    out.push((b >> k) & 1);
                }
            }
            out
        }

        fn gpio_in_word(&self, sio_out: u32, sio_oe: u32) -> u32 {
            let mut w = sio_out & sio_oe;
            if (sio_oe & (1 << 24)) == 0 {
                if self.powered && self.gspi_strapped && self.cur_miso_bit != 0 {
                    w |= 1 << 24;
                } else {
                    w &= !(1 << 24);
                }
            }
            w
        }

        fn build_read_payload(&mut self, fn_num: u32, addr: u32, sz: usize) -> Vec<u8> {
            let aligned = (sz + 3) & !3;
            let mut resp = Vec::new();
            if fn_num == 1 && !self.bus_swapped {
                resp.resize(self.f1_resp_delay, 0u8);
            }
            match fn_num {
                0 => {
                    let val: u32 = match addr {
                        0x0000 => self.bus_ctrl,
                        0x0004 => self.spi_int as u32,
                        0x0006 => self.spi_int_en as u32,
                        0x0008 => {
                            let mut st = 0u32;
                            if self.arm_resetctrl == 0 && (self.arm_ioctrl & 1) != 0 {
                                st |= 0x0000_0020; // STATUS_F2_RX_READY
                            }
                            if let Some(front) = self.f2_rx_packets.front() {
                                let plen = (front.len() as u32) & 0x7FF;
                                st |= 0x0000_0100 | (plen << 9); // STATUS_F2_PKT_AVAILABLE
                            }
                            st
                        }
                        0x0014 => 0xFEED_BEAD,
                        _ => 0,
                    };
                    let word = if self.bus_swapped {
                        Self::swap16x2(val)
                    } else {
                        val
                    };
                    resp.extend_from_slice(&word.to_le_bytes());
                }
                1 => {
                    if addr >= 0x10000 {
                        let b: u8 = match addr {
                            0x10008 => self.f2_watermark,
                            0x1000A => ((self.backplane_window >> 8) & 0xFF) as u8,
                            0x1000B => ((self.backplane_window >> 16) & 0xFF) as u8,
                            0x1000C => ((self.backplane_window >> 24) & 0xFF) as u8,
                            0x1000E => {
                                let mut c = self.clock_csr;
                                if (c & 0x08) != 0 {
                                    c |= 0x40; // SBSDIO_ALP_AVAIL
                                }
                                if self.arm_resetctrl == 0 && (self.arm_ioctrl & 1) != 0 {
                                    c |= 0x80; // SBSDIO_HT_AVAIL
                                }
                                c
                            }
                            0x1000F => self.pull_up,
                            0x1001F => self.sleep_csr,
                            _ => 0,
                        };
                        resp.extend_from_slice(&(b as u32).to_le_bytes());
                    } else {
                        let full = self.backplane_window | (addr & 0x7FFF);
                        let val: u32 = match full {
                            0x1800_0000 => 0xA9AF, // CYW43439 chip ID (43439 = 0xA9AF)
                            0x1800_0064 => self.cc_gpioout,
                            0x1800_0068 => self.cc_gpioouten,
                            0x1800_006C => self.cc_gpiocontrol,
                            0x1810_3408 => self.arm_ioctrl as u32,
                            0x1810_3800 => self.arm_resetctrl as u32,
                            0x1810_4408 => self.socram_ioctrl as u32,
                            0x1810_4800 => self.socram_resetctrl as u32,
                            _ => 0,
                        };
                        resp.extend_from_slice(&val.to_le_bytes());
                    }
                }
                2 => {
                    let mut drain = vec![0u8; aligned.max(4)];
                    if let Some(pkt) = self.f2_rx_packets.pop_front() {
                        let n = drain.len().min(pkt.len());
                        drain[..n].copy_from_slice(&pkt[..n]);
                    }
                    resp.extend_from_slice(&drain);
                }
                _ => {
                    resp.resize(resp.len() + aligned.max(4), 0);
                }
            }
            resp
        }

        fn commit_write(&mut self) {
            if self.rx_bits.len() < 32 {
                return;
            }
            let payload_bytes = Self::bits_to_bytes(&self.rx_bits[32..]);
            if payload_bytes.is_empty() {
                return;
            }
            match self.cur_cmd_fn {
                0 => {
                    if payload_bytes.len() >= 4 {
                        let raw = u32::from_le_bytes(payload_bytes[0..4].try_into().unwrap());
                        let val = if self.bus_swapped {
                            Self::swap16x2(raw)
                        } else {
                            raw
                        };
                        match self.cur_cmd_addr {
                            0x0000 => {
                                self.bus_ctrl = val;
                                if (val & 0x03) == 0x03 {
                                    self.bus_swapped = false;
                                }
                            }
                            0x0004 => {
                                self.spi_int &= !(val as u16);
                            }
                            0x0006 => {
                                self.spi_int_en = val as u16;
                            }
                            0x001D => {
                                self.f1_resp_delay = (val & 0xFF) as usize;
                            }
                            _ => {}
                        }
                    }
                }
                1 => {
                    if self.cur_cmd_addr >= 0x10000 {
                        let b = payload_bytes[0];
                        match self.cur_cmd_addr {
                            0x10008 => self.f2_watermark = b,
                            0x1000A => {
                                self.backplane_window =
                                    (self.backplane_window & !0x0000_FF00) | ((b as u32) << 8);
                            }
                            0x1000B => {
                                self.backplane_window =
                                    (self.backplane_window & !0x00FF_0000) | ((b as u32) << 16);
                            }
                            0x1000C => {
                                self.backplane_window =
                                    (self.backplane_window & !0xFF00_0000) | ((b as u32) << 24);
                            }
                            0x1000E => self.clock_csr = b,
                            0x1000F => self.pull_up = b,
                            0x1001F => self.sleep_csr = b,
                            _ => {}
                        }
                    } else {
                        let full = self.backplane_window | (self.cur_cmd_addr & 0x7FFF);
                        if full < (self.socsram.len() as u32) {
                            let n = self.cur_cmd_sz.min(payload_bytes.len());
                            let start = full as usize;
                            self.socsram[start..start + n].copy_from_slice(&payload_bytes[..n]);
                        } else if payload_bytes.len() >= 4 {
                            let v32 = u32::from_le_bytes(payload_bytes[0..4].try_into().unwrap());
                            let v8 = payload_bytes[0];
                            match full {
                                0x1800_0064 => {
                                    self.cc_gpioout = v32;
                                    self.cc_gpioout_history.push(v32);
                                }
                                0x1800_0068 => self.cc_gpioouten = v32,
                                0x1800_006C => self.cc_gpiocontrol = v32,
                                0x1800_4010 => self.sram_bankx_idx = v32,
                                0x1800_4044 => self.sram_bankx_pda = v32,
                                0x1810_3408 => self.arm_ioctrl = v8,
                                0x1810_3800 => self.arm_resetctrl = v8,
                                0x1810_4408 => self.socram_ioctrl = v8,
                                0x1810_4800 => self.socram_resetctrl = v8,
                                _ => {}
                            }
                        }
                    }
                }
                2 => {
                    if payload_bytes.len() >= 12 {
                        let s_len = u16::from_le_bytes([payload_bytes[0], payload_bytes[1]]) as usize;
                        let s_not = u16::from_le_bytes([payload_bytes[2], payload_bytes[3]]);
                        assert_eq!((s_len as u16) ^ s_not, 0xFFFF);
                        let chan = payload_bytes[5] & 0x0F;
                        let hdrlen = payload_bytes[7] as usize;
                        if chan == 0 && payload_bytes.len() >= hdrlen + 16 {
                            let cmd = u32::from_le_bytes(
                                payload_bytes[hdrlen..hdrlen + 4].try_into().unwrap(),
                            );
                            let outlen = u16::from_le_bytes([
                                payload_bytes[hdrlen + 4],
                                payload_bytes[hdrlen + 5],
                            ]) as usize;
                            let flags = u32::from_le_bytes(
                                payload_bytes[hdrlen + 8..hdrlen + 12].try_into().unwrap(),
                            );
                            let req_id_hi = flags & 0xFFFF_0000;
                            let io_pay = &payload_bytes[hdrlen + 16..s_len.min(payload_bytes.len())];
                            let mut resp_pay = vec![0u8; outlen];

                            if cmd == 263 && io_pay.starts_with(b"gpioout\0") && io_pay.len() >= 16 {
                                let mask = u32::from_le_bytes(io_pay[8..12].try_into().unwrap());
                                let val = u32::from_le_bytes(io_pay[12..16].try_into().unwrap());
                                self.f2_gpio_history.push((mask, val));
                            } else if cmd == 263 && io_pay.starts_with(b"clmload\0") && io_pay.len() >= 20 {
                                let clen = u32::from_le_bytes(io_pay[12..16].try_into().unwrap()) as usize;
                                self.clm_received.extend_from_slice(&io_pay[20..20 + clen]);
                            } else if cmd == 262 && io_pay.starts_with(b"cur_etheraddr\0") {
                                resp_pay[0..6].copy_from_slice(&[0x28, 0xCD, 0xC1, 0x11, 0x22, 0x33]);
                            } else if cmd == 268 && io_pay.len() >= 68 {
                                let klen = u16::from_le_bytes([io_pay[0], io_pay[1]]) as usize;
                                self.joined_psk = String::from_utf8_lossy(&io_pay[4..4 + klen]).to_string();
                            } else if cmd == 26 && io_pay.len() >= 36 {
                                let slen = u32::from_le_bytes(io_pay[0..4].try_into().unwrap()) as usize;
                                self.joined_ssid = String::from_utf8_lossy(&io_pay[4..4 + slen]).to_string();
                            }

                            // Queue SDPCM CONTROL response with matching req_id!
                            let ack_len = 28 + resp_pay.len();
                            let mut ack = vec![0u8; ack_len];
                            ack[0..2].copy_from_slice(&(ack_len as u16).to_le_bytes());
                            ack[2..4].copy_from_slice(&(!(ack_len as u16)).to_le_bytes());
                            ack[5] = 0; // CONTROL
                            ack[7] = 12;
                            ack[12..16].copy_from_slice(&cmd.to_le_bytes());
                            ack[16..18].copy_from_slice(&(resp_pay.len() as u16).to_le_bytes());
                            ack[20..24].copy_from_slice(&req_id_hi.to_le_bytes());
                            ack[28..].copy_from_slice(&resp_pay);
                            self.f2_rx_packets.push_back(ack);

                            // If WLC_SET_SSID (26) just ran, queue WLC_E_LINK (16) + WLC_E_PSK_SUP (46, status=6)!
                            if cmd == 26 {
                                self.push_async_event(16, 1, 0); // EV_LINK (flags=1 LINK_UP)
                                self.push_async_event(46, 0, 6); // EV_PSK_SUP (status=6 KEYED)
                            }
                        } else if chan == 2 && s_len >= hdrlen + 4 {
                            let doff = payload_bytes[hdrlen + 3] as usize;
                            let eth_off = hdrlen + 4 + doff * 4;
                            if s_len > eth_off && s_len <= payload_bytes.len() {
                                let eth = payload_bytes[eth_off..s_len].to_vec();
                                self.on_eth_tx_from_mcu(&eth);
                                self.tx_eth_frames.push(eth);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        fn on_sio_transition(&mut self, prev_out: u32, _prev_oe: u32, new_out: u32, new_oe: u32) {
            let prev_on = (prev_out >> 23) & 1;
            let new_on = (new_out >> 23) & 1;
            if prev_on == 0 && new_on == 1 {
                self.powered = true;
                // CYW43439 straps into gSPI mode ONLY if WL_DIO (GP24) is LOW when WL_REG_ON rises!
                let dio_high = (new_oe & (1 << 24)) != 0 && (new_out & (1 << 24)) != 0;
                self.gspi_strapped = !dio_high;
                self.bus_swapped = true;
            } else if new_on == 0 {
                self.powered = false;
            }

            if !self.powered || !self.gspi_strapped {
                return;
            }

            let prev_cs = (prev_out >> 25) & 1;
            let new_cs = (new_out >> 25) & 1;
            if prev_cs == 1 && new_cs == 0 {
                self.rx_bits.clear();
                self.tx_bits.clear();
                self.tx_pos = 0;
                self.cur_miso_bit = 0;
                self.cmd_parsed = false;
                return;
            }
            if prev_cs == 0 && new_cs == 1 {
                if self.cmd_parsed && self.cur_cmd_wr {
                    self.commit_write();
                }
                self.cur_miso_bit = 0;
                return;
            }

            if new_cs == 0 {
                let prev_clk = (prev_out >> 29) & 1;
                let new_clk = (new_out >> 29) & 1;
                // Rising edge: sample MOSI during command/write phase
                if prev_clk == 0 && new_clk == 1 && self.tx_bits.is_empty() {
                    assert_ne!(
                        new_oe & (1 << 24),
                        0,
                        "GP24 (WL_DIO) must be output-enabled during gSPI write clock rising edge"
                    );
                    let mosi = ((new_out >> 24) & 1) as u8;
                    self.rx_bits.push(mosi);
                    if self.rx_bits.len() == 32 && !self.cmd_parsed {
                        self.cmd_parsed = true;
                        let cmd_bytes = Self::bits_to_bytes(&self.rx_bits);
                        let raw_u32 = u32::from_le_bytes(cmd_bytes.try_into().unwrap());
                        let cmd = if self.bus_swapped {
                            Self::swap16x2(raw_u32)
                        } else {
                            raw_u32
                        };
                        self.cur_cmd_wr = ((cmd >> 31) & 1) != 0;
                        self.cur_cmd_fn = (cmd >> 28) & 3;
                        self.cur_cmd_addr = (cmd >> 11) & 0x1FFFF;
                        self.cur_cmd_sz = (cmd & 0x7FF) as usize;
                        if !self.cur_cmd_wr {
                            let resp = self.build_read_payload(
                                self.cur_cmd_fn,
                                self.cur_cmd_addr,
                                self.cur_cmd_sz,
                            );
                            self.tx_bits = Self::bytes_to_bits(&resp);
                            self.tx_pos = 0;
                        }
                    }
                }
                // Falling edge: shift out MISO in normal-speed mode (starting on 32nd falling edge!)
                if prev_clk == 1 && new_clk == 0 && !self.tx_bits.is_empty() {
                    self.cur_miso_bit = self.tx_bits.get(self.tx_pos).copied().unwrap_or(0);
                    self.tx_pos += 1;
                }
            }
        }

        fn push_async_event(&mut self, event_type: u32, flags: u16, status: u32) {
            // 14B SDPCM+pad + 4B BDC + 48B ETHER+BCMETH+EVENT_HDR = 66 bytes (pad to 68)
            let s_len = 66usize;
            let mut pkt = vec![0u8; 68];
            pkt[0..2].copy_from_slice(&(s_len as u16).to_le_bytes());
            pkt[2..4].copy_from_slice(&(!(s_len as u16)).to_le_bytes());
            pkt[5] = 1;  // ASYNCEVENT_HEADER
            pkt[7] = 14; // hdrlen
            pkt[14] = 0x20;
            let eth = &mut pkt[18..];
            eth[12..14].copy_from_slice(&0x886Cu16.to_be_bytes());
            eth[26..28].copy_from_slice(&flags.to_be_bytes());
            eth[28..32].copy_from_slice(&event_type.to_be_bytes());
            eth[32..36].copy_from_slice(&status.to_be_bytes());
            self.f2_rx_packets.push_back(pkt);
        }

        fn push_eth_rx_to_mcu(&mut self, eth_frame: &[u8]) {
            let s_len = 18 + eth_frame.len();
            let aligned = (s_len + 3) & !3;
            let mut pkt = vec![0u8; aligned];
            pkt[0..2].copy_from_slice(&(s_len as u16).to_le_bytes());
            pkt[2..4].copy_from_slice(&(!(s_len as u16)).to_le_bytes());
            pkt[5] = 2;  // DATA_HEADER
            pkt[7] = 14; // hdrlen
            pkt[14] = 0x20;
            pkt[18..18 + eth_frame.len()].copy_from_slice(eth_frame);
            self.f2_rx_packets.push_back(pkt);
        }

        fn on_eth_tx_from_mcu(&mut self, eth: &[u8]) {
            // Auto-respond to DHCP DISCOVER (1) -> OFFER (2) and REQUEST (3) -> ACK (5)!
            if eth.len() >= 42 + 240 && eth[12..14] == [0x08, 0x00] && eth[23] == 17 {
                let sport = u16::from_be_bytes([eth[34], eth[35]]);
                let dport = u16::from_be_bytes([eth[36], eth[37]]);
                if sport == 68 && dport == 67 {
                    let dhcp = &eth[42..];
                    assert_eq!(&dhcp[236..240], &[99, 130, 83, 99]);
                    let mut mtype = 0u8;
                    let mut pos = 240usize;
                    while pos < dhcp.len() && dhcp[pos] != 255 {
                        if dhcp[pos] == 0 {
                            pos += 1;
                            continue;
                        }
                        let olen = dhcp[pos + 1] as usize;
                        if dhcp[pos] == 53 && olen >= 1 {
                            mtype = dhcp[pos + 2];
                        }
                        pos += 2 + olen;
                    }
                    let reply_type = if mtype == 1 {
                        2u8 // DHCPOFFER
                    } else if mtype == 3 {
                        5u8 // DHCPACK
                    } else {
                        0u8
                    };
                    if reply_type != 0 {
                        let mut rep = vec![0u8; 42 + 272];
                        // Ethernet: router AA:BB:CC:DD:EE:01 -> MCU 28:CD:C1:11:22:33
                        rep[0..6].copy_from_slice(&eth[6..12]);
                        rep[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
                        rep[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
                        // IPv4: 192.168.1.1 -> 192.168.1.222
                        let ip = &mut rep[14..34];
                        ip[0] = 0x45;
                        ip[2..4].copy_from_slice(&((20 + 8 + 272) as u16).to_be_bytes());
                        ip[8] = 64;
                        ip[9] = 17;
                        ip[12..16].copy_from_slice(&[192, 168, 1, 1]);
                        ip[16..20].copy_from_slice(&[192, 168, 1, 222]);
                        // UDP: 67 -> 68
                        rep[34..36].copy_from_slice(&67u16.to_be_bytes());
                        rep[36..38].copy_from_slice(&68u16.to_be_bytes());
                        rep[38..40].copy_from_slice(&((8 + 272) as u16).to_be_bytes());
                        // BOOTP / DHCP reply
                        let d = &mut rep[42..];
                        d[0] = 2; // BOOTREPLY
                        d[1] = 1;
                        d[2] = 6;
                        d[4..8].copy_from_slice(&dhcp[4..8]); // xid
                        d[16..20].copy_from_slice(&[192, 168, 1, 222]); // yiaddr
                        d[28..34].copy_from_slice(&dhcp[28..34]); // chaddr
                        d[236..240].copy_from_slice(&[99, 130, 83, 99]);
                        let mut o = 240usize;
                        d[o..o + 3].copy_from_slice(&[53, 1, reply_type]);
                        o += 3;
                        d[o..o + 6].copy_from_slice(&[54, 4, 192, 168, 1, 1]); // Server ID
                        o += 6;
                        d[o..o + 6].copy_from_slice(&[1, 4, 255, 255, 255, 0]); // Subnet
                        o += 6;
                        d[o..o + 6].copy_from_slice(&[3, 4, 192, 168, 1, 1]); // Router
                        o += 6;
                        d[o] = 255;
                        self.push_eth_rx_to_mcu(&rep);
                    }
                }
            }
        }
    }

    #[test]
    fn test_cyw43439_pico2w_gspi_fw_boot_and_led_blink() {
        let ir = r#"
target triple = "thumbv8m.main-none-eabihf"

declare i64 @cyw43_led_init()
declare i64 @cyw43_led_put(i64)

define i64 @run_blink_cycle() {
entry:
  %rc = call i64 @cyw43_led_init()
  %ok = icmp eq i64 %rc, 0
  br i1 %ok, label %do_blink, label %fail

fail:
  ret i64 %rc

do_blink:
  %p1 = call i64 @cyw43_led_put(i64 1)
  %p0 = call i64 @cyw43_led_put(i64 0)
  %p2 = call i64 @cyw43_led_put(i64 1)
  ret i64 42
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip)
                .expect("failed to compile CYW43439 Pico W / Pico 2 W firmware");
            assert!(
                flash.len() > cyw43_fw::CYW43_43439_FW_BLOB.len(),
                "CYW43439 firmware blob must be linked into Flash .rodata"
            );

            let mut emu = ThumbCpuEmu::new(chip, flash, true);
            emu.init_heap_only();
            emu.cyw43_slave = Some(SimCyw43439::new());

            let ret = emu.call_fn(&asm, "run_blink_cycle", [0; 4], 80_000_000) as i64;
            assert_eq!(ret, 42, "cyw43_led_init failed with error code {}", ret);

            // Verify GP24 pad (`pads_bank0_base + 0x64`) was configured with pull-down (`0x76`), NOT pull-up (`0x7A`), and ISO=0!
            let gp24_pad = emu.mmio_regs.get(&(chip.pads_bank0_base + 0x64)).copied().unwrap_or(0);
            assert_eq!(gp24_pad, 0x76, "GP24 pad must be pull-down + 12mA + Schmitt + ISO=0");

            let cyw = emu.cyw43_slave.as_ref().unwrap();
            assert!(cyw.gspi_strapped);
            assert!(!cyw.bus_swapped);
            assert_eq!(cyw.f1_resp_delay, 4);
            assert_eq!(cyw.sram_bankx_idx, 3);
            assert_eq!(cyw.sram_bankx_pda, 0);
            assert_eq!(cyw.arm_resetctrl, 0);
            assert_eq!(cyw.arm_ioctrl, 1);

            // Byte-for-byte verification of all 224,192 bytes of CYW43439 A0 firmware in SOCSRAM[0..224192]!
            assert_eq!(
                &cyw.socsram[0..cyw43_fw::CYW43_43439_FW_BLOB.len()],
                cyw43_fw::CYW43_43439_FW_BLOB,
                "SOCSRAM firmware image mismatch!"
            );

            // Byte-for-byte verification of all 744 bytes of NVRAM at 0x7FD14 + size token at 0x7FFFC!
            let nv_len = cyw43_fw::CYW43_43439_NVRAM_BLOB.len();
            let nv_base = 512 * 1024 - 4 - nv_len;
            assert_eq!(
                &cyw.socsram[nv_base..nv_base + nv_len],
                cyw43_fw::CYW43_43439_NVRAM_BLOB,
                "SOCSRAM NVRAM image mismatch!"
            );
            let tok = u32::from_le_bytes(cyw.socsram[0x7FFFC..0x80000].try_into().unwrap());
            assert_eq!(tok, 0xFF45_00BA);

            // Verify BOTH SDPCM/BCDC WLC_SET_VAR "gpioout" packets AND ChipCommon GPIOOUT registers toggled 1 -> 0 -> 1!
            assert_eq!(cyw.f2_gpio_history, vec![(1, 1), (1, 0), (1, 1)]);
            assert_eq!(cyw.cc_gpioouten, 1);
            assert_eq!(cyw.cc_gpioout_history, vec![1, 0, 1]);
            assert!(cyw.f2_rx_packets.is_empty(), "F2 RX ack queue must be drained");
        }
    }

    fn build_host_tcp_frame(
        mcu_mac: [u8; 6],
        sport: u16,
        dport: u16,
        seq: u32,
        ack: u32,
        flags: u8,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut pkt = vec![0u8; 14 + 20 + 20 + payload.len()];
        pkt[0..6].copy_from_slice(&mcu_mac);
        pkt[6..12].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x95]);
        pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        pkt[14] = 0x45;
        pkt[16..18].copy_from_slice(&((20 + 20 + payload.len()) as u16).to_be_bytes());
        pkt[22] = 64;
        pkt[23] = 6; // TCP
        pkt[26..30].copy_from_slice(&[192, 168, 1, 149]);
        pkt[30..34].copy_from_slice(&[192, 168, 1, 222]);
        pkt[34..36].copy_from_slice(&sport.to_be_bytes());
        pkt[36..38].copy_from_slice(&dport.to_be_bytes());
        pkt[38..42].copy_from_slice(&seq.to_be_bytes());
        pkt[42..46].copy_from_slice(&ack.to_be_bytes());
        pkt[46] = 5 << 4;
        pkt[47] = flags;
        pkt[48..50].copy_from_slice(&8192u16.to_be_bytes());
        pkt[54..].copy_from_slice(payload);
        pkt
    }

    fn verify_ones_complement_zero(bytes: &[u8], initial: u32) -> u16 {
        let mut sum = initial;
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            sum += u16::from_be_bytes([bytes[i], bytes[i + 1]]) as u32;
            i += 2;
        }
        if i < bytes.len() {
            sum += (bytes[i] as u32) << 8;
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        (!sum) as u16
    }

    #[test]
    fn test_cyw43439_wifi_dhcp_and_rfc6455_websocket_port_1424() {
        let ir = r#"
target triple = "thumbv8m.main-none-eabihf"

@.ssid = private unnamed_addr constant [13 x i8] c"OmniTestSSID\00", align 1
@.psk  = private unnamed_addr constant [17 x i8] c"OmniTestPass1234\00", align 1
@.rep  = private unnamed_addr constant [10 x i8] c"PONG_1424\00", align 1

declare i64 @wifi_connect(i64, i64)
declare i8* @wifi_ip()
declare i64 @ws_listen(i64)
declare i8* @ws_poll()
declare i64 @ws_send(i64)
declare i64 @ws_is_open()

define i64 @step_connect_and_listen() {
entry:
  %s0 = ptrtoint [13 x i8]* @.ssid to i64
  %p0 = ptrtoint [17 x i8]* @.psk to i64
  %ip = call i64 @wifi_connect(i64 %s0, i64 %p0)
  %l = call i64 @ws_listen(i64 1424)
  ret i64 %ip
}

define i8* @step_ip_str() {
entry:
  %p = call i8* @wifi_ip()
  ret i8* %p
}

define i8* @step_poll() {
entry:
  %m = call i8* @ws_poll()
  ret i8* %m
}

define i64 @step_send_pong() {
entry:
  %r = ptrtoint [10 x i8]* @.rep to i64
  %n = call i64 @ws_send(i64 %r)
  ret i64 %n
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        let chip = RpChipProfile::rp2350();
        let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip)
            .expect("failed to compile Pico 2 W WebSocket firmware");

        let mut emu = ThumbCpuEmu::new(chip, flash, true);
        emu.init_heap_only();
        emu.cyw43_slave = Some(SimCyw43439::new());

        // 1) Connect to "OmniTestSSID" + acquire DHCP IP 192.168.1.222 + listen on :1424!
        let ip_u32 = emu.call_fn(&asm, "step_connect_and_listen", [0; 4], 120_000_000) as u32;
        assert_eq!(ip_u32, 0xC0A8_01DE, "expected 192.168.1.222 (0xC0A801DE)");

        let ip_ptr = emu.call_fn(&asm, "step_ip_str", [0; 4], 50_000) as u32;
        assert_eq!(emu.read_cstr(ip_ptr), "192.168.1.222");

        {
            let cyw = emu.cyw43_slave.as_ref().unwrap();
            assert_eq!(cyw.clm_received, cyw43_fw::CYW43_43439_CLM_BLOB);
            assert_eq!(cyw.joined_ssid, "OmniTestSSID");
            assert_eq!(cyw.joined_psk, "OmniTestPass1234");
            let has_beacon = cyw.tx_eth_frames.iter().any(|f| {
                f.len() > 42
                    && String::from_utf8_lossy(&f[42..])
                        .contains("OMNI_PICO2W ip=192.168.1.222 ws_port=1424")
            });
            assert!(has_beacon, "expected UDP :1425 discovery beacon");
        }

        // 2) TCP 3-way handshake on port 1424 + verify IPv4 & TCP checksums on MCU's SYN|ACK!
        let mcu_mac = [0x28, 0xCD, 0xC1, 0x11, 0x22, 0x33];
        let mut host_seq: u32 = 0x4000_1000;
        let syn_pkt = build_host_tcp_frame(mcu_mac, 54321, 1424, host_seq, 0, 0x02, &[]);
        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&syn_pkt);
        let idle = emu.call_fn(&asm, "step_poll", [0; 4], 500_000) as u32;
        assert_eq!(emu.read_cstr(idle), "");

        let syn_ack = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .last()
            .expect("expected SYN|ACK frame")
            .clone();
        assert_eq!(verify_ones_complement_zero(&syn_ack[14..34], 0), 0, "bad IPv4 checksum");
        let tcp_len = syn_ack.len() - 34;
        let pseudo = u16::from_be_bytes([syn_ack[26], syn_ack[27]]) as u32
            + u16::from_be_bytes([syn_ack[28], syn_ack[29]]) as u32
            + u16::from_be_bytes([syn_ack[30], syn_ack[31]]) as u32
            + u16::from_be_bytes([syn_ack[32], syn_ack[33]]) as u32
            + 6
            + (tcp_len as u32);
        assert_eq!(verify_ones_complement_zero(&syn_ack[34..], pseudo), 0, "bad TCP checksum");
        assert_eq!(syn_ack[47], 0x12); // SYN|ACK
        let mcu_isn = u32::from_be_bytes(syn_ack[38..42].try_into().unwrap());
        let mut mcu_seq = mcu_isn.wrapping_add(1);
        host_seq = host_seq.wrapping_add(1);

        // 3) Send ACK + RFC 6455 HTTP Upgrade request (using official RFC 6455 test vector key!)
        let ack_pkt = build_host_tcp_frame(mcu_mac, 54321, 1424, host_seq, mcu_seq, 0x10, &[]);
        let http_req = b"GET / HTTP/1.1\r\nHost: 192.168.1.222:1424\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let upgrade_pkt = build_host_tcp_frame(mcu_mac, 54321, 1424, host_seq, mcu_seq, 0x18, http_req);
        host_seq = host_seq.wrapping_add(http_req.len() as u32);

        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&ack_pkt);
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&upgrade_pkt);
        let _ = emu.call_fn(&asm, "step_poll", [0; 4], 2_000_000);

        let http_101 = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .last()
            .expect("expected HTTP 101 Switching Protocols")
            .clone();
        let http_101_str = String::from_utf8_lossy(&http_101[54..]);
        assert!(
            http_101_str.contains("HTTP/1.1 101 Switching Protocols\r\n"),
            "missing 101 status: {}",
            http_101_str
        );
        assert!(
            http_101_str.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n"),
            "RFC 6455 SHA-1 Sec-WebSocket-Accept mismatch: {}",
            http_101_str
        );
        mcu_seq = mcu_seq.wrapping_add((http_101.len() - 54) as u32);
        assert_eq!(emu.call_fn(&asm, "ws_is_open", [0; 4], 50_000), 1);

        // 4) Send masked RFC 6455 client Text frame ("status") and verify `ws_poll()` + `ws_send("PONG_1424")`!
        let mask = [0x37u8, 0xFA, 0x21, 0x3D];
        let plain = b"status";
        let mut ws_frame = vec![0x81u8, 0x80 | (plain.len() as u8)];
        ws_frame.extend_from_slice(&mask);
        for (i, &b) in plain.iter().enumerate() {
            ws_frame.push(b ^ mask[i & 3]);
        }
        let data_pkt = build_host_tcp_frame(mcu_mac, 54321, 1424, host_seq, mcu_seq, 0x18, &ws_frame);
        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&data_pkt);

        let rx_msg_ptr = emu.call_fn(&asm, "step_poll", [0; 4], 1_000_000) as u32;
        assert_eq!(emu.read_cstr(rx_msg_ptr), "status");

        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        let sent_n = emu.call_fn(&asm, "step_send_pong", [0; 4], 1_000_000);
        assert_eq!(sent_n, 9);

        let pong_eth = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .last()
            .expect("expected server WebSocket Text frame")
            .clone();
        assert_eq!(verify_ones_complement_zero(&pong_eth[14..34], 0), 0);
        assert_eq!(&pong_eth[54..56], &[0x81, 0x09]);
        assert_eq!(&pong_eth[56..65], b"PONG_1424");
    }

    #[test]
    fn test_usb_cdc_acm_enumeration_ttyacm_output_and_1200_baud_bootsel() {
        let ir = r#"
@.boot = private unnamed_addr constant [12 x i8] c"BOOT_LOG_OK\00", align 1
@.live = private unnamed_addr constant [12 x i8] c"WS RX: ping\00", align 1

declare void @__omni_print_str(i8*)
declare i64 @usb_cdc_getc()

define i64 @print_live() {
entry:
  call void @__omni_print_str(i8* getelementptr inbounds ([12 x i8], [12 x i8]* @.live, i64 0, i64 0))
  ret i64 0
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  call void @__omni_print_str(i8* getelementptr inbounds ([12 x i8], [12 x i8]* @.boot, i64 0, i64 0))
  ret i32 0
}
"#;
        for chip in [RpChipProfile::rp2040(), RpChipProfile::rp2350()] {
            let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip).unwrap();
            let mut emu = ThumbCpuEmu::new(chip, flash, true);

            // Verify Vector Table slot `16 + usbctrl_irq_num` points to `USBCTRL_ISR | 1`!
            let vt_off = *asm.symbols.get("__vector_table").unwrap() as u32;
            let usb_isr_off = *asm.symbols.get("USBCTRL_ISR").unwrap() as u32;
            let vec_word = emu.read32(chip.flash_base + vt_off + (16 + chip.usbctrl_irq_num) * 4);
            assert_eq!(vec_word, (chip.flash_base + usb_isr_off) | 1);

            // 1) Run full `Reset_Handler` -> `@main` (prints `"BOOT_LOG_OK\n"` BEFORE USB enumeration!)
            emu.call_fn(&asm, "Reset_Handler", [0; 4], 500_000);
            assert_eq!(
                emu.mmio_regs.get(&0xE000_ED08).copied(),
                Some(chip.flash_base + vt_off),
                "SCB->VTOR must anchor to __vector_table"
            );
            assert_eq!(
                emu.mmio_regs.get(&(chip.clocks_base + chip.clk_usb_ctrl_off)).copied(),
                Some(0x0800),
                "CLK_USB must be enabled on PLL_USB"
            );
            assert_eq!(
                emu.mmio_regs.get(&(chip.usb_regs_base + 0x4C)).copied(),
                Some(0x2001_0000),
                "SIE_CTRL must assert EP0_INT_1BUF | PULLUP_EN"
            );
            assert_eq!(
                emu.mmio_regs.get(&0xE000_E100).copied(),
                Some(1u32 << chip.usbctrl_irq_num),
                "NVIC_ISER0 must enable USBCTRL_IRQ"
            );
            assert_eq!(String::from_utf8_lossy(&emu.uart_tx), "BOOT_LOG_OK\n");
            // Early boot output ("BOOT_LOG_OK\r\n" = 13 bytes) must already sit in USB DPRAM TX ring!
            assert_eq!(emu.read32(0x5010_02B0), 13);
            assert_eq!(emu.read32(0x5010_02B4), 0);

            let fire_usb_isr = |emu: &mut ThumbCpuEmu, ints: u32| {
                emu.mmio_regs.insert(0x5011_0098, ints);
                emu.call_fn(&asm, "USBCTRL_ISR", [0; 4], 200_000);
                emu.mmio_regs.insert(0x5011_0098, 0);
            };
            let send_setup = |emu: &mut ThumbCpuEmu, pkt: [u8; 8]| {
                for (i, b) in pkt.into_iter().enumerate() {
                    emu.write8(0x5010_0000 + (i as u32), b);
                }
                emu.mmio_regs.insert(0x5011_0050, 0x0002_0000); // SETUP_REC
                fire_usb_isr(emu, 0x0001_0000); // USB_INTS_SETUP_REQ
            };

            // 2) Host sends BUS_RESET -> early boot `tx_head == 13` MUST be preserved!
            emu.mmio_regs.insert(0x5011_0050, 0x0008_0000);
            fire_usb_isr(&mut emu, 0x0000_1000);
            assert_eq!(emu.read32(0x5010_02B0), 13);

            // 3) Host sends GET_DESCRIPTOR(Device, wLength=64) -> verify 18-byte `2e8a:000a` descriptor!
            send_setup(&mut emu, [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 64, 0]);
            let ep0_in_ctrl = emu.read32(0x5010_0080);
            assert_eq!(ep0_in_ctrl, 0x8000 | 0x2000 | 0x0400 | 18);
            assert_eq!(&emu.usb_dpram[0x100..0x112], &usb_cdc_fw::USB_DEV_DESC);
            // Complete EP0_IN + Status ZLP OUT
            emu.write32(0x5010_0080, 0);
            emu.mmio_regs.insert(0x5011_0058, 3);
            fire_usb_isr(&mut emu, 0x0000_0010);

            // 4) Host sends SET_ADDRESS(5) -> deferred until Status ZLP IN finishes!
            send_setup(&mut emu, [0x00, 0x05, 5, 0x00, 0x00, 0x00, 0, 0]);
            assert_eq!(emu.mmio_regs.get(&0x5011_0000).copied().unwrap_or(0), 0);
            emu.write32(0x5010_0080, 0);
            emu.mmio_regs.insert(0x5011_0058, 1); // EP0_IN ZLP complete
            fire_usb_isr(&mut emu, 0x0000_0010);
            assert_eq!(emu.mmio_regs.get(&0x5011_0000).copied(), Some(5));

            // 5) Host sends GET_DESCRIPTOR(Configuration, wLength=255) -> 2 packets: 64B (DATA1) + 3B (DATA0)!
            send_setup(&mut emu, [0x80, 0x06, 0x00, 0x02, 0x00, 0x00, 255, 0]);
            assert_eq!(emu.read32(0x5010_0080), 0x8000 | 0x2000 | 0x0400 | 64);
            let mut rx_cfg = Vec::new();
            rx_cfg.extend_from_slice(&emu.usb_dpram[0x100..0x140]);
            // Host consumes packet #1 -> ISR arms packet #2 (3 bytes, DATA0)
            emu.write32(0x5010_0080, 0);
            emu.mmio_regs.insert(0x5011_0058, 1);
            fire_usb_isr(&mut emu, 0x0000_0010);
            assert_eq!(emu.read32(0x5010_0080), 0x8000 | 0x0000 | 0x0400 | 3);
            rx_cfg.extend_from_slice(&emu.usb_dpram[0x100..0x103]);
            assert_eq!(rx_cfg.as_slice(), &usb_cdc_fw::USB_CFG_DESC);
            emu.write32(0x5010_0080, 0);
            emu.mmio_regs.insert(0x5011_0058, 3);
            fire_usb_isr(&mut emu, 0x0000_0010);

            // 6) Host sends SET_CONFIGURATION(1) + opens `/dev/ttyACM0` via SET_CONTROL_LINE_STATE(DTR=1)!
            send_setup(&mut emu, [0x00, 0x09, 1, 0x00, 0x00, 0x00, 0, 0]);
            emu.write32(0x5010_0080, 0);
            emu.mmio_regs.insert(0x5011_0058, 1);
            fire_usb_isr(&mut emu, 0x0000_0010);

            send_setup(&mut emu, [0x21, 0x22, 0x01, 0x00, 0x00, 0x00, 0, 0]);
            // Early boot log `"BOOT_LOG_OK\r\n"` MUST immediately flush onto `EP2_IN` (`0x5010_0200`)!
            let ep2_ctrl = emu.read32(0x5010_0090);
            assert_eq!(ep2_ctrl, 0x8000 | 0x0000 | 0x0400 | 13);
            assert_eq!(&emu.usb_dpram[0x200..0x20D], b"BOOT_LOG_OK\r\n");

            // Host consumes `EP2_IN` packet #1
            emu.write32(0x5010_0090, 0);
            emu.mmio_regs.insert(0x5011_0058, 17); // EP0_IN ZLP + EP2_IN complete
            fire_usb_isr(&mut emu, 0x0000_0010);

            // 7) Live `print("WS RX: ping")`: 1st char `'W'` kicks idle `EP2_IN` immediately (`DATA1`, len 1),
            //    while remaining 12 bytes `"S RX: ping\r\n"` coalesce in `tx_ring` and drain on `BUFF_STATUS` (`DATA0`, len 12)!
            emu.call_fn(&asm, "print_live", [0; 4], 200_000);
            assert_eq!(emu.read32(0x5010_0090), 0x8000 | 0x2000 | 0x0400 | 1);
            assert_eq!(&emu.usb_dpram[0x200..0x201], b"W");
            emu.write32(0x5010_0090, 0);
            emu.mmio_regs.insert(0x5011_0058, 16); // EP2_IN complete -> ISR pumps coalesced 12 bytes!
            fire_usb_isr(&mut emu, 0x0000_0010);
            assert_eq!(emu.read32(0x5010_0090), 0x8000 | 0x0000 | 0x0400 | 12);
            assert_eq!(&emu.usb_dpram[0x200..0x20C], b"S RX: ping\r\n");

            // 8) Host writes `"ok"` to `/dev/ttyACM0` (`EP2_OUT` at `0x5010_0240`) -> `usb_cdc_getc()` reads it!
            emu.usb_dpram[0x240..0x242].copy_from_slice(b"ok");
            emu.write32(0x5010_0094, 0x8000 | 2);
            emu.mmio_regs.insert(0x5011_0058, 32); // EP2_OUT bit 5
            fire_usb_isr(&mut emu, 0x0000_0010);
            assert_eq!(emu.call_fn(&asm, "usb_cdc_getc", [0; 4], 50_000), b'o' as u64);
            assert_eq!(emu.call_fn(&asm, "usb_cdc_getc", [0; 4], 50_000), b'k' as u64);
            assert_eq!(emu.call_fn(&asm, "usb_cdc_getc", [0; 4], 50_000) as i64, -1);

            // 8b) Universal `@__omni_input(i8* null)` reads a line from USB CDC `EP2_OUT` (with BS editing + CRLF)
            let typed = b"ab\x08cd\r\n";
            emu.usb_dpram[0x240..0x240 + typed.len()].copy_from_slice(typed);
            emu.write32(0x5010_0094, 0x8000 | (typed.len() as u32));
            emu.mmio_regs.insert(0x5011_0058, 32);
            fire_usb_isr(&mut emu, 0x0000_0010);
            let line_ptr = emu.call_fn(&asm, "__omni_input", [0; 4], 200_000) as u32;
            assert_ne!(line_ptr, 0, "__omni_input must return a heap string pointer, not NULL");
            assert_eq!(emu.read_cstr(line_ptr), "acd");

            // 9) Host sets `1200` baud via `CDC_SET_LINE_CODING` -> arms hands-free `BOOTSEL` reboot!
            send_setup(&mut emu, [0x21, 0x20, 0x00, 0x00, 0x00, 0x00, 7, 0]);
            emu.usb_dpram[0x100..0x107].copy_from_slice(&[0xB0, 0x04, 0x00, 0x00, 0, 0, 8]); // 1200 8N1
            emu.write32(0x5010_0084, 0x8000 | 7);
            emu.mmio_regs.insert(0x5011_0058, 2); // EP0_OUT data stage complete
            fire_usb_isr(&mut emu, 0x0000_0010);
            assert_eq!(emu.read32(0x5010_02A4), 1, "1200-baud must arm BOOTSEL reboot");
        }
    }

    #[test]
    fn test_cyw43439_raw_tcp_socket_server_and_icmp_ping() {
        let ir = r#"
target triple = "thumbv8m.main-none-eabihf"

@.ssid = private unnamed_addr constant [13 x i8] c"OmniTestSSID\00", align 1
@.psk  = private unnamed_addr constant [17 x i8] c"OmniTestPass1234\00", align 1
@.pong = private unnamed_addr constant [6 x i8] c"pong\0A\00", align 1

declare i64 @wifi_connect(i64, i64)
declare i64 @tcp_listen(i64)
declare i8* @tcp_poll()
declare i64 @tcp_send(i64)
declare i64 @tcp_is_open()
declare i64 @tcp_close()

define i64 @step_connect_and_listen() {
entry:
  %s0 = ptrtoint [13 x i8]* @.ssid to i64
  %p0 = ptrtoint [17 x i8]* @.psk to i64
  %ip = call i64 @wifi_connect(i64 %s0, i64 %p0)
  %l = call i64 @tcp_listen(i64 4242)
  ret i64 %ip
}

define i8* @step_poll() {
entry:
  %m = call i8* @tcp_poll()
  ret i8* %m
}

define i64 @step_is_open() {
entry:
  %o = call i64 @tcp_is_open()
  ret i64 %o
}

define i64 @step_send_pong() {
entry:
  %r = ptrtoint [6 x i8]* @.pong to i64
  %n = call i64 @tcp_send(i64 %r)
  ret i64 %n
}

define i64 @step_close() {
entry:
  %c = call i64 @tcp_close()
  ret i64 %c
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 0
}
"#;
        let chip = RpChipProfile::rp2350();
        let (flash, asm) = compile_llvm_ir_to_flash_image(ir, &chip)
            .expect("failed to compile Pico 2 W raw TCP socket server firmware");

        let mut emu = ThumbCpuEmu::new(chip, flash, true);
        emu.init_heap_only();
        emu.cyw43_slave = Some(SimCyw43439::new());

        // 1) Connect Wi-Fi + DHCP + listen on raw TCP :4242
        let ip_u32 = emu.call_fn(&asm, "step_connect_and_listen", [0; 4], 120_000_000) as u32;
        assert_eq!(ip_u32, 0xC0A8_01DE, "expected 192.168.1.222");
        assert_eq!(emu.call_fn(&asm, "step_is_open", [0; 4], 50_000), 0);

        let mcu_mac = [0x28, 0xCD, 0xC1, 0x11, 0x22, 0x33];

        // 2) Send ICMP Echo Request (ping) -> verify Pico 2 W responds with ICMP Echo Reply!
        let mut icmp_pkt = vec![0u8; 14 + 20 + 12];
        icmp_pkt[0..6].copy_from_slice(&mcu_mac);
        icmp_pkt[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
        icmp_pkt[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        icmp_pkt[14] = 0x45;
        icmp_pkt[16..18].copy_from_slice(&32u16.to_be_bytes());
        icmp_pkt[22] = 64;
        icmp_pkt[23] = 1; // ICMP
        icmp_pkt[26..30].copy_from_slice(&[192, 168, 1, 10]);
        icmp_pkt[30..34].copy_from_slice(&[192, 168, 1, 222]);
        let ip_cs = verify_ones_complement_zero(&icmp_pkt[14..34], 0);
        icmp_pkt[24..26].copy_from_slice(&ip_cs.to_be_bytes());
        icmp_pkt[34] = 8; // Echo Request
        icmp_pkt[38..40].copy_from_slice(&0x1234u16.to_be_bytes()); // id
        icmp_pkt[40..42].copy_from_slice(&0x0001u16.to_be_bytes()); // seq
        icmp_pkt[42..46].copy_from_slice(b"PING");
        let icmp_cs = verify_ones_complement_zero(&icmp_pkt[34..], 0);
        icmp_pkt[36..38].copy_from_slice(&icmp_cs.to_be_bytes());

        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&icmp_pkt);
        let _ = emu.call_fn(&asm, "step_poll", [0; 4], 500_000);
        let icmp_rep = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .iter()
            .find(|f| f.len() >= 46 && f[23] == 1 && f[34] == 0)
            .expect("expected ICMP Echo Reply (type=0)")
            .clone();
        assert_eq!(&icmp_rep[42..46], b"PING", "ICMP echo payload mismatch");
        assert_eq!(verify_ones_complement_zero(&icmp_rep[34..], 0), 0, "bad ICMP checksum");

        // 3) Raw TCP 3-way handshake on port 4242 -> directly ESTABLISHED (no HTTP upgrade!)
        let mut host_seq: u32 = 0x5000_2000;
        let syn_pkt = build_host_tcp_frame(mcu_mac, 55123, 4242, host_seq, 0, 0x02, &[]);
        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&syn_pkt);
        let _ = emu.call_fn(&asm, "step_poll", [0; 4], 500_000);

        let syn_ack = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .last()
            .expect("expected SYN|ACK")
            .clone();
        assert_eq!(syn_ack[47], 0x12);
        let mcu_seq = u32::from_be_bytes(syn_ack[38..42].try_into().unwrap()).wrapping_add(1);
        host_seq = host_seq.wrapping_add(1);

        let ack_pkt = build_host_tcp_frame(mcu_mac, 55123, 4242, host_seq, mcu_seq, 0x10, &[]);
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&ack_pkt);
        let _ = emu.call_fn(&asm, "step_poll", [0; 4], 500_000);
        assert_eq!(emu.call_fn(&asm, "step_is_open", [0; 4], 50_000), 1, "raw TCP socket must be ESTABLISHED");

        // 4) Host sends raw TCP "ping\n" -> MCU reads via `tcp_poll()` and replies "pong\n" via `tcp_send()`!
        let ping_pkt = build_host_tcp_frame(mcu_mac, 55123, 4242, host_seq, mcu_seq, 0x18, b"ping\n");
        emu.cyw43_slave.as_mut().unwrap().tx_eth_frames.clear();
        emu.cyw43_slave.as_mut().unwrap().push_eth_rx_to_mcu(&ping_pkt);
        let rx_ptr = emu.call_fn(&asm, "step_poll", [0; 4], 500_000) as u32;
        assert_eq!(emu.read_cstr(rx_ptr), "ping\n");

        let sent = emu.call_fn(&asm, "step_send_pong", [0; 4], 500_000) as i64;
        assert_eq!(sent, 5);
        let pong_frame = emu
            .cyw43_slave
            .as_ref()
            .unwrap()
            .tx_eth_frames
            .last()
            .expect("expected raw TCP PSH|ACK pong frame")
            .clone();
        assert_eq!(pong_frame[47], 0x18); // PSH|ACK
        assert_eq!(&pong_frame[54..], b"pong\n");
        let tcp_len = pong_frame.len() - 34;
        let pseudo = u16::from_be_bytes([pong_frame[26], pong_frame[27]]) as u32
            + u16::from_be_bytes([pong_frame[28], pong_frame[29]]) as u32
            + u16::from_be_bytes([pong_frame[30], pong_frame[31]]) as u32
            + u16::from_be_bytes([pong_frame[32], pong_frame[33]]) as u32
            + 6
            + (tcp_len as u32);
        assert_eq!(verify_ones_complement_zero(&pong_frame[34..], pseudo), 0, "bad TCP pong checksum");

        // 5) `tcp_close()` sends FIN|ACK and returns to LISTEN
        emu.call_fn(&asm, "step_close", [0; 4], 500_000);
        assert_eq!(emu.call_fn(&asm, "step_is_open", [0; 4], 50_000), 0);
        let fin_frame = emu.cyw43_slave.as_ref().unwrap().tx_eth_frames.last().unwrap();
        assert_eq!(fin_frame[47], 0x11); // FIN|ACK
    }

    #[test]
    fn test_stage1_rp2350_uf2_executes_fib10_in_thumb_emu() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        let omnic_bin = repo_root.join("target/release/omnic");
        let stage1_src = repo_root.join("bootstrap/omnic_stage1.omni");
        if !omnic_bin.exists() || !stage1_src.exists() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("omni_rp2350_emu_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let stage1_bin = tmp.join("omnic_stage1");
        let prog_src = tmp.join("fib_rp2350.omni");
        let prog_uf2 = tmp.join("fib_rp2350.uf2");
        std::fs::write(
            &prog_src,
            "fn fib(n: int) -> int {\n    if n <= 1 {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nfn main() -> int {\n    return fib(10)\n}\n",
        )
        .unwrap();

        let b0 = std::process::Command::new(&omnic_bin)
            .args(["build", "--no-cache", stage1_src.to_str().unwrap(), "-o", stage1_bin.to_str().unwrap()])
            .output()
            .expect("build omnic_stage1");
        assert!(b0.status.success());

        let b1 = std::process::Command::new(&stage1_bin)
            .args([prog_src.to_str().unwrap(), prog_uf2.to_str().unwrap()])
            .output()
            .expect("emit rp2350 uf2");
        assert!(b1.status.success());

        let uf2 = std::fs::read(&prog_uf2).expect("read uf2");
        assert!(uf2.len() >= 512 && uf2.len() % 512 == 0);
        let mut flash = Vec::new();
        for blk in uf2.chunks_exact(512) {
            assert_eq!(&blk[0..4], &0x0A324655u32.to_le_bytes());
            assert_eq!(&blk[28..32], &0xE48BFF59u32.to_le_bytes());
            flash.extend_from_slice(&blk[32..288]);
        }

        let chip = RpChipProfile::rp2350();
        let mut emu = ThumbCpuEmu::new(chip, flash, false);
        let reset_vec = u32::from_le_bytes([emu.flash[4], emu.flash[5], emu.flash[6], emu.flash[7]]);
        emu.r[15] = reset_vec & !1;
        emu.r[14] = SENTINEL_EXIT_PC;
        emu.run(50_000);
        assert_eq!(emu.r[0], 55, "Stage-1 RP2350 Thumb-2 UF2 fib(10) must return r0 == 55");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
