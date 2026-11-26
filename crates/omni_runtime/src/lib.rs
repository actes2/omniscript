use std::path::PathBuf;

/// Returns the canonical LLVM target triple of the current host machine (Linux, macOS, Windows).
pub fn host_target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "arm64-apple-macosx11.0.0";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-macosx10.15.0";
    #[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "gnu"))]
    return "x86_64-pc-windows-gnu";
    #[cfg(all(target_os = "windows", target_arch = "x86_64", not(target_env = "gnu")))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64", target_env = "gnu"))]
    return "aarch64-pc-windows-gnu";
    #[cfg(all(target_os = "windows", target_arch = "aarch64", not(target_env = "gnu")))]
    return "aarch64-pc-windows-msvc";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-pc-linux-gnu";
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
    )))]
    return "unknown-unknown-unknown";
}

/// The compiled native C systems runtime static archive (Arena, Channels, Fibers, Slices, Robin Hood Map).
/// Embedded directly into the binary to ensure 100% standalone execution without depending on source repos.
pub static EMBEDDED_LIBOMNI_RUNTIME: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/omni_runtime_archive.bin"));

/// Returns the path to the native runtime static library (`libomni_runtime_native.a` on Unix/MinGW, `omni_runtime_native.lib` on MSVC).
/// If not already present in `~/.omnicache/lib/` (or if bytes have changed), it writes
/// the embedded static archive out atomically.
pub fn runtime_archive_path() -> PathBuf {
    let cache_dir = if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        PathBuf::from(home).join(".omnicache").join("lib")
    } else {
        std::env::temp_dir().join("omnicache").join("lib")
    };
    let lib_path = cache_dir.join(env!("OMNI_RUNTIME_ARCHIVE_FILENAME"));

    let needs_write = match std::fs::read(&lib_path) {
        Ok(existing) => existing != EMBEDDED_LIBOMNI_RUNTIME,
        Err(_) => true,
    };

    if needs_write {
        let _ = std::fs::create_dir_all(&cache_dir);
        let _ = std::fs::write(&lib_path, EMBEDDED_LIBOMNI_RUNTIME);
    }

    lib_path
}

/// Returns the paths of all native runtime libraries required for Clang binary linking.
/// Returns the self-contained static archive path, ensuring zero dependencies on local source trees.
pub fn runtime_c_sources() -> Vec<PathBuf> {
    vec![runtime_archive_path()]
}

