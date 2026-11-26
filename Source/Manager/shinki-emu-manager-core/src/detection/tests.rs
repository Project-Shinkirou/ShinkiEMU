use super::*;

fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn quad(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn hcb() -> Vec<u8> {
    let mut bytes = vec![0; 16];
    word(&mut bytes, 0, 16);
    bytes.extend(4u32.to_le_bytes());
    bytes.extend([0, 0, 0, 0, 8, 0, 2, b'T', 0]);
    bytes.extend(1u16.to_le_bytes());
    bytes.extend([0, 5, b'T', b'e', b's', b't', 0]);
    bytes.extend(0u16.to_le_bytes());
    bytes
}
fn ps2a() -> Vec<u8> {
    let mut bytes = vec![0; 50];
    bytes[..4].copy_from_slice(b"PS2A");
    word(&mut bytes, 4, 48);
    word(&mut bytes, 20, 2);
    word(&mut bytes, 40, 2);
    bytes
}
fn cpz(version: u8) -> Vec<u8> {
    let size = if version == b'3' {
        20
    } else if version == b'7' {
        72
    } else {
        64
    };
    let mut bytes = vec![0; size + 256];
    bytes[..4].copy_from_slice(&[b'C', b'P', b'Z', version]);
    if version == b'3' {
        word(&mut bytes, 4, 1 ^ 0x5e9c4f37);
        word(&mut bytes, 8, 32 ^ 0xf32aed17);
    } else {
        let old = version == b'5';
        word(&mut bytes, 4, 1 ^ if old { 0xfe3a53d9 } else { 0xfe3a53da });
        word(
            &mut bytes,
            8,
            32 ^ if old { 0x37f298e7 } else { 0x37f298e8 },
        );
        word(
            &mut bytes,
            12,
            32 ^ if old { 0x7a6f3a2c } else { 0x7a6f3a2d },
        );
        if version == b'7' {
            word(&mut bytes, 64, 24 ^ 0x65ef99f3);
        }
    }
    bytes
}
fn pfs(version: u8, name: &[u8]) -> Vec<u8> {
    let size = 4 + 4 + name.len() + 12;
    let base = 7 + size;
    let mut bytes = vec![0; base + 1];
    bytes[..3].copy_from_slice(&[b'p', b'f', version]);
    word(&mut bytes, 3, size as u32);
    word(&mut bytes, 7, 1);
    word(&mut bytes, 11, name.len() as u32);
    bytes[15..15 + name.len()].copy_from_slice(name);
    word(&mut bytes, 19 + name.len(), base as u32);
    word(&mut bytes, 23 + name.len(), 1);
    bytes
}
fn bgi(modern: bool, name: &[u8]) -> Vec<u8> {
    let (record, name_size) = if modern { (128, 96) } else { (32, 16) };
    let mut bytes = vec![0; 16 + record + 1];
    bytes[..12].copy_from_slice(if modern {
        b"BURIKO ARC20"
    } else {
        b"PackFile    "
    });
    word(&mut bytes, 12, 1);
    bytes[16..16 + name.len()].copy_from_slice(name);
    word(&mut bytes, 16 + name_size + 4, 1);
    bytes
}
fn xp3() -> Vec<u8> {
    let mut bytes = b"XP3\r\n \n\x1a\x8b\x67\x01".to_vec();
    bytes.extend(19u64.to_le_bytes());
    bytes.push(0);
    bytes.extend(1u64.to_le_bytes());
    bytes.push(0);
    bytes
}
fn mpk() -> Vec<u8> {
    let mut bytes = vec![0; 321];
    bytes[..8].copy_from_slice(b"MPK\0\0\0\x02\0");
    word(&mut bytes, 8, 1);
    quad(&mut bytes, 72, 320);
    quad(&mut bytes, 80, 1);
    quad(&mut bytes, 88, 1);
    bytes[96..98].copy_from_slice(b"a\0");
    bytes
}
fn rpa() -> Vec<u8> {
    let mut bytes = b"RPA-3.0 0000000000000022 00000000\n".to_vec();
    bytes.extend([0x78, 0x9c]);
    bytes
}
fn rpyc() -> Vec<u8> {
    let mut bytes = vec![0; 35];
    bytes[..10].copy_from_slice(b"RENPY RPC2");
    word(&mut bytes, 10, 1);
    word(&mut bytes, 14, 34);
    word(&mut bytes, 18, 1);
    bytes
}
fn siglus(signed: bool) -> Vec<u8> {
    let mut bytes = vec![0; 128];
    word(&mut bytes, 0, 92);
    for (offset, value) in [
        (52, 92),
        (56, 1),
        (60, 100),
        (64, 1),
        (68, 104),
        (72, 1),
        (76, 112),
        (80, 1),
    ] {
        word(&mut bytes, offset, value);
    }
    word(&mut bytes, 108, 16);
    if signed {
        for offset in [52, 60, 68, 76] {
            let value = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            word(&mut bytes, offset, value + 8);
        }
        [b"pack_scn".as_slice(), &bytes].concat()
    } else {
        bytes
    }
}
fn softpal() -> Vec<u8> {
    let mut bytes = vec![0; 13];
    bytes[..4].copy_from_slice(b"Sv20");
    word(&mut bytes, 8, 12);
    bytes
}
fn cases() -> Vec<(&'static str, &'static str, Vec<u8>)> {
    vec![
        ("changed.hcb", "fvp", hcb()),
        ("Scene.pck", "siglus", siglus(false)),
        ("renamed.bin", "siglus", siglus(true)),
        ("arbitrary.bin", "artemis", pfs(b'8', b"system.ini\0")),
        ("arbitrary.bin", "softpal", softpal()),
        ("arbitrary.bin", "krkr", xp3()),
        ("arbitrary.bin", "ethornell", bgi(true, b"a\0")),
        ("arbitrary.bin", "ethornell", bgi(false, b"ipl._bp\0")),
        ("arbitrary.bin", "cmvs", ps2a()),
        ("arbitrary.bin", "mages", mpk()),
        ("arbitrary.bin", "renpy", rpa()),
        ("arbitrary.bin", "renpy", rpyc()),
    ]
}
fn detect(root: &Path) -> DetectionResult<Vec<EngineDetection>> {
    detect_game_directory(root, &NativeCancellation::new(1))
}
#[test]
fn recognizes_only_an_adv32_root_with_a_structural_bootstrap() {
    let collection = tempfile::tempdir().unwrap();
    let game = collection.path().join("Arbitrary Folder");
    let mes = game.join("aDvDaTa/mEs");
    fs::create_dir_all(&mes).unwrap();
    // A correctly named file with no MES structure is not a match.
    let startup = mes.join("StArT.MeS");
    fs::write(&startup, b"unrelated bytes").unwrap();
    assert!(detect(&game).unwrap().is_empty());

    let mut bytes = vec![0; 25];
    word(&mut bytes, 0, 2);
    word(&mut bytes, 4, 3);
    word(&mut bytes, 8, 6 | (1 << 31));
    bytes[16..18].copy_from_slice(&0x9f9au16.to_le_bytes());
    // Two three-byte instructions, one at each entrypoint.
    fs::write(game.join("start.mes"), &bytes).unwrap();
    fs::remove_file(&startup).unwrap();
    assert!(detect(&game).unwrap().is_empty());
    fs::write(&startup, &bytes).unwrap();
    let (matches, has_files) = DeclaredRules::default()
        .inspect_game_directory(&game, &NativeCancellation::new(1))
        .unwrap();
    assert!(has_files);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].family_id, "adv32");
    assert_eq!(matches[0].format, "adv32.mes");
    assert!(detect(collection.path()).unwrap().is_empty());

    word(&mut bytes, 8, 9 | (1 << 31));
    fs::write(&startup, bytes).unwrap();
    assert!(detect(&game).unwrap().is_empty());
}

