use crate::{Error, Result, audio::Audio};
use image::RgbaImage;
use shinki_emu_sdk::{
    CoreError,
    video::{
        DecodeCompletion, DecodedMediaPacket, FfmpegAudioOutputFormat, FfmpegStreamLimits,
        PcmQueue, VideoDecoderWorker,
    },
};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

const MAX_MOVIE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_VIDEO_BYTES: usize = 128 * 1024 * 1024;
const MAX_DECODE_BATCH_BYTES: usize = 64 * 1024 * 1024;
const MAX_VIDEO_FRAMES: usize = 32;
const MAX_AUDIO_PACKET_BYTES: usize = 64 * 1024;
const BATCH: usize = 4;
const MIX_RATE: u32 = 48_000;

pub(crate) struct MovieAudio {
    queue: Mutex<PcmQueue>,
    accepted_position: AtomicU64,
}

impl MovieAudio {
    pub(crate) fn mix(&self, samples: &mut [f32]) -> Result<u64> {
        self.queue
            .lock()
            .map_err(movie_error)?
            .mix_into(samples)
            .map(|result| result.position_us)
            .map_err(movie_error)
    }

    pub(crate) fn accepted(&self, position: u64) {
        self.accepted_position.store(position, Ordering::Release);
    }
}

pub(crate) struct Movie {
    decoder: VideoDecoderWorker,
    audio: Option<Arc<MovieAudio>>,
    next_audio_frame: Option<u64>,
    video: VecDeque<(u64, RgbaImage)>,
    frame: Option<RgbaImage>,
    bytes: usize,
    clock: Duration,
    duration: Duration,
    has_audio: bool,
    clock_started: bool,
    opened: bool,
    pending: bool,
    eof: bool,
}

impl Movie {
    pub(crate) fn open(path: PathBuf) -> Result<Self> {
        let decoder = VideoDecoderWorker::open_file(
            move |stop| {
                let convert = |error: std::io::Error| {
                    CoreError::invalid("SHINKI_EMU_ADV32_MOVIE_SOURCE", error.to_string())
                };
                let mut source = File::open(&path).map_err(convert)?;
                let length = source.metadata().map_err(convert)?.len();
                if length == 0 || length > MAX_MOVIE_BYTES {
                    return Err(CoreError::invalid(
                        "SHINKI_EMU_ADV32_MOVIE_SOURCE",
                        "invalid movie length",
                    ));
                }
                let mut staged = tempfile::Builder::new()
                    .prefix("adv32-video-")
                    .tempfile()
                    .map_err(convert)?;
                let mut buffer = vec![0; 1024 * 1024];
                let mut remaining = length;
                while remaining != 0 {
                    if stop.load(Ordering::Acquire) {
                        return Err(CoreError::invalid(
                            "SHINKI_EMU_ADV32_MOVIE_CANCELLED",
                            "movie open cancelled",
                        ));
                    }
                    let count = (remaining as usize).min(buffer.len());
                    source.read_exact(&mut buffer[..count]).map_err(convert)?;
                    staged.write_all(&buffer[..count]).map_err(convert)?;
                    remaining -= count as u64;
                }
                Ok(staged)
            },
            FfmpegStreamLimits {
                max_encoded_bytes: MAX_MOVIE_BYTES as usize,
                max_audio_packet_bytes: MAX_AUDIO_PACKET_BYTES,
                ..Default::default()
            },
            Some(FfmpegAudioOutputFormat {
                sample_rate: MIX_RATE,
                channels: 2,
            }),
        )
        .map_err(movie_error)?;
        Ok(Self {
            decoder,
            audio: None,
            next_audio_frame: None,
            video: VecDeque::new(),
            frame: None,
            bytes: 0,
            clock: Duration::ZERO,
            duration: Duration::ZERO,
            has_audio: false,
            clock_started: false,
            opened: false,
            pending: true,
            eof: false,
        })
    }

