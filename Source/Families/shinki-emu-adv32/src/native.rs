use crate::{Error, game::Game, vm::Vm};
use roukaku_media_core::{Extent2D, OwnedPixelBuffer};
use shinki_emu_abi as abi;
#[cfg(not(target_env = "ohos"))]
use shinki_emu_sdk::gpu_scene::GpuSceneHost;
#[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
use shinki_emu_sdk::runtime::WindowCommand;
use shinki_emu_sdk::{
    StageCanvas,
    native::{NativeFailure, ServiceContext, config_entries, lease_cpu, window_request},
    runtime::{ConfigValue, RuntimeEvent},
};
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const FORMAT: abi::Bytes = abi::Bytes::from_static(b"adv32.mes");
const FORMATS: &[abi::Bytes] = &[FORMAT];
const RENDER: abi::RenderExtentSupport = abi::RenderExtentSupport {
    min: abi::Extent {
        width: 320,
        height: 180,
    },
    // At most 64 MiB per tightly packed RGBA8 frame, including letterboxing.
    max: abi::Extent {
        width: 4096,
        height: 4096,
    },
    default: abi::Extent {
        width: 0,
        height: 0,
    },
    flags: abi::RENDER_PRESERVE_ASPECT,
    reserved: 0,
};
#[cfg(not(target_env = "ohos"))]
const HARDWARE_COMPOSITING: &str = "hardware_compositing";
#[cfg(not(target_env = "ohos"))]
const HEADLESS_AUDIO_DEVICE: &str = "headless_audio_device";
#[cfg(not(target_env = "ohos"))]
const NULL_AUDIO_DEVICE: &str = "nullaudiodevice";
#[cfg(not(target_env = "ohos"))]
const AUDIO_DEVICES: &[abi::Bytes] = &[
    abi::Bytes::from_static(b"native"),
    abi::Bytes::from_static(NULL_AUDIO_DEVICE.as_bytes()),
];
#[cfg(not(target_env = "ohos"))]
const AUDIO_DEVICE_LABELS: &[abi::Bytes] = &[
    abi::Bytes::from_static(b"Native audio"),
    abi::Bytes::from_static(b"Clocked null audio (test only)"),
];
const FONT_FILE: &str = "font_file";
const SKIP_DISC_CHECK: &str = "skip_disc_check";
const DISC_CONFIGURATION: abi::ConfigField = abi::ConfigField {
    id: abi::Bytes::from_static(SKIP_DISC_CHECK.as_bytes()),
    label: abi::Bytes::from_static(b"Skip startup disc check"),
    group: abi::Bytes::from_static(b"Startup"),
    default_value: abi::ConfigValue {
        kind: abi::VALUE_BOOL,
        reserved: 0,
        integer: 0,
        number: 0.0,
        text: abi::Bytes::EMPTY,
    },
    integer_min: 0,
    integer_max: 0,
    number_min: 0.0,
    number_max: 0.0,
    max_bytes: 0,
    flags: abi::CONFIG_FLAG_ADVANCED,
    choices: ptr::null(),
    choice_count: 0,
    description: abi::Bytes::from_static(
        b"Start ADV32 from installed data without performing native startup media authorization.",
    ),
    choice_labels: ptr::null(),
    step: 0.0,
    unit: abi::Bytes::EMPTY,
};
const FONT_CONFIGURATION: abi::ConfigField = abi::ConfigField {
    id: abi::Bytes::from_static(FONT_FILE.as_bytes()),
    label: abi::Bytes::from_static(b"Font file"),
    group: abi::Bytes::from_static(b"Text"),
    default_value: abi::ConfigValue {
        kind: abi::VALUE_STRING,
        reserved: 0,
        integer: 0,
        number: 0.0,
        text: abi::Bytes::EMPTY,
    },
    integer_min: 0,
    integer_max: 0,
    number_min: 0.0,
    number_max: 0.0,
    max_bytes: 1024,
    flags: abi::CONFIG_FLAG_FILE_PATH | abi::CONFIG_FLAG_GAME_RELATIVE,
    choices: ptr::null(),
    choice_count: 0,
    description: abi::Bytes::from_static(b"Use a font file from the game directory."),
    choice_labels: ptr::null(),
    step: 0.0,
    unit: abi::Bytes::EMPTY,
};
#[cfg(target_env = "ohos")]
const CONFIGURATION: &[abi::ConfigField] = &[FONT_CONFIGURATION, DISC_CONFIGURATION];
#[cfg(not(target_env = "ohos"))]
const CONFIGURATION: &[abi::ConfigField] = &[
    FONT_CONFIGURATION,
    DISC_CONFIGURATION,
    abi::ConfigField {
        id: abi::Bytes::from_static(HARDWARE_COMPOSITING.as_bytes()),
        label: abi::Bytes::from_static(b"Hardware compositing"),
        group: abi::Bytes::from_static(b"Video"),
        default_value: abi::ConfigValue {
            kind: abi::VALUE_BOOL,
            reserved: 0,
            integer: 0,
            number: 0.0,
            text: abi::Bytes::EMPTY,
        },
        integer_min: 0,
        integer_max: 0,
        number_min: 0.0,
        number_max: 0.0,
        max_bytes: 0,
        flags: 0,
        choices: ptr::null(),
        choice_count: 0,
        description: abi::Bytes::from_static(b"Use a hardware GPU to composite supported scenes."),
        choice_labels: ptr::null(),
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
static PROBE_ID: Mutex<String> = Mutex::new(String::new());
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

type Result<T> = std::result::Result<T, NativeFailure>;

struct SessionOptions {
    #[cfg(not(target_env = "ohos"))]
    hardware_compositing: bool,
    #[cfg(not(target_env = "ohos"))]
    gpu_host: Option<GpuSceneHost>,
    #[cfg(not(target_env = "ohos"))]
    null_audio: bool,
    skip_disc_check: bool,
    font_file: Option<PathBuf>,
    render_extent: abi::Extent,
}

/// Query used by the statically linked Manager.
///
/// # Safety
/// A non-null output pointer must be writable for one pointer.
pub unsafe extern "C" fn adv32_query(
    version: u32,
    output: *mut *const abi::CoreApi,
) -> abi::Status {
    if output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    unsafe { output.write(ptr::null()) };
    if version != abi::ABI_VERSION {
        return abi::UNSUPPORTED;
    }
    unsafe { output.write(&API) };
    abi::OK
}

#[cfg(feature = "dynamic-plugin-export")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(
    version: u32,
    output: *mut *const abi::CoreApi,
) -> abi::Status {
    unsafe { adv32_query(version, output) }
}

fn descriptor() -> abi::Descriptor {
    abi::Descriptor {
        header: abi::Header::of::<abi::Descriptor>(),
        family_id: abi::Bytes::from_static(b"adv32"),
        plugin_id: abi::Bytes::from_static(b"shinki.emu.adv32"),
        version: abi::Bytes::from_static(env!("CARGO_PKG_VERSION").as_bytes()),
        capabilities: abi::CAP_CPU_SURFACE | abi::CAP_RENDER_EXTENT,
        formats: FORMATS.as_ptr(),
        format_count: FORMATS.len(),
        configuration: CONFIGURATION.as_ptr(),
        configuration_count: CONFIGURATION.len(),
        display_name: abi::Bytes::from_static(b"Shinki ADV32"),
        family_name: abi::Bytes::from_static(b"ADV32"),
        render: RENDER,
    }
}

fn invalid(status: abi::Status) -> NativeFailure {
    NativeFailure::new(
        status,
        "SHINKI_EMU_ADV32_ABI",
        "invalid ADV32 ABI request or state",
    )
}

fn runtime(cause: Error) -> NativeFailure {
    NativeFailure::new(
        abi::CORE_ERROR,
        "SHINKI_EMU_ADV32_RUNTIME",
        cause.to_string(),
    )
}

fn boundary(error: *mut abi::Error, action: impl FnOnce() -> Result<()>) -> abi::Status {
    let result =
        catch_unwind(AssertUnwindSafe(action)).unwrap_or_else(|_| Err(invalid(abi::PANICKED)));
    match result {
        Ok(()) => abi::OK,
        Err(cause) => {
            if let Some(error) = unsafe { error.as_mut() } {
                *error = cause.diagnostic();
            }
            cause.status
        }
    }
}

unsafe extern "C" fn initialize(
    diagnostics: *const abi::Diagnostics,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let diagnostics =
            unsafe { diagnostics.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if !diagnostics.header.accepts::<abi::Diagnostics>() || diagnostics.emit.is_none() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        unsafe { abi::diagnostic_bridge::install(*diagnostics) }.map_err(invalid)
    })
}

