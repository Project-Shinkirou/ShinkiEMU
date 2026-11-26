use crate::{
    binary,
    build::{self, BuildArtifact},
    cores::{Core, Inventory},
    package::{self, CorePackage, FileRecord},
};
use anyhow::{Context, Result, bail, ensure};
use object::{Object, ObjectKind, macho, read::macho::MachHeader};
use serde::{Deserialize, Serialize};
use shinki_emu_build::BuildAsset;
use shinki_emu_metadata::CoreIdentity;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const TARGETS: [&str; 3] = [
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "x86_64-apple-ios",
];
const APP_ID: &str = "org.shinkirou.shinkiemu";
pub(crate) const MANIFEST: &str = "ios.json";
const SCHEMA: &str = "shinki.emu.ios_package.v1";

pub(crate) fn target_supported(target: &str) -> bool {
    TARGETS.contains(&target)
}

pub(crate) fn device(target: &str) -> bool {
    target == TARGETS[0]
}

pub(crate) fn validate_inventory(inv: &Inventory) -> Result<()> {
    let has_manager = inv
        .targets
        .iter()
        .any(|target| target.manager_supported && target_supported(&target.triple));
    ensure!(
        !has_manager || inv.cores.iter().any(|core| core.ios_framework),
        "iOS Manager requires at least one declared Framework"
    );
    Ok(())
}

pub(crate) fn require_native_host() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("iOS Framework packaging requires native macOS and Xcode");
    }
    Ok(())
}

pub(crate) fn install_name(id: &str) -> String {
    format!("@rpath/{id}.framework/{id}")
}

pub(crate) struct Signing {
    identity: String,
    keychain: Option<PathBuf>,
    profile: Option<PathBuf>,
    entitlements: Option<PathBuf>,
    developer_certificates: Vec<Vec<u8>>,
}

impl Signing {
    pub(crate) fn for_target(target: &str) -> Result<Self> {
        require_native_host()?;
        if !device(target) {
            return Ok(Self {
                identity: "-".into(),
                keychain: None,
                profile: None,
                entitlements: None,
                developer_certificates: Vec::new(),
            });
        }
        let identity = env::var("SHINKI_EMU_IOS_SIGN_IDENTITY")
            .context("set SHINKI_EMU_IOS_SIGN_IDENTITY to an Apple Development signing identity")?;
        ensure!(
            !identity.trim().is_empty() && identity != "-",
            "iOS device tests require a provisioned Apple Development identity"
        );
        let file = |var| -> Result<PathBuf> {
            let path = PathBuf::from(env::var_os(var).with_context(|| format!("set {var}"))?);
            ensure!(path.is_file(), "{var} must name an existing file");
            Ok(path)
        };
        let keychain = env::var_os("SHINKI_EMU_IOS_KEYCHAIN")
            .map(|_| file("SHINKI_EMU_IOS_KEYCHAIN"))
            .transpose()?;
        let profile = file("SHINKI_EMU_IOS_PROVISIONING_PROFILE")?;
        let entitlements = file("SHINKI_EMU_IOS_ENTITLEMENTS")?;
        let developer_certificates = development_certificates(&profile, &entitlements)?;
        Ok(Self {
            identity,
            keychain,
            profile: Some(profile),
            entitlements: Some(entitlements),
            developer_certificates,
        })
    }

    fn sign(&self, path: &Path, app: bool) -> Result<()> {
        let mut command = Command::new("codesign");
        command.args(["--force", "--timestamp=none", "--sign", &self.identity]);
        if let Some(keychain) = &self.keychain {
            command.arg("--keychain").arg(keychain);
        }
        if app && let Some(entitlements) = &self.entitlements {
            command.arg("--entitlements").arg(entitlements);
        }
        // Do not serialize the command or signing diagnostics into CI reports.
        let output = command.arg(path).output().context("launching codesign")?;
        ensure!(
            output.status.success(),
            "codesign failed for {} ({})",
            path.display(),
            output.status
        );
        let mut verify = Command::new("codesign");
        verify.args(["--verify", "--strict"]);
        if app {
            verify.arg("--deep");
        }
        super::checked(verify.arg(path))?;
        Ok(())
    }

