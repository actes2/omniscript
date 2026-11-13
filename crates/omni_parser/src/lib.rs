use omni_mem::{MemoryNormalizer, UnifiedMemOp};
pub use omni_preamble::ImportDirective;
use regex::Regex;
use serde::{Deserialize, Serialize};

pub mod diagnostic;
pub use diagnostic::*;

// Paired block-comment delimiters `(open, close, allow_nesting)` across supported ecosystems:
// HTML/XML, Jinja/Twig, PowerShell, Lisp/Scheme/Racket, Julia, Haskell/Elm, OCaml/Pascal/ML, C-family/Rust, CoffeeScript.
const POLYGLOT_BLOCK_COMMENTS: &[(&str, &str, bool)] = &[
    ("<!--", "-->", false),
    ("{#", "#}", false),
    ("<#", "#>", false),
    ("#|", "|#", true),
    ("#=", "=#", true),
    ("{-", "-}", true),
    ("(*", "*)", true),
    ("/*", "*/", true),
    ("###", "###", false),
];

// Line-anchored block-comment pairs `(start_prefix, end_prefix)` matched at the beginning of trimmed lines
// (Ruby `=begin`/`=end`, Perl POD `=pod`/`=head`/`=cut`, MATLAB/Octave `%{`/`%}`).
const LINE_ANCHORED_BLOCK_COMMENTS: &[(&[&str], &str)] = &[
    (&["=begin"], "=end"),
    (&["=pod", "=head1", "=head2", "=head3", "=head4", "=over"], "=cut"),
];

// Statement prefixes from brace/semicolon languages that never use `//` as Python floor division.
const NON_FLOOR_DIV_PREFIXES: &[&str] = &[
    "let ", "const ", "var ", "int ", "int32 ", "int64 ", "long ", "float ", "double ",
    "bool ", "boolean ", "void ", "auto ", "char ", "char*", "public ", "private ",
    "protected ", "internal ", "static ", "final ", "sealed ", "fn ", "func ",
    "function ", "package ", "use ", "struct ", "impl ", "interface ", "enum ",
];

// Valid Python binary/clause keywords permitted between adjacent identifiers after `//` floor division.
const PYTHON_EXPR_KEYWORDS: &[&str] = &[
    "in", "not", "is", "and", "or", "if", "else", "for",
];

