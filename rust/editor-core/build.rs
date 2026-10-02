fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rerun-if-changed=src/android_allocator.c");
        cc::Build::new()
            .file("src/android_allocator.c")
            .include(std::env::var("DEP_MIMALLOC_INCLUDE_DIR").expect("mimalloc headers"))
            .flag("-fvisibility=hidden")
            .compile("editor_android_allocator");
    }
}
