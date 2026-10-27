//! CMVS movie player: MGV1 container playback per the recovered native
//! pipeline (`MoviePlayer_open 0x412800`, decode worker `0x417370`,
//! present `0x412A90`).
//!
//! The container is `[32B header][4B*N index][Vorbis ogg blob][Theora ogg
//! stream]`. Open starts playback immediately: the Ogg Vorbis blob plays
//! through channel sound slot 2 with looping enabled, and the audio-slot
//! position acts as the playback clock (`SndSlot_positionMs 0x48C160`).
//! `player->this[8]` — the frame counter read by command 198 — advances as
//! `position_ms / ms_per_frame`. At the index boundary the engine releases
//! the player (`0x41B4C0`), so command 198 then returns -1.
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
    pub(super) rgba8: roukaku_media_core::OwnedPixelBuffer,
}

pub(super) struct MoviePlayer {
    /// Index entry count: the saturating upper bound of the frame counter
    /// (header dword 4, byte offset 16).
    pub(super) index_count: u32,
    /// MGV index high words map presentation frames to zero-based Theora
    /// decode indices; repeated entries hold the same decoded image.
    frame_indices: Vec<u16>,
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
    #[cfg(feature = "video-ffmpeg")]
    video_eof: bool,
}

impl MoviePlayer {
    /// Parses the MGV1 container and starts the audio on slot 2.
    pub(super) fn open(
        bytes: &[u8],
        audio: &mut audio::AudioBridge,
        native_gain: u32,
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
        let frame_indices = bytes[HEADER_SIZE..index_end]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|entry| u16::from_le_bytes([entry[2], entry[3]]))
            .collect();
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
        audio.play_channel(MOVIE_AUDIO_CHANNEL, clip, true, native_gain)?;
        #[cfg(feature = "video-ffmpeg")]
        let video = if video_bytes == 0 {
            None
        } else {
            match shinki_emu_sdk::video::VideoDecoderWorker::open(
                "ogg".into(),
                bytes[ogg_end..video_end].into(),
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
            frame_indices,
            fps_num,
            fps_den,
            audio_frames,
            audio_rate,
            clock_frames_written: 0,
            #[cfg(feature = "video-ffmpeg")]
            video,
            frames: std::collections::VecDeque::new(),
            #[cfg(feature = "video-ffmpeg")]
            video_eof: false,
        })
    }

    /// Advances the audio clock and returns the current presentation index.
    /// The session releases the player when it reaches `index_count`.
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
    pub(super) fn pump(&mut self, target_frame: u32) -> Option<(u64, std::sync::Arc<MovieFrame>)> {
        let target = u64::from(*self.frame_indices.get(target_frame as usize)?);
        self.retire_due_frames(target);
        #[cfg(feature = "video-ffmpeg")]
        {
            let mut keep_worker = true;
            if let Some(worker) = self.video.as_mut() {
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
                                // SDK sequences start at one; MGV indices at zero.
                                self.frames.push_back((
                                    meta.sequence - 1,
                                    std::sync::Arc::new(MovieFrame {
                                        width: meta.width,
                                        height: meta.height,
                                        rgba8: bgra_to_rgba(&bgra8).into(),
                                    }),
                                ));
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
        self.retire_due_frames(target);
        #[cfg(feature = "video-ffmpeg")]
        if !self.video_eof {
            if let Some(worker) = self.video.as_mut() {
                let available = FRAME_CACHE - self.frames.len();
                if available != 0 {
                    // Only decode into free slots. Never evict a frame
                    // whose presentation time has not arrived.
                    let _ = worker.request_next(available, 32 * 1024 * 1024);
                }
            }
        }
        self.frames
            .front()
            .filter(|(sequence, _)| *sequence <= target)
            .map(|(sequence, frame)| (*sequence, std::sync::Arc::clone(frame)))
    }

    fn retire_due_frames(&mut self, target: u64) {
        while self
            .frames
            .get(1)
            .is_some_and(|(sequence, _)| *sequence <= target)
        {
            self.frames.pop_front();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> MoviePlayer {
        MoviePlayer {
            index_count: 20,
            frame_indices: (0..20).collect(),
            fps_num: 24,
            fps_den: 1,
            audio_frames: 48_000.0 * 10.0,
            audio_rate: 48_000,
            clock_frames_written: 0,
            #[cfg(feature = "video-ffmpeg")]
            video: None,
            frames: std::collections::VecDeque::new(),
            #[cfg(feature = "video-ffmpeg")]
            video_eof: false,
        }
    }

    fn frame() -> std::sync::Arc<MovieFrame> {
        std::sync::Arc::new(MovieFrame {
            width: 1,
            height: 1,
            rgba8: vec![255, 0, 0, 255].into(),
        })
    }

    #[test]
    fn decoding_ahead_does_not_present_a_future_frame() {
        let mut player = player();
        player.frames.push_back((2, frame()));
        player.frames.push_back((3, frame()));
        assert!(player.pump(1).is_none());
    }

    #[test]
    fn mgv_repeated_indices_hold_the_due_image_until_the_next_decode_index() {
        let mut player = player();
        player.frame_indices = vec![0, 0, 1, 1, 2];
        player.index_count = 5;
        let images = [frame(), frame(), frame()];
        for (index, image) in images.iter().enumerate() {
            player.frames.push_back((index as u64, image.clone()));
        }
        for (presentation, decoded) in [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)] {
            let (sequence, image) = player.pump(presentation).unwrap();
            assert_eq!(sequence, decoded as u64);
            assert!(std::sync::Arc::ptr_eq(&image, &images[decoded]));
        }
        assert!(player.pump(5).is_none());
    }

    #[test]
    fn late_decode_replaces_the_held_frame_without_advancing_the_clock() {
        let mut player = player();
        player.frames.push_back((0, frame()));
        assert_eq!(player.pump(3).unwrap().0, 0);
        player.frames.push_back((2, frame()));
        player.frames.push_back((4, frame()));
        assert_eq!(player.pump(3).unwrap().0, 2);
        assert_eq!(player.pump(3).unwrap().0, 2);
        assert_eq!(player.pump(4).unwrap().0, 4);
    }
}