macro_rules! omni_runtime_decls {
    ($setjmp_sig:expr) => {
        concat!(
            r#"
; --- OmniScript Framework Runtime Declarations ---
; Standard POSIX libc & System Declarations
declare i32 @sleep(i32)
declare i32 @usleep(i32)

; Printing, Time & Hashing helpers
declare void @__omni_print_str(i8*)
declare void @__omni_print_str_inline(i8*)
declare void @__omni_print_str_raw(i8*)
declare void @__omni_print_int(i64)
declare void @__omni_print_float(double)
declare i8* @__omni_input(i8*)
declare void @exit(i32)
declare i32 @strcmp(i8*, i8*)
declare i8* @__omni_str_concat(i8*, i8*)
declare i64 @__omni_str_len(i8*)
declare i64 @__omni_str_eq(i8*, i8*)
declare i8* @__omni_str_char_at(i8*, i64)
declare i64 @__omni_str_byte_at(i8*, i64)
declare i64 @__omni_str_ord(i8*)
declare i8* @__omni_str_chr(i64)
declare i8* @__omni_str_slice_step(i8*, i64, i64, i64, i64, i64)
declare i64 @__omni_str_contains(i8*, i8*)
declare i64 @__omni_str_starts_with(i8*, i8*)
declare i64 @__omni_str_ends_with(i8*, i8*)
declare i64 @__omni_str_find(i8*, i8*)
declare i8* @__omni_str_upper(i8*)
declare i8* @__omni_str_lower(i8*)
declare i8* @__omni_str_trim(i8*)
declare i8* @__omni_str_replace(i8*, i8*, i8*)
declare i8* @__omni_str_split(i8*, i8*)
declare i8* @__omni_str_to_chars(i8*)
declare i8* @__omni_int_to_str(i64)
declare i8* @__omni_float_to_str(double)
declare double @__omni_c_str_to_f64(i8*)
declare i64 @__omni_c_str_to_i64(i8*)
declare void @__omni_sleep_s(i64)
declare void @__omni_sleep_ms(i64)
declare i64 @__omni_random_int()
declare i64 @__omni_random_range(i64, i64)
declare i64 @__omni_ipow64(i64, i64)
declare double @llvm.pow.f64(double, double)
declare void @fast_hash_block(i8*, i64, i8*)

; Thread-Local Stack-Unwinding Exception Runtime (setjmp / longjmp)
"#,
            $setjmp_sig,
            r#"
declare i8* @__omni_try_push()
declare void @__omni_try_pop()
declare void @__omni_throw(i64, i8*, i8*)
declare void @__omni_rethrow()
declare i8* @__omni_catch_msg()
declare i8* @__omni_catch_type()
declare i64 @__omni_catch_code()
declare i32 @__omni_catch_matches(i8*)

; Arena Allocator
declare i8* @__omni_arena_create(i64)
declare i8* @__omni_arena_alloc(i8*, i64, i64)
declare i64 @__omni_arena_mark(i8*)
declare void @__omni_arena_rewind(i8*, i64)
declare void @__omni_arena_reset(i8*)
declare void @__omni_arena_destroy(i8*)

; Channels (Go Concurrency Emulation)
declare i8* @__omni_chan_make(i64, i64)
declare i1 @__omni_chan_send(i8*, i8*)
declare i1 @__omni_chan_recv(i8*, i8*)
declare i1 @__omni_chan_send_int(i8*, i64)
declare i64 @__omni_chan_recv_int(i8*)
declare i64 @__omni_chan_len(i8*)
declare void @__omni_chan_close(i8*)
declare void @__omni_chan_destroy(i8*)

; Fiber / M:N Goroutine Runtime
declare void @__omni_fiber_init()
declare i8* @__omni_fiber_create(void (i8*)*, i8*, i64)
declare void @__omni_fiber_yield()
declare void @__omni_fiber_join_all()
declare void @__omni_fiber_destroy(i8*)
declare void @__omni_go_spawn(void (i8*)*, i8*)
declare void @__omni_thread_spawn(void (i8*)*, i8*)
declare void @__omni_thread_join_all()
declare i8* @__omni_fiber_run_on_main_stack(i8* (i8*)*, i8*)

; Polyglot Collections (Slices / Lists)
declare i8* @__omni_slice_create(i64)
declare void @__omni_slice_append(i8*, i64)
declare i64 @__omni_slice_get(i8*, i64)
declare void @__omni_slice_set(i8*, i64, i64)
declare i64 @__omni_slice_len(i8*)
declare i64 @__omni_slice_pop(i8*)
declare i8* @__omni_slice_subslice(i8*, i64, i64, i64, i64, i64)
declare i64 @__omni_slice_contains(i8*, i64, i64)
declare void @__omni_slice_print(i8*)
declare void @__omni_slice_destroy(i8*)

; Polyglot Collections (Hash Maps / Dicts)
declare i8* @__omni_map_create(i64)
declare void @__omni_map_set(i8*, i8*, i64)
declare i64 @__omni_map_get(i8*, i8*)
declare i1 @__omni_map_contains(i8*, i8*)
declare void @__omni_map_delete(i8*, i8*)
declare i64 @__omni_map_len(i8*)
declare void @__omni_map_destroy(i8*)
declare void @__omni_map_set_int(i8*, i64, i64)
declare i64 @__omni_map_get_int(i8*, i64)
declare i1 @__omni_map_contains_int(i8*, i64)
declare void @__omni_map_delete_int(i8*, i64)

; Perceus Refcounting & Fat Closures (`{ fn_ptr, env_ptr }`)
declare i8* @__omni_rc_retain(i8*)
declare void @__omni_rc_release(i8*)
declare i8* @__omni_closure_create(i8*, i8*)
declare void @__omni_closure_set_env_rc_mask(i8*, i64)
declare i64 @__omni_closure_is(i8*)
declare i8* @__omni_closure_fn(i8*)
declare i8* @__omni_closure_env(i8*)
declare void @__omni_closure_destroy(i8*)
declare i8* @__omni_obj_alloc(i64, i64)
declare void @__omni_obj_set_type(i8*, i64)
declare i64 @__omni_obj_get_type(i8*)

; Interpretive C-Frontier Short-Circuit Intrinsics & OS Socket Syscalls
declare i8* @__omni_c_http_get(i8*, i32*)
declare i8* @__omni_c_json_parse(i8*)
declare i8* @__omni_c_json_get(i8*, i8*)
declare i8* @__omni_obj_get(i8*, i8*)
declare i32 @__omni_c_to_float(i8*)
declare i32 @__omni_c_to_int(i8*)
declare i8* @__omni_c_return_str(i8*)
declare i8* @__omni_c_get_returned_str()
declare void @__omni_c_return_ptr(i8*)
declare i8* @__omni_c_get_returned_ptr()
declare i8* @__omni_c_consume_returned_ptr()
declare i8* @__omni_str_token(i8*, i32)
declare i8* @__omni_str_token_arena(i8*, i8*, i32)
declare i32 @__omni_os_socket_listen(i32)
declare i32 @__omni_os_socket_accept(i32)
declare i8* @__omni_os_socket_read(i32)
declare i8* @__omni_os_socket_read_arena(i8*, i32)
declare i32 @__omni_os_socket_write_http(i32, i32, i8*)
declare void @__omni_os_socket_close(i32)
declare i32 @__omni_os_socket_create(i32, i32, i32)
declare i32 @__omni_os_socket_bind(i32, i8*, i32)
declare i32 @__omni_os_socket_listen_fd(i32, i32)
declare i8* @__omni_os_socket_accept_tuple(i32)
declare i32 @__omni_os_socket_connect(i32, i8*, i32)
declare i8* @__omni_os_socket_getpeername(i32)
declare i32 @__omni_os_socket_send(i32, i8*)
declare i8* @__omni_os_socket_recv(i32, i32)
declare i8* @__omni_os_pipe_pair()

; Universal Growable Byte Buffer & File I/O (Self-Hosting & Systems Primitives)
declare i8* @__omni_bytes_create(i64)
declare void @__omni_bytes_push_u8(i8*, i64)
declare void @__omni_bytes_push_u16(i8*, i64)
declare void @__omni_bytes_push_u32(i8*, i64)
declare void @__omni_bytes_push_u64(i8*, i64)
declare void @__omni_bytes_push_str(i8*, i8*)
declare void @__omni_bytes_patch_u32(i8*, i64, i64)
declare i64 @__omni_bytes_get(i8*, i64)
declare i64 @__omni_bytes_len(i8*)
declare void @__omni_bytes_destroy(i8*)
declare i8* @__omni_fs_read_text(i8*)
declare i64 @__omni_fs_write_text(i8*, i8*)
declare i64 @__omni_fs_write_bytes(i8*, i8*)
declare i64 @__omni_fs_exists(i8*)
"#
        )
    };
}

