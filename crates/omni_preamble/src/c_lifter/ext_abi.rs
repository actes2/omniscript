use super::binfmt::{BinaryAbi, BinaryArch, ParsedBinaryImage};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// LLVM struct layout shared across all lifted internal functions:
/// - Field 0: `[32 x i64]` GP registers (`RAX..R15` on x86_64; `X0..X30, SP` on AArch64; slot 30 on x86_64 = scratch errno cell)
/// - Field 1: `[32 x <16 x i8>]` 128-bit SIMD registers (`XMM0..XMM15` / `V0..V31`)
/// - Field 2: `[4 x i8]` condition flags (`[ZF/Z, SF/N, CF/C, OF/V]`)
pub const OMNI_CPU_STATE_TY: &str =
    "%OmniCpuState = type { [32 x i64], [32 x <16 x i8>], [4 x i8] }";

pub const FLAG_Z: u8 = 0;
pub const FLAG_N: u8 = 1;
pub const FLAG_C: u8 = 2;
pub const FLAG_V: u8 = 3;

/// Format `[N x i8] c"..."` byte initializer for LLVM IR global memory image.
pub fn emit_llvm_byte_array_global(out: &mut String, global_name: &str, bytes: &[u8]) {
    let len = bytes.len().max(16);
    let _ = write!(
        out,
        "@{} = internal global [{} x i8] c\"",
        global_name, len
    );
    for i in 0..len {
        let b = bytes.get(i).copied().unwrap_or(0);
        if (0x20..=0x7E).contains(&b) && b != b'"' && b != b'\\' {
            out.push(b as char);
        } else {
            let _ = write!(out, "\\{:02X}", b);
        }
    }
    out.push_str("\", align 16\n");
}

/// Emit `alwaysinline` VMA-to-host-pointer translator so both direct `lea`/`adrp` pointers
/// and relocated `.got`/`.data.rel.ro` VMAs resolve seamlessly to `@__omni_lifted_img_<id>`.
pub fn emit_vma_helpers(out: &mut String, mod_id: &str, img: &ParsedBinaryImage) {
    let img_len = img.image_bytes.len().max(16);
    let img_glob = format!("__omni_lifted_img_{}", mod_id);
    let _ = writeln!(
        out,
        "define internal i8* @__omni_vma_to_ptr_{id}(i64 %addr) alwaysinline nounwind {{
entry:
  %off = sub i64 %addr, {base}
  %in_img = icmp ult i64 %off, {len}
  %safe_off = select i1 %in_img, i64 %off, i64 0
  %img_p = getelementptr inbounds [{len} x i8], [{len} x i8]* @{glob}, i64 0, i64 %safe_off
  %raw_p = inttoptr i64 %addr to i8*
  %res = select i1 %in_img, i8* %img_p, i8* %raw_p
  ret i8* %res
}}

define internal i64 @__omni_norm_ret_ptr_{id}(i64 %val) alwaysinline nounwind {{
entry:
  %off = sub i64 %val, {base}
  %in_img = icmp ult i64 %off, {len}
  %safe_off = select i1 %in_img, i64 %off, i64 0
  %img_p = getelementptr inbounds [{len} x i8], [{len} x i8]* @{glob}, i64 0, i64 %safe_off
  %host_i = ptrtoint i8* %img_p to i64
  %res = select i1 %in_img, i64 %host_i, i64 %val
  ret i64 %res
}}",
        id = mod_id,
        base = img.base_vma,
        len = img_len,
        glob = img_glob,
    );
}

#[derive(Default)]
pub struct ExternalDeclTracker {
    pub needed_decls: BTreeSet<String>,
    pub uses_dynamic_stack_probe: bool,
}

