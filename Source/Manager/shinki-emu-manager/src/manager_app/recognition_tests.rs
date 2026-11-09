use super::*;

pub(super) fn hcb() -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[..4].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend(4u32.to_le_bytes());
    bytes.extend([0, 0, 0, 0, 8, 0, 2, b'T', 0]);
    bytes.extend(1u16.to_le_bytes());
    bytes.extend([0, 5, b'T', b'e', b's', b't', 0]);
    bytes.extend(0u16.to_le_bytes());
    bytes
}

pub(super) fn ps2a() -> Vec<u8> {
    let mut bytes = vec![0; 50];
    bytes[..4].copy_from_slice(b"PS2A");
    for (offset, value) in [(4, 48u32), (20, 2), (40, 2)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(super) fn write_game(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("boot.hcb"), hcb()).unwrap();
    fs::write(path.join("native-marker"), b"selected runtime fixture").unwrap();
}

pub(super) fn manifest(data: &Path, plugin: &str, stem: &str, bytes: &[u8]) -> PathBuf {
    let directory = data.join("cores").join(format!("shinki_emu_{stem}"));
    fs::create_dir_all(&directory).unwrap();
    let library = format!("shinki_emu_{stem}.{}", env::consts::DLL_EXTENSION);
    fs::write(directory.join(&library), bytes).unwrap();
    let manifest = core_package::CoreManifest {
        schema: core_package::CORE_PACKAGE_SCHEMA.into(), plugin_id: plugin.into(), library: library.clone(),
        version: "0.2.0".into(), identity: None,
        abi: shinki_emu_manager_core::INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files: vec![core_package::CoreFile { path: library.clone(), sha256: core_package::sha256_hex(bytes) }],
    };
    fs::write(directory.join("core.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
    directory.join(library)
}

pub(super) fn open(data: &Path) -> ShinkiEmuManagerController {
    ShinkiEmuManagerController::open_library(data.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

pub(super) fn settle(app: &mut ShinkiEmuManagerController) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.startup_busy() {
        app.poll_platform().unwrap();
        assert!(Instant::now() < deadline, "owner operation did not complete");
        std::thread::yield_now();
    }
}

pub(super) fn import(app: &mut ShinkiEmuManagerController, path: &Path) -> String {
    app.add_game_directory(path).unwrap();
    settle(app);
    app.library.list_games().unwrap().into_iter().find(|game| Path::new(&game.location) == fs::canonicalize(path).unwrap()).unwrap().game_id
}

#[test]
fn recognized_missing_core_survives_reopen_and_install_without_duplicate_games() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    write_game(&game);
    let data = root.path().join("manager");
    let mut app = open(&data);
    let id = import(&mut app, &game);
    let saved = app.game(&id).unwrap();
    assert_eq!(saved.family_id.as_deref(), Some("fvp"));
    assert_eq!(app.model().unwrap().selection.game.emulator_id, "rfvp");
    assert!(!app.model().unwrap().selection.game.launchable);
    assert!(app.model().unwrap().selection.game.problem.contains("核心"));
    assert_eq!(app.launch(&id).unwrap_err(), "SHINKI_EMU_CORE_MISSING");
    drop(app);
    // Installing a package changes availability, not durable game/path identity.
    manifest(&data, "rfvp", "fixture", b"not executed by browsing");
    let mut app = open(&data);
    assert_eq!(app.game(&id).unwrap(), saved);
    import(&mut app, &game);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(app.model().unwrap().selection.game.emulator_id, "rfvp");
    assert!(app.model().unwrap().selection.game.launchable);
    assert!(app.registry.descriptor("rfvp").is_none());
}

#[test]
fn multiple_plugins_and_families_need_choice_and_never_replace_saved_binding() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("resource.bin"), ps2a()).unwrap();
    let data = root.path().join("manager");
    let mut app = open(&data);
    let id = import(&mut app, &path);
    assert_eq!(app.probe_choices[&id].len(), 2);
    assert!(!app.candidates.contains_key(&id));
    assert_eq!(app.game(&id).unwrap().family_id.as_deref(), Some("cmvs"));
    app.set_game_emulator(&id, "cmvs-rs").unwrap();
    fs::write(path.join("extra.hcb"), hcb()).unwrap();
    import(&mut app, &path);
    assert_eq!(app.candidates[&id].plugin_id, "cmvs-rs");
    assert_eq!(app.probe_choices[&id].len(), 3);
    drop(app);
    let mut app = open(&data);
    assert_eq!(app.candidates[&id].plugin_id, "cmvs-rs");
    import(&mut app, &path);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(app.candidates[&id].plugin_id, "cmvs-rs");
    // A new ambiguous directory has no guessed family/plugin binding.
    let second = root.path().join("second");
    fs::create_dir(&second).unwrap();
    fs::write(second.join("resource.bin"), ps2a()).unwrap();
    fs::write(second.join("extra.hcb"), hcb()).unwrap();
    let second_id = import(&mut app, &second);
    assert!(app.game(&second_id).unwrap().family_id.is_none());
    assert!(!app.candidates.contains_key(&second_id));
}

#[test]
fn missing_saved_plugin_never_rebinds_to_a_new_catalog_choice() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    write_game(&game);
    let mut app = open(&root.path().join("manager"));
    let id = import(&mut app, &game);
    app.library.select_game_emulator(&id, "fvp", "removed.user.plugin").unwrap();
    import(&mut app, &game);
    assert!(!app.candidates.contains_key(&id));
    let model = app.model().unwrap();
    assert_eq!(model.selection.game.emulator_id, "removed.user.plugin");
    assert!(!model.selection.game.launchable);
    assert_eq!(app.launch(&id).unwrap_err(), "SHINKI_EMU_CORE_MISSING");
    app.set_game_emulator(&id, "rfvp").unwrap();
    assert_eq!(app.library.game_settings(&id).unwrap().unwrap().emulator_id.as_deref(), Some("rfvp"));
}

