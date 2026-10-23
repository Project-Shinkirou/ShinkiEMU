use crate::{binary, build::BuildResult, cores::Inventory, package::*};
use shinki_emu_build::{BuildAsset, CoreBuildManifest};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};
struct Fixture {
    _dir: tempfile::TempDir,
    library: PathBuf,
    dependency: PathBuf,
    target: String,
}
fn fixture() -> &'static Fixture {
    static F: LazyLock<Fixture> = LazyLock::new(|| {
        let dir = tempfile::tempdir().unwrap();
        let version = Command::new("rustc").arg("-vV").output().unwrap();
        assert!(version.status.success());
        let target = String::from_utf8(version.stdout)
            .unwrap()
            .lines()
            .find_map(|s| s.strip_prefix("host: "))
            .unwrap()
            .to_owned();
        let (prefix, suffix) = if target.contains("windows") {
            ("", "dll")
        } else if target.contains("apple") {
            ("lib", "dylib")
        } else {
            ("lib", "so")
        };
        let dep_source = dir.path().join("dependency.rs");
        fs::write(&dep_source,"#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{}}\n#[no_mangle] pub extern \"C\" fn dep_value()->u32 {42}\n").unwrap();
        let dependency = dir
            .path()
            .join(format!("{prefix}shinki_fixture_dep.{suffix}"));
        let mut cmd = Command::new("rustc");
        cmd.arg(&dep_source)
            .args([
                "--crate-name",
                "shinki_fixture_dep",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
                "-o",
            ])
            .arg(&dependency);
        if target.contains("windows") {
            cmd.args(["-C", "link-arg=/NOENTRY"]);
        }
        let status = cmd.status().unwrap();
        assert!(status.success());
        if target.contains("windows") {
            shinki_emu_build::generate_import_library(&dependency, dir.path(), &target).unwrap();
        }
        let source = dir.path().join("core.rs");
        fs::write(&source,"#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{}}\n#[link(name=\"shinki_fixture_dep\")] extern \"C\" {fn dep_value()->u32;}\n#[no_mangle] pub unsafe extern \"C\" fn shinki_emu_query()->u32 {dep_value()}\n").unwrap();
        let library = dir
            .path()
            .join(format!("{prefix}shinki_emu_fixture.{suffix}"));
        let mut cmd = Command::new("rustc");
        cmd.arg(source)
            .args(["--crate-type", "cdylib", "-C", "panic=abort"])
            .arg(format!("-Lnative={}", dir.path().display()))
            .arg("-o")
            .arg(&library);
        if target.contains("windows") {
            cmd.args(["-C", "link-arg=/NOENTRY"]);
        }
        let status = cmd.status().unwrap();
        assert!(status.success());
        Fixture {
            _dir: dir,
            library,
            dependency,
            target,
        }
    });
    &F
}
fn result(temp: &Path) -> BuildResult {
    let f = fixture();
    let license = temp.join("LICENSE");
    fs::write(&license, "Public synthetic test fixture, CC0-1.0\n").unwrap();
    BuildResult {
        component: "fixture".into(),
        target: f.target.clone(),
        profile: "release".into(),
        library: f.library.clone(),
        core_build: Some(CoreBuildManifest {
            schema: shinki_emu_build::SCHEMA.into(),
            plugin_id: "fixture".into(),
            target: f.target.clone(),
            profile: "release".into(),
            abi: shinki_emu_build::ABI.into(),
            artifact_kind: "rust-cdylib".into(),
            native_library: None,
            resources: vec![],
            licenses: vec![BuildAsset {
                source: license,
                dest: "licenses/fixture/LICENSE".into(),
            }],
        }),
        source_commit: "fixture".into(),
        source_dirty: false,
        toolchain: "rustc".into(),
        features: vec![],
        target_dir: temp.to_owned(),
    }
}
#[test]
fn paths_reject_traversal_ads_devices_and_collisions() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::write(&source, "original").unwrap();
    for bad in [
        "../escape",
        "/absolute",
        "x//y",
        "x/./y",
        "x:y",
        "x\\y",
        "CON.txt",
        "LPT1",
        "name.",
        "x/../../a",
    ] {
        assert!(portable(bad).is_err(), "{bad}");
    }
    let out = temp.path().join("out");
    assert!(atomic_directory(&out, |stage| copy_entries(
        &[
            (source.clone(), "File".into()),
            (source.clone(), "file".into())
        ],
        stage
    ))
    .is_err());
    assert!(!out.exists());
    assert_eq!(fs::read_to_string(source).unwrap(), "original");
}
#[test]
fn atomic_publication_preserves_existing_and_racing_outputs_and_cleans_staging() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("user"), "keep").unwrap();
    assert!(atomic_directory(&out, |_| panic!("existing output must not stage")).is_err());
    assert_eq!(fs::read_to_string(out.join("user")).unwrap(), "keep");
    let race = t.path().join("race");
    assert!(atomic_directory(&race, |stage| {
        fs::write(stage.join("ours"), "ours")?;
        fs::create_dir(&race)?;
        fs::write(race.join("theirs"), "theirs")?;
        Ok(())
    })
    .is_err());
    assert_eq!(fs::read_to_string(race.join("theirs")).unwrap(), "theirs");
    assert!(!race.join("ours").exists());
    let failed = t.path().join("failed");
    assert!(atomic_directory(&failed, |stage| {
        fs::write(stage.join("partial"), "partial")?;
        anyhow::bail!("intentional failure")
    })
    .is_err());
    assert!(!failed.exists());
    assert!(!fs::read_dir(t.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".shinki-stage-")));
}
#[test]
fn real_binary_dependency_arch_and_export_validation() {
    let f = fixture();
    let info = binary::inspect(&f.library, &f.target, true).unwrap();
    assert!(info
        .dependencies
        .iter()
        .any(|d| d.contains("shinki_fixture_dep")));
    assert!(binary::closure(&f.library, &[], &f.target).is_err());
    let closure = binary::closure(&f.library, &[f._dir.path().to_owned()], &f.target).unwrap();
    assert!(closure.contains(&f.dependency));
    assert!(binary::inspect(&f.dependency, &f.target, true).is_err());
    let wrong = if f.target.starts_with("x86_64") {
        f.target.replacen("x86_64", "aarch64", 1)
    } else {
        f.target.replacen("aarch64", "x86_64", 1)
    };
    assert!(binary::inspect(&f.library, &wrong, true).is_err());
}
#[cfg(windows)]
#[test]
fn delay_imports_participate_in_dependency_closure() {
    let f = fixture();
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("shinki_emu_delayed.dll");
    let status = Command::new("rustc")
        .arg(f._dir.path().join("core.rs"))
        .args([
            "--crate-type",
            "cdylib",
            "-C",
            "panic=abort",
            "-C",
            "link-arg=/NOENTRY",
            "-C",
            "link-arg=/DELAYLOAD:shinki_fixture_dep.dll",
            "-C",
            "link-arg=delayimp.lib",
            "-C",
            "link-arg=kernel32.lib",
        ])
        .arg(format!("-Lnative={}", f._dir.path().display()))
        .arg("-o")
        .arg(&library)
        .status()
        .unwrap();
    assert!(status.success());
    let bytes = fs::read(&library).unwrap();
    let file = object::read::pe::PeFile64::parse(bytes.as_slice()).unwrap();
    assert!(file
        .data_directories()
        .delay_load_import_table(file.data(), &file.section_table())
        .unwrap()
        .is_some());
    assert!(binary::closure(&library, &[], &f.target).is_err());
    assert!(
        binary::closure(&library, &[f._dir.path().to_owned()], &f.target)
            .unwrap()
            .contains(&f.dependency)
    );
}

