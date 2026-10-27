use super::*;
use crate::audio::native_stretch::NativeMono175;

/// Native source-format notifications,48D780 conversion, then the shared48k bus.
pub(in crate::session::audio) struct NativePcm {
    stretch: Option<NativeMono175>,
    source: Vec<i16>,
    repeat_source: Option<Vec<i16>>,
    source_repeated: bool,
    continuation: Option<(Vec<i16>, Option<Vec<i16>>)>,
    decoded: Vec<f32>,
    source_rate: u32,
    device_rate: u32,
    channels: usize,
    device_chunk_frames: usize,
    source_cursor: usize,
    looping: bool,
    raw_eof: bool,
    pub(in crate::session::audio) decoded_bytes: u32,
    pub(in crate::session::audio) finished: bool,
    boundary: bool,
    level_ring: [[u32; 2]; 3],
    level_index: usize,
}

impl NativePcm {
    pub(in crate::session::audio) fn raw(
        source_rate: u32,
        channels: u16,
        device_rate: u32,
        native_chunk_bytes: u32,
        frames: &[f32],
    ) -> Self {
        let channels = usize::from(channels);
        let device_chunk_frames = native_chunk_bytes as usize / 4;
        let source_chunk_frames = if source_rate == 44100 && device_rate == 22050 {
            device_chunk_frames * 2
        } else if source_rate == 22050 && device_rate == 44100 {
            device_chunk_frames / 2
        } else {
            device_chunk_frames
        };
        let mut source = Vec::with_capacity(frames.len() / 2 * channels);
        for frame in frames.as_chunks::<2>().0 {
            for &sample in &frame[..channels] {
                source.push((sample * 32768.0).clamp(-32768.0, 32767.0) as i16);
            }
        }
        Self {
            stretch: None,
            source,
            repeat_source: None,
            source_repeated: false,
            continuation: None,
            decoded: vec![0.0; source_chunk_frames * channels],
            source_rate,
            device_rate,
            channels,
            device_chunk_frames,
            source_cursor: 0,
            looping: false,
            raw_eof: false,
            decoded_bytes: 0,
            finished: false,
            boundary: false,
            level_ring: [[0; 2]; 3],
            level_index: 0,
        }
    }

    pub(in crate::session::audio) fn stretched(state: NativeMono175, frames: &[f32]) -> Self {
        let source_rate = state.source_rate();
        let device_rate = state.device_rate();
        let chunk_samples = state.source_chunk_samples();
        let device_frames = chunk_samples * device_rate as usize / source_rate as usize;
        let mut pcm = Self::raw(
            source_rate,
            1,
            device_rate,
            device_frames as u32 * 4,
            frames,
        );
        pcm.stretch = Some(state);
        pcm
    }

    pub(in crate::session::audio) fn attach_stretch(&mut self, state: NativeMono175) {
        self.device_rate = state.device_rate();
        self.source_rate = state.source_rate();
        self.decoded.resize(state.source_chunk_samples(), 0.0);
        self.device_chunk_frames =
            self.decoded.len() * self.device_rate as usize / self.source_rate as usize;
        self.stretch = Some(state);
    }

    pub(in crate::session::audio) fn set_repeat(&mut self, frames: &[f32]) {
        let mut repeat = Vec::with_capacity(frames.len() / 2 * self.channels);
        for frame in frames.as_chunks::<2>().0 {
            for &sample in &frame[..self.channels] {
                repeat.push((sample * 32768.0).clamp(-32768.0, 32767.0) as i16);
            }
        }
        self.repeat_source = Some(repeat);
    }

    pub(in crate::session::audio) fn set_continuation(&mut self, next: Self) {
        self.continuation = Some((next.source, next.repeat_source));
    }

    pub(in crate::session::audio) fn buffer_capacity(&self) -> usize {
        let host_frames = (CHUNK_FRAMES * self.device_rate as usize).div_ceil(OUTPUT_RATE as usize);
        (self.device_chunk_frames + host_frames + 2) * 2
    }

    pub(in crate::session::audio) fn device_rate(&self) -> u32 {
        self.device_rate
    }
    pub(in crate::session::audio) fn source_rate(&self) -> u32 {
        self.source_rate
    }

    pub(in crate::session::audio) fn decoder_source_frame(&self) -> f64 {
        let cursor = self
            .stretch
            .as_ref()
            .map_or(self.source_cursor, NativeMono175::decoder_position_samples);
        cursor as f64 / self.channels as f64
    }

    pub(in crate::session::audio) fn levels(&self) -> [u32; 2] {
        self.level_ring[self.level_index]
    }

