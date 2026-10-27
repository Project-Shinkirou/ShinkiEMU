use crate::protocol::{Manifest, ManifestFile, hash_matches_async, needs_transfer, safe_relative};
use futures_util::StreamExt;
use reqwest::{
    Client, StatusCode, Url,
    header::{CONTENT_RANGE, RANGE},
};
use shinki_emu_store::{
    CancelToken, DownloadPlan, InstallNotice, Progress, ProgressUpdates, StoreError,
    TransferSummary,
};
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub(crate) struct DmmPlan {
    pub manifest: Manifest,
}

pub(crate) fn signed_query(sign: &str) -> Vec<(String, String)> {
    let query = sign.replace(';', "&");
    let Ok(url) = Url::parse(&format!("https://cdn-gameplayer.games.dmm.com/?{query}")) else {
        return Vec::new();
    };
    url.query_pairs()
        .filter_map(|(key, value)| {
            let public_key = match key.as_ref() {
                "CloudFront-Policy" => "Policy",
                "CloudFront-Signature" => "Signature",
                "CloudFront-Key-Pair-Id" => "Key-Pair-Id",
                _ => return None,
            };
            (!value.is_empty()).then(|| (public_key.to_owned(), value.into_owned()))
        })
        .collect()
}

pub(crate) async fn download(
    client: &Client,
    plan: &DownloadPlan,
    query: &[(String, String)],
    dest: &Path,
    work_dir: &Path,
    progress: ProgressUpdates,
    cancel: CancelToken,
) -> Result<TransferSummary, StoreError> {
    let Some(payload) = plan.payload.downcast_ref::<DmmPlan>() else {
        return Err(StoreError::Metadata);
    };
    let domain = Url::parse(&payload.manifest.domain).map_err(|_| StoreError::Metadata)?;
    if domain.scheme() != "https"
        || domain.host_str() != Some("cdn-gameplayer.games.dmm.com")
        || domain.query().is_some()
        || domain.username() != ""
        || domain.password().is_some()
        || query.len() != 3
    {
        return Err(StoreError::Metadata);
    }
    for file in &payload.manifest.files {
        if !safe_relative(&file.local_path) {
            return Err(StoreError::Metadata);
        }
        if !file.force_delete
            && (!file.check_hash
                || file.hash.len() != 32
                || !file.hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(StoreError::Metadata);
        }
    }
    tokio::fs::create_dir_all(dest).await?;
    tokio::fs::create_dir_all(work_dir).await?;
    let files_total = payload.manifest.files.len() as u64;
    let bytes_total: u64 = payload
        .manifest
        .files
        .iter()
        .filter(|file| !file.force_delete)
        .map(|file| file.size)
        .sum();
    let mut summary = TransferSummary::default();
    let mut bytes_done = 0_u64;
    for file in &payload.manifest.files {
        if cancel.is_cancelled() {
            summary.cancelled = true;
            return Ok(summary);
        }
        check_no_symlinks(dest, &file.local_path).await?;
        let final_path = dest.join(&file.local_path);
        if file.force_delete {
            if tokio::fs::try_exists(&final_path).await? {
                tokio::fs::remove_file(&final_path).await?;
            }
            summary.files_completed += 1;
            emit_progress(
                &progress,
                bytes_done,
                bytes_total,
                summary.files_completed + summary.files_skipped,
                files_total,
            );
            continue;
        }
        if !needs_transfer(&final_path, file).await {
            bytes_done += file.size;
            summary.files_skipped += 1;
        } else {
            let downloaded =
                match download_one(client, &domain, file, query, dest, work_dir, &cancel).await {
                    Ok(bytes) => bytes,
                    Err(StoreError::Cancelled) => {
                        summary.cancelled = true;
                        return Ok(summary);
                    }
                    Err(error) => return Err(error),
                };
            bytes_done += file.size;
            summary.bytes_downloaded += downloaded;
            summary.files_completed += 1;
        }
        if file
            .local_path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ["zip", "7z", "rar"]
                    .iter()
                    .any(|archive| ext.eq_ignore_ascii_case(archive))
            })
        {
            summary.notice = Some(InstallNotice::ManualExtractionRequired);
        }
        emit_progress(
            &progress,
            bytes_done,
            bytes_total,
            summary.files_completed + summary.files_skipped,
            files_total,
        );
    }
    Ok(summary)
}

async fn check_no_symlinks(root: &Path, relative: &Path) -> Result<(), StoreError> {
    let mut cursor = root.to_path_buf();
    for part in relative.components() {
        cursor.push(part.as_os_str());
        match tokio::fs::symlink_metadata(&cursor).await {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(StoreError::Metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StoreError::Io),
            Ok(_) => {}
        }
    }
    Ok(())
}

fn emit_progress(
    progress: &ProgressUpdates,
    done: u64,
    total: u64,
    files_done: u64,
    files_total: u64,
) {
    progress(Progress {
        bytes_done: done,
        bytes_total: total,
        files_done,
        files_total,
        ..Progress::default()
    });
}