#[test]
fn patch_receipt_configuration_is_deferred_until_explicit_native_validation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("game");
    write_game(&path);
    let mut app = open(&root.path().join("manager"));
    let id = import(&mut app, &path);
    let profile = shinki_emu_patch::OutputProfile {
        plugin_id: "rfvp".into(), configuration: BTreeMap::from([("launch_mode".into(), "title".into())]),
        required_capabilities: shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY,
    };
    let choices = app.probe_choices[&id].clone();
    let selected = app.select_import_profile(&choices, &profile).unwrap();
    app.apply_import_profile(&id, choices, selected, &profile).unwrap();
    assert!(app.registry.descriptors().next().is_none());
    assert_eq!(app.library.app_state::<BTreeMap<String, String>>(&format!("pending-patch-configuration:{id}")).unwrap(), Some(profile.configuration));
    assert_eq!(app.library.game_settings(&id).unwrap().unwrap().emulator_id.as_deref(), Some("rfvp"));
    assert!(!app.model().unwrap().selection.game.launchable);
}

#[cfg(windows)]
mod native_consumer {
    use super::*;
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(path: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }
    struct Counters { module: *mut c_void, count: unsafe extern "C" fn(u32) -> u64 }
    impl Counters {
        fn open(path: &Path) -> Self {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                let module = LoadLibraryW(wide.as_ptr());
                assert!(!module.is_null(), "fixture library unavailable: {}", path.display());
                let symbol = GetProcAddress(module, c"recognition_fixture_count".as_ptr().cast());
                assert!(!symbol.is_null());
                Self { module, count: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(u32) -> u64>(symbol) }
            }
        }
        fn values(&self) -> [u64; 6] { std::array::from_fn(|index| unsafe { (self.count)(index as u32) }) }
    }
    impl Drop for Counters { fn drop(&mut self) { unsafe { FreeLibrary(self.module); } } }
    fn install(data: &Path, variable: &str, plugin: &str, stem: &str) -> PathBuf {
        let source = env::var_os(variable).expect("run the documented native-fixture gate first");
        manifest(data, plugin, stem, &fs::read(source).unwrap())
    }
    #[test]
    fn scan_library_and_core_list_execute_no_available_or_failing_native_entry() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("manager");
        let selected = install(&data, "SHINKI_TEST_NATIVE_FIXTURE", "rfvp", "selected");
        let other = install(&data, "SHINKI_TEST_NATIVE_ALTERNATIVE", "siglus_rs", "other");
        let failure = install(&data, "SHINKI_TEST_NATIVE_FAILURE", "ethornell-rs", "failure");
        let game = root.path().join("game");
        write_game(&game);
        let mut app = open(&data);
        let initial = app.model().unwrap();
        assert!(initial.cores.items.iter().all(|core| core.capabilities.is_empty()));
        let id = import(&mut app, &game);
        app.select_case(&id).unwrap();
        app.search("").unwrap();
        app.model().unwrap();
        assert_eq!(app.registry.descriptors().count(), 0);
        let selected_calls = Counters::open(&selected);
        let other_calls = Counters::open(&other);
        let failure_calls = Counters::open(&failure);
        assert_eq!(selected_calls.values(), [0; 6]);
        assert_eq!(other_calls.values(), [0; 6]);
        assert_eq!(failure_calls.values(), [0; 6]);
        println!("consumer scan/library/core-list native calls: selected={:?} other={:?} failing={:?}", selected_calls.values(), other_calls.values(), failure_calls.values());
        let pending = app.prepare_game_configuration(&id).unwrap();
        assert_eq!(pending.game_settings.configuration_state, "loading");
        settle(&mut app);
        assert_eq!(app.model().unwrap().game_settings.configuration_state, "ready");
        assert!(!app.model().unwrap().game_settings.fields.is_empty());
        let called = selected_calls.values();
        assert_eq!(&called[..4], &[1, 1, 1, 0]);
        assert_eq!(other_calls.values(), [0; 6]);
        assert_eq!(failure_calls.values(), [0; 6]);
        app.diagnose_core("ethornell-rs").unwrap();
        settle(&mut app);
        assert_eq!(app.configuration_state(Some("ethornell-rs")), "failed");
        assert_eq!(selected_calls.values(), called);
        assert_eq!(other_calls.values(), [0; 6]);
        let failed = failure_calls.values();
        assert_eq!(&failed[..4], &[1, 1, 0, 0]);
        app.model().unwrap();
        assert_eq!(failure_calls.values(), failed);
        // Starting validates/opens only the selected implementation.
        app.set_host_wake(std::sync::Arc::new(|| {}));
        app.window_state = Some(WindowState { width: 1, height: 1, scale_factor: 1.0, focused: true, visible: true, suspended: false });
        app.launch(&id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.active.as_ref().is_some_and(|active| active.opened) {
            app.poll_platform().unwrap();
            assert!(Instant::now() < deadline, "selected native open did not complete: {}", app.diagnostic);
            std::thread::yield_now();
        }
        assert_eq!(selected_calls.values()[4], 1);
        assert!(selected_calls.values()[3] >= 2);
        assert_eq!(other_calls.values(), [0; 6]);
        assert_eq!(failure_calls.values(), failed);
        app.leave_game().unwrap();
    }

    #[test]
    #[ignore = "read-only local game directories and compiled native fixtures supplied by the consumer smoke gate"]
    fn real_directory_consumer_smoke() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("manager");
        let selected = install(&data, "SHINKI_TEST_NATIVE_FIXTURE", "rfvp", "selected");
        let other = install(&data, "SHINKI_TEST_NATIVE_ALTERNATIVE", "siglus_rs", "other");
        let failure = install(&data, "SHINKI_TEST_NATIVE_FAILURE", "ethornell-rs", "failure");
        let paths: Vec<PathBuf> = serde_json::from_str(&env::var("SHINKI_RECOGNITION_SMOKE_PATHS").expect("explicit smoke paths")).unwrap();
        assert!(!paths.is_empty());
        let mut app = open(&data);
        for path in &paths {
            let id = import(&mut app, path);
            app.select_case(&id).unwrap();
            let detections = app.library.app_state::<Vec<shinki_emu_manager_core::detection::EngineDetection>>(&format!("recognition:{id}")).unwrap().unwrap();
            assert!(!detections.is_empty());
            println!("consumer {} => {:?}", path.display(), detections);
        }
        let model = app.model().unwrap();
        assert_eq!(model.library.games.len(), paths.len());
        assert!(model.cores.items.iter().all(|item| item.capabilities.is_empty()));
        assert_eq!(app.registry.descriptors().count(), 0);
        for library in [selected, other, failure] {
            let counters = Counters::open(&library);
            assert_eq!(counters.values(), [0; 6]);
            println!("consumer scan/library/core-list {} => native calls {:?}", library.display(), counters.values());
        }
    }

    #[test]
    fn scoped_configuration_error_follows_game_plugin_and_survives_unrelated_notices() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("manager");
        let selected = install(&data, "SHINKI_TEST_NATIVE_FIXTURE", "rfvp", "selected");
        let failure = install(&data, "SHINKI_TEST_NATIVE_FAILURE", "ethornell-rs", "failure");
        let first = root.path().join("first");
        let second = root.path().join("second");
        write_game(&first);
        write_game(&second);
        fs::write(first.join("other-family.bin"), ps2a()).unwrap();
        let receipt = first.join("shinki-patch-result.json");
        fs::write(&receipt, serde_json::to_vec(&serde_json::json!({
            "schema": "shinki.emu.file_patch.v1",
            "profile": { "plugin_id": "rfvp", "configuration": { "unsupported_field": "value" } }
        })).unwrap()).unwrap();
        let mut app = open(&data);
        let first_id = import(&mut app, &first);
        let second_id = import(&mut app, &second);
        app.select_case(&first_id).unwrap();
        let selected_calls = Counters::open(&selected);
        let failure_calls = Counters::open(&failure);
        assert_eq!(selected_calls.values(), [0; 6]);
        assert_eq!(failure_calls.values(), [0; 6]);
        let loading = app.prepare_game_configuration(&first_id).unwrap();
        assert_eq!(loading.game_settings.configuration_state, "loading");
        assert!(loading.game_settings.configuration_error.is_empty());
        assert!(app.select_case(&second_id).unwrap().game_settings.configuration_error.is_empty());
        settle(&mut app);
        assert!(app.model().unwrap().game_settings.configuration_error.is_empty(), "late failure belongs to the requested game, not the currently viewed game");
        let error = app.select_case(&first_id).unwrap().game_settings.configuration_error;
        assert!(error.contains("unknown field"));
        assert!(app.game_configuration_error(&first_id, Some("cmvs-rs")).is_empty(), "the cached error must also match the requested plugin");
        let calls = selected_calls.values();
        assert_eq!(&calls[..4], &[1, 1, 1, 0]);
        app.plugin_errors.insert("unrelated".into(), "SHINKI_EMU_CORE_PACKAGE_MANIFEST: unrelated notice".into());
        let unrelated = app.model().unwrap();
        assert_eq!(unrelated.notices.items[0].core, "unrelated");
        assert_eq!(unrelated.game_settings.configuration_error, error);
        app.diagnostic.clear();
        assert_eq!(app.model().unwrap().game_settings.configuration_error, error);
        assert!(app.select_case(&second_id).unwrap().game_settings.configuration_error.is_empty());
        assert_eq!(app.select_case(&first_id).unwrap().game_settings.configuration_error, error);
        assert_eq!(app.set_game_emulator(&first_id, "rfvp").unwrap().game_settings.configuration_error, error);
        let changed = app.set_game_emulator(&first_id, "cmvs-rs").unwrap();
        assert!(changed.game_settings.configuration_error.is_empty());
        assert_eq!(changed.game_settings.configuration_state, "missing-core");
        app.set_game_emulator(&first_id, "rfvp").unwrap();
        assert!(app.model().unwrap().game_settings.configuration_error.is_empty());
        assert_eq!(selected_calls.values(), calls);
        assert_eq!(failure_calls.values(), [0; 6]);
        app.prepare_game_configuration(&first_id).unwrap();
        settle(&mut app);
        assert_eq!(app.model().unwrap().game_settings.configuration_error, error);
        fs::write(&receipt, serde_json::to_vec(&serde_json::json!({
            "schema": "shinki.emu.file_patch.v1",
            "profile": { "plugin_id": "rfvp", "configuration": { "launch_mode": "title" } }
        })).unwrap()).unwrap();
        import(&mut app, &first);
        let retry = app.prepare_game_configuration(&first_id).unwrap();
        assert_eq!(retry.game_settings.configuration_state, "loading");
        assert!(retry.game_settings.configuration_error.is_empty());
        settle(&mut app);
        let success = app.model().unwrap();
        assert_eq!(success.game_settings.configuration_state, "ready");
        assert!(success.game_settings.configuration_error.is_empty());
        assert_eq!(selected_calls.values(), calls);
        assert_eq!(failure_calls.values(), [0; 6]);
        println!("scoped game error: unrelated/dismiss/game-switch/same-plugin retained; plugin-switch/retry/success cleared; native={calls:?}, unrelated={:?}", failure_calls.values());
    }

    #[test]
    fn scoped_configuration_error_for_defaults_uses_only_current_plugin() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("manager");
        let selected = install(&data, "SHINKI_TEST_NATIVE_FIXTURE", "rfvp", "selected");
        let failure = install(&data, "SHINKI_TEST_NATIVE_FAILURE", "ethornell-rs", "failure");
        let mut app = open(&data);
        app.prepare_library().unwrap();
        settle(&mut app);
        let selected_calls = Counters::open(&selected);
        let failure_calls = Counters::open(&failure);
        let loading = app.select_emulator_defaults("ethornell-rs").unwrap();
        assert_eq!(loading.cores.default_configuration_state, "loading");
        assert!(loading.cores.default_configuration_error.is_empty());
        settle(&mut app);
        let failed = app.model().unwrap();
        let error = failed.cores.default_configuration_error;
        assert_eq!(failed.cores.default_plugin, "ethornell-rs");
        assert_eq!(failed.cores.default_configuration_state, "failed");
        assert!(!error.is_empty());
        let calls = failure_calls.values();
        assert_eq!(&calls[..4], &[1, 1, 0, 0]);
        app.plugin_errors.insert("unrelated".into(), "SHINKI_EMU_CORE_PACKAGE_MANIFEST: unrelated notice".into());
        app.diagnostic.clear();
        assert_eq!(app.model().unwrap().cores.default_configuration_error, error);
        assert_eq!(selected_calls.values(), [0; 6]);
        assert_eq!(failure_calls.values(), calls);
        let cleared = app.select_emulator_defaults("").unwrap();
        assert!(cleared.cores.default_plugin.is_empty());
        assert!(cleared.cores.default_configuration_error.is_empty());
        app.select_emulator_defaults("rfvp").unwrap();
        app.select_emulator_defaults("").unwrap();
        settle(&mut app);
        let cancelled_selection = app.model().unwrap();
        assert!(cancelled_selection.cores.default_plugin.is_empty(), "late success must not restore a cleared selection");
        assert!(cancelled_selection.cores.default_configuration_error.is_empty());
        let selected_once = selected_calls.values();
        assert_eq!(&selected_once[..4], &[1, 1, 1, 0]);
        app.select_emulator_defaults("rfvp").unwrap();
        settle(&mut app);
        let success = app.model().unwrap();
        assert_eq!(success.cores.default_plugin, "rfvp");
        assert_eq!(success.cores.default_configuration_state, "ready");
        assert!(success.cores.default_configuration_error.is_empty());
        assert_eq!(selected_calls.values(), selected_once);
        let retry = app.select_emulator_defaults("ethornell-rs").unwrap();
        assert_eq!(retry.cores.default_configuration_state, "loading");
        assert!(retry.cores.default_configuration_error.is_empty());
        settle(&mut app);
        assert!(!app.model().unwrap().cores.default_configuration_error.is_empty());
        assert_eq!(&failure_calls.values()[..4], &[2, 2, 0, 0]);
        assert_eq!(selected_calls.values(), selected_once);
        println!("scoped defaults error: notice-dismiss preserved; clear/pending-selection/success/retry associated correctly; selected={selected_once:?}, explicit failing retries={:?}", failure_calls.values());
    }
}

