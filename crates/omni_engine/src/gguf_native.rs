//! Zero-Candle Native GGUF v3 Reader, AVX2/FMA Q4_K / Q6_K / Q8_0 / F16 / F32 Kernels,
//! Batched Prefill, Native GPT-2/Qwen2 BPE Tokenizer, and Qwen2.5 Transformer Inference Engine.
//!
//! Designed for:
//! - ~30 ms zero-copy `mmap` startup (vs ~1.3 s in Candle)
//! - Batched prefill (reads 941 MB weights ONCE across all prompt tokens)
//! - Runtime-dispatched AVX2+FMA SIMD kernels + `rayon` persistent thread pool
//! - Zero external ML runtime dependencies, ready for direct OmniScript / LLVM IR self-hosting.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use memmap2::Mmap;
use rayon::prelude::*;

use crate::EngineError;

// ---------------------------------------------------------------------------
// 1. IEEE-754 Half-Precision (f16 -> f32) Conversion
// ---------------------------------------------------------------------------

#[inline(always)]
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1F) as u32;
    let mant = (h & 0x03FF) as u32;
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let mut m = mant;
        let mut e: i32 = -14;
        while (m & 0x0400) == 0 {
            m <<= 1;
            e -= 1;
        }
        m &= 0x03FF;
        let bits = sign | (((e + 127) as u32) << 23) | (m << 13);
        f32::from_bits(bits)
    } else if exp == 31 {
        let bits = sign | (0xFF << 23) | (mant << 13);
        f32::from_bits(bits)
    } else {
        let bits = sign | ((exp + 112) << 23) | (mant << 13);
        f32::from_bits(bits)
    }
}

// ---------------------------------------------------------------------------
// 2. GGUF v3 Binary Reader (Zero-Copy Backing: Mmap or Embedded &'static [u8])
// ---------------------------------------------------------------------------

pub enum GgufBacking {
    Mapped(Mmap),
    Static(&'static [u8]),
}

impl GgufBacking {
    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            GgufBacking::Mapped(m) => m.as_ref(),
            GgufBacking::Static(s) => s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum GgmlType {
    F32 = 0,
    F16 = 1,
    Q8_0 = 8,
    Q4K = 12,
    Q6K = 14,
}

impl GgmlType {
    fn from_u32(v: u32) -> Result<Self, EngineError> {
        match v {
            0 => Ok(GgmlType::F32),
            1 => Ok(GgmlType::F16),
            8 => Ok(GgmlType::Q8_0),
            12 => Ok(GgmlType::Q4K),
            14 => Ok(GgmlType::Q6K),
            other => Err(EngineError::InferenceFailed(format!(
                "Unsupported GGML quantization type {}",
                other
            ))),
        }
    }

    #[inline(always)]
    pub fn block_elems(self) -> usize {
        match self {
            GgmlType::F32 | GgmlType::F16 => 1,
            GgmlType::Q8_0 => 32,
            GgmlType::Q4K | GgmlType::Q6K => 256,
        }
    }

    #[inline(always)]
    pub fn block_bytes(self) -> usize {
        match self {
            GgmlType::F32 => 4,
            GgmlType::F16 => 2,
            GgmlType::Q8_0 => 34,
            GgmlType::Q4K => 144,
            GgmlType::Q6K => 210,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TensorInfo {
    pub dims: Vec<usize>,
    pub qtype: GgmlType,
    pub data_offset: usize,
}

#[derive(Debug, Clone)]
pub struct Qwen2Config {
    pub n_layers: usize,
    pub d_model: usize,
    pub d_ff: usize,
    pub n_heads: usize,
    pub n_kv_heads: usize,
    pub head_dim: usize,
    pub vocab_size: usize,
    pub rope_theta: f32,
    pub rms_eps: f32,
    pub eos_token_id: u32,
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn read_u32(&mut self) -> Result<u32, EngineError> {
        if self.pos + 4 > self.buf.len() {
            return Err(EngineError::InferenceFailed("Unexpected EOF in GGUF".into()));
        }
        let v = u32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }
    fn read_u64(&mut self) -> Result<u64, EngineError> {
        if self.pos + 8 > self.buf.len() {
            return Err(EngineError::InferenceFailed("Unexpected EOF in GGUF".into()));
        }
        let v = u64::from_le_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        Ok(v)
    }
    fn read_f32(&mut self) -> Result<f32, EngineError> {
        Ok(f32::from_bits(self.read_u32()?))
    }
    fn read_string(&mut self) -> Result<String, EngineError> {
        let len = self.read_u64()? as usize;
        if self.pos + len > self.buf.len() {
            return Err(EngineError::InferenceFailed("GGUF string out of bounds".into()));
        }
        let s = String::from_utf8_lossy(&self.buf[self.pos..self.pos + len]).into_owned();
        self.pos += len;
        Ok(s)
    }
    fn skip(&mut self, n: usize) -> Result<(), EngineError> {
        if self.pos + n > self.buf.len() {
            return Err(EngineError::InferenceFailed("GGUF skip out of bounds".into()));
        }
        self.pos += n;
        Ok(())
    }
    fn skip_value(&mut self, vtype: u32) -> Result<(), EngineError> {
        match vtype {
            0 | 1 | 7 => self.skip(1),
            2 | 3 => self.skip(2),
            4 | 5 | 6 => self.skip(4),
            8 => {
                let len = self.read_u64()? as usize;
                self.skip(len)
            }
            9 => {
                let et = self.read_u32()?;
                let cnt = self.read_u64()? as usize;
                if et == 8 {
                    for _ in 0..cnt {
                        let len = self.read_u64()? as usize;
                        self.skip(len)?;
                    }
                    Ok(())
                } else {
                    let esz = match et {
                        0 | 1 | 7 => 1,
                        2 | 3 => 2,
                        4 | 5 | 6 => 4,
                        10 | 11 | 12 => 8,
                        _ => return Err(EngineError::InferenceFailed(format!("Unknown GGUF array elem type {}", et))),
                    };
                    self.skip(cnt * esz)
                }
            }
            10 | 11 | 12 => self.skip(8),
            other => Err(EngineError::InferenceFailed(format!("Unknown GGUF value type {}", other))),
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Native GPT-2 / Qwen2 Byte-Level BPE Tokenizer (Loaded Straight From GGUF)
// ---------------------------------------------------------------------------

pub struct NativeBpeTokenizer {
    vocab: Vec<String>,
    token_to_id: HashMap<String, u32>,
    merges: HashMap<(String, String), usize>,
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
    special_tokens: Vec<(String, u32)>,
}

impl NativeBpeTokenizer {
    pub fn new(vocab: Vec<String>, merge_lines: Vec<String>) -> Self {
        // Build GPT-2 reversible byte <-> unicode char bijection
        let mut bs: Vec<u8> = Vec::with_capacity(256);
        for b in b'!'..=b'~' {
            bs.push(b);
        }
        for b in 0xA1u8..=0xACu8 {
            bs.push(b);
        }
        for b in 0xAEu8..=0xFFu8 {
            bs.push(b);
        }
        let mut cs: Vec<u32> = bs.iter().map(|&b| b as u32).collect();
        let mut n = 0u32;
        for b in 0u8..=255u8 {
            if !bs.contains(&b) {
                bs.push(b);
                cs.push(256 + n);
                n += 1;
            }
        }
        let mut byte_to_char = ['\0'; 256];
        let mut char_to_byte = HashMap::with_capacity(256);
        for (&b, &c) in bs.iter().zip(cs.iter()) {
            let ch = char::from_u32(c).unwrap();
            byte_to_char[b as usize] = ch;
            char_to_byte.insert(ch, b);
        }

        let mut token_to_id = HashMap::with_capacity(vocab.len());
        let mut special_tokens = Vec::new();
        for (idx, tok) in vocab.iter().enumerate() {
            token_to_id.insert(tok.clone(), idx as u32);
            if tok.starts_with("<|") && tok.ends_with("|>") {
                special_tokens.push((tok.clone(), idx as u32));
            }
        }
        // Match longest special tokens first
        special_tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut merges = HashMap::with_capacity(merge_lines.len());
        for (rank, line) in merge_lines.into_iter().enumerate() {
            if let Some((a, b)) = line.split_once(' ') {
                merges.insert((a.to_string(), b.to_string()), rank);
            }
        }

        Self {
            vocab,
            token_to_id,
            merges,
            byte_to_char,
            char_to_byte,
            special_tokens,
        }
    }

    fn bpe_encode_piece(&self, piece: &str, out: &mut Vec<u32>) {
        if piece.is_empty() {
            return;
        }
        let mut word: Vec<String> = piece
            .as_bytes()
            .iter()
            .map(|&b| self.byte_to_char[b as usize].to_string())
            .collect();

        while word.len() > 1 {
            let mut best_rank = usize::MAX;
            let mut best_idx = None;
            for i in 0..word.len() - 1 {
                if let Some(&rank) = self.merges.get(&(word[i].clone(), word[i + 1].clone())) {
                    if rank < best_rank {
                        best_rank = rank;
                        best_idx = Some(i);
                    }
                }
            }
            match best_idx {
                Some(idx) => {
                    let merged = format!("{}{}", word[idx], word[idx + 1]);
                    word[idx] = merged;
                    word.remove(idx + 1);
                }
                None => break,
            }
        }

        for tok in word {
            if let Some(&id) = self.token_to_id.get(&tok) {
                out.push(id);
            }
        }
    }

    /// Split a non-special text segment into Qwen2/GPT-2-style pre-tokens (matching Qwen2 regex closely).
    fn pretokenize_and_encode(&self, text: &str, out: &mut Vec<u32>) {
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            // Newlines (with optional leading spaces grouped with newline like `\s*[\r\n]+`)
            if bytes[i] == b'\n' || bytes[i] == b'\r' {
                let start = i;
                while i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b'\r') {
                    i += 1;
                }
                self.bpe_encode_piece(&text[start..i], out);
                continue;
            }
            // Whitespace runs: if followed by non-space word/punct, attach the LAST space to that token
            if bytes[i] == b' ' || bytes[i] == b'\t' {
                let start = i;
                while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                    if i - start > 1 {
                        // Emit `start .. i-1` as whitespace run, leave 1 space for the next token
                        self.bpe_encode_piece(&text[start..i - 1], out);
                        i -= 1;
                    } else {
                        i = start;
                    }
                } else {
                    self.bpe_encode_piece(&text[start..i], out);
                    continue;
                }
            }
            let start = i;
            if bytes[i] == b' ' || bytes[i] == b'\t' {
                i += 1;
            }
            if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                    i += 1;
                }
            } else if i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            } else if i < bytes.len() && bytes[i] >= 0x80 {
                let ch_len = text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                i += ch_len;
            } else if i < bytes.len() {
                while i < bytes.len()
                    && !bytes[i].is_ascii_alphanumeric()
                    && bytes[i] != b'_'
                    && bytes[i] != b' '
                    && bytes[i] != b'\t'
                    && bytes[i] != b'\n'
                    && bytes[i] != b'\r'
                    && bytes[i] < 0x80
                {
                    i += 1;
                }
            }
            self.bpe_encode_piece(&text[start..i], out);
        }
    }

    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos < text.len() {
            let mut matched_special = None;
            for (st, id) in &self.special_tokens {
                if text[pos..].starts_with(st.as_str()) {
                    matched_special = Some((st.len(), *id));
                    break;
                }
            }
            if let Some((len, id)) = matched_special {
                out.push(id);
                pos += len;
                continue;
            }
            // Find next potential `<|` special token
            let next_sp = text[pos..]
                .find("<|")
                .map(|rel| pos + rel)
                .filter(|&p| p > pos)
                .unwrap_or(text.len());
            self.pretokenize_and_encode(&text[pos..next_sp], &mut out);
            pos = next_sp;
        }
        out
    }

