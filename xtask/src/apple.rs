use anyhow::{Context, Result, bail};
use object::read::macho::MachHeader;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) mod ios_frameworks;

pub fn supported(target: &str) -> bool {
    matches!(
        target,
        "x86_64-apple-darwin"
            | "aarch64-apple-darwin"
            | "aarch64-apple-ios"
            | "aarch64-apple-ios-sim"
            | "x86_64-apple-ios"
    )
}
pub fn cross_host(target: &str) -> bool {
    supported(target) && !cfg!(target_os = "macos")
}
fn checked(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("running {command:?}"))?;
    if !status.success() {
        bail!("{command:?} failed: {status}");
    }
    Ok(())
}
pub fn sign_binary(path: &Path) -> Result<()> {
    if path
        .extension()
        .is_some_and(|extension| extension == "dylib")
    {
        let tool = if cfg!(target_os = "macos") {
            "install_name_tool"
        } else {
            "llvm-install-name-tool"
        };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("core dylib filename")?;
        checked(
            Command::new(tool)
                .args(["-id", &format!("@rpath/{name}")])
                .arg(path),
        )?;
    }
    if cfg!(target_os = "macos") {
        checked(
            Command::new("codesign")
                .args(["--force", "--sign", "-"])
                .arg(path),
        )?;
        checked(
            Command::new("codesign")
                .args(["--verify", "--strict"])
                .arg(path),
        )?;
    } else {
        checked(Command::new("rcodesign").arg("sign").arg(path))?;
        checked(Command::new("rcodesign").arg("verify").arg(path))?;
    }
    Ok(())
}
fn image_name(target: &str) -> String {
    format!("shinkirou/apple-cross:{target}-1.99.1")
}

pub fn preflight(target: &str) -> Result<()> {
    if !supported(target) {
        bail!("unsupported Apple triple {target}");
    }
    if cfg!(target_os = "macos") {
        checked(Command::new("xcrun").args(["--sdk", sdk_name(target), "--show-sdk-path"]))?;
    } else {
        let version = Command::new("cross")
            .arg("--version")
            .output()
            .context("install cross 0.2.5 explicitly")?;
        if !version.status.success() || !String::from_utf8_lossy(&version.stdout).contains("0.2.5")
        {
            bail!("Apple cross builds require cross 0.2.5");
        }
        let status = Command::new("docker")
            .args(["image", "inspect", &image_name(target)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!(
                "private Apple toolchain image missing; set SHINKI_APPLE_SDK_DIR to your extracted {} SDK and run cargo xtask apple-image --target {target}",
                sdk_name(target)
            );
        }
    }
    Ok(())
}
fn sdk_name(target: &str) -> &'static str {
    if target.ends_with("darwin") {
        "macosx"
    } else if target.ends_with("-sim") || target.starts_with("x86_64") {
        "iphonesimulator"
    } else {
        "iphoneos"
    }
}
pub fn command(root: &Path, target: &str) -> Result<Command> {
    if !supported(target) {
        bail!("unsupported Apple triple {target}");
    }
    preflight(target)?;
    let mut command = if cross_host(target) {
        let mut command = Command::new("cross");
        command
            .arg("+1.99.1")
            .env("CROSS_CONFIG", root.join("Cross.toml"));
        command
    } else {
        Command::new("cargo")
    };
    // Only the target's own variable: clang treats a set IPHONEOS_DEPLOYMENT_TARGET
    // as an iOS target for compiles without an explicit `-target` (aws-lc-sys probes).
    if target.ends_with("darwin") {
        command.env("MACOSX_DEPLOYMENT_TARGET", "13.0");
    } else {
        command.env("IPHONEOS_DEPLOYMENT_TARGET", "16.0");
    }
    Ok(command)
}

pub fn image(root: &Path, target: &str) -> Result<()> {
    if !supported(target) {
        bail!("unsupported Apple triple {target}");
    }
    let sdk = PathBuf::from(std::env::var_os("SHINKI_APPLE_SDK_DIR").context(
        "SHINKI_APPLE_SDK_DIR must point to a legally obtained, extracted Apple .sdk directory",
    )?)
    .canonicalize()?;
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(sdk.join("SDKSettings.json"))?)?;
    let version = settings["Version"]
        .as_str()
        .context("SDKSettings.json Version missing")?;
    let canonical = settings["CanonicalName"]
        .as_str()
        .context("SDKSettings.json CanonicalName missing")?;
    if !canonical.starts_with(sdk_name(target))
        || !version.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        bail!("SDK {canonical} does not match {target}");
    }
    // Image construction is explicit and may consume several GiB. Never evict user images.
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("df").args(["-Pk", "/"]).output()?;
        let available = String::from_utf8(output.stdout)?
            .lines()
            .last()
            .context("df output")?
            .split_whitespace()
            .nth(3)
            .context("df available blocks")?
            .parse::<u64>()?;
        if available < 8 * 1024 * 1024 {
            bail!(
                "Apple image build needs at least 8 GiB free; available {} MiB; no user files/images were removed",
                available / 1024
            );
        }
    }
    checked(Command::new("docker").args(["buildx", "version"]))
        .context("Docker Buildx/BuildKit is required; install the official plugin explicitly")?;
    checked(
        Command::new("docker")
            .current_dir(root)
            .args(["buildx", "build", "--load", "--build-context"])
            .arg(format!("apple-sdk={}", sdk.display()))
            .args([
                "--build-arg",
                &format!("TARGET={target}"),
                "--build-arg",
                &format!("SDK_VERSION={version}"),
                "-t",
                &image_name(target),
                "-f",
                "build/cross/Dockerfile",
                ".",
            ]),
    )
    .context("build private Apple SDK image")
}

