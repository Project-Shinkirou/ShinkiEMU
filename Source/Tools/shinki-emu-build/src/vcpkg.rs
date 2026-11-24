//! Cargo-owned, target-isolated vcpkg manifest installations.
use crate::{NativeInputs, pins, run, watch_env, work_root};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Installation {
    pub root: PathBuf,
    pub triplet: String,
    /// Directory containing the `installed/` tree (the root for vcpkg-rs).
    pub install_root: PathBuf,
    pub prefix: PathBuf,
}

fn triplet(target: &str) -> Result<&'static str> {
    Ok(match target {
        "x86_64-pc-windows-msvc" => "x64-windows",
        "x86_64-unknown-linux-gnu" => "x64-linux-dynamic",
        "aarch64-linux-android" => "arm64-android-ffmpeg",
        "x86_64-linux-android" => "x64-android-ffmpeg",
        "aarch64-unknown-linux-ohos" => "arm64-ohos",
        "aarch64-apple-darwin" => "arm64-osx",
        "x86_64-apple-darwin" => "x64-osx",
        "aarch64-apple-ios" => "arm64-ios",
        "aarch64-apple-ios-sim" => "arm64-ios-simulator",
        "x86_64-apple-ios" => "x64-ios",
        _ => bail!("no vcpkg triplet for {target}"),
    })
}

fn target_triplet(target: &str) -> Result<String> {
    // The private cross image supplies its own SDK chainload triplet.
    if watch_env("SHINKI_EMU_APPLE_CROSS").is_some() {
        return Ok(match target {
            "x86_64-apple-darwin" => "x64-osx-cross",
            "aarch64-apple-darwin" => "arm64-osx-cross",
            "aarch64-apple-ios" => "arm64-ios-cross",
            "aarch64-apple-ios-sim" => "arm64-ios-sim-cross",
            "x86_64-apple-ios" => "x64-ios-sim-cross",
            _ => bail!("no private Apple cross toolchain for {target}"),
        }.into());
    }
    Ok(triplet(target)?.to_owned())
}

pub fn install_core(source: &Path, id: &str) -> Result<Installation> {
    let target = env::var("TARGET")?;
    let config = crate::workspace().join("build/vcpkg");
    let mut ports = Vec::new();
    if target.ends_with("-linux-android") {
        ports.push(config.join("mobile-ports"));
    }
    let overlay = source.join("vcpkg/ports");
    if overlay.is_dir() {
        ports.push(overlay);
    }
    let shared_libass = id == "impacto" && target.ends_with("-linux-android");
    let installation = install(source, &target_triplet(&target)?, ports, shared_libass)?;
    installation.report(&format!("native-vcpkg/{id}"), &target)?;
    Ok(installation)
}

pub fn install_shared(name: &str) -> Result<Installation> {
    let target = env::var("TARGET")?;
    let lock = pins()?;
    let triplet = match (name, target.as_str()) {
        ("ffmpeg", "x86_64-pc-windows-msvc") => lock.ffmpeg_windows_triplet,
        ("subtitle", "x86_64-pc-windows-msvc") => lock.subtitle_windows_triplet,
        ("ffmpeg" | "subtitle", _) => target_triplet(&target)?,
        _ => bail!("unknown shared manifest {name}"),
    };
    install(&crate::workspace().join("build/vcpkg").join(name), &triplet, Vec::new(), false)
}

