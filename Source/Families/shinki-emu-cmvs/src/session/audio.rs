//! CMVS-private software audio worker: mixes the native crossfade BGM pair
//! and the channel sound table into the native output device.
//!
//! The native engine drives DirectSound through two alternating BGM
//! channels (`0x222A`/`0x222D`, blend = `(elapsed << 8) / duration`, full
//! volume 256), a channel-sound table and a voice channel. The worker owns
//! a fixed 48 kHz stereo f32 mix bus, opens the output on its own thread and
//! blocks on the device queue so the device callback never calls back into
//! the Family.
use crate::family::error;
use shinki_emu_sdk::NativeAudioOutput;
use shinki_emu_sdk::runtime::{SessionError, SessionResult};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
mod static_sounds;
mod streams;
#[cfg(test)]
mod test_playback;
pub(super) use streams::{
    FIXED_STREAM_CHANNEL, NativeStreamControl, NativeStreamMarker, NativeStreamSnapshot,
};

const OUTPUT_RATE: u32 = 48_000;
const CHUNK_FRAMES: usize = 2_400;
// Streaming bank592 slot15 uses bus39; static bank608 slots0..23 use15..38.
const CHANNEL_SOUNDS: usize = 40;
/// Upper bound on one decoded clip: ten minutes at the source rate.
pub(super) const MAX_CLIP_FRAMES: usize = 48_000 * 600;

/// Decoded source and optional native notification state. The retained output
/// buffer contains interleaved stereo f32 frames at the worker's input rate.
pub(super) struct DecodedClip {
    sample_rate: u32,
    frames: Arc<Vec<f32>>,
    source: Option<String>,
    native_channels: u16,
    indeterminate_tail: bool,
    native_restart_rewinds: bool,
    native_repeat: Option<Vec<f32>>,
    native_pcm: Option<Box<streams::native_pcm::NativePcm>>,
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
            source: None,
            native_channels: 2,
            indeterminate_tail: false,
            native_restart_rewinds: true,
            native_repeat: None,
            native_pcm: None,
        }
    }

    pub(super) fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    pub(super) fn with_native_channels(mut self, channels: u16) -> Self {
        self.native_channels = channels;
        self
    }

    pub(super) fn native_channels(&self) -> u16 {
        self.native_channels
    }

    pub(super) fn source_sample_rate(&self) -> u32 {
        self.native_pcm
            .as_ref()
            .map_or(self.sample_rate, |pcm| pcm.source_rate())
    }

    #[cfg(test)]
    pub(super) fn with_indeterminate_tail(mut self) -> Self {
        self.indeterminate_tail = true;
        self
    }

    pub(super) fn with_native_restart_rewinds(mut self, rewinds: bool) -> Self {
        self.native_restart_rewinds = rewinds;
        self
    }

    pub(super) fn with_native_repeat(
        mut self,
        data: Option<kira::sound::static_sound::StaticSoundData>,
    ) -> Self {
        self.native_repeat = data.map(|data| {
            let mut frames = Vec::with_capacity(data.frames.len() * 2);
            for frame in data.frames.iter() {
                frames.push(frame.left);
                frames.push(frame.right);
            }
            frames
        });
        self
    }

    pub(super) fn with_native_stretch(
        mut self,
        state: crate::audio::native_stretch::NativeMono175,
    ) -> Self {
        let reuse_buffer = self.native_pcm.is_some();
        let mut pcm = if let Some(mut pcm) = self.native_pcm.take() {
            pcm.attach_stretch(state);
            pcm
        } else {
            Box::new(streams::native_pcm::NativePcm::stretched(
                state,
                self.frames.as_slice(),
            ))
        };
        if let Some(repeat) = self.native_repeat.take() {
            pcm.set_repeat(&repeat);
        }
        self.sample_rate = pcm.device_rate();
        self.native_channels = 1;
        if reuse_buffer {
            let frames = Arc::get_mut(&mut self.frames).expect("exclusive native PCM buffer");
            frames.clear();
            let capacity = pcm.buffer_capacity();
            if frames.capacity() < capacity {
                frames.reserve_exact(capacity);
            }
        } else {
            self.frames = Arc::new(Vec::with_capacity(pcm.buffer_capacity()));
        }
        self.native_pcm = Some(pcm);
        self
    }

    pub(super) fn with_native_format(mut self, device_rate: u32, native_chunk_bytes: u32) -> Self {
        let mut pcm = Box::new(streams::native_pcm::NativePcm::raw(
            self.sample_rate,
            self.native_channels,
            device_rate,
            native_chunk_bytes,
            self.frames.as_slice(),
        ));
        if let Some(repeat) = self.native_repeat.take() {
            pcm.set_repeat(&repeat);
        }
        self.sample_rate = pcm.device_rate();
        self.frames = Arc::new(Vec::with_capacity(pcm.buffer_capacity()));
        self.native_pcm = Some(pcm);
        self
    }
}

enum Command {
    Crossfade {
        primary: DecodedClip,
        secondary: Option<DecodedClip>,
        fade_ms: u32,
        playback_flag: bool,
        native_gain: u32,
    },
    FadeOut {
        fade_ms: u32,
    },
    Stop,
    PlayChannel {
        channel: u8,
        generation: u64,
        clip: DecodedClip,
        looping: bool,
        native_gain: u32,
    },
    StopChannel {
        channel: u8,
    },
    RestoreNative {
        bgm: Box<[Option<Stream>; 2]>,
        channels: Box<[Option<Stream>; CHANNEL_SOUNDS]>,
    },
    ConfigureNativeChannel {
        channel: usize,
        stream: Box<Stream>,
    },
    ApplyNativeGains {
        bgm: Option<u32>,
        channels: [Option<u32>; 15],
    },
    StreamControl {
        channel: u8,
        generation: u64,
        control: NativeStreamControl,
    },
    StreamSnapshot {
        channel: u8,
        observe_position: bool,
        reply: std::sync::mpsc::Sender<NativeStreamSnapshot>,
    },
    Static(static_sounds::StaticCommand),
    Suspend(bool),
    Shutdown,
}

struct Stream {
    clip: DecodedClip,
    /// Channel play request owning this stream; zero for the BGM pair.
    generation: u64,
    /// Play position in source frames (fractional for cross-rate reads).
    position: f64,
    looping: bool,
    playing: bool,
    continuation: Option<DecodedClip>,
    /// Native48C8B0 gain word, independent of the fade envelope.
    native_gain: u32,
    volume: f32,
    stereo_gain: [f32; 2],
    /// Per-sample linear fade step and target, or `None` when steady.
    fade: Option<Fade>,
    native_volume: u32,
    native_pan: i32,
    /// Static DirectSound buffers have their own amplitude law.
    native_mixer: bool,
    native_fade: Option<streams::NativeFade>,
    output_frames: u64,
    decoded_frames: f64,
    level_ring: [[u32; 2]; 3],
    level_index: usize,
    marker: NativeStreamMarker,
    pending_output_frames: u32,
    playback: streams::progress::PlaybackTimeline,
}

struct Fade {
    step_per_frame: f32,
    target: f32,
    removes_at_target: bool,
}

type AudioFailure = Arc<Mutex<Option<SessionError>>>;

pub(super) struct AudioBridge {
    commands: std::sync::mpsc::Sender<Command>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Shared with the output device and every session decode; the device
    /// sets it on failure so blocked writers and decoders unwind.
    cancelled: Arc<AtomicBool>,
    failure: AudioFailure,
    playback: streams::progress::PlaybackClock,
    pending_drains: streams::progress::PendingDrains,
    /// Output frames written per channel sound slot; the movie player reads
    /// slot 2 as its audio clock (`SndSlot_positionMs`).
    channel_frames_written: Arc<[AtomicU64; CHANNEL_SOUNDS]>,
    /// Nonzero request generation means a channel play is queued or playing.
    channel_generations: Arc<[AtomicU64; CHANNEL_SOUNDS]>,
    next_generation: u64,
    native: super::native_save_audio::NativeAudioCapture,
    native_source_missing: bool,
}

pub(super) struct NativeChannelPlayback {
    pub slot: usize,
    pub clip: DecodedClip,
    pub looping: bool,
    pub playing: bool,
    pub native_gain: u32,
    pub native_volume: u32,
    pub native_pan: i32,
    pub volume: f32,
    pub stereo_gain: [f32; 2],
}

