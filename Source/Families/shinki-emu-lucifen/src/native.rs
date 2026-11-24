use crate::{Host, Step, Surface};
use shinki_emu_abi as abi;
use shinki_emu_lucifen_formats::FORMAT_ID;
use shinki_emu_sdk::{
    StageCanvas,
    native::{self, NativeFailure, ServiceContext},
    runtime::{ConfigValue, RuntimeEvent},
};
use std::{
    cell::RefCell,
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const PLUGIN_ID: &str = "shinki.emu.lucifen";
const FORMATS: &[abi::Bytes] = &[abi::Bytes::from_static(FORMAT_ID.as_bytes())];
const TEXT_ENCODING: &str = "text_encoding";
const HEADLESS_AUDIO_DEVICE: &str = "headless_audio_device";
const NULL_AUDIO_DEVICE: &str = "nullaudiodevice";
const AUDIO_DEVICES: &[abi::Bytes] = &[
    abi::Bytes::from_static(b"native"),
    abi::Bytes::from_static(NULL_AUDIO_DEVICE.as_bytes()),
];
const AUDIO_DEVICE_LABELS: &[abi::Bytes] = &[
    abi::Bytes::from_static(b"Native audio"),
    abi::Bytes::from_static(b"Clocked null audio (test only)"),
];
const CAPABILITIES: u64 = abi::CAP_CPU_SURFACE | abi::CAP_NATIVE_EMBEDDED_TEXT;
static INITIALIZED: AtomicBool = AtomicBool::new(false);

const CONFIGURATION: &[abi::ConfigField] = &[
    abi::ConfigField {
        id: abi::Bytes::from_static(TEXT_ENCODING.as_bytes()),
        label: abi::Bytes::from_static(b"Script text encoding"),
        group: abi::Bytes::from_static(b"Text"),
        default_value: abi::ConfigValue {
            kind: abi::VALUE_STRING,
            reserved: 0,
            integer: 0,
            number: 0.0,
            text: abi::Bytes::from_static(b"cp932"),
        },
        integer_min: 0,
        integer_max: 0,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 16,
        flags: 0,
        choices: std::ptr::null(),
        choice_count: 0,
        description: abi::Bytes::from_static(b"cp932 or gbk"),
        choice_labels: std::ptr::null(),
        step: 0.0,
        unit: abi::Bytes::EMPTY,
    },
    abi::ConfigField {
        id: abi::Bytes::from_static(HEADLESS_AUDIO_DEVICE.as_bytes()),
        label: abi::Bytes::from_static(b"Headless audio device"),
        group: abi::Bytes::from_static(b"Diagnostics"),
        default_value: abi::ConfigValue {
            kind: abi::VALUE_ENUM,
            reserved: 0,
            integer: 0,
            number: 0.0,
            text: AUDIO_DEVICES[0],
        },
        integer_min: 0,
        integer_max: 0,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 0,
        flags: abi::CONFIG_FLAG_ADVANCED,
        choices: AUDIO_DEVICES.as_ptr(),
        choice_count: AUDIO_DEVICES.len(),
        description: abi::Bytes::from_static(b"Use a clocked PCM sink for explicit audio tests."),
        choice_labels: AUDIO_DEVICE_LABELS.as_ptr(),
        step: 0.0,
        unit: abi::Bytes::EMPTY,
    },
];

fn descriptor() -> abi::Descriptor {
    abi::Descriptor {
        header: abi::Header::of::<abi::Descriptor>(),
        family_id: abi::Bytes::from_static(b"lucifen"),
        plugin_id: abi::Bytes::from_static(PLUGIN_ID.as_bytes()),
        version: abi::Bytes::from_static(env!("CARGO_PKG_VERSION").as_bytes()),
        capabilities: CAPABILITIES,
        formats: FORMATS.as_ptr(),
        format_count: FORMATS.len(),
        configuration: CONFIGURATION.as_ptr(),
        configuration_count: CONFIGURATION.len(),
        display_name: abi::Bytes::from_static(b"Shinki Lucifen"),
        family_name: abi::Bytes::from_static(b"Lucifen"),
        render: abi::RenderExtentSupport::default(),
    }
}

static API: abi::CoreApi = abi::CoreApi {
    header: abi::Header::of::<abi::CoreApi>(),
    initialize: Some(initialize),
    describe: Some(describe),
    probe: Some(probe),
    open: Some(open),
    input: Some(input),
    control: Some(control),
    state: Some(state),
    frame: Some(frame),
    cancel: Some(cancel),
    close: Some(close),
    extension: Some(extension),
};

/// # Safety
/// The output must be null or writable for one pointer.
pub unsafe extern "C" fn lucifen_query(
    version: u32,
    output: *mut *const abi::CoreApi,
) -> abi::Status {
    if output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    unsafe { output.write(std::ptr::null()) };
    if version != abi::ABI_VERSION {
        return abi::UNSUPPORTED;
    }
    unsafe { output.write(&API) };
    abi::OK
}

/// # Safety
/// The output must be null or writable for one pointer.
#[cfg(feature = "dynamic-plugin-export")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(
    version: u32,
    output: *mut *const abi::CoreApi,
) -> abi::Status {
    unsafe { lucifen_query(version, output) }
}

