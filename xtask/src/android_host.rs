//! APK Java/AIDL/JAR inputs, independent of the Android core builds.
use super::tree;
use crate::cores::Inventory;
use anyhow::{Result, anyhow, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

type StageHost = fn(&Path) -> Result<(), Box<dyn std::error::Error>>;
const HOSTS: &[(&str, StageHost, &str)] = include!(concat!(env!("OUT_DIR"), "/android_hosts.rs"));

pub(super) fn inputs(
    inv: &Inventory,
    targets: &[(&str, &str)],
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    let mut inputs = Vec::new();
    let mut classes = Vec::with_capacity(HOSTS.len());
    for &(id, stage, class) in HOSTS {
        let core = inv.core(id)?;
        if !targets.iter().any(|(triple, _)| {
            core.supported_targets
                .iter()
                .any(|supported| supported == triple)
        }) {
            continue;
        }
        if core.android_dynamic.is_none() {
            bail!("Android host {id} has no dynamic admission policy");
        }
        let root = scratch.join(id);
        stage(&root).map_err(|error| anyhow!("staging Android host for {id}: {error}"))?;
        let files = files(&root)?;
        let java = format!("java/{}.java", class.replace('.', "/"));
        if !files.iter().any(|(_, name)| name == &java) {
            bail!("Android host {id} lacks its declared Activity adapter: {class}");
        }
        classes.push(class);
        let notices = tree(&root.join("licenses"))?;
        if notices.is_empty() {
            bail!("{id} Android host lacks Java licenses");
        }
        inputs.extend(
            notices
                .into_iter()
                .map(|(path, name)| (path, format!("assets/licenses/{id}/android-java/{name}"))),
        );
        inputs.extend(
            files
                .into_iter()
                .map(|(path, name)| (path, format!("android-host/{id}/{name}"))),
        );
    }
    let registry = scratch.join("AndroidActivityAdapters.java");
    fs::write(&registry, activity_registry(&classes))?;
    inputs.push((
        registry,
        "android-host/_registry/java/org/shinkirou/shinkiemu/AndroidActivityAdapters.java".into(),
    ));
    Ok(inputs)
}

/// Generate only a composition root. Java type checking enforces the adapter contract.
fn activity_registry(classes: &[&str]) -> String {
    use std::fmt::Write as _;
    let mut source = String::from(concat!(
        "package org.shinkirou.shinkiemu;\n",
        "final class AndroidActivityAdapters {\n",
        "    static AndroidActivityAdapter[] create() {\n",
        "        return new AndroidActivityAdapter[] {\n",
    ));
    for class in classes {
        writeln!(source, "            new {class}(),").expect("writing to a String");
    }
    source.push_str("        };\n    }\n}\n");
    source
}

pub(super) fn is_runtime_asset(name: &str) -> bool {
    name.split_once('/').is_some_and(|(root, tail)| {
        root.eq_ignore_ascii_case("Resources")
            && tail
                .split('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("android-host"))
    })
}

fn files(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut inputs = Vec::new();
    let java = tree(&root.join("java"))?;
    if !java.iter().any(|(_, name)| name.ends_with(".java")) {
        bail!("Android host input has no Java sources");
    }
    for (source, name) in java {
        if !name.ends_with(".java") && !name.ends_with(".aidl") {
            bail!("unexpected Android host source {name}");
        }
        inputs.push((source, format!("java/{name}")));
    }
    let libs = root.join("libs");
    match fs::symlink_metadata(&libs) {
        Ok(_) => {
            for (source, name) in tree(&libs)? {
                if name.contains('/') || !name.ends_with(".jar") {
                    bail!("unexpected Android host compile dependency {name}");
                }
                inputs.push((source, format!("libs/{name}")));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the pinned upstream Java source archives"]
    fn android_host_stages_pinned_adapters_in_lifecycle_order() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        let scratch = crate::temporary::directory().unwrap();
        let entries = inputs(&inv, &super::super::ANDROID_TARGETS, scratch.path()).unwrap();

        let input = |target: &str| {
            let (source, _) = entries
                .iter()
                .find(|(_, name)| name == target)
                .unwrap_or_else(|| panic!("missing host input {target}"));
            source
        };
        for path in [
            "android-host/kirikiroid2-web/java/org/shinkirou/krkr/AndroidActivityAdapter.java",
            "android-host/kirikiroid2-web/java/com/enhance/gameservice/IGameTuningService.aidl",
            "android-host/kirikiroid2-web/libs/com.android.vending.expansion.zipfile.jar",
            "assets/licenses/kirikiroid2-web/android-java/SDL-LICENSE.txt",
            "android-host/krkr2/java/org/shinkirou/krk2/AndroidActivityAdapter.java",
            "android-host/krkr2/java/com/krkr2xx/gameservice/IGameTuningService.aidl",
            "android-host/krkr2/libs/com.android.vending.expansion.zipfile.jar",
            "assets/licenses/krkr2/android-java/LICENSE_cocos2d-x.txt",
        ] {
            assert!(fs::metadata(input(path)).unwrap().len() > 0, "{path}");
        }

        let registry = fs::read_to_string(input(
            "android-host/_registry/java/org/shinkirou/shinkiemu/AndroidActivityAdapters.java",
        ))
        .unwrap();
        let kiri_web = registry
            .find("new org.shinkirou.krkr.AndroidActivityAdapter()")
            .unwrap();
        let krkr2 = registry
            .find("new org.shinkirou.krk2.AndroidActivityAdapter()")
            .unwrap();
        assert!(kiri_web < krkr2);
    }

    #[test]
    fn android_host_rejects_native_files_in_java_input_tree() {
        let temp = crate::temporary::directory().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("java")).unwrap();
        fs::write(root.join("java/Host.java"), "class Host {}").unwrap();
        fs::write(root.join("java/Service.aidl"), "interface Service {}").unwrap();
        fs::create_dir(root.join("libs")).unwrap();
        fs::write(root.join("libs/shared.jar"), b"fixture JAR").unwrap();
        let staged: Vec<_> = files(root)
            .unwrap()
            .into_iter()
            .map(|(_, target)| target)
            .collect();
        assert_eq!(
            staged,
            ["java/Host.java", "java/Service.aidl", "libs/shared.jar"]
        );
        fs::write(root.join("libs/native.so"), b"unexpected").unwrap();
        assert!(files(root).is_err());
    }
}
