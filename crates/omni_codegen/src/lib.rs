use omni_engine::InferenceEngine;
use omni_mem::UnifiedMemOp;
use omni_parser::{OmniModule, Statement};

pub struct CodeGenerator<'a> {
    engine: &'a dyn InferenceEngine,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(engine: &'a dyn InferenceEngine) -> Self {
        Self { engine }
    }

    /// Generate unified LLVM IR text from OmniModule
    pub fn generate_module_ir(&self, module: &OmniModule, temp: f32) -> anyhow::Result<String> {
        let mut ir = String::new();

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

        ir.push_str("@.str.ok = private unnamed_addr constant [27 x i8] c\"[OmniScript] Execution OK\\0A\\00\", align 1\n\n");

        // 1. Classes -> Struct Types
        for cls in &module.structs_and_classes {
            ir.push_str(&format!("; Class: {}\n", cls.name));
            ir.push_str(&format!("%struct.{} = type {{ i8* }}\n\n", cls.name));
        }

        // 2. Functions
        for func in &module.functions {
            ir.push_str(&format!("define i32 @{}(i8* %data, i64 %len) {{\nentry:\n", func.name));

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

            // Lower body statements
            for stmt in &func.body {
                match stmt {
                    Statement::Intent { instruction } => {
                        let synthesized = self.engine.lower_intent(&func.name, instruction, temp)?;
                        ir.push_str(&synthesized);
                    }
                    Statement::Concrete { code } => {
                        ir.push_str(&format!("  ; [Concrete]: {}\n", code));
                    }
                    _ => {}
                }
            }

            ir.push_str("  ret i32 0\n}\n\n");
        }

        // 3. Entrypoint @main
        ir.push_str("define i32 @main(i32 %argc, i8** %argv) {\nentry:\n");
        ir.push_str("  %msg_ptr = getelementptr inbounds [27 x i8], [27 x i8]* @.str.ok, i64 0, i64 0\n");
        ir.push_str("  call i32 (i8*, ...) @printf(i8* %msg_ptr)\n");
        
        // Invoke first function if present
        if let Some(f) = module.functions.first() {
            ir.push_str(&format!("  %res = call i32 @{}(i8* null, i64 0)\n", f.name));
        }

        ir.push_str("  ret i32 0\n}\n");

        Ok(ir)
    }
}
