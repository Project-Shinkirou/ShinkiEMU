//! Native 4704D0/4703C0 LZSS: zero-filled 2048-byte ring, cursor2015,
//! LSB-first flags (one means literal), match lengths2..33.
use super::slot_io::{Reader, invalid};
use shinki_emu_sdk::CoreError;

/// Literal packets are a native encoding with linear runtime and bounded growth.
pub(crate) fn compress(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len().div_ceil(8));
    for chunk in bytes.chunks(8) {
        out.push(((1u16 << chunk.len()) - 1) as u8);
        out.extend_from_slice(chunk);
    }
    out
}

pub(crate) fn decompress(bytes: &[u8], expected: usize) -> Result<Vec<u8>, CoreError> {
    if expected > 64 * 1024 * 1024 {
        return Err(invalid("native LZSS output exceeds its byte limit"));
    }
    let mut r = Reader::new(bytes);
    let mut ring = [0u8; 2048];
    let mut cursor = 2015;
    let mut out = Vec::with_capacity(expected);
    while out.len() < expected {
        let flags = r.u8()?;
        for bit in 0..8 {
            if out.len() == expected {
                break;
            }
            if flags & (1 << bit) != 0 {
                let byte = r.u8()?;
                out.push(byte);
                ring[cursor] = byte;
                cursor = (cursor + 1) & 2047;
            } else {
                let low = r.u8()? as usize;
                let high = r.u8()? as usize;
                let source = low | ((high & 224) << 3);
                let length = (high & 31) + 2;
                if length > expected - out.len() {
                    return Err(invalid("native LZSS match exceeds declared output"));
                }
                for offset in 0..length {
                    let byte = ring[(source + offset) & 2047];
                    out.push(byte);
                    ring[cursor] = byte;
                    cursor = (cursor + 1) & 2047;
                }
            }
        }
    }
    r.finish()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_literals_and_overlapping_matches() {
        // One literal at ring2015, then an overlapping six-byte match.
        assert_eq!(decompress(&[1, b'x', 0xdf, 0xe4], 7).unwrap(), b"xxxxxxx");
        assert_eq!(decompress(&[0, 0, 0], 2).unwrap(), [0, 0]);
        let bytes = b"abcdefgh0123456789";
        assert_eq!(decompress(&compress(bytes), bytes.len()).unwrap(), bytes);
    }
    #[test]
    fn rejects_truncated_oversized_and_trailing_streams() {
        for bytes in [&[0u8, 1][..], &[1u8][..]] {
            assert!(decompress(bytes, 2).is_err());
        }
        assert!(decompress(&[0, 0, 31], 2).is_err());
        assert!(decompress(&[1, b'x', 0], 1).is_err());
        assert!(decompress(&[], usize::MAX).is_err());
    }
}