fn failure(status: abi::Status) -> NativeFailure {
    NativeFailure::new(
        status,
        "SHINKI_EMU_LUCIFEN_ABI",
        "Invalid Lucifen host operation",
    )
}

fn runtime_error(error: std::io::Error) -> NativeFailure {
    NativeFailure::new(
        abi::CORE_ERROR,
        "SHINKI_EMU_LUCIFEN_RUNTIME",
        error.to_string(),
    )
}

fn boundary(
    error: *mut abi::Error,
    operation: impl FnOnce() -> Result<(), NativeFailure>,
) -> abi::Status {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).unwrap_or_else(|_| {
            Err(NativeFailure::new(
                abi::PANICKED,
                "SHINKI_EMU_LUCIFEN_PANIC",
                "Lucifen core panicked",
            ))
        });
    match result {
        Ok(()) => {
            if !error.is_null() {
                unsafe { error.write(abi::Error::default()) };
            }
            abi::OK
        }
        Err(cause) => {
            if !error.is_null() {
                unsafe { error.write(cause.diagnostic()) };
            }
            cause.status
        }
    }
}

fn initialized() -> Result<(), NativeFailure> {
    if INITIALIZED.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(failure(abi::INVALID_STATE))
    }
}

unsafe extern "C" fn initialize(
    diagnostics: *const abi::Diagnostics,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let diagnostics =
            unsafe { diagnostics.as_ref() }.ok_or_else(|| failure(abi::INVALID_ARGUMENT))?;
        if !diagnostics.header.accepts::<abi::Diagnostics>() || diagnostics.emit.is_none() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        unsafe { abi::diagnostic_bridge::install(*diagnostics) }.map_err(failure)?;
        INITIALIZED.store(true, Ordering::Release);
        Ok(())
    })
}

unsafe extern "C" fn describe(output: *mut abi::Descriptor, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        initialized()?;
        if output.is_null() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        unsafe { output.write(descriptor()) };
        Ok(())
    })
}

thread_local! { static GAME_ID: RefCell<String> = const { RefCell::new(String::new()) }; }

unsafe extern "C" fn probe(
    path: abi::Bytes,
    output: *mut abi::ProbeResult,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        initialized()?;
        if output.is_null() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        let path = Path::new(unsafe { path.text(32768) }.map_err(failure)?);
        if Host::open(path).is_err() {
            return Err(NativeFailure::new(
                abi::NO_MATCH,
                "SHINKI_EMU_LUCIFEN_NO_MATCH",
                "No Lucifen script archive with a valid startup module",
            ));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| failure(abi::INVALID_ARGUMENT))?;
        GAME_ID.with_borrow_mut(|id| {
            id.clear();
            id.push_str(name);
            unsafe {
                output.write(abi::ProbeResult {
                    game_id: abi::Bytes::new(id.as_bytes()),
                    format: FORMATS[0],
                    confidence_permyriad: 9500,
                    reserved: 0,
                });
            }
        });
        Ok(())
    })
}

