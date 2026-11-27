//! Pure-Rust Cranelift Desktop Backend (`x86_64` / `aarch64`) for OmniScript SSA IR.
//!
//! Lowers `omni_codegen`'s LLVM-syntax SSA IR directly to Cranelift CLIF for:
//! - Zero-`clang` in-process SSA verification (`verify_ir_with_cranelift`)
//! - Native ELF64 / Mach-O / COFF `.o` emission (`compile_ir_to_object`)
//! - Sub-millisecond in-process native JIT execution (`jit_execute_ir`)

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, Function, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, Type, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

use crate::VerifyError;

// ---------------------------------------------------------------------------
// 1. Minimal Parsed LLVM-Syntax SSA Module Representation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct StructLayout {
    field_offsets: Vec<i32>,
    field_types: Vec<String>,
    total_size: u32,
}

#[derive(Debug, Clone)]
struct GlobalConst {
    name: String, // without leading `@`
    bytes: Vec<u8>,
    writable: bool,
    relocs: Vec<(u32, String, i64)>, // (byte_offset, target_sym, byte_addend)
}

#[derive(Debug, Clone)]
struct FnDecl {
    name: String, // without leading `@`
    ret_ty: Option<String>,
    param_tys: Vec<String>,
    is_extern_weak: bool,
}

#[derive(Debug, Clone)]
struct ParsedBlock {
    label: String,
    lines: Vec<(usize, String)>, // (1-based line number, trimmed instruction)
}

#[derive(Debug, Clone)]
struct ParsedFunction {
    name: String,
    ret_ty: Option<String>,
    params: Vec<(String, String)>, // (type_str, param_name_with_%)
    blocks: Vec<ParsedBlock>,
    is_weak: bool,
    start_line: usize,
}

#[derive(Debug, Clone)]
struct ParsedModule {
    structs: BTreeMap<String, StructLayout>,
    globals: Vec<GlobalConst>,
    decls: Vec<FnDecl>,
    funcs: Vec<ParsedFunction>,
}

fn syntax_err(line: usize, msg: impl Into<String>) -> VerifyError {
    let m = msg.into();
    VerifyError::SyntaxError {
        file: "<cranelift-ir>".to_string(),
        line,
        column: 1,
        message: m.clone(),
        raw_stderr: m,
    }
}

