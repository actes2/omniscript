use std::fs;
use std::process::Command;

#[test]
fn test_pure_rust_c99_frontend_executes_live_in_cranelift_jit_zero_clang() {
    let c_src = r#"
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#define ADD_SCALED(a, b) ((a) * 10 + (b))
#define MODE 2

#if MODE == 2 && defined(ADD_SCALED)
#define BONUS 7
#else
#define BONUS 999
#endif

typedef struct Vec2 {
    int32_t x;
    int32_t y;
} Vec2;

typedef union Bits32 {
    int32_t i;
    uint32_t u;
} Bits32;

enum Kind { K_A = 1, K_B = 5, K_C };

static int g_bias = 3;

static int apply_op(int (*fn)(int, int), int a, int b) {
    return fn(a, b);
}

static int mul_pair(int a, int b) {
    return a * b;
}

int c99_suite_score(int n) {
    // 1. Preprocessor + enum + global: 27 + 6 + 3 = 36
    int acc = ADD_SCALED(2, BONUS) + K_C + g_bias;

    // 2. Structs, pointers, `->`, `&`, sizeof: 36 + 6*9 + 8 = 98
    Vec2 v = { .x = 4, .y = 9 };
    Vec2 *vp = &v;
    vp->x += 2;
    acc += vp->x * vp->y + (int)sizeof(Vec2);

    // 3. Union punning: 98 + 16 = 114
    Bits32 bu;
    bu.u = 0x10;
    acc += bu.i;

    // 4. Local array + pointer arithmetic + for loop: 114 + 10 = 124
    int arr[4] = { 1, 2, 3, 4 };
    int *p = arr;
    for (int i = 0; i < n && i < 4; i++) {
        acc += *(p + i);
    }

    // 5. Switch with C fallthrough: 124 + 11 = 135
    int sw = 0;
    switch (n) {
        case 4:
            sw += 5;
        case 3:
            sw += 6;
            break;
        default:
            sw = -1;
            break;
    }
    acc += sw;

    // 6. Function pointer + ternary + static local: 135 + 15 + calls (1 then 2)
    static int calls = 0;
    calls++;
    int fp = apply_op(&mul_pair, 3, 5);
    acc += (fp > 10 ? fp : 0) + calls;

    // 7. Double arithmetic + cast + mixed-type ternary: +10 + 2 = +12
    double d = 2.5 * 4.0;
    double bonus_d = (n == 4) ? 2 : 9.5;
    acc += (int)(d + bonus_d);

    return acc;
}

int main(void) {
    int r1 = c99_suite_score(4); // 163
    int r2 = c99_suite_score(4); // 164 (static local incremented!)
    return r1 + r2;              // 327
}
"#;

    let ll = omni_preamble::c_lifter::c_frontend::compile_c_source_str_to_ll(
        c_src,
        None,
        &[],
        &omni_runtime::host_target_triple(),
    )
    .expect("Pure-Rust C99 frontend should compile C99 source to LLVM IR without clang");

    let exit_code = omni_verify::cranelift_backend::jit_execute_ir(&ll, &[])
        .unwrap_or_else(|e| panic!("Cranelift JIT failed on C99 IR:\n{}\n\nIR:\n{}", e, ll));
    assert_eq!(exit_code, 327, "Expected 163 + 164 = 327 from zero-clang C99 JIT");
}

#[test]
fn test_omnic_run_c_source_import_with_empty_path_zero_clang() {
    let dir = std::env::temp_dir().join(format!("omni_zero_clang_c_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    fs::write(
        dir.join("fast_c.h"),
        "#pragma once\nstatic inline int c_inline_mul(int a, int b) { return a * b + 1; }\nint c_sum_squares(int n);\n",
    )
    .unwrap();
    fs::write(
        dir.join("fast_c.c"),
        "#include \"fast_c.h\"\nint c_sum_squares(int n) { int s = 0; for (int i = 1; i <= n; i++) s += i * i; return s; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.omni"),
        r#"import "./fast_c.c" as fast_c

def main():
    a = fast_c.c_inline_mul(6, 7)
    b = fast_c.c_sum_squares(5)
    print("zero_clang_c:", a + b)
    return 0
"#,
    )
    .unwrap();

    let omnic_bin = env!("CARGO_BIN_EXE_omnic");
    let out = Command::new(omnic_bin)
        .arg("run")
        .arg("--no-cache")
        .arg(dir.join("main.omni"))
        .env("PATH", "/nonexistent")
        .current_dir(&dir)
        .output()
        .expect("Failed to execute omnic run with PATH=/nonexistent");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::remove_dir_all(&dir);

    assert!(
        out.status.success(),
        "omnic run failed with PATH=/nonexistent (zero clang)!\nStdout:\n{}\nStderr:\n{}",
        stdout,
        stderr
    );
    // 6*7+1 = 43; 1+4+9+16+25 = 55; 43+55 = 98
    assert!(
        stdout.contains("zero_clang_c: 98"),
        "Expected 'zero_clang_c: 98' in stdout:\n{}\nStderr:\n{}",
        stdout,
        stderr
    );
}
