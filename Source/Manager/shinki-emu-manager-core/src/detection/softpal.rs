use super::formats::Rejected;
use super::*;

impl<T: DetectionTree> Scan<'_, T> {
    fn pac_entry(
        &mut self,
        source: &mut Source,
        wanted: &[u8],
    ) -> Result<Option<(u64, u64)>, Rejected> {
        // pal-asset/pac.rs: 255 buckets at 0x0c, 40-byte records at 0x804.
        // An index alone is not a family match; callers validate actual resources.
        if source.length < 0x804 {
            return Err(Rejected::NoMatch);
        }
        let header = self.bytes(source, 0, 0x804)?;
        let word = |bytes: &[u8], at: usize| {
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as u64
        };
        // Reject unrelated large files before any record-table allocation.
        for bucket in header[12..].chunks_exact(8) {
            self.checkpoint()?;
            let count = word(bucket, 4);
            if count != 0 && 0x804 + (word(bucket, 0) + count) * 40 > source.length {
                return Err(Rejected::NoMatch);
            }
        }
        let mut found = None;
        for bucket in header[12..].chunks_exact(8) {
            self.checkpoint()?;
            let first = word(bucket, 0);
            let count = word(bucket, 4);
            if count == 0 {
                continue;
            }
            let records = self.bytes(source, 0x804 + first * 40, (count * 40) as usize)?;
            for record in records.chunks_exact(40) {
                self.checkpoint()?;
                let size = word(record, 32);
                let offset = word(record, 36);
                if offset + size > source.length {
                    return Err(Rejected::NoMatch);
                }
                let end = record[..32].iter().position(|b| *b == 0).unwrap_or(32);
                if record[..end].eq_ignore_ascii_case(wanted) {
                    found = Some((offset, size));
                }
            }
        }
        Ok(found)
    }
    pub(super) fn softpal_pac(&mut self, source: &mut Source) -> Result<(), Rejected> {
        let Some((offset, size)) = self.pac_entry(source, b"Script.src")? else {
            return Err(Rejected::NoMatch);
        };
        if size < 13 {
            return Err(Rejected::NoMatch);
        }
        let mut header = [0; 12];
        self.read_at(source, offset, &mut header)?;
        let entry = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;
        if &header[..4] != b"Sv20" || entry < 12 || entry >= size {
            return Err(Rejected::NoMatch);
        }
        self.matched("softpal", "softpal.sv20")?;
        Ok(())
    }
    pub(super) fn softpal_manifest(&mut self, root: &Path) -> DetectionResult<()> {
        if self.matches.iter().any(|m| m.family_id == "softpal") {
            return Ok(());
        }
        // ResourceManager::bootstrap: data first, root loose fallback, then csv.
        for (name, packed) in [
            ("data/archive.dat", false),
            ("data.pac", true),
            ("archive.dat", false),
            ("csv/archive.dat", false),
            ("csv.pac", true),
        ] {
            let Some(path) = self.relative_path(root, name)? else {
                continue;
            };
            let bytes = if packed {
                let full = root.join(&path);
                let Some(mut source) = self.open(&full, 128)? else {
                    continue;
                };
                let (offset, size) = match self.pac_entry(&mut source, b"archive.dat") {
                    Ok(Some(entry)) => entry,
                    Ok(None) | Err(Rejected::NoMatch) => continue,
                    Err(Rejected::Abort(error)) => return Err(error),
                };
                self.bytes(&mut source, offset, size as usize)?
            } else {
                self.resource_bytes(root, &path)?
            };
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            let text = std::str::from_utf8(&bytes[..end])
                .ok()
                .map(std::borrow::Cow::Borrowed)
                .or_else(|| {
                    shinki_emu_musica_format::ScriptEncoding::ShiftJis.decode(&bytes[..end])
                });
            let Some(text) = text else {
                return Ok(());
            };
            // Native startup selects profile zero; later comma profiles require
            // script execution and are not silently substituted at recognition.
            let first = text.split(',').next().unwrap_or("");
            let compact = first
                .chars()
                .filter(|c| !matches!(c, '\r' | '\n' | ' ' | '\t'))
                .collect::<String>();
            for base in compact.split('|').filter(|s| !s.is_empty()) {
                self.checkpoint()?;
                if base.len() > 4096 {
                    continue;
                }
                let base = base.replace('\\', "/");
                for candidate in [format!("{base}/Script.src"), format!("{base}.pac")] {
                    if let Some(path) = self.relative_path(root, &candidate)? {
                        if self.files == MAX_FILES {
                            return Err(DetectionError::ReadLimit);
                        }
                        self.files += 1;
                        self.inspect(root, &path)?;
                        if self.matches.iter().any(|m| m.family_id == "softpal") {
                            return Ok(());
                        }
                    }
                }
            }
            return Ok(());
        }
        Ok(())
    }
}
