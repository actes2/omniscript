use omni_engine::InferenceEngine;
use omni_mem::UnifiedMemOp;
use omni_parser::{OmniModule, Statement};
use regex::Regex;
use std::collections::HashMap;

pub struct CodeGenerator<'a> {
    engine: &'a dyn InferenceEngine,
    cmp_re: Regex,
    bin_re: Regex,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self {
            engine,
            cmp_re: Regex::new(r#"^\s*(.*?)\s*(==|!=|<=|>=|<|>)\s*(.*?)\s*$"#).unwrap(),
            bin_re: Regex::new(r#"^\s*(.*?)\s*(\+|\-|\*|/|%)\s*(.*?)\s*$"#).unwrap(),
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
                Statement::Print { expr } => {
                    let trimmed = expr.trim();
                    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    {
                        let text = trimmed[1..trimmed.len() - 1].to_string();
                        let const_name = format!("@.str.print_{}", *counter);
                        *counter += 1;
                        let len_with_null = text.len() + 1; // \00
                        string_constants.push((const_name, text, len_with_null));
                    }
                }
                Statement::If { then_branch, else_branch, .. } => {
                    self.scan_strings_recursive(then_branch, string_constants, counter);
                    if let Some(eb) = else_branch {
                        self.scan_strings_recursive(eb, string_constants, counter);
                    }
                }
                Statement::WhileLoop { body, .. } | Statement::ForLoop { body, .. } => {
                    self.scan_strings_recursive(body, string_constants, counter);
                }
                _ => {}
            }
        }
    }

    /// Evaluates an expression or variable into an LLVM i32 register
    fn eval_val(
        &self,
        expr: &str,
        ir: &mut String,
        sym_table: &HashMap<String, String>,
        temp_counter: &mut usize,
    ) -> String {
        let trimmed = expr.trim();

        // Integer constant
        if let Ok(num) = trimmed.parse::<i32>() {
            return num.to_string();
        }

        // Binary arithmetic: a + b, x - 1, etc.
        if let Some(caps) = self.bin_re.captures(trimmed) {
            let left = self.eval_val(&caps[1], ir, sym_table, temp_counter);
            let op = &caps[2];
            let right = self.eval_val(&caps[3], ir, sym_table, temp_counter);

            let res_var = format!("%t_{}", *temp_counter);
            *temp_counter += 1;

            let instr = match op {
                "+" => "add",
                "-" => "sub",
                "*" => "mul",
                "/" => "sdiv",
                "%" => "srem",
                _ => "add",
            };

            ir.push_str(&format!("  {} = {} i32 {}, {}\n", res_var, instr, left, right));
            return res_var;
        }

        // Variable in symbol table
        if let Some(alloca_ptr) = sym_table.get(trimmed) {
            let res_var = format!("%t_{}", *temp_counter);
            *temp_counter += 1;
            ir.push_str(&format!("  {} = load i32, i32* {}\n", res_var, alloca_ptr));
            return res_var;
        }

        "0".to_string()
    }

    /// Evaluates a comparison expression into an LLVM i1 register
    fn eval_cond(
        &self,
        cond: &str,
        ir: &mut String,
        sym_table: &HashMap<String, String>,
        temp_counter: &mut usize,
    ) -> String {
        let trimmed = cond.trim();

        if let Some(caps) = self.cmp_re.captures(trimmed) {
            let left = self.eval_val(&caps[1], ir, sym_table, temp_counter);
            let op = &caps[2];
            let right = self.eval_val(&caps[3], ir, sym_table, temp_counter);

            let cond_var = format!("%cond_{}", *temp_counter);
            *temp_counter += 1;

            let predicate = match op {
                "==" => "eq",
                "!=" => "ne",
                "<" => "slt",
                "<=" => "sle",
                ">" => "sgt",
                ">=" => "sge",
                _ => "ne",
            };

            ir.push_str(&format!(
                "  {} = icmp {} i32 {}, {}\n",
                cond_var, predicate, left, right
            ));
            return cond_var;
        }

        // Truthiness check on value != 0
        let val = self.eval_val(trimmed, ir, sym_table, temp_counter);
        let cond_var = format!("%cond_{}", *temp_counter);
        *temp_counter += 1;
        ir.push_str(&format!("  {} = icmp ne i32 {}, 0\n", cond_var, val));
        cond_var
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_statements(
        &self,
        stmts: &[Statement],
        ir: &mut String,
        sym_table: &mut HashMap<String, String>,
        string_constants: &[(String, String, usize)],
        cur_str_idx: &mut usize,
        lbl_counter: &mut usize,
        temp_counter: &mut usize,
        fn_name: &str,
        temp: f32,
    ) -> anyhow::Result<bool> {
        let mut custom_return = false;

        for stmt in stmts {
            match stmt {
                // 1. Variable Assignment: `x = 5`, `count = x + 1`
                Statement::Assign { target, expr } => {
                    let alloca_ptr = if let Some(p) = sym_table.get(target) {
                        p.clone()
                    } else {
                        let ptr = format!("%var_{}", target);
                        ir.push_str(&format!("  {} = alloca i32\n", ptr));
                        sym_table.insert(target.clone(), ptr.clone());
                        ptr
                    };

                    let val = self.eval_val(expr, ir, sym_table, temp_counter);
                    ir.push_str(&format!("  store i32 {}, i32* {}\n", val, alloca_ptr));
                }

                // 2. If Statement with optional Else
                Statement::If { condition, then_branch, else_branch } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let then_lbl = format!("if_then_{}", id);
                    let else_lbl = format!("if_else_{}", id);
                    let merge_lbl = format!("if_merge_{}", id);

                    let cond_var = self.eval_cond(condition, ir, sym_table, temp_counter);
                    
                    if else_branch.is_some() {
                        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cond_var, then_lbl, else_lbl));
                    } else {
                        ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cond_var, then_lbl, merge_lbl));
                    }

                    // Then branch
                    ir.push_str(&format!("{}:\n", then_lbl));
                    let then_ret = self.lower_statements(
                        then_branch, ir, sym_table, string_constants, cur_str_idx, lbl_counter, temp_counter, fn_name, temp
                    )?;
                    if !then_ret {
                        ir.push_str(&format!("  br label %{}\n\n", merge_lbl));
                    }

                    // Else branch
                    if let Some(eb) = else_branch {
                        ir.push_str(&format!("{}:\n", else_lbl));
                        let else_ret = self.lower_statements(
                            eb, ir, sym_table, string_constants, cur_str_idx, lbl_counter, temp_counter, fn_name, temp
                        )?;
                        if !else_ret {
                            ir.push_str(&format!("  br label %{}\n\n", merge_lbl));
                        }
                    }

                    ir.push_str(&format!("{}:\n", merge_lbl));
                }

                // 3. While Loop
                Statement::WhileLoop { condition, body } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let cond_lbl = format!("while_cond_{}", id);
                    let body_lbl = format!("while_body_{}", id);
                    let exit_lbl = format!("while_exit_{}", id);

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    ir.push_str(&format!("{}:\n", cond_lbl));

                    let cond_var = self.eval_cond(condition, ir, sym_table, temp_counter);
                    ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cond_var, body_lbl, exit_lbl));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    let body_ret = self.lower_statements(
                        body, ir, sym_table, string_constants, cur_str_idx, lbl_counter, temp_counter, fn_name, temp
                    )?;
                    if !body_ret {
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    }

                    ir.push_str(&format!("{}:\n", exit_lbl));
                }

                // 4. For Loop
                Statement::ForLoop { var, count, body } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let cond_lbl = format!("for_cond_{}", id);
                    let body_lbl = format!("for_body_{}", id);
                    let exit_lbl = format!("for_exit_{}", id);

                    let idx_ptr = format!("%var_{}", var);
                    ir.push_str(&format!("  {} = alloca i32\n", idx_ptr));
                    ir.push_str(&format!("  store i32 0, i32* {}\n", idx_ptr));
                    sym_table.insert(var.clone(), idx_ptr.clone());

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    ir.push_str(&format!("{}:\n", cond_lbl));

                    let curr_val = format!("%curr_{}", id);
                    ir.push_str(&format!("  {} = load i32, i32* {}\n", curr_val, idx_ptr));

                    let count_limit = self.eval_val(count, ir, sym_table, temp_counter);
                    let cmp_var = format!("%cmp_{}", id);
                    ir.push_str(&format!(
                        "  {} = icmp slt i32 {}, {}\n",
                        cmp_var, curr_val, count_limit
                    ));
                    ir.push_str(&format!("  br i1 {}, label %{}, label %{}\n\n", cmp_var, body_lbl, exit_lbl));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    let body_ret = self.lower_statements(
                        body, ir, sym_table, string_constants, cur_str_idx, lbl_counter, temp_counter, fn_name, temp
                    )?;

                    let next_val = format!("%next_{}", id);
                    ir.push_str(&format!("  {} = add i32 {}, 1\n", next_val, curr_val));
                    ir.push_str(&format!("  store i32 {}, i32* {}\n", next_val, idx_ptr));

                    if !body_ret {
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    }

                    ir.push_str(&format!("{}:\n", exit_lbl));
                }

                // 5. Print Statement
                Statement::Print { expr } => {
                    let trimmed = expr.trim();
                    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    {
                        // Print literal string via @__omni_print_str
                        let (ref const_name, _, len) = string_constants[*cur_str_idx];
                        *cur_str_idx += 1;
                        let str_ptr = format!("%str_ptr_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                            str_ptr, len, len, const_name
                        ));
                        ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", str_ptr));
                    } else {
                        // Print integer / expression value via @__omni_print_int
                        let val = self.eval_val(trimmed, ir, sym_table, temp_counter);
                        let val_64 = format!("%ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));
                        ir.push_str(&format!("  call void @__omni_print_int(i64 {})\n", val_64));
                    }
                }

                // 6. Return Statement
                Statement::Return { expr } => {
                    let ret_val = if let Some(e) = expr {
                        self.eval_val(e, ir, sym_table, temp_counter)
                    } else {
                        "0".to_string()
                    };
                    ir.push_str(&format!("  ret i32 {}\n", ret_val));
                    custom_return = true;
                    break;
                }

                // 7. Intent Statement
                Statement::Intent { instruction } => {
                    let synthesized = self.engine.lower_intent(fn_name, instruction, temp)?;
                    ir.push_str(&synthesized);
                }

                // 8. Fallback Concrete / Polyglot Statement -> routed to AI Lowering
                Statement::Concrete { code } => {
                    let synthesized = self.engine.lower_intent(fn_name, code, temp)?;
                    ir.push_str(&synthesized);
                }
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
                "{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
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
        let mut lbl_counter = 0;
        let mut temp_counter = 0;

        for func in &module.functions {
            if func.name == "main" {
                has_user_main = true;
            }

            ir.push_str(&format!(
                "define i32 @__omni_fn_{}(i8* %data, i64 %len) {{\nentry:\n",
                func.name
            ));

            let mut sym_table = HashMap::new();

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
                &mut sym_table,
                &string_constants,
                &mut cur_str_idx,
                &mut lbl_counter,
                &mut temp_counter,
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
