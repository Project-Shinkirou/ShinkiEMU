use crate::{
    binary,
    build::{self, BuildArtifact},
    cores::Inventory,
    dependencies,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shinki_emu_build::BuildAsset;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

#[path = "android_host.rs"]
mod android_host;

pub fn portable(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("empty package path");
    }
    for p in name.split('/') {
        let stem = p
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if p.is_empty()
            || !p.as_bytes()[0].is_ascii_alphanumeric() && p.as_bytes()[0] != b'_'
            || !p
                .bytes()
                .all(|b| b.is_ascii() && !b.is_ascii_control() && !b"\\:<>|?*\"".contains(&b))
            || p.ends_with(['.', ' '])
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("unsafe package path {name}");
        }
    }
    Ok(())
}
pub fn plain(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    for component in absolute.ancestors() {
        let m = fs::symlink_metadata(component)
            .with_context(|| format!("reading {}", component.display()))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                bail!("reparse input {}", component.display());
            }
        }
        if m.file_type().is_symlink() || (!m.is_file() && !m.is_dir()) {
            bail!("linked or special input {}", component.display());
        }
    }
    Ok(())
}
pub fn sha256(path: &Path) -> Result<String> {
    plain(path)?;
    sha256_reader(fs::File::open(path)?)
}

pub(crate) fn sha256_reader(mut reader: impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
pub fn tree(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    plain(root)?;
    if !root.is_dir() {
        bail!("not a directory {}", root.display());
    }
    let mut result = Vec::new();
    for e in walkdir::WalkDir::new(root).follow_links(false) {
        let e = e?;
        plain(e.path())?;
        if e.file_type().is_file() {
            let relative = e
                .path()
                .strip_prefix(root)?
                .to_str()
                .context("non UTF-8 package path")?
                .replace('\\', "/");
            portable(&relative)?;
            result.push((e.path().to_owned(), relative));
        }
    }
    result.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(result)
}
pub fn publish_noreplace(source: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        fs::rename(source, dest)?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let a = CString::new(source.as_os_str().as_bytes())?;
        let b = CString::new(dest.as_os_str().as_bytes())?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let a = CString::new(source.as_os_str().as_bytes())?;
        let b = CString::new(dest.as_os_str().as_bytes())?;
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        bail!("no atomic no-replace publish primitive on this host");
    }
    Ok(())
}
pub fn atomic_directory(out: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if fs::symlink_metadata(out).is_ok() {
        bail!("output must be new: {}", out.display());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    plain(parent)?;
    let temp = tempfile::Builder::new()
        .prefix(".shinki-stage-")
        .tempdir_in(parent)?;
    let stage = temp.path().join("package");
    fs::create_dir(&stage)?;
    f(&stage)?;
    publish_noreplace(&stage, out)
}
pub fn copy_entries(entries: &[(PathBuf, String)], stage: &Path) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (source, name) in entries {
        portable(name)?;
        plain(source)?;
        if !source.is_file() {
            bail!("not a regular file {}", source.display());
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            bail!("case-insensitive package collision: {name}");
        }
        // Detect file/directory collisions before copying anything.
        for ancestor in Path::new(name)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if seen.contains(
                &ancestor
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            ) {
                bail!("file/directory collision {name}");
            }
        }
    }
    for (source, name) in entries {
        let dest = stage.join(name);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(source, &dest)?;
    }
    Ok(())
}
#[cfg(test)]
pub fn extract_zip(archive: &Path, out: &Path) -> Result<()> {
    atomic_directory(out, |stage| {
        let mut z = zip::ZipArchive::new(fs::File::open(archive)?)?;
        let mut names = BTreeSet::new();
        for i in 0..z.len() {
            let mut item = z.by_index(i)?;
            if item.is_symlink() {
                bail!("linked archive input");
            }
            let name = item.name()?.trim_end_matches('/').to_owned();
            portable(&name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                bail!("archive path collision");
            }
            let dest = stage.join(&name);
            if item.is_dir() {
                fs::create_dir_all(dest)?;
            } else {
                fs::create_dir_all(dest.parent().unwrap())?;
                io::copy(
                    &mut item,
                    &mut fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(dest)?,
                )?;
            }
        }
        Ok(())
    })
}
pub fn zip_entries(entries: &[(PathBuf, String)], out: &Path) -> Result<()> {
    if fs::symlink_metadata(out).is_ok() {
        bail!("archive output must be new");
    }
    let parent = out.parent().context("archive parent")?;
    fs::create_dir_all(parent)?;
    plain(parent)?;
    // tempfile persists through Win32 directly; retain canonical extended paths.
    let parent = fs::canonicalize(parent)?;
    let out = parent.join(out.file_name().context("archive filename")?);
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    let mut seen = BTreeSet::new();
    let mut tmp = tempfile::NamedTempFile::new_in(&parent)?;
    {
        let mut zip = zip::ZipWriter::new(tmp.as_file_mut());
        for (source, name) in entries {
            portable(&name)?;
            plain(&source)?;
            if !seen.insert(name.to_ascii_lowercase()) {
                bail!("duplicate archive path");
            }
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&source)?.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            };
            #[cfg(not(unix))]
            let permissions = 0o644;
            let opt = zip::write::SimpleFileOptions::default()
                .last_modified_time(zip::DateTime::default())
                .large_file(fs::metadata(&source)?.len() >= u64::from(u32::MAX))
                .unix_permissions(permissions)
                .compression_method(if name.starts_with("downloads/") || name.ends_with(".ipa") {
                    zip::CompressionMethod::Stored
                } else {
                    zip::CompressionMethod::Deflated
                });
            zip.start_file(name, opt)?;
            io::copy(&mut fs::File::open(source)?, &mut zip)?;
        }
        zip.finish()?;
    }
    tmp.persist_noclobber(&out)
        .map_err(|e| e.error)
        .with_context(|| {
            format!(
                "persisting ZIP {} (path bytes {}; parent {} exists={})",
                out.display(),
                out.as_os_str().len(),
                parent.display(),
                parent.is_dir()
            )
        })?;
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorePackage {
    pub schema: String,
    pub plugin_id: String,
    pub library: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<shinki_emu_metadata::CoreIdentity>,
    #[serde(default, skip_serializing_if = "shinki_emu_metadata::CoreRecognition::is_empty")]
    pub recognition: shinki_emu_metadata::CoreRecognition,
    pub abi: String,
    pub files: Vec<FileRecord>,
}

const PRODUCT_SOURCE_COMMIT: &str = "source-commit.txt";

