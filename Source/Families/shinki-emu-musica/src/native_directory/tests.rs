use super::*;
use std::time::{Duration, SystemTime};

fn legacy_flags() -> Vec<String> {
    ["TOHKA_CLEAR", "AYAME_CLEAR", "SUI_CLEAR", "REN_CLEAR"]
        .map(str::to_owned)
        .into()
}

/// Synthetic release descriptor; production never branches on these names.
pub(crate) fn identity_fixture(root: &Path, extended: bool) -> SaveIdentity {
    let identity = SaveIdentity {
        version: 1,
        directory: if extended {
            ".shinki/musica/minori/夏空のペルセウス"
        } else {
            ".shinki/musica/minori/eden (English edition)"
        }
        .into(),
        slot_prefix: if extended { "perseus_" } else { "eden" }.into(),
        append_slot_prefix: (!extended).then(|| "edenPM".into()),
        slot_digits: 4,
        signature: if extended {
            "perseus 1.00"
        } else {
            "eden_en 1.00"
        }
        .into(),
        system_format: if extended {
            minori_data::SystemFormat::Blowfish
        } else {
            minori_data::SystemFormat::Rc4
        },
        extended,
        serial_key: None,
        prefix: Some(0),
        auto_save_cycle: if extended { 400 } else { 200 },
        voice_keys: extended.then(|| ["ren", "sui", "aya", "tou", "etc"].map(str::to_owned)),
    };
    std::fs::create_dir_all(root.join(".shinki/musica")).unwrap();
    std::fs::write(
        root.join(".shinki/musica").join(IDENTITY),
        serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    identity
}

fn game(executable: Option<&str>) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    if let Some(executable) = executable {
        std::fs::write(root.path().join(executable), b"").unwrap();
        identity_fixture(root.path(), executable == "perseus.exe");
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
fn globals_round_trip_through_each_envelope_and_keep_foreign_entries() {
    for extended in [false, true] {
        let root = game(Some(if extended {
            "perseus.exe"
        } else {
            "eden_en.exe"
        }));
        let identity = identity_fixture(root.path(), extended);
        let mut files = open(root.path());
        let dir = &files.dir;
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
            .join(if extended {
                "夏空のペルセウス"
            } else {
                "eden (English edition)"
            })
            .join(SYSTEM);
        let stored =
            minori_data::decode_system(&std::fs::read(&path).unwrap(), identity.system_format)
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
    let dir = &files.dir;
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
    files
        .import_legacy_progress(root.path(), id, &legacy_flags())
        .unwrap();
    assert_eq!(files.globals(), globals(&[("REN_CLEAR", 1)]));
    assert!(!saves.join(legacy::PROGRESS).exists());
    let imported = saves.join("global-progress.json.imported");
    assert!(imported.exists());
    let mut again = open(root.path());
    again
        .import_legacy_progress(root.path(), id, &legacy_flags())
        .unwrap();
    assert_eq!(again.globals(), globals(&[("REN_CLEAR", 1)]));
    assert!(imported.exists());

    // A native system.dat that already exists wins; the JSON stays untouched.
    let second = progress_json(id, &["SUI_CLEAR"]);
    std::fs::write(saves.join(legacy::PROGRESS), &second).unwrap();
    let mut kept = open(root.path());
    kept.import_legacy_progress(root.path(), id, &legacy_flags())
        .unwrap();
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
                .import_legacy_progress(root.path(), Hash256::from_sha256(b"game"), &legacy_flags())
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
fn unknown_executable_imports_legacy_progress_and_persists_changes() {
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
    assert_eq!(
        files.dir.root,
        root.path().join(".shinki/musica/generic-v1")
    );
    files
        .import_legacy_progress(root.path(), id, &legacy_flags())
        .unwrap();
    assert_eq!(files.globals(), globals(&[("TOHKA_CLEAR", 1)]));
    files
        .store_globals(&globals(&[("TOHKA_CLEAR", 1), ("x", 2)]))
        .unwrap();
    files.record_cg("black.png").unwrap();
    assert!(!saves.join(legacy::PROGRESS).exists());
    let reopened = open(root.path());
    assert_eq!(reopened.globals(), globals(&[("TOHKA_CLEAR", 1), ("x", 2)]));
    assert!(reopened.cg.contains(b"black.png".as_slice()));
}

fn sysparams_path(root: &Path, extended: bool) -> PathBuf {
    root.join(".shinki/musica/minori")
        .join(if extended {
            "夏空のペルセウス"
        } else {
            "eden (English edition)"
        })
        .join(sysparams::SYSPARAMS)
}

fn legacy_json(schema: &str, game: Hash256, field: &str, value: serde_json::Value) -> Vec<u8> {
    let mut object = serde_json::json!({ "schema": schema, "game": game });
    object[field] = value;
    serde_json::to_vec(&object).unwrap()
}

#[test]
fn configuration_round_trips_through_sysparams_and_keeps_native_keys() {
    for extended in [false, true] {
        let root = game(Some(if extended {
            "perseus.exe"
        } else {
            "eden_en.exe"
        }));
        let path = sysparams_path(root.path(), extended);
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
        config.character_voices = ["ren", "sui", "aya", "tou", "mot"]
            .into_iter()
            .zip([true, false, true, true, false])
            .map(
                |(prefix, enabled)| crate::voice_preferences::VoiceCharacter {
                    prefix: prefix.into(),
                    label: prefix.into(),
                    enabled,
                },
            )
            .collect();
        files.store_configuration(&config).unwrap();
        files.store_quick_cursor(4).unwrap();
        files.store_auto_cursor(6).unwrap();

        let reopened = open(root.path());
        assert_eq!(reopened.configuration(config.clone()), config);
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
        match extended {
            true => {
                assert_eq!(stored[b"sui".as_slice()], b"0");
                assert_eq!(stored[b"etc".as_slice()], b"0");
                assert!(!musica.exists());
            }
            false => {
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
    let path = sysparams_path(root.path(), true);
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
    let config = crate::MusicaConfigState {
        bgm_volume: 12,
        ..Default::default()
    };
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
    let path = sysparams_path(root.path(), true);
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
    assert!(!sysparams_path(root.path(), true).exists());
}

#[test]
fn unknown_executable_imports_legacy_settings_and_persists_changes() {
    let id = Hash256::from_sha256(b"game");
    let root = game(None);
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    let config = crate::MusicaConfigState {
        animation: false,
        ..Default::default()
    };
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
    assert!(!saves.join(legacy::CONFIGURATION).exists());
    assert!(!saves.join(legacy::QUICK_CURSOR).exists());
    let reopened = open(root.path());
    assert_eq!(
        reopened.configuration(crate::MusicaConfigState::default()),
        crate::MusicaConfigState::default()
    );
    assert_eq!(reopened.quick_cursor(), 6);
}

fn message_save(extended: bool, prefix: u32) -> crate::eden_save::EdenSave {
    use crate::eden_save::{EdenEdition, EdenSave, EdenSaveEncoding};
    let source = b".set score = 17\n.stage front.png 0 0 back.png 0 0\n.panel 1\n.message 1  Speaker Body\n.end\n";
    let script = crate::parse_sc_with_encoding(
        source,
        &crate::ScOpcodeCatalog::observed_musica(),
        ScriptEncoding::ShiftJis,
    )
    .unwrap();
    let mut vm = crate::MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        0,
    )
    .unwrap();
    let edition = if extended {
        EdenEdition::Perseus
    } else {
        EdenEdition::English
    };
    vm.set_native_edition(edition);
    while vm.state().message.is_none() {
        vm.step(vm.state().fixed_tick + 1).unwrap();
    }
    EdenSave::from_checkpoint(
        &vm.native_checkpoint().unwrap(),
        edition,
        EdenSaveEncoding::ShiftJis,
        prefix,
        "fixture".into(),
    )
    .unwrap()
}

#[test]
fn source_files_establish_identity_and_preserve_original_formats_without_executables() {
    for extended in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".shinki/musica/minori/parsed-release");
        std::fs::create_dir_all(&directory).unwrap();
        let format = if extended {
            minori_data::SystemFormat::Blowfish
        } else {
            minori_data::SystemFormat::Rc4
        };
        let prefix = if extended { 0x1234_abcd } else { 0 };
        let save = message_save(extended, prefix);
        let stem = if extended { "perseus_" } else { "eden" };
        std::fs::write(
            directory.join(SYSTEM),
            minori_data::encode_system(
                &globals(&[("score", 1)])
                    .into_iter()
                    .map(|(key, value)| (key.into_bytes(), value.to_string().into_bytes()))
                    .collect(),
                format,
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.join(format!("{stem}0020.sav")),
            save.encode().unwrap(),
        )
        .unwrap();
        if !extended {
            std::fs::write(directory.join("edenPM0020.sav"), save.encode().unwrap()).unwrap();
        }
        let mut files = open(root.path());
        assert_eq!(files.dir.root, directory);
        assert_eq!(files.uses_slot_banks().unwrap(), extended);
        assert!(
            files.slot(20).unwrap().unwrap().checkpoint().unwrap() == save.checkpoint().unwrap()
        );
        files.store_globals(&globals(&[("score", 19)])).unwrap();
        files.record_cg("scene.png").unwrap();
        let mut bits = [0; minori_data::READ_FLAG_BYTES];
        bits[4] = 0x40;
        files.store_read_flags("test", &bits).unwrap();
        files.write_slot(21, &save, b"thumbnail").unwrap();
        let reopened = open(root.path());
        assert_eq!(reopened.globals(), globals(&[("score", 19)]));
        assert_eq!(reopened.read_flags("test").unwrap(), Some(bits));
        assert!(
            reopened.slot(21).unwrap().unwrap().checkpoint().unwrap() == save.checkpoint().unwrap()
        );
        assert_eq!(
            minori_data::decode_system(&std::fs::read(directory.join(SYSTEM)).unwrap(), format)
                .unwrap()[b"score".as_slice()],
            b"19"
        );
        assert_eq!(
            minori_data::decode_cg_names(&std::fs::read(directory.join(CG_VIEWS)).unwrap(), prefix)
                .unwrap(),
            [b"scene.png".to_vec()]
        );
        let stored = std::fs::read(directory.join(format!("{stem}0021.sav"))).unwrap();
        assert!(
            crate::eden_save::EdenSave::decode(&stored, save.edition, save.encoding).unwrap()
                == save
        );
    }
}

#[test]
fn unknown_games_persist_all_state_and_slots_across_restart_without_cross_game_writes() {
    let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    for (index, root) in roots.iter().enumerate() {
        // A familiar EXE name carries no identity information by itself.
        std::fs::write(root.path().join("eden_en.exe"), b"not a format descriptor").unwrap();
        let mut files = open(root.path());
        files
            .store_globals(&globals(&[("route", index as i64 + 1)]))
            .unwrap();
        files.record_cg(&format!("scene{index}.png")).unwrap();
        let mut bits = [0; minori_data::READ_FLAG_BYTES];
        bits[index] = 0x80;
        files.store_read_flags("story", &bits).unwrap();
        let mut save = message_save(false, 0);
        save.comment = format!("game{index}");
        files.write_slot(20, &save, b"thumbnail").unwrap();
        let path = root.path().join(".shinki/musica/generic-v1/slot_0020.sav");
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes[4..].starts_with(b"shinki.musica.save.v1\0"));
    }
    for (index, root) in roots.iter().enumerate() {
        let files = open(root.path());
        assert_eq!(files.globals(), globals(&[("route", index as i64 + 1)]));
        assert_eq!(
            files.slot(20).unwrap().unwrap().comment,
            format!("game{index}")
        );
        assert_eq!(
            files.cg,
            BTreeSet::from([format!("scene{index}.png").into_bytes()])
        );
        let mut expected = [0; minori_data::READ_FLAG_BYTES];
        expected[index] = 0x80;
        assert_eq!(files.read_flags("story").unwrap(), Some(expected));
    }
}

#[test]
fn preset_supplies_arbitrary_native_identity_and_is_retained_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let mut identity = SaveIdentity::generic();
    identity.directory = "custom-original-save".into();
    identity.slot_prefix = "release-".into();
    identity.slot_digits = 3;
    identity.signature = "unlisted-engine-release 2.0".into();
    identity.extended = true;
    identity.system_format = minori_data::SystemFormat::Blowfish;
    identity.prefix = Some(7);
    let configured = serde_json::to_string(&identity).unwrap();
    let mut files =
        NativeFiles::open_configured(root.path(), ScriptEncoding::ShiftJis, &configured).unwrap();
    files.store_globals(&globals(&[("custom", 4)])).unwrap();
    files
        .write_slot(20, &message_save(true, 7), b"thumbnail")
        .unwrap();
    let files = open(root.path());
    assert!(files.uses_slot_banks().unwrap());
    assert_eq!(files.globals(), globals(&[("custom", 4)]));
    assert_eq!(files.slot(20).unwrap().unwrap().prefix, 7);
    assert!(
        root.path()
            .join("custom-original-save/release-020.sav")
            .exists()
    );
}

#[test]
fn incomplete_legacy_native_identity_requires_configuration_without_losing_data() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join(".shinki/musica/minori/previous-release");
    std::fs::create_dir_all(&directory).unwrap();
    let original = minori_data::encode_system(
        &Properties::from([(b"route".to_vec(), b"9".to_vec())]),
        minori_data::SystemFormat::Rc4,
    )
    .unwrap();
    std::fs::write(directory.join(SYSTEM), &original).unwrap();
    assert_eq!(
        NativeFiles::open(root.path(), ScriptEncoding::ShiftJis)
            .err()
            .unwrap()
            .code,
        "SHINKI_EMU_MUSICA_SAVE_CONFIG"
    );
    assert_eq!(std::fs::read(directory.join(SYSTEM)).unwrap(), original);
    assert!(!root.path().join(".shinki/musica/generic-v1").exists());
    let mut identity = SaveIdentity::generic();
    identity.directory = ".shinki/musica/minori/previous-release".into();
    identity.signature = "arbitrary-native 1.0".into();
    let files = NativeFiles::open_configured(
        root.path(),
        ScriptEncoding::ShiftJis,
        &serde_json::to_string(&identity).unwrap(),
    )
    .unwrap();
    assert_eq!(files.globals(), globals(&[("route", 9)]));
    assert_eq!(open(root.path()).globals(), globals(&[("route", 9)]));
}

#[test]
fn unmapped_legacy_hashes_keep_sources_and_complete_after_configuration() {
    let root = game(None);
    let id = Hash256::from_sha256(b"unknown-release");
    let saves = root.path().join(".shinki/musica/saves");
    std::fs::create_dir_all(&saves).unwrap();
    let bytes = progress_json(id, &["chapter_complete", "bonus_available"]);
    std::fs::write(saves.join(legacy::PROGRESS), &bytes).unwrap();
    let mut files = open(root.path());
    files.import_legacy_progress(root.path(), id, &[]).unwrap();
    assert!(files.globals().is_empty());
    assert_eq!(std::fs::read(saves.join(legacy::PROGRESS)).unwrap(), bytes);
    files
        .import_legacy_progress(root.path(), id, &["chapter_complete".into()])
        .unwrap();
    assert_eq!(files.globals(), globals(&[("chapter_complete", 1)]));
    let diagnostic: serde_json::Value =
        serde_json::from_slice(&std::fs::read(saves.join(legacy::UNMAPPED_PROGRESS)).unwrap())
            .unwrap();
    assert_eq!(
        diagnostic["game_directory"],
        root.path().to_string_lossy().as_ref()
    );
    assert_eq!(
        diagnostic["unresolved"],
        serde_json::json!([Hash256::from_sha256(b"bonus_available")])
    );
    assert_eq!(std::fs::read(saves.join(legacy::PROGRESS)).unwrap(), bytes);
    files
        .store_globals(&globals(&[("chapter_complete", 0), ("foreign", 63)]))
        .unwrap();
    let mut reopened = open(root.path());
    reopened
        .import_legacy_progress(
            root.path(),
            id,
            &["chapter_complete".into(), "bonus_available".into()],
        )
        .unwrap();
    assert_eq!(
        reopened.globals(),
        globals(&[
            ("chapter_complete", 0),
            ("bonus_available", 1),
            ("foreign", 63)
        ])
    );
    assert!(!saves.join(legacy::PROGRESS).exists());
    assert!(!saves.join(legacy::UNMAPPED_PROGRESS).exists());
    assert_eq!(
        std::fs::read(saves.join("global-progress.json.imported")).unwrap(),
        bytes
    );
}