fn install(manifest: &Path, triplet: &str, mut ports: Vec<PathBuf>, shared_libass: bool) -> Result<Installation> {
    ensure!(!triplet.is_empty() && triplet.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'), "invalid vcpkg triplet {triplet:?}");
    let workspace = crate::workspace();
    let work = work_root();
    let lock = pins()?;
    let root = watch_env("SHINKI_EMU_VCPKG_ROOT").map_or_else(|| work.join("vcpkg"), PathBuf::from);
    let root = std::path::absolute(root)?;
    let parent = root.parent().context("vcpkg root must have a parent")?;
    fs::create_dir_all(parent)?;
    let mut lock_name = root.file_name().context("vcpkg root must have a name")?.to_os_string();
    lock_name.push(".lock");
    // vcpkg itself also takes a lock, but bootstrap and the vcpkg buildtrees
    // shared across different --x-install-root values need the same lock.
    let _lock = crate::lock_file(&parent.join(lock_name))?;
    bootstrap(&root, &work, &lock.vcpkg_url, &lock.vcpkg_commit)?;
    let manifest = dunce::canonicalize(manifest)?;
    println!("cargo:rerun-if-changed={}", manifest.join("vcpkg.json").display());
    let config = manifest.join("vcpkg-configuration.json");
    if config.is_file() {
        println!("cargo:rerun-if-changed={}", config.display());
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config)?)?;
        for key in ["overlay-ports", "overlay-triplets"] {
            for path in config[key].as_array().into_iter().flatten() {
                println!("cargo:rerun-if-changed={}", manifest.join(path.as_str().context("vcpkg overlay path")?).display());
            }
        }
    }
    if shared_libass {
        // Impacto's older baseline must resolve the same libass ABI as the
        // product subtitle manifest on Android.
        ports.insert(0, root.join("ports/libass"));
    }
    let mut triplets = vec![
        workspace.join("build/vcpkg/apple-triplets"),
        workspace.join("build/vcpkg/mobile-triplets"),
        workspace.join("build/vcpkg/subtitle-triplets"),
    ];
    if triplet.ends_with("-cross") {
        triplets.push(workspace.join("build/cross/triplets"));
        println!("cargo:rerun-if-changed={}", workspace.join("build/cross/toolchain.cmake").display());
    }
    for path in ports.iter().chain(&triplets) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for name in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "OHOS_SDK_NATIVE", "VCPKG_BINARY_SOURCES"] {
        watch_env(name);
    }
    let install_root = install_root(&work, &manifest, triplet, &lock.vcpkg_commit);
    let installed = install_root.join("installed");
    fs::create_dir_all(&install_root)?;
    let _install_lock = crate::lock_file(&install_root.join("install.lock"))?;
    let mut command = install_command(&root, &manifest, triplet, &installed, &ports, &triplets);
    crate::configure_native_environment(&work, &mut command)?;
    if triplet.contains("-android") {
        command.env("ANDROID_NDK_HOME", crate::android_ndk()?);
    }
    run(&mut command)?;
    println!("cargo:rerun-if-changed={}", installed.join("vcpkg/status").display());
    fs::copy(root.join(".vcpkg-root"), install_root.join(".vcpkg-root"))?;
    Ok(Installation { root, triplet: triplet.into(), install_root, prefix: installed.join(triplet) })
}

fn install_root(work: &Path, manifest: &Path, triplet: &str, commit: &str) -> PathBuf {
    let mut hash = Sha256::new();
    hash.update(manifest.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(commit.as_bytes());
    work.join("vcpkg-installs").join(format!("{triplet}-{}", hex::encode(&hash.finalize()[..8])))
}

fn binary(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) { "vcpkg.exe" } else { "vcpkg" })
}

fn install_command(root: &Path, manifest: &Path, triplet: &str, installed: &Path, ports: &[PathBuf], triplets: &[PathBuf]) -> Command {
    let mut command = Command::new(binary(root));
    command.current_dir(manifest)
        .args(["install", "--x-wait-for-lock", "--triplet", triplet, "--clean-buildtrees-after-build", "--clean-packages-after-build"])
        .arg(format!("--x-manifest-root={}", manifest.display()))
        .arg(format!("--x-install-root={}", installed.display()))
        .env("VCPKG_ROOT", root)
        .env("VCPKG_DISABLE_METRICS", "1")
        .env_remove("MACOSX_DEPLOYMENT_TARGET")
        .env_remove("IPHONEOS_DEPLOYMENT_TARGET");
    for (flag, paths) in [("--overlay-ports", ports), ("--overlay-triplets", triplets)] {
        command.args(paths.iter().map(|path| format!("{flag}={}", path.display())));
    }
    if cfg!(windows) {
        command.arg("--x-abi-tools-use-exact-versions").env_remove("VCPKG_FORCE_SYSTEM_BINARIES");
    }
    command
}

fn bootstrap(root: &Path, work: &Path, url: &str, commit: &str) -> Result<()> {
    if !root.exists() {
        let stage = tempfile::tempdir_in(root.parent().unwrap())?;
        let checkout = stage.path().join("vcpkg");
        // No shallow/partial clone: builtin-baseline lookups need old port trees.
        let mut clone = Command::new("git");
        clone.args(["clone", "--no-checkout", url]).arg(&checkout);
        crate::configure_native_environment(work, &mut clone)?;
        run(&mut clone)?;
        run(Command::new("git").arg("-C").arg(&checkout).args(["checkout", "--detach", commit]))?;
        fs::rename(checkout, root)?;
    }
    let git = || {
        let mut command = Command::new("git");
        command.arg("-C").arg(root);
        command
    };
    let actual = crate::capture(git().args(["rev-parse", "HEAD"]))?;
    ensure!(actual.trim() == commit, "{} is not pinned vcpkg {commit}; choose a new SHINKI_EMU_VCPKG_ROOT", root.display());
    let shallow = crate::capture(git().args(["rev-parse", "--is-shallow-repository"]))?;
    if shallow.trim() == "true" {
        run(git().args(["fetch", "--unshallow", url]))?;
    }
    if !binary(root).is_file() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/c", "bootstrap-vcpkg.bat"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.arg("bootstrap-vcpkg.sh");
            command
        };
        command.current_dir(root).arg("-disableMetrics");
        crate::configure_native_environment(work, &mut command)?;
        run(&mut command)?;
    }
    Ok(())
}