pub(crate) fn write_product_source_commit(dir: &Path, commit: &str) -> Result<FileRecord> {
    use std::io::Write;

    ensure!(
        commit.len() == 40 && commit.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "invalid product source commit"
    );
    fs::File::create_new(dir.join(PRODUCT_SOURCE_COMMIT))?.write_all(commit.as_bytes())?;
    Ok(FileRecord {
        path: PRODUCT_SOURCE_COMMIT.into(),
        sha256: hex::encode(Sha256::digest(commit)),
    })
}
pub(crate) fn asset_entries(asset: &BuildAsset) -> Result<Vec<(PathBuf, String)>> {
    portable(&asset.dest)?;
    plain(&asset.source)?;
    if asset.source.is_dir() {
        let entries = tree(&asset.source)?;
        if entries.is_empty() {
            bail!("empty resource directory");
        }
        Ok(entries
            .into_iter()
            .map(|(s, n)| (s, format!("{}/{n}", asset.dest)))
            .collect())
    } else {
        Ok(vec![(asset.source.clone(), asset.dest.clone())])
    }
}
fn reject_overlap(source: &Path, out: &Path) -> Result<()> {
    let mut destination = PathBuf::new();
    for component in std::path::absolute(out)?.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                destination.pop();
            }
            other => destination.push(other.as_os_str()),
        }
    }
    let mut ancestor = destination.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().context("output has no existing parent")?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    plain(ancestor)?;
    let destination = fs::canonicalize(ancestor)?.join(destination.strip_prefix(ancestor)?);
    let source = fs::canonicalize(source)?;
    if source.starts_with(&destination) || (source.is_dir() && destination.starts_with(&source)) {
        bail!("output overlaps package input {}", source.display());
    }
    Ok(())
}
pub fn stage_core(
    result: &BuildArtifact,
    version: &str,
    identity: Option<&shinki_emu_metadata::CoreIdentity>,
    recognition: &shinki_emu_metadata::CoreRecognition,
    out: &Path,
    search: &[PathBuf],
) -> Result<PathBuf> {
    shinki_emu_metadata::validate_core_version(version, identity)?;
    let m = result
        .core_build
        .as_ref()
        .context("missing core-build manifest")?;
    m.validate()?;
    let android = result.target.ends_with("-linux-android");
    if m.licenses.is_empty() {
        bail!("core package requires license files");
    }
    let library = result
        .library
        .file_name()
        .and_then(|s| s.to_str())
        .context("core filename")?;
    if !(library.starts_with("shinki_emu_") || library.starts_with("libshinki_emu_")) {
        bail!("core library needs the shinki_emu_ prefix");
    }
    if android && !library.ends_with(".so") {
        bail!("Android core library needs the .so extension");
    }
    let stem = Path::new(library)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("core stem")?;
    portable(stem)?;
    let mut entries = vec![(result.library.clone(), library.into())];
    for asset in m.resources.iter().chain(&m.licenses) {
        entries.extend(asset_entries(asset)?);
    }
    if android && entries.iter().any(|(_, path)| android_host::is_runtime_asset(path)) {
        bail!("Android core packages cannot contain APK-only host compile inputs");
    }
    let is_runtime = |path: &Path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        name.ends_with(".dll")
            || name.ends_with(".so")
            || name.ends_with(".dylib")
            || name.ends_with(".pyd")
            || name.contains(".so.")
    };
    if android
        && entries.iter().any(|(_, relative)| {
            Path::new(relative)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| binary::system(name, &result.target))
        })
    {
        bail!("Android core package includes a device system library");
    }
    let mut roots: Vec<PathBuf> = entries
        .iter()
        .filter(|(path, relative)| is_runtime(if android { Path::new(relative) } else { path }))
        .map(|(path, _)| path.clone())
        .collect();
    let mut runtimes = std::collections::BTreeMap::<String, (PathBuf, String)>::new();
    let mut add_runtime = |path: PathBuf, name: String| -> Result<()> {
        if result.target.contains("apple") && entries.iter().any(|(source, _)| source == &path) {
            return Ok(());
        }
        let key = name.to_ascii_lowercase();
        if key == library.to_ascii_lowercase() {
            if sha256(&path)? != sha256(&result.library)? {
                bail!("runtime conflicts with the core library");
            }
            return Ok(());
        }
        if key.starts_with("shinki_emu_")
            || key.starts_with("libshinki_emu_")
            || ["core.json", "licenses", "resources"].contains(&key.as_str())
        {
            bail!("runtime dependency collides with core layout");
        }
        if let Some((existing, existing_name)) = runtimes.get(&key) {
            if (android && existing_name != &name)
                || (existing != &path && sha256(existing)? != sha256(&path)?)
            {
                bail!("conflicting runtime dependency {name}");
            }
        } else {
            runtimes.insert(key, (path, name));
        }
        Ok(())
    };
    if m.artifact_kind == "native-shared" {
        // CMake's recorded install directory also holds modules loaded by name,
        // such as ANGLE; these do not necessarily appear in static imports.
        let directory = result
            .library
            .parent()
            .context("native install directory")?;
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            if result.target == "x86_64-unknown-linux-gnu"
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        let soname = name
                            .find(".so.")
                            .and_then(|offset| {
                                name[offset + 4..]
                                    .find('.')
                                    .map(|end| &name[..offset + 4 + end])
                            })
                            .unwrap_or(name);
                        binary::system(soname, &result.target)
                    })
            {
                continue;
            }
            if path.is_file() && is_runtime(&path) && path != result.library {
                let name = path
                    .file_name()
                    .context("runtime filename")?
                    .to_str()
                    .context("runtime filename is not UTF-8")?
                    .to_owned();
                if android && binary::system(&name, &result.target) {
                    bail!("Android native install includes device system library {name}");
                }
                let source = binary::runtime_source(&path, directory, &result.target)?;
                add_runtime(source.clone(), name)?;
                roots.push(source);
            }
        }
    }
    let mut search = search.to_vec();
    search.extend(
        roots
            .iter()
            .filter_map(|path| path.parent().map(Path::to_owned)),
    );
    search.sort();
    search.dedup();
    for root in roots {
        for (source, name) in binary::closure(&root, &search, &result.target)? {
            add_runtime(source, name)?;
        }
    }
    entries.extend(runtimes.into_values());
    reject_overlap(&result.library, out)?;
    for asset in m.resources.iter().chain(&m.licenses) {
        reject_overlap(&asset.source, out)?;
    }
    atomic_directory(out, |stage| {
        let dir = stage.join(stem);
        fs::create_dir(&dir)?;
        copy_entries(&entries, &dir)?;
        if crate::apple::supported(&result.target) {
            for (_, relative) in &entries {
                if is_runtime(Path::new(relative)) {
                    crate::apple::sign_binary(&dir.join(relative))?;
                }
            }
        }
        let mut files = tree(&dir)?
            .into_iter()
            .map(|(s, n)| {
                Ok(FileRecord {
                    path: n,
                    sha256: sha256(&s)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !files.iter().any(|file| file.path.split('/').next()
                .is_some_and(|name| name.eq_ignore_ascii_case(PRODUCT_SOURCE_COMMIT))),
            "core inputs collide with {PRODUCT_SOURCE_COMMIT}"
        );
        if let Some(commit) = result.source_commit.as_deref() {
            ensure!(identity.is_none() && version != "0.0.0-local", "invalid product core release");
            files.push(write_product_source_commit(&dir, commit)?);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest = CorePackage {
            schema: "shinki.emu.core_package.v1".into(),
            plugin_id: m.plugin_id.clone(),
            library: library.into(),
            version: version.into(),
            identity: identity.cloned(),
            recognition: recognition.clone(),
            abi: shinki_emu_build::ABI.into(),
            files,
        };
        fs::write(dir.join("core.json"), serde_json::to_vec_pretty(&manifest)?)?;
        verify_core(&dir)?;
        Ok(())
    })?;
    Ok(out.join(stem))
}
pub fn verify_core(dir: &Path) -> Result<CorePackage> {
    let m: CorePackage = serde_json::from_slice(&fs::read(dir.join("core.json"))?)?;
    if m.schema != "shinki.emu.core_package.v1" || m.abi != shinki_emu_build::ABI {
        bail!("invalid core package protocol");
    }
    portable(&m.plugin_id)?;
    shinki_emu_metadata::validate_core_version(&m.version, m.identity.as_ref())?;
    portable(&m.library)?;
    if m.library.contains('/') || Path::new(&m.library).file_stem() != dir.file_name() {
        bail!("core library/stem mismatch");
    }
    let mut expected = BTreeSet::from(["core.json".to_owned()]);
    for f in &m.files {
        portable(&f.path)?;
        if !expected.insert(f.path.to_ascii_lowercase()) {
            bail!("duplicate core file");
        }
        if sha256(&dir.join(&f.path))? != f.sha256 {
            bail!("core file hash mismatch: {}", f.path);
        }
    }
    if !expected.contains(&m.library.to_ascii_lowercase())
        || !expected.iter().any(|s| s.starts_with("licenses/"))
    {
        bail!("core package omits library/licenses");
    }
    let actual = tree(dir)?
        .into_iter()
        .map(|(_, n)| n.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if actual != expected {
        bail!("core package contains unlisted files");
    }
    Ok(m)
}
pub(crate) fn search_dirs(inv: &Inventory, result: &BuildArtifact) -> Result<Vec<PathBuf>> {
    let mut dirs = vec![
        result
            .library
            .parent()
            .context("library parent")?
            .to_owned(),
    ];
    for input in &result.native_inputs {
        for path in &input.runtime {
            let dir = path.parent().context("native runtime parent")?;
            if !dirs.iter().any(|seen| seen == dir) {
                dirs.push(dir.to_owned());
            }
        }
    }
    if let Some(native) = &result.core_build
        && let Some(p) = &native.native_library
    {
        dirs.push(p.parent().context("native library parent")?.to_owned());
        dirs.push(p.parent().context("native library parent")?.join("lib"));
    }
    // MSVC runtime redistributables are explicit SDK redistributable inputs, never System32.
    if result.target.contains("windows")
        && let Some(redist) = std::env::var_os("VCToolsRedistDir")
    {
        let root = PathBuf::from(redist).join("x64");
        if root.is_dir() {
            for d in fs::read_dir(root)? {
                let p = d?.path();
                if p.is_dir() {
                    dirs.push(p);
                }
            }
        }
    }
    if let Some(target) = shinki_emu_build::ohos::Target::from_rust_target(&result.target) {
        // libc++_shared.so; system libraries come from the device.
        dirs.push(target.cxx_runtime_dir(&dependencies::ohos_sdk_native()?));
    }
    if result.target.contains("android") {
        let ndk = dependencies::ndk_root(inv)?;
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        if result.native_inputs.iter().any(|input| input.ndk_openmp) {
            let clang = ndk
                .join("toolchains/llvm/prebuilt")
                .join(host)
                .join("bin")
                .join(if cfg!(windows) { "clang.exe" } else { "clang" });
            let runtime = dependencies::capture(
                std::process::Command::new(clang)
                    .arg(format!("--target={}{}", result.target, binary::ANDROID_API))
                    .arg("--print-file-name=libomp.so"),
            )?;
            let runtime = PathBuf::from(String::from_utf8(runtime)?.trim());
            let runtime = dunce::canonicalize(&runtime)
                .context("NDK Clang did not resolve its OpenMP runtime")?;
            if !runtime.starts_with(dunce::canonicalize(&ndk)?) {
                bail!("OpenMP runtime is outside the selected NDK");
            }
            dirs.push(
                runtime
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .context("NDK Clang did not resolve its OpenMP runtime")?
                    .to_owned(),
            );
        }
        let ndk_lib = ndk
            .join("toolchains/llvm/prebuilt")
            .join(host)
            .join("sysroot/usr/lib")
            .join(&result.target);
        // Public system libraries are resolved by the device, not copied from
        // NDK API stub directories. libc++_shared is a redistributable real DSO.
        dirs.push(ndk_lib);
    }
    Ok(dirs)
}
pub(crate) fn licenses(
    inv: &Inventory,
    result: &BuildArtifact,
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = result.native_inputs.iter()
        .flat_map(|input| input.licenses.iter())
        .map(|asset| (asset.source.clone(), asset.dest.clone()))
        .collect::<Vec<_>>();
    if result.target.contains("android") && result.component != "manager" {
        let ndk = dependencies::ndk_root(inv)?;
        for notice in ["NOTICE", "NOTICE.toolchain"] {
            entries.push((ndk.join(notice), format!("licenses/android-ndk/{notice}")));
        }
    }
    if result.component == "manager" {
        // cargo-about 0.9.3 drops matching LicenseRef clarifications from its report.
        const SLINT_LICENSE: &str = "LicenseRef-Slint-Royalty-free-2.0.md";
        let metadata = build::cargo_metadata(inv, &result.target)?;
        let mut slint = metadata["packages"]
            .as_array()
            .context("Cargo packages")?
            .iter()
            .filter(|package| package["name"] == "slint");
        let manifest = slint
            .next()
            .and_then(|package| package["manifest_path"].as_str())
            .context("Slint manifest missing from Cargo metadata")?;
        ensure!(slint.next().is_none(), "ambiguous Slint package in Cargo metadata");
        let slint_root = Path::new(manifest).parent().context("Slint crate directory")?;
        entries.extend([
            (
                slint_root.join("LICENSES").join(SLINT_LICENSE),
                format!("licenses/slint/{SLINT_LICENSE}"),
            ),
            (
                inv.root
                    .join("Source/Manager/shinki-emu-manager-ui-slint/material-1.1.0/LICENSE.md"),
                "licenses/slint-material/LICENSE.md".into(),
            ),
            (
                inv.root
                    .join("Source/Manager/shinki-emu-manager-ui-slint/ui/icons/LICENSE"),
                "licenses/material-symbols/LICENSE".into(),
            ),
            (
                inv.root.join("Assets/Fonts/OFL-NotoSansJP.txt"),
                "licenses/fonts/OFL-NotoSansJP.txt".into(),
            ),
            (
                inv.root.join("Assets/Fonts/OFL-NotoSansSC.txt"),
                "licenses/fonts/OFL-NotoSansSC.txt".into(),
            ),
            (
                inv.root.join("Assets/Effects/Anime4KOriginal/LICENSE"),
                "licenses/anime4k-original/LICENSE".into(),
            ),
        ]);
    }
    entries.extend(crate::rust_licenses::entries(inv, result, scratch)?);
    Ok(entries)
}
pub(crate) fn metadata_entries(inv: &Inventory, stage: &Path) -> Result<Vec<(PathBuf, String)>> {
    let out = stage.join("snapshot");
    // Use the same metadata command as the online snapshot, with a new private output.
    crate::metadata::run(crate::metadata::MetadataCommand::Build {
        out: out.clone(),
        root: inv.root.join("Metadata"),
    })?;
    let root = if out.join("v1/index.json").is_file() {
        out.join("v1")
    } else {
        out
    };
    let index: serde_json::Value = serde_json::from_slice(&fs::read(root.join("index.json"))?)?;
    if index["schema"] != "shinki.emu.metadata.index.v1" {
        bail!("invalid metadata index");
    }
    let files = index["files"].as_array().context("metadata index files")?;
    if files.len() > 4096 {
        bail!("too many metadata files");
    }
    let mut total = 0u64;
    let mut entries = vec![(root.join("index.json"), "metadata/index.json".into())];
    let mut seen = BTreeSet::new();
    for r in files {
        let name = r["path"].as_str().context("metadata path")?;
        portable(name)?;
        if !seen.insert(name.to_ascii_lowercase()) || name == "index.json" {
            bail!("duplicate metadata entry");
        }
        let path = root.join(name);
        let size = r["size"].as_u64().context("metadata size")?;
        total = total.checked_add(size).context("metadata size overflow")?;
        if size > 8 * 1024 * 1024
            || total > 64 * 1024 * 1024
            || fs::metadata(&path)?.len() != size
            || Some(sha256(&path)?.as_str()) != r["sha256"].as_str()
        {
            bail!("metadata size/hash mismatch");
        }
        entries.push((path, format!("metadata/{name}")));
    }
    Ok(entries)
}
pub fn package(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    inv.require_buildable(component, target)?;
    let resolved = crate::release::resolve_version(inv, component, version)?;
    let version = resolved.as_str();
    if component == "manager" && target.contains("android") {
        return package_android(inv, target, version, out, AndroidPackageMode::Universal);
    }
    let ios_signing = (component == "manager" && crate::apple::ios_frameworks::target_supported(target))
        .then(|| crate::apple::ios_frameworks::Signing::for_target(target))
        .transpose()?;
    let result = build::package_artifact(inv, target, component, version)?;
    reject_overlap(&result.library, out)?;
    if component == "manager" {
        if let Some(target) = shinki_emu_build::ohos::Target::from_rust_target(target) {
            return crate::ohos::package(inv, target, &result, version, out);
        }
        atomic_directory(out, |stage| {
            let temp = tempfile::tempdir_in(stage)?;
            let name = result
                .library
                .file_name()
                .context("Manager filename")?
                .to_string_lossy()
                .into_owned();
            let mut entries = vec![
                (result.library.clone(), name),
                (
                    inv.root.join("THIRD_PARTY_NOTICES.md"),
                    "THIRD_PARTY_NOTICES.md".into(),
                ),
            ];
            if ios_signing.is_none() {
                entries.extend(binary::closure(&result.library, &search_dirs(inv, &result)?, target)?);
            }
            entries.extend(result.native_inputs.iter()
                .flat_map(|input| input.package_files.iter())
                .map(|asset| (asset.source.clone(), asset.dest.clone())));
            entries.extend(licenses(inv, &result, temp.path())?);
            entries.extend(metadata_entries(inv, temp.path())?);
            entries.extend(static_core_entries(inv, &result)?);
            copy_entries(&entries, stage)?;
            drop(temp);
            fs::create_dir_all(stage.join("cores"))?;
            let instructions = if ios_signing.is_some() {
                "ShinkiEMU iOS test app. The signed cores are included in the app bundle."
            } else {
                "Run shinki-emu-manager. Install separately released cores from Project-Shinkirou/ShinkiEMU."
            };
            fs::write(stage.join("README.md"), format!("# ShinkiEMU {version}\n\n{instructions}\n"))?;
            if let Some(signing) = &ios_signing {
                crate::apple::ios_frameworks::stage(inv, &result, stage, signing, version)?;
            }
            if target == "x86_64-unknown-linux-gnu" {
                let icon = crate::icon::Icon::new()?;
                for (size, dir) in [
                    (16, "16x16"),
                    (32, "32x32"),
                    (48, "48x48"),
                    (64, "64x64"),
                    (128, "128x128"),
                    (256, "256x256"),
                ] {
                    icon.png(
                        size,
                        true,
                        &stage.join(format!("share/icons/hicolor/{dir}/apps/shinkiemu.png")),
                    )?;
                }
                fs::create_dir_all(stage.join("share/applications"))?;
                fs::write(
                    stage.join("share/applications/shinkiemu.desktop"),
                    "[Desktop Entry]\nType=Application\nName=ShinkiEMU\nComment=Visual novel emulator manager\nExec=shinki-emu-manager\nIcon=shinkiemu\nTerminal=false\nCategories=Game;Emulator;\n",
                )?;
            }
            if crate::apple::supported(target) {
                let app = crate::apple::bundle(
                    stage,
                    target,
                    "shinki-emu-manager",
                    "ShinkiEMU",
                    "org.shinkirou.shinkiemu",
                    version,
                    ios_signing.as_ref(),
                )?;
                if ios_signing.is_some() {
                    let manifest = crate::apple::ios_frameworks::MANIFEST;
                    fs::copy(app.join(manifest), stage.join(manifest))?;
                }
                if target == "aarch64-apple-ios" {
                    let payload_stage = tempfile::tempdir_in(stage)?;
                    let payload = payload_stage.path().join("Payload");
                    fs::create_dir(&payload)?;
                    fs::rename(
                        &app,
                        payload.join(app.file_name().context("iOS app filename")?),
                    )?;
                    let ipa = stage.join(crate::apple::ios_frameworks::device_filename(target, version));
                    zip_entries(&tree(payload_stage.path())?, &ipa)?;
                }
            }
            Ok(())
        })?;
        Ok(out.to_owned())
    } else {
        let temporary = inv.root.join(".work/tmp");
        fs::create_dir_all(&temporary)?;
        let scratch = tempfile::tempdir_in(&temporary)?;
        stage_built_core(inv, result, version, out, scratch.path())
    }
}

fn stage_built_core(
    inv: &Inventory,
    mut result: BuildArtifact,
    version: &str,
    out: &Path,
    scratch: &Path,
) -> Result<PathBuf> {
    let c = inv.core(&result.component)?;
    let m = result
        .core_build
        .as_mut()
        .context("missing core-build.json")?;
    for (source, path, _) in c.license_files(&inv.root, &result.target)? {
        m.licenses.push(BuildAsset {
            source,
            dest: path.find("licenses/").map_or_else(
                || format!("licenses/{}/{}", c.id, path.replace('/', "_")),
                |offset| path[offset..].to_owned(),
            ),
        });
    }
    // Manifest-owned license assets take precedence over inventory duplicates.
    let mut seen = BTreeSet::new();
    m.licenses.retain(|a| {
        seen.insert(dunce::canonicalize(&a.source).unwrap_or_else(|_| a.source.clone()))
    });
    for path in &c.resources {
        m.resources.push(BuildAsset {
            source: inv.root.join(path),
            dest: format!(
                "Resources/{}",
                Path::new(path).file_name().unwrap().to_string_lossy()
            ),
        });
    }
    let additional_licenses = licenses(inv, &result, scratch)?;
    let m = result.core_build.as_mut().unwrap();
    m.licenses.extend(
        additional_licenses
            .into_iter()
            .map(|(source, dest)| BuildAsset { source, dest }),
    );
    let identity = c.identity(&inv.root)?;
    stage_core(
        &result,
        version,
        identity.as_ref(),
        &c.recognition,
        out,
        &search_dirs(inv, &result)?,
    )
}
/// Resolve license inputs from the actual Cargo checkouts used for embedding.
pub(crate) fn static_core_entries(
    inv: &Inventory,
    result: &BuildArtifact,
) -> Result<Vec<(PathBuf, String)>> {
    let target = &result.target;
    let features = &result.features;
    let mut entries = Vec::new();
    for core in &inv.cores {
        if !core.embedded_in_manager(target, features) {
            continue;
        }
        for (source, path, from_adapter) in core.license_files(&inv.root, &result.target)? {
            let dest = if from_adapter {
                format!("licenses/{}/{path}", core.id)
            } else {
                path.to_owned()
            };
            let asset = BuildAsset { source, dest };
            entries.extend(asset_entries(&asset)?);
        }
        let manifest = result
            .embedded_core_builds
            .iter()
            .find(|build| build.plugin_id == core.id);
        if manifest.is_none()
            && (core.artifact_source() == crate::cores::ArtifactSource::NativeLibrary
                || core.embedded_candidate(target, features))
        {
            bail!("embedded core {} has no recorded build manifest", core.id);
        }
        if let Some(manifest) = manifest {
            manifest.validate()?;
            for asset in &manifest.resources {
                for (source, destination) in asset_entries(asset)? {
                    if result.target.ends_with("-linux-android")
                        && android_host::is_runtime_asset(&destination)
                    {
                        continue;
                    }
                    entries.push((source, format!("cores/{}/{destination}", core.id)));
                }
            }
            for asset in &manifest.licenses {
                for (source, destination) in asset_entries(asset)? {
                    entries.push((
                        source,
                        format!("licenses/embedded/{}/{destination}", core.id),
                    ));
                }
            }
        }
    }
    Ok(entries)
}

const ANDROID_TARGETS: [(&str, &str); 2] = [
    ("aarch64-linux-android", "arm64-v8a"),
    ("x86_64-linux-android", "x86_64"),
];
const ANDROID_BUNDLE_MANIFEST: &str = "assets/bundled.manifest";
const ANDROID_QA_MUSICA_ID: &str = "shinki.emu.musica";
const ANDROID_QA_MUSICA_LIBRARY: &str = "libshinki_emu_musica.so";
const ANDROID_QA_MUSICA_ASSETS: &str = "assets/qa-cores/musica";
const ANDROID_QA_MUSICA_GRADLE: &str = "SHINKI_EMU_ANDROID_QA_BUNDLE_MUSICA";
// The official APK signing certificate (SHA-256).
const ANDROID_PRODUCTION_CERT: &str =
    "c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc";
// The published manager-v0.1.0-beta.8 APK used versionCode 86.
const LAST_PUBLIC_ANDROID_VERSION_CODE: u32 = 86;

fn android_asset_entries(
    inv: &Inventory,
    result: &BuildArtifact,
    shared_scratch: Option<&Path>,
) -> Result<Vec<(PathBuf, String)>> {
    let mut assets = if let Some(scratch) = shared_scratch {
        // This includes native notices for this ABI and one cargo-about
        // report that already covers both Android targets.
        let mut assets = licenses(inv, result, scratch)?;
        assets.extend(metadata_entries(inv, scratch)?);
        assets.push((
            inv.root.join("THIRD_PARTY_NOTICES.md"),
            "THIRD_PARTY_NOTICES.md".into(),
        ));
        let ndk = dependencies::ndk_root(inv)?;
        for notice in ["NOTICE", "NOTICE.toolchain"] {
            assets.push((ndk.join(notice), format!("licenses/android-ndk/{notice}")));
        }
        assets
    } else {
        result
            .native_inputs
            .iter()
            .flat_map(|input| &input.licenses)
            .map(|asset| (asset.source.clone(), asset.dest.clone()))
            .collect()
    };
    assets.extend(static_core_entries(inv, result)?);
    for (_, name) in &mut assets {
        name.insert_str(0, "assets/");
    }
    Ok(assets)
}

fn android_inputs(
    inv: &Inventory,
    result: &BuildArtifact,
    abi: &str,
    shared_scratch: Option<&Path>,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<()> {
    entries.push((
        result.library.clone(),
        format!("jniLibs/{abi}/libshinki_emu_manager.so"),
    ));
    for (source, name) in binary::closure(&result.library, &search_dirs(inv, result)?, &result.target)? {
        entries.push((source, format!("jniLibs/{abi}/{name}")));
    }
    entries.extend(android_asset_entries(inv, result, shared_scratch)?);
    Ok(())
}

struct AndroidQaMusica {
    manifest: CorePackage,
    manifest_sha256: String,
}

fn android_qa_musica_inputs(
    inv: &Inventory,
    manager: &BuildArtifact,
    core: BuildArtifact,
    version: &str,
    abi: &str,
    scratch: &Path,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<AndroidQaMusica> {
    let commit = manager.source_commit.as_deref().context("missing Android Manager source commit")?;
    ensure!(
        manager.product_version.as_deref() == Some(version)
            && core.product_version.as_deref() == Some(version)
            && core.source_commit.as_deref() == Some(commit),
        "Android QA Musica must be built with the Manager's exact product version and source commit"
    );
    let report = core.core_build.as_ref().context("missing Android QA Musica build report")?;
    ensure!(
        core.component == ANDROID_QA_MUSICA_ID
            && report.plugin_id == ANDROID_QA_MUSICA_ID
            && core.target == manager.target
            && report.target == core.target
            && report.artifact_kind == "rust-cdylib"
            && core.library.file_name().is_some_and(|name| name == ANDROID_QA_MUSICA_LIBRARY),
        "Android QA Musica must be the dynamic C ABI library for the Manager's target"
    );
    ensure!(
        report.resources.is_empty() && inv.core(ANDROID_QA_MUSICA_ID)?.resources.is_empty(),
        "Android QA Musica does not support bundled core resources"
    );
    let core_scratch = scratch.join("qa-musica");
    fs::create_dir(&core_scratch)?;
    let dir = stage_built_core(inv, core, version, &core_scratch.join("package"), &core_scratch)?;
    android_qa_musica_entries(&dir, version, commit, abi, entries)
}

fn android_qa_musica_asset(path: &str) -> bool {
    path == PRODUCT_SOURCE_COMMIT || path.starts_with("licenses/")
}

fn android_qa_musica_entries(
    dir: &Path,
    version: &str,
    commit: &str,
    abi: &str,
    entries: &mut Vec<(PathBuf, String)>,
) -> Result<AndroidQaMusica> {
    let manifest = verify_core(dir)?;
    ensure!(
        manifest.plugin_id == ANDROID_QA_MUSICA_ID
            && manifest.library == ANDROID_QA_MUSICA_LIBRARY
            && manifest.version == version
            && manifest.identity.is_none(),
        "unexpected Android QA Musica core declaration"
    );
    ensure!(
        manifest.files.iter().any(|file| file.path == PRODUCT_SOURCE_COMMIT)
            && fs::read(dir.join(PRODUCT_SOURCE_COMMIT))?.as_slice() == commit.as_bytes(),
        "Android QA Musica source stamp must match the Manager's source commit"
    );
    for file in &manifest.files {
        let path = &file.path;
        let source = dir.join(path);
        if android_qa_musica_asset(path) {
            entries.push((source, format!("{ANDROID_QA_MUSICA_ASSETS}/{path}")));
            continue;
        }
        ensure!(
            path.starts_with("lib") && path.ends_with(".so") && !path.contains('/'),
            "unsupported Android QA Musica package entry: {path}"
        );
        verify_android_qa_library(&source, abi)?;
        let name = format!("jniLibs/{abi}/{path}");
        if let Some((existing, _)) = entries.iter().find(|(_, candidate)| candidate == &name) {
            ensure!(
                same_android_asset(existing, &source, fs::metadata(existing)?.len())?,
                "conflicting Android QA Musica native library {name}: {} and {}",
                existing.display(),
                source.display(),
            );
        } else {
            entries.push((source, name));
        }
    }
    let declaration = dir.join("core.json");
    let manifest_sha256 = sha256(&declaration)?;
    entries.push((declaration, format!("{ANDROID_QA_MUSICA_ASSETS}/core.json")));
    Ok(AndroidQaMusica { manifest, manifest_sha256 })
}

fn verify_android_qa_library(path: &Path, abi: &str) -> Result<()> {
    use object::elf::{ELFCLASS64, ELFDATA2LSB, EM_AARCH64, EM_X86_64, ET_DYN};

    let machine = match abi {
        "arm64-v8a" => EM_AARCH64,
        "x86_64" => EM_X86_64,
        _ => bail!("unsupported Android QA ABI: {abi}"),
    };
    // stage_core's closure parsing has already read the ELF. Only the file
    // header is needed here to check the architecture and shared-object type.
    let mut header = [0; 20];
    fs::File::open(path)?.read_exact(&mut header)?;
    ensure!(
        header.starts_with(&[0x7f, b'E', b'L', b'F', ELFCLASS64.0, ELFDATA2LSB.0])
            && u16::from_le_bytes([header[16], header[17]]) == ET_DYN.0
            && u16::from_le_bytes([header[18], header[19]]) == machine.0,
        "{} is not an Android {abi} ELF64 shared library",
        path.display(),
    );
    Ok(())
}

fn verify_android_qa_musica_archive(
    archive: &mut zip::ZipArchive<impl Read + io::Seek>,
    abi: &str,
    musica: &AndroidQaMusica,
    bundle_manifest_sha256: &str,
) -> Result<()> {
    let native = format!("lib/{abi}/");
    for name in archive.file_names() {
        let name = name?;
        ensure!(
            !name.starts_with("lib/") || name == "lib/" || name.starts_with(&native),
            "Android QA APK contains a native entry outside {native}: {name}"
        );
    }
    let mut verify = |name: &str, expected: &str| -> Result<()> {
        let entry = archive.by_name(name)
            .with_context(|| format!("Android QA APK is missing {name}"))?;
        ensure!(
            !entry.is_dir() && !entry.is_symlink() && sha256_reader(entry)? == expected,
            "Android QA APK content differs from verified staging: {name}"
        );
        Ok(())
    };
    verify(ANDROID_BUNDLE_MANIFEST, bundle_manifest_sha256)?;
    verify(&format!("{ANDROID_QA_MUSICA_ASSETS}/core.json"), &musica.manifest_sha256)?;
    for file in &musica.manifest.files {
        let name = if android_qa_musica_asset(&file.path) {
            format!("{ANDROID_QA_MUSICA_ASSETS}/{}", file.path)
        } else {
            format!("{native}{}", file.path)
        };
        verify(&name, &file.sha256)?;
    }
    Ok(())
}

fn same_android_asset(first: &Path, second: &Path, mut len: u64) -> Result<bool> {
    plain(first)?;
    plain(second)?;
    let mut first = fs::File::open(first)?;
    let mut second = fs::File::open(second)?;
    let metadata = second.metadata()?;
    if !metadata.is_file() || metadata.len() != len {
        return Ok(false);
    }
    let mut a = [0; 8192];
    let mut b = [0; 8192];
    while len != 0 {
        let n = len.min(a.len() as u64) as usize;
        first.read_exact(&mut a[..n])?;
        second.read_exact(&mut b[..n])?;
        if a[..n] != b[..n] {
            return Ok(false);
        }
        len -= n as u64;
    }
    Ok(true)
}

fn stage_android_inputs(mut entries: Vec<(PathBuf, String)>, stage: &Path) -> Result<()> {
    // Sort the union so repeated asset destinations are adjacent and the
    // runtime extraction list is deterministic.
    entries.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    let mut manifest = String::new();
    for group in entries.chunk_by(|a, b| a.1 == b.1) {
        let (source, name) = &group[0];
        let Some(asset) = name.strip_prefix("assets/") else {
            continue;
        };
        ensure!(
            !name.eq_ignore_ascii_case(ANDROID_BUNDLE_MANIFEST),
            "reserved Android asset destination: {name}"
        );
        let len = fs::metadata(source)?.len();
        ensure!(
            len <= 32 * 1024 * 1024,
            "Android asset exceeds the runtime size limit: {name}"
        );
        for (other, _) in &group[1..] {
            ensure!(
                source == other || same_android_asset(source, other, len)?,
                "conflicting Android asset {name}: {} and {}",
                source.display(),
                other.display(),
            );
        }
        manifest.push_str(asset);
        manifest.push('\n');
    }
    if manifest.len() > 1024 * 1024 {
        bail!("Android bundled manifest exceeds the runtime size limit");
    }
    entries.dedup_by(|a, b| a.1 == b.1 && a.1.starts_with("assets/"));
    copy_entries(&entries, stage)?;
    fs::write(stage.join(ANDROID_BUNDLE_MANIFEST), manifest)?;
    Ok(())
}

fn android_keytool() -> Result<std::process::Command> {
    let java = PathBuf::from(std::env::var_os("JAVA_HOME").context("set JAVA_HOME to JDK 27")?);
    Ok(std::process::Command::new(java.join("bin").join(if cfg!(windows) {
        "keytool.exe"
    } else {
        "keytool"
    })))
}

fn prepare_android_debug_key() -> Result<PathBuf> {
    let home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".gradle"))
        })
        .context("set GRADLE_USER_HOME for the Android test signing key")?;
    fs::create_dir_all(&home)?;
    let key = home.join("shinkiemu-debug.keystore");
    if key.exists() {
        plain(&key)?;
        return Ok(key);
    }
    let temp = tempfile::tempdir_in(&home)?;
    let generated = temp.path().join("debug.keystore");
    dependencies::run(
        android_keytool()?
            .args(["-genkeypair", "-noprompt", "-storetype", "JKS", "-keystore"])
            .arg(&generated)
            .args([
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
            ]),
    )?;
    // Publish the complete test key without overwriting a concurrent creator's key.
    if let Err(error) = fs::hard_link(&generated, &key)
        && error.kind() != io::ErrorKind::AlreadyExists
    {
        return Err(error.into());
    }
    plain(&key)?;
    Ok(key)
}

fn android_debug_certificate(key: &Path) -> Result<String> {
    let der = dependencies::capture(
        android_keytool()?
            .args(["-exportcert", "-keystore"])
            .arg(key)
            .args(["-storepass", "android", "-alias", "androiddebugkey"]),
    )?;
    let digest = hex::encode(Sha256::digest(der));
    if digest == ANDROID_PRODUCTION_CERT {
        bail!("the Android debug keystore contains the production certificate");
    }
    Ok(digest)
}

fn verify_android_certificate(signing: &str, expected: &str) -> Result<()> {
    let mut count = signing
        .lines()
        .filter_map(|line| line.strip_prefix("Number of signers: "));
    ensure!(
        count.next() == Some("1") && count.next().is_none(),
        "Android APK must report exactly one signer"
    );
    let mut seen = 0;
    for (label, digest) in signing
        .lines()
        .filter_map(|line| line.split_once(" certificate SHA-256 digest: "))
    {
        // SDK 37 reports one certificate per signature scheme. A single
        // signer can appear under several schemes, but all must match.
        let scheme = match label {
            "Signer #1" => 1 << 0,
            "V1 Signer:" => 1 << 1,
            "V2 Signer:" => 1 << 2,
            "V3 Signer:" => 1 << 3,
            "V3.1 Signer:" => 1 << 4,
            "V3.2 Signer:" => 1 << 5,
            "V4 Signer:" => 1 << 6,
            _ => bail!("unexpected Android APK certificate label: {label}"),
        };
        ensure!(
            seen & scheme == 0,
            "repeated Android APK certificate label: {label}"
        );
        ensure!(
            digest.trim() == expected,
            "Android APK signer does not match the configured certificate"
        );
        seen |= scheme;
    }
    ensure!(seen != 0, "Android APK signer certificate is missing");
    Ok(())
}

fn signed_android_version_code(value: &str) -> Result<u32> {
    let code: u32 = value.parse().context("invalid SHINKI_EMU_ANDROID_VERSION_CODE")?;
    ensure!(
        (LAST_PUBLIC_ANDROID_VERSION_CODE + 1..=i32::MAX as u32).contains(&code),
        "signed Android versionCode must exceed {LAST_PUBLIC_ANDROID_VERSION_CODE} and fit a positive Android int",
    );
    Ok(code)
}

pub(crate) fn package_android_qa(
    inv: &Inventory,
    target: &str,
    version: &str,
    out: &Path,
    bundle_musica: bool,
) -> Result<PathBuf> {
    let version = crate::release::resolve_version(inv, "manager", version)?;
    package_android(
        inv,
        target,
        &version,
        out,
        AndroidPackageMode::Qa { bundle_musica },
    )
}

enum AndroidPackageMode {
    Universal,
    Qa { bundle_musica: bool },
}

fn package_android(
    inv: &Inventory,
    target: &str,
    version: &str,
    out: &Path,
    mode: AndroidPackageMode,
) -> Result<PathBuf> {
    use base64::Engine as _;
    let (qa, bundle_musica) = match mode {
        AndroidPackageMode::Universal => (false, false),
        AndroidPackageMode::Qa { bundle_musica } => (true, bundle_musica),
    };
    let targets = if qa {
        std::slice::from_ref(
            ANDROID_TARGETS
                .iter()
                .find(|(triple, _)| *triple == target)
                .context("--qa-single-abi requires an Android Manager target")?,
        )
    } else {
        ANDROID_TARGETS.as_slice()
    };
    let abi = targets[0].1;

    let key = std::env::var_os("SHINKI_EMU_ANDROID_KEYSTORE").filter(|s| !s.is_empty());
    let encoded = std::env::var_os("SHINKI_EMU_ANDROID_KEYSTORE_BASE64").filter(|s| !s.is_empty());
    let passwords = [
        "SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD",
        "SHINKI_EMU_ANDROID_KEY_ALIAS",
        "SHINKI_EMU_ANDROID_KEY_PASSWORD",
    ];
    let present = passwords
        .iter()
        .filter(|key| std::env::var_os(key).is_some_and(|s| !s.is_empty()))
        .count();
    let signed = key.is_some() || encoded.is_some();
    if qa && (signed || present != 0) {
        bail!("--qa-single-abi requires the debug key; unset all Android release signing inputs");
    }
    if key.is_some() && encoded.is_some() {
        bail!("provide one Android keystore input, not both path and base64");
    }
    if (signed && present != 3) || (!signed && present != 0) {
        bail!(
            "Android release signing requires the keystore and all three password/alias variables; partial signing configuration is an error"
        );
    }
    let version_code = if signed {
        signed_android_version_code(
            &std::env::var("SHINKI_EMU_ANDROID_VERSION_CODE")
                .context("set SHINKI_EMU_ANDROID_VERSION_CODE for a signed release APK")?,
        )?
    } else {
        std::env::var("SHINKI_EMU_ANDROID_VERSION_CODE").as_deref().unwrap_or("1").parse()?
    };
    let private_key = if let Some(encoded) = encoded {
        let key = crate::temporary::file()?;
        fs::write(
            key.path(),
            base64::engine::general_purpose::STANDARD.decode(
                encoded
                    .to_str()
                    .context("Android keystore base64 is not UTF-8")?
                    .trim(),
            )?,
        )?;
        Some(key)
    } else {
        None
    };
    let key = key
        .map(PathBuf::from)
        .or_else(|| private_key.as_ref().map(|key| key.path().to_owned()));
    if let Some(key) = &key {
        plain(key)?;
    }
    let result = build::package_artifact(inv, target, "manager", version)?;
    reject_overlap(&result.library, out)?;
    atomic_directory(out, |stage| {
        let scratch = crate::temporary::directory()?;
        let mut entries = Vec::new();
        for (target, abi) in targets {
            if *target == result.target {
                android_inputs(inv, &result, abi, Some(scratch.path()), &mut entries)?;
            } else {
                let built = build::package_artifact(inv, target, "manager", version)?;
                reject_overlap(&built.library, out)?;
                android_inputs(inv, &built, abi, None, &mut entries)?;
            }
        }
        // The Java host sources are shared by both Android ABIs.
        entries.extend(android_host::inputs(inv, targets, scratch.path())?);
        let musica = if bundle_musica {
            let core = build::package_artifact(inv, target, ANDROID_QA_MUSICA_ID, version)?;
            reject_overlap(&core.library, out)?;
            Some(android_qa_musica_inputs(
                inv,
                &result,
                core,
                version,
                abi,
                scratch.path(),
                &mut entries,
            )?)
        } else {
            None
        };
        stage_android_inputs(entries, stage)?;
        drop(scratch);
        crate::icon::Icon::new()?.android(stage)?;
        let debug_key = if signed { None } else { Some(prepare_android_debug_key()?) };
        let certificate = match debug_key {
            Some(key) => android_debug_certificate(&key)?,
            None => ANDROID_PRODUCTION_CERT.into(),
        };
        let universal_filter;
        let abi_filter = if qa {
            abi
        } else {
            universal_filter = format!("{},{}", abi, targets[1].1);
            &universal_filter
        };
        let gradle = inv.root.join("Platforms/Android").join(if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        });
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            cmd.arg("/c");
        }
        cmd.arg(gradle)
            .args([
                "--no-daemon",
                "--console=plain",
                if signed {
                    ":app:assembleRelease"
                } else {
                    ":app:assembleDebug"
                },
            ])
            .current_dir(inv.root.join("Platforms/Android"))
            .env("SHINKI_EMU_ANDROID_VERSION_NAME", version)
            .env("SHINKI_EMU_ANDROID_VERSION_CODE", version_code.to_string())
            .env(
                "SHINKI_EMU_ANDROID_GENERATED_ROOT",
                dunce::canonicalize(stage)?,
            )
            .env("SHINKI_EMU_ANDROID_ABI", abi_filter);
        if bundle_musica {
            cmd.env(ANDROID_QA_MUSICA_GRADLE, "1");
        } else {
            cmd.env_remove(ANDROID_QA_MUSICA_GRADLE);
        }
        if std::env::var_os("SHINKI_EMU_GRADLE_OFFLINE").is_some_and(|value| value == "1") {
            cmd.arg("--offline");
        }
        if let Some(key) = &key {
            cmd.env("SHINKI_EMU_ANDROID_KEYSTORE", key);
        }
        cmd.env_remove("SHINKI_EMU_ANDROID_KEYSTORE_BASE64");
        dependencies::run(&mut cmd)?;
        let outputs = inv
            .root
            .join("Platforms/Android/app/build/outputs/apk")
            .join(if signed { "release" } else { "debug" });
        let mut apks = tree(&outputs)?
            .into_iter()
            .filter(|(_, name)| name.ends_with(".apk"));
        let (source, _) = apks.next().context("Gradle did not produce a signed APK")?;
        if apks.next().is_some() {
            bail!("Gradle must produce exactly one signed APK");
        }
        let variant = match (bundle_musica, qa, signed) {
            (true, _, _) => "qa-musica-debug",
            (_, true, _) => "qa-debug",
            (_, _, true) => "release",
            _ => "debug",
        };
        let platform = if qa { abi } else { "universal" };
        let apk_name = format!("ShinkiEMU-{version}-android-{platform}-{variant}-signed.apk");
        let apk = stage.join(&apk_name);
        fs::copy(source, &apk)?;
        verify_android_apk(&apk, &certificate, version, version_code)?;
        if let Some(musica) = musica {
            let mut archive = zip::ZipArchive::new(fs::File::open(&apk)?)?;
            verify_android_qa_musica_archive(
                &mut archive,
                abi,
                &musica,
                &sha256(&stage.join(ANDROID_BUNDLE_MANIFEST))?,
            )?;
            fs::write(
                stage.join("android-qa-musica.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "signing": "debug-test",
                    "apk": apk_name,
                    "sha256": sha256(&apk)?,
                    "certificate_sha256": certificate,
                    "target": target,
                    "abi": abi,
                    "version": version,
                    "source_commit": result.source_commit,
                    "core_manifest_sha256": musica.manifest_sha256,
                }))?,
            )?;
        } else if !qa {
            fs::write(
                stage.join("android.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "signing": if signed { "release" } else { "debug-test" },
                    "apk": apk_name,
                }))?,
            )?;
        }
        Ok(())
    })?;
    Ok(out.to_owned())
}

