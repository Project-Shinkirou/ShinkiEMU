//! Online core downloads. Each job streams a catalog package into
//! `cores/.pending/.download-*`, checks its length and SHA-256 against the
//! catalog entry and stages it with [`core_package::stage_package`]. At most
//! [`MAX_PARALLEL`] transfers run at once; later jobs wait in the queue.
//!
//! Cancelling drops the job's future: the temporary download file is removed
//! with it and nothing reaches `pending/<stem>/`. Staging itself is a short
//! atomic rename and cannot be cancelled once it has begun.

use std::{
    collections::BTreeMap,
    future::Future,
    io::Write,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use futures_util::{Stream, StreamExt};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use shinki_emu_manager::HostWake;
use shinki_emu_metadata::{CoreCatalogEntry, MAX_CORE_PACKAGE_BYTES};

use crate::core_package;

pub(crate) const MAX_PARALLEL: usize = 2;
/// Progress wakes the UI at most this often per job.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Transfer rate is averaged over windows of at least this length.
const SPEED_WINDOW: Duration = Duration::from_millis(500);

const CORE_REDIRECT_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
const MAX_CORE_REDIRECTS: usize = 10;

pub(crate) type Body = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, String>> + Send>>;
type Opened = Pin<Box<dyn Future<Output = Result<(Option<u64>, Body), String>> + Send>>;
/// Opens the package of a catalog entry: announced length and byte stream.
pub(crate) type Fetch = Arc<dyn Fn(CoreCatalogEntry) -> Opened + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Queued,
    Downloading,
    Verifying,
    Installing,
}

/// What the UI shows for one running job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Progress {
    pub(crate) phase: Phase,
    pub(crate) received: u64,
    /// Zero while the size is unknown.
    pub(crate) total: u64,
    pub(crate) bytes_per_second: u64,
}

/// A job that ended on its own; cancelled jobs report nothing.
#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) plugin_id: String,
    pub(crate) version: String,
    /// The staged core directory stem.
    pub(crate) result: Result<String, String>,
}

struct Shared {
    progress: Progress,
    cancelled: bool,
}

struct Job {
    serial: u64,
    shared: Arc<Mutex<Shared>>,
    task: tokio::task::JoinHandle<()>,
}

pub(crate) struct CoreDownloads {
    fetch: Fetch,
    runtime: Option<tokio::runtime::Runtime>,
    permits: Arc<tokio::sync::Semaphore>,
    jobs: BTreeMap<String, Job>,
    serial: u64,
    sender: mpsc::Sender<(u64, Finished)>,
    receiver: mpsc::Receiver<(u64, Finished)>,
}

