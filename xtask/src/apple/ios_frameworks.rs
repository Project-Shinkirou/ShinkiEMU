use crate::{
    build::{self, BuildResult},
    cores::Inventory,
    package::{self, CorePackage, FileRecord},
};
use anyhow::{Context, Result, bail};
use object::{Object, ObjectSection, ObjectSymbol, read::macho::MachHeader};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const FEATURE: &str = "ios-framework-cores";
const APP_ID: &str = "org.shinkirou.shinkiemu";

pub(crate) fn target_supported(target: &str) -> bool {
    matches!(target, "aarch64-apple-ios" | "aarch64-apple-ios-sim" | "x86_64-apple-ios")
}

pub(crate) fn enabled(target: &str, features: &[String]) -> bool {
    target_supported(target) && features.iter().any(|feature| feature == FEATURE)
}

pub(crate) fn require_native_host() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("the private iOS Framework build requires native macOS and Xcode");
    }
    Ok(())
}

pub(crate) fn install_name(id: &str) -> String {
    format!("@rpath/{id}.framework/{id}")
}

#[derive(Deserialize, Serialize)]
struct BuildInputs {
    manager_sha256: String,
    cores: BTreeMap<String, Input>,
}

#[derive(Deserialize, Serialize)]
struct Input {
    build: BuildResult,
    sha256: String,
}

fn inputs_path(manager: &BuildResult) -> PathBuf {
    manager.target_dir.join("ios-framework-inputs.json")
}

pub(crate) fn build_inputs(inv: &Inventory, manager: &BuildResult) -> Result<()> {
    let mut inputs = BuildInputs {
        manager_sha256: package::sha256(&manager.library)?,
        cores: BTreeMap::new(),
    };
    for core in inv.cores.iter().filter(|core| core.ios_framework) {
        let build = build::execute_ios_framework(inv, &manager.target, &core.id)?;
        if build.source_commit != manager.source_commit || build.source_dirty != manager.source_dirty {
            bail!("{} and Manager were not built from the same source state", core.id);
        }
        let sha256 = package::sha256(&build.library)?;
        inputs.cores.insert(core.id.clone(), Input { build, sha256 });
    }
    if inputs.cores.is_empty() {
        bail!("the iOS Framework build has no Framework recipes");
    }
    let output = inputs_path(manager);
    let tmp = tempfile::NamedTempFile::new_in(&manager.target_dir)?;
    serde_json::to_writer(tmp.as_file(), &inputs)?;
    tmp.persist(output)?;
    Ok(())
}

