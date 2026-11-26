use super::*;
use shinki_emu_manager_core::{detection::{DeclaredRules, EngineDetection}, static_core_catalog};
#[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
use shinki_emu_metadata::CoreRecognition;

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
    #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
    { core_directories(&core_directory(data_dir)?) }
    #[cfg(target_os = "android")]
    { Ok(vec![core_directory(data_dir)?]) }
    #[cfg(any(target_os = "ios", target_env = "ohos"))]
    { let _ = data_dir; Ok(Vec::new()) }
}

pub(super) fn is_static(plugin_id: &str) -> bool {
    static_query(plugin_id).is_some()
}

fn static_query(plugin_id: &str) -> Option<shinki_emu_abi::Query> {
    shinki_emu_manager::static_cores::STATIC_CORES.iter()
        .find_map(|(id, query)| (*id == plugin_id).then_some(*query))
}

pub(super) fn supports_installed_packages(plugin_id: &str) -> bool {
    #[cfg(target_os = "android")]
    { static_core_catalog::lookup(plugin_id).is_some_and(|core| core.android_dynamic.is_some()) }
    #[cfg(any(target_os = "ios", target_env = "ohos"))]
    { let _ = plugin_id; false }
    #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
    { supported_core(plugin_id).is_ok() }
}

pub(super) fn allows_installed_release(
    plugin_id: &str,
    version: &str,
    identity: Option<&shinki_emu_metadata::CoreIdentity>,
) -> bool {
    #[cfg(target_os = "android")]
    {
        let Some(policy) = static_core_catalog::lookup(plugin_id).and_then(|core| core.android_dynamic) else {
            return false;
        };
        android_release_eligible(
            policy.minimum,
            is_static(plugin_id),
            cores::embedded_identity(plugin_id),
            cores::embedded_product_release(plugin_id).map(|(version, _)| version),
            version,
            identity,
        )
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (version, identity);
        supports_installed_packages(plugin_id)
    }
}

#[cfg(any(target_os = "android", test))]
fn android_release_eligible(
    floor: static_core_catalog::AndroidMinimumRelease,
    linked: bool,
    embedded: Option<&shinki_emu_metadata::CoreIdentity>,
    embedded_product_version: Option<&str>,
    version: &str,
    identity: Option<&shinki_emu_metadata::CoreIdentity>,
) -> bool {
    use static_core_catalog::AndroidMinimumRelease;
    match floor {
        AndroidMinimumRelease::Sequence(minimum) => identity.is_some_and(|candidate| {
            candidate.release_sequence >= minimum
                && (!linked || embedded.is_some_and(|old| candidate.release_sequence > old.release_sequence))
        }),
        AndroidMinimumRelease::Semver(minimum) => {
            identity.is_none() && version != "0.0.0-local"
                && semver::Version::parse(version).is_ok_and(|candidate| {
                    let minimum = semver::Version::parse(minimum)
                        .expect("validated Android release floor");
                    !candidate.cmp_precedence(&minimum).is_lt()
                        && (!linked || embedded_product_version.is_some_and(|embedded| {
                            semver::Version::parse(embedded).is_ok_and(|embedded| {
                                candidate.cmp_precedence(&embedded).is_gt()
                            })
                        }))
                })
        }
    }
}

#[cfg(test)]
mod android_policy_tests {
    use super::*;
    use static_core_catalog::AndroidMinimumRelease::{Semver, Sequence};

