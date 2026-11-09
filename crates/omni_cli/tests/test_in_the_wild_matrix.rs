use std::path::PathBuf;
use std::process::Command;

fn run_in_the_wild_test(sub_dir: &str, expected_substr: &str) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let target_file = repo_root
        .join("tests")
        .join("in_the_wild_matrix")
        .join(sub_dir)
        .join("main.omni");
    assert!(
        target_file.exists(),
        "Target file does not exist: {}",
        target_file.display()
    );

    let omnic_bin = env!("CARGO_BIN_EXE_omnic");
    let output = Command::new(omnic_bin)
        .arg("--no-cache")
        .arg("run")
        .arg(&target_file)
        .current_dir(repo_root)
        .output()
        .expect("Failed to execute omnic");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    assert!(
        output.status.success(),
        "omnic run failed for in-the-wild suite {}\nExit code: {:?}\nOutput:\n{}",
        sub_dir,
        output.status.code(),
        combined
    );

    assert!(
        combined.contains(expected_substr),
        "In-the-wild suite {} expected substring '{}' not found in output:\n{}",
        sub_dir,
        expected_substr,
        combined
    );
}

#[test]
fn test_in_the_wild_01_c_system_so() {
    run_in_the_wild_test(
        "01_c_system_so",
        "Calculated zlib compressBound for 1000 bytes: 1013",
    );
}

#[test]
fn test_in_the_wild_02_jvm_bytecode_so() {
    run_in_the_wild_test(
        "02_jvm_bytecode_so",
        "BytecodeService.compute_ratio(50, 5): 100",
    );
}

#[test]
fn test_in_the_wild_03_python_site_packages() {
    run_in_the_wild_test(
        "03_python_site_packages",
        "Transformed metric: 1345",
    );
}

#[test]
fn test_in_the_wild_04_node_package_json() {
    run_in_the_wild_test(
        "04_node_package_json",
        "Verified token: 61",
    );
}