/// Returns the C header declarations for LLVM IR module inclusion parameterized by target triple.
pub fn runtime_llvm_declarations_for_triple(triple: &str) -> &'static str {
    if triple.contains("windows") {
        omni_runtime_decls!("declare i32 @_setjmp(i8*, i8*) returns_twice")
    } else {
        omni_runtime_decls!("declare i32 @_setjmp(i8*) returns_twice")
    }
}

/// Returns the C header declarations for LLVM IR module inclusion on the current host.
pub fn runtime_llvm_declarations() -> &'static str {
    runtime_llvm_declarations_for_triple(host_target_triple())
}

/// Returns in-process function pointer bindings for every `omni_runtime_native` C symbol
/// so `cranelift-jit` can execute compiled OmniScript SSA IR directly in memory with zero linker latency.
pub fn jit_runtime_symbols() -> Vec<(&'static str, *const u8)> {
    extern "C" {
        fn __omni_print_str(s: *const i8);
        fn __omni_print_str_inline(s: *const i8);
        fn __omni_print_str_raw(s: *const i8);
        fn __omni_print_int(v: i64);
        fn __omni_print_float(v: f64);
        fn __omni_input(prompt: *const i8) -> *mut i8;
        fn __omni_str_concat(a: *const i8, b: *const i8) -> *mut i8;
        fn __omni_str_len(s: *const i8) -> i64;
        fn __omni_str_eq(a: *const i8, b: *const i8) -> i64;
        fn __omni_str_char_at(s: *const i8, idx: i64) -> *mut i8;
        fn __omni_str_byte_at(s: *const i8, idx: i64) -> i64;
        fn __omni_str_ord(s: *const i8) -> i64;
        fn __omni_str_chr(code: i64) -> *mut i8;
        fn __omni_str_slice_step(s: *const i8, a: i64, b: i64, c: i64, d: i64, e: i64) -> *mut i8;
        fn __omni_str_contains(a: *const i8, b: *const i8) -> i64;
        fn __omni_str_starts_with(a: *const i8, b: *const i8) -> i64;
        fn __omni_str_ends_with(a: *const i8, b: *const i8) -> i64;
        fn __omni_str_find(a: *const i8, b: *const i8) -> i64;
        fn __omni_str_upper(s: *const i8) -> *mut i8;
        fn __omni_str_lower(s: *const i8) -> *mut i8;
        fn __omni_str_trim(s: *const i8) -> *mut i8;
        fn __omni_str_replace(s: *const i8, a: *const i8, b: *const i8) -> *mut i8;
        fn __omni_str_split(s: *const i8, d: *const i8) -> *mut u8;
        fn __omni_str_to_chars(s: *const i8) -> *mut u8;
        fn __omni_int_to_str(v: i64) -> *mut i8;
        fn __omni_float_to_str(v: f64) -> *mut i8;
        fn __omni_c_str_to_f64(s: *const i8) -> f64;
        fn __omni_c_str_to_i64(s: *const i8) -> i64;
        fn __omni_sleep_s(s: i64);
        fn __omni_sleep_ms(ms: i64);
        fn __omni_random_int() -> i64;
        fn __omni_random_range(lo: i64, hi: i64) -> i64;
        fn __omni_ipow64(base: i64, exp: i64) -> i64;

        fn __omni_try_push() -> *mut u8;
        fn __omni_try_pop();
        fn __omni_throw(code: i64, ty: *const i8, msg: *const i8);
        fn __omni_rethrow();
        fn __omni_catch_msg() -> *const i8;
        fn __omni_catch_type() -> *const i8;
        fn __omni_catch_code() -> i64;
        fn __omni_catch_matches(ty: *const i8) -> i32;

        fn __omni_arena_create(cap: i64) -> *mut u8;
        fn __omni_arena_alloc(a: *mut u8, sz: i64, al: i64) -> *mut u8;
        fn __omni_arena_mark(a: *mut u8) -> i64;
        fn __omni_arena_rewind(a: *mut u8, m: i64);
        fn __omni_arena_reset(a: *mut u8);
        fn __omni_arena_destroy(a: *mut u8);

        fn __omni_chan_make(elem_sz: i64, cap: i64) -> *mut u8;
        fn __omni_chan_send(ch: *mut u8, ptr: *const u8) -> bool;
        fn __omni_chan_recv(ch: *mut u8, ptr: *mut u8) -> bool;
        fn __omni_chan_send_int(ch: *mut u8, val: i64) -> bool;
        fn __omni_chan_recv_int(ch: *mut u8) -> i64;
        fn __omni_chan_len(ch: *mut u8) -> i64;
        fn __omni_chan_close(ch: *mut u8);
        fn __omni_chan_destroy(ch: *mut u8);

        fn __omni_fiber_init();
        fn __omni_fiber_create(f: *const u8, arg: *mut u8, sz: i64) -> *mut u8;
        fn __omni_fiber_yield();
        fn __omni_fiber_join_all();
        fn __omni_fiber_destroy(f: *mut u8);
        fn __omni_go_spawn(f: *const u8, arg: *mut u8);
        fn __omni_thread_spawn(f: *const u8, arg: *mut u8);
        fn __omni_thread_join_all();
        fn __omni_fiber_run_on_main_stack(f: *const u8, arg: *mut u8) -> *mut u8;

        fn __omni_slice_create(cap: i64) -> *mut u8;
        fn __omni_slice_append(s: *mut u8, v: i64);
        fn __omni_slice_get(s: *mut u8, idx: i64) -> i64;
        fn __omni_slice_set(s: *mut u8, idx: i64, v: i64);
        fn __omni_slice_len(s: *mut u8) -> i64;
        fn __omni_slice_pop(s: *mut u8) -> i64;
        fn __omni_slice_subslice(s: *mut u8, a: i64, b: i64, c: i64, d: i64, e: i64) -> *mut u8;
        fn __omni_slice_contains(s: *mut u8, v: i64, is_str: i64) -> i64;
        fn __omni_slice_print(s: *mut u8);
        fn __omni_slice_destroy(s: *mut u8);

        fn __omni_map_create(cap: i64) -> *mut u8;
        fn __omni_map_set(m: *mut u8, k: *const i8, v: i64);
        fn __omni_map_get(m: *mut u8, k: *const i8) -> i64;
        fn __omni_map_contains(m: *mut u8, k: *const i8) -> bool;
        fn __omni_map_delete(m: *mut u8, k: *const i8);
        fn __omni_map_len(m: *mut u8) -> i64;
        fn __omni_map_destroy(m: *mut u8);
        fn __omni_map_set_int(m: *mut u8, k: i64, v: i64);
        fn __omni_map_get_int(m: *mut u8, k: i64) -> i64;
        fn __omni_map_contains_int(m: *mut u8, k: i64) -> bool;
        fn __omni_map_delete_int(m: *mut u8, k: i64);

        fn __omni_rc_retain(p: *mut u8) -> *mut u8;
        fn __omni_rc_release(p: *mut u8);
        fn __omni_closure_create(f: *const u8, env: *mut u8) -> *mut u8;
        fn __omni_closure_set_env_rc_mask(c: *mut u8, mask: i64);
        fn __omni_closure_is(p: *const u8) -> i64;
        fn __omni_closure_fn(c: *const u8) -> *const u8;
        fn __omni_closure_env(c: *const u8) -> *mut u8;
        fn __omni_closure_destroy(c: *mut u8);
        fn __omni_obj_alloc(sz: i64, rc_mask: i64) -> *mut u8;
        fn __omni_obj_set_type(p: *mut u8, tid: i64);
        fn __omni_obj_get_type(p: *const u8) -> i64;

        fn __omni_c_http_get(url: *const i8, status: *mut i32) -> *mut i8;
        fn __omni_c_json_parse(s: *const i8) -> *mut u8;
        fn __omni_c_json_get(j: *mut u8, k: *const i8) -> *mut i8;
        fn __omni_obj_get(o: *mut u8, k: *const i8) -> *mut i8;
        fn __omni_c_to_float(s: *const i8) -> i32;
        fn __omni_c_to_int(s: *const i8) -> i32;
        fn __omni_c_return_str(s: *const i8) -> *const i8;
        fn __omni_c_get_returned_str() -> *const i8;
        fn __omni_c_return_ptr(p: *mut u8);
        fn __omni_c_get_returned_ptr() -> *mut u8;
        fn __omni_c_consume_returned_ptr() -> *mut u8;
        fn __omni_str_token(s: *const i8, idx: i32) -> *mut i8;
        fn __omni_str_token_arena(a: *mut u8, s: *const i8, idx: i32) -> *mut i8;
        fn __omni_os_socket_listen(port: i32) -> i32;
        fn __omni_os_socket_accept(fd: i32) -> i32;
        fn __omni_os_socket_read(fd: i32) -> *mut i8;
        fn __omni_os_socket_read_arena(a: *mut u8, fd: i32) -> *mut i8;
        fn __omni_os_socket_write_http(fd: i32, status: i32, body: *const i8) -> i32;
        fn __omni_os_socket_close(fd: i32);
        fn __omni_os_socket_create(dom: i32, ty: i32, proto: i32) -> i32;
        fn __omni_os_socket_bind(fd: i32, host: *const i8, port: i32) -> i32;
        fn __omni_os_socket_listen_fd(fd: i32, backlog: i32) -> i32;
        fn __omni_os_socket_accept_tuple(fd: i32) -> *mut u8;
        fn __omni_os_socket_connect(fd: i32, host: *const i8, port: i32) -> i32;
        fn __omni_os_socket_getpeername(fd: i32) -> *mut i8;
        fn __omni_os_socket_send(fd: i32, data: *const i8) -> i32;
        fn __omni_os_socket_recv(fd: i32, n: i32) -> *mut i8;
        fn __omni_os_pipe_pair() -> *mut u8;

        fn __omni_bytes_create(cap: i64) -> *mut u8;
        fn __omni_bytes_push_u8(b: *mut u8, v: i64);
        fn __omni_bytes_push_u16(b: *mut u8, v: i64);
        fn __omni_bytes_push_u32(b: *mut u8, v: i64);
        fn __omni_bytes_push_u64(b: *mut u8, v: i64);
        fn __omni_bytes_push_str(b: *mut u8, s: *const i8);
        fn __omni_bytes_patch_u32(b: *mut u8, off: i64, v: i64);
        fn __omni_bytes_get(b: *mut u8, off: i64) -> i64;
        fn __omni_bytes_len(b: *mut u8) -> i64;
        fn __omni_bytes_destroy(b: *mut u8);
        fn __omni_fs_read_text(path: *const i8) -> *mut i8;
        fn __omni_fs_write_text(path: *const i8, content: *const i8) -> i64;
        fn __omni_fs_write_bytes(path: *const i8, b: *mut u8) -> i64;
        fn __omni_fs_exists(path: *const i8) -> i64;

        fn pow(x: f64, y: f64) -> f64;
    }

    vec![
        ("__omni_print_str", __omni_print_str as *const u8),
        ("__omni_print_str_inline", __omni_print_str_inline as *const u8),
        ("__omni_print_str_raw", __omni_print_str_raw as *const u8),
        ("__omni_print_int", __omni_print_int as *const u8),
        ("__omni_print_float", __omni_print_float as *const u8),
        ("__omni_input", __omni_input as *const u8),
        ("__omni_str_concat", __omni_str_concat as *const u8),
        ("__omni_str_len", __omni_str_len as *const u8),
        ("__omni_str_eq", __omni_str_eq as *const u8),
        ("__omni_str_char_at", __omni_str_char_at as *const u8),
        ("__omni_str_byte_at", __omni_str_byte_at as *const u8),
        ("__omni_str_ord", __omni_str_ord as *const u8),
        ("__omni_str_chr", __omni_str_chr as *const u8),
        ("__omni_str_slice_step", __omni_str_slice_step as *const u8),
        ("__omni_str_contains", __omni_str_contains as *const u8),
        ("__omni_str_starts_with", __omni_str_starts_with as *const u8),
        ("__omni_str_ends_with", __omni_str_ends_with as *const u8),
        ("__omni_str_find", __omni_str_find as *const u8),
        ("__omni_str_upper", __omni_str_upper as *const u8),
        ("__omni_str_lower", __omni_str_lower as *const u8),
        ("__omni_str_trim", __omni_str_trim as *const u8),
        ("__omni_str_replace", __omni_str_replace as *const u8),
        ("__omni_str_split", __omni_str_split as *const u8),
        ("__omni_str_to_chars", __omni_str_to_chars as *const u8),
        ("__omni_int_to_str", __omni_int_to_str as *const u8),
        ("__omni_float_to_str", __omni_float_to_str as *const u8),
        ("__omni_c_str_to_f64", __omni_c_str_to_f64 as *const u8),
        ("__omni_c_str_to_i64", __omni_c_str_to_i64 as *const u8),
        ("__omni_sleep_s", __omni_sleep_s as *const u8),
        ("__omni_sleep_ms", __omni_sleep_ms as *const u8),
        ("__omni_random_int", __omni_random_int as *const u8),
        ("__omni_random_range", __omni_random_range as *const u8),
        ("__omni_ipow64", __omni_ipow64 as *const u8),
        ("__omni_try_push", __omni_try_push as *const u8),
        ("__omni_try_pop", __omni_try_pop as *const u8),
        ("__omni_throw", __omni_throw as *const u8),
        ("__omni_rethrow", __omni_rethrow as *const u8),
        ("__omni_catch_msg", __omni_catch_msg as *const u8),
        ("__omni_catch_type", __omni_catch_type as *const u8),
        ("__omni_catch_code", __omni_catch_code as *const u8),
        ("__omni_catch_matches", __omni_catch_matches as *const u8),
        ("__omni_arena_create", __omni_arena_create as *const u8),
        ("__omni_arena_alloc", __omni_arena_alloc as *const u8),
        ("__omni_arena_mark", __omni_arena_mark as *const u8),
        ("__omni_arena_rewind", __omni_arena_rewind as *const u8),
        ("__omni_arena_reset", __omni_arena_reset as *const u8),
        ("__omni_arena_destroy", __omni_arena_destroy as *const u8),
        ("__omni_chan_make", __omni_chan_make as *const u8),
        ("__omni_chan_send", __omni_chan_send as *const u8),
        ("__omni_chan_recv", __omni_chan_recv as *const u8),
        ("__omni_chan_send_int", __omni_chan_send_int as *const u8),
        ("__omni_chan_recv_int", __omni_chan_recv_int as *const u8),
        ("__omni_chan_len", __omni_chan_len as *const u8),
        ("__omni_chan_close", __omni_chan_close as *const u8),
        ("__omni_chan_destroy", __omni_chan_destroy as *const u8),
        ("__omni_fiber_init", __omni_fiber_init as *const u8),
        ("__omni_fiber_create", __omni_fiber_create as *const u8),
        ("__omni_fiber_yield", __omni_fiber_yield as *const u8),
        ("__omni_fiber_join_all", __omni_fiber_join_all as *const u8),
        ("__omni_fiber_destroy", __omni_fiber_destroy as *const u8),
        ("__omni_go_spawn", __omni_go_spawn as *const u8),
        ("__omni_thread_spawn", __omni_thread_spawn as *const u8),
        ("__omni_thread_join_all", __omni_thread_join_all as *const u8),
        ("__omni_fiber_run_on_main_stack", __omni_fiber_run_on_main_stack as *const u8),
        ("__omni_slice_create", __omni_slice_create as *const u8),
        ("__omni_slice_append", __omni_slice_append as *const u8),
        ("__omni_slice_get", __omni_slice_get as *const u8),
        ("__omni_slice_set", __omni_slice_set as *const u8),
        ("__omni_slice_len", __omni_slice_len as *const u8),
        ("__omni_slice_pop", __omni_slice_pop as *const u8),
        ("__omni_slice_subslice", __omni_slice_subslice as *const u8),
        ("__omni_slice_contains", __omni_slice_contains as *const u8),
        ("__omni_slice_print", __omni_slice_print as *const u8),
        ("__omni_slice_destroy", __omni_slice_destroy as *const u8),
        ("__omni_map_create", __omni_map_create as *const u8),
        ("__omni_map_set", __omni_map_set as *const u8),
        ("__omni_map_get", __omni_map_get as *const u8),
        ("__omni_map_contains", __omni_map_contains as *const u8),
        ("__omni_map_delete", __omni_map_delete as *const u8),
        ("__omni_map_len", __omni_map_len as *const u8),
        ("__omni_map_destroy", __omni_map_destroy as *const u8),
        ("__omni_map_set_int", __omni_map_set_int as *const u8),
        ("__omni_map_get_int", __omni_map_get_int as *const u8),
        ("__omni_map_contains_int", __omni_map_contains_int as *const u8),
        ("__omni_map_delete_int", __omni_map_delete_int as *const u8),
        ("__omni_rc_retain", __omni_rc_retain as *const u8),
        ("__omni_rc_release", __omni_rc_release as *const u8),
        ("__omni_closure_create", __omni_closure_create as *const u8),
        ("__omni_closure_set_env_rc_mask", __omni_closure_set_env_rc_mask as *const u8),
        ("__omni_closure_is", __omni_closure_is as *const u8),
        ("__omni_closure_fn", __omni_closure_fn as *const u8),
        ("__omni_closure_env", __omni_closure_env as *const u8),
        ("__omni_closure_destroy", __omni_closure_destroy as *const u8),
        ("__omni_obj_alloc", __omni_obj_alloc as *const u8),
        ("__omni_obj_set_type", __omni_obj_set_type as *const u8),
        ("__omni_obj_get_type", __omni_obj_get_type as *const u8),
        ("__omni_c_http_get", __omni_c_http_get as *const u8),
        ("__omni_c_json_parse", __omni_c_json_parse as *const u8),
        ("__omni_c_json_get", __omni_c_json_get as *const u8),
        ("__omni_obj_get", __omni_obj_get as *const u8),
        ("__omni_c_to_float", __omni_c_to_float as *const u8),
        ("__omni_c_to_int", __omni_c_to_int as *const u8),
        ("__omni_c_return_str", __omni_c_return_str as *const u8),
        ("__omni_c_get_returned_str", __omni_c_get_returned_str as *const u8),
        ("__omni_c_return_ptr", __omni_c_return_ptr as *const u8),
        ("__omni_c_get_returned_ptr", __omni_c_get_returned_ptr as *const u8),
        ("__omni_c_consume_returned_ptr", __omni_c_consume_returned_ptr as *const u8),
        ("__omni_str_token", __omni_str_token as *const u8),
        ("__omni_str_token_arena", __omni_str_token_arena as *const u8),
        ("__omni_os_socket_listen", __omni_os_socket_listen as *const u8),
        ("__omni_os_socket_accept", __omni_os_socket_accept as *const u8),
        ("__omni_os_socket_read", __omni_os_socket_read as *const u8),
        ("__omni_os_socket_read_arena", __omni_os_socket_read_arena as *const u8),
        ("__omni_os_socket_write_http", __omni_os_socket_write_http as *const u8),
        ("__omni_os_socket_close", __omni_os_socket_close as *const u8),
        ("__omni_os_socket_create", __omni_os_socket_create as *const u8),
        ("__omni_os_socket_bind", __omni_os_socket_bind as *const u8),
        ("__omni_os_socket_listen_fd", __omni_os_socket_listen_fd as *const u8),
        ("__omni_os_socket_accept_tuple", __omni_os_socket_accept_tuple as *const u8),
        ("__omni_os_socket_connect", __omni_os_socket_connect as *const u8),
        ("__omni_os_socket_getpeername", __omni_os_socket_getpeername as *const u8),
        ("__omni_os_socket_send", __omni_os_socket_send as *const u8),
        ("__omni_os_socket_recv", __omni_os_socket_recv as *const u8),
        ("__omni_os_pipe_pair", __omni_os_pipe_pair as *const u8),
        ("__omni_bytes_create", __omni_bytes_create as *const u8),
        ("__omni_bytes_push_u8", __omni_bytes_push_u8 as *const u8),
        ("__omni_bytes_push_u16", __omni_bytes_push_u16 as *const u8),
        ("__omni_bytes_push_u32", __omni_bytes_push_u32 as *const u8),
        ("__omni_bytes_push_u64", __omni_bytes_push_u64 as *const u8),
        ("__omni_bytes_push_str", __omni_bytes_push_str as *const u8),
        ("__omni_bytes_patch_u32", __omni_bytes_patch_u32 as *const u8),
        ("__omni_bytes_get", __omni_bytes_get as *const u8),
        ("__omni_bytes_len", __omni_bytes_len as *const u8),
        ("__omni_bytes_destroy", __omni_bytes_destroy as *const u8),
        ("__omni_fs_read_text", __omni_fs_read_text as *const u8),
        ("__omni_fs_write_text", __omni_fs_write_text as *const u8),
        ("__omni_fs_write_bytes", __omni_fs_write_bytes as *const u8),
        ("__omni_fs_exists", __omni_fs_exists as *const u8),
        ("pow", pow as *const u8),
    ]
}

