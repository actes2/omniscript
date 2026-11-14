use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct AssembledPioProgram {
    pub name: String,
    pub instructions: Vec<u16>,
    pub jmp_indices: Vec<usize>,
    pub wrap_target: usize,
    pub wrap: usize,
    pub sideset_bits: u8,
    pub sideset_opt: bool,
    pub sideset_pindirs: bool,
    pub origin: Option<u8>,
    pub public_defines: HashMap<String, i64>,
}

/// Pure-Rust RP2040 / RP2350 PIO State-Machine Assembler (zero `pioasm` dependency).
pub fn assemble_pio_source(src: &str) -> Result<Vec<AssembledPioProgram>, String> {
    let stripped = strip_block_comments(src);
    let mut programs = Vec::new();
    let mut global_defines: HashMap<String, i64> = HashMap::new();

    let mut cur: Option<ProgParts> = None;

    for (lineno, raw_line) in stripped.lines().enumerate() {
        let line_no_cmt = trim_line_comment(raw_line).trim();
        if line_no_cmt.is_empty() || line_no_cmt.starts_with('%') {
            // `% c-sdk { ... %}` blocks ignored cleanly
            continue;
        }
        if line_no_cmt.starts_with(".program ") {
            if let Some(done) = cur.take() {
                programs.push(finalize_program(done, &global_defines)?);
            }
            let pname = line_no_cmt[".program ".len()..]
                .split_whitespace()
                .next()
                .ok_or_else(|| format!("line {}: missing .program name", lineno + 1))?
                .to_string();
            cur = Some(ProgParts {
                name: pname,
                ..Default::default()
            });
            continue;
        }

        if line_no_cmt.starts_with(".define ") {
            let rest = line_no_cmt[".define ".len()..].trim();
            let (is_pub, rem) = if let Some(r) = rest.strip_prefix("public ") {
                (true, r.trim())
            } else {
                (false, rest)
            };
            let mut parts = rem.splitn(2, char::is_whitespace);
            let sym = parts
                .next()
                .ok_or_else(|| format!("line {}: malformed .define", lineno + 1))?
                .trim();
            let expr = parts
                .next()
                .ok_or_else(|| format!("line {}: missing .define value", lineno + 1))?
                .trim();
            let val = if let Some(ref p) = cur {
                eval_pio_expr(expr, &p.defines, &global_defines)?
            } else {
                eval_pio_expr(expr, &global_defines, &global_defines)?
            };
            if let Some(ref mut p) = cur {
                p.defines.insert(sym.to_string(), val);
                if is_pub {
                    p.public_defines.insert(sym.to_string(), val);
                }
            } else {
                global_defines.insert(sym.to_string(), val);
            }
            continue;
        }

        // Ignore `% c-sdk` body lines if any slipped through, or `.lang_opt`
        if line_no_cmt.starts_with(".lang_opt") || line_no_cmt.starts_with(".pio_version") {
            continue;
        }

        // Auto-open default program if instructions appear before `.program`
        if cur.is_none() {
            cur = Some(ProgParts {
                name: "pio_prog".to_string(),
                ..Default::default()
            });
        }
        let prog = cur.as_mut().unwrap();

        if let Some(rest) = line_no_cmt.strip_prefix(".side_set ") {
            let toks: Vec<&str> = rest.split_whitespace().collect();
            if toks.is_empty() {
                return Err(format!("line {}: .side_set missing bit count", lineno + 1));
            }
            prog.sideset_bits = eval_pio_expr(toks[0], &prog.defines, &global_defines)? as u8;
            for &t in &toks[1..] {
                match t.to_ascii_lowercase().as_str() {
                    "opt" => prog.sideset_opt = true,
                    "pindirs" => prog.sideset_pindirs = true,
                    _ => {}
                }
            }
            continue;
        }

        if let Some(rest) = line_no_cmt.strip_prefix(".origin ") {
            let v = eval_pio_expr(rest.trim(), &prog.defines, &global_defines)? as u8;
            prog.origin = Some(v & 0x1F);
            continue;
        }

        if line_no_cmt.eq_ignore_ascii_case(".wrap_target") {
            prog.wrap_target = Some(prog.inst_lines.len());
            continue;
        }

        if line_no_cmt.eq_ignore_ascii_case(".wrap") {
            prog.wrap = Some(prog.inst_lines.len().saturating_sub(1));
            continue;
        }

        if line_no_cmt.starts_with(".word ") {
            prog.inst_lines
                .push((lineno + 1, line_no_cmt.to_string()));
            continue;
        }

        // Check for `<label>:` prefix (possibly with `public <label>:`)
        let mut work = line_no_cmt;
        if let Some(colon_pos) = work.find(':') {
            // Make sure `:` is not `::` (bit-reverse operator in `mov`)!
            let is_double_colon = work.as_bytes().get(colon_pos + 1) == Some(&b':');
            if !is_double_colon {
                let lbl_raw = work[..colon_pos].trim();
                let lbl = lbl_raw
                    .strip_prefix("public ")
                    .unwrap_or(lbl_raw)
                    .trim()
                    .to_string();
                let pc_idx = prog.inst_lines.len();
                prog.labels.insert(lbl.clone(), pc_idx);
                if lbl_raw.starts_with("public ") {
                    prog.public_defines.insert(lbl, pc_idx as i64);
                }
                work = work[colon_pos + 1..].trim();
            }
        }

        if !work.is_empty() {
            if prog.inst_lines.len() >= 32 {
                return Err(format!(
                    "line {}: PIO program '{}' exceeds 32-instruction hardware limit",
                    lineno + 1,
                    prog.name
                ));
            }
            prog.inst_lines.push((lineno + 1, work.to_string()));
        }
    }

    if let Some(done) = cur.take() {
        programs.push(finalize_program(done, &global_defines)?);
    }

    Ok(programs)
}

