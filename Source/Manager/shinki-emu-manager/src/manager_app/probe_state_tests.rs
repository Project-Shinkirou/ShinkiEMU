use super::*;
use crate::test_native::{self, Fixture};
use super::recognition_tests::{hcb, write_game};

fn provider(plugin_id: &'static str, confidence: u16) -> shinki_emu_manager_core::LoadedFamilyPlugin {
    provider_with_marker(plugin_id, confidence, "native-marker")
}
fn provider_with_marker(plugin_id: &'static str, confidence: u16, marker: &'static str) -> shinki_emu_manager_core::LoadedFamilyPlugin {
    let mut fixture = Fixture::new(plugin_id, "fvp", "fixture.native");
    fixture.confidence = u32::from(confidence);
    fixture.marker = Some(marker);
    test_native::install(if plugin_id == "test-a" { 0 } else { 1 }, fixture)
}
fn controller(root: &Path) -> ShinkiEmuManagerController {
    let mut app = ShinkiEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap();
    app.prepare_library().unwrap();
    settle_background(&mut app);
    app
}
fn game(root: &Path) -> PathBuf {
    let path = root.join("title");
    write_game(&path);
    fs::canonicalize(path).unwrap()
}
pub(super) fn settle_background(app: &mut ShinkiEmuManagerController) { super::recognition_tests::settle(app); }
fn settle_provider(app: &mut ShinkiEmuManagerController) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.cores_ready() {
        app.poll_platform().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
fn scan_paths(app: &mut ShinkiEmuManagerController, paths: &[PathBuf]) -> Vec<String> {
    settle_provider(app);
    app.begin_scan(probe_worker::ScanKind::Import, paths.to_vec()).unwrap();
    settle_background(app);
    app.library.list_games().unwrap().into_iter().map(|game| game.game_id).collect()
}
fn scan_once(app: &mut ShinkiEmuManagerController, path: &Path) { scan_paths(app, &[path.into()]); }
fn import(app: &mut ShinkiEmuManagerController, path: &Path) -> ManagerViewModel {
    app.add_game_directory(path).unwrap();
    settle_background(app);
    app.model().unwrap()
}
fn record(id: &str, location: &Path) -> GameRecord {
    GameRecord { game_id: id.into(), title: "Saved title".into(), user_title: Some("My game".into()),
        location: location.to_str().unwrap().into(), family_id: Some("fvp".into()),
        content_fingerprint: Some("saved-fingerprint".into()), added_at_unix_ms: 7 }
}

#[test]
fn preparing_preserves_legacy_identity_settings_and_metadata_without_native_loading() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("manager");
    let path = game(&root.path().join("library"));
    let saved = record("legacy", &path);
    {
        let mut app = ShinkiEmuManagerController::open_library(data.clone(), FrameMailbox::new(), Vec::new()).unwrap();
        app.library.add_game(&saved).unwrap();
        app.library.select_game_emulator("legacy", "fvp", "rfvp").unwrap();
    }
    let mut app = ShinkiEmuManagerController::open_library(data, FrameMailbox::new(), Vec::new()).unwrap();
    assert_eq!(app.game("legacy").unwrap(), saved);
    assert_eq!(app.model().unwrap().selection.game.emulator_id, "rfvp");
    assert_eq!(app.registry.descriptors().count(), 0);
    app.prepare_library().unwrap();
    settle_background(&mut app);
    assert_eq!(app.game("legacy").unwrap(), saved);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(app.registry.descriptors().count(), 0);
}

#[test]
fn unsupported_import_preserves_existing_game_and_selection() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(&root.path().join("manager"));
    import(&mut app, &path);
    let unsupported = root.path().join("unsupported");
    fs::create_dir(&unsupported).unwrap();
    let model = import(&mut app, &unsupported);
    assert_eq!(model.library.games.len(), 1);
    assert_eq!(model.library.selected_id, path_id(&path));
}

#[test]
fn scanning_stops_inside_a_recognized_game() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let assets = path.join("data/pack");
    write_game(&assets);
    let mut app = controller(&root.path().join("manager"));
    assert_eq!(scan_paths(&mut app, &[root.path().into(), assets]), [path_id(&path)]);
}

