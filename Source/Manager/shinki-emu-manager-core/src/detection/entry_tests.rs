use super::*;
use std::io::Write;

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn scene() -> Vec<u8> {
    let mut bytes = vec![0; 128];
    word(&mut bytes, 0, 92);
    for (at, value) in [
        (52, 92),
        (56, 1),
        (60, 100),
        (64, 1),
        (68, 104),
        (72, 1),
        (76, 112),
        (80, 1),
        (108, 16),
    ] {
        word(&mut bytes, at, value);
    }
    bytes
}
fn detect(path: &Path) -> DetectionResult<Vec<EngineDetection>> {
    detect_game_directory(path, &NativeCancellation::new(1))
}
#[test]
fn siglus_uses_first_valid_select_append_and_case_insensitive_components() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("Assets/Episode")).unwrap();
    fs::write(root.path().join("Assets/Episode/SCENE.CHS"), scene()).unwrap();
    fs::write(
        root.path().join("Select.ini"),
        "invalid\tthree\tcolumns\r\nassets\\episode\tAny display name\r\n",
    )
    .unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "siglus");
    fs::write(
        root.path().join("Select.ini"),
        "missing\tFirst\r\nAssets/Episode\tSecond\r\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("Data")).unwrap();
    fs::write(root.path().join("Data/Scene.pck"), scene()).unwrap();
    fs::write(root.path().join("Scene.pck"), scene()).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::create_dir_all(root.path().join("追加/章")).unwrap();
    fs::write(root.path().join("追加/章/Scene.pck"), scene()).unwrap();
    let text = shinki_emu_musica_format::ScriptEncoding::ShiftJis
        .encode("追加\\章\t表示名\r\n")
        .unwrap();
    fs::write(root.path().join("Select.ini"), text).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "siglus");
}
#[test]
fn siglus_append_cannot_escape_the_selected_directory() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("game");
    fs::create_dir(&root).unwrap();
    fs::create_dir(parent.path().join("outside")).unwrap();
    fs::write(parent.path().join("outside/Scene.pck"), scene()).unwrap();
    fs::write(root.join("Select.ini"), "../outside\tExternal\n").unwrap();
    assert!(detect(&root).unwrap().is_empty());
}
#[test]
fn softpal_requires_actual_sv20_inside_a_valid_pac_index() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = vec![0; 0x804 + 40 + 13];
    word(&mut bytes, 12 + b'S' as usize * 8 + 4, 1);
    bytes[0x804..0x804 + 11].copy_from_slice(b"SCRIPT.SRC\0");
    word(&mut bytes, 0x804 + 32, 13);
    word(&mut bytes, 0x804 + 36, 0x804 + 40);
    let start = 0x804 + 40;
    bytes[start..start + 4].copy_from_slice(b"Sv20");
    word(&mut bytes, start + 8, 12);
    fs::write(root.path().join("renamed.container"), &bytes).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "softpal");
    bytes[start] = b'X';
    fs::write(root.path().join("renamed.container"), &bytes).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    bytes[start] = b'S';
    bytes.truncate(start + 12);
    fs::write(root.path().join("renamed.container"), &bytes).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}
fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn legacy_rpyc_recognizes_serialized_ast_without_importing_or_constructing_it() {
    let root = tempfile::tempdir().unwrap();
    // Data-only pickle of (metadata, [a renpy.ast object]); GLOBAL/NEWOBJ/BUILD
    // are inspected as bytes, never evaluated by the detector.
    let pickle = b"\x80\x02}]crenpy.ast\nSay\n)\x81}ba\x86.";
    let protocol4 = b"\x80\x04}]\x8c\x09renpy.ast\x94\x8c\x03Say\x94\x93)\x81}ba\x86.";
    for data in [pickle.as_slice(), protocol4.as_slice()] {
        fs::write(root.path().join("renamed.rpyc"), zlib(data)).unwrap();
        assert_eq!(detect(root.path()).unwrap()[0].family_id, "renpy");
    }
    for unrelated in [
        b"not pickle".as_slice(),
        b"\x80\x02}.",
        b"\x80\x02U\x09renpy.ast.",
        b"\x80\x02crenpy.ast\nSay\n.",
        b"\x80\x02cother.ast\nSay\n)\x81}b.",
    ] {
        fs::write(root.path().join("renamed.rpyc"), zlib(unrelated)).unwrap();
        assert!(detect(root.path()).unwrap().is_empty());
    }
    for cut in 0..pickle.len() {
        fs::write(root.path().join("renamed.rpyc"), zlib(&pickle[..cut])).unwrap();
        assert!(detect(root.path()).unwrap().is_empty());
    }
}
#[test]
fn loose_tjs_requires_literal_bootstrap_and_engine_calls_in_actual_target() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("system")).unwrap();
    fs::write(
        root.path().join("startup.tjs"),
        "Scripts.execStorage(\"system/boot.tjs\");",
    )
    .unwrap();
    fs::write(
        root.path().join("system/boot.tjs"),
        "System.inform(\"public fixture\");",
    )
    .unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "krkr");
    fs::write(
        root.path().join("system/boot.tjs"),
        "/System.inform('regexp, not a call')/;",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::write(
        root.path().join("system/boot.tjs"),
        "// System.inform('not a call');\nvar text = 'System.inform(1)';",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::write(
        root.path().join("startup.tjs"),
        "// Scripts.execStorage(\"system/boot.tjs\");",
    )
    .unwrap();
    fs::write(
        root.path().join("system/boot.tjs"),
        "System.inform(\"public\");",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::write(
        root.path().join("startup.tjs"),
        "Scripts.execStorage(\"../outside.tjs\");",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}

#[test]
fn fake_rpa1_header_and_generic_rpi_data_pair_do_not_establish_renpy() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("data.rpa"), b"RPA-1.0 unrelated bytes").unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::write(
        root.path().join("data.rpi"),
        zlib(b"\x80\x02}U\x01x]K\x00K\x08\x86as."),
    )
    .unwrap();
    fs::write(root.path().join("data.rpa"), b"ordinary").unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}

#[test]
fn legacy_rpyc_decompression_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("large.rpyc"),
        zlib(&vec![0; MAX_FILE_READ_BYTES as usize + 1]),
    )
    .unwrap();
    assert!(matches!(
        detect(root.path()),
        Err(DetectionError::ReadLimit)
    ));
}

fn pac_resource(name: &[u8], payload: &[u8]) -> Vec<u8> {
    let start = 0x804 + 40;
    let mut bytes = vec![0; start + payload.len()];
    word(
        &mut bytes,
        12 + name[0].to_ascii_uppercase() as usize * 8 + 4,
        1,
    );
    bytes[0x804..0x804 + name.len()].copy_from_slice(name);
    word(&mut bytes, 0x804 + 32, payload.len() as u32);
    word(&mut bytes, 0x804 + 36, start as u32);
    bytes[start..].copy_from_slice(payload);
    bytes
}

#[test]
fn siglus_scene_routing_follows_magic_and_stops_before_pac_fallback() {
    let root = tempfile::tempdir().unwrap();
    let scene = root.path().join("Scene.pck");
    fs::write(root.path().join("Select.ini"), "missing\tAppend\n").unwrap();

    let mut script = [0; 13];
    script[..4].copy_from_slice(b"Sv20");
    word(&mut script, 8, 12);
    fs::write(&scene, script).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "softpal");

    fs::write(&scene, pac_resource(b"SCRIPT.SRC", &script)).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::rename(scene, root.path().join("script.pac")).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "softpal");
}

#[test]
fn softpal_follows_initial_archive_dat_profile_without_using_later_profiles() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("content/packs")).unwrap();
    let mut script = vec![0; 13];
    script[..4].copy_from_slice(b"Sv20");
    word(&mut script, 8, 12);
    fs::write(
        root.path().join("content/packs/scripts.pac"),
        pac_resource(b"SCRIPT.SRC", &script),
    )
    .unwrap();
    fs::write(
        root.path().join("data/archive.dat"),
        b" content\\packs\\scripts | missing , ignored\\later\0",
    )
    .unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "softpal");
    fs::write(
        root.path().join("data/archive.dat"),
        b"missing,content\\packs\\scripts\0",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::remove_file(root.path().join("data/archive.dat")).unwrap();
    fs::write(
        root.path().join("data.pac"),
        pac_resource(b"ARCHIVE.DAT", b"content\\packs\\scripts\0"),
    )
    .unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "softpal");
}
