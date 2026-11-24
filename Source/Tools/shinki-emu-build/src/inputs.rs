//! Pinned host tools and source archives used by native build scripts.
use crate::{BuildAsset, NativeInputs, capture, lock_file, pins, run, sha256, watch_env, work_root};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io, path::{Path, PathBuf}, process::Command, time::Duration};

#[derive(Deserialize)]
pub(crate) struct Pins {
    pub vcpkg_url: String,
    pub vcpkg_commit: String,
    pub subtitle_windows_triplet: String,
    pub ffmpeg_windows_triplet: String,
    bison_url: String,
    bison_sha256: String,
    native: BTreeMap<String, NativeArchive>,
}

#[derive(Deserialize, Serialize)]
struct NativeArchive {
    url: String,
    sha256: String,
    file_name: String,
    format: String,
    strip_prefix: String,
    #[serde(default)]
    include: Vec<String>,
    required: Vec<String>,
}

#[derive(Serialize)]
struct NativeLayer<'a> {
    archive: &'a NativeArchive,
    destination: &'a str,
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
                let name = entry.name()?.into_owned();
                let directory = entry.is_dir();
                let linked = entry.is_symlink();
                extract(&name, directory, linked, &mut entry)?;
                #[cfg(unix)]
                if !directory {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
                    if let Some(relative) = selected_native_path(archive, &name)? {
                        fs::set_permissions(
                            root.join(destination).join(relative),
                            fs::Permissions::from_mode(mode),
                        )?;
                    }
                }
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
                #[cfg(unix)]
                if kind.is_file() {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(relative) = selected_native_path(archive, &name)? {
                        fs::set_permissions(
                            root.join(destination).join(relative),
                            fs::Permissions::from_mode(entry.header().mode()? & 0o777),
                        )?;
                    }
                }
            }
        }
        other => bail!("unknown native archive format {other}"),
    }
    for required in &archive.required {
        native_path(required)?;
        let path = root.join(destination).join(required);
        ensure!(path.exists(), "native archive lacks required input {}", path.display());
    }
    Ok(())
}
fn prepare_tree(name: &str, layers: &[NativeLayer<'_>]) -> Result<PathBuf> {
    prepare_tree_in(&work_root(), name, layers)
}

fn prepare_tree_in(work: &Path, name: &str, layers: &[NativeLayer<'_>]) -> Result<PathBuf> {
    let key = Sha256::digest(serde_json::to_vec(layers)?);
    let parent = work.join("native-downloads");
    fs::create_dir_all(&parent)?;
    let out = parent.join(format!("{name}-{}", hex::encode(&key[..8])));
    let _lock = lock_file(&out.with_extension("lock"))?;
    if out.is_dir() {
        return Ok(out);
    }
    let stage = tempfile::tempdir_in(&parent)?;
    let stage = stage.path();
    for layer in layers {
        let input = download_in(work, &layer.archive.url, &layer.archive.sha256, &layer.archive.file_name)?;
        extract_native_archive(layer.archive, &input, stage, layer.destination)?;
    }
    if name == "sdl2-posix" {
        run(Command::new("sh")
            .arg("configure")
            .arg("--disable-shared")
            .current_dir(stage))?;
    }
    if name == "python" && cfg!(unix) {
        build_host_python(stage)?;
    }
    fs::rename(stage, &out)?;
    Ok(out)
}

fn build_host_python(stage: &Path) -> Result<()> {
    let openssl = stage.join("host-openssl");
    if cfg!(target_os = "macos") {
        let source = stage.join("build-openssl");
        run(Command::new("perl")
            .arg("Configure")
            .args(["no-shared", "no-tests", "--openssldir=/etc/ssl"])
            .arg(format!("--prefix={}", openssl.display()))
            .current_dir(&source))?;
        run(Command::new("make").arg("-j3").current_dir(&source))?;
        run(Command::new("make").arg("install_sw").current_dir(&source))?;
    }
    let mut configure = Command::new("sh");
    configure
        .arg("configure")
        .arg("--without-ensurepip")
        .current_dir(stage)
        .env("PYTHONDONTWRITEBYTECODE", "1");
    if cfg!(target_os = "macos") {
        configure.arg(format!("--with-openssl={}", openssl.display()));
    }
    run(&mut configure)?;
    run(Command::new("make")
        .args(["-j3", "all"])
        .current_dir(stage)
        .env("PYTHONDONTWRITEBYTECODE", "1"))?;
    // CPython's build tree links extension modules back to Modules/.
    // Publish ordinary files so the verified tree remains relocatable.
    let modules = fs::read_to_string(stage.join("pybuilddir.txt"))?;
    native_path(modules.trim())?;
    let stage_root = fs::canonicalize(stage)?;
    for entry in fs::read_dir(stage.join(modules.trim()))? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            let source = fs::canonicalize(entry.path())?;
            if !source.starts_with(&stage_root) || !source.is_file() {
                bail!("CPython generated an external module link");
            }
            fs::remove_file(entry.path())?;
            fs::copy(source, entry.path())?;
        }
    }
    // Keep the executable distinct from CPython's Python/ source directory.
    let makefile = fs::read_to_string(stage.join("Makefile"))?;
    let suffix = makefile
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == "BUILDEXE").then(|| value.trim())
        })
        .context("CPython BUILDEXE output")?;
    let executable = format!("python{suffix}");
    native_path(&executable)?;
    fs::rename(stage.join(executable), stage.join(build_python_relative()))?;
    Ok(())
}

