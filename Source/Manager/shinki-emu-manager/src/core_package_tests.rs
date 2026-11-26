use super::*;
use zip::write::SimpleFileOptions;

const CORE_STEM: &str = "shinki_emu_fixture";

fn library_name(stem: &str) -> String {
    format!("{stem}.{}", std::env::consts::DLL_EXTENSION)
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn manifest(plugin_id: &str, files: &[(String, &[u8])]) -> Vec<u8> {
    let mut files: Vec<CoreFile> = files
        .iter()
        .map(|(path, bytes)| CoreFile {
            path: path.clone(),
            sha256: sha256_hex(bytes),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&CoreManifest {
        schema: CORE_PACKAGE_SCHEMA.into(),
        plugin_id: plugin_id.into(),
        library: library_name(CORE_STEM),
        version: "0.2.0".into(),
        identity: None,
        recognition: Default::default(),
        abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files,
    })
    .unwrap()
}

#[test]
fn android_first_party_admission_requires_a_valid_hashed_product_commit() {
    const PLUGIN: &str = "shinki.emu.cmvs";
    const COMMIT: &[u8] = b"0123456789abcdef0123456789abcdef01234567";
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let pending = cores.join(PENDING);
    let archive = root.path().join("core.zip");
    let stage = || stage_open_package(
        fs::File::open(&archive).unwrap(), Some((PLUGIN, "0.2.0", None)), &pending, true, None,
    );

    valid_package(&archive, PLUGIN);
    assert!(stage().unwrap_err().starts_with("SHINKI_EMU_CORE_PACKAGE_RELEASE_UNSUPPORTED"));
    assert!(pending_is_empty(&cores));

    for payload in [b"0123456789ABCDEF0123456789ABCDEF01234567".as_slice(), &[b'a'; 41], COMMIT] {
        let mut files = valid_files();
        files.push(("source-commit.txt".into(), payload));
        let encoded = manifest(PLUGIN, &files);
        // For the valid-looking case, supply different bytes than the manifest lists.
        if payload == COMMIT {
            files.last_mut().unwrap().1 = &[b'a'; 40];
        }
        files.push((CORE_MANIFEST.into(), &encoded));
        write_zip(&archive, &files);
        assert!(stage().is_err());
        assert!(pending_is_empty(&cores));
    }

    let mut files = valid_files();
    let unstamped = manifest(PLUGIN, &files);
    files.push(("source-commit.txt".into(), COMMIT));
    files.push((CORE_MANIFEST.into(), &unstamped));
    write_zip(&archive, &files);
    assert!(stage_open_package(fs::File::open(&archive).unwrap(), None, &pending, false, None).is_err());
    files.pop();

    let forged = manifest("rfvp", &files);
    files.push((CORE_MANIFEST.into(), &forged));
    write_zip(&archive, &files);
    assert!(stage_open_package(fs::File::open(&archive).unwrap(), None, &pending, false, None).is_err());
    files.pop();

    let stamped = manifest(PLUGIN, &files);
    files.push((CORE_MANIFEST.into(), &stamped));
    write_zip(&archive, &files);
    let (stem, manifest) = stage().unwrap();
    assert_eq!(
        read_product_metadata(&pending.join(stem), manifest, true).unwrap().source_commit.as_deref(),
        Some(std::str::from_utf8(COMMIT).unwrap()),
    );
}

#[test]
fn invalid_pending_product_metadata_keeps_the_live_release() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let pending = cores.join(PENDING);
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    files.push(("source-commit.txt".into(), &[b'a'; 40]));
    let encoded = manifest("shinki.emu.cmvs", &files);
    files.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &files);
    let (stem, _) = stage_package(&archive, None, &pending).unwrap();
    assert!(apply_pending(&cores).is_empty());
    stage_package(&archive, None, &pending).unwrap();

    let live = cores.join(&stem);
    let staged = pending.join(&stem);
    fs::write(staged.join("source-commit.txt"), [b'b'; 40]).unwrap();
    assert_eq!(apply_pending(&cores).len(), 1);
    assert!(staged.is_dir());
    assert_eq!(read_package(&live).unwrap().source_commit.as_deref(), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));

    fs::remove_file(live.join("source-commit.txt")).unwrap();
    assert_eq!(read_package(&live).unwrap_err(), INVALID_PRODUCT_COMMIT);
}

