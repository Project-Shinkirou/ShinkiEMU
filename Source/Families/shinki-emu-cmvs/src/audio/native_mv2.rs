// Native40C130/40BD00/40BDD0 restoration, cross-checked with GARbro's
// AudioMV2.cs (https://github.com/morkt/GARbro/blob/master/ArcFormats/Cmvs/AudioMV2.cs).
//
// Copyright (C) 2016 by morkt
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

use super::{
    NativeDecodedAudio,
    native_mv2_tables::{QUANTIZER, SYNTHESIS, WINDOW},
};
use kira::{
    Frame,
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
};
use shinki_emu_sdk::CoreError;
use std::sync::atomic::{AtomicBool, Ordering};

fn boundary(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_MV2_BOUNDARY", message)
}

/// 40BC70/40BCC0 refill a little-endian DWORD, then consume its least
/// significant bit first while assembling the requested value MSB first.
struct Bits<'a> {
    source: &'a [u8],
    cursor: usize,
    word: u32,
    remaining: u32,
}
impl Bits<'_> {
    fn read(&mut self, count: u32) -> Result<i32, CoreError> {
        let mut value = 0i32;
        for _ in 0..count {
            if self.remaining == 0 {
                self.word = u32::from_le_bytes(
                    self.source
                        .get(self.cursor..self.cursor + 4)
                        .ok_or_else(|| boundary("native MV2 bit reader accesses beyond the asset"))?
                        .try_into()
                        .unwrap(),
                );
                self.cursor += 4;
                self.remaining = 32;
            }
            value = value.wrapping_shl(1) | (self.word & 1) as i32;
            self.word >>= 1;
            self.remaining -= 1;
        }
        Ok(value)
    }
    fn count(&mut self) -> Result<u32, CoreError> {
        let mut count = 0u32;
        while self.read(1)? != 0 {
            count += 1;
        }
        Ok(count)
    }
}

struct Header {
    channels: u16,
    rate: u32,
    declared_frames: u32,
    blocks: u32,
    shift: u8,
}

pub(super) fn decode(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    if source.get(..4) != Some(b"MV2X") {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_AUDIO_CODEC",
            "filename selects CMusicMV, which requires MV2X",
        ));
    }
    let header = source
        .get(..18)
        .ok_or_else(|| boundary("native MV2 header is not fully defined"))?;
    let header = Header {
        declared_frames: u32::from_le_bytes(header[4..8].try_into().unwrap()),
        rate: u32::from(u16::from_le_bytes(header[10..12].try_into().unwrap())),
        channels: u16::from(header[12]),
        blocks: u32::from_le_bytes(header[14..18].try_into().unwrap()),
        shift: header[13],
    };
    let extent = 2u32
        .wrapping_mul(u32::from(header.channels))
        .wrapping_mul(header.declared_frames);
    let (data, repeat_data) = match unpack(source, &header, max_frames, cancelled) {
        Ok((data, repeat)) => (Ok(data), repeat),
        Err(cause) => (Err(cause), None),
    };
    Ok(NativeDecodedAudio {
        data,
        repeat_data,
        channels: header.channels,
        extent: Some(extent),
        restart_rewinds: false,
    })
}

