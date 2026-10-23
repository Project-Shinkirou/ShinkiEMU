use super::*;
use shinki_emu_sdk::runtime::{RuntimeStatus, WindowState};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};
pub(super) struct Launch {
    pub path: String,
    pub configuration: Vec<shinki_emu_sdk::runtime::ConfigEntry>,
    pub window: abi::WindowState,
}
pub(super) fn run(
    graphics_owner: owner::GraphicsOwner,
    launch: Launch,
    services: Arc<shinki_emu_sdk::native::ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>, Failure>>,
) -> Result<(), Failure> {
    let window = WindowState {
        width: launch.window.drawable_width,
        height: launch.window.drawable_height,
        focused: launch.window.focused != 0,
        visible: launch.window.suspended == 0,
    };
    let session = graphics_owner
        .lock()
        .map_err(|_| invalid(abi::PANICKED))?
        .open(&launch.path, &launch.configuration, window)?;
    serve(
        session,
        launch.window.suspended != 0,
        services,
        commands,
        ready,
    )
}
pub(super) fn serve(
    mut session: crate::session::CmvsSession,
    suspended: bool,
    services: Arc<shinki_emu_sdk::native::ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>, Failure>>,
) -> Result<(), Failure> {
    if suspended {
        session.native_pause(true)?;
    }
    let (info, pixels) = session.native_frame()?;
    let info = abi::SurfaceInfo {
        kind: abi::SURFACE_CPU,
        pixel_format: abi::PIXEL_RGBA8,
        color: abi::COLOR_SRGB,
        alpha: abi::ALPHA_IGNORED,
        rows: abi::ROWS_TOP_DOWN,
        logical: abi::Extent {
            width: info.logical_width,
            height: info.logical_height,
        },
        raster: abi::Extent {
            width: info.width,
            height: info.height,
        },
        stride: u64::from(info.stride),
    };
    info.validate().map_err(invalid)?;
    let canvas = shinki_emu_sdk::StageCanvas::new(
        roukaku_media_core::Extent2D {
            width: info.logical.width,
            height: info.logical.height,
        },
        roukaku_media_core::Extent2D {
            width: info.raster.width,
            height: info.raster.height,
        },
    )
    .map_err(|cause| Failure::new(abi::CORE_ERROR, "SHINKI_EMU_CMVS_CANVAS", cause.to_string()))?;
    let shared = Arc::new(Shared {
        services: services.clone(),
        audio_stop: session.native_audio_cancellation(),
        frame: Mutex::new(Published {
            info,
            pixels,
            generation: 1,
            timestamp_ns: 0,
        }),
        state: Mutex::new(abi::SessionState {
            state: if suspended {
                abi::SESSION_PAUSED
            } else {
                abi::SESSION_RUNNING
            },
            flags: 0,
            context_epoch: 1,
            output_epoch: 1,
            error: abi::Error::default(),
        }),
        canvas,
        leases: Arc::new(AtomicUsize::new(0)),
    });
    if ready.send(Ok(shared.clone())).is_err() {
        shared.cancel();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match pump(&mut session, &shared, commands) {
            Err(cause) if cause.status == abi::CANCELLED && services.is_cancelled() => Ok(()),
            result => result,
        }
    }));
    if let Ok(Err(cause)) = &result {
        fail(&shared, cause);
    }
    if result.is_err() {
        fail(
            &shared,
            &Failure::new(
                abi::PANICKED,
                "SHINKI_EMU_CMVS_WORKER_PANIC",
                "native scheduler panicked",
            ),
        );
        services.cancel();
    }
    // Invalidate first, then close every media/audio owner even after failure.
    services.invalidate();
    let cleanup = Box::new(session).close().map_err(Failure::from);
    match result {
        Ok(result) => {
            result?;
            cleanup
        }
        Err(_) => Err(Failure::new(
            abi::PANICKED,
            "SHINKI_EMU_CMVS_WORKER_PANIC",
            "native scheduler panicked",
        )),
    }
}
fn fail(shared: &Shared, cause: &Failure) {
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    state.state = abi::SESSION_FAILED;
    state.flags = 0;
    state.error = cause.diagnostic();
}
fn pump(
    session: &mut crate::session::CmvsSession,
    shared: &Shared,
    commands: Receiver<Command>,
) -> Result<(), Failure> {
    let started = Instant::now();
    let mut previous = Instant::now();
    let mut paused = session.native_suspended();
    let mut finished = false;
    let mut window_sequence = 0u64;
    while !shared.services.is_cancelled() {
        let command = match commands.recv_timeout(Duration::from_millis(8)) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if shared.services.is_cancelled() {
            break;
        }
        let now = Instant::now();
        let elapsed = now
            .duration_since(previous)
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        previous = now;
        let response = match command {
            Some(Command::Control(control)) => {
                paused = control == abi::CONTROL_PAUSE;
                session.native_pause(paused)?;
                shared
                    .state
                    .lock()
                    .map_err(|_| invalid(abi::PANICKED))?
                    .state = if finished {
                    abi::SESSION_FINISHED
                } else if paused {
                    abi::SESSION_PAUSED
                } else {
                    abi::SESSION_RUNNING
                };
                previous = Instant::now();
                continue;
            }
            Some(Command::Input(_)) if finished => continue,
            Some(Command::Input(events)) => {
                for event in &events {
                    if let RuntimeEvent::WindowSuspended { suspended } = event {
                        paused = *suspended;
                    }
                }
                let response = session.advance(if paused { 0 } else { elapsed }, &events)?;
                if events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::WindowSuspended { .. }))
                {
                    session.native_pause(paused)?;
                }
                response
            }
            None if paused || finished => continue,
            None => session.advance(elapsed, &[])?,
        };
        finished = response.status == RuntimeStatus::Finished;
        if let Some(command) = response.window_command {
            window_sequence = window_sequence
                .checked_add(1)
                .ok_or_else(|| invalid(abi::INVALID_STATE))?;
            shinki_emu_sdk::native::window_request(&shared.services, window_sequence, &command)?;
        }
        if response.reset_clock {
            previous = Instant::now();
        }
        shared
            .state
            .lock()
            .map_err(|_| invalid(abi::PANICKED))?
            .state = if finished {
            abi::SESSION_FINISHED
        } else if paused {
            abi::SESSION_PAUSED
        } else {
            abi::SESSION_RUNNING
        };
        let (_, pixels) = session.native_frame()?;
        let mut frame = shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
        if pixels.allocation_ptr() != frame.pixels.allocation_ptr()
            || pixels.len() != frame.pixels.len()
        {
            frame.generation = frame
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid(abi::INVALID_STATE))?;
            frame.pixels = pixels;
            frame.timestamp_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        }
        if finished {
            shared.services.invalidate();
        }
    }
    Ok(())
}