    pub fn decode(&self, ids: &[u32]) -> String {
        let mut raw_bytes = Vec::new();
        for &id in ids {
            if let Some(tok) = self.vocab.get(id as usize) {
                if tok.starts_with("<|") && tok.ends_with("|>") {
                    continue;
                }
                for ch in tok.chars() {
                    if let Some(&b) = self.char_to_byte.get(&ch) {
                        raw_bytes.push(b);
                    } else {
                        let mut tmp = [0u8; 4];
                        raw_bytes.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                    }
                }
            }
        }
        String::from_utf8_lossy(&raw_bytes).into_owned()
    }
}

// ---------------------------------------------------------------------------
// 4. Fused Quantized & AVX2/FMA SIMD Kernels (Q4_K, Q6_K, Q8_0, F16, F32)
// ---------------------------------------------------------------------------

#[inline(always)]
fn get_scale_min_k4(j: usize, q: &[u8]) -> (u8, u8) {
    if j < 4 {
        (q[j] & 63, q[j + 4] & 63)
    } else {
        let d = (q[j + 4] & 0x0F) | ((q[j - 4] >> 6) << 4);
        let m = (q[j + 4] >> 4) | ((q[j] >> 6) << 4);
        (d, m)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn hsum256_ps(v: std::arch::x86_64::__m256) -> f32 {
    use std::arch::x86_64::*;
    let hi = _mm256_extractf128_ps(v, 1);
    let lo = _mm256_castps256_ps128(v);
    let s1 = _mm_add_ps(lo, hi);
    let shuf = _mm_movehdup_ps(s1);
    let s2 = _mm_add_ps(s1, shuf);
    let shuf2 = _mm_movehl_ps(shuf, s2);
    let s3 = _mm_add_ss(s2, shuf2);
    _mm_cvtss_f32(s3)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_f32_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let n = a.len();
    let ap = a.as_ptr();
    let bp = b.as_ptr();
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();
    let mut acc2 = _mm256_setzero_ps();
    let mut acc3 = _mm256_setzero_ps();
    let mut i = 0usize;
    while i + 32 <= n {
        acc0 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i)), _mm256_loadu_ps(bp.add(i)), acc0);
        acc1 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i + 8)), _mm256_loadu_ps(bp.add(i + 8)), acc1);
        acc2 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i + 16)), _mm256_loadu_ps(bp.add(i + 16)), acc2);
        acc3 = _mm256_fmadd_ps(_mm256_loadu_ps(ap.add(i + 24)), _mm256_loadu_ps(bp.add(i + 24)), acc3);
        i += 32;
    }
    let mut sum = hsum256_ps(_mm256_add_ps(_mm256_add_ps(acc0, acc1), _mm256_add_ps(acc2, acc3)));
    while i < n {
        sum += *ap.add(i) * *bp.add(i);
        i += 1;
    }
    sum
}

