use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/native");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let mut build = cc::Build::new();
    build.include("src/native");
    if target_os == "windows" {
        build.define("_CRT_SECURE_NO_WARNINGS", None);
        build.define("_CRT_NONSTDC_NO_DEPRECATE", None);
        build.define("WIN32_LEAN_AND_MEAN", None);
    }
    build.file("src/native/omni_arena.c");
    build.file("src/native/omni_chan.c");
    build.file("src/native/omni_fiber.c");
    build.file("src/native/omni_map.c");
    build.file("src/native/omni_slice.c");
    build.file("src/native/omni_value.c");
    build.opt_level(2);
    build.compile("omni_runtime_native");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let (produced_name, canonical_filename) = if target_os == "windows" && target_env == "msvc" {
        ("omni_runtime_native.lib", "omni_runtime_native.lib")
    } else {
        ("libomni_runtime_native.a", "libomni_runtime_native.a")
    };

    let src_archive = out_dir.join(produced_name);
    let fallback_archive = if produced_name.ends_with(".lib") {
        out_dir.join("libomni_runtime_native.a")
    } else {
        out_dir.join("omni_runtime_native.lib")
    };
    let actual_src = if src_archive.exists() {
        src_archive
    } else {
        fallback_archive
    };

    let embed_dest = out_dir.join("omni_runtime_archive.bin");
    fs::copy(&actual_src, &embed_dest).unwrap_or_else(|e| {
        panic!(
            "Failed copying compiled runtime archive '{}' to '{}': {}",
            actual_src.display(),
            embed_dest.display(),
            e
        )
    });

    println!(
        "cargo:rustc-env=OMNI_RUNTIME_ARCHIVE_FILENAME={}",
        canonical_filename
    );
    if target_os == "windows" {
        println!("cargo:rustc-link-lib=ws2_32");
    }
}