    pub(super) fn sign_app(&self, app: &Path) -> Result<()> {
        self.sign(app, true)?;
        if self.profile.is_some() {
            let temp = tempfile::tempdir()?;
            let prefix = temp.path().join("certificate");
            let mut extract = std::ffi::OsString::from("--extract-certificates=");
            extract.push(prefix);
            let output = Command::new("codesign")
                .args(["--display", "--verbose=2"])
                .arg(extract)
                .arg(app)
                .output()?;
            ensure!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stderr).lines().any(|line| {
                        line.starts_with("Authority=Apple Development:")
                            || line.starts_with("Authority=iPhone Developer:")
                    }),
                "iOS device test apps must be signed with an Apple Development certificate"
            );
            let leaf = fs::read(temp.path().join("certificate0"))
                .context("cannot extract the device app's signing certificate")?;
            ensure!(
                self.developer_certificates.contains(&leaf),
                "the iOS Development signing certificate is not authorized by the provisioning profile"
            );
        }
        Ok(())
    }
}

fn development_certificates(profile: &Path, entitlements: &Path) -> Result<Vec<Vec<u8>>> {
    use base64::Engine;
    use std::io::Write;
    let decoded = Command::new("security")
        .args(["cms", "-D", "-i"])
        .arg(profile)
        .output()?;
    ensure!(
        decoded.status.success(),
        "cannot decode the iOS development profile"
    );
    let mut plist = tempfile::NamedTempFile::new()?;
    plist.write_all(&decoded.stdout)?;
    let read = |path: &Path, key: &str| -> Result<String> {
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Print :{key}")])
            .arg(path)
            .output()?;
        ensure!(
            output.status.success(),
            "missing {key} in iOS device signing inputs"
        );
        Ok(String::from_utf8(output.stdout)?.trim_end().to_owned())
    };
    ensure!(
        !read(plist.path(), "ProvisionedDevices:0")?.is_empty()
            && read(plist.path(), "Entitlements:get-task-allow")? == "true"
            && read(entitlements, "get-task-allow")? == "true",
        "iOS device tests require a device-listed development profile and development entitlements"
    );
    let team = read(plist.path(), "TeamIdentifier:0")?;
    let app = read(plist.path(), "Entitlements:application-identifier")?;
    let requested = read(entitlements, "application-identifier")?;
    ensure!(
        (app == requested
            || app
                .strip_suffix('*')
                .is_some_and(|prefix| requested.starts_with(prefix)))
            && requested
                .split_once('.')
                .is_some_and(|(_, id)| id == APP_ID)
            && read(entitlements, "com.apple.developer.team-identifier")? == team,
        "iOS development profile and entitlements must authorize {APP_ID} on the same team"
    );
    let extract = |key: &str| -> Result<Option<String>> {
        let output = Command::new("plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(plist.path())
            .output()?;
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout))
            .transpose()
            .context("invalid provisioning profile plist")
    };
    let expires =
        extract("ExpirationDate")?.context("missing development profile expiration date")?;
    ensure!(
        chrono::DateTime::parse_from_rfc3339(expires.trim())
            .context("invalid development profile expiration date")?
            > chrono::Utc::now(),
        "the iOS development profile has expired"
    );
    let mut certificates = Vec::new();
    while let Some(value) = extract(&format!("DeveloperCertificates.{}", certificates.len()))? {
        certificates.push(
            base64::engine::general_purpose::STANDARD
                .decode(value.trim())
                .context("invalid development certificate in the provisioning profile")?,
        );
    }
    ensure!(
        !certificates.is_empty(),
        "the iOS development profile contains no signing certificate"
    );
    Ok(certificates)
}

