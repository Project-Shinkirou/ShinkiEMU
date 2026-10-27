//! Original PC files only: Play-optimized assets are not game installers.
use super::{Http, PlanData};
use futures_util::StreamExt;
use reqwest::{
    StatusCode, Url,
    header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, COOKIE, LOCATION, RANGE},
};
use serde::Serialize;
use shinki_emu_store::{
    CancelToken, InstallNotice, OwnedTitle, Progress, ProgressUpdates, StoreError, TransferSummary,
};
use std::{collections::HashSet, path::Path};
use tokio::{fs, io::AsyncWriteExt};

#[derive(Serialize)]
struct InstalledPart {
    filename: String,
    size_bytes: u64,
}

#[derive(Serialize)]
struct InstalledParts<'a> {
    workno: &'a str,
    parts: &'a [InstalledPart],
    manual_extraction_required: bool,
}

pub(super) async fn download_parts(
    http: &Http,
    plan: &PlanData,
    title: &OwnedTitle,
    dest: &Path,
    work_dir: &Path,
    progress: &ProgressUpdates,
    cancel: &CancelToken,
) -> Result<TransferSummary, StoreError> {
    if !title.id.0.chars().all(|c| c.is_ascii_alphanumeric()) || title.id.0.is_empty() {
        return Err(StoreError::Metadata);
    }
    let staging = work_dir.join(&title.id.0);
    fs::create_dir_all(&staging).await?;
    let mut summary = TransferSummary::default();
    let mut parts = Vec::with_capacity(plan.files.len());
    let mut names = HashSet::new();

    for (index, item) in plan.files.iter().enumerate() {
        if cancel.is_cancelled() {
            summary.cancelled = true;
            return Ok(summary);
        }
        let partial = staging.join(format!("{index:04}.partial"));
        let offset = match fs::metadata(&partial).await {
            Ok(info) => info.len(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
            Err(_) => return Err(StoreError::Io),
        };
        let response = open_download(http, item.url.clone(), offset).await?;
        let filename = response_filename(&response)?;
        if response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/html"))
        { return Err(StoreError::SessionExpired); }
        if !names.insert(filename.clone()) {
            return Err(StoreError::Metadata);
        }
        let append = offset != 0 && response.status() == StatusCode::PARTIAL_CONTENT;
        if response.status() == StatusCode::PARTIAL_CONTENT && !append {
            return Err(StoreError::Integrity);
        }
        if append && !response.headers().get(CONTENT_RANGE)
            .and_then(|header| header.to_str().ok())
            .is_some_and(|header| range_total(header, offset).is_some())
        {
            return Err(StoreError::Integrity);
        }
        let expected_total = if append {
            response.headers().get(CONTENT_RANGE).and_then(|header| header.to_str().ok())
                .and_then(|header| range_total(header, offset))
        } else {
            response.headers().get(CONTENT_LENGTH).and_then(|header| header.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        };
        let mut file = if append {
            fs::OpenOptions::new().append(true).open(&partial).await?
        } else {
            fs::File::create(&partial).await?
        };
        let mut stream = response.bytes_stream();
        let mut received = 0_u64;
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                summary.cancelled = true;
                return Ok(summary);
            }
            let chunk = chunk.map_err(|_| StoreError::TransferFailed)?;
            file.write_all(&chunk).await?;
            received += chunk.len() as u64;
        }
        file.flush().await?;
        drop(file);
        let size = fs::metadata(&partial).await?.len();
        if expected_total.is_some_and(|expected| expected != size) ||
            (append && size != offset + received)
        {
            return Err(StoreError::Integrity);
        }
        summary.bytes_downloaded += received;
        summary.files_completed += 1;
        progress(Progress {
            bytes_done: summary.bytes_downloaded,
            bytes_total: title.size_bytes,
            files_done: summary.files_completed,
            files_total: plan.files.len() as u64,
            ..Progress::default()
        });
        parts.push(InstalledPart { filename, size_bytes: size });
    }
    if cancel.is_cancelled() {
        summary.cancelled = true;
        return Ok(summary);
    }
    let zip = parts.len() == 1 && parts[0].filename.to_ascii_lowercase().ends_with(".zip");
    let manifest = InstalledParts {
        workno: &title.id.0,
        parts: &parts,
        manual_extraction_required: !zip,
    };
    let manifest = serde_json::to_vec(&manifest).map_err(|_| StoreError::Metadata)?;
    fs::write(staging.join("installed-parts.json"), manifest).await?;
    if zip {
        let archive = staging.join("0000.partial");
        let staging_clone = staging.clone();
        let dest = dest.to_path_buf();
        let token = cancel.clone();
        tokio::task::spawn_blocking(move || extract_zip(&archive, &staging_clone, &dest, &token))
            .await.map_err(|_| StoreError::Integrity)??;
    } else {
        fs::create_dir_all(dest).await?;
        for (index, part) in parts.iter().enumerate() {
            let output = dest.join(&part.filename);
            if fs::try_exists(&output).await? {
                if fs::metadata(&output).await?.len() == part.size_bytes {
                    summary.files_skipped += 1;
                    continue;
                }
                return Err(StoreError::Io);
            }
            fs::rename(staging.join(format!("{index:04}.partial")), output).await?;
        }
        summary.notice = Some(InstallNotice::ManualExtractionRequired);
    }
    Ok(summary)
}

