//! Core directory layout v2: `cores/<stem>/` holds one Family library, its
//! dependencies and a `core.json` manifest. Installs and removals are staged
//! under `cores/.pending/` and applied before the owner loads any core.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use shinki_emu_manager_core::{
    is_family_library_path, library_stem, INDEPENDENT_FAMILY_ABI_FINGERPRINT,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const PENDING: &str = ".pending";
pub(crate) const CORE_MANIFEST: &str = "core.json";
pub(crate) const CORE_PACKAGE_SCHEMA: &str = "shinki.emu.core_package.v1";
const REMOVE_SUFFIX: &str = ".remove";
const OLD_SUFFIX: &str = ".old";
const PARTIAL_PREFIX: &str = ".partial-";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_ENTRIES: usize = 8192;
const MAX_EXTRACTED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PACKAGE_PATH_BYTES: usize = 512;
const MAX_BUILD_RECEIPT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoreManifest {
    pub(crate) schema: String,
    pub(crate) plugin_id: String,
    pub(crate) library: String,
    pub(crate) version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) identity: Option<shinki_emu_metadata::CoreIdentity>,
    #[serde(default, skip_serializing_if = "shinki_emu_metadata::CoreRecognition::is_empty")]
    pub(crate) recognition: shinki_emu_metadata::CoreRecognition,
    pub(crate) abi: String,
    pub(crate) files: Vec<CoreFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CoreFile {
    pub(crate) path: String,
    pub(crate) sha256: String,
}

impl CoreManifest {
    fn validate(&self, expected_plugin_id: Option<&str>) -> Result<String, String> {
        let invalid = || "SHINKI_EMU_CORE_PACKAGE_MANIFEST".to_owned();
        if self.schema != CORE_PACKAGE_SCHEMA
            || self.abi != INDEPENDENT_FAMILY_ABI_FINGERPRINT
            || self.plugin_id.is_empty()
            || self.plugin_id.len() > 128
            || shinki_emu_metadata::validate_core_version(&self.version, self.identity.as_ref()).is_err()
            || self.library.contains(['/', '\\'])
            || !is_family_library_path(Path::new(&self.library))
        {
            return Err(invalid());
        }
        if expected_plugin_id.is_some_and(|expected| expected != self.plugin_id) {
            return Err("SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH".into());
        }
        let mut paths = BTreeSet::new();
        for file in &self.files {
            portable_path(&file.path)?;
            if !paths.insert(file.path.to_ascii_lowercase())
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(invalid());
            }
        }
        let libraries = self
            .files
            .iter()
            .filter(|file| {
                !file.path.contains('/') && is_family_library_path(Path::new(&file.path))
            })
            .count();
        if libraries != 1 || !paths.contains(&self.library.to_ascii_lowercase()) {
            return Err(invalid());
        }
        Ok(library_stem(Path::new(&self.library)))
    }
}

#[derive(Deserialize)]
struct BuildReceipt {
    schema: String,
    component: String,
    target: String,
    profile: String,
    product_version: String,
    source_commit: String,
    source_dirty: bool,
}

/// The receipt hash is already checked at official staging. Recheck this
/// small file when admitting or presenting an installed first-party core.
pub(crate) fn product_source_commit(directory: &Path, manifest: &CoreManifest) -> Option<String> {
    if manifest.identity.is_some() {
        return None;
    }
    let listed = manifest.files.iter().find(|file| file.path == "build.json")?;
    let path = directory.join(&listed.path);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BUILD_RECEIPT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(MAX_BUILD_RECEIPT_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BUILD_RECEIPT_BYTES || sha256_hex(&bytes) != listed.sha256 {
        return None;
    }
    let receipt: BuildReceipt = serde_json::from_slice(&bytes).ok()?;
    (receipt.schema == "shinki.emu.build_identity.v1"
        && receipt.component == manifest.plugin_id
        && receipt.target == env!("SHINKI_EMU_BUILD_TARGET")
        && receipt.profile == "release"
        && receipt.product_version == manifest.version
        && !receipt.source_dirty
        && receipt.source_commit.len() == 40
        && receipt.source_commit.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
        .then_some(receipt.source_commit)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes).as_slice())
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A package-relative path: forward slashes, no absolute/drive/`.`/`..` parts.
fn portable_path(path: &str) -> Result<PathBuf, String> {
    let rejected = || "SHINKI_EMU_CORE_PACKAGE_PATH".to_owned();
    if path.is_empty()
        || path.len() > MAX_PACKAGE_PATH_BYTES
        || path.contains(['\\', ':', '\0'])
        || path.starts_with('/')
    {
        return Err(rejected());
    }
    let mut relative = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.chars().any(char::is_control) {
            return Err(rejected());
        }
        relative.push(part);
    }
    Ok(relative)
}

