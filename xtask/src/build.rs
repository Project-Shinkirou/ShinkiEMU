use crate::{
    cores::{ArtifactSource, Inventory},
    dependencies,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use shinki_emu_build::ohos::Target as OhosTarget;
use shinki_emu_build::{CoreBuildManifest, NativeInputs};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub struct BuildArtifact {
    pub component: String,
    pub target: String,
    pub product_version: Option<String>,
    pub library: PathBuf,
    pub core_build: Option<CoreBuildManifest>,
    pub embedded_core_builds: Vec<CoreBuildManifest>,
    pub native_inputs: Vec<NativeInputs>,
    pub source_commit: Option<String>,
    pub features: Vec<String>,
}

fn select_product_version<'a>(
    package_version: &'a str,
    explicit: Option<&'a str>,
    ci: Option<&'a str>,
) -> Result<&'a str> {
    if let (Some(explicit), Some(ci)) = (explicit, ci)
        && explicit != ci
    {
        bail!("SHINKI_EMU_PRODUCT_VERSION and SHINKI_EMU_CI_VERSION disagree");
    }
    let version = explicit.or(ci).unwrap_or(package_version);
    semver::Version::parse(version).context("product version must be SemVer")?;
    Ok(version)
}

fn target_directory(inv: &Inventory, target: &str, component: &str) -> Result<PathBuf> {
    let standard = dunce::simplified(&inv.root)
        .join(".work/target")
        .join(target)
        .join(component);
    if let Some(directory) = std::env::var_os("CARGO_TARGET_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let explicit = std::env::var_os("SHINKI_EMU_TARGET_ROOT").map(PathBuf::from);
    // Leave headroom for shaderc/CMake's nested source and object filenames.
    if explicit.is_none() && (!cfg!(windows) || standard.as_os_str().len() <= 60) {
        return Ok(standard);
    }
    let repository = dunce::canonicalize(&inv.root)?;
    let identity = hex::encode(Sha256::digest(format!(
        "{}:{target}:{component}",
        repository.display()
    )));
    let short_root = if let Some(root) = explicit {
        dunce::simplified(&std::path::absolute(root)?).to_owned()
    } else {
        repository.join(".work/t")
    };
    Ok(short_root.join(&identity[..12]))
}
pub fn cargo_metadata(inv: &Inventory, target: &str) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&dependencies::capture(
        Command::new("cargo").current_dir(&inv.root).args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ]),
    )?)?)
}
fn embedded_core_packages<'a>(
    inv: &'a Inventory,
    target: &str,
    features: &[String],
    metadata: &'a serde_json::Value,
) -> Result<BTreeMap<&'a str, &'a str>> {
    let packages = metadata["packages"].as_array().context("cargo packages")?;
    let mut selected = BTreeMap::new();
    for core in &inv.cores {
        if !core.embedded_in_manager(target, features) {
            continue;
        }
        let adapter = core.adapter_package();
        let mut matches = packages.iter().filter(|package| package["name"] == adapter);
        let package = matches.next().with_context(|| {
            format!("static core package {} absent from Cargo metadata", adapter)
        })?;
        if matches.next().is_some() {
            bail!("ambiguous static core package {adapter}");
        }
        let id = package["id"]
            .as_str()
            .context("static core Cargo package id")?;
        if selected.insert(id, core.id.as_str()).is_some() {
            bail!("static cores share Cargo package {id}");
        }
    }
    Ok(selected)
}

pub(crate) fn source_commit(inv: &Inventory) -> Result<String> {
    let stdout = dependencies::capture(
        Command::new("git")
            .current_dir(&inv.root)
            .args(["rev-parse", "HEAD"]),
    )?;
    Ok(std::str::from_utf8(&stdout)?.trim().to_owned())
}

fn manager_manifest() -> Result<toml::Value> {
    Ok(toml::from_str(include_str!(
        "../../Source/Manager/shinki-emu-manager/Cargo.toml"
    ))?)
}

