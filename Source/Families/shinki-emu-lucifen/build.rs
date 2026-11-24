use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for file in ["src", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing Cargo manifest directory")?)
            .join("../../..")
            .canonicalize()?;
    let mut manifest = CoreBuildManifest::new("shinki.emu.lucifen", "rust-cdylib")?;
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.md"] {
        manifest.licenses.push(BuildAsset {
            source: root.join(name),
            dest: format!("licenses/shinki.emu.lucifen/{name}"),
        });
    }
    for name in ["OFL-NotoSansJP.txt", "OFL-NotoSansSC.txt"] {
        manifest.licenses.push(BuildAsset {
            source: root.join("Assets/Fonts").join(name),
            dest: format!("licenses/shinki.emu.lucifen/{name}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
