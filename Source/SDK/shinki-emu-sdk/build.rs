fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("ohos")
        || std::env::var_os("CARGO_FEATURE_RUNTIME").is_none()
    {
        return;
    }
    let source = "src/runtime/audio/ohos.c";
    println!("cargo:rerun-if-changed={source}");
    cc::Build::new()
        .file(source)
        .flag("-Wall")
        .flag("-Werror")
        .compile("shinki_ohos_audio");
    println!("cargo:rustc-link-lib=dylib=ohaudio");
}