#[test]
fn recognizes_format_structures_without_executables_or_plugins() {
    for (name, family, bytes) in cases() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(name), bytes).unwrap();
        let found = detect(root.path()).unwrap();
        assert_eq!(found.len(), 1, "{family}");
        assert_eq!(found[0].family_id, family);
    }
}

#[test]
fn reads_shared_game_children_through_a_seekable_read_only_tree() {
    struct SelectedFolder(PathBuf);
    impl DetectionTree for SelectedFolder {
        fn entries(
            &self,
            path: &Path,
            limit: usize,
            visit: &mut dyn FnMut(OsString, EntryKind) -> DetectionResult<()>,
        ) -> DetectionResult<()> {
            LocalTree.entries(&self.0.join(path), limit, visit)
        }

        fn kind(&self, path: &Path) -> io::Result<EntryKind> {
            LocalTree.kind(&self.0.join(path))
        }

        fn open(&self, path: &Path) -> io::Result<Option<(File, u64)>> {
            // A document provider can transfer a read-only file handle even
            // when the client has no native path to the selected folder.
            LocalTree.open(&self.0.join(path))
        }
    }

    let source = tempfile::tempdir().unwrap();
    let collection = source.path().join("Games");
    for index in 0..8 {
        fs::create_dir_all(collection.join(index.to_string())).unwrap();
    }
    let hcb_file = collection.join("1/boot.hcb");
    let ps2a_file = collection.join("3/data.bin");
    fs::write(&hcb_file, hcb()).unwrap();
    fs::write(&ps2a_file, ps2a()).unwrap();
    let cancellation = NativeCancellation::new(1);
    let scan = || {
        let mut found = Vec::new();
        let tree = SelectedFolder(collection.clone());
        assert!(detect_game_tree(&tree, &cancellation).unwrap().is_empty());
        tree.entries(Path::new(""), MAX_ENTRIES as usize, &mut |name, kind| {
            assert!(kind == EntryKind::Directory);
            let child = SelectedFolder(collection.join(name));
            let from_provider = detect_game_tree(&child, &cancellation)?;
            assert_eq!(
                from_provider,
                detect_game_directory(&child.0, &cancellation)?
            );
            found.extend(from_provider.into_iter().map(|engine| engine.family_id));
            Ok(())
        })
        .unwrap();
        found.sort();
        found
    };
    assert_eq!(scan(), ["cmvs", "fvp"]);
    assert_eq!(scan(), ["cmvs", "fvp"]);
    assert_eq!(fs::read(&hcb_file).unwrap(), hcb());
    assert_eq!(fs::read(&ps2a_file).unwrap(), ps2a());

    let custom = collection.join("4/Assets");
    fs::create_dir(&custom).unwrap();
    let header = custom.join("engine.id");
    let tree = SelectedFolder(collection.join("4"));
    let mut rules = DeclaredRules::new("fixture".into());
    let core = crate::core_catalog::lookup("rfvp").unwrap();
    rules.add(core, "assets/engine.id", 4094, vec![0xde, 0xad]);
    fs::write(&header, vec![0; 4095]).unwrap();
    assert!(
        rules
            .inspect_game_tree(&tree, &cancellation)
            .unwrap()
            .0
            .is_empty()
    );
    let mut bytes = vec![0; 4096];
    bytes[4094..].copy_from_slice(&[0xde, 0xad]);
    fs::write(&header, &bytes).unwrap();
    let (matches, _) = rules.inspect_game_tree(&tree, &cancellation).unwrap();
    assert_eq!(
        matches,
        rules.detect_game_directory(&tree.0, &cancellation).unwrap()
    );
    assert_eq!(matches[0].declared.as_ref().unwrap().plugin_ids, ["rfvp"]);
    assert_eq!(fs::read(header).unwrap(), bytes);
}

