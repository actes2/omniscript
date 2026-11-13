pub mod arm64;
pub mod binfmt;
pub mod ext_abi;
pub mod ir_ingest;
pub mod x86_64;

use crate::shimmer::TargetPlatform;
use arm64::lift_aarch64_functions;
use binfmt::{load_binary_payload, BinaryAbi, BinaryArch, BinaryLoadOutcome, ParsedBinaryImage};
use ext_abi::{emit_llvm_byte_array_global, emit_vma_helpers, next_t, OMNI_CPU_STATE_TY};
use ir_ingest::{
    bitcode_bytes_to_ll_text, compile_c_source_to_ll_text, compile_header_inline_defs_to_ll_text,
    slice_and_adapt_ll_module, CallerCallSiteSig,
};
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use x86_64::lift_x86_64_functions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CReturnHint {
    Signed32,
    Unsigned32,
    Int64OrPtr,
    Float64,
    Float32,
    Void,
}

#[derive(Debug, Default)]
pub struct CIngestionReport {
    pub internalized_symbols: HashSet<String>,
    pub internalized_files: HashSet<PathBuf>,
    pub diagnostics: Vec<String>,
}

/// Inspect `omni_ir`, lift reachable C symbols from all resolved C artifacts (`.ll`, `.bc`, `.c`, `.cpp`,
/// `.h` inline, `.o`, `.obj`, `.a`, `.lib`, `.so`, `.dylib`, `.dll`) across `x86_64` and `aarch64`
/// directly into LLVM IR `define` instructions, splice them into `omni_ir`, and prune `foreign_c_files`!
pub fn ingest_and_splice_c_dependencies(
    omni_ir: &mut String,
    foreign_c_files: &mut Vec<PathBuf>,
    all_dependency_files: &[PathBuf],
    target_platform: &TargetPlatform,
    cache_dir: &Path,
) -> CIngestionReport {
    let mut report = CIngestionReport::default();

    // 1. Discover external C call sites in `omni_ir` (excluding OmniScript runtime & host libc primitives)
    let caller_sigs = harvest_external_c_call_sites(omni_ir, all_dependency_files);
    if caller_sigs.is_empty() {
        return report;
    }

    let mut remaining_symbols: Vec<String> = caller_sigs.keys().cloned().collect();
    remaining_symbols.sort();

    // Collect include directories & header return-type hints across `all_dependency_files`
    let mut include_dirs: Vec<PathBuf> = Vec::new();
    let mut header_files: Vec<PathBuf> = Vec::new();
    for df in all_dependency_files {
        let ext = df
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "h" | "hpp") && df.is_file() {
            header_files.push(df.clone());
            if let Some(p) = df.parent() {
                if !include_dirs.contains(&p.to_path_buf()) {
                    include_dirs.push(p.to_path_buf());
                }
                if let Some(gp) = p.parent() {
                    if !include_dirs.contains(&gp.to_path_buf()) {
                        include_dirs.push(gp.to_path_buf());
                    }
                }
            }
        }
    }
    let ret_hints = harvest_header_return_hints(&header_files, &remaining_symbols);

    // Also include `.ll`, `.bc`, `.o`, `.obj` files from `all_dependency_files` if not already in `foreign_c_files`
    let mut candidate_artifacts: Vec<PathBuf> = Vec::new();
    for f in foreign_c_files.iter().chain(all_dependency_files.iter()) {
        if f.is_file() && !candidate_artifacts.contains(f) {
            candidate_artifacts.push(f.clone());
        }
    }

    let mut needs_cpu_state_ty = false;
    let mut spliced_blocks: Vec<String> = Vec::new();
    let mut extra_decls: BTreeSet<String> = BTreeSet::new();

    // ========================================================================
    // TIER 0 & TIER 1: `.ll`, `.bc`, `.c`, `.cc`, `.cpp`, and `static inline` `.h`
    // ========================================================================
    for art in &candidate_artifacts {
        if remaining_symbols.is_empty() {
            break;
        }
        let ext = art
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "ll" | "bc" | "c" | "cc" | "cpp") {
            continue;
        }
        let mod_id = artifact_short_id(art);
        let raw_ll_res = match ext.as_str() {
            "ll" => fs::read_to_string(art).map_err(|e| e.to_string()),
            "bc" => fs::read(art)
                .map_err(|e| e.to_string())
                .and_then(|b| bitcode_bytes_to_ll_text(&b, &target_platform.llvm_triple)),
            _ => compile_c_source_to_ll_text(art, &include_dirs, &target_platform.llvm_triple),
        };
        let Ok(raw_ll) = raw_ll_res else {
            continue;
        };
        let empty_redir = HashMap::new();
        if let Ok(sliced) = slice_and_adapt_ll_module(
            &raw_ll,
            &mod_id,
            &remaining_symbols,
            &empty_redir,
            &caller_sigs,
        ) {
            if !sliced.resolved_symbols.is_empty() {
                report.diagnostics.push(format!(
                    "[C-Lifter Tier 0/1] Lowered {} -> {} IR symbol(s): {:?}",
                    art.display(),
                    sliced.resolved_symbols.len(),
                    sliced.resolved_symbols
                ));
                for s in &sliced.resolved_symbols {
                    report.internalized_symbols.insert(s.clone());
                }
                remaining_symbols.retain(|s| !sliced.resolved_symbols.contains(s));
                report.internalized_files.insert(art.clone());
                extra_decls.extend(sliced.needed_decls);
                spliced_blocks.push(sliced.ir_code);
            }
        }
    }

    // Tier 1b: Header-only `static inline` / `inline` C functions in `.h` / `.hpp`
    for hdr in &header_files {
        if remaining_symbols.is_empty() {
            break;
        }
        if let Ok(Some((raw_ll, redir))) = compile_header_inline_defs_to_ll_text(
            hdr,
            &remaining_symbols,
            &include_dirs,
            &target_platform.llvm_triple,
        ) {
            let mod_id = artifact_short_id(hdr);
            if let Ok(sliced) = slice_and_adapt_ll_module(
                &raw_ll,
                &mod_id,
                &remaining_symbols,
                &redir,
                &caller_sigs,
            ) {
                if !sliced.resolved_symbols.is_empty() {
                    report.diagnostics.push(format!(
                        "[C-Lifter Tier 1-Header] Inlined {} -> {:?}",
                        hdr.display(),
                        sliced.resolved_symbols
                    ));
                    for s in &sliced.resolved_symbols {
                        report.internalized_symbols.insert(s.clone());
                    }
                    remaining_symbols.retain(|s| !sliced.resolved_symbols.contains(s));
                    extra_decls.extend(sliced.needed_decls);
                    spliced_blocks.push(sliced.ir_code);
                }
            }
        }
    }

    // ========================================================================
    // TIER 2 & TIER 3: `.o`, `.obj`, `.a`, `.lib`, `.so`, `.dylib`, `.dll`
    // ========================================================================
    for art in &candidate_artifacts {
        if remaining_symbols.is_empty() {
            break;
        }
        if !is_binary_c_artifact(art) {
            continue;
        }

        let Some((effective_bin_path, outcome)) =
            load_or_pivot_binary_artifact(art, target_platform, &remaining_symbols)
        else {
            continue;
        };

        let mod_id = artifact_short_id(&effective_bin_path);
        match outcome {
            BinaryLoadOutcome::Bitcode(bc_bytes) => {
                // Embedded LTO fat-object bitcode (`.llvmbc` / `__LLVM,__bitcode`)!
                if let Ok(raw_ll) = bitcode_bytes_to_ll_text(&bc_bytes, &target_platform.llvm_triple)
                {
                    let empty_redir = HashMap::new();
                    if let Ok(sliced) = slice_and_adapt_ll_module(
                        &raw_ll,
                        &mod_id,
                        &remaining_symbols,
                        &empty_redir,
                        &caller_sigs,
                    ) {
                        if !sliced.resolved_symbols.is_empty() {
                            report.diagnostics.push(format!(
                                "[C-Lifter LTO-Bitcode] Extracted embedded bitcode from {} -> {:?}",
                                effective_bin_path.display(),
                                sliced.resolved_symbols
                            ));
                            for s in &sliced.resolved_symbols {
                                report.internalized_symbols.insert(s.clone());
                            }
                            remaining_symbols.retain(|s| !sliced.resolved_symbols.contains(s));
                            report.internalized_files.insert(art.clone());
                            report.internalized_files.insert(effective_bin_path);
                            extra_decls.extend(sliced.needed_decls);
                            spliced_blocks.push(sliced.ir_code);
                        }
                    }
                }
            }
            BinaryLoadOutcome::Image(img) => {
                // Match requested symbols to entry VMAs inside `img`
                let mut matched_entries: Vec<(String, u64)> = Vec::new();
                for sym in &remaining_symbols {
                    if let Some(vma) = img.resolve_symbol_vma(sym) {
                        if img.is_exec_vma(vma) {
                            matched_entries.push((sym.clone(), vma));
                        }
                    }
                }
                if matched_entries.is_empty() {
                    continue;
                }

                // Check lifted IR disk cache (`<cache_dir>/lifted_c/<blake3>.ll`)
                let cache_key = compute_lifted_cache_key(
                    &effective_bin_path,
                    &img,
                    &matched_entries,
                    &caller_sigs,
                    &ret_hints,
                );
                let lifted_cache_dir = cache_dir.join("lifted_c");
                let cached_ll_path = lifted_cache_dir.join(format!("{}.ll", cache_key));
                let cached_decls_path = lifted_cache_dir.join(format!("{}.decls", cache_key));

                if let (Ok(cached_ir), Ok(cached_decls)) = (
                    fs::read_to_string(&cached_ll_path),
                    fs::read_to_string(&cached_decls_path),
                ) {
                    needs_cpu_state_ty = true;
                    for dline in cached_decls.lines() {
                        if !dline.trim().is_empty() {
                            extra_decls.insert(dline.to_string());
                        }
                    }
                    let syms_done: Vec<String> =
                        matched_entries.iter().map(|(s, _)| s.clone()).collect();
                    report.diagnostics.push(format!(
                        "[C-Lifter Binary Cache Hit] {} [{} / {}] -> {:?}",
                        effective_bin_path.display(),
                        img.format_label,
                        img.abi.as_str(),
                        syms_done
                    ));
                    for s in &syms_done {
                        report.internalized_symbols.insert(s.clone());
                    }
                    remaining_symbols.retain(|s| !report.internalized_symbols.contains(s));
                    report.internalized_files.insert(art.clone());
                    report.internalized_files.insert(effective_bin_path);
                    spliced_blocks.push(cached_ir);
                    continue;
                }

                let root_vmas: Vec<u64> = matched_entries.iter().map(|(_, v)| *v).collect();
                let lift_res = match img.arch {
                    BinaryArch::X86_64 => lift_x86_64_functions(&img, &mod_id, &root_vmas),
                    BinaryArch::Aarch64 => lift_aarch64_functions(&img, &mod_id, &root_vmas),
                };

                match lift_res {
                    Ok(mut lifted) => {
                        needs_cpu_state_ty = true;
                        let mut block_ir = String::new();
                        let img_glob = format!("__omni_lifted_img_{}", mod_id);
                        emit_llvm_byte_array_global(&mut block_ir, &img_glob, &img.image_bytes);
                        block_ir.push('\n');
                        emit_vma_helpers(&mut block_ir, &mod_id, &img);
                        block_ir.push('\n');
                        block_ir.push_str(&lifted.ir_functions);
                        block_ir.push('\n');

                        // Emit exported entry wrappers matching `omni_ir` call sites!
                        for (sym, entry_vma) in &matched_entries {
                            let default_sig = CallerCallSiteSig {
                                ret_ty: "i64".to_string(),
                                param_tys: vec![
                                    "i64".to_string(),
                                    "i64".to_string(),
                                    "i64".to_string(),
                                    "i64".to_string(),
                                    "i64".to_string(),
                                    "i64".to_string(),
                                ],
                                is_unsigned_ret: false,
                            };
                            let csig = caller_sigs.get(sym).unwrap_or(&default_sig);
                            let rhint = ret_hints
                                .get(sym)
                                .copied()
                                .unwrap_or(if csig.is_unsigned_ret {
                                    CReturnHint::Unsigned32
                                } else {
                                    CReturnHint::Signed32
                                });
                            emit_binary_export_wrapper(
                                &mut block_ir,
                                &img,
                                &mod_id,
                                sym,
                                *entry_vma,
                                csig,
                                rhint,
                                lifted.max_static_stack_alloc,
                                &mut lifted.ext_tracker.needed_decls,
                            );
                        }

                        // Persist to `<cache_dir>/lifted_c/`
                        let _ = fs::create_dir_all(&lifted_cache_dir);
                        let decls_joined: String = lifted
                            .ext_tracker
                            .needed_decls
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        let _ = fs::write(&cached_ll_path, &block_ir);
                        let _ = fs::write(&cached_decls_path, &decls_joined);

                        let syms_done: Vec<String> =
                            matched_entries.iter().map(|(s, _)| s.clone()).collect();
                        report.diagnostics.push(format!(
                            "[C-Lifter Binary -> LLVM IR] {} [{} / {}]: lifted {} functions ({} instructions) for {:?}",
                            effective_bin_path.display(),
                            img.format_label,
                            img.abi.as_str(),
                            lifted.lifted_fn_vmas.len(),
                            lifted.total_instructions,
                            syms_done
                        ));
                        extra_decls.extend(lifted.ext_tracker.needed_decls);
                        for s in &syms_done {
                            report.internalized_symbols.insert(s.clone());
                        }
                        remaining_symbols.retain(|s| !report.internalized_symbols.contains(s));
                        if remaining_symbols.is_empty() {
                            report.internalized_files.insert(art.clone());
                            report.internalized_files.insert(effective_bin_path);
                        } else {
                            // Even if unrelated symbols remain for other libraries, if `img` has no more matches, `art` is done!
                            let has_unhandled_in_img = remaining_symbols
                                .iter()
                                .any(|rs| img.resolve_symbol_vma(rs).is_some());
                            if !has_unhandled_in_img {
                                report.internalized_files.insert(art.clone());
                                report.internalized_files.insert(effective_bin_path);
                            }
                        }
                        spliced_blocks.push(block_ir);
                    }
                    Err(err_msg) => {
                        report.diagnostics.push(format!(
                            "[C-Lifter Native Fallback] Retained native link boundary for {} ({})",
                            effective_bin_path.display(),
                            err_msg
                        ));
                    }
                }
            }
            BinaryLoadOutcome::ImportLibraryOnly { .. } => {}
        }
    }

    if spliced_blocks.is_empty() {
        return report;
    }

    // Splice lifted definitions & deduplicated external declarations into `omni_ir`!
    splice_lifted_ir_into_module(
        omni_ir,
        &report.internalized_symbols,
        needs_cpu_state_ty,
        &extra_decls,
        &spliced_blocks,
    );

    // Prune fully internalized C artifacts from `foreign_c_files` so ZERO external `.so`/`.dll`/`.dylib`/`.a` is linked!
    foreign_c_files.retain(|f| !report.internalized_files.contains(f));

    report
}