fn verify_android_apk(
    apk: &Path,
    certificate: &str,
    version: &str,
    version_code: u32,
) -> Result<()> {
    let sdk = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .context("set ANDROID_HOME")?;
    let tools = sdk.join("build-tools/37.0.0");
    let signer = tools.join(if cfg!(windows) {
        "apksigner.bat"
    } else {
        "apksigner"
    });
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.arg("/c");
        command
    } else {
        std::process::Command::new("sh")
    };
    command
        .arg(signer)
        .args(["verify", "--verbose", "--print-certs"])
        .arg(apk);
    let signing = dependencies::capture(&mut command)?;
    let signing = std::str::from_utf8(&signing)?;
    verify_android_certificate(signing, certificate)?;
    let aapt2 = tools.join(if cfg!(windows) { "aapt2.exe" } else { "aapt2" });
    let badging = dependencies::capture(
        std::process::Command::new(aapt2).args(["dump", "badging"]).arg(apk),
    )?;
    verify_android_badging(std::str::from_utf8(&badging)?, version, version_code)?;
    // The bundled manifest is the runtime extraction list.
    let mut archive = zip::ZipArchive::new(fs::File::open(apk)?)?;
    archive.by_name("assets/metadata/index.json")?;
    archive.by_name(ANDROID_BUNDLE_MANIFEST)?;
    Ok(())
}

