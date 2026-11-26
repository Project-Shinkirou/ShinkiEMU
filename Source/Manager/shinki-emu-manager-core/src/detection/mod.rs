//! Read-only engine recognition, independent of installed implementations.
//!
//! Manager recognition rules live here. Adapter probes validate only the selected
//! implementation at launch/explicit diagnosis; they are not scan fallbacks.
//! No registry, loader, executable, game script or ABI service is called here.
#![forbid(unsafe_code)]

use crate::NativeCancellation;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use thiserror::Error;

mod adv32;
mod builtin;
mod declared;
pub use declared::{DeclaredMatch, DeclaredRules};
#[cfg(test)]
mod entry_tests;
mod formats;
mod krkr;
mod musica;
#[cfg(test)]
mod musica_tests;
mod renpy;
mod roots;
mod siglus;
mod softpal;
#[cfg(test)]
mod tests;

// Change when the recognition/traversal rules change, independently of core pins.
// Future cache consumers must also invalidate on directory-content changes.
pub const CACHE_VERSION: u32 = 4;
const MAX_ENTRIES: u32 = 4096;
const MAX_FILES: u32 = 1024;
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILE_READ_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineDetection {
    pub family_id: String,
    /// Built-in resource-format identity; empty for a declaration-only match.
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<DeclaredMatch>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Other,
}

/// A read-only tree. Names emitted by `entries` are single components. Count
/// every entry, including unusable ones, against `limit` before exposing it.
/// Files must be seekable; no implementation may stage or rewrite resources.
pub trait DetectionTree {
    fn entries(
        &self,
        path: &Path,
        limit: usize,
        visit: &mut dyn FnMut(OsString, EntryKind) -> Result<(), DetectionError>,
    ) -> Result<(), DetectionError>;

    fn kind(&self, path: &Path) -> io::Result<EntryKind>;
    fn open(&self, path: &Path) -> io::Result<Option<(File, u64)>>;
}

struct LocalTree;

impl LocalTree {
    fn kind_of(ty: fs::FileType) -> EntryKind {
        if ty.is_file() {
            EntryKind::File
        } else if ty.is_dir() {
            EntryKind::Directory
        } else {
            EntryKind::Other
        }
    }
}

impl DetectionTree for LocalTree {
    fn entries(
        &self,
        path: &Path,
        limit: usize,
        visit: &mut dyn FnMut(OsString, EntryKind) -> DetectionResult<()>,
    ) -> DetectionResult<()> {
        for (index, entry) in fs::read_dir(path)?.enumerate() {
            if index == limit {
                return Err(DetectionError::ReadLimit);
            }
            let entry = entry?;
            visit(entry.file_name(), Self::kind_of(entry.file_type()?))?;
        }
        Ok(())
    }

    fn kind(&self, path: &Path) -> io::Result<EntryKind> {
        fs::symlink_metadata(path).map(|meta| Self::kind_of(meta.file_type()))
    }

    fn open(&self, path: &Path) -> io::Result<Option<(File, u64)>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() {
            return Ok(None);
        }
        File::open(path).map(|file| Some((file, metadata.len())))
    }
}

/// Inspect default layouts plus confined engine-declared resource paths.
/// Links and special files are not read.
/// Empty means no established family in this scope. Errors never return partial
/// candidates and must not be treated/cached as no-match. Results are sorted by
/// family, with at most one format per family. Runtime compatibility is separate.
/// Cancellation is cooperative between filesystem calls; blocked OS I/O cannot
/// be interrupted. Internal fixed limits bound enumeration, reads and allocation.
pub fn detect_game_directory(
    root: &Path,
    cancellation: &NativeCancellation,
) -> DetectionResult<Vec<EngineDetection>> {
    detect_tree_at(&LocalTree, root, cancellation, None).map(|(matches, _)| matches)
}

/// Inspect a provider-backed tree with exactly the same format rules and budgets.
pub fn detect_game_tree<T: DetectionTree>(
    tree: &T,
    cancellation: &NativeCancellation,
) -> DetectionResult<Vec<EngineDetection>> {
    detect_tree_at(tree, Path::new(""), cancellation, None).map(|(matches, _)| matches)
}

