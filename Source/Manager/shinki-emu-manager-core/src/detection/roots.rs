use super::*;
use std::path::Component;

impl<T: DetectionTree> Scan<'_, T> {
    /// Resolve Windows-style case-insensitive resource components without links.
    pub(super) fn relative_path(
        &mut self,
        root: &Path,
        name: &str,
    ) -> DetectionResult<Option<PathBuf>> {
        if name.is_empty() || name.len() > 4096 || name.contains(['\\', ':', '\0']) {
            return Ok(None);
        }
        let mut result = PathBuf::new();
        let mut components = Path::new(name).components().peekable();
        while let Some(part) = components.next() {
            let Component::Normal(wanted) = part else {
                return Ok(None);
            };
            let mut found = None;
            let mut ambiguous = false;
            self.entries(&root.join(&result), |name, kind| {
                if name.eq_ignore_ascii_case(wanted) {
                    if found.is_some()
                        || kind == EntryKind::Other
                        || (components.peek().is_some() && kind != EntryKind::Directory)
                    {
                        ambiguous = true;
                    }
                    found = Some(name);
                }
                Ok(())
            })?;
            let Some(found) = found.filter(|_| !ambiguous) else {
                return Ok(None);
            };
            result.push(found);
        }
        Ok(Some(result))
    }
    pub(super) fn resource_bytes(&mut self, root: &Path, path: &Path) -> DetectionResult<Vec<u8>> {
        self.checkpoint()?;
        let full = root.join(path);
        let Some(mut source) = self.open(&full, 128)? else {
            return Ok(Vec::new());
        };
        if source.length > MAX_FILE_READ_BYTES {
            return Err(DetectionError::ReadLimit);
        }
        let length = source.length as usize;
        self.bytes(&mut source, 0, length)
    }
}