pub(super) struct PreparedNativePlayback {
    bgm: [Option<Stream>; 2],
    channels: Box<[Option<Stream>; CHANNEL_SOUNDS]>,
}

impl PreparedNativePlayback {
    pub(super) fn new(
        [primary, secondary]: [Option<DecodedClip>; 2],
        bgm_gain: u32,
        channels: Vec<NativeChannelPlayback>,
    ) -> Self {
        let incoming = primary.map(|clip| {
            let mut stream = Worker::start_stream(clip, secondary.is_none(), 0, false, bgm_gain);
            stream.set_continuation(secondary);
            stream
        });
        let mut slots = Box::new(std::array::from_fn(|_| None));
        for channel in channels {
            let mut stream =
                Worker::start_stream(channel.clip, channel.looping, 0, false, channel.native_gain);
            stream.volume = channel.volume;
            stream.stereo_gain = channel.stereo_gain;
            stream.playing = channel.playing;
            stream.native_volume = channel.native_volume;
            stream.native_pan = channel.native_pan;
            stream.native_mixer =
                channel.slot < 15 || channel.slot == usize::from(FIXED_STREAM_CHANNEL);
            slots[channel.slot] = Some(stream);
        }
        Self {
            bgm: [incoming, None],
            channels: slots,
        }
    }
}

/// The worker's device writer. The playback clock is a separate shared view
/// so it stays readable while the writer blocks.
trait Output {
    fn check(&self) -> SessionResult<()>;
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>>;
    /// Makes every accepted frame audible before the worker idles.
    fn flush(&mut self, stop: &AtomicBool) -> SessionResult<Option<()>>;
}

impl Output for NativeAudioOutput {
    fn check(&self) -> SessionResult<()> {
        NativeAudioOutput::check(self)
    }
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
        NativeAudioOutput::write(self, samples, stop)
    }
    fn flush(&mut self, stop: &AtomicBool) -> SessionResult<Option<()>> {
        NativeAudioOutput::flush(self, stop)
    }
}

impl AudioBridge {
    pub(super) fn new() -> SessionResult<Self> {
        Self::start(|stop| {
            let output = NativeAudioOutput::open(stop)?;
            let clock: streams::progress::PlaybackClock = Arc::new(output.progress_reader());
            Ok((output, clock))
        })
    }

    fn start<W: Output + 'static>(
        open: impl FnOnce(Arc<AtomicBool>) -> SessionResult<(W, streams::progress::PlaybackClock)>
        + Send
        + 'static,
    ) -> SessionResult<Self> {
        let pending_drains: streams::progress::PendingDrains =
            Arc::new(Mutex::new([None; CHANNEL_SOUNDS]));
        let worker_drains = Arc::clone(&pending_drains);
        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let failure: AudioFailure = Arc::new(Mutex::new(None));
        let channel_frames_written: Arc<[AtomicU64; CHANNEL_SOUNDS]> =
            Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let channel_generations: Arc<[AtomicU64; CHANNEL_SOUNDS]> =
            Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_failure = Arc::clone(&failure);
        let worker_counters = Arc::clone(&channel_frames_written);
        let worker_generations = Arc::clone(&channel_generations);
        let (ready, startup) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("cmvs-audio-worker".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (mut output, clock) = match open(Arc::clone(&worker_cancelled)) {
                        Ok(opened) => opened,
                        Err(cause) => {
                            let _ = ready.send(Err(cause));
                            return;
                        }
                    };
                    if ready.send(Ok(Arc::clone(&clock))).is_err() {
                        return;
                    }
                    let state = Worker {
                        bgm: [None, None],
                        active_bgm: 0,
                        channels: std::array::from_fn(|_| None),
                        static_voices: std::array::from_fn(|_| Vec::new()),
                        retired_stream_markers: [NativeStreamMarker::default(); CHANNEL_SOUNDS],
                    };
                    run_worker(
                        &mut output,
                        rx,
                        state,
                        &worker_cancelled,
                        &worker_failure,
                        &worker_counters,
                        &worker_generations,
                        clock.as_ref(),
                        &worker_drains,
                    );
                    if let Err(cause) = output.check() {
                        record_error(&worker_failure, cause);
                    }
                }));
                if result.is_err() {
                    record_failure(&worker_failure, "CMVS audio worker panicked");
                }
                for generation in worker_generations.iter() {
                    generation.store(0, Ordering::Release);
                }
            })
            .map_err(|_| {
                error(
                    "SHINKI_EMU_CMVS_AUDIO_WORKER",
                    "CMVS audio worker thread could not start",
                )
            })?;
        let playback = match startup.recv() {
            Ok(Ok(clock)) => clock,
            Ok(Err(cause)) => {
                let _ = worker.join();
                return Err(cause);
            }
            Err(_) => {
                let _ = worker.join();
                return Err(error(
                    "SHINKI_EMU_CMVS_AUDIO_START",
                    "CMVS audio worker exited during startup",
                ));
            }
        };
        Ok(Self {
            commands: tx,
            worker: Some(worker),
            cancelled,
            playback,
            pending_drains,
            failure,
            channel_frames_written,
            channel_generations,
            next_generation: 0,
            native: super::native_save_audio::NativeAudioCapture::default(),
            native_source_missing: false,
        })
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    /// Pauses mixing while the window is suspended; queued audio drains first.
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

    /// Bit i is set while slot i has an accepted play queued or playing.
    pub(super) fn playing_channels(&self) -> SessionResult<u16> {
        self.check_failure()?;
        streams::progress::retire_drained(
            self.playback.as_ref(),
            &self.pending_drains,
            &self.channel_generations,
        )?;
        let mut playing = 0;
        for (slot, generation) in self.channel_generations.iter().take(9).enumerate() {
            if generation.load(Ordering::Acquire) != 0 {
                playing |= 1 << slot;
            }
        }
        self.check_failure()?;
        Ok(playing)
    }

    pub(super) fn play_crossfade(
        &mut self,
        primary: DecodedClip,
        secondary: Option<DecodedClip>,
        fade_ms: u32,
        playback_flag: bool,
        native_gain: u32,
    ) -> SessionResult<()> {
        self.check_failure()?;
        let source_missing = primary.source.is_none()
            || secondary.as_ref().is_some_and(|clip| clip.source.is_none());
        let sources = crate::system_save::slot_globals::NativeBgmSources {
            primary: primary.source.clone().unwrap_or_default(),
            secondary: secondary
                .as_ref()
                .and_then(|clip| clip.source.clone())
                .unwrap_or_default(),
        };
        self.submit(Command::Crossfade {
            primary,
            secondary,
            fade_ms,
            playback_flag,
            native_gain,
        })?;
        self.native.bgm = sources;
        self.native_source_missing = source_missing;
        Ok(())
    }

    pub(super) fn fade_out(&mut self, fade_ms: u32) -> SessionResult<()> {
        self.submit(Command::FadeOut { fade_ms })?;
        self.native.bgm = Default::default();
        self.native_source_missing = false;
        Ok(())
    }

    pub(super) fn stop(&mut self) -> SessionResult<()> {
        self.submit(Command::Stop)?;
        self.native.bgm = Default::default();
        self.native_source_missing = false;
        Ok(())
    }

    pub(super) fn play_channel(
        &mut self,
        channel: u8,
        clip: DecodedClip,
        looping: bool,
        native_gain: u32,
    ) -> SessionResult<()> {
        self.check_failure()?;
        let generation = self.publish_generation(channel);
        let result = self.submit(Command::PlayChannel {
            channel,
            generation,
            clip,
            looping,
            native_gain,
        });
        if result.is_err() {
            retire_channel(&self.channel_generations, channel, generation);
        }
        result
    }

    pub(super) fn stop_channel(&mut self, channel: u8) -> SessionResult<()> {
        self.check_failure()?;
        if let Some(slot) = self.channel_generations.get(usize::from(channel)) {
            slot.store(0, Ordering::Release);
        }
        self.submit(Command::StopChannel { channel })
    }

    fn submit(&self, command: Command) -> SessionResult<()> {
        self.check_failure()?;
        self.commands.send(command).map_err(|_| {
            record_failure(
                &self.failure,
                "CMVS audio worker stopped accepting commands",
            );
            error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "CMVS audio worker stopped accepting commands",
            )
        })
    }

    pub(in crate::session) fn check_failure(&self) -> SessionResult<()> {
        let failure = self.failure.lock().map_err(|_| {
            error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "CMVS audio failure slot is poisoned",
            )
        })?;
        match failure.as_ref() {
            Some(failure) => Err(failure.clone()),
            None if self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
                && !self.cancelled.load(Ordering::Acquire) =>
            {
                Err(error(
                    "SHINKI_EMU_CMVS_AUDIO_WORKER",
                    "CMVS audio worker exited unexpectedly",
                ))
            }
            None => Ok(()),
        }
    }

    /// Cancel outstanding requests, drain the queue and join the worker.
    /// Any close error is still returned after cleanup completes.
    pub(super) fn close(&mut self) -> SessionResult<()> {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Shutdown);
        let mut result = self.check_failure();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            record_failure(&self.failure, "CMVS audio worker panicked");
        }
        for generation in self.channel_generations.iter() {
            generation.store(0, Ordering::Release);
        }
        if result.is_ok() {
            result = self.check_failure();
        }
        result
    }
    pub(super) fn capture_native_audio(
        &self,
        globals: &mut crate::system_save::slot_globals::NativeGlobals,
    ) -> SessionResult<()> {
        self.check_failure()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "native audio capture requested after shutdown",
            ));
        }
        if self.native_source_missing {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_SOURCE",
                "live BGM has no native source name",
            ));
        }
        self.native.write_globals(globals);
        Ok(())
    }

    pub(super) fn commit_native_audio(
        &mut self,
        mut playback: PreparedNativePlayback,
        capture: super::native_save_audio::NativeAudioCapture,
    ) -> SessionResult<()> {
        for channel in (5..15).chain(std::iter::once(usize::from(FIXED_STREAM_CHANNEL))) {
            self.channel_generations[channel].store(0, Ordering::Release);
            if let Some(stream) = playback.channels[channel].as_mut().filter(|s| s.playing) {
                stream.begin_generation(self.publish_generation(channel as u8));
            }
        }
        self.submit(Command::RestoreNative {
            bgm: Box::new(playback.bgm),
            channels: playback.channels,
        })?;
        self.native = capture;
        self.native_source_missing = false;
        Ok(())
    }

    pub(super) fn configure_native_resource(
        &mut self,
        record: crate::system_save::slot_globals::NativeResourceChannel,
        playback: NativeChannelPlayback,
        fade_in_ms: u32,
    ) -> SessionResult<()> {
        if !(9..15).contains(&playback.slot) {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
                "resource stream slot must be9..14",
            ));
        }
        let index = playback.slot - 9;
        let mut stream = Worker::start_stream(
            playback.clip,
            playback.looping,
            0,
            false,
            playback.native_gain,
        );
        stream.playing = playback.playing;
        stream.volume = playback.volume;
        stream.stereo_gain = playback.stereo_gain;
        stream.native_volume = playback.native_volume;
        stream.native_pan = playback.native_pan;
        if stream.playing {
            stream.begin_generation(self.publish_generation(playback.slot as u8));
            if fade_in_ms != 0 {
                stream.native_fade = Some(streams::NativeFade::new(fade_in_ms, true));
            }
        } else {
            self.channel_generations[playback.slot].store(0, Ordering::Release);
        }
        let generation = stream.generation;
        if let Err(cause) = self.submit(Command::ConfigureNativeChannel {
            channel: playback.slot,
            stream: Box::new(stream),
        }) {
            retire_channel(&self.channel_generations, playback.slot as u8, generation);
            return Err(cause);
        }
        self.native.resource_channels.slots[index] = record;
        Ok(())
    }

    pub(super) fn native_resource_volume(&self, slot: usize) -> u32 {
        self.native.resource_channels.slots[slot].volume
    }

    pub(super) fn apply_native_gains(
        &mut self,
        bgm: Option<u32>,
        channels: [Option<u32>; 15],
    ) -> SessionResult<()> {
        self.submit(Command::ApplyNativeGains { bgm, channels })
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
    static_voices: [Vec<Stream>; static_sounds::STATIC_COUNT],
    retired_stream_markers: [NativeStreamMarker; CHANNEL_SOUNDS],
}