#[test]
fn unsafe_zip_paths_cannot_escape_or_publish_partial_output() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("unsafe.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    zip.start_file("../outside", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"untrusted archive").unwrap();
    zip.finish().unwrap();
    let out = temp.path().join("out");
    assert!(extract_zip(&archive, &out).is_err());
    assert!(!out.exists());
    assert!(!temp.path().join("outside").exists());
}
#[test]
fn staged_disk_set_hashes_and_deterministic_archive_match() {
    let t = tempfile::tempdir().unwrap();
    let r = result(t.path());
    let f = fixture();
    let dir = stage_core(
        &r,
        "1.0.0",
        &t.path().join("out"),
        &[f._dir.path().to_owned()],
    )
    .unwrap();
    let m = verify_core(&dir).unwrap();
    let actual = tree(&dir).unwrap();
    assert_eq!(actual.len(), m.files.len() + 1);
    for record in &m.files {
        assert_eq!(sha256(&dir.join(&record.path)).unwrap(), record.sha256);
    }
    let one = t.path().join("one.zip");
    let two = t.path().join("two.zip");
    zip_entries(&actual, &one).unwrap();
    zip_entries(&actual, &two).unwrap();
    assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap());
    let original = fs::read(&one).unwrap();
    assert!(zip_entries(&actual, &one).is_err());
    assert_eq!(fs::read(&one).unwrap(), original);
    fs::write(dir.join(&m.library), "corrupt").unwrap();
    assert!(verify_core(&dir).is_err());
}
#[test]
fn unlisted_duplicate_and_missing_licenses_fail_without_output() {
    let t = tempfile::tempdir().unwrap();
    let f = fixture();
    let mut r = result(t.path());
    r.core_build.as_mut().unwrap().licenses.clear();
    assert!(stage_core(
        &r,
        "1.0.0",
        &t.path().join("no-license"),
        &[f._dir.path().to_owned()]
    )
    .is_err());
    assert!(!t.path().join("no-license").exists());
    let r = result(t.path());
    let dir = stage_core(
        &r,
        "1.0.0",
        &t.path().join("out"),
        &[f._dir.path().to_owned()],
    )
    .unwrap();
    let mut m = verify_core(&dir).unwrap();
    fs::write(dir.join("unlisted"), "hidden").unwrap();
    assert!(verify_core(&dir).is_err());
    fs::remove_file(dir.join("unlisted")).unwrap();
    let first = &m.files[0];
    m.files.push(FileRecord {
        path: first.path.to_ascii_uppercase(),
        sha256: first.sha256.clone(),
    });
    fs::write(dir.join("core.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(verify_core(&dir).is_err());
}
#[cfg(windows)]
#[test]
fn reparse_input_is_rejected() {
    use std::os::windows::fs::symlink_dir;
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("file"), "data").unwrap();
    let link = t.path().join("link");
    if symlink_dir(&real, &link).is_err() {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "junction creation is required to exercise reparse rejection"
        );
    }
    assert!(tree(&link).is_err());
    assert!(plain(&link.join("file")).is_err());
}
#[cfg(unix)]
#[test]
fn linked_parent_and_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("file"), "data").unwrap();
    let link = t.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(tree(&link).is_err());
    assert!(plain(&link.join("file")).is_err());
}
#[test]
fn matrix_covers_each_core_and_target_without_accepting_unknown_keys() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let inv = Inventory::load(root).unwrap();
    let matrix = inv.matrix().unwrap();
    for core in &inv.cores {
        for target in &inv.targets {
            assert!(matrix
                .include
                .iter()
                .any(|r| r.component == core.id && r.target == target.triple));
        }
    }
    assert!(inv.blocked("unknown", &inv.targets[0].triple).is_err());
    assert!(inv.blocked("manager", "unknown").is_err());
}