fn build_python_relative() -> &'static str {
    if cfg!(windows) {
        "python.exe"
    } else {
        "build-python"
    }
}

fn renpy_host_pin(name: &str) -> String {
    if !cfg!(windows) {
        format!("{name}-posix")
    } else {
        name.into()
    }
}

fn download(url: &str, digest: &str, name: &str) -> Result<PathBuf> {
    download_in(&work_root(), url, digest, name)
}

/// Download a checked vcpkg source archive independently of vcpkg's cache.
pub fn download_sha512(url: &str, digest: &str, name: &str) -> Result<PathBuf> {
    let path = download_checked(&work_root(), url, digest, name, 128, crate::file_digest::<sha2::Sha512>)?;
    println!("cargo:rerun-if-changed={}", path.display());
    Ok(path)
}

fn download_in(work: &Path, url: &str, digest: &str, name: &str) -> Result<PathBuf> {
    download_checked(work, url, digest, name, 64, sha256)
}

fn download_checked(
    work: &Path,
    url: &str,
    digest: &str,
    name: &str,
    digest_len: usize,
    hash: fn(&Path) -> Result<String>,
) -> Result<PathBuf> {
    ensure!(url.starts_with("https://") && digest.len() == digest_len && digest.bytes().all(|c| c.is_ascii_hexdigit()), "invalid pinned download {url}");
    native_path(name)?;
    ensure!(!name.contains('/'), "download expects a filename");
    let parent = work.join("downloads").join(digest);
    fs::create_dir_all(&parent)?;
    let _lock = lock_file(&parent.join("download.lock"))?;
    let output = parent.join(name);
    if !output.is_file() {
        let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
        let mut response = reqwest::blocking::Client::builder()
            .user_agent("Project-Shinkirou-build")
            .timeout(Duration::from_secs(3600))
            .build()?.get(url).send()?.error_for_status()?;
        io::copy(&mut response, &mut temp)?;
        ensure!(hash(temp.path())? == digest, "archive digest mismatch for {url}");
        temp.persist_noclobber(&output).map_err(|err| err.error)?;
    } else {
        ensure!(hash(&output)? == digest, "archive digest mismatch for cached {}", output.display());
    }
    Ok(output)
}

fn pinned_tree(lock: &Pins, name: &str) -> Result<PathBuf> {
    let archive = lock.native.get(name).with_context(|| format!("no native archive {name}"))?;
    prepare_tree(name, &[NativeLayer { archive, destination: "" }])
}

pub fn impacto_sources() -> Result<(PathBuf, PathBuf)> {
    let lock = pins()?;
    Ok((pinned_tree(&lock, "libatrac9")?, pinned_tree(&lock, "magic_enum")?))
}

pub fn bison() -> Result<PathBuf> {
    let executable = find_bison()?;
    println!("cargo:rerun-if-changed={}", executable.display());
    Ok(executable)
}

