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

    fn scan_strings_recursive(
        &self,
        stmts: &[Statement],
        string_constants: &mut Vec<(String, String, usize)>,
        counter: &mut usize,
    ) {
        for stmt in stmts {
            match stmt {
                Statement::Concrete { code } => {
                    if let Some(caps) = self.print_re.captures(code.trim()) {
                        let text = caps[1].to_string();
                        let const_name = format!("@.str.print_{}", *counter);
                        *counter += 1;
                        let len_with_null_and_newline = text.len() + 2; // \0A\00
                        string_constants.push((const_name, text, len_with_null_and_newline));
                    }
                }
                Statement::ForLoop { body, .. } => {
                    self.scan_strings_recursive(body, string_constants, counter);
                }
                _ => {}
            }
        }
    }

    fn lower_statements(
        &self,
        stmts: &[Statement],
        ir: &mut String,
        string_constants: &[(String, String, usize)],
        cur_str_idx: &mut usize,
        loop_counter: &mut usize,
        fn_name: &str,
        temp: f32,
    ) -> anyhow::Result<bool> {
        let mut custom_return = false;

        for stmt in stmts {
            match stmt {
                Statement::ForLoop { var, count, body } => {
                    let loop_id = *loop_counter;
                    *loop_counter += 1;

                    let count_num: i32 = count.parse().unwrap_or(10);
                    let idx_var = format!("%loop_idx_{}", loop_id);
                    let cond_lbl = format!("loop_cond_{}", loop_id);
                    let body_lbl = format!("loop_body_{}", loop_id);
                    let exit_lbl = format!("loop_exit_{}", loop_id);

                    ir.push_str(&format!("  ; --- Begin ForLoop {} in {} ---\n", var, count));
                    ir.push_str(&format!("  {} = alloca i32\n", idx_var));
                    ir.push_str(&format!("  store i32 0, i32* {}\n", idx_var));
                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

                    ir.push_str(&format!("{}:\n", cond_lbl));
                    ir.push_str(&format!("  %curr_{} = load i32, i32* {}\n", loop_id, idx_var));
                    ir.push_str(&format!("  %cmp_{} = icmp slt i32 %curr_{}, {}\n", loop_id, loop_id, count_num));
                    ir.push_str(&format!("  br i1 %cmp_{}, label %{}, label %{}\n\n", loop_id, body_lbl, exit_lbl));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    self.lower_statements(body, ir, string_constants, cur_str_idx, loop_counter, fn_name, temp)?;
                    
                    ir.push_str(&format!("  %next_{} = add i32 %curr_{}, 1\n", loop_id, loop_id));
                    ir.push_str(&format!("  store i32 %next_{}, i32* {}\n", loop_id, idx_var));
                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));

                    ir.push_str(&format!("{}:\n", exit_lbl));
                    ir.push_str(&format!("  ; --- End ForLoop {} ---\n", var));
                }
                Statement::Intent { instruction } => {
                    let synthesized = self.engine.lower_intent(fn_name, instruction, temp)?;
                    ir.push_str(&synthesized);
                }
                Statement::Concrete { code } => {
                    let trimmed = code.trim();
                    if let Some(_) = self.print_re.captures(trimmed) {
                        let (ref const_name, _, len) = string_constants[*cur_str_idx];
                        *cur_str_idx += 1;
                        ir.push_str(&format!(
                            "  %str_ptr_{} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                            *cur_str_idx, len, len, const_name
                        ));
                        ir.push_str(&format!(
                            "  call i32 (i8*, ...) @printf(i8* %str_ptr_{})\n",
                            *cur_str_idx
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

        Ok(custom_return)
    }

    /// Generate unified LLVM IR text from OmniModule
    pub fn generate_module_ir(&self, module: &OmniModule, temp: f32) -> anyhow::Result<String> {
        let mut ir = String::new();
        let mut string_constants: Vec<(String, String, usize)> = Vec::new();
        let mut str_counter = 0;

        // Pre-scan statements for print("...") recursively to extract string constants
        for func in &module.functions {
            self.scan_strings_recursive(&func.body, &mut string_constants, &mut str_counter);
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

        // 2. Functions (namespaced as @__omni_fn_<name>)
        let mut has_user_main = false;
        let mut cur_str_idx = 0;
        let mut loop_counter = 0;

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

            let custom_ret = self.lower_statements(
                &func.body,
                &mut ir,
                &string_constants,
                &mut cur_str_idx,
                &mut loop_counter,
                &func.name,
                temp,
            )?;

            if !custom_ret {
                ir.push_str("  ret i32 0\n");
            }
            ir.push_str("}\n\n");
        }

        // 3. Entrypoint @main
        ir.push_str("define i32 @main(i32 %argc, i8** %argv) {\nentry:\n");

        if has_user_main {
            ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* null, i64 0)\n");
            ir.push_str("  ret i32 %exit_code\n");
        } else if let Some(f) = module.functions.first() {
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