struct Published {
    pixels: Arc<Surface>,
    generation: u64,
    time_ns: u64,
}

fn surface_info(width: u32, height: u32) -> abi::SurfaceInfo {
    let extent = abi::Extent { width, height };
    abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_IGNORED,
        rows: abi::ROWS_TOP_DOWN,
        logical: extent,
        raster: extent,
        stride: u64::from(extent.width) * 4,
    }
}

struct Shared {
    services: ServiceContext,
    audio_stop: Arc<AtomicBool>,
    published: Mutex<Option<Published>>,
    surface: OnceLock<abi::SurfaceInfo>,
    leases: Arc<AtomicUsize>,
    state: Mutex<abi::SessionState>,
}

enum Command {
    Control(u32),
    Input(Vec<RuntimeEvent>),
}

struct Session {
    shared: Arc<Shared>,
    commands: SyncSender<Command>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.services.cancel();
        self.shared.audio_stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

unsafe fn borrow_session<'a>(session: abi::Session) -> Result<&'a Session, NativeFailure> {
    unsafe { session.cast::<Session>().as_ref() }.ok_or_else(|| failure(abi::INVALID_ARGUMENT))
}

unsafe extern "C" fn open(
    request: *const abi::OpenRequest,
    output: *mut abi::OpenResult,
    error: *mut abi::Error,
) -> abi::Status {
    if !output.is_null() {
        unsafe { (*output).session = std::ptr::null_mut() };
    }
    boundary(error, || {
        initialized()?;
        let request = unsafe { request.as_ref() }.ok_or_else(|| failure(abi::INVALID_ARGUMENT))?;
        if output.is_null()
            || !request.header.accepts::<abi::OpenRequest>()
            || !request.host.header.accepts::<abi::HostServices>()
            || request.session_epoch == 0
            || request.host.cancellation.is_cancelled.is_none()
        {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        abi::RenderExtentSupport::default()
            .validate_request(CAPABILITIES, request.render_extent)
            .map_err(failure)?;
        let configuration =
            unsafe { native::config_entries(descriptor(), request) }.map_err(failure)?;
        let mut encoding = None;
        let mut null_audio = false;
        for entry in configuration {
            match (entry.id.as_str(), entry.value) {
                (TEXT_ENCODING, ConfigValue::String(value)) => encoding = Some(value),
                (HEADLESS_AUDIO_DEVICE, ConfigValue::Enum(device)) => {
                    null_audio = device == NULL_AUDIO_DEVICE;
                }
                _ => {}
            }
        }
        if !matches!(encoding.as_deref(), None | Some("cp932" | "gbk")) {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        let root = PathBuf::from(unsafe { request.game_path.text(32768) }.map_err(failure)?);
        let shared = Arc::new(Shared {
            services: ServiceContext {
                session: request.session_epoch,
                epoch: AtomicU64::new(1),
                cancelled: AtomicBool::new(false),
                waiting: AtomicBool::new(false),
                text: None,
                window: None,
                host_cancel: Some(request.host.cancellation),
            },
            audio_stop: Arc::new(AtomicBool::new(false)),
            published: Mutex::default(),
            surface: OnceLock::new(),
            leases: Arc::new(AtomicUsize::new(0)),
            state: Mutex::new(abi::SessionState {
                state: abi::SESSION_RUNNING,
                flags: 0,
                context_epoch: 1,
                output_epoch: 1,
                error: abi::Error::default(),
            }),
        });
        let (ready, result) = mpsc::sync_channel(1);
        let (commands, receiver) = mpsc::sync_channel(64);
        let worker_state = shared.clone();
        let worker = thread::Builder::new()
            .name("lucifen".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        &root,
                        encoding.as_deref().unwrap_or("cp932"),
                        null_audio,
                        &worker_state,
                        &ready,
                        receiver,
                    )
                }))
                .unwrap_or_else(|_| Err(failure(abi::PANICKED)));
                if let Err(cause) = result {
                    let _ = ready.try_send(Err(cause.clone()));
                    if let Ok(mut state) = worker_state.state.lock() {
                        state.state = abi::SESSION_FAILED;
                        state.error = cause.diagnostic();
                    }
                } else if let Ok(mut state) = worker_state.state.lock() {
                    state.state = abi::SESSION_FINISHED;
                }
                worker_state.services.invalidate();
            })
            .map_err(runtime_error)?;
        let session = Box::new(Session {
            shared,
            commands,
            worker: Some(worker),
        });
        let surface = loop {
            if session.shared.services.is_cancelled() {
                return Err(failure(abi::CANCELLED));
            }
            match result.recv_timeout(Duration::from_millis(10)) {
                Ok(info) => break info?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(failure(abi::CORE_ERROR)),
            }
        };
        unsafe {
            output.write(abi::OpenResult {
                session: Box::into_raw(session).cast(),
                context_epoch: 1,
                output_epoch: 1,
                surface,
            })
        };
        Ok(())
    })
}

