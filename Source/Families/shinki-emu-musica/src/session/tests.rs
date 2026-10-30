use super::types::*;
use crate::test_fixture as fixture;
use crate::{MusicaVm, audio::Audio, mount_musica, scene::core_error};
use shinki_emu_musica_keys::MUSICA_KEY_FILE;
use shinki_emu_sdk::runtime::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
// The provider intentionally permits one live session per process.
static PROVIDER_SESSION: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
#[cfg(feature = "ffmpeg-vcpkg")]
#[path = "tests/movie.rs"]
mod movie;
#[path = "tests/translation.rs"]
mod translation;

#[cfg(not(feature = "ffmpeg-vcpkg"))]
#[test]
fn movie_without_ffmpeg_is_an_explicit_feature_error() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".movie 1 sample.mp4 1280 720 t\r\n.end\r\n");
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let error = opened.session.advance(16_666_667, &[]).err().unwrap();
    assert_eq!(error.code.as_str(), "SHINKI_EMU_MUSICA_MOVIE_UNAVAILABLE");
    opened.session.close().unwrap();
}

#[test]
fn unsupported_command_diagnostic_keeps_ordinal_without_source_text() {
    let error = super::vm_error(crate::MusicaRuntimeError::UnsupportedOpcode {
        opcode: "private script text: hidden".into(),
        ordinal: 42,
    });
    assert_eq!(error.code.as_str(), "SHINKI_EMU_MUSICA_RUNTIME_OPCODE");
    assert_eq!(
        error.message.as_str(),
        "script command at ordinal 42 is not implemented"
    );
}

#[test]
fn render_extent_sets_raster_and_zero_uses_logical_canvas() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1  speaker First\r\n.end\r\n");
    let mut provider = fixture::provider();
    for (requested, raster) in [((0, 0), (1280, 720)), ((1920, 1080), (1920, 1080))] {
        let (response, session) = provider
            .open_native_session(
                request(root.path(), Sink::default()),
                shinki_emu_abi::Extent {
                    width: requested.0,
                    height: requested.1,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            (
                response.frame.width,
                response.frame.height,
                response.frame.logical_width,
                response.frame.logical_height
            ),
            (raster.0, raster.1, 1280, 720)
        );
        drop(session);
    }
}

