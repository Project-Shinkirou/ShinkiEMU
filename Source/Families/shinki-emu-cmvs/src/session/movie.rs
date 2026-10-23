//! CMVS movie player: MGV1 container playback per the recovered native
//! pipeline (`MoviePlayer_open 0x412800`, decode worker `0x417370`,
//! present `0x412A90`).
//!
//! The container is `[32B header][4B*N index][Vorbis ogg blob][Theora ogg
//! stream]`. Open starts playback immediately: the Ogg Vorbis blob plays
//! through channel sound slot 2 with looping enabled, and the audio-slot
//! position acts as the playback clock (`SndSlot_positionMs 0x48C160`).
//! `player->this[8]` — the frame counter read by command 198 — advances as
//! `position_ms / ms_per_frame`, saturating at the index entry count; the
//! counter freezes when the looping audio clock wraps, which is the
//! script-visible EOF signal.
//!
//! Video decode runs through the SDK `VideoDecoderWorker` (the Theora
//! stream is a standard Ogg elementary stream). Without the FFmpeg
//! provider the player still runs the audio clock and the frame counter,
//! so the script lifecycle stays native-faithful; `has_video_stream()`
//! reports the gap honestly.

use super::*;
use crate::family::error;
use shinki_emu_sdk::runtime::SessionResult;
use std::sync::atomic::AtomicBool;

/// Byte size of the MGV1 header.
const HEADER_SIZE: usize = 32;
/// Channel sound slot the movie audio plays on (`this+592+4*2`).
pub(super) const MOVIE_AUDIO_CHANNEL: u8 = 2;
/// The native decode worker caches eight decoded frames.
#[cfg(feature = "video-ffmpeg")]
const FRAME_CACHE: usize = 8;

/// One decoded presentation frame in tightly packed RGBA8.
pub(super) struct MovieFrame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba8: Vec<u8>,
}

pub(super) struct MoviePlayer {
    /// Index entry count: the saturating upper bound of the frame counter
    /// (header dword 4, byte offset 16).
    pub(super) index_count: u32,
    /// Frames per second as `fps_num / fps_den` (dwords 7/6).
    fps_num: u32,
    fps_den: u32,
    audio_frames: f64,
    audio_rate: u32,
    /// Total output frames the audio worker reported for slot 2 so far.
    clock_frames_written: u64,
    /// Incremental Theora decode worker; absent without the FFmpeg
    /// provider (the audio clock and counter still run).
    #[cfg(feature = "video-ffmpeg")]
    video: Option<shinki_emu_sdk::video::VideoDecoderWorker>,
    /// Bounded decoded-frame ring keyed by decode sequence; the native
    /// decode worker keeps an eight-frame cache, this mirrors it.
    frames: std::collections::VecDeque<(u64, std::sync::Arc<MovieFrame>)>,
    video_eof: bool,
}

impl MoviePlayer {
    /// Parses the MGV1 container and starts the audio on slot 2.
    pub(super) fn open(
        bytes: &[u8],
        audio: &mut audio::AudioBridge,
        cancelled: &AtomicBool,
    ) -> SessionResult<Self> {
        let bad = |code: &'static str, message: &'static str| error(code, message);
        if bytes.len() < HEADER_SIZE || &bytes[0..4] != b"MGV1" {
            return Err(bad(
                "SHINKI_EMU_CMVS_MGV_HEADER",
                "movie is not an MGV1 stream",
            ));
        }
        let video_bytes = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let ogg_bytes = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let index_count = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let fps_den = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let fps_num = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
        if fps_num == 0 || fps_den == 0 || index_count == 0 {
            return Err(bad(
                "SHINKI_EMU_CMVS_MGV_TIMING",
                "movie timing fields are invalid",
            ));
        }
        let index_end = HEADER_SIZE
            .checked_add(index_count as usize * 4)
            .ok_or_else(|| bad("SHINKI_EMU_CMVS_MGV_HEADER", "movie index size overflowed"))?;
        let ogg_end = index_end
            .checked_add(ogg_bytes)
            .ok_or_else(|| bad("SHINKI_EMU_CMVS_MGV_HEADER", "movie audio size overflowed"))?;
        let video_end = ogg_end
            .checked_add(video_bytes)
            .ok_or_else(|| bad("SHINKI_EMU_CMVS_MGV_HEADER", "movie video size overflowed"))?;
        if video_end > bytes.len() {
            return Err(bad(
                "SHINKI_EMU_CMVS_MGV_HEADER",
                "movie ranges exceed the file",
            ));
        }
        // The embedded Ogg blob decodes as ordinary Vorbis audio.
        let clip_data = crate::audio::decode_audio_public(
            &bytes[index_end..ogg_end],
            audio::MAX_CLIP_FRAMES,
            cancelled,
        )
        .map_err(crate::family::core)?;
        let audio_frames = clip_data.frames.len() as f64;
        let audio_rate = clip_data.sample_rate;
        let clip = audio::DecodedClip::from_kira(clip_data);
        audio.play_channel(MOVIE_AUDIO_CHANNEL, clip, true)?;
        #[cfg(feature = "video-ffmpeg")]
        let video = if video_bytes == 0 {
            None
        } else {
            match shinki_emu_sdk::video::VideoDecoderWorker::open(
                "ogg".into(),
                bytes[ogg_end..video_end].to_vec().into(),
                {
                    let mut limits = shinki_emu_sdk::video::FfmpegStreamLimits::default();
                    // 1080p Theora frames routinely exceed the 100 ms
                    // default tick; the movie paces itself on the audio
                    // clock, so decode needs a wider budget and window.
                    limits.max_tick_us = 2_000_000;
                    limits.max_video_lead_us = 10_000_000;
                    limits.max_video_lag_us = 10_000_000;
                    limits
                },
                None,
            ) {
                Ok(worker) => Some(worker),
                Err(failure) => {
                    tracing::warn!(
                        event = "cmvs.session.movie_video_unavailable",
                        error = %failure.code()
                    );
                    None
                }
            }
        };
        Ok(Self {
            index_count,
            fps_num,
            fps_den,
            audio_frames,
            audio_rate,
            clock_frames_written: 0,
            #[cfg(feature = "video-ffmpeg")]
            video,
            frames: std::collections::VecDeque::new(),
            video_eof: false,
        })
    }

