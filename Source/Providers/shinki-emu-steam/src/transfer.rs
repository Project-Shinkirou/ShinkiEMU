use crate::{Error, Game, Session, timed};
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use steamroom::{
    cdn::{CdnClient, CdnServerPool},
    depot::manifest::DepotManifest,
    depot::{AppId, CellId, ChunkId, DepotId, ManifestId},
};
use steamroom_client::{
    depot_config::DepotConfig,
    download::{BoxError, CdnChunkFetcher, ChunkFetcher, DepotJob, OldChunkLoc, RetryConfig},
    event::DownloadEvent,
    manifest::parse_cdn_manifest,
};
use tokio::sync::{mpsc, watch};

pub struct DownloadPlan {
    pub game: Game,
    pub depot_ids: Vec<u32>,
    pub branch: String,
    /// Shared destination for all selected depots, below a user-selected root.
    pub install_dir: PathBuf,
    pub verify: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub speed_bytes_per_second: u64,
    pub eta_seconds: u64,
}

#[derive(Debug, Default)]
pub struct TransferSummary {
    pub files_completed: u64,
    pub files_skipped: u64,
    pub bytes_downloaded: u64,
    pub cancelled: bool,
}

struct CancellableFetcher {
    inner: CdnChunkFetcher,
    cancel: watch::Receiver<bool>,
    downloaded: AtomicU64,
}
impl ChunkFetcher for CancellableFetcher {
    async fn fetch_chunk(&self, depot: DepotId, chunk: &ChunkId) -> Result<bytes::Bytes, BoxError> {
        let fetch = async {
            let mut attempts = 0;
            loop {
                attempts += 1;
                match self.inner.fetch_chunk(depot, chunk).await {
                    Ok(bytes) => {
                        self.downloaded
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        return Ok(bytes);
                    }
                    Err(error) if attempts == 3 => return Err(error),
                    Err(_) => tokio::time::sleep(Duration::from_millis(100 * attempts)).await,
                }
            }
        };
        // Retry inside this cancellation boundary, not inside DepotJob:
        // cancelled semaphore waiters fail once instead of sleeping/retrying.
        tokio::select! {
            biased;
            _ = cancelled(self.cancel.clone()) => Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled").into()),
            result = fetch => result,
        }
    }
}

async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

pub fn has_update(install_dir: &Path, game: &Game, selected: &[u32], branch: &str) -> bool {
    let config = DepotConfig::load(install_dir);
    selected.iter().any(|id| {
        game.depots
            .iter()
            .find(|d| d.depot_id == *id)
            .and_then(|d| d.manifests.get(branch))
            .is_some_and(|remote| {
                config
                    .depots
                    .get(id)
                    .is_none_or(|local| local.manifest_id != *remote || local.installing.is_some())
            })
    })
}

fn validate_paths(root: &Path, manifest: &DepotManifest) -> Result<(), Error> {
    for file in &manifest.files {
        let normalized = file.normalized_path();
        let path = Path::new(&normalized);
        if normalized.contains(':')
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
            || path.components().next().is_none()
            || path
                .components()
                .find(|part| matches!(part, Component::Normal(_)))
                .and_then(|component| component.as_os_str().to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(".DepotDownloader"))
        {
            return Err(Error::Integrity);
        }
        let mut ancestor = root.to_path_buf();
        if let Some(parent) = path.parent() {
            for part in parent.components() {
                ancestor.push(part);
                if ancestor
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    return Err(Error::Integrity);
                }
            }
        }
        if let Some(target) = &file.link_target {
            // Resolve symlink targets lexically relative to the link's parent.
            let mut depth = path.parent().map_or(0, |p| p.components().count()) as i64;
            for part in Path::new(target).components() {
                match part {
                    Component::Normal(_) => depth += 1,
                    Component::ParentDir => {
                        depth -= 1;
                        if depth < 0 {
                            return Err(Error::Integrity);
                        }
                    }
                    Component::CurDir => {}
                    _ => return Err(Error::Integrity),
                }
            }
        } else if root
            .join(path)
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error::Integrity);
        }
    }
    Ok(())
}

