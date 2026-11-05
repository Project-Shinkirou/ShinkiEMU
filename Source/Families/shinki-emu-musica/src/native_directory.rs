//! Data-driven native save directories and a game-local generic persistence
//! format for installations whose original format cannot be established.

use crate::{
    ScriptEncoding,
    minori_data::{self, FilePrefix, Properties, SaveIdentity},
    scene::{core_error, error},
};
use roukaku_core::Hash256;
use shinki_emu_sdk::runtime::SessionResult;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

mod legacy;
mod slots;
mod sysparams;
#[cfg(test)]
pub(crate) mod tests;

const SYSTEM: &str = "system.dat";
const CG_VIEWS: &str = "cgv.dat";
const IDENTITY: &str = "save-identity.json";

fn read_optional(path: &Path) -> SessionResult<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(cause) if cause.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error(
            "SHINKI_EMU_MUSICA_SAVE_READ",
            "native save file cannot be read",
        )),
    }
}

/// Replaces `dir/name` atomically through a sibling temporary file.
fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> SessionResult<()> {
    let failed = |_| {
        error(
            "SHINKI_EMU_MUSICA_SAVE_WRITE",
            "native save file cannot be written",
        )
    };
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(failed)?;
    file.write_all(bytes).map_err(failed)?;
    file.as_file().sync_all().map_err(failed)?;
    file.persist(dir.join(name))
        .map_err(|cause| failed(cause.error))?;
    Ok(())
}

struct NativeDirectory {
    identity: SaveIdentity,
    game_root: PathBuf,
    root: PathBuf,
    slot_root: PathBuf,
    prefix: FilePrefix,
}

impl NativeDirectory {
    fn open(game_root: &Path, identity: SaveIdentity) -> SessionResult<Self> {
        identity.validate().map_err(core_error)?;
        let root = game_root.join(&identity.directory);
        std::fs::create_dir_all(&root).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_PATH",
                "native save directory cannot be created",
            )
        })?;
        let prefix = minori_data::resolve_prefix(
            identity.serial_key.as_deref(),
            identity.prefix,
            registry_serial,
            existing_prefix(&root),
        );
        if !identity.extended && prefix.value != 0 {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                "basic checkpoint layout requires a zero container prefix",
            ));
        }
        Ok(Self {
            identity,
            game_root: game_root.to_path_buf(),
            slot_root: root.clone(),
            root,
            prefix,
        })
    }

    fn system_data(&self) -> SessionResult<Option<Properties>> {
        read_optional(&self.root.join(SYSTEM))?
            .map(|bytes| minori_data::decode_system(&bytes, self.identity.system_format))
            .transpose()
            .map_err(core_error)
    }

    /// Skips the write when the stored table already decodes to `properties`.
    fn write_system_data(&self, properties: &Properties) -> SessionResult<()> {
        if self
            .system_data()
            .is_ok_and(|stored| stored.as_ref() == Some(properties))
        {
            return Ok(());
        }
        let bytes = minori_data::encode_system(properties, self.identity.system_format)
            .map_err(core_error)?;
        replace_file(&self.root, SYSTEM, &bytes)
    }

    fn cg_names(&self) -> SessionResult<Vec<Vec<u8>>> {
        read_optional(&self.root.join(CG_VIEWS))?
            .map(|bytes| minori_data::decode_cg_names(&bytes, self.prefix.value))
            .transpose()
            .map_err(core_error)
            .map(Option::unwrap_or_default)
    }

    fn write_cg_names(&self, names: &[Vec<u8>]) -> SessionResult<()> {
        if self.cg_names().is_ok_and(|stored| stored == names) {
            return Ok(());
        }
        let bytes = minori_data::encode_cg_names(names, self.prefix.value).map_err(core_error)?;
        replace_file(&self.root, CG_VIEWS, &bytes)
    }
}