fn run(
    root: &Path,
    encoding: &str,
    null_audio: bool,
    shared: &Shared,
    ready: &SyncSender<Result<abi::SurfaceInfo, NativeFailure>>,
    commands: Receiver<Command>,
) -> Result<(), NativeFailure> {
    let mut host = Host::open(root).map_err(runtime_error)?;
    host.set_text_encoding(encoding).map_err(runtime_error)?;
    host.configure_audio(shared.audio_stop.clone(), null_audio);
    let (width, height) = host.display_size();
    let info = surface_info(width, height);
    info.validate().map_err(failure)?;
    shared
        .surface
        .set(info)
        .map_err(|_| failure(abi::INVALID_STATE))?;
    let _ = ready.try_send(Ok(info));
    let start = Instant::now();
    let mut paused = false;
    while !shared.services.is_cancelled() {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Input(input) => host.receive_input(input),
                Command::Control(control) => {
                    paused = control == abi::CONTROL_PAUSE;
                    host.set_suspended(paused);
                    shared
                        .state
                        .lock()
                        .map_err(|_| failure(abi::PANICKED))?
                        .state = if paused {
                        abi::SESSION_PAUSED
                    } else {
                        abi::SESSION_RUNNING
                    };
                }
            }
        }
        if !paused {
            for _ in 0..64 {
                let step = host.advance(2048).map_err(runtime_error)?;
                if let Some(surface) = host.graphics().presented() {
                    let mut published = shared
                        .published
                        .lock()
                        .map_err(|_| failure(abi::PANICKED))?;
                    if published
                        .as_ref()
                        .is_none_or(|old| !Arc::ptr_eq(&old.pixels, surface))
                    {
                        let frame = Published {
                            pixels: surface.clone(),
                            generation: published.as_ref().map_or(1, |old| old.generation + 1),
                            time_ns: start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
                        };
                        *published = Some(frame);
                    }
                }
                match step {
                    Step::BudgetExhausted => {}
                    Step::Yield => break,
                    Step::Finished => {
                        if shared
                            .published
                            .lock()
                            .map_err(|_| failure(abi::PANICKED))?
                            .is_none()
                        {
                            return Err(NativeFailure::new(
                                abi::CORE_ERROR,
                                "SHINKI_EMU_LUCIFEN_NO_FRAME",
                                "Lucifen exited before presenting a frame",
                            ));
                        }
                        return Ok(());
                    }
                    Step::Calling(call) => {
                        return Err(NativeFailure::new(
                            abi::UNSUPPORTED,
                            "SHINKI_EMU_LUCIFEN_NATIVE_CALL",
                            format!("Unsupported Lucifen native call {}", call.function),
                        ));
                    }
                }
            }
        }
        thread::park_timeout(Duration::from_millis(8));
    }
    Ok(())
}