impl CoreDownloads {
    pub(crate) fn new(fetch: Fetch) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            fetch,
            runtime: None,
            permits: Arc::new(tokio::sync::Semaphore::new(MAX_PARALLEL)),
            jobs: BTreeMap::new(),
            serial: 0,
            sender,
            receiver,
        }
    }

    pub(crate) fn github() -> Self {
        Self::new(Arc::new(|entry| Box::pin(open_github(entry))))
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.jobs.is_empty()
    }

    pub(crate) fn is_running(&self, plugin_id: &str) -> bool {
        self.jobs.contains_key(plugin_id)
    }

    pub(crate) fn progress(&self, plugin_id: &str) -> Option<Progress> {
        self.jobs
            .get(plugin_id)
            .map(|job| job.shared.lock().progress)
    }

    /// Queue a download of `entry` into `pending`.
    pub(crate) fn start(
        &mut self,
        entry: CoreCatalogEntry,
        pending: PathBuf,
        wake: HostWake,
    ) -> Result<(), String> {
        if self.jobs.contains_key(&entry.plugin_id) {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_PENDING".into());
        }
        if self.runtime.is_none() {
            self.runtime = Some(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(MAX_PARALLEL)
                    .thread_name("shinki-emu-core-download")
                    .enable_all()
                    .build()
                    .map_err(|_| "SHINKI_EMU_CORE_DOWNLOAD_RUNTIME".to_owned())?,
            );
        }
        let runtime = self.runtime.as_ref().ok_or("SHINKI_EMU_CORE_DOWNLOAD_RUNTIME")?;
        self.serial += 1;
        let serial = self.serial;
        let shared = Arc::new(Mutex::new(Shared {
            progress: Progress {
                phase: Phase::Queued,
                received: 0,
                total: entry.size,
                bytes_per_second: 0,
            },
            cancelled: false,
        }));
        let plugin_id = entry.plugin_id.clone();
        let task = runtime.spawn(run(
            entry,
            pending,
            Arc::clone(&self.fetch),
            Arc::clone(&self.permits),
            Arc::clone(&shared),
            wake,
            serial,
            self.sender.clone(),
        ));
        self.jobs.insert(
            plugin_id,
            Job {
                serial,
                shared,
                task,
            },
        );
        Ok(())
    }

    /// Stop a queued or transferring job. `Ok(false)` when nothing runs for
    /// `plugin_id`; an error once the package is being installed.
    pub(crate) fn cancel(&mut self, plugin_id: &str) -> Result<bool, String> {
        let Some(job) = self.jobs.get(plugin_id) else {
            return Ok(false);
        };
        {
            let mut shared = job.shared.lock();
            if shared.progress.phase == Phase::Installing {
                return Err("SHINKI_EMU_CORE_DOWNLOAD_INSTALLING".into());
            }
            shared.cancelled = true;
        }
        job.task.abort();
        self.jobs.remove(plugin_id);
        tracing::info!(event = "shinki.emu.manager.core_download_cancelled", plugin_id = %plugin_id);
        Ok(true)
    }

    /// Jobs that ended since the last call.
    pub(crate) fn finished(&mut self) -> Vec<Finished> {
        let mut done = Vec::new();
        while let Ok((serial, finished)) = self.receiver.try_recv() {
            if self
                .jobs
                .get(&finished.plugin_id)
                .is_some_and(|job| job.serial == serial)
            {
                self.jobs.remove(&finished.plugin_id);
                done.push(finished);
            }
        }
        done
    }
}

