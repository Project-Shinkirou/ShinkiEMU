use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
use std::{env, fs::File, io::Read, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for file in ["src", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={file}");
    }
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing Cargo manifest directory")?)
            .join("../../..")
            .canonicalize()?;
    for name in ["NotoSansJP-Variable.ttf", "NotoSansSC-Variable.ttf"] {
        let path = root.join("Assets/Fonts").join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut header = [0; 4];
        if File::open(&path)?.read_exact(&mut header).is_err() || header != [0, 1, 0, 0] {
            return Err(format!(
                "{} is not a TrueType font; materialize the Git LFS assets before building",
                path.display()
            )
            .into());
        }
    }
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
    for name in ["SOURCE.md", "aws-lc-sys-0.46.0-LICENSE.txt"] {
        let path = format!("docs/licenses/shinki-emu-lucifen/{name}");
        let source = root.join(&path);
        println!("cargo:rerun-if-changed={}", source.display());
        manifest.licenses.push(BuildAsset {
            source,
            dest: format!("licenses/shinki.emu.lucifen/{name}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