fn has_executable_rpath(path: &Path) -> Result<bool> {
    let bytes = fs::read(path)?;
    let object::File::MachO64(macho) = object::File::parse(bytes.as_slice())? else {
        bail!("expected a Mach-O bundle executable: {}", path.display());
    };
    let endian = macho.endian();
    let mut commands = macho.macho_header().load_commands(endian, macho.data(), 0)?;
    while let Some(command) = commands.next()? {
        if command.cmd() == object::macho::LC_RPATH {
            let rpath: &object::macho::RpathCommand<_> = command.data()?;
            if command.string(endian, rpath.path)? == b"@executable_path" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Convert validated flat staging to a bundle. Rewriting and signing only touch staging.
pub fn bundle(
    stage: &Path,
    target: &str,
    executable: &str,
    name: &str,
    id: &str,
    version: &str,
    ios_signing: Option<&ios_frameworks::Signing>,
) -> Result<PathBuf> {
    let parsed = semver::Version::parse(version)?;
    let version = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
    if !supported(target) {
        bail!("unsupported Apple triple {target}");
    }
    for value in [executable, name, id] {
        if value.is_empty()
            || matches!(value, "." | "..")
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            bail!("unsafe Apple bundle identity");
        }
    }
    let ios = target.contains("-ios");
    let simulator = ios && sdk_name(target) == "iphonesimulator";
    let bundle_name = if simulator {
        format!("{name}-simulator-test.app")
    } else {
        format!("{name}.app")
    };
    let app = stage.join(bundle_name);
    let entries = fs::read_dir(stage)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    if entries
        .iter()
        .any(|p| p.file_name().is_some_and(|n| n == app.file_name().unwrap()))
    {
        bail!("Apple bundle output already exists");
    }
    let mut binaries = vec![stage.join(executable)];
    binaries.extend(
        entries
            .iter()
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "dylib")
            })
            .cloned(),
    );
    if ios_signing.is_some()
        && (!ios || binaries.len() != 1 || !stage.join("Frameworks").is_dir())
    {
        bail!("the iOS Framework app must embed code under Frameworks, not as loose dylibs");
    }
    let contents = if ios {
        app.clone()
    } else {
        app.join("Contents")
    };
    let binary_dir = if ios {
        app.clone()
    } else {
        contents.join("MacOS")
    };
    let resources = if ios {
        app.clone()
    } else {
        contents.join("Resources")
    };
    fs::create_dir_all(&binary_dir)?;
    fs::create_dir_all(&resources)?;
    let icon = crate::icon::Icon::new()?;
    if ios {
        icon.ios(&resources)?;
    } else {
        icon.icns(&resources.join("ShinkiEMU.icns"))?;
    }
    let main = binary_dir.join(executable);
    for entry in entries {
        if fs::symlink_metadata(&entry)?.file_type().is_symlink() {
            bail!("link in Apple staging input");
        }
        let leaf = entry.file_name().context("bundle input name")?;
        let binary = leaf == executable || entry.extension().is_some_and(|e| e == "dylib");
        fs::rename(
            &entry,
            if binary {
                binary_dir.join(leaf)
            } else {
                resources.join(leaf)
            },
        )?;
    }
    if !main.is_file() {
        bail!("Apple bundle lacks executable {executable}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&main, fs::Permissions::from_mode(0o755))?;
    }
    let platform = if ios {
        if sdk_name(target) == "iphonesimulator" {
            "iPhoneSimulator"
        } else {
            "iPhoneOS"
        }
    } else {
        "MacOSX"
    };
    let minimum = if ios {
        "<key>MinimumOSVersion</key><string>16.0</string><key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array><key>UILaunchScreen</key><dict/><key>UIFileSharingEnabled</key><true/><key>LSSupportsOpeningDocumentsInPlace</key><true/>"
    } else {
        "<key>LSMinimumSystemVersion</key><string>13.0</string><key>NSHighResolutionCapable</key><true/>"
    };
    let icon_metadata = if ios {
        "<key>CFBundleIcons</key><dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array><string>AppIcon60x60</string></array><key>UIPrerenderedIcon</key><true/></dict></dict><key>CFBundleIcons~ipad</key><dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array><string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array></dict></dict>"
    } else {
        "<key>CFBundleIconFile</key><string>ShinkiEMU</string>"
    };
    fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleName</key><string>{name}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>1</string><key>CFBundleSupportedPlatforms</key><array><string>{platform}</string></array>{icon_metadata}{minimum}</dict></plist>"
        ),
    )?;
    let install_name_tool = if cfg!(target_os = "macos") {
        "install_name_tool"
    } else {
        "llvm-install-name-tool"
    };
    if binaries.len() > 1 && !has_executable_rpath(&main)? {
        checked(
            Command::new(install_name_tool)
                .args(["-add_rpath", "@executable_path"])
                .arg(&main),
        )?;
    }
    if let Some(signing) = ios_signing {
        signing.sign_app(&app)?;
    } else if cfg!(target_os = "macos") {
        checked(
            Command::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(&app),
        )?;
        checked(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&app),
        )?;
    } else {
        checked(Command::new("rcodesign").arg("sign").arg(&app))?;
        for binary in binaries {
            checked(
                Command::new("rcodesign")
                    .arg("verify")
                    .arg(binary_dir.join(binary.file_name().context("signed binary filename")?)),
            )?;
        }
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_identity_never_mutates_staging() {
        let tmp = crate::temporary::directory().unwrap();
        fs::write(tmp.path().join("player"), b"original").unwrap();
        assert!(
            bundle(
                tmp.path(),
                "aarch64-apple-darwin",
                "player",
                "../escape",
                "org.shinkirou.player",
                "0.1.0",
                None,
            )
            .is_err()
        );
        assert_eq!(fs::read(tmp.path().join("player")).unwrap(), b"original");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
}
