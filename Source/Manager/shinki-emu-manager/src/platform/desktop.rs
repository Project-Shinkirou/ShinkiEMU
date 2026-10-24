//! Desktop platform services: native dialogs through `rfd`, the per-user data
//! directory, and in-place process restart.
use std::path::PathBuf;

use super::{FileFilter, PickDone};

pub(super) fn pick_folder(title: &str, done: PickDone) {
    if let Some(path) = rfd::FileDialog::new().set_title(title).pick_folder() {
        done(Ok(path));
    }
}

pub(super) fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for filter in filters {
        dialog = dialog.add_filter(filter.label, filter.extensions);
    }
    if let Some(path) = dialog.pick_file() {
        done(Ok(path));
    }
}

/// A folder picker only returns existing directories; the save dialog names a
/// new one, which the caller creates.
pub(super) fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    if let Some(path) = rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(default_name)
        .save_file()
    {
        done(Ok(path));
    }
}

pub(super) fn report_startup_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title("ShinkiEMU 启动失败")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("dev", "RoukakuEngine", "ShinkiEMU")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "SHINKI_EMU_DATA_DIRECTORY_UNAVAILABLE".into())
}

pub(super) fn program_file() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("SHINKI_EMU_EXECUTABLE_LOCATION: {error}"))
}

pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    let parent = program_file().ok()?.parent()?.to_path_buf();
    #[cfg(target_os = "macos")]
    if parent.file_name().is_some_and(|name| name == "MacOS") {
        return Some(parent.parent()?.join("Resources"));
    }
    Some(parent)
}

pub(super) fn restart() -> Result<(), String> {
    std::process::Command::new(program_file()?)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(drop)
        .map_err(|error| format!("SHINKI_EMU_RESTART_FAILED: {error}"))
}

/// Open a file or directory in the platform file manager without a shell.
pub(super) fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|error| format!("SHINKI_EMU_OPEN_PATH_FAILED: {error}"))
}