    /// Advances the audio clock and returns the current frame counter
    /// (`player->this[8]`): `position_ms / ms_per_frame`, saturating at the
    /// index count. The counter freezes once the looping audio clock wraps.
    pub(super) fn current_frame(&mut self, audio: &audio::AudioBridge) -> u32 {
        let written = audio.channel_position_frames(MOVIE_AUDIO_CHANNEL);
        self.clock_frames_written = self.clock_frames_written.max(written);
        let position_ms = (self.clock_frames_written as f64) * 1000.0 / 48_000.0;
        let audio_ms = self.audio_frames * 1000.0 / f64::from(self.audio_rate);
        if position_ms >= audio_ms {
            return self.index_count;
        }
        let ms_per_frame = 1000.0 * f64::from(self.fps_den) / f64::from(self.fps_num);
        let frame = (position_ms / ms_per_frame).floor();
        frame.clamp(0.0, f64::from(self.index_count)) as u32
    }
    /// Pumps the incremental decode worker and returns the frame whose
    /// decode sequence best matches the audio-clock target, if decoded.
    pub(super) fn pump(&mut self, target_frame: u32) -> Option<std::sync::Arc<MovieFrame>> {
        tracing::debug!(
            event = "cmvs.session.movie_pump",
            target = target_frame,
            cached = self.frames.len(),
            eof = self.video_eof
        );
        #[cfg(feature = "video-ffmpeg")]
        {
            let mut keep_worker = true;
            if let Some(worker) = self.video.as_mut() {
                // Request a bounded batch; `request_next` rejects while
                // one is already outstanding, which covers the busy case.
                if !self.video_eof {
                    let _ = worker.request_next(4, 32 * 1024 * 1024);
                }
                match worker.poll() {
                    Ok(Some(shinki_emu_sdk::video::DecodeCompletion::Opened(_))) => {}
                    Ok(Some(shinki_emu_sdk::video::DecodeCompletion::Packets { packets, eof })) => {
                        self.video_eof = eof;
                        for packet in packets {
                            if let shinki_emu_sdk::video::DecodedMediaPacket::Video {
                                packet: meta,
                                bgra8,
                            } = packet
                            {
                                let rgba8 = bgra_to_rgba(&bgra8);
                                self.frames.push_back((
                                    meta.sequence,
                                    std::sync::Arc::new(MovieFrame {
                                        width: meta.width,
                                        height: meta.height,
                                        rgba8,
                                    }),
                                ));
                                while self.frames.len() > FRAME_CACHE {
                                    self.frames.pop_front();
                                }
                            }
                        }
                    }
                    Ok(Some(shinki_emu_sdk::video::DecodeCompletion::Seeked { .. })) => {}
                    Ok(None) => {}
                    Err(failure) => {
                        tracing::warn!(
                            event = "cmvs.session.movie_video_failed",
                            error = %failure
                        );
                        keep_worker = false;
                    }
                }
            }
            if !keep_worker {
                if let Some(worker) = self.video.take() {
                    let _ = worker.close();
                }
            }
        }
        // Decode sequences map 1:1 onto container frames in decode order;
        // pick the newest cached frame at or before the target.
        let target = u64::from(target_frame.min(self.index_count));
        let mut best: Option<u64> = None;
        for (sequence, _) in self.frames.iter() {
            if *sequence <= target {
                best = Some(*sequence);
            } else {
                break;
            }
        }
        let sequence = best.or_else(|| self.frames.back().map(|(sequence, _)| *sequence))?;
        self.frames
            .iter()
            .find(|(candidate, _)| *candidate == sequence)
            .map(|(_, frame)| std::sync::Arc::clone(frame))
    }

    /// True when the Theora stream is retained for a video decoder.
    pub(super) fn has_video_stream(&self) -> bool {
        #[cfg(feature = "video-ffmpeg")]
        {
            self.video.is_some()
        }
        #[cfg(not(feature = "video-ffmpeg"))]
        {
            false
        }
    }

    /// Stops the slot-2 audio and releases the player
    /// (`MoviePlayer_close 0x412670`).
    pub(super) fn close(&mut self, audio: &mut audio::AudioBridge) -> SessionResult<()> {
        #[cfg(feature = "video-ffmpeg")]
        let video = self
            .video
            .take()
            .map(|worker| worker.close().map_err(crate::family::core))
            .transpose();
        self.frames.clear();
        let audio = if audio.is_cancelled() {
            Ok(())
        } else {
            audio.stop_channel(MOVIE_AUDIO_CHANNEL)
        };
        #[cfg(feature = "video-ffmpeg")]
        video?;
        audio
    }
}

/// Converts tightly packed BGRA8 into RGBA8.
#[cfg(feature = "video-ffmpeg")]
fn bgra_to_rgba(bgra8: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0_u8; bgra8.len()];
    for (dst, src) in rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(bgra8.as_chunks::<4>().0)
    {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    rgba
}
