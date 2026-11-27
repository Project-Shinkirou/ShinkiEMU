//! OpenHarmony host. The ArkTS shell owns the XComponent and forwards surface,
//! input, lifecycle and configuration events through the NAPI glue in
//! `Platforms/OHOS/native`. Slint runs as a custom platform on its own thread:
//! the software renderer draws straight into mapped OHNativeWindow buffers,
//! and the stage frame reaches Slint as a CPU image (see `stage_software.rs`).
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    time::Duration,
};

use shinki_emu_manager_core::model::{FamilyEvent, KeyCode, KeyModifiers, KeyState, MAX_TEXT_BYTES};
use slint::platform::{
    EventLoopProxy, Key, Platform, PointerEventButton, WindowAdapter, WindowEvent,
    software_renderer::{
        MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer,
    },
};

use super::{FileFilter, PickDone};

/// Lifecycle changes delivered to the handler installed by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// The ability moved to the foreground (`true`) or background.
    Foreground(bool),
    /// The XComponent surface was created (`true`) or destroyed.
    Surface(bool),
    /// Color mode or display density changed.
    ConfigChanged,
}

enum Event {
    Frame,
    Resize(u32, u32),
    Touch(f32, f32, TouchPhase),
    Key(i32, bool),
    Text(u64, String),
    Surface(bool),
    Foreground(bool),
    Configuration { dark: bool, scale: f32 },
    PickResult(u32, String, String),
    Invoke(Box<dyn FnOnce() + Send>),
    Quit,
}

#[derive(Clone, Copy)]
enum TouchPhase {
    Down,
    Up,
    Move,
    Cancel,
}

/// Runs before Slint renders a frame and after the frame reached the surface.
pub type FrameHook = Box<dyn FnMut()>;
/// Shared so delivery can release the slot before running the handler.
type LifecycleHandler = Rc<dyn Fn(LifecycleEvent)>;
type InputHandler = Rc<dyn Fn(Option<u64>, FamilyEvent) -> bool>;

struct AbilityDirectories {
    files: Arc<str>,
    cache: Arc<str>,
}

static EVENTS: OnceLock<mpsc::Sender<Event>> = OnceLock::new();
static DIRECTORIES: OnceLock<AbilityDirectories> = OnceLock::new();
static NEXT_PICK_ID: AtomicU32 = AtomicU32::new(1);
static FOREGROUND: AtomicBool = AtomicBool::new(true);
static SURFACE: AtomicBool = AtomicBool::new(false);
static DARK: AtomicBool = AtomicBool::new(false);

thread_local! {
    static PENDING_PICKERS: RefCell<HashMap<u32, PickDone>> = RefCell::new(HashMap::new());
    static LIFECYCLE_HANDLER: RefCell<Option<LifecycleHandler>> = RefCell::new(None);
    static FRAME_HOOKS: RefCell<Option<(FrameHook, FrameHook)>> = RefCell::new(None);
    static INPUT_HANDLER: RefCell<Option<InputHandler>> = RefCell::new(None);
    static TEXT_TARGET: Cell<Option<u64>> = const { Cell::new(None) };
}

unsafe extern "C" {
    fn shinki_ohos_present(
        draw: unsafe extern "C" fn(*mut c_void, *mut u8, i32, i32, i32),
        data: *mut c_void,
    ) -> i32;
    fn shinki_ohos_request_frame();
    fn shinki_ohos_notify(kind: *const c_char, payload: *const c_char) -> i32;
}

struct Proxy(mpsc::Sender<Event>);

impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.0
            .send(Event::Quit)
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }

    fn invoke_from_event_loop(
        &self,
        event: Box<dyn FnOnce() + Send>,
    ) -> Result<(), slint::EventLoopError> {
        self.0
            .send(Event::Invoke(event))
            .map_err(|_| slint::EventLoopError::EventLoopTerminated)
    }
}

struct OhosPlatform {
    window: Rc<MinimalSoftwareWindow>,
    events: mpsc::Receiver<Event>,
    sender: mpsc::Sender<Event>,
}

