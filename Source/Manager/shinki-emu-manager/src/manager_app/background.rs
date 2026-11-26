use super::*;
use owner::PrepareProgress;
use probe_worker::{ScanKind, ScanReport, ScanRequest, UnrecognizedRoot};
use shinki_emu_manager_ui_slint::tr;

fn scanned_game(path: &Path, #[cfg(target_os = "android")] saf: Option<&probe_worker::SafProbe>) -> Result<GameRecord, String> {
    let location = path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?;
    #[cfg(target_os = "android")]
    let (game_id, title) = if let Some(saf) = saf {
        use shinki_emu_manager::platform::android_tree::{authority, document_identity};
        let identity = format!("{}/{}", authority(location), document_identity(location));
        (format!("game-{}", sha256_id(&identity)), saf.title.clone())
    } else {
        (path_id(path), title_for_path(path))
    };
    #[cfg(not(target_os = "android"))]
    let (game_id, title) = (path_id(path), title_for_path(path));
    Ok(GameRecord {
        game_id,
        title,
        user_title: None,
        location: location.into(),
        family_id: None,
        content_fingerprint: None,
        added_at_unix_ms: unix_time_ms()?,
    })
}

fn same_scan_location(a: &str, b: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        use shinki_emu_manager::platform::android_tree as saf;
        if saf::is_library_uri(a) && saf::is_library_uri(b) {
            return saf::authority(a) == saf::authority(b) && saf::document_identity(a) == saf::document_identity(b);
        }
    }
    a == b
}

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
    Roots(BTreeMap<PathBuf, bool>),
    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    Optical(OpticalVolume),
}