#[test]
fn importing_parent_keeps_existing_path_identity_and_settings() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let alias = PathBuf::from(format!("{}{}", path.display(), std::path::MAIN_SEPARATOR));
    let saved = record("legacy-path-id", &alias);
    let mut app = controller(&root.path().join("manager"));
    app.library.add_game(&saved).unwrap();
    app.library.set_game_settings(&saved.game_id, &shinki_emu_manager_core::GameSettings {
        emulator_id: Some("rfvp".into()), input_mapping: Some(default_vn_preset()), ..Default::default()
    }).unwrap();
    assert_eq!(scan_paths(&mut app, &[root.path().into()]), ["legacy-path-id"]);
    assert_eq!(app.game("legacy-path-id").unwrap(), saved);
    assert!(app.library.game_settings("legacy-path-id").unwrap().unwrap().input_mapping.is_some());
}

#[test]
fn scanning_existing_aliases_keeps_both_records_without_location_conflict() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let alias = root.path().join(".").join("title");
    let mut app = controller(&root.path().join("manager"));
    for (id, path) in [("original", &alias), ("canonical", &path)] { app.library.add_game(&record(id, path)).unwrap(); }
    app.prepare_library().unwrap();
    settle_background(&mut app);
    assert_eq!(app.library.list_games().unwrap().len(), 2);
    assert_eq!(app.game("original").unwrap().location, alias.to_str().unwrap());
    assert_eq!(app.game("canonical").unwrap().location, path.to_str().unwrap());
    assert!(app.candidates.contains_key("original"));
    assert!(app.candidates.contains_key("canonical"));
    fs::remove_file(path.join("boot.hcb")).unwrap();
    app.prepare_library().unwrap();
    settle_background(&mut app);
    assert!(app.candidates.is_empty());
    assert_eq!(app.library.list_games().unwrap().len(), 2);
}

#[test]
fn unavailable_paths_and_no_match_preserve_records_but_disable_launch() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let data = root.path().join("manager");
    let mut app = controller(&data);
    let id = super::recognition_tests::import(&mut app, &path);
    app.library.set_game_user_title(&id, Some("Saved title")).unwrap();
    let saved = app.game(&id).unwrap();
    fs::remove_file(path.join("boot.hcb")).unwrap();
    scan_once(&mut app, &path);
    assert!(app.candidates.is_empty());
    assert_eq!(app.game(&id).unwrap(), saved);
    assert!(!app.model().unwrap().selection.game.launchable);
    fs::write(path.join("boot.hcb"), hcb()).unwrap();
    scan_once(&mut app, &path);
    assert_eq!(app.candidates[&id].plugin_id, "rfvp");
    fs::remove_dir_all(&path).unwrap();
    app.prepare_library().unwrap();
    settle_background(&mut app);
    assert_eq!(app.game(&id).unwrap(), saved);
    assert!(app.candidates.is_empty());
    drop(app);
    let app = ShinkiEmuManagerController::open_library(data, FrameMailbox::new(), Vec::new()).unwrap();
    assert!(app.candidates.is_empty(), "no-match must persist across reopen");
}

#[test]
fn import_is_queued_and_published_only_after_owner_completes_it() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(&root.path().join("manager"));
    let (release, gate) = std::sync::mpsc::channel();
    let (entered, receiver) = std::sync::mpsc::channel();
    app.registry.register_provider(move || {
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(10)).unwrap();
        provider("test-b", 8000)
    }).unwrap();
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(app.add_game_directory(&path).unwrap().library.games.is_empty());
    assert!(app.startup_busy());
    release.send(()).unwrap();
    settle_background(&mut app);
    assert_eq!(app.library.list_games().unwrap().len(), 1);
}

#[test]
fn inventory_failure_requires_restart_and_shutdown_still_joins_owner() {
    let root = tempfile::tempdir().unwrap();
    let mut app = ShinkiEmuManagerController::open_library(root.path().into(), FrameMailbox::new(), Vec::new()).unwrap();
    fs::remove_dir(root.path().join("cores")).unwrap();
    fs::write(root.path().join("cores"), b"not a directory").unwrap();
    assert!(app.prepare_library().is_err(), "invalid inventory also fails the immediate model read");
    assert!(app.cancel_prepare().is_err());
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.startup_busy() {
        let _ = app.poll_platform(); // The deliberately invalid directory also prevents model rendering.
        assert!(Instant::now() < deadline, "failed inventory owner did not settle");
        std::thread::yield_now();
    }
    assert!(!app.cores_ready());
    assert!(app.prepare_library().is_err());
    assert!(app.add_game_directory(root.path()).is_err());
    app.registry.shutdown().unwrap();
    app.registry.shutdown().unwrap();
}

