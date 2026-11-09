use super::*;
use shinki_emu_musica_format::{ScLineKind, ScOpcodeCatalog, ScriptEncoding, decode_scheme, parse_sc_with_encoding, paz};
use shinki_emu_musica_keys::{MAX_KEY_BYTES, parse_key_file};

impl Scan<'_> {
    pub(super) fn musica(&mut self, root: &Path, files: &[PathBuf]) -> DetectionResult<()> {
        self.checkpoint()?;
        let find = |name: &str| files.iter().find(|path| path.file_name().is_some_and(|file| file.eq_ignore_ascii_case(name)));
        let (Some(key_path), Some(archive_path)) = (find("key.toml"), find("scr.paz")) else { return Ok(()); };
        let Some(mut key_source) = self.musica_source(root, key_path)? else { return Ok(()); };
        if key_source.length > MAX_KEY_BYTES { return Ok(()); }
        let length = key_source.length as usize;
        let key_bytes = self.bytes(&mut key_source, 0, length)?;
        self.checkpoint()?;
        let Ok(key) = parse_key_file(&key_bytes) else { return Ok(()); };
        let Some(archive_key) = key.archive_keys.get("scr") else { return Ok(()); };
        let scheme = decode_scheme(archive_key, key.rc4_skip_crc);
        let Some(mut source) = self.musica_source(root, archive_path)? else { return Ok(()); };
        let mut header = [0; 36];
        let length = if key.paz_version == 0 { 4 } else { 36 };
        if source.length < length as u64 { return Ok(()); }
        self.read_at(&mut source, 0, &mut header[..length])?;
        let Ok(layout) = paz::index_layout(&header[..length], key.paz_version, key.index_size_xor, source.length) else { return Ok(()); };
        let mut encrypted = self.bytes(&mut source, layout.offset, layout.size as usize)?;
        paz::xor_byte(&mut encrypted, layout.xor);
        let Ok(decoded) = paz::blowfish_decrypt(&scheme.index_key, &encrypted) else { return Ok(()); };
        self.checkpoint()?;
        let entries = paz::parse_index(&decoded, "scr", source.length, || self.cancellation.is_cancelled());
        self.checkpoint()?;
        let Ok(mut entries) = entries else { return Ok(()); };
        entries.retain(|entry| entry.name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("sc")));
        // Smaller complete scripts establish the format without unpacking a game.
        entries.sort_by_key(|entry| entry.aligned_size);
        for entry in entries {
            self.checkpoint()?;
            if entry.aligned_size == 0 || entry.unpacked_size == 0 { continue; }
            if entry.aligned_size > MAX_FILE_READ_BYTES || entry.unpacked_size > MAX_FILE_READ_BYTES {
                return Err(DetectionError::ReadLimit);
            }
            let mut encrypted = self.bytes(&mut source, entry.offset, entry.aligned_size as usize)?;
            paz::xor_byte(&mut encrypted, layout.xor);
            let Ok(decoded) = paz::decrypt_entry_chunk(&scheme, key.paz_version, &entry, 0, &encrypted) else { continue; };
            self.checkpoint()?;
            let Ok(decoded) = paz::finish_entry(&entry, decoded) else { continue; };
            self.checkpoint()?;
            let encoding = ScriptEncoding::detect(&decoded, ScriptEncoding::ShiftJis);
            let parsed = parse_sc_with_encoding(&decoded, &ScOpcodeCatalog::observed_musica(), encoding);
            self.checkpoint()?;
            if parsed.is_ok_and(|script| script.lines.iter().any(|line| matches!(&line.kind, ScLineKind::Command { command } if command.known))) {
                return self.matched("musica", "musica.paz");
            }
        }
        Ok(())
    }
    fn musica_source(&self, root: &Path, path: &Path) -> DetectionResult<Option<Source>> {
        self.checkpoint()?;
        let metadata = fs::symlink_metadata(root.join(path))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() { return Ok(None); }
        self.checkpoint()?;
        let file = File::open(root.join(path))?;
        // Enumeration already inspected up to 128 header bytes of this file.
        Ok(Some(Source { file, length: metadata.len(), read_bytes: 128 }))
    }
}
