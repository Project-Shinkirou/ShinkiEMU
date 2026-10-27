use crate::runtime::{SessionError, SessionResult};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
fn error(code: &str, message: &str) -> SessionError {
    SessionError::invalid(code, message)
}
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use roukaku_platform::{AudioOutputLane, AudioWakeRegistration};
use roukaku_platform_common::{NativeAudioConsumer, NativeAudioProducer, NativeAudioQueue};

/// The VM/Kira/movie source clock written to every native output.
const SOURCE_RATE: u32 = 48_000;
const SOURCE_CHANNELS: u16 = 2;

/// Output timeline in 48 kHz source frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NativeAudioProgress {
    /// Frames of every `write` that returned `Ok(Some(_))`.
    pub accepted_frames: u64,
    /// Frames the device has made audible: consumed device frames minus the
    /// reported output latency and the resampler delay, mapped to 48 kHz.
    /// Underrun silence never advances it. Monotonic and `<= accepted_frames`.
    pub played_frames: u64,
}

/// Maps the device queue's consumed-sample counter back to source frames.
struct PlaybackClock {
    device_rate: u64,
    device_channels: u64,
    /// Device frames emitted before the first source frame (resampler delay).
    delay_frames: u64,
    accepted: u64,
    played: AtomicU64,
}
impl PlaybackClock {
    fn new(device_rate: u32, device_channels: u16, delay_frames: usize) -> Self {
        Self {
            device_rate: u64::from(device_rate),
            device_channels: u64::from(device_channels),
            delay_frames: delay_frames as u64,
            accepted: 0,
            played: AtomicU64::new(0),
        }
    }
    fn accept(&mut self, samples: usize) {
        self.accepted += (samples / usize::from(SOURCE_CHANNELS)) as u64;
    }
    fn progress(&self, lane: &impl AudioOutputLane) -> NativeAudioProgress {
        // Device latency varies per callback; the watermark never retracts.
        let audible = lane
            .consumed_samples()
            .saturating_sub(lane.output_latency_samples())
            / self.device_channels;
        let source = u128::from(audible.saturating_sub(self.delay_frames))
            * u128::from(SOURCE_RATE)
            / u128::from(self.device_rate);
        // Converter input and the unsubmitted partial block are accepted but
        // not consumed, so the cap only matters around the resampler delay.
        let played = u64::try_from(source).unwrap_or(u64::MAX).min(self.accepted);
        NativeAudioProgress {
            accepted_frames: self.accepted,
            played_frames: self.played.fetch_max(played, Ordering::Relaxed).max(played),
        }
    }
}

