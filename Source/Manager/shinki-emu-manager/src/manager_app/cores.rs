use super::*;
use crate::core_download::Phase;
use crate::core_package::{self, PendingKind};
use shinki_emu_manager_core::{FamilyCapability, INDEPENDENT_FAMILY_ABI_FINGERPRINT};
use shinki_emu_manager_ui_slint::tr;
use shinki_emu_manager_ui_slint::{
    CoreBatchViewModel, CoreDownloadViewModel, CoreViewModel, CoresViewModel, describe_error,
};
use shinki_emu_metadata::{CoreCatalogEntry, CoreIdentity};
use std::io::{self, BufRead, Seek, SeekFrom};

/// One 全部更新 run: every member ends succeeded, failed or cancelled.
#[derive(Debug, Default)]
pub(super) struct CoreBatch {
    /// Plugin id and package size of every core in the run.
    members: BTreeMap<String, u64>,
    succeeded: BTreeSet<String>,
    failed: BTreeSet<String>,
    cancelled: BTreeSet<String>,
    /// The end-of-run snackbar was shown.
    summarized: bool,
}

impl CoreBatch {
    fn is_done(&self, plugin_id: &str) -> bool {
        self.succeeded.contains(plugin_id)
            || self.failed.contains(plugin_id)
            || self.cancelled.contains(plugin_id)
    }

    fn finished(&self) -> bool {
        self.members.keys().all(|id| self.is_done(id))
    }

    /// A failed or cancelled member retried on its own leaves the run.
    fn forget(&mut self, plugin_id: &str) {
        if self.failed.remove(plugin_id) || self.cancelled.remove(plugin_id) {
            self.members.remove(plugin_id);
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Queued => "queued",
        Phase::Downloading => "downloading",
        Phase::Verifying => "verifying",
        Phase::Installing => "installing",
    }
}

/// Release platform of this Manager build, e.g. `windows-x86_64`.
pub(super) fn core_platform() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

/// Online core install and update in this build. The Cargo feature
/// `no-core-download` turns it off; iOS and OHOS never download cores.
/// Which plugins a download may stage stays with `allows_core_packages`.
pub(super) const fn online_core_downloads() -> bool {
    !cfg!(any(feature = "no-core-download", target_os = "ios", target_env = "ohos"))
}

fn require_online_core_downloads() -> Result<(), String> {
    if online_core_downloads() { Ok(()) } else { Err("SHINKI_EMU_CORE_DOWNLOAD_DISABLED".into()) }
}

pub(super) fn embedded_identity(
    plugin_id: &str,
) -> Option<&'static shinki_emu_metadata::CoreIdentity> {
    static IDENTITIES: std::sync::LazyLock<BTreeMap<String, shinki_emu_metadata::CoreIdentity>> =
        std::sync::LazyLock::new(|| {
            let identities: BTreeMap<String, shinki_emu_metadata::CoreIdentity> =
                serde_json::from_str(include_str!(concat!(
                    env!("OUT_DIR"),
                    "/embedded-core-identities.json"
                )))
                .expect("built core identities");
            for identity in identities.values() {
                identity.validate().expect("valid embedded core identity");
            }
            identities
        });
    IDENTITIES.get(plugin_id)
}

pub(super) fn embedded_product_release(plugin_id: &str) -> Option<(&'static str, &'static str)> {
    if !recognition::is_static(plugin_id)
        || !shinki_emu_manager_core::static_core_catalog::lookup(plugin_id)
            .is_some_and(|entry| entry.first_party)
    {
        return None;
    }
    let commit = env!("SHINKI_EMU_COMPILED_SOURCE_COMMIT");
    (!commit.is_empty()).then_some((shinki_emu_manager::version(), commit))
}

fn capability_label(capability: FamilyCapability) -> &'static str {
    match capability {
        FamilyCapability::CpuFrame => tr!("软件渲染"),
        FamilyCapability::RenderExtent => tr!("可调渲染分辨率"),
        FamilyCapability::TextReplacement => tr!("实时翻译"),
        FamilyCapability::AsyncTextReplacement => tr!("非阻塞翻译"),
        FamilyCapability::NativeScriptIdentity => tr!("原生脚本存档隔离"),
        FamilyCapability::ConfiguredFontPriority => tr!("优先使用配置字体"),
        FamilyCapability::NativeEmbeddedText => tr!("原生内嵌译文"),
        FamilyCapability::NativeNameResources => tr!("原生姓名资源"),
    }
}

/// Short commits only in the Manager's core cards. The identity still owns
/// the full commits used to compare versions, select packages, and verify installs.
fn core_card_version(version: &str, identity: Option<&CoreIdentity>) -> String {
    let Some(identity) = identity else { return version.to_owned() };
    format!(
        "{}@{} (adapter {})",
        identity.upstream.ref_name,
        &identity.upstream.commit[..8],
        &identity.adapter.commit[..8],
    )
}

pub(super) fn product_version_label(version: &str, source_commit: Option<&str>) -> String {
    match source_commit.and_then(|commit| commit.get(..8)) {
        Some(short) => format!("{version} ({short})"),
        None => version.to_owned(),
    }
}

fn release_matches_target(entry: &CoreCatalogEntry, platform: &str) -> bool {
    entry.platform == platform && entry.abi == INDEPENDENT_FAMILY_ABI_FINGERPRINT
}

pub(super) fn latest_eligible_release<'a>(
    entries: &'a [CoreCatalogEntry],
    plugin_id: &str,
    platform: &str,
    eligible: impl Fn(&CoreCatalogEntry) -> bool,
) -> Option<&'a CoreCatalogEntry> {
    entries
        .iter()
        .filter(|entry| entry.plugin_id == plugin_id && release_matches_target(entry, platform))
        .filter(|entry| eligible(entry))
        // Legacy provenance sorts ahead of SemVer. Rank only eligible releases.
        .max_by(|left, right| left.release_cmp(right))
}

impl ShinkiEmuManagerController {
    fn cores_dir(&self) -> Result<PathBuf, String> {
        core_directory(&self.data_dir)
    }

    fn allows_core_packages(plugin_id: &str) -> bool {
        recognition::supports_installed_packages(plugin_id)
    }

    /// Highest released version of a core for this platform and ABI.
    pub(super) fn online_core(&self, plugin_id: &str) -> Option<&CoreCatalogEntry> {
        if !Self::allows_core_packages(plugin_id) { return None; }
        latest_eligible_release(
            &self.metadata_catalog.as_ref()?.cores.entries,
            plugin_id,
            &core_platform(),
            |entry| recognition::allows_installed_release(plugin_id, &entry.version, entry.identity.as_ref()),
        )
    }