unsafe extern "C" fn input(
    session: abi::Session,
    events: *const abi::InputEvent,
    count: usize,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { borrow_session(session) }?;
        let events = unsafe { abi::borrowed_array(events, count, 256) }.map_err(failure)?;
        let info = session
            .shared
            .surface
            .get()
            .ok_or_else(|| failure(abi::INVALID_STATE))?;
        let logical = roukaku_media_core::Extent2D {
            width: info.logical.width,
            height: info.logical.height,
        };
        let raster = roukaku_media_core::Extent2D {
            width: info.raster.width,
            height: info.raster.height,
        };
        let canvas =
            StageCanvas::new(logical, raster).map_err(|_| failure(abi::INVALID_ARGUMENT))?;
        let input = unsafe {
            shinki_emu_sdk::native_input::translate(&session.shared.services, &canvas, events)
        }
        .map_err(failure)?;
        session
            .commands
            .try_send(Command::Input(input))
            .map_err(|_| failure(abi::CAPACITY))?;
        if let Some(worker) = &session.worker {
            worker.thread().unpark();
        }
        Ok(())
    })
}

unsafe extern "C" fn control(
    session: abi::Session,
    command: u32,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { borrow_session(session) }?;
        if !matches!(command, abi::CONTROL_PAUSE | abi::CONTROL_RESUME) {
            return Err(failure(abi::UNSUPPORTED));
        }
        session
            .commands
            .try_send(Command::Control(command))
            .map_err(|_| failure(abi::CAPACITY))?;
        if let Some(worker) = &session.worker {
            worker.thread().unpark();
        }
        Ok(())
    })
}

unsafe extern "C" fn state(
    session: abi::Session,
    output: *mut abi::SessionState,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        if output.is_null() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        let session = unsafe { borrow_session(session) }?;
        let mut state = *session
            .shared
            .state
            .lock()
            .map_err(|_| failure(abi::PANICKED))?;
        state.context_epoch = session.shared.services.epoch.load(Ordering::Acquire);
        unsafe { output.write(state) };
        Ok(())
    })
}

unsafe extern "C" fn frame(
    session: abi::Session,
    after: abi::FrameIdentity,
    output: *mut abi::FrameSurface,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        if output.is_null() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        let session = unsafe { borrow_session(session) }?;
        let published = session
            .shared
            .published
            .lock()
            .map_err(|_| failure(abi::PANICKED))?;
        let frame = published.as_ref();
        let info = session
            .shared
            .surface
            .get()
            .copied()
            .ok_or_else(|| failure(abi::INVALID_STATE))?;
        let identity = abi::FrameIdentity {
            session_epoch: session.shared.services.session,
            output_epoch: 1,
            generation: frame.map_or(0, |frame| frame.generation),
        };
        let mut surface = abi::FrameSurface {
            state: abi::FRAME_PENDING,
            reserved: 0,
            identity,
            timestamp_ns: frame.map_or(0, |frame| frame.time_ns),
            info,
            cpu: abi::CpuLease {
                pixels: abi::Bytes::EMPTY,
                owner: std::ptr::null_mut(),
                release: None,
            },
        };
        if let Some(frame) = frame {
            if frame.pixels.width != info.raster.width || frame.pixels.height != info.raster.height
            {
                return Err(failure(abi::INVALID_STATE));
            }
            if identity == after {
                surface.state = abi::FRAME_UNCHANGED;
            } else if let Some(lease) =
                native::lease_cpu(&frame.pixels, &session.shared.leases, |pixels| {
                    pixels.pixels()
                })
            {
                surface.state = abi::FRAME_READY;
                surface.cpu = lease;
            }
        }
        unsafe { output.write(surface) };
        Ok(())
    })
}

unsafe extern "C" fn cancel(session: abi::Session) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(session) = unsafe { borrow_session(session) } {
            session.shared.services.cancel();
            session.shared.audio_stop.store(true, Ordering::Release);
            if let Some(worker) = &session.worker {
                worker.thread().unpark();
            }
        }
    }));
}

unsafe extern "C" fn close(session: abi::Session, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if session.is_null() {
            return Err(failure(abi::INVALID_ARGUMENT));
        }
        drop(unsafe { Box::from_raw(session.cast::<Session>()) });
        Ok(())
    })
}

unsafe extern "C" fn extension(
    _name: abi::Bytes,
    _version: u32,
    output: *mut *const c_void,
) -> abi::Status {
    if output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    unsafe { output.write(std::ptr::null()) };
    abi::UNSUPPORTED
}
