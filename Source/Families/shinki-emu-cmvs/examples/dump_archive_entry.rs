//! List one archive directory and dump an entry by URI suffix.
//! Usage: dump_archive_entry <game-root> <profile> <dir-uri> [suffix out-path]

use std::path::PathBuf;

use shinki_emu_cmvs::{mount_cmvs, CMVS_PROFILE_FILE};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("missing game root")?);
    let profile = PathBuf::from(args.next().unwrap_or_else(|| CMVS_PROFILE_FILE.into()));
    let dir = args.next().ok_or("missing dir uri")?;
    let archive = mount_cmvs(&root, &profile)?;
    for node in archive.read_dir(&dir)? {
        println!("{}", node.uri);
    }
    if let (Some(suffix), Some(out)) = (args.next(), args.next()) {
        let nodes = archive.read_dir(&dir)?;
        let hit = nodes
            .iter()
            .find(|node| node.uri.ends_with(&suffix))
            .ok_or_else(|| format!("no entry ending with {suffix}"))?;
        let size = archive.stat(&hit.uri)?.size;
        let mut bytes = Vec::with_capacity(size as usize);
        let mut offset = 0_u64;
        while offset < size {
            let len = (size - offset).min(8 * 1024 * 1024);
            let part = archive.read_range(&hit.uri, offset, len)?;
            bytes.extend_from_slice(&part.bytes);
            offset += len;
        }
        std::fs::write(&out, &bytes)?;
        println!("wrote {} bytes to {}", bytes.len(), out);
    }
    Ok(())
}
