use omni_engine::{extract_declarations_and_body, InferenceEngine, SsaContextEnvelope, SsaVariable};
use omni_mem::UnifiedMemOp;
use omni_parser::{OmniModule, Statement};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// Returns the canonical target triple of the current host machine (Linux, macOS, Windows).
pub fn host_target_triple() -> &'static str {
    omni_runtime::host_target_triple()
}

#[derive(Debug, Clone)]
pub enum CleanupOp {
    Free(String),
    DestroySlice(String),
    DestroyMap(String),
    DestroyChan(String),
}

#[derive(Debug, Clone)]
pub struct SymbolBinding {
    pub name: String,
    pub llvm_ptr: String,
    pub type_hint: String,
    pub is_scoped: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScopeTree {
    scopes: Vec<HashMap<String, SymbolBinding>>,
    pub class_fields: HashMap<String, HashMap<String, usize>>,
    pub enum_variants: HashMap<String, (String, usize)>,
    pub known_functions: HashSet<String>,
}

impl ScopeTree {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            class_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            known_functions: HashSet::new(),
        }
    }

    pub fn resolve_enum_variant(&self, expr: &str) -> Option<(String, (usize, Option<String>))> {
        let trimmed = expr.trim();
        if let Some((e, tag)) = self.enum_variants.get(trimmed) {
            return Some((e.clone(), (*tag, None)));
        }
        if let Some(open) = trimmed.find('(') {
            if trimmed.ends_with(')') {
                let var_name = trimmed[..open].trim();
                let payload = trimmed[open + 1..trimmed.len() - 1].trim().to_string();
                if let Some((e, tag)) = self.enum_variants.get(var_name) {
                    return Some((e.clone(), (*tag, Some(payload))));
                }
            }
        }
        None
    }

    pub fn resolve_pattern_tag(&self, enum_name: &str, pattern: &str) -> Option<usize> {
        let pat_clean = pattern.split('(').next().unwrap_or(pattern).trim();
        if let Some((_, tag)) = self.enum_variants.get(pat_clean) {
            return Some(*tag);
        }
        let full_name = format!("{}::{}", enum_name, pat_clean);
        if let Some((_, tag)) = self.enum_variants.get(&full_name) {
            return Some(*tag);
        }
        None
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn insert(&mut self, name: String, binding: SymbolBinding) {
        if let Some(curr) = self.scopes.last_mut() {
            curr.insert(name, binding);
        }
    }

    pub fn resolve(&self, name: &str) -> Option<&SymbolBinding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return Some(b);
            }
        }
        None
    }

    pub fn active_bindings(&self) -> Vec<SymbolBinding> {
        let mut map = HashMap::new();
        for scope in &self.scopes {
            for (k, v) in scope {
                map.insert(k.clone(), v.clone());
            }
        }
        map.into_values().collect()
    }
}

