use super::*;
use shinki_emu_manager_ui_slint::tr;

impl ShinkiEmuManagerController {
    pub(super) fn launch_candidate_for_game(
        &self,
        game: &GameRecord,
    ) -> Option<&Candidate> {
        if !self.cores_ready() { return None; }
        let candidate = self.candidates.get(&game.game_id)?;
        // A manual game binds its family from the chosen core only in memory.
        if (!self.manual_games.contains(&game.game_id)
            && game.family_id.as_deref() != Some(candidate.family_id.as_str()))
            || !self.plugin_available(&candidate.plugin_id)
        {
            return None;
        }
        Some(candidate)
    }

    pub(super) fn select_case(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if self.session_running() {
            return Err("SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        if case_id.is_empty() {
            self.selected_case_id = None;
        } else {
            self.game(case_id)?;
            self.selected_case_id = Some(case_id.into());
        }
        self.model()
    }

    pub(super) fn search(&mut self, query: &str) -> Result<ManagerViewModel, String> {
        if query.chars().count() > 256 {
            return Err("SHINKI_EMU_SEARCH_QUERY_BOUNDS".into());
        }
        self.search_query = query.into();
        self.model()
    }

    pub(super) fn launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        let retired_before = self.diagnostic.clone();
        let result = self.try_launch(case_id);
        if let Err(error) = &result && self.diagnostic == retired_before {
            self.diagnostic.clone_from(error);
            self.diagnostic_severity = "error";
        }
        result
    }