#[derive(Clone, Default)]
struct Sink {
    cancelled: Arc<AtomicBool>,
    nonzero: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    block: bool,
    left: Arc<AtomicUsize>,
    right: Arc<AtomicUsize>,
}
impl AudioSink for Sink {
    fn configure(&self, format: PcmFormatSpec) -> SessionResult<()> {
        assert_eq!(format, crate::audio::FORMAT);
        Ok(())
    }
    fn write(&self, samples: Vec<f32>) -> SessionResult<Option<Vec<f32>>> {
        self.writes.fetch_add(1, Ordering::AcqRel);
        while self.block && !self.cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        for frame in samples.as_chunks::<2>().0 {
            if frame[0].abs() > 0.01 {
                self.left.fetch_add(1, Ordering::Relaxed);
            }
            if frame[1].abs() > 0.01 {
                self.right.fetch_add(1, Ordering::Relaxed);
            }
        }
        if samples.iter().any(|s| s.abs() > 0.01) {
            self.nonzero.store(true, Ordering::Release);
        }
        std::thread::sleep(Duration::from_millis(1));
        Ok(Some(samples))
    }
    fn cancel(&self) -> SessionResult<()> {
        self.cancelled.store(true, Ordering::Release);
        Ok(())
    }
}
fn request(root: &std::path::Path, sink: Sink) -> OpenRequest {
    OpenRequest {
        session_epoch: shinki_emu_sdk::runtime::allocate_session_epoch().unwrap(),
        game_path: root.to_str().unwrap().into(),
        configuration: vec![ConfigEntry {
            id: "launch_mode".into(),
            value: ConfigValue::Enum("direct".into()),
        }],
        initial_window: WindowState {
            width: 1280,
            height: 720,
            focused: true,
            visible: true,
        },
        audio_output: Some(Box::new(sink)),
    }
}
fn key(code: KeyCode) -> RuntimeEvent {
    RuntimeEvent::Key {
        code,
        state: KeyState::Pressed,
        modifiers: KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        },
    }
}
struct Capture(Vec<u8>);
impl Capture {
    fn read(&mut self, poll: FramePoll) -> SessionResult<()> {
        let FramePoll::Ready(frame) = poll else {
            return Err(SessionError::invalid(
                "TEST_FRAME_PENDING",
                "test expected a completed frame",
            ));
        };
        self.0 = frame.pixels().to_vec();
        Ok(())
    }
}
fn until(test: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !test() {
        assert!(Instant::now() < deadline, "worker progress deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn native_gpu_choice_focus_save_restore_and_confirm_follow_the_selected_branch() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".select First:left Second:right\r\n.label left\r\n.message 1   Wrong\r\n.end\r\n.label right\r\n.message 2   Selected\r\n.end\r\n");
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    assert_eq!(
        opened.session.advance(16_666_667, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    let mut frame = Capture(Vec::new());
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let first = frame.0.clone();
    assert!(first.as_chunks::<4>().0.iter().any(|p| p[0] != 0));
    opened
        .session
        .advance(0, &[key(KeyCode::ArrowDown)])
        .unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let second = frame.0.clone();
    assert_ne!(first, second);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened
        .session
        .advance(0, &[key(KeyCode::ArrowDown)])
        .unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, first);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, second);
    opened
        .session
        .advance(16_666_667, &[key(KeyCode::Enter)])
        .unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_ne!(frame.0, second);
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_choice_pointer_ignores_blank_clicks_and_selects_hovered_branch() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".select First:left Second:right\r\n.label left\r\n.message 1   Wrong\r\n.end\r\n.label right\r\n.end\r\n");
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let click = || RuntimeEvent::PointerButton {
        button: PointerButton::Primary,
        state: KeyState::Pressed,
    };
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut frame = Capture(Vec::new());
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let first = frame.0.clone();
    // No known pointer position must not confirm the keyboard focus.
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[click()])
            .unwrap()
            .status,
        RuntimeStatus::Waiting
    );
    for (x, y) in [(20.0, 20.0), (250.0, 300.0), (1040.0, 310.0)] {
        assert_eq!(
            opened
                .session
                .advance(16_666_667, &[RuntimeEvent::PointerMove { x, y }, click()])
                .unwrap()
                .status,
            RuntimeStatus::Waiting
        );
        frame
            .read(opened.session.acquire_frame(None).unwrap())
            .unwrap();
        assert_eq!(frame.0, first);
    }
    opened
        .session
        .advance(0, &[RuntimeEvent::PointerMove { x: 250.0, y: 310.0 }])
        .unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_ne!(frame.0, first);
    // Only the second branch ends immediately; the first waits for dialogue input.
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[click()])
            .unwrap()
            .status,
        RuntimeStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_crossfade_clear_removes_the_previous_effect_frame() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".effect CrossFade BG.png:* 32 100\r\n.effect CrossFade\r\n.end\r\n",
    );
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(&frame.0[..4], &[25, 100, 220, 255]);
    opened.session.advance(16_666_667, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert!(
        frame
            .0
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [0, 0, 0, 255])
    );
    opened.session.close().unwrap();
}
#[test]
fn native_family_advances_real_archive_scene_audio_input_save_restore_and_close() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage * BG.png 0 0\r\n.playBGM tone.ogg[50,-100]\r\n.message 1   Hello\r\n.end\r\n",
    );
    let mut provider = fixture::provider();
    assert!(
        provider
            .probe(ProbeRequest {
                game_path: root.path().to_str().unwrap().into()
            })
            .unwrap()
            .is_some()
    );
    let sink = Sink::default();
    let mut opened = provider.open(request(root.path(), sink.clone())).unwrap();
    assert_eq!(
        provider
            .open(request(root.path(), Sink::default()))
            .err()
            .unwrap()
            .code(),
        "SHINKI_EMU_MUSICA_ACTIVE"
    );
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut frame = Capture(vec![]);
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(&frame.0[..4], &[25, 100, 220, 255]);
    opened.session.advance(16_666_667, &[]).unwrap();
    until(|| sink.nonzero.load(Ordering::Acquire));
    assert!(sink.left.load(Ordering::Relaxed) > 0);
    assert_eq!(sink.right.load(Ordering::Relaxed), 0);
    assert_eq!(
        opened.session.advance(16_666_667, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let dialogue = frame.0.clone();
    assert!(
        dialogue
            .as_chunks::<4>()
            .0
            .iter()
            .skip(528 * 1280)
            .any(|p| p[0] > 0)
    );
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        RuntimeStatus::Finished
    );
    assert_eq!(
        opened
            .session
            .advance(0, &[key(KeyCode::F9)])
            .unwrap()
            .status,
        RuntimeStatus::Waiting
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, dialogue);
    opened.session.close().unwrap();
    assert!(sink.cancelled.load(Ordering::Acquire));
    let reopened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    reopened.session.close().unwrap();
}
#[test]
fn audio_close_cancels_blocked_write_and_joins_worker() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let archive = Arc::new(
        mount_musica(root.path(), &root.path().join(MUSICA_KEY_FILE))
            .map_err(core_error)
            .unwrap(),
    );
    let sink = Sink {
        block: true,
        ..Default::default()
    };
    let mut audio = Audio::start(archive, Some(Box::new(sink.clone()))).unwrap();
    until(|| sink.writes.load(Ordering::Acquire) > 0);
    let duration = audio.duration(4).unwrap();
    assert!(matches!(
        duration.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    audio.shutdown().unwrap();
    assert!(matches!(
        duration.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let writes = sink.writes.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(sink.writes.load(Ordering::Acquire), writes);
}
#[test]
fn full_audio_control_queue_waits_and_cancellation_releases_the_sender() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let archive = Arc::new(mount_musica(root.path(), &root.path().join(MUSICA_KEY_FILE)).unwrap());
    let sink = Sink {
        block: true,
        ..Default::default()
    };
    let audio = Arc::new(Audio::start(archive, Some(Box::new(sink.clone()))).unwrap());
    until(|| sink.writes.load(Ordering::Acquire) > 0);
    for _ in 0..crate::audio::COMMAND_CAPACITY {
        audio.suspend(false).unwrap();
    }
    let (completed, completion) = std::sync::mpsc::sync_channel(1);
    let sender = audio.clone();
    let worker = std::thread::spawn(move || {
        let result = sender.suspend(true);
        completed.send(()).unwrap();
        result
    });
    until(|| audio.take_backpressure());
    let pending = matches!(
        completion.recv_timeout(Duration::from_millis(30)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );
    audio.cancellation_flag().store(true, Ordering::Release);
    sink.cancel().unwrap();
    let result = worker.join().unwrap();
    let mut audio =
        Arc::try_unwrap(audio).unwrap_or_else(|_| panic!("audio sender still retained"));
    audio.shutdown().unwrap();
    assert!(
        pending,
        "a full queue must apply backpressure, not fail gameplay"
    );
    assert_eq!(
        result.unwrap_err().code.as_str(),
        "SHINKI_EMU_AUDIO_CANCELLED"
    );
    assert!(!audio.take_backpressure());
}
#[test]
fn audio_snapshot_timeout_cancels_blocked_worker_and_rejects_late_commands() {
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let archive = Arc::new(mount_musica(root.path(), &root.path().join(MUSICA_KEY_FILE)).unwrap());
    let sink = Sink {
        block: true,
        ..Default::default()
    };
    let mut audio = Audio::start(archive, Some(Box::new(sink.clone()))).unwrap();
    until(|| sink.writes.load(Ordering::Acquire) > 0);
    assert_eq!(
        audio.snapshot().err().unwrap().code.as_str(),
        "SHINKI_EMU_MUSICA_AUDIO_SNAPSHOT"
    );
    assert!(sink.cancelled.load(Ordering::Acquire));
    assert_eq!(
        audio.apply(Vec::new()).unwrap_err().code.as_str(),
        "SHINKI_EMU_MUSICA_AUDIO_SNAPSHOT"
    );
    assert!(audio.shutdown().is_err());
    let writes = sink.writes.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(sink.writes.load(Ordering::Acquire), writes);
}
#[test]

fn non_yielding_variable_loop_is_bounded() {
    let source = b".label loop\r\n.set count = count + 1\r\n.goto loop\r\n";
    let script = crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        roukaku_core::Hash256::from_sha256(source),
        script,
        0,
    )
    .unwrap();
    assert!(vm.step(1).is_err());
}

#[test]
fn native_gpu_control_keys_respect_release_focus_and_choice_boundaries() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".pragma enable_control\r\n.message 1   First\r\n.message 2   Second\r\n.select Confirm:done\r\n.label done\r\n.end\r\n");
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    opened.session.advance(16_666_667, &[]).unwrap();
    let mut capture = Capture(Vec::new());
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let first = capture.0.clone();
    opened
        .session
        .advance(0, &[key(KeyCode::ControlLeft), key(KeyCode::ControlRight)])
        .unwrap();
    let mut release_left = key(KeyCode::ControlLeft);
    if let RuntimeEvent::Key { state, .. } = &mut release_left {
        *state = KeyState::Released;
    }
    opened.session.advance(16_666_667, &[release_left]).unwrap();
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let second = capture.0.clone();
    assert_ne!(first, second);
    opened
        .session
        .advance(
            100_000_000,
            &[RuntimeEvent::WindowFocused { focused: false }],
        )
        .unwrap();
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(capture.0, second);
    opened
        .session
        .advance(
            0,
            &[
                key(KeyCode::ControlRight),
                RuntimeEvent::WindowSuspended { suspended: true },
            ],
        )
        .unwrap();
    opened
        .session
        .advance(
            100_000_000,
            &[RuntimeEvent::WindowSuspended { suspended: false }],
        )
        .unwrap();
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(capture.0, second);

    opened
        .session
        .advance(
            16_666_667,
            &[
                RuntimeEvent::WindowFocused { focused: true },
                key(KeyCode::ControlRight),
            ],
        )
        .unwrap();
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let choice = capture.0.clone();
    assert_ne!(choice, second);
    assert_eq!(
        opened.session.advance(100_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    capture
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(capture.0, choice);
    assert_eq!(
        opened
            .session
            .advance(16_666_667, &[key(KeyCode::Enter)])
            .unwrap()
            .status,
        RuntimeStatus::Finished
    );
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_screen_shake_advances_during_wait_and_restores_through_family_input() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".stage BG.png 0 0 * 0 0\r\n.shakescreen V 4 30\r\n.wait 1000\r\n.end\r\n",
    );
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let original = frame.0.clone();
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(30_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let negative = frame.0.clone();
    assert_ne!(negative, original);
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let positive = frame.0.clone();
    assert_ne!(positive, negative);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, negative);
    opened.session.advance(30_000_000, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, positive);
    opened
        .session
        .advance(0, &[RuntimeEvent::WindowSuspended { suspended: true }])
        .unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, positive);
    opened
        .session
        .advance(0, &[RuntimeEvent::WindowSuspended { suspended: false }])
        .unwrap();
    opened.session.advance(30_000_000, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, negative);
    opened.session.close().unwrap();
}

