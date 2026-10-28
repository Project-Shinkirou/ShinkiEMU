//! Native MP3 consumer. CMVS hands raw Layer III data to the Windows MPEG
//! Layer-3 ACM codec; this module reproduces that consumer's observable
//! contract with a pure-Rust MPEG decoder on every platform:
//! - `acmStreamSize` defines the extent from the format block alone;
//! - each `acmStreamConvert` tops up an 8192-byte codec input buffer and
//!   returns at most one 32 KiB destination buffer of whole frames;
//! - a frame decodes only once the following 4-byte header is buffered;
//! - the caller never sends `ACM_STREAMCONVERTF_END`, so frames still in the
//!   codec buffer when the source is exhausted are never emitted.
use super::{NativeDecodedAudio, append_pcm_s16};
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use shinki_emu_sdk::CoreError;
use std::sync::atomic::AtomicBool;
use symphonia::core::{
    codecs::audio::{
        AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_MP3,
    },
    packet::Packet,
};

/// Input bytes the codec buffers between conversions.
const CODEC_INPUT_BYTES: usize = 8192;
/// 48FAA0's fixed destination buffer.
const CONVERT_OUTPUT_BYTES: usize = 0x8000;

fn boundary(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_MP3_BOUNDARY", message)
}

struct Request<'a> {
    format: [u8; 30],
    input: u32,
    payload: &'a [u8],
}

/// 48F850/48FC10/48FE70: one raw MPEG-1/2 Layer III header, no scanning,
/// RIFF unwrapping, footer search, Xing duration, or LAME delay adjustment.
fn request(source: &[u8]) -> Result<Request<'_>, CoreError> {
    let invalid = || {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_MP3_HEADER",
            "native MP3 header fields are not fully defined",
        )
    };
    let mut end = source.len();
    if end >= 128 && source.get(end - 128..end - 125) == Some(b"TAG") {
        end -= 128;
    }
    let mut start = 0usize;
    if source.starts_with(b"ID3") {
        let tag = source.get(..10).ok_or_else(invalid)?;
        start = 10
            + ((usize::from(tag[6]) & 127) << 21)
            + ((usize::from(tag[7]) & 127) << 14)
            + ((usize::from(tag[8]) & 127) << 7)
            + (usize::from(tag[9]) & 127);
    }
    let header = u32::from_be_bytes(
        source
            .get(start..start + 4)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    );
    if header >> 20 != 0xfff || (header >> 17) & 3 != 1 {
        return Err(invalid());
    }
    let index = ((header >> 12) & 15) as usize;
    let rate_index = ((header >> 10) & 3) as usize;
    // Indices0/15 and reserved rate3 read adjacent/uninitialized native
    // table state; they are not evidence of defined free-format support.
    if index == 0 || index == 15 || rate_index == 3 {
        return Err(invalid());
    }
    let mpeg1 = header & (1 << 19) != 0;
    let rates = if mpeg1 {
        [44100u32, 48000, 32000]
    } else {
        [22050u32, 24000, 16000]
    };
    let bitrates = if mpeg1 {
        [
            0u32, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ]
    } else {
        [
            0u32, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160,
        ]
    };
    let rate = rates[rate_index];
    let kbps = bitrates[index];
    let channels = if (header >> 6) & 3 == 3 { 1u16 } else { 2u16 };
    let mut format = [0u8; 30];
    format[..2].copy_from_slice(&0x55u16.to_le_bytes());
    format[2..4].copy_from_slice(&channels.to_le_bytes());
    format[4..8].copy_from_slice(&rate.to_le_bytes());
    format[8..12].copy_from_slice(&(kbps * 125).to_le_bytes());
    format[12..14].copy_from_slice(&1u16.to_le_bytes());
    format[16..18].copy_from_slice(&12u16.to_le_bytes());
    format[18..20].copy_from_slice(&1u16.to_le_bytes());
    format[20..24].copy_from_slice(&2u32.to_le_bytes());
    format[24..26].copy_from_slice(&((144000 * kbps / rate) as u16).to_le_bytes());
    format[26..28].copy_from_slice(&1u16.to_le_bytes());
    format[28..30].copy_from_slice(&1393u16.to_le_bytes());
    Ok(Request {
        format,
        input: (end as u32).wrapping_sub(start as u32),
        payload: source.get(start..end).unwrap_or(&[]),
    })
}