fn find_bison() -> Result<PathBuf> {
    if let Some(executable) = watch_env("BISON_EXECUTABLE") {
        return resolve_bison(Path::new(&executable));
    }
    if cfg!(windows) {
        let lock = pins()?;
        let file_name = lock.bison_url.rsplit('/').next().context("Bison archive name")?.to_owned();
        let archive = NativeArchive {
            url: lock.bison_url, sha256: lock.bison_sha256, file_name,
            format: "zip".into(), strip_prefix: String::new(), include: Vec::new(),
            required: vec!["win_bison.exe".into()],
        };
        return Ok(prepare_tree("bison", &[NativeLayer { archive: &archive, destination: "" }])?.join("win_bison.exe"));
    }
    if cfg!(target_os = "macos") {
        for prefix in ["/opt/homebrew/opt/bison/bin", "/usr/local/opt/bison/bin"] {
            let path = Path::new(prefix).join("bison");
            if path.is_file() { return Ok(path); }
        }
    }
    resolve_bison(Path::new("bison"))
}

fn resolve_bison(executable: &Path) -> Result<PathBuf> {
    if executable.is_file() {
        return Ok(std::path::absolute(executable)?);
    }
    if executable.parent() != Some(Path::new("")) {
        bail!("Bison executable does not exist: {}", executable.display());
    }
    if let Some(path) = watch_env("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(executable);
            if candidate.is_file() {
                return Ok(std::path::absolute(candidate)?);
            }
            if cfg!(windows) && candidate.extension().is_none() {
                let candidate = candidate.with_extension("exe");
                if candidate.is_file() {
                    return Ok(std::path::absolute(candidate)?);
                }
            }
        }
    }
    bail!("{} not found on PATH; set BISON_EXECUTABLE", executable.display())
}

pub fn dxc(lock: &Path) -> Result<()> {
    println!("cargo:rerun-if-changed={}", lock.display());
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(lock)?)?;
    let value = |key| manifest[key].as_str().with_context(|| format!("DXC lock missing {key}"));
    let archive = download(value("url")?, value("sha256")?, value("asset")?)?;
    let mut archive = zip::ZipArchive::new(fs::File::open(archive)?)?;
    let root = crate::out_dir()?.join("dxc");
    fs::create_dir_all(&root)?;
    let mut inputs = NativeInputs::default();
    for entry in [value("library")?, value("validator")?, "LICENSE-MS.txt", "LICENSE-LLVM.txt", "LICENCE-MIT.txt"] {
        let mut file = archive.by_name(entry)?;
        ensure!(!file.is_symlink() && !file.is_dir(), "invalid DXC ZIP member {entry}");
        let name = entry.rsplit(['/', '\\']).next().context("DXC member name")?;
        native_path(name)?;
        let source = root.join(name);
        io::copy(&mut file, &mut fs::File::create(&source)?)?;
        if name.ends_with(".dll") {
            inputs.runtime.push(source.clone());
        }
        inputs.package_files.push(BuildAsset { source, dest: name.into() });
    }
    inputs.write()
}

pub struct Renpy {
    pub sdk: PathBuf,
    pub python: PathBuf,
    pub interpreter: PathBuf,
    pub sdl_include: PathBuf,
    pub cython_license: PathBuf,
    pub static_root: Option<PathBuf>,
}

pub fn renpy(recipe: &Path, target: &str, static_build: bool) -> Result<Renpy> {
    let lock = pins()?;
    let sdk_file = recipe.join("sdk.json");
    println!("cargo:rerun-if-changed={}", sdk_file.display());
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(sdk_file)?)?;
    let get = |key: &str| lock.native.get(key).with_context(|| format!("missing {key} pin"));
    let python_pin = get(&renpy_host_pin("python"))?;
    let cython = get(&renpy_host_pin("cython"))?;
    let sdk = pinned_tree(&lock, "renpy-sdk")?;
    let mut python_layers = vec![
        NativeLayer { archive: python_pin, destination: "" },
        NativeLayer { archive: cython, destination: if cfg!(windows) { "Lib/site-packages" } else { "Lib" } },
    ];
    if cfg!(target_os = "macos") {
        python_layers.push(NativeLayer { archive: get("renpy-host-openssl")?, destination: "build-openssl" });
    }
    let python = prepare_tree("python", &python_layers)?;
    let interpreter = python.join(build_python_relative());
    let sdl_include = if static_build {
        // Static builds use the SDL headers from the target SDK.
        PathBuf::new()
    } else {
        pinned_tree(&lock, &renpy_host_pin("sdl2"))?.join("include")
    };
    let cython_license = pinned_tree(&lock, "cython-license")?.join("LICENSE.txt");
    let static_root = if static_build {
        let root = if let Some(root) = watch_env("RENPY_STATIC_ROOT") {
            let root = dunce::canonicalize(root)?;
            println!("cargo:rerun-if-changed={}", root.display());
            root
        } else if target.ends_with("-linux-android") {
            renpy_android(recipe, target, &manifest, &interpreter)?
        } else {
            bail!("set RENPY_STATIC_ROOT to the target Ren'Py runtime archives for {target}");
        };
        if target.ends_with("-linux-android") {
            let sysconfig = root.join("Resources/lib").join(target).join("python3.12/_sysconfigdata__linux_.py");
            ensure!(sysconfig.is_file(), "missing Ren'Py runtime for {target}: {}", sysconfig.display());
        }
        Some(root)
    } else {
        None
    };
    Ok(Renpy { sdk, python, interpreter, sdl_include, cython_license, static_root })
}