#[test]
fn corresponding_sources_bind_downloads_and_port_patches_to_installed_hashes() {
    use sha2::Digest;
    let temp = tempfile::tempdir().unwrap();
    let original = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut inv = Inventory::load(original).unwrap();
    inv.root = temp.path().to_owned();
    fs::create_dir_all(temp.path().join("build")).unwrap();
    fs::copy(
        original.join("build/dependencies.toml"),
        temp.path().join("build/dependencies.toml"),
    )
    .unwrap();
    let port = temp.path().join(".work/vcpkg/ports/fribidi");
    fs::create_dir_all(&port).unwrap();
    fs::write(
        port.join("portfile.cmake"),
        "public port with a local patch",
    )
    .unwrap();
    let downloads = temp.path().join(".work/vcpkg-downloads");
    fs::create_dir_all(&downloads).unwrap();
    let upstream = downloads.join("public-source.tar.gz");
    fs::write(&upstream, b"public synthetic upstream archive").unwrap();
    let share = temp
        .path()
        .join(".work/vcpkg-subtitle/installed/x64-windows-static-md-dynbidi/share/fribidi");
    fs::create_dir_all(&share).unwrap();
    let provenance = serde_json::json!({
        "packages":[{"SPDXID":"SPDXRef-resource-0","checksums":[{"algorithm":"SHA512","checksumValue":hex::encode(sha2::Sha512::digest(fs::read(&upstream).unwrap()))}]}],
        "files":[{"SPDXID":"SPDXRef-port-file-0","fileName":"./portfile.cmake","checksums":[{"algorithm":"SHA256","checksumValue":sha256(&port.join("portfile.cmake")).unwrap()}]}]
    });
    fs::write(
        share.join("vcpkg.spdx.json"),
        serde_json::to_vec(&provenance).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join("PUBLIC.txt"), "public source fixture").unwrap();
    assert!(Command::new("git")
        .arg("init")
        .arg(temp.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(temp.path())
        .args(["add", "PUBLIC.txt"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(temp.path())
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "public source"
        ])
        .status()
        .unwrap()
        .success());
    let out = temp.path().join("sources.zip");
    // Explicitly use the test installation, not a developer override.
    let target = "aarch64-linux-android";
    let android = temp
        .path()
        .join(".work/vcpkg-subtitle/installed/arm64-android-dynbidi/share/fribidi");
    fs::create_dir_all(&android).unwrap();
    fs::copy(
        share.join("vcpkg.spdx.json"),
        android.join("vcpkg.spdx.json"),
    )
    .unwrap();
    source_bundle(&inv, "manager", target, &out).unwrap();
    let original_hash = sha256(&out).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(&out).unwrap()).unwrap();
    let mut archive_bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut zip.by_name("downloads/public-source.tar.gz").unwrap(),
        &mut archive_bytes,
    )
    .unwrap();
    assert_eq!(archive_bytes, fs::read(&upstream).unwrap());
    drop(zip);
    fs::write(&upstream, b"modified archive with the same filename").unwrap();
    let failed = temp.path().join("wrong-source.zip");
    assert!(source_bundle(&inv, "manager", target, &failed).is_err());
    assert!(!failed.exists());
    assert_eq!(sha256(&out).unwrap(), original_hash);
    fs::write(port.join("portfile.cmake"), "changed port recipe").unwrap();
    assert!(source_bundle(&inv, "manager", target, &temp.path().join("wrong-port.zip")).is_err());
}

