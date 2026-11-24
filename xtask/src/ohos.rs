//! HarmonyOS/OpenHarmony Manager HAP: stage the Rust cdylib and bundled
//! resources into `Platforms/OHOS`, build with hvigor, sign with the SDK's
//! hap-sign-tool and verify the signed archive.
use crate::{
    binary,
    build::BuildArtifact,
    cores::Inventory,
    dependencies,
    package::{self, atomic_directory, copy_entries, sha256},
};
use anyhow::{Context, Result, bail};
use shinki_emu_build::ohos::Target;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const BUNDLE_NAME: &str = "org.shinkirou.shinkiemu";
const MANAGER: &str = "libshinki_emu_manager.so";
const MUSICA: &str = "shinki.emu.musica";
const CORE_LIBRARY: &str = "libshinki_emu_core_musica.so";
const FEATURE: &str = "ohos-bundled-musica";

pub(crate) fn is_core_input(component: &str, target: &str) -> bool {
    component == MUSICA && Target::from_rust_target(target).is_some()
}

pub(crate) fn bundles_core(component: &str, target: &str, features: &[String]) -> bool {
    is_core_input(component, target) && features.iter().any(|feature| feature == FEATURE)
}

pub(crate) fn manager_mode(target: &str, features: &[String]) -> Result<bool> {
    let enabled = features.iter().any(|feature| feature == FEATURE);
    if enabled && Target::from_rust_target(target).is_none() {
        bail!("{FEATURE} is only supported on OHOS ARM64 and x86_64");
    }
    Ok(enabled)
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

fn hap_name(target: Target, version: &str, bundled: bool) -> String {
    let qa = if target.local_qa() { "-local-qa" } else { "" };
    let variant = if bundled { "-bundled-musica" } else { "" };
    format!(
        "ShinkiEMU-{version}-ohos-{}{qa}{variant}-debug-test-signed.hap",
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
    if result.target != triple {
        bail!("OHOS Manager artifact target {} does not match {triple}", result.target);
    }
    let musica = manager_mode(triple, &result.features)?
        .then(|| crate::build::execute_ohos_hap(inv, triple, MUSICA, version))
        .transpose()?;
    let tools = tools()?;
    let signing = signing_material(&tools)?;
    let project = inv.root.join("Platforms/OHOS");
    atomic_directory(out, |stage| {
        let declaration = stage_inputs(inv, target, result, musica.as_ref(), &tools, &project)?;
        let unsigned = build_hap(&tools, target, &project, version)?;
        let name = hap_name(target, version, musica.is_some());
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
        verify_hap(&tools, target, &hap, declaration.as_deref())?;
        let record = serde_json::json!({
            "hap": name,
            "sha256": sha256(&hap)?,
            "target": triple,
            "abi": target.abi(),
            "local_qa": target.local_qa(),
            "version": version,
            "source_commit": result.source_commit.as_deref().context("OHOS Manager source commit")?,
            "features": result.features,
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
    musica: Option<&BuildArtifact>,
    tools: &Tools,
    project: &Path,
) -> Result<Option<Vec<u8>>> {
    let libs = project.join("entry/libs");
    let bundled = project.join("entry/src/main/resources/rawfile/bundled");
    for generated in [&libs, &bundled] {
        match fs::remove_dir_all(generated) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut native = BTreeMap::from([(MANAGER.to_owned(), result.library.clone())]);
    let manager_search = package::search_dirs(inv, result)?;
    for (library, name) in binary::closure(&result.library, &manager_search, target.rust_target())? {
        native.insert(name, library);
    }
    if let Some(musica) = musica {
        if native.insert(CORE_LIBRARY.to_owned(), musica.library.clone()).is_some() {
            bail!("bundled Musica is already in the Manager's native closure");
        }
        for (library, name) in
            binary::closure(&musica.library, &package::search_dirs(inv, musica)?, target.rust_target())?
        {
            if let Some(existing) = native.get(&name) {
                if existing != &library && sha256(existing)? != sha256(&library)? {
                    bail!("conflicting OHOS HAP dependency {name}");
                }
            } else {
                native.insert(name, library);
            }
        }
    }
    let libs = libs.join(target.abi());
    fs::create_dir_all(&libs)?;
    let native: Vec<_> = native.into_iter().map(|(name, path)| (path, name)).collect();
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
    let declaration = if let Some(musica) = musica {
        Some(stage_core_manifest(
            inv,
            musica,
            temp.path(),
            &mut entries,
        )?)
    } else {
        None
    };
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
    musica: &BuildArtifact,
    scratch: &Path,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<Vec<u8>> {
    let core = inv.core(MUSICA)?;
    let build = musica.core_build.as_ref().context("bundled Musica build manifest")?;
    let scratch = scratch.join("musica");
    fs::create_dir(&scratch)?;
    let mut files = vec![package::FileRecord {
        path: CORE_LIBRARY.into(),
        sha256: sha256(&musica.library)?,
    }];
    let mut data = package::licenses(inv, musica, &scratch)?;
    for asset in build.licenses.iter().chain(&build.resources) {
        data.extend(package::asset_entries(asset)?);
    }
    if build.licenses.is_empty() {
        bail!("bundled Musica must have license inputs");
    }
    for (source, relative) in data {
        if Path::new(&relative).extension().is_some_and(|extension| extension == "so") {
            bail!("native code may not be staged in the HAP's rawfile data");
        }
        files.push(package::FileRecord { path: relative.clone(), sha256: sha256(&source)? });
        entries.push((source, format!("cores/{MUSICA}/{relative}")));
    }
    let version = musica.product_version.as_deref().context("bundled Musica product version")?;
    shinki_emu_metadata::validate_core_version(version, None)?;
    let commit = musica.source_commit.as_deref().context("bundled Musica product source commit")?;
    let product_source = package::write_product_source_commit(&scratch, commit)?;
    entries.push((scratch.join(&product_source.path), format!("cores/{MUSICA}/{}", product_source.path)));
    files.push(product_source);
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let declaration = serde_json::to_vec_pretty(&package::CorePackage {
        schema: "shinki.emu.core_package.v1".into(),
        plugin_id: MUSICA.into(),
        library: CORE_LIBRARY.into(),
        version: version.into(),
        identity: None,
        recognition: core.recognition.clone(),
        abi: shinki_emu_build::ABI.into(),
        files,
    })?;
    let path = scratch.join("core.json");
    fs::write(&path, &declaration)?;
    entries.push((path, format!("cores/{MUSICA}/core.json")));
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

fn verify_hap(tools: &Tools, target: Target, hap: &Path, declaration: Option<&[u8]>) -> Result<()> {
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
    verify_archive(target, fs::File::open(hap)?, declaration)
}

fn verify_archive(
    target: Target,
    hap: impl io::Read + io::Seek,
    declaration: Option<&[u8]>,
) -> Result<()> {
    let abi = target.abi();
    let native_prefix = format!("{abi}/");
    let mut archive = zip::ZipArchive::new(hap)?;
    for name in archive.file_names() {
        if name?.strip_prefix("libs/")
            .is_some_and(|path| !path.is_empty() && !path.starts_with(&native_prefix))
        {
            bail!("signed HAP contains native libraries outside libs/{abi}/");
        }
    }
    let member = format!("libs/{abi}/{CORE_LIBRARY}");
    if declaration.is_some() != archive.by_name(&member).is_ok() {
        bail!("signed HAP contains an unexpected bundled-core roster");
    }
    if let Some(declaration) = declaration {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let prefix = format!("resources/rawfile/bundled/cores/{MUSICA}");
        let signed_declaration = {
            let mut bytes = Vec::new();
            archive.by_name(&format!("{prefix}/core.json"))?.read_to_end(&mut bytes)?;
            bytes
        };
        if signed_declaration.as_slice() != declaration {
            bail!("signed HAP bundled Musica declaration differs from the staged declaration");
        }
        let manifest: package::CorePackage = serde_json::from_slice(declaration)?;
        for file in manifest.files {
            let name = if file.path == CORE_LIBRARY {
                member.clone()
            } else {
                format!("{prefix}/{}", file.path)
            };
            let mut entry = archive.by_name(&name)?;
            if name == member
                && (entry.compression() != zip::CompressionMethod::Stored
                    || entry.data_start().is_none_or(|offset| offset % target.page_size() != 0))
            {
                bail!("{member} must be stored uncompressed on a {}-byte page boundary", target.page_size());
            }
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 8192];
            loop {
                let len = entry.read(&mut buffer)?;
                if len == 0 {
                    break;
                }
                digest.update(&buffer[..len]);
            }
            if hex::encode(digest.finalize()) != file.sha256 {
                bail!("signed HAP content differs from bundled Musica declaration: {name}");
            }
        }
    }
    for required in [
        format!("libs/{abi}/{MANAGER}"),
        "resources/rawfile/bundled/bundled.manifest".to_owned(),
        "resources/rawfile/bundled/metadata/index.json".to_owned(),
        "module.json".to_owned(),
    ] {
        archive.by_name(&required)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    #[test]
    fn ohos_hap_checks_the_selected_abi_and_lazy_load_alignment() -> Result<()> {
        let core = b"bundled library bytes";
        let declaration = serde_json::to_vec(&serde_json::json!({
            "schema": "shinki.emu.core_package.v1",
            "plugin_id": MUSICA,
            "library": CORE_LIBRARY,
            "version": "1.0.0",
            "abi": shinki_emu_build::ABI,
            "files": [{"path": CORE_LIBRARY, "sha256": hex::encode(Sha256::digest(core))}],
        }))?;
        for (target, other) in [(Target::Arm64, Target::X64), (Target::X64, Target::Arm64)] {
            let abi = target.abi();
            let create_archive = |alignment, compression, mixed| -> Result<_> {
                let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
                let stored = SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                let bundled = stored.with_alignment(alignment).compression_method(compression);
                archive.start_file(format!("libs/{abi}/{MANAGER}"), stored)?;
                archive.start_file(format!("libs/{abi}/{CORE_LIBRARY}"), bundled)?;
                archive.write_all(core)?;
                if mixed {
                    archive.start_file(format!("libs/{}/libc++_shared.so", other.abi()), stored)?;
                }
                for path in [
                    "module.json",
                    "resources/rawfile/bundled/bundled.manifest",
                    "resources/rawfile/bundled/metadata/index.json",
                ] {
                    archive.start_file(path, stored)?;
                }
                archive.start_file(
                    format!("resources/rawfile/bundled/cores/{MUSICA}/core.json"),
                    stored,
                )?;
                archive.write_all(&declaration)?;
                let mut hap = archive.finish()?;
                hap.set_position(0);
                Ok(hap)
            };
            let page = u16::try_from(target.page_size())?;
            let stored = zip::CompressionMethod::Stored;
            let hap = create_archive(page, stored, false)?;
            verify_archive(target, hap.clone(), Some(&declaration))?;
            assert!(verify_archive(other, hap.clone(), Some(&declaration)).is_err());
            assert!(verify_archive(target, hap, None).is_err());
            for hap in [
                create_archive(page, stored, true)?,
                create_archive(1, stored, false)?,
                create_archive(page, zip::CompressionMethod::Deflated, false)?,
            ] {
                assert!(verify_archive(target, hap, Some(&declaration)).is_err());
            }
        }
        Ok(())
    }
}
