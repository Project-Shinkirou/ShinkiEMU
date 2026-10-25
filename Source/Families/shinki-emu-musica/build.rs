use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
use std::{env, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is missing")?)
            .join("../../..")
            .canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.musica", "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        manifest.licenses.push(BuildAsset {
            source: root.join(file),
            dest: format!("licenses/shinki.emu.musica/{file}"),
        });
    }
    for file in ["OFL-NotoSansJP.txt", "OFL-NotoSansSC.txt"] {
        manifest.licenses.push(BuildAsset {
            source: root.join("Assets/Fonts").join(file),
            dest: format!("licenses/shinki.emu.musica/{file}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