pub(super) fn decode(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    let request = request(source)?;
    let channels = u16::from_le_bytes([request.format[2], request.format[3]]);
    Ok(NativeDecodedAudio {
        data: convert(&request, channels, max_frames, cancelled),
        channels,
        extent: Some(stream_size(&request)),
        repeat_data: None,
        restart_rewinds: false,
    })
}

fn format_rate(request: &Request<'_>) -> u32 {
    u32::from_le_bytes([
        request.format[4],
        request.format[5],
        request.format[6],
        request.format[7],
    ])
}

/// `acmStreamSize(source)`: whole frames of `nBlockSize * spf / 1152` input
/// bytes, rounded up, times one frame of 16-bit PCM. The frame count is exact;
/// the byte product is a DWORD and wraps.
fn stream_size(request: &Request<'_>) -> u32 {
    let channels = u32::from(u16::from_le_bytes([request.format[2], request.format[3]]));
    let block = u64::from(u16::from_le_bytes([request.format[24], request.format[25]]));
    let spf = samples_per_frame(format_rate(request));
    let unit = block * u64::from(spf) / 1152;
    if unit == 0 {
        return 0;
    }
    let frames = u64::from(request.input).div_ceil(unit);
    (frames as u32)
        .wrapping_mul(spf)
        .wrapping_mul(channels)
        .wrapping_mul(2)
}

fn samples_per_frame(rate: u32) -> u32 {
    if rate >= 32000 { 1152 } else { 576 }
}

/// Length of the Layer III frame starting with `header`, if it is one the
/// codec decodes (MPEG-1/2, a defined bitrate and sample rate).
fn frame_length(header: [u8; 4]) -> Option<usize> {
    let header = u32::from_be_bytes(header);
    if header >> 21 != 0x7ff || (header >> 19) & 3 < 2 || (header >> 17) & 3 != 1 {
        return None;
    }
    let mpeg1 = header & (1 << 19) != 0;
    let index = ((header >> 12) & 15) as usize;
    let rate_index = ((header >> 10) & 3) as usize;
    if index == 0 || index == 15 || rate_index == 3 {
        return None;
    }
    let (rate, kbps, factor) = if mpeg1 {
        (
            [44100, 48000, 32000][rate_index],
            [
                0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
            ][index],
            144_000,
        )
    } else {
        (
            [22050, 24000, 16000][rate_index],
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160][index],
            72_000,
        )
    };
    Some(factor * kbps / rate + ((header >> 9) & 1) as usize)
}

