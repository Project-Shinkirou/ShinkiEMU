//! APK Java/AIDL/JAR inputs produced by the KiriKiri native adapters.
use super::{asset_entries, plain, sha256, tree};
use crate::{
    binary,
    build::{self, BuildResult},
    cores::Inventory,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

const CORES: [(&str, &str); 2] = [
    ("kirikiroid2-web", "libshinki_emu_kirikiroid2_web.so"),
    ("krkr2", "libshinki_emu_krkr2.so"),
];
const ZIP_HELPER: &str = "libs/com.android.vending.expansion.zipfile.jar";
const MAX_CONTRACT_BYTES: u64 = 1024 * 1024;
const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn is_runtime_asset(name: &str) -> bool {
    name.split_once('/').is_some_and(|(root, tail)| {
        root.eq_ignore_ascii_case("Resources")
            && tail
                .split('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("android-host"))
    })
}

pub(super) fn is_core_library(name: &str) -> bool {
    CORES.iter().any(|(_, library)| name == *library)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JniMethod {
    class: String,
    name: String,
    signature: String,
    native: bool,
}

fn files(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let contract = root.join("jni-contract.json");
    plain(&contract)?;
    let mut bytes = Vec::new();
    fs::File::open(&contract)?
        .take(MAX_CONTRACT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTRACT_BYTES {
        bail!("Android host JNI contract exceeds its input bound");
    }
    let methods: Vec<JniMethod> = serde_json::from_slice(&bytes)?;
    let mut seen = BTreeSet::new();
    for method in &methods {
        if !method.class.starts_with('L')
            || !method.class.ends_with(';')
            || method.name.is_empty()
            || !method.signature.starts_with('(')
            || !method.signature.contains(')')
            || !seen.insert((&method.class, &method.name, &method.signature))
        {
            bail!("invalid or duplicate Android host JNI method");
        }
    }
    if !methods.iter().any(|method| method.native) || !methods.iter().any(|method| !method.native) {
        bail!("Android host requires native and Java methods in its JNI contract");
    }
    let mut inputs = vec![(contract, "jni-contract.json".into())];
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
    for (source, name) in &inputs {
        if fs::metadata(source)?.len() > MAX_INPUT_BYTES {
            bail!("Android host compile input exceeds its size limit: {name}");
        }
    }
    Ok(inputs)
}

pub(super) fn inputs(
    inv: &Inventory,
    manager: &BuildResult,
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    let mut inputs = Vec::new();
    for (id, basename) in CORES {
        let Some(core) = inv.cores.iter().find(|core| core.id == id) else {
            continue;
        };
        let pin = core.source.as_ref().context("Android host fork pin")?;
        let standalone;
        let (manifest, root, kind) = if let Some(manifest) = manager
            .embedded_core_builds
            .iter()
            .find(|manifest| manifest.plugin_id == id)
        {
            if !core.static_manager_targets.contains(&manager.target) {
                bail!("{id} Android Manager receipt still embeds a removed core");
            }
            let resources = manifest
                .resources
                .iter()
                .find(|asset| asset.dest == "Resources")
                .context("embedded Android core is missing its Resources input")?;
            (manifest, resources.source.join("android-host"), "native-static")
        } else {
            if core.static_manager_targets.contains(&manager.target)
                || core.android_dynamic.is_none()
                || !core.supported_targets.contains(&manager.target)
                || core.artifact_kind != "native-shared"
                || !core.features.is_empty()
            {
                bail!("{id} has no embedded or approved standalone Android host producer");
            }
            standalone = build::load_receipt(inv, &manager.target, id).with_context(|| {
                format!("build {id} explicitly with --core for {} before Manager", manager.target)
            })?;
            standalone.require_product_source(&pin.rev)?;
            let manifest = standalone
                .core_build
                .as_ref()
                .context("standalone Android host producer is missing core-build.json")?;
            if standalone.profile != manager.profile
                || !standalone.features.is_empty()
                || standalone.library.file_name().and_then(|name| name.to_str())
                    != Some(basename)
                || manifest.native_library.as_ref() != Some(&standalone.library)
            {
                bail!("{id} has an incompatible standalone Android host build receipt");
            }
            binary::inspect(&standalone.library, &manager.target, true)?;
            let library_dir = standalone
                .library
                .parent()
                .context("Android core library parent")?;
            (manifest, library_dir.join("android-host"), "native-shared")
        };
        manifest.validate()?;
        if manifest.plugin_id != id
            || manifest.target != manager.target
            || manifest.profile != manager.profile
            || manifest.artifact_kind != kind
        {
            bail!("{id} Android host manifest does not match its producer");
        }
        let files = files(&root)?;
        if id == "kirikiroid2-web" && !files.iter().any(|(_, name)| name == ZIP_HELPER) {
            bail!("{id} Android host lacks its declared ZIP helper JAR");
        }
        let prefix = format!("android-host/{id}");
        inputs.extend(
            files
                .into_iter()
                .map(|(path, name)| (path, format!("{prefix}/{name}"))),
        );
        let producer = scratch.join(format!("{id}-android-host-producer.json"));
        fs::write(
            &producer,
            serde_json::to_vec_pretty(&serde_json::json!({
                "plugin_id": id, "source_commit": pin.rev, "artifact_kind": kind,
            }))?,
        )?;
        inputs.push((producer, format!("{prefix}/producer.json")));
        if kind == "native-shared" {
            let license = format!("licenses/{id}/android-java");
            let notices = manifest
                .licenses
                .iter()
                .find(|asset| asset.dest == license)
                .context("standalone Android host is missing its declared Java licenses")?;
            inputs.extend(
                asset_entries(notices)?
                    .into_iter()
                    .map(|(path, name)| (path, format!("assets/{name}"))),
            );
        }
    }
    Ok(inputs)
}

pub(super) fn hashes(entries: &[(PathBuf, String)]) -> Result<BTreeMap<String, String>> {
    entries.iter()
        .filter(|(_, name)| {
            name.starts_with("android-host/")
                || CORES.iter().any(|(id, _)| {
                    name.strip_prefix("assets/licenses/")
                        .and_then(|name| name.strip_prefix(*id))
                        .is_some_and(|rest| rest.starts_with("/android-java/"))
                })
        })
        .map(|(source, name)| Ok((name.clone(), sha256(source)?)))
        .collect()
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
        fs::write(
            root.join("jni-contract.json"),
            br#"[{"class":"LHost;","name":"start","signature":"()V","native":false},
                 {"class":"LHost;","name":"nativeStart","signature":"()V","native":true}]"#,
        )
        .unwrap();
        assert_eq!(files(root).unwrap().len(), 2);
        fs::create_dir(root.join("libs")).unwrap();
        fs::write(root.join("libs/native.so"), b"unexpected").unwrap();
        assert!(files(root).is_err());
    }
}
