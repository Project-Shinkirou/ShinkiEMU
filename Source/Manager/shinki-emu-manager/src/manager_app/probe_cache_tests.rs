use super::*;
use super::super::{probe_worker::{ScanKind, ScanReport, ScanRequest}, recognition_tests::{hcb, write_game}};
use shinki_emu_manager_core::NativeCancellation;
use std::sync::atomic::AtomicBool;

fn game(path: &Path) -> GameRecord {
    GameRecord { game_id: "registered".into(), title: "Saved title".into(), user_title: Some("Alias".into()),
        location: path.to_str().unwrap().into(), family_id: Some("fvp".into()), content_fingerprint: None, added_at_unix_ms: 1 }
}
fn run_scan(cache: &mut ProbeCache, kind: ScanKind, paths: Vec<PathBuf>, games: Vec<GameRecord>) -> ScanReport {
    super::super::probe_worker::scan(cache, ScanRequest { kind, paths, games, excluded: BTreeSet::new(), cancel: NativeCancellation::new(0) },
        &AtomicBool::new(false), &mut |_| {}).unwrap()
}
fn refresh(cache: &mut ProbeCache, path: &Path) -> ScanReport { run_scan(cache, ScanKind::Full, Vec::new(), vec![game(path)]) }

#[test]
fn registered_hints_follow_resource_changes_but_not_native_inventory() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    let first = refresh(&mut cache, &path);
    assert_eq!(first.cache_hits, 0);
    assert_eq!(first.games[0].detections[0].family_id, "fvp");
    let mut cache = ProbeCache::load(root.path());
    assert_eq!(refresh(&mut cache, &path).cache_hits, 1);
    fs::write(root.path().join("new-core.dll"), b"inventory is not a recognition input").unwrap();
    assert_eq!(refresh(&mut cache, &path).cache_hits, 1);
    fs::write(path.join("boot.hcb"), b"truncated").unwrap();
    let changed = refresh(&mut cache, &path);
    assert_eq!(changed.cache_hits, 0);
    assert!(changed.games[0].detections.is_empty());
    fs::write(path.join("boot.hcb"), hcb()).unwrap();
    assert_eq!(refresh(&mut cache, &path).games[0].detections.len(), 1);
    fs::remove_file(path.join("boot.hcb")).unwrap();
    assert!(refresh(&mut cache, &path).games[0].detections.is_empty());
}

#[test]
fn cache_upgrade_and_detector_rule_changes_are_misses() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    refresh(&mut cache, &path);
    let file = root.path().join("probe-cache.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    for (key, value) in [("version", 1u32), ("detector_version", detection::CACHE_VERSION + 1)] {
        let mut changed = original.clone();
        changed[key] = value.into();
        fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
        let mut cache = ProbeCache::load(root.path());
        assert!(cache.get(&path).is_none());
        assert_eq!(refresh(&mut cache, &path).games[0].detections.len(), 1);
    }
}

#[test]
fn malformed_cache_deep_layouts_and_missing_roots_never_drop_library_records() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    fs::write(root.path().join("probe-cache.json"), b"{broken").unwrap();
    let mut cache = ProbeCache::load(root.path());
    assert_eq!(refresh(&mut cache, &path).cache_hits, 0);
    fs::create_dir_all(path.join("unknown/nested")).unwrap();
    assert_eq!(refresh(&mut cache, &path).cache_hits, 0);
    assert!(cache.get(&path).is_none());
    fs::remove_dir_all(path.join("unknown")).unwrap();
    refresh(&mut cache, &path);
    fs::remove_dir_all(&path).unwrap();
    let report = refresh(&mut cache, &path);
    assert_eq!(report.missing, 1);
    assert_eq!(report.games[0].game_ids, ["registered"]);
    assert!(cache.entries.is_empty());
}

#[test]
fn unregistered_discoveries_and_explicit_imports_always_read_freshly() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    refresh(&mut cache, &path);
    let discovered = run_scan(&mut cache, ScanKind::Full, vec![root.path().into()], Vec::new());
    assert_eq!(discovered.cache_hits, 0);
    assert_eq!(discovered.games.len(), 1);
    assert!(discovered.games[0].game_ids.is_empty());
    let imported = run_scan(&mut cache, ScanKind::Import, vec![path.clone()], vec![game(&path)]);
    assert_eq!(imported.cache_hits, 0);
    assert_eq!(imported.games[0].game_ids, ["registered"]);
}

#[test]
fn owned_saves_do_not_invalidate_recognition_but_assets_do() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let saves = path.join(".shinki/musica/saves");
    fs::create_dir_all(&saves).unwrap();
    let mut cache = ProbeCache::load(root.path());
    refresh(&mut cache, &path);
    fs::write(saves.join("slot-01"), b"new save").unwrap();
    assert_eq!(refresh(&mut cache, &path).cache_hits, 1);
    fs::write(path.join("boot.hcb"), b"changed asset").unwrap();
    assert_eq!(refresh(&mut cache, &path).cache_hits, 0);
}

#[test]
fn registered_nested_games_are_independently_refreshed() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("parent");
    let child = parent.join("child");
    write_game(&parent);
    write_game(&child);
    let mut nested = game(&child);
    nested.game_id = "nested".into();
    let mut cache = ProbeCache::load(root.path());
    let report = run_scan(&mut cache, ScanKind::Full, Vec::new(), vec![game(&parent), nested]);
    assert_eq!(report.games.len(), 2);
    for result in report.games { assert_eq!(result.detections.len(), 1); }
}

