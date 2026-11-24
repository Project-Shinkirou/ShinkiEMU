use crate::runtime::{SessionError, SessionResult};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
fn error(code: &str, message: &str) -> SessionError {
    SessionError::invalid(code, message)
}
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use roukaku_platform::{AudioOutputLane, AudioOutputRequest, AudioWakeRegistration};
use roukaku_platform_common::{
    AudioQueueTelemetryReader, NativeAudioConsumer, NativeAudioProducer, NativeAudioQueue,
    NullAudioDevice,
};

/// The VM/Kira/movie source clock written to every native output.
const SOURCE_RATE: u32 = 48_000;
const SOURCE_CHANNELS: u16 = 2;

/// Output timeline in 48 kHz source frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeAudioProgress {
    /// Frames of every `write` that returned `Ok(Some(_))`, plus `flush` padding.
    pub accepted_frames: u64,
    /// Frames the device has made audible: consumed device frames minus the
    /// reported output latency and the resampler delay, mapped to 48 kHz.
    /// Underrun silence never advances it. Monotonic and `<= accepted_frames`.
    pub played_frames: u64,
}

/// Cloneable, thread-safe view of one output's timeline. It stays readable
/// while the writer thread is blocked or idle, and after the output closes.
#[derive(Clone)]
pub struct NativeAudioProgressReader {
    clock: Arc<PlaybackClock>,
}
impl NativeAudioProgressReader {
    pub fn progress(&self) -> NativeAudioProgress {
        self.clock.progress()
    }
}

/// Maps the device queue's consumed-sample counter back to source frames.
struct PlaybackClock {
    telemetry: AudioTelemetry,
    device_rate: u64,
    device_channels: u64,
    /// Device frames emitted before the first source frame (resampler delay).
    delay_frames: u64,
    /// Device samples consumed by the latest callback but not yet audible.
    latency_samples: AtomicU64,
    accepted: AtomicU64,
    played: AtomicU64,
}

enum AudioTelemetry {
    Device(AudioQueueTelemetryReader),
    Null(Arc<Mutex<NativeAudioProducer>>),
}

impl AudioTelemetry {
    fn consumed_samples(&self) -> u64 {
        match self {
            Self::Device(reader) => reader.snapshot().consumed_samples,
            // Keep reads nonblocking while the null endpoint's producer waits for room.
            Self::Null(producer) => producer
                .try_lock()
                .map_or(0, |value| value.consumed_samples()),
        }
    }
}

impl PlaybackClock {
    fn new(
        telemetry: AudioTelemetry,
        device_rate: u32,
        device_channels: u16,
        delay_frames: usize,
    ) -> Self {
        Self {
            telemetry,
            device_rate: u64::from(device_rate),
            device_channels: u64::from(device_channels),
            delay_frames: delay_frames as u64,
            latency_samples: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            played: AtomicU64::new(0),
        }
    }
    /// Only the writer thread calls this, after the samples were queued.
    fn accept(&self, samples: usize) {
        let frames = (samples / usize::from(SOURCE_CHANNELS)) as u64;
        self.accepted.fetch_add(frames, Ordering::Release);
    }
    /// `latency` is the callback's playback-minus-callback delay;
    /// `callback_samples` is the buffer the callback is about to fill.
    fn record_latency(&self, latency: Duration, callback_samples: usize) {
        let delayed = latency.as_nanos() * u128::from(self.device_rate * self.device_channels)
            / 1_000_000_000;
        let samples = u64::try_from(delayed)
            .unwrap_or(u64::MAX)
            .saturating_add(callback_samples as u64);
        self.latency_samples.store(samples, Ordering::Release);
    }
    fn accepted_frames(&self) -> u64 {
        self.accepted.load(Ordering::Acquire)
    }
    /// Device samples the queue must have consumed before `progress` reports
    /// `frames` as played, at the latest recorded output latency.
    fn samples_until_audible(&self, frames: u64) -> u64 {
        let device_frames =
            (u128::from(frames) * u128::from(self.device_rate)).div_ceil(u128::from(SOURCE_RATE));
        let device_frames = u64::try_from(device_frames).unwrap_or(u64::MAX);
        (self.delay_frames.saturating_add(device_frames))
            .saturating_mul(self.device_channels)
            .saturating_add(self.latency_samples.load(Ordering::Acquire))
    }
    fn progress(&self) -> NativeAudioProgress {
        let accepted = self.accepted.load(Ordering::Acquire);
        // Device latency varies per callback; the watermark never retracts.
        let audible = self
            .telemetry
            .consumed_samples()
            .saturating_sub(self.latency_samples.load(Ordering::Acquire))
            / self.device_channels;
        let source = u128::from(audible.saturating_sub(self.delay_frames))
            * u128::from(SOURCE_RATE)
            / u128::from(self.device_rate);
        // Converter input and the unsubmitted partial block are accepted but
        // not consumed, so the cap only matters around the resampler delay.
        let played = u64::try_from(source).unwrap_or(u64::MAX).min(accepted);
        let played = self.played.fetch_max(played, Ordering::AcqRel).max(played);
        // A concurrent reader may have raised the watermark from a newer
        // `accepted`; reloading after the RMW keeps `played <= accepted`.
        NativeAudioProgress {
            accepted_frames: self.accepted.load(Ordering::Acquire),
            played_frames: played,
        }
    }
}