    fn legacy_siglus() -> shinki_emu_metadata::CoreCatalogEntry {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../Metadata/engines/cores/siglus_rs/android-x86_64/",
            "tag-1-0-0-b800731f5cb5a67479e6f5fe98d89882dd534f864bfff17aa0209993a5065482.json"
        ))).unwrap()
    }

    #[test]
    fn old_official_android_releases_do_not_shadow_or_become_dynamic_only() {
        let siglus = legacy_siglus();
        let artemis: shinki_emu_metadata::CoreCatalogEntry = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../Metadata/engines/cores/art3m1s-core/android-x86_64/",
            "branch-master-4a8daa07610e299a8ff8c7ccce8bed61dbc28af2810ee081c1241cc182c0e894.json"
        ))).unwrap();
        for release in [siglus, artemis] {
            release.validate().unwrap();
            let old = release.identity.as_ref().unwrap();
            let minimum = old.release_sequence + 2;
            let mut candidate = old.clone();
            candidate.release_sequence = minimum;
            let floor = Sequence(minimum);
            assert!(!android_release_eligible(floor, false, None, None, &release.version, Some(old)));
            assert!(!android_release_eligible(floor, false, None, None, "1.0.0", None));
            assert!(android_release_eligible(floor, false, None, None, &candidate.version(), Some(&candidate)));
            assert!(android_release_eligible(floor, true, Some(old), None, &candidate.version(), Some(&candidate)));
            assert!(!android_release_eligible(floor, true, Some(&candidate), None, &candidate.version(), Some(&candidate)));
            assert!(!android_release_eligible(floor, true, None, None, &candidate.version(), Some(&candidate)));
        }
    }

    #[test]
    fn in_repo_release_needs_no_external_sequence_or_unknown_static_overlay() {
        let floor = Semver("1.2.3");
        assert!(android_release_eligible(floor, false, None, None, "1.2.3", None));
        assert!(android_release_eligible(floor, false, None, None, "1.3.0", None));
        assert!(!android_release_eligible(floor, false, None, None, "1.2.3-rc.1", None));
        assert!(!android_release_eligible(floor, true, None, None, "2.0.0", None));
        let stamped = Some("1.3.0+authenticated");
        assert!(!android_release_eligible(floor, true, None, stamped, "1.3.0+different", None));
        assert!(!android_release_eligible(floor, true, None, stamped, "1.2.4", None));
        assert!(android_release_eligible(floor, true, None, stamped, "1.3.1", None));
    }

    #[test]
    fn online_selection_ranks_only_android_releases_that_can_be_installed() {
        let legacy = legacy_siglus();
        let platform = &legacy.platform;
        let abi = shinki_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT;
        let product = |version: &str, platform: &str, abi: &str| {
            shinki_emu_metadata::CoreCatalogEntry {
                version: version.into(),
                platform: platform.into(),
                abi: abi.into(),
                identity: None,
                ..legacy.clone()
            }
        };
        let releases = [
            legacy.clone(),
            product("1.2.3-rc.1", platform, abi),
            product("1.2.3", platform, abi),
            product("1.3.0", platform, abi),
            product("2.0.0", "android-aarch64", abi),
            product("3.0.0", platform, "incompatible-abi"),
        ];
        let select = |releases| cores::latest_eligible_release(
            releases, &legacy.plugin_id, platform, |entry| {
                android_release_eligible(
                    Semver("1.2.3"), false, None, None, &entry.version, entry.identity.as_ref(),
                )
            },
        );
        assert!(select(&releases[..2]).is_none());
        assert_eq!(select(&releases[..3]).unwrap().version, "1.2.3");
        let update = select(&releases).unwrap();
        assert_eq!(update.version, "1.3.0");
        assert!(update.is_update_for("1.2.3", None));
    }

    #[test]
    fn rejecting_a_staged_legacy_release_preserves_the_installed_core() {
        let root = tempfile::tempdir().unwrap();
        let stem = "libshinki_emu_siglus_rs";
        let library = format!("{stem}.{}", env::consts::DLL_EXTENSION);
        let live = root.path().join(stem);
        let stage = root.path().join(core_package::PENDING).join(stem);
        let old = legacy_siglus().identity.unwrap();
        let mut new = old.clone();
        new.release_sequence += 2;
        let floor = Sequence(new.release_sequence);
        for (dir, identity, bytes) in [
            (&live, new, b"supported core" as &[u8]),
            (&stage, old, b"legacy core" as &[u8]),
        ] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(&library), bytes).unwrap();
            let manifest = core_package::CoreManifest {
                schema: core_package::CORE_PACKAGE_SCHEMA.into(),
                plugin_id: "siglus_rs".into(),
                version: identity.version(),
                identity: Some(identity),
                library: library.clone(),
                recognition: Default::default(),
                abi: shinki_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
                files: vec![core_package::CoreFile {
                    path: library.clone(),
                    sha256: core_package::sha256_hex(bytes),
                }],
            };
            fs::write(dir.join(core_package::CORE_MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        }
        let policy = |dir: &Path| {
            let manifest = core_package::read_manifest(dir).unwrap();
            android_release_eligible(floor, false, None, None, &manifest.version, manifest.identity.as_ref())
        };
        let failures = core_package::apply_pending_checked(root.path(), |_, dir| {
            policy(dir).then_some(()).ok_or("legacy release")
        });
        assert_eq!(failures.len(), 1);
        assert!(policy(&live));
        assert_eq!(fs::read(live.join(&library)).unwrap(), b"supported core");
        assert_eq!(fs::read(stage.join(&library)).unwrap(), b"legacy core");
    }
}