unsafe extern "C" fn describe(output: *mut abi::Descriptor, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let output = unsafe { output.as_mut() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        *output = descriptor();
        Ok(())
    })
}

unsafe extern "C" fn probe(
    path: abi::Bytes,
    output: *mut abi::ProbeResult,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let output = unsafe { output.as_mut() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        let path = Path::new(unsafe { path.text(32768) }.map_err(invalid)?);
        let game = Game::find(path)
            .map_err(runtime)?
            .ok_or_else(|| invalid(abi::NO_MATCH))?;
        let mut id = PROBE_ID.lock().map_err(|_| invalid(abi::PANICKED))?;
        *id = game.id;
        *output = abi::ProbeResult {
            game_id: abi::Bytes::new(id.as_bytes()),
            format: FORMAT,
            confidence_permyriad: 10000,
            reserved: 0,
        };
        Ok(())
    })
}

struct Frame {
    pixels: OwnedPixelBuffer,
    generation: u64,
    timestamp_ns: u64,
}

struct Shared {
    services: Arc<ServiceContext>,
    surface: abi::SurfaceInfo,
    canvas: StageCanvas,
    frame: Mutex<Frame>,
    state: Mutex<abi::SessionState>,
    leases: Arc<AtomicUsize>,
}

enum Command {
    Input(Vec<RuntimeEvent>),
    Pause(bool),
}

