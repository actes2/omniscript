use omni_engine::{extract_declarations_and_body, InferenceEngine, SsaContextEnvelope, SsaVariable};
use omni_mem::UnifiedMemOp;
use omni_parser::{ExprAst, OmniModule, OmniParser, Statement};
use regex::Regex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Returns the canonical target triple of the current host machine (Linux, macOS, Windows).
pub fn host_target_triple() -> &'static str {
    omni_runtime::host_target_triple()
}

// First-class LLVM value type lattice for lossless 64-bit integer, IEEE-754 `double`, and pointer lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrType {
    I64,
    I32,
    F64,
    I1,
    Ptr,
    StructPtr(String),
}

#[derive(Debug, Clone)]
pub struct TypedValue {
    pub reg: String,
    pub ty: IrType,
}

// Type-hint substrings that require 64-bit / pointer-width ABI slots instead of 32-bit integer truncation.
const ABI_64BIT_SUBSTRINGS: &[&str] = &[
    "string", "char*", "error", "tuple", "list", "slice", "dict", "map",
];

// Exact scalar type names mapped to 64-bit LLVM registers (`i64`).
const ABI_64BIT_EXACT: &[&str] = &[
    "i64", "u64", "long", "usize", "isize", "int64", "uint64", "int",
];

// Exact/substring type annotations mapped to IEEE-754 64-bit `double`.
const FLOAT_TYPE_HINTS: &[&str] = &[
    "double", "float", "f64", "f32", "float64", "float32",
];

// Polyglot float cast prefixes lowered to `sitofp` / `@__omni_c_str_to_f64`.
const FLOAT_CAST_PREFIXES: &[&str] = &[
    "float(", "f64(", "float64(", "double(", "f64::from(", "f32::from(",
];

// Polyglot integer cast prefixes lowered to 64-bit `i64` (`fptosi` / `@__omni_c_str_to_i64` / `sext`).
const INT_CAST_PREFIXES: &[&str] = &[
    "u32::from(", "u64::from(", "i32::from(", "i64::from(", "usize::from(",
    "uint64(", "uint32(", "uint16(", "uint8(", "int64(", "int32(", "int(", "byte(",
];

// Polyglot string cast prefixes lowered to `eval_str_ptr`.
const STRING_CAST_PREFIXES: &[&str] = &[
    "str(", "String(", "tostring(",
];

// Type-hint substrings representing raw C string or Go `error` message pointers (`i8*`).
const STRING_HINT_SUBSTRINGS: &[&str] = &[
    "string", "char*", "error",
];

// Canonical POSIX `<sys/socket.h>` / `<netinet/in.h>` constants lowered without LLM round-trips.
const POSIX_SOCKET_CONSTANTS: &[(&str, &str)] = &[
    ("socket.AF_INET", "2"),
    ("AF_INET", "2"),
    ("socket.AF_INET6", "10"),
    ("AF_INET6", "10"),
    ("socket.SOCK_STREAM", "1"),
    ("SOCK_STREAM", "1"),
    ("socket.SOCK_DGRAM", "2"),
    ("SOCK_DGRAM", "2"),
    ("socket.SOL_SOCKET", "1"),
    ("socket.SOL_SOCKET", "1"),
    ("SO_REUSEADDR", "2"),
    ("socket.SO_REUSEADDR", "2"),
    ("socket.IPPROTO_MPTCP", "262"),
    ("IPPROTO_MPTCP", "262"),
];

// Receiver-qualified constructors lowered directly to OS socket/thread/pipe/error runtime primitives.
const NATIVE_MODULE_CONSTRUCTORS: &[(&str, &str)] = &[
    ("socket", "socket"),
    ("threading", "Thread"),
    ("mp", "Pipe"),
    ("mp", "Process"),
    ("errors", "New"),
    ("fmt", "Errorf"),
];

// Method names intercepted by deterministic C runtime intrinsics when not overridden by a user `%struct.*`.
const NATIVE_RUNTIME_METHODS: &[&str] = &[
    "send", "recv", "bind", "listen", "accept", "connect", "close", "getpeername",
    "start", "terminate", "encode", "decode", "join", "replace", "urandom", "randint",
];

// Call substrings handled natively by deterministic lowering rather than external FFI synthesis.
const NATIVE_CALL_PATTERNS: &[&str] = &[
    "Thread(", "Process(", "errors.New(", "fmt.Errorf(",
];

// Self/class receiver keywords excluded from external foreign-module classification.
const SELF_RECEIVERS: &[&str] = &["self", "this", "cls"];

// Standard ISO C libc functions invoked via direct FFI.
const NATIVE_C_LIB_FNS: &[&str] = &[
    "abs", "labs", "atoi", "atol", "strlen", "strcpy", "strncpy", "strcat",
    "strncmp", "strchr", "strstr", "memcpy", "memmove", "memset", "memcmp",
    "calloc", "realloc", "fprintf", "sprintf", "snprintf", "puts", "putchar",
    "getchar", "fopen", "fclose", "fread", "fwrite", "fseek", "ftell", "fflush",
    "remove", "rename", "getenv", "system", "abort", "time", "clock", "srand", "rand",
];

// Subset of `NATIVE_C_LIB_FNS` whose C ABI return width on LP64 / 64-bit targets is `i64` (`size_t` / `long` / pointer).
const NATIVE_C_LIB_I64_FNS: &[&str] = &[
    "strlen", "atol", "labs", "time", "ftell",
    "calloc", "realloc", "strcpy", "strncpy", "strcat", "strchr", "strstr",
    "memcpy", "memmove", "memset", "fopen", "getenv", "fread", "fwrite",
];

/// Classifies whether a parsed type annotation maps to IEEE-754 `double`.
#[inline]
pub fn is_float_type_hint(type_hint: &str) -> bool {
    let tl = type_hint.trim().to_lowercase();
    FLOAT_TYPE_HINTS.contains(&tl.as_str())
}

/// Classifies whether a parsed type annotation requires a 64-bit / pointer-width LLVM slot.
#[inline]
pub fn is_64bit_type_hint(type_hint: &str) -> bool {
    if type_hint.ends_with('*') {
        return true;
    }
    let tl = type_hint.trim().to_lowercase();
    ABI_64BIT_EXACT.contains(&tl.as_str())
        || ABI_64BIT_SUBSTRINGS.iter().any(|tok| tl.contains(tok))
}

/// Classifies whether a type annotation represents a C string or Go `error` pointer (`i8*`).
#[inline]
pub fn is_string_type_hint(type_hint: &str) -> bool {
    if type_hint == "i8*" {
        return true;
    }
    if type_hint.contains("omni_slice")
        || type_hint.contains("omni_map")
        || type_hint.contains("omni_chan")
        || type_hint.contains("omni_fn")
        || type_hint.contains("omni_unpack")
    {
        return false;
    }
    let tl = type_hint.trim().to_lowercase();
    tl == "str"
        || tl == "&str"
        || tl.contains("(str)")
        || STRING_HINT_SUBSTRINGS.iter().any(|tok| tl.contains(tok))
}

/// Universal bottom sentinels for untyped `i64` slots:
/// - `OMNI_UNSET_SENTINEL`: caller omitted a parameter -> substitute default in callee prologue.
/// - `OMNI_NULL_SENTINEL`: explicit `None` / `null` / `nil` / `undefined` in an `i64` slot,
///   distinct from integer `0` and boolean `false`, yet falsy in `eval_cond`.
pub const OMNI_UNSET_SENTINEL: i64 = -581005069656919554;
pub const OMNI_NULL_SENTINEL: i64 = -581005069656919553;

#[inline]
fn is_null_literal(expr: &str) -> bool {
    matches!(expr.trim(), "null" | "nil" | "None" | "undefined")
}

fn classify_type_token(tok: &str, class_fields: &HashMap<String, HashMap<String, usize>>) -> IrType {
    let clean = tok
        .trim()
        .trim_start_matches('*')
        .trim_start_matches('&')
        .trim_end_matches('*')
        .trim();
    if is_float_type_hint(clean) {
        IrType::F64
    } else if is_string_type_hint(clean) || clean == "str" || clean == "bytes" {
        IrType::Ptr
    } else if class_fields.contains_key(clean) {
        IrType::StructPtr(clean.to_string())
    } else if clean == "bool" || clean == "boolean" || clean == "i1" {
        IrType::I1
    } else {
        IrType::I64
    }
}

fn parse_tuple_return_types(
    ret_hint: &str,
    class_fields: &HashMap<String, HashMap<String, usize>>,
) -> Option<Vec<IrType>> {
    let t = ret_hint.trim();
    let inner = if t.starts_with('(') && t.ends_with(')') {
        &t[1..t.len() - 1]
    } else if let Some(open_b) = t.find('[') {
        let head = t[..open_b].trim().to_lowercase();
        if (head.ends_with("tuple") || head == "pair") && t.ends_with(']') {
            &t[open_b + 1..t.len() - 1]
        } else {
            return None;
        }
    } else {
        return None;
    };
    let parts = omni_parser::OmniParser::split_respecting_quotes(inner, ',');
    if parts.len() < 2 {
        return None;
    }
    Some(
        parts
            .iter()
            .map(|p| {
                let pt = p.trim();
                // Support Go named return slots like `(quot int64, err error)`
                let ty_part = pt.split_whitespace().last().unwrap_or(pt);
                classify_type_token(ty_part, class_fields)
            })
            .collect(),
    )
}

// Parses decimal, hex (`0x`), binary (`0b`), and octal (`0o`) 64-bit integer literals with optional `_` separators.
fn parse_polyglot_i64(raw: &str) -> Option<i64> {
    let s = raw.trim().replace('_', "");
    if s.is_empty() {
        return None;
    }
    let s_clean = s
        .trim_end_matches("LL")
        .trim_end_matches("ll")
        .trim_end_matches("i64")
        .trim_end_matches("u64")
        .trim_end_matches('L');
    if s_clean.is_empty() {
        return None;
    }
    let (neg, rest) = if let Some(r) = s_clean.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = s_clean.strip_prefix('+') {
        (false, r)
    } else {
        (false, s_clean)
    };
    let val_u64 = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()?
    } else if let Some(bin) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B")) {
        u64::from_str_radix(bin, 2).ok()?
    } else if let Some(oct) = rest.strip_prefix("0o").or_else(|| rest.strip_prefix("0O")) {
        u64::from_str_radix(oct, 8).ok()?
    } else {
        if rest.contains('.') || rest.contains('e') || rest.contains('E') {
            return None;
        }
        rest.parse::<u64>().ok()?
    };
    if neg {
        Some((val_u64 as i64).wrapping_neg())
    } else {
        Some(val_u64 as i64)
    }
}

// Parses IEEE-754 floating-point literals (`3.14`, `-0.25`, `1.5e-3`, `2e6`, `4.0f64`).
fn parse_polyglot_f64(raw: &str) -> Option<f64> {
    let s = raw.trim().replace('_', "");
    if s.is_empty() {
        return None;
    }
    let body = s.trim_start_matches(['+', '-']);
    if body.starts_with("0x") || body.starts_with("0X") || body.starts_with("0b") || body.starts_with("0B") || body.starts_with("0o") || body.starts_with("0O") {
        return None;
    }
    let clean = s
        .trim_end_matches("f64")
        .trim_end_matches("f32")
        .trim_end_matches('f');
    if !clean.contains('.') && !clean.contains('e') && !clean.contains('E') {
        return None;
    }
    clean.parse::<f64>().ok()
}

// Formats an `f64` into LLVM IR's exact 64-bit IEEE-754 hexadecimal `double` literal (`0x...`).
#[inline]
fn format_llvm_f64(val: f64) -> String {
    format!("0x{:016X}", val.to_bits())
}

// Splits a top-level ternary expression `cond ? then_expr : else_expr` outside quotes and parens/brackets.
fn split_ternary_expr(expr: &str) -> Option<(&str, &str, &str)> {
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut depth = 0i32;
    let mut in_q: Option<u8> = None;
    let mut q_idx = None;
    let mut colon_idx = None;
    let mut ternary_nest = 0i32;
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if let Some(q) = in_q {
            if b == b'\\' && i + 1 < len {
                i += 2;
                continue;
            }
            if b == q {
                in_q = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => in_q = Some(b),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'?' if depth == 0 => {
                // Ignore Rust `?` postfix or C# `?.` / `??`
                let next = bytes.get(i + 1).copied();
                if next != Some(b'.') && next != Some(b'?') && i + 1 < len {
                    if q_idx.is_none() {
                        q_idx = Some(i);
                    } else {
                        ternary_nest += 1;
                    }
                }
            }
            b':' if depth == 0 && q_idx.is_some() => {
                if bytes.get(i + 1) == Some(&b':') || bytes.get(i.wrapping_sub(1)) == Some(&b':') {
                    i += 1;
                    continue;
                }
                if ternary_nest == 0 {
                    colon_idx = Some(i);
                    break;
                } else {
                    ternary_nest -= 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let qi = q_idx?;
    let ci = colon_idx?;
    let cond = expr[..qi].trim();
    let tr = expr[qi + 1..ci].trim();
    let fa = expr[ci + 1..].trim();
    if cond.is_empty() || tr.is_empty() || fa.is_empty() {
        return None;
    }
    Some((cond, tr, fa))
}

// Advances `temp_counter` past all `%<stem>_<N>` registers emitted in an AI-synthesized IR snippet
// so subsequent deterministic lowering never collides with synthesized local SSA names.
fn advance_ssa_counter_past_body(body: &str, temp_counter: &mut usize) {
    let mut max_seen = *temp_counter + 16;
    for token in body.split(|c: char| !c.is_ascii_alphanumeric() && c != '%' && c != '_') {
        if let Some(reg) = token.strip_prefix('%') {
            if let Some(num_str) = reg.rsplit('_').next() {
                if let Ok(n) = num_str.parse::<usize>() {
                    if n + 1 > max_seen {
                        max_seen = n + 1;
                    }
                }
            }
        }
    }
    *temp_counter = max_seen;
}

#[derive(Debug, Clone)]
pub enum CleanupOp {
    Free(String),
    DestroySlice(String),
    DestroyMap(String),
    DestroyChan(String),
    ReleaseRc(String),
    FreeHeapPtr { slot_ptr: String, elem_ty: String },
    PopTryFrame,
}

#[derive(Debug, Clone)]
pub struct SymbolBinding {
    pub name: String,
    pub llvm_ptr: String,
    pub type_hint: String,
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScopeTree {
    scopes: Vec<HashMap<String, SymbolBinding>>,
    pub class_fields: HashMap<String, HashMap<String, usize>>,
    pub class_field_types: HashMap<String, HashMap<String, String>>,
    pub class_field_order: HashMap<String, Vec<(String, Option<String>, Option<String>)>>,
    pub class_decorators: HashMap<String, Vec<String>>,
    pub class_type_ids: HashMap<String, u64>,
    pub c_module_aliases: HashSet<String>,
    pub valid_extmod_prefixes: HashSet<String>,
    pub has_open_ns_import: bool,
    pub collection_elem_types: HashMap<String, String>,
    /// Per-key static value types for heterogeneous maps: `(map_var, key_str) -> "str" | "f64" | "i64" | "slice" | "map"`.
    pub map_key_types: HashMap<(String, String), String>,
    /// `@property` method-backed attributes per class: `class_name -> { prop_method_name, ... }`.
    pub class_properties: HashMap<String, HashSet<String>>,
    pub enum_variants: HashMap<String, (String, usize)>,
    pub known_functions: HashSet<String>,
    pub extern_fns: HashMap<String, (Vec<IrType>, Option<IrType>)>,
    pub fn_param_defs: HashMap<String, Vec<omni_parser::ParamDef>>,
    pub method_param_defs: HashMap<(String, String), Vec<omni_parser::ParamDef>>,
    pub method_defining_class: HashMap<(String, String), String>,
    pub fn_return_types: HashMap<String, IrType>,
    pub fn_return_ptr_kinds: HashMap<String, String>,
    pub fn_curried_return_types: HashMap<String, IrType>,
    pub fn_tuple_return_types: HashMap<String, Vec<IrType>>,
    pub method_return_types: HashMap<(String, String), IrType>,
    pub loop_exit_labels: Vec<String>,
    pub loop_cond_labels: Vec<String>,
    pub loop_entry_try_depths: Vec<usize>,
    pub loop_cleanup_marks: Vec<usize>,
    pub closure_env_writebacks: Vec<(usize, String, String)>,
    pub active_try_depth: usize,
}

impl ScopeTree {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            class_fields: HashMap::new(),
            class_field_types: HashMap::new(),
            class_field_order: HashMap::new(),
            class_decorators: HashMap::new(),
            class_type_ids: HashMap::new(),
            c_module_aliases: HashSet::new(),
            valid_extmod_prefixes: HashSet::new(),
            has_open_ns_import: false,
            collection_elem_types: HashMap::new(),
            map_key_types: HashMap::new(),
            class_properties: HashMap::new(),
            enum_variants: HashMap::new(),
            known_functions: HashSet::new(),
            extern_fns: HashMap::new(),
            fn_param_defs: HashMap::new(),
            method_param_defs: HashMap::new(),
            method_defining_class: HashMap::new(),
            fn_return_types: HashMap::new(),
            fn_return_ptr_kinds: HashMap::new(),
            fn_curried_return_types: HashMap::new(),
            fn_tuple_return_types: HashMap::new(),
            method_return_types: HashMap::new(),
            loop_exit_labels: Vec::new(),
            loop_cond_labels: Vec::new(),
            loop_entry_try_depths: Vec::new(),
            loop_cleanup_marks: Vec::new(),
            closure_env_writebacks: Vec::new(),
            active_try_depth: 0,
        }
    }

    pub fn fn_abi_arity(&self, fn_name: &str) -> usize {
        self.fn_param_defs
            .get(fn_name)
            .map(|p| p.len().max(6))
            .unwrap_or(6)
    }

    pub fn method_abi_arity(&self, cls: &str, method: &str) -> usize {
        self.method_param_defs
            .get(&(cls.to_string(), method.to_string()))
            .map(|p| p.len().max(5))
            .unwrap_or(5)
    }

    /// Structurally resolves a declared field `(field_name, field_idx, optional_type_hint)` on `cls`
    /// matching an unresolved method name (case/underscore-normalized stem), independent of any framework.
    pub fn match_structural_field(&self, cls: &str, method: &str) -> Option<(String, usize, Option<String>, &'static str)> {
        let f_map = self.class_fields.get(cls)?;
        let f_types = self.class_field_types.get(cls);
        let norm = |s: &str| -> String {
            s.chars()
                .filter(|c| *c != '_')
                .flat_map(|c| c.to_lowercase())
                .collect()
        };
        let m_lower = method.to_lowercase();
        let (kind, stem) = if let Some(rest) = method.strip_prefix("with").or_else(|| method.strip_prefix("With")) {
            if !rest.is_empty() {
                ("fluent_set", rest)
            } else {
                ("exact", method)
            }
        } else if let Some(rest) = method.strip_prefix("set").or_else(|| method.strip_prefix("Set")) {
            if !rest.is_empty() {
                ("set", rest)
            } else {
                ("exact", method)
            }
        } else if let Some(rest) = method.strip_prefix("get").or_else(|| method.strip_prefix("Get")) {
            if !rest.is_empty() {
                ("get", rest)
            } else {
                ("exact", method)
            }
        } else if let Some(rest) = method.strip_prefix("is") {
            if rest.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                ("get", rest)
            } else {
                ("exact", method)
            }
        } else if m_lower.ends_with('_') {
            ("set", method.trim_end_matches('_'))
        } else {
            ("exact", method)
        };

        let target_norm = norm(stem);
        if target_norm.is_empty() {
            return None;
        }
        for (fname, &fidx) in f_map {
            if norm(fname) == target_norm {
                let th = f_types.and_then(|m| m.get(fname)).cloned();
                return Some((fname.clone(), fidx, th, kind));
            }
        }
        None
    }

    /// Infers the LLVM `IrType` returned by an unresolved method on a decorated/annotated `%struct.<cls>`.
    pub fn infer_structural_method_return_type(&self, cls: &str, method: &str, arity: usize) -> Option<IrType> {
        let has_decorators = self
            .class_decorators
            .get(cls)
            .map(|d| !d.is_empty())
            .unwrap_or(false);
        if !has_decorators {
            return None;
        }
        let (_, _, th_opt, kind) = self.match_structural_field(cls, method)?;
        match (kind, arity) {
            ("fluent_set", 1) => Some(IrType::StructPtr(cls.to_string())),
            ("get" | "exact", 0) => {
                if let Some(ref th) = th_opt {
                    Some(classify_type_token(th, &self.class_fields))
                } else {
                    Some(IrType::I64)
                }
            }
            ("set", 1) => Some(IrType::I64),
            _ => None,
        }
    }

    pub fn resolve_defining_class(&self, cls: &str, method: &str) -> String {
        self.method_defining_class
            .get(&(cls.to_string(), method.to_string()))
            .cloned()
            .unwrap_or_else(|| cls.to_string())
    }

    /// Checks if `rec` is a bound `%struct.*` instance or known class that explicitly defines `method`,
    /// preventing OS/runtime duck-typed intrinsics from shadowing user-defined methods.
    pub fn has_user_struct_method(&self, rec: &str, method: &str) -> bool {
        if let Some(b) = self.resolve(rec) {
            if b.type_hint.starts_with("%struct.") {
                let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                return self
                    .method_defining_class
                    .contains_key(&(cls.to_string(), method.to_string()));
            }
        }
        if self.class_fields.contains_key(rec) {
            return self
                .method_defining_class
                .contains_key(&(rec.to_string(), method.to_string()));
        }
        false
    }

    /// Determines whether a dotted call (`rec.method(...)`) is handled natively by local scope or
    /// deterministic runtime intrinsics rather than requiring external foreign-module FFI synthesis.
    pub fn is_natively_routed_call(&self, rec: &str, method: &str, parts: &[&str], full_expr: &str) -> bool {
        NATIVE_MODULE_CONSTRUCTORS.contains(&(rec, method))
            || NATIVE_RUNTIME_METHODS.contains(&method)
            || NATIVE_C_LIB_FNS.contains(&method)
            || NATIVE_CALL_PATTERNS.iter().any(|pat| full_expr.contains(pat))
            || SELF_RECEIVERS.contains(&rec)
            || rec.starts_with('"')
            || rec.starts_with('\'')
            || self.known_functions.contains(method)
            || self.known_functions.contains(&format!("{}_{}", rec, method))
            || parts.iter().any(|p| self.class_fields.contains_key(*p))
            || self.method_defining_class.iter().any(|((_, m), _)| m == method)
    }

    pub fn resolve_enum_variant(&self, expr: &str) -> Option<(String, (usize, Option<String>))> {
        let trimmed = expr.trim();
        if let Some((e, tag)) = self.enum_variants.get(trimmed) {
            return Some((e.clone(), (*tag, None)));
        }
        if let Some(open) = trimmed.find('(') {
            if trimmed.ends_with(')') {
                let var_name = trimmed[..open].trim();
                let payload = trimmed[open + 1..trimmed.len() - 1].trim().to_string();
                if let Some((e, tag)) = self.enum_variants.get(var_name) {
                    return Some((e.clone(), (*tag, Some(payload))));
                }
            }
        }
        None
    }

    pub fn resolve_pattern_tag(&self, enum_name: &str, pattern: &str) -> Option<usize> {
        let pat_clean = pattern.split('(').next().unwrap_or(pattern).trim();
        if let Some((_, tag)) = self.enum_variants.get(pat_clean) {
            return Some(*tag);
        }
        let full_name = format!("{}::{}", enum_name, pat_clean);
        if let Some((_, tag)) = self.enum_variants.get(&full_name) {
            return Some(*tag);
        }
        None
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn insert(&mut self, name: String, binding: SymbolBinding) {
        if !binding.is_scoped {
            if let Some(root) = self.scopes.first_mut() {
                root.insert(name, binding);
            }
        } else if let Some(curr) = self.scopes.last_mut() {
            curr.insert(name, binding);
        }
    }

    pub fn resolve(&self, name: &str) -> Option<&SymbolBinding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return Some(b);
            }
        }
        None
    }

    pub fn active_bindings(&self) -> Vec<SymbolBinding> {
        let mut map = HashMap::new();
        for scope in &self.scopes {
            for (k, v) in scope {
                map.insert(k.clone(), v.clone());
            }
        }
        map.into_values().collect()
    }

    fn edit_distance(a: &str, b: &str) -> usize {
        let ac: Vec<char> = a.to_ascii_lowercase().chars().collect();
        let bc: Vec<char> = b.to_ascii_lowercase().chars().collect();
        let (m, n) = (ac.len(), bc.len());
        if m == 0 {
            return n;
        }
        if n == 0 {
            return m;
        }
        let mut prev: Vec<usize> = (0..=n).collect();
        let mut curr = vec![0usize; n + 1];
        for i in 1..=m {
            curr[0] = i;
            for j in 1..=n {
                let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
                curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[n]
    }

    /// Harvests static scope, struct-layout, and dependency-graph evidence ("reviewer in the middle")
    /// to guide abductive intent synthesis or produce proof-bearing semantic error diagnostics.
    pub fn gather_abductive_evidence(&self, raw_src: &str, import_chain: &[String]) -> Vec<String> {
        let mut proofs = Vec::new();
        let active = self.active_bindings();

        // Extract identifiers from raw_src outside quotes
        let mut idents: Vec<String> = Vec::new();
        let bytes = raw_src.as_bytes();
        let mut i = 0usize;
        let mut in_quote: Option<u8> = None;
        while i < bytes.len() {
            let b = bytes[i];
            if let Some(q) = in_quote {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_quote = None;
                }
                i += 1;
                continue;
            }
            if b == b'"' || b == b'\'' {
                in_quote = Some(b);
                i += 1;
                continue;
            }
            if b.is_ascii_alphabetic() || b == b'_' {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let tok = &raw_src[start..i];
                if !idents.iter().any(|s| s == tok) {
                    idents.push(tok.to_string());
                }
            } else {
                i += 1;
            }
        }

        for id in &idents {
            if let Some(b) = self.resolve(id) {
                if b.type_hint.starts_with("%struct.") {
                    let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                    let mut flds: Vec<&str> = self
                        .class_fields
                        .get(cls)
                        .map(|m| m.keys().map(|s| s.as_str()).collect())
                        .unwrap_or_default();
                    flds.sort();
                    let mut mths: Vec<&str> = self
                        .method_defining_class
                        .keys()
                        .filter_map(|(c, m)| if c == cls { Some(m.as_str()) } else { None })
                        .collect();
                    mths.sort();
                    proofs.push(format!(
                        "receiver '{}' : %struct.{}* (fields=[{}], methods=[{}])",
                        id,
                        cls,
                        flds.join(", "),
                        mths.join(", ")
                    ));
                } else {
                    proofs.push(format!("live_var '{}' : {} ({})", id, b.type_hint, b.llvm_ptr));
                }
            } else if id.len() >= 3
                && !matches!(
                    id.as_str(),
                    "let" | "mut" | "var" | "val" | "def" | "fn" | "func" | "return"
                        | "true" | "false" | "True" | "False" | "None" | "null" | "nil" | "undefined"
                        | "int" | "float" | "double" | "str" | "string" | "new" | "and" | "or" | "not"
                        | "len" | "ord" | "chr" | "list" | "make" | "range" | "print" | "byte" | "unsafe" | "Pointer"
                )
            {
                // Find closest live binding or known/imported function by edit distance
                let mut candidates: Vec<(usize, String)> = Vec::new();
                for b in &active {
                    let d = Self::edit_distance(id, &b.name);
                    if d > 0 && d <= 3 {
                        candidates.push((d, format!("var '{}' ({})", b.name, b.type_hint)));
                    }
                }
                for kf in &self.known_functions {
                    let d = Self::edit_distance(id, kf);
                    if d <= 3 {
                        candidates.push((d, format!("fn '{}'", kf)));
                    }
                }
                for imp in import_chain {
                    if let Some(cf) = imp.strip_prefix("__OMNI_CARVED_FN__:") {
                        let d = Self::edit_distance(id, cf.trim());
                        if d <= 3 {
                            candidates.push((d, format!("imported_fn '{}'", cf.trim())));
                        }
                    } else if let Some(cs) = imp.strip_prefix("__OMNI_C_SYM__:") {
                        if let Some((sym_n, _)) = cs.split_once(':') {
                            let d = Self::edit_distance(id, sym_n.trim());
                            if d <= 3 {
                                candidates.push((d, format!("c_sym '{}'", sym_n.trim())));
                            }
                        }
                    }
                }
                candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                candidates.dedup_by(|a, b| a.1 == b.1);
                if !candidates.is_empty() {
                    let top: Vec<String> = candidates.into_iter().take(3).map(|(_, s)| s).collect();
                    proofs.push(format!("unresolved_token '{}' -> closest_evidence=[{}]", id, top.join(", ")));
                }
            }
        }
        proofs
    }
}

#[derive(Clone)]
struct ActiveFnAiContext {
    enclosing_fn: String,
    return_type: String,
    struct_defs: Vec<String>,
    temp: f32,
    ecosystem_context: Option<String>,
    import_chain: Vec<String>,
    dynamic_declarations: HashSet<String>,
    escalation_depth: usize,
}

pub struct CodeGenerator<'a> {
    engine: &'a dyn InferenceEngine,
    target_triple: String,
    platform_slug: String,
    cmp_re: Regex,
    idx_re: Regex,
    len_re: Regex,
    pop_re: Regex,
    active_ai_ctx: RefCell<Option<ActiveFnAiContext>>,
    drilled_spines: Vec<String>,
    user_functions: Option<HashSet<String>>,
    foreign_symbols: HashSet<String>,
    verified_annotated_classes: RefCell<HashSet<String>>,
    pending_semantic_errors: RefCell<Vec<String>>,
    lifted_closures_ir: RefCell<String>,
    vdispatch_emitted: RefCell<HashMap<String, String>>,
    closure_counter: std::cell::Cell<usize>,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self {
            engine,
            target_triple: host_target_triple().to_string(),
            platform_slug: String::new(),
            cmp_re: Regex::new(r#"^\s*(.*?)\s*(===|!==|==|!=|~=|<=|>=|<|>)\s*(.*?)\s*$"#).unwrap(),
            idx_re: Regex::new(r#"^([a-zA-Z_0-9]+)\[(.+?)\]$"#).unwrap(),
            len_re: Regex::new(r#"^len\s*\(\s*([a-zA-Z_0-9]+)\s*\)$"#).unwrap(),
            pop_re: Regex::new(r#"^([a-zA-Z_0-9]+)\.pop\s*\(\s*\)$"#).unwrap(),
            active_ai_ctx: RefCell::new(None),
            drilled_spines: Vec::new(),
            user_functions: None,
            foreign_symbols: HashSet::new(),
            verified_annotated_classes: RefCell::new(HashSet::new()),
            pending_semantic_errors: RefCell::new(Vec::new()),
            lifted_closures_ir: RefCell::new(String::new()),
            vdispatch_emitted: RefCell::new(HashMap::new()),
            closure_counter: std::cell::Cell::new(0),
        }
    }

    fn is_user_fn(&self, fn_name: &str) -> bool {
        self.user_functions
            .as_ref()
            .map(|set| set.contains(fn_name))
            .unwrap_or(true)
    }

    pub fn with_user_functions(mut self, user_fns: HashSet<String>) -> Self {
        self.user_functions = Some(user_fns);
        self
    }

    pub fn with_foreign_symbols(mut self, syms: HashSet<String>) -> Self {
        self.foreign_symbols = syms;
        self
    }

    pub fn with_target_triple(mut self, triple: impl Into<String>) -> Self {
        let t = triple.into();
        if !t.trim().is_empty() {
            self.target_triple = t;
        }
        self
    }

    pub fn with_platform_slug(mut self, slug: impl Into<String>) -> Self {
        self.platform_slug = slug.into();
        self
    }

    fn emit_extern_fn_call(
        &self,
        fn_name: &str,
        ret_ty: Option<&IrType>,
        actual_c_args: &str,
        ir: &mut String,
        temp_counter: &mut usize,
    ) -> TypedValue {
        match ret_ty {
            Some(IrType::Ptr) => {
                let r = format!("%ext_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i8* @{}({})\n", r, fn_name, actual_c_args));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                TypedValue { reg: r, ty: IrType::Ptr }
            }
            Some(IrType::F64) => {
                let r = format!("%ext_f64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call double @{}({})\n", r, fn_name, actual_c_args));
                TypedValue { reg: r, ty: IrType::F64 }
            }
            None => {
                ir.push_str(&format!("  call void @{}({})\n", fn_name, actual_c_args));
                TypedValue { reg: "0".to_string(), ty: IrType::I64 }
            }
            Some(IrType::I32) => {
                let r = format!("%ext_i32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i32 @{}({})\n", r, fn_name, actual_c_args));
                TypedValue { reg: r, ty: IrType::I32 }
            }
            Some(IrType::I1) => {
                let r = format!("%ext_i1_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i1 @{}({})\n", r, fn_name, actual_c_args));
                TypedValue { reg: r, ty: IrType::I1 }
            }
            Some(_) => {
                let r = format!("%ext_i64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @{}({})\n", r, fn_name, actual_c_args));
                TypedValue { reg: r, ty: IrType::I64 }
            }
        }
    }

    pub fn with_drilled_spines(mut self, spines: Vec<String>) -> Self {
        self.drilled_spines = spines;
        self
    }

    /// Infers the canonical element-type tag (`"double"`, `"i8* (string)"`, `"omni_slice:<inner>"`,
    /// `"omni_map:<inner>"`, `"%struct.<Cls>*"`, or `"i64"`) for an element stored into a collection.
    fn infer_elem_tag_from_expr(expr: &str, tv_ty: &IrType, scope_tree: &ScopeTree) -> String {
        let t = expr.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if let Ok(ast) = OmniParser::parse_expr(t) {
                if let ExprAst::ListLit(elems) = ast.node {
                    if let Some(first) = elems.first() {
                        let first_s = first.slice(t);
                        let sub = if parse_polyglot_f64(first_s).is_some() {
                            "double".to_string()
                        } else if (first_s.starts_with('"') && first_s.ends_with('"'))
                            || (first_s.starts_with('\'') && first_s.ends_with('\''))
                            || Self::is_string_expr(first_s, scope_tree)
                        {
                            "i8* (string)".to_string()
                        } else {
                            "i64".to_string()
                        };
                        return format!("omni_slice:{}", sub);
                    }
                    return "omni_slice:i64".to_string();
                }
            }
        }
        if let Some(b) = scope_tree.resolve(t) {
            if b.type_hint.contains("omni_slice") {
                let inner = scope_tree
                    .collection_elem_types
                    .get(t)
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());
                return format!("omni_slice:{}", inner);
            }
            if b.type_hint.contains("omni_map") {
                let inner = scope_tree
                    .collection_elem_types
                    .get(t)
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());
                return format!("omni_map:{}", inner);
            }
            if is_string_type_hint(&b.type_hint) {
                return "i8* (string)".to_string();
            }
        }
        match tv_ty {
            IrType::F64 => "double".to_string(),
            IrType::StructPtr(cls) => format!("%struct.{}*", cls),
            IrType::Ptr => {
                if Self::is_string_expr(t, scope_tree)
                    || (t.starts_with('"') && t.ends_with('"'))
                    || (t.starts_with('\'') && t.ends_with('\''))
                {
                    "i8* (string)".to_string()
                } else {
                    "i8*".to_string()
                }
            }
            _ => "i64".to_string(),
        }
    }

    /// Resolves the collection kind (`"omni_slice"` or `"omni_map"`) and preserved element-type tag
    /// for an arbitrary collection target expression (`items`, `matrix[i]`, `self.items`).
    fn resolve_collection_target_info(
        target_expr: &str,
        scope_tree: &ScopeTree,
    ) -> (bool, Option<String>) {
        let t = target_expr.trim();
        if let Some(b) = scope_tree.resolve(t) {
            let is_map = b.type_hint.contains("omni_map");
            let elem = scope_tree.collection_elem_types.get(t).cloned();
            return (is_map, elem);
        }
        if let Ok(ast) = OmniParser::parse_expr(t) {
            match ast.node {
                ExprAst::Index { target, .. } => {
                    let (_, parent_elem) = Self::resolve_collection_target_info(target.slice(t), scope_tree);
                    if let Some(pe) = parent_elem {
                        if let Some(sub) = pe.strip_prefix("omni_slice:") {
                            return (false, Some(sub.to_string()));
                        }
                        if let Some(sub) = pe.strip_prefix("omni_map:") {
                            return (true, Some(sub.to_string()));
                        }
                        if pe == "omni_slice" {
                            return (false, Some("i64".to_string()));
                        }
                        if pe == "omni_map" {
                            return (true, Some("i64".to_string()));
                        }
                    }
                }
                ExprAst::Member { target, field } => {
                    let rec = target.slice(t);
                    if let Some(b) = scope_tree.resolve(rec) {
                        if b.type_hint.starts_with("%struct.") {
                            let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            if let Some(ft) = scope_tree.class_field_types.get(cls).and_then(|m| m.get(&field)) {
                                if let Some(sub) = ft.strip_prefix("omni_slice:") {
                                    return (false, Some(sub.to_string()));
                                }
                                if let Some(sub) = ft.strip_prefix("omni_map:") {
                                    return (true, Some(sub.to_string()));
                                }
                                if ft.contains("omni_map") {
                                    return (true, None);
                                }
                                if ft.contains("omni_slice") {
                                    return (false, None);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        (false, None)
    }

    #[inline]
    fn elem_tag_to_map_kind(tag: &str) -> i64 {
        if tag.contains("omni_map") || tag == "map" {
            4
        } else if tag.contains("omni_slice") || tag == "slice" {
            3
        } else if tag == "str" || tag.contains("string") || tag == "i8*" {
            1
        } else if tag == "f64" || tag == "double" || tag == "float" {
            2
        } else if tag == "bool" || tag == "i1" {
            5
        } else {
            0
        }
    }

    /// Coerces a raw 64-bit slot loaded from `@__omni_slice_get` / `@__omni_map_get` back into
    /// its preserved `TypedValue` (`F64` via `bitcast`, `StructPtr` / `Ptr` via `inttoptr`, or `I64`).
    fn unpack_raw_elem_bits(
        raw_reg: String,
        elem_tag: Option<&str>,
        ir: &mut String,
        temp_counter: &mut usize,
    ) -> TypedValue {
        if let Some(tag) = elem_tag {
            if tag == "double" || is_float_type_hint(tag) {
                let f_reg = format!("%elem_f64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = bitcast i64 {} to double\n", f_reg, raw_reg));
                return TypedValue { reg: f_reg, ty: IrType::F64 };
            }
            if tag.starts_with("%struct.") {
                let cls = tag.trim_start_matches("%struct.").trim_end_matches('*');
                let s_reg = format!("%elem_sptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", s_reg, raw_reg, cls));
                return TypedValue {
                    reg: s_reg,
                    ty: IrType::StructPtr(cls.to_string()),
                };
            }
            if tag.starts_with("i8*")
                || tag.starts_with("omni_slice")
                || tag.starts_with("omni_map")
                || is_string_type_hint(tag)
            {
                let p_reg = format!("%elem_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", p_reg, raw_reg));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p_reg));
                return TypedValue { reg: p_reg, ty: IrType::Ptr };
            }
        }
        TypedValue { reg: raw_reg, ty: IrType::I64 }
    }

    /// Evaluates positional & keyword call-site arguments into raw `i64` ABI registers, expanding
    /// declared parameter default expressions at the call site and supporting arbitrary arities.
    fn eval_call_args_with_defaults(
        &self,
        raw_args: &[String],
        param_defs_opt: Option<&Vec<omni_parser::ParamDef>>,
        min_abi_slots: usize,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> Vec<String> {
        if let Some(pdefs) = param_defs_opt {
            let varargs_idx = pdefs.iter().position(|p| p.type_hint.as_deref() == Some("omni_varargs"));
            let kwargs_idx = pdefs.iter().position(|p| p.type_hint.as_deref() == Some("omni_kwargs"));
            let target_slots = pdefs.len().max(min_abi_slots);
            let alloc_slots = target_slots.max(raw_args.len());
            let mut slot_exprs: Vec<Option<String>> = vec![None; alloc_slots];
            let mut extra_varargs: Vec<String> = Vec::new();
            let mut extra_kwargs: Vec<(String, String)> = Vec::new();
            let mut forwarded_kwargs: Vec<String> = Vec::new();
            let mut next_pos = 0usize;

            for a in raw_args {
                let trimmed_a = a.trim();
                // Forwarded `**map_expr` into `**kwargs`
                if let Some(fw) = trimmed_a.strip_prefix("**") {
                    if kwargs_idx.is_some() {
                        forwarded_kwargs.push(fw.trim().to_string());
                        continue;
                    }
                }
                let mut matched_kw_idx = None;
                let mut unmatched_kw: Option<(String, String)> = None;
                let mut kw_rhs = "";
                if !trimmed_a.starts_with('"') && !trimmed_a.starts_with('\'') {
                    if let Some((lhs, rhs)) = trimmed_a.split_once('=') {
                        let lhs_t = lhs.trim();
                        if !rhs.starts_with('=')
                            && !lhs_t.is_empty()
                            && !lhs_t.ends_with('!')
                            && !lhs_t.ends_with('<')
                            && !lhs_t.ends_with('>')
                            && lhs_t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            if let Some(p_idx) = pdefs.iter().position(|p| p.name == lhs_t) {
                                matched_kw_idx = Some(p_idx);
                                kw_rhs = rhs.trim();
                            } else if kwargs_idx.is_some() {
                                unmatched_kw = Some((lhs_t.to_string(), rhs.trim().to_string()));
                            }
                        }
                    }
                }

                if let Some((k, v)) = unmatched_kw {
                    extra_kwargs.push((k, v));
                } else if let Some(p_idx) = matched_kw_idx {
                    if p_idx < slot_exprs.len() {
                        slot_exprs[p_idx] = Some(kw_rhs.to_string());
                    }
                } else {
                    while next_pos < slot_exprs.len()
                        && (slot_exprs[next_pos].is_some()
                            || Some(next_pos) == varargs_idx
                            || Some(next_pos) == kwargs_idx)
                    {
                        next_pos += 1;
                    }
                    let clean = Self::strip_kwarg_prefix(trimmed_a);
                    let pos_limit = varargs_idx.or(kwargs_idx).unwrap_or(slot_exprs.len());
                    if next_pos < pos_limit {
                        slot_exprs[next_pos] = Some(clean.to_string());
                        next_pos += 1;
                    } else if varargs_idx.is_some() {
                        extra_varargs.push(clean.to_string());
                    } else if next_pos < slot_exprs.len() {
                        slot_exprs[next_pos] = Some(clean.to_string());
                        next_pos += 1;
                    }
                }
            }

            let param_expects_f64 = |idx: usize| -> bool {
                pdefs.get(idx).map(|p| {
                    if let Some(ref th) = p.type_hint {
                        let c = th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim();
                        is_float_type_hint(c)
                    } else if let Some(ref dv) = p.default_val {
                        Self::parse_default_i64(&p.default_val).is_none() && parse_polyglot_f64(dv.trim()).is_some()
                    } else {
                        false
                    }
                }).unwrap_or(false)
            };
            let mut out = Vec::with_capacity(target_slots);
            for idx in 0..target_slots {
                if Some(idx) == varargs_idx {
                    let cap = extra_varargs.len().max(4);
                    let sl = format!("%va_slice_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_slice_create(i64 {})\n", sl, cap));
                    for va_e in &extra_varargs {
                        let tv = self.eval_typed(va_e, ir, scope_tree, string_constants, temp_counter);
                        let vbits = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                        ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", sl, vbits));
                    }
                    let sl_i64 = format!("%va_i64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", sl_i64, sl));
                    out.push(sl_i64);
                } else if Some(idx) == kwargs_idx {
                    let cap = (extra_kwargs.len() + 4).max(8);
                    let mp = format!("%kw_map_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_map_create(i64 {})\n", mp, cap));
                    for fw_e in &forwarded_kwargs {
                        let fw_ptr = if let Some(fb) = scope_tree.resolve(fw_e.trim()).cloned() {
                            if fb.type_hint.contains("omni_map") {
                                let mld = format!("%kw_fw_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", mld, fb.llvm_ptr));
                                mld
                            } else {
                                let fw_tv = self.eval_typed(fw_e, ir, scope_tree, string_constants, temp_counter);
                                self.coerce_to_ptr(fw_tv, ir, temp_counter)
                            }
                        } else {
                            let fw_tv = self.eval_typed(fw_e, ir, scope_tree, string_constants, temp_counter);
                            self.coerce_to_ptr(fw_tv, ir, temp_counter)
                        };
                        ir.push_str(&format!("  call void @__omni_map_update(i8* {}, i8* {})\n", mp, fw_ptr));
                    }
                    for (k, v) in &extra_kwargs {
                        let vtv = self.eval_typed(v, ir, scope_tree, string_constants, temp_counter);
                        let vtag = Self::infer_elem_tag_from_expr(v, &vtv.ty, scope_tree);
                        let vkind = Self::elem_tag_to_map_kind(&vtag);
                        let vbits = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);
                        if let Some((cn, len)) = Self::find_string_constant(string_constants, k) {
                            let kp = format!("%kw_k_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                kp, len, len, cn, mp, kp, vbits, vkind
                            ));
                        }
                    }
                    let mp_i64 = format!("%kw_i64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", mp_i64, mp));
                    out.push(mp_i64);
                } else if let Some(ref expr_s) = slot_exprs[idx] {
                    let p_expects_ptr = pdefs.get(idx).and_then(|p| p.type_hint.as_deref()).map(|th| {
                        let c = th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim();
                        scope_tree.class_fields.contains_key(c)
                            || is_string_type_hint(c)
                            || c == "str"
                            || c.contains("slice")
                            || c.contains("list")
                            || c.contains("map")
                            || c.contains("dict")
                            || c.contains("chan")
                    }).unwrap_or(false);
                    if is_null_literal(expr_s.trim()) && !p_expects_ptr && !param_expects_f64(idx) {
                        out.push(OMNI_NULL_SENTINEL.to_string());
                    } else {
                        let mut tv = self.eval_typed(expr_s, ir, scope_tree, string_constants, temp_counter);
                        if param_expects_f64(idx) && tv.ty != IrType::F64 {
                            let fv = self.coerce_to_f64(tv, ir, temp_counter);
                            tv = TypedValue { reg: fv, ty: IrType::F64 };
                        }
                        out.push(self.coerce_to_raw_i64_bits(tv, ir, temp_counter));
                    }
                } else if let Some(def_s) = pdefs.get(idx).and_then(|p| p.default_val.as_ref()) {
                    let p_expects_ptr = pdefs.get(idx).and_then(|p| p.type_hint.as_deref()).map(|th| {
                        let c = th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim();
                        scope_tree.class_fields.contains_key(c)
                            || is_string_type_hint(c)
                            || c == "str"
                            || c.contains("slice")
                            || c.contains("list")
                            || c.contains("map")
                            || c.contains("dict")
                            || c.contains("chan")
                    }).unwrap_or(false);
                    if is_null_literal(def_s.trim()) && !p_expects_ptr && !param_expects_f64(idx) {
                        out.push(OMNI_NULL_SENTINEL.to_string());
                    } else {
                        let mut tv = self.eval_typed(def_s.trim(), ir, scope_tree, string_constants, temp_counter);
                        if param_expects_f64(idx) && tv.ty != IrType::F64 {
                            let fv = self.coerce_to_f64(tv, ir, temp_counter);
                            tv = TypedValue { reg: fv, ty: IrType::F64 };
                        }
                        out.push(self.coerce_to_raw_i64_bits(tv, ir, temp_counter));
                    }
                } else {
                    out.push(OMNI_UNSET_SENTINEL.to_string());
                }
            }
            out
        } else {
            let mut out = Vec::with_capacity(raw_args.len().max(min_abi_slots));
            for a in raw_args {
                let clean = Self::strip_kwarg_prefix(a.trim());
                let tv = self.eval_typed(clean, ir, scope_tree, string_constants, temp_counter);
                out.push(self.coerce_to_raw_i64_bits(tv, ir, temp_counter));
            }
            while out.len() < min_abi_slots {
                out.push("0".to_string());
            }
            out
        }
    }

    fn emit_checked_int_div_rem(
        &self,
        op: &str,
        left_val: &str,
        right_val: &str,
        ir: &mut String,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> TypedValue {
        let op_name = if op == "%" { "srem" } else { "sdiv" };
        if self.target_triple.starts_with("thumb") {
            let is_zero = format!("%div_zero_{}", *temp_counter);
            let safe_div = format!("%safe_div_{}", *temp_counter);
            let res_var = format!("%t_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, right_val));
            ir.push_str(&format!("  {} = select i1 {}, i64 1, i64 {}\n", safe_div, is_zero, right_val));
            ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, op_name, left_val, safe_div));
            return TypedValue { reg: res_var, ty: IrType::I64 };
        }

        let id = *temp_counter;
        *temp_counter += 1;
        let is_zero = format!("%div_zero_{}", id);
        let throw_lbl = format!("div_throw_{}", id);
        let ok_lbl = format!("div_ok_{}", id);
        let res_var = format!("%div_res_{}", id);

        ir.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, right_val));
        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", is_zero, throw_lbl, ok_lbl));

        ir.push_str(&format!("{}:\n", throw_lbl));
        let ty_ptr = if let Some((cn, clen)) = Self::find_string_constant(string_constants, "ZeroDivisionError") {
            let p = format!("%zde_ty_{}", id);
            ir.push_str(&format!(
                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                p, clen, clen, cn
            ));
            p
        } else {
            "null".to_string()
        };
        let msg_ptr = if let Some((cn, clen)) = Self::find_string_constant(string_constants, "division by zero") {
            let p = format!("%zde_msg_{}", id);
            ir.push_str(&format!(
                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                p, clen, clen, cn
            ));
            p
        } else {
            "null".to_string()
        };
        ir.push_str(&format!("  call void @__omni_throw(i64 1, i8* {}, i8* {})\n", ty_ptr, msg_ptr));
        ir.push_str("  unreachable\n\n");

        ir.push_str(&format!("{}:\n", ok_lbl));
        ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, op_name, left_val, right_val));
        TypedValue { reg: res_var, ty: IrType::I64 }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_method_dispatch(
        &self,
        static_receiver_cls: Option<&str>,
        def_cls: &str,
        method: &str,
        raw_inst: &str,
        raw_args: &[String],
        allow_virtual: bool,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> TypedValue {
        let m_arity = scope_tree.method_abi_arity(def_cls, method);
        let pdefs_opt = scope_tree
            .method_param_defs
            .get(&(def_cls.to_string(), method.to_string()));
        let mut arg_regs = self.eval_call_args_with_defaults(
            raw_args,
            pdefs_opt,
            m_arity,
            ir,
            scope_tree,
            string_constants,
            temp_counter,
        );
        arg_regs.truncate(m_arity);
        let fmt_args = arg_regs
            .iter()
            .map(|r| format!("i64 {}", r))
            .collect::<Vec<_>>()
            .join(", ");

        let is_ctor_or_ctx = matches!(method, "__init__" | "new" | "default" | "__enter__" | "__exit__");
        let base_ret = scope_tree
            .method_return_types
            .get(&(def_cls.to_string(), method.to_string()))
            .cloned();

        let mut poly_cases: Vec<(u64, String, Option<IrType>)> = Vec::new();
        let mut distinct_defs: HashSet<String> = HashSet::new();
        let is_numeric_ir = |r: &Option<IrType>| matches!(r, None | Some(IrType::I64 | IrType::I32 | IrType::F64));
        let mut any_f64_arm = matches!(base_ret, Some(IrType::F64));
        if allow_virtual && !is_ctor_or_ctx && scope_tree.class_type_ids.len() >= 2 {
            let mut sorted_cls: Vec<(&String, &u64)> = scope_tree.class_type_ids.iter().collect();
            sorted_cls.sort_by_key(|(_, tid)| **tid);
            for (cname, &tid) in sorted_cls {
                if let Some(impl_def) = scope_tree
                    .method_defining_class
                    .get(&(cname.clone(), method.to_string()))
                {
                    if scope_tree.method_abi_arity(impl_def, method) == m_arity {
                        let impl_ret = scope_tree
                            .method_return_types
                            .get(&(impl_def.clone(), method.to_string()))
                            .cloned();
                        if static_receiver_cls.is_none()
                            || impl_ret == base_ret
                            || (is_numeric_ir(&impl_ret) && is_numeric_ir(&base_ret))
                        {
                            if matches!(impl_ret, Some(IrType::F64)) {
                                any_f64_arm = true;
                            }
                            distinct_defs.insert(impl_def.clone());
                            poly_cases.push((tid, impl_def.clone(), impl_ret));
                        }
                    }
                }
            }
        }

        let eff_ret = if distinct_defs.len() >= 2 && any_f64_arm {
            Some(IrType::F64)
        } else {
            base_ret.clone()
        };

        let target_sym = if distinct_defs.len() >= 2 {
            let f64_tag = if matches!(eff_ret, Some(IrType::F64)) { "_f64" } else { "" };
            let vd_sym = format!("__omni_vdispatch_{}_{}_{}{}", def_cls, method, m_arity, f64_tag);
            if !self.vdispatch_emitted.borrow().contains_key(&vd_sym) {
                let param_sig = (0..m_arity)
                    .map(|i| format!("i64 %a{}", i))
                    .collect::<Vec<_>>()
                    .join(", ");
                let fwd_args = (0..m_arity)
                    .map(|i| format!("i64 %a{}", i))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut vd_ir = format!(
                    "define i64 @{}(i8* %self_raw, {}) {{\nentry:\n  %tid = call i64 @__omni_obj_get_type(i8* %self_raw)\n  switch i64 %tid, label %vd_default [\n",
                    vd_sym, param_sig
                );
                for (tid, _, _) in &poly_cases {
                    vd_ir.push_str(&format!("    i64 {}, label %vd_case_{}\n", tid, tid));
                }
                vd_ir.push_str("  ]\n");
                for (tid, impl_def, impl_ret) in &poly_cases {
                    if matches!(eff_ret, Some(IrType::F64)) && !matches!(impl_ret, Some(IrType::F64)) {
                        vd_ir.push_str(&format!(
                            "vd_case_{0}:\n  %r_{0} = call i64 @__omni_method_{1}_{2}(i8* %self_raw, {3})\n  %rf_{0} = sitofp i64 %r_{0} to double\n  %rb_{0} = bitcast double %rf_{0} to i64\n  ret i64 %rb_{0}\n",
                            tid, impl_def, method, fwd_args
                        ));
                    } else {
                        vd_ir.push_str(&format!(
                            "vd_case_{0}:\n  %r_{0} = call i64 @__omni_method_{1}_{2}(i8* %self_raw, {3})\n  ret i64 %r_{0}\n",
                            tid, impl_def, method, fwd_args
                        ));
                    }
                }
                if matches!(eff_ret, Some(IrType::F64)) && !matches!(base_ret, Some(IrType::F64)) {
                    vd_ir.push_str(&format!(
                        "vd_default:\n  %r_def = call i64 @__omni_method_{0}_{1}(i8* %self_raw, {2})\n  %rf_def = sitofp i64 %r_def to double\n  %rb_def = bitcast double %rf_def to i64\n  ret i64 %rb_def\n}}\n\n",
                        def_cls, method, fwd_args
                    ));
                } else {
                    vd_ir.push_str(&format!(
                        "vd_default:\n  %r_def = call i64 @__omni_method_{}_{}(i8* %self_raw, {})\n  ret i64 %r_def\n}}\n\n",
                        def_cls, method, fwd_args
                    ));
                }
                self.vdispatch_emitted.borrow_mut().insert(vd_sym.clone(), vd_ir);
            }
            vd_sym
        } else {
            format!("__omni_method_{}_{}", def_cls, method)
        };

        let ret_reg = format!("%method_ret_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!(
            "  {} = call i64 @{}(i8* {}, {})\n",
            ret_reg, target_sym, raw_inst, fmt_args
        ));
        let mret = eff_ret
            .or_else(|| scope_tree.infer_structural_method_return_type(def_cls, method, raw_args.len()));
        Self::apply_return_ir_type(ret_reg, mret.as_ref(), ir, temp_counter)
    }

    fn extract_ident_tokens(text: &str, out: &mut HashSet<String>) {
        let mut cur = String::new();
        let mut in_quote: Option<char> = None;
        let mut escaped = false;
        for ch in text.chars() {
            if let Some(q) = in_quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == q {
                    in_quote = None;
                }
                continue;
            }
            if ch == '"' || ch == '\'' {
                if !cur.is_empty() {
                    if !cur.chars().next().unwrap().is_ascii_digit() {
                        out.insert(cur.clone());
                    }
                    cur.clear();
                }
                in_quote = Some(ch);
                continue;
            }
            if ch.is_ascii_alphanumeric() || ch == '_' {
                cur.push(ch);
            } else if !cur.is_empty() {
                if !cur.chars().next().unwrap().is_ascii_digit() {
                    out.insert(cur.clone());
                }
                cur.clear();
            }
        }
        if !cur.is_empty() && !cur.chars().next().unwrap().is_ascii_digit() {
            out.insert(cur);
        }
    }

    fn collect_referenced_idents(stmts: &[Statement], out: &mut HashSet<String>) {
        for s in stmts {
            match s {
                Statement::Assign { target, expr, .. } => {
                    Self::extract_ident_tokens(target, out);
                    Self::extract_ident_tokens(expr, out);
                }
                Statement::TupleUnpack { expr, .. }
                | Statement::Print { expr }
                | Statement::ListAppend { expr, .. }
                | Statement::ChanSend { expr, .. } => {
                    Self::extract_ident_tokens(expr, out);
                }
                Statement::Return { expr: Some(e) } => {
                    Self::extract_ident_tokens(e, out);
                }
                Statement::ListInit { elements, .. } => {
                    for el in elements {
                        Self::extract_ident_tokens(el, out);
                    }
                }
                Statement::MapInit { entries, .. } => {
                    for (k, v) in entries {
                        Self::extract_ident_tokens(k, out);
                        Self::extract_ident_tokens(v, out);
                    }
                }
                Statement::IndexSet { target, index_expr, val_expr } => {
                    Self::extract_ident_tokens(target, out);
                    Self::extract_ident_tokens(index_expr, out);
                    Self::extract_ident_tokens(val_expr, out);
                }
                Statement::If { condition, then_branch, else_branch } => {
                    Self::extract_ident_tokens(condition, out);
                    Self::collect_referenced_idents(then_branch, out);
                    if let Some(eb) = else_branch {
                        Self::collect_referenced_idents(eb, out);
                    }
                }
                Statement::WhileLoop { condition, body } => {
                    Self::extract_ident_tokens(condition, out);
                    Self::collect_referenced_idents(body, out);
                }
                Statement::ForLoop { count, body, .. } => {
                    Self::extract_ident_tokens(count, out);
                    Self::collect_referenced_idents(body, out);
                }
                Statement::WithContext { context_expr, body, .. } => {
                    Self::extract_ident_tokens(context_expr, out);
                    Self::collect_referenced_idents(body, out);
                }
                Statement::Match { target, arms } => {
                    Self::extract_ident_tokens(target, out);
                    for arm in arms {
                        Self::collect_referenced_idents(&arm.body, out);
                    }
                }
                Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                    Self::collect_referenced_idents(try_block, out);
                    for c in catch_clauses {
                        Self::collect_referenced_idents(&c.body, out);
                    }
                    if let Some(fb) = finally_block {
                        Self::collect_referenced_idents(fb, out);
                    }
                }
                Statement::LocalFnDef { func } => {
                    Self::collect_referenced_idents(&func.body, out);
                }
                Statement::Concrete { code, .. } => {
                    Self::extract_ident_tokens(code, out);
                }
                _ => {}
            }
        }
    }

    /// Lambda-lifts a local function or inline closure expression into `@__omni_closure_body_<id>`
    /// and emits a heap-allocated `OmniClosure` (`{ magic, rc, flags, fn_ptr, env_ptr }`) at the
    /// current program point so lexical captures survive outer-frame return without a tracing GC.
    fn lower_closure_def(
        &self,
        func: &omni_parser::FunctionDef,
        ir: &mut String,
        outer_scope: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let cid = self.closure_counter.get();
        self.closure_counter.set(cid + 1);
        let clos_sym = format!("__omni_closure_body_{}", cid);

        let mut ref_idents = HashSet::new();
        Self::collect_referenced_idents(&func.body, &mut ref_idents);
        for p in &func.params {
            if let Some(ref dv) = p.default_val {
                Self::extract_ident_tokens(dv, &mut ref_idents);
            }
        }
        let param_names: HashSet<&str> = func.params.iter().map(|p| p.name.as_str()).collect();
        let mut captures: Vec<SymbolBinding> = outer_scope
            .active_bindings()
            .into_iter()
            .filter(|b| {
                b.name != func.name
                    && !param_names.contains(b.name.as_str())
                    && ref_idents.contains(&b.name)
            })
            .collect();
        captures.sort_by(|a, b| a.name.cmp(&b.name));

        let u_arity = func.params.len().max(6);
        let param_sig = (0..u_arity)
            .map(|i| format!("i64 %arg{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        let param_types_sig = vec!["i64"; u_arity].join(", ");

        let mut clos_ir = if captures.is_empty() {
            format!("define i64 @{}({}) {{\nentry:\n", clos_sym, param_sig)
        } else {
            format!("define i64 @{}(i64 %env_raw, {}) {{\nentry:\n", clos_sym, param_sig)
        };

        let mut inner_scope = ScopeTree::new();
        inner_scope.class_fields = outer_scope.class_fields.clone();
        inner_scope.class_field_types = outer_scope.class_field_types.clone();
        inner_scope.class_field_order = outer_scope.class_field_order.clone();
        inner_scope.class_decorators = outer_scope.class_decorators.clone();
        inner_scope.class_properties = outer_scope.class_properties.clone();
        inner_scope.class_type_ids = outer_scope.class_type_ids.clone();
        inner_scope.c_module_aliases = outer_scope.c_module_aliases.clone();
        inner_scope.valid_extmod_prefixes = outer_scope.valid_extmod_prefixes.clone();
        inner_scope.has_open_ns_import = outer_scope.has_open_ns_import;
        inner_scope.enum_variants = outer_scope.enum_variants.clone();
        inner_scope.known_functions = outer_scope.known_functions.clone();
        inner_scope.extern_fns = outer_scope.extern_fns.clone();
        inner_scope.fn_param_defs = outer_scope.fn_param_defs.clone();
        inner_scope.method_param_defs = outer_scope.method_param_defs.clone();
        inner_scope.method_defining_class = outer_scope.method_defining_class.clone();
        inner_scope.fn_return_types = outer_scope.fn_return_types.clone();
        inner_scope.fn_return_ptr_kinds = outer_scope.fn_return_ptr_kinds.clone();
        inner_scope.fn_curried_return_types = outer_scope.fn_curried_return_types.clone();
        inner_scope.fn_tuple_return_types = outer_scope.fn_tuple_return_types.clone();
        inner_scope.method_return_types = outer_scope.method_return_types.clone();
        inner_scope
            .fn_param_defs
            .insert(func.name.clone(), func.params.clone());

        let mut inner_temp = 0usize;
        let mut inner_lbl = 0usize;
        let mut inner_str_idx = 0usize;
        let mut inner_frees: Vec<CleanupOp> = Vec::new();
        let mut inner_decls: HashSet<String> = HashSet::new();

        if !captures.is_empty() {
            clos_ir.push_str("  %env_ptr = inttoptr i64 %env_raw to i8*\n");
            for (cap_idx, cap_b) in captures.iter().enumerate() {
                let raw_r = format!("%cap_raw_{}_{}", cap_idx, inner_temp);
                inner_temp += 1;
                clos_ir.push_str(&format!(
                    "  {} = call i64 @__omni_slice_get(i8* %env_ptr, i64 {})\n",
                    raw_r, cap_idx
                ));
                if cap_b.type_hint == "double" {
                    let fv = format!("%cap_f64_{}", inner_temp);
                    let slot = format!("%cap_slot_{}_{}", cap_b.name, inner_temp);
                    inner_temp += 1;
                    clos_ir.push_str(&format!(
                        "  {} = bitcast i64 {} to double\n  {} = alloca double\n  store double {}, double* {}\n",
                        fv, raw_r, slot, fv, slot
                    ));
                    inner_scope
                        .closure_env_writebacks
                        .push((cap_idx, slot.clone(), "double".to_string()));
                    inner_scope.insert(
                        cap_b.name.clone(),
                        SymbolBinding {
                            name: cap_b.name.clone(),
                            llvm_ptr: slot,
                            type_hint: "double".to_string(),
                            is_scoped: true,
                        },
                    );
                } else if cap_b.type_hint.starts_with("%struct.") {
                    let cls = cap_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                    let sp = format!("%cap_sp_{}", inner_temp);
                    let slot = format!("%cap_slot_{}_{}", cap_b.name, inner_temp);
                    inner_temp += 1;
                    clos_ir.push_str(&format!(
                        "  {} = inttoptr i64 {} to %struct.{}*\n  {} = alloca %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                        sp, raw_r, cls, slot, cls, cls, sp, cls, slot
                    ));
                    inner_scope.insert(
                        cap_b.name.clone(),
                        SymbolBinding {
                            name: cap_b.name.clone(),
                            llvm_ptr: slot,
                            type_hint: cap_b.type_hint.clone(),
                            is_scoped: true,
                        },
                    );
                } else if cap_b.type_hint.starts_with("%enum.") {
                    let en = cap_b.type_hint.trim_start_matches("%enum.").trim_end_matches('*');
                    let ep = format!("%cap_ep_{}", inner_temp);
                    let slot = format!("%cap_slot_{}_{}", cap_b.name, inner_temp);
                    inner_temp += 1;
                    clos_ir.push_str(&format!(
                        "  {} = inttoptr i64 {} to %enum.{}*\n  {} = alloca %enum.{}*\n  store %enum.{}* {}, %enum.{}** {}\n",
                        ep, raw_r, en, slot, en, en, ep, en, slot
                    ));
                    inner_scope.insert(
                        cap_b.name.clone(),
                        SymbolBinding {
                            name: cap_b.name.clone(),
                            llvm_ptr: slot,
                            type_hint: cap_b.type_hint.clone(),
                            is_scoped: true,
                        },
                    );
                } else if cap_b.type_hint.starts_with("i8*") {
                    let pp = format!("%cap_ptr_{}", inner_temp);
                    let slot = format!("%cap_slot_{}_{}", cap_b.name, inner_temp);
                    inner_temp += 1;
                    clos_ir.push_str(&format!(
                        "  {} = inttoptr i64 {} to i8*\n  {} = alloca i8*\n  store i8* {}, i8** {}\n",
                        pp, raw_r, slot, pp, slot
                    ));
                    if let Some(et) = outer_scope.collection_elem_types.get(&cap_b.name) {
                        inner_scope.collection_elem_types.insert(cap_b.name.clone(), et.clone());
                    }
                    inner_scope.insert(
                        cap_b.name.clone(),
                        SymbolBinding {
                            name: cap_b.name.clone(),
                            llvm_ptr: slot,
                            type_hint: cap_b.type_hint.clone(),
                            is_scoped: true,
                        },
                    );
                } else {
                    let slot = format!("%cap_slot_{}_{}", cap_b.name, inner_temp);
                    inner_temp += 1;
                    clos_ir.push_str(&format!(
                        "  {} = alloca i64\n  store i64 {}, i64* {}\n",
                        slot, raw_r, slot
                    ));
                    inner_scope
                        .closure_env_writebacks
                        .push((cap_idx, slot.clone(), "i64".to_string()));
                    inner_scope.insert(
                        cap_b.name.clone(),
                        SymbolBinding {
                            name: cap_b.name.clone(),
                            llvm_ptr: slot,
                            type_hint: "i64".to_string(),
                            is_scoped: true,
                        },
                    );
                }
            }
        }

        if ref_idents.contains(&func.name) && !func.name.starts_with("__lambda_") {
            let self_slot = format!("%clos_self_slot_{}", inner_temp);
            let self_fn_i8 = format!("%clos_self_fn_{}", inner_temp);
            inner_temp += 1;
            clos_ir.push_str(&format!("  {} = alloca i8*\n", self_slot));
            if captures.is_empty() {
                clos_ir.push_str(&format!(
                    "  {} = bitcast i64 ({})* @{} to i8*\n  store i8* {}, i8** {}\n",
                    self_fn_i8, param_types_sig, clos_sym, self_fn_i8, self_slot
                ));
            } else {
                let self_cl = format!("%clos_self_obj_{}", inner_temp);
                inner_temp += 1;
                clos_ir.push_str(&format!(
                    "  {} = bitcast i64 (i64, {})* @{} to i8*\n  {} = call i8* @__omni_closure_create(i8* {}, i8* %env_ptr)\n  store i8* {}, i8** {}\n",
                    self_fn_i8, param_types_sig, clos_sym, self_cl, self_fn_i8, self_cl, self_slot
                ));
                inner_frees.push(CleanupOp::ReleaseRc(self_slot.clone()));
            }
            inner_scope.insert(
                func.name.clone(),
                SymbolBinding {
                    name: func.name.clone(),
                    llvm_ptr: self_slot,
                    type_hint: "i8* (omni_fn)".to_string(),
                    is_scoped: true,
                },
            );
        }

        for (i, param) in func.params.iter().enumerate() {
            let arg_src = format!("%arg{}", i);
            Self::bind_abi_param_i64(
                &mut clos_ir,
                &mut inner_scope,
                &mut inner_temp,
                param,
                &arg_src,
                &outer_scope.class_fields,
                string_constants,
            );
        }

        let prev_ctx = self.active_ai_ctx.borrow().clone();
        let (s_defs, ai_temp, eco_opt, imp_vec) = if let Some(ref c) = prev_ctx {
            (c.struct_defs.clone(), c.temp, c.ecosystem_context.clone(), c.import_chain.clone())
        } else {
            (Vec::new(), 0.1f32, None, Vec::new())
        };
        *self.active_ai_ctx.borrow_mut() = Some(ActiveFnAiContext {
            enclosing_fn: clos_sym.clone(),
            return_type: "i64".to_string(),
            struct_defs: s_defs.clone(),
            temp: ai_temp,
            ecosystem_context: eco_opt.clone(),
            import_chain: imp_vec.clone(),
            dynamic_declarations: HashSet::new(),
            escalation_depth: 0,
        });

        let lower_res = self.lower_statements(
            &func.body,
            &mut clos_ir,
            &mut inner_scope,
            &mut inner_frees,
            string_constants,
            &mut inner_str_idx,
            &mut inner_lbl,
            &mut inner_temp,
            &clos_sym,
            &s_defs,
            ai_temp,
            eco_opt.as_deref(),
            &imp_vec,
            &mut inner_decls,
            true,
        );

        let sub_ctx_decls = self
            .active_ai_ctx
            .borrow_mut()
            .take()
            .map(|c| c.dynamic_declarations)
            .unwrap_or_default();
        *self.active_ai_ctx.borrow_mut() = prev_ctx;
        if let Some(parent_ctx) = self.active_ai_ctx.borrow_mut().as_mut() {
            parent_ctx.dynamic_declarations.extend(inner_decls);
            parent_ctx.dynamic_declarations.extend(sub_ctx_decls);
        }

        match lower_res {
            Ok(custom_ret) => {
                if !custom_ret {
                    Self::emit_closure_writebacks(&mut clos_ir, &inner_scope.closure_env_writebacks, &mut inner_temp);
                    Self::emit_cleanups(&mut clos_ir, &inner_frees, &mut inner_temp, "Closure Exit");
                    clos_ir.push_str("  ret i64 0\n");
                }
                clos_ir.push_str("}\n\n");
                let hoisted = Self::hoist_allocas(&clos_ir);
                self.lifted_closures_ir.borrow_mut().push_str(&hoisted);
                self.lifted_closures_ir.borrow_mut().push('\n');
            }
            Err(e) => {
                self.pending_semantic_errors.borrow_mut().push(e.to_string());
                let fallback_hdr = if captures.is_empty() {
                    format!("define i64 @{}({}) {{\nentry:\n  ret i64 0\n}}\n\n", clos_sym, param_sig)
                } else {
                    format!("define i64 @{}(i64 %env_raw, {}) {{\nentry:\n  ret i64 0\n}}\n\n", clos_sym, param_sig)
                };
                self.lifted_closures_ir.borrow_mut().push_str(&fallback_hdr);
            }
        }

        // Emit closure allocation at definition/expression site in `ir`
        let fn_i8 = format!("%clos_fn_i8_{}", *temp_counter);
        let clos_obj = format!("%clos_obj_{}", *temp_counter);
        *temp_counter += 1;

        if captures.is_empty() {
            ir.push_str(&format!(
                "  {} = bitcast i64 ({})* @{} to i8*\n  {} = call i8* @__omni_closure_create(i8* {}, i8* null)\n",
                fn_i8, param_types_sig, clos_sym, clos_obj, fn_i8
            ));
        } else {
            let env_sl = format!("%clos_env_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = call i8* @__omni_slice_create(i64 {})\n",
                env_sl,
                captures.len().max(2)
            ));
            let mut ptr_mask: u64 = 0;
            for (cap_idx, cap_b) in captures.iter().enumerate() {
                let cap_bits = if cap_b.type_hint == "double" {
                    let ld = format!("%cap_pack_f_{}", *temp_counter);
                    let bc = format!("%cap_pack_fb_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = load double, double* {}\n  {} = bitcast double {} to i64\n",
                        ld, cap_b.llvm_ptr, bc, ld
                    ));
                    bc
                } else if cap_b.type_hint.starts_with("%struct.") {
                    let cls = cap_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                    let ld = format!("%cap_pack_s_{}", *temp_counter);
                    let r8 = format!("%cap_pack_s8_{}", *temp_counter);
                    let ret8 = format!("%cap_pack_sr_{}", *temp_counter);
                    let p2i = format!("%cap_pack_si_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = load %struct.{}*, %struct.{}** {}\n  {} = bitcast %struct.{}* {} to i8*\n  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = ptrtoint i8* {} to i64\n",
                        ld, cls, cls, cap_b.llvm_ptr, r8, cls, ld, ret8, r8, p2i, ret8
                    ));
                    if cap_idx < 32 {
                        ptr_mask |= 1u64 << cap_idx;
                    }
                    p2i
                } else if cap_b.type_hint.starts_with("%enum.") {
                    let en = cap_b.type_hint.trim_start_matches("%enum.").trim_end_matches('*');
                    let ld = format!("%cap_pack_e_{}", *temp_counter);
                    let r8 = format!("%cap_pack_e8_{}", *temp_counter);
                    let ret8 = format!("%cap_pack_er_{}", *temp_counter);
                    let p2i = format!("%cap_pack_ei_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = load %enum.{}*, %enum.{}** {}\n  {} = bitcast %enum.{}* {} to i8*\n  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = ptrtoint i8* {} to i64\n",
                        ld, en, en, cap_b.llvm_ptr, r8, en, ld, ret8, r8, p2i, ret8
                    ));
                    if cap_idx < 32 {
                        ptr_mask |= 1u64 << cap_idx;
                    }
                    p2i
                } else if cap_b.type_hint.starts_with("i8*") {
                    let ld = format!("%cap_pack_p_{}", *temp_counter);
                    let ret8 = format!("%cap_pack_pr_{}", *temp_counter);
                    let p2i = format!("%cap_pack_pi_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = load i8*, i8** {}\n  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = ptrtoint i8* {} to i64\n",
                        ld, cap_b.llvm_ptr, ret8, ld, p2i, ret8
                    ));
                    if cap_idx < 32 {
                        ptr_mask |= 1u64 << cap_idx;
                    }
                    p2i
                } else if cap_b.type_hint == "i64" {
                    let ld = format!("%cap_pack_i_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", ld, cap_b.llvm_ptr));
                    ld
                } else {
                    let ld32 = format!("%cap_pack_i32_{}", *temp_counter);
                    let sx = format!("%cap_pack_sx_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = load i32, i32* {}\n  {} = sext i32 {} to i64\n",
                        ld32, cap_b.llvm_ptr, sx, ld32
                    ));
                    sx
                };
                ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", env_sl, cap_bits));
            }
            ir.push_str(&format!(
                "  {} = bitcast i64 (i64, {})* @{} to i8*\n  {} = call i8* @__omni_closure_create(i8* {}, i8* {})\n",
                fn_i8, param_types_sig, clos_sym, clos_obj, fn_i8, env_sl
            ));
            if ptr_mask != 0 {
                ir.push_str(&format!(
                    "  call void @__omni_closure_set_env_rc_mask(i8* {}, i64 {})\n",
                    clos_obj, ptr_mask as i64
                ));
            }
            ir.push_str(&format!("  call void @__omni_slice_destroy(i8* {})\n", env_sl));
        }

        clos_obj
    }

    fn emit_closure_writebacks(
        ir: &mut String,
        writebacks: &[(usize, String, String)],
        temp_counter: &mut usize,
    ) {
        for (idx, slot_ptr, th) in writebacks {
            if th == "double" {
                let lf = format!("%wb_f_{}", *temp_counter);
                let bc = format!("%wb_fb_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = load double, double* {}\n  {} = bitcast double {} to i64\n  call void @__omni_slice_set(i8* %env_ptr, i64 {}, i64 {})\n",
                    lf, slot_ptr, bc, lf, idx, bc
                ));
            } else {
                let li = format!("%wb_i_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = load i64, i64* {}\n  call void @__omni_slice_set(i8* %env_ptr, i64 {}, i64 {})\n",
                    li, slot_ptr, idx, li
                ));
            }
        }
    }

    /// Universal structural aggregate initialization for heap/stack `%struct.<cls_name>*` allocations:
    /// zero-initializes all declared field slots, applies field default expressions, dispatches to an
    /// explicit constructor method if one exists in source, or directly populates declared GEP slots
    /// from positional and `kw=val` arguments (handling >5 fields and keyword init across all languages).
    fn init_struct_instance(
        &self,
        cls_name: &str,
        raw_inst: &str,
        inst_struct_ptr: &str,
        assoc_ctor_method: &str,
        raw_args: &[String],
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) {
        if let Some(&tid) = scope_tree.class_type_ids.get(cls_name) {
            ir.push_str(&format!("  call void @__omni_obj_set_type(i8* {}, i64 {})\n", raw_inst, tid));
        }
        let field_count = scope_tree
            .class_fields
            .get(cls_name)
            .map(|m| m.len())
            .unwrap_or(1);
        for f_idx in 0..field_count {
            let zgep = format!("%zinit_gep_{}_{}", f_idx, *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n",
                zgep, cls_name, cls_name, inst_struct_ptr, f_idx
            ));
            ir.push_str(&format!("  store i64 0, i64* {}\n", zgep));
        }

        let ordered_fields = scope_tree
            .class_field_order
            .get(cls_name)
            .cloned()
            .unwrap_or_default();
        for (fname, _, def_opt) in &ordered_fields {
            if let Some(def_expr) = def_opt {
                if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fname)) {
                    let dtv = self.eval_typed(def_expr, ir, scope_tree, string_constants, temp_counter);
                    let dbits = self.coerce_to_raw_i64_bits(dtv, ir, temp_counter);
                    let dgep = format!("%dinit_gep_{}_{}", f_idx, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n",
                        dgep, cls_name, cls_name, inst_struct_ptr, f_idx
                    ));
                    ir.push_str(&format!("  store i64 {}, i64* {}\n", dbits, dgep));
                }
            }
        }

        let has_explicit_ctor = scope_tree
            .method_defining_class
            .contains_key(&(cls_name.to_string(), assoc_ctor_method.to_string()));

        if has_explicit_ctor {
            let def_cls = scope_tree.resolve_defining_class(cls_name, assoc_ctor_method);
            let m_arity = scope_tree.method_abi_arity(&def_cls, assoc_ctor_method);
            let pdefs_opt = scope_tree
                .method_param_defs
                .get(&(def_cls.clone(), assoc_ctor_method.to_string()));
            let mut arg_regs = self.eval_call_args_with_defaults(
                raw_args,
                pdefs_opt,
                m_arity,
                ir,
                scope_tree,
                string_constants,
                temp_counter,
            );
            arg_regs.truncate(m_arity);
            let fmt_args = arg_regs
                .iter()
                .map(|r| format!("i64 {}", r))
                .collect::<Vec<_>>()
                .join(", ");
            let init_ret = format!("%init_ret_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = call i64 @__omni_method_{}_{}(i8* {}, {})\n",
                init_ret, def_cls, assoc_ctor_method, raw_inst, fmt_args
            ));
            return;
        }

        let mut pos_vals: Vec<String> = Vec::new();
        let mut kw_vals: Vec<(String, String)> = Vec::new();
        for a in raw_args {
            let trimmed_a = a.trim();
            let mut parsed_kw = None;
            if let Some(eq_idx) = trimmed_a.find('=') {
                let before = trimmed_a[..eq_idx].trim();
                let after_eq = &trimmed_a[eq_idx + 1..];
                if !before.is_empty()
                    && !after_eq.starts_with('=')
                    && !before.ends_with('!')
                    && !before.ends_with('<')
                    && !before.ends_with('>')
                    && before.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && scope_tree
                        .class_fields
                        .get(cls_name)
                        .map(|m| m.contains_key(before))
                        .unwrap_or(false)
                {
                    parsed_kw = Some((before.to_string(), after_eq.trim().to_string()));
                }
            }
            if let Some((k, vexpr)) = parsed_kw {
                let tv = self.eval_typed(&vexpr, ir, scope_tree, string_constants, temp_counter);
                let bits = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                kw_vals.push((k, bits));
            } else {
                let clean = Self::strip_kwarg_prefix(trimmed_a);
                let tv = self.eval_typed(clean, ir, scope_tree, string_constants, temp_counter);
                let bits = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                pos_vals.push(bits);
            }
        }

        for (idx, bits) in pos_vals.iter().enumerate() {
            if let Some((fname, _, _)) = ordered_fields.get(idx) {
                if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fname)) {
                    let pgep = format!("%pinit_gep_{}_{}", f_idx, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n",
                        pgep, cls_name, cls_name, inst_struct_ptr, f_idx
                    ));
                    ir.push_str(&format!("  store i64 {}, i64* {}\n", bits, pgep));
                }
            }
        }

        for (k, bits) in kw_vals {
            if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(&k)) {
                let kgep = format!("%kwinit_gep_{}_{}", f_idx, *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n",
                    kgep, cls_name, cls_name, inst_struct_ptr, f_idx
                ));
                ir.push_str(&format!("  store i64 {}, i64* {}\n", bits, kgep));
            }
        }
    }

    /// Synthesizes an unresolved `@__omni_method_<cls_name>_<method_name>` on an annotated/decorated struct
    /// via `self.engine.lower_intent` (`omni-coder.gguf` + `VerifiedSynthesisEngine` Clang verification),
    /// using drilled bytecode/package spines from `DependencyDriller` and `%struct.<cls_name>` GEP metadata.
    fn synthesize_unresolved_struct_method(
        &self,
        sym: &str,
        cls_name: &str,
        method_name: &str,
        scope_tree: &ScopeTree,
        struct_defs: &[String],
        ecosystem_context: Option<&str>,
        import_chain: &[String],
        temp: f32,
        temp_counter: &mut usize,
    ) -> Option<String> {
        let empty_decs = Vec::new();
        let decorators = scope_tree.class_decorators.get(cls_name).unwrap_or(&empty_decs);
        let (fname, f_idx, f_type_opt, kind) = scope_tree.match_structural_field(cls_name, method_name)?;
        if decorators.is_empty() && kind == "fluent_set" {
            return None;
        }

        let slot_id = *temp_counter;
        *temp_counter += 8;
        let this_var = format!("%var_this_{}", slot_id);
        let arg_var = format!("%var_arg_{}", slot_id);
        let ret_var = format!("%var_ret_{}", slot_id);

        let structural_obligation = match kind {
            "get" | "exact" => {
                let extra_ptr_hint = if f_type_opt
                    .as_deref()
                    .map(|t| t.starts_with("i8*") || is_string_type_hint(t))
                    .unwrap_or(false)
                {
                    format!(
                        "  %p8_{0} = inttoptr i64 %fv_{0} to i8*\n  call void @__omni_c_return_ptr(i8* %p8_{0})\n",
                        slot_id
                    )
                } else {
                    String::new()
                };
                format!(
                    "__OMNI_SHORT_CIRCUIT_IR__:  %st_{0} = load %struct.{1}*, %struct.{1}** {2}\n  %fg_{0} = getelementptr inbounds %struct.{1}, %struct.{1}* %st_{0}, i32 0, i32 {3}\n  %fv_{0} = load i64, i64* %fg_{0}\n{4}  store i64 %fv_{0}, i64* {5}",
                    slot_id, cls_name, this_var, f_idx, extra_ptr_hint, ret_var
                )
            }
            "set" => format!(
                "__OMNI_SHORT_CIRCUIT_IR__:  %st_{0} = load %struct.{1}*, %struct.{1}** {2}\n  %av_{0} = load i64, i64* {3}\n  %fg_{0} = getelementptr inbounds %struct.{1}, %struct.{1}* %st_{0}, i32 0, i32 {4}\n  store i64 %av_{0}, i64* %fg_{0}\n  store i64 0, i64* {5}",
                slot_id, cls_name, this_var, arg_var, f_idx, ret_var
            ),
            "fluent_set" => format!(
                "__OMNI_SHORT_CIRCUIT_IR__:  %st_{0} = load %struct.{1}*, %struct.{1}** {2}\n  %av_{0} = load i64, i64* {3}\n  %fg_{0} = getelementptr inbounds %struct.{1}, %struct.{1}* %st_{0}, i32 0, i32 {4}\n  store i64 %av_{0}, i64* %fg_{0}\n  %sp8_{0} = bitcast %struct.{1}* %st_{0} to i8*\n  call void @__omni_c_return_ptr(i8* %sp8_{0})\n  %si_{0} = ptrtoint %struct.{1}* %st_{0} to i64\n  store i64 %si_{0}, i64* {5}",
                slot_id, cls_name, this_var, arg_var, f_idx, ret_var
            ),
            _ => return None,
        };

        let mut env_imports = import_chain.to_vec();
        env_imports.push(structural_obligation.clone());

        let intent_desc = match kind {
            "get" | "exact" => format!(
                "method_ret = this.{} ; annotations: [{}], method: {}.{}()",
                fname,
                decorators.join(", "),
                cls_name,
                method_name
            ),
            "set" => format!(
                "this.{} = val ; annotations: [{}], method: {}.{}(val)",
                fname,
                decorators.join(", "),
                cls_name,
                method_name
            ),
            _ => format!(
                "this.{} = val ; method_ret = this ; annotations: [{}], method: {}.{}(val)",
                fname,
                decorators.join(", "),
                cls_name,
                method_name
            ),
        };

        let envelope = SsaContextEnvelope {
            target_abi: self.target_triple.clone(),
            enclosing_fn: sym.to_string(),
            return_type: "i64".to_string(),
            next_ssa_idx: *temp_counter,
            active_symbols: vec![
                SsaVariable {
                    name: "this".to_string(),
                    llvm_name: this_var.clone(),
                    type_hint: format!("%struct.{}*", cls_name),
                },
                SsaVariable {
                    name: "val".to_string(),
                    llvm_name: arg_var.clone(),
                    type_hint: "i64".to_string(),
                },
                SsaVariable {
                    name: "method_ret".to_string(),
                    llvm_name: ret_var.clone(),
                    type_hint: "i64".to_string(),
                },
            ],
            struct_definitions: struct_defs.to_vec(),
            raw_intent: intent_desc.clone(),
            idiom_hint: Some("AnnotatedStructMethodSynthesis".to_string()),
            anchor_id: Some(slot_id),
            physical_source: Some(intent_desc),
            concurrency_mode: None,
            yield_expectation: Some(format!("store 64-bit result to {}", ret_var)),
            action_directive: Some("SYNTHESIZE_ANNOTATED_METHOD".to_string()),
            ecosystem_context: ecosystem_context.map(|s| s.to_string()),
            lifetime_context: None,
            import_chain: env_imports,
        };

        let class_cache_key = format!("{}::[{}]", cls_name, decorators.join(","));
        let sc_body_only = structural_obligation
            .strip_prefix("__OMNI_SHORT_CIRCUIT_IR__:")
            .unwrap_or(&structural_obligation)
            .to_string();

        let synthesized = if self.verified_annotated_classes.borrow().contains(&class_cache_key) {
            sc_body_only
        } else {
            let syn = self.engine.lower_intent(&envelope, temp).ok()?;
            self.verified_annotated_classes.borrow_mut().insert(class_cache_key);
            syn
        };
        let (_, raw_body) = extract_declarations_and_body(&synthesized);
        let clean_body: String = raw_body
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.starts_with("ret ") && t != "ret"
            })
            .map(|l| format!("  {}\n", l.trim()))
            .collect();
        advance_ssa_counter_past_body(&clean_body, temp_counter);

        let fn_ir = format!(
            "define i64 @{sym}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  %self_cast_{slot} = bitcast i8* %self_raw to %struct.{cls}*\n  {this_var} = alloca %struct.{cls}*\n  store %struct.{cls}* %self_cast_{slot}, %struct.{cls}** {this_var}\n  {arg_var} = alloca i64\n  store i64 %len, i64* {arg_var}\n  {ret_var} = alloca i64\n  store i64 0, i64* {ret_var}\n{body}  %final_ret_{slot} = load i64, i64* {ret_var}\n  ret i64 %final_ret_{slot}\n}}\n\n",
            sym = sym,
            slot = slot_id,
            cls = cls_name,
            this_var = this_var,
            arg_var = arg_var,
            ret_var = ret_var,
            body = clean_body
        );
        Some(fn_ir)
    }

    /// Mid-expression LLM Escalation Gate: lowers an unrecognized or foreign sub-expression
    /// via `self.engine.lower_intent` using live `ScopeTree` SSA bindings and returns its `TypedValue`.
    fn escalate_subexpr_to_llm(
        &self,
        expr: &str,
        idiom_hint: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> Option<TypedValue> {
        let ctx_snapshot = {
            let mut guard = self.active_ai_ctx.borrow_mut();
            let ctx = guard.as_mut()?;
            if ctx.escalation_depth >= 2 {
                return None;
            }
            ctx.escalation_depth += 1;
            ctx.clone()
        };

        let reset_depth = || {
            if let Some(ctx) = self.active_ai_ctx.borrow_mut().as_mut() {
                ctx.escalation_depth = ctx.escalation_depth.saturating_sub(1);
            }
        };

        // Deterministically evaluate any non-trivial call arguments into temporary SSA bindings first
        // so the 1.5B model receives clean scalar symbols rather than nested arithmetic inside foreign calls.
        let mut extra_vars: Vec<SsaVariable> = Vec::new();
        let mut rewritten_expr = expr.trim().to_string();
        if let Ok(ast) = OmniParser::parse_expr(expr.trim()) {
            if let ExprAst::Call { callee, args } = ast.node {
                let mut new_args = Vec::new();
                let mut any_rewritten = false;
                for (ai, arg) in args.iter().enumerate() {
                    let is_atomic = matches!(
                        arg.value.node,
                        ExprAst::IntLit(_) | ExprAst::FloatLit(_) | ExprAst::BoolLit(_) | ExprAst::StrLit(_)
                    ) || (matches!(arg.value.node, ExprAst::Ident(ref id) if scope_tree.resolve(id).is_some()));
                    if is_atomic {
                        new_args.push(arg.raw.clone());
                    } else {
                        let arg_slice = arg.value.slice(expr.trim());
                        let atv = self.eval_typed(arg_slice, ir, scope_tree, string_constants, temp_counter);
                        let abits = self.coerce_to_raw_i64_bits(atv, ir, temp_counter);
                        let arg_sym = format!("ai_arg_{}_{}", ai, *temp_counter);
                        let arg_ptr = format!("%var_{}", arg_sym);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca i64\n", arg_ptr));
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", abits, arg_ptr));
                        extra_vars.push(SsaVariable {
                            name: arg_sym.clone(),
                            llvm_name: arg_ptr,
                            type_hint: "i64".to_string(),
                        });
                        if let Some(ref kw) = arg.name {
                            new_args.push(format!("{}={}", kw, arg_sym));
                        } else {
                            new_args.push(arg_sym);
                        }
                        any_rewritten = true;
                    }
                }
                if any_rewritten {
                    rewritten_expr = format!("{}({})", callee.slice(expr.trim()), new_args.join(", "));
                }
            }
        }

        let slot_id = *temp_counter;
        *temp_counter += 1;
        let out_name = ["jitter", "nonce", "ticket", "sample_id"]
            .iter()
            .find(|cand| scope_tree.resolve(cand).is_none())
            .map(|s| (*s).to_string())
            .unwrap_or_else(|| format!("jitter_{}", slot_id));
        let slot_ptr = format!("%var_{}_{}", out_name, slot_id);

        let mut active_symbols: Vec<SsaVariable> = scope_tree
            .active_bindings()
            .into_iter()
            .map(|b| SsaVariable {
                name: b.name,
                llvm_name: b.llvm_ptr,
                type_hint: b.type_hint,
            })
            .collect();
        active_symbols.extend(extra_vars);
        active_symbols.push(SsaVariable {
            name: out_name.clone(),
            llvm_name: slot_ptr.clone(),
            type_hint: "i64".to_string(),
        });

        let intent_stmt = format!("{} = {}", out_name, rewritten_expr);
        let mut enriched_imports = ctx_snapshot.import_chain.clone();
        for ev in scope_tree.gather_abductive_evidence(&rewritten_expr, &ctx_snapshot.import_chain) {
            enriched_imports.push(format!("__OMNI_ABDUCTIVE_EVIDENCE__:{}", ev));
        }
        // Expose in-scope user functions & extern/HAL declarations so the LLM can dynamically
        // target real available symbols in the current module/platform without hardcoded aliases.
        let mut avail_sigs: Vec<String> = Vec::new();
        let mut sorted_fns: Vec<&String> = scope_tree.known_functions.iter().collect();
        sorted_fns.sort();
        for fn_sym in sorted_fns {
            if fn_sym != "main" && fn_sym != &ctx_snapshot.enclosing_fn {
                let ret_str = match scope_tree.fn_return_types.get(fn_sym) {
                    Some(IrType::Ptr) => "i8*",
                    Some(IrType::F64) => "double",
                    Some(IrType::I32) => "i32",
                    _ => "i64",
                };
                avail_sigs.push(format!("{} @__omni_fn_{}(...)", ret_str, fn_sym));
            }
        }
        let mut ext_keys: Vec<&String> = scope_tree.extern_fns.keys().collect();
        ext_keys.sort();
        for ek in ext_keys {
            if let Some((params, ret_opt)) = scope_tree.extern_fns.get(ek) {
                let ret_str = match ret_opt {
                    None => "void",
                    Some(IrType::Ptr) => "i8*",
                    Some(IrType::F64) => "double",
                    Some(IrType::I32) => "i32",
                    _ => "i64",
                };
                let p_str = vec!["i64"; params.len()].join(", ");
                avail_sigs.push(format!("{} @{}({})", ret_str, ek, p_str));
            }
        }
        for sig in avail_sigs.into_iter().take(24) {
            enriched_imports.push(format!("__OMNI_AVAILABLE_SYM__:{}", sig));
        }
        enriched_imports.push(format!(
            "__OMNI_SEMANTIC_OBLIGATION__:compute '{}' using active registers or runtime intrinsics and store result into {}",
            rewritten_expr, slot_ptr
        ));
        let envelope = SsaContextEnvelope {
            target_abi: self.target_triple.clone(),
            enclosing_fn: ctx_snapshot.enclosing_fn.clone(),
            return_type: ctx_snapshot.return_type.clone(),
            next_ssa_idx: *temp_counter,
            active_symbols,
            struct_definitions: ctx_snapshot.struct_defs.clone(),
            raw_intent: intent_stmt.clone(),
            idiom_hint: Some(idiom_hint.to_string()),
            anchor_id: Some(slot_id),
            physical_source: Some(intent_stmt),
            concurrency_mode: None,
            yield_expectation: Some(format!("store 64-bit scalar result to {}", slot_ptr)),
            action_directive: Some("LOWER_ASSIGNMENT".to_string()),
            ecosystem_context: ctx_snapshot.ecosystem_context.clone(),
            lifetime_context: None,
            import_chain: enriched_imports,
        };

        let synth_res = self.engine.lower_intent(&envelope, ctx_snapshot.temp);
        reset_depth();

        let synthesized = match synth_res {
            Ok(s) => s,
            Err(e) => {
                let is_user_fn = self
                    .user_functions
                    .as_ref()
                    .map(|set| set.contains(&ctx_snapshot.enclosing_fn))
                    .unwrap_or(true);
                if is_user_fn {
                    self.pending_semantic_errors.borrow_mut().push(e.to_string());
                } else if std::env::var("OMNI_VERBOSE").is_ok() {
                    eprintln!(
                        "  [Semantic Prune Notice] Unlowered foreign dependency sub-expression '{}' in carved fn '{}'",
                        rewritten_expr, ctx_snapshot.enclosing_fn
                    );
                }
                return None;
            }
        };
        let (dyn_decls, raw_body) = extract_declarations_and_body(&synthesized);
        // Strip any stray `ret` terminators so a mid-expression block never exits the enclosing function early
        let filtered_lines: Vec<&str> = raw_body
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.starts_with("ret ") && t != "ret"
            })
            .collect();
        let body = filtered_lines.join("\n");
        let exec_lines: Vec<&str> = filtered_lines
            .iter()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with(';'))
            .collect();
        if exec_lines.is_empty() {
            return None;
        }

        advance_ssa_counter_past_body(&body, temp_counter);
        if let Some(ctx) = self.active_ai_ctx.borrow_mut().as_mut() {
            for d in dyn_decls {
                ctx.dynamic_declarations.insert(d);
            }
        }

        ir.push_str(&format!("  {} = alloca i64\n", slot_ptr));
        ir.push_str(&format!("  store i64 0, i64* {}\n", slot_ptr));
        ir.push_str(&body);
        if !body.ends_with('\n') {
            ir.push('\n');
        }

        let mut produced_ptr_reg: Option<String> = None;
        let already_stored = exec_lines.iter().any(|l| {
            l.starts_with("store ")
                && (l.contains(&format!("* {}", slot_ptr)) || l.contains(&format!("*{}", slot_ptr)))
        });
        // Detect if the synthesized IR produced a string/pointer (`i8*`) value (e.g. `call i8* @__omni_input(...)`)
        let last_ptr_call = exec_lines.iter().rev().find_map(|l| {
            if l.starts_with('%') && l.contains('=') {
                let eq = l.find('=')?;
                let reg = l[..eq].trim();
                let rhs = l[eq + 1..].trim();
                let head = rhs.split('(').next().unwrap_or(rhs);
                if rhs.starts_with("call i8*")
                    || rhs.starts_with("load i8*")
                    || head.contains("i8*")
                    || rhs.starts_with("inttoptr")
                    || rhs.starts_with("getelementptr")
                {
                    return Some(reg.to_string());
                }
            }
            None
        });
        if !already_stored {
            if let Some(last_assign) = exec_lines.iter().rev().find(|l| l.starts_with('%') && l.contains('=')) {
                if let Some(eq_pos) = last_assign.find('=') {
                    let reg = last_assign[..eq_pos].trim();
                    let rhs = last_assign[eq_pos + 1..].trim();
                    let head = rhs.split('(').next().unwrap_or(rhs);
                    if rhs.starts_with("call i8*")
                        || rhs.starts_with("load i8*")
                        || head.contains("i8*")
                        || rhs.starts_with("inttoptr")
                        || rhs.starts_with("getelementptr")
                    {
                        produced_ptr_reg = Some(reg.to_string());
                        let cast_reg = format!("%ptr_to_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", cast_reg, reg));
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", cast_reg, slot_ptr));
                    } else if (rhs.starts_with("call i64") || rhs.starts_with("load i64") || head.contains("i64"))
                        && !rhs.contains("to i32")
                    {
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", reg, slot_ptr));
                    } else if rhs.starts_with("call double") || rhs.starts_with("load double") || head.contains("double") {
                        let fbits = format!("%f_to_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = fptosi double {} to i64\n", fbits, reg));
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", fbits, slot_ptr));
                    } else {
                        let sext_reg = format!("%sext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", sext_reg, reg));
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", sext_reg, slot_ptr));
                    }
                }
            }
        } else if let Some(ptr_reg) = last_ptr_call {
            // If the LLM emitted `call i8* @__omni_input(...)` and stored `ptrtoint` into `slot_ptr`,
            // preserve its `IrType::Ptr` string type for downstream callers like `uart_puts` / `wifi_connect`!
            if exec_lines.iter().any(|l| l.contains("ptrtoint i8*")) {
                produced_ptr_reg = Some(ptr_reg);
            }
        }

        if let Some(ptr_reg) = produced_ptr_reg {
            return Some(TypedValue {
                reg: ptr_reg,
                ty: IrType::Ptr,
            });
        }

        let loaded = format!("%ai_subexpr_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = load i64, i64* {}\n", loaded, slot_ptr));
        Some(TypedValue {
            reg: loaded,
            ty: IrType::I64,
        })
    }

    fn escape_llvm_string(src: &str) -> (String, usize) {
        let mut bytes = Vec::new();
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    match next {
                        'n' => { chars.next(); bytes.push(b'\n'); }
                        'r' => { chars.next(); bytes.push(b'\r'); }
                        't' => { chars.next(); bytes.push(b'\t'); }
                        '\\' => { chars.next(); bytes.push(b'\\'); }
                        '"' => { chars.next(); bytes.push(b'"'); }
                        '\'' => { chars.next(); bytes.push(b'\''); }
                        'e' => { chars.next(); bytes.push(0x1B); }
                        'x' => {
                            chars.next();
                            let mut hex_str = String::new();
                            for _ in 0..2 {
                                if let Some(&h) = chars.peek() {
                                    if h.is_ascii_hexdigit() {
                                        hex_str.push(h);
                                        chars.next();
                                    }
                                }
                            }
                            if let Ok(b) = u8::from_str_radix(&hex_str, 16) {
                                bytes.push(b);
                            }
                        }
                        '0'..='7' => {
                            let mut oct_str = String::new();
                            for _ in 0..3 {
                                if let Some(&o) = chars.peek() {
                                    if ('0'..='7').contains(&o) {
                                        oct_str.push(o);
                                        chars.next();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            if let Ok(b) = u8::from_str_radix(&oct_str, 8) {
                                bytes.push(b);
                            }
                        }
                        _ => {
                            bytes.push(b'\\');
                        }
                    }
                } else {
                    bytes.push(b'\\');
                }
            } else {
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                bytes.extend_from_slice(encoded.as_bytes());
            }
        }

        let len_with_null = bytes.len() + 1;
        let mut llvm_str = String::new();
        for &b in &bytes {
            if b >= 32 && b <= 126 && b != b'"' && b != b'\\' {
                llvm_str.push(b as char);
            } else {
                llvm_str.push_str(&format!("\\{:02X}", b));
            }
        }
        (llvm_str, len_with_null)
    }

    fn intern_string(
        string_constants: &mut Vec<(String, String, usize)>,
        counter: &mut usize,
        text: &str,
    ) {
        if string_constants.iter().any(|(_, s, _)| s == text) {
            return;
        }
        let const_name = format!("@.str.omni_{}", *counter);
        *counter += 1;
        let (_, len_with_null) = Self::escape_llvm_string(text);
        string_constants.push((const_name, text.to_string(), len_with_null));
    }

    fn find_string_constant<'b>(
        string_constants: &'b [(String, String, usize)],
        text: &str,
    ) -> Option<(&'b str, usize)> {
        string_constants
            .iter()
            .find(|(_, s, _)| s == text)
            .map(|(c, _, len)| (c.as_str(), *len))
    }

    fn extract_pattern_binding(pattern: &str) -> Option<String> {
        if let Some(open) = pattern.find('(') {
            if pattern.ends_with(')') {
                let inner = pattern[open + 1..pattern.len() - 1].trim();
                if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Some(inner.to_string());
                }
            }
        }
        None
    }

    fn emit_cleanups(
        ir: &mut String,
        cleanups: &[CleanupOp],
        temp_counter: &mut usize,
        context: &str,
    ) {
        for op in cleanups.iter().rev() {
            match op {
                CleanupOp::Free(ptr) => {
                    ir.push_str(&format!("  ; [Deferred Free at {}: %{}]\n", context, ptr));
                    ir.push_str(&format!("  call void @free(i8* %{})\n", ptr));
                }
                CleanupOp::DestroySlice(alloca_ptr) => {
                    let loaded = format!("%clean_slice_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Slice Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  store i8* null, i8** {}\n", alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_slice_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyMap(alloca_ptr) => {
                    let loaded = format!("%clean_map_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Map Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  store i8* null, i8** {}\n", alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_map_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyChan(alloca_ptr) => {
                    let loaded = format!("%clean_chan_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Chan Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  store i8* null, i8** {}\n", alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_chan_destroy(i8* {})\n", loaded));
                }
                CleanupOp::ReleaseRc(alloca_ptr) => {
                    let loaded = format!("%clean_rc_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [Perceus RC Release at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  store i8* null, i8** {}\n", alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_rc_release(i8* {})\n", loaded));
                }
                CleanupOp::FreeHeapPtr { slot_ptr, elem_ty } => {
                    let loaded = format!("%clean_hp_{}", *temp_counter);
                    let raw = format!("%clean_hp_i8_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [Perceus Heap Aggregate RC Release at {}]\n", context));
                    ir.push_str(&format!("  {} = load {}, {}* {}\n", loaded, elem_ty, elem_ty, slot_ptr));
                    ir.push_str(&format!("  store {} null, {}* {}\n", elem_ty, elem_ty, slot_ptr));
                    ir.push_str(&format!("  {} = bitcast {} {} to i8*\n", raw, elem_ty, loaded));
                    ir.push_str(&format!("  call void @__omni_rc_release(i8* {})\n", raw));
                }
                CleanupOp::PopTryFrame => {
                    ir.push_str(&format!("  ; [Unwind Try Frame Pop at {}]\n", context));
                    ir.push_str("  call void @__omni_try_pop()\n");
                }
            }
        }
    }

    /// Parses Python `raise [ExcType(msg)] [from cause]` and JS/Java/C++ `throw [new] ExcType(msg)` / `throw expr`.
    /// Returns `Some((None, None))` for bare rethrow (`raise` / `throw`), or `Some((Some(type_name), msg_expr))`.
    fn parse_raise_or_throw(code: &str) -> Option<(Option<String>, Option<String>)> {
        let trimmed = code.trim().trim_end_matches(';').trim();
        if trimmed == "raise" || trimmed == "throw" {
            return Some((None, None));
        }
        let rest = if let Some(r) = trimmed.strip_prefix("raise ") {
            r.trim()
        } else if let Some(r) = trimmed.strip_prefix("throw ") {
            r.trim()
        } else {
            return None;
        };
        if rest.is_empty() {
            return Some((None, None));
        }
        // Strip Python exception chaining suffix: `raise ValueError("x") from err`
        let core = if let Some(idx) = rest.rfind(" from ") {
            let suffix = rest[idx + 6..].trim();
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                rest[..idx].trim()
            } else {
                rest
            }
        } else {
            rest
        };
        let core = core.strip_prefix("new ").map(|s| s.trim()).unwrap_or(core);
        if let Some(lp) = core.find('(') {
            if core.ends_with(')') {
                let ty_cand = core[..lp].trim();
                if !ty_cand.is_empty()
                    && ty_cand.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                {
                    let inside = core[lp + 1..core.len() - 1].trim();
                    let args = omni_parser::OmniParser::split_respecting_quotes(inside, ',');
                    let msg_expr = args
                        .first()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    return Some((Some(ty_cand.to_string()), msg_expr));
                }
            }
        }
        // Bare exception class `raise StopIteration` vs value `throw "bad"`
        if core
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false)
            && core.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return Some((Some(core.to_string()), None));
        }
        Some((Some("Exception".to_string()), Some(core.to_string())))
    }

    fn split_binary_expr(expr: &str) -> Option<(String, String, String)> {
        OmniParser::pratt_split_top_binary(expr)
    }

    fn scan_strings_recursive(
        &self,
        stmts: &[Statement],
        string_constants: &mut Vec<(String, String, usize)>,
        counter: &mut usize,
    ) {
        fn scan_text(text: &str, string_constants: &mut Vec<(String, String, usize)>, counter: &mut usize) {
            let chars: Vec<char> = text.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '"' || chars[i] == '\'' {
                    let q = chars[i];
                    let start = i + 1;
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\\' {
                            i += 2;
                            continue;
                        }
                        if chars[i] == q {
                            let extracted: String = chars[start..i].iter().collect();
                            CodeGenerator::intern_string(string_constants, counter, &extracted);
                            break;
                        }
                        i += 1;
                    }
                } else if chars[i] == '.' && i + 1 < chars.len() && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_') {
                    let start = i + 1;
                    let mut j = start;
                    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j] != '(' || j == chars.len() {
                        let prop: String = chars[start..j].iter().collect();
                        CodeGenerator::intern_string(string_constants, counter, &prop);
                    }
                } else if chars[i] == '='
                    && i > 0
                    && (i + 1 >= chars.len() || chars[i + 1] != '=')
                    && !matches!(chars[i - 1], '=' | '!' | '<' | '>' | '+' | '-' | '*' | '/' | ':' | '&' | '|' | '^')
                {
                    // Intern keyword argument names (`timeout=5`) for `**kwargs` map packing
                    let mut end = i;
                    while end > 0 && chars[end - 1].is_whitespace() {
                        end -= 1;
                    }
                    let mut start = end;
                    while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '_') {
                        start -= 1;
                    }
                    if start < end && (chars[start].is_ascii_alphabetic() || chars[start] == '_') {
                        let kw: String = chars[start..end].iter().collect();
                        CodeGenerator::intern_string(string_constants, counter, &kw);
                    }
                }
                i += 1;
            }
        }

        for stmt in stmts {
            match stmt {
                Statement::Print { expr } => {
                    scan_text(expr, string_constants, counter);
                    if expr.contains("{}") && expr.contains(',') {
                        if let Some(comma_pos) = expr.rfind(',') {
                            let fmt = expr[..comma_pos]
                                .trim()
                                .trim_start_matches('f')
                                .trim_matches('"')
                                .trim_matches('\'');
                            let prefix = fmt.replace("{}", "");
                            Self::intern_string(string_constants, counter, &prefix);
                        }
                    }
                }
                Statement::ListInit { elements, .. } => {
                    for elem in elements {
                        scan_text(elem, string_constants, counter);
                    }
                }
                Statement::MapInit { entries, .. } => {
                    for (k, v) in entries {
                        if !k.starts_with("\x01int:") {
                            Self::intern_string(string_constants, counter, k);
                        }
                        scan_text(v, string_constants, counter);
                    }
                }
                Statement::IndexSet { index_expr, val_expr, .. } => {
                    scan_text(index_expr, string_constants, counter);
                    scan_text(val_expr, string_constants, counter);
                }
                Statement::Assign { target, expr, .. } => {
                    if let Some((_, prop)) = target.split_once('.') {
                        let p = prop.trim();
                        if !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                            scan_text(&format!("\"{}\"", p), string_constants, counter);
                        }
                    }
                    scan_text(expr, string_constants, counter);
                }
                Statement::TupleUnpack { expr, .. }
                | Statement::ListAppend { expr, .. }
                | Statement::ChanSend { expr, .. } => {
                    scan_text(expr, string_constants, counter);
                }
                Statement::Return { expr } => {
                    if let Some(e) = expr {
                        scan_text(e, string_constants, counter);
                    }
                }
                Statement::Concrete { code, .. } => {
                    scan_text(code, string_constants, counter);
                    if let Some((exc_ty_opt, _)) = Self::parse_raise_or_throw(code) {
                        Self::intern_string(string_constants, counter, "");
                        Self::intern_string(string_constants, counter, "Exception");
                        if let Some(exc_ty) = exc_ty_opt {
                            Self::intern_string(string_constants, counter, &exc_ty);
                        }
                    }
                }
                Statement::Match { arms, .. } => {
                    for arm in arms {
                        self.scan_strings_recursive(&arm.body, string_constants, counter);
                    }
                }
                Statement::If { condition, then_branch, else_branch } => {
                    scan_text(condition, string_constants, counter);
                    self.scan_strings_recursive(then_branch, string_constants, counter);
                    if let Some(eb) = else_branch {
                        self.scan_strings_recursive(eb, string_constants, counter);
                    }
                }
                Statement::WhileLoop { condition, body } => {
                    scan_text(condition, string_constants, counter);
                    self.scan_strings_recursive(body, string_constants, counter);
                }
                Statement::ForLoop { count, body, .. } => {
                    scan_text(count, string_constants, counter);
                    self.scan_strings_recursive(body, string_constants, counter);
                }
                Statement::WithContext { context_expr, body, .. } => {
                    scan_text(context_expr, string_constants, counter);
                    self.scan_strings_recursive(body, string_constants, counter);
                }
                Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                    Self::intern_string(string_constants, counter, "");
                    Self::intern_string(string_constants, counter, "Exception");
                    self.scan_strings_recursive(try_block, string_constants, counter);
                    for c in catch_clauses {
                        if let Some(ref et) = c.exception_type {
                            Self::intern_string(string_constants, counter, et);
                        }
                        self.scan_strings_recursive(&c.body, string_constants, counter);
                    }
                    if let Some(ref fb) = finally_block {
                        self.scan_strings_recursive(fb, string_constants, counter);
                    }
                }
                Statement::LocalFnDef { func } => {
                    for p in &func.params {
                        if let Some(ref dv) = p.default_val {
                            scan_text(dv, string_constants, counter);
                        }
                    }
                    self.scan_strings_recursive(&func.body, string_constants, counter);
                }
                _ => {}
            }
        }
    }

    fn harvest_called_symbols(stmts: &[Statement], set: &mut HashSet<String>) {
        omni_parser::harvest_called_symbols(stmts, set);
    }

    fn find_outer_call_open_paren(trimmed: &str) -> Option<usize> {
        if !trimmed.ends_with(')') {
            return None;
        }
        let bytes = trimmed.as_bytes();
        let mut depth = 0i32;
        let mut in_q: u8 = 0;
        let mut idx = bytes.len();
        while idx > 0 {
            idx -= 1;
            let b = bytes[idx];
            if in_q != 0 {
                if b == in_q && (idx == 0 || bytes[idx - 1] != b'\\') {
                    in_q = 0;
                }
                continue;
            }
            if b == b'"' || b == b'\'' {
                in_q = b;
            } else if b == b')' {
                depth += 1;
            } else if b == b'(' {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
        }
        None
    }

    fn strip_str_literal_quotes(s: &str) -> Option<&str> {
        let t = s.trim();
        let rest = if let Some(r) = t
            .strip_prefix("br")
            .or_else(|| t.strip_prefix("rb"))
            .or_else(|| t.strip_prefix("BR"))
            .or_else(|| t.strip_prefix("RB"))
        {
            r
        } else if let Some(r) = t
            .strip_prefix('b')
            .or_else(|| t.strip_prefix('B'))
            .or_else(|| t.strip_prefix('r'))
            .or_else(|| t.strip_prefix('R'))
            .or_else(|| t.strip_prefix('u'))
            .or_else(|| t.strip_prefix('U'))
        {
            r
        } else {
            t
        };
        if rest.len() >= 2
            && ((rest.starts_with('"') && rest.ends_with('"'))
                || (rest.starts_with('\'') && rest.ends_with('\'')))
        {
            Some(&rest[1..rest.len() - 1])
        } else {
            None
        }
    }

    pub fn is_string_expr(expr: &str, scope_tree: &ScopeTree) -> bool {
        let trimmed = expr.trim();
        if Self::strip_str_literal_quotes(trimmed).is_some() {
            return true;
        }
        if trimmed.starts_with("input(") || trimmed == "input()" {
            return true;
        }
        if (trimmed.starts_with("chr(") || trimmed.starts_with("String.fromCharCode("))
            && trimmed.ends_with(')')
        {
            return true;
        }
        for s_suf in &[
            ".upper()", ".toUpperCase()", ".to_uppercase()",
            ".lower()", ".toLowerCase()", ".to_lowercase()",
            ".strip()", ".trim()",
        ] {
            if trimmed.ends_with(s_suf) {
                return true;
            }
        }
        for s_pat in &[".replace(", ".replaceAll(", ".char_at(", ".charAt(", ".substring(", ".substr("] {
            if let Some(lp) = trimmed.rfind(s_pat) {
                if trimmed.ends_with(')') {
                    let rec = trimmed[..lp].trim();
                    if !scope_tree.has_user_struct_method(rec, s_pat.trim_start_matches('.').trim_end_matches('(')) {
                        return true;
                    }
                }
            }
        }
        if STRING_CAST_PREFIXES
            .iter()
            .any(|p| trimmed.starts_with(p) && trimmed.ends_with(')'))
        {
            return true;
        }
        if (trimmed.starts_with("errors.New(") || trimmed.starts_with("fmt.Errorf("))
            && trimmed.ends_with(')')
        {
            return true;
        }
        if trimmed.ends_with(".encode()") || trimmed.ends_with(".decode()") || trimmed.ends_with(".as_bytes()") {
            return true;
        }
        if (trimmed.starts_with("[]byte(") || trimmed.starts_with("bytes(")) && trimmed.ends_with(')') {
            return true;
        }
        if trimmed.starts_with("*(*[]byte)(unsafe.Pointer(") && trimmed.ends_with(')') {
            return true;
        }
        for pat in &[".getpeername(", ".recv(", ".join("] {
            if let Some(lp) = trimmed.find(pat) {
                if !trimmed[..lp].contains('(') {
                    return true;
                }
            }
        }
        if let Some(lp) = Self::find_outer_call_open_paren(trimmed) {
            if trimmed.ends_with(')') {
                let callee = trimmed[..lp].trim();
                if let Some(IrType::Ptr) = scope_tree.fn_return_types.get(callee) {
                    let pk = scope_tree.fn_return_ptr_kinds.get(callee).map(|s| s.as_str());
                    if pk.is_none() || pk == Some("i8* (string)") {
                        return true;
                    }
                }
                if let Some((rec, mname)) = callee.rsplit_once('.') {
                    let rec = rec.trim();
                    let mname = mname.trim();
                    if scope_tree.c_module_aliases.contains(rec)
                        && matches!(scope_tree.extern_fns.get(mname), Some((_, Some(IrType::Ptr))))
                    {
                        return true;
                    }
                    let inner = trimmed[lp + 1..trimmed.len() - 1].trim();
                    let arity = if inner.is_empty() {
                        0
                    } else {
                        omni_parser::OmniParser::split_respecting_quotes(inner, ',').len()
                    };
                    if let Some(b) = scope_tree.resolve(rec) {
                        if b.type_hint.starts_with("%struct.") {
                            let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            let def_cls = scope_tree.resolve_defining_class(cls, mname);
                            if matches!(
                                scope_tree.method_return_types.get(&(def_cls.clone(), mname.to_string())),
                                Some(IrType::Ptr)
                            ) || matches!(
                                scope_tree.infer_structural_method_return_type(&def_cls, mname, arity),
                                Some(IrType::Ptr)
                            ) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        if let Some((left, op, right)) = Self::split_binary_expr(trimmed) {
            if op == "+" {
                return Self::is_string_expr(&left, scope_tree) || Self::is_string_expr(&right, scope_tree);
            }
        }
        if trimmed.contains('.') && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() == 2 {
                let rec = parts[0].trim();
                let fld = parts[1].trim();
                if let Some(b) = scope_tree.resolve(rec) {
                    if b.type_hint.starts_with("%struct.") {
                        let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        return scope_tree
                            .class_field_types
                            .get(cls_name)
                            .and_then(|m| m.get(fld))
                            .map(|th| th.starts_with("i8*") || is_string_type_hint(th))
                            .unwrap_or(false);
                    }
                }
            }
        }
        if let Ok(ast) = OmniParser::parse_expr(trimmed) {
            match ast.node {
                ExprAst::Index { target, .. } => {
                    let target_s = target.slice(trimmed);
                    if Self::is_string_expr(target_s, scope_tree) {
                        return true;
                    }
                    let (_, elem_tag) = Self::resolve_collection_target_info(target_s, scope_tree);
                    if let Some(tag) = elem_tag {
                        if is_string_type_hint(&tag) {
                            return true;
                        }
                    }
                }
                ExprAst::Slice { target, .. } => {
                    let target_s = target.slice(trimmed);
                    if Self::is_string_expr(target_s, scope_tree) {
                        return true;
                    }
                }
                ExprAst::Call { callee, .. } => {
                    if let ExprAst::Member { target, field } = callee.node {
                        if field == "slice" && Self::is_string_expr(target.slice(trimmed), scope_tree) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(b) = scope_tree.resolve(trimmed) {
            return is_string_type_hint(&b.type_hint);
        }
        false
    }

    pub fn eval_str_ptr(
        &self,
        expr: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let trimmed = expr.trim();

        // 1. String / byte literal (`"..."`, `'...'`, `b'...'`, `r"..."`, etc.)
        if let Some(text) = Self::strip_str_literal_quotes(trimmed) {
            if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                let str_ptr = format!("%str_lit_eval_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                    str_ptr, len, len, const_name
                ));
                return str_ptr;
            }
        }

        if is_null_literal(trimmed) {
            return "null".to_string();
        }

        // Top-level ternary (`cond ? a : b` or `a if cond else b`) and `+` concatenation first
        // so `str(a) + str(b)` splits at `+` instead of matching `str(...)` across both calls.
        if let Ok(ast) = OmniParser::parse_expr(trimmed) {
            match ast.node {
                ExprAst::Ternary { cond, then_br, else_br } => {
                    let cond_v = self.eval_cond(cond.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let tr_p = self.eval_str_ptr(then_br.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let fa_p = self.eval_str_ptr(else_br.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let tern_ptr = format!("%tern_str_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = select i1 {}, i8* {}, i8* {}\n",
                        tern_ptr, cond_v, tr_p, fa_p
                    ));
                    return tern_ptr;
                }
                ExprAst::Binary { ref op, ref lhs, ref rhs } if op == "+" => {
                    let left_ptr = self.eval_str_ptr(lhs.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let right_ptr = self.eval_str_ptr(rhs.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let concat_res = format!("%concat_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_str_concat(i8* {}, i8* {})\n",
                        concat_res, left_ptr, right_ptr
                    ));
                    return concat_res;
                }
                _ => {}
            }
        }

        // Go `errors.New(msg)` / `fmt.Errorf(fmt, ...)` error string constructors
        for err_prefix in &["errors.New(", "fmt.Errorf("] {
            if trimmed.starts_with(err_prefix) && trimmed.ends_with(')') {
                let inner = trimmed[err_prefix.len()..trimmed.len() - 1].trim();
                let args = omni_parser::OmniParser::split_respecting_quotes(inner, ',');
                if let Some(first) = args.first() {
                    return self.eval_str_ptr(first.trim(), ir, scope_tree, string_constants, temp_counter);
                }
                return "null".to_string();
            }
        }

        // Go / Rust / Python zero-copy string-to-bytes views (`[]byte(s)`, `s.as_bytes()`, `*(*[]byte)(unsafe.Pointer(&sliceHeader{s, len(s)}))`)
        if trimmed.ends_with(".as_bytes()") {
            let base = trimmed[..trimmed.len() - 11].trim();
            if !scope_tree.has_user_struct_method(base, "as_bytes") {
                return self.eval_str_ptr(base, ir, scope_tree, string_constants, temp_counter);
            }
        }
        if let Some(inner) = trimmed.strip_prefix("[]byte(").and_then(|s| s.strip_suffix(')')) {
            return self.eval_str_ptr(inner.trim(), ir, scope_tree, string_constants, temp_counter);
        }
        if trimmed.starts_with("bytes(")
            && Self::find_outer_call_open_paren(trimmed) == Some(5)
            && !scope_tree.known_functions.contains("bytes")
        {
            let inner_t = trimmed[6..trimmed.len() - 1].trim();
            if inner_t.is_empty() {
                if let Some((cn, clen)) = Self::find_string_constant(string_constants, "") {
                    let empty_b = format!("%bytes_empty_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                        empty_b, clen, clen, cn
                    ));
                    return empty_b;
                }
                return "null".to_string();
            }
            if Self::is_string_expr(inner_t, scope_tree) {
                return self.eval_str_ptr(inner_t, ir, scope_tree, string_constants, temp_counter);
            }
            // Python 1-element tuple idiom `bytes((b,))`: allocate exact 2-byte `[b, 0]` buffer (safe even for `b == 0`)
            if inner_t.starts_with('(')
                && inner_t.ends_with(')')
                && inner_t[1..inner_t.len() - 1].trim().ends_with(',')
            {
                let elem_expr = inner_t[1..inner_t.len() - 1].trim().trim_end_matches(',').trim();
                let tv = self.eval_typed(elem_expr, ir, scope_tree, string_constants, temp_counter);
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                let b_buf = format!("%bytes_1b_{}", *temp_counter);
                let b_tr = format!("%bytes_tr_{}", *temp_counter);
                let b_p1 = format!("%bytes_p1_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = call i8* @malloc(i64 2)\n  {} = trunc i64 {} to i8\n  store i8 {}, i8* {}\n  {} = getelementptr inbounds i8, i8* {}, i64 1\n  store i8 0, i8* {}\n",
                    b_buf, b_tr, v64, b_tr, b_buf, b_p1, b_buf, b_p1
                ));
                return b_buf;
            }
            let tv = self.eval_typed(inner_t, ir, scope_tree, string_constants, temp_counter);
            if tv.ty == IrType::Ptr {
                // Convert `OmniSlice` of byte integers `[65, 66]` into a C byte buffer if slice, or pass C string through
                let bid = *temp_counter;
                *temp_counter += 1;
                let sl_len = format!("%bytes_sllen_{}", bid);
                let sl_alloc = format!("%bytes_slsz_{}", bid);
                let out_buf = format!("%bytes_slbuf_{}", bid);
                let idx_slot = format!("%bytes_slidx_{}", bid);
                ir.push_str(&format!(
                    "  {} = call i64 @__omni_slice_len(i8* {})\n  {} = add i64 {}, 1\n  {} = call i8* @malloc(i64 {})\n  {} = alloca i64\n  store i64 0, i64* {}\n  br label %bytes_sl_cond_{}\n\nbytes_sl_cond_{}:\n  %bsl_i_{} = load i64, i64* {}\n  %bsl_c_{} = icmp slt i64 %bsl_i_{}, {}\n  br i1 %bsl_c_{}, label %bytes_sl_body_{}, label %bytes_sl_done_{}\n\nbytes_sl_body_{}:\n  %bsl_v_{} = call i64 @__omni_slice_get(i8* {}, i64 %bsl_i_{})\n  %bsl_t_{} = trunc i64 %bsl_v_{} to i8\n  %bsl_p_{} = getelementptr inbounds i8, i8* {}, i64 %bsl_i_{}\n  store i8 %bsl_t_{}, i8* %bsl_p_{}\n  %bsl_n_{} = add i64 %bsl_i_{}, 1\n  store i64 %bsl_n_{}, i64* {}\n  br label %bytes_sl_cond_{}\n\nbytes_sl_done_{}:\n  %bsl_end_{} = getelementptr inbounds i8, i8* {}, i64 {}\n  store i8 0, i8* %bsl_end_{}\n",
                    sl_len, tv.reg, sl_alloc, sl_len, out_buf, sl_alloc, idx_slot, idx_slot, bid,
                    bid, bid, idx_slot, bid, bid, sl_len, bid, bid, bid,
                    bid, bid, tv.reg, bid, bid, bid, bid, out_buf, bid, bid, bid, bid, bid, bid, idx_slot, bid,
                    bid, bid, out_buf, sl_len, bid
                ));
                return out_buf;
            }
            let v64 = self.coerce_to_i64(tv, ir, temp_counter);
            let clamped = format!("%bytes_clmp_{}", *temp_counter);
            let is_neg = format!("%bytes_neg_{}", *temp_counter);
            let sz1 = format!("%bytes_sz1_{}", *temp_counter);
            let z_buf = format!("%bytes_zbuf_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = icmp slt i64 {}, 0\n  {} = select i1 {}, i64 0, i64 {}\n  {} = add i64 {}, 1\n  {} = call i8* @__omni_obj_alloc(i64 {}, i64 0)\n",
                is_neg, v64, clamped, is_neg, v64, sz1, clamped, z_buf, sz1
            ));
            return z_buf;
        }
        if trimmed.starts_with("*(*[]byte)(unsafe.Pointer(") && trimmed.ends_with(')') {
            if let Some(brace_open) = trimmed.find('{') {
                if let Some(brace_close) = trimmed[brace_open + 1..].find('}') {
                    let fields = &trimmed[brace_open + 1..brace_open + 1 + brace_close];
                    let parts = omni_parser::OmniParser::split_respecting_quotes(fields, ',');
                    if let Some(first_fld) = parts.first() {
                        let clean_fld = first_fld.rsplit(':').next().unwrap_or(first_fld).trim();
                        if !clean_fld.is_empty() {
                            return self.eval_str_ptr(clean_fld, ir, scope_tree, string_constants, temp_counter);
                        }
                    }
                }
            }
        }
        if trimmed.ends_with(".decode()") {
            let base = trimmed[..trimmed.len() - 9].trim();
            if !scope_tree.has_user_struct_method(base, "decode") {
                return self.eval_str_ptr(base, ir, scope_tree, string_constants, temp_counter);
            }
        }
        if trimmed.ends_with(".encode()") {
            let base = trimmed[..trimmed.len() - 9].trim();
            if !scope_tree.has_user_struct_method(base, "encode") {
                return self.eval_str_ptr(base, ir, scope_tree, string_constants, temp_counter);
            }
        }

        // Top-level ternary conditional returning `i8*` (`cond ? then_str : else_str`)
        if let Some((cond_s, tr_s, fa_s)) = split_ternary_expr(trimmed) {
            let cond_v = self.eval_cond(cond_s, ir, scope_tree, string_constants, temp_counter);
            let tr_p = self.eval_str_ptr(tr_s, ir, scope_tree, string_constants, temp_counter);
            let fa_p = self.eval_str_ptr(fa_s, ir, scope_tree, string_constants, temp_counter);
            let tern_ptr = format!("%tern_str_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = select i1 {}, i8* {}, i8* {}\n",
                tern_ptr, cond_v, tr_p, fa_p
            ));
            return tern_ptr;
        }

        // Universal `repr(x)` in string context: maps -> JSON representation, others -> eval_str_ptr
        if trimmed.starts_with("repr(") && trimmed.ends_with(')') && !scope_tree.known_functions.contains("repr") {
            let inner = trimmed[5..trimmed.len() - 1].trim();
            if let Some(b) = scope_tree.resolve(inner) {
                if b.type_hint.contains("omni_map") {
                    let ld = format!("%repr_m_{}", *temp_counter);
                    let js = format!("%repr_js_{}", *temp_counter);
                    *temp_counter += 2;
                    ir.push_str(&format!(
                        "  {} = load i8*, i8** {}\n  {} = call i8* @__omni_map_to_json(i8* {})\n",
                        ld, b.llvm_ptr, js, ld
                    ));
                    return js;
                }
            }
            return self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
        }

        // Polyglot string casts: Python `str(x)`, JS `String(x)`, Lua `tostring(x)`
        for prefix in STRING_CAST_PREFIXES {
            if trimmed.starts_with(prefix) && trimmed.ends_with(')') {
                let fn_ident = prefix.trim_end_matches('(');
                if !scope_tree.known_functions.contains(fn_ident) {
                    let inner = trimmed[prefix.len()..trimmed.len() - 1].trim();
                    return self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
                }
            }
        }

        // Builtin input(prompt)
        if trimmed == "input()" || (trimmed.starts_with("input(") && trimmed.ends_with(')')) {
            let prompt_ptr = if trimmed == "input()" {
                "null".to_string()
            } else {
                let inner = trimmed[6..trimmed.len() - 1].trim();
                if inner.is_empty() {
                    "null".to_string()
                } else {
                    self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter)
                }
            };
            let input_ptr = format!("%input_str_res_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = call i8* @__omni_input(i8* {})\n", input_ptr, prompt_ptr));
            return input_ptr;
        }

        // POSIX socket getpeername / recv returning `i8*` unless receiver is a user struct with that method.
        if let Some(lp) = trimmed.find(".getpeername(") {
            if !trimmed[..lp].contains('(') {
                let rec = trimmed[..lp].trim();
                if !scope_tree.has_user_struct_method(rec, "getpeername") {
                    let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                    let peer_ptr = format!("%peer_str_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_os_socket_getpeername(i32 {})\n", peer_ptr, r_fd));
                    return peer_ptr;
                }
            }
        }
        if let Some(lp) = trimmed.find(".recv(") {
            if !trimmed[..lp].contains('(') {
                let rec = trimmed[..lp].trim();
                if !scope_tree.has_user_struct_method(rec, "recv") {
                    let inner = &trimmed[lp + 6..trimmed.len() - 1].trim();
                    let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                    let max_bytes = if inner.is_empty() { "1024".to_string() } else { self.eval_val(inner, ir, scope_tree, string_constants, temp_counter) };
                    let recv_ptr = format!("%recv_str_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_os_socket_recv(i32 {}, i32 {})\n", recv_ptr, r_fd, max_bytes));
                    return recv_ptr;
                }
            }
        }

        if let Some(lp) = trimmed.find(".join(") {
            if !trimmed[..lp].contains('(') {
                let str_res = format!("%join_str_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i8* @__omni_int_to_str(i64 0)\n", str_res));
                return str_res;
            }
        }

        // 2. Binary concatenation: left + right
        if let Some((left, op, right)) = Self::split_binary_expr(trimmed) {
            if op == "+" {
                let left_ptr = self.eval_str_ptr(&left, ir, scope_tree, string_constants, temp_counter);
                let right_ptr = self.eval_str_ptr(&right, ir, scope_tree, string_constants, temp_counter);
                let concat_res = format!("%concat_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = call i8* @__omni_str_concat(i8* {}, i8* {})\n",
                    concat_res, left_ptr, right_ptr
                ));
                return concat_res;
            }
        }

        // 3. Struct field access: rec.fld
        if trimmed.contains('.') && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() == 2 {
                let rec = parts[0].trim();
                let fld = parts[1].trim();
                if let Some(b) = scope_tree.resolve(rec) {
                    if b.type_hint.starts_with("%struct.") {
                        let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fld)) {
                            let is_float_fld = scope_tree
                                .class_field_types
                                .get(cls_name)
                                .and_then(|m| m.get(fld))
                                .map(|th| is_float_type_hint(th))
                                .unwrap_or(false);
                            let is_str_fld = scope_tree
                                .class_field_types
                                .get(cls_name)
                                .and_then(|m| m.get(fld))
                                .map(|th| th.starts_with("i8*") || is_string_type_hint(th))
                                .unwrap_or(false);

                            let inst_ld = format!("%inst_fld_str_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                            let gep = format!("%str_gep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                            let val64 = format!("%val64_str_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));
                            if is_str_fld {
                                let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", str_ptr, val64));
                                return str_ptr;
                            } else if is_float_fld {
                                let fval = format!("%fld_f64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fval, val64));
                                let fstr = format!("%fld_fstr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_float_to_str(double {})\n", fstr, fval));
                                return fstr;
                            } else {
                                let istr = format!("%fld_istr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_int_to_str(i64 {})\n", istr, val64));
                                return istr;
                            }
                        }
                    }
                }
            }
        }

        // 4. Local string variable in scope
        if let Some(b) = scope_tree.resolve(trimmed) {
            if is_string_type_hint(&b.type_hint) {
                if b.type_hint.starts_with("i8*") {
                    let str_ptr = format!("%var_str_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", str_ptr, b.llvm_ptr));
                    return str_ptr;
                } else {
                    let val64 = format!("%var_val64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, b.llvm_ptr));
                    let str_ptr = format!("%var_str_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", str_ptr, val64));
                    return str_ptr;
                }
            }
        }

        // 5. Typed fallback: format `double` via `@__omni_float_to_str`, pass `i8*`, or format `i64` via `@__omni_int_to_str`
        ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
        let tv = self.eval_typed(trimmed, ir, scope_tree, string_constants, temp_counter);
        match tv.ty {
            IrType::F64 => {
                let flt_str = format!("%flt_str_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i8* @__omni_float_to_str(double {})\n", flt_str, tv.reg));
                flt_str
            }
            IrType::Ptr => {
                // Consume TLS slot so nested `f() + g()` never leaks `f()`'s pointer into `g()`
                let _ = format!("%consume_str_{}", *temp_counter);
                ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                tv.reg
            }
            IrType::StructPtr(cls) => {
                let raw = format!("%struct_str_cast_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw, cls, tv.reg));
                raw
            }
            IrType::I64 | IrType::I32 | IrType::I1 => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                if trimmed.ends_with(')') {
                    let ret_ptr = format!("%call_str_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", ret_ptr));
                    let is_non_null = format!("%call_str_non_null_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = icmp ne i8* {}, null\n", is_non_null, ret_ptr));
                    let int_str = format!("%int_str_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_int_to_str(i64 {})\n", int_str, v64));
                    let sel_str = format!("%sel_str_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = select i1 {}, i8* {}, i8* {}\n", sel_str, is_non_null, ret_ptr, int_str));
                    return sel_str;
                }
                let int_str = format!("%int_str_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i8* @__omni_int_to_str(i64 {})\n", int_str, v64));
                int_str
            }
        }
    }

    fn strip_kwarg_prefix(arg: &str) -> &str {
        let trimmed = arg.trim();
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            return trimmed;
        }
        if let Some((lhs, rhs)) = trimmed.split_once('=') {
            if !rhs.starts_with('=')
                && !lhs.ends_with('!')
                && !lhs.ends_with('<')
                && !lhs.ends_with('>')
                && !lhs.trim().is_empty()
                && lhs
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return rhs.trim();
            }
        }
        trimmed
    }

    fn coerce_to_i64(&self, tv: TypedValue, ir: &mut String, temp_counter: &mut usize) -> String {
        match tv.ty {
            IrType::I64 => tv.reg,
            IrType::I32 => {
                if tv.reg.parse::<i64>().is_ok() {
                    tv.reg
                } else {
                    let ext = format!("%sext64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, tv.reg));
                    ext
                }
            }
            IrType::I1 => {
                let ext = format!("%zext64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = zext i1 {} to i64\n", ext, tv.reg));
                ext
            }
            IrType::F64 => {
                let f2i = format!("%fptosi64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = fptosi double {} to i64\n", f2i, tv.reg));
                f2i
            }
            IrType::Ptr => {
                if tv.reg == "null" {
                    "0".to_string()
                } else {
                    let p2i = format!("%p2i64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", p2i, tv.reg));
                    p2i
                }
            }
            IrType::StructPtr(cls) => {
                if tv.reg == "null" {
                    "0".to_string()
                } else {
                    let p2i = format!("%sp2i64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = ptrtoint %struct.{}* {} to i64\n", p2i, cls, tv.reg));
                    p2i
                }
            }
        }
    }

    /// Packs any `TypedValue` into a lossless 64-bit ABI slot (`double` is `bitcast` to `i64` rather than truncated).
    fn coerce_to_raw_i64_bits(
        &self,
        tv: TypedValue,
        ir: &mut String,
        temp_counter: &mut usize,
    ) -> String {
        if tv.ty == IrType::F64 {
            let bits = format!("%f64_bits_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = bitcast double {} to i64\n", bits, tv.reg));
            bits
        } else {
            self.coerce_to_i64(tv, ir, temp_counter)
        }
    }

    fn coerce_to_i32(&self, tv: TypedValue, ir: &mut String, temp_counter: &mut usize) -> String {
        match tv.ty {
            IrType::I32 => tv.reg,
            IrType::I64 => {
                if let Ok(n64) = tv.reg.parse::<i64>() {
                    (n64 as i32).to_string()
                } else {
                    let tr = format!("%trunc32_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", tr, tv.reg));
                    tr
                }
            }
            IrType::I1 => {
                let ext = format!("%zext32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = zext i1 {} to i32\n", ext, tv.reg));
                ext
            }
            IrType::F64 => {
                let f2i = format!("%fptosi32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = fptosi double {} to i32\n", f2i, tv.reg));
                f2i
            }
            IrType::Ptr | IrType::StructPtr(_) => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                if v64 == "0" {
                    "0".to_string()
                } else {
                    let tr = format!("%ptr_trunc32_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", tr, v64));
                    tr
                }
            }
        }
    }

    fn coerce_to_f64(&self, tv: TypedValue, ir: &mut String, temp_counter: &mut usize) -> String {
        match tv.ty {
            IrType::F64 => tv.reg,
            IrType::I64 => {
                let cvt = format!("%sitofp64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = sitofp i64 {} to double\n", cvt, tv.reg));
                cvt
            }
            IrType::I32 => {
                let cvt = format!("%sitofp32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = sitofp i32 {} to double\n", cvt, tv.reg));
                cvt
            }
            IrType::I1 => {
                let cvt = format!("%uitofp1_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = uitofp i1 {} to double\n", cvt, tv.reg));
                cvt
            }
            IrType::Ptr | IrType::StructPtr(_) => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                let cvt = format!("%psitofp_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = sitofp i64 {} to double\n", cvt, v64));
                cvt
            }
        }
    }

    fn coerce_to_ptr(&self, tv: TypedValue, ir: &mut String, temp_counter: &mut usize) -> String {
        match tv.ty {
            IrType::Ptr => tv.reg,
            IrType::StructPtr(cls) => {
                if tv.reg == "null" {
                    "null".to_string()
                } else {
                    let bc = format!("%sp_to_i8_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", bc, cls, tv.reg));
                    bc
                }
            }
            _ => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                if v64 == "0" {
                    "null".to_string()
                } else {
                    let i2p = format!("%i64_to_p_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", i2p, v64));
                    i2p
                }
            }
        }
    }

    fn apply_return_ir_type(
        ret_i64_reg: String,
        ret_ty: Option<&IrType>,
        ir: &mut String,
        temp_counter: &mut usize,
    ) -> TypedValue {
        match ret_ty {
            Some(IrType::F64) => {
                let f_reg = format!("%ret_f64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = bitcast i64 {} to double\n", f_reg, ret_i64_reg));
                TypedValue { reg: f_reg, ty: IrType::F64 }
            }
            Some(IrType::Ptr) => {
                let p_reg = format!("%ret_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", p_reg, ret_i64_reg));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p_reg));
                TypedValue { reg: p_reg, ty: IrType::Ptr }
            }
            Some(IrType::StructPtr(cls)) => {
                let sp_reg = format!("%ret_sptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", sp_reg, ret_i64_reg, cls));
                let raw_p = format!("%ret_rawp_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_p, cls, sp_reg));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_p));
                TypedValue { reg: sp_reg, ty: IrType::StructPtr(cls.clone()) }
            }
            _ => TypedValue { reg: ret_i64_reg, ty: IrType::I64 },
        }
    }

    /// Universal polyglot builtin & method reconciler:
    /// Handles `len(x)`, `strlen(x)`, `ord(x)`, `chr(x)`, `list(s)`, first-class string methods,
    /// and abductive cross-ecosystem slice/map/channel methods (`.push`, `.add`, `.put`, `.insert`, `.send`, `.recv`, etc.).
    fn try_eval_universal_call(
        &self,
        full_expr: &str,
        callee: &omni_parser::SpannedExpr,
        args: &[omni_parser::CallArg],
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> Option<TypedValue> {
        match &callee.node {
            ExprAst::Ident(fn_name) => {
                if scope_tree.known_functions.contains(fn_name.as_str()) {
                    return None;
                }
                match fn_name.as_str() {
                    "len" | "strlen" if args.len() == 1 => {
                        let arg_s = args[0].value.slice(full_expr).trim();
                        if Self::is_string_expr(arg_s, scope_tree) {
                            let s_ptr = self.eval_str_ptr(arg_s, ir, scope_tree, string_constants, temp_counter);
                            let len_r = format!("%str_len_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_len(i8* {})\n", len_r, s_ptr));
                            return Some(TypedValue { reg: len_r, ty: IrType::I64 });
                        }
                        if let Some(binding) = scope_tree.resolve(arg_s) {
                            if binding.type_hint.starts_with("i8*") {
                                let loaded = format!("%col_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                                let len_res = format!("%len_res_{}", *temp_counter);
                                *temp_counter += 1;
                                if binding.type_hint.contains("omni_chan") {
                                    ir.push_str(&format!("  {} = call i64 @__omni_chan_len(i8* {})\n", len_res, loaded));
                                } else if binding.type_hint.contains("omni_map") {
                                    ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_res, loaded));
                                } else {
                                    ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, loaded));
                                }
                                return Some(TypedValue { reg: len_res, ty: IrType::I64 });
                            }
                        }
                        let tv = self.eval_typed(arg_s, ir, scope_tree, string_constants, temp_counter);
                        let p = self.coerce_to_ptr(tv, ir, temp_counter);
                        let len_res = format!("%dyn_len_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, p));
                        return Some(TypedValue { reg: len_res, ty: IrType::I64 });
                    }
                    "ord" if args.len() == 1 => {
                        let arg_s = args[0].value.slice(full_expr).trim();
                        let s_ptr = self.eval_str_ptr(arg_s, ir, scope_tree, string_constants, temp_counter);
                        let r = format!("%str_ord_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i64 @__omni_str_ord(i8* {})\n", r, s_ptr));
                        return Some(TypedValue { reg: r, ty: IrType::I64 });
                    }
                    "chr" if args.len() == 1 => {
                        let arg_s = args[0].value.slice(full_expr).trim();
                        let tv = self.eval_typed(arg_s, ir, scope_tree, string_constants, temp_counter);
                        let c64 = self.coerce_to_i64(tv, ir, temp_counter);
                        let r = format!("%str_chr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_str_chr(i64 {})\n", r, c64));
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                        return Some(TypedValue { reg: r, ty: IrType::Ptr });
                    }
                    "list" if args.len() == 1 => {
                        let arg_s = args[0].value.slice(full_expr).trim();
                        if Self::is_string_expr(arg_s, scope_tree) {
                            let s_ptr = self.eval_str_ptr(arg_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%str_chars_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_to_chars(i8* {})\n", r, s_ptr));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                    }
                    _ => {}
                }
                None
            }
            ExprAst::Member { target, field } => {
                let target_s = target.slice(full_expr).trim();
                let m = field.as_str();

                if target_s == "String" && m == "fromCharCode" && args.len() == 1 {
                    let arg_s = args[0].value.slice(full_expr).trim();
                    let tv = self.eval_typed(arg_s, ir, scope_tree, string_constants, temp_counter);
                    let c64 = self.coerce_to_i64(tv, ir, temp_counter);
                    let r = format!("%str_chr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_str_chr(i64 {})\n", r, c64));
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                    return Some(TypedValue { reg: r, ty: IrType::Ptr });
                }

                // Do not hijack explicit user-defined methods on struct instances
                if scope_tree.has_user_struct_method(target_s, m) {
                    return None;
                }
                if let Some(b) = scope_tree.resolve(target_s) {
                    if b.type_hint.starts_with("%struct.") {
                        return None;
                    }
                }

                // 1. First-class String methods (works on literals, vars, slices, concatenations)
                if Self::is_string_expr(target_s, scope_tree) {
                    match m {
                        "len" | "size" | "count" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_len_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_len(i8* {})\n", r, s_ptr));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "is_empty" | "isEmpty" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let l = format!("%s_len_{}", *temp_counter);
                            let c = format!("%s_emp_{}", *temp_counter);
                            let z = format!("%s_emp64_{}", *temp_counter);
                            *temp_counter += 3;
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_str_len(i8* {})\n  {} = icmp eq i64 {}, 0\n  {} = zext i1 {} to i64\n",
                                l, s_ptr, c, l, z, c
                            ));
                            return Some(TypedValue { reg: z, ty: IrType::I64 });
                        }
                        "upper" | "toUpperCase" | "to_uppercase" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_up_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_upper(i8* {})\n", r, s_ptr));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "lower" | "toLowerCase" | "to_lowercase" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_lo_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_lower(i8* {})\n", r, s_ptr));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "strip" | "trim" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_tr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_trim(i8* {})\n", r, s_ptr));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "char_at" | "charAt" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let itv = self.eval_typed(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let i64v = self.coerce_to_i64(itv, ir, temp_counter);
                            let r = format!("%s_cat_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_char_at(i8* {}, i64 {})\n", r, s_ptr, i64v));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "byte_at" | "charCodeAt" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let itv = self.eval_typed(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let i64v = self.coerce_to_i64(itv, ir, temp_counter);
                            let r = format!("%s_bat_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_byte_at(i8* {}, i64 {})\n", r, s_ptr, i64v));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "contains" | "includes" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let n_ptr = self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_has_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_contains(i8* {}, i8* {})\n", r, s_ptr, n_ptr));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "startswith" | "starts_with" | "startsWith" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let p_ptr = self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_sw_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_starts_with(i8* {}, i8* {})\n", r, s_ptr, p_ptr));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "endswith" | "ends_with" | "endsWith" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let p_ptr = self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_ew_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_ends_with(i8* {}, i8* {})\n", r, s_ptr, p_ptr));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "find" | "indexOf" if args.len() == 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let p_ptr = self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_fnd_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_find(i8* {}, i8* {})\n", r, s_ptr, p_ptr));
                            return Some(TypedValue { reg: r, ty: IrType::I64 });
                        }
                        "replace" | "replaceAll" if args.len() == 2 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let old_p = self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let new_p = self.eval_str_ptr(args[1].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_rep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_replace(i8* {}, i8* {}, i8* {})\n", r, s_ptr, old_p, new_p));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "split" if args.len() <= 1 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let sep_p = if args.is_empty() {
                                "null".to_string()
                            } else {
                                self.eval_str_ptr(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter)
                            };
                            let r = format!("%s_spl_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_split(i8* {}, i8* {})\n", r, s_ptr, sep_p));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "chars" | "toCharArray" if args.is_empty() => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let r = format!("%s_chs_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_str_to_chars(i8* {})\n", r, s_ptr));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        "slice" | "substring" if !args.is_empty() && args.len() <= 2 => {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let st_tv = self.eval_typed(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                            let st_64 = self.coerce_to_i64(st_tv, ir, temp_counter);
                            let (en_64, has_en) = if args.len() == 2 {
                                let etv = self.eval_typed(args[1].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                                (self.coerce_to_i64(etv, ir, temp_counter), "1")
                            } else {
                                ("0".to_string(), "0")
                            };
                            let r = format!("%s_sub_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i8* @__omni_str_slice_step(i8* {}, i64 {}, i64 {}, i64 1, i64 1, i64 {})\n",
                                r, s_ptr, st_64, en_64, has_en
                            ));
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", r));
                            return Some(TypedValue { reg: r, ty: IrType::Ptr });
                        }
                        _ => {}
                    }
                }

                // 2. Abductive Cross-Ecosystem Collection & Channel Reconciler
                if let Some(b) = scope_tree.resolve(target_s).cloned() {
                    if b.type_hint.contains("omni_slice") {
                        match m {
                            "push" | "add" | "append" | "enqueue" if args.len() == 1 => {
                                let ld = format!("%abd_sl_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let atv = self.eval_typed(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                                let v64 = self.coerce_to_raw_i64_bits(atv, ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", ld, v64));
                                return Some(TypedValue { reg: "0".to_string(), ty: IrType::I64 });
                            }
                            "len" | "size" | "count" if args.is_empty() => {
                                let ld = format!("%abd_sl_ld_{}", *temp_counter);
                                let r = format!("%abd_sl_len_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n  {} = call i64 @__omni_slice_len(i8* {})\n", ld, b.llvm_ptr, r, ld));
                                return Some(TypedValue { reg: r, ty: IrType::I64 });
                            }
                            "is_empty" | "isEmpty" if args.is_empty() => {
                                let ld = format!("%abd_sl_ld_{}", *temp_counter);
                                let r = format!("%abd_sl_len_{}", *temp_counter);
                                let c = format!("%abd_sl_emp_{}", *temp_counter);
                                let z = format!("%abd_sl_emp64_{}", *temp_counter);
                                *temp_counter += 4;
                                ir.push_str(&format!(
                                    "  {} = load i8*, i8** {}\n  {} = call i64 @__omni_slice_len(i8* {})\n  {} = icmp eq i64 {}, 0\n  {} = zext i1 {} to i64\n",
                                    ld, b.llvm_ptr, r, ld, c, r, z, c
                                ));
                                return Some(TypedValue { reg: z, ty: IrType::I64 });
                            }
                            "contains" | "includes" if args.len() == 1 => {
                                let ld = format!("%abd_sl_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let arg_s = args[0].value.slice(full_expr).trim();
                                let is_str = if Self::is_string_expr(arg_s, scope_tree) { "1" } else { "0" };
                                let atv = self.eval_typed(arg_s, ir, scope_tree, string_constants, temp_counter);
                                let v64 = self.coerce_to_raw_i64_bits(atv, ir, temp_counter);
                                let r = format!("%abd_sl_has_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i64 @__omni_slice_contains(i8* {}, i64 {}, i64 {})\n", r, ld, v64, is_str));
                                return Some(TypedValue { reg: r, ty: IrType::I64 });
                            }
                            _ => {}
                        }
                    } else if b.type_hint.contains("omni_map") {
                        match m {
                            "put" | "insert" | "set" if args.len() == 2 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                let vs = args[1].value.slice(full_expr).trim();
                                let vtv = self.eval_typed(vs, ir, scope_tree, string_constants, temp_counter);
                                let vtag = Self::infer_elem_tag_from_expr(vs, &vtv.ty, scope_tree);
                                let vkind = Self::elem_tag_to_map_kind(&vtag);
                                let v64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);
                                if Self::is_string_expr(ks, scope_tree) {
                                    let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n", ld, kp, v64, vkind));
                                } else {
                                    let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                                    let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                    ir.push_str(&format!("  call void @__omni_map_set_int_typed(i8* {}, i64 {}, i64 {}, i64 {})\n", ld, k64, v64, vkind));
                                }
                                return Some(TypedValue { reg: "0".to_string(), ty: IrType::I64 });
                            }
                            "get" if !args.is_empty() && args.len() <= 2 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_get_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                let mut per_key_tag: Option<String> = None;
                                if (ks.starts_with('"') && ks.ends_with('"')) || (ks.starts_with('\'') && ks.ends_with('\'')) {
                                    let kt = &ks[1..ks.len() - 1];
                                    per_key_tag = scope_tree.map_key_types.get(&(target_s.to_string(), kt.to_string())).cloned();
                                }
                                if args.len() == 2 && Self::is_string_expr(ks, scope_tree) {
                                    let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                    let ds = args[1].value.slice(full_expr).trim();
                                    let dtv = self.eval_typed(ds, ir, scope_tree, string_constants, temp_counter);
                                    let dtag = Self::infer_elem_tag_from_expr(ds, &dtv.ty, scope_tree);
                                    let d64 = self.coerce_to_raw_i64_bits(dtv, ir, temp_counter);
                                    ir.push_str(&format!("  {} = call i64 @__omni_map_get_or(i8* {}, i8* {}, i64 {})\n", r, ld, kp, d64));
                                    let etag = per_key_tag.as_deref().or(Some(dtag.as_str()));
                                    return Some(Self::unpack_raw_elem_bits(r, etag, ir, temp_counter));
                                } else if Self::is_string_expr(ks, scope_tree) {
                                    let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n", r, ld, kp));
                                } else {
                                    let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                                    let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                    ir.push_str(&format!("  {} = call i64 @__omni_map_get_int(i8* {}, i64 {})\n", r, ld, k64));
                                }
                                let etag = per_key_tag.as_deref().or_else(|| scope_tree.collection_elem_types.get(target_s).map(|s| s.as_str()));
                                return Some(Self::unpack_raw_elem_bits(r, etag, ir, temp_counter));
                            }
                            "keys" if args.is_empty() => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_keys_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n  {} = call i8* @__omni_map_keys(i8* {})\n  call void @__omni_c_return_ptr(i8* {})\n", ld, b.llvm_ptr, r, ld, r));
                                return Some(TypedValue { reg: r, ty: IrType::Ptr });
                            }
                            "values" if args.is_empty() => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_vals_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n  {} = call i8* @__omni_map_values(i8* {})\n  call void @__omni_c_return_ptr(i8* {})\n", ld, b.llvm_ptr, r, ld, r));
                                return Some(TypedValue { reg: r, ty: IrType::Ptr });
                            }
                            "update" if args.len() == 1 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let other_s = args[0].value.slice(full_expr).trim();
                                let optr = if let Some(ob) = scope_tree.resolve(other_s).cloned() {
                                    if ob.type_hint.contains("omni_map") {
                                        let old = format!("%abd_mp_oth_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", old, ob.llvm_ptr));
                                        old
                                    } else {
                                        let otv = self.eval_typed(other_s, ir, scope_tree, string_constants, temp_counter);
                                        self.coerce_to_ptr(otv, ir, temp_counter)
                                    }
                                } else {
                                    let otv = self.eval_typed(other_s, ir, scope_tree, string_constants, temp_counter);
                                    self.coerce_to_ptr(otv, ir, temp_counter)
                                };
                                ir.push_str(&format!("  call void @__omni_map_update(i8* {}, i8* {})\n", ld, optr));
                                return Some(TypedValue { reg: "0".to_string(), ty: IrType::I64 });
                            }
                            "setdefault" if !args.is_empty() && args.len() <= 2 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_sd_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                let (d64, dkind, dtag) = if args.len() == 2 {
                                    let ds = args[1].value.slice(full_expr).trim();
                                    let dtv = self.eval_typed(ds, ir, scope_tree, string_constants, temp_counter);
                                    let tag = Self::infer_elem_tag_from_expr(ds, &dtv.ty, scope_tree);
                                    let k = Self::elem_tag_to_map_kind(&tag);
                                    (self.coerce_to_raw_i64_bits(dtv, ir, temp_counter), k, tag)
                                } else {
                                    ("0".to_string(), 0, "i64".to_string())
                                };
                                ir.push_str(&format!("  {} = call i64 @__omni_map_setdefault_typed(i8* {}, i8* {}, i64 {}, i64 {})\n", r, ld, kp, d64, dkind));
                                return Some(Self::unpack_raw_elem_bits(r, Some(dtag.as_str()), ir, temp_counter));
                            }
                            "pop" if !args.is_empty() && args.len() <= 2 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_pop_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                let d64 = if args.len() == 2 {
                                    let ds = args[1].value.slice(full_expr).trim();
                                    let dtv = self.eval_typed(ds, ir, scope_tree, string_constants, temp_counter);
                                    self.coerce_to_raw_i64_bits(dtv, ir, temp_counter)
                                } else {
                                    "0".to_string()
                                };
                                ir.push_str(&format!("  {} = call i64 @__omni_map_pop(i8* {}, i8* {}, i64 {})\n", r, ld, kp, d64));
                                let etag = scope_tree.collection_elem_types.get(target_s).map(|s| s.as_str());
                                return Some(Self::unpack_raw_elem_bits(r, etag, ir, temp_counter));
                            }
                            "has_key" | "containsKey" | "contains_key" | "has" | "contains" if args.len() == 1 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r1 = format!("%abd_mp_has1_{}", *temp_counter);
                                let r64 = format!("%abd_mp_has64_{}", *temp_counter);
                                *temp_counter += 3;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                if Self::is_string_expr(ks, scope_tree) {
                                    let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  {} = call i1 @__omni_map_contains(i8* {}, i8* {})\n", r1, ld, kp));
                                } else {
                                    let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                                    let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                    ir.push_str(&format!("  {} = call i1 @__omni_map_contains_int(i8* {}, i64 {})\n", r1, ld, k64));
                                }
                                ir.push_str(&format!("  {} = zext i1 {} to i64\n", r64, r1));
                                return Some(TypedValue { reg: r64, ty: IrType::I64 });
                            }
                            "remove" | "delete" if args.len() == 1 => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let ks = args[0].value.slice(full_expr).trim();
                                if Self::is_string_expr(ks, scope_tree) {
                                    let kp = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  call void @__omni_map_delete(i8* {}, i8* {})\n", ld, kp));
                                } else {
                                    let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                                    let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                    ir.push_str(&format!("  call void @__omni_map_delete_int(i8* {}, i64 {})\n", ld, k64));
                                }
                                return Some(TypedValue { reg: "0".to_string(), ty: IrType::I64 });
                            }
                            "len" | "size" | "count" if args.is_empty() => {
                                let ld = format!("%abd_mp_ld_{}", *temp_counter);
                                let r = format!("%abd_mp_len_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n  {} = call i64 @__omni_map_len(i8* {})\n", ld, b.llvm_ptr, r, ld));
                                return Some(TypedValue { reg: r, ty: IrType::I64 });
                            }
                            _ => {}
                        }
                    } else if b.type_hint.contains("omni_chan") {
                        match m {
                            "send" | "push" if args.len() == 1 => {
                                let ld = format!("%abd_ch_ld_{}", *temp_counter);
                                let r1 = format!("%abd_ch_ok_{}", *temp_counter);
                                let r64 = format!("%abd_ch_ok64_{}", *temp_counter);
                                *temp_counter += 3;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let vtv = self.eval_typed(args[0].value.slice(full_expr).trim(), ir, scope_tree, string_constants, temp_counter);
                                let v64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);
                                ir.push_str(&format!(
                                    "  {} = call i1 @__omni_chan_send_int(i8* {}, i64 {})\n  {} = zext i1 {} to i64\n",
                                    r1, ld, v64, r64, r1
                                ));
                                return Some(TypedValue { reg: r64, ty: IrType::I64 });
                            }
                            "recv" | "pop" if args.is_empty() => {
                                let ld = format!("%abd_ch_ld_{}", *temp_counter);
                                let r = format!("%abd_ch_rv_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!(
                                    "  {} = load i8*, i8** {}\n  {} = call i64 @__omni_chan_recv_int(i8* {})\n",
                                    ld, b.llvm_ptr, r, ld
                                ));
                                return Some(TypedValue { reg: r, ty: IrType::I64 });
                            }
                            "close" if args.is_empty() => {
                                let ld = format!("%abd_ch_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = load i8*, i8** {}\n  call void @__omni_chan_close(i8* {})\n",
                                    ld, b.llvm_ptr, ld
                                ));
                                return Some(TypedValue { reg: "0".to_string(), ty: IrType::I64 });
                            }
                            "len" if args.is_empty() => {
                                let ld = format!("%abd_ch_ld_{}", *temp_counter);
                                let r = format!("%abd_ch_len_{}", *temp_counter);
                                *temp_counter += 2;
                                ir.push_str(&format!(
                                    "  {} = load i8*, i8** {}\n  {} = call i64 @__omni_chan_len(i8* {})\n",
                                    ld, b.llvm_ptr, r, ld
                                ));
                                return Some(TypedValue { reg: r, ty: IrType::I64 });
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Evaluates an expression into a first-class 64-bit `TypedValue` (`I64`, `F64`, `Ptr`, `StructPtr`, `I32`, `I1`).
    pub fn eval_typed(
        &self,
        expr: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> TypedValue {
        let trimmed = expr.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let mut depth = 0;
            let mut wraps_all = true;
            let chars: Vec<char> = trimmed.chars().collect();
            for (idx, &c) in chars.iter().enumerate() {
                if c == '(' { depth += 1; }
                else if c == ')' { depth -= 1; }
                if depth == 0 && idx < chars.len() - 1 {
                    wraps_all = false;
                    break;
                }
            }
            if wraps_all {
                let inner = trimmed[1..trimmed.len() - 1].trim();
                if !inner.contains(',') {
                    return self.eval_typed(inner, ir, scope_tree, string_constants, temp_counter);
                } else if let Some(single_tuple_elem) = inner.strip_suffix(',') {
                    if !single_tuple_elem.contains(',') {
                        return self.eval_typed(single_tuple_elem.trim(), ir, scope_tree, string_constants, temp_counter);
                    }
                }
            }
        }

        // 1a. IEEE-754 double literal (`3.14159`, `1.5e-3`, `2e6`)
        if let Some(fv) = parse_polyglot_f64(trimmed) {
            return TypedValue {
                reg: format_llvm_f64(fv),
                ty: IrType::F64,
            };
        }

        // 1b. 64-bit integer constant (decimal, 0x hex, 0b binary, 0o octal, `_` separators)
        if let Some(num) = parse_polyglot_i64(trimmed) {
            return TypedValue {
                reg: num.to_string(),
                ty: IrType::I64,
            };
        }

        // 1c. String constant literal (`"hello"` or `'world'`)
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            let text = &trimmed[1..trimmed.len() - 1];
            if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                let str_ptr = format!("%str_lit_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                    str_ptr, len, len, const_name
                ));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", str_ptr));
                return TypedValue { reg: str_ptr, ty: IrType::Ptr };
            }
        }

        // Literal keywords
        if is_null_literal(trimmed) {
            ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
            return TypedValue { reg: "null".to_string(), ty: IrType::Ptr };
        }
        if trimmed == "true" || trimmed == "True" {
            return TypedValue { reg: "1".to_string(), ty: IrType::I64 };
        }
        if trimmed == "false" || trimmed == "False" {
            return TypedValue { reg: "0".to_string(), ty: IrType::I64 };
        }

        // First-class inline lambda / closure expression (`lambda x: ...`, `(x, y) => ...`, `|x| ...`)
        if let Some((lam_params, lam_body_expr)) = OmniParser::try_parse_lambda_expr(trimmed) {
            let cid = self.closure_counter.get();
            let lam_stmts = if lam_body_expr.starts_with('{') && lam_body_expr.ends_with('}') {
                let inner_blk = &lam_body_expr[1..lam_body_expr.len() - 1];
                OmniParser::new().parse_module("lambda_blk", inner_blk).top_level_statements
            } else {
                vec![Statement::Return {
                    expr: Some(lam_body_expr),
                }]
            };
            let syn_fn = omni_parser::FunctionDef {
                name: format!("__lambda_{}", cid),
                params: lam_params,
                return_type: None,
                body: lam_stmts,
                is_method: false,
                annotations: Vec::new(),
                memory_ops: Vec::new(),
            };
            let clos_ptr = self.lower_closure_def(&syn_fn, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", clos_ptr));
            return TypedValue { reg: clos_ptr, ty: IrType::Ptr };
        }

        // Pratt AST Top-Level Structural Dispatch:
        // Evaluates lowest-precedence root operators (`Ternary`, `Binary`, `Cast`, `Unary`, `ListLit`,
        // `MapLit`, arbitrary-depth `Index`, and chained `Member`) BEFORE single-call/cast prefix matches
        // so `foo(1) + bar(2)`, `int(a) + int(b)`, `2 * 3 ** 2`, `~x`, `matrix[i][j]`, and `x as f64`
        // lower with exact precedence and zero substring hijacking.
        if let Ok(ast) = OmniParser::parse_expr(trimmed) {
            match ast.node {
                ExprAst::Ternary { cond, then_br, else_br } => {
                    let cond_v = self.eval_cond(cond.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let tr_tv = self.eval_typed(then_br.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    let fa_tv = self.eval_typed(else_br.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                    if tr_tv.ty == IrType::F64 || fa_tv.ty == IrType::F64 {
                        let tr_f = self.coerce_to_f64(tr_tv, ir, temp_counter);
                        let fa_f = self.coerce_to_f64(fa_tv, ir, temp_counter);
                        let tern_reg = format!("%tern_f64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = select i1 {}, double {}, double {}\n",
                            tern_reg, cond_v, tr_f, fa_f
                        ));
                        return TypedValue { reg: tern_reg, ty: IrType::F64 };
                    }
                    if tr_tv.ty == IrType::Ptr && fa_tv.ty == IrType::Ptr {
                        let tern_reg = format!("%tern_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = select i1 {}, i8* {}, i8* {}\n",
                            tern_reg, cond_v, tr_tv.reg, fa_tv.reg
                        ));
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", tern_reg));
                        return TypedValue { reg: tern_reg, ty: IrType::Ptr };
                    }
                    let tr_i = self.coerce_to_i64(tr_tv, ir, temp_counter);
                    let fa_i = self.coerce_to_i64(fa_tv, ir, temp_counter);
                    let tern_reg = format!("%tern_i64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = select i1 {}, i64 {}, i64 {}\n",
                        tern_reg, cond_v, tr_i, fa_i
                    ));
                    return TypedValue { reg: tern_reg, ty: IrType::I64 };
                }
                ExprAst::Binary { op, lhs, rhs } => {
                    let left = lhs.slice(trimmed);
                    let right = rhs.slice(trimmed);
                    if op == "|>" {
                        let ltv = self.eval_typed(left, ir, scope_tree, string_constants, temp_counter);
                        let pid = *temp_counter;
                        *temp_counter += 1;
                        let pipe_sym = format!("__pipe_lhs_{}", pid);
                        let mut sub_scope = scope_tree.clone();
                        match ltv.ty {
                            IrType::F64 => {
                                let fp = format!("%f64_var_pipe_{}", pid);
                                ir.push_str(&format!("  {} = alloca double\n", fp));
                                ir.push_str(&format!("  store double {}, double* {}\n", ltv.reg, fp));
                                sub_scope.insert(
                                    pipe_sym.clone(),
                                    SymbolBinding {
                                        name: pipe_sym.clone(),
                                        llvm_ptr: fp,
                                        type_hint: "double".to_string(),
                                        is_scoped: true,
                                    },
                                );
                            }
                            IrType::Ptr => {
                                let pp = format!("%str_var_pipe_{}", pid);
                                ir.push_str(&format!("  {} = alloca i8*\n", pp));
                                ir.push_str(&format!("  store i8* {}, i8** {}\n", ltv.reg, pp));
                                sub_scope.insert(
                                    pipe_sym.clone(),
                                    SymbolBinding {
                                        name: pipe_sym.clone(),
                                        llvm_ptr: pp,
                                        type_hint: "i8* (string)".to_string(),
                                        is_scoped: true,
                                    },
                                );
                            }
                            IrType::StructPtr(ref cls) => {
                                let sp = format!("%var_pipe_{}", pid);
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", sp, cls));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls, ltv.reg, cls, sp));
                                sub_scope.insert(
                                    pipe_sym.clone(),
                                    SymbolBinding {
                                        name: pipe_sym.clone(),
                                        llvm_ptr: sp,
                                        type_hint: format!("%struct.{}*", cls),
                                        is_scoped: true,
                                    },
                                );
                            }
                            _ => {
                                let v64 = self.coerce_to_i64(ltv, ir, temp_counter);
                                let ip = format!("%var_pipe_{}", pid);
                                ir.push_str(&format!("  {} = alloca i64\n", ip));
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", v64, ip));
                                sub_scope.insert(
                                    pipe_sym.clone(),
                                    SymbolBinding {
                                        name: pipe_sym.clone(),
                                        llvm_ptr: ip,
                                        type_hint: "i64".to_string(),
                                        is_scoped: true,
                                    },
                                );
                            }
                        }
                        let piped_call = if let Some(open) = right.find('(') {
                            if right.ends_with(')') {
                                let fn_head = right[..open].trim();
                                let inner = right[open + 1..right.len() - 1].trim();
                                if inner.is_empty() {
                                    format!("{}({})", fn_head, pipe_sym)
                                } else {
                                    format!("{}({}, {})", fn_head, pipe_sym, inner)
                                }
                            } else {
                                format!("{}({})", right, pipe_sym)
                            }
                        } else {
                            format!("{}({})", right, pipe_sym)
                        };
                        return self.eval_typed(&piped_call, ir, &sub_scope, string_constants, temp_counter);
                    }
                    if matches!(
                        op.as_str(),
                        "||" | "or" | "&&" | "and" | "==" | "!=" | "===" | "!==" | "~=" | "<" | "<=" | ">" | ">=" | "is" | "is not" | "in" | "not in"
                    ) {
                        let c_reg = self.eval_cond(trimmed, ir, scope_tree, string_constants, temp_counter);
                        let z_reg = format!("%bool_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = zext i1 {} to i64\n", z_reg, c_reg));
                        return TypedValue { reg: z_reg, ty: IrType::I64 };
                    }

                    if op == "+" && (Self::is_string_expr(left, scope_tree) || Self::is_string_expr(right, scope_tree)) {
                        let str_ptr = self.eval_str_ptr(trimmed, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", str_ptr));
                        return TypedValue { reg: str_ptr, ty: IrType::Ptr };
                    }

                    if op == "**" {
                        if let (Some(lb), Some(rb)) = (parse_polyglot_i64(left), parse_polyglot_i64(right)) {
                            if (0..=62).contains(&rb) {
                                return TypedValue {
                                    reg: lb.wrapping_pow(rb as u32).to_string(),
                                    ty: IrType::I64,
                                };
                            }
                        }
                    }

                    let ltv = self.eval_typed(left, ir, scope_tree, string_constants, temp_counter);
                    let rtv = self.eval_typed(right, ir, scope_tree, string_constants, temp_counter);

                    if ltv.ty == IrType::F64 || rtv.ty == IrType::F64 {
                        let lf = self.coerce_to_f64(ltv, ir, temp_counter);
                        let rf = self.coerce_to_f64(rtv, ir, temp_counter);
                        let res_var = format!("%tf_{}", *temp_counter);
                        *temp_counter += 1;
                        if op == "**" {
                            if let Some(exp_i) = parse_polyglot_i64(right) {
                                if exp_i == 0 {
                                    return TypedValue { reg: format_llvm_f64(1.0), ty: IrType::F64 };
                                } else if (1..=12).contains(&exp_i) {
                                    let mut acc = lf.clone();
                                    for _ in 1..exp_i {
                                        let step = format!("%powf_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = fmul double {}, {}\n", step, acc, lf));
                                        acc = step;
                                    }
                                    return TypedValue { reg: acc, ty: IrType::F64 };
                                }
                            }
                            ir.push_str(&format!("  {} = call double @llvm.pow.f64(double {}, double {})\n", res_var, lf, rf));
                            return TypedValue { reg: res_var, ty: IrType::F64 };
                        }
                        let finstr = match op.as_str() {
                            "+" => "fadd",
                            "-" => "fsub",
                            "*" => "fmul",
                            "/" => "fdiv",
                            "%" => "frem",
                            _ => "fadd",
                        };
                        ir.push_str(&format!("  {} = {} double {}, {}\n", res_var, finstr, lf, rf));
                        return TypedValue { reg: res_var, ty: IrType::F64 };
                    }

                    let left_val = self.coerce_to_i64(ltv, ir, temp_counter);
                    let right_val = self.coerce_to_i64(rtv, ir, temp_counter);

                    let res_var = format!("%t_{}", *temp_counter);
                    *temp_counter += 1;

                    let instr = match op.as_str() {
                        "+" => "add",
                        "-" => "sub",
                        "*" => "mul",
                        "/" | "%" | "//" => {
                            return self.emit_checked_int_div_rem(
                                &op,
                                &left_val,
                                &right_val,
                                ir,
                                string_constants,
                                temp_counter,
                            );
                        }
                        "^" => "xor",
                        "&" => "and",
                        "|" => "or",
                        "<<" => "shl",
                        ">>" => "ashr",
                        ">>>" => "lshr",
                        "**" => {
                            if let Some(exp_i) = parse_polyglot_i64(right) {
                                if exp_i == 0 {
                                    return TypedValue { reg: "1".to_string(), ty: IrType::I64 };
                                } else if (1..=16).contains(&exp_i) {
                                    let mut acc = left_val.clone();
                                    for _ in 1..exp_i {
                                        let step = format!("%pow64_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = mul i64 {}, {}\n", step, acc, left_val));
                                        acc = step;
                                    }
                                    return TypedValue { reg: acc, ty: IrType::I64 };
                                }
                            }
                            ir.push_str(&format!("  {} = call i64 @__omni_ipow64(i64 {}, i64 {})\n", res_var, left_val, right_val));
                            return TypedValue { reg: res_var, ty: IrType::I64 };
                        }
                        _ => "add",
                    };

                    ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, instr, left_val, right_val));
                    return TypedValue { reg: res_var, ty: IrType::I64 };
                }
                ExprAst::Cast { expr: inner, target_type } => {
                    let inner_s = inner.slice(trimmed);
                    let tt = target_type.trim();
                    if is_float_type_hint(tt) {
                        let tv = self.eval_typed(inner_s, ir, scope_tree, string_constants, temp_counter);
                        let freg = self.coerce_to_f64(tv, ir, temp_counter);
                        return TypedValue { reg: freg, ty: IrType::F64 };
                    } else if is_string_type_hint(tt) {
                        let sptr = self.eval_str_ptr(inner_s, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", sptr));
                        return TypedValue { reg: sptr, ty: IrType::Ptr };
                    } else {
                        let tv = self.eval_typed(inner_s, ir, scope_tree, string_constants, temp_counter);
                        let ireg = self.coerce_to_i64(tv, ir, temp_counter);
                        if matches!(tt, "i32" | "int32" | "int32_t" | "c_int") {
                            let tr = format!("%cast_tr32_{}", *temp_counter);
                            let sx = format!("%cast_sx64_{}", *temp_counter);
                            *temp_counter += 2;
                            ir.push_str(&format!(
                                "  {} = trunc i64 {} to i32\n  {} = sext i32 {} to i64\n",
                                tr, ireg, sx, tr
                            ));
                            return TypedValue { reg: sx, ty: IrType::I64 };
                        } else if matches!(tt, "u32" | "uint32" | "uint32_t") {
                            let tr = format!("%cast_utr32_{}", *temp_counter);
                            let zx = format!("%cast_zx64_{}", *temp_counter);
                            *temp_counter += 2;
                            ir.push_str(&format!(
                                "  {} = trunc i64 {} to i32\n  {} = zext i32 {} to i64\n",
                                tr, ireg, zx, tr
                            ));
                            return TypedValue { reg: zx, ty: IrType::I64 };
                        }
                        return TypedValue { reg: ireg, ty: IrType::I64 };
                    }
                }
                ExprAst::Unary { ref op, ref rhs } => {
                    let inner_s = rhs.slice(trimmed);
                    match op.as_str() {
                        "!" | "not" => {
                            let c_reg = self.eval_cond(trimmed, ir, scope_tree, string_constants, temp_counter);
                            let z_reg = format!("%not_i64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = zext i1 {} to i64\n", z_reg, c_reg));
                            return TypedValue { reg: z_reg, ty: IrType::I64 };
                        }
                        "~" => {
                            let tv = self.eval_typed(inner_s, ir, scope_tree, string_constants, temp_counter);
                            let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                            let res = format!("%bitnot_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = xor i64 {}, -1\n", res, v64));
                            return TypedValue { reg: res, ty: IrType::I64 };
                        }
                        "-" => {
                            let tv = self.eval_typed(inner_s, ir, scope_tree, string_constants, temp_counter);
                            if tv.ty == IrType::F64 {
                                let res = format!("%fneg_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = fsub double 0x0000000000000000, {}\n", res, tv.reg));
                                return TypedValue { reg: res, ty: IrType::F64 };
                            } else {
                                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                                let res = format!("%ineg_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = sub i64 0, {}\n", res, v64));
                                return TypedValue { reg: res, ty: IrType::I64 };
                            }
                        }
                        "+" => {
                            return self.eval_typed(inner_s, ir, scope_tree, string_constants, temp_counter);
                        }
                        _ => {}
                    }
                }
                ExprAst::ListLit(elems) => {
                    let cap = elems.len().max(4);
                    let slice_res = format!("%rval_slice_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_slice_create(i64 {})\n", slice_res, cap));
                    for elem in &elems {
                        let es = elem.slice(trimmed);
                        let etv = self.eval_typed(es, ir, scope_tree, string_constants, temp_counter);
                        let ebits = self.coerce_to_raw_i64_bits(etv, ir, temp_counter);
                        ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", slice_res, ebits));
                    }
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", slice_res));
                    return TypedValue { reg: slice_res, ty: IrType::Ptr };
                }
                ExprAst::MapLit(entries) => {
                    let cap = entries.len().max(8);
                    let map_res = format!("%rval_map_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_map_create(i64 {})\n", map_res, cap));
                    for (k, v) in &entries {
                        let ks = k.slice(trimmed);
                        let vs = v.slice(trimmed);
                        let vtv = self.eval_typed(vs, ir, scope_tree, string_constants, temp_counter);
                        let vtag = Self::infer_elem_tag_from_expr(vs, &vtv.ty, scope_tree);
                        let vkind = Self::elem_tag_to_map_kind(&vtag);
                        let vbits = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);
                        if Self::is_string_expr(ks, scope_tree)
                            || (ks.starts_with('"') && ks.ends_with('"'))
                            || (ks.starts_with('\'') && ks.ends_with('\''))
                        {
                            let kptr = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                            ir.push_str(&format!("  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n", map_res, kptr, vbits, vkind));
                        } else {
                            let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                            let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                            ir.push_str(&format!("  call void @__omni_map_set_int_typed(i8* {}, i64 {}, i64 {}, i64 {})\n", map_res, k64, vbits, vkind));
                        }
                    }
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", map_res));
                    return TypedValue { reg: map_res, ty: IrType::Ptr };
                }
                ExprAst::Index { ref target, ref index } => {
                    let target_s = target.slice(trimmed);
                    let idx_expr = index.slice(trimmed);
                    if Self::is_string_expr(target_s, scope_tree) {
                        let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                        let idx_tv = self.eval_typed(idx_expr, ir, scope_tree, string_constants, temp_counter);
                        let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                        let ch_res = format!("%str_char_at_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i8* @__omni_str_char_at(i8* {}, i64 {})\n",
                            ch_res, s_ptr, idx_64
                        ));
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", ch_res));
                        return TypedValue { reg: ch_res, ty: IrType::Ptr };
                    }
                    let (is_map, elem_tag) = Self::resolve_collection_target_info(target_s, scope_tree);
                    let col_ptr = if let Some(b) = scope_tree.resolve(target_s) {
                        if b.type_hint.starts_with("i8*") {
                            let loaded = format!("%col_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                            Some(loaded)
                        } else {
                            let t_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                            Some(self.coerce_to_ptr(t_tv, ir, temp_counter))
                        }
                    } else {
                        let t_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                        if t_tv.ty == IrType::Ptr || matches!(t_tv.ty, IrType::StructPtr(_)) {
                            Some(self.coerce_to_ptr(t_tv, ir, temp_counter))
                        } else {
                            None
                        }
                    };

                    if let Some(loaded) = col_ptr {
                        if is_map {
                            let val_res = format!("%map_get_{}", *temp_counter);
                            *temp_counter += 1;
                            let mut per_key_tag: Option<String> = None;
                            if (idx_expr.starts_with('"') && idx_expr.ends_with('"'))
                                || (idx_expr.starts_with('\'') && idx_expr.ends_with('\''))
                            {
                                let text = &idx_expr[1..idx_expr.len() - 1];
                                per_key_tag = scope_tree
                                    .map_key_types
                                    .get(&(target_s.to_string(), text.to_string()))
                                    .cloned();
                                if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                                    let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        str_ptr, len, len, const_name
                                    ));
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                                        val_res, loaded, str_ptr
                                    ));
                                } else {
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_map_get(i8* {}, i8* null)\n",
                                        val_res, loaded
                                    ));
                                }
                            } else if Self::is_string_expr(idx_expr, scope_tree) {
                                let key_ptr = self.eval_str_ptr(idx_expr, ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                                    val_res, loaded, key_ptr
                                ));
                            } else {
                                let idx_tv = self.eval_typed(idx_expr, ir, scope_tree, string_constants, temp_counter);
                                let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_map_get_int(i8* {}, i64 {})\n",
                                    val_res, loaded, idx_64
                                ));
                            }
                            let eff_tag = per_key_tag.as_deref().or(elem_tag.as_deref());
                            return Self::unpack_raw_elem_bits(val_res, eff_tag, ir, temp_counter);
                        } else {
                            let idx_tv = self.eval_typed(idx_expr, ir, scope_tree, string_constants, temp_counter);
                            let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                            let val_res = format!("%slice_get_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n",
                                val_res, loaded, idx_64
                            ));
                            return Self::unpack_raw_elem_bits(val_res, elem_tag.as_deref(), ir, temp_counter);
                        }
                    }
                }
                ExprAst::Slice { ref target, ref start, ref end, ref step } => {
                    let target_s = target.slice(trimmed);
                    let (st_val, has_st) = if let Some(ref s_box) = start {
                        let tv = self.eval_typed(s_box.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                        (self.coerce_to_i64(tv, ir, temp_counter), "1")
                    } else {
                        ("0".to_string(), "0")
                    };
                    let (en_val, has_en) = if let Some(ref e_box) = end {
                        let tv = self.eval_typed(e_box.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                        (self.coerce_to_i64(tv, ir, temp_counter), "1")
                    } else {
                        ("0".to_string(), "0")
                    };
                    let step_val = if let Some(ref stp_box) = step {
                        let tv = self.eval_typed(stp_box.slice(trimmed), ir, scope_tree, string_constants, temp_counter);
                        self.coerce_to_i64(tv, ir, temp_counter)
                    } else {
                        "1".to_string()
                    };

                    if Self::is_string_expr(target_s, scope_tree) {
                        let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                        let sub_res = format!("%str_slice_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i8* @__omni_str_slice_step(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                            sub_res, s_ptr, st_val, en_val, step_val, has_st, has_en
                        ));
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", sub_res));
                        return TypedValue { reg: sub_res, ty: IrType::Ptr };
                    } else {
                        let col_ptr = if let Some(b) = scope_tree.resolve(target_s) {
                            if b.type_hint.starts_with("i8*") {
                                let loaded = format!("%slice_src_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                                loaded
                            } else {
                                let t_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                                self.coerce_to_ptr(t_tv, ir, temp_counter)
                            }
                        } else {
                            let t_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                            self.coerce_to_ptr(t_tv, ir, temp_counter)
                        };
                        let sub_res = format!("%subslice_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i8* @__omni_slice_subslice(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                            sub_res, col_ptr, st_val, en_val, step_val, has_st, has_en
                        ));
                        ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", sub_res));
                        return TypedValue { reg: sub_res, ty: IrType::Ptr };
                    }
                }
                ExprAst::Call { ref callee, ref args } => {
                    if let Some(u_tv) = self.try_eval_universal_call(
                        trimmed,
                        callee,
                        args,
                        ir,
                        scope_tree,
                        string_constants,
                        temp_counter,
                    ) {
                        return u_tv;
                    }
                    if let ExprAst::Member { ref target, ref field } = callee.node {
                        if !matches!(target.node, ExprAst::Ident(_)) {
                            let target_s = target.slice(trimmed);
                            let target_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                            if let IrType::StructPtr(cls_name) = target_tv.ty {
                                let def_cls = scope_tree.resolve_defining_class(&cls_name, field);
                                let raw_inst = format!("%chain_call_inst_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = bitcast %struct.{}* {} to i8*\n",
                                    raw_inst, cls_name, target_tv.reg
                                ));
                                let mut arg_regs = Vec::new();
                                for a in args {
                                    let a_clean = Self::strip_kwarg_prefix(&a.raw);
                                    let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                                    arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                                }
                                while arg_regs.len() < 5 {
                                    arg_regs.push("0".to_string());
                                }
                                let ret_reg = format!("%chain_method_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                    ret_reg, def_cls, field, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4]
                                ));
                                let mret = scope_tree
                                    .method_return_types
                                    .get(&(def_cls.clone(), field.clone()))
                                    .cloned()
                                    .or_else(|| scope_tree.infer_structural_method_return_type(&def_cls, field, args.len()));
                                return Self::apply_return_ir_type(ret_reg, mret.as_ref(), ir, temp_counter);
                            }
                        }
                    }
                }
                ExprAst::Member { ref target, ref field } => {
                    let target_s = target.slice(trimmed);
                    let has_struct_field = scope_tree
                        .resolve(target_s)
                        .filter(|b| b.type_hint.starts_with("%struct."))
                        .and_then(|b| {
                            let c = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            scope_tree.class_fields.get(c).map(|m| m.contains_key(field))
                        })
                        .unwrap_or(false);
                    if !has_struct_field && matches!(field.as_str(), "length" | "Length" | "Count") {
                        if Self::is_string_expr(target_s, scope_tree) {
                            let s_ptr = self.eval_str_ptr(target_s, ir, scope_tree, string_constants, temp_counter);
                            let len_r = format!("%str_prop_len_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i64 @__omni_str_len(i8* {})\n", len_r, s_ptr));
                            return TypedValue { reg: len_r, ty: IrType::I64 };
                        }
                        if let Some(b) = scope_tree.resolve(target_s) {
                            if b.type_hint.starts_with("i8*") {
                                let ld = format!("%prop_col_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let len_r = format!("%prop_len_{}", *temp_counter);
                                *temp_counter += 1;
                                if b.type_hint.contains("omni_chan") {
                                    ir.push_str(&format!("  {} = call i64 @__omni_chan_len(i8* {})\n", len_r, ld));
                                } else if b.type_hint.contains("omni_map") {
                                    ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_r, ld));
                                } else {
                                    ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_r, ld));
                                }
                                return TypedValue { reg: len_r, ty: IrType::I64 };
                            }
                        }
                    }
                    // `@property` method-backed attribute access on class instances: `obj.prop` -> `__omni_method_C_prop(obj)`
                    if !has_struct_field {
                        if let Some(b) = scope_tree.resolve(target_s) {
                            if b.type_hint.starts_with("%struct.") {
                                let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                if scope_tree.class_properties.get(cls).map(|s| s.contains(field)).unwrap_or(false) {
                                    let def_cls = scope_tree
                                        .method_defining_class
                                        .get(&(cls.to_string(), field.clone()))
                                        .map(|s| s.as_str())
                                        .unwrap_or(cls);
                                    let sld = format!("%prop_self_{}", *temp_counter);
                                    let sraw = format!("%prop_sraw_{}", *temp_counter);
                                    let ret64 = format!("%prop_ret_{}", *temp_counter);
                                    *temp_counter += 3;
                                    ir.push_str(&format!(
                                        "  {} = load %struct.{}*, %struct.{}** {}\n  {} = bitcast %struct.{}* {} to i8*\n  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                                        sld, cls, cls, b.llvm_ptr, sraw, cls, sld, ret64, def_cls, field, sraw
                                    ));
                                    let m_ret = scope_tree
                                        .method_return_types
                                        .get(&(cls.to_string(), field.clone()))
                                        .cloned()
                                        .unwrap_or(IrType::I64);
                                    return match m_ret {
                                        IrType::F64 => {
                                            let f = format!("%prop_f64_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = bitcast i64 {} to double\n", f, ret64));
                                            TypedValue { reg: f, ty: IrType::F64 }
                                        }
                                        IrType::Ptr => {
                                            let p = format!("%prop_ptr_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n  call void @__omni_c_return_ptr(i8* {})\n", p, ret64, p));
                                            TypedValue { reg: p, ty: IrType::Ptr }
                                        }
                                        _ => TypedValue { reg: ret64, ty: IrType::I64 },
                                    };
                                }
                            }
                        }
                    }
                    // JS/Lua/Python-style property access on maps: `foo.name` -> `foo["name"]`
                    if !has_struct_field {
                        if let Some(b) = scope_tree.resolve(target_s) {
                            if b.type_hint.contains("omni_map") {
                                let ld = format!("%map_prop_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                let val_res = format!("%map_prop_get_{}", *temp_counter);
                                *temp_counter += 1;
                                if let Some((const_name, len)) = Self::find_string_constant(string_constants, field) {
                                    let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        str_ptr, len, len, const_name
                                    ));
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                                        val_res, ld, str_ptr
                                    ));
                                } else {
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_map_get(i8* {}, i8* null)\n",
                                        val_res, ld
                                    ));
                                }
                                let per_key = scope_tree
                                    .map_key_types
                                    .get(&(target_s.to_string(), field.clone()))
                                    .map(|s| s.as_str())
                                    .or_else(|| scope_tree.collection_elem_types.get(target_s).map(|s| s.as_str()));
                                return Self::unpack_raw_elem_bits(val_res, per_key, ir, temp_counter);
                            }
                        }
                    }
                    if !matches!(target.node, ExprAst::Ident(_)) {
                        let target_tv = self.eval_typed(target_s, ir, scope_tree, string_constants, temp_counter);
                        if let IrType::StructPtr(cls_name) = target_tv.ty {
                            if let Some(&f_idx) = scope_tree.class_fields.get(&cls_name).and_then(|m| m.get(field)) {
                                let gep = format!("%chain_gep_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n",
                                    gep, cls_name, cls_name, target_tv.reg, f_idx
                                ));
                                let val64 = format!("%chain_val64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));
                                let ft = scope_tree.class_field_types.get(&cls_name).and_then(|m| m.get(field)).map(|s| s.as_str());
                                return Self::unpack_raw_elem_bits(val64, ft, ir, temp_counter);
                            }
                        }
                    }
                }
                _ => {}
            }
            // If outer parentheses wrapped a sub-expression (e.g. `(foo(1))`), evaluate the unwrapped inner slice!
            let unwrapped = ast.slice(trimmed);
            if unwrapped != trimmed && !unwrapped.is_empty() {
                return self.eval_typed(unwrapped, ir, scope_tree, string_constants, temp_counter);
            }
        }

        // Go error constructors: `errors.New("...")` / `fmt.Errorf("...")`
        if (trimmed.starts_with("errors.New(") || trimmed.starts_with("fmt.Errorf("))
            && trimmed.ends_with(')')
        {
            let err_ptr = self.eval_str_ptr(trimmed, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", err_ptr));
            return TypedValue { reg: err_ptr, ty: IrType::Ptr };
        }

        // Go / Rust zero-copy string-to-byte-slice views in `eval_typed`
        if trimmed.ends_with(".as_bytes()") {
            let base = trimmed[..trimmed.len() - 11].trim();
            if !scope_tree.has_user_struct_method(base, "as_bytes") {
                let s_ptr = self.eval_str_ptr(base, ir, scope_tree, string_constants, temp_counter);
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", s_ptr));
                return TypedValue { reg: s_ptr, ty: IrType::Ptr };
            }
        }
        if let Some(inner) = trimmed.strip_prefix("[]byte(").and_then(|s| s.strip_suffix(')')) {
            let s_ptr = self.eval_str_ptr(inner.trim(), ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", s_ptr));
            return TypedValue { reg: s_ptr, ty: IrType::Ptr };
        }
        if trimmed.starts_with("*(*[]byte)(unsafe.Pointer(") && trimmed.ends_with(')') {
            let s_ptr = self.eval_str_ptr(trimmed, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", s_ptr));
            return TypedValue { reg: s_ptr, ty: IrType::Ptr };
        }

        // Python byte encode/decode identity unless user struct defines custom `.encode()` / `.decode()`
        if trimmed.ends_with(".decode()") {
            let base = trimmed[..trimmed.len() - 9].trim();
            if !scope_tree.has_user_struct_method(base, "decode") {
                return self.eval_typed(base, ir, scope_tree, string_constants, temp_counter);
            }
        }
        if trimmed.ends_with(".encode()") {
            let base = trimmed[..trimmed.len() - 9].trim();
            if !scope_tree.has_user_struct_method(base, "encode") {
                return self.eval_typed(base, ir, scope_tree, string_constants, temp_counter);
            }
        }

        // Top-level ternary conditional (`cond ? then_expr : else_expr`)
        if let Some((cond_s, tr_s, fa_s)) = split_ternary_expr(trimmed) {
            let cond_v = self.eval_cond(cond_s, ir, scope_tree, string_constants, temp_counter);
            let tr_tv = self.eval_typed(tr_s, ir, scope_tree, string_constants, temp_counter);
            let fa_tv = self.eval_typed(fa_s, ir, scope_tree, string_constants, temp_counter);
            if tr_tv.ty == IrType::F64 || fa_tv.ty == IrType::F64 {
                let tr_f = self.coerce_to_f64(tr_tv, ir, temp_counter);
                let fa_f = self.coerce_to_f64(fa_tv, ir, temp_counter);
                let tern_reg = format!("%tern_f64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = select i1 {}, double {}, double {}\n",
                    tern_reg, cond_v, tr_f, fa_f
                ));
                return TypedValue { reg: tern_reg, ty: IrType::F64 };
            }
            if tr_tv.ty == IrType::Ptr && fa_tv.ty == IrType::Ptr {
                let tern_reg = format!("%tern_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = select i1 {}, i8* {}, i8* {}\n",
                    tern_reg, cond_v, tr_tv.reg, fa_tv.reg
                ));
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", tern_reg));
                return TypedValue { reg: tern_reg, ty: IrType::Ptr };
            }
            let tr_i = self.coerce_to_i64(tr_tv, ir, temp_counter);
            let fa_i = self.coerce_to_i64(fa_tv, ir, temp_counter);
            let tern_reg = format!("%tern_i64_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = select i1 {}, i64 {}, i64 {}\n",
                tern_reg, cond_v, tr_i, fa_i
            ));
            return TypedValue { reg: tern_reg, ty: IrType::I64 };
        }

        // Universal formatting & introspection builtins: `repr(x)`, `isinstance(x, T)`, `hasattr(o, k)`, `getattr(o, k[, default])`
        if trimmed.starts_with("repr(") && trimmed.ends_with(')') && !scope_tree.known_functions.contains("repr") {
            let inner = trimmed[5..trimmed.len() - 1].trim();
            if let Some(b) = scope_tree.resolve(inner) {
                if b.type_hint.contains("omni_map") {
                    let ld = format!("%repr_m_{}", *temp_counter);
                    let js = format!("%repr_js_{}", *temp_counter);
                    *temp_counter += 2;
                    ir.push_str(&format!(
                        "  {} = load i8*, i8** {}\n  {} = call i8* @__omni_map_to_json(i8* {})\n  call void @__omni_c_return_ptr(i8* {})\n",
                        ld, b.llvm_ptr, js, ld, js
                    ));
                    return TypedValue { reg: js, ty: IrType::Ptr };
                }
            }
            let sp = self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", sp));
            return TypedValue { reg: sp, ty: IrType::Ptr };
        }
        if trimmed.starts_with("isinstance(") && trimmed.ends_with(')') && !scope_tree.known_functions.contains("isinstance") {
            let inner = trimmed[11..trimmed.len() - 1].trim();
            let parts = OmniParser::split_respecting_quotes(inner, ',');
            if parts.len() == 2 {
                let obj_s = parts[0].trim();
                let ty_s = parts[1].trim().trim_matches(|c| c == '(' || c == ')');
                let obj_hint = scope_tree.resolve(obj_s).map(|b| b.type_hint.clone()).unwrap_or_default();
                let obj_is_str = Self::is_string_expr(obj_s, scope_tree) || is_string_type_hint(&obj_hint);
                let obj_is_map = obj_hint.contains("omni_map");
                let obj_is_slice = obj_hint.contains("omni_slice");
                let obj_is_float = is_float_type_hint(&obj_hint) || parse_polyglot_f64(obj_s).is_some();
                let obj_cls = obj_hint.strip_prefix("%struct.").map(|s| s.trim_end_matches('*'));
                let matched = ty_s.split(',').any(|t| {
                    let tc = t.trim();
                    match tc {
                        "dict" | "map" | "Map" => obj_is_map,
                        "list" | "tuple" | "slice" | "Array" => obj_is_slice,
                        "str" | "string" | "String" | "bytes" => obj_is_str && !obj_is_map && !obj_is_slice,
                        "float" | "double" | "f64" | "f32" => obj_is_float,
                        "int" | "i64" | "i32" | "bool" => !obj_is_str && !obj_is_map && !obj_is_slice && !obj_is_float && obj_cls.is_none(),
                        other => obj_cls == Some(other),
                    }
                });
                return TypedValue { reg: if matched { "1" } else { "0" }.to_string(), ty: IrType::I64 };
            }
        }
        if trimmed.starts_with("hasattr(") && trimmed.ends_with(')') && !scope_tree.known_functions.contains("hasattr") {
            let inner = trimmed[8..trimmed.len() - 1].trim();
            let parts = OmniParser::split_respecting_quotes(inner, ',');
            if parts.len() == 2 {
                let obj_s = parts[0].trim();
                let attr_s = parts[1].trim();
                if let Some(b) = scope_tree.resolve(obj_s) {
                    if b.type_hint.contains("omni_map") {
                        let ld = format!("%ha_ld_{}", *temp_counter);
                        let r1 = format!("%ha_r1_{}", *temp_counter);
                        let r64 = format!("%ha_r64_{}", *temp_counter);
                        *temp_counter += 3;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                        let kp = self.eval_str_ptr(attr_s, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!("  {} = call i1 @__omni_map_contains(i8* {}, i8* {})\n  {} = zext i1 {} to i64\n", r1, ld, kp, r64, r1));
                        return TypedValue { reg: r64, ty: IrType::I64 };
                    }
                    if b.type_hint.starts_with("%struct.") {
                        let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        let aname = attr_s.trim_matches('"').trim_matches('\'');
                        let exists = scope_tree.class_fields.get(cls).map(|m| m.contains_key(aname)).unwrap_or(false)
                            || scope_tree.class_properties.get(cls).map(|s| s.contains(aname)).unwrap_or(false)
                            || scope_tree.method_defining_class.contains_key(&(cls.to_string(), aname.to_string()));
                        return TypedValue { reg: if exists { "1" } else { "0" }.to_string(), ty: IrType::I64 };
                    }
                }
            }
        }
        if trimmed.starts_with("getattr(") && trimmed.ends_with(')') && !scope_tree.known_functions.contains("getattr") {
            let inner = trimmed[8..trimmed.len() - 1].trim();
            let parts = OmniParser::split_respecting_quotes(inner, ',');
            if parts.len() == 2 || parts.len() == 3 {
                let obj_s = parts[0].trim();
                let attr_s = parts[1].trim();
                if let Some(b) = scope_tree.resolve(obj_s) {
                    if b.type_hint.contains("omni_map") {
                        let ld = format!("%ga_ld_{}", *temp_counter);
                        let r = format!("%ga_r_{}", *temp_counter);
                        *temp_counter += 2;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                        let kp = self.eval_str_ptr(attr_s, ir, scope_tree, string_constants, temp_counter);
                        let (d64, dtag) = if parts.len() == 3 {
                            let ds = parts[2].trim();
                            let dtv = self.eval_typed(ds, ir, scope_tree, string_constants, temp_counter);
                            let tag = Self::infer_elem_tag_from_expr(ds, &dtv.ty, scope_tree);
                            (self.coerce_to_raw_i64_bits(dtv, ir, temp_counter), Some(tag))
                        } else {
                            ("0".to_string(), None)
                        };
                        ir.push_str(&format!("  {} = call i64 @__omni_map_get_or(i8* {}, i8* {}, i64 {})\n", r, ld, kp, d64));
                        return Self::unpack_raw_elem_bits(r, dtag.as_deref(), ir, temp_counter);
                    }
                    if b.type_hint.starts_with("%struct.") {
                        let aname = attr_s.trim_matches('"').trim_matches('\'');
                        let cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        let has_f = scope_tree.class_fields.get(cls).map(|m| m.contains_key(aname)).unwrap_or(false)
                            || scope_tree.class_properties.get(cls).map(|s| s.contains(aname)).unwrap_or(false);
                        if has_f {
                            return self.eval_typed(&format!("{}.{}", obj_s, aname), ir, scope_tree, string_constants, temp_counter);
                        } else if parts.len() == 3 {
                            return self.eval_typed(parts[2].trim(), ir, scope_tree, string_constants, temp_counter);
                        }
                    }
                }
            }
        }

        // Polyglot string casts: Python `str(x)`, JS `String(x)`, Lua `tostring(x)`
        for prefix in STRING_CAST_PREFIXES {
            if trimmed.starts_with(prefix) && trimmed.ends_with(')') {
                let fn_ident = prefix.trim_end_matches('(');
                if !scope_tree.known_functions.contains(fn_ident) {
                    let inner = trimmed[prefix.len()..trimmed.len() - 1].trim();
                    let str_ptr = self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", str_ptr));
                    return TypedValue { reg: str_ptr, ty: IrType::Ptr };
                }
            }
        }

        // Polyglot float casts: Python `float(x)`, Go `float64(x)`, Rust `f64::from(x)`, JS `Number(x)`
        for flt_prefix in FLOAT_CAST_PREFIXES {
            if trimmed.starts_with(flt_prefix) && trimmed.ends_with(')') {
                let fn_ident = flt_prefix.trim_end_matches('(');
                if !scope_tree.known_functions.contains(fn_ident) {
                    let inner = trimmed[flt_prefix.len()..trimmed.len() - 1].trim();
                    if Self::is_string_expr(inner, scope_tree) {
                        let sptr = self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
                        let freg = format!("%str_to_f64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call double @__omni_c_str_to_f64(i8* {})\n", freg, sptr));
                        return TypedValue { reg: freg, ty: IrType::F64 };
                    }
                    let inner_tv = self.eval_typed(inner, ir, scope_tree, string_constants, temp_counter);
                    let freg = self.coerce_to_f64(inner_tv, ir, temp_counter);
                    return TypedValue { reg: freg, ty: IrType::F64 };
                }
            }
        }

        // Polyglot integer conversions: Rust `i64::from(x)` / Go `int64(x)` / Python `int(x)`
        for num_prefix in INT_CAST_PREFIXES {
            if trimmed.starts_with(num_prefix) && trimmed.ends_with(')') {
                let fn_ident = num_prefix.trim_end_matches('(');
                if !scope_tree.known_functions.contains(fn_ident) {
                    let inner = trimmed[num_prefix.len()..trimmed.len() - 1].trim();
                    if Self::is_string_expr(inner, scope_tree) {
                        let sptr = self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter);
                        let ireg = format!("%str_to_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i64 @__omni_c_str_to_i64(i8* {})\n", ireg, sptr));
                        return TypedValue { reg: ireg, ty: IrType::I64 };
                    }
                    let inner_tv = self.eval_typed(inner, ir, scope_tree, string_constants, temp_counter);
                    let ireg = self.coerce_to_i64(inner_tv, ir, temp_counter);
                    return TypedValue { reg: ireg, ty: IrType::I64 };
                }
            }
        }

        // Builtin input(prompt)
        if trimmed == "input()" || (trimmed.starts_with("input(") && trimmed.ends_with(')')) {
            let prompt_ptr = if trimmed == "input()" {
                "null".to_string()
            } else {
                let inner = trimmed[6..trimmed.len() - 1].trim();
                if inner.is_empty() {
                    "null".to_string()
                } else {
                    self.eval_str_ptr(inner, ir, scope_tree, string_constants, temp_counter)
                }
            };
            let input_ptr = format!("%input_val_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = call i8* @__omni_input(i8* {})\n", input_ptr, prompt_ptr));
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", input_ptr));
            return TypedValue { reg: input_ptr, ty: IrType::Ptr };
        }

        // Builtin Python `bytes(...)` / Go `[]byte(...)` constructor
        if trimmed.starts_with("bytes(")
            && Self::find_outer_call_open_paren(trimmed) == Some(5)
            && !scope_tree.known_functions.contains("bytes")
        {
            let bp = self.eval_str_ptr(trimmed, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", bp));
            return TypedValue { reg: bp, ty: IrType::Ptr };
        }

        // Builtin exit(code)
        if trimmed == "exit()" || (trimmed.starts_with("exit(") && trimmed.ends_with(')')) {
            let code_val = if trimmed == "exit()" {
                "0".to_string()
            } else {
                let inner = trimmed[5..trimmed.len() - 1].trim();
                if inner.is_empty() {
                    "0".to_string()
                } else {
                    self.eval_val(inner, ir, scope_tree, string_constants, temp_counter)
                }
            };
            ir.push_str(&format!("  call void @exit(i32 {})\n", code_val));
            return TypedValue { reg: "0".to_string(), ty: IrType::I64 };
        }

        // Canonical POSIX socket constants from top-of-file lookup table
        if let Some((_, val)) = POSIX_SOCKET_CONSTANTS.iter().find(|(k, _)| *k == trimmed) {
            return TypedValue { reg: (*val).to_string(), ty: IrType::I64 };
        }

        // Builtin OS Pipe creation: `mp.Pipe(...)` or `Pipe(...)`
        if trimmed.starts_with("mp.Pipe(") || trimmed.starts_with("Pipe(") {
            let slice_ptr = format!("%pipe_tuple_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = call i8* @__omni_os_pipe_pair()\n", slice_ptr));
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", slice_ptr));
            return TypedValue { reg: slice_ptr, ty: IrType::Ptr };
        }

        // Concurrency Fiber Spawning via Thread/Process abstractions
        let unwrap_start = if trimmed.ends_with(".start()") {
            trimmed[..trimmed.len() - 8].trim()
        } else {
            trimmed
        };
        if (unwrap_start.starts_with("threading.Thread(") || unwrap_start.starts_with("Thread(")
            || unwrap_start.starts_with("mp.Process(") || unwrap_start.starts_with("Process("))
            && unwrap_start.ends_with(')')
        {
            let mut target_fn = String::new();
            let mut arg_val = String::new();
            if let Some(open) = unwrap_start.find('(') {
                let inside = &unwrap_start[open + 1..unwrap_start.len() - 1];
                let parts = omni_parser::OmniParser::split_respecting_quotes(inside, ',');
                for p in parts {
                    let p_trim = p.trim();
                    if let Some((k, v)) = p_trim.split_once('=') {
                        let key = k.trim();
                        let val = v.trim();
                        if key == "target" {
                            target_fn = val.to_string();
                        } else if key == "args" {
                            let clean_args = val.trim_start_matches('(').trim_end_matches(')').trim().trim_end_matches(',').trim();
                            arg_val = clean_args.to_string();
                        }
                    } else if target_fn.is_empty() {
                        target_fn = p_trim.to_string();
                    }
                }
            }
            let arg_reg = if !arg_val.is_empty() {
                if let Some(binding) = scope_tree.resolve(&arg_val) {
                    if binding.type_hint == "i64" {
                        let loaded = format!("%arg_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i64, i64* {}\n", loaded, binding.llvm_ptr));
                        let ptr_cast = format!("%arg_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, loaded));
                        ptr_cast
                    } else if binding.type_hint == "i32" {
                        let loaded = format!("%arg_i32_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i32, i32* {}\n", loaded, binding.llvm_ptr));
                        let ext = format!("%arg_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, loaded));
                        let ptr_cast = format!("%arg_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, ext));
                        ptr_cast
                    } else if binding.type_hint.starts_with("%struct.") {
                        let cls_name = binding.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        let loaded = format!("%arg_s_ld_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", loaded, cls_name, cls_name, binding.llvm_ptr));
                        let bc = format!("%arg_s_bc_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", bc, cls_name, loaded));
                        bc
                    } else {
                        let loaded = format!("%arg_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                        loaded
                    }
                } else if let Some(val) = parse_polyglot_i64(&arg_val) {
                    let ptr_cast = format!("%arg_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, val));
                    ptr_cast
                } else {
                    "null".to_string()
                }
            } else {
                let u_ptr = format!("%thr_unset_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = inttoptr i64 -581005069656919554 to i8*\n", u_ptr));
                u_ptr
            };
            if !target_fn.is_empty() {
                let spawn_fn = if unwrap_start.starts_with("threading.Thread(") || unwrap_start.starts_with("Thread(") {
                    "@__omni_thread_spawn"
                } else {
                    "@__omni_go_spawn"
                };
                ir.push_str(&format!(
                    "  call void {}(void (i8*)* @__omni_trampoline_{}, i8* {})\n",
                    spawn_fn, target_fn, arg_reg
                ));
            }
            return TypedValue { reg: "0".to_string(), ty: IrType::I64 };
        }

        // Random generator calls: `new Random().nextInt()` or `Random.nextInt()`
        if trimmed.contains("nextInt") || trimmed.contains("random()") || trimmed.contains(".random()") || trimmed.contains("urandom(") {
            let r = format!("%rnd_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = call i64 @__omni_random_int()\n", r));
            return TypedValue { reg: r, ty: IrType::I64 };
        }

        // Struct field access: rec.fld
        if trimmed.contains('.') && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() == 2 {
                let rec = parts[0].trim();
                let fld = parts[1].trim();
                if let Some(b) = scope_tree.resolve(rec) {
                    if b.type_hint.starts_with("%struct.") {
                        let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fld)) {
                            let inst_ld = format!("%inst_fld_eval_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                            let gep = format!("%eval_gep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                            let val64 = format!("%val64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));

                            if let Some(ft) = scope_tree.class_field_types.get(cls_name).and_then(|m| m.get(fld)) {
                                if is_float_type_hint(ft) {
                                    let fval = format!("%fld_f64_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fval, val64));
                                    return TypedValue { reg: fval, ty: IrType::F64 };
                                } else if ft.starts_with("i8*")
                                    || ft.starts_with("omni_slice")
                                    || ft.starts_with("omni_map")
                                    || is_string_type_hint(ft)
                                {
                                    let sval = format!("%fld_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", sval, val64));
                                    return TypedValue { reg: sval, ty: IrType::Ptr };
                                } else if ft.starts_with("%struct.") {
                                    let target_c = ft.trim_start_matches("%struct.").trim_end_matches('*');
                                    let sp = format!("%fld_sptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", sp, val64, target_c));
                                    return TypedValue { reg: sp, ty: IrType::StructPtr(target_c.to_string()) };
                                }
                            }
                            return TypedValue { reg: val64, ty: IrType::I64 };
                        }
                    }
                }
            }
        }

        // Inline Struct Literal (`Point { x: 10, y: 20 }`) & Tagged Enum Variant (`Result::Ok(100)`) in expressions
        if let Some(open) = trimmed.find('{') {
            if trimmed.ends_with('}') {
                let raw_sc = trimmed[..open].trim().strip_prefix("new ").map(|s| s.trim()).unwrap_or_else(|| trimmed[..open].trim());
                let struct_cand = raw_sc.split('<').next().unwrap_or(raw_sc).trim();
                if !struct_cand.is_empty() && scope_tree.class_fields.contains_key(struct_cand) {
                    let f_count = scope_tree.class_fields.get(struct_cand).map(|m| m.len()).unwrap_or(1);
                    let alloc_bytes = (f_count * 8).max(8);
                    let tid = scope_tree.class_type_ids.get(struct_cand).copied().unwrap_or(0);
                    let raw_inst = format!("%expr_slit_raw_{}", *temp_counter);
                    let inst_alloc = format!("%expr_slit_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_obj_alloc(i64 {}, i64 {})\n  {} = bitcast i8* {} to %struct.{}*\n",
                        raw_inst, alloc_bytes, tid, inst_alloc, raw_inst, struct_cand
                    ));
                    let inner = &trimmed[open + 1..trimmed.len() - 1];
                    for field_part in omni_parser::OmniParser::split_respecting_quotes(inner, ',') {
                        let fp_trim = field_part.trim();
                        if let Some((f_name, f_val)) = fp_trim.split_once(':') {
                            let fn_clean = f_name.trim().trim_start_matches('.');
                            if let Some(&f_idx) = scope_tree.class_fields.get(struct_cand).and_then(|m| m.get(fn_clean)) {
                                let ftv = self.eval_typed(f_val.trim(), ir, scope_tree, string_constants, temp_counter);
                                let bits = self.coerce_to_raw_i64_bits(ftv, ir, temp_counter);
                                let gep = format!("%expr_slit_gep_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n  store i64 {}, i64* {}\n",
                                    gep, struct_cand, struct_cand, inst_alloc, f_idx, bits, gep
                                ));
                            }
                        }
                    }
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_inst));
                    return TypedValue {
                        reg: inst_alloc,
                        ty: IrType::StructPtr(struct_cand.to_string()),
                    };
                }
            }
        }
        if let Some((e_name, (tag, payload_val))) = scope_tree.resolve_enum_variant(trimmed) {
            let raw_enum = format!("%expr_enum_raw_{}", *temp_counter);
            let inst_alloc = format!("%expr_enum_ptr_{}", *temp_counter);
            let tag_gep = format!("%expr_etag_{}", *temp_counter);
            let pay_gep = format!("%expr_epay_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = call i8* @__omni_obj_alloc(i64 16, i64 0)\n  {} = bitcast i8* {} to %enum.{}*\n  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 0\n  store i32 {}, i32* {}\n  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 1\n",
                raw_enum, inst_alloc, raw_enum, e_name, tag_gep, e_name, e_name, inst_alloc, tag, tag_gep, pay_gep, e_name, e_name, inst_alloc
            ));
            if let Some(p_expr) = payload_val {
                let ptv = self.eval_typed(&p_expr, ir, scope_tree, string_constants, temp_counter);
                let pbits = self.coerce_to_raw_i64_bits(ptv, ir, temp_counter);
                ir.push_str(&format!("  store i64 {}, i64* {}\n", pbits, pay_gep));
            } else {
                ir.push_str(&format!("  store i64 0, i64* {}\n", pay_gep));
            }
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_enum));
            return TypedValue { reg: raw_enum, ty: IrType::Ptr };
        }

        // Method invocation: `obj.method(args...)`, direct call `func(args...)`, or curried/IIFE `make_adder(20)(7)`
        if let Some(lp) = Self::find_outer_call_open_paren(trimmed) {
            if trimmed.ends_with(')') {
                let raw_call_target = trimmed[..lp].trim();
                let stripped_new = raw_call_target
                    .strip_prefix("new ")
                    .map(|s| s.trim())
                    .unwrap_or(raw_call_target);
                let call_target = if !stripped_new.contains('(') && stripped_new.contains('<') && stripped_new.ends_with('>') {
                    stripped_new.split('<').next().unwrap_or(stripped_new).trim()
                } else {
                    stripped_new
                };
                let is_ident_or_dot = !call_target.is_empty()
                    && call_target.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
                let inner_paren_ct = if call_target.starts_with('(') && call_target.ends_with(')') {
                    Some(OmniParser::unwrap_outer_balanced_parens(call_target))
                } else {
                    None
                };
                let is_curried_or_iife = !call_target.is_empty()
                    && !call_target.contains('.')
                    && ((!call_target.starts_with('(') && call_target.ends_with(')'))
                        || inner_paren_ct.as_deref().map_or(false, |ict| {
                            scope_tree.known_functions.contains(ict)
                                || scope_tree.resolve(ict).is_some()
                                || OmniParser::try_parse_lambda_expr(ict).is_some()
                        })
                        || (call_target.starts_with('(')
                            && (call_target.contains("lambda")
                                || call_target.contains("=>")
                                || call_target.contains('|')
                                || call_target.contains("func")
                                || call_target.contains("->"))));
                let is_valid_call_target = is_ident_or_dot || is_curried_or_iife;

                if is_valid_call_target {
                    let args_str = &trimmed[lp + 1..trimmed.len() - 1];
                    let raw_args: Vec<String> = if args_str.is_empty() {
                        Vec::new()
                    } else {
                        omni_parser::OmniParser::split_respecting_quotes(args_str, ',')
                    };

                    if call_target.contains('.') {
                        let parts: Vec<&str> = call_target.split('.').collect();
                        if parts.len() >= 2 {
                            let rec = parts[parts.len() - 2].trim();
                            let method = parts[parts.len() - 1].trim();

                            // Socket operations
                            if rec == "socket" && method == "socket" {
                                ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                                let sock_fd = format!("%sock_fd_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_create(i32 2, i32 1, i32 0)\n", sock_fd));
                                return TypedValue { reg: sock_fd, ty: IrType::I32 };
                            }

                            let is_user_method = scope_tree.has_user_struct_method(rec, method);

                            if !is_user_method && method == "send" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let arg_data = if let Some(a) = raw_args.first() {
                                    let a_trim = a.trim();
                                    if Self::is_string_expr(a_trim, scope_tree) || a_trim.starts_with('"') || a_trim.starts_with('\'') {
                                        self.eval_str_ptr(a_trim, ir, scope_tree, string_constants, temp_counter)
                                    } else {
                                        let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                        self.coerce_to_ptr(tv, ir, temp_counter)
                                    }
                                } else {
                                    "null".to_string()
                                };
                                let ret_reg = format!("%send_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_send(i32 {}, i8* {})\n", ret_reg, r_fd, arg_data));
                                return TypedValue { reg: ret_reg, ty: IrType::I32 };
                            }

                            if !is_user_method && method == "recv" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let max_bytes = if let Some(a) = raw_args.first() {
                                    self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter)
                                } else {
                                    "1024".to_string()
                                };
                                let ret_ptr = format!("%recv_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_os_socket_recv(i32 {}, i32 {})\n", ret_ptr, r_fd, max_bytes));
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", ret_ptr));
                                return TypedValue { reg: ret_ptr, ty: IrType::Ptr };
                            }

                            if !is_user_method && method == "bind" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let addr_ptr = if let Some(a) = raw_args.first() {
                                    let a_trim = a.trim();
                                    if let Some(b) = scope_tree.resolve(a_trim) {
                                        if b.type_hint.contains("omni_slice") || b.type_hint.starts_with("i8*") {
                                            let ld = format!("%bind_addr_ld_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                            ld
                                        } else {
                                            let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                            self.coerce_to_ptr(tv, ir, temp_counter)
                                        }
                                    } else {
                                        let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                        self.coerce_to_ptr(tv, ir, temp_counter)
                                    }
                                } else {
                                    "null".to_string()
                                };
                                let ret_reg = format!("%bind_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_bind(i32 {}, i8* {}, i32 0)\n", ret_reg, r_fd, addr_ptr));
                                return TypedValue { reg: ret_reg, ty: IrType::I32 };
                            }

                            if !is_user_method && method == "listen" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let ret_reg = format!("%listen_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_listen_fd(i32 {}, i32 128)\n", ret_reg, r_fd));
                                return TypedValue { reg: ret_reg, ty: IrType::I32 };
                            }

                            if !is_user_method && method == "accept" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let slice_ptr = format!("%accept_tuple_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_os_socket_accept_tuple(i32 {})\n", slice_ptr, r_fd));
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", slice_ptr));
                                return TypedValue { reg: slice_ptr, ty: IrType::Ptr };
                            }

                            if !is_user_method && method == "connect" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let addr_ptr = if let Some(a) = raw_args.first() {
                                    let a_trim = a.trim();
                                    if let Some(b) = scope_tree.resolve(a_trim) {
                                        if b.type_hint.contains("omni_slice") || b.type_hint.starts_with("i8*") {
                                            let ld = format!("%conn_addr_ld_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                            ld
                                        } else {
                                            let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                            self.coerce_to_ptr(tv, ir, temp_counter)
                                        }
                                    } else {
                                        let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                        self.coerce_to_ptr(tv, ir, temp_counter)
                                    }
                                } else {
                                    "null".to_string()
                                };
                                let ret_reg = format!("%conn_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_connect(i32 {}, i8* {}, i32 0)\n", ret_reg, r_fd, addr_ptr));
                                return TypedValue { reg: ret_reg, ty: IrType::I32 };
                            }

                            if !is_user_method && method == "getpeername" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                let peer_ptr = format!("%peer_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_os_socket_getpeername(i32 {})\n", peer_ptr, r_fd));
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", peer_ptr));
                                return TypedValue { reg: peer_ptr, ty: IrType::Ptr };
                            }

                            if !is_user_method && method == "close" {
                                let r_fd = self.eval_val(rec, ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!("  call void @__omni_os_socket_close(i32 {})\n", r_fd));
                                return TypedValue { reg: "0".to_string(), ty: IrType::I64 };
                            }

                            if !is_user_method && (method == "terminate" || method == "start") {
                                return TypedValue { reg: "0".to_string(), ty: IrType::I64 };
                            }

                            if let Some(b) = scope_tree.resolve(rec) {
                                if b.type_hint.starts_with("%struct.") {
                                    let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                    let def_cls = scope_tree.resolve_defining_class(cls_name, method);

                                    let inst_ld = format!("%inst_call_ld_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                                    let raw_inst = format!("%raw_call_inst_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_ld));

                                    return self.emit_method_dispatch(
                                        Some(cls_name),
                                        &def_cls,
                                        method,
                                        &raw_inst,
                                        &raw_args,
                                        true,
                                        ir,
                                        scope_tree,
                                        string_constants,
                                        temp_counter,
                                    );
                                } else if b.type_hint.starts_with("i8*") || b.type_hint == "i64" {
                                    let mut matching_defs: Vec<&String> = scope_tree
                                        .method_defining_class
                                        .iter()
                                        .filter_map(|((_, m), dc)| if m == method { Some(dc) } else { None })
                                        .collect();
                                    matching_defs.sort();
                                    if let Some(first_def) = matching_defs.first() {
                                        let def_cls = (*first_def).clone();
                                        let raw_inst = if b.type_hint.starts_with("i8*") {
                                            let ld = format!("%raw_i8_inst_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                                            ld
                                        } else {
                                            let ld64 = format!("%raw_i64_inst_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, b.llvm_ptr));
                                            let i2p = format!("%raw_i64_i8_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", i2p, ld64));
                                            i2p
                                        };
                                        return self.emit_method_dispatch(
                                            None,
                                            &def_cls,
                                            method,
                                            &raw_inst,
                                            &raw_args,
                                            true,
                                            ir,
                                            scope_tree,
                                            string_constants,
                                            temp_counter,
                                        );
                                    }
                                }
                            } else if scope_tree.class_fields.contains_key(rec) {
                                // Static / Base class method invocation: ClassName.method(instance, args...)
                                let def_cls = scope_tree.resolve_defining_class(rec, method);
                                let (raw_inst, remaining_args) = if !raw_args.is_empty() {
                                    let inst_name = raw_args[0].trim();
                                    if let Some(b) = scope_tree.resolve(inst_name) {
                                        if b.type_hint.starts_with("%struct.") {
                                            let inst_cls = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                            let inst_ld = format!("%inst_call_ld_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, inst_cls, inst_cls, b.llvm_ptr));
                                            let raw = format!("%raw_base_inst_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw, inst_cls, inst_ld));
                                            (raw, &raw_args[1..])
                                        } else {
                                            ("null".to_string(), &raw_args[..])
                                        }
                                    } else {
                                        ("null".to_string(), &raw_args[..])
                                    }
                                } else {
                                    ("null".to_string(), &raw_args[..])
                                };

                                return self.emit_method_dispatch(
                                    Some(rec),
                                    &def_cls,
                                    method,
                                    &raw_inst,
                                    remaining_args,
                                    false,
                                    ir,
                                    scope_tree,
                                    string_constants,
                                    temp_counter,
                                );
                            } else {
                                // Foreign module call: `fast_math.compute_luhn(card_num)` -> `@__omni_fn_compute_luhn`
                                if method == "randint" && raw_args.len() >= 2 {
                                    let lo_tv = self.eval_typed(raw_args[0].trim(), ir, scope_tree, string_constants, temp_counter);
                                    let lo_64 = self.coerce_to_i64(lo_tv, ir, temp_counter);
                                    let hi_tv = self.eval_typed(raw_args[1].trim(), ir, scope_tree, string_constants, temp_counter);
                                    let hi_64 = self.coerce_to_i64(hi_tv, ir, temp_counter);
                                    let hi_excl = format!("%randint_hi_{}", *temp_counter);
                                    let rnd_res = format!("%randint_res_{}", *temp_counter);
                                    *temp_counter += 2;
                                    ir.push_str(&format!(
                                        "  {} = add i64 {}, 1\n  {} = call i64 @__omni_random_range(i64 {}, i64 {})\n",
                                        hi_excl, hi_64, rnd_res, lo_64, hi_excl
                                    ));
                                    return TypedValue { reg: rnd_res, ty: IrType::I64 };
                                }

                                let prefixed_fn = format!("{}_{}", rec, method);
                                if scope_tree.known_functions.contains(method) {
                                    let abi_n = scope_tree.fn_abi_arity(method);
                                    let mut arg_regs = self.eval_call_args_with_defaults(
                                        &raw_args,
                                        scope_tree.fn_param_defs.get(method),
                                        abi_n,
                                        ir,
                                        scope_tree,
                                        string_constants,
                                        temp_counter,
                                    );
                                    arg_regs.truncate(abi_n);
                                    let fmt_args = arg_regs.iter().map(|r| format!("i64 {}", r)).collect::<Vec<_>>().join(", ");
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_fn_{}({})\n",
                                        ret_reg, method, fmt_args
                                    ));
                                    let fret = scope_tree.fn_return_types.get(method);
                                    return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                                } else if scope_tree.known_functions.contains(&prefixed_fn) {
                                    let abi_n = scope_tree.fn_abi_arity(&prefixed_fn);
                                    let mut arg_regs = self.eval_call_args_with_defaults(
                                        &raw_args,
                                        scope_tree.fn_param_defs.get(&prefixed_fn),
                                        abi_n,
                                        ir,
                                        scope_tree,
                                        string_constants,
                                        temp_counter,
                                    );
                                    arg_regs.truncate(abi_n);
                                    let fmt_args = arg_regs.iter().map(|r| format!("i64 {}", r)).collect::<Vec<_>>().join(", ");
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_fn_{}({})\n",
                                        ret_reg, prefixed_fn, fmt_args
                                    ));
                                    let fret = scope_tree.fn_return_types.get(&prefixed_fn);
                                    return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                                }

                                let mut arg_regs = Vec::new();
                                for a in &raw_args {
                                    let a_clean = Self::strip_kwarg_prefix(a);
                                    let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                                    arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                                }
                                let actual_c_args: String = arg_regs
                                    .iter()
                                    .map(|r| format!("i64 {}", r))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                while arg_regs.len() < 6 {
                                    arg_regs.push("0".to_string());
                                }

                                if let Some((_, ref ret_ty)) = scope_tree.extern_fns.get(method).cloned() {
                                    if scope_tree.c_module_aliases.contains(rec) {
                                        return self.emit_extern_fn_call(
                                            method,
                                            ret_ty.as_ref(),
                                            &actual_c_args,
                                            ir,
                                            temp_counter,
                                        );
                                    }
                                }
                                if NATIVE_C_LIB_I64_FNS.contains(&method) {
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @{}({})\n",
                                        ret_reg, method, actual_c_args
                                    ));
                                    return TypedValue { reg: ret_reg, ty: IrType::I64 };
                                } else if NATIVE_C_LIB_FNS.contains(&method) {
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i32 @{}({})\n",
                                        ret_reg, method, actual_c_args
                                    ));
                                    return TypedValue { reg: ret_reg, ty: IrType::I32 };
                                } else if scope_tree.c_module_aliases.contains(rec) {
                                    if let Some(ctx) = self.active_ai_ctx.borrow_mut().as_mut() {
                                        ctx.dynamic_declarations
                                            .insert(format!("declare i64 @{}(...)", method));
                                    }
                                    let ret_reg = format!("%c_ffi_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @{}({})\n",
                                        ret_reg, method, actual_c_args
                                    ));
                                    return TypedValue { reg: ret_reg, ty: IrType::I64 };
                                } else {
                                    let is_c_eco = scope_tree.c_module_aliases.contains(rec)
                                        || self
                                            .active_ai_ctx
                                            .borrow()
                                            .as_ref()
                                            .and_then(|c| c.ecosystem_context.as_deref())
                                            .map(|ec| ec.contains("c (module:") && ec.contains(rec))
                                            .unwrap_or(false);
                                    if !is_c_eco
                                        && !scope_tree.is_natively_routed_call(rec, method, &parts, trimmed)
                                        && scope_tree.resolve(rec).is_none()
                                        && !scope_tree.class_fields.contains_key(rec)
                                    {
                                        if let Some(ai_tv) = self.escalate_subexpr_to_llm(
                                            trimmed,
                                            "ForeignModuleAssignment",
                                            ir,
                                            scope_tree,
                                            string_constants,
                                            temp_counter,
                                        ) {
                                            return ai_tv;
                                        }
                                    }
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    let ext_sym = if scope_tree.resolve(rec).is_none()
                                        && !scope_tree.class_fields.contains_key(rec)
                                    {
                                        let clean_rec: String = rec
                                            .chars()
                                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                                            .collect();
                                        let clean_mth: String = method
                                            .chars()
                                            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                                            .collect();
                                        format!("__omni_extmod__{}__{}", clean_rec, clean_mth)
                                    } else {
                                        method.to_string()
                                    };
                                    ir.push_str(&format!(
                                        "  {} = call i64 @{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                        ret_reg, ext_sym, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                                    ));
                                    return TypedValue { reg: ret_reg, ty: IrType::I64 };
                                }
                            }
                        }
                    } else if !call_target.is_empty()
                        && call_target != "len"
                        && call_target != "make"
                        && call_target != "range"
                        && call_target != "print"
                        && call_target != "println!"
                    {
                        let effective_cls = if scope_tree.class_fields.contains_key(call_target) {
                            Some(call_target.to_string())
                        } else if call_target == "cls" {
                            scope_tree.resolve("cls").and_then(|b| {
                                if b.type_hint.starts_with("%struct.") {
                                    Some(b.type_hint.trim_start_matches("%struct.").trim_end_matches('*').to_string())
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        };

                        if let Some(target_cls) = effective_cls {
                            // Dynamic heap constructor instantiation: `Class(...)` or `new Class(...)`
                            let field_count = scope_tree.class_fields.get(&target_cls).map(|m| m.len()).unwrap_or(1);
                            let alloc_bytes = (field_count * 8).max(8);
                            let tid = scope_tree.class_type_ids.get(&target_cls).copied().unwrap_or(0);
                            let raw_inst = format!("%ctor_raw_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i8* @__omni_obj_alloc(i64 {}, i64 {})\n",
                                raw_inst, alloc_bytes, tid
                            ));

                            let struct_ptr = format!("%ctor_struct_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = bitcast i8* {} to %struct.{}*\n",
                                struct_ptr, raw_inst, target_cls
                            ));

                            self.init_struct_instance(
                                &target_cls,
                                &raw_inst,
                                &struct_ptr,
                                "__init__",
                                &raw_args,
                                ir,
                                scope_tree,
                                string_constants,
                                temp_counter,
                            );

                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_inst));
                            return TypedValue {
                                reg: struct_ptr,
                                ty: IrType::StructPtr(target_cls),
                            };
                        }

                        // Indirect fat-closure / bare function pointer call (`f(args)` or curried `make_adder(10)(5)` / `(lambda x: x+1)(5)`)
                        let is_expr_callee = !scope_tree.known_functions.contains(call_target)
                            && !scope_tree.extern_fns.contains_key(call_target)
                            && !NATIVE_C_LIB_I64_FNS.contains(&call_target)
                            && !NATIVE_C_LIB_FNS.contains(&call_target)
                            && (call_target.contains('(')
                                || call_target.contains("=>")
                                || call_target.contains("lambda")
                                || call_target.starts_with('|'));

                        if scope_tree.resolve(call_target).is_some() || is_expr_callee {
                            let fptr_ld = if let Some(binding) = scope_tree.resolve(call_target) {
                                let ld = format!("%fptr_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                if binding.type_hint.starts_with("i8*") {
                                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, binding.llvm_ptr));
                                } else if binding.type_hint == "i64" {
                                    let ld64 = format!("%fptr_ld64_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, binding.llvm_ptr));
                                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ld, ld64));
                                } else {
                                    let ld32 = format!("%fptr_ld32_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i32, i32* {}\n", ld32, binding.llvm_ptr));
                                    let ext64 = format!("%fptr_ext64_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = zext i32 {} to i64\n", ext64, ld32));
                                    ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ld, ext64));
                                }
                                ld
                            } else {
                                let callee_tv = self.eval_typed(call_target, ir, scope_tree, string_constants, temp_counter);
                                self.coerce_to_ptr(callee_tv, ir, temp_counter)
                            };

                            let unwrapped_callee = call_target
                                .trim()
                                .trim_start_matches('(')
                                .trim_end_matches(')')
                                .trim();
                            let static_pdefs = scope_tree
                                .fn_param_defs
                                .get(call_target)
                                .or_else(|| scope_tree.fn_param_defs.get(unwrapped_callee));
                            let ind_arity = static_pdefs
                                .map(|p| p.len())
                                .unwrap_or(0)
                                .max(raw_args.len())
                                .max(6);
                            let has_static_pdefs = static_pdefs.is_some();
                            let mut arg_regs = self.eval_call_args_with_defaults(
                                &raw_args,
                                static_pdefs,
                                ind_arity,
                                ir,
                                scope_tree,
                                string_constants,
                                temp_counter,
                            );
                            if !has_static_pdefs {
                                for slot in arg_regs.iter_mut().skip(raw_args.len()) {
                                    *slot = "-581005069656919554".to_string();
                                }
                            }
                            arg_regs.truncate(ind_arity);
                            let bare_ty_sig = vec!["i64"; ind_arity].join(", ");
                            let bare_call_args = arg_regs
                                .iter()
                                .map(|r| format!("i64 {}", r))
                                .collect::<Vec<_>>()
                                .join(", ");

                            let cid_lbl = *temp_counter;
                            *temp_counter += 1;
                            let is_null = format!("%fptr_null_{}", cid_lbl);
                            let valid_call_lbl = format!("call_valid_{}", cid_lbl);
                            let env_call_lbl = format!("call_env_{}", cid_lbl);
                            let bare_call_lbl = format!("call_bare_{}", cid_lbl);
                            let null_call_lbl = format!("call_null_{}", cid_lbl);
                            let cont_call_lbl = format!("call_cont_{}", cid_lbl);

                            ir.push_str(&format!("  {} = icmp eq i8* {}, null\n", is_null, fptr_ld));
                            ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", is_null, null_call_lbl, valid_call_lbl));

                            ir.push_str(&format!("{}:\n", valid_call_lbl));
                            let raw_fn = format!("%clos_fn_{}", cid_lbl);
                            let raw_env = format!("%clos_envp_{}", cid_lbl);
                            let has_env = format!("%clos_has_env_{}", cid_lbl);
                            ir.push_str(&format!(
                                "  call void @__omni_c_return_ptr(i8* null)\n  {} = call i8* @__omni_closure_fn(i8* {})\n  {} = call i8* @__omni_closure_env(i8* {})\n  {} = icmp ne i8* {}, null\n  br i1 {}, label %{}, label %{}\n\n",
                                raw_fn, fptr_ld, raw_env, fptr_ld, has_env, raw_env, has_env, env_call_lbl, bare_call_lbl
                            ));

                            ir.push_str(&format!("{}:\n", env_call_lbl));
                            let env_i64 = format!("%clos_env_i64_{}", cid_lbl);
                            let fptr_env = format!("%fptr_env_{}", cid_lbl);
                            let ret_env = format!("%ret_call_env_{}", cid_lbl);
                            ir.push_str(&format!(
                                "  {} = ptrtoint i8* {} to i64\n  {} = bitcast i8* {} to i64 (i64, {})*\n  {} = call i64 {}(i64 {}, {})\n  br label %{}\n\n",
                                env_i64, raw_env, fptr_env, raw_fn, bare_ty_sig, ret_env, fptr_env, env_i64, bare_call_args, cont_call_lbl
                            ));

                            ir.push_str(&format!("{}:\n", bare_call_lbl));
                            let fptr_bare = format!("%fptr_bare_{}", cid_lbl);
                            let ret_bare = format!("%ret_call_bare_{}", cid_lbl);
                            ir.push_str(&format!(
                                "  {} = bitcast i8* {} to i64 ({})*\n  {} = call i64 {}({})\n  br label %{}\n\n",
                                fptr_bare, raw_fn, bare_ty_sig, ret_bare, fptr_bare, bare_call_args, cont_call_lbl
                            ));

                            ir.push_str(&format!("{}:\n", null_call_lbl));
                            ir.push_str(&format!("  br label %{}\n\n", cont_call_lbl));

                            ir.push_str(&format!("{}:\n", cont_call_lbl));
                            let phi_ret = format!("%indirect_ret_{}", cid_lbl);
                            ir.push_str(&format!(
                                "  {} = phi i64 [ {}, %{} ], [ {}, %{} ], [ 0, %{} ]\n",
                                phi_ret, ret_env, env_call_lbl, ret_bare, bare_call_lbl, null_call_lbl
                            ));
                            let curried_head = call_target
                                .split('(')
                                .next()
                                .map(|s| s.trim())
                                .unwrap_or("");
                            let inline_lam_ret = OmniParser::try_parse_lambda_expr(call_target.trim().trim_start_matches('(').trim_end_matches(')').trim())
                                .and_then(|(ps, body)| {
                                    let mut lmap = Self::build_param_ir_types(&ps, &scope_tree.class_fields);
                                    if body.starts_with('{') && body.ends_with('}') {
                                        let stmts = OmniParser::new().parse_module("lam_ret", &body[1..body.len() - 1]).top_level_statements;
                                        let mut sc = None;
                                        let mut tp = None;
                                        Self::infer_return_signature_with_locals(&stmts, &scope_tree.class_fields, None, &mut lmap, &mut sc, &mut tp);
                                        sc
                                    } else {
                                        Self::infer_expr_return_ir_type_with_locals(&body, &scope_tree.class_fields, None, Some(&lmap))
                                    }
                                });
                            let fret = scope_tree
                                .fn_return_types
                                .get(call_target)
                                .or_else(|| scope_tree.fn_return_types.get(unwrapped_callee))
                                .or_else(|| scope_tree.fn_curried_return_types.get(curried_head))
                                .or_else(|| inline_lam_ret.as_ref());
                            return Self::apply_return_ir_type(phi_ret, fret, ir, temp_counter);
                        }

                        if scope_tree.known_functions.contains(call_target) {
                            let abi_n = scope_tree.fn_abi_arity(call_target);
                            let mut arg_regs = self.eval_call_args_with_defaults(
                                &raw_args,
                                scope_tree.fn_param_defs.get(call_target),
                                abi_n,
                                ir,
                                scope_tree,
                                string_constants,
                                temp_counter,
                            );
                            arg_regs.truncate(abi_n);
                            let fmt_args = arg_regs
                                .iter()
                                .map(|r| format!("i64 {}", r))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let ret_reg = format!("%fn_ret_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_fn_{}({})\n",
                                ret_reg, call_target, fmt_args
                            ));
                            let fret = scope_tree.fn_return_types.get(call_target);
                            return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                        }

                        let mut arg_regs = Vec::new();
                        for a in &raw_args {
                            let a_clean = Self::strip_kwarg_prefix(a);
                            let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                            arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                        }
                        let actual_c_args: String = arg_regs
                            .iter()
                            .map(|r| format!("i64 {}", r))
                            .collect::<Vec<_>>()
                            .join(", ");
                        if let Some((_, ref ret_ty)) = scope_tree.extern_fns.get(call_target).cloned() {
                            return self.emit_extern_fn_call(
                                call_target,
                                ret_ty.as_ref(),
                                &actual_c_args,
                                ir,
                                temp_counter,
                            );
                        }
                        while arg_regs.len() < 6 {
                            arg_regs.push("0".to_string());
                        }
                        let ret_reg = format!("%fn_ret_{}", *temp_counter);
                        *temp_counter += 1;
                        if NATIVE_C_LIB_I64_FNS.contains(&call_target) {
                            ir.push_str(&format!(
                                "  {} = call i64 @{}({})\n",
                                ret_reg, call_target, actual_c_args
                            ));
                            return TypedValue { reg: ret_reg, ty: IrType::I64 };
                        } else if NATIVE_C_LIB_FNS.contains(&call_target) {
                            ir.push_str(&format!(
                                "  {} = call i32 @{}({})\n",
                                ret_reg, call_target, actual_c_args
                            ));
                            return TypedValue { reg: ret_reg, ty: IrType::I32 };
                        } else {
                            ir.push_str(&format!(
                                "  {} = call i64 @{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                ret_reg, call_target, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                            ));
                            return TypedValue { reg: ret_reg, ty: IrType::I64 };
                        }
                    }
                }
            }
        }

        // 2. Builtin len(coll) -> full i64
        if let Some(caps) = self.len_re.captures(trimmed) {
            let var_name = caps[1].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%col_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                if binding.type_hint.contains("omni_chan") {
                    ir.push_str(&format!("  {} = call i64 @__omni_chan_len(i8* {})\n", len_res, loaded));
                } else if binding.type_hint.contains("omni_map") {
                    ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_res, loaded));
                } else {
                    ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, loaded));
                }
                return TypedValue { reg: len_res, ty: IrType::I64 };
            }
        }

        // 3. Slice pop: items.pop() -> full i64
        if let Some(caps) = self.pop_re.captures(trimmed) {
            let var_name = caps[1].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%slice_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                let pop_res = format!("%pop_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_slice_pop(i8* {})\n", pop_res, loaded));
                let elem_tag = scope_tree.collection_elem_types.get(var_name).map(|s| s.as_str());
                return Self::unpack_raw_elem_bits(pop_res, elem_tag, ir, temp_counter);
            }
        }

        // 4. Index Lookup: items[i] or m["key"] -> full i64
        if let Some(caps) = self.idx_re.captures(trimmed) {
            let var_name = caps[1].trim();
            let idx_expr = caps[2].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%col_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                if binding.type_hint.contains("omni_map") {
                    let val_res = format!("%map_get_{}", *temp_counter);
                    *temp_counter += 1;

                    if (idx_expr.starts_with('"') && idx_expr.ends_with('"'))
                        || (idx_expr.starts_with('\'') && idx_expr.ends_with('\''))
                    {
                        let text = &idx_expr[1..idx_expr.len() - 1];
                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                                val_res, loaded, str_ptr
                            ));
                        } else {
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_map_get(i8* {}, i8* null)\n",
                                val_res, loaded
                            ));
                        }
                    } else if Self::is_string_expr(idx_expr, scope_tree) {
                        let key_ptr = self.eval_str_ptr(idx_expr, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                            val_res, loaded, key_ptr
                        ));
                    } else {
                        let idx_tv = self.eval_typed(idx_expr, ir, scope_tree, string_constants, temp_counter);
                        let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_map_get_int(i8* {}, i64 {})\n",
                            val_res, loaded, idx_64
                        ));
                    }
                    return TypedValue { reg: val_res, ty: IrType::I64 };
                } else {
                    let idx_tv = self.eval_typed(idx_expr, ir, scope_tree, string_constants, temp_counter);
                    let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);

                    let val_res = format!("%slice_get_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n",
                        val_res, loaded, idx_64
                    ));
                    return TypedValue { reg: val_res, ty: IrType::I64 };
                }
            }
        }

        // 5. Binary arithmetic with IEEE-754 `double` promotion & 64-bit `i64` arithmetic
        if let Some((left, op, right)) = Self::split_binary_expr(trimmed) {
            if op == "+" && (Self::is_string_expr(&left, scope_tree) || Self::is_string_expr(&right, scope_tree)) {
                let str_ptr = self.eval_str_ptr(trimmed, ir, scope_tree, string_constants, temp_counter);
                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", str_ptr));
                return TypedValue { reg: str_ptr, ty: IrType::Ptr };
            }

            if op == "**" {
                if let (Some(lb), Some(rb)) = (parse_polyglot_i64(&left), parse_polyglot_i64(&right)) {
                    if (0..=62).contains(&rb) {
                        return TypedValue {
                            reg: lb.wrapping_pow(rb as u32).to_string(),
                            ty: IrType::I64,
                        };
                    }
                }
            }

            let ltv = self.eval_typed(&left, ir, scope_tree, string_constants, temp_counter);
            let rtv = self.eval_typed(&right, ir, scope_tree, string_constants, temp_counter);

            if ltv.ty == IrType::F64 || rtv.ty == IrType::F64 {
                let lf = self.coerce_to_f64(ltv, ir, temp_counter);
                let rf = self.coerce_to_f64(rtv, ir, temp_counter);
                let res_var = format!("%tf_{}", *temp_counter);
                *temp_counter += 1;
                if op == "**" {
                    if let Some(exp_i) = parse_polyglot_i64(&right) {
                        if exp_i == 0 {
                            return TypedValue { reg: format_llvm_f64(1.0), ty: IrType::F64 };
                        } else if (1..=12).contains(&exp_i) {
                            let mut acc = lf.clone();
                            for _ in 1..exp_i {
                                let step = format!("%powf_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = fmul double {}, {}\n", step, acc, lf));
                                acc = step;
                            }
                            return TypedValue { reg: acc, ty: IrType::F64 };
                        }
                    }
                    ir.push_str(&format!("  {} = fmul double {}, {}\n", res_var, lf, lf));
                    return TypedValue { reg: res_var, ty: IrType::F64 };
                }
                let finstr = match op.as_str() {
                    "+" => "fadd",
                    "-" => "fsub",
                    "*" => "fmul",
                    "/" => "fdiv",
                    "%" => "frem",
                    _ => "fadd",
                };
                ir.push_str(&format!("  {} = {} double {}, {}\n", res_var, finstr, lf, rf));
                return TypedValue { reg: res_var, ty: IrType::F64 };
            }

            let left_val = self.coerce_to_i64(ltv, ir, temp_counter);
            let right_val = self.coerce_to_i64(rtv, ir, temp_counter);

            let res_var = format!("%t_{}", *temp_counter);
            *temp_counter += 1;

            let instr = match op.as_str() {
                "+" => "add",
                "-" => "sub",
                "*" => "mul",
                "/" | "%" | "//" => {
                    return self.emit_checked_int_div_rem(
                        &op,
                        &left_val,
                        &right_val,
                        ir,
                        string_constants,
                        temp_counter,
                    );
                }
                "^" => "xor",
                "&" => "and",
                "|" => "or",
                "<<" => "shl",
                ">>" => "ashr",
                ">>>" => "lshr",
                "**" => {
                    if let Some(exp_i) = parse_polyglot_i64(&right) {
                        if exp_i == 0 {
                            return TypedValue { reg: "1".to_string(), ty: IrType::I64 };
                        } else if (1..=16).contains(&exp_i) {
                            let mut acc = left_val.clone();
                            for _ in 1..exp_i {
                                let step = format!("%pow64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = mul i64 {}, {}\n", step, acc, left_val));
                                acc = step;
                            }
                            return TypedValue { reg: acc, ty: IrType::I64 };
                        }
                    }
                    ir.push_str(&format!("  {} = call i64 @__omni_ipow64(i64 {}, i64 {})\n", res_var, left_val, right_val));
                    return TypedValue { reg: res_var, ty: IrType::I64 };
                }
                _ => "add",
            };

            ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, instr, left_val, right_val));
            return TypedValue { reg: res_var, ty: IrType::I64 };
        }

        // Unary negation (`-expr`) and unary plus (`+expr`) on non-literal operands
        if let Some(neg_inner) = trimmed.strip_prefix('-') {
            let inner = neg_inner.trim();
            if !inner.is_empty() {
                let tv = self.eval_typed(inner, ir, scope_tree, string_constants, temp_counter);
                if tv.ty == IrType::F64 {
                    let res = format!("%fneg_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = fsub double 0x0000000000000000, {}\n", res, tv.reg));
                    return TypedValue { reg: res, ty: IrType::F64 };
                } else {
                    let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                    let res = format!("%ineg_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = sub i64 0, {}\n", res, v64));
                    return TypedValue { reg: res, ty: IrType::I64 };
                }
            }
        }
        if let Some(pos_inner) = trimmed.strip_prefix('+') {
            let inner = pos_inner.trim();
            if !inner.is_empty() {
                return self.eval_typed(inner, ir, scope_tree, string_constants, temp_counter);
            }
        }

        // 6. Variable in symbol table
        if let Some(binding) = scope_tree.resolve(trimmed) {
            if binding.type_hint == "double" {
                let res_var = format!("%flt_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load double, double* {}\n", res_var, binding.llvm_ptr));
                return TypedValue { reg: res_var, ty: IrType::F64 };
            } else if binding.type_hint == "i64" {
                let res_var = format!("%t64_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i64, i64* {}\n", res_var, binding.llvm_ptr));
                return TypedValue { reg: res_var, ty: IrType::I64 };
            } else if binding.type_hint.contains("omni_slice") {
                let loaded = format!("%slice_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, loaded));
                return TypedValue { reg: len_res, ty: IrType::I64 };
            } else if binding.type_hint.contains("omni_map") {
                let loaded = format!("%map_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_res, loaded));
                return TypedValue { reg: len_res, ty: IrType::I64 };
            } else if binding.type_hint.starts_with("%struct.") {
                let cls_name = binding.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                let loaded = format!("%inst_ptr_ld_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", loaded, cls_name, cls_name, binding.llvm_ptr));
                return TypedValue {
                    reg: loaded,
                    ty: IrType::StructPtr(cls_name.to_string()),
                };
            } else if binding.type_hint.starts_with("%enum.") {
                let en_name = binding.type_hint.trim_start_matches("%enum.").trim_end_matches('*');
                let loaded = format!("%enum_ptr_ld_{}", *temp_counter);
                let raw_i8 = format!("%enum_i8_ld_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!(
                    "  {} = load %enum.{}*, %enum.{}** {}\n  {} = bitcast %enum.{}* {} to i8*\n",
                    loaded, en_name, en_name, binding.llvm_ptr, raw_i8, en_name, loaded
                ));
                return TypedValue { reg: raw_i8, ty: IrType::Ptr };
            } else if binding.type_hint.starts_with("i8*") {
                let loaded = format!("%ptr_ld_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                return TypedValue { reg: loaded, ty: IrType::Ptr };
            } else {
                let res_var = format!("%t_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i32, i32* {}\n", res_var, binding.llvm_ptr));
                return TypedValue { reg: res_var, ty: IrType::I32 };
            }
        }

        // Unqualified field access on `self`
        if let Some(self_b) = scope_tree.resolve("self") {
            if self_b.type_hint.starts_with("%struct.") {
                let cls_name = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(trimmed)) {
                    let inst_ld = format!("%inst_fld_eval_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, self_b.llvm_ptr));
                    let gep = format!("%eval_gep_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                    let val64 = format!("%val64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));
                    if let Some(ft) = scope_tree.class_field_types.get(cls_name).and_then(|m| m.get(trimmed)) {
                        if is_float_type_hint(ft) {
                            let fval = format!("%self_fld_f64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fval, val64));
                            return TypedValue { reg: fval, ty: IrType::F64 };
                        } else if ft.starts_with("i8*")
                            || ft.starts_with("omni_slice")
                            || ft.starts_with("omni_map")
                            || is_string_type_hint(ft)
                        {
                            let sval = format!("%self_fld_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", sval, val64));
                            return TypedValue { reg: sval, ty: IrType::Ptr };
                        }
                    }
                    return TypedValue { reg: val64, ty: IrType::I64 };
                }
            }
        }

        // Module-qualified or bare function reference as a first-class value (`rsa.prime.getprime` or `getprime`)
        if trimmed.contains('.') && !trimmed.contains('(') {
            if let Some(last_seg) = trimmed.rsplit('.').next() {
                let fn_cand = last_seg.trim();
                if scope_tree.known_functions.contains(fn_cand) {
                    let abi_n = scope_tree.fn_abi_arity(fn_cand);
                    let sig = vec!["i64"; abi_n].join(", ");
                    let raw_fptr = format!("%fn_raw_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = bitcast i64 ({})* @__omni_fn_{} to i8*\n",
                        raw_fptr, sig, fn_cand
                    ));
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_fptr));
                    return TypedValue { reg: raw_fptr, ty: IrType::Ptr };
                }
            }
        } else if scope_tree.known_functions.contains(trimmed) {
            let abi_n = scope_tree.fn_abi_arity(trimmed);
            let sig = vec!["i64"; abi_n].join(", ");
            let raw_fptr = format!("%fn_raw_ptr_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = bitcast i64 ({})* @__omni_fn_{} to i8*\n",
                raw_fptr, sig, trimmed
            ));
            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_fptr));
            return TypedValue { reg: raw_fptr, ty: IrType::Ptr };
        }

        // Mid-Expression LLM Escalation Gate for non-trivial unrecognized polyglot sub-expressions
        if !trimmed.is_empty()
            && (trimmed.contains('(')
                || trimmed.contains('[')
                || trimmed.contains('.')
                || trimmed.contains("::")
                || trimmed.contains("..")
                || trimmed.contains("=>"))
        {
            if let Some(ai_tv) = self.escalate_subexpr_to_llm(
                trimmed,
                "PolyglotSubExpression",
                ir,
                scope_tree,
                string_constants,
                temp_counter,
            ) {
                return ai_tv;
            }
        }

        TypedValue { reg: "0".to_string(), ty: IrType::I64 }
    }

    /// Evaluates an expression or variable into an LLVM `i32` register (used for POSIX fds & exit codes).
    fn eval_val(
        &self,
        expr: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
        self.coerce_to_i32(tv, ir, temp_counter)
    }

    /// Evaluates a comparison or boolean expression into an LLVM `i1` register (`icmp` / `fcmp` / `@strcmp`).
    fn eval_cond(
        &self,
        cond: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let src_cond = cond.trim();

        // Pratt AST boolean & relational evaluation: handles arbitrary parenthesized `(a || b) && !(c && d)`,
        // Python `is None` / `is not None` / `not x == 5`, and float/int/string/null comparisons.
        if let Ok(ast) = OmniParser::parse_expr(src_cond) {
            match ast.node {
                ExprAst::BoolLit(b) => {
                    let cval = if b { "true" } else { "false" };
                    let cond_var = format!("%bool_lit_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = icmp eq i1 {}, true\n", cond_var, cval));
                    return cond_var;
                }
                ExprAst::Binary { ref op, ref lhs, ref rhs } if op == "||" || op == "or" => {
                    let sc_id = *temp_counter;
                    *temp_counter += 1;
                    let l_res = self.eval_cond(lhs.slice(src_cond), ir, scope_tree, string_constants, temp_counter);
                    ir.push_str(&format!(
                        "  br i1 {}, label %sc_or_short_{}, label %sc_or_rhs_{}\n\nsc_or_short_{}:\n  br label %sc_or_merge_{}\n\nsc_or_rhs_{}:\n",
                        l_res, sc_id, sc_id, sc_id, sc_id, sc_id
                    ));
                    let r_res = self.eval_cond(rhs.slice(src_cond), ir, scope_tree, string_constants, temp_counter);
                    let res = format!("%or_cond_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  br label %sc_or_done_{}\n\nsc_or_done_{}:\n  br label %sc_or_merge_{}\n\nsc_or_merge_{}:\n  {} = phi i1 [ true, %sc_or_short_{} ], [ {}, %sc_or_done_{} ]\n",
                        sc_id, sc_id, sc_id, sc_id, res, sc_id, r_res, sc_id
                    ));
                    return res;
                }
                ExprAst::Binary { ref op, ref lhs, ref rhs } if op == "&&" || op == "and" => {
                    let sc_id = *temp_counter;
                    *temp_counter += 1;
                    let l_res = self.eval_cond(lhs.slice(src_cond), ir, scope_tree, string_constants, temp_counter);
                    ir.push_str(&format!(
                        "  br i1 {}, label %sc_and_rhs_{}, label %sc_and_short_{}\n\nsc_and_short_{}:\n  br label %sc_and_merge_{}\n\nsc_and_rhs_{}:\n",
                        l_res, sc_id, sc_id, sc_id, sc_id, sc_id
                    ));
                    let r_res = self.eval_cond(rhs.slice(src_cond), ir, scope_tree, string_constants, temp_counter);
                    let res = format!("%and_cond_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  br label %sc_and_done_{}\n\nsc_and_done_{}:\n  br label %sc_and_merge_{}\n\nsc_and_merge_{}:\n  {} = phi i1 [ false, %sc_and_short_{} ], [ {}, %sc_and_done_{} ]\n",
                        sc_id, sc_id, sc_id, sc_id, res, sc_id, r_res, sc_id
                    ));
                    return res;
                }
                ExprAst::Unary { ref op, ref rhs } if op == "!" || op == "not" => {
                    let inner = self.eval_cond(rhs.slice(src_cond), ir, scope_tree, string_constants, temp_counter);
                    let neg = format!("%not_cond_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = xor i1 {}, true\n", neg, inner));
                    return neg;
                }
                ExprAst::Binary { ref op, ref lhs, ref rhs } if op == "in" || op == "not in" => {
                    let needle_raw = lhs.slice(src_cond).trim();
                    let hay_raw = rhs.slice(src_cond).trim();
                    let raw_i1 = if let Some(b) = scope_tree.resolve(hay_raw).filter(|b| b.type_hint.contains("omni_map")).cloned() {
                        let ld = format!("%in_mp_ld_{}", *temp_counter);
                        let c1 = format!("%in_mp_res_{}", *temp_counter);
                        *temp_counter += 2;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                        if Self::is_string_expr(needle_raw, scope_tree) {
                            let kp = self.eval_str_ptr(needle_raw, ir, scope_tree, string_constants, temp_counter);
                            ir.push_str(&format!("  {} = call i1 @__omni_map_contains(i8* {}, i8* {})\n", c1, ld, kp));
                        } else {
                            let ktv = self.eval_typed(needle_raw, ir, scope_tree, string_constants, temp_counter);
                            let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                            ir.push_str(&format!("  {} = call i1 @__omni_map_contains_int(i8* {}, i64 {})\n", c1, ld, k64));
                        }
                        c1
                    } else if Self::is_string_expr(hay_raw, scope_tree) {
                        let hp = self.eval_str_ptr(hay_raw, ir, scope_tree, string_constants, temp_counter);
                        let np = self.eval_str_ptr(needle_raw, ir, scope_tree, string_constants, temp_counter);
                        let r64 = format!("%in_str_r_{}", *temp_counter);
                        let c1 = format!("%in_str_c_{}", *temp_counter);
                        *temp_counter += 2;
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_str_contains(i8* {}, i8* {})\n  {} = icmp ne i64 {}, 0\n",
                            r64, hp, np, c1, r64
                        ));
                        c1
                    } else {
                        let hp = if let Some(b) = scope_tree.resolve(hay_raw).filter(|b| b.type_hint.starts_with("i8*")) {
                            let ld = format!("%in_sl_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                            ld
                        } else {
                            let htv = self.eval_typed(hay_raw, ir, scope_tree, string_constants, temp_counter);
                            self.coerce_to_ptr(htv, ir, temp_counter)
                        };
                        let is_s = if Self::is_string_expr(needle_raw, scope_tree) { "1" } else { "0" };
                        let ntv = self.eval_typed(needle_raw, ir, scope_tree, string_constants, temp_counter);
                        let n64 = self.coerce_to_raw_i64_bits(ntv, ir, temp_counter);
                        let r64 = format!("%in_sl_r_{}", *temp_counter);
                        let c1 = format!("%in_sl_c_{}", *temp_counter);
                        *temp_counter += 2;
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_slice_contains(i8* {}, i64 {}, i64 {})\n  {} = icmp ne i64 {}, 0\n",
                            r64, hp, n64, is_s, c1, r64
                        ));
                        c1
                    };
                    if op == "not in" {
                        let neg = format!("%notin_c_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = xor i1 {}, true\n", neg, raw_i1));
                        return neg;
                    }
                    return raw_i1;
                }
                ExprAst::Binary { ref op, ref lhs, ref rhs }
                    if matches!(
                        op.as_str(),
                        "==" | "!=" | "===" | "!==" | "~=" | "<" | "<=" | ">" | ">=" | "is" | "is not"
                    ) =>
                {
                    let left_raw = lhs.slice(src_cond);
                    let right_raw = rhs.slice(src_cond);
                    let norm_op = match op.as_str() {
                        "is" => "==",
                        "is not" => "!=",
                        other => other,
                    };

                    if is_null_literal(left_raw) || is_null_literal(right_raw) {
                        let non_null_expr = if is_null_literal(left_raw) { right_raw } else { left_raw };
                        let predicate = match norm_op {
                            "===" | "==" => "eq",
                            _ => "ne",
                        };
                        if is_null_literal(non_null_expr) {
                            let cval = if predicate == "eq" { "true" } else { "false" };
                            let cond_var = format!("%null_const_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = icmp eq i1 {}, true\n", cond_var, cval));
                            return cond_var;
                        }
                        let tv = self.eval_typed(non_null_expr, ir, scope_tree, string_constants, temp_counter);
                        let cond_var = format!("%null_cmp_{}", *temp_counter);
                        *temp_counter += 1;
                        match tv.ty {
                            IrType::Ptr => {
                                ir.push_str(&format!("  {} = icmp {} i8* {}, null\n", cond_var, predicate, tv.reg));
                            }
                            IrType::StructPtr(cls) => {
                                ir.push_str(&format!("  {} = icmp {} %struct.{}* {}, null\n", cond_var, predicate, cls, tv.reg));
                            }
                            _ => {
                                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                                let is_n = format!("%null_s1_{}", *temp_counter);
                                let is_u = format!("%null_s2_{}", *temp_counter);
                                *temp_counter += 2;
                                if predicate == "eq" {
                                    ir.push_str(&format!(
                                        "  {} = icmp eq i64 {}, {}\n  {} = icmp eq i64 {}, {}\n  {} = or i1 {}, {}\n",
                                        is_n, v64, OMNI_NULL_SENTINEL, is_u, v64, OMNI_UNSET_SENTINEL, cond_var, is_n, is_u
                                    ));
                                } else {
                                    ir.push_str(&format!(
                                        "  {} = icmp ne i64 {}, {}\n  {} = icmp ne i64 {}, {}\n  {} = and i1 {}, {}\n",
                                        is_n, v64, OMNI_NULL_SENTINEL, is_u, v64, OMNI_UNSET_SENTINEL, cond_var, is_n, is_u
                                    ));
                                }
                            }
                        }
                        return cond_var;
                    }

                    if (Self::is_string_expr(left_raw, scope_tree) || Self::is_string_expr(right_raw, scope_tree))
                        && parse_polyglot_i64(left_raw).is_none()
                        && parse_polyglot_i64(right_raw).is_none()
                        && parse_polyglot_f64(left_raw).is_none()
                        && parse_polyglot_f64(right_raw).is_none()
                    {
                        let left_ptr = self.eval_str_ptr(left_raw, ir, scope_tree, string_constants, temp_counter);
                        let right_ptr = self.eval_str_ptr(right_raw, ir, scope_tree, string_constants, temp_counter);
                        let cmp_res = format!("%strcmp_res_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i32 @strcmp(i8* {}, i8* {})\n", cmp_res, left_ptr, right_ptr));
                        let cond_var = format!("%cond_{}", *temp_counter);
                        *temp_counter += 1;
                        let predicate = match norm_op {
                            "===" | "==" => "eq",
                            "!==" | "!=" | "~=" => "ne",
                            "<" => "slt",
                            "<=" => "sle",
                            ">" => "sgt",
                            ">=" => "sge",
                            _ => "eq",
                        };
                        ir.push_str(&format!("  {} = icmp {} i32 {}, 0\n", cond_var, predicate, cmp_res));
                        return cond_var;
                    }

                    let ltv = self.eval_typed(left_raw, ir, scope_tree, string_constants, temp_counter);
                    let rtv = self.eval_typed(right_raw, ir, scope_tree, string_constants, temp_counter);
                    let cond_var = format!("%cond_{}", *temp_counter);
                    *temp_counter += 1;
                    if ltv.ty == IrType::F64 || rtv.ty == IrType::F64 {
                        let lf = self.coerce_to_f64(ltv, ir, temp_counter);
                        let rf = self.coerce_to_f64(rtv, ir, temp_counter);
                        let fpred = match norm_op {
                            "===" | "==" => "oeq",
                            "!==" | "!=" | "~=" => "une",
                            "<" => "olt",
                            "<=" => "ole",
                            ">" => "ogt",
                            ">=" => "oge",
                            _ => "une",
                        };
                        ir.push_str(&format!("  {} = fcmp {} double {}, {}\n", cond_var, fpred, lf, rf));
                        return cond_var;
                    }

                    let left = self.coerce_to_i64(ltv, ir, temp_counter);
                    let right = self.coerce_to_i64(rtv, ir, temp_counter);
                    let predicate = match norm_op {
                        "===" | "==" => "eq",
                        "!==" | "!=" | "~=" => "ne",
                        "<" => "slt",
                        "<=" => "sle",
                        ">" => "sgt",
                        ">=" => "sge",
                        _ => "ne",
                    };
                    ir.push_str(&format!("  {} = icmp {} i64 {}, {}\n", cond_var, predicate, left, right));
                    return cond_var;
                }
                _ => {}
            }
        }

        let mut clean_cond = src_cond.to_string();
        if clean_cond.contains(" is not None") {
            clean_cond = clean_cond.replace(" is not None", " != null");
        } else if clean_cond.contains(" is None") {
            clean_cond = clean_cond.replace(" is None", " == null");
        }

        if let Some((left_c, right_c)) = clean_cond
            .split_once(" or ")
            .or_else(|| clean_cond.split_once(" || "))
        {
            let sc_id = *temp_counter;
            *temp_counter += 1;
            let l_res = self.eval_cond(left_c, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!(
                "  br i1 {}, label %sc_or_short_{}, label %sc_or_rhs_{}\n\nsc_or_short_{}:\n  br label %sc_or_merge_{}\n\nsc_or_rhs_{}:\n",
                l_res, sc_id, sc_id, sc_id, sc_id, sc_id
            ));
            let r_res = self.eval_cond(right_c, ir, scope_tree, string_constants, temp_counter);
            let res = format!("%or_cond_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  br label %sc_or_done_{}\n\nsc_or_done_{}:\n  br label %sc_or_merge_{}\n\nsc_or_merge_{}:\n  {} = phi i1 [ true, %sc_or_short_{} ], [ {}, %sc_or_done_{} ]\n",
                sc_id, sc_id, sc_id, sc_id, res, sc_id, r_res, sc_id
            ));
            return res;
        }
        if let Some((left_c, right_c)) = clean_cond
            .split_once(" and ")
            .or_else(|| clean_cond.split_once(" && "))
        {
            let sc_id = *temp_counter;
            *temp_counter += 1;
            let l_res = self.eval_cond(left_c, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!(
                "  br i1 {}, label %sc_and_rhs_{}, label %sc_and_short_{}\n\nsc_and_short_{}:\n  br label %sc_and_merge_{}\n\nsc_and_rhs_{}:\n",
                l_res, sc_id, sc_id, sc_id, sc_id, sc_id
            ));
            let r_res = self.eval_cond(right_c, ir, scope_tree, string_constants, temp_counter);
            let res = format!("%and_cond_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  br label %sc_and_done_{}\n\nsc_and_done_{}:\n  br label %sc_and_merge_{}\n\nsc_and_merge_{}:\n  {} = phi i1 [ false, %sc_and_short_{} ], [ {}, %sc_and_done_{} ]\n",
                sc_id, sc_id, sc_id, sc_id, res, sc_id, r_res, sc_id
            ));
            return res;
        }

        let trimmed = clean_cond.trim();

        if let Some(rest) = trimmed.strip_prefix("not ").or_else(|| {
            if trimmed.starts_with('!') && !trimmed.starts_with("!=") {
                Some(&trimmed[1..])
            } else {
                None
            }
        }) {
            let inner = self.eval_cond(rest.trim(), ir, scope_tree, string_constants, temp_counter);
            let neg = format!("%not_cond_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = xor i1 {}, true\n", neg, inner));
            return neg;
        }

        if let Some(caps) = self.cmp_re.captures(trimmed) {
            let left_raw = caps[1].trim();
            let op = &caps[2];
            let right_raw = caps[3].trim();

            // 1. Null / nil / None checks (`if err != nil`, `if node == null`) — never pass `null` to `@strcmp`
            if is_null_literal(left_raw) || is_null_literal(right_raw) {
                let non_null_expr = if is_null_literal(left_raw) { right_raw } else { left_raw };
                let predicate = match op {
                    "===" | "==" => "eq",
                    "!==" | "!=" | "~=" => "ne",
                    _ => "ne",
                };
                if is_null_literal(non_null_expr) {
                    let cval = if predicate == "eq" { "true" } else { "false" };
                    let cond_var = format!("%null_const_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = icmp eq i1 {}, true\n", cond_var, cval));
                    return cond_var;
                }
                let tv = self.eval_typed(non_null_expr, ir, scope_tree, string_constants, temp_counter);
                let cond_var = format!("%null_cmp_{}", *temp_counter);
                *temp_counter += 1;
                match tv.ty {
                    IrType::Ptr => {
                        ir.push_str(&format!("  {} = icmp {} i8* {}, null\n", cond_var, predicate, tv.reg));
                    }
                    IrType::StructPtr(cls) => {
                        ir.push_str(&format!(
                            "  {} = icmp {} %struct.{}* {}, null\n",
                            cond_var, predicate, cls, tv.reg
                        ));
                    }
                    _ => {
                        let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                        let is_n = format!("%null_s1_{}", *temp_counter);
                        let is_u = format!("%null_s2_{}", *temp_counter);
                        *temp_counter += 2;
                        if predicate == "eq" {
                            ir.push_str(&format!(
                                "  {} = icmp eq i64 {}, {}\n  {} = icmp eq i64 {}, {}\n  {} = or i1 {}, {}\n",
                                is_n, v64, OMNI_NULL_SENTINEL, is_u, v64, OMNI_UNSET_SENTINEL, cond_var, is_n, is_u
                            ));
                        } else {
                            ir.push_str(&format!(
                                "  {} = icmp ne i64 {}, {}\n  {} = icmp ne i64 {}, {}\n  {} = and i1 {}, {}\n",
                                is_n, v64, OMNI_NULL_SENTINEL, is_u, v64, OMNI_UNSET_SENTINEL, cond_var, is_n, is_u
                            ));
                        }
                    }
                }
                return cond_var;
            }

            // 2. Lexicographical string comparisons via `@strcmp`
            if (Self::is_string_expr(left_raw, scope_tree) || Self::is_string_expr(right_raw, scope_tree))
                && parse_polyglot_i64(left_raw).is_none()
                && parse_polyglot_i64(right_raw).is_none()
                && parse_polyglot_f64(left_raw).is_none()
                && parse_polyglot_f64(right_raw).is_none()
            {
                let left_ptr = self.eval_str_ptr(left_raw, ir, scope_tree, string_constants, temp_counter);
                let right_ptr = self.eval_str_ptr(right_raw, ir, scope_tree, string_constants, temp_counter);
                let cmp_res = format!("%strcmp_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i32 @strcmp(i8* {}, i8* {})\n", cmp_res, left_ptr, right_ptr));

                let cond_var = format!("%cond_{}", *temp_counter);
                *temp_counter += 1;
                let predicate = match op {
                    "===" | "==" => "eq",
                    "!==" | "!=" | "~=" => "ne",
                    "<" => "slt",
                    "<=" => "sle",
                    ">" => "sgt",
                    ">=" => "sge",
                    _ => "eq",
                };
                ir.push_str(&format!(
                    "  {} = icmp {} i32 {}, 0\n",
                    cond_var, predicate, cmp_res
                ));
                return cond_var;
            }

            // 3. Typed IEEE-754 `fcmp` or 64-bit `icmp`
            let ltv = self.eval_typed(left_raw, ir, scope_tree, string_constants, temp_counter);
            let rtv = self.eval_typed(right_raw, ir, scope_tree, string_constants, temp_counter);

            let cond_var = format!("%cond_{}", *temp_counter);
            *temp_counter += 1;

            if ltv.ty == IrType::F64 || rtv.ty == IrType::F64 {
                let lf = self.coerce_to_f64(ltv, ir, temp_counter);
                let rf = self.coerce_to_f64(rtv, ir, temp_counter);
                let fpred = match op {
                    "===" | "==" => "oeq",
                    "!==" | "!=" | "~=" => "une",
                    "<" => "olt",
                    "<=" => "ole",
                    ">" => "ogt",
                    ">=" => "oge",
                    _ => "une",
                };
                ir.push_str(&format!("  {} = fcmp {} double {}, {}\n", cond_var, fpred, lf, rf));
                return cond_var;
            }

            let left = self.coerce_to_i64(ltv, ir, temp_counter);
            let right = self.coerce_to_i64(rtv, ir, temp_counter);

            let predicate = match op {
                "===" | "==" => "eq",
                "!==" | "!=" | "~=" => "ne",
                "<" => "slt",
                "<=" => "sle",
                ">" => "sgt",
                ">=" => "sge",
                _ => "ne",
            };

            ir.push_str(&format!(
                "  {} = icmp {} i64 {}, {}\n",
                cond_var, predicate, left, right
            ));
            return cond_var;
        }

        // Truthiness check on typed value
        let tv = self.eval_typed(trimmed, ir, scope_tree, string_constants, temp_counter);
        let cond_var = format!("%cond_{}", *temp_counter);
        *temp_counter += 1;
        match tv.ty {
            IrType::F64 => {
                ir.push_str(&format!(
                    "  {} = fcmp une double {}, 0x0000000000000000\n",
                    cond_var, tv.reg
                ));
            }
            IrType::Ptr => {
                if Self::is_string_expr(trimmed, scope_tree) {
                    let slen = format!("%str_tlen_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i64 @__omni_str_len(i8* {})\n  {} = icmp ne i64 {}, 0\n",
                        slen, tv.reg, cond_var, slen
                    ));
                } else {
                    ir.push_str(&format!("  {} = icmp ne i8* {}, null\n", cond_var, tv.reg));
                }
            }
            IrType::StructPtr(cls) => {
                ir.push_str(&format!(
                    "  {} = icmp ne %struct.{}* {}, null\n",
                    cond_var, cls, tv.reg
                ));
            }
            IrType::I1 => return tv.reg,
            _ => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                let nz = format!("%tr_nz_{}", *temp_counter);
                let nn = format!("%tr_nn_{}", *temp_counter);
                let nu = format!("%tr_nu_{}", *temp_counter);
                let n1 = format!("%tr_n1_{}", *temp_counter);
                *temp_counter += 4;
                ir.push_str(&format!(
                    "  {} = icmp ne i64 {}, 0\n  {} = icmp ne i64 {}, {}\n  {} = icmp ne i64 {}, {}\n  {} = and i1 {}, {}\n  {} = and i1 {}, {}\n",
                    nz, v64, nn, v64, OMNI_NULL_SENTINEL, nu, v64, OMNI_UNSET_SENTINEL, n1, nz, nn, cond_var, n1, nu
                ));
            }
        }
        cond_var
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_collection_filter_inline(
        &self,
        target: &str,
        src_col: &str,
        param_var: &str,
        predicate: &str,
        is_scoped_decl: bool,
        ir: &mut String,
        scope_tree: &mut ScopeTree,
        deferred_frees: &mut Vec<CleanupOp>,
        temp_counter: &mut usize,
        lbl_counter: &mut usize,
        string_constants: &[(String, String, usize)],
    ) -> bool {
        let src_binding = match scope_tree.resolve(src_col).cloned() {
            Some(b) => b,
            None => return false,
        };

        let ptr = format!("%slice_var_{}_{}", target, *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = alloca i8*\n", ptr));

        let slice_res = format!("%slice_filter_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = call i8* @__omni_slice_create(i64 4)\n", slice_res));
        ir.push_str(&format!("  store i8* {}, i8** {}\n", slice_res, ptr));

        deferred_frees.push(CleanupOp::DestroySlice(ptr.clone()));
        scope_tree.insert(
            target.to_string(),
            SymbolBinding {
                name: target.to_string(),
                llvm_ptr: ptr,
                type_hint: "i8* (omni_slice)".to_string(),
                is_scoped: is_scoped_decl,
            },
        );

        let src_ld = format!("%src_filter_ld_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = load i8*, i8** {}\n", src_ld, src_binding.llvm_ptr));

        let len_res = format!("%src_len_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, src_ld));

        let loop_id = *lbl_counter;
        *lbl_counter += 1;
        let cond_lbl = format!("filter_cond_{}", loop_id);
        let body_lbl = format!("filter_body_{}", loop_id);
        let inc_lbl = format!("filter_inc_{}", loop_id);
        let exit_lbl = format!("filter_exit_{}", loop_id);

        let idx_var = format!("%filter_idx_{}", loop_id);
        ir.push_str(&format!("  {} = alloca i64\n", idx_var));
        ir.push_str(&format!("  store i64 0, i64* {}\n", idx_var));
        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

        ir.push_str(&format!("{}:\n", cond_lbl));
        let curr_idx = format!("%curr_filter_idx_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = load i64, i64* {}\n", curr_idx, idx_var));
        let cmp = format!("%filter_cmp_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = icmp slt i64 {}, {}\n", cmp, curr_idx, len_res));
        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cmp, body_lbl, exit_lbl));

        ir.push_str(&format!("{}:\n", body_lbl));
        let elem_val = format!("%elem_val_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n", elem_val, src_ld, curr_idx));

        let param_ptr = format!("%var_{}_{}", param_var, *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = alloca i64\n", param_ptr));
        ir.push_str(&format!("  store i64 {}, i64* {}\n", elem_val, param_ptr));
        scope_tree.insert(
            param_var.to_string(),
            SymbolBinding {
                name: param_var.to_string(),
                llvm_ptr: param_ptr,
                type_hint: "i64".to_string(),
                is_scoped: true,
            },
        );

        let cond_val = self.eval_cond(predicate, ir, scope_tree, string_constants, temp_counter);
        let append_lbl = format!("filter_append_{}", loop_id);
        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cond_val, append_lbl, inc_lbl));

        ir.push_str(&format!("{}:\n", append_lbl));
        ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", slice_res, elem_val));
        ir.push_str(&format!("  br label %{}\n\n", inc_lbl));

        ir.push_str(&format!("{}:\n", inc_lbl));
        let next_idx = format!("%next_filter_idx_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = add i64 {}, 1\n", next_idx, curr_idx));
        ir.push_str(&format!("  store i64 {}, i64* {}\n", next_idx, idx_var));
        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

        ir.push_str(&format!("{}:\n", exit_lbl));
        true
    }

    fn emit_typed_print_arg(
        &self,
        arg_expr: &str,
        print_fn: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) {
        let arg_trim = arg_expr.trim();
        if Self::is_string_expr(arg_trim, scope_tree)
            || scope_tree
                .resolve(arg_trim)
                .map(|b| is_string_type_hint(&b.type_hint) || b.type_hint.contains("omni_unpack"))
                .unwrap_or(false)
        {
            let str_ptr = self.eval_str_ptr(arg_trim, ir, scope_tree, string_constants, temp_counter);
            ir.push_str(&format!("  call void @{}(i8* {})\n", print_fn, str_ptr));
            return;
        }

        let tv = self.eval_typed(arg_trim, ir, scope_tree, string_constants, temp_counter);
        match tv.ty {
            IrType::F64 => {
                if print_fn == "__omni_print_str" {
                    ir.push_str(&format!("  call void @__omni_print_float(double {})\n", tv.reg));
                } else {
                    let fstr = format!("%print_fstr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_float_to_str(double {})\n", fstr, tv.reg));
                    ir.push_str(&format!("  call void @{}(i8* {})\n", print_fn, fstr));
                }
            }
            IrType::Ptr => {
                ir.push_str(&format!("  call void @{}(i8* {})\n", print_fn, tv.reg));
            }
            _ => {
                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                if print_fn == "__omni_print_str" {
                    ir.push_str(&format!("  call void @__omni_print_int(i64 {})\n", v64));
                } else {
                    let istr = format!("%print_istr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_int_to_str(i64 {})\n", istr, v64));
                    ir.push_str(&format!("  call void @{}(i8* {})\n", print_fn, istr));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_statements(
        &self,
        stmts: &[Statement],
        ir: &mut String,
        scope_tree: &mut ScopeTree,
        deferred_frees: &mut Vec<CleanupOp>,
        string_constants: &[(String, String, usize)],
        cur_str_idx: &mut usize,
        lbl_counter: &mut usize,
        temp_counter: &mut usize,
        fn_name: &str,
        struct_defs: &[String],
        temp: f32,
        ecosystem_context: Option<&str>,
        import_chain: &[String],
        dynamic_declarations: &mut HashSet<String>,
        is_tail_position: bool,
    ) -> anyhow::Result<bool> {
        let mut custom_return = false;

        for (stmt_idx, stmt) in stmts.iter().enumerate() {
            let is_last_stmt = stmt_idx + 1 == stmts.len();
            match stmt {
                // 1. Variable Assignment or Scoped Declaration: `x = 5`, `count := 0`
                Statement::Assign {
                    target,
                    expr,
                    idiom: _,
                    is_scoped_decl,
                } => {
                    let trimmed_expr = expr.trim();

                    // Dotted or implicit self field assignment: `self.x = expr`, `current.next = expr`, or `root = expr`
                    let (eff_rec, eff_fld) = if target.contains('.') {
                        let parts: Vec<&str> = target.split('.').collect();
                        (parts[0].trim(), parts[1].trim())
                    } else if !*is_scoped_decl && scope_tree.resolve(target).is_none() {
                        if let Some(self_b) = scope_tree.resolve("self") {
                            if self_b.type_hint.starts_with("%struct.") {
                                let cls_name = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                if scope_tree.class_fields.get(cls_name).and_then(|m| m.get(target.as_str())).is_some() {
                                    ("self", target.as_str())
                                } else {
                                    ("", "")
                                }
                            } else {
                                ("", "")
                            }
                        } else {
                            ("", "")
                        }
                    } else {
                        ("", "")
                    };

                    if !eff_rec.is_empty() {
                        let rec = eff_rec;
                        let fld = eff_fld;
                        if let Some(b) = scope_tree.resolve(rec).cloned() {
                            if b.type_hint.starts_with("%struct.") {
                                let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                let syn_fld = fld;
                                if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(syn_fld)) {
                                    let is_struct_var = scope_tree.resolve(trimmed_expr)
                                        .map(|sb| sb.type_hint.starts_with("%struct."))
                                        .unwrap_or(false);
                                    let is_field_access = {
                                        let parts: Vec<&str> = trimmed_expr.split('.').collect();
                                        parts.len() == 2
                                            && parts[0].trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                                            && parts[1].trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                                            && parse_polyglot_f64(trimmed_expr).is_none()
                                    };
                                    let is_str_binding = scope_tree.resolve(trimmed_expr)
                                        .map(|sb| is_string_type_hint(&sb.type_hint))
                                        .unwrap_or(false);
                                    let store_val = if is_struct_var {
                                        let src_b = scope_tree.resolve(trimmed_expr).cloned().unwrap();
                                        let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*').to_string();
                                        if let Some(ft_map) = scope_tree.class_field_types.get_mut(cls_name) {
                                            ft_map.entry(syn_fld.to_string()).or_insert_with(|| format!("%struct.{}*", s_cls));
                                        }
                                        let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, src_b.llvm_ptr));
                                        let ptr64 = format!("%ptr_i64_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = ptrtoint %struct.{}* {} to i64\n", ptr64, s_cls, ld_inst));
                                        ptr64
                                    } else if is_field_access {
                                        let parts: Vec<&str> = trimmed_expr.split('.').collect();
                                        let rec_src = parts[0].trim();
                                        let fld_src = parts[1].trim();
                                        if let Some(src_b) = scope_tree.resolve(rec_src) {
                                            if src_b.type_hint.starts_with("%struct.") {
                                                let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                                let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(fld_src)).copied().unwrap_or(0);
                                                let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                                *temp_counter += 1;
                                                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, src_b.llvm_ptr));
                                                let gep_src = format!("%fld_gep_{}", *temp_counter);
                                                *temp_counter += 1;
                                                ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep_src, s_cls, s_cls, ld_inst, f_idx_src));
                                                let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                                *temp_counter += 1;
                                                ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, gep_src));
                                                ld64
                                            } else {
                                                let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                                self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                                            }
                                        } else {
                                            let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                            self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                                        }
                                    } else if scope_tree.resolve(trimmed_expr).is_none()
                                        && scope_tree
                                            .resolve("self")
                                            .map(|b| {
                                                let sc = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                                scope_tree.class_fields.get(sc).map(|m| m.contains_key(trimmed_expr)).unwrap_or(false)
                                            })
                                            .unwrap_or(false)
                                    {
                                        let self_b = scope_tree.resolve("self").unwrap();
                                        let s_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                        let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(trimmed_expr)).copied().unwrap_or(0);
                                        let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, self_b.llvm_ptr));
                                        let gep_src = format!("%fld_gep_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep_src, s_cls, s_cls, ld_inst, f_idx_src));
                                        let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, gep_src));
                                        ld64
                                    } else if is_null_literal(trimmed_expr) {
                                        "0".to_string()
                                    } else if is_str_binding {
                                        let src_b = scope_tree.resolve(trimmed_expr).cloned().unwrap();
                                        if let Some(ft_map) = scope_tree.class_field_types.get_mut(cls_name) {
                                            ft_map.entry(syn_fld.to_string()).or_insert_with(|| "i8* (string)".to_string());
                                        }
                                        if src_b.type_hint.starts_with("i8*") {
                                            let ldp = format!("%str_ldp_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ldp, src_b.llvm_ptr));
                                            let p64 = format!("%str_p64_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", p64, ldp));
                                            p64
                                        } else {
                                            let ld64 = format!("%str_ld64_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, src_b.llvm_ptr));
                                            ld64
                                        }
                                    } else {
                                        let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                        let inferred_ft = if tv.ty == IrType::F64 {
                                            Some("f64".to_string())
                                        } else {
                                            let tag = Self::infer_elem_tag_from_expr(trimmed_expr, &tv.ty, scope_tree);
                                            if tag != "i64" && tag != "i8*" {
                                                Some(tag)
                                            } else {
                                                None
                                            }
                                        };
                                        if let Some(ft) = inferred_ft {
                                            if let Some(ft_map) = scope_tree.class_field_types.get_mut(cls_name) {
                                                let should_set = ft_map
                                                    .get(syn_fld)
                                                    .map(|ex| ex == "i64" || ex == "any")
                                                    .unwrap_or(true);
                                                if should_set {
                                                    ft_map.insert(syn_fld.to_string(), ft);
                                                }
                                            }
                                        }
                                        self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                                    };
                                    let inst_ld = format!("%inst_fld_ld_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                                    let gep = format!("%fld_gep_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                                    ir.push_str(&format!("  store i64 {}, i64* {}\n", store_val, gep));
                                    continue;
                                }
                            } else if b.type_hint.contains("omni_map") {
                                // Universal JS/Python-style object/map property assignment (`foo.name = val`)
                                let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                let elem_tag = Self::infer_elem_tag_from_expr(trimmed_expr, &tv.ty, scope_tree);
                                let vkind = Self::elem_tag_to_map_kind(&elem_tag);
                                let v64 = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                                let m_ld = format!("%map_ps_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", m_ld, b.llvm_ptr));
                                let kp = self.eval_str_ptr(&format!("\"{}\"", fld), ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                    m_ld, kp, v64, vkind
                                ));
                                scope_tree.map_key_types.insert((rec.to_string(), fld.to_string()), elem_tag);
                                continue;
                            }
                        }
                    }

                    // Universal struct pointer variable assignment (`cur = self.root`, `cur = cur.next`, `prev = cur`, `prev = None`)
                    let inferred_rhs_struct: Option<String> = if let Some(b) = scope_tree.resolve(trimmed_expr) {
                        b.type_hint
                            .strip_prefix("%struct.")
                            .map(|s| s.trim_end_matches('*').to_string())
                    } else if let Some((rec_src, fld_src)) = trimmed_expr.split_once('.') {
                        let rec_src = rec_src.trim();
                        let fld_src = fld_src.trim();
                        scope_tree
                            .resolve(rec_src)
                            .and_then(|sb| sb.type_hint.strip_prefix("%struct."))
                            .map(|s| s.trim_end_matches('*'))
                            .and_then(|s_cls| scope_tree.class_field_types.get(s_cls))
                            .and_then(|m| m.get(fld_src))
                            .and_then(|ft| ft.strip_prefix("%struct."))
                            .map(|s| s.trim_end_matches('*').to_string())
                    } else if let Some(self_b) = scope_tree.resolve("self") {
                        let s_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        scope_tree
                            .class_field_types
                            .get(s_cls)
                            .and_then(|m| m.get(trimmed_expr))
                            .and_then(|ft| ft.strip_prefix("%struct."))
                            .map(|s| s.trim_end_matches('*').to_string())
                    } else {
                        None
                    };

                    let existing_lhs_struct: Option<String> = scope_tree
                        .resolve(target)
                        .and_then(|b| b.type_hint.strip_prefix("%struct."))
                        .map(|s| s.trim_end_matches('*').to_string());

                    if let Some(struct_cls) = existing_lhs_struct.or(inferred_rhs_struct) {
                        if !trimmed_expr.starts_with("new ")
                            && !trimmed_expr.ends_with(')')
                            && !trimmed_expr.ends_with('}')
                        {
                            let elem_ty = format!("%struct.{}*", struct_cls);
                            let alloca_ptr = if *is_scoped_decl || scope_tree.resolve(target).is_none() {
                                let ptr = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", ptr, struct_cls));
                                deferred_frees.push(CleanupOp::FreeHeapPtr {
                                    slot_ptr: ptr.clone(),
                                    elem_ty: elem_ty.clone(),
                                });
                                scope_tree.insert(
                                    target.clone(),
                                    SymbolBinding {
                                        name: target.clone(),
                                        llvm_ptr: ptr.clone(),
                                        type_hint: elem_ty.clone(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                ptr
                            } else {
                                let existing_p = scope_tree.resolve(target).unwrap().llvm_ptr.clone();
                                let old_ld = format!("%old_s_ld_{}", *temp_counter);
                                let old_i8 = format!("%old_s_i8_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = load {}, {}* {}\n  {} = bitcast {} {} to i8*\n  call void @__omni_rc_release(i8* {})\n",
                                    old_ld, elem_ty, elem_ty, existing_p, old_i8, elem_ty, old_ld, old_i8
                                ));
                                existing_p
                            };

                            if is_null_literal(trimmed_expr) {
                                ir.push_str(&format!("  store %struct.{}* null, %struct.{}** {}\n", struct_cls, struct_cls, alloca_ptr));
                            } else if let Some(src_b) = scope_tree.resolve(trimmed_expr) {
                                if src_b.type_hint.starts_with("%struct.") {
                                    let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                    let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                    let raw_i8 = format!("%inst_ret_i8_{}", *temp_counter);
                                    let ret_i8 = format!("%inst_ret_ok_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = load %struct.{}*, %struct.{}** {}\n  {} = bitcast %struct.{}* {} to i8*\n  {} = call i8* @__omni_rc_retain(i8* {})\n",
                                        ld_inst, s_cls, s_cls, src_b.llvm_ptr, raw_i8, s_cls, ld_inst, ret_i8, raw_i8
                                    ));
                                    let store_inst = if s_cls == struct_cls {
                                        ld_inst
                                    } else {
                                        let bc = format!("%inst_bc_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = bitcast %struct.{}* {} to %struct.{}*\n", bc, s_cls, ld_inst, struct_cls));
                                        bc
                                    };
                                    ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", struct_cls, store_inst, struct_cls, alloca_ptr));
                                }
                            } else if let Some((rec_src, fld_src)) = trimmed_expr.split_once('.') {
                                let rec_src = rec_src.trim();
                                let fld_src = fld_src.trim();
                                if let Some(src_b) = scope_tree.resolve(rec_src) {
                                    if src_b.type_hint.starts_with("%struct.") {
                                        let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                        let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(fld_src)).copied().unwrap_or(0);
                                        let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                        let gep_src = format!("%fld_gep_{}", *temp_counter);
                                        let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                        let raw_i8 = format!("%fld_i8_{}", *temp_counter);
                                        let ret_i8 = format!("%fld_ret_i8_{}", *temp_counter);
                                        let ptr_cast = format!("%ptr_cast_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!(
                                            "  {} = load %struct.{}*, %struct.{}** {}\n  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n  {} = load i64, i64* {}\n  {} = inttoptr i64 {} to i8*\n  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = bitcast i8* {} to %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                                            ld_inst, s_cls, s_cls, src_b.llvm_ptr,
                                            gep_src, s_cls, s_cls, ld_inst, f_idx_src,
                                            ld64, gep_src,
                                            raw_i8, ld64,
                                            ret_i8, raw_i8,
                                            ptr_cast, ret_i8, struct_cls,
                                            struct_cls, ptr_cast, struct_cls, alloca_ptr
                                        ));
                                    }
                                }
                            } else if let Some(self_b) = scope_tree.resolve("self") {
                                let s_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                if let Some(&f_idx_src) = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(trimmed_expr)) {
                                    let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                    let gep_src = format!("%fld_gep_{}", *temp_counter);
                                    let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                    let raw_i8 = format!("%fld_i8_{}", *temp_counter);
                                    let ret_i8 = format!("%fld_ret_i8_{}", *temp_counter);
                                    let ptr_cast = format!("%ptr_cast_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = load %struct.{}*, %struct.{}** {}\n  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n  {} = load i64, i64* {}\n  {} = inttoptr i64 {} to i8*\n  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = bitcast i8* {} to %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                                        ld_inst, s_cls, s_cls, self_b.llvm_ptr,
                                        gep_src, s_cls, s_cls, ld_inst, f_idx_src,
                                        ld64, gep_src,
                                        raw_i8, ld64,
                                        ret_i8, raw_i8,
                                        ptr_cast, ret_i8, struct_cls,
                                        struct_cls, ptr_cast, struct_cls, alloca_ptr
                                    ));
                                } else {
                                    let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                    let raw_p = self.coerce_to_ptr(tv, ir, temp_counter);
                                    let ret_p = format!("%s_expr_ret_{}", *temp_counter);
                                    let sp = format!("%s_expr_sp_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = bitcast i8* {} to %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                                        ret_p, raw_p, sp, ret_p, struct_cls, struct_cls, sp, struct_cls, alloca_ptr
                                    ));
                                }
                            } else {
                                let tv = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                let raw_p = self.coerce_to_ptr(tv, ir, temp_counter);
                                let ret_p = format!("%s_expr_ret_{}", *temp_counter);
                                let sp = format!("%s_expr_sp_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i8* @__omni_rc_retain(i8* {})\n  {} = bitcast i8* {} to %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                                    ret_p, raw_p, sp, ret_p, struct_cls, struct_cls, sp, struct_cls, alloca_ptr
                                ));
                            }
                            continue;
                        }
                    }

                    // Class / Struct Constructor Call: `c = Counter(10)`, `u = new User<T>(...)`, or Rust `h = Adler32::new()`
                    let ctor_candidate_expr = trimmed_expr.strip_prefix("new ").unwrap_or(trimmed_expr).trim();
                    if let Some(open) = ctor_candidate_expr.find('(') {
                        if ctor_candidate_expr.ends_with(')') {
                            let raw_cls_cand = ctor_candidate_expr[..open].trim();
                            let cls_cand = raw_cls_cand.split('<').next().unwrap_or(raw_cls_cand).trim();
                            let (cls_name_opt, assoc_ctor_method) = if scope_tree.class_fields.contains_key(cls_cand) {
                                (Some(cls_cand), "__init__")
                            } else if let Some((base_cls, assoc_m)) = raw_cls_cand.split_once("::") {
                                let base_cls = base_cls.split('<').next().unwrap_or(base_cls).trim();
                                let assoc_m = assoc_m.split('<').next().unwrap_or(assoc_m).trim();
                                if scope_tree.class_fields.contains_key(base_cls)
                                    && (assoc_m == "new" || assoc_m == "default" || assoc_m.starts_with("from_"))
                                {
                                    (Some(base_cls), assoc_m)
                                } else {
                                    (None, "__init__")
                                }
                            } else {
                                (None, "__init__")
                            };
                            if let Some(cls_name) = cls_name_opt {
                                let f_count = scope_tree.class_fields.get(cls_name).map(|m| m.len()).unwrap_or(1);
                                let alloc_bytes = (f_count * 8).max(8);
                                let tid = scope_tree.class_type_ids.get(cls_name).copied().unwrap_or(0);

                                let raw_inst = format!("%raw_init_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i8* @__omni_obj_alloc(i64 {}, i64 {})\n",
                                    raw_inst, alloc_bytes, tid
                                ));

                                let inst_alloc = format!("%inst_alloc_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast i8* {} to %struct.{}*\n", inst_alloc, raw_inst, cls_name));

                                let inner = &ctor_candidate_expr[open + 1..ctor_candidate_expr.len() - 1];
                                let raw_args: Vec<String> = if inner.is_empty() {
                                    Vec::new()
                                } else {
                                    omni_parser::OmniParser::split_respecting_quotes(inner, ',')
                                };

                                self.init_struct_instance(
                                    cls_name,
                                    &raw_inst,
                                    &inst_alloc,
                                    assoc_ctor_method,
                                    &raw_args,
                                    ir,
                                    scope_tree,
                                    string_constants,
                                    temp_counter,
                                );

                                let expected_th = format!("%struct.{}*", cls_name);
                                let target_ptr = if let Some(eb) = (!*is_scoped_decl)
                                    .then(|| scope_tree.resolve(target))
                                    .flatten()
                                    .filter(|b| b.type_hint == expected_th)
                                {
                                    let old_ld = format!("%old_ctor_{}", *temp_counter);
                                    let old_i8 = format!("%old_ctor_i8_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = load {}, {}* {}\n  {} = bitcast {} {} to i8*\n  call void @__omni_rc_release(i8* {})\n",
                                        old_ld, expected_th, expected_th, eb.llvm_ptr, old_i8, expected_th, old_ld, old_i8
                                    ));
                                    eb.llvm_ptr.clone()
                                } else {
                                    let tp = format!("%var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", tp, cls_name));
                                    deferred_frees.push(CleanupOp::FreeHeapPtr {
                                        slot_ptr: tp.clone(),
                                        elem_ty: expected_th.clone(),
                                    });
                                    scope_tree.insert(target.clone(), SymbolBinding {
                                        name: target.clone(),
                                        llvm_ptr: tp.clone(),
                                        type_hint: expected_th,
                                        is_scoped: true,
                                    });
                                    tp
                                };
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                continue;
                            }
                        }
                    }

                    // Rust Struct Literal: `p = Point { x: 10, y: 20 }` (Perceus refcounted via `@__omni_obj_alloc`)
                    if let Some(open) = trimmed_expr.find('{') {
                        if trimmed_expr.ends_with('}') {
                            let struct_cand = trimmed_expr[..open].trim();
                            if scope_tree.class_fields.contains_key(struct_cand) {
                                let cls_name = struct_cand;
                                let f_count = scope_tree.class_fields.get(cls_name).map(|m| m.len()).unwrap_or(1);
                                let alloc_bytes = (f_count * 8).max(8);
                                let tid = scope_tree.class_type_ids.get(cls_name).copied().unwrap_or(0);
                                let raw_inst = format!("%raw_slit_{}_{}", target, *temp_counter);
                                let inst_alloc = format!("%inst_alloc_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i8* @__omni_obj_alloc(i64 {}, i64 {})\n  {} = bitcast i8* {} to %struct.{}*\n",
                                    raw_inst, alloc_bytes, tid, inst_alloc, raw_inst, cls_name
                                ));
                                for f_idx in 0..f_count {
                                    let zgep = format!("%zlit_gep_{}_{}", f_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n  store i64 0, i64* {}\n",
                                        zgep, cls_name, cls_name, inst_alloc, f_idx, zgep
                                    ));
                                }

                                let inner = &trimmed_expr[open + 1..trimmed_expr.len() - 1];
                                for field_part in omni_parser::OmniParser::split_respecting_quotes(inner, ',') {
                                    let fp_trim = field_part.trim();
                                    if let Some((f_name, f_val)) = fp_trim.split_once(':') {
                                        let fn_clean = f_name.trim().trim_start_matches('.');
                                        if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fn_clean)) {
                                            let ftv = self.eval_typed(f_val.trim(), ir, scope_tree, string_constants, temp_counter);
                                            if ftv.ty == IrType::F64 {
                                                if let Some(ft_map) = scope_tree.class_field_types.get_mut(cls_name) {
                                                    ft_map.entry(fn_clean.to_string()).or_insert_with(|| "f64".to_string());
                                                }
                                            }
                                            let bits = self.coerce_to_raw_i64_bits(ftv, ir, temp_counter);
                                            let gep = format!("%init_gep_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_alloc, f_idx));
                                            ir.push_str(&format!("  store i64 {}, i64* {}\n", bits, gep));
                                        }
                                    }
                                }

                                let elem_ty = format!("%struct.{}*", cls_name);
                                let target_ptr = if let Some(eb) = (!*is_scoped_decl)
                                    .then(|| scope_tree.resolve(target))
                                    .flatten()
                                    .filter(|b| b.type_hint == elem_ty)
                                {
                                    let old_ld = format!("%old_slit_{}", *temp_counter);
                                    let old_i8 = format!("%old_slit_i8_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = load {}, {}* {}\n  {} = bitcast {} {} to i8*\n  call void @__omni_rc_release(i8* {})\n",
                                        old_ld, elem_ty, elem_ty, eb.llvm_ptr, old_i8, elem_ty, old_ld, old_i8
                                    ));
                                    eb.llvm_ptr.clone()
                                } else {
                                    let tp = format!("%var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", tp, cls_name));
                                    deferred_frees.push(CleanupOp::FreeHeapPtr {
                                        slot_ptr: tp.clone(),
                                        elem_ty: elem_ty.clone(),
                                    });
                                    scope_tree.insert(target.clone(), SymbolBinding {
                                        name: target.clone(),
                                        llvm_ptr: tp.clone(),
                                        type_hint: elem_ty,
                                        is_scoped: true,
                                    });
                                    tp
                                };
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                continue;
                            }
                        }
                    }

                    // Rust Tagged Enum Variant Assignment: `s = Status::Pending` or `res = Result::Ok(100)` (Perceus refcounted via `@__omni_obj_alloc`)
                    if let Some((e_name, var_info)) = scope_tree.resolve_enum_variant(trimmed_expr) {
                        let (tag, payload_val) = var_info;
                        let raw_enum = format!("%raw_enum_{}_{}", target, *temp_counter);
                        let inst_alloc = format!("%enum_alloc_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i8* @__omni_obj_alloc(i64 16, i64 0)\n  {} = bitcast i8* {} to %enum.{}*\n",
                            raw_enum, inst_alloc, raw_enum, e_name
                        ));

                        let tag_gep = format!("%tag_gep_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 0\n", tag_gep, e_name, e_name, inst_alloc));
                        ir.push_str(&format!("  store i32 {}, i32* {}\n", tag, tag_gep));

                        let payload_gep = format!("%payload_gep_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 1\n", payload_gep, e_name, e_name, inst_alloc));

                        if let Some(payload_expr) = payload_val {
                            let ptv = self.eval_typed(&payload_expr, ir, scope_tree, string_constants, temp_counter);
                            let pbits = self.coerce_to_raw_i64_bits(ptv, ir, temp_counter);
                            ir.push_str(&format!("  store i64 {}, i64* {}\n", pbits, payload_gep));
                        } else {
                            ir.push_str(&format!("  store i64 0, i64* {}\n", payload_gep));
                        }

                        let expected_enum_th = format!("%enum.{}*", e_name);
                        let target_ptr = if let Some(eb) = (!*is_scoped_decl)
                            .then(|| scope_tree.resolve(target))
                            .flatten()
                            .filter(|b| b.type_hint == expected_enum_th)
                        {
                            let old_ld = format!("%old_enum_{}", *temp_counter);
                            let old_i8 = format!("%old_enum_i8_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = load {}, {}* {}\n  {} = bitcast {} {} to i8*\n  call void @__omni_rc_release(i8* {})\n",
                                old_ld, expected_enum_th, expected_enum_th, eb.llvm_ptr, old_i8, expected_enum_th, old_ld, old_i8
                            ));
                            eb.llvm_ptr.clone()
                        } else {
                            let tp = format!("%var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca %enum.{}*\n", tp, e_name));
                            deferred_frees.push(CleanupOp::FreeHeapPtr {
                                slot_ptr: tp.clone(),
                                elem_ty: expected_enum_th.clone(),
                            });
                            scope_tree.insert(target.clone(), SymbolBinding {
                                name: target.clone(),
                                llvm_ptr: tp.clone(),
                                type_hint: expected_enum_th,
                                is_scoped: *is_scoped_decl,
                            });
                            tp
                        };
                        ir.push_str(&format!("  store %enum.{}* {}, %enum.{}** {}\n", e_name, inst_alloc, e_name, target_ptr));
                        continue;
                    }

                    // Universal Collection Filtering: LINQ `.Where(n => ...)` or Python `list(filter(lambda x: ..., coll))`
                    let mut filter_info = None;
                    let unwrapped_filter_expr = if trimmed_expr.starts_with("list(") && trimmed_expr.ends_with(')') {
                        trimmed_expr[5..trimmed_expr.len() - 1].trim()
                    } else {
                        trimmed_expr
                    };

                    if let Some(open) = unwrapped_filter_expr.find('(') {
                        if unwrapped_filter_expr.ends_with(')') {
                            let head = unwrapped_filter_expr[..open].trim();
                            if head.ends_with(".Where") || head.ends_with(".filter") {
                                if let Some(dot_idx) = head.rfind('.') {
                                    let src_col = head[..dot_idx].trim();
                                    let inner = unwrapped_filter_expr[open + 1..unwrapped_filter_expr.len() - 1].trim();
                                    if let Some((param_part, pred_part)) = inner.split_once("=>").or_else(|| inner.split_once("->")).or_else(|| inner.split_once(':')) {
                                        let param = param_part.trim_start_matches("lambda").trim();
                                        filter_info = Some((src_col.to_string(), param.to_string(), pred_part.trim().to_string()));
                                    }
                                }
                            } else if head == "filter" || head.ends_with("filter") {
                                let inner = unwrapped_filter_expr[open + 1..unwrapped_filter_expr.len() - 1].trim_end_matches(')').trim();
                                if let Some(last_comma) = inner.rfind(',') {
                                    let lambda_part = inner[..last_comma].trim();
                                    let src_col = inner[last_comma + 1..].trim();
                                    if let Some((param_part, pred_part)) = lambda_part.split_once(':').or_else(|| lambda_part.split_once("=>")) {
                                        let param = param_part.trim_start_matches("lambda").trim();
                                        filter_info = Some((src_col.to_string(), param.to_string(), pred_part.trim().to_string()));
                                    }
                                }
                            }
                        }
                    }

                    if let Some((src_col, param, pred)) = filter_info {
                        if self.lower_collection_filter_inline(
                            target,
                            &src_col,
                            &param,
                            &pred,
                            *is_scoped_decl,
                            ir,
                            scope_tree,
                            deferred_frees,
                            temp_counter,
                            lbl_counter,
                            string_constants,
                        ) {
                            continue;
                        }
                    }

                    // Method returning slice instance: `linkedlist.asList()`
                    if trimmed_expr.ends_with(".asList()") || trimmed_expr.ends_with(".toList()") {
                        ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                        let _ = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                        let slice_ptr = format!("%slice_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", slice_ptr));
                        let target_ptr = format!("%slice_var_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca i8*\n", target_ptr));
                        ir.push_str(&format!("  store i8* {}, i8** {}\n", slice_ptr, target_ptr));
                        deferred_frees.push(CleanupOp::DestroySlice(target_ptr.clone()));
                        scope_tree.insert(
                            target.clone(),
                            SymbolBinding {
                                name: target.clone(),
                                llvm_ptr: target_ptr,
                                type_hint: "i8* (omni_slice)".to_string(),
                                is_scoped: *is_scoped_decl,
                            },
                        );
                        continue;
                    }

                    // Foreign module call where receiver is foreign ecosystem (e.g. `RandomNumberGenerator.GetInt32(0, 100)`):
                    // Only match when the ROOT Pratt AST node is `ExprAst::Call` (so compound expressions like
                    // `10 + RandomNumberGenerator.GetInt32(5, 6)` split via Pratt in `eval_typed` first!).
                    let is_root_call = OmniParser::parse_expr(trimmed_expr)
                        .ok()
                        .map(|a| matches!(a.node, ExprAst::Call { .. }))
                        .unwrap_or(false);
                    let is_foreign_call = if is_root_call {
                        if let Some(open) = trimmed_expr.find('(') {
                            if trimmed_expr.ends_with(')') {
                                let callee = trimmed_expr[..open].trim();
                                if callee.contains('.') {
                                    let parts: Vec<&str> = callee.split('.').collect();
                                    let rec = parts[0].trim();
                                    let method = parts[parts.len() - 1].trim();
                                    if scope_tree.is_natively_routed_call(rec, method, &parts, trimmed_expr) {
                                        false
                                    } else {
                                        scope_tree.resolve(rec).is_none()
                                            && !scope_tree.class_fields.contains_key(rec)
                                            && (scope_tree.has_open_ns_import
                                                || scope_tree.valid_extmod_prefixes.contains(rec))
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    let is_c_ecosystem_call = if let Some(open) = trimmed_expr.find('(') {
                        if trimmed_expr.ends_with(')') {
                            let callee = trimmed_expr[..open].trim();
                            if let Some((rec, _)) = callee.split_once('.') {
                                let rec_trim = rec.trim();
                                scope_tree.c_module_aliases.contains(rec_trim)
                                    || ecosystem_context
                                        .map(|ec| ec.contains("c (module:") && ec.contains(rec_trim))
                                        .unwrap_or(false)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if is_foreign_call && !is_c_ecosystem_call {
                        let target_ptr = if !*is_scoped_decl {
                            if let Some(eb) = scope_tree.resolve(target).filter(|b| b.type_hint == "i64") {
                                eb.llvm_ptr.clone()
                            } else {
                                let p = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i64\n", p));
                                scope_tree.insert(
                                    target.clone(),
                                    SymbolBinding {
                                        name: target.clone(),
                                        llvm_ptr: p.clone(),
                                        type_hint: "i64".to_string(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                p
                            }
                        } else {
                            let p = format!("%var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i64\n", p));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: p.clone(),
                                    type_hint: "i64".to_string(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            p
                        };

                        let active_symbols: Vec<SsaVariable> = scope_tree
                            .active_bindings()
                            .into_iter()
                            .map(|b| SsaVariable {
                                name: b.name,
                                llvm_name: b.llvm_ptr,
                                type_hint: b.type_hint,
                            })
                            .collect();

                        let envelope = SsaContextEnvelope {
                            target_abi: self.target_triple.clone(),
                            enclosing_fn: fn_name.to_string(),
                            return_type: if fn_name == "main" { "i32".to_string() } else { "i64".to_string() },
                            next_ssa_idx: *temp_counter,
                            active_symbols,
                            struct_definitions: struct_defs.to_vec(),
                            raw_intent: format!("{} = {}", target, trimmed_expr),
                            idiom_hint: Some("ForeignModuleAssignment".to_string()),
                            anchor_id: Some(*lbl_counter),
                            physical_source: Some(format!("{} = {}", target, trimmed_expr)),
                            concurrency_mode: None,
                            yield_expectation: Some(format!("store 64-bit scalar result to {}", target_ptr)),
                            action_directive: Some("LOWER_ASSIGNMENT".to_string()),
                            ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                            lifetime_context: None,
                            import_chain: import_chain.to_vec(),
                        };

                        let synthesized = self.engine.lower_intent(&envelope, temp)?;
                        let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                        advance_ssa_counter_past_body(&body, temp_counter);
                        for d in dyn_decls {
                            dynamic_declarations.insert(d);
                        }
                        ir.push_str(&body);
                        if !body.ends_with('\n') {
                            ir.push('\n');
                        }

                        let exec_lines: Vec<&str> = body
                            .lines()
                            .map(|l| l.trim())
                            .filter(|l| !l.is_empty() && !l.starts_with(';'))
                            .collect();
                        let already_stored = exec_lines.iter().any(|l| {
                            l.starts_with("store ")
                                && (l.contains(&format!("* {}", target_ptr)) || l.contains(&format!("*{}", target_ptr)))
                        });

                        if !already_stored {
                            if let Some(last_assign) = exec_lines.iter().rev().find(|l| l.starts_with('%') && l.contains('=')) {
                                if let Some(eq_pos) = last_assign.find('=') {
                                    let reg = last_assign[..eq_pos].trim();
                                    let rhs = last_assign[eq_pos + 1..].trim();
                                    let head = rhs.split('(').next().unwrap_or(rhs);
                                    if rhs.starts_with("call i8*") || rhs.starts_with("load i8*") || head.contains("i8*") || rhs.starts_with("inttoptr") || rhs.starts_with("getelementptr") {
                                        let cast_reg = format!("%ptr_to_i64_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", cast_reg, reg));
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", cast_reg, target_ptr));
                                    } else if (rhs.starts_with("call i64") || rhs.starts_with("load i64") || head.contains("i64")) && !rhs.contains("to i32") {
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", reg, target_ptr));
                                    } else if rhs.starts_with("call double") || rhs.starts_with("load double") || head.contains("double") {
                                        let fbits = format!("%f_to_i64_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = fptosi double {} to i64\n", fbits, reg));
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", fbits, target_ptr));
                                    } else {
                                        let sext_reg = format!("%sext_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", sext_reg, reg));
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", sext_reg, target_ptr));
                                    }
                                }
                            } else if exec_lines.is_empty() {
                                ir.push_str(&format!("  store i64 0, i64* {}\n", target_ptr));
                            }
                        }

                        // If the LLM synthesized a string-returning call (`call i8* @__omni_input`,
                        // `call i8* @__omni_str_*`, or `ptrtoint i8* ... to i64`), promote the binding
                        // to `i8* (string)` so `print(x)`, `puts(x)`, `strlen(x)`, and `x + "..."`
                        // work identically on both x86_64 desktop and Cortex-M targets!
                        let produced_str_ptr = exec_lines.iter().any(|l| {
                            l.contains("call i8* @__omni_input")
                                || l.contains("call i8* @__omni_str_")
                                || l.contains("ptrtoint i8*")
                        });
                        if produced_str_ptr {
                            let str_slot = format!("%str_var_{}_{}", target, *temp_counter);
                            let raw_ld = format!("%str_i64_{}", *temp_counter);
                            let cast_p = format!("%str_p_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = alloca i8*\n  {} = load i64, i64* {}\n  {} = inttoptr i64 {} to i8*\n  store i8* {}, i8** {}\n",
                                str_slot, raw_ld, target_ptr, cast_p, raw_ld, cast_p, str_slot
                            ));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: str_slot,
                                    type_hint: "i8* (string)".to_string(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                        }
                        continue;
                    }

                    let existing_src_binding = scope_tree.resolve(trimmed_expr).cloned();

                    if let Some(src) = existing_src_binding.filter(|b| b.type_hint.starts_with("i8*")) {
                        // Perceus refcount-safe aliasing (`b = a` increments `rc` and releases on scope exit)
                        if let Some(elem_ty) = scope_tree.collection_elem_types.get(trimmed_expr).cloned() {
                            scope_tree.collection_elem_types.insert(target.clone(), elem_ty);
                        }
                        if let Some(pdefs) = scope_tree.fn_param_defs.get(trimmed_expr).cloned() {
                            scope_tree.fn_param_defs.insert(target.clone(), pdefs);
                        }
                        if let Some(rt) = scope_tree.fn_return_types.get(trimmed_expr).cloned() {
                            scope_tree.fn_return_types.insert(target.clone(), rt);
                        }
                        let loaded = format!("%ref_load_{}", *temp_counter);
                        let retained = format!("%ref_ret_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = load i8*, i8** {}\n  {} = call i8* @__omni_rc_retain(i8* {})\n",
                            loaded, src.llvm_ptr, retained, loaded
                        ));
                        let ptr = if *is_scoped_decl || scope_tree.resolve(target).filter(|b| b.type_hint.starts_with("i8*")).is_none() {
                            let p = format!("%col_ref_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i8*\n", p));
                            deferred_frees.push(CleanupOp::ReleaseRc(p.clone()));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: p.clone(),
                                    type_hint: src.type_hint.clone(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            p
                        } else {
                            let existing_p = scope_tree.resolve(target).unwrap().llvm_ptr.clone();
                            let old_ld = format!("%ref_old_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = load i8*, i8** {}\n  call void @__omni_rc_release(i8* {})\n",
                                old_ld, existing_p, old_ld
                            ));
                            existing_p
                        };

                        ir.push_str(&format!("  store i8* {}, i8** {}\n", retained, ptr));
                    } else if Self::is_string_expr(trimmed_expr, scope_tree)
                        || trimmed_expr.starts_with("input(")
                        || trimmed_expr == "input()"
                        || (trimmed_expr.starts_with('"') && trimmed_expr.ends_with('"'))
                        || (trimmed_expr.starts_with('\'') && trimmed_expr.ends_with('\''))
                    {
                        let alloca_ptr = if *is_scoped_decl || scope_tree.resolve(target).filter(|b| b.type_hint.starts_with("i8*")).is_none() {
                            let ptr = format!("%str_var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                            deferred_frees.push(CleanupOp::ReleaseRc(ptr.clone()));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: ptr.clone(),
                                    type_hint: "i8* (string)".to_string(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            ptr
                        } else {
                            scope_tree.resolve(target).unwrap().llvm_ptr.clone()
                        };

                        let str_val = self.eval_str_ptr(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!("  store i8* {}, i8** {}\n", str_val, alloca_ptr));
                    } else {
                        let is_fn_ref = !trimmed_expr.contains('(') && trimmed_expr.rsplit('.').next().map(|s| scope_tree.known_functions.contains(s.trim())).unwrap_or(false);
                        if is_fn_ref && scope_tree.resolve(trimmed_expr).is_none() {
                            let ptr = format!("%fn_var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: ptr.clone(),
                                    type_hint: "i8* (omni_fn)".to_string(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
                            let fptr = self.coerce_to_ptr(tv, ir, temp_counter);
                            ir.push_str(&format!("  store i8* {}, i8** {}\n", fptr, ptr));
                            continue;
                        }

                        ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                        let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
                        let existing_target = if !*is_scoped_decl {
                            scope_tree.resolve(target).cloned()
                        } else {
                            None
                        };

                        match tv.ty {
                            IrType::F64 => {
                                let alloca_ptr = if let Some(eb) = existing_target.filter(|b| b.type_hint == "double") {
                                    eb.llvm_ptr
                                } else {
                                    let ptr = format!("%flt_var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca double\n", ptr));
                                    scope_tree.insert(
                                        target.clone(),
                                        SymbolBinding {
                                            name: target.clone(),
                                            llvm_ptr: ptr.clone(),
                                            type_hint: "double".to_string(),
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    ptr
                                };
                                ir.push_str(&format!("  store double {}, double* {}\n", tv.reg, alloca_ptr));
                            }
                            IrType::StructPtr(ref cls) => {
                                let expected_th = format!("%struct.{}*", cls);
                                let alloca_ptr = if let Some(eb) = existing_target.filter(|b| b.type_hint == expected_th) {
                                    eb.llvm_ptr
                                } else {
                                    let ptr = format!("%sptr_var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", ptr, cls));
                                    scope_tree.insert(
                                        target.clone(),
                                        SymbolBinding {
                                            name: target.clone(),
                                            llvm_ptr: ptr.clone(),
                                            type_hint: expected_th,
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    ptr
                                };
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls, tv.reg, cls, alloca_ptr));
                            }
                            IrType::Ptr => {
                                if is_null_literal(trimmed_expr)
                                    && existing_target.as_ref().map(|b| b.type_hint == "i64").unwrap_or(true)
                                {
                                    // Untyped `x = None / null / nil / undefined` -> uniform `OMNI_NULL_SENTINEL` in `i64` slot
                                    // so `x is None` is distinct from `x == 0` / `x == false` and later `x = 42` stays valid.
                                    let alloca_ptr = if let Some(eb) = existing_target {
                                        eb.llvm_ptr
                                    } else {
                                        let ptr = format!("%var_{}_{}", target, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = alloca i64\n", ptr));
                                        scope_tree.insert(
                                            target.clone(),
                                            SymbolBinding {
                                                name: target.clone(),
                                                llvm_ptr: ptr.clone(),
                                                type_hint: "i64".to_string(),
                                                is_scoped: *is_scoped_decl,
                                            },
                                        );
                                        ptr
                                    };
                                    ir.push_str(&format!("  store i64 {}, i64* {}\n", OMNI_NULL_SENTINEL, alloca_ptr));
                                    continue;
                                }
                                if let Some((lam_ps, lam_body)) = OmniParser::try_parse_lambda_expr(trimmed_expr) {
                                    scope_tree.fn_param_defs.insert(target.clone(), lam_ps.clone());
                                    let mut lmap = Self::build_param_ir_types(&lam_ps, &scope_tree.class_fields);
                                    let inferred_rt = if lam_body.starts_with('{') && lam_body.ends_with('}') {
                                        let stmts = OmniParser::new().parse_module("lam_rt", &lam_body[1..lam_body.len() - 1]).top_level_statements;
                                        let mut sc = None;
                                        let mut tp = None;
                                        Self::infer_return_signature_with_locals(&stmts, &scope_tree.class_fields, None, &mut lmap, &mut sc, &mut tp);
                                        sc
                                    } else {
                                        Self::infer_expr_return_ir_type_with_locals(&lam_body, &scope_tree.class_fields, None, Some(&lmap))
                                    };
                                    if let Some(rt) = inferred_rt {
                                        scope_tree.fn_return_types.insert(target.clone(), rt);
                                    }
                                } else if let Some(open_p) = trimmed_expr.find('(') {
                                    let head = trimmed_expr[..open_p].trim();
                                    if let Some(crt) = scope_tree.fn_curried_return_types.get(head).cloned() {
                                        scope_tree.fn_return_types.insert(target.clone(), crt);
                                    }
                                }
                                let call_head_ptr_kind = trimmed_expr.find('(').and_then(|op| {
                                    if trimmed_expr.ends_with(')') {
                                        let h = trimmed_expr[..op].trim();
                                        if h == "divmod" {
                                            Some("i8* (omni_slice)".to_string())
                                        } else {
                                            scope_tree.fn_return_ptr_kinds.get(h).cloned()
                                        }
                                    } else {
                                        None
                                    }
                                });
                                let (ptr_hint, nested_elem) = if OmniParser::try_parse_lambda_expr(trimmed_expr).is_some() {
                                    ("i8* (omni_fn)".to_string(), None)
                                } else if let Some(chk) = call_head_ptr_kind {
                                    (chk, None)
                                } else if trimmed_expr.starts_with('[') && trimmed_expr.ends_with(']') {
                                    let tag = Self::infer_elem_tag_from_expr(trimmed_expr, &tv.ty, scope_tree);
                                    let sub = tag.strip_prefix("omni_slice:").map(|s| s.to_string());
                                    ("i8* (omni_slice)".to_string(), sub)
                                } else if trimmed_expr.starts_with('{') && trimmed_expr.ends_with('}') {
                                    ("i8* (omni_map)".to_string(), None)
                                } else if trimmed_expr.contains(".split(")
                                    || trimmed_expr.ends_with(".chars()")
                                    || trimmed_expr.ends_with(".toCharArray()")
                                    || trimmed_expr.ends_with(".keys()")
                                    || (trimmed_expr.starts_with("list(") && trimmed_expr.ends_with(')'))
                                {
                                    ("i8* (omni_slice)".to_string(), Some("str".to_string()))
                                } else if trimmed_expr.ends_with(".values()") {
                                    ("i8* (omni_slice)".to_string(), None)
                                } else if let Ok(ast) = OmniParser::parse_expr(trimmed_expr) {
                                    if let ExprAst::Slice { ref target, .. } = ast.node {
                                        let ts = target.slice(trimmed_expr).trim();
                                        if Self::is_string_expr(ts, scope_tree) {
                                            ("i8* (string)".to_string(), None)
                                        } else {
                                            let sub = scope_tree.collection_elem_types.get(ts).cloned();
                                            ("i8* (omni_slice)".to_string(), sub)
                                        }
                                    } else {
                                        let (is_inner_map, sub_opt) = Self::resolve_collection_target_info(trimmed_expr, scope_tree);
                                        if let Some(sub) = sub_opt {
                                            let h = if is_inner_map { "i8* (omni_map)".to_string() } else { "i8* (omni_slice)".to_string() };
                                            (h, Some(sub))
                                        } else {
                                            ("i8* (string)".to_string(), None)
                                        }
                                    }
                                } else {
                                    let (is_inner_map, sub_opt) = Self::resolve_collection_target_info(trimmed_expr, scope_tree);
                                    if let Some(sub) = sub_opt {
                                        let h = if is_inner_map {
                                            "i8* (omni_map)".to_string()
                                        } else {
                                            "i8* (omni_slice)".to_string()
                                        };
                                        (h, Some(sub))
                                    } else {
                                        ("i8* (string)".to_string(), None)
                                    }
                                };
                                if let Some(sub_ty) = nested_elem {
                                    scope_tree.collection_elem_types.insert(target.clone(), sub_ty);
                                }
                                let alloca_ptr = if let Some(eb) = existing_target.filter(|b| b.type_hint.starts_with("i8*")) {
                                    let old_ld = format!("%ptr_old_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = load i8*, i8** {}\n  call void @__omni_rc_release(i8* {})\n",
                                        old_ld, eb.llvm_ptr, old_ld
                                    ));
                                    eb.llvm_ptr
                                } else {
                                    let ptr = format!("%ptr_var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                                    deferred_frees.push(CleanupOp::ReleaseRc(ptr.clone()));
                                    scope_tree.insert(
                                        target.clone(),
                                        SymbolBinding {
                                            name: target.clone(),
                                            llvm_ptr: ptr.clone(),
                                            type_hint: ptr_hint,
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    ptr
                                };
                                ir.push_str(&format!("  store i8* {}, i8** {}\n", tv.reg, alloca_ptr));
                            }
                            IrType::I64 | IrType::I32 | IrType::I1 => {
                                if let Some(eb) = existing_target {
                                    if eb.type_hint == "double" {
                                        let fv = self.coerce_to_f64(tv, ir, temp_counter);
                                        ir.push_str(&format!("  store double {}, double* {}\n", fv, eb.llvm_ptr));
                                        continue;
                                    } else if eb.type_hint.starts_with("i8*") {
                                        let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                                        let tramp = format!("%assign_tramp_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", tramp));
                                        let is_null = format!("%assign_is_null_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = icmp eq i8* {}, null\n", is_null, tramp));
                                        let fallback = format!("%assign_fallback_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", fallback, v64));
                                        let final_ptr = format!("%assign_ptr_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = select i1 {}, i8* {}, i8* {}\n", final_ptr, is_null, fallback, tramp));
                                        ir.push_str(&format!("  store i8* {}, i8** {}\n", final_ptr, eb.llvm_ptr));
                                        continue;
                                    } else if eb.type_hint == "i32" {
                                        let v32 = self.coerce_to_i32(tv, ir, temp_counter);
                                        ir.push_str(&format!("  store i32 {}, i32* {}\n", v32, eb.llvm_ptr));
                                        continue;
                                    } else if eb.type_hint == "i64" {
                                        let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", v64, eb.llvm_ptr));
                                        continue;
                                    }
                                }

                                let v64 = self.coerce_to_i64(tv, ir, temp_counter);
                                let ptr = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i64\n", ptr));
                                scope_tree.insert(
                                    target.clone(),
                                    SymbolBinding {
                                        name: target.clone(),
                                        llvm_ptr: ptr.clone(),
                                        type_hint: "i64".to_string(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", v64, ptr));
                            }
                        }
                    }
                }

                // Pattern Matching: `match expr { ... }`
                Statement::Match { target, arms } => {
                    let match_id = *lbl_counter;
                    *lbl_counter += 1;
                    let exit_lbl = format!("match_{}_exit", match_id);
                    let default_lbl = format!("match_{}_default", match_id);

                    if let Some(b) = scope_tree.resolve(target).cloned() {
                        if b.type_hint.starts_with("%enum.") {
                            let e_name = b.type_hint.trim_start_matches("%enum.").trim_end_matches('*').to_string();
                            let inst_ld = format!("%match_inst_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %enum.{}*, %enum.{}** {}\n", inst_ld, e_name, e_name, b.llvm_ptr));
                            let tag_gep = format!("%match_tag_gep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 0\n", tag_gep, e_name, e_name, inst_ld));
                            let tag_val = format!("%match_tag_val_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i32, i32* {}\n", tag_val, tag_gep));

                            let mut switch_cases = Vec::new();
                            for (arm_idx, arm) in arms.iter().enumerate() {
                                let arm_lbl = format!("match_{}_arm_{}", match_id, arm_idx);
                                let tag_num = scope_tree.resolve_pattern_tag(&e_name, &arm.pattern).unwrap_or(arm_idx);
                                switch_cases.push(format!("i32 {}, label %{}", tag_num, arm_lbl));
                            }
                            ir.push_str(&format!("  switch i32 {}, label %{} [\n", tag_val, default_lbl));
                            for c in &switch_cases {
                                ir.push_str(&format!("    {}\n", c));
                            }
                            ir.push_str("  ]\n");

                            for (arm_idx, arm) in arms.iter().enumerate() {
                                let arm_lbl = format!("match_{}_arm_{}", match_id, arm_idx);
                                ir.push_str(&format!("\n{}:\n", arm_lbl));

                                if let Some(bind_var) = Self::extract_pattern_binding(&arm.pattern) {
                                    let p_gep = format!("%arm_{}_p_gep_{}", arm_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 1\n", p_gep, e_name, e_name, inst_ld));
                                    let p_val64 = format!("%arm_{}_pval64_{}", arm_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i64, i64* {}\n", p_val64, p_gep));
                                    let p_var_ptr = format!("%var_{}_{}", bind_var, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca i64\n", p_var_ptr));
                                    ir.push_str(&format!("  store i64 {}, i64* {}\n", p_val64, p_var_ptr));
                                    scope_tree.insert(bind_var.clone(), SymbolBinding {
                                        name: bind_var,
                                        llvm_ptr: p_var_ptr,
                                        type_hint: "i64".to_string(),
                                        is_scoped: true,
                                    });
                                }

                                let arm_ret = self.lower_statements(
                                    &arm.body,
                                    ir,
                                    scope_tree,
                                    deferred_frees,
                                    string_constants,
                                    cur_str_idx,
                                    lbl_counter,
                                    temp_counter,
                                    fn_name,
                                    struct_defs,
                                    temp,
                                    ecosystem_context,
                                    import_chain,
                                    dynamic_declarations,
                                    is_tail_position && is_last_stmt,
                                )?;
                                if !arm_ret {
                                    ir.push_str(&format!("  br label %{}\n", exit_lbl));
                                }
                            }

                            ir.push_str(&format!("\n{}:\n  br label %{}\n", default_lbl, exit_lbl));
                            ir.push_str(&format!("\n{}:\n", exit_lbl));
                            continue;
                        }
                    }
                }

                // 2. If Statement with optional Else
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let then_lbl = format!("if_then_{}", id);
                    let else_lbl = format!("if_else_{}", id);
                    let merge_lbl = format!("if_merge_{}", id);

                    let cond_var = self.eval_cond(condition, ir, scope_tree, string_constants, temp_counter);

                    if else_branch.is_some() {
                        ir.push_str(&format!(
                            "  br i1 {}, label %{}, label %{}\n\n",
                            cond_var, then_lbl, else_lbl
                        ));
                    } else {
                        ir.push_str(&format!(
                            "  br i1 {}, label %{}, label %{}\n\n",
                            cond_var, then_lbl, merge_lbl
                        ));
                    }

                    let branch_is_tail = is_tail_position && is_last_stmt && else_branch.is_some();
                    // Then branch
                    ir.push_str(&format!("{}:\n", then_lbl));
                    scope_tree.enter_scope();
                    let then_snapshot = deferred_frees.len();
                    let then_ret = self.lower_statements(
                        then_branch,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                        branch_is_tail,
                    )?;
                    if !then_ret {
                        if let Some(slice) = deferred_frees.get(then_snapshot..) {
                            Self::emit_cleanups(ir, slice, temp_counter, "Then Branch Exit");
                        }
                    }
                    deferred_frees.truncate(then_snapshot);
                    scope_tree.exit_scope();
                    if !then_ret {
                        ir.push_str(&format!("  br label %{}\n\n", merge_lbl));
                    }

                    // Else branch
                    if let Some(eb) = else_branch {
                        ir.push_str(&format!("{}:\n", else_lbl));
                        scope_tree.enter_scope();
                        let else_snapshot = deferred_frees.len();
                        let else_ret = self.lower_statements(
                            eb,
                            ir,
                            scope_tree,
                            deferred_frees,
                            string_constants,
                            cur_str_idx,
                            lbl_counter,
                            temp_counter,
                            fn_name,
                            struct_defs,
                            temp,
                            ecosystem_context,
                            import_chain,
                            dynamic_declarations,
                            branch_is_tail,
                        )?;
                        if !else_ret {
                            if let Some(slice) = deferred_frees.get(else_snapshot..) {
                                Self::emit_cleanups(ir, slice, temp_counter, "Else Branch Exit");
                            }
                        }
                        deferred_frees.truncate(else_snapshot);
                        scope_tree.exit_scope();
                        if !else_ret {
                            ir.push_str(&format!("  br label %{}\n\n", merge_lbl));
                        }
                    }

                    ir.push_str(&format!("{}:\n", merge_lbl));
                }

                // 3. While Loop
                Statement::WhileLoop { condition, body } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let cond_lbl = format!("while_cond_{}", id);
                    let body_lbl = format!("while_body_{}", id);
                    let exit_lbl = format!("while_exit_{}", id);

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    ir.push_str(&format!("{}:\n", cond_lbl));

                    let cond_var = self.eval_cond(condition, ir, scope_tree, string_constants, temp_counter);
                    ir.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n\n",
                        cond_var, body_lbl, exit_lbl
                    ));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    scope_tree.enter_scope();
                    scope_tree.loop_exit_labels.push(exit_lbl.clone());
                    scope_tree.loop_cond_labels.push(cond_lbl.clone());
                    scope_tree.loop_entry_try_depths.push(scope_tree.active_try_depth);
                    let while_snapshot = deferred_frees.len();
                    scope_tree.loop_cleanup_marks.push(while_snapshot);
                    let body_ret = self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                        false,
                    )?;
                    scope_tree.loop_cleanup_marks.pop();
                    scope_tree.loop_exit_labels.pop();
                    scope_tree.loop_cond_labels.pop();
                    scope_tree.loop_entry_try_depths.pop();
                    if !body_ret {
                        if let Some(slice) = deferred_frees.get(while_snapshot..) {
                            Self::emit_cleanups(ir, slice, temp_counter, "While Iteration Exit");
                        }
                    }
                    deferred_frees.truncate(while_snapshot);
                    scope_tree.exit_scope();
                    if !body_ret {
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    }

                    ir.push_str(&format!("{}:\n", exit_lbl));
                }

                // 4. For Loop (64-bit counter OR typed slice value iteration `for val in items:`)
                Statement::ForLoop { var, count, body } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let cond_lbl = format!("for_cond_{}", id);
                    let body_lbl = format!("for_body_{}", id);
                    let inc_lbl = format!("for_inc_{}", id);
                    let exit_lbl = format!("for_exit_{}", id);

                    let count_trim = count.trim();
                    let slice_iter_info: Option<(Option<SymbolBinding>, Option<String>)> = if !count_trim.contains("..")
                        && !count_trim.starts_with("len(")
                        && !count_trim.starts_with("range(")
                    {
                        if let Some(sb) = scope_tree.resolve(count_trim).filter(|b| b.type_hint.contains("omni_slice")).cloned() {
                            let etag = scope_tree.collection_elem_types.get(count_trim).cloned();
                            Some((Some(sb), etag))
                        } else if Self::is_string_expr(count_trim, scope_tree) {
                            Some((None, Some("str".to_string())))
                        } else {
                            let (is_map, etag_opt) = Self::resolve_collection_target_info(count_trim, scope_tree);
                            if !is_map && etag_opt.is_some() {
                                Some((None, etag_opt))
                            } else {
                                None
                            }
                        }
                    } else {
                        None
                    };

                    if let Some((sb_opt, elem_tag)) = slice_iter_info {
                        let slice_ld = if let Some(sb) = sb_opt {
                            let ld = format!("%for_slice_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, sb.llvm_ptr));
                            ld
                        } else if elem_tag.as_deref() == Some("str") && Self::is_string_expr(count_trim, scope_tree) {
                            self.eval_str_ptr(count_trim, ir, scope_tree, string_constants, temp_counter)
                        } else {
                            let stv = self.eval_typed(count_trim, ir, scope_tree, string_constants, temp_counter);
                            self.coerce_to_ptr(stv, ir, temp_counter)
                        };
                        let slice_len = format!("%for_slice_len_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", slice_len, slice_ld));

                        let idx_ptr = format!("%for_idx_{}_{}", id, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca i64\n", idx_ptr));
                        ir.push_str(&format!("  store i64 0, i64* {}\n", idx_ptr));

                        scope_tree.enter_scope();
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                        ir.push_str(&format!("{}:\n", cond_lbl));

                        let curr_val = format!("%curr_{}", id);
                        ir.push_str(&format!("  {} = load i64, i64* {}\n", curr_val, idx_ptr));
                        let cmp_var = format!("%cmp_{}", id);
                        ir.push_str(&format!("  {} = icmp slt i64 {}, {}\n", cmp_var, curr_val, slice_len));
                        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cmp_var, body_lbl, exit_lbl));

                        ir.push_str(&format!("{}:\n", body_lbl));
                        let raw_elem = format!("%for_raw_elem_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n", raw_elem, slice_ld, curr_val));
                        let elem_tv = Self::unpack_raw_elem_bits(raw_elem, elem_tag.as_deref(), ir, temp_counter);
                        match elem_tv.ty {
                            IrType::F64 => {
                                let vptr = format!("%for_flt_{}_{}", var, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca double\n", vptr));
                                ir.push_str(&format!("  store double {}, double* {}\n", elem_tv.reg, vptr));
                                scope_tree.insert(var.clone(), SymbolBinding {
                                    name: var.clone(),
                                    llvm_ptr: vptr,
                                    type_hint: "double".to_string(),
                                    is_scoped: true,
                                });
                            }
                            IrType::StructPtr(cls) => {
                                let vptr = format!("%for_sptr_{}_{}", var, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", vptr, cls));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls, elem_tv.reg, cls, vptr));
                                scope_tree.insert(var.clone(), SymbolBinding {
                                    name: var.clone(),
                                    llvm_ptr: vptr,
                                    type_hint: format!("%struct.{}*", cls),
                                    is_scoped: true,
                                });
                            }
                            IrType::Ptr => {
                                let vptr = format!("%for_ptr_{}_{}", var, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i8*\n", vptr));
                                ir.push_str(&format!("  store i8* {}, i8** {}\n", elem_tv.reg, vptr));
                                let th = if let Some(ref et) = elem_tag {
                                    if let Some(sub) = et.strip_prefix("omni_slice:") {
                                        scope_tree.collection_elem_types.insert(var.clone(), sub.to_string());
                                        "i8* (omni_slice)".to_string()
                                    } else if let Some(sub) = et.strip_prefix("omni_map:") {
                                        scope_tree.collection_elem_types.insert(var.clone(), sub.to_string());
                                        "i8* (omni_map)".to_string()
                                    } else {
                                        "i8* (string)".to_string()
                                    }
                                } else {
                                    "i8* (string)".to_string()
                                };
                                scope_tree.insert(var.clone(), SymbolBinding {
                                    name: var.clone(),
                                    llvm_ptr: vptr,
                                    type_hint: th,
                                    is_scoped: true,
                                });
                            }
                            _ => {
                                let v64 = self.coerce_to_i64(elem_tv, ir, temp_counter);
                                let vptr = format!("%for_var_{}_{}", var, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i64\n", vptr));
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", v64, vptr));
                                scope_tree.insert(var.clone(), SymbolBinding {
                                    name: var.clone(),
                                    llvm_ptr: vptr,
                                    type_hint: "i64".to_string(),
                                    is_scoped: true,
                                });
                            }
                        }

                        scope_tree.loop_exit_labels.push(exit_lbl.clone());
                        scope_tree.loop_cond_labels.push(inc_lbl.clone());
                        scope_tree.loop_entry_try_depths.push(scope_tree.active_try_depth);
                        let for_snapshot = deferred_frees.len();
                        scope_tree.loop_cleanup_marks.push(for_snapshot);
                        let body_ret = self.lower_statements(
                            body,
                            ir,
                            scope_tree,
                            deferred_frees,
                            string_constants,
                            cur_str_idx,
                            lbl_counter,
                            temp_counter,
                            fn_name,
                            struct_defs,
                            temp,
                            ecosystem_context,
                            import_chain,
                            dynamic_declarations,
                            false,
                        )?;
                        scope_tree.loop_cleanup_marks.pop();
                        scope_tree.loop_exit_labels.pop();
                        scope_tree.loop_cond_labels.pop();
                        scope_tree.loop_entry_try_depths.pop();
                        if !body_ret {
                            Self::emit_cleanups(ir, &deferred_frees[for_snapshot..], temp_counter, "For Iteration Exit");
                            ir.push_str(&format!("  br label %{}\n\n", inc_lbl));
                        }
                        deferred_frees.truncate(for_snapshot);

                        ir.push_str(&format!("{}:\n", inc_lbl));
                        let load_for_inc = format!("%curr_inc_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i64, i64* {}\n", load_for_inc, idx_ptr));
                        let next_val = format!("%next_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = add i64 {}, 1\n", next_val, load_for_inc));
                        ir.push_str(&format!("  store i64 {}, i64* {}\n", next_val, idx_ptr));

                        scope_tree.exit_scope();
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                        ir.push_str(&format!("{}:\n", exit_lbl));
                        continue;
                    }

                    let (start_val, limit_expr) = if let Some((s, e)) = count.split_once("..") {
                        let s_clean = s.trim();
                        let e_clean = e.trim();
                        let stv = self.eval_typed(s_clean, ir, scope_tree, string_constants, temp_counter);
                        let s_val = self.coerce_to_i64(stv, ir, temp_counter);
                        (s_val, e_clean)
                    } else {
                        ("0".to_string(), count.as_str())
                    };

                    let idx_ptr = format!("%var_{}_{}", var, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i64\n", idx_ptr));
                    ir.push_str(&format!("  store i64 {}, i64* {}\n", start_val, idx_ptr));

                    scope_tree.enter_scope();
                    scope_tree.insert(
                        var.clone(),
                        SymbolBinding {
                            name: var.clone(),
                            llvm_ptr: idx_ptr.clone(),
                            type_hint: "i64".to_string(),
                            is_scoped: true,
                        },
                    );

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    ir.push_str(&format!("{}:\n", cond_lbl));

                    let curr_val = format!("%curr_{}", id);
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", curr_val, idx_ptr));

                    let ltv = self.eval_typed(limit_expr, ir, scope_tree, string_constants, temp_counter);
                    let count_limit = self.coerce_to_i64(ltv, ir, temp_counter);
                    let cmp_var = format!("%cmp_{}", id);
                    ir.push_str(&format!(
                        "  {} = icmp slt i64 {}, {}\n",
                        cmp_var, curr_val, count_limit
                    ));
                    ir.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n\n",
                        cmp_var, body_lbl, exit_lbl
                    ));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    scope_tree.loop_exit_labels.push(exit_lbl.clone());
                    scope_tree.loop_cond_labels.push(inc_lbl.clone());
                    scope_tree.loop_entry_try_depths.push(scope_tree.active_try_depth);
                    let for_snapshot = deferred_frees.len();
                    scope_tree.loop_cleanup_marks.push(for_snapshot);
                    let body_ret = self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                        false,
                    )?;
                    scope_tree.loop_cleanup_marks.pop();
                    scope_tree.loop_exit_labels.pop();
                    scope_tree.loop_cond_labels.pop();
                    scope_tree.loop_entry_try_depths.pop();
                    if !body_ret {
                        Self::emit_cleanups(ir, &deferred_frees[for_snapshot..], temp_counter, "For Iteration Exit");
                        ir.push_str(&format!("  br label %{}\n\n", inc_lbl));
                    }
                    deferred_frees.truncate(for_snapshot);

                    ir.push_str(&format!("{}:\n", inc_lbl));
                    let load_for_inc = format!("%curr_inc_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", load_for_inc, idx_ptr));
                    let next_val = format!("%next_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = add i64 {}, 1\n", next_val, load_for_inc));
                    ir.push_str(&format!("  store i64 {}, i64* {}\n", next_val, idx_ptr));

                    scope_tree.exit_scope();

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

                    ir.push_str(&format!("{}:\n", exit_lbl));
                }

                // 5. Print Statement
                Statement::Print { expr } => {
                    let trimmed = expr.trim();

                    let clean_var = trimmed.trim_matches(|c| c == '(' || c == ')').trim();
                    if let Some(b) = scope_tree.resolve(clean_var) {
                        if b.type_hint.contains("omni_slice") {
                            let loaded = format!("%slice_print_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                            ir.push_str(&format!("  call void @__omni_slice_print(i8* {})\n", loaded));
                            continue;
                        }
                        if b.type_hint.contains("omni_map") {
                            let loaded = format!("%map_print_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                            ir.push_str(&format!("  call void @__omni_map_print(i8* {})\n", loaded));
                            continue;
                        }
                    }
                    // Runtime-tagged heterogeneous map lookup printing: `print(foo["id"])`, `print(foo["name"])`, `print(foo.name)`, `print(foo[0])`
                    if let Ok(parsed_print) = OmniParser::parse_expr(clean_var) {
                        match parsed_print.node {
                            ExprAst::Index { ref target, ref index } => {
                                let ts = target.slice(clean_var).trim();
                                let ix = index.slice(clean_var).trim();
                                if let Some(b) = scope_tree.resolve(ts) {
                                    if b.type_hint.contains("omni_map") {
                                        let loaded = format!("%map_pv_ld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                                        if (ix.starts_with('"') && ix.ends_with('"'))
                                            || (ix.starts_with('\'') && ix.ends_with('\''))
                                            || Self::is_string_expr(ix, scope_tree)
                                        {
                                            let kp = self.eval_str_ptr(ix, ir, scope_tree, string_constants, temp_counter);
                                            ir.push_str(&format!(
                                                "  call void @__omni_map_print_val(i8* {}, i8* {})\n",
                                                loaded, kp
                                            ));
                                        } else {
                                            let ktv = self.eval_typed(ix, ir, scope_tree, string_constants, temp_counter);
                                            let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                            ir.push_str(&format!(
                                                "  call void @__omni_map_print_int_val(i8* {}, i64 {})\n",
                                                loaded, k64
                                            ));
                                        }
                                        continue;
                                    }
                                }
                            }
                            ExprAst::Member { ref target, ref field }
                                if !matches!(field.as_str(), "length" | "Length" | "Count") =>
                            {
                                let ts = target.slice(clean_var).trim();
                                if let Some(b) = scope_tree.resolve(ts) {
                                    if b.type_hint.contains("omni_map") {
                                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, field) {
                                            let loaded = format!("%map_pm_ld_{}", *temp_counter);
                                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                                            ir.push_str(&format!(
                                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                                str_ptr, len, len, const_name
                                            ));
                                            ir.push_str(&format!(
                                                "  call void @__omni_map_print_val(i8* {}, i8* {})\n",
                                                loaded, str_ptr
                                            ));
                                            continue;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if clean_var.contains(".asList()") || clean_var.contains(".toList()")
                        || clean_var.ends_with(".asList") || clean_var.ends_with(".toList")
                        || trimmed.contains(".asList()") || trimmed.contains(".toList()")
                    {
                        let call_target = if clean_var.ends_with(".asList") {
                            format!("{}()", clean_var)
                        } else {
                            clean_var.to_string()
                        };
                        ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                        let _ = self.eval_typed(&call_target, ir, scope_tree, string_constants, temp_counter);
                        let slice_ptr = format!("%slice_print_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", slice_ptr));
                        ir.push_str(&format!("  call void @__omni_slice_print(i8* {})\n", slice_ptr));
                        continue;
                    }

                    let raw_args = omni_parser::OmniParser::split_respecting_quotes(trimmed, ',');
                    let mut is_inline_end = false;
                    let mut top_args = Vec::new();
                    for a in raw_args {
                        let a_trim = a.trim();
                        if let Some(rest) = a_trim.strip_prefix("end=") {
                            let val = rest.trim();
                            let unquoted = if (val.starts_with('\'') && val.ends_with('\''))
                                || (val.starts_with('"') && val.ends_with('"'))
                            {
                                &val[1..val.len() - 1]
                            } else {
                                val
                            };
                            if unquoted.is_empty() {
                                is_inline_end = true;
                            }
                        } else {
                            top_args.push(a);
                        }
                    }

                    if top_args.is_empty() {
                        if !is_inline_end {
                            let empty_str = self.eval_str_ptr("\"\"", ir, scope_tree, string_constants, temp_counter);
                            ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", empty_str));
                        }
                        continue;
                    }

                    if top_args.len() >= 2 && top_args[0].contains("{}") {
                        let fmt = top_args[0]
                            .trim()
                            .trim_start_matches('f')
                            .trim_matches('"')
                            .trim_matches('\'');
                        let prefix = fmt.replace("{}", "");
                        let arg_expr = top_args[1].trim();
                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, &prefix) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!("  call void @__omni_print_str_raw(i8* {})\n", str_ptr));
                        } else if let Some((const_name, len)) = Self::find_string_constant(string_constants, fmt) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", str_ptr));
                        }
                        self.emit_typed_print_arg(arg_expr, "__omni_print_str", ir, scope_tree, string_constants, temp_counter);
                        continue;
                    }

                    if top_args.len() > 1 && !trimmed.contains("{}") {
                        let first_arg = top_args[0].trim();
                        if (first_arg.starts_with('"') || first_arg.starts_with('\''))
                            && (first_arg.contains("%d") || first_arg.contains("%i") || first_arg.contains("%s") || first_arg.contains("%ld") || first_arg.contains("%f"))
                        {
                            let fmt_ptr = self.eval_str_ptr(first_arg, ir, scope_tree, string_constants, temp_counter);
                            let rhs_tv = self.eval_typed(top_args[1].trim(), ir, scope_tree, string_constants, temp_counter);
                            if first_arg.contains("%f") {
                                let fv = self.coerce_to_f64(rhs_tv, ir, temp_counter);
                                ir.push_str(&format!("  call i32 (i8*, ...) @printf(i8* {}, double {})\n", fmt_ptr, fv));
                            } else if first_arg.contains("%s") {
                                let pv = self.coerce_to_ptr(rhs_tv, ir, temp_counter);
                                ir.push_str(&format!("  call i32 (i8*, ...) @printf(i8* {}, i8* {})\n", fmt_ptr, pv));
                            } else if first_arg.contains("%ld") {
                                let iv = self.coerce_to_i64(rhs_tv, ir, temp_counter);
                                ir.push_str(&format!("  call i32 (i8*, ...) @printf(i8* {}, i64 {})\n", fmt_ptr, iv));
                            } else {
                                let iv32 = self.coerce_to_i32(rhs_tv, ir, temp_counter);
                                ir.push_str(&format!("  call i32 (i8*, ...) @printf(i8* {}, i32 {})\n", fmt_ptr, iv32));
                            }
                            continue;
                        }

                        for (arg_idx, arg) in top_args.iter().enumerate() {
                            let arg_trim = arg.trim();
                            let is_last = arg_idx == top_args.len() - 1;
                            let print_fn = if is_last {
                                if is_inline_end { "__omni_print_str_raw" } else { "__omni_print_str" }
                            } else {
                                "__omni_print_str_inline"
                            };

                            if let Some((cond_part, then_part, else_part)) = split_ternary_expr(arg_trim) {
                                let cond_val = self.eval_cond(cond_part, ir, scope_tree, string_constants, temp_counter);
                                let then_str = self.eval_str_ptr(then_part.trim(), ir, scope_tree, string_constants, temp_counter);
                                let else_str = self.eval_str_ptr(else_part.trim(), ir, scope_tree, string_constants, temp_counter);
                                let sel_str = format!("%tern_str_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = select i1 {}, i8* {}, i8* {}\n", sel_str, cond_val, then_str, else_str));
                                ir.push_str(&format!("  call void @{}(i8* {})\n", print_fn, sel_str));
                                continue;
                            }

                            self.emit_typed_print_arg(arg_trim, print_fn, ir, scope_tree, string_constants, temp_counter);
                        }
                        continue;
                    }

                    let single_arg = top_args[0].trim();
                    if single_arg.contains("{}") && single_arg.contains(',') {
                        if let Some(comma_pos) = single_arg.rfind(',') {
                            let fmt = single_arg[..comma_pos]
                                .trim()
                                .trim_start_matches('f')
                                .trim_matches('"')
                                .trim_matches('\'');
                            let arg_expr = single_arg[comma_pos + 1..].trim();
                            if let Some((const_name, len)) = Self::find_string_constant(string_constants, fmt) {
                                let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                    str_ptr, len, len, const_name
                                ));
                                ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", str_ptr));
                            }
                            self.emit_typed_print_arg(arg_expr, "__omni_print_str", ir, scope_tree, string_constants, temp_counter);
                        }
                    } else {
                        let print_fn = if is_inline_end { "__omni_print_str_raw" } else { "__omni_print_str" };
                        self.emit_typed_print_arg(single_arg, print_fn, ir, scope_tree, string_constants, temp_counter);
                    }
                }

                // 6. Unified Memory Operations (Alloc / Free / Defer)
                Statement::MemOp { op, is_deferred } => match op {
                    UnifiedMemOp::Alloc {
                        target,
                        size_bytes,
                        ..
                    } => {
                        let sz: u64 = size_bytes.parse().unwrap_or(64);
                        ir.push_str(&format!("  ; [UMO Inline Alloc: {} bytes]\n", sz));
                        let ptr_name = format!("%{}", target);
                        ir.push_str(&format!("  {} = call i8* @malloc(i64 {})\n", ptr_name, sz));
                        let slot_ptr = format!("%umo_slot_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca i8*\n", slot_ptr));
                        ir.push_str(&format!("  store i8* {}, i8** {}\n", ptr_name, slot_ptr));
                        scope_tree.insert(
                            target.clone(),
                            SymbolBinding {
                                name: target.clone(),
                                llvm_ptr: slot_ptr,
                                type_hint: "i8*".to_string(),
                                is_scoped: false,
                            },
                        );
                    }
                    UnifiedMemOp::Free { target } => {
                        if *is_deferred {
                            ir.push_str(&format!("  ; [UMO Registered Defer Free: %{}]\n", target));
                            deferred_frees.push(CleanupOp::Free(target.clone()));
                        } else {
                            ir.push_str(&format!("  ; [UMO Inline Free: %{}]\n", target));
                            ir.push_str(&format!("  call void @free(i8* %{})\n", target));
                        }
                    }
                    _ => {}
                },

                // 7. Return Statement (64-bit `i64` / Tuple Slice / `@main` `i32` Exit Code)
                Statement::Return { expr } => {
                    let ret_is_i32 = fn_name == "main";
                    let ret_val_i64 = if let Some(e) = expr {
                        let trimmed_e = e.trim();
                        let unwrapped_e = if trimmed_e.starts_with('(') && trimmed_e.ends_with(')') {
                            let inner = &trimmed_e[1..trimmed_e.len() - 1];
                            let mut d = 0i32;
                            let mut balanced = true;
                            for c in inner.chars() {
                                if c == '(' { d += 1; }
                                else if c == ')' {
                                    d -= 1;
                                    if d < 0 { balanced = false; break; }
                                }
                            }
                            if balanced && d == 0 { inner.trim() } else { trimmed_e }
                        } else {
                            trimmed_e
                        };
                        let tuple_parts = omni_parser::OmniParser::split_respecting_quotes(unwrapped_e, ',');
                        if tuple_parts.len() > 1 {
                            let tup_slice = format!("%ret_tuple_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i8* @__omni_slice_create(i64 {})\n",
                                tup_slice,
                                tuple_parts.len().max(4)
                            ));
                            for part in &tuple_parts {
                                let p_trim = part.trim();
                                let elem_i64 = if is_null_literal(p_trim) {
                                    "0".to_string()
                                } else {
                                    let tv = self.eval_typed(p_trim, ir, scope_tree, string_constants, temp_counter);
                                    self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                                };
                                ir.push_str(&format!(
                                    "  call void @__omni_slice_append(i8* {}, i64 {})\n",
                                    tup_slice, elem_i64
                                ));
                            }
                            ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", tup_slice));
                            let cast_i64 = format!("%ret_tup_i64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", cast_i64, tup_slice));
                            cast_i64
                        } else if let Some(b) = scope_tree.resolve(trimmed_e) {
                            if b.type_hint.contains("omni_slice")
                                || b.type_hint.contains("omni_map")
                                || b.type_hint.contains("omni_chan")
                                || b.type_hint.starts_with("i8*")
                            {
                                // Preserve returned heap/closure/collection handle across RAII cleanups
                                deferred_frees.retain(|op| match op {
                                    CleanupOp::DestroySlice(p)
                                    | CleanupOp::DestroyMap(p)
                                    | CleanupOp::DestroyChan(p)
                                    | CleanupOp::ReleaseRc(p) => p != &b.llvm_ptr,
                                    CleanupOp::FreeHeapPtr { slot_ptr, .. } => slot_ptr != &b.llvm_ptr,
                                    _ => true,
                                });
                                let loaded = format!("%ret_ptr_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, b.llvm_ptr));
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", loaded));
                                let cast_i64 = format!("%ret_ptr_i64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = ptrtoint i8* {} to i64\n", cast_i64, loaded));
                                cast_i64
                            } else if b.type_hint.starts_with("%struct.") {
                                deferred_frees.retain(|op| match op {
                                    CleanupOp::FreeHeapPtr { slot_ptr, .. } => slot_ptr != &b.llvm_ptr,
                                    _ => true,
                                });
                                let tv = self.eval_typed(trimmed_e, ir, scope_tree, string_constants, temp_counter);
                                let p8 = self.coerce_to_ptr(tv.clone(), ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p8));
                                self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                            } else if b.type_hint.starts_with("%enum.") {
                                deferred_frees.retain(|op| match op {
                                    CleanupOp::FreeHeapPtr { slot_ptr, .. } => slot_ptr != &b.llvm_ptr,
                                    _ => true,
                                });
                                let en_name = b.type_hint.trim_start_matches("%enum.").trim_end_matches('*');
                                let loaded = format!("%ret_enum_ld_{}", *temp_counter);
                                let raw_i8 = format!("%ret_enum_i8_{}", *temp_counter);
                                let cast_i64 = format!("%ret_enum_i64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = load %enum.{}*, %enum.{}** {}\n  {} = bitcast %enum.{}* {} to i8*\n  call void @__omni_c_return_ptr(i8* {})\n  {} = ptrtoint i8* {} to i64\n",
                                    loaded, en_name, en_name, b.llvm_ptr, raw_i8, en_name, loaded, raw_i8, cast_i64, raw_i8
                                ));
                                cast_i64
                            } else {
                                let tv = self.eval_typed(e, ir, scope_tree, string_constants, temp_counter);
                                self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                            }
                        } else {
                            let tv = self.eval_typed(e, ir, scope_tree, string_constants, temp_counter);
                            if matches!(tv.ty, IrType::Ptr | IrType::StructPtr(_)) {
                                let p8 = self.coerce_to_ptr(tv.clone(), ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p8));
                            }
                            self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                        }
                    } else {
                        "0".to_string()
                    };

                    // Drain active fibers/threads BEFORE main destroys local channels/collections (`[C6.5]`)
                    if fn_name == "main" && !self.target_triple.starts_with("thumb") {
                        ir.push_str("  call void @__omni_fiber_join_all()\n");
                        ir.push_str("  call void @__omni_thread_join_all()\n");
                    }
                    if !scope_tree.closure_env_writebacks.is_empty() {
                        Self::emit_closure_writebacks(ir, &scope_tree.closure_env_writebacks, temp_counter);
                    }
                    // Execute active cleanups (including popping any active `jmp_buf` try frames)
                    Self::emit_cleanups(ir, deferred_frees, temp_counter, "Return");

                    if ret_is_i32 {
                        if let Ok(n64) = ret_val_i64.parse::<i64>() {
                            ir.push_str(&format!("  ret i32 {}\n", n64 as i32));
                        } else {
                            let ret_i32 = format!("%ret_main_i32_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = trunc i64 {} to i32\n", ret_i32, ret_val_i64));
                            ir.push_str(&format!("  ret i32 {}\n", ret_i32));
                        }
                    } else {
                        ir.push_str(&format!("  ret i64 {}\n", ret_val_i64));
                    }
                    custom_return = true;
                    break;
                }

                // 8. Channel Operations
                Statement::ChanMake {
                    target,
                    capacity,
                    elem_size,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%chan_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let make_res = format!("%make_ch_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_chan_make(i64 {}, i64 {})\n",
                        make_res, capacity, elem_size
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", make_res, ptr));

                    // Register RAII cleanup for channel resource
                    deferred_frees.push(CleanupOp::DestroyChan(ptr.clone()));

                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_chan)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                Statement::ChanSend { channel, expr } => {
                    if let Some(binding) = scope_tree.resolve(channel).cloned() {
                        if binding.type_hint.contains("omni_chan") {
                            let ch_loaded = format!("%ch_load_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ch_loaded, binding.llvm_ptr));

                            let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
                            let val_64 = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);

                            let send_ok = format!("%send_ok_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i1 @__omni_chan_send_int(i8* {}, i64 {})\n",
                                send_ok, ch_loaded, val_64
                            ));
                        } else {
                            let send_expr = format!("{}.send({})", channel, expr);
                            let _ = self.eval_typed(&send_expr, ir, scope_tree, string_constants, temp_counter);
                        }
                    } else {
                        let send_expr = format!("{}.send({})", channel, expr);
                        let _ = self.eval_typed(&send_expr, ir, scope_tree, string_constants, temp_counter);
                    }
                }

                Statement::ChanRecv {
                    target,
                    channel,
                    is_scoped_decl,
                } => {
                    if let Some(binding) = scope_tree.resolve(channel) {
                        let ch_loaded = format!("%ch_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ch_loaded, binding.llvm_ptr));

                        let recv_64 = format!("%recv_64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_chan_recv_int(i8* {})\n",
                            recv_64, ch_loaded
                        ));

                        if let Some(ref var_name) = target {
                            let existing = if !*is_scoped_decl {
                                scope_tree.resolve(var_name).cloned()
                            } else {
                                None
                            };
                            if let Some(eb) = existing {
                                if eb.type_hint == "i32" {
                                    let recv_32 = format!("%recv_32_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", recv_32, recv_64));
                                    ir.push_str(&format!("  store i32 {}, i32* {}\n", recv_32, eb.llvm_ptr));
                                } else {
                                    ir.push_str(&format!("  store i64 {}, i64* {}\n", recv_64, eb.llvm_ptr));
                                }
                            } else {
                                let ptr = format!("%var_{}_{}", var_name, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i64\n", ptr));
                                scope_tree.insert(
                                    var_name.clone(),
                                    SymbolBinding {
                                        name: var_name.clone(),
                                        llvm_ptr: ptr.clone(),
                                        type_hint: "i64".to_string(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", recv_64, ptr));
                            }
                        }
                    }
                }

                // 9. Polyglot List / Slice Operations
                Statement::ListInit {
                    target,
                    elements,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%slice_var_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let initial_cap = elements.len().max(4);
                    let slice_res = format!("%slice_init_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_slice_create(i64 {})\n",
                        slice_res, initial_cap
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", slice_res, ptr));

                    let mut inferred_list_elem: Option<String> = None;

                    if elements.len() == 1 && elements[0].contains(" for ") && elements[0].contains(" in ") {
                        let comp_str = &elements[0];
                        if let Some((map_part, rest)) = comp_str.split_once(" for ") {
                            let map_expr = map_part.trim();
                            let (var_part, rest_after_var) = rest.split_once(" in ").unwrap_or((rest, ""));
                            let var_name = var_part.trim();
                            let (iter_name, cond_opt) = if let Some((it, cd)) = rest_after_var.split_once(" if ") {
                                (it.trim(), Some(cd.trim()))
                            } else {
                                (rest_after_var.trim(), None)
                            };

                            let resolved_comp_src: Option<(String, Option<String>)> =
                                if let Some(src_binding) = scope_tree.resolve(iter_name).cloned() {
                                    let src_elem_tag = if is_string_type_hint(&src_binding.type_hint) {
                                        Some("str".to_string())
                                    } else {
                                        scope_tree.collection_elem_types.get(iter_name).cloned()
                                    };
                                    let src_ld = format!("%comp_src_ld_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", src_ld, src_binding.llvm_ptr));
                                    Some((src_ld, src_elem_tag))
                                } else if Self::is_string_expr(iter_name, scope_tree) {
                                    let sp = self.eval_str_ptr(iter_name, ir, scope_tree, string_constants, temp_counter);
                                    Some((sp, Some("str".to_string())))
                                } else {
                                    let (is_map, etag_opt) = Self::resolve_collection_target_info(iter_name, scope_tree);
                                    let stv = self.eval_typed(iter_name, ir, scope_tree, string_constants, temp_counter);
                                    if !is_map && matches!(stv.ty, IrType::Ptr) {
                                        let etag = etag_opt.or_else(|| {
                                            if iter_name.contains(".split(") || iter_name.ends_with(".chars()") {
                                                Some("str".to_string())
                                            } else {
                                                None
                                            }
                                        });
                                        Some((stv.reg, etag))
                                    } else {
                                        None
                                    }
                                };

                            if let Some((src_ld, src_elem_tag)) = resolved_comp_src {

                                let src_len = format!("%comp_src_len_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", src_len, src_ld));

                                let comp_id = *lbl_counter;
                                *lbl_counter += 1;
                                let cond_lbl = format!("comp_cond_{}", comp_id);
                                let body_lbl = format!("comp_body_{}", comp_id);
                                let inc_lbl = format!("comp_inc_{}", comp_id);
                                let exit_lbl = format!("comp_exit_{}", comp_id);

                                let idx_ptr = format!("%comp_idx_{}", comp_id);
                                ir.push_str(&format!("  {} = alloca i64\n", idx_ptr));
                                ir.push_str(&format!("  store i64 0, i64* {}\n", idx_ptr));
                                ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

                                ir.push_str(&format!("{}:\n", cond_lbl));
                                let curr_idx = format!("%curr_comp_idx_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i64, i64* {}\n", curr_idx, idx_ptr));
                                let cmp = format!("%comp_cmp_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = icmp slt i64 {}, {}\n", cmp, curr_idx, src_len));
                                ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cmp, body_lbl, exit_lbl));

                                ir.push_str(&format!("{}:\n", body_lbl));
                                let elem_val = format!("%comp_elem_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n", elem_val, src_ld, curr_idx));

                                scope_tree.enter_scope();
                                let unpacked_src = Self::unpack_raw_elem_bits(elem_val, src_elem_tag.as_deref(), ir, temp_counter);
                                match unpacked_src.ty {
                                    IrType::F64 => {
                                        let param_ptr = format!("%var_flt_{}_{}", var_name, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = alloca double\n", param_ptr));
                                        ir.push_str(&format!("  store double {}, double* {}\n", unpacked_src.reg, param_ptr));
                                        scope_tree.insert(var_name.to_string(), SymbolBinding {
                                            name: var_name.to_string(),
                                            llvm_ptr: param_ptr,
                                            type_hint: "double".to_string(),
                                            is_scoped: true,
                                        });
                                    }
                                    IrType::StructPtr(cls) => {
                                        let param_ptr = format!("%var_sp_{}_{}", var_name, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = alloca %struct.{}*\n", param_ptr, cls));
                                        ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls, unpacked_src.reg, cls, param_ptr));
                                        scope_tree.insert(var_name.to_string(), SymbolBinding {
                                            name: var_name.to_string(),
                                            llvm_ptr: param_ptr,
                                            type_hint: format!("%struct.{}*", cls),
                                            is_scoped: true,
                                        });
                                    }
                                    IrType::Ptr => {
                                        let param_ptr = format!("%var_ptr_{}_{}", var_name, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = alloca i8*\n", param_ptr));
                                        ir.push_str(&format!("  store i8* {}, i8** {}\n", unpacked_src.reg, param_ptr));
                                        scope_tree.insert(var_name.to_string(), SymbolBinding {
                                            name: var_name.to_string(),
                                            llvm_ptr: param_ptr,
                                            type_hint: "i8* (string)".to_string(),
                                            is_scoped: true,
                                        });
                                    }
                                    _ => {
                                        let v64 = self.coerce_to_i64(unpacked_src, ir, temp_counter);
                                        let param_ptr = format!("%var_{}_{}", var_name, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = alloca i64\n", param_ptr));
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", v64, param_ptr));
                                        scope_tree.insert(
                                            var_name.to_string(),
                                            SymbolBinding {
                                                name: var_name.to_string(),
                                                llvm_ptr: param_ptr,
                                                type_hint: "i64".to_string(),
                                                is_scoped: true,
                                            },
                                        );
                                    }
                                }

                                let append_lbl = format!("comp_append_{}", comp_id);
                                if let Some(cond) = cond_opt {
                                    let cond_val = self.eval_cond(cond, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cond_val, append_lbl, inc_lbl));
                                } else {
                                    ir.push_str(&format!("  br label %{}\n\n", append_lbl));
                                }

                                ir.push_str(&format!("{}:\n", append_lbl));
                                let mapped_tv = self.eval_typed(map_expr, ir, scope_tree, string_constants, temp_counter);
                                inferred_list_elem = Some(Self::infer_elem_tag_from_expr(map_expr, &mapped_tv.ty, scope_tree));
                                let mapped_64 = self.coerce_to_raw_i64_bits(mapped_tv, ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_slice_append(i8* {}, i64 {})\n", slice_res, mapped_64));
                                ir.push_str(&format!("  br label %{}\n\n", inc_lbl));

                                scope_tree.exit_scope();

                                ir.push_str(&format!("{}:\n", inc_lbl));
                                let next_idx = format!("%next_comp_idx_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = add i64 {}, 1\n", next_idx, curr_idx));
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", next_idx, idx_ptr));
                                ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

                                ir.push_str(&format!("{}:\n", exit_lbl));
                            }
                        }
                    } else {
                        for elem in elements {
                            let elem_trim = elem.trim();
                            let elem_tv = self.eval_typed(elem_trim, ir, scope_tree, string_constants, temp_counter);
                            if inferred_list_elem.is_none() {
                                inferred_list_elem = Some(Self::infer_elem_tag_from_expr(elem_trim, &elem_tv.ty, scope_tree));
                            }
                            let val_64 = self.coerce_to_raw_i64_bits(elem_tv, ir, temp_counter);
                            ir.push_str(&format!(
                                "  call void @__omni_slice_append(i8* {}, i64 {})\n",
                                slice_res, val_64
                            ));
                        }
                    }

                    // Register RAII cleanup for slice resource
                    deferred_frees.push(CleanupOp::DestroySlice(ptr.clone()));

                    if let Some(etag) = inferred_list_elem {
                        scope_tree.collection_elem_types.insert(target.clone(), etag);
                    }
                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_slice)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                Statement::ListAppend { target, expr } => {
                    if let Some(binding) = scope_tree.resolve(target).cloned() {
                        if binding.type_hint.starts_with("%struct.") {
                            let cls_name = binding.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            let def_cls = scope_tree.resolve_defining_class(cls_name, "add");
                            let inst_ld = format!("%inst_call_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, binding.llvm_ptr));
                            let raw_inst = format!("%raw_call_inst_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_ld));
                            let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
                            let val_64 = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                            let ret_reg = format!("%method_ret_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_method_{}_add(i8* {}, i64 {}, i64 0, i64 0, i64 0, i64 0)\n",
                                ret_reg, def_cls, raw_inst, val_64
                            ));
                        } else {
                            let slice_loaded = format!("%slice_load_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", slice_loaded, binding.llvm_ptr));

                            let tv = self.eval_typed(expr, ir, scope_tree, string_constants, temp_counter);
                            let etag = Self::infer_elem_tag_from_expr(expr, &tv.ty, scope_tree);
                            if etag != "i64" || !scope_tree.collection_elem_types.contains_key(target) {
                                scope_tree.collection_elem_types.insert(target.clone(), etag);
                            }
                            let val_64 = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                            ir.push_str(&format!(
                                "  call void @__omni_slice_append(i8* {}, i64 {})\n",
                                slice_loaded, val_64
                            ));
                        }
                    }
                }

                // 10. Polyglot Index Assignment: `items[0] = 99`, `grid[1][0] = 99`, `m["key"] = 10`
                Statement::IndexSet {
                    target,
                    index_expr,
                    val_expr,
                } => {
                    let resolved_col: Option<(String, bool)> = if let Some(binding) = scope_tree.resolve(target).cloned() {
                        let col_loaded = format!("%col_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", col_loaded, binding.llvm_ptr));
                        Some((col_loaded, binding.type_hint.contains("omni_map")))
                    } else {
                        let (is_map, _) = Self::resolve_collection_target_info(target, scope_tree);
                        let ttv = self.eval_typed(target, ir, scope_tree, string_constants, temp_counter);
                        if matches!(ttv.ty, IrType::Ptr) {
                            Some((ttv.reg, is_map))
                        } else {
                            None
                        }
                    };

                    if let Some((col_loaded, is_map_target)) = resolved_col {
                        let vtv = self.eval_typed(val_expr, ir, scope_tree, string_constants, temp_counter);
                        let val_etag = Self::infer_elem_tag_from_expr(val_expr, &vtv.ty, scope_tree);
                        let val_kind = Self::elem_tag_to_map_kind(&val_etag);
                        if scope_tree.resolve(target).is_some()
                            && (val_etag != "i64" || !scope_tree.collection_elem_types.contains_key(target))
                        {
                            scope_tree.collection_elem_types.insert(target.clone(), val_etag.clone());
                        }
                        let val_64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);

                        if is_map_target {
                            let trimmed_idx = index_expr.trim();
                            if (trimmed_idx.starts_with('"') && trimmed_idx.ends_with('"'))
                                || (trimmed_idx.starts_with('\'') && trimmed_idx.ends_with('\''))
                            {
                                let text = &trimmed_idx[1..trimmed_idx.len() - 1];
                                scope_tree.map_key_types.insert((target.clone(), text.to_string()), val_etag);
                                if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                                    let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        str_ptr, len, len, const_name
                                    ));
                                    ir.push_str(&format!(
                                        "  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                        col_loaded, str_ptr, val_64, val_kind
                                    ));
                                }
                            } else if Self::is_string_expr(trimmed_idx, scope_tree) {
                                let str_ptr = self.eval_str_ptr(trimmed_idx, ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                    col_loaded, str_ptr, val_64, val_kind
                                ));
                            } else {
                                let idx_tv = self.eval_typed(index_expr, ir, scope_tree, string_constants, temp_counter);
                                let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_int_typed(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                    col_loaded, idx_64, val_64, val_kind
                                ));
                            }
                        } else {
                            // Default to slice
                            let idx_tv = self.eval_typed(index_expr, ir, scope_tree, string_constants, temp_counter);
                            let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                            ir.push_str(&format!(
                                "  call void @__omni_slice_set(i8* {}, i64 {}, i64 {})\n",
                                col_loaded, idx_64, val_64
                            ));
                        }
                    }
                }

                // 11. Polyglot Map / Dict Operations: `m = {"k": 10}`
                Statement::MapInit {
                    target,
                    entries,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%map_var_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let initial_cap = entries.len().max(8);
                    let map_res = format!("%map_init_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_map_create(i64 {})\n",
                        map_res, initial_cap
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", map_res, ptr));

                    let mut inferred_map_elem: Option<String> = None;
                    for (k, v) in entries {
                        let vtv = self.eval_typed(v, ir, scope_tree, string_constants, temp_counter);
                        let vtag = Self::infer_elem_tag_from_expr(v, &vtv.ty, scope_tree);
                        let vkind = Self::elem_tag_to_map_kind(&vtag);
                        if inferred_map_elem.is_none() {
                            inferred_map_elem = Some(vtag.clone());
                        }
                        let val_64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);

                        if let Some(int_str) = k.strip_prefix("\x01int:") {
                            let k64 = int_str.parse::<i64>().unwrap_or(0);
                            ir.push_str(&format!(
                                "  call void @__omni_map_set_int_typed(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                map_res, k64, val_64, vkind
                            ));
                        } else if let Some(kb) = scope_tree.resolve(k) {
                            if is_string_type_hint(&kb.type_hint) {
                                let kp = self.eval_str_ptr(k, ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                    map_res, kp, val_64, vkind
                                ));
                            } else {
                                let ktv = self.eval_typed(k, ir, scope_tree, string_constants, temp_counter);
                                let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_int_typed(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                    map_res, k64, val_64, vkind
                                ));
                            }
                        } else if let Some((const_name, len)) = Self::find_string_constant(string_constants, k) {
                            scope_tree.map_key_types.insert((target.clone(), k.clone()), vtag);
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  call void @__omni_map_set_typed(i8* {}, i8* {}, i64 {}, i64 {})\n",
                                map_res, str_ptr, val_64, vkind
                            ));
                        }
                    }

                    // Register RAII cleanup for map resource
                    deferred_frees.push(CleanupOp::DestroyMap(ptr.clone()));

                    if let Some(etag) = inferred_map_elem {
                        scope_tree.collection_elem_types.insert(target.clone(), etag);
                    }
                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_map)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                // 12. Go Concurrency Spawn: `go <call_expr>`
                Statement::GoSpawn { call_expr, idiom: _ } => {
                    let parts: Vec<&str> = call_expr.splitn(2, '(').collect();
                    let target_fn = parts[0].trim();
                    let raw_arg_str = parts.get(1).map(|s| s.trim_end_matches(')').trim()).unwrap_or("");
                    let spawn_args: Vec<String> = if raw_arg_str.is_empty() {
                        Vec::new()
                    } else {
                        omni_parser::OmniParser::split_respecting_quotes(raw_arg_str, ',')
                    };

                    if spawn_args.len() >= 2 {
                        // Multi-argument goroutine spawn: heap-pack up to 6 `i64` slots (48 bytes) for `@__omni_trampoline_pack_<fn>`.
                        let pack_raw = format!("%spawn_pack_raw_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @malloc(i64 48)\n", pack_raw));
                        let pack_i64 = format!("%spawn_pack_i64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = bitcast i8* {} to i64*\n", pack_i64, pack_raw));

                        for slot in 0..6usize {
                            let val_i64 = if let Some(arg_s) = spawn_args.get(slot) {
                                let a_trim = Self::strip_kwarg_prefix(arg_s.trim());
                                let tv = self.eval_typed(a_trim, ir, scope_tree, string_constants, temp_counter);
                                self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
                            } else {
                                "-581005069656919554".to_string()
                            };
                            let slot_ptr = format!("%spawn_slot_{}_{}", slot, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds i64, i64* {}, i64 {}\n", slot_ptr, pack_i64, slot));
                            ir.push_str(&format!("  store i64 {}, i64* {}\n", val_i64, slot_ptr));
                        }

                        ir.push_str(&format!("  ; >>> [Go Concurrency Spawn: {}]\n", call_expr));
                        ir.push_str(&format!(
                            "  call void @__omni_go_spawn(void (i8*)* @__omni_trampoline_pack_{}, i8* {})\n",
                            target_fn, pack_raw
                        ));
                        ir.push_str("  ; <<< [End Go Spawn]\n");
                    } else {
                        let raw_arg = spawn_args.first().map(|s| s.trim()).unwrap_or("");
                        let arg_reg = if !raw_arg.is_empty() {
                            let tv = self.eval_typed(raw_arg, ir, scope_tree, string_constants, temp_counter);
                            self.coerce_to_ptr(tv, ir, temp_counter)
                        } else {
                            let u_ptr = format!("%spawn_unset_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = inttoptr i64 -581005069656919554 to i8*\n", u_ptr));
                            u_ptr
                        };

                        ir.push_str(&format!("  ; >>> [Go Concurrency Spawn: {}]\n", call_expr));
                        ir.push_str(&format!(
                            "  call void @__omni_go_spawn(void (i8*)* @__omni_trampoline_{}, i8* {})\n",
                            target_fn, arg_reg
                        ));
                        ir.push_str("  ; <<< [End Go Spawn]\n");
                    }
                }

                // 9. Source-Anchored Interpretive Block
                Statement::InterpretiveAnchor {
                    anchor_id,
                    file,
                    line_start,
                    line_end,
                    physical_source,
                    idiom,
                } => {
                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let env_ret = if fn_name == "main" { "i32".to_string() } else { "i64".to_string() };
                    let envelope = SsaContextEnvelope {
                        target_abi: self.target_triple.clone(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: env_ret.clone(),
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: physical_source.clone(),
                        idiom_hint: Some(format!("{:?}", idiom)),
                        anchor_id: Some(*anchor_id),
                        physical_source: Some(physical_source.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some(env_ret),
                        action_directive: Some("SYNTHESIZE_ANONYMOUS_BLOCK".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    ir.push_str(&format!(
                        "  ; >>> [OMNI INTERPRETIVE ANCHOR #{} at {}:{}-{}]\n",
                        anchor_id, file, line_start, line_end
                    ));
                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    advance_ssa_counter_past_body(&body, temp_counter);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                    ir.push_str(&format!("  ; <<< [END ANCHOR #{}]\n", anchor_id));
                }

                // 10. Intent Statement -> packed into SsaContextEnvelope
                Statement::Intent { instruction } => {
                    let clean_instruction = instruction
                        .trim_start_matches("intent")
                        .trim_start_matches(':')
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();

                    if clean_instruction == "pass" || clean_instruction.is_empty() {
                        continue;
                    }

                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let idiom_hint = if clean_instruction.contains(".append(") {
                        Some("PythonListTransform".to_string())
                    } else {
                        None
                    };

                    let envelope = SsaContextEnvelope {
                        target_abi: self.target_triple.clone(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: if fn_name == "main" { "i32".to_string() } else { "i64".to_string() },
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: clean_instruction.clone(),
                        idiom_hint,
                        anchor_id: Some(*lbl_counter),
                        physical_source: Some(clean_instruction.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    ir.push_str(&format!(
                        "  ; =========================================================================\n  ; >>> [OMNI INTERPRETIVE ANCHOR: ANCHOR_{}]\n  ; Source: {}:{}\n  ; Directive: LOWER_ANONYMOUS_INTENT\n  ; Physical Source: \"{}\"\n  ; =========================================================================\n",
                        *lbl_counter, fn_name, *lbl_counter, instruction.trim()
                    ));
                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    advance_ssa_counter_past_body(&body, temp_counter);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                    ir.push_str(&format!("  ; <<< [END ANCHOR: ANCHOR_{}]\n", *lbl_counter));
                }

                // 11. Fallback Concrete / Polyglot Statement -> packed with Idiom hint
                Statement::Concrete { code, idiom } => {
                    let trimmed_code = code.trim().trim_end_matches(';').trim();
                    if trimmed_code == "break" {
                        if let Some(exit_lbl) = scope_tree.loop_exit_labels.last().cloned() {
                            if let Some(&mark) = scope_tree.loop_cleanup_marks.last() {
                                if mark <= deferred_frees.len() {
                                    Self::emit_cleanups(ir, &deferred_frees[mark..], temp_counter, "Break Loop Jump");
                                }
                            } else {
                                let entry_depth = scope_tree.loop_entry_try_depths.last().copied().unwrap_or(0);
                                for _ in entry_depth..scope_tree.active_try_depth {
                                    ir.push_str("  call void @__omni_try_pop()\n");
                                }
                            }
                            ir.push_str(&format!("  br label %{}\n", exit_lbl));
                            custom_return = true;
                            break;
                        }
                        continue;
                    }
                    if trimmed_code == "continue" {
                        if let Some(cond_lbl) = scope_tree.loop_cond_labels.last().cloned() {
                            if let Some(&mark) = scope_tree.loop_cleanup_marks.last() {
                                if mark <= deferred_frees.len() {
                                    Self::emit_cleanups(ir, &deferred_frees[mark..], temp_counter, "Continue Loop Jump");
                                }
                            } else {
                                let entry_depth = scope_tree.loop_entry_try_depths.last().copied().unwrap_or(0);
                                for _ in entry_depth..scope_tree.active_try_depth {
                                    ir.push_str("  call void @__omni_try_pop()\n");
                                }
                            }
                            ir.push_str(&format!("  br label %{}\n", cond_lbl));
                            custom_return = true;
                            break;
                        }
                        continue;
                    }
                    if trimmed_code == "pass"
                        || trimmed_code == "yield"
                        || trimmed_code.starts_with("yield ")
                        || trimmed_code.starts_with("assert ")
                        || trimmed_code.is_empty()
                        || trimmed_code.starts_with("\"\"\"")
                        || trimmed_code.starts_with("'''")
                        || ((trimmed_code.starts_with('"') && trimmed_code.ends_with('"'))
                            || (trimmed_code.starts_with('\'') && trimmed_code.ends_with('\'')))
                        || trimmed_code.starts_with("warnings.warn(")
                        || trimmed_code.starts_with("warnings.")
                        || trimmed_code.starts_with("super().")
                        || trimmed_code.starts_with("super(")
                        || trimmed_code.starts_with("base.")
                        || trimmed_code.starts_with("base(")
                    {
                        continue;
                    }

                    // Stack-unwinding `raise` / `throw` via `_Thread_local` `setjmp`/`longjmp`
                    if let Some((exc_type_opt, msg_opt)) = Self::parse_raise_or_throw(trimmed_code) {
                        if exc_type_opt.is_none() && msg_opt.is_none() {
                            if scope_tree.active_try_depth == 0 {
                                Self::emit_cleanups(ir, deferred_frees, temp_counter, "Rethrow Function Unwind");
                            }
                            ir.push_str("  call void @__omni_rethrow()\n");
                            ir.push_str("  unreachable\n");
                        } else {
                            let ty_name = exc_type_opt.as_deref().unwrap_or("Exception");
                            let ty_ptr = if let Some((cn, clen)) = Self::find_string_constant(string_constants, ty_name) {
                                let p = format!("%exc_ty_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                    p, clen, clen, cn
                                ));
                                p
                            } else {
                                self.eval_str_ptr("\"Exception\"", ir, scope_tree, string_constants, temp_counter)
                            };
                            let msg_ptr = if let Some(ref m_expr) = msg_opt {
                                self.eval_str_ptr(m_expr, ir, scope_tree, string_constants, temp_counter)
                            } else {
                                self.eval_str_ptr("\"\"", ir, scope_tree, string_constants, temp_counter)
                            };
                            if scope_tree.active_try_depth == 0 {
                                Self::emit_cleanups(ir, deferred_frees, temp_counter, "Throw Function Unwind");
                            }
                            ir.push_str(&format!(
                                "  call void @__omni_throw(i64 1, i8* {}, i8* {})\n",
                                ty_ptr, msg_ptr
                            ));
                            ir.push_str("  unreachable\n");
                        }
                        custom_return = true;
                        break;
                    }

                    // Python `del m[k]` / `del m[k1], m[k2]` map entry deletion (`[C5.2]`)
                    if let Some(del_rest) = trimmed_code.strip_prefix("del ") {
                        let mut handled_all_del = false;
                        for del_item in omni_parser::OmniParser::split_respecting_quotes(del_rest.trim(), ',') {
                            let dt = del_item.trim();
                            if let (Some(lb), true) = (dt.find('['), dt.ends_with(']')) {
                                let map_var = dt[..lb].trim();
                                let key_expr = dt[lb + 1..dt.len() - 1].trim();
                                if let Some(b) = scope_tree.resolve(map_var).cloned() {
                                    if b.type_hint.contains("omni_map") {
                                        let m_ld = format!("%del_map_ld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", m_ld, b.llvm_ptr));
                                        let ktv = self.eval_typed(key_expr, ir, scope_tree, string_constants, temp_counter);
                                        if ktv.ty == IrType::Ptr {
                                            let kp = self.coerce_to_ptr(ktv, ir, temp_counter);
                                            let d_ign = format!("%del_res_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = call i64 @__omni_map_delete(i8* {}, i8* {})\n", d_ign, m_ld, kp));
                                        } else {
                                            let ki = self.coerce_to_i64(ktv, ir, temp_counter);
                                            let d_ign = format!("%del_res_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = call i64 @__omni_map_delete_int(i8* {}, i64 {})\n", d_ign, m_ld, ki));
                                        }
                                        handled_all_del = true;
                                    }
                                }
                            }
                        }
                        if handled_all_del {
                            continue;
                        }
                    }

                    // Builtin exit(code)
                    if trimmed_code == "exit()" || (trimmed_code.starts_with("exit(") && trimmed_code.ends_with(')')) {
                        let code_val = if trimmed_code == "exit()" {
                            "0".to_string()
                        } else {
                            let inner = trimmed_code[5..trimmed_code.len() - 1].trim();
                            if inner.is_empty() {
                                "0".to_string()
                            } else {
                                self.eval_val(inner, ir, scope_tree, string_constants, temp_counter)
                            }
                        };
                        ir.push_str(&format!("  call void @exit(i32 {})\n", code_val));
                        ir.push_str("  unreachable\n");
                        custom_return = true;
                        break;
                    }

                    if trimmed_code == "Self::default()" {
                        if let Some(self_b) = scope_tree.resolve("self").cloned() {
                            if self_b.type_hint.starts_with("%struct.") {
                                let cls_name = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                let self_ld = format!("%self_def_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", self_ld, cls_name, cls_name, self_b.llvm_ptr));
                                let self_i8 = format!("%self_def_i8_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", self_i8, cls_name, self_ld));
                                let def_ret = format!("%self_def_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_method_{}_default(i8* {}, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                                    def_ret, cls_name, self_i8
                                ));
                                Self::emit_cleanups(ir, deferred_frees, temp_counter, "Self Default Return");
                                ir.push_str(&format!("  ret i64 {}\n", def_ret));
                                custom_return = true;
                                break;
                            }
                        }
                    }

                    // Rust inline struct literal in `default` / constructor method (`Adler32 { a: 1, b: 0 }`)
                    if let (Some(open_b), true) = (trimmed_code.find('{'), trimmed_code.ends_with('}')) {
                        let lit_cls = trimmed_code[..open_b].trim();
                        if let Some(self_b) = scope_tree.resolve("self").cloned() {
                            let cur_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            if (lit_cls == cur_cls || lit_cls == "Self") && scope_tree.class_fields.contains_key(cur_cls) {
                                let inner_fields = &trimmed_code[open_b + 1..trimmed_code.len() - 1];
                                for entry in omni_parser::OmniParser::split_respecting_quotes(inner_fields, ',') {
                                    if let Some((f_name, f_expr)) = entry.split_once(':') {
                                        let f_name = f_name.trim();
                                        let f_expr = f_expr.trim();
                                        if let Some(&f_idx) = scope_tree.class_fields.get(cur_cls).and_then(|m| m.get(f_name)) {
                                            let ftv = self.eval_typed(f_expr, ir, scope_tree, string_constants, temp_counter);
                                            let fv64 = self.coerce_to_raw_i64_bits(ftv, ir, temp_counter);
                                            let inst_ld = format!("%lit_self_ld_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cur_cls, cur_cls, self_b.llvm_ptr));
                                            let gep = format!("%lit_gep_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cur_cls, cur_cls, inst_ld, f_idx));
                                            ir.push_str(&format!("  store i64 {}, i64* {}\n", fv64, gep));
                                        }
                                    }
                                }
                                Self::emit_cleanups(ir, deferred_frees, temp_counter, "Struct Literal Return");
                                ir.push_str("  ret i64 0\n");
                                custom_return = true;
                                break;
                            }
                        }
                    }

                    // Direct method invocation on known object instance: `obj.method(args...)` or base class `Base.method(...)`
                    let is_rust_tail_expr = is_tail_position && is_last_stmt && fn_name != "main" && !code.trim().ends_with(';');
                    if let Some(open) = trimmed_code.find('(') {
                        if trimmed_code.ends_with(')') {
                            let target = trimmed_code[..open].trim();
                            if target.contains('.') {
                                let parts: Vec<&str> = target.split('.').collect();
                                if parts.len() >= 2 {
                                    let rec = parts[parts.len() - 2].trim();
                                    let method = parts[parts.len() - 1].trim();
                                    if method == "send" || method == "bind" || method == "listen" || method == "connect"
                                        || method == "close" || method == "recv" || method == "start" || method == "terminate"
                                        || trimmed_code.contains("Thread(") || trimmed_code.contains("Process(")
                                    {
                                        let _ = self.eval_typed(trimmed_code, ir, scope_tree, string_constants, temp_counter);
                                        continue;
                                    }
                                    if !is_rust_tail_expr {
                                        if scope_tree.resolve(rec).is_some()
                                            || scope_tree.class_fields.contains_key(rec)
                                            || scope_tree.c_module_aliases.contains(rec)
                                            || scope_tree.known_functions.contains(method)
                                            || rec.contains('(')
                                        {
                                            let _ = self.eval_typed(trimmed_code, ir, scope_tree, string_constants, temp_counter);
                                            continue;
                                        }
                                    }
                                }
                            } else if !is_rust_tail_expr
                                && (scope_tree.known_functions.contains(target)
                                    || scope_tree.extern_fns.contains_key(target)
                                    || NATIVE_C_LIB_FNS.contains(&target)
                                    || NATIVE_C_LIB_I64_FNS.contains(&target))
                            {
                                let _ = self.eval_typed(trimmed_code, ir, scope_tree, string_constants, temp_counter);
                                continue;
                            }
                        }
                    }

                    // Implicit return expression in non-main functions (e.g. Rust block `(value ^ salt) + 7` or `h.checksum()`)
                    if is_tail_position && is_last_stmt && fn_name != "main" && !code.trim().ends_with(';') {
                        if Self::split_binary_expr(code).is_some()
                            || scope_tree.resolve(code).is_some()
                            || parse_polyglot_i64(code).is_some()
                            || parse_polyglot_f64(code).is_some()
                            || (is_last_stmt && trimmed_code.ends_with(')') && trimmed_code.contains('('))
                        {
                            let rtv = self.eval_typed(trimmed_code, ir, scope_tree, string_constants, temp_counter);
                            if matches!(rtv.ty, IrType::Ptr | IrType::StructPtr(_)) {
                                let p8 = self.coerce_to_ptr(rtv.clone(), ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p8));
                            }
                            let ret_val = self.coerce_to_raw_i64_bits(rtv, ir, temp_counter);
                            if !scope_tree.closure_env_writebacks.is_empty() {
                                Self::emit_closure_writebacks(ir, &scope_tree.closure_env_writebacks, temp_counter);
                            }
                            Self::emit_cleanups(ir, deferred_frees, temp_counter, "Implicit Return");
                            ir.push_str(&format!("  ret i64 {}\n", ret_val));
                            custom_return = true;
                            break;
                        }
                    }

                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let env_ret = if fn_name == "main" { "i32".to_string() } else { "i64".to_string() };
                    let envelope = SsaContextEnvelope {
                        target_abi: self.target_triple.clone(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: env_ret.clone(),
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: code.clone(),
                        idiom_hint: Some(format!("{:?}", idiom)),
                        anchor_id: Some(*lbl_counter),
                        physical_source: Some(code.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some(env_ret),
                        action_directive: Some("LOWER_POLYGLOT_SYNTAX".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    advance_ssa_counter_past_body(&body, temp_counter);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                }

                // 12. Native Context Manager: `with Expr() as var:`
                Statement::WithContext {
                    context_expr,
                    var_alias,
                    body,
                } => {
                    let trimmed_expr = context_expr.trim();
                    let mut resource_cls = None;
                    let mut resource_raw = None;

                    let with_cand_expr = trimmed_expr.strip_prefix("new ").unwrap_or(trimmed_expr).trim();
                    if let Some(open) = with_cand_expr.find('(') {
                        if with_cand_expr.ends_with(')') {
                            let raw_cls_cand = with_cand_expr[..open].trim();
                            let cls_cand = raw_cls_cand.split('<').next().unwrap_or(raw_cls_cand).trim();
                            if scope_tree.class_fields.contains_key(cls_cand) {
                                let cls_name = cls_cand.to_string();
                                let f_count = scope_tree.class_fields.get(&cls_name).map(|m| m.len()).unwrap_or(1);
                                let alloc_bytes = (f_count * 8).max(8);
                                let tid = scope_tree.class_type_ids.get(&cls_name).copied().unwrap_or(0);
                                let raw_inst = format!("%raw_with_init_{}", *temp_counter);
                                let inst_alloc = format!("%inst_alloc_with_{}", *temp_counter);
                                let hold_slot = format!("%with_hold_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i8* @__omni_obj_alloc(i64 {}, i64 {})\n  {} = bitcast i8* {} to %struct.{}*\n  {} = alloca %struct.{}*\n  store %struct.{}* {}, %struct.{}** {}\n",
                                    raw_inst, alloc_bytes, tid, inst_alloc, raw_inst, cls_name, hold_slot, cls_name, cls_name, inst_alloc, cls_name, hold_slot
                                ));
                                deferred_frees.push(CleanupOp::FreeHeapPtr {
                                    slot_ptr: hold_slot,
                                    elem_ty: format!("%struct.{}*", cls_name),
                                });

                                let inner = &with_cand_expr[open + 1..with_cand_expr.len() - 1];
                                let raw_args: Vec<String> = if inner.is_empty() {
                                    Vec::new()
                                } else {
                                    omni_parser::OmniParser::split_respecting_quotes(inner, ',')
                                };

                                self.init_struct_instance(
                                    &cls_name,
                                    &raw_inst,
                                    &inst_alloc,
                                    "__init__",
                                    &raw_args,
                                    ir,
                                    scope_tree,
                                    string_constants,
                                    temp_counter,
                                );

                                let def_enter = scope_tree.resolve_defining_class(&cls_name, "__enter__");
                                if scope_tree.method_defining_class.contains_key(&(def_enter.clone(), "__enter__".to_string())) {
                                    let enter_ret = format!("%with_enter_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_method_{}___enter__(i8* {}, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                                        enter_ret, def_enter, raw_inst
                                    ));
                                }

                                if let Some(ref t) = var_alias {
                                    let target_ptr = format!("%var_{}_{}", t, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                    ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                    scope_tree.insert(t.clone(), SymbolBinding {
                                        name: t.clone(),
                                        llvm_ptr: target_ptr,
                                        type_hint: format!("%struct.{}*", cls_name),
                                        is_scoped: true,
                                    });
                                }

                                resource_cls = Some(cls_name);
                                resource_raw = Some(raw_inst);
                            }
                        }
                    } else if let Some(b) = scope_tree.resolve(with_cand_expr).cloned() {
                        if b.type_hint.starts_with("%struct.") {
                            let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*').to_string();
                            let inst_ld = format!("%with_var_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                            let raw_inst = format!("%raw_with_var_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_ld));

                            let def_enter = scope_tree.resolve_defining_class(&cls_name, "__enter__");
                            if scope_tree.method_defining_class.contains_key(&(def_enter.clone(), "__enter__".to_string())) {
                                let enter_ret = format!("%with_enter_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_method_{}___enter__(i8* {}, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                                    enter_ret, def_enter, raw_inst
                                ));
                            }

                            if let Some(ref t) = var_alias {
                                let target_ptr = format!("%var_{}_{}", t, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_ld, cls_name, target_ptr));
                                scope_tree.insert(t.clone(), SymbolBinding {
                                    name: t.clone(),
                                    llvm_ptr: target_ptr,
                                    type_hint: format!("%struct.{}*", cls_name),
                                    is_scoped: true,
                                });
                            }

                            resource_cls = Some(cls_name);
                            resource_raw = Some(raw_inst);
                        }
                    }

                    self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                        false,
                    )?;

                    if let (Some(cls_name), Some(raw_inst)) = (resource_cls, resource_raw) {
                        let def_exit = scope_tree.resolve_defining_class(&cls_name, "__exit__");
                        let exit_ret = format!("%with_exit_ret_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_method_{}___exit__(i8* {}, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                            exit_ret, def_exit, raw_inst
                        ));
                    }
                }

                // 13. Annotation / Decorator Binding
                Statement::AnnotationBinding { .. } => {}

                // 14. Type-Safe Polyglot Tuple Unpack (`q, err := safe_div(...)`, `pub_k, priv_k = rsa.newkeys(...)`)
                Statement::TupleUnpack { targets, expr, is_scoped_decl } => {
                    let expr_trim = expr.trim();
                    let unwrapped_rhs = if expr_trim.starts_with('(') && expr_trim.ends_with(')') {
                        let inner = &expr_trim[1..expr_trim.len() - 1];
                        let mut d = 0i32;
                        let mut balanced = true;
                        for c in inner.chars() {
                            if c == '(' { d += 1; }
                            else if c == ')' {
                                d -= 1;
                                if d < 0 { balanced = false; break; }
                            }
                        }
                        if balanced && d == 0 { inner.trim() } else { expr_trim }
                    } else {
                        expr_trim
                    };
                    let rhs_tuple_parts = omni_parser::OmniParser::split_respecting_quotes(unwrapped_rhs, ',');
                    let is_inline_tuple = rhs_tuple_parts.len() == targets.len() && rhs_tuple_parts.len() > 1;

                    let (rhs_ptr_opt, pre_evaluated_rhs): (Option<String>, Option<Vec<TypedValue>>) = if is_inline_tuple {
                        let mut vals = Vec::with_capacity(targets.len());
                        for idx in 0..targets.len() {
                            let p_trim = rhs_tuple_parts[idx].trim();
                            let tv = self.eval_typed(p_trim, ir, scope_tree, string_constants, temp_counter);
                            vals.push(tv);
                        }
                        (None, Some(vals))
                    } else if let Some(b) = scope_tree.resolve(expr_trim) {
                        let tl = b.type_hint.to_lowercase();
                        if b.type_hint.contains("omni_slice") || b.type_hint.starts_with("i8*") {
                            let ld = format!("%unpack_src_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i8*, i8** {}\n", ld, b.llvm_ptr));
                            (Some(ld), None)
                        } else if tl.contains("tuple") || tl.contains("slice") || tl.contains("list") || tl == "i64" {
                            let ld = format!("%unpack_src_i64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i64, i64* {}\n", ld, b.llvm_ptr));
                            let ptr = format!("%unpack_src_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr, ld));
                            (Some(ptr), None)
                        } else {
                            ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                            let rhs_tv = self.eval_typed(expr_trim, ir, scope_tree, string_constants, temp_counter);
                            let tramp_ptr = format!("%tramp_unpack_src_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", tramp_ptr));
                            let tramp_is_null = format!("%tramp_is_null_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = icmp eq i8* {}, null\n", tramp_is_null, tramp_ptr));
                            let fallback_ptr = self.coerce_to_ptr(rhs_tv, ir, temp_counter);
                            let ptr = format!("%unpack_src_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = select i1 {}, i8* {}, i8* {}\n", ptr, tramp_is_null, fallback_ptr, tramp_ptr));
                            (Some(ptr), None)
                        }
                    } else {
                        ir.push_str("  call void @__omni_c_return_ptr(i8* null)\n");
                        let rhs_tv = self.eval_typed(expr_trim, ir, scope_tree, string_constants, temp_counter);
                        let tramp_ptr = format!("%tramp_unpack_src_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_c_consume_returned_ptr()\n", tramp_ptr));
                        let tramp_is_null = format!("%tramp_is_null_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = icmp eq i8* {}, null\n", tramp_is_null, tramp_ptr));
                        let fallback_ptr = self.coerce_to_ptr(rhs_tv, ir, temp_counter);
                        let ptr = format!("%unpack_src_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = select i1 {}, i8* {}, i8* {}\n", ptr, tramp_is_null, fallback_ptr, tramp_ptr));
                        (Some(ptr), None)
                    };

                    // Resolve per-slot `IrType` from inline tuple expressions, intrinsics, or `fn_tuple_return_types`
                    let callee_tuple_types: Option<Vec<IrType>> = if pre_evaluated_rhs.is_some() {
                        None
                    } else if expr_trim.contains(".accept(") {
                        Some(vec![IrType::I64, IrType::Ptr])
                    } else if let Some(open) = expr_trim.find('(') {
                        let raw_callee = expr_trim[..open].trim();
                        let fn_ident = raw_callee.rsplit('.').next().unwrap_or(raw_callee).trim();
                        scope_tree
                            .fn_tuple_return_types
                            .get(raw_callee)
                            .or_else(|| scope_tree.fn_tuple_return_types.get(fn_ident))
                            .cloned()
                    } else {
                        None
                    };

                    for (idx, target) in targets.iter().enumerate() {
                        let target_trim = target.trim();
                        if target_trim == "_" || target_trim.is_empty() {
                            continue;
                        }

                        let (elem_bits_i64, slot_ty): (String, IrType) = if let Some(ref pre_vals) = pre_evaluated_rhs {
                            let tv = pre_vals[idx].clone();
                            let ty = tv.ty.clone();
                            let bits = self.coerce_to_raw_i64_bits(tv, ir, temp_counter);
                            (bits, ty)
                        } else {
                            let rhs_ptr = rhs_ptr_opt.as_ref().unwrap();
                            let ev = format!("%unpack_elem_{}_{}", idx, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n",
                                ev, rhs_ptr, idx
                            ));
                            let inferred_ty = callee_tuple_types
                                .as_ref()
                                .and_then(|v| v.get(idx).cloned())
                                .unwrap_or_else(|| {
                                    let tl = target_trim.to_lowercase();
                                    if tl == "err"
                                        || tl == "error"
                                        || tl == "addr"
                                        || tl.ends_with("_err")
                                        || tl.ends_with("_error")
                                    {
                                        IrType::Ptr
                                    } else {
                                        IrType::I64
                                    }
                                });
                            (ev, inferred_ty)
                        };

                        if target_trim.contains('.') {
                            let parts: Vec<&str> = target_trim.split('.').collect();
                            let rec = parts[0].trim();
                            let fld = parts[1].trim();
                            if let Some(b) = scope_tree.resolve(rec) {
                                if b.type_hint.starts_with("%struct.") {
                                    let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                    let f_idx = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fld)).copied().unwrap_or(0);
                                    let inst_ld = format!("%inst_fld_ld_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                                    let gep = format!("%fld_gep_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                                    ir.push_str(&format!("  store i64 {}, i64* {}\n", elem_bits_i64, gep));
                                    continue;
                                }
                            }
                        }

                        let existing_binding = if !*is_scoped_decl {
                            scope_tree.resolve(target_trim).cloned()
                        } else {
                            None
                        };

                        match slot_ty {
                            IrType::F64 => {
                                let fval = format!("%unpack_f64_{}_{}", idx, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fval, elem_bits_i64));
                                let target_ptr = if let Some(ep) = existing_binding
                                    .filter(|b| is_float_type_hint(&b.type_hint))
                                    .map(|b| b.llvm_ptr)
                                {
                                    ep
                                } else {
                                    let tp = format!("%fvar_{}_{}", target_trim, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca double\n", tp));
                                    scope_tree.insert(
                                        target_trim.to_string(),
                                        SymbolBinding {
                                            name: target_trim.to_string(),
                                            llvm_ptr: tp.clone(),
                                            type_hint: "double".to_string(),
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    tp
                                };
                                ir.push_str(&format!("  store double {}, double* {}\n", fval, target_ptr));
                            }
                            IrType::StructPtr(cls) => {
                                let sptr = format!("%unpack_sptr_{}_{}", idx, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = inttoptr i64 {} to %struct.{}*\n",
                                    sptr, elem_bits_i64, cls
                                ));
                                let expected_th = format!("%struct.{}*", cls);
                                let target_ptr = if let Some(ep) = existing_binding
                                    .filter(|b| b.type_hint == expected_th)
                                    .map(|b| b.llvm_ptr)
                                {
                                    ep
                                } else {
                                    let tp = format!("%svar_{}_{}", target_trim, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", tp, cls));
                                    scope_tree.insert(
                                        target_trim.to_string(),
                                        SymbolBinding {
                                            name: target_trim.to_string(),
                                            llvm_ptr: tp.clone(),
                                            type_hint: expected_th,
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    tp
                                };
                                ir.push_str(&format!(
                                    "  store %struct.{}* {}, %struct.{}** {}\n",
                                    cls, sptr, cls, target_ptr
                                ));
                            }
                            IrType::Ptr => {
                                let elem_ptr = format!("%unpack_ptr_{}_{}", idx, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = inttoptr i64 {} to i8*\n",
                                    elem_ptr, elem_bits_i64
                                ));
                                let target_ptr = if let Some(ep) = existing_binding
                                    .filter(|b| b.type_hint.starts_with("i8*"))
                                    .map(|b| b.llvm_ptr)
                                {
                                    ep
                                } else {
                                    let tp = format!("%ptr_{}_{}", target_trim, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca i8*\n", tp));
                                    scope_tree.insert(
                                        target_trim.to_string(),
                                        SymbolBinding {
                                            name: target_trim.to_string(),
                                            llvm_ptr: tp.clone(),
                                            type_hint: "i8* (string)".to_string(),
                                            is_scoped: *is_scoped_decl,
                                        },
                                    );
                                    tp
                                };
                                ir.push_str(&format!("  store i8* {}, i8** {}\n", elem_ptr, target_ptr));
                            }
                            _ => {
                                if let Some(ref eb) = existing_binding {
                                    if eb.type_hint == "i32" {
                                        let elem_i32 = format!("%unpack_i32_{}_{}", idx, *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = trunc i64 {} to i32\n", elem_i32, elem_bits_i64));
                                        ir.push_str(&format!("  store i32 {}, i32* {}\n", elem_i32, eb.llvm_ptr));
                                        continue;
                                    } else if eb.type_hint == "i64" {
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", elem_bits_i64, eb.llvm_ptr));
                                        continue;
                                    }
                                }
                                let tp = format!("%ivar_{}_{}", target_trim, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i64\n", tp));
                                scope_tree.insert(
                                    target_trim.to_string(),
                                    SymbolBinding {
                                        name: target_trim.to_string(),
                                        llvm_ptr: tp.clone(),
                                        type_hint: "i64".to_string(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                ir.push_str(&format!("  store i64 {}, i64* {}\n", elem_bits_i64, tp));
                            }
                        }
                    }
                }

                // 15. Stack-Unwinding `try / catch / except / finally` via `_setjmp` / `longjmp`
                Statement::TryCatch {
                    try_block,
                    catch_clauses,
                    finally_block,
                } => {
                    let try_id = *lbl_counter;
                    *lbl_counter += 1;

                    let try_body_lbl = format!("try_body_{}", try_id);
                    let catch_disp_lbl = format!("catch_disp_{}", try_id);
                    let finally_lbl = format!("try_finally_{}", try_id);
                    let unhandled_lbl = format!("try_unhandled_{}", try_id);
                    let end_lbl = format!("try_end_{}", try_id);

                    let jb_reg = format!("%jb_frame_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = call i8* @__omni_try_push()\n", jb_reg));

                    let sj_reg = format!("%sj_res_{}", *temp_counter);
                    *temp_counter += 1;
                    if self.target_triple.contains("windows") {
                        ir.push_str(&format!(
                            "  {} = call i32 @_setjmp(i8* {}, i8* null) returns_twice\n",
                            sj_reg, jb_reg
                        ));
                    } else {
                        ir.push_str(&format!(
                            "  {} = call i32 @_setjmp(i8* {}) returns_twice\n",
                            sj_reg, jb_reg
                        ));
                    }

                    let is_zero = format!("%sj_ok_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = icmp eq i32 {}, 0\n", is_zero, sj_reg));
                    ir.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n\n",
                        is_zero, try_body_lbl, catch_disp_lbl
                    ));

                    // --- Try Body ---
                    ir.push_str(&format!("{}:\n", try_body_lbl));
                    let try_cleanup_mark = deferred_frees.len();
                    deferred_frees.push(CleanupOp::PopTryFrame);
                    scope_tree.active_try_depth += 1;

                    let try_terminated = self.lower_statements(
                        try_block,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                        false,
                    )?;

                    scope_tree.active_try_depth -= 1;
                    if !try_terminated {
                        // Pop `PopTryFrame` + any inner try-scoped resources on normal completion
                        Self::emit_cleanups(ir, &deferred_frees[try_cleanup_mark..], temp_counter, "Try Normal Exit");
                        ir.push_str(&format!("  br label %{}\n\n", finally_lbl));
                    }

                    // --- Catch Dispatch (`longjmp` already popped the `OmniTryFrame` in C runtime) ---
                    ir.push_str(&format!("{}:\n", catch_disp_lbl));
                    if try_cleanup_mark + 1 < deferred_frees.len() {
                        Self::emit_cleanups(ir, &deferred_frees[try_cleanup_mark + 1..], temp_counter, "Try Unwind Cleanup");
                    }
                    deferred_frees.truncate(try_cleanup_mark);
                    if catch_clauses.is_empty() {
                        ir.push_str(&format!("  br label %{}\n\n", unhandled_lbl));
                    } else {
                        for (c_idx, clause) in catch_clauses.iter().enumerate() {
                            let match_body_lbl = format!("catch_body_{}_{}", try_id, c_idx);
                            let next_check_lbl = if c_idx + 1 < catch_clauses.len() {
                                format!("catch_check_{}_{}", try_id, c_idx + 1)
                            } else {
                                unhandled_lbl.clone()
                            };

                            if let Some(ref exc_ty) = clause.exception_type {
                                let ty_ptr = if let Some((cn, clen)) = Self::find_string_constant(string_constants, exc_ty.trim()) {
                                    let p = format!("%catch_ty_{}_{}", try_id, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        p, clen, clen, cn
                                    ));
                                    p
                                } else {
                                    "null".to_string()
                                };
                                let m_i32 = format!("%catch_m32_{}_{}", try_id, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i32 @__omni_catch_matches(i8* {})\n", m_i32, ty_ptr));
                                let m_ok = format!("%catch_mok_{}_{}", try_id, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = icmp ne i32 {}, 0\n", m_ok, m_i32));
                                ir.push_str(&format!(
                                    "  br i1 {}, label %{}, label %{}\n\n",
                                    m_ok, match_body_lbl, next_check_lbl
                                ));
                            } else {
                                ir.push_str(&format!("  br label %{}\n\n", match_body_lbl));
                            }

                            ir.push_str(&format!("{}:\n", match_body_lbl));
                            scope_tree.enter_scope();
                            if let Some(ref vname) = clause.var_name {
                                let msg_reg = format!("%catch_msg_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @__omni_catch_msg()\n", msg_reg));
                                let v_ptr = format!("%err_var_{}_{}", vname, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i8*\n", v_ptr));
                                ir.push_str(&format!("  store i8* {}, i8** {}\n", msg_reg, v_ptr));
                                scope_tree.insert(
                                    vname.clone(),
                                    SymbolBinding {
                                        name: vname.clone(),
                                        llvm_ptr: v_ptr,
                                        type_hint: "i8* (string)".to_string(),
                                        is_scoped: true,
                                    },
                                );
                            }

                            let catch_mark = deferred_frees.len();
                            let catch_ret = self.lower_statements(
                                &clause.body,
                                ir,
                                scope_tree,
                                deferred_frees,
                                string_constants,
                                cur_str_idx,
                                lbl_counter,
                                temp_counter,
                                fn_name,
                                struct_defs,
                                temp,
                                ecosystem_context,
                                import_chain,
                                dynamic_declarations,
                                false,
                            )?;
                            scope_tree.exit_scope();
                            if !catch_ret {
                                Self::emit_cleanups(ir, &deferred_frees[catch_mark..], temp_counter, "Catch Exit");
                                ir.push_str(&format!("  br label %{}\n\n", finally_lbl));
                            }
                            deferred_frees.truncate(catch_mark);

                            if c_idx + 1 < catch_clauses.len() {
                                ir.push_str(&format!("{}:\n", next_check_lbl));
                            }
                        }
                    }

                    // --- Unhandled Exception Path (run `finally` if present, then `@__omni_rethrow`) ---
                    ir.push_str(&format!("{}:\n", unhandled_lbl));
                    if let Some(ref fin) = finally_block {
                        scope_tree.enter_scope();
                        let fin_mark = deferred_frees.len();
                        let fin_ret = self.lower_statements(
                            fin,
                            ir,
                            scope_tree,
                            deferred_frees,
                            string_constants,
                            cur_str_idx,
                            lbl_counter,
                            temp_counter,
                            fn_name,
                            struct_defs,
                            temp,
                            ecosystem_context,
                            import_chain,
                            dynamic_declarations,
                            false,
                        )?;
                        scope_tree.exit_scope();
                        if !fin_ret {
                            Self::emit_cleanups(ir, &deferred_frees[fin_mark..], temp_counter, "Finally Unhandled Exit");
                            if scope_tree.active_try_depth == 0 {
                                Self::emit_cleanups(ir, &deferred_frees[..fin_mark], temp_counter, "Unhandled Function Unwind");
                            }
                            ir.push_str("  call void @__omni_rethrow()\n");
                            ir.push_str("  unreachable\n\n");
                        }
                        deferred_frees.truncate(fin_mark);
                    } else {
                        if scope_tree.active_try_depth == 0 {
                            Self::emit_cleanups(ir, deferred_frees, temp_counter, "Unhandled Function Unwind");
                        }
                        ir.push_str("  call void @__omni_rethrow()\n");
                        ir.push_str("  unreachable\n\n");
                    }

                    // --- Normal / Handled `finally` Path ---
                    ir.push_str(&format!("{}:\n", finally_lbl));
                    if let Some(ref fin) = finally_block {
                        scope_tree.enter_scope();
                        let fin_mark = deferred_frees.len();
                        let fin_ret = self.lower_statements(
                            fin,
                            ir,
                            scope_tree,
                            deferred_frees,
                            string_constants,
                            cur_str_idx,
                            lbl_counter,
                            temp_counter,
                            fn_name,
                            struct_defs,
                            temp,
                            ecosystem_context,
                            import_chain,
                            dynamic_declarations,
                            false,
                        )?;
                        scope_tree.exit_scope();
                        deferred_frees.truncate(fin_mark);
                        if fin_ret {
                            custom_return = true;
                            break;
                        }
                    }
                    ir.push_str(&format!("  br label %{}\n\n", end_lbl));
                    ir.push_str(&format!("{}:\n", end_lbl));
                }

                // 16. Nested Local Function / Lambda Declaration (`def inner(x): ...`, `f = lambda x: ...`)
                Statement::LocalFnDef { func } => {
                    let clos_ptr = self.lower_closure_def(func, ir, scope_tree, string_constants, temp_counter);
                    let slot = if let Some(eb) = scope_tree
                        .resolve(&func.name)
                        .filter(|b| b.type_hint.starts_with("i8*"))
                    {
                        let p = eb.llvm_ptr.clone();
                        let old_ld = format!("%clos_old_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = load i8*, i8** {}\n  call void @__omni_rc_release(i8* {})\n",
                            old_ld, p, old_ld
                        ));
                        p
                    } else {
                        let p = format!("%clos_var_{}_{}", func.name, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca i8*\n", p));
                        deferred_frees.push(CleanupOp::ReleaseRc(p.clone()));
                        scope_tree.insert(
                            func.name.clone(),
                            SymbolBinding {
                                name: func.name.clone(),
                                llvm_ptr: p.clone(),
                                type_hint: "i8* (omni_fn)".to_string(),
                                is_scoped: true,
                            },
                        );
                        p
                    };
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", clos_ptr, slot));
                    scope_tree.fn_param_defs.insert(func.name.clone(), func.params.clone());
                    if let Some(ref rt) = func.return_type {
                        scope_tree
                            .fn_return_types
                            .insert(func.name.clone(), classify_type_token(rt, &scope_tree.class_fields));
                    } else {
                        let mut sc = None;
                        let mut tp = None;
                        let mut lmap = Self::build_param_ir_types(&func.params, &scope_tree.class_fields);
                        Self::infer_return_signature_with_locals(&func.body, &scope_tree.class_fields, None, &mut lmap, &mut sc, &mut tp);
                        if let Some(slots) = tp {
                            let resolved: Vec<IrType> = slots.into_iter().map(|o| o.unwrap_or(IrType::I64)).collect();
                            scope_tree.fn_tuple_return_types.insert(func.name.clone(), resolved);
                            scope_tree.fn_return_types.insert(func.name.clone(), IrType::Ptr);
                        } else if let Some(s_ty) = sc {
                            scope_tree.fn_return_types.insert(func.name.clone(), s_ty);
                        }
                        if let Some(pk) = Self::infer_return_ptr_kind(&func.body, &func.params) {
                            scope_tree.fn_return_ptr_kinds.insert(func.name.clone(), pk);
                        }
                    }
                }
            }
        }

        Ok(custom_return)
    }

    /// Hoist all `alloca` instructions in a single LLVM function body to immediately after `entry:`,
    /// zero-initializing pointer `alloca` slots (`i8*`, `%struct.X*`, `%enum.E*`) to `null` so early
    /// `break` / `continue` / `_setjmp` unwind cleanups never read uninitialized stack slots.
    fn hoist_allocas(fn_ir: &str) -> String {
        let mut allocas = Vec::new();
        let mut body_lines = Vec::new();
        for line in fn_ir.lines() {
            if line.starts_with("  %") && line.contains(" = alloca ") {
                allocas.push(line);
            } else {
                body_lines.push(line);
            }
        }
        if allocas.is_empty() {
            return fn_ir.to_string();
        }
        let mut out = String::with_capacity(fn_ir.len() + allocas.len() * 32);
        let mut inserted = false;
        for line in body_lines {
            out.push_str(line);
            out.push('\n');
            if !inserted && line.trim() == "entry:" {
                for a in &allocas {
                    out.push_str(a);
                    out.push('\n');
                }
                for a in &allocas {
                    if let Some((lhs, rhs)) = a.trim().split_once(" = alloca ") {
                        let ty = rhs.trim();
                        if ty.ends_with('*') {
                            out.push_str(&format!("  store {} null, {}* {}\n", ty, ty, lhs.trim()));
                        }
                    }
                }
                inserted = true;
            }
        }
        out
    }

    #[allow(dead_code)]
    fn parse_default_i64(def_opt: &Option<String>) -> Option<i64> {
        let d = def_opt.as_ref()?.trim();
        if d == "True" || d == "true" {
            Some(1)
        } else if d == "False" || d == "false" || is_null_literal(d) {
            Some(0)
        } else {
            parse_polyglot_i64(d)
        }
    }

    fn build_param_ir_types(
        params: &[omni_parser::ParamDef],
        class_fields: &HashMap<String, HashMap<String, usize>>,
    ) -> HashMap<String, IrType> {
        let mut map = HashMap::new();
        for p in params {
            if let Some(ref th) = p.type_hint {
                let clean = th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim();
                if is_float_type_hint(clean) {
                    map.insert(p.name.clone(), IrType::F64);
                } else if is_string_type_hint(clean) || clean == "str" {
                    map.insert(p.name.clone(), IrType::Ptr);
                } else if class_fields.contains_key(clean) {
                    map.insert(p.name.clone(), IrType::StructPtr(clean.to_string()));
                }
            } else if let Some(ref dv) = p.default_val {
                let dt = dv.trim();
                if (dt.starts_with('"') && dt.ends_with('"')) || (dt.starts_with('\'') && dt.ends_with('\'')) {
                    map.insert(p.name.clone(), IrType::Ptr);
                } else if Self::parse_default_i64(&p.default_val).is_none() && parse_polyglot_f64(dt).is_some() {
                    map.insert(p.name.clone(), IrType::F64);
                }
            }
        }
        map
    }

    fn infer_return_ptr_kind(stmts: &[Statement], params: &[omni_parser::ParamDef]) -> Option<String> {
        fn walk(stmts: &[Statement], kinds: &mut HashMap<String, String>, out: &mut Option<String>) {
            for s in stmts {
                if let Statement::LocalFnDef { func } = s {
                    kinds.insert(func.name.clone(), "i8* (omni_fn)".to_string());
                }
            }
            for s in stmts {
                match s {
                    Statement::ListInit { target, .. } => {
                        kinds.insert(target.trim().to_string(), "i8* (omni_slice)".to_string());
                    }
                    Statement::MapInit { target, .. } => {
                        kinds.insert(target.trim().to_string(), "i8* (omni_map)".to_string());
                    }
                    Statement::Assign { target, expr, .. } => {
                        let e = expr.trim();
                        if !target.contains('.') && !target.contains('[') {
                            let k_opt = if (e.starts_with('[') && e.ends_with(']'))
                                || (e.starts_with("list(") && e.ends_with(')'))
                                || e.contains(".split(")
                                || e.ends_with(".chars()")
                                || e.starts_with("divmod(")
                            {
                                Some("i8* (omni_slice)".to_string())
                            } else if OmniParser::try_parse_lambda_expr(e).is_some() {
                                Some("i8* (omni_fn)".to_string())
                            } else if e.starts_with('{') && e.ends_with('}') {
                                Some("i8* (omni_map)".to_string())
                            } else if e.starts_with("make(") && e.contains("chan") {
                                Some("i8* (omni_chan)".to_string())
                            } else {
                                kinds.get(e).cloned()
                            };
                            if let Some(k) = k_opt {
                                kinds.insert(target.trim().to_string(), k);
                            }
                        }
                    }
                    Statement::Return { expr: Some(e) } => {
                        let t = e.trim();
                        let u = if t.starts_with('(') && t.ends_with(')') {
                            t[1..t.len() - 1].trim()
                        } else {
                            t
                        };
                        if OmniParser::split_respecting_quotes(u, ',').len() > 1
                            || (u.starts_with('[') && u.ends_with(']'))
                            || (u.starts_with("list(") && u.ends_with(')'))
                            || u.contains(".split(")
                            || u.starts_with("divmod(")
                        {
                            *out = Some("i8* (omni_slice)".to_string());
                        } else if OmniParser::try_parse_lambda_expr(u).is_some() {
                            *out = Some("i8* (omni_fn)".to_string());
                        } else if u.starts_with('{') && u.ends_with('}') {
                            *out = Some("i8* (omni_map)".to_string());
                        } else if let Some(k) = kinds.get(u) {
                            *out = Some(k.clone());
                        }
                    }
                    Statement::If { then_branch, else_branch, .. } => {
                        walk(then_branch, kinds, out);
                        if let Some(eb) = else_branch {
                            walk(eb, kinds, out);
                        }
                    }
                    Statement::WhileLoop { body, .. }
                    | Statement::ForLoop { body, .. }
                    | Statement::WithContext { body, .. } => {
                        walk(body, kinds, out);
                    }
                    Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                        walk(try_block, kinds, out);
                        for c in catch_clauses {
                            walk(&c.body, kinds, out);
                        }
                        if let Some(fb) = finally_block {
                            walk(fb, kinds, out);
                        }
                    }
                    Statement::Match { arms, .. } => {
                        for arm in arms {
                            walk(&arm.body, kinds, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut kinds = HashMap::new();
        for p in params {
            let pl = p.name.to_lowercase();
            let th_l = p.type_hint.as_deref().unwrap_or("").to_lowercase();
            if th_l.contains("slice") || th_l.contains("list") || matches!(pl.as_str(), "slice" | "items" | "arr" | "list") {
                kinds.insert(p.name.clone(), "i8* (omni_slice)".to_string());
            } else if th_l.contains("map") || th_l.contains("dict") || matches!(pl.as_str(), "map" | "dict") {
                kinds.insert(p.name.clone(), "i8* (omni_map)".to_string());
            } else if th_l.contains("chan") || matches!(pl.as_str(), "ch" | "chan") {
                kinds.insert(p.name.clone(), "i8* (omni_chan)".to_string());
            } else if th_l.contains("callable") || th_l.contains("fn") || pl == "callback" || pl.ends_with("_fn") || pl.ends_with("_func") {
                kinds.insert(p.name.clone(), "i8* (omni_fn)".to_string());
            }
        }
        let mut out = None;
        walk(stmts, &mut kinds, &mut out);
        out
    }

    #[allow(dead_code)]
    fn infer_expr_return_ir_type(
        expr: &str,
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
    ) -> Option<IrType> {
        Self::infer_expr_return_ir_type_with_locals(expr, class_fields, self_field_types, None)
    }

    fn infer_expr_return_ir_type_with_locals(
        expr: &str,
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
        local_types: Option<&HashMap<String, IrType>>,
    ) -> Option<IrType> {
        let e = expr.trim();
        if e.is_empty() || is_null_literal(e) {
            return None;
        }
        if let Some(lt_map) = local_types {
            if let Some(found) = lt_map.get(e) {
                return Some(found.clone());
            }
        }
        if Self::strip_str_literal_quotes(e).is_some()
            || e.starts_with("f\"")
            || e.starts_with("f'")
            || e.starts_with("errors.New(")
            || e.starts_with("fmt.Errorf(")
            || STRING_CAST_PREFIXES.iter().any(|p| e.starts_with(p) && e.ends_with(')'))
        {
            return Some(IrType::Ptr);
        }
        if parse_polyglot_f64(e).is_some()
            || FLOAT_CAST_PREFIXES.iter().any(|p| e.starts_with(p) && e.ends_with(')'))
        {
            return Some(IrType::F64);
        }
        if let Some(open) = e.find('(') {
            if e.ends_with(')') {
                let head = e[..open].trim().trim_start_matches("new ").trim();
                let head_base = head.split('<').next().unwrap_or(head).trim();
                if class_fields.contains_key(head_base) {
                    return Some(IrType::StructPtr(head_base.to_string()));
                }
            }
        }
        if let Some(open_b) = e.find('{') {
            if e.ends_with('}') {
                let head = e[..open_b].trim();
                if class_fields.contains_key(head) {
                    return Some(IrType::StructPtr(head.to_string()));
                }
            }
        }
        if OmniParser::try_parse_lambda_expr(e).is_some() {
            return Some(IrType::Ptr);
        }
        if let Ok(ast) = OmniParser::parse_expr(e) {
            match ast.node {
                ExprAst::Ident(ref id) => {
                    if let Some(lt_map) = local_types {
                        if let Some(found) = lt_map.get(id) {
                            return Some(found.clone());
                        }
                    }
                }
                ExprAst::FloatLit(_) => return Some(IrType::F64),
                ExprAst::StrLit(_) | ExprAst::ListLit(_) | ExprAst::MapLit(_) => return Some(IrType::Ptr),
                ExprAst::Unary { op, rhs } if op == "-" || op == "+" => {
                    return Self::infer_expr_return_ir_type_with_locals(rhs.slice(e), class_fields, self_field_types, local_types);
                }
                ExprAst::Member { target, field } => {
                    let rec = target.slice(e);
                    if (rec == "self" || rec == "this" || rec == "cls")
                        && self_field_types.is_some()
                    {
                        if let Some(ft) = self_field_types.unwrap().get(&field) {
                            if is_float_type_hint(ft) {
                                return Some(IrType::F64);
                            } else if ft.starts_with("%struct.") {
                                let c = ft.trim_start_matches("%struct.").trim_end_matches('*');
                                return Some(IrType::StructPtr(c.to_string()));
                            } else if ft.starts_with("i8*") || ft.starts_with("omni_slice") || ft.starts_with("omni_map") {
                                return Some(IrType::Ptr);
                            }
                        }
                    }
                }
                ExprAst::Binary { op, lhs, rhs } => {
                    if matches!(op.as_str(), "+" | "-" | "*" | "/" | "**" | "%") {
                        let lt = Self::infer_expr_return_ir_type_with_locals(lhs.slice(e), class_fields, self_field_types, local_types);
                        let rt = Self::infer_expr_return_ir_type_with_locals(rhs.slice(e), class_fields, self_field_types, local_types);
                        if lt == Some(IrType::F64) || rt == Some(IrType::F64) {
                            return Some(IrType::F64);
                        }
                        if op == "+" && (lt == Some(IrType::Ptr) || rt == Some(IrType::Ptr)) {
                            return Some(IrType::Ptr);
                        }
                    }
                }
                _ => {}
            }
        }
        Some(IrType::I64)
    }

    #[allow(dead_code)]
    fn infer_return_signature_recursive(
        stmts: &[Statement],
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
        scalar_acc: &mut Option<IrType>,
        tuple_acc: &mut Option<Vec<Option<IrType>>>,
    ) {
        let mut lmap = HashMap::new();
        Self::infer_return_signature_with_locals(stmts, class_fields, self_field_types, &mut lmap, scalar_acc, tuple_acc);
    }

    fn infer_return_signature_with_locals(
        stmts: &[Statement],
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
        local_types: &mut HashMap<String, IrType>,
        scalar_acc: &mut Option<IrType>,
        tuple_acc: &mut Option<Vec<Option<IrType>>>,
    ) {
        let mut local_fn_names: HashSet<String> = HashSet::new();
        for s in stmts {
            if let Statement::LocalFnDef { func } = s {
                local_fn_names.insert(func.name.clone());
                local_types.insert(func.name.clone(), IrType::Ptr);
            }
        }
        for s in stmts {
            match s {
                Statement::Assign { target, expr, .. } => {
                    if !target.contains('.') && !target.contains('[') {
                        if let Some(inferred) = Self::infer_expr_return_ir_type_with_locals(expr, class_fields, self_field_types, Some(local_types)) {
                            if matches!(inferred, IrType::Ptr | IrType::F64 | IrType::StructPtr(_)) {
                                local_types.insert(target.trim().to_string(), inferred);
                            }
                        }
                    }
                }
                Statement::ListInit { target, .. } | Statement::MapInit { target, .. } => {
                    local_types.insert(target.trim().to_string(), IrType::Ptr);
                }
                Statement::Return { expr: Some(e) } => {
                    let trimmed_e = e.trim();
                    let unwrapped = if trimmed_e.starts_with('(') && trimmed_e.ends_with(')') {
                        let inner = &trimmed_e[1..trimmed_e.len() - 1];
                        let mut d = 0i32;
                        let mut balanced = true;
                        for c in inner.chars() {
                            if c == '(' { d += 1; }
                            else if c == ')' {
                                d -= 1;
                                if d < 0 { balanced = false; break; }
                            }
                        }
                        if balanced && d == 0 { inner.trim() } else { trimmed_e }
                    } else {
                        trimmed_e
                    };
                    if local_fn_names.contains(unwrapped) {
                        *scalar_acc = Some(IrType::Ptr);
                        continue;
                    }
                    let parts = omni_parser::OmniParser::split_respecting_quotes(unwrapped, ',');
                    if parts.len() > 1 {
                        let slots = tuple_acc.get_or_insert_with(|| vec![None; parts.len()]);
                        for (idx, p) in parts.iter().enumerate() {
                            if idx >= slots.len() {
                                break;
                            }
                            if let Some(inferred) = Self::infer_expr_return_ir_type_with_locals(p, class_fields, self_field_types, Some(local_types)) {
                                if slots[idx].is_none() || matches!(inferred, IrType::Ptr | IrType::F64 | IrType::StructPtr(_)) {
                                    slots[idx] = Some(inferred);
                                }
                            }
                        }
                    } else if let Some(inferred) = Self::infer_expr_return_ir_type_with_locals(unwrapped, class_fields, self_field_types, Some(local_types)) {
                        if scalar_acc.is_none() || matches!(inferred, IrType::Ptr | IrType::F64 | IrType::StructPtr(_)) {
                            *scalar_acc = Some(inferred);
                        }
                    }
                }
                Statement::If { then_branch, else_branch, .. } => {
                    Self::infer_return_signature_with_locals(then_branch, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    if let Some(eb) = else_branch {
                        Self::infer_return_signature_with_locals(eb, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    }
                }
                Statement::WhileLoop { body, .. }
                | Statement::ForLoop { body, .. }
                | Statement::WithContext { body, .. } => {
                    Self::infer_return_signature_with_locals(body, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                }
                Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                    Self::infer_return_signature_with_locals(try_block, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    for c in catch_clauses {
                        Self::infer_return_signature_with_locals(&c.body, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    }
                    if let Some(fb) = finally_block {
                        Self::infer_return_signature_with_locals(fb, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    }
                }
                Statement::Match { arms, .. } => {
                    for arm in arms {
                        Self::infer_return_signature_with_locals(&arm.body, class_fields, self_field_types, local_types, scalar_acc, tuple_acc);
                    }
                }
                _ => {}
            }
        }
    }

    fn bind_abi_param_i64(
        functions_ir: &mut String,
        scope_tree: &mut ScopeTree,
        temp_counter: &mut usize,
        param: &omni_parser::ParamDef,
        reg_src: &str,
        class_fields_map: &HashMap<String, HashMap<String, usize>>,
        string_constants: &[(String, String, usize)],
    ) {
        let p_name_lower = param.name.to_lowercase();
        let clean_th = param
            .type_hint
            .as_deref()
            .map(|th| th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim());
        let th_is_float = clean_th.map(is_float_type_hint).unwrap_or(false);

        // Compute universal default `i64` bit-pattern BEFORE branching on parameter type (`[C2.1]–[C2.4]`)
        let mut inferred_str_default = false;
        let mut inferred_float_default = false;
        let fallback_bits = if let Some(ref dv) = param.default_val {
            let dt = dv.trim();
            if (dt.starts_with('"') && dt.ends_with('"')) || (dt.starts_with('\'') && dt.ends_with('\'')) {
                inferred_str_default = true;
                let inner = &dt[1..dt.len() - 1];
                let (cn, clen) = Self::find_string_constant(string_constants, inner)
                    .or_else(|| Self::find_string_constant(string_constants, ""))
                    .unwrap_or(("@.str.omni_0", 1));
                let sgep = format!("%param_def_sgep_{}_{}", param.name, *temp_counter);
                let si64 = format!("%param_def_si64_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!(
                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n  {} = ptrtoint i8* {} to i64\n",
                    sgep, clen, clen, cn, si64, sgep
                ));
                si64
            } else if is_null_literal(dt) {
                let th_is_ptr = clean_th
                    .map(|th| {
                        class_fields_map.contains_key(th)
                            || is_string_type_hint(th)
                            || th == "str"
                            || th == "omni_kwargs"
                            || th == "omni_varargs"
                            || th.contains("slice")
                            || th.contains("list")
                            || th.contains("map")
                            || th.contains("dict")
                            || th.contains("chan")
                    })
                    .unwrap_or(false);
                if th_is_ptr || th_is_float {
                    "0".to_string()
                } else {
                    OMNI_NULL_SENTINEL.to_string()
                }
            } else if let Some(iv) = Self::parse_default_i64(&param.default_val) {
                if th_is_float
                    && !is_null_literal(dt)
                    && dt != "True"
                    && dt != "true"
                    && dt != "False"
                    && dt != "false"
                {
                    ((iv as f64).to_bits() as i64).to_string()
                } else {
                    iv.to_string()
                }
            } else if let Some(fv) = parse_polyglot_f64(dt) {
                inferred_float_default = true;
                (fv.to_bits() as i64).to_string()
            } else {
                "0".to_string()
            }
        } else {
            "0".to_string()
        };

        let is_unset = format!("%param_unset_{}_{}", param.name, *temp_counter);
        let clean_reg = format!("%param_dsel_{}_{}", param.name, *temp_counter);
        *temp_counter += 1;
        functions_ir.push_str(&format!(
            "  {} = icmp eq i64 {}, -581005069656919554\n  {} = select i1 {}, i64 {}, i64 {}\n",
            is_unset, reg_src, clean_reg, is_unset, fallback_bits, reg_src
        ));

        if th_is_float || (param.type_hint.is_none() && inferred_float_default) {
            let fp = format!("%param_f64_{}_{}", param.name, *temp_counter);
            *temp_counter += 1;
            functions_ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fp, clean_reg));
            let ptr = format!("%param_{}_{}", param.name, *temp_counter);
            *temp_counter += 1;
            functions_ir.push_str(&format!("  {} = alloca double\n", ptr));
            functions_ir.push_str(&format!("  store double {}, double* {}\n", fp, ptr));
            scope_tree.insert(
                param.name.clone(),
                SymbolBinding {
                    name: param.name.clone(),
                    llvm_ptr: ptr,
                    type_hint: "double".to_string(),
                    is_scoped: true,
                },
            );
            return;
        }

        if let Some(th) = clean_th {
            if class_fields_map.contains_key(th) {
                let sp = format!("%param_sp_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", sp, clean_reg, th));
                let ptr = format!("%param_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = alloca %struct.{}*\n", ptr, th));
                functions_ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", th, sp, th, ptr));
                scope_tree.insert(
                    param.name.clone(),
                    SymbolBinding {
                        name: param.name.clone(),
                        llvm_ptr: ptr,
                        type_hint: format!("%struct.{}*", th),
                        is_scoped: true,
                    },
                );
                return;
            }
            if th == "omni_kwargs" || th == "dict" || th == "map" || th == "Map" || th == "HashMap" {
                let mp = format!("%param_map_{}_{}", param.name, *temp_counter);
                let ptr = format!("%param_{}_{}", param.name, *temp_counter);
                *temp_counter += 2;
                functions_ir.push_str(&format!(
                    "  {} = inttoptr i64 {} to i8*\n  {} = alloca i8*\n  store i8* {}, i8** {}\n",
                    mp, clean_reg, ptr, mp, ptr
                ));
                scope_tree.insert(
                    param.name.clone(),
                    SymbolBinding {
                        name: param.name.clone(),
                        llvm_ptr: ptr,
                        type_hint: "i8* (omni_map)".to_string(),
                        is_scoped: true,
                    },
                );
                return;
            }
            if th == "omni_varargs" {
                let sp = format!("%param_va_{}_{}", param.name, *temp_counter);
                let ptr = format!("%param_{}_{}", param.name, *temp_counter);
                *temp_counter += 2;
                functions_ir.push_str(&format!(
                    "  {} = inttoptr i64 {} to i8*\n  {} = alloca i8*\n  store i8* {}, i8** {}\n",
                    sp, clean_reg, ptr, sp, ptr
                ));
                scope_tree.insert(
                    param.name.clone(),
                    SymbolBinding {
                        name: param.name.clone(),
                        llvm_ptr: ptr,
                        type_hint: "i8* (omni_slice)".to_string(),
                        is_scoped: true,
                    },
                );
                return;
            }
            if is_string_type_hint(th) || th == "str" {
                let sp = format!("%param_strp_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", sp, clean_reg));
                let ptr = format!("%param_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                functions_ir.push_str(&format!("  store i8* {}, i8** {}\n", sp, ptr));
                scope_tree.insert(
                    param.name.clone(),
                    SymbolBinding {
                        name: param.name.clone(),
                        llvm_ptr: ptr,
                        type_hint: "i8* (string)".to_string(),
                        is_scoped: true,
                    },
                );
                return;
            }
        }

        let is_chan = param.type_hint.as_deref().map(|th| th.contains("chan")).unwrap_or(false)
            || p_name_lower == "ch"
            || p_name_lower == "chan"
            || p_name_lower.starts_with("chan_")
            || p_name_lower.ends_with("_chan")
            || p_name_lower.starts_with("ch_");
        let is_slice = param.type_hint.as_deref().map(|th| th.contains("slice") || th.contains("list")).unwrap_or(false)
            || p_name_lower == "slice"
            || p_name_lower == "items"
            || p_name_lower == "arr"
            || p_name_lower == "list"
            || p_name_lower.starts_with("items_")
            || p_name_lower.starts_with("arr_")
            || p_name_lower.starts_with("slice_")
            || p_name_lower.ends_with("_arr")
            || p_name_lower.ends_with("_slice")
            || p_name_lower.ends_with("_list");
        let is_map = param.type_hint.as_deref().map(|th| th.contains("map") || th.contains("dict")).unwrap_or(false)
            || p_name_lower == "map"
            || p_name_lower == "dict"
            || p_name_lower.starts_with("map_")
            || p_name_lower.starts_with("dict_")
            || p_name_lower.ends_with("_map")
            || p_name_lower.ends_with("_dict");
        let is_callable = param
            .type_hint
            .as_deref()
            .map(|th| th.to_lowercase().contains("callable") || th.contains("fn"))
            .unwrap_or(false)
            || p_name_lower.ends_with("_func")
            || p_name_lower.ends_with("_fn")
            || p_name_lower == "callback";

        if is_chan || is_slice || is_map || is_callable {
            let ptr_cast = format!("%ptr_cast_{}_{}", param.name, *temp_counter);
            *temp_counter += 1;
            functions_ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, clean_reg));
            let ptr = format!("%param_{}_{}", param.name, *temp_counter);
            *temp_counter += 1;
            functions_ir.push_str(&format!("  {} = alloca i8*\n", ptr));
            functions_ir.push_str(&format!("  store i8* {}, i8** {}\n", ptr_cast, ptr));
            let type_hint = if is_map {
                "i8* (omni_map)".to_string()
            } else if is_slice {
                "i8* (omni_slice)".to_string()
            } else if is_callable {
                "i8* (omni_fn)".to_string()
            } else {
                "i8* (omni_chan)".to_string()
            };
            scope_tree.insert(
                param.name.clone(),
                SymbolBinding {
                    name: param.name.clone(),
                    llvm_ptr: ptr,
                    type_hint,
                    is_scoped: true,
                },
            );
            return;
        }

        if param.type_hint.is_none() && inferred_str_default {
            let sp = format!("%param_strd_{}_{}", param.name, *temp_counter);
            let ptr = format!("%param_{}_{}", param.name, *temp_counter);
            *temp_counter += 1;
            functions_ir.push_str(&format!(
                "  {} = inttoptr i64 {} to i8*\n  {} = alloca i8*\n  store i8* {}, i8** {}\n",
                sp, clean_reg, ptr, sp, ptr
            ));
            scope_tree.insert(
                param.name.clone(),
                SymbolBinding {
                    name: param.name.clone(),
                    llvm_ptr: ptr,
                    type_hint: "i8* (string)".to_string(),
                    is_scoped: true,
                },
            );
            return;
        }

        let ptr = format!("%param_{}_{}", param.name, *temp_counter);
        *temp_counter += 1;
        functions_ir.push_str(&format!("  {} = alloca i64\n", ptr));
        functions_ir.push_str(&format!("  store i64 {}, i64* {}\n", clean_reg, ptr));
        scope_tree.insert(
            param.name.clone(),
            SymbolBinding {
                name: param.name.clone(),
                llvm_ptr: ptr,
                type_hint: "i64".to_string(),
                is_scoped: true,
            },
        );
    }

    /// Generate unified LLVM IR text from OmniModule
    pub fn generate_module_ir(&self, module: &OmniModule, temp: f32) -> anyhow::Result<String> {
        self.pending_semantic_errors.borrow_mut().clear();
        self.lifted_closures_ir.borrow_mut().clear();
        self.vdispatch_emitted.borrow_mut().clear();
        self.closure_counter.set(0);

        let mut string_constants: Vec<(String, String, usize)> = Vec::new();
        let mut str_counter = 0;

        // Always intern empty string, "Exception", and "ZeroDivisionError" so unwinding/div0 always has GEP constants
        Self::intern_string(&mut string_constants, &mut str_counter, "");
        Self::intern_string(&mut string_constants, &mut str_counter, "Exception");
        Self::intern_string(&mut string_constants, &mut str_counter, "ZeroDivisionError");
        Self::intern_string(&mut string_constants, &mut str_counter, "division by zero");

        let intern_param_default_strings = |params: &[omni_parser::ParamDef], sc: &mut Vec<(String, String, usize)>, cnt: &mut usize| {
            for p in params {
                if let Some(ref dv) = p.default_val {
                    let chars: Vec<char> = dv.chars().collect();
                    let mut idx = 0;
                    while idx < chars.len() {
                        if chars[idx] == '"' || chars[idx] == '\'' {
                            let q = chars[idx];
                            let st = idx + 1;
                            idx += 1;
                            while idx < chars.len() {
                                if chars[idx] == '\\' { idx += 2; continue; }
                                if chars[idx] == q {
                                    let s: String = chars[st..idx].iter().collect();
                                    Self::intern_string(sc, cnt, &s);
                                    break;
                                }
                                idx += 1;
                            }
                        }
                        idx += 1;
                    }
                }
            }
        };

        // Pre-scan statements & parameter defaults recursively to extract string & exception constants
        for func in &module.functions {
            intern_param_default_strings(&func.params, &mut string_constants, &mut str_counter);
            self.scan_strings_recursive(&func.body, &mut string_constants, &mut str_counter);
        }
        for cls in &module.structs_and_classes {
            for f in &cls.fields {
                if let Some(ref dv) = f.default_val {
                    let dt = dv.trim();
                    if (dt.starts_with('"') && dt.ends_with('"')) || (dt.starts_with('\'') && dt.ends_with('\'')) {
                        let inner = &dt[1..dt.len() - 1];
                        Self::intern_string(&mut string_constants, &mut str_counter, inner);
                    }
                }
            }
            for method in &cls.methods {
                intern_param_default_strings(&method.params, &mut string_constants, &mut str_counter);
                self.scan_strings_recursive(&method.body, &mut string_constants, &mut str_counter);
            }
        }
        self.scan_strings_recursive(&module.top_level_statements, &mut string_constants, &mut str_counter);

        // Extract ecosystem, import context, and valid external module/symbol sets (`[C1]`)
        let mut ecosystem_hints = Vec::new();
        let mut import_chain = Vec::new();
        let mut c_module_aliases_set = HashSet::new();
        let mut valid_extmod_prefixes: HashSet<String> = HashSet::new();
        let mut local_foreign_syms: HashSet<String> = self.foreign_symbols.clone();
        let mut has_open_ns_import = false;
        let mut has_unaliased_c_import = false;

        let sanitize_ext_id = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect()
        };
        let insert_mod_prefixes = |raw: &str, out: &mut HashSet<String>| {
            let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
            if trimmed.is_empty() {
                return;
            }
            out.insert(sanitize_ext_id(trimmed));
            for seg in trimmed.split(|c: char| c == '.' || c == '/' || c == ':') {
                let s = seg.trim();
                if !s.is_empty() && s != "*" {
                    out.insert(sanitize_ext_id(s));
                }
            }
        };

        for imp in &module.imports {
            match imp {
                omni_preamble::ImportDirective::Ecosystem { ecosystem, module, alias } => {
                    let eco_l = ecosystem.to_ascii_lowercase();
                    insert_mod_prefixes(module, &mut valid_extmod_prefixes);
                    if let Some(a) = alias {
                        for part in a.split(',') {
                            let pt = part.trim();
                            if !pt.is_empty() {
                                valid_extmod_prefixes.insert(sanitize_ext_id(pt));
                                local_foreign_syms.insert(pt.to_string());
                            }
                        }
                    }
                    if matches!(
                        eco_l.as_str(),
                        "csharp" | "c#" | "dotnet" | "java" | "jvm" | "kotlin" | "cpp" | "c++"
                    ) && alias.is_none()
                        || module.ends_with('*')
                    {
                        has_open_ns_import = true;
                    }
                    if eco_l == "c" || eco_l == "cpp" || eco_l == "c++" {
                        c_module_aliases_set.insert(module.clone());
                        if let Some(a) = alias {
                            c_module_aliases_set.insert(a.clone());
                        } else {
                            has_unaliased_c_import = true;
                        }
                    }
                    let desc = match alias {
                        Some(a) => format!("{} (module: {}, alias: {})", ecosystem, module, a),
                        None => format!("{} (module: {})", ecosystem, module),
                    };
                    ecosystem_hints.push(desc.clone());
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::FileAlias { path, alias } => {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        insert_mod_prefixes(stem, &mut valid_extmod_prefixes);
                        let clean = stem.trim_start_matches("lib");
                        if !clean.is_empty() {
                            insert_mod_prefixes(clean, &mut valid_extmod_prefixes);
                        }
                    }
                    if let Some(a) = alias {
                        insert_mod_prefixes(a, &mut valid_extmod_prefixes);
                    }
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let fname = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if matches!(
                        ext.as_str(),
                        "h" | "hpp" | "c" | "cc" | "cpp" | "ll" | "bc" | "o" | "obj" | "a" | "lib" | "so" | "dylib" | "dll"
                    ) || fname.contains(".so.")
                    {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            c_module_aliases_set.insert(stem.to_string());
                            let clean = stem.trim_start_matches("lib");
                            if !clean.is_empty() {
                                c_module_aliases_set.insert(clean.to_string());
                            }
                        }
                        if let Some(a) = alias {
                            c_module_aliases_set.insert(a.clone());
                        } else {
                            has_unaliased_c_import = true;
                        }
                    }
                    let desc = match alias {
                        Some(a) => format!("file: {} as {}", path.display(), a),
                        None => format!("file: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::DirectoryAlias { path, alias } => {
                    if let Some(dn) = path.file_name().and_then(|s| s.to_str()) {
                        insert_mod_prefixes(dn, &mut valid_extmod_prefixes);
                    }
                    if let Some(a) = alias {
                        insert_mod_prefixes(a, &mut valid_extmod_prefixes);
                    }
                    let desc = match alias {
                        Some(a) => format!("dir: {} as {}", path.display(), a),
                        None => format!("dir: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::FromDirectory { dir_path, symbols } => {
                    if let Some(dn) = dir_path.file_name().and_then(|s| s.to_str()) {
                        insert_mod_prefixes(dn, &mut valid_extmod_prefixes);
                    }
                    for sym_n in symbols {
                        for part in sym_n.split(" as ") {
                            let pt = part.trim();
                            if !pt.is_empty() {
                                local_foreign_syms.insert(pt.to_string());
                                insert_mod_prefixes(pt, &mut valid_extmod_prefixes);
                            }
                        }
                    }
                    import_chain.push(format!("from {}: {:?}", dir_path.display(), symbols));
                }
            }
        }

        for spine in &self.drilled_spines {
            import_chain.push(format!("__OMNI_DRILLED_MECH__:{}", spine));
        }

        let ecosystem_context = if ecosystem_hints.is_empty() {
            None
        } else {
            Some(ecosystem_hints.join(", "))
        };

        // 1. Structs and Classes -> Monomorphized Field Layout
        let class_map: HashMap<String, &omni_parser::ClassDef> = module.structs_and_classes.iter().map(|c| (c.name.clone(), c)).collect();

        fn collect_inherited_fields(
            cls_name: &str,
            class_map: &HashMap<String, &omni_parser::ClassDef>,
            visited: &mut HashSet<String>,
        ) -> Vec<(String, Option<String>, Option<String>)> {
            if visited.contains(cls_name) {
                return Vec::new();
            }
            visited.insert(cls_name.to_string());
            let mut result = Vec::new();
            if let Some(cls) = class_map.get(cls_name) {
                for base in &cls.base_classes {
                    let base_fields = collect_inherited_fields(base, class_map, visited);
                    for bf in base_fields {
                        if !result.iter().any(|(n, _, _)| n == &bf.0) {
                            result.push(bf);
                        }
                    }
                }
                for f in &cls.fields {
                    if !result.iter().any(|(n, _, _)| n == &f.name) {
                        result.push((f.name.clone(), f.type_hint.clone(), f.default_val.clone()));
                    }
                }
            }
            result
        }

        fn scan_unannotated_self_fields(
            stmts: &[Statement],
            known_class_names: &HashSet<String>,
            param_types: &HashMap<String, String>,
            field_names: &mut Vec<String>,
            f_types: &mut HashMap<String, String>,
        ) {
            for s in stmts {
                match s {
                    Statement::Assign { target, expr, .. } => {
                        let maybe_fld = target
                            .strip_prefix("self.")
                            .or_else(|| target.strip_prefix("this."))
                            .map(|f| f.trim());
                        if let Some(fld) = maybe_fld {
                            if !fld.is_empty() && !fld.contains('.') && !fld.contains('[') {
                                if !field_names.iter().any(|n| n == fld) {
                                    field_names.push(fld.to_string());
                                }
                                let needs_infer = f_types
                                    .get(fld)
                                    .map(|existing| existing == "i64" || existing == "any")
                                    .unwrap_or(true);
                                if needs_infer {
                                    let rhs = expr.trim();
                                    let inferred = if parse_polyglot_f64(rhs).is_some()
                                        || FLOAT_CAST_PREFIXES.iter().any(|p| rhs.starts_with(p) && rhs.ends_with(')'))
                                    {
                                        Some("f64".to_string())
                                    } else if (rhs.starts_with('"') && rhs.ends_with('"'))
                                        || (rhs.starts_with('\'') && rhs.ends_with('\''))
                                        || rhs.starts_with("f\"")
                                        || STRING_CAST_PREFIXES.iter().any(|p| rhs.starts_with(p) && rhs.ends_with(')'))
                                    {
                                        Some("i8* (string)".to_string())
                                    } else if rhs.starts_with('[') && rhs.ends_with(']') {
                                        Some("omni_slice:i64".to_string())
                                    } else if rhs.starts_with('{') && rhs.ends_with('}') {
                                        Some("omni_map:i64".to_string())
                                    } else if let Some(pth) = param_types.get(rhs) {
                                        if is_float_type_hint(pth) {
                                            Some("f64".to_string())
                                        } else if is_string_type_hint(pth) || pth == "str" {
                                            Some("i8* (string)".to_string())
                                        } else if known_class_names.contains(pth) {
                                            Some(format!("%struct.{}*", pth))
                                        } else {
                                            None
                                        }
                                    } else if let Some(open) = rhs.find('(') {
                                        let head = rhs[..open].trim().trim_start_matches("new ").trim();
                                        if known_class_names.contains(head) {
                                            Some(format!("%struct.{}*", head))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    if let Some(ft) = inferred {
                                        f_types.insert(fld.to_string(), ft);
                                    }
                                }
                            }
                        }
                    }
                    Statement::If { then_branch, else_branch, .. } => {
                        scan_unannotated_self_fields(then_branch, known_class_names, param_types, field_names, f_types);
                        if let Some(eb) = else_branch {
                            scan_unannotated_self_fields(eb, known_class_names, param_types, field_names, f_types);
                        }
                    }
                    Statement::WhileLoop { body, .. }
                    | Statement::ForLoop { body, .. }
                    | Statement::WithContext { body, .. } => {
                        scan_unannotated_self_fields(body, known_class_names, param_types, field_names, f_types);
                    }
                    Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                        scan_unannotated_self_fields(try_block, known_class_names, param_types, field_names, f_types);
                        for c in catch_clauses {
                            scan_unannotated_self_fields(&c.body, known_class_names, param_types, field_names, f_types);
                        }
                        if let Some(fb) = finally_block {
                            scan_unannotated_self_fields(fb, known_class_names, param_types, field_names, f_types);
                        }
                    }
                    _ => {}
                }
            }
        }

        let known_class_names: HashSet<String> = class_map.keys().cloned().collect();
        let mut class_fields_map: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut class_field_types_map: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut class_field_order_map: HashMap<String, Vec<(String, Option<String>, Option<String>)>> = HashMap::new();
        let mut class_decorators_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut struct_defs = Vec::new();
        let mut struct_defs_ir = String::new();

        for cls in &module.structs_and_classes {
            let all_fields = collect_inherited_fields(&cls.name, &class_map, &mut HashSet::new());
            let mut field_names: Vec<String> = Vec::new();
            let mut f_types: HashMap<String, String> = HashMap::new();
            let mut f_defaults: HashMap<String, Option<String>> = HashMap::new();
            for (f_name, th, dv) in all_fields {
                if !field_names.contains(&f_name) {
                    field_names.push(f_name.clone());
                }
                if let Some(t) = th {
                    f_types.insert(f_name.clone(), t);
                }
                f_defaults.insert(f_name, dv);
            }
            for m in &cls.methods {
                let mut p_types = HashMap::new();
                for p in &m.params {
                    if let Some(ref th) = p.type_hint {
                        p_types.insert(p.name.clone(), th.trim().to_string());
                    }
                }
                scan_unannotated_self_fields(&m.body, &known_class_names, &p_types, &mut field_names, &mut f_types);
            }
            if field_names.is_empty() {
                field_names.push("_unused".to_string());
            }

            let mut f_map = HashMap::new();
            let mut ordered_vec = Vec::new();
            let mut field_desc_parts = Vec::new();
            for (idx, name) in field_names.iter().enumerate() {
                f_map.insert(name.clone(), idx);
                let th_opt = f_types.get(name).cloned();
                let dv_opt = f_defaults.get(name).cloned().flatten();
                field_desc_parts.push(format!("{}: {} ({})", idx, name, th_opt.as_deref().unwrap_or("i64")));
                ordered_vec.push((name.clone(), th_opt, dv_opt));
            }
            class_fields_map.insert(cls.name.clone(), f_map);
            class_field_types_map.insert(cls.name.clone(), f_types);
            class_field_order_map.insert(cls.name.clone(), ordered_vec);
            class_decorators_map.insert(cls.name.clone(), cls.decorators.clone());

            let type_list = vec!["i64"; field_names.len()].join(", ");
            let s_def = format!("%struct.{} = type {{ {} }}", cls.name, type_list);
            let s_def_annotated = if cls.decorators.is_empty() {
                format!("{} ; fields: [{}]", s_def, field_desc_parts.join(", "))
            } else {
                format!(
                    "{} ; decorators: [{}] ; fields: [{}]",
                    s_def,
                    cls.decorators.join(", "),
                    field_desc_parts.join(", ")
                )
            };
            struct_defs_ir.push_str(&format!("; Struct Layout: {} ({} fields: {})\n", cls.name, field_names.len(), field_names.join(", ")));
            struct_defs_ir.push_str(&format!("{}\n\n", s_def));
            struct_defs.push(s_def_annotated);
        }

        let mut class_type_ids_map: HashMap<String, u64> = HashMap::new();
        for (cls_idx, cls) in module.structs_and_classes.iter().enumerate() {
            class_type_ids_map.insert(cls.name.clone(), (cls_idx + 1) as u64);
        }

        let mut method_defining_map: HashMap<(String, String), String> = HashMap::new();
        let mut method_return_types_map: HashMap<(String, String), IrType> = HashMap::new();
        let mut method_param_defs_map: HashMap<(String, String), Vec<omni_parser::ParamDef>> = HashMap::new();
        let mut class_properties_map: HashMap<String, HashSet<String>> = HashMap::new();
        for cls in &module.structs_and_classes {
            let self_ft_opt = class_field_types_map.get(&cls.name);
            for m in &cls.methods {
                method_defining_map.insert((cls.name.clone(), m.name.clone()), cls.name.clone());
                if m.annotations.iter().any(|a| {
                    let t = a.target_expr.trim();
                    t == "property" || t == "cached_property" || t.ends_with(".property") || t.ends_with(".cached_property")
                }) {
                    class_properties_map.entry(cls.name.clone()).or_default().insert(m.name.clone());
                }
                let skip_n = if m
                    .params
                    .first()
                    .map(|p| p.name == "self" || p.name == "this" || p.name == "cls")
                    .unwrap_or(false)
                {
                    1
                } else {
                    0
                };
                method_param_defs_map.insert(
                    (cls.name.clone(), m.name.clone()),
                    m.params.iter().skip(skip_n).cloned().collect(),
                );
                let m_ret = if let Some(ref rt) = m.return_type {
                    classify_type_token(rt, &class_fields_map)
                } else {
                    let mut sc = None;
                    let mut tp = None;
                    let mut lmap = Self::build_param_ir_types(&m.params[skip_n..], &class_fields_map);
                    Self::infer_return_signature_with_locals(&m.body, &class_fields_map, self_ft_opt, &mut lmap, &mut sc, &mut tp);
                    if tp.is_some() {
                        IrType::Ptr
                    } else {
                        sc.unwrap_or(IrType::I64)
                    }
                };
                method_return_types_map.insert((cls.name.clone(), m.name.clone()), m_ret);
            }
        }
        for cls in &module.structs_and_classes {
            let mut queue = cls.base_classes.clone();
            let mut seen_bases = HashSet::new();
            while let Some(b) = queue.pop() {
                if seen_bases.insert(b.clone()) {
                    if let Some(base_cls) = class_map.get(&b) {
                        for m in &base_cls.methods {
                            method_defining_map.entry((cls.name.clone(), m.name.clone())).or_insert(b.clone());
                            if let Some(base_rt) = method_return_types_map.get(&(b.clone(), m.name.clone())).cloned() {
                                method_return_types_map.entry((cls.name.clone(), m.name.clone())).or_insert(base_rt);
                            }
                            if let Some(base_pd) = method_param_defs_map.get(&(b.clone(), m.name.clone())).cloned() {
                                method_param_defs_map.entry((cls.name.clone(), m.name.clone())).or_insert(base_pd);
                            }
                            if class_properties_map.get(&b).map(|s| s.contains(&m.name)).unwrap_or(false) {
                                class_properties_map.entry(cls.name.clone()).or_default().insert(m.name.clone());
                            }
                        }
                        for bb in &base_cls.base_classes {
                            queue.push(bb.clone());
                        }
                    }
                }
            }
        }

        // 1-B. Enums -> Tagged Union Types & Variant Maps
        let mut enum_variants_map: HashMap<String, (String, usize)> = HashMap::new();
        for e in &module.enums {
            let e_def = format!("%enum.{} = type {{ i32, i64 }}", e.name);
            struct_defs_ir.push_str(&format!("; Tagged Enum Layout: {} ({} variants)\n", e.name, e.variants.len()));
            struct_defs_ir.push_str(&format!("{}\n\n", e_def));
            struct_defs.push(e_def);

            for v in &e.variants {
                let qualified = format!("{}::{}", e.name, v.name);
                enum_variants_map.insert(qualified, (e.name.clone(), v.tag));
                enum_variants_map.insert(v.name.clone(), (e.name.clone(), v.tag));
            }
        }

        // 2. Functions & Return Type Lattice
        let mut known_functions_set = HashSet::new();
        let mut extern_fns_map: HashMap<String, (Vec<IrType>, Option<IrType>)> = HashMap::new();
        let mut fn_param_defs_map: HashMap<String, Vec<omni_parser::ParamDef>> = HashMap::new();
        let mut fn_return_types_map: HashMap<String, IrType> = HashMap::new();
        let mut fn_return_ptr_kinds_map: HashMap<String, String> = HashMap::new();
        let mut fn_curried_return_types_map: HashMap<String, IrType> = HashMap::new();
        let mut fn_tuple_return_types_map: HashMap<String, Vec<IrType>> = HashMap::new();
        let mut main_param_count = 0usize;

        for f in &module.functions {
            known_functions_set.insert(f.name.clone());
            fn_param_defs_map.insert(f.name.clone(), f.params.clone());
            if f.name == "main" {
                main_param_count = f.params.len();
                fn_return_types_map.insert(f.name.clone(), IrType::I32);
                continue;
            }
            // Infer curried inner closure return type (`def make_greeter(): def greet(name="World"): return "Hi " + name; return greet`)
            let mut local_fn_map: HashMap<String, &omni_parser::FunctionDef> = HashMap::new();
            for s in &f.body {
                if let Statement::LocalFnDef { func } = s {
                    local_fn_map.insert(func.name.clone(), func);
                }
            }
            for s in &f.body {
                if let Statement::Return { expr: Some(ref re) } = s {
                    let rt_trim = re.trim();
                    if let Some(inner_f) = local_fn_map.get(rt_trim) {
                        if let Some(ref irt) = inner_f.return_type {
                            fn_curried_return_types_map.insert(f.name.clone(), classify_type_token(irt, &class_fields_map));
                        } else {
                            let mut isc = None;
                            let mut itp = None;
                            let mut ilmap = Self::build_param_ir_types(&f.params, &class_fields_map);
                            ilmap.extend(Self::build_param_ir_types(&inner_f.params, &class_fields_map));
                            Self::infer_return_signature_with_locals(&inner_f.body, &class_fields_map, None, &mut ilmap, &mut isc, &mut itp);
                            if let Some(irty) = isc {
                                fn_curried_return_types_map.insert(f.name.clone(), irty);
                            }
                        }
                    } else if let Some((lps, lbody)) = OmniParser::try_parse_lambda_expr(rt_trim) {
                        let mut ilmap = Self::build_param_ir_types(&f.params, &class_fields_map);
                        ilmap.extend(Self::build_param_ir_types(&lps, &class_fields_map));
                        let irty_opt = if lbody.starts_with('{') && lbody.ends_with('}') {
                            let lstmts = OmniParser::new().parse_module("cur_lam", &lbody[1..lbody.len() - 1]).top_level_statements;
                            let mut isc = None;
                            let mut itp = None;
                            Self::infer_return_signature_with_locals(&lstmts, &class_fields_map, None, &mut ilmap, &mut isc, &mut itp);
                            isc
                        } else {
                            Self::infer_expr_return_ir_type_with_locals(&lbody, &class_fields_map, None, Some(&ilmap))
                        };
                        if let Some(irty) = irty_opt {
                            fn_curried_return_types_map.insert(f.name.clone(), irty);
                        }
                    }
                }
            }
            if let Some(ref rt) = f.return_type {
                if let Some(tup) = parse_tuple_return_types(rt, &class_fields_map) {
                    fn_tuple_return_types_map.insert(f.name.clone(), tup);
                    fn_return_types_map.insert(f.name.clone(), IrType::Ptr);
                    continue;
                }
                fn_return_types_map.insert(f.name.clone(), classify_type_token(rt, &class_fields_map));
            } else {
                let mut sc = None;
                let mut tp = None;
                let mut lmap = Self::build_param_ir_types(&f.params, &class_fields_map);
                Self::infer_return_signature_with_locals(&f.body, &class_fields_map, None, &mut lmap, &mut sc, &mut tp);
                if let Some(slots) = tp {
                    let resolved: Vec<IrType> = slots
                        .into_iter()
                        .map(|o| o.unwrap_or(IrType::I64))
                        .collect();
                    fn_tuple_return_types_map.insert(f.name.clone(), resolved);
                    fn_return_types_map.insert(f.name.clone(), IrType::Ptr);
                    fn_return_ptr_kinds_map.insert(f.name.clone(), "i8* (omni_slice)".to_string());
                } else if let Some(s_ty) = sc {
                    fn_return_types_map.insert(f.name.clone(), s_ty);
                }
            }
            if let Some(pk) = Self::infer_return_ptr_kind(&f.body, &f.params) {
                fn_return_ptr_kinds_map.insert(f.name.clone(), pk);
            }
        }

        // 2-B. External / HAL Syscall `extern fn` Declarations (Platform Prelude + `import "pico*"` + User `extern fn`)
        let mut collected_extern_defs: Vec<omni_parser::ExternFnDef> = Vec::new();
        if self.target_triple.starts_with("thumb") {
            let plat_prelude = if matches!(self.platform_slug.as_str(), "rp2040" | "rp2350") {
                omni_preamble::PICO_HAL_BASE_PRELUDE
            } else {
                omni_preamble::PICO_HAL_FULL_PRELUDE
            };
            collected_extern_defs.extend(
                omni_parser::OmniParser::new()
                    .parse_module("__hal_platform_prelude", plat_prelude)
                    .extern_functions,
            );
        }
        for imp in &module.imports {
            if let Some((hal_mod, alias_opt)) = omni_preamble::extract_builtin_hal_import(imp) {
                let stem = hal_mod.split('/').next().unwrap_or(hal_mod);
                c_module_aliases_set.insert(stem.to_string());
                if let Some(a) = alias_opt {
                    c_module_aliases_set.insert(a.to_string());
                }
                if let Some(src) = omni_preamble::builtin_hal_prelude_for_import(hal_mod) {
                    collected_extern_defs.extend(
                        omni_parser::OmniParser::new()
                            .parse_module("__hal_import_prelude", src)
                            .extern_functions,
                    );
                }
            }
        }
        collected_extern_defs.extend(module.extern_functions.iter().cloned());

        for ext in collected_extern_defs {
            if known_functions_set.contains(&ext.name) {
                continue;
            }
            let p_tys: Vec<IrType> = ext
                .params
                .iter()
                .map(|p| {
                    p.type_hint
                        .as_deref()
                        .map(|th| classify_type_token(th, &class_fields_map))
                        .unwrap_or(IrType::I64)
                })
                .collect();
            let r_ty: Option<IrType> = match ext.return_type.as_deref().map(|s| s.trim()) {
                Some("void") | Some("()") | Some("None") => None,
                Some("i32") | Some("int32") => Some(IrType::I32),
                Some(rt) => Some(classify_type_token(rt, &class_fields_map)),
                None => Some(IrType::I64),
            };
            if let Some(ref rt_inner) = r_ty {
                if *rt_inner == IrType::Ptr {
                    fn_return_ptr_kinds_map
                        .entry(ext.name.clone())
                        .or_insert_with(|| "i8* (string)".to_string());
                }
                fn_return_types_map.insert(ext.name.clone(), rt_inner.clone());
            }
            extern_fns_map.insert(ext.name, (p_tys, r_ty));
        }

        let mut functions_ir = String::new();
        let mut user_emitted_ir = String::new();
        let mut dynamic_declarations = HashSet::new();
        let mut has_user_main = false;
        let mut cur_str_idx = 0;
        let mut lbl_counter = 0;
        let mut temp_counter = 0;

        // Reachability analysis for dead-code pruning
        let mut reachable_symbols = HashSet::new();
        Self::harvest_called_symbols(&module.top_level_statements, &mut reachable_symbols);
        for f in &module.functions {
            if f.name == "main" {
                reachable_symbols.insert(f.name.clone());
                Self::harvest_called_symbols(&f.body, &mut reachable_symbols);
            }
        }
        let mut reach_changed = true;
        while reach_changed {
            reach_changed = false;
            for f in &module.functions {
                if reachable_symbols.contains(&f.name) {
                    let before_len = reachable_symbols.len();
                    Self::harvest_called_symbols(&f.body, &mut reachable_symbols);
                    if reachable_symbols.len() > before_len {
                        reach_changed = true;
                    }
                }
            }
            for cls in &module.structs_and_classes {
                for m in &cls.methods {
                    let qualified = format!("{}.{}", cls.name, m.name);
                    if reachable_symbols.contains(&m.name)
                        || reachable_symbols.contains(&qualified)
                        || (m.name == "__init__" && reachable_symbols.contains(&cls.name))
                    {
                        let before_len = reachable_symbols.len();
                        Self::harvest_called_symbols(&m.body, &mut reachable_symbols);
                        if reachable_symbols.len() > before_len {
                            reach_changed = true;
                        }
                    }
                }
            }
        }

        // 1-C. Struct & Class Inherent Methods (`i64 @__omni_method_<cls>_<method>(i8*, i64 x max(5, arity))`)
        for cls in &module.structs_and_classes {
            for method in &cls.methods {
                let skip_count = if method
                    .params
                    .first()
                    .map(|p| p.name == "self" || p.name == "this" || p.name == "cls")
                    .unwrap_or(false)
                {
                    1
                } else {
                    0
                };
                let m_arity = method.params.len().saturating_sub(skip_count).max(5);
                let m_param_sig = (0..m_arity)
                    .map(|i| {
                        if i == 0 {
                            "i64 %len".to_string()
                        } else {
                            format!("i64 %arg{}", i + 1)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                let qualified = format!("{}.{}", cls.name, method.name);
                let is_reachable = reachable_symbols.contains(&method.name)
                    || reachable_symbols.contains(&qualified)
                    || (method.name == "__init__" && reachable_symbols.contains(&cls.name));

                if !is_reachable {
                    functions_ir.push_str(&format!(
                        "define i64 @__omni_method_{}_{}(i8* %self_raw, {}) {{\nentry:\n  ret i64 0\n}}\n\n",
                        cls.name, method.name, m_param_sig
                    ));
                    continue;
                }

                let method_start_len = functions_ir.len();
                let method_saved_str_idx = cur_str_idx;
                let method_saved_lbl = lbl_counter;
                let method_saved_temp = temp_counter;

                functions_ir.push_str(&format!(
                    "define i64 @__omni_method_{}_{}(i8* %self_raw, {}) {{\nentry:\n",
                    cls.name, method.name, m_param_sig
                ));

                let mut scope_tree = ScopeTree::new();
                scope_tree.class_fields = class_fields_map.clone();
                scope_tree.class_field_types = class_field_types_map.clone();
                scope_tree.class_field_order = class_field_order_map.clone();
                scope_tree.class_decorators = class_decorators_map.clone();
                scope_tree.class_properties = class_properties_map.clone();
                scope_tree.class_type_ids = class_type_ids_map.clone();
                scope_tree.c_module_aliases = c_module_aliases_set.clone();
                scope_tree.valid_extmod_prefixes = valid_extmod_prefixes.clone();
                scope_tree.has_open_ns_import = has_open_ns_import;
                scope_tree.enum_variants = enum_variants_map.clone();
                scope_tree.known_functions = known_functions_set.clone();
                scope_tree.extern_fns = extern_fns_map.clone();
                scope_tree.fn_param_defs = fn_param_defs_map.clone();
                scope_tree.method_param_defs = method_param_defs_map.clone();
                scope_tree.method_defining_class = method_defining_map.clone();
                scope_tree.fn_return_types = fn_return_types_map.clone();
                scope_tree.fn_return_ptr_kinds = fn_return_ptr_kinds_map.clone();
                scope_tree.fn_curried_return_types = fn_curried_return_types_map.clone();
                scope_tree.fn_tuple_return_types = fn_tuple_return_types_map.clone();
                scope_tree.method_return_types = method_return_types_map.clone();
                let mut deferred_frees: Vec<CleanupOp> = Vec::new();

                let self_ptr = format!("%self_ptr_{}", temp_counter);
                temp_counter += 1;
                functions_ir.push_str(&format!("  {} = bitcast i8* %self_raw to %struct.{}*\n", self_ptr, cls.name));
                let self_alloca = format!("%self_alloca_{}", temp_counter);
                temp_counter += 1;
                functions_ir.push_str(&format!("  {} = alloca %struct.{}*\n", self_alloca, cls.name));
                functions_ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls.name, self_ptr, cls.name, self_alloca));

                scope_tree.insert("self".to_string(), SymbolBinding {
                    name: "self".to_string(),
                    llvm_ptr: self_alloca.clone(),
                    type_hint: format!("%struct.{}*", cls.name),
                    is_scoped: true,
                });
                scope_tree.insert("this".to_string(), SymbolBinding {
                    name: "this".to_string(),
                    llvm_ptr: self_alloca.clone(),
                    type_hint: format!("%struct.{}*", cls.name),
                    is_scoped: true,
                });
                scope_tree.insert("cls".to_string(), SymbolBinding {
                    name: "cls".to_string(),
                    llvm_ptr: self_alloca,
                    type_hint: format!("%struct.{}*", cls.name),
                    is_scoped: true,
                });

                for (p_idx, param) in method.params.iter().skip(skip_count).enumerate() {
                    let reg_src = if p_idx == 0 {
                        "%len".to_string()
                    } else {
                        format!("%arg{}", p_idx + 1)
                    };
                    Self::bind_abi_param_i64(
                        &mut functions_ir,
                        &mut scope_tree,
                        &mut temp_counter,
                        param,
                        &reg_src,
                        &class_fields_map,
                        &string_constants,
                    );
                }

                let method_fn_name = format!("{}_{}", cls.name, method.name);
                *self.active_ai_ctx.borrow_mut() = Some(ActiveFnAiContext {
                    enclosing_fn: method_fn_name.clone(),
                    return_type: "i64".to_string(),
                    struct_defs: struct_defs.clone(),
                    temp,
                    ecosystem_context: ecosystem_context.clone(),
                    import_chain: import_chain.clone(),
                    dynamic_declarations: HashSet::new(),
                    escalation_depth: 0,
                });

                let lower_res = self.lower_statements(
                    &method.body,
                    &mut functions_ir,
                    &mut scope_tree,
                    &mut deferred_frees,
                    &string_constants,
                    &mut cur_str_idx,
                    &mut lbl_counter,
                    &mut temp_counter,
                    &method_fn_name,
                    &struct_defs,
                    temp,
                    ecosystem_context.as_deref(),
                    &import_chain,
                    &mut dynamic_declarations,
                    true,
                );

                if let Some(ctx) = self.active_ai_ctx.borrow_mut().take() {
                    dynamic_declarations.extend(ctx.dynamic_declarations);
                }
                for (cname, fmap) in &scope_tree.class_field_types {
                    let entry = class_field_types_map.entry(cname.clone()).or_default();
                    for (k, v) in fmap {
                        entry.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }

                match lower_res {
                    Ok(custom_ret) => {
                        if !custom_ret {
                            Self::emit_cleanups(&mut functions_ir, &deferred_frees, &mut temp_counter, "Method Exit");
                            functions_ir.push_str("  ret i64 0\n");
                        }
                        functions_ir.push_str("}\n\n");
                        let hoisted = Self::hoist_allocas(&functions_ir[method_start_len..]);
                        functions_ir.truncate(method_start_len);
                        functions_ir.push_str(&hoisted);
                        functions_ir.push('\n');
                    }
                    Err(e) => {
                        if is_reachable {
                            return Err(e);
                        } else {
                            functions_ir.truncate(method_start_len);
                            cur_str_idx = method_saved_str_idx;
                            lbl_counter = method_saved_lbl;
                            temp_counter = method_saved_temp;
                            functions_ir.push_str(&format!(
                                "define i64 @__omni_method_{}_{}(i8* %self_raw, {}) {{\nentry:\n  ret i64 0\n}}\n\n",
                                cls.name, method.name, m_param_sig
                            ));
                        }
                    }
                }
            }
        }

        // 2. Functions (namespaced as `i64 @__omni_fn_<name>(i64 x max(6, arity))` or `i32 @__omni_fn_main(i8*, i64)`)
        for func in &module.functions {
            let fn_arity = func.params.len().max(6);
            let fn_param_sig = (0..fn_arity)
                .map(|i| format!("i64 %arg{}", i))
                .collect::<Vec<_>>()
                .join(", ");
            let tramp_zero_tail = if fn_arity > 1 {
                format!(", {}", vec!["i64 -581005069656919554"; fn_arity - 1].join(", "))
            } else {
                String::new()
            };
            let mut pack_loads = String::new();
            let mut pack_args = Vec::with_capacity(fn_arity);
            for idx in 0..fn_arity {
                if idx < 6 {
                    pack_loads.push_str(&format!(
                        "  %p{0} = getelementptr inbounds i64, i64* %p64, i64 {0}\n  %a{0} = load i64, i64* %p{0}\n",
                        idx
                    ));
                    pack_args.push(format!("i64 %a{}", idx));
                } else {
                    pack_args.push("i64 -581005069656919554".to_string());
                }
            }
            let pack_args_str = pack_args.join(", ");

            let is_reachable = reachable_symbols.contains(&func.name) || self.is_user_fn(&func.name);
            let func_start_len = functions_ir.len();
            let func_saved_str_idx = cur_str_idx;
            let func_saved_lbl = lbl_counter;
            let func_saved_temp = temp_counter;

            if !is_reachable {
                if func.name == "main" {
                    has_user_main = true;
                    functions_ir.push_str(
                        "define i32 @__omni_fn_main(i8* %data, i64 %len) {\nentry:\n  ret i32 0\n}\n\n",
                    );
                } else {
                    functions_ir.push_str(&format!(
                        "define i64 @__omni_fn_{0}({1}) {{\nentry:\n  ret i64 %arg0\n}}\n\ndefine void @__omni_trampoline_{0}(i8* %raw_arg) {{\nentry:\n  %raw_int = ptrtoint i8* %raw_arg to i64\n  %res = call i64 @__omni_fn_{0}(i64 %raw_int{2})\n  ret void\n}}\n\ndefine void @__omni_trampoline_pack_{0}(i8* %raw_pack) {{\nentry:\n  %p64 = bitcast i8* %raw_pack to i64*\n{3}  call void @free(i8* %raw_pack)\n  %res = call i64 @__omni_fn_{0}({4})\n  ret void\n}}\n\n",
                        func.name, fn_param_sig, tramp_zero_tail, pack_loads, pack_args_str
                    ));
                }
                continue;
            }

            if func.name == "main" {
                has_user_main = true;
                functions_ir.push_str(
                    "define i32 @__omni_fn_main(i8* %data, i64 %len) {\nentry:\n",
                );
            } else {
                functions_ir.push_str(&format!(
                    "define i64 @__omni_fn_{}({}) {{\nentry:\n",
                    func.name, fn_param_sig
                ));
            }

            let mut scope_tree = ScopeTree::new();
            scope_tree.class_fields = class_fields_map.clone();
            scope_tree.class_field_types = class_field_types_map.clone();
            scope_tree.class_field_order = class_field_order_map.clone();
            scope_tree.class_decorators = class_decorators_map.clone();
            scope_tree.class_properties = class_properties_map.clone();
            scope_tree.class_type_ids = class_type_ids_map.clone();
            scope_tree.c_module_aliases = c_module_aliases_set.clone();
            scope_tree.valid_extmod_prefixes = valid_extmod_prefixes.clone();
            scope_tree.has_open_ns_import = has_open_ns_import;
            scope_tree.enum_variants = enum_variants_map.clone();
            scope_tree.known_functions = known_functions_set.clone();
            scope_tree.extern_fns = extern_fns_map.clone();
            scope_tree.fn_param_defs = fn_param_defs_map.clone();
            scope_tree.method_param_defs = method_param_defs_map.clone();
            scope_tree.method_defining_class = method_defining_map.clone();
            scope_tree.fn_return_types = fn_return_types_map.clone();
            scope_tree.fn_return_ptr_kinds = fn_return_ptr_kinds_map.clone();
            scope_tree.fn_curried_return_types = fn_curried_return_types_map.clone();
            scope_tree.fn_tuple_return_types = fn_tuple_return_types_map.clone();
            scope_tree.method_return_types = method_return_types_map.clone();
            let mut deferred_frees: Vec<CleanupOp> = Vec::new();

            // Bind function parameters with 64-bit / float / pointer ABI awareness
            if func.name == "main" {
                for (i, param) in func.params.iter().enumerate() {
                    let p_name_lower = param.name.to_lowercase();
                    let is_chan = param.type_hint.as_deref().map(|th| th.contains("chan")).unwrap_or(false)
                        || p_name_lower == "ch" || p_name_lower == "chan" || p_name_lower.starts_with("chan_") || p_name_lower.ends_with("_chan") || p_name_lower.starts_with("ch_");
                    let is_slice = param.type_hint.as_deref().map(|th| th.contains("slice") || th.contains("list")).unwrap_or(false)
                        || p_name_lower == "slice" || p_name_lower == "items" || p_name_lower == "arr" || p_name_lower == "list"
                        || p_name_lower == "args" || p_name_lower == "argv"
                        || p_name_lower.starts_with("items_") || p_name_lower.starts_with("arr_") || p_name_lower.starts_with("slice_") || p_name_lower.ends_with("_arr") || p_name_lower.ends_with("_slice") || p_name_lower.ends_with("_list");
                    let is_map = param.type_hint.as_deref().map(|th| th.contains("map") || th.contains("dict")).unwrap_or(false)
                        || p_name_lower == "map" || p_name_lower == "dict" || p_name_lower.starts_with("map_") || p_name_lower.starts_with("dict_") || p_name_lower.ends_with("_map") || p_name_lower.ends_with("_dict");
                    let is_int = param.type_hint.as_deref().map(|th| th == "int" || th == "i32" || th == "i64" || th == "int64").unwrap_or(false)
                        || (!is_chan && !is_slice && !is_map && (p_name_lower == "val" || p_name_lower == "num" || p_name_lower == "count" || p_name_lower == "idx" || p_name_lower == "n" || p_name_lower == "x" || p_name_lower == "y" || p_name_lower == "z" || p_name_lower == "argc"));

                    if i == 0 {
                        if is_int {
                            let cast_i64 = format!("%param_int64_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = ptrtoint i8* %data to i64\n", cast_i64));
                            let ptr = format!("%param_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = alloca i64\n", ptr));
                            functions_ir.push_str(&format!("  store i64 {}, i64* {}\n", cast_i64, ptr));
                            scope_tree.insert(
                                param.name.clone(),
                                SymbolBinding {
                                    name: param.name.clone(),
                                    llvm_ptr: ptr,
                                    type_hint: "i64".to_string(),
                                    is_scoped: true,
                                },
                            );
                        } else {
                            let type_hint = if is_map {
                                "i8* (omni_map)".to_string()
                            } else if is_slice {
                                scope_tree.collection_elem_types.insert(param.name.clone(), "i8* (string)".to_string());
                                "i8* (omni_slice)".to_string()
                            } else {
                                "i8* (omni_chan)".to_string()
                            };
                            let ptr = format!("%param_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                            functions_ir.push_str(&format!("  store i8* %data, i8** {}\n", ptr));
                            scope_tree.insert(
                                param.name.clone(),
                                SymbolBinding {
                                    name: param.name.clone(),
                                    llvm_ptr: ptr,
                                    type_hint,
                                    is_scoped: true,
                                },
                            );
                        }
                    } else if i == 1 {
                        let ptr = format!("%param_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = alloca i64\n", ptr));
                        functions_ir.push_str(&format!("  store i64 %len, i64* {}\n", ptr));
                        scope_tree.insert(
                            param.name.clone(),
                            SymbolBinding {
                                name: param.name.clone(),
                                llvm_ptr: ptr,
                                type_hint: "i64".to_string(),
                                is_scoped: true,
                            },
                        );
                    }
                }
            } else {
                for (i, param) in func.params.iter().enumerate() {
                    let arg_src = format!("%arg{}", i);
                    Self::bind_abi_param_i64(
                        &mut functions_ir,
                        &mut scope_tree,
                        &mut temp_counter,
                        param,
                        &arg_src,
                        &class_fields_map,
                        &string_constants,
                    );
                }
            }

            *self.active_ai_ctx.borrow_mut() = Some(ActiveFnAiContext {
                enclosing_fn: func.name.clone(),
                return_type: if func.name == "main" { "i32".to_string() } else { "i64".to_string() },
                struct_defs: struct_defs.clone(),
                temp,
                ecosystem_context: ecosystem_context.clone(),
                import_chain: import_chain.clone(),
                dynamic_declarations: HashSet::new(),
                escalation_depth: 0,
            });

            let lower_res = self.lower_statements(
                &func.body,
                &mut functions_ir,
                &mut scope_tree,
                &mut deferred_frees,
                &string_constants,
                &mut cur_str_idx,
                &mut lbl_counter,
                &mut temp_counter,
                &func.name,
                &struct_defs,
                temp,
                ecosystem_context.as_deref(),
                &import_chain,
                &mut dynamic_declarations,
                true,
            );

            if let Some(ctx) = self.active_ai_ctx.borrow_mut().take() {
                dynamic_declarations.extend(ctx.dynamic_declarations);
            }

            match lower_res {
                Ok(custom_ret) => {
                    if !custom_ret {
                        if func.name == "main" && !self.target_triple.starts_with("thumb") {
                            functions_ir.push_str("  call void @__omni_fiber_join_all()\n");
                            functions_ir.push_str("  call void @__omni_thread_join_all()\n");
                        }
                        Self::emit_cleanups(&mut functions_ir, &deferred_frees, &mut temp_counter, "Function Exit");
                        if func.name == "main" {
                            functions_ir.push_str("  ret i32 0\n");
                        } else {
                            functions_ir.push_str("  ret i64 0\n");
                        }
                    }
                    functions_ir.push_str("}\n\n");
                    let hoisted = Self::hoist_allocas(&functions_ir[func_start_len..]);
                    functions_ir.truncate(func_start_len);
                    functions_ir.push_str(&hoisted);
                    functions_ir.push('\n');
                    if self.is_user_fn(&func.name) {
                        user_emitted_ir.push_str(&hoisted);
                        user_emitted_ir.push('\n');
                    }
                }
                Err(e) => {
                    if func.name == "main" || self.is_user_fn(&func.name) {
                        return Err(e);
                    } else {
                        functions_ir.truncate(func_start_len);
                        cur_str_idx = func_saved_str_idx;
                        lbl_counter = func_saved_lbl;
                        temp_counter = func_saved_temp;
                        functions_ir.push_str(&format!(
                            "define i64 @__omni_fn_{}({}) {{\nentry:\n  ret i64 0\n}}\n\n",
                            func.name, fn_param_sig
                        ));
                    }
                }
            }

            // Static Trampoline for Fiber Concurrency (100% C-ABI compliant void (*)(void*))
            if func.name == "main" {
                functions_ir.push_str(&format!(
                    "define void @__omni_trampoline_{}(i8* %raw_arg) {{\nentry:\n  %res = call i32 @__omni_fn_{}(i8* %raw_arg, i64 0)\n  ret void\n}}\n\n",
                    func.name, func.name
                ));
            } else {
                functions_ir.push_str(&format!(
                    "define void @__omni_trampoline_{0}(i8* %raw_arg) {{\nentry:\n  %raw_int = ptrtoint i8* %raw_arg to i64\n  %res = call i64 @__omni_fn_{0}(i64 %raw_int{1})\n  ret void\n}}\n\ndefine void @__omni_trampoline_pack_{0}(i8* %raw_pack) {{\nentry:\n  %p64 = bitcast i8* %raw_pack to i64*\n{2}  call void @free(i8* %raw_pack)\n  %res = call i64 @__omni_fn_{0}({3})\n  ret void\n}}\n\n",
                    func.name, tramp_zero_tail, pack_loads, pack_args_str
                ));
            }
        }

        // Emit lambda-lifted local closures & polymorphic virtual dispatchers before symbol harvesting
        let lifted_clos = self.lifted_closures_ir.borrow().clone();
        if !lifted_clos.is_empty() {
            functions_ir.push_str(&lifted_clos);
            user_emitted_ir.push_str(&lifted_clos);
        }
        {
            let vd_map = self.vdispatch_emitted.borrow();
            let mut vd_keys: Vec<_> = vd_map.keys().cloned().collect();
            vd_keys.sort();
            for k in vd_keys {
                if let Some(vd_def) = vd_map.get(&k) {
                    functions_ir.push_str(vd_def);
                }
            }
        }

        // Harvest any foreign C or un-emitted method/function symbols directly called in functions_ir
        let runtime_headers = omni_runtime::runtime_llvm_declarations_for_triple(&self.target_triple);
        let call_c_re = Regex::new(r#"call\s+[a-zA-Z0-9_*]+\s+@([a-zA-Z0-9_]+)\("#).unwrap();
        let mut missing_omni_stubs = Vec::new();
        let mut seen_called = HashSet::new();

        let mut synth_scope = ScopeTree::new();
        synth_scope.class_fields = class_fields_map.clone();
        synth_scope.class_field_types = class_field_types_map.clone();
        synth_scope.class_field_order = class_field_order_map.clone();
        synth_scope.class_decorators = class_decorators_map.clone();
        synth_scope.class_properties = class_properties_map.clone();
        synth_scope.class_type_ids = class_type_ids_map.clone();
        synth_scope.method_defining_class = method_defining_map.clone();
        synth_scope.method_param_defs = method_param_defs_map.clone();
        synth_scope.known_functions = known_functions_set.clone();
        synth_scope.extern_fns = extern_fns_map.clone();
        synth_scope.fn_param_defs = fn_param_defs_map.clone();
        synth_scope.fn_return_types = fn_return_types_map.clone();
        synth_scope.fn_return_ptr_kinds = fn_return_ptr_kinds_map.clone();
        synth_scope.c_module_aliases = c_module_aliases_set.clone();
        synth_scope.valid_extmod_prefixes = valid_extmod_prefixes.clone();
        synth_scope.has_open_ns_import = has_open_ns_import;

        for cap in call_c_re.captures_iter(&functions_ir) {
            let sym = cap[1].to_string();
            if !seen_called.insert(sym.clone()) {
                continue;
            }
            if sym.starts_with("__omni_method_") {
                if !functions_ir.contains(&format!("define i64 @{}(", sym))
                    && !functions_ir.contains(&format!("define i32 @{}(", sym))
                {
                    let rest = &sym["__omni_method_".len()..];
                    let mut matched_cls: Option<(&str, &str)> = None;
                    for cname in class_map.keys() {
                        let prefix = format!("{}_", cname);
                        if let Some(mname) = rest.strip_prefix(&prefix) {
                            if matched_cls
                                .map(|(prev, _)| cname.len() > prev.len())
                                .unwrap_or(true)
                            {
                                matched_cls = Some((cname.as_str(), mname));
                            }
                        }
                    }
                    let synthesized_opt = matched_cls.and_then(|(cname, mname)| {
                        self.synthesize_unresolved_struct_method(
                            &sym,
                            cname,
                            mname,
                            &synth_scope,
                            &struct_defs,
                            ecosystem_context.as_deref(),
                            &import_chain,
                            temp,
                            &mut temp_counter,
                        )
                    });
                    if let Some(synth_fn) = synthesized_opt {
                        missing_omni_stubs.push(format!("{}\n", Self::hoist_allocas(&synth_fn)));
                    } else if module.imports.is_empty() || user_emitted_ir.contains(&format!("@{}(", sym)) {
                        if let Some((cname, mname)) = matched_cls {
                            let mut flds: Vec<&str> = synth_scope
                                .class_fields
                                .get(cname)
                                .map(|m| m.keys().map(|s| s.as_str()).collect())
                                .unwrap_or_default();
                            flds.sort();
                            let mut mths: Vec<&str> = synth_scope
                                .method_defining_class
                                .keys()
                                .filter_map(|(c, m)| if c == cname { Some(m.as_str()) } else { None })
                                .collect();
                            mths.sort();
                            let mut cands: Vec<(usize, String)> = mths
                                .iter()
                                .map(|m| (ScopeTree::edit_distance(mname, m), format!("{}()", m)))
                                .chain(flds.iter().map(|f| (ScopeTree::edit_distance(mname, f), format!("field '{}'", f))))
                                .filter(|(d, _)| *d <= 4)
                                .collect();
                            cands.sort_by(|a, b| a.0.cmp(&b.0));
                            let hint = if cands.is_empty() {
                                "none".to_string()
                            } else {
                                cands.into_iter().take(3).map(|(_, s)| s).collect::<Vec<_>>().join(", ")
                            };
                            anyhow::bail!(
                                "[OMNIC SEMANTIC ERROR] Unresolved method '{}.{}':\n  Evidence: '{}' defines fields [{}] and methods [{}].\n  Closest matches: [{}]",
                                cname, mname, cname, flds.join(", "), mths.join(", "), hint
                            );
                        }
                        anyhow::bail!("[OMNIC SEMANTIC ERROR] Unresolved method symbol '{}'", sym);
                    } else {
                        missing_omni_stubs.push(format!(
                            "define i64 @{}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 0\n}}\n\n",
                            sym
                        ));
                    }
                }
            } else if sym.starts_with("__omni_fn_") {
                if !functions_ir.contains(&format!("define i64 @{}(", sym))
                    && !functions_ir.contains(&format!("define i32 @{}(", sym))
                {
                    let fn_short = &sym["__omni_fn_".len()..];
                    let stub_n = synth_scope.fn_abi_arity(fn_short);
                    let stub_sig = (0..stub_n).map(|i| format!("i64 %arg{}", i)).collect::<Vec<_>>().join(", ");
                    missing_omni_stubs.push(format!(
                        "define i64 @{}({}) {{\nentry:\n  ret i64 0\n}}\n\n",
                        sym, stub_sig
                    ));
                }
            } else if sym.starts_with("__omni_extmod_") {
                if !functions_ir.contains(&format!("define i64 @{}(", sym))
                    && !functions_ir.contains(&format!("define weak i64 @{}(", sym))
                {
                    let ext_tail = sym
                        .strip_prefix("__omni_extmod__")
                        .or_else(|| sym.strip_prefix("__omni_extmod_"))
                        .unwrap_or(&sym);
                    let (ext_rec, ext_mth) = ext_tail.split_once("__").unwrap_or((ext_tail, ""));
                    if module.imports.is_empty() {
                        anyhow::bail!(
                            "[OMNIC SEMANTIC ERROR] Undefined module or foreign call '{}.{}' (module declares 0 imports)",
                            ext_rec,
                            ext_mth
                        );
                    } else if !has_open_ns_import
                        && !valid_extmod_prefixes.contains(ext_rec)
                        && user_emitted_ir.contains(&format!("@{}(", sym))
                    {
                        let mut imp_list: Vec<&str> = valid_extmod_prefixes.iter().map(|s| s.as_str()).collect();
                        imp_list.sort();
                        anyhow::bail!(
                            "[OMNIC SEMANTIC ERROR] Undefined module or receiver '{}' in call '{}.{}':\n  Evidence: module imports [{}].",
                            ext_rec,
                            ext_rec,
                            ext_mth,
                            imp_list.join(", ")
                        );
                    } else {
                        missing_omni_stubs.push(format!(
                            "define weak i64 @{}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {{\nentry:\n  ret i64 %a0\n}}\n\n",
                            sym
                        ));
                    }
                }
            } else if !sym.starts_with("__omni_")
                && sym != "_setjmp"
                && sym != "malloc"
                && sym != "free"
                && sym != "printf"
                && sym != "sleep"
                && sym != "usleep"
                && sym != "exit"
                && sym != "strcmp"
                && !runtime_headers.contains(&format!("@{}(", sym))
                && !known_functions_set.contains(&sym)
                && !functions_ir.contains(&format!("define i64 @{}(", sym))
                && !functions_ir.contains(&format!("define i32 @{}(", sym))
                && !dynamic_declarations.iter().any(|d| d.contains(&format!("@{}(", sym)))
            {
                if let Some((param_tys, ret_ty)) = extern_fns_map.get(&sym) {
                    let ret_llvm = match ret_ty {
                        Some(IrType::Ptr) => "i8*",
                        Some(IrType::F64) => "double",
                        None => "void",
                        Some(IrType::I32) => "i32",
                        Some(IrType::I1) => "i1",
                        Some(_) => "i64",
                    };
                    let params_llvm = vec!["i64"; param_tys.len()].join(", ");
                    dynamic_declarations.insert(format!("declare {} @{}({})", ret_llvm, sym, params_llvm));
                } else if NATIVE_C_LIB_I64_FNS.contains(&sym.as_str()) {
                    dynamic_declarations.insert(format!("declare i64 @{}(...)", sym));
                } else if NATIVE_C_LIB_FNS.contains(&sym.as_str()) {
                    dynamic_declarations.insert(format!("declare i32 @{}(...)", sym));
                } else if sym == "not" {
                    missing_omni_stubs.push(
                        "define weak i64 @not(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %c = icmp eq i64 %a0, 0\n  %r = zext i1 %c to i64\n  ret i64 %r\n}\n\n".to_string()
                    );
                } else if sym == "pow" {
                    missing_omni_stubs.push(
                        "define weak i64 @pow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %raw = call i64 @__omni_ipow64(i64 %a0, i64 %a1)\n  %has_mod = icmp sgt i64 %a2, 0\n  br i1 %has_mod, label %do_mod, label %done\ndo_mod:\n  %m = srem i64 %raw, %a2\n  br label %done\ndone:\n  %res = phi i64 [ %m, %do_mod ], [ %raw, %entry ]\n  ret i64 %res\n}\n\n".to_string()
                    );
                } else if sym == "max" {
                    missing_omni_stubs.push(
                        "define weak i64 @max(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %c = icmp sgt i64 %a0, %a1\n  %s = select i1 %c, i64 %a0, i64 %a1\n  ret i64 %s\n}\n\n".to_string()
                    );
                } else if sym == "min" {
                    missing_omni_stubs.push(
                        "define weak i64 @min(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %c = icmp slt i64 %a0, %a1\n  %s = select i1 %c, i64 %a0, i64 %a1\n  ret i64 %s\n}\n\n".to_string()
                    );
                } else if sym == "divmod" {
                    missing_omni_stubs.push(
                        "define weak i64 @divmod(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %z = icmp eq i64 %a1, 0\n  %d = select i1 %z, i64 1, i64 %a1\n  %q = sdiv i64 %a0, %d\n  %m = srem i64 %a0, %d\n  %s = call i8* @__omni_slice_create(i64 2)\n  call void @__omni_slice_append(i8* %s, i64 %q)\n  call void @__omni_slice_append(i8* %s, i64 %m)\n  call void @__omni_c_return_ptr(i8* %s)\n  %r = ptrtoint i8* %s to i64\n  ret i64 %r\n}\n\n".to_string()
                    );
                } else if local_foreign_syms.contains(&sym) || has_unaliased_c_import {
                    missing_omni_stubs.push(format!(
                        "define weak i64 @{}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {{\nentry:\n  ret i64 %a0\n}}\n\n",
                        sym
                    ));
                } else if module.imports.is_empty() || user_emitted_ir.contains(&format!("@{}(", sym)) {
                    let mut known_vec: Vec<&str> = known_functions_set.iter().map(|s| s.as_str()).collect();
                    known_vec.sort();
                    let mut cands: Vec<(usize, &str)> = known_vec
                        .iter()
                        .map(|k| (ScopeTree::edit_distance(&sym, k), *k))
                        .filter(|(d, _)| *d <= 4)
                        .collect();
                    cands.sort_by(|a, b| a.0.cmp(&b.0));
                    let closest = if cands.is_empty() {
                        "none".to_string()
                    } else {
                        cands.into_iter().take(3).map(|(_, s)| s).collect::<Vec<_>>().join(", ")
                    };
                    anyhow::bail!(
                        "[OMNIC SEMANTIC ERROR] Undefined function or symbol '{}':\n  Evidence: module defines functions [{}].\n  Closest matches: [{}]",
                        sym,
                        known_vec.join(", "),
                        closest
                    );
                } else {
                    missing_omni_stubs.push(format!(
                        "define weak i64 @{}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {{\nentry:\n  ret i64 %a0\n}}\n\n",
                        sym
                    ));
                }
            }
        }
        for stub in missing_omni_stubs {
            functions_ir.push_str(&stub);
        }

        // Assemble full Module IR
        let mut ir = String::new();
        ir.push_str("; ModuleID = '");
        ir.push_str(&module.name);
        ir.push_str("'\nsource_filename = \"");
        ir.push_str(&module.name);
        ir.push_str(".omni\"\ntarget triple = \"");
        ir.push_str(&self.target_triple);
        ir.push_str("\"\n\n");

        // Runtime headers & extern declarations
        ir.push_str("; Standard Runtime & libc Declarations\n");
        ir.push_str("declare i8* @malloc(i64)\n");
        ir.push_str("declare void @free(i8*)\n");
        ir.push_str("declare i32 @printf(i8*, ...)\n");
        ir.push_str(omni_runtime::runtime_llvm_declarations_for_triple(&self.target_triple));
        ir.push_str("\n");

        if !dynamic_declarations.is_empty() {
            ir.push_str("; Dynamically Harvested External Declarations\n");
            let mut sorted_decls: Vec<_> = dynamic_declarations.into_iter().collect();
            sorted_decls.sort();
            for decl in sorted_decls {
                ir.push_str(&decl);
                ir.push('\n');
            }
            ir.push('\n');
        }

        // Global string constants
        for (const_name, text, len) in &string_constants {
            let (llvm_escaped, _) = Self::escape_llvm_string(text);
            ir.push_str(&format!(
                "{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                const_name, len, llvm_escaped
            ));
        }
        ir.push_str("\n");

        // Struct definitions
        ir.push_str(&struct_defs_ir);

        // Function definitions
        ir.push_str(&functions_ir);

        // 3. Entrypoint @main
        ir.push_str("define i32 @main(i32 %argc, i8** %argv) {\nentry:\n");

        if has_user_main {
            if main_param_count > 0 && !self.target_triple.starts_with("thumb") {
                ir.push_str("  %argc64 = sext i32 %argc to i64\n");
                ir.push_str("  %argv_sl = call i8* @__omni_slice_create(i64 %argc64)\n");
                ir.push_str("  %argv_idx = alloca i64\n");
                ir.push_str("  store i64 0, i64* %argv_idx\n");
                ir.push_str("  br label %argv_loop_cond\n\n");
                ir.push_str("argv_loop_cond:\n");
                ir.push_str("  %ai = load i64, i64* %argv_idx\n");
                ir.push_str("  %acmp = icmp slt i64 %ai, %argc64\n");
                ir.push_str("  br i1 %acmp, label %argv_loop_body, label %argv_loop_done\n\n");
                ir.push_str("argv_loop_body:\n");
                ir.push_str("  %aptr = getelementptr inbounds i8*, i8** %argv, i64 %ai\n");
                ir.push_str("  %astr = load i8*, i8** %aptr\n");
                ir.push_str("  %aint = ptrtoint i8* %astr to i64\n");
                ir.push_str("  call void @__omni_slice_append(i8* %argv_sl, i64 %aint)\n");
                ir.push_str("  %anext = add i64 %ai, 1\n");
                ir.push_str("  store i64 %anext, i64* %argv_idx\n");
                ir.push_str("  br label %argv_loop_cond\n\n");
                ir.push_str("argv_loop_done:\n");
                ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* %argv_sl, i64 %argc64)\n");
                ir.push_str("  call void @__omni_slice_destroy(i8* %argv_sl)\n");
                ir.push_str("  ret i32 %exit_code\n");
            } else {
                ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* null, i64 0)\n");
                ir.push_str("  ret i32 %exit_code\n");
            }
        } else if let Some(f) = module.functions.first() {
            let f_arity = f.params.len().max(6);
            let z_args = vec!["i64 -581005069656919554"; f_arity].join(", ");
            ir.push_str(&format!(
                "  %res = call i64 @__omni_fn_{}({})\n",
                f.name, z_args
            ));
            if !self.target_triple.starts_with("thumb") {
                ir.push_str("  call void @__omni_fiber_join_all()\n");
                ir.push_str("  call void @__omni_thread_join_all()\n");
            }
            ir.push_str("  ret i32 0\n");
        } else {
            ir.push_str("  ret i32 0\n");
        }

        ir.push_str("}\n");

        let sem_errs = self.pending_semantic_errors.borrow();
        if !sem_errs.is_empty() {
            anyhow::bail!("{}", sem_errs.join("\n"));
        }

        Ok(ir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_engine::MockSynthesisEngine;
    use omni_parser::OmniParser;
    use omni_verify::LlvmVerifier;

    #[test]
    fn test_codegen_collections_verify() {
        let src = r#"
def main():
    items = [10, 20, 30]
    items.append(40)
    items[0] = 99
    scores = {"alice": 100, "bob": 200}
    scores["carol"] = 300
    for i in len(items):
        print(items[i])
    print(scores["alice"])
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("coll_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("__omni_slice_create"));
        assert!(ir.contains("__omni_slice_append"));
        assert!(ir.contains("__omni_slice_get"));
        assert!(ir.contains("__omni_map_create"));
        assert!(ir.contains("__omni_map_set"));
        assert!(ir.contains("__omni_map_get"));
        assert!(ir.contains("__omni_slice_destroy"));
        assert!(ir.contains("__omni_map_destroy"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier.verify_ir(&ir).expect("LLVM IR verification failed for collections");
    }

    #[test]
    fn test_codegen_channels_and_spawn_verify() {
        let src = r#"
def worker(ch):
    ch <- 42
    return 0

def main():
    ch := make(chan int, 4)
    go worker(ch)
    val := <-ch
    return val
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("chan_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("__omni_chan_make"));
        assert!(ir.contains("__omni_chan_send_int"));
        assert!(ir.contains("__omni_chan_recv_int"));
        assert!(ir.contains("__omni_go_spawn"));
        assert!(ir.contains("__omni_chan_destroy"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier.verify_ir(&ir).expect("LLVM IR verification failed for channels and spawn");
    }

    #[test]
    fn test_codegen_64bit_floats_unwind_and_multi_returns_verify() {
        let src = r#"
fn add_huge(a: i64, b: i64) -> i64 {
    return a + b
}

fn circle_area(r: f64) -> f64 {
    return 3.14159 * r * r
}

func safe_div(a int64, b int64) (int64, error) {
    if b == 0 {
        return 0, errors.New("div_zero")
    }
    return a / b, nil
}

def boom(x):
    if x < 0:
        raise ValueError("bad_input")
    return x

def main():
    huge = add_huge(5_000_000_000, 2_500_000_000)
    area = circle_area(2.0)
    print(huge, area)
    try:
        y = boom(-1)
    except ValueError as err:
        print(err)
    finally:
        print("done")
    q, e := safe_div(100, 5)
    if e == nil:
        print(q)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("backbone_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("define i64 @__omni_fn_add_huge("));
        assert!(ir.contains("fmul double"));
        assert!(ir.contains("@__omni_print_float"));
        assert!(ir.contains("@__omni_try_push"));
        assert!(ir.contains("@_setjmp"));
        assert!(ir.contains("@__omni_throw"));
        assert!(ir.contains("declare i32 @__omni_catch_matches(i8*)"));
        assert!(ir.contains("call i32 @__omni_catch_matches"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier
            .verify_ir(&ir)
            .expect("LLVM IR verification failed for 64-bit, double, setjmp unwind, and multi-return");
    }

    #[test]
    fn test_codegen_pratt_element_types_and_unannotated_fields_verify() {
        let src = r#"
class Amplifier:
    def __init__(self):
        self.gain = 2.5

    def scale(self, x):
        return x * self.gain

fn bump(v: i64) -> i64 {
    return v + 1
}

def main():
    bit_prec = 1 | 2 & 0
    pow_mul = 2 * 3 ** 2
    pow_assoc = 2 ** 3 ** 2
    n = 4; dyn_pow = 3 ** n
    dual_calls = bump(4) + bump(9)

    floats = [1.5, 2.5, 3.0]
    f_sum = 0.0
    for x in floats:
        f_sum = f_sum + x

    scores = {"pi": 3.25, "e": 2.75}
    m_sum = scores["pi"] + scores["e"]

    grid = [[10, 20], [30, 40]]
    grid[1][0] = 35
    grid_pick = grid[1][0] + grid[0][1]

    amp = new Amplifier<Double>()
    scaled = amp.scale(4)

    print(bit_prec, pow_mul, pow_assoc, dyn_pow, dual_calls, f_sum, m_sum, grid_pick, scaled)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("pratt_elem_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("@__omni_ipow64"));
        assert!(ir.contains("bitcast i64") && ir.contains("to double"));
        assert!(ir.contains("fadd double"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier
            .verify_ir(&ir)
            .expect("LLVM IR verification failed for Pratt + element types + unannotated fields");
    }

    #[test]
    fn test_universal_strings_as_arrays_and_abductive_collections_verify() {
        let src = r#"
fn dyn_len(seq) -> i64 {
    return len(seq)
}

def main():
    s = "OmniScript"
    n1 = len(s)
    n2 = len("hello")
    n3 = s.len()
    n4 = s.length
    n5 = dyn_len(s)

    first_ch = s[0]
    last_ch = s[-1]
    mid_sub = s[4:10]
    rev_sub = "abcd"[::-1]
    rust_sub = s[0..4]

    ascii_sum = 0
    for ch in "ABC":
        ascii_sum = ascii_sum + ord(ch)

    has_script = 1 if "Script" in s else 0
    no_xyz = 1 if "xyz" not in s else 0
    shout = "  hi omni  ".strip().upper().replace("HI", "YO")
    parts = "red,green,blue".split(",")
    second_color = parts[1]

    nums = [10, 20]
    nums.push(30)
    sub_nums = nums[1:3]
    has_30 = 1 if 30 in nums else 0

    tbl = {"a": 7}
    tbl.put("b", 13)
    has_b = 1 if "b" in tbl else 0

    print(n1, n2, n3, n4, n5)
    print(first_ch + last_ch + ":" + mid_sub + ":" + rev_sub + ":" + rust_sub)
    print(ascii_sum, ord(chr(90)), has_script, no_xyz)
    print(shout + ":" + second_color, len(parts), len(sub_nums), sub_nums[1], has_30, tbl.get("b"), has_b)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("strings_as_arrays_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier.verify_ir(&ir).expect("LLVM IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT execution failed");
        assert!(status.success(), "JIT exited non-zero: {}", stderr);
        assert!(out.contains("10 5 10 10 10"), "Unexpected lengths in output:\n{}", out);
        assert!(out.contains("Ot:Script:dcba:Omni"), "Unexpected string slices in output:\n{}", out);
        assert!(out.contains("198 90 1 1"), "Unexpected ord/chr/in checks in output:\n{}", out);
        assert!(out.contains("YO OMNI:green 3 2 30 1 13 1"), "Unexpected transform/abductive collection output:\n{}", out);
    }

    #[test]
    fn test_abductive_semantic_error_rejects_hallucinated_calls_and_silent_noops() {
        let parser = OmniParser::new();
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        // 1. Hallucinated bare function in self-contained module must fail loudly with closest match
        let bad_fn_src = r#"
fn compute_checksum(x: i64) -> i64 {
    return x * 31
}
def main():
    y = compute_checksm(10)
    print(y)
    return 0
"#;
        let m1 = parser.parse_module("bad_fn", bad_fn_src);
        let err1 = codegen.generate_module_ir(&m1, 0.0).expect_err("Expected semantic error on typo function");
        let msg1 = err1.to_string();
        assert!(msg1.contains("OMNIC SEMANTIC ERROR") && msg1.contains("compute_checksum"), "Got: {}", msg1);

        // 2. Hallucinated method on user struct must fail loudly with struct field/method evidence
        let bad_method_src = r#"
struct Account {
    pub balance: i64,
}
impl Account {
    pub fn deposit(&self, amt: i64) -> i64 {
        return self.balance + amt
    }
}
fn main() {
    let a = Account { balance: 100 };
    let z = a.depost(50);
    print(z);
}
"#;
        let m2 = parser.parse_module("bad_method", bad_method_src);
        let err2 = codegen.generate_module_ir(&m2, 0.0).expect_err("Expected semantic error on typo method");
        let msg2 = err2.to_string();
        assert!(msg2.contains("OMNIC SEMANTIC ERROR") && msg2.contains("deposit"), "Got: {}", msg2);

        // 3. Post-condition Semantic Obligation Verifier rejects comment-only no-ops and unread live operands
        let env = SsaContextEnvelope {
            target_abi: omni_runtime::host_target_triple().to_string(),
            enclosing_fn: "main".to_string(),
            return_type: "i32".to_string(),
            next_ssa_idx: 4,
            active_symbols: vec![
                SsaVariable {
                    name: "alpha".to_string(),
                    llvm_name: "%var_alpha_1".to_string(),
                    type_hint: "i64".to_string(),
                },
                SsaVariable {
                    name: "out".to_string(),
                    llvm_name: "%var_out_2".to_string(),
                    type_hint: "i64".to_string(),
                },
            ],
            struct_definitions: vec![],
            raw_intent: "out = custom_op(alpha)".to_string(),
            idiom_hint: None,
            anchor_id: Some(1),
            physical_source: Some("out = custom_op(alpha)".to_string()),
            concurrency_mode: None,
            yield_expectation: Some("store 64-bit scalar result to %var_out_2".to_string()),
            action_directive: Some("LOWER_ASSIGNMENT".to_string()),
            ecosystem_context: None,
            lifetime_context: None,
            import_chain: vec![],
        };
        assert!(
            omni_engine::VerifiedSynthesisEngine::verify_semantic_obligations(&env, "  ; [Synthesized generic intent]\n").is_err(),
            "Comment-only no-op must be rejected"
        );
        assert!(
            omni_engine::VerifiedSynthesisEngine::verify_semantic_obligations(&env, "  store i64 42, i64* %var_out_2\n").is_err(),
            "IR ignoring live operand 'alpha' (%var_alpha_1) must be rejected"
        );
        assert!(
            omni_engine::VerifiedSynthesisEngine::verify_semantic_obligations(
                &env,
                "  %t_4 = load i64, i64* %var_alpha_1\n  %t_5 = mul i64 %t_4, 2\n  store i64 %t_5, i64* %var_out_2\n"
            )
            .is_ok(),
            "Valid IR reading %var_alpha_1 and storing to %var_out_2 must pass"
        );
    }

    #[test]
    fn test_exact_arity_defaults_zero_preservation_and_fail_closed_helpers() {
        let src = r#"
fn sum8(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64) -> i64 {
    return a + b + c + d + e + f + g + h
}

class Accumulator:
    def __init__(self, bias=5):
        self.bias = bias

    def combine7(self, a, b, c, d, e, f, g=100):
        return self.bias + a + b + c + d + e + f + g

def with_defaults(x=42, y=100):
    return x + y

def greet(tag: str):
    return "hi_" + tag

def greet_opt(tag="W"):
    return "yo_" + tag

def num_id(n):
    return n

def main():
    s8 = sum8(1, 2, 3, 4, 5, 6, 7, 8)
    acc_def = Accumulator()
    acc_zero = Accumulator(0)
    m7_all = acc_def.combine7(10, 20, 30, 40, 50, 60, 70)
    m7_def = acc_zero.combine7(1, 2, 3, 4, 5, 6)

    d_both = with_defaults()
    d_zero_first = with_defaults(0)
    d_zero_both = with_defaults(0, 0)
    d_kwargs = with_defaults(y=7, x=3)

    nested_str = greet("A") + ":" + str(num_id(99)) + ":" + greet("B") + ":" + greet_opt()
    print(s8, m7_all, m7_def)
    print(d_both, d_zero_first, d_zero_both, d_kwargs)
    print(nested_str)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("arity_defaults_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        let verifier = LlvmVerifier::new().expect("Verifier init failed");
        verifier.verify_ir(&ir).expect("IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT failed");
        assert!(status.success(), "JIT stderr: {}", stderr);
        assert!(out.contains("36 285 121"), "Arity/ctor output mismatch:\n{}", out);
        assert!(out.contains("142 100 0 10"), "Default/zero-arg output mismatch:\n{}", out);
        assert!(out.contains("hi_A:99:hi_B:yo_W"), "Nested TLS return output mismatch:\n{}", out);

        // Non-main helper calling typoed bare function EVEN WITH imports must fail closed
        let bad_helper_with_import = r#"
import 'python' math

def helper(v):
    return compute_checksm(v)

def compute_checksum(v):
    return v * 2

def main():
    print(helper(21))
    return 0
"#;
        let m_bad = parser.parse_module("bad_helper_imp", bad_helper_with_import);
        let err = codegen
            .generate_module_ir(&m_bad, 0.0)
            .expect_err("Expected fail-closed error on typo inside helper even with import");
        assert!(
            err.to_string().contains("OMNIC SEMANTIC ERROR") && err.to_string().contains("compute_checksum"),
            "Unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_perceus_rc_escaping_aggregates_div_zero_and_loop_unwind() {
        let src = r#"
struct Point {
    pub x: i64,
    pub y: i64,
}

fn make_point(px: i64, py: i64) -> Point {
    let pt = Point { x: px, y: py };
    return pt
}

def make_aliased_slice():
    a = [11, 22]
    b = a
    b.append(33)
    return b

def div_probe(a, b):
    try:
        return a / b
    except ZeroDivisionError as err:
        print(err)
        return -999

def main():
    p = make_point(30, 47)
    escaped_sum = p.x + p.y

    s = make_aliased_slice()
    alias2 = s
    alias_sum = alias2[0] + alias2[1] + alias2[2]

    loop_acc = 0
    for i in 8:
        inner = [i, i * 10]
        if i == 2:
            continue
        if i == 5:
            break
        loop_acc = loop_acc + inner[1]

    dz_ok = div_probe(84, 2)
    dz_err = div_probe(10, 0)

    print(escaped_sum, alias_sum, len(alias2), loop_acc, dz_ok, dz_err)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("perceus_escape_div0_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        let verifier = LlvmVerifier::new().expect("Verifier init failed");
        verifier.verify_ir(&ir).expect("IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT failed");
        assert!(status.success(), "JIT stderr: {}", stderr);
        assert!(out.contains("division by zero"), "Expected ZeroDivisionError message:\n{}", out);
        // i in 0,1,3,4 -> 0 + 10 + 30 + 40 = 80
        assert!(out.contains("77 66 3 80 42 -999"), "Unexpected output:\n{}", out);
    }

    #[test]
    fn test_cooperative_multi_fiber_utf8_codepoints_and_typed_map_keys() {
        let src = r#"
def producer(ch_out):
    for i in 1..13:
        ch_out <- i
    return 0

def relay(ch_in, ch_out):
    for k in 12:
        v := <-ch_in
        ch_out <- (v * 2)
    return 0

def main():
    c1 := make(chan int, 1)
    c2 := make(chan int, 1)
    go producer(c1)
    go relay(c1, c2)

    fib_sum = 0
    for j in 12:
        got := <-c2
        fib_sum = fib_sum + got

    u = "héllo🚀"
    u_len = len(u)
    ch1 = u[1]
    ch5 = u[5]
    rev = "héllo"[::-1]

    m = {"42": 111}
    m[42] = 999
    str_val = m["42"]
    int_val = m[42]

    print(fib_sum, u_len, str_val, int_val)
    print(ch1 + ":" + ch5 + ":" + rev)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("fiber_utf8_map_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        let verifier = LlvmVerifier::new().expect("Verifier init failed");
        verifier.verify_ir(&ir).expect("IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT failed");
        assert!(status.success(), "JIT stderr: {}", stderr);
        // sum(1..=12)*2 = 78*2 = 156
        assert!(out.contains("156 6 111 999"), "Fiber/UTF-8/Map output mismatch:\n{}", out);
        assert!(out.contains("é:🚀:olléh"), "UTF-8 char_at/reverse mismatch:\n{}", out);
    }

    #[test]
    fn test_closures_lambdas_for_else_multiline_and_polymorphic_vdispatch() {
        let src = r#"
class Circle:
    def __init__(self, r):
        self.r = r

    def area(self):
        return self.r * self.r * 3

class Rect:
    def __init__(self, w, h):
        self.w = w
        self.h = h

    def area(self):
        return self.w * self.h

def make_adder(k):
    def add(x, bonus=1):
        return x + k + bonus
    return add

def apply_twice(fn_val, seed):
    first = fn_val(seed)
    return fn_val(first)

def search_else_probe(limit_hit):
    tag = 0
    for i in 5:
        if i == limit_hit:
            tag = 99
            break
    else:
        tag = 777
    return tag

def main():
    add10 = make_adder(10)
    c_a = add10(5)
    c_b = add10(5, 0)
    c_curried = make_adder(20)(7)

    factor = 4
    py_lam = lambda x: x * factor
    js_lam = (x, y) => x + y + factor
    rs_lam = |v| v + 3
    lam_res = py_lam(6) + js_lam(2, 3) + apply_twice(rs_lam, 10)

    no_brk = search_else_probe(99)
    with_brk = search_else_probe(3)

    chained = "  omni  "
        .strip()
        .upper()

    shapes = [Circle(3), Rect(4, 5), Circle(2)]
    total_area = 0
    for s in shapes:
        total_area = total_area + s.area()

    print(c_a, c_b, c_curried, lam_res)
    print(no_brk, with_brk, total_area, chained)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("closures_poly_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("@__omni_closure_create"));
        assert!(ir.contains("@__omni_vdispatch_"));

        let verifier = LlvmVerifier::new().expect("Verifier init failed");
        verifier.verify_ir(&ir).expect("IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT failed");
        assert!(status.success(), "JIT stderr: {}", stderr);
        // c_a=16, c_b=15, c_curried=28, lam_res = 24 + 9 + 16 = 49
        assert!(out.contains("16 15 28 49"), "Closure/lambda output mismatch:\n{}", out);
        // shapes: Circle(3)->27 + Rect(4,5)->20 + Circle(2)->12 = 59
        assert!(out.contains("777 99 59 OMNI"), "for-else/vdispatch/multiline mismatch:\n{}", out);
    }

    #[test]
    fn test_polyglot_audit_remediations_e2e_jit() {
        let src = r#"
class FCircle:
    def __init__(self, r):
        self.r = r

    def area(self):
        return self.r * self.r * 3.14

class IRect:
    def __init__(self, w, h):
        self.w = w
        self.h = h

    def area(self):
        return self.w * self.h

struct Vec2 {
    pub x: i64,
    pub y: i64,
}

def make_greeter(prefix="Hi"):
    def greet(name="World"):
        return prefix + " " + name
    return greet

def scale_val(x, factor=2.5):
    return x * factor

def make_counter(start):
    c = start
    def step():
        c = c + 1
        return c
    return step

def capture_local_slice():
    xs = [100, 200, 300]
    return lambda idx=1: xs[idx]

def alias_escape_vec():
    p1 = Vec2 { x: 12, y: 30 }
    p2 = p1
    p1 = Vec2 { x: 1, y: 2 }
    return p2

def main():
    g1 = make_greeter()()
    g2 = make_greeter("Yo")("Omni")

    sf1 = scale_val(4)
    sf2 = (scale_val)(6)

    cnt = make_counter(10)
    c1 = cnt()
    c2 = cnt()

    cap_fn = capture_local_slice()
    cap_v = cap_fn() + cap_fn(2)

    ep = alias_escape_vec()
    ep_sum = ep.x + ep.y

    b0 = len(bytes())
    b2 = bytes([65, 66])
    malf = "\xE0AB"[::2]

    mp = {42: 999, "42": 111}
    del mp[42]
    mp_str = mp["42"]
    mp_has_int = 1 if 42 in mp else 0

    rs_clos = move |a, b| a * b
    kt_clos = { n -> n + 7 }
    cpp_clos = [=](z) { return z * 3; }
    js_blk = (v) => {
        let doubled = v * 2;
        return doubled + 5;
    }
    poly_clos_sum = rs_clos(3, 4) + kt_clos(8) + cpp_clos(5) + js_blk(10)

    mixed_shapes = [FCircle(2), new IRect(4, 5)]
    mixed_area = 0.0
    for sh in mixed_shapes:
        mixed_area = mixed_area + sh.area()

    print(g1 + "|" + g2 + "|" + b2 + "|" + malf)
    print(sf1, sf2, c1, c2, cap_v, ep_sum)
    print(b0, mp_str, mp_has_int, poly_clos_sum, mixed_area)
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("polyglot_audit_remediations", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        let verifier = LlvmVerifier::new().expect("Verifier init failed");
        verifier.verify_ir(&ir).expect("IR verification failed");

        let runtime_c = omni_runtime::runtime_c_sources();
        let (status, out, stderr) = verifier
            .run_jit_captured(&ir, 0, &runtime_c, &[])
            .expect("JIT failed");
        assert!(status.success(), "JIT stderr: {}", stderr);
        assert!(out.contains("Hi World|Yo Omni|AB|\u{FFFD}B"), "String/bytes/UTF-8 mismatch:\n{}", out);
        assert!(out.contains("10.0 15.0 11 12 500 42") || out.contains("10 15 11 12 500 42"), "Float/counter/capture/escape mismatch:\n{}", out);
        // rs_clos=12 + kt_clos=15 + cpp_clos=15 + js_blk=25 = 67; mixed_area = 12.56 + 20.0 = 32.56
        assert!(out.contains("0 111 0 67 32.56"), "Map/polyglot-closure/mixed-vdispatch mismatch:\n{}", out);

        // [C1] Verify bogus unimported receiver fails closed even when another ecosystem module is imported
        let bad_extmod_src = r#"
import 'python' math

def main():
    x = bogus_mod.typo_fn(99)
    print(x)
    return 0
"#;
        let bad_mod = parser.parse_module("bad_extmod_test", bad_extmod_src);
        let err = codegen
            .generate_module_ir(&bad_mod, 0.0)
            .expect_err("Expected fail-closed error on unimported module receiver");
        assert!(
            err.to_string().contains("OMNIC SEMANTIC ERROR") && err.to_string().contains("bogus_mod"),
            "Unexpected error message: {}",
            err
        );
    }

    #[test]
    fn test_extern_fn_and_pico_hal_prelude_isolation() {
        let parser = OmniParser::new();
        let engine = MockSynthesisEngine::new();

        // 1) Desktop target WITHOUT `import "pico"` or `extern fn` MUST reject bare-metal `wifi_ip()` / `gpio_put()`
        let desktop_cg = CodeGenerator::new(&engine);
        let unimported_mod = parser.parse_module(
            "desktop_no_pico",
            "fn main() -> int {\n    let s = wifi_ip();\n    print(s);\n    return 0;\n}\n",
        );
        let err = desktop_cg
            .generate_module_ir(&unimported_mod, 0.0)
            .expect_err("Desktop target without `import \"pico\"` must reject `wifi_ip()`");
        assert!(
            err.to_string().contains("OMNIC SEMANTIC ERROR") && err.to_string().contains("wifi_ip"),
            "Unexpected error: {}",
            err
        );

        // 2) Non-wireless `--platform pico2` (`rp2350`) has GPIO/PIO/USB HAL, but rejects `wifi_ip()` unless `import "pico/net"`
        let pico2_cg = CodeGenerator::new(&engine)
            .with_target_triple("thumbv8m.main-none-eabihf")
            .with_platform_slug("rp2350");
        let gpio_only_mod = parser.parse_module(
            "pico2_gpio",
            "fn main() -> int {\n    gpio_init(25);\n    gpio_put(25, 1);\n    return 0;\n}\n",
        );
        let gpio_ir = pico2_cg
            .generate_module_ir(&gpio_only_mod, 0.0)
            .expect("pico2 base HAL should resolve gpio_init/gpio_put");
        assert!(gpio_ir.contains("declare i64 @gpio_put(i64, i64)"));

        let pico2_wifi_err = pico2_cg
            .generate_module_ir(&unimported_mod, 0.0)
            .expect_err("Non-wireless pico2 must reject `wifi_ip()` without `import \"pico/net\"`");
        assert!(pico2_wifi_err.to_string().contains("wifi_ip"));

        // 3) Explicit `import "pico"` (unqualified & aliased) + `--platform pico2w` resolves `wifi_ip() -> i8*` & `gpio_put -> i64`
        let pico_import_src = r#"
import "pico" as pico;

extern fn custom_sensor_read(reg: int) -> int;

fn main() -> int {
    pico.gpio_init(15);
    let ip = pico.wifi_ip();
    let msg = ws_poll();
    let v = custom_sensor_read(42);
    print(ip + msg);
    return v;
}
"#;
        let pico_mod = parser.parse_module("pico_import_test", pico_import_src);
        let pico_ir = desktop_cg
            .generate_module_ir(&pico_mod, 0.0)
            .expect("`import \"pico\"` + `extern fn` should lower cleanly");
        assert!(pico_ir.contains("declare i8* @wifi_ip()"), "IR:\n{}", pico_ir);
        assert!(pico_ir.contains("declare i8* @ws_poll()"), "IR:\n{}", pico_ir);
        assert!(pico_ir.contains("declare i64 @gpio_init(i64)"), "IR:\n{}", pico_ir);
        assert!(pico_ir.contains("declare i64 @custom_sensor_read(i64)"), "IR:\n{}", pico_ir);
    }

    #[test]
    fn test_deterministic_ssa_ir_emission_byte_identical() {
        let parser = OmniParser::new();
        let src = r#"
extern fn ext_alpha(a: int, b: int) -> int;
extern fn ext_beta(msg: str) -> str;

class Accumulator {
    total: int
    scale: int

    fn add(self, x: int) -> int {
        self.total = self.total + x * self.scale
        return self.total
    }
}

fn fib(n: int) -> int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

def main():
    let acc = Accumulator(10, 2)
    let r = acc.add(5)
    let f = fib(6)
    let m = {"alpha": r, "beta": f}
    let arr = [r, f, ext_alpha(1, 2)]
    let closure = |k| k + r
    print(ext_beta("ok"))
    return closure(arr[0]) + m["beta"]
"#;
        let engine = MockSynthesisEngine::new();
        let baseline_mod = parser.parse_module("det_test", src);
        let baseline_ir = CodeGenerator::new(&engine)
            .generate_module_ir(&baseline_mod, 0.0)
            .expect("baseline IR generation");

        for run in 0..8 {
            let m = parser.parse_module("det_test", src);
            let ir = CodeGenerator::new(&engine)
                .generate_module_ir(&m, 0.0)
                .expect("repeated IR generation");
            assert_eq!(
                baseline_ir, ir,
                "SSA IR drifted on run {} — nondeterministic emission detected!",
                run
            );
        }
    }
}
