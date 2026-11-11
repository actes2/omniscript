use omni_mem::{MemoryNormalizer, UnifiedMemOp};
pub use omni_preamble::ImportDirective;
use regex::Regex;
use serde::{Deserialize, Serialize};

pub mod diagnostic;
pub use diagnostic::*;

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
            let hint = p_trimmed[pos + 1..].trim().to_string();
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
                // Go style: `base int`, `items []string`, `node *Node`
                let raw_name = parts[..parts.len() - 1].join(" ");
                let name: String = raw_name.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                return ParamDef {
                    name: if name.is_empty() { "arg".to_string() } else { name },
                    type_hint: Some(last.to_string()),
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

    pub fn new() -> Self {
        Self {
            // Polyglot function pattern covering Python, Rust, Go, JS, Lua, Java, C#, C/C++
            fn_re: Regex::new(
                r#"^(?:(?:(?:public|private|protected|internal|pub|abstract|virtual|override|inline|sealed)\s+)*(?:static\s+)?(?:async\s+)?(?:def|fn|func|function|(?:(?:[A-Z][a-zA-Z0-9_<>*&\[\]:]*|int|int32|int64|long|float|double|bool|boolean|void|char\*|auto|string|String|List\[.*?\])\s+))|(?:local\s+)?function\s+)\s*([a-zA-Z_0-9]+)\s*\((.*?)\)\s*(?:->\s*(?:"[^"]*"|'[^']*'|[a-zA-Z_0-9*&\[\]:,"' .]+)|[a-zA-Z_0-9*&\[\]:]+\s*)?(?::|\{|\;)?$"#,
            )
            .unwrap(),
            // Matches if (x > 10) {, if x > 10 {, if x > 10:
            if_re: Regex::new(r#"^if\s*(?:\((.*)\)|(.*?))\s*(?::|\{)?$"#).unwrap(),
            // Matches elif x == 5:, else if (x == 5) {, else if x == 5 {
            elif_re: Regex::new(r#"^(?:elif|else\s+if)\s*(?:\((.*)\)|(.*?))\s*(?::|\{)?$"#).unwrap(),
            // Matches else:, else {
            else_re: Regex::new(r#"^else\s*(?::|\{)?$"#).unwrap(),
            // Matches while (x > 0) {, while x > 0 {, while x > 0:
            while_re: Regex::new(r#"^while\s*(?:\((.*)\)|(.*?))\s*(?::|\{)?$"#).unwrap(),
            // Matches for i in len(items):, for i in range(10):, for i in 10:
            plain_for_re: Regex::new(
                r#"^for\s+(?:(?:int|let|var)\s+)?([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*(.+?)\s*\)|(.+?))\s*(?::|\{)?$"#,
            )
            .unwrap(),
            // Matches for (i in 10), for (i in items)
            paren_for_re: Regex::new(
                r#"^for\s*\(\s*(?:(?:int|let|var)\s+)?([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*(.+?)\s*\)|(.+?))\s*\)\s*(?::|\{)?$"#,
            )
            .unwrap(),
            // Matches for (int i = 0; i < 10; i++) {
            c_for_re: Regex::new(
                r#"^for\s*\(\s*(?:(?:int|let|var)\s+)?([a-zA-Z_0-9]+)\s*=\s*0\s*;\s*[a-zA-Z_0-9]+\s*<\s*(.+?)\s*;\s*[^)]*\)\s*(?::|\{)?$"#,
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
                r#"^(?:(let(?:\s+mut)?|const|var|[A-Z][a-zA-Z0-9_<>*&]*|int|int32|int64|float|double|bool|boolean|String|string|auto)\s+)?([a-zA-Z_0-9]+(?:\.[a-zA-Z_0-9]+)*)(?:\s*:\s*[a-zA-Z_0-9*&<>\[\]]+)?\s*=\s*(.+?)\s*;?$"#,
            )
            .unwrap(),
            // Matches ch := make(chan int, 8), ch = Channel(10), let ch = Chan(4)
            make_chan_re: Regex::new(
                r#"^(?:(let|const|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*(?:make\s*\(\s*chan\s+[a-zA-Z_0-9*]+\s*(?:,\s*([0-9]+))?\s*\)|(?:Channel|Chan)\s*\(\s*([0-9]+)?\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches ch <- 42, out_chan.send(val)
            chan_send_re: Regex::new(
                r#"^(?:([a-zA-Z_0-9]+)\s+<-\s+(.+)|([a-zA-Z_0-9]+)\.send\s*\(\s*(.+)\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches val := <-ch, val = <-ch, val = ch.recv(), <-ch
            chan_recv_re: Regex::new(
                r#"^(?:(?:(let|const|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*)?(?:<-\s*([a-zA-Z_0-9]+)|([a-zA-Z_0-9]+)\.recv\s*\(\s*\))\s*;?$"#,
            )
            .unwrap(),
            // Matches items = [10, 20, 30], items := [], const items = [1, 2]
            list_init_re: Regex::new(
                r#"^(?:(let|const|var)\s+)?([a-zA-Z_0-9]+)(?:\s*:\s*[a-zA-Z0-9_*&<>\[\]]+)?\s*(?::=|=)\s*\[(.*)\]\s*;?$"#,
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
                r#"^(?:(let|const|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*\{(.*)\}\s*;?$"#,
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

    /// Strip trailing single-line comment (`//` or `#`) outside of string quotes
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
                in_single = !in_single;
            } else if b == b'"' && !in_single && !in_backtick {
                in_double = !in_double;
            } else if b == b'`' && !in_single && !in_double {
                in_backtick = !in_backtick;
            } else if !in_single && !in_double && !in_backtick {
                if b == b'#' {
                    return line[..i].trim_end();
                }
                if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    // Only treat `//` as a C-style comment if there is no assignment `=` on the line
                    // or if `//` is at the start of the line (`trimmed.starts_with("//")`), preserving Python floor division `q = a // b`.
                    let prefix = line[..i].trim();
                    if prefix.is_empty()
                        || (!prefix.contains('=')
                            && !prefix.starts_with("return ")
                            && !prefix.starts_with("if ")
                            && !prefix.starts_with("elif ")
                            && !prefix.starts_with("while ")
                            && !prefix.starts_with("print("))
                    {
                        return line[..i].trim_end();
                    }
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

    /// Canonicalizes source code: unifies bracket blocks (`{ ... }`), `end` blocks, postfix conditionals,
    /// and increments (`i++`) into hierarchical canonical AST lines.
    /// Preserves operators like `:=` and `=>` without destructive string replacement.
    pub fn canonicalize_source(source: &str) -> String {
        let base_indent = source
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| Self::indent_level(l))
            .min()
            .unwrap_or(0);

        let raw_lines: Vec<&str> = source.lines().collect();
        let mut out_lines = Vec::new();
        let mut brace_depth: usize = 0;
        let mut in_docstring: Option<&'static str> = None;
        let mut in_block_comment = false;

        let inc_re = Regex::new(r#"^([a-zA-Z_0-9]+)\+\+$"#).unwrap();
        let dec_re = Regex::new(r#"^([a-zA-Z_0-9]+)\-\-$"#).unwrap();
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

            let stripped = Self::strip_trailing_comment(line);
            let mut trimmed = stripped.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(quote) = in_docstring {
                if trimmed.contains(quote) {
                    in_docstring = None;
                }
                continue;
            }

            if in_block_comment {
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }

            if trimmed.starts_with("/*") {
                if !trimmed.ends_with("*/") {
                    in_block_comment = true;
                }
                continue;
            }

            if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                let quote = if trimmed.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
                if trimmed.len() > 3 && trimmed[3..].contains(quote) {
                    continue;
                }
                in_docstring = Some(quote);
                continue;
            }

            if trimmed.starts_with(':') && (trimmed.starts_with(":param") || trimmed.starts_with(":type") || trimmed.starts_with(":return") || trimmed.starts_with(":rtype")) {
                continue;
            }

            // Multi-line expressions: `methods = { ... }`, `items = [ ... ]`, `tuple = ( ... )`, or multi-line calls `fn( ... )`
            let is_dict_assign = trimmed.ends_with('{')
                && trimmed[..trimmed.len() - 1].trim().ends_with('=');
            let has_open_delimiter =
                trimmed.ends_with('(') || trimmed.ends_with('[') || is_dict_assign;
            if has_open_delimiter {
                let close_char = if trimmed.ends_with('{') { '}' } else if trimmed.ends_with('[') { ']' } else { ')' };
                let mut accum_str = trimmed.to_string();
                while idx < raw_lines.len() {
                    let n_raw = raw_lines[idx];
                    idx += 1;
                    let n_stripped = Self::strip_trailing_comment(n_raw).trim();
                    if n_stripped.is_empty() {
                        continue;
                    }
                    accum_str.push(' ');
                    accum_str.push_str(n_stripped);
                    if n_stripped.contains(close_char) {
                        break;
                    }
                }
                let current_indent = " ".repeat(Self::indent_level(line));
                out_lines.push(format!("{}{}", current_indent, accum_str));
                continue;
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

            // 5. Handle C-style increment/decrement: `i++` -> `i = i + 1`
            if let Some(caps) = inc_re.captures(&line_str) {
                let var = &caps[1];
                line_str = format!("{} = {} + 1", var, var);
            } else if let Some(caps) = dec_re.captures(&line_str) {
                let var = &caps[1];
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

            // 7. Handle Ruby/Perl Postfix `if`: `return 0 if x > 10` -> `if x > 10: return 0`
            // Ensure we do NOT match inside string literals like print("what if now") or list comprehensions `[x for x in y if ...]`
            if !line_str.starts_with("if ")
                && !line_str.starts_with("if(")
                && !line_str.contains('"')
                && !line_str.contains('\'')
                && !(line_str.contains('[') && line_str.contains(" for "))
            {
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

            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
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
                        if cur_indent <= parent_indent && !next_trimmed.contains(':') {
                            break;
                        }

                        let f_trim = next_trimmed.trim_end_matches(',').trim().trim_start_matches("pub ").trim();
                        if let Some((f_name, f_type)) = f_trim.split_once(':') {
                            class_fields.push(FieldDef {
                                name: f_name.trim().to_string(),
                                type_hint: Some(f_type.trim().to_string()),
                                default_val: None,
                            });
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

            // Inherent impl blocks: `impl Point { ... }`
            if trimmed.starts_with("impl ") {
                let impl_line = trimmed.trim_start_matches("impl ").trim_end_matches('{').trim();
                let struct_name = impl_line.split_whitespace().next().unwrap_or("").to_string();

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
                    if cur_indent <= parent_indent && !next_trimmed.starts_with("fn ") && !next_trimmed.starts_with("pub fn ") {
                        break;
                    }

                    if let Some(caps) = self.fn_re.captures(next_trimmed) {
                        let method_name = caps[1].to_string();
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
                            return_type: None,
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
                        if cur_indent <= parent_indent && !next_trimmed.contains(',') {
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
                            || next_trimmed.starts_with('#')
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
                                return_type: None,
                                body,
                                memory_ops,
                            });
                            continue;
                        }

                        method_decorators.clear();

                        // Field definitions
                        if !next_trimmed.contains('(')
                            && !next_trimmed.contains(')')
                            && !next_trimmed.starts_with("return")
                            && !next_trimmed.starts_with("\"\"\"")
                            && !next_trimmed.starts_with("'''")
                            && !next_trimmed.starts_with(">>>")
                            && !next_trimmed.starts_with(':')
                        {
                            let clean_line = next_trimmed.trim_end_matches(';').trim();
                            if clean_line.contains('=') && !clean_line.starts_with("if") {
                                let parts: Vec<&str> = clean_line.splitn(2, '=').collect();
                                let decl = parts[0].trim();
                                let default_val = parts[1].trim().to_string();
                                let f_tokens: Vec<&str> = decl.split_whitespace().collect();
                                let f_name = f_tokens.last().unwrap_or(&decl).to_string();
                                class_fields.push(FieldDef {
                                    name: f_name,
                                    type_hint: None,
                                    default_val: Some(default_val),
                                });
                            } else {
                                let clean_decl = clean_line
                                    .trim_start_matches("public ")
                                    .trim_start_matches("private ")
                                    .trim_start_matches("protected ")
                                    .trim_start_matches("static ")
                                    .trim_start_matches("final ")
                                    .trim();
                                let tokens: Vec<&str> = clean_decl.split_whitespace().collect();
                                if tokens.len() >= 2 {
                                    let type_str = tokens[0].to_string();
                                    let f_name = tokens[tokens.len() - 1].to_string();
                                    class_fields.push(FieldDef {
                                        name: f_name,
                                        type_hint: Some(type_str),
                                        default_val: None,
                                    });
                                } else if tokens.len() == 1 {
                                    class_fields.push(FieldDef {
                                        name: tokens[0].to_string(),
                                        type_hint: None,
                                        default_val: None,
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

            // Top-level functions: `def foo():`, `fn foo() -> i32:`, `int foo() {`
            if let Some(caps) = self.fn_re.captures(trimmed) {
                let fn_name = caps[1].to_string();
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
                    return_type: None,
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

            // Nested function definition inside a block: consume its indented body
            if self.fn_re.is_match(trimmed_text) {
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
                    let clause_trimmed = Self::strip_trailing_comment(clause_raw).trim().to_string();
                    if clause_trimmed.starts_with("except") || clause_trimmed.starts_with("catch") {
                        let (exc_part, inline_after) = clause_trimmed.split_once(':').unwrap_or((&clause_trimmed, ""));
                        let header = exc_part
                            .trim_start_matches("except")
                            .trim_start_matches("catch")
                            .trim_end_matches('{')
                            .trim();

                        let (exc_type, var_name) = if header.is_empty() {
                            (None, None)
                        } else if let Some((t, v)) = header.split_once(" as ") {
                            (Some(t.trim().to_string()), Some(v.trim().to_string()))
                        } else {
                            (Some(header.trim().trim_matches(|c| c == '(' || c == ')').to_string()), None)
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
                let condition = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str().trim().to_string();

                let (then_branch, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                let mut else_branch = None;

                // Check for `elif` chain and terminal `else` at the same indentation level
                let mut current_else = &mut else_branch;
                while next_i < lines.len() && lines[next_i].0 == indent {
                    let next_text = lines[next_i].1.trim();
                    if let Some(elif_caps) = self.elif_re.captures(next_text) {
                        let elif_cond = elif_caps.get(1).or_else(|| elif_caps.get(2)).unwrap().as_str().trim().to_string();
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
                let condition = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str().trim().to_string();
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

            // 13. Polyglot Index Assignment: `items[0] = 99`, `m["key"] = 10`
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
                    let inner = &clean_expr[1..clean_expr.len() - 1];
                    let elements = Self::split_respecting_quotes(inner, ',')
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    stmts.push(Statement::ListInit {
                        target,
                        elements,
                        is_scoped_decl: is_scoped,
                    });
                    i += 1;
                    continue;
                }
            }

            // Polyglot Tuple Unpack: `public_key, private_key = rsa.newkeys(1024)`, `client, addr = server.accept()`
            if text.contains('=') && !text.contains("==") && !text.contains("!=") && !text.contains("<=") && !text.contains(">=") && !text.starts_with("if ") {
                let assign_op = if text.contains(":=") { ":=" } else { "=" };
                if let Some((lhs, rhs)) = text.split_once(assign_op) {
                    let mut lhs_clean = lhs.trim().trim_start_matches("let ").trim_start_matches("var ").trim();
                    if lhs_clean.starts_with('(') && lhs_clean.ends_with(')') {
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
}
