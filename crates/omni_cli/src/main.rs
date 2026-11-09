use clap::{Parser as ClapParser, Subcommand};
use omni_codegen::CodeGenerator;
use omni_engine::{CandleGgufEngine, FallbackEngine, MockSynthesisEngine, OllamaInferenceEngine, VerifiedSynthesisEngine};
use omni_parser::OmniParser;
use omni_preamble::{DependencyDriller, ImportDirective, PackageShimmer, SymbolCarver};
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

    /// Target execution device for AI intent lowering: "auto" (default), "cuda", or "cpu"
    #[arg(long, global = true, default_value = "auto")]
    device: String,
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
    /// Install dependencies declared in omni-requirements.txt or custom manifest
    Install {
        /// Manifest path (defaults to omni-requirements.txt)
        #[arg(default_value = "omni-requirements.txt")]
        manifest: PathBuf,
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
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        Some(Commands::Repl) => run_repl()?,
        Some(Commands::Install { manifest }) => {
            install_dependencies(&manifest, args.verbose)?;
        }
        Some(Commands::SynthDataset { count, output }) => {
            synth_dataset(count, &output)?;
        }
        Some(Commands::Run { source, args: prog_args }) => {
            run_file(&source, &prog_args, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device)?;
        }
        Some(Commands::Build { source, output }) => {
            build_file(&source, &output, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device)?;
        }
        None => {
            if let Some(ref source) = args.source {
                build_file(source, &args.output, args.temp, args.opt_level, args.no_cache, &args.cache_dir, args.verbose, &args.device)?;
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

    println!("\n[SYNTHESIS COMPLETE]");
    println!("  -> Successfully synthesized and verified {} samples with Clang", written);
    println!("  -> Dataset saved to: {}", output.display());
    Ok(())
}

fn install_dependencies(manifest_path: &Path, verbose: bool) -> anyhow::Result<()> {
    if !manifest_path.exists() {
        anyhow::bail!("Requirements manifest '{}' not found.", manifest_path.display());
    }
    let shimmer = PackageShimmer::new();
    let specs = shimmer
        .parse_requirements_file(manifest_path)
        .map_err(|e| anyhow::anyhow!("Failed parsing requirements: {}", e))?;

    println!("OmniScript Package Shimmer: installing {} packages from {}...", specs.len(), manifest_path.display());
    let mut installed = 0;
    for spec in &specs {
        let desc = if spec.is_git {
            format!("[git]'{}' {}", spec.ecosystem.as_str(), spec.git_url.as_deref().unwrap_or(""))
        } else {
            format!("'{}' {} (version: {})", spec.ecosystem.as_str(), spec.name, spec.version.as_deref().unwrap_or("latest"))
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
    println!("Successfully synchronized {}/{} packages into ~/.omnicache/", installed, specs.len());
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

fn run_file(
    source_path: &Path,
    prog_args: &[String],
    temp: f32,
    opt_level: u8,
    no_cache: bool,
    cache_dir: &Path,
    verbose: bool,
    device: &str,
) -> anyhow::Result<()> {
    let (llvm_ir, foreign_c_files, stats) = compile_source(source_path, temp, opt_level, no_cache, cache_dir, verbose, device)?;
    let cache_str = if stats.from_cache { " [CACHE HIT]" } else { " [LIVE AI SYNTHESIS]" };
    println!(
        "[COMPILE] Ready in {:.2?} (Parse: {:.2?} | Codegen: {:.2?}{} | Verify: {:.2?})",
        stats.total_compile_time, stats.parse_time, stats.codegen_time, cache_str, stats.verify_time
    );
    let verifier = LlvmVerifier::new()?;
    let status = verifier.run_jit(&llvm_ir, opt_level, &foreign_c_files, prog_args)?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
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
) -> anyhow::Result<()> {
    let build_start = std::time::Instant::now();
    println!("==================================================");
    println!(" OmniScript Compiler (omnic v0.1.0)");
    println!(" Target Source: {}", source_path.display());
    println!(" Temp: {:.2} | Opt-Level: -O{} | Cache: {} | Device: {}", temp, opt_level, !no_cache, device);
    println!("==================================================");

    let (llvm_ir, foreign_c_files, stats) = compile_source(source_path, temp, opt_level, no_cache, cache_dir, verbose, device)?;
    let verifier = LlvmVerifier::new()?;

    println!("\n[5/5] Linking native ELF binary...");
    let link_start = std::time::Instant::now();
    verifier.compile_ir_to_binary(&llvm_ir, output, opt_level, &foreign_c_files)?;
    let link_time = link_start.elapsed();
    let total_build_time = build_start.elapsed();

    println!("  -> Successfully compiled and linked native binary: {}", output.display());
    let cache_str = if stats.from_cache { " [CACHE HIT]" } else { " [LIVE AI SYNTHESIS]" };
    println!(
        "\n[BUILD COMPLETE] Total: {:.2?} (Parse: {:.2?} | Drill: {:.2?} | Codegen: {:.2?}{} | Verify: {:.2?} | Link: {:.2?})",
        total_build_time, stats.parse_time, stats.drill_time, stats.codegen_time, cache_str, stats.verify_time, link_time
    );
    println!("Run binary with: ./{}", output.display());
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
) -> anyhow::Result<(String, Vec<PathBuf>, CompilationStats)> {
    let start_compile = std::time::Instant::now();
    if !source_path.exists() {
        eprintln!("[ERROR] Source file '{}' does not exist.", source_path.display());
        std::process::exit(1);
    }

    let source_content = fs::read_to_string(source_path)?;
    let module_name = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");

    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));

    // 1. Parse AST & Memory Instructions
    let t_parse_start = std::time::Instant::now();
    let parser = OmniParser::new();
    let mut module = parser.parse_module_with_dir(module_name, &source_content, Some(base_dir));
    let parse_time = t_parse_start.elapsed();

    // 2. Preamble Assembly & Symbol Carving
    let t_drill_start = std::time::Instant::now();
    let carver = SymbolCarver::new();
    let mut foreign_c_files = omni_runtime::runtime_c_sources();
    let mut all_dependency_files = Vec::new();

    for import in &module.imports {
        match import {
            ImportDirective::FileAlias { path, alias } => {
                let full_path = base_dir.join(path);
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
                if ext == "c" || ext == "cpp" {
                    let mut driller = DependencyDriller::new();
                    let slice = driller.drill(&full_path);
                    for src in slice.local_sources {
                        if !foreign_c_files.contains(&src) {
                            foreign_c_files.push(src);
                        }
                    }
                    if verbose && !slice.external_frontier.is_empty() {
                        println!("  -> External frontier dependencies detected: {:?}", slice.external_frontier);
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
                let mut driller = DependencyDriller::new();
                let walk = driller.crawl_ecosystem_import(
                    base_dir,
                    ecosystem,
                    mod_name,
                    &target_symbols,
                );
                for f in &walk.visited_files {
                    if !all_dependency_files.contains(f) {
                        all_dependency_files.push(f.clone());
                    }
                    let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if (ext == "c" || ext == "cpp" || ext == "so" || ext == "dylib" || ext == "a") && !foreign_c_files.contains(f) {
                        foreign_c_files.push(f.clone());
                    }
                }
                if let Some(ref orig) = walk.resolved_origin {
                    if !all_dependency_files.contains(orig) {
                        all_dependency_files.push(orig.clone());
                    }
                    let ext = orig.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if (ext == "c" || ext == "cpp" || ext == "so" || ext == "dylib" || ext == "a") && !foreign_c_files.contains(orig) {
                        foreign_c_files.push(orig.clone());
                    }
                }

                // Ingest pure-source crawled module items directly into AST
                let mut all_sources_to_merge = walk.visited_files.clone();
                if let Some(ref orig) = walk.resolved_origin {
                    if !all_sources_to_merge.contains(orig) {
                        all_sources_to_merge.push(orig.clone());
                    }
                }
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
                                let sub_mod = parser.parse_module(mod_name, &func_src);
                                for f in sub_mod.functions {
                                    if !module.functions.iter().any(|existing| existing.name == f.name) {
                                        module.functions.push(f);
                                    }
                                }
                            }
                        }
                    } else if ext == "rs" || ext == "py" || ext == "js" || ext == "go" || ext == "java" || ext == "cs" || ext == "lua" || ext == "c" {
                        if let Ok(src) = fs::read_to_string(vf) {
                            let sub_mod = parser.parse_module(mod_name, &src);
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
                    match carver.carve_symbol_from_dir(base_dir, dir_path, sym_name) {
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
    hasher.update(source_content.as_bytes());
    hasher.update(omni_codegen::host_target_triple().as_bytes());
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

    let (llvm_ir, from_cache) = if !no_cache && cache_file.exists() {
        (fs::read_to_string(&cache_file)?, true)
    } else {
        let verifier = LlvmVerifier::new()?;
        let fallback = MockSynthesisEngine::new();

        let pref = match device.to_lowercase().as_str() {
            "cuda" | "gpu" => {
                if !omni_engine::cuda_is_available() {
                    eprintln!("[ERROR] Explicit device '--device cuda' requested, but CUDA acceleration is not available or drivers/toolkit are missing.");
                    std::process::exit(1);
                }
                omni_engine::DevicePreference::Cuda(0)
            }
            "cpu" => omni_engine::DevicePreference::Cpu,
            _ => omni_engine::DevicePreference::Auto,
        };

        let generated = if let Some(candle_engine) = CandleGgufEngine::discover_default_with_device(pref) {
            let dev_tag = if candle_engine.is_cuda() { "CUDA GPU" } else { "CPU (AVX2/SIMD)" };
            if verbose {
                println!("  -> [Inference] Using in-process Candle GGUF engine on {} ({})", dev_tag, candle_engine.model_path.display());
            }
            let primary_verified = VerifiedSynthesisEngine::new(&candle_engine, &verifier);
            let codegen = CodeGenerator::new(&primary_verified);
            codegen.generate_module_ir(&module, temp)?
        } else {
            if verbose {
                println!("  -> [Inference] Fallback to Ollama service (omni-coder)");
            }
            let primary = OllamaInferenceEngine::new("omni-coder");
            let primary_verified = VerifiedSynthesisEngine::new(&primary, &verifier);
            let composite = FallbackEngine::new(primary_verified, fallback);
            let codegen = CodeGenerator::new(&composite);
            codegen.generate_module_ir(&module, temp)?
        };
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
        verifier.verify_ir(&llvm_ir)?;
        let _ = fs::create_dir_all(cache_dir);
        let tmp_cache = cache_dir.join(format!(".tmp_{}.ll", cache_hash));
        if fs::write(&tmp_cache, &llvm_ir).is_ok() {
            let _ = fs::rename(&tmp_cache, &cache_file);
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

fn run_repl() -> anyhow::Result<()> {
    println!("============================================================");
    println!(" OmniScript Interactive Polyglot REPL (omnic v0.1.0)");
    println!(" Type polyglot statements (Python, Go, C) or :help for commands");
    println!(" Zero-GC runtime | Live JIT Execution");
    println!("============================================================");

    let stdin = io::stdin();
    let mut session_stmts: Vec<String> = Vec::new();
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
        print!("omni> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match trimmed {
            ":exit" | ":quit" | "exit()" | "quit()" => {
                println!("Goodbye!");
                break;
            }
            ":help" => {
                println!("Available commands:");
                println!("  :ir       - Print synthesized LLVM IR for current session");
                println!("  :list     - List all statements in current session");
                println!("  :reset    - Clear current session memory and variables");
                println!("  :disambig - Toggle conversational disambiguation on ambiguous idioms");
                println!("  :quit     - Exit REPL");
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
                println!("Session cleared.");
                continue;
            }
            ":list" => {
                println!("--- Session Statements ---");
                for (idx, s) in session_stmts.iter().enumerate() {
                    println!("  [{}] {}", idx + 1, s);
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
        if (disambig_enabled || trimmed.ends_with('?')) && trimmed.contains(":=") {
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
        } else if trimmed.ends_with('?') {
            processed_line = trimmed.trim_end_matches('?').trim().to_string();
        }

        // If the statement is an expression without print or assignment, wrap it in print
        let stmt_to_add = if !processed_line.starts_with("print(")
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
                            if session_stmts.is_empty() {
                                if !stdout.is_empty() {
                                    print!("{}", stdout);
                                }
                            } else {
                                let prev_src = build_repl_source(&session_stmts);
                                let prev_mod = parser.parse_module("repl_prev", &prev_src);
                                if let Ok(prev_ir) = codegen.generate_module_ir(&prev_mod, 0.0) {
                                    if let Ok((_, prev_out, _)) = verifier.run_jit_captured(&prev_ir, 0, &runtime_c, &[]) {
                                        if stdout.starts_with(&prev_out) {
                                            print!("{}", &stdout[prev_out.len()..]);
                                        } else {
                                            print!("{}", stdout);
                                        }
                                    } else {
                                        print!("{}", stdout);
                                    }
                                } else {
                                    print!("{}", stdout);
                                }
                            }
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

fn build_repl_source(stmts: &[String]) -> String {
    let mut out = String::from("def main():\n");
    if stmts.is_empty() {
        out.push_str("    return 0\n");
    } else {
        for s in stmts {
            out.push_str(&format!("    {}\n", s));
        }
        out.push_str("    return 0\n");
    }
    out
}
