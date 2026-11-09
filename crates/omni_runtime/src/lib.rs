use std::path::PathBuf;

/// Returns the canonical LLVM target triple of the current host machine (Linux, macOS, Windows).
pub fn host_target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "arm64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
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
    include_bytes!(concat!(env!("OUT_DIR"), "/libomni_runtime_native.a"));

/// Returns the path to the native runtime static library (`libomni_runtime_native.a`).
/// If not already present in `~/.omnicache/lib/` (or if bytes have changed), it writes
/// the embedded static archive out atomically.
pub fn runtime_archive_path() -> PathBuf {
    let cache_dir = if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        PathBuf::from(home).join(".omnicache").join("lib")
    } else {
        std::env::temp_dir().join("omnicache").join("lib")
    };
    let lib_path = cache_dir.join("libomni_runtime_native.a");

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

/// Returns the C header declarations for LLVM IR module inclusion
pub fn runtime_llvm_declarations() -> &'static str {
    r#"
; --- OmniScript Framework Runtime Declarations ---
; Standard POSIX libc & System Declarations
declare i32 @sleep(i32)
declare i32 @usleep(i32)

; Printing, Time & Hashing helpers
declare void @__omni_print_str(i8*)
declare void @__omni_print_int(i64)
declare i8* @__omni_str_concat(i8*, i8*)
declare i8* @__omni_int_to_str(i64)
declare void @__omni_sleep_s(i64)
declare void @__omni_sleep_ms(i64)
declare void @fast_hash_block(i8*, i64, i8*)

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
declare void @__omni_fiber_destroy(i8*)
declare void @__omni_go_spawn(void (i8*)*, i8*)
declare i8* @__omni_fiber_run_on_main_stack(i8* (i8*)*, i8*)

; Polyglot Collections (Slices / Lists)
declare i8* @__omni_slice_create(i64)
declare void @__omni_slice_append(i8*, i64)
declare i64 @__omni_slice_get(i8*, i64)
declare void @__omni_slice_set(i8*, i64, i64)
declare i64 @__omni_slice_len(i8*)
declare i64 @__omni_slice_pop(i8*)
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

; Interpretive C-Frontier Short-Circuit Intrinsics & OS Socket Syscalls
declare i8* @__omni_c_http_get(i8*, i32*)
declare i8* @__omni_c_json_parse(i8*)
declare i8* @__omni_c_json_get(i8*, i8*)
declare i8* @__omni_obj_get(i8*, i8*)
declare i32 @__omni_c_to_float(i8*)
declare i32 @__omni_c_to_int(i8*)
declare i8* @__omni_c_return_str(i8*)
declare i8* @__omni_c_get_returned_str()
declare i8* @__omni_str_token(i8*, i32)
declare i8* @__omni_str_token_arena(i8*, i8*, i32)
declare i32 @__omni_os_socket_listen(i32)
declare i32 @__omni_os_socket_accept(i32)
declare i8* @__omni_os_socket_read(i32)
declare i8* @__omni_os_socket_read_arena(i8*, i32)
declare i32 @__omni_os_socket_write_http(i32, i32, i8*)
declare void @__omni_os_socket_close(i32)
"#
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