    pub(crate) fn advance(&mut self, host: &Audio, elapsed: Duration) -> Result<bool> {
        if self.pending
            && let Some(result) = self.decoder.poll().map_err(movie_error)?
        {
            self.pending = false;
            match result {
                DecodeCompletion::Opened(config) => {
                    if !config.has_video {
                        return Err(Error::Invalid("ADV32 movie without a picture stream"));
                    }
                    self.opened = true;
                    self.duration = Duration::from_micros(config.duration_us);
                    self.has_audio = config.has_audio;
                }
                DecodeCompletion::Packets { packets, eof } => {
                    self.eof = eof;
                    for packet in packets {
                        match packet {
                            DecodedMediaPacket::Audio {
                                mut packet,
                                samples,
                            } => {
                                packet.pts_us = align_audio_pts(
                                    &mut self.next_audio_frame,
                                    packet.pts_us,
                                    packet.frame_count,
                                )?;
                                if self.has_audio && self.audio.is_none() {
                                    let origin =
                                        self.video.front().map_or(packet.pts_us, |(pts, _)| {
                                            (*pts).min(packet.pts_us)
                                        });
                                    self.start_clock(origin);
                                    let queue = PcmQueue::new(
                                        MIX_RATE,
                                        2,
                                        packet.generation,
                                        origin,
                                        MIX_RATE as usize * 3,
                                        128,
                                    )
                                    .map_err(movie_error)?;
                                    let track = Arc::new(MovieAudio {
                                        queue: Mutex::new(queue),
                                        accepted_position: AtomicU64::new(origin),
                                    });
                                    host.set_movie_audio(Some(track.clone()))?;
                                    self.audio = Some(track);
                                }
                                let audio = self
                                    .audio
                                    .as_ref()
                                    .ok_or(Error::Invalid("unexpected ADV32 movie audio"))?;
                                audio
                                    .queue
                                    .lock()
                                    .map_err(movie_error)?
                                    .push(packet, samples)
                                    .map_err(movie_error)?;
                            }
                            DecodedMediaPacket::Video { packet, bgra8 } => {
                                if !self.has_audio && !self.clock_started {
                                    self.start_clock(packet.pts_us);
                                }
                                if self.video.len() >= MAX_VIDEO_FRAMES
                                    || bgra8.len() > MAX_VIDEO_BYTES - self.bytes
                                {
                                    continue;
                                }
                                let mut bytes = bgra8.as_ref().to_vec();
                                for pixel in bytes.as_chunks_mut::<4>().0 {
                                    pixel.swap(0, 2);
                                }
                                self.bytes += bytes.len();
                                let frame = RgbaImage::from_raw(packet.width, packet.height, bytes)
                                    .ok_or(Error::Invalid("ADV32 movie raster"))?;
                                self.video.push_back((packet.pts_us, frame));
                            }
                        }
                    }
                }
                DecodeCompletion::Seeked { .. } => {
                    return Err(Error::Invalid("unsolicited ADV32 movie seek"));
                }
            }
        }
        let (audio_position, audio_drained, audio_room) = if let Some(audio) = &self.audio {
            let queue = audio.queue.lock().map_err(movie_error)?;
            (
                Some(Duration::from_micros(
                    audio.accepted_position.load(Ordering::Acquire),
                )),
                queue.is_empty(),
                queue.can_buffer(BATCH * MAX_AUDIO_PACKET_BYTES / 4, BATCH),
            )
        } else {
            (None, true, true)
        };
        let video_room =
            self.video.len() + BATCH <= MAX_VIDEO_FRAMES && self.bytes < MAX_VIDEO_BYTES;
        let audio_needs_data = self.has_audio && audio_drained;
        if self.opened
            && !self.pending
            && !self.eof
            && (video_room || audio_needs_data)
            && audio_room
        {
            // MPEG program streams may deliver several pictures before more
            // audio. Refill the audio clock even when pictures are backed up.
            self.decoder
                .request_next(BATCH, MAX_DECODE_BATCH_BYTES)
                .map_err(movie_error)?;
            self.pending = true;
        }
        if let Some(position) = audio_position {
            self.clock = self.clock.max(position);
        }
        if self.opened
            && (!self.has_audio && (!self.video.is_empty() || self.eof)
                || self.eof && audio_drained)
        {
            self.clock = self.clock.saturating_add(elapsed);
        }
        let mut updated = false;
        while self
            .video
            .front()
            .is_some_and(|(pts, _)| Duration::from_micros(*pts) <= self.clock)
        {
            let (_, frame) = self.video.pop_front().expect("checked movie frame");
            self.bytes -= frame.as_raw().len();
            self.frame = Some(frame);
            updated = true;
        }
        Ok(updated)
    }

    fn start_clock(&mut self, origin: u64) {
        self.clock_started = true;
        self.clock = Duration::from_micros(origin);
        self.duration = self.duration.saturating_add(self.clock);
    }

    pub(crate) fn frame(&self) -> Option<&RgbaImage> {
        self.frame.as_ref()
    }

    pub(crate) fn ended(&self) -> bool {
        self.eof
            && self.video.is_empty()
            && self.clock >= self.duration
            && self
                .audio
                .as_ref()
                .is_none_or(|audio| audio.queue.lock().is_ok_and(|queue| queue.is_empty()))
    }

    pub(crate) fn close(self, host: &Audio) -> Result<()> {
        host.set_movie_audio(None)?;
        self.decoder.close().map_err(movie_error)
    }
}

fn align_audio_pts(next_frame: &mut Option<u64>, pts_us: u64, frames: u32) -> Result<u64> {
    let timestamp = (u128::from(pts_us) * u128::from(MIX_RATE) + 500_000) / 1_000_000;
    let timestamp = timestamp as u64;
    // MPEG program streams can repeat or skip one audio frame's timestamp.
    // Use the decoded sample count across that jitter and keep longer gaps.
    let start = next_frame
        .filter(|expected| expected.abs_diff(timestamp) <= u64::from(frames) + 1)
        .unwrap_or(timestamp);
    let aligned = u64::try_from(u128::from(start) * 1_000_000 / u128::from(MIX_RATE))
        .map_err(|_| Error::Invalid("ADV32 movie audio timestamp"))?;
    *next_frame = start.checked_add(u64::from(frames));
    Ok(aligned)
}

pub(crate) fn movie_error(error: impl std::fmt::Display) -> Error {
    Error::Movie(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_audio_keeps_sample_order_when_a_packet_timestamp_repeats() {
        let mut next = None;
        for (provided, expected) in [
            (500_000, 500_000),
            (552_000, 526_000),
            (552_000, 552_000),
            (604_000, 578_000),
            (604_000, 604_000),
            (710_000, 710_000),
        ] {
            assert_eq!(
                align_audio_pts(&mut next, provided, 1248).unwrap(),
                expected
            );
        }
    }
}