pub struct NativeAudioOutput {
    pipeline: Pipeline,
    _stream: Option<cpal::Stream>,
    _null_device: Option<NullAudioDevice>,
}
impl NativeAudioOutput {
    pub fn open(stop: Arc<AtomicBool>) -> SessionResult<Self> {
        Self::open_device(Some(stop), None, AudioWakeRegistration::default())
    }

    /// Bound queue backpressure and report a device failure without modifying
    /// the caller's stop flag. Notify `wake` after setting stop to release a
    /// blocked writer immediately.
    pub fn open_bounded_with_wake(
        stall_timeout: Duration,
        wake: AudioWakeRegistration,
    ) -> SessionResult<Self> {
        Self::open_device(None, Some(stall_timeout), wake)
    }

    fn open_device(
        stop_on_error: Option<Arc<AtomicBool>>,
        stall_timeout: Option<Duration>,
        wake: AudioWakeRegistration,
    ) -> SessionResult<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_AUDIO_DEVICE",
                    "native output device is unavailable",
                )
            })?;
        // Device negotiation and Rubato conversion retain the old output
        // backend's policy; the VM/Kira/movie source clock remains 48 kHz.
        let supported = device.default_output_config().map_err(|_| {
            error(
                "SHINKI_EMU_AUDIO_DEFAULT_FORMAT",
                "cannot query native device format",
            )
        })?;
        let converter = crate::NativeAudioConverter::new(
            SOURCE_RATE,
            SOURCE_CHANNELS,
            supported.sample_rate(),
            supported.channels(),
        )
        .map_err(conversion_error)?;
        let chunk_samples = 512 * usize::from(supported.channels());
        const DEVICE_BLOCKS: usize = 4;
        let format = supported.sample_format();
        let config = supported.into();
        let (producer, consumer, telemetry) =
            NativeAudioQueue::create(DEVICE_BLOCKS, chunk_samples, wake.clone())
                .map_err(|_| error("SHINKI_EMU_AUDIO_QUEUE", "cannot create device queue"))?;
        let clock = Arc::new(PlaybackClock::new(
            AudioTelemetry::Device(telemetry),
            supported.sample_rate(),
            supported.channels(),
            converter.output_delay(),
        ));
        let bounded = stall_timeout.map(|timeout| DeviceCapacity {
            wake: wake.clone(),
            observed_wake: 0,
            blocks: DEVICE_BLOCKS as u64,
            timeout,
        });
        let failed = Arc::new(OnceLock::new());
        let callback = Callback {
            consumer,
            wake,
            failed: failed.clone(),
            stop_on_error,
            clock: clock.clone(),
        };
        let stream = match format {
            cpal::SampleFormat::I8 => build::<i8>(&device, config, callback),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, callback),
            cpal::SampleFormat::I24 => build::<cpal::I24>(&device, config, callback),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, callback),
            cpal::SampleFormat::I64 => build::<i64>(&device, config, callback),
            cpal::SampleFormat::U8 => build::<u8>(&device, config, callback),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, callback),
            cpal::SampleFormat::U32 => build::<u32>(&device, config, callback),
            cpal::SampleFormat::U64 => build::<u64>(&device, config, callback),
            cpal::SampleFormat::F32 => build::<f32>(&device, config, callback),
            cpal::SampleFormat::F64 => build::<f64>(&device, config, callback),
            _ => {
                return Err(error(
                    "SHINKI_EMU_AUDIO_FORMAT",
                    "unsupported native sample format",
                ));
            }
        }
        .map_err(|_| error("SHINKI_EMU_AUDIO_STREAM", "native stream creation failed"))?;
        stream
            .play()
            .map_err(|_| error("SHINKI_EMU_AUDIO_START", "native stream start failed"))?;
        Ok(Self {
            pipeline: Pipeline {
                blocks: BlockWriter {
                    producer: Producer::Device(producer),
                    partial: vec![0.0; chunk_samples],
                    partial_len: 0,
                    submitted: 0,
                    bounded,
                },
                failed,
                converter,
                clock,
            },
            _stream: Some(stream),
            _null_device: None,
        })
    }

    /// Explicit audio clock for automation. Never selected on device failure.
    pub fn open_null() -> SessionResult<Self> {
        let request = AudioOutputRequest {
            sample_rate: SOURCE_RATE,
            channels: SOURCE_CHANNELS,
            chunk_frames: 512,
            max_buffered_frames: 2048,
            start_paused: false,
            capture_samples: false,
        };
        let (device, producer) =
            NullAudioDevice::open(request, AudioWakeRegistration::default())
                .map_err(|cause| error("SHINKI_EMU_AUDIO_NULL", &cause.to_string()))?;
        let producer = Arc::new(Mutex::new(producer));
        let converter = crate::NativeAudioConverter::new(
            SOURCE_RATE,
            SOURCE_CHANNELS,
            SOURCE_RATE,
            SOURCE_CHANNELS,
        )
        .map_err(conversion_error)?;
        let clock = Arc::new(PlaybackClock::new(
            AudioTelemetry::Null(producer.clone()),
            SOURCE_RATE,
            SOURCE_CHANNELS,
            0,
        ));
        Ok(Self {
            pipeline: Pipeline {
                blocks: BlockWriter {
                    producer: Producer::Null(producer),
                    partial: vec![0.0; 512 * usize::from(SOURCE_CHANNELS)],
                    partial_len: 0,
                    submitted: 0,
                    bounded: None,
                },
                failed: Arc::new(OnceLock::new()),
                converter,
                clock,
            },
            _stream: None,
            _null_device: Some(device),
        })
    }
    /// Observes the device queue; call `check` separately for device failure.
    pub fn progress(&self) -> NativeAudioProgress {
        self.pipeline.clock.progress()
    }
    /// Hands the same timeline to threads that do not own the output.
    pub fn progress_reader(&self) -> NativeAudioProgressReader {
        NativeAudioProgressReader {
            clock: self.pipeline.clock.clone(),
        }
    }
    pub fn check(&self) -> SessionResult<()> {
        self.pipeline.check()
    }
    /// Accepts interleaved 48 kHz stereo of any length. Device samples are
    /// queued in whole device blocks; a trailing partial block is carried to
    /// the next `write` and reaches the device only once it fills.
    pub fn write(
        &mut self,
        samples: Vec<f32>,
        stop: &AtomicBool,
    ) -> SessionResult<Option<Vec<f32>>> {
        self.pipeline.write(samples, stop)
    }
    /// Writes silence until every frame accepted before the call can become
    /// audible, then returns; call it before the writer idles. The padding
    /// counts as accepted, so the timeline stays continuous for later writes
    /// and `played_frames` may settle below `accepted_frames`. Blocks on
    /// queue capacity like `write`; returns `Ok(None)` once `stop` is set.
    pub fn flush(&mut self, stop: &AtomicBool) -> SessionResult<Option<()>> {
        self.pipeline.flush(stop)
    }
}
/// Converter, block regrouping and timeline of one output, without the stream.
struct Pipeline {
    blocks: BlockWriter,
    failed: Arc<OnceLock<cpal::Error>>,
    converter: crate::NativeAudioConverter,
    clock: Arc<PlaybackClock>,
}
impl Pipeline {
    fn check(&self) -> SessionResult<()> {
        self.failed
            .get()
            .map_or(Ok(()), |cause| Err(stream_error(cause)))
    }
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
        self.check()?;
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let blocks = &mut self.blocks;
        let failed = &self.failed;
        let conversion = if self.converter.needs_conversion() {
            self.converter
                .convert(&samples, |converted| blocks.push(converted, failed, stop))
        } else {
            blocks.push(&samples, failed, stop)
        };
        self.check()?;
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        conversion.map_err(conversion_error)?;
        self.clock.accept(samples.len());
        Ok(Some(samples))
    }
    fn flush(&mut self, stop: &AtomicBool) -> SessionResult<Option<()>> {
        self.check()?;
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        // One extra block absorbs callback-to-callback latency jitter.
        let target = self
            .clock
            .samples_until_audible(self.clock.accepted_frames())
            + self.blocks.partial.len() as u64;
        let mut silence = vec![0.0; 512 * usize::from(SOURCE_CHANNELS)];
        while self.blocks.submitted < target {
            match self.write(silence, stop)? {
                Some(recycled) => silence = recycled,
                None => return Ok(None),
            }
        }
        Ok(Some(()))
    }
}