fn cached_manifest(
    root: &Path,
    depot: DepotId,
    manifest: ManifestId,
    key: &steamroom::depot::DepotKey,
) -> Result<Option<DepotManifest>, Error> {
    let path = DepotConfig::config_dir(root)
        .join("manifests")
        .join(format!("{}_{}.zip", depot.0, manifest.0));
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut manifest = parse_cdn_manifest(&bytes).map_err(|_| Error::Integrity)?;
    if manifest.filenames_encrypted {
        manifest
            .decrypt_filenames(key)
            .map_err(|_| Error::Integrity)?;
    }
    validate_paths(root, &manifest)?;
    Ok(Some(manifest))
}

fn old_layouts(manifest: DepotManifest) -> HashMap<String, Vec<OldChunkLoc>> {
    manifest
        .files
        .into_iter()
        .map(|file| {
            let path = file.normalized_path();
            let mut offset = 0;
            let chunks = file
                .chunks
                .into_iter()
                .map(|chunk| {
                    let position = chunk.offset.unwrap_or(offset);
                    offset = position + u64::from(chunk.uncompressed_size);
                    OldChunkLoc {
                        id: chunk.id,
                        offset: position,
                        size: chunk.uncompressed_size,
                    }
                })
                .collect();
            (path, chunks)
        })
        .collect()
}