fn renpy_android(recipe: &Path, target: &str, sdk: &serde_json::Value, python: &Path) -> Result<PathBuf> {
    let recipe_files = [
        "android_runtime.py",
        "android/SDL2-offscreen-default-display.patch",
        "sdk.json",
        "../licenses/renpy/python-sources.json",
    ];
    let mut key = Sha256::new();
    for file in recipe_files {
        let path = recipe.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        key.update(sha256(&path)?);
    }
    let runtime = work_root().join("native-downloads/renpy-android").join(target)
        .join(hex::encode(&key.finalize()[..8]));
    fs::create_dir_all(runtime.parent().unwrap())?;
    let _runtime_lock = lock_file(&runtime.with_extension("lock"))?;
    if runtime.is_dir() {
        return Ok(runtime);
    }
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "Ren'Py {target} runtime must be built on Linux x86_64 with {}, then supplied as RENPY_STATIC_ROOT",
        recipe.join("android_runtime.py").display()
    );
    let android = &sdk["android"];
    let build_rev = android["renpy_build"].as_str().context("Ren'Py build revision")?;
    let source_rev = sdk["source_revision"].as_str().context("Ren'Py source revision")?;
    let build = work_root().join(format!("renpy-build-{build_rev}"));
    let _build_lock = lock_file(&build.with_extension("lock"))?;
    pinned_checkout(&build, "https://github.com/renpy/renpy-build.git", build_rev)?;
    pinned_checkout(&build.join("renpy"), "https://github.com/renpy/renpy.git", source_rev)?;
    let archives = build.join("tars");
    fs::create_dir_all(&archives)?;
    for (key, name) in [("ndk", "android-ndk-r29-linux.zip"), ("cubism", "CubismSdkForNative-4-r.6.2.zip")] {
        let archive = download(
            android[format!("{key}_url")].as_str().context("Ren'Py archive URL")?,
            android[format!("{key}_sha256")].as_str().context("Ren'Py archive SHA256")?,
            name,
        )?;
        let destination = archives.join(name);
        if !destination.is_file() {
            fs::hard_link(archive, destination)?;
        }
    }
    let stage = tempfile::tempdir_in(runtime.parent().unwrap())?;
    let output = stage.path().join("runtime");
    run(Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(recipe.join("android_runtime.py"))
        .arg("--build-root").arg(build)
        .args(["--target", target])
        .arg("--output").arg(&output)
        .arg("--host-python").arg(python))?;
    fs::rename(output, &runtime)?;
    Ok(runtime)
}

