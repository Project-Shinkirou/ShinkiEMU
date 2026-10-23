use crate::{cores::Inventory, package};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    native: BTreeMap<String, NativeArchive>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeArchive {
    version: String,
    url: String,
    sha256: String,
    file_name: String,
    format: String,
    strip_prefix: String,
    #[serde(default)]
    include: Vec<String>,
    required: Vec<String>,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct NativeNeeds {
    pub subtitle: bool,
    pub ffmpeg: bool,
    pub dxc: bool,
    pub native_vcpkg: bool,
    pub impacto_sources: bool,
    pub renpy: bool,
    pub bison: bool,
}

pub(crate) fn needs(inv: &Inventory, target: &str, component: &str) -> Result<NativeNeeds> {
    let mut result = NativeNeeds::default();
    if component == "manager" {
        result.dxc = target.contains("windows");
        result.subtitle = target.contains("android");
        return Ok(result);
    }
    for requirement in &inv.core(component)?.dependencies {
        match requirement.as_str() {
            "subtitle" => result.subtitle = true,
            "ffmpeg" => result.ffmpeg = true,
            "dxc" => result.dxc = true,
            "native-vcpkg" => result.native_vcpkg = true,
            "impacto-sources" => result.impacto_sources = true,
            "renpy" => result.renpy = true,
            "bison" => result.bison = true,
            other => bail!("unknown native dependency requirement {other} for {component}"),
        }
    }
    Ok(result)
}

const NATIVE_RECORD: &str = ".shinki-native.json";

#[derive(Serialize)]
struct NativeLayer<'a> {
    archive: &'a NativeArchive,
    destination: &'a str,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct NativeFile {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRecord {
    schema: String,
    layers: serde_json::Value,
    files: Vec<NativeFile>,
}

fn native_path(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("empty native dependency path");
    }
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || "\\:<>|?*\"".contains(c))
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("unsafe native dependency path {name}");
        }
    }
    Ok(())
}