pub struct CodeGenerator<'a> {
    engine: &'a dyn InferenceEngine,
    cmp_re: Regex,
    idx_re: Regex,
    len_re: Regex,
    pop_re: Regex,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self {
            engine,
            cmp_re: Regex::new(r#"^\s*(.*?)\s*(==|!=|<=|>=|<|>)\s*(.*?)\s*$"#).unwrap(),
            idx_re: Regex::new(r#"^([a-zA-Z_0-9]+)\[(.+?)\]$"#).unwrap(),
            len_re: Regex::new(r#"^len\s*\(\s*([a-zA-Z_0-9]+)\s*\)$"#).unwrap(),
            pop_re: Regex::new(r#"^([a-zA-Z_0-9]+)\.pop\s*\(\s*\)$"#).unwrap(),
        }
    }

    fn intern_string(
        string_constants: &mut Vec<(String, String, usize)>,
        counter: &mut usize,
        text: &str,
    ) {
        if string_constants.iter().any(|(_, s, _)| s == text) {
            return;
        }
        let const_name = format!("@.str.omni_{}", *counter);
        *counter += 1;
        let len_with_null = text.len() + 1; // \00
        string_constants.push((const_name, text.to_string(), len_with_null));
    }

    fn find_string_constant<'b>(
        string_constants: &'b [(String, String, usize)],
        text: &str,
    ) -> Option<(&'b str, usize)> {
        string_constants
            .iter()
            .find(|(_, s, _)| s == text)
            .map(|(c, _, len)| (c.as_str(), *len))
    }

    fn extract_pattern_binding(pattern: &str) -> Option<String> {
        if let Some(open) = pattern.find('(') {
            if pattern.ends_with(')') {
                let inner = pattern[open + 1..pattern.len() - 1].trim();
                if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Some(inner.to_string());
                }
            }
        }
        None
    }

    fn emit_cleanups(
        ir: &mut String,
        cleanups: &[CleanupOp],
        temp_counter: &mut usize,
        context: &str,
    ) {
        for op in cleanups.iter().rev() {
            match op {
                CleanupOp::Free(ptr) => {
                    ir.push_str(&format!("  ; [Deferred Free at {}: %{}]\n", context, ptr));
                    ir.push_str(&format!("  call void @free(i8* %{})\n", ptr));
                }
                CleanupOp::DestroySlice(alloca_ptr) => {
                    let loaded = format!("%clean_slice_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Slice Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_slice_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyMap(alloca_ptr) => {
                    let loaded = format!("%clean_map_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Map Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_map_destroy(i8* {})\n", loaded));
                }
                CleanupOp::DestroyChan(alloca_ptr) => {
                    let loaded = format!("%clean_chan_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  ; [RAII Chan Cleanup at {}]\n", context));
                    ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, alloca_ptr));
                    ir.push_str(&format!("  call void @__omni_chan_destroy(i8* {})\n", loaded));
                }
            }
        }
    }

    fn split_binary_expr(expr: &str) -> Option<(String, String, String)> {
        let trimmed = expr.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let mut depth = 0;
            let mut wraps_all = true;
            let chars: Vec<char> = trimmed.chars().collect();
            for (idx, &c) in chars.iter().enumerate() {
                if c == '(' { depth += 1; }
                else if c == ')' { depth -= 1; }
                if depth == 0 && idx < chars.len() - 1 {
                    wraps_all = false;
                    break;
                }
            }
            if wraps_all {
                return Self::split_binary_expr(&trimmed[1..trimmed.len() - 1]);
            }
        }

        let mut depth;
        let mut in_quote;
        let chars: Vec<char> = trimmed.chars().collect();

        // Pass 0: Look for bitwise operators (|, ^, &) from right to left
        depth = 0;
        in_quote = false;
        for i in (1..chars.len()).rev() {
            let c = chars[i];
            if c == '"' || c == '\'' { in_quote = !in_quote; }
            if in_quote { continue; }
            if c == ')' { depth += 1; }
            else if c == '(' { depth -= 1; }
            if depth == 0 && (c == '|' || c == '^' || c == '&') {
                let prev = chars[i - 1];
                if prev != '|' && prev != '&' && (i + 1 == chars.len() || chars[i + 1] != c) {
                    let left: String = chars[..i].iter().collect();
                    let right: String = chars[i + 1..].iter().collect();
                    return Some((left.trim().to_string(), c.to_string(), right.trim().to_string()));
                }
            }
        }

        // Pass 1: Look for lowest precedence operators (+, -) from right to left
        depth = 0;
        in_quote = false;
        for i in (1..chars.len()).rev() {
            let c = chars[i];
            if c == '"' || c == '\'' { in_quote = !in_quote; }
            if in_quote { continue; }
            if c == ')' { depth += 1; }
            else if c == '(' { depth -= 1; }
            if depth == 0 && (c == '+' || c == '-') {
                let prev = chars[i - 1];
                if prev != '+' && prev != '-' && prev != '*' && prev != '/' && prev != '%' && prev != '(' {
                    let left: String = chars[..i].iter().collect();
                    let right: String = chars[i + 1..].iter().collect();
                    return Some((left.trim().to_string(), c.to_string(), right.trim().to_string()));
                }
            }
        }

        // Pass 2: Look for higher precedence operators (*, /, %) from right to left
        depth = 0;
        in_quote = false;
        for i in (1..chars.len()).rev() {
            let c = chars[i];
            if c == '"' || c == '\'' { in_quote = !in_quote; }
            if in_quote { continue; }
            if c == ')' { depth += 1; }
            else if c == '(' { depth -= 1; }
            if depth == 0 && (c == '*' || c == '/' || c == '%') {
                let left: String = chars[..i].iter().collect();
                let right: String = chars[i + 1..].iter().collect();
                return Some((left.trim().to_string(), c.to_string(), right.trim().to_string()));
            }
        }

        None
    }

    fn scan_strings_recursive(
        &self,
        stmts: &[Statement],
        string_constants: &mut Vec<(String, String, usize)>,
        counter: &mut usize,
    ) {
        let quote_re = Regex::new(r#"["']([^"']+)["']"#).unwrap();

        for stmt in stmts {
            match stmt {
                Statement::Print { expr } => {
                    let trimmed = expr.trim();
                    if trimmed.contains("{}") && trimmed.contains(',') {
                        if let Some(comma_pos) = trimmed.rfind(',') {
                            let fmt = trimmed[..comma_pos]
                                .trim()
                                .trim_start_matches('f')
                                .trim_matches('"')
                                .trim_matches('\'');
                            Self::intern_string(string_constants, counter, fmt);
                        }
                    } else if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    {
                        let text = &trimmed[1..trimmed.len() - 1];
                        Self::intern_string(string_constants, counter, text);
                    }
                }
                Statement::MapInit { entries, .. } => {
                    for (k, _) in entries {
                        Self::intern_string(string_constants, counter, k);
                    }
                }
                Statement::IndexSet { index_expr, .. } => {
                    for cap in quote_re.captures_iter(index_expr) {
                        Self::intern_string(string_constants, counter, &cap[1]);
                    }
                }
                Statement::Assign { expr, .. } => {
                    for cap in quote_re.captures_iter(expr) {
                        Self::intern_string(string_constants, counter, &cap[1]);
                    }
                }
                Statement::Match { arms, .. } => {
                    for arm in arms {
                        self.scan_strings_recursive(&arm.body, string_constants, counter);
                    }
                }
                Statement::If { then_branch, else_branch, .. } => {
                    self.scan_strings_recursive(then_branch, string_constants, counter);
                    if let Some(eb) = else_branch {
                        self.scan_strings_recursive(eb, string_constants, counter);
                    }
                }
                Statement::WhileLoop { body, .. } | Statement::ForLoop { body, .. } | Statement::WithContext { body, .. } => {
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
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let trimmed = expr.trim();

        // 1. Integer constant
        if let Ok(num) = trimmed.parse::<i32>() {
            return num.to_string();
        }

        // Struct field access: rec.fld
        if trimmed.contains('.') && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed.split('.').collect();
            if parts.len() == 2 {
                let rec = parts[0].trim();
                let fld = parts[1].trim();
                if let Some(b) = scope_tree.resolve(rec) {
                    if b.type_hint.starts_with("%struct.") {
                        let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                        if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fld)) {
                            let inst_ld = format!("%inst_fld_eval_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                            let gep = format!("%eval_gep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                            let val64 = format!("%val64_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i64, i64* {}\n", val64, gep));
                            let val32 = format!("%val32_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = trunc i64 {} to i32\n", val32, val64));
                            return val32;
                        }
                    }
                }
            }
        }

        // Method invocation: `obj.method(args...)` or direct function call `func(args...)`
        if let Some(lp) = trimmed.find('(') {
            if trimmed.ends_with(')') {
                let call_target = trimmed[..lp].trim();
                let is_valid_call_target = !call_target.is_empty()
                    && call_target.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');

                if is_valid_call_target {
                    let args_str = &trimmed[lp + 1..trimmed.len() - 1];
                    let raw_args: Vec<String> = if args_str.is_empty() {
                        Vec::new()
                    } else {
                        omni_parser::OmniParser::split_respecting_quotes(args_str, ',')
                    };

                    if call_target.contains('.') {
                    let parts: Vec<&str> = call_target.split('.').collect();
                    if parts.len() == 2 {
                        let rec = parts[0].trim();
                        let method = parts[1].trim();
                        if let Some(b) = scope_tree.resolve(rec) {
                            if b.type_hint.starts_with("%struct.") {
                                let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');

                                let inst_ld = format!("%inst_call_ld_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                                let raw_inst = format!("%raw_call_inst_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_ld));

                                let mut arg_regs = Vec::new();
                                for a in &raw_args {
                                    let r = self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                    let ext = format!("%call_ext_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, r));
                                    arg_regs.push(ext);
                                }
                                while arg_regs.len() < 3 {
                                    arg_regs.push("0".to_string());
                                }

                                let ret_reg = format!("%method_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i32 @__omni_method_{}_{}(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                    ret_reg, cls_name, method, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2]
                                ));
                                return ret_reg;
                            }
                        } else {
                            // Foreign module call: `fast_math.compute_luhn(card_num)` -> `@__omni_fn_compute_luhn`
                            let mut arg_regs = Vec::new();
                            for a in &raw_args {
                                let r = self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                let ext = format!("%foreign_ext_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, r));
                                arg_regs.push(ext);
                            }
                            while arg_regs.len() < 4 {
                                arg_regs.push("0".to_string());
                            }

                            let ret_reg = format!("%foreign_ret_{}", *temp_counter);
                            *temp_counter += 1;
                            if scope_tree.known_functions.contains(method) {
                                ir.push_str(&format!(
                                    "  {} = call i32 @__omni_fn_{}(i64 {}, i64 {}, i64 {}, i64 {})\n",
                                    ret_reg, method, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3]
                                ));
                            } else {
                                // Direct foreign C ABI symbol invocation (e.g. `zlib.compressBound` -> `@compressBound`)
                                ir.push_str(&format!(
                                    "  {} = call i32 @{}(i64 {})\n",
                                    ret_reg, method, arg_regs[0]
                                ));
                            }
                            return ret_reg;
                        }
                    }
                } else if !call_target.is_empty()
                    && call_target != "len"
                    && call_target != "make"
                    && call_target != "range"
                    && call_target != "print"
                    && call_target != "println!"
                    && !scope_tree.class_fields.contains_key(call_target)
                {
                    // Direct function invocation: `rotate_hash(seed, 4)` -> `@__omni_fn_rotate_hash`
                    let mut arg_regs = Vec::new();
                    for a in &raw_args {
                        let r = self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter);
                        let ext = format!("%fn_arg_ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, r));
                        arg_regs.push(ext);
                    }
                    while arg_regs.len() < 4 {
                        arg_regs.push("0".to_string());
                    }

                    let ret_reg = format!("%fn_ret_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i32 @__omni_fn_{}(i64 {}, i64 {}, i64 {}, i64 {})\n",
                        ret_reg, call_target, arg_regs[0], arg_regs[1], arg_regs[2], arg_regs[3]
                    ));
                    return ret_reg;
                }
            }
        }
    }

        // 2. Builtin len(coll)
        if let Some(caps) = self.len_re.captures(trimmed) {
            let var_name = caps[1].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%col_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                if binding.type_hint.contains("omni_chan") {
                    ir.push_str(&format!("  {} = call i64 @__omni_chan_len(i8* {})\n", len_res, loaded));
                } else if binding.type_hint.contains("omni_map") {
                    ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_res, loaded));
                } else {
                    ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, loaded));
                }

                let len_32 = format!("%len_32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = trunc i64 {} to i32\n", len_32, len_res));
                return len_32;
            }
        }

        // 3. Slice pop: items.pop()
        if let Some(caps) = self.pop_re.captures(trimmed) {
            let var_name = caps[1].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%slice_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                let pop_res = format!("%pop_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_slice_pop(i8* {})\n", pop_res, loaded));

                let pop_32 = format!("%pop_32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = trunc i64 {} to i32\n", pop_32, pop_res));
                return pop_32;
            }
        }

        // 4. Index Lookup: items[i] or m["key"]
        if let Some(caps) = self.idx_re.captures(trimmed) {
            let var_name = caps[1].trim();
            let idx_expr = caps[2].trim();
            if let Some(binding) = scope_tree.resolve(var_name) {
                let loaded = format!("%col_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));

                if binding.type_hint.contains("omni_map") {
                    let val_res = format!("%map_get_{}", *temp_counter);
                    *temp_counter += 1;

                    if (idx_expr.starts_with('"') && idx_expr.ends_with('"'))
                        || (idx_expr.starts_with('\'') && idx_expr.ends_with('\''))
                    {
                        let text = &idx_expr[1..idx_expr.len() - 1];
                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_map_get(i8* {}, i8* {})\n",
                                val_res, loaded, str_ptr
                            ));
                        } else {
                            ir.push_str(&format!(
                                "  {} = call i64 @__omni_map_get(i8* {}, i8* null)\n",
                                val_res, loaded
                            ));
                        }
                    } else {
                        let idx_val = self.eval_val(idx_expr, ir, scope_tree, string_constants, temp_counter);
                        let idx_64 = format!("%idx_64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", idx_64, idx_val));
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_map_get_int(i8* {}, i64 {})\n",
                            val_res, loaded, idx_64
                        ));
                    }

                    let val_32 = format!("%val_32_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", val_32, val_res));
                    return val_32;
                } else {
                    // Default to Slice
                    let idx_val = self.eval_val(idx_expr, ir, scope_tree, string_constants, temp_counter);
                    let idx_64 = format!("%idx_64_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = sext i32 {} to i64\n", idx_64, idx_val));

                    let val_res = format!("%slice_get_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i64 @__omni_slice_get(i8* {}, i64 {})\n",
                        val_res, loaded, idx_64
                    ));

                    let val_32 = format!("%val_32_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", val_32, val_res));
                    return val_32;
                }
            }
        }

        // 5. Binary arithmetic with operator precedence (+, - first, then *, /, %)
        if let Some((left, op, right)) = Self::split_binary_expr(trimmed) {
            let left_val = self.eval_val(&left, ir, scope_tree, string_constants, temp_counter);
            let right_val = self.eval_val(&right, ir, scope_tree, string_constants, temp_counter);

            let res_var = format!("%t_{}", *temp_counter);
            *temp_counter += 1;

            let instr = match op.as_str() {
                "+" => "add",
                "-" => "sub",
                "*" => "mul",
                "/" => "sdiv",
                "%" => "srem",
                "^" => "xor",
                "&" => "and",
                "|" => "or",
                _ => "add",
            };

            ir.push_str(&format!("  {} = {} i32 {}, {}\n", res_var, instr, left_val, right_val));
            return res_var;
        }

        // 6. Variable in symbol table
        if let Some(binding) = scope_tree.resolve(trimmed) {
            if binding.type_hint.contains("omni_slice") {
                let loaded = format!("%slice_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_slice_len(i8* {})\n", len_res, loaded));
                let len_32 = format!("%len_32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = trunc i64 {} to i32\n", len_32, len_res));
                return len_32;
            } else if binding.type_hint.contains("omni_map") {
                let loaded = format!("%map_ptr_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                let len_res = format!("%len_res_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = call i64 @__omni_map_len(i8* {})\n", len_res, loaded));
                let len_32 = format!("%len_32_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = trunc i64 {} to i32\n", len_32, len_res));
                return len_32;
            } else {
                let res_var = format!("%t_{}", *temp_counter);
                *temp_counter += 1;
                ir.push_str(&format!("  {} = load i32, i32* {}\n", res_var, binding.llvm_ptr));
                return res_var;
            }
        }

        "0".to_string()
    }

    /// Evaluates a comparison expression into an LLVM i1 register
    fn eval_cond(
        &self,
        cond: &str,
        ir: &mut String,
        scope_tree: &ScopeTree,
        string_constants: &[(String, String, usize)],
        temp_counter: &mut usize,
    ) -> String {
        let trimmed = cond.trim();

        if let Some(caps) = self.cmp_re.captures(trimmed) {
            let left = self.eval_val(&caps[1], ir, scope_tree, string_constants, temp_counter);
            let op = &caps[2];
            let right = self.eval_val(&caps[3], ir, scope_tree, string_constants, temp_counter);

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
        let val = self.eval_val(trimmed, ir, scope_tree, string_constants, temp_counter);
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
        scope_tree: &mut ScopeTree,
        deferred_frees: &mut Vec<CleanupOp>,
        string_constants: &[(String, String, usize)],
        cur_str_idx: &mut usize,
        lbl_counter: &mut usize,
        temp_counter: &mut usize,
        fn_name: &str,
        struct_defs: &[String],
        temp: f32,
        ecosystem_context: Option<&str>,
        import_chain: &[String],
        dynamic_declarations: &mut HashSet<String>,
    ) -> anyhow::Result<bool> {
        let mut custom_return = false;

        for stmt in stmts {
            match stmt {
                // 1. Variable Assignment or Scoped Declaration: `x = 5`, `count := 0`
                Statement::Assign {
                    target,
                    expr,
                    idiom: _,
                    is_scoped_decl,
                } => {
                    let trimmed_expr = expr.trim();

                    // Dotted field assignment: `self.x = expr` or `obj.step = expr`
                    if target.contains('.') {
                        let parts: Vec<&str> = target.split('.').collect();
                        if parts.len() == 2 {
                            let rec = parts[0].trim();
                            let fld = parts[1].trim();
                            if let Some(b) = scope_tree.resolve(rec).cloned() {
                                if b.type_hint.starts_with("%struct.") {
                                    let cls_name = b.type_hint.trim_start_matches("%struct.").trim_end_matches('*');
                                    if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fld)) {
                                        let val_reg = self.eval_val(trimmed_expr, ir, scope_tree, string_constants, temp_counter);
                                        let ext = format!("%ext_fld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, val_reg));
                                        let inst_ld = format!("%inst_fld_ld_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = load %struct.{}*, %struct.{}** {}\n", inst_ld, cls_name, cls_name, b.llvm_ptr));
                                        let gep = format!("%fld_gep_{}", *temp_counter);
                                        *temp_counter += 1;
                                        ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_ld, f_idx));
                                        ir.push_str(&format!("  store i64 {}, i64* {}\n", ext, gep));
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Class / Struct Constructor Call: `c = Counter(10)`
                    if let Some(open) = trimmed_expr.find('(') {
                        if trimmed_expr.ends_with(')') {
                            let cls_cand = trimmed_expr[..open].trim();
                            if scope_tree.class_fields.contains_key(cls_cand) {
                                let cls_name = cls_cand;
                                let inst_alloc = format!("%inst_alloc_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}\n", inst_alloc, cls_name));

                                let raw_inst = format!("%raw_init_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_alloc));

                                let inner = &trimmed_expr[open + 1..trimmed_expr.len() - 1];
                                let raw_args: Vec<String> = if inner.is_empty() {
                                    Vec::new()
                                } else {
                                    omni_parser::OmniParser::split_respecting_quotes(inner, ',')
                                };

                                let mut arg_regs = Vec::new();
                                for a in &raw_args {
                                    let r = self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                    let ext = format!("%init_ext_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, r));
                                    arg_regs.push(ext);
                                }
                                while arg_regs.len() < 3 {
                                    arg_regs.push("0".to_string());
                                }

                                let init_ret = format!("%init_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i32 @__omni_method_{}___init__(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                    init_ret, cls_name, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2]
                                ));

                                let target_ptr = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                scope_tree.insert(target.clone(), SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: target_ptr,
                                    type_hint: format!("%struct.{}*", cls_name),
                                    is_scoped: true,
                                });
                                continue;
                            }
                        }
                    }

                    // Rust Struct Literal: `p = Point { x: 10, y: 20 }`
                    if let Some(open) = trimmed_expr.find('{') {
                        if trimmed_expr.ends_with('}') {
                            let struct_cand = trimmed_expr[..open].trim();
                            if scope_tree.class_fields.contains_key(struct_cand) {
                                let cls_name = struct_cand;
                                let inst_alloc = format!("%inst_alloc_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}\n", inst_alloc, cls_name));

                                let inner = &trimmed_expr[open + 1..trimmed_expr.len() - 1];
                                for field_part in inner.split(',') {
                                    let fp_trim = field_part.trim();
                                    if let Some((f_name, f_val)) = fp_trim.split_once(':') {
                                        let fn_clean = f_name.trim();
                                        if let Some(&f_idx) = scope_tree.class_fields.get(cls_name).and_then(|m| m.get(fn_clean)) {
                                            let val_reg = self.eval_val(f_val.trim(), ir, scope_tree, string_constants, temp_counter);
                                            let ext = format!("%ext_init_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, val_reg));
                                            let gep = format!("%init_gep_{}", *temp_counter);
                                            *temp_counter += 1;
                                            ir.push_str(&format!("  {} = getelementptr inbounds %struct.{}, %struct.{}* {}, i32 0, i32 {}\n", gep, cls_name, cls_name, inst_alloc, f_idx));
                                            ir.push_str(&format!("  store i64 {}, i64* {}\n", ext, gep));
                                        }
                                    }
                                }

                                let target_ptr = format!("%var_{}_{}", target, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                scope_tree.insert(target.clone(), SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: target_ptr,
                                    type_hint: format!("%struct.{}*", cls_name),
                                    is_scoped: true,
                                });
                                continue;
                            }
                        }
                    }

                    // Rust Tagged Enum Variant Assignment: `s = Status::Pending` or `res = Result::Ok(100)`
                    if let Some((e_name, var_info)) = scope_tree.resolve_enum_variant(trimmed_expr) {
                        let (tag, payload_val) = var_info;
                        let inst_alloc = format!("%enum_alloc_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca %enum.{}\n", inst_alloc, e_name));

                        let tag_gep = format!("%tag_gep_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 0\n", tag_gep, e_name, e_name, inst_alloc));
                        ir.push_str(&format!("  store i32 {}, i32* {}\n", tag, tag_gep));

                        let payload_gep = format!("%payload_gep_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 1\n", payload_gep, e_name, e_name, inst_alloc));

                        if let Some(payload_expr) = payload_val {
                            let val_reg = self.eval_val(&payload_expr, ir, scope_tree, string_constants, temp_counter);
                            let ext = format!("%payload_ext_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, val_reg));
                            ir.push_str(&format!("  store i64 {}, i64* {}\n", ext, payload_gep));
                        } else {
                            ir.push_str(&format!("  store i64 0, i64* {}\n", payload_gep));
                        }

                        let target_ptr = format!("%var_{}_{}", target, *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = alloca %enum.{}*\n", target_ptr, e_name));
                        ir.push_str(&format!("  store %enum.{}* {}, %enum.{}** {}\n", e_name, inst_alloc, e_name, target_ptr));
                        scope_tree.insert(target.clone(), SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: target_ptr,
                            type_hint: format!("%enum.{}*", e_name),
                            is_scoped: true,
                        });
                        continue;
                    }

                    let existing_binding = scope_tree.resolve(trimmed_expr).cloned();

                    if let Some(src) = existing_binding.filter(|b| b.type_hint.starts_with("i8*")) {
                        // Preserving collection/channel reference semantics (no truncating to scalar int!)
                        let ptr = if *is_scoped_decl || scope_tree.resolve(target).is_none() {
                            let p = format!("%col_ref_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i8*\n", p));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: p.clone(),
                                    type_hint: src.type_hint.clone(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            p
                        } else {
                            scope_tree.resolve(target).unwrap().llvm_ptr.clone()
                        };

                        let loaded = format!("%ref_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, src.llvm_ptr));
                        ir.push_str(&format!("  store i8* {}, i8** {}\n", loaded, ptr));
                    } else {
                        let alloca_ptr = if *is_scoped_decl || scope_tree.resolve(target).is_none() {
                            let ptr = format!("%var_{}_{}", target, *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = alloca i32\n", ptr));
                            scope_tree.insert(
                                target.clone(),
                                SymbolBinding {
                                    name: target.clone(),
                                    llvm_ptr: ptr.clone(),
                                    type_hint: "i32".to_string(),
                                    is_scoped: *is_scoped_decl,
                                },
                            );
                            ptr
                        } else {
                            scope_tree.resolve(target).unwrap().llvm_ptr.clone()
                        };

                        let val = self.eval_val(expr, ir, scope_tree, string_constants, temp_counter);
                        ir.push_str(&format!("  store i32 {}, i32* {}\n", val, alloca_ptr));
                    }
                }

                // Pattern Matching: `match expr { ... }`
                Statement::Match { target, arms } => {
                    let match_id = *lbl_counter;
                    *lbl_counter += 1;
                    let exit_lbl = format!("match_{}_exit", match_id);
                    let default_lbl = format!("match_{}_default", match_id);

                    if let Some(b) = scope_tree.resolve(target).cloned() {
                        if b.type_hint.starts_with("%enum.") {
                            let e_name = b.type_hint.trim_start_matches("%enum.").trim_end_matches('*').to_string();
                            let inst_ld = format!("%match_inst_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load %enum.{}*, %enum.{}** {}\n", inst_ld, e_name, e_name, b.llvm_ptr));
                            let tag_gep = format!("%match_tag_gep_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 0\n", tag_gep, e_name, e_name, inst_ld));
                            let tag_val = format!("%match_tag_val_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = load i32, i32* {}\n", tag_val, tag_gep));

                            let mut switch_cases = Vec::new();
                            for (arm_idx, arm) in arms.iter().enumerate() {
                                let arm_lbl = format!("match_{}_arm_{}", match_id, arm_idx);
                                let tag_num = scope_tree.resolve_pattern_tag(&e_name, &arm.pattern).unwrap_or(arm_idx);
                                switch_cases.push(format!("i32 {}, label %{}", tag_num, arm_lbl));
                            }
                            ir.push_str(&format!("  switch i32 {}, label %{} [\n", tag_val, default_lbl));
                            for c in &switch_cases {
                                ir.push_str(&format!("    {}\n", c));
                            }
                            ir.push_str("  ]\n");

                            for (arm_idx, arm) in arms.iter().enumerate() {
                                let arm_lbl = format!("match_{}_arm_{}", match_id, arm_idx);
                                ir.push_str(&format!("\n{}:\n", arm_lbl));

                                if let Some(bind_var) = Self::extract_pattern_binding(&arm.pattern) {
                                    let p_gep = format!("%arm_{}_p_gep_{}", arm_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = getelementptr inbounds %enum.{}, %enum.{}* {}, i32 0, i32 1\n", p_gep, e_name, e_name, inst_ld));
                                    let p_val64 = format!("%arm_{}_pval64_{}", arm_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = load i64, i64* {}\n", p_val64, p_gep));
                                    let p_val32 = format!("%arm_{}_pval32_{}", arm_idx, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = trunc i64 {} to i32\n", p_val32, p_val64));
                                    let p_var_ptr = format!("%var_{}_{}", bind_var, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca i32\n", p_var_ptr));
                                    ir.push_str(&format!("  store i32 {}, i32* {}\n", p_val32, p_var_ptr));
                                    scope_tree.insert(bind_var.clone(), SymbolBinding {
                                        name: bind_var,
                                        llvm_ptr: p_var_ptr,
                                        type_hint: "i32".to_string(),
                                        is_scoped: true,
                                    });
                                }

                                self.lower_statements(
                                    &arm.body,
                                    ir,
                                    scope_tree,
                                    deferred_frees,
                                    string_constants,
                                    cur_str_idx,
                                    lbl_counter,
                                    temp_counter,
                                    fn_name,
                                    struct_defs,
                                    temp,
                                    ecosystem_context,
                                    import_chain,
                                    dynamic_declarations,
                                )?;
                                ir.push_str(&format!("  br label %{}\n", exit_lbl));
                            }

                            ir.push_str(&format!("\n{}:\n  br label %{}\n", default_lbl, exit_lbl));
                            ir.push_str(&format!("\n{}:\n", exit_lbl));
                            continue;
                        }
                    }
                }

                // 2. If Statement with optional Else
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let id = *lbl_counter;
                    *lbl_counter += 1;

                    let then_lbl = format!("if_then_{}", id);
                    let else_lbl = format!("if_else_{}", id);
                    let merge_lbl = format!("if_merge_{}", id);

                    let cond_var = self.eval_cond(condition, ir, scope_tree, string_constants, temp_counter);

                    if else_branch.is_some() {
                        ir.push_str(&format!(
                            "  br i1 {}, label %{}, label %{}\n\n",
                            cond_var, then_lbl, else_lbl
                        ));
                    } else {
                        ir.push_str(&format!(
                            "  br i1 {}, label %{}, label %{}\n\n",
                            cond_var, then_lbl, merge_lbl
                        ));
                    }

                    // Then branch
                    ir.push_str(&format!("{}:\n", then_lbl));
                    scope_tree.enter_scope();
                    let then_snapshot = deferred_frees.len();
                    let then_ret = self.lower_statements(
                        then_branch,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                    )?;
                    if !then_ret {
                        Self::emit_cleanups(ir, &deferred_frees[then_snapshot..], temp_counter, "Then Branch Exit");
                        deferred_frees.truncate(then_snapshot);
                    }
                    scope_tree.exit_scope();
                    if !then_ret {
                        ir.push_str(&format!("  br label %{}\n\n", merge_lbl));
                    }

                    // Else branch
                    if let Some(eb) = else_branch {
                        ir.push_str(&format!("{}:\n", else_lbl));
                        scope_tree.enter_scope();
                        let else_snapshot = deferred_frees.len();
                        let else_ret = self.lower_statements(
                            eb,
                            ir,
                            scope_tree,
                            deferred_frees,
                            string_constants,
                            cur_str_idx,
                            lbl_counter,
                            temp_counter,
                            fn_name,
                            struct_defs,
                            temp,
                            ecosystem_context,
                            import_chain,
                            dynamic_declarations,
                        )?;
                        if !else_ret {
                            Self::emit_cleanups(ir, &deferred_frees[else_snapshot..], temp_counter, "Else Branch Exit");
                            deferred_frees.truncate(else_snapshot);
                        }
                        scope_tree.exit_scope();
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

                    let cond_var = self.eval_cond(condition, ir, scope_tree, string_constants, temp_counter);
                    ir.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n\n",
                        cond_var, body_lbl, exit_lbl
                    ));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    scope_tree.enter_scope();
                    let while_snapshot = deferred_frees.len();
                    let body_ret = self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                    )?;
                    if !body_ret {
                        Self::emit_cleanups(ir, &deferred_frees[while_snapshot..], temp_counter, "While Iteration Exit");
                        deferred_frees.truncate(while_snapshot);
                    }
                    scope_tree.exit_scope();
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

                    let idx_ptr = format!("%var_{}_{}", var, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i32\n", idx_ptr));
                    ir.push_str(&format!("  store i32 0, i32* {}\n", idx_ptr));

                    scope_tree.enter_scope();
                    scope_tree.insert(
                        var.clone(),
                        SymbolBinding {
                            name: var.clone(),
                            llvm_ptr: idx_ptr.clone(),
                            type_hint: "i32".to_string(),
                            is_scoped: true,
                        },
                    );

                    ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    ir.push_str(&format!("{}:\n", cond_lbl));

                    let curr_val = format!("%curr_{}", id);
                    ir.push_str(&format!("  {} = load i32, i32* {}\n", curr_val, idx_ptr));

                    let count_limit = self.eval_val(count, ir, scope_tree, string_constants, temp_counter);
                    let cmp_var = format!("%cmp_{}", id);
                    ir.push_str(&format!(
                        "  {} = icmp slt i32 {}, {}\n",
                        cmp_var, curr_val, count_limit
                    ));
                    ir.push_str(&format!(
                        "  br i1 {}, label %{}, label %{}\n\n",
                        cmp_var, body_lbl, exit_lbl
                    ));

                    ir.push_str(&format!("{}:\n", body_lbl));
                    let for_snapshot = deferred_frees.len();
                    let body_ret = self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                    )?;
                    if !body_ret {
                        Self::emit_cleanups(ir, &deferred_frees[for_snapshot..], temp_counter, "For Iteration Exit");
                        deferred_frees.truncate(for_snapshot);
                    }

                    let load_for_inc = format!("%curr_inc_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = load i32, i32* {}\n", load_for_inc, idx_ptr));
                    let next_val = format!("%next_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = add i32 {}, 1\n", next_val, load_for_inc));
                    ir.push_str(&format!("  store i32 {}, i32* {}\n", next_val, idx_ptr));

                    scope_tree.exit_scope();

                    if !body_ret {
                        ir.push_str(&format!("  br label %{}\n\n", cond_lbl));
                    }

                    ir.push_str(&format!("{}:\n", exit_lbl));
                }

                // 5. Print Statement
                Statement::Print { expr } => {
                    let trimmed = expr.trim();
                    if trimmed.contains("{}") && trimmed.contains(',') {
                        if let Some(comma_pos) = trimmed.rfind(',') {
                            let fmt = trimmed[..comma_pos]
                                .trim()
                                .trim_start_matches('f')
                                .trim_matches('"')
                                .trim_matches('\'');
                            let arg_expr = trimmed[comma_pos + 1..].trim();
                            if let Some((const_name, len)) = Self::find_string_constant(string_constants, fmt) {
                                let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                    str_ptr, len, len, const_name
                                ));
                                ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", str_ptr));
                            }
                            let val = self.eval_val(arg_expr, ir, scope_tree, string_constants, temp_counter);
                            let val_64 = format!("%ext_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));
                            ir.push_str(&format!("  call void @__omni_print_int(i64 {})\n", val_64));
                        }
                    } else if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                    {
                        let text = &trimmed[1..trimmed.len() - 1];
                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!("  call void @__omni_print_str(i8* {})\n", str_ptr));
                        }
                    } else {
                        // Print integer / expression value via @__omni_print_int
                        let val = self.eval_val(trimmed, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));
                        ir.push_str(&format!("  call void @__omni_print_int(i64 {})\n", val_64));
                    }
                }

                // 6. Unified Memory Operations (Alloc / Free / Defer)
                Statement::MemOp { op, is_deferred } => match op {
                    UnifiedMemOp::Alloc {
                        target,
                        size_bytes,
                        ..
                    } => {
                        let sz: u64 = size_bytes.parse().unwrap_or(64);
                        ir.push_str(&format!("  ; [UMO Inline Alloc: {} bytes]\n", sz));
                        let ptr_name = format!("%{}", target);
                        ir.push_str(&format!("  {} = call i8* @malloc(i64 {})\n", ptr_name, sz));
                        scope_tree.insert(
                            target.clone(),
                            SymbolBinding {
                                name: target.clone(),
                                llvm_ptr: ptr_name,
                                type_hint: "i8*".to_string(),
                                is_scoped: false,
                            },
                        );
                    }
                    UnifiedMemOp::Free { target } => {
                        if *is_deferred {
                            ir.push_str(&format!("  ; [UMO Registered Defer Free: %{}]\n", target));
                            deferred_frees.push(CleanupOp::Free(target.clone()));
                        } else {
                            ir.push_str(&format!("  ; [UMO Inline Free: %{}]\n", target));
                            ir.push_str(&format!("  call void @free(i8* %{})\n", target));
                        }
                    }
                    _ => {}
                },

                // 7. Return Statement
                Statement::Return { expr } => {
                    let ret_val = if let Some(e) = expr {
                        self.eval_val(e, ir, scope_tree, string_constants, temp_counter)
                    } else {
                        "0".to_string()
                    };

                    // Execute any active cleanups before returning
                    Self::emit_cleanups(ir, deferred_frees, temp_counter, "Return");

                    ir.push_str(&format!("  ret i32 {}\n", ret_val));
                    custom_return = true;
                    break;
                }

                // 8. Channel Operations
                Statement::ChanMake {
                    target,
                    capacity,
                    elem_size,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%chan_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let make_res = format!("%make_ch_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_chan_make(i64 {}, i64 {})\n",
                        make_res, capacity, elem_size
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", make_res, ptr));

                    // Register RAII cleanup for channel resource
                    deferred_frees.push(CleanupOp::DestroyChan(ptr.clone()));

                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_chan)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                Statement::ChanSend { channel, expr } => {
                    if let Some(binding) = scope_tree.resolve(channel) {
                        let ch_loaded = format!("%ch_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ch_loaded, binding.llvm_ptr));

                        let val = self.eval_val(expr, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%val_ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));

                        let send_ok = format!("%send_ok_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i1 @__omni_chan_send_int(i8* {}, i64 {})\n",
                            send_ok, ch_loaded, val_64
                        ));
                    }
                }

                Statement::ChanRecv {
                    target,
                    channel,
                    is_scoped_decl,
                } => {
                    if let Some(binding) = scope_tree.resolve(channel) {
                        let ch_loaded = format!("%ch_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", ch_loaded, binding.llvm_ptr));

                        let recv_64 = format!("%recv_64_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i64 @__omni_chan_recv_int(i8* {})\n",
                            recv_64, ch_loaded
                        ));

                        let recv_32 = format!("%recv_32_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = trunc i64 {} to i32\n", recv_32, recv_64));

                        if let Some(ref var_name) = target {
                            let alloca_ptr = if *is_scoped_decl || scope_tree.resolve(var_name).is_none() {
                                let ptr = format!("%var_{}_{}", var_name, *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca i32\n", ptr));
                                scope_tree.insert(
                                    var_name.clone(),
                                    SymbolBinding {
                                        name: var_name.clone(),
                                        llvm_ptr: ptr.clone(),
                                        type_hint: "i32".to_string(),
                                        is_scoped: *is_scoped_decl,
                                    },
                                );
                                ptr
                            } else {
                                scope_tree.resolve(var_name).unwrap().llvm_ptr.clone()
                            };

                            ir.push_str(&format!("  store i32 {}, i32* {}\n", recv_32, alloca_ptr));
                        }
                    }
                }

                // 9. Polyglot List / Slice Operations
                Statement::ListInit {
                    target,
                    elements,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%slice_var_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let initial_cap = elements.len().max(4);
                    let slice_res = format!("%slice_init_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_slice_create(i64 {})\n",
                        slice_res, initial_cap
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", slice_res, ptr));

                    for elem in elements {
                        let val = self.eval_val(elem, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%elem_ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));
                        ir.push_str(&format!(
                            "  call void @__omni_slice_append(i8* {}, i64 {})\n",
                            slice_res, val_64
                        ));
                    }

                    // Register RAII cleanup for slice resource
                    deferred_frees.push(CleanupOp::DestroySlice(ptr.clone()));

                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_slice)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                Statement::ListAppend { target, expr } => {
                    if let Some(binding) = scope_tree.resolve(target) {
                        let slice_loaded = format!("%slice_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", slice_loaded, binding.llvm_ptr));

                        let val = self.eval_val(expr, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%app_ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));
                        ir.push_str(&format!(
                            "  call void @__omni_slice_append(i8* {}, i64 {})\n",
                            slice_loaded, val_64
                        ));
                    }
                }

                // 10. Polyglot Index Assignment: `items[0] = 99`, `m["key"] = 10`
                Statement::IndexSet {
                    target,
                    index_expr,
                    val_expr,
                } => {
                    if let Some(binding) = scope_tree.resolve(target) {
                        let col_loaded = format!("%col_load_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = load i8*, i8** {}\n", col_loaded, binding.llvm_ptr));

                        let val = self.eval_val(val_expr, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%val_ext_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));

                        if binding.type_hint.contains("omni_map") {
                            let trimmed_idx = index_expr.trim();
                            if (trimmed_idx.starts_with('"') && trimmed_idx.ends_with('"'))
                                || (trimmed_idx.starts_with('\'') && trimmed_idx.ends_with('\''))
                            {
                                let text = &trimmed_idx[1..trimmed_idx.len() - 1];
                                if let Some((const_name, len)) = Self::find_string_constant(string_constants, text) {
                                    let str_ptr = format!("%str_ptr_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!(
                                        "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                        str_ptr, len, len, const_name
                                    ));
                                    ir.push_str(&format!(
                                        "  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n",
                                        col_loaded, str_ptr, val_64
                                    ));
                                }
                            } else {
                                let idx_val = self.eval_val(index_expr, ir, scope_tree, string_constants, temp_counter);
                                let idx_64 = format!("%idx_ext_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = sext i32 {} to i64\n", idx_64, idx_val));
                                ir.push_str(&format!(
                                    "  call void @__omni_map_set_int(i8* {}, i64 {}, i64 {})\n",
                                    col_loaded, idx_64, val_64
                                ));
                            }
                        } else {
                            // Default to slice
                            let idx_val = self.eval_val(index_expr, ir, scope_tree, string_constants, temp_counter);
                            let idx_64 = format!("%idx_ext_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = sext i32 {} to i64\n", idx_64, idx_val));
                            ir.push_str(&format!(
                                "  call void @__omni_slice_set(i8* {}, i64 {}, i64 {})\n",
                                col_loaded, idx_64, val_64
                            ));
                        }
                    }
                }

                // 11. Polyglot Map / Dict Operations: `m = {"k": 10}`
                Statement::MapInit {
                    target,
                    entries,
                    is_scoped_decl,
                } => {
                    let ptr = format!("%map_var_{}_{}", target, *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!("  {} = alloca i8*\n", ptr));

                    let initial_cap = entries.len().max(8);
                    let map_res = format!("%map_init_{}", *temp_counter);
                    *temp_counter += 1;
                    ir.push_str(&format!(
                        "  {} = call i8* @__omni_map_create(i64 {})\n",
                        map_res, initial_cap
                    ));
                    ir.push_str(&format!("  store i8* {}, i8** {}\n", map_res, ptr));

                    for (k, v) in entries {
                        let val = self.eval_val(v, ir, scope_tree, string_constants, temp_counter);
                        let val_64 = format!("%map_val_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!("  {} = sext i32 {} to i64\n", val_64, val));

                        if let Some((const_name, len)) = Self::find_string_constant(string_constants, k) {
                            let str_ptr = format!("%str_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!(
                                "  {} = getelementptr inbounds [{} x i8], [{} x i8]* {}, i64 0, i64 0\n",
                                str_ptr, len, len, const_name
                            ));
                            ir.push_str(&format!(
                                "  call void @__omni_map_set(i8* {}, i8* {}, i64 {})\n",
                                map_res, str_ptr, val_64
                            ));
                        }
                    }

                    // Register RAII cleanup for map resource
                    deferred_frees.push(CleanupOp::DestroyMap(ptr.clone()));

                    scope_tree.insert(
                        target.clone(),
                        SymbolBinding {
                            name: target.clone(),
                            llvm_ptr: ptr,
                            type_hint: "i8* (omni_map)".to_string(),
                            is_scoped: *is_scoped_decl,
                        },
                    );
                }

                // 12. Go Concurrency Spawn: `go <call_expr>`
                Statement::GoSpawn { call_expr, idiom: _ } => {
                    let parts: Vec<&str> = call_expr.splitn(2, '(').collect();
                    let target_fn = parts[0].trim();
                    let raw_arg = parts.get(1).map(|s| s.trim_end_matches(')').trim()).unwrap_or("");

                    let arg_reg = if !raw_arg.is_empty() {
                        if let Some(binding) = scope_tree.resolve(raw_arg) {
                            if binding.type_hint == "i32" {
                                let loaded = format!("%arg_i32_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i32, i32* {}\n", loaded, binding.llvm_ptr));
                                let ext = format!("%arg_i64_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = zext i32 {} to i64\n", ext, loaded));
                                let ptr_cast = format!("%arg_ptr_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, ext));
                                ptr_cast
                            } else {
                                let loaded = format!("%arg_load_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = load i8*, i8** {}\n", loaded, binding.llvm_ptr));
                                loaded
                            }
                        } else if let Ok(val) = raw_arg.parse::<i32>() {
                            let ptr_cast = format!("%arg_ptr_{}", *temp_counter);
                            *temp_counter += 1;
                            ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, val));
                            ptr_cast
                        } else {
                            "null".to_string()
                        }
                    } else {
                        "null".to_string()
                    };

                    ir.push_str(&format!("  ; >>> [Go Concurrency Spawn: {}]\n", call_expr));
                    ir.push_str(&format!(
                        "  call void @__omni_go_spawn(void (i8*)* @__omni_trampoline_{}, i8* {})\n",
                        target_fn, arg_reg
                    ));
                    ir.push_str("  ; <<< [End Go Spawn]\n");
                }

                // 9. Source-Anchored Interpretive Block
                Statement::InterpretiveAnchor {
                    anchor_id,
                    file,
                    line_start,
                    line_end,
                    physical_source,
                    idiom,
                } => {
                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let envelope = SsaContextEnvelope {
                        target_abi: host_target_triple().to_string(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: "i32".to_string(),
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: physical_source.clone(),
                        idiom_hint: Some(format!("{:?}", idiom)),
                        anchor_id: Some(*anchor_id),
                        physical_source: Some(physical_source.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some("i32".to_string()),
                        action_directive: Some("SYNTHESIZE_ANONYMOUS_BLOCK".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    ir.push_str(&format!(
                        "  ; >>> [OMNI INTERPRETIVE ANCHOR #{} at {}:{}-{}]\n",
                        anchor_id, file, line_start, line_end
                    ));
                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                    ir.push_str(&format!("  ; <<< [END ANCHOR #{}]\n", anchor_id));
                }

                // 10. Intent Statement -> packed into SsaContextEnvelope
                Statement::Intent { instruction } => {
                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let clean_instruction = instruction
                        .trim_start_matches("intent")
                        .trim_start_matches(':')
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();

                    let idiom_hint = if clean_instruction.contains(".append(") {
                        Some("PythonListTransform".to_string())
                    } else {
                        None
                    };

                    let envelope = SsaContextEnvelope {
                        target_abi: host_target_triple().to_string(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: "i32".to_string(),
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: clean_instruction.clone(),
                        idiom_hint,
                        anchor_id: Some(*lbl_counter),
                        physical_source: Some(clean_instruction.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some("void".to_string()),
                        action_directive: Some("LOWER_ANONYMOUS_INTENT".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    ir.push_str(&format!(
                        "  ; =========================================================================\n  ; >>> [OMNI INTERPRETIVE ANCHOR: ANCHOR_{}]\n  ; Source: {}:{}\n  ; Directive: LOWER_ANONYMOUS_INTENT\n  ; Physical Source: \"{}\"\n  ; =========================================================================\n",
                        *lbl_counter, fn_name, *lbl_counter, instruction.trim()
                    ));
                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                    ir.push_str(&format!("  ; <<< [END ANCHOR: ANCHOR_{}]\n", *lbl_counter));
                }

                // 11. Fallback Concrete / Polyglot Statement -> packed with Idiom hint
                Statement::Concrete { code, idiom } => {
                    // Implicit return expression in non-main functions (e.g. Rust block `(value ^ salt) + 7`)
                    if fn_name != "main" && !code.trim().ends_with(';') {
                        if Self::split_binary_expr(code).is_some()
                            || scope_tree.resolve(code).is_some()
                            || code.trim().parse::<i32>().is_ok()
                        {
                            let ret_val = self.eval_val(code, ir, scope_tree, string_constants, temp_counter);
                            Self::emit_cleanups(ir, deferred_frees, temp_counter, "Implicit Return");
                            ir.push_str(&format!("  ret i32 {}\n", ret_val));
                            custom_return = true;
                            break;
                        }
                    }

                    let active_symbols: Vec<SsaVariable> = scope_tree
                        .active_bindings()
                        .into_iter()
                        .map(|b| SsaVariable {
                            name: b.name,
                            llvm_name: b.llvm_ptr,
                            type_hint: b.type_hint,
                        })
                        .collect();

                    let envelope = SsaContextEnvelope {
                        target_abi: host_target_triple().to_string(),
                        enclosing_fn: fn_name.to_string(),
                        return_type: "i32".to_string(),
                        next_ssa_idx: *temp_counter,
                        active_symbols,
                        struct_definitions: struct_defs.to_vec(),
                        raw_intent: code.clone(),
                        idiom_hint: Some(format!("{:?}", idiom)),
                        anchor_id: Some(*lbl_counter),
                        physical_source: Some(code.clone()),
                        concurrency_mode: None,
                        yield_expectation: Some("i32".to_string()),
                        action_directive: Some("LOWER_POLYGLOT_SYNTAX".to_string()),
                        ecosystem_context: ecosystem_context.map(|s| s.to_string()),
                        lifetime_context: None,
                        import_chain: import_chain.to_vec(),
                    };

                    let synthesized = self.engine.lower_intent(&envelope, temp)?;
                    let (dyn_decls, body) = extract_declarations_and_body(&synthesized);
                    for d in dyn_decls {
                        dynamic_declarations.insert(d);
                    }
                    ir.push_str(&body);
                    if !body.ends_with('\n') {
                        ir.push('\n');
                    }
                }

                // 12. Native Context Manager: `with Expr() as var:`
                Statement::WithContext {
                    context_expr,
                    var_alias,
                    body,
                } => {
                    let trimmed_expr = context_expr.trim();
                    let mut resource_cls = None;
                    let mut resource_raw = None;

                    if let Some(open) = trimmed_expr.find('(') {
                        if trimmed_expr.ends_with(')') {
                            let cls_cand = trimmed_expr[..open].trim();
                            if scope_tree.class_fields.contains_key(cls_cand) {
                                let cls_name = cls_cand.to_string();
                                let inst_alloc = format!("%inst_alloc_with_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = alloca %struct.{}\n", inst_alloc, cls_name));

                                let raw_inst = format!("%raw_with_init_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!("  {} = bitcast %struct.{}* {} to i8*\n", raw_inst, cls_name, inst_alloc));

                                let inner = &trimmed_expr[open + 1..trimmed_expr.len() - 1];
                                let raw_args: Vec<String> = if inner.is_empty() {
                                    Vec::new()
                                } else {
                                    omni_parser::OmniParser::split_respecting_quotes(inner, ',')
                                };

                                let mut arg_regs = Vec::new();
                                for a in &raw_args {
                                    let r = self.eval_val(a.trim(), ir, scope_tree, string_constants, temp_counter);
                                    let ext = format!("%with_init_ext_{}", *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = sext i32 {} to i64\n", ext, r));
                                    arg_regs.push(ext);
                                }
                                while arg_regs.len() < 3 {
                                    arg_regs.push("0".to_string());
                                }

                                let init_ret = format!("%with_init_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i32 @__omni_method_{}___init__(i8* {}, i64 {}, i64 {}, i64 {})\n",
                                    init_ret, cls_name, raw_inst, arg_regs[0], arg_regs[1], arg_regs[2]
                                ));

                                let enter_ret = format!("%with_enter_ret_{}", *temp_counter);
                                *temp_counter += 1;
                                ir.push_str(&format!(
                                    "  {} = call i32 @__omni_method_{}___enter__(i8* {}, i64 0, i64 0, i64 0)\n",
                                    enter_ret, cls_name, raw_inst
                                ));

                                if let Some(ref t) = var_alias {
                                    let target_ptr = format!("%var_{}_{}", t, *temp_counter);
                                    *temp_counter += 1;
                                    ir.push_str(&format!("  {} = alloca %struct.{}*\n", target_ptr, cls_name));
                                    ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls_name, inst_alloc, cls_name, target_ptr));
                                    scope_tree.insert(t.clone(), SymbolBinding {
                                        name: t.clone(),
                                        llvm_ptr: target_ptr,
                                        type_hint: format!("%struct.{}*", cls_name),
                                        is_scoped: true,
                                    });
                                }

                                resource_cls = Some(cls_name);
                                resource_raw = Some(raw_inst);
                            }
                        }
                    }

                    self.lower_statements(
                        body,
                        ir,
                        scope_tree,
                        deferred_frees,
                        string_constants,
                        cur_str_idx,
                        lbl_counter,
                        temp_counter,
                        fn_name,
                        struct_defs,
                        temp,
                        ecosystem_context,
                        import_chain,
                        dynamic_declarations,
                    )?;

                    if let (Some(cls_name), Some(raw_inst)) = (resource_cls, resource_raw) {
                        let exit_ret = format!("%with_exit_ret_{}", *temp_counter);
                        *temp_counter += 1;
                        ir.push_str(&format!(
                            "  {} = call i32 @__omni_method_{}___exit__(i8* {}, i64 0, i64 0, i64 0)\n",
                            exit_ret, cls_name, raw_inst
                        ));
                    }
                }

                // 13. Annotation / Decorator Binding
                Statement::AnnotationBinding { .. } => {}
            }
        }

        Ok(custom_return)
    }

    /// Generate unified LLVM IR text from OmniModule
    pub fn generate_module_ir(&self, module: &OmniModule, temp: f32) -> anyhow::Result<String> {
        let mut string_constants: Vec<(String, String, usize)> = Vec::new();
        let mut str_counter = 0;

        // Pre-scan statements for print("...") recursively to extract string constants
        for func in &module.functions {
            self.scan_strings_recursive(&func.body, &mut string_constants, &mut str_counter);
        }
        for cls in &module.structs_and_classes {
            for method in &cls.methods {
                self.scan_strings_recursive(&method.body, &mut string_constants, &mut str_counter);
            }
        }
        self.scan_strings_recursive(&module.top_level_statements, &mut string_constants, &mut str_counter);

        // Extract ecosystem and import context
        let mut ecosystem_hints = Vec::new();
        let mut import_chain = Vec::new();

        for imp in &module.imports {
            match imp {
                omni_preamble::ImportDirective::Ecosystem { ecosystem, module, alias } => {
                    let desc = match alias {
                        Some(a) => format!("{} (module: {}, alias: {})", ecosystem, module, a),
                        None => format!("{} (module: {})", ecosystem, module),
                    };
                    ecosystem_hints.push(desc.clone());
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::FileAlias { path, alias } => {
                    let desc = match alias {
                        Some(a) => format!("file: {} as {}", path.display(), a),
                        None => format!("file: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::DirectoryAlias { path, alias } => {
                    let desc = match alias {
                        Some(a) => format!("dir: {} as {}", path.display(), a),
                        None => format!("dir: {}", path.display()),
                    };
                    import_chain.push(desc);
                }
                omni_preamble::ImportDirective::FromDirectory { dir_path, symbols } => {
                    import_chain.push(format!("from {}: {:?}", dir_path.display(), symbols));
                }
            }
        }

        let ecosystem_context = if ecosystem_hints.is_empty() {
            None
        } else {
            Some(ecosystem_hints.join(", "))
        };

        // 1. Structs and Classes -> Monomorphized Field Layout
        let mut class_fields_map: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut struct_defs = Vec::new();
        let mut struct_defs_ir = String::new();

        for cls in &module.structs_and_classes {
            let mut field_names: Vec<String> = Vec::new();
            for f in &cls.fields {
                if !field_names.contains(&f.name) {
                    field_names.push(f.name.clone());
                }
            }
            if field_names.is_empty() {
                field_names.push("_unused".to_string());
            }

            let mut f_map = HashMap::new();
            for (idx, name) in field_names.iter().enumerate() {
                f_map.insert(name.clone(), idx);
            }
            class_fields_map.insert(cls.name.clone(), f_map);

            let type_list = vec!["i64"; field_names.len()].join(", ");
            let s_def = format!("%struct.{} = type {{ {} }}", cls.name, type_list);
            struct_defs_ir.push_str(&format!("; Struct Layout: {} ({} fields: {})\n", cls.name, field_names.len(), field_names.join(", ")));
            struct_defs_ir.push_str(&format!("{}\n\n", s_def));
            struct_defs.push(s_def);
        }

        // 1-B. Enums -> Tagged Union Types & Variant Maps
        let mut enum_variants_map: HashMap<String, (String, usize)> = HashMap::new();
        for e in &module.enums {
            let e_def = format!("%enum.{} = type {{ i32, i64 }}", e.name);
            struct_defs_ir.push_str(&format!("; Tagged Enum Layout: {} ({} variants)\n", e.name, e.variants.len()));
            struct_defs_ir.push_str(&format!("{}\n\n", e_def));
            struct_defs.push(e_def);

            for v in &e.variants {
                let qualified = format!("{}::{}", e.name, v.name);
                enum_variants_map.insert(qualified, (e.name.clone(), v.tag));
                enum_variants_map.insert(v.name.clone(), (e.name.clone(), v.tag));
            }
        }

        // 2. Functions & Methods Shared State
        let mut known_functions_set = HashSet::new();
        for f in &module.functions {
            known_functions_set.insert(f.name.clone());
        }

        let mut functions_ir = String::new();
        let mut dynamic_declarations = HashSet::new();
        let mut has_user_main = false;
        let mut cur_str_idx = 0;
        let mut lbl_counter = 0;
        let mut temp_counter = 0;

        // 1-C. Struct & Class Inherent Methods (@__omni_method_<cls>_<method>)
        for cls in &module.structs_and_classes {
            for method in &cls.methods {
                functions_ir.push_str(&format!(
                    "define i32 @__omni_method_{}_{}(i8* %self_raw, i64 %len, i64 %arg2, i64 %arg3) {{\nentry:\n",
                    cls.name, method.name
                ));

                let mut scope_tree = ScopeTree::new();
                scope_tree.class_fields = class_fields_map.clone();
                scope_tree.enum_variants = enum_variants_map.clone();
                scope_tree.known_functions = known_functions_set.clone();
                let mut deferred_frees: Vec<CleanupOp> = Vec::new();

                let self_ptr = format!("%self_ptr_{}", temp_counter);
                temp_counter += 1;
                functions_ir.push_str(&format!("  {} = bitcast i8* %self_raw to %struct.{}*\n", self_ptr, cls.name));
                let self_alloca = format!("%self_alloca_{}", temp_counter);
                temp_counter += 1;
                functions_ir.push_str(&format!("  {} = alloca %struct.{}*\n", self_alloca, cls.name));
                functions_ir.push_str(&format!("  store %struct.{}* {}, %struct.{}** {}\n", cls.name, self_ptr, cls.name, self_alloca));

                scope_tree.insert("self".to_string(), SymbolBinding {
                    name: "self".to_string(),
                    llvm_ptr: self_alloca,
                    type_hint: format!("%struct.{}*", cls.name),
                    is_scoped: true,
                });

                for (p_idx, param) in method.params.iter().skip(1).enumerate() {
                    let reg_src = match p_idx {
                        0 => "%len",
                        1 => "%arg2",
                        _ => "%arg3",
                    };
                    let cast_i32 = format!("%param_i32_{}_{}", param.name, temp_counter);
                    temp_counter += 1;
                    functions_ir.push_str(&format!("  {} = trunc i64 {} to i32\n", cast_i32, reg_src));
                    let ptr = format!("%param_{}_{}", param.name, temp_counter);
                    temp_counter += 1;
                    functions_ir.push_str(&format!("  {} = alloca i32\n", ptr));
                    functions_ir.push_str(&format!("  store i32 {}, i32* {}\n", cast_i32, ptr));
                    scope_tree.insert(param.name.clone(), SymbolBinding {
                        name: param.name.clone(),
                        llvm_ptr: ptr,
                        type_hint: "i32".to_string(),
                        is_scoped: true,
                    });
                }

                let custom_ret = self.lower_statements(
                    &method.body,
                    &mut functions_ir,
                    &mut scope_tree,
                    &mut deferred_frees,
                    &string_constants,
                    &mut cur_str_idx,
                    &mut lbl_counter,
                    &mut temp_counter,
                    &format!("{}_{}", cls.name, method.name),
                    &struct_defs,
                    temp,
                    ecosystem_context.as_deref(),
                    &import_chain,
                    &mut dynamic_declarations,
                )?;

                if !custom_ret {
                    Self::emit_cleanups(&mut functions_ir, &deferred_frees, &mut temp_counter, "Method Exit");
                    functions_ir.push_str("  ret i32 0\n");
                }
                functions_ir.push_str("}\n\n");
            }
        }

        // 2. Functions (namespaced as @__omni_fn_<name>)

        for func in &module.functions {
            if func.name == "main" {
                has_user_main = true;
                functions_ir.push_str(
                    "define i32 @__omni_fn_main(i8* %data, i64 %len) {\nentry:\n",
                );
            } else {
                functions_ir.push_str(&format!(
                    "define i32 @__omni_fn_{}(i64 %arg0, i64 %arg1, i64 %arg2, i64 %arg3) {{\nentry:\n",
                    func.name
                ));
            }

            let mut scope_tree = ScopeTree::new();
            scope_tree.class_fields = class_fields_map.clone();
            scope_tree.enum_variants = enum_variants_map.clone();
            scope_tree.known_functions = known_functions_set.clone();
            let mut deferred_frees: Vec<CleanupOp> = Vec::new();

            // Bind function parameters with ABI and type awareness
            if func.name == "main" {
                for (i, param) in func.params.iter().enumerate() {
                    let p_name_lower = param.name.to_lowercase();
                    let is_chan = param.type_hint.as_deref().map(|th| th.contains("chan")).unwrap_or(false)
                        || p_name_lower == "ch" || p_name_lower == "chan" || p_name_lower.starts_with("chan_") || p_name_lower.ends_with("_chan") || p_name_lower.starts_with("ch_");
                    let is_slice = param.type_hint.as_deref().map(|th| th.contains("slice") || th.contains("list")).unwrap_or(false)
                        || p_name_lower == "slice" || p_name_lower == "items" || p_name_lower == "arr" || p_name_lower == "list"
                        || p_name_lower.starts_with("items_") || p_name_lower.starts_with("arr_") || p_name_lower.starts_with("slice_") || p_name_lower.ends_with("_arr") || p_name_lower.ends_with("_slice") || p_name_lower.ends_with("_list");
                    let is_map = param.type_hint.as_deref().map(|th| th.contains("map") || th.contains("dict")).unwrap_or(false)
                        || p_name_lower == "map" || p_name_lower == "dict" || p_name_lower.starts_with("map_") || p_name_lower.starts_with("dict_") || p_name_lower.ends_with("_map") || p_name_lower.ends_with("_dict");
                    let is_int = param.type_hint.as_deref().map(|th| th == "int" || th == "i32" || th == "i64").unwrap_or(false)
                        || (!is_chan && !is_slice && !is_map && (p_name_lower == "val" || p_name_lower == "num" || p_name_lower == "count" || p_name_lower == "idx" || p_name_lower == "n" || p_name_lower == "x" || p_name_lower == "y" || p_name_lower == "z"));

                    if i == 0 {
                        if is_int {
                            let cast_i64 = format!("%param_int64_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = ptrtoint i8* %data to i64\n", cast_i64));
                            let cast_i32 = format!("%param_i32_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = trunc i64 {} to i32\n", cast_i32, cast_i64));
                            let ptr = format!("%param_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = alloca i32\n", ptr));
                            functions_ir.push_str(&format!("  store i32 {}, i32* {}\n", cast_i32, ptr));
                            scope_tree.insert(
                                param.name.clone(),
                                SymbolBinding {
                                    name: param.name.clone(),
                                    llvm_ptr: ptr,
                                    type_hint: "i32".to_string(),
                                    is_scoped: true,
                                },
                            );
                        } else {
                            let type_hint = if is_map {
                                "i8* (omni_map)".to_string()
                            } else if is_slice {
                                "i8* (omni_slice)".to_string()
                            } else {
                                "i8* (omni_chan)".to_string()
                            };
                            let ptr = format!("%param_{}_{}", param.name, temp_counter);
                            temp_counter += 1;
                            functions_ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                            functions_ir.push_str(&format!("  store i8* %data, i8** {}\n", ptr));
                            scope_tree.insert(
                                param.name.clone(),
                                SymbolBinding {
                                    name: param.name.clone(),
                                    llvm_ptr: ptr,
                                    type_hint,
                                    is_scoped: true,
                                },
                            );
                        }
                    } else if i == 1 {
                        let cast_i32 = format!("%param_i32_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = trunc i64 %len to i32\n", cast_i32));
                        let ptr = format!("%param_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = alloca i32\n", ptr));
                        functions_ir.push_str(&format!("  store i32 {}, i32* {}\n", cast_i32, ptr));
                        scope_tree.insert(
                            param.name.clone(),
                            SymbolBinding {
                                name: param.name.clone(),
                                llvm_ptr: ptr,
                                type_hint: "i32".to_string(),
                                is_scoped: true,
                            },
                        );
                    }
                }
            } else {
                for (i, param) in func.params.iter().enumerate() {
                    if i >= 4 { break; }
                    let arg_src = format!("%arg{}", i);
                    let p_name_lower = param.name.to_lowercase();
                    let is_chan = param.type_hint.as_deref().map(|th| th.contains("chan")).unwrap_or(false)
                        || p_name_lower == "ch" || p_name_lower == "chan" || p_name_lower.starts_with("chan_") || p_name_lower.ends_with("_chan") || p_name_lower.starts_with("ch_");
                    let is_slice = param.type_hint.as_deref().map(|th| th.contains("slice") || th.contains("list")).unwrap_or(false)
                        || p_name_lower == "slice" || p_name_lower == "items" || p_name_lower == "arr" || p_name_lower == "list"
                        || p_name_lower.starts_with("items_") || p_name_lower.starts_with("arr_") || p_name_lower.starts_with("slice_") || p_name_lower.ends_with("_arr") || p_name_lower.ends_with("_slice") || p_name_lower.ends_with("_list");
                    let is_map = param.type_hint.as_deref().map(|th| th.contains("map") || th.contains("dict")).unwrap_or(false)
                        || p_name_lower == "map" || p_name_lower == "dict" || p_name_lower.starts_with("map_") || p_name_lower.starts_with("dict_") || p_name_lower.ends_with("_map") || p_name_lower.ends_with("_dict");

                    if is_chan || is_slice || is_map {
                        let ptr_cast = format!("%ptr_cast_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = inttoptr i64 {} to i8*\n", ptr_cast, arg_src));
                        let ptr = format!("%param_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = alloca i8*\n", ptr));
                        functions_ir.push_str(&format!("  store i8* {}, i8** {}\n", ptr_cast, ptr));
                        let type_hint = if is_map {
                            "i8* (omni_map)".to_string()
                        } else if is_slice {
                            "i8* (omni_slice)".to_string()
                        } else {
                            "i8* (omni_chan)".to_string()
                        };
                        scope_tree.insert(
                            param.name.clone(),
                            SymbolBinding {
                                name: param.name.clone(),
                                llvm_ptr: ptr,
                                type_hint,
                                is_scoped: true,
                            },
                        );
                    } else {
                        let cast_i32 = format!("%param_i32_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = trunc i64 {} to i32\n", cast_i32, arg_src));
                        let ptr = format!("%param_{}_{}", param.name, temp_counter);
                        temp_counter += 1;
                        functions_ir.push_str(&format!("  {} = alloca i32\n", ptr));
                        functions_ir.push_str(&format!("  store i32 {}, i32* {}\n", cast_i32, ptr));
                        scope_tree.insert(
                            param.name.clone(),
                            SymbolBinding {
                                name: param.name.clone(),
                                llvm_ptr: ptr,
                                type_hint: "i32".to_string(),
                                is_scoped: true,
                            },
                        );
                    }
                }
            }

            let custom_ret = self.lower_statements(
                &func.body,
                &mut functions_ir,
                &mut scope_tree,
                &mut deferred_frees,
                &string_constants,
                &mut cur_str_idx,
                &mut lbl_counter,
                &mut temp_counter,
                &func.name,
                &struct_defs,
                temp,
                ecosystem_context.as_deref(),
                &import_chain,
                &mut dynamic_declarations,
            )?;

            if !custom_ret {
                Self::emit_cleanups(&mut functions_ir, &deferred_frees, &mut temp_counter, "Function Exit");
                functions_ir.push_str("  ret i32 0\n");
            }
            functions_ir.push_str("}\n\n");

            // Static Trampoline for Fiber Concurrency (100% C-ABI compliant void (*)(void*))
            if func.name == "main" {
                functions_ir.push_str(&format!(
                    "define void @__omni_trampoline_{}(i8* %raw_arg) {{\nentry:\n  %res = call i32 @__omni_fn_{}(i8* %raw_arg, i64 0)\n  ret void\n}}\n\n",
                    func.name, func.name
                ));
            } else {
                functions_ir.push_str(&format!(
                    "define void @__omni_trampoline_{}(i8* %raw_arg) {{\nentry:\n  %raw_int = ptrtoint i8* %raw_arg to i64\n  %res = call i32 @__omni_fn_{}(i64 %raw_int, i64 0, i64 0, i64 0)\n  ret void\n}}\n\n",
                    func.name, func.name
                ));
            }
        }

        // Harvest any foreign C symbols directly called in functions_ir
        let call_c_re = Regex::new(r#"call\s+[a-zA-Z0-9_*]+\s+@([a-zA-Z0-9_]+)\("#).unwrap();
        for cap in call_c_re.captures_iter(&functions_ir) {
            let sym = &cap[1];
            if !sym.starts_with("__omni_")
                && sym != "malloc"
                && sym != "free"
                && sym != "printf"
                && sym != "sleep"
                && sym != "usleep"
                && sym != "fast_hash_block"
                && !known_functions_set.contains(sym)
            {
                dynamic_declarations.insert(format!("declare i32 @{}(...)", sym));
            }
        }

        // Assemble full Module IR
        let mut ir = String::new();
        ir.push_str("; ModuleID = '");
        ir.push_str(&module.name);
        ir.push_str("'\nsource_filename = \"");
        ir.push_str(&module.name);
        ir.push_str(".omni\"\ntarget triple = \"");
        ir.push_str(host_target_triple());
        ir.push_str("\"\n\n");

        // Runtime headers & extern declarations
        ir.push_str("; Standard Runtime & libc Declarations\n");
        ir.push_str("declare i8* @malloc(i64)\n");
        ir.push_str("declare void @free(i8*)\n");
        ir.push_str("declare i32 @printf(i8*, ...)\n");
        ir.push_str(omni_runtime::runtime_llvm_declarations());
        ir.push_str("\n");

        if !dynamic_declarations.is_empty() {
            ir.push_str("; Dynamically Harvested External Declarations\n");
            let mut sorted_decls: Vec<_> = dynamic_declarations.into_iter().collect();
            sorted_decls.sort();
            for decl in sorted_decls {
                ir.push_str(&decl);
                ir.push('\n');
            }
            ir.push('\n');
        }

        // Global string constants
        for (const_name, text, len) in &string_constants {
            ir.push_str(&format!(
                "{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1\n",
                const_name, len, text
            ));
        }
        ir.push_str("\n");

        // Struct definitions
        ir.push_str(&struct_defs_ir);

        // Function definitions
        ir.push_str(&functions_ir);

        // 3. Entrypoint @main
        ir.push_str("define i32 @main(i32 %argc, i8** %argv) {\nentry:\n");

        if has_user_main {
            ir.push_str("  %exit_code = call i32 @__omni_fn_main(i8* null, i64 0)\n");
            ir.push_str("  ret i32 %exit_code\n");
        } else if let Some(f) = module.functions.first() {
            ir.push_str(&format!(
                "  %res = call i32 @__omni_fn_{}(i64 0, i64 0, i64 0, i64 0)\n",
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

#[cfg(test)]
mod tests {
    use super::*;
    use omni_engine::MockSynthesisEngine;
    use omni_parser::OmniParser;
    use omni_verify::LlvmVerifier;

    #[test]
    fn test_codegen_collections_verify() {
        let src = r#"
def main():
    items = [10, 20, 30]
    items.append(40)
    items[0] = 99
    scores = {"alice": 100, "bob": 200}
    scores["carol"] = 300
    for i in len(items):
        print(items[i])
    print(scores["alice"])
    return 0
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("coll_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("__omni_slice_create"));
        assert!(ir.contains("__omni_slice_append"));
        assert!(ir.contains("__omni_slice_get"));
        assert!(ir.contains("__omni_map_create"));
        assert!(ir.contains("__omni_map_set"));
        assert!(ir.contains("__omni_map_get"));
        assert!(ir.contains("__omni_slice_destroy"));
        assert!(ir.contains("__omni_map_destroy"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier.verify_ir(&ir).expect("LLVM IR verification failed for collections");
    }

    #[test]
    fn test_codegen_channels_and_spawn_verify() {
        let src = r#"
def worker(ch):
    ch <- 42
    return 0

def main():
    ch := make(chan int, 4)
    go worker(ch)
    val := <-ch
    return val
"#;
        let parser = OmniParser::new();
        let module = parser.parse_module("chan_codegen_test", src);
        let engine = MockSynthesisEngine::new();
        let codegen = CodeGenerator::new(&engine);

        let ir = codegen.generate_module_ir(&module, 0.0).expect("Codegen failed");
        assert!(ir.contains("__omni_chan_make"));
        assert!(ir.contains("__omni_chan_send_int"));
        assert!(ir.contains("__omni_chan_recv_int"));
        assert!(ir.contains("__omni_go_spawn"));
        assert!(ir.contains("__omni_chan_destroy"));

        let verifier = LlvmVerifier::new().expect("Failed to initialize LlvmVerifier");
        verifier.verify_ir(&ir).expect("LLVM IR verification failed for channels and spawn");
    }
}
