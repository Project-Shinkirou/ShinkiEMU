use super::*;
use owner::PrepareProgress;
use probe_worker::{ScanKind, ScanReport, ScanRequest};
use shinki_emu_manager_ui_slint::tr;

pub(super) enum StartupState {
    Waiting,
    Loading,
    Scanning(ScanKind),
    Ready,
    Failed(String),
    OwnerFailed(String),
}

#[derive(Clone, Copy)]
pub(super) enum FullScan {
    Initial,
    Refresh,
}

pub(super) enum LibraryScan {
    Full(FullScan),
    Roots(BTreeSet<PathBuf>),
}

impl ShinkiEmuManagerController {
    pub(super) fn startup_busy(&self) -> bool {
        self.native_action.is_some() || matches!(
            self.startup_state,
            StartupState::Loading | StartupState::Scanning(_)
        )
    }

    pub(super) fn cores_ready(&self) -> bool {
        self.registry.is_ready()
    }

    pub(super) fn cores_started(&self) -> bool {
        !matches!(self.startup_state, StartupState::Waiting)
    }

    pub(super) fn startup_status(&self) -> String {
        match &self.startup_state {
            StartupState::Waiting | StartupState::Loading => tr!("正在读取核心安装清单…").into(),
            StartupState::Scanning(kind) => {
                let import = *kind == ScanKind::Import;
                match (self.prepare_progress, import) {
                    (Some(PrepareProgress::Scan { directories, games }), true) => tr!(
                        "正在导入游戏目录：已检查 {directories} 个目录，识别 {games} 部作品…",
                        directories = directories,
                        games = games
                    ),
                    (Some(PrepareProgress::Scan { directories, games }), false) => tr!(
                        "正在扫描资料库：已检查 {directories} 个目录，识别 {games} 部作品…",
                        directories = directories,
                        games = games
                    ),
                    (_, true) => tr!("正在导入游戏目录…").into(),
                    (_, false) => tr!("正在扫描资料库…").into(),
                }
            }
            StartupState::Failed(error) => {
                let (code, detail) = error.split_once(": ").unwrap_or((error.as_str(), error.as_str()));
                match code {
                    "SHINKI_EMU_GAME_DIRECTORY_INVALID" => tr!("目录已不可用，请检查或移除该目录。").into(),
                    "SHINKI_EMU_GAME_DIRECTORY_IO" | "SHINKI_EMU_GAME_DIRECTORY_ENUMERATION" => {
                        tr!("无法读取游戏目录：{error}", error = detail)
                    }
                    "SHINKI_EMU_GAME_DIRECTORY_BOUNDS" => tr!("目录范围过大，请移除该目录并添加较小的目录。").into(),
                    code if code.starts_with("SHINKI_EMU_PATCH_") => {
                        tr!("补丁输出信息无效，请检查补丁或移除该目录。").into()
                    }
                    _ => tr!("游戏扫描失败：{error}", error = detail),
                }
            }
            StartupState::OwnerFailed(error) => {
                tr!("核心清单或后台运行失败：{error}。请重启应用。", error = error)
            }
            StartupState::Ready => String::new(),
        }
    }

    pub(super) fn prepare_view(&self) -> (&'static str, &'static str, bool) {
        match self.startup_state {
            StartupState::Waiting | StartupState::Loading => ("loading-cores", "", false),
            StartupState::Scanning(_) => (
                "scanning",
                "",
                self.scan_cancel.as_ref().is_some_and(|cancel| !cancel.is_cancelled()),
            ),
            StartupState::Ready => ("ready", tr!("刷新资料库"), false),
            StartupState::Failed(_) => ("failed", tr!("重试"), false),
            StartupState::OwnerFailed(_) => ("failed", "", false),
        }
    }

    pub(super) fn can_refresh_library(&self) -> bool {
        self.cores_ready() && !self.startup_busy() && self.active.is_none() && !self.patch_busy()
    }

