use super::{NativeDecodedAudio, pcm_s16};
use kira::{
    Frame,
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
};
use shinki_emu_sdk::CoreError;
use std::sync::atomic::{AtomicBool, Ordering};

fn boundary(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_PCM_BOUNDARY", message)
}
fn word(bytes: &[u8], offset: usize) -> Result<u16, CoreError> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or_else(|| boundary("native WAV format reads uninitialized bytes"))?
            .try_into()
            .unwrap(),
    ))
}
fn dword(bytes: &[u8], offset: usize) -> Result<u32, CoreError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| boundary("native RIFF parser reads outside the asset"))?
            .try_into()
            .unwrap(),
    ))
}

/// 490A20 stops as soon as both fmt and nonempty data have been seen. The
/// decoder's input end is the physical asset end, not the RIFF/data extent.
pub(super) fn decode(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    if source.get(..4) != Some(b"RIFF") || source.get(8..12) != Some(b"WAVE") {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_AUDIO_CODEC",
            "filename selects CMusicPCM, which requires RIFF/WAVE",
        ));
    }
    let end = dword(source, 4)?
        .checked_add(8)
        .ok_or_else(|| boundary("native RIFF size wraps the input address"))?
        as usize;
    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    let mut fact = 0u32;
    while cursor < end {
        let header = source
            .get(cursor..cursor + 8)
            .ok_or_else(|| boundary("native RIFF chunk header exceeds the asset"))?;
        let size = dword(header, 4)? as usize;
        let start = cursor + 8;
        match &header[..4] {
            b"fmt " => {
                if format.is_some() {
                    break;
                }
                if !(14..=1024).contains(&size) {
                    return Err(boundary("native WAV format buffer is not fully defined"));
                }
                format = Some(
                    source
                        .get(start..start + size)
                        .ok_or_else(|| boundary("native WAV fmt copy exceeds the asset"))?,
                );
            }
            b"data" if data.is_none() && size != 0 => {
                data = Some(start);
            }
            b"fact" => {
                fact = dword(source, start)?;
            }
            _ => {}
        }
        if format.is_some() && data.is_some() {
            break;
        }
        cursor = start
            .checked_add(size)
            .and_then(|n| n.checked_add(size & 1))
            .ok_or_else(|| boundary("native RIFF chunk address wraps"))?;
    }
    let format = format.ok_or_else(|| boundary("native WAV has no defined format"))?;
    let start = data
        .filter(|start| *start < end && *start <= source.len())
        .ok_or_else(|| boundary("native WAV has no defined data pointer"))?;
    let channels = word(format, 2)?;
    let rate = dword(format, 4)?;
    let pcm = if word(format, 0)? == 1 {
        // CMusicPCM advertises signed 16-bit output and copies these bytes;
        // it never dispatches on wBitsPerSample or converts float/8-bit PCM.
        pcm_s16(&source[start..], channels, rate, max_frames, cancelled)
    } else {
        decode_blocks(
            &source[start..],
            format,
            channels,
            rate,
            fact.wrapping_mul(4),
            max_frames,
            cancelled,
        )
    };
    Ok(NativeDecodedAudio {
        data: pcm,
        channels,
        extent: Some(source.len() as u32),
        repeat_data: None,
        restart_rewinds: false,
    })
}