pub(crate) fn default_product_version() -> Result<String> {
    let manifest = manager_manifest()?;
    let read_env = |name| match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).context(name),
    };
    let explicit = read_env("SHINKI_EMU_PRODUCT_VERSION")?;
    let ci = read_env("SHINKI_EMU_CI_VERSION")?;
    Ok(select_product_version(
        manifest["package"]["version"]
            .as_str()
            .context("Manager package version")?,
        explicit.as_deref(),
        ci.as_deref(),
    )?
    .to_owned())
}

pub(crate) fn package_artifact(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
) -> Result<BuildArtifact> {
    execute_configured(inv, target, component, None, version)
}

pub(crate) fn build_restricted_core(inv: &Inventory, target: &str, component: &str) -> Result<()> {
    let package = inv.core(component)?.entry_package();
    let target_dir = target_directory(inv, target, component)?;
    dependencies::run(
        cargo_command(inv, target)?
            .args(["build", "--locked", "--release", "--lib", "-p", &package])
            .args(["--target", target, "--target-dir"])
            .arg(target_dir),
    )
}

pub(crate) fn execute_ios_framework(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
) -> Result<BuildArtifact> {
    execute_configured(
        inv,
        target,
        component,
        Some(BundledInput::IosFramework),
        version,
    )
}