fn validate_image(path: &Path, target: &str, framework_id: Option<&str>) -> Result<()> {
    package::plain(path)?;
    let bytes = fs::read(path)?;
    let object::File::MachO64(image) = object::File::parse(bytes.as_slice())? else {
        bail!("expected a thin Mach-O64 image");
    };
    let arch = if target.starts_with("aarch64") {
        object::Architecture::Aarch64
    } else {
        object::Architecture::X86_64
    };
    let platform = if device(target) {
        macho::PLATFORM_IOS
    } else {
        macho::PLATFORM_IOSSIMULATOR
    };
    ensure!(
        image.architecture() == arch,
        "wrong Mach-O architecture for {target}"
    );
    let kind = if framework_id.is_some() {
        ObjectKind::Dynamic
    } else {
        ObjectKind::Executable
    };
    ensure!(image.kind() == kind, "wrong Mach-O image type");

    let expected_name = framework_id.map(install_name);
    let endian = image.endian();
    let mut commands = image
        .macho_header()
        .load_commands(endian, image.data(), 0)?;
    let (mut target_found, mut name_found) = (false, false);
    while let Some(command) = commands.next()? {
        if let Some((version, _)) = command.build_version(endian)? {
            ensure!(
                !target_found
                    && version.platform.get(endian) == platform
                    && version.minos.get(endian) <= macho::Version::new(16, 0, 0),
                "wrong Mach-O platform or deployment target for {target}"
            );
            target_found = true;
        }
        if command.cmd() == macho::LC_ID_DYLIB {
            let id: &macho::DylibCommand<_> = command.data()?;
            ensure!(
                !name_found
                    && expected_name.as_deref().map(str::as_bytes)
                        == Some(command.string(endian, id.dylib.name)?),
                "wrong Framework install name"
            );
            name_found = true;
        }
        if let Some(dependency) = command.dylib()? {
            let name = std::str::from_utf8(command.string(endian, dependency.dylib.name)?)?;
            ensure!(
                binary::system(name, target),
                "non-system dylib dependency {name}; iOS Frameworks must link private dependencies into their own image"
            );
        }
    }
    ensure!(
        target_found && name_found == framework_id.is_some(),
        "missing required Mach-O load command"
    );
    if framework_id.is_some() {
        for export in image.exports()? {
            let export = export?;
            if export.name() == object::NameOrOrdinal::Name(b"_shinki_emu_query".as_slice())
                && matches!(export.target(), object::ExportTarget::Address { .. })
            {
                return Ok(());
            }
        }
        bail!("Framework does not export shinki_emu_query");
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Framework {
    version: String,
    identity: Option<CoreIdentity>,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    schema: String,
    source_commit: String,
    version: String,
    target: String,
    distribution: String,
    frameworks: BTreeMap<String, Framework>,
}

fn distribution(target: &str) -> &'static str {
    if device(target) {
        "development-device-test"
    } else {
        "ad-hoc-simulator-test"
    }
}

fn core_version(identity: Option<&CoreIdentity>, version: &str) -> String {
    identity.map_or_else(|| version.to_owned(), CoreIdentity::version)
}

impl Manifest {
    pub(crate) fn validate(
        &self,
        inv: &Inventory,
        target: &str,
        version: &str,
        commit: &str,
    ) -> Result<()> {
        ensure!(
            self.distribution == distribution(target),
            "iOS package must be signed for its test distribution channel"
        );
        self.validate_build(inv, target, version, commit)
    }

    fn validate_build(
        &self,
        inv: &Inventory,
        target: &str,
        version: &str,
        commit: &str,
    ) -> Result<()> {
        ensure!(
            target_supported(target)
                && self.schema == SCHEMA
                && self.source_commit == commit
                && self.version == version
                && self.target == target,
            "iOS package does not match the exact-head test row {version}/{target}/{commit}"
        );
        let mut expected = inv.cores.iter().filter(|core| core.ios_framework);
        ensure!(
            self.frameworks.len() == expected.clone().count()
                && expected.all(|core| self.frameworks.contains_key(&core.id)),
            "missing or unexpected iOS Frameworks in manifest"
        );
        for (id, entry) in &self.frameworks {
            let identity = inv.core(id)?.identity(&inv.root)?;
            ensure!(
                entry.identity == identity
                    && entry.version == core_version(identity.as_ref(), version)
                    && entry.sha256.len() == 64
                    && entry
                        .sha256
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "invalid iOS Framework identity or hash for {id}"
            );
        }
        Ok(())
    }

    pub(crate) fn verify_release_archive(&self, path: &Path) -> Result<()> {
        let mut zip = zip::ZipArchive::new(fs::File::open(path)?)?;
        self.verify_manifest_entry(&mut zip, MANIFEST)?;
        if device(&self.target) {
            let ipa = zip.by_name_seek(&device_filename(&self.target, &self.version))?;
            self.verify_app(&mut zip::ZipArchive::new(ipa)?, "Payload/ShinkiEMU.app")
        } else {
            self.verify_app(&mut zip, "ShinkiEMU-simulator-test.app")
        }
    }

    fn verify_manifest_entry<R: Read + Seek>(
        &self,
        zip: &mut zip::ZipArchive<R>,
        path: &str,
    ) -> Result<()> {
        let entry = zip.by_name(path)?;
        ensure!(
            entry.is_file() && !entry.is_symlink() && entry.size() <= 64 * 1024,
            "invalid iOS package manifest entry {path}"
        );
        ensure!(
            serde_json::from_reader::<_, Self>(entry)? == *self,
            "archived iOS manifest differs from release record"
        );
        Ok(())
    }

    fn verify_app<R: Read + Seek>(&self, zip: &mut zip::ZipArchive<R>, app: &str) -> Result<()> {
        self.verify_manifest_entry(zip, &format!("{app}/{MANIFEST}"))?;
        let exists = |zip: &mut zip::ZipArchive<R>, name: &str| -> Result<()> {
            let entry = zip
                .by_name(name)
                .with_context(|| format!("missing iOS bundle entry {name}"))?;
            ensure!(
                entry.is_file() && !entry.is_symlink() && entry.size() > 0,
                "invalid iOS bundle entry {name}"
            );
            Ok(())
        };
        exists(zip, &format!("{app}/shinki-emu-manager"))?;
        exists(zip, &format!("{app}/_CodeSignature/CodeResources"))?;
        if device(&self.target) {
            exists(zip, &format!("{app}/embedded.mobileprovision"))?;
        }
        let prefix = format!("{app}/Frameworks/");
        let mut actual = BTreeSet::new();
        for path in zip.file_names() {
            let path = path?;
            if let Some(leaf) = path
                .strip_prefix(&prefix)
                .and_then(|path| path.split('/').next())
                && !leaf.is_empty()
            {
                actual.insert(leaf.to_owned());
            }
        }
        let expected: BTreeSet<_> = self
            .frameworks
            .keys()
            .map(|id| format!("{id}.framework"))
            .collect();
        ensure!(
            actual == expected,
            "archived iOS Framework inventory differs from the manifest"
        );
        for (id, record) in &self.frameworks {
            let framework = format!("{app}/Frameworks/{id}.framework");
            exists(zip, &format!("{framework}/Info.plist"))?;
            exists(zip, &format!("{framework}/_CodeSignature/CodeResources"))?;
            exists(zip, &format!("{app}/cores/{id}/core.json"))?;
            let image = zip
                .by_name(&format!("{framework}/{id}"))
                .with_context(|| format!("missing iOS Framework executable {id}"))?;
            ensure!(
                image.is_file()
                    && !image.is_symlink()
                    && image.size() > 0
                    && package::sha256_reader(image)? == record.sha256,
                "archived iOS Framework does not match the manifest: {id}"
            );
        }
        Ok(())
    }
}

pub(crate) fn device_filename(target: &str, version: &str) -> String {
    format!("ShinkiEMU-{version}-{target}-device-test.ipa")
}

pub(crate) fn read_manifest(
    inv: &Inventory,
    dir: &Path,
    target: &str,
    version: &str,
    commit: &str,
) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(dir.join(MANIFEST)).context("missing packaged iOS manifest")?,
    )?;
    manifest.validate(inv, target, version, commit)?;
    Ok(manifest)
}