impl ExternalDeclTracker {
    /// Read `idx`-th 64-bit integer/pointer argument from `%OmniCpuState` according to the binary's ABI!
    fn emit_read_abi_arg(
        &self,
        out: &mut String,
        tmp: &mut usize,
        abi: BinaryAbi,
        mod_id: &str,
        arg_idx: usize,
    ) -> String {
        let gpr_slot: Option<usize> = match abi {
            BinaryAbi::SysV64 => match arg_idx {
                0 => Some(7), // RDI
                1 => Some(6), // RSI
                2 => Some(2), // RDX
                3 => Some(1), // RCX
                4 => Some(8), // R8
                5 => Some(9), // R9
                _ => None,
            },
            BinaryAbi::Win64 => match arg_idx {
                0 => Some(1), // RCX
                1 => Some(2), // RDX
                2 => Some(8), // R8
                3 => Some(9), // R9
                _ => None,
            },
            BinaryAbi::Aapcs64 | BinaryAbi::AppleArm64 => {
                if arg_idx < 8 {
                    Some(arg_idx) // X0..X7
                } else {
                    None
                }
            }
        };

        if let Some(slot) = gpr_slot {
            let p = next_t(tmp);
            let v = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}",
                p, slot
            );
            let _ = writeln!(out, "  {} = load i64, i64* {}, align 8", v, p);
            v
        } else {
            // Stack argument (before pushing return address):
            // - Win64: 32-byte shadow space at `[RSP + 0..31]`, arg4 at `[RSP + 32]`, arg5 at `[RSP + 40]`
            // - SysV64: arg6 at `[RSP + 0]`, arg7 at `[RSP + 8]`
            // - ARM64: arg8 at `[SP + 0]`, arg9 at `[SP + 8]`
            let (sp_slot, stack_off) = match abi {
                BinaryAbi::Win64 => (4usize, 32u64 + ((arg_idx - 4) as u64) * 8),
                BinaryAbi::SysV64 => (4usize, ((arg_idx - 6) as u64) * 8),
                BinaryAbi::Aapcs64 | BinaryAbi::AppleArm64 => {
                    (31usize, ((arg_idx - 8) as u64) * 8)
                }
            };
            let sp_p = next_t(tmp);
            let sp_v = next_t(tmp);
            let ea = next_t(tmp);
            let ptr8 = next_t(tmp);
            let ptr64 = next_t(tmp);
            let val = next_t(tmp);
            let _ = writeln!(
                out,
                "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}",
                sp_p, sp_slot
            );
            let _ = writeln!(out, "  {} = load i64, i64* {}, align 8", sp_v, sp_p);
            let _ = writeln!(out, "  {} = add i64 {}, {}", ea, sp_v, stack_off);
            let _ = writeln!(
                out,
                "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
                ptr8, mod_id, ea
            );
            let _ = writeln!(out, "  {} = bitcast i8* {} to i64*", ptr64, ptr8);
            let _ = writeln!(out, "  {} = load i64, i64* {}, align 1", val, ptr64);
            val
        }
    }

    fn emit_write_abi_ret(&self, out: &mut String, tmp: &mut usize, arch: BinaryArch, val_reg: &str) {
        let ret_slot = match arch {
            BinaryArch::X86_64 => 0,  // RAX
            BinaryArch::Aarch64 => 0, // X0
        };
        let p = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = getelementptr inbounds %OmniCpuState, %OmniCpuState* %st, i32 0, i32 0, i32 {}",
            p, ret_slot
        );
        let _ = writeln!(out, "  store i64 {}, i64* {}, align 8", val_reg, p);
    }

    fn emit_arg_as_ptr(
        &self,
        out: &mut String,
        tmp: &mut usize,
        abi: BinaryAbi,
        mod_id: &str,
        arg_idx: usize,
    ) -> String {
        let raw = self.emit_read_abi_arg(out, tmp, abi, mod_id, arg_idx);
        let p = next_t(tmp);
        let _ = writeln!(
            out,
            "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
            p, mod_id, raw
        );
        p
    }

    /// Emit a marshaled call from `%OmniCpuState` (in the source binary's ABI) to a host/target C symbol.
    pub fn emit_external_call(
        &mut self,
        out: &mut String,
        tmp: &mut usize,
        arch: BinaryArch,
        abi: BinaryAbi,
        mod_id: &str,
        raw_sym: &str,
    ) {
        let sym = raw_sym.trim_start_matches("__imp_");
        match sym {
            // Stack-probe helpers on Windows/macOS: our wrapper stack buffer is pre-allocated & mapped!
            "___chkstk_ms" | "__chkstk_ms" | "__chkstk" | "___chkstk" | "__chkstk_darwin"
            | "___chkstk_darwin" | "_alloca" => {
                self.uses_dynamic_stack_probe = true;
            }

            // Thread-local `errno` pointer across Linux (`__errno_location`), macOS (`__error`), Windows (`_errno`)
            "__errno_location" | "__error" | "_errno" => {
                // Slot 28 in `regs` (unused on x86_64; for ARM64 use a dedicated scratch `alloca` or slot)
                // Better: return address of `[4 x i8]` or slot 30 on x86_64; to be safe on ARM64 too,
                // emit an internal global `@__omni_errno_cell_<id>`!
                self.needed_decls
                    .insert(format!("@__omni_errno_cell_{} = internal global i32 0, align 4", mod_id));
                let p = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = ptrtoint i32* @__omni_errno_cell_{} to i64",
                    p, mod_id
                );
                self.emit_write_abi_ret(out, tmp, arch, &p);
            }

            "malloc" | "_malloc" | "_aligned_malloc" => {
                self.needed_decls
                    .insert("declare i8* @malloc(i64)".to_string());
                let sz = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let p = next_t(tmp);
                let pi = next_t(tmp);
                let _ = writeln!(out, "  {} = call i8* @malloc(i64 {})", p, sz);
                let _ = writeln!(out, "  {} = ptrtoint i8* {} to i64", pi, p);
                self.emit_write_abi_ret(out, tmp, arch, &pi);
            }
            "calloc" | "_calloc" => {
                self.needed_decls
                    .insert("declare i8* @calloc(i64, i64)".to_string());
                let n = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let sz = self.emit_read_abi_arg(out, tmp, abi, mod_id, 1);
                let p = next_t(tmp);
                let pi = next_t(tmp);
                let _ = writeln!(out, "  {} = call i8* @calloc(i64 {}, i64 {})", p, n, sz);
                let _ = writeln!(out, "  {} = ptrtoint i8* {} to i64", pi, p);
                self.emit_write_abi_ret(out, tmp, arch, &pi);
            }
            "realloc" | "_realloc" => {
                self.needed_decls
                    .insert("declare i8* @realloc(i8*, i64)".to_string());
                let old_p = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let sz = self.emit_read_abi_arg(out, tmp, abi, mod_id, 1);
                let p = next_t(tmp);
                let pi = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @realloc(i8* {}, i64 {})",
                    p, old_p, sz
                );
                let _ = writeln!(out, "  {} = ptrtoint i8* {} to i64", pi, p);
                self.emit_write_abi_ret(out, tmp, arch, &pi);
            }
            "free" | "_free" | "_aligned_free" => {
                self.needed_decls
                    .insert("declare void @free(i8*)".to_string());
                let p = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let _ = writeln!(out, "  call void @free(i8* {})", p);
            }
            "memcpy" | "__memcpy_chk" => {
                self.needed_decls
                    .insert("declare i8* @memcpy(i8*, i8*, i64)".to_string());
                let dst_raw = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let dst = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
                    dst, mod_id, dst_raw
                );
                let src = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 1);
                let len = self.emit_read_abi_arg(out, tmp, abi, mod_id, 2);
                let r = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @memcpy(i8* {}, i8* {}, i64 {})",
                    r, dst, src, len
                );
                self.emit_write_abi_ret(out, tmp, arch, &dst_raw);
            }
            "memmove" | "__memmove_chk" => {
                self.needed_decls
                    .insert("declare i8* @memmove(i8*, i8*, i64)".to_string());
                let dst_raw = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let dst = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
                    dst, mod_id, dst_raw
                );
                let src = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 1);
                let len = self.emit_read_abi_arg(out, tmp, abi, mod_id, 2);
                let r = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @memmove(i8* {}, i8* {}, i64 {})",
                    r, dst, src, len
                );
                self.emit_write_abi_ret(out, tmp, arch, &dst_raw);
            }
            "memset" | "__memset_chk" => {
                self.needed_decls
                    .insert("declare i8* @memset(i8*, i32, i64)".to_string());
                let dst_raw = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let dst = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @__omni_vma_to_ptr_{}(i64 {})",
                    dst, mod_id, dst_raw
                );
                let c64 = self.emit_read_abi_arg(out, tmp, abi, mod_id, 1);
                let c32 = next_t(tmp);
                let _ = writeln!(out, "  {} = trunc i64 {} to i32", c32, c64);
                let len = self.emit_read_abi_arg(out, tmp, abi, mod_id, 2);
                let r = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @memset(i8* {}, i32 {}, i64 {})",
                    r, dst, c32, len
                );
                self.emit_write_abi_ret(out, tmp, arch, &dst_raw);
            }
            "bzero" | "__bzero" => {
                self.needed_decls
                    .insert("declare i8* @memset(i8*, i32, i64)".to_string());
                let dst = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let len = self.emit_read_abi_arg(out, tmp, abi, mod_id, 1);
                let r = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i8* @memset(i8* {}, i32 0, i64 {})",
                    r, dst, len
                );
            }
            "memcmp" => {
                self.needed_decls
                    .insert("declare i32 @memcmp(i8*, i8*, i64)".to_string());
                let p1 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let p2 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 1);
                let len = self.emit_read_abi_arg(out, tmp, abi, mod_id, 2);
                let r32 = next_t(tmp);
                let r64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i32 @memcmp(i8* {}, i8* {}, i64 {})",
                    r32, p1, p2, len
                );
                let _ = writeln!(out, "  {} = sext i32 {} to i64", r64, r32);
                self.emit_write_abi_ret(out, tmp, arch, &r64);
            }
            "strlen" => {
                self.needed_decls
                    .insert("declare i64 @strlen(i8*)".to_string());
                let p = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let r = next_t(tmp);
                let _ = writeln!(out, "  {} = call i64 @strlen(i8* {})", r, p);
                self.emit_write_abi_ret(out, tmp, arch, &r);
            }
            "strcmp" => {
                self.needed_decls
                    .insert("declare i32 @strcmp(i8*, i8*)".to_string());
                let p1 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let p2 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 1);
                let r32 = next_t(tmp);
                let r64 = next_t(tmp);
                let _ = writeln!(out, "  {} = call i32 @strcmp(i8* {}, i8* {})", r32, p1, p2);
                let _ = writeln!(out, "  {} = sext i32 {} to i64", r64, r32);
                self.emit_write_abi_ret(out, tmp, arch, &r64);
            }
            "strncmp" => {
                self.needed_decls
                    .insert("declare i32 @strncmp(i8*, i8*, i64)".to_string());
                let p1 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 0);
                let p2 = self.emit_arg_as_ptr(out, tmp, abi, mod_id, 1);
                let n = self.emit_read_abi_arg(out, tmp, abi, mod_id, 2);
                let r32 = next_t(tmp);
                let r64 = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i32 @strncmp(i8* {}, i8* {}, i64 {})",
                    r32, p1, p2, n
                );
                let _ = writeln!(out, "  {} = sext i32 {} to i64", r64, r32);
                self.emit_write_abi_ret(out, tmp, arch, &r64);
            }
            "abort"
            | "__assert_rtn"
            | "__assert_fail"
            | "_assert"
            | "__stack_chk_fail"
            | "TerminateProcess"
            | "RaiseException"
            | "__fastfail"
            | "_amsg_exit"
            | "__report_gsfailure"
            | "quick_exit" => {
                self.needed_decls
                    .insert("declare void @abort()".to_string());
                let _ = writeln!(out, "  call void @abort()");
            }
            "exit" | "_exit" | "_Exit" => {
                self.needed_decls
                    .insert("declare void @exit(i32)".to_string());
                let c64 = self.emit_read_abi_arg(out, tmp, abi, mod_id, 0);
                let c32 = next_t(tmp);
                let _ = writeln!(out, "  {} = trunc i64 {} to i32", c32, c64);
                let _ = writeln!(out, "  call void @exit(i32 {})", c32);
            }
            "GetCurrentProcess" => {
                self.emit_write_abi_ret(out, tmp, arch, "-1");
            }
            "GetCurrentThread" => {
                self.emit_write_abi_ret(out, tmp, arch, "-2");
            }
            "IsProcessorFeaturePresent"
            | "UnhandledExceptionFilter"
            | "SetUnhandledExceptionFilter"
            | "RtlCaptureContext"
            | "CaptureContext"
            | "RtlLookupFunctionEntry"
            | "RtlVirtualUnwind"
            | "__C_specific_handler"
            | "_gnu_exception_handler"
            | "GetCurrentProcessId"
            | "GetCurrentThreadId"
            | "GetTickCount"
            | "GetTickCount64"
            | "QueryPerformanceCounter"
            | "GetSystemTimeAsFileTime"
            | "GetLastError"
            | "SetLastError"
            | "DisableThreadLibraryCalls"
            | "InitializeCriticalSection"
            | "DeleteCriticalSection"
            | "EnterCriticalSection"
            | "LeaveCriticalSection" => {
                self.emit_write_abi_ret(out, tmp, arch, "0");
            }
            other => {
                // Generic external C boundary function: sanitize symbol identifier and pass 6 ABI args!
                let clean: String = other
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if clean.is_empty() {
                    return;
                }
                self.needed_decls
                    .insert(format!("declare i64 @{}(...)", clean));
                let mut args_ir = Vec::with_capacity(6);
                for idx in 0..6 {
                    let raw = self.emit_read_abi_arg(out, tmp, abi, mod_id, idx);
                    let norm = next_t(tmp);
                    let _ = writeln!(
                        out,
                        "  {} = call i64 @__omni_norm_ret_ptr_{}(i64 {})",
                        norm, mod_id, raw
                    );
                    args_ir.push(format!("i64 {}", norm));
                }
                let r = next_t(tmp);
                let _ = writeln!(
                    out,
                    "  {} = call i64 @{}({})",
                    r,
                    clean,
                    args_ir.join(", ")
                );
                self.emit_write_abi_ret(out, tmp, arch, &r);
            }
        }
    }
}

#[inline]
pub fn next_t(tmp: &mut usize) -> String {
    *tmp += 1;
    format!("%t{}", *tmp)
}
