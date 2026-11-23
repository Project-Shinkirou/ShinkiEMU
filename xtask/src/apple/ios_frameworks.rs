use crate::{
    build::{self, BuildArtifact},
    cores::Inventory,
    package::{self, CorePackage, FileRecord},
};
use anyhow::{Context, Result, bail};
use std::{
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

pub(crate) fn stage(
    inv: &Inventory,
    manager: &BuildArtifact,
    stage: &Path,
    signing: &Signing,
    version: &str,
) -> Result<()> {
    let frameworks = stage.join("Frameworks");
    fs::create_dir(&frameworks)?;
    let platform = if super::sdk_name(&manager.target) == "iphoneos" { "iPhoneOS" } else { "iPhoneSimulator" };
    let parsed = semver::Version::parse(version)?;
    let short_version = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
    for core in inv.cores.iter().filter(|core| core.ios_framework) {
        let id = &core.id;
        let build = build::execute_ios_framework(inv, &manager.target, id, version)?;
        let framework = frameworks.join(format!("{id}.framework"));
        fs::create_dir(&framework)?;
        let executable = framework.join(id);
        package::plain(&build.library)?;
        fs::copy(&build.library, &executable)?;
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
            version: version.to_owned(),
            identity: None,
            source_commit: manager.source_commit.clone(),
            recognition: core.recognition.clone(),
            abi: shinki_emu_build::ABI.into(),
            files: vec![FileRecord { path: id.clone(), sha256: package::sha256(&executable)? }],
        };
        fs::write(dir.join("core.json"), serde_json::to_vec_pretty(&sidecar)?)?;
    }
    if let Some(profile) = &signing.profile {
        fs::copy(profile, stage.join("embedded.mobileprovision"))?;
    }
    Ok(())
}