fn native_files(root: &Path) -> Result<Vec<NativeFile>> {
    package::plain(root)?;
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        package::plain(entry.path())?;
        let name = entry
            .path()
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF-8 native dependency path")?
            .replace('\\', "/");
        if name.is_empty() || name == NATIVE_RECORD {
            continue;
        }
        native_path(&name)?;
        if !seen.insert(name.to_lowercase()) {
            bail!("native dependency case collision: {name}");
        }
        if entry.file_type().is_file() {
            files.push(NativeFile {
                path: name,
                sha256: package::sha256(entry.path())?,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn verify_native_tree(out: &Path, identity: &serde_json::Value) -> Result<()> {
    package::plain(&out.join(NATIVE_RECORD))?;
    let record: NativeRecord = serde_json::from_slice(&fs::read(out.join(NATIVE_RECORD))?)?;
    if record.schema != "shinki.emu.native_dependency.v1"
        || record.layers != *identity
        || record.files != native_files(out)?
    {
        bail!("native dependency tree differs from its pinned inputs: {}; remove that owned tree before explicit re-preparation", out.display());
    }
    Ok(())
}

fn selected_native_path(archive: &NativeArchive, raw: &str) -> Result<Option<String>> {
    let raw = raw.trim_end_matches('/');
    native_path(raw)?;
    let relative = if archive.strip_prefix.is_empty() {
        raw
    } else if raw == archive.strip_prefix {
        return Ok(None);
    } else if let Some(rest) = raw.strip_prefix(&format!("{}/", archive.strip_prefix)) {
        rest
    } else {
        return Ok(None);
    };
    if !archive.include.is_empty()
        && !archive
            .include
            .iter()
            .any(|prefix| relative == prefix || relative.starts_with(&format!("{prefix}/")))
    {
        return Ok(None);
    }
    native_path(relative)?;
    Ok(Some(relative.to_owned()))
}

fn reserve_native_path(
    seen: &mut BTreeMap<String, (String, bool)>,
    name: &str,
    directory: bool,
) -> Result<()> {
    let mut current = String::with_capacity(name.len());
    for part in name.split('/') {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        let is_directory = current.len() != name.len() || directory;
        let key = current.to_lowercase();
        if let Some((original, previous_directory)) = seen.get(&key) {
            if original != &current || !previous_directory || !is_directory {
                bail!("duplicate or conflicting native archive path {name}");
            }
        } else {
            seen.insert(key, (current.clone(), is_directory));
        }
    }
    Ok(())
}

fn extract_native_archive(
    archive: &NativeArchive,
    input: &Path,
    root: &Path,
    destination: &str,
) -> Result<()> {
    if !destination.is_empty() {
        native_path(destination)?;
    }
    let mut seen = BTreeMap::new();
    let mut extract =
        |raw: &str, directory: bool, linked: bool, reader: &mut dyn io::Read| -> Result<()> {
            let Some(relative) = selected_native_path(archive, raw)? else {
                return Ok(());
            };
            if linked {
                bail!("linked native archive member {raw}");
            }
            let relative = if destination.is_empty() {
                relative
            } else {
                format!("{destination}/{relative}")
            };
            if relative == NATIVE_RECORD {
                bail!("native archive collides with verification record");
            }
            reserve_native_path(&mut seen, &relative, directory)?;
            let output = root.join(&relative);
            if directory {
                fs::create_dir_all(output)?;
            } else {
                fs::create_dir_all(output.parent().context("native archive parent")?)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(output)?;
                io::copy(reader, &mut file)?;
            }
            Ok(())
        };
    match archive.format.as_str() {
        "zip" => {
            let mut zip = zip::ZipArchive::new(fs::File::open(input)?)?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                let name = entry.name().to_owned();
                let directory = entry.is_dir();
                let linked = entry.is_symlink();
                extract(&name, directory, linked, &mut entry)?;
            }
        }
        "tar-gz" => {
            let stream = flate2::read::GzDecoder::new(fs::File::open(input)?);
            let mut tar = tar::Archive::new(stream);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let name = entry
                    .path()?
                    .to_str()
                    .context("non-UTF-8 tar path")?
                    .to_owned();
                let kind = entry.header().entry_type();
                extract(
                    &name,
                    kind.is_dir(),
                    !kind.is_dir() && !kind.is_file(),
                    &mut entry,
                )?;
            }
        }
        other => bail!("unknown native archive format {other}"),
    }
    for required in &archive.required {
        native_path(required)?;
        let path = root.join(destination).join(required);
        package::plain(&path)
            .with_context(|| format!("native archive lacks required input {}", path.display()))?;
    }
    Ok(())
}

fn prepare_native_tree(
    root: &Path,
    name: &str,
    layers: &[NativeLayer<'_>],
    python_versions: Option<(&str, &str)>,
) -> Result<PathBuf> {
    package::portable(name)?;
    let out = root.join(".work/native-downloads").join(name);
    let identity = serde_json::to_value(layers)?;
    if fs::symlink_metadata(&out).is_ok() {
        verify_native_tree(&out, &identity)?;
        return Ok(out);
    }
    let mut archives = Vec::new();
    for layer in layers {
        package::portable(&layer.archive.file_name)?;
        if layer.archive.file_name.contains('/') {
            bail!("native download must have a plain filename");
        }
        let input = root.join(".work/downloads").join(&layer.archive.file_name);
        download(&layer.archive.url, &layer.archive.sha256, &input)?;
        archives.push(input);
    }
    package::atomic_directory(&out, |stage| {
        for (layer, input) in layers.iter().zip(&archives) {
            extract_native_archive(layer.archive, input, stage, layer.destination)?;
        }
        if let Some((python, cython)) = python_versions {
            validate_build_python(&stage.join("python.exe"), python, cython)?;
        }
        let record = NativeRecord {
            schema: "shinki.emu.native_dependency.v1".into(),
            layers: identity,
            files: native_files(stage)?,
        };
        fs::write(
            stage.join(NATIVE_RECORD),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })?;
    Ok(out)
}

fn validate_build_python(executable: &Path, python: &str, cython: &str) -> Result<()> {
    run(Command::new(executable).args([
        "-B", "-I", "-c",
        "import sys, struct, Cython; assert struct.calcsize('P') == 8; assert sys.version_info[:3] == tuple(map(int, sys.argv[1].split('.'))); assert Cython.__version__ == sys.argv[2]",
        python, cython,
    ]))
}

fn prepare_native_inputs(inv: &Inventory, lock: &Lock, selected: NativeNeeds) -> Result<()> {
    let get = |name: &str| {
        lock.native
            .get(name)
            .with_context(|| format!("native dependency lock missing {name}"))
    };
    if selected.impacto_sources {
        for name in ["libatrac9", "magic_enum"] {
            prepare_native_tree(
                &inv.root,
                name,
                &[NativeLayer {
                    archive: get(name)?,
                    destination: "",
                }],
                None,
            )?;
        }
    }
    if selected.renpy {
        if !cfg!(windows) {
            bail!("RenPy's pinned build interpreter requires a Windows x64 host");
        }
        let sdk: serde_json::Value = serde_json::from_slice(&fs::read(
            inv.root.join("ThirdParty/renpy/shinki/sdk.json"),
        )?)?;
        let sdk_archive = get("renpy-sdk")?;
        let python = get("python")?;
        let cython = get("cython")?;
        let sdl = get("sdl2")?;
        if sdk["url"] != sdk_archive.url
            || sdk["sha256"] != sdk_archive.sha256
            || sdk["python"] != python.version
            || sdk["cython"] != cython.version
            || sdk["sdl"] != sdl.version
        {
            bail!("RenPy sdk.json and build/dependencies.toml pins disagree");
        }
        let sdk_root = prepare_native_tree(
            &inv.root,
            "renpy-sdk",
            &[NativeLayer {
                archive: sdk_archive,
                destination: "",
            }],
            None,
        )?;
        for (name, expected) in sdk["windows_x86_64"]
            .as_object()
            .context("SDK runtime hashes")?
        {
            package::portable(name)?;
            if Some(package::sha256(&sdk_root.join("lib/py3-windows-x86_64").join(name))?.as_str())
                != expected.as_str()
            {
                bail!("official RenPy SDK runtime hash mismatch: {name}");
            }
        }
        prepare_native_tree(
            &inv.root,
            "python",
            &[
                NativeLayer {
                    archive: python,
                    destination: "",
                },
                NativeLayer {
                    archive: cython,
                    destination: "Lib/site-packages",
                },
            ],
            Some((&python.version, &cython.version)),
        )?;
        prepare_native_tree(
            &inv.root,
            "sdl2",
            &[NativeLayer {
                archive: sdl,
                destination: "",
            }],
            None,
        )?;
        prepare_native_tree(
            &inv.root,
            "cython-license",
            &[NativeLayer {
                archive: get("cython-license")?,
                destination: "",
            }],
            None,
        )?;
    }
    Ok(())
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
    let selected = needs(inv, target, core.unwrap_or("manager"))?;
    let mut missing = Vec::new();
    if let Some(id) = core {
        let core = inv.core(id)?;
        match crate::sources::verify(inv, core, false) {
            Ok(directory) => {
                if let Some(source) = &core.source {
                    println!("source: {id} @ {} ({})", source.rev, directory.display());
                }
            }
            Err(error) => missing.push(format!("{error:#}")),
        }
    }
    for tool in &t.tools {
        if matches!(tool.as_str(), "cmake" | "ninja")
            && !(selected.ffmpeg || selected.native_vcpkg || selected.renpy)
        {
            continue;
        }
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
        if let Some(compiler) = cc::windows_registry::find_tool(target, "cl.exe") {
            println!("MSVC: {}", compiler.path().display());
        } else if env::var_os("VCToolsInstallDir").is_none() && executable("cl").is_none() {
            missing.push("MSVC x64 compiler/build tools".into());
        }
        let mut paths = Vec::new();
        if selected.subtitle {
            paths.push(subtitle_install(inv, target)?.join("lib/pkgconfig/libass.pc"));
        }
        if selected.ffmpeg {
            paths.push(
                inv.root
                    .join(".work/vcpkg-ffmpeg/installed/x64-windows/lib/avcodec.lib"),
            );
        }
        if selected.dxc {
            paths.push(inv.root.join(".work/dxc/dxcompiler.dll"));
        }
        if selected.bison {
            paths.push(inv.root.join(".work/bison/win_bison.exe"));
        }
        if selected.native_vcpkg {
            paths.push(
                inv.root
                    .join(".work/native-vcpkg")
                    .join(core.context("native core id")?)
                    .join("installed/vcpkg/status"),
            );
        }
        for p in paths {
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
        if selected.subtitle
            && !subtitle_install(inv, target)?
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
    if selected.renpy {
        let base = inv.root.join(".work/native-downloads");
        for (variable, path) in [
            (
                "RENPY_SDK_ROOT",
                base.join("renpy-sdk/lib/py3-windows-x86_64/libpython3.12.dll"),
            ),
            ("RENPY_PYTHON_DEV", base.join("python/include/Python.h")),
            ("RENPY_SDL_INCLUDE", base.join("sdl2/include/SDL.h")),
            (
                "RENPY_CYTHON_LICENSE",
                base.join("cython-license/LICENSE.txt"),
            ),
        ] {
            if path.is_file() {
                println!("{variable}: {}", path.display());
            } else {
                missing.push(format!(
                    "{variable}: {} (cargo xtask deps --target {target} --core renpy)",
                    path.display()
                ));
            }
        }
        let pins = lock(inv)?;
        if let (Some(python), Some(cython)) = (pins.native.get("python"), pins.native.get("cython"))
        {
            if validate_build_python(
                &base.join("python/python.exe"),
                &python.version,
                &cython.version,
            )
            .is_err()
            {
                missing.push(
                    "task-owned CPython/Cython build interpreter (cargo xtask deps --core renpy)"
                        .into(),
                );
            }
        } else {
            missing.push("CPython/Cython dependency pins".into());
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
    let mut response = client()?
        .get(url)
        .timeout(std::time::Duration::from_secs(3600))
        .send()?
        .error_for_status()?;
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
fn vcpkg_command(inv: &Inventory, executable: &Path) -> Result<Command> {
    let mut command = Command::new(executable);
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut command)?;
    Ok(command)
}

fn registry_config(directory: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["config", "--local", "--get", key])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned())),
        Some(1) => Ok(None),
        _ => bail!(
            "cannot read vcpkg repository configuration: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn materialize_registry_history(inv: &Inventory, directory: &Path, pins: &Lock) -> Result<()> {
    let keys = [
        "remote.origin.promisor",
        "remote.origin.partialclonefilter",
        "extensions.partialClone",
    ];
    let mut partial = false;
    for key in keys {
        partial |= registry_config(directory, key)?.is_some();
    }
    let shallow = capture(
        Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(["rev-parse", "--is-shallow-repository"]),
    )?;
    let shallow = String::from_utf8_lossy(&shallow).trim() == "true";
    if !(partial || shallow) {
        return Ok(());
    }
    let mut fetch = Command::new("git");
    fetch
        .arg("-C")
        .arg(directory)
        .args(["fetch", "--refetch", "--no-filter", "--tags"]);
    if shallow {
        fetch.arg("--unshallow");
    }
    fetch
        .arg(&pins.vcpkg_url)
        .arg("refs/heads/*:refs/remotes/origin/*");
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut fetch)?;
    run(&mut fetch)?;
    for key in keys {
        if registry_config(directory, key)?.is_some() {
            run(Command::new("git").arg("-C").arg(directory).args([
                "config",
                "--local",
                "--unset-all",
                key,
            ]))?;
        }
    }
    Ok(())
}

pub fn apply_environment(
    inv: &Inventory,
    target: &str,
    component: &str,
    cmd: &mut Command,
) -> Result<()> {
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), cmd)?;
    let selected = needs(inv, target, component)?;
    let subtitle = subtitle_install(inv, target)?;
    if target.contains("windows") {
        cmd.env_remove("ROUKAKU_SUBTITLE_VCPKG_ROOT")
            .env_remove("VCPKG_ROOT")
            .env_remove("BISON_EXECUTABLE");
        let mut paths = Vec::new();
        if selected.subtitle {
            let root = subtitle
                .parent()
                .and_then(Path::parent)
                .context("subtitle root")?;
            cmd.env("ROUKAKU_SUBTITLE_VCPKG_ROOT", root);
            paths.push(subtitle.join("bin"));
        }
        if selected.ffmpeg {
            cmd.env("VCPKG_ROOT", inv.root.join(".work/vcpkg-ffmpeg"));
        }
        if selected.dxc {
            paths.push(inv.root.join(".work/dxc"));
        }
        if selected.bison {
            paths.push(inv.root.join(".work/bison"));
            cmd.env(
                "BISON_EXECUTABLE",
                inv.root.join(".work/bison/win_bison.exe"),
            );
        }
        cmd.env(
            "PATH",
            env::join_paths(
                paths
                    .into_iter()
                    .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
            )?,
        );
        if selected.renpy {
            let base = inv.root.join(".work/native-downloads");
            cmd.env("RENPY_SDK_ROOT", base.join("renpy-sdk"))
                .env("RENPY_PYTHON_DEV", base.join("python"))
                .env("RENPY_SDL_INCLUDE", base.join("sdl2/include"))
                .env("Python3_EXECUTABLE", base.join("python/python.exe"))
                .env(
                    "RENPY_CYTHON_LICENSE",
                    base.join("cython-license/LICENSE.txt"),
                )
                .env("PYTHONDONTWRITEBYTECODE", "1");
        }
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
            let tools = inv.root.join(".work/vcpkg-downloads/tools/msys2");
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
    let target_spec = inv.target(target)?;
    if let Some(core) = core {
        inv.require_supported(core, target)?;
    }
    if !matches!(target_spec.platform.as_str(), "windows" | "android") {
        bail!("no dependency preparation path for blocked target {target}");
    }
    if let Some(id) = core {
        crate::sources::prepare(inv, inv.core(id)?)?;
    }
    let selected = needs(inv, target, core.unwrap_or("manager"))?;
    let pins = lock(inv)?;
    prepare_native_inputs(inv, &pins, selected)?;
    if selected.dxc {
        dxc(inv)?;
    }
    if selected.bison {
        let archive = inv.root.join(".work/downloads/win_flex_bison-2.5.25.zip");
        download(&pins.bison_url, &pins.bison_sha256, &archive)?;
        let out = inv.root.join(".work/bison");
        if !out.exists() {
            package::extract_zip(&archive, &out)?;
        }
    }
    if target_spec.platform == "android" {
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
        if current.as_ref().is_none_or(|version| {
            !String::from_utf8_lossy(version).contains(&pins.cargo_ndk_version)
        }) {
            run(Command::new("cargo")
                .current_dir(&inv.root)
                .args([
                    "install",
                    "--locked",
                    "cargo-ndk",
                    "--version",
                    &format!("={}", pins.cargo_ndk_version),
                    "--root",
                ])
                .arg(inv.root.join(".work/tools")))?;
        }
    }
    if !(selected.subtitle || selected.ffmpeg || selected.native_vcpkg) {
        return Ok(());
    }
    let vcpkg = inv.root.join(".work/vcpkg");
    if !vcpkg.exists() {
        let mut clone = Command::new("git");
        clone.args(["clone", &pins.vcpkg_url]).arg(&vcpkg);
        shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut clone)?;
        run(&mut clone)?;
        run(Command::new("git").arg("-C").arg(&vcpkg).args([
            "checkout",
            "--detach",
            &pins.vcpkg_commit,
        ]))?;
    }
    let actual = capture(
        Command::new("git")
            .arg("-C")
            .arg(&vcpkg)
            .args(["rev-parse", "HEAD"]),
    )?;
    if String::from_utf8_lossy(&actual).trim() != pins.vcpkg_commit {
        bail!("vcpkg cache revision differs from dependency lock");
    }
    materialize_registry_history(inv, &vcpkg, &pins)?;
    let binary = vcpkg.join(if cfg!(windows) { "vcpkg.exe" } else { "vcpkg" });
    if !binary.is_file() {
        let mut bootstrap = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.arg("/c").arg(vcpkg.join("bootstrap-vcpkg.bat"));
            command
        } else {
            let mut command = Command::new("sh");
            command.arg(vcpkg.join("bootstrap-vcpkg.sh"));
            command
        };
        bootstrap.arg("-disableMetrics");
        shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut bootstrap)?;
        run(&mut bootstrap)?;
    }
    if selected.subtitle {
        let subtitle = subtitle_source(inv, target)?;
        let triplet = if target_spec.platform == "android" {
            if target_spec.architecture == "aarch64" {
                "arm64-android-dynbidi"
            } else {
                "x64-android-dynbidi"
            }
        } else {
            &pins.subtitle_windows_triplet
        };
        let install = inv.root.join(".work/vcpkg-subtitle/installed");
        fs::create_dir_all(install.parent().unwrap())?;
        let mut command = vcpkg_command(inv, &binary)?;
        command
            .arg("install")
            .arg("--x-wait-for-lock")
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
        if target_spec.platform == "android" {
            command.env("ANDROID_NDK_HOME", ndk_root(inv)?);
        }
        run(&mut command)?;
        fs::copy(
            vcpkg.join(".vcpkg-root"),
            install.parent().unwrap().join(".vcpkg-root"),
        )?;
    }
    if selected.ffmpeg {
        let ff = inv.root.join(".work/vcpkg-ffmpeg");
        fs::create_dir_all(&ff)?;
        run(vcpkg_command(inv, &binary)?
            .arg("install")
            .arg("--x-wait-for-lock")
            .arg(format!(
                "--x-manifest-root={}",
                inv.root.join("build/vcpkg/ffmpeg").display()
            ))
            .args(["--triplet", &pins.ffmpeg_windows_triplet])
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
    }
    if selected.native_vcpkg {
        let native = inv.core(core.context("native core selection missing")?)?;
        let source = inv
            .root
            .join(native.manifest.components().take(2).collect::<PathBuf>());
        if !source.join("vcpkg.json").is_file() {
            bail!(
                "native core manifest missing vcpkg.json: {}",
                source.display()
            );
        }
        let installed = inv
            .root
            .join(".work/native-vcpkg")
            .join(&native.id)
            .join("installed");
        let mut command = vcpkg_command(inv, &binary)?;
        command
            .arg("install")
            .arg("--x-wait-for-lock")
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
    Ok(())
}

pub fn prepare_all(inv: &Inventory, target: &str) -> Result<()> {
    inv.target(target)?;
    let mut prepared = false;
    for core in &inv.cores {
        if let Some(reason) = inv.blocked(&core.id, target)? {
            eprintln!("{}: blocked: {reason}", core.id);
            continue;
        }
        prepare(inv, target, Some(&core.id))?;
        prepared = true;
    }
    if !prepared {
        bail!("no standalone core supports {target}; select Manager prerequisites instead");
    }
    Ok(())
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use std::io::Write;

    fn cached_zip(root: &Path, entries: &[(&str, &[u8])]) -> (PathBuf, NativeArchive) {
        let archive = root.join(".work/downloads/fixture.zip");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let pin = NativeArchive {
            version: "public-fixture".into(),
            url: "https://not-used.invalid/fixture.zip".into(),
            sha256: package::sha256(&archive).unwrap(),
            file_name: "fixture.zip".into(),
            format: "zip".into(),
            strip_prefix: "fixture".into(),
            include: Vec::new(),
            required: vec!["source.txt".into()],
        };
        (archive, pin)
    }

    #[test]
    fn verified_native_tree_reuses_files_without_an_archive_or_network() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(
            temp.path(),
            &[
                ("fixture/source.txt", b"public source"),
                ("fixture/.gitignore", b"generated/\n"),
            ],
        );
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        let tree = prepare_native_tree(temp.path(), "fixture", &layers, None).unwrap();
        fs::remove_file(&archive).unwrap();
        assert_eq!(
            prepare_native_tree(temp.path(), "fixture", &layers, None).unwrap(),
            tree
        );
        assert_eq!(fs::read(tree.join("source.txt")).unwrap(), b"public source");
        assert_eq!(fs::read(tree.join(".gitignore")).unwrap(), b"generated/\n");
        assert!(!archive.exists());
        fs::write(tree.join("source.txt"), b"user modification").unwrap();
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert_eq!(
            fs::read(tree.join("source.txt")).unwrap(),
            b"user modification"
        );
        assert!(!archive.exists());
    }

    #[test]
    fn bad_native_archive_hash_never_publishes_a_partial_tree() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(temp.path(), &[("fixture/source.txt", b"public source")]);
        fs::write(&archive, b"changed download").unwrap();
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert!(!temp.path().join(".work/native-downloads/fixture").exists());
        assert_eq!(fs::read(&archive).unwrap(), b"changed download");
    }

    #[test]
    fn unsafe_native_archive_cleans_its_staging_and_preserves_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(
            temp.path(),
            &[
                ("fixture/source.txt", b"public source"),
                ("fixture/../escaped.txt", b"unsafe path"),
            ],
        );
        let before = package::sha256(&archive).unwrap();
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert!(!temp.path().join(".work/native-downloads/fixture").exists());
        assert!(!temp
            .path()
            .join(".work/native-downloads/escaped.txt")
            .exists());
        assert!(fs::read_dir(temp.path().join(".work/native-downloads"))
            .unwrap()
            .next()
            .is_none());
        assert_eq!(package::sha256(&archive).unwrap(), before);
    }
}
