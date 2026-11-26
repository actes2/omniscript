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
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
