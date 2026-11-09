//! Read-only engine recognition, independent of installed implementations.
//!
//! Manager recognition rules live here. Adapter probes validate only the selected
//! implementation at launch/explicit diagnosis; they are not scan fallbacks.
//! No registry, loader, executable, game script or ABI service is called here.
#![forbid(unsafe_code)]

use crate::NativeCancellation;
use serde::{Deserialize, Serialize};
use std::{fs::{self, File}, io::{self, Read, Seek, SeekFrom}, path::{Path, PathBuf}};
use thiserror::Error;

mod formats;
mod musica;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod musica_tests;

// Change when the recognition/traversal rules change, independently of core pins.
// Future cache consumers must also invalidate on directory-content changes.
pub const CACHE_VERSION: u32 = 1;
const MAX_ENTRIES: u32 = 4096;
const MAX_FILES: u32 = 1024;
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_READ_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineDetection {
    pub family_id: String,
    /// Resource-format identity, not an installed plugin or runtime guarantee.
    pub format: String,
}
#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("cannot read game resources: {0}")]
    Io(#[from] io::Error),
    #[error("engine recognition cancelled")]
    Cancelled,
    #[error("engine recognition read limit exceeded")]
    ReadLimit,
}
type DetectionResult<T> = Result<T, DetectionError>;

/// Inspect root files and Data, Data/pack, StartData, game and USRDIR layouts
/// (case-insensitive). Other directories, links and special files are not read.
/// Empty means no established family in this scope. Errors never return partial
/// candidates and must not be treated/cached as no-match. Results are sorted by
/// family, with at most one format per family. Runtime compatibility is separate.
/// Cancellation is cooperative between filesystem calls; blocked OS I/O cannot
/// be interrupted. Internal fixed limits bound enumeration, reads and allocation.
pub fn detect_game_directory(root: &Path, cancellation: &NativeCancellation) -> DetectionResult<Vec<EngineDetection>> {
    let mut scan = Scan { cancellation, matches: Vec::new(), entries: 0, files: 0, read_bytes: 0 };
    scan.checkpoint()?;
    let meta = fs::symlink_metadata(root)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "game root must be a directory, not a link").into());
    }
    scan.directory(root, Path::new(""), true)?;
    scan.checkpoint()?;
    scan.matches.sort_by(|a, b| a.family_id.cmp(&b.family_id));
    Ok(scan.matches)
}

struct Scan<'a> {
    cancellation: &'a NativeCancellation,
    matches: Vec<EngineDetection>,
    entries: u32,
    files: u32,
    read_bytes: u64,
}
impl Scan<'_> {
    fn checkpoint(&self) -> DetectionResult<()> {
        if self.cancellation.is_cancelled() { Err(DetectionError::Cancelled) } else { Ok(()) }
    }
    fn matched(&mut self, family: &str, format: &str) -> DetectionResult<()> {
        self.checkpoint()?;
        if !self.matches.iter().any(|m| m.family_id == family) {
            self.matches.push(EngineDetection { family_id: family.into(), format: format.into() });
        }
        Ok(())
    }
    fn directory(&mut self, root: &Path, relative: &Path, root_directory: bool) -> DetectionResult<()> {
        self.checkpoint()?;
        let entries = fs::read_dir(root.join(relative))?;
        let mut children = Vec::new();
        let mut files = Vec::new();
        for entry in entries {
            self.checkpoint()?;
            if self.entries == MAX_ENTRIES { return Err(DetectionError::ReadLimit); }
            self.entries += 1;
            let entry = entry?;
            let path = relative.join(entry.file_name());
            self.checkpoint()?;
            let ty = entry.file_type()?;
            if ty.is_symlink() { continue; }
            if ty.is_file() { files.push(path); }
            else if ty.is_dir() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if (root_directory && matches!(name.as_str(), "data" | "startdata" | "game" | "usrdir"))
                    || (relative.as_os_str().to_string_lossy().eq_ignore_ascii_case("data") && name == "pack") { children.push(path); }
            }
        }
        files.sort();
        children.sort();
        for path in &files {
            self.checkpoint()?;
            if self.files == MAX_FILES { return Err(DetectionError::ReadLimit); }
            self.files += 1;
            self.inspect(root, &path)?;
        }
        if root_directory { self.musica(root, &files)?; }
        for path in children { self.directory(root, &path, false)?; }
        Ok(())
    }
    fn inspect(&mut self, root: &Path, path: &Path) -> DetectionResult<()> {
        self.checkpoint()?;
        let metadata = fs::symlink_metadata(root.join(path))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() { return Ok(()); }
        self.checkpoint()?;
        let file = File::open(root.join(path))?;
        let mut source = Source { file, length: metadata.len(), read_bytes: 0 };
        let mut head = [0; 128];
        let size = source.length.min(head.len() as u64) as usize;
        self.read_at(&mut source, 0, &mut head[..size])?;
        match self.formats(path, &mut source, &head[..size]) {
            Ok(()) | Err(formats::Rejected::NoMatch) => Ok(()),
            Err(formats::Rejected::Abort(error)) => Err(error),
        }
    }
    fn read_at(&mut self, source: &mut Source, offset: u64, output: &mut [u8]) -> DetectionResult<()> {
        self.checkpoint()?;
        if offset.checked_add(output.len() as u64).is_none_or(|end| end > source.length) {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        if output.len() as u64 > MAX_READ_BYTES - self.read_bytes || output.len() as u64 > MAX_FILE_READ_BYTES - source.read_bytes {
            return Err(DetectionError::ReadLimit);
        }
        source.file.seek(SeekFrom::Start(offset))?;
        let mut done = 0;
        while done < output.len() {
            self.checkpoint()?;
            let count = match source.file.read(&mut output[done..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                Ok(count) => count,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            source.read_bytes += count as u64;
            self.read_bytes += count as u64;
            done += count;
        }
        self.checkpoint()
    }
    fn bytes(&mut self, source: &mut Source, offset: u64, length: usize) -> DetectionResult<Vec<u8>> {
        self.checkpoint()?;
        // Check declared lengths and budgets before allocation.
        if offset.checked_add(length as u64).is_none_or(|end| end > source.length) {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        if length as u64 > MAX_READ_BYTES - self.read_bytes || length as u64 > MAX_FILE_READ_BYTES - source.read_bytes {
            return Err(DetectionError::ReadLimit);
        }
        let mut bytes = vec![0; length];
        self.read_at(source, offset, &mut bytes)?;
        Ok(bytes)
    }
}
struct Source { file: File, length: u64, read_bytes: u64 }
