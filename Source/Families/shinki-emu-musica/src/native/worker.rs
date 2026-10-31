use super::*;
use crate::session::types::OpenRequest;
use shinki_emu_sdk::runtime::{RuntimeStatus, WindowState};
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

pub(super) struct Launch {
    pub path: String,
    pub configuration: Vec<shinki_emu_sdk::runtime::ConfigEntry>,
    pub render_extent: abi::Extent,
    pub window: abi::WindowState,
    pub replacement: Option<text::TextBridge>,
}
pub(super) fn run(
    graphics_owner: owner::GraphicsOwner,
    launch: Launch,
    services: Arc<text::ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>, Failure>>,
) -> Result<(), Failure> {
    let Launch {
        path,
        configuration,
        render_extent,
        window,
        replacement,
    } = launch;
    let request = OpenRequest {
        game_path: path,
        session_epoch: services.session,
        configuration,
        initial_window: WindowState {
            width: window.drawable_width,
            height: window.drawable_height,
            focused: window.focused != 0,
            visible: window.suspended == 0,
        },
        audio_output: shinki_emu_sdk::runtime::open_host_audio_output().transpose()?,
    };
    let (_, session) = {
        let mut owner = graphics_owner.lock().map_err(|_| invalid(abi::PANICKED))?;
        owner.open_native_session(request, render_extent, replacement)?
    };
    serve(session, window.suspended != 0, services, commands, ready)
}
/// Publishes the opened session to the C boundary and drives it until cancel.
pub(super) fn serve(
    mut session: crate::session::MusicaSession,
    suspended: bool,
    services: Arc<text::ServiceContext>,
    commands: Receiver<Command>,
    ready: &SyncSender<Result<Arc<Shared>, Failure>>,
) -> Result<(), Failure> {
    if suspended {
        session.native_pause(true)?;
    }
    let (info, pixels) = session.native_frame();
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
    .map_err(|cause| {
        Failure::new(
            abi::CORE_ERROR,
            "SHINKI_EMU_MUSICA_CANVAS",
            cause.to_string(),
        )
    })?;
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
            state: abi::SESSION_RUNNING,
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
        pump(&mut session, &shared, commands)
    }));
    if let Ok(Err(cause)) = &result {
        fail(&shared, cause, false);
    }
    if result.is_err() {
        fail(
            &shared,
            &Failure::new(
                abi::PANICKED,
                "SHINKI_EMU_MUSICA_WORKER_PANIC",
                "native scheduler panicked",
            ),
            false,
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
            "SHINKI_EMU_MUSICA_WORKER_PANIC",
            "native scheduler panicked",
        )),
    }
}
fn fail(shared: &Shared, cause: &Failure, can_retry: bool) {
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    state.state = abi::SESSION_FAILED;
    state.flags = if can_retry { abi::SESSION_CAN_RETRY } else { 0 };
    state.error = cause.diagnostic();
}
fn pump(
    session: &mut crate::session::MusicaSession,
    shared: &Shared,
    commands: Receiver<Command>,
) -> Result<(), Failure> {
    let started = Instant::now();
    let mut previous = Instant::now();
    let mut failed = false;
    let mut paused = session.native_suspended();
    let mut finished = false;
    let mut pending_window = None;
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
        if let Some(Command::Control(control @ (abi::CONTROL_PAUSE | abi::CONTROL_RESUME))) =
            &command
        {
            paused = *control == abi::CONTROL_PAUSE;
            if let Err(cause) = session.native_pause(paused || failed) {
                failed = true;
                fail(shared, &cause.into(), false);
            }
            if !failed {
                // A finished game stays finished; host pause/resume only gates audio.
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
            }
            previous = Instant::now();
            continue;
        }
        let recovering = failed;
        let result = match command {
            Some(Command::Control(abi::CONTROL_RETRY)) if pending_window.is_some() => Ok(None),
            Some(Command::Control(abi::CONTROL_RETRY)) => session.native_retry().map(Some),
            Some(Command::Control(_)) => return Err(invalid(abi::UNSUPPORTED)),
            // The game ended itself; later input must not resume it while the host closes.
            Some(Command::Input(_)) if finished => continue,
            Some(Command::Input(events)) => {
                // During a failed hook only native restore is actionable; ordinary
                // gameplay input cannot commit choices or run half an operation.
                let restore = events.iter().any(|event| {
                    matches!(
                        event,
                        RuntimeEvent::Key {
                            code: shinki_emu_sdk::runtime::KeyCode::F9,
                            state: shinki_emu_sdk::runtime::KeyState::Pressed,
                            ..
                        }
                    )
                });
                for event in &events {
                    if let RuntimeEvent::WindowSuspended { suspended } = event {
                        paused = *suspended;
                    }
                }
                if failed && !restore {
                    continue;
                }
                session
                    .advance(if failed || paused { 0 } else { elapsed }, &events)
                    .map(Some)
            }
            None if failed || finished || paused => continue,
            None => session.advance(elapsed, &[]).map(Some),
        };
        match result {
            Ok(response) => {
                if recovering {
                    session.native_pause(paused)?;
                    previous = Instant::now();
                }
                failed = false;
                if let Some(response) = response {
                    finished = response.status == RuntimeStatus::Finished;
                    if response.reset_clock {
                        previous = Instant::now();
                    }
                    if let Some(command) = response.window_command {
                        window_sequence = window_sequence
                            .checked_add(1)
                            .ok_or_else(|| invalid(abi::INVALID_STATE))?;
                        pending_window = Some((window_sequence, command));
                    }
                }
                if let Some((id, command)) = &pending_window {
                    if let Err(cause) =
                        shinki_emu_sdk::native::window_request(&shared.services, *id, command)
                    {
                        failed = true;
                        fail(
                            shared,
                            &cause,
                            matches!(cause.status, abi::CORE_ERROR | abi::CAPACITY),
                        );
                        session.native_pause(true)?;
                        previous = Instant::now();
                        continue;
                    }
                    pending_window = None;
                    session.native_pause(paused)?;
                    previous = Instant::now();
                }
                let mut state = shared.state.lock().map_err(|_| invalid(abi::PANICKED))?;
                state.state = if finished {
                    abi::SESSION_FINISHED
                } else if paused {
                    abi::SESSION_PAUSED
                } else {
                    abi::SESSION_RUNNING
                };
                state.flags = 0;
                state.error = abi::Error::default();
                drop(state);
                let (_, pixels) = session.native_frame();
                let mut frame = shared.frame.lock().map_err(|_| invalid(abi::PANICKED))?;
                if !Arc::ptr_eq(&pixels, &frame.pixels) {
                    frame.generation = frame
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| invalid(abi::INVALID_STATE))?;
                    frame.pixels = pixels;
                    frame.timestamp_ns =
                        started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                }
                if finished {
                    shared.services.invalidate();
                }
            }
            Err(cause) => {
                if cause.code.as_str() == "SHINKI_EMU_AUDIO_CANCELLED"
                    && shared.services.is_cancelled()
                {
                    break;
                }
                failed = true;
                fail(shared, &cause.into(), session.native_failed_hook());
                previous = Instant::now();
                // semantic_event already suspended audio before requesting the
                // host. Other fatal errors suspend it here but never resume VM.
                if !session.native_failed_hook() {
                    let _ = session.native_pause(true);
                }
            }
        }
    }
    Ok(())
}