fn strip_block_comments(src: &str) -> String {
    // Also strip `% c-sdk { ... %}` blocks cleanly while preserving line count
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'%' && (i == 0 || bytes[i - 1] == b'\n') {
            // Skip until `%}`
            while i + 1 < bytes.len() && !(bytes[i] == b'%' && bytes[i + 1] == b'}') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn trim_line_comment(line: &str) -> &str {
    let mut end = line.len();
    if let Some(p) = line.find(';') {
        end = end.min(p);
    }
    if let Some(p) = line.find("//") {
        end = end.min(p);
    }
    &line[..end]
}

fn finalize_program(
    p: ProgParts,
    global_defs: &HashMap<String, i64>,
) -> Result<AssembledPioProgram, String> {
    let n_inst = p.inst_lines.len();
    if n_inst == 0 {
        return Err(format!("PIO program '{}' contains 0 instructions", p.name));
    }
    let wrap_target = p.wrap_target.unwrap_or(0);
    let wrap = p.wrap.unwrap_or(n_inst - 1);

    let total_ss_field = p.sideset_bits + if p.sideset_opt { 1 } else { 0 };
    if total_ss_field > 5 {
        return Err(format!(
            "PIO program '{}': side_set bits ({}) + opt exceed 5-bit field",
            p.name, p.sideset_bits
        ));
    }
    let delay_bits = 5 - total_ss_field;
    let max_delay = (1u16 << delay_bits) - 1;

    let mut instructions = Vec::with_capacity(n_inst);
    let mut jmp_indices = Vec::new();

    for (idx, (lineno, text)) in p.inst_lines.iter().enumerate() {
        let (core_str, side_opt, delay_val) =
            extract_sideset_and_delay(text, &p.defines, global_defs, *lineno)?;
        if delay_val > max_delay {
            return Err(format!(
                "line {}: delay [{}] exceeds max {} (side_set uses {} of 5 bits)",
                lineno, delay_val, max_delay, total_ss_field
            ));
        }
        if !p.sideset_opt && p.sideset_bits > 0 && side_opt.is_none() {
            return Err(format!(
                "line {}: non-optional .side_set {} requires `side <val>` on every instruction",
                lineno, p.sideset_bits
            ));
        }

        let ss_delay_5bit: u16 = if p.sideset_opt {
            if let Some(sv) = side_opt {
                let mask = (1u16 << p.sideset_bits) - 1;
                (1u16 << 4) | (((sv & mask) as u16) << delay_bits) | (delay_val & max_delay)
            } else {
                delay_val & max_delay
            }
        } else if p.sideset_bits > 0 {
            let sv = side_opt.unwrap_or(0);
            let mask = (1u16 << p.sideset_bits) - 1;
            (((sv & mask) as u16) << delay_bits) | (delay_val & max_delay)
        } else {
            delay_val & max_delay
        };

        let (base_word, is_jmp) = encode_pio_core(
            &core_str,
            &p.labels,
            &p.defines,
            global_defs,
            *lineno,
        )?;
        if is_jmp {
            jmp_indices.push(idx);
        }
        let word = if core_str.trim_start().starts_with(".word ") {
            base_word
        } else {
            (base_word & 0xE0FF) | ((ss_delay_5bit & 0x1F) << 8)
        };
        instructions.push(word);
    }

    Ok(AssembledPioProgram {
        name: p.name,
        instructions,
        jmp_indices,
        wrap_target,
        wrap,
        sideset_bits: p.sideset_bits,
        sideset_opt: p.sideset_opt,
        sideset_pindirs: p.sideset_pindirs,
        origin: p.origin,
        public_defines: p.public_defines,
    })
}

#[derive(Default)]
struct ProgParts {
    name: String,
    sideset_bits: u8,
    sideset_opt: bool,
    sideset_pindirs: bool,
    origin: Option<u8>,
    wrap_target: Option<usize>,
    wrap: Option<usize>,
    defines: HashMap<String, i64>,
    public_defines: HashMap<String, i64>,
    labels: HashMap<String, usize>,
    inst_lines: Vec<(usize, String)>,
}

// Implement for the local RawProg in assemble_pio_source via helper conversion:
fn extract_sideset_and_delay(
    line: &str,
    defs: &HashMap<String, i64>,
    gdefs: &HashMap<String, i64>,
    lineno: usize,
) -> Result<(String, Option<u16>, u16), String> {
    let mut work = line.trim().to_string();
    let mut delay_val: u16 = 0;

    // Extract `[delay]` anywhere at the end (or before `side`)
    while let (Some(lb), Some(rb)) = (work.rfind('['), work.rfind(']')) {
        if lb < rb {
            let inside = work[lb + 1..rb].trim();
            let v = eval_pio_expr(inside, defs, gdefs)
                .map_err(|e| format!("line {}: invalid delay '{}': {}", lineno, inside, e))?;
            delay_val = v as u16;
            work = format!("{} {}", &work[..lb], &work[rb + 1..])
                .trim()
                .to_string();
        } else {
            break;
        }
    }

    // Extract `side <val>` or `sideset <val>`
    let mut side_opt: Option<u16> = None;
    let lower = work.to_ascii_lowercase();
    for kw in [" side ", "\tside ", " sideset "] {
        if let Some(pos) = lower.rfind(kw) {
            let after = work[pos + kw.len()..].trim();
            // Take first token/parenthesized expr
            let val_str = after;
            let sv = eval_pio_expr(val_str, defs, gdefs)
                .map_err(|e| format!("line {}: invalid side-set '{}': {}", lineno, val_str, e))?;
            side_opt = Some(sv as u16);
            work = work[..pos].trim().to_string();
            break;
        }
    }

    Ok((work, side_opt, delay_val))
}

fn encode_pio_core(
    core: &str,
    labels: &HashMap<String, usize>,
    defs: &HashMap<String, i64>,
    gdefs: &HashMap<String, i64>,
    lineno: usize,
) -> Result<(u16, bool), String> {
    let trimmed = core.trim();
    if let Some(rest) = trimmed.strip_prefix(".word ") {
        let v = eval_pio_expr(rest.trim(), defs, gdefs)?;
        return Ok((v as u16, false));
    }

    let mut split = trimmed.splitn(2, |c: char| c.is_whitespace());
    let mnem = split
        .next()
        .ok_or_else(|| format!("line {}: empty PIO instruction", lineno))?
        .to_ascii_lowercase();
    let rest = split.next().unwrap_or("").trim();

    match mnem.as_str() {
        "nop" => {
            // `mov y, y` = opcode 101 (5), dest=010 (y), op=00, src=010 (y) => 0xA042
            Ok((0xA042, false))
        }
        "jmp" => {
            // `jmp [cond,] target`
            let cleaned = rest.replace(',', " ");
            let toks: Vec<&str> = cleaned.split_whitespace().collect();
            let (cond_bits, target_tok): (u16, &str) = if toks.len() == 1 {
                (0, toks[0])
            } else if toks.len() >= 2 {
                let c_raw: String = toks[..toks.len() - 1].join("");
                let c_low = c_raw.to_ascii_lowercase();
                let c = match c_low.as_str() {
                    "!x" => 1,
                    "x--" => 2,
                    "!y" => 3,
                    "y--" => 4,
                    "x!=y" => 5,
                    "pin" => 6,
                    "!osre" => 7,
                    _ => {
                        return Err(format!(
                            "line {}: invalid jmp condition '{}'",
                            lineno, c_raw
                        ))
                    }
                };
                (c, toks[toks.len() - 1])
            } else {
                return Err(format!("line {}: jmp missing target", lineno));
            };

            let addr = if let Some(&pc) = labels.get(target_tok) {
                pc as u16
            } else {
                eval_pio_expr(target_tok, defs, gdefs)? as u16
            };
            Ok(((cond_bits << 5) | (addr & 0x1F), true))
        }
        "wait" => {
            let cleaned = rest.replace(',', " ");
            let toks: Vec<&str> = cleaned.split_whitespace().collect();
            if toks.len() < 3 {
                return Err(format!("line {}: malformed wait '{}'", lineno, rest));
            }
            let pol = (eval_pio_expr(toks[0], defs, gdefs)? as u16) & 1;
            let src = match toks[1].to_ascii_lowercase().as_str() {
                "gpio" => 0u16,
                "pin" => 1u16,
                "irq" => 2u16,
                other => return Err(format!("line {}: unknown wait source '{}'", lineno, other)),
            };
            let is_rel = toks
                .iter()
                .skip(3)
                .any(|t| t.eq_ignore_ascii_case("rel"));
            let mut idx = (eval_pio_expr(toks[2], defs, gdefs)? as u16) & 0x1F;
            if src == 2 && is_rel {
                idx = (idx & 0x07) | 0x10;
            }
            Ok(((1u16 << 13) | (pol << 7) | (src << 5) | idx, false))
        }
        "in" => {
            let (src_s, cnt_s) = split_two_operands(rest, lineno, "in")?;
            let src = match src_s.to_ascii_lowercase().as_str() {
                "pins" => 0u16,
                "x" => 1u16,
                "y" => 2u16,
                "null" => 3u16,
                "isr" => 6u16,
                "osr" => 7u16,
                other => return Err(format!("line {}: invalid `in` source '{}'", lineno, other)),
            };
            let cnt = eval_pio_expr(cnt_s, defs, gdefs)?;
            if !(1..=32).contains(&cnt) {
                return Err(format!("line {}: `in` bit count must be 1..=32", lineno));
            }
            let cnt_enc = (cnt as u16) & 0x1F; // 32 -> 0
            Ok(((2u16 << 13) | (src << 5) | cnt_enc, false))
        }
        "out" => {
            let (dst_s, cnt_s) = split_two_operands(rest, lineno, "out")?;
            let dst = match dst_s.to_ascii_lowercase().as_str() {
                "pins" => 0u16,
                "x" => 1u16,
                "y" => 2u16,
                "null" => 3u16,
                "pindirs" => 4u16,
                "pc" => 5u16,
                "isr" => 6u16,
                "exec" => 7u16,
                other => return Err(format!("line {}: invalid `out` dest '{}'", lineno, other)),
            };
            let cnt = eval_pio_expr(cnt_s, defs, gdefs)?;
            if !(1..=32).contains(&cnt) {
                return Err(format!("line {}: `out` bit count must be 1..=32", lineno));
            }
            let cnt_enc = (cnt as u16) & 0x1F;
            Ok(((3u16 << 13) | (dst << 5) | cnt_enc, false))
        }
        "push" | "pull" => {
            let is_pull = if mnem == "pull" { 1u16 } else { 0u16 };
            let mut if_flag = 0u16;
            let mut blk = 1u16;
            for tok in rest.replace(',', " ").split_whitespace() {
                match tok.to_ascii_lowercase().as_str() {
                    "iffull" | "ifempty" => if_flag = 1,
                    "noblock" => blk = 0,
                    "block" => blk = 1,
                    _ => {}
                }
            }
            Ok((
                (4u16 << 13) | (is_pull << 7) | (if_flag << 6) | (blk << 5),
                false,
            ))
        }
        "mov" => {
            let (dst_s, raw_src) = split_two_operands(rest, lineno, "mov")?;
            let dst = match dst_s.to_ascii_lowercase().as_str() {
                "pins" => 0u16,
                "x" => 1u16,
                "y" => 2u16,
                "exec" => 4u16,
                "pc" => 5u16,
                "isr" => 6u16,
                "osr" => 7u16,
                other => return Err(format!("line {}: invalid `mov` dest '{}'", lineno, other)),
            };
            let rs = raw_src.trim();
            let (op_bits, src_clean) = if let Some(s) = rs.strip_prefix('~').or_else(|| rs.strip_prefix('!')) {
                (1u16, s.trim())
            } else if let Some(s) = rs.strip_prefix("::") {
                (2u16, s.trim())
            } else {
                (0u16, rs)
            };
            let src = match src_clean.to_ascii_lowercase().as_str() {
                "pins" => 0u16,
                "x" => 1u16,
                "y" => 2u16,
                "null" => 3u16,
                "status" => 5u16,
                "isr" => 6u16,
                "osr" => 7u16,
                other => return Err(format!("line {}: invalid `mov` source '{}'", lineno, other)),
            };
            Ok(((5u16 << 13) | (dst << 5) | (op_bits << 3) | src, false))
        }
        "irq" => {
            let cleaned = rest.replace(',', " ");
            let toks: Vec<&str> = cleaned.split_whitespace().collect();
            if toks.is_empty() {
                return Err(format!("line {}: irq missing index", lineno));
            }
            let mut clr = 0u16;
            let mut wait = 0u16;
            let mut is_rel = false;
            let mut idx_tok: Option<&str> = None;
            for t in toks {
                match t.to_ascii_lowercase().as_str() {
                    "clear" => clr = 1,
                    "wait" => wait = 1,
                    "nowait" | "set" => {}
                    "rel" => is_rel = true,
                    _ => idx_tok = Some(t),
                }
            }
            let raw_idx = eval_pio_expr(
                idx_tok.ok_or_else(|| format!("line {}: irq missing index", lineno))?,
                defs,
                gdefs,
            )? as u16;
            let idx = if is_rel {
                (raw_idx & 0x07) | 0x10
            } else {
                raw_idx & 0x1F
            };
            Ok(((6u16 << 13) | (clr << 6) | (wait << 5) | idx, false))
        }
        "set" => {
            let (dst_s, val_s) = split_two_operands(rest, lineno, "set")?;
            let dst = match dst_s.to_ascii_lowercase().as_str() {
                "pins" => 0u16,
                "x" => 1u16,
                "y" => 2u16,
                "pindirs" => 4u16,
                other => return Err(format!("line {}: invalid `set` dest '{}'", lineno, other)),
            };
            let val = eval_pio_expr(val_s, defs, gdefs)?;
            if !(0..=31).contains(&val) {
                return Err(format!(
                    "line {}: `set` immediate {} out of 0..=31 range",
                    lineno, val
                ));
            }
            Ok(((7u16 << 13) | (dst << 5) | (val as u16 & 0x1F), false))
        }
        other => Err(format!("line {}: unknown PIO mnemonic '{}'", lineno, other)),
    }
}

fn split_two_operands<'a>(rest: &'a str, lineno: usize, mnem: &str) -> Result<(&'a str, &'a str), String> {
    if let Some((a, b)) = rest.split_once(',') {
        Ok((a.trim(), b.trim()))
    } else {
        let mut sp = rest.splitn(2, char::is_whitespace);
        let a = sp
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("line {}: `{}` missing operands", lineno, mnem))?;
        let b = sp
            .next()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("line {}: `{}` missing second operand", lineno, mnem))?;
        Ok((a.trim(), b))
    }
}

