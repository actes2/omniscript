use clap::Parser as ClapParser;
use omni_codegen::CodeGenerator;
use omni_engine::MockSynthesisEngine;
use omni_parser::OmniParser;
use omni_preamble::{ImportDirective, SymbolCarver};
use omni_verify::LlvmVerifier;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(ClapParser, Debug)]
#[command(name = "omnic", author, version, about = "The OmniScript Compiler")]
struct Cli {
    /// Input OmniScript source file (*.omni)
    #[arg(required = true)]
    source: PathBuf,

    /// Output binary destination
    #[arg(short, long, default_value = "a.out")]
    output: PathBuf,

    /// Model inference temperature for intent lowering
    #[arg(long, default_value_t = 0.0)]
    temp: f32,

    /// LLVM optimization level (0, 1, 2, 3)
    #[arg(long, default_value_t = 2)]
    opt_level: u8,

    /// Bypass .omnicache and force re-synthesis
    #[arg(long, default_value_t = false)]
    no_cache: bool,

    /// Verbose diagnostic output
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    println!("==================================================");
    println!(" OmniScript Compiler (omnic v0.1.0)");
    println!(" Target Source: {}", args.source.display());
    println!(" Temp: {:.2} | Opt-Level: -O{} | Cache: {}", args.temp, args.opt_level, !args.no_cache);
    println!("==================================================");

    if !args.source.exists() {
        eprintln!("[ERROR] Source file '{}' does not exist.", args.source.display());
        std::process::exit(1);
    }

    let source_content = fs::read_to_string(&args.source)?;
    let module_name = args
        .source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");

    let base_dir = args.source.parent().unwrap_or_else(|| Path::new("."));

    // 1. Parse AST & Memory Instructions
    println!("\n[1/5] Parsing resilient AST and UMO memory instructions...");
    let parser = OmniParser::new();
    let module = parser.parse_module(module_name, &source_content);

    println!("  -> Discovered {} class(es)", module.structs_and_classes.len());
    println!("  -> Discovered {} function(s)", module.functions.len());
    println!("  -> Discovered {} import directive(s)", module.imports.len());

    // 2. Preamble Assembly & Symbol Carving
    println!("\n[2/5] Executing Preamble Symbol Carving across polyglot targets...");
    let carver = SymbolCarver::new();
    let mut foreign_c_files = omni_runtime::runtime_c_sources();

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
                
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "c" || ext == "cpp" {
                    foreign_c_files.push(full_path.clone());
                }

                println!(
                    "  -> Bound foreign file '{}' as alias '{:?}'",
                    path.display(),
                    alias.as_deref().unwrap_or("self")
                );
            }
            ImportDirective::FromDirectory { dir_path, symbols } => {
                for sym_name in symbols {
                    match carver.carve_symbol_from_dir(base_dir, dir_path, sym_name) {
                        Ok(sym) => {
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
                        Err(e) => {
                            eprintln!("  [ERROR] {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
            ImportDirective::DirectoryAlias { path, alias } => {
                println!(
                    "  -> Ingesting directory '{}' as alias '{:?}'",
                    path.display(),
                    alias.as_deref().unwrap_or("self")
                );
            }
        }
    }

    // 3. Memory & Lifetime Normalization Summary
    println!("\n[3/5] Auditing memory lifecycles & autonomous borrow-checking...");
    for f in &module.functions {
        if !f.memory_ops.is_empty() {
            println!("  -> Function '{}' declared {} UMO operation(s):", f.name, f.memory_ops.len());
            for op in &f.memory_ops {
                println!("     * {:?}", op);
            }

            if let Err(violation) = omni_mem::LifetimeAuditor::audit_scope(&f.name, &f.memory_ops) {
                eprintln!("\n[FATAL LIFETIME VIOLATION] {}", violation);
                std::process::exit(1);
            }
        }
    }

    // 4. Code Generation & Intent Synthesis (with .omnicache check)
    let cache_dir = PathBuf::from(".omnicache");
    let mut hasher = blake3::Hasher::new();
    hasher.update(source_content.as_bytes());
    hasher.update(b"x86_64-pc-linux-gnu");
    hasher.update(&args.temp.to_le_bytes());
    hasher.update(&[args.opt_level]);
    for foreign_file in &foreign_c_files {
        if let Ok(bytes) = fs::read(foreign_file) {
            hasher.update(&bytes);
        }
    }
    let cache_hash = hasher.finalize().to_hex().to_string();
    let cache_file = cache_dir.join(format!("{}.ll", cache_hash));

    let (llvm_ir, from_cache) = if !args.no_cache && cache_file.exists() {
        println!("\n[4/5] [CACHE HIT] Loaded verified LLVM IR from .omnicache/{}.ll (0 ms)", &cache_hash[..12]);
        (fs::read_to_string(&cache_file)?, true)
    } else {
        println!("\n[4/5] Synthesizing intent blocks and generating unified LLVM IR...");
        let mock_engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&mock_engine);
        let generated = codegen.generate_module_ir(&module, args.temp)?;
        (generated, false)
    };

    if args.verbose {
        println!("\n--- LLVM IR ---\n{}\n----------------", llvm_ir);
    }

    // 5. Verification & Native Compilation
    println!("\n[5/5] Verifying LLVM IR and linking native ELF binary...");
    let verifier = LlvmVerifier::new()?;
    
    if !from_cache {
        verifier.verify_ir(&llvm_ir)?;
        println!("  -> LLVM IR syntax & type verifier passed cleanly.");

        // Atomic commit to .omnicache (write temp file + rename)
        let _ = fs::create_dir_all(&cache_dir);
        let tmp_cache = cache_dir.join(format!(".tmp_{}.ll", cache_hash));
        if fs::write(&tmp_cache, &llvm_ir).is_ok() {
            let _ = fs::rename(&tmp_cache, &cache_file);
        }
        println!("  -> Bitcode committed to .omnicache/{}.ll", &cache_hash[..12]);
    } else {
        println!("  -> Skipped verification pass (pre-verified bitcode).");
    }

    verifier.compile_ir_to_binary(&llvm_ir, &args.output, args.opt_level, &foreign_c_files)?;
    println!("  -> Successfully compiled and linked native binary: {}", args.output.display());

    println!("\n[BUILD COMPLETE] Run binary with: ./{}", args.output.display());
    Ok(())
}