fn pinned_checkout(path: &Path, url: &str, rev: &str) -> Result<()> {
    if path.is_dir() && capture(Command::new("git").arg("-C").arg(path).args(["rev-parse", "HEAD"]))
        .is_ok_and(|actual| actual.trim() == rev)
    {
        return Ok(());
    }
    // Some upstream repos carry an unpopulated gitlink at the nested path.
    // Git clone accepts the empty directory; stage next to it for atomicity.
    let stage = tempfile::tempdir_in(path.parent().context("checkout parent")?)?;
    let checkout = stage.path().join("source");
    let git = || {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&checkout).args(["-c", "core.autocrlf=false"]);
        cmd
    };
    fs::create_dir(&checkout)?;
    run(git().args(["init", "-q"]))?;
    run(git().args(["fetch", "--depth", "1", "--no-tags", url, rev]))?;
    run(git().args(["checkout", "--detach", rev]))?;
    run(git().args(["submodule", "update", "--init", "--recursive"]))?;
    if path.is_dir() {
        fs::remove_dir(path).with_context(|| format!("existing checkout differs: {}", path.display()))?;
    }
    fs::rename(checkout, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(work: &Path, entries: &[(&str, &[u8])]) -> Result<(PathBuf, NativeArchive)> {
        let temp = tempfile::NamedTempFile::new_in(work)?;
        let mut zip = zip::ZipWriter::new(temp.reopen()?);
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default().unix_permissions(0o755))?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
        let sha256 = sha256(temp.path())?;
        let directory = work.join("downloads").join(&sha256);
        fs::create_dir_all(&directory)?;
        let path = directory.join("fixture.zip");
        temp.persist(&path)?;
        Ok((path, NativeArchive {
            url: "https://not-used.invalid/fixture.zip".into(),
            sha256, file_name: "fixture.zip".into(), format: "zip".into(), strip_prefix: "fixture".into(),
            include: vec![], required: vec!["source.txt".into()],
        }))
    }

    #[test]
    fn prepared_source_tree_is_reused_without_fetching() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let (zip, pin) = fixture(temp.path(), &[("fixture/source.txt", b"source")])?;
        let layers = [NativeLayer { archive: &pin, destination: "" }];
        let out = prepare_tree_in(temp.path(), "fixture", &layers)?;
        fs::remove_file(&zip)?;
        assert_eq!(prepare_tree_in(temp.path(), "fixture", &layers)?, out);
        assert_eq!(fs::read(out.join("source.txt"))?, b"source");
        assert!(!zip.exists());
        Ok(())
    }

    #[test]
    fn sha512_download_uses_only_a_matching_cached_archive() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let bytes = b"pinned archive";
        let digest = hex::encode(sha2::Sha512::digest(bytes));
        let directory = temp.path().join("downloads").join(&digest);
        fs::create_dir_all(&directory)?;
        let archive = directory.join("source.tar.gz");
        fs::write(&archive, bytes)?;
        let fetch = || download_checked(
            temp.path(), "https://not-used.invalid/source.tar.gz", &digest,
            "source.tar.gz", 128, crate::file_digest::<sha2::Sha512>,
        );
        assert_eq!(fetch()?, archive);
        fs::write(&archive, b"corrupted")?;
        assert!(fetch().is_err());
        Ok(())
    }

    #[test]
    fn corrupt_or_escaping_archives_never_publish_a_tree() -> Result<()> {
        for (entries, corrupt) in [
            (vec![("fixture/source.txt", &b"public source"[..])], true),
            (vec![("fixture/source.txt", &b"public source"[..]), ("fixture/../escaped.txt", &b"unsafe"[..])], false),
        ] {
            let temp = tempfile::tempdir()?;
            let (archive, pin) = fixture(temp.path(), &entries)?;
            if corrupt {
                fs::write(&archive, b"changed download")?;
            }
            let before = sha256(&archive)?;
            let layers = [NativeLayer { archive: &pin, destination: "" }];
            assert!(prepare_tree_in(temp.path(), "fixture", &layers).is_err());
            for entry in fs::read_dir(temp.path().join("native-downloads"))? {
                assert_eq!(entry?.path().extension().and_then(|s| s.to_str()), Some("lock"));
            }
            assert_eq!(sha256(&archive)?, before);
            assert!(!temp.path().join("escaped.txt").exists());
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn native_archives_preserve_executable_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"#!/bin/sh\nexit 7\n";
        for format in ["zip", "tar-gz"] {
            let input = temp.path().join(format);
            if format == "zip" {
                let mut zip = zip::ZipWriter::new(fs::File::create(&input).unwrap());
                zip.start_file(
                    "fixture/tool",
                    zip::write::SimpleFileOptions::default().unix_permissions(0o755),
                )
                .unwrap();
                zip.write_all(bytes).unwrap();
                zip.finish().unwrap();
            } else {
                let gzip = flate2::write::GzEncoder::new(
                    fs::File::create(&input).unwrap(),
                    flate2::Compression::default(),
                );
                let mut tar = tar::Builder::new(gzip);
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append_data(&mut header, "fixture/tool", &bytes[..])
                    .unwrap();
                tar.into_inner().unwrap().finish().unwrap();
            }
            let pin = NativeArchive {
                url: "https://not-used.invalid/fixture".into(),
                sha256: sha256(&input).unwrap(),
                file_name: format.into(),
                format: format.into(),
                strip_prefix: "fixture".into(),
                include: Vec::new(),
                required: vec!["tool".into()],
            };
            let out = temp.path().join(format!("out-{format}"));
            extract_native_archive(&pin, &input, &out, "bin").unwrap();
            assert_eq!(
                Command::new(out.join("bin/tool")).status().unwrap().code(),
                Some(7)
            );
        }
    }

}
