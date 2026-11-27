use crate::{cores::Inventory, dependencies};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use shinki_emu_build::ohos::Target as OhosTarget;
use shinki_emu_build::{CoreBuildManifest, NativeInputs};
use std::{
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
        if !crate::ohos::cores(inv, target).any(|(core, _)| core.id == component) {
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
    // The Manager may use a core's format readers, but not its dynamic entry package.
    let ohos_entries = if component == "manager" {
        crate::ohos::cores(inv, target)
            .map(|(core, _)| package_id(&core.entry_package()))
            .collect::<Result<std::collections::BTreeSet<_>>>()?
    } else {
        std::collections::BTreeSet::new()
    };
    let package_id = package_id(&package)?;

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let output = child.stdout.take().context("Cargo stdout")?;
    let mut libraries = Vec::new();
    let mut core_build = None;
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
        if value["package_id"].as_str().is_some_and(|id| ohos_entries.contains(id))
            && (value["reason"] == "compiler-artifact"
                || value["reason"] == "build-script-executed")
        {
            parse_error = Some(anyhow::anyhow!(
                "OHOS Manager must build without bundled core entry package: {}",
                value["package_id"]
            ));
        }
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
        if let Some(core) = core
            && value["reason"] == "build-script-executed"
            && (value["package_id"] == package_id
                || adapter_package_id.is_some_and(|id| value["package_id"] == id))
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
                    Ok(manifest) => {
                        if core_build.is_none() && manifest.plugin_id == core.id {
                            core_build = Some(manifest);
                        }
                    }
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
    let library = select_library(
        &package,
        target,
        ios_framework_input,
        core_build.as_ref(),
        libraries,
    )?;
    Ok(BuildArtifact {
        component: component.into(),
        target: target.into(),
        library,
        core_build,
        native_inputs,
        product_version,
        source_commit: product_source,
        features,
    })
}

fn select_library(
    package: &str,
    target: &str,
    ios_framework: bool,
    manifest: Option<&CoreBuildManifest>,
    mut cargo_libraries: Vec<PathBuf>,
) -> Result<PathBuf> {
    if let Some(manifest) = manifest {
        if manifest.target != target {
            bail!(
                "{package} reported an artifact for {} rather than {target}",
                manifest.target
            );
        }
        manifest.validate()?;
        if !ios_framework {
            match manifest.artifact_kind.as_str() {
                "native-shared" => {
                    return manifest
                        .native_library
                        .clone()
                        .context("missing native shared library");
                }
                "native-static" => {
                    bail!("{package} reported a static library for a standalone core")
                }
                _ => {}
            }
        }
    }
    cargo_libraries.sort();
    cargo_libraries.dedup();
    if cargo_libraries.len() != 1 {
        bail!(
            "Cargo did not report exactly one dynamic library/executable for {package}: {cargo_libraries:?}"
        );
    }
    Ok(cargo_libraries.remove(0))
}

fn requested_manager_features() -> Result<Vec<String>> {
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
    fn artifact_selection_honors_native_report_without_a_core_id_list() {
        let dir = tempfile::tempdir().unwrap();
        let native = dir.path().join("native.so");
        let wrapper = dir.path().join("wrapper.so");
        fs::write(&native, b"native").unwrap();
        let mut manifest = CoreBuildManifest {
            plugin_id: "independent-core".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            artifact_kind: "native-shared".into(),
            native_library: Some(native.clone()),
            resources: vec![],
            licenses: vec![],
        };
        let target = manifest.target.clone();
        let select = |manifest: &CoreBuildManifest, framework| {
            select_library(
                "wrapper",
                &target,
                framework,
                Some(manifest),
                vec![wrapper.clone()],
            )
        };
        assert_eq!(select(&manifest, false).unwrap(), native);
        assert_eq!(select(&manifest, true).unwrap(), wrapper);
        manifest.artifact_kind = "rust-cdylib".into();
        assert!(select(&manifest, false).is_err());
        manifest.native_library = None;
        assert_eq!(select(&manifest, false).unwrap(), wrapper);
        manifest.target = "aarch64-unknown-linux-gnu".into();
        assert!(select(&manifest, false).is_err());
    }

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
    fn ohos_companion_builds_are_available_only_inside_the_hap() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        for target in OhosTarget::ALL {
            let triple = target.rust_target();
            assert!(inventory.require_buildable("manager", triple).is_ok());
            let mut roster = crate::ohos::cores(&inventory, triple).peekable();
            assert!(roster.peek().is_some());
            for (core, _) in roster {
                assert!(inventory.require_buildable(&core.id, triple).is_err());
            }
        }
        for triple in ["x86_64-unknown-linux-gnu", "armv7-unknown-linux-ohos"] {
            assert!(crate::ohos::cores(&inventory, triple).next().is_none());
        }
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