impl Worker {
    fn any_active(&self) -> bool {
        self.bgm
            .iter()
            .chain(self.channels.iter())
            .flatten()
            .any(|stream| stream.playing || stream.native_fade.is_some())
            || self.static_voices.iter().any(|voices| !voices.is_empty())
    }

    fn start_stream(
        mut clip: DecodedClip,
        looping: bool,
        fade_ms: u32,
        fade_in: bool,
        native_gain: u32,
    ) -> Stream {
        if let Some(pcm) = clip.native_pcm.as_mut() {
            pcm.start(looping);
        }
        let volume = if fade_in && fade_ms != 0 { 0.0 } else { 1.0 };
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
            generation: 0,
            position: 0.0,
            looping,
            playing: true,
            continuation: None,
            native_gain: if native_gain <= 256 { native_gain } else { 0 },
            volume,
            stereo_gain: [1.0, 1.0],
            fade,
            native_volume: 256,
            native_pan: 0,
            native_mixer: true,
            native_fade: None,
            output_frames: 0,
            decoded_frames: 0.0,
            level_ring: [[0; 2]; 3],
            level_index: 0,
            marker: NativeStreamMarker::default(),
            pending_output_frames: 0,
            playback: streams::progress::PlaybackTimeline::default(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    output: &mut impl Output,
    commands: std::sync::mpsc::Receiver<Command>,
    mut state: Worker,
    cancelled: &AtomicBool,
    failure: &AudioFailure,
    counters: &[AtomicU64; CHANNEL_SOUNDS],
    generations: &[AtomicU64; CHANNEL_SOUNDS],
    playback: &dyn streams::progress::PlaybackSource,
    pending_drains: &streams::progress::PendingDrains,
) {
    let mut mix = vec![0.0_f32; CHUNK_FRAMES * 2];
    let mut accepted_bus_frames = 0u64;
    let mut suspended = false;
    let mut unflushed = false;
    loop {
        // Command intake: drain everything queued, then wait when idle.
        loop {
            let command = if state.any_active() && !suspended {
                match commands.try_recv() {
                    Ok(command) => command,
                    Err(_) => break,
                }
            } else {
                if unflushed {
                    // A voice's end frame becomes audible only once the
                    // device's partial block and converter tail are pushed.
                    match output.flush(cancelled) {
                        Ok(Some(())) => {}
                        Ok(None) => return record_closed(failure, cancelled),
                        Err(cause) => return record_error(failure, cause),
                    }
                    accepted_bus_frames = playback.progress().accepted_frames;
                    unflushed = false;
                }
                match commands.recv() {
                    Ok(command) => command,
                    Err(_) => return record_closed(failure, cancelled),
                }
            };
            match command {
                Command::Crossfade {
                    primary,
                    secondary,
                    fade_ms,
                    playback_flag,
                    native_gain,
                } => {
                    // Native 48DFE0 feeds an intro and looping continuation
                    // into one decoder; the other BGM slot is the outgoing song.
                    let incoming = if playback_flag {
                        let mut stream = Worker::start_stream(
                            primary,
                            secondary.is_none(),
                            fade_ms,
                            true,
                            native_gain,
                        );
                        stream.set_continuation(secondary);
                        Some(stream)
                    } else {
                        None
                    };
                    if fade_ms == 0 {
                        state.bgm = [None, None];
                    } else if let Some(outgoing) = state.bgm[state.active_bgm].as_mut() {
                        outgoing.fade = if fade_ms > 0 {
                            Some(Fade {
                                step_per_frame: 1000.0 / (fade_ms as f32 * OUTPUT_RATE as f32),
                                target: 0.0,
                                removes_at_target: true,
                            })
                        } else {
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
                    if fade_ms == 0 {
                        state.bgm = [None, None];
                    }
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
                    generation,
                    clip,
                    looping,
                    native_gain,
                } => {
                    if let Some(slot) = state.channels.get_mut(usize::from(channel)) {
                        let mut stream = Worker::start_stream(clip, looping, 0, false, native_gain);
                        stream.begin_generation(generation);
                        *slot = Some(stream);
                    }
                }
                Command::StopChannel { channel } => {
                    if let Some(slot) = state.channels.get_mut(usize::from(channel))
                        && let Some(stream) = slot.take()
                    {
                        state.retired_stream_markers[usize::from(channel)] =
                            stream.terminal_marker(accepted_bus_frames);
                    }
                }
                Command::RestoreNative { bgm, channels } => {
                    state.bgm = *bgm;
                    state.active_bgm = 0;
                    for voices in &mut state.static_voices {
                        voices.clear();
                    }
                    for (index, stream) in (*channels).into_iter().enumerate() {
                        if index >= 5 {
                            if let Some(old) = state.channels[index].as_ref() {
                                state.retired_stream_markers[index] =
                                    old.terminal_marker(accepted_bus_frames);
                            }
                            state.channels[index] = stream;
                        }
                    }
                }
                Command::ConfigureNativeChannel { channel, stream } => {
                    state.channels[channel] = Some(*stream);
                }
                Command::StreamControl {
                    channel,
                    generation,
                    control,
                } => {
                    state.control_stream(
                        channel,
                        generation,
                        control,
                        generations,
                        accepted_bus_frames,
                    );
                }
                Command::StreamSnapshot {
                    channel,
                    observe_position,
                    reply,
                } => {
                    let snapshot = state.stream_snapshot(
                        channel,
                        accepted_bus_frames,
                        observe_position,
                        playback,
                    );
                    let _ = reply.send(snapshot);
                }
                Command::ApplyNativeGains { bgm, channels } => {
                    if let Some(gain) = bgm {
                        for stream in state.bgm.iter_mut().flatten() {
                            stream.set_native_gain(gain);
                        }
                    }
                    for (slot, gain) in channels.into_iter().enumerate() {
                        if let (Some(gain), Some(stream)) = (gain, state.channels[slot].as_mut()) {
                            stream.set_native_gain(gain);
                        }
                    }
                }
                Command::Static(command) => state.static_command(command),
                Command::Suspend(paused) => suspended = paused,
                Command::Shutdown => return record_closed(failure, cancelled),
            }
        }
        if cancelled.load(Ordering::Acquire) {
            return record_closed(failure, cancelled);
        }
        mix.clear();
        mix.resize(CHUNK_FRAMES * 2, 0.0);
        let chunk_frames = state
            .bgm
            .iter_mut()
            .chain(state.channels.iter_mut())
            .flatten()
            .filter_map(|stream| stream.prepare_for_mix(CHUNK_FRAMES))
            .min()
            .unwrap_or(CHUNK_FRAMES);
        if chunk_frames == 0 {
            return record_error(
                failure,
                error(
                    "SHINKI_EMU_CMVS_NATIVE_AUDIO_STRETCH_INDETERMINATE",
                    "native time-stretch attempted to consume unwritten heap samples",
                ),
            );
        }
        mix.truncate(chunk_frames * 2);
        for stream in state.bgm.iter_mut().flatten() {
            mix_stream(stream, &mut mix);
        }
        for (slot, stream) in state.channels.iter_mut().enumerate() {
            if let Some(stream) = stream.as_mut() {
                if !stream.playing && stream.native_fade.is_none() {
                    continue;
                }
                mix_stream(stream, &mut mix);
                counters[slot].fetch_add(chunk_frames as u64, Ordering::Release);
            }
        }
        state.mix_static_duplicates(&mut mix);
        match output.write(mix, cancelled) {
            Ok(Some(recycled)) => {
                mix = recycled;
                unflushed = true;
                let bus_start_frame = accepted_bus_frames;
                accepted_bus_frames = accepted_bus_frames.wrapping_add(chunk_frames as u64);
                for stream in state
                    .bgm
                    .iter_mut()
                    .chain(state.channels.iter_mut())
                    .flatten()
                {
                    stream.accept_chunk(bus_start_frame);
                }
                state.bgm[0] = state.bgm[0].take().filter(|s| !s.finished());
                state.bgm[1] = state.bgm[1].take().filter(|s| !s.finished());
                for (channel, slot) in state.channels.iter_mut().enumerate() {
                    if slot.as_ref().is_some_and(Stream::finished) {
                        let stream = slot.as_mut().expect("finished stream");
                        if ((5..15).contains(&channel)
                            || channel == usize::from(FIXED_STREAM_CHANNEL))
                            && stream.generation != 0
                            && stream.marker.accepted_generation_frames != 0
                            && generations[channel].load(Ordering::Acquire) == stream.generation
                        {
                            let mut pending = match pending_drains.lock() {
                                Ok(pending) => pending,
                                Err(_) => {
                                    return record_failure(
                                        failure,
                                        "audio drain table is poisoned",
                                    );
                                }
                            };
                            pending[channel] = Some(streams::progress::PendingDrain {
                                generation: stream.generation,
                                end_frame: stream
                                    .marker
                                    .accepted_end_frame
                                    .expect("accepted stream end"),
                            });
                        } else {
                            retire_channel(generations, channel as u8, stream.generation);
                        }
                        if stream.fade_disposes() || channel < 5 {
                            state.retired_stream_markers[channel] =
                                stream.terminal_marker(accepted_bus_frames);
                            *slot = None;
                        } else {
                            stream.playing = false;
                            if !stream.native_mixer {
                                stream.position = 0.0;
                            }
                        }
                    }
                }
            }
            Ok(None) => return record_closed(failure, cancelled),
            Err(cause) => return record_error(failure, cause),
        }
    }
}

impl Stream {
    fn set_native_gain(&mut self, gain: u32) {
        //48C8B0 retains the previous gain when the unsigned request exceeds256.
        if gain <= 256 {
            self.native_gain = gain;
        }
    }

    fn native_mix_word(&self) -> u32 {
        let envelope = self.native_fade.as_ref().map_or_else(
            || (self.volume * 256.0).clamp(0.0, 256.0) as u32,
            streams::NativeFade::envelope,
        );
        self.native_gain
            .wrapping_mul(envelope)
            .wrapping_shr(8)
            .min(256)
    }

    fn finished(&self) -> bool {
        self.fade_disposes()
            || (self.playing
                && self.clip.native_pcm.as_ref().is_none_or(|pcm| pcm.finished)
                && !self.clip.indeterminate_tail
                && self.continuation.is_none()
                && self.position >= self.clip.frames.len() as f64 / 2.0)
    }
}

fn mix_stream(stream: &mut Stream, mix: &mut [f32]) {
    let mut levels = [0u32; 2];
    let was_playing = stream.playing;
    let fallback_levels = stream.clip.native_pcm.is_none();
    let mut gains = if stream.native_mixer {
        stream.native_channel_gains()
    } else {
        [0; 2]
    };
    let mut static_gain = if stream.native_mixer {
        0.0
    } else {
        let gain = stream.native_mix_word() as f32 / 256.0;
        gain * gain
    };
    let frame_count = mix.len() / 2;
    for frame in 0..frame_count {
        let native_fading = stream.native_fade.is_some();
        if let Some(fade) = stream.native_fade.as_mut() {
            fade.advance_frame();
            if fade.finished() {
                if fade.fade_in {
                    stream.native_fade = None;
                    stream.volume = 1.0;
                } else {
                    break;
                }
            }
        }
        if !stream.playing {
            continue;
        }
        let base = frame * 2;
        let source_frames = (stream.clip.frames.len() / 2) as f64;
        if stream.position >= source_frames {
            if let Some(continuation) = stream.continuation.take() {
                stream.clip = continuation;
                stream.position = 0.0;
                stream.looping = true;
            } else {
                break;
            }
        }
        let frames = stream.clip.frames.as_slice();
        let source_frames = (frames.len() / 2) as f64;
        let step = f64::from(stream.clip.sample_rate) / f64::from(OUTPUT_RATE);
        let index = stream.position.floor() as usize;
        let frac = (stream.position - index as f64) as f32;
        let samples = [
            sample_linear(frames, index, 0, frac),
            sample_linear(frames, index, 1, frac),
        ];
        if let Some(fade) = stream.fade.as_ref() {
            stream.volume = if fade.target > stream.volume {
                (stream.volume + fade.step_per_frame).min(fade.target)
            } else {
                (stream.volume - fade.step_per_frame).max(fade.target)
            };
        }
        if native_fading || stream.fade.is_some() {
            if stream.native_mixer {
                gains = stream.native_channel_gains();
            } else {
                let gain = stream.native_mix_word() as f32 / 256.0;
                static_gain = gain * gain;
            }
        }
        if stream.native_mixer {
            for channel in 0..2 {
                let sample = (samples[channel] * 32768.0).clamp(-32768.0, 32767.0) as i32;
                if fallback_levels {
                    levels[channel] = levels[channel].wrapping_add(sample.unsigned_abs());
                }
                let scaled = if gains[channel] < 256 {
                    (gains[channel].wrapping_mul(sample as u32) as i32) >> 8
                } else {
                    sample
                };
                let value = scaled as f32 / 32768.0 * stream.stereo_gain[channel];
                mix[base + channel] = (mix[base + channel] + value).clamp(-1.0, 32767.0 / 32768.0);
            }
        } else {
            for channel in 0..2 {
                mix[base + channel] += samples[channel] * static_gain * stream.stereo_gain[channel];
            }
        }
        stream.output_frames = stream.output_frames.wrapping_add(1);
        stream.pending_output_frames += 1;
        stream.decoded_frames += if stream.looping {
            step
        } else {
            step.min(source_frames - stream.position)
        };
        stream.position += step;
        if stream.looping
            && stream.clip.native_pcm.is_none()
            && !stream.clip.indeterminate_tail
            && stream.position >= source_frames
        {
            stream.position %= source_frames;
        }
    }
    if was_playing && fallback_levels {
        stream.level_ring[stream.level_index] = levels.map(|sum| sum / frame_count as u32);
        stream.level_index = (stream.level_index + 1) % 3;
    }
}

fn sample_linear(frames: &[f32], index: usize, channel: usize, frac: f32) -> f32 {
    let current = frames.get(index * 2 + channel).copied().unwrap_or_default();
    if frac == 0.0 {
        return current;
    }
    let next = frames
        .get((index + 1) * 2 + channel)
        .copied()
        .unwrap_or_default();
    current + (next - current) * frac
}

fn retire_channel(generations: &[AtomicU64; CHANNEL_SOUNDS], channel: u8, generation: u64) {
    if let Some(slot) = generations.get(usize::from(channel)) {
        let _ = slot.compare_exchange(generation, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

fn record_closed(failure: &AudioFailure, cancelled: &AtomicBool) {
    if !cancelled.load(Ordering::Acquire) {
        record_failure(
            failure,
            "native audio output or command queue closed unexpectedly",
        );
    }
}

fn record_failure(failure: &AudioFailure, message: &str) {
    record_error(failure, error("SHINKI_EMU_CMVS_AUDIO_WORKER", message));
}

fn record_error(failure: &AudioFailure, cause: SessionError) {
    if let Ok(mut slot) = failure.lock()
        && slot.is_none()
    {
        *slot = Some(cause);
    }
}

/// Session fixture output: a real-time paced headless device.
#[cfg(test)]
pub(super) fn test_output() -> AudioBridge {
    test_playback::PacedOutput::start(None)
}

#[cfg(test)]
pub(super) mod tests {
    use super::streams::progress::{PlaybackClock, PlaybackSource};
    use super::test_playback::{PacedOutput, TestPlayback};
    use super::*;
    use crate::{
        CmvsPs2aControlInstruction, CmvsPs2aInstructionFrame, CmvsPs2aSourceSpan, CmvsPs2aVmAction,
        CmvsPs2aVmState, CmvsResourceSearchBank,
    };
    use parking_lot::Mutex;
    use std::collections::BTreeMap;

    fn tone_clip(frames: usize, rate: u32) -> DecodedClip {
        let mut data = Vec::with_capacity(frames * 2);
        for index in 0..frames {
            let value = if index % 2 == 0 { 0.5 } else { -0.5 };
            data.push(value);
            data.push(value);
        }
        DecodedClip {
            sample_rate: rate,
            frames: Arc::new(data),
            source: None,
            native_channels: 2,
            indeterminate_tail: false,
            native_restart_rewinds: true,
            native_repeat: None,
            native_pcm: None,
        }
    }

    fn recording_output() -> (AudioBridge, Arc<Mutex<Vec<Vec<f32>>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        (PacedOutput::start(Some(Arc::clone(&recorded))), recorded)
    }

    #[test]
    fn crossfade_playback_writes_nonzero_samples_then_close_joins() {
        let (mut bridge, recorded) = recording_output();
        bridge
            .play_crossfade(tone_clip(9600, 48_000), None, 500, true, 256)
            .unwrap();
        // The worker writes 50 ms chunks; 200 ms makes the fade audible.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while recorded.lock().len() < 4 {
            assert!(
                std::time::Instant::now() < deadline,
                "worker wrote too few chunks"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        bridge.close().unwrap();
        assert!(
            recorded
                .lock()
                .iter()
                .any(|chunk| chunk.iter().any(|sample| sample.abs() > 0.01)),
            "crossfade playback must produce audible samples"
        );
    }

    #[test]
    fn fade_out_and_stop_retire_streams() {
        let (mut bridge, recorded) = recording_output();
        bridge
            .play_crossfade(tone_clip(480_000, 48_000), None, 0, true, 256)
            .unwrap();
        bridge.fade_out(0).unwrap();
        bridge.stop().unwrap();
        bridge.close().unwrap();
        // An immediate fade-out and stop write at most the in-flight chunk.
        let written = recorded.lock().len();
        assert!(written <= 2, "streams must retire promptly: {written}");
    }

    #[test]
    fn device_open_failure_is_returned_and_joins_the_worker() {
        let result = AudioBridge::start(|_| -> SessionResult<(PacedOutput, PlaybackClock)> {
            Err(error("SHINKI_EMU_AUDIO_DEVICE", "no device"))
        });
        assert_eq!(
            result.err().map(|cause| cause.code),
            Some("SHINKI_EMU_AUDIO_DEVICE".to_owned())
        );
    }

    #[test]
    fn suspend_stops_mixing_until_resumed() {
        let (mut bridge, recorded) = recording_output();
        bridge
            .play_crossfade(tone_clip(480_000, 48_000), None, 0, true, 256)
            .unwrap();
        let wait_for = |count: usize| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while recorded.lock().len() < count {
                assert!(std::time::Instant::now() < deadline, "worker stalled");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        wait_for(1);
        bridge.suspend(true).unwrap();
        // The suspend command lands within one in-flight chunk.
        std::thread::sleep(std::time::Duration::from_millis(120));
        let paused = recorded.lock().len();
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(recorded.lock().len(), paused);
        bridge.suspend(false).unwrap();
        wait_for(paused + 1);
        bridge.close().unwrap();
    }

    pub(in crate::session) enum OutputStep {
        Accept,
        Fail,
        Close,
    }

    struct OutputGate {
        entered: std::sync::mpsc::Sender<()>,
        steps: Mutex<std::sync::mpsc::Receiver<OutputStep>>,
        released: AtomicBool,
        chunks: Mutex<Vec<Vec<f32>>>,
        playback: Arc<TestPlayback>,
    }

    /// Blocks every write until the test releases it with an `OutputStep`.
    struct SteppedOutput(Arc<OutputGate>);

    impl Output for SteppedOutput {
        fn check(&self) -> SessionResult<()> {
            Ok(())
        }

        fn write(&mut self, samples: Vec<f32>, _: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
            let frames = samples.len() / 2;
            if self.0.released.load(Ordering::Acquire) {
                self.0.playback.accept(frames);
                return Ok(Some(samples));
            }
            self.0.chunks.lock().push(samples.clone());
            let _ = self.0.entered.send(());
            match self
                .0
                .steps
                .lock()
                .recv_timeout(std::time::Duration::from_secs(5))
            {
                Ok(OutputStep::Accept) => {
                    self.0.playback.accept(frames);
                    Ok(Some(samples))
                }
                Ok(OutputStep::Close) => Ok(None),
                Ok(OutputStep::Fail) | Err(_) => Err(error(
                    "SHINKI_EMU_CMVS_AUDIO_TEST",
                    "controlled audio output failure",
                )),
            }
        }

        fn flush(&mut self, _: &AtomicBool) -> SessionResult<Option<()>> {
            Ok(Some(()))
        }
    }

    pub(in crate::session) struct OutputControl {
        gate: Arc<OutputGate>,
        entered: std::sync::mpsc::Receiver<()>,
        pub(in crate::session) steps: std::sync::mpsc::Sender<OutputStep>,
    }

    impl OutputControl {
        pub(in crate::session) fn new() -> (AudioBridge, Self) {
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (steps_tx, steps_rx) = std::sync::mpsc::channel();
            let gate = Arc::new(OutputGate {
                entered: entered_tx,
                steps: Mutex::new(steps_rx),
                released: AtomicBool::new(false),
                chunks: Mutex::new(Vec::new()),
                playback: Arc::new(TestPlayback::default()),
            });
            let output = Arc::clone(&gate);
            let bridge = AudioBridge::start(move |_| {
                let clock: PlaybackClock = output.playback.clone();
                Ok((SteppedOutput(output), clock))
            })
            .unwrap();
            (
                bridge,
                Self {
                    gate,
                    entered: entered_rx,
                    steps: steps_tx,
                },
            )
        }

        pub(in crate::session) fn wait_for_write(&self) {
            self.entered
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("worker must reach the controlled output");
        }

        pub(in crate::session) fn accept_and_wait(&self) {
            self.steps.send(OutputStep::Accept).unwrap();
            self.wait_for_write();
        }

        pub(in crate::session) fn latest_pcm(&self) -> Vec<f32> {
            self.gate
                .chunks
                .lock()
                .last()
                .expect("recorded PCM chunk")
                .clone()
        }

        pub(in crate::session) fn consume_to(&self, frame: u64) {
            self.gate.playback.consume_to(frame);
        }

        pub(in crate::session) fn consume_all(&self) {
            self.gate.playback.consume_all();
        }

        pub(in crate::session) fn accepted_frames(&self) -> u64 {
            self.gate.playback.progress().accepted_frames
        }

        pub(in crate::session) fn played_frames(&self) -> u64 {
            self.gate.playback.progress().played_frames
        }

        pub(in crate::session) fn finish(&self, bridge: &mut AudioBridge) {
            self.gate.released.store(true, Ordering::Release);
            let _ = self.steps.send(OutputStep::Accept);
            bridge.close().unwrap();
            assert_eq!(bridge.playing_channels().unwrap(), 0);
        }
    }

    impl Drop for OutputControl {
        fn drop(&mut self) {
            self.gate.released.store(true, Ordering::Release);
            let _ = self.steps.send(OutputStep::Accept);
        }
    }

    fn apply_setting(vm: &mut CmvsPs2aVmState, bridge: &mut AudioBridge, command: u16, value: u32) {
        for word in [value, 1] {
            crate::execute_cmvs390_frame(
                vm,
                &CmvsPs2aInstructionFrame::Control(CmvsPs2aControlInstruction::PushImmediate {
                    span: CmvsPs2aSourceSpan {
                        offset: 0,
                        byte_length: 8,
                    },
                    stack_bytes: 4,
                    value: word,
                }),
            )
            .unwrap();
        }
        let action = crate::execute_cmvs390_frame(
            vm,
            &CmvsPs2aInstructionFrame::Command {
                span: CmvsPs2aSourceSpan {
                    offset: 0,
                    byte_length: 2,
                },
                command_id: command,
            },
        )
        .unwrap()
        .unwrap();
        let CmvsPs2aVmAction::ApplyNativeAudioGainSetting { command_id } = action else {
            panic!("setter must emit its native audio action");
        };
        super::super::native_save_audio::apply_gain_setting(vm, bridge, command_id).unwrap();
    }

    fn write_pcm_wave(path: &std::path::Path) {
        let frames = CHUNK_FRAMES * 8;
        let size = (frames * 4) as u32;
        let mut bytes = Vec::with_capacity(44 + size as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&OUTPUT_RATE.to_le_bytes());
        bytes.extend_from_slice(&(OUTPUT_RATE * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&size.to_le_bytes());
        for _ in 0..frames * 2 {
            bytes.extend_from_slice(&16384i16.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn decoded_native_reload_applies_current_gain_once_and_preserves_static_buffers() {
        let (root, archive, mut vm) = crate::scene::parent_load_tests::fixture();
        write_pcm_wave(&root.path().join("gain.wav"));
        for bank in [
            CmvsResourceSearchBank::CrossfadeAudio,
            CmvsResourceSearchBank::Native0d0c,
        ] {
            vm.resource_search_paths
                .insert(bank, BTreeMap::from([(0, String::new())]));
        }
        for source in 0..3 {
            vm.write_interpreter_word(1560, 128);
            vm.write_interpreter_word(1564, 128);
            vm.write_interpreter_word(1568, 128);
            let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
            match source {
                0 => globals.bgm.primary = "gain.wav".into(),
                1 => {
                    globals.resource_channels.slots[0] =
                        crate::system_save::slot_globals::NativeResourceChannel {
                            name: "gain.wav".into(),
                            loop_playback: 1,
                            restart_mode: 1,
                            volume: 128,
                            channel_volume: 128,
                            stereo_pan: 0,
                        }
                }
                _ => globals.static_sounds.entries.push(
                    crate::system_save::slot_globals::NativeStaticSound {
                        owner: 23,
                        name: "gain.wav".into(),
                        playing: 1,
                        volume: 128,
                    },
                ),
            }
            let prepared = super::super::native_save_audio::PreparedNativeAudio::prepare(
                &archive,
                &vm,
                &globals,
                [44100, 8192],
            )
            .unwrap();
            let (mut bridge, control) = OutputControl::new();
            prepared.commit(&mut bridge).unwrap();
            control.wait_for_write();
            let expected = match source {
                0 => 0.125,
                1 => 0.03125,
                _ => 0.0078125,
            };
            assert!(
                (control.latest_pcm()[0] - expected).abs() < 0.000001,
                "restored source{source}"
            );
            apply_setting(&mut vm, &mut bridge, 729, 256);
            control.accept_and_wait();
            let expected = match source {
                0 => 0.25,
                1 => 0.125,
                _ => 0.0078125,
            };
            assert!(
                (control.latest_pcm()[0] - expected).abs() < 0.000001,
                "live source{source}"
            );
            let mut captured = crate::system_save::slot_globals::NativeGlobals::default();
            bridge.capture_native_audio(&mut captured).unwrap();
            assert_eq!(captured.bgm, globals.bgm);
            assert_eq!(captured.resource_channels, globals.resource_channels);
            assert_eq!(captured.static_sounds, globals.static_sounds);
            control.finish(&mut bridge);
        }
    }

    #[test]
    fn session_native_gain_action_reaches_the_live_pcm_worker() {
        let (root, archive) = super::super::test_support::game();
        let mut session = super::super::test_support::open(root.path(), archive);
        let (mut bridge, control) = OutputControl::new();
        bridge
            .play_crossfade(
                tone_clip(CHUNK_FRAMES * 8, OUTPUT_RATE).with_source("session.ogg".into()),
                None,
                0,
                true,
                256,
            )
            .unwrap();
        control.wait_for_write();
        session.audio = Some(bridge);
        session.vm.interpreter_words.insert(81220, 0x1234);
        super::super::test_support::command(&mut session, 730, &[1, 128]);
        control.accept_and_wait();
        assert_eq!(control.latest_pcm()[0], 0.25);
        assert_eq!(session.vm.interpreter_words[&81220], 0x1234);
        //Getter ignores its second word and must not disturb playback.
        super::super::test_support::command(&mut session, 730, &[0, u32::MAX]);
        control.accept_and_wait();
        assert_eq!(control.latest_pcm()[0], 0.25);
        assert_eq!(session.vm.interpreter_words[&81220], 128);
        super::super::test_support::command(&mut session, 730, &[1, (-7i32) as u32]);
        control.accept_and_wait();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        super::super::test_support::command(&mut session, 730, &[1, 999]);
        control.accept_and_wait();
        assert_eq!(control.latest_pcm()[0], 0.5);
        control.finish(session.audio.as_mut().unwrap());
    }

    #[test]
    fn native_bgm_setting_updates_both_crossfade_streams() {
        let (mut bridge, control) = OutputControl::new();
        let mut vm = CmvsPs2aVmState::new(0);
        bridge
            .play_crossfade(
                tone_clip(CHUNK_FRAMES * 16, OUTPUT_RATE),
                None,
                0,
                true,
                256,
            )
            .unwrap();
        control.wait_for_write();
        bridge
            .play_crossfade(
                tone_clip(CHUNK_FRAMES * 16, OUTPUT_RATE),
                None,
                1000,
                true,
                256,
            )
            .unwrap();
        apply_setting(&mut vm, &mut bridge, 730, 128);
        control.accept_and_wait();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| sample.abs() <= 0.25)
        );
        apply_setting(&mut vm, &mut bridge, 730, 0);
        control.accept_and_wait();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        apply_setting(&mut vm, &mut bridge, 730, 128);
        control.accept_and_wait();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| sample.abs() <= 0.25)
        );
        assert!(control.latest_pcm()[0] > 0.24);
        control.finish(&mut bridge);
    }

    #[test]
    fn native_setting_reports_bad_commands_and_worker_failure() {
        let (mut bridge, control) = OutputControl::new();
        let vm = CmvsPs2aVmState::new(0);
        assert!(
            super::super::native_save_audio::apply_gain_setting(&vm, &mut bridge, 728).is_err()
        );
        control.finish(&mut bridge);
        assert!(
            super::super::native_save_audio::apply_gain_setting(&vm, &mut bridge, 729).is_err()
        );
    }

    #[test]
    fn live_voice_mute_partial_and_unmute_keep_the_pcm_cursor() {
        let (mut bridge, control) = OutputControl::new();
        let mut vm = CmvsPs2aVmState::new(0);
        let clip = DecodedClip {
            sample_rate: OUTPUT_RATE,
            frames: Arc::new(
                (0..CHUNK_FRAMES * 8)
                    .flat_map(|frame| [(frame / CHUNK_FRAMES + 1) as f32 / 16.0; 2])
                    .collect(),
            ),
            source: None,
            native_channels: 2,
            indeterminate_tail: false,
            native_restart_rewinds: true,
            native_repeat: None,
            native_pcm: None,
        };
        bridge
            .play_channel(
                5,
                clip,
                true,
                super::super::native_save_audio::channel_gain(&vm, 5),
            )
            .unwrap();
        control.wait_for_write();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| *sample == 1.0 / 16.0)
        );
        apply_setting(&mut vm, &mut bridge, 732, 0);
        control.accept_and_wait();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        apply_setting(&mut vm, &mut bridge, 732, 128);
        control.accept_and_wait();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| *sample == 3.0 / 32.0)
        );
        apply_setting(&mut vm, &mut bridge, 732, 256);
        control.accept_and_wait();
        assert!(
            control
                .latest_pcm()
                .iter()
                .all(|sample| *sample == 1.0 / 4.0)
        );
        assert_eq!(bridge.channel_position_frames(5), (CHUNK_FRAMES * 4) as u64);
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 5);
        control.finish(&mut bridge);
    }

    #[test]
    fn native_setting_groups_isolate_bgm_system_voice_sfx_and_static_pcm() {
        //Source0 is BGM;39 is a static duplicate, while38 is a static main.
        for command in 729..=733 {
            for source in [0, 2, 4, 5, 8, 9, 14, 38, 39] {
                let (mut bridge, control) = OutputControl::new();
                let mut vm = CmvsPs2aVmState::new(0);
                let clip = tone_clip(CHUNK_FRAMES * 8, OUTPUT_RATE);
                match source {
                    0 => bridge.play_crossfade(clip, None, 0, true, 256).unwrap(),
                    38 | 39 => {
                        bridge
                            .load_static_sound(23, "static.wav".into(), clip)
                            .unwrap();
                        bridge
                            .play_static_sound(
                                23,
                                256,
                                if source == 38 { Some(true) } else { None },
                            )
                            .unwrap();
                    }
                    _ => bridge.play_channel(source, clip, true, 256).unwrap(),
                }
                control.wait_for_write();
                assert_eq!(control.latest_pcm()[0], 0.5);
                apply_setting(&mut vm, &mut bridge, command, 128);
                control.accept_and_wait();
                let affected = match command {
                    729 => source < 15,
                    730 => source == 0,
                    731 => (9..=14).contains(&source),
                    732 => (5..=8).contains(&source),
                    733 => (2..=4).contains(&source),
                    _ => unreachable!(),
                };
                assert_eq!(
                    control.latest_pcm()[0],
                    if affected { 0.25 } else { 0.5 },
                    "command{command}, source{source}"
                );
                control.finish(&mut bridge);
            }
        }
    }

    #[test]
    fn sfx_setter_uses_signed_record_volume_master_does_not_and_large_gain_is_retained() {
        for volume in [128, 0, (-1i32) as u32, 1024] {
            let (mut bridge, control) = OutputControl::new();
            let mut vm = CmvsPs2aVmState::new(0);
            let record = crate::system_save::slot_globals::NativeResourceChannel {
                name: "effect.wav".into(),
                volume,
                ..Default::default()
            };
            bridge
                .configure_native_resource(
                    record.clone(),
                    NativeChannelPlayback {
                        slot: 9,
                        clip: tone_clip(CHUNK_FRAMES * 8, OUTPUT_RATE),
                        looping: true,
                        playing: true,
                        native_gain: 32,
                        volume: 1.0,
                        stereo_gain: [1.0; 2],
                        native_volume: 256,
                        native_pan: 0,
                    },
                    0,
                )
                .unwrap();
            control.wait_for_write();
            apply_setting(&mut vm, &mut bridge, 731, 128);
            control.accept_and_wait();
            let expected = match volume {
                128 => 0.125,
                1024 => 0.0625, //512 is rejected, preserving gain32.
                _ => 0.25,
            };
            assert_eq!(control.latest_pcm()[0], expected, "record volume{volume}");
            apply_setting(&mut vm, &mut bridge, 729, 128);
            control.accept_and_wait();
            assert_eq!(control.latest_pcm()[0], 0.125);
            let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
            bridge.capture_native_audio(&mut globals).unwrap();
            assert_eq!(globals.resource_channels.slots[0], record);
            control.finish(&mut bridge);
        }
    }

    #[test]
    fn live_bgm_gain_keeps_fade_progress_and_native_capture_sources() {
        let (mut bridge, control) = OutputControl::new();
        let mut vm = CmvsPs2aVmState::new(0);
        bridge
            .play_crossfade(
                tone_clip(CHUNK_FRAMES * 16, OUTPUT_RATE).with_source("fade.ogg".into()),
                None,
                1000,
                true,
                256,
            )
            .unwrap();
        control.wait_for_write();
        apply_setting(&mut vm, &mut bridge, 730, 0);
        control.accept_and_wait();
        assert!(control.latest_pcm().iter().all(|sample| *sample == 0.0));
        apply_setting(&mut vm, &mut bridge, 730, 256);
        control.accept_and_wait();
        let pcm = control.latest_pcm();
        assert_eq!(
            pcm[0],
            25.0 / 512.0,
            "fade must resume from100ms, not restart"
        );
        assert!(
            pcm[CHUNK_FRAMES * 2 - 2].abs() > 0.073,
            "fade must keep progressing while muted"
        );
        let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert_eq!(globals.bgm.primary, "fade.ogg");
        bridge.fade_out(1000).unwrap();
        apply_setting(&mut vm, &mut bridge, 730, 0);
        control.accept_and_wait();
        apply_setting(&mut vm, &mut bridge, 730, 256);
        control.accept_and_wait();
        let pcm = control.latest_pcm();
        assert_eq!(pcm[0], 25.0 / 512.0);
        assert!(
            pcm[CHUNK_FRAMES * 2 - 2].abs() < 0.026,
            "gain update must preserve outgoing fade"
        );
        control.finish(&mut bridge);
    }

    #[test]
    fn future_channel_and_bgm_playback_consume_current_settings_once() {
        for channel in [2, 5, 9] {
            let (mut bridge, control) = OutputControl::new();
            let mut vm = CmvsPs2aVmState::new(0);
            vm.write_interpreter_word(1560, 128);
            vm.write_interpreter_word(
                match channel {
                    2 => 1576,
                    5 => 1572,
                    _ => 1568,
                },
                128,
            );
            bridge
                .play_channel(
                    channel,
                    tone_clip(CHUNK_FRAMES * 2, OUTPUT_RATE),
                    true,
                    super::super::native_save_audio::channel_gain(&vm, channel),
                )
                .unwrap();
            control.wait_for_write();
            assert_eq!(control.latest_pcm()[0], 0.125);
            control.finish(&mut bridge);
        }
        let (mut bridge, control) = OutputControl::new();
        let mut vm = CmvsPs2aVmState::new(0);
        vm.write_interpreter_word(1560, 128);
        vm.write_interpreter_word(1564, 128);
        bridge
            .play_crossfade(
                tone_clip(CHUNK_FRAMES * 2, OUTPUT_RATE),
                None,
                0,
                true,
                super::super::native_save_audio::bgm_gain(&vm),
            )
            .unwrap();
        control.wait_for_write();
        assert_eq!(control.latest_pcm()[0], 0.125);
        control.finish(&mut bridge);
    }

    #[test]
    fn native_preparation_is_silent_until_commit_and_capture_survives_reload() {
        let (mut bridge, control) = OutputControl::new();
        let mut capture = super::super::native_save_audio::NativeAudioCapture::default();
        capture.bgm.primary = "native.ogg".into();
        let playback = PreparedNativePlayback::new(
            [
                Some(tone_clip(100, 48_000).with_source("native.ogg".into())),
                None,
            ],
            256,
            Vec::new(),
        );
        assert!(matches!(
            control.entered.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        bridge.commit_native_audio(playback, capture).unwrap();
        control.wait_for_write();
        let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert_eq!(globals.bgm.primary, "native.ogg");
        bridge.fade_out(250).unwrap();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert!(globals.bgm.primary.is_empty());
        control.finish(&mut bridge);
        assert!(bridge.capture_native_audio(&mut globals).is_err());
    }

    #[test]
    fn native_paired_bgm_plays_intro_once_then_loops_continuation() {
        let mut prepared = PreparedNativePlayback::new(
            [Some(tone_clip(2, 48_000)), Some(tone_clip(1, 48_000))],
            256,
            Vec::new(),
        );
        let stream = prepared.bgm[0].as_mut().unwrap();
        let mut mix = vec![0.0; CHUNK_FRAMES * 2];
        mix_stream(stream, &mut mix);
        assert_eq!(&mix[..8], &[0.5, 0.5, -0.5, -0.5, 0.5, 0.5, 0.5, 0.5]);
        assert!(stream.looping);
        assert!(stream.continuation.is_none());
        assert!(!stream.finished());
    }

    #[test]
    fn native_channel_bank_restores_last_owner_and_stereo_gain() {
        let mut prepared = PreparedNativePlayback::new(
            [None, None],
            256,
            vec![NativeChannelPlayback {
                slot: 38,
                clip: tone_clip(1, 48_000),
                looping: true,
                playing: true,
                native_gain: 128,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                stereo_gain: [0.0, 1.0],
            }],
        );
        let mut mix = vec![0.0; CHUNK_FRAMES * 2];
        mix_stream(prepared.channels[38].as_mut().unwrap(), &mut mix);
        assert_eq!(&mix[..4], &[0.0, 0.125, 0.0, 0.125]);
        assert!(prepared.bgm.iter().all(Option::is_none));
        assert!(prepared.channels[..38].iter().all(Option::is_none));
    }

    #[test]
    fn native_loaded_stopped_source_remains_loaded_without_advancing_pcm() {
        let mut prepared = PreparedNativePlayback::new(
            [None, None],
            256,
            vec![NativeChannelPlayback {
                slot: 9,
                clip: tone_clip(1, 48_000),
                looping: true,
                playing: false,
                native_gain: 256,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                stereo_gain: [1.0, 1.0],
            }],
        );
        let stream = prepared.channels[9].as_mut().unwrap();
        let mut mix = vec![0.0; CHUNK_FRAMES * 2];
        mix_stream(stream, &mut mix);
        assert!(mix.iter().all(|sample| *sample == 0.0));
        assert_eq!(stream.position, 0.0);
        assert!(!stream.finished());
        stream.playing = true;
        mix_stream(stream, &mut mix);
        assert_eq!(&mix[..2], &[0.5, 0.5]);
    }

    #[test]
    fn queued_voice_slot_eight_is_active_and_stop_does_not_clear_replay() {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .play_channel(2, tone_clip(100, 48_000), true, 256)
            .unwrap();
        control.wait_for_write();
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 2);
        let movie_clock = bridge.channel_position_frames(2);
        bridge
            .play_channel(8, tone_clip(100, 48_000), true, 256)
            .unwrap();
        assert_eq!(bridge.playing_channels().unwrap(), (1 << 2) | (1 << 8));
        assert_eq!(bridge.channel_position_frames(8), 0);
        assert_eq!(bridge.channel_position_frames(2), movie_clock);
        bridge.stop_channel(8).unwrap();
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 2);
        bridge
            .play_channel(8, tone_clip(100, 48_000), true, 256)
            .unwrap();
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), (1 << 2) | (1 << 8));
        assert_eq!(bridge.channel_position_frames(8), CHUNK_FRAMES as u64);
        assert_eq!(
            bridge.channel_position_frames(2),
            movie_clock + CHUNK_FRAMES as u64
        );
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), (1 << 2) | (1 << 8));
        control.finish(&mut bridge);
    }