fn interactive_offer(scan: Option<&LibraryScan>, requested: &Path) -> bool {
    match scan {
        Some(LibraryScan::Roots(roots)) => roots.get(requested) == Some(&true),
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        Some(LibraryScan::Optical(volume)) => volume.root == requested,
        _ => false,
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn startup_busy(&self) -> bool {
        self.native_action.is_some()
            || self.pending_config_path.is_some()
            || matches!(self.startup_state, StartupState::Loading | StartupState::Scanning(_))
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
                let import = *kind == ScanKind::Import && self.interactive_import();
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
                    "SHINKI_EMU_GAME_DIRECTORY_BOUNDS" => tr!("扫描未完成").into(),
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

    fn interactive_import(&self) -> bool {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if matches!(&self.library_scan, Some(LibraryScan::Optical(_))) {
            return true;
        }
        matches!(&self.library_scan, Some(LibraryScan::Roots(roots)) if roots.values().any(|interactive| *interactive))
    }

    pub(super) fn prepare_view(&self) -> (&'static str, &'static str, bool) {
        match self.startup_state {
            StartupState::Waiting | StartupState::Loading => ("loading-cores", "", false),
            StartupState::Scanning(_) => (
                if self.interactive_import() { "importing" } else { "scanning" },
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

    pub(super) fn queue_library_root(&mut self, root: PathBuf, interactive: bool) {
        if interactive { self.clear_unrecognized_offer(None); }
        if !matches!(&self.library_scan, Some(LibraryScan::Roots(roots)) if roots.contains_key(&root)) {
            self.pending_scan_roots
                .entry(root)
                .and_modify(|pending| *pending |= interactive)
                .or_insert(interactive);
        }
        self.pump_library_scan();
    }

    pub(super) fn forget_queued_root(&mut self, root: &Path) {
        self.clear_unrecognized_offer(Some(root));
        self.pending_scan_roots.remove(root);
        if let Some(LibraryScan::Roots(roots)) = &mut self.library_scan
            && roots.remove(root).is_some()
        {
            self.pending_scan_roots.append(roots);
            if let Some(cancel) = &self.scan_cancel {
                cancel.cancel();
            }
        }
    }

    pub(super) fn admit_unrecognized_root(&mut self, root: UnrecognizedRoot) -> Result<ManagerViewModel, String> {
        let invalid = "SHINKI_EMU_GAME_DIRECTORY_INVALID";
        self.ensure_owner_idle()?;
        if !matches!(self.startup_state, StartupState::Ready) || self.startup_busy() || self.patch_busy() {
            return Err("SHINKI_EMU_PREPARE_BUSY".into());
        }
        let requested = root.requested.to_str().ok_or(invalid)?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let optical = root.optical.as_ref();
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let is_optical = optical.is_some();
        #[cfg(not(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos")))))]
        let is_optical = false;
        if !is_optical && !self.library.library_roots().map_err(|error| error.to_string())?.iter().any(|row| row.path == requested) {
            return Err(invalid.into());
        }
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if let Some(volume) = optical {
            volume.validate()?;
        }
        let location = root.path.to_str().ok_or(invalid)?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let optical_location = optical.map(|volume| optical::game_location(volume, &root.path)).transpose()?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let location = optical_location.as_deref().unwrap_or(location);
        #[cfg(target_os = "android")]
        let is_saf = root.saf.is_some();
        #[cfg(not(target_os = "android"))]
        let is_saf = false;
        if !is_saf {
            if probe_worker::canonical_directory(&root.requested)?.as_ref() != Some(&root.path)
                || shinki_emu_patch::is_staging_path(&root.path)
            {
                return Err(invalid.into());
            }
            fs::read_dir(&root.path).map_err(|error| format!("SHINKI_EMU_GAME_DIRECTORY_IO: {error}"))?;
        }
        #[cfg(target_os = "android")]
        if is_saf {
            use shinki_emu_manager::platform::android_tree as saf;
            if !saf::document(requested).is_ok_and(|doc| doc.directory
                && same_scan_location(&doc.uri, location)
                && !shinki_emu_patch::is_staging_path(Path::new(&doc.name)))
            {
                return Err(invalid.into());
            }
        }
        if self.library.excluded_games().map_err(|error| error.to_string())?.iter()
            .any(|game| same_scan_location(&game.location, location))
            || self.library.list_games().map_err(|error| error.to_string())?.iter()
                .any(|game| same_scan_location(&game.location, location))
        {
            return Err("SHINKI_EMU_SNACKBAR_ACTION_UNAVAILABLE".into());
        }
        let game = scanned_game(
            &root.path,
            #[cfg(target_os = "android")]
            root.saf.as_ref(),
        )?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let game = if let Some(volume) = optical {
            self.bind_scanned_optical_game(game, volume, &root.path)?
        } else {
            game
        };
        self.library.add_game(&game).map_err(|error| error.to_string())?;
        #[cfg(target_os = "android")]
        if let Some(path) = root.saf.and_then(|saf| saf.local_path)
            && shinki_emu_manager::platform::android_tree::local_path(&game.location).ok().flatten().as_ref() == Some(&path)
        {
            self.saf_paths.insert(game.game_id.clone(), path);
        }
        self.set_recognition(&game.game_id, Vec::new())?;
        self.selected_case_id = Some(game.game_id);
        self.model()
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
                paths.retain(|path| !self.pending_scan_roots.contains_key(path));
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
        let offer_unrecognized = roots.len() == 1 && roots.values().any(|interactive| *interactive);
        match self.dispatch_scan(
            ScanKind::Import, roots.keys().cloned().collect(), offer_unrecognized,
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            None,
        ) {
            Ok(()) => self.library_scan = Some(LibraryScan::Roots(roots)),
            Err(error) => {
                self.requeue_library_scan(Some(LibraryScan::Roots(roots)));
                self.scan_failed(error);
            }
        }
        true
    }

    pub(super) fn begin_scan(&mut self, kind: ScanKind, paths: Vec<PathBuf>) -> Result<(), String> {
        self.dispatch_scan(
            kind, paths, false,
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            None,
        )
    }

    #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
    pub(super) fn scan_optical_volume(&mut self, volume: OpticalVolume) -> Result<(), String> {
        self.dispatch_scan(ScanKind::Import, vec![volume.root.clone()], true, Some(volume.clone()))?;
        self.library_scan = Some(LibraryScan::Optical(volume));
        Ok(())
    }

    fn dispatch_scan(
        &mut self, kind: ScanKind, paths: Vec<PathBuf>, offer_unrecognized: bool,
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        optical: Option<OpticalVolume>,
    ) -> Result<(), String> {
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
        #[cfg_attr(not(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos")))), allow(unused_mut))]
        let mut games = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if let Some(volume) = optical.as_ref() {
            games.retain_mut(|game| {
                let Some(path) = self.scan_optical_game_path(&game.location, volume) else { return false; };
                game.location = path.to_owned();
                true
            });
        } else {
            games.retain(|game| !optical::is_optical(&game.location));
        }
        if kind == ScanKind::Full && paths.is_empty() && games.is_empty() {
            return Ok(());
        }
        let excluded = self
            .library
            .excluded_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter_map(|game| {
                #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                {
                    if let Some(volume) = optical.as_ref() {
                        return self.scan_optical_game_path(&game.location, volume).map(PathBuf::from);
                    }
                    if optical::is_optical(&game.location) {
                        return None;
                    }
                }
                Some(PathBuf::from(game.location))
            })
            .collect();
        let cancel = shinki_emu_manager_core::NativeCancellation::new(0);
        self.clear_unrecognized_offer(None);
        self.registry.scan(ScanRequest {
            kind,
            offer_unrecognized,
            paths,
            games,
            excluded,
            cancel: cancel.clone(),
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            optical,
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
                    if let Err(error) = self.restore_recognition() {
                        self.background_failed(error);
                        continue;
                    }
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
                        Err(error) => self.request_scan_failed(scan, error),
                        Ok(report) => {
                            self.startup_state = StartupState::Ready;
                            let traversal_limited = report.traversal_limited;
                            let skipped_candidates = report.skipped_candidates;
                            let io_error = report.has_io_error();
                            match self.apply_scan_report(kind, report, scan.as_ref()) {
                                Ok(added) => {
                                    let refresh = matches!(scan, Some(LibraryScan::Full(FullScan::Refresh)));
                                    if scan.is_some() && traversal_limited && added > 0 {
                                        self.show_snackbar(tr!("已识别 {added} 部新作品；部分目录未扫描", added = added), None);
                                    } else if added > 0 {
                                        self.show_snackbar(tr!("发现 {added} 部新作品", added = added), None);
                                    } else if scan.is_some() && !io_error && (traversal_limited || skipped_candidates > 0) {
                                        self.show_snackbar(tr!("部分目录未扫描"), None);
                                    } else if refresh && !io_error {
                                        self.show_snackbar(tr!("资料库已是最新"), None);
                                    }
                                }
                                Err(error) => self.request_scan_failed(scan, error),
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
            && let Err(error) = recognition::runnable_core(self.registry.installed(), action.plugin_id())
                .map_err(str::to_owned)
                .and_then(|_| self.registry.activate(action.plugin_id()))
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

    fn request_scan_failed(&mut self, scan: Option<LibraryScan>, error: String) {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if let Some(LibraryScan::Optical(volume)) = &scan {
            self.opened_optical.remove(&volume.root);
            self.startup_state = StartupState::Ready;
            self.diagnostic = error;
            self.diagnostic_severity = "error";
            return;
        }
        self.requeue_library_scan(scan);
        self.scan_failed(error);
    }

    fn requeue_library_scan(&mut self, scan: Option<LibraryScan>) {
        match scan {
            Some(LibraryScan::Full(full)) => self.pending_full_scan = Some(full),
            Some(LibraryScan::Roots(roots)) => {
                for root in roots.into_keys() {
                    self.pending_scan_roots.entry(root).or_insert(false);
                }
            }
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            Some(LibraryScan::Optical(_)) => {}
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
    fn apply_scan_report(&mut self, kind: ScanKind, mut report: ScanReport, scan: Option<&LibraryScan>) -> Result<usize, String> {
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
            self.apply_scan_report_inner(kind, report, automatic.as_ref(), scan)
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
        scan: Option<&LibraryScan>,
    ) -> Result<usize, String> {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let optical = if let Some(LibraryScan::Optical(volume)) = scan {
            volume.validate()?;
            Some(volume)
        } else {
            None
        };
        tracing::info!(
            event = "shinki.emu.manager.probe_refresh",
            games = report.games.len(),
            missing = report.missing,
            skipped_candidates = report.skipped_candidates,
            traversal_limited = report.traversal_limited,
            cache_hits = report.cache_hits
        );
        let incomplete = report.traversal_limited || report.skipped_candidates != 0 || report.has_io_error();
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
                    if let Some(saf) = &result.saf {
                        if let Some(path) = &saf.local_path {
                            self.saf_paths.insert(id.clone(), path.clone());
                        } else {
                            self.saf_paths.remove(&id);
                        }
                    }
                    self.set_recognition(&id, Vec::new())?;
                }
                continue;
            }
            let mut choices = Vec::new();
            let selected = if let Some(profile) = &profile {
                choices = recognition::choices(&result.detections);
                Some(self.select_import_profile(&choices, profile)?)
            } else {
                None
            };
            if ids.is_empty() {
                let game = scanned_game(
                    &result.path,
                    #[cfg(target_os = "android")]
                    result.saf.as_ref(),
                )?;
                #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
                let game = if let Some(volume) = optical {
                    self.bind_scanned_optical_game(game, volume, &result.path)?
                } else {
                    game
                };
                self.library
                    .add_game(&game)
                    .map_err(|error| error.to_string())?;
                ids.push(game.game_id);
                added += 1;
            }
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
                }
                imported.insert(id);
            }
        }
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if let Some(volume) = optical {
            self.opened_optical.insert(volume.root.clone(), volume.clone());
        }
        if kind == ScanKind::Import && automatic.is_none() {
            if let Some(id) = imported.first() {
                self.selected_case_id = Some(id.clone());
            }
            if imported.is_empty() && !incomplete {
                if let Some(root) = report.unrecognized_root
                    && interactive_offer(scan, &root.requested)
                    && self.pending_scan_roots.is_empty()
                {
                    self.show_snackbar(
                        tr!("未识别到游戏引擎，可添加所选文件夹后手动选择核心。"),
                        Some(snackbar::SnackbarAction::AddUnrecognized(root)),
                    );
                } else {
                    self.diagnostic = tr!("所选目录中未识别到游戏引擎，请选择原生游戏目录。").into();
                    self.diagnostic_severity = "warning";
                }
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
