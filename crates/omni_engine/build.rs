use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_FEATURE_FULL_BUILD").is_ok() {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let default_model_path = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("models").join("omni-coder.gguf"))
            .unwrap_or_else(|| PathBuf::from("models/omni-coder.gguf"));

        let model_path = match env::var("OMNI_MODEL_PATH") {
            Ok(p) => PathBuf::from(p),
            Err(_) => default_model_path,
        };

        if !model_path.exists() {
            panic!(
                "Feature 'full-build' is enabled, but model weights were not found at: {}\n\
                 Place omni-coder.gguf in models/ or specify OMNI_MODEL_PATH.",
                model_path.display()
            );
        }

        println!("cargo:rerun-if-changed={}", model_path.display());

        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        let abs_model_path = fs::canonicalize(&model_path).unwrap_or(model_path);

        // MSVC cl.exe / ml64.exe does not support GNU `.incbin` directives in `.S` files;
        // export canonical path for compile-time `include_bytes!` fallback on Windows MSVC.
        if target_os == "windows" && target_env == "msvc" {
            println!(
                "cargo:rustc-env=OMNI_EMBEDDED_MODEL_ABS_PATH={}",
                abs_model_path.display()
            );
            return;
        }

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let asm_path = out_dir.join("embedded_model.S");

        let is_apple = target_os == "macos" || target_os == "ios";

        let (section_decl, sym_prefix) = if is_apple {
            (".section __TEXT,__const", "_")
        } else if target_os == "windows" {
            (".section .rdata", "")
        } else {
            (".section .rodata", "")
        };

        let sym_start = format!("{}omni_embedded_model_start", sym_prefix);
        let sym_end = format!("{}omni_embedded_model_end", sym_prefix);

        // Escape backslashes for assembly string literal on Windows (MinGW/Clang)
        let abs_path_str = abs_model_path.to_string_lossy().replace('\\', "/");

        let asm_source = format!(
            r#"{section_decl}
.globl {sym_start}
.globl {sym_end}
{sym_start}:
    .incbin "{abs_path_str}"
{sym_end}:
"#
        );

        fs::write(&asm_path, asm_source).expect("Failed to write embedded_model.S");

        cc::Build::new()
            .file(&asm_path)
            .compile("omni_embedded_model");
    }
}
