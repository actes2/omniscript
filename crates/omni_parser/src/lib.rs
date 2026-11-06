use omni_mem::UnifiedMemOp;
use omni_preamble::ImportDirective;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Statement {
    /// Variable assignment: `x = 5`, `int x = 0`, `let count = x + 1`
    Assign {
        target: String,
        expr: String,
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
    /// Abstract intent block awaiting model synthesis
    Intent {
        instruction: String,
    },
    /// Fallback concrete or polyglot statement
    Concrete {
        code: String,
    },
}

pub struct OmniParser {
    fn_re: Regex,
    if_re: Regex,
    elif_re: Regex,
    else_re: Regex,
    while_re: Regex,
    for_re: Regex,
    print_re: Regex,
    return_re: Regex,
    assign_re: Regex,
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
                r#"^(?:def|fn|func|function|(?:[a-zA-Z_0-9*]+\s+)+)\s*([a-zA-Z_0-9]+)\s*\((.*?)\)\s*(?:->\s*[a-zA-Z_0-9*]+\s*)?(?::|\{)?$"#,
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
            // Matches for (i in 10) {, for (int i = 0; i < 10; i++) {, for i in 10:
            for_re: Regex::new(
                r#"^for\s*(?:\((?:(?:int|let|var)\s+)?([a-zA-Z_0-9]+)\s+(?:in|:=|=)\s+(?:0\s*;\s*[a-zA-Z_0-9]+\s*<\s*)?(?:range\s*\(\s*)?([a-zA-Z_0-9]+)(?:\s*\))?(?:;\s*[^)]*)?\)|(?:(?:int|let|var)\s+)?([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*)?([a-zA-Z_0-9]+)(?:\s*\))?)\s*(?::|\{)?$"#,
            )
            .unwrap(),
            print_re: Regex::new(r#"^print\s*\(\s*(.*)\s*\)$"#).unwrap(),
            return_re: Regex::new(r#"^return(?:\s+(.*))?$"#).unwrap(),
            // Matches x = 5, int x = 0, let count = 10, var y = 2
            assign_re: Regex::new(
                r#"^(?:let\s+|var\s+|(?:int|int32|int64|float|double|bool|auto)\s+)?([a-zA-Z_0-9]+)\s*=\s*(.+)$"#,
            )
            .unwrap(),
        }
    }

    /// Canonicalizes source code: unifies bracket blocks (`{ ... }`), `end` blocks, postfix conditionals,
    /// increments (`i++`), and walrus operators (`:=`) into hierarchical canonical AST lines.
    pub fn canonicalize_source(source: &str) -> String {
        let raw_lines = source.lines();
        let has_explicit_blocks = source.contains('{') || source.contains('}') || source.contains("end");

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

            // 5. Replace Go walrus `:=` with `=`
            line_str = line_str.replace(":=", "=");

            // 6. Handle C-style increment/decrement: `i++` -> `i = i + 1`
            if let Some(caps) = inc_re.captures(&line_str) {
                let var = &caps[1];
                line_str = format!("{} = {} + 1", var, var);
            } else if let Some(caps) = dec_re.captures(&line_str) {
                let var = &caps[1];
                line_str = format!("{} = {} - 1", var, var);
            }

            // 7. Check if line ends with opening brace
            let ends_with_open = line_str.ends_with('{');
            if ends_with_open {
                line_str = line_str[..line_str.len() - 1].trim().to_string();
            }

            let current_indent = if has_explicit_blocks {
                "    ".repeat(brace_depth)
            } else {
                let count = Self::indent_level(line);
                " ".repeat(count)
            };

            // 8. Handle Ruby/Perl Postfix `if`: `return 0 if x > 10` -> `if x > 10: return 0`
            if !line_str.starts_with("if ") && !line_str.starts_with("if(") {
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
        let mem_norm = omni_mem::MemoryNormalizer::new();
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

            // 1. Decorator `@dataclass`
            if trimmed.starts_with('@') {
                current_decorators.push(trimmed.to_string());
                idx += 1;
                continue;
            }

            // 2. Import Directive
            if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
                if let Some(directive) = omni_preamble::SymbolCarver::parse_directive(trimmed) {
                    imports.push(directive);
                }
                idx += 1;
                continue;
            }

            // 3. Class Definition
            if trimmed.starts_with("class ") {
                let class_name = trimmed[6..]
                    .trim_end_matches(':')
                    .trim()
                    .to_string();
                let class_def = ClassDef {
                    name: class_name,
                    decorators: current_decorators.clone(),
                    base_classes: Vec::new(),
                    fields: Vec::new(),
                    methods: Vec::new(),
                };
                classes.push(class_def);
                current_decorators.clear();
                idx += 1;
                continue;
            }

            // 4. Function Definition (across Python, C, Go, Rust, JS styles)
            if let Some(caps) = self.fn_re.captures(trimmed) {
                let fn_name = caps[1].to_string();
                let fn_indent = Self::indent_level(line);
                
                idx += 1;
                let mut block_lines: Vec<(usize, String)> = Vec::new();

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

                let body = self.parse_block(&block_lines);
                let raw_joined = block_lines.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join("\n");
                let memory_ops = mem_norm.normalize_block(&raw_joined, &fn_name);

                functions.push(FunctionDef {
                    name: fn_name,
                    is_method: false,
                    params: Vec::new(),
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
            });
            idx += 1;
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
    fn parse_block(&self, lines: &[(usize, String)]) -> Vec<Statement> {
        let mut stmts = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let (indent, ref text) = lines[i];

            // 1. If statement with optional elif/else
            if let Some(caps) = self.if_re.captures(text) {
                let condition = caps.get(1).or_else(|| caps.get(2)).unwrap().as_str().trim().to_string();

                let (then_branch, mut next_i) = self.consume_indented_block(lines, i + 1, indent);
                let mut else_branch = None;

                // Check for `elif` or `else` at the same indentation level
                if next_i < lines.len() && lines[next_i].0 == indent {
                    let next_text = lines[next_i].1.trim();
                    if let Some(elif_caps) = self.elif_re.captures(next_text) {
                        let elif_cond = elif_caps.get(1).or_else(|| elif_caps.get(2)).unwrap().as_str().trim().to_string();
                        let (elif_then, after_elif) = self.consume_indented_block(lines, next_i + 1, indent);
                        else_branch = Some(vec![Statement::If {
                            condition: elif_cond,
                            then_branch: elif_then,
                            else_branch: None,
                        }]);
                        next_i = after_elif;
                    } else if self.else_re.is_match(next_text) {
                        let (else_body, after_else) = self.consume_indented_block(lines, next_i + 1, indent);
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
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent);
                stmts.push(Statement::WhileLoop { condition, body });
                i = next_i;
                continue;
            }

            // 3. For loop
            if let Some(caps) = self.for_re.captures(text) {
                let var = caps.get(1).or_else(|| caps.get(3)).unwrap().as_str().to_string();
                let count = caps.get(2).or_else(|| caps.get(4)).unwrap().as_str().to_string();
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent);
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

            // 6. Assignment (e.g. `x = 5`, `int count = 10`)
            if let Some(caps) = self.assign_re.captures(text) {
                stmts.push(Statement::Assign {
                    target: caps[1].trim().to_string(),
                    expr: caps[2].trim().to_string(),
                });
                i += 1;
                continue;
            }

            // 7. Intent
            if text.starts_with("intent") {
                stmts.push(Statement::Intent {
                    instruction: text.clone(),
                });
                i += 1;
                continue;
            }

            // 8. Fallback Concrete / Non-standard syntax
            stmts.push(Statement::Concrete {
                code: text.clone(),
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

        let body = self.parse_block(&nested);
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
        
        // count = 0, count = count + 1, if count > 0: return 0
        assert_eq!(f.body.len(), 3);
        assert_eq!(f.body[0], Statement::Assign { target: "count".to_string(), expr: "0".to_string() });
        assert_eq!(f.body[1], Statement::Assign { target: "count".to_string(), expr: "count + 1".to_string() });
        
        if let Statement::If { condition, then_branch, .. } = &f.body[2] {
            assert_eq!(condition, "count > 0");
            assert_eq!(then_branch.len(), 1);
            assert_eq!(then_branch[0], Statement::Return { expr: Some("0".to_string()) });
        } else {
            panic!("Expected Statement::If for postfix if");
        }
    }
}
