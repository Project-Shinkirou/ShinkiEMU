use super::*;
use zip::write::SimpleFileOptions;

const LIBRARY: &str = "shinki_emu_fixture.dll";

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn manifest(plugin_id: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut files: Vec<CoreFile> = files
        .iter()
        .map(|(path, bytes)| CoreFile {
            path: (*path).into(),
            sha256: sha256_hex(bytes),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&CoreManifest {
        schema: CORE_PACKAGE_SCHEMA.into(),
        plugin_id: plugin_id.into(),
        library: LIBRARY.into(),
        version: "0.2.0".into(),
        source_commit: None,
        identity: None,
        recognition: Default::default(),
        abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files,
    })
    .unwrap()
}

#[test]
fn first_party_source_commit_is_validated_from_core_json() {
    let plugin = "shinki.emu.cmvs";
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let bytes = manifest(plugin, &valid_files());
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let decode = |value: &serde_json::Value| serde_json::from_value::<CoreManifest>(value.clone()).unwrap();
    assert!(decode(&value).product_source_commit().is_none());
    value["source_commit"] = commit.into();
    let decoded = decode(&value);
    assert!(decoded.validate(Some(plugin)).is_ok());
    assert_eq!(decoded.product_source_commit(), Some(commit));
    value["source_commit"] = commit.to_ascii_uppercase().into();
    assert!(decode(&value).validate(Some(plugin)).is_err());
    value["source_commit"] = commit.into();
    value["version"] = "0.0.0-local".into();
    assert!(decode(&value).validate(Some(plugin)).is_err());
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        writer.start_file(*name, options()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn valid_files() -> Vec<(&'static str, &'static [u8])> {
    vec![
        (LIBRARY, b"new library".as_slice()),
        ("avcodec-62.dll", b"dependency"),
        ("licenses/LICENSE.txt", b"license"),
    ]
}

fn valid_package(path: &Path, plugin_id: &str) {
    let files = valid_files();
    let manifest = manifest(plugin_id, &files);
    let mut entries = files;
    entries.push((CORE_MANIFEST, &manifest));
    write_zip(path, &entries);
}

fn pending_is_empty(cores: &Path) -> bool {
    fs::read_dir(cores.join(PENDING)).map_or(true, |mut entries| entries.next().is_none())
}

#[test]
fn signed_framework_sidecar_requires_the_selected_id_and_exact_executable() {
    let root = tempfile::tempdir().unwrap();
    let id = "shinki.emu.fixture";
    let manifest_path = root.path().join(CORE_MANIFEST);
    let mut sidecar: CoreManifest = serde_json::from_slice(&manifest(id, &[(id, b"digest")])).unwrap();
    sidecar.library = id.into();
    let write = |sidecar: &CoreManifest| {
        fs::write(&manifest_path, serde_json::to_vec(sidecar).unwrap()).unwrap();
    };
    write(&sidecar);
    assert_eq!(read_bundled_manifest(root.path(), id), Some(sidecar.clone()));
    assert!(read_manifest(root.path()).is_none());

    sidecar.plugin_id = "shinki.emu.foreign".into();
    write(&sidecar);
    assert!(read_bundled_manifest(root.path(), id).is_none());

    sidecar.plugin_id = id.into();
    sidecar.files[0].path = id.to_ascii_uppercase();
    write(&sidecar);
    assert!(read_bundled_manifest(root.path(), id).is_none());
}

#[test]
fn package_with_parent_directory_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    files.push(("../evil.dll", b"escape"));
    let manifest = manifest("shinki.emu.fixture", &valid_files());
    files.push((CORE_MANIFEST, &manifest));
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
    write_zip(&archive, &[("/abs.dll", b"x")]);
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
    files.push(("AVCODEC-62.DLL", b"other"));
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
    files.push((CORE_MANIFEST, &manifest));
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
    files.push((CORE_MANIFEST, &encoded));
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
    let installed = cores.join("shinki_emu_fixture");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(LIBRARY), b"old library").unwrap();
    fs::write(installed.join("stale-dependency.dll"), b"old").unwrap();
    let archive = root.path().join("core.zip");
    valid_package(&archive, "shinki.emu.fixture");

    let (stem, staged) =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap();
    assert_eq!(stem, "shinki_emu_fixture");
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([(stem.clone(), PendingKind::Install)])
    );
    assert_eq!(
        fs::read(cores.join(PENDING).join(&stem).join(LIBRARY)).unwrap(),
        b"new library"
    );

    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"new library");
    assert_eq!(
        fs::read(installed.join("licenses/LICENSE.txt")).unwrap(),
        b"license"
    );
    assert!(!installed.join("stale-dependency.dll").exists());
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
    fs::write(cores.join("shinki_emu_y").join("shinki_emu_y.dll"), b"y").unwrap();
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
    let library = format!("shinki_emu_x.{}", std::env::consts::DLL_EXTENSION);
    let bare = cores.join(&library);
    fs::write(&bare, b"legacy").unwrap();
    let dependency = cores.join(format!("avcodec-62.{}", std::env::consts::DLL_EXTENSION));
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
    let installed = cores.join("shinki_emu_fixture");
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(LIBRARY), b"installed").unwrap();
    let staged = cores.join(PENDING).join("shinki_emu_fixture");
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join(LIBRARY), b"replacement").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(staged.join(LIBRARY))
        .unwrap();
    assert_eq!(apply_pending(&cores).len(), 1);
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"installed");
    drop(locked);
    assert_eq!(fs::read(staged.join(LIBRARY)).unwrap(), b"replacement");
    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(LIBRARY)).unwrap(), b"replacement");
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
    entries.push((CORE_MANIFEST, &encoded));
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

#[test]
fn published_beta_package_without_identity_remains_installable() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("legacy.zip");
    let files = valid_files();
    let mut receipt: CoreManifest = serde_json::from_slice(&manifest("engine", &files)).unwrap();
    receipt.version = "0.1.0-beta.3".into();
    let encoded = serde_json::to_vec(&receipt).unwrap();
    let mut entries = files;
    entries.push((CORE_MANIFEST, &encoded));
    write_zip(&archive, &entries);
    let (_, staged) = stage_package(&archive, Some(("engine", "0.1.0-beta.3", None)), &root.path().join(PENDING)).unwrap();
    assert_eq!(staged.version, "0.1.0-beta.3");
    assert!(staged.identity.is_none());
}
