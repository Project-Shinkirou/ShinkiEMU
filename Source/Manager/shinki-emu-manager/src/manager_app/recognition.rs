use super::*;
use shinki_emu_manager_core::{detection::EngineDetection, static_core_catalog};

/// A static association, never a runtime descriptor or native probe result.
#[derive(Clone)]
pub(super) enum NativeAction {
    ConfigureGame { game_id: String, plugin_id: String },
    Diagnose(String),
    Defaults(String),
    Launch { game_id: String, plugin_id: String },
}

impl NativeAction {
    #[cfg(any(target_os = "android", test))]
    pub(super) fn is_launch(&self) -> bool {
        matches!(self, Self::Launch { .. })
    }

    pub(super) fn plugin_id(&self) -> &str {
        match self {
            Self::ConfigureGame { plugin_id, .. } | Self::Launch { plugin_id, .. } => plugin_id,
            Self::Diagnose(id) | Self::Defaults(id) => id,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Candidate {
    pub(super) plugin_id: String,
    pub(super) family_id: String,
}

pub(super) struct GameConfigurationError {
    pub(super) plugin_id: String,
    pub(super) message: String,
}

pub(super) fn directories(data_dir: &Path) -> Result<Vec<PathBuf>, String> {
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    { core_directories(&data_dir.join("cores")) }
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    { let _ = data_dir; Ok(Vec::new()) }
}

pub(super) fn is_static(plugin_id: &str) -> bool {
    shinki_emu_manager::static_cores::STATIC_CORES.iter().any(|(id, _)| *id == plugin_id)
}

pub(super) fn choices(detections: &[EngineDetection]) -> Vec<Candidate> {
    static_core_catalog::entries().iter()
        .filter(|entry| entry.supported && detections.iter().any(|item| item.family_id == entry.family_id))
        .map(|entry| Candidate { plugin_id: entry.plugin_id.into(), family_id: entry.family_id.into() })
        .collect()
}

#[derive(Clone)]
pub(super) struct InstalledCore {
    pub(super) manifest: core_package::CoreManifest,
    pub(super) library: PathBuf,
    pub(super) bundled: bool,
}

/// Read package identities only. Bare libraries cannot establish an identity.
pub(super) fn installed(directories: &[PathBuf]) -> Result<BTreeMap<String, InstalledCore>, String> {
    let mut found = BTreeMap::new();
    let mut duplicate = BTreeSet::new();
    let mut stems = BTreeSet::new();
    for (index, directory) in directories.iter().enumerate() {
        for entry in fs::read_dir(directory).map_err(|error| format!("SHINKI_EMU_CORE_DIRECTORY: {error}"))? {
            let entry = entry.map_err(|error| format!("SHINKI_EMU_CORE_DIRECTORY: {error}"))?;
            let stem = entry.file_name();
            if stem.to_str().is_none_or(|stem| stem.starts_with('.')) || !entry.file_type().map_err(|error| error.to_string())?.is_dir() {
                continue;
            }
            let Some(manifest) = core_package::read_manifest(&entry.path()) else { continue };
            if !stems.insert(shinki_emu_manager_core::library_stem(Path::new(&manifest.library))) { continue; }
            let id = manifest.plugin_id.clone();
            let core = InstalledCore { library: entry.path().join(&manifest.library), manifest, bundled: index > 0 };
            if !core.library.is_file() { continue; }
            if found.insert(id.clone(), core).is_some() { duplicate.insert(id); }
        }
    }
    for id in duplicate { found.remove(&id); }
    Ok(found)
}

impl ShinkiEmuManagerController {
    pub(super) fn plugin_available(&self, plugin_id: &str) -> bool {
        static_core_catalog::lookup(plugin_id).is_some_and(|entry| (entry.supported || is_static(plugin_id)) && (is_static(plugin_id) || self.registry.installed().contains_key(plugin_id)))
    }

    pub(super) fn restore_recognition(&mut self) -> Result<(), String> {
        for game in self.library.list_games().map_err(|error| error.to_string())? {
            let detections = self.library.app_state::<Vec<EngineDetection>>(&format!("recognition:{}", game.game_id)).map_err(|error| error.to_string())?;
            if let Some(detections) = detections {
                self.set_probe_candidates(&game.game_id, choices(&detections))?;
            } else if let Some(family) = game.family_id {
                // Legacy records retain identity and binding. Fresh selected probe is
                // still mandatory before open; no format/runtime claim is invented.
                let choices = static_core_catalog::entries().iter().filter(|entry| entry.supported && entry.family_id == family)
                    .map(|entry| Candidate { plugin_id: entry.plugin_id.into(), family_id: family.clone() }).collect();
                self.set_probe_candidates(&game.game_id, choices)?;
            }
        }
        Ok(())
    }

    pub(super) fn set_recognition(&mut self, game_id: &str, detections: Vec<EngineDetection>) -> Result<(), String> {
        self.library.set_app_state(&format!("recognition:{game_id}"), &detections).map_err(|error| error.to_string())?;
        if let [detection] = detections.as_slice() {
            self.library.set_game_family(game_id, Some(&detection.family_id)).map_err(|error| error.to_string())?;
        }
        self.set_probe_candidates(game_id, choices(&detections))
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn configuration_state(&self, plugin_id: Option<&str>) -> &'static str {
        let Some(id) = plugin_id else { return "selection-required"; };
        if self.native_action.as_ref().is_some_and(|action| action.plugin_id() == id) { return "loading"; }
        if self.native_failures.contains_key(id) { return "failed"; }
        if self.registry.descriptor(id).is_some() { return "ready"; }
        if !self.plugin_available(id) { return "missing-core"; }
        "unprepared"
    }

    pub(super) fn game_configuration_error(&self, game_id: &str, plugin_id: Option<&str>) -> &str {
        let Some(plugin_id) = plugin_id else { return ""; };
        self.configuration_errors.get(game_id)
            .filter(|error| error.plugin_id == plugin_id)
            .map(|error| error.message.as_str())
            .or_else(|| self.native_failures.get(plugin_id).map(String::as_str))
            .unwrap_or("")
    }

    pub(super) fn request_activation(&mut self, action: NativeAction) -> Result<(), String> {
        self.ensure_owner_idle()?;
        if self.native_action.is_some() || self.startup_busy() { return Err("SHINKI_EMU_PREPARE_BUSY".into()); }
        let plugin = action.plugin_id().to_owned();
        if static_core_catalog::lookup(&plugin).is_none() && !self.registry.installed().contains_key(&plugin) && self.registry.descriptor(&plugin).is_none() {
            return Err("SHINKI_EMU_CORE_UNKNOWN_PLUGIN".into());
        }
        self.native_failures.remove(&plugin);
        if let NativeAction::ConfigureGame { game_id, .. } | NativeAction::Launch { game_id, .. } = &action {
            self.configuration_errors.remove(game_id);
        }
        self.native_action = Some(action);
        if self.registry.is_ready() {
            if let Err(error) = self.registry.activate(&plugin) {
                self.native_action = None;
                return Err(error);
            }
        } else if !self.cores_started() {
            if let Err(error) = self.refresh_cores() {
                self.native_action = None;
                return Err(error);
            }
        } else {
            self.native_action = None;
            return Err("SHINKI_EMU_PREPARE_RESTART_REQUIRED".into());
        }
        Ok(())
    }

    pub(super) fn prepare_game_configuration(&mut self, game_id: &str) -> Result<ManagerViewModel, String> {
        let plugin_id = self.configurable_emulator(game_id)?;
        self.game(game_id)?;
        self.request_activation(NativeAction::ConfigureGame { game_id: game_id.into(), plugin_id })?;
        self.model()
    }

    pub(super) fn diagnose_core(&mut self, plugin_id: &str) -> Result<ManagerViewModel, String> {
        self.request_activation(NativeAction::Diagnose(plugin_id.into()))?;
        self.model()
    }

    pub(super) fn activated(&mut self, plugin_id: String, result: Result<owner::CoreMetadata, String>) -> Result<(), String> {
        #[cfg(target_os = "android")]
        if self.native_action.as_ref().is_some_and(|action| action.plugin_id() != plugin_id) {
            // A canceled launch's activation can finish after a newer request.
            return Ok(());
        }
        let Some(action) = self.native_action.take() else { return Ok(()); };
        if action.plugin_id() != plugin_id { return Err("SHINKI_EMU_CORE_ACTIVATION_MISMATCH".into()); }
        match result {
            Err(error) => {
                self.native_failures.insert(plugin_id, error.clone());
                self.diagnostic = error;
                self.diagnostic_severity = "error";
            }
            Ok(_) => {
                self.native_failures.remove(&plugin_id);
                let game_id = match &action {
                    NativeAction::ConfigureGame { game_id, .. } | NativeAction::Launch { game_id, .. } => Some(game_id.clone()),
                    _ => None,
                };
                let completion = match action {
                    NativeAction::Launch { game_id, .. } => self.launch(&game_id).map(|_| ()),
                    NativeAction::ConfigureGame { game_id, .. } => self.apply_pending_profile(&game_id, &plugin_id),
                    NativeAction::Defaults(_) => Ok(()),
                    NativeAction::Diagnose(_) => Ok(()),
                };
                if let Err(error) = completion {
                    if let Some(game_id) = game_id {
                        self.configuration_errors.insert(game_id, GameConfigurationError { plugin_id, message: error.clone() });
                    }
                    self.diagnostic = error;
                    self.diagnostic_severity = "error";
                }
            }
        }
        Ok(())
    }
}
