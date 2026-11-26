//! Read-only Android DocumentsContract access. Opaque document IDs are never local paths.
use super::android::call_activity;
use jni::{
    jni_sig, jni_str,
    objects::{JString, JValue},
    strings::JNIStr,
};
use serde::Deserialize;
use shinki_emu_manager_core::detection::{DetectionError, DetectionTree, EntryKind};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{self, Seek},
    os::fd::FromRawFd,
    path::{Path, PathBuf},
};

pub fn is_library_uri(value: &str) -> bool {
    value.starts_with("content://")
}

/// Documents providers promise stable document IDs within their authority. Do not use
/// the grant's tree ID here: choosing a parent and then a child must not duplicate a title.
pub fn document_identity(uri: &str) -> &str {
    // Retain the full grant for access, but use the final /document segment
    // separately with the authority when keying in the scanner.
    uri.rsplit_once("/document/").map_or(uri, |(_, id)| id)
}

pub fn authority(uri: &str) -> &str {
    uri.strip_prefix("content://")
        .and_then(|uri| uri.split_once('/'))
        .map_or("", |(authority, _)| authority)
}

#[derive(Deserialize)]
#[serde(from = "(String, String, bool)")]
pub struct Document {
    pub uri: String,
    pub name: String,
    pub directory: bool,
}

impl From<(String, String, bool)> for Document {
    fn from((uri, name, directory): (String, String, bool)) -> Self {
        Self {
            uri,
            name,
            directory,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Query {
    Rows { rows: Vec<Document> },
    Error { kind: String, error: String },
}

fn query(uri: &str, children: Option<(usize, bool)>) -> Result<Vec<Document>, DetectionError> {
    let name = if children.is_some() {
        "libraryTreeChildren"
    } else {
        "libraryTreeDocument"
    };
    let json = call_activity(name, |env, activity| {
        let uri = env.new_string(uri)?;
        let value = if let Some((limit, directories_only)) = children {
            env.call_method(
                activity,
                jni_str!("libraryTreeChildren"),
                jni_sig!("(Ljava/lang/String;IZ)Ljava/lang/String;"),
                &[
                    JValue::Object(&uri),
                    JValue::Int(limit as i32),
                    JValue::Bool(directories_only),
                ],
            )?
        } else {
            env.call_method(
                activity,
                jni_str!("libraryTreeDocument"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&uri)],
            )?
        };
        env.cast_local::<JString>(value.l()?)?.try_to_string(env)
    })
    .map_err(io::Error::other)?;
    match serde_json::from_str(&json).map_err(io::Error::other)? {
        Query::Rows { rows } => Ok(rows),
        Query::Error { kind, .. } if kind == "bounds" => Err(DetectionError::ReadLimit),
        Query::Error { kind, error } => Err(io::Error::new(
            match kind.as_str() {
                "permission" => io::ErrorKind::PermissionDenied,
                "missing" => io::ErrorKind::NotFound,
                _ => io::ErrorKind::Other,
            },
            error,
        )
        .into()),
    }
}

pub fn document(uri: &str) -> Result<Document, DetectionError> {
    query(uri, None)?.pop().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "document provider returned no folder",
        )
        .into()
    })
}

pub fn child_directories(uri: &str, limit: usize) -> Result<Vec<Document>, DetectionError> {
    query(uri, Some((limit, true)))
}

pub fn local_path(uri: &str) -> Result<Option<PathBuf>, String> {
    let method: &JNIStr = jni_str!("localLibraryTreePath");
    call_activity("localLibraryTreePath", |env, activity| {
        let uri = env.new_string(uri)?;
        let result = env
            .call_method(
                activity,
                method,
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&uri)],
            )?
            .l()?;
        if result.as_raw().is_null() {
            return Ok(None);
        }
        env.cast_local::<JString>(result)?
            .try_to_string(env)
            .map(PathBuf::from)
            .map(Some)
    })
}

fn open_file(uri: &str) -> io::Result<(File, u64)> {
    let fd = call_activity("openLibraryTreeFile", |env, activity| {
        let uri = env.new_string(uri)?;
        env.call_method(
            activity,
            jni_str!("openLibraryTreeFile"),
            jni_sig!("(Ljava/lang/String;)I"),
            &[JValue::Object(&uri)],
        )?
        .i()
    })
    .map_err(io::Error::other)?;
    if fd < 0 {
        let (kind, detail) = match fd {
            -2 => (
                io::ErrorKind::PermissionDenied,
                "folder read permission was revoked; select it again",
            ),
            -3 => (io::ErrorKind::NotFound, "document is unavailable"),
            _ => (
                io::ErrorKind::Other,
                "document provider could not open file",
            ),
        };
        return Err(io::Error::new(kind, detail));
    }
    // SAFETY: Kotlin transfers ownership from ParcelFileDescriptor.detachFd, exactly once.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let length = file.seek(io::SeekFrom::End(0)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("document provider file is not seekable: {error}"),
        )
    })?;
    Ok((file, length))
}

pub struct SafTree<'a> {
    root: &'a Document,
    entries: RefCell<BTreeMap<PathBuf, Vec<Document>>>,
}

impl<'a> SafTree<'a> {
    pub fn new(root: &'a Document) -> Self {
        Self {
            root,
            entries: RefCell::new(BTreeMap::new()),
        }
    }

    fn lookup<T>(&self, path: &Path, use_document: impl FnOnce(&Document) -> T) -> io::Result<T> {
        if path.as_os_str().is_empty() {
            return Ok(use_document(self.root));
        }
        let missing = || io::Error::new(io::ErrorKind::NotFound, "document is unavailable");
        let name = path.file_name().ok_or_else(missing)?;
        let parent = path.parent().ok_or_else(missing)?;
        let cache = self.entries.borrow();
        let entry = cache
            .get(parent)
            .and_then(|children| children.iter().find(|child| name == child.name.as_str()))
            .ok_or_else(missing)?;
        Ok(use_document(entry))
    }
}

impl DetectionTree for SafTree<'_> {
    fn entries(
        &self,
        path: &Path,
        limit: usize,
        visit: &mut dyn FnMut(OsString, EntryKind) -> Result<(), DetectionError>,
    ) -> Result<(), DetectionError> {
        if !self.entries.borrow().contains_key(path) {
            let rows = self.lookup(path, |doc| query(&doc.uri, Some((limit, false))))??;
            let mut names = std::collections::BTreeSet::new();
            if rows.iter().any(|row| !names.insert(&row.name)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "document provider returned duplicate names",
                )
                .into());
            }
            self.entries.borrow_mut().insert(path.to_owned(), rows);
        }
        let cache = self.entries.borrow();
        let rows = &cache[path];
        if rows.len() > limit {
            return Err(DetectionError::ReadLimit);
        }
        for row in rows {
            visit(
                OsString::from(&row.name),
                if row.directory {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
            )?;
        }
        Ok(())
    }

    fn kind(&self, path: &Path) -> io::Result<EntryKind> {
        self.lookup(path, |row| {
            if row.directory {
                EntryKind::Directory
            } else {
                EntryKind::File
            }
        })
    }

    fn open(&self, path: &Path) -> io::Result<Option<(File, u64)>> {
        self.lookup(path, |row| {
            if row.directory {
                Ok(None)
            } else {
                open_file(&row.uri).map(Some)
            }
        })?
    }
}