fn io_error(code: &str) -> impl Fn(io::Error) -> String + '_ {
    move |error| format!("{code}: {}", error.kind())
}

fn remove_path(path: &Path) -> io::Result<()> {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Copy a staged directory into `pending/<stem>/` atomically: the content is
/// prepared under a partial name and renamed into place; a stale removal
/// marker for the same stem is dropped.
fn publish_pending(pending: &Path, stem: &str, partial: &Path) -> Result<(), String> {
    let target = pending.join(stem);
    remove_path(&target).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    fs::rename(partial, &target).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    remove_path(&pending.join(format!("{stem}{REMOVE_SUFFIX}")))
        .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))
}

fn partial_dir(pending: &Path) -> Result<tempfile::TempDir, String> {
    fs::create_dir_all(pending).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    tempfile::Builder::new()
        .prefix(PARTIAL_PREFIX)
        .tempdir_in(pending)
        .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))
}

/// Snapshot local input while hashing so the bytes staged are the bytes matched
/// against the already verified release catalog, even if the source is edited.
pub(crate) fn stage_local_release<'a>(
    archive: &Path,
    entries: impl Iterator<Item = &'a shinki_emu_metadata::CoreCatalogEntry>,
    pending: &Path,
) -> Result<String, String> {
    use std::io::{Seek, SeekFrom};
    let read_error = io_error("SHINKI_EMU_CORE_PACKAGE_READ");
    let input = fs::File::open(archive).map_err(&read_error)?;
    let size = input.metadata().map_err(&read_error)?.len();
    if size > shinki_emu_metadata::MAX_CORE_PACKAGE_BYTES {
        return Err("SHINKI_EMU_CORE_PACKAGE_BOUNDS".into());
    }
    let mut releases = entries.filter(|entry| entry.size == size).peekable();
    if releases.peek().is_none() { return Err("SHINKI_EMU_CORE_NOT_AVAILABLE".into()); }
    let mut input = input.take(size + 1);
    let mut snapshot = tempfile::tempfile().map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut copied = 0;
    loop {
        let count = input.read(&mut buffer).map_err(&read_error)?;
        if count == 0 { break; }
        snapshot.write_all(&buffer[..count]).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
        hasher.update(&buffer[..count]);
        copied += count as u64;
    }
    let digest = hex_digest(hasher.finalize().as_slice());
    let release = releases.find(|entry| copied == size && entry.sha256 == digest)
        .ok_or("SHINKI_EMU_CORE_NOT_AVAILABLE")?;
    snapshot.seek(SeekFrom::Start(0)).map_err(&read_error)?;
    let (stem, _) = stage_open_package(
        snapshot,
        Some((&release.plugin_id, &release.version, release.identity.as_ref())),
        pending,
    )?;
    Ok(stem)
}

type ExpectedRelease<'a> = (&'a str, &'a str, Option<&'a shinki_emu_metadata::CoreIdentity>);
/// Validate a core package zip and stage it as `pending/<stem>/`.
///
/// Rejects absolute paths, `..`, symlinks and duplicate entries, a manifest
/// whose plugin/version/provenance differs from `expected`, and any file whose
/// SHA-256 is not the manifest value. Returns the staged stem.
pub(crate) fn stage_package(
    archive: &Path,
    expected: Option<ExpectedRelease<'_>>,
    pending: &Path,
) -> Result<(String, CoreManifest), String> {
    let file = fs::File::open(archive).map_err(io_error("SHINKI_EMU_CORE_PACKAGE_READ"))?;
    stage_open_package(file, expected, pending)
}