pub(crate) fn execute_ohos_hap(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
) -> Result<BuildArtifact> {
    execute_configured(inv, target, component, Some(BundledInput::OhosHap), version)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BundledInput {
    IosFramework,
    OhosHap,
}

fn cargo_command(inv: &Inventory, target: &str) -> Result<Command> {
    let mut cmd = if crate::apple::supported(target) {
        crate::apple::command(&inv.root, target)?
    } else {
        Command::new("cargo")
    };
    cmd.current_dir(&inv.root);
    if let Some(target) = OhosTarget::from_rust_target(target) {
        dependencies::apply_ohos_environment(inv, target, &mut cmd)?;
    }
    if target.ends_with("android") {
        // cargo-ndk filters Cargo's build-script JSON. Import its toolchain
        // environment so packaging can read every producer's OUT_DIR.
        let api = crate::binary::ANDROID_API.to_string();
        let ndk = std::path::absolute(dependencies::ndk_root(inv)?)?;
        let mut environment = Command::new("cargo");
        environment
            .current_dir(&inv.root)
            .env("ANDROID_NDK_HOME", &ndk)
            .env("CARGO_NDK_HOME", &ndk)
            .args(["ndk-env", "--target", target, "-P", &api, "--json"]);
        let environment: BTreeMap<String, String> =
            serde_json::from_slice(&dependencies::capture(&mut environment)?)?;
        let clang = environment
            .get("CLANG_PATH")
            .context("cargo ndk-env omitted CLANG_PATH")?;
        let abi = if target.starts_with("aarch64") {
            "arm64-v8a"
        } else {
            "x86_64"
        };
        // ndk-env omits the two variables its linker wrapper requires.
        cmd.envs(&environment)
            .env("ANDROID_NDK_HOME", &ndk)
            .env("CARGO_NDK_HOME", &ndk)
            .env("_CARGO_NDK_LINK_CLANG", clang)
            .env("_CARGO_NDK_LINK_TARGET", format!("--target={target}{api}"))
            .env(
                format!("CMAKE_TOOLCHAIN_FILE_{}", target.replace('-', "_")),
                inv.root.join("build/android.toolchain.cmake"),
            )
            // ANDROID_PLATFORM selects Java's SDK jar in Slint's build script.
            .env("SHINKI_EMU_ANDROID_NATIVE_API", &api)
            .env("ANDROID_ABI", abi);
    }
    Ok(cmd)
}

fn execute_configured(
    inv: &Inventory,
    target: &str,
    component: &str,
    bundled_input: Option<BundledInput>,
    version: &str,
) -> Result<BuildArtifact> {
    let ios_framework_input = bundled_input == Some(BundledInput::IosFramework);
    let ohos_hap_input = bundled_input == Some(BundledInput::OhosHap);
    if ios_framework_input {
        if !crate::apple::ios_frameworks::target_supported(target)
            || !inv.core(component)?.ios_framework
        {
            bail!("no bundled iOS Framework recipe for {component}/{target}");
        }
    } else if ohos_hap_input {
        if !crate::ohos::is_core_input(component, target) {
            bail!("no bundled OHOS HAP recipe for {component}/{target}");
        }
    } else {
        inv.require_buildable(component, target)?;
    }
    let core = if component == "manager" {
        None
    } else {
        Some(inv.core(component)?)
    };
    let product_owned = core.is_none_or(|core| core.attribution.is_none());
    let package = core
        .map(|core| core.entry_package())
        .unwrap_or_else(|| "shinki-emu-manager".to_owned());
    let features = if component == "manager" {
        requested_manager_features()?
    } else {
        Vec::new()
    };
    let ios_framework_manager =
        component == "manager" && crate::apple::ios_frameworks::target_supported(target);
    if ios_framework_input || ios_framework_manager {
        crate::apple::ios_frameworks::require_native_host()?;
    }
    let target_dir = if ios_framework_input || ios_framework_manager {
        // Each core links independently but can reuse the Manager's Cargo cache.
        target_directory(inv, target, "manager")?
    } else {
        target_directory(inv, target, component)?
    };
    let product_version = product_owned.then(|| version.to_owned());
    // First-party cores expose the source commit in their runtime identity.
    let product_source = product_owned.then(|| source_commit(inv)).transpose()?;
    let mut cmd = cargo_command(inv, target)?;
    let action = if ios_framework_input {
        "rustc"
    } else {
        "build"
    };
    cmd.args([action, "--locked", "--release", "-p", &package])
        .args(["--target", target, "--target-dir"])
        .arg(&target_dir)
        .arg("--message-format=json");
    // Android and OHOS load the Manager as a cdylib from their app shells.
    let library_host = target.contains("android") || OhosTarget::from_rust_target(target).is_some();
    if core.is_some() || library_host {
        cmd.arg("--lib");
    } else {
        cmd.args(["--bin", "shinki-emu-manager"]);
    }
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    if let Some(version) = &product_version {
        cmd.env("SHINKI_EMU_PRODUCT_VERSION", version);
    }
    if let Some(commit) = &product_source {
        cmd.env("SHINKI_EMU_PRODUCT_SOURCE_COMMIT", commit);
    }
    if ios_framework_input {
        let install_name = crate::apple::ios_frameworks::install_name(component);
        cmd.args([
            "--crate-type",
            "cdylib",
            "--",
            "-C",
            &format!("link-arg=-Wl,-install_name,{install_name}"),
        ]);
    }

    let metadata = cargo_metadata(inv, target)?;
    let packages = metadata["packages"].as_array().context("Cargo packages")?;
    let package_id = |name: &str| {
        packages
            .iter()
            .find(|entry| entry["name"] == name)
            .and_then(|entry| entry["id"].as_str())
            .with_context(|| format!("package {name} absent from Cargo metadata for {target}"))
    };
    let adapter_package_id = core
        .map(|core| package_id(&core.adapter_package()))
        .transpose()?;
    let package_id = package_id(&package)?;
    let embedded_packages = if component == "manager" {
        embedded_core_packages(inv, target, &features, &metadata)?
    } else {
        BTreeMap::new()
    };

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let output = child.stdout.take().context("Cargo stdout")?;
    let mut libraries = Vec::new();
    let mut manifests = Vec::new();
    let mut native_inputs = Vec::new();
    let mut parse_error = None;
    for line in BufReader::new(output).lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                parse_error = Some(anyhow::Error::from(e));
                break;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("{line}");
                continue;
            }
        };
        if value["reason"] == "compiler-message"
            && let Some(s) = value["message"]["rendered"].as_str()
        {
            eprint!("{s}");
        }
        if value["reason"] == "build-script-executed"
            && let Some(dir) = value["out_dir"].as_str()
        {
            let path = Path::new(dir).join(NativeInputs::FILE);
            if path.is_file() {
                match fs::read(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| Ok(serde_json::from_slice::<NativeInputs>(&bytes)?))
                {
                    Ok(inputs) => native_inputs.push(inputs),
                    Err(error) => parse_error = Some(error.context(path.display().to_string())),
                }
            }
        }
        let expected_embedded = value["package_id"]
            .as_str()
            .and_then(|id| embedded_packages.get(id))
            .copied();
        if value["reason"] == "build-script-executed"
            && (value["package_id"] == package_id
                || adapter_package_id.is_some_and(|id| value["package_id"] == id)
                || expected_embedded.is_some())
            && let Some(dir) = value["out_dir"].as_str()
        {
            let path = Path::new(dir).join("core-build.json");
            if path.is_file() {
                match fs::read(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| {
                        serde_json::from_slice::<CoreBuildManifest>(&bytes)
                            .map_err(anyhow::Error::from)
                    }) {
                    Ok(m) => manifests.push(m),
                    Err(error) => parse_error = Some(error),
                }
            }
        }
        if value["package_id"] != package_id {
            continue;
        }
        if value["reason"] == "compiler-artifact" {
            if core.is_none() && !library_host {
                if let Some(s) = value["executable"].as_str() {
                    libraries.push(PathBuf::from(s));
                }
            } else if ios_framework_input
                || value["target"]["crate_types"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "cdylib"))
            {
                for f in value["filenames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                {
                    let p = Path::new(f);
                    if p.extension()
                        .is_some_and(|s| s == "dll" || s == "so" || s == "dylib")
                    {
                        libraries.push(p.to_owned());
                    }
                }
            }
        }
    }
    let status = child.wait()?;
    if let Some(e) = parse_error {
        return Err(e);
    }
    if !status.success() {
        bail!("Cargo {action} failed for {component}/{target}: {status}");
    }
    let (core_build, embedded_core_builds) = if core.is_some() {
        let own = manifests
            .into_iter()
            .find(|manifest| manifest.plugin_id == component);
        (own, Vec::new())
    } else {
        (None, manifests)
    };
    let library = if !ios_framework_input
        && core.is_some_and(|core| core.artifact_source() == ArtifactSource::NativeLibrary)
    {
        core_build
            .as_ref()
            .and_then(|manifest| manifest.native_library.clone())
            .with_context(|| {
                format!("{package} did not report its native library in core-build.json")
            })?
    } else {
        libraries.sort();
        libraries.dedup();
        if libraries.len() != 1 {
            bail!(
                "Cargo did not report exactly one dynamic library/executable for {package}: {libraries:?}"
            );
        }
        libraries.remove(0)
    };
    Ok(BuildArtifact {
        component: component.into(),
        target: target.into(),
        library,
        core_build,
        embedded_core_builds,
        native_inputs,
        product_version,
        source_commit: product_source,
        features,
    })
}