struct Handle {
    shared: Arc<Shared>,
    queue: SyncSender<Command>,
    thread: JoinHandle<Result<()>>,
}

unsafe fn handle<'a>(session: abi::Session) -> Result<&'a Handle> {
    unsafe { session.cast::<Handle>().as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))
}

unsafe extern "C" fn open(
    raw_request: *const abi::OpenRequest,
    output: *mut abi::OpenResult,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let output = unsafe { output.as_mut() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        output.session = ptr::null_mut();
        let request =
            unsafe { raw_request.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if !request.header.accepts::<abi::OpenRequest>()
            || !request.host.header.accepts::<abi::HostServices>()
            || request.session_epoch == 0
            || request.host.cancellation.is_cancelled.is_none()
            || request.window.suspended > 1
        {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let descriptor = descriptor();
        descriptor
            .render
            .validate_request(descriptor.capabilities, request.render_extent)
            .map_err(invalid)?;
        let config = unsafe { config_entries(descriptor, request) }.map_err(invalid)?;
        #[cfg(not(target_env = "ohos"))]
        let hardware_compositing = config.iter().any(|entry| {
            entry.id == HARDWARE_COMPOSITING && matches!(entry.value, ConfigValue::Bool(true))
        });
        #[cfg(not(target_env = "ohos"))]
        let gpu_host = if hardware_compositing {
            unsafe { GpuSceneHost::from_request(raw_request) }.map_err(invalid)?
        } else {
            None
        };
        #[cfg(not(target_env = "ohos"))]
        let null_audio = config.iter().any(|entry| {
            entry.id == HEADLESS_AUDIO_DEVICE
                && matches!(&entry.value, ConfigValue::Enum(device) if device == NULL_AUDIO_DEVICE)
        });
        let skip_disc_check = config.iter().any(|entry| {
            entry.id == SKIP_DISC_CHECK && matches!(entry.value, ConfigValue::Bool(true))
        });
        let font_file = config.iter().find_map(|entry| match &entry.value {
            ConfigValue::String(value) if entry.id == FONT_FILE && !value.is_empty() => {
                Some(PathBuf::from(value))
            }
            _ => None,
        });
        let options = SessionOptions {
            #[cfg(not(target_env = "ohos"))]
            hardware_compositing,
            #[cfg(not(target_env = "ohos"))]
            gpu_host,
            #[cfg(not(target_env = "ohos"))]
            null_audio,
            skip_disc_check,
            font_file,
            render_extent: request.render_extent,
        };
        let path = PathBuf::from(unsafe { request.game_path.text(32768) }.map_err(invalid)?);
        let services = Arc::new(ServiceContext {
            session: request.session_epoch,
            epoch: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            text: None,
            window: request.host.window.request.map(|_| request.host.window),
            host_cancel: Some(request.host.cancellation),
        });
        if services.is_cancelled() {
            return Err(invalid(abi::CANCELLED));
        }
        let paused = request.window.suspended != 0;
        let (queue, commands) = sync_channel(64);
        let (ready, startup) = sync_channel(1);
        let thread = thread::Builder::new()
            .name("adv32-native".into())
            .spawn(move || {
                let worker_services = Arc::clone(&services);
                let mut result = catch_unwind(AssertUnwindSafe(|| {
                    serve(&path, paused, options, worker_services, commands, &ready)
                }))
                .unwrap_or_else(|_| Err(invalid(abi::PANICKED)));
                if let Err(cause) = &result {
                    let _ = ready.try_send(Err(cause.clone()));
                }
                services.invalidate();
                if services.cancelled.load(Ordering::Acquire)
                    && result
                        .as_ref()
                        .is_err_and(|cause| cause.status == abi::CANCELLED)
                {
                    result = Ok(());
                }
                result
            })
            .map_err(|_| invalid(abi::CORE_ERROR))?;
        let shared = match startup.recv() {
            Ok(Ok(shared)) => shared,
            result => {
                let _ = thread.join();
                return Err(result
                    .ok()
                    .and_then(std::result::Result::err)
                    .unwrap_or_else(|| invalid(abi::CORE_ERROR)));
            }
        };
        *output = abi::OpenResult {
            surface: shared.surface,
            session: Box::into_raw(Box::new(Handle {
                shared,
                queue,
                thread,
            }))
            .cast(),
            context_epoch: 1,
            output_epoch: 1,
        };
        Ok(())
    })
}

fn serve(
    path: &Path,
    mut paused: bool,
    options: SessionOptions,
    services: Arc<ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>>>,
) -> Result<()> {
    let game = Game::find(path)
        .map_err(runtime)?
        .ok_or_else(|| invalid(abi::NO_MATCH))?;
    let mut vm = Vm::new(game, options.font_file.as_deref()).map_err(runtime)?;
    vm.set_skip_disc_check(options.skip_disc_check);
    #[cfg(not(target_env = "ohos"))]
    vm.set_null_audio(options.null_audio);
    let (width, height) = vm.extent();
    let extent = abi::Extent { width, height };
    let raster = if options.render_extent == abi::Extent::default() {
        extent
    } else {
        options.render_extent
    };
    let canvas = StageCanvas::new(
        Extent2D::new(extent.width, extent.height),
        Extent2D::new(raster.width, raster.height),
    )
    .map_err(|_| invalid(abi::INVALID_ARGUMENT))?;
    vm.set_presentation(canvas);
    #[cfg(not(target_env = "ohos"))]
    if options.hardware_compositing {
        vm.enable_hardware_compositor(canvas, options.gpu_host)
            .map_err(runtime)?;
    }
    let surface = abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_IGNORED,
        rows: abi::ROWS_TOP_DOWN,
        logical: extent,
        raster,
        stride: u64::from(raster.width) * 4,
    };
    surface.validate().map_err(invalid)?;
    let frame_bytes = surface.cpu_bytes().map_err(invalid)?;
    let shared = Arc::new(Shared {
        services,
        surface,
        canvas,
        frame: Mutex::new(Frame {
            pixels: OwnedPixelBuffer::from_vec(Vec::new()),
            generation: 0,
            timestamp_ns: 0,
        }),
        state: Mutex::new(abi::SessionState {
            state: if paused {
                abi::SESSION_PAUSED
            } else {
                abi::SESSION_RUNNING
            },
            flags: 0,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        }),
        leases: Arc::new(AtomicUsize::new(0)),
    });
    if ready.send(Ok(Arc::clone(&shared))).is_err() {
        return Ok(());
    }
    let started = Instant::now();
    let mut previous = started;
    let mut events = Vec::new();
    let mut window_sequence = 0;
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
        while !shared.services.is_cancelled() {
            match commands.recv_timeout(Duration::from_millis(8)) {
                Ok(Command::Input(input)) => events = input,
                Ok(Command::Pause(value)) => {
                    paused = value;
                    vm.set_paused(value);
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if shared.services.is_cancelled() {
                break;
            }
            let now = Instant::now();
            if !paused
                && let Some(image) = vm
                    .tick(now.saturating_duration_since(previous), &events)
                    .map_err(runtime)?
            {
                if image.as_slice().len() != frame_bytes {
                    return Err(runtime(Error::Invalid("ADV32 presentation frame size")));
                }
                let mut frame = shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
                if image.as_slice() != frame.pixels.as_slice() {
                    frame.pixels = image;
                    frame.generation += 1;
                    frame.timestamp_ns =
                        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                }
            }
            events.clear();
            while let Some(window) = vm.window_command() {
                // Mobile hosts own one fixed game surface, not a movable client window.
                #[cfg(any(target_os = "android", target_os = "ios", target_env = "ohos"))]
                if matches!(
                    window,
                    WindowCommand::SetVisibility(_) | WindowCommand::SetClientSize { .. }
                ) {
                    continue;
                }
                window_sequence += 1;
                window_request(&shared.services, window_sequence, &window)?;
            }
            previous = now;
            let mut state = shared.state.lock().map_err(|_| invalid(abi::PANICKED))?;
            state.state = if vm.finished() {
                abi::SESSION_FINISHED
            } else if paused {
                abi::SESSION_PAUSED
            } else {
                abi::SESSION_RUNNING
            };
        }
        vm.flush_pending_saves().map_err(runtime)
    }))
    .unwrap_or_else(|_| Err(invalid(abi::PANICKED)));
    if let Err(cause) = &result {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.state = abi::SESSION_FAILED;
        state.error = cause.diagnostic();
    }
    result
}

