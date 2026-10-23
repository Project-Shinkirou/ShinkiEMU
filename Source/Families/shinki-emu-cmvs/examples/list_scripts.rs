//! Lists every script in the script pack for chapter-structure audits.
//!
//! Usage: `cargo run --release --example list_scripts -- <game_root> <profile>`

use std::path::PathBuf;

use shinki_emu_cmvs::mount_cmvs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("missing game root")?);
    let profile = PathBuf::from(args.next().unwrap_or_else(|| "cmvs.profile.json".into()));
    let archive = mount_cmvs(&root, &profile)?;
    let manifest = archive.manifest()?;
    let scripts: Vec<&str> = manifest
        .entries
        .iter()
        .filter(|entry| entry.uri.contains("/code/") && entry.uri.ends_with(".ps3"))
        .map(|entry| entry.uri.as_str())
        .collect();
    println!("total scripts: {}", scripts.len());
    for uri in scripts {
        println!("{}", uri);
    }
    Ok(())
}
