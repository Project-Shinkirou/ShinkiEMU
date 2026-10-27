use super::*;
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use sha2::{Digest, Sha256};
use shinki_emu_metadata::{PatchArtifact, PatchKind};
use std::{io::{Read, Write}, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{Receiver, TryRecvError, sync_channel}}, thread::JoinHandle};

pub(super) struct PatchDownloadJob {
    cancelled: Arc<AtomicBool>,
    completion: Receiver<Result<PathBuf, String>>,
    thread: Option<JoinHandle<()>>,
}

impl PatchDownloadJob {
    pub(super) fn start(game: PathBuf, artifact: PatchArtifact, kind: PatchKind, wake: HostWake) -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let (sender, completion) = sync_channel(1);
        let thread = std::thread::Builder::new().name("shinki-patch-download".into()).spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread().enable_all().build()
                .map_err(|e| format!("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: {e}"))
                .and_then(|runtime| runtime.block_on(download(&game, &artifact, kind, &signal)));
            let _ = sender.send(result);
            wake();
        }).map_err(|e| format!("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: {e}"))?;
        Ok(Self { cancelled, completion, thread: Some(thread) })
    }

    pub(super) fn cancel(&self) { self.cancelled.store(true, Ordering::Release); }

    pub(super) fn result(&self) -> Result<Option<Result<PathBuf, String>>, String> {
        match self.completion.try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: download worker disconnected".into()),
        }
    }

    pub(super) fn join(&mut self) -> Result<(), String> {
        if self.thread.take().is_some_and(|thread| thread.join().is_err()) {
            Err("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: download worker panicked".into())
        } else { Ok(()) }
    }
}

impl Drop for PatchDownloadJob {
    fn drop(&mut self) {
        if self.thread.is_some() { self.cancel(); let _ = self.join(); }
    }
}

fn private_patch_dir(game: &Path) -> Result<(Dir, PathBuf), String> {
    let root = Dir::open_ambient_dir(game, ambient_authority())
        .map_err(|_| "SHINKI_EMU_PATCH_PATH: game root unavailable")?;
    let state = match root.create_dir(shinki_emu_patch::GAME_STATE_DIR) {
        Ok(()) => root.open_dir_nofollow(shinki_emu_patch::GAME_STATE_DIR),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => root.open_dir_nofollow(shinki_emu_patch::GAME_STATE_DIR),
        Err(_) => return Err("SHINKI_EMU_PATCH_PATH: cannot create private game state".into()),
    }.map_err(|_| "SHINKI_EMU_PATCH_PATH: unsafe private game state")?;
    let patch = match state.create_dir(shinki_emu_patch::PATCH_DIR) {
        Ok(()) => state.open_dir_nofollow(shinki_emu_patch::PATCH_DIR),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => state.open_dir_nofollow(shinki_emu_patch::PATCH_DIR),
        Err(_) => return Err("SHINKI_EMU_PATCH_PATH: cannot create private patch directory".into()),
    }.map_err(|_| "SHINKI_EMU_PATCH_PATH: unsafe private patch directory")?;
    let path = std::fs::canonicalize(game)
        .map_err(|_| "SHINKI_EMU_PATCH_PATH: game path unavailable")?
        .join(shinki_emu_patch::GAME_STATE_DIR).join(shinki_emu_patch::PATCH_DIR);
    Ok((patch, path))
}

fn check_file(mut file: impl Read, entry: &PatchArtifact) -> Result<(), String> {
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO")?;
        if count == 0 { break; }
        received = received.saturating_add(count as u64);
        if received > entry.size { return Err("SHINKI_EMU_PATCH_DOWNLOAD_SIZE".into()); }
        hasher.update(&bytes[..count]);
    }
    if received != entry.size { return Err("SHINKI_EMU_PATCH_DOWNLOAD_SIZE".into()); }
    if hex::encode(hasher.finalize()) != entry.sha256 {
        return Err("SHINKI_EMU_PATCH_DOWNLOAD_HASH_MISMATCH".into());
    }
    Ok(())
}

struct ArtifactWriter<'a> {
    entry: &'a PatchArtifact,
    part: tempfile::NamedTempFile,
    received: u64,
    hasher: Sha256,
}

impl<'a> ArtifactWriter<'a> {
    fn new(entry: &'a PatchArtifact, directory: &Path) -> Result<Self, String> {
        let part = tempfile::Builder::new().prefix(".").suffix(".part")
            .tempfile_in(directory).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO")?;
        Ok(Self { entry, part, received: 0, hasher: Sha256::new() })
    }

