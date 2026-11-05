use super::*;
use crate::core_download::Phase;
use crate::core_package::{self, PendingKind};
use shinki_emu_manager_core::{FamilyCapability, INDEPENDENT_FAMILY_ABI_FINGERPRINT};
use shinki_emu_manager_ui_slint::tr;
use shinki_emu_manager_ui_slint::{
    CoreBatchViewModel, CoreDownloadViewModel, CoreViewModel, CoresViewModel, describe_error,
};
use shinki_emu_metadata::CoreCatalogEntry;
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

impl ShinkiEmuManagerController {
    fn cores_dir(&self) -> PathBuf {
        self.data_dir.join("cores")
    }

    /// Highest released version of a core for this platform and ABI.
    pub(super) fn online_core(&self, plugin_id: &str) -> Option<&CoreCatalogEntry> {
        self.metadata_catalog.as_ref()?.cores.latest(
            plugin_id,
            &core_platform(),
            INDEPENDENT_FAMILY_ABI_FINGERPRINT,
        )
    }
    pub(super) fn core_release<'a>(
        &'a self,
        descriptor: &'a FamilyPluginDescriptor,
    ) -> (&'a str, Option<&'a shinki_emu_metadata::CoreIdentity>) {
        self.registry
            .core_metadata()
            .find(|meta| meta.descriptor.plugin_id == descriptor.plugin_id)
            .map_or((descriptor.version.as_str(), None), |meta| {
                (
                    meta.release_version.as_str(),
                    meta.release_identity.as_ref(),
                )
            })
    }

    /// Loaded, installed-but-unloaded, failed, pending and online-available
    /// cores, plus each item's directory stem (empty for online-only entries).
    fn core_items(&self) -> Result<(CoresViewModel, Vec<String>), String> {
        let cores = self.cores_dir();
        let pending = core_package::pending_entries(&cores)?;
        let mut items = Vec::new();
        let mut listed_plugins = BTreeSet::new();
        let mut listed_stems = BTreeSet::new();
        for meta in self.registry.core_metadata() {
            listed_stems.insert(meta.stem.clone());
            listed_plugins.insert(meta.descriptor.plugin_id.clone());
            let state = match pending.get(&meta.stem) {
                Some(PendingKind::Remove) => "pending-removal",
                Some(PendingKind::Install) => "pending-update",
                None => "loaded",
            };
            let installed_version = meta.release_version.as_str();
            let installed_identity = meta.release_identity.as_ref();
            let update_version = self
                .online_core(&meta.descriptor.plugin_id)
                .filter(|entry| {
                    state == "loaded" && entry.is_update_for(installed_version, installed_identity)
                })
                .map(CoreCatalogEntry::display_version)
                .unwrap_or_default();
            items.push((
                CoreViewModel {
                    id: meta.descriptor.plugin_id.clone(),
                    name: meta.descriptor.display_name.clone(),
                    engine: meta.descriptor.family_name.clone(),
                    version: installed_identity.map_or_else(
                        || installed_version.to_owned(),
                        shinki_emu_metadata::CoreIdentity::display_version,
                    ),
                    capabilities: meta
                        .descriptor
                        .capabilities
                        .iter()
                        .copied()
                        .map(capability_label)
                        .map(str::to_owned)
                        .collect(),
                    state: state.into(),
                    source: if meta.bundled { "bundled" } else { "user" }.into(),
                    message: String::new(),
                    removable: !meta.bundled,
                    update_version,
                    download: self.download_view(&meta.descriptor.plugin_id),
                },
                meta.stem.clone(),
            ));
        }
        // Before Prepare there is no registry metadata. Installed manifests
        // still belong in the inventory, with no claim that their DLL loaded.
        if !self.registry.is_ready() {
            let entries = fs::read_dir(&cores)
                .map_err(|error| format!("SHINKI_EMU_CORE_PENDING_APPLY: {}", error.kind()))?;
            for entry in entries {
                let entry = entry
                    .map_err(|error| format!("SHINKI_EMU_CORE_PENDING_APPLY: {}", error.kind()))?;
                let Ok(stem) = entry.file_name().into_string() else {
                    continue;
                };
                if listed_stems.contains(&stem) || self.plugin_errors.contains_key(&stem) {
                    continue;
                }
                let Some(manifest) = core_package::read_manifest(&entry.path()) else {
                    continue;
                };
                let state = match pending.get(&stem) {
                    Some(PendingKind::Remove) => "pending-removal",
                    Some(PendingKind::Install) => "pending-update",
                    None => "installed",
                };
                let online = self.online_core(&manifest.plugin_id);
                let update_version = online
                    .filter(|entry| {
                        state == "installed"
                            && entry.is_update_for(&manifest.version, manifest.identity.as_ref())
                    })
                    .map(CoreCatalogEntry::display_version)
                    .unwrap_or_default();
                listed_stems.insert(stem.clone());
                listed_plugins.insert(manifest.plugin_id.clone());
                items.push((
                    CoreViewModel {
                        id: manifest.plugin_id.clone(),
                        name: online.map_or_else(
                            || manifest.plugin_id.clone(),
                            |entry| entry.display_name.clone(),
                        ),
                        engine: online.map(|entry| entry.family_name.clone()).unwrap_or_default(),
                        version: manifest.identity.as_ref().map_or_else(
                            || manifest.version.clone(),
                            shinki_emu_metadata::CoreIdentity::display_version,
                        ),
                        state: state.into(),
                        source: "user".into(),
                        removable: true,
                        update_version,
                        download: self.download_view(&manifest.plugin_id),
                        ..CoreViewModel::default()
                    },
                    stem,
                ));
            }
        }
        items.sort_by(|left, right| left.0.name.cmp(&right.0.name));
        let updates_available = items
            .iter()
            .filter(|(item, _)| !item.update_version.is_empty())
            .count();
        let update_all_count = items
            .iter()
            .filter(|(item, _)| {
                !item.update_version.is_empty() && !self.core_downloads.is_running(&item.id)
            })
            .count();
        for (stem, raw) in &self.plugin_errors {
            let described = describe_error(raw);
            items.push((
                CoreViewModel {
                    id: stem.clone(),
                    name: stem.clone(),
                    state: "failed".into(),
                    source: "user".into(),
                    message: described.title,
                    removable: cores.join(stem).is_dir(),
                    ..CoreViewModel::default()
                },
                stem.clone(),
            ));
        }
        for (stem, kind) in &pending {
            if *kind != PendingKind::Install || listed_stems.contains(stem) {
                continue;
            }
            let manifest =
                core_package::read_manifest(&cores.join(core_package::PENDING).join(stem));
            let plugin_id = manifest.as_ref().map(|manifest| manifest.plugin_id.clone());
            let online = plugin_id.as_deref().and_then(|id| self.online_core(id));
            let name = online.map_or_else(|| stem.clone(), |entry| entry.display_name.clone());
            let engine = online
                .map(|entry| entry.family_name.clone())
                .unwrap_or_default();
            listed_plugins.extend(plugin_id.clone());
            items.push((
                CoreViewModel {
                    id: plugin_id.unwrap_or_else(|| stem.clone()),
                    name,
                    engine,
                    version: manifest
                        .map(|manifest| {
                            manifest.identity.as_ref().map_or_else(
                                || manifest.version.clone(),
                                shinki_emu_metadata::CoreIdentity::display_version,
                            )
                        })
                        .unwrap_or_default(),
                    state: "pending-install".into(),
                    source: "user".into(),
                    removable: true,
                    ..CoreViewModel::default()
                },
                stem.clone(),
            ));
        }
        if let Some(catalog) = &self.metadata_catalog {
            let available: BTreeSet<&str> = catalog
                .cores
                .entries
                .iter()
                .map(|entry| entry.plugin_id.as_str())
                .filter(|plugin_id| !listed_plugins.contains(*plugin_id))
                .collect();
            for entry in available.into_iter().filter_map(|id| self.online_core(id)) {
                items.push((
                    CoreViewModel {
                        id: entry.plugin_id.clone(),
                        name: entry.display_name.clone(),
                        engine: entry.family_name.clone(),
                        version: entry.display_version(),
                        state: "available".into(),
                        source: "online".into(),
                        download: self.download_view(&entry.plugin_id),
                        ..CoreViewModel::default()
                    },
                    String::new(),
                ));
            }
        }
        let (items, stems) = items.into_iter().unzip();
        Ok((
            CoresViewModel {
                items,
                restart_required: !pending.is_empty() && self.cores_started(),
                updates_available: i32::try_from(updates_available).unwrap_or(i32::MAX),
                update_all_count: i32::try_from(update_all_count).unwrap_or(i32::MAX),
                batch: self.batch_view(),
                ..CoresViewModel::default()
            },
            stems,
        ))
    }

    pub(super) fn cores_view(&self) -> Result<CoresViewModel, String> {
        self.core_items().map(|(view, _)| view)
    }

    /// Stage a local `.zip` core package or a single core library.
    pub(super) fn install_core(&mut self, source: &Path) -> Result<ManagerViewModel, String> {
        let pending = self.cores_dir().join(core_package::PENDING);
        let package = source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"));
        if package {
            core_package::stage_package(source, None, &pending)?;
        } else {
            core_package::stage_library(source, &pending)?;
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
        self.require_network()?;
        let entry = self
            .online_core(plugin_id)
            .cloned()
            .ok_or_else(|| "SHINKI_EMU_CORE_NOT_AVAILABLE".to_owned())?;
        let wake = self
            .host_wake
            .clone()
            .ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        let size = entry.size;
        let pending = self.cores_dir().join(core_package::PENDING);
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
        self.require_network()?;
        if self.core_batch.as_ref().is_some_and(|batch| !batch.finished()) {
            return Err("SHINKI_EMU_CORE_DOWNLOAD_PENDING".into());
        }
        let targets = self.updatable_cores()?;
        self.start_core_batch(targets)
    }

    /// Start a new run with the failed members of the last one.
    pub(super) fn retry_failed_cores(&mut self) -> Result<ManagerViewModel, String> {
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

    /// Core directory stem of a core list entry (plugin id or stem).
    fn core_stem(&self, core_id: &str) -> Result<Option<String>, String> {
        let (view, stems) = self.core_items()?;
        Ok(view
            .items
            .iter()
            .zip(stems)
            .find(|(item, stem)| item.id == core_id && !stem.is_empty())
            .map(|(_, stem)| stem))
    }

    /// Cancel a pending install, or mark an installed user core for removal.
    pub(super) fn uninstall_core(&mut self, core_id: &str) -> Result<ManagerViewModel, String> {
        let stem = self
            .core_stem(core_id)?
            .ok_or("SHINKI_EMU_CORE_UNINSTALL_BUNDLED")?;
        let stem = stem.as_str();
        let cores = self.cores_dir();
        let staged = cores.join(core_package::PENDING).join(stem);
        let installed = cores.join(stem);
        if staged.is_dir() && !installed.is_dir() {
            core_package::cancel_pending_install(&cores, stem)?;
        } else if installed.is_dir() {
            core_package::mark_removal(&cores, stem)?;
        } else {
            return Err("SHINKI_EMU_CORE_UNINSTALL_BUNDLED".into());
        }
        self.model()
    }

    /// Diagnostics mentioning a core, addressed by plugin id or directory stem.
    pub(super) fn log_text(&self, core: &str) -> Result<(String, usize), String> {
        let stem = if core.is_empty() {
            None
        } else {
            self.core_stem(core)?
        };
        let core = stem.as_deref().unwrap_or(core);
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
                .map(|candidate| candidate.descriptor.plugin_id.clone())
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
        serde_json::Value::String(text) => text.contains(core),
        serde_json::Value::Array(items) => items.iter().any(|item| json_contains_core(item, core)),
        serde_json::Value::Object(map) => map.values().any(|item| json_contains_core(item, core)),
        _ => false,
    }
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
        assert_eq!(view.items.len(), 1);
        assert_eq!(view.items[0].state, "available");
        assert_eq!(view.items[0].source, "online");
        assert_eq!(view.items[0].version, "0.3.0");
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
        tests::{Served, entry as package_entry, package_for, serve},
    };

    const PLUGINS: [(&str, &str); 3] = [
        ("shinki.emu.alpha", "shinki_emu_alpha"),
        ("shinki.emu.beta", "shinki_emu_beta"),
        ("shinki.emu.gamma", "shinki_emu_gamma"),
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
    fn restart_applies_install_update_and_removal_before_prepare() {
        let root = tempfile::tempdir().unwrap();
        let (id, stem) = PLUGINS[0];
        let library = root.path().join(format!("{stem}.{}", env::consts::DLL_EXTENSION));
        fs::write(&library, b"first installed library").unwrap();
        let open = || {
            ShinkiEmuManagerController::open_library(
                root.path().into(),
                FrameMailbox::new(),
                Vec::new(),
            )
            .unwrap()
        };
        let mut app = open();
        app.install_core(&library).unwrap();
        // A bare library uses its stem until an exported descriptor is loaded.
        assert_eq!(item(&app, stem).state, "pending-install");
        drop(app);

        let mut app = open();
        assert!(!app.cores_started());
        assert_eq!(item(&app, stem).state, "installed");
        assert!(!app.cores_view().unwrap().restart_required);
        let cores = root.path().join("cores");
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        assert_eq!(
            fs::read(cores.join(stem).join(library.file_name().unwrap())).unwrap(),
            b"first installed library"
        );

        let archive = root.path().join("update.zip");
        fs::write(&archive, package_for(id, stem)).unwrap();
        app.install_core(&archive).unwrap();
        assert_eq!(item(&app, stem).state, "pending-update");
        drop(app);

        let mut app = open();
        let installed = item(&app, id);
        assert_eq!(installed.state, "installed");
        assert_eq!(installed.version, "0.2.0");
        assert!(!app.cores_started());
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        assert_eq!(app.cores_view().unwrap().items.len(), 1);

        app.uninstall_core(id).unwrap();
        assert_eq!(item(&app, id).state, "pending-removal");
        drop(app);
        let app = open();
        assert!(!cores.join(stem).exists());
        assert!(core_package::pending_entries(&cores).unwrap().is_empty());
        assert!(app.cores_view().unwrap().items.is_empty());
    }

    #[test]
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
}
