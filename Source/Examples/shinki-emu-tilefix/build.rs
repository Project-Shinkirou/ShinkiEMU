// SPDX-License-Identifier: MIT OR Apache-2.0

use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for input in ["HOST", "TARGET", "PROFILE"] {
        println!("cargo:rerun-if-env-changed={input}");
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()?;
    let id = "shinki.example.tilefix";
    let mut manifest = CoreBuildManifest::new(id, "rust-cdylib")?;
    for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
        println!("cargo:rerun-if-changed=../../../{file}");
        manifest.licenses.push(BuildAsset {
            source: root.join(file),
            dest: format!("licenses/{id}/{file}"),
        });
    }
    write_core_build_manifest(&manifest)?;
    Ok(())
}
