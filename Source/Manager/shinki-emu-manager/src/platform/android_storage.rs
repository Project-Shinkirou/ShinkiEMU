use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Prepare the app-owned parent; only a successful patch publishes the child.
pub(super) fn propose_export(data: &Path, name: &str, timestamp_ms: u128) -> io::Result<PathBuf> {
    let parent = data.join("exports");
    match fs::create_dir(&parent) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && parent.is_dir() => {}
        Err(error) => return Err(error),
    }
    Ok(parent.join(format!("{name}-{timestamp_ms}")))
}

/// Siglus separates resource editions and languages under this per-game root.
pub(crate) fn fill_siglus_save_root(
    data: &Path,
    game: &Path,
    value: &mut String,
) -> Result<(), String> {
    if !value.is_empty() {
        return Ok(());
    }
    let canonical = fs::canonicalize(game)
        .map_err(|error| format!("SHINKI_EMU_GAME_DIRECTORY_INVALID: {error}"))?;
    let identity = Sha256::digest(canonical.as_os_str().as_encoded_bytes());
    let path = data
        .join("siglus_rs")
        .join("saves")
        .join(hex::encode(identity));
    *value = path.to_str().ok_or("SHINKI_EMU_RESOURCE_PATH_UTF8")?.into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_selection_creates_only_export_parent() {
        let data = tempfile::tempdir().unwrap();
        let parent = data.path().join("exports");
        assert!(!parent.exists());
        let output = propose_export(data.path(), "localized-output", 42).unwrap();
        assert_eq!(output, parent.join("localized-output-42"));
        assert!(parent.is_dir());
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        assert_ne!(
            propose_export(data.path(), "localized-output", 43).unwrap(),
            output
        );
    }

    #[test]
    fn export_parent_file_is_reported_and_preserved() {
        let data = tempfile::tempdir().unwrap();
        let parent = data.path().join("exports");
        fs::write(&parent, b"keep").unwrap();
        let error = propose_export(data.path(), "localized-output", 42).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(parent).unwrap(), b"keep");
    }

    #[test]
    fn siglus_save_defaults_are_per_game_and_preserve_explicit_overrides() {
        let data = tempfile::tempdir().unwrap();
        let games = tempfile::tempdir().unwrap();
        let first_game = games.path().join("first");
        let other_game = games.path().join("second");
        fs::create_dir(&first_game).unwrap();
        fs::create_dir(&other_game).unwrap();
        let mut first = String::new();
        let mut second = String::new();
        let mut same = String::new();
        fill_siglus_save_root(data.path(), &first_game, &mut first).unwrap();
        fill_siglus_save_root(data.path(), &first_game.join("."), &mut same).unwrap();
        fill_siglus_save_root(data.path(), &other_game, &mut second).unwrap();
        assert_eq!(first, same);
        assert_ne!(first, second);
        assert!(Path::new(&first).starts_with(data.path().join("siglus_rs/saves")));
        assert!(!data.path().join("siglus_rs").exists());

        let missing = games.path().join("missing");
        let mut explicit = "explicit/invalid-relative-path".to_owned();
        fill_siglus_save_root(data.path(), &missing, &mut explicit).unwrap();
        assert_eq!(explicit, "explicit/invalid-relative-path");
        assert!(fill_siglus_save_root(data.path(), &missing, &mut String::new()).is_err());
    }
}