/// Build a temporary `__omni_slice` of C-string pointers for `argv` forwarding into
/// `OmniBcVm` (`omnic run app.obc arg1 arg2 ...`), invoke `f(slice_ptr as u64)`, and
/// destroy the slice cleanly afterwards.
pub fn with_argv_slice<R>(args: &[String], f: impl FnOnce(u64) -> R) -> R {
    extern "C" {
        fn __omni_slice_create(cap: i64) -> *mut u8;
        fn __omni_slice_append(s: *mut u8, v: i64);
        fn __omni_slice_destroy(s: *mut u8);
    }
    let cstrings: Vec<std::ffi::CString> = args
        .iter()
        .map(|s| std::ffi::CString::new(s.as_str()).unwrap_or_else(|_| std::ffi::CString::new("").unwrap()))
        .collect();
    unsafe {
        let sl = __omni_slice_create(cstrings.len().max(1) as i64);
        for cs in &cstrings {
            __omni_slice_append(sl, cs.as_ptr() as i64);
        }
        let res = f(sl as u64);
        __omni_slice_destroy(sl);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_sources_exist() {
        let sources = runtime_c_sources();
        assert_eq!(sources.len(), 1);
        for s in sources {
            assert!(s.exists(), "Source does not exist: {}", s.display());
            let metadata = std::fs::metadata(&s).expect("Failed to get metadata");
            assert!(metadata.len() > 10_000, "Archive size too small: {}", metadata.len());
        }
    }
}
