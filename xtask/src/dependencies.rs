use crate::{cores::Inventory, package};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
struct Lock {
    schema: String,
    vcpkg_url: String,
    vcpkg_commit: String,
    cargo_ndk_version: String,
    android_ndk_version: String,
    subtitle_windows_triplet: String,
    ffmpeg_windows_triplet: String,
    bison_url: String,
    bison_sha256: String,
}
fn lock(inv: &Inventory) -> Result<Lock> {
    let l: Lock = toml::from_str(&fs::read_to_string(
        inv.root.join("build/dependencies.toml"),
    )?)?;
    if l.schema != "shinki.emu.build_dependencies.v1" {
        bail!("invalid dependency lock");
    }
    Ok(l)
}
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !status.success() {
        bail!(
            "{} failed with {status}",
            cmd.get_program().to_string_lossy()
        );
    }
    Ok(())
}
pub fn capture(cmd: &mut Command) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !output.status.success() {
        bail!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}
pub fn executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for suffix in if cfg!(windows) {
            &[".exe", ".cmd", ".bat", ""][..]
        } else {
            &[""][..]
        } {
            let p = dir.join(format!("{name}{suffix}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
pub fn ndk_root(inv: &Inventory) -> Result<PathBuf> {
    let l = lock(inv)?;
    let root = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("ANDROID_NDK_ROOT"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ANDROID_HOME")
                .or_else(|| env::var_os("ANDROID_SDK_ROOT"))
                .map(|p| PathBuf::from(p).join("ndk").join(&l.android_ndk_version))
        })
        .context("set ANDROID_NDK_HOME or ANDROID_HOME")?;
    let props = fs::read_to_string(root.join("source.properties"))
        .context("NDK source.properties missing")?;
    if !props.lines().any(|s| {
        s.split_once('=')
            .is_some_and(|(k, v)| k.trim() == "Pkg.Revision" && v.trim() == l.android_ndk_version)
    }) {
        bail!(
            "NDK {} required at {}",
            l.android_ndk_version,
            root.display()
        );
    }
    Ok(root)
}
pub fn doctor(inv: &Inventory, target: &str, core: Option<&str>) -> Result<()> {
    let t = inv.target(target)?;
    if let Some(c) = core {
        inv.require_supported(c, target)?;
    }
    let mut missing = Vec::new();
    for tool in &t.tools {
        match executable(tool) {
            Some(path) => println!("{tool}: {}", path.display()),
            None => {
                println!("{tool}: MISSING");
                missing.push(tool.clone());
            }
        }
    }
    let targets = capture(Command::new("rustup").args(["target", "list", "--installed"]))?;
    if !String::from_utf8_lossy(&targets)
        .lines()
        .any(|s| s == target)
    {
        missing.push(format!("rustup target add {target}"));
    }
    if t.platform == "windows" {
        if env::var_os("VCToolsInstallDir").is_none() && executable("cl").is_none() {
            missing.push("MSVC x64 developer environment".into());
        }
        let subtitle = subtitle_install(inv, target)?;
        for p in [
            subtitle.join("lib/pkgconfig/libass.pc"),
            inv.root
                .join(".work/vcpkg-ffmpeg/installed/x64-windows/lib/avcodec.lib"),
            inv.root.join(".work/dxc/dxcompiler.dll"),
        ] {
            if !p.is_file() {
                missing.push(format!(
                    "{} (cargo xtask deps --target {target})",
                    p.display()
                ));
            } else {
                println!("dependency: {}", p.display());
            }
        }
    }
    if t.platform == "android" {
        match ndk_root(inv) {
            Ok(p) => println!("NDK: {}", p.display()),
            Err(e) => missing.push(e.to_string()),
        };
        let ndk = inv.root.join(".work/tools/bin").join(if cfg!(windows) {
            "cargo-ndk.exe"
        } else {
            "cargo-ndk"
        });
        let tool = if ndk.is_file() {
            Some(ndk)
        } else {
            executable("cargo-ndk")
        };
        let ndk_version = tool.and_then(|path| capture(Command::new(path).arg("--version")).ok());
        if ndk_version
            .as_ref()
            .is_none_or(|bytes| !String::from_utf8_lossy(bytes).contains("4.1.2"))
        {
            missing.push("cargo-ndk 4.1.2 (cargo xtask deps)".into());
        }
        match Command::new("java").arg("-version").output() {
            Ok(output) => {
                let version = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if !output.status.success()
                    || !version
                        .lines()
                        .any(|line| line.contains("version \"17.") || line.contains("openjdk 17."))
                {
                    missing.push("JDK 17".into());
                } else {
                    println!("Java: {}", version.lines().next().unwrap_or("JDK 17"));
                }
            }
            Err(_) => missing.push("JDK 17".into()),
        }
        if !subtitle_install(inv, target)?
            .join("lib/pkgconfig/libass.pc")
            .is_file()
        {
            missing.push("Android subtitle vcpkg tree (cargo xtask deps)".into());
        }
    }
    if t.platform == "ios" && env::var_os("DEVELOPER_DIR").is_none() {
        println!("Xcode: using xcode-select developer directory");
    }
    if t.platform == "ohos" && env::var_os("OHOS_SDK_NATIVE").is_none() {
        missing.push("OHOS_SDK_NATIVE".into());
    }
    if core == Some("renpy") {
        for (variable, file) in [
            ("RENPY_SDK_ROOT", "lib/py3-windows-x86_64/libpython3.12.dll"),
            ("RENPY_PYTHON_DEV", "include/Python.h"),
            ("RENPY_SDL_INCLUDE", "SDL.h"),
        ] {
            match env::var_os(variable).map(PathBuf::from){
                Some(path) if path.join(file).is_file()=>println!("{variable}: {}",path.display()),
                _=>missing.push(format!("{variable} containing {file} (official pinned RenPy SDK/CPython 3.12.7/SDL 2.0.20 input)")),
            }
        }
        let python = env::var_os("Python3_EXECUTABLE").unwrap_or_else(|| "python".into());
        if capture(
            Command::new(python)
                .args(["-c", "import Cython; assert Cython.__version__ == '3.2.4'"]),
        )
        .is_err()
        {
            missing.push("build interpreter with official Cython 3.2.4".into());
        }
    }
    if !missing.is_empty() {
        bail!("missing prerequisites:\n  {}", missing.join("\n  "));
    }
    Ok(())
}
pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Project-Shinkirou-xtask")
        .timeout(std::time::Duration::from_secs(600))
        .build()?)
}
pub fn download(url: &str, sha: &str, out: &Path) -> Result<()> {
    if !url.starts_with("https://")
        || sha.len() != 64
        || !sha.bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("invalid locked download");
    }
    if out.exists() {
        if package::sha256(out)? == sha {
            return Ok(());
        }
        bail!("cached download hash mismatch: {}", out.display());
    }
    fs::create_dir_all(out.parent().context("download parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(out.parent().unwrap())?;
    let mut response = client()?.get(url).send()?.error_for_status()?;
    io::copy(&mut response, &mut temp)?;
    if package::sha256(temp.path())? != sha {
        bail!("SHA256 mismatch for locked download");
    }
    temp.persist_noclobber(out).map_err(|e| e.error)?;
    Ok(())
}
fn dxc(inv: &Inventory) -> Result<()> {
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(inv.root.join("Assets/Effects/dxc.lock.json"))?)?;
    let value = |key: &str| -> Result<&str> {
        v[key]
            .as_str()
            .with_context(|| format!("DXC lock missing {key}"))
    };
    let out = inv.root.join(".work/dxc");
    let members = [
        value("library")?,
        "bin/x64/dxil.dll",
        "LICENSE-MS.txt",
        "LICENSE-LLVM.txt",
    ];
    let archive = inv.root.join(".work/downloads").join(value("asset")?);
    download(value("url")?, value("sha256")?, &archive)?;
    if out.exists() {
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
        for name in &members {
            let mut input = zip.by_name(name)?;
            let dest = out.join(Path::new(name).file_name().unwrap());
            package::plain(&dest)?;
            let mut expected = Vec::new();
            io::Read::read_to_end(&mut input, &mut expected)?;
            if fs::read(dest)? != expected {
                bail!("DXC cache modified; remove .work/dxc before explicit deps");
            }
        }
        return Ok(());
    }
    package::atomic_directory(&out, |stage| {
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
        for name in &members {
            let mut source = zip.by_name(name)?;
            if source.is_symlink() {
                bail!("linked DXC member");
            }
            let dest = stage.join(Path::new(name).file_name().unwrap());
            io::copy(&mut source, &mut fs::File::create(dest)?)?;
        }
        Ok(())
    })
}
/// Locate the exact Cargo-resolved shared source, not an adjacent developer checkout.
fn subtitle_source(inv: &Inventory, target: &str) -> Result<PathBuf> {
    let value: serde_json::Value = serde_json::from_slice(&capture(
        Command::new("cargo").current_dir(&inv.root).args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ]),
    )?)?;
    let packages = value["packages"]
        .as_array()
        .context("cargo metadata packages")?;
    let package = packages
        .iter()
        .find(|p| p["name"] == "roukaku-subtitle")
        .context("pinned roukaku-subtitle missing in Cargo resolution")?;
    let source=package["source"].as_str().context("roukaku-subtitle must resolve from the pinned RoukakuEngine git revision, not a local patch")?;
    if !source.starts_with("git+https://github.com/Project-Shinkirou/RoukakuEngine")
        || source.rsplit('#').next().is_none_or(|s| s.len() != 40)
    {
        bail!("roukaku-subtitle source is not a pinned public git revision");
    }
    Ok(Path::new(
        package["manifest_path"]
            .as_str()
            .context("subtitle manifest_path")?,
    )
    .parent()
    .context("subtitle parent")?
    .to_owned())
}
pub fn subtitle_install(inv: &Inventory, target: &str) -> Result<PathBuf> {
    let l = lock(inv)?;
    Ok(if target.contains("android") {
        inv.root
            .join(".work/vcpkg-subtitle/installed")
            .join(if target.starts_with("aarch64") {
                "arm64-android-dynbidi"
            } else {
                "x64-android-dynbidi"
            })
    } else {
        env::var_os("ROUKAKU_SUBTITLE_VCPKG_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| inv.root.join(".work/vcpkg-subtitle"))
            .join("installed")
            .join(l.subtitle_windows_triplet)
    })
}
pub fn apply_environment(inv: &Inventory, target: &str, cmd: &mut Command) -> Result<()> {
    let subtitle = subtitle_install(inv, target)?;
    if target.contains("windows") {
        let root = subtitle
            .parent()
            .and_then(Path::parent)
            .context("subtitle root")?;
        cmd.env("ROUKAKU_SUBTITLE_VCPKG_ROOT", root)
            .env("VCPKG_ROOT", inv.root.join(".work/vcpkg-ffmpeg"));
        if let Ok(paths) = env::join_paths(
            [
                subtitle.join("bin"),
                inv.root.join(".work/dxc"),
                inv.root.join(".work/bison"),
            ]
            .into_iter()
            .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
        ) {
            cmd.env("PATH", paths);
        }
        cmd.env(
            "BISON_EXECUTABLE",
            inv.root.join(".work/bison/win_bison.exe"),
        );
    }
    if target.contains("android") {
        let ndk = ndk_root(inv)?;
        for var in [
            "PKG_CONFIG",
            "PKG_CONFIG_PATH",
            "PKG_CONFIG_LIBDIR",
            "PKG_CONFIG_SYSROOT_DIR",
        ] {
            cmd.env_remove(format!("{var}_{target}"))
                .env_remove(format!("{var}_{}", target.replace('-', "_")))
                .env_remove(format!("TARGET_{var}"));
        }
        cmd.env_remove("PKG_CONFIG_PATH")
            .env_remove("PKG_CONFIG_SYSROOT_DIR");
        cmd.env("ANDROID_NDK_HOME", &ndk)
            .env("ANDROID_NDK_ROOT", ndk)
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_LIBDIR", subtitle.join("lib/pkgconfig"));
        let pkg_config = if let Some(path) = env::var_os("PKG_CONFIG") {
            PathBuf::from(path)
        } else if let Some(path) = executable("pkgconf").or_else(|| executable("pkg-config")) {
            path
        } else {
            let tools = inv.root.join(".work/vcpkg/downloads/tools/msys2");
            walkdir::WalkDir::new(&tools)
                .into_iter()
                .filter_map(|e| e.ok())
                .find(|e| e.file_name() == "pkgconf.exe" && e.file_type().is_file())
                .map(|e| e.path().to_owned())
                .context("pkg-config is required for Android subtitle dependencies")?
        };
        cmd.env("PKG_CONFIG", pkg_config);
        if let Some(jar) = env::var_os("ANDROID_JAR") {
            cmd.env("ANDROID_JAR", jar);
        } else if let Some(sdk) =
            env::var_os("ANDROID_HOME").or_else(|| env::var_os("ANDROID_SDK_ROOT"))
        {
            let platforms = PathBuf::from(sdk).join("platforms");
            let mut jars = Vec::new();
            if platforms.is_dir() {
                for item in fs::read_dir(platforms)? {
                    let item = item?;
                    let name = item.file_name();
                    if let Some(version) = name
                        .to_str()
                        .and_then(|s| s.strip_prefix("android-"))
                        .and_then(|s| s.parse::<u32>().ok())
                    {
                        let jar = item.path().join("android.jar");
                        if jar.is_file() {
                            jars.push((version, jar));
                        }
                    }
                }
            }
            jars.sort_by_key(|(version, _)| *version);
            if let Some((_, jar)) = jars.last() {
                cmd.env("ANDROID_JAR", jar);
            }
        }
        let tools = inv.root.join(".work/tools/bin");
        cmd.env(
            "PATH",
            env::join_paths(
                std::iter::once(tools)
                    .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
            )?,
        );
    }
    Ok(())
}
pub fn prepare(inv: &Inventory, target: &str, core: Option<&str>) -> Result<()> {
    let t = inv.target(target)?;
    if let Some(c) = core {
        inv.require_supported(c, target)?;
    }
    if !matches!(t.platform.as_str(), "windows" | "android") {
        bail!("no dependency preparation path for blocked target {target}");
    }
    let l = lock(inv)?;
    let vcpkg = inv.root.join(".work/vcpkg");
    if !vcpkg.exists() {
        run(Command::new("git")
            .args(["clone", "--filter=blob:none", &l.vcpkg_url])
            .arg(&vcpkg))?;
        run(Command::new("git").arg("-C").arg(&vcpkg).args([
            "checkout",
            "--detach",
            &l.vcpkg_commit,
        ]))?;
    }
    let actual = capture(
        Command::new("git")
            .arg("-C")
            .arg(&vcpkg)
            .args(["rev-parse", "HEAD"]),
    )?;
    if String::from_utf8_lossy(&actual).trim() != l.vcpkg_commit {
        bail!("vcpkg cache revision differs from dependency lock");
    }
    let binary = vcpkg.join(if cfg!(windows) { "vcpkg.exe" } else { "vcpkg" });
    if !binary.is_file() {
        let bootstrap = vcpkg.join(if cfg!(windows) {
            "bootstrap-vcpkg.bat"
        } else {
            "bootstrap-vcpkg.sh"
        });
        if cfg!(windows) {
            run(Command::new("cmd")
                .arg("/c")
                .arg(bootstrap)
                .arg("-disableMetrics"))?;
        } else {
            run(Command::new("sh").arg(bootstrap).arg("-disableMetrics"))?;
        }
    }
    let subtitle = subtitle_source(inv, target)?;
    let triplet = if t.platform == "android" {
        if t.architecture == "aarch64" {
            "arm64-android-dynbidi"
        } else {
            "x64-android-dynbidi"
        }
    } else {
        &l.subtitle_windows_triplet
    };
    let install = inv.root.join(".work/vcpkg-subtitle/installed");
    fs::create_dir_all(install.parent().unwrap())?;
    let mut command = Command::new(&binary);
    command
        .arg("install")
        .arg(format!("--x-manifest-root={}", subtitle.display()))
        .arg(format!(
            "--overlay-triplets={}",
            subtitle.join("triplets").display()
        ))
        .args(["--triplet", triplet])
        .arg(format!("--x-install-root={}", install.display()))
        .args([
            "--clean-buildtrees-after-build",
            "--clean-packages-after-build",
        ])
        .env("VCPKG_DISABLE_METRICS", "1");
    if t.platform == "android" {
        command.env("ANDROID_NDK_HOME", ndk_root(inv)?);
    }
    run(&mut command)?;
    fs::copy(
        vcpkg.join(".vcpkg-root"),
        install.parent().unwrap().join(".vcpkg-root"),
    )?;
    if t.platform == "windows" {
        let ff = inv.root.join(".work/vcpkg-ffmpeg");
        fs::create_dir_all(&ff)?;
        run(Command::new(&binary)
            .arg("install")
            .arg(format!(
                "--x-manifest-root={}",
                inv.root.join("build/vcpkg/ffmpeg").display()
            ))
            .args(["--triplet", &l.ffmpeg_windows_triplet])
            .arg(format!(
                "--x-install-root={}",
                ff.join("installed").display()
            ))
            .args([
                "--clean-buildtrees-after-build",
                "--clean-packages-after-build",
            ])
            .env("VCPKG_DISABLE_METRICS", "1"))?;
        fs::copy(vcpkg.join(".vcpkg-root"), ff.join(".vcpkg-root"))?;
        dxc(inv)?;
        let archive = inv.root.join(".work/downloads/win_flex_bison-2.5.25.zip");
        download(&l.bison_url, &l.bison_sha256, &archive)?;
        let out = inv.root.join(".work/bison");
        if !out.exists() {
            package::extract_zip(&archive, &out)?;
        }
        for native in inv
            .cores
            .iter()
            .filter(|c| c.artifact_kind == "native-shared" && core.is_none_or(|id| id == c.id))
        {
            let source = inv
                .root
                .join(native.manifest.components().take(2).collect::<PathBuf>());
            if !source.join("vcpkg.json").is_file() {
                continue;
            }
            let installed = inv
                .root
                .join(".work/native-vcpkg")
                .join(&native.id)
                .join("installed");
            let mut command = Command::new(&binary);
            command
                .arg("install")
                .arg(format!("--x-manifest-root={}", source.display()))
                .arg(format!("--x-install-root={}", installed.display()))
                .args([
                    "--triplet",
                    "x64-windows",
                    "--clean-buildtrees-after-build",
                    "--clean-packages-after-build",
                ])
                .env("VCPKG_DISABLE_METRICS", "1");
            let overlay = source.join("vcpkg/ports");
            if overlay.is_dir() {
                command.arg(format!("--overlay-ports={}", overlay.display()));
            }
            run(&mut command)?;
        }
    } else {
        let tool = inv.root.join(".work/tools/bin").join(if cfg!(windows) {
            "cargo-ndk.exe"
        } else {
            "cargo-ndk"
        });
        let current = if tool.exists() {
            Some(capture(Command::new(&tool).arg("--version"))?)
        } else {
            None
        };
        if current
            .as_ref()
            .is_none_or(|s| !String::from_utf8_lossy(s).contains(&l.cargo_ndk_version))
        {
            run(Command::new("cargo")
                .current_dir(&inv.root)
                .args([
                    "install",
                    "--locked",
                    "cargo-ndk",
                    "--version",
                    &format!("={}", l.cargo_ndk_version),
                    "--root",
                ])
                .arg(inv.root.join(".work/tools")))?;
        }
    }
    Ok(())
}