impl Session {
    /// Fetch/decrypt manifests, stream verified chunks, and commit each depot's
    /// installed ID after it succeeds. Cancellation drains outstanding writes
    /// before returning, so a cancelled job cannot race the next repair.
    pub async fn download(
        &self,
        plan: &DownloadPlan,
        cancel: watch::Receiver<bool>,
        notify: impl Fn(Progress),
    ) -> Result<TransferSummary, Error> {
        if plan.depot_ids.is_empty() {
            return Err(Error::DepotRequired);
        }
        tokio::fs::create_dir_all(&plan.install_dir).await?;
        let mut config = DepotConfig::load(&plan.install_dir);
        let mut summary = TransferSummary::default();
        let mut aggregate = Progress::default();
        let start = Instant::now();
        let mut downloaded = Vec::new();
        let prepare = async {
            // Manifests are metadata; chunk bodies stay bounded by four fetches.
            for id in &plan.depot_ids {
                let depot = plan
                    .game
                    .depots
                    .iter()
                    .find(|d| d.depot_id == *id)
                    .ok_or(Error::Metadata)?;
                let depot_id = DepotId(*id);
                let manifest_id = ManifestId(
                    *depot
                        .manifests
                        .get(&plan.branch)
                        .ok_or(Error::AccessDenied)?,
                );
                let app_id = AppId(depot.source_app_id);
                let key = timed(self.client.get_depot_decryption_key(depot_id, app_id)).await?;
                let servers = timed(self.client.get_cdn_servers(CellId(0), Some(20))).await?;
                if servers.is_empty() {
                    return Err(Error::Network);
                }
                let request_code = timed(
                    self.client.get_manifest_request_code(
                        app_id,
                        depot_id,
                        manifest_id,
                        Some(&plan.branch),
                        plan.game
                            .branch_hashes
                            .get(&plan.branch)
                            .map(String::as_str),
                    ),
                )
                .await?
                .unwrap_or(0);
                let auth = if self.account_name.is_empty() {
                    None
                } else {
                    timed(
                        self.client
                            .get_cdn_auth_token(app_id, depot_id, &servers[0].host),
                    )
                    .await?
                    .token
                };
                let pool = CdnServerPool::new(servers);
                let cdn = CdnClient::new()?;
                let raw = timed(cdn.download_manifest_pooled(
                    &pool,
                    depot_id,
                    manifest_id,
                    request_code,
                    auth.as_deref(),
                ))
                .await?;
                let mut manifest = parse_cdn_manifest(&raw).map_err(|_| Error::Integrity)?;
                if manifest.filenames_encrypted {
                    manifest
                        .decrypt_filenames(&key)
                        .map_err(|_| Error::Integrity)?;
                }
                if manifest.depot_id.is_some_and(|id| id != depot_id)
                    || manifest.manifest_id.is_some_and(|id| id != manifest_id)
                {
                    return Err(Error::Integrity);
                }
                validate_paths(&plan.install_dir, &manifest)?;
                aggregate.bytes_total += manifest.files.iter().map(|f| f.size).sum::<u64>();
                aggregate.files_total += manifest
                    .files
                    .iter()
                    .filter(|file| {
                        !steamroom::enums::DepotFileFlags::from_bits_retain(file.flags)
                            .is_directory()
                    })
                    .count() as u64;
                let old = if let Some((old_id, old_key)) = config.get_installed(depot_id) {
                    cached_manifest(&plan.install_dir, depot_id, old_id, &old_key)?
                } else {
                    None
                };
                downloaded.push((
                    depot_id,
                    manifest_id,
                    key,
                    cdn,
                    pool,
                    auth,
                    raw,
                    manifest,
                    old,
                ));
            }
            Ok::<(), Error>(())
        };
        tokio::select! {
            result = prepare => result?,
            _ = cancelled(cancel.clone()) => {
                summary.cancelled = true;
                return Ok(summary);
            }
        }
        let mut new_paths: HashSet<String> = downloaded
            .iter()
            .flat_map(|(_, _, _, _, _, _, _, manifest, _)| {
                manifest.files.iter().map(|file| file.normalized_path())
            })
            .collect();
        // Unselected installed language/platform depots can share files too.
        for id in config
            .depots
            .keys()
            .filter(|id| !plan.depot_ids.contains(id))
        {
            let (manifest_id, key) = config.get_installed(DepotId(*id)).ok_or(Error::Integrity)?;
            let manifest = cached_manifest(&plan.install_dir, DepotId(*id), manifest_id, &key)?
                .ok_or(Error::Integrity)?;
            new_paths.extend(
                manifest
                    .files
                    .into_iter()
                    .map(|file| file.normalized_path()),
            );
        }
        notify(aggregate.clone());
        // Execute one file at a time. Upstream creates one task/event per chunk;
        // limiting job scope bounds both to the current file's manifest, while
        // max_downloads bounds resident chunk bodies. The UI receives snapshots.
        let mut retry = RetryConfig::default();
        retry.max_attempts = 1;
        retry.initial_delay = Duration::ZERO;
        for (id, manifest_id, key, cdn, pool, auth, raw, manifest, old) in downloaded {
            if *cancel.borrow() {
                summary.cancelled = true;
                return Ok(summary);
            }
            let mut layouts = old.map(old_layouts).unwrap_or_default();
            let removed: Vec<String> = layouts
                .keys()
                .filter(|path| !new_paths.contains(*path))
                .cloned()
                .collect();
            config.set_installing(id, manifest_id, &key);
            config.save(&plan.install_dir)?;
            DepotConfig::save_manifest_raw(&plan.install_dir, id, manifest_id, &raw)?;
            let fetcher = Arc::new(CancellableFetcher {
                inner: CdnChunkFetcher::new(cdn, pool, auth),
                cancel: cancel.clone(),
                downloaded: AtomicU64::new(0),
            });
            let previous_bytes = summary.bytes_downloaded;
            for file in manifest.files {
                if *cancel.borrow() {
                    summary.cancelled = true;
                    return Ok(summary);
                }
                let filename = file.normalized_path();
                let size = file.size;
                let files_before = aggregate.files_done;
                let one_file = DepotManifest::new(vec![file]);
                let (tx, mut rx) = mpsc::unbounded_channel();
                let mut builder = DepotJob::builder()
                    .depot_id(id)
                    .depot_key(key.clone())
                    .install_dir(plan.install_dir.clone())
                    .max_downloads(4)
                    .verify(plan.verify)
                    .retry(retry.clone())
                    .event_sender(tx);
                if let Some(old) = layouts.remove(&filename) {
                    builder = builder.old_file_layouts(HashMap::from([(filename, old)]));
                }
                let job = builder.build().map_err(|_| Error::Metadata)?;
                let future = job.download(&one_file, fetcher.clone());
                tokio::pin!(future);
                let mut publish = tokio::time::interval(Duration::from_millis(100));
                let result = loop {
                    tokio::select! {
                        result = &mut future => break result,
                        Some(event) = rx.recv() => update_progress(&mut aggregate, event, size),
                        _ = publish.tick() => {
                            let seconds = start.elapsed().as_secs_f64().max(0.001);
                            aggregate.speed_bytes_per_second = ((previous_bytes + fetcher.downloaded.load(Ordering::Relaxed)) as f64 / seconds) as u64;
                            aggregate.eta_seconds = aggregate.bytes_total.saturating_sub(aggregate.bytes_done).checked_div(aggregate.speed_bytes_per_second).unwrap_or(0);
                            notify(aggregate.clone());
                        },
                    }
                };
                while let Ok(event) = rx.try_recv() {
                    update_progress(&mut aggregate, event, size);
                }
                // Upstream DownloadStats counts reused bytes as \"downloaded\".
                // Count actual CDN response bodies at the fetcher boundary.
                summary.bytes_downloaded =
                    previous_bytes + fetcher.downloaded.load(Ordering::Relaxed);
                if *cancel.borrow() {
                    summary.cancelled = true;
                    notify(aggregate);
                    return Ok(summary);
                }
                let stats = result.map_err(|_| Error::TransferFailed)?;
                // Upstream emits no FileCompleted event for newly created empty files.
                aggregate.files_done = files_before + stats.files_completed + stats.files_skipped;
                summary.files_completed += stats.files_completed;
                summary.files_skipped += stats.files_skipped;
                notify(aggregate.clone());
            }
            if *cancel.borrow() {
                summary.cancelled = true;
                return Ok(summary);
            }
            // Reuse the downloader's file-removal rules, protecting every file
            // present in any of the selected new depot manifests.
            if !removed.is_empty() {
                let cleanup = DepotJob::builder()
                    .depot_id(id)
                    .depot_key(key.clone())
                    .install_dir(plan.install_dir.clone())
                    .old_manifest_files(removed)
                    .build()
                    .map_err(|_| Error::Metadata)?;
                cleanup
                    .download(&DepotManifest::new(Vec::new()), fetcher)
                    .await
                    .map_err(|_| Error::TransferFailed)?;
            }
            config.set_installed(id, manifest_id, &key);
            config.save(&plan.install_dir)?;
        }
        Ok(summary)
    }
}

