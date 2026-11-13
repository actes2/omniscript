use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static IR_INGEST_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct CallerCallSiteSig {
    pub ret_ty: String,        // e.g. `"i64"` or `"i32"`
    pub param_tys: Vec<String>, // e.g. `["i64", "i64", "i64", "i64"]`
    pub is_unsigned_ret: bool,
}

#[derive(Debug)]
pub struct IrSliceResult {
    pub ir_code: String,
    pub resolved_symbols: HashSet<String>,
    pub needed_decls: BTreeSet<String>,
}

pub fn find_clang_executable() -> Option<PathBuf> {
    if let Some(custom) =
        std::env::var_os("OMNI_CLANG_PATH").or_else(|| std::env::var_os("CLANG_PATH"))
    {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }
    let candidates = [
        PathBuf::from("/usr/lib/llvm21/bin/clang"),
        PathBuf::from("/usr/lib/llvm20/bin/clang"),
        PathBuf::from("/usr/lib/llvm19/bin/clang"),
        PathBuf::from("/usr/lib/llvm18/bin/clang"),
        PathBuf::from("/opt/homebrew/opt/llvm/bin/clang"),
        PathBuf::from("/opt/homebrew/bin/clang"),
        PathBuf::from("/usr/local/opt/llvm/bin/clang"),
        PathBuf::from("/opt/local/bin/clang"),
        PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/clang"),
        PathBuf::from("/usr/bin/clang"),
        PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe"),
        PathBuf::from(r"C:\LLVM\bin\clang.exe"),
        PathBuf::from(r"C:\msys64\ucrt64\bin\clang.exe"),
        PathBuf::from(r"C:\msys64\clang64\bin\clang.exe"),
        PathBuf::from(r"C:\msys64\mingw64\bin\clang.exe"),
        PathBuf::from(
            r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin\clang.exe",
        ),
        PathBuf::from(
            r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\Llvm\x64\bin\clang.exe",
        ),
        PathBuf::from(
            r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\Llvm\x64\bin\clang.exe",
        ),
        PathBuf::from(
            r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin\clang.exe",
        ),
    ];
    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    for bare in ["clang", "clang.exe"] {
        if let Ok(out) = Command::new(bare).arg("--version").output() {
            if out.status.success() {
                return Some(PathBuf::from(bare));
            }
        }
    }
    None
}

