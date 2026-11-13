use crate::shimmer::TargetArch;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Sentinel prefix written into GOT / IAT / unresolved external pointer slots.
/// Bits [63:48] = `0xFFFF`, bits [31:0] = external symbol table index.
pub const EXT_SENTINEL_BASE: u64 = 0xFFFF_0000_0000_0000;
pub const EXT_SENTINEL_MASK: u64 = 0xFFFF_FFFF_0000_0000;

#[inline]
pub fn is_ext_sentinel(addr: u64) -> bool {
    (addr & EXT_SENTINEL_MASK) == EXT_SENTINEL_BASE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryArch {
    X86_64,
    Aarch64,
}

impl BinaryArch {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinaryArch::X86_64 => "x86_64",
            BinaryArch::Aarch64 => "aarch64",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryAbi {
    /// System V AMD64 ABI (Linux, BSD, macOS x86_64): RDI, RSI, RDX, RCX, R8, R9
    SysV64,
    /// Microsoft x64 ABI (Windows PE/COFF x86_64): RCX, RDX, R8, R9 + 32-byte shadow space
    Win64,
    /// Standard ARM64 AAPCS64 (Linux, Windows ARM64): X0..X7
    Aapcs64,
    /// Apple ARM64 ABI (macOS/iOS Mach-O arm64): X0..X7, leading `_` symbol convention
    AppleArm64,
}

impl BinaryAbi {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinaryAbi::SysV64 => "sysv64",
            BinaryAbi::Win64 => "win64",
            BinaryAbi::Aapcs64 => "aapcs64",
            BinaryAbi::AppleArm64 => "apple-arm64",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedBinaryImage {
    pub format_label: String,
    pub arch: BinaryArch,
    pub abi: BinaryAbi,
    pub base_vma: u64,
    pub image_bytes: Vec<u8>,
    /// Half-open `[start_vma, end_vma)` executable code ranges.
    pub exec_ranges: Vec<(u64, u64)>,
    /// Canonical symbol name (and unprefixed Mach-O name) -> VMA in `image_bytes`.
    pub symbols: HashMap<String, u64>,
    /// VMA -> canonical symbol name for internal functions/labels.
    pub addr_to_sym: HashMap<u64, String>,
    /// Direct PLT / `__stubs` / PE import thunk VMA -> external symbol name.
    pub plt_stubs: HashMap<u64, String>,
    /// External sentinel address (`EXT_SENTINEL_BASE | idx`) -> external symbol name.
    pub ext_sentinels: HashMap<u64, String>,
    /// Reverse lookup: external symbol name -> sentinel address.
    pub ext_sym_to_sentinel: HashMap<String, u64>,
    /// Embedded LLVM bitcode section if fat LTO object.
    pub embedded_bitcode: Option<Vec<u8>>,
}

impl ParsedBinaryImage {
    pub fn read_bytes(&self, vma: u64, len: usize) -> Option<&[u8]> {
        if vma < self.base_vma {
            return None;
        }
        let off = (vma - self.base_vma) as usize;
        let end = off.checked_add(len)?;
        if end <= self.image_bytes.len() {
            Some(&self.image_bytes[off..end])
        } else {
            None
        }
    }

    pub fn read_u32_le(&self, vma: u64) -> Option<u32> {
        let b = self.read_bytes(vma, 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_u64_le(&self, vma: u64) -> Option<u64> {
        let b = self.read_bytes(vma, 8)?;
        Some(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn read_i32_le(&self, vma: u64) -> Option<i32> {
        self.read_u32_le(vma).map(|v| v as i32)
    }

    pub fn is_exec_vma(&self, vma: u64) -> bool {
        self.exec_ranges
            .iter()
            .any(|&(lo, hi)| vma >= lo && vma < hi)
    }

    pub fn is_mapped_vma(&self, vma: u64) -> bool {
        vma >= self.base_vma && (vma - self.base_vma) < self.image_bytes.len() as u64
    }

    pub fn intern_external_symbol(&mut self, raw_name: &str) -> u64 {
        let clean = normalize_ext_sym_name(raw_name);
        if let Some(&s) = self.ext_sym_to_sentinel.get(&clean) {
            return s;
        }
        let idx = (self.ext_sentinels.len() as u64) + 1;
        let sentinel = EXT_SENTINEL_BASE | idx;
        self.ext_sentinels.insert(sentinel, clean.clone());
        self.ext_sym_to_sentinel.insert(clean, sentinel);
        sentinel
    }

    pub fn resolve_symbol_vma(&self, sym: &str) -> Option<u64> {
        if let Some(&v) = self.symbols.get(sym) {
            return Some(v);
        }
        let with_underscore = format!("_{}", sym);
        if let Some(&v) = self.symbols.get(&with_underscore) {
            return Some(v);
        }
        if let Some(stripped) = sym.strip_prefix('_') {
            if let Some(&v) = self.symbols.get(stripped) {
                return Some(v);
            }
        }
        None
    }
}

#[derive(Debug)]
pub enum BinaryLoadOutcome {
    Image(ParsedBinaryImage),
    Bitcode(Vec<u8>),
    /// COFF / MinGW `.dll.a` or `.lib` containing only import descriptors (`__imp_*`) and no code.
    ImportLibraryOnly { dll_names: Vec<String> },
}

pub fn normalize_ext_sym_name(raw: &str) -> String {
    let mut s = raw.trim();
    // Strip ELF symbol version suffix (`memcpy@@GLIBC_2.14` or `malloc@GLIBC_2.2.5`)
    if let Some(at_idx) = s.find('@') {
        if at_idx > 0 {
            s = &s[..at_idx];
        }
    }
    // Strip PE `__imp_` prefix
    if let Some(rest) = s.strip_prefix("__imp_") {
        s = rest;
    }
    // Strip Mach-O single leading underscore on standard C/libc names (`_malloc` -> `malloc`,
    // `___assert_rtn` -> `__assert_rtn`, `___chkstk_darwin` -> `__chkstk_darwin`)
    if s.starts_with("___") {
        s = &s[1..];
    } else if let Some(rest) = s.strip_prefix('_') {
        if !rest.starts_with('_') && !rest.is_empty() {
            s = rest;
        }
    }
    s.to_string()
}

#[inline]
fn rd_u16_le(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

#[inline]
fn rd_u32_le(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
fn rd_u32_be(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
fn rd_u64_le(d: &[u8], off: usize) -> Option<u64> {
    let b = d.get(off..off.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

#[inline]
fn rd_i64_le(d: &[u8], off: usize) -> Option<i64> {
    rd_u64_le(d, off).map(|v| v as i64)
}

#[inline]
fn wr_u32_le(d: &mut [u8], off: usize, val: u32) {
    if let Some(dst) = d.get_mut(off..off.saturating_add(4)) {
        dst.copy_from_slice(&val.to_le_bytes());
    }
}

#[inline]
fn wr_u64_le(d: &mut [u8], off: usize, val: u64) {
    if let Some(dst) = d.get_mut(off..off.saturating_add(8)) {
        dst.copy_from_slice(&val.to_le_bytes());
    }
}

fn rd_cstr(d: &[u8], off: usize) -> String {
    if off >= d.len() {
        return String::new();
    }
    let slice = &d[off..];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

#[inline]
fn align_up(val: u64, align: u64) -> u64 {
    let a = align.max(1);
    (val + a - 1) & !(a - 1)
}

/// Parse any binary/archive/bitcode payload in pure Rust.
pub fn load_binary_payload(
    bytes: &[u8],
    preferred_arch: TargetArch,
    requested_symbols: &[String],
) -> Result<BinaryLoadOutcome, String> {
    if bytes.len() < 4 {
        return Err("Binary file too short (< 4 bytes)".to_string());
    }
    // Raw LLVM bitcode (`BC\xc0\xde`) or Darwin bitcode wrapper (`\xde\xc0\x17\x0b`)
    if &bytes[0..4] == b"BC\xc0\xde" || &bytes[0..4] == b"\xde\xc0\x17\x0b" {
        return Ok(BinaryLoadOutcome::Bitcode(bytes.to_vec()));
    }
    // Unix / BSD / COFF `ar` archive (`!<arch>\n`)
    if bytes.len() >= 8 && &bytes[0..8] == b"!<arch>\n" {
        return parse_archive(bytes, preferred_arch, requested_symbols);
    }
    // ELF64
    if &bytes[0..4] == b"\x7fELF" {
        let img = parse_elf64(bytes)?;
        if let Some(bc) = img.embedded_bitcode.clone() {
            return Ok(BinaryLoadOutcome::Bitcode(bc));
        }
        return Ok(BinaryLoadOutcome::Image(img));
    }
    // Mach-O 64 (`0xFEEDFACF`) or Universal Fat (`0xCAFEBABE` / `0xBEBAFECA`)
    let magic_le = rd_u32_le(bytes, 0).unwrap_or(0);
    let magic_be = rd_u32_be(bytes, 0).unwrap_or(0);
    if magic_le == 0xFEED_FACF || magic_be == 0xCAFE_BABE || magic_be == 0xCAFE_BABF {
        let img = parse_macho(bytes, preferred_arch)?;
        if let Some(bc) = img.embedded_bitcode.clone() {
            return Ok(BinaryLoadOutcome::Bitcode(bc));
        }
        return Ok(BinaryLoadOutcome::Image(img));
    }
    // PE32+ (`MZ`)
    if &bytes[0..2] == b"MZ" {
        let img = parse_pe_dll(bytes)?;
        return Ok(BinaryLoadOutcome::Image(img));
    }
    // Raw COFF `.obj` (`0x8664` AMD64 or `0xAA64` ARM64)
    let machine = rd_u16_le(bytes, 0).unwrap_or(0);
    if machine == 0x8664 || machine == 0xAA64 {
        let img = parse_coff_objects(&[("object.obj".to_string(), bytes)])?;
        return Ok(BinaryLoadOutcome::Image(img));
    }

    Err("Unrecognized binary magic".to_string())
}

// ============================================================================
// 1. Archive (`.a` GNU/BSD & `.lib` COFF) Parser + Multi-Object Linker
// ============================================================================

struct RawArMember<'a> {
    name: String,
    data: &'a [u8],
}

fn extract_ar_members(bytes: &[u8]) -> Result<Vec<RawArMember<'_>>, String> {
    if bytes.len() < 8 || &bytes[0..8] != b"!<arch>\n" {
        return Err("Invalid ar header".to_string());
    }
    let mut pos = 8usize;
    let mut gnu_strtab: Option<&[u8]> = None;
    let mut out = Vec::new();

    while pos + 60 <= bytes.len() {
        let hdr = &bytes[pos..pos + 60];
        if &hdr[58..60] != b"`\n" {
            // Re-align on 2-byte boundary if needed
            pos += 1;
            continue;
        }
        let raw_name = std::str::from_utf8(&hdr[0..16]).unwrap_or("").trim();
        let size_str = std::str::from_utf8(&hdr[48..58]).unwrap_or("").trim();
        let size: usize = size_str.parse().unwrap_or(0);
        let data_start = pos + 60;
        let data_end = data_start.saturating_add(size).min(bytes.len());
        let mut payload = &bytes[data_start..data_end];
        pos = data_end + (data_end & 1);

        if raw_name == "//" {
            gnu_strtab = Some(payload);
            continue;
        }
        if raw_name == "/" || raw_name == "/SYM64/" || raw_name.starts_with("__.SYMDEF") {
            continue;
        }

        let name = if let Some(bsd_len_str) = raw_name.strip_prefix("#1/") {
            let nlen: usize = bsd_len_str.trim().parse().unwrap_or(0);
            if nlen <= payload.len() {
                let n = String::from_utf8_lossy(&payload[..nlen])
                    .trim_end_matches('\0')
                    .to_string();
                payload = &payload[nlen..];
                if n.starts_with("__.SYMDEF") {
                    continue;
                }
                n
            } else {
                raw_name.to_string()
            }
        } else if let Some(off_str) = raw_name.strip_prefix('/') {
            if let (Ok(off), Some(tab)) = (off_str.trim().parse::<usize>(), gnu_strtab) {
                if off < tab.len() {
                    let rest = &tab[off..];
                    let end = rest
                        .iter()
                        .position(|&b| b == b'\n' || b == b'/' || b == 0)
                        .unwrap_or(rest.len());
                    String::from_utf8_lossy(&rest[..end]).into_owned()
                } else {
                    raw_name.trim_end_matches('/').to_string()
                }
            } else {
                raw_name.trim_end_matches('/').to_string()
            }
        } else {
            raw_name.trim_end_matches('/').to_string()
        };

        if !payload.is_empty() {
            out.push(RawArMember {
                name,
                data: payload,
            });
        }
    }

    Ok(out)
}

fn is_coff_import_object(data: &[u8]) -> Option<String> {
    if data.len() < 20 {
        return None;
    }
    let sig1 = rd_u16_le(data, 0)?;
    let sig2 = rd_u16_le(data, 2)?;
    if sig1 == 0x0000 && sig2 == 0xFFFF {
        let size_of_data = rd_u32_le(data, 12)? as usize;
        let strings = data.get(20..20usize.saturating_add(size_of_data))?;
        // Strings contains `symbol_name\0dll_name\0`
        let nul1 = strings.iter().position(|&b| b == 0)?;
        if nul1 + 1 < strings.len() {
            let dll = rd_cstr(strings, nul1 + 1);
            return Some(dll);
        }
        return Some(String::new());
    }
    None
}

fn parse_archive(
    bytes: &[u8],
    preferred_arch: TargetArch,
    requested_symbols: &[String],
) -> Result<BinaryLoadOutcome, String> {
    // Handle Mach-O Universal Fat archive (`0xCAFEBABE` wrapping multiple `!<arch>\n` slices)
    let members = extract_ar_members(bytes)?;
    if members.is_empty() {
        return Err("Archive contains no object members".to_string());
    }

    let mut dll_imports = HashSet::new();
    let mut elf_objs: Vec<(String, &[u8])> = Vec::new();
    let mut macho_objs: Vec<(String, Vec<u8>)> = Vec::new();
    let mut coff_objs: Vec<(String, &[u8])> = Vec::new();

    for m in &members {
        let d = m.data;
        if d.len() >= 4 && (&d[0..4] == b"BC\xc0\xde" || &d[0..4] == b"\xde\xc0\x17\x0b") {
            return Ok(BinaryLoadOutcome::Bitcode(d.to_vec()));
        }
        if let Some(dll) = is_coff_import_object(d) {
            if !dll.is_empty() {
                dll_imports.insert(dll);
            }
            continue;
        }
        if d.len() >= 4 && &d[0..4] == b"\x7fELF" {
            elf_objs.push((m.name.clone(), d));
        } else if d.len() >= 4
            && (rd_u32_le(d, 0) == Some(0xFEED_FACF) || rd_u32_be(d, 0) == Some(0xCAFE_BABE))
        {
            let slice = unwrap_macho_fat_slice(d, preferred_arch)?;
            macho_objs.push((m.name.clone(), slice.to_vec()));
        } else if d.len() >= 20 {
            let mach = rd_u16_le(d, 0).unwrap_or(0);
            if mach == 0x8664 || mach == 0xAA64 {
                // Distinguish real COFF code objects from MinGW `.idata`-only import stubs (`lib*.dll.a`)
                if coff_member_has_code_section(d) {
                    coff_objs.push((m.name.clone(), d));
                } else if let Some(dll) = extract_mingw_idata_dll_name(d) {
                    dll_imports.insert(dll);
                }
            }
        }
    }

    if elf_objs.is_empty() && macho_objs.is_empty() && coff_objs.is_empty() {
        if !dll_imports.is_empty() {
            let mut dll_names: Vec<String> = dll_imports.into_iter().collect();
            dll_names.sort();
            return Ok(BinaryLoadOutcome::ImportLibraryOnly { dll_names });
        }
        return Err("Archive contained no liftable ELF64/Mach-O64/COFF64 code objects".to_string());
    }

    if !elf_objs.is_empty() {
        let selected = select_reachable_elf_objects(&elf_objs, requested_symbols);
        let img = link_elf64_relocatable_objects(&selected)?;
        return Ok(BinaryLoadOutcome::Image(img));
    }
    if !macho_objs.is_empty() {
        let refs: Vec<(String, &[u8])> = macho_objs
            .iter()
            .map(|(n, b)| (n.clone(), b.as_slice()))
            .collect();
        let img = link_macho64_objects(&refs)?;
        return Ok(BinaryLoadOutcome::Image(img));
    }
    let img = parse_coff_objects(&coff_objs)?;
    Ok(BinaryLoadOutcome::Image(img))
}

fn coff_member_has_code_section(d: &[u8]) -> bool {
    let Some(nsects) = rd_u16_le(d, 2) else {
        return false;
    };
    let Some(opt_sz) = rd_u16_le(d, 16) else {
        return false;
    };
    let sect_base = 20usize + opt_sz as usize;
    for i in 0..(nsects as usize) {
        let off = sect_base + i * 40;
        if off + 40 > d.len() {
            break;
        }
        let name = std::str::from_utf8(&d[off..off + 8])
            .unwrap_or("")
            .trim_end_matches('\0');
        let raw_sz = rd_u32_le(d, off + 16).unwrap_or(0);
        let chars = rd_u32_le(d, off + 36).unwrap_or(0);
        // IMAGE_SCN_CNT_CODE = 0x20; exclude 6-byte `.text` import jump thunks in `.idata` members
        if (chars & 0x20) != 0 && raw_sz > 0 && !name.starts_with(".idata") {
            // Check if this member also has `.idata$` sections (MinGW import thunk object!)
            let has_idata = (0..nsects as usize).any(|j| {
                let joff = sect_base + j * 40;
                joff + 8 <= d.len() && d[joff..joff + 8].starts_with(b".idata")
            });
            if !has_idata || raw_sz > 16 {
                return true;
            }
        }
    }
    false
}

fn extract_mingw_idata_dll_name(d: &[u8]) -> Option<String> {
    // Scan ASCII strings ending in `.dll` inside `.idata$7`
    let text = String::from_utf8_lossy(d);
    for part in text.split('\0') {
        let trimmed = part.trim();
        if trimmed.to_ascii_lowercase().ends_with(".dll")
            && trimmed.len() <= 64
            && trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Some(trimmed.to_string());
        }
    }
    None
}

// ============================================================================
// 2. ELF64 Parser (`.so` Shared Objects & Multi-`.o` Relocatable Linker)
// ============================================================================

#[derive(Clone)]
struct ElfSym {
    name: String,
    _info: u8,
    shndx: u16,
    value: u64,
}

fn parse_elf64_symbols_only(d: &[u8]) -> (HashSet<String>, HashSet<String>) {
    let mut defined = HashSet::new();
    let mut undefined = HashSet::new();
    if d.len() < 64 || d[4] != 2 || d[5] != 1 {
        return (defined, undefined);
    }
    let shoff = rd_u64_le(d, 40).unwrap_or(0) as usize;
    let shentsize = rd_u16_le(d, 58).unwrap_or(0) as usize;
    let shnum = rd_u16_le(d, 60).unwrap_or(0) as usize;
    if shoff == 0 || shentsize < 64 {
        return (defined, undefined);
    }
    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        if sh + 64 > d.len() {
            break;
        }
        let sh_type = rd_u32_le(d, sh + 4).unwrap_or(0);
        if sh_type != 2 {
            continue; // SHT_SYMTAB
        }
        let sym_off = rd_u64_le(d, sh + 24).unwrap_or(0) as usize;
        let sym_sz = rd_u64_le(d, sh + 32).unwrap_or(0) as usize;
        let link = rd_u32_le(d, sh + 40).unwrap_or(0) as usize;
        let str_sh = shoff + link * shentsize;
        if str_sh + 64 > d.len() {
            continue;
        }
        let str_off = rd_u64_le(d, str_sh + 24).unwrap_or(0) as usize;
        let str_sz = rd_u64_le(d, str_sh + 32).unwrap_or(0) as usize;
        let Some(strtab) = d.get(str_off..str_off.saturating_add(str_sz)) else {
            continue;
        };
        let nsyms = sym_sz / 24;
        for s in 0..nsyms {
            let so = sym_off + s * 24;
            if so + 24 > d.len() {
                break;
            }
            let st_name = rd_u32_le(d, so).unwrap_or(0) as usize;
            let st_info = d[so + 4];
            let st_shndx = rd_u16_le(d, so + 6).unwrap_or(0);
            let bind = st_info >> 4;
            if bind == 0 {
                continue; // STB_LOCAL
            }
            let name = rd_cstr(strtab, st_name);
            if name.is_empty() {
                continue;
            }
            if st_shndx == 0 {
                undefined.insert(name);
            } else {
                defined.insert(name);
            }
        }
    }
    (defined, undefined)
}

fn select_reachable_elf_objects<'a>(
    objs: &[(String, &'a [u8])],
    requested: &[String],
) -> Vec<(String, &'a [u8])> {
    if requested.is_empty() || objs.len() <= 1 {
        return objs.to_vec();
    }
    let meta: Vec<(HashSet<String>, HashSet<String>)> =
        objs.iter().map(|(_, b)| parse_elf64_symbols_only(b)).collect();
    let mut chosen = vec![false; objs.len()];
    let mut needed: Vec<String> = requested.to_vec();
    let mut seen_syms: HashSet<String> = requested.iter().cloned().collect();

    while let Some(sym) = needed.pop() {
        for (idx, (defs, undefs)) in meta.iter().enumerate() {
            if defs.contains(&sym) && !chosen[idx] {
                chosen[idx] = true;
                for u in undefs {
                    if seen_syms.insert(u.clone()) {
                        needed.push(u.clone());
                    }
                }
            }
        }
    }

    let out: Vec<(String, &'a [u8])> = objs
        .iter()
        .enumerate()
        .filter_map(|(i, item)| if chosen[i] { Some(item.clone()) } else { None })
        .collect();
    if out.is_empty() {
        objs.to_vec()
    } else {
        out
    }
}

fn parse_elf64(d: &[u8]) -> Result<ParsedBinaryImage, String> {
    if d.len() < 64 || &d[0..4] != b"\x7fELF" {
        return Err("Invalid ELF header".to_string());
    }
    if d[4] != 2 || d[5] != 1 {
        return Err("Only 64-bit little-endian ELF is supported".to_string());
    }
    let e_type = rd_u16_le(d, 16).ok_or("Truncated ELF e_type")?;
    if e_type == 1 {
        return link_elf64_relocatable_objects(&[("object.o".to_string(), d)]);
    }
    let e_machine = rd_u16_le(d, 18).ok_or("Truncated ELF e_machine")?;
    let (arch, abi) = match e_machine {
        62 => (BinaryArch::X86_64, BinaryAbi::SysV64),
        183 => (BinaryArch::Aarch64, BinaryAbi::Aapcs64),
        other => return Err(format!("Unsupported ELF machine {}", other)),
    };

    let phoff = rd_u64_le(d, 32).unwrap_or(0) as usize;
    let shoff = rd_u64_le(d, 40).unwrap_or(0) as usize;
    let phentsize = rd_u16_le(d, 54).unwrap_or(0) as usize;
    let phnum = rd_u16_le(d, 56).unwrap_or(0) as usize;
    let shentsize = rd_u16_le(d, 58).unwrap_or(0) as usize;
    let shnum = rd_u16_le(d, 60).unwrap_or(0) as usize;
    let shstrndx = rd_u16_le(d, 62).unwrap_or(0) as usize;

    // Check embedded `.llvmbc` bitcode section first
    let mut embedded_bitcode = None;
    let mut shstrtab: &[u8] = &[];
    if shoff > 0 && shentsize >= 64 && shstrndx < shnum {
        let s_sh = shoff + shstrndx * shentsize;
        if s_sh + 64 <= d.len() {
            let off = rd_u64_le(d, s_sh + 24).unwrap_or(0) as usize;
            let sz = rd_u64_le(d, s_sh + 32).unwrap_or(0) as usize;
            if let Some(st) = d.get(off..off.saturating_add(sz)) {
                shstrtab = st;
            }
        }
    }

    // Gather PT_LOAD segments
    let mut loads = Vec::new();
    let mut min_vma = u64::MAX;
    let mut max_vma = 0u64;
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if ph + 56 > d.len() {
            break;
        }
        let p_type = rd_u32_le(d, ph).unwrap_or(0);
        if p_type != 1 {
            continue; // PT_LOAD = 1
        }
        let p_flags = rd_u32_le(d, ph + 4).unwrap_or(0);
        let p_offset = rd_u64_le(d, ph + 8).unwrap_or(0) as usize;
        let p_vaddr = rd_u64_le(d, ph + 16).unwrap_or(0);
        let p_filesz = rd_u64_le(d, ph + 32).unwrap_or(0) as usize;
        let p_memsz = rd_u64_le(d, ph + 40).unwrap_or(0);
        min_vma = min_vma.min(p_vaddr);
        max_vma = max_vma.max(p_vaddr.saturating_add(p_memsz));
        loads.push((p_vaddr, p_offset, p_filesz, p_memsz, p_flags));
    }
    if loads.is_empty() || max_vma <= min_vma {
        return Err("ELF64 shared object has no PT_LOAD segments".to_string());
    }

    let rebase_bias = if min_vma < 0x10000 { 0x1000_0000u64 } else { 0u64 };
    let base_vma = min_vma + rebase_bias;
    let total_span = (max_vma - min_vma) as usize;
    if total_span > 256 * 1024 * 1024 {
        return Err("ELF64 virtual image exceeds 256 MB safety bound".to_string());
    }
    // Reserve extra 64 bytes at end for synthetic `__stack_chk_guard` canary slot
    let canary_off = align_up(total_span as u64, 16) as usize;
    let mut image_bytes = vec![0u8; canary_off + 64];
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = base_vma + canary_off as u64;

    let mut exec_ranges = Vec::new();
    for &(p_vaddr, p_offset, p_filesz, p_memsz, p_flags) in &loads {
        let img_off = (p_vaddr - min_vma) as usize;
        if let Some(src) = d.get(p_offset..p_offset.saturating_add(p_filesz)) {
            if let Some(dst) = image_bytes.get_mut(img_off..img_off.saturating_add(src.len())) {
                dst.copy_from_slice(src);
            }
        }
        if (p_flags & 1) != 0 && p_memsz > 0 {
            exec_ranges.push((p_vaddr + rebase_bias, p_vaddr + rebase_bias + p_memsz));
        }
    }

    let mut img = ParsedBinaryImage {
        format_label: format!("ELF64-shared ({})", arch.as_str()),
        arch,
        abi,
        base_vma,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode: None,
    };

    // Parse section headers for `.dynsym`, `.symtab`, `.rela.*`, `.plt`, `.plt.sec`, `.llvmbc`
    let mut symtabs: HashMap<usize, Vec<ElfSym>> = HashMap::new();
    let mut rela_sections = Vec::new();
    let mut plt_sections = Vec::new();
    let mut sh_exec_ranges = Vec::new();

    for i in 0..shnum {
        let sh = shoff + i * shentsize;
        if sh + 64 > d.len() {
            break;
        }
        let sh_name_off = rd_u32_le(d, sh).unwrap_or(0) as usize;
        let sh_name = rd_cstr(shstrtab, sh_name_off);
        let sh_type = rd_u32_le(d, sh + 4).unwrap_or(0);
        let sh_flags = rd_u64_le(d, sh + 8).unwrap_or(0);
        let sh_addr = rd_u64_le(d, sh + 16).unwrap_or(0);
        let sh_offset = rd_u64_le(d, sh + 24).unwrap_or(0) as usize;
        let sh_size = rd_u64_le(d, sh + 32).unwrap_or(0) as usize;
        let sh_link = rd_u32_le(d, sh + 40).unwrap_or(0) as usize;

        if (sh_flags & 0x4) != 0 && sh_size > 0 && sh_addr >= min_vma {
            sh_exec_ranges.push((
                sh_addr + rebase_bias,
                sh_addr + rebase_bias + sh_size as u64,
            ));
        }

        if sh_name == ".llvmbc" {
            if let Some(bc) = d.get(sh_offset..sh_offset.saturating_add(sh_size)) {
                if !bc.is_empty() {
                    embedded_bitcode = Some(bc.to_vec());
                }
            }
        } else if sh_type == 2 || sh_type == 11 {
            // SHT_SYMTAB (2) or SHT_DYNSYM (11)
            let str_sh = shoff + sh_link * shentsize;
            if str_sh + 64 <= d.len() {
                let str_off = rd_u64_le(d, str_sh + 24).unwrap_or(0) as usize;
                let str_sz = rd_u64_le(d, str_sh + 32).unwrap_or(0) as usize;
                if let Some(strtab) = d.get(str_off..str_off.saturating_add(str_sz)) {
                    let mut syms = Vec::new();
                    let count = sh_size / 24;
                    for s in 0..count {
                        let so = sh_offset + s * 24;
                        if so + 24 > d.len() {
                            break;
                        }
                        let st_name = rd_u32_le(d, so).unwrap_or(0) as usize;
                        let st_info = d[so + 4];
                        let st_shndx = rd_u16_le(d, so + 6).unwrap_or(0);
                        let st_val = rd_u64_le(d, so + 8).unwrap_or(0);
                        let raw_nm = rd_cstr(strtab, st_name);
                        let clean_nm = if let Some(at) = raw_nm.find('@') {
                            if at > 0 {
                                raw_nm[..at].to_string()
                            } else {
                                raw_nm
                            }
                        } else {
                            raw_nm
                        };
                        if st_shndx != 0 && st_val >= min_vma && !clean_nm.is_empty() {
                            let vma = st_val + rebase_bias;
                            img.symbols.entry(clean_nm.clone()).or_insert(vma);
                            img.addr_to_sym.entry(vma).or_insert(clean_nm.clone());
                        }
                        syms.push(ElfSym {
                            name: clean_nm,
                            _info: st_info,
                            shndx: st_shndx,
                            value: st_val,
                        });
                    }
                    symtabs.insert(i, syms);
                }
            }
        } else if sh_type == 4 {
            // SHT_RELA
            rela_sections.push((sh_offset, sh_size, sh_link));
        } else if sh_name == ".plt" || sh_name == ".plt.sec" || sh_name == ".plt.got" {
            plt_sections.push((sh_addr + rebase_bias, sh_size as u64));
        }
    }
    if !sh_exec_ranges.is_empty() {
        img.exec_ranges = sh_exec_ranges;
    }
    img.embedded_bitcode = embedded_bitcode;

    // Apply dynamic relocations (`.rela.dyn`, `.rela.plt`)
    let mut got_slot_to_sym: HashMap<u64, String> = HashMap::new();
    let mut got_slot_to_internal_vma: HashMap<u64, u64> = HashMap::new();

    for (r_off, r_sz, r_link) in rela_sections {
        let Some(syms) = symtabs.get(&r_link).cloned() else {
            continue;
        };
        let count = r_sz / 24;
        for k in 0..count {
            let ro = r_off + k * 24;
            if ro + 24 > d.len() {
                break;
            }
            let r_offset = rd_u64_le(d, ro).unwrap_or(0);
            let r_info = rd_u64_le(d, ro + 8).unwrap_or(0);
            let r_addend = rd_i64_le(d, ro + 16).unwrap_or(0);
            let r_type = (r_info & 0xFFFF_FFFF) as u32;
            let r_sym = (r_info >> 32) as usize;

            if r_offset < min_vma {
                continue;
            }
            let target_vma = r_offset + rebase_bias;
            let img_idx = (target_vma - img.base_vma) as usize;
            if img_idx + 8 > img.image_bytes.len() {
                continue;
            }

            let is_relative = (arch == BinaryArch::X86_64 && r_type == 8)
                || (arch == BinaryArch::Aarch64 && r_type == 1027);
            if is_relative {
                let val = (rebase_bias as i64).wrapping_add(r_addend) as u64;
                wr_u64_le(&mut img.image_bytes, img_idx, val);
                continue;
            }

            let Some(sym) = syms.get(r_sym) else {
                continue;
            };
            let is_abs_or_got = matches!(
                (arch, r_type),
                (BinaryArch::X86_64, 1 | 6 | 7) | (BinaryArch::Aarch64, 257 | 1025 | 1026)
            );
            if !is_abs_or_got {
                continue;
            }

            if sym.shndx != 0 && sym.value >= min_vma {
                let resolved = ((sym.value + rebase_bias) as i64).wrapping_add(r_addend) as u64;
                wr_u64_le(&mut img.image_bytes, img_idx, resolved);
                got_slot_to_internal_vma.insert(target_vma, resolved);
            } else if !sym.name.is_empty() {
                if sym.name.contains("stack_chk_guard") {
                    wr_u64_le(&mut img.image_bytes, img_idx, canary_vma);
                } else {
                    let sentinel = img.intern_external_symbol(&sym.name);
                    wr_u64_le(&mut img.image_bytes, img_idx, sentinel);
                    let clean = normalize_ext_sym_name(&sym.name);
                    got_slot_to_sym.insert(target_vma, clean);
                }
            }
        }
    }

    // Decode `.plt` / `.plt.sec` thunks so direct `call plt_entry` resolves immediately!
    for (plt_vma, plt_sz) in plt_sections {
        decode_elf_plt_stubs(
            &mut img,
            plt_vma,
            plt_sz,
            &got_slot_to_sym,
            &got_slot_to_internal_vma,
        );
    }

    Ok(img)
}

fn decode_elf_plt_stubs(
    img: &mut ParsedBinaryImage,
    plt_vma: u64,
    plt_sz: u64,
    got_slot_to_sym: &HashMap<u64, String>,
    got_slot_to_internal_vma: &HashMap<u64, u64>,
) {
    match img.arch {
        BinaryArch::X86_64 => {
            // Each PLT / PLT.sec entry is 16 bytes; contains `jmp qword ptr [rip + disp32]` (`FF 25 <disp32>`)
            // optionally preceded by `endbr64` (`F3 0F 1E FA`) or BND prefix (`F2`).
            let mut cur = plt_vma;
            let end = plt_vma + plt_sz;
            while cur + 16 <= end {
                if let Some(chunk) = img.read_bytes(cur, 16) {
                    for off in 0..=10 {
                        if chunk[off] == 0xFF && chunk[off + 1] == 0x25 {
                            let disp = i32::from_le_bytes([
                                chunk[off + 2],
                                chunk[off + 3],
                                chunk[off + 4],
                                chunk[off + 5],
                            ]) as i64;
                            let next_ip = (cur + off as u64 + 6) as i64;
                            let got_vma = next_ip.wrapping_add(disp) as u64;
                            if let Some(ext_sym) = got_slot_to_sym.get(&got_vma) {
                                img.plt_stubs.insert(cur, ext_sym.clone());
                            } else if let Some(&int_vma) = got_slot_to_internal_vma.get(&got_vma) {
                                // Internal symbol called via PLT: map internal redirect via addr_to_sym or direct jump!
                                if let Some(nm) = img.addr_to_sym.get(&int_vma).cloned() {
                                    img.symbols.entry(nm).or_insert(int_vma);
                                }
                            }
                            break;
                        }
                    }
                }
                cur += 16;
            }
        }
        BinaryArch::Aarch64 => {
            // Standard ARM64 PLT stub: `adrp x16, page; ldr x17, [x16, #off]; add x16, x16, #off; br x17` (16 bytes)
            // (after 32-byte PLT0 header, or 16-byte entries throughout). Scan every 4-byte boundary for `adrp x16` + `ldr x17, [x16, #off]`.
            let mut cur = plt_vma;
            let end = plt_vma + plt_sz;
            while cur + 12 <= end {
                if let (Some(w0), Some(w1)) = (img.read_u32_le(cur), img.read_u32_le(cur + 4)) {
                    let w0_check = if w0 == 0xD503_245F {
                        // BTI c prefix (`hint #34`)
                        if let (Some(n0), Some(n1)) =
                            (img.read_u32_le(cur + 4), img.read_u32_le(cur + 8))
                        {
                            Some((cur + 4, n0, n1))
                        } else {
                            None
                        }
                    } else {
                        Some((cur, w0, w1))
                    };
                    if let Some((adrp_pc, iw0, iw1)) = w0_check {
                        // `adrp x16, ...`: `(iw0 & 0x9F00_001F) == 0x9000_0010`
                        // `ldr x17, [x16, #imm12]`: `(iw1 & 0xFFC0_03FF) == 0xF940_0211`
                        if (iw0 & 0x9F00_001F) == 0x9000_0010
                            && (iw1 & 0xFFC0_03FF) == 0xF940_0211
                        {
                            let immlo = ((iw0 >> 29) & 0x3) as i64;
                            let immhi = (((iw0 >> 5) & 0x7FFFF) as i32) << 13 >> 11;
                            let page_off = ((immhi as i64) | immlo) << 12;
                            let page = ((adrp_pc & !0xFFF) as i64).wrapping_add(page_off) as u64;
                            let ldr_off = (((iw1 >> 10) & 0xFFF) as u64) * 8;
                            let got_vma = page + ldr_off;
                            if let Some(ext_sym) = got_slot_to_sym.get(&got_vma) {
                                img.plt_stubs.insert(cur, ext_sym.clone());
                            }
                        }
                    }
                }
                cur += 4;
            }
        }
    }
}

/// Multi-member ELF64 relocatable (`.o` / `.a`) in-memory static linker.
fn link_elf64_relocatable_objects(objs: &[(String, &[u8])]) -> Result<ParsedBinaryImage, String> {
    if objs.is_empty() {
        return Err("No ELF64 objects provided".to_string());
    }
    let first = objs[0].1;
    let e_machine = rd_u16_le(first, 18).ok_or("Truncated ELF header")?;
    let (arch, abi) = match e_machine {
        62 => (BinaryArch::X86_64, BinaryAbi::SysV64),
        183 => (BinaryArch::Aarch64, BinaryAbi::Aapcs64),
        other => return Err(format!("Unsupported ELF relocatable machine {}", other)),
    };

    let base_vma = 0x1000_0000u64;
    let mut image_bytes: Vec<u8> = Vec::new();
    let mut exec_ranges: Vec<(u64, u64)> = Vec::new();
    // `(obj_idx, sec_idx) -> sec_vma`
    let mut sec_vmas: HashMap<(usize, usize), u64> = HashMap::new();
    let mut embedded_bitcode: Option<Vec<u8>> = None;

    // Pass 1: Lay out all SHF_ALLOC sections (plus PROGBITS `.rodata*`/`.text*`/`.data*`/`.bss*`)
    for (oidx, (_, d)) in objs.iter().enumerate() {
        let shoff = rd_u64_le(d, 40).unwrap_or(0) as usize;
        let shentsize = rd_u16_le(d, 58).unwrap_or(0) as usize;
        let shnum = rd_u16_le(d, 60).unwrap_or(0) as usize;
        let shstrndx = rd_u16_le(d, 62).unwrap_or(0) as usize;
        let mut shstrtab: &[u8] = &[];
        if shoff > 0 && shentsize >= 64 && shstrndx < shnum {
            let s_sh = shoff + shstrndx * shentsize;
            if s_sh + 64 <= d.len() {
                let off = rd_u64_le(d, s_sh + 24).unwrap_or(0) as usize;
                let sz = rd_u64_le(d, s_sh + 32).unwrap_or(0) as usize;
                if let Some(st) = d.get(off..off.saturating_add(sz)) {
                    shstrtab = st;
                }
            }
        }

        for sidx in 0..shnum {
            let sh = shoff + sidx * shentsize;
            if sh + 64 > d.len() {
                break;
            }
            let sh_name = rd_cstr(shstrtab, rd_u32_le(d, sh).unwrap_or(0) as usize);
            let sh_type = rd_u32_le(d, sh + 4).unwrap_or(0);
            let sh_flags = rd_u64_le(d, sh + 8).unwrap_or(0);
            let sh_offset = rd_u64_le(d, sh + 24).unwrap_or(0) as usize;
            let sh_size = rd_u64_le(d, sh + 32).unwrap_or(0) as usize;
            let sh_align = rd_u64_le(d, sh + 48).unwrap_or(16).max(16);

            if sh_name == ".llvmbc" {
                if let Some(bc) = d.get(sh_offset..sh_offset.saturating_add(sh_size)) {
                    if !bc.is_empty() {
                        embedded_bitcode = Some(bc.to_vec());
                    }
                }
            }
            // SHF_ALLOC = 0x2, SHT_PROGBITS = 1, SHT_NOBITS = 8, SHT_INIT_ARRAY = 14
            if (sh_flags & 0x2) == 0 || (sh_type != 1 && sh_type != 8 && sh_type != 14) {
                continue;
            }
            let aligned_off = align_up(image_bytes.len() as u64, sh_align) as usize;
            image_bytes.resize(aligned_off + sh_size.max(16), 0);
            if sh_type != 8 && sh_size > 0 {
                if let Some(src) = d.get(sh_offset..sh_offset.saturating_add(sh_size)) {
                    image_bytes[aligned_off..aligned_off + sh_size].copy_from_slice(src);
                }
            }
            let vma = base_vma + aligned_off as u64;
            sec_vmas.insert((oidx, sidx), vma);
            if (sh_flags & 0x4) != 0 && sh_size > 0 {
                exec_ranges.push((vma, vma + sh_size as u64));
            }
        }
    }

    // Reserve synthetic GOT & stack canary area at the end of `image_bytes`
    let canary_off = align_up(image_bytes.len() as u64, 16) as usize;
    image_bytes.resize(canary_off + 32, 0);
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = base_vma + canary_off as u64;

    let mut img = ParsedBinaryImage {
        format_label: format!("ELF64-relocatable ({})", arch.as_str()),
        arch,
        abi,
        base_vma,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode,
    };

    // Pass 2: Parse all symbol tables & resolve global symbol definitions
    let mut obj_syms: Vec<Vec<(String, u8, u16, u64)>> = Vec::with_capacity(objs.len());
    for (oidx, (_, d)) in objs.iter().enumerate() {
        let shoff = rd_u64_le(d, 40).unwrap_or(0) as usize;
        let shentsize = rd_u16_le(d, 58).unwrap_or(0) as usize;
        let shnum = rd_u16_le(d, 60).unwrap_or(0) as usize;
        let mut syms_vec = Vec::new();
        for sidx in 0..shnum {
            let sh = shoff + sidx * shentsize;
            if sh + 64 > d.len() {
                break;
            }
            if rd_u32_le(d, sh + 4).unwrap_or(0) != 2 {
                continue;
            }
            let sym_off = rd_u64_le(d, sh + 24).unwrap_or(0) as usize;
            let sym_sz = rd_u64_le(d, sh + 32).unwrap_or(0) as usize;
            let sh_link = rd_u32_le(d, sh + 40).unwrap_or(0) as usize;
            let str_sh = shoff + sh_link * shentsize;
            if str_sh + 64 > d.len() {
                break;
            }
            let str_off = rd_u64_le(d, str_sh + 24).unwrap_or(0) as usize;
            let str_sz = rd_u64_le(d, str_sh + 32).unwrap_or(0) as usize;
            let Some(strtab) = d.get(str_off..str_off.saturating_add(str_sz)) else {
                break;
            };
            let nsyms = sym_sz / 24;
            for k in 0..nsyms {
                let so = sym_off + k * 24;
                if so + 24 > d.len() {
                    break;
                }
                let st_name = rd_cstr(strtab, rd_u32_le(d, so).unwrap_or(0) as usize);
                let st_info = d[so + 4];
                let st_shndx = rd_u16_le(d, so + 6).unwrap_or(0);
                let st_val = rd_u64_le(d, so + 8).unwrap_or(0);
                let bind = st_info >> 4;
                if st_shndx == 0xFFF2 {
                    // SHN_COMMON: allocate zeroed storage in `img.image_bytes`
                    let sz = rd_u64_le(d, so + 16).unwrap_or(8) as usize;
                    let off = align_up(img.image_bytes.len() as u64, 16) as usize;
                    img.image_bytes.resize(off + sz.max(8), 0);
                    let vma = base_vma + off as u64;
                    if !st_name.is_empty() {
                        img.symbols.entry(st_name.clone()).or_insert(vma);
                    }
                    syms_vec.push((st_name, st_info, st_shndx, vma));
                } else if let Some(&sec_base) = sec_vmas.get(&(oidx, st_shndx as usize)) {
                    let vma = sec_base + st_val;
                    if !st_name.is_empty() && bind != 0 {
                        // Prefer strong (STB_GLOBAL = 1) over weak (STB_WEAK = 2)
                        if bind == 1 || !img.symbols.contains_key(&st_name) {
                            img.symbols.insert(st_name.clone(), vma);
                        }
                        img.addr_to_sym.entry(vma).or_insert(st_name.clone());
                    } else if !st_name.is_empty() && (st_info & 0xF) == 2 {
                        let scoped = format!("__local_{}_{}", oidx, st_name);
                        img.addr_to_sym.entry(vma).or_insert(scoped);
                    }
                    syms_vec.push((st_name, st_info, st_shndx, vma));
                } else {
                    syms_vec.push((st_name, st_info, st_shndx, st_val));
                }
            }
            break;
        }
        obj_syms.push(syms_vec);
    }

    // Helper to allocate an 8-byte GOT slot inside `img.image_bytes`
    let mut got_cache: HashMap<u64, u64> = HashMap::new();
    let alloc_got_slot = |img: &mut ParsedBinaryImage, got_cache: &mut HashMap<u64, u64>, target_val: u64| -> u64 {
        if let Some(&slot_vma) = got_cache.get(&target_val) {
            return slot_vma;
        }
        let off = align_up(img.image_bytes.len() as u64, 8) as usize;
        img.image_bytes.resize(off + 8, 0);
        wr_u64_le(&mut img.image_bytes, off, target_val);
        let slot_vma = img.base_vma + off as u64;
        got_cache.insert(target_val, slot_vma);
        slot_vma
    };

    // Pass 3: Apply all SHT_RELA relocations across all objects
    for (oidx, (_, d)) in objs.iter().enumerate() {
        let shoff = rd_u64_le(d, 40).unwrap_or(0) as usize;
        let shentsize = rd_u16_le(d, 58).unwrap_or(0) as usize;
        let shnum = rd_u16_le(d, 60).unwrap_or(0) as usize;

        for sidx in 0..shnum {
            let sh = shoff + sidx * shentsize;
            if sh + 64 > d.len() {
                break;
            }
            if rd_u32_le(d, sh + 4).unwrap_or(0) != 4 {
                continue; // SHT_RELA
            }
            let r_offset_file = rd_u64_le(d, sh + 24).unwrap_or(0) as usize;
            let r_size = rd_u64_le(d, sh + 32).unwrap_or(0) as usize;
            let target_sec = rd_u32_le(d, sh + 44).unwrap_or(0) as usize;
            let Some(&target_sec_vma) = sec_vmas.get(&(oidx, target_sec)) else {
                continue;
            };

            let count = r_size / 24;
            for k in 0..count {
                let ro = r_offset_file + k * 24;
                if ro + 24 > d.len() {
                    break;
                }
                let r_off = rd_u64_le(d, ro).unwrap_or(0);
                let r_info = rd_u64_le(d, ro + 8).unwrap_or(0);
                let r_addend = rd_i64_le(d, ro + 16).unwrap_or(0);
                let r_type = (r_info & 0xFFFF_FFFF) as u32;
                let r_sym = (r_info >> 32) as usize;

                let place_vma = target_sec_vma + r_off;
                let place_idx = (place_vma - img.base_vma) as usize;
                let Some((sym_name, sym_info, sym_shndx, local_vma)) =
                    obj_syms[oidx].get(r_sym).cloned()
                else {
                    continue;
                };

                // Resolve symbol target `S` (either internal VMA or external sentinel / PLT stub!)
                let mut is_ext = false;
                let s_val = if sym_shndx != 0 {
                    if (sym_info >> 4) != 0 && !sym_name.is_empty() {
                        *img.symbols.get(&sym_name).unwrap_or(&local_vma)
                    } else {
                        local_vma
                    }
                } else if let Some(&glob_vma) = img.symbols.get(&sym_name) {
                    glob_vma
                } else if sym_name.contains("stack_chk_guard") {
                    canary_vma
                } else {
                    is_ext = true;
                    img.intern_external_symbol(&sym_name)
                };

                match arch {
                    BinaryArch::X86_64 => {
                        match r_type {
                            1 => {
                                // R_X86_64_64: S + A
                                let v = (s_val as i64).wrapping_add(r_addend) as u64;
                                wr_u64_le(&mut img.image_bytes, place_idx, v);
                            }
                            2 | 4 => {
                                // R_X86_64_PC32 (2) / R_X86_64_PLT32 (4): S + A - P
                                let target_s = if is_ext {
                                    alloc_synthetic_x86_plt_thunk(&mut img, &sym_name, s_val, &mut got_cache)
                                } else {
                                    s_val
                                };
                                let rel = (target_s as i64)
                                    .wrapping_add(r_addend)
                                    .wrapping_sub(place_vma as i64);
                                wr_u32_le(&mut img.image_bytes, place_idx, rel as i32 as u32);
                            }
                            9 | 41 | 42 => {
                                // R_X86_64_GOTPCREL / GOTPCRELX / REX_GOTPCRELX: G + A - P
                                let got_vma = alloc_got_slot(&mut img, &mut got_cache, s_val);
                                let rel = (got_vma as i64)
                                    .wrapping_add(r_addend)
                                    .wrapping_sub(place_vma as i64);
                                wr_u32_le(&mut img.image_bytes, place_idx, rel as i32 as u32);
                            }
                            10 | 11 => {
                                // R_X86_64_32 / R_X86_64_32S: S + A
                                let v = (s_val as i64).wrapping_add(r_addend) as u32;
                                wr_u32_le(&mut img.image_bytes, place_idx, v);
                            }
                            _ => {}
                        }
                    }
                    BinaryArch::Aarch64 => {
                        apply_aarch64_elf_reloc(
                            &mut img,
                            &mut got_cache,
                            place_vma,
                            place_idx,
                            r_type,
                            s_val,
                            r_addend,
                            is_ext,
                            &sym_name,
                        );
                    }
                }
            }
        }
    }

    Ok(img)
}

fn alloc_synthetic_x86_plt_thunk(
    img: &mut ParsedBinaryImage,
    sym_name: &str,
    sentinel: u64,
    got_cache: &mut HashMap<u64, u64>,
) -> u64 {
    let clean = normalize_ext_sym_name(sym_name);
    if let Some((&vma, _)) = img.plt_stubs.iter().find(|(_, s)| *s == &clean) {
        return vma;
    }
    let got_vma = if let Some(&g) = got_cache.get(&sentinel) {
        g
    } else {
        let goff = align_up(img.image_bytes.len() as u64, 8) as usize;
        img.image_bytes.resize(goff + 8, 0);
        wr_u64_le(&mut img.image_bytes, goff, sentinel);
        let gv = img.base_vma + goff as u64;
        got_cache.insert(sentinel, gv);
        gv
    };
    let toff = align_up(img.image_bytes.len() as u64, 16) as usize;
    img.image_bytes.resize(toff + 16, 0x90);
    let thunk_vma = img.base_vma + toff as u64;
    let disp = (got_vma as i64).wrapping_sub((thunk_vma + 6) as i64) as i32;
    img.image_bytes[toff] = 0xFF;
    img.image_bytes[toff + 1] = 0x25;
    img.image_bytes[toff + 2..toff + 6].copy_from_slice(&disp.to_le_bytes());
    img.plt_stubs.insert(thunk_vma, clean);
    thunk_vma
}

fn alloc_synthetic_arm64_plt_thunk(img: &mut ParsedBinaryImage, sym_name: &str) -> u64 {
    let clean = normalize_ext_sym_name(sym_name);
    if let Some((&vma, _)) = img.plt_stubs.iter().find(|(_, s)| *s == &clean) {
        return vma;
    }
    let toff = align_up(img.image_bytes.len() as u64, 16) as usize;
    img.image_bytes.resize(toff + 8, 0);
    // Emit `ret` (`D65F03C0`) as placeholder; `img.plt_stubs` intercepts before decoding!
    wr_u32_le(&mut img.image_bytes, toff, 0xD65F_03C0);
    let thunk_vma = img.base_vma + toff as u64;
    img.plt_stubs.insert(thunk_vma, clean);
    thunk_vma
}

fn apply_aarch64_elf_reloc(
    img: &mut ParsedBinaryImage,
    got_cache: &mut HashMap<u64, u64>,
    place_vma: u64,
    place_idx: usize,
    r_type: u32,
    s_val: u64,
    r_addend: i64,
    is_ext: bool,
    sym_name: &str,
) {
    let sa = (s_val as i64).wrapping_add(r_addend) as u64;
    match r_type {
        257 => {
            // R_AARCH64_ABS64
            wr_u64_le(&mut img.image_bytes, place_idx, sa);
        }
        261 => {
            // R_AARCH64_PREL32
            let rel = (sa as i64).wrapping_sub(place_vma as i64) as i32;
            wr_u32_le(&mut img.image_bytes, place_idx, rel as u32);
        }
        282 | 283 => {
            // R_AARCH64_JUMP26 (282) / R_AARCH64_CALL26 (283)
            let target = if is_ext {
                alloc_synthetic_arm64_plt_thunk(img, sym_name)
            } else {
                sa
            };
            let imm26 = (((target as i64).wrapping_sub(place_vma as i64) >> 2) as u32) & 0x03FF_FFFF;
            let orig = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0);
            wr_u32_le(&mut img.image_bytes, place_idx, (orig & 0xFC00_0000) | imm26);
        }
        275 | 311 => {
            // R_AARCH64_ADR_PREL_PG_HI21 (275) / R_AARCH64_ADR_GOT_PAGE (311)
            let target = if r_type == 311 {
                if let Some(&g) = got_cache.get(&s_val) {
                    g
                } else {
                    let goff = align_up(img.image_bytes.len() as u64, 8) as usize;
                    img.image_bytes.resize(goff + 8, 0);
                    wr_u64_le(&mut img.image_bytes, goff, s_val);
                    let gv = img.base_vma + goff as u64;
                    got_cache.insert(s_val, gv);
                    gv
                }
            } else {
                sa
            };
            patch_arm64_adrp(&mut img.image_bytes, place_idx, place_vma, target);
        }
        277 => {
            // R_AARCH64_ADD_ABS_LO12_NC
            let imm12 = ((sa & 0xFFF) as u32) << 10;
            let orig = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0);
            wr_u32_le(&mut img.image_bytes, place_idx, (orig & !(0xFFF << 10)) | imm12);
        }
        278 | 284 | 285 | 286 | 299 | 312 => {
            // R_AARCH64_LDST{8,16,32,64,128}_ABS_LO12_NC & R_AARCH64_LD64_GOT_LO12_NC (312)
            let target = if r_type == 312 {
                if let Some(&g) = got_cache.get(&s_val) {
                    g
                } else {
                    let goff = align_up(img.image_bytes.len() as u64, 8) as usize;
                    img.image_bytes.resize(goff + 8, 0);
                    wr_u64_le(&mut img.image_bytes, goff, s_val);
                    let gv = img.base_vma + goff as u64;
                    got_cache.insert(s_val, gv);
                    gv
                }
            } else {
                sa
            };
            let shift = match r_type {
                278 => 0,
                284 => 1,
                285 => 2,
                286 | 312 => 3,
                299 => 4,
                _ => 0,
            };
            let imm12 = (((target & 0xFFF) >> shift) as u32) << 10;
            let orig = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0);
            wr_u32_le(&mut img.image_bytes, place_idx, (orig & !(0xFFF << 10)) | imm12);
        }
        _ => {}
    }
}

fn patch_arm64_adrp(buf: &mut [u8], idx: usize, place_vma: u64, target_vma: u64) {
    let page_delta = ((target_vma & !0xFFF) as i64).wrapping_sub((place_vma & !0xFFF) as i64) >> 12;
    let immlo = ((page_delta as u32) & 0x3) << 29;
    let immhi = (((page_delta as u32) >> 2) & 0x7FFFF) << 5;
    let orig = rd_u32_le(buf, idx).unwrap_or(0);
    let mask = !((0x3 << 29) | (0x7FFFF << 5));
    wr_u32_le(buf, idx, (orig & mask) | immlo | immhi);
}

// ============================================================================
// 3. Mach-O 64 & Universal Fat Parser (`.dylib` & `.o`)
// ============================================================================

fn unwrap_macho_fat_slice(bytes: &[u8], preferred_arch: TargetArch) -> Result<&[u8], String> {
    let magic_be = rd_u32_be(bytes, 0).unwrap_or(0);
    if magic_be != 0xCAFE_BABE && magic_be != 0xCAFE_BABF {
        return Ok(bytes);
    }
    let is_64 = magic_be == 0xCAFE_BABF;
    let nfat = rd_u32_be(bytes, 4).unwrap_or(0) as usize;
    let want_cpu: u32 = match preferred_arch {
        TargetArch::X86_64 => 0x0100_0007,  // CPU_TYPE_X86_64
        TargetArch::Aarch64 => 0x0100_000C, // CPU_TYPE_ARM64
    };

    let mut fallback: Option<&[u8]> = None;
    for i in 0..nfat.min(16) {
        let entry_off = 8 + i * if is_64 { 32 } else { 20 };
        let Some(cputype) = rd_u32_be(bytes, entry_off) else {
            break;
        };
        let (off, sz) = if is_64 {
            let o = u64::from_be_bytes(
                bytes
                    .get(entry_off + 8..entry_off + 16)
                    .ok_or("Truncated fat_arch_64")?
                    .try_into()
                    .unwrap(),
            ) as usize;
            let s = u64::from_be_bytes(
                bytes
                    .get(entry_off + 16..entry_off + 24)
                    .ok_or("Truncated fat_arch_64")?
                    .try_into()
                    .unwrap(),
            ) as usize;
            (o, s)
        } else {
            (
                rd_u32_be(bytes, entry_off + 8).unwrap_or(0) as usize,
                rd_u32_be(bytes, entry_off + 12).unwrap_or(0) as usize,
            )
        };
        if let Some(slice) = bytes.get(off..off.saturating_add(sz)) {
            if cputype == want_cpu {
                return Ok(slice);
            }
            if (cputype == 0x0100_0007 || cputype == 0x0100_000C) && fallback.is_none() {
                fallback = Some(slice);
            }
        }
    }
    fallback.ok_or_else(|| "Mach-O Universal binary contains no 64-bit x86_64/arm64 slice".to_string())
}

fn parse_macho(raw_bytes: &[u8], preferred_arch: TargetArch) -> Result<ParsedBinaryImage, String> {
    let d = unwrap_macho_fat_slice(raw_bytes, preferred_arch)?;
    if d.len() < 32 || rd_u32_le(d, 0) != Some(0xFEED_FACF) {
        return Err("Invalid Mach-O 64 magic".to_string());
    }
    let filetype = rd_u32_le(d, 12).unwrap_or(0);
    if filetype == 1 {
        // MH_OBJECT (`.o`)
        return link_macho64_objects(&[("object.o".to_string(), d)]);
    }
    let cputype = rd_u32_le(d, 4).unwrap_or(0);
    let (arch, abi) = match cputype {
        0x0100_0007 => (BinaryArch::X86_64, BinaryAbi::SysV64),
        0x0100_000C => (BinaryArch::Aarch64, BinaryAbi::AppleArm64),
        other => return Err(format!("Unsupported Mach-O cputype 0x{:x}", other)),
    };
    let ncmds = rd_u32_le(d, 16).unwrap_or(0) as usize;

    #[derive(Clone)]
    struct MachSeg {
        _name: String,
        vmaddr: u64,
        vmsize: u64,
        fileoff: usize,
        filesize: usize,
        initprot: u32,
    }
    #[derive(Clone)]
    struct MachSect {
        _seg: String,
        _sect: String,
        addr: u64,
        size: u64,
        _offset: usize,
        flags: u32,
        reserved1: u32,
        reserved2: u32,
    }

    let mut segs = Vec::new();
    let mut sects = Vec::new();
    let mut symoff = 0usize;
    let mut nsyms = 0usize;
    let mut stroff = 0usize;
    let mut strsize = 0usize;
    let mut indirectsymoff = 0usize;
    let mut nindirectsyms = 0usize;
    let mut dyld_bind: Vec<(usize, usize)> = Vec::new();
    let mut dyld_rebase: Option<(usize, usize)> = None;
    let mut chained_fixups: Option<(usize, usize)> = None;
    let mut embedded_bitcode = None;

    let mut cmd_off = 32usize;
    for _ in 0..ncmds {
        if cmd_off + 8 > d.len() {
            break;
        }
        let cmd = rd_u32_le(d, cmd_off).unwrap_or(0);
        let cmdsize = rd_u32_le(d, cmd_off + 4).unwrap_or(0) as usize;
        if cmdsize < 8 || cmd_off + cmdsize > d.len() {
            break;
        }

        match cmd {
            0x19 => {
                // LC_SEGMENT_64
                if cmdsize >= 72 {
                    let segname = std::str::from_utf8(&d[cmd_off + 8..cmd_off + 24])
                        .unwrap_or("")
                        .trim_end_matches('\0')
                        .to_string();
                    let vmaddr = rd_u64_le(d, cmd_off + 24).unwrap_or(0);
                    let vmsize = rd_u64_le(d, cmd_off + 32).unwrap_or(0);
                    let fileoff = rd_u64_le(d, cmd_off + 40).unwrap_or(0) as usize;
                    let filesize = rd_u64_le(d, cmd_off + 48).unwrap_or(0) as usize;
                    let initprot = rd_u32_le(d, cmd_off + 60).unwrap_or(0);
                    let nsects = rd_u32_le(d, cmd_off + 64).unwrap_or(0) as usize;

                    if segname != "__PAGEZERO" && segname != "__LINKEDIT" && vmsize > 0 {
                        segs.push(MachSeg {
                            _name: segname.clone(),
                            vmaddr,
                            vmsize,
                            fileoff,
                            filesize,
                            initprot,
                        });
                    }

                    for s in 0..nsects {
                        let so = cmd_off + 72 + s * 80;
                        if so + 80 > cmd_off + cmdsize {
                            break;
                        }
                        let sectname = std::str::from_utf8(&d[so..so + 16])
                            .unwrap_or("")
                            .trim_end_matches('\0')
                            .to_string();
                        let s_seg = std::str::from_utf8(&d[so + 16..so + 32])
                            .unwrap_or("")
                            .trim_end_matches('\0')
                            .to_string();
                        let addr = rd_u64_le(d, so + 32).unwrap_or(0);
                        let size = rd_u64_le(d, so + 40).unwrap_or(0);
                        let offset = rd_u32_le(d, so + 48).unwrap_or(0) as usize;
                        let flags = rd_u32_le(d, so + 64).unwrap_or(0);
                        let reserved1 = rd_u32_le(d, so + 68).unwrap_or(0);
                        let reserved2 = rd_u32_le(d, so + 72).unwrap_or(0);
                        if s_seg == "__LLVM" && sectname == "__bitcode" && size > 0 {
                            if let Some(bc) = d.get(offset..offset.saturating_add(size as usize)) {
                                embedded_bitcode = Some(bc.to_vec());
                            }
                        }
                        sects.push(MachSect {
                            _seg: s_seg,
                            _sect: sectname,
                            addr,
                            size,
                            _offset: offset,
                            flags,
                            reserved1,
                            reserved2,
                        });
                    }
                }
            }
            0x02 => {
                // LC_SYMTAB
                if cmdsize >= 24 {
                    symoff = rd_u32_le(d, cmd_off + 8).unwrap_or(0) as usize;
                    nsyms = rd_u32_le(d, cmd_off + 12).unwrap_or(0) as usize;
                    stroff = rd_u32_le(d, cmd_off + 16).unwrap_or(0) as usize;
                    strsize = rd_u32_le(d, cmd_off + 20).unwrap_or(0) as usize;
                }
            }
            0x0B => {
                // LC_DYSYMTAB
                if cmdsize >= 80 {
                    indirectsymoff = rd_u32_le(d, cmd_off + 56).unwrap_or(0) as usize;
                    nindirectsyms = rd_u32_le(d, cmd_off + 60).unwrap_or(0) as usize;
                }
            }
            0x22 | 0x8000_0022 => {
                // LC_DYLD_INFO / LC_DYLD_INFO_ONLY
                if cmdsize >= 48 {
                    let rb_off = rd_u32_le(d, cmd_off + 8).unwrap_or(0) as usize;
                    let rb_sz = rd_u32_le(d, cmd_off + 12).unwrap_or(0) as usize;
                    if rb_off > 0 && rb_sz > 0 {
                        dyld_rebase = Some((rb_off, rb_sz));
                    }
                    for pair_off in [16usize, 24, 32] {
                        let bo = rd_u32_le(d, cmd_off + pair_off).unwrap_or(0) as usize;
                        let bs = rd_u32_le(d, cmd_off + pair_off + 4).unwrap_or(0) as usize;
                        if bo > 0 && bs > 0 {
                            dyld_bind.push((bo, bs));
                        }
                    }
                }
            }
            0x8000_0034 => {
                // LC_DYLD_CHAINED_FIXUPS
                if cmdsize >= 16 {
                    let doff = rd_u32_le(d, cmd_off + 8).unwrap_or(0) as usize;
                    let dsz = rd_u32_le(d, cmd_off + 12).unwrap_or(0) as usize;
                    if doff > 0 && dsz > 0 {
                        chained_fixups = Some((doff, dsz));
                    }
                }
            }
            _ => {}
        }
        cmd_off += cmdsize;
    }

    if segs.is_empty() {
        return Err("Mach-O 64 binary has no loadable segments".to_string());
    }
    let min_vma = segs.iter().map(|s| s.vmaddr).min().unwrap_or(0);
    let max_vma = segs
        .iter()
        .map(|s| s.vmaddr.saturating_add(s.vmsize))
        .max()
        .unwrap_or(0);
    let rebase_bias = if min_vma < 0x10000 { 0x1000_0000u64 } else { 0u64 };
    let base_vma = min_vma + rebase_bias;
    let total_span = (max_vma - min_vma) as usize;
    if total_span > 256 * 1024 * 1024 {
        return Err("Mach-O virtual image exceeds 256 MB safety bound".to_string());
    }
    let canary_off = align_up(total_span as u64, 16) as usize;
    let mut image_bytes = vec![0u8; canary_off + 64];
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = base_vma + canary_off as u64;

    let mut exec_ranges = Vec::new();
    for seg in &segs {
        let img_off = (seg.vmaddr - min_vma) as usize;
        if let Some(src) = d.get(seg.fileoff..seg.fileoff.saturating_add(seg.filesize)) {
            if let Some(dst) = image_bytes.get_mut(img_off..img_off.saturating_add(src.len())) {
                dst.copy_from_slice(src);
            }
        }
    }
    // Prefer exact `S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS` sections so `__TEXT,__const`
    // and `__TEXT,__cstring` data tables inside `__TEXT` are never mistaken for executable functions!
    for sect in &sects {
        if (sect.flags & 0x8000_0400) != 0 && sect.size > 0 {
            exec_ranges.push((
                sect.addr + rebase_bias,
                sect.addr + rebase_bias + sect.size,
            ));
        }
    }
    if exec_ranges.is_empty() {
        for seg in &segs {
            if (seg.initprot & 4) != 0 && seg.vmsize > 0 {
                exec_ranges.push((
                    seg.vmaddr + rebase_bias,
                    seg.vmaddr + rebase_bias + seg.vmsize,
                ));
            }
        }
    }

    let mut img = ParsedBinaryImage {
        format_label: format!("Mach-O64 ({})", arch.as_str()),
        arch,
        abi,
        base_vma,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode,
    };

    // Parse LC_SYMTAB (`nlist_64` = 16 bytes)
    let mut sym_names = Vec::with_capacity(nsyms);
    if let Some(strtab) = d.get(stroff..stroff.saturating_add(strsize)) {
        for i in 0..nsyms {
            let so = symoff + i * 16;
            if so + 16 > d.len() {
                break;
            }
            let n_strx = rd_u32_le(d, so).unwrap_or(0) as usize;
            let n_type = d[so + 4];
            let n_value = rd_u64_le(d, so + 8).unwrap_or(0);
            let nm = rd_cstr(strtab, n_strx);
            // N_SECT = 0x0E (mask N_TYPE = 0x0E)
            if (n_type & 0x0E) == 0x0E && (n_type & 0xE0) == 0 && n_value >= min_vma && !nm.is_empty() {
                let vma = n_value + rebase_bias;
                img.symbols.entry(nm.clone()).or_insert(vma);
                if let Some(unpref) = nm.strip_prefix('_') {
                    if !unpref.is_empty() {
                        img.symbols.entry(unpref.to_string()).or_insert(vma);
                    }
                }
                img.addr_to_sym.entry(vma).or_insert(nm.clone());
            }
            sym_names.push(nm);
        }
    }

    // Parse LC_DYSYMTAB indirect symbol table for `__stubs` and `__got` / `__la_symbol_ptr` / `__nl_symbol_ptr`
    if indirectsymoff > 0 && nindirectsyms > 0 {
        for sect in &sects {
            let sect_type = sect.flags & 0xFF;
            // S_NON_LAZY_SYMBOL_POINTERS (0x6), S_LAZY_SYMBOL_POINTERS (0x7), S_SYMBOL_STUBS (0x8)
            if !matches!(sect_type, 0x6 | 0x7 | 0x8) {
                continue;
            }
            let stride = if sect_type == 0x8 {
                (sect.reserved2 as u64).max(6)
            } else {
                8
            };
            let count = sect.size / stride;
            for k in 0..count {
                let idx_pos = indirectsymoff + ((sect.reserved1 as usize) + k as usize) * 4;
                let Some(sym_idx) = rd_u32_le(d, idx_pos) else {
                    break;
                };
                if (sym_idx & 0xC000_0000) != 0 {
                    continue; // INDIRECT_SYMBOL_LOCAL / ABS
                }
                let Some(raw_nm) = sym_names.get(sym_idx as usize) else {
                    continue;
                };
                if raw_nm.is_empty() {
                    continue;
                }
                let entry_vma = sect.addr + rebase_bias + k * stride;
                if sect_type == 0x8 {
                    // `__TEXT,__stubs`
                    if let Some(&int_vma) = img.symbols.get(raw_nm) {
                        let _ = int_vma;
                    } else {
                        img.plt_stubs
                            .insert(entry_vma, normalize_ext_sym_name(raw_nm));
                    }
                } else {
                    // GOT / symbol pointer slot
                    let img_idx = (entry_vma - img.base_vma) as usize;
                    if let Some(&int_vma) = img.symbols.get(raw_nm) {
                        wr_u64_le(&mut img.image_bytes, img_idx, int_vma);
                    } else if raw_nm.contains("stack_chk_guard") {
                        wr_u64_le(&mut img.image_bytes, img_idx, canary_vma);
                    } else {
                        let sentinel = img.intern_external_symbol(raw_nm);
                        wr_u64_le(&mut img.image_bytes, img_idx, sentinel);
                    }
                }
            }
        }
    }

    // Apply LC_DYLD_INFO rebases & binds if present
    let seg_vmas: Vec<u64> = segs.iter().map(|s| s.vmaddr + rebase_bias).collect();
    // Wait: segment indices in dyld opcodes include __PAGEZERO / __LINKEDIT! Build full raw segment table!
    let mut raw_seg_vmas = Vec::new();
    {
        let mut co = 32usize;
        for _ in 0..ncmds {
            if co + 8 > d.len() {
                break;
            }
            let c = rd_u32_le(d, co).unwrap_or(0);
            let csz = rd_u32_le(d, co + 4).unwrap_or(0) as usize;
            if c == 0x19 && csz >= 72 {
                let va = rd_u64_le(d, co + 24).unwrap_or(0);
                raw_seg_vmas.push(va + rebase_bias);
            }
            co += csz;
        }
        let _ = seg_vmas;
    }

    if let Some((rb_off, rb_sz)) = dyld_rebase {
        if rebase_bias != 0 {
            if let Some(ops) = d.get(rb_off..rb_off.saturating_add(rb_sz)) {
                apply_macho_dyld_rebase(&mut img, ops, &raw_seg_vmas, rebase_bias);
            }
        }
    }
    for (b_off, b_sz) in dyld_bind {
        if let Some(ops) = d.get(b_off..b_off.saturating_add(b_sz)) {
            apply_macho_dyld_bind(&mut img, ops, &raw_seg_vmas, canary_vma);
        }
    }
    if let Some((cf_off, cf_sz)) = chained_fixups {
        if let Some(cf_bytes) = d.get(cf_off..cf_off.saturating_add(cf_sz)) {
            apply_macho_chained_fixups(&mut img, d, cf_bytes, &raw_seg_vmas, min_vma, rebase_bias, canary_vma);
        }
    }

    // Also scan `__TEXT,__stubs` on arm64/x86_64 in case LC_DYLD_CHAINED_FIXUPS bound GOT slots without LC_DYSYMTAB entries!
    for sect in &sects {
        if (sect.flags & 0xFF) == 0x8 && sect.size > 0 {
            let got_map: HashMap<u64, String> = collect_sentinel_slots_map(&img);
            let empty_int = HashMap::new();
            decode_elf_plt_stubs(
                &mut img,
                sect.addr + rebase_bias,
                sect.size,
                &got_map,
                &empty_int,
            );
        }
    }

    Ok(img)
}

fn collect_sentinel_slots_map(img: &ParsedBinaryImage) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    let mut off = 0usize;
    while off + 8 <= img.image_bytes.len() {
        let val = rd_u64_le(&img.image_bytes, off).unwrap_or(0);
        if is_ext_sentinel(val) {
            if let Some(sym) = img.ext_sentinels.get(&val) {
                out.insert(img.base_vma + off as u64, sym.clone());
            }
        }
        off += 8;
    }
    out
}

fn read_uleb128(buf: &[u8], pos: &mut usize) -> u64 {
    let mut result = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let byte = buf[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift = shift.saturating_add(7);
    }
    result
}

fn read_sleb128(buf: &[u8], pos: &mut usize) -> i64 {
    let mut result = 0i64;
    let mut shift = 0u32;
    let mut byte = 0u8;
    while *pos < buf.len() {
        byte = buf[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as i64) << shift;
        shift = shift.saturating_add(7);
        if (byte & 0x80) == 0 {
            break;
        }
    }
    if shift < 64 && (byte & 0x40) != 0 {
        result |= !0i64 << shift;
    }
    result
}

fn apply_macho_dyld_rebase(
    img: &mut ParsedBinaryImage,
    ops: &[u8],
    seg_vmas: &[u64],
    rebase_bias: u64,
) {
    let mut pos = 0usize;
    let mut seg_idx = 0usize;
    let mut seg_off = 0u64;
    let step = |img: &mut ParsedBinaryImage, seg_idx: usize, seg_off: u64| {
        if let Some(&svma) = seg_vmas.get(seg_idx) {
            let vma = svma.wrapping_add(seg_off);
            if vma >= img.base_vma {
                let idx = (vma - img.base_vma) as usize;
                if let Some(cur) = rd_u64_le(&img.image_bytes, idx) {
                    wr_u64_le(&mut img.image_bytes, idx, cur.wrapping_add(rebase_bias));
                }
            }
        }
    };
    while pos < ops.len() {
        let b = ops[pos];
        pos += 1;
        let opcode = b & 0xF0;
        let imm = (b & 0x0F) as usize;
        match opcode {
            0x00 => break,
            0x10 => {}
            0x20 => {
                seg_idx = imm;
                seg_off = read_uleb128(ops, &mut pos);
            }
            0x30 => {
                seg_off = seg_off.wrapping_add(read_uleb128(ops, &mut pos));
            }
            0x40 => {
                seg_off = seg_off.wrapping_add((imm as u64) * 8);
            }
            0x50 => {
                for _ in 0..imm {
                    step(img, seg_idx, seg_off);
                    seg_off += 8;
                }
            }
            0x60 => {
                let count = read_uleb128(ops, &mut pos);
                for _ in 0..count {
                    step(img, seg_idx, seg_off);
                    seg_off += 8;
                }
            }
            0x70 => {
                step(img, seg_idx, seg_off);
                seg_off = seg_off.wrapping_add(read_uleb128(ops, &mut pos) + 8);
            }
            0x80 => {
                let count = read_uleb128(ops, &mut pos);
                let skip = read_uleb128(ops, &mut pos);
                for _ in 0..count {
                    step(img, seg_idx, seg_off);
                    seg_off = seg_off.wrapping_add(skip + 8);
                }
            }
            _ => break,
        }
    }
}

fn apply_macho_dyld_bind(
    img: &mut ParsedBinaryImage,
    ops: &[u8],
    seg_vmas: &[u64],
    canary_vma: u64,
) {
    let mut pos = 0usize;
    let mut seg_idx = 0usize;
    let mut seg_off = 0u64;
    let mut cur_sym = String::new();
    let mut addend = 0i64;

    let do_bind = |img: &mut ParsedBinaryImage,
                   seg_idx: usize,
                   seg_off: u64,
                   sym: &str,
                   addend: i64| {
        let Some(&svma) = seg_vmas.get(seg_idx) else {
            return;
        };
        let vma = svma.wrapping_add(seg_off);
        if vma < img.base_vma || sym.is_empty() {
            return;
        }
        let idx = (vma - img.base_vma) as usize;
        if let Some(&int_vma) = img.symbols.get(sym) {
            wr_u64_le(
                &mut img.image_bytes,
                idx,
                (int_vma as i64).wrapping_add(addend) as u64,
            );
        } else if sym.contains("stack_chk_guard") {
            wr_u64_le(&mut img.image_bytes, idx, canary_vma);
        } else {
            let sentinel = img.intern_external_symbol(sym);
            wr_u64_le(&mut img.image_bytes, idx, sentinel);
        }
    };

    while pos < ops.len() {
        let b = ops[pos];
        pos += 1;
        let opcode = b & 0xF0;
        let imm = (b & 0x0F) as usize;
        match opcode {
            0x00 => {}
            0x10 => {}
            0x20 => {
                let _ = read_uleb128(ops, &mut pos);
            }
            0x30 => {}
            0x40 => {
                cur_sym = rd_cstr(ops, pos);
                while pos < ops.len() && ops[pos] != 0 {
                    pos += 1;
                }
                if pos < ops.len() {
                    pos += 1;
                }
            }
            0x50 => {}
            0x60 => {
                addend = read_sleb128(ops, &mut pos);
            }
            0x70 => {
                seg_idx = imm;
                seg_off = read_uleb128(ops, &mut pos);
            }
            0x80 => {
                seg_off = (seg_off as i64).wrapping_add(read_sleb128(ops, &mut pos)) as u64;
            }
            0x90 => {
                do_bind(img, seg_idx, seg_off, &cur_sym, addend);
                seg_off += 8;
            }
            0xA0 => {
                do_bind(img, seg_idx, seg_off, &cur_sym, addend);
                seg_off = seg_off.wrapping_add(read_uleb128(ops, &mut pos) + 8);
            }
            0xB0 => {
                do_bind(img, seg_idx, seg_off, &cur_sym, addend);
                seg_off = seg_off.wrapping_add((imm as u64) * 8 + 8);
            }
            0xC0 => {
                let count = read_uleb128(ops, &mut pos);
                let skip = read_uleb128(ops, &mut pos);
                for _ in 0..count {
                    do_bind(img, seg_idx, seg_off, &cur_sym, addend);
                    seg_off = seg_off.wrapping_add(skip + 8);
                }
            }
            _ => break,
        }
    }
}

fn apply_macho_chained_fixups(
    img: &mut ParsedBinaryImage,
    file_bytes: &[u8],
    cf: &[u8],
    _raw_seg_vmas: &[u64],
    min_vma: u64,
    rebase_bias: u64,
    canary_vma: u64,
) {
    if cf.len() < 28 {
        return;
    }
    let starts_offset = rd_u32_le(cf, 4).unwrap_or(0) as usize;
    let imports_offset = rd_u32_le(cf, 8).unwrap_or(0) as usize;
    let symbols_offset = rd_u32_le(cf, 12).unwrap_or(0) as usize;
    let imports_count = rd_u32_le(cf, 16).unwrap_or(0) as usize;
    let imports_format = rd_u32_le(cf, 20).unwrap_or(0);

    // 1. Parse chained imports table
    let mut import_syms = Vec::with_capacity(imports_count.min(65536));
    if let Some(sym_pool) = cf.get(symbols_offset..) {
        for i in 0..imports_count.min(65536) {
            let name_off = match imports_format {
                1 => {
                    // DYLD_CHAINED_IMPORT (4 bytes: bits [31:9] = name_offset)
                    rd_u32_le(cf, imports_offset + i * 4).map(|v| (v >> 9) as usize)
                }
                2 => {
                    // DYLD_CHAINED_IMPORT_ADDEND (8 bytes)
                    rd_u32_le(cf, imports_offset + i * 8).map(|v| (v >> 9) as usize)
                }
                3 => {
                    // DYLD_CHAINED_IMPORT_ADDEND64 (16 bytes: bits [63:32] = name_offset)
                    rd_u64_le(cf, imports_offset + i * 16).map(|v| (v >> 32) as usize)
                }
                _ => None,
            };
            if let Some(noff) = name_off {
                import_syms.push(rd_cstr(sym_pool, noff));
            } else {
                import_syms.push(String::new());
            }
        }
    }

    // 2. Walk `dyld_chained_starts_in_image` -> `dyld_chained_starts_in_segment`
    let Some(seg_count) = rd_u32_le(cf, starts_offset) else {
        return;
    };
    for s in 0..(seg_count as usize).min(32) {
        let Some(seg_info_rel) = rd_u32_le(cf, starts_offset + 4 + s * 4) else {
            continue;
        };
        if seg_info_rel == 0 {
            continue;
        }
        let si = starts_offset + seg_info_rel as usize;
        if si + 22 > cf.len() {
            continue;
        }
        let page_size = rd_u16_le(cf, si + 4).unwrap_or(4096) as usize;
        let ptr_format = rd_u16_le(cf, si + 6).unwrap_or(0);
        let seg_file_offset = rd_u64_le(cf, si + 8).unwrap_or(0);
        let page_count = rd_u16_le(cf, si + 20).unwrap_or(0) as usize;

        // Stride in bytes per `next` step: 8 bytes for arm64e formats (1, 7, 8, 12), 4 bytes for ptr_64 formats (2, 6)
        let stride: usize = match ptr_format {
            1 | 7 | 8 | 12 => 8,
            2 | 6 | 9 => 4,
            _ => continue,
        };

        for p in 0..page_count {
            let Some(page_start) = rd_u16_le(cf, si + 22 + p * 2) else {
                break;
            };
            if page_start == 0xFFFF {
                continue; // DYLD_CHAINED_PTR_START_NONE
            }
            let mut chain_file_off =
                (seg_file_offset as usize) + p * page_size + (page_start as usize);
            let mut guard = 0usize;
            while chain_file_off + 8 <= file_bytes.len() && guard < 4096 {
                guard += 1;
                let raw_val = rd_u64_le(file_bytes, chain_file_off).unwrap_or(0);
                // Convert file offset to VMA via segment offset (for dylibs, file_offset maps 1:1 in segment or find via `min_vma` + offset)
                // Compute VMA from `seg_file_offset` (`segment_offset` in `dyld_chained_starts_in_segment` is actually VM offset from mach_header!)
                let target_vma = min_vma + rebase_bias + (chain_file_off as u64);
                let img_idx = (target_vma.saturating_sub(img.base_vma)) as usize;

                let (next_steps, resolved_opt) = match ptr_format {
                    2 | 6 => {
                        // DYLD_CHAINED_PTR_64 (2) / DYLD_CHAINED_PTR_64_OFFSET (6)
                        let bind = (raw_val >> 63) & 1;
                        let next = ((raw_val >> 51) & 0xFFF) as usize;
                        if bind == 1 {
                            let ord = (raw_val & 0xFF_FFFF) as usize;
                            let sym = import_syms.get(ord).map(|s| s.as_str()).unwrap_or("");
                            let val = resolve_macho_bind_sym(img, sym, canary_vma);
                            (next, val)
                        } else {
                            let target = raw_val & 0x000F_FFFF_FFFF;
                            let v = if ptr_format == 6 {
                                min_vma + rebase_bias + target
                            } else {
                                target + rebase_bias
                            };
                            (next, Some(v))
                        }
                    }
                    1 | 7 | 8 | 12 => {
                        // DYLD_CHAINED_PTR_ARM64E / USERLAND (7) / USERLAND24 (12)
                        let auth = (raw_val >> 63) & 1;
                        let bind = (raw_val >> 62) & 1;
                        let next = ((raw_val >> 51) & 0x7FF) as usize;
                        if bind == 1 {
                            let ord_mask = if ptr_format == 12 { 0xFF_FFFF } else { 0xFFFF };
                            let ord = (raw_val & ord_mask) as usize;
                            let sym = import_syms.get(ord).map(|s| s.as_str()).unwrap_or("");
                            let val = resolve_macho_bind_sym(img, sym, canary_vma);
                            (next, val)
                        } else if auth == 1 {
                            let target = raw_val & 0xFFFF_FFFF;
                            (next, Some(min_vma + rebase_bias + target))
                        } else {
                            let target = raw_val & 0x0007_FFFF_FFFF;
                            let v = if ptr_format == 1 {
                                target + rebase_bias
                            } else {
                                min_vma + rebase_bias + target
                            };
                            (next, Some(v))
                        }
                    }
                    _ => (0, None),
                };

                if let Some(resolved) = resolved_opt {
                    wr_u64_le(&mut img.image_bytes, img_idx, resolved);
                }
                if next_steps == 0 {
                    break;
                }
                chain_file_off += next_steps * stride;
            }
        }
    }
}

fn resolve_macho_bind_sym(
    img: &mut ParsedBinaryImage,
    sym: &str,
    canary_vma: u64,
) -> Option<u64> {
    if sym.is_empty() {
        return None;
    }
    if let Some(&int_vma) = img.symbols.get(sym) {
        Some(int_vma)
    } else if sym.contains("stack_chk_guard") {
        Some(canary_vma)
    } else {
        Some(img.intern_external_symbol(sym))
    }
}

/// Multi-member Mach-O 64 `.o` relocatable linker (`MH_OBJECT`).
fn link_macho64_objects(objs: &[(String, &[u8])]) -> Result<ParsedBinaryImage, String> {
    if objs.is_empty() {
        return Err("No Mach-O objects provided".to_string());
    }
    let first = objs[0].1;
    let cputype = rd_u32_le(first, 4).unwrap_or(0);
    let (arch, abi) = match cputype {
        0x0100_0007 => (BinaryArch::X86_64, BinaryAbi::SysV64),
        0x0100_000C => (BinaryArch::Aarch64, BinaryAbi::AppleArm64),
        other => return Err(format!("Unsupported Mach-O object cputype 0x{:x}", other)),
    };

    let base_vma = 0x1000_0000u64;
    let mut image_bytes = Vec::new();
    let mut exec_ranges = Vec::new();
    // `(oidx, sect_1based) -> (new_vma, orig_addr, reloff, nreloc, size)`
    let mut sect_map: HashMap<(usize, usize), (u64, u64, usize, usize, usize)> = HashMap::new();
    let mut embedded_bitcode = None;

    for (oidx, (_, d)) in objs.iter().enumerate() {
        let ncmds = rd_u32_le(d, 16).unwrap_or(0) as usize;
        let mut co = 32usize;
        for _ in 0..ncmds {
            if co + 8 > d.len() {
                break;
            }
            let cmd = rd_u32_le(d, co).unwrap_or(0);
            let cmdsize = rd_u32_le(d, co + 4).unwrap_or(0) as usize;
            if cmd == 0x19 && cmdsize >= 72 {
                let nsects = rd_u32_le(d, co + 64).unwrap_or(0) as usize;
                for s in 0..nsects {
                    let so = co + 72 + s * 80;
                    if so + 80 > d.len() {
                        break;
                    }
                    let sectname = std::str::from_utf8(&d[so..so + 16])
                        .unwrap_or("")
                        .trim_end_matches('\0');
                    let segname = std::str::from_utf8(&d[so + 16..so + 32])
                        .unwrap_or("")
                        .trim_end_matches('\0');
                    let orig_addr = rd_u64_le(d, so + 32).unwrap_or(0);
                    let size = rd_u64_le(d, so + 40).unwrap_or(0) as usize;
                    let offset = rd_u32_le(d, so + 48).unwrap_or(0) as usize;
                    let align_pow = rd_u32_le(d, so + 52).unwrap_or(4).min(12);
                    let reloff = rd_u32_le(d, so + 56).unwrap_or(0) as usize;
                    let nreloc = rd_u32_le(d, so + 60).unwrap_or(0) as usize;
                    let flags = rd_u32_le(d, so + 64).unwrap_or(0);

                    if segname == "__LLVM" && sectname == "__bitcode" && size > 0 {
                        if let Some(bc) = d.get(offset..offset.saturating_add(size)) {
                            embedded_bitcode = Some(bc.to_vec());
                        }
                    }
                    if segname == "__DWARF" || sectname.starts_with("__debug") {
                        continue;
                    }
                    let aligned_off =
                        align_up(image_bytes.len() as u64, (1u64 << align_pow).max(16)) as usize;
                    image_bytes.resize(aligned_off + size.max(16), 0);
                    let is_zerofill = (flags & 0xFF) == 1;
                    if !is_zerofill && size > 0 {
                        if let Some(src) = d.get(offset..offset.saturating_add(size)) {
                            image_bytes[aligned_off..aligned_off + size].copy_from_slice(src);
                        }
                    }
                    let new_vma = base_vma + aligned_off as u64;
                    sect_map.insert((oidx, s + 1), (new_vma, orig_addr, reloff, nreloc, size));
                    // S_ATTR_PURE_INSTRUCTIONS (0x80000000) or S_ATTR_SOME_INSTRUCTIONS (0x00000400)
                    if (flags & 0x8000_0400) != 0 && size > 0 {
                        exec_ranges.push((new_vma, new_vma + size as u64));
                    }
                }
            }
            co += cmdsize;
        }
    }

    let canary_off = align_up(image_bytes.len() as u64, 16) as usize;
    image_bytes.resize(canary_off + 32, 0);
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = base_vma + canary_off as u64;

    let mut img = ParsedBinaryImage {
        format_label: format!("Mach-O64-object ({})", arch.as_str()),
        arch,
        abi,
        base_vma,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode,
    };

    // Parse symbol tables across objects
    let mut obj_syms: Vec<Vec<(String, u8, u8, u64)>> = Vec::with_capacity(objs.len());
    for (oidx, (_, d)) in objs.iter().enumerate() {
        let ncmds = rd_u32_le(d, 16).unwrap_or(0) as usize;
        let mut co = 32usize;
        let mut syms = Vec::new();
        for _ in 0..ncmds {
            if co + 8 > d.len() {
                break;
            }
            let cmd = rd_u32_le(d, co).unwrap_or(0);
            let cmdsize = rd_u32_le(d, co + 4).unwrap_or(0) as usize;
            if cmd == 0x02 && cmdsize >= 24 {
                let symoff = rd_u32_le(d, co + 8).unwrap_or(0) as usize;
                let nsyms = rd_u32_le(d, co + 12).unwrap_or(0) as usize;
                let stroff = rd_u32_le(d, co + 16).unwrap_or(0) as usize;
                let strsize = rd_u32_le(d, co + 20).unwrap_or(0) as usize;
                if let Some(strtab) = d.get(stroff..stroff.saturating_add(strsize)) {
                    for k in 0..nsyms {
                        let so = symoff + k * 16;
                        if so + 16 > d.len() {
                            break;
                        }
                        let nm = rd_cstr(strtab, rd_u32_le(d, so).unwrap_or(0) as usize);
                        let n_type = d[so + 4];
                        let n_sect = d[so + 5];
                        let n_val = rd_u64_le(d, so + 8).unwrap_or(0);
                        let resolved_vma = if (n_type & 0x0E) == 0x0E {
                            if let Some(&(svma, orig_base, _, _, _)) =
                                sect_map.get(&(oidx, n_sect as usize))
                            {
                                let v = svma + n_val.saturating_sub(orig_base);
                                if (n_type & 0xE0) == 0 && !nm.is_empty() {
                                    if (n_type & 0x01) != 0 {
                                        // N_EXT
                                        img.symbols.entry(nm.clone()).or_insert(v);
                                        if let Some(unp) = nm.strip_prefix('_') {
                                            if !unp.is_empty() {
                                                img.symbols.entry(unp.to_string()).or_insert(v);
                                            }
                                        }
                                    }
                                    img.addr_to_sym.entry(v).or_insert(nm.clone());
                                }
                                v
                            } else {
                                n_val
                            }
                        } else if (n_type & 0x0E) == 0 && (n_type & 0x01) != 0 && n_val > 0 {
                            // Common tentative symbol
                            let off = align_up(img.image_bytes.len() as u64, 16) as usize;
                            img.image_bytes.resize(off + (n_val as usize).max(8), 0);
                            let v = base_vma + off as u64;
                            img.symbols.entry(nm.clone()).or_insert(v);
                            v
                        } else {
                            0
                        };
                        syms.push((nm, n_type, n_sect, resolved_vma));
                    }
                }
                break;
            }
            co += cmdsize;
        }
        obj_syms.push(syms);
    }

    // Apply Mach-O section relocations
    let mut got_cache: HashMap<u64, u64> = HashMap::new();
    let sect_entries: Vec<((usize, usize), (u64, u64, usize, usize, usize))> =
        sect_map.iter().map(|(&k, &v)| (k, v)).collect();
    for ((oidx, _), (sec_vma, _, reloff, nreloc, _)) in sect_entries {
        let d = objs[oidx].1;
        let mut pending_arm64_addend: i64 = 0;
        for r in 0..nreloc {
            let ro = reloff + r * 8;
            if ro + 8 > d.len() {
                break;
            }
            let r_addr = rd_u32_le(d, ro).unwrap_or(0) as u64;
            let r_info = rd_u32_le(d, ro + 4).unwrap_or(0);
            let r_symbolnum = (r_info & 0x00FF_FFFF) as usize;
            let r_pcrel = ((r_info >> 24) & 1) != 0;
            let r_length = (r_info >> 25) & 3;
            let r_extern = ((r_info >> 27) & 1) != 0;
            let r_type = (r_info >> 28) & 0xF;

            // ARM64_RELOC_ADDEND (10): sets signed 24-bit addend for the NEXT relocation!
            if arch == BinaryArch::Aarch64 && r_type == 10 {
                let raw24 = (r_info & 0x00FF_FFFF) as i32;
                pending_arm64_addend = ((raw24 << 8) >> 8) as i64;
                continue;
            }
            let addend = pending_arm64_addend;
            pending_arm64_addend = 0;

            let place_vma = sec_vma + r_addr;
            let place_idx = (place_vma - img.base_vma) as usize;

            let mut is_ext = false;
            let mut sym_name = String::new();
            let target_s = if r_extern {
                if let Some((nm, n_type, _, local_vma)) = obj_syms[oidx].get(r_symbolnum).cloned() {
                    sym_name = nm;
                    if (n_type & 0x0E) == 0x0E && local_vma != 0 {
                        local_vma
                    } else if let Some(&gv) = img.symbols.get(&sym_name) {
                        gv
                    } else if sym_name.contains("stack_chk_guard") {
                        canary_vma
                    } else {
                        is_ext = true;
                        img.intern_external_symbol(&sym_name)
                    }
                } else {
                    continue;
                }
            } else if let Some(&(svma, orig_sbase, _, _, _)) = sect_map.get(&(oidx, r_symbolnum)) {
                // Non-extern relocation: read embedded value in instruction/data and rebase from `orig_sbase` -> `svma`
                if r_length == 3 {
                    let cur = rd_u64_le(&img.image_bytes, place_idx).unwrap_or(0);
                    svma + cur.saturating_sub(orig_sbase)
                } else {
                    svma
                }
            } else {
                continue;
            };

            match arch {
                BinaryArch::X86_64 => {
                    // X86_64_RELOC_UNSIGNED(0), SIGNED(1), BRANCH(2), GOT_LOAD(3), GOT(4)
                    match r_type {
                        0 if !r_pcrel && r_length == 3 => {
                            let cur_add = if r_extern {
                                rd_i64_le(&img.image_bytes, place_idx).unwrap_or(0)
                            } else {
                                0
                            };
                            wr_u64_le(
                                &mut img.image_bytes,
                                place_idx,
                                (target_s as i64).wrapping_add(cur_add) as u64,
                            );
                        }
                        1 | 2 => {
                            let cur_disp = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0) as i32 as i64;
                            let dst = if is_ext && r_type == 2 {
                                alloc_synthetic_x86_plt_thunk(&mut img, &sym_name, target_s, &mut got_cache)
                            } else {
                                target_s
                            };
                            let extra = match r_type {
                                6 => -1,
                                7 => -2,
                                8 => -4,
                                _ => 0,
                            };
                            let rel = (dst as i64)
                                .wrapping_add(cur_disp)
                                .wrapping_add(extra)
                                .wrapping_sub((place_vma + 4) as i64);
                            wr_u32_le(&mut img.image_bytes, place_idx, rel as i32 as u32);
                        }
                        3 | 4 => {
                            let got_vma = if let Some(&g) = got_cache.get(&target_s) {
                                g
                            } else {
                                let goff = align_up(img.image_bytes.len() as u64, 8) as usize;
                                img.image_bytes.resize(goff + 8, 0);
                                wr_u64_le(&mut img.image_bytes, goff, target_s);
                                let gv = img.base_vma + goff as u64;
                                got_cache.insert(target_s, gv);
                                gv
                            };
                            let rel = (got_vma as i64).wrapping_sub((place_vma + 4) as i64);
                            wr_u32_le(&mut img.image_bytes, place_idx, rel as i32 as u32);
                        }
                        _ => {}
                    }
                }
                BinaryArch::Aarch64 => {
                    // ARM64_RELOC_UNSIGNED(0), BRANCH26(2), PAGE21(3), PAGEOFF12(4), GOT_LOAD_PAGE21(5), GOT_LOAD_PAGEOFF12(6)
                    let sa = (target_s as i64).wrapping_add(addend) as u64;
                    match r_type {
                        0 if r_length == 3 => {
                            let cur_add = if r_extern {
                                rd_i64_le(&img.image_bytes, place_idx).unwrap_or(0)
                            } else {
                                0
                            };
                            wr_u64_le(
                                &mut img.image_bytes,
                                place_idx,
                                (sa as i64).wrapping_add(cur_add) as u64,
                            );
                        }
                        2 => {
                            let dst = if is_ext {
                                alloc_synthetic_arm64_plt_thunk(&mut img, &sym_name)
                            } else {
                                sa
                            };
                            let imm26 = (((dst as i64).wrapping_sub(place_vma as i64) >> 2) as u32)
                                & 0x03FF_FFFF;
                            let orig = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0);
                            wr_u32_le(&mut img.image_bytes, place_idx, (orig & 0xFC00_0000) | imm26);
                        }
                        3 | 5 => {
                            let dst = if r_type == 5 {
                                if let Some(&g) = got_cache.get(&target_s) {
                                    g
                                } else {
                                    let goff = align_up(img.image_bytes.len() as u64, 8) as usize;
                                    img.image_bytes.resize(goff + 8, 0);
                                    wr_u64_le(&mut img.image_bytes, goff, target_s);
                                    let gv = img.base_vma + goff as u64;
                                    got_cache.insert(target_s, gv);
                                    gv
                                }
                            } else {
                                sa
                            };
                            patch_arm64_adrp(&mut img.image_bytes, place_idx, place_vma, dst);
                        }
                        4 | 6 => {
                            let dst = if r_type == 6 {
                                *got_cache.get(&target_s).unwrap_or(&sa)
                            } else {
                                sa
                            };
                            let orig = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0);
                            // Check if instruction is `ADD` (`(orig & 0x1F00_0000) == 0x1100_0000`) vs Load/Store (`size` in bits [31:30])
                            let shift = if (orig & 0x1F00_0000) == 0x1100_0000 {
                                0
                            } else {
                                let sz = (orig >> 30) & 3;
                                let is_vec_128 = ((orig >> 26) & 1) == 1 && ((orig >> 23) & 1) == 1 && sz == 0;
                                if is_vec_128 { 4 } else { sz }
                            };
                            let imm12 = (((dst & 0xFFF) >> shift) as u32) << 10;
                            wr_u32_le(
                                &mut img.image_bytes,
                                place_idx,
                                (orig & !(0xFFF << 10)) | imm12,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(img)
}

// ============================================================================
// 4. PE32+ (`.dll`) & COFF (`.obj` / `.lib`) Parser
// ============================================================================

fn parse_pe_dll(d: &[u8]) -> Result<ParsedBinaryImage, String> {
    if d.len() < 0x40 || &d[0..2] != b"MZ" {
        return Err("Invalid MZ DOS header".to_string());
    }
    let pe_off = rd_u32_le(d, 0x3C).ok_or("Missing e_lfanew")? as usize;
    if pe_off + 24 > d.len() || &d[pe_off..pe_off + 4] != b"PE\0\0" {
        return Err("Invalid PE signature".to_string());
    }
    let coff_off = pe_off + 4;
    let machine = rd_u16_le(d, coff_off).unwrap_or(0);
    let (arch, abi) = match machine {
        0x8664 => (BinaryArch::X86_64, BinaryAbi::Win64),
        0xAA64 => (BinaryArch::Aarch64, BinaryAbi::Aapcs64),
        other => return Err(format!("Unsupported PE machine 0x{:x}", other)),
    };
    let nsects = rd_u16_le(d, coff_off + 2).unwrap_or(0) as usize;
    let opt_sz = rd_u16_le(d, coff_off + 16).unwrap_or(0) as usize;
    let opt_off = coff_off + 20;
    if opt_sz < 112 || opt_off + opt_sz > d.len() {
        return Err("Truncated PE OptionalHeader".to_string());
    }
    let opt_magic = rd_u16_le(d, opt_off).unwrap_or(0);
    if opt_magic != 0x020B {
        return Err("Only PE32+ (64-bit) DLLs are supported".to_string());
    }

    let raw_image_base = rd_u64_le(d, opt_off + 24).unwrap_or(0x1_8000_0000);
    let image_base = if raw_image_base < 0x10000 {
        0x1_8000_0000
    } else {
        raw_image_base
    };
    let size_of_image = rd_u32_le(d, opt_off + 56).unwrap_or(0) as usize;
    let num_rva_dirs = rd_u32_le(d, opt_off + 108).unwrap_or(0) as usize;
    if size_of_image == 0 || size_of_image > 256 * 1024 * 1024 {
        return Err("Invalid PE SizeOfImage".to_string());
    }

    let canary_off = align_up(size_of_image as u64, 16) as usize;
    let mut image_bytes = vec![0u8; canary_off + 64];
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = image_base + canary_off as u64;
    let mut exec_ranges = Vec::new();
    let sect_base = opt_off + opt_sz;
    for i in 0..nsects {
        let so = sect_base + i * 40;
        if so + 40 > d.len() {
            break;
        }
        let virt_sz = rd_u32_le(d, so + 8).unwrap_or(0) as usize;
        let virt_rva = rd_u32_le(d, so + 12).unwrap_or(0) as usize;
        let raw_sz = rd_u32_le(d, so + 16).unwrap_or(0) as usize;
        let raw_ptr = rd_u32_le(d, so + 20).unwrap_or(0) as usize;
        let chars = rd_u32_le(d, so + 36).unwrap_or(0);

        let copy_len = raw_sz.min(virt_sz.max(raw_sz));
        if raw_ptr > 0 && copy_len > 0 {
            if let Some(src) = d.get(raw_ptr..raw_ptr.saturating_add(copy_len)) {
                if let Some(dst) = image_bytes.get_mut(virt_rva..virt_rva.saturating_add(src.len()))
                {
                    dst.copy_from_slice(src);
                }
            }
        }
        // IMAGE_SCN_MEM_EXECUTE (0x20000000) or IMAGE_SCN_CNT_CODE (0x20)
        if (chars & 0x2000_0020) != 0 && virt_sz > 0 {
            exec_ranges.push((
                image_base + virt_rva as u64,
                image_base + (virt_rva + virt_sz) as u64,
            ));
        }
    }

    // Apply `.reloc` base relocations if `image_base != raw_image_base`
    let delta = (image_base as i64).wrapping_sub(raw_image_base as i64);
    if delta != 0 && num_rva_dirs > 5 {
        let reloc_rva = rd_u32_le(d, opt_off + 112 + 5 * 8).unwrap_or(0) as usize;
        let reloc_sz = rd_u32_le(d, opt_off + 112 + 5 * 8 + 4).unwrap_or(0) as usize;
        let mut pos = reloc_rva;
        let end = reloc_rva.saturating_add(reloc_sz).min(image_bytes.len());
        while pos + 8 <= end {
            let page_rva = rd_u32_le(&image_bytes, pos).unwrap_or(0) as usize;
            let block_sz = rd_u32_le(&image_bytes, pos + 4).unwrap_or(0) as usize;
            if block_sz < 8 || pos + block_sz > end {
                break;
            }
            let entries = (block_sz - 8) / 2;
            for k in 0..entries {
                let ent = rd_u16_le(&image_bytes, pos + 8 + k * 2).unwrap_or(0);
                let rtype = ent >> 12;
                let roff = (ent & 0x0FFF) as usize;
                if rtype == 10 {
                    // IMAGE_REL_BASED_DIR64
                    let idx = page_rva + roff;
                    if let Some(cur) = rd_u64_le(&image_bytes, idx) {
                        wr_u64_le(&mut image_bytes, idx, (cur as i64).wrapping_add(delta) as u64);
                    }
                }
            }
            pos += block_sz;
        }
    }

    let mut img = ParsedBinaryImage {
        format_label: format!("PE32+ DLL ({})", arch.as_str()),
        arch,
        abi,
        base_vma: image_base,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode: None,
    };

    // 1. Parse Export Directory (`DataDirectory[0]`)
    if num_rva_dirs > 0 {
        let exp_rva = rd_u32_le(d, opt_off + 112).unwrap_or(0) as usize;
        let exp_sz = rd_u32_le(d, opt_off + 116).unwrap_or(0) as usize;
        if exp_rva > 0 && exp_rva + 40 <= img.image_bytes.len() {
            let num_funcs = rd_u32_le(&img.image_bytes, exp_rva + 20).unwrap_or(0) as usize;
            let num_names = rd_u32_le(&img.image_bytes, exp_rva + 24).unwrap_or(0) as usize;
            let addr_funcs = rd_u32_le(&img.image_bytes, exp_rva + 28).unwrap_or(0) as usize;
            let addr_names = rd_u32_le(&img.image_bytes, exp_rva + 32).unwrap_or(0) as usize;
            let addr_ords = rd_u32_le(&img.image_bytes, exp_rva + 36).unwrap_or(0) as usize;

            for i in 0..num_names.min(65536) {
                let Some(name_rva) = rd_u32_le(&img.image_bytes, addr_names + i * 4) else {
                    break;
                };
                let Some(ord) = rd_u16_le(&img.image_bytes, addr_ords + i * 2) else {
                    break;
                };
                if (ord as usize) >= num_funcs {
                    continue;
                }
                let Some(fn_rva) = rd_u32_le(&img.image_bytes, addr_funcs + (ord as usize) * 4)
                else {
                    continue;
                };
                let fr = fn_rva as usize;
                // Exclude forwarded exports pointing inside the export directory itself
                if fr >= exp_rva && fr < exp_rva + exp_sz {
                    continue;
                }
                let nm = rd_cstr(&img.image_bytes, name_rva as usize);
                if !nm.is_empty() && fr > 0 {
                    let vma = image_base + fr as u64;
                    img.symbols.insert(nm.clone(), vma);
                    img.addr_to_sym.entry(vma).or_insert(nm);
                }
            }
        }
    }

    // 2. Parse Import Directory (`DataDirectory[1]`) -> populate IAT slots with external sentinels!
    let mut iat_slot_to_sym: HashMap<u64, String> = HashMap::new();
    if num_rva_dirs > 1 {
        let imp_rva = rd_u32_le(d, opt_off + 112 + 8).unwrap_or(0) as usize;
        if imp_rva > 0 {
            let mut desc_off = imp_rva;
            while desc_off + 20 <= img.image_bytes.len() {
                let orig_first_thunk = rd_u32_le(&img.image_bytes, desc_off).unwrap_or(0) as usize;
                let dll_name_rva = rd_u32_le(&img.image_bytes, desc_off + 12).unwrap_or(0) as usize;
                let first_thunk = rd_u32_le(&img.image_bytes, desc_off + 16).unwrap_or(0) as usize;
                if orig_first_thunk == 0 && dll_name_rva == 0 && first_thunk == 0 {
                    break;
                }
                let ilt_base = if orig_first_thunk != 0 {
                    orig_first_thunk
                } else {
                    first_thunk
                };
                let mut slot = 0usize;
                while ilt_base + slot * 8 + 8 <= img.image_bytes.len()
                    && first_thunk + slot * 8 + 8 <= img.image_bytes.len()
                {
                    let thunk_data = rd_u64_le(&img.image_bytes, ilt_base + slot * 8).unwrap_or(0);
                    if thunk_data == 0 {
                        break;
                    }
                    let sym_name = if (thunk_data & (1u64 << 63)) != 0 {
                        format!("ordinal_{}", thunk_data & 0xFFFF)
                    } else {
                        // `IMAGE_IMPORT_BY_NAME`: 2-byte `Hint` followed by ASCII name
                        let hint_name_rva = (thunk_data & 0x7FFF_FFFF) as usize;
                        rd_cstr(&img.image_bytes, hint_name_rva + 2)
                    };
                    if !sym_name.is_empty() {
                        let iat_off = first_thunk + slot * 8;
                        let norm_nm = normalize_ext_sym_name(&sym_name);
                        if matches!(
                            norm_nm.as_str(),
                            "__stack_chk_guard" | "___stack_chk_guard" | "__security_cookie"
                        ) {
                            wr_u64_le(&mut img.image_bytes, iat_off, canary_vma);
                        } else {
                            let sentinel = img.intern_external_symbol(&sym_name);
                            wr_u64_le(&mut img.image_bytes, iat_off, sentinel);
                            let iat_vma = image_base + iat_off as u64;
                            iat_slot_to_sym.insert(iat_vma, norm_nm);
                        }
                    }
                    slot += 1;
                }
                desc_off += 20;
            }
        }
    }

    // 3. Scan executable ranges for 6-byte `jmp qword ptr [rip + disp32]` (`FF 25 <disp32>`) import thunks
    if arch == BinaryArch::X86_64 && !iat_slot_to_sym.is_empty() {
        let ranges = img.exec_ranges.clone();
        for (lo, hi) in ranges {
            let mut cur = lo;
            while cur + 6 <= hi {
                if let Some(b) = img.read_bytes(cur, 6) {
                    if b[0] == 0xFF && b[1] == 0x25 {
                        let disp = i32::from_le_bytes([b[2], b[3], b[4], b[5]]) as i64;
                        let target_iat = ((cur + 6) as i64).wrapping_add(disp) as u64;
                        if let Some(ext_sym) = iat_slot_to_sym.get(&target_iat) {
                            img.plt_stubs.insert(cur, ext_sym.clone());
                        }
                    }
                }
                cur += 1;
            }
        }
    }

    Ok(img)
}

/// Multi-member COFF 64-bit `.obj` / static `.lib` linker.
fn parse_coff_objects(objs: &[(String, &[u8])]) -> Result<ParsedBinaryImage, String> {
    if objs.is_empty() {
        return Err("No COFF objects provided".to_string());
    }
    let machine = rd_u16_le(objs[0].1, 0).unwrap_or(0);
    let (arch, abi) = match machine {
        0x8664 => (BinaryArch::X86_64, BinaryAbi::Win64),
        0xAA64 => (BinaryArch::Aarch64, BinaryAbi::Aapcs64),
        other => return Err(format!("Unsupported COFF machine 0x{:x}", other)),
    };

    let base_vma = 0x1000_0000u64;
    let mut image_bytes = Vec::new();
    let mut exec_ranges = Vec::new();
    let mut sec_vmas: HashMap<(usize, usize), (u64, usize, usize)> = HashMap::new();

    for (oidx, (_, d)) in objs.iter().enumerate() {
        let nsects = rd_u16_le(d, 2).unwrap_or(0) as usize;
        let opt_sz = rd_u16_le(d, 16).unwrap_or(0) as usize;
        let sect_base = 20 + opt_sz;
        for s in 0..nsects {
            let so = sect_base + s * 40;
            if so + 40 > d.len() {
                break;
            }
            let name = std::str::from_utf8(&d[so..so + 8])
                .unwrap_or("")
                .trim_end_matches('\0');
            if name.starts_with(".debug") || name == ".drectve" {
                continue;
            }
            let raw_sz = rd_u32_le(d, so + 16).unwrap_or(0) as usize;
            let raw_ptr = rd_u32_le(d, so + 20).unwrap_or(0) as usize;
            let rel_ptr = rd_u32_le(d, so + 24).unwrap_or(0) as usize;
            let nreloc = rd_u16_le(d, so + 32).unwrap_or(0) as usize;
            let chars = rd_u32_le(d, so + 36).unwrap_or(0);

            let aligned_off = align_up(image_bytes.len() as u64, 16) as usize;
            image_bytes.resize(aligned_off + raw_sz.max(16), 0);
            if raw_ptr > 0 && raw_sz > 0 && (chars & 0x80) == 0 {
                if let Some(src) = d.get(raw_ptr..raw_ptr.saturating_add(raw_sz)) {
                    image_bytes[aligned_off..aligned_off + raw_sz].copy_from_slice(src);
                }
            }
            let vma = base_vma + aligned_off as u64;
            sec_vmas.insert((oidx, s + 1), (vma, rel_ptr, nreloc));
            if (chars & 0x2000_0020) != 0 && raw_sz > 0 {
                exec_ranges.push((vma, vma + raw_sz as u64));
            }
        }
    }

    let canary_off = align_up(image_bytes.len() as u64, 16) as usize;
    image_bytes.resize(canary_off + 32, 0);
    wr_u64_le(&mut image_bytes, canary_off, 0x5A5A_5A5A_A5A5_A5A5);
    let canary_vma = base_vma + canary_off as u64;

    let mut img = ParsedBinaryImage {
        format_label: format!("COFF64-object ({})", arch.as_str()),
        arch,
        abi,
        base_vma,
        image_bytes,
        exec_ranges,
        symbols: HashMap::new(),
        addr_to_sym: HashMap::new(),
        plt_stubs: HashMap::new(),
        ext_sentinels: HashMap::new(),
        ext_sym_to_sentinel: HashMap::new(),
        embedded_bitcode: None,
    };
    img.symbols.insert("__ImageBase".to_string(), base_vma);
    img.symbols.insert("__stack_chk_guard".to_string(), canary_vma);
    img.symbols.insert("___stack_chk_guard".to_string(), canary_vma);
    img.symbols.insert("__security_cookie".to_string(), canary_vma);

    // Parse COFF symbol tables (18 bytes per symbol + 4-byte string-table size right after symbol table!)
    let mut obj_syms: Vec<BTreeMap<usize, (String, i16, u8, u64)>> = Vec::with_capacity(objs.len());
    for (oidx, (_, d)) in objs.iter().enumerate() {
        let sym_ptr = rd_u32_le(d, 8).unwrap_or(0) as usize;
        let nsyms = rd_u32_le(d, 12).unwrap_or(0) as usize;
        let str_base = sym_ptr.saturating_add(nsyms * 18);
        let str_sz = rd_u32_le(d, str_base).unwrap_or(0) as usize;
        let strtab = d
            .get(str_base..str_base.saturating_add(str_sz))
            .unwrap_or(&[]);

        let mut smap = BTreeMap::new();
        let mut idx = 0usize;
        while idx < nsyms {
            let so = sym_ptr + idx * 18;
            if so + 18 > d.len() {
                break;
            }
            let nm = if rd_u32_le(d, so) == Some(0) {
                let stroff = rd_u32_le(d, so + 4).unwrap_or(0) as usize;
                rd_cstr(strtab, stroff)
            } else {
                std::str::from_utf8(&d[so..so + 8])
                    .unwrap_or("")
                    .trim_end_matches('\0')
                    .to_string()
            };
            let val = rd_u32_le(d, so + 8).unwrap_or(0) as u64;
            let sect_num = rd_u16_le(d, so + 12).unwrap_or(0) as i16;
            let storage_class = d[so + 16];
            let naux = d[so + 17] as usize;

            let resolved = if sect_num > 0 {
                if let Some(&(svma, _, _)) = sec_vmas.get(&(oidx, sect_num as usize)) {
                    let v = svma + val;
                    // IMAGE_SYM_CLASS_EXTERNAL = 2
                    if storage_class == 2 && !nm.is_empty() {
                        img.symbols.entry(nm.clone()).or_insert(v);
                        img.addr_to_sym.entry(v).or_insert(nm.clone());
                    }
                    v
                } else {
                    val
                }
            } else if sect_num == 0 && storage_class == 2 && val > 0 {
                // Common uninitialized global
                let off = align_up(img.image_bytes.len() as u64, 16) as usize;
                img.image_bytes.resize(off + (val as usize).max(8), 0);
                let v = base_vma + off as u64;
                img.symbols.entry(nm.clone()).or_insert(v);
                v
            } else {
                0
            };
            smap.insert(idx, (nm, sect_num, storage_class, resolved));
            idx += 1 + naux;
        }
        obj_syms.push(smap);
    }

    // Apply COFF section relocations (10 bytes each: `VirtualAddress: u32, SymbolTableIndex: u32, Type: u16`)
    let mut got_cache: HashMap<u64, u64> = HashMap::new();
    let entries: Vec<((usize, usize), (u64, usize, usize))> =
        sec_vmas.iter().map(|(&k, &v)| (k, v)).collect();
    for ((oidx, _), (sec_vma, rel_ptr, nreloc)) in entries {
        let d = objs[oidx].1;
        for r in 0..nreloc {
            let ro = rel_ptr + r * 10;
            if ro + 10 > d.len() {
                break;
            }
            let r_va = rd_u32_le(d, ro).unwrap_or(0) as u64;
            let sym_idx = rd_u32_le(d, ro + 4).unwrap_or(0) as usize;
            let r_type = rd_u16_le(d, ro + 8).unwrap_or(0);

            let place_vma = sec_vma + r_va;
            let place_idx = (place_vma - img.base_vma) as usize;
            let Some((sym_name, sect_num, _, local_vma)) = obj_syms[oidx].get(&sym_idx).cloned()
            else {
                continue;
            };

            let mut is_ext = false;
            let target_s = if sect_num > 0 && local_vma != 0 {
                local_vma
            } else if let Some(&gv) = img.symbols.get(&sym_name) {
                gv
            } else if let Some(imp_rest) = sym_name.strip_prefix("__imp_") {
                // `__imp_foo` in COFF object expects an 8-byte IAT pointer slot!
                let inner_target = if let Some(&gv) = img.symbols.get(imp_rest) {
                    gv
                } else {
                    img.intern_external_symbol(imp_rest)
                };
                if let Some(&slot) = got_cache.get(&inner_target) {
                    slot
                } else {
                    let off = align_up(img.image_bytes.len() as u64, 8) as usize;
                    img.image_bytes.resize(off + 8, 0);
                    wr_u64_le(&mut img.image_bytes, off, inner_target);
                    let sv = img.base_vma + off as u64;
                    got_cache.insert(inner_target, sv);
                    sv
                }
            } else {
                is_ext = true;
                img.intern_external_symbol(&sym_name)
            };

            if arch == BinaryArch::X86_64 {
                match r_type {
                    0x0001 => {
                        // IMAGE_REL_AMD64_ADDR64
                        let add = rd_i64_le(&img.image_bytes, place_idx).unwrap_or(0);
                        wr_u64_le(
                            &mut img.image_bytes,
                            place_idx,
                            (target_s as i64).wrapping_add(add) as u64,
                        );
                    }
                    0x0003 => {
                        // IMAGE_REL_AMD64_ADDR32NB (RVA from base_vma)
                        let add = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0) as i32 as i64;
                        let rva = (target_s.saturating_sub(img.base_vma) as i64).wrapping_add(add);
                        wr_u32_le(&mut img.image_bytes, place_idx, rva as i32 as u32);
                    }
                    0x0004..=0x0009 => {
                        // IMAGE_REL_AMD64_REL32 .. REL32_5
                        let dst = if is_ext && r_type == 0x0004 {
                            alloc_synthetic_x86_plt_thunk(&mut img, &sym_name, target_s, &mut got_cache)
                        } else {
                            target_s
                        };
                        let extra = (r_type - 0x0004) as i64;
                        let add = rd_u32_le(&img.image_bytes, place_idx).unwrap_or(0) as i32 as i64;
                        let rel = (dst as i64)
                            .wrapping_add(add)
                            .wrapping_sub((place_vma + 4) as i64 + extra);
                        wr_u32_le(&mut img.image_bytes, place_idx, rel as i32 as u32);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(img)
}