    pub(super) fn refresh_library(&mut self) -> Result<ManagerViewModel, String> {
        self.ensure_owner_idle()?;
        if self.patch_busy() {
            return Err(format!(
                "SHINKI_EMU_PATCH_INVALID_REQUEST: {}",
                tr!("文件补丁转换期间不能扫描游戏")
            ));
        }
        if self.startup_busy() || matches!(self.startup_state, StartupState::Waiting) {
            return Err("SHINKI_EMU_PREPARE_BUSY".into());
        }
        if !self.cores_ready() {
            return Err("SHINKI_EMU_PREPARE_RESTART_REQUIRED".into());
        }
        if !matches!(self.startup_state, StartupState::Failed(_))
            || (self.pending_full_scan.is_none() && self.pending_scan_roots.is_empty())
        {
            self.pending_full_scan = Some(FullScan::Refresh);
        }
        self.startup_state = StartupState::Ready;
        self.pump_library_scan();
        if self.network_allowed
            && let Err(error) = self.submit_metadata_sync()
        {
            self.record_sync_error(error);
        }
        self.model()
    }

    pub(super) fn cancel_library_scan(&mut self) -> Result<ManagerViewModel, String> {
        if !matches!(self.startup_state, StartupState::Scanning(_)) {
            return Err("SHINKI_EMU_PREPARE_NOT_CANCELLABLE".into());
        }
        if let Some(cancel) = &self.scan_cancel {
            cancel.cancel();
        }
        self.model()
    }

