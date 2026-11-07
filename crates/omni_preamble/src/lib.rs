use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum PreambleError {
    #[error("Import path '{0}' does not exist")]
    PathNotFound(String),

    #[error("Target '{0}' is not a directory for wildcard/from-import")]
    NotADirectory(String),

    #[error("Symbol '{symbol}' not found in directory '{dir}'. Available candidates: {candidates:?}")]
    SymbolNotFound {
        symbol: String,
        dir: String,
        candidates: Vec<String>,
    },

    #[error("Ambiguous symbol '{symbol}' found in multiple locations:\n{locations}")]
    AmbiguousSymbol { symbol: String, locations: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportDirective {
    /// `import "path/file.ext" as Alias`
    FileAlias {
        path: PathBuf,
        alias: Option<String>,
    },
    /// `import "dir" as Alias`
    DirectoryAlias {
        path: PathBuf,
        alias: Option<String>,
    },
    /// `from "dir" import Symbol1, Symbol2`
    FromDirectory {
        dir_path: PathBuf,
        symbols: Vec<String>,
    },
    /// Ecosystem dialect import: `import 'python' time as pytime`, `import 'c' "sys/socket"`
    Ecosystem {
        ecosystem: String,
        module: String,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct CarvedSymbol {
    pub name: String,
    pub source_file: PathBuf,
    pub line_number: usize,
    pub language: String,
    pub signature: String,
    pub is_uncaged: bool,
}

pub struct SymbolCarver {
    go_func_re: Regex,
    go_type_re: Regex,
    py_class_re: Regex,
    py_func_re: Regex,
    c_func_re: Regex,
    c_struct_re: Regex,
    js_func_re: Regex,
    js_class_re: Regex,
    rs_fn_re: Regex,
    rs_struct_re: Regex,
}

impl Default for SymbolCarver {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolCarver {
    pub fn new() -> Self {
        Self {
            // Go: Matches `func (r *Limiter) allow(...)` or `func allow(...)`
            go_func_re: Regex::new(r"func\s+(?:\([^)]*\)\s+)?([a-zA-Z_0-9]+)\s*\(").unwrap(),
            go_type_re: Regex::new(r"type\s+([a-zA-Z_0-9]+)\s+(?:struct|interface)").unwrap(),

            // Python: `class Foo:` or `def bar(`
            py_class_re: Regex::new(r"class\s+([a-zA-Z_0-9]+)\b").unwrap(),
            py_func_re: Regex::new(r"def\s+([a-zA-Z_0-9]+)\s*\(").unwrap(),

            // C / C++: `void fast_hash(...)` or `struct PacketHeader`
            c_func_re: Regex::new(
                r"(?:[a-zA-Z_0-9*]+\s+)+([a-zA-Z_0-9]+)\s*\([^)]*\)\s*\{?",
            )
            .unwrap(),
            c_struct_re: Regex::new(r"(?:typedef\s+)?(?:struct|class)\s+([a-zA-Z_0-9]+)").unwrap(),

            // JS / TS: `function parse(` or `class Parser` or `export const parse =`
            js_func_re: Regex::new(
                r"(?:export\s+)?(?:async\s+)?function\s+([a-zA-Z_0-9]+)\s*\(",
            )
            .unwrap(),
            js_class_re: Regex::new(r"(?:export\s+)?class\s+([a-zA-Z_0-9]+)").unwrap(),

            // Rust: `pub fn parse(` or `pub struct Parser`
            rs_fn_re: Regex::new(r"(?:pub(?:\(crate\))?\s+)?fn\s+([a-zA-Z_0-9]+)\s*\(").unwrap(),
            rs_struct_re: Regex::new(
                r"(?:pub(?:\(crate\))?\s+)?(?:struct|enum|trait)\s+([a-zA-Z_0-9]+)",
            )
            .unwrap(),
        }
    }

    /// Parse an import line into an `ImportDirective`
    pub fn parse_directive(line: &str) -> Option<ImportDirective> {
        let trimmed = line.trim();

        // 1. Ecosystem dialect import: `import 'python' time [as alias]`, `import 'c' "sys/socket"`, `import 'python'time`
        let eco_re = Regex::new(
            r#"^import\s+['"]([a-zA-Z0-9_\-]+)['"]\s*["']?([a-zA-Z0-9_\-./:]+)["']?(?:\s+as\s+([a-zA-Z_0-9]+))?$"#,
        ).unwrap();
        if let Some(caps) = eco_re.captures(trimmed) {
            let ecosystem = caps[1].to_string();
            let module = caps[2].to_string();
            let alias = caps.get(3).map(|m| m.as_str().to_string());
            return Some(ImportDirective::Ecosystem {
                ecosystem,
                module,
                alias,
            });
        }

        // 2. `from "stuff" import Symbol1, Symbol2`
        let from_re = Regex::new(r#"^from\s+"([^"]+)"\s+import\s+(.+)$"#).unwrap();
        if let Some(caps) = from_re.captures(trimmed) {
            let dir_path = PathBuf::from(&caps[1]);
            let symbols = caps[2]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            return Some(ImportDirective::FromDirectory { dir_path, symbols });
        }

        // 3. `import "path" (as Alias)?`
        let import_re = Regex::new(r#"^import\s+"([^"]+)"(?:\s+as\s+([a-zA-Z_0-9]+))?$"#).unwrap();
        if let Some(caps) = import_re.captures(trimmed) {
            let path = PathBuf::from(&caps[1]);
            let alias = caps.get(2).map(|m| m.as_str().to_string());

            if path.extension().is_some() {
                return Some(ImportDirective::FileAlias { path, alias });
            } else {
                return Some(ImportDirective::DirectoryAlias { path, alias });
            }
        }

        None
    }

    /// Verify directory/file path and carve the requested symbol from a directory
    pub fn carve_symbol_from_dir(
        &self,
        base_dir: &Path,
        dir_path: &Path,
        symbol_name: &str,
    ) -> Result<CarvedSymbol, PreambleError> {
        let full_dir = if dir_path.is_absolute() {
            dir_path.to_path_buf()
        } else {
            base_dir.join(dir_path)
        };

        if !full_dir.exists() {
            return Err(PreambleError::PathNotFound(
                full_dir.display().to_string(),
            ));
        }

        if !full_dir.is_dir() {
            return Err(PreambleError::NotADirectory(
                full_dir.display().to_string(),
            ));
        }

        let mut matches: Vec<CarvedSymbol> = Vec::new();
        let mut all_candidates: Vec<String> = Vec::new();

        for entry in WalkDir::new(&full_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            // Match against file name directly: e.g. parser.go or parser.c matching "Parser"
            let file_stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if file_stem.eq_ignore_ascii_case(symbol_name) {
                // Whole file target match
                return Ok(CarvedSymbol {
                    name: symbol_name.to_string(),
                    source_file: path.to_path_buf(),
                    line_number: 1,
                    language: ext.clone(),
                    signature: format!("file module {}", path.display()),
                    is_uncaged: false,
                });
            }

            if let Ok(content) = fs::read_to_string(path) {
                let symbols_in_file = self.carve_from_content(path, &ext, &content);
                for sym in symbols_in_file {
                    if sym.name == symbol_name {
                        matches.push(sym);
                    } else if !all_candidates.contains(&sym.name) {
                        all_candidates.push(sym.name);
                    }
                }
            }
        }

        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(PreambleError::SymbolNotFound {
                symbol: symbol_name.to_string(),
                dir: full_dir.display().to_string(),
                candidates: all_candidates,
            }),
            _ => {
                let locations = matches
                    .iter()
                    .map(|m| {
                        format!(
                            "  - {}:{} [{}] {}",
                            m.source_file.display(),
                            m.line_number,
                            m.language,
                            m.signature
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Err(PreambleError::AmbiguousSymbol {
                    symbol: symbol_name.to_string(),
                    locations,
                })
            }
        }
    }

    /// Carve all recognizable symbols from file content
    pub fn carve_from_content(
        &self,
        path: &Path,
        ext: &str,
        content: &str,
    ) -> Vec<CarvedSymbol> {
        let mut symbols = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let trimmed = line.trim();

            match ext {
                "go" => {
                    if let Some(caps) = self.go_func_re.captures(trimmed) {
                        let name = caps[1].to_string();
                        let is_uncaged = name.chars().next().map_or(false, |c| c.is_lowercase());
                        symbols.push(CarvedSymbol {
                            name,
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "go".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged,
                        });
                    } else if let Some(caps) = self.go_type_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "go".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    }
                }
                "py" | "omni" => {
                    if let Some(caps) = self.py_class_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "python".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    } else if let Some(caps) = self.py_func_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "python".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    }
                }
                "c" | "h" | "cpp" | "hpp" => {
                    if let Some(caps) = self.c_struct_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "c".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    } else if let Some(caps) = self.c_func_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "c".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    }
                }
                "js" | "ts" => {
                    if let Some(caps) = self.js_class_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "javascript".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    } else if let Some(caps) = self.js_func_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "javascript".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    }
                }
                "rs" => {
                    if let Some(caps) = self.rs_struct_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "rust".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    } else if let Some(caps) = self.rs_fn_re.captures(trimmed) {
                        symbols.push(CarvedSymbol {
                            name: caps[1].to_string(),
                            source_file: path.to_path_buf(),
                            line_number: line_num,
                            language: "rust".to_string(),
                            signature: trimmed.to_string(),
                            is_uncaged: false,
                        });
                    }
                }
                _ => {}
            }
        }

        symbols
    }
}