fn is_binary_c_artifact(p: &Path) -> bool {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let fname = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "o" | "obj" | "a" | "lib" | "so" | "dylib" | "dll"
    ) || fname.contains(".so.")
}

/// Load a binary artifact, automatically pivoting from a Windows COFF import library (`.dll.a` / `.lib`)
/// to the sibling `.dll` in `<pkg_dir>/bin/` or `<pkg_dir>/lib/` so real `Win64` machine code is lifted!
fn load_or_pivot_binary_artifact(
    art: &Path,
    target_platform: &TargetPlatform,
    requested_symbols: &[String],
) -> Option<(PathBuf, BinaryLoadOutcome)> {
    let bytes = fs::read(art).ok()?;
    let outcome = load_binary_payload(&bytes, target_platform.arch, requested_symbols).ok()?;
    match outcome {
        BinaryLoadOutcome::ImportLibraryOnly { ref dll_names } => {
            let mut search_dirs = Vec::new();
            if let Some(lib_dir) = art.parent() {
                search_dirs.push(lib_dir.to_path_buf());
                if let Some(pkg_root) = lib_dir.parent() {
                    search_dirs.push(pkg_root.join("bin"));
                    search_dirs.push(pkg_root.to_path_buf());
                }
            }
            // 1. Exact DLL name referenced inside the COFF import library!
            for dir in &search_dirs {
                for dll in dll_names {
                    let cand = dir.join(dll);
                    if cand.is_file() {
                        if let Ok(dbytes) = fs::read(&cand) {
                            if let Ok(dout) = load_binary_payload(
                                &dbytes,
                                target_platform.arch,
                                requested_symbols,
                            ) {
                                return Some((cand, dout));
                            }
                        }
                    }
                }
            }
            // 2. Fallback: any `.dll` in `<pkg_root>/bin` or `<lib_dir>`
            for dir in &search_dirs {
                if let Ok(entries) = fs::read_dir(dir) {
                    for ent in entries.filter_map(|e| e.ok()) {
                        let p = ent.path();
                        if p.is_file()
                            && p.extension()
                                .and_then(|s| s.to_str())
                                .map(|s| s.eq_ignore_ascii_case("dll"))
                                .unwrap_or(false)
                        {
                            if let Ok(dbytes) = fs::read(&p) {
                                if let Ok(dout) = load_binary_payload(
                                    &dbytes,
                                    target_platform.arch,
                                    requested_symbols,
                                ) {
                                    return Some((p, dout));
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        other => Some((art.to_path_buf(), other)),
    }
}

fn artifact_short_id(p: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(p.to_string_lossy().as_bytes());
    let hex = h.finalize().to_hex().to_string();
    hex[..10].to_string()
}

fn compute_lifted_cache_key(
    path: &Path,
    img: &ParsedBinaryImage,
    matched_entries: &[(String, u64)],
    caller_sigs: &HashMap<String, CallerCallSiteSig>,
    ret_hints: &HashMap<String, CReturnHint>,
) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"omni_c_lifter_v1");
    h.update(path.to_string_lossy().as_bytes());
    h.update(&img.image_bytes);
    for (sym, vma) in matched_entries {
        h.update(sym.as_bytes());
        h.update(&vma.to_le_bytes());
        if let Some(cs) = caller_sigs.get(sym) {
            h.update(cs.ret_ty.as_bytes());
            for p in &cs.param_tys {
                h.update(p.as_bytes());
            }
        }
        if let Some(rh) = ret_hints.get(sym) {
            h.update(&[*rh as u8]);
        }
    }
    h.finalize().to_hex().to_string()
}

fn is_builtin_host_or_omni_symbol(sym: &str) -> bool {
    if sym.starts_with("__omni_") || sym.starts_with("llvm.") {
        return true;
    }
    matches!(
        sym,
        "main"
            | "malloc"
            | "calloc"
            | "realloc"
            | "free"
            | "memcpy"
            | "memmove"
            | "memset"
            | "memcmp"
            | "strlen"
            | "strcpy"
            | "strncpy"
            | "strcat"
            | "strcmp"
            | "strncmp"
            | "strchr"
            | "strstr"
            | "printf"
            | "fprintf"
            | "sprintf"
            | "snprintf"
            | "puts"
            | "putchar"
            | "getchar"
            | "fopen"
            | "fclose"
            | "fread"
            | "fwrite"
            | "fseek"
            | "ftell"
            | "fflush"
            | "remove"
            | "rename"
            | "getenv"
            | "system"
            | "abort"
            | "exit"
            | "sleep"
            | "usleep"
            | "time"
            | "clock"
            | "srand"
            | "rand"
            | "_setjmp"
            | "setjmp"
            | "longjmp"
    )
}

/// Scan `omni_ir` for external C calls (`call <ret_ty> @<sym>(<args>)`) that are NOT strongly defined
/// in `omni_ir` and NOT standard `libc` / `__omni_*` runtime intrinsics.
fn harvest_external_c_call_sites(
    omni_ir: &str,
    all_dep_files: &[PathBuf],
) -> HashMap<String, CallerCallSiteSig> {
    if all_dep_files.is_empty() {
        return HashMap::new();
    }

    // Collect strongly defined functions (`define i64 @foo` / `define i32 @foo` / `define void @foo` — NOT `define weak`!)
    let strong_def_re =
        Regex::new(r"(?m)^define\s+(?:internal\s+|dso_local\s+)?(?:i64|i32|i8\*|void|double)\s+@([A-Za-z0-9_.$]+)\(")
            .unwrap();
    let mut strong_defs = HashSet::new();
    for caps in strong_def_re.captures_iter(omni_ir) {
        strong_defs.insert(caps[1].to_string());
    }

    let call_re =
        Regex::new(r"call\s+(i64|i32|i8\*|void|double)\s+@([A-Za-z0-9_.$]+)\(([^)]*)\)").unwrap();
    let mut out: HashMap<String, CallerCallSiteSig> = HashMap::new();

    for caps in call_re.captures_iter(omni_ir) {
        let ret_ty = caps[1].to_string();
        let sym = caps[2].to_string();
        let raw_args = caps[3].trim();

        if is_builtin_host_or_omni_symbol(&sym) || strong_defs.contains(&sym) {
            continue;
        }

        let mut param_tys = Vec::new();
        if !raw_args.is_empty() {
            for part in raw_args.split(',') {
                let pt = part.trim();
                if let Some(ty_tok) = pt.split_whitespace().next() {
                    param_tys.push(ty_tok.to_string());
                }
            }
        }

        out.entry(sym).or_insert(CallerCallSiteSig {
            ret_ty,
            param_tys,
            is_unsigned_ret: false,
        });
    }

    out
}

/// Inspect resolved C headers (`.h`/`.hpp`) to classify return types (`int` vs `unsigned` vs `*`/`size_t` vs `double`).
fn harvest_header_return_hints(
    headers: &[PathBuf],
    symbols: &[String],
) -> HashMap<String, CReturnHint> {
    let mut out = HashMap::new();
    if headers.is_empty() || symbols.is_empty() {
        return out;
    }

    let mut combined_hdrs = String::new();
    for h in headers {
        if let Ok(txt) = fs::read_to_string(h) {
            combined_hdrs.push_str(&txt);
            combined_hdrs.push('\n');
        }
    }
    if combined_hdrs.is_empty() {
        return out;
    }

    for sym in symbols {
        let pat = format!(
            r"(?m)^[^\n/;#]*?\b([A-Za-z0-9_*\s]+?)\b{}\s*\(",
            regex::escape(sym)
        );
        let Ok(re) = Regex::new(&pat) else {
            continue;
        };
        for caps in re.captures_iter(&combined_hdrs) {
            let prefix = caps[1].trim();
            if prefix.contains("define") || prefix.contains("typedef") {
                continue;
            }
            let pl = prefix.to_ascii_lowercase();
            let hint = if prefix.contains('*')
                || pl.contains("size_t")
                || pl.contains("ssize_t")
                || pl.contains("int64")
                || pl.contains("uint64")
                || pl.contains("intptr")
                || pl.contains("uintptr")
                || pl.contains("ptrdiff")
                || pl.contains("ulong")
                || pl.contains("long")
            {
                CReturnHint::Int64OrPtr
            } else if pl.contains("double") {
                CReturnHint::Float64
            } else if pl.contains("float") {
                CReturnHint::Float32
            } else if pl == "void" || pl.ends_with(" void") {
                CReturnHint::Void
            } else if pl.contains("unsigned")
                || pl.contains("uint")
                || pl.contains("u32")
                || pl.contains("bool")
            {
                CReturnHint::Unsigned32
            } else {
                CReturnHint::Signed32
            };
            out.insert(sym.clone(), hint);
            break;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_binary_export_wrapper(
    out: &mut String,
    img: &ParsedBinaryImage,
    mod_id: &str,
    pub_sym: &str,
    entry_vma: u64,
    caller_sig: &CallerCallSiteSig,
    ret_hint: CReturnHint,
    max_static_stack_alloc: u64,
    needed_decls: &mut BTreeSet<String>,
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
        "define {} @{}({}) nounwind {{\nentry:",
        caller_sig.ret_ty, pub_sym, params_decl
    );

    // 1. Allocate & zero-initialize `%OmniCpuState` (`SROA`-friendly!)
    out.push_str("  %st = alloca %OmniCpuState, align 16\n  store %OmniCpuState zeroinitializer, %OmniCpuState* %st, align 16\n");

    // 2. Allocate stack buffer:
    //    - Small frames (`<= 4 KB`): 16 KB stack `alloca` (eliminated by LLVM DCE when unused, safe inside 64 KB fibers!)
    //    - Large frames (`> 4 KB`, e.g. `LZ4_compress_default`'s 16 KB stack hash table): 256 KB heap buffer freed before `ret`!
    let use_heap_stack = max_static_stack_alloc > 4096;
    let stk_size: u64 = if use_heap_stack { 262_144 } else { 16_384 };
    let stk_base_i64 = if use_heap_stack {
        needed_decls.insert("declare i8* @malloc(i64)".to_string());
        needed_decls.insert("declare void @free(i8*)".to_string());
        let hp = next_t(&mut tmp);
        let hi = next_t(&mut tmp);
        let _ = writeln!(
            out,
            "  {} = call i8* @malloc(i64 {})\n  {} = ptrtoint i8* {} to i64",
            hp, stk_size, hi, hp
        );
        (Some(hp), hi)
    } else {
        let sp_i = next_t(&mut tmp);
        let _ = writeln!(
            out,
            "  %stk_buf = alloca [{} x i8], align 16\n  {} = ptrtoint [{} x i8]* %stk_buf to i64",
            stk_size, sp_i, stk_size
        );
        (None, sp_i)
    };
    let (heap_ptr_opt, stk_i64) = stk_base_i64;

    // Leave 128 bytes at top of buffer for stack arguments & shadow space, 16-byte aligned!
    let top_raw = next_t(&mut tmp);
    let sp_aligned = next_t(&mut tmp);
    let _ = writeln!(
        out,
        "  {} = add i64 {}, {}\n  {} = and i64 {}, -16",
        top_raw,
        stk_i64,
        stk_size - 128,
        sp_aligned,
        top_raw
    );

    // Normalize caller args to `i64`
    let mut arg_vals64 = Vec::with_capacity(caller_sig.param_tys.len());
    for (idx, pty) in caller_sig.param_tys.iter().enumerate() {
        let areg = format!("%a{}", idx);
        let v64 = if pty == "i64" {
            areg
        } else if pty == "i32" {
            let r = next_t(&mut tmp);
            let _ = writeln!(out, "  {} = sext i32 {} to i64", r, areg);
            r
        } else if pty.ends_with('*') || pty == "ptr" {
            let r = next_t(&mut tmp);
            let _ = writeln!(out, "  {} = ptrtoint {} {} to i64", r, pty, areg);
            r
        } else if pty == "double" {
            let r = next_t(&mut tmp);
            let _ = writeln!(out, "  {} = bitcast double {} to i64", r, areg);
            r
        } else {
            areg
        };
        arg_vals64.push(v64);
    }

    // 3. Populate ABI registers & stack slots for `img.arch` / `img.abi`!
    match img.arch {
        BinaryArch::X86_64 => {
            // x86_64 ABI invariant at function entry: `RSP % 16 == 8` (`[RSP]` = 8-byte return address)!
            let rsp_entry = next_t(&mut tmp);
            let rsp_slot = next_t(&mut tmp);
            let ret_p8 = next_t(&mut tmp);
            let ret_p64 = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = sub i64 {}, 8\n  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 4\n  store i64 {}, i64* {}, align 8\n  {} = inttoptr i64 {} to i8*\n  {} = bitcast i8* {} to i64*\n  store i64 0, i64* {}, align 8",
                rsp_entry, sp_aligned, rsp_slot, rsp_entry, rsp_slot, ret_p8, rsp_entry, ret_p64, ret_p8, ret_p64
            );

            for (i, val64) in arg_vals64.iter().enumerate() {
                let reg_slot_opt = match img.abi {
                    BinaryAbi::Win64 => match i {
                        0 => Some(1), // RCX
                        1 => Some(2), // RDX
                        2 => Some(8), // R8
                        3 => Some(9), // R9
                        _ => None,
                    },
                    _ => match i {
                        0 => Some(7), // RDI
                        1 => Some(6), // RSI
                        2 => Some(2), // RDX
                        3 => Some(1), // RCX
                        4 => Some(8), // R8
                        5 => Some(9), // R9
                        _ => None,
                    },
                };
                if let Some(slot) = reg_slot_opt {
                    let gp = next_t(&mut tmp);
                    let _ = writeln!(
                        out,
                        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  store i64 {}, i64* {}, align 8",
                        gp, slot, val64, gp
                    );
                } else {
                    let stk_off = match img.abi {
                        // `[rsp_entry + 8 .. + 39]` = 32-byte Win64 shadow space; arg4 at `+40`!
                        BinaryAbi::Win64 => 40 + ((i - 4) as u64) * 8,
                        _ => 8 + ((i - 6) as u64) * 8,
                    };
                    let ea = next_t(&mut tmp);
                    let sp64 = next_t(&mut tmp);
                    let _ = writeln!(
                        out,
                        "  {} = add i64 {}, {}\n  {} = inttoptr i64 {} to i64*\n  store i64 {}, i64* {}, align 8",
                        ea, rsp_entry, stk_off, sp64, ea, val64, sp64
                    );
                }
            }
        }
        BinaryArch::Aarch64 => {
            // AAPCS64 / AppleArm64 invariant at function entry: `SP % 16 == 0` (slot 31)!
            let sp_slot = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 31\n  store i64 {}, i64* {}, align 8",
                sp_slot, sp_aligned, sp_slot
            );
            for (i, val64) in arg_vals64.iter().enumerate() {
                if i < 8 {
                    let gp = next_t(&mut tmp);
                    let _ = writeln!(
                        out,
                        "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}\n  store i64 {}, i64* {}, align 8",
                        gp, i, val64, gp
                    );
                } else {
                    let stk_off = ((i - 8) as u64) * 8;
                    let ea = next_t(&mut tmp);
                    let sp64 = next_t(&mut tmp);
                    let _ = writeln!(
                        out,
                        "  {} = add i64 {}, {}\n  {} = inttoptr i64 {} to i64*\n  store i64 {}, i64* {}, align 8",
                        ea, sp_aligned, stk_off, sp64, ea, val64, sp64
                    );
                }
            }
        }
    }

    // 4. Execute lifted internal entrypoint!
    let _ = writeln!(
        out,
        "  call void @__omni_lifted_{}_sub_{:x}(%OmniCpuState* %st)",
        mod_id, entry_vma
    );

    // 5. Extract return value according to `ret_hint` & `caller_sig.ret_ty`
    let ret64 = match ret_hint {
        CReturnHint::Void => "0".to_string(),
        CReturnHint::Float64 => {
            let vp = next_t(&mut tmp);
            let vv = next_t(&mut tmp);
            let bc = next_t(&mut tmp);
            let e0 = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 0\n  {} = load <16 x i8>, <16 x i8>* {}, align 16\n  {} = bitcast <16 x i8> {} to <2 x i64>\n  {} = extractelement <2 x i64> {}, i32 0",
                vp, vv, vp, bc, vv, e0, bc
            );
            e0
        }
        CReturnHint::Float32 => {
            let vp = next_t(&mut tmp);
            let vv = next_t(&mut tmp);
            let bc = next_t(&mut tmp);
            let f0 = next_t(&mut tmp);
            let d0 = next_t(&mut tmp);
            let i0 = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 1, i32 0\n  {} = load <16 x i8>, <16 x i8>* {}, align 16\n  {} = bitcast <16 x i8> {} to <4 x float>\n  {} = extractelement <4 x float> {}, i32 0\n  {} = fpext float {} to double\n  {} = bitcast double {} to i64",
                vp, vv, vp, bc, vv, f0, bc, d0, f0, i0, d0
            );
            i0
        }
        CReturnHint::Int64OrPtr => {
            let rp = next_t(&mut tmp);
            let rv = next_t(&mut tmp);
            let norm = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 0\n  {} = load i64, i64* {}, align 8\n  {} = call i64 @__omni_norm_ret_ptr_{}(i64 {})",
                rp, rv, rp, norm, mod_id, rv
            );
            norm
        }
        CReturnHint::Unsigned32 => {
            let rp = next_t(&mut tmp);
            let rv = next_t(&mut tmp);
            let norm = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 0\n  {} = load i64, i64* {}, align 8\n  {} = call i64 @__omni_norm_ret_ptr_{}(i64 {})",
                rp, rv, rp, norm, mod_id, rv
            );
            norm
        }
        CReturnHint::Signed32 => {
            // Sign-extend 32-bit `EAX`/`W0` when upper 32 bits are 0 (all 32-bit writes on x86_64/ARM64 zero upper 32 bits!),
            // while preserving full 64-bit pointers if upper 32 bits are non-zero!
            let rp = next_t(&mut tmp);
            let rv = next_t(&mut tmp);
            let hi = next_t(&mut tmp);
            let is32 = next_t(&mut tmp);
            let tr32 = next_t(&mut tmp);
            let sx64 = next_t(&mut tmp);
            let norm = next_t(&mut tmp);
            let sel = next_t(&mut tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 0\n  {} = load i64, i64* {}, align 8\n  {} = lshr i64 {}, 32\n  {} = icmp eq i64 {}, 0\n  {} = trunc i64 {} to i32\n  {} = sext i32 {} to i64\n  {} = call i64 @__omni_norm_ret_ptr_{}(i64 {})\n  {} = select i1 {}, i64 {}, i64 {}",
                rp, rv, rp, hi, rv, is32, hi, tr32, rv, sx64, tr32, norm, mod_id, rv, sel, is32, sx64, norm
            );
            sel
        }
    };

    if let Some(hp) = heap_ptr_opt {
        let _ = writeln!(out, "  call void @free(i8* {})", hp);
    }

    if caller_sig.ret_ty == "i64" {
        let _ = writeln!(out, "  ret i64 {}\n}}\n", ret64);
    } else if caller_sig.ret_ty == "i32" {
        let r32 = next_t(&mut tmp);
        let _ = writeln!(
            out,
            "  {} = trunc i64 {} to i32\n  ret i32 {}\n}}\n",
            r32, ret64, r32
        );
    } else if caller_sig.ret_ty == "void" {
        out.push_str("  ret void\n}\n\n");
    } else {
        let _ = writeln!(out, "  ret {} zeroinitializer\n}}\n", caller_sig.ret_ty);
    }
}

/// Remove superseded `declare ... @sym(...)` and `define weak ... @sym(...)` stubs from `omni_ir`,
/// inject missing external libc/intrinsic `declare`s, and append lifted IR definitions before `@main`!
fn splice_lifted_ir_into_module(
    omni_ir: &mut String,
    internalized_symbols: &HashSet<String>,
    needs_cpu_state_ty: bool,
    extra_decls: &BTreeSet<String>,
    spliced_blocks: &[String],
) {
    let decl_sym_re = Regex::new(r"^declare\s+.*@([A-Za-z0-9_.$]+)\(").unwrap();
    let weak_def_re = Regex::new(r"^define\s+weak\s+.*@([A-Za-z0-9_.$]+)\(").unwrap();

    // Pass 1: Strip `declare ... @<internalized>` and `define weak ... @<internalized> { ... }`
    // while tracking which `@sym` declarations ALREADY exist in `omni_ir`!
    let mut existing_decls = HashSet::new();
    let mut cleaned_lines: Vec<&str> = Vec::new();
    let src_lines: Vec<&str> = omni_ir.lines().collect();
    let mut i = 0usize;
    while i < src_lines.len() {
        let line = src_lines[i];
        let trimmed = line.trim();
        if let Some(caps) = decl_sym_re.captures(trimmed) {
            let sym = &caps[1];
            if internalized_symbols.contains(sym) {
                i += 1;
                continue;
            }
            existing_decls.insert(sym.to_string());
        }
        if let Some(caps) = weak_def_re.captures(trimmed) {
            let sym = &caps[1];
            if internalized_symbols.contains(sym) {
                // Skip entire `define weak ... { ... }` block
                if !trimmed.ends_with('}') {
                    i += 1;
                    while i < src_lines.len() {
                        if src_lines[i].trim() == "}" {
                            break;
                        }
                        i += 1;
                    }
                }
                i += 1;
                continue;
            }
        }
        cleaned_lines.push(line);
        i += 1;
    }

    let mut rebuilt = cleaned_lines.join("\n");
    rebuilt.push('\n');

    // Prepare header block (`%OmniCpuState` + new `declare` / `@__omni_errno_cell_*` lines)
    let mut header_inject = String::new();
    if needs_cpu_state_ty && !rebuilt.contains("%OmniCpuState = type") {
        header_inject.push_str(OMNI_CPU_STATE_TY);
        header_inject.push('\n');
    }
    for d in extra_decls {
        if d.starts_with('@') {
            if !rebuilt.contains(d) {
                header_inject.push_str(d);
                header_inject.push('\n');
            }
        } else if let Some(caps) = decl_sym_re.captures(d) {
            let dsym = &caps[1];
            if !existing_decls.contains(dsym) && !internalized_symbols.contains(dsym) {
                existing_decls.insert(dsym.to_string());
                header_inject.push_str(d);
                header_inject.push('\n');
            }
        }
    }

    let mut body_inject = String::new();
    body_inject.push_str("\n; === Materialized & Lifted C Dependencies (Pure LLVM IR) ===\n");
    for blk in spliced_blocks {
        body_inject.push_str(blk);
        body_inject.push('\n');
    }

    // Insert `header_inject` right after `declare i32 @printf` (or after `target triple`),
    // and insert `body_inject` right before `define i32 @main(`!
    if !header_inject.is_empty() {
        if let Some(pos) = rebuilt.find("declare i32 @printf(i8*, ...)\n") {
            let after = pos + "declare i32 @printf(i8*, ...)\n".len();
            rebuilt.insert_str(after, &header_inject);
        } else if let Some(pos) = rebuilt.find("\n\n") {
            rebuilt.insert_str(pos + 2, &header_inject);
        } else {
            rebuilt = format!("{}\n{}", header_inject, rebuilt);
        }
    }

    if let Some(main_pos) = rebuilt.rfind("define i32 @main(") {
        rebuilt.insert_str(main_pos, &body_inject);
    } else {
        rebuilt.push_str(&body_inject);
    }

    *omni_ir = rebuilt;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shimmer::{TargetArch, TargetPlatform};
    use std::process::Command;

    fn sample_omni_caller_ir(call_expr: &str, decl_line: &str) -> String {
        format!(
            r#"; ModuleID = 'clifter_unit'
declare i8* @malloc(i64)
declare void @free(i8*)
declare i32 @printf(i8*, ...)
{}
@.fmt = private unnamed_addr constant [6 x i8] c"%lld\0A\00", align 1

define i32 @main() {{
entry:
  %res = {}
  %fmt_ptr = getelementptr inbounds [6 x i8], [6 x i8]* @.fmt, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %fmt_ptr, i64 %res)
  ret i32 0
}}
"#,
            decl_line, call_expr
        )
    }

    #[test]
    fn test_tier1_header_inline_and_c_source_ingestion() {
        let Some(clang) = ir_ingest::find_clang_executable() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("omni_clifter_ut1_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let hdr = dir.join("ut_math.h");
        let csrc = dir.join("ut_math.c");
        fs::write(
            &hdr,
            "static inline int ut_inl_scale(int x, int y) { return (x * 11) + y; }\nint ut_c_accum(int n);\n",
        )
        .unwrap();
        fs::write(
            &csrc,
            "#include \"ut_math.h\"\nint ut_c_accum(int n) { int s = 0; for (int i = 1; i <= n; i++) s += i * i; return s; }\n",
        )
        .unwrap();

        let mut ir = r#"; ModuleID = 'ut1'
declare i8* @malloc(i64)
declare void @free(i8*)
declare i32 @printf(i8*, ...)
declare i64 @ut_inl_scale(i64, i64)
declare i64 @ut_c_accum(i64)
@.fmt = private unnamed_addr constant [6 x i8] c"%lld\0A\00", align 1

define i32 @main() {
entry:
  %a = call i64 @ut_inl_scale(i64 6, i64 4)
  %b = call i64 @ut_c_accum(i64 5)
  %sum = add i64 %a, %b
  %fmt_ptr = getelementptr inbounds [6 x i8], [6 x i8]* @.fmt, i64 0, i64 0
  call i32 (i8*, ...) @printf(i8* %fmt_ptr, i64 %sum)
  ret i32 0
}
"#
        .to_string();

        let mut foreign_c = vec![csrc.clone()];
        let all_deps = vec![hdr.clone(), csrc.clone()];
        let cache_dir = dir.join(".omnicache");
        let report = ingest_and_splice_c_dependencies(
            &mut ir,
            &mut foreign_c,
            &all_deps,
            &TargetPlatform::detect_host(),
            &cache_dir,
        );

        assert!(report.internalized_symbols.contains("ut_inl_scale"));
        assert!(report.internalized_symbols.contains("ut_c_accum"));
        assert!(
            foreign_c.is_empty(),
            "Expected foreign_c_files pruned after IR ingestion, got {:?}",
            foreign_c
        );

        let ll_file = dir.join("out.ll");
        let bin_file = dir.join("out_bin");
        fs::write(&ll_file, &ir).unwrap();
        let status = Command::new(&clang)
            .arg("-O2")
            .arg(&ll_file)
            .arg("-o")
            .arg(&bin_file)
            .status()
            .unwrap();
        assert!(status.success());
        let out = Command::new(&bin_file).output().unwrap();
        // ut_inl_scale(6, 4) = 70; ut_c_accum(5) = 1+4+9+16+25 = 55; sum = 125
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "125");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cross_isa_x86_64_and_aarch64_binary_lifting_parity() {
        let Some(clang) = ir_ingest::find_clang_executable() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("omni_clifter_ut2_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let c_src = dir.join("cipher.c");
        fs::write(
            &c_src,
            r#"
static const unsigned int SBOX[4] = { 0x13579BDFu, 0x2468ACE0u, 0xFEDCBA98u, 0x76543210u };
int round_step(int state, int idx) {
    unsigned int u = (unsigned int)state ^ SBOX[idx & 3];
    u = (u << 5) | (u >> 27);
    return (int)(u & 0x7FFFFFFFu);
}
int compute_tag(int seed, int rounds) {
    int st = seed;
    for (int i = 0; i < rounds; i++) {
        st = round_step(st + i, i);
    }
    return st;
}
"#,
        )
        .unwrap();

        let obj_x64 = dir.join("cipher_x64.o");
        let obj_arm64 = dir.join("cipher_arm64.o");
        assert!(Command::new(&clang)
            .args([
                "--target=x86_64-unknown-linux-gnu",
                "-O2",
                "-fPIC",
                "-fno-stack-protector",
                "-c",
            ])
            .arg(&c_src)
            .arg("-o")
            .arg(&obj_x64)
            .status()
            .unwrap()
            .success());
        assert!(Command::new(&clang)
            .args([
                "--target=aarch64-unknown-linux-gnu",
                "-O2",
                "-fPIC",
                "-fno-stack-protector",
                "-c",
            ])
            .arg(&c_src)
            .arg("-o")
            .arg(&obj_arm64)
            .status()
            .unwrap()
            .success());
        let _ = fs::remove_file(&c_src);

        let mut results = Vec::new();
        for (label, obj_path, arch) in [
            ("x86_64", obj_x64, TargetArch::X86_64),
            ("aarch64", obj_arm64, TargetArch::Aarch64),
        ] {
            let mut ir = sample_omni_caller_ir(
                "call i64 @compute_tag(i64 42, i64 12)",
                "declare i64 @compute_tag(i64, i64)",
            );
            let mut foreign_c = vec![obj_path.clone()];
            let all_deps = vec![obj_path.clone()];
            let plat = TargetPlatform::arch(arch);
            let cache_dir = dir.join(format!("cache_{}", label));
            let rep = ingest_and_splice_c_dependencies(
                &mut ir,
                &mut foreign_c,
                &all_deps,
                &plat,
                &cache_dir,
            );
            assert!(
                rep.internalized_symbols.contains("compute_tag"),
                "{} failed to lift compute_tag: {:?}",
                label,
                rep.diagnostics
            );
            assert!(foreign_c.is_empty());

            let ll_out = dir.join(format!("lifted_{}.ll", label));
            let bin_out = dir.join(format!("lifted_{}_bin", label));
            fs::write(&ll_out, &ir).unwrap();
            let st = Command::new(&clang)
                .arg("-O2")
                .arg(&ll_out)
                .arg("-o")
                .arg(&bin_out)
                .status()
                .unwrap();
            assert!(st.success(), "{} lifted LLVM IR failed clang build", label);
            let run = Command::new(&bin_out).output().unwrap();
            assert!(run.status.success());
            results.push(String::from_utf8_lossy(&run.stdout).trim().to_string());
        }

        assert_eq!(results[0], results[1], "x86_64 vs aarch64 lifted IR parity mismatch");
        assert!(!results[0].is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_syscall_boundary_retains_native_fallback() {
        let Some(clang) = ir_ingest::find_clang_executable() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("omni_clifter_ut3_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let c_src = dir.join("raw_sys.c");
        fs::write(
            &c_src,
            "long raw_pid_syscall(void) { long ret; __asm__ volatile(\"syscall\" : \"=a\"(ret) : \"a\"(39) : \"rcx\", \"r11\", \"memory\"); return ret; }\n",
        )
        .unwrap();
        let obj_x64 = dir.join("raw_sys.o");
        assert!(Command::new(&clang)
            .args(["--target=x86_64-unknown-linux-gnu", "-O2", "-c"])
            .arg(&c_src)
            .arg("-o")
            .arg(&obj_x64)
            .status()
            .unwrap()
            .success());

        let mut ir = sample_omni_caller_ir(
            "call i64 @raw_pid_syscall()",
            "declare i64 @raw_pid_syscall()",
        );
        let mut foreign_c = vec![obj_x64.clone()];
        let all_deps = vec![obj_x64.clone()];
        let rep = ingest_and_splice_c_dependencies(
            &mut ir,
            &mut foreign_c,
            &all_deps,
            &TargetPlatform::arch(TargetArch::X86_64),
            &dir.join("cache"),
        );

        assert!(!rep.internalized_symbols.contains("raw_pid_syscall"));
        assert_eq!(
            foreign_c.len(),
            1,
            "Expected kernel syscall object to remain in foreign_c_files for native link fallback"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
