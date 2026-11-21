use super::*;
use shinki_emu_musica_format::ScriptEncoding;
use std::path::Component;

pub(super) type Configuration = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Clone)]
pub(super) enum SiglusRoot { Default, Append(PathBuf), Unavailable }
impl SiglusRoot {
    pub(super) fn allows(&self, path: &Path) -> bool {
        let directory = path.parent().unwrap_or(Path::new(""));
        match self {
            Self::Default => directory.as_os_str().is_empty() || directory.as_os_str().eq_ignore_ascii_case("data"),
            Self::Append(active) => directory == active,
            Self::Unavailable => false,
        }
    }
}

impl<T: DetectionTree> Scan<'_, T> {
    /// Resolve Windows-style case-insensitive resource components without links.
    pub(super) fn relative_path(&mut self, root: &Path, name: &str) -> DetectionResult<Option<PathBuf>> {
        if name.is_empty() || name.len() > 4096 || name.contains(['\\', ':', '\0']) { return Ok(None); }
        let mut result = PathBuf::new();
        let mut components = Path::new(name).components().peekable();
        while let Some(part) = components.next() {
            let Component::Normal(wanted) = part else { return Ok(None); };
            let mut found = None;
            let mut ambiguous = false;
            self.entries(&root.join(&result), |name, kind| {
                if name.eq_ignore_ascii_case(wanted) {
                    if found.is_some() || kind == EntryKind::Other || (components.peek().is_some() && kind != EntryKind::Directory) {
                        ambiguous = true;
                    }
                    found = Some(name);
                }
                Ok(())
            })?;
            let Some(found) = found.filter(|_| !ambiguous) else { return Ok(None); };
            result.push(found);
        }
        Ok(Some(result))
    }
    pub(super) fn resource_bytes(&mut self, root: &Path, path: &Path) -> DetectionResult<Vec<u8>> {
        self.checkpoint()?;
        let full = root.join(path);
        let Some(mut source) = self.open(&full, 128)? else { return Ok(Vec::new()); };
        if source.length > MAX_FILE_READ_BYTES { return Err(DetectionError::ReadLimit); }
        let length = source.length as usize;
        self.bytes(&mut source, 0, length)
    }
    pub(super) fn configuration(&mut self, root: &Path) -> DetectionResult<Option<Configuration>> {
        #[derive(Deserialize)]
        struct Receipt { schema: String, profile: Profile }
        #[derive(Deserialize)]
        struct Profile { #[serde(default)] configuration: Configuration }
        // Same precedence as shinki-emu-patch::read_output_profile.
        for name in ["shinki-patch-result.json", ".shinki/patch-result.json"] {
            let Some(path) = self.relative_path(root, name)? else { continue; };
            let bytes = self.resource_bytes(root, &path)?;
            self.checkpoint()?;
            return Ok(serde_json::from_slice::<Receipt>(&bytes).ok()
                .filter(|r| r.schema == "shinki.emu.file_patch.v1")
                .map(|r| r.profile.configuration));
        }
        Ok(None)
    }
    pub(super) fn select_root(&mut self, root: &Path, files: &[PathBuf]) -> DetectionResult<SiglusRoot> {
        let Some(path) = files.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Select.ini"))) else { return Ok(SiglusRoot::Default); };
        let bytes = self.resource_bytes(root, path)?;
        let text = std::str::from_utf8(&bytes).ok().map(std::borrow::Cow::Borrowed)
            .or_else(|| ScriptEncoding::ShiftJis.decode(&bytes));
        let Some(text) = text else { return Ok(SiglusRoot::Unavailable); };
        for raw in text.lines() {
            self.checkpoint()?;
            let line = raw.trim_end_matches('\r');
            if line.is_empty() { continue; }
            let mut columns = line.split('\t');
            let directory = columns.next().unwrap_or("");
            let _display_name = columns.next();
            if columns.next().is_some() { continue; }
            // The original engine activates the FIRST valid entry, not all appends.
            if directory.len() > 4096 { return Ok(SiglusRoot::Unavailable); }
            let name = directory.replace('\\', "/");
            if name.is_empty() { return Ok(SiglusRoot::Default); }
            let Some(path) = self.relative_path(root, &name)? else { return Ok(SiglusRoot::Unavailable); };
            if self.tree.kind(&root.join(&path))? == EntryKind::Directory { return Ok(SiglusRoot::Append(path)); }
            return Ok(SiglusRoot::Unavailable);
        }
        Ok(SiglusRoot::Default)
    }
    pub(super) fn scan_append(&mut self, root: &Path) -> DetectionResult<()> {
        if let SiglusRoot::Append(path) = self.siglus.clone() {
            let known = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
            if !matches!(known.as_str(), "data" | "data/pack" | "startdata" | "game" | "usrdir") {
                self.directory(root, &path, false)?;
            }
        }
        Ok(())
    }
}
