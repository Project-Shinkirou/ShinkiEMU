use super::*;
use crate::test_native::{Call, Fixture};
use parking_lot::Mutex;
use std::sync::Arc;

const MUSICA: &str = "shinki.emu.musica";
const RFVP: &str = "rfvp";
const GAME: &str = "game";

type Calls = Arc<Mutex<Vec<Call>>>;

struct Manual {
    app: ShinkiEmuManagerController,
    musica: Calls,
    rfvp: Calls,
}

fn count(calls: &Calls, name: &str) -> usize {
    calls.lock().iter().filter(|call| call.name == name).count()
}

fn register(app: &mut ShinkiEmuManagerController, slot: usize, plugin: &'static str, family: &'static str, marker: Option<&'static str>) -> Calls {
    let calls: Calls = Arc::new(Mutex::new(Vec::new()));
    let native = calls.clone();
    app.registry.register_provider(move || {
        let mut fixture = Fixture::new(plugin, family, "manual.test");
        fixture.calls = native;
        fixture.marker = marker;
        crate::test_native::install(slot, fixture)
    }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.registry.descriptor(plugin).is_none() {
        app.poll_platform().unwrap();
        assert!(Instant::now() < deadline, "provider metadata did not arrive");
        std::thread::yield_now();
    }
    calls
}

/// One library game whose rescan found no rule detections. Musica is an
/// installed official core; RFVP is one too when `with_rfvp` is set.
fn unrecognized(root: &Path, with_rfvp: bool, rfvp_marker: Option<&'static str>) -> Manual {
    super::super::recognition_tests::manifest(root, MUSICA, "manual_musica", b"manual musica fixture");
    if with_rfvp {
        super::super::recognition_tests::manifest(root, RFVP, "manual_rfvp", b"manual rfvp fixture");
    }
    let mut app = ShinkiEmuManagerController::open_library(root.to_path_buf(), FrameMailbox::new(), Vec::new()).unwrap();
    app.set_host_wake(Arc::new(|| {}));
    super::super::probe_state_tests::settle_background(&mut app);
    let musica = register(&mut app, 0, MUSICA, "musica", None);
    let rfvp = if with_rfvp { register(&mut app, 1, RFVP, "fvp", rfvp_marker) } else { Arc::default() };
    let location = root.join("game");
    fs::create_dir_all(&location).unwrap();
    app.library.add_game(&GameRecord {
        game_id: GAME.into(),
        title: "Manual test".into(),
        user_title: None,
        location: location.to_str().unwrap().into(),
        family_id: None,
        content_fingerprint: None,
        added_at_unix_ms: 1,
    }).unwrap();
    app.library.set_app_state(&format!("recognition:{GAME}"), &Vec::<shinki_emu_manager_core::detection::EngineDetection>::new()).unwrap();
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    app.selected_case_id = Some(GAME.into());
    app.window_state = Some(WindowState { width: 1, height: 1, scale_factor: 1.0, focused: true, visible: true, suspended: false });
    Manual { app, musica, rfvp }
}

fn choice_ids(app: &ShinkiEmuManagerController) -> Vec<String> {
    app.probe_choices[GAME].iter().map(|choice| choice.plugin_id.clone()).collect()
}

fn saved(app: &ShinkiEmuManagerController) -> Option<String> {
    app.library.game_settings(GAME).unwrap().and_then(|settings| settings.emulator_id)
}

fn close_session(app: &mut ShinkiEmuManagerController) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut error = None;
    while app.active.is_some() {
        if let Err(failure) = app.poll_platform() {
            error = Some(failure);
        }
        assert!(Instant::now() < deadline, "native session did not close");
        std::thread::yield_now();
    }
    error
}

#[test]
fn unrecognized_game_offers_installed_official_cores_without_picking_or_probing() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, musica, rfvp } = unrecognized(root.path(), true, None);
    let ids = choice_ids(&app);
    assert!(ids.contains(&MUSICA.to_owned()) && ids.contains(&RFVP.to_owned()));
    assert!(ids.iter().all(|id| shinki_emu_manager_core::static_core_catalog::lookup(id).is_some() && app.plugin_available(id)));
    assert!(!app.candidates.contains_key(GAME));
    let model = app.model().unwrap();
    assert_eq!(model.selection.emulator_choices.len(), ids.len());
    assert_eq!(model.selection.game.emulator_id, "");
    assert_eq!(model.selection.game.engine, "未识别引擎");
    assert!(!model.selection.game.launchable);
    assert_eq!(model.selection.game.problem, "需要选择模拟器");
    assert_eq!(app.launch(GAME).unwrap_err(), "SHINKI_EMU_FAMILY_PROVIDER_SELECTION_REQUIRED");

    // Outside the eligible official set: unknown third-party, catalogued but not installed.
    assert_eq!(app.set_game_emulator(GAME, "third.party.core").unwrap_err(), "SHINKI_EMU_EMULATOR_SELECTION_INVALID");
    let missing = shinki_emu_manager_core::static_core_catalog::entries().iter()
        .find(|entry| !app.plugin_available(entry.plugin_id)).expect("a catalogued core that is not installed");
    assert_eq!(app.set_game_emulator(GAME, missing.plugin_id).unwrap_err(), "SHINKI_EMU_EMULATOR_SELECTION_INVALID");

    let model = app.set_game_emulator(GAME, MUSICA).unwrap();
    assert_eq!(model.selection.game.emulator_id, MUSICA);
    assert!(model.selection.game.launchable);
    assert_eq!(model.game_settings.emulator_id, MUSICA);
    assert_eq!(saved(&app), None);
    assert_eq!(app.game(GAME).unwrap().family_id, None);
    // A normal refresh keeps the pending pick.
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    assert_eq!(app.candidates[GAME].plugin_id, MUSICA);
    for calls in [&musica, &rfvp] {
        assert_eq!(count(calls, "probe"), 0);
        assert_eq!(count(calls, "open"), 0);
    }
}

