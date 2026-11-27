//! Pure-Rust, zero-`clang` C99 Frontend -> LLVM SSA IR Lowerer.
//!
//! Eliminates the external `clang -S -emit-llvm` dependency for `.c` and `.h`
//! source ingestion across Linux, macOS, Windows, and bare-metal Cortex-M targets:
//!
//! 1. **C99 Preprocessor (`preprocess_c`)**: line-splicing, comments, `#include`,
//!    `#pragma once`, virtual standard C headers (`<stdint.h>`, `<stddef.h>`,
//!    `<stdbool.h>`, `<limits.h>`, `<string.h>`, `<stdlib.h>`, `<stdio.h>`, `<math.h>`),
//!    object-like & function-like `#define` macros (`#`, `##`, `__VA_ARGS__`),
//!    `#undef`, and `#if`/`#ifdef`/`#ifndef`/`#elif`/`#else`/`#endif`.
//! 2. **Typedef-Aware C99 Parser**: full C99 expressions, statements, `struct`,
//!    `union`, `enum`, `typedef`, arrays, pointers, function pointers, `sizeof`,
//!    `switch` fallthrough, `do`/`while`, `goto`, `static` locals, and globals.
//! 3. **LLVM SSA IR Lowerer**: emits classic typed-pointer LLVM IR directly into
//!    `ir_ingest::slice_and_adapt_ll_module` -> Cranelift JIT / AOT / Cortex-M.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Public Entrypoints
// ============================================================================

/// Compile a `.c` / `.h` file on disk directly to textual LLVM SSA IR without `clang`.
pub fn compile_c_file_to_ll(
    src_path: &Path,
    include_dirs: &[PathBuf],
    target_triple: &str,
) -> Result<String, String> {
    let raw = fs::read_to_string(src_path)
        .map_err(|e| format!("c_frontend: failed to read '{}': {}", src_path.display(), e))?;
    let mut dirs = include_dirs.to_vec();
    if let Some(parent) = src_path.parent() {
        if !parent.as_os_str().is_empty() && !dirs.contains(&parent.to_path_buf()) {
            dirs.insert(0, parent.to_path_buf());
        }
    }
    compile_c_source_str_to_ll(&raw, Some(src_path), &dirs, target_triple)
}

/// Compile an in-memory C99 source string directly to textual LLVM SSA IR without `clang`.
pub fn compile_c_source_str_to_ll(
    source: &str,
    source_path: Option<&Path>,
    include_dirs: &[PathBuf],
    target_triple: &str,
) -> Result<String, String> {
    let preprocessed = preprocess_c(source, source_path, include_dirs, target_triple)?;
    let tokens = lex_c(&preprocessed)?;
    let mut parser = CParser::new(tokens, target_triple);
    let tu = parser.parse_translation_unit()?;
    let mut lowerer = CLlvmLowerer::new(target_triple, parser.structs, parser.unions, parser.enum_consts);
    lowerer.emit_tu(&tu)
}

// ============================================================================
// Virtual Standard C Headers (Zero `/usr/include` Dependency)
// ============================================================================

fn virtual_std_header(name: &str) -> Option<&'static str> {
    match name {
        "stdint.h" | "inttypes.h" => Some(
            r#"
typedef signed char int8_t;
typedef short int16_t;
typedef int int32_t;
typedef long long int64_t;
typedef unsigned char uint8_t;
typedef unsigned short uint16_t;
typedef unsigned int uint32_t;
typedef unsigned long long uint64_t;
typedef long intptr_t;
typedef unsigned long uintptr_t;
typedef long long intmax_t;
typedef unsigned long long uintmax_t;
typedef unsigned long size_t;
typedef long ptrdiff_t;
typedef long ssize_t;
#define INT8_MIN (-128)
#define INT8_MAX 127
#define UINT8_MAX 255
#define INT16_MIN (-32768)
#define INT16_MAX 32767
#define UINT16_MAX 65535
#define INT32_MIN (-2147483647 - 1)
#define INT32_MAX 2147483647
#define UINT32_MAX 4294967295U
#define INT64_MIN (-9223372036854775807LL - 1)
#define INT64_MAX 9223372036854775807LL
#define UINT64_MAX 18446744073709551615ULL
"#,
        ),
        "stddef.h" | "sys/types.h" => Some(
            r#"
typedef unsigned long size_t;
typedef long ptrdiff_t;
typedef long ssize_t;
typedef int wchar_t;
#ifndef NULL
#define NULL ((void*)0)
#endif
#define offsetof(type, member) ((size_t)&(((type*)0)->member))
"#,
        ),
        "stdbool.h" => Some(
            r#"
#ifndef bool
#define bool _Bool
#endif
#define true 1
#define false 0
"#,
        ),
        "limits.h" => Some(
            r#"
#define CHAR_BIT 8
#define SCHAR_MIN (-128)
#define SCHAR_MAX 127
#define UCHAR_MAX 255
#define SHRT_MIN (-32768)
#define SHRT_MAX 32767
#define USHRT_MAX 65535
#define INT_MIN (-2147483647 - 1)
#define INT_MAX 2147483647
#define UINT_MAX 4294967295U
#define LONG_MAX 9223372036854775807LL
#define ULONG_MAX 18446744073709551615ULL
"#,
        ),
        "string.h" | "strings.h" => Some(
            r#"
typedef unsigned long size_t;
#ifndef NULL
#define NULL ((void*)0)
#endif
extern void *memcpy(void *dst, const void *src, size_t n);
extern void *memmove(void *dst, const void *src, size_t n);
extern void *memset(void *dst, int c, size_t n);
extern int memcmp(const void *a, const void *b, size_t n);
extern size_t strlen(const char *s);
extern int strcmp(const char *a, const char *b);
extern int strncmp(const char *a, const char *b, size_t n);
extern char *strcpy(char *dst, const char *src);
extern char *strncpy(char *dst, const char *src, size_t n);
extern char *strchr(const char *s, int c);
"#,
        ),
        "stdlib.h" => Some(
            r#"
typedef unsigned long size_t;
#ifndef NULL
#define NULL ((void*)0)
#endif
extern void *malloc(size_t sz);
extern void *calloc(size_t n, size_t sz);
extern void *realloc(void *ptr, size_t sz);
extern void free(void *ptr);
extern int abs(int x);
extern long labs(long x);
extern void exit(int code);
extern void abort(void);
extern int atoi(const char *s);
extern long atol(const char *s);
"#,
        ),
        "stdio.h" => Some(
            r#"
typedef unsigned long size_t;
typedef void FILE;
#ifndef NULL
#define NULL ((void*)0)
#endif
#define EOF (-1)
extern int printf(const char *fmt, ...);
extern int fprintf(FILE *f, const char *fmt, ...);
extern int sprintf(char *buf, const char *fmt, ...);
extern int snprintf(char *buf, size_t n, const char *fmt, ...);
extern int puts(const char *s);
extern int putchar(int c);
extern int getchar(void);
"#,
        ),
        "math.h" => Some(
            r#"
#define M_PI 3.14159265358979323846
extern double sqrt(double x);
extern double fabs(double x);
extern double floor(double x);
extern double ceil(double x);
extern double sin(double x);
extern double cos(double x);
extern double tan(double x);
extern double pow(double a, double b);
extern double exp(double x);
extern double log(double x);
extern float sqrtf(float x);
extern float fabsf(float x);
"#,
        ),
        "ctype.h" => Some(
            r#"
extern int isspace(int c);
extern int isdigit(int c);
extern int isalpha(int c);
extern int isalnum(int c);
extern int tolower(int c);
extern int toupper(int c);
"#,
        ),
        "assert.h" => Some("#define assert(x) ((void)0)\n"),
        "stdarg.h" | "errno.h" | "float.h" => Some(""),
        _ => None,
    }
}

// ============================================================================
// Stage 1: C99 Preprocessor
// ============================================================================

#[derive(Debug, Clone)]
struct MacroDef {
    params: Option<Vec<String>>,
    variadic: bool,
    body: String,
}

struct Preprocessor {
    macros: HashMap<String, MacroDef>,
    include_dirs: Vec<PathBuf>,
    pragma_once: HashSet<String>,
}

impl Preprocessor {
    fn new(include_dirs: &[PathBuf], target_triple: &str) -> Self {
        let mut macros = HashMap::new();
        let mut def = |k: &str, v: &str| {
            macros.insert(
                k.to_string(),
                MacroDef {
                    params: None,
                    variadic: false,
                    body: v.to_string(),
                },
            );
        };
        def("__STDC__", "1");
        def("__STDC_VERSION__", "199901L");
        def("__STDC_HOSTED__", "1");
        def("__OMNI_C_FRONTEND__", "1");
        let t = target_triple.to_ascii_lowercase();
        if t.contains("x86_64") || t.contains("amd64") {
            def("__x86_64__", "1");
            def("__LP64__", "1");
            def("__SIZEOF_POINTER__", "8");
        } else if t.contains("aarch64") || t.contains("arm64") {
            def("__aarch64__", "1");
            def("__LP64__", "1");
            def("__SIZEOF_POINTER__", "8");
        } else if t.contains("thumb") || t.contains("arm") {
            def("__arm__", "1");
            def("__ILP32__", "1");
            def("__SIZEOF_POINTER__", "4");
        } else {
            def("__x86_64__", "1");
            def("__LP64__", "1");
            def("__SIZEOF_POINTER__", "8");
        }
        if t.contains("linux") {
            def("__linux__", "1");
            def("__unix__", "1");
        } else if t.contains("apple") || t.contains("darwin") {
            def("__APPLE__", "1");
            def("__unix__", "1");
        } else if t.contains("windows") {
            def("_WIN32", "1");
            def("_WIN64", "1");
        }
        Self {
            macros,
            include_dirs: include_dirs.to_vec(),
            pragma_once: HashSet::new(),
        }
    }

    fn process(&mut self, src: &str, cur_file: Option<&Path>, depth: usize) -> Result<String, String> {
        if depth > 32 {
            return Err("c_frontend: #include recursion depth exceeded (>32)".to_string());
        }
        let cleaned = strip_c_comments_and_splice(src);
        let mut out = String::with_capacity(cleaned.len());
        // Conditional compilation stack: (parent_active, branch_active, any_branch_taken)
        let mut cond_stack: Vec<(bool, bool, bool)> = Vec::new();
        let is_active = |st: &[(bool, bool, bool)]| st.iter().all(|&(_, a, _)| a);

        for raw_line in cleaned.lines() {
            let trimmed = raw_line.trim();
            if let Some(dir_rest) = trimmed.strip_prefix('#') {
                let dir_rest = dir_rest.trim_start();
                let (kw, arg) = split_ident_head(dir_rest);
                let arg = arg.trim();
                match kw {
                    "ifdef" => {
                        let parent = is_active(&cond_stack);
                        let (name, _) = split_ident_head(arg);
                        let cond = parent && self.macros.contains_key(name);
                        cond_stack.push((parent, cond, cond));
                    }
                    "ifndef" => {
                        let parent = is_active(&cond_stack);
                        let (name, _) = split_ident_head(arg);
                        let cond = parent && !self.macros.contains_key(name);
                        cond_stack.push((parent, cond, cond));
                    }
                    "if" => {
                        let parent = is_active(&cond_stack);
                        let cond = if parent {
                            self.eval_pp_expr(arg) != 0
                        } else {
                            false
                        };
                        cond_stack.push((parent, cond, cond));
                    }
                    "elif" => {
                        if let Some((parent, active, taken)) = cond_stack.last_mut() {
                            if *parent && !*taken {
                                let cond = self.eval_pp_expr(arg) != 0;
                                *active = cond;
                                *taken = cond;
                            } else {
                                *active = false;
                            }
                        }
                    }
                    "else" => {
                        if let Some((parent, active, taken)) = cond_stack.last_mut() {
                            *active = *parent && !*taken;
                            *taken = true;
                        }
                    }
                    "endif" => {
                        cond_stack.pop();
                    }
                    _ if !is_active(&cond_stack) => {}
                    "pragma" => {
                        if arg == "once" {
                            if let Some(p) = cur_file {
                                self.pragma_once.insert(p.to_string_lossy().to_string());
                            }
                        }
                    }
                    "define" => {
                        self.parse_define(arg);
                    }
                    "undef" => {
                        let (name, _) = split_ident_head(arg);
                        if !name.is_empty() {
                            self.macros.remove(name);
                        }
                    }
                    "include" => {
                        let expanded_arg = self.expand_line(arg, &HashSet::new());
                        let inc_trim = expanded_arg.trim();
                        let (is_angle, inc_name) = if inc_trim.starts_with('<') && inc_trim.ends_with('>') {
                            (true, &inc_trim[1..inc_trim.len() - 1])
                        } else if inc_trim.starts_with('"') && inc_trim.ends_with('"') {
                            (false, &inc_trim[1..inc_trim.len() - 1])
                        } else {
                            continue;
                        };
                        if let Some((hdr_text, hdr_path_opt, key)) =
                            self.resolve_include(inc_name, is_angle, cur_file)
                        {
                            if !self.pragma_once.contains(&key) {
                                let sub = self.process(&hdr_text, hdr_path_opt.as_deref(), depth + 1)?;
                                out.push_str(&sub);
                                out.push('\n');
                            }
                        }
                    }
                    "error" => {
                        return Err(format!("c_frontend: #error {}", arg));
                    }
                    _ => {}
                }
                continue;
            }
            if !is_active(&cond_stack) {
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            let expanded = self.expand_line(raw_line, &HashSet::new());
            out.push_str(&expanded);
            out.push('\n');
        }
        Ok(out)
    }

    fn resolve_include(
        &self,
        name: &str,
        is_angle: bool,
        cur_file: Option<&Path>,
    ) -> Option<(String, Option<PathBuf>, String)> {
        if is_angle {
            if let Some(v) = virtual_std_header(name) {
                return Some((v.to_string(), None, format!("<{}>", name)));
            }
        }
        // Check relative to current file first (for `"..."`)
        if !is_angle {
            if let Some(cf) = cur_file {
                if let Some(dir) = cf.parent() {
                    let cand = dir.join(name);
                    if cand.is_file() {
                        if let Ok(text) = fs::read_to_string(&cand) {
                            let canon = fs::canonicalize(&cand).unwrap_or(cand);
                            let key = canon.to_string_lossy().to_string();
                            return Some((text, Some(canon), key));
                        }
                    }
                }
            }
        }
        // Check explicit include dirs
        for inc_dir in &self.include_dirs {
            let cand = inc_dir.join(name);
            if cand.is_file() {
                if let Ok(text) = fs::read_to_string(&cand) {
                    let canon = fs::canonicalize(&cand).unwrap_or(cand);
                    let key = canon.to_string_lossy().to_string();
                    return Some((text, Some(canon), key));
                }
            }
        }
        // Check absolute path
        let direct = PathBuf::from(name);
        if direct.is_file() {
            if let Ok(text) = fs::read_to_string(&direct) {
                let canon = fs::canonicalize(&direct).unwrap_or(direct);
                let key = canon.to_string_lossy().to_string();
                return Some((text, Some(canon), key));
            }
        }
        // Fallback to virtual standard header even for `"stdint.h"`
        if let Some(v) = virtual_std_header(name) {
            return Some((v.to_string(), None, format!("<{}>", name)));
        }
        None
    }

    fn parse_define(&mut self, arg: &str) {
        let (name, rest) = split_ident_head(arg);
        if name.is_empty() {
            return;
        }
        // Function-like macro ONLY if '(' immediately follows macro name with no whitespace
        if let Some(after_lp) = rest.strip_prefix('(') {
            if let Some(rp) = after_lp.find(')') {
                let param_str = &after_lp[..rp];
                let body = after_lp[rp + 1..].trim().to_string();
                let mut params = Vec::new();
                let mut variadic = false;
                for p in param_str.split(',') {
                    let pt = p.trim();
                    if pt.is_empty() {
                        continue;
                    }
                    if pt == "..." {
                        variadic = true;
                    } else if let Some(named_va) = pt.strip_suffix("...") {
                        params.push(named_va.trim().to_string());
                        variadic = true;
                    } else {
                        params.push(pt.to_string());
                    }
                }
                self.macros.insert(
                    name.to_string(),
                    MacroDef {
                        params: Some(params),
                        variadic,
                        body,
                    },
                );
                return;
            }
        }
        self.macros.insert(
            name.to_string(),
            MacroDef {
                params: None,
                variadic: false,
                body: rest.trim().to_string(),
            },
        );
    }

    fn expand_line(&self, line: &str, hidden: &HashSet<String>) -> String {
        let bytes = line.as_bytes();
        let mut out = String::with_capacity(line.len());
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            // Preserve string & char literals verbatim
            if b == b'"' || b == b'\'' {
                let quote = b;
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                out.push_str(&line[start..i]);
                continue;
            }
            if is_ident_start(b) {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_cont(bytes[i]) {
                    i += 1;
                }
                let ident = &line[start..i];
                if !hidden.contains(ident) {
                    if let Some(mdef) = self.macros.get(ident) {
                        if let Some(ref params) = mdef.params {
                            // Look ahead for '(' skipping whitespace
                            let mut j = i;
                            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                                j += 1;
                            }
                            if j < bytes.len() && bytes[j] == b'(' {
                                if let Some((args, after_rp)) = parse_macro_call_args(line, j) {
                                    let mut next_hidden = hidden.clone();
                                    next_hidden.insert(ident.to_string());
                                    let substituted = substitute_macro_body(
                                        &mdef.body,
                                        params,
                                        mdef.variadic,
                                        &args,
                                        self,
                                        hidden,
                                    );
                                    let reexpanded = self.expand_line(&substituted, &next_hidden);
                                    out.push_str(&reexpanded);
                                    i = after_rp;
                                    continue;
                                }
                            }
                        } else {
                            let mut next_hidden = hidden.clone();
                            next_hidden.insert(ident.to_string());
                            let reexpanded = self.expand_line(&mdef.body, &next_hidden);
                            out.push_str(&reexpanded);
                            continue;
                        }
                    }
                }
                out.push_str(ident);
                continue;
            }
            out.push(b as char);
            i += 1;
        }
        out
    }

    fn eval_pp_expr(&self, expr: &str) -> i64 {
        // First replace `defined(X)` and `defined X` before macro expansion
        let mut pre = String::with_capacity(expr.len());
        let bytes = expr.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if is_ident_start(bytes[i]) {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_cont(bytes[i]) {
                    i += 1;
                }
                let word = &expr[start..i];
                if word == "defined" {
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    let has_paren = i < bytes.len() && bytes[i] == b'(';
                    if has_paren {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                            i += 1;
                        }
                    }
                    let ns = i;
                    while i < bytes.len() && is_ident_cont(bytes[i]) {
                        i += 1;
                    }
                    let nm = &expr[ns..i];
                    if has_paren {
                        while i < bytes.len() && bytes[i] != b')' {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1;
                        }
                    }
                    pre.push_str(if self.macros.contains_key(nm) { "1" } else { "0" });
                    continue;
                }
                pre.push_str(word);
                continue;
            }
            pre.push(bytes[i] as char);
            i += 1;
        }
        let expanded = self.expand_line(&pre, &HashSet::new());
        let Ok(toks) = lex_c(&expanded) else {
            return 0;
        };
        let mut pos = 0usize;
        eval_pp_ternary(&toks, &mut pos)
    }
}

fn preprocess_c(
    src: &str,
    cur_file: Option<&Path>,
    include_dirs: &[PathBuf],
    target_triple: &str,
) -> Result<String, String> {
    let mut pp = Preprocessor::new(include_dirs, target_triple);
    pp.process(src, cur_file, 0)
}

fn strip_c_comments_and_splice(src: &str) -> String {
    // 1. Normalize CRLF and splice `\\\n`
    let norm = src.replace("\r\n", "\n").replace("\\\n", "");
    let bytes = norm.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            let q = b;
            out.push(b as char);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out.push(bytes[i] as char);
                    out.push(bytes[i + 1] as char);
                    i += 2;
                    continue;
                }
                out.push(bytes[i] as char);
                let cur = bytes[i];
                i += 1;
                if cur == q {
                    break;
                }
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(' ');
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn split_ident_head(s: &str) -> (&str, &str) {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && is_ident_start(b[i]) {
        i += 1;
        while i < b.len() && is_ident_cont(b[i]) {
            i += 1;
        }
    }
    (&s[..i], &s[i..])
}

