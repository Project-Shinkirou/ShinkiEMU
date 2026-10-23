//! CMVS-private software audio worker: mixes the native crossfade BGM pair
//! and the channel sound table into its native device queue.
//!
//! The native engine drives DirectSound through two alternating BGM
//! channels (`0x222A`/`0x222D`, blend = `(elapsed << 8) / duration`, full
//! volume 256), a channel-sound table and a voice channel. The worker owns
//! a fixed 48 kHz stereo f32 mix bus, advances fades by the wall-clock
//! sample position and blocks on its bounded queue so the device
//! callback never calls back into the VM.
use crate::family::error;
#[cfg(test)]
use roukaku_platform::AudioOutputLane;
use shinki_emu_sdk::runtime::SessionResult;
use shinki_emu_sdk::NativeAudioOutput;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

const OUTPUT_RATE: u32 = 48_000;
const CHUNK_FRAMES: usize = 512;
const CHANNEL_SOUNDS: usize = 8;
/// Upper bound on one decoded clip: ten minutes at the source rate.
pub(super) const MAX_CLIP_FRAMES: usize = 48_000 * 600;

/// One fully decoded stereo clip at its source sample rate. Frames are
/// interleaved left/right f32 pairs.
pub(super) struct DecodedClip {
    sample_rate: u32,
    frames: Arc<Vec<f32>>,
}

impl DecodedClip {
    pub(super) fn from_kira(data: kira::sound::static_sound::StaticSoundData) -> Self {
        let mut frames = Vec::with_capacity(data.frames.len() * 2);
        for frame in data.frames.iter() {
            frames.push(frame.left);
            frames.push(frame.right);
        }
        Self {
            sample_rate: data.sample_rate,
            frames: Arc::new(frames),
        }
    }
}

enum Command {
    Crossfade {
        primary: DecodedClip,
        secondary: Option<DecodedClip>,
        fade_ms: u32,
        playback_flag: bool,
    },
    FadeOut {
        fade_ms: u32,
    },
    Stop,
    PlayChannel {
        channel: u8,
        clip: DecodedClip,
        looping: bool,
    },
    StopChannel {
        channel: u8,
    },
    Suspend(bool),
    Shutdown,
}

struct Stream {
    clip: DecodedClip,
    /// Play position in source frames (fractional for cross-rate reads).
    position: f64,
    looping: bool,
    volume: f32,
    /// Per-sample linear fade step and target, or `None` when steady.
    fade: Option<Fade>,
}

struct Fade {
    step_per_frame: f32,
    target: f32,
    removes_at_target: bool,
}

pub(super) struct AudioBridge {
    commands: std::sync::mpsc::Sender<Command>,
    worker: Option<std::thread::JoinHandle<()>>,
    cancelled: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    /// Output frames written per channel sound slot; the movie player reads
    /// slot 2 as its audio clock (`SndSlot_positionMs`).
    channel_frames_written: Arc<[AtomicU64; CHANNEL_SOUNDS]>,
}