fn update_progress(progress: &mut Progress, event: DownloadEvent, file_size: u64) {
    match event {
        DownloadEvent::ChunkCompleted { bytes } => progress.bytes_done += bytes,
        DownloadEvent::FileCompleted { .. } => progress.files_done += 1,
        DownloadEvent::FileSkipped { .. } => {
            progress.files_done += 1;
            progress.bytes_done += file_size;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steamroom::depot::manifest::ManifestFile;
    #[test]
    fn manifest_paths_cannot_escape_installation() {
        for path in ["../outside", "./.depotdownloader/depot.json", "C:/outside"] {
            let manifest = DepotManifest::new(vec![ManifestFile::new(path.into(), 0)]);
            assert!(validate_paths(Path::new("root"), &manifest).is_err());
        }
    }
    #[test]
    fn installed_manifest_changes_and_interrupted_installs_require_update() {
        let temp = tempfile::tempdir().unwrap();
        let mut game = Game::default();
        let mut depot = crate::Depot {
            depot_id: 481,
            ..Default::default()
        };
        depot.manifests.insert("public".into(), 123);
        game.depots.push(depot);
        let mut config = DepotConfig::default();
        config.set_installed(
            DepotId(481),
            ManifestId(123),
            &steamroom::depot::DepotKey([0; 32]),
        );
        config.save(temp.path()).unwrap();
        assert!(!has_update(temp.path(), &game, &[481], "public"));
        game.depots[0].manifests.insert("public".into(), 124);
        assert!(has_update(temp.path(), &game, &[481], "public"));
        game.depots[0].manifests.insert("public".into(), 123);
        config.set_installing(
            DepotId(481),
            ManifestId(124),
            &steamroom::depot::DepotKey([0; 32]),
        );
        config.save(temp.path()).unwrap();
        assert!(has_update(temp.path(), &game, &[481], "public"));
    }

    #[tokio::test]
    async fn cancellation_requires_a_signal_not_just_a_closed_sender() {
        let (sender, receiver) = watch::channel(false);
        drop(sender);
        assert!(
            tokio::time::timeout(Duration::from_millis(10), cancelled(receiver))
                .await
                .is_err()
        );
        let (sender, receiver) = watch::channel(false);
        let waiter = tokio::spawn(cancelled(receiver));
        sender.send_replace(true);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
    }
}