pub struct NativeAudioOutput {
    producer: NativeAudioProducer,
    failed: Arc<AtomicBool>,
    converter: crate::NativeAudioConverter,
    clock: PlaybackClock,
    partial: Vec<f32>,
    partial_len: usize,
    _stream: cpal::Stream,
}
impl NativeAudioOutput {
    pub fn open(stop: Arc<AtomicBool>) -> SessionResult<Self> {
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
        let clock = PlaybackClock::new(
            supported.sample_rate(),
            supported.channels(),
            converter.output_delay(),
        );
        let chunk_samples = 512 * usize::from(supported.channels());
        let partial = if converter.needs_conversion() {
            vec![0.0; chunk_samples]
        } else {
            Vec::new()
        };
        let format = supported.sample_format();
        let config = supported.into();
        let wake = AudioWakeRegistration::default();
        let (producer, consumer, _) = NativeAudioQueue::create(4, chunk_samples, wake.clone())
            .map_err(|_| error("SHINKI_EMU_AUDIO_QUEUE", "cannot create device queue"))?;
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match format {
            cpal::SampleFormat::I8 => {
                build::<i8>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I16 => {
                build::<i16>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I24 => {
                build::<cpal::I24>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I32 => {
                build::<i32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::I64 => {
                build::<i64>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U8 => {
                build::<u8>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U16 => {
                build::<u16>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U32 => {
                build::<u32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::U64 => {
                build::<u64>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::F32 => {
                build::<f32>(&device, config, consumer, wake, failed.clone(), stop)
            }
            cpal::SampleFormat::F64 => {
                build::<f64>(&device, config, consumer, wake, failed.clone(), stop)
            }
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
            producer,
            failed,
            converter,
            clock,
            partial,
            partial_len: 0,
            _stream: stream,
        })
    }
    /// Observes the device queue; call `check` separately for device failure.
    pub fn progress(&self) -> NativeAudioProgress {
        self.clock.progress(&self.producer)
    }
    pub fn check(&self) -> SessionResult<()> {
        if self.failed.load(Ordering::Acquire) {
            Err(error(
                "SHINKI_EMU_AUDIO_DEVICE",
                "native output device failed",
            ))
        } else {
            Ok(())
        }
    }
    pub fn write(
        &mut self,
        samples: Vec<f32>,
        stop: &AtomicBool,
    ) -> SessionResult<Option<Vec<f32>>> {
        // wait_for_capacity observes this independent flag at most every 20 ms.
        // The device error callback also sets it, waking a blocked producer.
        self.check()?;
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        if self.converter.needs_conversion() {
            let producer = &mut self.producer;
            let partial = &mut self.partial;
            let partial_len = &mut self.partial_len;
            let failed = &self.failed;
            self.converter
                .convert(&samples, |mut converted| {
                    while !converted.is_empty() {
                        if stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let count = (partial.len() - *partial_len).min(converted.len());
                        partial[*partial_len..*partial_len + count]
                            .copy_from_slice(&converted[..count]);
                        *partial_len += count;
                        converted = &converted[count..];
                        if *partial_len == partial.len() {
                            producer
                                .wait_for_capacity(partial.len(), stop)
                                .map_err(|_| "SHINKI_EMU_AUDIO_QUEUE".to_owned())?;
                            if failed.load(Ordering::Acquire) {
                                return Err("SHINKI_EMU_AUDIO_DEVICE".into());
                            }
                            if stop.load(Ordering::Acquire) {
                                return Ok(());
                            }
                            *partial = producer
                                .submit(std::mem::take(partial))
                                .map_err(|_| "SHINKI_EMU_AUDIO_QUEUE".to_owned())?;
                            *partial_len = 0;
                        }
                    }
                    Ok(())
                })
                .map_err(conversion_error)?;
            self.check()?;
            if stop.load(Ordering::Acquire) {
                return Ok(None);
            }
            self.clock.accept(samples.len());
            return Ok(Some(samples));
        }
        self.producer
            .wait_for_capacity(samples.len(), stop)
            .map_err(|_| error("SHINKI_EMU_AUDIO_QUEUE", "device queue wait failed"))?;
        if self.failed.load(Ordering::Acquire) {
            return Err(error(
                "SHINKI_EMU_AUDIO_DEVICE",
                "native output device failed",
            ));
        }
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let accepted = samples.len();
        let recycled = self
            .producer
            .submit(samples)
            .map_err(|_| error("SHINKI_EMU_AUDIO_QUEUE", "device queue submission failed"))?;
        self.clock.accept(accepted);
        Ok(Some(recycled))
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
fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut consumer: NativeAudioConsumer,
    wake: AudioWakeRegistration,
    failed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error> {
    let error_wake = wake.clone();
    let (sample_rate, channels) = (config.sample_rate, config.channels);
    device.build_output_stream(
        config,
        move |output: &mut [T], info| {
            // Record before filling so a concurrent progress query can only
            // under-report this callback's not yet audible samples.
            let time = info.timestamp();
            consumer.record_output_latency(
                time.playback.duration_since(time.callback),
                sample_rate,
                channels,
                output.len(),
            );
            let mut scratch = [0.0; 1024];
            for chunk in output.chunks_mut(scratch.len()) {
                consumer.fill_output_f32(&mut scratch[..chunk.len()]);
                for (target, source) in chunk.iter_mut().zip(&scratch) {
                    *target = T::from_sample(source.clamp(-1.0, 1.0));
                }
            }
            wake.notify();
        },
        move |_| {
            failed.store(true, Ordering::Release);
            stop.store(true, Ordering::Release);
            error_wake.notify();
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn progress_follows_audible_device_frames_and_never_exceeds_accepted() {
        // 96 kHz stereo device with 8 frames of resampler delay: 2 device
        // frames per source frame, 96 device frames per queued chunk.
        let (mut producer, mut consumer, _) =
            NativeAudioQueue::create(4, 192, AudioWakeRegistration::default()).unwrap();
        let mut clock = PlaybackClock::new(96_000, 2, 8);
        let played = |clock: &PlaybackClock, producer: &NativeAudioProducer| {
            clock.progress(producer).played_frames
        };
        assert_eq!(clock.progress(&producer), NativeAudioProgress::default());
        producer.submit(vec![0.0; 192]).unwrap();
        clock.accept(96);
        assert_eq!(clock.progress(&producer).accepted_frames, 48);
        assert_eq!(played(&clock, &producer), 0);

        consumer.pop_samples(&mut [0.0; 64]);
        assert_eq!(played(&clock, &producer), 12);
        // Larger reported latency hides more frames, but never retracts.
        consumer.record_output_latency(Duration::ZERO, 96_000, 2, 64);
        assert_eq!(played(&clock, &producer), 12);
        consumer.record_output_latency(Duration::ZERO, 96_000, 2, 0);
        consumer.pop_samples(&mut [0.0; 128]);
        assert_eq!(played(&clock, &producer), 44);
        // Underrun silence does not advance the source clock.
        assert!(consumer.fill_output_f32(&mut [0.0; 64]));
        assert_eq!(played(&clock, &producer), 44);

        // A callback may drain a chunk before the writer records acceptance.
        producer.submit(vec![0.0; 192]).unwrap();
        consumer.pop_samples(&mut [0.0; 192]);
        assert_eq!(played(&clock, &producer), 48);
        clock.accept(96);
        let progress = clock.progress(&producer);
        assert_eq!(progress.accepted_frames, 96);
        assert_eq!(progress.played_frames, 92);
    }
}
