use regex::Regex;
use serde::{Deserialize, Serialize};

/// Unified Memory Operation (UMO)
/// Normalized intermediate representation of memory instructions across all languages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnifiedMemOp {
    /// Allocation on heap or arena
    Alloc {
        target: String,
        size_bytes: String,
        is_arena: bool,
    },
    /// Explicit deallocation
    Free {
        target: String,
    },
    /// Immutable/shared borrow
    BorrowShared {
        target: String,
        source: String,
    },
    /// Mutable/exclusive borrow
    BorrowMut {
        target: String,
        source: String,
    },
    /// Ownership transfer / move
    Transfer {
        target: String,
        source: String,
    },
    /// Lifetime binding (e.g. self.field = allocated_buf)
    BindLifetime {
        parent: String,
        child: String,
    },
    /// Forbidden escape detection (e.g. returning address of local stack variable)
    EscapeScope {
        symbol: String,
        scope: String,
    },
}

pub struct MemoryNormalizer {
    alloc_re: Regex,
    free_re: Regex,
    new_re: Regex,
    defer_free_re: Regex,
    bind_re: Regex,
    escape_re: Regex,
}

impl Default for MemoryNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryNormalizer {
    pub fn new() -> Self {
        Self {
            // `let x = alloc(1024)` or `x = malloc(size)`
            alloc_re: Regex::new(
                r#"(?:let\s+)?([a-zA-Z_0-9.]+)\s*=\s*(?:alloc|malloc)\(([^)]+)\)"#,
            )
            .unwrap(),

            // `free(x)` or `drop(x)` or `delete x`
            free_re: Regex::new(r#"(?:free|drop)\s*\(\s*([a-zA-Z_0-9.]+)\s*\)|delete\s+([a-zA-Z_0-9.]+)"#).unwrap(),

            // `x = new ClassName(...)`
            new_re: Regex::new(r#"(?:let\s+)?([a-zA-Z_0-9.]+)\s*=\s*new\s+([a-zA-Z_0-9]+)\("#).unwrap(),

            // `defer free(x)`
            defer_free_re: Regex::new(r#"defer\s+(?:free|drop)\s*\(\s*([a-zA-Z_0-9.]+)\s*\)"#).unwrap(),

            // `self.foo = bar` (binding child lifetime to parent)
            bind_re: Regex::new(r#"self\.([a-zA-Z_0-9]+)\s*=\s*([a-zA-Z_0-9]+)"#).unwrap(),

            // `return &local_var` (dangerous scope escape)
            escape_re: Regex::new(r#"return\s+&([a-zA-Z_0-9]+)"#).unwrap(),
        }
    }

    /// Normalize a single line of polyglot source code into a UMO if applicable
    pub fn normalize_line(&self, line: &str, current_scope: &str) -> Option<UnifiedMemOp> {
        let trimmed = line.trim();

        // 1. Escaping local address check
        if let Some(caps) = self.escape_re.captures(trimmed) {
            return Some(UnifiedMemOp::EscapeScope {
                symbol: caps[1].to_string(),
                scope: current_scope.to_string(),
            });
        }

        // 2. Defer free check
        if let Some(caps) = self.defer_free_re.captures(trimmed) {
            return Some(UnifiedMemOp::Free {
                target: caps[1].to_string(),
            });
        }

        // 3. Alloc / Malloc check
        if let Some(caps) = self.alloc_re.captures(trimmed) {
            let target = caps[1].to_string();
            let size_bytes = caps[2].trim().to_string();
            return Some(UnifiedMemOp::Alloc {
                target,
                size_bytes,
                is_arena: false,
            });
        }

        // 4. Free check
        if let Some(caps) = self.free_re.captures(trimmed) {
            let target = caps
                .get(1)
                .or_else(|| caps.get(2))
                .unwrap()
                .as_str()
                .to_string();
            return Some(UnifiedMemOp::Free { target });
        }

        // 5. New object allocation
        if let Some(caps) = self.new_re.captures(trimmed) {
            let target = caps[1].to_string();
            let class_name = caps[2].to_string();
            return Some(UnifiedMemOp::Alloc {
                target,
                size_bytes: format!("sizeof({})", class_name),
                is_arena: false,
            });
        }

        // 6. Bind lifetime to self
        if let Some(caps) = self.bind_re.captures(trimmed) {
            return Some(UnifiedMemOp::BindLifetime {
                parent: "self".to_string(),
                child: caps[2].to_string(),
            });
        }

        None
    }

    /// Extract all UMOs from a multi-line block
    pub fn normalize_block(&self, code: &str, current_scope: &str) -> Vec<UnifiedMemOp> {
        let mut ops = Vec::new();
        for line in code.lines() {
            if let Some(op) = self.normalize_line(line, current_scope) {
                ops.push(op);
            }
        }
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_alloc_and_free() {
        let norm = MemoryNormalizer::new();
        let op1 = norm.normalize_line("let buffer = alloc(4096)", "func_main").unwrap();
        assert_eq!(
            op1,
            UnifiedMemOp::Alloc {
                target: "buffer".to_string(),
                size_bytes: "4096".to_string(),
                is_arena: false,
            }
        );

        let op2 = norm.normalize_line("free(buffer)", "func_main").unwrap();
        assert_eq!(
            op2,
            UnifiedMemOp::Free {
                target: "buffer".to_string(),
            }
        );
    }

    #[test]
    fn test_detect_forbidden_escape() {
        let norm = MemoryNormalizer::new();
        let op = norm.normalize_line("return &stack_var", "my_func").unwrap();
        assert_eq!(
            op,
            UnifiedMemOp::EscapeScope {
                symbol: "stack_var".to_string(),
                scope: "my_func".to_string(),
            }
        );
    }
}