    fn chunk(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.received = self.received.saturating_add(bytes.len() as u64);
        if self.received > self.entry.size { return Err("SHINKI_EMU_PATCH_DOWNLOAD_SIZE".into()); }
        self.hasher.update(bytes);
        self.part.write_all(bytes).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO".into())
    }

    fn finish(self, target: &Path) -> Result<(), String> {
        if self.received != self.entry.size { return Err("SHINKI_EMU_PATCH_DOWNLOAD_SIZE".into()); }
        if hex::encode(self.hasher.finalize()) != self.entry.sha256 {
            return Err("SHINKI_EMU_PATCH_DOWNLOAD_HASH_MISMATCH".into());
        }
        self.part.as_file().sync_all().map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO")?;
        self.part.persist_noclobber(target).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO")?;
        Ok(())
    }
}

async fn download(game: &Path, entry: &PatchArtifact, kind: PatchKind, cancelled: &AtomicBool) -> Result<PathBuf, String> {
    if !shinki_emu_metadata::is_safe_metadata_relative_path(&entry.file_name) || entry.file_name.contains('/')
        || entry.size == 0 || entry.size > shinki_emu_metadata::MAX_PATCH_ARTIFACT_BYTES
        || entry.sha256.len() != 64 || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("SHINKI_EMU_PATCH_DOWNLOAD_INVALID_ARTIFACT".into());
    }
    let (patch, directory) = private_patch_dir(game)?;
    match patch.symlink_metadata(&entry.file_name) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            check_file(patch.open(&entry.file_name).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_IO")?, entry)?;
            return Ok(directory);
        }
        Ok(_) => return Err("SHINKI_EMU_PATCH_PATH: artifact target is not an ordinary file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("SHINKI_EMU_PATCH_PATH: cannot inspect artifact target".into()),
    }
    let url = reqwest::Url::parse(&entry.url).map_err(|_| "SHINKI_EMU_PATCH_DOWNLOAD_URL")?;
    let https = |url: &reqwest::Url| url.scheme() == "https" && url.username().is_empty()
        && url.password().is_none() && url.host_str().is_some();
    if !https(&url) || (kind == PatchKind::Hosted && !metadata_runtime::core_redirect_allowed(&url)) {
        return Err("SHINKI_EMU_PATCH_DOWNLOAD_URL".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 || !https(attempt.url())
                || (kind == PatchKind::Hosted && !metadata_runtime::core_redirect_allowed(attempt.url())) {
                attempt.error("SHINKI_EMU_PATCH_DOWNLOAD_REDIRECT")
            } else { attempt.follow() }
        }))
        .build().map_err(|e| format!("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: {}", e.without_url()))?;
    let network = |e: reqwest::Error| format!("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: {}", e.without_url());
    let mut response = client.get(url).send().await.map_err(network)?;
    if !response.status().is_success() {
        return Err(format!("SHINKI_EMU_PATCH_DOWNLOAD_NETWORK: http-{}", response.status().as_u16()));
    }
    if response.content_length().is_some_and(|size| size != entry.size) {
        return Err("SHINKI_EMU_PATCH_DOWNLOAD_SIZE".into());
    }
    // NamedTempFile removes the .part on errors, cancellation, and hash failure.
    let mut writer = ArtifactWriter::new(entry, &directory)?;
    while let Some(bytes) = response.chunk().await.map_err(network)? {
        if cancelled.load(Ordering::Acquire) { return Err("SHINKI_EMU_PATCH_CANCELLED".into()); }
        writer.chunk(&bytes)?;
    }
    if cancelled.load(Ordering::Acquire) { return Err("SHINKI_EMU_PATCH_CANCELLED".into()); }
    writer.finish(&directory.join(&entry.file_name))?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_sha_or_size_rejects_without_creating_patch_file() {
        let game = tempfile::tempdir().unwrap();
        let entry = PatchArtifact {
            url: "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/test/fixture.zip".into(),
            sha256: "0".repeat(64), size: 4, file_name: "fixture.zip".into(),
        };
        let (_dir, path) = private_patch_dir(game.path()).unwrap();
        let mut too_large = ArtifactWriter::new(&entry, &path).unwrap();
        assert_eq!(too_large.chunk(b"wrong").unwrap_err(), "SHINKI_EMU_PATCH_DOWNLOAD_SIZE");
        drop(too_large);
        let mut wrong_hash = ArtifactWriter::new(&entry, &path).unwrap();
        wrong_hash.chunk(b"test").unwrap();
        assert_eq!(wrong_hash.finish(&path.join(&entry.file_name)).unwrap_err(),
            "SHINKI_EMU_PATCH_DOWNLOAD_HASH_MISMATCH");
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    }
}