impl Platform for OhosPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy(self.sender.clone())))
    }

    fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
        loop {
            slint::platform::update_timers_and_animations();
            let timeout = slint::platform::duration_until_next_timer_update()
                .unwrap_or(Duration::from_secs(3600));
            match self.events.recv_timeout(timeout) {
                Ok(Event::Quit) => return Ok(()),
                Ok(Event::Frame) => {
                    if self.frame() {
                        // Keep the vsync chain alive while frames keep changing;
                        // an idle Manager stops requesting callbacks.
                        request_frame();
                    }
                    continue;
                }
                Ok(event) => self.dispatch(event),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
            // Events and timers may have requested a redraw; the next vsync
            // draws only when Slint marked the window dirty.
            request_frame();
        }
    }
}

impl OhosPlatform {
    fn frame(&self) -> bool {
        if !FOREGROUND.load(Ordering::Acquire) || !SURFACE.load(Ordering::Acquire) {
            return false;
        }
        slint::platform::update_timers_and_animations();
        let drawn = self.window.draw_if_needed(|renderer| {
            run_hook(0);
            let result = unsafe {
                shinki_ohos_present(draw, std::ptr::from_ref(renderer).cast_mut().cast())
            };
            if result != 0 {
                tracing::error!(
                    event = "shinki.emu.ohos.present_failed",
                    diagnostic_code = "SHINKI_EMU_OHOS_PRESENT",
                    result
                );
                self.window.request_redraw();
            }
        });
        if drawn {
            run_hook(1);
        }
        drawn || self.window.has_active_animations()
    }

    fn dispatch(&self, event: Event) {
        match event {
            Event::Frame | Event::Quit => {}
            Event::Invoke(callback) => callback(),
            Event::Resize(width, height) => {
                self.window
                    .set_size(slint::PhysicalSize::new(width, height));
                self.window.request_redraw();
                lifecycle(LifecycleEvent::ConfigChanged);
            }
            Event::Surface(present) => {
                SURFACE.store(present, Ordering::Release);
                if !present {
                    set_text_target(None);
                }
                self.window.request_redraw();
                lifecycle(LifecycleEvent::Surface(present));
            }
            Event::Foreground(active) => {
                FOREGROUND.store(active, Ordering::Release);
                if !active {
                    set_text_target(None);
                }
                self.window
                    .dispatch_event(WindowEvent::WindowActiveChanged(active));
                self.window.request_redraw();
                lifecycle(LifecycleEvent::Foreground(active));
            }
            Event::Configuration { dark, scale } => {
                DARK.store(dark, Ordering::Release);
                if scale.is_finite() && scale > 0.0 {
                    self.window.dispatch_event(WindowEvent::ScaleFactorChanged {
                        scale_factor: scale,
                    });
                }
                self.window.request_redraw();
                lifecycle(LifecycleEvent::ConfigChanged);
            }
            Event::PickResult(id, path, error) => {
                let done = PENDING_PICKERS.with(|pending| pending.borrow_mut().remove(&id));
                if let Some(done) = done
                    && error != "cancelled"
                {
                    done(if error.is_empty() {
                        Ok(path.into())
                    } else {
                        Err(error)
                    });
                }
            }
            Event::Touch(x, y, phase) => {
                let scale = self.window.scale_factor();
                let position = slint::LogicalPosition::new(x / scale, y / scale);
                let button = PointerEventButton::Left;
                self.window.dispatch_event(match phase {
                    TouchPhase::Down => WindowEvent::PointerPressed { position, button },
                    TouchPhase::Up => WindowEvent::PointerReleased { position, button },
                    TouchPhase::Move => WindowEvent::PointerMoved { position },
                    TouchPhase::Cancel => WindowEvent::PointerExited,
                });
            }
            Event::Key(code, pressed) => {
                if lifecycle_state() == (true, true)
                    && let Some(code) = physical_key(code)
                    && dispatch_runtime_input(
                        None,
                        FamilyEvent::Key {
                            code,
                            logical: None,
                            state: if pressed { KeyState::Pressed } else { KeyState::Released },
                            modifiers: KeyModifiers {
                                shift: false,
                                control: false,
                                alt: false,
                                super_key: false,
                            },
                        },
                    )
                {
                    return;
                }
                if let Some(text) = key_text(code) {
                    self.window.dispatch_event(if pressed {
                        WindowEvent::KeyPressed { text }
                    } else {
                        WindowEvent::KeyReleased { text }
                    });
                }
            }
            Event::Text(epoch, text) => {
                if TEXT_TARGET.get() == Some(epoch) {
                    dispatch_runtime_input(Some(epoch), FamilyEvent::TextInput { text });
                }
            }
        }
    }
}

