// SPDX-License-Identifier: MIT OR Apache-2.0

use super::*;
use std::{
    fs,
    mem::MaybeUninit,
    thread,
    time::{Duration, Instant},
};

const SESSION_EPOCH: u64 = 7;
const WINDOW: abi::WindowState = abi::WindowState {
    drawable_width: 0,
    drawable_height: 0,
    scale_factor: 1.0,
    focused: 1,
    suspended: 0,
};

fn api() -> &'static abi::CoreApi {
    let mut table = ptr::null();
    assert_eq!(
        unsafe { tilefix_query(abi::ABI_VERSION, &mut table) },
        abi::OK
    );
    let api = unsafe { &*table };
    api.validate().unwrap();
    api
}

fn tile_data(width: u16, height: u16, [r, g, b]: [u8; 3]) -> [u8; 12] {
    let [w0, w1] = width.to_le_bytes();
    let [h0, h1] = height.to_le_bytes();
    [b'T', b'I', b'L', b'E', 1, w0, w1, h0, h1, r, g, b]
}

fn write_game(path: &Path, data: &[u8]) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("tilefix.dat"), data).unwrap();
}

fn path_bytes(path: &Path) -> abi::Bytes {
    abi::Bytes::new(path.to_str().unwrap().as_bytes())
}

fn probe_game(path: &Path) -> Result<String, abi::Status> {
    let mut result = MaybeUninit::<abi::ProbeResult>::uninit();
    let status =
        unsafe { api().probe.unwrap()(path_bytes(path), result.as_mut_ptr(), ptr::null_mut()) };
    if status != abi::OK {
        return Err(status);
    }
    let result = unsafe { result.assume_init() };
    Ok(unsafe { result.game_id.text(256) }.unwrap().to_owned())
}

unsafe extern "C" fn never_cancel(_: *mut c_void, _: u64) -> u32 {
    0
}

unsafe extern "C" fn already_cancelled(_: *mut c_void, _: u64) -> u32 {
    1
}

struct Opened(abi::OpenResult);

impl Opened {
    fn open(path: &Path) -> Result<Self, abi::Status> {
        Self::open_with(path, never_cancel)
    }

    fn open_with(
        path: &Path,
        is_cancelled: unsafe extern "C" fn(*mut c_void, u64) -> u32,
    ) -> Result<Self, abi::Status> {
        let request = abi::OpenRequest {
            header: abi::Header::of::<abi::OpenRequest>(),
            game_path: path_bytes(path),
            session_epoch: SESSION_EPOCH,
            configuration: ptr::null(),
            configuration_count: 0,
            window: WINDOW,
            host: abi::HostServices {
                header: abi::Header::of::<abi::HostServices>(),
                text: abi::TextService {
                    context: ptr::null_mut(),
                    request: None,
                    acknowledge: None,
                    invalidate: None,
                },
                window: abi::WindowService {
                    context: ptr::null_mut(),
                    request: None,
                    invalidate: None,
                },
                cancellation: abi::CancellationService {
                    context: ptr::null_mut(),
                    is_cancelled: Some(is_cancelled),
                },
            },
            module_owner: ptr::null_mut(),
            render_extent: abi::Extent::default(),
        };
        let mut out = MaybeUninit::<abi::OpenResult>::uninit();
        let status = unsafe { api().open.unwrap()(&request, out.as_mut_ptr(), ptr::null_mut()) };
        if status != abi::OK {
            assert!(unsafe { (&raw const (*out.as_ptr()).session).read() }.is_null());
            return Err(status);
        }
        let out = unsafe { out.assume_init() };
        out.surface.validate().unwrap();
        Ok(Self(out))
    }