#[test]
fn selected_append_under_default_lookahead_opens_the_scene_once() {
    struct CountedScene<'a> {
        root: &'a Path,
        opens: std::cell::Cell<usize>,
    }
    impl DetectionTree for CountedScene<'_> {
        fn entries(
            &self,
            path: &Path,
            limit: usize,
            visit: &mut dyn FnMut(OsString, EntryKind) -> DetectionResult<()>,
        ) -> DetectionResult<()> {
            LocalTree.entries(&self.root.join(path), limit, visit)
        }

        fn kind(&self, path: &Path) -> io::Result<EntryKind> {
            LocalTree.kind(&self.root.join(path))
        }

        fn open(&self, path: &Path) -> io::Result<Option<(File, u64)>> {
            if path == Path::new("DaTa/PaCk/Scene.pck") {
                self.opens.set(self.opens.get() + 1);
            }
            LocalTree.open(&self.root.join(path))
        }
    }

    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("DaTa/PaCk")).unwrap();
    fs::create_dir(root.path().join("DaTa/unrelated")).unwrap();
    fs::write(root.path().join("DaTa/PaCk/Scene.pck"), siglus(false)).unwrap();
    fs::write(root.path().join("DaTa/unrelated/game.hcb"), hcb()).unwrap();
    fs::write(root.path().join("Select.ini"), "data\\pack\tAppend\n").unwrap();
    let tree = CountedScene {
        root: root.path(),
        opens: std::cell::Cell::new(0),
    };
    let matches = detect_game_tree(&tree, &NativeCancellation::new(1)).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].family_id, "siglus");
    assert_eq!(tree.opens.get(), 1);
}