#[test]
fn native_packages_include_dynamically_loaded_modules_and_validate_resource_binaries() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture();
    let native = temp.path().join("native");
    let resources = temp.path().join("resources");
    fs::create_dir(&native).unwrap();
    fs::create_dir(&resources).unwrap();
    let library = native.join(fixture.library.file_name().unwrap());
    fs::copy(&fixture.library, &library).unwrap();
    fs::copy(
        &fixture.dependency,
        native.join(fixture.dependency.file_name().unwrap()),
    )
    .unwrap();
    let optional = fixture
        .dependency
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace("_dep", "_optional");
    fs::copy(&fixture.dependency, native.join(&optional)).unwrap();
    let extension = if cfg!(windows) {
        "extension.pyd"
    } else {
        "extension.so"
    };
    fs::copy(&fixture.dependency, resources.join(extension)).unwrap();
    let mut result = result(temp.path());
    result.library = library.clone();
    let manifest = result.core_build.as_mut().unwrap();
    manifest.artifact_kind = "native-shared".into();
    manifest.native_library = Some(library);
    manifest.resources.push(BuildAsset {
        source: resources.clone(),
        dest: "Resources".into(),
    });
    let output = temp.path().join("complete");
    let core = stage_core(&result, "1.0.0", &output, std::slice::from_ref(&native)).unwrap();
    let manifest = verify_core(&core).unwrap();
    assert!(manifest.files.iter().any(|file| file.path == optional));
    assert!(manifest
        .files
        .iter()
        .any(|file| file.path == format!("Resources/{extension}")));
    fs::write(resources.join(extension), b"invalid native extension").unwrap();
    let rejected = temp.path().join("rejected");
    assert!(stage_core(&result, "1.0.0", &rejected, &[native]).is_err());
    assert!(!rejected.exists());
}

#[test]
fn output_nested_in_resource_inputs_is_rejected_without_modifying_sources() {
    let temp = tempfile::tempdir().unwrap();
    let resources = temp.path().join("resources");
    fs::create_dir(&resources).unwrap();
    let source = resources.join("public.txt");
    fs::write(&source, b"original public resource").unwrap();
    let mut result = result(temp.path());
    result
        .core_build
        .as_mut()
        .unwrap()
        .resources
        .push(BuildAsset {
            source: resources.clone(),
            dest: "Resources".into(),
        });
    let output = resources.join("nested").join("output");
    let fixture = fixture();
    assert!(stage_core(&result, "1.0.0", &output, &[fixture._dir.path().to_owned()]).is_err());
    assert!(!resources.join("nested").exists());
    assert_eq!(fs::read(&source).unwrap(), b"original public resource");
}