struct DeviceCapacity {
    wake: AudioWakeRegistration,
    observed_wake: u64,
    blocks: u64,
    timeout: Duration,
}

impl DeviceCapacity {
    fn wait(
        &mut self,
        lane: &NativeAudioProducer,
        submitted: u64,
        chunk_samples: u64,
        failed: &OnceLock<cpal::Error>,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        // Only fully consumed blocks have returned a reusable allocation.
        let full = || submitted - lane.consumed_samples() > (self.blocks - 1) * chunk_samples;
        if !full() {
            return Ok(());
        }
        let start = Instant::now();
        while full() && failed.get().is_none() && !stop.load(Ordering::Acquire) {
            let remaining = self.timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err("SHINKI_EMU_AUDIO_STALLED: native device queue made no progress".into());
            }
            if let Some(sequence) = self.wake.wait_timeout(self.observed_wake, remaining) {
                self.observed_wake = sequence;
            }
        }
        Ok(())
    }
}

/// Regroups device-format samples into the queue's fixed block size.
struct BlockWriter {
    producer: Producer,
    partial: Vec<f32>,
    partial_len: usize,
    /// Device samples handed to the queue.
    submitted: u64,
    bounded: Option<DeviceCapacity>,
}

enum Producer {
    Device(NativeAudioProducer),
    Null(Arc<Mutex<NativeAudioProducer>>),
}