fn eval_pio_expr(
    expr: &str,
    local_defs: &HashMap<String, i64>,
    global_defs: &HashMap<String, i64>,
) -> Result<i64, String> {
    let s = expr.trim();
    if s.is_empty() {
        return Err("empty expression".to_string());
    }
    // Strip outer parens
    if s.starts_with('(') && s.ends_with(')') {
        let inner = &s[1..s.len() - 1];
        let mut depth = 0i32;
        let mut balanced = true;
        for ch in inner.chars() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth < 0 {
                    balanced = false;
                    break;
                }
            }
        }
        if balanced && depth == 0 {
            return eval_pio_expr(inner, local_defs, global_defs);
        }
    }

    // Binary `+` / `-` outside parens (right-to-left for left-associative evaluation)
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for i in (1..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'+' | b'-' if depth == 0 => {
                let lhs = eval_pio_expr(&s[..i], local_defs, global_defs)?;
                let rhs = eval_pio_expr(&s[i + 1..], local_defs, global_defs)?;
                return Ok(if bytes[i] == b'+' { lhs + rhs } else { lhs - rhs });
            }
            _ => {}
        }
    }
    depth = 0;
    for i in (1..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'*' | b'/' if depth == 0 => {
                let lhs = eval_pio_expr(&s[..i], local_defs, global_defs)?;
                let rhs = eval_pio_expr(&s[i + 1..], local_defs, global_defs)?;
                return if bytes[i] == b'*' {
                    Ok(lhs * rhs)
                } else if rhs != 0 {
                    Ok(lhs / rhs)
                } else {
                    Err("division by zero in PIO expression".to_string())
                };
            }
            _ => {}
        }
    }

    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(&hex.replace('_', ""), 16).map_err(|e| e.to_string());
    }
    if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return i64::from_str_radix(&bin.replace('_', ""), 2).map_err(|e| e.to_string());
    }
    if let Ok(v) = s.parse::<i64>() {
        return Ok(v);
    }
    if let Some(&v) = local_defs.get(s).or_else(|| global_defs.get(s)) {
        return Ok(v);
    }
    Err(format!("unknown PIO symbol '{}'", s))
}