unsafe extern "C" fn input(
    session: abi::Session,
    events: *const abi::InputEvent,
    count: usize,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { handle(session) }?;
        let events = unsafe { abi::borrowed_array(events, count, 256) }.map_err(invalid)?;
        let events = unsafe {
            shinki_emu_sdk::native_input::translate(
                &session.shared.services,
                &session.shared.canvas,
                events,
            )
        }
        .map_err(invalid)?;
        match session.queue.try_send(Command::Input(events)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(invalid(abi::CAPACITY)),
            Err(TrySendError::Disconnected(_)) => {
                let state = session
                    .shared
                    .state
                    .lock()
                    .map_err(|_| invalid(abi::PANICKED))?;
                if state.state != abi::SESSION_FAILED {
                    return Err(invalid(abi::CORE_ERROR));
                }
                let code =
                    String::from_utf8_lossy(&state.error.code[..state.error.code_len as usize]);
                let message = String::from_utf8_lossy(
                    &state.error.message[..state.error.message_len as usize],
                );
                Err(NativeFailure::new(
                    abi::CORE_ERROR,
                    code.into_owned(),
                    message.into_owned(),
                ))
            }
        }
    })
}

unsafe extern "C" fn control(
    session: abi::Session,
    command: u32,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { handle(session) }?;
        let paused = match command {
            abi::CONTROL_PAUSE => true,
            abi::CONTROL_RESUME => false,
            _ => return Err(invalid(abi::UNSUPPORTED)),
        };
        session
            .queue
            .try_send(Command::Pause(paused))
            .map_err(|_| invalid(abi::CAPACITY))
    })
}

