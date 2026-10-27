use super::*;

impl CmvsArchive {
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn stat(&self, uri: &str) -> Result<ArchiveStat, CoreError> {
        if let Some(path) = self.native_file(uri)? {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::File,
                size: native::native_size(&path)?,
                archive_role: None,
                method: Some("native-file".into()),
            });
        }
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: Some(format!("loose-{role}")),
                kind: ArchiveNodeKind::File,
                size: source.byte_size,
                archive_role: Some(format!("loose-{role}")),
                method: Some("loose".into()),
            });
        }
        if uri == format!("{}loose", self.prefix) && !self.loose.is_empty() {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        if uri == self.prefix
            || self
                .entries
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{}/", uri.trim_end_matches('/'))))
        {
            return Ok(ArchiveStat {
                uri: uri.into(),
                entry_id: None,
                kind: ArchiveNodeKind::Directory,
                size: 0,
                archive_role: None,
                method: None,
            });
        }
        let entry = self.entry(uri)?;
        Ok(ArchiveStat {
            uri: uri.into(),
            entry_id: Some(entry.entry_id.clone()),
            kind: ArchiveNodeKind::File,
            size: entry.descriptor.stored_size,
            archive_role: Some(self.archives[entry.archive].role.clone()),
            method: Some("cpz5-encrypted".into()),
        })
    }

    pub fn read_range(
        &self,
        uri: &str,
        offset: u64,
        length: u64,
    ) -> Result<ArchiveReadResult, CoreError> {
        if length > ARCHIVE_MAX_READ_BYTES {
            return Err(invalid(
                "SHINKI_EMU_VFS_READ_LIMIT",
                "VFS read exceeds the bounded range limit",
            ));
        }
        if let Some(path) = self.native_file(uri)? {
            let size = native::native_size(&path)?;
            let bytes = native::read_native_range(&path, offset, length)?;
            return Ok(ArchiveReadResult {
                uri: uri.into(),
                offset,
                bytes: bytes.into(),
                eof: offset + length == size,
                cache_hit: false,
            });
        }
        if let Some(role) = self.loose_role_from_uri(uri) {
            let source = &self.loose[&role];
            if offset > source.byte_size
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > source.byte_size)
            {
                return Err(invalid(
                    "SHINKI_EMU_VFS_READ_RANGE",
                    "VFS range is outside the loose resource",
                ));
            }
            let bytes = self.read_loose_range(source, offset, length)?;
            let eof = offset + length == source.byte_size;
            return Ok(ArchiveReadResult {
                uri: uri.into(),
                offset,
                bytes: bytes.into(),
                eof,
                cache_hit: false,
            });
        }
        let entry = self.entry(uri)?;
        if offset > entry.descriptor.stored_size
            || offset
                .checked_add(length)
                .is_none_or(|end| end > entry.descriptor.stored_size)
        {
            return Err(invalid(
                "SHINKI_EMU_VFS_READ_RANGE",
                "VFS range is outside the entry",
            ));
        }
        let (decoded, cache_hit) = self.decode_entry(entry)?;
        let start = usize::try_from(offset).map_err(|_| {
            invalid(
                "SHINKI_EMU_VFS_READ_RANGE",
                "VFS range exceeds platform bounds",
            )
        })?;
        let end = start
            .checked_add(usize::try_from(length).map_err(|_| {
                invalid(
                    "SHINKI_EMU_VFS_READ_RANGE",
                    "VFS range exceeds platform bounds",
                )
            })?)
            .ok_or_else(|| invalid("SHINKI_EMU_VFS_READ_RANGE", "VFS range overflowed"))?;
        let eof = end == decoded.len();
        let bytes = OwnedByteBuffer::from_owner((decoded, start..end), |(owner, range)| {
            &owner[range.clone()]
        });
        Ok(ArchiveReadResult {
            uri: uri.into(),
            offset,
            bytes,
            eof,
            cache_hit,
        })
    }
}