fn run_hook(index: usize) {
    // Take the hooks out so a hook may re-enter Slint without a borrow held.
    let hooks = FRAME_HOOKS.with(|slot| slot.borrow_mut().take());
    if let Some(mut hooks) = hooks {
        if index == 0 {
            (hooks.0)();
        } else {
            (hooks.1)();
        }
        FRAME_HOOKS.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(hooks);
            }
        });
    }
}

fn lifecycle(event: LifecycleEvent) {
    // Clone out of the slot so the handler may replace itself.
    let handler = LIFECYCLE_HANDLER.with(|slot| slot.borrow().clone());
    if let Some(handler) = handler {
        handler(event);
    }
}

fn request_frame() {
    unsafe { shinki_ohos_request_frame() };
}

/// OH_NativeXComponent_KeyCode values to Slint key text.
fn key_text(code: i32) -> Option<slint::SharedString> {
    Some(match code {
        2000..=2009 => char::from_u32((code - 2000 + 48) as u32)?
            .to_string()
            .into(),
        2012 => Key::UpArrow.into(),
        2013 => Key::DownArrow.into(),
        2014 => Key::LeftArrow.into(),
        2015 => Key::RightArrow.into(),
        2017..=2042 => char::from_u32((code - 2017 + 97) as u32)?
            .to_string()
            .into(),
        2043 => ",".into(),
        2044 => ".".into(),
        2049 => Key::Tab.into(),
        2050 => " ".into(),
        2054 => Key::Return.into(),
        2055 => Key::Backspace.into(),
        2056 => "`".into(),
        2057 => "-".into(),
        2058 => "=".into(),
        2059 => "[".into(),
        2060 => "]".into(),
        2061 => "\\".into(),
        2062 => ";".into(),
        2063 => "'".into(),
        2064 => "/".into(),
        // KEY_ESCAPE, and KEY_BACK which the ArkTS shell forwards for the system back gesture.
        2 | 2070 => Key::Escape.into(),
        _ => return None,
    })
}

/// Native XComponent key codes. Back remains on its existing Slint route.
fn physical_key(code: i32) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match code {
        2000..=2009 => {
            [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9]
                [(code - 2000) as usize]
        }
        2012 => ArrowUp,
        2013 => ArrowDown,
        2014 => ArrowLeft,
        2015 => ArrowRight,
        2017..=2042 => [
            A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        ][(code - 2017) as usize],
        2043 => Comma,
        2044 => Period,
        2045 => AltLeft,
        2046 => AltRight,
        2047 => ShiftLeft,
        2048 => ShiftRight,
        2049 => Tab,
        2050 => Space,
        2054 => Enter,
        2055 => Backspace,
        2056 => Grave,
        2057 => Minus,
        2058 => Equals,
        2059 => BracketLeft,
        2060 => BracketRight,
        2061 => Backslash,
        2062 => Semicolon,
        2063 => Apostrophe,
        2064 => Slash,
        2068 => PageUp,
        2069 => PageDown,
        2070 => Escape,
        2071 => Delete,
        2072 => ControlLeft,
        2073 => ControlRight,
        2081 => Home,
        2082 => End,
        2083 => Insert,
        2090..=2101 => [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12][(code - 2090) as usize],
        2103..=2112 => [
            Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8,
            Numpad9,
        ][(code - 2103) as usize],
        2113 => NumpadDivide,
        2114 => NumpadMultiply,
        2115 => NumpadSubtract,
        2116 => NumpadAdd,
        2119 => NumpadEnter,
        _ => return None,
    })
}

unsafe extern "C" fn draw(
    data: *mut c_void,
    pixels: *mut u8,
    width: i32,
    height: i32,
    stride: i32,
) {
    if data.is_null()
        || pixels.is_null()
        || width <= 0
        || height <= 0
        || stride < width * 4
        || stride % 4 != 0
    {
        return;
    }
    let renderer = unsafe { &*data.cast::<SoftwareRenderer>() };
    let line = stride as usize / 4;
    let pixels = unsafe {
        std::slice::from_raw_parts_mut(
            pixels.cast::<PremultipliedRgbaColor>(),
            line * height as usize,
        )
    };
    renderer.render(pixels, line);
}

fn send(event: Event) {
    if let Some(sender) = EVENTS.get() {
        let _ = sender.send(event);
    }
}