#[test]
fn failed_idle_owner_wakes_the_host_and_preserves_its_join_error() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let (sender, receiver) = crossbeam_channel::bounded(1);
    app.set_host_wake(std::sync::Arc::new(move || {
        let _ = sender.try_send(());
    }));
    app.registry
        .register_provider(|| panic!("intentional owner failure"))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !matches!(app.startup_state, background::StartupState::Failed(_)) {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        app.poll_platform().unwrap();
    }
    assert!(app.startup_status().contains("SHINKI_EMU_OWNER_PANICKED"));
    assert!(
        app.registry
            .shutdown()
            .unwrap_err()
            .contains("SHINKI_EMU_OWNER_PANICKED")
    );
}

fn root_with_games(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let library_root = root.join("library");
    for name in ["g1", "g2"] {
        let game = library_root.join(name);
        fs::create_dir_all(&game).unwrap();
        write_game(&game);
    }
    let library_root = fs::canonicalize(library_root).unwrap();
    (
        library_root.clone(),
        library_root.join("g1"),
        library_root.join("g2"),
    )
}

fn visible_games(app: &ShinkiEmuManagerController) -> Vec<String> {
    app.library
        .library_snapshot()
        .unwrap()
        .games
        .into_iter()
        .map(|game| game.game_id)
        .collect()
}

fn prepared(app: &mut ShinkiEmuManagerController) -> ManagerViewModel {
    app.prepare_library().unwrap();
    settle_background(app);
    app.model().unwrap()
}

#[test]
fn removed_games_stay_hidden_across_scans_until_undone() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, g2) = root_with_games(root.path());
    let (g1, g2) = (path_id(&g1), path_id(&g2));
    let mut app = controller(&root.path().join("manager"));
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    settle_provider(&mut app);

    let first = prepared(&mut app);
    assert_eq!(first.notices.snackbar.text, "发现 2 部新作品");
    let mut ids = visible_games(&app);
    ids.sort();
    let mut expected = vec![g1.clone(), g2.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    let session = app.library.start_play_session(&g1, 1_000).unwrap();
    app.library
        .end_play_session(&session, 61_000, PlaySessionEndReason::Leave)
        .unwrap();
    let stats = app.library.library_snapshot().unwrap().play_stats[&g1].clone();

    app.selected_case_id = Some(g1.clone());
    let removed = app.remove_from_library(&g1).unwrap();
    assert_eq!(visible_games(&app), vec![g2.clone()]);
    assert_eq!(removed.library.selected_id, "");
    assert_eq!(removed.notices.snackbar.action, "撤销");
    assert!(removed.notices.snackbar.text.contains("已从资料库移除"));
    assert_eq!(removed.settings.excluded.len(), 1);
    assert_eq!(removed.settings.excluded[0].id, g1);

    let rescanned = prepared(&mut app);
    assert_eq!(visible_games(&app), vec![g2.clone()]);
    assert_eq!(rescanned.notices.snackbar.text, "资料库已是最新");

    app.undo_remove().unwrap();
    let mut ids = visible_games(&app);
    ids.sort();
    assert_eq!(ids, expected);
    assert_eq!(
        app.library.library_snapshot().unwrap().play_stats[&g1],
        stats
    );
    assert!(app.model().unwrap().settings.excluded.is_empty());
    assert_eq!(
        app.undo_remove().unwrap_err(),
        "SHINKI_EMU_UNDO_UNAVAILABLE"
    );

    app.remove_from_library(&g2).unwrap();
    match app.snackbar_action().unwrap() {
        shinki_emu_manager::SnackbarOutcome::Model(model) => {
            assert!(model.library.games.iter().any(|game| game.id == g2));
        }
        shinki_emu_manager::SnackbarOutcome::Restart => panic!("undo must not restart"),
    }
    app.remove_from_library(&g2).unwrap();
    app.restore_game(&g2).unwrap();
    assert!(visible_games(&app).contains(&g2));
}