impl Producer {
    fn with_lane<T>(&mut self, action: impl FnOnce(&mut NativeAudioProducer) -> T) -> T {
        match self {
            Self::Device(producer) => action(producer),
            Self::Null(producer) => action(&mut producer.lock().expect("audio output producer")),
        }
    }
}

impl BlockWriter {
    fn push(
        &mut self,
        mut data: &[f32],
        failed: &OnceLock<cpal::Error>,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        while !data.is_empty() {
            if failed.get().is_some() || stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let count = (self.partial.len() - self.partial_len).min(data.len());
            self.partial[self.partial_len..self.partial_len + count]
                .copy_from_slice(&data[..count]);
            self.partial_len += count;
            data = &data[count..];
            if self.partial_len == self.partial.len() {
                let len = self.partial.len();
                if let Some(capacity) = &mut self.bounded {
                    self.producer.with_lane(|lane| {
                        capacity.wait(lane, self.submitted, len as u64, failed, stop)
                    })?;
                } else {
                    self.producer
                        .with_lane(|lane| lane.wait_for_capacity(len, stop))
                        .map_err(|_| "SHINKI_EMU_AUDIO_QUEUE".to_owned())?;
                }
                if failed.get().is_some() || stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                self.partial = self
                    .producer
                    .with_lane(|lane| lane.submit(std::mem::take(&mut self.partial)))
                    .map_err(|_| "SHINKI_EMU_AUDIO_QUEUE".to_owned())?;
                self.partial_len = 0;
                self.submitted += len as u64;
            }
        }
        Ok(())
    }
}
fn conversion_error(cause: String) -> SessionError {
    error(
        cause
            .split(':')
            .next()
            .unwrap_or("SHINKI_EMU_AUDIO_CONVERSION"),
        &cause,
    )
}
struct Callback {
    consumer: NativeAudioConsumer,
    wake: AudioWakeRegistration,
    failed: Arc<OnceLock<cpal::Error>>,
    stop_on_error: Option<Arc<AtomicBool>>,
    clock: Arc<PlaybackClock>,
}