pub(crate) fn requested_manager_features() -> Result<Vec<String>> {
    let requested = std::env::var_os("SHINKI_EMU_MANAGER_FEATURES");
    let requested = requested
        .as_deref()
        .map(|value| {
            value
                .to_str()
                .context("non-UTF-8 SHINKI_EMU_MANAGER_FEATURES")
        })
        .transpose()?;
    manager_features(requested, &manager_manifest()?)
}

/// Additive Manager features from `SHINKI_EMU_MANAGER_FEATURES` (comma list).
/// Default features stay on; each name must be declared by the Manager package.
fn manager_features(requested: Option<&str>, package: &toml::Value) -> Result<Vec<String>> {
    let declared = package["features"]
        .as_table()
        .context("Manager package features")?;
    let mut features: Vec<String> = requested
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    features.sort();
    features.dedup();
    if let Some(name) = features
        .iter()
        .find(|name| *name == "default" || !declared.contains_key(name.as_str()))
    {
        bail!("SHINKI_EMU_MANAGER_FEATURES: `{name}` is not an additive Manager feature");
    }
    Ok(features)
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn manager_features_are_declared_additive_and_normalized() {
        let package: toml::Value = toml::from_str(
            "[features]\ndefault = ['store-steam']\nstore-steam = []\nno-core-download = []",
        )
        .unwrap();
        assert!(manager_features(None, &package).unwrap().is_empty());
        assert_eq!(
            manager_features(
                Some(" no-core-download,store-steam,no-core-download,"),
                &package
            )
            .unwrap(),
            ["no-core-download", "store-steam"]
        );
        assert!(manager_features(Some("default"), &package).is_err());
        assert!(manager_features(Some("no-core-downloads"), &package).is_err());
    }

    #[test]
    fn reader_dependencies_are_not_embedded_without_target_registration() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(root).unwrap();
        inv.cores.retain(|core| core.id == "shinki.emu.musica");
        inv.cores[0].static_manager_targets = vec!["aarch64-linux-android".into()];
        let mut metadata = serde_json::json!({
            "packages": [
                {"name": "shinki-emu-musica", "id": "reader-package"},
                {"name": "shinki-emu-patch", "id": "patch-package"}
            ]
        });
        assert!(
            embedded_core_packages(&inv, "x86_64-unknown-linux-gnu", &[], &metadata)
                .unwrap()
                .is_empty()
        );
        let selected =
            embedded_core_packages(&inv, "aarch64-linux-android", &[], &metadata).unwrap();
        assert_eq!(selected.get("reader-package"), Some(&"shinki.emu.musica"));
        assert!(!selected.contains_key("patch-package"));
        metadata["packages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": "shinki-emu-musica", "id": "other-version"
            }));
        assert!(embedded_core_packages(&inv, "aarch64-linux-android", &[], &metadata).is_err());
        metadata["packages"] = serde_json::json!([]);
        assert!(embedded_core_packages(&inv, "aarch64-linux-android", &[], &metadata).is_err());
    }

    #[test]
    fn default_manager_builds_have_no_embedded_core_inputs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root).unwrap();
        // The build collector must not ask for an adapter build report for any
        // default Manager, including the local OHOS simulator variant.
        let metadata = serde_json::json!({"packages": []});
        for target in inv
            .targets
            .iter()
            .filter(|target| target.manager_supported || target.is_local_qa())
        {
            assert!(
                embedded_core_packages(&inv, &target.triple, &[], &metadata)
                    .unwrap_or_else(|error| panic!("{}: {error}", target.triple))
                    .is_empty()
            );
        }
    }

    #[test]
    fn adv32_build_reports_are_selected_only_with_the_candidate_feature() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(root).unwrap();
        inv.cores
            .retain(|core| matches!(core.id.as_str(), "shinki.emu.adv32" | "shinki.emu.musica"));
        let target = "x86_64-pc-windows-msvc";
        let metadata = serde_json::json!({
            "packages": [
                {"name": "shinki-emu-adv32", "id": "adv32-package"},
                {"name": "shinki-emu-musica", "id": "musica-package"}
            ]
        });
        assert!(
            embedded_core_packages(&inv, target, &[], &metadata)
                .unwrap()
                .is_empty()
        );
        let features = [crate::cores::ADV32_CANDIDATE_FEATURE.to_owned()];
        let selected = embedded_core_packages(&inv, target, &features, &metadata).unwrap();
        assert_eq!(selected.get("adv32-package"), Some(&"shinki.emu.adv32"));
    }

    #[test]
    fn ohos_musica_build_is_available_only_inside_the_hap() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let musica = inventory.core("shinki.emu.musica").unwrap();
        for target in [OhosTarget::Arm64, OhosTarget::X64] {
            let triple = target.rust_target();
            assert!(inventory.require_buildable("manager", triple).is_ok());
            assert!(inventory.require_buildable(&musica.id, triple).is_err());
            assert!(!musica.embedded_in_manager(triple, &[]));
            assert!(crate::ohos::is_core_input(&musica.id, triple));
            assert!(!crate::ohos::is_core_input("shinki.emu.adv32", triple));
        }
        assert!(!crate::ohos::is_core_input(
            &musica.id,
            "x86_64-unknown-linux-gnu"
        ));
        assert!(!crate::ohos::is_core_input(
            &musica.id,
            "armv7-unknown-linux-ohos"
        ));
    }

    #[test]
    fn manager_compile_inputs_reject_conflicting_and_invalid_versions() {
        assert!(
            select_product_version("0.1.0-beta.6", Some("0.1.0-beta.8"), Some("0.1.0-beta.7"))
                .is_err()
        );
        for invalid in ["", "beta.8", "v0.1.0-beta.8", "0.1.0-beta.8 "] {
            assert!(select_product_version("0.1.0-beta.6", Some(invalid), None).is_err());
            assert!(select_product_version("0.1.0-beta.6", None, Some(invalid)).is_err());
        }
    }
}