fn stage_open_package(
    file: fs::File,
    expected: Option<ExpectedRelease<'_>>,
    pending: &Path,
) -> Result<(String, CoreManifest), String> {
    let mut zip = zip::ZipArchive::new(io::BufReader::new(file))
        .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?;
    if zip.len() > MAX_PACKAGE_ENTRIES {
        return Err("SHINKI_EMU_CORE_PACKAGE_BOUNDS".into());
    }
    let mut entries = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut manifest_index = None;
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?;
        let name = entry
            .name()
            .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_PATH".to_owned())?
            .into_owned();
        if entry.is_symlink() || entry.enclosed_name().is_none() {
            return Err("SHINKI_EMU_CORE_PACKAGE_PATH".into());
        }
        let directory = entry.is_dir();
        let trimmed = if directory {
            name.strip_suffix('/').unwrap_or(&name).to_owned()
        } else {
            name.clone()
        };
        portable_path(&trimmed)?;
        if !names.insert(trimmed.to_ascii_lowercase()) {
            return Err("SHINKI_EMU_CORE_PACKAGE_DUPLICATE".into());
        }
        if directory {
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_EXTRACTED_BYTES {
            return Err("SHINKI_EMU_CORE_PACKAGE_BOUNDS".into());
        }
        if trimmed == CORE_MANIFEST {
            manifest_index = Some(index);
        } else {
            entries.insert(trimmed, index);
        }
    }
    let manifest_index = manifest_index.ok_or("SHINKI_EMU_CORE_PACKAGE_MANIFEST")?;
    let mut manifest_bytes = Vec::new();
    zip.by_index(manifest_index)
        .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest_bytes)
        .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("SHINKI_EMU_CORE_PACKAGE_MANIFEST".into());
    }
    let manifest: CoreManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_MANIFEST".to_owned())?;
    let stem = manifest.validate(expected.map(|(plugin, _, _)| plugin))?;
    if expected.is_some_and(|(_, version, identity)| manifest.version != version || manifest.identity.as_ref() != identity) {
        return Err("SHINKI_EMU_CORE_PACKAGE_MANIFEST".into());
    }
    let listed: BTreeSet<&str> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    if listed.len() != entries.len() || !entries.keys().all(|path| listed.contains(path.as_str())) {
        return Err("SHINKI_EMU_CORE_PACKAGE_MANIFEST".into());
    }
    let partial = partial_dir(pending)?;
    for file in &manifest.files {
        let destination = partial.path().join(portable_path(&file.path)?);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
        }
        let mut source = zip
            .by_index(entries[&file.path])
            .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?;
        let mut output =
            fs::File::create(&destination).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|_| "SHINKI_EMU_CORE_PACKAGE_FORMAT".to_owned())?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            output
                .write_all(&buffer[..read])
                .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
        }
        output
            .sync_all()
            .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
        if hex_digest(hasher.finalize().as_slice()) != file.sha256 {
            return Err(format!(
                "SHINKI_EMU_CORE_PACKAGE_HASH_MISMATCH: {}",
                file.path
            ));
        }
    }
    fs::write(partial.path().join(CORE_MANIFEST), &manifest_bytes)
        .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    #[cfg(target_os = "android")]
    if shinki_emu_manager_core::static_core_catalog::lookup(&manifest.plugin_id)
        .is_some_and(|core| core.first_party)
        && product_source_commit(partial.path(), &manifest).is_none()
    {
        return Err("SHINKI_EMU_CORE_PACKAGE_RELEASE_UNSUPPORTED: missing verified product build receipt".into());
    }
    publish_pending(pending, &stem, &partial.keep())?;
    Ok((stem, manifest))
}

#[cfg(not(target_os = "android"))]
fn write_generated(directory: &Path, name: &str, stem: &str, bytes: &[u8]) -> Result<(), String> {
    fs::write(directory.join(name), bytes).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    let manifest = CoreManifest {
        schema: CORE_PACKAGE_SCHEMA.into(),
        plugin_id: stem.into(),
        library: name.into(),
        version: "0.0.0-local".into(),
        identity: None,
        recognition: Default::default(),
        abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files: vec![CoreFile {
            path: name.into(),
            sha256: sha256_hex(bytes),
        }],
    };
    let mut encoded = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| "SHINKI_EMU_CORE_INSTALL_COPY".to_owned())?;
    encoded.push(b'\n');
    fs::write(directory.join(CORE_MANIFEST), encoded)
        .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))
}

/// Read `core.json` from a core directory.
pub(crate) fn read_manifest(directory: &Path) -> Option<CoreManifest> {
    let path = directory.join(CORE_MANIFEST);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return None;
    }
    let manifest: CoreManifest = serde_json::from_slice(&bytes).ok()?;
    manifest.validate(None).ok()?;
    Some(manifest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingKind {
    Install,
    Remove,
}

fn is_stem(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

/// Pending installs (`pending/<stem>/`) and removals (`pending/<stem>.remove`).
pub(crate) fn pending_entries(cores: &Path) -> Result<BTreeMap<String, PendingKind>, String> {
    let entries = match fs::read_dir(cores.join(PENDING)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("SHINKI_EMU_CORE_PENDING_APPLY: {}", error.kind())),
    };
    let mut pending = BTreeMap::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("SHINKI_EMU_CORE_PENDING_APPLY: {}", error.kind()))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if let Some(stem) = name
            .strip_suffix(REMOVE_SUFFIX)
            .filter(|stem| is_stem(stem))
        {
            if !directory {
                pending.insert(stem.to_owned(), PendingKind::Remove);
            }
        } else if directory && is_stem(&name) && !name.ends_with(OLD_SUFFIX) {
            pending.entry(name).or_insert(PendingKind::Install);
        }
    }
    Ok(pending)
}

