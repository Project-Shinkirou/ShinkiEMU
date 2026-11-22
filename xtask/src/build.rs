use crate::{cores::Inventory, dependencies, sources};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shinki_emu_build::CoreBuildManifest;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) const ANDROID_CORE_LINK_ARG: &str =
    "-Wl,--no-undefined,-z,max-page-size=16384,--enable-new-dtags,-rpath,$ORIGIN";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub component: String,
    pub target: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_version: Option<String>,
    pub library: PathBuf,
    pub core_build: Option<CoreBuildManifest>,
    pub embedded_core_builds: Vec<CoreBuildManifest>,
    pub source_commit: String,
    pub source_dirty: bool,
    pub toolchain: String,
    pub features: Vec<String>,
    pub target_dir: PathBuf,
}

impl BuildResult {
    pub fn require_product_version(&self, expected: &str) -> Result<()> {
        let actual = self.product_version.as_deref().context(
            "build receipt lacks product_version; rebuild with SHINKI_EMU_PRODUCT_VERSION",
        )?;
        if actual != expected {
            bail!(
                "{} was built as {actual}, not requested package version {expected}; rebuild with the requested product version",
                self.component
            );
        }
        Ok(())
    }

    pub(crate) fn require_product_source(&self, expected: &str) -> Result<()> {
        if self.source_dirty
            || !valid_source_commit(&self.source_commit)
            || self.source_commit != expected
        {
            bail!(
                "{} requires a clean build receipt at source commit {expected}",
                self.component
            );
        }
        Ok(())
    }
}

pub(crate) fn valid_source_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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

#[derive(Deserialize)]
struct ManagerBuildVersion {
    version: String,
    source_commit: Option<String>,
}

fn manager_build_version(path: &Path, expected: &str, source: Option<&str>) -> Result<String> {
    let compiled: ManagerBuildVersion = serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("missing Manager compile identity {}", path.display()))?,
    )?;
    if compiled.version != expected {
        bail!(
            "Manager build-script version {} differs from requested compile version {expected}",
            compiled.version
        );
    }
    if compiled.source_commit.as_deref() != source {
        bail!("Manager build-script source commit differs from its compile input");
    }
    Ok(compiled.version)
}
fn receipt_path(inv: &Inventory, target: &str, component: &str) -> PathBuf {
    inv.root
        .join(".work/build")
        .join(target)
        .join(format!("{component}.json"))
}

