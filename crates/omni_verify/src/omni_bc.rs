//! `OmniBC` — Pure-Rust LLVM SSA IR -> Register Bytecode Compiler, `.obc` Serializer,
//! Clang/SIMD Shim Layer, and Polyglot Register-VM Interpreter.
//!
//! Tier-0 execution engine alongside Tier-1 Cranelift (`cranelift_backend.rs`) and
//! Tier-1-MCU Cortex-M (`cortex_m/`):
//! - Compiles both `omni_codegen` SSA IR and Clang-emitted `.ll` (including opaque `ptr`,
//!   `llvm.memcpy`/`memset`, and `<4 x float>` / `<4 x i32>` SIMD vectors) to flat register bytecode.
//! - Deterministic `.obc` binary format (`OBC1` magic) for portable caching & Stage-1 self-hosting.
//! - Direct polyglot FFI bridge into `omni_runtime::jit_runtime_symbols()` and host C functions.

use std::collections::{BTreeMap, HashMap};

use crate::VerifyError;

// ---------------------------------------------------------------------------
// 1. Compact Register Bytecode ISA (`OmniBC`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntCmpOp {
    Eq = 0,
    Ne = 1,
    Slt = 2,
    Sle = 3,
    Sgt = 4,
    Sge = 5,
    Ult = 6,
    Ule = 7,
    Ugt = 8,
    Uge = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FloatCmpOp {
    Oeq = 0,
    One = 1,
    Olt = 2,
    Ole = 3,
    Ogt = 4,
    Oge = 5,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BcOp {
    Nop,
    LoadImm { dst: u16, val: u64 },
    LoadGlobalPtr { dst: u16, global_idx: u32 },
    LoadFnPtr { dst: u16, fn_idx: u32 },
    Mov { dst: u16, src: u16 },

    // Stack & Memory
    Alloca { dst: u16, size: u32 },
    Load { dst: u16, ptr: u16, width: u8 },
    Store { ptr: u16, val: u16, width: u8 },
    LoadVec128 { dst: u16, ptr: u16 },
    StoreVec128 { ptr: u16, src: u16 },
    MemCpy { dst: u16, src: u16, len: u16 },
    MemSet { dst: u16, byte_val: u16, len: u16 },

    // Scalar Integer ALU
    Add { dst: u16, a: u16, b: u16 },
    Sub { dst: u16, a: u16, b: u16 },
    Mul { dst: u16, a: u16, b: u16 },
    SDiv { dst: u16, a: u16, b: u16 },
    UDiv { dst: u16, a: u16, b: u16 },
    SRem { dst: u16, a: u16, b: u16 },
    URem { dst: u16, a: u16, b: u16 },
    And { dst: u16, a: u16, b: u16 },
    Or { dst: u16, a: u16, b: u16 },
    Xor { dst: u16, a: u16, b: u16 },
    Shl { dst: u16, a: u16, b: u16 },
    AShr { dst: u16, a: u16, b: u16 },
    LShr { dst: u16, a: u16, b: u16 },
    AddImm { dst: u16, a: u16, imm: i64 },
    MulImm { dst: u16, a: u16, imm: i64 },
    ICmp { dst: u16, cc: IntCmpOp, a: u16, b: u16 },
    Select { dst: u16, cond: u16, a: u16, b: u16 },

    // Scalar Float64 ALU
    FAdd { dst: u16, a: u16, b: u16 },
    FSub { dst: u16, a: u16, b: u16 },
    FMul { dst: u16, a: u16, b: u16 },
    FDiv { dst: u16, a: u16, b: u16 },
    FNeg { dst: u16, src: u16 },
    FSqrt { dst: u16, src: u16 },
    FAbs { dst: u16, src: u16 },
    FPow { dst: u16, a: u16, b: u16 },
    FFloor { dst: u16, src: u16 },
    FCeil { dst: u16, src: u16 },
    FCmp { dst: u16, cc: FloatCmpOp, a: u16, b: u16 },

    // Casts & Bit Ops
    SExt { dst: u16, src: u16, from_bits: u8 },
    ZExt { dst: u16, src: u16, from_bits: u8 },
    Trunc { dst: u16, src: u16, to_bits: u8 },
    SIToFP { dst: u16, src: u16 },
    UIToFP { dst: u16, src: u16 },
    FPToSI { dst: u16, src: u16 },
    FPToUI { dst: u16, src: u16 },
    F32ToF64 { dst: u16, src: u16 },
    F64ToF32 { dst: u16, src: u16 },
    BSwap64 { dst: u16, src: u16 },
    Ctlz64 { dst: u16, src: u16 },

    // 128-bit SIMD Vector ALU (`<4 x float>`, `<4 x i32>`, `<2 x double>`)
    VecFAdd4 { dst: u16, a: u16, b: u16 },
    VecFSub4 { dst: u16, a: u16, b: u16 },
    VecFMul4 { dst: u16, a: u16, b: u16 },
    VecFDiv4 { dst: u16, a: u16, b: u16 },
    VecFAdd2D { dst: u16, a: u16, b: u16 },
    VecFSub2D { dst: u16, a: u16, b: u16 },
    VecFMul2D { dst: u16, a: u16, b: u16 },
    VecFDiv2D { dst: u16, a: u16, b: u16 },
    VecIAdd4 { dst: u16, a: u16, b: u16 },
    VecIMul4 { dst: u16, a: u16, b: u16 },
    VecInsertF32 { dst: u16, vec: u16, val_f64: u16, lane: u8 },
    VecExtractF32 { dst_f64: u16, vec: u16, lane: u8 },
    VecInsertF64 { dst: u16, vec: u16, val_f64: u16, lane: u8 },
    VecExtractF64 { dst_f64: u16, vec: u16, lane: u8 },
    VecInsertI32 { dst: u16, vec: u16, val_i64: u16, lane: u8 },
    VecExtractI32 { dst_i64: u16, vec: u16, lane: u8 },
    VecShuffle4 { dst: u16, a: u16, b: u16, mask: [u8; 4] },
    FFma { dst: u16, a: u16, b: u16, c: u16 },
    Cttz64 { dst: u16, src: u16 },

    // Control Flow
    Jmp { target: u32 },
    BrIf { cond: u16, true_target: u32, false_target: u32 },
    Switch { val: u16, default_target: u32, cases: Vec<(u64, u32)> },
    Ret { val: Option<u16> },
    Trap,

    // Calls
    CallInternal { dst: Option<u16>, fn_idx: u32, args: Vec<u16> },
    CallExtern { dst: Option<u16>, ext_idx: u32, args: Vec<u16>, arg_is_f64: Vec<bool>, ret_is_f64: bool },
    CallIndirect { dst: Option<u16>, fn_ptr: u16, args: Vec<u16> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BcGlobal {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BcExtern {
    pub name: String,
    pub ret_is_f64: bool,
    pub arg_is_f64: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BcFunction {
    pub name: String,
    pub num_params: u16,
    pub num_regs: u16,
    pub stack_bytes: u32,
    pub ops: Vec<BcOp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OmniBcModule {
    pub globals: Vec<BcGlobal>,
    pub externs: Vec<BcExtern>,
    pub functions: Vec<BcFunction>,
}

// ---------------------------------------------------------------------------
// 2. Deterministic `.obc` Binary Serializer & Deserializer (`OBC1` magic)
// ---------------------------------------------------------------------------

impl OmniBcModule {
    const MAGIC: &'static [u8; 4] = b"OBC1";

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(Self::MAGIC);
        write_u32(&mut out, self.globals.len() as u32);
        for g in &self.globals {
            write_str(&mut out, &g.name);
            write_u32(&mut out, g.bytes.len() as u32);
            out.extend_from_slice(&g.bytes);
        }
        write_u32(&mut out, self.externs.len() as u32);
        for e in &self.externs {
            write_str(&mut out, &e.name);
            out.push(e.ret_is_f64 as u8);
            write_u32(&mut out, e.arg_is_f64.len() as u32);
            for &b in &e.arg_is_f64 {
                out.push(b as u8);
            }
        }
        write_u32(&mut out, self.functions.len() as u32);
        for f in &self.functions {
            write_str(&mut out, &f.name);
            write_u16(&mut out, f.num_params);
            write_u16(&mut out, f.num_regs);
            write_u32(&mut out, f.stack_bytes);
            write_u32(&mut out, f.ops.len() as u32);
            for op in &f.ops {
                encode_op(&mut out, op);
            }
        }
        out
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, VerifyError> {
        if buf.len() < 4 || &buf[0..4] != Self::MAGIC {
            return Err(VerifyError::LinkError("Invalid OmniBC (.obc) magic".into()));
        }
        let mut pos = 4usize;
        let ng = read_u32(buf, &mut pos)? as usize;
        let mut globals = Vec::with_capacity(ng);
        for _ in 0..ng {
            let name = read_str(buf, &mut pos)?;
            let blen = read_u32(buf, &mut pos)? as usize;
            if pos + blen > buf.len() {
                return Err(VerifyError::LinkError("Truncated .obc global".into()));
            }
            let bytes = buf[pos..pos + blen].to_vec();
            pos += blen;
            globals.push(BcGlobal { name, bytes });
        }
        let ne = read_u32(buf, &mut pos)? as usize;
        let mut externs = Vec::with_capacity(ne);
        for _ in 0..ne {
            let name = read_str(buf, &mut pos)?;
            let ret_is_f64 = read_u8(buf, &mut pos)? != 0;
            let na = read_u32(buf, &mut pos)? as usize;
            let mut arg_is_f64 = Vec::with_capacity(na);
            for _ in 0..na {
                arg_is_f64.push(read_u8(buf, &mut pos)? != 0);
            }
            externs.push(BcExtern { name, ret_is_f64, arg_is_f64 });
        }
        let nf = read_u32(buf, &mut pos)? as usize;
        let mut functions = Vec::with_capacity(nf);
        for _ in 0..nf {
            let name = read_str(buf, &mut pos)?;
            let num_params = read_u16(buf, &mut pos)?;
            let num_regs = read_u16(buf, &mut pos)?;
            let stack_bytes = read_u32(buf, &mut pos)?;
            let nops = read_u32(buf, &mut pos)? as usize;
            let mut ops = Vec::with_capacity(nops);
            for _ in 0..nops {
                ops.push(decode_op(buf, &mut pos)?);
            }
            functions.push(BcFunction { name, num_params, num_regs, stack_bytes, ops });
        }
        Ok(Self { globals, externs, functions })
    }
}

fn write_u16(o: &mut Vec<u8>, v: u16) { o.extend_from_slice(&v.to_le_bytes()); }
fn write_u32(o: &mut Vec<u8>, v: u32) { o.extend_from_slice(&v.to_le_bytes()); }
fn write_u64(o: &mut Vec<u8>, v: u64) { o.extend_from_slice(&v.to_le_bytes()); }
fn write_str(o: &mut Vec<u8>, s: &str) { write_u32(o, s.len() as u32); o.extend_from_slice(s.as_bytes()); }

fn read_u8(b: &[u8], p: &mut usize) -> Result<u8, VerifyError> {
    if *p + 1 > b.len() { return Err(VerifyError::LinkError("Truncated .obc".into())); }
    let v = b[*p]; *p += 1; Ok(v)
}
fn read_u16(b: &[u8], p: &mut usize) -> Result<u16, VerifyError> {
    if *p + 2 > b.len() { return Err(VerifyError::LinkError("Truncated .obc".into())); }
    let v = u16::from_le_bytes([b[*p], b[*p + 1]]); *p += 2; Ok(v)
}
fn read_u32(b: &[u8], p: &mut usize) -> Result<u32, VerifyError> {
    if *p + 4 > b.len() { return Err(VerifyError::LinkError("Truncated .obc".into())); }
    let v = u32::from_le_bytes(b[*p..*p + 4].try_into().unwrap()); *p += 4; Ok(v)
}
fn read_u64(b: &[u8], p: &mut usize) -> Result<u64, VerifyError> {
    if *p + 8 > b.len() { return Err(VerifyError::LinkError("Truncated .obc".into())); }
    let v = u64::from_le_bytes(b[*p..*p + 8].try_into().unwrap()); *p += 8; Ok(v)
}
fn read_str(b: &[u8], p: &mut usize) -> Result<String, VerifyError> {
    let n = read_u32(b, p)? as usize;
    if *p + n > b.len() { return Err(VerifyError::LinkError("Truncated .obc str".into())); }
    let s = String::from_utf8_lossy(&b[*p..*p + n]).into_owned(); *p += n; Ok(s)
}

fn encode_op(o: &mut Vec<u8>, op: &BcOp) {
    match op {
        BcOp::Nop => o.push(0),
        BcOp::LoadImm { dst, val } => { o.push(1); write_u16(o, *dst); write_u64(o, *val); }
        BcOp::LoadGlobalPtr { dst, global_idx } => { o.push(2); write_u16(o, *dst); write_u32(o, *global_idx); }
        BcOp::LoadFnPtr { dst, fn_idx } => { o.push(3); write_u16(o, *dst); write_u32(o, *fn_idx); }
        BcOp::Mov { dst, src } => { o.push(4); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::Alloca { dst, size } => { o.push(5); write_u16(o, *dst); write_u32(o, *size); }
        BcOp::Load { dst, ptr, width } => { o.push(6); write_u16(o, *dst); write_u16(o, *ptr); o.push(*width); }
        BcOp::Store { ptr, val, width } => { o.push(7); write_u16(o, *ptr); write_u16(o, *val); o.push(*width); }
        BcOp::LoadVec128 { dst, ptr } => { o.push(8); write_u16(o, *dst); write_u16(o, *ptr); }
        BcOp::StoreVec128 { ptr, src } => { o.push(9); write_u16(o, *ptr); write_u16(o, *src); }
        BcOp::MemCpy { dst, src, len } => { o.push(10); write_u16(o, *dst); write_u16(o, *src); write_u16(o, *len); }
        BcOp::MemSet { dst, byte_val, len } => { o.push(11); write_u16(o, *dst); write_u16(o, *byte_val); write_u16(o, *len); }
        BcOp::Add { dst, a, b } => { o.push(12); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Sub { dst, a, b } => { o.push(13); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Mul { dst, a, b } => { o.push(14); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::SDiv { dst, a, b } => { o.push(15); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::UDiv { dst, a, b } => { o.push(16); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::SRem { dst, a, b } => { o.push(17); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::URem { dst, a, b } => { o.push(18); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::And { dst, a, b } => { o.push(19); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Or { dst, a, b } => { o.push(20); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Xor { dst, a, b } => { o.push(21); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Shl { dst, a, b } => { o.push(22); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::AShr { dst, a, b } => { o.push(23); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::LShr { dst, a, b } => { o.push(24); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::AddImm { dst, a, imm } => { o.push(25); write_u16(o, *dst); write_u16(o, *a); write_u64(o, *imm as u64); }
        BcOp::MulImm { dst, a, imm } => { o.push(26); write_u16(o, *dst); write_u16(o, *a); write_u64(o, *imm as u64); }
        BcOp::ICmp { dst, cc, a, b } => { o.push(27); write_u16(o, *dst); o.push(*cc as u8); write_u16(o, *a); write_u16(o, *b); }
        BcOp::Select { dst, cond, a, b } => { o.push(28); write_u16(o, *dst); write_u16(o, *cond); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FAdd { dst, a, b } => { o.push(29); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FSub { dst, a, b } => { o.push(30); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FMul { dst, a, b } => { o.push(31); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FDiv { dst, a, b } => { o.push(32); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FNeg { dst, src } => { o.push(33); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FSqrt { dst, src } => { o.push(34); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FAbs { dst, src } => { o.push(35); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FPow { dst, a, b } => { o.push(36); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::FFloor { dst, src } => { o.push(37); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FCeil { dst, src } => { o.push(38); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FCmp { dst, cc, a, b } => { o.push(39); write_u16(o, *dst); o.push(*cc as u8); write_u16(o, *a); write_u16(o, *b); }
        BcOp::SExt { dst, src, from_bits } => { o.push(40); write_u16(o, *dst); write_u16(o, *src); o.push(*from_bits); }
        BcOp::ZExt { dst, src, from_bits } => { o.push(41); write_u16(o, *dst); write_u16(o, *src); o.push(*from_bits); }
        BcOp::Trunc { dst, src, to_bits } => { o.push(42); write_u16(o, *dst); write_u16(o, *src); o.push(*to_bits); }
        BcOp::SIToFP { dst, src } => { o.push(43); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::UIToFP { dst, src } => { o.push(44); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FPToSI { dst, src } => { o.push(45); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::FPToUI { dst, src } => { o.push(46); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::F32ToF64 { dst, src } => { o.push(47); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::F64ToF32 { dst, src } => { o.push(48); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::BSwap64 { dst, src } => { o.push(49); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::Ctlz64 { dst, src } => { o.push(50); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::VecFAdd4 { dst, a, b } => { o.push(51); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFSub4 { dst, a, b } => { o.push(52); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFMul4 { dst, a, b } => { o.push(53); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFDiv4 { dst, a, b } => { o.push(54); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecIAdd4 { dst, a, b } => { o.push(55); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecIMul4 { dst, a, b } => { o.push(56); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecInsertF32 { dst, vec, val_f64, lane } => { o.push(57); write_u16(o, *dst); write_u16(o, *vec); write_u16(o, *val_f64); o.push(*lane); }
        BcOp::VecExtractF32 { dst_f64, vec, lane } => { o.push(58); write_u16(o, *dst_f64); write_u16(o, *vec); o.push(*lane); }
        BcOp::VecInsertI32 { dst, vec, val_i64, lane } => { o.push(59); write_u16(o, *dst); write_u16(o, *vec); write_u16(o, *val_i64); o.push(*lane); }
        BcOp::VecExtractI32 { dst_i64, vec, lane } => { o.push(60); write_u16(o, *dst_i64); write_u16(o, *vec); o.push(*lane); }
        BcOp::VecFAdd2D { dst, a, b } => { o.push(69); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFSub2D { dst, a, b } => { o.push(70); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFMul2D { dst, a, b } => { o.push(71); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecFDiv2D { dst, a, b } => { o.push(72); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); }
        BcOp::VecInsertF64 { dst, vec, val_f64, lane } => { o.push(73); write_u16(o, *dst); write_u16(o, *vec); write_u16(o, *val_f64); o.push(*lane); }
        BcOp::VecExtractF64 { dst_f64, vec, lane } => { o.push(74); write_u16(o, *dst_f64); write_u16(o, *vec); o.push(*lane); }
        BcOp::VecShuffle4 { dst, a, b, mask } => { o.push(75); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); o.extend_from_slice(mask); }
        BcOp::FFma { dst, a, b, c } => { o.push(76); write_u16(o, *dst); write_u16(o, *a); write_u16(o, *b); write_u16(o, *c); }
        BcOp::Cttz64 { dst, src } => { o.push(77); write_u16(o, *dst); write_u16(o, *src); }
        BcOp::Jmp { target } => { o.push(61); write_u32(o, *target); }
        BcOp::BrIf { cond, true_target, false_target } => { o.push(62); write_u16(o, *cond); write_u32(o, *true_target); write_u32(o, *false_target); }
        BcOp::Switch { val, default_target, cases } => {
            o.push(63); write_u16(o, *val); write_u32(o, *default_target);
            write_u32(o, cases.len() as u32);
            for &(cv, tgt) in cases { write_u64(o, cv); write_u32(o, tgt); }
        }
        BcOp::Ret { val } => {
            o.push(64);
            match val {
                Some(r) => { o.push(1); write_u16(o, *r); }
                None => o.push(0),
            }
        }
        BcOp::Trap => o.push(65),
        BcOp::CallInternal { dst, fn_idx, args } => {
            o.push(66);
            match dst { Some(r) => { o.push(1); write_u16(o, *r); } None => o.push(0) }
            write_u32(o, *fn_idx);
            write_u16(o, args.len() as u16);
            for &a in args { write_u16(o, a); }
        }
        BcOp::CallExtern { dst, ext_idx, args, arg_is_f64, ret_is_f64 } => {
            o.push(67);
            match dst { Some(r) => { o.push(1); write_u16(o, *r); } None => o.push(0) }
            write_u32(o, *ext_idx);
            o.push(*ret_is_f64 as u8);
            write_u16(o, args.len() as u16);
            for (&a, &isf) in args.iter().zip(arg_is_f64.iter()) {
                write_u16(o, a);
                o.push(isf as u8);
            }
        }
        BcOp::CallIndirect { dst, fn_ptr, args } => {
            o.push(68);
            match dst { Some(r) => { o.push(1); write_u16(o, *r); } None => o.push(0) }
            write_u16(o, *fn_ptr);
            write_u16(o, args.len() as u16);
            for &a in args { write_u16(o, a); }
        }
    }
}

fn decode_op(b: &[u8], p: &mut usize) -> Result<BcOp, VerifyError> {
    let tag = read_u8(b, p)?;
    Ok(match tag {
        0 => BcOp::Nop,
        1 => BcOp::LoadImm { dst: read_u16(b, p)?, val: read_u64(b, p)? },
        2 => BcOp::LoadGlobalPtr { dst: read_u16(b, p)?, global_idx: read_u32(b, p)? },
        3 => BcOp::LoadFnPtr { dst: read_u16(b, p)?, fn_idx: read_u32(b, p)? },
        4 => BcOp::Mov { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        5 => BcOp::Alloca { dst: read_u16(b, p)?, size: read_u32(b, p)? },
        6 => BcOp::Load { dst: read_u16(b, p)?, ptr: read_u16(b, p)?, width: read_u8(b, p)? },
        7 => BcOp::Store { ptr: read_u16(b, p)?, val: read_u16(b, p)?, width: read_u8(b, p)? },
        8 => BcOp::LoadVec128 { dst: read_u16(b, p)?, ptr: read_u16(b, p)? },
        9 => BcOp::StoreVec128 { ptr: read_u16(b, p)?, src: read_u16(b, p)? },
        10 => BcOp::MemCpy { dst: read_u16(b, p)?, src: read_u16(b, p)?, len: read_u16(b, p)? },
        11 => BcOp::MemSet { dst: read_u16(b, p)?, byte_val: read_u16(b, p)?, len: read_u16(b, p)? },
        12 => BcOp::Add { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        13 => BcOp::Sub { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        14 => BcOp::Mul { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        15 => BcOp::SDiv { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        16 => BcOp::UDiv { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        17 => BcOp::SRem { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        18 => BcOp::URem { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        19 => BcOp::And { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        20 => BcOp::Or { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        21 => BcOp::Xor { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        22 => BcOp::Shl { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        23 => BcOp::AShr { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        24 => BcOp::LShr { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        25 => BcOp::AddImm { dst: read_u16(b, p)?, a: read_u16(b, p)?, imm: read_u64(b, p)? as i64 },
        26 => BcOp::MulImm { dst: read_u16(b, p)?, a: read_u16(b, p)?, imm: read_u64(b, p)? as i64 },
        27 => {
            let dst = read_u16(b, p)?;
            let cc = match read_u8(b, p)? {
                0 => IntCmpOp::Eq, 1 => IntCmpOp::Ne, 2 => IntCmpOp::Slt, 3 => IntCmpOp::Sle,
                4 => IntCmpOp::Sgt, 5 => IntCmpOp::Sge, 6 => IntCmpOp::Ult, 7 => IntCmpOp::Ule,
                8 => IntCmpOp::Ugt, _ => IntCmpOp::Uge,
            };
            BcOp::ICmp { dst, cc, a: read_u16(b, p)?, b: read_u16(b, p)? }
        }
        28 => BcOp::Select { dst: read_u16(b, p)?, cond: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        29 => BcOp::FAdd { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        30 => BcOp::FSub { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        31 => BcOp::FMul { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        32 => BcOp::FDiv { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        33 => BcOp::FNeg { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        34 => BcOp::FSqrt { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        35 => BcOp::FAbs { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        36 => BcOp::FPow { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        37 => BcOp::FFloor { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        38 => BcOp::FCeil { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        39 => {
            let dst = read_u16(b, p)?;
            let cc = match read_u8(b, p)? {
                0 => FloatCmpOp::Oeq, 1 => FloatCmpOp::One, 2 => FloatCmpOp::Olt,
                3 => FloatCmpOp::Ole, 4 => FloatCmpOp::Ogt, _ => FloatCmpOp::Oge,
            };
            BcOp::FCmp { dst, cc, a: read_u16(b, p)?, b: read_u16(b, p)? }
        }
        40 => BcOp::SExt { dst: read_u16(b, p)?, src: read_u16(b, p)?, from_bits: read_u8(b, p)? },
        41 => BcOp::ZExt { dst: read_u16(b, p)?, src: read_u16(b, p)?, from_bits: read_u8(b, p)? },
        42 => BcOp::Trunc { dst: read_u16(b, p)?, src: read_u16(b, p)?, to_bits: read_u8(b, p)? },
        43 => BcOp::SIToFP { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        44 => BcOp::UIToFP { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        45 => BcOp::FPToSI { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        46 => BcOp::FPToUI { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        47 => BcOp::F32ToF64 { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        48 => BcOp::F64ToF32 { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        49 => BcOp::BSwap64 { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        50 => BcOp::Ctlz64 { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        51 => BcOp::VecFAdd4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        52 => BcOp::VecFSub4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        53 => BcOp::VecFMul4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        54 => BcOp::VecFDiv4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        55 => BcOp::VecIAdd4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        56 => BcOp::VecIMul4 { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        57 => BcOp::VecInsertF32 { dst: read_u16(b, p)?, vec: read_u16(b, p)?, val_f64: read_u16(b, p)?, lane: read_u8(b, p)? },
        58 => BcOp::VecExtractF32 { dst_f64: read_u16(b, p)?, vec: read_u16(b, p)?, lane: read_u8(b, p)? },
        59 => BcOp::VecInsertI32 { dst: read_u16(b, p)?, vec: read_u16(b, p)?, val_i64: read_u16(b, p)?, lane: read_u8(b, p)? },
        60 => BcOp::VecExtractI32 { dst_i64: read_u16(b, p)?, vec: read_u16(b, p)?, lane: read_u8(b, p)? },
        61 => BcOp::Jmp { target: read_u32(b, p)? },
        62 => BcOp::BrIf { cond: read_u16(b, p)?, true_target: read_u32(b, p)?, false_target: read_u32(b, p)? },
        63 => {
            let val = read_u16(b, p)?;
            let default_target = read_u32(b, p)?;
            let nc = read_u32(b, p)? as usize;
            let mut cases = Vec::with_capacity(nc);
            for _ in 0..nc { cases.push((read_u64(b, p)?, read_u32(b, p)?)); }
            BcOp::Switch { val, default_target, cases }
        }
        64 => {
            let has = read_u8(b, p)? != 0;
            BcOp::Ret { val: if has { Some(read_u16(b, p)?) } else { None } }
        }
        65 => BcOp::Trap,
        66 => {
            let dst = if read_u8(b, p)? != 0 { Some(read_u16(b, p)?) } else { None };
            let fn_idx = read_u32(b, p)?;
            let na = read_u16(b, p)? as usize;
            let mut args = Vec::with_capacity(na);
            for _ in 0..na { args.push(read_u16(b, p)?); }
            BcOp::CallInternal { dst, fn_idx, args }
        }
        67 => {
            let dst = if read_u8(b, p)? != 0 { Some(read_u16(b, p)?) } else { None };
            let ext_idx = read_u32(b, p)?;
            let ret_is_f64 = read_u8(b, p)? != 0;
            let na = read_u16(b, p)? as usize;
            let mut args = Vec::with_capacity(na);
            let mut arg_is_f64 = Vec::with_capacity(na);
            for _ in 0..na {
                args.push(read_u16(b, p)?);
                arg_is_f64.push(read_u8(b, p)? != 0);
            }
            BcOp::CallExtern { dst, ext_idx, args, arg_is_f64, ret_is_f64 }
        }
        68 => {
            let dst = if read_u8(b, p)? != 0 { Some(read_u16(b, p)?) } else { None };
            let fn_ptr = read_u16(b, p)?;
            let na = read_u16(b, p)? as usize;
            let mut args = Vec::with_capacity(na);
            for _ in 0..na { args.push(read_u16(b, p)?); }
            BcOp::CallIndirect { dst, fn_ptr, args }
        }
        69 => BcOp::VecFAdd2D { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        70 => BcOp::VecFSub2D { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        71 => BcOp::VecFMul2D { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        72 => BcOp::VecFDiv2D { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)? },
        73 => BcOp::VecInsertF64 { dst: read_u16(b, p)?, vec: read_u16(b, p)?, val_f64: read_u16(b, p)?, lane: read_u8(b, p)? },
        74 => BcOp::VecExtractF64 { dst_f64: read_u16(b, p)?, vec: read_u16(b, p)?, lane: read_u8(b, p)? },
        75 => {
            let dst = read_u16(b, p)?; let a = read_u16(b, p)?; let b_r = read_u16(b, p)?;
            let mask = [read_u8(b, p)?, read_u8(b, p)?, read_u8(b, p)?, read_u8(b, p)?];
            BcOp::VecShuffle4 { dst, a, b: b_r, mask }
        }
        76 => BcOp::FFma { dst: read_u16(b, p)?, a: read_u16(b, p)?, b: read_u16(b, p)?, c: read_u16(b, p)? },
        77 => BcOp::Cttz64 { dst: read_u16(b, p)?, src: read_u16(b, p)? },
        other => return Err(VerifyError::LinkError(format!("Unknown .obc opcode {}", other))),
    })
}

// ---------------------------------------------------------------------------
// 3. LLVM SSA IR (OmniScript + Clang `.ll`) -> `OmniBC` Compiler
// ---------------------------------------------------------------------------

fn bc_err(line: usize, msg: impl Into<String>) -> VerifyError {
    let m = msg.into();
    VerifyError::SyntaxError {
        file: "<omni-bc>".to_string(),
        line,
        column: 1,
        message: m.clone(),
        raw_stderr: m,
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if !in_str && bytes[i] == b';' {
            return &line[..i];
        }
    }
    line
}

fn split_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut dp, mut db, mut dc, mut da) = (0i32, 0i32, 0i32, 0i32);
    let mut in_str = false;
    let mut start = 0usize;
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        } else if !in_str {
            match b {
                b'(' => dp += 1, b')' => dp -= 1,
                b'[' => db += 1, b']' => db -= 1,
                b'{' => dc += 1, b'}' => dc -= 1,
                b'<' => da += 1, b'>' => da -= 1,
                b',' if dp == 0 && db == 0 && dc == 0 && da == 0 => {
                    out.push(s[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    let tail = s[start..].trim();
    if !tail.is_empty() { out.push(tail); }
    out
}

fn decode_c_str(raw: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = ((bytes[i + 1] as char).to_digit(16), (bytes[i + 2] as char).to_digit(16)) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Strip Clang parameter/return attributes (`noundef`, `signext`, `zeroext`, `nocapture`, `align 8`, `nonnull`, etc.)
fn strip_clang_attrs(s: &str) -> String {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if matches!(
            t,
            "noundef" | "signext" | "zeroext" | "nocapture" | "readonly" | "writeonly"
                | "nonnull" | "noalias" | "returned" | "inreg" | "dso_local" | "local_unnamed_addr"
                | "unnamed_addr" | "internal" | "private" | "weak" | "linkonce_odr" | "tail" | "musttail"
        ) {
            i += 1;
            continue;
        }
        if matches!(t, "align" | "dereferenceable" | "byval") {
            i += 2;
            continue;
        }
        if t.starts_with('#') {
            i += 1;
            continue;
        }
        out.push(t);
        i += 1;
    }
    out.join(" ")
}

fn type_width_bytes(ty: &str, structs: &BTreeMap<String, (Vec<i32>, u32)>) -> u32 {
    let t = strip_clang_attrs(ty);
    let t = t.trim();
    if t.ends_with('*') || t == "ptr" { return 8; }
    if t.starts_with("<4 x ") || t.starts_with("<2 x double") || t.starts_with("<2 x i64") { return 16; }
    if let Some(sn) = t.strip_prefix("%struct.") {
        return structs.get(sn).map(|s| s.1).unwrap_or(64);
    }
    if t.starts_with('[') && t.ends_with(']') {
        if let Some((cnt, elem)) = t[1..t.len() - 1].split_once('x') {
            if let Ok(c) = cnt.trim().parse::<u32>() {
                return c.saturating_mul(type_width_bytes(elem.trim(), structs)).max(1);
            }
        }
    }
    match t {
        "i1" | "i8" => 1,
        "i16" => 2,
        "i32" | "float" => 4,
        "i64" | "double" => 8,
        _ => 8,
    }
}

fn type_bits(ty: &str) -> u8 {
    match strip_clang_attrs(ty).trim() {
        "i1" => 1, "i8" => 8, "i16" => 16, "i32" => 32, _ => 64,
    }
}

fn split_ty_op(s: &str) -> Option<(String, String)> {
    let cleaned = strip_clang_attrs(s);
    let t = cleaned.trim();
    for kw in [" getelementptr ", " bitcast (", " ptrtoint ("] {
        if let Some(pos) = t.find(kw) {
            return Some((t[..pos].trim().to_string(), t[pos + 1..].trim().to_string()));
        }
    }
    // Handle vector types `<4 x float> %v`
    if t.starts_with('<') {
        if let Some(gt) = t.find('>') {
            return Some((t[..=gt].trim().to_string(), t[gt + 1..].trim().to_string()));
        }
    }
    let (ty, op) = t.rsplit_once(' ')?;
    Some((ty.trim().to_string(), op.trim().to_string()))
}

struct FnBuilder<'a> {
    global_map: &'a HashMap<String, u32>,
    fn_map: &'a HashMap<String, u32>,
    ext_map: &'a HashMap<String, (u32, bool, Vec<bool>)>,
    reg_map: HashMap<String, u16>,
    next_reg: u16,
    stack_bytes: u32,
    ops: Vec<BcOp>,
}

impl<'a> FnBuilder<'a> {
    fn alloc_tmp(&mut self) -> u16 {
        let r = self.next_reg;
        self.next_reg += 1;
        r
    }

    fn get_or_alloc_reg(&mut self, name: &str) -> u16 {
        if let Some(&r) = self.reg_map.get(name) {
            return r;
        }
        let r = self.alloc_tmp();
        self.reg_map.insert(name.to_string(), r);
        r
    }

    fn eval_op(&mut self, raw: &str, ty_hint: &str, line_no: usize) -> Result<u16, VerifyError> {
        let s = raw.trim();
        if s.starts_with("getelementptr ") {
            if let Some(at) = s.find('@') {
                let gname: String = s[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if let Some(&gidx) = self.global_map.get(&gname) {
                    let r = self.alloc_tmp();
                    self.ops.push(BcOp::LoadGlobalPtr { dst: r, global_idx: gidx });
                    return Ok(r);
                }
            }
            let r = self.alloc_tmp();
            self.ops.push(BcOp::LoadImm { dst: r, val: 0 });
            return Ok(r);
        }
        if (s.starts_with("bitcast (") || s.starts_with("ptrtoint (")) && s.contains('@') {
            if let Some(at) = s.find('@') {
                let sym: String = s[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if let Some(&fidx) = self.fn_map.get(&sym) {
                    let r = self.alloc_tmp();
                    self.ops.push(BcOp::LoadFnPtr { dst: r, fn_idx: fidx });
                    return Ok(r);
                }
                if let Some(&gidx) = self.global_map.get(&sym) {
                    let r = self.alloc_tmp();
                    self.ops.push(BcOp::LoadGlobalPtr { dst: r, global_idx: gidx });
                    return Ok(r);
                }
            }
        }
        if s.starts_with('%') {
            if let Some(&r) = self.reg_map.get(s) {
                return Ok(r);
            }
            return Err(bc_err(line_no, format!("use of undefined value '{}'", s)));
        }
        if let Some(sym) = s.strip_prefix('@') {
            if let Some(&gidx) = self.global_map.get(sym) {
                let r = self.alloc_tmp();
                self.ops.push(BcOp::LoadGlobalPtr { dst: r, global_idx: gidx });
                return Ok(r);
            }
            if let Some(&fidx) = self.fn_map.get(sym) {
                let r = self.alloc_tmp();
                self.ops.push(BcOp::LoadFnPtr { dst: r, fn_idx: fidx });
                return Ok(r);
            }
            return Err(bc_err(line_no, format!("Undefined symbol '@{}'", sym)));
        }
        let r = self.alloc_tmp();
        let val: u64 = match s {
            "null" | "undef" | "poison" | "zeroinitializer" | "false" => 0,
            "true" => 1,
            _ => {
                if ty_hint.trim() == "double" {
                    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                        u64::from_str_radix(hex, 16).unwrap_or(0)
                    } else {
                        s.parse::<f64>().map(|f| f.to_bits()).unwrap_or(0)
                    }
                } else if ty_hint.trim() == "float" {
                    s.parse::<f32>().map(|f| (f as f64).to_bits()).unwrap_or(0)
                } else if let Ok(iv) = s.parse::<i64>() {
                    iv as u64
                } else if let Ok(uv) = s.parse::<u64>() {
                    uv
                } else {
                    return Err(bc_err(line_no, format!("Cannot parse operand '{}'", s)));
                }
            }
        };
        self.ops.push(BcOp::LoadImm { dst: r, val });
        Ok(r)
    }
}

/// Compile LLVM SSA IR (from `omni_codegen` or Clang `-S -emit-llvm`) into `OmniBcModule`.
pub fn compile_llvm_ir_to_bc(ir: &str) -> Result<OmniBcModule, VerifyError> {
    let mut structs: BTreeMap<String, (Vec<i32>, u32)> = BTreeMap::new();
    let mut globals: Vec<BcGlobal> = Vec::new();
    let mut global_map: HashMap<String, u32> = HashMap::new();
    let mut externs: Vec<BcExtern> = Vec::new();
    let mut ext_map: HashMap<String, (u32, bool, Vec<bool>)> = HashMap::new();

    struct RawFn {
        name: String,
        params: Vec<(String, String)>,
        blocks: Vec<(String, Vec<(usize, String)>)>,
        start_line: usize,
    }
    let mut raw_fns: Vec<RawFn> = Vec::new();
    let mut fn_map: HashMap<String, u32> = HashMap::new();

    let lines: Vec<&str> = ir.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line_no = i + 1;
        let trimmed = strip_comment(lines[i]).trim();
        i += 1;
        if trimmed.is_empty()
            || trimmed.starts_with("source_filename")
            || trimmed.starts_with("target ")
            || trimmed.starts_with("attributes ")
            || trimmed.starts_with('!')
        {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("%struct.") {
            if let Some((sn, rhs)) = rest.split_once(" = type ") {
                let body = rhs.trim().trim_start_matches('{').trim_end_matches('}').trim();
                let mut offs = Vec::new();
                let mut off = 0u32;
                if !body.is_empty() {
                    for fty in split_commas(body) {
                        let sz = type_width_bytes(fty, &structs);
                        let al = sz.min(8).max(1);
                        off = (off + al - 1) & !(al - 1);
                        offs.push(off as i32);
                        off += sz;
                    }
                }
                let total = ((off + 7) & !7).max(8);
                structs.insert(sn.trim().to_string(), (offs, total));
            }
            continue;
        }

        if trimmed.starts_with('@') && (trimmed.contains(" constant ") || trimmed.contains(" global ")) {
            if let Some((lhs, rhs)) = trimmed.split_once('=') {
                let gname = lhs.trim().trim_start_matches('@').to_string();
                let idx = globals.len() as u32;
                if let Some(cpos) = rhs.find(" c\"") {
                    let after = &rhs[cpos + 3..];
                    if let Some(endq) = after.find('"') {
                        global_map.insert(gname.clone(), idx);
                        globals.push(BcGlobal { name: gname, bytes: decode_c_str(&after[..endq]) });
                    }
                } else {
                    let after_kw = rhs
                        .split_once(" global ")
                        .or_else(|| rhs.split_once(" constant "))
                        .map(|(_, r)| r.trim())
                        .unwrap_or("i64 0");
                    let head = after_kw.split(',').next().unwrap_or("i64 0").trim();
                    let (ty_s, init_s) = head.split_once(' ').unwrap_or((head, "0"));
                    let sz = type_width_bytes(ty_s.trim(), &structs).max(4) as usize;
                    let mut bytes = vec![0u8; sz];
                    if let Ok(iv) = init_s.trim().parse::<i64>() {
                        let le = iv.to_le_bytes();
                        let n = bytes.len().min(8);
                        bytes[..n].copy_from_slice(&le[..n]);
                    }
                    global_map.insert(gname.clone(), idx);
                    globals.push(BcGlobal { name: gname, bytes });
                }
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("declare ") {
            if let Some(at) = rest.find('@') {
                let ret_clean = strip_clang_attrs(&rest[..at]);
                let ret_is_f64 = ret_clean.trim() == "double" || ret_clean.trim() == "float";
                let after = &rest[at + 1..];
                if let (Some(lp), Some(rp)) = (after.find('('), after.rfind(')')) {
                    let name = after[..lp].trim().to_string();
                    let mut arg_is_f64 = Vec::new();
                    let pstr = after[lp + 1..rp].trim();
                    if !pstr.is_empty() {
                        for p in split_commas(pstr) {
                            if p != "..." {
                                let pc = strip_clang_attrs(p);
                                arg_is_f64.push(pc.starts_with("double") || pc.starts_with("float"));
                            }
                        }
                    }
                    if !ext_map.contains_key(&name) {
                        let idx = externs.len() as u32;
                        ext_map.insert(name.clone(), (idx, ret_is_f64, arg_is_f64.clone()));
                        externs.push(BcExtern { name, ret_is_f64, arg_is_f64 });
                    }
                }
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("define ") {
            let at = rest.find('@').ok_or_else(|| bc_err(line_no, format!("Malformed define: {}", trimmed)))?;
            let after = &rest[at + 1..];
            let lp = after.find('(').ok_or_else(|| bc_err(line_no, format!("Malformed define: {}", trimmed)))?;
            let rp = after.rfind(')').ok_or_else(|| bc_err(line_no, format!("Malformed define: {}", trimmed)))?;
            let fname = after[..lp].trim().to_string();
            let pstr = after[lp + 1..rp].trim();
            let mut params = Vec::new();
            if !pstr.is_empty() {
                for (pidx, p) in split_commas(pstr).into_iter().enumerate() {
                    let pc = strip_clang_attrs(p);
                    let toks: Vec<&str> = pc.split_whitespace().collect();
                    if let Some(last) = toks.last() {
                        if last.starts_with('%') {
                            params.push((toks[..toks.len() - 1].join(" "), (*last).to_string()));
                        } else {
                            params.push((pc, format!("%{}", pidx)));
                        }
                    }
                }
            }
            let mut blocks: Vec<(String, Vec<(usize, String)>)> = Vec::new();
            let mut cur_lbl = "entry".to_string();
            let mut cur_lines: Vec<(usize, String)> = Vec::new();
            let mut started_explicit = false;
            while i < lines.len() {
                let blno = i + 1;
                let bt = strip_comment(lines[i]).trim();
                i += 1;
                if bt.is_empty() { continue; }
                if bt == "}" {
                    if !cur_lines.is_empty() || started_explicit {
                        blocks.push((cur_lbl, cur_lines));
                    }
                    break;
                }
                if bt.ends_with(':') && !bt.contains(' ') && !bt.starts_with('%') {
                    if !cur_lines.is_empty() || started_explicit {
                        blocks.push((cur_lbl, cur_lines));
                        cur_lines = Vec::new();
                    }
                    cur_lbl = bt.trim_end_matches(':').to_string();
                    started_explicit = true;
                } else if bt.starts_with("switch ") && bt.contains('[') && !bt.contains(']') {
                    let mut joined = bt.to_string();
                    while i < lines.len() {
                        let st = strip_comment(lines[i]).trim();
                        i += 1;
                        if !st.is_empty() { joined.push(' '); joined.push_str(st); }
                        if st.contains(']') { break; }
                    }
                    cur_lines.push((blno, joined));
                } else {
                    cur_lines.push((blno, bt.to_string()));
                }
            }
            let fidx = raw_fns.len() as u32;
            fn_map.insert(fname.clone(), fidx);
            raw_fns.push(RawFn { name: fname, params, blocks, start_line: line_no });
        }
    }

    // Compile each function to flat `BcOp` bytecode with backpatched jumps & phi edge moves
    let mut functions = Vec::with_capacity(raw_fns.len());
    for rf in &raw_fns {
        let mut fb = FnBuilder {
            global_map: &global_map,
            fn_map: &fn_map,
            ext_map: &ext_map,
            reg_map: HashMap::new(),
            next_reg: 0,
            stack_bytes: 0,
            ops: Vec::new(),
        };

        for (_, pname) in &rf.params {
            fb.get_or_alloc_reg(pname);
        }

        // Pre-scan all `phi` nodes so destination registers exist and incoming edges are known
        // target_label -> Vec<(dst_reg, ty, Vec<(pred_label, val_expr)>)>
        let mut phi_map: HashMap<String, Vec<(u16, String, Vec<(String, String)>)>> = HashMap::new();
        for (blbl, blines) in &rf.blocks {
            for (lno, inst) in blines {
                if let Some((lhs, rhs)) = inst.split_once(" = ") {
                    if let Some(phi_rest) = rhs.trim().strip_prefix("phi ") {
                        let brk = phi_rest.find('[').ok_or_else(|| bc_err(*lno, format!("Malformed phi: {}", inst)))?;
                        let ty_s = phi_rest[..brk].trim().to_string();
                        let dst = fb.get_or_alloc_reg(lhs.trim());
                        let mut pairs = Vec::new();
                        let mut cur = &phi_rest[brk..];
                        while let Some(o) = cur.find('[') {
                            let after = &cur[o + 1..];
                            let c = after.find(']').ok_or_else(|| bc_err(*lno, "Unclosed phi"))?;
                            let (v, p) = after[..c].rsplit_once(',').ok_or_else(|| bc_err(*lno, "Bad phi pair"))?;
                            pairs.push((p.trim().trim_start_matches('%').to_string(), v.trim().to_string()));
                            cur = &after[c + 1..];
                        }
                        phi_map.entry(blbl.clone()).or_default().push((dst, ty_s, pairs));
                    }
                }
            }
        }

        let mut label_pc: HashMap<String, u32> = HashMap::new();
        enum Patch {
            Jmp(usize, String),
            BrIf(usize, String, String),
            Switch(usize, String, Vec<(u64, String)>),
        }
        let mut patches: Vec<Patch> = Vec::new();

        // Helper to emit phi parallel copies along edge `cur_lbl -> target_lbl`
        let emit_phi_moves = |fb: &mut FnBuilder, cur_lbl: &str, tgt_lbl: &str, lno: usize| -> Result<(), VerifyError> {
            if let Some(phis) = phi_map.get(tgt_lbl) {
                let mut staged = Vec::with_capacity(phis.len());
                for (dst, ty, pairs) in phis {
                    let expr = pairs.iter().find(|(p, _)| p == cur_lbl).map(|(_, v)| v.as_str()).unwrap_or("0");
                    let src = fb.eval_op(expr, ty, lno)?;
                    let tmp = fb.alloc_tmp();
                    fb.ops.push(BcOp::Mov { dst: tmp, src });
                    staged.push((*dst, tmp));
                }
                for (dst, tmp) in staged {
                    fb.ops.push(BcOp::Mov { dst, src: tmp });
                }
            }
            Ok(())
        };

        for (blbl, blines) in &rf.blocks {
            label_pc.insert(blbl.clone(), fb.ops.len() as u32);
            for (lno, inst) in blines {
                let s = inst.trim();

                // Terminators
                if s == "ret void" {
                    fb.ops.push(BcOp::Ret { val: None });
                    continue;
                }
                if let Some(rest) = s.strip_prefix("ret ") {
                    let (ty, op) = split_ty_op(rest).ok_or_else(|| bc_err(*lno, format!("Malformed ret: {}", s)))?;
                    let r = fb.eval_op(&op, &ty, *lno)?;
                    fb.ops.push(BcOp::Ret { val: Some(r) });
                    continue;
                }
                if s == "unreachable" {
                    fb.ops.push(BcOp::Trap);
                    continue;
                }
                if let Some(rest) = s.strip_prefix("br label %") {
                    let tgt = rest.trim().to_string();
                    emit_phi_moves(&mut fb, blbl, &tgt, *lno)?;
                    let idx = fb.ops.len();
                    fb.ops.push(BcOp::Jmp { target: 0 });
                    patches.push(Patch::Jmp(idx, tgt));
                    continue;
                }
                if let Some(rest) = s.strip_prefix("br i1 ") {
                    let parts = split_commas(rest);
                    if parts.len() != 3 { return Err(bc_err(*lno, format!("Malformed br: {}", s))); }
                    let cond = fb.eval_op(parts[0], "i1", *lno)?;
                    let t_lbl = parts[1].trim().strip_prefix("label %").ok_or_else(|| bc_err(*lno, "Bad br true"))?.to_string();
                    let f_lbl = parts[2].trim().strip_prefix("label %").ok_or_else(|| bc_err(*lno, "Bad br false"))?.to_string();
                    let has_t_phi = phi_map.contains_key(&t_lbl);
                    let has_f_phi = phi_map.contains_key(&f_lbl);
                    if !has_t_phi && !has_f_phi {
                        let idx = fb.ops.len();
                        fb.ops.push(BcOp::BrIf { cond, true_target: 0, false_target: 0 });
                        patches.push(Patch::BrIf(idx, t_lbl, f_lbl));
                    } else {
                        // Emit mini-trampoline blocks for edge phi moves
                        let br_idx = fb.ops.len();
                        fb.ops.push(BcOp::BrIf { cond, true_target: 0, false_target: 0 });
                        let t_tramp = fb.ops.len() as u32;
                        emit_phi_moves(&mut fb, blbl, &t_lbl, *lno)?;
                        let tj_idx = fb.ops.len();
                        fb.ops.push(BcOp::Jmp { target: 0 });
                        patches.push(Patch::Jmp(tj_idx, t_lbl));
                        let f_tramp = fb.ops.len() as u32;
                        emit_phi_moves(&mut fb, blbl, &f_lbl, *lno)?;
                        let fj_idx = fb.ops.len();
                        fb.ops.push(BcOp::Jmp { target: 0 });
                        patches.push(Patch::Jmp(fj_idx, f_lbl));
                        fb.ops[br_idx] = BcOp::BrIf { cond, true_target: t_tramp, false_target: f_tramp };
                    }
                    continue;
                }
                if let Some(rest) = s.strip_prefix("switch ") {
                    let bo = rest.find('[').ok_or_else(|| bc_err(*lno, "Malformed switch"))?;
                    let bc = rest.rfind(']').ok_or_else(|| bc_err(*lno, "Malformed switch"))?;
                    let (vp, dp) = rest[..bo].trim().split_once(',').ok_or_else(|| bc_err(*lno, "Bad switch hdr"))?;
                    let (ty, vop) = split_ty_op(vp).ok_or_else(|| bc_err(*lno, "Bad switch val"))?;
                    let def_lbl = dp.trim().strip_prefix("label %").ok_or_else(|| bc_err(*lno, "Bad switch def"))?.to_string();
                    let val = fb.eval_op(&vop, &ty, *lno)?;
                    let mut cases = Vec::new();
                    let mut toks = rest[bo + 1..bc].split_whitespace();
                    while let Some(_) = toks.next() {
                        let imm_s = toks.next().unwrap_or("0").trim_end_matches(',');
                        let _ = toks.next(); // label
                        let tgt = toks.next().unwrap_or("").trim_start_matches('%').to_string();
                        if tgt.is_empty() { break; }
                        let iv = imm_s.parse::<i64>().map(|v| v as u64).or_else(|_| imm_s.parse::<u64>()).unwrap_or(0);
                        cases.push((iv, tgt));
                    }
                    let idx = fb.ops.len();
                    fb.ops.push(BcOp::Switch { val, default_target: 0, cases: Vec::new() });
                    patches.push(Patch::Switch(idx, def_lbl, cases));
                    continue;
                }

                // Store
                if let Some(rest) = s.strip_prefix("store ") {
                    let parts = split_commas(rest);
                    if parts.len() < 2 { return Err(bc_err(*lno, format!("Malformed store: {}", s))); }
                    let (vty, vop) = split_ty_op(parts[0]).ok_or_else(|| bc_err(*lno, "Bad store val"))?;
                    let (_, pop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad store ptr"))?;
                    let val = fb.eval_op(&vop, &vty, *lno)?;
                    let ptr = fb.eval_op(&pop, "ptr", *lno)?;
                    if vty.starts_with("<4 x ") || vty.starts_with("<2 x double") {
                        fb.ops.push(BcOp::StoreVec128 { ptr, src: val });
                    } else {
                        let w = type_width_bytes(&vty, &structs).min(8) as u8;
                        fb.ops.push(BcOp::Store { ptr, val, width: w });
                    }
                    continue;
                }

                // Void call
                if s.starts_with("call ") || s.starts_with("tail call ") {
                    lower_bc_call(&mut fb, s.strip_prefix("tail ").unwrap_or(s), None, *lno)?;
                    continue;
                }

                // Assignment: `%dst = <op>`
                if let Some((lhs, rhs)) = s.split_once(" = ") {
                    let dst_name = lhs.trim();
                    let rhs = rhs.trim();
                    if rhs.starts_with("phi ") { continue; }
                    if rhs.starts_with("call ") || rhs.starts_with("tail call ") {
                        let dst = fb.get_or_alloc_reg(dst_name);
                        lower_bc_call(&mut fb, rhs.strip_prefix("tail ").unwrap_or(rhs), Some(dst), *lno)?;
                        continue;
                    }
                    let dst = fb.get_or_alloc_reg(dst_name);

                    if let Some(rest) = rhs.strip_prefix("alloca ") {
                        let ty_s = rest.split(',').next().unwrap_or("i64").trim();
                        let sz = ((type_width_bytes(ty_s, &structs) + 15) & !15).max(16);
                        fb.stack_bytes += sz;
                        fb.ops.push(BcOp::Alloca { dst, size: sz });
                        continue;
                    }
                    if let Some(rest) = rhs.strip_prefix("load ") {
                        let parts = split_commas(rest);
                        if parts.len() < 2 { return Err(bc_err(*lno, format!("Bad load: {}", s))); }
                        let vty = strip_clang_attrs(parts[0]);
                        let (_, pop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad load ptr"))?;
                        let ptr = fb.eval_op(&pop, "ptr", *lno)?;
                        if vty.starts_with("<4 x ") || vty.starts_with("<2 x double") {
                            fb.ops.push(BcOp::LoadVec128 { dst, ptr });
                        } else {
                            let w = type_width_bytes(&vty, &structs).min(8) as u8;
                            fb.ops.push(BcOp::Load { dst, ptr, width: w });
                        }
                        continue;
                    }
                    if let Some(rest) = rhs.strip_prefix("getelementptr ") {
                        let mut rest = rest.strip_prefix("inbounds ").unwrap_or(rest).trim();
                        if rest.starts_with('(') && rest.ends_with(')') { rest = &rest[1..rest.len() - 1]; }
                        let parts = split_commas(rest);
                        if parts.len() < 3 { return Err(bc_err(*lno, format!("Bad GEP: {}", s))); }
                        let base_ty = parts[0].trim();
                        let (_, pop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad GEP ptr"))?;
                        let mut cur_ptr = fb.eval_op(&pop, "ptr", *lno)?;
                        let (_, idx0_op) = split_ty_op(parts[2]).ok_or_else(|| bc_err(*lno, "Bad GEP idx0"))?;
                        if idx0_op != "0" {
                            let bsz = type_width_bytes(base_ty, &structs) as i64;
                            let idx0 = fb.eval_op(&idx0_op, "i64", *lno)?;
                            let off = fb.alloc_tmp();
                            fb.ops.push(BcOp::MulImm { dst: off, a: idx0, imm: bsz });
                            let next = fb.alloc_tmp();
                            fb.ops.push(BcOp::Add { dst: next, a: cur_ptr, b: off });
                            cur_ptr = next;
                        }
                        if parts.len() >= 4 {
                            let (_, idx1_op) = split_ty_op(parts[3]).ok_or_else(|| bc_err(*lno, "Bad GEP idx1"))?;
                            if let Some(sn) = base_ty.strip_prefix("%struct.") {
                                if let (Some((offs, _)), Ok(fi)) = (structs.get(sn), idx1_op.parse::<usize>()) {
                                    let foff = offs.get(fi).copied().unwrap_or((fi * 8) as i32) as i64;
                                    if foff != 0 {
                                        let next = fb.alloc_tmp();
                                        fb.ops.push(BcOp::AddImm { dst: next, a: cur_ptr, imm: foff });
                                        cur_ptr = next;
                                    }
                                }
                            } else if base_ty.starts_with('[') && base_ty.ends_with(']') && idx1_op != "0" {
                                let esz = base_ty[1..base_ty.len() - 1]
                                    .split_once('x')
                                    .map(|(_, et)| type_width_bytes(et.trim(), &structs) as i64)
                                    .unwrap_or(1);
                                let idx1 = fb.eval_op(&idx1_op, "i64", *lno)?;
                                let off = fb.alloc_tmp();
                                fb.ops.push(BcOp::MulImm { dst: off, a: idx1, imm: esz });
                                let next = fb.alloc_tmp();
                                fb.ops.push(BcOp::Add { dst: next, a: cur_ptr, b: off });
                                cur_ptr = next;
                            }
                        }
                        fb.ops.push(BcOp::Mov { dst, src: cur_ptr });
                        continue;
                    }
                    // SIMD `insertelement <N x T> %v, T %x, i32 <lane>`
                    if let Some(rest) = rhs.strip_prefix("insertelement ") {
                        let parts = split_commas(rest);
                        if parts.len() == 3 {
                            let (vty, vop) = split_ty_op(parts[0]).ok_or_else(|| bc_err(*lno, "Bad insertelement"))?;
                            let (ety, eop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad insertelement"))?;
                            let (_, lop) = split_ty_op(parts[2]).ok_or_else(|| bc_err(*lno, "Bad insertelement"))?;
                            let vec = fb.eval_op(&vop, &vty, *lno)?;
                            let val = fb.eval_op(&eop, &ety, *lno)?;
                            let lane = lop.parse::<u8>().unwrap_or(0);
                            if vty.contains("double") {
                                fb.ops.push(BcOp::VecInsertF64 { dst, vec, val_f64: val, lane });
                            } else if vty.contains("float") {
                                fb.ops.push(BcOp::VecInsertF32 { dst, vec, val_f64: val, lane });
                            } else {
                                fb.ops.push(BcOp::VecInsertI32 { dst, vec, val_i64: val, lane });
                            }
                            continue;
                        }
                    }
                    // SIMD `extractelement <N x T> %v, i32 <lane>`
                    if let Some(rest) = rhs.strip_prefix("extractelement ") {
                        let parts = split_commas(rest);
                        if parts.len() == 2 {
                            let (vty, vop) = split_ty_op(parts[0]).ok_or_else(|| bc_err(*lno, "Bad extractelement"))?;
                            let (_, lop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad extractelement"))?;
                            let vec = fb.eval_op(&vop, &vty, *lno)?;
                            let lane = lop.parse::<u8>().unwrap_or(0);
                            if vty.contains("double") {
                                fb.ops.push(BcOp::VecExtractF64 { dst_f64: dst, vec, lane });
                            } else if vty.contains("float") {
                                fb.ops.push(BcOp::VecExtractF32 { dst_f64: dst, vec, lane });
                            } else {
                                fb.ops.push(BcOp::VecExtractI32 { dst_i64: dst, vec, lane });
                            }
                            continue;
                        }
                    }
                    // SIMD `shufflevector <4 x T> %a, <4 x T> %b, <4 x i32> <i32 0, i32 0, i32 0, i32 0>`
                    if let Some(rest) = rhs.strip_prefix("shufflevector ") {
                        let parts = split_commas(rest);
                        if parts.len() == 3 {
                            let (aty, aop) = split_ty_op(parts[0]).ok_or_else(|| bc_err(*lno, "Bad shufflevector"))?;
                            let (bty, bop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad shufflevector"))?;
                            let a = fb.eval_op(&aop, &aty, *lno)?;
                            let b = fb.eval_op(&bop, &bty, *lno)?;
                            let mut mask = [0u8; 4];
                            if let (Some(lo), Some(hi)) = (parts[2].rfind('<'), parts[2].rfind('>')) {
                                if hi > lo {
                                    for (idx, lane_s) in parts[2][lo + 1..hi].split(',').take(4).enumerate() {
                                        let num_tok = lane_s.split_whitespace().last().unwrap_or("0");
                                        mask[idx] = num_tok.parse::<u8>().unwrap_or(0);
                                    }
                                }
                            }
                            fb.ops.push(BcOp::VecShuffle4 { dst, a, b, mask });
                            continue;
                        }
                    }
                    if let Some(rest) = rhs.strip_prefix("select ") {
                        let parts = split_commas(rest);
                        if parts.len() == 3 {
                            let (_, cop) = split_ty_op(parts[0]).ok_or_else(|| bc_err(*lno, "Bad select"))?;
                            let (ty, aop) = split_ty_op(parts[1]).ok_or_else(|| bc_err(*lno, "Bad select"))?;
                            let (_, bop) = split_ty_op(parts[2]).ok_or_else(|| bc_err(*lno, "Bad select"))?;
                            let cond = fb.eval_op(&cop, "i1", *lno)?;
                            let a = fb.eval_op(&aop, &ty, *lno)?;
                            let b = fb.eval_op(&bop, &ty, *lno)?;
                            fb.ops.push(BcOp::Select { dst, cond, a, b });
                            continue;
                        }
                    }
                    if let Some(rest) = rhs.strip_prefix("icmp ") {
                        let (pred, after) = rest.split_once(' ').ok_or_else(|| bc_err(*lno, "Bad icmp"))?;
                        let (lp, rop) = after.split_once(',').ok_or_else(|| bc_err(*lno, "Bad icmp"))?;
                        let (ty, lop) = split_ty_op(lp).ok_or_else(|| bc_err(*lno, "Bad icmp"))?;
                        let cc = match pred.trim() {
                            "eq" => IntCmpOp::Eq, "ne" => IntCmpOp::Ne, "slt" => IntCmpOp::Slt,
                            "sle" => IntCmpOp::Sle, "sgt" => IntCmpOp::Sgt, "sge" => IntCmpOp::Sge,
                            "ult" => IntCmpOp::Ult, "ule" => IntCmpOp::Ule, "ugt" => IntCmpOp::Ugt,
                            "uge" => IntCmpOp::Uge,
                            o => return Err(bc_err(*lno, format!("Unknown icmp '{}'", o))),
                        };
                        let a = fb.eval_op(&lop, &ty, *lno)?;
                        let b = fb.eval_op(rop.trim(), &ty, *lno)?;
                        fb.ops.push(BcOp::ICmp { dst, cc, a, b });
                        continue;
                    }
                    if let Some(rest) = rhs.strip_prefix("fcmp ") {
                        let (pred, after) = rest.split_once(' ').ok_or_else(|| bc_err(*lno, "Bad fcmp"))?;
                        let (lp, rop) = after.split_once(',').ok_or_else(|| bc_err(*lno, "Bad fcmp"))?;
                        let (ty, lop) = split_ty_op(lp).ok_or_else(|| bc_err(*lno, "Bad fcmp"))?;
                        let cc = match pred.trim() {
                            "oeq" | "ueq" => FloatCmpOp::Oeq, "one" | "une" => FloatCmpOp::One,
                            "olt" | "ult" => FloatCmpOp::Olt, "ole" | "ule" => FloatCmpOp::Ole,
                            "ogt" | "ugt" => FloatCmpOp::Ogt, "oge" | "uge" => FloatCmpOp::Oge,
                            o => return Err(bc_err(*lno, format!("Unknown fcmp '{}'", o))),
                        };
                        let a = fb.eval_op(&lop, &ty, *lno)?;
                        let b = fb.eval_op(rop.trim(), &ty, *lno)?;
                        fb.ops.push(BcOp::FCmp { dst, cc, a, b });
                        continue;
                    }
                    // Casts
                    let mut handled_cast = false;
                    for cop in [
                        "sext ", "zext ", "trunc ", "sitofp ", "uitofp ", "fptosi ", "fptoui ",
                        "fpext ", "fptrunc ", "ptrtoint ", "inttoptr ", "bitcast ",
                    ] {
                        if let Some(rest) = rhs.strip_prefix(cop) {
                            let (src_part, dst_ty) = rest.rsplit_once(" to ").ok_or_else(|| bc_err(*lno, "Bad cast"))?;
                            let (src_ty, sop) = split_ty_op(src_part).ok_or_else(|| bc_err(*lno, "Bad cast src"))?;
                            let src = fb.eval_op(&sop, &src_ty, *lno)?;
                            match cop.trim() {
                                "sext" => fb.ops.push(BcOp::SExt { dst, src, from_bits: type_bits(&src_ty) }),
                                "zext" => fb.ops.push(BcOp::ZExt { dst, src, from_bits: type_bits(&src_ty) }),
                                "trunc" => fb.ops.push(BcOp::Trunc { dst, src, to_bits: type_bits(dst_ty) }),
                                "sitofp" => fb.ops.push(BcOp::SIToFP { dst, src }),
                                "uitofp" => fb.ops.push(BcOp::UIToFP { dst, src }),
                                "fptosi" => fb.ops.push(BcOp::FPToSI { dst, src }),
                                "fptoui" => fb.ops.push(BcOp::FPToUI { dst, src }),
                                "fpext" => fb.ops.push(BcOp::F32ToF64 { dst, src }),
                                "fptrunc" => fb.ops.push(BcOp::F64ToF32 { dst, src }),
                                _ => fb.ops.push(BcOp::Mov { dst, src }),
                            }
                            handled_cast = true;
                            break;
                        }
                    }
                    if handled_cast { continue; }

                    if let Some(rest) = rhs.strip_prefix("fneg ") {
                        let (ty, op) = split_ty_op(rest).ok_or_else(|| bc_err(*lno, "Bad fneg"))?;
                        let src = fb.eval_op(&op, &ty, *lno)?;
                        fb.ops.push(BcOp::FNeg { dst, src });
                        continue;
                    }

                    // Binary scalar & SIMD vector ALU ops
                    if let Some((op_raw, rest)) = rhs.split_once(' ') {
                        let op = op_raw.trim();
                        if matches!(
                            op,
                            "add" | "sub" | "mul" | "sdiv" | "udiv" | "srem" | "urem"
                                | "and" | "or" | "xor" | "shl" | "ashr" | "lshr"
                                | "fadd" | "fsub" | "fmul" | "fdiv"
                        ) {
                            let mut r = rest.trim();
                            while let Some(x) = r.strip_prefix("nsw ").or_else(|| r.strip_prefix("nuw ")).or_else(|| r.strip_prefix("exact ")).or_else(|| r.strip_prefix("fast ")) {
                                r = x.trim();
                            }
                            let (lp, rop) = r.split_once(',').ok_or_else(|| bc_err(*lno, "Bad binop"))?;
                            let (ty, lop) = split_ty_op(lp).ok_or_else(|| bc_err(*lno, "Bad binop lhs"))?;
                            let a = fb.eval_op(&lop, &ty, *lno)?;
                            let b = fb.eval_op(rop.trim(), &ty, *lno)?;
                            let is_vec4 = ty.starts_with("<4 x ");
                            let is_vec2d = ty.starts_with("<2 x double");
                            let inst_op = match (op, is_vec4, is_vec2d) {
                                ("fadd", true, _) => BcOp::VecFAdd4 { dst, a, b },
                                ("fsub", true, _) => BcOp::VecFSub4 { dst, a, b },
                                ("fmul", true, _) => BcOp::VecFMul4 { dst, a, b },
                                ("fdiv", true, _) => BcOp::VecFDiv4 { dst, a, b },
                                ("fadd", _, true) => BcOp::VecFAdd2D { dst, a, b },
                                ("fsub", _, true) => BcOp::VecFSub2D { dst, a, b },
                                ("fmul", _, true) => BcOp::VecFMul2D { dst, a, b },
                                ("fdiv", _, true) => BcOp::VecFDiv2D { dst, a, b },
                                ("add", true, _) => BcOp::VecIAdd4 { dst, a, b },
                                ("mul", true, _) => BcOp::VecIMul4 { dst, a, b },
                                ("add", false, false) => BcOp::Add { dst, a, b },
                                ("sub", false, false) => BcOp::Sub { dst, a, b },
                                ("mul", false, false) => BcOp::Mul { dst, a, b },
                                ("sdiv", false, false) => BcOp::SDiv { dst, a, b },
                                ("udiv", false, false) => BcOp::UDiv { dst, a, b },
                                ("srem", false, false) => BcOp::SRem { dst, a, b },
                                ("urem", false, false) => BcOp::URem { dst, a, b },
                                ("and", false, false) => BcOp::And { dst, a, b },
                                ("or", false, false) => BcOp::Or { dst, a, b },
                                ("xor", false, false) => BcOp::Xor { dst, a, b },
                                ("shl", false, false) => BcOp::Shl { dst, a, b },
                                ("ashr", false, false) => BcOp::AShr { dst, a, b },
                                ("lshr", false, false) => BcOp::LShr { dst, a, b },
                                ("fadd", false, false) => BcOp::FAdd { dst, a, b },
                                ("fsub", false, false) => BcOp::FSub { dst, a, b },
                                ("fmul", false, false) => BcOp::FMul { dst, a, b },
                                ("fdiv", false, false) => BcOp::FDiv { dst, a, b },
                                _ => return Err(bc_err(*lno, format!("Unsupported vector op '{}'", op))),
                            };
                            fb.ops.push(inst_op);
                            continue;
                        }
                    }
                }

                return Err(bc_err(*lno, format!("Unsupported SSA instruction for OmniBC: '{}'", s)));
            }
        }

        // Backpatch jump/branch/switch target PCs
        for p in patches {
            match p {
                Patch::Jmp(idx, lbl) => {
                    let tgt = *label_pc.get(&lbl).ok_or_else(|| bc_err(rf.start_line, format!("Undefined label '%{}'", lbl)))?;
                    fb.ops[idx] = BcOp::Jmp { target: tgt };
                }
                Patch::BrIf(idx, tl, fl) => {
                    let tt = *label_pc.get(&tl).ok_or_else(|| bc_err(rf.start_line, format!("Undefined label '%{}'", tl)))?;
                    let ft = *label_pc.get(&fl).ok_or_else(|| bc_err(rf.start_line, format!("Undefined label '%{}'", fl)))?;
                    if let BcOp::BrIf { cond, .. } = fb.ops[idx] {
                        fb.ops[idx] = BcOp::BrIf { cond, true_target: tt, false_target: ft };
                    }
                }
                Patch::Switch(idx, def_l, cases) => {
                    let dt = *label_pc.get(&def_l).ok_or_else(|| bc_err(rf.start_line, format!("Undefined label '%{}'", def_l)))?;
                    let mut resolved = Vec::with_capacity(cases.len());
                    for (cv, cl) in cases {
                        let ct = *label_pc.get(&cl).ok_or_else(|| bc_err(rf.start_line, format!("Undefined label '%{}'", cl)))?;
                        resolved.push((cv, ct));
                    }
                    if let BcOp::Switch { val, .. } = fb.ops[idx] {
                        fb.ops[idx] = BcOp::Switch { val, default_target: dt, cases: resolved };
                    }
                }
            }
        }

        functions.push(BcFunction {
            name: rf.name.clone(),
            num_params: rf.params.len() as u16,
            num_regs: fb.next_reg,
            stack_bytes: fb.stack_bytes,
            ops: fb.ops,
        });
    }

    Ok(OmniBcModule { globals, externs, functions })
}

fn lower_bc_call(
    fb: &mut FnBuilder,
    call_str: &str,
    dst: Option<u16>,
    lno: usize,
) -> Result<(), VerifyError> {
    let rest = call_str.trim().strip_prefix("call ").unwrap_or(call_str).trim();
    let rp = rest.rfind(')').ok_or_else(|| bc_err(lno, format!("Malformed call: {}", call_str)))?;
    let mut depth = 0i32;
    let mut lp_opt = None;
    for (idx, b) in rest[..=rp].bytes().enumerate().rev() {
        if b == b')' { depth += 1; } else if b == b'(' { depth -= 1; if depth == 0 { lp_opt = Some(idx); break; } }
    }
    let lp = lp_opt.ok_or_else(|| bc_err(lno, format!("Malformed call args: {}", call_str)))?;
    let head = rest[..lp].trim();
    let args_str = rest[lp + 1..rp].trim();
    let (ret_part, callee) = head.rsplit_once(' ').ok_or_else(|| bc_err(lno, format!("Malformed call head: {}", call_str)))?;
    let ret_clean = strip_clang_attrs(ret_part.split('(').next().unwrap_or(ret_part));
    let ret_is_f64 = ret_clean.trim() == "double" || ret_clean.trim() == "float";

    let mut args = Vec::new();
    let mut arg_is_f64 = Vec::new();
    if !args_str.is_empty() {
        for a in split_commas(args_str) {
            let (aty, aop) = split_ty_op(a).ok_or_else(|| bc_err(lno, format!("Malformed call arg '{}'", a)))?;
            let r = fb.eval_op(&aop, &aty, lno)?;
            args.push(r);
            arg_is_f64.push(aty.trim() == "double" || aty.trim() == "float");
        }
    }

    if let Some(sym) = callee.strip_prefix('@') {
        // Clang / LLVM intrinsic shims
        if (sym.starts_with("llvm.memcpy.") || sym.starts_with("llvm.memmove.")) && args.len() >= 3 {
            fb.ops.push(BcOp::MemCpy { dst: args[0], src: args[1], len: args[2] });
            return Ok(());
        }
        if sym == "llvm.fma.f64" && args.len() == 3 {
            if let Some(d) = dst { fb.ops.push(BcOp::FFma { dst: d, a: args[0], b: args[1], c: args[2] }); }
            return Ok(());
        }
        if sym.starts_with("llvm.cttz.") && !args.is_empty() {
            if let Some(d) = dst { fb.ops.push(BcOp::Cttz64 { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym.starts_with("llvm.memset.") && args.len() >= 3 {
            fb.ops.push(BcOp::MemSet { dst: args[0], byte_val: args[1], len: args[2] });
            return Ok(());
        }
        if sym == "llvm.sqrt.f64" && args.len() == 1 {
            if let Some(d) = dst { fb.ops.push(BcOp::FSqrt { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym == "llvm.fabs.f64" && args.len() == 1 {
            if let Some(d) = dst { fb.ops.push(BcOp::FAbs { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym == "llvm.pow.f64" && args.len() == 2 {
            if let Some(d) = dst { fb.ops.push(BcOp::FPow { dst: d, a: args[0], b: args[1] }); }
            return Ok(());
        }
        if sym == "llvm.floor.f64" && args.len() == 1 {
            if let Some(d) = dst { fb.ops.push(BcOp::FFloor { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym == "llvm.ceil.f64" && args.len() == 1 {
            if let Some(d) = dst { fb.ops.push(BcOp::FCeil { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym.starts_with("llvm.bswap.") && args.len() == 1 {
            if let Some(d) = dst { fb.ops.push(BcOp::BSwap64 { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym.starts_with("llvm.ctlz.") && !args.is_empty() {
            if let Some(d) = dst { fb.ops.push(BcOp::Ctlz64 { dst: d, src: args[0] }); }
            return Ok(());
        }
        if sym.starts_with("llvm.lifetime.") || sym.starts_with("llvm.dbg.") {
            return Ok(());
        }

        if let Some(&fidx) = fb.fn_map.get(sym) {
            fb.ops.push(BcOp::CallInternal { dst, fn_idx: fidx, args });
            return Ok(());
        }
        if let Some(&(eidx, _, _)) = fb.ext_map.get(sym) {
            fb.ops.push(BcOp::CallExtern { dst, ext_idx: eidx, args, arg_is_f64, ret_is_f64 });
            return Ok(());
        }
        return Err(bc_err(lno, format!("Call to undeclared function '@{}'", sym)));
    }

    if callee.starts_with('%') {
        let fn_ptr = fb.eval_op(callee, "ptr", lno)?;
        fb.ops.push(BcOp::CallIndirect { dst, fn_ptr, args });
        return Ok(());
    }

    Err(bc_err(lno, format!("Invalid call target '{}'", callee)))
}

// ---------------------------------------------------------------------------
// 4. `OmniBC` Register-VM Interpreter with Polyglot FFI & 128-bit SIMD
// ---------------------------------------------------------------------------

pub struct OmniBcVm<'a> {
    module: &'a OmniBcModule,
    ext_ptrs: Vec<Option<*const u8>>,
}

impl<'a> OmniBcVm<'a> {
    pub fn new(module: &'a OmniBcModule, host_syms: &[(&str, *const u8)]) -> Self {
        let sym_map: HashMap<&str, *const u8> = host_syms.iter().copied().collect();
        let ext_ptrs = module
            .externs
            .iter()
            .map(|e| sym_map.get(e.name.as_str()).copied())
            .collect();
        Self { module, ext_ptrs }
    }

    /// Execute a named function (default `"main"`) with up to 6 `u64` arguments.
    pub fn call_by_name(&self, fn_name: &str, args: &[u64]) -> Result<u64, VerifyError> {
        let fidx = self
            .module
            .functions
            .iter()
            .position(|f| f.name == fn_name)
            .ok_or_else(|| VerifyError::LinkError(format!("Function '{}' not found in OmniBC module", fn_name)))?;
        self.exec_fn(fidx, args)
    }

    fn exec_fn(&self, fn_idx: usize, args: &[u64]) -> Result<u64, VerifyError> {
        let func = &self.module.functions[fn_idx];
        let mut regs = vec![0u64; (func.num_regs as usize).max(args.len()).max(1)];
        let mut vregs = vec![[0u8; 16]; regs.len()];
        for (i, &a) in args.iter().enumerate() {
            regs[i] = a;
        }
        let mut stack = vec![0u8; (func.stack_bytes as usize).max(16)];
        let mut sp = 0usize;
        let mut pc = 0usize;

        while pc < func.ops.len() {
            match &func.ops[pc] {
                BcOp::Nop => pc += 1,
                BcOp::LoadImm { dst, val } => { regs[*dst as usize] = *val; pc += 1; }
                BcOp::LoadGlobalPtr { dst, global_idx } => {
                    let ptr = self.module.globals[*global_idx as usize].bytes.as_ptr() as usize as u64;
                    regs[*dst as usize] = ptr;
                    pc += 1;
                }
                BcOp::LoadFnPtr { dst, fn_idx } => {
                    // Tag internal function index with high bit so CallIndirect can distinguish internal vs native fn ptr
                    regs[*dst as usize] = (1u64 << 63) | (*fn_idx as u64);
                    pc += 1;
                }
                BcOp::Mov { dst, src } => {
                    regs[*dst as usize] = regs[*src as usize];
                    vregs[*dst as usize] = vregs[*src as usize];
                    pc += 1;
                }
                BcOp::Alloca { dst, size } => {
                    let sz = *size as usize;
                    if sp + sz > stack.len() {
                        return Err(VerifyError::LinkError("OmniBC stack frame overflow".into()));
                    }
                    let ptr = unsafe { stack.as_mut_ptr().add(sp) } as usize as u64;
                    sp += sz;
                    regs[*dst as usize] = ptr;
                    pc += 1;
                }
                BcOp::Load { dst, ptr, width } => {
                    let p = regs[*ptr as usize] as usize as *const u8;
                    let val: u64 = unsafe {
                        match *width {
                            1 => *(p as *const u8) as u64,
                            2 => (p as *const u16).read_unaligned() as u64,
                            4 => (p as *const u32).read_unaligned() as u64,
                            _ => (p as *const u64).read_unaligned(),
                        }
                    };
                    regs[*dst as usize] = val;
                    pc += 1;
                }
                BcOp::Store { ptr, val, width } => {
                    let p = regs[*ptr as usize] as usize as *mut u8;
                    let v = regs[*val as usize];
                    unsafe {
                        match *width {
                            1 => *(p as *mut u8) = v as u8,
                            2 => (p as *mut u16).write_unaligned(v as u16),
                            4 => (p as *mut u32).write_unaligned(v as u32),
                            _ => (p as *mut u64).write_unaligned(v),
                        }
                    }
                    pc += 1;
                }
                BcOp::LoadVec128 { dst, ptr } => {
                    let p = regs[*ptr as usize] as usize as *const u8;
                    unsafe { std::ptr::copy_nonoverlapping(p, vregs[*dst as usize].as_mut_ptr(), 16); }
                    pc += 1;
                }
                BcOp::StoreVec128 { ptr, src } => {
                    let p = regs[*ptr as usize] as usize as *mut u8;
                    unsafe { std::ptr::copy_nonoverlapping(vregs[*src as usize].as_ptr(), p, 16); }
                    pc += 1;
                }
                BcOp::MemCpy { dst, src, len } => {
                    let d = regs[*dst as usize] as usize as *mut u8;
                    let s = regs[*src as usize] as usize as *const u8;
                    let n = regs[*len as usize] as usize;
                    unsafe { std::ptr::copy(s, d, n); }
                    pc += 1;
                }
                BcOp::MemSet { dst, byte_val, len } => {
                    let d = regs[*dst as usize] as usize as *mut u8;
                    let v = regs[*byte_val as usize] as u8;
                    let n = regs[*len as usize] as usize;
                    unsafe { std::ptr::write_bytes(d, v, n); }
                    pc += 1;
                }
                BcOp::Add { dst, a, b } => { regs[*dst as usize] = regs[*a as usize].wrapping_add(regs[*b as usize]); pc += 1; }
                BcOp::Sub { dst, a, b } => { regs[*dst as usize] = regs[*a as usize].wrapping_sub(regs[*b as usize]); pc += 1; }
                BcOp::Mul { dst, a, b } => { regs[*dst as usize] = regs[*a as usize].wrapping_mul(regs[*b as usize]); pc += 1; }
                BcOp::SDiv { dst, a, b } => {
                    let bv = regs[*b as usize] as i64;
                    regs[*dst as usize] = if bv == 0 { 0 } else { (regs[*a as usize] as i64).wrapping_div(bv) as u64 };
                    pc += 1;
                }
                BcOp::UDiv { dst, a, b } => {
                    let bv = regs[*b as usize];
                    regs[*dst as usize] = if bv == 0 { 0 } else { regs[*a as usize] / bv };
                    pc += 1;
                }
                BcOp::SRem { dst, a, b } => {
                    let bv = regs[*b as usize] as i64;
                    regs[*dst as usize] = if bv == 0 { 0 } else { (regs[*a as usize] as i64).wrapping_rem(bv) as u64 };
                    pc += 1;
                }
                BcOp::URem { dst, a, b } => {
                    let bv = regs[*b as usize];
                    regs[*dst as usize] = if bv == 0 { 0 } else { regs[*a as usize] % bv };
                    pc += 1;
                }
                BcOp::And { dst, a, b } => { regs[*dst as usize] = regs[*a as usize] & regs[*b as usize]; pc += 1; }
                BcOp::Or { dst, a, b } => { regs[*dst as usize] = regs[*a as usize] | regs[*b as usize]; pc += 1; }
                BcOp::Xor { dst, a, b } => { regs[*dst as usize] = regs[*a as usize] ^ regs[*b as usize]; pc += 1; }
                BcOp::Shl { dst, a, b } => { regs[*dst as usize] = regs[*a as usize].wrapping_shl(regs[*b as usize] as u32); pc += 1; }
                BcOp::AShr { dst, a, b } => { regs[*dst as usize] = ((regs[*a as usize] as i64).wrapping_shr(regs[*b as usize] as u32)) as u64; pc += 1; }
                BcOp::LShr { dst, a, b } => { regs[*dst as usize] = regs[*a as usize].wrapping_shr(regs[*b as usize] as u32); pc += 1; }
                BcOp::AddImm { dst, a, imm } => { regs[*dst as usize] = regs[*a as usize].wrapping_add(*imm as u64); pc += 1; }
                BcOp::MulImm { dst, a, imm } => { regs[*dst as usize] = regs[*a as usize].wrapping_mul(*imm as u64); pc += 1; }
                BcOp::ICmp { dst, cc, a, b } => {
                    let av = regs[*a as usize];
                    let bv = regs[*b as usize];
                    let res = match cc {
                        IntCmpOp::Eq => av == bv,
                        IntCmpOp::Ne => av != bv,
                        IntCmpOp::Slt => (av as i64) < (bv as i64),
                        IntCmpOp::Sle => (av as i64) <= (bv as i64),
                        IntCmpOp::Sgt => (av as i64) > (bv as i64),
                        IntCmpOp::Sge => (av as i64) >= (bv as i64),
                        IntCmpOp::Ult => av < bv,
                        IntCmpOp::Ule => av <= bv,
                        IntCmpOp::Ugt => av > bv,
                        IntCmpOp::Uge => av >= bv,
                    };
                    regs[*dst as usize] = res as u64;
                    pc += 1;
                }
                BcOp::Select { dst, cond, a, b } => {
                    regs[*dst as usize] = if regs[*cond as usize] != 0 { regs[*a as usize] } else { regs[*b as usize] };
                    pc += 1;
                }
                BcOp::FAdd { dst, a, b } => { regs[*dst as usize] = (f64::from_bits(regs[*a as usize]) + f64::from_bits(regs[*b as usize])).to_bits(); pc += 1; }
                BcOp::FSub { dst, a, b } => { regs[*dst as usize] = (f64::from_bits(regs[*a as usize]) - f64::from_bits(regs[*b as usize])).to_bits(); pc += 1; }
                BcOp::FMul { dst, a, b } => { regs[*dst as usize] = (f64::from_bits(regs[*a as usize]) * f64::from_bits(regs[*b as usize])).to_bits(); pc += 1; }
                BcOp::FDiv { dst, a, b } => { regs[*dst as usize] = (f64::from_bits(regs[*a as usize]) / f64::from_bits(regs[*b as usize])).to_bits(); pc += 1; }
                BcOp::FNeg { dst, src } => { regs[*dst as usize] = (-f64::from_bits(regs[*src as usize])).to_bits(); pc += 1; }
                BcOp::FSqrt { dst, src } => { regs[*dst as usize] = f64::from_bits(regs[*src as usize]).sqrt().to_bits(); pc += 1; }
                BcOp::FAbs { dst, src } => { regs[*dst as usize] = f64::from_bits(regs[*src as usize]).abs().to_bits(); pc += 1; }
                BcOp::FPow { dst, a, b } => { regs[*dst as usize] = f64::from_bits(regs[*a as usize]).powf(f64::from_bits(regs[*b as usize])).to_bits(); pc += 1; }
                BcOp::FFloor { dst, src } => { regs[*dst as usize] = f64::from_bits(regs[*src as usize]).floor().to_bits(); pc += 1; }
                BcOp::FCeil { dst, src } => { regs[*dst as usize] = f64::from_bits(regs[*src as usize]).ceil().to_bits(); pc += 1; }
                BcOp::FCmp { dst, cc, a, b } => {
                    let av = f64::from_bits(regs[*a as usize]);
                    let bv = f64::from_bits(regs[*b as usize]);
                    let res = match cc {
                        FloatCmpOp::Oeq => av == bv, FloatCmpOp::One => av != bv,
                        FloatCmpOp::Olt => av < bv, FloatCmpOp::Ole => av <= bv,
                        FloatCmpOp::Ogt => av > bv, FloatCmpOp::Oge => av >= bv,
                    };
                    regs[*dst as usize] = res as u64;
                    pc += 1;
                }
                BcOp::SExt { dst, src, from_bits } => {
                    let v = regs[*src as usize];
                    regs[*dst as usize] = match *from_bits {
                        1 => if (v & 1) != 0 { u64::MAX } else { 0 },
                        8 => (v as i8) as i64 as u64,
                        16 => (v as i16) as i64 as u64,
                        32 => (v as i32) as i64 as u64,
                        _ => v,
                    };
                    pc += 1;
                }
                BcOp::ZExt { dst, src, from_bits } => {
                    let v = regs[*src as usize];
                    regs[*dst as usize] = match *from_bits {
                        1 => v & 1, 8 => v & 0xFF, 16 => v & 0xFFFF, 32 => v & 0xFFFF_FFFF, _ => v,
                    };
                    pc += 1;
                }
                BcOp::Trunc { dst, src, to_bits } => {
                    let v = regs[*src as usize];
                    regs[*dst as usize] = match *to_bits {
                        1 => v & 1, 8 => v & 0xFF, 16 => v & 0xFFFF, 32 => v & 0xFFFF_FFFF, _ => v,
                    };
                    pc += 1;
                }
                BcOp::SIToFP { dst, src } => { regs[*dst as usize] = ((regs[*src as usize] as i64) as f64).to_bits(); pc += 1; }
                BcOp::UIToFP { dst, src } => { regs[*dst as usize] = (regs[*src as usize] as f64).to_bits(); pc += 1; }
                BcOp::FPToSI { dst, src } => { regs[*dst as usize] = (f64::from_bits(regs[*src as usize]) as i64) as u64; pc += 1; }
                BcOp::FPToUI { dst, src } => { regs[*dst as usize] = f64::from_bits(regs[*src as usize]) as u64; pc += 1; }
                BcOp::F32ToF64 { dst, src } => {
                    let f = f32::from_bits(regs[*src as usize] as u32);
                    regs[*dst as usize] = (f as f64).to_bits();
                    pc += 1;
                }
                BcOp::F64ToF32 { dst, src } => {
                    let f = f64::from_bits(regs[*src as usize]) as f32;
                    regs[*dst as usize] = f.to_bits() as u64;
                    pc += 1;
                }
                BcOp::BSwap64 { dst, src } => { regs[*dst as usize] = regs[*src as usize].swap_bytes(); pc += 1; }
                BcOp::Ctlz64 { dst, src } => { regs[*dst as usize] = regs[*src as usize].leading_zeros() as u64; pc += 1; }

                // 128-bit SIMD Vector ALU
                BcOp::VecFAdd4 { dst, a, b } => {
                    let va = bytes_to_f32x4(&vregs[*a as usize]);
                    let vb = bytes_to_f32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = f32x4_to_bytes([va[0] + vb[0], va[1] + vb[1], va[2] + vb[2], va[3] + vb[3]]);
                    pc += 1;
                }
                BcOp::VecFSub4 { dst, a, b } => {
                    let va = bytes_to_f32x4(&vregs[*a as usize]);
                    let vb = bytes_to_f32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = f32x4_to_bytes([va[0] - vb[0], va[1] - vb[1], va[2] - vb[2], va[3] - vb[3]]);
                    pc += 1;
                }
                BcOp::VecFMul4 { dst, a, b } => {
                    let va = bytes_to_f32x4(&vregs[*a as usize]);
                    let vb = bytes_to_f32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = f32x4_to_bytes([va[0] * vb[0], va[1] * vb[1], va[2] * vb[2], va[3] * vb[3]]);
                    pc += 1;
                }
                BcOp::VecFDiv4 { dst, a, b } => {
                    let va = bytes_to_f32x4(&vregs[*a as usize]);
                    let vb = bytes_to_f32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = f32x4_to_bytes([va[0] / vb[0], va[1] / vb[1], va[2] / vb[2], va[3] / vb[3]]);
                    pc += 1;
                }
                BcOp::VecIAdd4 { dst, a, b } => {
                    let va = bytes_to_i32x4(&vregs[*a as usize]);
                    let vb = bytes_to_i32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = i32x4_to_bytes([va[0].wrapping_add(vb[0]), va[1].wrapping_add(vb[1]), va[2].wrapping_add(vb[2]), va[3].wrapping_add(vb[3])]);
                    pc += 1;
                }
                BcOp::VecIMul4 { dst, a, b } => {
                    let va = bytes_to_i32x4(&vregs[*a as usize]);
                    let vb = bytes_to_i32x4(&vregs[*b as usize]);
                    vregs[*dst as usize] = i32x4_to_bytes([va[0].wrapping_mul(vb[0]), va[1].wrapping_mul(vb[1]), va[2].wrapping_mul(vb[2]), va[3].wrapping_mul(vb[3])]);
                    pc += 1;
                }
                BcOp::VecInsertF32 { dst, vec, val_f64, lane } => {
                    let mut v = bytes_to_f32x4(&vregs[*vec as usize]);
                    v[(*lane as usize) & 3] = f64::from_bits(regs[*val_f64 as usize]) as f32;
                    vregs[*dst as usize] = f32x4_to_bytes(v);
                    pc += 1;
                }
                BcOp::VecExtractF32 { dst_f64, vec, lane } => {
                    let v = bytes_to_f32x4(&vregs[*vec as usize]);
                    regs[*dst_f64 as usize] = (v[(*lane as usize) & 3] as f64).to_bits();
                    pc += 1;
                }
                BcOp::VecInsertI32 { dst, vec, val_i64, lane } => {
                    let mut v = bytes_to_i32x4(&vregs[*vec as usize]);
                    v[(*lane as usize) & 3] = regs[*val_i64 as usize] as i32;
                    vregs[*dst as usize] = i32x4_to_bytes(v);
                    pc += 1;
                }
                BcOp::VecExtractI32 { dst_i64, vec, lane } => {
                    let v = bytes_to_i32x4(&vregs[*vec as usize]);
                    regs[*dst_i64 as usize] = (v[(*lane as usize) & 3] as i64) as u64;
                    pc += 1;
                }
                BcOp::VecFAdd2D { dst, a, b } => {
                    let va = bytes_to_f64x2(&vregs[*a as usize]);
                    let vb = bytes_to_f64x2(&vregs[*b as usize]);
                    vregs[*dst as usize] = f64x2_to_bytes([va[0] + vb[0], va[1] + vb[1]]);
                    pc += 1;
                }
                BcOp::VecFSub2D { dst, a, b } => {
                    let va = bytes_to_f64x2(&vregs[*a as usize]);
                    let vb = bytes_to_f64x2(&vregs[*b as usize]);
                    vregs[*dst as usize] = f64x2_to_bytes([va[0] - vb[0], va[1] - vb[1]]);
                    pc += 1;
                }
                BcOp::VecFMul2D { dst, a, b } => {
                    let va = bytes_to_f64x2(&vregs[*a as usize]);
                    let vb = bytes_to_f64x2(&vregs[*b as usize]);
                    vregs[*dst as usize] = f64x2_to_bytes([va[0] * vb[0], va[1] * vb[1]]);
                    pc += 1;
                }
                BcOp::VecFDiv2D { dst, a, b } => {
                    let va = bytes_to_f64x2(&vregs[*a as usize]);
                    let vb = bytes_to_f64x2(&vregs[*b as usize]);
                    vregs[*dst as usize] = f64x2_to_bytes([va[0] / vb[0], va[1] / vb[1]]);
                    pc += 1;
                }
                BcOp::VecInsertF64 { dst, vec, val_f64, lane } => {
                    let mut v = bytes_to_f64x2(&vregs[*vec as usize]);
                    v[(*lane as usize) & 1] = f64::from_bits(regs[*val_f64 as usize]);
                    vregs[*dst as usize] = f64x2_to_bytes(v);
                    pc += 1;
                }
                BcOp::VecExtractF64 { dst_f64, vec, lane } => {
                    let v = bytes_to_f64x2(&vregs[*vec as usize]);
                    regs[*dst_f64 as usize] = v[(*lane as usize) & 1].to_bits();
                    pc += 1;
                }
                BcOp::VecShuffle4 { dst, a, b, mask } => {
                    let va = bytes_to_i32x4(&vregs[*a as usize]);
                    let vb = bytes_to_i32x4(&vregs[*b as usize]);
                    let mut out = [0i32; 4];
                    for lane in 0..4 {
                        let m = mask[lane] as usize;
                        out[lane] = if m < 4 { va[m] } else { vb[(m - 4) & 3] };
                    }
                    vregs[*dst as usize] = i32x4_to_bytes(out);
                    pc += 1;
                }
                BcOp::FFma { dst, a, b, c } => {
                    let av = f64::from_bits(regs[*a as usize]);
                    let bv = f64::from_bits(regs[*b as usize]);
                    let cv = f64::from_bits(regs[*c as usize]);
                    regs[*dst as usize] = av.mul_add(bv, cv).to_bits();
                    pc += 1;
                }
                BcOp::Cttz64 { dst, src } => {
                    regs[*dst as usize] = regs[*src as usize].trailing_zeros() as u64;
                    pc += 1;
                }

                // Control Flow
                BcOp::Jmp { target } => pc = *target as usize,
                BcOp::BrIf { cond, true_target, false_target } => {
                    pc = if regs[*cond as usize] != 0 { *true_target as usize } else { *false_target as usize };
                }
                BcOp::Switch { val, default_target, cases } => {
                    let v = regs[*val as usize];
                    pc = cases.iter().find(|(cv, _)| *cv == v).map(|(_, t)| *t).unwrap_or(*default_target) as usize;
                }
                BcOp::Ret { val } => return Ok(val.map(|r| regs[r as usize]).unwrap_or(0)),
                BcOp::Trap => return Err(VerifyError::LinkError("OmniBC VM hit trap/unreachable".into())),

                // Calls
                BcOp::CallInternal { dst, fn_idx, args } => {
                    let cargs: Vec<u64> = args.iter().map(|&r| regs[r as usize]).collect();
                    let ret = self.exec_fn(*fn_idx as usize, &cargs)?;
                    if let Some(d) = dst { regs[*d as usize] = ret; }
                    pc += 1;
                }
                BcOp::CallIndirect { dst, fn_ptr, args } => {
                    let raw_ptr = regs[*fn_ptr as usize];
                    let cargs: Vec<u64> = args.iter().map(|&r| regs[r as usize]).collect();
                    let ret = if (raw_ptr & (1u64 << 63)) != 0 {
                        let fidx = (raw_ptr & !(1u64 << 63)) as usize;
                        self.exec_fn(fidx, &cargs)?
                    } else {
                        invoke_native_c_fn(raw_ptr as usize as *const u8, &cargs, &vec![false; cargs.len()], false)
                    };
                    if let Some(d) = dst { regs[*d as usize] = ret; }
                    pc += 1;
                }
                BcOp::CallExtern { dst, ext_idx, args, arg_is_f64, ret_is_f64 } => {
                    let fptr = self.ext_ptrs[*ext_idx as usize].ok_or_else(|| {
                        VerifyError::LinkError(format!(
                            "Unresolved external FFI symbol '{}' in OmniBC VM",
                            self.module.externs[*ext_idx as usize].name
                        ))
                    })?;
                    let cargs: Vec<u64> = args.iter().map(|&r| regs[r as usize]).collect();
                    let ret = invoke_native_c_fn(fptr, &cargs, arg_is_f64, *ret_is_f64);
                    if let Some(d) = dst { regs[*d as usize] = ret; }
                    pc += 1;
                }
            }
        }
        Ok(0)
    }
}

#[inline(always)]
fn bytes_to_f32x4(b: &[u8; 16]) -> [f32; 4] {
    [
        f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        f32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        f32::from_le_bytes([b[12], b[13], b[14], b[15]]),
    ]
}
#[inline(always)]
fn f32x4_to_bytes(v: [f32; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&v[0].to_le_bytes());
    b[4..8].copy_from_slice(&v[1].to_le_bytes());
    b[8..12].copy_from_slice(&v[2].to_le_bytes());
    b[12..16].copy_from_slice(&v[3].to_le_bytes());
    b
}
#[inline(always)]
fn bytes_to_i32x4(b: &[u8; 16]) -> [i32; 4] {
    [
        i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        i32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        i32::from_le_bytes([b[12], b[13], b[14], b[15]]),
    ]
}
#[inline(always)]
fn i32x4_to_bytes(v: [i32; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&v[0].to_le_bytes());
    b[4..8].copy_from_slice(&v[1].to_le_bytes());
    b[8..12].copy_from_slice(&v[2].to_le_bytes());
    b[12..16].copy_from_slice(&v[3].to_le_bytes());
    b
}
#[inline(always)]
fn bytes_to_f64x2(b: &[u8; 16]) -> [f64; 2] {
    [
        f64::from_le_bytes(b[0..8].try_into().unwrap()),
        f64::from_le_bytes(b[8..16].try_into().unwrap()),
    ]
}
#[inline(always)]
fn f64x2_to_bytes(v: [f64; 2]) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..8].copy_from_slice(&v[0].to_le_bytes());
    b[8..16].copy_from_slice(&v[1].to_le_bytes());
    b
}

/// Trampoline for invoking native C / `omni_runtime` functions from `OmniBC` VM.
fn invoke_native_c_fn(fptr: *const u8, args: &[u64], arg_is_f64: &[bool], ret_is_f64: bool) -> u64 {
    unsafe {
        if args.len() == 1 && arg_is_f64.first().copied().unwrap_or(false) {
            let f: extern "C" fn(f64) -> u64 = std::mem::transmute(fptr);
            return f(f64::from_bits(args[0]));
        }
        if ret_is_f64 {
            match args.len() {
                0 => { let f: extern "C" fn() -> f64 = std::mem::transmute(fptr); f().to_bits() }
                1 => { let f: extern "C" fn(u64) -> f64 = std::mem::transmute(fptr); f(args[0]).to_bits() }
                2 => { let f: extern "C" fn(u64, u64) -> f64 = std::mem::transmute(fptr); f(args[0], args[1]).to_bits() }
                _ => 0,
            }
        } else {
            match args.len() {
                0 => { let f: extern "C" fn() -> u64 = std::mem::transmute(fptr); f() }
                1 => { let f: extern "C" fn(u64) -> u64 = std::mem::transmute(fptr); f(args[0]) }
                2 => { let f: extern "C" fn(u64, u64) -> u64 = std::mem::transmute(fptr); f(args[0], args[1]) }
                3 => { let f: extern "C" fn(u64, u64, u64) -> u64 = std::mem::transmute(fptr); f(args[0], args[1], args[2]) }
                4 => { let f: extern "C" fn(u64, u64, u64, u64) -> u64 = std::mem::transmute(fptr); f(args[0], args[1], args[2], args[3]) }
                5 => { let f: extern "C" fn(u64, u64, u64, u64, u64) -> u64 = std::mem::transmute(fptr); f(args[0], args[1], args[2], args[3], args[4]) }
                _ => {
                    let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64 = std::mem::transmute(fptr);
                    f(args[0], args[1], args[2], args[3], args[4], args[5])
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_omnibc_compile_serialize_roundtrip_and_exec() {
        let ir = r#"
define i64 @fib(i64 %n) {
entry:
  %c = icmp sle i64 %n, 1
  br i1 %c, label %base, label %rec
base:
  ret i64 %n
rec:
  %n1 = sub i64 %n, 1
  %f1 = call i64 @fib(i64 %n1)
  %n2 = sub i64 %n, 2
  %f2 = call i64 @fib(i64 %n2)
  %sum = add i64 %f1, %f2
  ret i64 %sum
}

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %res = call i64 @fib(i64 12)
  %r32 = trunc i64 %res to i32
  ret i32 %r32
}
"#;
        let bc = compile_llvm_ir_to_bc(ir).expect("compile LLVM IR -> OmniBC");
        let bytes = bc.to_bytes();
        assert_eq!(&bytes[0..4], b"OBC1");
        let bc2 = OmniBcModule::from_bytes(&bytes).expect("roundtrip .obc deserialize");
        assert_eq!(bc, bc2, "OmniBC binary serialization must be lossless & deterministic");

        let vm = OmniBcVm::new(&bc2, &[]);
        let ret = vm.call_by_name("main", &[0, 0]).expect("VM exec");
        assert_eq!(ret, 144, "fib(12) must equal 144");
    }

    #[test]
    fn test_omnibc_simd_vector_4xf32_and_clang_intrinsics() {
        let ir = r#"
declare double @llvm.sqrt.f64(double)

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %va0 = insertelement <4 x float> zeroinitializer, float 2.0, i32 0
  %va1 = insertelement <4 x float> %va0, float 3.0, i32 1
  %vb0 = insertelement <4 x float> zeroinitializer, float 4.0, i32 0
  %vb1 = insertelement <4 x float> %vb0, float 5.0, i32 1
  %vprod = fmul <4 x float> %va1, %vb1
  %vsum = fadd <4 x float> %vprod, %va1
  %e0 = extractelement <4 x float> %vsum, i32 0
  %e1 = extractelement <4 x float> %vsum, i32 1
  %tot = fadd double %e0, %e1
  ; e0 = 2*4 + 2 = 10.0, e1 = 3*5 + 3 = 18.0 -> tot = 28.0; sqrt(16.0) = 4.0 -> 32
  %sq = call double @llvm.sqrt.f64(double 16.0)
  %final_f = fadd double %tot, %sq
  %final_i = fptosi double %final_f to i64
  %r32 = trunc i64 %final_i to i32
  ret i32 %r32
}
"#;
        let bc = compile_llvm_ir_to_bc(ir).expect("compile SIMD + intrinsic IR");
        let vm = OmniBcVm::new(&bc, &[]);
        let ret = vm.call_by_name("main", &[0, 0]).expect("SIMD VM exec");
        assert_eq!(ret, 32);
    }

    #[test]
    fn test_omnibc_polyglot_runtime_ffi_strings_and_slices() {
        let ir = r#"
@.str.a = private unnamed_addr constant [6 x i8] c"Omni_\00", align 1
@.str.b = private unnamed_addr constant [5 x i8] c"BCVM\00", align 1

declare i8* @__omni_str_concat(i8*, i8*)
declare i64 @__omni_str_len(i8*)
declare i8* @__omni_slice_create(i64)
declare void @__omni_slice_append(i8*, i64)
declare i64 @__omni_slice_len(i8*)

define i32 @main(i32 %argc, i8** %argv) {
entry:
  %pa = getelementptr inbounds ([6 x i8], [6 x i8]* @.str.a, i64 0, i64 0)
  %pb = getelementptr inbounds ([5 x i8], [5 x i8]* @.str.b, i64 0, i64 0)
  %cat = call i8* @__omni_str_concat(i8* %pa, i8* %pb)
  %slen = call i64 @__omni_str_len(i8* %cat)
  %sl = call i8* @__omni_slice_create(i64 4)
  call void @__omni_slice_append(i8* %sl, i64 10)
  call void @__omni_slice_append(i8* %sl, i64 20)
  %arr_len = call i64 @__omni_slice_len(i8* %sl)
  %sum = add i64 %slen, %arr_len
  %r = trunc i64 %sum to i32
  ret i32 %r
}
"#;
        let bc = compile_llvm_ir_to_bc(ir).expect("compile runtime FFI IR");
        let syms = omni_runtime::jit_runtime_symbols();
        let vm = OmniBcVm::new(&bc, &syms);
        let ret = vm.call_by_name("main", &[0, 0]).expect("FFI VM exec");
        // strlen("Omni_BCVM") == 9, slice_len == 2 -> 11
        assert_eq!(ret, 11);
    }

    #[test]
    fn test_omnibc_live_clang_emitted_c_ir_interpretation() {
        // Real Clang `-S -emit-llvm -O0` style IR (with opaque `ptr`, `dso_local`, `noundef`, `align 4`, `nsw`)
        let clang_ir = r#"
define dso_local i32 @sum_squares(i32 noundef %n) #0 {
entry:
  %n.addr = alloca i32, align 4
  %acc = alloca i32, align 4
  %i = alloca i32, align 4
  store i32 %n, ptr %n.addr, align 4
  store i32 0, ptr %acc, align 4
  store i32 1, ptr %i, align 4
  br label %for.cond

for.cond:
  %0 = load i32, ptr %i, align 4
  %1 = load i32, ptr %n.addr, align 4
  %cmp = icmp sle i32 %0, %1
  br i1 %cmp, label %for.body, label %for.end

for.body:
  %2 = load i32, ptr %i, align 4
  %3 = load i32, ptr %i, align 4
  %mul = mul nsw i32 %2, %3
  %4 = load i32, ptr %acc, align 4
  %add = add nsw i32 %4, %mul
  store i32 %add, ptr %acc, align 4
  %5 = load i32, ptr %i, align 4
  %inc = add nsw i32 %5, 1
  store i32 %inc, ptr %i, align 4
  br label %for.cond

for.end:
  %6 = load i32, ptr %acc, align 4
  ret i32 %6
}
"#;
        let bc = compile_llvm_ir_to_bc(clang_ir).expect("compile Clang -O0 opaque-ptr IR -> OmniBC");
        let vm = OmniBcVm::new(&bc, &[]);
        // 1^2 + 2^2 + 3^2 + 4^2 + 5^2 = 55
        let ret = vm.call_by_name("sum_squares", &[5]).expect("exec Clang C IR in OmniBC VM");
        assert_eq!(ret, 55);
    }

    #[test]
    fn test_omnibc_live_clang_subprocess_c_and_simd() {
        let clang_ok = std::process::Command::new("clang")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !clang_ok {
            return;
        }
        let c_src = r#"
typedef float f32x4 __attribute__((vector_size(16)));
int dot4_scaled(int scale) {
    f32x4 a = {1.0f, 2.0f, 3.0f, 4.0f};
    f32x4 b = {2.0f, 3.0f, 4.0f, 5.0f};
    f32x4 c = a * b;
    float sum = c[0] + c[1] + c[2] + c[3];
    return ((int)sum) * scale;
}
"#;
        let td = std::env::temp_dir();
        let c_path = td.join(format!("omnibc_clang_test_{}.c", std::process::id()));
        let ll_path = td.join(format!("omnibc_clang_test_{}.ll", std::process::id()));
        std::fs::write(&c_path, c_src).unwrap();
        let out = std::process::Command::new("clang")
            .args(["-S", "-emit-llvm", "-O0", "-o"])
            .arg(&ll_path)
            .arg(&c_path)
            .output()
            .unwrap();
        assert!(out.status.success(), "clang failed: {}", String::from_utf8_lossy(&out.stderr));
        let ll = std::fs::read_to_string(&ll_path).unwrap();
        let _ = std::fs::remove_file(&c_path);
        let _ = std::fs::remove_file(&ll_path);

        let bc = compile_llvm_ir_to_bc(&ll).expect("compile live Clang SIMD .ll -> OmniBC");
        let vm = OmniBcVm::new(&bc, &[]);
        // (1*2 + 2*3 + 3*4 + 4*5) = (2+6+12+20) = 40; 40 * 3 = 120
        let res = vm.call_by_name("dot4_scaled", &[3]).expect("exec live Clang SIMD in OmniBC");
        assert_eq!(res as i32, 120);
    }
}
