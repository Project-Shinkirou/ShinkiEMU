// SPDX-License-Identifier: MIT OR Apache-2.0

//! A local-only example core for the authored TILE v1 checkerboard format.
#![deny(unsafe_op_in_unsafe_fn)]

mod session;
mod tile;
#[cfg(test)]
mod tests;

use session::{Command, Session};
use shinki_emu_abi as abi;
use shinki_emu_sdk::native::NativeFailure as Failure;
use std::{
    cell::RefCell,
    ffi::c_void,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    ptr,
};
use tile::Tile;

const FORMAT: abi::Bytes = abi::Bytes::from_static(b"tilefix.v1");
const PLUGIN_ID: abi::Bytes = abi::Bytes::from_static(b"shinki.example.tilefix");
const PATH_LIMIT: usize = 32_768;
const INPUT_LIMIT: usize = 256;

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

/// Query entry for a statically linked host.
///
/// # Safety
/// A non-null `output` must be writable for one table pointer.
pub unsafe extern "C" fn tilefix_query(version: u32, output: *mut *const abi::CoreApi) -> abi::Status {
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

/// Query entry for a dynamically loaded core package.
///
/// # Safety
/// A non-null `output` must be writable for one table pointer.
#[cfg(feature = "dynamic-plugin-export")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shinki_emu_query(
    version: u32,
    output: *mut *const abi::CoreApi,
) -> abi::Status {
    unsafe { tilefix_query(version, output) }
}

fn invalid(status: abi::Status) -> Failure {
    Failure::new(status, "TILEFIX_REQUEST", "invalid Tilefix ABI request")
}

fn boundary(error: *mut abi::Error, run: impl FnOnce() -> Result<(), Failure>) -> abi::Status {
    let result = catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|_| {
        Err(Failure::new(
            abi::PANICKED,
            "TILEFIX_PANICKED",
            "Tilefix callback panicked",
        ))
    });
    let (status, diagnostic) = match result {
        Ok(()) => (abi::OK, abi::Error::default()),
        Err(cause) if cause.status == abi::NO_MATCH => (abi::NO_MATCH, abi::Error::default()),
        Err(cause) => (cause.status, cause.diagnostic()),
    };
    if !error.is_null() {
        unsafe { error.write(diagnostic) };
    }
    status
}

unsafe extern "C" fn initialize(
    diagnostics: *const abi::Diagnostics,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let sink = unsafe { diagnostics.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if !sink.header.accepts::<abi::Diagnostics>() || sink.emit.is_none() || sink.flush.is_none() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        // Tilefix emits no logs; errors are returned directly to the caller.
        Ok(())
    })
}

unsafe extern "C" fn describe(output: *mut abi::Descriptor, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        unsafe {
            output.write(abi::Descriptor {
                header: abi::Header::of::<abi::Descriptor>(),
                family_id: abi::Bytes::from_static(b"tilefix"),
                plugin_id: PLUGIN_ID,
                version: abi::Bytes::from_static(env!("CARGO_PKG_VERSION").as_bytes()),
                capabilities: abi::CAP_CPU_SURFACE,
                formats: &FORMAT,
                format_count: 1,
                configuration: ptr::null(),
                configuration_count: 0,
                display_name: abi::Bytes::from_static(b"Tilefix Example"),
                family_name: abi::Bytes::from_static(b"Tilefix"),
                render: abi::RenderExtentSupport::default(),
            });
        }
        Ok(())
    })
}

fn load_tile(path: &Path) -> Result<Tile, Failure> {
    if path.as_os_str().is_empty() {
        return Err(invalid(abi::INVALID_ARGUMENT));
    }
    match Tile::load(path) {
        Ok(Some(tile)) => Ok(tile),
        Ok(None) => Err(Failure::new(abi::NO_MATCH, "", "")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(Failure::new(abi::NO_MATCH, "", ""))
        }
        Err(_) => Err(Failure::new(
            abi::CORE_ERROR,
            "TILEFIX_RESOURCE_IO",
            "could not read Tilefix resource",
        )),
    }
}

thread_local! {
    static GAME_ID: RefCell<String> = const { RefCell::new(String::new()) };
}

