pub mod chip;
pub mod elf_uf2;
pub mod ir_lower;
pub mod runtime_fw;
pub mod thumb_enc;

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
}