pub(crate) fn build_device_closure(inv: &Inventory, version: &str) -> Result<Manifest> {
    let target = TARGETS[0];
    let manager = build::package_artifact(inv, target, "manager", version)?;
    validate_image(&manager.library, target, None)
        .context("validating the iOS Manager executable")?;
    let commit = manager.source_commit.context("iOS Manager source commit")?;
    let mut frameworks = BTreeMap::new();
    for core in inv.cores.iter().filter(|core| core.ios_framework) {
        let (artifact, identity) = build_framework(inv, core, target, version)?;
        let entry = Framework {
            version: core_version(identity.as_ref(), version),
            identity,
            sha256: package::sha256(&artifact.library)?,
        };
        frameworks.insert(core.id.clone(), entry);
    }
    let manifest = Manifest {
        schema: SCHEMA.into(),
        source_commit: commit.clone(),
        version: version.into(),
        target: target.into(),
        distribution: "unsigned-device-build".into(),
        frameworks,
    };
    manifest.validate_build(inv, target, version, &commit)?;
    Ok(manifest)
}

fn build_framework(
    inv: &Inventory,
    core: &Core,
    target: &str,
    version: &str,
) -> Result<(BuildArtifact, Option<CoreIdentity>)> {
    let result = (|| {
        let artifact = build::execute_ios_framework(inv, target, &core.id, version)?;
        validate_image(&artifact.library, target, Some(&core.id))?;
        let manifest = artifact
            .core_build
            .as_ref()
            .context("producer must report core-build.json")?;
        manifest.validate()?;
        ensure!(
            manifest.target == target,
            "producer reported the wrong target"
        );
        let identity = core.identity(&inv.root)?;
        ensure!(
            identity.is_some() == core.attribution.is_some(),
            "missing pinned adapter identity"
        );
        Ok((artifact, identity))
    })();
    result.with_context(|| format!("building iOS Framework {}/{target}", core.id))
}