impl Installation {
    pub fn pkg_config_dirs(&self) -> [PathBuf; 2] {
        [self.prefix.join("lib/pkgconfig"), self.prefix.join("share/pkgconfig")]
    }

    pub fn report(&self, category: &str, target: &str) -> Result<NativeInputs> {
        let mut inputs = NativeInputs::default();
        if let Ok(share) = fs::read_dir(self.prefix.join("share")) {
            for port in share {
                let port = port?;
                let name = port.file_name();
                let copyright = port.path().join("copyright");
                if copyright.is_file() {
                    let section = if category == "ffmpeg" && name == "ffmpeg" {
                        "ffmpeg".to_owned()
                    } else {
                        format!("{category}/{}", name.to_string_lossy())
                    };
                    inputs.licenses.push(crate::BuildAsset {
                        source: copyright,
                        dest: format!("licenses/{section}/copyright"),
                    });
                }
            }
        }
        if target == "x86_64-unknown-linux-gnu" {
            let runtime = crate::out_dir()?.join("vcpkg-runtime");
            fs::create_dir_all(&runtime)?;
            let lib = dunce::canonicalize(self.prefix.join("lib"))?;
            for entry in fs::read_dir(&lib)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().contains(".so") {
                    inputs.runtime.push(stage_runtime_entry(&lib, &entry.path(), &runtime.join(entry.file_name()))?);
                }
            }
            let fmod = self.prefix.join("share/cocos2dx/linux-specific/fmod/prebuilt/64-bit/libfmod.so");
            if fmod.is_file() {
                ensure!(crate::sha256(&fmod)? == "d4454884763c746c70c4ea2a9b0ca35555fef2bf174072be21924cb9a80d84a9", "unexpected Cocos FMOD runtime");
                inputs.runtime.push(stage_elf(&fmod, &runtime.join("libfmod.so.6"))?);
            }
        } else {
            let (dir, extension) = if target.contains("windows") { ("bin", "dll") }
                else if target.contains("apple") { ("lib", "dylib") }
                else { ("lib", "so") };
            if let Ok(entries) = fs::read_dir(self.prefix.join(dir)) {
                for entry in entries {
                    let entry = entry?;
                    if entry.path().extension().is_some_and(|ext| ext == extension) {
                        inputs.runtime.push(entry.path());
                    }
                }
            }
        }
        inputs.ndk_openmp = category.starts_with("native-vcpkg/") && target.ends_with("-linux-android");
        inputs.runtime.sort();
        inputs.licenses.sort_by(|a, b| a.dest.cmp(&b.dest));
        inputs.write()?;
        Ok(inputs)
    }

    pub fn publish(&self, inputs: &NativeInputs) -> Result<()> {
        println!("cargo::metadata=root={}", self.prefix.display());
        println!("cargo::metadata=vcpkg_root={}", self.install_root.display());
        println!("cargo::metadata=triplet={}", self.triplet);
        println!("cargo::metadata=pkg_config_dirs={}", env::join_paths(self.pkg_config_dirs())?.to_string_lossy());
        println!("cargo::metadata=runtime_dsos={}", env::join_paths(&inputs.runtime)?.to_string_lossy());
        println!("cargo::metadata=pkg_config={}", pkg_config_program(&env::var("TARGET")?, Some(&self.install_root.join("installed")))?.display());
        Ok(())
    }
}

fn stage_runtime_entry(root: &Path, src: &Path, out: &Path) -> Result<PathBuf> {
    let src = dunce::canonicalize(src)?;
    ensure!(src.starts_with(root) && src.is_file(), "runtime library outside {}", root.display());
    stage_elf(&src, out)
}