async fn open_download(
    http: &Http, mut url: Url, offset: u64,
) -> Result<reqwest::Response, StoreError> {
    for _ in 0..8 {
        if url.scheme() != "https" { return Err(StoreError::TransferFailed); }
        let mut request = http.client.get(url.clone());
        if url.host_str().is_some_and(|host| host == "dlsite.com" || host.ends_with(".dlsite.com")) {
            request = request.header(COOKIE, http.cookie.as_str());
        }
        if offset != 0 { request = request.header(RANGE, format!("bytes={offset}-")); }
        let response = request.send().await.map_err(|_| StoreError::Network)?;
        if response.status().is_redirection() {
            let location = response.headers().get(LOCATION).and_then(|h| h.to_str().ok())
                .ok_or(StoreError::Metadata)?;
            url = response.url().join(location).map_err(|_| StoreError::Metadata)?;
            continue;
        }
        if response.status() == StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); }
        if response.status() == StatusCode::FORBIDDEN { return Err(StoreError::AccessDenied); }
        if response.status() != StatusCode::OK && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(StoreError::TransferFailed);
        }
        return Ok(response);
    }
    Err(StoreError::TransferFailed)
}

fn range_total(header: &str, offset: u64) -> Option<u64> {
    let (range, total) = header.strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = range.split_once('-')?;
    let first = first.parse::<u64>().ok()?;
    let last = last.parse::<u64>().ok()?;
    let total = total.parse::<u64>().ok()?;
    (first == offset && last >= first && last < total).then_some(total)
}

fn response_filename(response: &reqwest::Response) -> Result<String, StoreError> {
    if let Some(disposition) = response.headers().get(CONTENT_DISPOSITION)
        .and_then(|h| h.to_str().ok()) {
        for parameter in disposition.split(';').map(str::trim) {
            if let Some(value) = parameter.strip_prefix("filename*=UTF-8''") {
                return validated_filename(&percent_decode(value)?);
            }
        }
        for parameter in disposition.split(';').map(str::trim) {
            if let Some(value) = parameter.strip_prefix("filename=") {
                return validated_filename(value.trim_matches('"'));
            }
        }
    }
    let name = response.url().path_segments().and_then(Iterator::last)
        .ok_or(StoreError::Metadata)?;
    let name = percent_decode(name)?;
    if !name.contains('.') { return Err(StoreError::Metadata); }
    validated_filename(&name)
}

