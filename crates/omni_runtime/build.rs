fn main() {
    println!("cargo:rerun-if-changed=src/native");

    let mut build = cc::Build::new();
    build.include("src/native");
    build.file("src/native/omni_arena.c");
    build.file("src/native/omni_chan.c");
    build.file("src/native/omni_fiber.c");
    build.file("src/native/omni_map.c");
    build.file("src/native/omni_slice.c");
    build.file("src/native/omni_value.c");
    build.opt_level(2);
    build.compile("omni_runtime_native");
}
