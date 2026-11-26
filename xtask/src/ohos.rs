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
use object::{Object, ObjectSymbol, read::elf::ProgramHeader};
use sha2::{Digest, Sha256};
use shinki_emu_build::ohos::{HapCore, MANAGER_LIBRARY as MANAGER, Target};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

const BUNDLE_NAME: &str = "org.shinkirou.shinkiemu";

pub(crate) fn companion<'a>(inv: &'a Inventory, target: &str) -> Option<(&'a Core, &'a HapCore)> {
    Target::from_rust_target(target)?;
    inv.cores
        .iter()
        .find_map(|core| core.ohos_hap.as_ref().map(|recipe| (core, recipe)))
}

struct CompanionBuild<'a> {
    core: &'a Core,
    recipe: &'a HapCore,
    artifact: &'a BuildArtifact,
}

impl CompanionBuild<'_> {
    fn validate(&self, triple: &str, version: &str, commit: &str) -> Result<()> {
        let artifact = self.artifact;
        if artifact.component != self.core.id
            || artifact.target != triple
            || artifact.product_version.as_deref() != Some(version)
            || artifact.source_commit.as_deref() != Some(commit)
        {
            bail!(
                "wrong OHOS companion artifact for {}/{triple}/{version}",
                self.core.id
            );
        }
        Ok(())
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
    let (core, recipe) = companion(inv, triple).context("OHOS HAP companion recipe")?;
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
    if result
        .embedded_core_builds
        .iter()
        .any(|build| build.plugin_id == core.id)
        || core.embedded_in_manager(triple, &result.features)
    {
        bail!("OHOS Manager must not embed its companion core {}", core.id);
    }
    let manager = fs::read(&result.library)?;
    verify_native(target, &manager, MANAGER, NativeRole::Manager)?;
    let manager_sha256 = hex::encode(Sha256::digest(manager));
    let artifact = crate::build::execute_ohos_hap(inv, triple, &core.id, version)?;
    let bundled = CompanionBuild {
        core,
        recipe,
        artifact: &artifact,
    };
    bundled.validate(triple, version, commit)?;
    let core_library = &recipe.library;
    let tools = tools()?;
    let signing = signing_material(&tools)?;
    let project = inv.root.join("Platforms/OHOS");
    atomic_directory(out, |stage| {
        let declaration = stage_inputs(inv, target, result, &bundled, &tools, &project)?;
        let unsigned = build_hap(&tools, target, &project, version)?;
        let name = hap_name(target, version);
        let hap = stage.join(&name);
        dependencies::run(
            sign_tool(&tools)
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
                .arg(&unsigned)
                .args(["-signAlg", "SHA256withECDSA", "-keystoreFile"])
                .arg(signing.join("debug.p12"))
                .args(["-keystorePwd", TEST_KEYSTORE_PASSWORD, "-signCode", "1"])
                .arg("-outFile")
                .arg(&hap),
        )?;
        let core = verify_hap(&tools, target, &hap, &declaration, &manager_sha256)?;
        let library = core
            .files
            .iter()
            .find(|file| file.path == *core_library)
            .context("verified HAP companion library record")?;
        let record = serde_json::json!({
            "hap": name,
            "sha256": sha256(&hap)?,
            "target": triple,
            "abi": target.abi(),
            "local_qa": target.local_qa(),
            "version": version,
            "source_commit": commit,
            "features": result.features,
            "bundled_core": {
                "plugin_id": core.plugin_id,
                "version": core.version,
                "source_commit": commit,
                "abi": core.abi,
                "library": format!("libs/{}/{core_library}", target.abi()),
                "sha256": library.sha256,
            },
        });
        fs::write(stage.join("ohos.json"), serde_json::to_vec_pretty(&record)?)?;
        Ok(())
    })?;
    Ok(out.to_owned())
}