#[test]
fn scan_progress_exclusions_and_cancellation_do_not_publish_partial_results() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..70 { fs::create_dir(root.path().join(format!("empty-{index:02}"))).unwrap(); }
    let hidden = root.path().join("hidden");
    write_game(&hidden);
    let mut cache = ProbeCache::load(root.path());
    let mut progress = Vec::new();
    let report = super::super::probe_worker::scan(&mut cache, ScanRequest { kind: ScanKind::Full, paths: vec![root.path().into()], games: Vec::new(),
        excluded: BTreeSet::from([hidden]), cancel: NativeCancellation::new(0) }, &AtomicBool::new(false), &mut |event| progress.push(event)).unwrap();
    assert!(report.games.is_empty());
    assert!(!progress.is_empty());
    let cancel = NativeCancellation::new(0);
    let worker_cancel = cancel.clone();
    let result = super::super::probe_worker::scan(&mut cache, ScanRequest { kind: ScanKind::Full, paths: vec![root.path().into()], games: Vec::new(),
        excluded: BTreeSet::new(), cancel: worker_cancel }, &AtomicBool::new(false), &mut |_| cancel.cancel());
    assert_eq!(result.err().as_deref(), Some("SHINKI_EMU_SCAN_CANCELLED"));
}

#[test]
fn cancellation_read_limits_and_io_errors_are_not_negative_cache_entries() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    let cancel = NativeCancellation::new(0);
    cancel.cancel();
    assert!(cache.detect(&path, false, &cancel).unwrap_err().contains("CANCELLED"));
    assert!(cache.get(&path).is_none());
    for index in 0..1030 { fs::write(path.join(format!("resource-{index}")), b"x").unwrap(); }
    assert!(cache.detect(&path, false, &NativeCancellation::new(0)).unwrap_err().contains("limit"));
    assert!(cache.get(&path).is_none());
}

#[cfg(windows)]
#[test]
fn real_sharing_violation_is_an_error_not_no_match_or_a_cache_hit() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    refresh(&mut cache, &path);
    let locked = fs::OpenOptions::new().read(true).share_mode(0).open(path.join("boot.hcb")).unwrap();
    assert!(cache.detect(&path, false, &NativeCancellation::new(0)).unwrap_err().contains("cannot read"));
    assert!(cache.get(&path).is_none());
    drop(locked);
    assert_eq!(cache.detect(&path, false, &NativeCancellation::new(0)).unwrap().0.len(), 1);
}

#[test]
fn cached_recognition_is_freshly_rejected_before_open_without_probing_alternatives() {
    use crate::test_native::{self, Fixture};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    cache.detect(&path, false, &NativeCancellation::new(0)).unwrap();
    cache.save().unwrap();
    let selected_calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let other_calls = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut selected = Fixture::new("rfvp", "fvp", "fixture.native");
    selected.marker = Some("native-marker");
    selected.calls = selected_calls.clone();
    let mut other = Fixture::new("alternative", "fvp", "fixture.native");
    other.calls = other_calls.clone();
    let mut registry = FamilyProviderRegistry::new();
    registry.register_provider(test_native::install(0, selected)).unwrap();
    registry.register_provider(test_native::install(1, other)).unwrap();
    let request = ProbeRequest { game_path: path.to_str().unwrap().into() };
    let FamilyProbeSelection::Selected(candidate) = registry.probe_selected(&request, "rfvp").unwrap() else { panic!("selected fixture") };
    selected_calls.lock().clear();
    other_calls.lock().clear();
    fs::remove_file(path.join("native-marker")).unwrap();
    // The family remains recognizable; a static recognition result is not an
    // implementation-specific open credential.
    assert_eq!(cache.detect(&path, true, &NativeCancellation::new(0)).unwrap().0[0].family_id, "fvp");
    let epoch = shinki_emu_manager_core::model::allocate_session_epoch().unwrap();
    let cancel = NativeCancellation::new(epoch);
    let error = registry.open_selected(&candidate, NativeOpenRequest {
        game_path: request.game_path, configuration: Vec::new(), session_epoch: epoch,
        initial_window: WindowState { width: 1, height: 1, scale_factor: 1.0, focused: true, visible: true, suspended: false },
        host: shinki_emu_manager_core::no_host_services(&cancel), cancellation: cancel, text_async: None, render_extent: None,
    }).err().unwrap();
    assert!(error.to_string().contains("SHINKI_EMU_FAMILY_FRESH_PROBE_NO_MATCH"));
    assert_eq!(selected_calls.lock().iter().filter(|call| call.name == "probe").count(), 1);
    assert!(!selected_calls.lock().iter().any(|call| call.name == "open"));
    assert!(other_calls.lock().is_empty());
}

#[test]
fn rejected_duplicate_provider_cannot_be_revived_by_persistent_family_recognition() {
    use crate::test_native::{self, Fixture};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("title");
    write_game(&path);
    let mut cache = ProbeCache::load(root.path());
    cache.detect(&path, false, &NativeCancellation::new(0)).unwrap();
    cache.save().unwrap();
    let mut registry = FamilyProviderRegistry::new();
    registry.register_provider(test_native::install(0, Fixture::new("rfvp", "fvp", "fixture.native"))).unwrap();
    assert!(registry.register_provider(test_native::install(1, Fixture::new("rfvp", "fvp", "fixture.native"))).is_err());
    assert!(registry.descriptor("rfvp").is_none());
    let cache = ProbeCache::load(root.path());
    assert_eq!(cache.get(&path).unwrap()[0].family_id, "fvp");
    assert!(registry.probe_selected(&ProbeRequest { game_path: path.to_str().unwrap().into() }, "rfvp").is_err());
}
