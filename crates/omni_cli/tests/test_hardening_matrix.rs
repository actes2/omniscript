use omni_parser::OmniParser;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_compile_and_execute_comment_torture() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let target_file = repo_root.join("examples/hardening/comment_torture.omni");

    let omnic_bin = env!("CARGO_BIN_EXE_omnic");
    let output = Command::new(omnic_bin)
        .arg("--no-cache")
        .arg("run")
        .arg(&target_file)
        .current_dir(repo_root)
        .output()
        .expect("Failed to execute omnic on comment_torture.omni");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        output.status.success(),
        "comment_torture.omni failed!\nExit: {:?}\nOutput:\n{}",
        output.status.code(),
        combined
    );
    assert!(
        combined.contains("Vault total: 197"),
        "Expected 'Vault total: 197' in output:\n{}",
        combined
    );
    assert!(
        combined.contains("markers: # // /* */ -- ;; %% <!-- -->"),
        "Expected literal comment markers preserved inside string:\n{}",
        combined
    );
}

fn ensure_hardening_oenv(repo_root: &std::path::Path, omnic_bin: &str) {
    let oenv_dir = repo_root.join("examples/hardening/oenv");
    if !oenv_dir.join("packages/python/varint-1.0.2").exists()
        || !oenv_dir.join("packages/node/crc-32-1.2.2").exists()
        || !oenv_dir.join("packages/rust/adler-1.0.2").exists()
        || !oenv_dir.join("packages/java/lombok-1.18.36").exists()
        || !oenv_dir.join("git/go/github.com/cespare/xxhash@v2.3.0").exists()
    {
        let reqs = repo_root.join("examples/hardening/omni.requirements");
        let status = Command::new(omnic_bin)
            .arg("install")
            .arg("--oenv")
            .arg(&oenv_dir)
            .arg(&reqs)
            .current_dir(repo_root)
            .status()
            .expect("Failed to execute omnic install for hardening oenv");
        assert!(status.success(), "omnic install failed for hardening oenv");
    }
}

