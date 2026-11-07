use omni_mem::{MemoryNormalizer, UnifiedMemOp};
pub use omni_preamble::ImportDirective;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniModule {
    pub name: String,
    pub imports: Vec<ImportDirective>,
    pub structs_and_classes: Vec<ClassDef>,
    pub functions: Vec<FunctionDef>,
    pub top_level_statements: Vec<Statement>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub is_method: bool,
    pub params: Vec<ParamDef>,
    pub return_type: Option<String>,
    pub body: Vec<Statement>,
    pub memory_ops: Vec<UnifiedMemOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    pub type_hint: Option<String>,
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
pub enum Statement {
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
    /// Fallback concrete or polyglot statement
    Concrete {
        code: String,
        idiom: IdiomFamily,
    },
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
    pub fn new() -> Self {
        Self {
            // Matches def foo():, int foo() {, fn foo() -> i32 {, func foo() {, function foo() {
            fn_re: Regex::new(
                r#"^(?:def|fn|func|function|(?:(?:int|int32|int64|float|double|bool|void|char\*|auto)\s+))\s*([a-zA-Z_0-9]+)\s*\((.*?)\)\s*(?:->\s*[a-zA-Z_0-9*]+\s*)?(?::|\{)?$"#,
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
            print_re: Regex::new(r#"^print\s*\(\s*(.*)\s*\)$"#).unwrap(),
            return_re: Regex::new(r#"^return(?:\s+(.*))?$"#).unwrap(),
            // Go scoped declaration: count := 0
            walrus_assign_re: Regex::new(r#"^([a-zA-Z_0-9]+)\s*:=\s*(.+)$"#).unwrap(),
            // Matches x = 5, int x = 0, let count = 10, var y = 2
            assign_re: Regex::new(
                r#"^(?:(let|var|int|int32|int64|float|double|bool|auto)\s+)?([a-zA-Z_0-9]+)\s*=\s*(.+)$"#,
            )
            .unwrap(),
            // Matches ch := make(chan int, 8), ch = Channel(10), let ch = Chan(4)
            make_chan_re: Regex::new(
                r#"^(?:(let|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*(?:make\s*\(\s*chan\s+[a-zA-Z_0-9*]+\s*(?:,\s*([0-9]+))?\s*\)|(?:Channel|Chan)\s*\(\s*([0-9]+)?\s*\))$"#,
            )
            .unwrap(),
            // Matches ch <- 42, out_chan.send(val)
            chan_send_re: Regex::new(
                r#"^(?:([a-zA-Z_0-9]+)\s+<-\s+(.+)|([a-zA-Z_0-9]+)\.send\s*\(\s*(.+)\s*\))$"#,
            )
            .unwrap(),
            // Matches val := <-ch, val = <-ch, val = ch.recv(), <-ch
            chan_recv_re: Regex::new(
                r#"^(?:(?:(let|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*)?(?:<-\s*([a-zA-Z_0-9]+)|([a-zA-Z_0-9]+)\.recv\s*\(\s*\))$"#,
            )
            .unwrap(),
            // Matches items = [10, 20, 30], items := []
            list_init_re: Regex::new(
                r#"^(?:(let|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*\[(.*)\]$"#,
            )
            .unwrap(),
            // Matches items.append(40), items.push(50)
            list_append_re: Regex::new(
                r#"^([a-zA-Z_0-9]+)\.(?:append|push)\s*\(\s*(.+)\s*\)$"#,
            )
            .unwrap(),
            // Matches items[0] = 99, m["key"] = 10
            index_set_re: Regex::new(
                r#"^([a-zA-Z_0-9]+)\[(.+?)\]\s*=\s*(.+)$"#,
            )
            .unwrap(),
            // Matches m = {"alice": 100, "bob": 200}, m := {}
            map_init_re: Regex::new(
                r#"^(?:(let|var)\s+)?([a-zA-Z_0-9]+)\s*(?::=|=)\s*\{(.*)\}$"#,
            )
            .unwrap(),
        }
    }

    /// Token-aware splitting that respects double and single quotes
    pub fn split_respecting_quotes(s: &str, delimiter: char) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut in_double_quote = false;
        let mut in_single_quote = false;
        let mut escape = false;

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
            if c == delimiter && !in_double_quote && !in_single_quote {
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

    /// Canonicalizes source code: unifies bracket blocks (`{ ... }`), `end` blocks, postfix conditionals,
    /// and increments (`i++`) into hierarchical canonical AST lines.
    /// Preserves operators like `:=` and `=>` without destructive string replacement.
    pub fn canonicalize_source(source: &str) -> String {
        let raw_lines = source.lines();
        let mut out_lines = Vec::new();
        let mut brace_depth: usize = 0;

        let inc_re = Regex::new(r#"^([a-zA-Z_0-9]+)\+\+$"#).unwrap();
        let dec_re = Regex::new(r#"^([a-zA-Z_0-9]+)\-\-$"#).unwrap();
        let postfix_if_re = Regex::new(r#"^(.*?)\s+if\s+(.*)$"#).unwrap();

        for line in raw_lines {
            let mut trimmed = line.trim();
            if trimmed.is_empty() {
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

            // Scoped indentation: if inside an explicit brace block, format via brace_depth.
            // If at brace_depth == 0 (e.g. standard Python functions), preserve exact original indentation!
            let current_indent = if brace_depth > 0 {
                "    ".repeat(brace_depth)
            } else {
                let count = Self::indent_level(line);
                " ".repeat(count)
            };

            // 7. Handle Ruby/Perl Postfix `if`: `return 0 if x > 10` -> `if x > 10: return 0`
            // Ensure we do NOT match inside string literals like print("what if now")
            if !line_str.starts_with("if ")
                && !line_str.starts_with("if(")
                && !line_str.contains('"')
                && !line_str.contains('\'')
            {
                if let Some(caps) = postfix_if_re.captures(&line_str) {
                    let stmt = caps[1].trim();
                    let cond = caps[2].trim();
                    out_lines.push(format!("{}if {}:", current_indent, cond));
                    out_lines.push(format!("{}    {}", current_indent, stmt));
                    continue;
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

    /// Parse an OmniScript source string into an `OmniModule`
    pub fn parse_module(&self, module_name: &str, raw_source: &str) -> OmniModule {
        let source = Self::canonicalize_source(raw_source);
        let mem_norm = MemoryNormalizer::new();
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut top_level = Vec::new();

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

            // Decorators
            if trimmed.starts_with('@') {
                current_decorators.push(trimmed.to_string());
                idx += 1;
                continue;
            }

            // Import directives
            if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
                if let Some(directive) = omni_preamble::SymbolCarver::parse_directive(trimmed) {
                    imports.push(directive);
                }
                idx += 1;
                continue;
            }

            // Class definitions: `class DataPipeline:` or `class Foo(Bar):`
            if trimmed.starts_with("class ") {
                let class_line = trimmed.trim_start_matches("class ").trim_end_matches(':').trim();
                let (class_name, bases) = if let Some(open) = class_line.find('(') {
                    let name = class_line[..open].trim().to_string();
                    let end = class_line.rfind(')').unwrap_or(class_line.len());
                    let base_str = &class_line[open + 1..end];
                    let base_list: Vec<String> = base_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (name, base_list)
                } else {
                    (class_line.to_string(), Vec::new())
                };

                let parent_indent = Self::indent_level(line);
                idx += 1;

                let mut class_methods = Vec::new();
                let mut class_fields = Vec::new();

                while idx < raw_lines.len() {
                    let next_line = raw_lines[idx];
                    let next_trimmed = next_line.trim();

                    if next_trimmed.is_empty() || next_trimmed.starts_with('#') || next_trimmed.starts_with("//") {
                        idx += 1;
                        continue;
                    }

                    let cur_indent = Self::indent_level(next_line);
                    if cur_indent <= parent_indent {
                        break;
                    }

                    if let Some(caps) = self.fn_re.captures(next_trimmed) {
                        let method_name = caps[1].to_string();
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
                            params: Vec::new(),
                            return_type: None,
                            body,
                            memory_ops,
                        });
                        continue;
                    }

                    if next_trimmed.contains('=') && !next_trimmed.starts_with("if") {
                        let parts: Vec<&str> = next_trimmed.splitn(2, '=').collect();
                        class_fields.push(FieldDef {
                            name: parts[0].trim().to_string(),
                            type_hint: None,
                            default_val: Some(parts[1].trim().to_string()),
                        });
                    }

                    idx += 1;
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

            // Top-level functions: `def foo():`, `fn foo() -> i32:`, `int foo() {`
            if let Some(caps) = self.fn_re.captures(trimmed) {
                let fn_name = caps[1].to_string();
                let raw_params = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let params: Vec<ParamDef> = raw_params
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|p| {
                        let name = p.split(':').next().unwrap_or(p).trim().split_whitespace().last().unwrap_or(p);
                        ParamDef {
                            name: name.to_string(),
                            type_hint: None,
                        }
                    })
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

                functions.push(FunctionDef {
                    name: fn_name,
                    is_method: false,
                    params,
                    return_type: None,
                    body,
                    memory_ops,
                });
                current_decorators.clear();
                continue;
            }

            // Top-level statement
            top_level.push(Statement::Concrete {
                code: trimmed.to_string(),
                idiom: IdiomFamily::Standard,
            });
            idx += 1;
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
            functions,
            top_level_statements: top_level,
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
            let (indent, ref text) = lines[i];

            // 1. If statement with optional elif/else
            if let Some(caps) = self.if_re.captures(text) {
                let condition = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str().trim().to_string();

                let (then_branch, mut next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                let mut else_branch = None;

                // Check for `elif` or `else` at the same indentation level
                if next_i < lines.len() && lines[next_i].0 == indent {
                    let next_text = lines[next_i].1.trim();
                    if let Some(elif_caps) = self.elif_re.captures(next_text) {
                        let elif_cond = elif_caps.get(1).or_else(|| elif_caps.get(2)).unwrap().as_str().trim().to_string();
                        let (elif_then, after_elif) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        else_branch = Some(vec![Statement::If {
                            condition: elif_cond,
                            then_branch: elif_then,
                            else_branch: None,
                        }]);
                        next_i = after_elif;
                    } else if self.else_re.is_match(next_text) {
                        let (else_body, after_else) = self.consume_indented_block(lines, next_i + 1, indent, fn_name, mem_norm);
                        else_branch = Some(else_body);
                        next_i = after_else;
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

            // 2. While loop
            if let Some(caps) = self.while_re.captures(text) {
                let condition = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str().trim().to_string();
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
                stmts.push(Statement::WhileLoop { condition, body });
                i = next_i;
                continue;
            }

            // 3. For loop
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
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent, fn_name, mem_norm);
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

            // 5. Return
            if let Some(caps) = self.return_re.captures(text) {
                let expr = caps.get(1).map(|m| m.as_str().trim().to_string());
                stmts.push(Statement::Return { expr });
                i += 1;
                continue;
            }

            // 6. Explicit Memory Operations (e.g. `temp_buf = alloc(2048)`, `defer free(temp_buf)`)
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

            // 8. Standard Assignment (e.g. `x = 5`, `int count = 10`, `let count = 0`)
            if let Some(caps) = self.assign_re.captures(text) {
                let is_scoped = caps.get(1).is_some();
                stmts.push(Statement::Assign {
                    target: caps[2].trim().to_string(),
                    expr: caps[3].trim().to_string(),
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
}
