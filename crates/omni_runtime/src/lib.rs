use std::path::PathBuf;

/// Returns paths to all bundled C runtime sources (Arena, Channels, etc.)
pub fn runtime_c_sources() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_dir = manifest_dir.join("src").join("native");
    vec![
        native_dir.join("omni_arena.c"),
        native_dir.join("omni_chan.c"),
        native_dir.join("omni_value.c"),
        native_dir.join("omni_fiber.c"),
    ]
}

/// Returns the C header declarations for LLVM IR module inclusion
pub fn runtime_llvm_declarations() -> &'static str {
    r#"
; --- OmniScript Framework Runtime Declarations ---
; Printing helpers
declare void @__omni_print_str(i8*)
declare void @__omni_print_int(i64)

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
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_sources_exist() {
        let sources = runtime_c_sources();
        assert_eq!(sources.len(), 4);
        for s in sources {
            assert!(s.exists(), "Source does not exist: {}", s.display());
        }
    }
}