// Standard comment prose markers that immediately classify `//` as a comment rather than division.
const COMMENT_PROSE_MARKERS: &[&str] = &[
    "todo", "fixme", "note:", "hack", "xxx", "bug:",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniModule {
    pub name: String,
    pub imports: Vec<ImportDirective>,
    pub structs_and_classes: Vec<ClassDef>,
    pub enums: Vec<EnumDef>,
    pub functions: Vec<FunctionDef>,
    pub top_level_statements: Vec<Statement>,
    pub base_dir: Option<std::path::PathBuf>,
    pub diagnostics: Vec<OmniDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub tag: usize,
    pub payload_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: String,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDef {
    pub name: String,
    pub decorators: Vec<String>,
    pub base_classes: Vec<String>,
    pub fields: Vec<FieldDef>,
    pub methods: Vec<FunctionDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub type_hint: Option<String>,
    pub default_val: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationDef {
    /// Raw polyglot decorator/annotation syntax (e.g. `@app.route("/")`, `#[inline(always)]`, `@GetMapping("/api")`)
    pub raw_syntax: String,
    /// Target callable or attribute path (e.g. `app.route`, `inline`, `GetMapping`)
    pub target_expr: String,
    /// Parsed argument expressions passed to the annotation
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub is_method: bool,
    pub annotations: Vec<AnnotationDef>,
    pub params: Vec<ParamDef>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
    pub memory_ops: Vec<UnifiedMemOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub type_hint: Option<String>,
    pub default_val: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdiomFamily {
    Standard,
    GoScopedDeclaration,
    PythonWalrus,
    ClosureLambda,
    PatternArm,
    Unknown(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatchClause {
    pub exception_type: Option<String>,
    pub var_name: Option<String>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Statement {
    /// Tuple unpacking: `a, b = expr` or `(a, b) := expr`
    TupleUnpack {
        targets: Vec<String>,
        expr: String,
        is_scoped_decl: bool,
    },
    /// Polyglot Try / Catch / Except / Finally statement
    TryCatch {
        try_block: Vec<Statement>,
        catch_clauses: Vec<CatchClause>,
        finally_block: Option<Vec<Statement>>,
    },
    /// Variable assignment or scoped declaration: `x = 5`, `int x = 0`, `count := 10`
    Assign {
        target: String,
        expr: String,
        idiom: IdiomFamily,
        is_scoped_decl: bool,
    },
    /// If statement with optional else
    If {
        condition: String,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
    /// While loop: `while cond:` or `while (cond) {`
    WhileLoop {
        condition: String,
        body: Vec<Statement>,
    },
    /// For loop: `for var in count:` or `for (int i = 0; i < N; i++) {`
    ForLoop {
        var: String,
        count: String,
        body: Vec<Statement>,
    },
    /// Print: `print(expr)`
    Print {
        expr: String,
    },
    /// Return statement: `return` or `return expr`
    Return {
        expr: Option<String>,
    },
    /// Pattern matching: `match expr { Variant => stmt, ... }`
    Match {
        target: String,
        arms: Vec<MatchArm>,
    },
    /// Explicit Unified Memory Operation embedded in statement sequence
    MemOp {
        op: UnifiedMemOp,
        is_deferred: bool,
    },
    /// Channel creation: `ch := make(chan int, 8)` or `ch = Channel(8)`
    ChanMake {
        target: String,
        capacity: usize,
        elem_size: usize,
        is_scoped_decl: bool,
    },
    /// Channel send: `ch <- expr` or `ch.send(expr)`
    ChanSend {
        channel: String,
        expr: String,
    },
    /// Channel receive: `val := <-ch` or `val = <-ch` or `<-ch`
    ChanRecv {
        target: Option<String>,
        channel: String,
        is_scoped_decl: bool,
    },
    /// Native Pythonic context manager: `with Expr() as var:` or `with Expr():`
    WithContext {
        context_expr: String,
        var_alias: Option<String>,
        body: Vec<Statement>,
    },
    /// Polyglot List / Slice creation: `items = [10, 20, 30]`, `items := []`
    ListInit {
        target: String,
        elements: Vec<String>,
        is_scoped_decl: bool,
    },
    /// Polyglot List append: `items.append(40)`, `items.push(40)`
    ListAppend {
        target: String,
        expr: String,
    },
    /// Polyglot index assignment: `items[0] = 99`, `m["key"] = 10`
    IndexSet {
        target: String,
        index_expr: String,
        val_expr: String,
    },
    /// Polyglot Map / Dict creation: `m = {"a": 10, "b": 20}`, `m := {}`
    MapInit {
        target: String,
        entries: Vec<(String, String)>,
        is_scoped_decl: bool,
    },
    /// Abstract intent block awaiting model synthesis
    Intent {
        instruction: String,
    },
    /// Asynchronous Fiber / Goroutine spawn: `go <call_expr>` or `go func(...)`
    GoSpawn {
        call_expr: String,
        idiom: IdiomFamily,
    },
    /// Source-anchored polyglot interpretive block with physical source grounding
    InterpretiveAnchor {
        anchor_id: usize,
        file: String,
        line_start: usize,
        line_end: usize,
        physical_source: String,
        idiom: IdiomFamily,
    },
    /// First-class polyglot annotation / decorator binding applied to a function or class symbol
    AnnotationBinding {
        target_symbol: String,
        annotation: AnnotationDef,
    },
    /// Fallback concrete or polyglot statement
    Concrete {
        code: String,
        idiom: IdiomFamily,
    },
}

pub fn harvest_called_symbols(stmts: &[Statement], set: &mut std::collections::HashSet<String>) {
    for stmt in stmts {
        match stmt {
            Statement::Assign { expr, .. } => {
                for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::If { condition, then_branch, else_branch } => {
                for word in condition.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                harvest_called_symbols(then_branch, set);
                if let Some(eb) = else_branch {
                    harvest_called_symbols(eb, set);
                }
            }
            Statement::WhileLoop { condition, body } => {
                for word in condition.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                harvest_called_symbols(body, set);
            }
            Statement::ForLoop { count, body, .. } => {
                for word in count.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                harvest_called_symbols(body, set);
            }
            Statement::WithContext { context_expr, body, .. } => {
                set.insert("__enter__".to_string());
                set.insert("__exit__".to_string());
                for word in context_expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                harvest_called_symbols(body, set);
            }
            Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                harvest_called_symbols(try_block, set);
                for c in catch_clauses {
                    harvest_called_symbols(&c.body, set);
                }
                if let Some(ref fb) = finally_block {
                    harvest_called_symbols(fb, set);
                }
            }
            Statement::Print { expr } => {
                for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::Return { expr } => {
                if let Some(e) = expr {
                    for word in e.split(|c: char| !c.is_alphanumeric() && c != '_') {
                        if !word.is_empty() { set.insert(word.to_string()); }
                    }
                }
            }
            Statement::TupleUnpack { expr, .. } => {
                for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::Concrete { code, .. } => {
                for word in code.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::ListInit { elements, .. } => {
                for el in elements {
                    for word in el.split(|c: char| !c.is_alphanumeric() && c != '_') {
                        if !word.is_empty() { set.insert(word.to_string()); }
                    }
                }
            }
            Statement::ListAppend { target, expr } => {
                for word in target.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                set.insert("add".to_string());
                set.insert("append".to_string());
                set.insert("push".to_string());
            }
            Statement::MapInit { entries, .. } => {
                for (_, v) in entries {
                    for word in v.split(|c: char| !c.is_alphanumeric() && c != '_') {
                        if !word.is_empty() { set.insert(word.to_string()); }
                    }
                }
            }
            Statement::IndexSet { index_expr, val_expr, .. } => {
                for word in index_expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                for word in val_expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::GoSpawn { call_expr, .. } => {
                for word in call_expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            Statement::ChanSend { channel, expr } => {
                for word in channel.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
                for word in expr.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if !word.is_empty() { set.insert(word.to_string()); }
                }
            }
            _ => {}
        }
    }
}

pub struct OmniParser {
    fn_re: Regex,
    if_re: Regex,
    elif_re: Regex,
    else_re: Regex,
    while_re: Regex,
    plain_for_re: Regex,
    paren_for_re: Regex,
    c_for_re: Regex,
    print_re: Regex,
    return_re: Regex,
    with_re: Regex,
    walrus_assign_re: Regex,
    assign_re: Regex,
    make_chan_re: Regex,
    chan_send_re: Regex,
    chan_recv_re: Regex,
    list_init_re: Regex,
    list_append_re: Regex,
    index_set_re: Regex,
    map_init_re: Regex,
}

impl Default for OmniParser {
    fn default() -> Self {
        Self::new()
    }
}

impl OmniParser {
    pub fn clean_param_name(p: &str) -> String {
        let p_trimmed = p.trim().trim_start_matches('&').trim_start_matches("mut ").trim();
        if p_trimmed == "self" {
            return "self".to_string();
        }
        if let Some(pos) = p_trimmed.find(':') {
            return p_trimmed[..pos]
                .trim()
                .split_whitespace()
                .last()
                .unwrap_or(p_trimmed)
                .to_string();
        }
        let parts: Vec<&str> = p_trimmed.split_whitespace().collect();
        if parts.len() <= 1 {
            return p_trimmed.to_string();
        }
        let known_types = [
            "int", "int32", "int64", "long", "float", "double", "bool", "boolean",
            "string", "String", "i32", "i64", "f32", "f64", "u32", "u64", "usize",
            "isize", "byte", "char*", "auto",
        ];
        if known_types.contains(&parts[0]) || parts[0].ends_with("[]") {
            return parts.last().unwrap_or(&parts[0]).to_string();
        }
        if known_types.contains(&parts[parts.len() - 1]) {
            return parts[0].to_string();
        }
        parts.last().unwrap_or(&parts[0]).to_string()
    }

    pub fn parse_param(p: &str) -> ParamDef {
        let (p_no_default, default_val) = if let Some(eq_idx) = p.find('=') {
            (&p[..eq_idx], Some(p[eq_idx + 1..].trim().to_string()))
        } else {
            (p, None)
        };
        let p_trimmed = p_no_default.trim().trim_start_matches('&').trim_start_matches("mut ").trim();
        if p_trimmed == "self" || p_trimmed == "this" {
            return ParamDef {
                name: p_trimmed.to_string(),
                type_hint: Some(format!("&{}", p_trimmed)),
                default_val: None,
            };
        }
        if let Some(pos) = p_trimmed.find(':') {
            let raw_name = p_trimmed[..pos]
                .trim()
                .split_whitespace()
                .last()
                .unwrap_or(p_trimmed);
            let name: String = raw_name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
            let hint = p_trimmed[pos + 1..].trim_start_matches(':').trim().to_string();
            return ParamDef {
                name: if name.is_empty() { "arg".to_string() } else { name },
                type_hint: Some(hint),
                default_val,
            };
        }
        let parts: Vec<&str> = p_trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            let known_types = [
                "int", "int32", "int64", "long", "float", "double", "bool", "boolean",
                "string", "String", "i32", "i64", "f32", "f64", "u32", "u64", "usize",
                "isize", "byte", "char*", "auto", "void*",
            ];
            let last = parts[parts.len() - 1];
            let first = parts[0];
            let last_is_type = known_types.contains(&last)
                || last.starts_with('*')
                || last.starts_with("[]")
                || last.starts_with('&');
            let first_is_type = known_types.contains(&first)
                || first.ends_with('*')
                || first.ends_with("[]")
                || first.ends_with('&');

            if last_is_type && !first_is_type {
                // Go style: `base int`, `ch chan int`, `items []string`, `node *Node`
                let name: String = first.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                let go_type = parts[1..].join(" ");
                return ParamDef {
                    name: if name.is_empty() { "arg".to_string() } else { name },
                    type_hint: Some(go_type),
                    default_val,
                };
            }
            // C/Java style: `String username`, `int base`, `const char* s`
            let raw_name = parts[parts.len() - 1];
            let name: String = raw_name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
            let type_str = parts[..parts.len() - 1].join(" ");
            return ParamDef {
                name: if name.is_empty() { "arg".to_string() } else { name },
                type_hint: Some(type_str),
                default_val,
            };
        }
        let cleaned = Self::clean_param_name(p_trimmed);
        let name: String = cleaned.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
        ParamDef {
            name: if name.is_empty() { "arg".to_string() } else { name },
            type_hint: None,
            default_val,
        }
    }

    pub fn extract_fn_return_type(header_line: &str, fn_name: &str) -> Option<String> {
        let clean = Self::strip_trailing_comment(header_line).trim();
        let name_idx = clean.find(fn_name)?;
        let prefix = clean[..name_idx].trim();

        // Check after balanced `(params)` first (Python/Rust `-> T`, Go `(T, error)` / `int64`, TS `: number`)
        let after_name = &clean[name_idx + fn_name.len()..];
        if let Some(open_p) = after_name.find('(') {
            let mut depth = 0i32;
            let mut in_quote: Option<char> = None;
            let mut close_idx = None;
            for (i, ch) in after_name[open_p..].char_indices() {
                if let Some(q) = in_quote {
                    if ch == q {
                        in_quote = None;
                    }
                    continue;
                }
                if ch == '"' || ch == '\'' {
                    in_quote = Some(ch);
                } else if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(open_p + i);
                        break;
                    }
                }
            }
            if let Some(ci) = close_idx {
                let mut suffix = after_name[ci + 1..].trim();
                while suffix.ends_with('{') || suffix.ends_with(':') || suffix.ends_with(';') || suffix.ends_with('=') {
                    suffix = suffix[..suffix.len() - 1].trim();
                }
                if let Some(arrow_rest) = suffix.strip_prefix("->") {
                    let rt = arrow_rest.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                    if !rt.is_empty() {
                        return Some(rt.to_string());
                    }
                } else if let Some(colon_rest) = suffix.strip_prefix("::").or_else(|| suffix.strip_prefix(':')) {
                    let rt = colon_rest.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                    if !rt.is_empty() {
                        return Some(rt.to_string());
                    }
                } else if !suffix.is_empty()
                    && !suffix.starts_with("throws ")
                    && !suffix.starts_with("where ")
                {
                    return Some(suffix.to_string());
                }
            }
        }

        // Check C / Java / C# leading return type before `fn_name`
        let mut p_tokens: Vec<&str> = prefix
            .split_whitespace()
            .filter(|t| {
                !matches!(
                    *t,
                    "public"
                        | "private"
                        | "protected"
                        | "internal"
                        | "pub"
                        | "abstract"
                        | "virtual"
                        | "override"
                        | "inline"
                        | "sealed"
                        | "static"
                        | "async"
                        | "final"
                        | "local"
                        | "export"
                )
            })
            .collect();
        if let Some(&first) = p_tokens.first() {
            if !matches!(first, "def" | "fn" | "func" | "function" | "fun" | "proc") {
                let joined = p_tokens.drain(..).collect::<Vec<_>>().join(" ");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
        }

        None
    }

    pub fn unwrap_outer_balanced_parens(raw: &str) -> String {
        let mut s = raw.trim();
        loop {
            if !(s.starts_with('(') && s.ends_with(')')) {
                return s.to_string();
            }
            let mut depth = 0i32;
            let mut in_quote: Option<char> = None;
            let mut esc = false;
            let mut wraps_entire = true;
            let chars: Vec<char> = s.chars().collect();
            for (idx, &c) in chars.iter().enumerate() {
                if let Some(q) = in_quote {
                    if esc {
                        esc = false;
                    } else if c == '\\' {
                        esc = true;
                    } else if c == q {
                        in_quote = None;
                    }
                    continue;
                }
                if c == '"' || c == '\'' {
                    in_quote = Some(c);
                    continue;
                }
                if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 && idx + 1 < chars.len() {
                        wraps_entire = false;
                        break;
                    }
                }
            }
            if wraps_entire && depth == 0 {
                s = s[1..s.len() - 1].trim();
            } else {
                return s.to_string();
            }
        }
    }

    pub fn new() -> Self {
        Self {
            // Polyglot function pattern covering Python, Rust, Go, Kotlin, Swift, Julia, Nim, Ruby, Zig, Dart, JS, Lua, Java, C#, C/C++
            fn_re: Regex::new(
                r#"^(?:(?:(?:public|private|protected|internal|pub|abstract|virtual|override|inline|sealed|open|suspend)\s+)*(?:static\s+)?(?:async\s+)?(?:def|fn|func|fun|proc|function|(?:(?:[A-Z][a-zA-Z0-9_<>*&\[\]:]*|int|int32|int64|i32|i64|u32|u64|long|float|float32|float64|f32|f64|double|bool|boolean|void|char\*|auto|string|String|List\[.*?\])\s+))|(?:local\s+)?function\s+)\s*([a-zA-Z_0-9]+)\s*(?:<[^>]*>)?\s*\((.*?)\)\s*(?:->\s*(?:"[^"]*"|'[^']*'|\([^)]*\)|[a-zA-Z_0-9*&\[\]:,"' .<>]+)|::?\s*[a-zA-Z_0-9*&\[\]:.<>]+\s*|\([^)]*\)\s*|[a-zA-Z_0-9*&\[\]:.<>]+\s*)?(?::|\{|\;|\=)?$"#,
            )
            .unwrap(),
            // Matches if (x > 10) {, if x > 10 {, if (a) and (b):
            if_re: Regex::new(r#"^if\b\s*(.*?)\s*(?::|\{)?$"#).unwrap(),
            // Matches elif x == 5:, elsif x == 5, else if (x == 5) {, else if x == 5 {
            elif_re: Regex::new(r#"^(?:elif|elsif|else\s+if)\b\s*(.*?)\s*(?::|\{)?$"#).unwrap(),
            // Matches else:, else {
            else_re: Regex::new(r#"^else\s*(?::|\{)?$"#).unwrap(),
            // Matches while (x > 0) {, while x > 0 {, while (f(x)) != 0:
            while_re: Regex::new(r#"^while\b\s*(.*?)\s*(?::|\{)?$"#).unwrap(),
            // Matches for i in len(items):, for i in range(10):, for i in 10:
            plain_for_re: Regex::new(
                r#"^for\s+(?:(?:int|let|var|val)\s+)?([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*(.+?)\s*\)|(.+?))\s*(?::|\{)?$"#,
            )
            .unwrap(),
            // Matches for (i in 10), for (i in items)
            paren_for_re: Regex::new(
                r#"^for\s*\(\s*(?:(?:int|let|var|val)\s+)?([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*(.+?)\s*\)|(.+?))\s*\)\s*(?::|\{)?$"#,
            )
            .unwrap(),
            // Matches for (int i = 0; i < 10; i++) {
            c_for_re: Regex::new(
                r#"^for\s*\(\s*(?:(?:int|let|var|val)\s+)?([a-zA-Z_0-9]+)\s*=\s*0\s*;\s*[a-zA-Z_0-9]+\s*<\s*(.+?)\s*;\s*[^)]*\)\s*(?::|\{)?$"#,
            )
            .unwrap(),
            print_re: Regex::new(
                r#"^(?:print|println!?|WriteLine|Console\.WriteLine|console\.log|fmt\.Println|puts|printf|System\.out\.println|System\.out\.print)\s*\(\s*(.*)\s*\)\s*;?$"#,
            )
            .unwrap(),
            return_re: Regex::new(r#"^return(?:\s+(.*?))?\s*;?$"#).unwrap(),
            // Matches with Resource() as r:, with Mutex():
            with_re: Regex::new(r#"^with\s+(.*?)(?:\s+as\s+([a-zA-Z_0-9]+))?\s*(?::|\{)?$"#).unwrap(),
            // Go scoped declaration: count := 0
            walrus_assign_re: Regex::new(r#"^([a-zA-Z_0-9]+)\s*:=\s*(.+?)\s*;?$"#).unwrap(),
            // Matches x = 5, int x = 0, let count = 10, let mut acc = 20, let total: i64 = 0, self.val = 20, var y = 2, User u = new User(...)
            assign_re: Regex::new(
                r#"^(?:(let(?:\s+mut)?|const|var|val|[A-Z][a-zA-Z0-9_<>*&]*|int|int32|int64|float|double|bool|boolean|String|string|auto)\s+)?([a-zA-Z_0-9]+(?:\.[a-zA-Z_0-9]+)*)(?:\s*:\s*[a-zA-Z_0-9*&<>\[\]]+)?\s*=\s*(.+?)\s*;?$"#,
            )
            .unwrap(),
            // Matches ch := make(chan int, 8), ch = Channel(10), let ch = Chan(4)
            make_chan_re: Regex::new(
                r#"^(?:(let|const|var|val)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*(?:make\s*\(\s*chan\s+[a-zA-Z_0-9*]+\s*(?:,\s*([0-9]+))?\s*\)|(?:Channel|Chan)\s*\(\s*([0-9]+)?\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches ch <- 42, out_chan.send(val)
            chan_send_re: Regex::new(
                r#"^(?:([a-zA-Z_0-9]+)\s+<-\s+(.+)|([a-zA-Z_0-9]+)\.send\s*\(\s*(.+)\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches val := <-ch, val = <-ch, val = ch.recv(), <-ch
            chan_recv_re: Regex::new(
                r#"^(?:(?:(let|const|var|val)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*)?(?:<-\s*([a-zA-Z_0-9]+)|([a-zA-Z_0-9]+)\.recv\s*\(\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches items = [10, 20, 30], items := [], const items = [1, 2]
            list_init_re: Regex::new(
                r#"^(?:(let|const|var|val)\s+)?([a-zA-Z_0-9]+)(?:\s*:\s*[a-zA-Z0-9_*&<>\[\]]+)?\s*(?::=|=)\s*\[(.*)\]\s*;?$"#,
            )
            .unwrap(),
            // Matches items.append(40), items.push(50), items.add(60)
            list_append_re: Regex::new(
                r#"^([a-zA-Z_0-9]+)\.(?:append|push|add)\s*\(\s*(.+)\s*\)\s*;?$"#,
            )
            .unwrap(),
            // Matches items[0] = 99, m["key"] = 10
            index_set_re: Regex::new(
                r#"^([a-zA-Z_0-9]+)\[(.+?)\]\s*=\s*(.+?)\s*;?$"#,
            )
            .unwrap(),
            // Matches m = {"alice": 100, "bob": 200}, m := {}
            map_init_re: Regex::new(
                r#"^(?:(let|const|var|val)\s+)?([a-zA-Z_0-9]+)(?:\s*:\s*[a-zA-Z_0-9_*&<>\[\], ]+)?\s*(?::=|=)\s*\{(.*)\}\s*;?$"#,
            )
            .unwrap(),
        }
    }

    /// Token-aware splitting that respects double/single quotes and balanced brackets/parens
    pub fn split_respecting_quotes(s: &str, delimiter: char) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut in_double_quote = false;
        let mut in_single_quote = false;
        let mut escape = false;
        let mut bracket_depth: usize = 0;

        for c in s.chars() {
            if escape {
                current.push(c);
                escape = false;
                continue;
            }
            if c == '\\' {
                current.push(c);
                escape = true;
                continue;
            }
            if c == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                current.push(c);
                continue;
            }
            if c == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                current.push(c);
                continue;
            }
            if !in_double_quote && !in_single_quote {
                if c == '(' || c == '[' || c == '{' {
                    bracket_depth += 1;
                } else if c == ')' || c == ']' || c == '}' {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
            }
            if c == delimiter && !in_double_quote && !in_single_quote && bracket_depth == 0 {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
                current.clear();
                continue;
            }
            current.push(c);
        }
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
        parts
    }

    /// Checks whether `#` starts a comment/shebang/directive rather than a Rust/C# attribute (`#[...]`, `#![...]`).
    #[inline]
    pub fn is_hash_comment(trimmed: &str) -> bool {
        trimmed.starts_with('#') && !trimmed.starts_with("#[") && !trimmed.starts_with("#![")
    }

    /// Distinguishes Python/Lua floor division (`q = a // b`) from C-family `//` single-line comments.
    fn is_python_floor_div(line_before: &str, after_slashes: &str) -> bool {
        let prefix = line_before.trim();
        if prefix.is_empty()
            || prefix.contains(';')
            || prefix.contains(":=")
            || prefix.contains("->")
            || prefix.contains('{')
            || NON_FLOOR_DIV_PREFIXES.iter().any(|p| prefix.starts_with(p))
        {
            return false;
        }

        let last_b = match prefix.as_bytes().last() {
            Some(&b) => b,
            None => return false,
        };
        if !(last_b.is_ascii_alphanumeric() || last_b == b'_' || last_b == b')' || last_b == b']') {
            return false;
        }

        // Trim any trailing `#` comment on the same Python line before inspecting the RHS operand.
        let rhs_raw = after_slashes.split('#').next().unwrap_or(after_slashes).trim();
        if rhs_raw.is_empty() {
            return false;
        }

        let lower_rhs = rhs_raw.to_lowercase();
        if COMMENT_PROSE_MARKERS.iter().any(|m| lower_rhs.starts_with(m))
            || rhs_raw.ends_with('.')
            || rhs_raw.ends_with('!')
            || rhs_raw.ends_with('?')
        {
            return false;
        }

        let first_rhs = rhs_raw.as_bytes()[0];
        let valid_start = first_rhs.is_ascii_alphanumeric()
            || matches!(first_rhs, b'_' | b'(' | b'[' | b'-' | b'+' | b'~');
        if !valid_start {
            return false;
        }

        // Reject natural-language prose (two adjacent identifier words not joined by a Python keyword).
        let mut prev_was_ident = false;
        for tok in rhs_raw.split_whitespace() {
            let clean = tok.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | ':' | ','));
            if clean.is_empty() {
                continue;
            }
            let is_word = clean.chars().all(|c| c.is_ascii_alphabetic() || c == '_');
            let is_kw = PYTHON_EXPR_KEYWORDS.contains(&clean);
            if is_word && !is_kw {
                if prev_was_ident {
                    return false;
                }
                prev_was_ident = true;
            } else {
                prev_was_ident = false;
            }
        }

        true
    }

    /// Distinguishes C/JS/Go `--` decrement (`i--`, `--i`) from Lua/SQL/Haskell/Ada `--` comments.
    fn is_decrement_op(bytes: &[u8], i: usize, line_before: &str) -> bool {
        let line_end = bytes[i + 2..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + 2 + p)
            .unwrap_or(bytes.len());
        let after = std::str::from_utf8(&bytes[i + 2..line_end]).unwrap_or("");
        let after_trim = after.trim_start_matches(|c: char| c == ' ' || c == '\t');

        // Postfix decrement: `i--`, `arr[idx]--`, `get()--`
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || matches!(prev, b'_' | b')' | b']') {
                if after_trim.is_empty()
                    || after_trim.starts_with(';')
                    || after_trim.starts_with(')')
                    || after_trim.starts_with(']')
                    || after_trim.starts_with(',')
                    || after_trim.starts_with('}')
                    || after_trim.starts_with('{')
                    || after_trim.starts_with("//")
                    || after_trim.starts_with("/*")
                    || after_trim.starts_with('#')
                {
                    return true;
                }
            }
        }

        // Prefix decrement: `--i;`, `x = --i`, `foo(--i)`
        let prev_non_ws = line_before.trim_end().as_bytes().last().copied();
        let valid_prefix_ctx = matches!(
            prev_non_ws,
            None | Some(b'=' | b'(' | b'[' | b',' | b';' | b':' | b'+' | b'-' | b'*' | b'{')
        );
        if valid_prefix_ctx && !after.is_empty() {
            let first = after.as_bytes()[0];
            if first.is_ascii_alphabetic() || first == b'_' {
                let ident_len = after
                    .bytes()
                    .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
                    .count();
                let tail = after[ident_len..].trim_start_matches(|c: char| c == ' ' || c == '\t');
                let has_stmt_delim = tail.starts_with(';')
                    || tail.starts_with(')')
                    || tail.starts_with(']')
                    || tail.starts_with(',');
                if has_stmt_delim || (prev_non_ws.is_some() && tail.is_empty()) {
                    return true;
                }
            }
        }

        false
    }

    /// Checks if `'` at `pos` opens a single-quoted literal on the same line (excluding Rust lifetimes `'a`).
    fn is_single_quote_literal_start(bytes: &[u8], pos: usize) -> bool {
        let prev = if pos > 0 { Some(bytes[pos - 1]) } else { None };

        // Rust reference lifetime `&'a` or generic lifetime `<'a>` / `<'a, 'b>`
        if matches!(prev, Some(b'&' | b'<')) && pos + 1 < bytes.len() {
            let first = bytes[pos + 1];
            if first.is_ascii_alphabetic() || first == b'_' {
                let mut k = pos + 2;
                while k < bytes.len() && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_') {
                    k += 1;
                }
                if k >= bytes.len() || bytes[k] != b'\'' {
                    return false;
                }
            }
        }

        //Haskell/ML primed identifier `x'`
        if let Some(p) = prev {
            if p.is_ascii_alphanumeric() || p == b'_' {
                // Allow Python/Rust byte/raw/format prefixes (`b'x'`, `r'x'`, `f'x'`, `u'x'`)
                let is_str_prefix = matches!(p, b'b' | b'r' | b'f' | b'u' | b'B' | b'R' | b'F' | b'U');
                if !is_str_prefix {
                    return false;
                }
            }
        }

        let mut j = pos + 1;
        let mut escaped = false;
        while j < bytes.len() && bytes[j] != b'\n' {
            if escaped {
                escaped = false;
            } else if bytes[j] == b'\\' {
                escaped = true;
            } else if bytes[j] == b'\'' {
                return true;
            }
            j += 1;
        }
        false
    }

    /// Verifies that treating `open` at `pos_after_open` as a nested block-comment opener still
    /// allows `depth` to reach `0` before EOF—skipping live-code string literals once `depth == 1`
    /// so C glob docs (`/* matches /*.c */`) never swallow subsequent code via a later `"*/"` string.
    fn can_close_nested_comment(source: &str, pos_after_open: usize, open: &str, close: &str, mut depth: usize) -> bool {
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut j = pos_after_open;
        while j < len {
            if depth == 1 && (bytes[j] == b'"' || bytes[j] == b'`') {
                let delim = bytes[j];
                j += 1;
                while j < len {
                    if bytes[j] == b'\\' && j + 1 < len {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == delim {
                        j += 1;
                        break;
                    }
                    j += source[j..].chars().next().unwrap().len_utf8();
                }
                continue;
            }
            if source[j..].starts_with(open) && !(open == "(*" && source[j..].starts_with("(*)")) {
                depth += 1;
                j += open.len();
            } else if source[j..].starts_with(close) {
                depth -= 1;
                j += close.len();
                if depth == 0 {
                    return true;
                }
            } else {
                j += source[j..].chars().next().unwrap().len_utf8();
            }
        }
        false
    }

    /// Disambiguates block-comment openers (`(*`, `{-`, etc.) from Python `*args` unpacking
    /// (`cls(*as_ints)`), C pointer dereferences (`(*ptr)`), and negative set/array literals (`{-1, -2}`).
    fn is_valid_block_comment_open(source: &str, i: usize, open: &str, close: &str) -> bool {
        let rest = &source[i..];
        if !rest.starts_with(open) {
            return false;
        }
        let after = &source[i + open.len()..];
        if !after.contains(close) {
            return false;
        }
        let bytes = source.as_bytes();
        let next_b = bytes.get(i + open.len()).copied();
        match open {
            "(*" => {
                if rest.starts_with("(*)") {
                    return false;
                }
                if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
                    return false;
                }
                if matches!(next_b, Some(b' ' | b'\t' | b'\r' | b'\n' | b'*')) {
                    return true;
                }
                let line_after = after.split('\n').next().unwrap_or("");
                match (line_after.find("*)"), line_after.find(')')) {
                    (Some(c_idx), Some(p_idx)) => c_idx < p_idx,
                    _ => false,
                }
            }
            "{-" => {
                if matches!(next_b, Some(b' ' | b'\t' | b'\r' | b'\n' | b'-' | b'#')) {
                    return true;
                }
                let line_after = after.split('\n').next().unwrap_or("");
                match (line_after.find("-}"), line_after.find('}')) {
                    (Some(c_idx), Some(p_idx)) => c_idx < p_idx,
                    _ => false,
                }
            }
            _ => true,
        }
    }

    /// Stateful byte-level pre-scanner that strips single-line, inline, nested, and line-anchored
    /// comments across all supported languages while preserving exact line counts, indentation,
    /// string literals, Rust attributes (`#[...]`), C decrements (`i--`), and Python `//` floor division.
    pub fn strip_polyglot_comments(source: &str) -> String {
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut out = String::with_capacity(len);
        let mut i = 0usize;
        let mut at_line_start = true;
        let mut paren_depth = 0usize;

        while i < len {
            let b = bytes[i];

            if b == b'\n' {
                out.push('\n');
                at_line_start = true;
                i += 1;
                continue;
            }

            if b == b' ' || b == b'\t' || b == b'\r' {
                out.push(b as char);
                i += 1;
                continue;
            }

            let rest = &source[i..];

            // 1. Line-anchored multi-line comments when at the first non-whitespace column of a line
            if at_line_start {
                let line_end = rest.find('\n').map(|p| i + p).unwrap_or(len);
                let line_trimmed = source[i..line_end].trim_end();

                // Ruby `=begin`..`=end` and Perl POD `=pod`/`=head1`..`=cut`
                if let Some((_, end_marker)) = LINE_ANCHORED_BLOCK_COMMENTS.iter().find(|(starts, _)| {
                    starts.iter().any(|st| {
                        line_trimmed == *st
                            || (line_trimmed.starts_with(st)
                                && line_trimmed[st.len()..].starts_with(|c: char| c.is_whitespace()))
                    })
                }) {
                    i = line_end;
                    while i < len {
                        if bytes[i] == b'\n' {
                            out.push('\n');
                            i += 1;
                            let nl_end = source[i..].find('\n').map(|p| i + p).unwrap_or(len);
                            let cand = source[i..nl_end].trim();
                            if cand == *end_marker
                                || (cand.starts_with(end_marker)
                                    && cand[end_marker.len()..].starts_with(|c: char| c.is_whitespace()))
                            {
                                i = nl_end;
                                break;
                            }
                        } else {
                            i += 1;
                        }
                    }
                    continue;
                }

                // MATLAB / Octave `%{` .. `%}` (valid only when `%{` is alone on the trimmed line)
                if line_trimmed == "%{" {
                    i = line_end;
                    while i < len {
                        if bytes[i] == b'\n' {
                            out.push('\n');
                            i += 1;
                            let nl_end = source[i..].find('\n').map(|p| i + p).unwrap_or(len);
                            if source[i..nl_end].trim() == "%}" {
                                i = nl_end;
                                break;
                            }
                        } else {
                            i += 1;
                        }
                    }
                    continue;
                }

                // Standalone multi-line docstring (`"""..."""`, `'''...'''`, plus raw/byte/format prefixes `r"""..."""`, `fr'''...'''`)
                let doc_prefix_len = {
                    let b0 = bytes[i].to_ascii_lowercase();
                    let b1 = bytes.get(i + 1).map(|c| c.to_ascii_lowercase());
                    if rest.len() >= 5
                        && matches!(
                            (b0, b1),
                            (b'f', Some(b'r')) | (b'r', Some(b'f')) | (b'b', Some(b'r')) | (b'r', Some(b'b'))
                        )
                        && (rest[2..].starts_with("\"\"\"") || rest[2..].starts_with("'''"))
                    {
                        Some(2usize)
                    } else if rest.len() >= 4
                        && matches!(b0, b'r' | b'b' | b'u' | b'f')
                        && (rest[1..].starts_with("\"\"\"") || rest[1..].starts_with("'''"))
                    {
                        Some(1usize)
                    } else if rest.starts_with("\"\"\"") || rest.starts_with("'''") {
                        Some(0usize)
                    } else {
                        None
                    }
                };
                if let Some(plen) = doc_prefix_len {
                    let quote = &rest[plen..plen + 3];
                    i += plen + 3;
                    while i < len {
                        if source[i..].starts_with(quote) {
                            i += 3;
                            break;
                        }
                        if bytes[i] == b'\\' && i + 1 < len {
                            if bytes[i + 1] == b'\n' {
                                out.push('\n');
                            }
                            i += 2;
                            continue;
                        }
                        if bytes[i] == b'\n' {
                            out.push('\n');
                        }
                        i += source[i..].chars().next().unwrap().len_utf8();
                    }
                    continue;
                }

                // Full-line Assembly/Lisp/INI `;`, Erlang/Prolog/MATLAB/LaTeX `%`, Fortran `! `, BASIC/Batch `REM `
                let is_bol_rem = (line_trimmed.len() >= 4
                    && line_trimmed[..3].eq_ignore_ascii_case("rem")
                    && line_trimmed.as_bytes()[3].is_ascii_whitespace())
                    || line_trimmed.eq_ignore_ascii_case("rem");
                let is_bol_fortran = rest.starts_with("! ") || rest.starts_with("!\t") || rest.starts_with("!!");
                let is_bol_percent = b == b'%'
                    && (rest.starts_with("%%")
                        || (paren_depth == 0 && !rest[1..].trim_start().starts_with('(')));
                if b == b';' || is_bol_percent || is_bol_fortran || is_bol_rem {
                    i = line_end;
                    continue;
                }
            }

            // 2. Lua parameterized long-bracket block comments: `--[[ ... ]]`, `--[=[ ... ]=]`
            if rest.starts_with("--[") {
                let mut eq_cnt = 0usize;
                let mut k = i + 3;
                while k < len && bytes[k] == b'=' {
                    eq_cnt += 1;
                    k += 1;
                }
                if k < len && bytes[k] == b'[' {
                    let close_pat = format!("]{}]", "=".repeat(eq_cnt));
                    if source[k + 1..].contains(&close_pat) {
                        i = k + 1;
                        let mut spanned_nl = false;
                        while i < len {
                            if source[i..].starts_with(&close_pat) {
                                i += close_pat.len();
                                break;
                            }
                            if bytes[i] == b'\n' {
                                out.push('\n');
                                spanned_nl = true;
                            }
                            i += source[i..].chars().next().unwrap().len_utf8();
                        }
                        if !spanned_nl {
                            if at_line_start {
                                while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                                    i += 1;
                                }
                            } else {
                                out.push(' ');
                            }
                        } else {
                            at_line_start = true;
                        }
                        continue;
                    }
                }
            }

            // 3. Paired polyglot block comments (`/* */`, `(* *)`, `{- -}`, `#| |#`, `#= =#`, `<!-- -->`, `{# #}`, `<# #>`, `### ###`)
            if let Some(&(open, close, allow_nesting)) = POLYGLOT_BLOCK_COMMENTS
                .iter()
                .find(|&&(op, cl, _)| Self::is_valid_block_comment_open(source, i, op, cl))
            {
                i += open.len();
                let mut depth = 1usize;
                let mut spanned_nl = false;
                while i < len && depth > 0 {
                    if bytes[i] == b'\n' {
                        out.push('\n');
                        spanned_nl = true;
                        i += 1;
                    } else if allow_nesting
                        && open != close
                        && source[i..].starts_with(open)
                        && Self::can_close_nested_comment(source, i + open.len(), open, close, depth + 1)
                    {
                        depth += 1;
                        i += open.len();
                    } else if source[i..].starts_with(close) {
                        depth -= 1;
                        i += close.len();
                    } else {
                        i += source[i..].chars().next().unwrap().len_utf8();
                    }
                }
                if !spanned_nl {
                    if at_line_start {
                        while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                            i += 1;
                        }
                    } else {
                        out.push(' ');
                    }
                } else {
                    at_line_start = true;
                }
                continue;
            }

            // 4. Protected String Literals (Rust raw strings, value triple-strings, backticks, double & single quotes)
            // 4a. Rust raw string `r#"..."#` / `br#"..."#`
            let is_ident_prev = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if !is_ident_prev && (b == b'r' || (b == b'b' && i + 1 < len && bytes[i + 1] == b'r')) {
                let r_pos = if b == b'b' { i + 1 } else { i };
                let mut h_pos = r_pos + 1;
                let mut hashes = 0usize;
                while h_pos < len && bytes[h_pos] == b'#' {
                    hashes += 1;
                    h_pos += 1;
                }
                if hashes > 0 && h_pos < len && bytes[h_pos] == b'"' {
                    let close_raw = format!("\"{}", "#".repeat(hashes));
                    let start_i = i;
                    i = h_pos + 1;
                    while i < len {
                        if source[i..].starts_with(&close_raw) {
                            i += close_raw.len();
                            break;
                        }
                        i += source[i..].chars().next().unwrap().len_utf8();
                    }
                    out.push_str(&source[start_i..i]);
                    at_line_start = out.ends_with('\n');
                    continue;
                }
            }

            // 4b. Value-position triple-quoted string (`msg = """hello # world"""`)
            if rest.starts_with("\"\"\"") || rest.starts_with("'''") {
                let quote = &rest[..3];
                let start_i = i;
                i += 3;
                while i < len {
                    if source[i..].starts_with(quote) {
                        i += 3;
                        break;
                    }
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
                out.push_str(&source[start_i..i]);
                at_line_start = false;
                continue;
            }

            // 4c. Double-quoted `"..."` or backtick `` `...` `` string literal
            if b == b'"' || b == b'`' {
                let delim = b;
                let start_i = i;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == delim {
                        i += 1;
                        break;
                    }
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
                out.push_str(&source[start_i..i]);
                at_line_start = false;
                continue;
            }

            // 4d. Single-quoted `'...'` literal (guarded against Rust lifetimes `'a` and primed idents `x'`)
            if b == b'\'' && Self::is_single_quote_literal_start(bytes, i) {
                let start_i = i;
                i += 1;
                while i < len && bytes[i] != b'\n' {
                    if bytes[i] == b'\\' && i + 1 < len {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    }
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
                out.push_str(&source[start_i..i]);
                at_line_start = false;
                continue;
            }

            // 5. Single-Line Comments (outside literals)
            let cur_line_start = out.rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_before = &out[cur_line_start..];
            let line_end = rest.find('\n').map(|p| i + p).unwrap_or(len);

            // 5a. Rust / C# attributes (`#[...]` / `#![...]`) preserved before `#` comment stripping
            if rest.starts_with("#[") || rest.starts_with("#![") {
                out.push('#');
                at_line_start = false;
                i += 1;
                continue;
            }

            // 5b. Hash `#` comment (Python, Ruby, Bash, Perl, YAML, TOML, R, Elixir, Julia, Shebang `#!`)
            if b == b'#' {
                let trim_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
                if !at_line_start {
                    out.truncate(trim_len);
                }
                i = line_end;
                continue;
            }

            // 5c. Double-slash `//` comment vs Python `//` floor division (`a // b`, `a //= b`)
            if rest.starts_with("//") {
                if rest.starts_with("//=") {
                    out.push_str("//=");
                    at_line_start = false;
                    i += 3;
                    continue;
                }
                let after_slashes = &source[i + 2..line_end];
                if !rest.starts_with("///")
                    && !rest.starts_with("//!")
                    && Self::is_python_floor_div(line_before, after_slashes)
                {
                    out.push_str("//");
                    at_line_start = false;
                    i += 2;
                    continue;
                }
                let trim_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
                if !at_line_start {
                    out.truncate(trim_len);
                }
                i = line_end;
                continue;
            }

            // 5d. Double-dash `--` comment (Lua, SQL, Haskell, Ada, VHDL) vs `--` decrement (`i--`, `--i`)
            if rest.starts_with("--") {
                if !rest.starts_with("---") && Self::is_decrement_op(bytes, i, line_before) {
                    out.push_str("--");
                    at_line_start = false;
                    i += 2;
                    continue;
                }
                let trim_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
                if !at_line_start {
                    out.truncate(trim_len);
                }
                i = line_end;
                continue;
            }

            // 5e. Double-semicolon `;;` comment (Lisp, Clojure, Scheme, WebAssembly WAT), preserving C `for (;;)`
            if rest.starts_with(";;") && !line_before.contains("for") {
                let trim_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
                if !at_line_start {
                    out.truncate(trim_len);
                }
                i = line_end;
                continue;
            }

            // 5f. Double-percent `%%` comment (Erlang, Prolog, MATLAB, LaTeX)
            if rest.starts_with("%%") {
                let trim_len = out.trim_end_matches(|c: char| c == ' ' || c == '\t').len();
                if !at_line_start {
                    out.truncate(trim_len);
                }
                i = line_end;
                continue;
            }

            // Regular source character
            if b == b'(' || b == b'[' {
                paren_depth += 1;
            } else if b == b')' || b == b']' {
                paren_depth = paren_depth.saturating_sub(1);
            }
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            at_line_start = false;
            i += ch.len_utf8();
        }

        out
    }

    /// Strip trailing single-line comment (`//`, `#`, `--`, `;;`, `%%`) outside string literals on a single line.
    pub fn strip_trailing_comment(line: &str) -> &str {
        let mut in_single = false;
        let mut in_double = false;
        let mut in_backtick = false;
        let mut escaped = false;
        let bytes = line.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            let b = bytes[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if b == b'\\' {
                escaped = true;
                i += 1;
                continue;
            }

            if b == b'\'' && !in_double && !in_backtick {
                if in_single || Self::is_single_quote_literal_start(bytes, i) {
                    in_single = !in_single;
                }
            } else if b == b'"' && !in_single && !in_backtick {
                in_double = !in_double;
            } else if b == b'`' && !in_single && !in_double {
                in_backtick = !in_backtick;
            } else if !in_single && !in_double && !in_backtick {
                let rest = &line[i..];
                if b == b'#' && !rest.starts_with("#[") && !rest.starts_with("#![") {
                    return line[..i].trim_end();
                }
                if rest.starts_with("//") && !rest.starts_with("//=") {
                    if rest.starts_with("///")
                        || rest.starts_with("//!")
                        || !Self::is_python_floor_div(&line[..i], &line[i + 2..])
                    {
                        return line[..i].trim_end();
                    }
                    i += 2;
                    continue;
                }
                if rest.starts_with("--") {
                    if rest.starts_with("---") || !Self::is_decrement_op(bytes, i, &line[..i]) {
                        return line[..i].trim_end();
                    }
                    i += 2;
                    continue;
                }
                if (rest.starts_with(";;") && !line[..i].contains("for")) || rest.starts_with("%%") {
                    return line[..i].trim_end();
                }
            }
            i += 1;
        }
        line
    }

    /// Parse class / interface / struct header: `class LinkedList<T> extends Listifiable implements MyList<T> {`
    pub fn parse_class_header(line: &str) -> Option<(String, String, Vec<String>)> {
        let mut s = line.trim().trim_end_matches(':').trim_end_matches('{').trim();
        if s.starts_with("template") {
            if let Some(pos) = s.rfind('>') {
                s = s[pos + 1..].trim();
            }
        }
        let tokens: Vec<&str> = s.split_whitespace().collect();
        let qualifiers = [
            "public", "private", "protected", "internal", "pub", "abstract",
            "virtual", "override", "inline", "sealed", "final", "static",
        ];
        let mut i = 0;
        while i < tokens.len() && qualifiers.contains(&tokens[i]) {
            i += 1;
        }
        if i >= tokens.len() {
            return None;
        }
        let kind = tokens[i];
        if kind != "class" && kind != "interface" && kind != "struct" {
            return None;
        }
        let rest = tokens[i + 1..].join(" ");
        let name_end = rest
            .find(|c: char| c == '<' || c == '(' || c == ' ' || c == ':' || c == '{')
            .unwrap_or(rest.len());
        let name = rest[..name_end].trim().to_string();
        if name.is_empty() {
            return None;
        }

        let mut bases = Vec::new();
        if let Some(open) = rest.find('(') {
            let end = rest.rfind(')').unwrap_or(rest.len());
            let base_str = &rest[open + 1..end];
            for b in base_str.split(',') {
                let clean_b = b.trim();
                if !clean_b.is_empty() {
                    bases.push(clean_b.to_string());
                }
            }
        } else if rest.contains(" extends ") || rest.contains(" implements ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let mut is_base = false;
            for part in &parts {
                if *part == "extends" || *part == "implements" {
                    is_base = true;
                    continue;
                }
                if *part == "where" {
                    break;
                }
                let cleaned = part.trim_matches(|c| c == ',' || c == '{');
                if is_base && !cleaned.is_empty() {
                    let clean_base = cleaned.split('<').next().unwrap_or(cleaned).trim();
                    if !clean_base.is_empty() {
                        bases.push(clean_base.to_string());
                    }
                }
            }
        }

        Some((kind.to_string(), name, bases))
    }

    /// Canonicalizes source code: runs the stateful polyglot comment pre-scanner first, then unifies
    /// bracket blocks (`{ ... }`), `end` blocks, postfix conditionals, and `++`/`--` into hierarchical AST lines.
    /// Preserves operators like `:=` and `=>` without destructive string replacement.
    pub fn canonicalize_source(source: &str) -> String {
        let stripped_source = Self::strip_polyglot_comments(source);
        let base_indent = stripped_source
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| Self::indent_level(l))
            .min()
            .unwrap_or(0);

        let raw_lines: Vec<&str> = stripped_source.lines().collect();
        let mut out_lines = Vec::new();
        let mut brace_depth: usize = 0;

        let inc_re = Regex::new(r#"^(?:([a-zA-Z_0-9]+)\+\+|\+\+([a-zA-Z_0-9]+))$"#).unwrap();
        let dec_re = Regex::new(r#"^(?:([a-zA-Z_0-9]+)\-\-|\-\-([a-zA-Z_0-9]+))$"#).unwrap();
        let postfix_if_re = Regex::new(r#"^(.*?)\s+if\s+(.*)$"#).unwrap();

        let mut idx = 0;
        while idx < raw_lines.len() {
            let raw_line = raw_lines[idx];
            idx += 1;
            let line = if base_indent > 0
                && raw_line.len() >= base_indent
                && raw_line[..base_indent].trim().is_empty()
            {
                &raw_line[base_indent..]
            } else {
                raw_line
            };

            let mut trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with(':')
                && (trimmed.starts_with(":param")
                    || trimmed.starts_with(":type")
                    || trimmed.starts_with(":return")
                    || trimmed.starts_with(":rtype"))
            {
                continue;
            }

            // Generalized multi-line expression/header joining:
            // Join subsequent lines whenever `( )`, `[ ]`, or dict-literal `{ }` depth > 0,
            // OR when a line ends with an explicit continuation (`\`) or binary continuation operator.
            let scan_delim_deltas = |s: &str| -> (isize, isize, isize) {
                let mut p_d = 0isize;
                let mut b_d = 0isize;
                let mut c_d = 0isize;
                let mut in_q: Option<char> = None;
                let mut esc = false;
                for ch in s.chars() {
                    if esc {
                        esc = false;
                        continue;
                    }
                    if in_q.is_some() {
                        if ch == '\\' {
                            esc = true;
                        } else if Some(ch) == in_q {
                            in_q = None;
                        }
                        continue;
                    }
                    match ch {
                        '"' | '\'' | '`' => in_q = Some(ch),
                        '(' => p_d += 1,
                        ')' => p_d -= 1,
                        '[' => b_d += 1,
                        ']' => b_d -= 1,
                        '{' => c_d += 1,
                        '}' => c_d -= 1,
                        _ => {}
                    }
                }
                (p_d, b_d, c_d)
            };
            let has_trailing_cont_op = |s: &str| -> bool {
                let t = s.trim_end();
                if t.is_empty() || t.ends_with('{') || t.ends_with('}') || t.ends_with(';') || t.ends_with(':') {
                    return false;
                }
                if t.ends_with('\\') {
                    return true;
                }
                if t.ends_with("++") || t.ends_with("--") || t.ends_with("->") || t.ends_with("=>") || t.ends_with("<-") {
                    return false;
                }
                if t.ends_with("&&")
                    || t.ends_with("||")
                    || t.ends_with("==")
                    || t.ends_with("!=")
                    || t.ends_with("<=")
                    || t.ends_with(">=")
                    || t.ends_with("<<")
                    || t.ends_with(">>")
                    || t.ends_with("**")
                    || t.ends_with("//")
                    || t.ends_with("|>")
                    || t.ends_with(" ?")
                    || t.ends_with(" and")
                    || t.ends_with(" or")
                    || t.ends_with('+')
                {
                    return true;
                }
                // Single-char binary operators `-`, `*`, `/`, `%`, `|`, `^`, `&` when preceded by expression context
                if (t.ends_with('-') || t.ends_with('*') || t.ends_with('/') || t.ends_with('%') || t.ends_with('|') || t.ends_with('^') || t.ends_with('&'))
                    && !t.starts_with('|')
                    && !t.starts_with('@')
                    && (t.contains('=') || t.starts_with("return ") || t.starts_with("if ") || t.starts_with("while ") || t.starts_with("elif "))
                {
                    return true;
                }
                false
            };

            let is_dict_assign = {
                let t_no_brace = trimmed.strip_suffix('{').map(|s| s.trim_end());
                matches!(t_no_brace, Some(b) if b.ends_with('=') || b.ends_with(":=") || b == "return")
            };
            let (mut p_depth, mut b_depth, mut c_depth) = scan_delim_deltas(trimmed);
            let track_c_depth = is_dict_assign;
            let mut cont = p_depth > 0
                || b_depth > 0
                || (track_c_depth && c_depth > 0)
                || has_trailing_cont_op(trimmed);

            let joined_buf: String;
            if cont {
                let mut accum_str = trimmed.to_string();
                if accum_str.ends_with('\\') {
                    accum_str.pop();
                    accum_str = accum_str.trim_end().to_string();
                }
                while cont && idx < raw_lines.len() {
                    let n_raw = raw_lines[idx];
                    idx += 1;
                    let n_stripped = Self::strip_trailing_comment(n_raw).trim();
                    if n_stripped.is_empty() {
                        continue;
                    }
                    let (dp, db, dc) = scan_delim_deltas(n_stripped);
                    p_depth += dp;
                    b_depth += db;
                    c_depth += dc;
                    if !accum_str.is_empty() {
                        accum_str.push(' ');
                    }
                    accum_str.push_str(n_stripped);
                    let had_backslash = accum_str.ends_with('\\');
                    if had_backslash {
                        accum_str.pop();
                        accum_str = accum_str.trim_end().to_string();
                    }
                    cont = p_depth > 0
                        || b_depth > 0
                        || (track_c_depth && c_depth > 0)
                        || had_backslash
                        || (p_depth == 0 && b_depth == 0 && (!track_c_depth || c_depth <= 0) && has_trailing_cont_op(&accum_str));
                }
                if is_dict_assign {
                    if accum_str.ends_with(';') {
                        accum_str.truncate(accum_str.len() - 1);
                        accum_str = accum_str.trim_end().to_string();
                    }
                    let current_indent = " ".repeat(Self::indent_level(line).max(brace_depth * 4));
                    out_lines.push(format!("{}{}", current_indent, accum_str));
                    continue;
                }
                joined_buf = accum_str;
                trimmed = joined_buf.trim();
            }

            // 1. Handle Lua/Ruby/Julia `end` block terminator
            if trimmed == "end" || trimmed == "end;" {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }

            // 2. Closing brace at start of line
            if trimmed.starts_with('}') {
                brace_depth = brace_depth.saturating_sub(1);
                trimmed = trimmed[1..].trim();
                if trimmed.is_empty() {
                    continue;
                }
            }

            // 3. Strip trailing semicolon
            if trimmed.ends_with(';') {
                trimmed = trimmed[..trimmed.len() - 1].trim();
            }

            // 4. Strip Lua `then` and `do`
            let mut line_str = trimmed.to_string();
            if line_str.ends_with(" then") {
                line_str = line_str[..line_str.len() - 5].trim().to_string();
            } else if line_str.ends_with(" do") {
                line_str = line_str[..line_str.len() - 3].trim().to_string();
            }

            // 4.5. Split top-level semicolon-separated statements on a single line (`a = 1; b = 2`)
            // when `;` occurs at `()[]{}` depth 0 outside quotes and line is not a `for` header.
            if line_str.contains(';') && !line_str.starts_with("for ") && !line_str.starts_with("for(") {
                let mut semi_parts = Vec::new();
                let mut seg_start = 0usize;
                let mut pd = 0isize;
                let mut bd = 0isize;
                let mut cd = 0isize;
                let mut in_q: Option<char> = None;
                let mut esc = false;
                for (bi, ch) in line_str.char_indices() {
                    if esc {
                        esc = false;
                        continue;
                    }
                    if in_q.is_some() {
                        if ch == '\\' {
                            esc = true;
                        } else if Some(ch) == in_q {
                            in_q = None;
                        }
                        continue;
                    }
                    match ch {
                        '"' | '\'' | '`' => in_q = Some(ch),
                        '(' => pd += 1,
                        ')' => pd -= 1,
                        '[' => bd += 1,
                        ']' => bd -= 1,
                        '{' => cd += 1,
                        '}' => cd -= 1,
                        ';' if pd == 0 && bd == 0 && cd == 0 => {
                            let part = line_str[seg_start..bi].trim();
                            if !part.is_empty() {
                                semi_parts.push(part.to_string());
                            }
                            seg_start = bi + 1;
                        }
                        _ => {}
                    }
                }
                if !semi_parts.is_empty() {
                    let tail_part = line_str[seg_start..].trim();
                    if !tail_part.is_empty() {
                        semi_parts.push(tail_part.to_string());
                    }
                    if semi_parts.len() > 1 {
                        let count = Self::indent_level(line);
                        let current_indent = " ".repeat(count.max(brace_depth * 4));
                        for mut sub in semi_parts {
                            if let Some(caps) = inc_re.captures(&sub) {
                                let var = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str();
                                sub = format!("{} = {} + 1", var, var);
                            } else if let Some(caps) = dec_re.captures(&sub) {
                                let var = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str();
                                sub = format!("{} = {} - 1", var, var);
                            }
                            out_lines.push(format!("{}{}", current_indent, sub));
                        }
                        continue;
                    }
                }
            }

            // 5. Handle C-style increment/decrement: `i++` / `++i` -> `i = i + 1`
            if let Some(caps) = inc_re.captures(&line_str) {
                let var = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str();
                line_str = format!("{} = {} + 1", var, var);
            } else if let Some(caps) = dec_re.captures(&line_str) {
                let var = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str();
                line_str = format!("{} = {} - 1", var, var);
            }

            // 6. Check if line ends with opening brace (explicit block)
            let ends_with_open = line_str.ends_with('{');
            if ends_with_open {
                line_str = line_str[..line_str.len() - 1].trim().to_string();
            }

            // Scoped indentation: preserve the maximum of brace-derived and source indentation
            let count = Self::indent_level(line);
            let current_indent = " ".repeat(count.max(brace_depth * 4));

            // 6.5. Handle Ruby/Perl `unless <cond>` block header -> `if !(<cond>):`
            if let Some(unless_cond) = line_str.strip_prefix("unless ") {
                let clean_c = unless_cond.trim().trim_end_matches(':').trim();
                line_str = format!("if !({})", clean_c);
            }

            // 7. Handle Ruby/Perl Postfix `if` / `unless`: `return 0 if x > 10` -> `if x > 10: return 0`
            // Ensure we do NOT match inside string literals like print("what if now") or list comprehensions `[x for x in y if ...]`
            if !line_str.starts_with("if ")
                && !line_str.starts_with("if(")
                && !line_str.contains('"')
                && !line_str.contains('\'')
                && !(line_str.contains('[') && line_str.contains(" for "))
            {
                if let Some((stmt_part, unless_cond)) = line_str.rsplit_once(" unless ") {
                    if !stmt_part.trim().is_empty() && !unless_cond.trim().is_empty() {
                        out_lines.push(format!("{}if !({}):", current_indent, unless_cond.trim()));
                        out_lines.push(format!("{}    {}", current_indent, stmt_part.trim()));
                        continue;
                    }
                }
                if let Some(caps) = postfix_if_re.captures(&line_str) {
                    let stmt = caps[1].trim();
                    let cond = caps[2].trim();
                    out_lines.push(format!("{}if {}:", current_indent, cond));
                    out_lines.push(format!("{}    {}", current_indent, stmt));
                    continue;
                }
            }

            // 8. Python single-line suites: `else: exit()`, `except: exit()`, `except: pass`
            if let Some(pos) = line_str.find(':') {
                let head = line_str[..=pos].trim();
                let tail = line_str[pos + 1..].trim();
                if !tail.is_empty() {
                    let is_suite_head = head == "else:"
                        || head == "finally:"
                        || head == "try:"
                        || head.starts_with("elif ")
                        || head.starts_with("except:")
                        || head.starts_with("except ")
                        || (head.starts_with("if ") && !head[3..].contains('"') && !head[3..].contains('\''));
                    if is_suite_head {
                        out_lines.push(format!("{}{}", current_indent, head));
                        out_lines.push(format!("{}    {}", current_indent, tail));
                        continue;
                    }
                }
            }

            out_lines.push(format!("{}{}", current_indent, line_str));

            if ends_with_open {
                brace_depth += 1;
            }
        }

        out_lines.join("\n")
    }

    /// Measure indentation level of a raw line
    fn indent_level(line: &str) -> usize {
        line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
    }

    /// Parse a polyglot decorator / annotation / attribute line (`@expr`, `#[attr]`, `[Attr]`)
    /// into a structured `AnnotationDef` without framework-specific assumptions.
    pub fn parse_annotation_line(trimmed: &str) -> Option<AnnotationDef> {
        let inner = if let Some(s) = trimmed.strip_prefix('@') {
            s.trim()
        } else if trimmed.starts_with("#[") && trimmed.ends_with(']') {
            trimmed[2..trimmed.len() - 1].trim()
        } else if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
            trimmed[2..trimmed.len() - 2].trim()
        } else {
            return None;
        };

        if inner.is_empty() {
            return None;
        }

        let (target_expr, arguments) = if let Some(lp) = inner.find('(') {
            let rp = inner.rfind(')').unwrap_or(inner.len());
            let head = inner[..lp].trim().to_string();
            let args_raw = inner[lp + 1..rp].trim();
            let args = if args_raw.is_empty() {
                Vec::new()
            } else {
                Self::split_respecting_quotes(args_raw, ',')
            };
            (head, args)
        } else {
            (inner.to_string(), Vec::new())
        };

        Some(AnnotationDef {
            raw_syntax: trimmed.to_string(),
            target_expr,
            arguments,
        })
    }

    /// Parse an OmniScript source string into an `OmniModule`
    pub fn parse_module(&self, module_name: &str, raw_source: &str) -> OmniModule {
        self.parse_module_with_dir(module_name, raw_source, None)
    }

    /// Parse an OmniScript source string with explicit base directory context
    pub fn parse_module_with_dir(
        &self,
        module_name: &str,
        raw_source: &str,
        base_dir: Option<&std::path::Path>,
    ) -> OmniModule {
        let source = Self::canonicalize_source(raw_source);
        let mem_norm = MemoryNormalizer::new();
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut enums = Vec::new();
        let mut top_level = Vec::new();
        let mut top_level_lines: Vec<(usize, String)> = Vec::new();

        let raw_lines: Vec<&str> = source.lines().collect();
        let mut idx = 0;
        let mut current_decorators: Vec<String> = Vec::new();

        while idx < raw_lines.len() {
            let line = raw_lines[idx];
            let trimmed = line.trim();

            if trimmed.is_empty() || Self::is_hash_comment(trimmed) || trimmed.starts_with("//") {
                idx += 1;
                continue;
            }

            // Inner crate/module attributes (`#![allow(...)]`)
            if trimmed.starts_with("#![") && trimmed.ends_with(']') {
                idx += 1;
                continue;
            }

            // Java / package statement: `package com.omni.example;`
            if trimmed.starts_with("package ") {
                idx += 1;
                continue;
            }

            // Decorators / Annotations / Attributes (`@...`, `#[...]`, `[[...]]`)
            if trimmed.starts_with('@')
                || (trimmed.starts_with("#[") && trimmed.ends_with(']'))
                || (trimmed.starts_with("[[") && trimmed.ends_with("]]"))
            {
                current_decorators.push(trimmed.to_string());
                idx += 1;
                continue;
            }

            // Import directives
            if trimmed.starts_with("import ") || trimmed.starts_with("from ") || trimmed.starts_with("use ") {
                if let Some(directive) = omni_preamble::SymbolCarver::parse_directive(trimmed) {
                    imports.push(directive);
                }
                idx += 1;
                continue;
            }

            // Struct definitions: `struct Point { ... }` or `pub struct Point { ... }`
            let is_struct = trimmed.starts_with("struct ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub(crate) struct ");
            if is_struct {
                let after_struct = if let Some(pos) = trimmed.find("struct ") {
                    &trimmed[pos + 7..]
                } else {
                    trimmed
                };
                let struct_name = after_struct
                    .split(|c: char| c == '{' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let mut class_fields = Vec::new();
                let parent_indent = Self::indent_level(line);

                if trimmed.contains('{') && trimmed.contains('}') {
                    if let Some(open) = trimmed.find('{') {
                        if let Some(close) = trimmed.rfind('}') {
                            let inner = &trimmed[open + 1..close];
                            for field_part in inner.split(',') {
                                let f_trim = field_part.trim().trim_start_matches("pub ").trim();
                                if let Some((f_name, f_type)) = f_trim.split_once(':') {
                                    class_fields.push(FieldDef {
                                        name: f_name.trim().to_string(),
                                        type_hint: Some(f_type.trim().to_string()),
                                        default_val: None,
                                    });
                                }
                            }
                        }
                    }
                    idx += 1;
                } else {
                    idx += 1;
                    while idx < raw_lines.len() {
                        let next_line = raw_lines[idx];
                        let next_trimmed = next_line.trim();

                        if next_trimmed.is_empty() || next_trimmed.starts_with('#') || next_trimmed.starts_with("//") {
                            idx += 1;
                            continue;
                        }

                        if next_trimmed == "}" || next_trimmed.starts_with("};") {
                            idx += 1;
                            break;
                        }

                        let cur_indent = Self::indent_level(next_line);
                        if cur_indent <= parent_indent {
                            break;
                        }

                        let f_trim = next_trimmed
                            .trim_end_matches(',')
                            .trim_end_matches(';')
                            .trim()
                            .trim_start_matches("pub ")
                            .trim();
                        if let Some((f_name, f_type)) = f_trim.split_once(':') {
                            class_fields.push(FieldDef {
                                name: f_name.trim().to_string(),
                                type_hint: Some(f_type.trim().to_string()),
                                default_val: None,
                            });
                        } else {
                            let tokens: Vec<&str> = f_trim.split_whitespace().collect();
                            if tokens.len() >= 2 {
                                class_fields.push(FieldDef {
                                    name: tokens[tokens.len() - 1].to_string(),
                                    type_hint: Some(tokens[0].to_string()),
                                    default_val: None,
                                });
                            }
                        }

                        idx += 1;
                    }
                }

                classes.push(ClassDef {
                    name: struct_name,
                    decorators: current_decorators.clone(),
                    base_classes: Vec::new(),
                    fields: class_fields,
                    methods: Vec::new(),
                });
                current_decorators.clear();
                continue;
            }

            // Inherent & trait impl blocks: `impl Point { ... }` or `impl Default for Adler32 { ... }`
            if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
                let impl_line = trimmed
                    .trim_start_matches("impl")
                    .trim_end_matches('{')
                    .trim_end_matches(':')
                    .trim();
                let target_seg = if let Some((_, rhs)) = impl_line.rsplit_once(" for ") {
                    rhs.trim()
                } else {
                    impl_line
                };
                let struct_name = target_seg
                    .split(|c: char| c.is_whitespace() || c == '<')
                    .find(|tok| !tok.is_empty())
                    .unwrap_or("")
                    .to_string();

                let parent_indent = Self::indent_level(line);
                idx += 1;

                let mut impl_methods = Vec::new();

                while idx < raw_lines.len() {
                    let next_line = raw_lines[idx];
                    let next_trimmed = next_line.trim();

                    if next_trimmed.is_empty() || next_trimmed.starts_with('#') || next_trimmed.starts_with("//") {
                        idx += 1;
                        continue;
                    }

                    if next_trimmed == "}" {
                        idx += 1;
                        break;
                    }

                    let cur_indent = Self::indent_level(next_line);
                    if cur_indent <= parent_indent {
                        break;
                    }

                    if let Some(caps) = self.fn_re.captures(next_trimmed) {
                        let method_name = caps[1].to_string();
                        let method_ret_type = Self::extract_fn_return_type(next_trimmed, &method_name);
                        let raw_params = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                        let params: Vec<ParamDef> = Self::split_respecting_quotes(raw_params, ',')
                            .iter()
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .map(|p| Self::parse_param(p))
                            .collect();
                        let method_indent = cur_indent;
                        idx += 1;

                        let mut method_lines = Vec::new();
                        while idx < raw_lines.len() {
                            let m_line = raw_lines[idx];
                            let m_trimmed = m_line.trim();
                            if m_trimmed.is_empty() {
                                idx += 1;
                                continue;
                            }
                            if m_trimmed == "}" && Self::indent_level(m_line) == method_indent {
                                idx += 1;
                                break;
                            }
                            let m_indent = Self::indent_level(m_line);
                            if m_indent > method_indent {
                                method_lines.push((m_indent, m_trimmed.to_string()));
                                idx += 1;
                            } else {
                                break;
                            }
                        }

                        let body = self.parse_block(&method_lines, &method_name, &mem_norm);
                        let raw_joined = method_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                        let memory_ops = mem_norm.normalize_block(&raw_joined, &method_name);

                        impl_methods.push(FunctionDef {
                            name: method_name,
                            is_method: true,
                            annotations: Vec::new(),
                            params,
                            return_type: method_ret_type,
                            body,
                            memory_ops,
                        });
                        continue;
                    }

                    idx += 1;
                }

                if let Some(existing) = classes.iter_mut().find(|c| c.name == struct_name) {
                    existing.methods.extend(impl_methods);
                } else {
                    classes.push(ClassDef {
                        name: struct_name,
                        decorators: Vec::new(),
                        base_classes: Vec::new(),
                        fields: Vec::new(),
                        methods: impl_methods,
                    });
                }
                continue;
            }

            // Enum definitions: `enum Status { ... }` or `pub enum Status { ... }`
            let is_enum = trimmed.starts_with("enum ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub(crate) enum ");
            if is_enum {
                let after_enum = if let Some(pos) = trimmed.find("enum ") {
                    &trimmed[pos + 5..]
                } else {
                    trimmed
                };
                let enum_name = after_enum
                    .split(|c: char| c == '{' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let mut variants = Vec::new();
                let mut tag = 0;
                let parent_indent = Self::indent_level(line);

                if trimmed.contains('{') && trimmed.contains('}') {
                    if let Some(open) = trimmed.find('{') {
                        if let Some(close) = trimmed.rfind('}') {
                            let inner = &trimmed[open + 1..close];
                            for var_part in inner.split(',') {
                                let v_trim = var_part.trim();
                                if v_trim.is_empty() { continue; }
                                let (v_name, p_type) = if let Some((n, rest)) = v_trim.split_once('(') {
                                    (n.trim().to_string(), Some(rest.trim_end_matches(')').trim().to_string()))
                                } else {
                                    (v_trim.to_string(), None)
                                };
                                variants.push(EnumVariant { name: v_name, tag, payload_type: p_type });
                                tag += 1;
                            }
                        }
                    }
                    idx += 1;
                } else {
                    idx += 1;
                    while idx < raw_lines.len() {
                        let next_line = raw_lines[idx];
                        let next_trimmed = next_line.trim();
                        if next_trimmed.is_empty() || next_trimmed.starts_with('#') || next_trimmed.starts_with("//") {
                            idx += 1;
                            continue;
                        }
                        if next_trimmed == "}" || next_trimmed.starts_with("};") {
                            idx += 1;
                            break;
                        }
                        let cur_indent = Self::indent_level(next_line);
                        if cur_indent <= parent_indent {
                            break;
                        }
                        for var_part in next_trimmed.split(',') {
                            let v_trim = var_part.trim();
                            if v_trim.is_empty() { continue; }
                            let (v_name, p_type) = if let Some((n, rest)) = v_trim.split_once('(') {
                                (n.trim().to_string(), Some(rest.trim_end_matches(')').trim().to_string()))
                            } else {
                                (v_trim.to_string(), None)
                            };
                            variants.push(EnumVariant { name: v_name, tag, payload_type: p_type });
                            tag += 1;
                        }
                        idx += 1;
                    }
                }

                enums.push(EnumDef {
                    name: enum_name,
                    variants,
                });
                continue;
            }

            // Class / Interface / Struct / Template definitions
            let is_decl_start = Self::parse_class_header(trimmed).is_some()
                || trimmed.starts_with("template");

            if is_decl_start {
                let mut full_header = trimmed.to_string();
                let mut accum_idx = idx;

                while accum_idx + 1 < raw_lines.len() {
                    let next = raw_lines[accum_idx + 1].trim();
                    if next.is_empty()
                        || next.starts_with("//")
                        || next.starts_with('#')
                        || next.starts_with("/*")
                        || next.starts_with('*')
                    {
                        accum_idx += 1;
                        continue;
                    }

                    let has_class_name = Self::parse_class_header(&full_header).is_some();
                    let next_first = next.split_whitespace().next().unwrap_or("");
                    let is_continuation = ["extends", "implements", "where", "throws", "with"].contains(&next_first)
                        || next.starts_with(':')
                        || next.starts_with('{')
                        || next.starts_with(',')
                        || !has_class_name;

                    if !is_continuation {
                        break;
                    }

                    accum_idx += 1;
                    full_header.push(' ');
                    full_header.push_str(next);
                    if next.contains('{') || next.ends_with(':') || next.ends_with(';') {
                        break;
                    }
                }

                if let Some((_kind, class_name, bases)) = Self::parse_class_header(&full_header) {
                    idx = accum_idx + 1;
                    let parent_indent = Self::indent_level(line);
                    let mut class_methods = Vec::new();
                    let mut class_fields = Vec::new();
                    let mut method_decorators = Vec::new();

                    while idx < raw_lines.len() {
                        let next_line = raw_lines[idx];
                        let next_trimmed = next_line.trim();

                        if next_trimmed.is_empty()
                            || Self::is_hash_comment(next_trimmed)
                            || next_trimmed.starts_with("//")
                            || next_trimmed.starts_with("/*")
                            || next_trimmed.starts_with('*')
                        {
                            idx += 1;
                            continue;
                        }

                        // Skip docstrings in class bodies
                        if next_trimmed.starts_with("\"\"\"") || next_trimmed.starts_with("'''") {
                            let quote = if next_trimmed.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
                            if next_trimmed.len() > 3 && next_trimmed[3..].contains(quote) {
                                idx += 1;
                                continue;
                            }
                            idx += 1;
                            while idx < raw_lines.len() {
                                let l = raw_lines[idx].trim();
                                if l.contains(quote) {
                                    idx += 1;
                                    break;
                                }
                                idx += 1;
                            }
                            continue;
                        }

                        if next_trimmed == "}" {
                            idx += 1;
                            break;
                        }

                        let cur_indent = Self::indent_level(next_line);
                        if cur_indent <= parent_indent {
                            break;
                        }

                        // Decorators: @classmethod, @staticmethod, etc.
                        if next_trimmed.starts_with('@') || (next_trimmed.starts_with("#[") && next_trimmed.ends_with(']')) {
                            method_decorators.push(next_trimmed.to_string());
                            idx += 1;
                            continue;
                        }

                        // Inner struct/class definition (e.g. `sealed struct Node<T> { ... }`)
                        let inner_clean = next_trimmed.trim_start_matches("template");
                        let inner_first = inner_clean.split_whitespace().next().unwrap_or("");
                        if ["struct", "sealed", "class", "interface"].contains(&inner_first) {
                            if let Some((_ikind, iname, ibases)) = Self::parse_class_header(next_trimmed) {
                                let mut inner_methods = Vec::new();
                                let mut inner_fields = Vec::new();
                                let inner_indent = cur_indent;
                                idx += 1;

                                while idx < raw_lines.len() {
                                    let i_line = raw_lines[idx];
                                    let i_trimmed = i_line.trim();
                                    if i_trimmed.is_empty()
                                        || i_trimmed.starts_with('#')
                                        || i_trimmed.starts_with("//")
                                        || i_trimmed.starts_with("/*")
                                        || i_trimmed.starts_with('*')
                                    {
                                        idx += 1;
                                        continue;
                                    }
                                    let i_indent = Self::indent_level(i_line);
                                    if i_indent <= inner_indent {
                                        if i_trimmed == "}" {
                                            idx += 1;
                                        }
                                        break;
                                    }

                                    let clean_ctor = i_trimmed.trim_start_matches("public ").trim();
                                    let is_inner_ctor = (clean_ctor.starts_with("constructor") && clean_ctor.contains('('))
                                        || (clean_ctor.starts_with(&iname) && clean_ctor[iname.len()..].trim_start().starts_with('('));
                                    if is_inner_ctor {
                                        if let Some(open) = clean_ctor.find('(') {
                                            let close = clean_ctor.rfind(')').unwrap_or(clean_ctor.len());
                                            let raw_p = &clean_ctor[open + 1..close];
                                            let mut params: Vec<ParamDef> = Self::split_respecting_quotes(raw_p, ',')
                                                .iter()
                                                .map(|s| s.trim())
                                                .filter(|s| !s.is_empty())
                                                .map(|p| Self::parse_param(p))
                                                .collect();
                                            if params.is_empty() || (params[0].name != "self" && params[0].name != "this") {
                                                params.insert(0, ParamDef {
                                                    name: "this".to_string(),
                                                    type_hint: None,
                                                    default_val: None,
                                                });
                                            }
                                            let m_indent = Self::indent_level(i_line);
                                            idx += 1;
                                            let mut m_lines = Vec::new();
                                            while idx < raw_lines.len() {
                                                let ml = raw_lines[idx];
                                                let mt = ml.trim();
                                                if mt.is_empty() {
                                                    idx += 1;
                                                    continue;
                                                }
                                                if mt == "}" && Self::indent_level(ml) == m_indent {
                                                    idx += 1;
                                                    break;
                                                }
                                                if Self::indent_level(ml) > m_indent {
                                                    m_lines.push((Self::indent_level(ml), mt.to_string()));
                                                    idx += 1;
                                                } else {
                                                    break;
                                                }
                                            }
                                            let body = self.parse_block(&m_lines, "__init__", &mem_norm);
                                            let joined = m_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                                            inner_methods.push(FunctionDef {
                                                name: "__init__".to_string(),
                                                is_method: true,
                                                annotations: Vec::new(),
                                                params,
                                                return_type: None,
                                                body,
                                                memory_ops: mem_norm.normalize_block(&joined, "__init__"),
                                            });
                                            continue;
                                        }
                                    } else if !i_trimmed.contains('(') && !i_trimmed.contains(')') && !i_trimmed.starts_with("return") {
                                        let clean_f = i_trimmed.trim_end_matches(';').trim();
                                        let tokens: Vec<&str> = clean_f.split_whitespace().collect();
                                        if tokens.len() >= 2 {
                                            inner_fields.push(FieldDef {
                                                name: tokens[tokens.len() - 1].to_string(),
                                                type_hint: Some(tokens[0].to_string()),
                                                default_val: None,
                                            });
                                        } else if tokens.len() == 1 {
                                            inner_fields.push(FieldDef {
                                                name: tokens[0].to_string(),
                                                type_hint: None,
                                                default_val: None,
                                            });
                                        }
                                    }
                                    idx += 1;
                                }

                                classes.push(ClassDef {
                                    name: iname,
                                    decorators: Vec::new(),
                                    base_classes: ibases,
                                    fields: inner_fields,
                                    methods: inner_methods,
                                });
                                continue;
                            }
                        }

                        // Check for constructor: `constructor(...)`, `public User(...)`, `User(...)`
                        let clean_ctor = next_trimmed
                            .trim_start_matches("public ")
                            .trim_start_matches("private ")
                            .trim_start_matches("protected ")
                            .trim();
                        let is_constructor = (clean_ctor.starts_with("constructor") && clean_ctor[11..].trim_start().starts_with('('))
                            || (clean_ctor.starts_with(&class_name)
                                && clean_ctor[class_name.len()..].trim_start().starts_with('('));

                        if is_constructor {
                            let method_name = "__init__".to_string();
                            let open_idx = clean_ctor.find('(').unwrap();
                            let close_idx = clean_ctor.rfind(')').unwrap_or(clean_ctor.len());
                            let raw_params = &clean_ctor[open_idx + 1..close_idx];
                            let mut params: Vec<ParamDef> = Self::split_respecting_quotes(raw_params, ',')
                                .iter()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(|p| Self::parse_param(p))
                                .collect();
                            // Prepend receiver parameter `this`
                            if params.is_empty() || (params[0].name != "self" && params[0].name != "this") {
                                params.insert(0, ParamDef {
                                    name: "this".to_string(),
                                    type_hint: None,
                                    default_val: None,
                                });
                            }
                            let method_indent = cur_indent;
                            idx += 1;

                            let mut method_lines = Vec::new();
                            while idx < raw_lines.len() {
                                let m_line = raw_lines[idx];
                                let m_trimmed = m_line.trim();
                                if m_trimmed.is_empty() {
                                    idx += 1;
                                    continue;
                                }
                                if m_trimmed == "}" && Self::indent_level(m_line) == method_indent {
                                    idx += 1;
                                    break;
                                }
                                let m_indent = Self::indent_level(m_line);
                                if m_indent > method_indent {
                                    method_lines.push((m_indent, m_trimmed.to_string()));
                                    idx += 1;
                                } else {
                                    break;
                                }
                            }

                            let body = self.parse_block(&method_lines, &method_name, &mem_norm);
                            let raw_joined = method_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                            let memory_ops = mem_norm.normalize_block(&raw_joined, &method_name);

                            class_methods.push(FunctionDef {
                                name: method_name,
                                is_method: true,
                                annotations: Vec::new(),
                                params,
                                return_type: None,
                                body,
                                memory_ops,
                            });
                            continue;
                        }

                        if let Some(caps) = self.fn_re.captures(next_trimmed) {
                            let method_name = caps[1].to_string();
                            let method_ret_type = Self::extract_fn_return_type(next_trimmed, &method_name);
                            let raw_params = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                            let mut params: Vec<ParamDef> = Self::split_respecting_quotes(raw_params, ',')
                                .iter()
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(|p| Self::parse_param(p))
                                .collect();
                            let is_static = next_trimmed.contains("static ")
                                || method_decorators.iter().any(|d| d.contains("staticmethod") || d.contains("classmethod"));
                            let ann: Vec<AnnotationDef> = method_decorators.drain(..).filter_map(|d| Self::parse_annotation_line(&d)).collect();
                            if !is_static && (params.is_empty() || (params[0].name != "self" && params[0].name != "this")) {
                                params.insert(0, ParamDef {
                                    name: "this".to_string(),
                                    type_hint: None,
                                    default_val: None,
                                });
                            }

                            let method_indent = cur_indent;
                            idx += 1;

                            let mut method_lines = Vec::new();
                            while idx < raw_lines.len() {
                                let m_line = raw_lines[idx];
                                let m_trimmed = m_line.trim();
                                if m_trimmed.is_empty() {
                                    idx += 1;
                                    continue;
                                }
                                if m_trimmed == "}" && Self::indent_level(m_line) == method_indent {
                                    idx += 1;
                                    break;
                                }
                                let m_indent = Self::indent_level(m_line);
                                if m_indent > method_indent {
                                    method_lines.push((m_indent, m_trimmed.to_string()));
                                    idx += 1;
                                } else {
                                    break;
                                }
                            }

                            let body = self.parse_block(&method_lines, &method_name, &mem_norm);
                            let raw_joined = method_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                            let memory_ops = mem_norm.normalize_block(&raw_joined, &method_name);

                            if method_name == "main" {
                                functions.push(FunctionDef {
                                    name: "main".to_string(),
                                    is_method: false,
                                    annotations: Vec::new(),
                                    params: Vec::new(),
                                    return_type: None,
                                    body: body.clone(),
                                    memory_ops: memory_ops.clone(),
                                });
                            }

                            class_methods.push(FunctionDef {
                                name: method_name,
                                is_method: !is_static,
                                annotations: ann,
                                params,
                                return_type: method_ret_type,
                                body,
                                memory_ops,
                            });
                            continue;
                        }

                        method_decorators.clear();

                        // Field definitions (Java `private int width = 760;`, Python/Kotlin/Rust `pub xy: i64,`)
                        if !next_trimmed.contains('(')
                            && !next_trimmed.contains(')')
                            && !next_trimmed.starts_with("return")
                            && !next_trimmed.starts_with("\"\"\"")
                            && !next_trimmed.starts_with("'''")
                            && !next_trimmed.starts_with(">>>")
                            && !next_trimmed.starts_with(':')
                        {
                            let clean_line = next_trimmed
                                .trim_end_matches(';')
                                .trim_end_matches(',')
                                .trim();
                            let (decl_part, default_val_opt) = if clean_line.contains('=') && !clean_line.starts_with("if") {
                                let parts: Vec<&str> = clean_line.splitn(2, '=').collect();
                                (parts[0].trim(), Some(parts[1].trim().to_string()))
                            } else {
                                (clean_line, None)
                            };
                            let clean_decl = decl_part
                                .trim_start_matches("public ")
                                .trim_start_matches("private ")
                                .trim_start_matches("protected ")
                                .trim_start_matches("pub ")
                                .trim_start_matches("static ")
                                .trim_start_matches("final ")
                                .trim_start_matches("val ")
                                .trim_start_matches("var ")
                                .trim_start_matches("let ")
                                .trim_start_matches("mut ")
                                .trim();
                            if clean_decl.contains(':') {
                                let cparts: Vec<&str> = clean_decl.splitn(2, ':').collect();
                                let f_name = cparts[0].split_whitespace().last().unwrap_or(cparts[0]).trim().to_string();
                                let t_str = cparts[1].trim_start_matches(':').trim().to_string();
                                if !f_name.is_empty() {
                                    class_fields.push(FieldDef {
                                        name: f_name,
                                        type_hint: if t_str.is_empty() { None } else { Some(t_str) },
                                        default_val: default_val_opt,
                                    });
                                }
                            } else {
                                let tokens: Vec<&str> = clean_decl.split_whitespace().collect();
                                if tokens.len() >= 2 {
                                    let type_str = tokens[0].to_string();
                                    let f_name = tokens[tokens.len() - 1].to_string();
                                    class_fields.push(FieldDef {
                                        name: f_name,
                                        type_hint: Some(type_str),
                                        default_val: default_val_opt,
                                    });
                                } else if tokens.len() == 1 && !tokens[0].is_empty() {
                                    class_fields.push(FieldDef {
                                        name: tokens[0].to_string(),
                                        type_hint: None,
                                        default_val: default_val_opt,
                                    });
                                }
                            }
                        }

                        idx += 1;
                    }

                    // Auto-discover dynamically assigned instance fields in methods (recursively traversing blocks)
                    for m in &class_methods {
                        Self::discover_fields_recursive(&m.body, &mut class_fields);
                    }

                    classes.push(ClassDef {
                        name: class_name,
                        decorators: current_decorators.clone(),
                        base_classes: bases,
                        fields: class_fields,
                        methods: class_methods,
                    });
                    current_decorators.clear();
                    continue;
                }
            }

            // Top-level JS/TS/C# single-expression arrow functions: `const bump = (x: number): number => x * 2 + 5`
            if trimmed.contains("=>") && !trimmed.starts_with("match ") && !trimmed.starts_with("if ") {
                let arrow_re = Regex::new(
                    r#"^(?:(?:export\s+)?(?:const|let|var|auto)\s+)?([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(?:\(([^)]*)\)|([a-zA-Z_][a-zA-Z0-9_]*))\s*(?::\s*[^=]+?)?\s*=>\s*(.+?)\s*;?$"#,
                )
                .unwrap();
                if let Some(caps) = arrow_re.captures(trimmed) {
                    let fn_name = caps[1].to_string();
                    let raw_params = caps
                        .get(2)
                        .or_else(|| caps.get(3))
                        .map(|m| m.as_str())
                        .unwrap_or("");
                    let params: Vec<ParamDef> = Self::split_respecting_quotes(raw_params, ',')
                        .iter()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|p| Self::parse_param(p))
                        .collect();
                    let expr_rhs = caps[4].trim().to_string();
                    functions.push(FunctionDef {
                        name: fn_name,
                        is_method: false,
                        annotations: Vec::new(),
                        params,
                        return_type: None,
                        body: vec![Statement::Return { expr: Some(expr_rhs) }],
                        memory_ops: Vec::new(),
                    });
                    current_decorators.clear();
                    idx += 1;
                    continue;
                }
            }

            // Top-level functions: `def foo():`, `fn foo() -> i32:`, `int foo() {`
            if let Some(caps) = self.fn_re.captures(trimmed) {
                let fn_name = caps[1].to_string();
                let fn_ret_type = Self::extract_fn_return_type(trimmed, &fn_name);
                let raw_params = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let params: Vec<ParamDef> = Self::split_respecting_quotes(raw_params, ',')
                    .iter()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|p| Self::parse_param(p))
                    .collect();

                let fn_indent = Self::indent_level(line);
                idx += 1;

                let mut block_lines = Vec::new();

                while idx < raw_lines.len() {
                    let next_line = raw_lines[idx];
                    let next_trimmed = next_line.trim();

                    if next_trimmed.is_empty() {
                        idx += 1;
                        continue;
                    }

                    let cur_indent = Self::indent_level(next_line);
                    if cur_indent > fn_indent {
                        block_lines.push((cur_indent, next_trimmed.to_string()));
                        idx += 1;
                    } else {
                        break;
                    }
                }

                let body = self.parse_block(&block_lines, &fn_name, &mem_norm);
                let raw_joined = block_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                let memory_ops = mem_norm.normalize_block(&raw_joined, &fn_name);

                let annotations: Vec<AnnotationDef> = current_decorators
                    .iter()
                    .filter_map(|d| Self::parse_annotation_line(d))
                    .collect();

                for ann in &annotations {
                    let is_compiler_attr = matches!(
                        ann.target_expr.as_str(),
                        "inline" | "alwaysinline" | "noinline" | "cold" | "derive" | "Override"
                    );
                    if !is_compiler_attr {
                        top_level_lines.push((
                            0,
                            format!("__omni_annotate__::{}::{}", fn_name, ann.raw_syntax),
                        ));
                    }
                }

                functions.push(FunctionDef {
                    name: fn_name,
                    is_method: false,
                    annotations,
                    params,
                    return_type: fn_ret_type,
                    body,
                    memory_ops,
                });
                current_decorators.clear();
                continue;
            }

            // Top-level statement line (preserve indentation for block constructs like top-level if/else/for)
            top_level_lines.push((Self::indent_level(line), trimmed.to_string()));
            idx += 1;
        }

        if !top_level_lines.is_empty() {
            let parsed_top = self.parse_block(&top_level_lines, "main", &mem_norm);
            let raw_joined = top_level_lines
                .iter()
                .map(|(_, s)| s.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let memory_ops = mem_norm.normalize_block(&raw_joined, "main");
            top_level = parsed_top.clone();

            if !functions.iter().any(|f| f.name == "main") {
                functions.push(FunctionDef {
                    name: "main".to_string(),
                    is_method: false,
                    annotations: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    body: parsed_top,
                    memory_ops,
                });
            }
        }

        // If top-level functions were empty but methods exist, lift methods to top-level functions
        if functions.is_empty() {
            for cls in &classes {
                for method in &cls.methods {
                    functions.push(method.clone());
                }
            }
        }

        OmniModule {
            name: module_name.to_string(),
            imports,
            structs_and_classes: classes,
            enums,
            functions,
            top_level_statements: top_level,
            base_dir: base_dir.map(|p| p.to_path_buf()),
            diagnostics: Vec::new(),
        }
    }

    fn discover_fields_recursive(stmts: &[Statement], class_fields: &mut Vec<FieldDef>) {
        for stmt in stmts {
            match stmt {
                Statement::Assign { target, .. } => {
                    let fld_opt = target.strip_prefix("self.").or_else(|| target.strip_prefix("this."));
                    if let Some(fld) = fld_opt {
                        let fld_name = fld.trim().to_string();
                        if !class_fields.iter().any(|f| f.name == fld_name) {
                            class_fields.push(FieldDef {
                                name: fld_name,
                                type_hint: Some("i64".to_string()),
                                default_val: None,
                            });
                        }
                    }
                }
                Statement::If { then_branch, else_branch, .. } => {
                    Self::discover_fields_recursive(then_branch, class_fields);
                    if let Some(eb) = else_branch {
                        Self::discover_fields_recursive(eb, class_fields);
                    }
                }
                Statement::WhileLoop { body, .. }
                | Statement::ForLoop { body, .. }
                | Statement::WithContext { body, .. } => {
                    Self::discover_fields_recursive(body, class_fields);
                }
                Statement::TryCatch { try_block, catch_clauses, finally_block } => {
                    Self::discover_fields_recursive(try_block, class_fields);
                    for c in catch_clauses {
                        Self::discover_fields_recursive(&c.body, class_fields);
                    }
                    if let Some(fb) = finally_block {
                        Self::discover_fields_recursive(fb, class_fields);
                    }
                }
                _ => {}
            }
        }
    }

    /// Recursively parse indented block lines into statements
    fn parse_block(
        &self,
        lines: &[(usize, String)],
        fn_name: &str,
        mem_norm: &MemoryNormalizer,
    ) -> Vec<Statement> {
        let mut stmts = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let (indent, ref raw_text) = lines[i];
            let stripped_comment = Self::strip_trailing_comment(raw_text).to_string();
            let text = &stripped_comment;
            let trimmed_text = text.trim();
            if trimmed_text.is_empty() || trimmed_text.starts_with("//") || trimmed_text.starts_with('#') || trimmed_text == "}" {
                i += 1;
                continue;
            }

            // Skip docstrings and docstring tags
            if trimmed_text.starts_with("\"\"\"") || trimmed_text.starts_with("'''") {
                let quote = if trimmed_text.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
                if trimmed_text.len() > 3 && trimmed_text[3..].contains(quote) {
                    i += 1;
                    continue;
                }
                i += 1;
                while i < lines.len() {
                    let (_, ref l_raw) = lines[i];
                    let l = l_raw.trim();
                    if l.contains(quote) {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }

            if trimmed_text.starts_with(':') && (trimmed_text.starts_with(":param") || trimmed_text.starts_with(":type") || trimmed_text.starts_with(":return") || trimmed_text.starts_with(":rtype")) {
                i += 1;
                continue;
            }

            // Local function imports: `import foo`, `from foo import bar`, `use std::...`
            if trimmed_text.starts_with("import ") || trimmed_text.starts_with("from ") || trimmed_text.starts_with("use ") {
                i += 1;
                continue;
            }

            // Nested function or local class/struct definition inside a block: consume its indented body
            if self.fn_re.is_match(trimmed_text)
                || Self::parse_class_header(trimmed_text).is_some()
                || trimmed_text.starts_with("struct ")
            {
                let (_nested_body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                i = next_i;
                continue;
            }

            // 0. First-class polyglot Annotation / Decorator binding (`__omni_annotate__::<fn>::<raw>`)
            if let Some(rest) = text.strip_prefix("__omni_annotate__::") {
                if let Some((target_sym, raw_ann)) = rest.split_once("::") {
                    if let Some(annotation) = Self::parse_annotation_line(raw_ann) {
                        stmts.push(Statement::AnnotationBinding {
                            target_symbol: target_sym.to_string(),
                            annotation,
                        });
                        i += 1;
                        continue;
                    }
                }
            }

            // Pattern Matching: `match expr { ... }`
            if trimmed_text.starts_with("match ") {
                let target = trimmed_text
                    .trim_start_matches("match ")
                    .trim_end_matches('{')
                    .trim()
                    .to_string();

                let mut arms = Vec::new();
                let mut next_i = i + 1;

                while next_i < lines.len() {
                    let (arm_indent, ref arm_text) = lines[next_i];
                    if arm_indent <= indent {
                        break;
                    }

                    let arm_trimmed = arm_text.trim();
                    if arm_trimmed.is_empty() || arm_trimmed.starts_with("//") || arm_trimmed.starts_with('#') {
                        next_i += 1;
                        continue;
                    }

                    if arm_trimmed == "}" {
                        next_i += 1;
                        continue;
                    }

                    if let Some((pat, rest)) = arm_trimmed.split_once("=>") {
                        let pat_clean = pat.trim().to_string();
                        let rest_trimmed = rest.trim();
                        let arm_body = if rest_trimmed.is_empty() || rest_trimmed == "{" {
                            let (body_stmts, after_arm) =
                                self.consume_indented_block(lines, next_i + 1, arm_indent, fn_name, mem_norm);
                            next_i = after_arm;
                            body_stmts
                        } else {
                            let single_stmt = rest_trimmed
                                .trim_matches(|c| c == '{' || c == '}')
                                .trim_end_matches(',')
                                .trim_end_matches(';')
                                .trim();
                            let parsed = self.parse_block(
                                &[(arm_indent + 4, single_stmt.to_string())],
                                fn_name,
                                mem_norm,
                            );
                            next_i += 1;
                            parsed
                        };

                        arms.push(MatchArm {
                            pattern: pat_clean,
                            body: arm_body,
                        });
                        continue;
                    }
                    next_i += 1;
                }

                stmts.push(Statement::Match { target, arms });
                i = next_i;
                continue;
            }

            // Polyglot Try / Except / Catch / Finally
            if trimmed_text == "try:" || trimmed_text == "try" || trimmed_text == "try {" {
                let (try_block, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                let mut catch_clauses = Vec::new();
                let mut finally_block = None;

                while next_i < lines.len() {
                    let (clause_indent, ref clause_raw) = lines[next_i];
                    if clause_indent != indent {
                        break;
                    }
                    let clause_trimmed = Self::strip_trailing_comment(clause_raw)
                        .trim()
                        .trim_start_matches('}')
                        .trim()
                        .to_string();
                    if clause_trimmed.starts_with("except") || clause_trimmed.starts_with("catch") {
                        let is_catch_kw = clause_trimmed.starts_with("catch");
                        let (exc_part, inline_after) = if clause_trimmed.ends_with('{') || !clause_trimmed.contains(':') {
                            (clause_trimmed.as_str(), "")
                        } else {
                            clause_trimmed.split_once(':').unwrap_or((&clause_trimmed, ""))
                        };
                        let header = exc_part
                            .trim_start_matches("except")
                            .trim_start_matches("catch")
                            .trim_end_matches('{')
                            .trim();

                        let (exc_type, var_name) = if header.is_empty() {
                            (None, None)
                        } else if let Some((t, v)) = header.split_once(" as ") {
                            let clean_t = t.trim().trim_matches(|c| c == '(' || c == ')').split(',').next().unwrap_or("").trim().to_string();
                            (
                                if clean_t.is_empty() { None } else { Some(clean_t) },
                                Some(v.trim().to_string()),
                            )
                        } else {
                            let inner = header.trim().trim_matches(|c| c == '(' || c == ')').trim();
                            if let Some((v, t)) = inner.split_once(':') {
                                // TypeScript / Rust / Kotlin `catch (e: ValueError)`
                                (Some(t.trim().to_string()), Some(v.trim().to_string()))
                            } else {
                                let toks: Vec<&str> = inner
                                    .split_whitespace()
                                    .filter(|w| !matches!(*w, "const" | "final" | "&"))
                                    .collect();
                                if toks.len() >= 2 {
                                    // Java / C++ / C# `catch (ValueError e)` or `catch (const std::exception& e)`
                                    let ty = toks[..toks.len() - 1].join(" ").trim_end_matches('&').trim().to_string();
                                    let vn = toks[toks.len() - 1].trim_start_matches('&').trim().to_string();
                                    (Some(ty), Some(vn))
                                } else if is_catch_kw && header.starts_with('(') && inner.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) {
                                    // JS `catch (err) {`
                                    (None, Some(inner.to_string()))
                                } else {
                                    let first_ty = inner.split(',').next().unwrap_or(inner).trim();
                                    (Some(first_ty.to_string()), None)
                                }
                            }
                        };

                        // Check if single-line except like `except: exit()` or `except: pass`
                        let after = inline_after.trim();
                        if !after.is_empty() && after != "{" {
                            if after != "pass" {
                                let inline_lines = vec![(indent + 4, after.to_string())];
                                let body = self.parse_block(&inline_lines, fn_name, mem_norm);
                                catch_clauses.push(CatchClause {
                                    exception_type: exc_type,
                                    var_name,
                                    body,
                                });
                            } else {
                                catch_clauses.push(CatchClause {
                                    exception_type: exc_type,
                                    var_name,
                                    body: Vec::new(),
                                });
                            }
                            next_i += 1;
                            continue;
                        }

                        let (catch_body, after_catch_i) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        catch_clauses.push(CatchClause {
                            exception_type: exc_type,
                            var_name,
                            body: catch_body,
                        });
                        next_i = after_catch_i;
                    } else if clause_trimmed.starts_with("finally") {
                        let (fin_body, after_fin_i) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        finally_block = Some(fin_body);
                        next_i = after_fin_i;
                    } else {
                        break;
                    }
                }

                stmts.push(Statement::TryCatch {
                    try_block,
                    catch_clauses,
                    finally_block,
                });
                i = next_i;
                continue;
            }

            // 1. If statement with optional elif/else
            if let Some(caps) = self.if_re.captures(text) {
                let condition = Self::unwrap_outer_balanced_parens(caps.get(1).map(|m| m.as_str()).unwrap_or(""));

                let (then_branch, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                let mut else_branch = None;

                // Check for `elif` chain and terminal `else` at the same indentation level
                let mut current_else = &mut else_branch;
                while next_i < lines.len() && lines[next_i].0 == indent {
                    let next_text = lines[next_i].1.trim();
                    if let Some(elif_caps) = self.elif_re.captures(next_text) {
                        let elif_cond = Self::unwrap_outer_balanced_parens(elif_caps.get(1).map(|m| m.as_str()).unwrap_or(""));
                        let (elif_then, after_elif) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        *current_else = Some(vec![Statement::If {
                            condition: elif_cond,
                            then_branch: elif_then,
                            else_branch: None,
                        }]);
                        next_i = after_elif;
                        if let Some(ref mut vec) = current_else {
                            if let Some(Statement::If { ref mut else_branch, .. }) = vec.first_mut() {
                                current_else = else_branch;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else if self.else_re.is_match(next_text) {
                        let (else_body, after_else) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        *current_else = Some(else_body);
                        next_i = after_else;
                        break;
                    } else {
                        break;
                    }
                }

                stmts.push(Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                });
                i = next_i;
                continue;
            }

            // 2. While loop (with optional Python `while ... else:`)
            if let Some(caps) = self.while_re.captures(text) {
                let condition = Self::unwrap_outer_balanced_parens(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
                let (body, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                if next_i < lines.len() && lines[next_i].0 == indent && self.else_re.is_match(lines[next_i].1.trim()) {
                    let (_else_body, after_else) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                    next_i = after_else;
                }
                stmts.push(Statement::WhileLoop { condition, body });
                i = next_i;
                continue;
            }

            // 3. For loop (with optional Python `for ... else:`)
            let for_match = if let Some(caps) = self.c_for_re.captures(text) {
                Some((caps[1].to_string(), caps[2].trim().to_string()))
            } else if let Some(caps) = self.paren_for_re.captures(text) {
                let cnt = caps.get(2).or_else(|| caps.get(3)).unwrap().as_str().trim().to_string();
                Some((caps[1].to_string(), cnt))
            } else if let Some(caps) = self.plain_for_re.captures(text) {
                let cnt = caps.get(2).or_else(|| caps.get(3)).unwrap().as_str().trim().to_string();
                Some((caps[1].to_string(), cnt))
            } else {
                None
            };

            if let Some((var, count)) = for_match {
                let (body, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                if next_i < lines.len() && lines[next_i].0 == indent && self.else_re.is_match(lines[next_i].1.trim()) {
                    let (_else_body, after_else) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                    next_i = after_else;
                }
                stmts.push(Statement::ForLoop { var, count, body });
                i = next_i;
                continue;
            }

            // 3b. Go / general C `for` forms (`for {`, `for cond {`, `for init; cond; step {`)
            if text == "for" || text.starts_with("for ") || text.starts_with("for(") {
                let raw_hdr = text
                    .trim_start_matches("for")
                    .trim()
                    .trim_end_matches(':')
                    .trim_end_matches("do")
                    .trim();
                let hdr = if raw_hdr.starts_with('(') && raw_hdr.ends_with(')') {
                    raw_hdr[1..raw_hdr.len() - 1].trim()
                } else {
                    raw_hdr
                };
                if hdr.is_empty() {
                    let (body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                    stmts.push(Statement::WhileLoop {
                        condition: "true".to_string(),
                        body,
                    });
                    i = next_i;
                    continue;
                } else if hdr.contains(';') {
                    let clauses: Vec<&str> = hdr.splitn(3, ';').collect();
                    let init_s = clauses.first().copied().unwrap_or("").trim();
                    let cond_s = clauses.get(1).copied().unwrap_or("").trim();
                    let step_s = clauses.get(2).copied().unwrap_or("").trim();
                    if !init_s.is_empty() {
                        let init_stmts = self.parse_block(&[(indent, init_s.to_string())], fn_name, mem_norm);
                        stmts.extend(init_stmts);
                    }
                    let (mut body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                    if !step_s.is_empty() {
                        let step_stmts = self.parse_block(&[(indent + 4, step_s.to_string())], fn_name, mem_norm);
                        body.extend(step_stmts);
                    }
                    stmts.push(Statement::WhileLoop {
                        condition: if cond_s.is_empty() { "true".to_string() } else { cond_s.to_string() },
                        body,
                    });
                    i = next_i;
                    continue;
                } else if !hdr.contains(" in ") && !hdr.contains(" of ") && !hdr.contains(":=") {
                    let (body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                    stmts.push(Statement::WhileLoop {
                        condition: hdr.to_string(),
                        body,
                    });
                    i = next_i;
                    continue;
                }
            }

            // 4. Print
            if let Some(caps) = self.print_re.captures(text) {
                stmts.push(Statement::Print {
                    expr: caps[1].trim().to_string(),
                });
                i += 1;
                continue;
            }
            if trimmed_text.starts_with("cout") && trimmed_text.contains("<<") {
                let parts: Vec<&str> = trimmed_text.split("<<").collect();
                if parts.len() >= 2 {
                    let expr_part = parts[1].trim().trim_end_matches(';').trim().to_string();
                    stmts.push(Statement::Print { expr: expr_part });
                    i += 1;
                    continue;
                }
            }

            // 5. Return
            if let Some(caps) = self.return_re.captures(text) {
                let expr = caps.get(1).map(|m| m.as_str().trim().to_string());
                stmts.push(Statement::Return { expr });
                i += 1;
                continue;
            }

            // 5b. With context manager
            if let Some(caps) = self.with_re.captures(text) {
                let context_expr = caps[1].trim().to_string();
                let var_alias = caps.get(2).map(|m| m.as_str().trim().to_string());
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                stmts.push(Statement::WithContext {
                    context_expr,
                    var_alias,
                    body,
                });
                i = next_i;
                continue;
            }

            // 6. Explicit Memory Operations (e.g. `temp_buf = alloc(2048)`, `defer free(temp_buf)`)
            if text.contains("alloc(") || text.contains("free(") || text.trim().starts_with("defer ") {
                if let Some(op) = mem_norm.normalize_line(text, fn_name) {
                    match op {
                        UnifiedMemOp::Alloc { .. } | UnifiedMemOp::Free { .. } => {
                            let is_deferred = text.trim().starts_with("defer ");
                            stmts.push(Statement::MemOp { op, is_deferred });
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            // 7. Channel Creation: `ch := make(chan int, 8)`, `ch = Channel(10)`
            if let Some(caps) = self.make_chan_re.captures(text) {
                let is_scoped = caps.get(1).is_some() || text.contains(":=");
                let target = caps[2].trim().to_string();
                let cap_str = caps.get(3).or_else(|| caps.get(4)).map(|m| m.as_str()).unwrap_or("1");
                let capacity = cap_str.parse::<usize>().unwrap_or(1);
                stmts.push(Statement::ChanMake {
                    target,
                    capacity,
                    elem_size: 8,
                    is_scoped_decl: is_scoped,
                });
                i += 1;
                continue;
            }

            // 8. Channel Send: `ch <- 42`, `ch.send(42)`
            if let Some(caps) = self.chan_send_re.captures(text) {
                let channel = caps.get(1).or_else(|| caps.get(3)).unwrap().as_str().trim().to_string();
                let expr = caps.get(2).or_else(|| caps.get(4)).unwrap().as_str().trim().to_string();
                stmts.push(Statement::ChanSend { channel, expr });
                i += 1;
                continue;
            }

            // 9. Channel Receive: `val := <-ch`, `val = <-ch`, `val = ch.recv()`, `<-ch`
            if let Some(caps) = self.chan_recv_re.captures(text) {
                let is_scoped = caps.get(1).is_some() || text.contains(":=");
                let target = caps.get(2).map(|m| m.as_str().trim().to_string());
                let channel = caps.get(3).or_else(|| caps.get(4)).unwrap().as_str().trim().to_string();
                stmts.push(Statement::ChanRecv {
                    target,
                    channel,
                    is_scoped_decl: is_scoped,
                });
                i += 1;
                continue;
            }

            // 10. Polyglot List / Slice Init: `items = [10, 20, 30]`, `items := []`
            if let Some(caps) = self.list_init_re.captures(text) {
                let is_scoped = caps.get(1).is_some() || text.contains(":=");
                let target = caps[2].trim().to_string();
                let inner = caps[3].trim();
                let elements = if inner.is_empty() {
                    Vec::new()
                } else {
                    Self::split_respecting_quotes(inner, ',')
                };
                stmts.push(Statement::ListInit {
                    target,
                    elements,
                    is_scoped_decl: is_scoped,
                });
                i += 1;
                continue;
            }

            // 11. Polyglot Map / Dict Init: `m = {"k": 10, "v": 20}`, `m := {}`
            if let Some(caps) = self.map_init_re.captures(text) {
                let is_scoped = caps.get(1).is_some() || text.contains(":=");
                let target = caps[2].trim().to_string();
                let inner = caps[3].trim();
                let entries = if inner.is_empty() {
                    Vec::new()
                } else {
                    Self::split_respecting_quotes(inner, ',')
                        .into_iter()
                        .filter_map(|pair| {
                            let pair = pair.trim();
                            if pair.is_empty() {
                                return None;
                            }
                            if let Some((k, v)) = pair.split_once(':') {
                                Some((
                                    k.trim().trim_matches('"').trim_matches('\'').to_string(),
                                    v.trim().to_string(),
                                ))
                            } else if let Some((k, v)) = pair.split_once("=>") {
                                Some((
                                    k.trim().trim_matches('"').trim_matches('\'').to_string(),
                                    v.trim().to_string(),
                                ))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                stmts.push(Statement::MapInit {
                    target,
                    entries,
                    is_scoped_decl: is_scoped,
                });
                i += 1;
                continue;
            }

            // 12. Polyglot List Append: `items.append(40)`, `items.push(50)`
            if let Some(caps) = self.list_append_re.captures(text) {
                let target = caps[1].trim().to_string();
                let expr = caps[2].trim().to_string();
                stmts.push(Statement::ListAppend { target, expr });
                i += 1;
                continue;
            }

            // 13. Polyglot Index Assignment: `items[0] = 99`, `grid[1][0] = 99`, `self.items[i] = 10`, `m["key"] = 10`
            if text.contains('[') && text.contains('=') && !text.starts_with("if ") && !text.starts_with("while ") {
                let mut eq_split_byte = None;
                let t_bytes = text.as_bytes();
                let mut p_d = 0i32;
                let mut b_d = 0i32;
                let mut c_d = 0i32;
                let mut in_dq = false;
                let mut in_sq = false;
                let mut bi = 0usize;
                while bi < t_bytes.len() {
                    let b = t_bytes[bi];
                    if in_dq {
                        if b == b'\\' { bi += 2; continue; }
                        if b == b'"' { in_dq = false; }
                    } else if in_sq {
                        if b == b'\\' { bi += 2; continue; }
                        if b == b'\'' { in_sq = false; }
                    } else {
                        match b {
                            b'"' => in_dq = true,
                            b'\'' => in_sq = true,
                            b'(' => p_d += 1,
                            b')' => p_d = (p_d - 1).max(0),
                            b'[' => b_d += 1,
                            b']' => b_d = (b_d - 1).max(0),
                            b'{' => c_d += 1,
                            b'}' => c_d = (c_d - 1).max(0),
                            b'=' if p_d == 0 && b_d == 0 && c_d == 0 => {
                                let prev = if bi > 0 { t_bytes[bi - 1] } else { 0 };
                                let next = t_bytes.get(bi + 1).copied().unwrap_or(0);
                                if !matches!(prev, b'=' | b'!' | b'<' | b'>' | b':' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^')
                                    && next != b'='
                                    && next != b'>'
                                {
                                    eq_split_byte = Some(bi);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    bi += 1;
                }
                if let Some(eq_idx) = eq_split_byte {
                    let lhs_trim = text[..eq_idx].trim();
                    let rhs_trim = text[eq_idx + 1..].trim().trim_end_matches(';').trim();
                    if lhs_trim.ends_with(']') && !rhs_trim.is_empty() {
                        if let Ok(lhs_ast) = Self::parse_expr(lhs_trim) {
                            if let ExprAst::Index { target, index } = lhs_ast.node {
                                stmts.push(Statement::IndexSet {
                                    target: target.slice(lhs_trim).to_string(),
                                    index_expr: index.slice(lhs_trim).to_string(),
                                    val_expr: rhs_trim.to_string(),
                                });
                                i += 1;
                                continue;
                            }
                        }
                    }
                }
            }
            if let Some(caps) = self.index_set_re.captures(text) {
                let target = caps[1].trim().to_string();
                let index_expr = caps[2].trim().to_string();
                let val_expr = caps[3].trim().to_string();
                stmts.push(Statement::IndexSet {
                    target,
                    index_expr,
                    val_expr,
                });
                i += 1;
                continue;
            }

            // 14. Go-style Scoped Walrus Declaration (e.g. `count := 0`)
            if let Some(caps) = self.walrus_assign_re.captures(text) {
                stmts.push(Statement::Assign {
                    target: caps[1].trim().to_string(),
                    expr: caps[2].trim().to_string(),
                    idiom: IdiomFamily::GoScopedDeclaration,
                    is_scoped_decl: true,
                });
                i += 1;
                continue;
            }

            // Polyglot Tuple Literal Assignment: `DEFAULT_IP_PORT = ("127.0.0.1", 9999)`
            // Desugar into Statement::ListInit
            if let Some(caps) = self.assign_re.captures(text) {
                let target = caps[2].trim().to_string();
                let clean_expr = caps[3].trim().trim_end_matches(';').trim();
                let is_scoped = caps.get(1).is_some();

                if clean_expr.starts_with('(') && clean_expr.ends_with(')') && clean_expr.contains(',') && !clean_expr.starts_with("(i32)") && !clean_expr.starts_with("(i64)") {
                    let wrapped = Self::unwrap_outer_balanced_parens(clean_expr);
                    let raw_parts = Self::split_respecting_quotes(&wrapped, ',');
                    if raw_parts.len() >= 2 {
                        let elements: Vec<String> = raw_parts
                            .into_iter()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if !elements.is_empty() {
                            stmts.push(Statement::ListInit {
                                target,
                                elements,
                                is_scoped_decl: is_scoped,
                            });
                            i += 1;
                            continue;
                        }
                    }
                }
            }

            // Polyglot Tuple Unpack: `public_key, private_key = rsa.newkeys(1024)`, `client, addr = server.accept()`
            if text.contains('=') && !text.contains("==") && !text.contains("!=") && !text.contains("<=") && !text.contains(">=") && !text.starts_with("if ") {
                let assign_op = if text.contains(":=") { ":=" } else { "=" };
                if let Some((lhs, rhs)) = text.split_once(assign_op) {
                    let mut lhs_clean = lhs
                        .trim()
                        .trim_start_matches("let ")
                        .trim_start_matches("var ")
                        .trim_start_matches("const ")
                        .trim();
                    if (lhs_clean.starts_with('(') && lhs_clean.ends_with(')'))
                        || (lhs_clean.starts_with('[') && lhs_clean.ends_with(']'))
                    {
                        lhs_clean = lhs_clean[1..lhs_clean.len() - 1].trim();
                    }
                    if lhs_clean.contains(',') && !lhs_clean.contains('(') && !lhs_clean.contains('[') {
                        let targets: Vec<String> = lhs_clean
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if targets.len() >= 2 {
                            let clean_rhs = rhs.trim().trim_end_matches(';').trim().to_string();
                            stmts.push(Statement::TupleUnpack {
                                targets,
                                expr: clean_rhs,
                                is_scoped_decl: assign_op == ":=" || lhs.starts_with("let ") || lhs.starts_with("var "),
                            });
                            i += 1;
                            continue;
                        }
                    }
                }
            }

            // Compound Assignment Desugaring: `x += y`, `x -= y`, `x *= y` -> `x = x + (y)`
            let mut handled_compound = false;
            for (comp_op, bin_op) in &[("+=", "+"), ("-=", "-"), ("*=", "*"), ("//=", "//"), ("/=", "/"), ("%=", "%"), ("<<=", "<<"), (">>=", ">>"), ("|=", "|"), ("&=", "&"), ("^=", "^")] {
                if let Some((lhs, rhs)) = text.split_once(comp_op) {
                    let lhs_trim = lhs.trim();
                    let rhs_trim = rhs.trim().trim_end_matches(';').trim();
                    if !lhs_trim.is_empty()
                        && lhs_trim.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                        && !rhs_trim.is_empty()
                    {
                        stmts.push(Statement::Assign {
                            target: lhs_trim.to_string(),
                            expr: format!("{} {} ({})", lhs_trim, bin_op, rhs_trim),
                            idiom: IdiomFamily::Standard,
                            is_scoped_decl: false,
                        });
                        i += 1;
                        handled_compound = true;
                        break;
                    }
                }
            }
            if handled_compound {
                continue;
            }

            // 8. Standard Assignment (e.g. `x = 5`, `int count = 10`, `let count = 0`)
            if let Some(caps) = self.assign_re.captures(text) {
                let is_scoped = caps.get(1).is_some();
                let clean_expr = caps[3].trim().trim_end_matches(';').trim().to_string();
                stmts.push(Statement::Assign {
                    target: caps[2].trim().to_string(),
                    expr: clean_expr,
                    idiom: IdiomFamily::Standard,
                    is_scoped_decl: is_scoped,
                });
                i += 1;
                continue;
            }

            // 9. Go Concurrency Spawn: `go <call_or_closure>`
            if text.starts_with("go ") {
                let call_expr = text.trim_start_matches("go ").trim().to_string();
                stmts.push(Statement::GoSpawn {
                    call_expr,
                    idiom: IdiomFamily::GoScopedDeclaration,
                });
                i += 1;
                continue;
            }

            // 10. Intent Block
            if text.starts_with("intent") {
                stmts.push(Statement::Intent {
                    instruction: text.clone(),
                });
                i += 1;
                continue;
            }

            // 11. Fallback Concrete / Polyglot Statement
            let idiom = if text.contains("=>") {
                IdiomFamily::ClosureLambda
            } else if text.contains("match ") || text.contains("case ") {
                IdiomFamily::PatternArm
            } else {
                IdiomFamily::Unknown("polyglot".to_string())
            };

            stmts.push(Statement::Concrete {
                code: text.clone(),
                idiom,
            });
            i += 1;
        }

        stmts
    }

    /// Helper to collect lines indented deeper than parent and parse them recursively
    fn consume_indented_block(
        &self,
        lines: &[(usize, String)],
        start_idx: usize,
        parent_indent: usize,
        fn_name: &str,
        mem_norm: &MemoryNormalizer,
    ) -> (Vec<Statement>, usize) {
        let mut nested = Vec::new();
        let mut j = start_idx;

        while j < lines.len() {
            if lines[j].0 > parent_indent {
                nested.push(lines[j].clone());
                j += 1;
            } else {
                break;
            }
        }

        let body = self.parse_block(&nested, fn_name, mem_norm);
        (body, j)
    }

    /// Parses an arbitrary polyglot expression into a span-preserving [`SpannedExpr`] AST
    /// using the [`PolyglotLexicon`] binding-power table and postfix appliers (`.`, `[]`, `()`).
    pub fn parse_expr(src: &str) -> Result<SpannedExpr, String> {
        let tokens = tokenize_expr(src)?;
        if tokens.is_empty() {
            return Err("empty expression".to_string());
        }
        let mut parser = PrattParser { src, tokens: &tokens, pos: 0 };
        let expr = parser.parse_bp(0)?;
        if parser.pos != tokens.len() {
            return Err(format!(
                "unexpected trailing token '{}' at byte {}",
                tokens[parser.pos].text, tokens[parser.pos].span.0
            ));
        }
        Ok(expr)
    }

    /// Splits `src` at its lowest-precedence top-level binary operator using [`Self::parse_expr`],
    /// returning `(lhs_slice, op, rhs_slice)` while respecting all parentheses, quotes, generics,
    /// unary operators, right-associative `**`, and postfix appliers.
    pub fn pratt_split_top_binary(src: &str) -> Option<(String, String, String)> {
        let ast = Self::parse_expr(src).ok()?;
        if let ExprAst::Binary { op, lhs, rhs } = ast.node {
            Some((lhs.slice(src).to_string(), op, rhs.slice(src).to_string()))
        } else {
            None
        }
    }
}

/// Span-preserving wrapper around an [`ExprAst`] node.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedExpr {
    pub node: ExprAst,
    pub span: (usize, usize),
}

impl SpannedExpr {
    pub fn slice<'a>(&self, src: &'a str) -> &'a str {
        src.get(self.span.0..self.span.1).unwrap_or("").trim()
    }
}

/// Parsed argument in a function/method/constructor invocation (`pos_val` or `key=val`).
#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: SpannedExpr,
    pub raw: String,
}

/// Polyglot expression AST produced by [`OmniParser::parse_expr`].
#[derive(Debug, Clone, PartialEq)]
pub enum ExprAst {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    NullLit,
    StrLit(String),
    Ident(String),
    ListLit(Vec<SpannedExpr>),
    MapLit(Vec<(SpannedExpr, SpannedExpr)>),
    Unary {
        op: String,
        rhs: Box<SpannedExpr>,
    },
    Binary {
        op: String,
        lhs: Box<SpannedExpr>,
        rhs: Box<SpannedExpr>,
    },
    Ternary {
        cond: Box<SpannedExpr>,
        then_br: Box<SpannedExpr>,
        else_br: Box<SpannedExpr>,
    },
    Member {
        target: Box<SpannedExpr>,
        field: String,
    },
    Index {
        target: Box<SpannedExpr>,
        index: Box<SpannedExpr>,
    },
    Call {
        callee: Box<SpannedExpr>,
        args: Vec<CallArg>,
    },
    New {
        class_name: String,
        args: Vec<CallArg>,
    },
    Cast {
        expr: Box<SpannedExpr>,
        target_type: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum LexTokenKind {
    Int(i64),
    Float(f64),
    Str(String),
    Ident,
    Op,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Question,
    Dot,
}

#[derive(Debug, Clone)]
struct LexToken<'a> {
    kind: LexTokenKind,
    text: &'a str,
    span: (usize, usize),
}

/// Numeric binding-power lexicon for polyglot operators and postfix appliers.
pub struct PolyglotLexicon;

impl PolyglotLexicon {
    pub fn prefix_rbp(op: &str) -> Option<u8> {
        match op {
            "not" => Some(18),
            "-" | "+" | "!" | "~" => Some(60),
            "new" => Some(85),
            _ => None,
        }
    }

    pub fn infix_bp(op: &str) -> Option<(u8, u8)> {
        match op {
            "?" | "if" => Some((4, 3)),
            "|>" => Some((6, 7)),
            "||" | "or" => Some((10, 11)),
            "&&" | "and" => Some((14, 15)),
            "==" | "!=" | "===" | "!==" | "~=" | "<" | "<=" | ">" | ">=" | "is" => Some((20, 21)),
            "|" => Some((26, 27)),
            "^" => Some((28, 29)),
            "&" => Some((30, 31)),
            "<<" | ">>" | ">>>" => Some((36, 37)),
            "+" | "-" => Some((40, 41)),
            "*" | "/" | "//" | "%" => Some((50, 51)),
            "as" => Some((55, 56)),
            "**" => Some((66, 65)),
            _ => None,
        }
    }

    pub const POSTFIX_LBP: u8 = 80;
}

fn is_ident_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_cont_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn tokenize_expr(src: &str) -> Result<Vec<LexToken<'_>>, String> {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();

    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Prefixed string literals: f"...", r"...", b"..."
        if (b == b'f' || b == b'r' || b == b'b' || b == b'F' || b == b'R' || b == b'B')
            && i + 1 < len
            && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'')
        {
            let start = i;
            let quote = bytes[i + 1];
            i += 2;
            let mut esc = false;
            let mut closed = false;
            while i < len {
                let c = bytes[i];
                i += 1;
                if esc {
                    esc = false;
                    continue;
                }
                if c == b'\\' {
                    esc = true;
                } else if c == quote {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err("unterminated prefixed string literal".to_string());
            }
            let raw = &src[start..i];
            tokens.push(LexToken {
                kind: LexTokenKind::Str(raw.to_string()),
                text: raw,
                span: (start, i),
            });
            continue;
        }

        // Standard string literals: "...", '...', `...`
        if b == b'"' || b == b'\'' || b == b'`' {
            let start = i;
            let quote = b;
            i += 1;
            let mut unescaped = String::new();
            let mut esc = false;
            let mut closed = false;
            while i < len {
                let ch = src[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if esc {
                    match ch {
                        'n' => unescaped.push('\n'),
                        't' => unescaped.push('\t'),
                        'r' => unescaped.push('\r'),
                        '0' => unescaped.push('\0'),
                        other => unescaped.push(other),
                    }
                    esc = false;
                    continue;
                }
                if ch == '\\' {
                    esc = true;
                } else if ch as u32 == quote as u32 {
                    closed = true;
                    break;
                } else {
                    unescaped.push(ch);
                }
            }
            if !closed {
                return Err("unterminated string literal".to_string());
            }
            tokens.push(LexToken {
                kind: LexTokenKind::Str(unescaped),
                text: &src[start..i],
                span: (start, i),
            });
            continue;
        }

        // Numeric literals (hex, bin, oct, float, int)
        let is_leading_dot_float = b == b'.'
            && i + 1 < len
            && bytes[i + 1].is_ascii_digit()
            && !matches!(
                tokens.last().map(|t: &LexToken<'_>| &t.kind),
                Some(LexTokenKind::Ident | LexTokenKind::RParen | LexTokenKind::RBracket | LexTokenKind::Int(_))
            );

        if b.is_ascii_digit() || is_leading_dot_float {
            let start = i;
            if b == b'0' && i + 1 < len && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
                let radix = match bytes[i + 1] {
                    b'x' | b'X' => 16,
                    b'b' | b'B' => 2,
                    _ => 8,
                };
                i += 2;
                let digits_start = i;
                while i < len
                    && (bytes[i] == b'_'
                        || (radix == 16 && bytes[i].is_ascii_hexdigit())
                        || (radix == 8 && (b'0'..=b'7').contains(&bytes[i]))
                        || (radix == 2 && (bytes[i] == b'0' || bytes[i] == b'1')))
                {
                    i += 1;
                }
                let clean: String = src[digits_start..i].chars().filter(|&c| c != '_').collect();
                // Skip optional integer type suffixes (`u64`, `i64`, `L`, `UL`, etc.)
                while i < len && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let val = i64::from_str_radix(&clean, radix)
                    .or_else(|_| u64::from_str_radix(&clean, radix).map(|u| u as i64))
                    .map_err(|e| format!("invalid radix literal: {}", e))?;
                tokens.push(LexToken {
                    kind: LexTokenKind::Int(val),
                    text: &src[start..i],
                    span: (start, i),
                });
                continue;
            }

            let mut is_float = false;
            if b == b'.' {
                is_float = true;
                i += 1;
            }
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            // Fractional part (avoiding Rust `..` range operator and method call `1.to_string()`)
            if !is_float
                && i < len
                && bytes[i] == b'.'
                && !(i + 1 < len && bytes[i + 1] == b'.')
                && !(i + 1 < len && is_ident_start_byte(bytes[i + 1]))
            {
                is_float = true;
                i += 1;
                while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                    i += 1;
                }
            }
            // Scientific exponent (`e` / `E`)
            if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mut exp_i = i + 1;
                if exp_i < len && (bytes[exp_i] == b'+' || bytes[exp_i] == b'-') {
                    exp_i += 1;
                }
                if exp_i < len && bytes[exp_i].is_ascii_digit() {
                    is_float = true;
                    i = exp_i + 1;
                    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                        i += 1;
                    }
                }
            }
            let num_end = i;
            // Optional Rust/C suffix (`f64`, `f32`, `i64`, `u64`, `usize`, `L`, `f`)
            if i < len && is_ident_start_byte(bytes[i]) {
                let suf_start = i;
                while i < len && is_ident_cont_byte(bytes[i]) {
                    i += 1;
                }
                let suf = &src[suf_start..i];
                if matches!(suf, "f" | "F" | "f32" | "f64" | "d" | "D") {
                    is_float = true;
                }
            }
            let clean: String = src[start..num_end].chars().filter(|&c| c != '_').collect();
            if is_float {
                let fv = clean.parse::<f64>().map_err(|e| format!("invalid float literal: {}", e))?;
                tokens.push(LexToken {
                    kind: LexTokenKind::Float(fv),
                    text: &src[start..i],
                    span: (start, i),
                });
            } else {
                let iv = clean
                    .parse::<i64>()
                    .or_else(|_| clean.parse::<u64>().map(|u| u as i64))
                    .map_err(|e| format!("invalid int literal: {}", e))?;
                tokens.push(LexToken {
                    kind: LexTokenKind::Int(iv),
                    text: &src[start..i],
                    span: (start, i),
                });
            }
            continue;
        }

        // Identifiers (including Rust/C++ `::` path segments, `::<T>` turbofish, and `macro!` before `(`/`[`)
        if is_ident_start_byte(b) {
            let start = i;
            i += 1;
            while i < len && is_ident_cont_byte(bytes[i]) {
                i += 1;
            }
            // Absorb `::Ident` and `::<...>` path segments
            loop {
                if i + 1 < len && bytes[i] == b':' && bytes[i + 1] == b':' {
                    if i + 2 < len && is_ident_start_byte(bytes[i + 2]) {
                        i += 2;
                        while i < len && is_ident_cont_byte(bytes[i]) {
                            i += 1;
                        }
                        continue;
                    } else if i + 2 < len && bytes[i + 2] == b'<' {
                        // Turbofish `::<...>`
                        let mut depth = 1usize;
                        let mut j = i + 3;
                        while j < len && depth > 0 {
                            if bytes[j] == b'<' {
                                depth += 1;
                            } else if bytes[j] == b'>' {
                                depth -= 1;
                            }
                            j += 1;
                        }
                        if depth == 0 {
                            i = j;
                            continue;
                        }
                    }
                }
                break;
            }
            // Rust macro bang: `println!(...)` or `vec![...]`
            if i + 1 < len && bytes[i] == b'!' && (bytes[i + 1] == b'(' || bytes[i + 1] == b'[') {
                i += 1;
            }
            tokens.push(LexToken {
                kind: LexTokenKind::Ident,
                text: &src[start..i],
                span: (start, i),
            });
            continue;
        }

        // Non-ASCII identifier start (Unicode identifiers)
        if !b.is_ascii() {
            let start = i;
            for ch in src[i..].chars() {
                if ch.is_alphanumeric() || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            if i == start {
                return Err(format!("unexpected non-ASCII character at byte {}", start));
            }
            tokens.push(LexToken {
                kind: LexTokenKind::Ident,
                text: &src[start..i],
                span: (start, i),
            });
            continue;
        }

        // 3-character operators
        if i + 2 < len {
            let tri = &src[i..i + 3];
            if matches!(tri, "===" | "!==" | ">>>" | "..." | "..=") {
                if tri == "..." || tri == "..=" {
                    return Err(format!("unsupported range/spread token '{}'", tri));
                }
                tokens.push(LexToken {
                    kind: LexTokenKind::Op,
                    text: tri,
                    span: (i, i + 3),
                });
                i += 3;
                continue;
            }
        }

        // 2-character operators
        if i + 1 < len {
            let duo = &src[i..i + 2];
            if matches!(duo, "=>" | ".." | "<-" | ":=") {
                return Err(format!("non-scalar expression operator '{}'", duo));
            }
            if duo == "?." {
                tokens.push(LexToken {
                    kind: LexTokenKind::Dot,
                    text: duo,
                    span: (i, i + 2),
                });
                i += 2;
                continue;
            }
            if matches!(duo, "**" | "//" | "==" | "!=" | "~=" | "<=" | ">=" | "<<" | ">>" | "&&" | "||" | "|>") {
                tokens.push(LexToken {
                    kind: LexTokenKind::Op,
                    text: duo,
                    span: (i, i + 2),
                });
                i += 2;
                continue;
            }
        }

        // Single-character delimiters & operators
        let single = &src[i..i + 1];
        let kind = match b {
            b'(' => LexTokenKind::LParen,
            b')' => LexTokenKind::RParen,
            b'[' => LexTokenKind::LBracket,
            b']' => LexTokenKind::RBracket,
            b'{' => LexTokenKind::LBrace,
            b'}' => LexTokenKind::RBrace,
            b',' => LexTokenKind::Comma,
            b':' => LexTokenKind::Colon,
            b'?' => LexTokenKind::Question,
            b'.' => LexTokenKind::Dot,
            b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^' | b'~' | b'!' | b'<' | b'>' | b'=' => {
                LexTokenKind::Op
            }
            _ => return Err(format!("unexpected character '{}' at byte {}", single, i)),
        };
        tokens.push(LexToken {
            kind,
            text: single,
            span: (i, i + 1),
        });
        i += 1;
    }

    Ok(tokens)
}

fn is_known_generic_type_ident(s: &str) -> bool {
    let base = s.rsplit("::").next().unwrap_or(s);
    base.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        || matches!(
            base,
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "int"
                | "float"
                | "double"
                | "bool"
                | "str"
                | "string"
                | "number"
                | "any"
                | "void"
                | "char"
        )
}

struct PrattParser<'a> {
    src: &'a str,
    tokens: &'a [LexToken<'a>],
    pos: usize,
}

impl<'a> PrattParser<'a> {
    fn peek(&self) -> Option<&'a LexToken<'a>> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Result<&'a LexToken<'a>, String> {
        let tok = self
            .tokens
            .get(self.pos)
            .ok_or_else(|| "unexpected end of expression".to_string())?;
        self.pos += 1;
        Ok(tok)
    }

    fn expect_kind(&mut self, expected: &LexTokenKind) -> Result<&'a LexToken<'a>, String> {
        let tok = self.next()?;
        if &tok.kind != expected {
            return Err(format!("expected {:?}, got '{}'", expected, tok.text));
        }
        Ok(tok)
    }

    /// Lookahead for generic call type arguments `<T, U>(...)`: returns index AFTER matching `>` if valid.
    fn try_scan_generic_call_args(&self) -> Option<usize> {
        let first = self.tokens.get(self.pos)?;
        if first.text != "<" {
            return None;
        }
        let mut idx = self.pos + 1;
        let mut depth = 1usize;
        let mut saw_type_ident = false;
        while idx < self.tokens.len() && depth > 0 {
            let t = &self.tokens[idx];
            match (&t.kind, t.text) {
                (LexTokenKind::Op, "<") => depth += 1,
                (LexTokenKind::Op, ">") => depth -= 1,
                (LexTokenKind::Op, ">>") if depth >= 2 => depth -= 2,
                (LexTokenKind::Ident, name) => {
                    if !is_known_generic_type_ident(name) {
                        return None;
                    }
                    saw_type_ident = true;
                }
                (LexTokenKind::Comma, _)
                | (LexTokenKind::LBracket, _)
                | (LexTokenKind::RBracket, _)
                | (LexTokenKind::Op, "*" | "&") => {}
                _ => return None,
            }
            idx += 1;
        }
        if depth == 0 && saw_type_ident {
            if matches!(self.tokens.get(idx).map(|t| &t.kind), Some(LexTokenKind::LParen)) {
                return Some(idx);
            }
        }
        None
    }

    fn parse_call_args(&mut self) -> Result<(Vec<CallArg>, usize), String> {
        let mut args = Vec::new();
        if let Some(tok) = self.peek() {
            if tok.kind == LexTokenKind::RParen {
                let end = tok.span.1;
                self.pos += 1;
                return Ok((args, end));
            }
        }
        loop {
            let arg_start = self.peek().map(|t| t.span.0).unwrap_or(0);
            let kw_name = if self.pos + 1 < self.tokens.len()
                && self.tokens[self.pos].kind == LexTokenKind::Ident
                && self.tokens[self.pos + 1].kind == LexTokenKind::Op
                && self.tokens[self.pos + 1].text == "="
            {
                let name = self.tokens[self.pos].text.to_string();
                self.pos += 2;
                Some(name)
            } else {
                None
            };
            let val = self.parse_bp(0)?;
            let arg_end = val.span.1;
            let raw = self.src.get(arg_start..arg_end).unwrap_or("").trim().to_string();
            args.push(CallArg {
                name: kw_name,
                value: val,
                raw,
            });
            match self.peek() {
                Some(tok) if tok.kind == LexTokenKind::Comma => {
                    self.pos += 1;
                    if let Some(after) = self.peek() {
                        if after.kind == LexTokenKind::RParen {
                            let end = after.span.1;
                            self.pos += 1;
                            return Ok((args, end));
                        }
                    }
                }
                Some(tok) if tok.kind == LexTokenKind::RParen => {
                    let end = tok.span.1;
                    self.pos += 1;
                    return Ok((args, end));
                }
                other => {
                    return Err(format!(
                        "expected ',' or ')' in call arguments, got {:?}",
                        other.map(|t| t.text)
                    ));
                }
            }
        }
    }

    fn parse_type_name(&mut self) -> Result<(String, usize), String> {
        let first = self.next()?;
        if first.kind != LexTokenKind::Ident && first.kind != LexTokenKind::Op {
            return Err(format!("expected type name after 'as', got '{}'", first.text));
        }
        let start = first.span.0;
        let mut end = first.span.1;
        // Optional pointer `*` or generic `<...>` on type cast
        while let Some(tok) = self.peek() {
            if tok.text == "*" {
                end = tok.span.1;
                self.pos += 1;
            } else if tok.text == "<" {
                let mut depth = 1usize;
                self.pos += 1;
                while self.pos < self.tokens.len() && depth > 0 {
                    let t = &self.tokens[self.pos];
                    if t.text == "<" {
                        depth += 1;
                    } else if t.text == ">" {
                        depth -= 1;
                    } else if t.text == ">>" && depth >= 2 {
                        depth -= 2;
                    }
                    end = t.span.1;
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        Ok((self.src[start..end].trim().to_string(), end))
    }

    fn parse_nud(&mut self) -> Result<SpannedExpr, String> {
        let tok = self.next()?;
        match &tok.kind {
            LexTokenKind::Int(v) => Ok(SpannedExpr {
                node: ExprAst::IntLit(*v),
                span: tok.span,
            }),
            LexTokenKind::Float(v) => Ok(SpannedExpr {
                node: ExprAst::FloatLit(*v),
                span: tok.span,
            }),
            LexTokenKind::Str(s) => Ok(SpannedExpr {
                node: ExprAst::StrLit(s.clone()),
                span: tok.span,
            }),
            LexTokenKind::LParen => {
                if let Some(next_tok) = self.peek() {
                    if next_tok.kind == LexTokenKind::RParen {
                        return Err("empty tuple '()' is not a scalar expression".to_string());
                    }
                }
                let mut inner = self.parse_bp(0)?;
                // Reject tuples `(a, b)` so tuple handling / Pipe destructuring is untouched
                if matches!(self.peek().map(|t| &t.kind), Some(LexTokenKind::Comma)) {
                    return Err("tuple literal is not a scalar expression".to_string());
                }
                let rp = self.expect_kind(&LexTokenKind::RParen)?;
                inner.span = (tok.span.0, rp.span.1);
                Ok(inner)
            }
            LexTokenKind::LBracket => {
                let start = tok.span.0;
                let mut elems = Vec::new();
                if let Some(t) = self.peek() {
                    if t.kind == LexTokenKind::RBracket {
                        let end = t.span.1;
                        self.pos += 1;
                        return Ok(SpannedExpr {
                            node: ExprAst::ListLit(elems),
                            span: (start, end),
                        });
                    }
                }
                loop {
                    let elem = self.parse_bp(0)?;
                    // Reject Python list comprehensions `[x for x in items]` so LLM escalation can handle them
                    if matches!(self.peek().map(|t| t.text), Some("for")) {
                        return Err("list comprehension requires non-literal lowering".to_string());
                    }
                    elems.push(elem);
                    match self.peek() {
                        Some(t) if t.kind == LexTokenKind::Comma => {
                            self.pos += 1;
                            if let Some(after) = self.peek() {
                                if after.kind == LexTokenKind::RBracket {
                                    let end = after.span.1;
                                    self.pos += 1;
                                    return Ok(SpannedExpr {
                                        node: ExprAst::ListLit(elems),
                                        span: (start, end),
                                    });
                                }
                            }
                        }
                        Some(t) if t.kind == LexTokenKind::RBracket => {
                            let end = t.span.1;
                            self.pos += 1;
                            return Ok(SpannedExpr {
                                node: ExprAst::ListLit(elems),
                                span: (start, end),
                            });
                        }
                        other => {
                            return Err(format!(
                                "expected ',' or ']' in list literal, got {:?}",
                                other.map(|t| t.text)
                            ));
                        }
                    }
                }
            }
            LexTokenKind::LBrace => {
                let start = tok.span.0;
                let mut entries = Vec::new();
                if let Some(t) = self.peek() {
                    if t.kind == LexTokenKind::RBrace {
                        let end = t.span.1;
                        self.pos += 1;
                        return Ok(SpannedExpr {
                            node: ExprAst::MapLit(entries),
                            span: (start, end),
                        });
                    }
                }
                loop {
                    let k = self.parse_bp(0)?;
                    self.expect_kind(&LexTokenKind::Colon)?;
                    let v = self.parse_bp(0)?;
                    entries.push((k, v));
                    match self.peek() {
                        Some(t) if t.kind == LexTokenKind::Comma => {
                            self.pos += 1;
                            if let Some(after) = self.peek() {
                                if after.kind == LexTokenKind::RBrace {
                                    let end = after.span.1;
                                    self.pos += 1;
                                    return Ok(SpannedExpr {
                                        node: ExprAst::MapLit(entries),
                                        span: (start, end),
                                    });
                                }
                            }
                        }
                        Some(t) if t.kind == LexTokenKind::RBrace => {
                            let end = t.span.1;
                            self.pos += 1;
                            return Ok(SpannedExpr {
                                node: ExprAst::MapLit(entries),
                                span: (start, end),
                            });
                        }
                        other => {
                            return Err(format!(
                                "expected ',' or '}}' in map literal, got {:?}",
                                other.map(|t| t.text)
                            ));
                        }
                    }
                }
            }
            LexTokenKind::Op => {
                if let Some(rbp) = PolyglotLexicon::prefix_rbp(tok.text) {
                    let rhs = self.parse_bp(rbp)?;
                    let end = rhs.span.1;
                    Ok(SpannedExpr {
                        node: ExprAst::Unary {
                            op: tok.text.to_string(),
                            rhs: Box::new(rhs),
                        },
                        span: (tok.span.0, end),
                    })
                } else {
                    Err(format!("unexpected prefix operator '{}'", tok.text))
                }
            }
            LexTokenKind::Ident => match tok.text {
                "true" | "True" => Ok(SpannedExpr {
                    node: ExprAst::BoolLit(true),
                    span: tok.span,
                }),
                "false" | "False" => Ok(SpannedExpr {
                    node: ExprAst::BoolLit(false),
                    span: tok.span,
                }),
                "null" | "None" | "nil" | "nullptr" | "NULL" => Ok(SpannedExpr {
                    node: ExprAst::NullLit,
                    span: tok.span,
                }),
                "lambda" | "fn" | "func" => Err("inline lambda/closure expression".to_string()),
                "not" => {
                    let rbp = PolyglotLexicon::prefix_rbp("not").unwrap();
                    let rhs = self.parse_bp(rbp)?;
                    let end = rhs.span.1;
                    Ok(SpannedExpr {
                        node: ExprAst::Unary {
                            op: "not".to_string(),
                            rhs: Box::new(rhs),
                        },
                        span: (tok.span.0, end),
                    })
                }
                "new" => {
                    let cls_tok = self.next()?;
                    if cls_tok.kind != LexTokenKind::Ident {
                        return Err(format!("expected class name after 'new', got '{}'", cls_tok.text));
                    }
                    let mut class_name = cls_tok.text.to_string();
                    let mut end = cls_tok.span.1;
                    // Handle dotted class path `new pkg.ClassName(...)`
                    while matches!(self.peek().map(|t| &t.kind), Some(LexTokenKind::Dot)) {
                        self.pos += 1;
                        let seg = self.next()?;
                        if seg.kind != LexTokenKind::Ident {
                            return Err(format!("expected identifier after '.', got '{}'", seg.text));
                        }
                        class_name.push('.');
                        class_name.push_str(seg.text);
                        end = seg.span.1;
                    }
                    // Consume optional generic arguments `new ArrayList<Integer>(...)`
                    if matches!(self.peek().map(|t| t.text), Some("<")) {
                        let mut depth = 1usize;
                        self.pos += 1;
                        while self.pos < self.tokens.len() && depth > 0 {
                            let gt = &self.tokens[self.pos];
                            if gt.text == "<" {
                                depth += 1;
                            } else if gt.text == ">" {
                                depth -= 1;
                            } else if gt.text == ">>" && depth >= 2 {
                                depth -= 2;
                            }
                            end = gt.span.1;
                            self.pos += 1;
                        }
                    }
                    let args = if matches!(self.peek().map(|t| &t.kind), Some(LexTokenKind::LParen)) {
                        self.pos += 1;
                        let (parsed_args, call_end) = self.parse_call_args()?;
                        end = call_end;
                        parsed_args
                    } else {
                        Vec::new()
                    };
                    Ok(SpannedExpr {
                        node: ExprAst::New { class_name, args },
                        span: (tok.span.0, end),
                    })
                }
                _ => Ok(SpannedExpr {
                    node: ExprAst::Ident(tok.text.to_string()),
                    span: tok.span,
                }),
            },
            _ => Err(format!("unexpected token '{}' at start of expression", tok.text)),
        }
    }

    fn parse_bp(&mut self, min_bp: u8) -> Result<SpannedExpr, String> {
        let mut lhs = self.parse_nud()?;

        while let Some(tok) = self.peek() {
            // 1. Postfix appliers: `.field`, `[index]`, `(args...)`, and `<T>(args...)`
            if matches!(
                tok.kind,
                LexTokenKind::Dot | LexTokenKind::LBracket | LexTokenKind::LParen
            ) {
                if PolyglotLexicon::POSTFIX_LBP < min_bp {
                    break;
                }
                match tok.kind {
                    LexTokenKind::Dot => {
                        self.pos += 1;
                        let fld = self.next()?;
                        if fld.kind != LexTokenKind::Ident {
                            return Err(format!("expected field/method name after '.', got '{}'", fld.text));
                        }
                        let span = (lhs.span.0, fld.span.1);
                        lhs = SpannedExpr {
                            node: ExprAst::Member {
                                target: Box::new(lhs),
                                field: fld.text.to_string(),
                            },
                            span,
                        };
                        continue;
                    }
                    LexTokenKind::LBracket => {
                        self.pos += 1;
                        if matches!(self.peek().map(|t| &t.kind), Some(LexTokenKind::Colon)) {
                            return Err("slice range indexing requires non-scalar lowering".to_string());
                        }
                        let idx_expr = self.parse_bp(0)?;
                        if matches!(self.peek().map(|t| &t.kind), Some(LexTokenKind::Colon)) {
                            return Err("slice range indexing requires non-scalar lowering".to_string());
                        }
                        let rb = self.expect_kind(&LexTokenKind::RBracket)?;
                        let span = (lhs.span.0, rb.span.1);
                        lhs = SpannedExpr {
                            node: ExprAst::Index {
                                target: Box::new(lhs),
                                index: Box::new(idx_expr),
                            },
                            span,
                        };
                        continue;
                    }
                    LexTokenKind::LParen => {
                        self.pos += 1;
                        let (args, end) = self.parse_call_args()?;
                        let span = (lhs.span.0, end);
                        lhs = SpannedExpr {
                            node: ExprAst::Call {
                                callee: Box::new(lhs),
                                args,
                            },
                            span,
                        };
                        continue;
                    }
                    _ => unreachable!(),
                }
            }

            // Generic call applier `func<T>(args...)`
            if tok.text == "<" && matches!(lhs.node, ExprAst::Ident(_) | ExprAst::Member { .. }) {
                if let Some(lparen_idx) = self.try_scan_generic_call_args() {
                    if PolyglotLexicon::POSTFIX_LBP < min_bp {
                        break;
                    }
                    self.pos = lparen_idx + 1; // consume `<...>` and `(`
                    let (args, end) = self.parse_call_args()?;
                    let span = (lhs.span.0, end);
                    lhs = SpannedExpr {
                        node: ExprAst::Call {
                            callee: Box::new(lhs),
                            args,
                        },
                        span,
                    };
                    continue;
                }
            }

            // 2. C/JS Ternary `cond ? then_br : else_br`
            if tok.kind == LexTokenKind::Question {
                let (lbp, rbp) = PolyglotLexicon::infix_bp("?").unwrap();
                if lbp < min_bp {
                    break;
                }
                self.pos += 1;
                let then_br = self.parse_bp(0)?;
                self.expect_kind(&LexTokenKind::Colon)?;
                let else_br = self.parse_bp(rbp)?;
                let span = (lhs.span.0, else_br.span.1);
                lhs = SpannedExpr {
                    node: ExprAst::Ternary {
                        cond: Box::new(lhs),
                        then_br: Box::new(then_br),
                        else_br: Box::new(else_br),
                    },
                    span,
                };
                continue;
            }

            // 3. Python inline conditional `then_br if cond else else_br`
            if tok.kind == LexTokenKind::Ident && tok.text == "if" {
                let (lbp, rbp) = PolyglotLexicon::infix_bp("if").unwrap();
                if lbp < min_bp {
                    break;
                }
                self.pos += 1;
                let cond = self.parse_bp(0)?;
                let else_tok = self.next()?;
                if else_tok.text != "else" {
                    return Err(format!("expected 'else' in conditional expression, got '{}'", else_tok.text));
                }
                let else_br = self.parse_bp(rbp)?;
                let span = (lhs.span.0, else_br.span.1);
                lhs = SpannedExpr {
                    node: ExprAst::Ternary {
                        cond: Box::new(cond),
                        then_br: Box::new(lhs),
                        else_br: Box::new(else_br),
                    },
                    span,
                };
                continue;
            }

            // 4. Rust / TypeScript `expr as Type`
            if tok.kind == LexTokenKind::Ident && tok.text == "as" {
                let (lbp, _) = PolyglotLexicon::infix_bp("as").unwrap();
                if lbp < min_bp {
                    break;
                }
                self.pos += 1;
                let (target_type, end) = self.parse_type_name()?;
                let span = (lhs.span.0, end);
                lhs = SpannedExpr {
                    node: ExprAst::Cast {
                        expr: Box::new(lhs),
                        target_type,
                    },
                    span,
                };
                continue;
            }

            // 5. Infix binary operators (`+`, `*`, `**`, `&&`, `and`, `is`, `is not`, etc.)
            if matches!(tok.kind, LexTokenKind::Op | LexTokenKind::Ident) {
                if let Some((lbp, rbp)) = PolyglotLexicon::infix_bp(tok.text) {
                    if lbp < min_bp {
                        break;
                    }
                    let op_tok = self.next()?;
                    let op_str = if op_tok.text == "is"
                        && matches!(self.peek().map(|t| t.text), Some("not"))
                    {
                        self.pos += 1;
                        "is not".to_string()
                    } else {
                        op_tok.text.to_string()
                    };
                    let rhs = self.parse_bp(rbp)?;
                    let span = (lhs.span.0, rhs.span.1);
                    lhs = SpannedExpr {
                        node: ExprAst::Binary {
                            op: op_str,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        span,
                    };
                    continue;
                }
            }

            break;
        }

        Ok(lhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bracket_based_c_style() {
        let c_src = r#"
int main() {
    int x = 0;
    while (x < 3) {
        print(x);
        x = x + 1;
    }
    if (x == 3) {
        print("Success");
    } else {
        print("Failed");
    }
    return 0;
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("c_test", c_src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.len(), 4); // Assign x, WhileLoop, If, Return
    }

    #[test]
    fn test_parse_c_for_loop_with_brackets() {
        let src = r#"
fn run() {
    for (int i = 0; i < 10; i++) {
        print(i);
    }
    return 0;
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("for_test", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "run");

        if let Statement::ForLoop { var, count, body } = &f.body[0] {
            assert_eq!(var, "i");
            assert_eq!(count, "10");
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Statement::ForLoop");
        }
    }

    #[test]
    fn test_parse_exotic_quirks() {
        let src = r#"
def main() {
    count := 0;
    count++;
    return 0 if count > 0;
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("quirk_test", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "main");

        // count := 0 (GoScopedDeclaration), count = count + 1 (Standard), if count > 0: return 0
        assert_eq!(f.body.len(), 3);
        assert_eq!(
            f.body[0],
            Statement::Assign {
                target: "count".to_string(),
                expr: "0".to_string(),
                idiom: IdiomFamily::GoScopedDeclaration,
                is_scoped_decl: true,
            }
        );
        assert_eq!(
            f.body[1],
            Statement::Assign {
                target: "count".to_string(),
                expr: "count + 1".to_string(),
                idiom: IdiomFamily::Standard,
                is_scoped_decl: false,
            }
        );

        if let Statement::If { condition, then_branch, .. } = &f.body[2] {
            assert_eq!(condition, "count > 0");
            assert_eq!(then_branch.len(), 1);
            assert_eq!(then_branch[0], Statement::Return { expr: Some("0".to_string()) });
        } else {
            panic!("Expected Statement::If for postfix if");
        }
    }

    #[test]
    fn test_parse_go_spawn() {
        let src = r#"
def main() {
    ch := make(chan int, 10);
    go worker(ch);
    return 0;
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("go_spawn_test", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.len(), 3);

        assert_eq!(
            f.body[1],
            Statement::GoSpawn {
                call_expr: "worker(ch)".to_string(),
                idiom: IdiomFamily::GoScopedDeclaration,
            }
        );
    }

    #[test]
    fn test_parse_collections() {
        let src = r#"
def main():
    items = [10, 20, 30]
    items.append(40)
    items[0] = 99
    scores = {"alice": 100, "bob": 200}
    scores["carol"] = 300
    for i in len(items):
        print(items[i])
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("coll_test", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.len(), 7);

        assert_eq!(
            f.body[0],
            Statement::ListInit {
                target: "items".to_string(),
                elements: vec!["10".to_string(), "20".to_string(), "30".to_string()],
                is_scoped_decl: false,
            }
        );
        assert_eq!(
            f.body[1],
            Statement::ListAppend {
                target: "items".to_string(),
                expr: "40".to_string(),
            }
        );
        assert_eq!(
            f.body[2],
            Statement::IndexSet {
                target: "items".to_string(),
                index_expr: "0".to_string(),
                val_expr: "99".to_string(),
            }
        );
        assert_eq!(
            f.body[3],
            Statement::MapInit {
                target: "scores".to_string(),
                entries: vec![
                    ("alice".to_string(), "100".to_string()),
                    ("bob".to_string(), "200".to_string()),
                ],
                is_scoped_decl: false,
            }
        );
        assert_eq!(
            f.body[4],
            Statement::IndexSet {
                target: "scores".to_string(),
                index_expr: "\"carol\"".to_string(),
                val_expr: "300".to_string(),
            }
        );
    }

    #[test]
    fn test_mixed_c_and_python_indentation() {
        let src = r#"
def py_fn():
    x = 10
    return x

int c_fn() {
    return 20;
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("mixed_test", src);

        assert_eq!(module.functions.len(), 2);
        let py_f = &module.functions[0];
        assert_eq!(py_f.name, "py_fn");
        assert_eq!(py_f.body.len(), 2);
        assert_eq!(
            py_f.body[0],
            Statement::Assign {
                target: "x".to_string(),
                expr: "10".to_string(),
                idiom: IdiomFamily::Standard,
                is_scoped_decl: false,
            }
        );

        let c_f = &module.functions[1];
        assert_eq!(c_f.name, "c_fn");
        assert_eq!(c_f.body.len(), 1);
    }

    #[test]
    fn test_string_with_commas_and_if_keyword() {
        let src = r#"
def main():
    m = {"greeting": "hello, world", "count": 42}
    print("what if now")
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("comma_if_test", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.body.len(), 3);
        assert_eq!(
            f.body[0],
            Statement::MapInit {
                target: "m".to_string(),
                entries: vec![
                    ("greeting".to_string(), "\"hello, world\"".to_string()),
                    ("count".to_string(), "42".to_string()),
                ],
                is_scoped_decl: false,
            }
        );
        assert_eq!(
            f.body[1],
            Statement::Print {
                expr: "\"what if now\"".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_carved_java_fn() {
        let src = r#"    public static int add(int a, int b) {
        return a + b;
    }"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("add", src);
        println!("Java parsed functions: {:#?}", module.functions);
        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "add");
        assert_eq!(f.body.len(), 1);
    }

    #[test]
    fn test_polyglot_comments_every_language() {
        let src = r#"#!/usr/bin/env omni
#![allow(unused)]
; Assembly / INI full-line comment
% Erlang / MATLAB / LaTeX full-line comment
! Fortran 90 full-line comment
!! Fortran doc comment
REM BASIC / Batch full-line comment
rem lowercase batch comment
=begin
Ruby multi-line comment with "unclosed quote and # and */ inside
=end
=pod
=head1 Perl POD block
Ignored documentation with // and /*
=cut
%{
MATLAB multi-line comment with 100 % 3 inside
%}
<!-- HTML / XML
     multi-line comment -->
{# Jinja2 / Twig
   template comment #}
<# PowerShell
   block comment #>
#| Common Lisp / Scheme
   #| nested inner comment |#
   outer comment |#
#= Julia
   #= nested julia comment =#
   still comment =#
{- Haskell / Elm
   {- nested haskell comment -}
   still comment -}
(* OCaml / Pascal / ML
   (* nested ocaml comment *)
   still comment *)
###
CoffeeScript block comment
###
--[[ Lua block comment ]]
--[==[ Lua parameterized
       ] ] --[[ still inside ]]
       block comment ]==]
#[inline]
def compute(/* inline param comment */ a: int, b: int) -> int:
    """
    Standalone Python docstring with unbalanced quote: don't # break */
    """
    r"""Raw Python docstring with \x00 and >>> doctests
    >>> block = compute(1, 2)
    """
    '''Single-quoted standalone docstring'''
    /* prefix block comment preserving 4-space indent */ x = a + b // C++ trailing comment after =
    int /* mid-token */ y = /* rhs */ 10; /* trailing C block */
    /* outer Rust block /* nested inner block */ back to outer */
    /* C glob doc comment matching /*.c without second close */
    z = x + y -- Lua inline comment
    --- EmmyLua doc comment
    w = z + 5 ;; Clojure / Lisp / WAT inline comment
    total = w + 7 %% Erlang / Prolog / MATLAB inline comment
    return total // final prose comment with multiple words
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("comment_every_lang", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "compute");
        // Verify `#[inline]` survived `#` comment stripping!
        assert_eq!(f.annotations.len(), 1);
        assert_eq!(f.annotations[0].target_expr, "inline");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert_eq!(f.params[1].name, "b");
        // Statements inside compute: x, y, z, w, total, return total => 6 statements
        assert_eq!(f.body.len(), 6, "Parsed body was: {:#?}", f.body);
    }

    #[test]
    fn test_comment_vs_operator_and_attribute_disambiguation() {
        let src = r#"
fn borrow<'a>(s: &'a str) -> i32 { // Rust lifetimes must not swallow trailing comment
    int count = 10; // semicolon C assignment comment
    count++; // postfix inc
    ++count; // prefix inc
    count--; -- Lua comment after postfix dec? No: `;` ended statement!
    --count; // prefix dec
    return count; // done
}

def py_math(a, b):
    q = a // b # Python floor division preserved while # comment stripped
    r = a % b # modulo preserved
    msg = "keep // and /* and # and -- and ;; and %% inside quotes"
    raw_msg = """keep # inside assigned triple string"""
    key = cls(*as_ints) # Python *args unpacking must never trigger OCaml (* *)
    deref = (*ptr) + 1 // C pointer dereference must never trigger OCaml (* *)
    neg_set = {-1, -2} # Negative set literal must never trigger Haskell {- -}
    fmt_cont = (
        "vals %d %d"
        % (a, b)
    )
    return q + r
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("disambig", src);

        let borrow_fn = module.functions.iter().find(|f| f.name == "borrow").expect("borrow fn");
        assert_eq!(borrow_fn.body.len(), 6, "borrow body: {:#?}", borrow_fn.body);

        let py_fn = module.functions.iter().find(|f| f.name == "py_math").expect("py_math fn");
        assert_eq!(py_fn.body.len(), 9, "py_math body: {:#?}", py_fn.body);
        if let Statement::Assign { target, expr, .. } = &py_fn.body[0] {
            assert_eq!(target, "q");
            assert_eq!(expr, "a // b");
        } else {
            panic!("Expected floor division Assign, got {:?}", py_fn.body[0]);
        }
        let stripped = OmniParser::strip_polyglot_comments(src);
        assert!(stripped.contains("cls(*as_ints)"), "stripped broke *args: {}", stripped);
        assert!(stripped.contains("(*ptr)"), "stripped broke C deref: {}", stripped);
        assert!(stripped.contains("{-1, -2}"), "stripped broke negative set: {}", stripped);
        assert!(stripped.contains("% (a, b)"), "stripped broke multiline % fmt: {}", stripped);
    }

    #[test]
    fn test_pratt_precedence_and_appliers() {
        // 1. Bitwise precedence: `1 | 2 & 0` -> `1 | (2 & 0)` (root is `|`)
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("1 | 2 & 0").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("1", "|", "2 & 0"));

        // 2. Exponentiation tighter than `*` and right-associative:
        //    `2 * 3 ** 2` -> `2 * (3 ** 2)` (root is `*`)
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("2 * 3 ** 2").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("2", "*", "3 ** 2"));

        //    `2 ** 3 ** 2` -> `2 ** (3 ** 2)` (right-associative!)
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("2 ** 3 ** 2").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("2", "**", "3 ** 2"));

        // 3. Unary `-` looser than `**`, while `(-3) ** 2` binds `-3` as LHS of `**`
        assert!(OmniParser::pratt_split_top_binary("-3 ** 2").is_none());
        let neg_pow = OmniParser::parse_expr("-3 ** 2").unwrap();
        assert!(matches!(neg_pow.node, ExprAst::Unary { ref op, .. } if op == "-"));

        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("(-3) ** 2").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("(-3)", "**", "2"));

        // 3b. Parenthesized group inside chained arithmetic `(0 - n) * 2 - 1` must preserve outer parens in slice
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("(0 - n) * 2 - 1").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("(0 - n) * 2", "-", "1"));
        let (inner_l, inner_op, inner_r) = OmniParser::pratt_split_top_binary(&lhs).unwrap();
        assert_eq!((inner_l.as_str(), inner_op.as_str(), inner_r.as_str()), ("(0 - n)", "*", "2"));

        // 4. Two calls in a binary expression (`foo(1) + bar(2)`) must split at `+`, never merge args!
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("foo(1) + bar(2)").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("foo(1)", "+", "bar(2)"));

        // 5. Parenthesized boolean precedence: `false && (false || true)` splits at `&&`
        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("false && (false || true)").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("false", "&&", "(false || true)"));

        // 6. Chained postfix appliers: `self.items[i + 1].calc(k=10)[0]`
        let src = "self.items[i + 1].calc(k=10)[0]";
        let ast = OmniParser::parse_expr(src).unwrap();
        if let ExprAst::Index { target, index } = &ast.node {
            assert_eq!(index.slice(src), "0");
            assert_eq!(target.slice(src), "self.items[i + 1].calc(k=10)");
            if let ExprAst::Call { callee, args } = &target.node {
                assert_eq!(callee.slice(src), "self.items[i + 1].calc");
                assert_eq!(args.len(), 1);
                assert_eq!(args[0].name.as_deref(), Some("k"));
                assert_eq!(args[0].value.slice(src), "10");
            } else {
                panic!("Expected Call inside outer Index, got {:?}", target.node);
            }
        } else {
            panic!("Expected outer Index, got {:?}", ast.node);
        }

        // 7. Generic constructor & generic call appliers
        let gen_new = OmniParser::parse_expr("new ArrayList<Integer>(10)").unwrap();
        assert!(matches!(gen_new.node, ExprAst::New { ref class_name, ref args } if class_name == "ArrayList" && args.len() == 1));
        let gen_call = OmniParser::parse_expr("identity<i64>(42)").unwrap();
        assert!(matches!(gen_call.node, ExprAst::Call { .. }));
    }

    #[test]
    fn test_multiline_and_semicolon_canonicalization() {
        let src = r#"
fn calc() -> i64 {
    a = 10; b = 20; a++;
    total = a +
        b *
        2
    res = add_vals(
        total,
        5
    )
    return res
}
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("canon_test", src);
        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        // `a = 10`, `b = 20`, `a = a + 1`, `total = a + b * 2`, `res = add_vals( total, 5 )`, `return res`
        assert_eq!(f.body.len(), 6, "body: {:#?}", f.body);
    }

    #[test]
    fn test_polyglot_kotlin_swift_julia_nim_ruby_pipe() {
        assert_eq!(
            OmniParser::unwrap_outer_balanced_parens("(frame % 12 == 0) and (b % 3 == 0)"),
            "(frame % 12 == 0) and (b % 3 == 0)"
        );
        assert_eq!(
            OmniParser::unwrap_outer_balanced_parens("((x > 10))"),
            "x > 10"
        );

        let (lhs, op, rhs) = OmniParser::pratt_split_top_binary("b |> mix(frame) |> clamp(0, 255)").unwrap();
        assert_eq!((lhs.as_str(), op.as_str(), rhs.as_str()), ("b |> mix(frame)", "|>", "clamp(0, 255)"));

        let src = r#"
fun kt_clamp(v: Int, lo: Int, hi: Int): Int {
    val low = v < lo ? lo : v
    return low > hi ? hi : low
}

func swift_mix(_ a: Double, _ b: Double) -> Double {
    return a + b
}

function jl_wave(x::Float64)::Float64
    return x * 2.0
end

proc nim_hash(seed: int64): int64 =
    return seed ^ 42

def rb_curve(ch)
    unless ch >= 10
        ch = ch + 5
    elsif ch > 100
        ch = 100
    end
    ch = ch + 1 unless ch % 2 == 0
    return ch
end
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("polyglot_ext", src);
        let names: Vec<&str> = module.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["kt_clamp", "swift_mix", "jl_wave", "nim_hash", "rb_curve"]);
        assert_eq!(module.functions[1].params[0].name, "a");
        assert_eq!(module.functions[2].return_type.as_deref(), Some("Float64"));
    }
}