fn is_supported(core: &static_core_catalog::CoreCatalogEntry) -> bool {
    #[cfg(target_env = "ohos")]
    {
        is_static(core.plugin_id)
            || shinki_emu_manager::platform::ohos::bundled::configured_cores()
                .iter().any(|&(id, _)| id == core.plugin_id)
    }
    #[cfg(target_os = "android")]
    { is_static(core.plugin_id) || core.android_dynamic.is_some() }
    #[cfg(not(any(target_os = "android", target_env = "ohos")))]
    {
        if core.supported || is_static(core.plugin_id) {
            return true;
        }
        #[cfg(target_os = "ios")]
        { shinki_emu_manager::platform::bundled_core_ids().contains(&core.plugin_id) }
        #[cfg(not(target_os = "ios"))]
        false
    }
}

pub(super) fn supported_core(plugin_id: &str) -> Result<&'static static_core_catalog::CoreCatalogEntry, &'static str> {
    let core = static_core_catalog::lookup(plugin_id).ok_or("SHINKI_EMU_CORE_UNKNOWN_PLUGIN")?;
    if !is_supported(core) {
        return Err("SHINKI_EMU_CORE_UNSUPPORTED_TARGET");
    }
    Ok(core)
}

pub(super) fn runnable_core(
    installed: &BTreeMap<String, InstalledCore>, plugin_id: &str,
) -> Result<&'static static_core_catalog::CoreCatalogEntry, &'static str> {
    let core = supported_core(plugin_id)?;
    core_source(installed, plugin_id).ok_or("SHINKI_EMU_CORE_MISSING")?;
    Ok(core)
}

pub(super) enum CoreSource<'a> {
    #[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
    Static(shinki_emu_abi::Query),
    Package(&'a InstalledCore),
}

pub(super) fn core_source<'a>(
    installed: &'a BTreeMap<String, InstalledCore>,
    plugin_id: &str,
) -> Option<CoreSource<'a>> {
    let package = installed.get(plugin_id);
    #[cfg(target_env = "ohos")]
    if shinki_emu_manager::platform::ohos::bundled::configured_cores()
        .iter().any(|&(id, _)| id == plugin_id)
    {
        return package.filter(|core| core.bundled).map(CoreSource::Package);
    }
    #[cfg(target_os = "ios")]
    if shinki_emu_manager::platform::bundled_core_ids().contains(&plugin_id) {
        return package.filter(|core| core.bundled).map(CoreSource::Package);
    }
    // Android selects a verified install over a legacy linked copy.
    #[cfg(all(target_os = "android", any(feature = "static-cmvs", feature = "static-adv32")))]
    if let Some(package) = package {
        return Some(CoreSource::Package(package));
    }
    #[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
    if let Some(query) = static_query(plugin_id) {
        return Some(CoreSource::Static(query));
    }
    package.map(CoreSource::Package)
}

pub(super) fn choices(detections: &[EngineDetection]) -> Vec<Candidate> {
    static_core_catalog::entries().iter()
        .filter(|entry| is_supported(entry) && detections.iter().any(|item| {
            item.family_id == entry.family_id && item.declared.as_ref()
                .is_none_or(|declared| declared.plugin_ids.iter().any(|id| id == entry.plugin_id))
        }))
        .map(|entry| Candidate { plugin_id: entry.plugin_id.into(), family_id: entry.family_id.into() })
        .collect()
}

#[derive(Clone)]
pub(super) struct InstalledCore {
    pub(super) manifest: core_package::CoreManifest,
    pub(super) library: PathBuf,
    pub(super) source_commit: Option<String>,
    pub(super) bundled: bool,
}

pub(super) fn scan_rules(installed: &BTreeMap<String, InstalledCore>) -> DeclaredRules {
    #[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
    static EMBEDDED: std::sync::LazyLock<BTreeMap<&'static str, CoreRecognition>> = std::sync::LazyLock::new(|| {
        static_core_catalog::entries().iter()
            .filter(|entry| is_static(entry.plugin_id) && entry.embedded_recognition_json != "[]")
            .map(|entry| (
                entry.plugin_id,
                serde_json::from_str(entry.embedded_recognition_json).expect("validated embedded core recognition"),
            )).collect()
    });
    let rules: BTreeMap<_, _> = static_core_catalog::entries().iter().filter_map(|core| {
        let id = core.plugin_id;
        if !is_supported(core) { return None; }
        let declaration = match core_source(installed, id)? {
            #[cfg(any(feature = "static-cmvs", feature = "static-adv32"))]
            CoreSource::Static(_) => EMBEDDED.get(id)?,
            CoreSource::Package(package) => &package.manifest.recognition,
        };
        (!declaration.is_empty()).then_some((id, (core.family_id, declaration)))
    }).collect();
    if rules.is_empty() { return DeclaredRules::default(); }
    let canonical = serde_json::to_vec(&rules).expect("core recognition is serializable");
    let mut matcher = DeclaredRules::new(core_package::sha256_hex(&canonical));
    for (id, (_, declaration)) in rules {
        let core = static_core_catalog::lookup(id).expect("core from static catalog");
        for rule in declaration.rules() {
            matcher.add(core, rule.path(), rule.offset(), rule.magic_bytes());
        }
    }
    matcher
}