fn percent_decode(text: &str) -> Result<String, StoreError> {
    let mut decoded = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes.next().and_then(|n| (n as char).to_digit(16)).ok_or(StoreError::Metadata)?;
            let lo = bytes.next().and_then(|n| (n as char).to_digit(16)).ok_or(StoreError::Metadata)?;
            decoded.push((hi * 16 + lo) as u8);
        } else { decoded.push(byte); }
    }
    String::from_utf8(decoded).map_err(|_| StoreError::Metadata)
}

fn validated_filename(name: &str) -> Result<String, StoreError> {
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if name.is_empty() || name.len() > 240 || name == "." || name == ".." ||
        name.ends_with('.') || name.ends_with(' ') ||
        name.chars().any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch)) ||
        matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" |
            "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" |
            "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")
    {
        return Err(StoreError::Integrity);
    }
    Ok(name.to_owned())
}

fn extract_zip(archive: &Path, staging: &Path, dest: &Path, cancel: &CancelToken)
    -> Result<(), StoreError> {
    let extracted = staging.join("extracted");
    if extracted.exists() { std::fs::remove_dir_all(&extracted)?; }
    std::fs::create_dir_all(&extracted)?;
    let result = (|| {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)
            .map_err(|_| StoreError::Integrity)?;
        for index in 0..zip.len() {
            if cancel.is_cancelled() { return Err(StoreError::Cancelled); }
            let mut entry = zip.by_index(index).map_err(|_| StoreError::Integrity)?;
            let raw = entry.name();
            if raw.starts_with('/') || raw.contains('\\') || raw.contains(':') ||
                raw.split('/').any(|component| component == "..") ||
                entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
            { return Err(StoreError::Integrity); }
            for component in raw.split('/').filter(|component| !component.is_empty()) {
                validated_filename(component)?;
            }
            let enclosed = entry.enclosed_name().ok_or(StoreError::Integrity)?;
            let target = extracted.join(enclosed);
            if entry.is_dir() { std::fs::create_dir_all(target)?; continue; }
            if target.exists() { return Err(StoreError::Integrity); }
            if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
            let mut output = std::fs::File::create(target)?;
            std::io::copy(&mut entry, &mut output)?;
        }
        if cancel.is_cancelled() { return Err(StoreError::Cancelled); }
        if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent)?; }
        if dest.exists() {
            if std::fs::symlink_metadata(dest)?.file_type().is_symlink() || !dest.is_dir() ||
                std::fs::read_dir(dest)?.next().is_some() { return Err(StoreError::Io); }
            std::fs::remove_dir(dest)?;
        }
        std::fs::rename(&extracted, dest)?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_dir_all(extracted); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_must_match_exact_resume_offset() {
        assert_eq!(range_total("bytes 1024-2047/2048", 1024), Some(2048));
        assert_eq!(range_total("bytes 0-1023/2048", 1024), None);
        assert_eq!(range_total("bytes 1024-9999/2048", 1024), None);
    }

    #[test]
    fn signed_filename_is_decoded_and_restricted_to_leaf_name() {
        assert_eq!(percent_decode("game%20part1.rar").unwrap(), "game part1.rar");
        assert_eq!(validated_filename("../evil.zip"), Err(StoreError::Integrity));
        assert_eq!(validated_filename("work.part1.exe").unwrap(), "work.part1.exe");
    }

    #[test]
    fn zip_traversal_never_writes_into_install() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("archive.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer.start_file("../outside.txt", zip::write::SimpleFileOptions::default()).unwrap();
        use std::io::Write;
        writer.write_all(b"escape").unwrap();
        writer.finish().unwrap();
        let target = root.path().join("installed");
        assert_eq!(extract_zip(&archive, root.path(), &target, &CancelToken::new()), Err(StoreError::Integrity));
        assert!(!target.exists());
        assert!(!root.path().join("outside.txt").exists());
    }
}