/// 40C130 ->40BD00 ->40BDD0. Keep the physical native scratch layout:
/// the ten-bit coefficient count can reach past the 320 cleared spectral
/// entries into previously initialized scratch/history. Those writes are
/// deterministic and must not become a generic codec or a zero-filled tail.
fn unpack(
    source: &[u8],
    header: &Header,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<(StaticSoundData, Option<StaticSoundData>), CoreError> {
    let Header {
        channels,
        rate,
        declared_frames,
        blocks,
        shift,
    } = *header;
    if !matches!(channels, 1 | 2) || rate == 0 {
        return Err(boundary(
            "native MV2 metadata does not describe playable mono/stereo PCM",
        ));
    }
    let channels = usize::from(channels);
    //40C0D0 clears975..3662 and the two history cursors. The remaining
    //scratch entries read below are overwritten before each use.
    let mut state = [0i32; 3665];
    let mut bits = Bits {
        source,
        cursor: 18,
        word: 0,
        remaining: 0,
    };
    let mut pcm = [0i16; 640];
    let mut frames = Vec::new();
    let output_shift = 6u32.wrapping_sub(u32::from(shift)) & 31;
    //40C300 clears the same histories but forces shift4. Scaling is after
    //synthesis and never feeds back, so recover both passes in one decode.
    let mut repeat_pcm = if output_shift != 2 {
        Some([0i16; 640])
    } else {
        None
    };
    let mut repeat_frames = (output_shift != 2).then(Vec::new);
    for block in 0..blocks {
        if cancelled.load(Ordering::Acquire) {
            return Err(boundary("native MV2 decode cancelled"));
        }
        for channel in 0..channels {
            let count = bits.read(10)? as usize;
            let scale = QUANTIZER[bits.read(9)? as usize];
            state[655..975].fill(0);
            let mut index = 0usize;
            while index < count {
                let width = bits.count()?;
                if width != 0 {
                    let mut value = bits.read(width)?;
                    if value < 1i32.wrapping_shl(width.wrapping_sub(1)) {
                        value = value.wrapping_sub(1i32.wrapping_shl(width)).wrapping_add(1);
                    }
                    state[655 + index] = scale.wrapping_mul(value);
                    index += 1;
                } else {
                    index += bits.read(3)? as usize + 1;
                }
            }
            for part in 0..10 {
                let cursor = state[3663 + channel] as usize;
                let history = 1615 + 1024 * channel;
                for output in 0..64 {
                    let mut sample = 0i32;
                    for input in 0..32 {
                        let product = state[655 + part * 32 + input]
                            .wrapping_mul(i32::from(SYNTHESIS[output * 32 + input]));
                        sample = sample.wrapping_add(product >> 10);
                    }
                    state[history + cursor + output] = sample;
                }
                for output in 0..32 {
                    let mut position = cursor + output;
                    let mut value = 0i32;
                    for coefficient in 0..16 {
                        let term = state[history + (position & 1023)]
                            .wrapping_mul(i32::from(WINDOW[output + coefficient * 32]))
                            >> 10;
                        value = if coefficient & 2 == 0 {
                            value.wrapping_add(term)
                        } else {
                            value.wrapping_sub(term)
                        };
                        position += if coefficient & 1 == 0 { 96 } else { 32 };
                    }
                    state[335 + part * 32 + output] = value;
                }
                state[3663 + channel] = cursor.wrapping_sub(64) as i32 & 1023;
            }
            for sample in 0..320 {
                let value = state[335 + sample] >> output_shift;
                pcm[sample * channels + channel] = value.clamp(-32767, 32767) as i16;
                if let Some(repeat) = repeat_pcm.as_mut() {
                    repeat[sample * channels + channel] =
                        (state[335 + sample] >> 2).clamp(-32767, 32767) as i16;
                }
            }
        }
        let returned_bytes = 2u32
            .wrapping_mul(channels as u32)
            .wrapping_mul(declared_frames.wrapping_sub(320u32.wrapping_mul(block)))
            .min(640 * channels as u32);
        let returned_frames = returned_bytes as usize / (2 * channels);
        if frames
            .len()
            .checked_add(returned_frames)
            .is_none_or(|len| len > max_frames)
        {
            return Err(boundary("native MV2 PCM exceeds the frame limit"));
        }
        for sample in pcm[..returned_frames * channels].chunks_exact(channels) {
            frames.push(Frame::new(
                f32::from(sample[0]) * (1.0 / 32768.0),
                f32::from(sample[channels - 1]) * (1.0 / 32768.0),
            ));
        }
        if let (Some(frames), Some(pcm)) = (repeat_frames.as_mut(), repeat_pcm.as_ref()) {
            for sample in pcm[..returned_frames * channels].chunks_exact(channels) {
                frames.push(Frame::new(
                    f32::from(sample[0]) * (1.0 / 32768.0),
                    f32::from(sample[channels - 1]) * (1.0 / 32768.0),
                ));
            }
        }
    }
    if frames.is_empty() {
        return Err(boundary("native MV2 consumer produces no initialized PCM"));
    }
    let data = StaticSoundData {
        sample_rate: rate,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    };
    let repeat = repeat_frames.map(|frames| StaticSoundData {
        sample_rate: rate,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    });
    Ok((data, repeat))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(channels: u8, declared_frames: u32, blocks: u32, impulse: bool) -> Vec<u8> {
        let mut source = b"MV2X".to_vec();
        source.extend_from_slice(&declared_frames.to_le_bytes());
        source.extend_from_slice(&0u16.to_le_bytes());
        source.extend_from_slice(&44100u16.to_le_bytes());
        source.extend_from_slice(&[channels, 4]);
        source.extend_from_slice(&blocks.to_le_bytes());
        let mut bits = Vec::new();
        let mut push = |value: u32, width: usize| {
            for bit in (0..width).rev() {
                bits.push((value >> bit) & 1);
            }
        };
        for block in 0..blocks {
            for channel in 0..channels {
                let active = impulse && block == 0;
                push(u32::from(active), 10);
                push(200, 9);
                if active {
                    push(2, 2); // unary width1: one, zero
                    push(u32::from(channel == 0), 1); // positive left, negative right
                }
            }
        }
        for chunk in bits.chunks(32) {
            let word = chunk
                .iter()
                .enumerate()
                .fold(0u32, |word, (bit, value)| word | (value << bit));
            source.extend_from_slice(&word.to_le_bytes());
        }
        source
    }

    #[test]
    fn mv2x_decodes_actual_subband_pcm_and_native_extent_with_independent_stereo_history() {
        let source = fixture(2, 600, 2, true);
        let decoded = decode(&source, 640, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.extent, Some(2400));
        let pcm = decoded.data.unwrap();
        assert_eq!(pcm.sample_rate, 44100);
        assert_eq!(pcm.frames.len(), 600);
        assert!(pcm.frames.iter().any(|frame| frame.left != 0.0));
        assert!(
            pcm.frames
                .iter()
                .any(|frame| frame.right != 0.0 && frame.right != frame.left)
        );
        assert!(pcm.frames[320..].iter().any(|frame| frame.left != 0.0));
    }

    #[test]
    fn valid_silent_wire_is_decoded_but_truncated_payload_cannot_become_synthetic_silence() {
        let source = fixture(1, 320, 1, false);
        let decoded = decode(&source, 320, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.extent, Some(640));
        assert!(
            decoded
                .data
                .unwrap()
                .frames
                .iter()
                .all(|frame| *frame == Frame::new(0.0, 0.0))
        );
        let decoded = decode(&source[..18], 320, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.extent, Some(640));
        assert_eq!(
            decoded.data.err().unwrap().code(),
            "SHINKI_EMU_CMVS_NATIVE_MV2_BOUNDARY"
        );
        assert!(decode(b"MV2Z................", 320, &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn non_default_header_shift_keeps_first_pass_and_native_shift_four_repeat_distinct() {
        let source = fixture(2, 600, 2, true);
        let expected = decode(&source, 640, &AtomicBool::new(false))
            .unwrap()
            .data
            .unwrap();
        let mut changed = source.clone();
        changed[13] = 3;
        let decoded = decode(&changed, 640, &AtomicBool::new(false)).unwrap();
        let repeat = decoded.repeat_data.unwrap();
        assert_eq!(repeat.frames, expected.frames);
        assert_ne!(decoded.data.unwrap().frames, repeat.frames);
        changed[13] = 36; // x86 arithmetic shifts mask their count to five bits.
        let decoded = decode(&changed, 640, &AtomicBool::new(false)).unwrap();
        assert!(decoded.repeat_data.is_none());
        assert_eq!(decoded.data.unwrap().frames, expected.frames);
    }
}
