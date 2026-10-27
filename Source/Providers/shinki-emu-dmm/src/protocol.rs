use md5::{Digest, Md5};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(Clone, Debug)]
pub struct Manifest {
    pub domain: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Clone, Debug)]
pub struct ManifestFile {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub size: u64,
    pub hash: String,
    pub check_hash: bool,
    pub force_delete: bool,
}

#[derive(Deserialize)]
struct FileListEnvelope {
    data: FileListData,
}
#[derive(Deserialize)]
struct FileListData {
    domain: String,
    file_list: Vec<FileEntry>,
}
#[derive(Deserialize)]
struct FileEntry {
    local_path: String,
    path: String,
    size: u64,
    hash: String,
    check_hash_flg: bool,
    force_delete_flg: bool,
}

pub fn parse_file_list(body: &[u8]) -> Result<Manifest, serde_json::Error> {
    let envelope: FileListEnvelope = serde_json::from_slice(body)?;
    let files = envelope
        .data
        .file_list
        .into_iter()
        .map(|file| ManifestFile {
            local_path: PathBuf::from(file.local_path.trim_start_matches(['/', '\\'])),
            remote_path: file.path,
            size: file.size,
            hash: file.hash,
            check_hash: file.check_hash_flg,
            force_delete: file.force_delete_flg,
        })
        .collect();
    Ok(Manifest {
        domain: envelope.data.domain,
        files,
    })
}

pub fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
pub async fn needs_transfer(path: &Path, file: &ManifestFile) -> bool {
    file.force_delete || !hash_matches_async(path, file).await
}
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub async fn hash_matches_async(path: &Path, file: &ManifestFile) -> bool {
    let Ok(mut input) = tokio::fs::File::open(path).await else {
        return false;
    };
    if input.metadata().await.map(|meta| meta.len()).ok() != Some(file.size) {
        return false;
    }
    let mut digest = Md5::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let Ok(size) = input.read(&mut buffer).await else {
            return false;
        };
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    hex_digest(digest.finalize().as_slice()).eq_ignore_ascii_case(&file.hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    // This fixture uses only the field names/MD5 semantics seen in DGPSessionV2.download;
    // a redacted real DGP response is still required to confirm flag encodings.
    const RESPONSE: &[u8] = br#"{"data":{"domain":"https://cdn-gameplayer.games.dmm.com/product/x","file_list":[{"local_path":"/data/a.bin","path":"a","size":3,"hash":"900150983cd24fb0d6963f7d28e17f72","check_hash_flg":true,"force_delete_flg":false},{"local_path":"/data/b.bin","path":"b","size":2,"hash":"","check_hash_flg":false,"force_delete_flg":true}]}}"#;

    #[tokio::test]
    async fn unchanged_files_are_not_transferred_and_deleted_files_are_processed() {
        let manifest = parse_file_list(RESPONSE).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/a.bin");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"abc").await.unwrap();
        assert!(!needs_transfer(&path, &manifest.files[0]).await);
        assert!(needs_transfer(&temp.path().join("data/b.bin"), &manifest.files[1]).await);
        tokio::fs::write(&path, b"bad").await.unwrap();
        assert!(needs_transfer(&path, &manifest.files[0]).await);
    }

    #[test]
    fn rejects_traversal_paths() {
        assert!(!safe_relative(Path::new("../save")));
        assert!(safe_relative(Path::new("data/save")));
    }

    #[tokio::test]
    async fn hash_mismatch_is_not_current() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("x");
        tokio::fs::write(&path, b"bad").await.unwrap();
        let file = ManifestFile {
            local_path: PathBuf::from("x"),
            remote_path: "x".into(),
            size: 3,
            hash: "900150983cd24fb0d6963f7d28e17f72".into(),
            check_hash: true,
            force_delete: false,
        };
        assert!(!hash_matches_async(&path, &file).await);
    }
}