impl AudioBridge {
    pub(super) fn new() -> SessionResult<Self> {
        Self::start(NativeAudioOutput::open)
    }
    fn start<W: Output + 'static>(
        open: impl FnOnce(Arc<AtomicBool>) -> SessionResult<W> + Send + 'static,
    ) -> SessionResult<Self> {
        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let channel_frames_written: Arc<[AtomicU64; CHANNEL_SOUNDS]> =
            Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_failure = Arc::clone(&failure);
        let worker_counters = Arc::clone(&channel_frames_written);
        let (ready, startup) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("cmvs-audio-worker".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut output = match open(worker_cancelled.clone()) {
                        Ok(output) => output,
                        Err(cause) => {
                            let _ = ready.send(Err(cause));
                            return;
                        }
                    };
                    let _ = ready.send(Ok(()));
                    let state = Worker {
                        bgm: [None, None],
                        active_bgm: 0,
                        channels: std::array::from_fn(|_| None),
                    };
                    run_worker(
                        &mut output,
                        rx,
                        state,
                        worker_cancelled,
                        worker_failure.clone(),
                        worker_counters,
                    );
                    if let Err(cause) = output.check() {
                        record_failure(&worker_failure, &cause.to_string());
                    }
                }));
                if result.is_err() {
                    record_failure(&worker_failure, "CMVS audio worker panicked");
                }
            })
            .map_err(|_| {
                error(
                    "SHINKI_EMU_CMVS_AUDIO_WORKER",
                    "CMVS audio worker thread could not start",
                )
            })?;
        if let Err(cause) = startup.recv().unwrap_or_else(|_| {
            Err(error(
                "SHINKI_EMU_CMVS_AUDIO_START",
                "native audio owner exited during startup",
            ))
        }) {
            cancelled.store(true, Ordering::Release);
            let _ = worker.join();
            return Err(cause);
        }
        Ok(Self {
            commands: tx,
            worker: Some(worker),
            cancelled,
            failure,
            channel_frames_written,
        })
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub(super) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
    pub(super) fn suspend(&mut self, paused: bool) -> SessionResult<()> {
        self.submit(Command::Suspend(paused))
    }

    /// Output-frame position of one channel sound slot. The position wraps
    /// like the native looping clock; callers derive elapsed time from the
    /// difference between observations.
    pub(super) fn channel_position_frames(&self, channel: u8) -> u64 {
        self.channel_frames_written
            .get(usize::from(channel))
            .map(|counter| counter.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    pub(super) fn play_crossfade(
        &mut self,
        primary: DecodedClip,
        secondary: Option<DecodedClip>,
        fade_ms: u32,
        playback_flag: bool,
    ) -> SessionResult<()> {
        self.submit(Command::Crossfade {
            primary,
            secondary,
            fade_ms,
            playback_flag,
        })
    }

    pub(super) fn fade_out(&mut self, fade_ms: u32) -> SessionResult<()> {
        self.submit(Command::FadeOut { fade_ms })
    }

    pub(super) fn stop(&mut self) -> SessionResult<()> {
        self.submit(Command::Stop)
    }

    pub(super) fn play_channel(
        &mut self,
        channel: u8,
        clip: DecodedClip,
        looping: bool,
    ) -> SessionResult<()> {
        self.submit(Command::PlayChannel {
            channel,
            clip,
            looping,
        })
    }

    pub(super) fn stop_channel(&mut self, channel: u8) -> SessionResult<()> {
        self.submit(Command::StopChannel { channel })
    }

    fn submit(&mut self, command: Command) -> SessionResult<()> {
        self.check_failure()?;
        self.commands.send(command).map_err(|_| {
            error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "CMVS audio worker stopped accepting commands",
            )
        })
    }

    pub(super) fn check_failure(&self) -> SessionResult<()> {
        let failure = self.failure.lock().map_err(|_| {
            error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "CMVS audio failure slot is poisoned",
            )
        })?;
        match failure.as_deref() {
            Some(message) => Err(error("SHINKI_EMU_CMVS_AUDIO_WORKER", message)),
            None => Ok(()),
        }
    }

    /// Cancel outstanding requests, drain the queue and join the worker.
    /// Any close error is still returned after cleanup completes.
    pub(super) fn close(&mut self) -> SessionResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Shutdown);
        let mut result = self.check_failure();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                result = Err(error(
                    "SHINKI_EMU_CMVS_AUDIO_JOIN",
                    "audio owner panicked during close",
                ));
            }
        }
        if result.is_ok() {
            result = self.check_failure();
        }
        result
    }
}

impl Drop for AudioBridge {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct Worker {
    bgm: [Option<Stream>; 2],
    active_bgm: usize,
    channels: [Option<Stream>; CHANNEL_SOUNDS],
}

impl Worker {
    fn any_active(&self) -> bool {
        self.bgm.iter().flatten().next().is_some()
            || self.channels.iter().flatten().next().is_some()
    }