fn stage_elf(src: &Path, out: &Path) -> Result<PathBuf> {
    // An interrupted previous build must not redirect this write via a link.
    if let Ok(metadata) = fs::symlink_metadata(out) {
        ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "invalid runtime output {}", out.display());
        fs::remove_file(out)?;
    }
    let mut output = fs::OpenOptions::new().write(true).create_new(true).open(out)?;
    std::io::copy(&mut fs::File::open(src)?, &mut output)?;
    drop(output);
    run(Command::new("patchelf").args(["--set-rpath", "$ORIGIN"]).arg(out))?;
    Ok(out.to_owned())
}

pub(crate) fn pkg_config_program(target: &str, installed: Option<&Path>) -> Result<PathBuf> {
    if let Some(program) = watch_env("PKG_CONFIG") {
        return Ok(program.into());
    }
    if cfg!(windows) && !target.contains("windows") {
        if let Some(program) = installed
            .map(|root| root.join("x64-windows/tools/pkgconf/pkgconf.exe"))
            .filter(|program| program.is_file())
        {
            return Ok(program);
        }
        let tools = work_root().join("vcpkg-downloads/tools/msys2");
        let mut programs = Vec::new();
        if let Ok(entries) = fs::read_dir(&tools) {
            for entry in entries {
                let candidate = entry?.path().join("mingw64/bin/pkgconf.exe");
                if candidate.is_file() {
                    programs.push(candidate);
                }
            }
        }
        programs.sort();
        return programs.pop().with_context(|| format!("vcpkg did not install host pkgconf under {}", tools.display()));
    }
    Ok(PathBuf::from("pkg-config"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, fs::File};

    #[test]
    fn install_command_isolates_manifest_triplet_and_overlays() {
        let manifest = Path::new("manifest with spaces");
        let work = Path::new("work");
        let first = install_root(work, manifest, "x64-linux-dynamic", "pin");
        let second = install_root(work, manifest, "arm64-ios", "pin");
        assert_ne!(first, second);
        assert_ne!(first, install_root(work, Path::new("other manifest"), "x64-linux-dynamic", "pin"));
        let cmd = install_command(Path::new("vcpkg"), manifest, "x64-linux-dynamic", &first,
            &[PathBuf::from("overlay ports")], &[PathBuf::from("overlay triplets")]);
        let expected = format!("--x-install-root={}", first.display());
        let args: Vec<_> = cmd.get_args().collect();
        assert!(args.windows(2).any(|a| a == ["--triplet", "x64-linux-dynamic"]));
        for arg in ["--x-manifest-root=manifest with spaces", "--overlay-ports=overlay ports", "--overlay-triplets=overlay triplets", &expected] {
            assert!(args.contains(&OsStr::new(arg)));
        }
        assert_eq!(cmd.get_current_dir(), Some(manifest));
    }

    #[test]
    fn lock_is_exclusive_across_processes() -> Result<()> {
        const LOCK_PATH: &str = "SHINKI_TEST_VCPKG_LOCK";
        const EXPECT_LOCKED: &str = "SHINKI_TEST_VCPKG_LOCKED";
        if let Some(file) = env::var_os(LOCK_PATH) {
            let contender = File::options().read(true).write(true).open(file)?;
            if env::var_os(EXPECT_LOCKED).as_deref() == Some(OsStr::new("yes")) {
                assert!(matches!(contender.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
            } else {
                contender.try_lock()?;
            }
            return Ok(());
        }
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("build.lock");
        let lock = crate::lock_file(&path)?;
        let probe = |locked| -> Result<()> {
            let output = Command::new(env::current_exe()?)
                .args(["--exact", "vcpkg::tests::lock_is_exclusive_across_processes"])
                .env(LOCK_PATH, &path)
                .env(EXPECT_LOCKED, locked)
                .output()?;
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
            Ok(())
        };
        probe("yes")?;
        drop(lock);
        probe("no")
    }

    #[cfg(unix)]
    #[test]
    fn runtime_copies_reject_escaping_or_dangling_links() -> Result<()> {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir()?;
        let lib = temp.path().join("lib");
        fs::create_dir(&lib)?;
        let private = temp.path().join("private.so");
        fs::write(&private, b"private")?;
        let source = lib.join("libdemo.so");
        let out = temp.path().join("output.so");
        symlink(&private, &source)?;
        assert!(stage_runtime_entry(&lib, &source, &out).is_err());
        assert!(!out.exists());
        fs::remove_file(&source)?;
        fs::write(&source, b"source")?;
        let missing = temp.path().join("must-not-be-created");
        symlink(&missing, &out)?;
        assert!(stage_runtime_entry(&lib, &source, &out).is_err());
        assert!(!missing.exists());
        assert_eq!(fs::read(private)?, b"private");
        Ok(())
    }
}