unsafe extern "C" fn state(
    session: abi::Session,
    output: *mut abi::SessionState,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { handle(session) }?;
        let output = unsafe { output.as_mut() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        let mut state = *session
            .shared
            .state
            .lock()
            .map_err(|_| invalid(abi::PANICKED))?;
        state.context_epoch = session.shared.services.epoch.load(Ordering::Acquire);
        if session.shared.services.cancelled.load(Ordering::Acquire)
            && state.state != abi::SESSION_FAILED
        {
            state.state = abi::SESSION_FINISHED;
        }
        *output = state;
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
        let session = unsafe { handle(session) }?;
        let output = unsafe { output.as_mut() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        let shared = &session.shared;
        let frame = shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
        let identity = abi::FrameIdentity {
            session_epoch: shared.services.session,
            output_epoch: 1,
            generation: frame.generation,
        };
        *output = abi::FrameSurface {
            state: abi::FRAME_PENDING,
            reserved: 0,
            identity,
            timestamp_ns: frame.timestamp_ns,
            info: shared.surface,
            cpu: abi::CpuLease {
                pixels: abi::Bytes::EMPTY,
                owner: ptr::null_mut(),
                release: None,
            },
        };
        if frame.generation == 0 {
            return Ok(());
        }
        if after == identity {
            output.state = abi::FRAME_UNCHANGED;
        } else if let Some(lease) =
            lease_cpu(&frame.pixels, &shared.leases, OwnedPixelBuffer::as_slice)
        {
            output.state = abi::FRAME_READY;
            output.cpu = lease;
        }
        Ok(())
    })
}

unsafe extern "C" fn cancel(session: abi::Session) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(session) = unsafe { handle(session) } {
            session.shared.services.cancel();
        }
    }));
}

unsafe extern "C" fn close(session: abi::Session, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if session.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let session = unsafe { Box::from_raw(session.cast::<Handle>()) };
        session.shared.services.cancel();
        session.thread.join().map_err(|_| invalid(abi::PANICKED))?
    })
}

unsafe extern "C" fn extension(_: abi::Bytes, _: u32, output: *mut *const c_void) -> abi::Status {
    if output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    unsafe { output.write(ptr::null()) };
    abi::UNSUPPORTED
}