#[test]
fn a_single_eligible_core_is_listed_but_never_auto_picked() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, .. } = unrecognized(root.path(), false, None);
    #[cfg(not(feature = "static-cmvs"))]
    assert_eq!(choice_ids(&app), [MUSICA]);
    assert!(!app.candidates.contains_key(GAME));
    let model = app.model().unwrap();
    assert_eq!(model.selection.emulator_choices.len(), app.probe_choices[GAME].len());
    assert!(!model.selection.game.launchable);
}

#[test]
fn recognized_single_candidate_still_associates_automatically() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, .. } = unrecognized(root.path(), false, None);
    let detection = shinki_emu_manager_core::detection::EngineDetection { family_id: "musica".into(), format: "manual.test".into() };
    app.library.set_app_state(&format!("recognition:{GAME}"), &vec![detection]).unwrap();
    app.set_probe_candidates(GAME, vec![Candidate { plugin_id: MUSICA.into(), family_id: "musica".into() }]).unwrap();
    assert!(!app.manual_games.contains(GAME));
    assert_eq!(saved(&app).as_deref(), Some(MUSICA));
    assert_eq!(app.game(GAME).unwrap().family_id.as_deref(), Some("musica"));
    assert!(app.model().unwrap().selection.emulator_choices.is_empty());
}

#[test]
fn manual_core_is_saved_once_after_its_first_valid_frame() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, musica, rfvp } = unrecognized(root.path(), true, None);
    app.set_game_emulator(GAME, MUSICA).unwrap();
    app.launch(GAME).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.verified_manual_core(GAME).unwrap().is_none() {
        app.poll_platform().unwrap();
        assert!(Instant::now() < deadline, "first frame was not observed: {}", app.diagnostic);
        std::thread::yield_now();
    }
    assert_eq!(count(&musica, "probe"), 1);
    assert_eq!(count(&rfvp, "probe"), 0);
    assert!(count(&musica, "frame") >= 1);
    assert!(app.manual_launch.is_none());
    assert_eq!(saved(&app).as_deref(), Some(MUSICA));
    // The verified pick is not a detection result.
    assert_eq!(app.game(GAME).unwrap().family_id, None);
    assert!(app.library.app_state::<Vec<shinki_emu_manager_core::detection::EngineDetection>>(&format!("recognition:{GAME}")).unwrap().unwrap().is_empty());

    // Later frames never write again.
    let mut settings = app.library.game_settings(GAME).unwrap().unwrap();
    settings.emulator_id = Some(RFVP.into());
    app.library.set_game_settings(GAME, &settings).unwrap();
    let frames = count(&musica, "frame");
    let deadline = Instant::now() + Duration::from_secs(5);
    while count(&musica, "frame") < frames + 5 {
        app.poll_platform().unwrap();
        assert!(Instant::now() < deadline, "session stopped producing frames");
        std::thread::yield_now();
    }
    assert_eq!(saved(&app).as_deref(), Some(RFVP));
    settings.emulator_id = Some(MUSICA.into());
    app.library.set_game_settings(GAME, &settings).unwrap();
    app.leave_game().unwrap();
    assert_eq!(close_session(&mut app), None);

    // Refresh and restart restore the verified pick without auto-picking.
    app.clear_probe_state(GAME).unwrap();
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    assert_eq!(app.candidates[GAME].plugin_id, MUSICA);
    let model = app.model().unwrap();
    assert_eq!(model.selection.game.emulator_id, MUSICA);
    assert!(model.selection.game.launchable);
}

