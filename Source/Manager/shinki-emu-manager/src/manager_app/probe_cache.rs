use super::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use shinki_emu_manager_core::{NativeCancellation, detection::{self, EngineDetection}};

const VERSION: u32 = 2;
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
const MAX_FILES: usize = 512;
const MAX_CANDIDATES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileStamp {
    path: PathBuf,
    directory: bool,
    len: u64,
    modified_ns: u128,
    created_ns: u128,
}

fn stamp(path: &Path, relative: PathBuf) -> Option<FileStamp> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return None;
    }
    Some(FileStamp {
        path: relative,
        directory: metadata.is_dir(),
        len: metadata.len(),
        modified_ns: metadata
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos(),
        created_ns: metadata
            .created()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos(),
    })
}

// A bounded metadata walk, not a discovery scan. Deep/unknown layouts deliberately
// miss the cache; directory timestamps alone cannot detect changed boot files.
fn game_stamp(root: &Path) -> Option<Vec<FileStamp>> {
    let root = fs::canonicalize(root).ok()?;
    let mut stamps = vec![stamp(&root, root.clone())?];
    let mut pending = vec![(root.clone(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        for entry in fs::read_dir(directory).ok()? {
            let entry = entry.ok()?;
            // Core-owned saves/configuration are not inputs to native game identification.
            if depth == 0 && (entry.file_name() == ".shinki" || entry.file_name() == ".shinki-musica") {
                continue;
            }
            let path = entry.path();
            let value = stamp(&path, path.strip_prefix(&root).ok()?.to_owned())?;
            if value.directory {
                if depth >= 1 {
                    return None;
                }
                pending.push((path, depth + 1));
            }
            stamps.push(value);
            if stamps.len() > MAX_FILES {
                return None;
            }
        }
    }
    stamps.sort_by(|left, right| left.path.cmp(&right.path));
    Some(stamps)
}


#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    stamp: Vec<FileStamp>,
    detections: Vec<EngineDetection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u32,
    detector_version: u32,
    entries: BTreeMap<PathBuf, Entry>,
}

#[derive(Serialize)]
struct StoredRef<'a> {
    version: u32,
    detector_version: u32,
    entries: &'a BTreeMap<PathBuf, Entry>,
}

pub(super) struct ProbeCache {
    path: PathBuf,
    entries: BTreeMap<PathBuf, Entry>,
}

impl ProbeCache {
    pub(super) fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("probe-cache.json");
        let stored = (|| {
            let file = fs::File::open(&path).ok()?;
            if file.metadata().ok()?.len() > MAX_BYTES {
                return None;
            }
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() as u64 > MAX_BYTES {
                return None;
            }
            let stored: Stored = serde_json::from_slice(&bytes).ok()?;
            if stored.version != VERSION
                || stored.detector_version != detection::CACHE_VERSION
                || stored.entries.len() > MAX_ENTRIES
                || stored.entries.values().any(|entry| {
                    entry.stamp.len() > MAX_FILES || entry.detections.len() > MAX_CANDIDATES
                })
            {
                return None;
            }
            Some(stored)
        })();
        Self {
            path,
            entries: stored.map(|stored| stored.entries).unwrap_or_default(),
        }
    }

    pub(super) fn get(&self, path: &Path) -> Option<Vec<EngineDetection>> {
        let key = fs::canonicalize(path).ok()?;
        let entry = self.entries.get(&key)?;
        if game_stamp(&key)? != entry.stamp { return None; }
        Some(entry.detections.clone())
    }

    pub(super) fn detect(
        &mut self,
        path: &Path,
        reuse: bool,
        cancellation: &NativeCancellation,
    ) -> Result<(Vec<EngineDetection>, bool), detection::DetectionError> {
        if cancellation.is_cancelled() { return Err(detection::DetectionError::Cancelled); }
        let canonical = fs::canonicalize(path)?;
        let path = canonical.as_path();
        if reuse && let Some(detections) = self.get(path) {
            if cancellation.is_cancelled() { return Err(detection::DetectionError::Cancelled); }
            return Ok((detections, true));
        }
        self.entries.remove(path);
        let before = game_stamp(path);
        let detections = detection::detect_game_directory(path, cancellation)?;
        if cancellation.is_cancelled() { return Err(detection::DetectionError::Cancelled); }
        if detections.len() <= MAX_CANDIDATES
            && let Some(stamp) = before.filter(|before| game_stamp(path).as_ref() == Some(before)) {
            if self.entries.len() >= MAX_ENTRIES { self.entries.pop_first(); }
            self.entries.insert(path.to_owned(), Entry { stamp, detections: detections.clone() });
        }
        Ok((detections, false))
    }

    pub(super) fn retain_registered(&mut self, games: &[GameRecord]) {
        let registered: BTreeSet<_> = games
            .iter()
            .filter_map(|game| fs::canonicalize(&game.location).ok())
            .collect();
        self.entries.retain(|path, _| registered.contains(path));
    }

    pub(super) fn save(&self) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or("SHINKI_EMU_PROBE_CACHE_DIRECTORY")?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "SHINKI_EMU_PROBE_CACHE_CREATE")?;
        {
            let mut writer = std::io::BufWriter::new(LimitedWriter {
                inner: &mut temporary,
                remaining: MAX_BYTES,
            });
            serde_json::to_writer(
                &mut writer,
                &StoredRef {
                    version: VERSION,
                    detector_version: detection::CACHE_VERSION,
                    entries: &self.entries,
                },
            )
            .map_err(|_| "SHINKI_EMU_PROBE_CACHE_WRITE_OR_BOUNDS")?;
            writer
                .flush()
                .map_err(|_| "SHINKI_EMU_PROBE_CACHE_WRITE_OR_BOUNDS")?;
        }
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| "SHINKI_EMU_PROBE_CACHE_SYNC")?;
        temporary
            .persist(&self.path)
            .map_err(|_| "SHINKI_EMU_PROBE_CACHE_PUBLISH")?;
        Ok(())
    }
}

struct LimitedWriter<W> {
    inner: W,
    remaining: u64,
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other("probe cache size limit"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
#[path = "probe_cache_tests.rs"]
mod tests;