impl Drop for CoreDownloads {
    fn drop(&mut self) {
        for job in self.jobs.values() {
            job.task.abort();
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    entry: CoreCatalogEntry,
    pending: PathBuf,
    fetch: Fetch,
    permits: Arc<tokio::sync::Semaphore>,
    shared: Arc<Mutex<Shared>>,
    wake: HostWake,
    serial: u64,
    sender: mpsc::Sender<(u64, Finished)>,
) {
    let result = transfer(&entry, pending, fetch, permits, &shared, &wake).await;
    if let Err(error) = &result {
        if error == CANCELLED {
            return;
        }
        tracing::warn!(
            event = "shinki.emu.manager.core_download_failed",
            plugin_id = %entry.plugin_id,
            diagnostic_code = %error
        );
    }
    let finished = Finished {
        plugin_id: entry.plugin_id,
        version: entry.version,
        result,
    };
    if sender.send((serial, finished)).is_ok() {
        wake();
    }
}

const CANCELLED: &str = "SHINKI_EMU_CORE_DOWNLOAD_CANCELLED";

async fn transfer(
    entry: &CoreCatalogEntry,
    pending: PathBuf,
    fetch: Fetch,
    permits: Arc<tokio::sync::Semaphore>,
    shared: &Mutex<Shared>,
    wake: &HostWake,
) -> Result<String, String> {
    let _permit = permits
        .acquire_owned()
        .await
        .map_err(|_| "SHINKI_EMU_CORE_DOWNLOAD_RUNTIME".to_owned())?;
    shared.lock().progress.phase = Phase::Downloading;
    wake();
    let (announced, mut body) = fetch(entry.clone()).await?;
    let mut check = DownloadCheck::new(entry, announced)?;
    std::fs::create_dir_all(&pending).map_err(|_| "SHINKI_EMU_CORE_INSTALL_COPY")?;
    let mut archive = tempfile::Builder::new()
        .prefix(".download-")
        .tempfile_in(&pending)
        .map_err(|_| "SHINKI_EMU_CORE_INSTALL_COPY")?;
    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;
    let mut last_wake = Instant::now();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        check.chunk(&chunk)?;
        archive
            .write_all(&chunk)
            .map_err(|_| "SHINKI_EMU_CORE_INSTALL_COPY")?;
        window_bytes += chunk.len() as u64;
        let elapsed = window_start.elapsed();
        {
            let mut shared = shared.lock();
            shared.progress.received = check.received;
            if elapsed >= SPEED_WINDOW {
                shared.progress.bytes_per_second =
                    (u128::from(window_bytes) * 1000 / elapsed.as_millis().max(1)) as u64;
                window_start = Instant::now();
                window_bytes = 0;
            }
        }
        if last_wake.elapsed() >= PROGRESS_INTERVAL {
            last_wake = Instant::now();
            wake();
        }
    }
    {
        let mut shared = shared.lock();
        shared.progress.phase = Phase::Verifying;
        shared.progress.bytes_per_second = 0;
    }
    wake();
    check.finish()?;
    archive
        .as_file()
        .sync_all()
        .map_err(|_| "SHINKI_EMU_CORE_INSTALL_COPY")?;
    {
        let mut shared = shared.lock();
        if shared.cancelled {
            return Err(CANCELLED.into());
        }
        shared.progress.phase = Phase::Installing;
    }
    wake();
    let (stem, _) = core_package::stage_package(
        archive.path(),
        Some((&entry.plugin_id, &entry.version, entry.identity.as_ref())),
        &pending,
    )?;
    tracing::info!(
        event = "shinki.emu.manager.core_downloaded",
        plugin_id = %entry.plugin_id,
        version = %entry.version,
        core = %stem
    );
    Ok(stem)
}

fn network(error: reqwest::Error) -> String {
    format!("SHINKI_EMU_CORE_DOWNLOAD_NETWORK: {}", error.without_url())
}

async fn open_github(entry: CoreCatalogEntry) -> Result<(Option<u64>, Body), String> {
    let url = reqwest::Url::parse(&entry.url).map_err(|_| "SHINKI_EMU_CORE_DOWNLOAD_URL")?;
    if !core_redirect_allowed(&url) {
        return Err("SHINKI_EMU_CORE_DOWNLOAD_URL".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_CORE_REDIRECTS {
                attempt.error("SHINKI_EMU_CORE_DOWNLOAD_REDIRECT")
            } else if core_redirect_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("SHINKI_EMU_CORE_DOWNLOAD_REDIRECT")
            }
        }))
        .build()
        .map_err(network)?;
    let response = client.get(url).send().await.map_err(network)?;
    if !response.status().is_success() {
        return Err(format!(
            "SHINKI_EMU_CORE_DOWNLOAD_NETWORK: http-{}",
            response.status().as_u16()
        ));
    }
    let length = response.content_length();
    let body = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(network));
    Ok((length, Box::pin(body)))
}

/// GitHub release assets redirect to its asset CDN; nothing else is followed.
pub(crate) fn core_redirect_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .host_str()
            .is_some_and(|host| CORE_REDIRECT_HOSTS.contains(&host))
}

/// Running length/SHA-256 check of a core package download against its
/// catalog entry.
pub(crate) struct DownloadCheck<'a> {
    entry: &'a CoreCatalogEntry,
    received: u64,
    hasher: Sha256,
}

