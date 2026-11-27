//! HarmonyOS/OpenHarmony Manager HAP: stage the Rust cdylib and bundled
//! resources into `Platforms/OHOS`, build with hvigor, sign with the SDK's
//! hap-sign-tool and verify the signed archive.
use crate::{
    binary,
    build::BuildArtifact,
    cores::{Core, Inventory},
    dependencies,
    package::{self, atomic_directory, copy_entries, sha256},
};
use anyhow::{Context, Result, bail};
use object::{
    Object, ObjectSymbol,
    read::elf::{Dyn, ProgramHeader},
};
use sha2::{Digest, Sha256};
use shinki_emu_build::{
    BuildAsset,
    ohos::{MANAGER_LIBRARY as MANAGER, Target},
};
use shinki_emu_metadata::CoreIdentity;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

const BUNDLE_NAME: &str = "org.shinkirou.shinkiemu";

const BUNDLED: &str = "resources/rawfile/bundled/";
const BUNDLE_RECEIPT: &str = "ohos.json";
const CORE_SCHEMA: &str = "shinki.emu.core_package.v1";

fn core_version(identity: Option<&CoreIdentity>, product_version: &str) -> String {
    identity.map_or_else(|| product_version.to_owned(), CoreIdentity::version)
}

pub(crate) fn cores<'a>(
    inv: &'a Inventory,
    triple: &str,
) -> impl Iterator<Item = (&'a Core, &'a str)> + use<'a> {
    let target = Target::from_rust_target(triple);
    inv.cores
        .iter()
        .filter_map(move |core| Some((core, core.ohos_hap.as_ref()?.library_for(target?)?)))
}

struct CoreBuild<'a> {
    core: &'a Core,
    library: &'a str,
    identity: Option<CoreIdentity>,
    artifact: BuildArtifact,
}