fn target_directory(inv: &Inventory, target: &str, component: &str) -> Result<PathBuf> {
    let standard = dunce::simplified(&inv.root)
        .join(".work/target")
        .join(target)
        .join(component);
    let explicit = std::env::var_os("SHINKI_EMU_TARGET_ROOT").map(PathBuf::from);
    // Leave headroom for shaderc/CMake's nested source and object filenames.
    if explicit.is_none() && (!cfg!(windows) || standard.as_os_str().len() <= 60) {
        return Ok(standard);
    }
    let repository = dunce::canonicalize(&inv.root)?;
    let owner = serde_json::json!({
        "repository": repository, "target": target, "component": component
    });
    let identity = hex::encode(Sha256::digest(serde_json::to_vec(&owner)?));
    let short_root = if let Some(root) = explicit {
        dunce::simplified(&std::path::absolute(root)?).to_owned()
    } else {
        repository.join(".work/t")
    };
    let directory = short_root.join(&identity[..12]);
    fs::create_dir_all(&directory).with_context(|| {
        format!("cannot create isolated short target {}; set SHINKI_EMU_TARGET_ROOT to a writable short directory", directory.display())
    })?;
    crate::package::plain(&directory)?;
    let marker = directory.join("owner.json");
    if marker.exists() {
        let actual: serde_json::Value = serde_json::from_slice(&fs::read(&marker)?)?;
        if actual != owner {
            bail!(
                "short target directory belongs to another build: {}",
                directory.display()
            );
        }
    } else {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(marker)?;
        serde_json::to_writer_pretty(file, &owner)?;
    }
    Ok(directory)
}
pub fn load_receipt(inv: &Inventory, target: &str, component: &str) -> Result<BuildResult> {
    inv.require_buildable(component, target)?;
    let path = receipt_path(inv, target, component);
    let r: BuildResult = serde_json::from_slice(&fs::read(&path).with_context(|| {
        format!(
            "missing {}; run cargo xtask build --target {target} --{} --release",
            path.display(),
            if component == "manager" {
                "manager".into()
            } else {
                format!("core {component}")
            }
        )
    })?)?;
    if r.target != target || r.component != component {
        bail!("build receipt component/target mismatch");
    }
    crate::package::plain(&r.library)?;
    Ok(r)
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
    metadata: &'a serde_json::Value,
) -> Result<BTreeMap<&'a str, &'a str>> {
    let packages = metadata["packages"].as_array().context("cargo packages")?;
    let mut selected = BTreeMap::new();
    for core in &inv.cores {
        if !core
            .static_manager_targets
            .iter()
            .any(|triple| triple == target)
        {
            continue;
        }
        let mut matches = packages
            .iter()
            .filter(|package| package["name"] == core.package);
        let package = matches.next().with_context(|| {
            format!(
                "static core package {} absent from Cargo metadata",
                core.package
            )
        })?;
        if matches.next().is_some() {
            bail!("ambiguous static core package {}", core.package);
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

pub fn execute(
    inv: &Inventory,
    target: &str,
    component: &str,
    action: &str,
    release: bool,
) -> Result<Option<BuildResult>> {
    execute_configured(inv, target, component, action, release, false)
}

pub(crate) fn execute_ios_framework(
    inv: &Inventory,
    target: &str,
    component: &str,
) -> Result<BuildResult> {
    execute_configured(inv, target, component, "build", true, true)?
        .context("iOS Framework build did not return an artifact")
}

fn execute_configured(
    inv: &Inventory,
    target: &str,
    component: &str,
    action: &str,
    release: bool,
    ios_framework_input: bool,
) -> Result<Option<BuildResult>> {
    if ios_framework_input {
        if !crate::apple::ios_frameworks::target_supported(target)
            || !inv.core(component)?.ios_framework
        {
            bail!("no bundled iOS Framework recipe for {component}/{target}");
        }
        // The Manager's doctor already validated the target's static closures.
    } else {
        inv.require_buildable(component, target)?;
        dependencies::doctor(inv, target, Some(component).filter(|id| *id != "manager"))?;
    }
    let core = if component == "manager" {
        None
    } else {
        Some(inv.core(component)?)
    };
    let product_owned = core.is_none_or(|core| core.source.is_none());
    let manifest = core
        .map(|c| inv.root.join(&c.manifest))
        .unwrap_or_else(|| inv.root.join("Cargo.toml"));
    if !manifest.is_file() {
        bail!("adapter manifest missing: {}", manifest.display());
    }
    let source_state = if action == "build" {
        let directory = if let Some(core) = core {
            core.source_directory(&inv.root)?
        } else {
            inv.root.clone()
        };
        let state = sources::state(&directory)?;
        Some((directory, state))
    } else {
        None
    };
    let package = core
        .map(|c| c.package.as_str())
        .unwrap_or("shinki-emu-manager");
    let features = core.map(|c| c.features.clone()).unwrap_or_default();
    let metadata: serde_json::Value = if component == "manager"
        && inv.cores.iter().any(|core| {
            core.static_manager_targets
                .iter()
                .any(|triple| triple == target)
        }) {
        cargo_metadata(inv, target)?
    } else {
        serde_json::from_slice(&dependencies::capture(
            Command::new("cargo")
                .current_dir(&inv.root)
                .args([
                    "metadata",
                    "--locked",
                    "--format-version",
                    "1",
                    "--no-deps",
                    "--manifest-path",
                ])
                .arg(&manifest),
        )?)?
    };
    let embedded_packages = if component == "manager" {
        embedded_core_packages(inv, target, &metadata)?
    } else {
        BTreeMap::new()
    };
    let package_metadata = metadata["packages"]
        .as_array()
        .context("cargo packages")?
        .iter()
        .find(|p| p["name"] == package)
        .with_context(|| format!("package {package} absent from {}", manifest.display()))?;
    let package_id = package_metadata["id"]
        .as_str()
        .context("cargo package id")?;
    let features = if component == "manager" {
        let requested = std::env::var_os("SHINKI_EMU_MANAGER_FEATURES");
        let requested = requested
            .as_deref()
            .map(|v| v.to_str().context("non-UTF-8 SHINKI_EMU_MANAGER_FEATURES"))
            .transpose()?;
        manager_features(requested, package_metadata)?
    } else {
        features
    };
    let ios_framework_manager =
        component == "manager" && crate::apple::ios_frameworks::enabled(target, &features);
    if ios_framework_manager {
        crate::apple::ios_frameworks::require_native_host()?;
        if action == "build" && !release {
            bail!("use --release when building the iOS Framework Manager");
        }
    }
    let target_dir = if ios_framework_input || ios_framework_manager {
        // Private inputs and the opted-in Manager can share target dependencies,
        // but must never replace the default static Manager artifact.
        target_directory(inv, target, "manager-ios-framework")?
    } else {
        target_directory(inv, target, component)?
    };
    let version_inputs = product_owned.then(|| {
        [
            std::env::var_os("SHINKI_EMU_PRODUCT_VERSION"),
            std::env::var_os("SHINKI_EMU_CI_VERSION"),
        ]
    });
    let manager_manifest;
    let default_product_version = if product_owned && component != "manager" {
        // Standalone in-tree cores use the same development default as the
        // Manager, not the workspace version of their individual crate.
        manager_manifest = toml::from_str::<toml::Value>(&fs::read_to_string(
            inv.root.join("Source/Manager/shinki-emu-manager/Cargo.toml"),
        )?)?;
        manager_manifest["package"]["version"].as_str()
    } else {
        package_metadata["version"].as_str()
    };
    let product_version = version_inputs
        .as_ref()
        .map(|[explicit, ci]| {
            select_product_version(
                default_product_version.context("Manager package version")?,
                explicit
                    .as_deref()
                    .map(|v| v.to_str().context("non-UTF-8 SHINKI_EMU_PRODUCT_VERSION"))
                    .transpose()?,
                ci.as_deref()
                    .map(|v| v.to_str().context("non-UTF-8 SHINKI_EMU_CI_VERSION"))
                    .transpose()?,
            )
        })
        .transpose()?;
    let product_source = source_state
        .as_ref()
        .filter(|(_, state)| product_owned && !state.dirty)
        .map(|(_, state)| state.commit.as_str());
    let product_stamped = product_source.is_some();
    if let Some(version) = product_version {
        crate::release::check_product_version(version)?;
    }
    if product_owned && action == "build" {
        crate::release::check_product_source(product_source)?;
    }
    let android_abi = if target.contains("android") {
        Some(if target.starts_with("aarch64") {
            "arm64-v8a"
        } else {
            "x86_64"
        })
    } else {
        None
    };
    let mut cmd = if crate::apple::supported(target) {
        crate::apple::command(&inv.root, target)?
    } else {
        Command::new("cargo")
    };
    cmd.current_dir(&inv.root);
    let cargo_action = if ios_framework_input { "rustc" } else { action };
    cmd.args([cargo_action, "--locked", "--manifest-path"])
        .arg(&manifest)
        .args([
            "-p",
            package,
            "--message-format=json-render-diagnostics",
            "--target-dir",
        ])
        .arg(&target_dir);
    cmd.args(["--target", target]);
    if release {
        cmd.arg("--release");
    }
    // Android and OHOS load the Manager as a cdylib from their app shells.
    let library_host = target.contains("android") || target.ends_with("ohos");
    if core.is_some() || library_host {
        cmd.arg("--lib");
    } else {
        cmd.args(["--bin", "shinki-emu-manager"]);
    }
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    dependencies::apply_environment(inv, target, component, &mut cmd)?;
    if let Some(abi) = android_abi {
        // cargo-ndk consumes build-script JSON. Import its toolchain environment
        // and invoke Cargo directly so every embedded manifest stays observable.
        let api = crate::binary::ANDROID_API.to_string();
        let mut environment = if core.is_some() {
            dependencies::cargo_plugin(&dependencies::private_android_tool(inv, "cargo-ndk-env"))
        } else {
            Command::new("cargo")
        };
        environment
            .current_dir(&inv.root)
            .args(["ndk-env", "--target", target, "-P", &api, "--json"]);
        dependencies::apply_environment(inv, target, component, &mut environment)?;
        let environment: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&dependencies::capture(&mut environment)?)?;
        let clang = environment
            .get("CLANG_PATH")
            .context("cargo ndk-env omitted CLANG_PATH")?;
        // ndk-env omits the two private variables its linker wrapper requires.
        cmd.envs(&environment)
            .env("_CARGO_NDK_LINK_CLANG", clang)
            .env("_CARGO_NDK_LINK_TARGET", format!("--target={target}{api}"))
            .env("ANDROID_PLATFORM", &api)
            .env("ANDROID_ABI", abi)
            .env("CARGO_NDK_ANDROID_PLATFORM", abi);
        if core.is_some() {
            // Keep the linker beside the private tool that generated the
            // environment; Cargo's external plugin lookup can prefer CARGO_HOME.
            let key = target.replace('-', "_").to_ascii_uppercase();
            cmd.env(
                format!("CARGO_TARGET_{key}_LINKER"),
                dependencies::private_android_tool(inv, "cargo-ndk"),
            )
            .env_remove(format!("CARGO_TARGET_{key}_RUNNER"));
        }
        if core.is_some_and(|core| core.artifact_kind == "rust-cdylib") {
            let mut flags = environment
                .get("CARGO_ENCODED_RUSTFLAGS")
                .cloned()
                .or_else(|| std::env::var("CARGO_ENCODED_RUSTFLAGS").ok())
                .unwrap_or_else(|| {
                    std::env::var("RUSTFLAGS")
                        .unwrap_or_default()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join("\x1f")
                });
            if !flags.is_empty() {
                flags.push('\x1f');
            }
            // Bionic finds package-local dependencies beside the selected core.
            flags.push_str("-C\x1flink-arg=");
            flags.push_str(ANDROID_CORE_LINK_ARG);
            cmd.env("CARGO_ENCODED_RUSTFLAGS", flags);
        }
    }
    if let Some(version) = product_version {
        cmd.env("SHINKI_EMU_PRODUCT_VERSION", version);
    }
    cmd.env_remove("SHINKI_EMU_PRODUCT_SOURCE_COMMIT");
    if let Some(commit) = product_source {
        cmd.env("SHINKI_EMU_PRODUCT_SOURCE_COMMIT", commit);
    }
    if let Some((directory, before)) = &source_state {
        let immediately_before = sources::state(directory)?;
        if immediately_before.commit != before.commit || immediately_before.dirty != before.dirty {
            bail!("source checkout changed before Cargo started: {}", directory.display());
        }
    }
    if ios_framework_input {
        let install_name = crate::apple::ios_frameworks::install_name(component);
        cmd.args(["--", "-C", &format!("link-arg=-Wl,-install_name,{install_name}")]);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let output = child.stdout.take().context("Cargo stdout")?;
    let mut libraries = Vec::new();
    let mut manifests = Vec::new();
    let mut parse_error = None;
    let mut compiled_product_version = None;
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
        let expected_embedded = value["package_id"]
            .as_str()
            .and_then(|id| embedded_packages.get(id))
            .copied();
        if value["reason"] == "build-script-executed"
            && (value["package_id"] == package_id || expected_embedded.is_some())
            && let Some(dir) = value["out_dir"].as_str()
        {
            if component == "manager"
                && value["package_id"] == package_id
                && let Some(expected) = product_version
            {
                let path = Path::new(dir).join("manager-build-version.json");
                match manager_build_version(&path, expected, product_source) {
                    Ok(version) => compiled_product_version = Some(version),
                    Err(error) => parse_error = Some(error),
                }
            }
            let path = Path::new(dir).join("core-build.json");
            if path.is_file() {
                match fs::read(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| {
                        serde_json::from_slice::<CoreBuildManifest>(&bytes)
                            .map_err(anyhow::Error::from)
                    }) {
                    Ok(m) if expected_embedded.is_none_or(|id| m.plugin_id == id) => {
                        manifests.push(m);
                    }
                    Ok(_) => {
                        parse_error = Some(anyhow::anyhow!(
                            "core build manifest belongs to a different static core package"
                        ));
                    }
                    Err(e) => parse_error = Some(e),
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
            } else if value["target"]["crate_types"]
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
    if action != "build" {
        return Ok(None);
    }
    if component == "manager" && compiled_product_version.is_none() {
        bail!("Cargo did not report the Manager build-script version identity");
    }
    let mut embedded_core_builds = Vec::new();
    if component == "manager" {
        for manifest in manifests.drain(..) {
            let embedded = inv.core(&manifest.plugin_id)?;
            manifest.validate()?;
            if !embedded
                .static_manager_targets
                .iter()
                .any(|triple| triple == target)
                || manifest.target != target
                || manifest.profile != if release { "release" } else { "debug" }
                || !matches!(
                    manifest.artifact_kind.as_str(),
                    "native-static" | "rust-cdylib"
                )
                || embedded_core_builds
                    .iter()
                    .any(|accepted: &CoreBuildManifest| accepted.plugin_id == manifest.plugin_id)
            {
                bail!(
                    "native build manifest does not match embedded core {}",
                    embedded.id
                );
            }
            embedded_core_builds.push(manifest);
        }
        if embedded_core_builds.len() != embedded_packages.len() {
            bail!("Cargo did not report every selected static core build manifest");
        }
    }
    libraries.sort();
    libraries.dedup();
    let core_build = if core.is_some() {
        if manifests.len() != 1 {
            bail!(
                "expected one core-build.json from {package}'s build-script-executed, found {}",
                manifests.len()
            );
        }
        let m = manifests.remove(0);
        m.validate()?;
        if m.plugin_id != component
            || m.target != target
            || m.profile != if release { "release" } else { "debug" }
            || Some(m.artifact_kind.as_str()) != core.map(|c| c.artifact_kind.as_str())
        {
            bail!("core-build manifest does not match requested build");
        }
        Some(m)
    } else {
        None
    };
    let library = if let Some(native) = core_build.as_ref().and_then(|m| m.native_library.as_ref())
    {
        native.clone()
    } else {
        if libraries.len() != 1 {
            bail!(
                "Cargo did not report exactly one dynamic library/executable for {package}: {libraries:?}"
            );
        }
        libraries.remove(0)
    };
    if ios_framework_input {
        crate::apple::ios_frameworks::inspect(&library, target, component)?;
    } else {
        crate::binary::inspect(&library, target, core.is_some())?;
    }
    let (source_directory, before) = source_state.context("build source state missing")?;
    let after = sources::state(&source_directory)?;
    let source_commit = before.commit;
    let source_dirty = before.dirty || after.dirty || after.commit != source_commit;
    if product_stamped && source_dirty {
        bail!("product sources changed during a stamped build; rebuild from a clean checkout");
    }
    let toolchain = String::from_utf8(dependencies::capture(Command::new("rustc").arg("-Vv"))?)?;
    let result = BuildResult {
        component: component.into(),
        target: target.into(),
        profile: if release { "release" } else { "debug" }.into(),
        library,
        core_build,
        embedded_core_builds,
        product_version: if component == "manager" {
            compiled_product_version
        } else {
            product_version.map(str::to_owned)
        },
        source_commit,
        source_dirty,
        toolchain,
        features,
        target_dir,
    };
    if ios_framework_input {
        // An app-bundled Framework is not a standalone core build receipt.
        return Ok(Some(result));
    }
    if ios_framework_manager {
        crate::apple::ios_frameworks::build_inputs(inv, &result)?;
    }
    if component == "manager" && target.contains("android") {
        let generated = result.target_dir.join("android-inputs");
        if generated.exists() {
            fs::remove_dir_all(&generated)?;
        }
        crate::package::atomic_directory(&generated, |stage| {
            crate::package::android_inputs(inv, &result, stage)
        })?;
        println!("Android JNI/assets: {}", generated.display());
    }
    let path = receipt_path(inv, target, component);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    Ok(Some(result))
}
pub fn tests(inv: &Inventory, core: Option<&str>, manager: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&inv.root).args(["test", "--locked"]);
    if let Some(c) = core {
        let c = inv.core(c)?;
        cmd.arg("--manifest-path")
            .arg(inv.root.join(&c.manifest))
            .args(["-p", &c.package]);
        if !c.features.is_empty() {
            cmd.arg("--features").arg(c.features.join(","));
        }
        cmd.arg("--target-dir")
            .arg(inv.root.join(".work/test").join(&c.id));
    } else if manager {
        cmd.args(["-p", "shinki-emu-manager"])
            .arg("--target-dir")
            .arg(inv.root.join(".work/test/manager"));
    } else {
        cmd.args(["-p", "xtask", "-p", "shinki-emu-build"]);
    }
    if core.is_some() || manager {
        let version = String::from_utf8(dependencies::capture(Command::new("rustc").arg("-vV"))?)?;
        let host = version
            .lines()
            .find_map(|s| s.strip_prefix("host: "))
            .context("rustc host triple")?;
        dependencies::apply_environment(inv, host, core.unwrap_or("manager"), &mut cmd)?;
    }
    dependencies::run(&mut cmd)
}

/// Additive Manager features from `SHINKI_EMU_MANAGER_FEATURES` (comma list).
/// Default features stay on; each name must be declared by the Manager package.
fn manager_features(requested: Option<&str>, package: &serde_json::Value) -> Result<Vec<String>> {
    let declared = package["features"]
        .as_object()
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
        let package = serde_json::json!({
            "features": {"default": ["store-steam"], "store-steam": [], "no-core-download": []}
        });
        assert!(manager_features(None, &package).unwrap().is_empty());
        assert_eq!(
            manager_features(Some(" no-core-download,store-steam,no-core-download,"), &package)
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
            embedded_core_packages(&inv, "x86_64-unknown-linux-gnu", &metadata)
                .unwrap()
                .is_empty()
        );
        let selected = embedded_core_packages(&inv, "aarch64-linux-android", &metadata).unwrap();
        assert_eq!(selected.get("reader-package"), Some(&"shinki.emu.musica"));
        assert!(!selected.contains_key("patch-package"));
        metadata["packages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": "shinki-emu-musica", "id": "other-version"
            }));
        assert!(embedded_core_packages(&inv, "aarch64-linux-android", &metadata).is_err());
        metadata["packages"] = serde_json::json!([]);
        assert!(embedded_core_packages(&inv, "aarch64-linux-android", &metadata).is_err());
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

    #[test]
    fn manager_build_sidecar_rejects_an_old_or_missing_compile_identity() {
        let temp = crate::temporary::directory().unwrap();
        let path = temp.path().join("manager-build-version.json");
        let commit = "a".repeat(40);
        assert!(manager_build_version(&path, "0.1.0-beta.8", Some(&commit)).is_err());
        fs::write(&path, r#"{"version":"0.1.0-beta.6"}"#).unwrap();
        assert!(manager_build_version(&path, "0.1.0-beta.8", Some(&commit)).is_err());
        fs::write(&path, r#"{"version":"0.1.0-beta.8"}"#).unwrap();
        assert!(manager_build_version(&path, "0.1.0-beta.8", Some(&commit)).is_err());
        let compiled = serde_json::json!({"version":"0.1.0-beta.8", "source_commit":commit});
        fs::write(&path, serde_json::to_vec(&compiled).unwrap()).unwrap();
        assert!(manager_build_version(&path, "0.1.0-beta.8", Some(&"b".repeat(40))).is_err());
        manager_build_version(&path, "0.1.0-beta.8", compiled["source_commit"].as_str()).unwrap();
    }
}