pub(crate) fn mark_removal(cores: &Path, stem: &str) -> Result<(), String> {
    if !is_stem(stem) {
        return Err("SHINKI_EMU_CORE_INSTALL_NAME".into());
    }
    let pending = cores.join(PENDING);
    fs::create_dir_all(&pending).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    remove_path(&pending.join(stem)).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))?;
    fs::write(pending.join(format!("{stem}{REMOVE_SUFFIX}")), [])
        .map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))
}

pub(crate) fn cancel_pending_install(cores: &Path, stem: &str) -> Result<(), String> {
    if !is_stem(stem) {
        return Err("SHINKI_EMU_CORE_INSTALL_NAME".into());
    }
    remove_path(&cores.join(PENDING).join(stem)).map_err(io_error("SHINKI_EMU_CORE_INSTALL_COPY"))
}

fn failure(stem: &str, error: &io::Error) -> (String, String) {
    tracing::error!(
        event = "shinki.emu.manager.core_pending_failed",
        core = %stem,
        error = %error
    );
    (
        stem.to_owned(),
        format!("SHINKI_EMU_CORE_PENDING_APPLY: {}", error.kind()),
    )
}

/// Move pre-v2 top-level `cores/shinki_emu_*.dll` files into `cores/<stem>/`.
#[cfg(not(target_os = "android"))]
fn migrate_legacy(cores: &Path, failures: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(cores) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) || !is_family_library_path(&path) {
            continue;
        }
        let stem = library_stem(&path);
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let directory = cores.join(&stem);
        if directory.exists() {
            failures.push(failure(
                &stem,
                &io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "legacy core beside v2 directory",
                ),
            ));
            continue;
        }
        let migrated = fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                fs::create_dir(&directory).map_err(|error| error.to_string())?;
                write_generated(&directory, name, &stem, &bytes)
            })
            .and_then(|()| fs::remove_file(&path).map_err(|error| error.to_string()));
        match migrated {
            Ok(()) => {
                tracing::info!(event = "shinki.emu.manager.core_migrated", core = %stem);
            }
            Err(error) => {
                let _ = remove_path(&directory);
                failures.push(failure(&stem, &io::Error::other(error)));
            }
        }
    }
}

#[cfg(any(not(target_os = "android"), test))]
pub(crate) fn apply_pending(cores: &Path) -> Vec<(String, String)> {
    apply_pending_checked(cores, |_, _| Ok(()))
}

/// Apply staged changes before the owner loads cores: migrate legacy files,
/// then removals, then installs. An install renames the old directory to
/// `pending/<stem>.old`, moves the new one into place and deletes the old
/// copy; a failed move restores the old directory.
pub(crate) fn apply_pending_checked(
    cores: &Path,
    validate_install: impl Fn(&str, &Path) -> Result<(), &'static str>,
) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    #[cfg(not(target_os = "android"))]
    migrate_legacy(cores, &mut failures);
    let pending = cores.join(PENDING);
    let entries = match pending_entries(cores) {
        Ok(entries) => entries,
        Err(error) => return vec![("<pending>".into(), error)],
    };
    for (stem, kind) in &entries {
        if *kind != PendingKind::Remove {
            continue;
        }
        if let Err(error) = remove_path(&cores.join(stem)) {
            failures.push(failure(stem, &error));
            continue;
        }
        if let Err(error) = fs::remove_file(pending.join(format!("{stem}{REMOVE_SUFFIX}"))) {
            failures.push(failure(stem, &error));
        }
    }
    for (stem, kind) in &entries {
        if *kind != PendingKind::Install {
            continue;
        }
        let staged = pending.join(stem);
        if let Err(error) = validate_install(stem, &staged) {
            failures.push((stem.clone(), error.to_owned()));
            continue;
        }
        let target = cores.join(stem);
        let old = pending.join(format!("{stem}{OLD_SUFFIX}"));
        if let Err(error) = remove_path(&old) {
            failures.push(failure(stem, &error));
            continue;
        }
        let had_old = target.exists();
        if had_old
            && let Err(error) = fs::rename(&target, &old) {
                failures.push(failure(stem, &error));
                continue;
            }
        if let Err(error) = fs::rename(staged, &target) {
            if had_old
                && let Err(restore) = fs::rename(&old, &target) {
                    failures.push(failure(stem, &restore));
                }
            failures.push(failure(stem, &error));
            continue;
        }
        if let Err(error) = remove_path(&old) {
            failures.push(failure(stem, &error));
        }
    }
    failures
}

#[cfg(test)]
#[path = "core_package_tests.rs"]
mod tests;