/// Candidate locations are layout conventions, not game identities. Existing
/// portable minori folders are scanned by their contents, without naming titles.
fn discover_identity(
    game_root: &Path,
    encoding: ScriptEncoding,
    preset: Option<&SaveIdentity>,
) -> SessionResult<Option<SaveIdentity>> {
    if let Some(preset) = preset {
        return parse_directory_identity(game_root, &game_root.join(&preset.directory), encoding);
    }
    let mut directories = vec![
        game_root.to_path_buf(),
        game_root.join("save"),
        game_root.join("savedata"),
    ];
    let portable = game_root.join(".shinki/musica/minori");
    match std::fs::read_dir(&portable) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|_| {
                    error(
                        "SHINKI_EMU_MUSICA_SAVE_READ",
                        "save directory cannot be listed",
                    )
                })?;
                if entry.path().is_dir() {
                    directories.push(entry.path());
                }
            }
        }
        Err(cause) if cause.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_READ",
                "save directory cannot be listed",
            ));
        }
    }
    directories.sort();
    let mut found = None;
    for directory in directories {
        let Some(identity) = parse_directory_identity(game_root, &directory, encoding)? else {
            if directory.starts_with(&portable)
                && std::fs::read_dir(&directory)
                    .map_err(|_| {
                        error(
                            "SHINKI_EMU_MUSICA_SAVE_READ",
                            "save directory cannot be listed",
                        )
                    })?
                    .next()
                    .is_some()
            {
                return Err(error(
                    "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                    "existing portable saves need a native_save descriptor; original files were left untouched",
                ));
            }
            continue;
        };
        if found.replace(identity).is_some() {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                "multiple native save identities require an explicit descriptor",
            ));
        }
    }
    Ok(found)
}

fn parse_directory_identity(
    game_root: &Path,
    directory: &Path,
    encoding: ScriptEncoding,
) -> SessionResult<Option<SaveIdentity>> {
    use crate::eden_save::{EdenEdition, EdenSave, EdenSaveEncoding};
    let Some(system) = read_optional(&directory.join(SYSTEM))? else {
        return Ok(None);
    };
    let system_format = [
        minori_data::SystemFormat::Rc4,
        minori_data::SystemFormat::Blowfish,
    ]
    .into_iter()
    .find(|format| minori_data::decode_system(&system, *format).is_ok());
    let Some(system_format) = system_format else {
        return Ok(None);
    };
    let mut paths = std::fs::read_dir(directory)
        .map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_READ",
                "save directory cannot be listed",
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_READ",
                "save directory cannot be listed",
            )
        })?;
    paths.sort();
    let mut identities: Vec<SaveIdentity> = Vec::new();
    for path in paths {
        if path.extension().is_none_or(|extension| extension != "sav") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let digits = stem.bytes().rev().take_while(u8::is_ascii_digit).count();
        if !(1..=8).contains(&digits) {
            continue;
        }
        let slot_prefix = &stem[..stem.len() - digits];
        if identities
            .iter()
            .any(|identity| identity.slot_prefix == slot_prefix)
        {
            continue;
        }
        let bytes = read_optional(&path)?.unwrap_or_default();
        let Some(raw) = bytes.get(4..) else { continue };
        let Some(end) = raw
            .iter()
            .position(|byte| *byte == 0)
            .filter(|end| (1..=128).contains(end))
        else {
            continue;
        };
        let Ok(signature) = std::str::from_utf8(&raw[..end]) else {
            continue;
        };
        // Layout is established from decoded fields, not from the signature text.
        let edition = if minori_data::file_prefix(&bytes) == Some(0) {
            EdenEdition::English
        } else {
            EdenEdition::Perseus
        };
        let Ok(save) = EdenSave::decode_with_signature(
            &bytes,
            edition,
            EdenSaveEncoding::from_script(encoding),
            signature.as_bytes(),
        ) else {
            continue;
        };
        let extended = save
            .variables
            .iter()
            .any(|field| field.name == "eff2_Count");
        if !extended && save.prefix != 0 {
            continue;
        }
        let identity = SaveIdentity {
            version: 1,
            directory: {
                let relative = directory.strip_prefix(game_root).unwrap_or(directory);
                if relative.as_os_str().is_empty() {
                    ".".into()
                } else {
                    relative.to_path_buf()
                }
            },
            slot_prefix: slot_prefix.to_owned(),
            append_slot_prefix: None,
            slot_digits: digits as u8,
            signature: signature.to_owned(),
            system_format,
            extended,
            serial_key: None,
            prefix: minori_data::file_prefix(&bytes),
            auto_save_cycle: if extended { 400 } else { 200 },
            voice_keys: None,
        };
        identity.validate().map_err(core_error)?;
        identities.push(identity);
    }
    identities.sort_by(|a, b| {
        a.slot_prefix
            .len()
            .cmp(&b.slot_prefix.len())
            .then(a.slot_prefix.cmp(&b.slot_prefix))
    });
    let Some(mut identity) = identities.first().cloned() else {
        return Ok(None);
    };
    for other in identities.iter().skip(1) {
        if other.signature != identity.signature
            || other.extended != identity.extended
            || other.prefix != identity.prefix
            || other.slot_digits != identity.slot_digits
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                "native save files disagree on their format",
            ));
        }
        if other.slot_prefix != identity.slot_prefix {
            if identity
                .append_slot_prefix
                .as_ref()
                .is_some_and(|prefix| prefix != &other.slot_prefix)
            {
                return Err(error(
                    "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                    "native slot naming is ambiguous",
                ));
            }
            identity.append_slot_prefix = Some(other.slot_prefix.clone());
        }
    }
    Ok(Some(identity))
}