#[test]
fn root_files_win_over_a_custom_append_that_wins_over_default_lookahead() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("Assets")).unwrap();
    fs::create_dir(root.path().join("Data")).unwrap();
    fs::write(root.path().join("Select.ini"), "Assets\tAppend\n").unwrap();
    fs::write(root.path().join("Assets/engine.bin"), cpz(b'5')).unwrap();
    fs::write(root.path().join("Data/engine.bin"), cpz(b'6')).unwrap();

    // An append is inspected by the same file rules, including non-Siglus formats.
    assert_eq!(detect(root.path()).unwrap()[0].format, "cmvs.cpz5");
    fs::write(root.path().join("engine.bin"), ps2a()).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].format, "cmvs.ps2a");
}

#[test]
fn recognizes_cpz_versions_and_older_pfs_layouts_once_per_family() {
    let root = tempfile::tempdir().unwrap();
    for version in [b'3', b'5', b'6', b'7'] {
        fs::write(root.path().join(format!("{version}.bin")), cpz(version)).unwrap();
    }
    for version in [b'2', b'6', b'8'] {
        fs::write(
            root.path().join(format!("p{version}.bin")),
            pfs(version, b"system.ini\0"),
        )
        .unwrap();
    }
    assert_eq!(
        detect(root.path())
            .unwrap()
            .iter()
            .map(|m| m.family_id.as_str())
            .collect::<Vec<_>>(),
        ["artemis", "cmvs"]
    );
}

#[test]
fn bgi_archive_identifies_the_child_game_not_the_library_root() {
    for (layout, modern) in [("game", false), ("DATA", true)] {
        let library = tempfile::tempdir().unwrap();
        let game = library.path().join(layout);
        fs::create_dir(&game).unwrap();
        fs::write(library.path().join("System.arc"), b"unrelated bytes").unwrap();
        fs::write(game.join("System.arc"), bgi(modern, b"ipl._bp\0")).unwrap();

        assert!(detect(library.path()).unwrap().is_empty(), "{layout}");
        let matches = detect(&game).unwrap();
        assert_eq!(matches.len(), 1, "{layout}");
        assert_eq!(matches[0].family_id, "ethornell", "{layout}");
    }
}

#[test]
fn shared_extensions_names_and_profiles_never_establish_a_family() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "system.arc",
        "data.xp3",
        "startup.tjs",
        "Scene.pck",
        "Gameexe.dat",
        "game.hcb",
        "script.cpz",
        "scr.paz",
        "data.pac",
        "script.rpyc",
        "script.rpy",
        "eden.exe",
        "SiglusEngine.exe",
        "koikake.exe",
        "steam_appid.txt",
    ] {
        fs::write(root.path().join(name), b"unrelated bytes").unwrap();
    }
    // A copied private configuration is intent, not proof of its archive data.
    fs::write(
        root.path().join("key.toml"),
        b"schema = \"shinki.emu.musica.keys.v2\"\n",
    )
    .unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fs::write(root.path().join("system.arc"), bgi(false, b"ordinary\0")).unwrap();
    fs::write(root.path().join("engine.pfs"), pfs(b'8', b"ordinary\0")).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}