    pub(in crate::session::audio) fn refill(
        &mut self,
        frames: &mut Vec<f32>,
        position: &mut f64,
    ) -> bool {
        let consumed = (*position as usize).min(frames.len() / 2);
        if consumed != 0 {
            frames.drain(..consumed * 2);
            *position -= consumed as f64;
        }
        let needed = (*position
            + CHUNK_FRAMES as f64 * f64::from(self.device_rate) / f64::from(OUTPUT_RATE))
        .ceil() as usize
            + 1;
        while frames.len() / 2 < needed && !self.finished && !self.boundary {
            let was_stretched = self.stretch.is_some();
            let initialized_samples = if let Some(state) = self.stretch.as_mut() {
                let chunk = state.next_chunk(
                    &self.source,
                    self.repeat_source.as_deref(),
                    &mut self.decoded,
                );
                self.decoded_bytes = self.decoded_bytes.wrapping_add(chunk.native_decoded_bytes);
                self.finished = chunk.finished;
                self.boundary = chunk.indeterminate_tail;
                if chunk.finished {
                    self.source_cursor = state.decoder_position_samples();
                    self.source_repeated = state.decoder_repeated();
                    self.raw_eof = self.source_cursor >= self.source.len();
                    //48BC20 destroys a completed helper after its final conversion.
                    self.stretch = None;
                }
                chunk.initialized_samples
            } else {
                let mut written = 0;
                while written < self.decoded.len() && !self.raw_eof {
                    let source = if self.source_repeated {
                        self.repeat_source.as_deref().unwrap_or(&self.source)
                    } else {
                        &self.source
                    };
                    if self.source_cursor == source.len() {
                        if let Some((next, repeat)) = self.continuation.take() {
                            self.source = next;
                            self.repeat_source = repeat;
                            self.source_repeated = false;
                            self.source_cursor = 0;
                            self.looping = true;
                            if self.source.is_empty() {
                                break;
                            }
                        } else {
                            if !self.looping || self.source.is_empty() {
                                break;
                            }
                            self.source_cursor = 0;
                            self.source_repeated = true;
                        }
                    }
                    let source = if self.source_repeated {
                        self.repeat_source.as_deref().unwrap_or(&self.source)
                    } else {
                        &self.source
                    };
                    self.decoded[written] = source[self.source_cursor] as f32 / 32768.0;
                    self.source_cursor += 1;
                    written += 1;
                }
                self.decoded_bytes = self.decoded_bytes.wrapping_add(written as u32 * 2);
                self.finished = written < self.decoded.len();
                self.raw_eof = self.finished;
                written
            };
            let before = frames.len();
            let source_frames = initialized_samples / self.channels;
            let stride = if self.source_rate == 44100 && self.device_rate == 22050 {
                2
            } else {
                1
            };
            let output_frames = source_frames / stride;
            let copies = if self.source_rate == 22050 && self.device_rate == 44100 {
                2
            } else {
                1
            };
            let record_levels = output_frames * copies == self.device_chunk_frames
                || !was_stretched && initialized_samples != 0;
            let mut sums = [0u32; 2];
            for index in 0..output_frames {
                let base = index * stride * self.channels;
                let left = self.decoded[base];
                let right = self.decoded[base + self.channels - 1];
                if record_levels {
                    for (channel, sample) in [left, right].into_iter().enumerate() {
                        let amplitude = ((sample * 32768.0) as i32).unsigned_abs();
                        sums[channel] = sums[channel].wrapping_add(amplitude * copies as u32);
                    }
                }
                frames.extend_from_slice(&[left, right]);
                if copies == 2 {
                    frames.extend_from_slice(&[left, right]);
                }
            }
            //48BC20 zeros the native device buffer before a raw decoder read.
            //An unknown stretch suffix is never part of that initialized silence.
            if !was_stretched && initialized_samples != 0 {
                frames.resize(before + self.device_chunk_frames * 2, 0.0);
            }
            if record_levels {
                self.level_ring[self.level_index] =
                    sums.map(|sum| sum / self.device_chunk_frames as u32);
                self.level_index = (self.level_index + 1) % 3;
            }
        }
        self.boundary
    }

    /// Returns true when the retained decoder is already at EOF.
    pub(in crate::session::audio) fn start(&mut self, looping: bool) -> bool {
        self.looping = looping;
        if let Some(state) = self.stretch.as_mut() {
            state.set_looping(looping);
            false
        } else {
            self.finished = false;
            if self.raw_eof {
                self.boundary = false;
            }
            self.raw_eof
        }
    }

    pub(in crate::session::audio) fn restart(&mut self, looping: bool) {
        self.looping = looping;
        if let Some(state) = self.stretch.as_mut() {
            state.restart(looping);
        } else {
            self.source_cursor = 0;
            self.source_repeated = false;
            self.raw_eof = false;
        }
        self.finished = false;
        self.boundary = false;
    }
}