/// The prefix of the first readable compressed original file, in name order.
fn existing_prefix(dir: &Path) -> Option<u32> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name().is_some_and(|name| name == CG_VIEWS)
                || path.extension().is_some_and(|extension| extension == "tbr")
        })
        .collect();
    names.sort();
    names.into_iter().find_map(|path| {
        let bytes = std::fs::read(&path).ok()?;
        let prefix = minori_data::file_prefix(&bytes)?;
        let valid = if path.file_name().is_some_and(|name| name == CG_VIEWS) {
            minori_data::decode_cg_names(&bytes, prefix).is_ok()
        } else {
            minori_data::decode_read_flags(&bytes, prefix).is_ok()
        };
        valid.then_some(prefix)
    })
}

#[cfg(windows)]
fn registry_serial(key: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, REG_SZ, RegCloseKey, RegOpenKeyExA,
        RegQueryValueExA,
    };
    let key = std::ffi::CString::new(key).ok()?;
    let value = c"SerialCode";
    let mut handle: HKEY = std::ptr::null_mut();
    // SAFETY: `key` is NUL-terminated and `handle` is a valid out pointer.
    let opened = unsafe {
        RegOpenKeyExA(
            HKEY_LOCAL_MACHINE,
            key.as_ptr().cast(),
            0,
            KEY_READ | KEY_WOW64_32KEY,
            &mut handle,
        )
    };
    if opened != 0 {
        return None;
    }
    let mut kind = 0;
    let mut size = 0u32;
    // SAFETY: `handle` is open; a null data pointer asks only for the size.
    let sized = unsafe {
        RegQueryValueExA(
            handle,
            value.as_ptr().cast(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    let mut buffer = vec![0u8; size as usize];
    let read = sized == 0
        && kind == REG_SZ
        && size != 0
        // SAFETY: `buffer` holds `size` writable bytes.
        && unsafe {
            RegQueryValueExA(
                handle,
                value.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                buffer.as_mut_ptr(),
                &mut size,
            )
        } == 0;
    // SAFETY: `handle` was opened above and is closed exactly once.
    unsafe { RegCloseKey(handle) };
    read.then(|| {
        buffer.truncate(size as usize);
        buffer
    })
}

/// The original reads `SerialCode` only from the Windows registry; elsewhere
/// `resolve_prefix` falls back to an existing file's prefix, then zero.
#[cfg(not(windows))]
fn registry_serial(_key: &str) -> Option<Vec<u8>> {
    None
}

fn integer(raw: &[u8]) -> Option<i64> {
    std::str::from_utf8(raw).ok()?.parse().ok()
}

fn global(encoding: ScriptEncoding, key: &[u8], value: &[u8]) -> Option<(String, i64)> {
    Some((encoding.decode(key)?.into_owned(), integer(value)?))
}

/// `<stem>.tbr`; the stem comes from an archive script name and must stay a
/// plain file name inside the native directory.
fn read_flags_name(stem: &str) -> SessionResult<String> {
    if stem.is_empty()
        || stem.starts_with('.')
        || stem.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
    {
        return Err(error(
            "SHINKI_EMU_MUSICA_SAVE_PATH",
            "script name cannot name a read-flag file",
        ));
    }
    Ok(format!("{stem}.tbr"))
}

/// Session view of globals, CGs, settings, read flags and message checkpoints.
/// Every successful mutation is persisted, including unidentified installations.
pub(crate) struct NativeFiles {
    dir: NativeDirectory,
    encoding: ScriptEncoding,
    system: Properties,
    cg: BTreeSet<Vec<u8>>,
    /// `sysparams.txt`, `None` before the first write.
    settings: Option<Properties>,
    /// `musica-settings.txt`: settings without an original key.
    musica: Properties,
}

impl NativeFiles {
    pub(crate) fn open(game_root: &Path, encoding: ScriptEncoding) -> SessionResult<Self> {
        Self::open_configured(game_root, encoding, "")
    }

    pub(crate) fn open_configured(
        game_root: &Path,
        encoding: ScriptEncoding,
        configured: &str,
    ) -> SessionResult<Self> {
        // Parse original files first. A preset supplies facts that cannot be
        // recovered from them, and resolves ambiguous source directories.
        let cached = if configured.is_empty() {
            read_optional(&game_root.join(".shinki/musica").join(IDENTITY))?
        } else {
            Some(configured.as_bytes().to_vec())
        };
        let preset: Option<SaveIdentity> = cached
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                    "save identity JSON is invalid",
                )
            })?;
        if let Some(preset) = &preset {
            preset.validate().map_err(core_error)?;
        }
        let parsed = discover_identity(game_root, encoding, preset.as_ref())?;
        let identity = match (parsed, preset) {
            (Some(mut source), Some(preset)) => {
                if preset.append_slot_prefix.as_ref() == Some(&source.slot_prefix) {
                    source.slot_prefix = preset.slot_prefix;
                }
                source.serial_key = preset.serial_key;
                source.append_slot_prefix = source.append_slot_prefix.or(preset.append_slot_prefix);
                source.voice_keys = preset.voice_keys;
                source.auto_save_cycle = preset.auto_save_cycle;
                source
            }
            (Some(source), None) => source,
            (None, Some(preset)) => preset,
            (None, None) => SaveIdentity::generic(),
        };
        let dir = NativeDirectory::open(game_root, identity)?;
        let mut files = Self {
            dir,
            encoding,
            system: Properties::new(),
            cg: BTreeSet::new(),
            settings: None,
            musica: Properties::new(),
        };
        let dir = &files.dir;
        files.system = dir.system_data()?.unwrap_or_default();
        files.cg = dir.cg_names()?.into_iter().collect();
        files.settings = dir.settings()?;
        files.musica = dir.musica_settings()?;
        let cache_dir = game_root.join(".shinki/musica");
        std::fs::create_dir_all(&cache_dir).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_PATH",
                "save configuration directory cannot be created",
            )
        })?;
        let bytes = serde_json::to_vec(&files.dir.identity).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_CONFIG",
                "save identity cannot be serialized",
            )
        })?;
        if read_optional(&cache_dir.join(IDENTITY))?.as_deref() != Some(bytes.as_slice()) {
            replace_file(&cache_dir, IDENTITY, &bytes)?;
        }
        Ok(files)
    }

    /// One-time, no-clobber import of `<game>/.shinki/musica/saves/global-progress.json`
    /// into `system.dat`; the source becomes `global-progress.json.imported`.
    /// An existing `system.dat` is never overwritten and the JSON stays put.
    pub(crate) fn import_legacy_progress(
        &mut self,
        game_root: &Path,
        game: Hash256,
    ) -> SessionResult<()> {
        let saves = game_root.join(".shinki/musica/saves");
        let dir = &self.dir;
        if dir.system_data()?.is_some() {
            return Ok(());
        }
        let Some(unlocks) = legacy::progress(&saves, game)? else {
            return Ok(());
        };
        let globals = legacy::unlocked_globals(&unlocks)?;
        let system: Properties = globals
            .into_iter()
            .map(|(flag, value)| (flag.into_bytes(), value.to_string().into_bytes()))
            .collect();
        dir.write_system_data(&system)?;
        self.system = system;
        legacy::mark_imported(&saves, legacy::PROGRESS)
    }

    /// The `.setglobal` table stored in `system.dat`.
    pub(crate) fn globals(&self) -> BTreeMap<String, i64> {
        self.system
            .iter()
            .filter_map(|(key, value)| global(self.encoding, key, value))
            .collect()
    }

    /// Replaces the representable entries of `system.dat` with `globals`;
    /// entries Musica cannot decode stay verbatim.
    pub(crate) fn store_globals(&mut self, globals: &BTreeMap<String, i64>) -> SessionResult<()> {
        let dir = &self.dir;
        let encoding = self.encoding;
        let mut system: Properties = self
            .system
            .iter()
            .filter(|(key, value)| global(encoding, key, value).is_none())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (name, value) in globals {
            let key = encoding.encode(name).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_WRITE",
                    "global variable name has no native encoding",
                )
            })?;
            system.insert(key.into_owned(), value.to_string().into_bytes());
        }
        dir.write_system_data(&system)?;
        self.system = system;
        Ok(())
    }

    /// Adds one shown background image (lowercase file name) to `cgv.dat`.
    pub(crate) fn record_cg(&mut self, name: &str) -> SessionResult<()> {
        let dir = &self.dir;
        let bytes = self.encoding.encode(name).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_WRITE",
                "image name has no native encoding",
            )
        })?;
        if self.cg.contains(bytes.as_ref()) {
            return Ok(());
        }
        let mut names = self.cg.clone();
        names.insert(bytes.into_owned());
        dir.write_cg_names(&names.iter().cloned().collect::<Vec<_>>())?;
        self.cg = names;
        Ok(())
    }

    /// The stored `<stem>.tbr` read bitmap, `None` before the first write.
    pub(crate) fn read_flags(
        &self,
        stem: &str,
    ) -> SessionResult<Option<crate::runtime::ReadBitmap>> {
        let dir = &self.dir;
        read_optional(&dir.root.join(read_flags_name(stem)?))?
            .map(|bytes| minori_data::decode_read_flags(&bytes, dir.prefix.value))
            .transpose()
            .map_err(core_error)
    }

    /// Replaces `<stem>.tbr` with `bits`.
    pub(crate) fn store_read_flags(
        &mut self,
        stem: &str,
        bits: &crate::runtime::ReadBitmap,
    ) -> SessionResult<()> {
        let dir = &self.dir;
        let name = read_flags_name(stem)?;
        let bytes = minori_data::encode_read_flags(bits, dir.prefix.value).map_err(core_error)?;
        replace_file(&dir.root, &name, &bytes)
    }
}