#[test]
fn added_directory_is_persisted_as_a_library_root() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, _) = root_with_games(root.path());
    let mut app = controller(&root.path().join("manager"));
    settle_provider(&mut app);
    app.add_game_directory(&library_root.join("g1")).unwrap();
    settle_background(&mut app);
    let model = app.model().unwrap();
    assert_eq!(model.settings.roots, vec![g1.to_str().unwrap().to_owned()]);
    assert_eq!(model.library.selected_id, path_id(&g1));
    assert_eq!(app.scan_roots().unwrap(), vec![g1]);
}

#[test]
fn prepare_reads_inventory_then_scans_every_root_without_native_loading() {
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, g2) = root_with_games(root.path());
    let mut app = ShinkiEmuManagerController::open_library(
        root.path().join("manager"),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    let (sender, wakes) = crossbeam_channel::unbounded();
    app.set_host_wake(std::sync::Arc::new(move || {
        let _ = sender.send(());
    }));
    while wakes.try_recv().is_ok() {}
    let pending = app.prepare_library().unwrap();
    assert_eq!(pending.library.prepare_state, "loading-cores");
    assert!(matches!(
        app.startup_state,
        background::StartupState::Loading
    ));
    for _ in 0..1 {
        wakes.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    app.poll_platform().unwrap();
    assert!(matches!(
        app.startup_state,
        background::StartupState::Scanning(probe_worker::ScanKind::Full)
    ));
    let scanning = app.model().unwrap();
    assert_eq!(scanning.library.prepare_state, "scanning");
    assert!(scanning.library.can_cancel_prepare);
    settle_background(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Ready));
    let ready = app.model().unwrap();
    assert_eq!(ready.library.prepare_state, "ready");
    assert_eq!(ready.library.prepare_label, "刷新资料库");
    assert_eq!(ready.notices.snackbar.text, "发现 2 部新作品");
    let mut ids = visible_games(&app);
    ids.sort();
    let mut expected = vec![path_id(&g1), path_id(&g2)];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn cancelling_a_scan_restores_ready_without_touching_the_library() {
    struct Release(Option<std::sync::mpsc::Sender<()>>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (library_root, g1, _) = root_with_games(root.path());
    let mut app = controller(&root.path().join("manager"));
    scan_once(&mut app, &g1);
    let before = visible_games(&app);
    app.library
        .add_library_root(library_root.to_str().unwrap())
        .unwrap();
    let (sender, gate) = std::sync::mpsc::channel();
    let mut release = Release(Some(sender));
    let (entered, receiver) = std::sync::mpsc::channel();
    app.registry
        .register_provider(move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            provider_with_marker("test-b", 8_000, "absent-marker")
        })
        .unwrap();
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    app.prepare_library().unwrap();
    assert!(matches!(
        app.startup_state,
        background::StartupState::Scanning(probe_worker::ScanKind::Full)
    ));
    app.cancel_prepare().unwrap();
    release.0.take().unwrap().send(()).unwrap();
    settle_background(&mut app);
    assert!(matches!(app.startup_state, background::StartupState::Ready));
    assert_eq!(visible_games(&app), before);
    let model = app.model().unwrap();
    assert_eq!(model.notices.snackbar.text, "已取消扫描");
    assert_eq!(model.notices.snackbar.action, "");
    assert!(app.diagnostic.is_empty());
    assert_eq!(
        app.cancel_prepare().unwrap_err(),
        "SHINKI_EMU_PREPARE_NOT_CANCELLABLE"
    );
}

fn fake_session(app: &mut ShinkiEmuManagerController, game_id: &str) {
    let epoch = shinki_emu_manager_core::model::allocate_session_epoch().unwrap();
    app.active = Some(owner::ActiveSessionView::opening(epoch, game_id.into()));
}

#[test]
fn closing_the_window_leaves_a_running_game_and_keeps_the_manager_open() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    assert!(matches!(
        shinki_emu_manager::close_requested(&mut app),
        shinki_emu_manager::CloseDecision::HideWindow
    ));
    fake_session(&mut app, "game");
    assert!(ManagerController::is_game_active(&app));
    match shinki_emu_manager::close_requested(&mut app) {
        shinki_emu_manager::CloseDecision::KeepWindow(result) => {
            result.unwrap();
        }
        shinki_emu_manager::CloseDecision::HideWindow => panic!("running game must keep window"),
    }
    assert!(!ManagerController::is_game_active(&app));
    assert_eq!(
        app.active.as_ref().unwrap().closing,
        Some(PlaySessionEndReason::Leave)
    );
}

