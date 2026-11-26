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
}