    fn poll(&self, previous: abi::FrameIdentity) -> Result<abi::FrameSurface, abi::Status> {
        let mut out = MaybeUninit::<abi::FrameSurface>::uninit();
        let status = unsafe {
            api().frame.unwrap()(self.0.session, previous, out.as_mut_ptr(), ptr::null_mut())
        };
        if status != abi::OK {
            return Err(status);
        }
        let out = unsafe { out.assume_init() };
        out.validate_cpu(SESSION_EPOCH).unwrap();
        assert_eq!(out.identity.output_epoch, self.0.output_epoch);
        Ok(out)
    }

    fn next_frame(&self, previous: abi::FrameIdentity) -> abi::CpuFrame {
        until(|| {
            let frame = self.poll(previous).unwrap();
            (frame.state == abi::FRAME_READY)
                .then(|| unsafe { abi::CpuFrame::adopt(frame, SESSION_EPOCH) }.unwrap())
        })
    }

    fn space(&self, pressed: bool, repeat: bool) {
        let event = abi::InputEvent {
            kind: abi::INPUT_KEY,
            pressed: u32::from(pressed),
            repeat: u32::from(repeat),
            button: 0,
            physical_key: abi::Bytes::from_static(b"Space"),
            logical_key: abi::Bytes::from_static(b" "),
            x: 0.0,
            y: 0.0,
            displayed: abi::FrameIdentity::default(),
            window: WINDOW,
        };
        let status = unsafe { api().input.unwrap()(self.0.session, &event, 1, ptr::null_mut()) };
        assert_eq!(status, abi::OK);
    }

    fn control(&self, command: u32) {
        let status = unsafe { api().control.unwrap()(self.0.session, command, ptr::null_mut()) };
        assert_eq!(status, abi::OK);
    }

    fn state(&self) -> u32 {
        let mut out = MaybeUninit::<abi::SessionState>::uninit();
        let status =
            unsafe { api().state.unwrap()(self.0.session, out.as_mut_ptr(), ptr::null_mut()) };
        assert_eq!(status, abi::OK);
        let state = unsafe { out.assume_init() };
        state.validate().unwrap();
        state.state
    }

    fn close(mut self) {
        let handle = std::mem::replace(&mut self.0.session, ptr::null_mut());
        assert_eq!(
            unsafe { api().close.unwrap()(handle, ptr::null_mut()) },
            abi::OK
        );
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        if !self.0.session.is_null() {
            unsafe { API.close.unwrap()(self.0.session, ptr::null_mut()) };
        }
    }
}

fn until<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = poll() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "Tilefix worker did not publish in time"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn query_rejects_unsupported_versions_and_clears_the_output() {
    let mut out = api() as *const abi::CoreApi;
    assert_eq!(
        unsafe { tilefix_query(abi::ABI_VERSION + 1, &mut out) },
        abi::UNSUPPORTED
    );
    assert!(out.is_null());
    assert_eq!(
        unsafe { tilefix_query(abi::ABI_VERSION, ptr::null_mut()) },
        abi::INVALID_ARGUMENT
    );
}

#[test]
fn probe_and_open_share_format_and_resource_limits() {
    let root = tempfile::tempdir().unwrap();
    let valid = tile_data(24, 16, [34, 112, 198]);
    for name in ["first board", "renamed-board"] {
        let path = root.path().join(name);
        write_game(&path, &valid);
        assert_eq!(probe_game(&path).unwrap(), name);
        Opened::open(&path).unwrap().close();
    }

    let bad = root.path().join("bad-board");
    let mut wrong_version = tile_data(1, 1, [0; 3]);
    wrong_version[4] = 2;
    for data in [
        &valid[..11],
        tile_data(0, 2, [0; 3]).as_slice(),
        tile_data(257, 1, [0; 3]).as_slice(),
        wrong_version.as_slice(),
    ] {
        write_game(&bad, data);
        assert_eq!(probe_game(&bad), Err(abi::NO_MATCH));
        assert_eq!(Opened::open(&bad).err(), Some(abi::NO_MATCH));
    }
    write_game(&bad, &valid);
    fs::OpenOptions::new()
        .write(true)
        .open(bad.join("tilefix.dat"))
        .unwrap()
        .set_len(65_536)
        .unwrap();
    assert_eq!(probe_game(&bad), Err(abi::NO_MATCH));
    assert_eq!(Opened::open(&bad).err(), Some(abi::NO_MATCH));
    assert_eq!(
        probe_game(&root.path().join("missing")),
        Err(abi::NO_MATCH)
    );
}

