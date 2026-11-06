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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    /// Fully determined concrete code (let, if, return, direct ptrs, print)
    Concrete {
        code: String,
    },
    /// For loop: `for var in count/range:`
    ForLoop {
        var: String,
        count: String,
        body: Vec<Statement>,
    },
    /// Abstract intent block awaiting model synthesis
    Intent {
        instruction: String,
    },
    /// Embedded polyglot foreign block
    Polyglot {
        language: String,
        code: String,
    },
}

pub struct OmniParser {
    for_re: Regex,
}

impl Default for OmniParser {
    fn default() -> Self {
        Self::new()
    }
}

impl OmniParser {
    pub fn new() -> Self {
        Self {
            for_re: Regex::new(
                r#"^for\s+([a-zA-Z_0-9]+)\s+in\s+(?:range\s*\(\s*)?([a-zA-Z_0-9]+)(?:\s*\))?\s*:$"#,
            )
            .unwrap(),
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

    /// Recursively parse indented block lines into statements (including loops)
    fn parse_block(&self, lines: &[(usize, String)]) -> Vec<Statement> {
        let mut stmts = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let (indent, ref text) = lines[i];

            if let Some(caps) = self.for_re.captures(text) {
                let var = caps[1].to_string();
                let count = caps[2].to_string();

                // Collect nested lines indented deeper than this for-loop
                let mut nested = Vec::new();
                let mut j = i + 1;
                while j < lines.len() {
                    if lines[j].0 > indent {
                        nested.push(lines[j].clone());
                        j += 1;
                    } else {
                        break;
                    }
                }

                let inner_body = self.parse_block(&nested);
                stmts.push(Statement::ForLoop {
                    var,
                    count,
                    body: inner_body,
                });

                i = j;
            } else if text.starts_with("intent") {
                stmts.push(Statement::Intent {
                    instruction: text.clone(),
                });
                i += 1;
            } else {
                stmts.push(Statement::Concrete {
                    code: text.clone(),
                });
                i += 1;
            }
        }

        stmts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_for_loop() {
        let src = r#"
def main():
   for i in 10:
     print("Hello world!")
   return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("test_mod", src);

        assert_eq!(module.functions.len(), 1);
        let f = &module.functions[0];
        assert_eq!(f.name, "main");
        assert_eq!(f.body.len(), 2); // for loop and return 0

        if let Statement::ForLoop { var, count, body } = &f.body[0] {
            assert_eq!(var, "i");
            assert_eq!(count, "10");
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Statement::ForLoop");
        }
    }
}