    fn start_stream(clip: DecodedClip, looping: bool, fade_ms: u32, fade_in: bool) -> Stream {
        let volume = if fade_in { 0.0 } else { 1.0 };
        let fade = if fade_ms > 0 {
            Some(Fade {
                step_per_frame: 1000.0 / (fade_ms as f32 * OUTPUT_RATE as f32),
                target: if fade_in { 1.0 } else { 0.0 },
                removes_at_target: !fade_in,
            })
        } else {
            None
        };
        Stream {
            clip,
            position: 0.0,
            looping,
            volume,
            fade,
        }
    }
}

trait Output {
    fn check(&self) -> SessionResult<()>;
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>>;
}
impl Output for NativeAudioOutput {
    fn check(&self) -> SessionResult<()> {
        NativeAudioOutput::check(self)
    }
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
        NativeAudioOutput::write(self, samples, stop)
    }
}
fn run_worker(
    output: &mut impl Output,
    commands: std::sync::mpsc::Receiver<Command>,
    mut state: Worker,
    cancelled: Arc<AtomicBool>,
    failure: Arc<Mutex<Option<String>>>,
    counters: Arc<[AtomicU64; CHANNEL_SOUNDS]>,
) {
    let mut mix = vec![0.0_f32; CHUNK_FRAMES * 2];
    let mut suspended = false;
    loop {
        // Command intake: drain everything queued, then wait when idle.
        loop {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            let command = if state.any_active() && !suspended {
                match commands.try_recv() {
                    Ok(command) => command,
                    Err(_) => break,
                }
            } else {
                match commands.recv_timeout(std::time::Duration::from_millis(8)) {
                    Ok(command) => command,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return record_closed(&failure),
                }
            };
            match command {
                Command::Crossfade {
                    primary,
                    secondary,
                    fade_ms,
                    playback_flag,
                } => {
                    // The native pair alternates: the incoming clip takes
                    // the inactive channel and fades in while the previous
                    // one fades out over the same span. A secondary clip
                    // explicitly preempts the other side. `playback_flag`
                    // false keeps the pair stopped after the fade.
                    let incoming = if playback_flag {
                        Some(Worker::start_stream(primary, true, fade_ms, true))
                    } else {
                        None
                    };
                    if let Some(secondary) = secondary {
                        let other = Worker::start_stream(secondary, true, fade_ms, false);
                        state.bgm[state.active_bgm ^ 1] =
                            if playback_flag { Some(other) } else { None };
                    } else if let Some(outgoing) = state.bgm[state.active_bgm ^ 1].as_mut() {
                        outgoing.fade = if fade_ms > 0 {
                            Some(Fade {
                                step_per_frame: 1000.0 / (fade_ms as f32 * OUTPUT_RATE as f32),
                                target: 0.0,
                                removes_at_target: true,
                            })
                        } else {
                            outgoing.volume = 0.0;
                            None
                        };
                    }
                    let next_active = state.active_bgm ^ 1;
                    if let Some(stream) = incoming {
                        state.bgm[next_active] = Some(stream);
                        state.active_bgm = next_active;
                    }
                }
                Command::FadeOut { fade_ms } => {
                    for stream in state.bgm.iter_mut().flatten() {
                        if fade_ms > 0 {
                            stream.fade = Some(Fade {
                                step_per_frame: 1000.0 / (fade_ms as f32 * OUTPUT_RATE as f32),
                                target: 0.0,
                                removes_at_target: true,
                            });
                        } else {
                            stream.volume = 0.0;
                            stream.fade = None;
                        }
                    }
                }
                Command::Stop => {
                    state.bgm = [None, None];
                    state.active_bgm = 0;
                }
                Command::PlayChannel {
                    channel,
                    clip,
                    looping,
                } => {
                    if let Some(slot) = state.channels.get_mut(usize::from(channel)) {
                        *slot = Some(Worker::start_stream(clip, looping, 0, false));
                    }
                }
                Command::StopChannel { channel } => {
                    if let Some(slot) = state.channels.get_mut(usize::from(channel)) {
                        *slot = None;
                    }
                }
                Command::Suspend(paused) => suspended = paused,
                Command::Shutdown => return record_closed(&failure),
            }
        }
        if cancelled.load(Ordering::Acquire) {
            return record_closed(&failure);
        }
        mix.iter_mut().for_each(|sample| *sample = 0.0);
        for stream in state.bgm.iter_mut().flatten() {
            mix_stream(stream, &mut mix);
        }
        for (slot, stream) in state.channels.iter_mut().enumerate() {
            if let Some(stream) = stream.as_mut() {
                mix_stream(stream, &mut mix);
                counters[slot].fetch_add(CHUNK_FRAMES as u64, Ordering::Release);
            }
        }
        // Retire finished streams after mixing this chunk.
        state.bgm[0] = state.bgm[0].take().filter(|s| !s.finished());
        state.bgm[1] = state.bgm[1].take().filter(|s| !s.finished());
        for slot in state.channels.iter_mut() {
            *slot = slot.take().filter(|s| !s.finished());
        }
        match output.write(mix, &cancelled) {
            Ok(Some(recycled)) => mix = recycled,
            Ok(None) => return,
            Err(cause) => {
                if !cancelled.load(Ordering::Acquire) {
                    record_failure(&failure, &cause.to_string());
                }
                return;
            }
        }
    }
}

impl Stream {
    fn finished(&self) -> bool {
        // A faded-out stream is retired by the fade application itself; a
        // non-looping clip ends when its position passes the final frame.
        self.volume <= 0.0 && self.fade.as_ref().is_some_and(|f| f.removes_at_target)
            || (!self.looping && self.position >= self.clip.frames.len() as f64 / 2.0)
    }
}

fn mix_stream(stream: &mut Stream, mix: &mut [f32]) {
    let frames = stream.clip.frames.as_slice();
    let source_frames = (frames.len() / 2) as f64;
    let step = f64::from(stream.clip.sample_rate) / f64::from(OUTPUT_RATE);
    for frame in 0..CHUNK_FRAMES {
        let base = frame * 2;
        if !stream.looping && stream.position >= source_frames {
            break;
        }
        let index = stream.position.floor() as usize;
        let frac = (stream.position - index as f64) as f32;
        let left = sample_linear(frames, index, 0, frac);
        let right = sample_linear(frames, index, 1, frac);
        if let Some(fade) = stream.fade.as_ref() {
            stream.volume = if fade.target > stream.volume {
                (stream.volume + fade.step_per_frame).min(fade.target)
            } else {
                (stream.volume - fade.step_per_frame).max(fade.target)
            };
        }
        let gain = stream.volume;
        mix[base] += left * gain;
        mix[base + 1] += right * gain;
        stream.position += step;
        if stream.looping && stream.position >= source_frames {
            stream.position -= source_frames;
        }
    }
}