#[test]
fn stale_detector_binding_neither_blocks_nor_prepares_a_pending_pick() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, musica, rfvp } = unrecognized(root.path(), true, None);
    app.library.set_game_family(GAME, Some("fvp")).unwrap();
    app.library.set_game_settings(GAME, &shinki_emu_manager_core::GameSettings { emulator_id: Some("removed.detector.core".into()), ..Default::default() }).unwrap();
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    assert!(!app.candidates.contains_key(GAME));
    let model = app.model().unwrap();
    assert_eq!(model.selection.game.engine, "未识别引擎");
    assert_eq!(model.selection.game.emulator_id, "");
    assert_eq!(model.selection.game.problem, "需要选择模拟器");
    assert_eq!(app.prepare_game_configuration(GAME).unwrap_err(), "SHINKI_EMU_EMULATOR_SELECTION_REQUIRED");

    let model = app.set_game_emulator(GAME, MUSICA).unwrap();
    assert!(model.selection.game.launchable);
    assert_eq!(model.game_settings.configuration_state, "manual-pending");
    assert_eq!(app.prepare_game_configuration(GAME).unwrap_err(), "SHINKI_EMU_EMULATOR_SELECTION_REQUIRED");
    assert!(app.native_action.is_none());
    for calls in [&musica, &rfvp] {
        assert_eq!(count(calls, "probe"), 0);
        assert_eq!(count(calls, "open"), 0);
    }
}

#[test]
fn failed_probe_or_missing_frame_keeps_the_previous_saved_choice() {
    let root = tempfile::tempdir().unwrap();
    let Manual { mut app, rfvp, .. } = unrecognized(root.path(), true, Some("missing-manual-marker"));
    app.library.set_game_settings(GAME, &shinki_emu_manager_core::GameSettings { emulator_id: Some(MUSICA.into()), ..Default::default() }).unwrap();
    app.library.set_app_state(&format!("manual-core:{GAME}"), MUSICA).unwrap();
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    assert_eq!(app.candidates[GAME].plugin_id, MUSICA);

    // The chosen core rejects the directory at launch; nothing falls back.
    app.set_game_emulator(GAME, RFVP).unwrap();
    app.launch(GAME).unwrap();
    let error = close_session(&mut app).expect("fresh probe failure is reported");
    assert!(error.contains("SHINKI_EMU_FAMILY_FRESH_PROBE_NO_MATCH"), "{error}");
    assert_eq!(count(&rfvp, "probe"), 1);
    assert_eq!(count(&rfvp, "open"), 0);
    app.poll_platform().unwrap();
    assert!(app.manual_launch.is_none());
    assert_eq!(saved(&app).as_deref(), Some(MUSICA));
    assert_eq!(app.verified_manual_core(GAME).unwrap().as_deref(), Some(MUSICA));
    assert_eq!(app.candidates[GAME].plugin_id, RFVP);

    // Opened but closed before any frame.
    let epoch = 7;
    app.mailbox.activate(epoch).unwrap();
    app.active = Some(owner::ActiveSessionView::opening(epoch, GAME.into()));
    app.manual_launch = Some((epoch, GAME.into(), RFVP.into()));
    app.commit_manual_launch().unwrap();
    assert!(app.manual_launch.is_some());
    app.mailbox.clear(epoch).unwrap();
    app.active = None;
    app.commit_manual_launch().unwrap();
    assert!(app.manual_launch.is_none());
    assert_eq!(saved(&app).as_deref(), Some(MUSICA));
    assert_eq!(app.verified_manual_core(GAME).unwrap().as_deref(), Some(MUSICA));
}

#[test]
fn without_an_eligible_core_the_unavailable_state_has_no_picker() {
    let root = tempfile::tempdir().unwrap();
    let mut app = ShinkiEmuManagerController::open_library(root.path().to_path_buf(), FrameMailbox::new(), Vec::new()).unwrap();
    app.set_host_wake(Arc::new(|| {}));
    super::super::probe_state_tests::settle_background(&mut app);
    let location = root.path().join("game");
    fs::create_dir_all(&location).unwrap();
    app.library.add_game(&GameRecord {
        game_id: GAME.into(),
        title: "Manual test".into(),
        user_title: None,
        location: location.to_str().unwrap().into(),
        family_id: None,
        content_fingerprint: None,
        added_at_unix_ms: 1,
    }).unwrap();
    app.library.set_app_state(&format!("recognition:{GAME}"), &Vec::<shinki_emu_manager_core::detection::EngineDetection>::new()).unwrap();
    app.set_probe_candidates(GAME, Vec::new()).unwrap();
    app.selected_case_id = Some(GAME.into());
    if !shinki_emu_manager::static_cores::STATIC_CORES.is_empty() {
        return;
    }
    assert!(!app.probe_choices.contains_key(GAME));
    let model = app.model().unwrap();
    assert!(model.selection.emulator_choices.is_empty());
    assert_eq!(model.selection.game.problem, "未识别到可用的模拟器");
    assert_eq!(app.set_game_emulator(GAME, MUSICA).unwrap_err(), "SHINKI_EMU_EMULATOR_SELECTION_INVALID");
}