fn parse_macro_call_args(line: &str, lparen_idx: usize) -> Option<(Vec<String>, usize)> {
    let bytes = line.as_bytes();
    let mut i = lparen_idx + 1;
    let mut depth = 1usize;
    let mut args = Vec::new();
    let mut cur = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            let q = b;
            cur.push(b as char);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    cur.push(bytes[i] as char);
                    cur.push(bytes[i + 1] as char);
                    i += 2;
                    continue;
                }
                cur.push(bytes[i] as char);
                let ch = bytes[i];
                i += 1;
                if ch == q {
                    break;
                }
            }
            continue;
        }
        if b == b'(' {
            depth += 1;
            cur.push('(');
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                if !cur.trim().is_empty() || !args.is_empty() {
                    args.push(cur.trim().to_string());
                }
                return Some((args, i + 1));
            }
            cur.push(')');
        } else if b == b',' && depth == 1 {
            args.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(b as char);
        }
        i += 1;
    }
    None
}

fn substitute_macro_body(
    body: &str,
    params: &[String],
    variadic: bool,
    args: &[String],
    pp: &Preprocessor,
    hidden: &HashSet<String>,
) -> String {
    let mut param_map: HashMap<&str, String> = HashMap::new();
    for (idx, p) in params.iter().enumerate() {
        let raw_arg = args.get(idx).cloned().unwrap_or_default();
        param_map.insert(p.as_str(), raw_arg);
    }
    let va_str = if variadic && args.len() > params.len() {
        args[params.len()..].join(", ")
    } else {
        String::new()
    };

    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < bytes.len() {
        // Stringification `#param`
        if bytes[i] == b'#' && (i + 1 >= bytes.len() || bytes[i + 1] != b'#') {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let (nm, _) = split_ident_head(&body[j..]);
            if let Some(val) = param_map.get(nm) {
                let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
                i = j + nm.len();
                continue;
            }
        }
        // Token pasting `##`
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
            while out.ends_with(|c: char| c.is_ascii_whitespace()) {
                out.pop();
            }
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                let ch = bytes[i];
                i += 1;
                if ch == q {
                    break;
                }
            }
            out.push_str(&body[start..i]);
            continue;
        }
        if is_ident_start(bytes[i]) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let ident = &body[start..i];
            if ident == "__VA_ARGS__" {
                out.push_str(&va_str);
            } else if let Some(val) = param_map.get(ident) {
                // If adjacent to `##`, use raw unexpanded arg; otherwise expand arg
                let mut k = i;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                let next_is_paste = k + 1 < bytes.len() && bytes[k] == b'#' && bytes[k + 1] == b'#';
                if next_is_paste {
                    out.push_str(val);
                } else {
                    out.push_str(&pp.expand_line(val, hidden));
                }
            } else {
                out.push_str(ident);
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

// ============================================================================
// Stage 2: Lexer
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum CTok {
    Ident(String),
    IntLit(u64, bool, u8), // (value, unsigned, min_bits: 32 or 64)
    FloatLit(f64, bool),   // (value, is_f32)
    StrLit(Vec<u8>),
    Sym(&'static str),
    Eof,
}

fn lex_c(src: &str) -> Result<Vec<CTok>, String> {
    let bytes = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;

    const MULTI_SYMS: &[&str] = &[
        "<<=", ">>=", "...", "->", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
        "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    ];

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // String literal (concatenate adjacent string literals automatically)
        if b == b'"' {
            let mut sbytes = Vec::new();
            while i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        let (ch, adv) = decode_c_escape(bytes, i + 1);
                        sbytes.push(ch);
                        i += 1 + adv;
                    } else {
                        sbytes.push(bytes[i]);
                        i += 1;
                    }
                }
                if i < bytes.len() && bytes[i] == b'"' {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            toks.push(CTok::StrLit(sbytes));
            continue;
        }
        // Char literal
        if b == b'\'' {
            i += 1;
            let val = if i < bytes.len() && bytes[i] == b'\\' && i + 1 < bytes.len() {
                let (ch, adv) = decode_c_escape(bytes, i + 1);
                i += 1 + adv;
                ch as u64
            } else if i < bytes.len() {
                let ch = bytes[i] as u64;
                i += 1;
                ch
            } else {
                0
            };
            while i < bytes.len() && bytes[i] != b'\'' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            toks.push(CTok::IntLit(val, false, 32));
            continue;
        }
        // Number literal
        if b.is_ascii_digit() || (b == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            let mut is_float = false;
            if b == b'0' && i + 1 < bytes.len() && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let hex_str = &src[start + 2..i];
                let val = u64::from_str_radix(hex_str, 16).unwrap_or(0);
                let (uns, bits) = parse_int_suffix(bytes, &mut i, val);
                toks.push(CTok::IntLit(val, uns, bits));
                continue;
            }
            if b == b'0' && i + 1 < bytes.len() && (bytes[i + 1] == b'b' || bytes[i + 1] == b'B') {
                i += 2;
                while i < bytes.len() && (bytes[i] == b'0' || bytes[i] == b'1') {
                    i += 1;
                }
                let bin_str = &src[start + 2..i];
                let val = u64::from_str_radix(bin_str, 2).unwrap_or(0);
                let (uns, bits) = parse_int_suffix(bytes, &mut i, val);
                toks.push(CTok::IntLit(val, uns, bits));
                continue;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                is_float = true;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                is_float = true;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if is_float {
                let num_str = &src[start..i];
                let mut is_f32 = false;
                if i < bytes.len() && (bytes[i] == b'f' || bytes[i] == b'F') {
                    is_f32 = true;
                    i += 1;
                } else if i < bytes.len() && (bytes[i] == b'l' || bytes[i] == b'L') {
                    i += 1;
                }
                let val = num_str.parse::<f64>().unwrap_or(0.0);
                toks.push(CTok::FloatLit(val, is_f32));
            } else {
                let num_str = &src[start..i];
                let val = if num_str.len() > 1 && num_str.starts_with('0') {
                    u64::from_str_radix(&num_str[1..], 8).unwrap_or(0)
                } else {
                    num_str.parse::<u64>().unwrap_or(0)
                };
                let (uns, bits) = parse_int_suffix(bytes, &mut i, val);
                toks.push(CTok::IntLit(val, uns, bits));
            }
            continue;
        }
        // Identifier or keyword
        if is_ident_start(b) {
            let start = i;
            i += 1;
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let ident = &src[start..i];
            // Strip GCC/Clang `__attribute__((...))` cleanly
            if ident == "__attribute__" || ident == "__attribute" {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'(' {
                    let mut depth = 0usize;
                    while i < bytes.len() {
                        if bytes[i] == b'(' {
                            depth += 1;
                        } else if bytes[i] == b')' {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        i += 1;
                    }
                }
                continue;
            }
            if matches!(ident, "__extension__" | "__restrict" | "__restrict__" | "restrict") {
                continue;
            }
            toks.push(CTok::Ident(ident.to_string()));
            continue;
        }
        // Multi-char operators
        let rem = &src[i..];
        let mut matched_sym = None;
        for &sym in MULTI_SYMS {
            if rem.starts_with(sym) {
                matched_sym = Some(sym);
                i += sym.len();
                break;
            }
        }
        if let Some(sym) = matched_sym {
            toks.push(CTok::Sym(sym));
            continue;
        }
        // Single-char punctuation/operator
        let single = match b {
            b'(' => "(",
            b')' => ")",
            b'{' => "{",
            b'}' => "}",
            b'[' => "[",
            b']' => "]",
            b';' => ";",
            b',' => ",",
            b'.' => ".",
            b':' => ":",
            b'?' => "?",
            b'+' => "+",
            b'-' => "-",
            b'*' => "*",
            b'/' => "/",
            b'%' => "%",
            b'&' => "&",
            b'|' => "|",
            b'^' => "^",
            b'~' => "~",
            b'!' => "!",
            b'=' => "=",
            b'<' => "<",
            b'>' => ">",
            _ => {
                i += 1;
                continue;
            }
        };
        toks.push(CTok::Sym(single));
        i += 1;
    }
    toks.push(CTok::Eof);
    Ok(toks)
}

fn parse_int_suffix(bytes: &[u8], i: &mut usize, val: u64) -> (bool, u8) {
    let mut uns = false;
    let mut long_cnt = 0u8;
    while *i < bytes.len() {
        match bytes[*i] {
            b'u' | b'U' => {
                uns = true;
                *i += 1;
            }
            b'l' | b'L' => {
                long_cnt += 1;
                *i += 1;
            }
            _ => break,
        }
    }
    let bits = if long_cnt >= 1 || val > i32::MAX as u64 {
        64
    } else {
        32
    };
    (uns, bits)
}

fn decode_c_escape(bytes: &[u8], idx: usize) -> (u8, usize) {
    if idx >= bytes.len() {
        return (0, 0);
    }
    match bytes[idx] {
        b'n' => (b'\n', 1),
        b'r' => (b'\r', 1),
        b't' => (b'\t', 1),
        b'0'..=b'7' => {
            let mut val = 0u8;
            let mut n = 0usize;
            while n < 3 && idx + n < bytes.len() && (b'0'..=b'7').contains(&bytes[idx + n]) {
                val = val.wrapping_mul(8).wrapping_add(bytes[idx + n] - b'0');
                n += 1;
            }
            (val, n)
        }
        b'x' => {
            let mut val = 0u8;
            let mut n = 1usize;
            while n < 3 && idx + n < bytes.len() && bytes[idx + n].is_ascii_hexdigit() {
                let d = match bytes[idx + n] {
                    b'0'..=b'9' => bytes[idx + n] - b'0',
                    b'a'..=b'f' => bytes[idx + n] - b'a' + 10,
                    b'A'..=b'F' => bytes[idx + n] - b'A' + 10,
                    _ => 0,
                };
                val = val.wrapping_mul(16).wrapping_add(d);
                n += 1;
            }
            (val, n)
        }
        b'\\' => (b'\\', 1),
        b'"' => (b'"', 1),
        b'\'' => (b'\'', 1),
        b'a' => (7, 1),
        b'b' => (8, 1),
        b'f' => (12, 1),
        b'v' => (11, 1),
        other => (other, 1),
    }
}

// Preprocessor constant expression evaluator (over lexed CTok)
fn eval_pp_ternary(toks: &[CTok], pos: &mut usize) -> i64 {
    let cond = eval_pp_or(toks, pos);
    if matches!(toks.get(*pos), Some(CTok::Sym("?"))) {
        *pos += 1;
        let t = eval_pp_ternary(toks, pos);
        if matches!(toks.get(*pos), Some(CTok::Sym(":"))) {
            *pos += 1;
        }
        let f = eval_pp_ternary(toks, pos);
        if cond != 0 { t } else { f }
    } else {
        cond
    }
}

fn eval_pp_or(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_and(toks, pos);
    while matches!(toks.get(*pos), Some(CTok::Sym("||"))) {
        *pos += 1;
        let rhs = eval_pp_and(toks, pos);
        lhs = if lhs != 0 || rhs != 0 { 1 } else { 0 };
    }
    lhs
}

fn eval_pp_and(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_bitor(toks, pos);
    while matches!(toks.get(*pos), Some(CTok::Sym("&&"))) {
        *pos += 1;
        let rhs = eval_pp_bitor(toks, pos);
        lhs = if lhs != 0 && rhs != 0 { 1 } else { 0 };
    }
    lhs
}

fn eval_pp_bitor(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_bitxor(toks, pos);
    while matches!(toks.get(*pos), Some(CTok::Sym("|"))) {
        *pos += 1;
        lhs |= eval_pp_bitxor(toks, pos);
    }
    lhs
}

fn eval_pp_bitxor(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_bitand(toks, pos);
    while matches!(toks.get(*pos), Some(CTok::Sym("^"))) {
        *pos += 1;
        lhs ^= eval_pp_bitand(toks, pos);
    }
    lhs
}

fn eval_pp_bitand(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_eq(toks, pos);
    while matches!(toks.get(*pos), Some(CTok::Sym("&"))) {
        *pos += 1;
        lhs &= eval_pp_eq(toks, pos);
    }
    lhs
}

fn eval_pp_eq(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_rel(toks, pos);
    loop {
        match toks.get(*pos) {
            Some(CTok::Sym("==")) => {
                *pos += 1;
                lhs = if lhs == eval_pp_rel(toks, pos) { 1 } else { 0 };
            }
            Some(CTok::Sym("!=")) => {
                *pos += 1;
                lhs = if lhs != eval_pp_rel(toks, pos) { 1 } else { 0 };
            }
            _ => break,
        }
    }
    lhs
}

fn eval_pp_rel(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_shift(toks, pos);
    loop {
        match toks.get(*pos) {
            Some(CTok::Sym("<")) => {
                *pos += 1;
                lhs = if lhs < eval_pp_shift(toks, pos) { 1 } else { 0 };
            }
            Some(CTok::Sym("<=")) => {
                *pos += 1;
                lhs = if lhs <= eval_pp_shift(toks, pos) { 1 } else { 0 };
            }
            Some(CTok::Sym(">")) => {
                *pos += 1;
                lhs = if lhs > eval_pp_shift(toks, pos) { 1 } else { 0 };
            }
            Some(CTok::Sym(">=")) => {
                *pos += 1;
                lhs = if lhs >= eval_pp_shift(toks, pos) { 1 } else { 0 };
            }
            _ => break,
        }
    }
    lhs
}

fn eval_pp_shift(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_add(toks, pos);
    loop {
        match toks.get(*pos) {
            Some(CTok::Sym("<<")) => {
                *pos += 1;
                lhs = lhs.wrapping_shl(eval_pp_add(toks, pos) as u32);
            }
            Some(CTok::Sym(">>")) => {
                *pos += 1;
                lhs = lhs.wrapping_shr(eval_pp_add(toks, pos) as u32);
            }
            _ => break,
        }
    }
    lhs
}

fn eval_pp_add(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_mul(toks, pos);
    loop {
        match toks.get(*pos) {
            Some(CTok::Sym("+")) => {
                *pos += 1;
                lhs = lhs.wrapping_add(eval_pp_mul(toks, pos));
            }
            Some(CTok::Sym("-")) => {
                *pos += 1;
                lhs = lhs.wrapping_sub(eval_pp_mul(toks, pos));
            }
            _ => break,
        }
    }
    lhs
}

fn eval_pp_mul(toks: &[CTok], pos: &mut usize) -> i64 {
    let mut lhs = eval_pp_unary(toks, pos);
    loop {
        match toks.get(*pos) {
            Some(CTok::Sym("*")) => {
                *pos += 1;
                lhs = lhs.wrapping_mul(eval_pp_unary(toks, pos));
            }
            Some(CTok::Sym("/")) => {
                *pos += 1;
                let r = eval_pp_unary(toks, pos);
                lhs = if r != 0 { lhs.wrapping_div(r) } else { 0 };
            }
            Some(CTok::Sym("%")) => {
                *pos += 1;
                let r = eval_pp_unary(toks, pos);
                lhs = if r != 0 { lhs.wrapping_rem(r) } else { 0 };
            }
            _ => break,
        }
    }
    lhs
}

fn eval_pp_unary(toks: &[CTok], pos: &mut usize) -> i64 {
    match toks.get(*pos) {
        Some(CTok::Sym("!")) => {
            *pos += 1;
            if eval_pp_unary(toks, pos) == 0 { 1 } else { 0 }
        }
        Some(CTok::Sym("~")) => {
            *pos += 1;
            !eval_pp_unary(toks, pos)
        }
        Some(CTok::Sym("-")) => {
            *pos += 1;
            eval_pp_unary(toks, pos).wrapping_neg()
        }
        Some(CTok::Sym("+")) => {
            *pos += 1;
            eval_pp_unary(toks, pos)
        }
        Some(CTok::Sym("(")) => {
            *pos += 1;
            let v = eval_pp_ternary(toks, pos);
            if matches!(toks.get(*pos), Some(CTok::Sym(")"))) {
                *pos += 1;
            }
            v
        }
        Some(CTok::IntLit(v, _, _)) => {
            let v = *v as i64;
            *pos += 1;
            v
        }
        Some(CTok::Ident(_)) => {
            *pos += 1;
            0
        }
        _ => 0,
    }
}

// ============================================================================
// Stage 3: C99 Type System & AST
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum CType {
    Void,
    Bool,
    Int { bits: u8, signed: bool },
    Float,
    Double,
    Ptr(Box<CType>),
    Array(Box<CType>, usize),
    Struct(String),
    Union(String),
    FuncPtr {
        ret: Box<CType>,
        params: Vec<CType>,
        variadic: bool,
    },
}

impl CType {
    fn is_integer(&self) -> bool {
        matches!(self, CType::Bool | CType::Int { .. })
    }
    fn is_float(&self) -> bool {
        matches!(self, CType::Float | CType::Double)
    }
    fn is_ptr(&self) -> bool {
        matches!(self, CType::Ptr(_) | CType::FuncPtr { .. })
    }
    fn is_signed(&self) -> bool {
        matches!(self, CType::Int { signed: true, .. })
    }
    fn int_bits(&self) -> u8 {
        match self {
            CType::Bool => 1,
            CType::Int { bits, .. } => *bits,
            _ => 32,
        }
    }
}

#[derive(Debug, Clone)]
struct CBitfieldInfo {
    storage_idx: usize,
    storage_ty: CType,
    bit_offset: u8,
    bit_width: u8,
    logical_ty: CType,
}

#[derive(Debug, Clone)]
struct CStructDef {
    _name: String,
    fields: Vec<(String, CType, usize)>, // (field_name, type, byte_offset)
    bitfields: HashMap<String, CBitfieldInfo>,
    size: usize,
    align: usize,
}

#[derive(Debug, Clone)]
struct CUnionDef {
    _name: String,
    fields: Vec<(String, CType)>,
    size: usize,
    align: usize,
}

#[derive(Debug, Clone)]
struct CTranslationUnit {
    items: Vec<CTopItem>,
}

#[derive(Debug, Clone)]
enum CTopItem {
    FuncDecl {
        name: String,
        ret: CType,
        params: Vec<(String, CType)>,
        variadic: bool,
        is_static: bool,
        body: Option<Vec<CStmt>>,
    },
    GlobalVar {
        name: String,
        ty: CType,
        is_static: bool,
        is_extern: bool,
        init: Option<CInit>,
    },
}

#[derive(Debug, Clone)]
enum CInit {
    Expr(CExpr),
    List(Vec<(Option<String>, CInit)>),
}

#[derive(Debug, Clone)]
enum CStmt {
    Block(Vec<CStmt>),
    VarDecl {
        name: String,
        ty: CType,
        is_static: bool,
        init: Option<CInit>,
        vla_len: Option<CExpr>,
    },
    Expr(CExpr),
    Return(Option<CExpr>),
    If {
        cond: CExpr,
        then_s: Box<CStmt>,
        else_s: Option<Box<CStmt>>,
    },
    While {
        cond: CExpr,
        body: Box<CStmt>,
    },
    DoWhile {
        body: Box<CStmt>,
        cond: CExpr,
    },
    For {
        init: Option<Box<CStmt>>,
        cond: Option<CExpr>,
        inc: Option<CExpr>,
        body: Box<CStmt>,
    },
    Switch {
        expr: CExpr,
        body: Vec<CStmt>,
    },
    Case(i64),
    Default,
    Break,
    Continue,
    Label(String, Box<CStmt>),
    Goto(String),
    Empty,
}

#[derive(Debug, Clone)]
enum CExpr {
    Int(u64, bool, u8),
    Float(f64, bool),
    Str(Vec<u8>),
    Var(String),
    Unary(&'static str, Box<CExpr>),
    PostfixIncDec(&'static str, Box<CExpr>),
    Binary(&'static str, Box<CExpr>, Box<CExpr>),
    Assign(&'static str, Box<CExpr>, Box<CExpr>),
    Ternary(Box<CExpr>, Box<CExpr>, Box<CExpr>),
    Cast(CType, Box<CExpr>),
    Call(Box<CExpr>, Vec<CExpr>),
    Index(Box<CExpr>, Box<CExpr>),
    Member(Box<CExpr>, String, bool), // (base, field, is_arrow)
    SizeofType(CType),
    SizeofExpr(Box<CExpr>),
    CompoundLit(CType, Box<CInit>),
}

// ============================================================================
// Stage 3b: Typedef-Aware C99 Recursive-Descent Parser
// ============================================================================

struct CParser {
    toks: Vec<CTok>,
    pos: usize,
    ptr_size: usize,
    typedefs: HashMap<String, CType>,
    structs: BTreeMap<String, CStructDef>,
    unions: BTreeMap<String, CUnionDef>,
    enum_consts: HashMap<String, i64>,
    anon_seq: usize,
}

impl CParser {
    fn new(toks: Vec<CTok>, target_triple: &str) -> Self {
        let t = target_triple.to_ascii_lowercase();
        let ptr_size = if t.contains("thumb") || (t.contains("arm") && !t.contains("64")) || t.contains("i686") {
            4
        } else {
            8
        };
        let mut typedefs = HashMap::new();
        typedefs.insert("size_t".to_string(), CType::Int { bits: (ptr_size * 8) as u8, signed: false });
        typedefs.insert("ssize_t".to_string(), CType::Int { bits: (ptr_size * 8) as u8, signed: true });
        typedefs.insert("ptrdiff_t".to_string(), CType::Int { bits: (ptr_size * 8) as u8, signed: true });
        typedefs.insert("int8_t".to_string(), CType::Int { bits: 8, signed: true });
        typedefs.insert("uint8_t".to_string(), CType::Int { bits: 8, signed: false });
        typedefs.insert("int16_t".to_string(), CType::Int { bits: 16, signed: true });
        typedefs.insert("uint16_t".to_string(), CType::Int { bits: 16, signed: false });
        typedefs.insert("int32_t".to_string(), CType::Int { bits: 32, signed: true });
        typedefs.insert("uint32_t".to_string(), CType::Int { bits: 32, signed: false });
        typedefs.insert("int64_t".to_string(), CType::Int { bits: 64, signed: true });
        typedefs.insert("uint64_t".to_string(), CType::Int { bits: 64, signed: false });
        typedefs.insert("intptr_t".to_string(), CType::Int { bits: (ptr_size * 8) as u8, signed: true });
        typedefs.insert("uintptr_t".to_string(), CType::Int { bits: (ptr_size * 8) as u8, signed: false });
        typedefs.insert("bool".to_string(), CType::Bool);

        Self {
            toks,
            pos: 0,
            ptr_size,
            typedefs,
            structs: BTreeMap::new(),
            unions: BTreeMap::new(),
            enum_consts: HashMap::new(),
            anon_seq: 0,
        }
    }

    fn peek(&self) -> &CTok {
        self.toks.get(self.pos).unwrap_or(&CTok::Eof)
    }

    fn peek_at(&self, off: usize) -> &CTok {
        self.toks.get(self.pos + off).unwrap_or(&CTok::Eof)
    }

    fn bump(&mut self) -> CTok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat_sym(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), CTok::Sym(s) if *s == sym) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, sym: &str) -> Result<(), String> {
        if self.eat_sym(sym) {
            Ok(())
        } else {
            Err(format!("c_frontend: expected '{}', got {:?}", sym, self.peek()))
        }
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if matches!(self.peek(), CTok::Ident(s) if s == kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn is_type_start(&self) -> bool {
        match self.peek() {
            CTok::Ident(s) => {
                matches!(
                    s.as_str(),
                    "void"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "_Bool"
                        | "struct"
                        | "union"
                        | "enum"
                        | "const"
                        | "volatile"
                        | "static"
                        | "extern"
                        | "inline"
                        | "__inline"
                        | "__inline__"
                        | "register"
                        | "auto"
                        | "typedef"
                ) || self.typedefs.contains_key(s)
            }
            _ => false,
        }
    }

    fn sizeof_type(&self, ty: &CType) -> usize {
        match ty {
            CType::Void => 1,
            CType::Bool => 1,
            CType::Int { bits, .. } => (*bits as usize) / 8,
            CType::Float => 4,
            CType::Double => 8,
            CType::Ptr(_) | CType::FuncPtr { .. } => self.ptr_size,
            CType::Array(elem, n) => self.sizeof_type(elem) * (*n),
            CType::Struct(nm) => self.structs.get(nm).map(|s| s.size).unwrap_or(self.ptr_size),
            CType::Union(nm) => self.unions.get(nm).map(|u| u.size).unwrap_or(self.ptr_size),
        }
    }

    fn alignof_type(&self, ty: &CType) -> usize {
        match ty {
            CType::Void | CType::Bool => 1,
            CType::Int { bits, .. } => ((*bits as usize) / 8).max(1),
            CType::Float => 4,
            CType::Double => 8,
            CType::Ptr(_) | CType::FuncPtr { .. } => self.ptr_size,
            CType::Array(elem, _) => self.alignof_type(elem),
            CType::Struct(nm) => self.structs.get(nm).map(|s| s.align).unwrap_or(1),
            CType::Union(nm) => self.unions.get(nm).map(|u| u.align).unwrap_or(1),
        }
    }

    fn eval_const_expr(&self, expr: &CExpr) -> Option<i64> {
        match expr {
            CExpr::Int(v, _, _) => Some(*v as i64),
            CExpr::Var(nm) => self.enum_consts.get(nm).copied(),
            CExpr::SizeofType(t) => Some(self.sizeof_type(t) as i64),
            CExpr::Unary("-", e) => Some(self.eval_const_expr(e)?.wrapping_neg()),
            CExpr::Unary("+", e) => self.eval_const_expr(e),
            CExpr::Unary("~", e) => Some(!self.eval_const_expr(e)?),
            CExpr::Unary("!", e) => Some(if self.eval_const_expr(e)? == 0 { 1 } else { 0 }),
            CExpr::Cast(_, e) => self.eval_const_expr(e),
            CExpr::Binary(op, a, b) => {
                let l = self.eval_const_expr(a)?;
                let r = self.eval_const_expr(b)?;
                Some(match *op {
                    "+" => l.wrapping_add(r),
                    "-" => l.wrapping_sub(r),
                    "*" => l.wrapping_mul(r),
                    "/" => if r != 0 { l.wrapping_div(r) } else { 0 },
                    "%" => if r != 0 { l.wrapping_rem(r) } else { 0 },
                    "<<" => l.wrapping_shl(r as u32),
                    ">>" => l.wrapping_shr(r as u32),
                    "&" => l & r,
                    "|" => l | r,
                    "^" => l ^ r,
                    "==" => (l == r) as i64,
                    "!=" => (l != r) as i64,
                    "<" => (l < r) as i64,
                    "<=" => (l <= r) as i64,
                    ">" => (l > r) as i64,
                    ">=" => (l >= r) as i64,
                    "&&" => (l != 0 && r != 0) as i64,
                    "||" => (l != 0 || r != 0) as i64,
                    _ => return None,
                })
            }
            CExpr::Ternary(c, t, f) => {
                if self.eval_const_expr(c)? != 0 {
                    self.eval_const_expr(t)
                } else {
                    self.eval_const_expr(f)
                }
            }
            _ => None,
        }
    }

    fn parse_translation_unit(&mut self) -> Result<CTranslationUnit, String> {
        let mut items = Vec::new();
        while !matches!(self.peek(), CTok::Eof) {
            if self.eat_sym(";") {
                continue;
            }
            let (is_typedef, is_static, is_extern, base_ty) = self.parse_decl_specifiers()?;
            if self.eat_sym(";") {
                continue;
            }
            loop {
                let (name, full_ty, fn_params) = self.parse_declarator(base_ty.clone())?;
                if is_typedef {
                    if !name.is_empty() {
                        self.typedefs.insert(name, full_ty);
                    }
                    if self.eat_sym(",") {
                        continue;
                    }
                    self.expect_sym(";")?;
                    break;
                }
                if let Some((params, variadic, ret_ty)) = fn_params {
                    if matches!(self.peek(), CTok::Sym("{")) {
                        let body = self.parse_block_stmts()?;
                        items.push(CTopItem::FuncDecl {
                            name,
                            ret: ret_ty,
                            params,
                            variadic,
                            is_static,
                            body: Some(body),
                        });
                        break;
                    } else {
                        items.push(CTopItem::FuncDecl {
                            name,
                            ret: ret_ty,
                            params,
                            variadic,
                            is_static,
                            body: None,
                        });
                        if self.eat_sym(",") {
                            continue;
                        }
                        self.expect_sym(";")?;
                        break;
                    }
                } else {
                    let init = if self.eat_sym("=") {
                        Some(self.parse_initializer()?)
                    } else {
                        None
                    };
                    // Infer array size from initializer if `arr[] = {...}` or `s[] = "..."`
                    let mut final_ty = full_ty;
                    if let CType::Array(elem, 0) = &final_ty {
                        let inferred = match &init {
                            Some(CInit::List(v)) => v.len(),
                            Some(CInit::Expr(CExpr::Str(b))) => b.len() + 1,
                            _ => 1,
                        };
                        final_ty = CType::Array(elem.clone(), inferred);
                    }
                    items.push(CTopItem::GlobalVar {
                        name,
                        ty: final_ty,
                        is_static,
                        is_extern,
                        init,
                    });
                    if self.eat_sym(",") {
                        continue;
                    }
                    self.expect_sym(";")?;
                    break;
                }
            }
        }
        Ok(CTranslationUnit { items })
    }

    fn parse_decl_specifiers(&mut self) -> Result<(bool, bool, bool, CType), String> {
        let mut is_typedef = false;
        let mut is_static = false;
        let mut is_extern = false;
        let mut seen_signed = false;
        let mut seen_unsigned = false;
        let mut long_cnt = 0u8;
        let mut seen_short = false;
        let mut seen_char = false;
        let mut seen_int = false;
        let mut seen_float = false;
        let mut seen_double = false;
        let mut seen_void = false;
        let mut seen_bool = false;
        let mut explicit_ty: Option<CType> = None;

        loop {
            match self.peek().clone() {
                CTok::Ident(ref s) => match s.as_str() {
                    "typedef" => {
                        is_typedef = true;
                        self.bump();
                    }
                    "static" => {
                        is_static = true;
                        self.bump();
                    }
                    "extern" => {
                        is_extern = true;
                        self.bump();
                    }
                    "const" | "volatile" | "inline" | "__inline" | "__inline__" | "register" | "auto" => {
                        self.bump();
                    }
                    "signed" => {
                        seen_signed = true;
                        self.bump();
                    }
                    "unsigned" => {
                        seen_unsigned = true;
                        self.bump();
                    }
                    "short" => {
                        seen_short = true;
                        self.bump();
                    }
                    "long" => {
                        long_cnt += 1;
                        self.bump();
                    }
                    "char" => {
                        seen_char = true;
                        self.bump();
                    }
                    "int" => {
                        seen_int = true;
                        self.bump();
                    }
                    "float" => {
                        seen_float = true;
                        self.bump();
                    }
                    "double" => {
                        seen_double = true;
                        self.bump();
                    }
                    "void" => {
                        seen_void = true;
                        self.bump();
                    }
                    "_Bool" => {
                        seen_bool = true;
                        self.bump();
                    }
                    "struct" => {
                        self.bump();
                        explicit_ty = Some(self.parse_struct_or_union_spec(false)?);
                    }
                    "union" => {
                        self.bump();
                        explicit_ty = Some(self.parse_struct_or_union_spec(true)?);
                    }
                    "enum" => {
                        self.bump();
                        explicit_ty = Some(self.parse_enum_spec()?);
                    }
                    other => {
                        if explicit_ty.is_none()
                            && !seen_char
                            && !seen_short
                            && !seen_int
                            && !seen_float
                            && !seen_double
                            && !seen_void
                            && !seen_bool
                            && long_cnt == 0
                            && !seen_signed
                            && !seen_unsigned
                        {
                            if let Some(td) = self.typedefs.get(other).cloned() {
                                self.bump();
                                explicit_ty = Some(td);
                                continue;
                            }
                        }
                        break;
                    }
                },
                _ => break,
            }
        }

        let ty = if let Some(t) = explicit_ty {
            t
        } else if seen_void {
            CType::Void
        } else if seen_bool {
            CType::Bool
        } else if seen_float {
            CType::Float
        } else if seen_double {
            CType::Double
        } else if seen_char {
            CType::Int {
                bits: 8,
                signed: !seen_unsigned,
            }
        } else if seen_short {
            CType::Int {
                bits: 16,
                signed: !seen_unsigned,
            }
        } else if long_cnt >= 2 {
            CType::Int {
                bits: 64,
                signed: !seen_unsigned,
            }
        } else if long_cnt == 1 {
            CType::Int {
                bits: (self.ptr_size * 8) as u8,
                signed: !seen_unsigned,
            }
        } else if seen_int || seen_signed || seen_unsigned {
            CType::Int {
                bits: 32,
                signed: !seen_unsigned,
            }
        } else {
            return Err(format!("c_frontend: expected type specifier, got {:?}", self.peek()));
        };

        Ok((is_typedef, is_static, is_extern, ty))
    }

    fn parse_struct_or_union_spec(&mut self, is_union: bool) -> Result<CType, String> {
        let tag = if let CTok::Ident(nm) = self.peek().clone() {
            self.bump();
            nm
        } else {
            self.anon_seq += 1;
            format!("__anon_{}_{}", if is_union { "union" } else { "struct" }, self.anon_seq)
        };

        if self.eat_sym("{") {
            let mut raw_fields: Vec<(String, CType, Option<u8>)> = Vec::new();
            while !self.eat_sym("}") {
                if matches!(self.peek(), CTok::Eof) {
                    return Err("c_frontend: unterminated struct/union body".to_string());
                }
                if self.eat_sym(";") {
                    continue;
                }
                let (_, _, _, fbase) = self.parse_decl_specifiers()?;
                // Handle anonymous inner `struct { ... };` or `union { ... };`
                if self.eat_sym(";") {
                    raw_fields.push((String::new(), fbase, None));
                    continue;
                }
                loop {
                    let (fname, fty, _) = self.parse_declarator(fbase.clone())?;
                    let bf_width = if self.eat_sym(":") {
                        let w_expr = self.parse_expr_no_comma()?;
                        Some(self.eval_const_expr(&w_expr).unwrap_or(1).clamp(0, 64) as u8)
                    } else {
                        None
                    };
                    raw_fields.push((fname, fty, bf_width));
                    if self.eat_sym(",") {
                        continue;
                    }
                    self.expect_sym(";")?;
                    break;
                }
            }

            if is_union {
                let mut max_size = 1usize;
                let mut max_align = 1usize;
                let mut ufields = Vec::new();
                for (fname, fty, _) in raw_fields {
                    max_size = max_size.max(self.sizeof_type(&fty));
                    max_align = max_align.max(self.alignof_type(&fty));
                    ufields.push((fname, fty));
                }
                let aligned_size = (max_size + max_align - 1) & !(max_align - 1);
                self.unions.insert(
                    tag.clone(),
                    CUnionDef {
                        _name: tag.clone(),
                        fields: ufields,
                        size: aligned_size.max(1),
                        align: max_align,
                    },
                );
            } else {
                let mut laid_out: Vec<(String, CType, usize)> = Vec::new();
                let mut bitfields: HashMap<String, CBitfieldInfo> = HashMap::new();
                let mut offset = 0usize;
                let mut max_align = 1usize;
                // Track active bitfield storage unit: (storage_field_idx, storage_ty, used_bits, unit_bits)
                let mut active_bf: Option<(usize, CType, u8, u8)> = None;

                for (fname, fty, bf_opt) in raw_fields {
                    if let Some(width) = bf_opt {
                        let unit_bits = if fty.int_bits() > 32 { 64u8 } else { 32u8 };
                        let unit_ty = CType::Int {
                            bits: unit_bits,
                            signed: false,
                        };
                        if width == 0 {
                            active_bf = None;
                            continue;
                        }
                        let (s_idx, s_ty, bit_off) = match active_bf.clone() {
                            Some((idx, sty, used, cap)) if used + width <= cap && cap == unit_bits => {
                                active_bf = Some((idx, sty.clone(), used + width, cap));
                                (idx, sty, used)
                            }
                            _ => {
                                let fa = (unit_bits as usize) / 8;
                                max_align = max_align.max(fa);
                                offset = (offset + fa - 1) & !(fa - 1);
                                let idx = laid_out.len();
                                laid_out.push((format!("__bf_unit_{}", idx), unit_ty.clone(), offset));
                                offset += fa;
                                active_bf = Some((idx, unit_ty.clone(), width, unit_bits));
                                (idx, unit_ty, 0)
                            }
                        };
                        if !fname.is_empty() {
                            bitfields.insert(
                                fname,
                                CBitfieldInfo {
                                    storage_idx: s_idx,
                                    storage_ty: s_ty,
                                    bit_offset: bit_off,
                                    bit_width: width,
                                    logical_ty: fty,
                                },
                            );
                        }
                    } else {
                        active_bf = None;
                        let fa = self.alignof_type(&fty).max(1);
                        let fs = self.sizeof_type(&fty);
                        max_align = max_align.max(fa);
                        offset = (offset + fa - 1) & !(fa - 1);
                        laid_out.push((fname, fty, offset));
                        offset += fs;
                    }
                }
                let total_size = ((offset + max_align - 1) & !(max_align - 1)).max(1);
                self.structs.insert(
                    tag.clone(),
                    CStructDef {
                        _name: tag.clone(),
                        fields: laid_out,
                        bitfields,
                        size: total_size,
                        align: max_align,
                    },
                );
            }
        }
        Ok(if is_union {
            CType::Union(tag)
        } else {
            CType::Struct(tag)
        })
    }

    fn parse_enum_spec(&mut self) -> Result<CType, String> {
        if let CTok::Ident(_) = self.peek() {
            if matches!(self.peek_at(1), CTok::Sym("{")) || !matches!(self.peek_at(1), CTok::Sym(";")) {
                // Consume optional enum tag
                if matches!(self.peek_at(1), CTok::Sym("{")) {
                    self.bump();
                } else {
                    // `enum Foo x;` -> consume `Foo`
                    self.bump();
                    return Ok(CType::Int { bits: 32, signed: true });
                }
            } else {
                self.bump();
                return Ok(CType::Int { bits: 32, signed: true });
            }
        }
        if self.eat_sym("{") {
            let mut next_val = 0i64;
            while !self.eat_sym("}") {
                if matches!(self.peek(), CTok::Eof) {
                    return Err("c_frontend: unterminated enum".to_string());
                }
                let nm = match self.bump() {
                    CTok::Ident(s) => s,
                    other => return Err(format!("c_frontend: expected enum enumerator, got {:?}", other)),
                };
                if self.eat_sym("=") {
                    let e = self.parse_expr_no_comma()?;
                    if let Some(v) = self.eval_const_expr(&e) {
                        next_val = v;
                    }
                }
                self.enum_consts.insert(nm, next_val);
                next_val += 1;
                let _ = self.eat_sym(",");
            }
        }
        Ok(CType::Int { bits: 32, signed: true })
    }

    /// Parses a C declarator (including spiral declarators like `int (*p)[4]`,
    /// `int (*ops[4])(int, int)`, `int (*get_op(int))(int, int)`).
    fn parse_declarator(
        &mut self,
        ty: CType,
    ) -> Result<(String, CType, Option<(Vec<(String, CType)>, bool, CType)>), String> {
        let (name, full_ty, fn_info, _) = self.parse_declarator_ext(ty)?;
        Ok((name, full_ty, fn_info))
    }

    fn parse_declarator_ext(
        &mut self,
        mut ty: CType,
    ) -> Result<(String, CType, Option<(Vec<(String, CType)>, bool, CType)>, Option<CExpr>), String> {
        while self.eat_sym("*") {
            while self.eat_kw("const") || self.eat_kw("volatile") {}
            ty = CType::Ptr(Box::new(ty));
        }

        // Grouped inner declarator `(...)`: e.g. `int (*p)[4]`, `int (*ops[4])(int)`, `int (*get_op(int))(int)`
        if matches!(self.peek(), CTok::Sym("("))
            && !matches!(self.peek_at(1), CTok::Sym(")"))
            && !self.is_type_token(self.peek_at(1))
        {
            self.bump(); // consume '('
            // Capture the inner declarator tokens up to the matching ')'
            let inner_start = self.pos;
            let mut depth = 1usize;
            while !matches!(self.peek(), CTok::Eof) {
                if matches!(self.peek(), CTok::Sym("(")) {
                    depth += 1;
                } else if matches!(self.peek(), CTok::Sym(")")) {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                self.bump();
            }
            let inner_end = self.pos;
            self.expect_sym(")")?;

            // Parse outer suffixes (`[N]` or `(params...)`) and apply them to `ty` FIRST
            let mut outer_is_fn = false;
            let (outer_ty, _) = self.parse_declarator_suffixes(ty, &mut outer_is_fn)?;

            // Save position after outer suffixes, then parse the inner declarator wrapping `outer_ty`
            let after_outer = self.pos;
            let mut sub = self.toks[inner_start..inner_end].to_vec();
            sub.push(CTok::Eof);
            let saved_toks = std::mem::replace(&mut self.toks, sub);
            self.pos = 0;
            // If outer was a function `(params...)`, a single `*` inside `(*name)` produces a FuncPtr directly
            let inner_res = if outer_is_fn && matches!(self.peek(), CTok::Sym("*")) {
                self.bump();
                while self.eat_kw("const") || self.eat_kw("volatile") {}
                self.parse_declarator_ext(outer_ty)
            } else {
                self.parse_declarator_ext(outer_ty)
            };
            self.toks = saved_toks;
            self.pos = after_outer;
            return inner_res;
        }

        let name = match self.peek().clone() {
            CTok::Ident(s) => {
                self.bump();
                s
            }
            _ => String::new(),
        };

        // Direct function parameter list
        if self.eat_sym("(") {
            let (params, variadic) = self.parse_param_list()?;
            let fn_ty = CType::FuncPtr {
                ret: Box::new(ty.clone()),
                params: params.iter().map(|(_, t)| t.clone()).collect(),
                variadic,
            };
            return Ok((name, fn_ty, Some((params, variadic, ty)), None));
        }

        let mut is_fn = false;
        let (final_ty, vla_len) = self.parse_declarator_suffixes(ty, &mut is_fn)?;
        Ok((name, final_ty, None, vla_len))
    }

    fn is_type_token(&self, tok: &CTok) -> bool {
        match tok {
            CTok::Ident(s) => {
                matches!(
                    s.as_str(),
                    "void"
                        | "char"
                        | "short"
                        | "int"
                        | "long"
                        | "float"
                        | "double"
                        | "signed"
                        | "unsigned"
                        | "_Bool"
                        | "struct"
                        | "union"
                        | "enum"
                        | "const"
                        | "volatile"
                ) || self.typedefs.contains_key(s)
            }
            _ => false,
        }
    }

    fn parse_declarator_suffixes(
        &mut self,
        mut ty: CType,
        is_fn: &mut bool,
    ) -> Result<(CType, Option<CExpr>), String> {
        if self.eat_sym("(") {
            let (params, variadic) = self.parse_param_list()?;
            *is_fn = true;
            return Ok((
                CType::FuncPtr {
                    ret: Box::new(ty),
                    params: params.into_iter().map(|(_, t)| t).collect(),
                    variadic,
                },
                None,
            ));
        }

        let mut dims: Vec<Result<usize, CExpr>> = Vec::new();
        while self.eat_sym("[") {
            if self.eat_sym("]") {
                dims.push(Ok(0));
            } else {
                let sz_expr = self.parse_expr_no_comma()?;
                self.expect_sym("]")?;
                if let Some(n) = self.eval_const_expr(&sz_expr) {
                    dims.push(Ok(n.max(0) as usize));
                } else {
                    dims.push(Err(sz_expr));
                }
            }
        }
        let mut vla_out: Option<CExpr> = None;
        for d in dims.into_iter().rev() {
            match d {
                Ok(n) => ty = CType::Array(Box::new(ty), n),
                Err(e) => {
                    vla_out = Some(e);
                    ty = CType::Ptr(Box::new(ty));
                }
            }
        }
        Ok((ty, vla_out))
    }

    fn parse_param_list(&mut self) -> Result<(Vec<(String, CType)>, bool), String> {
        let mut params = Vec::new();
        let mut variadic = false;
        if self.eat_sym(")") {
            return Ok((params, false));
        }
        // `(void)` -> 0 params
        if matches!(self.peek(), CTok::Ident(s) if s == "void")
            && matches!(self.peek_at(1), CTok::Sym(")"))
        {
            self.bump();
            self.bump();
            return Ok((params, false));
        }
        loop {
            if self.eat_sym("...") {
                variadic = true;
                self.expect_sym(")")?;
                break;
            }
            let (_, _, _, pbase) = self.parse_decl_specifiers()?;
            let (pname, mut pty, _) = self.parse_declarator(pbase)?;
            // In C function parameters, `T arr[N]` decays to `T*`
            if let CType::Array(elem, _) = pty {
                pty = CType::Ptr(elem);
            }
            params.push((pname, pty));
            if self.eat_sym(",") {
                continue;
            }
            self.expect_sym(")")?;
            break;
        }
        Ok((params, variadic))
    }

    fn parse_type_name(&mut self) -> Result<CType, String> {
        let (_, _, _, base) = self.parse_decl_specifiers()?;
        let (_, full, _) = self.parse_declarator(base)?;
        Ok(full)
    }

    fn parse_initializer(&mut self) -> Result<CInit, String> {
        if self.eat_sym("{") {
            let mut elems = Vec::new();
            while !self.eat_sym("}") {
                if matches!(self.peek(), CTok::Eof) {
                    return Err("c_frontend: unterminated initializer list".to_string());
                }
                let mut field = None;
                if self.eat_sym(".") {
                    if let CTok::Ident(nm) = self.bump() {
                        field = Some(nm);
                    }
                    self.expect_sym("=")?;
                }
                let val = self.parse_initializer()?;
                elems.push((field, val));
                if !self.eat_sym(",") {
                    self.expect_sym("}")?;
                    break;
                }
            }
            Ok(CInit::List(elems))
        } else {
            Ok(CInit::Expr(self.parse_expr_no_comma()?))
        }
    }

    fn parse_block_stmts(&mut self) -> Result<Vec<CStmt>, String> {
        self.expect_sym("{")?;
        let mut stmts = Vec::new();
        while !self.eat_sym("}") {
            if matches!(self.peek(), CTok::Eof) {
                return Err("c_frontend: unterminated '{' block".to_string());
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> Result<CStmt, String> {
        if self.eat_sym(";") {
            return Ok(CStmt::Empty);
        }
        if matches!(self.peek(), CTok::Sym("{")) {
            return Ok(CStmt::Block(self.parse_block_stmts()?));
        }
        if self.eat_kw("return") {
            if self.eat_sym(";") {
                return Ok(CStmt::Return(None));
            }
            let e = self.parse_expr()?;
            self.expect_sym(";")?;
            return Ok(CStmt::Return(Some(e)));
        }
        if self.eat_kw("if") {
            self.expect_sym("(")?;
            let cond = self.parse_expr()?;
            self.expect_sym(")")?;
            let then_s = Box::new(self.parse_stmt()?);
            let else_s = if self.eat_kw("else") {
                Some(Box::new(self.parse_stmt()?))
            } else {
                None
            };
            return Ok(CStmt::If { cond, then_s, else_s });
        }
        if self.eat_kw("while") {
            self.expect_sym("(")?;
            let cond = self.parse_expr()?;
            self.expect_sym(")")?;
            let body = Box::new(self.parse_stmt()?);
            return Ok(CStmt::While { cond, body });
        }
        if self.eat_kw("do") {
            let body = Box::new(self.parse_stmt()?);
            if !self.eat_kw("while") {
                return Err("c_frontend: expected 'while' after 'do' body".to_string());
            }
            self.expect_sym("(")?;
            let cond = self.parse_expr()?;
            self.expect_sym(")")?;
            self.expect_sym(";")?;
            return Ok(CStmt::DoWhile { body, cond });
        }
        if self.eat_kw("for") {
            self.expect_sym("(")?;
            let init = if self.eat_sym(";") {
                None
            } else if self.is_type_start() {
                Some(Box::new(self.parse_local_decl()?))
            } else {
                let e = self.parse_expr()?;
                self.expect_sym(";")?;
                Some(Box::new(CStmt::Expr(e)))
            };
            let cond = if self.eat_sym(";") {
                None
            } else {
                let e = self.parse_expr()?;
                self.expect_sym(";")?;
                Some(e)
            };
            let inc = if self.eat_sym(")") {
                None
            } else {
                let e = self.parse_expr()?;
                self.expect_sym(")")?;
                Some(e)
            };
            let body = Box::new(self.parse_stmt()?);
            return Ok(CStmt::For { init, cond, inc, body });
        }
        if self.eat_kw("switch") {
            self.expect_sym("(")?;
            let expr = self.parse_expr()?;
            self.expect_sym(")")?;
            let body = self.parse_block_stmts()?;
            return Ok(CStmt::Switch { expr, body });
        }
        if self.eat_kw("case") {
            let e = self.parse_expr_no_comma()?;
            let v = self.eval_const_expr(&e).unwrap_or(0);
            self.expect_sym(":")?;
            return Ok(CStmt::Case(v));
        }
        if self.eat_kw("default") {
            self.expect_sym(":")?;
            return Ok(CStmt::Default);
        }
        if self.eat_kw("break") {
            self.expect_sym(";")?;
            return Ok(CStmt::Break);
        }
        if self.eat_kw("continue") {
            self.expect_sym(";")?;
            return Ok(CStmt::Continue);
        }
        if self.eat_kw("goto") {
            let lbl = match self.bump() {
                CTok::Ident(s) => s,
                other => return Err(format!("c_frontend: expected goto label, got {:?}", other)),
            };
            self.expect_sym(";")?;
            return Ok(CStmt::Goto(lbl));
        }
        // Labelled statement `label: stmt`
        if let CTok::Ident(lbl) = self.peek().clone() {
            if matches!(self.peek_at(1), CTok::Sym(":")) {
                self.bump();
                self.bump();
                let inner = Box::new(self.parse_stmt()?);
                return Ok(CStmt::Label(lbl, inner));
            }
        }
        if self.is_type_start() {
            return self.parse_local_decl();
        }
        let e = self.parse_expr()?;
        self.expect_sym(";")?;
        Ok(CStmt::Expr(e))
    }

    fn parse_local_decl(&mut self) -> Result<CStmt, String> {
        let (is_typedef, is_static, _, base_ty) = self.parse_decl_specifiers()?;
        if self.eat_sym(";") {
            return Ok(CStmt::Empty);
        }
        let mut decls = Vec::new();
        loop {
            let (name, mut full_ty, _, vla_len) = self.parse_declarator_ext(base_ty.clone())?;
            if is_typedef {
                if !name.is_empty() {
                    self.typedefs.insert(name, full_ty);
                }
            } else {
                let init = if self.eat_sym("=") {
                    Some(self.parse_initializer()?)
                } else {
                    None
                };
                if let CType::Array(elem, 0) = &full_ty {
                    let inferred = match &init {
                        Some(CInit::List(v)) => v.len(),
                        Some(CInit::Expr(CExpr::Str(b))) => b.len() + 1,
                        _ => 1,
                    };
                    full_ty = CType::Array(elem.clone(), inferred);
                }
                decls.push(CStmt::VarDecl {
                    name,
                    ty: full_ty,
                    is_static,
                    init,
                    vla_len,
                });
            }
            if self.eat_sym(",") {
                continue;
            }
            self.expect_sym(";")?;
            break;
        }
        if decls.len() == 1 {
            Ok(decls.pop().unwrap())
        } else {
            Ok(CStmt::Block(decls))
        }
    }

    // Expressions
    fn parse_expr(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_expr_no_comma()?;
        while self.eat_sym(",") {
            let rhs = self.parse_expr_no_comma()?;
            lhs = CExpr::Binary(",", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_expr_no_comma(&mut self) -> Result<CExpr, String> {
        self.parse_assign()
    }

    fn parse_assign(&mut self) -> Result<CExpr, String> {
        let lhs = self.parse_ternary()?;
        let op = match self.peek() {
            CTok::Sym(s)
                if matches!(
                    *s,
                    "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "|=" | "^="
                ) =>
            {
                *s
            }
            _ => return Ok(lhs),
        };
        self.bump();
        let rhs = self.parse_assign()?;
        Ok(CExpr::Assign(op, Box::new(lhs), Box::new(rhs)))
    }

    fn parse_ternary(&mut self) -> Result<CExpr, String> {
        let cond = self.parse_log_or()?;
        if self.eat_sym("?") {
            let t = self.parse_expr()?;
            self.expect_sym(":")?;
            let f = self.parse_ternary()?;
            Ok(CExpr::Ternary(Box::new(cond), Box::new(t), Box::new(f)))
        } else {
            Ok(cond)
        }
    }

    fn parse_log_or(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_log_and()?;
        while self.eat_sym("||") {
            let rhs = self.parse_log_and()?;
            lhs = CExpr::Binary("||", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_log_and(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_bit_or()?;
        while self.eat_sym("&&") {
            let rhs = self.parse_bit_or()?;
            lhs = CExpr::Binary("&&", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bit_or(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_bit_xor()?;
        while self.eat_sym("|") {
            let rhs = self.parse_bit_xor()?;
            lhs = CExpr::Binary("|", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bit_xor(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_bit_and()?;
        while self.eat_sym("^") {
            let rhs = self.parse_bit_and()?;
            lhs = CExpr::Binary("^", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_bit_and(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_equality()?;
        while self.eat_sym("&") {
            let rhs = self.parse_equality()?;
            lhs = CExpr::Binary("&", Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek() {
                CTok::Sym("==") => "==",
                CTok::Sym("!=") => "!=",
                _ => break,
            };
            self.bump();
            let rhs = self.parse_relational()?;
            lhs = CExpr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                CTok::Sym("<") => "<",
                CTok::Sym("<=") => "<=",
                CTok::Sym(">") => ">",
                CTok::Sym(">=") => ">=",
                _ => break,
            };
            self.bump();
            let rhs = self.parse_shift()?;
            lhs = CExpr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                CTok::Sym("<<") => "<<",
                CTok::Sym(">>") => ">>",
                _ => break,
            };
            self.bump();
            let rhs = self.parse_additive()?;
            lhs = CExpr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                CTok::Sym("+") => "+",
                CTok::Sym("-") => "-",
                _ => break,
            };
            self.bump();
            let rhs = self.parse_multiplicative()?;
            lhs = CExpr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<CExpr, String> {
        let mut lhs = self.parse_cast()?;
        loop {
            let op = match self.peek() {
                CTok::Sym("*") => "*",
                CTok::Sym("/") => "/",
                CTok::Sym("%") => "%",
                _ => break,
            };
            self.bump();
            let rhs = self.parse_cast()?;
            lhs = CExpr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_cast(&mut self) -> Result<CExpr, String> {
        if matches!(self.peek(), CTok::Sym("(")) {
            let saved = self.pos;
            self.bump();
            if self.is_type_start() {
                if let Ok(mut ty) = self.parse_type_name() {
                    if self.eat_sym(")") {
                        // C99 Compound Literal: `(TypeName){ ... }`
                        if matches!(self.peek(), CTok::Sym("{")) {
                            let init = self.parse_initializer()?;
                            if let CType::Array(elem, 0) = &ty {
                                let n = match &init {
                                    CInit::List(v) => v.len(),
                                    CInit::Expr(CExpr::Str(b)) => b.len() + 1,
                                    _ => 1,
                                };
                                ty = CType::Array(elem.clone(), n);
                            }
                            return self.parse_postfix_on(CExpr::CompoundLit(ty, Box::new(init)));
                        }
                        let inner = self.parse_cast()?;
                        return Ok(CExpr::Cast(ty, Box::new(inner)));
                    }
                }
            }
            self.pos = saved;
        }
        self.parse_unary()
    }

    fn parse_unary(&mut self) -> Result<CExpr, String> {
        if self.eat_kw("sizeof") {
            if matches!(self.peek(), CTok::Sym("(")) {
                let saved = self.pos;
                self.bump();
                if self.is_type_start() {
                    if let Ok(ty) = self.parse_type_name() {
                        if self.eat_sym(")") {
                            return Ok(CExpr::SizeofType(ty));
                        }
                    }
                }
                self.pos = saved;
            }
            let e = self.parse_unary()?;
            return Ok(CExpr::SizeofExpr(Box::new(e)));
        }
        let op = match self.peek() {
            CTok::Sym(s) if matches!(*s, "+" | "-" | "~" | "!" | "*" | "&" | "++" | "--") => *s,
            _ => return self.parse_postfix(),
        };
        self.bump();
        let inner = self.parse_cast()?;
        Ok(CExpr::Unary(op, Box::new(inner)))
    }

    fn parse_postfix(&mut self) -> Result<CExpr, String> {
        let expr = self.parse_primary()?;
        self.parse_postfix_on(expr)
    }

    fn parse_postfix_on(&mut self, mut expr: CExpr) -> Result<CExpr, String> {
        loop {
            if self.eat_sym("[") {
                let idx = self.parse_expr()?;
                self.expect_sym("]")?;
                expr = CExpr::Index(Box::new(expr), Box::new(idx));
            } else if self.eat_sym("(") {
                let mut args = Vec::new();
                if !self.eat_sym(")") {
                    loop {
                        args.push(self.parse_expr_no_comma()?);
                        if self.eat_sym(",") {
                            continue;
                        }
                        self.expect_sym(")")?;
                        break;
                    }
                }
                expr = CExpr::Call(Box::new(expr), args);
            } else if self.eat_sym(".") {
                let f = match self.bump() {
                    CTok::Ident(s) => s,
                    other => return Err(format!("c_frontend: expected field name after '.', got {:?}", other)),
                };
                expr = CExpr::Member(Box::new(expr), f, false);
            } else if self.eat_sym("->") {
                let f = match self.bump() {
                    CTok::Ident(s) => s,
                    other => return Err(format!("c_frontend: expected field name after '->', got {:?}", other)),
                };
                expr = CExpr::Member(Box::new(expr), f, true);
            } else if self.eat_sym("++") {
                expr = CExpr::PostfixIncDec("++", Box::new(expr));
            } else if self.eat_sym("--") {
                expr = CExpr::PostfixIncDec("--", Box::new(expr));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<CExpr, String> {
        match self.bump() {
            CTok::IntLit(v, u, b) => Ok(CExpr::Int(v, u, b)),
            CTok::FloatLit(v, f32_flag) => Ok(CExpr::Float(v, f32_flag)),
            CTok::StrLit(b) => Ok(CExpr::Str(b)),
            CTok::Ident(s) => {
                if let Some(&ev) = self.enum_consts.get(&s) {
                    Ok(CExpr::Int(ev as u64, false, 32))
                } else {
                    Ok(CExpr::Var(s))
                }
            }
            CTok::Sym("(") => {
                let e = self.parse_expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            other => Err(format!("c_frontend: unexpected token in expression: {:?}", other)),
        }
    }
}

// ============================================================================
// Stage 4: LLVM SSA IR Lowerer
// ============================================================================

#[derive(Debug, Clone)]
struct CFuncSig {
    ret: CType,
    params: Vec<CType>,
    variadic: bool,
}

#[derive(Debug, Clone)]
struct LocalSlot {
    reg: String, // `%l_...` or `@...`
    ty: CType,
}

struct CLlvmLowerer {
    target_triple: String,
    ptr_size: usize,
    structs: BTreeMap<String, CStructDef>,
    unions: BTreeMap<String, CUnionDef>,
    enum_consts: HashMap<String, i64>,
    func_sigs: HashMap<String, CFuncSig>,
    defined_fns: HashSet<String>,
    called_ext_fns: HashSet<String>,
    globals: HashMap<String, CType>,
    str_lits: Vec<(String, Vec<u8>)>,
    static_locals_ir: String,
    cur_allocas: String,
    vla_scopes: Vec<Vec<String>>,
    switch_cases: Vec<HashMap<i64, String>>,
    switch_defaults: Vec<Option<String>>,
    tmp_seq: usize,
    lbl_seq: usize,
    static_seq: usize,
}

impl CLlvmLowerer {
    fn new(
        target_triple: &str,
        structs: BTreeMap<String, CStructDef>,
        unions: BTreeMap<String, CUnionDef>,
        enum_consts: HashMap<String, i64>,
    ) -> Self {
        let t = target_triple.to_ascii_lowercase();
        let ptr_size = if t.contains("thumb") || (t.contains("arm") && !t.contains("64")) || t.contains("i686") {
            4
        } else {
            8
        };
        Self {
            target_triple: target_triple.to_string(),
            ptr_size,
            structs,
            unions,
            enum_consts,
            func_sigs: HashMap::new(),
            defined_fns: HashSet::new(),
            called_ext_fns: HashSet::new(),
            globals: HashMap::new(),
            str_lits: Vec::new(),
            static_locals_ir: String::new(),
            cur_allocas: String::new(),
            vla_scopes: Vec::new(),
            switch_cases: Vec::new(),
            switch_defaults: Vec::new(),
            tmp_seq: 0,
            lbl_seq: 0,
            static_seq: 0,
        }
    }

    fn next_tmp(&mut self) -> String {
        self.tmp_seq += 1;
        format!("%t{}", self.tmp_seq)
    }

    fn next_lbl(&mut self, prefix: &str) -> String {
        self.lbl_seq += 1;
        format!("{}_{}", prefix, self.lbl_seq)
    }

    fn sizeof_type(&self, ty: &CType) -> usize {
        match ty {
            CType::Void | CType::Bool => 1,
            CType::Int { bits, .. } => (*bits as usize) / 8,
            CType::Float => 4,
            CType::Double => 8,
            CType::Ptr(_) | CType::FuncPtr { .. } => self.ptr_size,
            CType::Array(elem, n) => self.sizeof_type(elem) * (*n),
            CType::Struct(nm) => self.structs.get(nm).map(|s| s.size).unwrap_or(self.ptr_size),
            CType::Union(nm) => self.unions.get(nm).map(|u| u.size).unwrap_or(self.ptr_size),
        }
    }

    fn llvm_ty(&self, ty: &CType) -> String {
        match ty {
            CType::Void => "void".to_string(),
            CType::Bool => "i8".to_string(),
            CType::Int { bits, .. } => format!("i{}", bits),
            CType::Float => "float".to_string(),
            CType::Double => "double".to_string(),
            CType::Ptr(inner) => {
                if matches!(inner.as_ref(), CType::Void) {
                    "i8*".to_string()
                } else {
                    format!("{}*", self.llvm_ty(inner))
                }
            }
            CType::Array(elem, n) => format!("[{} x {}]", n, self.llvm_ty(elem)),
            CType::Struct(nm) => {
                if self.structs.contains_key(nm) {
                    format!("%struct.{}", nm)
                } else {
                    "i8".to_string()
                }
            }
            CType::Union(nm) => {
                if self.unions.contains_key(nm) {
                    format!("%union.{}", nm)
                } else {
                    "i8".to_string()
                }
            }
            CType::FuncPtr { ret, params, variadic } => {
                let mut ps: Vec<String> = params.iter().map(|p| self.llvm_ty(p)).collect();
                if *variadic {
                    ps.push("...".to_string());
                }
                format!("{} ({})*", self.llvm_ty(ret), ps.join(", "))
            }
        }
    }

    fn emit_tu(&mut self, tu: &CTranslationUnit) -> Result<String, String> {
        // Pass 1: register all function signatures and global variables
        for item in &tu.items {
            match item {
                CTopItem::FuncDecl {
                    name,
                    ret,
                    params,
                    variadic,
                    body,
                    ..
                } => {
                    self.func_sigs.insert(
                        name.clone(),
                        CFuncSig {
                            ret: ret.clone(),
                            params: params.iter().map(|(_, t)| t.clone()).collect(),
                            variadic: *variadic,
                        },
                    );
                    if body.is_some() {
                        self.defined_fns.insert(name.clone());
                    }
                }
                CTopItem::GlobalVar { name, ty, .. } => {
                    self.globals.insert(name.clone(), ty.clone());
                }
            }
        }

        // Pass 2: emit globals and function bodies
        let mut globals_ir = String::new();
        let mut funcs_ir = String::new();
        for item in &tu.items {
            match item {
                CTopItem::GlobalVar {
                    name,
                    ty,
                    is_static,
                    is_extern,
                    init,
                } => {
                    if *is_extern && init.is_none() {
                        let _ = writeln!(globals_ir, "@{} = external global {}", name, self.llvm_ty(ty));
                    } else {
                        let link = if *is_static { "internal " } else { "" };
                        let init_str = self.format_const_init(ty, init.as_ref());
                        let _ = writeln!(
                            globals_ir,
                            "@{} = {}global {} {}",
                            name,
                            link,
                            self.llvm_ty(ty),
                            init_str
                        );
                    }
                }
                CTopItem::FuncDecl {
                    name,
                    ret,
                    params,
                    variadic,
                    is_static,
                    body: Some(body_stmts),
                } => {
                    let f_ir = self.emit_function(name, ret, params, *variadic, *is_static, body_stmts)?;
                    funcs_ir.push_str(&f_ir);
                    funcs_ir.push('\n');
                }
                _ => {}
            }
        }

        // Assemble final LLVM IR module
        let mut out = String::new();
        let _ = writeln!(out, "; ModuleID = 'omni_c_frontend'");
        if !self.target_triple.trim().is_empty() {
            let _ = writeln!(out, "target triple = \"{}\"\n", self.target_triple.trim());
        }

        // Struct & union type definitions
        for (nm, sd) in &self.structs {
            let field_tys: Vec<String> = sd.fields.iter().map(|(_, t, _)| self.llvm_ty(t)).collect();
            let _ = writeln!(out, "%struct.{} = type {{ {} }}", nm, field_tys.join(", "));
        }
        for (nm, ud) in &self.unions {
            let _ = writeln!(out, "%union.{} = type {{ [{} x i8] }}", nm, ud.size);
        }
        if !self.structs.is_empty() || !self.unions.is_empty() {
            out.push('\n');
        }

        // String constants
        for (gname, bytes) in &self.str_lits {
            let mut esc = String::new();
            for &b in bytes {
                if (0x20..=0x7E).contains(&b) && b != b'"' && b != b'\\' {
                    esc.push(b as char);
                } else {
                    let _ = write!(esc, "\\{:02X}", b);
                }
            }
            esc.push_str("\\00");
            let _ = writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1",
                gname,
                bytes.len() + 1,
                esc
            );
        }
        if !self.str_lits.is_empty() {
            out.push('\n');
        }

        out.push_str(&globals_ir);
        out.push_str(&self.static_locals_ir);
        if !globals_ir.is_empty() || !self.static_locals_ir.is_empty() {
            out.push('\n');
        }

        // External function declarations
        let mut ext_names: Vec<String> = self
            .called_ext_fns
            .iter()
            .filter(|n| !self.defined_fns.contains(*n))
            .cloned()
            .collect();
        ext_names.sort();
        for ext in ext_names {
            if let Some(sig) = self.func_sigs.get(&ext) {
                let mut ps: Vec<String> = sig.params.iter().map(|t| self.llvm_ty(t)).collect();
                if sig.variadic {
                    ps.push("...".to_string());
                }
                let _ = writeln!(out, "declare {} @{}({})", self.llvm_ty(&sig.ret), ext, ps.join(", "));
            } else {
                let _ = writeln!(out, "declare i64 @{}(...)", ext);
            }
        }
        out.push('\n');
        out.push_str(&funcs_ir);
        Ok(out)
    }

    fn format_const_init(&mut self, ty: &CType, init: Option<&CInit>) -> String {
        let Some(init) = init else {
            return "zeroinitializer".to_string();
        };
        match (ty, init) {
            (CType::Int { .. } | CType::Bool, CInit::Expr(e)) => {
                if let Some(v) = self.eval_const_i64(e) {
                    format!("{}", v)
                } else {
                    "0".to_string()
                }
            }
            (CType::Float | CType::Double, CInit::Expr(e)) => {
                if let Some(f) = self.eval_const_f64(e) {
                    format_llvm_float(f, matches!(ty, CType::Float))
                } else {
                    "0.0".to_string()
                }
            }
            (CType::Ptr(_), CInit::Expr(CExpr::Str(bytes))) => {
                let gname = self.intern_str(bytes);
                let len = bytes.len() + 1;
                format!(
                    "getelementptr inbounds ([{} x i8], [{} x i8]* @{}, i64 0, i64 0)",
                    len, len, gname
                )
            }
            (CType::Ptr(_) | CType::FuncPtr { .. }, CInit::Expr(e)) => {
                self.format_const_ptr_expr(ty, e)
            }
            (CType::Array(elem, n), CInit::List(items)) => {
                let ety = self.llvm_ty(elem);
                let mut parts = Vec::with_capacity(*n);
                for idx in 0..*n {
                    let v = items.get(idx).map(|(_, ci)| ci);
                    parts.push(format!("{} {}", ety, self.format_const_init(elem, v)));
                }
                format!("[ {} ]", parts.join(", "))
            }
            (CType::Array(_, n), CInit::Expr(CExpr::Str(bytes))) => {
                let mut parts = Vec::with_capacity(*n);
                for idx in 0..*n {
                    let b = bytes.get(idx).copied().unwrap_or(0);
                    parts.push(format!("i8 {}", b));
                }
                format!("[ {} ]", parts.join(", "))
            }
            (CType::Struct(nm), CInit::List(items)) => {
                if let Some(sd) = self.structs.get(nm).cloned() {
                    let mut parts = Vec::with_capacity(sd.fields.len());
                    for (idx, (fname, fty, _)) in sd.fields.iter().enumerate() {
                        let named = items.iter().find(|(k, _)| k.as_deref() == Some(fname.as_str()));
                        let ci = named.or_else(|| items.get(idx)).map(|(_, c)| c);
                        parts.push(format!("{} {}", self.llvm_ty(fty), self.format_const_init(fty, ci)));
                    }
                    format!("{{ {} }}", parts.join(", "))
                } else {
                    "zeroinitializer".to_string()
                }
            }
            _ => "zeroinitializer".to_string(),
        }
    }

    fn format_const_ptr_expr(&mut self, dst_ty: &CType, e: &CExpr) -> String {
        match e {
            CExpr::Str(bytes) => {
                let gname = self.intern_str(bytes);
                let len = bytes.len() + 1;
                format!(
                    "getelementptr inbounds ([{} x i8], [{} x i8]* @{}, i64 0, i64 0)",
                    len, len, gname
                )
            }
            CExpr::Var(nm) => {
                if self.func_sigs.contains_key(nm) {
                    if !self.defined_fns.contains(nm) {
                        self.called_ext_fns.insert(nm.clone());
                    }
                    return format!("@{}", nm);
                }
                if let Some(gty) = self.globals.get(nm).cloned() {
                    if let CType::Array(elem, n) = gty {
                        let arr_lty = format!("[{} x {}]", n, self.llvm_ty(&elem));
                        return format!(
                            "getelementptr inbounds ({}, {}* @{}, i64 0, i64 0)",
                            arr_lty, arr_lty, nm
                        );
                    }
                    return format!("@{}", nm);
                }
                "null".to_string()
            }
            CExpr::Unary("&", inner) => {
                if let CExpr::Var(nm) = inner.as_ref() {
                    if self.func_sigs.contains_key(nm) {
                        if !self.defined_fns.contains(nm) {
                            self.called_ext_fns.insert(nm.clone());
                        }
                        return format!("@{}", nm);
                    }
                    if let Some(gty) = self.globals.get(nm).cloned() {
                        let src_ptr_ty = format!("{}*", self.llvm_ty(&gty));
                        let dst_lty = self.llvm_ty(dst_ty);
                        if src_ptr_ty == dst_lty {
                            return format!("@{}", nm);
                        }
                        return format!("bitcast ({} @{} to {})", src_ptr_ty, nm, dst_lty);
                    }
                }
                "null".to_string()
            }
            CExpr::Binary(op @ ("+" | "-"), a, b) => {
                if let Some(mut off) = self.eval_const_i64(b) {
                    if *op == "-" {
                        off = -off;
                    }
                    let base = self.format_const_ptr_expr(dst_ty, a);
                    if base != "null" {
                        if let CType::Ptr(elem) = dst_ty {
                            let elty = if matches!(elem.as_ref(), CType::Void) {
                                "i8".to_string()
                            } else {
                                self.llvm_ty(elem)
                            };
                            return format!(
                                "getelementptr inbounds ({}, {}* {}, i64 {})",
                                elty, elty, base, off
                            );
                        }
                    }
                }
                "null".to_string()
            }
            CExpr::Cast(_, inner) => self.format_const_ptr_expr(dst_ty, inner),
            _ => "null".to_string(),
        }
    }

    fn eval_const_i64(&self, e: &CExpr) -> Option<i64> {
        match e {
            CExpr::Int(v, _, _) => Some(*v as i64),
            CExpr::Var(nm) => self.enum_consts.get(nm).copied(),
            CExpr::SizeofType(t) => Some(self.sizeof_type(t) as i64),
            CExpr::Unary("-", x) => Some(self.eval_const_i64(x)?.wrapping_neg()),
            CExpr::Unary("+", x) => self.eval_const_i64(x),
            CExpr::Unary("~", x) => Some(!self.eval_const_i64(x)?),
            CExpr::Cast(_, x) => self.eval_const_i64(x),
            CExpr::Binary("+", a, b) => Some(self.eval_const_i64(a)?.wrapping_add(self.eval_const_i64(b)?)),
            CExpr::Binary("-", a, b) => Some(self.eval_const_i64(a)?.wrapping_sub(self.eval_const_i64(b)?)),
            CExpr::Binary("*", a, b) => Some(self.eval_const_i64(a)?.wrapping_mul(self.eval_const_i64(b)?)),
            _ => None,
        }
    }

    fn eval_const_f64(&self, e: &CExpr) -> Option<f64> {
        match e {
            CExpr::Float(v, _) => Some(*v),
            CExpr::Int(v, _, _) => Some(*v as i64 as f64),
            CExpr::Unary("-", x) => Some(-self.eval_const_f64(x)?),
            CExpr::Cast(_, x) => self.eval_const_f64(x),
            _ => None,
        }
    }

    fn intern_str(&mut self, bytes: &[u8]) -> String {
        let gname = format!(".cstr.{}", self.str_lits.len());
        self.str_lits.push((gname.clone(), bytes.to_vec()));
        gname
    }

    fn emit_function(
        &mut self,
        name: &str,
        ret: &CType,
        params: &[(String, CType)],
        variadic: bool,
        is_static: bool,
        body: &[CStmt],
    ) -> Result<String, String> {
        let mut allocas = String::new();
        self.cur_allocas.clear();
        self.vla_scopes.clear();
        self.vla_scopes.push(Vec::new());
        let mut ir = String::new();
        let mut scopes: Vec<HashMap<String, LocalSlot>> = vec![HashMap::new()];
        let mut loop_stack: Vec<(String, String)> = Vec::new(); // (break_lbl, cont_lbl)
        let mut terminated = false;

        // Parameter signature
        let mut sig_parts = Vec::new();
        for (idx, (pname, pty)) in params.iter().enumerate() {
            let arg_reg = format!("%arg_{}", idx);
            let lty = self.llvm_ty(pty);
            sig_parts.push(format!("{} {}", lty, arg_reg));
            let slot_reg = format!("%p_{}_{}", idx, pname);
            let _ = writeln!(allocas, "  {} = alloca {}", slot_reg, lty);
            let _ = writeln!(ir, "  store {} {}, {}* {}", lty, arg_reg, lty, slot_reg);
            if !pname.is_empty() {
                scopes[0].insert(
                    pname.clone(),
                    LocalSlot {
                        reg: slot_reg,
                        ty: pty.clone(),
                    },
                );
            }
        }
        if variadic {
            sig_parts.push("...".to_string());
        }

        for st in body {
            self.emit_stmt(
                st,
                ret,
                &mut allocas,
                &mut ir,
                &mut scopes,
                &mut loop_stack,
                &mut terminated,
            )?;
        }

        if !terminated {
            self.emit_free_all_vlas(&mut ir);
            if matches!(ret, CType::Void) {
                let _ = writeln!(ir, "  ret void");
            } else {
                let z = self.zero_val(ret);
                let _ = writeln!(ir, "  ret {} {}", self.llvm_ty(ret), z);
            }
        }
        self.vla_scopes.clear();

        let linkage = if is_static { "internal " } else { "" };
        let mut out = String::new();
        let _ = writeln!(
            out,
            "define {}{} @{}({}) {{",
            linkage,
            self.llvm_ty(ret),
            name,
            sig_parts.join(", ")
        );
        let _ = writeln!(out, "entry:");
        out.push_str(&allocas);
        out.push_str(&self.cur_allocas);
        out.push_str(&ir);
        let _ = writeln!(out, "}}");
        Ok(out)
    }

    fn emit_free_vla_list(&mut self, ptrs: &[String], ir: &mut String) {
        if ptrs.is_empty() {
            return;
        }
        self.ensure_stdlib_decl("free", CType::Void, vec![CType::Ptr(Box::new(CType::Void))]);
        for p in ptrs.iter().rev() {
            let _ = writeln!(ir, "  call void @free(i8* {})", p);
        }
    }

    fn emit_free_all_vlas(&mut self, ir: &mut String) {
        let all: Vec<String> = self
            .vla_scopes
            .iter()
            .flat_map(|v| v.iter().cloned())
            .collect();
        self.emit_free_vla_list(&all, ir);
    }

    fn ensure_stdlib_decl(&mut self, name: &str, ret: CType, params: Vec<CType>) {
        if !self.defined_fns.contains(name) {
            self.called_ext_fns.insert(name.to_string());
            self.func_sigs.entry(name.to_string()).or_insert(CFuncSig {
                ret,
                params,
                variadic: false,
            });
        }
    }

    fn zero_val(&self, ty: &CType) -> &'static str {
        match ty {
            CType::Float | CType::Double => "0.0",
            CType::Ptr(_) | CType::FuncPtr { .. } => "null",
            CType::Struct(_) | CType::Union(_) | CType::Array(_, _) => "zeroinitializer",
            _ => "0",
        }
    }

    fn lookup_var(&self, name: &str, scopes: &[HashMap<String, LocalSlot>]) -> Option<LocalSlot> {
        for sc in scopes.iter().rev() {
            if let Some(slot) = sc.get(name) {
                return Some(slot.clone());
            }
        }
        if let Some(gty) = self.globals.get(name) {
            return Some(LocalSlot {
                reg: format!("@{}", name),
                ty: gty.clone(),
            });
        }
        None
    }

    fn emit_stmt(
        &mut self,
        stmt: &CStmt,
        fn_ret: &CType,
        allocas: &mut String,
        ir: &mut String,
        scopes: &mut Vec<HashMap<String, LocalSlot>>,
        loop_stack: &mut Vec<(String, String)>,
        terminated: &mut bool,
    ) -> Result<(), String> {
        // Labels, cases, defaults start a new reachable block even after a terminator
        match stmt {
            CStmt::Label(lbl, inner) => {
                if !*terminated {
                    let _ = writeln!(ir, "  br label %usr_{}", lbl);
                }
                let _ = writeln!(ir, "usr_{}:", lbl);
                *terminated = false;
                return self.emit_stmt(inner, fn_ret, allocas, ir, scopes, loop_stack, terminated);
            }
            CStmt::Case(v) => {
                if let Some(lbl) = self.switch_cases.last().and_then(|m| m.get(v)).cloned() {
                    if !*terminated {
                        let _ = writeln!(ir, "  br label %{}", lbl);
                    }
                    let _ = writeln!(ir, "{}:", lbl);
                    *terminated = false;
                }
                return Ok(());
            }
            CStmt::Default => {
                if let Some(Some(lbl)) = self.switch_defaults.last().cloned() {
                    if !*terminated {
                        let _ = writeln!(ir, "  br label %{}", lbl);
                    }
                    let _ = writeln!(ir, "{}:", lbl);
                    *terminated = false;
                }
                return Ok(());
            }
            _ if *terminated => return Ok(()),
            _ => {}
        }

        match stmt {
            CStmt::Empty => {}
            CStmt::Block(stmts) => {
                scopes.push(HashMap::new());
                self.vla_scopes.push(Vec::new());
                for s in stmts {
                    self.emit_stmt(s, fn_ret, allocas, ir, scopes, loop_stack, terminated)?;
                }
                let vla_ptrs = self.vla_scopes.pop().unwrap_or_default();
                if !*terminated {
                    self.emit_free_vla_list(&vla_ptrs, ir);
                }
                scopes.pop();
            }
            CStmt::VarDecl {
                name,
                ty,
                is_static,
                init,
                vla_len,
            } => {
                if let Some(len_expr) = vla_len {
                    // C99 VLA: `T arr[n]` -> heap-backed dynamic allocation with RAII scope free
                    let elem_ty = match ty {
                        CType::Ptr(inner) => inner.as_ref().clone(),
                        other => other.clone(),
                    };
                    let elem_sz = self.sizeof_type(&elem_ty).max(1);
                    let (nv, nty) = self.emit_expr(len_expr, ir, scopes)?;
                    let n64 = self.emit_cast(&nv, &nty, &CType::Int { bits: 64, signed: false }, ir);
                    let bytes = self.next_tmp();
                    let _ = writeln!(ir, "  {} = mul i64 {}, {}", bytes, n64, elem_sz);
                    self.ensure_stdlib_decl(
                        "malloc",
                        CType::Ptr(Box::new(CType::Void)),
                        vec![CType::Int { bits: 64, signed: false }],
                    );
                    let raw_ptr = self.next_tmp();
                    let _ = writeln!(ir, "  {} = call i8* @malloc(i64 {})", raw_ptr, bytes);
                    let lty = self.llvm_ty(ty);
                    let typed_ptr = if lty == "i8*" {
                        raw_ptr.clone()
                    } else {
                        let bc = self.next_tmp();
                        let _ = writeln!(ir, "  {} = bitcast i8* {} to {}", bc, raw_ptr, lty);
                        bc
                    };
                    let slot = self.next_tmp();
                    let _ = writeln!(allocas, "  {} = alloca {}", slot, lty);
                    let _ = writeln!(ir, "  store {} {}, {}* {}", lty, typed_ptr, lty, slot);
                    if let Some(sc) = self.vla_scopes.last_mut() {
                        sc.push(raw_ptr);
                    }
                    scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalSlot {
                            reg: slot,
                            ty: ty.clone(),
                        },
                    );
                } else if *is_static {
                    self.static_seq += 1;
                    let gname = format!("__omni_cstatic_{}_{}", self.static_seq, name);
                    let init_str = self.format_const_init(ty, init.as_ref());
                    let _ = writeln!(
                        self.static_locals_ir,
                        "@{} = internal global {} {}",
                        gname,
                        self.llvm_ty(ty),
                        init_str
                    );
                    scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalSlot {
                            reg: format!("@{}", gname),
                            ty: ty.clone(),
                        },
                    );
                } else {
                    let slot = self.next_tmp();
                    let lty = self.llvm_ty(ty);
                    let _ = writeln!(allocas, "  {} = alloca {}", slot, lty);
                    if let Some(ci) = init {
                        self.emit_local_init(&slot, ty, ci, ir, scopes)?;
                    }
                    scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalSlot {
                            reg: slot,
                            ty: ty.clone(),
                        },
                    );
                }
            }
            CStmt::Expr(e) => {
                let _ = self.emit_expr(e, ir, scopes)?;
            }
            CStmt::Return(opt_e) => {
                if matches!(fn_ret, CType::Void) {
                    if let Some(e) = opt_e {
                        let _ = self.emit_expr(e, ir, scopes)?;
                    }
                    self.emit_free_all_vlas(ir);
                    let _ = writeln!(ir, "  ret void");
                } else if let Some(e) = opt_e {
                    let (v, vty) = self.emit_expr(e, ir, scopes)?;
                    let cv = self.emit_cast(&v, &vty, fn_ret, ir);
                    self.emit_free_all_vlas(ir);
                    let _ = writeln!(ir, "  ret {} {}", self.llvm_ty(fn_ret), cv);
                } else {
                    self.emit_free_all_vlas(ir);
                    let _ = writeln!(ir, "  ret {} {}", self.llvm_ty(fn_ret), self.zero_val(fn_ret));
                }
                *terminated = true;
            }
            CStmt::If { cond, then_s, else_s } => {
                let (cv, cty) = self.emit_expr(cond, ir, scopes)?;
                let cbool = self.emit_to_i1(&cv, &cty, ir);
                let l_then = self.next_lbl("if_then");
                let l_else = self.next_lbl("if_else");
                let l_end = self.next_lbl("if_end");
                let false_target = if else_s.is_some() { &l_else } else { &l_end };
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", cbool, l_then, false_target);

                let _ = writeln!(ir, "{}:", l_then);
                let mut then_term = false;
                self.emit_stmt(then_s, fn_ret, allocas, ir, scopes, loop_stack, &mut then_term)?;
                if !then_term {
                    let _ = writeln!(ir, "  br label %{}", l_end);
                }

                let mut else_term = false;
                if let Some(es) = else_s {
                    let _ = writeln!(ir, "{}:", l_else);
                    self.emit_stmt(es, fn_ret, allocas, ir, scopes, loop_stack, &mut else_term)?;
                    if !else_term {
                        let _ = writeln!(ir, "  br label %{}", l_end);
                    }
                }

                if then_term && else_term && else_s.is_some() {
                    *terminated = true;
                } else {
                    let _ = writeln!(ir, "{}:", l_end);
                    *terminated = false;
                }
            }
            CStmt::While { cond, body } => {
                let l_cond = self.next_lbl("wh_cond");
                let l_body = self.next_lbl("wh_body");
                let l_end = self.next_lbl("wh_end");
                let _ = writeln!(ir, "  br label %{}", l_cond);
                let _ = writeln!(ir, "{}:", l_cond);
                let (cv, cty) = self.emit_expr(cond, ir, scopes)?;
                let cbool = self.emit_to_i1(&cv, &cty, ir);
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", cbool, l_body, l_end);

                let _ = writeln!(ir, "{}:", l_body);
                loop_stack.push((l_end.clone(), l_cond.clone()));
                let mut bterm = false;
                self.emit_stmt(body, fn_ret, allocas, ir, scopes, loop_stack, &mut bterm)?;
                loop_stack.pop();
                if !bterm {
                    let _ = writeln!(ir, "  br label %{}", l_cond);
                }
                let _ = writeln!(ir, "{}:", l_end);
                *terminated = false;
            }
            CStmt::DoWhile { body, cond } => {
                let l_body = self.next_lbl("do_body");
                let l_cond = self.next_lbl("do_cond");
                let l_end = self.next_lbl("do_end");
                let _ = writeln!(ir, "  br label %{}", l_body);
                let _ = writeln!(ir, "{}:", l_body);
                loop_stack.push((l_end.clone(), l_cond.clone()));
                let mut bterm = false;
                self.emit_stmt(body, fn_ret, allocas, ir, scopes, loop_stack, &mut bterm)?;
                loop_stack.pop();
                if !bterm {
                    let _ = writeln!(ir, "  br label %{}", l_cond);
                }
                let _ = writeln!(ir, "{}:", l_cond);
                let (cv, cty) = self.emit_expr(cond, ir, scopes)?;
                let cbool = self.emit_to_i1(&cv, &cty, ir);
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", cbool, l_body, l_end);
                let _ = writeln!(ir, "{}:", l_end);
                *terminated = false;
            }
            CStmt::For { init, cond, inc, body } => {
                scopes.push(HashMap::new());
                if let Some(init_s) = init {
                    let mut iterm = false;
                    self.emit_stmt(init_s, fn_ret, allocas, ir, scopes, loop_stack, &mut iterm)?;
                }
                let l_cond = self.next_lbl("for_cond");
                let l_body = self.next_lbl("for_body");
                let l_inc = self.next_lbl("for_inc");
                let l_end = self.next_lbl("for_end");
                let _ = writeln!(ir, "  br label %{}", l_cond);

                let _ = writeln!(ir, "{}:", l_cond);
                if let Some(ce) = cond {
                    let (cv, cty) = self.emit_expr(ce, ir, scopes)?;
                    let cbool = self.emit_to_i1(&cv, &cty, ir);
                    let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", cbool, l_body, l_end);
                } else {
                    let _ = writeln!(ir, "  br label %{}", l_body);
                }

                let _ = writeln!(ir, "{}:", l_body);
                loop_stack.push((l_end.clone(), l_inc.clone()));
                let mut bterm = false;
                self.emit_stmt(body, fn_ret, allocas, ir, scopes, loop_stack, &mut bterm)?;
                loop_stack.pop();
                if !bterm {
                    let _ = writeln!(ir, "  br label %{}", l_inc);
                }

                let _ = writeln!(ir, "{}:", l_inc);
                if let Some(ie) = inc {
                    let _ = self.emit_expr(ie, ir, scopes)?;
                }
                let _ = writeln!(ir, "  br label %{}", l_cond);

                let _ = writeln!(ir, "{}:", l_end);
                scopes.pop();
                *terminated = false;
            }
            CStmt::Switch { expr, body } => {
                let (sv, sty) = self.emit_expr(expr, ir, scopes)?;
                let sv64 = self.emit_cast(&sv, &sty, &CType::Int { bits: 64, signed: true }, ir);
                let l_end = self.next_lbl("sw_end");
                let mut case_lbls: Vec<(i64, String)> = Vec::new();
                let mut case_map: HashMap<i64, String> = HashMap::new();
                let mut default_lbl: Option<String> = None;

                // Recursively collect all Case / Default labels (supporting Duff's device & nested blocks!)
                self.collect_switch_targets(body, &mut case_lbls, &mut case_map, &mut default_lbl);

                let def_target = default_lbl.clone().unwrap_or_else(|| l_end.clone());
                let _ = writeln!(ir, "  switch i64 {}, label %{} [", sv64, def_target);
                for (v, l) in &case_lbls {
                    let _ = writeln!(ir, "    i64 {}, label %{}", v, l);
                }
                let _ = writeln!(ir, "  ]");

                let cont_target = loop_stack
                    .last()
                    .map(|(_, c)| c.clone())
                    .unwrap_or_else(|| l_end.clone());
                loop_stack.push((l_end.clone(), cont_target));
                self.switch_cases.push(case_map);
                self.switch_defaults.push(default_lbl);
                let mut cur_term = true;
                for s in body {
                    self.emit_stmt(s, fn_ret, allocas, ir, scopes, loop_stack, &mut cur_term)?;
                }
                self.switch_cases.pop();
                self.switch_defaults.pop();
                loop_stack.pop();
                if !cur_term {
                    let _ = writeln!(ir, "  br label %{}", l_end);
                }
                let _ = writeln!(ir, "{}:", l_end);
                *terminated = false;
            }
            CStmt::Case(_) | CStmt::Default => unreachable!(),
            CStmt::Break => {
                if let Some((brk, _)) = loop_stack.last() {
                    let _ = writeln!(ir, "  br label %{}", brk);
                    *terminated = true;
                }
            }
            CStmt::Continue => {
                if let Some((_, cont)) = loop_stack.last() {
                    let _ = writeln!(ir, "  br label %{}", cont);
                    *terminated = true;
                }
            }
            CStmt::Goto(lbl) => {
                let _ = writeln!(ir, "  br label %usr_{}", lbl);
                *terminated = true;
            }
            CStmt::Label(_, _) => unreachable!(),
        }
        Ok(())
    }

    fn emit_local_init(
        &mut self,
        slot_ptr: &str,
        ty: &CType,
        init: &CInit,
        ir: &mut String,
        scopes: &[HashMap<String, LocalSlot>],
    ) -> Result<(), String> {
        match (ty, init) {
            (CType::Array(elem, n), CInit::List(items)) => {
                let arr_lty = self.llvm_ty(ty);
                let _ = writeln!(ir, "  store {} zeroinitializer, {}* {}", arr_lty, arr_lty, slot_ptr);
                for (idx, (_, ci)) in items.iter().enumerate() {
                    if idx >= *n {
                        break;
                    }
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 {}",
                        gep, arr_lty, arr_lty, slot_ptr, idx
                    );
                    self.emit_local_init(&gep, elem, ci, ir, scopes)?;
                }
            }
            (CType::Array(_, n), CInit::Expr(CExpr::Str(bytes))) => {
                let arr_lty = self.llvm_ty(ty);
                let _ = writeln!(ir, "  store {} zeroinitializer, {}* {}", arr_lty, arr_lty, slot_ptr);
                for (idx, &b) in bytes.iter().take(*n).enumerate() {
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 {}",
                        gep, arr_lty, arr_lty, slot_ptr, idx
                    );
                    let _ = writeln!(ir, "  store i8 {}, i8* {}", b, gep);
                }
            }
            (CType::Struct(nm), CInit::List(items)) => {
                let slty = self.llvm_ty(ty);
                let _ = writeln!(ir, "  store {} zeroinitializer, {}* {}", slty, slty, slot_ptr);
                if let Some(sd) = self.structs.get(nm).cloned() {
                    for (idx, (fname, fty, _)) in sd.fields.iter().enumerate() {
                        let ci = items
                            .iter()
                            .find(|(k, _)| k.as_deref() == Some(fname.as_str()))
                            .or_else(|| items.get(idx))
                            .map(|(_, c)| c);
                        if let Some(ci) = ci {
                            let gep = self.next_tmp();
                            let _ = writeln!(
                                ir,
                                "  {} = getelementptr inbounds {}, {}* {}, i32 0, i32 {}",
                                gep, slty, slty, slot_ptr, idx
                            );
                            self.emit_local_init(&gep, fty, ci, ir, scopes)?;
                        }
                    }
                }
            }
            (_, CInit::Expr(e)) => {
                let (v, vty) = self.emit_expr(e, ir, scopes)?;
                let cv = self.emit_cast(&v, &vty, ty, ir);
                let lty = self.llvm_ty(ty);
                let _ = writeln!(ir, "  store {} {}, {}* {}", lty, cv, lty, slot_ptr);
            }
            (_, CInit::List(items)) => {
                if let Some((_, first)) = items.first() {
                    self.emit_local_init(slot_ptr, ty, first, ir, scopes)?;
                } else {
                    let lty = self.llvm_ty(ty);
                    let _ = writeln!(ir, "  store {} {}, {}* {}", lty, self.zero_val(ty), lty, slot_ptr);
                }
            }
        }
        Ok(())
    }

    fn collect_switch_targets(
        &mut self,
        stmts: &[CStmt],
        case_lbls: &mut Vec<(i64, String)>,
        case_map: &mut HashMap<i64, String>,
        default_lbl: &mut Option<String>,
    ) {
        for s in stmts {
            match s {
                CStmt::Case(v) => {
                    let l = self.next_lbl("sw_case");
                    case_lbls.push((*v, l.clone()));
                    case_map.insert(*v, l);
                }
                CStmt::Default => {
                    let l = self.next_lbl("sw_default");
                    *default_lbl = Some(l);
                }
                CStmt::Block(inner) => {
                    self.collect_switch_targets(inner, case_lbls, case_map, default_lbl);
                }
                CStmt::If { then_s, else_s, .. } => {
                    self.collect_switch_targets(std::slice::from_ref(then_s.as_ref()), case_lbls, case_map, default_lbl);
                    if let Some(es) = else_s {
                        self.collect_switch_targets(std::slice::from_ref(es.as_ref()), case_lbls, case_map, default_lbl);
                    }
                }
                CStmt::While { body, .. } | CStmt::DoWhile { body, .. } | CStmt::For { body, .. } => {
                    self.collect_switch_targets(std::slice::from_ref(body.as_ref()), case_lbls, case_map, default_lbl);
                }
                CStmt::Label(_, inner) => {
                    self.collect_switch_targets(std::slice::from_ref(inner.as_ref()), case_lbls, case_map, default_lbl);
                }
                // Do NOT descend into nested `Switch` (its cases belong to the inner switch)
                _ => {}
            }
        }
    }

    fn resolve_member_in_agg(
        &mut self,
        base_ptr: &str,
        base_ty: &CType,
        field: &str,
        ir: &mut String,
    ) -> Option<(String, CType)> {
        match base_ty {
            CType::Struct(nm) => {
                let sd = self.structs.get(nm)?.clone();
                if let Some((idx, (_, fty, _))) = sd.fields.iter().enumerate().find(|(_, (fnm, _, _))| fnm == field) {
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}",
                        gep, nm, nm, base_ptr, idx
                    );
                    return Some((gep, fty.clone()));
                }
                // Search anonymous inner struct/union fields recursively
                for (idx, (fnm, fty, _)) in sd.fields.iter().enumerate() {
                    if fnm.is_empty() && matches!(fty, CType::Struct(_) | CType::Union(_)) {
                        let mut sub_ir = String::new();
                        let gep = self.next_tmp();
                        let _ = writeln!(
                            sub_ir,
                            "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}",
                            gep, nm, nm, base_ptr, idx
                        );
                        if let Some(found) = self.resolve_member_in_agg(&gep, fty, field, &mut sub_ir) {
                            ir.push_str(&sub_ir);
                            return Some(found);
                        }
                    }
                }
                None
            }
            CType::Union(nm) => {
                let ud = self.unions.get(nm)?.clone();
                if let Some((_, fty)) = ud.fields.iter().find(|(fnm, _)| fnm == field) {
                    let cast = self.next_tmp();
                    let flty = self.llvm_ty(fty);
                    let _ = writeln!(
                        ir,
                        "  {} = bitcast %union.{}* {} to {}*",
                        cast, nm, base_ptr, flty
                    );
                    return Some((cast, fty.clone()));
                }
                for (fnm, fty) in &ud.fields {
                    if fnm.is_empty() && matches!(fty, CType::Struct(_) | CType::Union(_)) {
                        let mut sub_ir = String::new();
                        let cast = self.next_tmp();
                        let flty = self.llvm_ty(fty);
                        let _ = writeln!(
                            sub_ir,
                            "  {} = bitcast %union.{}* {} to {}*",
                            cast, nm, base_ptr, flty
                        );
                        if let Some(found) = self.resolve_member_in_agg(&cast, fty, field, &mut sub_ir) {
                            ir.push_str(&sub_ir);
                            return Some(found);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn resolve_bitfield_in_agg(
        &mut self,
        base_ptr: &str,
        base_ty: &CType,
        field: &str,
        ir: &mut String,
    ) -> Option<(String, CBitfieldInfo)> {
        if let CType::Struct(nm) = base_ty {
            let sd = self.structs.get(nm)?.clone();
            if let Some(bf) = sd.bitfields.get(field).cloned() {
                let gep = self.next_tmp();
                let _ = writeln!(
                    ir,
                    "  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}",
                    gep, nm, nm, base_ptr, bf.storage_idx
                );
                return Some((gep, bf));
            }
        }
        None
    }

    fn emit_bitfield_load(&mut self, storage_ptr: &str, bf: &CBitfieldInfo, ir: &mut String) -> (String, CType) {
        let slty = self.llvm_ty(&bf.storage_ty);
        let sbits = bf.storage_ty.int_bits();
        let raw = self.next_tmp();
        let _ = writeln!(ir, "  {} = load {}, {}* {}", raw, slty, slty, storage_ptr);
        let shifted = if bf.bit_offset > 0 {
            let t = self.next_tmp();
            let _ = writeln!(ir, "  {} = lshr {} {}, {}", t, slty, raw, bf.bit_offset);
            t
        } else {
            raw
        };
        let mask: u64 = if bf.bit_width >= 64 {
            u64::MAX
        } else {
            (1u64 << bf.bit_width) - 1
        };
        let masked = self.next_tmp();
        let _ = writeln!(ir, "  {} = and {} {}, {}", masked, slty, shifted, mask);
        let extracted = if bf.logical_ty.is_signed() && bf.bit_width < sbits {
            let sh = sbits - bf.bit_width;
            let t1 = self.next_tmp();
            let t2 = self.next_tmp();
            let _ = writeln!(ir, "  {} = shl {} {}, {}", t1, slty, masked, sh);
            let _ = writeln!(ir, "  {} = ashr {} {}, {}", t2, slty, t1, sh);
            t2
        } else {
            masked
        };
        let out = self.emit_cast(&extracted, &bf.storage_ty, &bf.logical_ty, ir);
        (out, bf.logical_ty.clone())
    }

    fn emit_bitfield_store(
        &mut self,
        storage_ptr: &str,
        bf: &CBitfieldInfo,
        val: &str,
        val_ty: &CType,
        ir: &mut String,
    ) -> (String, CType) {
        let slty = self.llvm_ty(&bf.storage_ty);
        let mask: u64 = if bf.bit_width >= 64 {
            u64::MAX
        } else {
            (1u64 << bf.bit_width) - 1
        };
        let clear_mask = !(mask.wrapping_shl(bf.bit_offset as u32));
        let cur = self.next_tmp();
        let _ = writeln!(ir, "  {} = load {}, {}* {}", cur, slty, slty, storage_ptr);
        let cleared = self.next_tmp();
        let clear_lit = if bf.storage_ty.int_bits() == 64 {
            format!("{}", clear_mask as i64)
        } else {
            format!("{}", clear_mask as i32)
        };
        let _ = writeln!(ir, "  {} = and {} {}, {}", cleared, slty, cur, clear_lit);
        let v_s = self.emit_cast(val, val_ty, &bf.storage_ty, ir);
        let v_m = self.next_tmp();
        let _ = writeln!(ir, "  {} = and {} {}, {}", v_m, slty, v_s, mask);
        let v_sh = if bf.bit_offset > 0 {
            let t = self.next_tmp();
            let _ = writeln!(ir, "  {} = shl {} {}, {}", t, slty, v_m, bf.bit_offset);
            t
        } else {
            v_m
        };
        let combined = self.next_tmp();
        let _ = writeln!(ir, "  {} = or {} {}, {}", combined, slty, cleared, v_sh);
        let _ = writeln!(ir, "  store {} {}, {}* {}", slty, combined, slty, storage_ptr);
        let ret_v = self.emit_cast(val, val_ty, &bf.logical_ty, ir);
        (ret_v, bf.logical_ty.clone())
    }

    /// Computes the address (lvalue pointer) of an expression and its pointee `CType`.
    fn emit_lvalue(
        &mut self,
        expr: &CExpr,
        ir: &mut String,
        scopes: &[HashMap<String, LocalSlot>],
    ) -> Result<(String, CType), String> {
        match expr {
            CExpr::Var(nm) => {
                if let Some(slot) = self.lookup_var(nm, scopes) {
                    Ok((slot.reg, slot.ty))
                } else {
                    Err(format!("c_frontend: undefined variable '{}'", nm))
                }
            }
            CExpr::Unary("*", inner) => {
                let (pv, pty) = self.emit_expr(inner, ir, scopes)?;
                match pty {
                    CType::Ptr(elem) => Ok((pv, *elem)),
                    CType::Array(elem, _) => Ok((pv, *elem)),
                    _ => Err(format!("c_frontend: cannot dereference non-pointer type {:?}", pty)),
                }
            }
            CExpr::Index(base, idx_e) => {
                let (iv, ity) = self.emit_expr(idx_e, ir, scopes)?;
                let iv64 = self.emit_cast(&iv, &ity, &CType::Int { bits: 64, signed: true }, ir);
                // If base is an array lvalue, GEP directly on the array
                if let Ok((addr, CType::Array(elem, n))) = self.emit_lvalue(base, ir, scopes) {
                    let arr_lty = format!("[{} x {}]", n, self.llvm_ty(&elem));
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 {}",
                        gep, arr_lty, arr_lty, addr, iv64
                    );
                    return Ok((gep, *elem));
                }
                let (bv, bty) = self.emit_expr(base, ir, scopes)?;
                match bty {
                    CType::Ptr(elem) => {
                        let elty = if matches!(elem.as_ref(), CType::Void) {
                            "i8".to_string()
                        } else {
                            self.llvm_ty(&elem)
                        };
                        let gep = self.next_tmp();
                        let _ = writeln!(
                            ir,
                            "  {} = getelementptr inbounds {}, {}* {}, i64 {}",
                            gep, elty, elty, bv, iv64
                        );
                        Ok((gep, *elem))
                    }
                    _ => Err(format!("c_frontend: cannot index non-pointer type {:?}", bty)),
                }
            }
            CExpr::CompoundLit(ty, init) => {
                let slot = self.next_tmp();
                let lty = self.llvm_ty(ty);
                let _ = writeln!(self.cur_allocas, "  {} = alloca {}", slot, lty);
                self.emit_local_init(&slot, ty, init, ir, scopes)?;
                Ok((slot, ty.clone()))
            }
            CExpr::Member(base, field, is_arrow) => {
                let (base_ptr, base_ty) = if *is_arrow {
                    let (pv, pty) = self.emit_expr(base, ir, scopes)?;
                    match pty {
                        CType::Ptr(inner) => (pv, *inner),
                        _ => return Err(format!("c_frontend: '->' requires pointer, got {:?}", pty)),
                    }
                } else {
                    self.emit_lvalue(base, ir, scopes)?
                };
                self.resolve_member_in_agg(&base_ptr, &base_ty, field, ir)
                    .ok_or_else(|| format!("c_frontend: {:?} has no field '{}'", base_ty, field))
            }
            _ => Err(format!("c_frontend: expression is not an assignable lvalue: {:?}", expr)),
        }
    }

    fn emit_expr(
        &mut self,
        expr: &CExpr,
        ir: &mut String,
        scopes: &[HashMap<String, LocalSlot>],
    ) -> Result<(String, CType), String> {
        match expr {
            CExpr::Int(v, uns, bits) => {
                let ty = CType::Int {
                    bits: *bits,
                    signed: !*uns,
                };
                let s = if *bits == 64 {
                    format!("{}", *v as i64)
                } else {
                    format!("{}", *v as i32)
                };
                Ok((s, ty))
            }
            CExpr::Float(v, is_f32) => {
                let ty = if *is_f32 { CType::Float } else { CType::Double };
                Ok((format_llvm_float(*v, *is_f32), ty))
            }
            CExpr::Str(bytes) => {
                let gname = self.intern_str(bytes);
                let len = bytes.len() + 1;
                let gep = self.next_tmp();
                let _ = writeln!(
                    ir,
                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* @{}, i64 0, i64 0",
                    gep, len, len, gname
                );
                Ok((gep, CType::Ptr(Box::new(CType::Int { bits: 8, signed: true }))))
            }
            CExpr::Var(nm) => {
                if let Some(slot) = self.lookup_var(nm, scopes) {
                    // Arrays decay to pointer to first element in rvalue position
                    if let CType::Array(elem, n) = &slot.ty {
                        let arr_lty = format!("[{} x {}]", n, self.llvm_ty(elem));
                        let gep = self.next_tmp();
                        let _ = writeln!(
                            ir,
                            "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 0",
                            gep, arr_lty, arr_lty, slot.reg
                        );
                        return Ok((gep, CType::Ptr(elem.clone())));
                    }
                    let tmp = self.next_tmp();
                    let lty = self.llvm_ty(&slot.ty);
                    let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, lty, lty, slot.reg);
                    return Ok((tmp, slot.ty));
                }
                if let Some(sig) = self.func_sigs.get(nm).cloned() {
                    if !self.defined_fns.contains(nm) {
                        self.called_ext_fns.insert(nm.clone());
                    }
                    return Ok((
                        format!("@{}", nm),
                        CType::FuncPtr {
                            ret: Box::new(sig.ret),
                            params: sig.params,
                            variadic: sig.variadic,
                        },
                    ));
                }
                Err(format!("c_frontend: undefined identifier '{}'", nm))
            }
            CExpr::SizeofType(t) => Ok((
                format!("{}", self.sizeof_type(t)),
                CType::Int {
                    bits: (self.ptr_size * 8) as u8,
                    signed: false,
                },
            )),
            CExpr::SizeofExpr(inner) => {
                let sz = if let CExpr::Var(nm) = inner.as_ref() {
                    if let Some(slot) = self.lookup_var(nm, scopes) {
                        self.sizeof_type(&slot.ty)
                    } else {
                        self.ptr_size
                    }
                } else {
                    let mut dummy = String::new();
                    let (_, ty) = self.emit_expr(inner, &mut dummy, scopes)?;
                    self.sizeof_type(&ty)
                };
                Ok((
                    format!("{}", sz),
                    CType::Int {
                        bits: (self.ptr_size * 8) as u8,
                        signed: false,
                    },
                ))
            }
            CExpr::Cast(dst_ty, inner) => {
                let (v, vty) = self.emit_expr(inner, ir, scopes)?;
                if matches!(dst_ty, CType::Void) {
                    return Ok(("0".to_string(), CType::Void));
                }
                let cv = self.emit_cast(&v, &vty, dst_ty, ir);
                Ok((cv, dst_ty.clone()))
            }
            CExpr::Unary("&", inner) => {
                if let CExpr::Var(nm) = inner.as_ref() {
                    if let Some(sig) = self.func_sigs.get(nm).cloned() {
                        if !self.defined_fns.contains(nm) {
                            self.called_ext_fns.insert(nm.clone());
                        }
                        return Ok((
                            format!("@{}", nm),
                            CType::FuncPtr {
                                ret: Box::new(sig.ret),
                                params: sig.params,
                                variadic: sig.variadic,
                            },
                        ));
                    }
                }
                let (addr, ty) = self.emit_lvalue(inner, ir, scopes)?;
                Ok((addr, CType::Ptr(Box::new(ty))))
            }
            CExpr::Unary("*", _inner) => {
                let (addr, ty) = self.emit_lvalue(expr, ir, scopes)?;
                let tmp = self.next_tmp();
                let lty = self.llvm_ty(&ty);
                let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, lty, lty, addr);
                Ok((tmp, ty))
            }
            CExpr::Unary("+", inner) => self.emit_expr(inner, ir, scopes),
            CExpr::Unary("-", inner) => {
                let (v, ty) = self.emit_expr(inner, ir, scopes)?;
                let tmp = self.next_tmp();
                let lty = self.llvm_ty(&ty);
                if ty.is_float() {
                    let _ = writeln!(ir, "  {} = fsub {} 0.0, {}", tmp, lty, v);
                } else {
                    let pty = self.promote_int(&ty);
                    let pv = self.emit_cast(&v, &ty, &pty, ir);
                    let plty = self.llvm_ty(&pty);
                    let _ = writeln!(ir, "  {} = sub {} 0, {}", tmp, plty, pv);
                    return Ok((tmp, pty));
                }
                Ok((tmp, ty))
            }
            CExpr::Unary("~", inner) => {
                let (v, ty) = self.emit_expr(inner, ir, scopes)?;
                let pty = self.promote_int(&ty);
                let pv = self.emit_cast(&v, &ty, &pty, ir);
                let tmp = self.next_tmp();
                let _ = writeln!(ir, "  {} = xor {} {}, -1", tmp, self.llvm_ty(&pty), pv);
                Ok((tmp, pty))
            }
            CExpr::Unary("!", inner) => {
                let (v, ty) = self.emit_expr(inner, ir, scopes)?;
                let b = self.emit_to_i1(&v, &ty, ir);
                let inv = self.next_tmp();
                let _ = writeln!(ir, "  {} = xor i1 {}, true", inv, b);
                let res = self.next_tmp();
                let _ = writeln!(ir, "  {} = zext i1 {} to i32", res, inv);
                Ok((res, CType::Int { bits: 32, signed: true }))
            }
            CExpr::Unary(op @ ("++" | "--"), inner) => {
                let (addr, ty) = self.emit_lvalue(inner, ir, scopes)?;
                let lty = self.llvm_ty(&ty);
                let cur = self.next_tmp();
                let _ = writeln!(ir, "  {} = load {}, {}* {}", cur, lty, lty, addr);
                let next = self.emit_step(&cur, &ty, *op == "++", ir);
                let _ = writeln!(ir, "  store {} {}, {}* {}", lty, next, lty, addr);
                Ok((next, ty))
            }
            CExpr::PostfixIncDec(op, inner) => {
                let (addr, ty) = self.emit_lvalue(inner, ir, scopes)?;
                let lty = self.llvm_ty(&ty);
                let cur = self.next_tmp();
                let _ = writeln!(ir, "  {} = load {}, {}* {}", cur, lty, lty, addr);
                let next = self.emit_step(&cur, &ty, *op == "++", ir);
                let _ = writeln!(ir, "  store {} {}, {}* {}", lty, next, lty, addr);
                Ok((cur, ty))
            }
            CExpr::CompoundLit(ty, init) => {
                let slot = self.next_tmp();
                let lty = self.llvm_ty(ty);
                let _ = writeln!(self.cur_allocas, "  {} = alloca {}", slot, lty);
                self.emit_local_init(&slot, ty, init, ir, scopes)?;
                if let CType::Array(elem, n) = ty {
                    let arr_lty = format!("[{} x {}]", n, self.llvm_ty(elem));
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 0",
                        gep, arr_lty, arr_lty, slot
                    );
                    return Ok((gep, CType::Ptr(elem.clone())));
                }
                let tmp = self.next_tmp();
                let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, lty, lty, slot);
                Ok((tmp, ty.clone()))
            }
            CExpr::Index(_, _) => {
                let (addr, ty) = self.emit_lvalue(expr, ir, scopes)?;
                if let CType::Array(elem, n) = &ty {
                    let arr_lty = format!("[{} x {}]", n, self.llvm_ty(elem));
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 0",
                        gep, arr_lty, arr_lty, addr
                    );
                    return Ok((gep, CType::Ptr(elem.clone())));
                }
                let tmp = self.next_tmp();
                let lty = self.llvm_ty(&ty);
                let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, lty, lty, addr);
                Ok((tmp, ty))
            }
            CExpr::Member(base, field, is_arrow) => {
                let (base_ptr, base_ty) = if *is_arrow {
                    let (pv, pty) = self.emit_expr(base, ir, scopes)?;
                    match pty {
                        CType::Ptr(inner) => (pv, *inner),
                        _ => return Err(format!("c_frontend: '->' requires pointer, got {:?}", pty)),
                    }
                } else {
                    self.emit_lvalue(base, ir, scopes)?
                };
                if let Some((sptr, bf)) = self.resolve_bitfield_in_agg(&base_ptr, &base_ty, field, ir) {
                    return Ok(self.emit_bitfield_load(&sptr, &bf, ir));
                }
                let (addr, ty) = self
                    .resolve_member_in_agg(&base_ptr, &base_ty, field, ir)
                    .ok_or_else(|| format!("c_frontend: {:?} has no field '{}'", base_ty, field))?;
                if let CType::Array(elem, n) = &ty {
                    let arr_lty = format!("[{} x {}]", n, self.llvm_ty(elem));
                    let gep = self.next_tmp();
                    let _ = writeln!(
                        ir,
                        "  {} = getelementptr inbounds {}, {}* {}, i64 0, i64 0",
                        gep, arr_lty, arr_lty, addr
                    );
                    return Ok((gep, CType::Ptr(elem.clone())));
                }
                let tmp = self.next_tmp();
                let lty = self.llvm_ty(&ty);
                let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, lty, lty, addr);
                Ok((tmp, ty))
            }
            CExpr::Assign(op, lhs_e, rhs_e) => {
                // Bitfield assignment support (`s.flags = 5`, `p->bits += 1`)
                if let CExpr::Member(base, field, is_arrow) = lhs_e.as_ref() {
                    let mut probe_ir = String::new();
                    let (base_ptr, base_ty) = if *is_arrow {
                        let (pv, pty) = self.emit_expr(base, &mut probe_ir, scopes)?;
                        match pty {
                            CType::Ptr(inner) => (pv, *inner),
                            _ => return Err(format!("c_frontend: '->' requires pointer, got {:?}", pty)),
                        }
                    } else {
                        self.emit_lvalue(base, &mut probe_ir, scopes)?
                    };
                    if let Some((sptr, bf)) = self.resolve_bitfield_in_agg(&base_ptr, &base_ty, field, &mut probe_ir) {
                        ir.push_str(&probe_ir);
                        let (rv, rty) = self.emit_expr(rhs_e, ir, scopes)?;
                        let (new_v, new_ty) = if *op == "=" {
                            (rv, rty)
                        } else {
                            let (cur_v, cur_ty) = self.emit_bitfield_load(&sptr, &bf, ir);
                            let bin_op = &op[..op.len() - 1];
                            self.emit_binary_vals(bin_op, &cur_v, &cur_ty, &rv, &rty, ir)?
                        };
                        return Ok(self.emit_bitfield_store(&sptr, &bf, &new_v, &new_ty, ir));
                    }
                }
                let (addr, lty_c) = self.emit_lvalue(lhs_e, ir, scopes)?;
                // Large struct/union copy assignment via `@memcpy`
                if *op == "=" && matches!(lty_c, CType::Struct(_) | CType::Union(_)) && self.sizeof_type(&lty_c) > 16 {
                    if let Ok((src_addr, _)) = self.emit_lvalue(rhs_e, ir, scopes) {
                        let sz = self.sizeof_type(&lty_c);
                        let lty = self.llvm_ty(&lty_c);
                        let dst_i8 = self.next_tmp();
                        let src_i8 = self.next_tmp();
                        let _ = writeln!(ir, "  {} = bitcast {}* {} to i8*", dst_i8, lty, addr);
                        let _ = writeln!(ir, "  {} = bitcast {}* {} to i8*", src_i8, lty, src_addr);
                        self.ensure_stdlib_decl(
                            "memcpy",
                            CType::Ptr(Box::new(CType::Void)),
                            vec![
                                CType::Ptr(Box::new(CType::Void)),
                                CType::Ptr(Box::new(CType::Void)),
                                CType::Int { bits: 64, signed: false },
                            ],
                        );
                        let _ = writeln!(ir, "  call i8* @memcpy(i8* {}, i8* {}, i64 {})", dst_i8, src_i8, sz);
                        return Ok(("0".to_string(), CType::Void));
                    }
                }
                let lty = self.llvm_ty(&lty_c);
                let new_val = if *op == "=" {
                    let (rv, rty) = self.emit_expr(rhs_e, ir, scopes)?;
                    self.emit_cast(&rv, &rty, &lty_c, ir)
                } else {
                    let cur = self.next_tmp();
                    let _ = writeln!(ir, "  {} = load {}, {}* {}", cur, lty, lty, addr);
                    let (rv, rty) = self.emit_expr(rhs_e, ir, scopes)?;
                    let bin_op = &op[..op.len() - 1];
                    let (res, res_ty) = self.emit_binary_vals(bin_op, &cur, &lty_c, &rv, &rty, ir)?;
                    self.emit_cast(&res, &res_ty, &lty_c, ir)
                };
                let _ = writeln!(ir, "  store {} {}, {}* {}", lty, new_val, lty, addr);
                Ok((new_val, lty_c))
            }
            CExpr::Ternary(cond, t_e, f_e) => {
                let (cv, cty) = self.emit_expr(cond, ir, scopes)?;
                let cbool = self.emit_to_i1(&cv, &cty, ir);
                let l_t = self.next_lbl("tern_t");
                let l_f = self.next_lbl("tern_f");
                let l_end = self.next_lbl("tern_end");
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", cbool, l_t, l_f);

                let mut t_ir = String::new();
                let (tv, tty) = self.emit_expr(t_e, &mut t_ir, scopes)?;
                let mut f_ir = String::new();
                let (fv, fty) = self.emit_expr(f_e, &mut f_ir, scopes)?;

                let common = self.common_type(&tty, &fty);
                let ctv = if matches!(common, CType::Void) {
                    tv
                } else {
                    self.emit_cast(&tv, &tty, &common, &mut t_ir)
                };
                let cfv = if matches!(common, CType::Void) {
                    fv
                } else {
                    self.emit_cast(&fv, &fty, &common, &mut f_ir)
                };

                let t_end_lbl = self.next_lbl("tern_te");
                let _ = writeln!(ir, "{}:", l_t);
                ir.push_str(&t_ir);
                let _ = writeln!(ir, "  br label %{}", t_end_lbl);
                let _ = writeln!(ir, "{}:", t_end_lbl);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let f_end_lbl = self.next_lbl("tern_fe");
                let _ = writeln!(ir, "{}:", l_f);
                ir.push_str(&f_ir);
                let _ = writeln!(ir, "  br label %{}", f_end_lbl);
                let _ = writeln!(ir, "{}:", f_end_lbl);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let _ = writeln!(ir, "{}:", l_end);
                if matches!(common, CType::Void) {
                    return Ok(("0".to_string(), CType::Void));
                }
                let phi = self.next_tmp();
                let _ = writeln!(
                    ir,
                    "  {} = phi {} [ {}, %{} ], [ {}, %{} ]",
                    phi,
                    self.llvm_ty(&common),
                    ctv,
                    t_end_lbl,
                    cfv,
                    f_end_lbl
                );
                Ok((phi, common))
            }
            CExpr::Binary(",", a, b) => {
                let _ = self.emit_expr(a, ir, scopes)?;
                self.emit_expr(b, ir, scopes)
            }
            CExpr::Binary("&&", a, b) => {
                let (av, aty) = self.emit_expr(a, ir, scopes)?;
                let ab = self.emit_to_i1(&av, &aty, ir);
                let l_rhs = self.next_lbl("and_rhs");
                let l_false = self.next_lbl("and_false");
                let l_end = self.next_lbl("and_end");
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", ab, l_rhs, l_false);

                let _ = writeln!(ir, "{}:", l_false);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let _ = writeln!(ir, "{}:", l_rhs);
                let (bv, bty) = self.emit_expr(b, ir, scopes)?;
                let bb = self.emit_to_i1(&bv, &bty, ir);
                let l_rhs_done = self.next_lbl("and_rd");
                let _ = writeln!(ir, "  br label %{}", l_rhs_done);
                let _ = writeln!(ir, "{}:", l_rhs_done);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let _ = writeln!(ir, "{}:", l_end);
                let phi = self.next_tmp();
                let _ = writeln!(ir, "  {} = phi i1 [ false, %{} ], [ {}, %{} ]", phi, l_false, bb, l_rhs_done);
                let res = self.next_tmp();
                let _ = writeln!(ir, "  {} = zext i1 {} to i32", res, phi);
                Ok((res, CType::Int { bits: 32, signed: true }))
            }
            CExpr::Binary("||", a, b) => {
                let (av, aty) = self.emit_expr(a, ir, scopes)?;
                let ab = self.emit_to_i1(&av, &aty, ir);
                let l_true = self.next_lbl("or_true");
                let l_rhs = self.next_lbl("or_rhs");
                let l_end = self.next_lbl("or_end");
                let _ = writeln!(ir, "  br i1 {}, label %{}, label %{}", ab, l_true, l_rhs);

                let _ = writeln!(ir, "{}:", l_true);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let _ = writeln!(ir, "{}:", l_rhs);
                let (bv, bty) = self.emit_expr(b, ir, scopes)?;
                let bb = self.emit_to_i1(&bv, &bty, ir);
                let l_rhs_done = self.next_lbl("or_rd");
                let _ = writeln!(ir, "  br label %{}", l_rhs_done);
                let _ = writeln!(ir, "{}:", l_rhs_done);
                let _ = writeln!(ir, "  br label %{}", l_end);

                let _ = writeln!(ir, "{}:", l_end);
                let phi = self.next_tmp();
                let _ = writeln!(ir, "  {} = phi i1 [ true, %{} ], [ {}, %{} ]", phi, l_true, bb, l_rhs_done);
                let res = self.next_tmp();
                let _ = writeln!(ir, "  {} = zext i1 {} to i32", res, phi);
                Ok((res, CType::Int { bits: 32, signed: true }))
            }
            CExpr::Binary(op, a, b) => {
                let (av, aty) = self.emit_expr(a, ir, scopes)?;
                let (bv, bty) = self.emit_expr(b, ir, scopes)?;
                self.emit_binary_vals(op, &av, &aty, &bv, &bty, ir)
            }
            CExpr::Call(callee, args) => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args {
                    eval_args.push(self.emit_expr(a, ir, scopes)?);
                }

                // Direct named function call vs indirect function pointer call
                let (callee_str, sig) = if let CExpr::Var(nm) = callee.as_ref() {
                    if let Some(slot) = self.lookup_var(nm, scopes) {
                        if let CType::FuncPtr { ret, params, variadic } = slot.ty.clone() {
                            let tmp = self.next_tmp();
                            let flty = self.llvm_ty(&slot.ty);
                            let _ = writeln!(ir, "  {} = load {}, {}* {}", tmp, flty, flty, slot.reg);
                            (
                                tmp,
                                CFuncSig {
                                    ret: *ret,
                                    params,
                                    variadic,
                                },
                            )
                        } else {
                            return Err(format!("c_frontend: '{}' is not callable", nm));
                        }
                    } else {
                        if !self.defined_fns.contains(nm) {
                            self.called_ext_fns.insert(nm.clone());
                        }
                        let sig = self.func_sigs.get(nm).cloned().unwrap_or(CFuncSig {
                            ret: CType::Int { bits: 64, signed: true },
                            params: eval_args.iter().map(|(_, t)| t.clone()).collect(),
                            variadic: false,
                        });
                        (format!("@{}", nm), sig)
                    }
                } else {
                    let (cv, cty) = self.emit_expr(callee, ir, scopes)?;
                    match cty {
                        CType::FuncPtr { ret, params, variadic } => (
                            cv,
                            CFuncSig {
                                ret: *ret,
                                params,
                                variadic,
                            },
                        ),
                        _ => return Err(format!("c_frontend: indirect callee is not a function pointer: {:?}", cty)),
                    }
                };

                let mut arg_strs = Vec::with_capacity(eval_args.len());
                for (idx, (av, aty)) in eval_args.into_iter().enumerate() {
                    if let Some(pty) = sig.params.get(idx) {
                        let cv = self.emit_cast(&av, &aty, pty, ir);
                        arg_strs.push(format!("{} {}", self.llvm_ty(pty), cv));
                    } else {
                        // C default argument promotions for variadic args (float -> double)
                        let pty = if matches!(aty, CType::Float) {
                            CType::Double
                        } else if aty.is_integer() && aty.int_bits() < 32 {
                            CType::Int { bits: 32, signed: true }
                        } else {
                            aty.clone()
                        };
                        let cv = self.emit_cast(&av, &aty, &pty, ir);
                        arg_strs.push(format!("{} {}", self.llvm_ty(&pty), cv));
                    }
                }

                let call_sig = if sig.variadic {
                    let mut ps: Vec<String> = sig.params.iter().map(|t| self.llvm_ty(t)).collect();
                    ps.push("...".to_string());
                    format!("{} ({})", self.llvm_ty(&sig.ret), ps.join(", "))
                } else {
                    self.llvm_ty(&sig.ret)
                };

                if matches!(sig.ret, CType::Void) {
                    let _ = writeln!(ir, "  call {} {}({})", call_sig, callee_str, arg_strs.join(", "));
                    Ok(("0".to_string(), CType::Void))
                } else {
                    let res = self.next_tmp();
                    let _ = writeln!(ir, "  {} = call {} {}({})", res, call_sig, callee_str, arg_strs.join(", "));
                    Ok((res, sig.ret))
                }
            }
            CExpr::Unary(other, _) => Err(format!("c_frontend: unsupported unary operator '{}'", other)),
        }
    }

    fn emit_step(&mut self, cur: &str, ty: &CType, is_inc: bool, ir: &mut String) -> String {
        let tmp = self.next_tmp();
        let lty = self.llvm_ty(ty);
        if let CType::Ptr(elem) = ty {
            let elty = if matches!(elem.as_ref(), CType::Void) {
                "i8".to_string()
            } else {
                self.llvm_ty(elem)
            };
            let delta = if is_inc { 1 } else { -1 };
            let _ = writeln!(ir, "  {} = getelementptr inbounds {}, {}* {}, i64 {}", tmp, elty, elty, cur, delta);
        } else if ty.is_float() {
            let op = if is_inc { "fadd" } else { "fsub" };
            let _ = writeln!(ir, "  {} = {} {} {}, 1.0", tmp, op, lty, cur);
        } else {
            let op = if is_inc { "add" } else { "sub" };
            let _ = writeln!(ir, "  {} = {} {} {}, 1", tmp, op, lty, cur);
        }
        tmp
    }

    fn emit_binary_vals(
        &mut self,
        op: &str,
        av: &str,
        aty: &CType,
        bv: &str,
        bty: &CType,
        ir: &mut String,
    ) -> Result<(String, CType), String> {
        // Pointer arithmetic: `ptr + int`, `int + ptr`, `ptr - int`, `ptr - ptr`
        if matches!(op, "+" | "-") && (aty.is_ptr() || bty.is_ptr()) {
            if let CType::Ptr(elem) = aty {
                if bty.is_integer() {
                    let idx64 = self.emit_cast(bv, bty, &CType::Int { bits: 64, signed: true }, ir);
                    let eff_idx = if op == "-" {
                        let neg = self.next_tmp();
                        let _ = writeln!(ir, "  {} = sub i64 0, {}", neg, idx64);
                        neg
                    } else {
                        idx64
                    };
                    let elty = if matches!(elem.as_ref(), CType::Void) {
                        "i8".to_string()
                    } else {
                        self.llvm_ty(elem)
                    };
                    let gep = self.next_tmp();
                    let _ = writeln!(ir, "  {} = getelementptr inbounds {}, {}* {}, i64 {}", gep, elty, elty, av, eff_idx);
                    return Ok((gep, aty.clone()));
                } else if op == "-" && bty.is_ptr() {
                    let a_i = self.next_tmp();
                    let b_i = self.next_tmp();
                    let _ = writeln!(ir, "  {} = ptrtoint {} {} to i64", a_i, self.llvm_ty(aty), av);
                    let _ = writeln!(ir, "  {} = ptrtoint {} {} to i64", b_i, self.llvm_ty(bty), bv);
                    let diff = self.next_tmp();
                    let _ = writeln!(ir, "  {} = sub i64 {}, {}", diff, a_i, b_i);
                    let esz = self.sizeof_type(elem).max(1);
                    if esz > 1 {
                        let q = self.next_tmp();
                        let _ = writeln!(ir, "  {} = sdiv i64 {}, {}", q, diff, esz);
                        return Ok((q, CType::Int { bits: 64, signed: true }));
                    }
                    return Ok((diff, CType::Int { bits: 64, signed: true }));
                }
            } else if op == "+" {
                if let CType::Ptr(elem) = bty {
                    let idx64 = self.emit_cast(av, aty, &CType::Int { bits: 64, signed: true }, ir);
                    let elty = if matches!(elem.as_ref(), CType::Void) {
                        "i8".to_string()
                    } else {
                        self.llvm_ty(elem)
                    };
                    let gep = self.next_tmp();
                    let _ = writeln!(ir, "  {} = getelementptr inbounds {}, {}* {}, i64 {}", gep, elty, elty, bv, idx64);
                    return Ok((gep, bty.clone()));
                }
            }
        }

        // Comparisons
        if matches!(op, "==" | "!=" | "<" | "<=" | ">" | ">=") {
            let common = if aty.is_ptr() || bty.is_ptr() {
                CType::Int { bits: 64, signed: false }
            } else {
                self.common_type(aty, bty)
            };
            let ca = self.emit_cast(av, aty, &common, ir);
            let cb = self.emit_cast(bv, bty, &common, ir);
            let lty = self.llvm_ty(&common);
            let cmp = self.next_tmp();
            if common.is_float() {
                let pred = match op {
                    "==" => "oeq",
                    "!=" => "une",
                    "<" => "olt",
                    "<=" => "ole",
                    ">" => "ogt",
                    ">=" => "oge",
                    _ => unreachable!(),
                };
                let _ = writeln!(ir, "  {} = fcmp {} {} {}, {}", cmp, pred, lty, ca, cb);
            } else {
                let pred = match (op, common.is_signed()) {
                    ("==", _) => "eq",
                    ("!=", _) => "ne",
                    ("<", true) => "slt",
                    ("<=", true) => "sle",
                    (">", true) => "sgt",
                    (">=", true) => "sge",
                    ("<", false) => "ult",
                    ("<=", false) => "ule",
                    (">", false) => "ugt",
                    (">=", false) => "uge",
                    _ => unreachable!(),
                };
                let _ = writeln!(ir, "  {} = icmp {} {} {}, {}", cmp, pred, lty, ca, cb);
            }
            let res = self.next_tmp();
            let _ = writeln!(ir, "  {} = zext i1 {} to i32", res, cmp);
            return Ok((res, CType::Int { bits: 32, signed: true }));
        }

        // Arithmetic & bitwise
        let common = self.common_type(aty, bty);
        let ca = self.emit_cast(av, aty, &common, ir);
        let cb = self.emit_cast(bv, bty, &common, ir);
        let lty = self.llvm_ty(&common);
        let res = self.next_tmp();
        if common.is_float() {
            let inst = match op {
                "+" => "fadd",
                "-" => "fsub",
                "*" => "fmul",
                "/" => "fdiv",
                "%" => "frem",
                _ => return Err(format!("c_frontend: invalid float op '{}'", op)),
            };
            let _ = writeln!(ir, "  {} = {} {} {}, {}", res, inst, lty, ca, cb);
        } else {
            let inst = match (op, common.is_signed()) {
                ("+", _) => "add",
                ("-", _) => "sub",
                ("*", _) => "mul",
                ("/", true) => "sdiv",
                ("/", false) => "udiv",
                ("%", true) => "srem",
                ("%", false) => "urem",
                ("&", _) => "and",
                ("|", _) => "or",
                ("^", _) => "xor",
                ("<<", _) => "shl",
                (">>", true) => "ashr",
                (">>", false) => "lshr",
                _ => return Err(format!("c_frontend: invalid integer op '{}'", op)),
            };
            let _ = writeln!(ir, "  {} = {} {} {}, {}", res, inst, lty, ca, cb);
        }
        Ok((res, common))
    }

    fn promote_int(&self, ty: &CType) -> CType {
        if ty.is_integer() && ty.int_bits() < 32 {
            CType::Int { bits: 32, signed: true }
        } else {
            ty.clone()
        }
    }

    fn common_type(&self, a: &CType, b: &CType) -> CType {
        if matches!(a, CType::Double) || matches!(b, CType::Double) {
            return CType::Double;
        }
        if matches!(a, CType::Float) || matches!(b, CType::Float) {
            return CType::Float;
        }
        if a.is_ptr() {
            return a.clone();
        }
        if b.is_ptr() {
            return b.clone();
        }
        let pa = self.promote_int(a);
        let pb = self.promote_int(b);
        let bits = pa.int_bits().max(pb.int_bits()).max(32);
        let signed = if pa.int_bits() > pb.int_bits() {
            pa.is_signed()
        } else if pb.int_bits() > pa.int_bits() {
            pb.is_signed()
        } else {
            pa.is_signed() && pb.is_signed()
        };
        CType::Int { bits, signed }
    }

    fn emit_to_i1(&mut self, val: &str, ty: &CType, ir: &mut String) -> String {
        let tmp = self.next_tmp();
        let lty = self.llvm_ty(ty);
        if ty.is_float() {
            let _ = writeln!(ir, "  {} = fcmp one {} {}, 0.0", tmp, lty, val);
        } else if ty.is_ptr() {
            let _ = writeln!(ir, "  {} = icmp ne {} {}, null", tmp, lty, val);
        } else {
            let _ = writeln!(ir, "  {} = icmp ne {} {}, 0", tmp, lty, val);
        }
        tmp
    }

    fn emit_cast(&mut self, val: &str, from: &CType, to: &CType, ir: &mut String) -> String {
        let f_lty = self.llvm_ty(from);
        let t_lty = self.llvm_ty(to);
        if f_lty == t_lty {
            return val.to_string();
        }
        if matches!(to, CType::Bool) {
            let b = self.emit_to_i1(val, from, ir);
            let z = self.next_tmp();
            let _ = writeln!(ir, "  {} = zext i1 {} to i8", z, b);
            return z;
        }
        let tmp = self.next_tmp();
        match (from, to) {
            (CType::Int { bits: fb, signed: fs }, CType::Int { bits: tb, .. }) => {
                if tb > fb {
                    let op = if *fs { "sext" } else { "zext" };
                    let _ = writeln!(ir, "  {} = {} {} {} to {}", tmp, op, f_lty, val, t_lty);
                } else {
                    let _ = writeln!(ir, "  {} = trunc {} {} to {}", tmp, f_lty, val, t_lty);
                }
            }
            (CType::Bool, CType::Int { .. }) => {
                let _ = writeln!(ir, "  {} = zext i8 {} to {}", tmp, val, t_lty);
            }
            (CType::Int { signed: fs, .. }, CType::Float | CType::Double) => {
                let op = if *fs { "sitofp" } else { "uitofp" };
                let _ = writeln!(ir, "  {} = {} {} {} to {}", tmp, op, f_lty, val, t_lty);
            }
            (CType::Bool, CType::Float | CType::Double) => {
                let _ = writeln!(ir, "  {} = uitofp i8 {} to {}", tmp, val, t_lty);
            }
            (CType::Float | CType::Double, CType::Int { signed: ts, .. }) => {
                let op = if *ts { "fptosi" } else { "fptoui" };
                let _ = writeln!(ir, "  {} = {} {} {} to {}", tmp, op, f_lty, val, t_lty);
            }
            (CType::Float, CType::Double) => {
                let _ = writeln!(ir, "  {} = fpext float {} to double", tmp, val);
            }
            (CType::Double, CType::Float) => {
                let _ = writeln!(ir, "  {} = fptrunc double {} to float", tmp, val);
            }
            (CType::Ptr(_) | CType::FuncPtr { .. }, CType::Ptr(_) | CType::FuncPtr { .. }) => {
                let _ = writeln!(ir, "  {} = bitcast {} {} to {}", tmp, f_lty, val, t_lty);
            }
            (CType::Ptr(_) | CType::FuncPtr { .. }, CType::Int { .. }) => {
                let _ = writeln!(ir, "  {} = ptrtoint {} {} to {}", tmp, f_lty, val, t_lty);
            }
            (CType::Int { .. } | CType::Bool, CType::Ptr(_) | CType::FuncPtr { .. }) => {
                let _ = writeln!(ir, "  {} = inttoptr {} {} to {}", tmp, f_lty, val, t_lty);
            }
            _ => return val.to_string(),
        }
        tmp
    }
}

fn format_llvm_float(v: f64, is_f32: bool) -> String {
    let f = if is_f32 { (v as f32) as f64 } else { v };
    if f.is_finite() {
        // Exact IEEE-754 hex double literal accepted by both LLVM and Cranelift IR parsers
        format!("0x{:016X}", f.to_bits())
    } else {
        "0.0".to_string()
    }
}

// ============================================================================
// Unit Tests (100% Zero-Clang!)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_rust_c99_frontend_zero_clang_comprehensive() {
        let c_src = r#"
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#define ADD_SCALED(a, b) ((a) * 10 + (b))
#define MODE 2

#if MODE == 2 && defined(ADD_SCALED)
#define BONUS 7
#else
#define BONUS 999
#endif

typedef struct Vec2 {
    int32_t x;
    int32_t y;
} Vec2;

typedef union Bits32 {
    int32_t i;
    uint32_t u;
} Bits32;

enum Kind { K_A = 1, K_B = 5, K_C };

static int g_bias = 3;

static int apply_op(int (*fn)(int, int), int a, int b) {
    return fn(a, b);
}

static int mul_pair(int a, int b) {
    return a * b;
}

int c99_suite_score(int n) {
    // 1. Preprocessor + enum + global
    int acc = ADD_SCALED(2, BONUS) + K_C + g_bias; // 27 + 6 + 3 = 36

    // 2. Structs, pointers, `->`, `&`, sizeof
    Vec2 v = { .x = 4, .y = 9 };
    Vec2 *vp = &v;
    vp->x += 2; // 6
    acc += vp->x * vp->y + (int)sizeof(Vec2); // 36 + 54 + 8 = 98

    // 3. Union punning
    Bits32 bu;
    bu.u = 0x10;
    acc += bu.i; // 98 + 16 = 114

    // 4. Local array + pointer arithmetic + for/do-while
    int arr[4] = { 1, 2, 3, 4 };
    int *p = arr;
    for (int i = 0; i < n && i < 4; i++) {
        acc += *(p + i);
    }
    // n=4 -> +10 => 124

    // 5. Switch with C fallthrough
    int sw = 0;
    switch (n) {
        case 4:
            sw += 5;
        case 3:
            sw += 6;
            break;
        default:
            sw = -1;
            break;
    }
    acc += sw; // 124 + 11 = 135

    // 6. Function pointer + ternary + static local
    static int calls = 0;
    calls++;
    int fp = apply_op(&mul_pair, 3, 5); // 15
    acc += (fp > 10 ? fp : 0) + calls; // 135 + 15 + 1 = 151

    // 7. Double arithmetic + cast
    double d = 2.5 * 4.0;
    acc += (int)d; // 151 + 10 = 161

    return acc;
}
"#;
        let ll = compile_c_source_str_to_ll(c_src, None, &[], "x86_64-unknown-linux-gnu")
            .expect("Pure-Rust C99 frontend should compile C99 source without clang");

        assert!(ll.contains("define i32 @c99_suite_score(i32 %arg_0)"));
        assert!(ll.contains("%struct.Vec2 = type { i32, i32 }"));
        assert!(ll.contains("%union.Bits32 = type { [4 x i8] }"));
        assert!(ll.contains("switch i64"));
        assert!(ll.contains("fmul double"));
    }
}
