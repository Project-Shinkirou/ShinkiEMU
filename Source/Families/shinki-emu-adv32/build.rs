use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest directory")?,
    )
    .join("../../..")
    .canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.adv32", "rust-cdylib")?;
    let windows_standalone = std::env::var("TARGET")? == "x86_64-pc-windows-msvc"
        && std::env::var_os("CARGO_FEATURE_DYNAMIC_PLUGIN_EXPORT").is_some()
        && std::env::var_os("CARGO_FEATURE_VIDEO_FFMPEG").is_some();
    for path in [
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "docs/licenses/shinki-emu-adv32/GARbro.txt",
        "Assets/Fonts/OFL-NotoSansJP.txt",
        "Assets/Fonts/OFL-NotoSansSC.txt",
    ]
    .into_iter()
    .chain(windows_standalone.then_some("docs/licenses/shinki-emu-adv32/WINDOWS-FFMPEG.md"))
    {
        let source = repo.join(path);
        println!("cargo:rerun-if-changed={}", source.display());
        let file = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("missing license filename")?;
        manifest.licenses.push(BuildAsset {
            source: source.clone(),
            dest: format!("licenses/shinki.emu.adv32/{file}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