/// Synthesize OmniScript source functions for assembled `.pio` state-machine programs
/// so `import "ws2812.pio" as ws` exposes zero-dependency loader & SM config functions!
pub fn synthesize_omni_pio_module(
    programs: &[AssembledPioProgram],
    alias: Option<&str>,
) -> String {
    let mut out = String::new();
    for (p_idx, prog) in programs.iter().enumerate() {
        let mut prefixes = vec![prog.name.clone()];
        if let Some(al) = alias {
            if p_idx == 0 && !al.is_empty() && al != prog.name {
                prefixes.push(al.to_string());
            }
        }
        let cfg_pack: i64 = ((prog.wrap_target as i64) & 0x1F)
            | (((prog.wrap as i64) & 0x1F) << 5)
            | (((prog.sideset_bits as i64) & 0x7) << 10)
            | ((if prog.sideset_opt { 1i64 } else { 0i64 }) << 13)
            | ((if prog.sideset_pindirs { 1i64 } else { 0i64 }) << 14);

        for pref in prefixes {
            let _ = writeln!(out, "fn {}_program_len() -> int {{", pref);
            let _ = writeln!(out, "    return {};", prog.instructions.len());
            out.push_str("}\n");

            let _ = writeln!(out, "fn {}_wrap_target() -> int {{", pref);
            let _ = writeln!(out, "    return {};", prog.wrap_target);
            out.push_str("}\n");

            let _ = writeln!(out, "fn {}_wrap() -> int {{", pref);
            let _ = writeln!(out, "    return {};", prog.wrap);
            out.push_str("}\n");

            let _ = writeln!(
                out,
                "fn {}_program_load(pio_idx: int, base_offset: int) -> int {{",
                pref
            );
            for (i, &w) in prog.instructions.iter().enumerate() {
                if prog.jmp_indices.contains(&i) {
                    let _ = writeln!(
                        out,
                        "    pio_instr_write(pio_idx, base_offset + {}, {} + base_offset);",
                        i, w
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "    pio_instr_write(pio_idx, base_offset + {}, {});",
                        i, w
                    );
                }
            }
            out.push_str("    return base_offset;\n}\n");

            let _ = writeln!(
                out,
                "fn {}_sm_init(pio_idx: int, sm: int, base_offset: int, pin: int, clkdiv_int: int) -> int {{",
                pref
            );
            let _ = writeln!(
                out,
                "    return pio_sm_config_and_enable(pio_idx, sm, base_offset, {}, pin, clkdiv_int);",
                cfg_pack
            );
            out.push_str("}\n");

            for (def_k, def_v) in &prog.public_defines {
                let _ = writeln!(out, "fn {}_{}() -> int {{", pref, def_k);
                let _ = writeln!(out, "    return {};", def_v);
                out.push_str("}\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pio_assembler_ws2812_and_blink_encodings() {
        // Canonical WS2812 4-instruction program from Raspberry Pi Pico datasheet!
        let ws2812_src = r#"
.program ws2812
.side_set 1
.define public T1 2
.define public T2 5
.define public T3 3

.wrap_target
bitloop:
    out x, 1       side 0 [T3 - 1] ; 0x6221
    jmp !x do_zero side 1 [T1 - 1] ; 0x1123
do_one:
    jmp bitloop    side 1 [T2 - 1] ; 0x1400
do_zero:
    nop            side 0 [T2 - 1] ; 0xA442
.wrap
"#;
        let progs = assemble_pio_source(ws2812_src).unwrap();
        assert_eq!(progs.len(), 1);
        let p = &progs[0];
        assert_eq!(p.name, "ws2812");
        assert_eq!(p.wrap_target, 0);
        assert_eq!(p.wrap, 3);
        assert_eq!(p.sideset_bits, 1);
        // Exact 16-bit machine words matching `pioasm` datasheet output!
        assert_eq!(p.instructions, vec![0x6221, 0x1123, 0x1400, 0xA442]);
    }
}