fn write_zip(path: &Path, entries: &[(String, &[u8])]) {
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        writer.start_file(name.as_str(), options()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn valid_files() -> Vec<(String, &'static [u8])> {
    vec![
        (library_name(CORE_STEM), b"new library".as_slice()),
        (library_name("avcodec-62"), b"dependency"),
        ("licenses/LICENSE.txt".into(), b"license"),
    ]
}

fn valid_package(path: &Path, plugin_id: &str) {
    let files = valid_files();
    let manifest = manifest(plugin_id, &files);
    let mut entries = files;
    entries.push((CORE_MANIFEST.into(), &manifest));
    write_zip(path, &entries);
}

fn pending_is_empty(cores: &Path) -> bool {
    fs::read_dir(cores.join(PENDING)).map_or(true, |mut entries| entries.next().is_none())
}

#[test]
fn signed_framework_sidecar_matches_the_selected_executable_and_product_hash() {
    let root = tempfile::tempdir().unwrap();
    let id = "shinki.emu.musica";
    let manifest_path = root.path().join(CORE_MANIFEST);
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let commit_path = root.path().join("source-commit.txt");
    fs::write(&commit_path, commit).unwrap();
    let mut sidecar: CoreManifest = serde_json::from_slice(&manifest(id, &[
        (id.into(), b"digest"),
        ("source-commit.txt".into(), commit.as_bytes()),
    ])).unwrap();
    sidecar.library = id.into();
    let write = |sidecar: &CoreManifest| {
        fs::write(&manifest_path, serde_json::to_vec(sidecar).unwrap()).unwrap();
    };
    write(&sidecar);
    let bundle = read_bundled_package(root.path(), id).unwrap();
    assert_eq!(bundle.manifest, sidecar);
    assert_eq!(bundle.source_commit.as_deref(), Some(commit));
    assert!(read_manifest(root.path()).is_none());
    let executable = root.path().join(id);
    fs::write(&executable, b"digest").unwrap();
    verify_bundled_executable(&bundle.manifest, &executable).unwrap();
    fs::write(&executable, b"changed").unwrap();
    assert!(verify_bundled_executable(&bundle.manifest, &executable).is_err());

    fs::remove_file(&commit_path).unwrap();
    assert!(read_bundled_package(root.path(), id).is_none());

    fs::write(&commit_path, [b'a'; 40]).unwrap();
    assert!(read_bundled_package(root.path(), id).is_none());
    fs::write(&commit_path, commit).unwrap();

    sidecar.plugin_id = "shinki.emu.foreign".into();
    write(&sidecar);
    assert!(read_bundled_package(root.path(), id).is_none());

    sidecar.plugin_id = id.into();
    sidecar.files[0].path = id.to_ascii_uppercase();
    write(&sidecar);
    assert!(read_bundled_package(root.path(), id).is_none());
}

#[test]
fn package_with_parent_directory_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    files.push(("../evil.dll".into(), b"escape"));
    let manifest = manifest("shinki.emu.fixture", &valid_files());
    files.push((CORE_MANIFEST.into(), &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap_err();
    assert_eq!(error, "SHINKI_EMU_CORE_PACKAGE_PATH");
    assert!(!root.path().join("evil.dll").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn package_with_absolute_symlink_or_duplicate_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let pending = root.path().join("cores").join(PENDING);
    let archive = root.path().join("core.zip");
    write_zip(&archive, &[("/abs.dll".into(), b"x")]);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PATH"
    );
    let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    writer
        .add_symlink("link.dll", "../../outside", options())
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PATH"
    );
    let mut files = valid_files();
    files.push((library_name("avcodec-62").to_ascii_uppercase(), b"other"));
    write_zip(&archive, &files);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_DUPLICATE"
    );
}

#[test]
fn package_with_hash_mismatch_or_foreign_plugin_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let manifest = manifest("shinki.emu.fixture", &valid_files());
    let mut files = valid_files();
    files[0].1 = b"tampered library";
    files.push((CORE_MANIFEST.into(), &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap_err();
    assert!(
        error.starts_with("SHINKI_EMU_CORE_PACKAGE_HASH_MISMATCH"),
        "{error}"
    );
    assert!(pending_is_empty(&cores));

    valid_package(&archive, "shinki.emu.fixture");
    assert_eq!(
        stage_package(&archive, Some(("shinki.emu.other", "0.2.0", None)), &cores.join(PENDING)).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn malformed_recognition_is_rejected_before_pending_or_installed_discovery() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    let mut invalid: serde_json::Value = serde_json::from_slice(&manifest("rfvp", &files)).unwrap();
    invalid["recognition"] = serde_json::json!([{"path":"game.bin", "magic":"0405", "offset":4095}]);
    let encoded = serde_json::to_vec(&invalid).unwrap();
    files.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &files);
    assert_eq!(
        stage_package(&archive, Some(("rfvp", "0.2.0", None)), &cores.join(PENDING)).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_MANIFEST"
    );
    assert!(pending_is_empty(&cores));
    fs::write(root.path().join(CORE_MANIFEST), encoded).unwrap();
    assert!(read_manifest(root.path()).is_none());
}

#[test]
fn valid_package_is_staged_and_apply_pending_replaces_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join(CORE_STEM);
    let library = library_name(CORE_STEM);
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(&library), b"old library").unwrap();
    let stale_dependency = library_name("stale-dependency");
    fs::write(installed.join(&stale_dependency), b"old").unwrap();
    let archive = root.path().join("core.zip");
    valid_package(&archive, "shinki.emu.fixture");

    let (stem, staged) =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap();
    assert_eq!(stem, CORE_STEM);
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([(stem.clone(), PendingKind::Install)])
    );
    assert_eq!(
        fs::read(cores.join(PENDING).join(&stem).join(&library)).unwrap(),
        b"new library"
    );

    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"new library");
    assert_eq!(
        fs::read(installed.join("licenses/LICENSE.txt")).unwrap(),
        b"license"
    );
    assert!(!installed.join(&stale_dependency).exists());
    assert_eq!(
        read_manifest(&installed).unwrap().plugin_id,
        "shinki.emu.fixture"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn removal_marker_deletes_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(cores.join("shinki_emu_y")).unwrap();
    fs::write(
        cores.join("shinki_emu_y").join(library_name("shinki_emu_y")),
        b"y",
    )
    .unwrap();
    mark_removal(&cores, "shinki_emu_y").unwrap();
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([("shinki_emu_y".to_owned(), PendingKind::Remove)])
    );
    assert!(apply_pending(&cores).is_empty());
    assert!(!cores.join("shinki_emu_y").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn legacy_top_level_library_is_not_admitted_on_android() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(&cores).unwrap();
    let library = library_name("shinki_emu_x");
    let bare = cores.join(&library);
    fs::write(&bare, b"legacy").unwrap();
    let dependency = cores.join(library_name("avcodec-62"));
    fs::write(&dependency, b"dependency").unwrap();
    assert!(apply_pending(&cores).is_empty());
    if cfg!(target_os = "android") {
        assert_eq!(fs::read(bare).unwrap(), b"legacy");
        assert!(!cores.join("shinki_emu_x").exists());
    } else {
        assert!(!bare.exists());
        assert_eq!(
            fs::read(cores.join("shinki_emu_x").join(&library)).unwrap(),
            b"legacy"
        );
        let manifest = read_manifest(&cores.join("shinki_emu_x")).unwrap();
        assert_eq!(manifest.library, library);
        assert_eq!(manifest.files[0].sha256, sha256_hex(b"legacy"));
    }
    assert_eq!(fs::read(dependency).unwrap(), b"dependency");
}

