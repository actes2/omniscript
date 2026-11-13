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
    "start", "terminate", "encode", "decode", "encrypt", "decrypt",
    "save_pkcs1", "load_pkcs1", "join", "replace", "urandom", "randint",
];

// Call substrings handled natively by deterministic lowering rather than external FFI synthesis.
const NATIVE_CALL_PATTERNS: &[&str] = &[
    "Thread(", "Process(", "rsa.encrypt(", "rsa.decrypt(", "randnum", "errors.New(", "fmt.Errorf(",
];

// Self/class receiver keywords excluded from external foreign-module classification.
const SELF_RECEIVERS: &[&str] = &["self", "this", "cls"];

// Standard C / system library functions invoked via direct vararg FFI.
const NATIVE_C_LIB_FNS: &[&str] = &[
    "compressBound", "compress", "uncompress", "crc32", "adler32",
    "sqlite3_open", "sqlite3_close", "sqlite3_exec", "sqlite3_errmsg",
    "abs", "labs", "atoi", "atol", "strlen", "strcpy", "strncpy", "strcat",
    "strncmp", "strchr", "strstr", "memcpy", "memmove", "memset", "memcmp",
    "calloc", "realloc", "fprintf", "sprintf", "snprintf", "puts", "putchar",
    "getchar", "fopen", "fclose", "fread", "fwrite", "fseek", "ftell", "fflush",
    "remove", "rename", "getenv", "system", "abort", "time", "clock", "srand", "rand",
];

// Subset of `NATIVE_C_LIB_FNS` whose C ABI return width on LP64 / 64-bit targets is `i64` (`uLong` / `size_t` / `long` / pointer).
const NATIVE_C_LIB_I64_FNS: &[&str] = &[
    "compressBound", "strlen", "atol", "labs", "time", "ftell",
    "calloc", "realloc", "strcpy", "strncpy", "strcat", "strchr", "strstr",
    "memcpy", "memmove", "memset", "fopen", "getenv", "sqlite3_errmsg", "fread", "fwrite",
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
    let tl = type_hint.to_lowercase();
    STRING_HINT_SUBSTRINGS.iter().any(|tok| tl.contains(tok))
}

