//! OpenHarmony software presentation. The compositor owns buffer allocation;
//! Slint draws directly into mapped RGBA buffers without an intermediate copy.
use std::{cell::{Cell, RefCell}, collections::HashMap, ffi::{CStr, CString, c_char, c_void}, path::{Path, PathBuf}, rc::Rc, sync::{Arc, LazyLock, Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, Ordering}, mpsc}, time::Duration};
use slint::platform::{EventLoopProxy, Platform, WindowAdapter, WindowEvent, software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType}};
use super::{FileFilter, PickDone};

enum Event { Frame, Resize(u32, u32), Touch(f32, f32, String), Key(i32, i32), Text(String), Surface(bool), Lifecycle(bool, f32), PickResult(u32, String, String), Invoke(Box<dyn FnOnce() + Send>), Quit }
static EVENTS: OnceLock<mpsc::Sender<Event>> = OnceLock::new();
static FILES: OnceLock<PathBuf> = OnceLock::new();
static NEXT_PICK_ID: AtomicU32 = AtomicU32::new(1);
thread_local! { static PENDING_PICKERS: RefCell<HashMap<u32, PickDone>> = RefCell::new(HashMap::new()); }
struct Proxy(mpsc::Sender<Event>);
impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> { self.0.send(Event::Quit).map_err(|_| slint::EventLoopError::EventLoopTerminated) }
    fn invoke_from_event_loop(&self, event: Box<dyn FnOnce() + Send>) -> Result<(), slint::EventLoopError> { self.0.send(Event::Invoke(event)).map_err(|_| slint::EventLoopError::EventLoopTerminated) }
}
struct OhosPlatform { window: Rc<MinimalSoftwareWindow>, events: RefCell<mpsc::Receiver<Event>>, sender: mpsc::Sender<Event>, active: Cell<bool>, surface: Cell<bool> }
impl Platform for OhosPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> { Ok(self.window.clone()) }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> { Some(Box::new(Proxy(self.sender.clone()))) }
    fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
        loop {
            let timeout = slint::platform::duration_until_next_timer_update().unwrap_or(Duration::from_secs(3600));
            let event = self.events.borrow_mut().recv_timeout(timeout);
            if let Ok(event) = event {
                match event {
                    Event::Quit => break,
                    Event::Invoke(callback) => callback(),
                    Event::Resize(width, height) => { self.window.set_size(slint::PhysicalSize::new(width, height)); self.window.request_redraw(); }
                    Event::Surface(visible) => { self.surface.set(visible); self.window.request_redraw(); }
                    Event::Lifecycle(active, dpi) => { self.active.set(active); self.window.dispatch_event(WindowEvent::WindowActiveChanged(active)); if dpi.is_finite() && dpi > 0.0 { self.window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: dpi }); } }
                    Event::PickResult(id, path, error) => {
                        if let Some(done) = PENDING_PICKERS.with(|pending| pending.borrow_mut().remove(&id)) {
                            if error != "cancelled" { done(if error.is_empty() { Ok(path.into()) } else { Err(error) }); }
                        }
                    }
                    Event::Touch(x, y, kind) => {
                        let position = slint::LogicalPosition::new(x / self.window.scale_factor(), y / self.window.scale_factor());
                        let button = slint::platform::PointerEventButton::Left;
                        self.window.dispatch_event(match kind.as_str() {
                            "down" => WindowEvent::PointerPressed { position, button },
                            "up" => WindowEvent::PointerReleased { position, button },
                            "cancel" => WindowEvent::PointerExited,
                            _ => WindowEvent::PointerMoved { position },
                        });
                    }
                    Event::Text(text) => { self.window.dispatch_event(WindowEvent::KeyPressed { text: text.clone().into() }); self.window.dispatch_event(WindowEvent::KeyReleased { text: text.into() }); }
                    Event::Key(code, action) => if let Some(text) = key_text(code) { self.window.dispatch_event(if action == 0 { WindowEvent::KeyPressed { text } } else { WindowEvent::KeyReleased { text } }); },
                    Event::Frame => if self.active.get() && self.surface.get() {
                        slint::platform::update_timers_and_animations();
                        self.window.draw_if_needed(|renderer| {
                            let result = unsafe { shinki_ohos_present(draw, renderer as *const _ as *mut c_void) };
                            if result != 0 { tracing::error!(event = "shinki.emu.ohos.present_failed", result); self.window.request_redraw(); }
                        });
                    },
                }
            } else if matches!(event, Err(mpsc::RecvTimeoutError::Disconnected)) { break; }
            slint::platform::update_timers_and_animations();
        }
        Ok(())
    }
}
fn key_text(code: i32) -> Option<slint::SharedString> {
    use slint::platform::Key;
    Some(match code {
        2000..=2009 => char::from_u32((code - 2000 + 48) as u32)?.to_string().into(),
        2012 => Key::UpArrow.into(), 2013 => Key::DownArrow.into(), 2014 => Key::LeftArrow.into(), 2015 => Key::RightArrow.into(),
        2017..=2042 => char::from_u32((code - 2017 + 97) as u32)?.to_string().into(),
        2043 => ",".into(), 2044 => ".".into(), 2049 => Key::Tab.into(), 2050 => " ".into(), 2054 => Key::Return.into(),
        2055 => Key::Backspace.into(), 2056 => "`".into(), 2057 => "-".into(), 2058 => "=".into(), 2059 => "[".into(),
        2060 => "]".into(), 2061 => "\\".into(), 2062 => ";".into(), 2063 => "'".into(), 2064 => "/".into(), 2070 => Key::Escape.into(),
        _ => return None,
    })
}
unsafe extern "C" fn draw(data: *mut c_void, pixels: *mut u8, width: i32, height: i32, stride: i32) {
    if width <= 0 || height <= 0 || stride < width * 4 || stride % 4 != 0 || pixels.is_null() { return; }
    let renderer = unsafe { &*(data as *const slint::platform::software_renderer::SoftwareRenderer) };
    let pixels = unsafe { std::slice::from_raw_parts_mut(pixels.cast::<PremultipliedRgbaColor>(), stride as usize / 4 * height as usize) };
    renderer.render(pixels, stride as usize / 4);
}
unsafe extern "C" {
    fn shinki_ohos_present(draw: unsafe extern "C" fn(*mut c_void, *mut u8, i32, i32, i32), data: *mut c_void) -> i32;
    fn shinki_ohos_notify(kind: *const c_char, payload: *const c_char) -> i32;
}
static ACTIVE: AtomicBool = AtomicBool::new(true);
static SURFACE: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_event(kind: i32, x: f64, y: f64, text: *const c_char) {
    let text = if text.is_null() { String::new() } else { unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned() };
    let event = match kind {
        0 => Event::Frame,
        1 => Event::Resize(x as u32, y as u32),
        2 => Event::Touch(x as f32, y as f32, text),
        3 => Event::Key(x as i32, y as i32),
        4 => Event::Text(text),
        5 => { SURFACE.store(x != 0.0, Ordering::Release); Event::Surface(x != 0.0) },
        6 => { ACTIVE.store(x != 0.0, Ordering::Release); Event::Lifecycle(x != 0.0, y as f32) },
        _ => return,
    };
    if let Some(sender) = EVENTS.get() { let _ = sender.send(event); }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_pick_result(id: u32, path: *const c_char, error: *const c_char) {
    let path = if path.is_null() { String::new() } else { unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned() };
    let error = if error.is_null() { String::new() } else { unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned() };
    if let Some(sender) = EVENTS.get() { let _ = sender.send(Event::PickResult(id, path, error)); }
}
#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_start(files: *const c_char) {
    if files.is_null() { return; }
    let files = unsafe { CStr::from_ptr(files) }.to_string_lossy().into_owned();
    if FILES.set(files.into()).is_err() { return; }
    let (sender, receiver) = mpsc::channel();
    if EVENTS.set(sender.clone()).is_err() { return; }
    std::thread::spawn(move || {
        let platform = OhosPlatform { window: MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer), events: RefCell::new(receiver), sender, active: Cell::new(true), surface: Cell::new(false) };
        let result = slint::platform::set_platform(Box::new(platform)).map_err(|error| error.to_string()).and_then(|()| crate::manager_app::run_application().map_err(|error| error.to_string()));
        if let Err(error) = result { tracing::error!(event = "shinki.emu.ohos.startup_failed", error); let _ = notify("error", &error); }
    });
}

