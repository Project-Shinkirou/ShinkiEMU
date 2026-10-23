//! Test-only C ABI driver. The session opens on the native worker exactly like
//! `open`, but with the test audio sink, so tests reach the real worker loop and
//! the exported `input`/`control`/`state`/`frame`/`close` entries.
use super::*;
use std::{
    sync::atomic::AtomicU64,
    time::{Duration, Instant},
};

pub(crate) struct NativeSession {
    session: abi::Session,
    epoch: u64,
}
impl NativeSession {
    pub(crate) fn serve(
        epoch: u64,
        open: impl FnOnce(&mut crate::MusicaProvider) -> crate::session::MusicaSession + Send + 'static,
    ) -> Self {
        let services = Arc::new(text::ServiceContext {
            session: epoch,
            epoch: AtomicU64::new(1),
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            text: None,
            window: None,
            host_cancel: None,
        });
        let (commands, receiver) = sync_channel(64);
        let (ready, ready_rx) = sync_channel(1);
        let worker_services = services.clone();
        let worker = std::thread::spawn(move || {
            // The provider plays the module owner and outlives its session.
            let mut provider = crate::test_fixture::provider();
            let session = open(&mut provider);
            let result = worker::serve(session, false, worker_services, receiver, &ready);
            drop(provider);
            result
        });
        let Ok(Ok(shared)) = ready_rx.recv() else {
            panic!("native worker did not publish the session");
        };
        Self {
            session: Box::into_raw(Box::new(Handle {
                shared,
                commands,
                worker: Some(worker),
            }))
            .cast(),
            epoch,
        }
    }
    /// Submits pressed physical keys (winit code names) in one `input` call.
    pub(crate) fn keys(&self, keys: &[&str]) -> abi::Status {
        let events = keys
            .iter()
            .map(|key| abi::InputEvent {
                kind: abi::INPUT_KEY,
                pressed: 1,
                repeat: 0,
                button: 0,
                physical_key: abi::Bytes::new(key.as_bytes()),
                logical_key: abi::Bytes::new(key.as_bytes()),
                x: 0.0,
                y: 0.0,
                displayed: abi::FrameIdentity::default(),
                window: abi::WindowState {
                    drawable_width: 1280,
                    drawable_height: 720,
                    scale_factor: 1.0,
                    focused: 1,
                    suspended: 0,
                },
            })
            .collect::<Vec<_>>();
        unsafe {
            input(
                self.session,
                events.as_ptr(),
                events.len(),
                std::ptr::null_mut(),
            )
        }
    }
    pub(crate) fn control(&self, command: u32) -> abi::Status {
        unsafe { control(self.session, command, std::ptr::null_mut()) }
    }
    pub(crate) fn state(&self) -> abi::SessionState {
        let mut output = std::mem::MaybeUninit::uninit();
        assert_eq!(
            unsafe { state(self.session, output.as_mut_ptr(), std::ptr::null_mut()) },
            abi::OK
        );
        let output = unsafe { output.assume_init() };
        output.validate().unwrap();
        output
    }
    /// Polls `state` until it reports `expected` or the deadline passes.
    pub(crate) fn wait_for_state(&self, expected: u32) -> abi::SessionState {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = self.state();
            if state.state == expected {
                return state;
            }
            assert!(
                Instant::now() < deadline,
                "session state {} never became {expected}",
                state.state
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    /// First RGBA pixel of the latest published frame.
    pub(crate) fn first_pixel(&self) -> [u8; 4] {
        let mut raw = std::mem::MaybeUninit::uninit();
        assert_eq!(
            unsafe {
                frame(
                    self.session,
                    abi::FrameIdentity::default(),
                    raw.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            },
            abi::OK
        );
        let lease = unsafe { abi::CpuFrame::adopt(raw.assume_init(), self.epoch) }.unwrap();
        lease.pixels()[..4].try_into().unwrap()
    }
    pub(crate) fn close(mut self) -> abi::Status {
        let session = std::mem::replace(&mut self.session, std::ptr::null_mut());
        unsafe { close(session, std::ptr::null_mut()) }
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        if !self.session.is_null() {
            unsafe { close(self.session, std::ptr::null_mut()) };
        }
    }
}