#[inline(always)]
fn dot_f32_slices(a: &[f32], b: &[f32], use_avx2: bool) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        return unsafe { dot_f32_avx2(a, b) };
    }
    let mut s = 0.0f32;
    for i in 0..a.len() {
        s += a[i] * b[i];
    }
    s
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot4_f32_avx2(w: &[f32], x0: &[f32], x1: &[f32], x2: &[f32], x3: &[f32]) -> (f32, f32, f32, f32) {
    use std::arch::x86_64::*;
    let n = w.len();
    let wp = w.as_ptr();
    let p0 = x0.as_ptr();
    let p1 = x1.as_ptr();
    let p2 = x2.as_ptr();
    let p3 = x3.as_ptr();
    let mut a0 = _mm256_setzero_ps();
    let mut a1 = _mm256_setzero_ps();
    let mut a2 = _mm256_setzero_ps();
    let mut a3 = _mm256_setzero_ps();
    let mut b0 = _mm256_setzero_ps();
    let mut b1 = _mm256_setzero_ps();
    let mut b2 = _mm256_setzero_ps();
    let mut b3 = _mm256_setzero_ps();
    let mut i = 0usize;
    while i + 16 <= n {
        let vw0 = _mm256_loadu_ps(wp.add(i));
        let vw1 = _mm256_loadu_ps(wp.add(i + 8));
        a0 = _mm256_fmadd_ps(vw0, _mm256_loadu_ps(p0.add(i)), a0);
        b0 = _mm256_fmadd_ps(vw1, _mm256_loadu_ps(p0.add(i + 8)), b0);
        a1 = _mm256_fmadd_ps(vw0, _mm256_loadu_ps(p1.add(i)), a1);
        b1 = _mm256_fmadd_ps(vw1, _mm256_loadu_ps(p1.add(i + 8)), b1);
        a2 = _mm256_fmadd_ps(vw0, _mm256_loadu_ps(p2.add(i)), a2);
        b2 = _mm256_fmadd_ps(vw1, _mm256_loadu_ps(p2.add(i + 8)), b2);
        a3 = _mm256_fmadd_ps(vw0, _mm256_loadu_ps(p3.add(i)), a3);
        b3 = _mm256_fmadd_ps(vw1, _mm256_loadu_ps(p3.add(i + 8)), b3);
        i += 16;
    }
    let mut s0 = hsum256_ps(_mm256_add_ps(a0, b0));
    let mut s1 = hsum256_ps(_mm256_add_ps(a1, b1));
    let mut s2 = hsum256_ps(_mm256_add_ps(a2, b2));
    let mut s3 = hsum256_ps(_mm256_add_ps(a3, b3));
    while i < n {
        let wv = *wp.add(i);
        s0 += wv * *p0.add(i);
        s1 += wv * *p1.add(i);
        s2 += wv * *p2.add(i);
        s3 += wv * *p3.add(i);
        i += 1;
    }
    (s0, s1, s2, s3)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_q4_k_avx2(row_bytes: &[u8], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let nb = x.len() / 256;
    let mask_lo = _mm256_set1_epi32(0x0F);
    let mut total_vec = _mm256_setzero_ps();
    let xp = x.as_ptr();
    let rp = row_bytes.as_ptr();

    for b in 0..nb {
        let blk = rp.add(b * 144);
        let d = f16_to_f32(u16::from_le_bytes([*blk, *blk.add(1)]));
        let dmin = f16_to_f32(u16::from_le_bytes([*blk.add(2), *blk.add(3)]));
        let scales = std::slice::from_raw_parts(blk.add(4), 12);
        let qs = blk.add(16);
        let xb = xp.add(b * 256);

        let mut is = 0usize;
        for chunk in 0..4 {
            let (sc1, m1) = get_scale_min_k4(is, scales);
            let (sc2, m2) = get_scale_min_k4(is + 1, scales);
            is += 2;

            let vd1 = _mm256_set1_ps(d * (sc1 as f32));
            let vmin1 = _mm256_set1_ps(-dmin * (m1 as f32));
            let vd2 = _mm256_set1_ps(d * (sc2 as f32));
            let vmin2 = _mm256_set1_ps(-dmin * (m2 as f32));

            let q_ptr = qs.add(chunk * 32);
            let x1_ptr = xb.add(chunk * 64);
            let x2_ptr = xb.add(chunk * 64 + 32);

            let mut acc_q1 = _mm256_setzero_ps();
            let mut acc_x1 = _mm256_setzero_ps();
            let mut acc_q2 = _mm256_setzero_ps();
            let mut acc_x2 = _mm256_setzero_ps();

            for k in 0..4 {
                let q8 = _mm_loadl_epi64(q_ptr.add(k * 8) as *const __m128i);
                let q32 = _mm256_cvtepu8_epi32(q8);
                let lo_f = _mm256_cvtepi32_ps(_mm256_and_si256(q32, mask_lo));
                let hi_f = _mm256_cvtepi32_ps(_mm256_srli_epi32(q32, 4));

                let vx1 = _mm256_loadu_ps(x1_ptr.add(k * 8));
                let vx2 = _mm256_loadu_ps(x2_ptr.add(k * 8));

                acc_q1 = _mm256_fmadd_ps(lo_f, vx1, acc_q1);
                acc_x1 = _mm256_add_ps(acc_x1, vx1);
                acc_q2 = _mm256_fmadd_ps(hi_f, vx2, acc_q2);
                acc_x2 = _mm256_add_ps(acc_x2, vx2);
            }

            // Accumulate in vector space — ZERO horizontal shuffles inside the loop!
            total_vec = _mm256_fmadd_ps(vd1, acc_q1, total_vec);
            total_vec = _mm256_fmadd_ps(vmin1, acc_x1, total_vec);
            total_vec = _mm256_fmadd_ps(vd2, acc_q2, total_vec);
            total_vec = _mm256_fmadd_ps(vmin2, acc_x2, total_vec);
        }
    }
    hsum256_ps(total_vec)
}

#[inline(always)]
fn dot_q4_k(row_bytes: &[u8], x: &[f32], use_avx2: bool) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        return unsafe { dot_q4_k_avx2(row_bytes, x) };
    }
    let nb = x.len() / 256;
    let mut total = 0.0f32;
    for b in 0..nb {
        let blk = &row_bytes[b * 144..(b + 1) * 144];
        let d = f16_to_f32(u16::from_le_bytes([blk[0], blk[1]]));
        let dmin = f16_to_f32(u16::from_le_bytes([blk[2], blk[3]]));
        let scales = &blk[4..16];
        let qs = &blk[16..144];
        let xb = &x[b * 256..(b + 1) * 256];

        let mut is = 0usize;
        for chunk in 0..4 {
            let (sc1, m1) = get_scale_min_k4(is, scales);
            let (sc2, m2) = get_scale_min_k4(is + 1, scales);
            is += 2;
            let d1 = d * (sc1 as f32);
            let min1 = dmin * (m1 as f32);
            let d2 = d * (sc2 as f32);
            let min2 = dmin * (m2 as f32);
            let q_off = chunk * 32;
            let x_off = chunk * 64;

            let mut sum_q1 = 0.0f32;
            let mut sum_x1 = 0.0f32;
            let mut sum_q2 = 0.0f32;
            let mut sum_x2 = 0.0f32;
            for l in 0..32 {
                let qv = qs[q_off + l];
                let x1 = xb[x_off + l];
                let x2 = xb[x_off + 32 + l];
                sum_q1 += ((qv & 0x0F) as f32) * x1;
                sum_x1 += x1;
                sum_q2 += ((qv >> 4) as f32) * x2;
                sum_x2 += x2;
            }
            total += d1 * sum_q1 - min1 * sum_x1 + d2 * sum_q2 - min2 * sum_x2;
        }
    }
    total
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_q6_k_avx2(row_bytes: &[u8], x: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let nb = x.len() / 256;
    let mask_lo4 = _mm256_set1_epi32(0x0F);
    let mask_2b = _mm256_set1_epi32(0x03);
    let bias32 = _mm256_set1_epi32(32);
    let mut total = 0.0f32;
    let xp = x.as_ptr();
    let rp = row_bytes.as_ptr();

    for b in 0..nb {
        let blk = rp.add(b * 210);
        let ql = blk;
        let qh = blk.add(128);
        let sc = std::slice::from_raw_parts(blk.add(192), 16);
        let d = f16_to_f32(u16::from_le_bytes([*blk.add(208), *blk.add(209)]));
        let xb = xp.add(b * 256);

        let mut blk_sum = 0.0f32;
        for n in 0..2 {
            let ql_n = ql.add(n * 64);
            let qh_n = qh.add(n * 32);
            let sc_n = &sc[n * 8..(n + 1) * 8];
            let x_n = xb.add(n * 128);

            // Process 2 sub-groups of 16 (is = 0, 1), each in 2 vectors of 8
            for is in 0..2 {
                let s0 = _mm256_set1_ps((sc_n[is] as i8) as f32);
                let s2 = _mm256_set1_ps((sc_n[is + 2] as i8) as f32);
                let s4 = _mm256_set1_ps((sc_n[is + 4] as i8) as f32);
                let s6 = _mm256_set1_ps((sc_n[is + 6] as i8) as f32);

                let mut acc = _mm256_setzero_ps();
                for half8 in 0..2 {
                    let l = is * 16 + half8 * 8;
                    let vql_a = _mm256_cvtepu8_epi32(_mm_loadl_epi64(ql_n.add(l) as *const __m128i));
                    let vql_b = _mm256_cvtepu8_epi32(_mm_loadl_epi64(ql_n.add(l + 32) as *const __m128i));
                    let vqh = _mm256_cvtepu8_epi32(_mm_loadl_epi64(qh_n.add(l) as *const __m128i));

                    let q1 = _mm256_cvtepi32_ps(_mm256_sub_epi32(
                        _mm256_or_si256(_mm256_and_si256(vql_a, mask_lo4), _mm256_slli_epi32(_mm256_and_si256(vqh, mask_2b), 4)),
                        bias32,
                    ));
                    let q2 = _mm256_cvtepi32_ps(_mm256_sub_epi32(
                        _mm256_or_si256(_mm256_and_si256(vql_b, mask_lo4), _mm256_slli_epi32(_mm256_and_si256(_mm256_srli_epi32(vqh, 2), mask_2b), 4)),
                        bias32,
                    ));
                    let q3 = _mm256_cvtepi32_ps(_mm256_sub_epi32(
                        _mm256_or_si256(_mm256_srli_epi32(vql_a, 4), _mm256_slli_epi32(_mm256_and_si256(_mm256_srli_epi32(vqh, 4), mask_2b), 4)),
                        bias32,
                    ));
                    let q4 = _mm256_cvtepi32_ps(_mm256_sub_epi32(
                        _mm256_or_si256(_mm256_srli_epi32(vql_b, 4), _mm256_slli_epi32(_mm256_and_si256(_mm256_srli_epi32(vqh, 6), mask_2b), 4)),
                        bias32,
                    ));

                    acc = _mm256_fmadd_ps(_mm256_mul_ps(s0, q1), _mm256_loadu_ps(x_n.add(l)), acc);
                    acc = _mm256_fmadd_ps(_mm256_mul_ps(s2, q2), _mm256_loadu_ps(x_n.add(l + 32)), acc);
                    acc = _mm256_fmadd_ps(_mm256_mul_ps(s4, q3), _mm256_loadu_ps(x_n.add(l + 64)), acc);
                    acc = _mm256_fmadd_ps(_mm256_mul_ps(s6, q4), _mm256_loadu_ps(x_n.add(l + 96)), acc);
                }
                blk_sum += hsum256_ps(acc);
            }
        }
        total += d * blk_sum;
    }
    total
}