fn assert_scroll_restores_background_position(command: &str, middle_pixel: usize) {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        format!(".stage * BG.png 0 0\r\n.{command}\r\n.endscroll false\r\n.end\r\n").as_bytes(),
    );
    let mut provider = fixture::provider();
    let mut opened = provider
        .open(request(root.path(), Sink::default()))
        .unwrap();
    let mut frame = Capture(Vec::new());
    opened.session.advance(16_666_667, &[]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let original = frame.0.clone();
    opened.session.advance(16_666_667, &[]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        RuntimeStatus::Waiting
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let middle = frame.0.clone();
    assert_ne!(middle, original);
    assert_eq!(&middle[..4], &[0, 0, 0, 255]);
    assert_eq!(
        &middle[middle_pixel * 4..middle_pixel * 4 + 4],
        &original[..4]
    );
    opened.session.advance(0, &[key(KeyCode::F5)]).unwrap();
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        RuntimeStatus::Finished
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    let finished = frame.0.clone();
    assert_ne!(finished, middle);
    opened.session.advance(0, &[key(KeyCode::F9)]).unwrap();
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, middle);
    assert_eq!(
        opened.session.advance(40_000_000, &[]).unwrap().status,
        RuntimeStatus::Finished
    );
    frame
        .read(opened.session.acquire_frame(None).unwrap())
        .unwrap();
    assert_eq!(frame.0, finished);
    opened.session.close().unwrap();
}