#[cfg(windows)]
#[test]
fn failed_install_restores_the_previous_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join(CORE_STEM);
    let library = library_name(CORE_STEM);
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(&library), b"installed").unwrap();
    let staged = cores.join(PENDING).join(CORE_STEM);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join(&library), b"replacement").unwrap();
    fs::write(
        staged.join(CORE_MANIFEST),
        manifest("fixture", &[(library.clone(), b"replacement")]),
    )
    .unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(staged.join(&library))
        .unwrap();
    assert_eq!(apply_pending(&cores).len(), 1);
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"installed");
    drop(locked);
    assert_eq!(fs::read(staged.join(&library)).unwrap(), b"replacement");
    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"replacement");
    assert!(pending_is_empty(&cores));
}

#[test]
fn installs_branch_identity_and_rejects_catalog_receipt_disagreement() {
    let identity = shinki_emu_metadata::CoreIdentity {
        upstream: shinki_emu_metadata::UpstreamIdentity {
            repo: "https://github.com/example/engine".into(),
            ref_kind: shinki_emu_metadata::UpstreamRefKind::Branch,
            ref_name: "release/next".into(), commit: "a".repeat(40),
        },
        adapter: shinki_emu_metadata::AdapterIdentity {
            repo: "https://github.com/Project-Shinkirou/engine".into(),
            branch: "shinki".into(), commit: "b".repeat(40),
        },
        release_sequence: 4,
    };
    let files = valid_files();
    let mut receipt: CoreManifest = serde_json::from_slice(&manifest("engine", &files)).unwrap();
    receipt.version = identity.version();
    receipt.identity = Some(identity.clone());
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("branch.zip");
    let encoded = serde_json::to_vec(&receipt).unwrap();
    let mut entries = files;
    entries.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &entries);
    let pending = root.path().join("pending");
    let (stem, accepted) = stage_package(&archive, Some(("engine", &receipt.version, Some(&identity))), &pending).unwrap();
    assert_eq!(accepted.identity, Some(identity.clone()));
    assert_eq!(read_manifest(&pending.join(&stem)).unwrap().version, receipt.version);
    let mut changed = identity.clone();
    changed.adapter.commit = "c".repeat(40);
    changed.release_sequence += 1;
    assert!(stage_package(&archive, Some(("engine", &changed.version(), Some(&changed))), &pending).is_err());
    assert_eq!(read_manifest(&pending.join(stem)).unwrap().identity, Some(identity));
}