fn c_string(value: *const c_char) -> String {
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Native glue entry: `kind` selects the event, `x`/`y`/`text` carry its values.
#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_event(kind: i32, x: f64, y: f64, text: *const c_char) {
    let event = match kind {
        0 => Event::Frame,
        1 => Event::Resize(x as u32, y as u32),
        2 => Event::Touch(
            x as f32,
            y as f32,
            match c_string(text).as_str() {
                "down" => TouchPhase::Down,
                "up" => TouchPhase::Up,
                "cancel" => TouchPhase::Cancel,
                _ => TouchPhase::Move,
            },
        ),
        3 => Event::Key(x as i32, y == 0.0),
        5 => Event::Surface(x != 0.0),
        6 => Event::Foreground(x != 0.0),
        7 => Event::Configuration {
            dark: x != 0.0,
            scale: y as f32,
        },
        _ => return,
    };
    send(event);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_submit_text(epoch: *const c_char, text: *const u8, length: usize) {
    if epoch.is_null() || text.is_null() || !(1..=MAX_TEXT_BYTES).contains(&length) {
        return;
    }
    let epoch = unsafe { CStr::from_ptr(epoch) }.to_str().ok().and_then(|value| value.parse().ok());
    let text = std::str::from_utf8(unsafe { std::slice::from_raw_parts(text, length) });
    if let (Some(epoch), Ok(text)) = (epoch, text)
        && !text.contains('\0')
    {
        send(Event::Text(epoch, text.to_owned()));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_pick_result(id: u32, path: *const c_char, error: *const c_char) {
    send(Event::PickResult(id, c_string(path), c_string(error)));
}

/// Start the Manager once per process with the Ability's private data and
/// cache directories. The application runs on a dedicated Slint thread.
///
/// # Safety
/// `files` and `cache` are NUL-terminated strings valid for this call.
pub unsafe fn start(
    files: *const c_char,
    cache: *const c_char,
    stack_bytes: usize,
    run: fn() -> Result<(), Box<dyn std::error::Error>>,
) {
    if DIRECTORIES.get().is_some() {
        return;
    }
    let parse = |value: *const c_char| {
        if value.is_null() {
            return None;
        }
        let path = unsafe { CStr::from_ptr(value) }.to_str().ok()?;
        (path.len() <= shinki_emu_abi::MAX_HOST_DIRECTORY_BYTES
            && Path::new(path).is_absolute()
            && Path::new(path).is_dir())
            .then(|| Arc::<str>::from(path))
    };
    let (Some(files), Some(cache)) = (parse(files), parse(cache)) else {
        report_startup_error("SHINKI_EMU_OHOS_CONTEXT_PATH: invalid Ability directory");
        return;
    };
    if let Err(error) = tempfile::tempfile_in(Path::new(cache.as_ref())) {
        report_startup_error(&format!("SHINKI_EMU_OHOS_CACHE: {error}"));
        return;
    }
    if DIRECTORIES.set(AbilityDirectories { files, cache }).is_err() {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    if EVENTS.set(sender.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("shinki-emu-manager".into())
        .stack_size(stack_bytes)
        .spawn(move || {
            let platform = OhosPlatform {
                window: MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer),
                events: receiver,
                sender,
            };
            let result = slint::platform::set_platform(Box::new(platform))
                .map_err(|error| error.to_string())
                .and_then(|()| run().map_err(|error| error.to_string()));
            if let Err(error) = result {
                tracing::error!(
                    event = "shinki.emu.ohos.startup_failed",
                    diagnostic_code = "SHINKI_EMU_OHOS_STARTUP",
                    error = %error
                );
                report_startup_error(&error);
            }
            let _ = notify("exit", "");
        });
    if let Err(error) = spawned {
        report_startup_error(&format!("SHINKI_EMU_OHOS_THREAD: {error}"));
    }
}

/// Install the frame hooks, replacing previous ones. Must be called on the
/// Slint event-loop thread, where frames are drawn.
pub fn set_frame_hooks(before: FrameHook, after: FrameHook) {
    FRAME_HOOKS.with(|slot| *slot.borrow_mut() = Some((before, after)));
}

/// Register the lifecycle handler, replacing the previous one. Must be called
/// on the Slint event-loop thread, where events are delivered.
pub fn set_lifecycle_handler(handler: Box<dyn Fn(LifecycleEvent)>) {
    let handler: LifecycleHandler = Rc::from(handler);
    LIFECYCLE_HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
}

pub(crate) fn set_input_handler(handler: impl Fn(Option<u64>, FamilyEvent) -> bool + 'static) {
    INPUT_HANDLER.with(|slot| *slot.borrow_mut() = Some(Rc::new(handler)));
}

fn dispatch_runtime_input(epoch: Option<u64>, event: FamilyEvent) -> bool {
    let handler = INPUT_HANDLER.with(|slot| slot.borrow().clone());
    handler.is_some_and(|handler| handler(epoch, event))
}

pub(crate) fn set_text_target(epoch: Option<u64>) {
    let epoch = match lifecycle_state() {
        (true, true) => epoch,
        _ => None,
    };
    if TEXT_TARGET.replace(epoch) != epoch {
        let payload = epoch.map(|value| value.to_string()).unwrap_or_default();
        if let Err(error) = notify("textTarget", &payload) {
            tracing::warn!(event = "shinki.emu.ohos.text_target_failed", %error);
        }
    }
}

/// `(foreground, surface)` as last reported by the ArkTS shell.
pub fn lifecycle_state() -> (bool, bool) {
    (
        FOREGROUND.load(Ordering::Acquire),
        SURFACE.load(Ordering::Acquire),
    )
}

/// Whether the system color mode is dark.
pub fn system_dark() -> bool {
    DARK.load(Ordering::Acquire)
}

/// Hide or show the mouse pointer over the Manager window
/// (`@ohos.multimodalInput.pointer` in the ArkTS shell).
pub fn set_pointer_hidden(hidden: bool) -> Result<(), String> {
    notify("pointerVisible", if hidden { "0" } else { "1" })
}

fn notify(kind: &str, payload: &str) -> Result<(), String> {
    let kind = CString::new(kind).map_err(|_| "SHINKI_EMU_OHOS_EVENT_KIND")?;
    let payload = CString::new(payload).map_err(|_| "SHINKI_EMU_OHOS_EVENT_PAYLOAD")?;
    let result = unsafe { shinki_ohos_notify(kind.as_ptr(), payload.as_ptr()) };
    if result == 0 {
        Ok(())
    } else {
        Err("SHINKI_EMU_OHOS_ARKTS_BRIDGE_UNAVAILABLE".into())
    }
}

fn request_picker(
    kind: &str,
    title: &str,
    extensions: &[&str],
    default_name: &str,
    done: PickDone,
) {
    let id = NEXT_PICK_ID.fetch_add(1, Ordering::Relaxed);
    PENDING_PICKERS.with(|pending| pending.borrow_mut().insert(id, done));
    let payload = serde_json::json!({
        "id": id,
        "title": title,
        "extensions": extensions,
        "defaultName": default_name,
    })
    .to_string();
    if let Err(error) = notify(kind, &payload)
        && let Some(done) = PENDING_PICKERS.with(|pending| pending.borrow_mut().remove(&id))
    {
        done(Err(error));
    }
}

pub(super) fn pick_folder(title: &str, done: PickDone) {
    request_picker("pickFolder", title, &[], "", done);
}

pub(super) fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    let extensions = filters
        .iter()
        .flat_map(|filter| filter.extensions.iter().copied())
        .collect::<Vec<_>>();
    request_picker("pickFile", title, &extensions, "", done);
}

pub(super) fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    request_picker("pickNewDirectory", title, &[], default_name, done);
}

pub(super) fn report_startup_error(message: &str) {
    let _ = notify("error", message);
}

pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    Ok(files_directory()?.join("ShinkiEMU"))
}

/// A location for host data discovery, not an installed native-library path.
pub(super) fn program_file() -> Result<PathBuf, String> {
    Ok(files_directory()?.join("libshinki_emu_manager.so"))
}

fn files_directory() -> Result<&'static Path, String> {
    DIRECTORIES
        .get()
        .map(|directories| Path::new(directories.files.as_ref()))
        .ok_or_else(|| "SHINKI_EMU_OHOS_CONTEXT_MISSING".into())
}