    /// The release online install, update and 全部更新 may fetch for a core;
    /// none while this build has online core downloads turned off.
    fn downloadable_core(&self, plugin_id: &str) -> Option<&CoreCatalogEntry> {
        if !online_core_downloads() { return None; }
        self.online_core(plugin_id)
    }
    pub(super) fn core_release<'a>(
        &'a self,
        plugin_id: &str,
    ) -> (&'a str, Option<&'a shinki_emu_metadata::CoreIdentity>, Option<&'a str>) {
        if let Some(core) = self.registry.installed().get(plugin_id) {
            return (&core.manifest.version, core.manifest.identity.as_ref(), core.source_commit.as_deref());
        }
        if let Some((version, source_commit)) = embedded_product_release(plugin_id) {
            return (version, None, Some(source_commit));
        }
        ("", embedded_identity(plugin_id), None)
    }

    /// Loaded, installed-but-unloaded, failed, pending and online-available cores.
    pub(super) fn cores_view(&self) -> Result<CoresViewModel, String> {
        let cores = self.cores_dir()?;
        let pending = core_package::pending_entries(&cores)?;
        #[cfg(target_os = "android")]
        let overlays: BTreeMap<_, _> = pending.iter()
            .filter(|(_, kind)| **kind == PendingKind::Install)
            .filter_map(|(stem, _)| {
                let package = core_package::read_package(&cores.join(core_package::PENDING).join(stem)).ok()?.manifest;
                (recognition::allows_installed_release(&package.plugin_id, &package.version, package.identity.as_ref())
                    && recognition::is_static(&package.plugin_id)
                    && !self.registry.installed().contains_key(&package.plugin_id))
                    .then_some((package.plugin_id, stem))
            })
            .collect();
        let mut items = Vec::new();
        let mut listed_plugins = BTreeSet::new();
        let mut listed_stems = BTreeSet::new();
        let mut ids: BTreeSet<String> = self.registry.installed().keys().cloned().collect();
        ids.extend(shinki_emu_manager::static_cores::STATIC_CORES.iter().map(|(id, _)| (*id).to_owned()));
        for id in ids {
            let installed = self.registry.installed().get(&id);
            let catalog = shinki_emu_manager_core::static_core_catalog::lookup(&id);
            let native = self.registry.descriptor(&id);
            let stem = installed
                .filter(|core| {
                    !core.bundled
                        || !cfg!(any(target_os = "android", target_os = "ios", target_env = "ohos"))
                })
                .and_then(|core| core.library.parent()?.file_name()?.to_str())
                .map(str::to_owned)
                .unwrap_or_default();
            #[cfg(target_os = "android")]
            let stem = overlays.get(&id).map_or(stem, |stem| String::clone(stem));
            let bundled = installed.map_or_else(
                || recognition::is_static(&id),
                |core| core.bundled || (!cfg!(target_os = "android") && recognition::is_static(&id)),
            );
            let pending_kind = pending.get(&stem);
            let rejected_stage = cfg!(target_os = "android")
                && pending_kind == Some(&PendingKind::Install)
                && self.plugin_errors.contains_key(&stem);
            let state = match pending_kind.filter(|_| !rejected_stage) {
                Some(PendingKind::Remove) => "pending-removal",
                Some(PendingKind::Install) => "pending-update",
                None if self.native_action.as_ref().is_some_and(|action| action.plugin_id() == id) => "loading",
                None if self.native_failures.contains_key(&id) => "failed",
                None if native.is_some() => "loaded",
                None => "installed",
            };
            let (version, identity, source_commit) = self.core_release(&id);
            let update_version = self.downloadable_core(&id).filter(|entry| {
                matches!(state, "installed" | "loaded") && entry.is_update_for(version, identity)
            }).map(|entry| core_card_version(&entry.version, entry.identity.as_ref())).unwrap_or_default();
            listed_plugins.insert(id.clone());
            listed_stems.insert(stem.clone());
            items.push(CoreViewModel {
                id: id.clone(),
                name: catalog.map(|entry| entry.display_name.to_owned()).or_else(|| native.map(|descriptor| descriptor.display_name.clone())).unwrap_or_else(|| id.clone()),
                engine: catalog.map(|entry| entry.family_id.to_owned()).or_else(|| native.map(|descriptor| descriptor.family_id.clone())).unwrap_or_default(),
                version: if identity.is_some() {
                    core_card_version(version, identity)
                } else {
                    product_version_label(version, source_commit)
                },
                capabilities: native.map(|descriptor| descriptor.capabilities.iter().copied().map(capability_label).map(str::to_owned).collect()).unwrap_or_default(),
                state: state.into(),
                source: if bundled { "bundled" } else { "user" }.into(),
                message: self.native_failures.get(&id).cloned().unwrap_or_default(),
                removable: !stem.is_empty() && !bundled && !rejected_stage
                    && matches!(state, "installed" | "loaded" | "failed"),
                update_version,
                download: self.download_view(&id),
                stem,
            });
        }
        for directory in recognition::directories(&self.data_dir)? {
            for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let Some(stem) = entry.file_name().to_str().map(str::to_owned) else { continue; };
                if stem.starts_with('.') || listed_stems.contains(&stem) || self.plugin_errors.contains_key(&stem)
                    || !entry.file_type().map_err(|error| error.to_string())?.is_dir() { continue; }
                let manifest = core_package::read_package(&entry.path()).ok().map(|package| package.manifest);
                listed_plugins.extend(manifest.map(|manifest| manifest.plugin_id));
                listed_stems.insert(stem.clone());
                let removing = directory == cores && pending.get(&stem) == Some(&PendingKind::Remove);
                items.push(CoreViewModel {
                    name: stem.clone(),
                    state: if removing { "pending-removal" } else { "failed" }.into(),
                    source: if directory == cores { "user" } else { "bundled" }.into(),
                    message: "SHINKI_EMU_CORE_PACKAGE_MANIFEST: missing or invalid package, library, or duplicate plugin identity; install an identified core package".into(),
                    removable: directory == cores && !removing,
                    stem,
                    ..CoreViewModel::default()
                });
            }
        }
        items.sort_by(|left, right| left.name.cmp(&right.name));
        let updates_available = items
            .iter()
            .filter(|item| !item.update_version.is_empty())
            .count();
        let update_all_count = items
            .iter()
            .filter(|item| {
                !item.update_version.is_empty() && !self.core_downloads.is_running(&item.id)
            })
            .count();
        for (stem, raw) in &self.plugin_errors {
            let described = describe_error(raw);
            let kind = pending.get(stem);
            let rejected_stage = cfg!(target_os = "android") && kind == Some(&PendingKind::Install);
            items.push(CoreViewModel {
                stem: stem.clone(),
                name: stem.clone(),
                state: if kind == Some(&PendingKind::Remove) { "pending-removal" } else { "failed" }.into(),
                source: "user".into(),
                message: described.title,
                removable: rejected_stage || (kind != Some(&PendingKind::Remove)
                    && !listed_stems.contains(stem) && cores.join(stem).is_dir()),
                ..CoreViewModel::default()
            });
        }
        for (stem, kind) in &pending {
            if *kind != PendingKind::Install || listed_stems.contains(stem)
                || (cfg!(target_os = "android") && self.plugin_errors.contains_key(stem))
            {
                continue;
            }
            let directory = cores.join(core_package::PENDING).join(stem);
            let package = core_package::read_package(&directory).ok();
            let plugin_id = package.as_ref().map(|package| package.manifest.plugin_id.clone());
            let online = plugin_id.as_deref().and_then(|id| self.online_core(id));
            let name = online.map_or_else(|| stem.clone(), |entry| entry.display_name.clone());
            let engine = online
                .map(|entry| entry.family_name.clone())
                .unwrap_or_default();
            listed_plugins.extend(plugin_id.clone());
            items.push(CoreViewModel {
                id: plugin_id.unwrap_or_default(),
                stem: stem.clone(),
                name,
                engine,
                version: package
                    .map(|package| match package.manifest.identity.as_ref() {
                        Some(identity) => core_card_version(&package.manifest.version, Some(identity)),
                        None => product_version_label(&package.manifest.version, package.source_commit.as_deref()),
                    })
                    .unwrap_or_default(),
                state: "pending-install".into(),
                source: "user".into(),
                removable: true,
                ..CoreViewModel::default()
            });
        }
        if let Some(catalog) = &self.metadata_catalog {
            let available: BTreeSet<&str> = catalog
                .cores
                .entries
                .iter()
                .map(|entry| entry.plugin_id.as_str())
                .filter(|plugin_id| !listed_plugins.contains(*plugin_id))
                .collect();
            for entry in available.into_iter().filter_map(|id| self.downloadable_core(id)) {
                items.push(CoreViewModel {
                    id: entry.plugin_id.clone(),
                    name: entry.display_name.clone(),
                    engine: entry.family_name.clone(),
                    version: core_card_version(&entry.version, entry.identity.as_ref()),
                    state: "available".into(),
                    source: "online".into(),
                    download: self.download_view(&entry.plugin_id),
                    ..CoreViewModel::default()
                });
            }
        }
        Ok(CoresViewModel {
            items,
            restart_required: if cfg!(target_os = "android") {
                pending.keys().any(|stem| !self.plugin_errors.contains_key(stem))
            } else {
                !pending.is_empty() && self.cores_started()
            },
            updates_available: i32::try_from(updates_available).unwrap_or(i32::MAX),
            update_all_count: i32::try_from(update_all_count).unwrap_or(i32::MAX),
            online_downloads: online_core_downloads(),
            batch: self.batch_view(),
            ..CoresViewModel::default()
        })
    }

    /// Stage a local ZIP matched against verified release metadata.
    pub(super) fn install_core(&mut self, source: &Path) -> Result<ManagerViewModel, String> {
        let pending = self.cores_dir()?.join(core_package::PENDING);
        let package = source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
        if !package {
            return Err("SHINKI_EMU_CORE_PACKAGE_FORMAT".into());
        }
        let platform = core_platform();
        let entries = self.metadata_catalog.as_ref().into_iter()
            .flat_map(|catalog| &catalog.cores.entries)
            .filter(|entry| release_matches_target(entry, &platform)
                && recognition::allows_installed_release(&entry.plugin_id, &entry.version, entry.identity.as_ref()))
            .map(core_package::LocalRelease::official);
        #[cfg(target_os = "android")]
        let qa = shinki_emu_manager::platform::android::qa_core_receipt();
        #[cfg(target_os = "android")]
        let entries = entries.chain(qa.iter().flat_map(|receipt| {
            receipt.cores.iter()
                .filter(|core| recognition::allows_installed_release(&core.plugin_id, &core.version, core.identity.as_ref()))
                .map(|core| core_package::LocalRelease::android_qa(receipt, core))
        }));
        let stem = core_package::stage_local_release(source, entries, &pending)?;
        if cfg!(target_os = "android") {
            self.plugin_errors.remove(&stem);
        }
        self.model()
    }

    fn download_view(&self, plugin_id: &str) -> CoreDownloadViewModel {
        if let Some(progress) = self.core_downloads.progress(plugin_id) {
            return CoreDownloadViewModel {
                phase: phase_name(progress.phase).into(),
                received: progress.received,
                total: progress.total,
                bytes_per_second: progress.bytes_per_second,
                error: String::new(),
            };
        }
        self.core_failures
            .get(plugin_id)
            .map(|raw| CoreDownloadViewModel {
                phase: "failed".into(),
                error: describe_error(raw).title,
                ..CoreDownloadViewModel::default()
            })
            .unwrap_or_default()
    }

    fn batch_view(&self) -> CoreBatchViewModel {
        let Some(batch) = &self.core_batch else {
            return CoreBatchViewModel::default();
        };
        let count = |set: &BTreeSet<String>| i32::try_from(set.len()).unwrap_or(i32::MAX);
        let done: f64 = batch
            .members
            .iter()
            .map(|(id, size)| {
                if batch.is_done(id) {
                    return 1.0;
                }
                match self.core_downloads.progress(id) {
                    Some(progress) if progress.phase != Phase::Downloading => 1.0,
                    Some(progress) if *size > 0 => progress.received as f64 / *size as f64,
                    _ => 0.0,
                }
            })
            .sum();
        CoreBatchViewModel {
            state: if batch.finished() { "finished" } else { "running" }.into(),
            total: i32::try_from(batch.members.len()).unwrap_or(i32::MAX),
            succeeded: count(&batch.succeeded),
            failed: count(&batch.failed),
            cancelled: count(&batch.cancelled),
            progress: if batch.members.is_empty() {
                0.0
            } else {
                (done / batch.members.len() as f64).clamp(0.0, 1.0) as f32
            },
        }
    }

    /// Queue the newest release of a core; returns its package size.
    fn start_core_download(&mut self, plugin_id: &str) -> Result<u64, String> {
        require_online_core_downloads()?;
        self.require_network()?;
        let entry = self
            .downloadable_core(plugin_id)
            .cloned()
            .ok_or_else(|| "SHINKI_EMU_CORE_NOT_AVAILABLE".to_owned())?;
        let wake = self
            .host_wake
            .clone()
            .ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        let size = entry.size;
        let pending = self.cores_dir()?.join(core_package::PENDING);
        self.core_downloads.start(entry, pending, wake)?;
        self.core_failures.remove(plugin_id);
        Ok(size)
    }

    /// Download the newest released package of a core into the pending area.
    pub(super) fn install_online_core(
        &mut self,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.start_core_download(plugin_id)?;
        if let Some(batch) = &mut self.core_batch {
            batch.forget(plugin_id);
            if batch.members.is_empty() {
                self.core_batch = None;
            }
        }
        self.model()
    }

    /// Plugin ids of installed cores 全部更新 would download now.
    fn updatable_cores(&self) -> Result<Vec<String>, String> {
        Ok(self
            .cores_view()?
            .items
            .into_iter()
            .filter(|item| {
                !item.update_version.is_empty() && !self.core_downloads.is_running(&item.id)
            })
            .map(|item| item.id)
            .collect())
    }

    /// Download every installed core that has a newer release, at most
    /// `core_download::MAX_PARALLEL` at a time.
    pub(super) fn update_all_cores(&mut self) -> Result<ManagerViewModel, String> {
        require_online_core_downloads()?;
        self.require_network()?;
        if self.core_batch.as_ref().is_some_and(|batch| !batch.finished()) {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_PENDING".into());
        }
        let targets = self.updatable_cores()?;
        self.start_core_batch(targets)
    }

    /// Start a new run with the failed members of the last one.
    pub(super) fn retry_failed_cores(&mut self) -> Result<ManagerViewModel, String> {
        require_online_core_downloads()?;
        self.require_network()?;
        let failed: Vec<String> = self
            .core_batch
            .as_ref()
            .filter(|batch| batch.finished())
            .map(|batch| batch.failed.iter().cloned().collect())
            .unwrap_or_default();
        self.start_core_batch(failed)
    }

    fn start_core_batch(&mut self, targets: Vec<String>) -> Result<ManagerViewModel, String> {
        if targets.is_empty() {
            return Err("SHINKI_EMU_CORE_NO_UPDATES".into());
        }
        let mut batch = CoreBatch::default();
        for plugin_id in targets {
            match self.start_core_download(&plugin_id) {
                Ok(size) => {
                    batch.members.insert(plugin_id, size);
                }
                Err(error) => {
                    batch.members.insert(plugin_id.clone(), 0);
                    batch.failed.insert(plugin_id.clone());
                    self.core_failures.insert(plugin_id, error);
                }
            }
        }
        tracing::info!(
            event = "shinki.emu.manager.core_batch_started",
            cores = batch.members.len()
        );
        self.core_batch = Some(batch);
        self.summarize_core_batch();
        self.model()
    }

    pub(super) fn cancel_core_download(
        &mut self,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        if self.core_downloads.cancel(plugin_id)? {
            match &mut self.core_batch {
                Some(batch) if batch.members.contains_key(plugin_id) => {
                    batch.cancelled.insert(plugin_id.into());
                    self.summarize_core_batch();
                }
                _ => self.show_snackbar(tr!("已取消下载"), None),
            }
        }
        self.model()
    }

    /// Cancel every member still queued or transferring; members already
    /// being installed finish normally.
    pub(super) fn cancel_core_batch(&mut self) -> Result<ManagerViewModel, String> {
        let running: Vec<String> = self
            .core_batch
            .as_ref()
            .map(|batch| {
                batch
                    .members
                    .keys()
                    .filter(|id| !batch.is_done(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for plugin_id in running {
            if self.core_downloads.cancel(&plugin_id).unwrap_or(false)
                && let Some(batch) = &mut self.core_batch
            {
                batch.cancelled.insert(plugin_id);
            }
        }
        self.summarize_core_batch();
        self.model()
    }

    /// Hide the summary of a finished run.
    pub(super) fn dismiss_core_batch(&mut self) -> Result<ManagerViewModel, String> {
        if self.core_batch.as_ref().is_some_and(CoreBatch::finished) {
            self.core_batch = None;
        }
        self.model()
    }

    /// Collect finished downloads; true while anything changed or runs.
    pub(super) fn poll_core_downloads(&mut self) -> bool {
        let finished = self.core_downloads.finished();
        let changed = !finished.is_empty() || !self.core_downloads.is_idle();
        for done in finished {
            let batch = self
                .core_batch
                .as_mut()
                .filter(|batch| batch.members.contains_key(&done.plugin_id));
            match done.result {
                Ok(stem) => {
                    self.core_failures.remove(&done.plugin_id);
                    if cfg!(target_os = "android") {
                        self.plugin_errors.remove(&stem);
                    }
                    tracing::info!(
                        event = "shinki.emu.manager.core_staged",
                        core = %stem,
                        version = %done.version
                    );
                    match batch {
                        Some(batch) => {
                            batch.succeeded.insert(done.plugin_id);
                        }
                        None => self.show_snackbar(
                            tr!("核心已下载，重启后生效"),
                            Some(snackbar::SnackbarAction::Restart),
                        ),
                    }
                }
                Err(error) => {
                    let reason = describe_error(&error).title;
                    self.core_failures.insert(done.plugin_id.clone(), error);
                    match batch {
                        Some(batch) => {
                            batch.failed.insert(done.plugin_id);
                        }
                        None => self.show_snackbar(
                            tr!("核心下载失败：{reason}", reason = reason),
                            None,
                        ),
                    }
                }
            }
        }
        self.summarize_core_batch();
        changed
    }

    /// One snackbar per finished run, offering a single restart.
    fn summarize_core_batch(&mut self) {
        let Some(batch) = &mut self.core_batch else {
            return;
        };
        if batch.summarized || !batch.finished() {
            return;
        }
        batch.summarized = true;
        let (succeeded, failed) = (batch.succeeded.len(), batch.failed.len());
        tracing::info!(
            event = "shinki.emu.manager.core_batch_finished",
            succeeded,
            failed,
            cancelled = batch.cancelled.len()
        );
        let text = match (succeeded, failed) {
            (0, 0) => tr!("已取消全部更新").to_owned(),
            (_, 0) => tr!("已更新 {count} 个核心，重启后生效", count = succeeded),
            (0, _) => tr!("{count} 个核心更新失败，可以在核心页重试", count = failed),
            _ => tr!(
                "已更新 {succeeded} 个核心，{failed} 个失败",
                succeeded = succeeded,
                failed = failed
            ),
        };
        let action = (succeeded > 0).then_some(snackbar::SnackbarAction::Restart);
        self.show_snackbar(text, action);
    }

    /// Cancel a pending install, or mark an installed user core for removal.
    pub(super) fn uninstall_core(&mut self, stem: &str) -> Result<ManagerViewModel, String> {
        let view = self.cores_view()?;
        let card = view.items.iter()
            .find(|item| !stem.is_empty() && item.stem == stem && item.removable)
            .ok_or("SHINKI_EMU_CORE_UNINSTALL_BUNDLED")?;
        let cores = self.cores_dir()?;
        let staged = cores.join(core_package::PENDING).join(stem);
        let installed = cores.join(stem);
        let rejected_stage = cfg!(target_os = "android") && self.plugin_errors.contains_key(stem);
        let staged = fs::symlink_metadata(staged).is_ok_and(|metadata| metadata.is_dir());
        let installed = fs::symlink_metadata(installed).is_ok_and(|metadata| metadata.is_dir());
        if staged && (card.state == "pending-install" || !installed || rejected_stage) {
            core_package::cancel_pending_install(&cores, stem)?;
            self.plugin_errors.remove(stem);
        } else if installed {
            core_package::mark_removal(&cores, stem)?;
        } else {
            return Err("SHINKI_EMU_CORE_UNINSTALL_BUNDLED".into());
        }
        self.model()
    }

    /// Filter diagnostics directly by directory stem, or by ID for a bundled
    /// core without a package directory. An empty filter returns all logs.
    pub(super) fn log_text(&self, core: &str) -> Result<(String, usize), String> {
        let path = self.data_dir.join("diagnostics").join("roukaku.jsonl");
        let mut file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((String::new(), 0));
            }
            Err(_) => return Err("SHINKI_EMU_LOG_READ".into()),
        };
        let len = file.metadata().map_err(|_| "SHINKI_EMU_LOG_READ")?.len();
        let start = len.saturating_sub(1024 * 1024);
        file.seek(SeekFrom::Start(start))
            .map_err(|_| "SHINKI_EMU_LOG_READ")?;
        let mut reader = io::BufReader::new(file);
        if start != 0 {
            let mut discarded = String::new();
            reader
                .read_line(&mut discarded)
                .map_err(|_| "SHINKI_EMU_LOG_READ")?;
        }
        let mut kept = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => return Err("SHINKI_EMU_LOG_READ".into()),
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            if core.is_empty() || log_matches_core(&value, core) {
                kept.push(render_log_line(&value));
            }
        }
        let count = kept.len();
        let skip = count.saturating_sub(200);
        let mut lines = kept.split_off(skip);
        lines.reverse();
        Ok((lines.join("\n"), count))
    }

    pub(super) fn engine_action(&mut self, control: u32) -> Result<(), String> {
        let (epoch, plugin_id) = {
            let active = self
                .active
                .as_ref()
                .filter(|active| active.opened && active.closing.is_none())
                .ok_or_else(|| "SHINKI_EMU_ENGINE_ACTION_UNAVAILABLE".to_owned())?;
            let plugin_id = self
                .candidates
                .get(&active.game_id)
                .map(|candidate| candidate.plugin_id.clone())
                .ok_or_else(|| "SHINKI_EMU_ENGINE_ACTION_UNAVAILABLE".to_owned())?;
            (active.epoch, plugin_id)
        };
        let listed = self.registry.core_metadata().any(|meta| {
            meta.descriptor.plugin_id == plugin_id
                && meta.menu.iter().any(|item| item.control == control)
        });
        if !listed {
            return Err("SHINKI_EMU_ENGINE_ACTION_UNAVAILABLE".into());
        }
        self.action_message.clear();
        self.registry.action(epoch, control)
    }
}

