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
    let mut f = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
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
                .compression_method(if name.starts_with("downloads/") {
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
    pub source_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<shinki_emu_metadata::CoreIdentity>,
    #[serde(default, skip_serializing_if = "shinki_emu_metadata::CoreRecognition::is_empty")]
    pub recognition: shinki_emu_metadata::CoreRecognition,
    pub abi: String,
    pub files: Vec<FileRecord>,
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
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest = CorePackage {
            schema: "shinki.emu.core_package.v1".into(),
            plugin_id: m.plugin_id.clone(),
            library: library.into(),
            version: version.into(),
            source_commit: result.source_commit.clone(),
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
    let selected = dependencies::needs(inv, &result.target, &result.component)?;
    let mut dirs = vec![
        result
            .library
            .parent()
            .context("library parent")?
            .to_owned(),
    ];
    if selected.ffmpeg
        && let Some(runtime) = dependencies::ffmpeg_runtime(inv, &result.target)?
    {
        dirs.push(runtime);
    }
    let separate_subtitle =
        selected.subtitle && !dependencies::shares_android_subtitle(&result.target, selected);
    if separate_subtitle {
        dirs.push(if result.target == "x86_64-unknown-linux-gnu" {
            dependencies::linux_runtime(inv, &result.target, "subtitle")
        } else if crate::apple::supported(&result.target) {
            dependencies::subtitle_install(inv, &result.target, &result.component)?.join("lib")
        } else {
            dependencies::subtitle_install(inv, &result.target, &result.component)?.join("bin")
        });
    }
    if selected.dxc {
        dirs.push(inv.root.join(".work/dxc"));
    }
    if let Some(native) = &result.core_build
        && let Some(p) = &native.native_library
    {
        dirs.push(p.parent().context("native library parent")?.to_owned());
        dirs.push(p.parent().context("native library parent")?.join("lib"));
    }
    for native in dependencies::native_cores(inv, &result.target, &result.component, selected) {
        dirs.push(if result.target == "x86_64-unknown-linux-gnu" {
            dependencies::linux_runtime(inv, &result.target, &native.id)
        } else {
            dependencies::native_install(inv, &result.target, &native.id)?
                .join(dependencies::native_triplet(inv, &result.target)?)
                .join(if result.target.contains("windows") {
                    "bin"
                } else {
                    "lib"
                })
        });
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
    if result.target.ends_with("ohos") {
        // libc++_shared.so; system libraries come from the device.
        dirs.push(dependencies::ohos_sdk_native()?.join("llvm/lib/aarch64-linux-ohos"));
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
        if selected.native_vcpkg {
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
    let selected = dependencies::needs(inv, &result.target, &result.component)?;
    let mut entries = vec![];
    if selected.ffmpeg {
        let share = dependencies::ffmpeg_install(inv, &result.target)?.join("share");
        entries.push((
            share.join("ffmpeg/copyright"),
            "licenses/ffmpeg/copyright".into(),
        ));
        for entry in fs::read_dir(share)? {
            let entry = entry?;
            if entry.file_name() == "ffmpeg" {
                continue;
            }
            let copyright = entry.path().join("copyright");
            if copyright.is_file() {
                entries.push((
                    copyright,
                    format!(
                        "licenses/ffmpeg/{}/copyright",
                        entry.file_name().to_string_lossy()
                    ),
                ));
            }
        }
    }
    if result.target.contains("android") && result.component != "manager" {
        let ndk = dependencies::ndk_root(inv)?;
        for notice in ["NOTICE", "NOTICE.toolchain"] {
            entries.push((ndk.join(notice), format!("licenses/android-ndk/{notice}")));
        }
    }
    let separate_subtitle =
        selected.subtitle && !dependencies::shares_android_subtitle(&result.target, selected);
    if separate_subtitle
        && (result.target == "x86_64-unknown-linux-gnu" || crate::apple::supported(&result.target))
    {
        let share =
            dependencies::subtitle_install(inv, &result.target, &result.component)?.join("share");
        for entry in fs::read_dir(share)? {
            let entry = entry?;
            let copyright = entry.path().join("copyright");
            if copyright.is_file() {
                entries.push((
                    copyright,
                    format!(
                        "licenses/subtitle/{}/copyright",
                        entry.file_name().to_string_lossy()
                    ),
                ));
            }
        }
    } else if separate_subtitle {
        let share =
            dependencies::subtitle_install(inv, &result.target, &result.component)?.join("share");
        for port in [
            "libass", "freetype", "fribidi", "harfbuzz", "brotli", "bzip2", "libpng", "zlib",
        ] {
            entries.push((
                share.join(port).join("copyright"),
                format!("licenses/subtitle/{port}/copyright"),
            ));
        }
    }
    for native in dependencies::native_cores(inv, &result.target, &result.component, selected) {
        let native_share = dependencies::native_install(inv, &result.target, &native.id)?
            .join(dependencies::native_triplet(inv, &result.target)?)
            .join("share");
        for item in fs::read_dir(native_share)? {
            let path = item?.path().join("copyright");
            if path.is_file() {
                let port = path
                    .parent()
                    .and_then(Path::file_name)
                    .context("native license port")?
                    .to_string_lossy();
                entries.push((
                    path.clone(),
                    format!("licenses/native-vcpkg/{}/{port}/copyright", native.id),
                ));
            }
        }
    }
    if result.component == "manager" {
        entries.extend([
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
    let mut result = build::package_artifact(inv, target, component, version)?;
    reject_overlap(&result.library, out)?;
    if component == "manager" {
        if target.contains("android") {
            return package_android(inv, &result, version, out, &ANDROID_TARGETS);
        }
        if target.ends_with("ohos") {
            return crate::ohos::package(inv, &result, version, out);
        }
        let ios_signing = crate::apple::ios_frameworks::enabled(target, &result.features)
            .then(|| crate::apple::ios_frameworks::Signing::for_target(target))
            .transpose()?;
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
            for dep in binary::closure(&result.library, &search_dirs(inv, &result)?, target)? {
                entries.push(dep);
            }
            if dependencies::needs(inv, target, component)?.dxc {
                for name in [
                    "dxcompiler.dll",
                    "dxil.dll",
                    "LICENSE-MS.txt",
                    "LICENSE-LLVM.txt",
                    "LICENCE-MIT.txt",
                ] {
                    entries.push((inv.root.join(".work/dxc").join(name), name.into()));
                }
            }
            entries.extend(licenses(inv, &result, temp.path())?);
            entries.extend(metadata_entries(inv, temp.path())?);
            entries.extend(static_core_entries(inv, &result)?);
            copy_entries(&entries, stage)?;
            drop(temp);
            fs::create_dir_all(stage.join("cores"))?;
            fs::write(
                stage.join("README.md"),
                format!(
                    "# ShinkiEMU {version}\n\nRun shinki-emu-manager. Install separately released cores from Project-Shinkirou/ShinkiEMU.\n"
                ),
            )?;
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
                if target == "aarch64-apple-ios" {
                    let payload_stage = tempfile::tempdir_in(stage)?;
                    let payload = payload_stage.path().join("Payload");
                    fs::create_dir(&payload)?;
                    fs::rename(
                        &app,
                        payload.join(app.file_name().context("iOS app filename")?),
                    )?;
                    let ipa = stage.join(format!("ShinkiEMU-{version}-{target}-device-test.ipa"));
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
        let c = inv.core(component)?;
        let m = result
            .core_build
            .as_mut()
            .context("missing core-build.json")?;
        for (source, path, _) in c.license_files(&inv.root, target)? {
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
        for (source, dest) in licenses(inv, &result, scratch.path())? {
            result
                .core_build
                .as_mut()
                .unwrap()
                .licenses
                .push(BuildAsset { source, dest });
        }
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
}
/// Resolve license inputs from the actual Cargo checkouts used for embedding.
pub(crate) fn static_core_entries(
    inv: &Inventory,
    result: &BuildArtifact,
) -> Result<Vec<(PathBuf, String)>> {
    if !inv
        .cores
        .iter()
        .any(|core| core.static_manager_targets.contains(&result.target))
    {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for core in &inv.cores {
        if !core.static_manager_targets.contains(&result.target) {
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
        let native = result
            .embedded_core_builds
            .iter()
            .find(|build| build.plugin_id == core.id);
        if core.artifact_source() == crate::cores::ArtifactSource::NativeLibrary && native.is_none() {
            bail!(
                "embedded native core {} has no recorded build manifest",
                core.id
            );
        }
        if let Some(native) = native {
            native.validate()?;
            for asset in &native.resources {
                for (source, destination) in asset_entries(asset)? {
                    if result.target.ends_with("-linux-android")
                        && android_host::is_runtime_asset(&destination)
                    {
                        continue;
                    }
                    entries.push((source, format!("cores/{}/{destination}", core.id)));
                }
            }
            for asset in &native.licenses {
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
// The official APK signing certificate (SHA-256).
const ANDROID_PRODUCTION_CERT: &str =
    "c415bd462d26574758ed455055ff528a3bd6036e39d5ac6887308f8cf69e97fc";

/// Stage the ABI's native files; stage shared resources and Java inputs once.
pub(crate) fn android_inputs(
    inv: &Inventory,
    result: &BuildArtifact,
    stage: &Path,
    shared: bool,
) -> Result<()> {
    let abi = ANDROID_TARGETS
        .iter()
        .find(|(target, _)| *target == result.target)
        .map(|(_, abi)| *abi)
        .context("unsupported Android ABI")?;
    let mut entries = vec![(
        result.library.clone(),
        format!("jniLibs/{abi}/libshinki_emu_manager.so"),
    )];
    for dep in binary::closure(&result.library, &search_dirs(inv, result)?, &result.target)? {
        let (source, name) = dep;
        entries.push((source, format!("jniLibs/{abi}/{name}")));
    }
    if !shared {
        return copy_entries(&entries, stage);
    }
    let temp = crate::temporary::directory()?;
    let metadata = metadata_entries(inv, temp.path())?;
    let mut bundled = vec!["THIRD_PARTY_NOTICES.md".to_owned()];
    entries.push((
        inv.root.join("THIRD_PARTY_NOTICES.md"),
        "assets/THIRD_PARTY_NOTICES.md".into(),
    ));
    let ndk = dependencies::ndk_root(inv)?;
    for notice in ["NOTICE", "NOTICE.toolchain"] {
        let name = format!("licenses/android-ndk/{notice}");
        bundled.push(name.clone());
        entries.push((ndk.join(notice), format!("assets/{name}")));
    }
    for (source, name) in metadata
        .into_iter()
        .chain(licenses(inv, result, temp.path())?)
        .chain(static_core_entries(inv, result)?)
    {
        bundled.push(name.clone());
        entries.push((source, format!("assets/{name}")));
    }
    for (source, name) in android_host::inputs(inv, result)? {
        if let Some(asset) = name.strip_prefix("assets/") {
            bundled.push(asset.to_owned());
        }
        entries.push((source, name));
    }
    for (source, name) in &entries {
        if name.starts_with("assets/") && fs::metadata(source)?.len() > 32 * 1024 * 1024 {
            bail!("Android asset exceeds the runtime size limit: {name}");
        }
    }
    bundled.sort();
    let manifest = bundled.join("\n") + "\n";
    if manifest.len() > 1024 * 1024 {
        bail!("Android bundled manifest exceeds the runtime size limit");
    }
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

pub(crate) fn package_android_qa(
    inv: &Inventory,
    target: &str,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    let abi = ANDROID_TARGETS
        .iter()
        .find(|(triple, _)| *triple == target)
        .context("--qa-single-abi requires an Android Manager target")?;
    let version = crate::release::resolve_version(inv, "manager", version)?;
    let result = build::package_artifact(inv, target, "manager", &version)?;
    reject_overlap(&result.library, out)?;
    package_android(inv, &result, &version, out, std::slice::from_ref(abi))
}

fn package_android(
    inv: &Inventory,
    result: &BuildArtifact,
    version: &str,
    out: &Path,
    targets: &[(&str, &str)],
) -> Result<PathBuf> {
    use base64::Engine as _;
    let qa = targets.len() == 1;
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
    let version_code = if signed {
        std::env::var("SHINKI_EMU_ANDROID_VERSION_CODE")
            .context("set SHINKI_EMU_ANDROID_VERSION_CODE for a signed release APK")?
            .parse::<std::num::NonZeroU32>()?.get()
    } else {
        std::env::var("SHINKI_EMU_ANDROID_VERSION_CODE").as_deref().unwrap_or("1").parse()?
    };
    atomic_directory(out, |stage| {
        for (target, _) in targets {
            if *target == result.target {
                android_inputs(inv, result, stage, true)?;
            } else {
                let built = build::package_artifact(inv, target, "manager", version)?;
                reject_overlap(&built.library, out)?;
                android_inputs(inv, &built, stage, false)?;
            }
        }
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
        let variant = if qa { "qa-debug" } else if signed { "release" } else { "debug" };
        let platform = if qa { abi } else { "universal" };
        let apk_name = format!("ShinkiEMU-{version}-android-{platform}-{variant}-signed.apk");
        let apk = stage.join(&apk_name);
        fs::copy(source, &apk)?;
        verify_android_apk(&apk, &certificate, version, version_code)?;
        if !qa {
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
    let badging = std::str::from_utf8(&badging)?;
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
    if !badging.lines().any(|line| line == "sdkVersion:'28'") {
        bail!("Android APK must require API 28");
    }
    // The bundled manifest is the runtime extraction list.
    let mut archive = zip::ZipArchive::new(fs::File::open(apk)?)?;
    archive.by_name("assets/metadata/index.json")?;
    archive.by_name(ANDROID_BUNDLE_MANIFEST)?;
    Ok(())
}

#[cfg(test)]
mod android_tests {
    use super::*;
    use clap::Parser as _;

    #[test]
    fn single_abi_qa_is_available_only_for_manager_packaging() {
        use crate::cli::Cli;
        let package = ["xtask", "package", "--target", "x86_64-linux-android", "--out", "qa"];
        assert!(Cli::try_parse_from(package.into_iter().chain(["--manager", "--qa-single-abi"])).is_ok());
        assert!(Cli::try_parse_from(package.into_iter().chain(["--core", "musica", "--qa-single-abi"])).is_err());
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
