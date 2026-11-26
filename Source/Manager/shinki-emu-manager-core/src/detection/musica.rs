use super::*;
use shinki_emu_musica_format::{
    ScLineKind, ScOpcodeCatalog, ScriptEncoding, decode_scheme, parse_sc_with_encoding, paz,
};
use shinki_emu_musica_keys::{MAX_KEY_BYTES, parse_key_file};

type Configuration = std::collections::BTreeMap<String, serde_json::Value>;

struct Archive {
    parts: Vec<Source>,
    length: u64,
}

impl<T: DetectionTree> Scan<'_, T> {
    pub(super) fn musica(&mut self, root: &Path, files: &[PathBuf]) -> DetectionResult<()> {
        let configuration = self.configuration(root)?;
        self.checkpoint()?;
        let configured = |name: &str| {
            configuration
                .as_ref()
                .and_then(|c| c.get(name))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        };
        let key_name = configured("profile_file").unwrap_or("key.toml");
        let Some(key_path) = self.relative_path(root, key_name)? else {
            return Ok(());
        };
        let full = root.join(&key_path);
        let Some(mut source) = self.open(&full, 128)? else {
            return Ok(());
        };
        if source.length > MAX_KEY_BYTES {
            return Ok(());
        }
        let length = source.length as usize;
        let key_bytes = self.bytes(&mut source, 0, length)?;
        self.checkpoint()?;
        let Ok(key) = parse_key_file(&key_bytes) else {
            return Ok(());
        };
        let Some(archive_key) = key.archive_keys.get("scr") else {
            return Ok(());
        };
        let scheme = decode_scheme(archive_key, key.rc4_skip_crc);
        let mut candidates = if let Some(name) = configured("scr_archive") {
            self.relative_path(root, name)?
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            // Alias configuration can live in Manager settings, outside this API's
            // directory input. Validate actual archive bytes, not their filenames.
            let mut paths = files.to_vec();
            paths.sort_by_key(|p| {
                !p.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("scr.paz"))
            });
            paths
        };
        let catalog = ScOpcodeCatalog::observed_musica();
        candidates.reverse();
        let mut search_nested = configured("scr_archive").is_none();
        loop {
            let path = match candidates.pop() {
                Some(path) => path,
                None if search_nested => {
                    search_nested = false;
                    candidates = self.musica_nested_files(root)?;
                    candidates.reverse();
                    continue;
                }
                None => break,
            };
            self.checkpoint()?;
            let Some(mut archive) = self.musica_archive(root, &path, files)? else {
                continue;
            };
            let length = if key.paz_version == 0 { 4 } else { 36 };
            if archive.length < length as u64 {
                continue;
            }
            let header = self.musica_bytes(&mut archive, 0, length)?;
            let Ok(layout) =
                paz::index_layout(&header, key.paz_version, key.index_size_xor, archive.length)
            else {
                continue;
            };
            // Check one decrypted count before spending the budget on an arbitrary
            // file whose header happens to resemble the magic-less PAZ layout.
            let mut first = self.musica_bytes(&mut archive, layout.offset, 8)?;
            paz::xor_byte(&mut first, layout.xor);
            let Ok(first) = paz::blowfish_decrypt(&scheme.index_key, &first) else {
                continue;
            };
            let count = u32::from_le_bytes(first[..4].try_into().unwrap()) as u64;
            if count == 0 || count > layout.size / 25 {
                continue;
            }
            let mut encrypted =
                self.musica_bytes(&mut archive, layout.offset, layout.size as usize)?;
            paz::xor_byte(&mut encrypted, layout.xor);
            let Ok(decoded) = paz::blowfish_decrypt(&scheme.index_key, &encrypted) else {
                continue;
            };
            self.checkpoint()?;
            let entries = paz::parse_index(&decoded, "scr", archive.length, || {
                self.cancellation.is_cancelled()
            });
            self.checkpoint()?;
            let Ok(mut entries) = entries else {
                continue;
            };
            if let Some(name) = configured("script_directory") {
                if let Some(directory) = self.relative_path(root, name)? {
                    for path in self.musica_files(root, &directory)? {
                        if !path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("sc"))
                        {
                            continue;
                        }
                        let bytes = self.resource_bytes(root, &path)?;
                        if self.musica_script(&bytes, &catalog)? {
                            return self.matched("musica", "musica.paz");
                        }
                    }
                }
            }
            entries.retain(|entry| {
                entry
                    .name
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("sc"))
            });
            entries.sort_by_key(|entry| entry.aligned_size);
            for entry in entries {
                self.checkpoint()?;
                if entry.aligned_size == 0 || entry.unpacked_size == 0 {
                    continue;
                }
                if entry.aligned_size > MAX_FILE_READ_BYTES
                    || entry.unpacked_size > MAX_FILE_READ_BYTES
                {
                    return Err(DetectionError::ReadLimit);
                }
                let mut encrypted =
                    self.musica_bytes(&mut archive, entry.offset, entry.aligned_size as usize)?;
                paz::xor_byte(&mut encrypted, layout.xor);
                let Ok(decoded) =
                    paz::decrypt_entry_chunk(&scheme, key.paz_version, &entry, 0, &encrypted)
                else {
                    continue;
                };
                self.checkpoint()?;
                let Ok(decoded) = paz::finish_entry(&entry, decoded) else {
                    continue;
                };
                if self.musica_script(&decoded, &catalog)? {
                    return self.matched("musica", "musica.paz");
                }
            }
        }
        Ok(())
    }

    fn configuration(&mut self, root: &Path) -> DetectionResult<Option<Configuration>> {
        #[derive(Deserialize)]
        struct Receipt {
            schema: String,
            profile: Profile,
        }
        #[derive(Deserialize)]
        struct Profile {
            #[serde(default)]
            configuration: Configuration,
        }
        // Same precedence as shinki-emu-patch::read_output_profile.
        for name in ["shinki-patch-result.json", ".shinki/patch-result.json"] {
            let Some(path) = self.relative_path(root, name)? else {
                continue;
            };
            let bytes = self.resource_bytes(root, &path)?;
            self.checkpoint()?;
            return Ok(serde_json::from_slice::<Receipt>(&bytes)
                .ok()
                .filter(|r| r.schema == "shinki.emu.file_patch.v1")
                .map(|r| r.profile.configuration));
        }
        Ok(None)
    }

    fn musica_script(&self, bytes: &[u8], catalog: &ScOpcodeCatalog) -> DetectionResult<bool> {
        self.checkpoint()?;
        let encoding = ScriptEncoding::detect(bytes, ScriptEncoding::ShiftJis);
        let parsed = parse_sc_with_encoding(bytes, catalog, encoding);
        self.checkpoint()?;
        Ok(parsed.is_ok_and(|script| {
            script
                .lines
                .iter()
                .any(|line| matches!(&line.kind, ScLineKind::Command { command } if command.known))
        }))
    }
    fn musica_nested_files(&mut self, root: &Path) -> DetectionResult<Vec<PathBuf>> {
        let mut pending = vec![PathBuf::new()];
        let mut files = Vec::new();
        while let Some(directory) = pending.pop() {
            let file_limit = MAX_FILES - self.files;
            self.entries(&root.join(&directory), |name, kind| {
                if kind == EntryKind::Directory {
                    pending.push(directory.join(name));
                } else if kind == EntryKind::File && !directory.as_os_str().is_empty() {
                    if files.len() == file_limit as usize {
                        return Err(DetectionError::ReadLimit);
                    }
                    files.push(directory.join(name));
                }
                Ok(())
            })?;
        }
        files.sort();
        Ok(files)
    }
    fn musica_files(&mut self, root: &Path, directory: &Path) -> DetectionResult<Vec<PathBuf>> {
        self.checkpoint()?;
        let mut files = Vec::new();
        self.entries(&root.join(directory), |name, kind| {
            if kind == EntryKind::File {
                files.push(directory.join(name));
            }
            Ok(())
        })?;
        files.sort();
        Ok(files)
    }
    fn musica_archive(
        &mut self,
        root: &Path,
        base: &Path,
        root_files: &[PathBuf],
    ) -> DetectionResult<Option<Archive>> {
        let parent = base.parent().unwrap_or(Path::new(""));
        let siblings = if parent.as_os_str().is_empty() {
            root_files.to_vec()
        } else {
            self.musica_files(root, parent)?
        };
        let mut paths = vec![base.to_owned()];
        let mut missing = false;
        // Native PAZ concatenates base, baseA..baseZ; gaps/empty volumes are invalid.
        for suffix in b'A'..=b'Z' {
            self.checkpoint()?;
            let mut name = base.as_os_str().to_owned();
            name.push((suffix as char).to_string());
            if let Some(path) = siblings
                .iter()
                .find(|p| p.as_os_str().eq_ignore_ascii_case(&name))
            {
                if missing {
                    return Ok(None);
                }
                paths.push(path.clone());
            } else {
                missing = true;
            }
        }
        let mut parts = Vec::new();
        let mut length = 0u64;
        for (index, path) in paths.iter().enumerate() {
            let full = root.join(path);
            let Some(part) = self.open(&full, 128)? else {
                return Ok(None);
            };
            if index > 0 && part.length == 0 {
                return Ok(None);
            }
            length = length
                .checked_add(part.length)
                .ok_or(DetectionError::ReadLimit)?;
            parts.push(part);
        }
        Ok(Some(Archive { parts, length }))
    }
    fn musica_bytes(
        &mut self,
        archive: &mut Archive,
        offset: u64,
        length: usize,
    ) -> DetectionResult<Vec<u8>> {
        self.checkpoint()?;
        if offset
            .checked_add(length as u64)
            .is_none_or(|end| end > archive.length)
        {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        if length as u64 > MAX_FILE_READ_BYTES || length as u64 > MAX_READ_BYTES - self.read_bytes {
            return Err(DetectionError::ReadLimit);
        }
        let mut logical = 0;
        for part in &archive.parts {
            let start = offset.max(logical);
            let end = (offset + length as u64).min(logical + part.length);
            if end > start && end - start > MAX_FILE_READ_BYTES - part.read_bytes {
                return Err(DetectionError::ReadLimit);
            }
            logical += part.length;
        }
        let mut output = vec![0; length];
        let mut logical = 0;
        let mut written = 0;
        for part in &mut archive.parts {
            self.checkpoint()?;
            let start = offset.max(logical);
            let end = (offset + length as u64).min(logical + part.length);
            if end > start {
                let size = (end - start) as usize;
                self.read_at(part, start - logical, &mut output[written..written + size])?;
                written += size;
            }
            logical += part.length;
        }
        Ok(output)
    }
}