/// Every non-one WAV tag selects 490730, including 3, 0x55, and 0x161.
fn decode_blocks(
    source: &[u8],
    format: &[u8],
    channels: u16,
    rate: u32,
    limit: u32,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    if !matches!(channels, 1 | 2) || rate == 0 {
        return Err(boundary(
            "native WAV output is not playable mono/stereo PCM",
        ));
    }
    let channels = usize::from(channels);
    let align = usize::from(word(format, 12)?);
    let samples = usize::from(word(format, 18)?);
    //490730 emits both seed frames even for a samples-per-block word<2.
    let count = samples.max(2) * channels;
    if align < 7 * channels || count > 0x24000 {
        return Err(boundary(
            "native ADPCM block accesses outside initialized scratch",
        ));
    }
    let nibble_bytes = (count - 2 * channels).div_ceil(2);
    if 7 * channels + nibble_bytes > align {
        return Err(boundary("native ADPCM nibbles exceed its block allocation"));
    }
    let mut block = vec![0u8; align];
    let mut decoded = vec![0i16; count + 1];
    let mut frames = Vec::with_capacity((limit as usize / (2 * channels)).min(max_frames));
    let mut consumed = 0u32;
    for encoded in source.chunks(align) {
        if cancelled.load(Ordering::Acquire) {
            return Err(boundary("native PCM decode cancelled"));
        }
        block[..encoded.len()].copy_from_slice(encoded);
        block[encoded.len()..].fill(0); //49079E explicitly pads short final blocks.
        let mut delta = [0i32; 2];
        let mut coefficients = [(0i32, 0i32); 2];
        for channel in 0..channels {
            let predictor = usize::from(block[channel]);
            coefficients[channel] = (
                word(format, 22 + 4 * predictor)? as i16 as i32,
                word(format, 24 + 4 * predictor)? as i16 as i32,
            );
            delta[channel] = word(&block, channels + 2 * channel)? as i16 as i32;
            decoded[channel] = word(&block, 5 * channels + 2 * channel)? as i16;
            decoded[channels + channel] = word(&block, 3 * channels + 2 * channel)? as i16;
        }
        const ADAPT: [i32; 16] = [
            230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
        ];
        for index in 2 * channels..count {
            let nibble = index - 2 * channels;
            let packed = block[7 * channels + nibble / 2];
            let value = if nibble & 1 == 0 {
                packed >> 4
            } else {
                packed & 15
            };
            let channel = index % channels;
            let previous_delta = delta[channel];
            delta[channel] = (previous_delta.wrapping_mul(ADAPT[usize::from(value)]) >> 8).max(16);
            let (a, b) = coefficients[channel];
            let prediction = (i32::from(decoded[index - channels])
                .wrapping_mul(a)
                .wrapping_add(i32::from(decoded[index - 2 * channels]).wrapping_mul(b)))
                >> 8;
            let signed = if value >= 8 {
                i32::from(value) - 16
            } else {
                i32::from(value)
            };
            decoded[index] = prediction
                .wrapping_add(previous_delta.wrapping_mul(signed))
                .clamp(-32768, 32767) as i16;
        }
        let bytes = (2 * count) as u32;
        let returned = if limit < consumed.wrapping_add(bytes) {
            limit.wrapping_sub(consumed)
        } else {
            bytes
        };
        if returned == 0 {
            break;
        }
        if !(returned as usize).is_multiple_of(2 * channels) {
            return Err(boundary("native ADPCM returns a partial output frame"));
        }
        let returned_frames = returned as usize / (2 * channels);
        if frames
            .len()
            .checked_add(returned_frames)
            .is_none_or(|len| len > max_frames)
        {
            return Err(boundary("native ADPCM output exceeds the frame limit"));
        }
        for frame in decoded[..returned as usize / 2].chunks_exact(channels) {
            let left = f32::from(frame[0]) * (1.0 / 32768.0);
            let right = f32::from(frame[channels - 1]) * (1.0 / 32768.0);
            frames.push(Frame::new(left, right));
        }
        consumed = consumed.wrapping_add(returned);
    }
    if frames.is_empty() {
        return Err(boundary(
            "native ADPCM consumer produces no initialized PCM",
        ));
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

    fn wave(format: &[u8], fact: Option<u32>, data: &[u8]) -> Vec<u8> {
        let mut source = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        source.extend_from_slice(&(format.len() as u32).to_le_bytes());
        source.extend_from_slice(format);
        if let Some(fact) = fact {
            source.extend_from_slice(b"fact\x04\0\0\0");
            source.extend_from_slice(&fact.to_le_bytes());
        }
        source.extend_from_slice(b"data");
        source.extend_from_slice(&(data.len() as u32).to_le_bytes());
        source.extend_from_slice(data);
        let len = source.len() as u32 - 8;
        source[4..8].copy_from_slice(&len.to_le_bytes());
        source
    }

    #[test]
    fn pcm_tag_one_copies_signed16_even_when_format_claims_eight_bit() {
        let mut format = [0u8; 16];
        format[..2].copy_from_slice(&1u16.to_le_bytes());
        format[2..4].copy_from_slice(&1u16.to_le_bytes());
        format[4..8].copy_from_slice(&44100u32.to_le_bytes());
        format[14..16].copy_from_slice(&8u16.to_le_bytes());
        let source = wave(&format, None, &16384i16.to_le_bytes());
        let decoded = decode(&source, 32, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.extent, Some(source.len() as u32));
        let data = decoded.data.unwrap();
        assert_eq!(data.frames.len(), 1);
        assert_eq!(data.frames[0], Frame::new(0.5, 0.5));
    }

    #[test]
    fn every_non_one_tag_consumes_native_adpcm_not_its_generic_codec() {
        for tag in [2u16, 3, 0x55, 0x161, 0xfffe] {
            let mut format = [0u8; 26];
            format[..2].copy_from_slice(&tag.to_le_bytes());
            format[2..4].copy_from_slice(&1u16.to_le_bytes());
            format[4..8].copy_from_slice(&22050u32.to_le_bytes());
            format[12..14].copy_from_slice(&10u16.to_le_bytes());
            format[18..20].copy_from_slice(&8u16.to_le_bytes());
            format[20..22].copy_from_slice(&1u16.to_le_bytes());
            format[22..24].copy_from_slice(&256u16.to_le_bytes());
            let block = [0, 16, 0, 0, 64, 0, 64, 0, 0, 0];
            let source = wave(&format, Some(4), &block);
            let decoded = decode(&source, 32, &AtomicBool::new(false)).unwrap();
            assert_eq!(decoded.extent, Some(source.len() as u32));
            let data = decoded.data.unwrap();
            assert_eq!(data.sample_rate, 22050);
            assert_eq!(data.frames.len(), 8);
            assert!(
                data.frames
                    .iter()
                    .all(|sample| *sample == Frame::new(0.5, 0.5))
            );
        }
    }

    #[test]
    fn malformed_non_pcm_keeps_physical_extent_but_has_no_generic_float_success() {
        let mut format = [0u8; 16];
        format[..2].copy_from_slice(&3u16.to_le_bytes());
        format[2..4].copy_from_slice(&1u16.to_le_bytes());
        format[4..8].copy_from_slice(&44100u32.to_le_bytes());
        format[8..12].copy_from_slice(&(44100u32 * 4).to_le_bytes());
        format[12..14].copy_from_slice(&4u16.to_le_bytes());
        format[14..16].copy_from_slice(&32u16.to_le_bytes());
        let source = wave(&format, None, &0.5f32.to_le_bytes());
        let decoded = decode(&source, 32, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.extent, Some(source.len() as u32));
        assert_eq!(
            decoded.data.err().unwrap().code(),
            "SHINKI_EMU_CMVS_NATIVE_PCM_BOUNDARY"
        );
    }
}