fn verify_android_badging(badging: &str, version: &str, version_code: u32) -> Result<()> {
    let package = badging.lines().find(|line| line.starts_with("package: "))
        .context("Android APK package metadata missing")?;
    let version_field = format!("versionName='{version}'");
    let code_field = format!("versionCode='{version_code}'");
    if !package.split_ascii_whitespace().any(|field| field == "name='org.shinkirou.shinkiemu'")
        || !package.split_ascii_whitespace().any(|field| field == version_field)
        || !package.split_ascii_whitespace().any(|field| field == code_field)
    {
        bail!("Android APK must use org.shinkirou.shinkiemu, versionName {version} and versionCode {version_code}");
    }
    let mut minimums = badging.lines().filter_map(|line| {
        line.strip_prefix("minSdkVersion:")
            .or_else(|| line.strip_prefix("sdkVersion:"))
    });
    if minimums.next() != Some("'28'") || minimums.any(|minimum| minimum != "'28'") {
        bail!("Android APK must require API 28");
    }
    Ok(())
}

#[cfg(test)]
mod android_tests {
    use super::*;
    use clap::Parser as _;
    use shinki_emu_build::{CoreBuildManifest, NativeInputs};

    #[test]
    fn android_badging_accepts_both_build_tools_minimum_sdk_fields() {
        let package =
            "package: name='org.shinkirou.shinkiemu' versionCode='1' versionName='0.1.0-qa.1'";
        for sdk in ["sdkVersion:'28'", "minSdkVersion:'28'"] {
            verify_android_badging(&format!("{package}\n{sdk}\n"), "0.1.0-qa.1", 1).unwrap();
        }
        for sdk in [
            "",
            "targetSdkVersion:'28'",
            "minSdkVersion:'27'",
            "minSdkVersion:'29'",
            "sdkVersion:'28'\nminSdkVersion:'27'",
        ] {
            assert!(
                verify_android_badging(&format!("{package}\n{sdk}\n"), "0.1.0-qa.1", 1)
                    .unwrap_err()
                    .to_string()
                    .contains("require API 28"),
                "{sdk}"
            );
        }
    }