/// Rawfile resources the ArkTS shell extracts before starting the Manager.
pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    Some(files_directory().ok()?.join("bundled"))
}

pub mod bundled {
    use super::{CStr, CString, Path, PathBuf, c_void};
    use crate::platform::ohos_bundle_path;
    use shinki_emu_manager_core::LoadedFamilyPlugin;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    const CORE: Option<(&str, &str)> = include!(concat!(env!("OUT_DIR"), "/ohos-hap-core.rs"));
    const BUNDLE_CODE: &str = "/data/storage/el1/bundle";
    const ARCHIVE_LIBS: &str = concat!("libs/", env!("SHINKI_EMU_OHOS_ABI"), "/");

    pub fn configured_core() -> Option<(&'static str, &'static str)> {
        CORE
    }

    pub fn manifest_dir() -> Option<PathBuf> {
        let (id, _) = CORE?;
        Some(super::bundled_resource_dir()?.join("cores").join(id))
    }

    fn image_path(address: *const c_void) -> Option<PathBuf> {
        let mut image = std::mem::MaybeUninit::<libc::Dl_info>::uninit();
        if unsafe { libc::dladdr(address, image.as_mut_ptr()) } == 0 {
            return None;
        }
        let name = unsafe { image.assume_init() }.dli_fname;
        if name.is_null() {
            return None;
        }
        Some(
            std::ffi::OsString::from_vec(unsafe { CStr::from_ptr(name) }.to_bytes().to_vec())
                .into(),
        )
    }