/// Disassemble raw LLVM bitcode (`.bc` or embedded `.llvmbc`) into textual `.ll` via `clang -S -emit-llvm -x ir`.
pub fn bitcode_bytes_to_ll_text(bc_bytes: &[u8], target_triple: &str) -> Result<String, String> {
    let clang = find_clang_executable().ok_or("Clang compiler not found on host")?;
    let seq = IR_INGEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let in_bc = tmp_dir.join(format!("omni_in_{}_{}.bc", std::process::id(), seq));
    let out_ll = tmp_dir.join(format!("omni_out_{}_{}.ll", std::process::id(), seq));
    fs::write(&in_bc, bc_bytes).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&clang);
    if !target_triple.trim().is_empty() {
        cmd.arg(format!("--target={}", target_triple.trim()));
    }
    let output = cmd
        .args(["-Wno-override-module", "-S", "-emit-llvm", "-x", "ir"])
        .arg(&in_bc)
        .arg("-o")
        .arg(&out_ll)
        .output()
        .map_err(|e| e.to_string());
    let _ = fs::remove_file(&in_bc);

    let output = output?;
    if !output.status.success() {
        let _ = fs::remove_file(&out_ll);
        return Err(format!(
            "clang bitcode disassembly failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let ll = fs::read_to_string(&out_ll).map_err(|e| e.to_string());
    let _ = fs::remove_file(&out_ll);
    ll
}

/// Lower `.c` / `.cc` / `.cpp` source directly to textual LLVM IR via `clang -S -emit-llvm -O2`.
pub fn compile_c_source_to_ll_text(
    src_path: &Path,
    include_dirs: &[PathBuf],
    target_triple: &str,
) -> Result<String, String> {
    let clang = find_clang_executable().ok_or("Clang compiler not found on host")?;
    let seq = IR_INGEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_ll = std::env::temp_dir().join(format!("omni_csrc_{}_{}.ll", std::process::id(), seq));

    let mut cmd = Command::new(&clang);
    if !target_triple.trim().is_empty() {
        cmd.arg(format!("--target={}", target_triple.trim()));
    }
    cmd.args([
        "-S",
        "-emit-llvm",
        "-O2",
        "-fno-strict-aliasing",
        "-Wno-everything",
    ]);
    for inc in include_dirs {
        if !inc.as_os_str().is_empty() && inc.is_dir() {
            cmd.arg("-I").arg(inc);
        }
    }
    let abs_src = fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
    if let Some(parent) = abs_src.parent() {
        if !parent.as_os_str().is_empty() {
            cmd.arg("-I").arg(parent);
        }
    }
    let output = cmd
        .arg(&abs_src)
        .arg("-o")
        .arg(&out_ll)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let _ = fs::remove_file(&out_ll);
        return Err(format!(
            "clang C->IR lowering failed for {}: {}",
            src_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let ll = fs::read_to_string(&out_ll).map_err(|e| e.to_string());
    let _ = fs::remove_file(&out_ll);
    ll
}

/// Detect and lower `static inline` / `inline` C function definitions inside a `.h` header file!
pub fn compile_header_inline_defs_to_ll_text(
    hdr_path: &Path,
    requested_symbols: &[String],
    include_dirs: &[PathBuf],
    target_triple: &str,
) -> Result<Option<(String, HashMap<String, String>)>, String> {
    if requested_symbols.is_empty() {
        return Ok(None);
    }
    let Ok(hdr_text) = fs::read_to_string(hdr_path) else {
        return Ok(None);
    };

    // Check which `requested_symbols` actually have an inline function body `{` in `hdr_text`
    let mut inline_hits: Vec<(String, String, Vec<(String, String)>)> = Vec::new();
    for sym in requested_symbols {
        let pat = format!(
            r"(?m)^\s*((?:static\s+|inline\s+|__inline__\s+|__inline\s+|const\s+|unsigned\s+|signed\s+|struct\s+[A-Za-z0-9_]+\s*|[A-Za-z0-9_*]+\s+)+)\b{}\s*\(([^)]*)\)\s*\{{",
            regex::escape(sym)
        );
        let Ok(re) = Regex::new(&pat) else {
            continue;
        };
        if let Some(caps) = re.captures(&hdr_text) {
            let raw_prefix = caps[1].trim();
            if !(raw_prefix.contains("inline") || raw_prefix.contains("static")) {
                continue;
            }
            let ret_c = raw_prefix
                .replace("static", "")
                .replace("__inline__", "")
                .replace("__inline", "")
                .replace("inline", "")
                .trim()
                .to_string();
            let raw_params = caps[2].trim();
            let mut parsed_params = Vec::new();
            if !raw_params.is_empty() && raw_params != "void" {
                for (idx, p) in raw_params.split(',').enumerate() {
                    let pt = p.trim();
                    // Split last identifier from C parameter declaration (`const char *src` -> (`const char *`, `a0`))
                    let name_end = pt
                        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let c_ty = if name_end > 0 && name_end < pt.len() {
                        pt[..name_end].trim().to_string()
                    } else {
                        pt.to_string()
                    };
                    parsed_params.push((c_ty, format!("__p{}", idx)));
                }
            }
            inline_hits.push((sym.clone(), ret_c, parsed_params));
        }
    }

    if inline_hits.is_empty() {
        return Ok(None);
    }

    // Synthesize a temporary C TU exporting non-inline wrappers `__omni_hinl_<sym>`
    let abs_hdr = fs::canonicalize(hdr_path).unwrap_or_else(|_| hdr_path.to_path_buf());
    let seq = IR_INGEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_c = std::env::temp_dir().join(format!("omni_hinl_{}_{}.c", std::process::id(), seq));
    let mut c_tu = String::new();
    let _ = writeln!(
        c_tu,
        "#include \"{}\"\n",
        abs_hdr.to_string_lossy().replace('\\', "/")
    );
    let mut alias_map = HashMap::new();

    for (sym, ret_c, params) in &inline_hits {
        let export_nm = format!("__omni_hinl_{}", sym);
        alias_map.insert(sym.clone(), export_nm.clone());
        let sig_params = if params.is_empty() {
            "void".to_string()
        } else {
            params
                .iter()
                .map(|(t, n)| format!("{} {}", t, n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let call_args = params
            .iter()
            .map(|(_, n)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if ret_c == "void" {
            let _ = writeln!(
                c_tu,
                "void {}({}) {{ {}({}); }}",
                export_nm, sig_params, sym, call_args
            );
        } else {
            let _ = writeln!(
                c_tu,
                "{} {}({}) {{ return {}({}); }}",
                ret_c, export_nm, sig_params, sym, call_args
            );
        }
    }

    fs::write(&tmp_c, &c_tu).map_err(|e| e.to_string())?;
    let mut all_incs = include_dirs.to_vec();
    if let Some(p) = abs_hdr.parent() {
        if !p.as_os_str().is_empty() && !all_incs.contains(&p.to_path_buf()) {
            all_incs.push(p.to_path_buf());
        }
    }
    let res = compile_c_source_to_ll_text(&tmp_c, &all_incs, target_triple);
    let _ = fs::remove_file(&tmp_c);
    let ll = res?;
    Ok(Some((ll, alias_map)))
}

/// Slice reachable definitions, globals, and types from textual `.ll` IR, namespace internal symbols,
/// and emit exact call-site adapters matching `omni_ir`!
pub fn slice_and_adapt_ll_module(
    raw_ll: &str,
    mod_id: &str,
    requested_symbols: &[String],
    symbol_redirect: &HashMap<String, String>,
    caller_sigs: &HashMap<String, CallerCallSiteSig>,
) -> Result<IrSliceResult, String> {
    let meta_attach_re = Regex::new(r",\s*![a-zA-Z0-9_.]+\s+![0-9]+").unwrap();
    let attr_grp_re = Regex::new(r"\s+#[0-9]+\b").unwrap();

    let mut type_defs: BTreeMap<String, String> = BTreeMap::new();
    let mut globals: BTreeMap<String, String> = BTreeMap::new();
    let mut declares: BTreeMap<String, String> = BTreeMap::new();
    let mut defines: BTreeMap<String, (String, Vec<String>, String)> = BTreeMap::new();

    let def_hdr_re =
        Regex::new(r"^define\s+(?:[a-zA-Z0-9_().]+\s+)*([a-zA-Z0-9_*<>]+)\s+@([^\s(]+)\(([^)]*)\)")
            .unwrap();
    let decl_re = Regex::new(r"^declare\s+.*@([^\s(]+)\(").unwrap();

    let lines: Vec<&str> = raw_ll.lines().collect();
    let mut idx = 0usize;
    while idx < lines.len() {
        let raw_line = lines[idx];
        let trimmed = raw_line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(';')
            || trimmed.starts_with("source_filename")
            || trimmed.starts_with("target ")
            || trimmed.starts_with("attributes #")
            || trimmed.starts_with('!')
            || trimmed.starts_with("module asm")
        {
            idx += 1;
            continue;
        }

        let cleaned_line = attr_grp_re.replace_all(
            &meta_attach_re.replace_all(raw_line, ""),
            "",
        ).into_owned();
        let ctrim = cleaned_line.trim();

        if ctrim.starts_with('%') && ctrim.contains("= type ") {
            if let Some(eq) = ctrim.find('=') {
                let tname = ctrim[..eq].trim().to_string();
                type_defs.insert(tname, ctrim.to_string());
            }
            idx += 1;
            continue;
        }

        if ctrim.starts_with('@') && ctrim.contains('=') {
            if let Some(eq) = ctrim.find('=') {
                let gname = ctrim[1..eq].trim().trim_matches('"').to_string();
                // Strip `, comdat...` or `, align ...` metadata safely (`align` is valid, `comdat` without `$` group can fail)
                let no_comdat = Regex::new(r",\s*comdat(?:\([^)]*\))?")
                    .unwrap()
                    .replace_all(ctrim, "")
                    .into_owned();
                globals.insert(gname, no_comdat);
            }
            idx += 1;
            continue;
        }

        if ctrim.starts_with("declare ") {
            if let Some(caps) = decl_re.captures(ctrim) {
                let dname = caps[1].trim_matches('"').to_string();
                declares.insert(dname, ctrim.to_string());
            }
            idx += 1;
            continue;
        }

        if ctrim.starts_with("define ") {
            // Collect full function body up to matching closing `}` at start of line
            let mut body_lines = Vec::new();
            let mut header_line = ctrim.to_string();
            // Strip `comdat(...)` / `personality ...` if any
            header_line = Regex::new(r"\s+comdat(?:\([^)]*\))?")
                .unwrap()
                .replace_all(&header_line, "")
                .into_owned();

            body_lines.push(header_line.clone());
            if !ctrim.ends_with('}') {
                idx += 1;
                while idx < lines.len() {
                    let bl = attr_grp_re
                        .replace_all(&meta_attach_re.replace_all(lines[idx], ""), "")
                        .into_owned();
                    let bt = bl.trim();
                    body_lines.push(bl.clone());
                    if bt == "}" {
                        break;
                    }
                    idx += 1;
                }
            }
            if let Some(caps) = def_hdr_re.captures(&header_line) {
                let ret_ty = caps[1].trim().to_string();
                let fn_name = caps[2].trim_matches('"').to_string();
                let raw_params = caps[3].trim();
                let param_tys = parse_llvm_def_param_types(raw_params);
                defines.insert(fn_name, (ret_ty, param_tys, body_lines.join("\n")));
            }
            idx += 1;
            continue;
        }

        idx += 1;
    }

    // Determine which requested symbols exist in `defines`
    let mut resolved_symbols = HashSet::new();
    let mut root_def_names = Vec::new();
    for req in requested_symbols {
        let lookup = symbol_redirect.get(req).map(|s| s.as_str()).unwrap_or(req);
        if defines.contains_key(lookup) {
            resolved_symbols.insert(req.clone());
            root_def_names.push(lookup.to_string());
        } else if let Some(stripped) = lookup.strip_prefix('_') {
            if defines.contains_key(stripped) {
                resolved_symbols.insert(req.clone());
                root_def_names.push(stripped.to_string());
            }
        }
    }

    if resolved_symbols.is_empty() {
        return Ok(IrSliceResult {
            ir_code: String::new(),
            resolved_symbols,
            needed_decls: BTreeSet::new(),
        });
    }

    // Reachability walk over `@symbol` and `%Type` references
    let sym_ref_re = Regex::new(r#"@("[^"]+"|[A-Za-z0-9_.$]+)"#).unwrap();
    let ty_ref_re = Regex::new(r#"%[A-Za-z0-9_.]+"#).unwrap();

    let mut reach_fns: BTreeSet<String> = BTreeSet::new();
    let mut reach_globals: BTreeSet<String> = BTreeSet::new();
    let mut reach_decls: BTreeSet<String> = BTreeSet::new();
    let mut reach_types: BTreeSet<String> = BTreeSet::new();
    let mut work: Vec<String> = root_def_names;

    while let Some(sym) = work.pop() {
        if let Some((_, _, body)) = defines.get(&sym) {
            if reach_fns.insert(sym.clone()) {
                for m in sym_ref_re.captures_iter(body) {
                    let r = m[1].trim_matches('"').to_string();
                    if r != sym {
                        work.push(r);
                    }
                }
                for m in ty_ref_re.find_iter(body) {
                    let t = m.as_str().to_string();
                    if type_defs.contains_key(&t) {
                        reach_types.insert(t);
                    }
                }
            }
        } else if let Some(gdef) = globals.get(&sym) {
            if reach_globals.insert(sym.clone()) {
                for m in sym_ref_re.captures_iter(gdef) {
                    let r = m[1].trim_matches('"').to_string();
                    if r != sym {
                        work.push(r);
                    }
                }
                for m in ty_ref_re.find_iter(gdef) {
                    let t = m.as_str().to_string();
                    if type_defs.contains_key(&t) {
                        reach_types.insert(t);
                    }
                }
            }
        } else if declares.contains_key(&sym) {
            reach_decls.insert(sym);
        }
    }

    // Transitively close `reach_types`
    let mut ty_changed = true;
    while ty_changed {
        ty_changed = false;
        let snapshot: Vec<String> = reach_types.iter().cloned().collect();
        for t in snapshot {
            if let Some(tdef) = type_defs.get(&t) {
                for m in ty_ref_re.find_iter(tdef) {
                    let sub_t = m.as_str().to_string();
                    if type_defs.contains_key(&sub_t) && reach_types.insert(sub_t) {
                        ty_changed = true;
                    }
                }
            }
        }
    }

    // Build symbol rename map for all reachable internal functions & globals so they never clash!
    let mut rename_map: HashMap<String, String> = HashMap::new();
    for f in &reach_fns {
        let clean_f: String = f
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        rename_map.insert(f.clone(), format!("__omni_cir_{}_{}", mod_id, clean_f));
    }
    for g in &reach_globals {
        let clean_g: String = g
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        rename_map.insert(g.clone(), format!("__omni_cirg_{}_{}", mod_id, clean_g));
    }

    let rewrite_at_refs = |text: &str| -> String {
        sym_ref_re
            .replace_all(text, |caps: &regex::Captures| {
                let raw = caps[1].trim_matches('"');
                if let Some(new_nm) = rename_map.get(raw) {
                    format!("@{}", new_nm)
                } else {
                    caps[0].to_string()
                }
            })
            .into_owned()
    };

    let mut out = String::new();
    for t in &reach_types {
        if let Some(tdef) = type_defs.get(t) {
            out.push_str(tdef);
            out.push('\n');
        }
    }
    for g in &reach_globals {
        if let Some(gdef) = globals.get(g) {
            let mut line = rewrite_at_refs(gdef);
            // Force `internal` linkage on sliced globals so multiple modules never collide
            line = line
                .replace("= dso_local ", "= internal ")
                .replace("= external ", "= internal ")
                .replace("= common ", "= internal ");
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push('\n');

    for f in &reach_fns {
        if let Some((_, _, body)) = defines.get(f) {
            let mut rewritten = rewrite_at_refs(body);
            // Ensure `define internal ` linkage so adapter owns the public `@<sym>` name!
            if rewritten.starts_with("define ") && !rewritten.starts_with("define internal ") {
                rewritten = rewritten
                    .replacen("define dso_local ", "define internal ", 1)
                    .replacen("define weak_odr ", "define internal ", 1)
                    .replacen("define linkonce_odr ", "define internal ", 1);
                if !rewritten.starts_with("define internal ") {
                    rewritten = rewritten.replacen("define ", "define internal ", 1);
                }
            }
            out.push_str(&rewritten);
            out.push_str("\n\n");
        }
    }

    // Collect external declarations needed by the C IR slice
    let mut needed_decls = BTreeSet::new();
    for d in &reach_decls {
        if let Some(dline) = declares.get(d) {
            needed_decls.insert(dline.clone());
        }
    }

    // Emit `alwaysinline` public call-site adapters `@<req_sym>` matching `omni_ir`!
    for req in &resolved_symbols {
        let lookup = symbol_redirect.get(req).map(|s| s.as_str()).unwrap_or(req);
        let actual_key = if defines.contains_key(lookup) {
            lookup
        } else {
            lookup.strip_prefix('_').unwrap_or(lookup)
        };
        let Some((c_ret_ty, c_param_tys, _)) = defines.get(actual_key) else {
            continue;
        };
        let Some(internal_nm) = rename_map.get(actual_key) else {
            continue;
        };

        let caller_sig = caller_sigs.get(req).cloned().unwrap_or_else(|| CallerCallSiteSig {
            ret_ty: "i64".to_string(),
            param_tys: vec!["i64".to_string(); c_param_tys.len()],
            is_unsigned_ret: false,
        });

        emit_cir_adapter(
            &mut out,
            req,
            internal_nm,
            c_ret_ty,
            c_param_tys,
            &caller_sig,
        );
    }

    Ok(IrSliceResult {
        ir_code: out,
        resolved_symbols,
        needed_decls,
    })
}

fn parse_llvm_def_param_types(raw_params: &str) -> Vec<String> {
    if raw_params.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for part in raw_params.split(',') {
        let pt = part.trim();
        if pt.is_empty() || pt == "..." {
            continue;
        }
        // First token is the LLVM type (`ptr`, `i8*`, `i32`, `i64`, `double`, `float`, `%struct.Foo*`)
        if let Some(first_tok) = pt.split_whitespace().next() {
            out.push(first_tok.to_string());
        }
    }
    out
}

fn emit_cir_adapter(
    out: &mut String,
    pub_sym: &str,
    internal_nm: &str,
    c_ret_ty: &str,
    c_param_tys: &[String],
    caller_sig: &CallerCallSiteSig,
) {
    let mut tmp = 0usize;
    let params_decl = caller_sig
        .param_tys
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{} %a{}", t, i))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = writeln!(
        out,
        "define {} @{}({}) alwaysinline nounwind {{\nentry:",
        caller_sig.ret_ty, pub_sym, params_decl
    );

    let mut call_args = Vec::with_capacity(c_param_tys.len());
    for (idx, c_ty) in c_param_tys.iter().enumerate() {
        let (in_ty, in_reg) = if idx < caller_sig.param_tys.len() {
            (caller_sig.param_tys[idx].as_str(), format!("%a{}", idx))
        } else {
            ("i64", "0".to_string())
        };
        // First normalize `in_reg` to `i64` if needed
        let reg64 = if in_ty == "i64" {
            in_reg
        } else if in_ty == "i32" {
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(out, "  {} = sext i32 {} to i64", r, in_reg);
            r
        } else if in_ty.ends_with('*') || in_ty == "ptr" {
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(out, "  {} = ptrtoint {} {} to i64", r, in_ty, in_reg);
            r
        } else {
            in_reg
        };

        // Now coerce `i64` -> `c_ty`
        let coerced = if c_ty == "i64" {
            reg64
        } else if c_ty == "ptr" || c_ty.ends_with('*') {
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(out, "  {} = inttoptr i64 {} to {}", r, reg64, c_ty);
            r
        } else if matches!(c_ty.as_str(), "i32" | "i16" | "i8" | "i1") {
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(out, "  {} = trunc i64 {} to {}", r, reg64, c_ty);
            r
        } else if c_ty == "double" {
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(out, "  {} = bitcast i64 {} to double", r, reg64);
            r
        } else if c_ty == "float" {
            tmp += 1;
            let tr = format!("%c{}", tmp);
            tmp += 1;
            let r = format!("%c{}", tmp);
            let _ = writeln!(
                out,
                "  {} = trunc i64 {} to i32\n  {} = bitcast i32 {} to float",
                tr, reg64, r, tr
            );
            r
        } else {
            reg64
        };
        call_args.push(format!("{} {}", c_ty, coerced));
    }

    if c_ret_ty == "void" {
        let _ = writeln!(out, "  call void @{}({})", internal_nm, call_args.join(", "));
        if caller_sig.ret_ty == "void" {
            out.push_str("  ret void\n}\n\n");
        } else {
            let _ = writeln!(out, "  ret {} 0\n}}\n", caller_sig.ret_ty);
        }
        return;
    }

    tmp += 1;
    let raw_ret = format!("%c{}", tmp);
    let _ = writeln!(
        out,
        "  {} = call {} @{}({})",
        raw_ret,
        c_ret_ty,
        internal_nm,
        call_args.join(", ")
    );

    // Coerce `c_ret_ty` -> `i64`, then -> `caller_sig.ret_ty`
    let ret64 = if c_ret_ty == "i64" {
        raw_ret
    } else if c_ret_ty == "ptr" || c_ret_ty.ends_with('*') {
        tmp += 1;
        let r = format!("%c{}", tmp);
        let _ = writeln!(out, "  {} = ptrtoint {} {} to i64", r, c_ret_ty, raw_ret);
        r
    } else if matches!(c_ret_ty, "i32" | "i16" | "i8") {
        tmp += 1;
        let r = format!("%c{}", tmp);
        let ext = if caller_sig.is_unsigned_ret {
            "zext"
        } else {
            "sext"
        };
        let _ = writeln!(out, "  {} = {} {} {} to i64", r, ext, c_ret_ty, raw_ret);
        r
    } else if c_ret_ty == "i1" {
        tmp += 1;
        let r = format!("%c{}", tmp);
        let _ = writeln!(out, "  {} = zext i1 {} to i64", r, raw_ret);
        r
    } else if c_ret_ty == "double" {
        tmp += 1;
        let r = format!("%c{}", tmp);
        let _ = writeln!(out, "  {} = bitcast double {} to i64", r, raw_ret);
        r
    } else if c_ret_ty == "float" {
        tmp += 1;
        let d = format!("%c{}", tmp);
        tmp += 1;
        let r = format!("%c{}", tmp);
        let _ = writeln!(
            out,
            "  {} = fpext float {} to double\n  {} = bitcast double {} to i64",
            d, raw_ret, r, d
        );
        r
    } else {
        "0".to_string()
    };

    if caller_sig.ret_ty == "i64" {
        let _ = writeln!(out, "  ret i64 {}\n}}\n", ret64);
    } else if caller_sig.ret_ty == "i32" {
        tmp += 1;
        let r32 = format!("%c{}", tmp);
        let _ = writeln!(
            out,
            "  {} = trunc i64 {} to i32\n  ret i32 {}\n}}\n",
            r32, ret64, r32
        );
    } else {
        let _ = writeln!(out, "  ret {} 0\n}}\n", caller_sig.ret_ty);
    }
}
