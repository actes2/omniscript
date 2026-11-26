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

    let _ = std::fs::remove_dir_all(&tmp);
}
