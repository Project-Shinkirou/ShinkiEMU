use super::formats::{ScanResult, range, require, u32le};
use super::*;
use shinki_emu_musica_format::ScriptEncoding;

pub(super) enum SelectedRoot {
    Default,
    Append(PathBuf),
    Unavailable,
}

impl SelectedRoot {
    pub(super) fn select<T: DetectionTree>(
        scan: &mut Scan<'_, T>,
        root: &Path,
        files: &[PathBuf],
    ) -> DetectionResult<Self> {
        let Some(path) = files.iter().find(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("Select.ini"))
        }) else {
            return Ok(Self::Default);
        };
        let bytes = scan.resource_bytes(root, path)?;
        let text = std::str::from_utf8(&bytes)
            .ok()
            .map(std::borrow::Cow::Borrowed)
            .or_else(|| ScriptEncoding::ShiftJis.decode(&bytes));
        let Some(text) = text else {
            return Ok(Self::Unavailable);
        };
        for raw in text.lines() {
            scan.checkpoint()?;
            let line = raw.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut columns = line.split('\t');
            let directory = columns.next().unwrap_or("");
            let _display_name = columns.next();
            if columns.next().is_some() {
                continue;
            }
            // The original engine activates the first valid entry, not all appends.
            if directory.len() > 4096 {
                return Ok(Self::Unavailable);
            }
            let name = directory.replace('\\', "/");
            if name.is_empty() {
                return Ok(Self::Default);
            }
            let Some(path) = scan.relative_path(root, &name)? else {
                return Ok(Self::Unavailable);
            };
            if scan.tree.kind(&root.join(&path))? == EntryKind::Directory {
                return Ok(Self::Append(path));
            }
            return Ok(Self::Unavailable);
        }
        Ok(Self::Default)
    }

    pub(super) fn append_directory(&self) -> Option<&Path> {
        match self {
            Self::Append(path) => Some(path),
            Self::Default | Self::Unavailable => None,
        }
    }

    // None leaves the file to later signature rules. A routed but rejected scene
    // candidate must not fall through to the extension and magic-less rules.
    pub(super) fn recognize<T: DetectionTree>(
        &self,
        scan: &mut Scan<'_, T>,
        path: &Path,
        source: &mut Source,
        head: &[u8],
    ) -> Option<ScanResult<()>> {
        let prefix = if head.starts_with(b"pack_scn") {
            8
        } else {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.eq_ignore_ascii_case("scene.pck") && !name.eq_ignore_ascii_case("scene.chs") {
                return None;
            }
            0
        };
        let directory = path.parent().unwrap_or(Path::new(""));
        let allowed = match self {
            Self::Default => {
                directory.as_os_str().is_empty()
                    || directory.as_os_str().eq_ignore_ascii_case("data")
            }
            Self::Append(active) => directory == active,
            Self::Unavailable => false,
        };
        Some(require(allowed).and_then(|()| validate_scene(scan, source, head, prefix)))
    }
}

fn validate_scene<T: DetectionTree>(
    scan: &mut Scan<'_, T>,
    source: &mut Source,
    head: &[u8],
    prefix: usize,
) -> ScanResult<()> {
    require(head.len() >= prefix + 92)?;
    let header = &head[prefix..];
    let size = u32le(header, 0);
    require(size >= 92 && size <= 4096 && size % 4 == 0)?;
    range(prefix as u64, size, source.length)?;
    let count = u32le(header, 56);
    require(
        count > 0
            && count == u32le(header, 64)
            && count == u32le(header, 72)
            && count == u32le(header, 80),
    )?;
    // Validate all fixed-width table ranges; UTF-16 names and payload remain encrypted.
    for (field, width) in [(4, 8), (12, 8), (28, 8), (36, 8), (52, 8), (68, 8)] {
        scan.checkpoint()?;
        let offset = u32le(header, field);
        let entries = u32le(header, field + 4);
        if entries > 0 {
            require(offset >= size + prefix as u64)?;
            range(offset, entries * width, source.length)?;
        }
    }
    let data = u32le(header, 76);
    require(data >= prefix as u64 + size && data < source.length)?;
    let indexes = scan.bytes(source, u32le(header, 68), (count * 8) as usize)?;
    for entry in indexes.chunks_exact(8) {
        scan.checkpoint()?;
        require(u32le(entry, 4) > 0)?;
        range(data + u32le(entry, 0), u32le(entry, 4), source.length)?;
    }
    scan.matched("siglus", "siglus.scene_pck")?;
    Ok(())
}