fn stream_error(cause: &cpal::Error) -> SessionError {
    // Keep symbolic backend codes such as AAudio's Disconnected. Freeform
    // messages can contain private host paths.
    let reason = cause.message().filter(|message| {
        !message.is_empty()
            && message.len() <= 64
            && message
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    let kind = cause.kind();
    SessionError::new(
        "SHINKI_EMU_AUDIO_DEVICE",
        match reason {
            Some(reason) => format!("native output device failed ({kind:?}): {reason}"),
            None => format!("native output device failed ({kind:?}): {kind}"),
        },
    )
}

fn on_stream_error(
    cause: cpal::Error,
    failed: &OnceLock<cpal::Error>,
    stop_on_error: Option<&AtomicBool>,
    wake: &AudioWakeRegistration,
) {
    use cpal::ErrorKind::{DeviceChanged, RealtimeDenied, Xrun};
    // These are notifications on a stream that keeps running. AAudio
    // reports Xrun from the normal data callback, for example.
    if matches!(cause.kind(), DeviceChanged | RealtimeDenied | Xrun) {
        return;
    }
    if failed.set(cause).is_ok() {
        if let Some(stop) = stop_on_error {
            stop.store(true, Ordering::Release);
        }
        wake.notify();
    }
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    callback: Callback,
) -> Result<cpal::Stream, cpal::Error> {
    let Callback {
        mut consumer,
        wake,
        failed,
        stop_on_error,
        clock,
    } = callback;
    let error_wake = wake.clone();
    device.build_output_stream(
        config,
        move |output: &mut [T], info| {
            // Record before filling so a concurrent progress query can only
            // under-report this callback's not yet audible samples.
            let time = info.timestamp();
            clock.record_latency(time.playback.duration_since(time.callback), output.len());
            let mut scratch = [0.0; 1024];
            for chunk in output.chunks_mut(scratch.len()) {
                consumer.fill_output_f32(&mut scratch[..chunk.len()]);
                for (target, source) in chunk.iter_mut().zip(&scratch) {
                    *target = T::from_sample(source.clamp(-1.0, 1.0));
                }
            }
            wake.notify();
        },
        move |cause| on_stream_error(cause, &failed, stop_on_error.as_deref(), &error_wake),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounded_pipeline(
        wake: &AudioWakeRegistration,
        timeout: Duration,
    ) -> (Pipeline, NativeAudioConsumer) {
        let (producer, consumer, telemetry) = NativeAudioQueue::create(1, 8, wake.clone()).unwrap();
        let clock = Arc::new(PlaybackClock::new(AudioTelemetry::Device(telemetry), 48_000, 2, 0));
        let pipeline = Pipeline {
            blocks: BlockWriter {
                producer: Producer::Device(producer),
                partial: vec![0.0; 8],
                partial_len: 0,
                submitted: 0,
                bounded: Some(DeviceCapacity {
                    wake: wake.clone(),
                    observed_wake: 0,
                    blocks: 1,
                    timeout,
                }),
            },
            failed: Arc::new(OnceLock::new()),
            converter: crate::NativeAudioConverter::new(48_000, 2, 48_000, 2).unwrap(),
            clock,
        };
        (pipeline, consumer)
    }

    #[test]
    fn stream_notices_keep_playing_and_terminal_error_retains_first_cause() {
        use cpal::ErrorKind::{BackendError, DeviceChanged, DeviceNotAvailable, RealtimeDenied, Xrun};

        let stop = AtomicBool::new(false);
        let wake = AudioWakeRegistration::default();
        let (mut pipeline, mut consumer) = bounded_pipeline(&wake, Duration::from_secs(2));
        let failed = pipeline.failed.clone();
        for kind in [Xrun, DeviceChanged, RealtimeDenied] {
            on_stream_error(kind.into(), &failed, Some(&stop), &wake);
            assert!(pipeline.write(vec![0.25; 8], &stop).unwrap().is_some());
            let mut consumed = [0.0; 8];
            assert_eq!(consumer.pop_samples(&mut consumed), 8);
            assert_eq!(consumed, [0.25; 8]);
        }
        pipeline.write(vec![0.5; 8], &stop).unwrap().unwrap();
        let first = cpal::Error::with_message(DeviceNotAvailable, "Disconnected");
        let cause = std::thread::scope(|scope| {
            let writer = scope.spawn(|| pipeline.write(vec![0.75; 8], &stop));
            on_stream_error(first.clone(), &failed, None, &wake);
            on_stream_error(BackendError.into(), &failed, None, &wake);
            assert_eq!(wake.wait_timeout(0, Duration::ZERO), Some(1));
            writer.join().unwrap().unwrap_err()
        });
        assert!(!stop.load(Ordering::Acquire));
        assert_eq!(failed.get(), Some(&first));
        assert_eq!(cause.code, "SHINKI_EMU_AUDIO_DEVICE");
        assert!(cause.message.contains("DeviceNotAvailable"));
        assert!(cause.message.ends_with("Disconnected"));
        assert_eq!(pipeline.clock.progress().played_frames, 12);
        assert_eq!(pipeline.clock.progress().accepted_frames, 16);
        assert_eq!(pipeline.flush(&stop), Err(cause));
    }

    #[test]
    fn stream_diagnostic_cannot_publish_freeform_backend_text() {
        let backend =
            cpal::Error::with_message(cpal::ErrorKind::BackendError, "opening /home/private");
        let cause = stream_error(&backend);
        assert!(cause.message.contains("BackendError"));
        assert!(!cause.message.contains("/home/private"));
    }

    #[test]
    fn bounded_queue_stalls_until_a_full_block_is_recycled() {
        let stop = AtomicBool::new(false);
        let wake = AudioWakeRegistration::default();
        let (mut pipeline, mut consumer) = bounded_pipeline(&wake, Duration::from_millis(30));
        pipeline.write(vec![0.25; 8], &stop).unwrap().unwrap();
        assert_eq!(consumer.pop_samples(&mut [0.0; 8]), 8);
        pipeline.write(vec![0.5; 8], &stop).unwrap().unwrap();
        assert_eq!(consumer.pop_samples(&mut [0.0; 7]), 7);
        let cause = pipeline.write(vec![0.75; 8], &stop).unwrap_err();
        assert_eq!(cause.code, "SHINKI_EMU_AUDIO_STALLED");
        assert_eq!(pipeline.clock.progress().accepted_frames, 8);
        assert!(!stop.load(Ordering::Acquire));
    }

    #[test]
    fn bounded_queue_host_cancel_does_not_report_an_audio_failure() {
        let stop = AtomicBool::new(false);
        let wake = AudioWakeRegistration::default();
        let (mut pipeline, _consumer) = bounded_pipeline(&wake, Duration::from_secs(5));
        pipeline.write(vec![0.25; 8], &stop).unwrap().unwrap();
        std::thread::scope(|scope| {
            let (entered, writing) = std::sync::mpsc::channel();
            let (complete, result) = std::sync::mpsc::channel();
            let pipeline = &mut pipeline;
            let stop = &stop;
            let writer = scope.spawn(move || {
                entered.send(()).unwrap();
                complete.send(pipeline.write(vec![0.5; 8], stop)).unwrap();
            });
            writing.recv_timeout(Duration::from_secs(5)).unwrap();
            stop.store(true, Ordering::Release);
            wake.notify();
            assert!(
                result
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
            writer.join().unwrap();
        });
        assert_eq!(pipeline.clock.progress().accepted_frames, 4);
        assert_eq!(pipeline.flush(&stop).unwrap(), None);
        assert!(pipeline.check().is_ok());
    }

    #[test]
    fn null_output_advances_the_same_clock_without_opening_a_device() {
        let stop = AtomicBool::new(false);
        let mut output = NativeAudioOutput::open_null().unwrap();
        // Exercise refill and recycling by submitting past queue capacity.
        const BLOCKS: u64 = 8;
        const FRAMES_PER_BLOCK: u64 = 512;
        let mut samples = vec![0.25; (FRAMES_PER_BLOCK * u64::from(SOURCE_CHANNELS)) as usize];
        for blocks in 1..=BLOCKS {
            samples = output.write(samples, &stop).unwrap().unwrap();
            let progress = output.progress();
            assert_eq!(progress.accepted_frames, blocks * FRAMES_PER_BLOCK);
            assert!(progress.played_frames <= progress.accepted_frames);
        }
        let frames = BLOCKS * FRAMES_PER_BLOCK;
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while output.progress().played_frames < frames && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(output.progress().played_frames, frames);
        stop.store(true, Ordering::Release);
        assert!(output.write(vec![0.0; 1024], &stop).unwrap().is_none());
    }

    #[test]
    fn progress_follows_audible_device_frames_and_never_exceeds_accepted() {
        // 96 kHz stereo device with 8 frames of resampler delay: 2 device
        // frames per source frame, 96 device frames per queued chunk.
        let (mut producer, mut consumer, telemetry) =
            NativeAudioQueue::create(4, 192, AudioWakeRegistration::default()).unwrap();
        let clock = Arc::new(PlaybackClock::new(
            AudioTelemetry::Device(telemetry),
            96_000,
            2,
            8,
        ));
        let reader = NativeAudioProgressReader {
            clock: clock.clone(),
        };
        let played = || reader.progress().played_frames;
        assert_eq!(reader.progress(), NativeAudioProgress::default());
        producer.submit(vec![0.0; 192]).unwrap();
        clock.accept(96);
        assert_eq!(reader.progress().accepted_frames, 48);
        assert_eq!(played(), 0);

        consumer.pop_samples(&mut [0.0; 64]);
        assert_eq!(played(), 12);
        // Larger reported latency hides more frames, but never retracts.
        clock.record_latency(Duration::ZERO, 64);
        assert_eq!(played(), 12);
        clock.record_latency(Duration::ZERO, 0);
        consumer.pop_samples(&mut [0.0; 128]);
        assert_eq!(played(), 44);
        // Underrun silence does not advance the source clock.
        assert!(consumer.fill_output_f32(&mut [0.0; 64]));
        assert_eq!(played(), 44);

        // A callback may drain a chunk before the writer records acceptance.
        producer.submit(vec![0.0; 192]).unwrap();
        consumer.pop_samples(&mut [0.0; 192]);
        assert_eq!(played(), 48);
        clock.accept(96);
        // Another thread observes the same timeline while the writer is idle.
        let remote = reader.clone();
        let progress = std::thread::spawn(move || remote.progress())
            .join()
            .unwrap();
        assert_eq!(progress.accepted_frames, 96);
        assert_eq!(progress.played_frames, 92);
    }

    #[test]
    fn writes_of_any_length_reach_the_device_in_order_as_whole_blocks() {
        let (producer, mut consumer, _) =
            NativeAudioQueue::create(4, 8, AudioWakeRegistration::default()).unwrap();
        let mut blocks = BlockWriter {
            producer: Producer::Device(producer),
            partial: vec![0.0; 8],
            partial_len: 0,
            submitted: 0,
            bounded: None,
        };
        let (failed, stop) = (OnceLock::new(), AtomicBool::new(false));
        let source: Vec<f32> = (0..24).map(|sample| sample as f32).collect();
        let mut device = [0.0; 24];

        blocks.push(&source[..5], &failed, &stop).unwrap();
        assert_eq!(consumer.pop_samples(&mut device), 0);
        blocks.push(&source[5..11], &failed, &stop).unwrap();
        assert_eq!(consumer.pop_samples(&mut device[..8]), 8);
        // The 3 carried samples lead the next block, which also spans writes.
        blocks.push(&source[11..24], &failed, &stop).unwrap();
        assert_eq!(consumer.pop_samples(&mut device[8..]), 16);
        assert_eq!(device[..], source[..]);
        assert_eq!(blocks.partial_len, 0);
    }

    #[test]
    fn flush_makes_every_accepted_frame_audible_through_resampler_and_carry() {
        // 44.1 kHz stereo device: the resampler and the partial block both
        // hold back the tail of the last write.
        let (producer, mut consumer, telemetry) =
            NativeAudioQueue::create(64, 1024, AudioWakeRegistration::default()).unwrap();
        let converter = crate::NativeAudioConverter::new(48_000, 2, 44_100, 2).unwrap();
        let clock = Arc::new(PlaybackClock::new(
            AudioTelemetry::Device(telemetry),
            44_100,
            2,
            converter.output_delay(),
        ));
        let mut pipeline = Pipeline {
            blocks: BlockWriter {
                producer: Producer::Device(producer),
                partial: vec![0.0; 1024],
                partial_len: 0,
                submitted: 0,
                bounded: None,
            },
            failed: Arc::new(OnceLock::new()),
            converter,
            clock: clock.clone(),
        };
        let stop = AtomicBool::new(false);
        let mut drain = || while consumer.pop_samples(&mut [0.0; 1024]) > 0 {};
        clock.record_latency(Duration::ZERO, 512);
        pipeline
            .write(vec![0.25; 2 * 2401], &stop)
            .unwrap()
            .unwrap();
        drain();
        let before = clock.progress();
        assert_eq!(before.accepted_frames, 2401);
        assert!(before.played_frames < before.accepted_frames);

        assert_eq!(pipeline.flush(&stop).unwrap(), Some(()));
        drain();
        let after = clock.progress();
        assert!(after.played_frames >= before.accepted_frames);
        assert!(after.accepted_frames > before.accepted_frames);
        assert!(after.played_frames <= after.accepted_frames);
        // Padding joined the timeline, so a later voice is not played early.
        pipeline
            .write(vec![0.25; 2 * 4800], &stop)
            .unwrap()
            .unwrap();
        let next = clock.progress();
        assert_eq!(next.accepted_frames, after.accepted_frames + 4800);
        assert!(next.played_frames < after.accepted_frames + 4800);

        stop.store(true, Ordering::Release);
        assert_eq!(pipeline.flush(&stop).unwrap(), None);
    }
}
