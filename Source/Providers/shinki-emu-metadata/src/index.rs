use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::is_sha256_hex;
use crate::MetadataError;

pub const METADATA_INDEX_SCHEMA_VERSION: &str = "shinki.emu.metadata.index.v1";
pub const METADATA_INDEX_FILE: &str = "index.json";
pub const MAX_METADATA_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_METADATA_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INDEXED_FILES: usize = 4096;
const MAX_PATH_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetadataIndex {
    #[schemars(regex(pattern = r"^shinki\.emu\.metadata\.index\.v1$"))]
    pub schema: String,
    #[schemars(range(min = 0))]
    pub generated_at_unix_ms: i64,
    #[schemars(regex(pattern = r"^([0-9a-f]{40}|[0-9a-f]{64})$"))]
    pub source_commit: String,
    pub files: Vec<IndexedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexedFile {
    #[schemars(
        regex(pattern = r"^(?!index\.json$)(?!.*\.\.)[A-Za-z0-9._+-]+(/[A-Za-z0-9._+-]+)*$"),
        length(max = 256)
    )]
    pub path: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(range(max = 8388608))]
    pub size: u64,
}

impl MetadataIndex {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != METADATA_INDEX_SCHEMA_VERSION {
            return Err(MetadataError::SchemaMismatch("index.schema"));
        }
        if self.generated_at_unix_ms < 0 {
            return Err(MetadataError::SchemaMismatch("index.generated_at_unix_ms"));
        }
        if !is_commit_id(&self.source_commit) {
            return Err(MetadataError::SchemaMismatch("index.source_commit"));
        }
        if self.files.len() > MAX_INDEXED_FILES {
            return Err(MetadataError::ResponseBounds("index.files"));
        }
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for file in &self.files {
            if !is_metadata_path(&file.path) {
                return Err(MetadataError::SchemaMismatch("index.path"));
            }
            if !is_sha256_hex(&file.sha256) {
                return Err(MetadataError::SchemaMismatch("index.sha256"));
            }
            if file.size > MAX_METADATA_FILE_BYTES {
                return Err(MetadataError::ResponseBounds("index.size"));
            }
            total += file.size;
            if total > MAX_METADATA_TOTAL_BYTES {
                return Err(MetadataError::ResponseBounds("index.total_size"));
            }
            if !paths.insert(file.path.to_ascii_lowercase()) {
                return Err(MetadataError::Duplicate(file.path.clone()));
            }
        }
        Ok(())
    }

    pub fn file(&self, path: &str) -> Option<&IndexedFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

pub fn parse_metadata_index(bytes: &[u8]) -> Result<MetadataIndex, MetadataError> {
    if bytes.len() as u64 > MAX_METADATA_FILE_BYTES {
        return Err(MetadataError::ResponseBounds("index"));
    }
    let index: MetadataIndex = serde_json::from_value(
        crate::parse_unique_json(bytes).map_err(|_| MetadataError::SchemaMismatch("index.json"))?,
    )
    .map_err(|_| MetadataError::SchemaMismatch("index.json"))?;
    index.validate()?;
    Ok(index)
}

fn is_commit_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn is_metadata_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && path != METADATA_INDEX_FILE
        && !path.contains("..")
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment.bytes().all(|byte| {
                    byte.is_ascii_alphabetic()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'.' | b'_' | b'-' | b'+')
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(files: Vec<IndexedFile>) -> MetadataIndex {
        MetadataIndex {
            schema: METADATA_INDEX_SCHEMA_VERSION.into(),
            generated_at_unix_ms: 1_700_000_000_000,
            source_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            files,
        }
    }

    fn file(path: &str, size: u64) -> IndexedFile {
        IndexedFile {
            path: path.into(),
            sha256: "a".repeat(64),
            size,
        }
    }

    #[test]
    fn preserves_case_sensitive_core_paths_without_allowing_filesystem_collisions() {
        let source = index(vec![file(
            "engines/cores/ReMinori/windows-x86_64/1.0.0.json",
            10,
        )]);
        let parsed = parse_metadata_index(&serde_json::to_vec(&source).unwrap()).unwrap();
        assert!(parsed
            .file("engines/cores/ReMinori/windows-x86_64/1.0.0.json")
            .is_some());
        assert!(parsed
            .file("engines/cores/reminori/windows-x86_64/1.0.0.json")
            .is_none());
        assert!(matches!(
            index(vec![file("patches/a.luau", 1), file("patches/A.luau", 1)]).validate(),
            Err(MetadataError::Duplicate(_))
        ));
    }

    #[test]
    fn rejects_unsafe_paths() {
        for path in [
            "",
            "../index.json",
            "patches/../x.json",
            "/abs.json",
            "a//b.json",
            "a/./b.json",
            "trailing/",
            "back\\slash.json",
            "space name.json",
            "index.json",
        ] {
            assert!(
                matches!(
                    index(vec![file(path, 1)]).validate(),
                    Err(MetadataError::SchemaMismatch("index.path"))
                ),
                "{path}"
            );
        }
    }

    #[test]
    fn enforces_size_limits_and_unique_paths() {
        assert!(matches!(
            index(vec![file("a.json", MAX_METADATA_FILE_BYTES + 1)]).validate(),
            Err(MetadataError::ResponseBounds("index.size"))
        ));
        let files = (0..9)
            .map(|n| file(&format!("f{n}.json"), MAX_METADATA_FILE_BYTES))
            .collect();
        assert!(matches!(
            index(files).validate(),
            Err(MetadataError::ResponseBounds("index.total_size"))
        ));
        assert!(matches!(
            index(vec![file("a.json", 1), file("a.json", 2)]).validate(),
            Err(MetadataError::Duplicate(_))
        ));
    }

    #[test]
    fn rejects_bad_schema_commit_and_hash() {
        let mut bad = index(vec![]);
        bad.schema = "shinki.emu.metadata.index.v0".into();
        assert!(bad.validate().is_err());
        let mut bad = index(vec![]);
        bad.source_commit = "HEAD".into();
        assert!(bad.validate().is_err());
        let mut entry = file("a.json", 1);
        entry.sha256 = "A".repeat(64);
        assert!(index(vec![entry]).validate().is_err());
    }
}