#[cfg(not(target_os = "android"))]
#[test]
fn published_first_party_v1_desktop_shape_remains_installable() {
    // Published Musica entry before product provenance was part of Manager admission.
    let published: shinki_emu_metadata::CoreCatalogEntry = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../Metadata/engines/cores/shinki.emu.musica/windows-x86_64/0.1.0-beta.4.json",
    ))).unwrap();
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("legacy.zip");
    let mut files = valid_files();
    files.push(("build.json".into(), br#"{"source_commit":"legacy build receipt"}"#));
    let mut old: CoreManifest = serde_json::from_slice(&manifest(&published.plugin_id, &files)).unwrap();
    old.version = published.version.clone();
    let encoded = serde_json::to_vec(&old).unwrap();
    files.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &files);
    let (stem, _) = stage_package(
        &archive,
        Some((&published.plugin_id, &published.version, published.identity.as_ref())),
        &root.path().join(PENDING),
    ).unwrap();
    assert!(apply_pending(root.path()).is_empty());
    let installed = read_package(&root.path().join(stem)).unwrap();
    assert_eq!(installed.manifest, old);
    assert!(installed.source_commit.is_none());
}

#[test]
fn android_qa_receipt_only_admits_matching_local_zip() {
    use crate::android_qa_receipt::{self, Receipt};

    const PLUGIN: &str = "shinki.emu.musica";
    const VERSION: &str = "0.1.0-qa.9";
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    const OTHER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const LIBRARY: &str = "libshinki_emu_musica.so";
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("core.zip");
    let pending = root.path().join(PENDING);
    let write = |abi: u16, commit: &str| {
        let mut elf = [0_u8; 64];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&3_u16.to_le_bytes());
        elf[18..20].copy_from_slice(&abi.to_le_bytes());
        elf[20..24].copy_from_slice(&1_u32.to_le_bytes());
        let mut files = vec![
            (LIBRARY.into(), elf.as_slice()),
            (PRODUCT_SOURCE_COMMIT.into(), commit.as_bytes()),
            ("licenses/NOTICE".into(), b"test fixture".as_slice()),
        ];
        let mut declaration: CoreManifest = serde_json::from_slice(&manifest(PLUGIN, &files)).unwrap();
        declaration.library = LIBRARY.into();
        declaration.version = VERSION.into();
        let bytes = serde_json::to_vec(&declaration).unwrap();
        files.push((CORE_MANIFEST.into(), &bytes));
        write_zip(&archive, &files);
    };

    let receipt = |source_commit: &str, abi: &str, digest: Option<String>| {
        let receipt = Receipt {
            schema: android_qa_receipt::SCHEMA.into(),
            abi: abi.into(),
            product_version: VERSION.into(),
            source_commit: source_commit.into(),
            cores: vec![android_qa_receipt::Core {
                plugin_id: PLUGIN.into(), version: VERSION.into(), identity: None,
                size: fs::metadata(&archive).unwrap().len(),
                sha256: digest.unwrap_or_else(|| sha256_hex(&fs::read(&archive).unwrap())),
            }],
            omitted: Default::default(),
        };
        serde_json::to_vec(&receipt).unwrap()
    };
    let read = |bytes| Receipt::read_debug_asset(true, || Some(bytes), "arm64-v8a", VERSION, COMMIT);
    let install = |receipt: &Receipt| {
        stage_local_release(
            &archive,
            receipt.cores.iter().map(|entry| LocalRelease::android_qa(receipt, entry)),
            &pending,
        )
    };

    write(183, COMMIT);
    assert!(Receipt::read_debug_asset(false, || panic!("opened {} in a non-debug app", android_qa_receipt::ASSET),
        "arm64-v8a", VERSION, COMMIT).is_none());
    assert!(read(receipt(OTHER, "arm64-v8a", None)).is_none());
    assert!(read(receipt(COMMIT, "x86_64", None)).is_none());
    assert!(stage_local_release(&archive, std::iter::empty(), &pending).is_err());
    let wrong_hash = read(receipt(COMMIT, "arm64-v8a", Some("f".repeat(64)))).unwrap();
    assert_eq!(install(&wrong_hash).unwrap_err(), "SHINKI_EMU_CORE_NOT_AVAILABLE");

    write(62, COMMIT);
    assert_eq!(
        install(&read(receipt(COMMIT, "arm64-v8a", None)).unwrap()).unwrap_err(),
        "SHINKI_EMU_CORE_UNSUPPORTED_TARGET"
    );
    write(183, OTHER);
    assert_eq!(install(&read(receipt(COMMIT, "arm64-v8a", None)).unwrap()).unwrap_err(), INVALID_PRODUCT_COMMIT);
    assert!(pending_is_empty(root.path()));

    write(183, COMMIT);
    let stem = install(&read(receipt(COMMIT, "arm64-v8a", None)).unwrap()).unwrap();
    let dir = pending.join(stem);
    assert_eq!(fs::read(dir.join(PRODUCT_SOURCE_COMMIT)).unwrap(), COMMIT.as_bytes());
    assert_eq!(fs::read(dir.join(LIBRARY)).unwrap()[18], 183);
}

#[cfg(not(target_os = "android"))]
#[test]
fn local_official_import_still_uses_its_catalog_hash() {
    let mut entry: shinki_emu_metadata::CoreCatalogEntry = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../Metadata/engines/cores/shinki.emu.musica/windows-x86_64/0.1.0-beta.4.json",
    ))).unwrap();
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("official.zip");
    let pending = root.path().join(PENDING);
    let files = valid_files();
    let mut declaration: CoreManifest = serde_json::from_slice(&manifest(&entry.plugin_id, &files)).unwrap();
    declaration.version = entry.version.clone();
    let encoded = serde_json::to_vec(&declaration).unwrap();
    let mut zip = files;
    zip.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &zip);
    let import = |entry| stage_local_release(&archive, std::iter::once(LocalRelease::official(entry)), &pending);
    entry.size = fs::metadata(&archive).unwrap().len();
    assert_eq!(import(&entry).unwrap_err(), "SHINKI_EMU_CORE_NOT_AVAILABLE");
    entry.sha256 = sha256_hex(&fs::read(&archive).unwrap());
    let stem = import(&entry).unwrap();
    assert_eq!(read_manifest(&pending.join(stem)).unwrap(), declaration);
}