#[test]
fn space_changes_the_frame_without_mutating_leased_pixels() {
    let game = tempfile::tempdir().unwrap();
    write_game(game.path(), &tile_data(16, 16, [34, 112, 198]));
    let session = Opened::open(game.path()).unwrap();
    let initial = session.next_frame(abi::FrameIdentity::default());
    assert_eq!(
        session.poll(initial.identity()).unwrap().state,
        abi::FRAME_UNCHANGED
    );
    session.space(true, false);
    let flipped = session.next_frame(initial.identity());
    assert!(flipped.identity().generation > initial.identity().generation);
    session.close();

    // Leases can be read and returned on another thread after close joined the worker.
    thread::spawn(move || {
        let blue = [34, 112, 198, 255];
        let complement = [221, 143, 57, 255];
        assert_eq!(&initial.pixels()[..4], &blue);
        assert_eq!(&initial.pixels()[8 * 4..9 * 4], &complement);
        assert_eq!(&flipped.pixels()[..4], &complement);
        assert_eq!(&flipped.pixels()[8 * 4..9 * 4], &blue);
    })
    .join()
    .unwrap();
}

#[test]
fn space_release_repeat_and_paused_press_leave_the_frame_unchanged() {
    let game = tempfile::tempdir().unwrap();
    write_game(game.path(), &tile_data(2, 2, [10, 20, 30]));
    let session = Opened::open(game.path()).unwrap();
    let initial = session.next_frame(abi::FrameIdentity::default());
    session.control(abi::CONTROL_PAUSE);
    until(|| (session.state() == abi::SESSION_PAUSED).then_some(()));
    session.space(true, false);
    session.control(abi::CONTROL_RESUME);
    until(|| (session.state() == abi::SESSION_RUNNING).then_some(()));
    session.space(false, false);
    session.space(true, true);
    assert_eq!(
        session.poll(initial.identity()).unwrap().state,
        abi::FRAME_UNCHANGED
    );
    session.space(true, false);
    assert_eq!(
        session.next_frame(initial.identity()).identity().generation,
        initial.identity().generation + 1
    );
}

#[test]
fn lease_capacity_recovers_when_a_consumer_releases() {
    let game = tempfile::tempdir().unwrap();
    write_game(game.path(), &tile_data(1, 1, [1, 2, 3]));
    let session = Opened::open(game.path()).unwrap();
    let previous = abi::FrameIdentity::default();
    let leases: [_; 3] = std::array::from_fn(|_| session.next_frame(previous));
    assert_eq!(session.poll(previous).unwrap_err(), abi::CAPACITY);
    let [first, _second, _third] = leases;
    drop(first);
    assert_eq!(session.next_frame(previous).pixels(), &[1, 2, 3, 255]);
}

#[test]
fn cancelled_open_returns_no_handle_and_cancelled_session_can_close() {
    let game = tempfile::tempdir().unwrap();
    write_game(game.path(), &tile_data(1, 1, [0; 3]));
    assert_eq!(
        Opened::open_with(game.path(), already_cancelled).err(),
        Some(abi::CANCELLED)
    );

    let session = Opened::open(game.path()).unwrap();
    let frame = session.next_frame(abi::FrameIdentity::default());
    unsafe { api().cancel.unwrap()(session.0.session) };
    assert_eq!(session.state(), abi::SESSION_FINISHED);
    assert_eq!(session.poll(frame.identity()).unwrap_err(), abi::CANCELLED);
    session.close();
    assert_eq!(frame.pixels(), &[0, 0, 0, 255]);
}
