use super::*;
use std::time::{Duration, Instant};

fn wait_state(session: abi::Session, expected: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut current = std::mem::MaybeUninit::uninit();
        assert_eq!(
            unsafe { state(session, current.as_mut_ptr(), std::ptr::null_mut()) },
            abi::OK
        );
        let current = unsafe { current.assume_init() };
        assert_ne!(current.state, abi::SESSION_FAILED, "{:?}", current.error);
        if current.state == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owner did not process queued input"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn pull(session: abi::Session) -> abi::CpuFrame {
    let mut output = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe {
            frame(
                session,
                abi::FrameIdentity::default(),
                output.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        },
        abi::OK
    );
    unsafe { abi::CpuFrame::adopt(output.assume_init(), 73) }.unwrap()
}
fn window_input(session: abi::Session, suspended: bool) {
    let mut event: abi::InputEvent = unsafe { std::mem::zeroed() };
    event.kind = abi::INPUT_WINDOW;
    event.window.focused = 1;
    event.window.suspended = u8::from(suspended).into();
    assert_eq!(
        unsafe { input(session, &event, 1, std::ptr::null_mut()) },
        abi::OK
    );
}
/// Host window stand-in: applies each requested fullscreen mode and records it.
unsafe extern "C" fn apply_window(
    context: *mut std::ffi::c_void,
    request: *const abi::WindowRequest,
    result: *mut abi::WindowResult,
    _: *mut abi::Error,
) -> abi::Status {
    let applied = unsafe { &*context.cast::<std::sync::Mutex<Vec<(u32, u32)>>>() };
    let (request, result) = unsafe { (&*request, &mut *result) };
    applied.lock().unwrap().push((request.kind, request.flags));
    if request.kind == abi::WINDOW_FULLSCREEN {
        result.valid_fields = abi::WINDOW_RESULT_MODE;
        result.fullscreen = request.flags;
    }
    abi::OK
}

#[test]
fn queued_input_drives_real_session_and_cpu_lease_survives_joined_close() {
    let _single = crate::family::TEST_SESSION_LOCK.lock();
    // The worker thread owns a clone, so the host context outlives every request.
    let applied = Arc::new(std::sync::Mutex::new(Vec::<(u32, u32)>::new()));
    let worker_applied = applied.clone();
    let services = Arc::new(shinki_emu_sdk::native::ServiceContext {
        session: 73,
        epoch: std::sync::atomic::AtomicU64::new(1),
        cancelled: AtomicBool::new(false),
        waiting: AtomicBool::new(false),
        text: None,
        window: Some(abi::WindowService {
            context: Arc::as_ptr(&applied).cast_mut().cast(),
            request: Some(apply_window),
            invalidate: None,
        }),
        host_cancel: None,
    });
    let worker_services = services.clone();
    let (commands, rx) = sync_channel(64);
    let (ready, opened) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _applied = worker_applied;
        let (_root, session) = crate::session::tests::fixture();
        worker::serve(session, true, worker_services, rx, &ready)
    });
    let shared = opened
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    let leases = shared.leases.clone();
    let cancellation = shared.cancellation.clone();
    let session = Box::into_raw(Box::new(Handle {
        shared,
        commands,
        worker: Some(worker),
    }))
    .cast();
    let retained = pull(session);
    let original_pixels = retained.pixels().to_vec();
    window_input(session, false);
    // The configured startup mode (no cmvs.cfg WINDOW_MODE = fullscreen)
    // reaches the host from the first scheduler advance.
    let deadline = Instant::now() + Duration::from_secs(5);
    while applied.lock().unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "startup mode never reached the host"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    wait_state(session, abi::SESSION_RUNNING);
    assert_eq!(
        applied.lock().unwrap().as_slice(),
        [(abi::WINDOW_FULLSCREEN, abi::FULLSCREEN_BORDERLESS)]
    );
    std::thread::sleep(Duration::from_millis(40));
    window_input(session, true);
    wait_state(session, abi::SESSION_PAUSED);
    let latest = pull(session);
    assert_eq!(latest.info().raster.width, 4);
    assert_eq!(latest.info().raster.height, 4);
    assert_eq!(retained.pixels(), original_pixels);
    drop(latest);
    assert_eq!(unsafe { close(session, std::ptr::null_mut()) }, abi::OK);
    assert!(cancellation.is_cancelled());
    assert_eq!(retained.pixels(), original_pixels);
    assert_eq!(leases.load(Ordering::Acquire), 1);
    std::thread::spawn(move || drop(retained)).join().unwrap();
    assert_eq!(leases.load(Ordering::Acquire), 0);
}