#[cfg(all(windows, feature = "static-cmvs"))]
#[test]
fn optional_static_cmvs_is_available_without_a_dll_and_only_activated_explicitly() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    fs::create_dir(&game).unwrap();
    fs::write(game.join("resource.bin"), ps2a()).unwrap();
    let mut app = open(&root.path().join("manager"));
    let id = import(&mut app, &game);
    app.set_game_emulator(&id, "shinki.emu.cmvs").unwrap();
    assert!(app.registry.installed().is_empty());
    assert_eq!(app.registry.descriptors().count(), 0);
    assert!(app.model().unwrap().selection.game.launchable);
    assert_eq!(app.model().unwrap().cores.items.iter().find(|item| item.id == "shinki.emu.cmvs").unwrap().state, "installed");
    app.diagnose_core("shinki.emu.cmvs").unwrap();
    settle(&mut app);
    assert_eq!(app.registry.descriptors().count(), 1);
    assert!(app.registry.descriptor("shinki.emu.cmvs").is_some());
}

#[cfg(windows)]
#[test]
fn transient_scan_io_failure_can_be_retried_in_the_same_owner_without_mutating_the_library() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut app = open(&root.path().join("manager"));
    let id = import(&mut app, &path);
    let saved = app.game(&id).unwrap();
    let locked = fs::OpenOptions::new().read(true).share_mode(0).open(path.join("boot.hcb")).unwrap();
    app.add_game_directory(&path).unwrap();
    settle(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Failed(_)));
    assert_eq!(app.game(&id).unwrap(), saved);
    assert!(app.registry.is_ready());
    drop(locked);
    app.add_game_directory(&path).unwrap();
    settle(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Ready));
    assert_eq!(app.game(&id).unwrap(), saved);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(app.registry.descriptors().count(), 0);
}
