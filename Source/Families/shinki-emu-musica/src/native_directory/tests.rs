use super::*;
use std::time::{Duration, SystemTime};

fn game(executable: Option<&str>) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    if let Some(executable) = executable {
        std::fs::write(root.path().join(executable), b"").unwrap();
    }
    root
}

fn open(root: &Path) -> NativeFiles {
    NativeFiles::open(root, ScriptEncoding::ShiftJis).unwrap()
}

fn age(path: &Path) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    old
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

fn globals(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

fn progress_json(game: Hash256, unlocks: &[&str]) -> Vec<u8> {
    let mut unlocks: Vec<Hash256> = unlocks
        .iter()
        .map(|flag| Hash256::from_sha256(flag.as_bytes()))
        .collect();
    unlocks.sort_unstable();
    serde_json::to_vec(&serde_json::json!({
        "schema": "shinki.musica.global_progress.v1",
        "game": game,
        "unlocks": unlocks,
    }))
    .unwrap()
}

#[test]
fn globals_round_trip_through_each_title_envelope_and_keep_foreign_entries() {
    for title in [MinoriTitle::EdenEnglish, MinoriTitle::Perseus] {
        let root = game(Some(title.executable()));
        let mut files = open(root.path());
        let dir = files.dir.as_ref().unwrap();
        let foreign = Properties::from([(b"\x82\xa0".to_vec(), b"text".to_vec())]);
        dir.write_system_data(&foreign).unwrap();
        let mut files_reopened = open(root.path());
        assert!(files_reopened.globals().is_empty());
        files_reopened
            .store_globals(&globals(&[("REN_CLEAR", 1), ("custom", -3)]))
            .unwrap();
        files = open(root.path());
        assert_eq!(
            files.globals(),
            globals(&[("REN_CLEAR", 1), ("custom", -3)])
        );
        let path = root
            .path()
            .join(".shinki/musica/minori")
            .join(title.directory())
            .join(SYSTEM);
        let stored =
            minori_data::decode_system(&std::fs::read(&path).unwrap(), title.system_format())
                .unwrap();
        assert_eq!(stored[b"\x82\xa0".as_slice()], b"text");
        assert_eq!(stored[b"custom".as_slice()], b"-3");
    }
}

#[test]
fn unchanged_tables_are_not_rewritten() {
    let root = game(Some("perseus.exe"));
    let mut files = open(root.path());
    let values = globals(&[("AYAME_CLEAR", 1)]);
    files.store_globals(&values).unwrap();
    files.record_cg("ev00_002a01.png").unwrap();
    let dir = root.path().join(".shinki/musica/minori/夏空のペルセウス");
    let system_time = age(&dir.join(SYSTEM));
    let cg_time = age(&dir.join(CG_VIEWS));
    let mut files = open(root.path());
    files.store_globals(&values).unwrap();
    files.record_cg("ev00_002a01.png").unwrap();
    assert_eq!(modified(&dir.join(SYSTEM)), system_time);
    assert_eq!(modified(&dir.join(CG_VIEWS)), cg_time);
    files
        .store_globals(&globals(&[("AYAME_CLEAR", 2)]))
        .unwrap();
    files.record_cg("black.png").unwrap();
    assert_ne!(modified(&dir.join(SYSTEM)), system_time);
    assert_ne!(modified(&dir.join(CG_VIEWS)), cg_time);
}

#[test]
fn cg_views_are_stored_sorted_once_with_the_resolved_prefix() {
    let root = game(Some("eden_en.exe"));
    let mut files = open(root.path());
    for name in ["white.png", "black.png", "white.png"] {
        files.record_cg(name).unwrap();
    }
    let dir = files.dir.as_ref().unwrap();
    assert_eq!(dir.prefix.value, 0);
    let bytes = std::fs::read(dir.root.join(CG_VIEWS)).unwrap();
    assert_eq!(&bytes[..4], &[0; 4]);
    assert_eq!(
        minori_data::decode_cg_names(&bytes, 0).unwrap(),
        [b"black.png".to_vec(), b"white.png".to_vec()]
    );
}

#[test]
fn legacy_progress_imports_once_and_never_overwrites_native_files() {
    let id = Hash256::from_sha256(b"game");
    let root = game(Some("perseus.exe"));
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(
        saves.join(legacy::PROGRESS),
        progress_json(id, &["REN_CLEAR"]),
    )
    .unwrap();
    let mut files = open(root.path());
    files.import_legacy_progress(root.path(), id).unwrap();
    assert_eq!(files.globals(), globals(&[("REN_CLEAR", 1)]));
    assert!(!saves.join(legacy::PROGRESS).exists());
    let imported = saves.join("global-progress.json.imported");
    assert!(imported.exists());
    let mut again = open(root.path());
    again.import_legacy_progress(root.path(), id).unwrap();
    assert_eq!(again.globals(), globals(&[("REN_CLEAR", 1)]));
    assert!(imported.exists());

    // A native system.dat that already exists wins; the JSON stays untouched.
    let second = progress_json(id, &["SUI_CLEAR"]);
    std::fs::write(saves.join(legacy::PROGRESS), &second).unwrap();
    let mut kept = open(root.path());
    kept.import_legacy_progress(root.path(), id).unwrap();
    assert_eq!(kept.globals(), globals(&[("REN_CLEAR", 1)]));
    assert_eq!(std::fs::read(saves.join(legacy::PROGRESS)).unwrap(), second);
}

#[test]
fn damaged_or_foreign_progress_fails_without_touching_any_file() {
    let root = game(Some("perseus.exe"));
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    for bytes in [
        b"damaged".to_vec(),
        progress_json(Hash256::from_sha256(b"other"), &["REN_CLEAR"]),
    ] {
        std::fs::write(saves.join(legacy::PROGRESS), &bytes).unwrap();
        let mut files = open(root.path());
        assert_eq!(
            files
                .import_legacy_progress(root.path(), Hash256::from_sha256(b"game"))
                .unwrap_err()
                .code,
            "SHINKI_EMU_MUSICA_GLOBAL_PROGRESS"
        );
        assert_eq!(std::fs::read(saves.join(legacy::PROGRESS)).unwrap(), bytes);
        assert!(
            !root
                .path()
                .join(".shinki/musica/minori/夏空のペルセウス")
                .join(SYSTEM)
                .exists()
        );
    }
}

#[test]
fn unknown_executable_reads_legacy_progress_and_writes_nothing() {
    let id = Hash256::from_sha256(b"game");
    let root = game(None);
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(
        saves.join(legacy::PROGRESS),
        progress_json(id, &["TOHKA_CLEAR"]),
    )
    .unwrap();
    let mut files = open(root.path());
    assert!(files.dir.is_none());
    files.import_legacy_progress(root.path(), id).unwrap();
    assert_eq!(files.globals(), globals(&[("TOHKA_CLEAR", 1)]));
    files
        .store_globals(&globals(&[("TOHKA_CLEAR", 1), ("x", 2)]))
        .unwrap();
    files.record_cg("black.png").unwrap();
    assert!(saves.join(legacy::PROGRESS).exists());
    assert!(!root.path().join(".shinki/musica/minori").exists());
}

fn sysparams_path(root: &Path, title: MinoriTitle) -> PathBuf {
    root.join(".shinki/musica/minori")
        .join(title.directory())
        .join(sysparams::SYSPARAMS)
}

fn legacy_json(schema: &str, game: Hash256, field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut object = serde_json::json!({ "schema": schema, "game": game });
    object[field] = value;
    serde_json::to_vec(&object).unwrap()
}

#[test]
fn configuration_round_trips_through_sysparams_and_keeps_native_keys() {
    for title in [MinoriTitle::EdenEnglish, MinoriTitle::Perseus] {
        let root = game(Some(title.executable()));
        let path = sysparams_path(root.path(), title);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // screenMode 3 (maximized) is not fullscreen and must survive a write.
        std::fs::write(
            &path,
            b"fontname\t\x82l\x82r \x83S\x83V\x83b\x83N\r\nscreenMode\t3\r\nwindowWidth\t1280\r\n",
        )
        .unwrap();
        let mut files = open(root.path());
        let mut config = files.configuration(crate::MusicaConfigState::default());
        assert!(!config.fullscreen);
        config.message_speed_unread = 7;
        config.message_speed_auto_play = 90;
        config.preferred_play_mode = crate::MusicaPlayMode::Skip;
        config.text_shadow = false;
        config.voice_volume = 33;
        config.se_muted = true;
        config.character_voice_enabled = [true, false, true, true, false];
        files.store_configuration(&config).unwrap();
        files.store_quick_cursor(4).unwrap();
        files.store_auto_cursor(6).unwrap();

        let reopened = open(root.path());
        assert_eq!(
            reopened.configuration(crate::MusicaConfigState::default()),
            config
        );
        assert_eq!(reopened.quick_cursor(), 4);
        assert_eq!(reopened.auto_cursor(), 6);
        let stored = minori_data::decode_settings(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            stored[b"fontname".as_slice()],
            b"\x82l\x82r \x83S\x83V\x83b\x83N"
        );
        assert_eq!(stored[b"screenMode".as_slice()], b"3");
        assert_eq!(stored[b"windowWidth".as_slice()], b"1280");
        assert_eq!(stored[b"messageSpeedTBR".as_slice()], b"7");
        assert_eq!(stored[b"playMode".as_slice()], b"0");
        assert_eq!(stored[b"quickSaveFileNumber".as_slice()], b"4");
        let musica = root
            .path()
            .join(".shinki/musica")
            .join(sysparams::MUSICA_SETTINGS);
        match title {
            MinoriTitle::Perseus => {
                assert_eq!(stored[b"sui".as_slice()], b"0");
                assert_eq!(stored[b"etc".as_slice()], b"0");
                assert!(!musica.exists());
            }
            MinoriTitle::EdenEnglish => {
                assert!(!stored.contains_key(b"sui".as_slice()));
                let extra = minori_data::decode_settings(&std::fs::read(&musica).unwrap()).unwrap();
                assert_eq!(extra[b"voice_sui".as_slice()], b"0");
                assert_eq!(extra[b"voice_mot".as_slice()], b"0");
            }
        }
    }
}

#[test]
fn unchanged_settings_are_not_rewritten() {
    let root = game(Some("perseus.exe"));
    let mut files = open(root.path());
    let config = crate::MusicaConfigState::default();
    files.store_configuration(&config).unwrap();
    files.store_quick_cursor(3).unwrap();
    let path = sysparams_path(root.path(), MinoriTitle::Perseus);
    let time = age(&path);
    let mut files = open(root.path());
    files.store_configuration(&config).unwrap();
    files.store_quick_cursor(3).unwrap();
    assert_eq!(modified(&path), time);
    files.store_quick_cursor(5).unwrap();
    assert_ne!(modified(&path), time);
}

#[test]
fn legacy_settings_import_once_and_never_overwrite_sysparams() {
    let id = Hash256::from_sha256(b"game");
    let root = game(Some("perseus.exe"));
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    let mut config = crate::MusicaConfigState::default();
    config.bgm_volume = 12;
    let configuration = legacy_json(
        "shinki.musica.configuration.v1",
        id,
        "settings",
        serde_json::to_value(&config).unwrap(),
    );
    let cursor = legacy_json("shinki.musica.quick_cursor.v1", id, "next", 7.into());
    std::fs::write(saves.join(legacy::CONFIGURATION), &configuration).unwrap();
    std::fs::write(saves.join(legacy::QUICK_CURSOR), &cursor).unwrap();

    let mut files = open(root.path());
    files.import_legacy_settings(root.path(), id).unwrap();
    assert_eq!(
        files.configuration(crate::MusicaConfigState::default()),
        config
    );
    assert_eq!(files.quick_cursor(), 7);
    assert!(saves.join("configuration.json.imported").exists());
    assert!(saves.join("quick-cursor.json.imported").exists());
    assert!(!saves.join(legacy::CONFIGURATION).exists());
    let reopened = open(root.path());
    assert_eq!(reopened.quick_cursor(), 7);

    // An existing sysparams.txt wins; new JSON files stay where they are.
    let path = sysparams_path(root.path(), MinoriTitle::Perseus);
    let native = std::fs::read(&path).unwrap();
    let other = legacy_json("shinki.musica.quick_cursor.v1", id, "next", 1.into());
    std::fs::write(saves.join(legacy::QUICK_CURSOR), &other).unwrap();
    let mut kept = open(root.path());
    kept.import_legacy_settings(root.path(), id).unwrap();
    assert_eq!(kept.quick_cursor(), 7);
    assert_eq!(std::fs::read(&path).unwrap(), native);
    assert_eq!(
        std::fs::read(saves.join(legacy::QUICK_CURSOR)).unwrap(),
        other
    );
}

#[test]
fn foreign_legacy_settings_fail_without_touching_any_file() {
    let root = game(Some("perseus.exe"));
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    let foreign = legacy_json(
        "shinki.musica.quick_cursor.v1",
        Hash256::from_sha256(b"other"),
        "next",
        2.into(),
    );
    std::fs::write(saves.join(legacy::QUICK_CURSOR), &foreign).unwrap();
    let mut files = open(root.path());
    assert_eq!(
        files
            .import_legacy_settings(root.path(), Hash256::from_sha256(b"game"))
            .unwrap_err()
            .code,
        "SHINKI_EMU_MUSICA_QUICK_CURSOR"
    );
    assert_eq!(
        std::fs::read(saves.join(legacy::QUICK_CURSOR)).unwrap(),
        foreign
    );
    assert!(!sysparams_path(root.path(), MinoriTitle::Perseus).exists());
}

#[test]
fn unknown_executable_reads_legacy_settings_and_writes_nothing() {
    let id = Hash256::from_sha256(b"game");
    let root = game(None);
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    let mut config = crate::MusicaConfigState::default();
    config.animation = false;
    std::fs::write(
        saves.join(legacy::CONFIGURATION),
        legacy_json(
            "shinki.musica.configuration.v1",
            id,
            "settings",
            serde_json::to_value(&config).unwrap(),
        ),
    )
    .unwrap();
    std::fs::write(
        saves.join(legacy::QUICK_CURSOR),
        legacy_json("shinki.musica.quick_cursor.v1", id, "next", 5.into()),
    )
    .unwrap();
    let mut files = open(root.path());
    files.import_legacy_settings(root.path(), id).unwrap();
    assert_eq!(
        files.configuration(crate::MusicaConfigState::default()),
        config
    );
    assert_eq!(files.quick_cursor(), 5);
    files
        .store_configuration(&crate::MusicaConfigState::default())
        .unwrap();
    files.store_quick_cursor(6).unwrap();
    assert!(saves.join(legacy::CONFIGURATION).exists());
    assert!(saves.join(legacy::QUICK_CURSOR).exists());
    assert!(!root.path().join(".shinki/musica/minori").exists());
    assert!(
        !root
            .path()
            .join(".shinki/musica")
            .join(sysparams::MUSICA_SETTINGS)
            .exists()
    );
}
