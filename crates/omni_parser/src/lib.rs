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
    /// Variable assignment: `x = 5` or `let count = x + 1`
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
    /// While loop: `while cond:`
    WhileLoop {
        condition: String,
        body: Vec<Statement>,
    },
    /// For loop: `for var in count/range:`
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
            if_re: Regex::new(r#"^if\s+(.+):$"#).unwrap(),
            elif_re: Regex::new(r#"^elif\s+(.+):$"#).unwrap(),
            else_re: Regex::new(r#"^else\s*:$"#).unwrap(),
            while_re: Regex::new(r#"^while\s+(.+):$"#).unwrap(),
            for_re: Regex::new(
                r#"^for\s+([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*)?([a-zA-Z_0-9]+)(?:\s*\))?\s*:$"#,
            )
            .unwrap(),
            print_re: Regex::new(r#"^print\s*\(\s*(.*)\s*\)$"#).unwrap(),
            return_re: Regex::new(r#"^return(?:\s+(.*))?$"#).unwrap(),
            assign_re: Regex::new(r#"^(?:let\s+)?([a-zA-Z_0-9]+)\s*=\s*(.+)$"#).unwrap(),
        }
    }

    /// Measure indentation level of a raw line
    fn indent_level(line: &str) -> usize {
        line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
    }

    /// Parse an OmniScript source string into an `OmniModule`
    pub fn parse_module(&self, module_name: &str, source: &str) -> OmniModule {
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

            // 4. Function Definition
            if trimmed.starts_with("def ") {
                let sig = trimmed[4..].trim_end_matches(':').trim();
                let fn_name = sig.split('(').next().unwrap_or("unnamed").trim().to_string();
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
                let condition = caps[1].trim().to_string();

                let (then_branch, mut next_i) = self.consume_indented_block(lines, i + 1, indent);
                let mut else_branch = None;

                // Check for `elif` or `else` at the same indentation level
                if next_i < lines.len() && lines[next_i].0 == indent {
                    let next_text = lines[next_i].1.trim();
                    if let Some(elif_caps) = self.elif_re.captures(next_text) {
                        let elif_cond = elif_caps[1].trim().to_string();
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
                let condition = caps[1].trim().to_string();
                let (body, next_i) = self.consume_indented_block(lines, i + 1, indent);
                stmts.push(Statement::WhileLoop { condition, body });
                i = next_i;
                continue;
            }

            // 3. For loop
            if let Some(caps) = self.for_re.captures(text) {
                let var = caps[1].to_string();
                let count = caps[2].to_string();
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

            // 6. Assignment (e.g. `x = 5`, `let val = a + b`)
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
    fn test_parse_if_else() {
        let src = r#"
def check(val):
   if val > 10:
     print("Greater")
   else:
     print("Lesser")
   return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("test_mod", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.body.len(), 2); // If and Return

        if let Statement::If { condition, then_branch, else_branch } = &f.body[0] {
            assert_eq!(condition, "val > 10");
            assert_eq!(then_branch.len(), 1);
            assert!(else_branch.is_some());
            assert_eq!(else_branch.as_ref().unwrap().len(), 1);
        } else {
            panic!("Expected Statement::If");
        }
    }

    #[test]
    fn test_parse_while_loop() {
        let src = r#"
def loop_test():
   x = 10
   while x > 0:
     print(x)
     x = x - 1
   return x
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("test_mod", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.body.len(), 3); // Assign, While, Return

        if let Statement::WhileLoop { condition, body } = &f.body[1] {
            assert_eq!(condition, "x > 0");
            assert_eq!(body.len(), 2);
        } else {
            panic!("Expected Statement::WhileLoop");
        }
    }
}
