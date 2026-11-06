use omni_mem::UnifiedMemOp;
use omni_preamble::ImportDirective;
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
    /// Fully determined concrete code (let, if, for, return, direct ptrs)
    Concrete {
        code: String,
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

pub struct OmniParser;

impl Default for OmniParser {
    fn default() -> Self {
        Self::new()
    }
}

impl OmniParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse an OmniScript source string into an `OmniModule`
    pub fn parse_module(&self, module_name: &str, source: &str) -> OmniModule {
        let mem_norm = omni_mem::MemoryNormalizer::new();
        let mut imports = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut top_level = Vec::new();

        let mut lines = source.lines().peekable();
        let mut current_decorators: Vec<String> = Vec::new();

        while let Some(line) = lines.next() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            // 1. Decorator `@dataclass`
            if trimmed.starts_with('@') {
                current_decorators.push(trimmed.to_string());
                continue;
            }

            // 2. Import Directive
            if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
                if let Some(directive) = omni_preamble::SymbolCarver::parse_directive(trimmed) {
                    imports.push(directive);
                }
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
                continue;
            }

            // 4. Function Definition
            if trimmed.starts_with("def ") {
                let sig = trimmed[4..].trim_end_matches(':').trim();
                let fn_name = sig.split('(').next().unwrap_or("unnamed").trim().to_string();
                
                let mut body = Vec::new();
                let mut raw_body_lines = Vec::new();

                // Consume indented or bracketed lines for function body
                while let Some(&next_line) = lines.peek() {
                    let next_trimmed = next_line.trim();
                    if next_trimmed.is_empty() {
                        lines.next();
                        continue;
                    }

                    // Check indentation (simple check for leading whitespace)
                    if next_line.starts_with("    ") || next_line.starts_with('\t') {
                        raw_body_lines.push(next_trimmed.to_string());
                        
                        if next_trimmed.starts_with("intent") {
                            body.push(Statement::Intent {
                                instruction: next_trimmed.to_string(),
                            });
                        } else {
                            body.push(Statement::Concrete {
                                code: next_trimmed.to_string(),
                            });
                        }
                        lines.next();
                    } else {
                        break;
                    }
                }

                let memory_ops = mem_norm.normalize_block(&raw_body_lines.join("\n"), &fn_name);

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

            // Default top-level statement
            top_level.push(Statement::Concrete {
                code: trimmed.to_string(),
            });
        }

        OmniModule {
            name: module_name.to_string(),
            imports,
            structs_and_classes: classes,
            functions,
            top_level_statements: top_level,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_omni_source() {
        let src = r#"
import "crypto.c" as Crypto
from "helpers" import RateLimiter

@dataclass
class Service:
    pass

def process_stream(data):
    let buf = alloc(1024)
    intent: filter streaming packets where temp > 80.0
    free(buf)
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("test_mod", src);

        assert_eq!(module.imports.len(), 2);
        assert_eq!(module.structs_and_classes.len(), 1);
        assert_eq!(module.structs_and_classes[0].name, "Service");
        assert_eq!(module.functions.len(), 1);
        assert_eq!(module.functions[0].name, "process_stream");
        assert_eq!(module.functions[0].memory_ops.len(), 2); // alloc & free
    }
}