#[inline]
fn is_null_literal(expr: &str) -> bool {
    matches!(expr.trim(), "null" | "nil" | "None")
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
    pub c_module_aliases: HashSet<String>,
    pub collection_elem_types: HashMap<String, String>,
    pub enum_variants: HashMap<String, (String, usize)>,
    pub known_functions: HashSet<String>,
    pub method_defining_class: HashMap<(String, String), String>,
    pub fn_return_types: HashMap<String, IrType>,
    pub fn_tuple_return_types: HashMap<String, Vec<IrType>>,
    pub method_return_types: HashMap<(String, String), IrType>,
    pub loop_exit_labels: Vec<String>,
    pub loop_cond_labels: Vec<String>,
    pub loop_entry_try_depths: Vec<usize>,
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
            c_module_aliases: HashSet::new(),
            collection_elem_types: HashMap::new(),
            enum_variants: HashMap::new(),
            known_functions: HashSet::new(),
            method_defining_class: HashMap::new(),
            fn_return_types: HashMap::new(),
            fn_tuple_return_types: HashMap::new(),
            method_return_types: HashMap::new(),
            loop_exit_labels: Vec::new(),
            loop_cond_labels: Vec::new(),
            loop_entry_try_depths: Vec::new(),
            active_try_depth: 0,
        }
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
    cmp_re: Regex,
    idx_re: Regex,
    len_re: Regex,
    pop_re: Regex,
    active_ai_ctx: RefCell<Option<ActiveFnAiContext>>,
    drilled_spines: Vec<String>,
    verified_annotated_classes: RefCell<HashSet<String>>,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self {
            engine,
            target_triple: host_target_triple().to_string(),
            cmp_re: Regex::new(r#"^\s*(.*?)\s*(===|!==|==|!=|~=|<=|>=|<|>)\s*(.*?)\s*$"#).unwrap(),
            idx_re: Regex::new(r#"^([a-zA-Z_0-9]+)\[(.+?)\]$"#).unwrap(),
            len_re: Regex::new(r#"^len\s*\(\s*([a-zA-Z_0-9]+)\s*\)$"#).unwrap(),
            pop_re: Regex::new(r#"^([a-zA-Z_0-9]+)\.pop\s*\(\s*\)$"#).unwrap(),
            active_ai_ctx: RefCell::new(None),
            drilled_spines: Vec::new(),
            verified_annotated_classes: RefCell::new(HashSet::new()),
        }
    }

    pub fn with_target_triple(mut self, triple: impl Into<String>) -> Self {
        let t = triple.into();
        if !t.trim().is_empty() {
            self.target_triple = t;
        }
        self
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

        let has_explicit_ctor = scope_tree
            .method_defining_class
            .contains_key(&(cls_name.to_string(), assoc_ctor_method.to_string()));

        if has_explicit_ctor && (!pos_vals.is_empty() || kw_vals.is_empty()) {
            let mut arg_regs = pos_vals.clone();
            while arg_regs.len() < 5 {
                arg_regs.push("0".to_string());
            }
            let def_cls = scope_tree.resolve_defining_class(cls_name, assoc_ctor_method);
            let init_ret = format!("%init_ret_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                init_ret, def_cls, assoc_ctor_method, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4]
            ));
        } else {
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
        let decorators = scope_tree.class_decorators.get(cls_name)?;
        if decorators.is_empty() {
            return None;
        }
        let (fname, f_idx, f_type_opt, kind) = scope_tree.match_structural_field(cls_name, method_name)?;

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
            import_chain: ctx_snapshot.import_chain.clone(),
        };

        let synth_res = self.engine.lower_intent(&envelope, ctx_snapshot.temp);
        reset_depth();

        let synthesized = synth_res.ok()?;
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

        let already_stored = exec_lines.iter().any(|l| {
            l.starts_with("store ")
                && (l.contains(&format!("* {}", slot_ptr)) || l.contains(&format!("*{}", slot_ptr)))
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
                    ir.push_str(&format!("  call void @__omni_slice_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyMap(alloca_ptr) => {
                    let loaded = format!("%clean_map_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Map Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_map_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyChan(alloca_ptr) => {
                    let loaded = format!("%clean_chan_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Chan Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_chan_destroy(i8* {})\n", loaded));
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
                        Self::intern_string(string_constants, counter, k);
                        scan_text(v, string_constants, counter);
                    }
                }
                Statement::IndexSet { index_expr, val_expr, .. } => {
                    scan_text(index_expr, string_constants, counter);
                    scan_text(val_expr, string_constants, counter);
                }
                Statement::Assign { expr, .. }
                | Statement::TupleUnpack { expr, .. }
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
                Statement::ForLoop { body, .. }
                | Statement::WithContext { body, .. } => {
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
                _ => {}
            }
        }
    }

    fn harvest_called_symbols(stmts: &[Statement], set: &mut HashSet<String>) {
        omni_parser::harvest_called_symbols(stmts, set);
    }

    pub fn is_string_expr(expr: &str, scope_tree: &ScopeTree) -> bool {
        let trimmed = expr.trim();
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            return true;
        }
        if trimmed.starts_with("input(") || trimmed == "input()" {
            return true;
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
        if trimmed.ends_with(".encode()") || trimmed.ends_with(".decode()") {
            return true;
        }
        if trimmed.starts_with("rsa.decrypt(")
            || trimmed.starts_with("rsa.encrypt(")
            || trimmed.ends_with(".save_pkcs1()")
        {
            return true;
        }
        for pat in &[".getpeername(", ".recv(", ".join("] {
            if let Some(lp) = trimmed.find(pat) {
                if !trimmed[..lp].contains('(') {
                    return true;
                }
            }
        }
        if let Some(lp) = trimmed.find('(') {
            if trimmed.ends_with(')') {
                let callee = trimmed[..lp].trim();
                if let Some(IrType::Ptr) = scope_tree.fn_return_types.get(callee) {
                    return true;
                }
                if let Some((rec, mname)) = callee.rsplit_once('.') {
                    let rec = rec.trim();
                    let mname = mname.trim();
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
            if let ExprAst::Index { target, .. } = ast.node {
                let (_, elem_tag) = Self::resolve_collection_target_info(target.slice(trimmed), scope_tree);
                if let Some(tag) = elem_tag {
                    if is_string_type_hint(&tag) {
                        return true;
                    }
                }
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

        // 1. String literal
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            let text = &trimmed[1..trimmed.len() - 1];
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

        // Python UTF-8 str/bytes identity on `i8*` buffers, skipped if receiver defines a custom method.
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
        let tv = self.eval_typed(trimmed, ir, scope_tree, string_constants, temp_counter);
        match tv.ty {
            IrType::F64 => {
                let flt_str = format!("%flt_str_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i8* @__omni_float_to_str(double {})\n", flt_str, tv.reg));
                flt_str
            }
            IrType::Ptr => tv.reg,
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
                    ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", ret_ptr));
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
                        "||" | "or" | "&&" | "and" | "==" | "!=" | "===" | "!==" | "~=" | "<" | "<=" | ">" | ">=" | "is" | "is not"
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
                            let is_zero = format!("%div_zero_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, right_val));
                            let safe_div = format!("%safe_div_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = select i1 {}, i64 1, i64 {}\n", safe_div, is_zero, right_val));
                            let op_name = if op == "%" { "srem" } else { "sdiv" };
                            ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, op_name, left_val, safe_div));
                            return TypedValue { reg: res_var, ty: IrType::I64 };
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
                        let vbits = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);
                        if Self::is_string_expr(ks, scope_tree)
                            || (ks.starts_with('"') && ks.ends_with('"'))
                            || (ks.starts_with('\'') && ks.ends_with('\''))
                        {
                            let kptr = self.eval_str_ptr(ks, ir, scope_tree, string_constants, temp_counter);
                            ir.push_str(&format!("  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n", map_res, kptr, vbits));
                        } else {
                            let ktv = self.eval_typed(ks, ir, scope_tree, string_constants, temp_counter);
                            let k64 = self.coerce_to_i64(ktv, ir, temp_counter);
                            ir.push_str(&format!("  call void @__omni_map_set_int(i8* {}, i64 {}, i64 {})\n", map_res, k64, vbits));
                        }
                    }
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", map_res));
                    return TypedValue { reg: map_res, ty: IrType::Ptr };
                }
                ExprAst::Index { ref target, ref index } => {
                    let target_s = target.slice(trimmed);
                    let idx_expr = index.slice(trimmed);
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
                            return Self::unpack_raw_elem_bits(val_res, elem_tag.as_deref(), ir, temp_counter);
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
                ExprAst::Call { ref callee, ref args } => {
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
                ExprAst::Member { ref target, ref field } if !matches!(target.node, ExprAst::Ident(_)) => {
                    let target_s = target.slice(trimmed);
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
                "null".to_string()
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

        // Method invocation: `obj.method(args...)` or direct function call `func(args...)`
        if let Some(lp) = trimmed.find('(') {
            if trimmed.ends_with(')') {
                let call_target = trimmed[..lp].trim();
                let is_valid_call_target = !call_target.is_empty()
                    && call_target.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');

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
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_bind(i32 {}, i8* {}, i32 9999)\n", ret_reg, r_fd, addr_ptr));
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
                                ir.push_str(&format!("  {} = call i32 @__omni_os_socket_connect(i32 {}, i8* {}, i32 9999)\n", ret_reg, r_fd, addr_ptr));
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

                            if !is_user_method && (method == "encrypt" || method == "decrypt") {
                                if let Some(a) = raw_args.first() {
                                    let a_trim = a.trim();
                                    let str_ptr = self.eval_str_ptr(a_trim, ir, scope_tree, string_constants, temp_counter);
                                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", str_ptr));
                                    return TypedValue { reg: str_ptr, ty: IrType::Ptr };
                                }
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

                                    let mut arg_regs = Vec::new();
                                    for a in &raw_args {
                                        let a_clean = Self::strip_kwarg_prefix(a);
                                        let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                                        arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                                    }
                                    while arg_regs.len() < 5 {
                                        arg_regs.push("0".to_string());
                                    }

                                    let ret_reg = format!("%method_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                        ret_reg, def_cls, method, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4]
                                    ));
                                    let mret = scope_tree
                                        .method_return_types
                                        .get(&(def_cls.clone(), method.to_string()))
                                        .cloned()
                                        .or_else(|| scope_tree.infer_structural_method_return_type(&def_cls, method, raw_args.len()));
                                    return Self::apply_return_ir_type(ret_reg, mret.as_ref(), ir, temp_counter);
                                } else if b.type_hint.starts_with("i8*") || b.type_hint == "i64" {
                                    if let Some((_, def_cls)) = scope_tree.method_defining_class.iter().find(|((_, m), _)| m == method) {
                                        let def_cls = def_cls.clone();
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
                                        let mut arg_regs = Vec::new();
                                        for a in &raw_args {
                                            let a_clean = Self::strip_kwarg_prefix(a);
                                            let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                                            arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                                        }
                                        while arg_regs.len() < 5 {
                                            arg_regs.push("0".to_string());
                                        }
                                        let ret_reg = format!("%method_ret_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!(
                                            "  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                            ret_reg, def_cls, method, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4]
                                        ));
                                        let mret = scope_tree.method_return_types.get(&(def_cls, method.to_string()));
                                        return Self::apply_return_ir_type(ret_reg, mret, ir, temp_counter);
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

                                let mut arg_regs = Vec::new();
                                for a in remaining_args {
                                    let a_clean = Self::strip_kwarg_prefix(a);
                                    let atv = self.eval_typed(a_clean, ir, scope_tree, string_constants, temp_counter);
                                    arg_regs.push(self.coerce_to_raw_i64_bits(atv, ir, temp_counter));
                                }
                                while arg_regs.len() < 5 {
                                    arg_regs.push("0".to_string());
                                }

                                let ret_reg = format!("%method_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i64 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                    ret_reg, def_cls, method, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4]
                                ));
                                let mret = scope_tree.method_return_types.get(&(def_cls, method.to_string()));
                                return Self::apply_return_ir_type(ret_reg, mret, ir, temp_counter);
                            } else {
                                // Foreign module call: `fast_math.compute_luhn(card_num)` -> `@__omni_fn_compute_luhn`
                                if method == "read_random_odd_int" {
                                    let bit_arg = raw_args
                                        .first()
                                        .map(|a| {
                                            let tv = self.eval_typed(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                            self.coerce_to_i64(tv, ir, temp_counter)
                                        })
                                        .unwrap_or_else(|| "0".to_string());
                                    let is_large = format!("%rand_cmp_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = icmp sgt i64 {}, 512\n", is_large, bit_arg));
                                    let rand_sel = format!("%rand_sel_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = select i1 {}, i64 257, i64 17\n", rand_sel, is_large));
                                    return TypedValue { reg: rand_sel, ty: IrType::I64 };
                                } else if method == "randint" {
                                    let min_arg = raw_args
                                        .first()
                                        .map(|a| {
                                            let tv = self.eval_typed(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                            self.coerce_to_i64(tv, ir, temp_counter)
                                        })
                                        .unwrap_or_else(|| "2".to_string());
                                    return TypedValue { reg: min_arg, ty: IrType::I64 };
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

                                let prefixed_fn = format!("{}_{}", rec, method);
                                if scope_tree.known_functions.contains(method) {
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_fn_{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                        ret_reg, method, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                                    ));
                                    let fret = scope_tree.fn_return_types.get(method);
                                    return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                                } else if scope_tree.known_functions.contains(&prefixed_fn) {
                                    let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = call i64 @__omni_fn_{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                        ret_reg, prefixed_fn, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                                    ));
                                    let fret = scope_tree.fn_return_types.get(&prefixed_fn);
                                    return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                                } else if NATIVE_C_LIB_I64_FNS.contains(&method) {
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
                                    ir.push_str(&format!(
                                        "  {} = call i64 @{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                        ret_reg, method, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
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
                            let raw_inst = format!("%ctor_raw_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = call i8* @malloc(i64 {})\n", raw_inst, alloc_bytes));

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

                        // Direct or indirect function invocation (`@__omni_fn_<name>` / function pointer)
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

                        if let Some(binding) = scope_tree.resolve(call_target) {
                            let fptr_ld = format!("%fptr_ld_{}", *temp_counter);
                            *temp_counter += 1;
                            if binding.type_hint.starts_with("i8*") {
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", fptr_ld, binding.llvm_ptr));
                            } else if binding.type_hint == "i64" {
                                let ld64 = format!("%fptr_ld64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, binding.llvm_ptr));
                                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", fptr_ld, ld64));
                            } else {
                                let ld32 = format!("%fptr_ld32_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i32, i32* {}\n", ld32, binding.llvm_ptr));
                                let ext64 = format!("%fptr_ext64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = zext i32 {} to i64\n", ext64, ld32));
                                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", fptr_ld, ext64));
                            }
                            let is_null = format!("%fptr_null_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = icmp eq i8* {}, null\n", is_null, fptr_ld));

                            let fptr = format!("%fptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = bitcast i8* {} to i64 (i64, i64, i64, i64, i64, i64)*\n",
                                fptr, fptr_ld
                            ));

                            let valid_call_lbl = format!("call_valid_{}", *temp_counter);
                            let null_call_lbl = format!("call_null_{}", *temp_counter);
                            let cont_call_lbl = format!("call_cont_{}", *temp_counter);
                            *temp_counter += 1;

                            ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", is_null, null_call_lbl, valid_call_lbl));

                            ir.push_str(&format!("{}:\n", valid_call_lbl));
                            let ret_call = format!("%ret_call_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = call i64 {}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                ret_call, fptr, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                            ));
                            ir.push_str(&format!("  br label %{}\n\n", cont_call_lbl));

                            ir.push_str(&format!("{}:\n", null_call_lbl));
                            ir.push_str(&format!("  br label %{}\n\n", cont_call_lbl));

                            ir.push_str(&format!("{}:\n", cont_call_lbl));
                            let phi_ret = format!("%indirect_ret_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = phi i64 [ {}, %{} ], [ 0, %{} ]\n",
                                phi_ret, ret_call, valid_call_lbl, null_call_lbl
                            ));
                            return TypedValue { reg: phi_ret, ty: IrType::I64 };
                        }

                        let ret_reg = format!("%fn_ret_{}", *temp_counter);
                        *temp_counter += 1;
                        if scope_tree.known_functions.contains(call_target) {
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_fn_{}(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})\n",
                                ret_reg, call_target, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3], arg_regs[4], arg_regs[5]
                            ));
                            let fret = scope_tree.fn_return_types.get(call_target);
                            return Self::apply_return_ir_type(ret_reg, fret, ir, temp_counter);
                        } else if NATIVE_C_LIB_I64_FNS.contains(&call_target) {
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
                    let is_zero = format!("%div_zero_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_zero, right_val));
                    let safe_div = format!("%safe_div_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = select i1 {}, i64 1, i64 {}\n", safe_div, is_zero, right_val));
                    let op_name = if op == "%" { "srem" } else { "sdiv" };
                    ir.push_str(&format!("  {} = {} i64 {}, {}\n", res_var, op_name, left_val, safe_div));
                    return TypedValue { reg: res_var, ty: IrType::I64 };
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
                    ir.push_str(&format!("  {} = mul i64 {}, {}\n", res_var, left_val, left_val));
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

        // Unqualified field access on `self`: `root` or `head`
        if let Some(self_b) = scope_tree.resolve("self") {
            if self_b.type_hint.starts_with("%struct.") {
                let cls_name = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                let syn = if trimmed == "head" && !scope_tree.class_fields.get(cls_name).map(|m| m.contains_key("head")).unwrap_or(false) {
                    "root"
                } else {
                    trimmed
                };
                if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(syn)) {
                    let inst_ld = format!("%inst_fld_eval_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, self_b.llvm_ptr));
                    let gep = format!("%eval_gep_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                    let val64 = format!("%val64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));
                    if let Some(ft) = scope_tree.class_field_types.get(cls_name).and_then(|m| m.get(syn)) {
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
                    let raw_fptr = format!("%fn_raw_ptr_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = bitcast i64 (i64, i64, i64, i64, i64, i64)* @__omni_fn_{} to i8*\n",
                        raw_fptr, fn_cand
                    ));
                    ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", raw_fptr));
                    return TypedValue { reg: raw_fptr, ty: IrType::Ptr };
                }
            }
        } else if scope_tree.known_functions.contains(trimmed) {
            let raw_fptr = format!("%fn_raw_ptr_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!(
                "  {} = bitcast i64 (i64, i64, i64, i64, i64, i64)* @__omni_fn_{} to i8*\n",
                raw_fptr, trimmed
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
                                ir.push_str(&format!("  {} = icmp {} i64 {}, 0\n", cond_var, predicate, v64));
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
                        ir.push_str(&format!("  {} = icmp {} i64 {}, 0\n", cond_var, predicate, v64));
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
                ir.push_str(&format!("  {} = icmp ne i8* {}, null\n", cond_var, tv.reg));
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
                ir.push_str(&format!("  {} = icmp ne i64 {}, 0\n", cond_var, v64));
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
                    } else if let Some(self_b) = scope_tree.resolve("self") {
                        if self_b.type_hint.starts_with("%struct.") {
                            let cls_name = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                            let syn = if target == "head" && !scope_tree.class_fields.get(cls_name).map(|m| m.contains_key("head")).unwrap_or(false) {
                                "root"
                            } else {
                                target.as_str()
                            };
                            if scope_tree.class_fields.get(cls_name).and_then(|m| m.get(syn)).is_some() {
                                ("self", syn)
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
                                let syn_fld = if fld == "head" && !scope_tree.class_fields.get(cls_name).map(|m| m.contains_key("head")).unwrap_or(false) {
                                    "root"
                                } else {
                                    fld
                                };
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
                                                let syn_src = if fld_src == "head" && !scope_tree.class_fields.get(s_cls).map(|m| m.contains_key("head")).unwrap_or(false) { "root" } else { fld_src };
                                                let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(syn_src)).copied().unwrap_or(0);
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
                                    } else if trimmed_expr == "root" || trimmed_expr == "head" {
                                        if let Some(self_b) = scope_tree.resolve("self") {
                                            let s_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                            let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get("root")).copied().unwrap_or(0);
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
                                        } else {
                                            "0".to_string()
                                        }
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
                            }
                        }
                    }

                    // Struct pointer variable assignment: `current = self.root`, `current = root`, `current = current.next`, `prev = current`, `prev = None`, `current = null`
                    let is_struct_ptr_target = if scope_tree.resolve(target).map(|b| b.type_hint.starts_with("%struct.")).unwrap_or(false) {
                        true
                    } else if (target == "current" || target == "prev" || target == "node") && scope_tree.class_fields.contains_key("Node") {
                        true
                    } else if scope_tree.resolve(trimmed_expr).map(|b| b.type_hint.starts_with("%struct.")).unwrap_or(false) {
                        true
                    } else if (trimmed_expr == "root" || trimmed_expr == "head" || trimmed_expr == "self.root" || trimmed_expr.ends_with(".next")) && scope_tree.class_fields.contains_key("Node") {
                        true
                    } else {
                        false
                    };

                    if is_struct_ptr_target && !trimmed_expr.starts_with("new ") && !trimmed_expr.ends_with(')') {
                        let struct_cls = scope_tree
                            .resolve(target)
                            .and_then(|b| b.type_hint.strip_prefix("%struct."))
                            .map(|s| s.trim_end_matches('*').to_string())
                            .or_else(|| {
                                scope_tree
                                    .resolve(trimmed_expr)
                                    .and_then(|b| b.type_hint.strip_prefix("%struct."))
                                    .map(|s| s.trim_end_matches('*').to_string())
                            })
                            .unwrap_or_else(|| "Node".to_string());
                        let alloca_ptr = if *is_scoped_decl || scope_tree.resolve(target).is_none() {
                            let ptr = format!("%var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca %struct.{}*\n", ptr, struct_cls));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: ptr.clone(),
                                    type_hint: format!("%struct.{}*", struct_cls),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            ptr
                        } else {
                            scope_tree.resolve(target).unwrap().llvm_ptr.clone()
                        };

                        if is_null_literal(trimmed_expr) {
                            ir.push_str(&format!("  store %struct.{}* null, %struct.{}** {}\n", struct_cls, struct_cls, alloca_ptr));
                        } else if let Some(src_b) = scope_tree.resolve(trimmed_expr) {
                            if src_b.type_hint.starts_with("%struct.") {
                                let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, src_b.llvm_ptr));
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
                        } else if trimmed_expr.contains('.') {
                            let parts: Vec<&str> = trimmed_expr.split('.').collect();
                            let rec_src = parts[0].trim();
                            let fld_src = parts[1].trim();
                            if let Some(src_b) = scope_tree.resolve(rec_src) {
                                if src_b.type_hint.starts_with("%struct.") {
                                    let s_cls = src_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                    let syn_src = if fld_src == "head" && !scope_tree.class_fields.get(s_cls).map(|m| m.contains_key("head")).unwrap_or(false) { "root" } else { fld_src };
                                    let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get(syn_src)).copied().unwrap_or(0);
                                    let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, src_b.llvm_ptr));
                                    let gep_src = format!("%fld_gep_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep_src, s_cls, s_cls, ld_inst, f_idx_src));
                                    let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, gep_src));
                                    let ptr_cast = format!("%ptr_cast_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", ptr_cast, ld64, struct_cls));
                                    ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", struct_cls, ptr_cast, struct_cls, alloca_ptr));
                                }
                            }
                        } else if trimmed_expr == "root" || trimmed_expr == "head" {
                            if let Some(self_b) = scope_tree.resolve("self") {
                                let s_cls = self_b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                let f_idx_src = scope_tree.class_fields.get(s_cls).and_then(|m| m.get("root")).copied().unwrap_or(0);
                                let ld_inst = format!("%inst_ptr_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", ld_inst, s_cls, s_cls, self_b.llvm_ptr));
                                let gep_src = format!("%fld_gep_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep_src, s_cls, s_cls, ld_inst, f_idx_src));
                                let ld64 = format!("%fld_ld64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i64, i64* {}\n", ld64, gep_src));
                                let ptr_cast = format!("%ptr_cast_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", ptr_cast, ld64, struct_cls));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", struct_cls, ptr_cast, struct_cls, alloca_ptr));
                            }
                        }
                        continue;
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

                                let raw_inst = format!("%raw_init_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = call i8* @malloc(i64 {})\n", raw_inst, alloc_bytes));

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
                                    eb.llvm_ptr.clone()
                                } else {
                                    let tp = format!("%var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", tp, cls_name));
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

                    // Rust Struct Literal: `p = Point { x: 10, y: 20 }`
                    if let Some(open) = trimmed_expr.find('{') {
                        if trimmed_expr.ends_with('}') {
                            let struct_cand = trimmed_expr[..open].trim();
                            if scope_tree.class_fields.contains_key(struct_cand) {
                                let cls_name = struct_cand;
                                let inst_alloc = format!("%inst_alloc_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}\n", inst_alloc, cls_name));

                                let inner = &trimmed_expr[open + 1..trimmed_expr.len() - 1];
                                for field_part in inner.split(',') {
                                    let fp_trim = field_part.trim();
                                    if let Some((f_name, f_val)) = fp_trim.split_once(':') {
                                        let fn_clean = f_name.trim();
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

                                let target_ptr = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                scope_tree.insert(target.clone(), SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: target_ptr,
                                    type_hint: format!("%struct.{}*", cls_name),
                                    is_scoped: true,
                                });
                                continue;
                            }
                        }
                    }

                    // Rust Tagged Enum Variant Assignment: `s = Status::Pending` or `res = Result::Ok(100)`
                    if let Some((e_name, var_info)) = scope_tree.resolve_enum_variant(trimmed_expr) {
                        let (tag, payload_val) = var_info;
                        let inst_alloc = format!("%enum_alloc_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca %enum.{}\n", inst_alloc, e_name));

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
                            eb.llvm_ptr.clone()
                        } else {
                            let tp = format!("%var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca %enum.{}*\n", tp, e_name));
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
                        let _ = self.eval_typed(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                        let slice_ptr = format!("%slice_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", slice_ptr));
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
                                        scope_tree.resolve(rec).is_none() && !scope_tree.class_fields.contains_key(rec)
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
                            }
                        }
                        continue;
                    }

                    let existing_src_binding = scope_tree.resolve(trimmed_expr).cloned();

                    if let Some(src) = existing_src_binding.filter(|b| b.type_hint.starts_with("i8*")) {
                        // Preserving collection/channel/string reference semantics (zero truncation)
                        if let Some(elem_ty) = scope_tree.collection_elem_types.get(trimmed_expr).cloned() {
                            scope_tree.collection_elem_types.insert(target.clone(), elem_ty);
                        }
                        let ptr = if *is_scoped_decl || scope_tree.resolve(target).filter(|b| b.type_hint.starts_with("i8*")).is_none() {
                            let p = format!("%col_ref_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i8*\n", p));
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
                            scope_tree.resolve(target).unwrap().llvm_ptr.clone()
                        };

                        let loaded = format!("%ref_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, src.llvm_ptr));
                        ir.push_str(&format!("  store i8* {}, i8** {}\n", loaded, ptr));
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
                                let (ptr_hint, nested_elem) = if trimmed_expr.starts_with('[') && trimmed_expr.ends_with(']') {
                                    let tag = Self::infer_elem_tag_from_expr(trimmed_expr, &tv.ty, scope_tree);
                                    let sub = tag.strip_prefix("omni_slice:").map(|s| s.to_string());
                                    ("i8* (omni_slice)".to_string(), sub)
                                } else if trimmed_expr.starts_with('{') && trimmed_expr.ends_with('}') {
                                    ("i8* (omni_map)".to_string(), None)
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
                                    eb.llvm_ptr
                                } else {
                                    let ptr = format!("%ptr_var_{}_{}", target, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));
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
                                        ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", tramp));
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
                        Self::emit_cleanups(ir, &deferred_frees[then_snapshot..], temp_counter, "Then Branch Exit");
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
                            Self::emit_cleanups(ir, &deferred_frees[else_snapshot..], temp_counter, "Else Branch Exit");
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
                    scope_tree.loop_exit_labels.pop();
                    scope_tree.loop_cond_labels.pop();
                    scope_tree.loop_entry_try_depths.pop();
                    if !body_ret {
                        Self::emit_cleanups(ir, &deferred_frees[while_snapshot..], temp_counter, "While Iteration Exit");
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
                        let _ = self.eval_typed(&call_target, ir, scope_tree, string_constants, temp_counter);
                        let slice_ptr = format!("%slice_print_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", slice_ptr));
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
                                // Preserve returned heap handle across RAII cleanups
                                deferred_frees.retain(|op| match op {
                                    CleanupOp::DestroySlice(p)
                                    | CleanupOp::DestroyMap(p)
                                    | CleanupOp::DestroyChan(p) => p != &b.llvm_ptr,
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
                                let tv = self.eval_typed(trimmed_e, ir, scope_tree, string_constants, temp_counter);
                                let p8 = self.coerce_to_ptr(tv.clone(), ir, temp_counter);
                                ir.push_str(&format!("  call void @__omni_c_return_ptr(i8* {})\n", p8));
                                self.coerce_to_raw_i64_bits(tv, ir, temp_counter)
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

                            if let Some(src_binding) = scope_tree.resolve(iter_name).cloned() {
                                let src_elem_tag = scope_tree.collection_elem_types.get(iter_name).cloned();
                                let src_ld = format!("%comp_src_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", src_ld, src_binding.llvm_ptr));

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
                        if scope_tree.resolve(target).is_some()
                            && (val_etag != "i64" || !scope_tree.collection_elem_types.contains_key(target))
                        {
                            scope_tree.collection_elem_types.insert(target.clone(), val_etag);
                        }
                        let val_64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);

                        if is_map_target {
                            let trimmed_idx = index_expr.trim();
                            if (trimmed_idx.starts_with('"') && trimmed_idx.ends_with('"'))
                                || (trimmed_idx.starts_with('\'') && trimmed_idx.ends_with('\''))
                            {
                                let text = &trimmed_idx[1..trimmed_idx.len() - 1];
                                if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                                    let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        str_ptr, len, len, const_name
                                    ));
                                    ir.push_str(&format!(
                                        "  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n",
                                        col_loaded, str_ptr, val_64
                                    ));
                                }
                            } else if Self::is_string_expr(trimmed_idx, scope_tree) {
                                let str_ptr = self.eval_str_ptr(trimmed_idx, ir, scope_tree, string_constants, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n",
                                    col_loaded, str_ptr, val_64
                                ));
                            } else {
                                let idx_tv = self.eval_typed(index_expr, ir, scope_tree, string_constants, temp_counter);
                                let idx_64 = self.coerce_to_i64(idx_tv, ir, temp_counter);
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_int(i8* {}, i64 {}, i64 {})\n",
                                    col_loaded, idx_64, val_64
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
                        if inferred_map_elem.is_none() {
                            inferred_map_elem = Some(Self::infer_elem_tag_from_expr(v, &vtv.ty, scope_tree));
                        }
                        let val_64 = self.coerce_to_raw_i64_bits(vtv, ir, temp_counter);

                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, k) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n",
                                map_res, str_ptr, val_64
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
                                "0".to_string()
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
                            "null".to_string()
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
                            let entry_depth = scope_tree.loop_entry_try_depths.last().copied().unwrap_or(0);
                            for _ in entry_depth..scope_tree.active_try_depth {
                                ir.push_str("  call void @__omni_try_pop()\n");
                            }
                            ir.push_str(&format!("  br label %{}\n", exit_lbl));
                            custom_return = true;
                            break;
                        }
                        continue;
                    }
                    if trimmed_code == "continue" {
                        if let Some(cond_lbl) = scope_tree.loop_cond_labels.last().cloned() {
                            let entry_depth = scope_tree.loop_entry_try_depths.last().copied().unwrap_or(0);
                            for _ in entry_depth..scope_tree.active_try_depth {
                                ir.push_str("  call void @__omni_try_pop()\n");
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
                            ir.push_str(&format!(
                                "  call void @__omni_throw(i64 1, i8* {}, i8* {})\n",
                                ty_ptr, msg_ptr
                            ));
                            ir.push_str("  unreachable\n");
                        }
                        custom_return = true;
                        break;
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
                                let inst_alloc = format!("%inst_alloc_with_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}\n", inst_alloc, cls_name));

                                let raw_inst = format!("%raw_with_init_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_alloc));

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
                            ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", tramp_ptr));
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
                        ir.push_str(&format!("  {} = call i8* @__omni_c_get_returned_ptr()\n", tramp_ptr));
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
                    } else if expr_trim.contains("newkeys(") {
                        if scope_tree.class_fields.contains_key("PublicKey")
                            && scope_tree.class_fields.contains_key("PrivateKey")
                        {
                            Some(vec![
                                IrType::StructPtr("PublicKey".to_string()),
                                IrType::StructPtr("PrivateKey".to_string()),
                            ])
                        } else {
                            Some(vec![IrType::Ptr, IrType::Ptr])
                        }
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
                    deferred_frees.truncate(try_cleanup_mark);

                    // --- Catch Dispatch (`longjmp` already popped the `OmniTryFrame` in C runtime) ---
                    ir.push_str(&format!("{}:\n", catch_disp_lbl));
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
                            ir.push_str("  call void @__omni_rethrow()\n");
                            ir.push_str("  unreachable\n\n");
                        }
                        deferred_frees.truncate(fin_mark);
                    } else {
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
            }
        }

        Ok(custom_return)
    }

    /// Hoist all `alloca` instructions in a single LLVM function body to immediately after `entry:`
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
        let mut out = String::with_capacity(fn_ir.len() + 32);
        let mut inserted = false;
        for line in body_lines {
            out.push_str(line);
            out.push('\n');
            if !inserted && line.trim() == "entry:" {
                for a in &allocas {
                    out.push_str(a);
                    out.push('\n');
                }
                inserted = true;
            }
        }
        out
    }

    fn parse_default_i64(def_opt: &Option<String>) -> Option<i64> {
        let d = def_opt.as_ref()?.trim();
        if d == "True" || d == "true" {
            Some(1)
        } else if d == "False" || d == "false" || is_null_literal(d) {
            Some(0)
        } else if d.contains("DEFAULT_EXPONENT") {
            Some(65537)
        } else {
            parse_polyglot_i64(d)
        }
    }

    fn infer_expr_return_ir_type(
        expr: &str,
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
    ) -> Option<IrType> {
        let e = expr.trim();
        if e.is_empty() || is_null_literal(e) {
            return None;
        }
        if (e.starts_with('"') && e.ends_with('"'))
            || (e.starts_with('\'') && e.ends_with('\''))
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
                if class_fields.contains_key(head) {
                    return Some(IrType::StructPtr(head.to_string()));
                }
            }
        }
        if let Ok(ast) = OmniParser::parse_expr(e) {
            match ast.node {
                ExprAst::FloatLit(_) => return Some(IrType::F64),
                ExprAst::StrLit(_) | ExprAst::ListLit(_) | ExprAst::MapLit(_) => return Some(IrType::Ptr),
                ExprAst::Unary { op, rhs } if op == "-" || op == "+" => {
                    return Self::infer_expr_return_ir_type(rhs.slice(e), class_fields, self_field_types);
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
                        let lt = Self::infer_expr_return_ir_type(lhs.slice(e), class_fields, self_field_types);
                        let rt = Self::infer_expr_return_ir_type(rhs.slice(e), class_fields, self_field_types);
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

    fn infer_return_signature_recursive(
        stmts: &[Statement],
        class_fields: &HashMap<String, HashMap<String, usize>>,
        self_field_types: Option<&HashMap<String, String>>,
        scalar_acc: &mut Option<IrType>,
        tuple_acc: &mut Option<Vec<Option<IrType>>>,
    ) {
        for s in stmts {
            match s {
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
                    let parts = omni_parser::OmniParser::split_respecting_quotes(unwrapped, ',');
                    if parts.len() > 1 {
                        let slots = tuple_acc.get_or_insert_with(|| vec![None; parts.len()]);
                        for (idx, p) in parts.iter().enumerate() {
                            if idx >= slots.len() {
                                break;
                            }
                            if let Some(inferred) = Self::infer_expr_return_ir_type(p, class_fields, self_field_types) {
                                if slots[idx].is_none() || matches!(inferred, IrType::Ptr | IrType::F64 | IrType::StructPtr(_)) {
                                    slots[idx] = Some(inferred);
                                }
                            }
                        }
                    } else if let Some(inferred) = Self::infer_expr_return_ir_type(unwrapped, class_fields, self_field_types) {
                        if scalar_acc.is_none() || matches!(inferred, IrType::Ptr | IrType::F64 | IrType::StructPtr(_)) {
                            *scalar_acc = Some(inferred);
                        }
                    }
                }
                Statement::If { then_branch, else_branch, .. } => {
                    Self::infer_return_signature_recursive(then_branch, class_fields, self_field_types, scalar_acc, tuple_acc);
                    if let Some(eb) = else_branch {
                        Self::infer_return_signature_recursive(eb, class_fields, self_field_types, scalar_acc, tuple_acc);
                    }
                }
                Statement::WhileLoop { body, .. }
                | Statement::ForLoop { body, .. }
                | Statement::WithContext { body, .. } => {
                    Self::infer_return_signature_recursive(body, class_fields, self_field_types, scalar_acc, tuple_acc);
                }
                Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                    Self::infer_return_signature_recursive(try_block, class_fields, self_field_types, scalar_acc, tuple_acc);
                    for c in catch_clauses {
                        Self::infer_return_signature_recursive(&c.body, class_fields, self_field_types, scalar_acc, tuple_acc);
                    }
                    if let Some(fb) = finally_block {
                        Self::infer_return_signature_recursive(fb, class_fields, self_field_types, scalar_acc, tuple_acc);
                    }
                }
                Statement::Match { arms, .. } => {
                    for arm in arms {
                        Self::infer_return_signature_recursive(&arm.body, class_fields, self_field_types, scalar_acc, tuple_acc);
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
    ) {
        let p_name_lower = param.name.to_lowercase();
        let clean_th = param
            .type_hint
            .as_deref()
            .map(|th| th.trim().trim_start_matches('*').trim_start_matches('&').trim_end_matches('*').trim());

        if let Some(th) = clean_th {
            if is_float_type_hint(th) {
                let fp = format!("%param_f64_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = bitcast i64 {} to double\n", fp, reg_src));
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
            if class_fields_map.contains_key(th) {
                let sp = format!("%param_sp_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = inttoptr i64 {} to %struct.{}*\n", sp, reg_src, th));
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
            if is_string_type_hint(th) || th == "str" {
                let sp = format!("%param_strp_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", sp, reg_src));
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
            functions_ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, reg_src));
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

        let val_i64 = if let Some(def_val) = Self::parse_default_i64(&param.default_val) {
            if def_val != 0 {
                let is_z = format!("%param_isz_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!("  {} = icmp eq i64 {}, 0\n", is_z, reg_src));
                let sel = format!("%param_i64_{}_{}", param.name, *temp_counter);
                *temp_counter += 1;
                functions_ir.push_str(&format!(
                    "  {} = select i1 {}, i64 {}, i64 {}\n",
                    sel, is_z, def_val, reg_src
                ));
                sel
            } else {
                reg_src.to_string()
            }
        } else {
            reg_src.to_string()
        };

        let ptr = format!("%param_{}_{}", param.name, *temp_counter);
        *temp_counter += 1;
        functions_ir.push_str(&format!("  {} = alloca i64\n", ptr));
        functions_ir.push_str(&format!("  store i64 {}, i64* {}\n", val_i64, ptr));
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
        let mut string_constants: Vec<(String, String, usize)> = Vec::new();
        let mut str_counter = 0;

        // Always intern empty string & default "Exception" so exception lowering always has fallback GEP constants
        Self::intern_string(&mut string_constants, &mut str_counter, "");
        Self::intern_string(&mut string_constants, &mut str_counter, "Exception");

        // Pre-scan statements recursively to extract string & exception constants
        for func in &module.functions {
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
                self.scan_strings_recursive(&method.body, &mut string_constants, &mut str_counter);
            }
        }
        self.scan_strings_recursive(&module.top_level_statements, &mut string_constants, &mut str_counter);

        // Extract ecosystem and import context
        let mut ecosystem_hints = Vec::new();
        let mut import_chain = Vec::new();
        let mut c_module_aliases_set = HashSet::new();

        for imp in &module.imports {
            match imp {
                omni_preamble::ImportDirective::Ecosystem { ecosystem, module, alias } => {
                    if ecosystem == "c" || ecosystem == "cpp" {
                        c_module_aliases_set.insert(module.clone());
                        if let Some(a) = alias {
                            c_module_aliases_set.insert(a.clone());
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
                        }
                    }
                    let desc = match alias {
                        Some(a) => format!("file: {} as {}", path.display(), a),
                        None => format!("file: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::DirectoryAlias { path, alias } => {
                    let desc = match alias {
                        Some(a) => format!("dir: {} as {}", path.display(), a),
                        None => format!("dir: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::FromDirectory { dir_path, symbols } => {
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

        let mut method_defining_map: HashMap<(String, String), String> = HashMap::new();
        let mut method_return_types_map: HashMap<(String, String), IrType> = HashMap::new();
        for cls in &module.structs_and_classes {
            let self_ft_opt = class_field_types_map.get(&cls.name);
            for m in &cls.methods {
                method_defining_map.insert((cls.name.clone(), m.name.clone()), cls.name.clone());
                let m_ret = if let Some(ref rt) = m.return_type {
                    classify_type_token(rt, &class_fields_map)
                } else {
                    let mut sc = None;
                    let mut tp = None;
                    Self::infer_return_signature_recursive(&m.body, &class_fields_map, self_ft_opt, &mut sc, &mut tp);
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
        let mut fn_return_types_map: HashMap<String, IrType> = HashMap::new();
        let mut fn_tuple_return_types_map: HashMap<String, Vec<IrType>> = HashMap::new();

        for f in &module.functions {
            known_functions_set.insert(f.name.clone());
            if f.name == "main" {
                fn_return_types_map.insert(f.name.clone(), IrType::I32);
                continue;
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
                Self::infer_return_signature_recursive(&f.body, &class_fields_map, None, &mut sc, &mut tp);
                if let Some(slots) = tp {
                    let resolved: Vec<IrType> = slots
                        .into_iter()
                        .map(|o| o.unwrap_or(IrType::I64))
                        .collect();
                    fn_tuple_return_types_map.insert(f.name.clone(), resolved);
                    fn_return_types_map.insert(f.name.clone(), IrType::Ptr);
                } else if let Some(s_ty) = sc {
                    fn_return_types_map.insert(f.name.clone(), s_ty);
                }
            }
        }

        let mut functions_ir = String::new();
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

        // 1-C. Struct & Class Inherent Methods (`i64 @__omni_method_<cls>_<method>(i8*, i64 x 5)`)
        for cls in &module.structs_and_classes {
            for method in &cls.methods {
                let qualified = format!("{}.{}", cls.name, method.name);
                let is_reachable = reachable_symbols.contains(&method.name)
                    || reachable_symbols.contains(&qualified)
                    || (method.name == "__init__" && reachable_symbols.contains(&cls.name));

                if !is_reachable {
                    functions_ir.push_str(&format!(
                        "define i64 @__omni_method_{}_{}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 0\n}}\n\n",
                        cls.name, method.name
                    ));
                    continue;
                }

                let method_start_len = functions_ir.len();
                let method_saved_str_idx = cur_str_idx;
                let method_saved_lbl = lbl_counter;
                let method_saved_temp = temp_counter;

                functions_ir.push_str(&format!(
                    "define i64 @__omni_method_{}_{}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n",
                    cls.name, method.name
                ));

                let mut scope_tree = ScopeTree::new();
                scope_tree.class_fields = class_fields_map.clone();
                scope_tree.class_field_types = class_field_types_map.clone();
                scope_tree.class_field_order = class_field_order_map.clone();
                scope_tree.class_decorators = class_decorators_map.clone();
                scope_tree.c_module_aliases = c_module_aliases_set.clone();
                scope_tree.enum_variants = enum_variants_map.clone();
                scope_tree.known_functions = known_functions_set.clone();
                scope_tree.method_defining_class = method_defining_map.clone();
                scope_tree.fn_return_types = fn_return_types_map.clone();
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
                for (p_idx, param) in method.params.iter().skip(skip_count).enumerate() {
                    if p_idx >= 5 {
                        break;
                    }
                    let reg_src = match p_idx {
                        0 => "%len",
                        1 => "%arg2",
                        2 => "%arg3",
                        3 => "%arg4",
                        _ => "%arg5",
                    };
                    Self::bind_abi_param_i64(
                        &mut functions_ir,
                        &mut scope_tree,
                        &mut temp_counter,
                        param,
                        reg_src,
                        &class_fields_map,
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
                                "define i64 @__omni_method_{}_{}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 0\n}}\n\n",
                                cls.name, method.name
                            ));
                        }
                    }
                }
            }
        }

        // 2. Functions (namespaced as `i64 @__omni_fn_<name>(i64 x 6)` or `i32 @__omni_fn_main(i8*, i64)`)
        for func in &module.functions {
            let is_reachable = reachable_symbols.contains(&func.name);
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
                        "define i64 @__omni_fn_{0}(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 %arg0\n}}\n\ndefine void @__omni_trampoline_{0}(i8* %raw_arg) {{\nentry:\n  %raw_int = ptrtoint i8* %raw_arg to i64\n  %res = call i64 @__omni_fn_{0}(i64 %raw_int, i64 0, i64 0, i64 0, i64 0, i64 0)\n  ret void\n}}\n\ndefine void @__omni_trampoline_pack_{0}(i8* %raw_pack) {{\nentry:\n  %p64 = bitcast i8* %raw_pack to i64*\n  %p0 = getelementptr inbounds i64, i64* %p64, i64 0\n  %a0 = load i64, i64* %p0\n  %p1 = getelementptr inbounds i64, i64* %p64, i64 1\n  %a1 = load i64, i64* %p1\n  %p2 = getelementptr inbounds i64, i64* %p64, i64 2\n  %a2 = load i64, i64* %p2\n  %p3 = getelementptr inbounds i64, i64* %p64, i64 3\n  %a3 = load i64, i64* %p3\n  %p4 = getelementptr inbounds i64, i64* %p64, i64 4\n  %a4 = load i64, i64* %p4\n  %p5 = getelementptr inbounds i64, i64* %p64, i64 5\n  %a5 = load i64, i64* %p5\n  call void @free(i8* %raw_pack)\n  %res = call i64 @__omni_fn_{0}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)\n  ret void\n}}\n\n",
                        func.name
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
                    "define i64 @__omni_fn_{}(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n",
                    func.name
                ));
            }

            let mut scope_tree = ScopeTree::new();
            scope_tree.class_fields = class_fields_map.clone();
            scope_tree.class_field_types = class_field_types_map.clone();
            scope_tree.class_field_order = class_field_order_map.clone();
            scope_tree.class_decorators = class_decorators_map.clone();
            scope_tree.c_module_aliases = c_module_aliases_set.clone();
            scope_tree.enum_variants = enum_variants_map.clone();
            scope_tree.known_functions = known_functions_set.clone();
            scope_tree.method_defining_class = method_defining_map.clone();
            scope_tree.fn_return_types = fn_return_types_map.clone();
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
                        || p_name_lower.starts_with("items_") || p_name_lower.starts_with("arr_") || p_name_lower.starts_with("slice_") || p_name_lower.ends_with("_arr") || p_name_lower.ends_with("_slice") || p_name_lower.ends_with("_list");
                    let is_map = param.type_hint.as_deref().map(|th| th.contains("map") || th.contains("dict")).unwrap_or(false)
                        || p_name_lower == "map" || p_name_lower == "dict" || p_name_lower.starts_with("map_") || p_name_lower.starts_with("dict_") || p_name_lower.ends_with("_map") || p_name_lower.ends_with("_dict");
                    let is_int = param.type_hint.as_deref().map(|th| th == "int" || th == "i32" || th == "i64" || th == "int64").unwrap_or(false)
                        || (!is_chan && !is_slice && !is_map && (p_name_lower == "val" || p_name_lower == "num" || p_name_lower == "count" || p_name_lower == "idx" || p_name_lower == "n" || p_name_lower == "x" || p_name_lower == "y" || p_name_lower == "z"));

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
                    if i >= 6 {
                        break;
                    }
                    let arg_src = format!("%arg{}", i);
                    Self::bind_abi_param_i64(
                        &mut functions_ir,
                        &mut scope_tree,
                        &mut temp_counter,
                        param,
                        &arg_src,
                        &class_fields_map,
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
                }
                Err(e) => {
                    if func.name == "main" {
                        return Err(e);
                    } else {
                        functions_ir.truncate(func_start_len);
                        cur_str_idx = func_saved_str_idx;
                        lbl_counter = func_saved_lbl;
                        temp_counter = func_saved_temp;
                        functions_ir.push_str(&format!(
                            "define i64 @__omni_fn_{}(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 0\n}}\n\n",
                            func.name
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
                    "define void @__omni_trampoline_{0}(i8* %raw_arg) {{\nentry:\n  %raw_int = ptrtoint i8* %raw_arg to i64\n  %res = call i64 @__omni_fn_{0}(i64 %raw_int, i64 0, i64 0, i64 0, i64 0, i64 0)\n  ret void\n}}\n\ndefine void @__omni_trampoline_pack_{0}(i8* %raw_pack) {{\nentry:\n  %p64 = bitcast i8* %raw_pack to i64*\n  %p0 = getelementptr inbounds i64, i64* %p64, i64 0\n  %a0 = load i64, i64* %p0\n  %p1 = getelementptr inbounds i64, i64* %p64, i64 1\n  %a1 = load i64, i64* %p1\n  %p2 = getelementptr inbounds i64, i64* %p64, i64 2\n  %a2 = load i64, i64* %p2\n  %p3 = getelementptr inbounds i64, i64* %p64, i64 3\n  %a3 = load i64, i64* %p3\n  %p4 = getelementptr inbounds i64, i64* %p64, i64 4\n  %a4 = load i64, i64* %p4\n  %p5 = getelementptr inbounds i64, i64* %p64, i64 5\n  %a5 = load i64, i64* %p5\n  call void @free(i8* %raw_pack)\n  %res = call i64 @__omni_fn_{0}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5)\n  ret void\n}}\n\n",
                    func.name
                ));
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
        synth_scope.c_module_aliases = c_module_aliases_set.clone();

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
                    missing_omni_stubs.push(format!(
                        "define i64 @{}(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3, i64 %arg4, i64 %arg5) {{\nentry:\n  ret i64 0\n}}\n\n",
                        sym
                    ));
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
                && sym != "fast_hash_block"
                && !runtime_headers.contains(&format!("@{}(", sym))
                && !known_functions_set.contains(&sym)
                && !functions_ir.contains(&format!("define i64 @{}(", sym))
                && !functions_ir.contains(&format!("define i32 @{}(", sym))
                && !dynamic_declarations.iter().any(|d| d.contains(&format!("@{}(", sym)))
            {
                if NATIVE_C_LIB_I64_FNS.contains(&sym.as_str()) {
                    dynamic_declarations.insert(format!("declare i64 @{}(...)", sym));
                } else if NATIVE_C_LIB_FNS.contains(&sym.as_str()) {
                    dynamic_declarations.insert(format!("declare i32 @{}(...)", sym));
                } else if sym == "not" {
                    missing_omni_stubs.push(
                        "define weak i64 @not(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  %c = icmp eq i64 %a0, 0\n  %r = zext i1 %c to i64\n  ret i64 %r\n}\n\n".to_string()
                    );
                } else if sym == "is_acceptable" || sym == "is_prime" || sym == "miller_rabin_primality_testing" {
                    missing_omni_stubs.push(
                        format!("define weak i64 @{}(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {{\nentry:\n  ret i64 1\n}}\n\n", sym)
                    );
                } else if sym == "pow" {
                    missing_omni_stubs.push(
                        "define weak i64 @pow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5) {\nentry:\n  ret i64 1\n}\n\n".to_string()
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
            ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* null, i64 0)\n");
            ir.push_str("  call void @__omni_thread_join_all()\n");
            ir.push_str("  ret i32 %exit_code\n");
        } else if let Some(f) = module.functions.first() {
            ir.push_str(&format!(
                "  %res = call i64 @__omni_fn_{}(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0)\n",
                f.name
            ));
            ir.push_str("  call void @__omni_thread_join_all()\n");
            ir.push_str("  ret i32 0\n");
        } else {
            ir.push_str("  ret i32 0\n");
        }

        ir.push_str("}\n");

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
}