impl<'a> DownloadCheck<'a> {
    pub(crate) fn new(entry: &'a CoreCatalogEntry, announced: Option<u64>) -> Result<Self, String> {
        if entry.size > MAX_CORE_PACKAGE_BYTES
            || announced.is_some_and(|length| length != entry.size)
        {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        Ok(Self {
            entry,
            received: 0,
            hasher: Sha256::new(),
        })
    }

    pub(crate) fn chunk(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.received = self.received.saturating_add(bytes.len() as u64);
        if self.received > self.entry.size {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        self.hasher.update(bytes);
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        if self.received != self.entry.size {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_SIZE".into());
        }
        let digest = hex::encode(self.hasher.finalize());
        if digest != self.entry.sha256 {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_HASH_MISMATCH".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn entry(plugin_id: &str, bytes: &[u8]) -> CoreCatalogEntry {
        CoreCatalogEntry {
            plugin_id: plugin_id.into(),
            display_name: "Shinki Fixture".into(),
            family_name: "Fixture".into(),
            version: "0.2.0".into(),
            abi: shinki_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            url: "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/emu-v0.2.0/core.zip"
                .into(),
            sha256: crate::metadata_cache::sha256_hex(bytes),
            size: bytes.len() as u64,
            license: "MIT".into(),
            release_tag: "emu-v0.2.0".into(),
            identity: None,
        }
    }

    /// Serves packages from memory. A plugin listed in `gated` sends its
    /// first half, then waits for its gate before sending the rest.
    pub(crate) struct Served {
        pub(crate) packages: BTreeMap<String, Vec<u8>>,
        pub(crate) gates: BTreeMap<String, Arc<tokio::sync::Notify>>,
        pub(crate) failing: Vec<String>,
    }

    pub(crate) fn serve(served: Served) -> Fetch {
        let served = Arc::new(served);
        Arc::new(move |entry: CoreCatalogEntry| {
            let served = Arc::clone(&served);
            Box::pin(async move {
                if served.failing.contains(&entry.plugin_id) {
                    return Err("SHINKI_EMU_CORE_DOWNLOAD_NETWORK: http-503".to_owned());
                }
                let bytes = served
                    .packages
                    .get(&entry.plugin_id)
                    .cloned()
                    .ok_or_else(|| "SHINKI_EMU_CORE_DOWNLOAD_NETWORK: http-404".to_owned())?;
                let gate = served.gates.get(&entry.plugin_id).cloned();
                let length = bytes.len() as u64;
                let half = bytes.len() / 2;
                let first = bytes::Bytes::copy_from_slice(&bytes[..half]);
                let rest = bytes::Bytes::copy_from_slice(&bytes[half..]);
                let body = futures_util::stream::once(async move { Ok(first) }).chain(
                    futures_util::stream::once(async move {
                        if let Some(gate) = gate {
                            gate.notified().await;
                        }
                        Ok(rest)
                    }),
                );
                Ok((Some(length), Box::pin(body) as Body))
            })
        })
    }

    /// A valid core package for `plugin_id` whose library is `<stem>.<dll>`.
    pub(crate) fn package_for(plugin_id: &str, stem: &str) -> Vec<u8> {
        use std::io::Cursor;
        let library = format!("{stem}.{}", std::env::consts::DLL_EXTENSION);
        let files: [(&str, &[u8]); 2] = [(&library, b"library bytes"), ("licenses/LICENSE.txt", b"license")];
        let mut listed: Vec<core_package::CoreFile> = files
            .iter()
            .map(|(path, bytes)| core_package::CoreFile {
                path: (*path).into(),
                sha256: core_package::sha256_hex(bytes),
            })
            .collect();
        listed.sort_by(|left, right| left.path.cmp(&right.path));
        let manifest = serde_json::to_vec(&core_package::CoreManifest {
            schema: core_package::CORE_PACKAGE_SCHEMA.into(),
            plugin_id: plugin_id.into(),
            library: library.clone(),
            version: "0.2.0".into(),
            identity: None,
            abi: shinki_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            files: listed,
        })
        .unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files.iter().copied().chain([(core_package::CORE_MANIFEST, manifest.as_slice())]) {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn noop_wake() -> HostWake {
        Arc::new(|| {})
    }

    #[test]
    fn redirects_are_limited_to_github_release_hosts_over_https() {
        for allowed in [
            "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/x/y.zip",
            "https://objects.githubusercontent.com/github-production-release-asset/1",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1",
        ] {
            assert!(core_redirect_allowed(&reqwest::Url::parse(allowed).unwrap()), "{allowed}");
        }
        for rejected in [
            "http://github.com/x",
            "https://example.com/core.zip",
            "https://github.com.evil.example/x",
            "https://user:pass@github.com/x",
        ] {
            assert!(!core_redirect_allowed(&reqwest::Url::parse(rejected).unwrap()), "{rejected}");
        }
    }

    #[test]
    fn download_check_enforces_size_and_sha256() {
        let entry = entry("shinki.emu.musica", b"package bytes");
        let mut check = DownloadCheck::new(&entry, Some(entry.size)).unwrap();
        check.chunk(b"package ").unwrap();
        check.chunk(b"bytes").unwrap();
        check.finish().unwrap();

        assert_eq!(
            DownloadCheck::new(&entry, Some(entry.size + 1)).err().unwrap(),
            "SHINKI_EMU_CORE_DOWNLOAD_SIZE"
        );
        let mut long = DownloadCheck::new(&entry, None).unwrap();
        assert_eq!(
            long.chunk(b"package bytes plus").unwrap_err(),
            "SHINKI_EMU_CORE_DOWNLOAD_SIZE"
        );
        let mut tampered = DownloadCheck::new(&entry, None).unwrap();
        tampered.chunk(b"package BYTES").unwrap();
        assert_eq!(
            tampered.finish().unwrap_err(),
            "SHINKI_EMU_CORE_DOWNLOAD_HASH_MISMATCH"
        );
    }

    #[test]
    fn progress_reports_bytes_then_stages_the_package() {
        let root = tempfile::tempdir().unwrap();
        let pending = root.path().join("cores").join(core_package::PENDING);
        let bytes = package_for("shinki.emu.fixture", "shinki_emu_fixture");
        let gate = Arc::new(tokio::sync::Notify::new());
        let mut downloads = CoreDownloads::new(serve(Served {
            packages: BTreeMap::from([("shinki.emu.fixture".into(), bytes.clone())]),
            gates: BTreeMap::from([("shinki.emu.fixture".into(), Arc::clone(&gate))]),
            failing: Vec::new(),
        }));
        downloads
            .start(entry("shinki.emu.fixture", &bytes), pending.clone(), noop_wake())
            .unwrap();
        let half = (bytes.len() / 2) as u64;
        wait_until(|| downloads.progress("shinki.emu.fixture").unwrap().received == half);
        let progress = downloads.progress("shinki.emu.fixture").unwrap();
        assert_eq!(progress.phase, Phase::Downloading);
        assert_eq!(progress.total, bytes.len() as u64);
        gate.notify_one();
        let mut finished = Vec::new();
        wait_until(|| {
            finished.extend(downloads.finished());
            !finished.is_empty()
        });
        assert_eq!(finished[0].result.as_deref(), Ok("shinki_emu_fixture"));
        assert!(pending.join("shinki_emu_fixture").join(core_package::CORE_MANIFEST).is_file());
        assert!(!downloads.is_running("shinki.emu.fixture"));
    }

    #[test]
    fn cancel_discards_the_partial_download() {
        let root = tempfile::tempdir().unwrap();
        let pending = root.path().join("cores").join(core_package::PENDING);
        let bytes = package_for("shinki.emu.fixture", "shinki_emu_fixture");
        let gate = Arc::new(tokio::sync::Notify::new());
        let mut downloads = CoreDownloads::new(serve(Served {
            packages: BTreeMap::from([("shinki.emu.fixture".into(), bytes.clone())]),
            gates: BTreeMap::from([("shinki.emu.fixture".into(), Arc::clone(&gate))]),
            failing: Vec::new(),
        }));
        downloads
            .start(entry("shinki.emu.fixture", &bytes), pending.clone(), noop_wake())
            .unwrap();
        wait_until(|| downloads.progress("shinki.emu.fixture").unwrap().received > 0);
        let partial = || {
            std::fs::read_dir(&pending)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".download-"))
                .count()
        };
        assert_eq!(partial(), 1);
        assert!(downloads.cancel("shinki.emu.fixture").unwrap());
        assert!(!downloads.cancel("shinki.emu.fixture").unwrap());
        wait_until(|| partial() == 0);
        gate.notify_one();
        std::thread::sleep(Duration::from_millis(50));
        assert!(downloads.finished().is_empty());
        assert!(
            std::fs::read_dir(&pending).unwrap().next().is_none(),
            "cancel left files in pending"
        );
    }
}