    #[test]
    fn stale_eos_cannot_clear_a_queued_replacement() {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .play_channel(5, tone_clip(100, 48_000), false, 256)
            .unwrap();
        control.wait_for_write();
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 5);
        bridge
            .play_channel(5, tone_clip(100, 48_000), true, 256)
            .unwrap();
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 5);
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 5);
        control.finish(&mut bridge);
    }

    #[test]
    fn eos_retirement_waits_for_consumption_while_another_writer_is_blocked() {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .play_channel(2, tone_clip(100, 48_000), true, 256)
            .unwrap();
        control.wait_for_write();
        bridge
            .play_channel(7, tone_clip(100, 48_000), false, 256)
            .unwrap();
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), (1 << 2) | (1 << 7));
        control.accept_and_wait();
        assert_eq!(bridge.playing_channels().unwrap(), (1 << 2) | (1 << 7));
        assert_eq!(control.played_frames(), 0);
        control.consume_to(CHUNK_FRAMES as u64 + 100);
        assert_eq!(bridge.playing_channels().unwrap(), 1 << 2);
        bridge.stop_channel(2).unwrap();
        assert_eq!(bridge.playing_channels().unwrap(), 0);
        control.finish(&mut bridge);
    }

    #[test]
    fn playing_state_propagates_worker_rejection_and_unexpected_closure() {
        for failure in [OutputStep::Fail, OutputStep::Close] {
            let (mut bridge, control) = OutputControl::new();
            bridge
                .play_channel(8, tone_clip(100, 48_000), true, 256)
                .unwrap();
            control.wait_for_write();
            control.steps.send(failure).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while bridge.playing_channels().is_ok() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "worker failure must reach consumer"
                );
                std::thread::yield_now();
            }
            assert!(bridge.close().is_err());
            assert!(bridge.playing_channels().is_err());
        }
    }

    #[test]
    fn playing_state_rejects_a_play_after_shutdown() {
        let (mut bridge, control) = OutputControl::new();
        control.finish(&mut bridge);
        assert!(
            bridge
                .play_channel(8, tone_clip(100, 48_000), false, 256)
                .is_err()
        );
        assert!(bridge.playing_channels().is_err());
    }
    #[test]
    fn static_main_natural_end_keeps_a_replayable_buffer_and_emits_real_pcm() {
        let (mut bridge, recorded) = recording_output();
        bridge
            .load_static_sound(16, "effect.wav".into(), tone_clip(4, OUTPUT_RATE))
            .unwrap();
        for expected in 1..=2 {
            bridge.play_static_sound(16, 128, Some(false)).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while recorded.lock().len() < expected {
                assert!(
                    std::time::Instant::now() < deadline,
                    "static main replay must reach the output"
                );
                std::thread::yield_now();
            }
        }
        bridge.close().unwrap();
        let chunks = recorded.lock();
        for chunk in chunks.iter() {
            assert_eq!(
                &chunk[..8],
                &[0.125, 0.125, -0.125, -0.125, 0.125, 0.125, -0.125, -0.125]
            );
            assert!(chunk[8..].iter().all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn native_static_capture_records_loop_flag_not_duplicate_or_one_shot_activity() {
        let (mut bridge, control) = OutputControl::new();
        bridge
            .load_static_sound(16, "effect.wav".into(), tone_clip(9600, OUTPUT_RATE))
            .unwrap();
        bridge.play_static_sound(16, 256, None).unwrap();
        control.wait_for_write();
        let mut globals = crate::system_save::slot_globals::NativeGlobals::default();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert_eq!(globals.static_sounds.entries[0].playing, 0);
        bridge.play_static_sound(16, 256, Some(true)).unwrap();
        bridge.play_static_sound(16, 256, Some(false)).unwrap();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert_eq!(globals.static_sounds.entries[0].playing, 1);
        bridge.stop_static_sound(16).unwrap();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert_eq!(globals.static_sounds.entries[0].playing, 0);
        bridge.clear_static_sound(16).unwrap();
        bridge.capture_native_audio(&mut globals).unwrap();
        assert!(globals.static_sounds.entries.is_empty());
        control.finish(&mut bridge);
    }
}
