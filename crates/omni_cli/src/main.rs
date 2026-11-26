use clap::{Parser as ClapParser, Subcommand};
use omni_codegen::CodeGenerator;
use omni_engine::{CandleGgufEngine, FallbackEngine, InferenceEngine, MockSynthesisEngine, OllamaInferenceEngine, VerifiedSynthesisEngine};
use omni_parser::OmniParser;
use omni_preamble::{DependencyDriller, ImportDirective, PackageEcosystem, PackageShimmer, PackageSpec, SymbolCarver, TargetArch, TargetPlatform};
use omni_verify::LlvmVerifier;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(ClapParser, Debug)]
#[command(name = "omnic", author, version, about = "The OmniScript Polyglot Systems Compiler & JIT Engine")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Direct input source file (shortcut for compilation)
    source: Option<PathBuf>,

    /// Output binary destination
    #[arg(short, long, default_value = "a.out")]
    output: PathBuf,

    /// Model inference temperature for intent lowering
    #[arg(long, global = true, default_value_t = 0.0)]
    temp: f32,

    /// LLVM optimization level (0, 1, 2, 3)
    #[arg(long, global = true, default_value_t = 2)]
    opt_level: u8,

    /// Bypass .omnicache and force re-synthesis
    #[arg(long, global = true, default_value_t = false)]
    no_cache: bool,

    /// Cache directory for LLVM IR bitcode
    #[arg(long, global = true, default_value = "tests/outputs/.omnicache")]
    cache_dir: PathBuf,

    /// Verbose diagnostic output
    #[arg(short, long, global = true, default_value_t = false)]
    verbose: bool,

    /// Target execution device for AI intent lowering: "auto" (default), "cuda", "metal", or "cpu"
    #[arg(long, global = true, default_value = "auto")]
    device: String,

    /// Target platform & C package registry preset: "auto" (default), "arch-x86_64", "debian-x86_64", "yum-x86_64", "suse-x86_64", "alpine-x86_64", "macos-arm64", "windows-x86_64" (+ aarch64 variants), or raw LLVM triple
    #[arg(long, global = true, default_value = "auto")]
    platform: String,

    /// Automatically fetch and cache missing third-party dependencies from package indices
    #[arg(long, global = true, default_value_t = false)]
    auto_resolve_dependencies: bool,

    /// Polyglot virtual environment directory (e.g. --oenv oenv). Auto-scans if present.
    #[arg(long, global = true, num_args = 0..=1, default_missing_value = "oenv")]
    oenv: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build an OmniScript program into a standalone native binary
    Build {
        /// Source file (*.omni)
        source: PathBuf,
        /// Output binary path
        #[arg(short, long, default_value = "a.out")]
        output: PathBuf,
    },
    /// Directly compile and execute an OmniScript program in-memory (JIT mode)
    Run {
        /// Source file (*.omni)
        source: PathBuf,
        /// Arguments passed to program
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Launch the interactive polyglot OmniScript REPL
    Repl,
    /// Install dependencies declared in omni-requirements.txt or individual package
    #[command(
        about = "Install dependencies from an omni-requirements manifest or package spec",
        long_about = "Synchronize polyglot third-party dependencies into a local oenv or ~/.omnicache/.\n\n\
POLYGLOT ENVIRONMENT (--oenv) & PLATFORM (--platform):\n  \
omnic install --oenv [dir]     Wrap all dependencies into an isolated local environment (defaults to './oenv').\n  \
                               Zero-sourcing: omnic run and build automatically scan for ./oenv or ./.oenv\n  \
                               in the project root with priority over local paths and host caches.\n  \
--platform <preset>            Parameterize C upstream package repo & target ABI (auto, arch-x86_64,\n  \
                               debian-x86_64, yum-x86_64, suse-x86_64, alpine-x86_64, macos-arm64,\n  \
                               windows-x86_64, etc.) with zero sudo.\n\n\
MANIFEST SYNTAX (omni-requirements.txt):\n  \
# Ecosystem Registry Packages:\n  \
'c'      lz4\n  \
'python' requests == 2.31.0\n  \
'java'   org.projectlombok:lombok == 1.18.36\n  \
'node'   express >= 4.18.2\n  \
'rust'   serde == 1.0.197\n  \
'go'     github.com/gin-gonic/gin\n\n  \
# Direct Git Repositories:\n  \
[git]'python' https://github.com/psf/requests.git#v2.31.0\n  \
[git]'rust'   https://github.com/serde-rs/serde.git#v1.0.197\n\n\
OMNISCRIPT IMPORT STYLES:\n  \
1. Explicit Dialect:  import 'c' lz4; / import 'java' lombok.Data;\n  \
2. Bare Polyglot:     import lombok.Data;  (Tier 1 AI / Tier 2 AST cues auto-detects dialect)\n  \
3. Auto-Fetch Mode:   omnic run --auto-resolve-dependencies app.omni\n\n\
EXAMPLES:\n  \
omnic install                                               # Installs from ./omni-requirements.txt\n  \
omnic install --oenv oenv 'c' lz4                           # Rootless host C package install into ./oenv\n  \
omnic install --oenv oenv --platform windows-x86_64 'c' lz4 # Rootless MSYS2 UCRT64 Windows C package install\n  \
omnic install --oenv oenv 'java' lombok                     # Installs lombok into ./oenv\n  \
omnic install 'python' fastapi                              # Installs fastapi"
    )]
    Install {
        /// Manifest path (e.g. omni-requirements.txt) or ecosystem dialect (e.g. 'java')
        #[arg(default_value = "omni-requirements.txt")]
        target: String,
        /// Optional package name when target is an ecosystem (e.g. 'org.projectlombok:lombok')
        package: Option<String>,
    },
    /// Generate a verified synthetic dataset of <omni_ir_task> <-> LLVM IR pairs (Zero external downloads)
    #[command(alias = "export-dataset")]
    SynthDataset {
        /// Number of verified samples to synthesize
        #[arg(short, long, default_value_t = 100)]
        count: usize,
        /// Destination JSONL file
        #[arg(short, long, default_value = "tests/outputs/synthetic_training_dataset.jsonl")]
        output: PathBuf,
    },
    /// Fast zero-LLM dry-run diagnostics for editors (VS Code / Vim): reports compile errors and LLM-inferred lines
    Check {
        /// Source file (*.omni)
        source: PathBuf,
        /// Emit machine-readable JSON diagnostics
        #[arg(long)]
        json: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        Some(Commands::Repl) => run_repl()?,
        Some(Commands::Check { source, json }) => {
            let has_errors = check_file(&source, json, &args.platform, args.oenv.as_deref())?;
            if has_errors {
                std::process::exit(1);
            }
        }
        Some(Commands::Install { target, package }) => {
            install_dependencies(&target, package.as_deref(), args.oenv.as_deref(), &args.platform, args.verbose)?;
        }
        Some(Commands::SynthDataset { count, output }) => {
            synth_dataset(count, &output)?;
        }
        Some(Commands::Run { source, args: prog_args }) => {
            if source
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("obc"))
                .unwrap_or(false)
            {
                let raw = fs::read(&source)?;
                let bc = omni_verify::omni_bc::OmniBcModule::from_bytes(&raw)?;
                let syms = omni_runtime::jit_runtime_symbols();
                let vm = omni_verify::omni_bc::OmniBcVm::new(&bc, &syms);
                let code = vm.call_by_name("main", &[0, 0])? as i32;
                if code != 0 {
                    std::process::exit(code);
                }
            } else {
                run_file(&source, &prog_args, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device, &args.platform, args.auto_resolve_dependencies, args.oenv.as_deref())?;
            }
        }
        Some(Commands::Build { source, output }) => {
            build_file(&source, &output, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device, &args.platform, args.auto_resolve_dependencies, args.oenv.as_deref())?;
        }
        None => {
            if let Some(ref source) = args.source {
                build_file(source, &args.output, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device, &args.platform, args.auto_resolve_dependencies, args.oenv.as_deref())?;
            } else {
                // If no arguments or subcommands provided, launch interactive REPL
                run_repl()?;
            }
        }
    }

    Ok(())
}

fn synth_dataset(count: usize, output: &Path) -> anyhow::Result<()> {
    use omni_engine::SyntheticDatasetGenerator;

    println!("============================================================");
    println!(" OmniScript Synthetic Training Dataset Generator");
    println!(" Generating {} verified <omni_ir_task> <-> LLVM IR pairs", count);
    println!(" Destination: {}", output.display());
    println!(" Hotspot-Safe Mode: ZERO external weight downloads");
    println!("============================================================");

    let generator = SyntheticDatasetGenerator::new()?;
    let records = generator.generate_verified_dataset(count);
    let written = SyntheticDatasetGenerator::write_jsonl(&records, output)?;
    let imatrix_path = output.with_file_name("imatrix_calib.txt");
    let calib_written = SyntheticDatasetGenerator::write_imatrix_calibration(&records, &imatrix_path, 300)?;

    println!("\n[SYNTHESIS COMPLETE]");
    println!("  -> Successfully synthesized and verified {} samples with Clang", written);
    println!("  -> Dataset saved to: {}", output.display());
    println!("  -> Importance-matrix calibration ({} traces) saved to: {}", calib_written, imatrix_path.display());
    Ok(())
}

fn install_dependencies(
    target: &str,
    package: Option<&str>,
    oenv_arg: Option<&str>,
    platform_spec: &str,
    verbose: bool,
) -> anyhow::Result<()> {
    let target_platform = TargetPlatform::parse(platform_spec)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let target_path = Path::new(target);
    let effective_oenv: Option<PathBuf> = oenv_arg
        .map(PathBuf::from)
        .or_else(|| DependencyDriller::discover_oenv(Path::new(".")));

    let (shimmer, dest_desc) = match effective_oenv {
        Some(ref p) => {
            PackageShimmer::init_oenv(p)
                .map_err(|e| anyhow::anyhow!("Failed to initialize oenv directory '{}': {}", p.display(), e))?;
            println!(
                "[OMNIC] Active polyglot environment target: {} (platform: {}, provider: {})",
                p.display(),
                target_platform.slug,
                target_platform.provider.as_str()
            );
            (
                PackageShimmer::with_cache_root(p.clone()).with_platform(target_platform.clone()),
                format!("{}/", p.display()),
            )
        }
        None => {
            println!(
                "[OMNIC] Target platform: {} (provider: {})",
                target_platform.slug,
                target_platform.provider.as_str()
            );
            (
                PackageShimmer::new().with_platform(target_platform.clone()),
                "~/.omnicache/".to_string(),
            )
        }
    };

    if package.is_none()
        && (target_path.is_file()
            || target.ends_with(".txt")
            || target.ends_with(".json")
            || target.ends_with(".toml"))
    {
        if !target_path.exists() {
            anyhow::bail!("Requirements manifest '{}' not found.", target_path.display());
        }
        let specs = shimmer
            .parse_requirements_file(target_path)
            .map_err(|e| anyhow::anyhow!("Failed parsing requirements: {}", e))?;

        println!(
            "OmniScript Package Shimmer: installing {} packages from {} into {}...",
            specs.len(),
            target_path.display(),
            dest_desc
        );
        let mut installed = 0;
        for spec in &specs {
            let desc = if spec.is_git {
                format!(
                    "[git]'{}' {}",
                    spec.ecosystem.as_str(),
                    spec.git_url.as_deref().unwrap_or("")
                )
            } else {
                format!(
                    "'{}' {} (version: {})",
                    spec.ecosystem.as_str(),
                    spec.name,
                    spec.version.as_deref().unwrap_or("latest")
                )
            };
            print!("  -> Resolving {} ... ", desc);
            let _ = io::stdout().flush();
            match shimmer.shim_package(spec) {
                Ok(cached_path) => {
                    println!("OK");
                    if verbose {
                        println!("     Cache: {}", cached_path.display());
                    }
                    installed += 1;
                }
                Err(e) => {
                    println!("FAILED: {}", e);
                }
            }
        }
        println!(
            "Successfully synchronized {}/{} packages into {}",
            installed,
            specs.len(),
            dest_desc
        );
        return Ok(());
    }

    // Direct single package installation
    let spec = if let Some(pkg) = package {
        let clean_eco = target.trim_matches('\'').trim_matches('"');
        let eco = PackageEcosystem::from_str(clean_eco);
        PackageSpec {
            ecosystem: eco,
            name: pkg.to_string(),
            version: None,
            is_git: false,
            git_url: None,
            git_ref: None,
            raw_line: format!("'{}' {}", clean_eco, pkg),
        }
    } else {
        let specs = PackageShimmer::parse_requirements(target);
        if let Some(first) = specs.into_iter().next() {
            first
        } else {
            PackageSpec {
                ecosystem: PackageEcosystem::from_str("polyglot"),
                name: target.to_string(),
                version: None,
                is_git: false,
                git_url: None,
                git_ref: None,
                raw_line: target.to_string(),
            }
        }
    };

    println!(
        "OmniScript Package Shimmer: resolving single package '{}' (ecosystem: {}) into {}...",
        spec.name,
        spec.ecosystem.as_str(),
        dest_desc
    );
    let cached = shimmer
        .shim_package(&spec)
        .map_err(|e| anyhow::anyhow!("Failed to shim package: {}", e))?;
    println!("Successfully cached '{}' at {}", spec.name, cached.display());
    Ok(())
}

#[derive(Debug, Clone)]
struct CompilationStats {
    pub from_cache: bool,
    pub parse_time: std::time::Duration,
    pub drill_time: std::time::Duration,
    pub codegen_time: std::time::Duration,
    pub verify_time: std::time::Duration,
    pub total_compile_time: std::time::Duration,
}

fn is_target_host_runnable(target_platform: &TargetPlatform) -> bool {
    let same_arch = target_platform.arch == TargetArch::host();
    let same_os = (cfg!(target_os = "linux") && target_platform.llvm_triple.contains("linux"))
        || (cfg!(target_os = "macos")
            && (target_platform.llvm_triple.contains("apple")
                || target_platform.llvm_triple.contains("macosx")))
        || (cfg!(target_os = "windows") && target_platform.llvm_triple.contains("windows"));
    same_arch && same_os
}