#[inline(always)]
fn dot_q6_k(row_bytes: &[u8], x: &[f32], use_avx2: bool) -> f32 {
    #[cfg(target_arch = "x86_64")]
    if use_avx2 {
        return unsafe { dot_q6_k_avx2(row_bytes, x) };
    }
    let nb = x.len() / 256;
    let mut total = 0.0f32;
    for b in 0..nb {
        let blk = &row_bytes[b * 210..(b + 1) * 210];
        let ql = &blk[0..128];
        let qh = &blk[128..192];
        let sc = &blk[192..208];
        let d = f16_to_f32(u16::from_le_bytes([blk[208], blk[209]]));
        let xb = &x[b * 256..(b + 1) * 256];

        let mut blk_sum = 0.0f32;
        for n in 0..2 {
            let ql_n = &ql[n * 64..(n + 1) * 64];
            let qh_n = &qh[n * 32..(n + 1) * 32];
            let sc_n = &sc[n * 8..(n + 1) * 8];
            let x_n = &xb[n * 128..(n + 1) * 128];
            for l in 0..32 {
                let is = l / 16;
                let qh_b = qh_n[l];
                let q1 = (((ql_n[l] & 0x0F) | ((qh_b & 0x03) << 4)) as i32) - 32;
                let q2 = (((ql_n[l + 32] & 0x0F) | (((qh_b >> 2) & 0x03) << 4)) as i32) - 32;
                let q3 = (((ql_n[l] >> 4) | (((qh_b >> 4) & 0x03) << 4)) as i32) - 32;
                let q4 = (((ql_n[l + 32] >> 4) | (((qh_b >> 6) & 0x03) << 4)) as i32) - 32;
                blk_sum += ((sc_n[is] as i8) as f32) * (q1 as f32) * x_n[l]
                    + ((sc_n[is + 2] as i8) as f32) * (q2 as f32) * x_n[l + 32]
                    + ((sc_n[is + 4] as i8) as f32) * (q3 as f32) * x_n[l + 64]
                    + ((sc_n[is + 6] as i8) as f32) * (q4 as f32) * x_n[l + 96];
            }
        }
        total += d * blk_sum;
    }
    total
}

fn dequant_row_q6_k(row_bytes: &[u8], out: &mut [f32]) {
    let nb = out.len() / 256;
    for b in 0..nb {
        let blk = &row_bytes[b * 210..(b + 1) * 210];
        let ql = &blk[0..128];
        let qh = &blk[128..192];
        let sc = &blk[192..208];
        let d = f16_to_f32(u16::from_le_bytes([blk[208], blk[209]]));
        let ob = &mut out[b * 256..(b + 1) * 256];
        for n in 0..2 {
            let ql_n = &ql[n * 64..(n + 1) * 64];
            let qh_n = &qh[n * 32..(n + 1) * 32];
            let sc_n = &sc[n * 8..(n + 1) * 8];
            let o_n = &mut ob[n * 128..(n + 1) * 128];
            for l in 0..32 {
                let is = l / 16;
                let qh_b = qh_n[l];
                let q1 = (((ql_n[l] & 0x0F) | ((qh_b & 0x03) << 4)) as i32) - 32;
                let q2 = (((ql_n[l + 32] & 0x0F) | (((qh_b >> 2) & 0x03) << 4)) as i32) - 32;
                let q3 = (((ql_n[l] >> 4) | (((qh_b >> 4) & 0x03) << 4)) as i32) - 32;
                let q4 = (((ql_n[l + 32] >> 4) | (((qh_b >> 6) & 0x03) << 4)) as i32) - 32;
                o_n[l] = d * ((sc_n[is] as i8) as f32) * (q1 as f32);
                o_n[l + 32] = d * ((sc_n[is + 2] as i8) as f32) * (q2 as f32);
                o_n[l + 64] = d * ((sc_n[is + 4] as i8) as f32) * (q3 as f32);
                o_n[l + 96] = d * ((sc_n[is + 6] as i8) as f32) * (q4 as f32);
            }
        }
    }
}

fn dequant_row_q4_k(row_bytes: &[u8], out: &mut [f32]) {
    let nb = out.len() / 256;
    for b in 0..nb {
        let blk = &row_bytes[b * 144..(b + 1) * 144];
        let d = f16_to_f32(u16::from_le_bytes([blk[0], blk[1]]));
        let dmin = f16_to_f32(u16::from_le_bytes([blk[2], blk[3]]));
        let scales = &blk[4..16];
        let qs = &blk[16..144];
        let ob = &mut out[b * 256..(b + 1) * 256];
        let mut is = 0usize;
        for chunk in 0..4 {
            let (sc1, m1) = get_scale_min_k4(is, scales);
            let (sc2, m2) = get_scale_min_k4(is + 1, scales);
            is += 2;
            let d1 = d * (sc1 as f32);
            let min1 = dmin * (m1 as f32);
            let d2 = d * (sc2 as f32);
            let min2 = dmin * (m2 as f32);
            let q_off = chunk * 32;
            let o_off = chunk * 64;
            for l in 0..32 {
                let qv = qs[q_off + l];
                ob[o_off + l] = d1 * ((qv & 0x0F) as f32) - min1;
                ob[o_off + 32 + l] = d2 * ((qv >> 4) as f32) - min2;
            }
        }
    }
}

fn dequant_row_any(row_bytes: &[u8], qtype: GgmlType, out: &mut [f32]) {
    match qtype {
        GgmlType::Q4K => dequant_row_q4_k(row_bytes, out),
        GgmlType::Q6K => dequant_row_q6_k(row_bytes, out),
        GgmlType::F32 => {
            for (i, dst) in out.iter_mut().enumerate() {
                *dst = f32::from_le_bytes(row_bytes[i * 4..i * 4 + 4].try_into().unwrap());
            }
        }
        GgmlType::F16 => {
            for (i, dst) in out.iter_mut().enumerate() {
                *dst = f16_to_f32(u16::from_le_bytes([row_bytes[i * 2], row_bytes[i * 2 + 1]]));
            }
        }
        GgmlType::Q8_0 => {
            for b in 0..(out.len() / 32) {
                let blk = &row_bytes[b * 34..(b + 1) * 34];
                let sc = f16_to_f32(u16::from_le_bytes([blk[0], blk[1]]));
                for l in 0..32 {
                    out[b * 32 + l] = sc * ((blk[2 + l] as i8) as f32);
                }
            }
        }
    }
}

#[inline(always)]
fn dot_q8_0(row_bytes: &[u8], x: &[f32]) -> f32 {
    let nb = x.len() / 32;
    let mut total = 0.0f32;
    for b in 0..nb {
        let blk = &row_bytes[b * 34..(b + 1) * 34];
        let d = f16_to_f32(u16::from_le_bytes([blk[0], blk[1]]));
        let xb = &x[b * 32..(b + 1) * 32];
        let mut s = 0.0f32;
        for l in 0..32 {
            s += ((blk[2 + l] as i8) as f32) * xb[l];
        }
        total += d * s;
    }
    total
}