pub(crate) fn stage(
    inv: &Inventory,
    manager: &BuildArtifact,
    stage: &Path,
    signing: &Signing,
    version: &str,
) -> Result<()> {
    let target = &manager.target;
    let commit = manager
        .source_commit
        .as_deref()
        .context("iOS Manager source commit")?;
    validate_image(&manager.library, target, None)
        .context("validating the iOS Manager executable")?;
    let frameworks = stage.join("Frameworks");
    fs::create_dir(&frameworks)?;
    let mut records = BTreeMap::new();
    for core in inv.cores.iter().filter(|core| core.ios_framework) {
        let record = stage_framework(inv, core, manager, stage, signing, version)
            .with_context(|| format!("iOS Framework {}/{target}", core.id))?;
        records.insert(core.id.clone(), record);
    }
    if let Some(profile) = &signing.profile {
        fs::copy(profile, stage.join("embedded.mobileprovision"))?;
    }
    let manifest = Manifest {
        schema: SCHEMA.into(),
        source_commit: commit.into(),
        version: version.into(),
        target: target.clone(),
        distribution: distribution(target).into(),
        frameworks: records,
    };
    manifest.validate(inv, target, version, commit)?;
    fs::write(stage.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

fn stage_framework(
    inv: &Inventory,
    core: &Core,
    manager: &BuildArtifact,
    stage: &Path,
    signing: &Signing,
    version: &str,
) -> Result<Framework> {
    let id = &core.id;
    let target = &manager.target;
    let (build, identity) = build_framework(inv, core, target, version)?;
    let manifest = build
        .core_build
        .as_ref()
        .context("producer must report core-build.json")?;

    let framework = stage.join("Frameworks").join(format!("{id}.framework"));
    fs::create_dir(&framework)?;
    let executable = framework.join(id);
    fs::copy(&build.library, &executable)?;

    let mut resources = Vec::new();
    for path in &core.resources {
        resources.extend(package::asset_entries(&BuildAsset {
            source: inv.root.join(path),
            dest: format!(
                "Resources/{}",
                Path::new(path)
                    .file_name()
                    .context("core resource filename")?
                    .to_string_lossy()
            ),
        })?);
    }
    for asset in manifest.resources.iter().chain(
        build
            .native_inputs
            .iter()
            .flat_map(|input| &input.package_files),
    ) {
        resources.extend(package::asset_entries(asset)?);
    }
    let subdir = core
        .ios_resource_subdir
        .as_ref()
        .map(|dir| framework.join(dir));
    package::copy_entries(&resources, subdir.as_deref().unwrap_or(&framework))?;

    let temp = tempfile::tempdir_in(stage)?;
    let mut notices = package::licenses(inv, &build, temp.path())?;
    let mut seen = BTreeSet::new();
    for asset in &manifest.licenses {
        seen.insert(fs::canonicalize(&asset.source)?);
        notices.extend(package::asset_entries(asset)?);
    }
    for (source, path) in core.license_files(&inv.root, target)? {
        if seen.insert(fs::canonicalize(&source)?) {
            notices.extend(package::asset_entries(&BuildAsset {
                source,
                dest: format!("licenses/inventory/{path}"),
            })?);
        }
    }
    package::copy_entries(&notices, &stage.join("licenses/frameworks").join(id))?;

    let platform = if device(target) {
        "iPhoneOS"
    } else {
        "iPhoneSimulator"
    };
    let parsed = semver::Version::parse(version)?;
    let short_version = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
    let bundle_id = id.replace('_', "-");
    fs::write(
        framework.join("Info.plist"),
        format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>",
                "<key>CFBundleExecutable</key><string>{}</string><key>CFBundleName</key><string>{}</string>",
                "<key>CFBundleIdentifier</key><string>{}.core.{}</string><key>CFBundlePackageType</key><string>FMWK</string>",
                "<key>CFBundleShortVersionString</key><string>{}</string><key>CFBundleVersion</key><string>1</string>",
                "<key>CFBundleSupportedPlatforms</key><array><string>{}</string></array>",
                "<key>MinimumOSVersion</key><string>16.0</string></dict></plist>"
            ),
            id, id, APP_ID, bundle_id, short_version, platform
        ),
    )?;
    signing.sign(&framework, false)?;

    // App-sealed sidecars hash the independently signed executable outside their own seal.
    let dir = stage.join("cores").join(id);
    fs::create_dir_all(&dir)?;
    let sha256 = package::sha256(&executable)?;
    let mut files = vec![FileRecord {
        path: id.clone(),
        sha256: sha256.clone(),
    }];
    if identity.is_none() {
        let commit = build
            .source_commit
            .as_deref()
            .context("Framework product source commit")?;
        ensure!(
            Some(commit) == manager.source_commit.as_deref(),
            "Framework and Manager source commits differ"
        );
        files.push(package::write_product_source_commit(&dir, commit)?);
    }
    let version = core_version(identity.as_ref(), version);
    let sidecar = CorePackage {
        schema: "shinki.emu.core_package.v1".into(),
        plugin_id: id.clone(),
        library: id.clone(),
        version: version.clone(),
        identity: identity.clone(),
        recognition: core.recognition.clone(),
        abi: shinki_emu_build::ABI.into(),
        files,
    };
    fs::write(dir.join("core.json"), serde_json::to_vec_pretty(&sidecar)?)?;
    Ok(Framework {
        version,
        identity,
        sha256,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn manifest_for(inv: &Inventory, target: &str, version: &str, commit: &str) -> Manifest {
        Manifest {
            schema: SCHEMA.into(),
            source_commit: commit.into(),
            version: version.into(),
            target: target.into(),
            distribution: distribution(target).into(),
            frameworks: inv
                .cores
                .iter()
                .filter(|core| core.ios_framework)
                .map(|core| {
                    let id = core.id.as_str();
                    let identity = core.identity(&inv.root).unwrap();
                    let version = core_version(identity.as_ref(), version);
                    let sha256 = package::sha256_reader(id.as_bytes()).unwrap();
                    (
                        id.into(),
                        Framework {
                            version,
                            identity,
                            sha256,
                        },
                    )
                })
                .collect(),
        }
    }

    fn inventory() -> Inventory {
        Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
    }

    #[test]
    fn release_manifest_rejects_missing_framework_and_wrong_head() {
        let mut inv = inventory();
        let mut manifest = manifest_for(&inv, TARGETS[0], "0.1.0", &"a".repeat(40));
        manifest
            .validate(&inv, TARGETS[0], "0.1.0", &"a".repeat(40))
            .unwrap();
        manifest.distribution = "unsigned-device-build".into();
        assert!(
            manifest
                .validate(&inv, TARGETS[0], "0.1.0", &"a".repeat(40))
                .is_err()
        );
        manifest.distribution = distribution(TARGETS[0]).into();
        assert!(
            manifest
                .validate(&inv, TARGETS[0], "0.1.0", &"b".repeat(40))
                .is_err()
        );
        let (missing, _) = manifest.frameworks.pop_first().unwrap();
        assert!(
            manifest
                .validate(&inv, TARGETS[0], "0.1.0", &"a".repeat(40))
                .is_err()
        );
        inv.cores.retain(|core| core.id != missing);
        manifest
            .validate(&inv, TARGETS[0], "0.1.0", &"a".repeat(40))
            .unwrap();
    }

    fn put(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    // Test archive verification only; actual code signing runs before archival.
    fn stage_app_fixture(app: &Path, manifest: &Manifest) {
        put(&app.join(MANIFEST), &serde_json::to_vec(manifest).unwrap());
        put(&app.join("shinki-emu-manager"), b"manager");
        put(&app.join("_CodeSignature/CodeResources"), b"app seal");
        if device(&manifest.target) {
            put(&app.join("embedded.mobileprovision"), b"device provision");
        }
        for id in manifest.frameworks.keys() {
            let dir = app.join("Frameworks").join(format!("{id}.framework"));
            put(&dir.join("Info.plist"), b"plist");
            put(&dir.join("_CodeSignature/CodeResources"), b"framework seal");
            put(&dir.join(id), id.as_bytes());
            put(
                &app.join("cores").join(id).join("core.json"),
                b"core manifest",
            );
        }
    }

    pub(crate) fn release_archive_fixture(
        inv: &Inventory,
        target: &str,
        version: &str,
        commit: &str,
        out: &Path,
    ) -> Manifest {
        let manifest = manifest_for(inv, target, version, commit);
        let temp = crate::temporary::directory().unwrap();
        let stage = temp.path().join("stage");
        put(
            &stage.join(MANIFEST),
            &serde_json::to_vec(&manifest).unwrap(),
        );
        if device(target) {
            let ipa_root = temp.path().join("ipa");
            stage_app_fixture(&ipa_root.join("Payload/ShinkiEMU.app"), &manifest);
            package::zip_entries(
                &package::tree(&ipa_root).unwrap(),
                &stage.join(device_filename(target, version)),
            )
            .unwrap();
        } else {
            stage_app_fixture(&stage.join("ShinkiEMU-simulator-test.app"), &manifest);
        }
        package::zip_entries(&package::tree(&stage).unwrap(), out).unwrap();
        manifest
    }

    #[test]
    fn archived_framework_inventory_and_content_are_verified() {
        let manifest = manifest_for(&inventory(), TARGETS[2], "0.1.0", &"a".repeat(40));
        let temp = crate::temporary::directory().unwrap();
        let root = temp.path();
        let stage = root.join("stage");
        let app = stage.join("ShinkiEMU-simulator-test.app");
        put(
            &stage.join(MANIFEST),
            &serde_json::to_vec(&manifest).unwrap(),
        );
        stage_app_fixture(&app, &manifest);
        let archive = |name| {
            let out = root.join(name);
            package::zip_entries(&package::tree(&stage).unwrap(), &out).unwrap();
            out
        };
        manifest
            .verify_release_archive(&archive("complete.zip"))
            .unwrap();
        let image = app.join("Frameworks/krkr2.framework/krkr2");
        put(&image, b"wrong framework");
        assert!(
            manifest
                .verify_release_archive(&archive("changed.zip"))
                .is_err()
        );
        fs::remove_file(image).unwrap();
        assert!(
            manifest
                .verify_release_archive(&archive("missing.zip"))
                .is_err()
        );
    }
}