/// 48FAA0's conversion loop over the codec's buffered input.
fn convert(
    request: &Request<'_>,
    channels: u16,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    let length = request.input as usize;
    if length > request.payload.len() {
        return Err(boundary("native MP3 extent exceeds the accessible source"));
    }
    let rate = format_rate(request);
    let frame_samples = samples_per_frame(rate) as usize * usize::from(channels);
    let frames_per_call = CONVERT_OUTPUT_BYTES / (frame_samples * 2);
    let mut params = AudioCodecParameters::new();
    params.for_codec(CODEC_ID_MP3);
    let mut decoder =
        symphonia::default::codecs::MpaDecoder::try_new(&params, &AudioDecoderOptions::default())
            .map_err(|_| boundary("native MP3 decoder is unavailable"))?;
    let mut buffered: Vec<u8> = Vec::with_capacity(CODEC_INPUT_BYTES);
    let mut position = 0;
    let mut frames = Vec::new();
    let mut samples: Vec<i16> = Vec::with_capacity(frame_samples);
    let mut pcm = Vec::with_capacity(CONVERT_OUTPUT_BYTES);
    while position < length {
        let take = (CODEC_INPUT_BYTES - buffered.len()).min(length - position);
        buffered.extend_from_slice(&request.payload[position..position + take]);
        position += take;
        pcm.clear();
        for _ in 0..frames_per_call {
            // The codec resynchronizes on the next decodable header.
            let Some((start, size)) = (0..buffered.len().saturating_sub(3)).find_map(|at| {
                let header = [
                    buffered[at],
                    buffered[at + 1],
                    buffered[at + 2],
                    buffered[at + 3],
                ];
                frame_length(header).map(|size| (at, size))
            }) else {
                let keep = buffered.len().saturating_sub(3);
                buffered.drain(..keep);
                break;
            };
            if start + size + 4 > buffered.len() {
                buffered.drain(..start);
                break;
            }
            let packet = Packet::new(
                0,
                0.into(),
                (frame_samples as u64).into(),
                &buffered[start..start + size],
            );
            samples.clear();
            match decoder.decode(&packet) {
                Ok(decoded) if decoded.frames() * usize::from(channels) == frame_samples => {
                    decoded.copy_to_vec_interleaved(&mut samples);
                }
                // A frame the decoder cannot reconstruct still occupies its
                // place in the PCM stream.
                _ => samples.resize(frame_samples, 0),
            }
            pcm.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
            buffered.drain(..start + size);
        }
        if pcm.is_empty() {
            break;
        }
        append_pcm_s16(&pcm, channels, &mut frames, max_frames, cancelled)?;
    }
    if frames.is_empty() {
        return Err(boundary("native MP3 consumer produced no PCM"));
    }
    Ok(StaticSoundData {
        sample_rate: rate,
        frames: frames.into(),
        settings: StaticSoundSettings::default(),
        slice: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp3_format_is_the_exact_native_thirty_byte_wire() {
        let source = [0xff, 0xfb, 0x90, 0xc0, 1, 2, 3, 4];
        let request = request(&source).unwrap();
        let expected = [
            0x55, 0, 1, 0, 0x44, 0xac, 0, 0, 0x80, 0x3e, 0, 0, 1, 0, 0, 0, 12, 0, 1, 0, 2, 0, 0, 0,
            0xa1, 1, 1, 0, 0x71, 5,
        ];
        assert_eq!(request.format, expected);
        assert_eq!(request.input, 8);
        assert_eq!(request.payload, source);
    }

    #[test]
    fn tag_trim_masks_synchsafe_bytes_and_does_not_scan_or_add_a_footer() {
        let mut source = vec![0; 14];
        source[..3].copy_from_slice(b"ID3");
        source[5] = 0x10; // Native still does not add a ten-byte footer.
        source[9] = 0x84;
        source.extend_from_slice(&[0xff, 0xfb, 0x90, 0xc0, 5, 6, 7, 8]);
        source.extend_from_slice(b"TAG");
        source.extend_from_slice(&[0; 125]);
        let decoded = request(&source).unwrap();
        assert_eq!(decoded.input, 8);
        assert_eq!(decoded.payload, &source[14..22]);
        assert!(request(b"RIFF\xff\xfb\x90\xc0WAVE").is_err());
        assert!(request(b"junk\xff\xfb\x90\xc0").is_err());
        assert!(request(b"ID3").is_err());
        assert!(request(&[0xff, 0xe3, 0x90, 0]).is_err()); // MPEG2.5 does not pass twelve ones.
    }

    #[test]
    fn encoded_extent_subtraction_wraps_without_exposing_inaccessible_pcm() {
        let mut source = vec![0; 160];
        source[..3].copy_from_slice(b"ID3");
        source[9] = 54; // Header starts64, while TAG trimming sets end32.
        source[32..35].copy_from_slice(b"TAG");
        source[64..68].copy_from_slice(&[0xff, 0xfb, 0x90, 0xc0]);
        let decoded = request(&source).unwrap();
        assert_eq!(decoded.input, 0xffff_ffe0);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn mpeg_two_uses_native_bitrate_rate_and_unhalved_blocksize_formula() {
        let decoded = request(&[0xff, 0xf3, 0x80, 0]).unwrap();
        assert_eq!(
            u32::from_le_bytes(decoded.format[4..8].try_into().unwrap()),
            22050
        );
        assert_eq!(
            u32::from_le_bytes(decoded.format[8..12].try_into().unwrap()),
            8000
        );
        assert_eq!(
            u16::from_le_bytes(decoded.format[24..26].try_into().unwrap()),
            417
        );
    }

    #[test]
    fn stream_size_counts_rounded_blocks_and_wraps_like_the_acm_dword() {
        // Values recorded from the Windows MPEG Layer-3 ACM codec.
        let mut mono = request(&[0xff, 0xfb, 0x50, 0xc0]).unwrap();
        let mut stereo = request(&[0xff, 0xf3, 0x84, 0x00]).unwrap();
        let mut odd_block = request(&[0xff, 0xf3, 0x80, 0x00]).unwrap();
        for (input, expected) in [
            (0, [0, 0, 0]),
            (1, [2304, 2304, 2304]),
            (0xd1, [4608, 4608, 4608]),
            (0x1_0000, [728_064, 787_968, 728_064]),
            (0x7fff_ffff, [2_312_675_584, 768, 2_312_675_584]),
            (0xffff_ffe0, [330_383_872, 1536, 330_383_872]),
        ] {
            mono.input = input;
            stereo.input = input;
            odd_block.input = input;
            assert_eq!(
                [
                    stream_size(&mono),
                    stream_size(&stereo),
                    stream_size(&odd_block)
                ],
                expected,
                "input {input:#x}"
            );
        }
    }

    #[test]
    fn consumer_drops_frames_still_buffered_when_the_source_is_exhausted() {
        // 67 frames of a 44.1 kHz mono 64 kbps LAME sine, no ID3 or Xing.
        let source = include_bytes!("fixtures/sine44m64.mp3");
        let decoded = decode(source, usize::MAX, &AtomicBool::new(false)).unwrap();
        // The ACM codec returned three 14-frame conversions for this file:
        // the third buffers the rest of the source, so 25 frames never decode.
        assert_eq!(decoded.extent, Some(156_672));
        assert_eq!(decoded.channels, 1);
        let data = decoded.data.unwrap();
        assert_eq!(data.sample_rate, 44100);
        assert_eq!(data.frames.len(), 3 * 14 * 1152);
        // One frame past the decoder delay carries the sine, not silence.
        let window = &data.frames[20_000..20_000 + 1152];
        let rms = (window.iter().map(|f| f.left * f.left).sum::<f32>() / 1152.0).sqrt();
        assert!(rms > 0.05, "rms {rms}");
    }

    #[test]
    fn a_frame_decodes_only_after_the_next_header_is_buffered() {
        let source = include_bytes!("fixtures/sine44m64.mp3");
        let first = frame_length([source[0], source[1], source[2], source[3]]).unwrap();
        let decoded = decode(&source[..first + 3], usize::MAX, &AtomicBool::new(false)).unwrap();
        // 211 input bytes span two 208-byte blocks.
        assert_eq!(decoded.extent, Some(4608));
        assert!(decoded.data.is_err());
        let decoded = decode(&source[..first + 4], usize::MAX, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.data.unwrap().frames.len(), 1152);
    }

    #[test]
    fn extent_stays_defined_when_the_encoded_length_is_inaccessible() {
        let mut query = request(&[0xff, 0xfb, 0x90, 0xc0]).unwrap();
        query.input = 0;
        assert_eq!(stream_size(&query), 0);
        let mut source = vec![0; 160];
        source[..3].copy_from_slice(b"ID3");
        source[9] = 54;
        source[32..35].copy_from_slice(b"TAG");
        source[64..68].copy_from_slice(&[0xff, 0xfb, 0x90, 0xc0]);
        let decoded = decode(&source, usize::MAX, &AtomicBool::new(false)).unwrap();
        assert!(decoded.extent.is_some());
        assert!(decoded.data.is_err());
    }
}