#[test]
fn all_truncated_formats_are_rejected_without_hiding_other_families() {
    for (name, family, bytes) in cases() {
        for length in 0..bytes.len() {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join(name), &bytes[..length]).unwrap();
            let result = detect(root.path()).unwrap();
            assert!(result.is_empty(), "{family}, truncated to {length}");
            fs::write(root.path().join("valid.xp3"), xp3()).unwrap();
            assert!(
                detect(root.path())
                    .unwrap()
                    .iter()
                    .any(|m| m.family_id == "krkr")
            );
        }
    }
}
#[test]
fn multi_family_layout_survives_directory_and_executable_renames() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("original");
    fs::create_dir_all(original.join("DaTa/PaCk")).unwrap();
    fs::write(original.join("DaTa/PaCk/start.ps3"), ps2a()).unwrap();
    fs::create_dir_all(original.join("game")).unwrap();
    fs::write(original.join("game/script.bin"), rpa()).unwrap();
    fs::write(original.join("changed.hcb"), hcb()).unwrap();
    fs::write(original.join("launch.exe"), b"not executable").unwrap();
    let expected = detect(&original).unwrap();
    let renamed = root.path().join("renamed");
    fs::rename(&original, &renamed).unwrap();
    fs::rename(renamed.join("launch.exe"), renamed.join("anything.exe")).unwrap();
    assert_eq!(detect(&renamed).unwrap(), expected);
    assert_eq!(
        expected
            .iter()
            .map(|m| m.family_id.as_str())
            .collect::<Vec<_>>(),
        ["cmvs", "fvp", "renpy"]
    );
}
#[test]
fn hcb_descriptor_allows_trailing_metadata_like_the_native_parser() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = hcb();
    bytes.extend(b"arbitrary converter metadata");
    fs::write(root.path().join("renamed.bch"), bytes).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "fvp");
}
#[test]
fn cancellation_stops_before_any_io_and_after_an_established_match() {
    let token = NativeCancellation::new(1);
    token.cancel();
    assert!(matches!(
        detect_game_directory(Path::new("does-not-exist"), &token),
        Err(DetectionError::Cancelled)
    ));
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, [0; 8]).unwrap();
    let mut source = Source {
        file: File::open(path).unwrap(),
        length: 8,
        read_bytes: 0,
    };
    let token = NativeCancellation::new(2);
    let mut scan = Scan {
        tree: &LocalTree,
        cancellation: &token,
        matches: Vec::new(),
        entries: 0,
        files: 0,
        read_bytes: 0,
    };
    scan.matched("cmvs", "cmvs.ps2a").unwrap();
    token.cancel();
    assert!(matches!(
        scan.read_at(&mut source, 4, &mut [0; 4]),
        Err(DetectionError::Cancelled)
    ));
    assert_eq!(source.file.stream_position().unwrap(), 0);
    assert_eq!(scan.read_bytes, 0);
    assert!(matches!(
        scan.matched("fvp", "fvp.hcb"),
        Err(DetectionError::Cancelled)
    ));
    assert_eq!(scan.matches.len(), 1);
}
#[test]
fn read_limits_are_errors_and_do_not_allocate_declared_indexes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.pfs");
    let mut bytes = pfs(b'8', b"system.ini\0");
    word(&mut bytes, 3, MAX_FILE_READ_BYTES as u32);
    fs::write(&path, bytes).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_FILE_READ_BYTES + 7)
        .unwrap();
    assert!(matches!(
        detect(root.path()),
        Err(DetectionError::ReadLimit)
    ));
    fs::remove_file(path).unwrap();
    for i in 0..=MAX_FILES {
        fs::write(root.path().join(format!("{i}.bin")), []).unwrap();
    }
    assert!(matches!(
        detect(root.path()),
        Err(DetectionError::ReadLimit)
    ));
}
#[test]
fn actual_short_read_is_an_io_error_not_a_format_miss() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("resource");
    fs::write(&path, []).unwrap();
    let token = NativeCancellation::new(1);
    let mut scan = Scan {
        tree: &LocalTree,
        cancellation: &token,
        matches: Vec::new(),
        entries: 0,
        files: 0,
        read_bytes: 0,
    };
    let mut source = Source {
        file: File::open(path).unwrap(),
        length: 4,
        read_bytes: 0,
    };
    assert!(
        matches!(scan.read_at(&mut source, 0, &mut [0; 4]), Err(DetectionError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof)
    );
}
#[test]
fn inaccessible_root_is_not_unknown() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        matches!(detect(&root.path().join("missing")), Err(DetectionError::Io(error)) if error.kind() == io::ErrorKind::NotFound)
    );
}
#[cfg(windows)]
#[test]
fn unreadable_resource_is_not_silently_skipped() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("locked.arc");
    fs::write(&path, bgi(true, b"a\0")).unwrap();
    let _locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap();
    assert!(matches!(detect(root.path()), Err(DetectionError::Io(_))));
}
#[test]
fn detector_does_not_touch_even_an_available_native_implementation() {
    let fixture = crate::test_native::Fixture::new("detector-sentinel", "cmvs", "cmvs.ps2a");
    let calls = fixture.calls.clone();
    let _loaded = crate::test_native::install(0, fixture);
    calls.lock().clear();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("resource.bin"), ps2a()).unwrap();
    fs::write(root.path().join("poison-core.dll"), b"not a native library").unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "cmvs");
    assert!(calls.lock().is_empty());
}
