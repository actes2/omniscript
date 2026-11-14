use super::chip::{RpChipKind, RpChipProfile};
use super::thumb_enc::ThumbAssembler;

/// Package a linked `0x1000_0000` XIP Flash binary into standard 512-byte UF2 blocks
/// for direct drag-and-drop `BOOTSEL` mass-storage or `picotool` flashing.
pub fn build_uf2_image(flash_bytes: &[u8], chip: &RpChipProfile) -> Vec<u8> {
    let num_blocks = ((flash_bytes.len().max(1) + 255) / 256) as u32;
    let mut uf2 = Vec::with_capacity((num_blocks as usize) * 512);

    for blk_idx in 0..num_blocks {
        let mut block = [0u8; 512];
        let src_off = (blk_idx as usize) * 256;
        let target_addr = chip.flash_base + blk_idx * 256;

        block[0..4].copy_from_slice(&0x0A32_4655u32.to_le_bytes()); // magicStart0 ("UF2\n")
        block[4..8].copy_from_slice(&0x9E5D_5157u32.to_le_bytes()); // magicStart1
        block[8..12].copy_from_slice(&0x0000_2000u32.to_le_bytes()); // UF2_FLAG_FAMILY_ID_PRESENT
        block[12..16].copy_from_slice(&target_addr.to_le_bytes());
        block[16..20].copy_from_slice(&256u32.to_le_bytes()); // payloadSize = 256
        block[20..24].copy_from_slice(&blk_idx.to_le_bytes());
        block[24..28].copy_from_slice(&num_blocks.to_le_bytes());
        block[28..32].copy_from_slice(&chip.uf2_family_id.to_le_bytes());

        if src_off < flash_bytes.len() {
            let copy_len = (flash_bytes.len() - src_off).min(256);
            block[32..32 + copy_len].copy_from_slice(&flash_bytes[src_off..src_off + copy_len]);
        }

        block[508..512].copy_from_slice(&0x0AB1_6F30u32.to_le_bytes()); // magicEnd
        uf2.extend_from_slice(&block);
    }

    uf2
}

