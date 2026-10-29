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