#[test]
fn a_full_input_queue_drops_input_without_ending_the_session() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    fake_session(&mut app, "game");
    let mut pressed = true;
    let error = loop {
        match app.game_input("a", pressed, 1.0) {
            Ok(()) => pressed = !pressed,
            Err(error) => break error,
        }
    };
    assert_eq!(error, "SHINKI_EMU_INPUT_QUEUE_FULL");
    assert_eq!(
        app.pending_events.len(),
        shinki_emu_manager_core::model::MAX_INPUT_EVENTS
    );
    assert!(ManagerController::is_game_active(&app));
    assert!(app.active.as_ref().unwrap().closing.is_none());
}

#[test]
fn key_capture_binds_named_keys_and_waits_on_unmappable_ones() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let mut app = controller(root.path());
    scan_once(&mut app, &path);
    let id = path_id(&path);

    app.begin_key_capture("south", None).unwrap();
    assert_eq!(app.key_capture().unwrap().button, "south");
    app.capture_key(KeyCode::Digit1).unwrap();
    assert_eq!(app.key_capture().unwrap().hint, "这个键不能用于映射");
    app.capture_key(KeyCode::Escape).unwrap();
    assert!(app.key_capture().is_none());
    assert_eq!(app.input_mapping.gamepad[&GamepadInput::South], "escape");

    app.begin_key_capture("west", Some(&id)).unwrap();
    app.capture_key(KeyCode::F5).unwrap();
    assert!(app.key_capture().is_none());
    let game_mapping = app
        .library
        .game_settings(&id)
        .unwrap()
        .unwrap()
        .input_mapping
        .unwrap();
    assert_eq!(game_mapping.gamepad[&GamepadInput::West], "f5");
    assert!(!app.input_mapping.gamepad.contains_key(&GamepadInput::West));

    app.begin_key_capture("north", None).unwrap();
    app.cancel_key_capture().unwrap();
    assert_eq!(
        app.capture_key(KeyCode::A).unwrap_err(),
        "SHINKI_EMU_KEY_CAPTURE_INACTIVE"
    );
    assert!(app.begin_key_capture("bogus", None).is_err());
}

#[test]
fn onboarding_roots_persist_without_loading_cores() {
    let root = tempfile::tempdir().unwrap();
    let path = game(root.path());
    let open = || {
        ShinkiEmuManagerController::open_library(
            root.path().into(),
            FrameMailbox::new(),
            Vec::new(),
        )
        .unwrap()
    };
    let mut app = open();
    assert!(!app.onboarding_completed().unwrap());
    let model = app.add_library_root(&path).unwrap();
    assert_eq!(model.onboarding.roots, [path.to_str().unwrap()]);
    assert_eq!(model.settings.roots, model.onboarding.roots);
    assert!(!app.cores_started());
    assert!(app.library.list_games().unwrap().is_empty());
    let start = Instant::now();
    app.acknowledge_notice(true).unwrap();
    for step in 0..=80 {
        app.notice_presented(start + Duration::from_millis(step * 100), true)
            .unwrap();
    }
    app.finish_onboarding().unwrap();
    drop(app);
    let mut app = open();
    assert!(app.onboarding_completed().unwrap());
    let model = app.remove_library_root(path.to_str().unwrap()).unwrap();
    assert!(model.onboarding.roots.is_empty());
}

#[test]
fn onboarding_requires_checked_notice_and_eight_seconds_of_visible_frames() {
    let root = tempfile::tempdir().unwrap();
    let mut app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        app.finish_onboarding().unwrap_err(),
        "SHINKI_EMU_NOTICE_ACKNOWLEDGEMENT_REQUIRED"
    );
    let start = Instant::now();
    app.acknowledge_notice(true).unwrap();
    for step in 0..80 {
        app.notice_presented(start + Duration::from_millis(step * 100), true)
            .unwrap();
    }
    assert!(app.finish_onboarding().is_err());
    app.notice_presented(start + Duration::from_secs(8), true)
        .unwrap();
    assert!(app.finish_onboarding().is_ok());
    let record = app
        .library
        .app_state::<shinki_emu_manager::resale_notice::Acknowledgement>(
            shinki_emu_manager::resale_notice::ACK_KEY,
        )
        .unwrap()
        .unwrap();
    assert!(record.is_current());
    assert_eq!(record.visible_millis, 8000);
    drop(app);
    let app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(app.onboarding_completed().unwrap());
}

