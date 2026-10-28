//! OpenHarmony host services.
//!
//! OpenHarmony exposes the application surface through an ArkUI `XComponent`.
//! The native bridge calls [`register_xcomponent`] when the component is created;
//! the callbacks retain only the opaque native-window address, which is converted
//! to `raw_window_handle::OhosNdkWindowHandle` by the rendering backend.
use std::{
    env,
    ffi::c_void,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::{FileFilter, PickDone};

const DEFAULT_DATA_DIR: &str = "/data/storage/el2/base/files";
static XCOMPONENT: AtomicUsize = AtomicUsize::new(0);
static NATIVE_WINDOW: AtomicUsize = AtomicUsize::new(0);
static SURFACE_READY: AtomicBool = AtomicBool::new(false);
static RESUMED: AtomicBool = AtomicBool::new(true);

#[repr(C)]
struct NativeXComponentCallback {
    on_surface_created: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    on_surface_changed: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    on_surface_destroyed: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    dispatch_touch_event: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
}

unsafe extern "C" {
    fn OH_NativeXComponent_RegisterCallback(
        component: *mut c_void,
        callback: *mut NativeXComponentCallback,
    ) -> i32;
}

unsafe extern "C" fn surface_created(_component: *mut c_void, window: *mut c_void) {
    NATIVE_WINDOW.store(window as usize, Ordering::Release);
    SURFACE_READY.store(true, Ordering::Release);
}
unsafe extern "C" fn surface_changed(_component: *mut c_void, window: *mut c_void) {
    NATIVE_WINDOW.store(window as usize, Ordering::Release);
}
unsafe extern "C" fn surface_destroyed(_component: *mut c_void, _window: *mut c_void) {
    NATIVE_WINDOW.store(0, Ordering::Release);
    SURFACE_READY.store(false, Ordering::Release);
}
unsafe extern "C" fn dispatch_touch_event(_component: *mut c_void, _window: *mut c_void) {}

static mut CALLBACK: NativeXComponentCallback = NativeXComponentCallback {
    on_surface_created: Some(surface_created),
    on_surface_changed: Some(surface_changed),
    on_surface_destroyed: Some(surface_destroyed),
    dispatch_touch_event: Some(dispatch_touch_event),
};

/// Register the native XComponent supplied by the ArkUI bridge.
///
/// The pointer is borrowed by ArkUI and remains valid until the destroy callback.
/// The function is idempotent and can be called again after a surface recreation.
pub unsafe fn register_xcomponent(component: *mut c_void) -> Result<(), String> {
    if component.is_null() {
        return Err("SHINKI_EMU_OHOS_XCOMPONENT_NULL".into());
    }
    XCOMPONENT.store(component as usize, Ordering::Release);
    // SAFETY: CALLBACK has static storage and the XComponent API takes ownership
    // of neither pointer. ArkUI invokes it on the component's UI thread.
    let result = unsafe { OH_NativeXComponent_RegisterCallback(component, &raw mut CALLBACK) };
    if result != 0 {
        XCOMPONENT.store(0, Ordering::Release);
        return Err(format!("SHINKI_EMU_OHOS_XCOMPONENT_REGISTER:{result}"));
    }
    Ok(())
}

/// Return the current OHOS native window for wgpu surface creation.
pub fn native_window() -> Option<*mut c_void> {
    let value = NATIVE_WINDOW.load(Ordering::Acquire);
    (!value.eq(&0)).then_some(value as *mut c_void)
}

/// Set the application lifecycle state from the ArkUI bridge.
pub fn set_resumed(resumed: bool) {
    RESUMED.store(resumed, Ordering::Release);
}

/// Whether the HAP currently owns a resumed surface.
pub fn is_resumed() -> bool {
    RESUMED.load(Ordering::Acquire) && SURFACE_READY.load(Ordering::Acquire)
}

pub(super) fn pick_folder(_title: &str, done: PickDone) {
    done(Err("SHINKI_EMU_OHOS_FILE_PICKER_UNAVAILABLE: use the ArkUI picker bridge".into()));
}

pub(super) fn pick_file(_title: &str, _filters: &[FileFilter<'_>], done: PickDone) {
    done(Err("SHINKI_EMU_OHOS_FILE_PICKER_UNAVAILABLE: use the ArkUI picker bridge".into()));
}

pub(super) fn pick_new_directory(_title: &str, _default_name: &str, done: PickDone) {
    done(Err("SHINKI_EMU_OHOS_FILE_PICKER_UNAVAILABLE: use the ArkUI picker bridge".into()));
}

pub(super) fn report_startup_error(message: &str) {
    eprintln!("ShinkiEMU OpenHarmony startup failed: {message}");
}

pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    let path = env::var_os("OHOS_APP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("SHINKI_EMU_OHOS_DATA_DIRECTORY:{error}"))?;
    Ok(path)
}

pub(super) fn program_file() -> Result<PathBuf, String> {
    env::current_exe().map_err(|error| format!("SHINKI_EMU_OHOS_EXECUTABLE_LOCATION:{error}"))
}

pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    env::var_os("OHOS_APP_RESOURCE_DIR")
        .map(PathBuf::from)
        .or_else(|| program_file().ok()?.parent().map(Path::to_path_buf))
}

pub(super) fn restart() -> Result<(), String> {
    Err("SHINKI_EMU_OHOS_RESTART_REQUIRES_ABILITY_MANAGER".into())
}

pub(super) fn open_path(_path: &Path) -> Result<(), String> {
    Err("SHINKI_EMU_OHOS_OPEN_PATH_REQUIRES_ARKUI_BRIDGE".into())
}

/// OHOS does not expose a filesystem keyring to native Rust; the translation
/// provider's file-backed store uses the app-private data directory.
pub(crate) fn secret_dir() -> Result<PathBuf, String> {
    let path = default_data_dir()?.join("secrets");
    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn data_dir_can_be_selected_by_host() {
        assert_eq!(Path::new(DEFAULT_DATA_DIR).is_absolute(), true);
    }
}