fn reject_invalid_diagnostics(diagnostics: abi::Diagnostics) {
    let mut error = abi::Error::default();
    assert_eq!(
        unsafe { initialize(std::ptr::null(), &mut error) },
        abi::INVALID_ARGUMENT
    );
    for invalid in [
        abi::Diagnostics {
            emit: None,
            ..diagnostics
        },
        abi::Diagnostics {
            flush: None,
            ..diagnostics
        },
        abi::Diagnostics {
            header: abi::Header {
                size: 0,
                ..diagnostics.header
            },
            ..diagnostics
        },
        abi::Diagnostics {
            header: abi::Header {
                version: 0,
                ..diagnostics.header
            },
            ..diagnostics
        },
    ] {
        assert_eq!(
            unsafe { initialize(&invalid, &mut error) },
            abi::INVALID_ARGUMENT
        );
    }
}

#[cfg(not(feature = "dynamic-plugin-export"))]
#[test]
fn static_initialize_preserves_the_installed_host_subscriber() {
    // A Manager has already installed diagnostics before registering static cores.
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();
    static CALLBACKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    unsafe extern "C" fn emit(_: *mut std::ffi::c_void, _: *const abi::LogRecord) {
        CALLBACKS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe extern "C" fn flush(_: *mut std::ffi::c_void) {
        CALLBACKS.fetch_add(1, Ordering::Relaxed);
    }
    let diagnostics = abi::Diagnostics {
        header: abi::Header::of::<abi::Diagnostics>(),
        context: std::ptr::null_mut(),
        emit: Some(emit),
        flush: Some(flush),
    };
    reject_invalid_diagnostics(diagnostics);
    let mut error = abi::Error::default();
    assert_eq!(unsafe { initialize(&diagnostics, &mut error) }, abi::OK);
    let mut descriptor = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { describe(descriptor.as_mut_ptr(), &mut error) },
        abi::OK
    );
    tracing::info!(event = "cmvs.static.initialized");
    assert!(tracing::dispatcher::get_default(|dispatch| {
        dispatch
            .downcast_ref::<tracing::subscriber::NoSubscriber>()
            .is_some()
    }));
    assert_eq!(CALLBACKS.load(Ordering::Relaxed), 0);
    reject_invalid_diagnostics(diagnostics);
    assert_eq!(
        unsafe { describe(descriptor.as_mut_ptr(), &mut error) },
        abi::OK
    );
}

#[cfg(feature = "dynamic-plugin-export")]
#[test]
fn dynamic_initialize_installs_module_bridge_and_rejects_sink_replacement() {
    static RECORDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    unsafe extern "C" fn emit(_: *mut std::ffi::c_void, _: *const abi::LogRecord) {
        RECORDS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe extern "C" fn flush(_: *mut std::ffi::c_void) {}
    let diagnostics = abi::Diagnostics {
        header: abi::Header::of::<abi::Diagnostics>(),
        context: std::ptr::null_mut(),
        emit: Some(emit),
        flush: Some(flush),
    };
    reject_invalid_diagnostics(diagnostics);
    let mut api = std::ptr::null();
    assert_eq!(
        unsafe { shinki_emu_query(abi::ABI_VERSION, &mut api) },
        abi::OK
    );
    let api = unsafe { &*api };
    let mut error = abi::Error::default();
    assert_eq!(
        unsafe { api.initialize.unwrap()(&diagnostics, &mut error) },
        abi::OK
    );
    tracing::error!(event = "cmvs.dynamic.diagnostic_roundtrip");
    assert!(RECORDS.load(Ordering::Relaxed) > 0);
    assert_eq!(
        unsafe { api.initialize.unwrap()(&diagnostics, &mut error) },
        abi::OK
    );
    let conflicting = abi::Diagnostics {
        context: std::ptr::dangling_mut::<u8>().cast(),
        ..diagnostics
    };
    assert_eq!(
        unsafe { api.initialize.unwrap()(&conflicting, &mut error) },
        abi::INVALID_STATE
    );
    reject_invalid_diagnostics(diagnostics);
    let mut descriptor = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { api.describe.unwrap()(descriptor.as_mut_ptr(), &mut error) },
        abi::OK
    );
}