pub(crate) fn inspect(path: &Path, target: &str, id: &str) -> Result<()> {
    let bytes = fs::read(path)?;
    let file = object::File::parse(bytes.as_slice())?;
    let (dependencies, _) = super::inspect_file(&file, path, target, false)?;
    if file.kind() != object::ObjectKind::Dynamic || !dependencies.is_empty() {
        bail!("{id} must be a Mach-O dylib with only system dependencies: {dependencies:?}");
    }
    let object::File::MachO64(macho) = &file else {
        bail!("{id} is not a thin 64-bit Mach-O");
    };
    let endian = macho.endian();
    let subtype = if target.starts_with("aarch64-") {
        object::macho::CPU_SUBTYPE_ARM64_ALL
    } else {
        object::macho::CPU_SUBTYPE_X86_64_ALL
    };
    if macho.macho_header().cpusubtype(endian).id() != subtype {
        bail!("{id} uses a CPU subtype outside the {target} slice");
    }
    let flags = macho.macho_header().flags(endian);
    if !flags.contains(object::macho::MH_TWOLEVEL) || flags.contains(object::macho::MH_FORCE_FLAT) {
        bail!("{id} must use Mach-O two-level symbol resolution");
    }
    let mut commands = macho.macho_header().load_commands(endian, macho.data(), 0)?;
    let mut id_matches = false;
    let mut has_build_version = false;
    let expected = install_name(id);
    while let Some(command) = commands.next()? {
        match command.cmd() {
            object::macho::LC_ID_DYLIB => {
                let dylib: &object::macho::DylibCommand<_> = command.data()?;
                if id_matches || command.string(endian, dylib.dylib.name)? != expected.as_bytes() {
                    bail!("{id} must have one install name: {expected}");
                }
                id_matches = true;
            }
            object::macho::LC_RPATH => bail!("{id} has an unnecessary runtime search path"),
            object::macho::LC_BUILD_VERSION => {
                let version: &object::macho::BuildVersionCommand<_> = command.data()?;
                if has_build_version || version.minos.get(endian) != object::macho::Version(0x10_00_00)
                {
                    bail!("{id} must target iOS 16.0");
                }
                has_build_version = true;
            }
            _ => {}
        }
    }
    if !has_build_version {
        bail!("{id} lacks a Mach-O platform and deployment version");
    }
    if !id_matches {
        bail!("{id} is missing its Framework install name");
    }
    let mut has_query = false;
    for export in file.exports()? {
        let export = export?;
        if !matches!(export.name(), object::read::NameOrOrdinal::Name(b"_shinki_emu_query")) {
            bail!("{id} exposes a non-ABI symbol: {:?}", export.name());
        }
        has_query = true;
    }
    if !has_query {
        bail!("{id} lacks shinki_emu_query");
    }
    // These offscreen cores own no UIKit classes. The sole Roukaku picker
    // delegate belongs to Manager; reject both native metadata and a linked
    // copy of that lazily registered Rust class, which dyld cannot isolate.
    for section in file.sections() {
        if section.size() != 0
            && matches!(
                section.name()?,
                "__objc_classlist" | "__objc_nlclslist" | "__objc_catlist" | "__objc_nlcatlist"
            )
        {
            bail!("{id} includes private Objective-C class/category metadata");
        }
    }
    if file.symbols().any(|symbol| {
        symbol.is_definition() && symbol.name().is_ok_and(|name| name.contains("PickerDelegate"))
    }) || file
        .section_by_name("__cstring")
        .map(|section| section.data())
        .transpose()?
        .is_some_and(|strings| strings.split(|byte| *byte == 0).any(|name| name == b"PickerDelegate"))
    {
        bail!("{id} contains the Manager's Roukaku UIKit picker delegate");
    }
    Ok(())
}

pub(crate) struct Signing {
    identity: String,
    profile: Option<PathBuf>,
    entitlements: Option<PathBuf>,
}

impl Signing {
    pub(crate) fn for_target(target: &str) -> Result<Self> {
        require_native_host()?;
        if super::sdk_name(target) == "iphonesimulator" {
            return Ok(Self { identity: "-".into(), profile: None, entitlements: None });
        }
        let identity = env::var("SHINKI_EMU_IOS_SIGN_IDENTITY")
            .context("set SHINKI_EMU_IOS_SIGN_IDENTITY to an Apple development signing identity")?;
        if identity.trim().is_empty() || identity == "-" {
            bail!("a private iOS device build needs a provisioned Apple signing identity");
        }
        let file = |var| -> Result<PathBuf> {
            let path = PathBuf::from(env::var_os(var).with_context(|| format!("set {var}"))?);
            let path = fs::canonicalize(&path).with_context(|| format!("reading {var}"))?;
            if !path.is_file() {
                bail!("{var} must name an existing file");
            }
            Ok(path)
        };
        Ok(Self {
            identity,
            profile: Some(file("SHINKI_EMU_IOS_PROVISIONING_PROFILE")?),
            entitlements: Some(file("SHINKI_EMU_IOS_ENTITLEMENTS")?),
        })
    }

    fn sign(&self, path: &Path, app: bool) -> Result<()> {
        let mut command = Command::new("codesign");
        command.args(["--force", "--timestamp=none", "--sign", &self.identity]);
        if app && let Some(entitlements) = &self.entitlements {
            command.arg("--entitlements").arg(entitlements);
        }
        super::checked(command.arg(path))?;
        let mut verify = Command::new("codesign");
        verify.args(["--verify", "--strict"]);
        if app {
            verify.arg("--deep");
        }
        super::checked(verify.arg(path))
    }