#[test]
fn test_compile_and_execute_polyglot_hardening_suite() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let omnic_bin = env!("CARGO_BIN_EXE_omnic");

    ensure_hardening_oenv(repo_root, omnic_bin);

    let cases: &[(&str, &[&str])] = &[
        (
            "examples/hardening/frankensyntax_codec.omni",
            &[
                "FrankenSyntax core: 1265",
                "polyglot deps resolved: varint + crc32 + zlib",
                "crc32 non-zero ok",
            ],
        ),
        (
            "examples/hardening/babel_mesh_pipeline.omni",
            &[
                "Babel mesh total: 169",
                "go+rust packages linked",
            ],
        ),
        (
            "examples/hardening/gauntlet_oop_exceptions.omni",
            &["Gauntlet total: 110"],
        ),
        (
            "examples/hardening/backbone_64bit_float_unwind.omni",
            &[
                "i64_big: 11794967296",
                "ke_str: 32.5",
                "caught: neg_frame",
                "Backbone score: 400",
            ],
        ),
        (
            "examples/hardening/pratt_types_llm_expr.omni",
            &[
                "pow_assoc: 512",
                "f_sum: 7.0",
                "m_sum: 6.0",
                "scaled: 10.0",
                "llm_mid: 15",
                "Pratt+Types+LLM score: 798",
            ],
        ),
        (
            "examples/hardening/magnum_opus_chimera.omni",
            &[
                "epoch64: 13794967296",
                "float_total: 29.5",
                "caught: chimera_breach",
                "ai_mid: 121",
                "6 ecosystems linked: python+node+rust+go+c+csharp",
                "Magnum Opus Chimera score: 2174",
            ],
        ),
        (
            "examples/hardening/polyglot_lombok_sdl2_nexus.omni",
            &[
                "canvas_title: OmniScript :: Lombok x Python x SDL2 Polyglot Nexus",
                "stage1_lombok_py: 1811",
                "stage2_go_sum: 818",
                "frame_digest: 13520",
                "frames_processed: 48",
                "7 ecosystems linked: java+c+python+node+rust+go+csharp",
                "Polyglot Lombok SDL2 Nexus score: 6594",
            ],
        ),
    ];

    for (rel_path, expected_lines) in cases {
        let target_file = repo_root.join(rel_path);
        let bin_out = std::env::temp_dir().join(format!(
            "omni_hardening_{}",
            target_file.file_stem().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_file(&bin_out);

        let build_out = Command::new(omnic_bin)
            .arg("build")
            .arg(&target_file)
            .arg("--no-cache")
            .arg("-o")
            .arg(&bin_out)
            .current_dir(repo_root)
            .output()
            .unwrap_or_else(|e| panic!("Failed to invoke omnic build on {}: {}", rel_path, e));

        assert!(
            build_out.status.success(),
            "Build failed for {}!\nStdout:\n{}\nStderr:\n{}",
            rel_path,
            String::from_utf8_lossy(&build_out.stdout),
            String::from_utf8_lossy(&build_out.stderr)
        );

        let run_out = Command::new(&bin_out)
            .current_dir(repo_root)
            .output()
            .unwrap_or_else(|e| panic!("Failed to execute compiled binary for {}: {}", rel_path, e));
        let _ = fs::remove_file(&bin_out);

        let stdout = String::from_utf8_lossy(&run_out.stdout);
        let stderr = String::from_utf8_lossy(&run_out.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        assert!(
            run_out.status.success(),
            "Execution failed for {} (exit {:?})!\nOutput:\n{}",
            rel_path,
            run_out.status.code(),
            combined
        );

        for expected in *expected_lines {
            assert!(
                combined.contains(expected),
                "Missing expected '{}' in {} output:\n{}",
                expected,
                rel_path,
                combined
            );
        }
    }
}

#[test]
fn test_debug_hardening_files() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let root = if manifest_dir.ends_with("omni_cli") {
        manifest_dir.parent().unwrap().parent().unwrap().to_path_buf()
    } else {
        manifest_dir
    };

    let p = OmniParser::new();
    let enc_path = root.join("examples/hardening/encryptchat_python.omni");
    let enc_src = fs::read_to_string(&enc_path).unwrap();
    let enc_mod = p.parse_module("encryptchat", &enc_src);
    println!("=== ENCRYPTCHAT ===");
    println!("Imports ({}):", enc_mod.imports.len());
    for imp in &enc_mod.imports {
        println!("  import: {:?}", imp);
    }
    println!("Functions ({}):", enc_mod.functions.len());
    for f in &enc_mod.functions {
        println!("  fn {}(params: {:?}) -> {:?}", f.name, f.params, f.return_type);
        println!("     body statements ({}):", f.body.len());
        for st in &f.body {
            println!("       st: {:?}", st);
        }
    }
    println!("Structs/Classes ({}):", enc_mod.structs_and_classes.len());

    let key_path = root.join("examples/hardening/oenv/packages/python/rsa-4.9.1/rsa/key.py");
    if let Ok(key_src) = fs::read_to_string(&key_path) {
        let raw_lines: Vec<&str> = key_src.lines().collect();
        println!("Total lines in key.py: {}", raw_lines.len());
        for (idx, line) in raw_lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with("class ") {
                println!("Line {}: class header: {}", idx + 1, t);
                let hdr = OmniParser::parse_class_header(t);
                println!("  parse_class_header -> {:?}", hdr);
            }
        }
        let key_mod = p.parse_module("rsa_key", &key_src);
        println!("=== RSA KEY.PY ===");
        println!("Key classes ({}):", key_mod.structs_and_classes.len());
        for c in &key_mod.structs_and_classes {
            println!("  class {}: fields: {:?}, methods: {:?}", c.name, c.fields, c.methods.iter().map(|m| &m.name).collect::<Vec<_>>());
        }
        println!("Key functions ({}):", key_mod.functions.len());
        for f in &key_mod.functions {
            println!("  fn {}", f.name);
        }
    }

    let ll_path = root.join("examples/hardening/linkedlist_hell.omni");
    let ll_src = fs::read_to_string(&ll_path).unwrap();
    let canon = OmniParser::canonicalize_source(&ll_src);
    println!("\n=== CANONICALIZED LINKEDLIST ===");
    for (i, line) in canon.lines().enumerate().take(55) {
        println!("{:2}: {}", i + 1, line);
    }
    let ll_mod = p.parse_module("linkedlist", &ll_src);
    println!("\n=== LINKEDLIST HELL ===");
    println!("Imports ({}):", ll_mod.imports.len());
    for imp in &ll_mod.imports {
        println!("  import: {:?}", imp);
    }
    println!("Functions ({}):", ll_mod.functions.len());
    for f in &ll_mod.functions {
        println!("  fn {}(params: {:?}) -> {:?}", f.name, f.params, f.return_type);
        println!("     body statements ({}):", f.body.len());
        for st in &f.body {
            println!("       st: {:?}", st);
        }
    }
    println!("Structs/Classes ({}):", ll_mod.structs_and_classes.len());
    for s in &ll_mod.structs_and_classes {
        println!("  struct/class {}: fields: {:?}, methods: {:?}", s.name, s.fields, s.methods.len());
        for m in &s.methods {
            println!("    method {}(params: {:?}) -> {:?}", m.name, m.params, m.return_type);
            println!("      body statements ({}):", m.body.len());
            for st in &m.body {
                println!("        st: {:?}", st);
            }
        }
    }
}