fn notify(kind: &str, payload: &str) -> Result<(), String> {
    let kind = CString::new(kind).map_err(|_| "SHINKI_EMU_OHOS_EVENT_KIND")?;
    let payload = CString::new(payload).map_err(|_| "SHINKI_EMU_OHOS_EVENT_PAYLOAD")?;
    let result = unsafe { shinki_ohos_notify(kind.as_ptr(), payload.as_ptr()) };
    (result == 0).then_some(()).ok_or_else(|| "SHINKI_EMU_OHOS_ARKTS_BRIDGE_UNAVAILABLE".into())
}

fn request_picker(kind: &str, title: &str, extensions: &[&str], done: PickDone) {
    let id = NEXT_PICK_ID.fetch_add(1, Ordering::Relaxed);
    PENDING_PICKERS.with(|pending| pending.borrow_mut().insert(id, done));
    let payload = serde_json::json!({ "id": id, "title": title, "extensions": extensions }).to_string();
    if let Err(error) = notify(kind, &payload) {
        if let Some(done) = PENDING_PICKERS.with(|pending| pending.borrow_mut().remove(&id)) { done(Err(error)); }
    }
}

pub(super) fn pick_folder(title: &str, done: PickDone) { request_picker("pickFolder", title, &[], done); }
pub(super) fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    let extensions = filters.iter().flat_map(|filter| filter.extensions.iter().copied()).collect::<Vec<_>>();
    request_picker("pickFile", title, &extensions, done);
}
pub(super) fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    request_picker("pickDirectory", title, &[default_name], done);
}
pub(super) fn report_startup_error(message: &str) { let _ = notify("error", message); }
pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    let path = FILES.get().ok_or("SHINKI_EMU_OHOS_CONTEXT_MISSING")?.join("ShinkiEMU");
    std::fs::create_dir_all(&path).map_err(|error| format!("SHINKI_EMU_DATA_DIRECTORY: {error}"))?;
    Ok(path)
}
pub(super) fn program_file() -> Result<PathBuf, String> {
    Ok(FILES.get().ok_or("SHINKI_EMU_OHOS_CONTEXT_MISSING")?.join("libshinki_emu_manager.so"))
}
pub(super) fn bundled_resource_dir() -> Option<PathBuf> { Some(FILES.get()?.join("resources")) }
pub(super) fn restart() -> Result<(), String> { notify("restart", "") }
pub(super) fn open_path(path: &Path) -> Result<(), String> { notify("openPath", &path.to_string_lossy()) }
static OHAUDIO_OUTPUTS: LazyLock<Mutex<Vec<Arc<Mutex<NativeAudioOutput>>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static OHOS_FRAME: OnceLock<crate::stage_renderer::FrameMailbox> = OnceLock::new();
pub(super) fn lifecycle_state() -> (bool, bool) { (ACTIVE.load(Ordering::Acquire), SURFACE.load(Ordering::Acquire)) }