use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct DependencySlice {
    pub local_sources: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub external_frontier: Vec<String>,
}

pub struct DependencyDriller {
    visited: HashSet<PathBuf>,
    include_c_re: Regex,
}

impl Default for DependencyDriller {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyDriller {
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            include_c_re: Regex::new(r#"^\s*#include\s*[<"]([^>"]+)[>"]"#).unwrap(),
        }
    }

    /// Recursively drill down imported files, extracting concrete source dependencies,
    /// include paths, and isolating the terminal external frontier (missing/system headers).
    pub fn drill(&mut self, start_path: &Path) -> DependencySlice {
        let mut slice = DependencySlice::default();
        self.walk_recursive(start_path, &mut slice);
        slice.local_sources.sort();
        slice.local_sources.dedup();
        slice.include_dirs.sort();
        slice.include_dirs.dedup();
        slice.external_frontier.sort();
        slice.external_frontier.dedup();
        slice
    }

    fn walk_recursive(&mut self, path: &Path, slice: &mut DependencySlice) {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
        };

        if self.visited.contains(&abs_path) || !abs_path.exists() {
            return;
        }

        self.visited.insert(abs_path.clone());
        if abs_path.is_file() {
            slice.local_sources.push(abs_path.clone());
            if let Some(parent) = abs_path.parent() {
                slice.include_dirs.push(parent.to_path_buf());
            }

            if let Ok(content) = fs::read_to_string(&abs_path) {
                let parent = abs_path.parent().unwrap_or_else(|| Path::new("."));
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(caps) = self.include_c_re.captures(trimmed) {
                        let target = &caps[1];
                        let candidate = parent.join(target);
                        if candidate.exists() {
                            self.walk_recursive(&candidate, slice);
                        } else {
                            slice.external_frontier.push(target.to_string());
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_parse_import_directives() {
        let d0 = SymbolCarver::parse_directive("import 'python' time").unwrap();
        assert_eq!(
            d0,
            ImportDirective::Ecosystem {
                ecosystem: "python".to_string(),
                module: "time".to_string(),
                alias: None,
            }
        );

        let d0_bare = SymbolCarver::parse_directive("import 'python'time").unwrap();
        assert_eq!(
            d0_bare,
            ImportDirective::Ecosystem {
                ecosystem: "python".to_string(),
                module: "time".to_string(),
                alias: None,
            }
        );

        let d0_alias = SymbolCarver::parse_directive("import 'rust' std::time as rtime").unwrap();
        assert_eq!(
            d0_alias,
            ImportDirective::Ecosystem {
                ecosystem: "rust".to_string(),
                module: "std::time".to_string(),
                alias: Some("rtime".to_string()),
            }
        );

        let d1 = SymbolCarver::parse_directive(r#"import "foo.js" as Foo"#).unwrap();
        assert_eq!(
            d1,
            ImportDirective::FileAlias {
                path: PathBuf::from("foo.js"),
                alias: Some("Foo".to_string())
            }
        );

        let d2 = SymbolCarver::parse_directive(r#"from "stuff" import Parser, TokenBucket"#).unwrap();
        assert_eq!(
            d2,
            ImportDirective::FromDirectory {
                dir_path: PathBuf::from("stuff"),
                symbols: vec!["Parser".to_string(), "TokenBucket".to_string()]
            }
        );
    }

    #[test]
    fn test_carve_go_uncaged_symbol() {
        let carver = SymbolCarver::new();
        let temp_dir = std::env::temp_dir().join("omni_test_go");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let go_file = temp_dir.join("limiter.go");
        let mut f = File::create(&go_file).unwrap();
        writeln!(
            f,
            r#"package limiter

type RateLimiter struct {{
    tokens int64
}}

func (r *RateLimiter) allow() bool {{
    return true
}}
"#
        )
        .unwrap();

        let sym = carver
            .carve_symbol_from_dir(&temp_dir, Path::new("."), "allow")
            .unwrap();
        assert_eq!(sym.name, "allow");
        assert_eq!(sym.language, "go");
        assert!(sym.is_uncaged); // verifies unexported lowercase Go func is successfully un-caged!

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_dependency_driller_recursive() {
        let temp_dir = std::env::temp_dir().join("omni_test_driller");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let header_file = temp_dir.join("local_types.h");
        fs::write(&header_file, "#include <time.h>\ntypedef int my_int;\n").unwrap();

        let source_file = temp_dir.join("worker.c");
        fs::write(&source_file, "#include \"local_types.h\"\n#include <unistd.h>\n").unwrap();

        let mut driller = DependencyDriller::new();
        let slice = driller.drill(&source_file);

        assert!(slice.local_sources.contains(&source_file));
        assert!(slice.local_sources.contains(&header_file));
        assert!(slice.external_frontier.contains(&"time.h".to_string()));
        assert!(slice.external_frontier.contains(&"unistd.h".to_string()));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