fn detect_tree_at<T: DetectionTree>(
    tree: &T,
    root: &Path,
    cancellation: &NativeCancellation,
    declared: Option<&DeclaredRules>,
) -> DetectionResult<(Vec<EngineDetection>, bool)> {
    let mut scan = Scan {
        tree,
        cancellation,
        matches: Vec::new(),
        entries: 0,
        files: 0,
        read_bytes: 0,
    };
    scan.checkpoint()?;
    if tree.kind(root)? != EntryKind::Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "game root must be a directory, not a link",
        )
        .into());
    }
    builtin::recognize(&mut scan, root)?;
    let has_files = scan.files != 0;
    if let Some(declared) = declared {
        declared.inspect(&mut scan, root)?;
    }
    scan.checkpoint()?;
    scan.matches.sort_by(|a, b| a.family_id.cmp(&b.family_id));
    Ok((scan.matches, has_files))
}

struct Scan<'a, T> {
    tree: &'a T,
    cancellation: &'a NativeCancellation,
    matches: Vec<EngineDetection>,
    entries: u32,
    files: u32,
    read_bytes: u64,
}
impl<T: DetectionTree> Scan<'_, T> {
    fn entries(
        &mut self,
        path: &Path,
        mut visit: impl FnMut(OsString, EntryKind) -> DetectionResult<()>,
    ) -> DetectionResult<()> {
        self.checkpoint()?;
        let limit = (MAX_ENTRIES - self.entries) as usize;
        let cancellation = self.cancellation;
        let entries = &mut self.entries;
        self.tree.entries(path, limit, &mut |name, kind| {
            if cancellation.is_cancelled() {
                return Err(DetectionError::Cancelled);
            }
            if *entries == MAX_ENTRIES {
                return Err(DetectionError::ReadLimit);
            }
            *entries += 1;
            if Path::new(&name).file_name() != Some(OsStr::new(&name)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid directory entry name",
                )
                .into());
            }
            visit(name, kind)
        })
    }

    fn count_file(&mut self) -> DetectionResult<()> {
        if self.files == MAX_FILES {
            return Err(DetectionError::ReadLimit);
        }
        self.files += 1;
        Ok(())
    }

    fn open(&self, path: &Path, read_bytes: u64) -> DetectionResult<Option<Source>> {
        self.checkpoint()?;
        Ok(self.tree.open(path)?.map(|(file, length)| Source {
            file,
            length,
            read_bytes,
        }))
    }

    fn checkpoint(&self) -> DetectionResult<()> {
        if self.cancellation.is_cancelled() {
            Err(DetectionError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn matched(&mut self, family: &str, format: &str) -> DetectionResult<()> {
        self.checkpoint()?;
        if !self.matches.iter().any(|m| m.family_id == family) {
            self.matches.push(EngineDetection {
                family_id: family.into(),
                format: format.into(),
                declared: None,
            });
        }
        Ok(())
    }
    fn read_at(
        &mut self,
        source: &mut Source,
        offset: u64,
        output: &mut [u8],
    ) -> DetectionResult<()> {
        self.checkpoint()?;
        if offset
            .checked_add(output.len() as u64)
            .is_none_or(|end| end > source.length)
        {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        if output.len() as u64 > MAX_READ_BYTES - self.read_bytes
            || output.len() as u64 > MAX_FILE_READ_BYTES - source.read_bytes
        {
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
    fn bytes(
        &mut self,
        source: &mut Source,
        offset: u64,
        length: usize,
    ) -> DetectionResult<Vec<u8>> {
        self.checkpoint()?;
        // Check declared lengths and budgets before allocation.
        if offset
            .checked_add(length as u64)
            .is_none_or(|end| end > source.length)
        {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }
        if length as u64 > MAX_READ_BYTES - self.read_bytes
            || length as u64 > MAX_FILE_READ_BYTES - source.read_bytes
        {
            return Err(DetectionError::ReadLimit);
        }
        let mut bytes = vec![0; length];
        self.read_at(source, offset, &mut bytes)?;
        Ok(bytes)
    }
}
struct Source {
    file: File,
    length: u64,
    read_bytes: u64,
}
