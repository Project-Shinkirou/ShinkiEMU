//! APK Java/AIDL/JAR inputs produced by the KiriKiri native adapters.
use super::{asset_entries, tree};
use crate::{build::{self, BuildArtifact}, cores::Inventory};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

const CORES: [&str; 2] = ["kirikiroid2-web", "krkr2"];
const ZIP_HELPER: &str = "libs/com.android.vending.expansion.zipfile.jar";
pub(super) fn inputs(inv: &Inventory, manager: &BuildArtifact) -> Result<Vec<(PathBuf, String)>> {
    let mut inputs = Vec::new();
    for id in CORES {
        let Some(core) = inv.cores.iter().find(|core| core.id == id) else {
            continue;
        };
        let root = if core.static_manager_targets.contains(&manager.target) {
            let manifest = manager.embedded_core_builds.iter().find(|manifest| manifest.plugin_id == id)
                .with_context(|| format!("missing Android build inputs for {id}"))?;
            manifest.resources.iter().find(|asset| asset.dest == "Resources")
                .context("embedded Android core is missing its Resources input")?
                .source.join("android-host")
        } else {
            if core.android_dynamic.is_none() || !core.supported_targets.contains(&manager.target) {
                bail!("{id} has no embedded or standalone Android host producer");
            }
            let version = crate::release::resolve_version(inv, id, "")?;
            let standalone = build::package_artifact(inv, &manager.target, id, &version)?;
            let manifest = standalone.core_build.as_ref().context("missing Android core build inputs")?;
            let license = format!("licenses/{id}/android-java");
            let notices = manifest.licenses.iter().find(|asset| asset.dest == license)
                .context("standalone Android host is missing its Java licenses")?;
            inputs.extend(asset_entries(notices)?.into_iter().map(|(path, name)| (path, format!("assets/{name}"))));
            standalone.library.parent().context("Android core library directory")?.join("android-host")
        };
        let files = files(&root)?;
        if id == "kirikiroid2-web" && !files.iter().any(|(_, name)| name == ZIP_HELPER) {
            bail!("{id} Android host lacks its ZIP helper JAR");
        }
        inputs.extend(files.into_iter().map(|(path, name)| (path, format!("android-host/{id}/{name}"))));
    }
    Ok(inputs)
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
    fn android_host_rejects_native_files_in_java_input_tree() {
        let temp = crate::temporary::directory().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("java")).unwrap();
        fs::write(root.join("java/Host.java"), "class Host {}").unwrap();
        fs::write(root.join("java/Service.aidl"), "interface Service {}").unwrap();
        fs::create_dir(root.join("libs")).unwrap();
        fs::write(root.join("libs/shared.jar"), b"fixture JAR").unwrap();
        let staged: Vec<_> = files(root).unwrap().into_iter().map(|(_, target)| target).collect();
        assert_eq!(staged, ["java/Host.java", "java/Service.aidl", "libs/shared.jar"]);
        fs::write(root.join("libs/native.so"), b"unexpected").unwrap();
        assert!(files(root).is_err());
    }
}