fn log_event(value: &serde_json::Value) -> Option<&str> {
    value
        .get("event")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("fields")
                .and_then(|fields| fields.get("event"))
                .and_then(serde_json::Value::as_str)
        })
}

fn log_fields(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value.get("fields")
}

fn json_contains_core(value: &serde_json::Value, core: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text_mentions_core(text, core),
        serde_json::Value::Array(items) => items.iter().any(|item| json_contains_core(item, core)),
        serde_json::Value::Object(map) => map.values().any(|item| json_contains_core(item, core)),
        _ => false,
    }
}

fn text_mentions_core(text: &str, core: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')))
        .any(|word| {
            let word = word.trim_end_matches('.');
            word == core || [".dll", ".so", ".dylib", ".remove"].into_iter()
                .any(|suffix| word.strip_suffix(suffix) == Some(core))
        })
}

fn log_matches_core(value: &serde_json::Value, core: &str) -> bool {
    log_event(value).is_some_and(|event| event.contains(core))
        || log_fields(value).is_some_and(|fields| json_contains_core(fields, core))
}

fn render_field_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn render_log_line(value: &serde_json::Value) -> String {
    let timestamp = value
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let stamp = if timestamp.len() >= 19 {
        timestamp[5..19].replace('T', " ")
    } else {
        timestamp.replace('T', " ")
    };
    let level = value
        .get("level")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let event = log_event(value).unwrap_or("");
    let mut pairs = Vec::new();
    if let Some(fields) = log_fields(value).and_then(serde_json::Value::as_object) {
        let mut keys: Vec<_> = fields.keys().cloned().collect();
        keys.sort();
        for key in keys {
            if let Some(field) = fields.get(&key) {
                pairs.push(format!("{key}={}", render_field_value(field)));
            }
        }
    }
    let rendered = format!("{stamp} {level} {event}");
    if pairs.is_empty() {
        rendered
    } else {
        format!("{rendered} {}", pairs.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(feature = "no-core-download"))]
    fn cores_view_lists_online_cores_and_gates_downloads() {
        let root = tempfile::tempdir().unwrap();
        let mut app = ShinkiEmuManagerController::open_library(
            root.path().into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        let entry = |version: &str| CoreCatalogEntry {
            plugin_id: "shinki.emu.musica".into(),
            display_name: "Shinki Musica".into(),
            family_name: "Musica".into(),
            version: version.into(),
            abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
            platform: core_platform(),
            url: format!(
                "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/emu-v{version}/core.zip"
            ),
            sha256: "a".repeat(64),
            size: 1,
            license: "MIT".into(),
            release_tag: format!("emu-v{version}"),
            identity: None,
        };
        let mut catalog = crate::metadata_cache::tests::sample_catalog(root.path());
        catalog.cores.entries = vec![entry("0.2.0"), entry("0.3.0")];
        app.metadata_catalog = Some(catalog);
        let view = app.cores_view().unwrap();
        let online = view.items.iter().find(|item| item.id == "shinki.emu.musica").unwrap();
        assert_eq!(online.state, "available");
        assert_eq!(online.source, "online");
        assert_eq!(online.version, "0.3.0");
        assert_eq!(view.updates_available, 0);
        assert_eq!(
            app.install_online_core("shinki.emu.musica").unwrap_err(),
            "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED"
        );
        app.network_allowed = true;
        assert_eq!(
            app.install_online_core("shinki.emu.other").unwrap_err(),
            "SHINKI_EMU_CORE_NOT_AVAILABLE"
        );
        assert!(entry("0.10.0").is_update_for("0.9.1", None));
        assert!(!entry("0.2.0").is_update_for("0.2.0", None));
    }

    #[test]
    fn log_text_filters_by_core_and_counts_all() {
        let root = tempfile::tempdir().unwrap();
        let diagnostics = root.path().join("diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        fs::write(
            diagnostics.join("roukaku.jsonl"),
            concat!(
                r#"{"timestamp":"2024-01-02T03:04:05.000Z","level":"INFO","event":"alpha","fields":{"x":"1"}}"#,
                "\n",
                r#"{"timestamp":"2024-01-02T03:04:06.000Z","level":"ERROR","event":"plugin","fields":{"core":"shinki_emu_rfvp.dll"}}"#,
                "\n",
                r#"{"timestamp":"2024-01-02T03:04:07.000Z","level":"WARN","event":"gamma","fields":{"y":"2"}}"#,
                "\n",
            ),
        )
        .unwrap();
        let app = ShinkiEmuManagerController::open_library(
            root.path().into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        let (_, count) = app.log_text("shinki_emu_rfvp.dll").unwrap();
        assert_eq!(count, 1);
        let (_, count_all) = app.log_text("").unwrap();
        assert_eq!(count_all, 3);
    }

    use crate::core_download::{
        CoreDownloads,
        tests::{Served, entry as package_entry, package_for, package_with_recognition, serve},
    };

    const PLUGINS: [(&str, &str); 3] = [
        ("rfvp", "shinki_emu_alpha"),
        ("siglus_rs", "shinki_emu_beta"),
        ("ethornell-rs", "shinki_emu_gamma"),
    ];

    /// A controller whose catalog lists the three fixture cores and whose
    /// downloads come from memory.
    fn online_app(root: &Path) -> (ShinkiEmuManagerController, BTreeMap<String, Vec<u8>>) {
        let mut app = ShinkiEmuManagerController::open_library(
            root.into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap();
        let packages: BTreeMap<String, Vec<u8>> = PLUGINS
            .iter()
            .map(|(id, stem)| ((*id).to_owned(), package_for(id, stem)))
            .collect();
        let mut catalog = crate::metadata_cache::tests::sample_catalog(root);
        catalog.cores.entries = packages
            .iter()
            .map(|(id, bytes)| package_entry(id, bytes))
            .collect();
        app.metadata_catalog = Some(catalog);
        app.network_allowed = true;
        app.host_wake = Some(std::sync::Arc::new(|| {}));
        (app, packages)
    }

    /// Poll until every download ended and its result was applied.
    fn settle(app: &mut ShinkiEmuManagerController) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let changed = app.poll_core_downloads();
            if !changed && app.core_downloads.is_idle() {
                break;
            }
            assert!(Instant::now() < deadline, "downloads did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn item(app: &ShinkiEmuManagerController, id: &str) -> CoreViewModel {
        app.cores_view()
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == id)
            .unwrap()
    }

    #[test]
    fn core_cards_shorten_only_validated_commit_fields() {
        use shinki_emu_metadata::{AdapterIdentity, UpstreamIdentity, UpstreamRefKind};

        let mut identity = CoreIdentity {
            upstream: UpstreamIdentity {
                repo: "https://github.com/upstream/engine".into(),
                ref_kind: UpstreamRefKind::Tag,
                ref_name: "v0.6.1-beta.12".into(),
                commit: "02c366a518d278eb4c2746c107baea061203c98d".into(),
            },
            adapter: AdapterIdentity {
                repo: "https://github.com/Project-Shinkirou/engine".into(),
                branch: "android/runtime".into(),
                commit: "e83a23bf7d03a36b5d3d53e2dc1752fd5f428dd1".into(),
            },
            release_sequence: 17,
        };
        identity.validate().unwrap();
        let original = identity.version();
        assert_eq!(
            core_card_version(&original, Some(&identity)),
            "v0.6.1-beta.12@02c366a5 (adapter e83a23bf)"
        );
        assert_eq!(identity.version(), original);

        identity.upstream.ref_kind = UpstreamRefKind::Branch;
        identity.upstream.ref_name = "feature/02c366a518d278eb".into();
        identity.validate().unwrap();
        assert_eq!(
            core_card_version(&identity.version(), Some(&identity)),
            "feature/02c366a518d278eb@02c366a5 (adapter e83a23bf)"
        );
        let semver = "0.6.1-beta.12+e83a23bf7d03a36b";
        assert_eq!(core_card_version(semver, None), semver);
    }

    #[test]
    fn restart_applies_install_update_and_removal_before_prepare() {
        let root = tempfile::tempdir().unwrap();
        let (id, stem) = PLUGINS[0];
        let archive = root.path().join("official.zip");
        let package = package_for(id, stem);
        let open = || {
            ShinkiEmuManagerController::open_library(
                root.path().into(),
                FrameMailbox::new(),
                Vec::new(),
            )
            .unwrap()
        };
        let mut app = open();
        let cores = root.path().join("cores");
        fs::write(&archive, &package).unwrap();
        assert!(app.install_core(&archive).is_err(), "self-hashes do not authorize a ZIP");
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        let mut catalog = crate::metadata_cache::tests::sample_catalog(root.path());
        catalog.cores.entries = vec![package_entry(id, &package)];
        app.metadata_catalog = Some(catalog);
        assert!(!app.network_allowed);
        let bare = root.path().join(format!("{stem}.{}", env::consts::DLL_EXTENSION));
        fs::write(&bare, b"unidentified library bytes").unwrap();
        assert!(app.install_core(&bare).is_err());
        let mut altered = package.clone();
        altered[0] ^= 1;
        fs::write(&archive, altered).unwrap();
        assert!(app.install_core(&archive).is_err(), "same-size archive still needs the official SHA");
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        fs::write(&archive, &package).unwrap();
        app.install_core(&archive).unwrap();
        assert_eq!(item(&app, id).state, "pending-install");
        drop(app);

        let mut app = open();
        assert!(!app.cores_started());
        assert_eq!(item(&app, id).state, "installed");
        assert!(!app.cores_view().unwrap().restart_required);
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        assert_eq!(
            fs::read(cores.join(stem).join(format!("{stem}.{}", env::consts::DLL_EXTENSION))).unwrap(),
            b"library bytes"
        );

        let update = package_with_recognition(id, stem, "0.3.0", Default::default());
        fs::write(&archive, &update).unwrap();
        let mut entry = package_entry(id, &update);
        entry.version = "0.3.0".into();
        let mut catalog = crate::metadata_cache::tests::sample_catalog(root.path());
        catalog.cores.entries = vec![entry];
        app.metadata_catalog = Some(catalog);
        app.install_core(&archive).unwrap();
        assert_eq!(item(&app, id).state, "pending-update");
        drop(app);

        let mut app = open();
        let installed = item(&app, id);
        assert_eq!(installed.state, "installed");
        assert_eq!(installed.version, "0.3.0");
        assert!(!app.cores_started());
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());

        app.uninstall_core(&installed.stem).unwrap();
        assert_eq!(item(&app, id).state, "pending-removal");
        drop(app);
        let app = open();
        assert!(!cores.join(stem).exists());
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        assert!(!app.cores_view().unwrap().items.iter().any(|item| item.id == id));
    }

    fn stage_fixture(root: &Path, id: &str, stem: &str) {
        let archive = root.join("fixture.zip");
        fs::write(&archive, package_for(id, stem)).unwrap();
        core_package::stage_package(&archive, None, &root.join("cores").join(core_package::PENDING))
            .unwrap();
    }

    #[test]
    fn duplicate_plugin_rows_cancel_remove_and_filter_the_selected_directory() {
        let root = tempfile::tempdir().unwrap();
        let (id, keep) = PLUGINS[0];
        let remove = "shinki_emu_alpha_old";
        let cores = root.path().join("cores");
        let open = || {
            let mut app = ShinkiEmuManagerController::open_library(
                root.path().into(), FrameMailbox::new(), Vec::new(),
            ).unwrap();
            let mut catalog = crate::metadata_cache::tests::sample_catalog(root.path());
            catalog.cores.entries = vec![package_entry(id, &package_for(id, keep))];
            app.metadata_catalog = Some(catalog);
            app
        };
        let mut app = open();
        for stem in [keep, remove] {
            stage_fixture(root.path(), id, stem);
        }
        let view = app.cores_view().unwrap();
        let selected = view.items.iter().find(|item| item.stem == remove).unwrap();
        assert_eq!(selected.id, id);
        assert_eq!(selected.state, "pending-install");
        app.uninstall_core(&selected.stem).unwrap();
        assert_eq!(core_package::pending_entries(&cores).unwrap(),
            BTreeMap::from([(keep.into(), PendingKind::Install)]));
        stage_fixture(root.path(), id, remove);
        drop(app);

        let mut app = open();
        for stem in [keep, remove] {
            assert_eq!(core_package::read_manifest(&cores.join(stem)).unwrap().plugin_id, id);
        }
        let view = app.cores_view().unwrap();
        let remaining = view.items.iter().find(|item| item.stem == keep).unwrap();
        let selected = view.items.iter().find(|item| item.stem == remove).unwrap();
        assert_eq!(remaining.state, "failed");
        assert_eq!(selected.state, "failed");
        assert!(selected.removable);
        let diagnostics = root.path().join("diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        fs::write(diagnostics.join("roukaku.jsonl"), concat!(
            r#"{"event":"keep-log","fields":{"path":"cores/shinki_emu_alpha/shinki_emu_alpha.dll"}}"#,
            "\n",
            r#"{"event":"remove-log","fields":{"path":"cores/shinki_emu_alpha_old/shinki_emu_alpha_old.dll"}}"#,
            "\n",
        )).unwrap();
        let (log, count) = app.log_text(&remaining.stem).unwrap();
        assert_eq!(count, 1);
        assert!(log.contains("keep-log"));
        let (log, count) = app.log_text(&selected.stem).unwrap();
        assert_eq!(count, 1);
        assert!(log.contains("remove-log"));
        app.uninstall_core(&selected.stem).unwrap();
        assert_eq!(core_package::pending_entries(&cores).unwrap(),
            BTreeMap::from([(remove.into(), PendingKind::Remove)]));
        drop(app);

        let mut app = open();
        assert!(!cores.join(remove).exists());
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        let installed = item(&app, id);
        assert_eq!((installed.stem.as_str(), installed.state.as_str(), installed.version.as_str()),
            (keep, "installed", "0.2.0"));
        assert!(!app.cores_started());
        app.host_wake = Some(std::sync::Arc::new(|| {}));
        app.diagnose_core(id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.native_action.is_some() {
            app.poll_background();
            assert!(Instant::now() < deadline, "native load did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        let failed = item(&app, id);
        assert_eq!((failed.stem.as_str(), failed.state.as_str(), failed.version.as_str()),
            (keep, "failed", "0.2.0"));
        drop(app);

        let app = open();
        let reopened = item(&app, id);
        assert_eq!((reopened.stem.as_str(), reopened.state.as_str(), reopened.version.as_str()),
            (keep, "installed", "0.2.0"));
        assert!(app.cores_view().unwrap().items.iter().all(|item| item.id != id || item.state != "available"));
    }

    #[test]
    fn error_notice_logs_use_the_failed_stem_even_if_the_inventory_is_unreadable() {
        let root = tempfile::tempdir().unwrap();
        let (id, installed) = PLUGINS[0];
        let pending = root.path().join("cores").join(core_package::PENDING);
        stage_fixture(root.path(), id, installed);
        fs::create_dir(pending.join(id)).unwrap();
        let mut app = ShinkiEmuManagerController::open_library(
            root.path().into(), FrameMailbox::new(), Vec::new(),
        ).unwrap();
        assert_eq!(item(&app, id).state, "installed");
        let model = app.model().unwrap();
        let notice = model.notices.items.iter().find(|notice| notice.core == id).unwrap();
        let diagnostics = root.path().join("diagnostics");
        fs::create_dir_all(&diagnostics).unwrap();
        fs::write(diagnostics.join("roukaku.jsonl"), concat!(
            r#"{"event":"invalid-package","fields":{"core":"rfvp"}}"#, "\n",
            r#"{"event":"working-package","fields":{"core":"shinki_emu_alpha"}}"#, "\n",
        )).unwrap();
        let (log, count) = app.log_text(&notice.core).unwrap();
        assert_eq!(count, 1);
        assert!(log.contains("invalid-package"));
        fs::remove_dir_all(&pending).unwrap();
        fs::write(pending, b"not a directory").unwrap();
        assert_eq!(app.log_text(&notice.core).unwrap(), (log, count));
    }

    #[test]
    fn bundled_card_cannot_remove_a_user_directory_with_the_same_stem() {
        let data = tempfile::tempdir().unwrap();
        let bundle = tempfile::tempdir().unwrap();
        let (id, stem) = PLUGINS[0];
        let user_cores = data.path().join("cores");
        let bundled_cores = bundle.path().join("cores");
        stage_fixture(bundle.path(), id, stem);
        assert!(core_package::apply_pending(&bundled_cores).is_empty());
        let mut app = ShinkiEmuManagerController::open_library(
            data.path().into(), FrameMailbox::new(), Vec::new(),
        ).unwrap();
        app.registry.scan_installed_for_test(&[user_cores.clone(), bundled_cores.clone()]);
        fs::create_dir(user_cores.join(stem)).unwrap();

        let card = item(&app, id);
        assert_eq!((card.source.as_str(), card.stem.as_str()), ("bundled", stem));
        assert!(!card.removable);
        assert!(app.uninstall_core(&card.stem).is_err());
        assert!(core_package::pending_entries(&user_cores).unwrap().is_empty());
        assert!(user_cores.join(stem).is_dir());
        assert!(core_package::read_manifest(&bundled_cores.join(stem)).is_some());
    }

    #[test]
    #[cfg(not(feature = "no-core-download"))]
    fn download_shows_byte_progress_then_stages_with_one_restart_prompt() {
        let root = tempfile::tempdir().unwrap();
        let (mut app, packages) = online_app(root.path());
        let (id, stem) = PLUGINS[0];
        let gate = std::sync::Arc::new(tokio::sync::Notify::new());
        app.core_downloads = CoreDownloads::new(serve(Served {
            packages: packages.clone(),
            gates: BTreeMap::from([(id.to_owned(), std::sync::Arc::clone(&gate))]),
            failing: Vec::new(),
        }));
        app.install_online_core(id).unwrap();
        assert_eq!(
            app.install_online_core(id).unwrap_err(),
            "SHINKI_EMU_CORE_DOWNLOAD_PENDING"
        );
        let half = (packages[id].len() / 2) as u64;
        let deadline = Instant::now() + Duration::from_secs(10);
        while item(&app, id).download.received != half {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let downloading = item(&app, id).download;
        assert_eq!(downloading.phase, "downloading");
        assert_eq!(downloading.total, packages[id].len() as u64);
        gate.notify_one();
        settle(&mut app);
        let staged = item(&app, id);
        assert_eq!(staged.state, "pending-install");
        assert_eq!(staged.download.phase, "");
        assert!(root.path().join("cores").join(core_package::PENDING).join(stem).is_dir());
        let snackbar = app.snackbar.view();
        assert_eq!(snackbar.text, tr!("核心已下载，重启后生效"));
        assert_eq!(snackbar.action, tr!("立即重启"));
    }

    #[test]
    #[cfg(not(feature = "no-core-download"))]
    fn cancel_leaves_no_package_and_records_no_failure() {
        let root = tempfile::tempdir().unwrap();
        let (mut app, packages) = online_app(root.path());
        let (id, _) = PLUGINS[1];
        let gate = std::sync::Arc::new(tokio::sync::Notify::new());
        app.core_downloads = CoreDownloads::new(serve(Served {
            packages: packages.clone(),
            gates: BTreeMap::from([(id.to_owned(), gate)]),
            failing: Vec::new(),
        }));
        app.install_online_core(id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while item(&app, id).download.received == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        app.cancel_core_download(id).unwrap();
        assert!(app.core_downloads.is_idle());
        let cancelled = item(&app, id);
        assert_eq!(cancelled.state, "available");
        assert_eq!(cancelled.download.phase, "");
        assert_eq!(app.snackbar.view().text, tr!("已取消下载"));
        let pending = root.path().join("cores").join(core_package::PENDING);
        while std::fs::read_dir(&pending).is_ok_and(|mut entries| entries.next().is_some()) {
            assert!(Instant::now() < deadline, "cancelled download left files");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(core_package::pending_entries(&root.path().join("cores")).unwrap().is_empty());
    }

    #[test]
    #[cfg(not(feature = "no-core-download"))]
    fn batch_continues_past_a_failure_summarizes_once_and_retries_the_failed() {
        let root = tempfile::tempdir().unwrap();
        let (mut app, packages) = online_app(root.path());
        let failing = PLUGINS[2].0;
        app.core_downloads = CoreDownloads::new(serve(Served {
            packages: packages.clone(),
            gates: BTreeMap::new(),
            failing: vec![failing.to_owned()],
        }));
        app.start_core_batch(PLUGINS.iter().map(|(id, _)| (*id).to_owned()).collect())
            .unwrap();
        assert_eq!(app.batch_view().state, "running");
        assert_eq!(
            app.update_all_cores().unwrap_err(),
            "SHINKI_EMU_CORE_DOWNLOAD_PENDING"
        );
        let serial = app.snackbar.view().serial;
        settle(&mut app);
        let batch = app.batch_view();
        assert_eq!(
            (batch.state.as_str(), batch.total, batch.succeeded, batch.failed),
            ("finished", 3, 2, 1)
        );
        assert_eq!(batch.progress, 1.0);
        let snackbar = app.snackbar.view();
        assert_eq!(snackbar.serial, serial.wrapping_add(1), "one summary per run");
        assert_eq!(
            snackbar.text,
            tr!("已更新 {succeeded} 个核心，{failed} 个失败", succeeded = 2, failed = 1)
        );
        assert_eq!(snackbar.action, tr!("立即重启"));
        assert_eq!(item(&app, failing).download.phase, "failed");
        for (id, _) in &PLUGINS[..2] {
            assert_eq!(item(&app, id).state, "pending-install");
        }

        app.core_downloads = CoreDownloads::new(serve(Served {
            packages,
            gates: BTreeMap::new(),
            failing: Vec::new(),
        }));
        app.retry_failed_cores().unwrap();
        settle(&mut app);
        let retried = app.batch_view();
        assert_eq!((retried.total, retried.succeeded, retried.failed), (1, 1, 0));
        assert_eq!(item(&app, failing).state, "pending-install");
        assert!(app.core_failures.is_empty());
        app.dismiss_core_batch().unwrap();
        assert_eq!(app.batch_view().state, "");
    }

    #[test]
    #[cfg(not(feature = "no-core-download"))]
    fn cancelling_a_batch_stops_queued_members() {
        let root = tempfile::tempdir().unwrap();
        let (mut app, packages) = online_app(root.path());
        let gates: BTreeMap<String, std::sync::Arc<tokio::sync::Notify>> = PLUGINS
            .iter()
            .map(|(id, _)| ((*id).to_owned(), std::sync::Arc::new(tokio::sync::Notify::new())))
            .collect();
        app.core_downloads = CoreDownloads::new(serve(Served {
            packages,
            gates,
            failing: Vec::new(),
        }));
        app.start_core_batch(PLUGINS.iter().map(|(id, _)| (*id).to_owned()).collect())
            .unwrap();
        let queued = PLUGINS
            .iter()
            .filter(|(id, _)| item(&app, id).download.phase == "queued")
            .count();
        assert!(queued >= 1, "at most {} run at once", crate::core_download::MAX_PARALLEL);
        app.cancel_core_batch().unwrap();
        settle(&mut app);
        let batch = app.batch_view();
        assert_eq!((batch.state.as_str(), batch.cancelled), ("finished", 3));
        assert_eq!(app.snackbar.view().text, tr!("已取消全部更新"));
        assert_eq!(app.snackbar.view().action, "");
    }

    /// Default builds list and fetch catalog releases; `no-core-download`
    /// builds list installed cores only, never call the fetcher and still
    /// accept local official packages. Run under both feature sets.
    #[test]
    fn online_core_downloads_follow_the_build_switch() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempfile::tempdir().unwrap();
        let (mut app, packages) = online_app(root.path());
        let fetches = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&fetches);
        let served = serve(Served { packages: packages.clone(), gates: BTreeMap::new(), failing: Vec::new() });
        app.core_downloads = CoreDownloads::new(std::sync::Arc::new(move |entry: CoreCatalogEntry| {
            counter.fetch_add(1, Ordering::SeqCst);
            served(entry)
        }));
        let cores = root.path().join("cores");
        let (id, stem) = PLUGINS[0];
        let view = app.cores_view().unwrap();
        assert_eq!(view.online_downloads, online_core_downloads());
        if online_core_downloads() {
            assert!(view.items.iter().any(|item| item.id == id && item.state == "available"));
            app.install_online_core(id).unwrap();
            settle(&mut app);
            assert_eq!(fetches.load(Ordering::SeqCst), 1);
            assert_eq!(item(&app, id).state, "pending-install");
            return;
        }
        assert!(view.items.iter().all(|item| item.state != "available" && item.update_version.is_empty()));
        assert_eq!((view.updates_available, view.update_all_count), (0, 0));
        for result in [app.install_online_core(id), app.update_all_cores(), app.retry_failed_cores()] {
            assert_eq!(result.unwrap_err(), "SHINKI_EMU_CORE_DOWNLOAD_DISABLED");
        }
        assert!(app.core_downloads.is_idle());
        assert_eq!(fetches.load(Ordering::SeqCst), 0, "no fetch while downloads are off");
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());

        let archive = root.path().join("official.zip");
        fs::write(&archive, &packages[id]).unwrap();
        app.install_core(&archive).unwrap();
        assert_eq!(item(&app, id).state, "pending-install");
        assert!(cores.join(core_package::PENDING).join(stem).is_dir());
    }
}