fn run_file(
    source_path: &Path,
    prog_args: &[String],
    temp: f32,
    opt_level: u8,
    no_cache: bool,
    cache_dir: &Path,
    verbose: bool,
    device: &str,
    platform_spec: &str,
    auto_resolve_dependencies: bool,
    oenv: Option<&str>,
) -> anyhow::Result<()> {
    let target_platform = TargetPlatform::parse(platform_spec)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let (llvm_ir, foreign_c_files, stats) = compile_source(
        source_path,
        temp,
        opt_level,
        no_cache,
        cache_dir,
        verbose,
        device,
        &target_platform,
        auto_resolve_dependencies,
        oenv,
    )?;
    let cache_str = if stats.from_cache { " [CACHE HIT]" } else { " [LIVE AI SYNTHESIS]" };
    println!(
        "[COMPILE] Ready in {:.2?} (Parse: {:.2?} | Codegen: {:.2?}{} | Verify: {:.2?})",
        stats.total_compile_time, stats.parse_time, stats.codegen_time, cache_str, stats.verify_time
    );
    if !is_target_host_runnable(&target_platform) {
        anyhow::bail!(
            "Target platform '{}' ({}) cannot be executed directly via host JIT on {}/{}. Use `omnic build --platform {} {}` to emit a cross-target binary.",
            target_platform.slug,
            target_platform.llvm_triple,
            std::env::consts::OS,
            std::env::consts::ARCH,
            target_platform.slug,
            source_path.display()
        );
    }
    let verifier = LlvmVerifier::new()?;
    let status = verifier.run_jit(&llvm_ir, opt_level, &foreign_c_files, prog_args)?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn normalize_output_binary_path(
    output: &Path,
    source_path: &Path,
    targeting_windows: bool,
    targeting_rp_mcu: bool,
) -> PathBuf {
    if targeting_rp_mcu {
        if output == Path::new("a.out") {
            let stem = source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("firmware");
            return PathBuf::from(format!("{}.uf2", stem));
        }
        if output.extension().is_none() {
            return output.with_extension("uf2");
        }
        return output.to_path_buf();
    }
    if targeting_windows || cfg!(windows) {
        if output == Path::new("a.out") {
            return PathBuf::from("a.exe");
        }
        if output.extension().is_none() {
            return output.with_extension("exe");
        }
    }
    output.to_path_buf()
}

fn build_file(
    source_path: &Path,
    output: &Path,
    temp: f32,
    opt_level: u8,
    no_cache: bool,
    cache_dir: &Path,
    verbose: bool,
    device: &str,
    platform_spec: &str,
    auto_resolve_dependencies: bool,
    oenv: Option<&str>,
) -> anyhow::Result<()> {
    let build_start = std::time::Instant::now();
    let mut target_platform = TargetPlatform::parse(platform_spec)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    if platform_spec.trim().eq_ignore_ascii_case("auto") {
        if let Ok(raw_src) = fs::read_to_string(source_path) {
            let peek_mod = OmniParser::new().parse_module("peek", &raw_src);
            for imp in &peek_mod.imports {
                if let Some((hal_mod, _)) = omni_preamble::extract_builtin_hal_import(imp) {
                    if let Some(inferred) = omni_preamble::infer_platform_from_hal_import(hal_mod) {
                        target_platform = inferred;
                        break;
                    }
                }
            }
        }
    }
    let targeting_windows = target_platform.llvm_triple.contains("windows");
    let targeting_rp_mcu = target_platform.is_rp_mcu();
    let final_output = normalize_output_binary_path(
        output,
        source_path,
        targeting_windows,
        targeting_rp_mcu,
    );
    println!("==================================================");
    println!(" OmniScript Compiler (omnic v{})", env!("CARGO_PKG_VERSION"));
    println!(" Target Source: {}", source_path.display());
    println!(" Platform: {} ({})", target_platform.slug, target_platform.llvm_triple);
    println!(" Temp: {:.2} | Opt-Level: -O{} | Cache: {} | Device: {} | Auto-Resolve: {}", temp, opt_level, !no_cache, device, auto_resolve_dependencies);
    println!("==================================================");

    let (llvm_ir, foreign_c_files, stats) = compile_source(
        source_path,
        temp,
        opt_level,
        no_cache,
        cache_dir,
        verbose,
        device,
        &target_platform,
        auto_resolve_dependencies,
        oenv,
    )?;
    let verifier = LlvmVerifier::new()?;

    let bin_fmt = if targeting_rp_mcu {
        "Bare-Metal UF2 + ARM-ELF"
    } else if targeting_windows {
        "PE/COFF"
    } else if target_platform.llvm_triple.contains("apple") || target_platform.llvm_triple.contains("macosx") {
        "Mach-O"
    } else {
        "ELF"
    };
    // Portable OmniBC (`.obc`) bytecode output target — zero linker, zero external toolchain
    if final_output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("obc"))
        .unwrap_or(false)
    {
        println!("\n[5/5] Emitting portable OmniBC bytecode (.obc)...");
        let bc_start = std::time::Instant::now();
        let bc = omni_verify::omni_bc::compile_llvm_ir_to_bc(&llvm_ir)?;
        let bytes = bc.to_bytes();
        fs::write(&final_output, &bytes)?;
        println!(
            "  -> Emitted OmniBC artifact: {} ({} bytes in {:.2?})",
            final_output.display(),
            bytes.len(),
            bc_start.elapsed()
        );
        return Ok(());
    }

    println!("\n[5/5] Linking native {} binary ({})...", bin_fmt, target_platform.llvm_triple);
    let link_start = std::time::Instant::now();
    let target_triple_opt = if target_platform.llvm_triple != omni_codegen::host_target_triple() {
        Some(target_platform.llvm_triple.as_str())
    } else {
        None
    };
    verifier.compile_ir_to_binary_for_target(
        &llvm_ir,
        &final_output,
        opt_level,
        &foreign_c_files,
        target_triple_opt,
    )?;
    let link_time = link_start.elapsed();
    let total_build_time = build_start.elapsed();

    println!("  -> Successfully compiled and linked native binary: {}", final_output.display());
    if targeting_rp_mcu {
        let ext_low = final_output
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext_low != "bin" {
            let companion = if ext_low == "elf" {
                final_output.with_extension("uf2")
            } else {
                final_output.with_extension("elf")
            };
            println!("  -> Companion image emitted: {}", companion.display());
        }
    }
    let cache_str = if stats.from_cache { " [CACHE HIT]" } else { " [LIVE AI SYNTHESIS]" };
    println!(
        "\n[BUILD COMPLETE] Total: {:.2?} (Parse: {:.2?} | Drill: {:.2?} | Codegen: {:.2?}{} | Verify: {:.2?} | Link: {:.2?})",
        total_build_time, stats.parse_time, stats.drill_time, stats.codegen_time, cache_str, stats.verify_time, link_time
    );
    let out_disp = final_output.display().to_string();
    if targeting_rp_mcu {
        println!(
            "Flash firmware via BOOTSEL mass-storage copy or: picotool load -f {}",
            out_disp
        );
    } else {
        let run_hint = if final_output.is_absolute() || out_disp.starts_with("./") || out_disp.starts_with(".\\") {
            out_disp
        } else if targeting_windows || cfg!(windows) {
            format!(".\\{}", out_disp)
        } else {
            format!("./{}", out_disp)
        };
        println!("Run binary with: {}", run_hint);
    }
    Ok(())
}