struct MusicaAudioSink { output: Arc<Mutex<Option<Arc<Mutex<NativeAudioOutput>>>>> }
impl shinki_emu_sdk::runtime::AudioSink for MusicaAudioSink {
    fn configure(&self, format: shinki_emu_sdk::runtime::PcmFormatSpec) -> shinki_emu_sdk::runtime::SessionResult<()> {
        if format.sample_rate != 48_000 || format.channels != 2 {
            return Err(shinki_emu_sdk::runtime::SessionError::invalid("SHINKI_EMU_OHOS_AUDIO_FORMAT", "OHAudio output requires 48 kHz stereo PCM"));
        }
        let output = NativeAudioOutput::open().map_err(|message| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_OPEN", message))?;
        let output = Arc::new(Mutex::new(output));
        OHAUDIO_OUTPUTS.lock().map_err(|_| shinki_emu_sdk::runtime::SessionError::invalid("SHINKI_EMU_OHOS_AUDIO_LOCK", "audio output registry poisoned"))?.push(output.clone());
        *self.output.lock().map_err(|_| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_LOCK", "audio sink poisoned"))? = Some(output);
        Ok(())
    }
    fn write(&self, samples: Vec<f32>) -> shinki_emu_sdk::runtime::SessionResult<Option<Vec<f32>>> {
        let output = self.output.lock().map_err(|_| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_LOCK", "audio sink poisoned"))?.clone();
        let Some(output) = output else { return Ok(None); };
        output.lock().map_err(|_| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_LOCK", "audio output poisoned"))?
            .write(&samples).map_err(|message| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_WRITE", message))?;
        Ok(Some(samples))
    }
    fn cancel(&self) -> shinki_emu_sdk::runtime::SessionResult<()> {
        *self.output.lock().map_err(|_| shinki_emu_sdk::runtime::SessionError::new("SHINKI_EMU_OHOS_AUDIO_LOCK", "audio sink poisoned"))? = None;
        Ok(())
    }
}

pub(crate) fn musica_audio_sink() -> shinki_emu_sdk::runtime::AudioSinkBox {
    Box::new(MusicaAudioSink { output: Arc::new(Mutex::new(None)) })
}


pub(crate) fn register_frame_mailbox(mailbox: crate::stage_renderer::FrameMailbox) {
    let _ = OHOS_FRAME.set(mailbox);
}
#[repr(C)]
struct OHAudioQueue { _opaque: [u8; 0] }
#[repr(C)]
struct OHAudioRenderer { _opaque: [u8; 0] }
unsafe extern "C" {
    fn shinki_ohos_audio_open(queue: *mut *mut OHAudioQueue, renderer: *mut *mut OHAudioRenderer) -> i32;
    fn shinki_ohos_audio_write(queue: *mut OHAudioQueue, samples: *const f32, frames: u32) -> i32;
    fn shinki_ohos_audio_close(queue: *mut OHAudioQueue, renderer: *mut OHAudioRenderer);
}

pub struct NativeAudioOutput {
    queue: *mut OHAudioQueue,
    renderer: *mut OHAudioRenderer,
}
unsafe impl Send for NativeAudioOutput {}
impl NativeAudioOutput {
    pub fn open() -> Result<Self, String> {
        let mut queue = std::ptr::null_mut();
        let mut renderer = std::ptr::null_mut();
        let result = unsafe { shinki_ohos_audio_open(&mut queue, &mut renderer) };
        if result != 0 { return Err(format!("SHINKI_EMU_OHOS_AUDIO_OPEN:{result}")); }
        Ok(Self { queue, renderer })
    }
    pub fn write(&mut self, samples: &[f32]) -> Result<(), String> {
        if samples.len() % 2 != 0 { return Err("SHINKI_EMU_OHOS_AUDIO_SAMPLE_ALIGNMENT".into()); }
        let result = unsafe { shinki_ohos_audio_write(self.queue, samples.as_ptr(), (samples.len() / 2) as u32) };
        if result == 0 { Ok(()) } else { Err(format!("SHINKI_EMU_OHOS_AUDIO_QUEUE_FULL:{result}")) }
    }
}
impl Drop for NativeAudioOutput {
    fn drop(&mut self) { unsafe { shinki_ohos_audio_close(self.queue, self.renderer); } }
}

fn present_stage() {
    let Some(frame) = OHOS_FRAME.get().and_then(|mailbox| mailbox.snapshot().ok().flatten()) else { return; };
    let width = frame.info.width as usize;
    let height = frame.info.height as usize;
    let stride = width * 4;
    let Some(source) = frame.pixels() else { return; };
    let source_stride = frame.info.stride as usize;
    let mut rgba = vec![0; stride * height];
    for y in 0..height {
        let source_y = if frame.info.rows == shinki_emu_manager_core::model::FrameRowOrder::BottomToTop { height - y - 1 } else { y };
        let source_row = &source[source_y * source_stride..source_y * source_stride + width * 4];
        let dest = &mut rgba[y * stride..(y + 1) * stride];
        match frame.info.format {
            shinki_emu_manager_core::model::FrameFormat::Rgba8Srgb { .. } | shinki_emu_manager_core::model::FrameFormat::Rgba8Linear { .. } => dest.copy_from_slice(source_row),
            shinki_emu_manager_core::model::FrameFormat::Bgra8Srgb { .. } | shinki_emu_manager_core::model::FrameFormat::Bgra8Linear { .. } => {
                for px in 0..width { dest[px * 4..px * 4 + 4].copy_from_slice(&[source_row[px * 4 + 2], source_row[px * 4 + 1], source_row[px * 4], source_row[px * 4 + 3]]); }
            }
        }
    }
    let _ = unsafe { shinki_ohos_present(draw_rgba, rgba.as_mut_ptr(), width as i32, height as i32, (stride / 4) as i32) };
    if let Some(mailbox) = OHOS_FRAME.get() { let _ = mailbox.presented(frame.identity, frame.info); }
}
unsafe extern "C" fn draw_rgba(data: *mut c_void, pixels: *mut u8, width: i32, height: i32, stride: i32) {
    if width <= 0 || height <= 0 || stride < width * 4 || pixels.is_null() || data.is_null() { return; }
    let source = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), width as usize * height as usize * 4) };
    let destination = unsafe { std::slice::from_raw_parts_mut(pixels, stride as usize * height as usize) };
    for row in 0..height as usize {
        destination[row * stride as usize..row * stride as usize + width as usize * 4]
            .copy_from_slice(&source[row * width as usize * 4..(row + 1) * width as usize * 4]);
    }
}
