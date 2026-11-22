use super::*;
use shinki_emu_manager_ui_slint::tr;

#[cfg(target_os = "android")]
pub(super) fn saf_only_problem() -> &'static str {
    tr!("此游戏文件夹由系统文件提供程序管理，当前核心无法直接读取。")
}

/// `ShinkiLibraryTree.localPath` never maps another authority's documents to a path,
/// whatever the permission. For the external-storage provider, whether a path exists
/// is only known once it is readable, so that case stays `"provider"`.
#[cfg(target_os = "android")]
pub(super) fn provider_access(location: &str) -> &'static str {
    use shinki_emu_manager::platform::android_tree::authority;
    if authority(location) == "com.android.externalstorage.documents" {
        "provider"
    } else {
        "unmappable"
    }
}

impl ShinkiEmuManagerController {
    /// Persist a root and recognize its directories without native activation.
    pub(super) fn add_game_directory(&mut self, path: &Path) -> Result<ManagerViewModel, String> {
        #[cfg(target_os = "android")]
        if path.to_str().is_some_and(shinki_emu_manager::platform::android_tree::is_library_uri) {
            self.register_library_root(path.to_owned(), true)?;
            return self.model();
        }
        let root = probe_worker::canonical_directory(path)?
            .ok_or("SHINKI_EMU_GAME_DIRECTORY_INVALID")?;
        self.register_library_root(root, true)?;
        self.model()
    }

    pub(super) fn register_library_root(&mut self, root: PathBuf, interactive: bool) -> Result<(), String> {
        if shinki_emu_patch::is_staging_path(&root) {
            return Err(
                "SHINKI_EMU_PATCH_INVALID_REQUEST: private staging cannot be imported".into(),
            );
        }
        self.library
            .add_library_root(root.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?)
            .map_err(|error| error.to_string())?;
        self.queue_library_root(root, interactive);
        Ok(())
    }

    /// Stop scanning a root; its games stay in the library.
    pub(super) fn remove_library_root(&mut self, path: &str) -> Result<ManagerViewModel, String> {
        self.library
            .remove_library_root(path)
            .map_err(|error| error.to_string())?;
        self.forget_queued_root(Path::new(path));
        self.model()
    }

    pub(super) fn onboarding_completed(&self) -> Result<bool, String> {
        Ok(self
            .library
            .app_state::<bool>(ONBOARDING_COMPLETED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false))
    }

