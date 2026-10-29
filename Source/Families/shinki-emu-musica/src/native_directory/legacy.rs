//! Reader for the `global-progress.json` file Musica wrote before `system.dat`.

use super::*;
use serde::Deserialize;
use shinki_emu_sdk::runtime::SessionError;
use std::io::Read;

pub(super) const PROGRESS: &str = "global-progress.json";
const PROGRESS_SCHEMA: &str = "shinki.musica.global_progress.v1";
const PROGRESS_LIMIT: u64 = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    schema: String,
    game: Hash256,
    unlocks: Vec<Hash256>,
}

fn invalid_progress() -> SessionError {
    error(
        "SHINKI_EMU_MUSICA_GLOBAL_PROGRESS",
        "legacy global progress is invalid or belongs to another game",
    )
}

fn read(path: &Path, limit: u64, invalid: fn() -> SessionError) -> SessionResult<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_PATH",
                "legacy save file is not a native regular file",
            ));
        }
        Ok(_) => {}
        Err(cause) if cause.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid()),
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| invalid())?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(Some(bytes))
}

pub(super) fn progress(saves: &Path, game: Hash256) -> SessionResult<Option<Vec<Hash256>>> {
    let Some(bytes) = read(&saves.join(PROGRESS), PROGRESS_LIMIT, invalid_progress)? else {
        return Ok(None);
    };
    let value: Progress = serde_json::from_slice(&bytes).map_err(|_| invalid_progress())?;
    if value.schema != PROGRESS_SCHEMA || value.game != game {
        return Err(invalid_progress());
    }
    Ok(Some(value.unlocks))
}

/// The `.setglobal` clear flags (`= 1`) that the stored unlock hashes name.
pub(super) fn unlocked_globals(unlocks: &[Hash256]) -> SessionResult<BTreeMap<String, i64>> {
    Ok(crate::runtime::progress::unlocked_flags(unlocks)
        .map_err(|_| invalid_progress())?
        .into_iter()
        .map(|flag| (flag.to_owned(), 1))
        .collect())
}

/// Renames `name` to `name.imported` without replacing an existing file.
pub(super) fn mark_imported(saves: &Path, name: &str) -> SessionResult<()> {
    let source = saves.join(name);
    std::fs::hard_link(&source, saves.join(format!("{name}.imported"))).map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_SAVE_PATH",
            "legacy save file cannot be renamed without overwrite",
        )
    })?;
    std::fs::remove_file(source).map_err(|_| {
        error(
            "SHINKI_EMU_MUSICA_SAVE_PATH",
            "legacy save file source cannot be removed",
        )
    })
}