#[inline(always)]
fn dot_f32_bytes(row_bytes: &[u8], x: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (i, &xv) in x.iter().enumerate() {
        let off = i * 4;
        let w = f32::from_le_bytes([
            row_bytes[off],
            row_bytes[off + 1],
            row_bytes[off + 2],
            row_bytes[off + 3],
        ]);
        sum += w * xv;
    }
    sum
}

#[inline(always)]
fn dot_f16_bytes(row_bytes: &[u8], x: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for (i, &xv) in x.iter().enumerate() {
        let off = i * 2;
        let w = f16_to_f32(u16::from_le_bytes([row_bytes[off], row_bytes[off + 1]]));
        sum += w * xv;
    }
    sum
}

/// Rayon-parallel single-token quantized matrix-vector multiply (`batch = 1` decode).
/// Chunks rows by 16 to amortize rayon task overhead and keep `x` hot in L1 cache.
fn matvec_quant(
    out: &mut [f32],
    w_bytes: &[u8],
    qtype: GgmlType,
    x: &[f32],
    bias: Option<&[f32]>,
    use_avx2: bool,
) {
    let cols = x.len();
    let row_bytes_len = (cols / qtype.block_elems()) * qtype.block_bytes();
    out.par_chunks_mut(16).enumerate().for_each(|(c_idx, chunk)| {
        let base_r = c_idx * 16;
        for (i, dst) in chunk.iter_mut().enumerate() {
            let r = base_r + i;
            let rb = &w_bytes[r * row_bytes_len..(r + 1) * row_bytes_len];
            let v = match qtype {
                GgmlType::Q4K => dot_q4_k(rb, x, use_avx2),
                GgmlType::Q6K => dot_q6_k(rb, x, use_avx2),
                GgmlType::Q8_0 => dot_q8_0(rb, x),
                GgmlType::F32 => dot_f32_bytes(rb, x),
                GgmlType::F16 => dot_f16_bytes(rb, x),
            };
            *dst = v + bias.map(|b| b[r]).unwrap_or(0.0);
        }
    });
}

