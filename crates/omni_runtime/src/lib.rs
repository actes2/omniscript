use std::path::PathBuf;

/// Returns paths to all bundled C runtime sources (Arena, Channels, etc.)
pub fn runtime_c_sources() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let native_dir = manifest_dir.join("src").join("native");
    vec![
        native_dir.join("omni_arena.c"),
        native_dir.join("omni_chan.c"),
        native_dir.join("omni_value.c"),
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
declare void @__omni_chan_close(i8*)
declare void @__omni_chan_destroy(i8*)
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_sources_exist() {
        let sources = runtime_c_sources();
        assert_eq!(sources.len(), 3);
        for s in sources {
            assert!(s.exists(), "Source does not exist: {}", s.display());
        }
    }
}