fn compile_source(
    source_path: &Path,
    temp: f32,
    opt_level: u8,
    no_cache: bool,
    cache_dir: &Path,
    verbose: bool,
    device: &str,
    target_platform: &TargetPlatform,
    auto_resolve_dependencies: bool,
    oenv: Option<&str>,
) -> anyhow::Result<(String, Vec<PathBuf>, CompilationStats)> {
    let start_compile = std::time::Instant::now();
    if !source_path.exists() {
        eprintln!("[ERROR] Source file '{}' does not exist.", source_path.display());
        std::process::exit(1);
    }

    let pref = match device.to_lowercase().as_str() {
        "cuda" | "gpu" => {
            if !omni_engine::cuda_is_available() {
                eprintln!("[ERROR] Explicit device '--device cuda' requested, but CUDA acceleration is not available or drivers/toolkit are missing.");
                std::process::exit(1);
            }
            omni_engine::DevicePreference::Cuda(0)
        }
        "metal" | "mps" => {
            if !omni_engine::metal_is_available() {
                eprintln!("[ERROR] Explicit device '--device metal' requested, but Apple Metal acceleration is not available (compile with '--features metal' on macOS).");
                std::process::exit(1);
            }
            omni_engine::DevicePreference::Metal(0)
        }
        "cpu" => omni_engine::DevicePreference::Cpu,
        _ => omni_engine::DevicePreference::Auto,
    };
    let candle_engine_opt = CandleGgufEngine::discover_default_with_device(pref);

    let source_content = fs::read_to_string(source_path)?;
    let module_name = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");

    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let effective_oenv_path: Option<PathBuf> = oenv
        .map(PathBuf::from)
        .or_else(|| DependencyDriller::discover_oenv(base_dir));

    if verbose {
        if let Some(ref env_p) = effective_oenv_path {
            println!("  -> Active polyglot environment (oenv): {}", env_p.display());
        }
    }

    // 1. Parse AST & Memory Instructions
    let t_parse_start = std::time::Instant::now();
    let parser = OmniParser::new();
    let mut module = parser.parse_module_with_dir(module_name, &source_content, Some(base_dir));
    let user_defined_fns: std::collections::HashSet<String> =
        module.functions.iter().map(|f| f.name.clone()).collect();
    let parse_time = t_parse_start.elapsed();

    if !module.diagnostics.is_empty() {
        for diag in &module.diagnostics {
            eprintln!("{}", diag.render());
        }
        if module.diagnostics.iter().any(|d| d.severity == omni_parser::DiagnosticSeverity::Error) {
            eprintln!("omnic: aborting due to parsing/syntax errors");
            std::process::exit(1);
        }
    }

    // 2. Preamble Assembly & Symbol Carving
    let t_drill_start = std::time::Instant::now();
    let carver = SymbolCarver::new();
    let mut foreign_c_files = if target_platform.is_rp_mcu() {
        Vec::new()
    } else {
        omni_runtime::runtime_c_sources()
    };
    let mut all_dependency_files = Vec::new();
    let mut collected_drilled_spines: Vec<String> = Vec::new();

    let mut pending_imports = module.imports.clone();
    let mut processed_imports = std::collections::HashSet::new();

    while let Some(import) = pending_imports.pop() {
        let import_key = format!("{:?}", import);
        if !processed_imports.insert(import_key) {
            continue;
        }
        if let Some((hal_mod, alias_opt)) = omni_preamble::extract_builtin_hal_import(&import) {
            if verbose {
                println!(
                    "  -> Activated built-in bare-metal HAL syscall prelude '{}' (alias: {:?})",
                    hal_mod,
                    alias_opt.unwrap_or("unqualified")
                );
            }
            continue;
        }
        match import {
            ImportDirective::FileAlias { path, alias } => {
                let full_path = base_dir.join(&path);
                if !full_path.exists() {
                    eprintln!(
                        "  [ERROR] Import file '{}' not found at '{}'",
                        path.display(),
                        full_path.display()
                    );
                    std::process::exit(1);
                }
                all_dependency_files.push(full_path.clone());
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let ext_low = ext.to_ascii_lowercase();
                let fname_low = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ext_low == "c" || ext_low == "cpp" || ext_low == "cc" {
                    let mut driller = DependencyDriller::new()
                        .with_oenv(effective_oenv_path.clone())
                        .with_platform(target_platform.clone());
                    let slice = driller.drill(&full_path);
                    for src in slice.local_sources {
                        if !foreign_c_files.contains(&src) {
                            foreign_c_files.push(src);
                        }
                    }
                    if verbose && !slice.external_frontier.is_empty() {
                        println!("  -> External frontier dependencies detected: {:?}", slice.external_frontier);
                    }
                } else if matches!(
                    ext_low.as_str(),
                    "h" | "hpp" | "ll" | "bc" | "o" | "obj" | "a" | "lib" | "so" | "dylib" | "dll"
                ) || fname_low.contains(".so.")
                {
                    if ext_low != "h" && ext_low != "hpp" && !foreign_c_files.contains(&full_path) {
                        foreign_c_files.push(full_path.clone());
                    }
                    // Discover sibling `.h` header for return-type hint extraction if present
                    if let (Some(parent), Some(stem)) = (
                        full_path.parent(),
                        full_path.file_stem().and_then(|s| s.to_str()),
                    ) {
                        let clean_stem = stem
                            .trim_start_matches("lib")
                            .split('.')
                            .next()
                            .unwrap_or(stem);
                        for h_cand in &[
                            parent.join(format!("{}.h", clean_stem)),
                            parent.join("include").join(format!("{}.h", clean_stem)),
                        ] {
                            if h_cand.is_file() && !all_dependency_files.contains(h_cand) {
                                all_dependency_files.push(h_cand.clone());
                            }
                        }
                    }
                } else if ext_low == "pio" {
                    let pio_src = fs::read_to_string(&full_path)?;
                    let progs = omni_preamble::assemble_pio_source(&pio_src).map_err(|e| {
                        anyhow::anyhow!("PIO assembly error in '{}': {}", full_path.display(), e)
                    })?;
                    let synth_src =
                        omni_preamble::synthesize_omni_pio_module(&progs, alias.as_deref());
                    let sub_mod = parser.parse_module(
                        path.file_stem().and_then(|s| s.to_str()).unwrap_or("pio"),
                        &synth_src,
                    );
                    for f in sub_mod.functions {
                        if !module.functions.iter().any(|existing| existing.name == f.name) {
                            module.functions.push(f);
                        }
                    }
                    if verbose {
                        println!(
                            "  -> Assembled {} PIO state-machine program(s) from '{}' (pure-Rust pioasm)",
                            progs.len(),
                            path.display()
                        );
                    }
                } else if ext == "omni" || ext == "py" || ext == "java" || ext == "rs" || ext == "js" {
                    if let Ok(src) = fs::read_to_string(&full_path) {
                        let sub_mod = parser.parse_module(path.file_stem().and_then(|s| s.to_str()).unwrap_or("module"), &src);
                        for f in sub_mod.functions {
                            if !module.functions.iter().any(|existing| existing.name == f.name) {
                                module.functions.push(f);
                            }
                        }
                        for s in sub_mod.structs_and_classes {
                            if !module.structs_and_classes.iter().any(|existing| existing.name == s.name) {
                                module.structs_and_classes.push(s);
                            }
                        }
                        for e in sub_mod.enums {
                            if !module.enums.iter().any(|existing| existing.name == e.name) {
                                module.enums.push(e);
                            }
                        }
                        for imp in sub_mod.imports {
                            if !module.imports.iter().any(|existing| existing == &imp) {
                                module.imports.push(imp.clone());
                                pending_imports.push(imp);
                            }
                        }
                    }
                }
                if verbose {
                    println!(
                        "  -> Bound foreign file '{}' as alias '{:?}'",
                        path.display(),
                        alias.as_deref().unwrap_or("self")
                    );
                }
            }
            ImportDirective::Ecosystem { ecosystem, module: mod_name, alias } => {
                let mut target_symbols =
                    omni_preamble::DependencyDriller::extract_dotted_symbols(&source_content);
                if let Some(ref sym) = alias {
                    if !target_symbols.contains(sym) {
                        target_symbols.push(sym.clone());
                    }
                }
                let mut driller = DependencyDriller::new()
                    .with_oenv(effective_oenv_path.clone())
                    .with_platform(target_platform.clone());
                let mut walk = driller.crawl_ecosystem_import(
                    base_dir,
                    &ecosystem,
                    &mod_name,
                    &target_symbols,
                );

                // Tiered dialect classification & missing package resolution
                if walk.resolved_origin.is_none() && walk.resolution_source == "unresolved" {
                    let mut inferred_dialect = if ecosystem != "polyglot" && ecosystem != "unknown" {
                        Some(ecosystem.clone())
                    } else {
                        None
                    };

                    // Tier 1: Local AI Inference (Hypothesis)
                    if inferred_dialect.is_none() {
                        if let Some(ref engine) = candle_engine_opt {
                            if let Ok(dial) = engine.infer_dialect(&source_content, &mod_name) {
                                if verbose {
                                    println!("  -> [Tier 1 LLM] Inferred dialect '{}' for import '{}'", dial, mod_name);
                                }
                                inferred_dialect = Some(dial);
                            }
                        }
                    }

                    // Tier 2: Static Parser Fallback
                    if inferred_dialect.is_none() {
                        if let Some(dial) = omni_preamble::static_infer_dialect(&source_content, &mod_name) {
                            if verbose {
                                println!("  -> [Tier 2 Static] Inferred dialect '{}' for import '{}'", dial, mod_name);
                            }
                            inferred_dialect = Some(dial);
                        }
                    }

                    if let Some(dial) = inferred_dialect {
                        let eco = PackageEcosystem::from_str(&dial);

                        // Retry crawling with inferred dialect if previously tried as polyglot
                        if ecosystem == "polyglot" {
                            walk = driller.crawl_ecosystem_import(base_dir, &dial, &mod_name, &target_symbols);
                        }

                        // If still unresolved, missing package handling
                        if walk.resolved_origin.is_none() && walk.resolution_source == "unresolved" {
                            let shimmer = match effective_oenv_path {
                                Some(ref p) => {
                                    let _ = PackageShimmer::init_oenv(p);
                                    PackageShimmer::with_cache_root(p.clone())
                                        .with_platform(target_platform.clone())
                                }
                                None => PackageShimmer::new()
                                    .with_platform(target_platform.clone()),
                            };
                            let dest_label = if let Some(ref p) = effective_oenv_path {
                                format!("oenv ({}, platform: {})", p.display(), target_platform.slug)
                            } else {
                                format!("~/.omnicache (platform: {})", target_platform.slug)
                            };

                            if auto_resolve_dependencies {
                                println!("[OMNIC] Missing dependency '{}' for ecosystem '{}' in {}.", mod_name, dial, dest_label);
                                println!("  -> Auto-resolving dependency via upstream package index (--auto-resolve-dependencies)...");
                                match shimmer.auto_resolve_and_shim(&eco, &mod_name) {
                                    Ok((spec, path)) => {
                                        println!("  -> Successfully auto-resolved '{}' into: {}", spec.name, path.display());
                                        walk = driller.crawl_ecosystem_import(base_dir, &dial, &mod_name, &target_symbols);
                                    }
                                    Err(e) => {
                                        eprintln!("[ERROR] Auto-resolution failed for '{}' ({:?}): {}", mod_name, eco, e);
                                        let candidates = shimmer.query_candidates(&eco, &mod_name);
                                        if !candidates.is_empty() {
                                            eprintln!("  Top candidate packages for {}:", dial);
                                            for c in candidates.iter().take(3) {
                                                eprintln!("    - {}", c.name);
                                            }
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            } else {
                                let candidates = shimmer.query_candidates(&eco, &mod_name);
                                eprintln!("\n[ERROR] Missing dependency '{}' for ecosystem '{}' in {}.", mod_name, dial, dest_label);
                                eprintln!("To install this dependency manually, run:");
                                let env_flag = if let Some(ref p) = effective_oenv_path {
                                    format!(" --oenv {}", p.display())
                                } else {
                                    String::new()
                                };
                                let plat_flag = if target_platform.slug != TargetPlatform::detect_host().slug {
                                    format!(" --platform {}", target_platform.slug)
                                } else {
                                    String::new()
                                };
                                if let Some(top) = candidates.first() {
                                    eprintln!("  omnic install{}{} '{}' {}", env_flag, plat_flag, dial, top.name);
                                } else {
                                    eprintln!("  omnic install{}{} '{}' {}", env_flag, plat_flag, dial, mod_name);
                                }
                                if !candidates.is_empty() {
                                    eprintln!("\nTop 3 candidate packages for {}:", dial);
                                    for (idx, c) in candidates.iter().take(3).enumerate() {
                                        let ver = c.version.as_deref().unwrap_or("latest");
                                        eprintln!("  {}. {} (version: {})", idx + 1, c.name, ver);
                                    }
                                }
                                eprintln!("\nAlternatively, re-run with '--auto-resolve-dependencies' to fetch and shim automatically.\n");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        // Tier 3: Unknown dialect warning
                        let dest_label = if let Some(ref p) = effective_oenv_path {
                            format!("oenv ({})", p.display())
                        } else {
                            "~/.omnicache".to_string()
                        };
                        println!(
                            "[WARN] Unresolved import '{}' (ecosystem: unknown). No matching package found in workspace or {}.",
                            mod_name, dest_label
                        );
                    }
                }

                let is_cpython_ext = |p: &std::path::Path| -> bool {
                    let s = p.to_string_lossy();
                    s.contains(".cpython-")
                        || s.contains(".abi3.")
                        || s.contains("lib-dynload")
                        || s.ends_with(".pyd")
                };
                let is_native_linkable = |p: &std::path::Path| -> bool {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let fname = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "c" | "cc" | "cpp" | "ll" | "bc" | "o" | "obj" | "so" | "dylib" | "tbd" | "a" | "lib" | "dll"
                    ) || fname.contains(".so.")
                };
                for f in &walk.visited_files {
                    if !all_dependency_files.contains(f) {
                        all_dependency_files.push(f.clone());
                    }
                    if is_native_linkable(f)
                        && !is_cpython_ext(f)
                        && !foreign_c_files.contains(f)
                    {
                        foreign_c_files.push(f.clone());
                    }
                }
                if let Some(ref orig) = walk.resolved_origin {
                    if !all_dependency_files.contains(orig) {
                        all_dependency_files.push(orig.clone());
                    }
                    if is_native_linkable(orig)
                        && !is_cpython_ext(orig)
                        && !foreign_c_files.contains(orig)
                    {
                        foreign_c_files.push(orig.clone());
                    }
                }

                // Propagate concrete resolved ecosystem (e.g. `import zlib` -> `"c"`, `import varint` -> `"python"`)
                // back into `module.imports` so CodeGenerator binds C FFI aliases & dialect contexts accurately.
                if (ecosystem == "polyglot" || ecosystem == "unknown")
                    && !walk.ecosystem.is_empty()
                    && walk.ecosystem != "polyglot"
                    && walk.resolution_source != "unresolved"
                {
                    for imp in &mut module.imports {
                        if let ImportDirective::Ecosystem {
                            ecosystem: ref mut imp_eco,
                            module: ref imp_mod,
                            ..
                        } = imp
                        {
                            if imp_mod == &mod_name && (*imp_eco == "polyglot" || *imp_eco == "unknown") {
                                *imp_eco = walk.ecosystem.clone();
                            }
                        }
                    }
                }

                // Ingest pure-source crawled module items directly into AST
                let mut all_sources_to_merge = walk.visited_files.clone();
                if let Some(ref orig) = walk.resolved_origin {
                    if !all_sources_to_merge.contains(orig) {
                        all_sources_to_merge.push(orig.clone());
                    }
                }
                for trace in &walk.call_traces {
                    for slice_line in &trace.drilled_code_slice {
                        if !slice_line.starts_with("jvm_dispatch(")
                            && !slice_line.starts_with("builtin::")
                            && !collected_drilled_spines.contains(slice_line)
                        {
                            collected_drilled_spines.push(slice_line.clone());
                        }
                    }
                    if !target_symbols.contains(&trace.symbol) {
                        target_symbols.push(trace.symbol.clone());
                    }
                    for hop in &trace.hop_chain {
                        for word in hop.split(|c: char| !c.is_alphanumeric() && c != '_') {
                            if !word.is_empty() && !target_symbols.contains(&word.to_string()) {
                                target_symbols.push(word.to_string());
                            }
                        }
                    }
                }
                // Sort sources so non-parallel submodules (e.g. prime.py, key.py) are ingested before parallel.py
                all_sources_to_merge.sort_by(|a, b| {
                    let a_par = a.to_string_lossy().contains("parallel");
                    let b_par = b.to_string_lossy().contains("parallel");
                    a_par.cmp(&b_par)
                });
                let mut sub_mods = Vec::new();
                for vf in &all_sources_to_merge {
                    let ext = vf.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext == "class" {
                        if let Ok(methods) = omni_preamble::DependencyDriller::decode_jvm_class(vf) {
                            for (m_name, params, decompiled_expr) in methods {
                                let body = if decompiled_expr.trim().starts_with("return") {
                                    decompiled_expr.trim().to_string()
                                } else {
                                    format!("return {}", decompiled_expr.trim())
                                };
                                let func_src = format!("fn {}({}) {{\n  {}\n}}\n", m_name, params.join(", "), body);
                                let sm = parser.parse_module(&mod_name, &func_src);
                                sub_mods.push(sm);
                            }
                        }
                    } else if ext == "rs" || ext == "py" || ext == "js" || ext == "go" || ext == "java" || ext == "cs" || ext == "lua" {
                        if let Ok(raw_src) = fs::read_to_string(vf) {
                            let src = if ext == "rs" {
                                raw_src.split("\n#[cfg(test)]").next().unwrap_or(&raw_src).to_string()
                            } else {
                                raw_src
                            };
                            let mut sm = parser.parse_module(&mod_name, &src);
                            sm.functions.retain(|f| {
                                !f.annotations.iter().any(|a| a.target_expr == "test" || a.raw_syntax.contains("#[test]"))
                            });
                            if ext == "go" {
                                // Go's compiler promotes escaping local addresses (`return &d`) to heap/arena allocations.
                                for func in &mut sm.functions {
                                    for op in &mut func.memory_ops {
                                        if let omni_mem::UnifiedMemOp::EscapeScope { symbol, .. } = op {
                                            *op = omni_mem::UnifiedMemOp::Alloc {
                                                target: symbol.clone(),
                                                size_bytes: "sizeof(GoEscape)".to_string(),
                                                is_arena: true,
                                            };
                                        }
                                    }
                                }
                            }
                            sub_mods.push(sm);
                        }
                    }
                }

                // Transitive reachability analysis across all crawled package submodules
                let mut reach_set: std::collections::HashSet<String> = target_symbols.iter().cloned().collect();
                let eff_alias = alias.as_deref().unwrap_or(&mod_name);
                for ts in &target_symbols {
                    reach_set.insert(format!("{}_{}", eff_alias, ts));
                }
                for word in source_content.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { reach_set.insert(word.to_string()); }
                }

                let mut changed = true;
                while changed {
                    changed = false;
                    for sm in &sub_mods {
                        for s in &sm.structs_and_classes {
                            if reach_set.contains(&s.name) {
                                for base in &s.base_classes {
                                    if reach_set.insert(base.clone()) {
                                        changed = true;
                                    }
                                }
                                for m in &s.methods {
                                    let before = reach_set.len();
                                    omni_parser::harvest_called_symbols(&m.body, &mut reach_set);
                                    if reach_set.len() > before {
                                        changed = true;
                                    }
                                }
                            }
                        }
                        for f in &sm.functions {
                            if reach_set.contains(&f.name) {
                                let before = reach_set.len();
                                omni_parser::harvest_called_symbols(&f.body, &mut reach_set);
                                if reach_set.len() > before {
                                    changed = true;
                                }
                            }
                        }
                    }
                }

                // Ingest all reachable classes, functions, and enums into module
                for sm in sub_mods {
                    for f in sm.functions {
                        if reach_set.contains(&f.name) && !module.functions.iter().any(|existing| existing.name == f.name) {
                            module.functions.push(f);
                        }
                    }
                    for s in sm.structs_and_classes {
                        if reach_set.contains(&s.name) && !module.structs_and_classes.iter().any(|existing| existing.name == s.name) {
                            module.structs_and_classes.push(s);
                        }
                    }
                    for e in sm.enums {
                        if reach_set.contains(&e.name) && !module.enums.iter().any(|existing| existing.name == e.name) {
                            module.enums.push(e);
                        }
                    }
                }
                if verbose {
                    println!(
                        "  -> Ingested foreign ecosystem '{}' module '{}' as alias '{:?}'",
                        ecosystem,
                        mod_name,
                        alias.as_deref().unwrap_or("self")
                    );
                    if let Some(ref origin) = walk.resolved_origin {
                        println!(
                            "     [Dependency Walk] Resolved '{}' via {} -> {}",
                            mod_name,
                            walk.resolution_source,
                            origin.display()
                        );
                        println!(
                            "     [Dependency Walk] Crawled {} package files; tracing call sites {:?}:",
                            walk.visited_files.len(),
                            target_symbols
                        );
                        for trace in &walk.call_traces {
                            if !trace.hop_chain.is_empty() {
                                println!(
                                    "       * .{} -> {}",
                                    trace.symbol,
                                    trace.hop_chain.join(" => ")
                                );
                                if let (Some(sc_sym), Some(sc_proto)) =
                                    (&trace.short_circuit_c_symbol, &trace.short_circuit_prototype)
                                {
                                    println!(
                                        "           |-> [C-Frontier Short-Circuit] @{} ({})",
                                        sc_sym, sc_proto
                                    );
                                }
                            } else {
                                println!("       * .{} -> [dynamic runtime attribute]", trace.symbol);
                            }
                        }
                        if !walk.native_frontier.is_empty() {
                            println!(
                                "     [Dependency Walk] Terminal Native / External Frontier ({} boundaries):",
                                walk.native_frontier.len()
                            );
                            for front in &walk.native_frontier {
                                println!("       ! {}", front);
                            }
                        }
                    } else {
                        println!(
                            "     [Dependency Walk] Module '{}' unresolved on host ({})",
                            mod_name, walk.resolution_source
                        );
                    }
                }
            }
            ImportDirective::FromDirectory { dir_path, symbols } => {
                for sym_name in symbols {
                    match carver.carve_symbol_from_dir(base_dir, &dir_path, &sym_name) {
                        Ok(sym) => {
                            if verbose {
                                let uncaged_str = if sym.is_uncaged { " [UN-CAGED]" } else { "" };
                                println!(
                                    "  -> Carved symbol '{}' from {}:{} ({}){}",
                                    sym.name,
                                    sym.source_file.display(),
                                    sym.line_number,
                                    sym.language,
                                    uncaged_str
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("  [ERROR] {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
            ImportDirective::DirectoryAlias { path, alias } => {
                if verbose {
                    println!(
                        "  -> Ingesting directory '{}' as alias '{:?}'",
                        path.display(),
                        alias.as_deref().unwrap_or("self")
                    );
                }
            }
        }
    }
    let drill_time = t_drill_start.elapsed();

    // 3. Memory & Lifetime Normalization Summary
    for f in &module.functions {
        if !f.memory_ops.is_empty() {
            if let Err(violation) = omni_mem::LifetimeAuditor::audit_scope(&f.name, &f.memory_ops) {
                eprintln!("\n[FATAL LIFETIME VIOLATION] {}", violation);
                std::process::exit(1);
            }
        }
    }

    // 4. Code Generation & Intent Synthesis (with .omnicache check)
    let t_codegen_start = std::time::Instant::now();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNI_IR_ABI_V4_POLYGLOT_RC_CLOSURES:");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(source_content.as_bytes());
    hasher.update(target_platform.slug.as_bytes());
    hasher.update(target_platform.llvm_triple.as_bytes());
    hasher.update(&temp.to_le_bytes());
    hasher.update(&[opt_level]);
    for foreign_file in &foreign_c_files {
        if let Ok(bytes) = fs::read(foreign_file) {
            hasher.update(&bytes);
        }
    }
    for dep_file in &all_dependency_files {
        if let Ok(bytes) = fs::read(dep_file) {
            hasher.update(&bytes);
        }
    }
    let cache_hash = hasher.finalize().to_hex().to_string();
    let cache_file = cache_dir.join(format!("{}.ll", cache_hash));
    let cache_links_file = cache_dir.join(format!("{}.links", cache_hash));

    let (llvm_ir, from_cache) = if !no_cache && cache_file.exists() {
        let cached_ir = fs::read_to_string(&cache_file)?;
        if let Ok(links_txt) = fs::read_to_string(&cache_links_file) {
            foreign_c_files = links_txt
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect();
        }
        (cached_ir, true)
    } else {
        let verifier = LlvmVerifier::new()?;
        let fallback = MockSynthesisEngine::new();

        let mut foreign_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
        for imp in &module.imports {
            if let omni_preamble::ImportDirective::FromDirectory { symbols, .. } = imp {
                for s in symbols {
                    for part in s.split(" as ") {
                        let pt = part.trim();
                        if !pt.is_empty() {
                            foreign_syms.insert(pt.to_string());
                        }
                    }
                }
            }
        }
        for c_path in foreign_c_files.iter().chain(all_dependency_files.iter()) {
            let ext = c_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(ext.as_str(), "h" | "hpp" | "c" | "cc" | "cpp") {
                if let Ok(src) = fs::read_to_string(c_path) {
                    let bytes = src.as_bytes();
                    let mut i = 0;
                    while i < bytes.len() {
                        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                            let start = i;
                            i += 1;
                            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                                i += 1;
                            }
                            let tok = &src[start..i];
                            let mut j = i;
                            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                                j += 1;
                            }
                            if j < bytes.len()
                                && bytes[j] == b'('
                                && !matches!(tok, "if" | "for" | "while" | "switch" | "return" | "sizeof" | "typeof" | "alignof")
                            {
                                foreign_syms.insert(tok.to_string());
                            }
                        } else {
                            i += 1;
                        }
                    }
                }
            }
        }

        let mut generated = if let Some(ref candle_engine) = candle_engine_opt {
            let dev_tag = if candle_engine.is_cuda() {
                "CUDA GPU"
            } else if candle_engine.is_metal() {
                "Apple Metal GPU"
            } else if cfg!(target_arch = "aarch64") {
                "CPU (NEON/SIMD)"
            } else {
                "CPU (AVX2/SIMD)"
            };
            if verbose {
                std::env::set_var("OMNI_VERBOSE", "1");
                println!("  -> [Inference] Using in-process Candle GGUF engine on {} ({})", dev_tag, candle_engine.model_path.display());
            }
            let primary_verified = VerifiedSynthesisEngine::new(candle_engine, &verifier);
            let codegen = CodeGenerator::new(&primary_verified)
                .with_target_triple(target_platform.llvm_triple.clone())
                .with_platform_slug(target_platform.slug.clone())
                .with_drilled_spines(collected_drilled_spines.clone())
                .with_user_functions(user_defined_fns.clone())
                .with_foreign_symbols(foreign_syms);
            codegen.generate_module_ir(&module, temp)?
        } else {
            if verbose {
                println!("  -> [Inference] Fallback to Ollama service (omni-coder)");
            }
            let primary = OllamaInferenceEngine::new("omni-coder");
            let primary_verified = VerifiedSynthesisEngine::new(&primary, &verifier);
            if std::env::var("OMNI_ALLOW_MOCK_FALLBACK").ok().as_deref() == Some("1") {
                let composite = FallbackEngine::new(primary_verified, fallback);
                let codegen = CodeGenerator::new(&composite)
                    .with_target_triple(target_platform.llvm_triple.clone())
                    .with_platform_slug(target_platform.slug.clone())
                    .with_drilled_spines(collected_drilled_spines.clone())
                    .with_user_functions(user_defined_fns.clone())
                    .with_foreign_symbols(foreign_syms);
                codegen.generate_module_ir(&module, temp)?
            } else {
                let codegen = CodeGenerator::new(&primary_verified)
                    .with_target_triple(target_platform.llvm_triple.clone())
                    .with_platform_slug(target_platform.slug.clone())
                    .with_drilled_spines(collected_drilled_spines.clone())
                    .with_user_functions(user_defined_fns.clone())
                    .with_foreign_symbols(foreign_syms);
                codegen.generate_module_ir(&module, temp)?
            }
        };

        let c_ingest = omni_preamble::ingest_and_splice_c_dependencies(
            &mut generated,
            &mut foreign_c_files,
            &all_dependency_files,
            target_platform,
            cache_dir,
        );
        if verbose {
            for diag in &c_ingest.diagnostics {
                println!("  -> {}", diag);
            }
        }
        (generated, false)
    };
    let codegen_time = t_codegen_start.elapsed();

    if verbose {
        println!("\n--- LLVM IR ---\n{}\n----------------", llvm_ir);
    }

    // 5. Verification
    let t_verify_start = std::time::Instant::now();
    let verifier = LlvmVerifier::new()?;
    if !from_cache {
        let target_triple_opt = if target_platform.llvm_triple != omni_codegen::host_target_triple() {
            Some(target_platform.llvm_triple.as_str())
        } else {
            None
        };
        verifier.verify_ir_for_target(&llvm_ir, target_triple_opt)?;
        let _ = fs::create_dir_all(cache_dir);
        let tmp_cache = cache_dir.join(format!(".tmp_{}_{}.ll", std::process::id(), cache_hash));
        if fs::write(&tmp_cache, &llvm_ir).is_ok() {
            let _ = fs::rename(&tmp_cache, &cache_file);
        }
        let links_blob = foreign_c_files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let tmp_links = cache_dir.join(format!(".tmp_{}_{}.links", std::process::id(), cache_hash));
        if fs::write(&tmp_links, &links_blob).is_ok() {
            let _ = fs::rename(&tmp_links, &cache_links_file);
        }
    }
    let verify_time = t_verify_start.elapsed();
    let total_compile_time = start_compile.elapsed();

    let stats = CompilationStats {
        from_cache,
        parse_time,
        drill_time,
        codegen_time,
        verify_time,
        total_compile_time,
    };

    Ok((llvm_ir, foreign_c_files, stats))
}

#[cfg(unix)]
fn stdin_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

#[cfg(not(unix))]
fn stdin_is_tty() -> bool {
    false
}

#[cfg(unix)]
struct RawTermGuard {
    orig: libc::termios,
}

#[cfg(unix)]
impl RawTermGuard {
    fn enter() -> io::Result<Self> {
        unsafe {
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut orig) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = orig;
            raw.c_iflag &= !(libc::IXON | libc::ICRNL | libc::BRKINT | libc::INPCK | libc::ISTRIP);
            raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
            raw.c_cflag |= libc::CS8;
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { orig })
        }
    }
}

#[cfg(unix)]
impl Drop for RawTermGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.orig);
        }
    }
}

fn highlight_omni_line(line: &str) -> String {
    const RESET: &str = "\x1b[0m";
    const KW: &str = "\x1b[1;35m";
    const TY: &str = "\x1b[36m";
    const STR_C: &str = "\x1b[32m";
    const NUM_C: &str = "\x1b[33m";
    const CMT_C: &str = "\x1b[90m";
    const FN_C: &str = "\x1b[94m";
    const DEC_C: &str = "\x1b[93m";
    const CMD_C: &str = "\x1b[1;36m";

    let trimmed = line.trim_start();
    if trimmed.starts_with(':') {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{}{}{}{}", indent, CMD_C, trimmed, RESET);
    }

    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n + 32);
    let mut i = 0usize;

    while i < n {
        let b = bytes[i];
        // Line comments: `//` or `#`
        if (b == b'/' && i + 1 < n && bytes[i + 1] == b'/') || b == b'#' {
            out.push_str(CMT_C);
            out.push_str(&line[i..]);
            out.push_str(RESET);
            break;
        }
        // String literals: `"..."` or `'...'`
        if b == b'"' || b == b'\'' {
            let quote = b;
            let start = i;
            i += 1;
            while i < n {
                if bytes[i] == b'\\' && i + 1 < n {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push_str(STR_C);
            out.push_str(&line[start..i]);
            out.push_str(RESET);
            continue;
        }
        // Decorators `@ident`
        if b == b'@' && i + 1 < n && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_') {
            let start = i;
            i += 1;
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push_str(DEC_C);
            out.push_str(&line[start..i]);
            out.push_str(RESET);
            continue;
        }
        // Numeric literals (`0x..`, `0b..`, ints, floats)
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < n
                && (bytes[i].is_ascii_hexdigit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'x'
                    || bytes[i] == b'X'
                    || bytes[i] == b'b'
                    || bytes[i] == b'B'
                    || bytes[i] == b'_')
            {
                i += 1;
            }
            out.push_str(NUM_C);
            out.push_str(&line[start..i]);
            out.push_str(RESET);
            continue;
        }
        // Identifiers, keywords, types, and function calls
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let is_kw = matches!(
                word,
                "fn" | "def" | "func" | "function" | "let" | "var" | "const" | "val" | "mut"
                    | "pub" | "extern" | "declare" | "import" | "using" | "from" | "as" | "if"
                    | "else" | "elif" | "while" | "for" | "in" | "loop" | "break" | "continue"
                    | "return" | "struct" | "class" | "enum" | "match" | "try" | "catch"
                    | "except" | "finally" | "throw" | "raise" | "go" | "spawn" | "async"
                    | "await" | "with" | "true" | "false" | "True" | "False" | "null" | "None"
                    | "nil" | "self" | "this" | "and" | "or" | "not" | "new"
            );
            let is_ty = matches!(
                word,
                "int" | "i64" | "i32" | "i16" | "i8" | "u64" | "u32" | "u16" | "u8" | "float"
                    | "f64" | "f32" | "double" | "str" | "String" | "bool" | "void" | "char"
                    | "byte" | "Vec" | "Map" | "List" | "Dict" | "Slice" | "Chan"
            );
            let next_non_ws = line[i..].bytes().find(|c| !c.is_ascii_whitespace());
            if is_kw {
                out.push_str(KW);
                out.push_str(word);
                out.push_str(RESET);
            } else if is_ty {
                out.push_str(TY);
                out.push_str(word);
                out.push_str(RESET);
            } else if next_non_ws == Some(b'(') {
                out.push_str(FN_C);
                out.push_str(word);
                out.push_str(RESET);
            } else {
                out.push_str(word);
            }
            continue;
        }
        // Any other UTF-8 char
        let ch = line[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Returns `true` when `buf` is an incomplete multi-line construct (unbalanced `{`/`(`/`[`,
/// trailing `\`, or a Python/Omni colon block `def ...:` / `if ...:` awaiting a blank line).
fn repl_needs_more_lines(buf: &str) -> bool {
    let trimmed = buf.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return false;
    }

    let mut brace: i32 = 0;
    let mut paren: i32 = 0;
    let mut bracket: i32 = 0;
    let mut in_str: Option<u8> = None;

    for raw_line in buf.lines() {
        let b = raw_line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if let Some(q) = in_str {
                if c == b'\\' && i + 1 < b.len() {
                    i += 2;
                    continue;
                }
                if c == q {
                    in_str = None;
                }
                i += 1;
                continue;
            }
            if (c == b'/' && i + 1 < b.len() && b[i + 1] == b'/') || c == b'#' {
                break;
            }
            if c == b'"' || c == b'\'' {
                in_str = Some(c);
            } else if c == b'{' {
                brace += 1;
            } else if c == b'}' {
                brace -= 1;
            } else if c == b'(' {
                paren += 1;
            } else if c == b')' {
                paren -= 1;
            } else if c == b'[' {
                bracket += 1;
            } else if c == b']' {
                bracket -= 1;
            }
            i += 1;
        }
    }

    if brace > 0 || paren > 0 || bracket > 0 {
        return true;
    }
    if trimmed.ends_with('\\') {
        return true;
    }

    // Python / indentation-style block (`def f():`, `if x:`, `for i in ...:`, `while ...:`, `class ...:`):
    // stay in multi-line mode until the user submits a blank continuation line (`\n` with empty tail).
    let first_non_empty = buf.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let starts_colon_block = first_non_empty.ends_with(':')
        && !first_non_empty.starts_with("//")
        && !first_non_empty.starts_with('#');
    if starts_colon_block {
        let ended_with_blank_line = buf.ends_with('\n')
            || (buf.contains('\n') && buf.split('\n').last().map(|l| l.trim().is_empty()).unwrap_or(false));
        if !ended_with_blank_line {
            return true;
        }
    }

    false
}

enum ReplReadEvent {
    Input(String),
    Interrupted,
    Eof,
}

struct ReplHistory {
    entries: Vec<String>,
    path: Option<PathBuf>,
}

impl ReplHistory {
    fn load() -> Self {
        let path = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|h| PathBuf::from(h).join(".omnic_history"));
        let mut entries = Vec::new();
        if let Some(ref p) = path {
            if let Ok(raw) = fs::read_to_string(p) {
                for line in raw.lines() {
                    let unesc = line.replace("\\n", "\n").replace("\\\\", "\\");
                    if !unesc.trim().is_empty() {
                        entries.push(unesc);
                    }
                }
            }
        }
        Self { entries, path }
    }

    fn push(&mut self, entry: &str) {
        let trimmed = entry.trim_end();
        if trimmed.is_empty() {
            return;
        }
        if self.entries.last().map(|s| s.as_str()) == Some(trimmed) {
            return;
        }
        self.entries.push(trimmed.to_string());
        if self.entries.len() > 500 {
            let drain = self.entries.len() - 500;
            self.entries.drain(..drain);
        }
        if let Some(ref p) = self.path {
            let serialized: String = self
                .entries
                .iter()
                .map(|e| format!("{}\n", e.replace('\\', "\\\\").replace('\n', "\\n")))
                .collect();
            let _ = fs::write(p, serialized);
        }
    }
}