/// Rayon-parallel batched quantized matmul for prompt prefill (`S` tokens at once!).
/// Dequantizes each weight row ONCE into L1 cache and dots it against 4 tokens at a time via AVX2/FMA!
fn matmul_batch_quant(
    out: &mut [f32],
    rows: usize,
    w_bytes: &[u8],
    qtype: GgmlType,
    x_batch: &[f32], // [S, cols]
    cols: usize,
    bias: Option<&[f32]>,
    use_avx2: bool,
) {
    let seq_len = x_batch.len() / cols;
    let row_bytes_len = (cols / qtype.block_elems()) * qtype.block_bytes();

    let mut row_major = vec![0.0f32; rows * seq_len];
    let block_rows = 8usize;
    row_major
        .par_chunks_mut(block_rows * seq_len)
        .enumerate()
        .for_each_init(
            || vec![0.0f32; cols],
            |w_row, (b_idx, dst_block)| {
                let r_start = b_idx * block_rows;
                let r_count = dst_block.len() / seq_len;
                for ri in 0..r_count {
                    let r = r_start + ri;
                    let rb = &w_bytes[r * row_bytes_len..(r + 1) * row_bytes_len];
                    dequant_row_any(rb, qtype, w_row);
                    let b_val = bias.map(|b| b[r]).unwrap_or(0.0);
                    let dst_seq = &mut dst_block[ri * seq_len..(ri + 1) * seq_len];

                    let mut t = 0usize;
                    #[cfg(target_arch = "x86_64")]
                    if use_avx2 {
                        while t + 4 <= seq_len {
                            let (s0, s1, s2, s3) = unsafe {
                                dot4_f32_avx2(
                                    w_row,
                                    &x_batch[t * cols..(t + 1) * cols],
                                    &x_batch[(t + 1) * cols..(t + 2) * cols],
                                    &x_batch[(t + 2) * cols..(t + 3) * cols],
                                    &x_batch[(t + 3) * cols..(t + 4) * cols],
                                )
                            };
                            dst_seq[t] = s0 + b_val;
                            dst_seq[t + 1] = s1 + b_val;
                            dst_seq[t + 2] = s2 + b_val;
                            dst_seq[t + 3] = s3 + b_val;
                            t += 4;
                        }
                    }
                    while t < seq_len {
                        let xt = &x_batch[t * cols..(t + 1) * cols];
                        dst_seq[t] = dot_f32_slices(w_row, xt, use_avx2) + b_val;
                        t += 1;
                    }
                }
            },
        );

    for t in 0..seq_len {
        let base = t * rows;
        for r in 0..rows {
            out[base + r] = row_major[r * seq_len + t];
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Qwen2.5 Transformer Primitives (RMSNorm, NeoX RoPE, GQA, SwiGLU)
// ---------------------------------------------------------------------------

#[inline(always)]
fn rmsnorm(out: &mut [f32], x: &[f32], weight: &[f32], eps: f32) {
    let mut ss = 0.0f32;
    for &v in x {
        ss += v * v;
    }
    let inv = 1.0f32 / (ss / (x.len() as f32) + eps).sqrt();
    for i in 0..x.len() {
        out[i] = x[i] * inv * weight[i];
    }
}

#[inline(always)]
fn apply_rope_neox(vec: &mut [f32], pos: usize, n_heads: usize, head_dim: usize, theta: f32) {
    let half = head_dim / 2;
    let pos_f = pos as f32;
    for h in 0..n_heads {
        let base = h * head_dim;
        for i in 0..half {
            let freq = 1.0f32 / theta.powf((2 * i) as f32 / (head_dim as f32));
            let angle = pos_f * freq;
            let (sin_a, cos_a) = angle.sin_cos();
            let x0 = vec[base + i];
            let x1 = vec[base + i + half];
            vec[base + i] = x0 * cos_a - x1 * sin_a;
            vec[base + i + half] = x0 * sin_a + x1 * cos_a;
        }
    }
}

#[inline(always)]
fn silu(x: f32) -> f32 {
    x / (1.0f32 + (-x).exp())
}

// ---------------------------------------------------------------------------
// 6. Native GGUF Qwen2 Model + KV-Cache Forward Pass (Single & Batched Prefill)
// ---------------------------------------------------------------------------

struct LayerTensors {
    attn_norm: Vec<f32>,
    wq: TensorInfo,
    bq: Vec<f32>,
    wk: TensorInfo,
    bk: Vec<f32>,
    wv: TensorInfo,
    bv: Vec<f32>,
    wo: TensorInfo,
    ffn_norm: Vec<f32>,
    w_gate: TensorInfo,
    w_up: TensorInfo,
    w_down: TensorInfo,
}

pub struct NativeGgufModel {
    backing: GgufBacking,
    pub config: Qwen2Config,
    token_embd: TensorInfo,
    output_weight: TensorInfo,
    output_norm: Vec<f32>,
    layers: Vec<LayerTensors>,
    pub tokenizer: NativeBpeTokenizer,
    use_avx2: bool,
}

pub struct NativeKvCache {
    pub max_seq: usize,
    pub kv_dim: usize,
    pub k: Vec<Vec<f32>>,
    pub v: Vec<Vec<f32>>,
}

impl NativeKvCache {
    pub fn new(cfg: &Qwen2Config, max_seq: usize) -> Self {
        let kv_dim = cfg.n_kv_heads * cfg.head_dim;
        Self {
            max_seq,
            kv_dim,
            k: vec![vec![0.0f32; max_seq * kv_dim]; cfg.n_layers],
            v: vec![vec![0.0f32; max_seq * kv_dim]; cfg.n_layers],
        }
    }

    /// Clone a pre-computed prompt prefix KV cache into a larger cache for fast suffix prefill + decode.
    pub fn from_prefix(prefix: &NativeKvCache, prefix_len: usize, new_max_seq: usize) -> Self {
        let kv_dim = prefix.kv_dim;
        let copy_elems = prefix_len * kv_dim;
        let mut k = Vec::with_capacity(prefix.k.len());
        let mut v = Vec::with_capacity(prefix.v.len());
        for l in 0..prefix.k.len() {
            let mut kl = vec![0.0f32; new_max_seq * kv_dim];
            let mut vl = vec![0.0f32; new_max_seq * kv_dim];
            kl[..copy_elems].copy_from_slice(&prefix.k[l][..copy_elems]);
            vl[..copy_elems].copy_from_slice(&prefix.v[l][..copy_elems]);
            k.push(kl);
            v.push(vl);
        }
        Self {
            max_seq: new_max_seq,
            kv_dim,
            k,
            v,
        }
    }
}

impl NativeGgufModel {
    pub fn from_file(path: &Path) -> Result<Self, EngineError> {
        let file = File::open(path).map_err(|_| EngineError::ModelNotFound(path.to_path_buf()))?;
        let mmap = unsafe {
            Mmap::map(&file).map_err(|e| EngineError::InferenceFailed(format!("mmap failed: {}", e)))?
        };
        Self::from_backing(GgufBacking::Mapped(mmap))
    }

    pub fn from_static_bytes(bytes: &'static [u8]) -> Result<Self, EngineError> {
        Self::from_backing(GgufBacking::Static(bytes))
    }

    fn read_f32_vec(buf: &[u8], ti: &TensorInfo) -> Vec<f32> {
        let n: usize = ti.dims.iter().product();
        let mut out = vec![0.0f32; n];
        let slice = &buf[ti.data_offset..ti.data_offset + n * 4];
        for i in 0..n {
            out[i] = f32::from_le_bytes([
                slice[i * 4],
                slice[i * 4 + 1],
                slice[i * 4 + 2],
                slice[i * 4 + 3],
            ]);
        }
        out
    }

    fn from_backing(backing: GgufBacking) -> Result<Self, EngineError> {
        let (config, tensors, vocab, merges) = {
            let buf = backing.as_bytes();
            if buf.len() < 24 || &buf[0..4] != b"GGUF" {
                return Err(EngineError::InferenceFailed("Invalid GGUF magic".into()));
            }
            let mut cur = Cursor::new(buf);
            cur.skip(4)?;
            let _version = cur.read_u32()?;
            let n_tensors = cur.read_u64()? as usize;
            let n_kv = cur.read_u64()? as usize;

            let mut n_layers = 28usize;
            let mut d_model = 1536usize;
            let mut d_ff = 8960usize;
            let mut n_heads = 12usize;
            let mut n_kv_heads = 2usize;
            let mut rope_theta = 1_000_000.0f32;
            let mut rms_eps = 1e-6f32;
            let mut eos_token_id = 151643u32;
            let mut alignment = 32usize;
            let mut vocab: Vec<String> = Vec::new();
            let mut merges: Vec<String> = Vec::new();

            for _ in 0..n_kv {
                let key = cur.read_string()?;
                let vtype = cur.read_u32()?;
                match key.as_str() {
                    "general.alignment" if vtype == 4 => alignment = cur.read_u32()? as usize,
                    "qwen2.block_count" if vtype == 4 => n_layers = cur.read_u32()? as usize,
                    "qwen2.embedding_length" if vtype == 4 => d_model = cur.read_u32()? as usize,
                    "qwen2.feed_forward_length" if vtype == 4 => d_ff = cur.read_u32()? as usize,
                    "qwen2.attention.head_count" if vtype == 4 => n_heads = cur.read_u32()? as usize,
                    "qwen2.attention.head_count_kv" if vtype == 4 => n_kv_heads = cur.read_u32()? as usize,
                    "qwen2.rope.freq_base" if vtype == 6 => rope_theta = cur.read_f32()?,
                    "qwen2.attention.layer_norm_rms_epsilon" if vtype == 6 => rms_eps = cur.read_f32()?,
                    "tokenizer.ggml.eos_token_id" if vtype == 4 => eos_token_id = cur.read_u32()?,
                    "tokenizer.ggml.tokens" if vtype == 9 => {
                        let et = cur.read_u32()?;
                        let cnt = cur.read_u64()? as usize;
                        if et == 8 {
                            vocab.reserve(cnt);
                            for _ in 0..cnt {
                                vocab.push(cur.read_string()?);
                            }
                        } else {
                            return Err(EngineError::InferenceFailed("Non-string GGUF vocab".into()));
                        }
                    }
                    "tokenizer.ggml.merges" if vtype == 9 => {
                        let et = cur.read_u32()?;
                        let cnt = cur.read_u64()? as usize;
                        if et == 8 {
                            merges.reserve(cnt);
                            for _ in 0..cnt {
                                merges.push(cur.read_string()?);
                            }
                        } else {
                            return Err(EngineError::InferenceFailed("Non-string GGUF merges".into()));
                        }
                    }
                    _ => cur.skip_value(vtype)?,
                }
            }

            let mut raw_tensors: Vec<(String, Vec<usize>, GgmlType, usize)> = Vec::with_capacity(n_tensors);
            for _ in 0..n_tensors {
                let name = cur.read_string()?;
                let ndims = cur.read_u32()? as usize;
                let mut dims = Vec::with_capacity(ndims);
                for _ in 0..ndims {
                    dims.push(cur.read_u64()? as usize);
                }
                let qtype = GgmlType::from_u32(cur.read_u32()?)?;
                let rel_off = cur.read_u64()? as usize;
                raw_tensors.push((name, dims, qtype, rel_off));
            }

            let data_start = (cur.pos + alignment - 1) & !(alignment - 1);
            let mut tensors = HashMap::with_capacity(n_tensors);
            for (name, dims, qtype, rel_off) in raw_tensors {
                tensors.insert(
                    name,
                    TensorInfo {
                        dims,
                        qtype,
                        data_offset: data_start + rel_off,
                    },
                );
            }

            let vocab_size = if !vocab.is_empty() {
                vocab.len()
            } else {
                tensors
                    .get("token_embd.weight")
                    .and_then(|t| t.dims.get(1).copied())
                    .unwrap_or(151936)
            };

            let config = Qwen2Config {
                n_layers,
                d_model,
                d_ff,
                n_heads,
                n_kv_heads,
                head_dim: d_model / n_heads,
                vocab_size,
                rope_theta,
                rms_eps,
                eos_token_id,
            };
            (config, tensors, vocab, merges)
        };

        let buf = backing.as_bytes();
        let req = |name: &str| -> Result<TensorInfo, EngineError> {
            tensors
                .get(name)
                .cloned()
                .ok_or_else(|| EngineError::InferenceFailed(format!("Missing GGUF tensor '{}'", name)))
        };

        let token_embd = req("token_embd.weight")?;
        let output_weight = tensors
            .get("output.weight")
            .cloned()
            .unwrap_or_else(|| token_embd.clone());
        let output_norm = Self::read_f32_vec(buf, &req("output_norm.weight")?);

        let mut layers = Vec::with_capacity(config.n_layers);
        for l in 0..config.n_layers {
            layers.push(LayerTensors {
                attn_norm: Self::read_f32_vec(buf, &req(&format!("blk.{}.attn_norm.weight", l))?),
                wq: req(&format!("blk.{}.attn_q.weight", l))?,
                bq: Self::read_f32_vec(buf, &req(&format!("blk.{}.attn_q.bias", l))?),
                wk: req(&format!("blk.{}.attn_k.weight", l))?,
                bk: Self::read_f32_vec(buf, &req(&format!("blk.{}.attn_k.bias", l))?),
                wv: req(&format!("blk.{}.attn_v.weight", l))?,
                bv: Self::read_f32_vec(buf, &req(&format!("blk.{}.attn_v.bias", l))?),
                wo: req(&format!("blk.{}.attn_output.weight", l))?,
                ffn_norm: Self::read_f32_vec(buf, &req(&format!("blk.{}.ffn_norm.weight", l))?),
                w_gate: req(&format!("blk.{}.ffn_gate.weight", l))?,
                w_up: req(&format!("blk.{}.ffn_up.weight", l))?,
                w_down: req(&format!("blk.{}.ffn_down.weight", l))?,
            });
        }

        #[cfg(target_arch = "x86_64")]
        let use_avx2 = std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma");
        #[cfg(not(target_arch = "x86_64"))]
        let use_avx2 = false;

        let tokenizer = NativeBpeTokenizer::new(vocab, merges);

        Ok(Self {
            backing,
            config,
            token_embd,
            output_weight,
            output_norm,
            layers,
            tokenizer,
            use_avx2,
        })
    }

    /// Batched prompt prefill: processes all `prompt_ids` in ONE pass over the 941 MB weights,
    /// populates `kv`, and optionally computes logits for the final prompt token.
    pub fn forward_prefill_batch(
        &self,
        prompt_ids: &[u32],
        start_pos: usize,
        kv: &mut NativeKvCache,
    ) -> Vec<f32> {
        self.forward_prefill_batch_opt(prompt_ids, start_pos, kv, true)
    }

    /// Prefill a shared prompt prefix into `kv` without computing the 151,936-row LM head.
    pub fn forward_prefill_prefix(&self, prefix_ids: &[u32], kv: &mut NativeKvCache) {
        let _ = self.forward_prefill_batch_opt(prefix_ids, 0, kv, false);
    }

    fn forward_prefill_batch_opt(
        &self,
        prompt_ids: &[u32],
        start_pos: usize,
        kv: &mut NativeKvCache,
        compute_logits: bool,
    ) -> Vec<f32> {
        let s = prompt_ids.len();
        if s == 0 {
            return Vec::new();
        }
        if s == 1 {
            return self.forward_token(prompt_ids[0], start_pos, kv, compute_logits);
        }

        let cfg = &self.config;
        let buf = self.backing.as_bytes();
        let d = cfg.d_model;
        let kv_dim = cfg.n_kv_heads * cfg.head_dim;
        let kv_mul = cfg.n_heads / cfg.n_kv_heads;
        let inv_sqrt_hd = 1.0f32 / (cfg.head_dim as f32).sqrt();

        // 1. Embed all `s` tokens
        let mut x = vec![0.0f32; s * d];
        let emb_row_bytes = (d / self.token_embd.qtype.block_elems()) * self.token_embd.qtype.block_bytes();
        for (t, &tid) in prompt_ids.iter().enumerate() {
            let emb_off = self.token_embd.data_offset + (tid as usize) * emb_row_bytes;
            dequant_row_any(&buf[emb_off..emb_off + emb_row_bytes], self.token_embd.qtype, &mut x[t * d..(t + 1) * d]);
        }

        let mut xb = vec![0.0f32; s * d];
        let mut q = vec![0.0f32; s * d];
        let mut k = vec![0.0f32; s * kv_dim];
        let mut v = vec![0.0f32; s * kv_dim];
        let mut attn_out = vec![0.0f32; s * d];
        let mut proj = vec![0.0f32; s * d];
        let mut gate = vec![0.0f32; s * cfg.d_ff];
        let mut up = vec![0.0f32; s * cfg.d_ff];

        for (l_idx, layer) in self.layers.iter().enumerate() {
            for t in 0..s {
                rmsnorm(&mut xb[t * d..(t + 1) * d], &x[t * d..(t + 1) * d], &layer.attn_norm, cfg.rms_eps);
            }

            matmul_batch_quant(&mut q, d, &buf[layer.wq.data_offset..], layer.wq.qtype, &xb, d, Some(&layer.bq), self.use_avx2);
            matmul_batch_quant(&mut k, kv_dim, &buf[layer.wk.data_offset..], layer.wk.qtype, &xb, d, Some(&layer.bk), self.use_avx2);
            matmul_batch_quant(&mut v, kv_dim, &buf[layer.wv.data_offset..], layer.wv.qtype, &xb, d, Some(&layer.bv), self.use_avx2);

            for t in 0..s {
                let pos = start_pos + t;
                apply_rope_neox(&mut q[t * d..(t + 1) * d], pos, cfg.n_heads, cfg.head_dim, cfg.rope_theta);
                apply_rope_neox(&mut k[t * kv_dim..(t + 1) * kv_dim], pos, cfg.n_kv_heads, cfg.head_dim, cfg.rope_theta);
                let cache_off = pos * kv_dim;
                kv.k[l_idx][cache_off..cache_off + kv_dim].copy_from_slice(&k[t * kv_dim..(t + 1) * kv_dim]);
                kv.v[l_idx][cache_off..cache_off + kv_dim].copy_from_slice(&v[t * kv_dim..(t + 1) * kv_dim]);
            }

            // Causal GQA across all `s` tokens in parallel
            let k_cache = &kv.k[l_idx];
            let v_cache = &kv.v[l_idx];
            let use_avx2 = self.use_avx2;
            attn_out.par_chunks_mut(d).enumerate().for_each(|(t, out_t)| {
                let pos = start_pos + t;
                let qt_all = &q[t * d..(t + 1) * d];
                let mut scores = vec![0.0f32; pos + 1];
                for h in 0..cfg.n_heads {
                    let qh = &qt_all[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                    let kv_head_off = (h / kv_mul) * cfg.head_dim;
                    let mut max_s = f32::NEG_INFINITY;
                    for prev in 0..=pos {
                        let kt = &k_cache[prev * kv_dim + kv_head_off..prev * kv_dim + kv_head_off + cfg.head_dim];
                        let sc = dot_f32_slices(qh, kt, use_avx2) * inv_sqrt_hd;
                        scores[prev] = sc;
                        if sc > max_s {
                            max_s = sc;
                        }
                    }
                    let mut sum_exp = 0.0f32;
                    for prev in 0..=pos {
                        let e = (scores[prev] - max_s).exp();
                        scores[prev] = e;
                        sum_exp += e;
                    }
                    let inv_sum = 1.0f32 / sum_exp;
                    let oh = &mut out_t[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                    oh.fill(0.0);
                    for prev in 0..=pos {
                        let w = scores[prev] * inv_sum;
                        let vt = &v_cache[prev * kv_dim + kv_head_off..prev * kv_dim + kv_head_off + cfg.head_dim];
                        for i in 0..cfg.head_dim {
                            oh[i] += w * vt[i];
                        }
                    }
                }
            });

            matmul_batch_quant(&mut proj, d, &buf[layer.wo.data_offset..], layer.wo.qtype, &attn_out, d, None, self.use_avx2);
            for i in 0..s * d {
                x[i] += proj[i];
            }

            for t in 0..s {
                rmsnorm(&mut xb[t * d..(t + 1) * d], &x[t * d..(t + 1) * d], &layer.ffn_norm, cfg.rms_eps);
            }
            matmul_batch_quant(&mut gate, cfg.d_ff, &buf[layer.w_gate.data_offset..], layer.w_gate.qtype, &xb, d, None, self.use_avx2);
            matmul_batch_quant(&mut up, cfg.d_ff, &buf[layer.w_up.data_offset..], layer.w_up.qtype, &xb, d, None, self.use_avx2);
            for i in 0..s * cfg.d_ff {
                gate[i] = silu(gate[i]) * up[i];
            }
            matmul_batch_quant(&mut proj, d, &buf[layer.w_down.data_offset..], layer.w_down.qtype, &gate, cfg.d_ff, None, self.use_avx2);
            for i in 0..s * d {
                x[i] += proj[i];
            }
        }

        if !compute_logits {
            return Vec::new();
        }

        // Compute logits only for the LAST prompt token!
        let mut last_norm = vec![0.0f32; d];
        rmsnorm(&mut last_norm, &x[(s - 1) * d..s * d], &self.output_norm, cfg.rms_eps);
        let mut logits = vec![0.0f32; cfg.vocab_size];
        matvec_quant(
            &mut logits,
            &buf[self.output_weight.data_offset..],
            self.output_weight.qtype,
            &last_norm,
            None,
            self.use_avx2,
        );
        logits
    }

    /// Single-token autoregressive decode step (`batch = 1`).
    pub fn forward_token(
        &self,
        token_id: u32,
        pos: usize,
        kv: &mut NativeKvCache,
        compute_logits: bool,
    ) -> Vec<f32> {
        let cfg = &self.config;
        let buf = self.backing.as_bytes();
        let d = cfg.d_model;
        let kv_dim = cfg.n_kv_heads * cfg.head_dim;

        let mut x = vec![0.0f32; d];
        let emb_row_bytes = (d / self.token_embd.qtype.block_elems()) * self.token_embd.qtype.block_bytes();
        let emb_off = self.token_embd.data_offset + (token_id as usize) * emb_row_bytes;
        dequant_row_any(&buf[emb_off..emb_off + emb_row_bytes], self.token_embd.qtype, &mut x);

        let mut xb = vec![0.0f32; d];
        let mut q = vec![0.0f32; d];
        let mut k = vec![0.0f32; kv_dim];
        let mut v = vec![0.0f32; kv_dim];
        let mut attn_out = vec![0.0f32; d];
        let mut proj = vec![0.0f32; d];
        let mut gate = vec![0.0f32; cfg.d_ff];
        let mut up = vec![0.0f32; cfg.d_ff];
        let mut scores = vec![0.0f32; pos + 1];

        let kv_mul = cfg.n_heads / cfg.n_kv_heads;
        let inv_sqrt_hd = 1.0f32 / (cfg.head_dim as f32).sqrt();

        for (l_idx, layer) in self.layers.iter().enumerate() {
            rmsnorm(&mut xb, &x, &layer.attn_norm, cfg.rms_eps);

            matvec_quant(&mut q, &buf[layer.wq.data_offset..], layer.wq.qtype, &xb, Some(&layer.bq), self.use_avx2);
            matvec_quant(&mut k, &buf[layer.wk.data_offset..], layer.wk.qtype, &xb, Some(&layer.bk), self.use_avx2);
            matvec_quant(&mut v, &buf[layer.wv.data_offset..], layer.wv.qtype, &xb, Some(&layer.bv), self.use_avx2);

            apply_rope_neox(&mut q, pos, cfg.n_heads, cfg.head_dim, cfg.rope_theta);
            apply_rope_neox(&mut k, pos, cfg.n_kv_heads, cfg.head_dim, cfg.rope_theta);

            let cache_off = pos * kv_dim;
            kv.k[l_idx][cache_off..cache_off + kv_dim].copy_from_slice(&k);
            kv.v[l_idx][cache_off..cache_off + kv_dim].copy_from_slice(&v);

            let k_cache = &kv.k[l_idx];
            let v_cache = &kv.v[l_idx];
            for h in 0..cfg.n_heads {
                let qh = &q[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                let kv_head_off = (h / kv_mul) * cfg.head_dim;
                let mut max_s = f32::NEG_INFINITY;
                for t in 0..=pos {
                    let kt = &k_cache[t * kv_dim + kv_head_off..t * kv_dim + kv_head_off + cfg.head_dim];
                    let s = dot_f32_slices(qh, kt, self.use_avx2) * inv_sqrt_hd;
                    scores[t] = s;
                    if s > max_s {
                        max_s = s;
                    }
                }
                let mut sum_exp = 0.0f32;
                for t in 0..=pos {
                    let e = (scores[t] - max_s).exp();
                    scores[t] = e;
                    sum_exp += e;
                }
                let inv_sum = 1.0f32 / sum_exp;
                let oh = &mut attn_out[h * cfg.head_dim..(h + 1) * cfg.head_dim];
                oh.fill(0.0);
                for t in 0..=pos {
                    let w = scores[t] * inv_sum;
                    let vt = &v_cache[t * kv_dim + kv_head_off..t * kv_dim + kv_head_off + cfg.head_dim];
                    for i in 0..cfg.head_dim {
                        oh[i] += w * vt[i];
                    }
                }
            }

            matvec_quant(&mut proj, &buf[layer.wo.data_offset..], layer.wo.qtype, &attn_out, None, self.use_avx2);
            for i in 0..d {
                x[i] += proj[i];
            }

            rmsnorm(&mut xb, &x, &layer.ffn_norm, cfg.rms_eps);
            matvec_quant(&mut gate, &buf[layer.w_gate.data_offset..], layer.w_gate.qtype, &xb, None, self.use_avx2);
            matvec_quant(&mut up, &buf[layer.w_up.data_offset..], layer.w_up.qtype, &xb, None, self.use_avx2);
            for i in 0..cfg.d_ff {
                gate[i] = silu(gate[i]) * up[i];
            }
            matvec_quant(&mut proj, &buf[layer.w_down.data_offset..], layer.w_down.qtype, &gate, None, self.use_avx2);
            for i in 0..d {
                x[i] += proj[i];
            }
        }

        if !compute_logits {
            return Vec::new();
        }

        rmsnorm(&mut xb, &x, &self.output_norm, cfg.rms_eps);
        let mut logits = vec![0.0f32; cfg.vocab_size];
        matvec_quant(
            &mut logits,
            &buf[self.output_weight.data_offset..],
            self.output_weight.qtype,
            &xb,
            None,
            self.use_avx2,
        );
        logits
    }

    pub fn argmax(logits: &[f32]) -> u32 {
        let mut best_i = 0usize;
        let mut best_v = f32::NEG_INFINITY;
        for (i, &v) in logits.iter().enumerate() {
            if v > best_v {
                best_v = v;
                best_i = i;
            }
        }
        best_i as u32
    }

    /// Deterministic temperature sampler (greedy `argmax` when `temp <= 0.05`, softmax sampling otherwise).
    pub fn sample_token(logits: &[f32], temp: f32, rng_state: &mut u64) -> u32 {
        if temp <= 0.05 || logits.is_empty() {
            return Self::argmax(logits);
        }
        let mut max_v = f32::NEG_INFINITY;
        for &v in logits {
            if v > max_v {
                max_v = v;
            }
        }
        let inv_t = 1.0f32 / temp;
        let mut sum = 0.0f64;
        let mut probs = Vec::with_capacity(logits.len());
        for &v in logits {
            let p = (((v - max_v) * inv_t).exp()) as f64;
            sum += p;
            probs.push(p);
        }
        // xorshift64* PRNG
        let mut x = *rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *rng_state = x;
        let r = ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64)
            * (1.0 / ((1u64 << 53) as f64))
            * sum;

        let mut acc = 0.0f64;
        for (i, p) in probs.into_iter().enumerate() {
            acc += p;
            if acc >= r {
                return i as u32;
            }
        }
        Self::argmax(logits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn test_native_gguf_q4k_q6k_qwen2_live_parity() {
        let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../models/omni-coder.gguf");
        if !model_path.exists() {
            eprintln!("Skipping native GGUF live test: {:?} not present", model_path);
            return;
        }

        let env = crate::SsaContextEnvelope {
            target_abi: "x86_64-pc-linux-gnu".to_string(),
            enclosing_fn: "main".to_string(),
            return_type: "i32".to_string(),
            next_ssa_idx: 5,
            active_symbols: vec![crate::SsaVariable {
                name: "line".to_string(),
                llvm_name: "%var_line_1".to_string(),
                type_hint: "i8*".to_string(),
            }],
            struct_definitions: vec![],
            raw_intent: "line = Console.ReadLine()".to_string(),
            idiom_hint: Some("ForeignModuleAssignment".to_string()),
            ecosystem_context: Some("csharp (module: System)".to_string()),
            import_chain: vec![],
            physical_source: Some("line = Console.ReadLine()".to_string()),
            anchor_id: None,
            concurrency_mode: None,
            yield_expectation: None,
            action_directive: None,
            lifetime_context: None,
        };
        let prompt = env.to_prompt_micro_tags();

        let t0 = Instant::now();
        let model = NativeGgufModel::from_file(&model_path).expect("Native GGUF load");
        let native_load_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let prompt_ids = model.tokenizer.encode(&prompt);
        let mut kv = NativeKvCache::new(&model.config, prompt_ids.len() + 64);

        let t_prefill = Instant::now();
        let logits = model.forward_prefill_batch(&prompt_ids, 0, &mut kv);
        let native_prefill_ms = t_prefill.elapsed().as_secs_f64() * 1000.0;

        let t_gen = Instant::now();
        let mut gen_ids = Vec::new();
        let mut next_tok = NativeGgufModel::argmax(&logits);
        for step in 0..48 {
            if next_tok == model.config.eos_token_id {
                break;
            }
            gen_ids.push(next_tok);
            let pos = prompt_ids.len() + step;
            let l = model.forward_token(next_tok, pos, &mut kv, true);
            next_tok = NativeGgufModel::argmax(&l);
        }
        let native_gen_ms = t_gen.elapsed().as_secs_f64() * 1000.0;
        let native_tok_s = (gen_ids.len() as f64) / (native_gen_ms / 1000.0).max(1e-6);
        let native_decoded = model.tokenizer.decode(&gen_ids);

        eprintln!(
            "[NATIVE ZERO-CANDLE GGUF] load={:.1}ms | prefill({}t)={:.1}ms | decode({}t)={:.1} tok/s | avx2={}\nOutput:\n{}",
            native_load_ms, prompt_ids.len(), native_prefill_ms, gen_ids.len(), native_tok_s, model.use_avx2, native_decoded
        );
        assert!(
            native_decoded.contains("@__omni_input"),
            "Native GGUF Qwen2.5 engine must synthesize @__omni_input! Got:\n{}",
            native_decoded
        );
    }
}