    pub(super) fn sign_app(&self, app: &Path) -> Result<()> {
        self.sign(app, true)
    }
}

#[derive(Deserialize)]
struct EmbeddedCore {
    id: String,
    version: String,
    identity: Option<shinki_emu_metadata::CoreIdentity>,
}

pub(crate) fn stage(
    inv: &Inventory,
    manager: &BuildResult,
    stage: &Path,
    signing: &Signing,
) -> Result<()> {
    let path = inputs_path(manager);
    let mut inputs: BuildInputs = serde_json::from_slice(&fs::read(&path)
        .with_context(|| format!("missing {}; rebuild the Framework Manager", path.display()))?)?;
    if inputs.manager_sha256 != package::sha256(&stage.join("shinki-emu-manager"))? {
        bail!("iOS Framework inputs belong to a different Manager binary");
    }
    if inputs.cores.is_empty() {
        bail!("the Manager receipt has no iOS Framework inputs");
    }
    let embedded: Vec<EmbeddedCore> = serde_json::from_slice(&fs::read(stage.join("embedded-cores.json"))?)?;
    let frameworks = stage.join("Frameworks");
    fs::create_dir(&frameworks)?;
    let platform = if super::sdk_name(&manager.target) == "iphoneos" { "iPhoneOS" } else { "iPhoneSimulator" };
    for core in inv.cores.iter().filter(|core| core.ios_framework) {
        let id = &core.id;
        let input = inputs.cores.remove(id).with_context(|| format!("missing {id} Framework input"))?;
        let build = &input.build;
        let manifest = build.core_build.as_ref().context("missing core build identity")?;
        manifest.validate()?;
        if &build.component != id || build.target != manager.target || build.profile != manager.profile
            || build.source_commit != manager.source_commit || build.source_dirty != manager.source_dirty
            || build.features != core.features || &manifest.plugin_id != id
            || manifest.target != manager.target || manifest.profile != manager.profile
            || manifest.artifact_kind != core.artifact_kind
            || !manifest.resources.is_empty()
        {
            bail!("{id} Framework build identity does not match Manager/recipe");
        }
        let embedded_core = embedded.iter().find(|entry| &entry.id == id).context("missing bundled core identity")?;
        let version = &embedded_core.version;
        shinki_emu_metadata::validate_core_version(version, embedded_core.identity.as_ref())?;
        let parsed = semver::Version::parse(version)?;
        let short_version = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
        let framework = frameworks.join(format!("{id}.framework"));
        fs::create_dir(&framework)?;
        let executable = framework.join(id);
        package::plain(&build.library)?;
        fs::copy(&build.library, &executable)?;
        if package::sha256(&executable)? != input.sha256 {
            bail!("{id} Framework input changed; rebuild the Manager");
        }
        inspect(&executable, &manager.target, id)?;
        fs::write(framework.join("Info.plist"), format!(concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>",
            "<key>CFBundleExecutable</key><string>{}</string><key>CFBundleName</key><string>{}</string>",
            "<key>CFBundleIdentifier</key><string>{}.core.{}</string><key>CFBundlePackageType</key><string>FMWK</string>",
            "<key>CFBundleShortVersionString</key><string>{}</string><key>CFBundleVersion</key><string>1</string>",
            "<key>CFBundleSupportedPlatforms</key><array><string>{}</string></array>",
            "<key>MinimumOSVersion</key><string>16.0</string></dict></plist>"
        ), id, id, APP_ID, id, short_version, platform))?;
        signing.sign(&framework, false)?;
        // The app seal covers this sidecar. It must be outside the Framework
        // whose signed executable it hashes, so the signatures have no cycle.
        let dir = stage.join("cores").join(id);
        fs::create_dir_all(&dir)?;
        let sidecar = CorePackage {
            schema: "shinki.emu.core_package.v1".into(),
            plugin_id: id.clone(),
            library: id.clone(),
            version: version.clone(),
            identity: embedded_core.identity.clone(),
            recognition: core.recognition.clone(),
            abi: manifest.abi.clone(),
            files: vec![FileRecord { path: id.clone(), sha256: package::sha256(&executable)? }],
        };
        fs::write(dir.join("core.json"), serde_json::to_vec_pretty(&sidecar)?)?;
    }
    if !inputs.cores.is_empty() {
        bail!("iOS Framework inputs do not match the bundled recipes");
    }
    if let Some(profile) = &signing.profile {
        fs::copy(profile, stage.join("embedded.mobileprovision"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dylib(cpu: u32, platform: u32, id: &str, dylib_dependency: Option<&str>, export: &[u8]) -> Vec<u8> {
        fn words(values: &[u32]) -> Vec<u8> {
            values.iter().flat_map(|value| value.to_le_bytes()).collect()
        }
        fn dylib_command(command: u32, name: &str) -> Vec<u8> {
            let size = (24 + name.len() + 1).next_multiple_of(8);
            let mut out = words(&[command, size as u32, 24, 0, 0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.resize(size, 0);
            out
        }

        let mut trie = vec![0, 1]; // empty root, one named child
        trie.extend_from_slice(export);
        trie.extend_from_slice(&[0, (export.len() + 4) as u8, 2, 0, 1, 0]);
        let mut commands = words(&[0x32, 24, platform, 0x10_00_00, 0x12_00_00, 0]);
        commands.extend(dylib_command(0xd, id));
        if let Some(name) = dylib_dependency {
            commands.extend(dylib_command(0xc, name));
        }
        let size = (commands.len() + 16) as u32;
        commands.extend(words(&[0x8000_0033, 16, 32 + size, trie.len() as u32]));
        let count = 3 + u32::from(dylib_dependency.is_some());
        let subtype = if cpu == 0x0100_0007 { 3 } else { 0 };
        let mut bytes = words(&[0xfeed_facf, cpu, subtype, 6, count, size, 0x80, 0]);
        bytes.extend(commands);
        bytes.extend(trie);
        bytes
    }

    #[test]
    fn framework_requires_the_matching_architecture_and_platform() {
        let root = crate::temporary::directory().unwrap();
        let path = root.path().join("core");
        let cases = [
            (0x0100_000c, 2, "aarch64-apple-ios"),
            (0x0100_000c, 7, "aarch64-apple-ios-sim"),
            (0x0100_0007, 7, "x86_64-apple-ios"),
        ];
        let id = "shinki.emu.cmvs";
        for (cpu, platform, expected) in cases {
            let bytes = dylib(cpu, platform, &install_name(id), Some("/usr/lib/libSystem.B.dylib"), b"_shinki_emu_query");
            fs::write(&path, bytes).unwrap();
            for (_, _, target) in cases {
                assert_eq!(inspect(&path, target, id).is_ok(), target == expected, "{expected} tested as {target}");
            }
        }
    }

    #[test]
    fn framework_rejects_unbundled_dependencies_and_non_abi_exports() {
        let root = crate::temporary::directory().unwrap();
        let path = root.path().join("core");
        let id = "shinki.emu.musica";
        let name = install_name(id);
        let inspect_bad = |install_id, dependency, export| {
            fs::write(&path, dylib(0x0100_000c, 7, install_id, dependency, export)).unwrap();
            inspect(&path, "aarch64-apple-ios-sim", id).unwrap_err().to_string()
        };
        assert!(inspect_bad(&name, Some("@rpath/libcodec.dylib"), b"_shinki_emu_query").contains("only system dependencies"));
        assert!(inspect_bad(&name, None, b"_private_symbol").contains("non-ABI symbol"));
        assert!(inspect_bad("@rpath/core.dylib", None, b"_shinki_emu_query").contains("install name"));
    }
}
