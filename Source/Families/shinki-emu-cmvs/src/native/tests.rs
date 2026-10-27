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
    let audio_stopped = shared.audio_stop.clone();
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
    assert!(audio_stopped.load(Ordering::Acquire));
    assert_eq!(retained.pixels(), original_pixels);
    assert_eq!(leases.load(Ordering::Acquire), 1);
    std::thread::spawn(move || drop(retained)).join().unwrap();
    assert_eq!(leases.load(Ordering::Acquire), 0);
}