// Public synthetic loader metadata, generated directly from the ELF64 format.
fn sectionless_elf(needed: &str, rpath: &str) -> Vec<u8> {
    let strings = format!("\0{needed}\0{rpath}\0").into_bytes();
    let size = 256 + strings.len();
    let mut bytes = vec![0u8; size];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&4u32.to_le_bytes());
    bytes[96..104].copy_from_slice(&(size as u64).to_le_bytes());
    bytes[104..112].copy_from_slice(&(size as u64).to_le_bytes());
    bytes[112..120].copy_from_slice(&4096u64.to_le_bytes());
    bytes[120..124].copy_from_slice(&2u32.to_le_bytes());
    bytes[124..128].copy_from_slice(&4u32.to_le_bytes());
    bytes[128..136].copy_from_slice(&176u64.to_le_bytes());
    bytes[136..144].copy_from_slice(&176u64.to_le_bytes());
    bytes[144..152].copy_from_slice(&176u64.to_le_bytes());
    bytes[152..160].copy_from_slice(&80u64.to_le_bytes());
    bytes[160..168].copy_from_slice(&80u64.to_le_bytes());
    bytes[168..176].copy_from_slice(&8u64.to_le_bytes());
    for (index, (tag, value)) in [
        (5u64, 256u64),
        (10, strings.len() as u64),
        (1, 1),
        (29, (needed.len() + 2) as u64),
        (0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 176 + index * 16;
        bytes[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&value.to_le_bytes());
    }
    bytes[256..].copy_from_slice(&strings);
    bytes
}

#[test]
fn elf_dynamic_segments_resolve_needed_and_runpath_without_section_headers() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("libsynthetic.so");
    fs::write(&library, sectionless_elf("libmissing.so", "$ORIGIN")).unwrap();
    let info = binary::inspect(&library, "x86_64-unknown-linux-gnu", false).unwrap();
    assert!(info.dependencies.contains("libmissing.so"));
    assert!(info.rpaths.iter().any(|path| path == "$ORIGIN"));
    assert!(binary::closure(&library, &[], "x86_64-unknown-linux-gnu").is_err());
    fs::write(
        &library,
        sectionless_elf("libmissing.so", "/developer/private"),
    )
    .unwrap();
    assert!(binary::inspect(&library, "x86_64-unknown-linux-gnu", false).is_err());
}

fn synthetic_macho(rpath: &str) -> Vec<u8> {
    let dependency = b"@rpath/libmissing.dylib\0";
    let library_size = (24 + dependency.len() + 7) & !7;
    let rpath_size = (12 + rpath.len() + 1 + 7) & !7;
    let mut bytes = vec![0u8; 32 + library_size + rpath_size];
    bytes[0..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x01000007u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&6u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&((library_size + rpath_size) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&0xcu32.to_le_bytes());
    bytes[36..40].copy_from_slice(&(library_size as u32).to_le_bytes());
    bytes[40..44].copy_from_slice(&24u32.to_le_bytes());
    bytes[56..56 + dependency.len()].copy_from_slice(dependency);
    let offset = 32 + library_size;
    bytes[offset..offset + 4].copy_from_slice(&0x8000001cu32.to_le_bytes());
    bytes[offset + 4..offset + 8].copy_from_slice(&(rpath_size as u32).to_le_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&12u32.to_le_bytes());
    bytes[offset + 12..offset + 12 + rpath.len()].copy_from_slice(rpath.as_bytes());
    bytes
}

#[test]
fn macho_load_commands_require_relocatable_rpaths_and_a_complete_closure() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("libsynthetic.dylib");
    fs::write(&library, synthetic_macho("@loader_path")).unwrap();
    let info = binary::inspect(&library, "x86_64-apple-darwin", false).unwrap();
    assert!(info.dependencies.contains("@rpath/libmissing.dylib"));
    assert!(info.rpaths.iter().any(|path| path == "@loader_path"));
    assert!(binary::closure(&library, &[], "x86_64-apple-darwin").is_err());
    fs::write(&library, synthetic_macho("/developer/private")).unwrap();
    assert!(binary::inspect(&library, "x86_64-apple-darwin", false).is_err());
}
