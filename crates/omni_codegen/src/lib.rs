use omni_engine::InferenceEngine;
use omni_mem::UnifiedMemOp;
use omni_parser::{OmniModule, Statement};
use regex::Regex;

pub struct CodeGenerator<'a> {
    engine: &'a dyn InferenceEngine,
    print_re: Regex,
    return_re: Regex,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self {
            engine,
            print_re: Regex::new(r#"^print\s*\(\s*(?:"|')(.*)(?:"|')\s*\)$"#).unwrap(),
            return_re: Regex::new(r#"^return\s+([0-9]+)$"#).unwrap(),
        }
    }

    /// Generate unified LLVM IR text from OmniModule
    pub fn generate_module_ir(&self, module: &OmniModule, temp: f32) -> anyhow::Result<String> {
        let mut ir = String::new();
        let mut string_constants: Vec<(String, String, usize)> = Vec::new();
        let mut str_counter = 0;

        // Pre-scan statements for print("...") to extract string constants
        for func in &module.functions {
            for stmt in &func.body {
                if let Statement::Concrete { code } = stmt {
                    if let Some(caps) = self.print_re.captures(code.trim()) {
                        let text = caps[1].to_string();
                        let const_name = format!("@.str.print_{}", str_counter);
                        str_counter += 1;
                        let len_with_null_and_newline = text.len() + 2; // \0A\00
                        string_constants.push((const_name, text, len_with_null_and_newline));
                    }
                }
            }
        }

        ir.push_str("; ModuleID = '");
        ir.push_str(&module.name);
        ir.push_str("'\nsource_filename = \"");
        ir.push_str(&module.name);
        ir.push_str(".omni\"\ntarget triple = \"x86_64-pc-linux-gnu\"\n\n");

        // Runtime headers & extern declarations
        ir.push_str("; Standard Runtime & libc Declarations\n");
        ir.push_str("declare i8* @malloc(i64)\n");
        ir.push_str("declare void @free(i8*)\n");
        ir.push_str("declare i32 @printf(i8*, ...)\n");
        ir.push_str("declare void @fast_hash_block(i8*, i64, i8*)\n");
        ir.push_str(omni_runtime::runtime_llvm_declarations());
        ir.push_str("\n");

        // Global string constants
        for (const_name, text, len) in &string_constants {
            ir.push_str(&format!(
                "{} = private unnamed_addr constant [{} x i8] c\"{}\\0A\\00\", align 1\n",
                const_name, len, text
            ));
        }
        ir.push_str("\n");

        // 1. Classes -> Struct Types
        for cls in &module.structs_and_classes {
            ir.push_str(&format!("; Class: {}\n", cls.name));
            ir.push_str(&format!("%struct.{} = type {{ i8* }}\n\n", cls.name));
        }

        // 2. Functions (namespaced as @__omni_fn_<name> to avoid C @main collision)
        let mut has_user_main = false;
        let mut cur_str_idx = 0;

        for func in &module.functions {
            if func.name == "main" {
                has_user_main = true;
            }

            ir.push_str(&format!(
                "define i32 @__omni_fn_{}(i8* %data, i64 %len) {{\nentry:\n",
                func.name
            ));

            // Allocate local buffers from UMO
            for op in &func.memory_ops {
                match op {
                    UnifiedMemOp::Alloc { target, size_bytes, .. } => {
                        let sz: u64 = size_bytes.parse().unwrap_or(64);
                        ir.push_str(&format!("  ; [UMO Alloc: {} bytes]\n", sz));
                        ir.push_str(&format!("  %{} = call i8* @malloc(i64 {})\n", target, sz));
                    }
                    UnifiedMemOp::Free { target } => {
                        ir.push_str(&format!("  ; [UMO Free: %{}]\n", target));
                        ir.push_str(&format!("  call void @free(i8* %{})\n", target));
                    }
                    _ => {}
                }
            }

            let mut custom_return = false;

            // Lower body statements
            for stmt in &func.body {
                match stmt {
                    Statement::Intent { instruction } => {
                        let synthesized = self.engine.lower_intent(&func.name, instruction, temp)?;
                        ir.push_str(&synthesized);
                    }
                    Statement::Concrete { code } => {
                        let trimmed = code.trim();
                        if let Some(_) = self.print_re.captures(trimmed) {
                            let (ref const_name, _, len) = string_constants[cur_str_idx];
                            cur_str_idx += 1;
                            ir.push_str(&format!(
                                "  %str_ptr_{} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                cur_str_idx, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  call i32 (i8*, ...) @printf(i8* %str_ptr_{})\n",
                                cur_str_idx
                            ));
                        } else if let Some(caps) = self.return_re.captures(trimmed) {
                            let ret_val = &caps[1];
                            ir.push_str(&format!("  ret i32 {}\n", ret_val));
                            custom_return = true;
                            break;
                        } else {
                            ir.push_str(&format!("  ; [Concrete]: {}\n", code));
                        }
                    }
                    _ => {}
                }
            }

            if !custom_return {
                ir.push_str("  ret i32 0\n");
            }
            ir.push_str("}\n\n");
        }

        // 3. Entrypoint @main
        ir.push_str("define i32 @main(i32 %argc, i8** %argv) {\nentry:\n");

        if has_user_main {
            // User defined `def main():`: delegate directly to user main!
            ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* null, i64 0)\n");
            ir.push_str("  ret i32 %exit_code\n");
        } else if let Some(f) = module.functions.first() {
            // Call the first available function
            ir.push_str(&format!(
                "  %res = call i32 @__omni_fn_{}(i8* null, i64 0)\n",
                f.name
            ));
            ir.push_str("  ret i32 0\n");
        } else {
            ir.push_str("  ret i32 0\n");
        }

        ir.push_str("}\n");

        Ok(ir)
    }
}