/// Package a linked XIP Flash binary + function symbols into a standalone 32-bit ARM EABI5 ELF (`EM_ARM`)
/// complete with `.ARM.attributes`, `$t`/`$d` mapping symbols, and `.symtab` for `picotool`, `objdump`,
/// `rizin`, Ghidra, and `openocd` SWD debugging.
pub fn build_elf32_arm_image(
    flash_bytes: &[u8],
    asm: &ThumbAssembler,
    chip: &RpChipProfile,
) -> Vec<u8> {
    let reset_off = asm.symbols.get("Reset_Handler").copied().unwrap_or(0) as u32;
    let entry_vma = (chip.flash_base + reset_off) | 1;

    let arm_attrs = build_arm_aeabi_attributes(chip);

    // Build `.strtab` & `.symtab` (ELF32_Sym = 16 bytes each; ALL `STB_LOCAL` must precede `STB_GLOBAL`!)
    let mut strtab: Vec<u8> = vec![0];
    let mut add_str = |s: &str| -> u32 {
        let idx = strtab.len() as u32;
        strtab.extend_from_slice(s.as_bytes());
        strtab.push(0);
        idx
    };

    let mut local_syms: Vec<[u8; 16]> = Vec::new();
    let mut global_syms: Vec<[u8; 16]> = Vec::new();

    // Symbol 0: STN_UNDEF
    local_syms.push([0u8; 16]);

    let sh_text_idx: u16 = 1;
    let t_name = add_str("$t");
    let d_name = add_str("$d");

    // Mark boot2/vector/picobin data vs Thumb code regions cleanly via AAELF32 `$d` and `$t` mapping symbols!
    match chip.kind {
        RpChipKind::Rp2040 => {
            // boot2 code at 0x00..0x30 (`$t`), boot2 literals+CRC + Vector Table at 0x30..0x1C0 (`$d`)
            local_syms.push(encode_elf32_sym(t_name, chip.flash_base, 0, 0x00, sh_text_idx));
            local_syms.push(encode_elf32_sym(
                d_name,
                chip.flash_base + 0x30,
                0,
                0x00,
                sh_text_idx,
            ));
        }
        RpChipKind::Rp2350 => {
            // Vector Table + PICOBIN at 0x00..0x124 (`$d`)
            local_syms.push(encode_elf32_sym(d_name, chip.flash_base, 0, 0x00, sh_text_idx));
        }
    }

    for rec in &asm.fn_records {
        let fn_vma = chip.flash_base + (rec.offset as u32);
        local_syms.push(encode_elf32_sym(t_name, fn_vma, 0, 0x00, sh_text_idx));
        let s_name = add_str(&rec.name);
        // `STB_GLOBAL (1) << 4 | STT_FUNC (2)` = `0x12`, Thumb bit `| 1` on `st_value`
        global_syms.push(encode_elf32_sym(
            s_name,
            fn_vma | 1,
            rec.size as u32,
            0x12,
            sh_text_idx,
        ));
    }

    // Mark trailing `.rodata` after last function as `$d` so `objdump -d` stops disassembling at string literals
    if let Some(last_fn) = asm.fn_records.last() {
        let ro_off = last_fn.offset + last_fn.size;
        if ro_off < flash_bytes.len() {
            local_syms.push(encode_elf32_sym(
                d_name,
                chip.flash_base + (ro_off as u32),
                0,
                0x00,
                sh_text_idx,
            ));
        }
    }

    let first_global_sym_idx = local_syms.len() as u32;
    let mut symtab_bytes = Vec::with_capacity((local_syms.len() + global_syms.len()) * 16);
    for s in &local_syms {
        symtab_bytes.extend_from_slice(s);
    }
    for s in &global_syms {
        symtab_bytes.extend_from_slice(s);
    }

    // Build `.shstrtab`
    let mut shstrtab: Vec<u8> = vec![0];
    let mut add_shstr = |s: &str| -> u32 {
        let idx = shstrtab.len() as u32;
        shstrtab.extend_from_slice(s.as_bytes());
        shstrtab.push(0);
        idx
    };
    let sh_name_text = add_shstr(".text");
    let sh_name_attr = add_shstr(".ARM.attributes");
    let sh_name_symtab = add_shstr(".symtab");
    let sh_name_strtab = add_shstr(".strtab");
    let sh_name_shstr = add_shstr(".shstrtab");

    // File layout:
    //   0x0000..0x0034: Elf32_Ehdr (52 B)
    //   0x0034..0x0054: Elf32_Phdr (32 B)
    //   0x1000..      : `.text` (4 KB page aligned `PT_LOAD` segment)
    //   aligned 4     : `.ARM.attributes`
    //   aligned 4     : `.symtab`
    //   aligned 1     : `.strtab`
    //   aligned 1     : `.shstrtab`
    //   aligned 4     : Section Header Table (6 x 40 B)
    let mut elf = vec![0u8; 0x1000];
    let text_file_off = elf.len() as u32;
    let text_len = flash_bytes.len() as u32;
    elf.extend_from_slice(flash_bytes);
    while (elf.len() & 3) != 0 {
        elf.push(0);
    }

    let attr_off = elf.len() as u32;
    let attr_len = arm_attrs.len() as u32;
    elf.extend_from_slice(&arm_attrs);
    while (elf.len() & 3) != 0 {
        elf.push(0);
    }

    let symtab_off = elf.len() as u32;
    let symtab_len = symtab_bytes.len() as u32;
    elf.extend_from_slice(&symtab_bytes);

    let strtab_off = elf.len() as u32;
    let strtab_len = strtab.len() as u32;
    elf.extend_from_slice(&strtab);

    let shstr_off = elf.len() as u32;
    let shstr_len = shstrtab.len() as u32;
    elf.extend_from_slice(&shstrtab);
    while (elf.len() & 3) != 0 {
        elf.push(0);
    }

    let shoff = elf.len() as u32;
    // Section 0: NULL
    elf.extend_from_slice(&[0u8; 40]);
    // Section 1: `.text` (`SHT_PROGBITS = 1`, `SHF_ALLOC | SHF_EXECINSTR = 6`)
    elf.extend_from_slice(&encode_elf32_shdr(
        sh_name_text,
        1,
        0x6,
        chip.flash_base,
        text_file_off,
        text_len,
        0,
        0,
        4,
        0,
    ));
    // Section 2: `.ARM.attributes` (`SHT_ARM_ATTRIBUTES = 0x70000003`)
    elf.extend_from_slice(&encode_elf32_shdr(
        sh_name_attr,
        0x7000_0003,
        0,
        0,
        attr_off,
        attr_len,
        0,
        0,
        1,
        0,
    ));
    // Section 3: `.symtab` (`SHT_SYMTAB = 2`, link = 4 `.strtab`, info = first_global_sym_idx, entsize = 16)
    elf.extend_from_slice(&encode_elf32_shdr(
        sh_name_symtab,
        2,
        0,
        0,
        symtab_off,
        symtab_len,
        4,
        first_global_sym_idx,
        4,
        16,
    ));
    // Section 4: `.strtab` (`SHT_STRTAB = 3`)
    elf.extend_from_slice(&encode_elf32_shdr(
        sh_name_strtab,
        3,
        0,
        0,
        strtab_off,
        strtab_len,
        0,
        0,
        1,
        0,
    ));
    // Section 5: `.shstrtab` (`SHT_STRTAB = 3`)
    elf.extend_from_slice(&encode_elf32_shdr(
        sh_name_shstr,
        3,
        0,
        0,
        shstr_off,
        shstr_len,
        0,
        0,
        1,
        0,
    ));

    // Write `Elf32_Ehdr` at `0x00..0x34`
    let mut ehdr = [0u8; 52];
    ehdr[0..4].copy_from_slice(b"\x7FELF");
    ehdr[4] = 1; // ELFCLASS32
    ehdr[5] = 1; // ELFDATA2LSB
    ehdr[6] = 1; // EV_CURRENT
    ehdr[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    ehdr[18..20].copy_from_slice(&40u16.to_le_bytes()); // EM_ARM
    ehdr[20..24].copy_from_slice(&1u32.to_le_bytes()); // EV_CURRENT
    ehdr[24..28].copy_from_slice(&entry_vma.to_le_bytes());
    ehdr[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
    ehdr[32..36].copy_from_slice(&shoff.to_le_bytes()); // e_shoff
    // EF_ARM_EABI_VER5 (0x0500_0000) | EF_ARM_ABI_FLOAT_SOFT (0x0000_0200)
    ehdr[36..40].copy_from_slice(&0x0500_0200u32.to_le_bytes());
    ehdr[40..42].copy_from_slice(&52u16.to_le_bytes()); // e_ehsize
    ehdr[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
    ehdr[44..46].copy_from_slice(&1u16.to_le_bytes());  // e_phnum
    ehdr[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
    ehdr[48..50].copy_from_slice(&6u16.to_le_bytes());  // e_shnum
    ehdr[50..52].copy_from_slice(&5u16.to_le_bytes());  // e_shstrndx
    elf[0..52].copy_from_slice(&ehdr);

    // Write `Elf32_Phdr` (`PT_LOAD`) at `0x34..0x54`
    let mut phdr = [0u8; 32];
    phdr[0..4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    phdr[4..8].copy_from_slice(&text_file_off.to_le_bytes());
    phdr[8..12].copy_from_slice(&chip.flash_base.to_le_bytes()); // p_vaddr
    phdr[12..16].copy_from_slice(&chip.flash_base.to_le_bytes()); // p_paddr
    phdr[16..20].copy_from_slice(&text_len.to_le_bytes());
    phdr[20..24].copy_from_slice(&text_len.to_le_bytes());
    phdr[24..28].copy_from_slice(&5u32.to_le_bytes()); // PF_R | PF_X
    phdr[28..32].copy_from_slice(&0x1000u32.to_le_bytes()); // p_align
    elf[52..84].copy_from_slice(&phdr);

    elf
}

fn build_arm_aeabi_attributes(chip: &RpChipProfile) -> Vec<u8> {
    let (cpu_arch, thumb_isa) = match chip.kind {
        RpChipKind::Rp2040 => (12u8, 1u8), // ARMv6S-M, Thumb-1
        RpChipKind::Rp2350 => (17u8, 2u8), // ARMv8-M.main, Thumb-2
    };
    let tags: [u8; 8] = [
        6, cpu_arch, // Tag_CPU_arch
        7, b'M',     // Tag_CPU_arch_profile ('M')
        8, 0,        // Tag_ARM_ISA_use = No
        9, thumb_isa,// Tag_THUMB_ISA_use
    ];
    let file_sub_len = (1 + 4 + tags.len()) as u32;
    let vendor_sub_len = (4 + 6 + file_sub_len as usize) as u32;

    let mut out = Vec::with_capacity(1 + vendor_sub_len as usize);
    out.push(b'A'); // format-version 'A'
    out.extend_from_slice(&vendor_sub_len.to_le_bytes());
    out.extend_from_slice(b"aeabi\0");
    out.push(1); // Tag_File
    out.extend_from_slice(&file_sub_len.to_le_bytes());
    out.extend_from_slice(&tags);
    out
}

fn encode_elf32_sym(name: u32, value: u32, size: u32, info: u8, shndx: u16) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&name.to_le_bytes());
    b[4..8].copy_from_slice(&value.to_le_bytes());
    b[8..12].copy_from_slice(&size.to_le_bytes());
    b[12] = info;
    b[13] = 0;
    b[14..16].copy_from_slice(&shndx.to_le_bytes());
    b
}

#[allow(clippy::too_many_arguments)]
fn encode_elf32_shdr(
    name: u32,
    sh_type: u32,
    flags: u32,
    addr: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    addralign: u32,
    entsize: u32,
) -> [u8; 40] {
    let mut b = [0u8; 40];
    b[0..4].copy_from_slice(&name.to_le_bytes());
    b[4..8].copy_from_slice(&sh_type.to_le_bytes());
    b[8..12].copy_from_slice(&flags.to_le_bytes());
    b[12..16].copy_from_slice(&addr.to_le_bytes());
    b[16..20].copy_from_slice(&offset.to_le_bytes());
    b[20..24].copy_from_slice(&size.to_le_bytes());
    b[24..28].copy_from_slice(&link.to_le_bytes());
    b[28..32].copy_from_slice(&info.to_le_bytes());
    b[32..36].copy_from_slice(&addralign.to_le_bytes());
    b[36..40].copy_from_slice(&entsize.to_le_bytes());
    b
}