fn package_library(
    directory: &Path,
    stem: &str,
    manifest: &core_package::CoreManifest,
) -> Result<(PathBuf, String), &'static str> {
    if !allows_installed_release(&manifest.plugin_id, &manifest.version, manifest.identity.as_ref())
        || manifest.version == "0.0.0-local"
    {
        return Err("SHINKI_EMU_CORE_PACKAGE_RELEASE_UNSUPPORTED: install a compatible official Android release");
    }
    let library = directory.join(&manifest.library);
    let expected_stem = shinki_emu_manager_core::library_stem(&library);
    if !stem.eq_ignore_ascii_case(&expected_stem) {
        return Err("SHINKI_EMU_CORE_PACKAGE_MANIFEST");
    }
    #[cfg(target_os = "android")]
    let eligible = android_core_library(&library);
    #[cfg(not(target_os = "android"))]
    let eligible = fs::symlink_metadata(&library).is_ok_and(|metadata| metadata.is_file());
    if !eligible {
        return Err("SHINKI_EMU_CORE_UNSUPPORTED_TARGET");
    }
    Ok((library, expected_stem))
}

#[cfg(target_os = "android")]
pub(super) fn validate_pending_package(cores: &Path, stem: &str, directory: &Path) -> Result<(), &'static str> {
    let package = core_package::read_package(directory)?;
    package_library(directory, stem, &package.manifest)?;
    let current = cores.join(stem);
    if fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.is_dir())
        && let Some(current) = core_package::read_manifest(&current)
        && current.plugin_id != package.manifest.plugin_id
    {
        return Err("SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH");
    }
    Ok(())
}

/// Read package identities only. Bare libraries cannot establish an identity.
pub(super) fn installed(directories: &[PathBuf]) -> Result<BTreeMap<String, InstalledCore>, String> {
    // OHOS executable packages come only from the installed app bundle.
    let directories = if cfg!(target_env = "ohos") { &[] } else { directories };
    let mut found = BTreeMap::new();
    let mut duplicate = BTreeSet::new();
    let mut stems = BTreeSet::new();
    for (index, directory) in directories.iter().enumerate() {
        let mut entries = fs::read_dir(directory)
            .and_then(|entries| entries.map(|entry| entry.map(|entry| (entry.file_name(), entry))).collect::<std::io::Result<Vec<_>>>())
            .map_err(|error| format!("SHINKI_EMU_CORE_DIRECTORY: {error}"))?;
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for (stem, entry) in entries {
            let Some(stem) = stem.to_str().filter(|stem| !stem.starts_with('.')) else { continue };
            if !entry.file_type().map_err(|error| error.to_string())?.is_dir() {
                continue;
            }
            let directory = entry.path();
            let Ok(package) = core_package::read_package(&directory) else { continue };
            let Ok((library, expected_stem)) = package_library(&directory, stem, &package.manifest) else { continue };
            if !stems.insert(expected_stem) { continue; }
            let id = package.manifest.plugin_id.clone();
            let core = InstalledCore {
                library, manifest: package.manifest, source_commit: package.source_commit, bundled: index > 0,
            };
            if found.insert(id.clone(), core).is_some() { duplicate.insert(id); }
        }
    }
    for id in duplicate { found.remove(&id); }
    #[cfg(target_os = "ios")]
    for &id in shinki_emu_manager::platform::bundled_core_ids() {
        use shinki_emu_manager::platform;
        let invalid = || format!("SHINKI_EMU_IOS_FRAMEWORK_PACKAGE: {id}");
        let library = platform::bundled_core_library(id).ok_or_else(invalid)?;
        let directory = platform::bundled_core_manifest_dir(id).ok_or_else(invalid)?;
        let package = core_package::read_bundled_package(&directory, id).ok_or_else(invalid)?;
        match cores::embedded_identity(id) {
            Some(identity) if package.manifest.identity.as_ref() == Some(identity) => {}
            None if package.manifest.identity.is_none()
                && package.manifest.version == shinki_emu_manager::version()
                && package.source_commit.as_deref() == Some(env!("SHINKI_EMU_COMPILED_SOURCE_COMMIT")) => {}
            _ => return Err(invalid()),
        }
        found.insert(id.to_owned(), InstalledCore {
            library, manifest: package.manifest, source_commit: package.source_commit, bundled: true,
        });
    }
    #[cfg(target_env = "ohos")]
    for &(id, basename) in shinki_emu_manager::platform::ohos::bundled::configured_cores() {
        use shinki_emu_manager::platform::ohos::bundled;
        let package = bundled::manifest_dir(id)
            .and_then(|directory| core_package::read_package(&directory).ok())
            .filter(|package| package.manifest.plugin_id == id
                && package.manifest.library == basename
                && package.manifest.files.iter().any(|file| file.path == basename)
                && package.manifest.identity.is_none()
                && package.manifest.version == shinki_emu_manager::version()
                && package.source_commit.as_deref() == Some(env!("SHINKI_EMU_COMPILED_SOURCE_COMMIT")));
        let (Some(package), Some(library)) = (package, bundled::library(id)) else {
            tracing::warn!(event = "shinki.emu.ohos.bundle_core_unavailable", plugin_id = id);
            continue;
        };
        found.insert(id.to_owned(), InstalledCore {
            library, manifest: package.manifest, source_commit: package.source_commit, bundled: true,
        });
    }
    Ok(found)
}