#[test]
fn native_gpu_axis_scroll_waits_and_restores_background_position() {
    assert_scroll_restores_background_position("hscroll 8 1", 4);
}

#[test]
fn native_gpu_linear_scroll_waits_and_restores_background_position() {
    assert_scroll_restores_background_position("scroll 8 4 1", 2 * 1280 + 4);
}

#[path = "tests/wscroll2.rs"]
mod wscroll2;

#[path = "tests/particles.rs"]
mod particles;

#[path = "tests/character.rs"]
mod character;

#[path = "tests/message.rs"]
mod message;

#[path = "tests/backlog.rs"]
mod backlog;

#[path = "tests/voice_preferences.rs"]
mod voice_preferences;

#[path = "tests/playback.rs"]
mod playback;

#[path = "tests/read_skip.rs"]
mod read_skip;

#[path = "tests/encoding.rs"]
mod encoding;

#[path = "tests/includes.rs"]
mod includes;

#[path = "tests/save_pages.rs"]
mod save_pages;

#[path = "tests/config.rs"]
mod config;
#[path = "tests/exit.rs"]
mod exit;
#[path = "tests/gallery.rs"]
mod gallery;
#[path = "tests/title.rs"]
mod title;

#[test]
#[ignore = "requires a hardware GPU"]
fn native_gpu_ani_companion_uses_wrapping_seed_and_shared_source_cache() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".end\r\n");
    let mut ani = vec![0, 1, 1, 0, 0, 0, 0, 0];
    ani.extend_from_slice(&encoding_rs::SHIFT_JIS.encode("speaker 開き目").0);
    ani.push(0);
    ani.extend_from_slice(&1u16.to_le_bytes());
    ani.extend_from_slice(&1u16.to_le_bytes());
    ani.extend_from_slice(&32u16.to_le_bytes());
    ani.extend_from_slice(&0i16.to_le_bytes());
    ani.extend_from_slice(&0i16.to_le_bytes());
    ani.extend_from_slice(&[0, 0, 255, 255]);
    fixture::asset(root.path(), "bg", "BG.ani", &ani);
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    let mut state =
        MusicaVm::decode_native_save(&session.vm.encode_native_save().unwrap()).unwrap();
    state.random_state = u64::MAX;
    session
        .vm
        .restore_native_save(&postcard::to_allocvec(&state).unwrap(), 1)
        .unwrap();
    let stage = crate::MusicaStageCommand {
        foreground: crate::MusicaForegroundLayers::default(),
        reference_position: None,
        background: Some(crate::MusicaStageLayer {
            resource_uri: "musica:/bg/BG.png".into(),
            x: 0,
            y: 0,
        }),
        stands: Vec::new(),
        transition: crate::MusicaTransitionState::default(),
    };
    session.attach_stage_animations(&stage).unwrap();
    let first = session.scene.ani_source("musica:/bg/BG.ani").unwrap();
    session.attach_stage_animations(&stage).unwrap();
    let second = session.scene.ani_source("musica:/bg/BG.ani").unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let patch = &session.vm.state().ani_patches["musica:/bg/BG.png"];
    assert_eq!(patch.rng, 0);
    assert_eq!(patch.characters[0].name, "speaker");
    assert!(session.vm.encode_native_save().is_ok());
    Box::new(session).close().unwrap();
}