async fn download_one(
    client: &Client,
    domain: &Url,
    file: &ManifestFile,
    query: &[(String, String)],
    dest: &Path,
    work_dir: &Path,
    cancel: &CancelToken,
) -> Result<u64, StoreError> {
    let remote = file.remote_path.trim_start_matches('/');
    if remote.is_empty()
        || remote.contains(['\\', '?', '#'])
        || remote
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(StoreError::Metadata);
    }
    let mut prefix = domain.clone();
    if !prefix.path().ends_with('/') {
        prefix.set_path(&format!("{}/", prefix.path()));
    }
    let mut url = prefix.join(remote).map_err(|_| StoreError::Metadata)?;
    if url.scheme() != "https" || url.host_str() != domain.host_str() {
        return Err(StoreError::Metadata);
    }
    url.query_pairs_mut().extend_pairs(
        query
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let final_path = dest.join(&file.local_path);
    let mut part_path = work_dir.join(&file.local_path);
    let name = part_path
        .file_name()
        .ok_or(StoreError::Metadata)?
        .to_string_lossy()
        .into_owned();
    part_path.set_file_name(format!("{name}.part"));
    check_no_symlinks(work_dir, &file.local_path).await?;
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Some(parent) = part_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut total_downloaded = 0_u64;
    for attempt in 0..3 {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        let mut existing = match tokio::fs::metadata(&part_path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(_) => return Err(StoreError::Io),
        };
        if existing > 0 && hash_matches_async(&part_path, file).await {
            tokio::fs::rename(&part_path, &final_path).await?;
            return Ok(0);
        }
        if existing >= file.size && existing > 0 {
            tokio::fs::remove_file(&part_path).await?;
            existing = 0;
        }
        let mut request = client.get(url.clone());
        if existing > 0 {
            request = request.header(RANGE, format!("bytes={existing}-"));
        }
        let response = tokio::select! {
            result = request.send() => result.map_err(map_request_error)?,
            () = cancel.cancelled() => return Err(StoreError::Cancelled),
        };
        if !matches!(
            response.status(),
            StatusCode::OK | StatusCode::PARTIAL_CONTENT
        ) {
            return Err(map_status(response.status()));
        }
        let partial = response.status() == StatusCode::PARTIAL_CONTENT;
        if partial {
            let range = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .ok_or(StoreError::Integrity)?;
            if !valid_content_range(range, existing, file.size) {
                return Err(StoreError::Integrity);
            }
        }
        let mut output = if partial && existing > 0 {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part_path)
                .await?
        } else {
            tokio::fs::File::create(&part_path).await?
        };
        let mut streamed = response.bytes_stream();
        // Include bytes from failed hash attempts in the transfer summary.
        loop {
            let next = tokio::select! {
                result = streamed.next() => result,
                () = cancel.cancelled() => return Err(StoreError::Cancelled),
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(map_request_error)?;
            total_downloaded += chunk.len() as u64;
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        drop(output);
        if verify_or_retry(&part_path, file, attempt).await? {
            tokio::fs::rename(&part_path, &final_path).await?;
            return Ok(total_downloaded);
        }
    }
    Err(StoreError::Integrity)
}

async fn verify_or_retry(
    path: &Path,
    file: &ManifestFile,
    attempt: u8,
) -> Result<bool, StoreError> {
    if hash_matches_async(path, file).await {
        return Ok(true);
    }
    tokio::fs::remove_file(path).await?;
    if attempt == 2 {
        Err(StoreError::Integrity)
    } else {
        Ok(false)
    }
}

fn valid_content_range(header: &str, offset: u64, size: u64) -> bool {
    let Some((range, total)) = header
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
    else {
        return false;
    };
    let Some((start, end)) = range.split_once('-') else {
        return false;
    };
    match (
        start.parse::<u64>(),
        end.parse::<u64>(),
        total.parse::<u64>(),
    ) {
        (Ok(start), Ok(end), Ok(total)) => {
            start == offset && end >= start && end < total && total == size
        }
        _ => false,
    }
}

fn map_request_error(error: reqwest::Error) -> StoreError {
    if error.is_timeout() {
        StoreError::Timeout
    } else {
        StoreError::Network
    }
}
fn map_status(status: StatusCode) -> StoreError {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        StoreError::AccessDenied
    } else {
        StoreError::TransferFailed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_cdn_query_uses_cloudfront_public_names() {
        let query = signed_query(
            "CloudFront-Policy=p%2Bq;CloudFront-Signature=s;CloudFront-Key-Pair-Id=k;secret=x",
        );
        assert_eq!(
            query,
            vec![
                ("Policy".into(), "p+q".into()),
                ("Signature".into(), "s".into()),
                ("Key-Pair-Id".into(), "k".into())
            ]
        );
    }
    #[test]
    fn resumed_response_requires_exact_range_start_and_total() {
        assert!(valid_content_range("bytes 3-5/6", 3, 6));
        assert!(!valid_content_range("bytes 2-5/6", 3, 6));
        assert!(!valid_content_range("bytes 3-5/7", 3, 6));
        assert!(!valid_content_range("bytes 3-8/6", 3, 6));
    }

    #[tokio::test]
    async fn checksum_mismatch_removes_partial_and_fails_after_three_attempts() {
        let temp = tempfile::tempdir().unwrap();
        let part = temp.path().join("game.dat.part");
        let file = ManifestFile {
            local_path: "game.dat".into(),
            remote_path: "game.dat".into(),
            size: 3,
            hash: "900150983cd24fb0d6963f7d28e17f72".into(),
            check_hash: true,
            force_delete: false,
        };
        for attempt in 0..2 {
            tokio::fs::write(&part, b"bad").await.unwrap();
            assert!(!verify_or_retry(&part, &file, attempt).await.unwrap());
            assert!(!part.exists());
        }
        tokio::fs::write(&part, b"bad").await.unwrap();
        assert_eq!(
            verify_or_retry(&part, &file, 2).await.unwrap_err(),
            StoreError::Integrity
        );
        assert!(!part.exists());
    }
}
