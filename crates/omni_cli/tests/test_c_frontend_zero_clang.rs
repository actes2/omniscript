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

#[test]
fn test_c99_advanced_gaps_bitfields_vlas_compound_lits_anon_aggs_duffs_device_zero_clang() {
    let c_src = r#"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct Flags {
    unsigned a : 3;
    int b : 5;
    unsigned c : 8;
} Flags;

typedef struct Pair {
    int x;
    int y;
} Pair;

typedef struct Packet {
    int tag;
    union {
        struct {
            int lo;
            int hi;
        };
        uint64_t raw;
    };
} Packet;

typedef struct FlexBlob {
    int len;
    int data[];
} FlexBlob;

typedef struct BigBox {
    int a, b, c, d, e, f; // 24 bytes (>16B -> memcpy copy)
} BigBox;

static int op_add(int a, int b) { return a + b; }
static int op_sub(int a, int b) { return a - b; }

// Spiral declarator: function returning function pointer
static int (*pick_op(int which))(int, int) {
    return which ? &op_add : &op_sub;
}

// Global pointer-offset & function-pointer initializers
static int g_vals[4] = { 10, 20, 30, 40 };
static int *g_mid = g_vals + 2;
static int (*g_fn)(int, int) = &op_add;

static int duff_sum(int *src, int count) {
    int sum = 0;
    int n = (count + 3) / 4;
    switch (count % 4) {
        case 0: do { sum += *src++;
        case 3:      sum += *src++;
        case 2:      sum += *src++;
        case 1:      sum += *src++;
                } while (--n > 0);
    }
    return sum;
}

static int test_vla(int n) {
    int buf[n];
    for (int i = 0; i < n; i++) {
        buf[i] = (i + 1) * 3;
    }
    int s = 0;
    for (int i = 0; i < n; i++) {
        s += buf[i];
    }
    return s; // n=5 -> 3+6+9+12+15 = 45
}

int main(void) {
    int total = 0;

    // 1. Bitfields (unsigned + signed negative sign-ext + compound assign)
    Flags f;
    f.a = 3;
    f.b = -5;
    f.c = 12;
    f.a += 2; // 5
    int s1 = (int)f.a + f.b + (int)f.c;
    if (s1 != 12) return 1000 + s1;
    total += s1;

    // 2. Compound literals (struct + inferred-length array)
    int cl1 = (Pair){ .x = 11, .y = 9 }.x;        // 11
    int cl2 = (int[]){ 4, 8, 13 }[2];             // 13
    int s2 = cl1 + cl2;
    if (s2 != 24) return 2000 + s2;
    total += s2;

    // 3. VLA with RAII cleanup
    int s3 = test_vla(5);
    if (s3 != 45) return 3000 + s3;
    total += s3;

    // 4. Anonymous nested struct inside anonymous union
    Packet pkt;
    pkt.tag = 1;
    pkt.lo = 7;
    pkt.hi = 14;
    int s4 = pkt.lo + pkt.hi;
    if (s4 != 21) return 4000 + s4;
    total += s4;

    // 5. Flexible array member (`sizeof(FlexBlob) == 4`)
    int s5 = (int)sizeof(FlexBlob);
    if (s5 != 4) return 5000 + s5;
    total += s5;

    // 6. Spiral declarators: ptr-to-array, array-of-fn-ptrs, fn-returning-fn-ptr
    int mat[2][3] = { { 1, 2, 3 }, { 4, 5, 6 } };
    int (*row_ptr)[3] = mat;
    int (*ops[2])(int, int) = { &op_add, &op_sub };
    int (*chosen)(int, int) = pick_op(1);
    int r6a = (*row_ptr)[2];
    int r6b = ops[0](6, 4);
    int r6c = ops[1](9, 2);
    int r6d = chosen(5, 3);
    if (r6a != 3) return 6100 + r6a;
    if (r6b != 10) return 6200 + r6b;
    if (r6c != 7) return 6300 + r6c;
    if (r6d != 8) return 6400 + r6d;
    int s6 = r6a + r6b + r6c + r6d;
    if (s6 != 28) return 6000 + s6;
    total += s6;

    // 7. Global pointer offset & global fn pointer
    int s7 = *g_mid + g_fn(2, 4);
    if (s7 != 36) return 7000 + s7;
    total += s7;

    // 8. Duff's device (nested case inside do-while)
    int d_arr[5] = { 1, 2, 3, 4, 5 };
    int s8 = duff_sum(d_arr, 5);
    if (s8 != 15) return 8000 + s8;
    total += s8;

    // 9. Large struct (>16B) copy via memcpy
    BigBox b1 = { 1, 2, 3, 4, 5, 6 };
    BigBox b2;
    b2 = b1;
    int s9 = b2.a + b2.f;
    if (s9 != 7) return 9000 + s9;
    total += s9;

    return total;
}
"#;

    let ll = omni_preamble::c_lifter::c_frontend::compile_c_source_str_to_ll(
        c_src,
        None,
        &[],
        &omni_runtime::host_target_triple(),
    )
    .expect("Pure-Rust C99 frontend should compile all advanced C99 features without clang");

    let syms = omni_runtime::jit_runtime_symbols();
    let exit_code = omni_verify::cranelift_backend::jit_execute_ir(&ll, &syms)
        .unwrap_or_else(|e| panic!("Cranelift JIT failed on advanced C99 IR:\n{}\n\nIR:\n{}", e, ll));
    assert_eq!(exit_code, 192, "Expected 192 from advanced C99 gaps suite");
}