#[test]
#[ignore = "requires a hardware GPU"]
fn native_gpu_transition_completion_publishes_the_final_frame_without_a_vm_tick() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".stage * BLACK.png 0 0\r\n.transition 0 * 2560\r\n.stage * WHITE.png 0 0\r\n.wait 100000\r\n.end\r\n");
    let png = |color| {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    fixture::assets(
        root.path(),
        "bg",
        &[
            ("BLACK.png", &png([0, 0, 0, 255])),
            ("WHITE.png", &png([255, 255, 255, 255])),
        ],
    );
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_session(request(root.path(), Sink::default()))
        .unwrap();
    for tick in 1..=2 {
        let event = session.vm.step(tick).unwrap();
        session.apply_event(event).unwrap();
    }
    session.advance(5_000_000, &[]).unwrap();
    assert!(session.vm.state().transition_active.is_some());
    let middle = session.scene.pixels.clone();
    let tick = session.vm.state().fixed_tick;
    session.advance(5_000_000, &[]).unwrap();
    assert_eq!(session.vm.state().fixed_tick, tick);
    assert!(session.vm.state().transition_active.is_none());
    assert_ne!(session.scene.pixels, middle);
    assert_eq!(&session.scene.pixels[..4], &[255, 255, 255, 255]);
    Box::new(session).close().unwrap();
}