impl CoreBuild<'_> {
    fn validate(&self, triple: &str, version: &str, commit: &str) -> Result<()> {
        let artifact = &self.artifact;
        let product = if self.identity.is_some() {
            (None, None)
        } else {
            (Some(version), Some(commit))
        };
        if artifact.component != self.core.id
            || artifact.target != triple
            || (
                artifact.product_version.as_deref(),
                artifact.source_commit.as_deref(),
            ) != product
        {
            bail!(
                "wrong OHOS core artifact for {}/{triple}/{version}",
                self.core.id
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeRole {
    Manager,
    Core,
    Dependency,
}

#[derive(Debug)]
struct NativeMember {
    path: PathBuf,
    role: NativeRole,
    sha256: String,
}

#[derive(Debug, Default)]
struct StagedHap {
    native: BTreeMap<String, NativeMember>,
    raw: BTreeMap<String, String>,
    cores: BTreeMap<String, package::CorePackage>,
}

fn native_basename(name: &str) -> Result<()> {
    if name.contains('/') || !name.starts_with("lib") || !name.ends_with(".so") {
        bail!("OHOS native library must be a portable .so basename: {name}");
    }
    package::portable(name)
}

impl StagedHap {
    fn add_native(&mut self, name: &str, path: &Path, role: NativeRole) -> Result<()> {
        native_basename(name)?;
        if let Some((existing, member)) = self
            .native
            .iter()
            .find(|(existing, _)| name.eq_ignore_ascii_case(existing))
        {
            if name != existing
                || role != NativeRole::Dependency
                || member.role != role
                || (member.path != path && member.sha256 != sha256(path)?)
            {
                bail!("conflicting OHOS HAP native library {name} ({existing})");
            }
        } else {
            let sha256 = sha256(path)?;
            if role != NativeRole::Dependency
                && self
                    .native
                    .values()
                    .any(|member| member.role != NativeRole::Dependency && member.sha256 == sha256)
            {
                bail!("OHOS HAP component {name} is not a distinct binary");
            }
            self.native.insert(
                name.into(),
                NativeMember {
                    path: path.into(),
                    role,
                    sha256,
                },
            );
        }
        Ok(())
    }

    fn add_closure(
        &mut self,
        inv: &Inventory,
        target: Target,
        artifact: &BuildArtifact,
    ) -> Result<()> {
        let search = package::search_dirs(inv, artifact)?;
        for (source, name) in binary::closure(&artifact.library, &search, target.rust_target())? {
            self.add_native(&name, &source, NativeRole::Dependency)?;
        }
        Ok(())
    }

    fn receipt(&self, target: Target, version: &str, commit: &str) -> serde_json::Value {
        let abi = target.abi();
        let bundled_cores: Vec<_> = self
            .cores
            .values()
            .map(|core| {
                let mut record = serde_json::json!({
                    "plugin_id": core.plugin_id,
                    "version": core.version,
                    "source_commit": commit,
                    "abi": core.abi,
                    "library": format!("libs/{abi}/{}", core.library),
                    "sha256": self.native[&core.library].sha256,
                });
                if let Some(identity) = &core.identity {
                    record["identity"] = serde_json::json!(identity);
                }
                record
            })
            .collect();
        let native_libraries: BTreeMap<_, _> = self
            .native
            .iter()
            .map(|(name, member)| (format!("libs/{abi}/{name}"), &member.sha256))
            .collect();
        serde_json::json!({
            "target": target.rust_target(),
            "abi": abi,
            "local_qa": target.local_qa(),
            "version": version,
            "source_commit": commit,
            "bundled_cores": bundled_cores,
            "native_libraries": native_libraries,
        })
    }
}

/// Public password of the OpenHarmony SDK test keystore (`OpenHarmony.p12`).
/// The generated debug key lives in a copy of that test keystore.
const TEST_KEYSTORE_PASSWORD: &str = "123456";
const DEBUG_KEY_ALIAS: &str = "shinkiemu debug";
const PROFILE_KEY_ALIAS: &str = "openharmony application profile debug";
const APP_CA_KEY_ALIAS: &str = "openharmony application ca";
const APP_CA: &str = "C=CN,O=OpenHarmony,OU=OpenHarmony Team,CN=OpenHarmony Application CA";
const DEBUG_SUBJECT: &str = "C=CN,O=Project-Shinkirou,OU=ShinkiEMU,CN=ShinkiEMU OHOS Debug";

/// DevEco Studio tools, located from the native SDK
/// (`<DevEco>/sdk/default/openharmony/native`).
pub(crate) struct Tools {
    sdk_home: PathBuf,
    node: PathBuf,
    hvigor: PathBuf,
    ohpm: PathBuf,
    java: PathBuf,
    sign_tool: PathBuf,
    test_pki: PathBuf,
    notices: [(PathBuf, &'static str); 2],
}

pub(crate) fn tools() -> Result<Tools> {
    let native = dependencies::ohos_sdk_native()?;
    let openharmony = native.parent().context("OHOS SDK layout")?;
    let sdk_home = openharmony
        .parent()
        .and_then(Path::parent)
        .context("OHOS SDK layout: expected <DevEco>/sdk/default/openharmony/native")?
        .to_owned();
    let deveco = sdk_home
        .parent()
        .context("DevEco Studio directory")?
        .to_owned();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let node = if cfg!(windows) {
        deveco.join("tools/node/node.exe")
    } else {
        deveco.join("tools/node/bin/node")
    };
    let jbr = deveco.join("jbr/bin").join(format!("java{exe}"));
    let java = if jbr.is_file() {
        jbr
    } else {
        PathBuf::from(std::env::var_os("JAVA_HOME").context(
            "hap-sign-tool needs Java: DevEco Studio's jbr is missing and JAVA_HOME is unset",
        )?)
        .join("bin")
        .join(format!("java{exe}"))
    };
    let lib = openharmony.join("toolchains/lib");
    let tools = Tools {
        sdk_home,
        node,
        hvigor: deveco.join("tools/hvigor/bin/hvigorw.js"),
        ohpm: deveco.join("tools/ohpm/bin/pm-cli.js"),
        java,
        sign_tool: lib.join("hap-sign-tool.jar"),
        test_pki: lib,
        notices: [
            (native.join("NOTICE.txt"), "licenses/ohos-sdk/NOTICE.txt"),
            (native.join("llvm/NOTICE"), "licenses/ohos-sdk/llvm-NOTICE"),
        ],
    };
    for path in [
        &tools.node,
        &tools.hvigor,
        &tools.ohpm,
        &tools.java,
        &tools.sign_tool,
        &tools.test_pki.join("OpenHarmony.p12"),
        &tools.test_pki.join("OpenHarmonyProfileDebug.pem"),
        &tools.test_pki.join("UnsgnedDebugProfileTemplate.json"),
    ]
    .into_iter()
    .chain(tools.notices.iter().map(|(path, _)| path))
    {
        if !path.is_file() {
            bail!(
                "DevEco Studio/OpenHarmony SDK file missing: {}",
                path.display()
            );
        }
    }
    Ok(tools)
}

fn hap_name(target: Target, version: &str) -> String {
    let qa = if target.local_qa() { "-local-qa" } else { "" };
    format!(
        "ShinkiEMU-{version}-ohos-{}{qa}-debug-test-signed.hap",
        target.abi()
    )
}

/// Build and test-sign the HAP from the Manager's Cargo output.
pub(crate) fn package(
    inv: &Inventory,
    target: Target,
    result: &BuildArtifact,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    let triple = target.rust_target();
    let roster: Vec<_> = cores(inv, triple).collect();
    let commit = result
        .source_commit
        .as_deref()
        .context("OHOS Manager source commit")?;
    if result.component != "manager"
        || result.target != triple
        || result.product_version.as_deref() != Some(version)
    {
        bail!("expected OHOS Manager artifact for {triple}/{version}");
    }
    let mut builds = Vec::with_capacity(roster.len());
    for (core, library) in roster {
        let identity = core.ohos_identity(&inv.root)?;
        let artifact = crate::build::execute_ohos_hap(inv, triple, &core.id, version)?;
        let build = CoreBuild {
            core,
            library,
            identity,
            artifact,
        };
        build.validate(triple, version, commit)?;
        builds.push(build);
    }
    let tools = tools()?;
    let signing = signing_material(&tools)?;
    let project = inv.root.join("Platforms/OHOS");
    atomic_directory(out, |stage| {
        let (contents, mut record) = stage_inputs(inv, target, result, &builds, &tools, &project)?;
        let unsigned = build_hap(&tools, target, &project, version)?;
        let name = hap_name(target, version);
        let hap = stage.join(&name);
        sign_hap(&tools, &signing, &unsigned, &hap)?;
        verify_hap(&tools, &signing, target, &hap, version, commit, &contents)?;
        record["hap"] = name.into();
        record["sha256"] = sha256(&hap)?.into();
        record["features"] = serde_json::json!(result.features);
        fs::write(
            stage.join(BUNDLE_RECEIPT),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })?;
    Ok(out.to_owned())
}

pub(crate) fn verify_receipt_roster(
    inv: &Inventory,
    triple: &str,
    version: &str,
    commit: &str,
    receipt: &serde_json::Value,
) -> Result<()> {
    let target = Target::from_rust_target(triple).context("OHOS receipt target")?;
    let abi = target.abi();
    if receipt["target"] != triple
        || receipt["version"] != version
        || receipt["source_commit"] != commit
        || receipt["local_qa"] != target.local_qa()
    {
        bail!("wrong target or product provenance in OHOS receipt");
    }
    let records = receipt["bundled_cores"]
        .as_array()
        .context("OHOS core receipt roster")?;
    let libraries = receipt["native_libraries"]
        .as_object()
        .context("OHOS native receipt roster")?;
    let prefix = format!("libs/{abi}/");
    if receipt["abi"] != abi || !libraries.contains_key(&format!("{prefix}{MANAGER}")) {
        bail!("missing Manager library or wrong ABI in the OHOS receipt");
    }
    for (path, hash) in libraries {
        native_basename(
            path.strip_prefix(&prefix)
                .context("wrong OHOS receipt library path")?,
        )?;
        if !hash.as_str().is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        }) {
            bail!("invalid OHOS native library digest in receipt: {path}");
        }
    }
    let expected: BTreeMap<_, _> = cores(inv, triple)
        .map(|(core, library)| (core.id.as_str(), (core, library)))
        .collect();
    if records.len() != expected.len() {
        bail!("wrong core roster size in OHOS receipt");
    }
    let mut seen = BTreeSet::new();
    for record in records {
        let id = record["plugin_id"]
            .as_str()
            .context("OHOS receipt plugin ID")?;
        let (core, basename) = expected
            .get(id)
            .context("unexpected core in OHOS receipt")?;
        let identity = core.ohos_identity(&inv.root)?;
        let path = format!("{prefix}{basename}");
        if !seen.insert(id)
            || record["version"] != core_version(identity.as_ref(), version)
            || record["identity"] != serde_json::to_value(&identity)?
            || record["source_commit"] != commit
            || record["abi"] != shinki_emu_build::ABI
            || record["library"] != path
            || libraries.get(&path) != Some(&record["sha256"])
        {
            bail!("wrong OHOS receipt for core {id}");
        }
    }
    Ok(())
}

/// Replace the generated `entry/libs` and `rawfile/bundled` trees (both
/// ignored by Git) with this build's runtime closure and bundled files.
fn stage_inputs(
    inv: &Inventory,
    target: Target,
    result: &BuildArtifact,
    builds: &[CoreBuild<'_>],
    tools: &Tools,
    project: &Path,
) -> Result<(StagedHap, serde_json::Value)> {
    let libs = project.join("entry/libs");
    let bundled = project.join("entry/src/main/resources/rawfile/bundled");
    for generated in [&libs, &bundled] {
        match fs::remove_dir_all(generated) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut contents = StagedHap::default();
    contents.add_native(MANAGER, &result.library, NativeRole::Manager)?;
    for build in builds {
        contents.add_native(build.library, &build.artifact.library, NativeRole::Core)?;
    }
    contents.add_closure(inv, target, result)?;
    for build in builds {
        contents.add_closure(inv, target, &build.artifact)?;
    }
    let libs = libs.join(target.abi());
    fs::create_dir_all(&libs)?;
    let native: Vec<_> = contents
        .native
        .iter()
        .map(|(name, member)| (member.path.clone(), name.clone()))
        .collect();
    copy_entries(&native, &libs)?;

    let temp = crate::temporary::directory()?;
    let mut entries = vec![(
        inv.root.join("THIRD_PARTY_NOTICES.md"),
        "THIRD_PARTY_NOTICES.md".to_owned(),
    )];
    entries.extend(
        tools
            .notices
            .iter()
            .map(|(path, name)| (path.clone(), (*name).to_owned())),
    );
    entries.extend(package::metadata_entries(inv, temp.path())?);
    entries.extend(package::licenses(inv, result, temp.path())?);
    for build in builds {
        let digest = &contents.native[build.library].sha256;
        let manifest = stage_core_manifest(inv, build, digest, temp.path(), &mut entries)?;
        if contents
            .cores
            .insert(manifest.plugin_id.clone(), manifest)
            .is_some()
        {
            bail!("duplicate OHOS HAP core {}", build.core.id);
        }
    }
    let version = result
        .product_version
        .as_deref()
        .context("OHOS Manager version")?;
    let commit = result
        .source_commit
        .as_deref()
        .context("OHOS Manager source commit")?;
    let receipt = contents.receipt(target, version, commit);
    verify_receipt_roster(inv, target.rust_target(), version, commit, &receipt)?;
    let receipt_path = temp.path().join(BUNDLE_RECEIPT);
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    entries.push((receipt_path, BUNDLE_RECEIPT.into()));
    fs::create_dir_all(&bundled)?;
    copy_entries(&entries, &bundled)?;
    for (source, name) in entries {
        contents.raw.insert(name, sha256(&source)?);
    }
    // Read by ManagerBridge.ets `extractBundled`.
    let manifest = bundled.join("bundled.manifest");
    let mut names = contents
        .raw
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    names.push('\n');
    fs::write(&manifest, names)?;
    contents
        .raw
        .insert("bundled.manifest".into(), sha256(&manifest)?);
    Ok((contents, receipt))
}

fn stage_core_manifest(
    inv: &Inventory,
    bundle: &CoreBuild<'_>,
    library_sha256: &str,
    scratch: &Path,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<package::CorePackage> {
    let core = bundle.core;
    let artifact = &bundle.artifact;
    let library = bundle.library;
    let id = &core.id;
    let build = artifact
        .core_build
        .as_ref()
        .context("OHOS bundled core build manifest")?;
    build.validate()?;
    if build.plugin_id != *id
        || build.target != artifact.target
        || !matches!(
            build.artifact_kind.as_str(),
            "rust-cdylib" | "native-shared"
        )
        || build
            .native_library
            .as_ref()
            .is_some_and(|path| path != &artifact.library)
    {
        bail!(
            "wrong OHOS bundled core build manifest for {id}/{}",
            artifact.target
        );
    }
    let scratch = scratch.join(id);
    fs::create_dir(&scratch)?;
    let mut files = vec![package::FileRecord {
        path: library.into(),
        sha256: library_sha256.into(),
    }];
    let mut data = package::licenses(inv, artifact, &scratch)?;
    core_assets(inv, core, build, &mut data)?;
    for (source, relative) in data {
        if Path::new(&relative)
            .extension()
            .is_some_and(|extension| extension == "so")
        {
            bail!("native code may not be staged in the HAP's rawfile data");
        }
        files.push(package::FileRecord {
            path: relative.clone(),
            sha256: sha256(&source)?,
        });
        entries.push((source, format!("cores/{id}/{relative}")));
    }
    let version = match &bundle.identity {
        Some(identity) => identity.version(),
        None => {
            let commit = artifact
                .source_commit
                .as_deref()
                .context("OHOS first-party core source commit")?;
            let product_source = package::write_product_source_commit(&scratch, commit)?;
            entries.push((
                scratch.join(&product_source.path),
                format!("cores/{id}/{}", product_source.path),
            ));
            files.push(product_source);
            artifact
                .product_version
                .clone()
                .context("OHOS first-party core product version")?
        }
    };
    shinki_emu_metadata::validate_core_version(&version, bundle.identity.as_ref())?;
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let declaration = package::CorePackage {
        schema: CORE_SCHEMA.into(),
        plugin_id: id.clone(),
        library: library.into(),
        version,
        identity: bundle.identity.clone(),
        recognition: core.recognition.clone(),
        abi: shinki_emu_build::ABI.into(),
        files,
    };
    let path = scratch.join("core.json");
    fs::write(&path, serde_json::to_vec_pretty(&declaration)?)?;
    entries.push((path, format!("cores/{id}/core.json")));
    Ok(declaration)
}

fn core_assets(
    inv: &Inventory,
    core: &Core,
    build: &shinki_emu_build::CoreBuildManifest,
    data: &mut Vec<(PathBuf, String)>,
) -> Result<()> {
    for asset in build.licenses.iter().chain(&build.resources) {
        data.extend(package::asset_entries(asset)?);
    }
    let licenses = core.license_files(&inv.root, &build.target)?;
    if build.licenses.is_empty() && licenses.is_empty() {
        bail!("OHOS bundled core {} must have license inputs", core.id);
    }
    let mut seen: BTreeSet<_> = data
        .iter()
        .map(|(source, _)| dunce::canonicalize(source))
        .collect::<io::Result<_>>()?;
    let mut add_inventory_asset = |asset| -> Result<()> {
        for (source, dest) in package::asset_entries(&asset)? {
            if seen.insert(dunce::canonicalize(&source)?) {
                data.push((source, dest));
            }
        }
        Ok(())
    };
    for (source, path) in licenses {
        add_inventory_asset(BuildAsset {
            source,
            dest: format!("licenses/inventory/{path}"),
        })?;
    }
    for path in &core.resources {
        let filename = Path::new(path)
            .file_name()
            .context("OHOS core resource filename")?
            .to_string_lossy();
        add_inventory_asset(BuildAsset {
            source: inv.root.join(path),
            dest: format!("Resources/{filename}"),
        })?;
    }
    Ok(())
}

fn build_hap(tools: &Tools, target: Target, project: &Path, version: &str) -> Result<PathBuf> {
    let output = project.join("entry/build/default/outputs/default");
    match fs::remove_dir_all(&output) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let hvigor_home = project
        .parent()
        .and_then(Path::parent)
        .context("repository root")?
        .join(".work/ohos")
        .join(target.rust_target())
        .join("hvigor-home");
    fs::create_dir_all(&hvigor_home)?;
    let node = |script: &Path| {
        let mut command = Command::new(&tools.node);
        command
            .arg(script)
            .current_dir(project)
            .env("DEVECO_SDK_HOME", &tools.sdk_home)
            .env("HVIGOR_USER_HOME", &hvigor_home)
            .env("SHINKI_EMU_OHOS_VERSION_NAME", version);
        command
    };
    dependencies::run(node(&tools.ohpm).args(["install", "--all"]))?;
    dependencies::run(
        node(&tools.hvigor)
            .args([
                "--mode",
                "module",
                "-p",
                "product=default",
                "-p",
                "buildMode=release",
            ])
            .args(["assembleHap", "--no-daemon"]),
    )?;
    // Only the top level: hvigor links symbol/ to its native cache.
    let mut haps = Vec::new();
    for entry in fs::read_dir(&output)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("-unsigned.hap"))
        {
            package::plain(&path)?;
            haps.push(path);
        }
    }
    if haps.len() != 1 {
        bail!(
            "hvigor must produce exactly one unsigned HAP in {}",
            output.display()
        );
    }
    Ok(haps.remove(0))
}

/// Debug signing material derived from the OpenHarmony SDK test PKI, kept in
/// `SHINKI_EMU_OHOS_SIGNING_DIR` outside the checkout. Created once: a copy of
/// the SDK test keystore with a new ECC key, an app certificate issued by the
/// SDK's test application CA, and a debug provision profile for this bundle
/// signed by the SDK's profile-debug key.
fn signing_material(tools: &Tools) -> Result<PathBuf> {
    let directory = PathBuf::from(std::env::var_os("SHINKI_EMU_OHOS_SIGNING_DIR").context(
        "set SHINKI_EMU_OHOS_SIGNING_DIR to a directory outside the checkout for OHOS debug signing material",
    )?);
    let files = ["debug.p12", "app-cert.cer", "profile.p7b"];
    if files.iter().all(|name| directory.join(name).is_file()) {
        for name in files {
            package::plain(&directory.join(name))?;
        }
        return Ok(directory);
    }
    if files.iter().any(|name| directory.join(name).exists()) {
        bail!(
            "incomplete OHOS signing material in {}; remove it to regenerate",
            directory.display()
        );
    }
    fs::create_dir_all(&directory)?;
    let temp = tempfile::tempdir_in(&directory)?;
    let work = temp.path();
    fs::copy(
        tools.test_pki.join("OpenHarmony.p12"),
        work.join("debug.p12"),
    )?;
    // hap-sign-tool accepts CA certificates only from .cer files: split the
    // SDK chain (root CA, application CA, profile-debug certificate).
    let chain = fs::read_to_string(tools.test_pki.join("OpenHarmonyProfileDebug.pem"))?;
    let certificates = pem_certificates(&chain);
    if certificates.len() != 3 {
        bail!(
            "OpenHarmonyProfileDebug.pem must hold root CA, application CA and profile certificate"
        );
    }
    fs::write(work.join("root-ca.cer"), certificates[0])?;
    fs::write(work.join("sub-ca.cer"), certificates[1])?;
    let keystore = |command: &mut Command| {
        command
            .arg("-keystoreFile")
            .arg(work.join("debug.p12"))
            .args(["-keystorePwd", TEST_KEYSTORE_PASSWORD])
            .current_dir(work);
    };
    let mut command = sign_tool(tools);
    command.args(["generate-keypair", "-keyAlias", DEBUG_KEY_ALIAS, "-keyPwd"]);
    command.args([
        TEST_KEYSTORE_PASSWORD,
        "-keyAlg",
        "ECC",
        "-keySize",
        "NIST-P-256",
    ]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    let mut command = sign_tool(tools);
    command
        .args(["generate-app-cert", "-keyAlias", DEBUG_KEY_ALIAS])
        .args([
            "-keyPwd",
            TEST_KEYSTORE_PASSWORD,
            "-signAlg",
            "SHA256withECDSA",
        ])
        .args(["-issuer", APP_CA, "-issuerKeyAlias", APP_CA_KEY_ALIAS])
        .args([
            "-issuerKeyPwd",
            TEST_KEYSTORE_PASSWORD,
            "-subject",
            DEBUG_SUBJECT,
        ])
        .args(["-validity", "3650", "-rootCaCertFile", "root-ca.cer"])
        .args(["-subCaCertFile", "sub-ca.cer", "-outForm", "certChain"])
        .args(["-outFile", "app-cert.cer"]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    let app_chain = fs::read_to_string(work.join("app-cert.cer"))?;
    let leaf = pem_certificates(&app_chain)
        .first()
        .copied()
        .context("generated app certificate chain is empty")?;
    let mut profile: serde_json::Value = serde_json::from_slice(&fs::read(
        tools.test_pki.join("UnsgnedDebugProfileTemplate.json"),
    )?)?;
    let now = chrono::Utc::now().timestamp();
    profile["uuid"] = profile_uuid(leaf, now).into();
    profile["validity"] = serde_json::json!({
        "not-before": now,
        "not-after": now + 3650 * 86_400,
    });
    let bundle = profile["bundle-info"]
        .as_object_mut()
        .context("profile template bundle-info")?;
    bundle.insert("developer-id".into(), "Project-Shinkirou".into());
    bundle.insert("bundle-name".into(), BUNDLE_NAME.into());
    bundle.insert("development-certificate".into(), format!("{leaf}\n").into());
    // The template's device list names two OpenHarmony test devices; an
    // unrestricted debug profile is valid on any device that trusts the PKI.
    profile
        .as_object_mut()
        .context("profile template")?
        .remove("debug-info");
    fs::write(
        work.join("profile.json"),
        serde_json::to_vec_pretty(&profile)?,
    )?;
    let mut command = sign_tool(tools);
    command
        .args([
            "sign-profile",
            "-mode",
            "localSign",
            "-keyAlias",
            PROFILE_KEY_ALIAS,
        ])
        .args(["-keyPwd", TEST_KEYSTORE_PASSWORD, "-profileCertFile"])
        .arg(tools.test_pki.join("OpenHarmonyProfileDebug.pem"))
        .args(["-inFile", "profile.json", "-signAlg", "SHA256withECDSA"])
        .args(["-outFile", "profile.p7b"]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    dependencies::run(
        sign_tool(tools)
            .args(["verify-profile", "-inFile", "profile.p7b"])
            .current_dir(work),
    )?;
    for name in files {
        fs::rename(work.join(name), directory.join(name))?;
    }
    Ok(directory)
}

fn pem_certificates(text: &str) -> Vec<&str> {
    const END: &str = "-----END CERTIFICATE-----";
    let mut certificates = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let Some(end) = rest[start..].find(END) else {
            break;
        };
        certificates.push(&rest[start..start + end + END.len()]);
        rest = &rest[start + end + END.len()..];
    }
    certificates
}

/// RFC 4122 version-4 layout over a digest of the certificate and creation time.
fn profile_uuid(leaf: &str, now: i64) -> String {
    use sha2::{Digest, Sha256};
    let mut bytes: [u8; 16] = Sha256::digest(format!("{leaf}{now}{}", std::process::id()))[..16]
        .try_into()
        .expect("SHA-256 digest is longer than a UUID");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn sign_tool(tools: &Tools) -> Command {
    let mut command = Command::new(&tools.java);
    command.arg("-jar").arg(&tools.sign_tool);
    command
}

fn sign_hap(tools: &Tools, signing: &Path, unsigned: &Path, hap: &Path) -> Result<()> {
    dependencies::run(
        sign_tool(tools)
            .args([
                "sign-app",
                "-mode",
                "localSign",
                "-keyAlias",
                DEBUG_KEY_ALIAS,
            ])
            .args(["-keyPwd", TEST_KEYSTORE_PASSWORD, "-appCertFile"])
            .arg(signing.join("app-cert.cer"))
            .arg("-profileFile")
            .arg(signing.join("profile.p7b"))
            .arg("-inFile")
            .arg(unsigned)
            .args(["-signAlg", "SHA256withECDSA", "-keystoreFile"])
            .arg(signing.join("debug.p12"))
            .args(["-keystorePwd", TEST_KEYSTORE_PASSWORD, "-signCode", "1"])
            .arg("-outFile")
            .arg(hap),
    )
}

fn verify_hap(
    tools: &Tools,
    signing: &Path,
    target: Target,
    hap: &Path,
    version: &str,
    commit: &str,
    contents: &StagedHap,
) -> Result<()> {
    let evidence = crate::temporary::directory()?;
    let chain = evidence.path().join("chain.cer");
    let profile = evidence.path().join("profile.p7b");
    dependencies::run(
        sign_tool(tools)
            .args(["verify-app", "-inFile"])
            .arg(hap)
            .arg("-outCertChain")
            .arg(&chain)
            .arg("-outProfile")
            .arg(&profile),
    )?;
    let certs = |path: &Path| -> Result<BTreeSet<String>> {
        Ok(pem_certificates(&fs::read_to_string(path)?)
            .into_iter()
            .map(|pem| pem.split_whitespace().collect())
            .collect())
    };
    let verified = certs(&chain)?;
    if verified.is_empty()
        || verified != certs(&signing.join("app-cert.cer"))?
        || fs::read(&profile)? != fs::read(signing.join("profile.p7b"))?
    {
        bail!("OHOS HAP signer or provision profile differs from the selected signing material");
    }
    verify_archive(target, fs::File::open(hap)?, version, commit, contents)
}

fn verify_archive(
    target: Target,
    hap: impl io::Read + io::Seek,
    version: &str,
    commit: &str,
    contents: &StagedHap,
) -> Result<()> {
    let native_prefix = format!("libs/{}/", target.abi());
    let mut archive = zip::ZipArchive::new(hap)?;
    if archive.has_duplicate_names() {
        bail!("duplicate signed HAP members in the ZIP central directory");
    }
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index)?;
        let name = entry.name()?;
        let path = name.strip_suffix('/').unwrap_or(&name);
        let folded = path.to_ascii_lowercase();
        if entry.is_symlink()
            || entry.enclosed_name().is_none()
            || name.contains('\\')
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
            || (folded.starts_with("libs/") && !name.starts_with("libs/"))
            || (folded.starts_with(BUNDLED) && !name.starts_with(BUNDLED))
            || !seen.insert(folded)
        {
            bail!("unsafe or duplicate signed HAP member {name}");
        }
        if let Some(path) = name.strip_prefix("libs/") {
            if entry.is_dir() && path.is_empty() {
                continue;
            }
            let Some(library) = name.strip_prefix(&native_prefix) else {
                bail!("signed HAP contains native libraries outside {native_prefix}");
            };
            if if entry.is_dir() {
                !library.is_empty()
            } else {
                !contents.native.contains_key(library)
            } {
                bail!("unexpected signed HAP native member {name}");
            }
        }
        if let Some(path) = name.strip_prefix(BUNDLED) {
            if entry.is_dir() {
                if !path.is_empty() && !contents.raw.keys().any(|name| name.starts_with(path)) {
                    bail!("unexpected signed HAP bundled directory {name}");
                }
            } else if !contents.raw.contains_key(path) {
                bail!("unexpected signed HAP bundled member {name}");
            }
        }
    }
    for (path, sha256) in &contents.raw {
        let name = format!("{BUNDLED}{path}");
        if package::sha256_reader(archive.by_name(&name)?)? != *sha256 {
            bail!("signed HAP content differs from the staged resource: {name}");
        }
    }
    let module: serde_json::Value = serde_json::from_reader(archive.by_name("module.json")?)?;
    if module["app"]["bundleName"] != BUNDLE_NAME || module["app"]["versionName"] != version {
        bail!("signed HAP bundle name or version differs from the build");
    }
    let receipt: serde_json::Value =
        serde_json::from_reader(archive.by_name(&format!("{BUNDLED}{BUNDLE_RECEIPT}"))?)?;
    if receipt != contents.receipt(target, version, commit) {
        bail!("wrong target, product provenance or roster in signed HAP receipt");
    }
    let mut libraries = BTreeSet::new();
    let commit_sha256 = hex::encode(Sha256::digest(commit));
    for (id, expected) in &contents.cores {
        let prefix = format!("cores/{id}/");
        let path = format!("{prefix}core.json");
        let core: package::CorePackage =
            serde_json::from_reader(archive.by_name(&format!("{BUNDLED}{path}"))?)?;
        if !contents.raw.contains_key(&path)
            || core.schema != CORE_SCHEMA
            || core.plugin_id != *id
            || core.library != expected.library
            || core.version != expected.version
            || core.identity != expected.identity
            || core.abi != shinki_emu_build::ABI
            || !libraries.insert(expected.library.as_str())
            || contents
                .native
                .get(&core.library)
                .is_none_or(|member| member.role != NativeRole::Core)
        {
            bail!("wrong identity, provenance or ABI for signed HAP core {id}");
        }
        let mut paths = BTreeSet::new();
        for file in &core.files {
            let expected = if file.path == core.library {
                Some(&contents.native[&core.library].sha256)
            } else {
                contents.raw.get(&format!("{prefix}{}", file.path))
            };
            if !paths.insert(file.path.as_str()) || expected != Some(&file.sha256) {
                bail!(
                    "wrong staged hash or duplicate core file: {prefix}{}",
                    file.path
                );
            }
        }
        if !paths.contains(core.library.as_str()) {
            bail!("signed HAP core {id} must record its library");
        }
        let product_source = contents.raw.get(&format!("{prefix}source-commit.txt"));
        let has_product_record = paths.contains("source-commit.txt");
        let valid_source = if core.identity.is_none() {
            has_product_record && product_source == Some(&commit_sha256)
        } else {
            !has_product_record && product_source.is_none()
        };
        if !valid_source {
            bail!("wrong product source commit payload for signed HAP core {id}");
        }
    }
    if contents
        .native
        .get(MANAGER)
        .is_none_or(|member| member.role != NativeRole::Manager)
        || libraries.len()
            != contents
                .native
                .values()
                .filter(|member| member.role == NativeRole::Core)
                .count()
    {
        bail!("incomplete OHOS HAP component roster");
    }
    let mut needed = BTreeMap::new();
    for (name, member) in &contents.native {
        needed.insert(
            name.as_str(),
            verify_native_member(&mut archive, target, name, member)?,
        );
    }
    verify_native_closure(target, contents, &needed)
}

fn verify_native_closure(
    target: Target,
    contents: &StagedHap,
    needed: &BTreeMap<&str, BTreeSet<String>>,
) -> Result<()> {
    fn visit<'a>(
        name: &'a str,
        target: Target,
        contents: &StagedHap,
        needed: &'a BTreeMap<&str, BTreeSet<String>>,
        visiting: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if !done.contains(name) {
            if !visiting.insert(name) {
                bail!("cyclic OHOS HAP native dependency at {name}");
            }
            for dep in &needed[name] {
                if binary::system(dep, target.rust_target()) {
                    continue;
                }
                match contents.native.get(dep) {
                    Some(member) if member.role == NativeRole::Dependency => {}
                    Some(_) => bail!("{name} must not depend on HAP component {dep}"),
                    None => bail!("{name} is missing HAP dependency {dep}"),
                }
                visit(dep, target, contents, needed, visiting, done)?;
            }
            visiting.remove(name);
            done.insert(name);
        }
        Ok(())
    }
    let mut done = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    for (name, member) in &contents.native {
        if member.role != NativeRole::Dependency {
            visit(name, target, contents, needed, &mut visiting, &mut done)?;
        }
    }
    if done.len() != contents.native.len() {
        bail!("OHOS HAP contains an unused native dependency");
    }
    Ok(())
}

fn verify_native_member(
    archive: &mut zip::ZipArchive<impl io::Read + io::Seek>,
    target: Target,
    library: &str,
    member: &NativeMember,
) -> Result<BTreeSet<String>> {
    let name = format!("libs/{}/{library}", target.abi());
    let mut entry = archive.by_name(&name)?;
    if entry.compression() != zip::CompressionMethod::Stored
        || entry
            .data_start()
            .is_none_or(|offset| offset % target.page_size() != 0)
    {
        bail!(
            "{name} must be stored uncompressed on a {}-byte page boundary",
            target.page_size()
        );
    }
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    if hex::encode(Sha256::digest(&bytes)) != member.sha256 {
        bail!("signed HAP content differs from the staged library: {name}");
    }
    verify_native(target, &bytes, library, member.role)
}

fn verify_native(
    target: Target,
    bytes: &[u8],
    name: &str,
    role: NativeRole,
) -> Result<BTreeSet<String>> {
    let expected = || {
        format!(
            "{name} is not a loadable {} ELF64 shared library",
            target.abi()
        )
    };
    let object::File::Elf64(file) = object::File::parse(bytes).with_context(expected)? else {
        bail!("{}", expected());
    };
    let segment_types = || {
        file.elf_program_headers()
            .iter()
            .map(|header| header.p_type(file.endian()))
    };
    if file.architecture() != target.architecture()
        || !file.is_little_endian()
        || file.kind() != object::ObjectKind::Dynamic
        || !segment_types().any(|kind| kind == object::elf::PT_LOAD)
        || !segment_types().any(|kind| kind == object::elf::PT_DYNAMIC)
        || segment_types().any(|kind| kind == object::elf::PT_INTERP)
    {
        bail!("{}", expected());
    }
    let endian = file.endian();
    let mut loads = file
        .elf_program_headers()
        .iter()
        .filter(|header| header.p_type(endian) == object::elf::PT_LOAD);
    if !loads.all(|load| {
        load.p_align(endian) >= target.page_size()
            && load.p_offset(endian) % target.page_size()
                == load.p_vaddr(endian) % target.page_size()
            && load.p_filesz(endian) <= load.p_memsz(endian)
            && load.data(endian, bytes).is_ok()
    }) {
        bail!("{name} has invalid ELF load segment alignment or bounds");
    }
    match role {
        NativeRole::Core => {
            if !file.dynamic_symbols().any(|symbol| {
                symbol.is_global()
                    && symbol.kind() == object::SymbolKind::Text
                    && symbol.is_definition()
                    && symbol.name_bytes() == Ok(b"shinki_emu_query".as_slice())
            }) {
                bail!("{name} does not export shinki_emu_query");
            }
        }
        _ if file.dynamic_symbols().chain(file.symbols()).any(|symbol| {
            symbol.is_definition() && symbol.name_bytes() == Ok(b"shinki_emu_query".as_slice())
        }) =>
        {
            bail!("{name} is not a core and must not define a core query");
        }
        _ => {}
    }
    let mut dynamic = file
        .elf_program_headers()
        .iter()
        .filter(|header| header.p_type(endian) == object::elf::PT_DYNAMIC);
    let entries = dynamic
        .next()
        .expect("validated PT_DYNAMIC")
        .dynamic(endian, bytes)?
        .context("missing ELF dynamic table")?;
    if dynamic.next().is_some() {
        bail!("{name} has more than one ELF dynamic table");
    }
    let entries = &entries[..entries
        .iter()
        .position(|entry| entry.tag(endian) == object::elf::DT_NULL)
        .context("ELF dynamic table lacks DT_NULL")?];
    if entries.iter().any(|entry| {
        matches!(
            entry.tag(endian),
            object::elf::DT_RPATH | object::elf::DT_RUNPATH
        )
    }) {
        bail!("{name} must use the app linker namespace, not ELF RPATH/RUNPATH");
    }
    let mut needed = BTreeSet::new();
    if entries.iter().any(|entry| {
        matches!(
            entry.tag(endian),
            object::elf::DT_NEEDED | object::elf::DT_SONAME
        )
    }) {
        let field = |tag| {
            entries
                .iter()
                .find(|entry| entry.tag(endian) == tag)
                .map(|entry| entry.d_val(endian))
                .context("missing ELF dynamic string table")
        };
        let address = field(object::elf::DT_STRTAB)?;
        let size = field(object::elf::DT_STRSZ)?;
        let mut data = None;
        for load in file
            .elf_program_headers()
            .iter()
            .filter(|header| header.p_type(endian) == object::elf::PT_LOAD)
        {
            data = load
                .data_range(endian, bytes, address, size)
                .map_err(|_| anyhow::anyhow!("invalid ELF load segment range"))?;
            if data.is_some() {
                break;
            }
        }
        let strings = object::read::StringTable::new(
            data.context("ELF dynamic strings are not in a loadable segment")?,
            0,
            size,
        );
        for entry in entries {
            match entry.tag(endian) {
                object::elf::DT_NEEDED => {
                    let dep = std::str::from_utf8(entry.string(endian, strings)?)?;
                    native_basename(dep)?;
                    needed.insert(dep.to_owned());
                }
                object::elf::DT_SONAME if entry.string(endian, strings)? != name.as_bytes() => {
                    bail!("{name} has a different ELF SONAME");
                }
                _ => {}
            }
        }
    }
    Ok(needed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    const ONE: (&str, &str) = ("test.alpha", "libshinki_emu_alpha.so");
    const TWO: (&str, &str) = ("test.beta", "libshinki_emu_beta.so");
    const DEP: &str = "libshinki_emu_fixture_dep.so";
    const VERSION: &str = "1.0.0";
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    // The verifier inspects ELF headers and the dynamic symbol table. Build
    // only those fields so the fixture runs on hosts without an OHOS SDK.
    fn elf(target: Target, exports_query: bool) -> Vec<u8> {
        const HEADERS: usize = 64;
        const PROGRAM_LEN: usize = 56;
        const STRINGS: usize = HEADERS + 2 * PROGRAM_LEN;
        const SYMBOLS: usize = 200;
        const DYNAMIC: u64 = 256;
        const SECTIONS: usize = 288;
        const SECTION_LEN: usize = 64;
        let mut bytes = vec![0; SECTIONS + 3 * SECTION_LEN];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        let machine = match target {
            Target::Arm64 => object::elf::EM_AARCH64,
            Target::X64 => object::elf::EM_X86_64,
        };
        bytes[16..18].copy_from_slice(&object::elf::ET_DYN.0.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.0.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&(HEADERS as u64).to_le_bytes());
        bytes[40..48].copy_from_slice(&(SECTIONS as u64).to_le_bytes());
        bytes[52..54].copy_from_slice(&(HEADERS as u16).to_le_bytes());
        bytes[54..56].copy_from_slice(&(PROGRAM_LEN as u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&2_u16.to_le_bytes());
        bytes[58..60].copy_from_slice(&(SECTION_LEN as u16).to_le_bytes());
        bytes[60..62].copy_from_slice(&3_u16.to_le_bytes());
        // The string table also serves as shstrtab; section names are unused.
        bytes[62..64].copy_from_slice(&1_u16.to_le_bytes());
        bytes[HEADERS..][..4].copy_from_slice(&object::elf::PT_LOAD.0.to_le_bytes());
        bytes[HEADERS + 48..][..8].copy_from_slice(&target.page_size().to_le_bytes());
        bytes[HEADERS + PROGRAM_LEN..][..4]
            .copy_from_slice(&object::elf::PT_DYNAMIC.0.to_le_bytes());
        bytes[HEADERS + PROGRAM_LEN + 8..][..8].copy_from_slice(&DYNAMIC.to_le_bytes());
        bytes[HEADERS + PROGRAM_LEN + 16..][..8].copy_from_slice(&DYNAMIC.to_le_bytes());
        bytes[HEADERS + PROGRAM_LEN + 32..][..8].copy_from_slice(&16_u64.to_le_bytes());
        let names = b"\0shinki_emu_query\0";
        bytes[STRINGS..][..names.len()].copy_from_slice(names);
        let strtab = &mut bytes[SECTIONS + SECTION_LEN..][..SECTION_LEN];
        strtab[4..8].copy_from_slice(&object::elf::SHT_STRTAB.0.to_le_bytes());
        strtab[24..32].copy_from_slice(&(STRINGS as u64).to_le_bytes());
        strtab[32..40].copy_from_slice(&(names.len() as u64).to_le_bytes());
        let symtab = &mut bytes[SECTIONS + 2 * SECTION_LEN..][..SECTION_LEN];
        symtab[4..8].copy_from_slice(&object::elf::SHT_DYNSYM.0.to_le_bytes());
        symtab[24..32].copy_from_slice(&(SYMBOLS as u64).to_le_bytes());
        symtab[32..40].copy_from_slice(&(if exports_query { 48_u64 } else { 24 }).to_le_bytes());
        symtab[40..44].copy_from_slice(&1_u32.to_le_bytes());
        symtab[56..64].copy_from_slice(&24_u64.to_le_bytes());
        if exports_query {
            let symbol = &mut bytes[SYMBOLS + 24..][..24];
            symbol[0..4].copy_from_slice(&1_u32.to_le_bytes());
            symbol[4] = (object::elf::STB_GLOBAL.0 << 4) | object::elf::STT_FUNC.0;
            symbol[6..8].copy_from_slice(&1_u16.to_le_bytes());
        }
        bytes
    }

    fn digest(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    type Members = Vec<(String, Vec<u8>)>;

    fn fixture(
        target: Target,
        directory: &Path,
        roster: &[(&str, &str)],
        deps: &[&str],
        external: Option<(&str, &CoreIdentity)>,
    ) -> Result<(StagedHap, Members)> {
        let mut contents = StagedHap::default();
        let mut members = Vec::new();
        for (basename, role) in [(MANAGER, NativeRole::Manager)]
            .into_iter()
            .chain(roster.iter().map(|&(_, name)| (name, NativeRole::Core)))
            .chain(deps.iter().map(|&name| (name, NativeRole::Dependency)))
        {
            let path = directory.join(basename);
            contents.add_native(basename, &path, role)?;
            members.push((format!("libs/{}/{basename}", target.abi()), fs::read(path)?));
        }
        let module = serde_json::json!({
            "app": {"bundleName": BUNDLE_NAME, "versionName": VERSION, "versionCode": 1},
            "module": {"name": "entry", "type": "entry", "packageName": "entry"},
        });
        members.push(("module.json".into(), serde_json::to_vec(&module)?));
        let mut raw = BTreeMap::from([("metadata/index.json".to_owned(), b"{}".to_vec())]);
        for &(id, name) in roster {
            let license = b"synthetic fixture license";
            let identity = external
                .filter(|(external_id, _)| *external_id == id)
                .map(|(_, identity)| identity);
            let mut manifest = package::CorePackage {
                schema: CORE_SCHEMA.into(),
                plugin_id: id.into(),
                library: name.into(),
                version: core_version(identity, VERSION),
                identity: identity.cloned(),
                recognition: Default::default(),
                abi: shinki_emu_build::ABI.into(),
                files: vec![
                    package::FileRecord {
                        path: name.into(),
                        sha256: contents.native[name].sha256.clone(),
                    },
                    package::FileRecord {
                        path: "licenses/LICENSE".into(),
                        sha256: digest(license),
                    },
                ],
            };
            if identity.is_none() {
                manifest.files.push(package::FileRecord {
                    path: "source-commit.txt".into(),
                    sha256: digest(COMMIT.as_bytes()),
                });
                raw.insert(
                    format!("cores/{id}/source-commit.txt"),
                    COMMIT.as_bytes().to_vec(),
                );
            }
            raw.insert(
                format!("cores/{id}/core.json"),
                serde_json::to_vec(&manifest)?,
            );
            raw.insert(format!("cores/{id}/licenses/LICENSE"), license.to_vec());
            contents.cores.insert(id.into(), manifest);
        }
        raw.insert(
            BUNDLE_RECEIPT.into(),
            serde_json::to_vec(&contents.receipt(target, VERSION, COMMIT))?,
        );
        let extraction_list = raw
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        raw.insert("bundled.manifest".to_owned(), extraction_list.into_bytes());
        for (name, data) in raw {
            contents.raw.insert(name.clone(), digest(&data));
            members.push((format!("{BUNDLED}{name}"), data));
        }
        Ok((contents, members))
    }

    fn archive(
        target: Target,
        members: &Members,
        options: Option<(&str, SimpleFileOptions)>,
    ) -> Result<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (path, bytes) in members {
            let default = if path.starts_with("libs/") {
                stored.with_alignment(target.page_size() as u16)
            } else {
                stored
            };
            writer.start_file(
                path,
                options
                    .filter(|(name, _)| *name == path)
                    .map_or(default, |(_, opts)| opts),
            )?;
            writer.write_all(bytes)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    #[test]
    fn ohos_hap_checks_identity_abi_alignment_roles_and_digests() -> Result<()> {
        for (target, other) in [(Target::Arm64, Target::X64), (Target::X64, Target::Arm64)] {
            let dir = crate::temporary::directory()?;
            let manager = elf(target, false);
            let alpha = elf(target, true);
            let mut beta = alpha.clone();
            beta.push(1);
            for (name, bytes) in [(MANAGER, &manager), (ONE.1, &alpha), (TWO.1, &beta)] {
                fs::write(dir.path().join(name), bytes)?;
            }
            let (single, members) = fixture(target, dir.path(), &[ONE], &[], None)?;
            verify_archive(
                target,
                Cursor::new(archive(target, &members, None)?),
                VERSION,
                COMMIT,
                &single,
            )?;
            let (mut staged, members) = fixture(target, dir.path(), &[ONE, TWO], &[], None)?;
            let good = archive(target, &members, None)?;
            let verify = |target, data, expected: &StagedHap| {
                verify_archive(target, Cursor::new(data), VERSION, COMMIT, expected)
            };
            verify(target, good.clone(), &staged)?;
            assert!(
                verify(other, good, &staged)
                    .unwrap_err()
                    .to_string()
                    .contains("outside libs/")
            );
            let name = format!("libs/{}/{}", target.abi(), TWO.1);

            let path = dir.path().join(TWO.1);
            assert!(staged.add_native(ONE.1, &path, NativeRole::Core).is_err());
            assert!(
                staged
                    .add_native("libshinki_emu_copy.so", &path, NativeRole::Core)
                    .is_err()
            );

            let bad = |members: &Members, reason: &str| -> Result<()> {
                let error = verify(target, archive(target, members, None)?, &staged).unwrap_err();
                assert!(error.to_string().contains(reason), "{error:#}");
                Ok(())
            };
            for extra in [
                format!("libs/{}/{}", target.abi(), DEP),
                format!("{BUNDLED}cores/test.unknown/core.json"),
                format!("libs/{}/{}", other.abi(), DEP),
            ] {
                let reason = if extra.contains(other.abi()) {
                    "outside libs/"
                } else {
                    "unexpected"
                };
                let mut members = members.clone();
                members.push((extra, beta.clone()));
                bad(&members, reason)?;
            }
            let mut duplicate = members.clone();
            let alias = name.replace(TWO.1, &TWO.1.to_ascii_uppercase());
            duplicate.push((alias.clone(), beta.clone()));
            bad(&duplicate, "duplicate")?;
            // Rewrite the same-length alias in both ZIP headers to exercise an
            // exact duplicate; the ZIP writer itself rejects duplicate names.
            let mut exact_duplicate = archive(target, &duplicate, None)?;
            while let Some(i) = exact_duplicate
                .windows(alias.len())
                .position(|bytes| bytes == alias.as_bytes())
            {
                exact_duplicate[i..i + alias.len()].copy_from_slice(name.as_bytes());
            }
            assert!(verify(target, exact_duplicate, &staged).is_err());

            for missing in [&name, &format!("{BUNDLED}cores/{}/core.json", TWO.0)] {
                let mut missing_members = members.clone();
                missing_members.retain(|(path, _)| path != missing);
                assert!(verify(target, archive(target, &missing_members, None)?, &staged).is_err());
            }
            for (changed, reason) in [
                (name.clone(), "staged library"),
                (format!("libs/{}/{MANAGER}", target.abi()), "staged library"),
                (format!("{BUNDLED}cores/{}/core.json", TWO.0), "core.json"),
                (
                    format!("{BUNDLED}cores/{}/source-commit.txt", TWO.0),
                    "source-commit.txt",
                ),
                (
                    format!("{BUNDLED}cores/{}/licenses/LICENSE", TWO.0),
                    "licenses/LICENSE",
                ),
            ] {
                let mut changed_members = members.clone();
                changed_members
                    .iter_mut()
                    .find(|(path, _)| *path == changed)
                    .unwrap()
                    .1[0] ^= 1;
                bad(&changed_members, reason)?;
            }
            let stored = zip::CompressionMethod::Stored;
            for (alignment, compression) in [
                (1, stored),
                (target.page_size() as u16, zip::CompressionMethod::Deflated),
            ] {
                let opts = SimpleFileOptions::default()
                    .with_alignment(alignment)
                    .compression_method(compression);
                let error = verify(
                    target,
                    archive(target, &members, Some((&name, opts)))?,
                    &staged,
                )
                .unwrap_err();
                assert!(error.to_string().contains("stored uncompressed"));
            }
            for (bytes, reason) in [
                (b"not ELF".to_vec(), "not a loadable"),
                (elf(other, true), "not a loadable"),
                (manager.clone(), "does not export"),
            ] {
                let error = verify_native(target, &bytes, TWO.1, NativeRole::Core).unwrap_err();
                assert!(error.to_string().contains(reason), "{error:#}");
            }
            assert!(verify_native(target, &alpha, MANAGER, NativeRole::Manager).is_err());
            assert!(verify_native(target, &alpha, DEP, NativeRole::Dependency).is_err());
            let core = staged.cores.get_mut(TWO.0).unwrap();
            core.abi = "shinki.emu.native.c.v2".into();
            assert!(verify(target, archive(target, &members, None)?, &staged).is_err());
        }
        Ok(())
    }

    fn replace_raw(staged: &mut StagedHap, members: &mut Members, path: &str, data: Vec<u8>) {
        let member = format!("{BUNDLED}{path}");
        let bytes = &mut members
            .iter_mut()
            .find(|(name, _)| *name == member)
            .unwrap()
            .1;
        staged.raw.insert(path.into(), digest(&data));
        *bytes = data;
    }

    #[test]
    fn ohos_receipt_and_hap_bind_external_adapter_to_product() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(root)?;
        let target = Target::Arm64;
        let triple = target.rust_target();
        let external = ("rfvp", "libshinki_emu_rfvp.so");
        let fork = inv
            .cores
            .iter_mut()
            .find(|core| core.id == external.0)
            .unwrap();
        fork.ohos_hap = Some(toml::from_str(&format!("library = {:?}", external.1))?);
        let locked = fork.ohos_identity(root)?.unwrap();
        let mut swapped = locked.clone();
        swapped.adapter = inv.core("siglus_rs")?.ohos_identity(root)?.unwrap().adapter;
        swapped.validate()?;
        inv.cores
            .retain(|core| core.id == external.0 || core.id == "shinki.emu.musica");

        let roster: Vec<_> = cores(&inv, triple)
            .map(|(core, basename)| (core.id.as_str(), basename))
            .collect();
        let directory = crate::temporary::directory()?;
        fs::write(directory.path().join(MANAGER), elf(target, false))?;
        for (index, &(_, name)) in roster.iter().enumerate() {
            let mut bytes = elf(target, true);
            bytes.extend(index.to_le_bytes());
            fs::write(directory.path().join(name), bytes)?;
        }
        let (mut staged, mut members) = fixture(
            target,
            directory.path(),
            &roster,
            &[],
            Some((external.0, &locked)),
        )?;
        let verify = |staged: &StagedHap, members: &Members| {
            verify_archive(
                target,
                Cursor::new(archive(target, members, None)?),
                VERSION,
                COMMIT,
                staged,
            )
        };
        verify(&staged, &members)?;

        let good_receipt = staged.receipt(target, VERSION, COMMIT);
        let mut receipt = good_receipt.clone();
        verify_receipt_roster(&inv, triple, VERSION, COMMIT, &receipt)?;
        let records = receipt["bundled_cores"].as_array().unwrap();
        let first_party = records
            .iter()
            .find(|record| record["plugin_id"] == "shinki.emu.musica")
            .unwrap();
        assert_eq!(first_party["version"], VERSION);
        assert_eq!(first_party["source_commit"], COMMIT);
        assert!(first_party["identity"].is_null());
        let index = records
            .iter()
            .position(|record| record["plugin_id"] == external.0)
            .unwrap();
        let external_record = &mut receipt["bundled_cores"][index];
        assert_eq!(external_record["version"], locked.version());
        assert_eq!(external_record["source_commit"], COMMIT);
        assert_eq!(external_record["identity"], serde_json::to_value(&locked)?);

        // Even a valid foreign identity and its matching version cannot reuse this product SHA.
        external_record["identity"] = serde_json::to_value(&swapped)?;
        external_record["version"] = swapped.version().into();
        let error = verify_receipt_roster(&inv, triple, VERSION, COMMIT, &receipt).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("wrong OHOS receipt for core rfvp")
        );
        replace_raw(
            &mut staged,
            &mut members,
            BUNDLE_RECEIPT,
            serde_json::to_vec(&receipt)?,
        );
        let error = verify(&staged, &members).unwrap_err();
        assert!(error.to_string().contains("signed HAP receipt"));

        for (field, wrong) in [
            ("source_commit", "a".repeat(40)),
            ("target", Target::X64.rust_target().into()),
        ] {
            let mut receipt = good_receipt.clone();
            receipt[field] = wrong.into();
            assert!(verify_receipt_roster(&inv, triple, VERSION, COMMIT, &receipt).is_err());
            replace_raw(
                &mut staged,
                &mut members,
                BUNDLE_RECEIPT,
                serde_json::to_vec(&receipt)?,
            );
            let error = verify(&staged, &members).unwrap_err();
            assert!(error.to_string().contains("signed HAP receipt"));
        }
        replace_raw(
            &mut staged,
            &mut members,
            BUNDLE_RECEIPT,
            serde_json::to_vec(&good_receipt)?,
        );
        let path = format!("cores/{}/core.json", external.0);
        let member = format!("{BUNDLED}{path}");
        let bytes = &members.iter().find(|(name, _)| *name == member).unwrap().1;
        let mut manifest: package::CorePackage = serde_json::from_slice(bytes)?;
        manifest.version = swapped.version();
        manifest.identity = Some(swapped);
        replace_raw(
            &mut staged,
            &mut members,
            &path,
            serde_json::to_vec(&manifest)?,
        );
        let error = verify(&staged, &members).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("wrong identity, provenance or ABI")
        );
        Ok(())
    }

    #[test]
    fn ohos_inventory_payload_keeps_manifest_paths_and_adds_missing_files() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inv = Inventory::load(root)?;
        let mut core = inv.core("shinki.emu.musica")?.clone();
        core.resources = vec![
            "Assets/Fonts/OFL-NotoSansJP.txt".into(),
            "THIRD_PARTY_NOTICES.md".into(),
        ];
        let manifest = shinki_emu_build::CoreBuildManifest {
            plugin_id: core.id.clone(),
            target: Target::Arm64.rust_target().into(),
            artifact_kind: "rust-cdylib".into(),
            native_library: None,
            resources: vec![BuildAsset {
                source: root.join(&core.resources[0]),
                dest: "Resources/font/OFL-NotoSansJP.txt".into(),
            }],
            licenses: vec![BuildAsset {
                source: root.join("LICENSE-MIT"),
                dest: "licenses/core/LICENSE-MIT".into(),
            }],
        };
        let mut data = Vec::new();
        core_assets(&inv, &core, &manifest, &mut data)?;
        let paths: BTreeSet<_> = data.iter().map(|(_, dest)| dest.as_str()).collect();
        assert_eq!(
            paths,
            BTreeSet::from([
                "Resources/THIRD_PARTY_NOTICES.md",
                "Resources/font/OFL-NotoSansJP.txt",
                "licenses/core/LICENSE-MIT",
                "licenses/inventory/LICENSE-APACHE",
            ]),
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires the OpenHarmony SDK and a local fixture output directory"]
    fn ohos_signed_multicore_hap() -> Result<()> {
        let output = PathBuf::from(
            std::env::var_os("SHINKI_EMU_OHOS_FIXTURE_DIR")
                .context("set SHINKI_EMU_OHOS_FIXTURE_DIR outside the checkout")?,
        );
        fs::create_dir_all(&output)?;
        let scratch = crate::temporary::directory()?;
        let tools = tools()?;
        let signing = signing_material(&tools)?;
        let sdk = dependencies::ohos_sdk_native()?;
        let clang = sdk
            .join("llvm/bin")
            .join(if cfg!(windows) { "clang.exe" } else { "clang" });
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = root.join("tests/fixtures/ohos_hap.c");
        let includes = root.join("../Source/FamilyApi/shinki-emu-abi/include");
        for target in Target::ALL {
            let directory = scratch.path().join(target.abi());
            fs::create_dir(&directory)?;
            for (name, define) in [
                (DEP, "-DFIXTURE_DEPENDENCY"),
                (MANAGER, "-DFIXTURE_MANAGER"),
                (ONE.1, ""),
                (TWO.1, ""),
            ] {
                let mut command = Command::new(&clang);
                command
                    .arg(format!("--target={}", target.llvm_target()))
                    .arg(format!("--sysroot={}", sdk.join("sysroot").display()))
                    .args(["-fPIC", "-nostdlib", "-shared", "-O2", "-Wl,--no-undefined"])
                    .arg(format!("-Wl,-z,max-page-size={}", target.page_size()))
                    .arg(format!("-Wl,-soname,{name}"))
                    .arg("-I")
                    .arg(&includes)
                    .arg(&source);
                if !define.is_empty() {
                    command.arg(define);
                } else {
                    let id = if name == ONE.1 { ONE.0 } else { TWO.0 };
                    command
                        .arg(format!("-DFIXTURE_ID=\"{id}\""))
                        .arg(format!("-DFIXTURE_VERSION=\"{VERSION}\""));
                }
                if name != DEP {
                    command.arg(directory.join(DEP));
                }
                dependencies::run(command.arg("-o").arg(directory.join(name)))?;
            }
            let (mut staged, members) = fixture(target, &directory, &[ONE, TWO], &[DEP], None)?;
            for basename in [MANAGER, ONE.1, TWO.1] {
                let closure = binary::closure(
                    &directory.join(basename),
                    std::slice::from_ref(&directory),
                    target.rust_target(),
                )?;
                assert_eq!(closure.len(), 1);
                for (path, name) in closure {
                    assert_eq!(name, DEP);
                    staged.add_native(&name, &path, NativeRole::Dependency)?;
                }
            }
            let (no_dep, incomplete) = fixture(target, &directory, &[ONE, TWO], &[], None)?;
            let error = verify_archive(
                target,
                Cursor::new(archive(target, &incomplete, None)?),
                VERSION,
                COMMIT,
                &no_dep,
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("missing HAP dependency"),
                "{error:#}"
            );
            let dep_copy = directory.join("copy.so");
            fs::copy(directory.join(DEP), &dep_copy)?;
            staged.add_native(DEP, &dep_copy, NativeRole::Dependency)?;
            fs::write(&dep_copy, b"different dependency")?;
            assert!(
                staged
                    .add_native(DEP, &dep_copy, NativeRole::Dependency)
                    .is_err()
            );
            let unsigned = directory.join("unsigned.hap");
            fs::write(&unsigned, archive(target, &members, None)?)?;
            let signed = output.join(format!("synthetic-{}.hap", target.abi()));
            sign_hap(&tools, &signing, &unsigned, &signed)?;
            verify_hap(&tools, &signing, target, &signed, VERSION, COMMIT, &staged)?;
            eprintln!(
                "verified signed {} fixture: {}",
                target.abi(),
                sha256(&signed)?
            );
        }
        Ok(())
    }
}
