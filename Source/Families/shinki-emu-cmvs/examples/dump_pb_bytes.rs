//! Extracts raw PB image files from the mounted game archive for offline
//! decoder analysis.
//!
//! Usage: `cargo run --release --example dump_pb_bytes -- <game_root> <profile> <out_dir> <name.pb3>...`

use std::path::PathBuf;

use shinki_emu_cmvs::{mount_cmvs, CmvsPs2aVmState, CmvsResourceSearchBank};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("missing game root")?);
    let profile = PathBuf::from(args.next().ok_or("missing profile path")?);
    let out_dir = PathBuf::from(args.next().ok_or("missing out dir")?);
    std::fs::create_dir_all(&out_dir)?;
    let names: Vec<String> = args.collect();
    let archive = mount_cmvs(&root, &profile)?;
    let packs = [
        "",
        "data\\pack\\balloon.cpz",
        "data\\pack\\bg.cpz",
        "data\\pack\\chip.cpz",
        "data\\pack\\event.cpz",
        "data\\pack\\event2.cpz",
        "data\\pack\\script.cpz",
        "data\\pack\\se.cpz",
        "data\\pack\\stand.cpz",
        "data\\pack\\up.cpz",
        "data\\pack\\update.cpz",
        "data\\pack\\video.cpz",
        "data\\pack\\voice.cpz",
    ];
    for name in &names {
        for pack in packs {
            let mut probe = CmvsPs2aVmState::new(0);
            probe
                .resource_search_paths
                .entry(CmvsResourceSearchBank::Texture)
                .or_default()
                .insert(0, pack.to_string());
            let Ok(uri) =
                archive.resolve_native_resource(&probe, CmvsResourceSearchBank::Texture, name)
            else {
                continue;
            };
            let image_type = archive
                .pb_image_type(&uri)
                .map(|t| t.to_string())
                .unwrap_or_else(|e| format!("?{}", e.code()));
            let image = match archive.decode_pb_image(&uri) {
                Ok(image) => image,
                Err(e) => {
                    println!("{name} @ {pack}: type {image_type} DECODE-ERR {}", e.code());
                    continue;
                }
            };
            let raw = image.as_raw();
            let mean = if raw.is_empty() {
                0.0
            } else {
                raw.iter().map(|b| u32::from(*b)).sum::<u32>() as f64 / raw.len() as f64
            };
            let first = &raw[..raw.len().min(4)];
            if let Ok(payload) = archive.debug_pb3_payload(&uri) {
                let payload_path = out_dir.join(format!("{}.payload", name.replace('\\', "_")));
                let _ = std::fs::write(&payload_path, &payload);
            }

            let out = out_dir.join(format!(
                "{}.{}.png",
                name.replace('\\', "_"),
                pack.replace(['\\', '/'], "_")
            ));
            if let Err(e) = image.save_with_format(&out, image::ImageFormat::Png) {
                println!("save failed: {e}");
            }
            println!(
                "{name} @ {pack}: type {image_type} {}x{} mean {mean:.2} first {first:?} -> {}",
                image.width(),
                image.height(),
                out.display()
            );
        }
    }
    Ok(())
}