fn repl_read_entry_piped(stdin: &io::Stdin) -> io::Result<ReplReadEvent> {
    let mut buf = String::new();
    let mut first = true;
    loop {
        if first {
            print!("omni> ");
        } else {
            print!("....> ");
        }
        io::stdout().flush()?;
        let mut line = String::new();
        let n = stdin.lock().read_line(&mut line)?;
        if n == 0 {
            if buf.trim().is_empty() {
                return Ok(ReplReadEvent::Eof);
            }
            return Ok(ReplReadEvent::Input(buf.trim_end().to_string()));
        }
        if !first {
            buf.push('\n');
        }
        buf.push_str(line.trim_end_matches(&['\r', '\n'][..]));
        first = false;
        if !repl_needs_more_lines(&buf) {
            return Ok(ReplReadEvent::Input(buf.trim_end().to_string()));
        }
    }
}

#[cfg(unix)]
fn repl_read_entry_tty(history: &ReplHistory) -> io::Result<ReplReadEvent> {
    use std::io::Read;

    let _raw = RawTermGuard::enter()?;
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut buf = String::new();
    let mut cursor: usize = 0; // byte index in `buf` (always char-boundary aligned)
    let mut hist_idx: Option<usize> = None;
    let mut draft_before_hist = String::new();
    let mut prev_cursor_row: usize = 0;

    let redraw = |out: &mut io::Stdout, buf: &str, cursor: usize, prev_row: &mut usize| -> io::Result<()> {
        if *prev_row > 0 {
            write!(out, "\x1b[{}A", *prev_row)?;
        }
        write!(out, "\r\x1b[J")?;

        let lines: Vec<&str> = buf.split('\n').collect();
        for (idx, l) in lines.iter().enumerate() {
            if idx == 0 {
                write!(out, "\x1b[1;32momni>\x1b[0m ")?;
            } else {
                write!(out, "\r\n\x1b[90m....>\x1b[0m ")?;
            }
            write!(out, "{}", highlight_omni_line(l))?;
        }

        // Compute target (row, col) for `cursor`
        let prefix = &buf[..cursor];
        let target_row = prefix.bytes().filter(|&b| b == b'\n').count();
        let last_nl = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let col_chars = prefix[last_nl..].chars().count();
        let total_rows = lines.len().saturating_sub(1);
        let move_up = total_rows.saturating_sub(target_row);
        if move_up > 0 {
            write!(out, "\x1b[{}A", move_up)?;
        }
        write!(out, "\r\x1b[{}C", 6 + col_chars)?;
        *prev_row = target_row;
        out.flush()
    };

    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;

    let mut byte_buf = [0u8; 1];
    loop {
        if stdin.read(&mut byte_buf)? == 0 {
            write!(stdout, "\r\n")?;
            stdout.flush()?;
            return Ok(ReplReadEvent::Eof);
        }
        let b = byte_buf[0];

        match b {
            // Ctrl+C -> cancel current entry cleanly
            0x03 => {
                let total_rows = buf.bytes().filter(|&x| x == b'\n').count();
                let down = total_rows.saturating_sub(prev_cursor_row);
                if down > 0 {
                    write!(stdout, "\x1b[{}B", down)?;
                }
                write!(stdout, "^C\r\n")?;
                stdout.flush()?;
                return Ok(ReplReadEvent::Interrupted);
            }
            // Ctrl+D -> EOF if empty, else forward delete
            0x04 => {
                if buf.is_empty() {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok(ReplReadEvent::Eof);
                } else if cursor < buf.len() {
                    let ch_len = buf[cursor..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    buf.drain(cursor..cursor + ch_len);
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                }
            }
            // Ctrl+L -> clear screen
            0x0C => {
                write!(stdout, "\x1b[2J\x1b[H")?;
                prev_cursor_row = 0;
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Ctrl+A -> start of current line
            0x01 => {
                cursor = buf[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Ctrl+E -> end of current line
            0x05 => {
                cursor = buf[cursor..].find('\n').map(|p| cursor + p).unwrap_or(buf.len());
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Ctrl+U -> delete to start of line
            0x15 => {
                let line_start = buf[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
                buf.drain(line_start..cursor);
                cursor = line_start;
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Ctrl+K -> delete to end of line
            0x0B => {
                let line_end = buf[cursor..].find('\n').map(|p| cursor + p).unwrap_or(buf.len());
                buf.drain(cursor..line_end);
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Ctrl+W -> delete previous word
            0x17 => {
                if cursor > 0 {
                    let mut p = cursor;
                    while p > 0 && buf.as_bytes()[p - 1].is_ascii_whitespace() {
                        p -= 1;
                    }
                    while p > 0 && !buf.as_bytes()[p - 1].is_ascii_whitespace() {
                        p -= 1;
                    }
                    buf.drain(p..cursor);
                    cursor = p;
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                }
            }
            // Ctrl+J -> explicit newline insertion
            0x0A => {
                buf.insert(cursor, '\n');
                cursor += 1;
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Enter (CR)
            0x0D => {
                let at_end = cursor == buf.len();
                let cur_line_start = buf[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let cur_line = &buf[cur_line_start..cursor];
                let trailing_blank_submit = buf.contains('\n') && cur_line.trim().is_empty() && at_end;
                let needs_more = !trailing_blank_submit && repl_needs_more_lines(&buf);

                if needs_more {
                    // Auto-indent 4 spaces if the current line opens a block (`{` or `:`)
                    let base_indent: String = cur_line.chars().take_while(|c| *c == ' ').collect();
                    let extra = if cur_line.trim_end().ends_with('{') || cur_line.trim_end().ends_with(':') {
                        "    "
                    } else {
                        ""
                    };
                    let insert = format!("\n{}{}", base_indent, extra);
                    buf.insert_str(cursor, &insert);
                    cursor += insert.len();
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                } else {
                    let total_rows = buf.bytes().filter(|&x| x == b'\n').count();
                    let down = total_rows.saturating_sub(prev_cursor_row);
                    if down > 0 {
                        write!(stdout, "\x1b[{}B", down)?;
                    }
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok(ReplReadEvent::Input(buf.trim_end().to_string()));
                }
            }
            // Tab -> insert 4 spaces
            0x09 => {
                buf.insert_str(cursor, "    ");
                cursor += 4;
                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
            }
            // Backspace (0x7F or 0x08)
            0x7F | 0x08 => {
                if cursor > 0 {
                    let prev_char = buf[..cursor].chars().next_back().unwrap();
                    let new_cursor = cursor - prev_char.len_utf8();
                    buf.drain(new_cursor..cursor);
                    cursor = new_cursor;
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                }
            }
            // Escape sequences (Arrow keys, Home, End, Delete, Alt+Enter)
            0x1B => {
                let mut seq1 = [0u8; 1];
                if stdin.read(&mut seq1)? == 0 {
                    continue;
                }
                if seq1[0] == 0x0D || seq1[0] == 0x0A {
                    // Alt+Enter -> insert newline
                    buf.insert(cursor, '\n');
                    cursor += 1;
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                    continue;
                }
                if seq1[0] == b'[' {
                    let mut seq2 = [0u8; 1];
                    if stdin.read(&mut seq2)? == 0 {
                        continue;
                    }
                    match seq2[0] {
                        // Up Arrow -> previous history entry
                        b'A' => {
                            if !history.entries.is_empty() {
                                let next_idx = match hist_idx {
                                    None => {
                                        draft_before_hist = buf.clone();
                                        history.entries.len() - 1
                                    }
                                    Some(0) => 0,
                                    Some(idx) => idx - 1,
                                };
                                hist_idx = Some(next_idx);
                                buf = history.entries[next_idx].clone();
                                cursor = buf.len();
                                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                            }
                        }
                        // Down Arrow -> next history entry (or restore draft)
                        b'B' => {
                            if let Some(idx) = hist_idx {
                                if idx + 1 < history.entries.len() {
                                    hist_idx = Some(idx + 1);
                                    buf = history.entries[idx + 1].clone();
                                } else {
                                    hist_idx = None;
                                    buf = draft_before_hist.clone();
                                }
                                cursor = buf.len();
                                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                            }
                        }
                        // Right Arrow
                        b'C' => {
                            if cursor < buf.len() {
                                let ch_len = buf[cursor..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                                cursor += ch_len;
                                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                            }
                        }
                        // Left Arrow
                        b'D' => {
                            if cursor > 0 {
                                let ch_len = buf[..cursor].chars().next_back().map(|c| c.len_utf8()).unwrap_or(1);
                                cursor -= ch_len;
                                redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                            }
                        }
                        // Home
                        b'H' => {
                            cursor = buf[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
                            redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                        }
                        // End
                        b'F' => {
                            cursor = buf[cursor..].find('\n').map(|p| cursor + p).unwrap_or(buf.len());
                            redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                        }
                        // Extended CSI (`3~` Delete, `1;5C`/`1;5D` Ctrl+Arrow, `1~`/`4~` Home/End)
                        b'0'..=b'9' => {
                            let mut ext = vec![seq2[0]];
                            loop {
                                let mut nxt = [0u8; 1];
                                if stdin.read(&mut nxt)? == 0 {
                                    break;
                                }
                                ext.push(nxt[0]);
                                if (0x40..=0x7E).contains(&nxt[0]) || ext.len() > 8 {
                                    break;
                                }
                            }
                            match ext.as_slice() {
                                b"3~" => {
                                    if cursor < buf.len() {
                                        let ch_len = buf[cursor..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                                        buf.drain(cursor..cursor + ch_len);
                                        redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                                    }
                                }
                                b"1~" | b"7~" => {
                                    cursor = buf[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
                                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                                }
                                b"4~" | b"8~" => {
                                    cursor = buf[cursor..].find('\n').map(|p| cursor + p).unwrap_or(buf.len());
                                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                                }
                                b"1;5C" => {
                                    // Ctrl+Right -> jump word right
                                    let mut p = cursor;
                                    while p < buf.len() && buf.as_bytes()[p].is_ascii_whitespace() {
                                        p += 1;
                                    }
                                    while p < buf.len() && !buf.as_bytes()[p].is_ascii_whitespace() {
                                        p += 1;
                                    }
                                    cursor = p;
                                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                                }
                                b"1;5D" => {
                                    // Ctrl+Left -> jump word left
                                    let mut p = cursor;
                                    while p > 0 && buf.as_bytes()[p - 1].is_ascii_whitespace() {
                                        p -= 1;
                                    }
                                    while p > 0 && !buf.as_bytes()[p - 1].is_ascii_whitespace() {
                                        p -= 1;
                                    }
                                    cursor = p;
                                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
            // Printable ASCII / UTF-8
            c if c >= 0x20 => {
                let mut utf8_bytes = vec![c];
                let expected = if c < 0x80 {
                    1
                } else if c & 0xE0 == 0xC0 {
                    2
                } else if c & 0xF0 == 0xE0 {
                    3
                } else {
                    4
                };
                for _ in 1..expected {
                    let mut cont = [0u8; 1];
                    if stdin.read(&mut cont)? == 1 {
                        utf8_bytes.push(cont[0]);
                    }
                }
                if let Ok(s) = std::str::from_utf8(&utf8_bytes) {
                    buf.insert_str(cursor, s);
                    cursor += s.len();
                    redraw(&mut stdout, &buf, cursor, &mut prev_cursor_row)?;
                }
            }
            _ => {}
        }
    }
}

#[cfg(not(unix))]
fn repl_read_entry_tty(_history: &ReplHistory) -> io::Result<ReplReadEvent> {
    repl_read_entry_piped(&io::stdin())
}

fn run_repl() -> anyhow::Result<()> {
    let is_tty = stdin_is_tty();
    if is_tty {
        println!("\x1b[1;36m============================================================\x1b[0m");
        println!(
            " \x1b[1;32mOmniScript Interactive Polyglot REPL\x1b[0m (omnic v{})",
            env!("CARGO_PKG_VERSION")
        );
        println!(" Multi-line blocks | \x1b[33m↑/↓\x1b[0m History | \x1b[35mLive Syntax Color\x1b[0m | \x1b[36m:help\x1b[0m");
        println!(" Zero-GC runtime | Live JIT Execution | Alt+Enter / Ctrl+J newline");
        println!("\x1b[1;36m============================================================\x1b[0m");
    } else {
        println!("============================================================");
        println!(" OmniScript Interactive Polyglot REPL (omnic v{})", env!("CARGO_PKG_VERSION"));
        println!(" Type polyglot statements (Python, Go, C, C#, Rust) or :help for commands");
        println!(" Zero-GC runtime | Live JIT Execution");
        println!("============================================================");
    }

    let stdin = io::stdin();
    let mut history = ReplHistory::load();
    let mut session_stmts: Vec<String> = Vec::new();
    let mut last_stdout = String::new();
    let mut disambig_enabled = false;
    let parser = OmniParser::new();
    let verifier = LlvmVerifier::new()?;
    let primary = OllamaInferenceEngine::new("omni-coder");
    let fallback = MockSynthesisEngine::new();
    let primary_verified = VerifiedSynthesisEngine::new(&primary, &verifier);
    let composite = FallbackEngine::new(primary_verified, fallback);
    let codegen = CodeGenerator::new(&composite);
    let runtime_c = omni_runtime::runtime_c_sources();

    loop {
        let ev = if is_tty {
            repl_read_entry_tty(&history)?
        } else {
            repl_read_entry_piped(&stdin)?
        };

        let raw_entry = match ev {
            ReplReadEvent::Eof => break,
            ReplReadEvent::Interrupted => continue,
            ReplReadEvent::Input(s) => s,
        };

        let trimmed = raw_entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        history.push(trimmed);

        match trimmed {
            ":exit" | ":quit" | "exit()" | "quit()" => {
                println!("Goodbye!");
                break;
            }
            ":help" => {
                println!("Available commands:");
                println!("  :ir       - Print synthesized LLVM IR for current session");
                println!("  :list     - List all statements in current session");
                println!("  :history  - Show recent command history");
                println!("  :clear    - Clear terminal screen");
                println!("  :reset    - Clear current session memory and variables");
                println!("  :disambig - Toggle conversational disambiguation on ambiguous idioms");
                println!("  :quit     - Exit REPL");
                println!("Editing keys: ↑/↓ history, ←/→ move, Ctrl+←/→ word jump, Alt+Enter / Ctrl+J newline");
                continue;
            }
            ":clear" => {
                if is_tty {
                    print!("\x1b[2J\x1b[H");
                    io::stdout().flush()?;
                }
                continue;
            }
            ":history" => {
                let start = history.entries.len().saturating_sub(25);
                for (i, h) in history.entries[start..].iter().enumerate() {
                    let preview = h.replace('\n', " ↵ ");
                    println!("  {:>3}: {}", start + i + 1, if is_tty { highlight_omni_line(&preview) } else { preview });
                }
                continue;
            }
            ":disambig" => {
                disambig_enabled = !disambig_enabled;
                println!(
                    "Conversational Disambiguation Mode: {}",
                    if disambig_enabled {
                        "ENABLED (Prompt on ambiguous operators)"
                    } else {
                        "DISABLED (Automatic Canonical Lowering)"
                    }
                );
                continue;
            }
            ":reset" => {
                session_stmts.clear();
                last_stdout.clear();
                println!("Session cleared.");
                continue;
            }
            ":list" => {
                println!("--- Session Statements ---");
                for (idx, s) in session_stmts.iter().enumerate() {
                    let display = if is_tty {
                        s.lines().map(highlight_omni_line).collect::<Vec<_>>().join("\n      ")
                    } else {
                        s.clone()
                    };
                    println!("  [{}] {}", idx + 1, display);
                }
                println!("--------------------------");
                continue;
            }
            ":ir" => {
                let candidate_src = build_repl_source(&session_stmts);
                let module = parser.parse_module("repl_session", &candidate_src);
                match codegen.generate_module_ir(&module, 0.0) {
                    Ok(ir) => println!("\n--- Synthesized LLVM IR ---\n{}\n---------------------------", ir),
                    Err(e) => eprintln!("Error generating IR: {}", e),
                }
                continue;
            }
            _ => {}
        }

        let mut processed_line = trimmed.to_string();
        if (disambig_enabled || trimmed.ends_with('?')) && trimmed.contains(":=") && !trimmed.contains('\n') {
            let clean = trimmed.trim_end_matches('?').trim();
            println!("\n[CONVERSATIONAL DISAMBIGUATION] Operator ':=' detected in: `{}`", clean);
            println!("  [1] Go-style Scoped Variable Declaration (alloca & initialize local register)");
            println!("  [2] Python-style Walrus Expression (assign & evaluate scalar in expression)");
            println!("  [3] Canonical Assignment (=)");
            print!("Select semantic interpretation [1-3] (default 1): ");
            io::stdout().flush()?;
            let mut choice = String::new();
            if stdin.lock().read_line(&mut choice).is_ok() {
                match choice.trim() {
                    "2" => {
                        println!("  -> Tagged as Python Walrus expression.");
                        processed_line = clean.to_string();
                    }
                    "3" => {
                        println!("  -> Rewritten as Canonical Assignment.");
                        processed_line = clean.replace(":=", "=");
                    }
                    _ => {
                        println!("  -> Tagged as Go Scoped Declaration.");
                        processed_line = clean.to_string();
                    }
                }
            }
        } else if trimmed.ends_with('?') && !trimmed.contains('\n') {
            processed_line = trimmed.trim_end_matches('?').trim().to_string();
        }

        let is_multiline = processed_line.contains('\n');
        let is_top_level = is_repl_top_level_decl(&processed_line);

        // If single-line expression without print/assignment/control-flow/top-level decl, wrap in print(...)
        let stmt_to_add = if !is_multiline
            && !is_top_level
            && !processed_line.starts_with("print(")
            && !processed_line.contains('=')
            && !processed_line.contains("<-")
            && !processed_line.contains(".append(")
            && !processed_line.contains(".push(")
            && !processed_line.contains(".send(")
            && !processed_line.starts_with("go ")
            && !processed_line.starts_with("for ")
            && !processed_line.starts_with("while ")
            && !processed_line.starts_with("if ")
            && !processed_line.starts_with("with ")
            && !processed_line.starts_with("return ")
        {
            format!("print({})", processed_line)
        } else {
            processed_line
        };

        let mut trial_stmts = session_stmts.clone();
        trial_stmts.push(stmt_to_add.clone());

        let candidate_src = build_repl_source(&trial_stmts);
        let module = parser.parse_module("repl_session", &candidate_src);

        match codegen.generate_module_ir(&module, 0.0) {
            Ok(ir) => {
                match verifier.run_jit_captured(&ir, 0, &runtime_c, &[]) {
                    Ok((status, stdout, stderr)) => {
                        if !status.success() {
                            eprintln!("Execution failed with status: {}", status);
                            if !stderr.is_empty() {
                                eprintln!("{}", stderr.trim());
                            }
                        } else {
                            if !last_stdout.is_empty() && stdout.starts_with(&last_stdout) {
                                print!("{}", &stdout[last_stdout.len()..]);
                            } else if !stdout.is_empty() {
                                print!("{}", stdout);
                            }
                            let _ = io::stdout().flush();
                            last_stdout = stdout;
                            session_stmts.push(stmt_to_add);
                        }
                    }
                    Err(e) => {
                        eprintln!("[REPL Error] {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[Codegen Error] {}", e);
            }
        }
    }

    Ok(())
}

fn is_repl_import(stmt: &str) -> bool {
    let t = stmt.trim_start();
    t.starts_with("import ") || t.starts_with("using ") || t.starts_with("from ")
}

fn is_repl_top_level_decl(stmt: &str) -> bool {
    let t = stmt.trim_start();
    is_repl_import(t)
        || t.starts_with("fn ")
        || t.starts_with("def ")
        || t.starts_with("func ")
        || t.starts_with("extern ")
        || t.starts_with("declare ")
        || t.starts_with("struct ")
        || t.starts_with("class ")
}

fn build_repl_source(stmts: &[String]) -> String {
    let mut imports = String::new();
    let mut top_decls = String::new();
    let mut body_stmts = String::new();

    for s in stmts {
        if is_repl_import(s) {
            imports.push_str(s);
            imports.push('\n');
        } else if is_repl_top_level_decl(s) {
            top_decls.push_str(s);
            top_decls.push_str("\n\n");
        } else {
            for line in s.lines() {
                if line.trim().is_empty() {
                    body_stmts.push('\n');
                } else {
                    body_stmts.push_str("    ");
                    body_stmts.push_str(line);
                    body_stmts.push('\n');
                }
            }
        }
    }

    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports);
        out.push('\n');
    }
    if !top_decls.is_empty() {
        out.push_str(&top_decls);
    }
    out.push_str("def main():\n");
    if body_stmts.is_empty() {
        out.push_str("    return 0\n");
    } else {
        out.push_str(&body_stmts);
        out.push_str("    return 0\n");
    }
    out
}

#[derive(Debug, Clone)]
struct ProbeIntentRecord {
    intent: String,
    ecosystem: String,
    enclosing_fn: String,
}

/// Zero-LLM dry-run synthesis probe used by `omnic check` (VS Code / Vim diagnostics).
/// Records every intent that would route into `models/omni-coder.gguf` and returns a benign
/// SSA placeholder so `omni_codegen` finishes checking the entire file in milliseconds.
struct ProbeSynthesisEngine {
    recorded: std::sync::Mutex<Vec<ProbeIntentRecord>>,
}

impl ProbeSynthesisEngine {
    fn new() -> Self {
        Self {
            recorded: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn take_records(&self) -> Vec<ProbeIntentRecord> {
        self.recorded
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

impl omni_engine::InferenceEngine for ProbeSynthesisEngine {
    fn lower_intent(
        &self,
        envelope: &omni_engine::SsaContextEnvelope,
        _temp: f32,
    ) -> Result<String, omni_engine::EngineError> {
        if let Ok(mut g) = self.recorded.lock() {
            g.push(ProbeIntentRecord {
                intent: envelope.raw_intent.clone(),
                ecosystem: envelope
                    .ecosystem_context
                    .clone()
                    .unwrap_or_else(|| "polyglot/default".to_string()),
                enclosing_fn: envelope.enclosing_fn.clone(),
            });
        }
        let ssa = envelope.next_ssa_idx;
        Ok(format!(
            "  ; >>> [Probe: {}]\n  %t_{} = add i64 0, 0\n",
            envelope.raw_intent, ssa
        ))
    }
}

fn find_source_location(src: &str, needle: &str, used_lines: &mut Vec<usize>) -> (usize, usize) {
    let trimmed = needle.trim();
    // Prefer matching the RHS expression (e.g. `Console.ReadLine()` from `ssid = Console.ReadLine()`)
    let rhs = trimmed
        .split_once('=')
        .map(|(_, r)| r.trim())
        .filter(|r| !r.is_empty())
        .unwrap_or(trimmed);

    for (idx, line) in src.lines().enumerate() {
        let ln = idx + 1;
        if used_lines.contains(&ln) {
            continue;
        }
        if let Some(pos) = line.find(trimmed).or_else(|| line.find(rhs)) {
            used_lines.push(ln);
            return (ln, pos + 1);
        }
    }
    for (idx, line) in src.lines().enumerate() {
        if let Some(pos) = line.find(rhs) {
            return (idx + 1, pos + 1);
        }
    }
    (1, 1)
}

fn check_file(
    source_path: &Path,
    as_json: bool,
    platform_arg: &str,
    explicit_oenv: Option<&str>,
) -> anyhow::Result<bool> {
    let src = fs::read_to_string(source_path)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", source_path.display(), e))?;
    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let mod_name = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");

    let parser = OmniParser::new();
    let module = parser.parse_module_with_dir(mod_name, &src, Some(base_dir));

    let mut target_platform = omni_preamble::TargetPlatform::parse(platform_arg)?;
    if platform_arg.eq_ignore_ascii_case("auto") {
        for imp in &module.imports {
            if let Some((hal_mod, _)) = omni_preamble::extract_builtin_hal_import(imp) {
                if let Some(inferred) = omni_preamble::infer_platform_from_hal_import(hal_mod) {
                    target_platform = inferred;
                    break;
                }
            }
        }
    }

    let mut driller = DependencyDriller::new().with_platform(target_platform.clone());
    if let Some(oenv_dir) = explicit_oenv {
        driller = driller.with_oenv(Some(PathBuf::from(oenv_dir)));
    } else if let Some(auto_oenv) = DependencyDriller::discover_oenv(base_dir) {
        driller = driller.with_oenv(Some(auto_oenv));
    }

    let mut collected_drilled_spines: Vec<String> = Vec::new();
    let mut spine_symbols: Vec<serde_json::Value> = Vec::new();
    let mut seen_spine: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut type_entries: Vec<serde_json::Value> = Vec::new();
    let mut seen_types: std::collections::HashSet<String> = std::collections::HashSet::new();

    let src_file_str = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf())
        .display()
        .to_string();

    let push_class_defs = |cls_list: &[omni_parser::ClassDef],
                           file_src: &str,
                           file_path_str: &str,
                           type_entries: &mut Vec<serde_json::Value>,
                           seen_types: &mut std::collections::HashSet<String>| {
        for cls in cls_list {
            if !seen_types.insert(cls.name.clone()) {
                continue;
            }
            let mut tmp = Vec::new();
            let (mut cls_line, _) = find_source_location(file_src, &format!("class {}", cls.name), &mut tmp);
            if cls_line == 1 && !file_src.lines().next().unwrap_or("").contains(&cls.name) {
                let mut tmp2 = Vec::new();
                cls_line = find_source_location(file_src, &format!("struct {}", cls.name), &mut tmp2).0;
            }
            let file_lines: Vec<&str> = file_src.lines().collect();
            let (cls_doc, cls_usage) = extract_doc_and_usage(
                &file_lines,
                cls_line.saturating_sub(1),
                false,
                &cls.name,
                "spine_class",
                &format!("class {}", cls.name),
            );
            let fields_json: Vec<serde_json::Value> = cls
                .fields
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "type": f.type_hint.as_deref().unwrap_or("int"),
                    })
                })
                .collect();
            let methods_json: Vec<serde_json::Value> = cls
                .methods
                .iter()
                .take(15)
                .map(|m| {
                    let params_s: Vec<String> = m
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, p.type_hint.as_deref().unwrap_or("any")))
                        .collect();
                    let ret_s = m.return_type.as_deref().unwrap_or("int");
                    let mut mtmp = Vec::new();
                    let (mline, _) = find_source_location(file_src, &format!("fn {}", m.name), &mut mtmp);
                    let mline = if mline == 1 && !file_src.lines().next().unwrap_or("").contains(&m.name) {
                        let mut mtmp2 = Vec::new();
                        let l2 = find_source_location(file_src, &format!("def {}", m.name), &mut mtmp2).0;
                        if l2 > 1 { l2 } else { cls_line }
                    } else {
                        mline
                    };
                    let msig = format!("fn {}({}) -> {}", m.name, params_s.join(", "), ret_s);
                    let (mdoc, musage) = extract_doc_and_usage(
                        &file_lines,
                        mline.saturating_sub(1),
                        false,
                        &format!("{}.{}", cls.name, m.name),
                        "spine_method",
                        &msig,
                    );
                    serde_json::json!({
                        "name": m.name,
                        "signature": msig,
                        "llvm_ir": format!("define i64 @__omni_method_{}_{}(%struct.{}* %self, ...)", cls.name, m.name, cls.name),
                        "doc": mdoc,
                        "usage": musage,
                        "line": mline,
                    })
                })
                .collect();
            type_entries.push(serde_json::json!({
                "name": cls.name,
                "fields": fields_json,
                "methods": methods_json,
                "doc": cls_doc,
                "usage": cls_usage,
                "file": file_path_str,
                "line": cls_line,
            }));
        }
    };

    push_class_defs(&module.structs_and_classes, &src, &src_file_str, &mut type_entries, &mut seen_types);

    for imp in &module.imports {
        if omni_preamble::extract_builtin_hal_import(imp).is_some() {
            continue;
        }
        match imp {
            omni_preamble::ImportDirective::Ecosystem {
                ecosystem,
                module: eco_mod,
                alias,
            } => {
                let rep = driller.crawl_ecosystem_import(base_dir, ecosystem, eco_mod, &[]);
                for trace in &rep.call_traces {
                    if let Some(ref sc) = trace.short_circuit_prototype {
                        collected_drilled_spines.push(format!("__OMNI_SHORT_CIRCUIT_IR__:{}", sc));
                    }
                }
                let prefix = alias
                    .as_deref()
                    .unwrap_or_else(|| eco_mod.rsplit(&['.', '/', ':'][..]).next().unwrap_or(eco_mod));
                let mut files_to_scan = rep.visited_files.clone();
                if files_to_scan.is_empty() {
                    if let Some(ref orig) = rep.resolved_origin {
                        files_to_scan.push(orig.clone());
                    }
                }
                let (sniped_syms, sniped_types) =
                    snipe_spine_exports(ecosystem, eco_mod, prefix, &files_to_scan);
                for sym_json in sniped_syms {
                    if spine_symbols.len() >= 90 {
                        break;
                    }
                    let qk = sym_json["qualified"].as_str().unwrap_or("").to_string();
                    if seen_spine.insert(qk) {
                        spine_symbols.push(sym_json);
                    }
                }
                for ty_json in sniped_types {
                    let tname = ty_json["name"].as_str().unwrap_or("").to_string();
                    if !tname.is_empty() && seen_types.insert(tname) {
                        type_entries.push(ty_json);
                    }
                }
            }
            omni_preamble::ImportDirective::FileAlias { path: rel_p, alias }
            | omni_preamble::ImportDirective::DirectoryAlias { path: rel_p, alias } => {
                let resolved = base_dir.join(rel_p);
                let cand = if resolved.is_file() {
                    Some(resolved)
                } else {
                    let with_ext = base_dir.join(format!("{}.omni", rel_p.display()));
                    if with_ext.is_file() { Some(with_ext) } else { None }
                };
                if let Some(local_file) = cand {
                    let mod_stem = local_file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("mod");
                    let prefix = alias.as_deref().unwrap_or(mod_stem);
                    if let Ok(local_src) = fs::read_to_string(&local_file) {
                        let local_mod = parser.parse_module(mod_stem, &local_src);
                        let local_file_str = local_file
                            .canonicalize()
                            .unwrap_or_else(|_| local_file.clone())
                            .display()
                            .to_string();
                        push_class_defs(
                            &local_mod.structs_and_classes,
                            &local_src,
                            &local_file_str,
                            &mut type_entries,
                            &mut seen_types,
                        );
                    }
                    let (sniped_syms, sniped_types) =
                        snipe_spine_exports("omni", mod_stem, prefix, &[local_file.clone()]);
                    for sym_json in sniped_syms {
                        if spine_symbols.len() >= 90 {
                            break;
                        }
                        let qk = sym_json["qualified"].as_str().unwrap_or("").to_string();
                        if seen_spine.insert(qk) {
                            spine_symbols.push(sym_json);
                        }
                    }
                    for ty_json in sniped_types {
                        let tname = ty_json["name"].as_str().unwrap_or("").to_string();
                        if !tname.is_empty() && seen_types.insert(tname) {
                            type_entries.push(ty_json);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let probe = ProbeSynthesisEngine::new();
    let codegen = CodeGenerator::new(&probe)
        .with_target_triple(target_platform.llvm_triple.clone())
        .with_platform_slug(target_platform.slug.clone())
        .with_drilled_spines(collected_drilled_spines);

    let mut diagnostics: Vec<serde_json::Value> = Vec::new();
    let mut used_lines: Vec<usize> = Vec::new();
    let mut has_errors = false;

    let codegen_res = codegen.generate_module_ir(&module, 0.0);

    // 1. Report every statement/expression that routes to `models/omni-coder.gguf` as an `info` diagnostic
    for rec in probe.take_records() {
        let (line, col) = find_source_location(&src, &rec.intent, &mut used_lines);
        diagnostics.push(serde_json::json!({
            "severity": "info",
            "line": line,
            "col": col,
            "code": "OMNI_LLM_INFERRED",
            "intent": rec.intent,
            "ecosystem": rec.ecosystem,
            "message": format!(
                "[LLM Inferred] '{}' in fn '{}' will be dynamically synthesized by omni-coder.gguf (ecosystem: {})",
                rec.intent, rec.enclosing_fn, rec.ecosystem
            )
        }));
    }

    // 2. Collect observed LLVM IR symbols (user functions, extern/HAL symbols, runtime intrinsics)
    let mut symbols: Vec<serde_json::Value> = Vec::new();
    let mut seen_syms: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Parse any active built-in HAL prelude externs so autocomplete & hover know all in-scope HAL symbols
    let mut prelude_srcs: Vec<&'static str> = Vec::new();
    if let Some(p) = omni_preamble::builtin_hal_prelude_for_platform(&target_platform) {
        prelude_srcs.push(p);
    }
    for imp in &module.imports {
        if let Some((hal_mod, _)) = omni_preamble::extract_builtin_hal_import(imp) {
            if let Some(p) = omni_preamble::builtin_hal_prelude_for_import(hal_mod) {
                if !prelude_srcs.contains(&p) {
                    prelude_srcs.push(p);
                }
            }
        }
    }
    for psrc in prelude_srcs {
        let pmod = parser.parse_module("__hal_prelude", psrc);
        for ef in pmod.extern_functions {
            if seen_syms.insert(ef.name.clone()) {
                let ret_ir = match ef.return_type.as_deref().map(|s| s.trim()) {
                    Some("str" | "String") => "i8*",
                    Some("float" | "f64" | "double") => "double",
                    Some("void" | "None") => "void",
                    _ => "i64",
                };
                let args_ir = vec!["i64"; ef.params.len()].join(", ");
                let sig_params: Vec<String> = ef
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_hint.as_deref().unwrap_or("int")))
                    .collect();
                symbols.push(serde_json::json!({
                    "name": ef.name,
                    "kind": "hal_extern",
                    "signature": format!("extern fn {}({}) -> {}", ef.name, sig_params.join(", "), ef.return_type.as_deref().unwrap_or("int")),
                    "llvm_ir": format!("declare {} @{}({})", ret_ir, ef.name, args_ir),
                }));
            }
        }
    }

    // 3. Extract exact LLVM IR signatures + source locations + doc comments from generated IR (functions, structs/classes, externs)
    let src_lines: Vec<&str> = src.lines().collect();
    match codegen_res {
        Ok(ir) => {
            for line in ir.lines() {
                let t = line.trim();
                // Struct / class LLVM layout: `%struct.Point = type { ... }`
                if let Some(rest) = t.strip_prefix("%struct.") {
                    if let Some(eq_pos) = rest.find(" = type ") {
                        let sname = rest[..eq_pos].trim();
                        if !sname.starts_with("__omni_") && seen_syms.insert(sname.to_string()) {
                            let mut tmp = Vec::new();
                            let (def_line, _) = find_source_location(&src, &format!("struct {}", sname), &mut tmp);
                            let def_line = if def_line == 1 && !src.lines().next().unwrap_or("").contains(sname) {
                                let mut tmp2 = Vec::new();
                                find_source_location(&src, &format!("class {}", sname), &mut tmp2).0
                            } else {
                                def_line
                            };
                            let (doc, usage) = extract_doc_and_usage(
                                &src_lines,
                                def_line.saturating_sub(1),
                                false,
                                sname,
                                "spine_class",
                                &format!("struct {}", sname),
                            );
                            symbols.push(serde_json::json!({
                                "name": sname,
                                "kind": "struct",
                                "signature": format!("struct {}", sname),
                                "llvm_ir": t,
                                "doc": doc,
                                "usage": usage,
                                "file": src_file_str,
                                "line": def_line,
                            }));
                        }
                    }
                } else if let Some(rest) = t.strip_prefix("define ") {
                    if let Some(at_pos) = rest.find('@') {
                        let after_at = &rest[at_pos + 1..];
                        if let Some(lp) = after_at.find('(') {
                            let raw_name = &after_at[..lp];
                            let user_name = raw_name.strip_prefix("__omni_fn_").unwrap_or(raw_name);
                            if !user_name.starts_with("__omni_") && seen_syms.insert(user_name.to_string()) {
                                let hdr = t.trim_end_matches('{').trim();
                                let mut tmp = Vec::new();
                                let (def_line, _) = find_source_location(&src, &format!("fn {}", user_name), &mut tmp);
                                let def_line = if def_line == 1 && !src.lines().next().unwrap_or("").contains(user_name) {
                                    let mut tmp2 = Vec::new();
                                    find_source_location(&src, &format!("def {}", user_name), &mut tmp2).0
                                } else {
                                    def_line
                                };
                                let raw_sig = src_lines
                                    .get(def_line.saturating_sub(1))
                                    .map(|l| l.trim().trim_end_matches('{').trim_end_matches(':').trim())
                                    .filter(|s| s.contains(user_name))
                                    .unwrap_or(user_name);
                                let (doc, usage) = extract_doc_and_usage(
                                    &src_lines,
                                    def_line.saturating_sub(1),
                                    raw_sig.starts_with("def "),
                                    user_name,
                                    "spine_fn",
                                    raw_sig,
                                );
                                symbols.push(serde_json::json!({
                                    "name": user_name,
                                    "kind": "function",
                                    "signature": if raw_sig == user_name { format!("fn {}(...)", user_name) } else { raw_sig.to_string() },
                                    "llvm_ir": hdr,
                                    "doc": doc,
                                    "usage": usage,
                                    "file": src_file_str,
                                    "line": def_line,
                                }));
                            }
                        }
                    }
                } else if let Some(rest) = t.strip_prefix("declare ") {
                    if let Some(at_pos) = rest.find('@') {
                        let after_at = &rest[at_pos + 1..];
                        if let Some(lp) = after_at.find('(') {
                            let sym = &after_at[..lp];
                            if seen_syms.insert(sym.to_string()) {
                                let kind = if sym.starts_with("__omni_") { "runtime" } else { "extern" };
                                symbols.push(serde_json::json!({
                                    "name": sym,
                                    "kind": kind,
                                    "signature": format!("@{}()", sym),
                                    "llvm_ir": t,
                                }));
                            }
                        }
                    }
                }
            }
        }
        Err(err) => {
            has_errors = true;
            let msg = err.to_string();
            let sym_hint = msg
                .split('\'')
                .nth(1)
                .filter(|s| !s.is_empty() && s.len() <= 80)
                .unwrap_or("");
            let (line, col) = if !sym_hint.is_empty() {
                let mut err_used = Vec::new();
                find_source_location(&src, sym_hint, &mut err_used)
            } else {
                (1, 1)
            };
            diagnostics.push(serde_json::json!({
                "severity": "error",
                "line": line,
                "col": col,
                "code": "OMNI_SEMANTIC_ERROR",
                "message": msg
            }));
        }
    }

    if as_json {
        let payload = serde_json::json!({
            "file": source_path.display().to_string(),
            "platform": format!("{} ({})", target_platform.slug, target_platform.llvm_triple),
            "diagnostics": diagnostics,
            "symbols": symbols,
            "spine_symbols": spine_symbols,
            "types": type_entries
        });
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        println!(
            "omnic check: {} [{} ({})]",
            source_path.display(),
            target_platform.slug,
            target_platform.llvm_triple
        );
        if diagnostics.is_empty() {
            println!("  OK (100% deterministic, zero LLM-inferred sites, zero errors)");
        } else {
            for d in &diagnostics {
                println!(
                    "  {}:{}:{} [{}] {}",
                    source_path.display(),
                    d["line"].as_u64().unwrap_or(1),
                    d["col"].as_u64().unwrap_or(1),
                    d["severity"].as_str().unwrap_or("info").to_uppercase(),
                    d["message"].as_str().unwrap_or("")
                );
            }
        }
    }

    Ok(has_errors)
}

/// Extract leading doc-comments (`///`, `//`, `#`, `/** ... */`) or Python `"""..."""` docstrings
/// directly from a function/class/method definition block, plus a concise synthesized usage snippet.
fn extract_doc_and_usage(
    lines: &[&str],
    def_idx: usize,
    is_python: bool,
    qualified: &str,
    kind: &str,
    sig: &str,
) -> (Option<String>, String) {
    let mut doc_lines: Vec<String> = Vec::new();

    // 1. Scan backwards up to 8 lines for leading doc-comments (skipping decorators/attributes)
    if def_idx > 0 && def_idx <= lines.len() {
        let mut cur = def_idx;
        let mut steps = 0usize;
        let mut in_block_comment = false;
        while cur > 0 && steps < 10 {
            cur -= 1;
            steps += 1;
            let t = lines[cur].trim();
            if t.is_empty() {
                if !doc_lines.is_empty() || in_block_comment {
                    break;
                }
                continue;
            }
            // Skip decorators/attributes (`@Override`, `@staticmethod`, `#[inline]`)
            if !in_block_comment && doc_lines.is_empty() && (t.starts_with('@') || t.starts_with("#[")) {
                continue;
            }
            if in_block_comment {
                let cleaned = t
                    .trim_start_matches("/**")
                    .trim_start_matches("/*")
                    .trim_start_matches('*')
                    .trim_end_matches("*/")
                    .trim();
                if !cleaned.is_empty() {
                    doc_lines.push(cleaned.to_string());
                }
                if t.contains("/*") {
                    break;
                }
                continue;
            }
            if t.ends_with("*/") {
                in_block_comment = true;
                let cleaned = t
                    .trim_start_matches("/**")
                    .trim_start_matches("/*")
                    .trim_end_matches("*/")
                    .trim_start_matches('*')
                    .trim();
                if !cleaned.is_empty() {
                    doc_lines.push(cleaned.to_string());
                }
                if t.contains("/*") {
                    break;
                }
                continue;
            }
            if let Some(rest) = t
                .strip_prefix("///")
                .or_else(|| t.strip_prefix("//!"))
                .or_else(|| t.strip_prefix("//"))
                .or_else(|| t.strip_prefix('#'))
            {
                let cleaned = rest.trim();
                if !cleaned.is_empty() {
                    doc_lines.push(cleaned.to_string());
                }
                continue;
            }
            break;
        }
        doc_lines.reverse();
    }

    // 2. If no leading comment found and Python-style block, scan 1..=6 lines forward for `"""..."""` or `'''...'''`
    if doc_lines.is_empty() && is_python && def_idx + 1 < lines.len() {
        let mut scan = def_idx + 1;
        while scan < lines.len() && scan <= def_idx + 2 && lines[scan].trim().is_empty() {
            scan += 1;
        }
        if scan < lines.len() {
            let first = lines[scan].trim();
            let delim = if first.starts_with("\"\"\"") || first.starts_with("r\"\"\"") {
                Some("\"\"\"")
            } else if first.starts_with("'''") || first.starts_with("r'''") {
                Some("'''")
            } else {
                None
            };
            if let Some(q) = delim {
                let after_open = first.trim_start_matches('r').trim_start_matches(q);
                if let Some(end_pos) = after_open.find(q) {
                    let body = after_open[..end_pos].trim();
                    if !body.is_empty() {
                        doc_lines.push(body.to_string());
                    }
                } else {
                    if !after_open.trim().is_empty() {
                        doc_lines.push(after_open.trim().to_string());
                    }
                    for l in lines.iter().skip(scan + 1).take(6) {
                        let lt = l.trim();
                        if let Some(end_pos) = lt.find(q) {
                            let tail = lt[..end_pos].trim();
                            if !tail.is_empty() {
                                doc_lines.push(tail.to_string());
                            }
                            break;
                        } else if !lt.is_empty() {
                            doc_lines.push(lt.to_string());
                        }
                    }
                }
            }
        }
    }

    let doc = if doc_lines.is_empty() {
        None
    } else {
        let joined: String = doc_lines.into_iter().take(6).collect::<Vec<_>>().join("\n");
        let capped: String = joined.chars().take(360).collect();
        Some(capped)
    };

    // 3. Synthesize concise usage snippet from signature parameter list
    let usage = if kind == "spine_class" {
        format!("let obj = {}()", qualified)
    } else if let (Some(lp), Some(rp)) = (sig.find('('), sig.rfind(')')) {
        if rp > lp {
            let clean_params: Vec<String> = sig[lp + 1..rp]
                .split(',')
                .map(|p| p.trim())
                .filter(|p| {
                    !p.is_empty()
                        && *p != "self"
                        && *p != "cls"
                        && *p != "&self"
                        && *p != "&mut self"
                        && !p.starts_with("self:")
                })
                .map(|p| {
                    // Keep concise parameter name/type
                    p.split('=').next().unwrap_or(p).trim().to_string()
                })
                .collect();
            format!("{}({})", qualified, clean_params.join(", "))
        } else {
            format!("{}()", qualified)
        }
    } else {
        format!("{}()", qualified)
    };

    (doc, usage)
}

/// Extract a method identifier from an indented class/impl body line (Python, Rust, JS/TS, Java, C#, Omni).
fn extract_class_method_ident(trimmed: &str) -> Option<&str> {
    let rest_opt = trimmed
        .strip_prefix("pub async fn ")
        .or_else(|| trimmed.strip_prefix("pub fn "))
        .or_else(|| trimmed.strip_prefix("async fn "))
        .or_else(|| trimmed.strip_prefix("fn "))
        .or_else(|| trimmed.strip_prefix("async def "))
        .or_else(|| trimmed.strip_prefix("def "))
        .or_else(|| trimmed.strip_prefix("func "));

    if let Some(rest) = rest_opt {
        let ident = rest
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if !ident.is_empty() && !ident.starts_with('_') {
            return Some(ident);
        }
        return None;
    }

    // Java / C# / TS / JS class methods: e.g. `public String encode(int x) {` or `static parse(s) {` or `compute(a, b) {`
    if let Some(lp) = trimmed.find('(') {
        let before = trimmed[..lp].trim();
        if before.is_empty()
            || before.contains('=')
            || before.contains('.')
            || before.contains("->")
        {
            return None;
        }
        let tokens: Vec<&str> = before.split_whitespace().collect();
        let ident = *tokens.last()?;
        const SKIP_KW: &[&str] = &[
            "if", "for", "while", "switch", "catch", "return", "new", "throw", "else", "match",
            "constructor", "super", "this", "typeof", "sizeof", "print", "println",
        ];
        if ident.is_empty()
            || ident.starts_with('_')
            || SKIP_KW.contains(&ident)
            || !ident.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
            || !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        // Require either a method body brace `{`, modifier prefix, or TS return type `:`
        let has_modifier = tokens.len() >= 2
            && matches!(
                tokens[0],
                "public"
                    | "protected"
                    | "static"
                    | "async"
                    | "final"
                    | "virtual"
                    | "override"
                    | "synchronized"
                    | "get"
                    | "set"
            );
        let looks_like_method = trimmed.ends_with('{') || trimmed.contains("):") || has_modifier;
        if looks_like_method {
            return Some(ident);
        }
    }
    None
}

/// Fast (~1-2 ms) 2-hop symbol sniper over files discovered by `DependencyDriller` (or local `.omni` imports).
/// - Hop 1: Top-level functions & classes/structs (`prefix.fn`, `prefix.Class`)
/// - Hop 2 (Max): Supporting methods inside classes/structs/`impl` blocks (`prefix.Class.method`),
///   culled at max 10 classes/file and max 15 methods/class so giant classes never bloat IDE completions.
/// - Extracts surrounding doc-comments / Python docstrings and concise usage snippets per definition.
/// - Also returns discovered foreign/local class definitions for `types` so `let x = prefix.Class(); x.` works.
fn snipe_spine_exports(
    ecosystem: &str,
    module_name: &str,
    prefix: &str,
    files: &[PathBuf],
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    const MAX_CLASSES_PER_FILE: usize = 10;
    const MAX_METHODS_PER_CLASS: usize = 15;
    const MAX_SPINE_SYMS: usize = 60;

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut class_records: Vec<(String, String, usize, Option<String>, String, Vec<serde_json::Value>)> =
        Vec::new();
    let mut class_index_map: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for path in files.iter().take(4) {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let file_str = path
            .canonicalize()
            .unwrap_or_else(|_| path.clone())
            .display()
            .to_string();
        let is_python = ecosystem.eq_ignore_ascii_case("python")
            || path.extension().and_then(|e| e.to_str()) == Some("py");

        let lines: Vec<&str> = content.lines().take(350).collect();
        // Track active class/impl scope for 2-hop method sniping:
        // (class_name, class_indent, is_brace_scoped, start_brace_depth, methods_sniped)
        let mut active_class: Option<(String, usize, bool, i32, usize)> = None;
        let mut brace_depth: i32 = 0;
        let mut classes_in_file: usize = 0;

        for (idx, raw_line) in lines.iter().enumerate() {
            let trimmed = raw_line.trim();
            let open_braces = trimmed.chars().filter(|&c| c == '{').count() as i32;
            let close_braces = trimmed.chars().filter(|&c| c == '}').count() as i32;
            let indent = raw_line.len() - raw_line.trim_start().len();

            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                brace_depth += open_braces - close_braces;
                continue;
            }

            // Close active class scope if indentation dropped (Python) or closing brace returned to outer depth
            if let Some((_, cls_indent, is_brace, start_depth, _)) = &active_class {
                if *is_brace {
                    if brace_depth <= *start_depth && trimmed.starts_with('}') {
                        active_class = None;
                    }
                } else if indent <= *cls_indent {
                    active_class = None;
                }
            }

            // 1. Check for Hop-2 supporting methods inside an active class/impl block
            if let Some((ref cls_name, cls_indent, is_brace, start_depth, ref mut mcount)) =
                active_class
            {
                let in_body = if is_brace {
                    brace_depth >= start_depth && indent > cls_indent && indent <= cls_indent + 8
                } else {
                    indent > cls_indent && indent <= cls_indent + 8
                };
                if in_body && *mcount < MAX_METHODS_PER_CLASS && out.len() < MAX_SPINE_SYMS {
                    if let Some(meth_name) = extract_class_method_ident(trimmed) {
                        let qualified = format!("{}.{}.{}", prefix, cls_name, meth_name);
                        if seen.insert(qualified.clone()) {
                            let sig: String = trimmed
                                .trim_end_matches('{')
                                .trim_end_matches(':')
                                .trim()
                                .chars()
                                .take(110)
                                .collect();
                            let (doc, usage) = extract_doc_and_usage(
                                &lines,
                                idx,
                                is_python || trimmed.ends_with(':'),
                                &qualified,
                                "spine_method",
                                &sig,
                            );
                            out.push(serde_json::json!({
                                "name": meth_name,
                                "qualified": qualified,
                                "parent": cls_name,
                                "hop": 2,
                                "module": module_name,
                                "ecosystem": ecosystem,
                                "kind": "spine_method",
                                "signature": sig,
                                "doc": doc,
                                "usage": usage,
                                "file": file_str,
                                "line": idx + 1,
                            }));
                            if let Some(&cidx) = class_index_map.get(cls_name) {
                                class_records[cidx].5.push(serde_json::json!({
                                    "name": meth_name,
                                    "signature": sig,
                                    "llvm_ir": format!("; [{} spine] {}.{}", ecosystem, cls_name, meth_name),
                                    "doc": doc,
                                    "usage": usage,
                                    "line": idx + 1,
                                }));
                            }
                            *mcount += 1;
                        }
                    }
                }
                brace_depth += open_braces - close_braces;
                if is_brace && brace_depth <= start_depth {
                    active_class = None;
                }
                continue;
            }

            // 2. Top-level Hop-1 exports (`indent <= 4`)
            if indent > 4 {
                brace_depth += open_braces - close_braces;
                continue;
            }

            // Rust `impl ClassName {` attaches Hop-2 methods to `ClassName`
            if let Some(rest) = trimmed.strip_prefix("impl ") {
                let target_cls = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .find(|s| !s.is_empty())
                    .unwrap_or("");
                if !target_cls.is_empty() && classes_in_file < MAX_CLASSES_PER_FILE {
                    let start_d = brace_depth;
                    brace_depth += open_braces - close_braces;
                    active_class = Some((target_cls.to_string(), indent, true, start_d, 0));
                    if !class_index_map.contains_key(target_cls) {
                        class_index_map.insert(target_cls.to_string(), class_records.len());
                        class_records.push((
                            target_cls.to_string(),
                            file_str.clone(),
                            idx + 1,
                            None,
                            format!("let obj = {}.{}()", prefix, target_cls),
                            Vec::new(),
                        ));
                    }
                    continue;
                }
            }

            let extracted: Option<(&str, &str)> = if let Some(rest) = trimmed
                .strip_prefix("pub async fn ")
                .or_else(|| trimmed.strip_prefix("pub fn "))
                .or_else(|| trimmed.strip_prefix("async fn "))
                .or_else(|| trimmed.strip_prefix("fn "))
                .or_else(|| trimmed.strip_prefix("async def "))
                .or_else(|| trimmed.strip_prefix("def "))
                .or_else(|| trimmed.strip_prefix("func "))
                .or_else(|| trimmed.strip_prefix("export async function "))
                .or_else(|| trimmed.strip_prefix("export function "))
                .or_else(|| trimmed.strip_prefix("function "))
            {
                let ident = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !ident.is_empty() && !ident.starts_with('_') {
                    Some((ident, "spine_fn"))
                } else {
                    None
                }
            } else if let Some(rest) = trimmed
                .strip_prefix("pub struct ")
                .or_else(|| trimmed.strip_prefix("struct "))
                .or_else(|| trimmed.strip_prefix("export default class "))
                .or_else(|| trimmed.strip_prefix("export class "))
                .or_else(|| trimmed.strip_prefix("public final class "))
                .or_else(|| trimmed.strip_prefix("public abstract class "))
                .or_else(|| trimmed.strip_prefix("public class "))
                .or_else(|| trimmed.strip_prefix("class "))
            {
                let ident = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !ident.is_empty() && !ident.starts_with('_') {
                    Some((ident, "spine_class"))
                } else {
                    None
                }
            } else if let Some(rest) = trimmed.strip_prefix("exports.") {
                let ident = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("");
                if !ident.is_empty() && !ident.starts_with('_') {
                    Some((ident, "spine_fn"))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((sym_name, kind)) = extracted {
                let qualified = format!("{}.{}", prefix, sym_name);
                let sig: String = trimmed
                    .trim_end_matches('{')
                    .trim_end_matches(':')
                    .trim()
                    .chars()
                    .take(110)
                    .collect();
                let (doc, usage) = extract_doc_and_usage(
                    &lines,
                    idx,
                    is_python || trimmed.ends_with(':'),
                    &qualified,
                    kind,
                    &sig,
                );
                if out.len() < MAX_SPINE_SYMS && seen.insert(qualified.clone()) {
                    out.push(serde_json::json!({
                        "name": sym_name,
                        "qualified": qualified,
                        "hop": 1,
                        "module": module_name,
                        "ecosystem": ecosystem,
                        "kind": kind,
                        "signature": sig,
                        "doc": doc,
                        "usage": usage,
                        "file": file_str,
                        "line": idx + 1,
                    }));
                }
                if kind == "spine_class" && classes_in_file < MAX_CLASSES_PER_FILE {
                    classes_in_file += 1;
                    let is_brace = trimmed.contains('{');
                    let start_d = brace_depth;
                    if !class_index_map.contains_key(sym_name) {
                        class_index_map.insert(sym_name.to_string(), class_records.len());
                        class_records.push((
                            sym_name.to_string(),
                            file_str.clone(),
                            idx + 1,
                            doc,
                            usage,
                            Vec::new(),
                        ));
                    }
                    active_class = Some((sym_name.to_string(), indent, is_brace, start_d, 0));
                }
            }

            brace_depth += open_braces - close_braces;
        }
    }

    let discovered_types: Vec<serde_json::Value> = class_records
        .into_iter()
        .filter(|(_, _, _, _, _, methods)| !methods.is_empty())
        .map(|(name, file, line, doc, usage, methods)| {
            serde_json::json!({
                "name": name,
                "fields": [],
                "methods": methods,
                "doc": doc,
                "usage": usage,
                "file": file,
                "line": line,
            })
        })
        .collect();

    (out, discovered_types)
}

#[cfg(test)]
mod repl_tests {
    use super::*;

    #[test]
    fn test_repl_multiline_detection_and_source_builder() {
        assert!(repl_needs_more_lines("fn add(a: int, b: int) -> int {"));
        assert!(!repl_needs_more_lines("fn add(a: int, b: int) -> int {\n    return a + b\n}"));
        assert!(repl_needs_more_lines("def sq(x):"));
        assert!(repl_needs_more_lines("def sq(x):\n    return x * x"));
        assert!(!repl_needs_more_lines("def sq(x):\n    return x * x\n"));
        assert!(!repl_needs_more_lines("let s = \"unclosed { in string\""));
        assert!(!repl_needs_more_lines(":help"));

        let src = build_repl_source(&[
            "fn double(x: int) -> int {\n    return x * 2\n}".to_string(),
            "let a = double(21)".to_string(),
            "print(a)".to_string(),
        ]);
        assert!(src.starts_with("fn double(x: int) -> int {"));
        assert!(src.contains("def main():\n    let a = double(21)\n    print(a)\n    return 0\n"));

        let colored = highlight_omni_line("fn foo(x: int) -> str { return \"ok\" } // hi");
        assert!(colored.contains("\x1b[1;35mfn\x1b[0m"));
        assert!(colored.contains("\x1b[36mint\x1b[0m"));
        assert!(colored.contains("\x1b[32m\"ok\"\x1b[0m"));
        assert!(colored.contains("\x1b[90m// hi\x1b[0m"));
    }
}