    #[test]
    fn adv32_candidate_package_stages_its_reported_resources_and_licenses() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(repo).unwrap();
        inv.cores.retain(|core| core.id == "shinki.emu.adv32");
        let scratch = crate::temporary::directory().unwrap();
        let resource = scratch.path().join("resource.dat");
        fs::write(&resource, b"candidate resource").unwrap();
        let target = "x86_64-pc-windows-msvc";
        let licenses = [
            ("docs/licenses/shinki-emu-adv32/GARbro.txt", "GARbro.txt"),
            ("Assets/Fonts/OFL-NotoSansJP.txt", "OFL-NotoSansJP.txt"),
            ("Assets/Fonts/OFL-NotoSansSC.txt", "OFL-NotoSansSC.txt"),
        ];
        let mut artifact = BuildArtifact {
            component: "manager".into(),
            target: target.into(),
            product_version: None,
            library: scratch.path().join("shinki-emu-manager.exe"),
            core_build: None,
            embedded_core_builds: vec![CoreBuildManifest {
                plugin_id: "shinki.emu.adv32".into(),
                target: target.into(),
                artifact_kind: "rust-cdylib".into(),
                native_library: None,
                resources: vec![BuildAsset { source: resource, dest: "Resources/resource.dat".into() }],
                licenses: licenses
                    .map(|(source, name)| BuildAsset {
                        source: repo.join(source),
                        dest: format!("licenses/shinki.emu.adv32/{name}"),
                    })
                    .into(),
            }],
            native_inputs: vec![],
            source_commit: None,
            features: vec![],
        };
        // A stale report cannot add ADV32 files to a default Manager package.
        assert!(static_core_entries(&inv, &artifact).unwrap().is_empty());
        artifact.features.push(crate::cores::ADV32_CANDIDATE_FEATURE.to_owned());
        let stage = scratch.path().join("candidate");
        fs::create_dir(&stage).unwrap();
        copy_entries(&static_core_entries(&inv, &artifact).unwrap(), &stage).unwrap();
        assert_eq!(
            fs::read(stage.join("cores/shinki.emu.adv32/Resources/resource.dat")).unwrap(),
            b"candidate resource",
        );
        let embedded_dir = stage.join("licenses/embedded/shinki.emu.adv32/licenses/shinki.emu.adv32");
        for (source, name) in licenses {
            assert_eq!(fs::read(embedded_dir.join(name)).unwrap(), fs::read(repo.join(source)).unwrap());
        }
        artifact.embedded_core_builds.clear();
        assert!(static_core_entries(&inv, &artifact).is_err());
    }

    #[test]
    fn android_stages_both_abi_asset_sets_and_lists_equal_resources_once() {
        let root = crate::temporary::directory().unwrap();
        let inv = Inventory {
            root: root.path().into(),
            cores: vec![serde_json::from_value(serde_json::json!({
                "id": "renpy",
                "family_id": "renpy",
                "display_name": "Ren'Py",
                "family_name": "Ren'Py",
                "license": "MIT",
                "resources": [],
                "supported_targets": [],
                "static_manager_targets": ANDROID_TARGETS.map(|(target, _)| target),
                "blocked": [],
                "publish_blocked": "",
            })).unwrap()],
            targets: vec![],
        };
        let common = "cores/renpy/Resources/lib/python3.12/os.py";
        let mut expected = vec![(common.to_owned(), "common stdlib")];
        let mut entries = Vec::new();
        for (target, abi) in ANDROID_TARGETS {
            let inputs = root.path().join(abi);
            fs::create_dir(&inputs).unwrap();
            let write = |name: &str, text: &str| {
                let source = inputs.join(name);
                fs::write(&source, text).unwrap();
                source
            };
            let sysconfig = format!("Resources/lib/{target}/python3.12/_sysconfigdata__linux_.py");
            let notice = format!("licenses/native/{abi}/NOTICE");
            let core_notice = format!("licenses/renpy/{abi}/NOTICE");
            let artifact = BuildArtifact {
                component: "manager".into(),
                target: target.into(),
                product_version: None,
                library: inputs.join("libshinki_emu_manager.so"),
                core_build: None,
                embedded_core_builds: vec![CoreBuildManifest {
                    plugin_id: "renpy".into(),
                    target: target.into(),
                    artifact_kind: "native-static".into(),
                    native_library: Some(write("librenpy.a", abi)),
                    resources: vec![
                        BuildAsset { source: write("sysconfig.py", abi), dest: sysconfig.clone() },
                        BuildAsset {
                            source: write("os.py", "common stdlib"),
                            dest: "Resources/lib/python3.12/os.py".into(),
                        },
                    ],
                    licenses: vec![BuildAsset {
                        source: write("core-notice", abi),
                        dest: core_notice.clone(),
                    }],
                }],
                native_inputs: vec![NativeInputs {
                    licenses: vec![BuildAsset { source: write("native-notice", abi), dest: notice.clone() }],
                    ..Default::default()
                }],
                source_commit: None,
                features: vec![],
            };
            entries.extend(android_asset_entries(&inv, &artifact, None).unwrap());
            expected.extend([
                (format!("cores/renpy/{sysconfig}"), abi),
                (format!("licenses/embedded/renpy/{core_notice}"), abi),
                (notice, abi),
            ]);
        }

        let stage = root.path().join("stage");
        stage_android_inputs(entries, &stage).unwrap();
        expected.sort_unstable();
        let manifest = fs::read_to_string(stage.join(ANDROID_BUNDLE_MANIFEST)).unwrap();
        assert!(manifest.lines().eq(expected.iter().map(|(name, _)| name.as_str())));
        for (name, bytes) in expected {
            assert_eq!(fs::read_to_string(stage.join("assets").join(name)).unwrap(), bytes);
        }
    }

    #[test]
    fn android_rejects_conflicting_bytes_at_the_same_asset_destination() {
        let root = crate::temporary::directory().unwrap();
        let arm = root.path().join("arm64");
        let x86 = root.path().join("x86_64");
        let mut data = vec![b'a'; 8193];
        fs::write(&arm, &data).unwrap();
        data[8192] = b'b';
        fs::write(&x86, &data).unwrap();
        let name = "assets/cores/renpy/Resources/lib/python3.12/os.py";
        let stage = root.path().join("stage");
        let error = stage_android_inputs(vec![(arm, name.into()), (x86, name.into())], &stage)
            .unwrap_err();
        assert!(error.to_string().contains(name));
        assert!(!stage.exists());
    }

    #[test]
    fn signed_android_version_code_must_upgrade_and_fit_an_android_int() {
        for code in ["-1", "0", "85", "86", "2147483648"] {
            assert!(signed_android_version_code(code).is_err(), "{code}");
        }
        assert_eq!(signed_android_version_code("87").unwrap(), 87);
        assert_eq!(signed_android_version_code("2147483647").unwrap(), 2147483647);
    }

    #[test]
    fn single_abi_qa_is_available_only_for_manager_packaging() {
        use crate::cli::Cli;
        let package = ["xtask", "package", "--target", "x86_64-linux-android", "--out", "qa"];
        assert!(Cli::try_parse_from(package.into_iter().chain(["--manager", "--qa-single-abi"])).is_ok());
        assert!(Cli::try_parse_from(package.into_iter().chain(["--core", "musica", "--qa-single-abi"])).is_err());
    }

    #[test]
    fn android_qa_musica_requires_single_abi_manager() {
        use crate::cli::Cli;
        let command = ["xtask", "package", "--target", "aarch64-linux-android", "--out", "qa"];
        assert!(Cli::try_parse_from(command.into_iter().chain([
            "--manager",
            "--qa-single-abi",
            "--qa-bundle-musica",
        ])).is_ok());
        assert!(Cli::try_parse_from(command.into_iter().chain([
            "--manager",
            "--qa-bundle-musica",
        ])).is_err());
        assert!(Cli::try_parse_from(command.into_iter().chain([
            "--core",
            ANDROID_QA_MUSICA_ID,
            "--qa-single-abi",
            "--qa-bundle-musica",
        ])).is_err());
    }

    #[test]
    fn android_qa_musica_refuses_release_signing_before_build() {
        const CHILD: &str = "SHINKI_EMU_XTASK_TEST_QA_SIGNING";
        if std::env::var_os(CHILD).is_some() {
            let root = crate::temporary::directory().unwrap();
            let out = root.path().join("out");
            let inv = Inventory { root: root.path().to_owned(), cores: vec![], targets: vec![] };
            let error = package_android_qa(
                &inv, "aarch64-linux-android", "0.1.0-qa.musica.1", &out, true,
            ).unwrap_err();
            assert!(error.to_string().contains("requires the debug key"), "{error:#}");
            assert!(!out.exists());
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "package::android_tests::android_qa_musica_refuses_release_signing_before_build"])
            .env(CHILD, "1")
            .env("SHINKI_EMU_ANDROID_KEYSTORE_BASE64", "release signing input")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    }

    #[test]
    fn android_qa_musica_keeps_native_and_extracted_assets_in_sync() {
        let root = crate::temporary::directory().unwrap();
        let inputs = root.path().join("inputs");
        fs::create_dir(&inputs).unwrap();
        let library = inputs.join(ANDROID_QA_MUSICA_LIBRARY);
        let dependency = "libqa-runtime.so";
        let runtime = inputs.join(dependency);
        let mut elf = [0; 64];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&object::elf::ET_DYN.0.to_le_bytes());
        elf[18..20].copy_from_slice(&object::elf::EM_AARCH64.0.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[52..54].copy_from_slice(&64u16.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[58..60].copy_from_slice(&64u16.to_le_bytes());
        fs::write(&library, elf).unwrap();
        fs::write(&runtime, elf).unwrap();
        let notice = inputs.join("NOTICE");
        fs::write(&notice, b"QA runtime license").unwrap();
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let version = "0.1.0-qa.musica.1";
        let target = "aarch64-linux-android";
        let report = CoreBuildManifest {
            plugin_id: ANDROID_QA_MUSICA_ID.into(),
            target: target.into(),
            // The generic core packager discovers sibling native modules here.
            artifact_kind: "native-shared".into(),
            native_library: Some(library.clone()),
            resources: vec![],
            licenses: vec![BuildAsset { source: notice, dest: "licenses/NOTICE".into() }],
        };
        let artifact = BuildArtifact {
            component: ANDROID_QA_MUSICA_ID.into(),
            target: target.into(),
            product_version: Some(version.into()),
            library,
            core_build: Some(report),
            embedded_core_builds: vec![],
            native_inputs: vec![],
            source_commit: Some(commit.into()),
            features: vec![],
        };
        let core = stage_core(
            &artifact,
            version,
            None,
            &Default::default(),
            &root.path().join("core"),
            &[],
        ).unwrap();
        let abi = "arm64-v8a";
        let shared = format!("jniLibs/{abi}/{dependency}");
        let mut entries = vec![(runtime.clone(), shared.clone())];
        let musica = android_qa_musica_entries(&core, version, commit, abi, &mut entries).unwrap();
        let stage = root.path().join("stage");
        stage_android_inputs(entries, &stage).unwrap();
        assert_eq!(fs::read(stage.join(shared)).unwrap().as_slice(), elf.as_slice());
        let bundle_list = fs::read_to_string(stage.join(ANDROID_BUNDLE_MANIFEST)).unwrap();
        assert_eq!(
            bundle_list.lines().collect::<Vec<_>>(),
            ["qa-cores/musica/core.json", "qa-cores/musica/licenses/NOTICE", "qa-cores/musica/source-commit.txt"]
        );
        let mut payload = tree(&stage).unwrap();
        for (_, name) in &mut payload {
            if let Some(relative) = name.strip_prefix("jniLibs/") {
                *name = format!("lib/{relative}");
            }
        }
        let manifest_hash = sha256(&stage.join(ANDROID_BUNDLE_MANIFEST)).unwrap();
        let check = |filename: &str, entries: &[(PathBuf, String)]| {
            let archive = root.path().join(filename);
            zip_entries(entries, &archive).unwrap();
            verify_android_qa_musica_archive(
                &mut zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap(),
                abi,
                &musica,
                &manifest_hash,
            )
        };
        check("complete.apk", &payload).unwrap();
        elf[63] = 1;
        fs::write(&runtime, elf).unwrap();
        let native = payload.iter().position(|(_, name)| name.ends_with(dependency)).unwrap();
        let original = std::mem::replace(&mut payload[native].0, runtime.clone());
        assert!(check("changed-native.apk", &payload).is_err());
        payload[native].0 = original;
        payload.retain(|(_, name)| !name.ends_with(PRODUCT_SOURCE_COMMIT));
        assert!(check("missing-stamp.apk", &payload).is_err());
        assert!(android_qa_musica_entries(&core, version, commit, "x86_64", &mut vec![]).is_err());
        assert!(android_qa_musica_entries(&core, "0.1.0-qa.musica.2", commit, abi, &mut vec![]).is_err());
        assert!(android_qa_musica_entries(&core, version, &"f".repeat(40), abi, &mut vec![]).is_err());

        let mut conflict = vec![(runtime, format!("jniLibs/{abi}/{dependency}"))];
        assert!(android_qa_musica_entries(&core, version, commit, abi, &mut conflict).is_err());
    }

    #[test]
    fn apk_certificate_requires_the_configured_signer_only() {
        let debug_cert = "a".repeat(64);
        let legacy = format!("Signer #1 certificate SHA-256 digest: {debug_cert}");
        let modern = format!("V2 Signer: certificate SHA-256 digest: {debug_cert}");
        let v3 = format!("V3 Signer: certificate SHA-256 digest: {debug_cert}");
        let production =
            format!("V3 Signer: certificate SHA-256 digest: {ANDROID_PRODUCTION_CERT}");
        let count = "Number of signers: 1";
        for signing in [
            format!("{count}\n{legacy}"),
            format!("{count}\n{modern}"),
            format!("{count}\n{modern}\n{v3}"),
        ] {
            assert!(verify_android_certificate(&signing, &debug_cert).is_ok());
        }
        for signing in [
            String::new(),
            modern.clone(),
            format!("{count}\n{production}"),
            format!("{count}\n{modern}\n{production}"),
            format!("{count}\n{modern}\n{modern}"),
            format!("{count}\n{count}\n{modern}"),
            format!("Number of signers: 2\n{modern}"),
            format!("{count}\n{legacy}\nSigner #2 certificate SHA-256 digest: {debug_cert}"),
        ] {
            assert!(verify_android_certificate(&signing, &debug_cert).is_err(), "{signing}");
        }
        assert!(
            verify_android_certificate(&format!("{count}\n{production}"), ANDROID_PRODUCTION_CERT)
                .is_ok()
        );
    }
}