/// Replace the generated `entry/libs` and `rawfile/bundled` trees (both
/// ignored by Git) with this build's runtime closure and bundled files.
fn stage_inputs(
    inv: &Inventory,
    target: Target,
    result: &BuildArtifact,
    bundled_core: &CompanionBuild<'_>,
    tools: &Tools,
    project: &Path,
) -> Result<Vec<u8>> {
    let libs = project.join("entry/libs");
    let bundled = project.join("entry/src/main/resources/rawfile/bundled");
    for generated in [&libs, &bundled] {
        match fs::remove_dir_all(generated) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let core_library = &bundled_core.recipe.library;
    let core_artifact = bundled_core.artifact;
    let mut native = BTreeMap::from([(MANAGER.to_owned(), result.library.clone())]);
    let manager_search = package::search_dirs(inv, result)?;
    for (library, name) in binary::closure(&result.library, &manager_search, target.rust_target())? {
        if &name == core_library {
            bail!("companion {name} is already in the Manager's native closure");
        }
        native.insert(name, library);
    }
    native.insert(core_library.clone(), core_artifact.library.clone());
    for (library, name) in binary::closure(
        &core_artifact.library,
        &package::search_dirs(inv, core_artifact)?,
        target.rust_target(),
    )? {
        if name == MANAGER {
            bail!("OHOS companion must not depend on the Manager library");
        }
        if let Some(existing) = native.get(&name) {
            if existing != &library && sha256(existing)? != sha256(&library)? {
                bail!("conflicting OHOS HAP dependency {name}");
            }
        } else {
            native.insert(name, library);
        }
    }
    let libs = libs.join(target.abi());
    fs::create_dir_all(&libs)?;
    let native: Vec<_> = native
        .into_iter()
        .map(|(name, path)| (path, name))
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
    entries.extend(package::static_core_entries(inv, result)?);
    let declaration = stage_core_manifest(inv, bundled_core, temp.path(), &mut entries)?;
    fs::create_dir_all(&bundled)?;
    copy_entries(&entries, &bundled)?;
    let mut names = entries
        .into_iter()
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    names.sort();
    // Read by ManagerBridge.ets `extractBundled`.
    fs::write(bundled.join("bundled.manifest"), names.join("\n") + "\n")?;
    Ok(declaration)
}

fn stage_core_manifest(
    inv: &Inventory,
    companion: &CompanionBuild<'_>,
    scratch: &Path,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<Vec<u8>> {
    let core = companion.core;
    let artifact = companion.artifact;
    let library = &companion.recipe.library;
    let id = &core.id;
    let build = artifact
        .core_build
        .as_ref()
        .context("OHOS companion build manifest")?;
    build.validate()?;
    if build.plugin_id != *id
        || build.target != artifact.target
        || !matches!(build.artifact_kind.as_str(), "rust-cdylib" | "native-shared")
        || build
            .native_library
            .as_ref()
            .is_some_and(|path| path != &artifact.library)
    {
        bail!(
            "wrong OHOS companion build manifest for {id}/{}",
            artifact.target
        );
    }
    let scratch = scratch.join(id);
    fs::create_dir(&scratch)?;
    let mut files = vec![package::FileRecord {
        path: library.clone(),
        sha256: sha256(&artifact.library)?,
    }];
    let mut data = package::licenses(inv, artifact, &scratch)?;
    for asset in build.licenses.iter().chain(&build.resources) {
        data.extend(package::asset_entries(asset)?);
    }
    if build.licenses.is_empty() {
        bail!("OHOS companion {id} must have license inputs");
    }
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
    let version = artifact
        .product_version
        .as_deref()
        .context("OHOS companion product version")?;
    shinki_emu_metadata::validate_core_version(version, None)?;
    let commit = artifact
        .source_commit
        .as_deref()
        .context("OHOS companion product source commit")?;
    let product_source = package::write_product_source_commit(&scratch, commit)?;
    entries.push((
        scratch.join(&product_source.path),
        format!("cores/{id}/{}", product_source.path),
    ));
    files.push(product_source);
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let declaration = serde_json::to_vec_pretty(&package::CorePackage {
        schema: "shinki.emu.core_package.v1".into(),
        plugin_id: id.clone(),
        library: library.clone(),
        version: version.into(),
        identity: None,
        recognition: core.recognition.clone(),
        abi: shinki_emu_build::ABI.into(),
        files,
    })?;
    let path = scratch.join("core.json");
    fs::write(&path, &declaration)?;
    entries.push((path, format!("cores/{id}/core.json")));
    Ok(declaration)
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

fn verify_hap(
    tools: &Tools,
    target: Target,
    hap: &Path,
    declaration: &[u8],
    manager_sha256: &str,
) -> Result<package::CorePackage> {
    let evidence = crate::temporary::directory()?;
    dependencies::run(
        sign_tool(tools)
            .args(["verify-app", "-inFile"])
            .arg(hap)
            .arg("-outCertChain")
            .arg(evidence.path().join("chain.cer"))
            .arg("-outProfile")
            .arg(evidence.path().join("profile.p7b")),
    )?;
    verify_archive(target, fs::File::open(hap)?, declaration, manager_sha256)
}

fn verify_archive(
    target: Target,
    hap: impl io::Read + io::Seek,
    declaration: &[u8],
    manager_sha256: &str,
) -> Result<package::CorePackage> {
    let abi = target.abi();
    let native_prefix = format!("{abi}/");
    let mut archive = zip::ZipArchive::new(hap)?;
    for name in archive.file_names() {
        if name?
            .strip_prefix("libs/")
            .is_some_and(|path| !path.is_empty() && !path.starts_with(&native_prefix))
        {
            bail!("signed HAP contains native libraries outside libs/{abi}/");
        }
    }
    let manifest: package::CorePackage = serde_json::from_slice(declaration)?;
    let prefix = format!("resources/rawfile/bundled/cores/{}", manifest.plugin_id);
    let library = &manifest.library;
    if library == MANAGER {
        bail!("OHOS Manager and companion must be separate libraries");
    }
    let mut signed_declaration = Vec::new();
    archive
        .by_name(&format!("{prefix}/core.json"))?
        .read_to_end(&mut signed_declaration)?;
    if signed_declaration != declaration {
        bail!("signed HAP companion declaration differs from the staged declaration");
    }
    let mut checked_library = false;
    for file in &manifest.files {
        if file.path == *library {
            verify_native_member(
                &mut archive,
                target,
                library,
                &file.sha256,
                NativeRole::Companion,
            )?;
            checked_library = true;
        } else {
            let name = format!("{prefix}/{}", file.path);
            if package::sha256_reader(archive.by_name(&name)?)? != file.sha256 {
                bail!("signed HAP content differs from companion declaration: {name}");
            }
        }
    }
    if !checked_library {
        bail!("signed HAP declaration lacks its companion library");
    }
    for required in [
        "resources/rawfile/bundled/bundled.manifest",
        "resources/rawfile/bundled/metadata/index.json",
        "module.json",
    ] {
        archive.by_name(required)?;
    }
    verify_native_member(
        &mut archive,
        target,
        MANAGER,
        manager_sha256,
        NativeRole::Manager,
    )?;
    Ok(manifest)
}

fn verify_native_member(
    archive: &mut zip::ZipArchive<impl io::Read + io::Seek>,
    target: Target,
    library: &str,
    expected_sha256: &str,
    role: NativeRole,
) -> Result<()> {
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
    if hex::encode(Sha256::digest(&bytes)) != expected_sha256 {
        bail!("signed HAP content differs from the staged library: {name}");
    }
    verify_native(target, &bytes, library, role)
}

enum NativeRole {
    Manager,
    Companion,
}

fn verify_native(target: Target, bytes: &[u8], name: &str, role: NativeRole) -> Result<()> {
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
    match role {
        NativeRole::Companion => {
            if !file.dynamic_symbols().any(|symbol| {
                symbol.is_global()
                    && symbol.kind() == object::SymbolKind::Text
                    && symbol.is_definition()
                    && symbol.name_bytes() == Ok(b"shinki_emu_query".as_slice())
            }) {
                bail!("{name} does not export shinki_emu_query");
            }
        }
        NativeRole::Manager
            if file.dynamic_symbols().chain(file.symbols()).any(|symbol| {
                symbol.is_definition() && symbol.name_bytes() == Ok(b"shinki_emu_query".as_slice())
            }) =>
        {
            bail!("{name} must not define a core query");
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    const ID: &str = "test.companion";
    const LIBRARY: &str = "libshinki_emu_fixture.so";

    // The verifier inspects ELF headers and the dynamic symbol table. Build
    // only those fields so the fixture runs on hosts without an OHOS SDK.
    fn elf(target: Target, exports_query: bool) -> Vec<u8> {
        const HEADERS: usize = 64;
        const PROGRAM_LEN: usize = 56;
        const STRINGS: usize = HEADERS + 2 * PROGRAM_LEN;
        const SYMBOLS: usize = 200;
        const SECTIONS: usize = 256;
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
        bytes[HEADERS + PROGRAM_LEN..][..4].copy_from_slice(&object::elf::PT_DYNAMIC.0.to_le_bytes());
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

    #[test]
    fn ohos_hap_checks_identity_abi_alignment_roles_and_digests() -> Result<()> {
        let commit = b"0123456789abcdef0123456789abcdef01234567";
        let prefix = format!("resources/rawfile/bundled/cores/{ID}");
        let digest = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
        for (target, other) in [(Target::Arm64, Target::X64), (Target::X64, Target::Arm64)] {
            let abi = target.abi();
            let core = elf(target, true);
            let manager = elf(target, false);
            let manager_sha256 = digest(&manager);
            let declaration = |core: &[u8]| {
                serde_json::to_vec(&serde_json::json!({
                    "schema": "shinki.emu.core_package.v1",
                    "plugin_id": ID,
                    "library": LIBRARY,
                    "version": "1.0.0",
                    "abi": shinki_emu_build::ABI,
                    "files": [
                        {"path": LIBRARY, "sha256": digest(core)},
                        {"path": "source-commit.txt", "sha256": digest(commit)},
                    ],
                }))
            };
            let expected = declaration(&core)?;
            let page = u16::try_from(target.page_size())?;
            let stored = zip::CompressionMethod::Stored;
            let create_archive =
                |alignment, compression, mixed, core: Option<&[u8]>, source: &[u8], signed: &[u8]|
                 -> Result<_> {
                    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
                    let files = SimpleFileOptions::default().compression_method(stored);
                    archive.start_file(format!("libs/{abi}/{MANAGER}"), files.with_alignment(page))?;
                    archive.write_all(&manager)?;
                    if let Some(core) = core {
                        archive.start_file(
                            format!("libs/{abi}/{LIBRARY}"),
                            files.with_alignment(alignment).compression_method(compression),
                        )?;
                        archive.write_all(core)?;
                    }
                    if mixed {
                        archive.start_file(format!("libs/{}/libc++_shared.so", other.abi()), files)?;
                    }
                    for path in [
                        "module.json",
                        "resources/rawfile/bundled/bundled.manifest",
                        "resources/rawfile/bundled/metadata/index.json",
                    ] {
                        archive.start_file(path, files)?;
                    }
                    archive.start_file(format!("{prefix}/core.json"), files)?;
                    archive.write_all(signed)?;
                    archive.start_file(format!("{prefix}/source-commit.txt"), files)?;
                    archive.write_all(source)?;
                    let mut hap = archive.finish()?;
                    hap.set_position(0);
                    Ok(hap)
                };
            let good = create_archive(page, stored, false, Some(&core), commit, &expected)?;
            let manifest = verify_archive(target, good.clone(), &expected, &manager_sha256)?;
            assert_eq!(
                (manifest.plugin_id.as_str(), manifest.version.as_str()),
                (ID, "1.0.0")
            );
            let error = |target, hap, staged: &[u8], manager: &str| {
                verify_archive(target, hap, staged, manager).unwrap_err().to_string()
            };
            assert!(
                error(other, good.clone(), &expected, &manager_sha256).contains("outside libs/")
            );
            assert!(error(target, good, &expected, &digest(&core)).contains("staged library"));
            let mixed = create_archive(page, stored, true, Some(&core), commit, &expected)?;
            assert!(error(target, mixed, &expected, &manager_sha256).contains("outside libs/"));
            for (alignment, compression) in [(1, stored), (page, zip::CompressionMethod::Deflated)] {
                let hap = create_archive(alignment, compression, false, Some(&core), commit, &expected)?;
                assert!(
                    error(target, hap, &expected, &manager_sha256).contains("stored uncompressed")
                );
            }
            for (bytes, reason) in [
                (b"not ELF".to_vec(), "not a loadable"),
                (elf(other, true), "not a loadable"),
                (manager.clone(), "does not export"),
            ] {
                let staged = declaration(&bytes)?;
                let hap = create_archive(page, stored, false, Some(&bytes), commit, &staged)?;
                assert!(error(target, hap, &staged, &manager_sha256).contains(reason));
            }
            assert!(verify_native(target, &core, MANAGER, NativeRole::Manager).is_err());
            let missing = create_archive(page, stored, false, None, commit, &expected)?;
            assert!(verify_archive(target, missing, &expected, &manager_sha256).is_err());
            let changed_dso = create_archive(page, stored, false, Some(&manager), commit, &expected)?;
            assert!(error(target, changed_dso, &expected, &manager_sha256).contains("staged library"));
            let changed_commit =
                create_archive(page, stored, false, Some(&core), &[b'a'; 40], &expected)?;
            assert!(
                error(target, changed_commit, &expected, &manager_sha256)
                    .contains("source-commit.txt")
            );
            let mut changed_manifest: serde_json::Value = serde_json::from_slice(&expected)?;
            changed_manifest["version"] = "1.0.1".into();
            let signed = serde_json::to_vec(&changed_manifest)?;
            let hap = create_archive(page, stored, false, Some(&core), commit, &signed)?;
            assert!(
                error(target, hap, &expected, &manager_sha256).contains("declaration differs")
            );
        }
        Ok(())
    }
}
