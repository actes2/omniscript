use omni_parser::OmniParser;
use std::fs;
use std::path::PathBuf;

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