    fn try_launch(&mut self, case_id: &str) -> Result<ManagerViewModel, String> {
        if self.patch_busy() {
            return Err(
                format!(
                    "SHINKI_EMU_PATCH_INVALID_REQUEST: {}",
                    tr!("文件补丁转换或导入期间不能启动游戏")
                ),
            );
        }
        tracing::info!(event = "shinki.emu.manager.launch.opening");
        self.ensure_owner_idle()?;
        if self.startup_busy() || !self.cores_ready() {
            return Err("SHINKI_EMU_FAMILY_INITIALIZING".into());
        }
        let game = self.game(case_id)?;
        #[cfg(target_os = "android")]
        let game = if shinki_emu_manager::platform::android_tree::is_library_uri(&game.location) {
            let path = self.native_game_directory(&game)?;
            let location = path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?.to_owned();
            GameRecord { location, ..game }
        } else {
            game
        };
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let optical = self.validate_optical_launch(&game)?;
        #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
        let game = if let Some(binding) = &optical {
            let location = binding.path.to_str().ok_or("SHINKI_EMU_GAME_PATH_UTF8")?.to_owned();
            GameRecord { location, ..game }
        } else {
            game
        };
        if shinki_emu_patch::is_staging_path(Path::new(&game.location)) {
            return Err(
                "SHINKI_EMU_PATCH_INVALID_REQUEST: private staging is not a published game".into(),
            );
        }
        let session_mapping = self.game_input_mapping(case_id)?;
        let saved_plugin = self.selected_emulator(case_id)?;
        let candidate = self.candidates.get(case_id).cloned().ok_or_else(|| match saved_plugin {
            Some(id) if !self.plugin_available(&id) => "SHINKI_EMU_CORE_MISSING",
            Some(_) => "SHINKI_EMU_FAMILY_PROBE_REQUIRED",
            None => "SHINKI_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED",
        })?;
        if !self.plugin_available(&candidate.plugin_id) { return Err("SHINKI_EMU_CORE_MISSING".into()); }
        let manual = self.manual_games.contains(case_id);
        let game = if manual {
            GameRecord { family_id: Some(candidate.family_id.clone()), ..game }
        } else {
            game
        };
        if game.family_id.as_deref() != Some(candidate.family_id.as_str()) {
            return Err("SHINKI_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        let Some(descriptor) = self.registry.descriptor(&candidate.plugin_id).cloned() else {
            self.request_activation(recognition::NativeAction::Launch { game_id: case_id.into(), plugin_id: candidate.plugin_id })?;
            return self.model();
        };
        self.apply_pending_profile(case_id, &candidate.plugin_id)?;
        if let Some(profile) = shinki_emu_patch::read_output_profile(
            Path::new(&game.location),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .map_err(|error| error.to_string())?
        {
            file_patch::require_profile_capabilities(&profile, &descriptor)?;
        }
        let epoch = shinki_emu_manager_core::model::allocate_session_epoch()
            .map_err(|error| error.to_string())?;
        let text = if descriptor
            .has_capability(shinki_emu_manager_core::FamilyCapability::TextReplacement)
        {
            let profile = self.launch_translation_profile()?;
            let extensions =
                shinki_emu_manager_core::extensions::ExtensionStore::new(&self.data_dir).list()?;
            if profile.is_some() || extensions.iter().any(|extension| extension.enabled) {
                if self.extensions.is_none() {
                    self.extensions = Some(
                        crate::text_service::ExtensionRuntime::new()
                            .map_err(|error| error.to_string())?,
                    );
                }
                let secrets = std::sync::Arc::new(
                    ManagerSecretStore::open().map_err(|error| error.to_string())?,
                );
                Some(
                    TextReplacementBridge::new(epoch, profile, secrets, extensions)
                        .map_err(|error| error.to_string())?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let wake = self
            .host_wake
            .clone()
            .ok_or("SHINKI_EMU_HOST_WAKE_REQUIRED")?;
        let schema = self
            .registry
            .configuration(&candidate.plugin_id)
            .map_err(|error| error.to_string())?;
        let configuration = self
            .library
            .family_configuration(&candidate.plugin_id, case_id, &schema)
            .map_err(|error| error.to_string())?;
        let retired_path_reported = self.report_retired_configuration(&configuration.retired);
        let configuration = configuration.values;
        let window = self.window_state.ok_or("SHINKI_EMU_WINDOW_STATE_REQUIRED")?;
        let settings = self
            .library
            .game_settings(case_id)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        let preference =
            self.effective_render_preference(&candidate.plugin_id, &settings)?;
        let render_extent = render_extent::resolve_render_extent(
            descriptor.render.as_ref(),
            &preference,
            (window.width, window.height),
            settings
                .last_surface
                .map(|surface| (surface.logical_width, surface.logical_height)),
        )?;
        self.mailbox
            .activate(epoch)
            .map_err(|error| error.to_string())?;
        let plugin_id = candidate.plugin_id.clone();
        self.registry.open(owner::OpenSession {
            epoch,
            game,
            #[cfg(any(windows, target_os = "macos", all(target_os = "linux", not(target_env = "ohos"))))]
            optical: optical.map(|binding| binding.volume),
            candidate,
            configuration,
            window,
            render_extent,
            text,
            wake,
            modal: None,
            window_service: None,
            cancellation: shinki_emu_manager_core::NativeCancellation::new(epoch),
        })?;
        self.manual_launch = manual.then(|| (epoch, case_id.to_owned(), plugin_id));
        self.selected_case_id = Some(case_id.into());
        self.session_mapping = Some(session_mapping);
        self.pending_events.clear();
        self.active = Some(owner::ActiveSessionView::opening(epoch, case_id.into()));
        if !retired_path_reported {
            self.diagnostic.clear();
        }
        self.model()
    }

    pub(super) fn leave_game(&mut self) -> Result<ManagerViewModel, String> {
        #[cfg(target_os = "android")]
        self.cancel_pending_game_launch();
        self.close_active(PlaySessionEndReason::Leave)?;
        self.pending_events.clear();
        self.model()
    }

    #[cfg(any(target_os = "android", test))]
    pub(super) fn cancel_pending_game_launch(&mut self) {
        if self.native_action.as_ref().is_some_and(recognition::NativeAction::is_launch) {
            self.native_action = None;
        }
    }

    pub(super) fn set_library_sort(&mut self, mode: &str) -> Result<ManagerViewModel, String> {
        if !matches!(mode, "title" | "recent" | "play_time") {
            return Err("SHINKI_EMU_LIBRARY_SORT_INVALID".into());
        }
        self.library_sort = mode.into();
        self.model()
    }

    pub(super) fn set_compatibility_filter(
        &mut self,
        filter: &str,
    ) -> Result<ManagerViewModel, String> {
        if !matches!(
            filter,
            "all" | "unknown" | "perfect" | "completable" | "flawed" | "boot_only" | "unplayable"
        ) {
            return Err("SHINKI_EMU_COMPATIBILITY_FILTER_INVALID".into());
        }
        self.compatibility_filter = filter.into();
        self.model()
    }
}