impl ShinkiEmuManagerController {
    pub(super) fn plugin_available(&self, plugin_id: &str) -> bool {
        runnable_core(self.registry.installed(), plugin_id).is_ok()
    }

    pub(super) fn restore_recognition(&mut self) -> Result<(), String> {
        let rules = scan_rules(self.registry.installed());
        for game in self.library.list_games().map_err(|error| error.to_string())? {
            let key = format!("recognition:{}", game.game_id);
            let detections = self.library.app_state::<Vec<EngineDetection>>(&key).map_err(|error| error.to_string())?;
            if let Some(mut detections) = detections {
                let count = detections.len();
                detections.retain(|detection| detection.declared.as_ref().is_none_or(|declared| declared.rules_key == rules.key()));
                if detections.len() != count {
                    self.library.set_app_state(&key, &detections).map_err(|error| error.to_string())?;
                }
                self.set_probe_candidates(&game.game_id, choices(&detections))?;
            } else if let Some(family) = game.family_id {
                // Legacy records retain identity and binding. Fresh selected probe is
                // still mandatory before open; no format/runtime claim is invented.
                let choices = static_core_catalog::entries().iter()
                    .filter(|entry| {
                        #[cfg(any(target_os = "ios", target_env = "ohos"))]
                        let supported = is_supported(entry);
                        #[cfg(not(any(target_os = "ios", target_env = "ohos")))]
                        let supported = entry.supported || is_static(entry.plugin_id);
                        supported && entry.family_id == family
                    })
                    .map(|entry| Candidate { plugin_id: entry.plugin_id.into(), family_id: family.clone() }).collect();
                self.set_probe_candidates(&game.game_id, choices)?;
            }
        }
        Ok(())
    }

    pub(super) fn set_recognition(&mut self, game_id: &str, detections: Vec<EngineDetection>) -> Result<(), String> {
        self.library.set_app_state(&format!("recognition:{game_id}"), &detections).map_err(|error| error.to_string())?;
        if let [detection] = detections.as_slice()
            && self.library.game_settings(game_id).map_err(|error| error.to_string())?
                .and_then(|settings| settings.emulator_id).is_none()
        {
            self.library.set_game_family(game_id, Some(&detection.family_id)).map_err(|error| error.to_string())?;
        }
        self.set_probe_candidates(game_id, choices(&detections))
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn configuration_state(&self, plugin_id: Option<&str>) -> &'static str {
        let Some(id) = plugin_id else { return "selection-required"; };
        if self.native_action.as_ref().is_some_and(|action| action.plugin_id() == id) { return "loading"; }
        if !self.plugin_available(id) { return "missing-core"; }
        if self.native_failures.contains_key(id) { return "failed"; }
        if self.registry.descriptor(id).is_some() { return "ready"; }
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
        runnable_core(self.registry.installed(), &plugin)?;
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