unsafe extern "C" fn probe(
    path: abi::Bytes,
    output: *mut abi::ProbeResult,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let path = Path::new(unsafe { path.text(PATH_LIMIT) }.map_err(invalid)?);
        let tile = load_tile(path)?;
        // The host copies this borrowed ID before calling the module again.
        let game_id = GAME_ID.with(|storage| {
            let mut id = storage.borrow_mut();
            *id = tile.game_id(path);
            abi::Bytes::new(id.as_bytes())
        });
        unsafe {
            output.write(abi::ProbeResult {
                game_id,
                format: FORMAT,
                confidence_permyriad: 10_000,
                reserved: 0,
            });
        }
        Ok(())
    })
}

unsafe extern "C" fn open(
    request: *const abi::OpenRequest,
    output: *mut abi::OpenResult,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        // Leave no owned handle on any failed open, even without a successful result.
        unsafe { (&raw mut (*output).session).write(ptr::null_mut()) };
        let request = unsafe { request.as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))?;
        if !request.header.accepts::<abi::OpenRequest>()
            || !request.host.header.accepts::<abi::HostServices>()
            || request.host.cancellation.is_cancelled.is_none()
            || request.session_epoch == 0
            || request.configuration_count != 0
            || request.render_extent != abi::Extent::default()
        {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        let path = Path::new(unsafe { request.game_path.text(PATH_LIMIT) }.map_err(invalid)?);
        let services = Session::services(request.session_epoch, request.host.cancellation);
        session::check_cancelled(&services)?;
        let tile = load_tile(path)?;
        session::check_cancelled(&services)?;
        let surface = tile.surface();
        let session = Session::start(services, tile, surface)?;
        if let Err(cause) = session.check_cancelled() {
            session.stop()?;
            return Err(cause);
        }
        unsafe {
            output.write(abi::OpenResult {
                session: Box::into_raw(Box::new(session)).cast(),
                context_epoch: session::INITIAL_CONTEXT,
                output_epoch: session::OUTPUT_EPOCH,
                surface,
            });
        }
        Ok(())
    })
}

/// The host serializes ordinary callbacks, and close waits for cancel to finish.
unsafe fn session_ref<'a>(handle: abi::Session) -> Result<&'a Session, Failure> {
    unsafe { handle.cast::<Session>().as_ref() }.ok_or_else(|| invalid(abi::INVALID_ARGUMENT))
}

unsafe extern "C" fn input(
    handle: abi::Session,
    events: *const abi::InputEvent,
    count: usize,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { session_ref(handle) }?;
        let events = unsafe { abi::borrowed_array(events, count, INPUT_LIMIT) }.map_err(invalid)?;
        let mut flip = false;
        for event in events {
            if event.kind == abi::INPUT_KEY
                && event.pressed == 1
                && event.repeat == 0
                && unsafe { event.physical_key.text(64) }.map_err(invalid)? == "Space"
            {
                flip = !flip;
            }
        }
        if flip {
            session.enqueue(Command::Flip)?;
        }
        Ok(())
    })
}

unsafe extern "C" fn control(handle: abi::Session, code: u32, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        let session = unsafe { session_ref(handle) }?;
        match code {
            abi::CONTROL_PAUSE => session.enqueue(Command::SetPaused(true)),
            abi::CONTROL_RESUME => session.enqueue(Command::SetPaused(false)),
            _ => Err(invalid(abi::UNSUPPORTED)),
        }
    })
}

unsafe extern "C" fn state(
    handle: abi::Session,
    output: *mut abi::SessionState,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { session_ref(handle) }?;
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        unsafe { output.write(session.state()) };
        Ok(())
    })
}

unsafe extern "C" fn frame(
    handle: abi::Session,
    previous: abi::FrameIdentity,
    output: *mut abi::FrameSurface,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        let session = unsafe { session_ref(handle) }?;
        if output.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        unsafe { output.write(session.frame(previous)?) };
        Ok(())
    })
}

unsafe extern "C" fn cancel(handle: abi::Session) {
    if let Some(session) = unsafe { handle.cast::<Session>().as_ref() } {
        session.cancel();
    }
}

unsafe extern "C" fn close(handle: abi::Session, error: *mut abi::Error) -> abi::Status {
    boundary(error, || {
        if handle.is_null() {
            return Err(invalid(abi::INVALID_ARGUMENT));
        }
        unsafe { Box::from_raw(handle.cast::<Session>()) }.stop()
    })
}

unsafe extern "C" fn extension(_: abi::Bytes, _: u32, output: *mut *const c_void) -> abi::Status {
    if output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    unsafe { output.write(ptr::null()) };
    abi::UNSUPPORTED
}