    /// Resolve only the configured companion beside the installed Manager.
    /// Discovery calls this without dlopen.
    pub fn library(plugin_id: &str) -> Option<PathBuf> {
        let (_, basename) = CORE.filter(|(id, _)| *id == plugin_id)?;
        let manager = image_path(library as *const c_void)?;
        ohos_bundle_path::resolve(Path::new(BUNDLE_CODE), &manager, ARCHIVE_LIBS, basename)
    }

    pub fn load(plugin_id: &str) -> Result<LoadedFamilyPlugin, String> {
        let directories = super::DIRECTORIES.get().ok_or("SHINKI_EMU_OHOS_CONTEXT_MISSING")?;
        let path = library(plugin_id).ok_or("SHINKI_EMU_OHOS_BUNDLED_CORE_ORIGIN")?;
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "SHINKI_EMU_OHOS_BUNDLED_CORE_ORIGIN")?;
        // An absolute installed path skips basename search (env, RPATH and
        // inherited namespaces). The caller's existing namespace still applies.
        tracing::info!(event = "shinki.emu.ohos.core_open", plugin_id, path = %path.display());
        let module = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if module.is_null() {
            let error = unsafe { libc::dlerror() };
            if error.is_null() {
                return Err("SHINKI_EMU_OHOS_BUNDLED_CORE_OPEN".into());
            }
            let error = unsafe { CStr::from_ptr(error) };
            return Err(format!(
                "SHINKI_EMU_OHOS_BUNDLED_CORE_OPEN: {}",
                error.to_string_lossy()
            ));
        }
        // As in LoadedFamilyPlugin::load, the image is resident through process
        // exit, including when ABI initialization or descriptor checks fail.
        let symbol = unsafe { libc::dlsym(module, shinki_emu_abi::QUERY_SYMBOL.as_ptr().cast()) };
        if symbol.is_null() || image_path(symbol).as_deref() != Some(path.as_path()) {
            return Err("SHINKI_EMU_OHOS_BUNDLED_CORE_QUERY_ORIGIN".into());
        }
        let query = unsafe { std::mem::transmute::<*mut c_void, shinki_emu_abi::Query>(symbol) };
        let provider = unsafe {
            LoadedFamilyPlugin::from_query_with_directories(
                query,
                directories.cache.clone(),
                directories.files.clone(),
            )
        }
        .map_err(|error| error.to_string())?;
        if provider.manager_descriptor().plugin_id != plugin_id {
            return Err("SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH".into());
        }
        tracing::info!(
            event = "shinki.emu.ohos.core_abi_ready",
            plugin_id,
            abi = shinki_emu_abi::ABI_VERSION,
            path = %path.display()
        );
        Ok(provider)
    }
}

pub(super) fn restart() -> Result<(), String> {
    notify("restart", "")
}

pub(super) fn open_path(path: &Path) -> Result<(), String> {
    notify("openPath", &path.to_string_lossy())
}

pub(super) fn open_url(url: &str) -> Result<(), String> {
    notify("openUrl", url)
}