/// Strip inline `; ...` comments while respecting double-quoted string literals.
fn strip_llvm_comment(line: &str) -> &str {
    let mut in_str = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if !in_str && bytes[i] == b';' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Split comma-separated items while respecting parentheses `(...)`, brackets `[...]`, and braces `{...}`.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth_paren = 0i32;
    let mut depth_brack = 0i32;
    let mut depth_brace = 0i32;
    let mut in_str = false;
    let mut start = 0usize;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if !in_str {
            match b {
                b'(' => depth_paren += 1,
                b')' => depth_paren -= 1,
                b'[' => depth_brack += 1,
                b']' => depth_brack -= 1,
                b'{' => depth_brace += 1,
                b'}' => depth_brace -= 1,
                b',' if depth_paren == 0 && depth_brack == 0 && depth_brace == 0 => {
                    out.push(s[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    let tail = s[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// Decode LLVM `c"...\00"` string literal escapes (`\XX` hex byte escapes).
fn decode_llvm_c_string(raw: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn llvm_type_size_align(ty: &str, structs: &BTreeMap<String, StructLayout>, ptr_bytes: u32) -> (u32, u32) {
    let t = ty.trim();
    if t.ends_with('*') || t == "ptr" {
        return (ptr_bytes, ptr_bytes);
    }
    if let Some(sname) = t.strip_prefix('%') {
        if let Some(sl) = structs.get(sname) {
            return (sl.total_size.max(8), 16);
        }
        if let Some(stripped) = sname.strip_prefix("struct.") {
            if let Some(sl) = structs.get(stripped) {
                return (sl.total_size.max(8), 8);
            }
        }
        return (64, 8);
    }
    if t.starts_with('<') && t.ends_with('>') {
        let inner = &t[1..t.len() - 1];
        if let Some((cnt_s, elem_s)) = inner.split_once('x') {
            if let Ok(cnt) = cnt_s.trim().parse::<u32>() {
                let (es, _) = llvm_type_size_align(elem_s.trim(), structs, ptr_bytes);
                let total = cnt.saturating_mul(es).max(1);
                return (total, total.min(16));
            }
        }
    }
    if t.starts_with('[') && t.ends_with(']') {
        let inner = &t[1..t.len() - 1];
        if let Some((cnt_s, elem_s)) = inner.split_once('x') {
            if let Ok(cnt) = cnt_s.trim().parse::<u32>() {
                let (es, ea) = llvm_type_size_align(elem_s.trim(), structs, ptr_bytes);
                return (cnt.saturating_mul(es).max(1), ea);
            }
        }
    }
    match t {
        "i1" | "i8" => (1, 1),
        "i16" => (2, 2),
        "i32" | "float" => (4, 4),
        "i64" | "double" => (8, 8),
        _ => (ptr_bytes, ptr_bytes),
    }
}

fn llvm_type_to_clif(ty: &str, ptr_ty: Type) -> Option<Type> {
    let t = ty.trim();
    if t == "void" {
        return None;
    }
    if t.ends_with('*') || t == "ptr" {
        return Some(ptr_ty);
    }
    match t {
        "i1" | "i8" => Some(types::I8),
        "i16" => Some(types::I16),
        "i32" => Some(types::I32),
        "i64" => Some(types::I64),
        "float" => Some(types::F32),
        "double" => Some(types::F64),
        _ => Some(ptr_ty),
    }
}

/// Parse an LLVM IR text module into `ParsedModule`.
fn parse_llvm_module(ir: &str, ptr_bytes: u32) -> Result<ParsedModule, VerifyError> {
    let mut structs: BTreeMap<String, StructLayout> = BTreeMap::new();
    let mut globals: Vec<GlobalConst> = Vec::new();
    let mut decls: Vec<FnDecl> = Vec::new();
    let mut funcs: Vec<ParsedFunction> = Vec::new();

    let lines: Vec<&str> = ir.lines().collect();
    let mut i = 0usize;

    while i < lines.len() {
        let line_no = i + 1;
        let trimmed = strip_llvm_comment(lines[i]).trim();
        i += 1;
        if trimmed.is_empty()
            || trimmed.starts_with("source_filename")
            || trimmed.starts_with("target ")
            || trimmed.starts_with("attributes ")
        {
            continue;
        }

        // 1. Named struct layout: `%struct.Foo = type { ... }` OR `%OmniCpuState = type { ... }`
        if let Some(rest) = trimmed.strip_prefix('%') {
            if let Some((sname_raw, rhs)) = rest.split_once(" = type ") {
                let sname = sname_raw.trim().to_string();
                let body = rhs.trim().trim_start_matches('{').trim_end_matches('}').trim();
                let mut field_offsets = Vec::new();
                let mut field_types = Vec::new();
                let mut offset: u32 = 0;
                if !body.is_empty() {
                    for fty in split_top_level_commas(body) {
                        let (sz, align) = llvm_type_size_align(fty, &structs, ptr_bytes);
                        let align = align.max(1);
                        offset = (offset + align - 1) & !(align - 1);
                        field_offsets.push(offset as i32);
                        field_types.push(fty.trim().to_string());
                        offset += sz;
                    }
                }
                let total_size = ((offset + 15) & !15).max(8);
                let layout = StructLayout {
                    field_offsets,
                    field_types,
                    total_size,
                };
                if let Some(stripped) = sname.strip_prefix("struct.") {
                    structs.insert(stripped.to_string(), layout.clone());
                }
                structs.insert(sname, layout);
            }
            continue;
        }

        // 2. Global string/byte constant or mutable byte array:
        // `@.str.0 = private unnamed_addr constant [6 x i8] c"hello\00", align 1`
        // `@__omni_lifted_img_... = internal global [N x i8] c"...", align 16`
        if trimmed.starts_with('@') && (trimmed.contains(" constant ") || trimmed.contains(" global ")) {
            let writable = trimmed.contains(" global ");
            if let Some((lhs, rhs)) = trimmed.split_once('=') {
                let gname = lhs.trim().trim_start_matches('@').to_string();
                if let Some(c_pos) = rhs.find(" c\"") {
                    let after_q = &rhs[c_pos + 3..];
                    if let Some(end_q) = after_q.find('"') {
                        let raw_s = &after_q[..end_q];
                        globals.push(GlobalConst {
                            name: gname,
                            bytes: decode_llvm_c_string(raw_s),
                            writable,
                            relocs: Vec::new(),
                        });
                    }
                } else {
                    // Scalar, array, pointer, or zeroinitializer global
                    let after_kw = rhs
                        .split_once(" global ")
                        .or_else(|| rhs.split_once(" constant "))
                        .map(|(_, r)| r.trim())
                        .unwrap_or("i64 0");
                    // Strip trailing `, align N` only at top-level bracket/paren depth 0
                    let head = {
                        let mut depth = 0i32;
                        let mut cut = after_kw.len();
                        for (idx, ch) in after_kw.char_indices() {
                            match ch {
                                '[' | '(' | '{' => depth += 1,
                                ']' | ')' | '}' => depth -= 1,
                                ',' if depth == 0 => {
                                    cut = idx;
                                    break;
                                }
                                _ => {}
                            }
                        }
                        after_kw[..cut].trim()
                    };
                    // Split leading LLVM type from initializer at top-level space
                    let (ty_s, init_s) = if head.starts_with('[') {
                        if let Some(rb) = head.find(']') {
                            (head[..=rb].trim(), head[rb + 1..].trim())
                        } else {
                            (head, "zeroinitializer")
                        }
                    } else if let Some(star_fn) = head.find(")*") {
                        (head[..star_fn + 2].trim(), head[star_fn + 2..].trim())
                    } else {
                        head.split_once(' ').unwrap_or((head, "0"))
                    };
                    let (sz, _) = llvm_type_size_align(ty_s.trim(), &structs, ptr_bytes);
                    let mut bytes = vec![0u8; sz.max(4) as usize];
                    let mut relocs = Vec::new();
                    let init_trim = init_s.trim();
                    if ty_s.trim().starts_with('[') && init_trim.starts_with('[') && init_trim.ends_with(']') {
                        // Array literal: `[ i32 10, i32 20, ... ]`
                        let inner = &init_trim[1..init_trim.len() - 1];
                        let mut byte_off = 0usize;
                        for elem in split_top_level_commas(inner) {
                            let et = elem.trim();
                            if et.is_empty() {
                                continue;
                            }
                            let (ety, eval) = et.split_once(' ').unwrap_or(("i32", et));
                            let (esz, _) = llvm_type_size_align(ety.trim(), &structs, ptr_bytes);
                            let esz_u = esz.max(1) as usize;
                            if let Ok(iv) = eval.trim().parse::<i64>() {
                                let le = iv.to_le_bytes();
                                let n = esz_u.min(8).min(bytes.len().saturating_sub(byte_off));
                                bytes[byte_off..byte_off + n].copy_from_slice(&le[..n]);
                            }
                            byte_off += esz_u;
                        }
                    } else if let Some(sym) = init_trim.strip_prefix('@') {
                        // Direct symbol pointer: `@op_add` or `@g_var`
                        let clean_sym = sym.split_whitespace().next().unwrap_or(sym).to_string();
                        relocs.push((0, clean_sym, 0));
                    } else if init_trim.starts_with("getelementptr") {
                        // Constant GEP: `getelementptr inbounds (i32, i32* getelementptr inbounds ([4 x i32], [4 x i32]* @g_vals, i64 0, i64 0), i64 2)`
                        if let Some(at_pos) = init_trim.find('@') {
                            let after_at = &init_trim[at_pos + 1..];
                            let sym: String = after_at
                                .chars()
                                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                                .collect();
                            // Extract outer element stride & trailing index if present
                            let mut addend: i64 = 0;
                            if let (Some(lp), Some(rp)) = (init_trim.find('('), init_trim.rfind(')')) {
                                let outer_args = split_top_level_commas(&init_trim[lp + 1..rp]);
                                if outer_args.len() >= 3 {
                                    let outer_ty = outer_args[0].trim();
                                    let last_idx = outer_args.last().unwrap().trim();
                                    if let Some((_, idx_s)) = last_idx.split_once(' ') {
                                        if let Ok(idx_v) = idx_s.trim().parse::<i64>() {
                                            let (stride, _) = llvm_type_size_align(outer_ty, &structs, ptr_bytes);
                                            addend = idx_v * (stride as i64);
                                        }
                                    }
                                }
                            }
                            relocs.push((0, sym, addend));
                        }
                    } else if let Ok(iv) = init_trim.parse::<i64>() {
                        let le = iv.to_le_bytes();
                        let n = bytes.len().min(8);
                        bytes[..n].copy_from_slice(&le[..n]);
                    }
                    globals.push(GlobalConst {
                        name: gname,
                        bytes,
                        writable,
                        relocs,
                    });
                }
            }
            continue;
        }

        // 3. Function declaration: `declare [extern_weak] [ret] @name(args...)`
        if let Some(rest) = trimmed.strip_prefix("declare ") {
            let (is_extern_weak, rest) = if let Some(r) = rest.strip_prefix("extern_weak ") {
                (true, r.trim())
            } else {
                (false, rest)
            };
            let at_pos = rest
                .find('@')
                .ok_or_else(|| syntax_err(line_no, format!("Malformed declare: {}", trimmed)))?;
            let ret_raw = rest[..at_pos].trim();
            let ret_ty = if ret_raw == "void" {
                None
            } else {
                Some(ret_raw.to_string())
            };
            let after_at = &rest[at_pos + 1..];
            let lp = after_at
                .find('(')
                .ok_or_else(|| syntax_err(line_no, format!("Missing '(' in declare: {}", trimmed)))?;
            let rp = after_at
                .rfind(')')
                .ok_or_else(|| syntax_err(line_no, format!("Missing ')' in declare: {}", trimmed)))?;
            let fname = after_at[..lp].trim().to_string();
            let params_str = after_at[lp + 1..rp].trim();
            let mut param_tys = Vec::new();
            if !params_str.is_empty() {
                for p in split_top_level_commas(params_str) {
                    if p != "..." {
                        // Strip optional parameter name (`i64 %a0` -> `i64`)
                        let pty = p
                            .split_whitespace()
                            .take_while(|tok| !tok.starts_with('%'))
                            .filter(|tok| !matches!(*tok, "noundef" | "nocapture" | "readonly" | "signext" | "zeroext"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !pty.is_empty() {
                            param_tys.push(pty);
                        }
                    }
                }
            }
            let is_crt_weak = matches!(
                fname.as_str(),
                "ITM_registerTMCloneTable"
                    | "_ITM_registerTMCloneTable"
                    | "ITM_deregisterTMCloneTable"
                    | "_ITM_deregisterTMCloneTable"
                    | "__gmon_start__"
                    | "_Jv_RegisterClasses"
            );
            decls.push(FnDecl {
                name: fname,
                ret_ty,
                param_tys,
                is_extern_weak: is_extern_weak || is_crt_weak,
            });
            continue;
        }

        // 4. Function definition: `define [weak] [ret] @name(params...) {`
        if let Some(rest) = trimmed.strip_prefix("define ") {
            let (is_weak, rest) = if let Some(r) = rest.strip_prefix("weak ") {
                (true, r)
            } else if let Some(r) = rest.strip_prefix("internal ") {
                (false, r)
            } else {
                (false, rest)
            };
            let at_pos = rest
                .find('@')
                .ok_or_else(|| syntax_err(line_no, format!("Malformed define: {}", trimmed)))?;
            let ret_raw = rest[..at_pos].trim();
            let ret_ty = if ret_raw == "void" {
                None
            } else {
                Some(ret_raw.to_string())
            };
            let after_at = &rest[at_pos + 1..];
            let lp = after_at
                .find('(')
                .ok_or_else(|| syntax_err(line_no, format!("Missing '(' in define: {}", trimmed)))?;
            let rp = after_at
                .rfind(')')
                .ok_or_else(|| syntax_err(line_no, format!("Missing ')' in define: {}", trimmed)))?;
            let fname = after_at[..lp].trim().to_string();
            let params_str = after_at[lp + 1..rp].trim();
            let mut params = Vec::new();
            if !params_str.is_empty() {
                for (pidx, p) in split_top_level_commas(params_str).into_iter().enumerate() {
                    let toks: Vec<&str> = p.split_whitespace().collect();
                    if let Some(last) = toks.last() {
                        if last.starts_with('%') {
                            let pty = toks[..toks.len() - 1].join(" ");
                            params.push((pty, (*last).to_string()));
                        } else {
                            params.push((p.to_string(), format!("%{}", pidx)));
                        }
                    }
                }
            }

            let mut blocks: Vec<ParsedBlock> = Vec::new();
            let mut cur_block: Option<ParsedBlock> = None;
            let mut closed = false;

            while i < lines.len() {
                let bline_no = i + 1;
                let bt = strip_llvm_comment(lines[i]).trim();
                i += 1;
                if bt.is_empty() {
                    continue;
                }
                if bt == "}" {
                    if let Some(b) = cur_block.take() {
                        blocks.push(b);
                    }
                    closed = true;
                    break;
                }
                // Label line: `entry:` or `loop_body:`
                if bt.ends_with(':') && !bt.contains(' ') && !bt.starts_with('%') {
                    if let Some(b) = cur_block.take() {
                        blocks.push(b);
                    }
                    cur_block = Some(ParsedBlock {
                        label: bt.trim_end_matches(':').to_string(),
                        lines: Vec::new(),
                    });
                } else {
                    if cur_block.is_none() {
                        cur_block = Some(ParsedBlock {
                            label: "entry".to_string(),
                            lines: Vec::new(),
                        });
                    }
                    // Join multi-line `switch ... [ ... ]` into a single logical instruction
                    if bt.starts_with("switch ") && bt.contains('[') && !bt.contains(']') {
                        let mut joined = bt.to_string();
                        while i < lines.len() {
                            let st = strip_llvm_comment(lines[i]).trim();
                            i += 1;
                            if !st.is_empty() {
                                joined.push(' ');
                                joined.push_str(st);
                            }
                            if st.contains(']') {
                                break;
                            }
                        }
                        cur_block
                            .as_mut()
                            .unwrap()
                            .lines
                            .push((bline_no, joined));
                    } else {
                        cur_block
                            .as_mut()
                            .unwrap()
                            .lines
                            .push((bline_no, bt.to_string()));
                    }
                }
            }

            if !closed {
                return Err(syntax_err(
                    line_no,
                    format!("Unclosed function definition '@{}' (missing '}}')", fname),
                ));
            }
            if blocks.is_empty() {
                return Err(syntax_err(
                    line_no,
                    format!("Function '@{}' has no basic blocks", fname),
                ));
            }
            funcs.push(ParsedFunction {
                name: fname,
                ret_ty,
                params,
                blocks,
                is_weak,
                start_line: line_no,
            });
            continue;
        }
    }

    Ok(ParsedModule {
        structs,
        globals,
        decls,
        funcs,
    })
}

// ---------------------------------------------------------------------------
// 2. Lowering Parsed SSA Functions into Cranelift CLIF
// ---------------------------------------------------------------------------

struct LowerCtx<'a, M: Module> {
    module: &'a mut M,
    parsed: &'a ParsedModule,
    ptr_ty: Type,
    ptr_bytes: u32,
    call_conv: CallConv,
    func_ids: &'a HashMap<String, (FuncId, Signature)>,
    data_ids: &'a HashMap<String, DataId>,
    // Per-function state:
    vars: HashMap<String, (Variable, Type)>,
    next_var: u32,
    blocks: HashMap<String, Block>,
    /// For each target block label: list of `(phi_dest_reg, phi_clif_ty, Vec<(pred_label, val_expr)>)`
    phi_incomings: HashMap<String, Vec<(String, Type, Vec<(String, String)>)>>,
}

impl<'a, M: Module> LowerCtx<'a, M> {
    fn get_or_declare_var(
        &mut self,
        builder: &mut FunctionBuilder,
        name: &str,
        ty: Type,
    ) -> Variable {
        if let Some((v, _)) = self.vars.get(name) {
            return *v;
        }
        let v = Variable::from_u32(self.next_var);
        self.next_var += 1;
        builder.declare_var(v, ty);
        self.vars.insert(name.to_string(), (v, ty));
        v
    }

    fn coerce_val(
        &self,
        builder: &mut FunctionBuilder,
        val: Value,
        expected: Type,
    ) -> Value {
        let actual = builder.func.dfg.value_type(val);
        if actual == expected {
            return val;
        }
        if actual.is_int() && expected.is_int() {
            if expected.bits() > actual.bits() {
                return builder.ins().sextend(expected, val);
            } else {
                return builder.ins().ireduce(expected, val);
            }
        }
        if actual == types::I64 && expected == types::F64 {
            return builder.ins().bitcast(types::F64, MemFlags::new(), val);
        }
        if actual == types::F64 && expected == types::I64 {
            return builder.ins().bitcast(types::I64, MemFlags::new(), val);
        }
        val
    }

    /// Resolve an LLVM operand (`%reg`, integer literal, float literal, `null`, `true`/`false`, `@global`, inline GEP).
    fn eval_operand(
        &mut self,
        builder: &mut FunctionBuilder,
        raw: &str,
        expected_ty: Type,
        line_no: usize,
    ) -> Result<Value, VerifyError> {
        let s = raw.trim();

        // Inline constant GEP expression: `getelementptr inbounds ([N x i8], [N x i8]* @.str.0, i64 0, i64 0)`
        if s.starts_with("getelementptr ") {
            if let Some(at_pos) = s.find('@') {
                let after = &s[at_pos + 1..];
                let gname: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if let Some(&did) = self.data_ids.get(&gname) {
                    let gv = self.module.declare_data_in_func(did, builder.func);
                    let ptr = builder.ins().symbol_value(self.ptr_ty, gv);
                    return Ok(self.coerce_val(builder, ptr, expected_ty));
                }
            }
            return Ok(builder.ins().iconst(expected_ty, 0));
        }

        // Inline `bitcast (...)` or `ptrtoint (...)` constant expression
        if (s.starts_with("bitcast (") || s.starts_with("ptrtoint (")) && s.contains('@') {
            if let Some(at_pos) = s.find('@') {
                let after = &s[at_pos + 1..];
                let sym: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if let Some((fid, _)) = self.func_ids.get(&sym) {
                    let fref = self.module.declare_func_in_func(*fid, builder.func);
                    let addr = builder.ins().func_addr(self.ptr_ty, fref);
                    return Ok(self.coerce_val(builder, addr, expected_ty));
                }
                if let Some(&did) = self.data_ids.get(&sym) {
                    let gv = self.module.declare_data_in_func(did, builder.func);
                    let addr = builder.ins().symbol_value(self.ptr_ty, gv);
                    return Ok(self.coerce_val(builder, addr, expected_ty));
                }
            }
        }

        if s.starts_with('%') {
            if let Some(&(var, _)) = self.vars.get(s) {
                let v = builder.use_var(var);
                return Ok(self.coerce_val(builder, v, expected_ty));
            }
            return Err(syntax_err(
                line_no,
                format!("use of undefined value '{}'", s),
            ));
        }

        if let Some(sym) = s.strip_prefix('@') {
            if let Some(&did) = self.data_ids.get(sym) {
                let gv = self.module.declare_data_in_func(did, builder.func);
                let addr = builder.ins().symbol_value(self.ptr_ty, gv);
                return Ok(self.coerce_val(builder, addr, expected_ty));
            }
            if let Some((fid, _)) = self.func_ids.get(sym) {
                let fref = self.module.declare_func_in_func(*fid, builder.func);
                let addr = builder.ins().func_addr(self.ptr_ty, fref);
                return Ok(self.coerce_val(builder, addr, expected_ty));
            }
            return Err(syntax_err(
                line_no,
                format!("Undefined global or function symbol '@{}'", sym),
            ));
        }

        match s {
            "null" | "undef" | "poison" | "zeroinitializer" => {
                if expected_ty == types::F64 {
                    return Ok(builder.ins().f64const(0.0));
                } else if expected_ty == types::F32 {
                    return Ok(builder.ins().f32const(0.0));
                }
                return Ok(builder.ins().iconst(expected_ty, 0));
            }
            "true" => return Ok(builder.ins().iconst(expected_ty, 1)),
            "false" => return Ok(builder.ins().iconst(expected_ty, 0)),
            _ => {}
        }

        if expected_ty == types::F64 {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                if let Ok(bits) = u64::from_str_radix(hex, 16) {
                    return Ok(builder.ins().f64const(f64::from_bits(bits)));
                }
            }
            if let Ok(fv) = s.parse::<f64>() {
                return Ok(builder.ins().f64const(fv));
            }
        } else if expected_ty == types::F32 {
            if let Ok(fv) = s.parse::<f32>() {
                return Ok(builder.ins().f32const(fv));
            }
        }

        if let Ok(iv) = s.parse::<i64>() {
            return Ok(builder.ins().iconst(expected_ty, iv));
        }
        if let Ok(uv) = s.parse::<u64>() {
            return Ok(builder.ins().iconst(expected_ty, uv as i64));
        }

        Err(syntax_err(
            line_no,
            format!("Cannot parse operand '{}' for type {}", s, expected_ty),
        ))
    }

    fn build_branch_args(
        &mut self,
        builder: &mut FunctionBuilder,
        cur_label: &str,
        target_label: &str,
        line_no: usize,
    ) -> Result<Vec<Value>, VerifyError> {
        let phis = match self.phi_incomings.get(target_label) {
            Some(v) => v.clone(),
            None => return Ok(Vec::new()),
        };
        let mut args = Vec::with_capacity(phis.len());
        for (_dest, ty, pairs) in phis {
            let expr = pairs
                .iter()
                .find(|(pred, _)| pred == cur_label)
                .map(|(_, val)| val.as_str())
                .unwrap_or("0");
            let v = self.eval_operand(builder, expr, ty, line_no)?;
            args.push(v);
        }
        Ok(args)
    }

    fn lower_function(
        &mut self,
        pfn: &ParsedFunction,
        func: &mut Function,
        fb_ctx: &mut FunctionBuilderContext,
    ) -> Result<(), VerifyError> {
        self.vars.clear();
        self.next_var = 0;
        self.blocks.clear();
        self.phi_incomings.clear();

        let mut builder = FunctionBuilder::new(func, fb_ctx);

        // Pass 1: Create all Cranelift blocks & pre-parse `phi` nodes as block params
        for pb in &pfn.blocks {
            if self.blocks.contains_key(&pb.label) {
                return Err(syntax_err(
                    pfn.start_line,
                    format!("Duplicate basic block label '%{}' in '@{}'", pb.label, pfn.name),
                ));
            }
            let blk = builder.create_block();
            self.blocks.insert(pb.label.clone(), blk);
        }

        // Attach function parameters to entry block
        let entry_blk = self.blocks[&pfn.blocks[0].label];
        builder.append_block_params_for_function_params(entry_blk);

        // Pre-scan `phi` nodes in every block
        for pb in &pfn.blocks {
            let blk = self.blocks[&pb.label];
            for (line_no, inst) in &pb.lines {
                let Some((lhs, rhs)) = inst.split_once(" = ") else {
                    continue;
                };
                let rhs = rhs.trim();
                let Some(phi_rest) = rhs.strip_prefix("phi ") else {
                    continue;
                };
                let dest = lhs.trim().to_string();
                let brk = phi_rest
                    .find('[')
                    .ok_or_else(|| syntax_err(*line_no, format!("Malformed phi: {}", inst)))?;
                let ty_s = phi_rest[..brk].trim();
                let clif_ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
                let mut pairs = Vec::new();
                let mut cursor = &phi_rest[brk..];
                while let Some(open) = cursor.find('[') {
                    let after_open = &cursor[open + 1..];
                    let close = after_open
                        .find(']')
                        .ok_or_else(|| syntax_err(*line_no, format!("Unclosed '[' in phi: {}", inst)))?;
                    let pair_str = &after_open[..close];
                    let (val_s, pred_s) = pair_str
                        .rsplit_once(',')
                        .ok_or_else(|| syntax_err(*line_no, format!("Malformed phi pair: {}", pair_str)))?;
                    pairs.push((
                        pred_s.trim().trim_start_matches('%').to_string(),
                        val_s.trim().to_string(),
                    ));
                    cursor = &after_open[close + 1..];
                }
                builder.append_block_param(blk, clif_ty);
                self.phi_incomings
                    .entry(pb.label.clone())
                    .or_default()
                    .push((dest, clif_ty, pairs));
            }
        }

        // Bind entry block function params to variables
        builder.switch_to_block(entry_blk);
        for (pidx, (pty, pname)) in pfn.params.iter().enumerate() {
            let clif_ty = llvm_type_to_clif(pty, self.ptr_ty).unwrap_or(types::I64);
            let pval = builder.block_params(entry_blk)[pidx];
            let var = self.get_or_declare_var(&mut builder, pname, clif_ty);
            builder.def_var(var, pval);
        }

        // Pass 2: Lower each basic block
        for (bidx, pb) in pfn.blocks.iter().enumerate() {
            let blk = self.blocks[&pb.label];
            if bidx > 0 {
                builder.switch_to_block(blk);
            }

            // Bind phi block params to their SSA destination variables
            if let Some(phis) = self.phi_incomings.get(&pb.label).cloned() {
                let param_offset = if bidx == 0 { pfn.params.len() } else { 0 };
                for (pidx, (dest, ty, _)) in phis.into_iter().enumerate() {
                    let pval = builder.block_params(blk)[param_offset + pidx];
                    let var = self.get_or_declare_var(&mut builder, &dest, ty);
                    builder.def_var(var, pval);
                }
            }

            let mut terminated = false;
            for (line_no, inst) in &pb.lines {
                if terminated {
                    break;
                }
                terminated = self.lower_instruction(&mut builder, pfn, &pb.label, inst, *line_no)?;
            }

            if !terminated {
                // Fail-closed: every basic block in valid SSA IR must end with a terminator
                return Err(syntax_err(
                    pb.lines.last().map(|(l, _)| *l).unwrap_or(pfn.start_line),
                    format!(
                        "Basic block '%{}' in '@{}' is missing a terminator instruction",
                        pb.label, pfn.name
                    ),
                ));
            }
        }

        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    fn lower_instruction(
        &mut self,
        builder: &mut FunctionBuilder,
        pfn: &ParsedFunction,
        cur_label: &str,
        inst: &str,
        line_no: usize,
    ) -> Result<bool, VerifyError> {
        let s = inst.trim();

        // Terminators
        if s == "ret void" {
            builder.ins().return_(&[]);
            return Ok(true);
        }
        if let Some(rest) = s.strip_prefix("ret ") {
            let (ty_s, val_s) = split_type_and_operand(rest)
                .ok_or_else(|| syntax_err(line_no, format!("Malformed ret: {}", s)))?;
            let expected = pfn
                .ret_ty
                .as_deref()
                .and_then(|t| llvm_type_to_clif(t, self.ptr_ty))
                .or_else(|| llvm_type_to_clif(ty_s, self.ptr_ty))
                .unwrap_or(types::I64);
            let v = self.eval_operand(builder, val_s, expected, line_no)?;
            builder.ins().return_(&[v]);
            return Ok(true);
        }
        if s == "unreachable" {
            builder.ins().trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            return Ok(true);
        }
        if let Some(rest) = s.strip_prefix("br label %") {
            let target = rest.trim();
            let dst_blk = *self.blocks.get(target).ok_or_else(|| {
                syntax_err(line_no, format!("Undefined branch target '%{}'", target))
            })?;
            let args = self.build_branch_args(builder, cur_label, target, line_no)?;
            builder.ins().jump(dst_blk, &args);
            return Ok(true);
        }
        if let Some(rest) = s.strip_prefix("switch ") {
            let brk_open = rest
                .find('[')
                .ok_or_else(|| syntax_err(line_no, format!("Malformed switch (missing '['): {}", s)))?;
            let brk_close = rest
                .rfind(']')
                .ok_or_else(|| syntax_err(line_no, format!("Malformed switch (missing ']'): {}", s)))?;
            let header = rest[..brk_open].trim();
            let cases_str = rest[brk_open + 1..brk_close].trim();
            let (val_part, def_part) = header
                .split_once(',')
                .ok_or_else(|| syntax_err(line_no, format!("Malformed switch header: {}", s)))?;
            let (ty_s, val_op) = split_type_and_operand(val_part)
                .ok_or_else(|| syntax_err(line_no, format!("Malformed switch value: {}", s)))?;
            let def_lbl = def_part
                .trim()
                .strip_prefix("label %")
                .ok_or_else(|| syntax_err(line_no, format!("Malformed switch default label: {}", s)))?;
            let def_blk = *self
                .blocks
                .get(def_lbl)
                .ok_or_else(|| syntax_err(line_no, format!("Undefined switch default '%{}'", def_lbl)))?;
            let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
            let discr = self.eval_operand(builder, val_op, ty, line_no)?;

            let mut sw = Switch::new();
            // Parse pairs of `<ty> <imm>, label %target`
            let mut toks = cases_str.split_whitespace().peekable();
            while let Some(_cty) = toks.next() {
                let imm_tok = toks.next().unwrap_or("0").trim_end_matches(',');
                let _lbl_kw = toks.next(); // "label"
                let tgt_tok = toks.next().unwrap_or("").trim_start_matches('%');
                if tgt_tok.is_empty() {
                    break;
                }
                let tgt_blk = *self.blocks.get(tgt_tok).ok_or_else(|| {
                    syntax_err(line_no, format!("Undefined switch case target '%{}'", tgt_tok))
                })?;
                let imm_val: u128 = if let Ok(v) = imm_tok.parse::<i64>() {
                    (v as u64) as u128
                } else if let Ok(v) = imm_tok.parse::<u64>() {
                    v as u128
                } else {
                    0
                };
                sw.set_entry(imm_val, tgt_blk);
            }
            sw.emit(builder, discr, def_blk);
            return Ok(true);
        }
        if let Some(rest) = s.strip_prefix("br i1 ") {
            let parts = split_top_level_commas(rest);
            if parts.len() != 3 {
                return Err(syntax_err(line_no, format!("Malformed conditional br: {}", s)));
            }
            let cond = self.eval_operand(builder, parts[0], types::I8, line_no)?;
            let t_lbl = parts[1].trim().strip_prefix("label %").ok_or_else(|| {
                syntax_err(line_no, format!("Malformed true label in br: {}", s))
            })?;
            let f_lbl = parts[2].trim().strip_prefix("label %").ok_or_else(|| {
                syntax_err(line_no, format!("Malformed false label in br: {}", s))
            })?;
            let t_blk = *self
                .blocks
                .get(t_lbl)
                .ok_or_else(|| syntax_err(line_no, format!("Undefined branch target '%{}'", t_lbl)))?;
            let f_blk = *self
                .blocks
                .get(f_lbl)
                .ok_or_else(|| syntax_err(line_no, format!("Undefined branch target '%{}'", f_lbl)))?;
            let t_args = self.build_branch_args(builder, cur_label, t_lbl, line_no)?;
            let f_args = self.build_branch_args(builder, cur_label, f_lbl, line_no)?;
            builder.ins().brif(cond, t_blk, &t_args, f_blk, &f_args);
            return Ok(true);
        }

        // `store <ty> <val>, <ty>* <ptr>`
        if let Some(rest) = s.strip_prefix("store ") {
            let parts = split_top_level_commas(rest);
            if parts.len() < 2 {
                return Err(syntax_err(line_no, format!("Malformed store: {}", s)));
            }
            let (val_ty_s, val_op) = split_type_and_operand(parts[0])
                .ok_or_else(|| syntax_err(line_no, format!("Malformed store value: {}", s)))?;
            let (_, ptr_op) = split_type_and_operand(parts[1])
                .ok_or_else(|| syntax_err(line_no, format!("Malformed store ptr: {}", s)))?;
            let vty = llvm_type_to_clif(val_ty_s, self.ptr_ty).unwrap_or(types::I64);
            let val = self.eval_operand(builder, val_op, vty, line_no)?;
            let ptr = self.eval_operand(builder, ptr_op, self.ptr_ty, line_no)?;
            builder.ins().store(MemFlags::new(), val, ptr, 0);
            return Ok(false);
        }

        // Void call: `call void @foo(...)`
        if s.starts_with("call ") {
            self.lower_call(builder, s, None, line_no)?;
            return Ok(false);
        }

        // Assignment instruction: `%dst = <op> ...`
        if let Some((lhs, rhs)) = s.split_once(" = ") {
            let dst = lhs.trim();
            let rhs = rhs.trim();

            if rhs.starts_with("phi ") {
                // Already handled via block parameters at block entry
                return Ok(false);
            }

            if rhs.starts_with("call ") {
                self.lower_call(builder, rhs, Some(dst), line_no)?;
                return Ok(false);
            }

            // `alloca <ty>[, align N]`
            if let Some(rest) = rhs.strip_prefix("alloca ") {
                let parts = split_top_level_commas(rest);
                let ty_s = parts.first().copied().unwrap_or("i64").trim();
                let (sz, align) = llvm_type_size_align(ty_s, &self.parsed.structs, self.ptr_bytes);
                let align_shift = align.max(1).trailing_zeros().min(4) as u8;
                let slot = builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    sz.max(8),
                    align_shift,
                ));
                let addr = builder.ins().stack_addr(self.ptr_ty, slot, 0);
                let var = self.get_or_declare_var(builder, dst, self.ptr_ty);
                builder.def_var(var, addr);
                return Ok(false);
            }

            // `load <ty>, <ty>* <ptr>[, align N]`
            if let Some(rest) = rhs.strip_prefix("load ") {
                let parts = split_top_level_commas(rest);
                if parts.len() < 2 {
                    return Err(syntax_err(line_no, format!("Malformed load: {}", s)));
                }
                let ty_s = parts[0].trim();
                let (_, ptr_op) = split_type_and_operand(parts[1])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed load ptr: {}", s)))?;
                let clif_ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
                let ptr = self.eval_operand(builder, ptr_op, self.ptr_ty, line_no)?;
                let loaded = builder.ins().load(clif_ty, MemFlags::new(), ptr, 0);
                let var = self.get_or_declare_var(builder, dst, clif_ty);
                builder.def_var(var, loaded);
                return Ok(false);
            }

            // `getelementptr [inbounds] <base_ty>, <base_ty>* <ptr>, <idx0>[, <idx1>...]`
            if let Some(rest) = rhs.strip_prefix("getelementptr ") {
                let mut rest = rest.strip_prefix("inbounds ").unwrap_or(rest).trim();
                if rest.starts_with('(') && rest.ends_with(')') {
                    rest = &rest[1..rest.len() - 1];
                }
                let parts = split_top_level_commas(rest);
                if parts.len() < 3 {
                    return Err(syntax_err(line_no, format!("Malformed getelementptr: {}", s)));
                }
                let base_ty = parts[0].trim();
                let (_, ptr_op) = split_type_and_operand(parts[1])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed GEP ptr: {}", s)))?;
                let mut cur_ptr = self.eval_operand(builder, ptr_op, self.ptr_ty, line_no)?;

                // Index 0 steps by sizeof(base_ty)
                let (_, idx0_op) = split_type_and_operand(parts[2])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed GEP idx0: {}", s)))?;
                if idx0_op != "0" {
                    let (base_sz, _) =
                        llvm_type_size_align(base_ty, &self.parsed.structs, self.ptr_bytes);
                    let idx0 = self.eval_operand(builder, idx0_op, self.ptr_ty, line_no)?;
                    let off = builder.ins().imul_imm(idx0, base_sz as i64);
                    cur_ptr = builder.ins().iadd(cur_ptr, off);
                }

                // Walk all subsequent aggregate indices (`parts[3..]`) through structs & nested arrays
                let mut cur_ty = base_ty.to_string();
                for idx_part in &parts[3..] {
                    let (_, idx_op) = split_type_and_operand(idx_part)
                        .ok_or_else(|| syntax_err(line_no, format!("Malformed GEP idx: {}", s)))?;
                    if let Some(sname) = cur_ty.strip_prefix('%') {
                        let sl_opt = self
                            .parsed
                            .structs
                            .get(sname)
                            .or_else(|| sname.strip_prefix("struct.").and_then(|st| self.parsed.structs.get(st)));
                        if let (Some(sl), Ok(fidx)) = (sl_opt, idx_op.parse::<usize>()) {
                            let foff = sl.field_offsets.get(fidx).copied().unwrap_or((fidx * 8) as i32);
                            if foff != 0 {
                                cur_ptr = builder.ins().iadd_imm(cur_ptr, foff as i64);
                            }
                            cur_ty = sl.field_types.get(fidx).cloned().unwrap_or_else(|| "i64".to_string());
                        }
                    } else if cur_ty.starts_with('[') && cur_ty.ends_with(']') {
                        let inner = &cur_ty[1..cur_ty.len() - 1];
                        let elem_ty = inner
                            .split_once('x')
                            .map(|(_, et)| et.trim().to_string())
                            .unwrap_or_else(|| "i8".to_string());
                        if idx_op != "0" {
                            let (elem_sz, _) =
                                llvm_type_size_align(&elem_ty, &self.parsed.structs, self.ptr_bytes);
                            let idx_v = self.eval_operand(builder, idx_op, self.ptr_ty, line_no)?;
                            let off = builder.ins().imul_imm(idx_v, elem_sz as i64);
                            cur_ptr = builder.ins().iadd(cur_ptr, off);
                        }
                        cur_ty = elem_ty;
                    }
                }

                let var = self.get_or_declare_var(builder, dst, self.ptr_ty);
                builder.def_var(var, cur_ptr);
                return Ok(false);
            }

            // `select i1 <cond>, <ty> <a>, <ty> <b>`
            if let Some(rest) = rhs.strip_prefix("select ") {
                let parts = split_top_level_commas(rest);
                if parts.len() != 3 {
                    return Err(syntax_err(line_no, format!("Malformed select: {}", s)));
                }
                let (_, c_op) = split_type_and_operand(parts[0])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed select cond: {}", s)))?;
                let (ty_s, a_op) = split_type_and_operand(parts[1])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed select true val: {}", s)))?;
                let (_, b_op) = split_type_and_operand(parts[2])
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed select false val: {}", s)))?;
                let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
                let cond = self.eval_operand(builder, c_op, types::I8, line_no)?;
                let a = self.eval_operand(builder, a_op, ty, line_no)?;
                let b = self.eval_operand(builder, b_op, ty, line_no)?;
                let res = builder.ins().select(cond, a, b);
                let var = self.get_or_declare_var(builder, dst, ty);
                builder.def_var(var, res);
                return Ok(false);
            }

            // `icmp <pred> <ty> <a>, <b>`
            if let Some(rest) = rhs.strip_prefix("icmp ") {
                let (pred_s, after_pred) = rest
                    .split_once(' ')
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed icmp: {}", s)))?;
                let (lhs_part, rhs_op) = after_pred
                    .split_once(',')
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed icmp args: {}", s)))?;
                let (ty_s, lhs_op) = split_type_and_operand(lhs_part)
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed icmp lhs: {}", s)))?;
                let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
                let cc = match pred_s.trim() {
                    "eq" => IntCC::Equal,
                    "ne" => IntCC::NotEqual,
                    "slt" => IntCC::SignedLessThan,
                    "sle" => IntCC::SignedLessThanOrEqual,
                    "sgt" => IntCC::SignedGreaterThan,
                    "sge" => IntCC::SignedGreaterThanOrEqual,
                    "ult" => IntCC::UnsignedLessThan,
                    "ule" => IntCC::UnsignedLessThanOrEqual,
                    "ugt" => IntCC::UnsignedGreaterThan,
                    "uge" => IntCC::UnsignedGreaterThanOrEqual,
                    other => return Err(syntax_err(line_no, format!("Unknown icmp predicate '{}'", other))),
                };
                let a = self.eval_operand(builder, lhs_op, ty, line_no)?;
                let b = self.eval_operand(builder, rhs_op.trim(), ty, line_no)?;
                let cmp = builder.ins().icmp(cc, a, b);
                let var = self.get_or_declare_var(builder, dst, types::I8);
                builder.def_var(var, cmp);
                return Ok(false);
            }

            // `fcmp <pred> double <a>, <b>`
            if let Some(rest) = rhs.strip_prefix("fcmp ") {
                let (pred_s, after_pred) = rest
                    .split_once(' ')
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed fcmp: {}", s)))?;
                let (lhs_part, rhs_op) = after_pred
                    .split_once(',')
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed fcmp args: {}", s)))?;
                let (ty_s, lhs_op) = split_type_and_operand(lhs_part)
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed fcmp lhs: {}", s)))?;
                let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::F64);
                let cc = match pred_s.trim() {
                    "oeq" => FloatCC::Equal,
                    "one" | "une" => FloatCC::NotEqual,
                    "olt" | "ult" => FloatCC::LessThan,
                    "ole" | "ule" => FloatCC::LessThanOrEqual,
                    "ogt" | "ugt" => FloatCC::GreaterThan,
                    "oge" | "uge" => FloatCC::GreaterThanOrEqual,
                    "ueq" => FloatCC::UnorderedOrEqual,
                    other => return Err(syntax_err(line_no, format!("Unknown fcmp predicate '{}'", other))),
                };
                let a = self.eval_operand(builder, lhs_op, ty, line_no)?;
                let b = self.eval_operand(builder, rhs_op.trim(), ty, line_no)?;
                let cmp = builder.ins().fcmp(cc, a, b);
                let var = self.get_or_declare_var(builder, dst, types::I8);
                builder.def_var(var, cmp);
                return Ok(false);
            }

            // Cast instructions: `<cast> <src_ty> <val> to <dst_ty>`
            for cast_op in [
                "sext ", "zext ", "trunc ", "sitofp ", "uitofp ", "fptosi ", "fptoui ",
                "ptrtoint ", "inttoptr ", "bitcast ",
            ] {
                if let Some(rest) = rhs.strip_prefix(cast_op) {
                    let (src_part, dst_ty_s) = rest
                        .rsplit_once(" to ")
                        .ok_or_else(|| syntax_err(line_no, format!("Malformed cast: {}", s)))?;
                    let (src_ty_s, val_op) = split_type_and_operand(src_part)
                        .ok_or_else(|| syntax_err(line_no, format!("Malformed cast src: {}", s)))?;
                    let src_ty = llvm_type_to_clif(src_ty_s, self.ptr_ty).unwrap_or(types::I64);
                    let dst_ty = llvm_type_to_clif(dst_ty_s, self.ptr_ty).unwrap_or(types::I64);
                    let val = self.eval_operand(builder, val_op, src_ty, line_no)?;
                    let res = match cast_op.trim() {
                        "sext" => {
                            if dst_ty.bits() > src_ty.bits() {
                                builder.ins().sextend(dst_ty, val)
                            } else {
                                val
                            }
                        }
                        "zext" => {
                            if dst_ty.bits() > src_ty.bits() {
                                builder.ins().uextend(dst_ty, val)
                            } else {
                                val
                            }
                        }
                        "trunc" => {
                            if dst_ty.bits() < src_ty.bits() {
                                builder.ins().ireduce(dst_ty, val)
                            } else {
                                val
                            }
                        }
                        "sitofp" => builder.ins().fcvt_from_sint(dst_ty, val),
                        "uitofp" => builder.ins().fcvt_from_uint(dst_ty, val),
                        "fptosi" => builder.ins().fcvt_to_sint_sat(dst_ty, val),
                        "fptoui" => builder.ins().fcvt_to_uint_sat(dst_ty, val),
                        "ptrtoint" | "inttoptr" => self.coerce_val(builder, val, dst_ty),
                        "bitcast" => {
                            if src_ty == dst_ty {
                                val
                            } else if src_ty.bits() == dst_ty.bits() {
                                builder.ins().bitcast(dst_ty, MemFlags::new(), val)
                            } else {
                                self.coerce_val(builder, val, dst_ty)
                            }
                        }
                        _ => val,
                    };
                    let var = self.get_or_declare_var(builder, dst, dst_ty);
                    builder.def_var(var, res);
                    return Ok(false);
                }
            }

            // Unary float negation: `fneg double %x`
            if let Some(rest) = rhs.strip_prefix("fneg ") {
                let (ty_s, val_op) = split_type_and_operand(rest)
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed fneg: {}", s)))?;
                let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::F64);
                let v = self.eval_operand(builder, val_op, ty, line_no)?;
                let res = builder.ins().fneg(v);
                let var = self.get_or_declare_var(builder, dst, ty);
                builder.def_var(var, res);
                return Ok(false);
            }

            // Binary arithmetic / bitwise ops
            if let Some((op_raw, rest)) = rhs.split_once(' ') {
                let op = op_raw.trim();
                if matches!(
                    op,
                    "add" | "sub" | "mul" | "sdiv" | "udiv" | "srem" | "urem"
                        | "and" | "or" | "xor" | "shl" | "ashr" | "lshr"
                        | "fadd" | "fsub" | "fmul" | "fdiv"
                ) {
                    // Strip optional `nsw` / `nuw` / `exact` flags
                    let mut r = rest.trim();
                    loop {
                        if let Some(x) = r.strip_prefix("nsw ").or_else(|| r.strip_prefix("nuw ")).or_else(|| r.strip_prefix("exact ")) {
                            r = x.trim();
                        } else {
                            break;
                        }
                    }
                    let (lhs_part, rhs_op) = r
                        .split_once(',')
                        .ok_or_else(|| syntax_err(line_no, format!("Malformed binop: {}", s)))?;
                    let (ty_s, lhs_op) = split_type_and_operand(lhs_part)
                        .ok_or_else(|| syntax_err(line_no, format!("Malformed binop lhs: {}", s)))?;
                    let ty = llvm_type_to_clif(ty_s, self.ptr_ty).unwrap_or(types::I64);
                    let a = self.eval_operand(builder, lhs_op, ty, line_no)?;
                    let b = self.eval_operand(builder, rhs_op.trim(), ty, line_no)?;
                    let res = match op {
                        "add" => builder.ins().iadd(a, b),
                        "sub" => builder.ins().isub(a, b),
                        "mul" => builder.ins().imul(a, b),
                        "sdiv" => builder.ins().sdiv(a, b),
                        "udiv" => builder.ins().udiv(a, b),
                        "srem" => builder.ins().srem(a, b),
                        "urem" => builder.ins().urem(a, b),
                        "and" => builder.ins().band(a, b),
                        "or" => builder.ins().bor(a, b),
                        "xor" => builder.ins().bxor(a, b),
                        "shl" => builder.ins().ishl(a, b),
                        "ashr" => builder.ins().sshr(a, b),
                        "lshr" => builder.ins().ushr(a, b),
                        "fadd" => builder.ins().fadd(a, b),
                        "fsub" => builder.ins().fsub(a, b),
                        "fmul" => builder.ins().fmul(a, b),
                        "fdiv" => builder.ins().fdiv(a, b),
                        _ => unreachable!(),
                    };
                    let var = self.get_or_declare_var(builder, dst, ty);
                    builder.def_var(var, res);
                    return Ok(false);
                }
            }
        }

        Err(syntax_err(
            line_no,
            format!("Unsupported or malformed SSA instruction: '{}'", s),
        ))
    }

    fn lower_call(
        &mut self,
        builder: &mut FunctionBuilder,
        call_str: &str,
        dst: Option<&str>,
        line_no: usize,
    ) -> Result<(), VerifyError> {
        let rest = call_str.trim().strip_prefix("call ").unwrap_or(call_str).trim();
        let rp = rest
            .rfind(')')
            .ok_or_else(|| syntax_err(line_no, format!("Malformed call: {}", call_str)))?;
        // Find the `(` matching the final `)` (skips variadic prototype `(i8*, ...)` before `@printf`)
        let mut depth = 0i32;
        let mut lp_opt = None;
        for (idx, b) in rest[..=rp].bytes().enumerate().rev() {
            if b == b')' {
                depth += 1;
            } else if b == b'(' {
                depth -= 1;
                if depth == 0 {
                    lp_opt = Some(idx);
                    break;
                }
            }
        }
        let lp = lp_opt
            .ok_or_else(|| syntax_err(line_no, format!("Malformed call args: {}", call_str)))?;

        let head = rest[..lp].trim();
        let args_str = rest[lp + 1..rp].trim();

        // Parse callee (`@name` or `%fn_ptr`) and return type
        let (ret_ty_s, callee_tok) = head
            .rsplit_once(' ')
            .ok_or_else(|| syntax_err(line_no, format!("Malformed call head: {}", call_str)))?;
        // Strip variadic signature `(i8*, ...)` if present before `@printf`,
        // but preserve function-pointer return types like `i32 (i32, i32)*`!
        let ret_ty_clean = if ret_ty_s.trim().ends_with('*') {
            ret_ty_s.trim()
        } else {
            ret_ty_s.split('(').next().unwrap_or(ret_ty_s).trim()
        };
        let ret_clif = llvm_type_to_clif(ret_ty_clean, self.ptr_ty);

        let mut arg_vals = Vec::new();
        let mut arg_types = Vec::new();
        if !args_str.is_empty() {
            for a in split_top_level_commas(args_str) {
                let (aty_s, aop) = split_type_and_operand(a)
                    .ok_or_else(|| syntax_err(line_no, format!("Malformed call arg '{}'", a)))?;
                let aty = llvm_type_to_clif(aty_s, self.ptr_ty).unwrap_or(types::I64);
                let av = self.eval_operand(builder, aop, aty, line_no)?;
                arg_vals.push(av);
                arg_types.push(aty);
            }
        }

        if let Some(sym) = callee_tok.strip_prefix('@') {
            // Handle common LLVM math intrinsics natively in Cranelift
            if sym == "llvm.sqrt.f64" && arg_vals.len() == 1 {
                let res = builder.ins().sqrt(arg_vals[0]);
                if let Some(dst_name) = dst {
                    let var = self.get_or_declare_var(builder, dst_name, types::F64);
                    builder.def_var(var, res);
                }
                return Ok(());
            } else if sym == "llvm.fabs.f64" && arg_vals.len() == 1 {
                let res = builder.ins().fabs(arg_vals[0]);
                if let Some(dst_name) = dst {
                    let var = self.get_or_declare_var(builder, dst_name, types::F64);
                    builder.def_var(var, res);
                }
                return Ok(());
            } else if sym == "llvm.floor.f64" && arg_vals.len() == 1 {
                let res = builder.ins().floor(arg_vals[0]);
                if let Some(dst_name) = dst {
                    let var = self.get_or_declare_var(builder, dst_name, types::F64);
                    builder.def_var(var, res);
                }
                return Ok(());
            } else if sym == "llvm.ceil.f64" && arg_vals.len() == 1 {
                let res = builder.ins().ceil(arg_vals[0]);
                if let Some(dst_name) = dst {
                    let var = self.get_or_declare_var(builder, dst_name, types::F64);
                    builder.def_var(var, res);
                }
                return Ok(());
            }
        }

        let call_inst = if let Some(sym) = callee_tok.strip_prefix('@') {
            let lookup_sym = if sym == "llvm.pow.f64" { "pow" } else { sym };
            if let Some((fid, sig)) = self.func_ids.get(lookup_sym).or_else(|| self.func_ids.get(sym)).cloned() {
                if arg_vals.len() != sig.params.len() {
                    // Variadic call (e.g. `printf(fmt, 42)`): emit indirect call with exact call-site signature
                    let fref = self.module.declare_func_in_func(fid, builder.func);
                    let faddr = builder.ins().func_addr(self.ptr_ty, fref);
                    let mut vsig = Signature::new(self.call_conv);
                    for at in &arg_types {
                        vsig.params.push(AbiParam::new(*at));
                    }
                    if let Some(rt) = ret_clif {
                        vsig.returns.push(AbiParam::new(rt));
                    }
                    let sig_ref = builder.import_signature(vsig);
                    builder.ins().call_indirect(sig_ref, faddr, &arg_vals)
                } else {
                    // Direct call with parameter type coercion
                    let mut coerced = Vec::with_capacity(arg_vals.len());
                    for (idx, av) in arg_vals.into_iter().enumerate() {
                        if let Some(p) = sig.params.get(idx) {
                            coerced.push(self.coerce_val(builder, av, p.value_type));
                        } else {
                            coerced.push(av);
                        }
                    }
                    let fref = self.module.declare_func_in_func(fid, builder.func);
                    builder.ins().call(fref, &coerced)
                }
            } else {
                return Err(syntax_err(
                    line_no,
                    format!("Call to undeclared function '@{}'", sym),
                ));
            }
        } else if callee_tok.starts_with('%') {
            // Indirect call via closure / function pointer
            let fptr = self.eval_operand(builder, callee_tok, self.ptr_ty, line_no)?;
            let mut sig = Signature::new(self.call_conv);
            for at in &arg_types {
                sig.params.push(AbiParam::new(*at));
            }
            if let Some(rt) = ret_clif {
                sig.returns.push(AbiParam::new(rt));
            }
            let sig_ref = builder.import_signature(sig);
            builder.ins().call_indirect(sig_ref, fptr, &arg_vals)
        } else {
            return Err(syntax_err(
                line_no,
                format!("Invalid call target '{}'", callee_tok),
            ));
        };

        if let (Some(dst_name), Some(rt)) = (dst, ret_clif) {
            let results = builder.inst_results(call_inst);
            let res_val = if let Some(&r0) = results.first() {
                self.coerce_val(builder, r0, rt)
            } else {
                builder.ins().iconst(rt, 0)
            };
            let var = self.get_or_declare_var(builder, dst_name, rt);
            builder.def_var(var, res_val);
        }

        Ok(())
    }
}

/// Split `"i64 %x"` or `"[6 x i8]* @.str.0"` or `"i8* getelementptr inbounds (...)"` into `(type_str, operand_str)`.
fn split_type_and_operand(s: &str) -> Option<(&str, &str)> {
    let t = s.trim();
    if let Some(gep_pos) = t.find(" getelementptr ") {
        return Some((t[..gep_pos].trim(), t[gep_pos + 1..].trim()));
    }
    if let Some(bc_pos) = t.find(" bitcast (") {
        return Some((t[..bc_pos].trim(), t[bc_pos + 1..].trim()));
    }
    if let Some(p2i_pos) = t.find(" ptrtoint (") {
        return Some((t[..p2i_pos].trim(), t[p2i_pos + 1..].trim()));
    }
    let (ty, op) = t.rsplit_once(' ')?;
    Some((ty.trim(), op.trim()))
}

fn compile_into_module<M: Module>(
    module: &mut M,
    parsed: &ParsedModule,
) -> Result<HashMap<String, (FuncId, Signature)>, VerifyError> {
    let ptr_ty = module.target_config().pointer_type();
    let ptr_bytes = module.target_config().pointer_bytes() as u32;
    let call_conv = module.target_config().default_call_conv;

    // 1. Pre-declare all global string/byte constants & mutable globals
    let mut data_ids: HashMap<String, DataId> = HashMap::new();
    for g in &parsed.globals {
        let did = module
            .declare_data(&g.name, Linkage::Local, g.writable, false)
            .map_err(|e| VerifyError::LinkError(e.to_string()))?;
        data_ids.insert(g.name.clone(), did);
    }

    // 2. Declare all external functions (and materialize `extern_weak` symbols as safe weak no-ops)
    let mut func_ids: HashMap<String, (FuncId, Signature)> = HashMap::new();
    let mut weak_stub_ids: Vec<(FuncId, Signature)> = Vec::new();
    for d in &parsed.decls {
        let link_name = if d.name == "llvm.pow.f64" {
            "pow"
        } else {
            d.name.as_str()
        };
        if link_name.starts_with("llvm.") {
            continue;
        }
        let mut sig = module.make_signature();
        for pty in &d.param_tys {
            if let Some(ct) = llvm_type_to_clif(pty, ptr_ty) {
                sig.params.push(AbiParam::new(ct));
            }
        }
        if let Some(ref rt) = d.ret_ty {
            if let Some(ct) = llvm_type_to_clif(rt, ptr_ty) {
                sig.returns.push(AbiParam::new(ct));
            }
        }
        let linkage = if d.is_extern_weak {
            Linkage::Preemptible
        } else {
            Linkage::Import
        };
        let fid = module
            .declare_function(link_name, linkage, &sig)
            .map_err(|e| VerifyError::LinkError(e.to_string()))?;
        if d.is_extern_weak {
            weak_stub_ids.push((fid, sig.clone()));
        }
        func_ids.insert(link_name.to_string(), (fid, sig.clone()));
        if link_name != d.name {
            func_ids.insert(d.name.clone(), (fid, sig));
        }
    }
    if !weak_stub_ids.is_empty() {
        let mut wctx = module.make_context();
        let mut wfb_ctx = FunctionBuilderContext::new();
        for (fid, sig) in weak_stub_ids {
            wctx.clear();
            wctx.func.signature = sig.clone();
            let mut b = FunctionBuilder::new(&mut wctx.func, &mut wfb_ctx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            if let Some(ret_p) = sig.returns.first() {
                let z = if ret_p.value_type == types::F64 {
                    b.ins().f64const(0.0)
                } else if ret_p.value_type == types::F32 {
                    b.ins().f32const(0.0)
                } else {
                    b.ins().iconst(ret_p.value_type, 0)
                };
                b.ins().return_(&[z]);
            } else {
                b.ins().return_(&[]);
            }
            b.seal_all_blocks();
            b.finalize();
            module
                .define_function(fid, &mut wctx)
                .map_err(|e| VerifyError::LinkError(e.to_string()))?;
        }
    }

    // 3. Pre-declare all defined functions (enabling mutual recursion)
    for f in &parsed.funcs {
        let mut sig = module.make_signature();
        for (pty, _) in &f.params {
            let ct = llvm_type_to_clif(pty, ptr_ty).unwrap_or(types::I64);
            sig.params.push(AbiParam::new(ct));
        }
        if let Some(ref rt) = f.ret_ty {
            if let Some(ct) = llvm_type_to_clif(rt, ptr_ty) {
                sig.returns.push(AbiParam::new(ct));
            }
        }
        let linkage = if f.is_weak {
            Linkage::Preemptible
        } else {
            Linkage::Export
        };
        let fid = module
            .declare_function(&f.name, linkage, &sig)
            .map_err(|e| VerifyError::LinkError(e.to_string()))?;
        func_ids.insert(f.name.clone(), (fid, sig));
    }

    // 3b. Now define all global data objects (with data & function-pointer relocations resolved!)
    for g in &parsed.globals {
        let did = data_ids[&g.name];
        let mut dd = DataDescription::new();
        dd.define(g.bytes.clone().into_boxed_slice());
        for (off, target_sym, addend) in &g.relocs {
            if let Some(&target_did) = data_ids.get(target_sym) {
                let gv = module.declare_data_in_data(target_did, &mut dd);
                dd.write_data_addr(*off, gv, *addend);
            } else if let Some(&(target_fid, _)) = func_ids.get(target_sym) {
                let fv = module.declare_func_in_data(target_fid, &mut dd);
                dd.write_function_addr(*off, fv);
            }
        }
        module
            .define_data(did, &dd)
            .map_err(|e| VerifyError::LinkError(e.to_string()))?;
    }

    // 4. Lower and define each function body
    let mut ctx = module.make_context();
    let mut fb_ctx = FunctionBuilderContext::new();
    for pfn in &parsed.funcs {
        let (fid, sig) = func_ids[&pfn.name].clone();
        ctx.clear();
        ctx.func.signature = sig;

        let mut lctx = LowerCtx {
            module,
            parsed,
            ptr_ty,
            ptr_bytes,
            call_conv,
            func_ids: &func_ids,
            data_ids: &data_ids,
            vars: HashMap::new(),
            next_var: 0,
            blocks: HashMap::new(),
            phi_incomings: HashMap::new(),
        };
        lctx.lower_function(pfn, &mut ctx.func, &mut fb_ctx)?;

        if let Err(verr) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
            return Err(syntax_err(
                pfn.start_line,
                format!("Cranelift CLIF verification error in '@{}': {}", pfn.name, verr),
            ));
        }

        module
            .define_function(fid, &mut ctx)
            .map_err(|e| VerifyError::LinkError(format!("Cranelift codegen failed in '@{}': {}", pfn.name, e)))?;
    }

    Ok(func_ids)
}

// ---------------------------------------------------------------------------
// 3. Public Entrypoints: Verify, Emit Native Object, and In-Process JIT
// ---------------------------------------------------------------------------

/// Zero-`clang` pure-Rust SSA IR verification via Cranelift CLIF lowering + verifier.
pub fn verify_ir_with_cranelift(ir: &str) -> Result<(), VerifyError> {
    let _obj = compile_ir_to_object(ir, None, 0)?;
    Ok(())
}

/// Compile OmniScript LLVM-syntax SSA IR directly to a native ELF64 / Mach-O / COFF `.o` object file
/// via Cranelift (`cranelift-object`) with zero external `clang` dependency.
pub fn compile_ir_to_object(
    ir: &str,
    target_triple: Option<&str>,
    opt_level: u8,
) -> Result<Vec<u8>, VerifyError> {
    let mut flag_builder = settings::builder();
    let opt_str = match opt_level {
        0 => "none",
        1 => "speed",
        _ => "speed_and_size",
    };
    flag_builder
        .set("opt_level", opt_str)
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;
    flag_builder
        .set("is_pic", "true")
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;

    let triple = match target_triple {
        Some(t) if !t.trim().is_empty() => {
            Triple::from_str(t.trim()).unwrap_or_else(|_| Triple::host())
        }
        _ => Triple::host(),
    };

    let isa_builder = cranelift_codegen::isa::lookup(triple)
        .map_err(|e| VerifyError::LinkError(format!("Unsupported Cranelift target: {}", e)))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;

    let ptr_bytes = isa.pointer_bytes() as u32;
    let parsed = parse_llvm_module(ir, ptr_bytes)?;

    let obj_builder = ObjectBuilder::new(
        isa,
        "omni_module",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| VerifyError::LinkError(e.to_string()))?;
    let mut obj_module = ObjectModule::new(obj_builder);

    compile_into_module(&mut obj_module, &parsed)?;

    let product = obj_module.finish();
    product
        .emit()
        .map_err(|e| VerifyError::LinkError(format!("Object emission failed: {}", e)))
}

/// Compile and execute OmniScript SSA IR directly in-process via `cranelift-jit`!
/// Calls `@main(argc, argv)` and returns its `i32` exit code with zero `clang` and zero temp files.
pub fn jit_execute_ir(
    ir: &str,
    external_syms: &[(&str, *const u8)],
) -> Result<i32, VerifyError> {
    jit_execute_ir_with_args(ir, external_syms, &[])
}

/// Compile and execute OmniScript SSA IR in-process via `cranelift-jit`, forwarding CLI arguments.
pub fn jit_execute_ir_with_args(
    ir: &str,
    external_syms: &[(&str, *const u8)],
    args: &[String],
) -> Result<i32, VerifyError> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;

    let isa_builder = cranelift_native::builder()
        .map_err(|e| VerifyError::LinkError(format!("Host ISA not supported by Cranelift: {}", e)))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| VerifyError::LinkError(e.to_string()))?;

    let ptr_bytes = isa.pointer_bytes() as u32;
    let parsed = parse_llvm_module(ir, ptr_bytes)?;

    let mut jit_builder =
        JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    for (name, ptr) in external_syms {
        jit_builder.symbol(*name, *ptr);
    }
    let mut jit_module = JITModule::new(jit_builder);

    let func_ids = compile_into_module(&mut jit_module, &parsed)?;
    jit_module
        .finalize_definitions()
        .map_err(|e| VerifyError::LinkError(format!("JIT finalize failed: {}", e)))?;

    let (main_fid, _) = func_ids
        .get("main")
        .ok_or_else(|| VerifyError::LinkError("No '@main' entrypoint found in IR".to_string()))?;
    let code_ptr = jit_module.get_finalized_function(*main_fid);
    let main_fn: extern "C" fn(i32, *const *const u8) -> i32 =
        unsafe { std::mem::transmute(code_ptr) };

    let mut c_args: Vec<std::ffi::CString> = Vec::with_capacity(args.len() + 1);
    c_args.push(std::ffi::CString::new("omnic").unwrap());
    for a in args {
        c_args.push(std::ffi::CString::new(a.as_str()).unwrap_or_else(|_| std::ffi::CString::new("").unwrap()));
    }
    let mut ptrs: Vec<*const u8> = c_args.iter().map(|s| s.as_ptr() as *const u8).collect();
    ptrs.push(std::ptr::null());

    Ok(main_fn(c_args.len() as i32, ptrs.as_ptr()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    static LAST_CAPTURED: AtomicI64 = AtomicI64::new(0);

    extern "C" fn test_capture_i64(val: i64) {
        LAST_CAPTURED.store(val, Ordering::SeqCst);
    }

    extern "C" fn test_str_len(ptr: *const u8) -> i64 {
        if ptr.is_null() {
            return 0;
        }
        let mut n = 0usize;
        unsafe {
            while *ptr.add(n) != 0 {
                n += 1;
            }
        }
        n as i64
    }

    #[test]
    fn test_cranelift_verify_valid_and_invalid_ir() {
        let valid_ir = r#"
define i32 @main(i32 %argc, i8** %argv) {
entry:
  %a = add i64 20, 22
  %r = trunc i64 %a to i32
  ret i32 %r
}
"#;
        verify_ir_with_cranelift(valid_ir).expect("valid IR should verify cleanly");

        let bad_undef = r#"
define i32 @main(i32 %argc, i8** %argv) {
entry:
  %a = add i64 %missing_reg, 1
  %r = trunc i64 %a to i32
  ret i32 %r
}
"#;
        let err = verify_ir_with_cranelift(bad_undef).expect_err("undefined reg must fail");
        assert!(err.to_string().contains("%missing_reg"), "err: {}", err);
    }

    #[test]
    fn test_cranelift_jit_loops_phi_and_recursion() {
        let ir = r#"
define i64 @fib(i64 %n) {
entry:
  %cmp = icmp sle i64 %n, 1
  br i1 %cmp, label %base, label %rec
base:
  ret i64 %n
rec:
  %n1 = sub i64 %n, 1
  %f1 = call i64 @fib(i64 %n1)
  %n2 = sub i64 %n, 2
  %f2 = call i64 @fib(i64 %n2)
  %sum = add i64 %f1, %f2
  ret i64 %sum
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %f10 = call i64 @fib(i64 10)
  %res = trunc i64 %f10 to i32
  ret i32 %res
}
"#;
        let rc = jit_execute_ir(ir, &[]).expect("JIT execution should succeed");
        assert_eq!(rc, 55, "fib(10) must equal 55");
    }

    #[test]
    fn test_cranelift_jit_structs_floats_strings_and_host_ffi() {
        let ir = r#"
@.str.0 = private unnamed_addr constant [11 x i8] c"OmniScript\00", align 1
%struct.Point = type { i64, double }

declare void @test_capture_i64(i64)
declare i64 @test_str_len(i8*)

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %p = alloca %struct.Point
  %fx = getelementptr inbounds %struct.Point, %struct.Point* %p, i32 0, i32 0
  store i64 32, i64* %fx
  %fy = getelementptr inbounds %struct.Point, %struct.Point* %p, i32 0, i32 1
  store double 2.5, double* %fy

  %xv = load i64, i64* %fx
  %yv = load double, double* %fy
  %xdf = sitofp i64 %xv to double
  %prod = fmul double %xdf, %yv
  %pi = fptosi double %prod to i64

  %sptr = getelementptr inbounds ([11 x i8], [11 x i8]* @.str.0, i64 0, i64 0)
  %slen = call i64 @test_str_len(i8* %sptr)
  %total = add i64 %pi, %slen
  call void @test_capture_i64(i64 %total)
  %ret32 = trunc i64 %total to i32
  ret i32 %ret32
}
"#;
        LAST_CAPTURED.store(0, Ordering::SeqCst);
        let rc = jit_execute_ir(
            ir,
            &[
                ("test_capture_i64", test_capture_i64 as *const u8),
                ("test_str_len", test_str_len as *const u8),
            ],
        )
        .expect("JIT struct/float/string/FFI execution");
        // 32 * 2.5 = 80, strlen("OmniScript") = 10 -> 90
        assert_eq!(rc, 90);
        assert_eq!(LAST_CAPTURED.load(Ordering::SeqCst), 90);
    }

    #[test]
    fn test_cranelift_emit_native_object_file() {
        let ir = r#"
define i32 @main(i32 %argc, i8** %argv) {
entry:
  ret i32 7
}
"#;
        let obj = compile_ir_to_object(ir, None, 2).expect("native .o emission");
        assert!(obj.len() > 64, "object file should be non-empty");
        #[cfg(target_os = "linux")]
        assert_eq!(&obj[0..4], b"\x7fELF", "Linux object must start with ELF magic");
    }
}
