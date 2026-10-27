use super::streams::progress::{PlaybackClock, PlaybackSource};
use super::{AudioBridge, OUTPUT_RATE, Output};
use parking_lot::Mutex;
use shinki_emu_sdk::NativeAudioProgress;
use shinki_emu_sdk::runtime::SessionResult;
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// Test output timeline: writes accept frames, the test consumes them.
#[derive(Default)]
pub(super) struct TestPlayback {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    accepted: u64,
    played: u64,
    queued_frames: VecDeque<u64>,
}

impl TestPlayback {
    pub(super) fn accept(&self, frames: usize) {
        let mut state = self.state.lock();
        state.accepted += frames as u64;
        if frames != 0 {
            state.queued_frames.push_back(frames as u64);
        }
    }

    /// Explicit silent test output consumption, independent of write acceptance.
    pub(super) fn consume_to(&self, target: u64) {
        let mut state = self.state.lock();
        assert!(target >= state.played && target <= state.accepted);
        let mut remaining = target - state.played;
        while remaining != 0 {
            let front = state
                .queued_frames
                .front_mut()
                .expect("accepted PCM awaiting test consumption");
            let consumed = remaining.min(*front);
            *front -= consumed;
            if *front == 0 {
                state.queued_frames.pop_front();
            }
            state.played += consumed;
            remaining -= consumed;
        }
    }

    pub(super) fn consume_all(&self) {
        let target = self.state.lock().accepted;
        self.consume_to(target);
    }
}

impl PlaybackSource for TestPlayback {
    fn progress(&self) -> NativeAudioProgress {
        let state = self.state.lock();
        NativeAudioProgress {
            accepted_frames: state.accepted,
            played_frames: state.played,
        }
    }
}

/// Headless device stand-in: accepts each chunk after its real-time duration
/// and reports it played at once, optionally recording the accepted PCM.
pub(super) struct PacedOutput {
    playback: Arc<TestPlayback>,
    recorded: Option<Arc<Mutex<Vec<Vec<f32>>>>>,
}

impl PacedOutput {
    pub(super) fn start(recorded: Option<Arc<Mutex<Vec<Vec<f32>>>>>) -> AudioBridge {
        AudioBridge::start(move |_| {
            let playback = Arc::new(TestPlayback::default());
            let clock: PlaybackClock = playback.clone();
            Ok((Self { playback, recorded }, clock))
        })
        .expect("paced test output starts")
    }
}

impl Output for PacedOutput {
    fn check(&self) -> SessionResult<()> {
        Ok(())
    }

    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
        let frames = samples.len() / 2;
        let deadline =
            Instant::now() + Duration::from_secs_f64(frames as f64 / f64::from(OUTPUT_RATE));
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            std::thread::sleep(remaining.min(Duration::from_millis(5)));
        }
        if let Some(recorded) = &self.recorded {
            recorded.lock().push(samples.clone());
        }
        self.playback.accept(frames);
        self.playback.consume_all();
        Ok(Some(samples))
    }

    fn flush(&mut self, _: &AtomicBool) -> SessionResult<Option<()>> {
        Ok(Some(()))
    }
}