    pub(super) fn finish_onboarding(&mut self) -> Result<ManagerViewModel, String> {
        self.library
            .set_app_state(ONBOARDING_COMPLETED_KEY, &true)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn game_location(&self, game_id: &str) -> Result<PathBuf, String> {
        let game = self.game(game_id)?;
        self.native_game_directory(&game).map(Path::to_owned)
    }

    pub(super) fn mapped_game_directory<'a>(&'a self, game: &'a GameRecord) -> Option<&'a Path> {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if optical::is_optical(&game.location) {
            return self.available_optical_game(game).map(|binding| binding.path.as_path());
        }
        #[cfg(target_os = "android")]
        if shinki_emu_manager::platform::android_tree::is_library_uri(&game.location) {
            return self.saf_paths.get(&game.game_id).map(PathBuf::as_path);
        }
        Some(Path::new(&game.location))
    }

    pub(super) fn native_game_directory<'a>(&'a self, game: &'a GameRecord) -> Result<&'a Path, String> {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if optical::is_optical(&game.location) {
            let binding = self.available_optical_game(game)
                .ok_or(OPTICAL_MEDIA_UNAVAILABLE)?;
            binding.volume.validate()?;
            return Ok(&binding.path);
        }
        #[cfg(target_os = "android")]
        if shinki_emu_manager::platform::android_tree::is_library_uri(&game.location) {
            use shinki_emu_manager::platform::android_tree::local_path;
            return self.mapped_game_directory(game)
                .filter(|path| local_path(&game.location).ok().flatten().as_deref() == Some(*path))
                .ok_or_else(|| format!("SHINKI_EMU_ANDROID_TREE_NATIVE_PATH: {}", saf_only_problem()));
        }
        Ok(Path::new(&game.location))
    }

    pub(super) fn display_game_location<'a>(&'a self, location: &'a str) -> &'a str {
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        if let Some(binding) = self.optical_games.get(location) {
            return binding.path.to_str().unwrap_or(location);
        }
        location
    }

    pub(super) fn remove_from_library(
        &mut self,
        game_id: &str,
    ) -> Result<ManagerViewModel, String> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.game_id == game_id)
        {
            self.ensure_owner_idle()?;
        }
        let game = self.game(game_id)?;
        self.library
            .exclude_game(game_id)
            .map_err(|error| error.to_string())?;
        self.auto_link_queue.retain(|id| id != game_id);
        if self.selected_case_id.as_deref() == Some(game_id) {
            self.selected_case_id = None;
        }
        if self.patch_game_id.as_deref() == Some(game_id) && !self.patch_busy() {
            self.patch_game_id = None;
            self.patch_state = Default::default();
        }
        self.last_excluded = Some(game_id.to_owned());
        self.show_snackbar(
            tr!("已从资料库移除《{title}》", title = game.display_title()),
            Some(snackbar::SnackbarAction::Undo),
        );
        self.model()
    }

    pub(super) fn undo_remove(&mut self) -> Result<ManagerViewModel, String> {
        let game_id = self
            .last_excluded
            .clone()
            .ok_or("SHINKI_EMU_UNDO_UNAVAILABLE")?;
        self.restore_game(&game_id)
    }

    pub(super) fn restore_game(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        if !self
            .library
            .restore_game(game_id)
            .map_err(|error| error.to_string())?
        {
            return Err("SHINKI_EMU_GAME_NOT_FOUND".into());
        }
        if self.last_excluded.as_deref() == Some(game_id) {
            self.last_excluded = None;
        }
        self.model()
    }

    pub(super) fn first_visible_game(&self) -> Result<Option<String>, String> {
        let excluded: BTreeSet<String> = self
            .library
            .excluded_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .collect();
        Ok(self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .find(|id| !excluded.contains(id)))
    }

    pub(super) fn open(mailbox: FrameMailbox) -> Result<Self, String> {
        Self::open_library(platform_data_dir()?, mailbox, parse_game_roots())
    }

    pub(super) fn open_library(
        data_dir: PathBuf,
        mailbox: FrameMailbox,
        game_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        fs::create_dir_all(&data_dir).map_err(|_| "SHINKI_EMU_DATA_DIRECTORY_CREATE")?;
        let mut library =
            Library::open(data_dir.join("library.sqlite3")).map_err(|error| error.to_string())?;
        library
            .settle_abandoned_sessions(unix_time_ms()?)
            .map_err(|error| error.to_string())?;
        translation::migrate_provider_book(&mut library)?;
        // Settle last session's staged packages before reading manifests. Native
        // initialization remains deferred until an explicit selected action.
        let cores = core_directory(&data_dir)?;
        fs::create_dir_all(&cores).map_err(|_| "SHINKI_EMU_CORES_DIRECTORY_CREATE")?;
        let pending_failures = apply_startup_core_packages(&cores);

        let registry = owner::FamilyOwner::new(data_dir.clone(), mailbox.clone())?;

        let input_mapping = library
            .load_input_mapping()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(default_vn_preset);
        let filter_settings = library
            .filter_settings()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let metadata_catalog = metadata_cache::select_catalog(
            &data_dir,
            metadata_cache::bundled_directory().as_deref(),
        );
        let metadata_sync_record = library
            .app_state::<metadata_sync::SyncRecord>(metadata_sync::METADATA_SYNC_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let metadata = MetadataRuntime::start()?;
        let auto_link_attempted = library
            .app_state::<BTreeSet<String>>(metadata_auto_link::AUTO_LINK_ATTEMPTED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let appearance = library.appearance_settings().map_err(|e| e.to_string())?;
        let games = library.list_games().map_err(|error| error.to_string())?;
        let mut pinned_releases = BTreeMap::new();
        for game in &games {
            if let Some(release) = library
                .game_release(&game.game_id)
                .map_err(|error| error.to_string())?
            {
                pinned_releases.insert(game.game_id.clone(), release);
            }
        }
        let network_allowed = library
            .app_state::<bool>(NETWORK_ALLOWED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let translation_enabled = library
            .app_state::<bool>(TRANSLATION_ENABLED_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let sensitive_covers = library
            .app_state::<bool>(SENSITIVE_COVERS_KEY)
            .map_err(|error| error.to_string())?
            .unwrap_or(false);
        let self_update = self_update::State::load(&library)?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let optical_games = games.iter().filter(|game| optical::is_optical(&game.location))
            .filter_map(|game| match library.app_state(&game.location) {
                Ok(Some(binding)) => Some(Ok((game.location.clone(), binding))),
                Ok(None) => None,
                Err(error) => Some(Err(error.to_string())),
            }).collect::<Result<_, _>>()?;
        let mut controller = Self {
            library,
            self_update,
            registry,
            candidates: BTreeMap::new(),
            startup_state: background::StartupState::Waiting,
            native_action: None,
            native_failures: BTreeMap::new(),
            configuration_errors: BTreeMap::new(),
            probe_choices: BTreeMap::new(),
            manual_games: BTreeSet::new(),
            manual_launch: None,
            selected_case_id: None,
            search_query: String::new(),
            diagnostic: String::new(),
            retired_diagnostic: None,
            diagnostic_severity: "info",
            plugin_errors: pending_failures.into_iter().collect(),
            store: store::StoreRuntime::new(&data_dir),
            data_dir,
            extensions: None,
            game_roots,
            mailbox,
            active: None,
            active_play_session: None,
            pending_events: Vec::new(),
            window_state: None,
            host_wake: None,
            physical_keys: Vec::new(),
            library_sort: "title".into(),
            compatibility_filter: "all".into(),
            metadata_catalog,
            metadata_sync: metadata_sync::SyncState {
                record: metadata_sync_record,
                ..Default::default()
            },
            pinned_releases,
            filter_settings,
            filter_selected_stage: 0,
            filter_rollback: None,
            input_mapping,
            appearance,
            session_mapping: None,
            defaults_plugin: None,
            metadata,
            metadata_sequence: 0,
            metadata_status: BTreeMap::new(),
            pending_metadata: BTreeMap::new(),
            match_candidates: BTreeMap::new(),
            auto_link_queue: VecDeque::new(),
            auto_link: None,
            auto_link_attempted,
            network_allowed,
            metadata_tokens: BTreeMap::new(),
            metadata_tokens_loaded: false,
            sensitive_covers,
            bangumi_play_status: "wish".into(),
            bangumi_rating: 0,
            bangumi_note: String::new(),
            releases: BTreeMap::new(),
            translation_enabled,
            translation_models: Vec::new(),
            translation_status: String::new(),
            translation_test_for: String::new(),
            translation_saved: 0,
            connection_test: None,
            core_downloads: crate::core_download::CoreDownloads::github(),
            core_failures: BTreeMap::new(),
            core_batch: None,
            held_controls: BTreeSet::new(),
            key_modifiers: KeyModifiers {
                shift: false,
                control: false,
                alt: false,
                super_key: false,
            },
            pointer_position: (0.0, 0.0),
            action_message: String::new(),
            action_serial: 0,
            patch_job: None,
            patch_import: None,
            patch_state: Default::default(),
            patch_release_mismatch: false,
            patch_game_id: None,
            patch_query: String::new(),
            patch_sequence: 0,
            snackbar: snackbar::Snackbar::default(),
            pending_full_scan: Some(background::FullScan::Initial),
            pending_scan_roots: BTreeMap::new(),
            library_scan: None,
            prepare_progress: None,
            scan_cancel: None,
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            optical_games,
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            opened_optical: BTreeMap::new(),
            #[cfg(target_os = "android")]
            saf_paths: BTreeMap::new(),
            last_excluded: None,
            key_capture: None,
        };
        controller.selected_case_id = controller.first_visible_game()?;
        controller.restore_recognition()?;
        Ok(controller)
    }

    pub(super) fn game(&self, game_id: &str) -> Result<GameRecord, String> {
        self.library
            .game(game_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "SHINKI_EMU_GAME_NOT_FOUND".into())
    }

    pub(super) fn selected_game(&self) -> Result<GameRecord, String> {
        self.selected_case_id
            .as_deref()
            .ok_or_else(|| "SHINKI_EMU_CASE_SELECTION_MISSING".to_owned())
            .and_then(|id| self.game(id))
    }

    pub(super) fn set_probe_candidates(
        &mut self,
        game_id: &str,
        choices: Vec<Candidate>,
    ) -> Result<(), String> {
        let detections = self.library.app_state::<Vec<shinki_emu_manager_core::detection::EngineDetection>>(&format!("recognition:{game_id}"))
            .map_err(|error| error.to_string())?;
        if choices.is_empty() && detections.as_ref().is_some_and(Vec::is_empty) {
            return self.set_manual_candidates(game_id);
        }
        self.clear_probe_state(game_id)?;
        let saved = self
            .library
            .game_settings(game_id)
            .map_err(|error| error.to_string())?
            .and_then(|settings| settings.emulator_id);
        let ambiguous_family = detections.is_some_and(|detections| detections.len() > 1);
        let selected = match saved.as_deref() {
            Some(id) => choices.iter().find(|choice| choice.plugin_id == id),
            None if choices.len() == 1 && !ambiguous_family => choices.first(),
            None => None,
        }
        .map(|choice| choice.plugin_id.clone());
        if saved.is_none() && selected.is_none() && !ambiguous_family
            && let Some(first) = choices.first().filter(|first| {
                choices
                    .iter()
                    .all(|choice| choice.family_id == first.family_id)
            })
        {
            self.library
                .set_game_family(game_id, Some(&first.family_id))
                .map_err(|error| error.to_string())?;
        }
        self.probe_choices.insert(game_id.to_owned(), choices);
        if let Some(plugin_id) = selected {
            self.install_probe_choice(game_id, &plugin_id)?;
        }
        Ok(())
    }

    pub(super) fn install_probe_choice(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<(), String> {
        let candidate = self
            .probe_choices
            .get(game_id)
            .and_then(|choices| {
                choices
                    .iter()
                    .find(|choice| choice.plugin_id == plugin_id)
            })
            .cloned()
            .ok_or_else(|| "SHINKI_EMU_EMULATOR_SELECTION_INVALID".to_owned())?;
        self.library
            .select_game_emulator(
                game_id,
                &candidate.family_id,
                &candidate.plugin_id,
            )
            .map_err(|error| error.to_string())?;
        self.candidates.insert(game_id.to_owned(), candidate);
        Ok(())
    }

    pub(super) fn probe_selection_for_game(
        &self,
        game_id: &str,
    ) -> Option<&[Candidate]> {
        self.probe_choices.get(game_id).map(Vec::as_slice)
    }

    pub(super) fn clear_probe_state(&mut self, game_id: &str) -> Result<(), String> {
        self.candidates.remove(game_id);
        self.probe_choices.remove(game_id);
        self.manual_games.remove(game_id);
        Ok(())
    }

    /// A game without rule detections may run any eligible official core the
    /// user picks. Nothing is picked for the user, even with one choice, and no
    /// core is activated or probed before the launch itself.
    fn set_manual_candidates(&mut self, game_id: &str) -> Result<(), String> {
        let pending = self.manual_games.contains(game_id)
            .then(|| self.candidates.get(game_id).map(|candidate| candidate.plugin_id.clone()))
            .flatten();
        self.clear_probe_state(game_id)?;
        self.manual_games.insert(game_id.to_owned());
        let choices: Vec<Candidate> = shinki_emu_manager_core::static_core_catalog::entries()
            .iter()
            .filter(|entry| self.plugin_available(entry.plugin_id))
            .map(|entry| Candidate { plugin_id: entry.plugin_id.into(), family_id: entry.family_id.into() })
            .collect();
        // Without an eligible core the existing unavailable state stays; no picker.
        if choices.is_empty() {
            return Ok(());
        }
        let selected = match pending {
            Some(id) => Some(id),
            None => self.verified_manual_core(game_id)?,
        };
        if let Some(candidate) = selected.and_then(|id| choices.iter().find(|choice| choice.plugin_id == id)) {
            self.candidates.insert(game_id.to_owned(), candidate.clone());
        }
        self.probe_choices.insert(game_id.to_owned(), choices);
        Ok(())
    }

    /// The saved core of a game without rule detections, if a session with it
    /// once produced a frame. Detector-era bindings are not carried over.
    pub(super) fn verified_manual_core(&self, game_id: &str) -> Result<Option<String>, String> {
        let verified = self.library.app_state::<String>(&manual_core_key(game_id)).map_err(|error| error.to_string())?;
        let saved = self.library.game_settings(game_id).map_err(|error| error.to_string())?
            .and_then(|settings| settings.emulator_id);
        Ok(verified.filter(|id| saved.as_ref() == Some(id)))
    }

    /// The core shown as selected: for a manual game the pending or verified
    /// manual choice, otherwise the saved binding.
    pub(super) fn selected_emulator(&self, game_id: &str) -> Result<Option<String>, String> {
        if !self.manual_games.contains(game_id) {
            return Ok(self.library.game_settings(game_id).map_err(|error| error.to_string())?
                .and_then(|settings| settings.emulator_id));
        }
        match self.candidates.get(game_id) {
            Some(candidate) => Ok(Some(candidate.plugin_id.clone())),
            None => self.verified_manual_core(game_id),
        }
    }

    /// A manual pick that has not yet shown a frame. Its settings are not
    /// prepared; launching is the only step that loads and validates the core.
    pub(super) fn manual_pick_pending(&self, game_id: &str) -> Result<bool, String> {
        let Some(candidate) = self.candidates.get(game_id).filter(|_| self.manual_games.contains(game_id)) else {
            return Ok(false);
        };
        Ok(self.verified_manual_core(game_id)?.as_deref() != Some(candidate.plugin_id.as_str()))
    }

    /// Core whose settings may be prepared. A manual game only offers its
    /// verified pick; an older saved binding is never used for it.
    pub(super) fn configurable_emulator(&self, game_id: &str) -> Result<String, String> {
        let plugin = if self.manual_games.contains(game_id) {
            self.verified_manual_core(game_id)?
                .filter(|id| self.candidates.get(game_id).is_some_and(|candidate| candidate.plugin_id == *id))
        } else {
            self.library.game_settings(game_id).map_err(|error| error.to_string())?
                .and_then(|settings| settings.emulator_id)
        };
        plugin.ok_or_else(|| "SHINKI_EMU_EMULATOR_SELECTION_REQUIRED".into())
    }

    /// Remember a manual core only after the session it opened published a
    /// valid frame. A failed probe/open or a close before that frame leaves the
    /// previous saved choice untouched.
    pub(super) fn commit_manual_launch(&mut self) -> Result<(), String> {
        let Some(epoch) = self.manual_launch.as_ref().map(|launch| launch.0) else {
            return Ok(());
        };
        if self.active.as_ref().is_none_or(|active| active.epoch != epoch) {
            self.manual_launch = None;
            return Ok(());
        }
        if self.mailbox.identity()?.is_none_or(|identity| identity.session_epoch != epoch) {
            return Ok(());
        }
        let Some((_, game_id, plugin_id)) = self.manual_launch.take() else {
            return Ok(());
        };
        let mut settings = self.library.game_settings(&game_id).map_err(|error| error.to_string())?
            .unwrap_or_default();
        if settings.emulator_id.as_deref() != Some(plugin_id.as_str()) {
            settings.emulator_id = Some(plugin_id.clone());
            self.library.set_game_settings(&game_id, &settings).map_err(|error| error.to_string())?;
        }
        let key = manual_core_key(&game_id);
        if self.library.app_state::<String>(&key).map_err(|error| error.to_string())?.as_deref() != Some(plugin_id.as_str()) {
            self.library.set_app_state(&key, &plugin_id).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn manual_core_key(game_id: &str) -> String {
    format!("manual-core:{game_id}")
}

#[cfg(test)]
#[path = "manual_choice_tests.rs"]
mod manual_choice_tests;