fn sample_linear(frames: &[f32], index: usize, channel: usize, frac: f32) -> f32 {
    let current = frames.get(index * 2 + channel).copied().unwrap_or_default();
    let next = frames
        .get((index + 1) * 2 + channel)
        .copied()
        .unwrap_or_default();
    current + (next - current) * frac
}

fn record_closed(failure: &Arc<Mutex<Option<String>>>) {
    let _ = failure;
}

fn record_failure(failure: &Arc<Mutex<Option<String>>>, message: &str) {
    if let Ok(mut slot) = failure.lock() {
        if slot.is_none() {
            *slot = Some(message.to_string());
        }
    }
}

#[cfg(test)]
pub(super) fn test_output() -> (AudioBridge, roukaku_platform_common::NativeAudioConsumer) {
    let (producer, consumer, _) = roukaku_platform_common::NativeAudioQueue::create(
        1,
        CHUNK_FRAMES * 2,
        roukaku_platform::AudioWakeRegistration::default(),
    )
    .unwrap();
    struct Queue(roukaku_platform_common::NativeAudioProducer);
    impl Output for Queue {
        fn check(&self) -> SessionResult<()> {
            Ok(())
        }
        fn write(
            &mut self,
            samples: Vec<f32>,
            stop: &AtomicBool,
        ) -> SessionResult<Option<Vec<f32>>> {
            self.0
                .wait_for_capacity(samples.len(), stop)
                .map_err(|_| error("QUEUE", "queue wait failed"))?;
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            self.0
                .submit(samples)
                .map(Some)
                .map_err(|_| error("QUEUE", "queue submit failed"))
        }
    }
    (
        AudioBridge::start(move |_| Ok(Queue(producer))).unwrap(),
        consumer,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use roukaku_platform::AudioWakeRegistration;
    use roukaku_platform_common::{NativeAudioProducer, NativeAudioQueue};
    use std::time::{Duration, Instant};
    struct QueueOutput {
        producer: NativeAudioProducer,
        writes: Arc<AtomicU64>,
    }
    impl Output for QueueOutput {
        fn check(&self) -> SessionResult<()> {
            Ok(())
        }
        fn write(
            &mut self,
            samples: Vec<f32>,
            stop: &AtomicBool,
        ) -> SessionResult<Option<Vec<f32>>> {
            self.writes.fetch_add(1, Ordering::Release);
            self.producer
                .wait_for_capacity(samples.len(), stop)
                .map_err(|_| error("QUEUE", "queue wait failed"))?;
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            self.producer
                .submit(samples)
                .map(Some)
                .map_err(|_| error("QUEUE", "queue submit failed"))
        }
    }
    fn clip(frames: usize) -> DecodedClip {
        DecodedClip {
            sample_rate: OUTPUT_RATE,
            frames: Arc::new(vec![0.5; frames * 2]),
        }
    }
    #[test]
    fn native_crossfade_and_fade_out_mix_real_pcm() {
        let mut stream = Worker::start_stream(clip(48000), true, 100, true);
        let mut mix = vec![0.0; CHUNK_FRAMES * 2];
        mix_stream(&mut stream, &mut mix);
        assert!(mix[0] > 0.0 && mix[0] < mix[mix.len() - 1]);
        assert!((mix[mix.len() - 1] - 0.5 * CHUNK_FRAMES as f32 / 4800.0).abs() < 0.0001);
        stream.fade = Some(Fade {
            step_per_frame: 1.0,
            target: 0.0,
            removes_at_target: true,
        });
        mix.fill(0.0);
        mix_stream(&mut stream, &mut mix);
        assert!(stream.finished());
        assert!(mix.iter().all(|sample| *sample == 0.0));
    }
    #[test]
    fn cancellation_unblocks_full_native_queue_and_close_joins_audio_owner() {
        let (producer, _consumer, _) =
            NativeAudioQueue::create(1, CHUNK_FRAMES * 2, AudioWakeRegistration::default())
                .unwrap();
        let writes = Arc::new(AtomicU64::new(0));
        let observed = writes.clone();
        let mut audio = AudioBridge::start(move |_| Ok(QueueOutput { producer, writes })).unwrap();
        audio.play_crossfade(clip(48000), None, 0, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while observed.load(Ordering::Acquire) < 2 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        audio.cancellation_flag().store(true, Ordering::Release);
        audio.close().unwrap();
        assert!(audio.worker.is_none());
        let stopped = observed.load(Ordering::Acquire);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(observed.load(Ordering::Acquire), stopped);
    }
}
