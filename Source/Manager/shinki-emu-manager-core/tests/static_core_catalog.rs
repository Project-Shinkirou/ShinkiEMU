#[path = "../build_support/catalog.rs"]
mod generation;
#[path = "../../../Families/shinki-emu-lucifen-formats/tests/support/mod.rs"]
mod lucifen_fixture;

use shinki_emu_manager_core::static_core_catalog;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn inventory() -> (toml::Value, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    (
        toml::from_str(&fs::read_to_string(root.join("build/cores.toml")).unwrap()).unwrap(),
        fs::read_to_string(root.join("build/targets.toml")).unwrap(),
    )
}

fn declared(core: &toml::Value, key: &str, target: &str) -> bool {
    core.get(key)
        .and_then(toml::Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(target)))
}

fn consume_generated(
    cores: &toml::Value,
    targets: &str,
    target: &str,
    enabled: bool,
    with_adv32: bool,
) {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("empty-installation");
    fs::create_dir(&data).unwrap();
    fs::write(
        root.path().join("static-core-catalog.rs"),
        generation::generate(
            &toml::to_string(cores).unwrap(),
            targets,
            target,
            with_adv32,
        )
        .unwrap(),
    )
    .unwrap();
    // Compile the actual public catalog module against each target's generated
    // data, using the host compiler. This executes Android data consumption on
    // Windows without pretending to cross-link or run an Android application.
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/static_core_catalog.rs");
    let mut consumer = format!(
        "#[path = {:?}] mod catalog;\nfn main() {{\nassert!(!std::path::Path::new(\"cores\").exists());\nassert!(catalog::lookup(\"unknown.external.plugin\").is_none());\nassert_eq!(catalog::entries().len(), {});\n",
        module.to_str().unwrap(),
        cores["cores"].as_array().unwrap().len()
    );
    for core in cores["cores"].as_array().unwrap() {
        let id = core["id"].as_str().unwrap();
        let family = core["family_id"].as_str().unwrap();
        let name = core["display_name"].as_str().unwrap();
        let embedded = enabled
            && (declared(core, "static_manager_targets", target)
                || (id == "shinki.emu.adv32" && with_adv32));
        let standalone = enabled && declared(core, "supported_targets", target);
        let supported = embedded || standalone;
        consumer.push_str(&format!(
            "let entry = catalog::lookup({id:?}).unwrap();\nassert_eq!((entry.family_id, entry.display_name, entry.supported, entry.standalone, entry.embedded), ({family:?}, {name:?}, {supported}, {standalone}, {embedded}));\nprintln!(\"{{}}\\t{{}}\\t{{}}\\t{{}}\\t{{}}\\t{{}}\", entry.plugin_id, entry.family_id, entry.display_name, entry.supported, entry.standalone, entry.embedded);\n"
        ));
        let first_party = core.get("attribution").is_none();
        consumer.push_str(&format!("assert_eq!(entry.first_party, {first_party});\n"));
        let android = ["aarch64-linux-android", "x86_64-linux-android"].contains(&target);
        let policy = core
            .get("android_dynamic")
            .filter(|_| android && standalone);
        let requires_context = policy.map(|policy| policy["requires_context"].as_bool().unwrap());
        consumer.push_str(&format!(
            "assert_eq!(entry.android_dynamic.map(|policy| policy.requires_context), {requires_context:?});\n"
        ));
        if id == "rfvp" && core.get("recognition").is_some() {
            consumer.push_str(&format!(
                "assert_eq!(entry.embedded_recognition_json.contains(\"Data/Engine.id\"), {embedded});\n"
            ));
        }
    }
    consumer.push_str("}\n");
    let source = root.path().join("consumer.rs");
    fs::write(&source, consumer).unwrap();
    let executable: PathBuf = root
        .path()
        .join(format!("consumer{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--edition=2024", "--crate-name", "catalog_consumer"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .env("OUT_DIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(executable)
        .current_dir(&data)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{target}:\n{}", String::from_utf8_lossy(&output.stdout));
    assert!(fs::read_dir(data).unwrap().next().is_none());
}

#[test]
fn windows_and_both_android_catalogs_are_consumable_without_installed_plugins() {
    let (mut cores, targets) = inventory();
    let rule: toml::Value = toml::from_str(
        r#"recognition = [{ path = "Data/Engine.id", offset = 4, magic = "53484b31" }]"#,
    )
    .unwrap();
    cores["cores"][0]
        .as_table_mut()
        .unwrap()
        .insert("recognition".into(), rule["recognition"].clone());
    for target in [
        "x86_64-pc-windows-msvc",
        "aarch64-linux-android",
        "x86_64-linux-android",
    ] {
        consume_generated(&cores, &targets, target, true, false);
    }
}

#[test]
fn adv32_mobile_candidate_embedding_is_opt_in() {
    let (cores, targets) = inventory();
    for target in [
        "aarch64-linux-android",
        "x86_64-linux-android",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
        "aarch64-unknown-linux-ohos",
    ] {
        for with_adv32 in [false, true] {
            consume_generated(&cores, &targets, target, true, with_adv32);
        }
    }
}

#[test]
fn android_policy_does_not_enable_a_foreign_or_blocked_target() {
    let (mut cores, targets) = inventory();
    let siglus = cores["cores"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|core| core["id"].as_str() == Some("siglus_rs"))
        .unwrap();
    siglus["supported_targets"]
        .as_array_mut()
        .unwrap()
        .retain(|target| target.as_str() != Some("x86_64-linux-android"));
    siglus.as_table_mut().unwrap().insert(
        "android_dynamic".into(),
        toml::from_str::<toml::Value>("minimum_release = 3\nrequires_context = true").unwrap(),
    );
    consume_generated(&cores, &targets, "aarch64-linux-android", true, false);
    consume_generated(&cores, &targets, "x86_64-linux-android", true, false);
    consume_generated(&cores, &targets, "x86_64-pc-windows-msvc", true, false);
}

#[test]
fn target_blocks_and_undeclared_targets_preserve_visible_but_unsupported_entries() {
    let (cores, targets) = inventory();
    let target = "aarch64-linux-android";
    let mut targets: toml::Value = toml::from_str(&targets).unwrap();
    let row = targets["targets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["triple"].as_str() == Some(target))
        .unwrap();
    row.as_table_mut()
        .unwrap()
        .insert("blocked".into(), "temporarily unavailable".into());
    consume_generated(
        &cores,
        &toml::to_string(&targets).unwrap(),
        target,
        false,
        true,
    );
    consume_generated(
        &cores,
        &toml::to_string(&targets).unwrap(),
        "undeclared-target",
        false,
        true,
    );
}

#[test]
fn generated_strings_and_publication_restrictions_keep_their_meaning() {
    let (mut cores, targets) = inventory();
    let core = &mut cores["cores"][0];
    core["display_name"] = "A \"quoted\" engine\\with\na new line 🦊".into();
    core["publish_blocked"] = "distribution is restricted".into();
    consume_generated(&cores, &targets, "x86_64-pc-windows-msvc", true, false);
}

#[test]
fn malformed_family_fields_and_duplicate_plugin_ids_are_rejected() {
    let (cores, targets) = inventory();
    let target = "x86_64-pc-windows-msvc";
    for family in ["", "Siglus", "../cmvs", " softpal", "1family", "引擎"] {
        let mut changed = cores.clone();
        changed["cores"][0]["family_id"] = family.into();
        assert!(
            generation::generate(&toml::to_string(&changed).unwrap(), &targets, target, false)
                .is_err(),
            "{family}"
        );
    }
    let mut changed = cores.clone();
    changed["cores"][0]
        .as_table_mut()
        .unwrap()
        .remove("family_id");
    assert!(
        generation::generate(&toml::to_string(&changed).unwrap(), &targets, target, false).is_err()
    );
    let mut changed = cores.clone();
    changed["cores"][0]["display_name"] = " \n".into();
    assert!(
        generation::generate(&toml::to_string(&changed).unwrap(), &targets, target, false).is_err()
    );
    let mut changed = cores;
    changed["cores"][1]["id"] = changed["cores"][0]["id"].clone();
    assert!(
        generation::generate(&toml::to_string(&changed).unwrap(), &targets, target, false).is_err()
    );
}

#[test]
fn recognized_family_has_displayable_candidates_without_any_core_installation() {
    let root = tempfile::tempdir().unwrap();
    let mut script = vec![0u8; 50];
    script[..4].copy_from_slice(b"PS2A");
    for (offset, value) in [(4, 48u32), (20, 2), (40, 2)] {
        script[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fs::write(root.path().join("arbitrary-name.bin"), script).unwrap();
    let detected = shinki_emu_manager_core::detection::detect_game_directory(
        root.path(),
        &shinki_emu_manager_core::NativeCancellation::new(1),
    )
    .unwrap();
    assert_eq!(detected.len(), 1);
    let candidates: Vec<_> = static_core_catalog::entries()
        .iter()
        .filter(|entry| entry.family_id == detected[0].family_id)
        .map(|entry| {
            (
                entry.plugin_id,
                entry.display_name,
                entry.supported,
                entry.embedded,
            )
        })
        .collect();
    assert!(
        !candidates.is_empty(),
        "recognized family must have catalog candidates"
    );
    assert!(
        candidates
            .iter()
            .all(|(id, name, _, _)| !id.is_empty() && !name.is_empty())
    );
    assert!(static_core_catalog::lookup("unknown.external.plugin").is_none());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    println!("recognized {}: {candidates:?}", detected[0].family_id);
}

#[test]
fn lucifen_recognizes_root_script_archive_without_loading_a_core() {
    use shinki_emu_lucifen_formats::{Archive, Sob};
    use shinki_emu_manager_core::{NativeCancellation, detection::detect_game_directory};

    let script = lucifen_fixture::SCRIPT_LPK;
    let read = |offset: u64, output: &mut [u8]| {
        let offset = offset as usize;
        output.copy_from_slice(&script[offset..offset + output.len()]);
        Ok(())
    };
    let archive = Archive::open("SCRIPT.LPK", script.len() as u64, None, read).unwrap();
    let entry = archive.entry("gameinit.sob").unwrap().unwrap();
    let sob = archive.read_member(entry, 64, read).unwrap();
    Sob::parse(&sob).unwrap();

    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/SCRIPT.LPK"), script).unwrap();
    fs::write(root.path().join("sample.lpk"), script).unwrap();
    let path = root.path().join("ScRiPt.LpK");
    let scan = || detect_game_directory(root.path(), &NativeCancellation::new(1)).unwrap();

    // Even at the expected path, magic with a truncated index is insufficient.
    fs::write(&path, &script[..8]).unwrap();
    assert!(scan().is_empty());

    fs::write(&path, script).unwrap();
    let found = scan();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].format, "lucifen.lpk1-sob");
    assert!(found[0].declared.is_none());
    let core = static_core_catalog::lookup("shinki.emu.lucifen").unwrap();
    assert_eq!(core.family_id, found[0].family_id);
    assert_eq!(
        core.standalone,
        cfg!(all(
            target_os = "windows",
            target_arch = "x86_64",
            target_env = "msvc"
        ))
    );
    assert!(!core.embedded);
}
