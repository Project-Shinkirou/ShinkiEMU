//! XPCM transform audio, using the Circus format documented by GARbro.
use crate::{Error, Result};
use flate2::read::ZlibDecoder;
use rustfft::{FftPlanner, num_complex::Complex32};
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};

const TRANSFORM: usize = 4096;
const OVERLAP: usize = 32;
const HOP: usize = TRANSFORM - OVERLAP;
const MAX_PCM_BYTES: usize = 64 * 1024 * 1024;

const PREFIX_GAINS: [[f32; 8]; 4] = [
    [2048., 2048., 4096., 4096., 8192., 8192., 16384., 16384.],
    [2048., 2048., 2048., 4096., 4096., 4096., 8192., 16384.],
    [2048., 2048., 2048., 2048., 2048., 4096., 4096., 8192.],
    [1024., 1024., 1024., 2048., 2048., 2048., 2048., 2048.],
];

pub(crate) fn decode_transform(
    data: &[u8],
    mode: u32,
    gain_preset: usize,
    decoded_bytes: usize,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    if decoded_bytes == 0 || decoded_bytes & 1 != 0 || decoded_bytes > MAX_PCM_BYTES {
        return Err(Error::Invalid("XPCM transform size"));
    }
    let gains = PREFIX_GAINS
        .get(gain_preset)
        .ok_or(Error::Invalid("XPCM gain preset"))?;
    let sample_count = decoded_bytes / 2;
    let encoded_bytes = sample_count.div_ceil(HOP) * TRANSFORM * 2;
    let encoded = match mode {
        1 => unpack_lz(data, encoded_bytes)?,
        3 => {
            let mut encoded = vec![0; encoded_bytes];
            ZlibDecoder::new(data).read_exact(&mut encoded)?;
            encoded
        }
        _ => return Err(Error::Unsupported("XPCM transform compression")),
    };

    let fft = FftPlanner::new().plan_fft_forward(TRANSFORM);
    let mut spectrum = vec![Complex32::default(); TRANSFORM];
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let mut pcm = vec![0i16; sample_count];
    for (block_index, block) in encoded.chunks_exact(TRANSFORM * 2).enumerate() {
        if stop.load(Ordering::Acquire) {
            return Err(Error::Audio("decode cancelled".into()));
        }
        let (high, low) = block.split_at(TRANSFORM);
        for (bin, complex) in spectrum[..TRANSFORM / 2].iter_mut().enumerate() {
            let first = low[bin];
            let second = low[bin + TRANSFORM / 2];
            let re = u16::from_le_bytes([(first & 0xf0) | (second >> 4), high[2 * bin]]);
            let im = u16::from_le_bytes([(first << 4) | (second & 0xf), high[2 * bin + 1]]);
            let gain = gains.get(bin / 256).copied().unwrap_or(1.0);
            *complex = Complex32::new(unfold(re) * gain, unfold(im) * gain);
        }
        spectrum[TRANSFORM / 2..].fill(Complex32::default());
        fft.process_with_scratch(&mut spectrum, &mut scratch);
        for (index, sample) in pcm[block_index * HOP..]
            .iter_mut()
            .take(TRANSFORM)
            .enumerate()
        {
            let value = spectrum[index].re / 1024.0;
            let value = if block_index != 0 && index < OVERLAP {
                (value * index as f32 + f32::from(*sample) * (OVERLAP - index) as f32)
                    / OVERLAP as f32
            } else {
                value
            };
            *sample = value
                .trunc()
                .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        }
    }
    let mut bytes = Vec::with_capacity(decoded_bytes);
    for sample in pcm {
        bytes.extend(sample.to_le_bytes());
    }
    Ok(bytes)
}

fn unfold(value: u16) -> f32 {
    let step = i32::from(value / 2);
    if value & 1 == 0 {
        -step as f32
    } else {
        f32::from((step + 1) as i16)
    }
}

fn byte(reader: &mut impl Read) -> Result<u8> {
    let mut out = [0];
    reader.read_exact(&mut out)?;
    Ok(out[0])
}

fn word(reader: &mut impl Read) -> Result<usize> {
    let mut out = [0; 2];
    reader.read_exact(&mut out)?;
    Ok(usize::from(u16::from_le_bytes(out)))
}

fn unpack_lz(data: &[u8], length: usize) -> Result<Vec<u8>> {
    let mut source = Cursor::new(data);
    let mut out = Vec::with_capacity(length);
    while out.len() < length {
        let flags = byte(&mut source)?;
        for bit in 0..8 {
            if out.len() == length {
                break;
            }
            if flags & (1 << bit) != 0 {
                out.push(byte(&mut source)?);
                continue;
            }
            let code = usize::from(byte(&mut source)?);
            let (count, distance) = match code {
                0xc0.. => (
                    4 + ((code >> 2) & 0xf),
                    ((code & 3) << 8) | usize::from(byte(&mut source)?),
                ),
                0x80.. => {
                    let distance = code & 31;
                    let distance = if distance == 0 {
                        usize::from(byte(&mut source)?)
                    } else {
                        distance
                    };
                    (2 + ((code >> 5) & 3), distance)
                }
                0x7f => (2 + word(&mut source)?, word(&mut source)?),
                _ => (4 + code, word(&mut source)?),
            };
            if distance == 0 || distance > out.len() {
                return Err(Error::Invalid("XPCM back reference"));
            }
            for _ in 0..count.min(length - out.len()) {
                out.push(out[out.len() - distance]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn a_zero_spectrum_stays_silent_across_the_transform_overlap() {
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(&[0; TRANSFORM * 4]).unwrap();
        let encoded = compressed.finish().unwrap();
        let pcm =
            decode_transform(&encoded, 3, 1, (HOP + 40) * 2, &AtomicBool::new(false)).unwrap();
        assert_eq!(pcm, vec![0; (HOP + 40) * 2]);
    }
}