#[test]
fn changed_notice_digest_requires_reading_and_acknowledgement_again() {
    let root = tempfile::tempdir().unwrap();
    let mut app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    app.library
        .set_app_state(ONBOARDING_COMPLETED_KEY, &true)
        .unwrap();
    app.library
        .set_app_state(
            shinki_emu_manager::resale_notice::ACK_KEY,
            &shinki_emu_manager::resale_notice::Acknowledgement {
                digest: "previous notice".into(),
                visible_millis: 8000,
                acknowledged_unix_millis: 1,
            },
        )
        .unwrap();
    drop(app);
    let mut app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(!app.onboarding_completed().unwrap());
    assert_eq!(app.model().unwrap().resale_notice.remaining_seconds, 8);
    assert!(app.finish_onboarding().is_err());
    app.acknowledge_notice(true).unwrap();
    assert!(app.finish_onboarding().is_err());
}

#[test]
fn dwell_without_checkbox_cannot_finish_and_unchecking_revokes_ack() {
    let root = tempfile::tempdir().unwrap();
    let mut app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    let start = Instant::now();
    for step in 0..=80 {
        app.notice_presented(start + Duration::from_millis(step * 100), true)
            .unwrap();
    }
    assert!(app.finish_onboarding().is_err());
    app.acknowledge_notice(true).unwrap();
    assert!(app.finish_onboarding().is_ok());
    app.acknowledge_notice(false).unwrap();
    assert!(app.finish_onboarding().is_err());
    drop(app);
    let app = ShinkiEmuManagerController::open_library(
        root.path().into(),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    assert!(!app.onboarding_completed().unwrap());
}

#[test]
fn patch_selection_keeps_choices_and_reports_steps() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    let script = root.path().join("local.luau");
    assert_eq!(
        app.set_patch_script(&script).unwrap_err(),
        "SHINKI_EMU_PATCH_GAME_REQUIRED"
    );
    assert!(!app.patch_state_view().can_start);
    let location = game(root.path());
    let location = location.to_str().unwrap();
    app.library
        .add_game(&GameRecord {
            game_id: "source".into(),
            title: "Source".into(),
            user_title: None,
            location: location.into(),
            family_id: None,
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    let model = app.select_patch_game("source").unwrap();
    assert_eq!(model.patch.game.id, "source");
    assert!(!model.patch.game.linked);
    assert!(model.patch.recipes.is_empty());
    assert_eq!(model.patch.hint, shinki_emu_manager_ui_slint::tr!("先在 游戏设置→作品资料 关联 VNDB 作品"));
    assert_eq!(model.patch.state.patch_directory, dunce::simplified(Path::new(location)).to_str().unwrap());
    assert_eq!(
        app.select_patch_recipe("missing").unwrap_err(),
        "SHINKI_EMU_PATCH_RECIPE_UNKNOWN"
    );
    app.set_patch_script(&script).unwrap();
    let model = app
        .set_patch_directory("output", &root.path().join("out"))
        .unwrap();
    let state = &model.patch.state;
    assert_eq!(state.step, 0);
    assert_eq!(state.script, script.to_str().unwrap());
    assert!(state.output_directory.ends_with("out"));
    assert!(app.set_patch_directory("elsewhere", root.path()).is_err());
    let model = app.search_patch_games("no such game").unwrap();
    assert!(model.patch.games.is_empty());
    assert_eq!(model.patch.game.id, "source");
    assert_eq!(app.search_patch_games("sour").unwrap().patch.games.len(), 1);
    let model = app.select_patch_game("source").unwrap();
    assert_eq!(model.patch.state.script, script.to_str().unwrap());
    app.patch_state.error = "SHINKI_EMU_PATCH_SCRIPT: failed".into();
    assert_eq!(app.patch_state_view().step, 4);
    app.patch_state.error.clear();
    app.patch_import_succeeded();
    assert_eq!(app.patch_state_view().step, 5);
    let model = app.select_patch_game("source").unwrap();
    assert_eq!(model.patch.state.step, 0);
    assert!(model.patch.state.script.is_empty());
    assert!(model.patch.state.output_directory.is_empty());
}