    pub(super) fn scan_roots(&self) -> Result<Vec<PathBuf>, String> {
        let mut roots: BTreeSet<PathBuf> = self.game_roots.iter().cloned().collect();
        roots.extend(
            self.library
                .library_roots()
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|root| PathBuf::from(root.path)),
        );
        Ok(roots.into_iter().collect())
    }

    pub(super) fn refresh_cores(&mut self) -> Result<(), String> {
        if self.cores_started() {
            return Err("SHINKI_EMU_OWNER_ALREADY_STARTED".into());
        }
        self.startup_state = StartupState::Loading;
        self.prepare_progress = None;
        if let Err(error) = self.registry.start() {
            self.background_failed(error.clone());
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn queue_library_root(&mut self, root: PathBuf) {
        if !matches!(&self.library_scan, Some(LibraryScan::Roots(roots)) if roots.contains(&root)) {
            self.pending_scan_roots.insert(root);
        }
        self.pump_library_scan();
    }

    pub(super) fn forget_queued_root(&mut self, root: &Path) {
        self.pending_scan_roots.remove(root);
        if let Some(LibraryScan::Roots(roots)) = &mut self.library_scan
            && roots.remove(root)
        {
            self.pending_scan_roots.append(roots);
            if let Some(cancel) = &self.scan_cancel {
                cancel.cancel();
            }
        }
    }

    /// Dispatch only while the owner can scan. Completions and newly added roots
    /// call this again; a failure keeps the request queued for an explicit retry.
    pub(super) fn pump_library_scan(&mut self) -> bool {
        if !matches!(self.startup_state, StartupState::Ready) || !self.can_refresh_library() {
            return false;
        }
        if let Some(full) = self.pending_full_scan.take() {
            let result = self.scan_roots().and_then(|mut paths| {
                // A newly chosen root needs import (including patch receipts),
                // not the startup pass over roots already in the library.
                paths.retain(|path| !self.pending_scan_roots.contains(path));
                self.begin_scan(ScanKind::Full, paths)
            });
            match result {
                Ok(()) if matches!(self.startup_state, StartupState::Scanning(_)) => {
                    self.library_scan = Some(LibraryScan::Full(full));
                    return true;
                }
                Ok(()) if matches!(full, FullScan::Refresh) && self.pending_scan_roots.is_empty() => {
                    self.show_snackbar(tr!("资料库已是最新"), None);
                    return true;
                }
                Ok(()) => {}
                Err(error) => {
                    self.pending_full_scan = Some(full);
                    self.scan_failed(error);
                    return true;
                }
            }
        }
        if self.pending_scan_roots.is_empty() {
            return false;
        }
        let roots = std::mem::take(&mut self.pending_scan_roots);
        match self.begin_scan(ScanKind::Import, roots.iter().cloned().collect()) {
            Ok(()) => self.library_scan = Some(LibraryScan::Roots(roots)),
            Err(error) => {
                self.pending_scan_roots = roots;
                self.scan_failed(error);
            }
        }
        true
    }

    pub(super) fn begin_scan(&mut self, kind: ScanKind, paths: Vec<PathBuf>) -> Result<(), String> {
        self.ensure_owner_idle()?;
        if self.patch_job.is_some() {
            return Err(format!(
                "SHINKI_EMU_PATCH_INVALID_REQUEST: {}",
                tr!("文件补丁转换期间不能扫描游戏")
            ));
        }
        if self.startup_busy() {
            return Err("SHINKI_EMU_FAMILY_INITIALIZING".into());
        }
        if !self.cores_ready() {
            return Err("SHINKI_EMU_CORES_NOT_READY".into());
        }
        let games = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?;
        if kind == ScanKind::Full && paths.is_empty() && games.is_empty() {
            return Ok(());
        }
        let excluded = self
            .library
            .excluded_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| PathBuf::from(game.location))
            .collect();
        let cancel = shinki_emu_manager_core::NativeCancellation::new(0);
        self.registry.scan(ScanRequest {
            kind,
            paths,
            games,
            excluded,
            cancel: cancel.clone(),
        })?;
        self.scan_cancel = Some(cancel);
        self.prepare_progress = None;
        self.startup_state = StartupState::Scanning(kind);
        Ok(())
    }

    pub(super) fn poll_background(&mut self) -> bool {
        let mut changed = false;
        let mut loaded = false;
        while let Some(event) = self.registry.take_background() {
            changed = true;
            match event {
                owner::BackgroundEvent::Ready(Ok((_, errors))) => {
                    self.plugin_errors = errors;
                    if matches!(self.startup_state, StartupState::Loading) {
                        self.startup_state = StartupState::Ready;
                        self.prepare_progress = None;
                        loaded = true;
                    }
                }
                owner::BackgroundEvent::Activated { plugin_id, result } => {
                    if let Err(error) = self.activated(plugin_id, result) {
                        self.diagnostic = error;
                        self.diagnostic_severity = "error";
                    }
                }
                owner::BackgroundEvent::Ready(Err(error)) => self.background_failed(error),
                owner::BackgroundEvent::Progress(progress) => {
                    self.prepare_progress = Some(progress);
                }
                owner::BackgroundEvent::Scanned(kind, result) => {
                    let cancelled = self.scan_cancel.take().is_some_and(|cancel| cancel.is_cancelled());
                    let scan = self.library_scan.take();
                    self.prepare_progress = None;
                    match result {
                        _ if cancelled => self.scan_cancelled(scan.as_ref()),
                        Err(error) if error == "SHINKI_EMU_SCAN_CANCELLED" => {
                            self.scan_cancelled(scan.as_ref());
                        }
                        Err(error) => {
                            self.requeue_library_scan(scan);
                            self.scan_failed(error);
                        }
                        Ok(report) => {
                            self.startup_state = StartupState::Ready;
                            match self.apply_scan_report(kind, report) {
                                Ok(added) => {
                                    let refresh = matches!(scan, Some(LibraryScan::Full(FullScan::Refresh)));
                                    if added > 0 {
                                        self.show_snackbar(tr!("发现 {added} 部新作品", added = added), None);
                                    } else if refresh {
                                        self.show_snackbar(tr!("资料库已是最新"), None);
                                    }
                                }
                                Err(error) => {
                                    self.requeue_library_scan(scan);
                                    self.scan_failed(error);
                                }
                            }
                        }
                    }
                }
                owner::BackgroundEvent::Exited(error) => {
                    if !matches!(self.startup_state, StartupState::OwnerFailed(_)) {
                        self.background_failed(error);
                    }
                }
            }
        }
        if loaded
            && let Some(action) = &self.native_action
            && let Err(error) = self.registry.activate(action.plugin_id())
        {
            let plugin_id = action.plugin_id().to_owned();
            self.native_action = None;
            self.native_failures.insert(plugin_id, error.clone());
            self.diagnostic = error;
            self.diagnostic_severity = "error";
        }
        changed
    }

    fn scan_cancelled(&mut self, scan: Option<&LibraryScan>) {
        self.startup_state = StartupState::Ready;
        if self.patch_import.take().is_some() {
            self.patch_import_failed("SHINKI_EMU_SCAN_CANCELLED".into());
        }
        if !matches!(scan, Some(LibraryScan::Roots(roots)) if roots.is_empty()) {
            self.show_snackbar(tr!("已取消扫描"), None);
        }
    }

    fn requeue_library_scan(&mut self, scan: Option<LibraryScan>) {
        match scan {
            Some(LibraryScan::Full(full)) => self.pending_full_scan = Some(full),
            Some(LibraryScan::Roots(mut roots)) => self.pending_scan_roots.append(&mut roots),
            None => {}
        }
    }

    fn scan_failed(&mut self, error: String) {
        tracing::warn!(event = "shinki.emu.manager.library_scan_failed", diagnostic_code = %error);
        if self.patch_import.take().is_some() {
            self.patch_import_failed(error);
            self.startup_state = StartupState::Ready;
        } else {
            self.startup_state = StartupState::Failed(error);
        }
    }

    fn background_failed(&mut self, error: String) {
        self.diagnostic.clone_from(&error);
        self.diagnostic_severity = "error";
        if let Some(action) = self.native_action.take() {
            self.native_failures.insert(action.plugin_id().into(), error.clone());
        }
        let scan = self.library_scan.take();
        self.requeue_library_scan(scan);
        self.scan_cancel = None;
        self.prepare_progress = None;
        if self.patch_import.take().is_some() {
            self.patch_import_failed(error.clone());
        }
        self.startup_state = StartupState::OwnerFailed(error);
    }

    /// Returns how many games the scan added to the library.
    fn apply_scan_report(&mut self, kind: ScanKind, mut report: ScanReport) -> Result<usize, String> {
        let automatic = if kind == ScanKind::Import {
            self.patch_import.take()
        } else {
            None
        };
        let result = (|| {
            if let Some(expected) = &automatic {
                let output = fs::canonicalize(&expected.output)
                    .map_err(|_| "SHINKI_EMU_PATCH_PATH: completed output is no longer available")?;
                // Never apply an automatic result to a nested/other game, nor
                // let unrelated missing library paths mutate the source cache.
                report.games.retain(|game| {
                    game.matches_output(&output)
                        && (!expected.in_place || game.game_ids.contains(&expected.source_game_id))
                });
                if report.games.len() != 1 {
                    #[cfg(target_os = "android")]
                    if let Some(error) = report.saf_error.take() {
                        return Err(error);
                    }
                    return Err(
                        "SHINKI_EMU_FAMILY_PROBE_REQUIRED: completed output root was not recognized"
                            .into(),
                    );
                }
                report.missing = 0;
            }
            self.apply_scan_report_inner(kind, report, automatic.as_ref())
        })();
        if automatic.is_some() {
            match result {
                Ok(_) => self.patch_import_succeeded(),
                Err(error) => self.patch_import_failed(error),
            }
            Ok(0)
        } else {
            result
        }
    }

    fn apply_scan_report_inner(
        &mut self,
        kind: ScanKind,
        report: ScanReport,
        automatic: Option<&file_patch::PendingImport>,
    ) -> Result<usize, String> {
        tracing::info!(
            event = "shinki.emu.manager.probe_refresh",
            games = report.games.len(),
            missing = report.missing,
            cache_hits = report.cache_hits
        );
        if let Some(error) = report.cache_error {
            tracing::warn!(event = "shinki.emu.manager.probe_cache_failed", diagnostic_code = %error);
        }
        #[cfg(target_os = "android")]
        if kind == ScanKind::Full {
            self.saf_paths.clear();
        }
        let mut imported = BTreeSet::new();
        let mut added = 0;
        for result in report.games {
            let mut ids = result.game_ids;
            let profile = result.patch_profile.transpose()?;
            if let Some(expected) = automatic {
                let profile = profile
                    .as_ref()
                    .ok_or("SHINKI_EMU_PATCH_INTEGRITY: completed output receipt is missing")?;
                if expected.profile.plugin_id != profile.plugin_id
                    || expected.profile.configuration != profile.configuration
                    || expected.profile.required_capabilities != profile.required_capabilities
                {
                    return Err(
                        "SHINKI_EMU_PATCH_INTEGRITY: output receipt changed before import".into(),
                    );
                }
            }
            if profile.is_some() && ids.len() > 1 {
                return Err("SHINKI_EMU_LIBRARY_INVALID_LOCATION: patch output has ambiguous library aliases".into());
            }
            if result.detections.is_empty() {
                if profile.is_some() {
                    return Err("SHINKI_EMU_EMULATOR_SELECTION_INVALID: patch output has no supported emulator".into());
                }
                for id in ids {
                    #[cfg(target_os = "android")]
                    if result.saf.is_some() {
                        self.saf_paths.remove(&id);
                    }
                    self.set_recognition(&id, Vec::new())?;
                }
                continue;
            }
            let choices = recognition::choices(&result.detections);
            let selected = profile.as_ref().map(|profile| self.select_import_profile(&choices, profile)).transpose()?;
            if ids.is_empty() {
                let location = result.path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?;
                #[cfg(target_os = "android")]
                let saf = result.saf.as_ref().map(|saf| {
                    use shinki_emu_manager::platform::android_tree::{authority, document_identity};
                    let identity = format!("{}/{}", authority(location), document_identity(location));
                    (format!("game-{}", sha256_id(&identity)), saf.title.clone())
                });
                #[cfg(target_os = "android")]
                let (game_id, title) = saf.unwrap_or_else(|| (path_id(&result.path), title_for_path(&result.path)));
                #[cfg(not(target_os = "android"))]
                let (game_id, title) = (path_id(&result.path), title_for_path(&result.path));
                let game = GameRecord {
                    game_id,
                    title,
                    user_title: None,
                    location: location.into(),
                    family_id: None,
                    content_fingerprint: None,
                    added_at_unix_ms: unix_time_ms()?,
                };
                self.library
                    .add_game(&game)
                    .map_err(|error| error.to_string())?;
                ids.push(game.game_id);
                added += 1;
            }
            let mut choices = choices;
            for id in ids {
                if automatic.is_some_and(|expected| expected.source_game_id == id && !expected.in_place) {
                    return Err("SHINKI_EMU_PATCH_INTEGRITY: output import would modify source game".into());
                }
                #[cfg(target_os = "android")]
                if let Some(saf) = &result.saf {
                    // Keep the game ID/settings but remember a currently usable grant.
                    let mut game = self.game(&id)?;
                    if let Some(uri) = result.path.to_str().filter(|uri| *uri != game.location) {
                        game.location = uri.into();
                        self.library.add_game(&game).map_err(|error| error.to_string())?;
                    }
                    if let Some(path) = &saf.local_path {
                        self.saf_paths.insert(id.clone(), path.clone());
                    } else {
                        self.saf_paths.remove(&id);
                    }
                }
                self.set_recognition(&id, result.detections.clone())?;
                if let Some((profile, selected)) = profile.as_ref().zip(selected.as_ref()) {
                    self.apply_import_profile(&id, std::mem::take(&mut choices), *selected, profile)?;
                    if let Some(expected) = automatic && !expected.in_place {
                        self.inherit_source_work(&expected.source_game_id, &id)?;
                    }
                } else {
                    self.set_probe_candidates(&id, choices.clone())?;
                }
                imported.insert(id);
            }
        }
        if kind == ScanKind::Import && automatic.is_none() {
            if let Some(id) = imported.first() {
                self.selected_case_id = Some(id.clone());
            }
            if imported.is_empty() {
                self.diagnostic = tr!("所选目录中未识别到游戏引擎，请选择原生游戏目录。").into();
                self.diagnostic_severity = "warning";
            }
        }
        if report.missing > 0 {
            self.diagnostic = tr!(
                "有 {count} 个游戏目录当前不可用。资料库信息已保留，恢复目录后点击“刷新资料库”。",
                count = report.missing
            );
            self.diagnostic_severity = "warning";
        }
        #[cfg(target_os = "android")]
        if let Some(error) = report.saf_error {
            self.diagnostic = error;
            self.diagnostic_severity = "warning";
        }
        if self.selected_case_id.is_none() && automatic.is_none() {
            self.selected_case_id = self.first_visible_game()?;
        }
        Ok(added)
    }
}
