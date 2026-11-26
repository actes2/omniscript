use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_self_hosting_stage1_stage2_stage3_fixpoint() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let stage1_src = repo_root.join("bootstrap/omnic_stage1.omni");
    assert!(stage1_src.exists(), "Missing {}", stage1_src.display());

    let omnic_bin = PathBuf::from(env!("CARGO_BIN_EXE_omnic"));
    let tmp = std::env::temp_dir().join(format!("omni_bootstrap_fixpoint_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // 1. Static determinism check (zero LLM sites, zero errors)
    let chk = Command::new(&omnic_bin)
        .args(["check", stage1_src.to_str().unwrap()])
        .output()
        .expect("omnic check failed");
    let chk_out = String::from_utf8_lossy(&chk.stdout);
    assert!(
        chk.status.success() && chk_out.contains("zero LLM-inferred sites"),
        "Stage-1 compiler must be 100% deterministic:\nstdout: {}\nstderr: {}",
        chk_out,
        String::from_utf8_lossy(&chk.stderr)
    );

    // 2. Stage-0 (Rust omnic) -> Stage-1 native compiler binary
    let stage1_bin = tmp.join("omnic_stage1");
    let b0 = Command::new(&omnic_bin)
        .args([
            "build",
            "--no-cache",
            stage1_src.to_str().unwrap(),
            "-o",
            stage1_bin.to_str().unwrap(),
        ])
        .output()
        .expect("Stage-0 build failed");
    assert!(
        b0.status.success(),
        "Stage-0 failed to build Stage-1:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&b0.stdout),
        String::from_utf8_lossy(&b0.stderr)
    );

    // 3. Stage-1 compiles its own source -> Stage-2 (.obc)
    let stage2_obc = tmp.join("omnic_stage2.obc");
    let b1 = Command::new(&stage1_bin)
        .args([stage1_src.to_str().unwrap(), stage2_obc.to_str().unwrap()])
        .output()
        .expect("Stage-1 self-compile failed");
    assert!(
        b1.status.success(),
        "Stage-1 failed to compile Stage-2:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&b1.stdout),
        String::from_utf8_lossy(&b1.stderr)
    );

    // 4. Stage-2 (running in OmniBcVm) compiles its own source -> Stage-3 (.obc)
    let stage3_obc = tmp.join("omnic_stage3.obc");
    let b2 = Command::new(&omnic_bin)
        .args([
            "run",
            stage2_obc.to_str().unwrap(),
            stage1_src.to_str().unwrap(),
            stage3_obc.to_str().unwrap(),
        ])
        .output()
        .expect("Stage-2 self-compile failed");
    assert!(
        b2.status.success(),
        "Stage-2 failed to compile Stage-3:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&b2.stdout),
        String::from_utf8_lossy(&b2.stderr)
    );

    // 5. Fixpoint assertion: Stage-2.obc === Stage-3.obc (bit-for-bit identical!)
    let bytes2 = std::fs::read(&stage2_obc).expect("read stage2.obc");
    let bytes3 = std::fs::read(&stage3_obc).expect("read stage3.obc");
    assert!(bytes2.starts_with(b"OBC1"), "Stage-2 output must be OBC1 bytecode");
    assert_eq!(
        bytes2, bytes3,
        "Self-hosting fixpoint violated: Stage-2 ({} B) != Stage-3 ({} B)",
        bytes2.len(),
        bytes3.len()
    );

    // 6. Stage-3 compiles and executes a user program exercising classes, recursion, &&/||, print
    let user_src = tmp.join("user_prog.omni");
    let user_obc = tmp.join("user_prog.obc");
    std::fs::write(
        &user_src,
        r#"
class Counter {
    val: int
    fn init(self, start: int) -> int {
        self.val = start
        return 0
    }
    fn add(self, n: int) -> int {
        self.val = self.val + n
        return self.val
    }
}
fn fib(n: int) -> int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}
fn main() -> int {
    let c = Counter()
    c.init(10)
    let r = c.add(fib(10))
    if r == 65 && (r > 60 || r < 0) {
        print("Stage-3 OK")
        print(r)
    }
    return r
}
"#,
    )
    .unwrap();

    let b3 = Command::new(&omnic_bin)
        .args([
            "run",
            stage3_obc.to_str().unwrap(),
            user_src.to_str().unwrap(),
            user_obc.to_str().unwrap(),
        ])
        .output()
        .expect("Stage-3 user compile failed");
    assert!(b3.status.success(), "Stage-3 failed to compile user_prog.omni");

    let run_u = Command::new(&omnic_bin)
        .args(["run", user_obc.to_str().unwrap()])
        .output()
        .expect("Running Stage-3 user_prog.obc failed");
    assert_eq!(run_u.status.code(), Some(65));
    let u_out = String::from_utf8_lossy(&run_u.stdout);
    assert!(u_out.contains("Stage-3 OK") && u_out.contains("65"), "Unexpected stdout: {}", u_out);

    // 7. Stage-3 compiles a Rust (.rs) program (pub struct, impl, &mut self, for..in, else if, +=, println!)
    let rust_src = tmp.join("rust_demo.rs");
    let rust_obc = tmp.join("rust_demo.obc");
    std::fs::write(
        &rust_src,
        r#"
pub struct Accumulator {
    pub total: i64,
    pub steps: i64,
}
impl Accumulator {
    pub fn reset(&mut self, base: i64) -> i64 {
        self.total = base;
        self.steps = 0;
        return 0;
    }
    pub fn add(&mut self, delta: i64) -> i64 {
        self.total += delta;
        self.steps += 1;
        return self.total;
    }
}
pub fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    } else if n == 2 {
        return 1;
    } else {
        return fib(n - 1) + fib(n - 2);
    }
}
pub fn main() -> i64 {
    let mut acc = Accumulator::new();
    acc.reset(0);
    for i in 1..11 {
        acc.add(i);
    }
    let f = fib(10);
    if !false && true {
        println!("Rust Stage-3 OK");
        println!(acc.total);
    }
    return acc.total + f;
}
"#,
    )
    .unwrap();

    let br = Command::new(&omnic_bin)
        .args([
            "run",
            stage3_obc.to_str().unwrap(),
            rust_src.to_str().unwrap(),
            rust_obc.to_str().unwrap(),
        ])
        .output()
        .expect("Stage-3 Rust compile failed");
    assert!(br.status.success(), "Stage-3 failed to compile rust_demo.rs");
    let run_r = Command::new(&omnic_bin)
        .args(["run", rust_obc.to_str().unwrap()])
        .output()
        .expect("Running rust_demo.obc failed");
    assert_eq!(run_r.status.code(), Some(110));
    let r_out = String::from_utf8_lossy(&run_r.stdout);
    assert!(r_out.contains("Rust Stage-3 OK") && r_out.contains("55"), "Unexpected Rust stdout: {}", r_out);

    // 8. Native Linux x86_64 ELF64 (.o) 3-stage fixpoint (Stage-2.o === Stage-3.o, zero Rust/Cranelift/Clang!)
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let stage2_o = tmp.join("omnic_stage2.o");
        let stage3_o = tmp.join("omnic_stage3.o");
        let stage2_bin = tmp.join("omnic_stage2_native");

        let e1 = Command::new(&stage1_bin)
            .args([stage1_src.to_str().unwrap(), stage2_o.to_str().unwrap()])
            .output()
            .expect("Stage-1 ELF64 emit failed");
        assert!(e1.status.success(), "Stage-1 failed to emit omnic_stage2.o");

        // Locate newest libomni_runtime_native.a in target/release/build or target/debug/build
        let mut rt_candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for profile in ["release", "debug"] {
            let build_dir = repo_root.join("target").join(profile).join("build");
            if let Ok(entries) = std::fs::read_dir(&build_dir) {
                for ent in entries.flatten() {
                    let cand = ent.path().join("out/libomni_runtime_native.a");
                    if let Ok(meta) = std::fs::metadata(&cand) {
                        if let Ok(mtime) = meta.modified() {
                            rt_candidates.push((mtime, cand));
                        }
                    }
                }
            }
        }
        rt_candidates.sort_by(|a, b| b.0.cmp(&a.0));
        let rt_lib = rt_candidates.into_iter().next().map(|(_, p)| p);
        if let Some(rt) = rt_lib {
            let lnk = Command::new("cc")
                .args([
                    stage2_o.to_str().unwrap(),
                    rt.to_str().unwrap(),
                    "-lm",
                    "-lpthread",
                    "-ldl",
                    "-o",
                    stage2_bin.to_str().unwrap(),
                ])
                .output()
                .expect("cc link of omnic_stage2.o failed");
            assert!(
                lnk.status.success(),
                "Linking Stage-2 native ELF failed:\n{}",
                String::from_utf8_lossy(&lnk.stderr)
            );

            let e2 = Command::new(&stage2_bin)
                .args([stage1_src.to_str().unwrap(), stage3_o.to_str().unwrap()])
                .output()
                .expect("Stage-2 native self-compile failed");
            assert!(e2.status.success(), "Stage-2 native failed to emit omnic_stage3.o");

            let o2 = std::fs::read(&stage2_o).expect("read stage2.o");
            let o3 = std::fs::read(&stage3_o).expect("read stage3.o");
            assert!(o2.starts_with(b"\x7fELF"), "Stage-2 .o must be valid ELF64");
            assert_eq!(
                o2, o3,
                "Native x86_64 ELF64 fixpoint violated: Stage-2.o ({} B) != Stage-3.o ({} B)",
                o2.len(),
                o3.len()
            );

            // 9. Stage-2 native binary compiles universal core primitives (enum/match, hex/bin, bitwise, arrays, break/continue, record literals)
            let univ_src = tmp.join("universal_core.omni");
            let univ_o = tmp.join("universal_core.o");
            let univ_bin = tmp.join("universal_core_bin");
            std::fs::write(
                &univ_src,
                r#"
#[derive(Debug, Clone, Copy)]
pub enum Chip {
    Rp2040,
    Rp2350,
}
pub struct Profile {
    pub kind: Chip,
    pub pll: i64,
    pub mask: i64,
}
impl Profile {
    pub const fn make_rp2350() -> Self {
        Self {
            kind: Chip::Rp2350,
            pll: (5 << 16) | (2 << 12),
            mask: (0x0160_3D20 & 0xFFFF) ^ 0b1010_0101,
        }
    }
}
fn main() -> int {
    let p = Profile::make_rp2350();
    let score = 0;
    match p.kind {
        Chip::Rp2040 => { score = 10; },
        Chip::Rp2350 => { score = 40; },
        _ => { score = 0; },
    }
    let arr = [10, 20, 30];
    arr[1] = 25;
    arr.push(5);
    let sum = 0;
    for i in 0..arr.len() {
        if i == 2 {
            continue;
        }
        sum += arr[i];
    }
    let bits = ((p.pll >> 12) & 0xFF) + (p.mask & 0x0F);
    let loops = 0;
    while loops < 100 {
        if loops == 7 {
            break;
        }
        loops += 1;
    }
    print(bits);
    return score + sum + loops;
}
"#,
            )
            .unwrap();
            let eu = Command::new(&stage2_bin)
                .args([univ_src.to_str().unwrap(), univ_o.to_str().unwrap()])
                .output()
                .expect("Stage-2 universal_core compile failed");
            assert!(eu.status.success(), "Stage-2 failed to compile universal_core.omni");
            let lu = Command::new("cc")
                .args([
                    univ_o.to_str().unwrap(),
                    rt.to_str().unwrap(),
                    "-lm",
                    "-lpthread",
                    "-ldl",
                    "-o",
                    univ_bin.to_str().unwrap(),
                ])
                .output()
                .expect("cc link of universal_core.o failed");
            assert!(lu.status.success(), "Linking universal_core.o failed");
            let ru = Command::new(&univ_bin)
                .output()
                .expect("Running universal_core_bin failed");
            assert_eq!(ru.status.code(), Some(87));
            let ru_out = String::from_utf8_lossy(&ru.stdout);
            assert!(ru_out.contains("87"), "Expected bitwise result 87, got: {}", ru_out);

            // 10. Multi-file diamond import resolution by Stage-2 native binary
            let mf_dir = tmp.join("multifile_proj");
            std::fs::create_dir_all(&mf_dir).unwrap();
            std::fs::write(
                mf_dir.join("math_helpers.omni"),
                "fn square(x: int) -> int {\n    return x * x\n}\n",
            )
            .unwrap();
            std::fs::write(
                mf_dir.join("vec_ops.omni"),
                "import \"math_helpers.omni\";\nfn sum_squares(arr: int) -> int {\n    let s = 0\n    for i in 0..arr.len() {\n        s += square(arr[i])\n    }\n    return s\n}\n",
            )
            .unwrap();
            let mf_app = mf_dir.join("app.omni");
            std::fs::write(
                &mf_app,
                "import \"math_helpers.omni\";\nimport \"vec_ops\";\nfn main() -> int {\n    let v = [2, 3, 4]\n    let res = sum_squares(v) + square(5)\n    print(res)\n    return res\n}\n",
            )
            .unwrap();
            let mf_o = mf_dir.join("app.o");
            let mf_bin = mf_dir.join("app_bin");
            let emf = Command::new(&stage2_bin)
                .args([mf_app.to_str().unwrap(), mf_o.to_str().unwrap()])
                .output()
                .expect("Stage-2 multi-file compile failed");
            assert!(emf.status.success(), "Stage-2 failed multi-file compile");
            let lmf = Command::new("cc")
                .args([
                    mf_o.to_str().unwrap(),
                    rt.to_str().unwrap(),
                    "-lm",
                    "-lpthread",
                    "-ldl",
                    "-o",
                    mf_bin.to_str().unwrap(),
                ])
                .output()
                .expect("cc link of multifile app.o failed");
            assert!(lmf.status.success(), "Linking multifile app.o failed");
            let rmf = Command::new(&mf_bin).output().expect("Running multifile app_bin failed");
            assert_eq!(rmf.status.code(), Some(54));

            // 11. Cross-target AArch64 ELF64 (.arm64.o) fixpoint: Stage-1 === Stage-2
            let arm2_o = tmp.join("omnic_stage2.arm64.o");
            let arm3_o = tmp.join("omnic_stage3.arm64.o");
            let ea1 = Command::new(&stage1_bin)
                .args([stage1_src.to_str().unwrap(), arm2_o.to_str().unwrap()])
                .output()
                .expect("Stage-1 AArch64 cross-emit failed");
            assert!(ea1.status.success(), "Stage-1 failed to emit omnic_stage2.arm64.o");
            let ea2 = Command::new(&stage2_bin)
                .args([stage1_src.to_str().unwrap(), arm3_o.to_str().unwrap()])
                .output()
                .expect("Stage-2 AArch64 cross-emit failed");
            assert!(ea2.status.success(), "Stage-2 failed to emit omnic_stage3.arm64.o");
            let a2 = std::fs::read(&arm2_o).expect("read arm64 stage2.o");
            let a3 = std::fs::read(&arm3_o).expect("read arm64 stage3.o");
            assert!(a2.starts_with(b"\x7fELF"), "AArch64 .o must be ELF64");
            // ELF64 e_machine at offset 18..20 must be EM_AARCH64 (183 = 0x00B7 LE)
            assert_eq!(&a2[18..20], &[0xB7, 0x00], "e_machine must be EM_AARCH64 (183)");
            assert_eq!(
                a2, a3,
                "Cross-target AArch64 ELF64 fixpoint violated: Stage-2 ({} B) != Stage-3 ({} B)",
                a2.len(),
                a3.len()
            );

            // 12. Standalone Windows x86_64 PE32+ (.exe) emitted by Stage-2 (zero MinGW/linker!)
            let win_src = tmp.join("win64_demo.omni");
            let win_exe = tmp.join("win64_demo.exe");
            std::fs::write(
                &win_src,
                "fn fib(n: int) -> int {\n    if n <= 1 {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nfn main() -> int {\n    print(\"Win64 PE32+ OK\")\n    let f = fib(10)\n    print(f)\n    return f\n}\n",
            )
            .unwrap();
            let ew = Command::new(&stage2_bin)
                .args([win_src.to_str().unwrap(), win_exe.to_str().unwrap()])
                .output()
                .expect("Stage-2 Win64 PE32+ emit failed");
            assert!(ew.status.success(), "Stage-2 failed to emit win64_demo.exe");
            let pe_bytes = std::fs::read(&win_exe).expect("read win64_demo.exe");
            assert!(pe_bytes.starts_with(b"MZ"), "Windows .exe must start with MZ header");
            assert_eq!(&pe_bytes[64..68], b"PE\0\0", "Windows .exe must have PE\\0\\0 signature");
            // If wine is installed and headless prefix is ready, execute the .exe live!
            if let Ok(home) = std::env::var("HOME") {
                let prefix = PathBuf::from(&home).join(".cache/omni_wine_prefix");
                if prefix.exists() {
                    if let Ok(rw) = Command::new("wine")
                        .arg(&win_exe)
                        .env_remove("DISPLAY")
                        .env_remove("WAYLAND_DISPLAY")
                        .env("WINEPREFIX", &prefix)
                        .env("WINEDLLOVERRIDES", "mscoree,mshtml=")
                        .env("WINEDEBUG", "-all")
                        .output()
                    {
                        assert_eq!(rw.status.code(), Some(55), "Win64 .exe under Wine must exit 55");
                        let wout = String::from_utf8_lossy(&rw.stdout);
                        assert!(wout.contains("Win64 PE32+ OK") && wout.contains("55"), "Unexpected Wine stdout: {}", wout);
                    }
                }
            }

            // 13. Bare-metal Cortex-M33 RP2350 Thumb-2 (.uf2) emitted by Stage-2 (zero arm-none-eabi-gcc!)
            let rp_src = tmp.join("rp2350_demo.omni");
            let rp_uf2 = tmp.join("rp2350_demo.uf2");
            std::fs::write(
                &rp_src,
                "fn fib(n: int) -> int {\n    if n <= 1 {\n        return n\n    }\n    return fib(n - 1) + fib(n - 2)\n}\nfn main() -> int {\n    return fib(10)\n}\n",
            )
            .unwrap();
            let erp = Command::new(&stage2_bin)
                .args([rp_src.to_str().unwrap(), rp_uf2.to_str().unwrap()])
                .output()
                .expect("Stage-2 RP2350 UF2 emit failed");
            assert!(erp.status.success(), "Stage-2 failed to emit rp2350_demo.uf2");
            let uf2_bytes = std::fs::read(&rp_uf2).expect("read rp2350_demo.uf2");
            assert!(uf2_bytes.len() >= 512 && uf2_bytes.len() % 512 == 0, "UF2 must be 512-byte blocks");
            assert_eq!(&uf2_bytes[0..4], &0x0A324655u32.to_le_bytes(), "UF2 magicStart0");
            assert_eq!(&uf2_bytes[28..32], &0xE48BFF59u32.to_le_bytes(), "UF2 RP2350 familyID");
            assert_eq!(&uf2_bytes[508..512], &0x0AB16F30u32.to_le_bytes(), "UF2 magicEnd");
            if let Ok(pt) = Command::new("picotool").args(["info", "-a", rp_uf2.to_str().unwrap()]).output() {
                if pt.status.success() {
                    let pout = String::from_utf8_lossy(&pt.stdout);
                    assert!(pout.contains("RP2350") && pout.contains("ARM Secure"), "Unexpected picotool output: {}", pout);
                }
            }

            // 14. Phase A: Idiomatic Python (no 'let'!), trailing # comment on ':', f64 + IEEE-754 NaN FCmp, string '+', maps
            let py_src = tmp.join("polyglot_py.py");
            let py_o = tmp.join("polyglot_py.o");
            let py_bin = tmp.join("polyglot_py_bin");
            std::fs::write(
                &py_src,
                "def compute(n):\n    if n < 5: # small\n        return n * 2\n    elif n == 10:\n        return 42\n    else:\n        return n + 1\n\ndef main():\n    msg = \"Hello \" + \"Polyglot!\"\n    print(msg)\n    pi2 = 3.14 * 2.0 + 0.5\n    print(pi2)\n    bonus = 0\n    if -2.0 < -1.0:\n        bonus = bonus + 1\n    nan = 0.0 / 0.0\n    if nan != nan:\n        bonus = bonus + 1\n    if nan == nan:\n        bonus = bonus + 100\n    if nan < 1.0:\n        bonus = bonus + 100\n    m = {\"a\": 10, \"b\": 20}\n    m[\"c\"] = compute(10)\n    return m[\"a\"] + m[\"b\"] + m[\"c\"] + bonus\n",
            )
            .unwrap();
            let epy = Command::new(&stage2_bin)
                .args([py_src.to_str().unwrap(), py_o.to_str().unwrap()])
                .output()
                .expect("Stage-2 Python compile failed");
            assert!(epy.status.success());
            let lpy = Command::new("cc")
                .args([py_o.to_str().unwrap(), rt.to_str().unwrap(), "-lm", "-lpthread", "-ldl", "-o", py_bin.to_str().unwrap()])
                .output()
                .expect("link polyglot_py.o");
            assert!(lpy.status.success(), "link polyglot_py.o failed:\n{}", String::from_utf8_lossy(&lpy.stderr));
            let rpy = Command::new(&py_bin).output().expect("run polyglot_py_bin");
            assert_eq!(rpy.status.code(), Some(74));
            let py_out = String::from_utf8_lossy(&rpy.stdout);
            assert!(py_out.contains("Hello Polyglot!") && py_out.contains("6.78"), "Unexpected Phase A stdout: {}", py_out);

            // 15. Phase B: Zero-cc standalone Linux x86_64 ET_EXEC via explicit --target (extensionless!), Mach-O, and fail-closed UF2
            let st_elf = tmp.join("standalone_noext");
            let e_elf = Command::new(&stage2_bin)
                .args(["--target", "x86_64-linux-exec", win_src.to_str().unwrap(), st_elf.to_str().unwrap()])
                .output()
                .expect("Stage-2 standalone ELF64 emit failed");
            assert!(e_elf.status.success());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&st_elf).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&st_elf, perms).unwrap();
            }
            let r_elf = Command::new(&st_elf).output().expect("execute zero-cc standalone_noext");
            assert_eq!(r_elf.status.code(), Some(55));
            let elf_out = String::from_utf8_lossy(&r_elf.stdout);
            assert!(elf_out.contains("Win64 PE32+ OK") && elf_out.contains("55"), "Unexpected standalone ELF stdout: {}", elf_out);

            // Fail-closed check: bare-metal UF2 rejects unsupported host extern call (`print`)
            let bad_uf2 = tmp.join("bad_host_extern.uf2");
            let e_bad = Command::new(&stage2_bin)
                .args([win_src.to_str().unwrap(), bad_uf2.to_str().unwrap()])
                .output()
                .expect("Stage-2 fail-closed UF2 test failed");
            assert!(!e_bad.status.success(), "Bare-metal UF2 must fail closed on unsupported host extern call");

            let st_macho = tmp.join("standalone.macho.o");
            let e_macho = Command::new(&stage2_bin)
                .args([win_src.to_str().unwrap(), st_macho.to_str().unwrap()])
                .output()
                .expect("Stage-2 Mach-O emit failed");
            assert!(e_macho.status.success());
            let mb = std::fs::read(&st_macho).expect("read macho.o");
            assert_eq!(&mb[0..4], &0xFEEDFACFu32.to_le_bytes(), "Mach-O must start with MH_MAGIC_64");
            assert_eq!(&mb[4..8], &0x0100000Cu32.to_le_bytes(), "Mach-O must be CPU_TYPE_ARM64");
            assert!(mb.len() > 600, "Mach-O .o must contain real lowered AArch64 code + relocs, got {} B", mb.len());
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
