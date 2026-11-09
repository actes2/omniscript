use std::path::PathBuf;
use std::process::Command;

fn run_matrix_test(sub_dir: &str, expected_substr: &str) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let target_file = repo_root
        .join("tests")
        .join("polyglot_matrix")
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
        "omnic run failed for suite {}\nExit code: {:?}\nOutput:\n{}",
        sub_dir,
        output.status.code(),
        combined
    );

    assert!(
        combined.contains(expected_substr),
        "Suite {} expected substring '{}' not found in output:\n{}",
        sub_dir,
        expected_substr,
        combined
    );
}

#[test]
fn test_matrix_01_python() {
    run_matrix_test("01_python", "Calculated Python metric: 17");
}

#[test]
fn test_matrix_02_javascript() {
    run_matrix_test("02_javascript", "Evaluated JS risk: 90");
}

#[test]
fn test_matrix_03_rust() {
    run_matrix_test("03_rust", "Rust checksum: 23138");
}

#[test]
fn test_matrix_04_go() {
    run_matrix_test("04_go", "Go event id: 90");
}

#[test]
fn test_matrix_05_c() {
    run_matrix_test("05_c", "C scaled sum: 80");
}

#[test]
fn test_matrix_06_java() {
    run_matrix_test("06_java", "Java mean: 42");
}

#[test]
fn test_matrix_07_csharp() {
    run_matrix_test("07_csharp", "C# Margin: 125");
}

#[test]
fn test_matrix_08_lua() {
    run_matrix_test("08_lua", "Lua Escape Margin: 500");
}

#[test]
fn test_matrix_09_polyglot_mesh() {
    run_matrix_test("09_polyglot_mesh", "Polyglot Mesh Final Result: 525");
}

#[test]
fn test_python_classes_and_context_managers() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let target_file = repo_root.join("examples").join("python_classes.omni");

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

    assert!(output.status.success(), "Failed: {}", combined);
    assert!(combined.contains("Counter after increment: 20"));
    assert!(combined.contains("Direct field read c.step: 2"));
    assert!(combined.contains("Entering ScopedResource context"));
    assert!(combined.contains("Computed inside context: 142"));
    assert!(combined.contains("Exiting ScopedResource context (RAII cleanup executed)"));
    assert!(combined.contains("Post context manager execution completed!"));
}
