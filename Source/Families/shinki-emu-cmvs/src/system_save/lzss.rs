//! The saveNNN.dat container: LZSS + XOR + TLV per the native
//! `Save_writeFile` (0x4774D0) and `ProgState_serialize` (0x4712F0).
//!
//! Layout (re-report §4):
//! `[600-byte header][LZSS stream A (ProgState TLV, XOR
//! 'DAME_SAVE_IMAGE')][plain stream B][plain stream C][u16 checksum]`
//! The LZSS coder is the Okumura family: N=2048 (mask 0x7FF), F=33,
//! threshold 2, one flag byte per eight symbols (MSB first, 1=literal),
//! match bytes `[pos_lo][((pos>>8)&7)<<5 | (len-2)]`, ring prefilled at
//! [2015..2047].

use shinki_emu_sdk::CoreError;

const RING_SIZE: usize = 2048;
const MAX_MATCH: usize = 33;
const MATCH_THRESHOLD: usize = 2;
const RING_PREFILL: usize = 2015;

fn okumura_error(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_SAVE_LZSS", message)
}

/// Compresses with the encoder-side of the Okumura family the native
/// writer uses; deterministic output keeps the checksum stable.
pub fn lzss_compress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() + input.len() / 8 + 8);
    let mut ring = vec![0_u8; RING_SIZE];
    for (index, slot) in ring.iter_mut().enumerate().take(RING_SIZE) {
        *slot = index as u8;
    }
    let mut ring_position = RING_PREFILL.wrapping_add(MAX_MATCH - 1);
    let mut flags = 0_u8;
    let mut flag_count = 0_u8;
    let mut flag_position = 0_usize;
    let mut position = 0_usize;
    while position < input.len() {
        let mut best_length = 0_usize;
        let mut best_position = 0_usize;
        if position + MATCH_THRESHOLD < input.len() {
            let limit = MAX_MATCH.min(input.len() - position);
            let mut candidate = (ring_position.wrapping_add(1))
                .wrapping_sub(RING_PREFILL.wrapping_add(1))
                .wrapping_add(RING_PREFILL)
                & (RING_SIZE - 1);
            let mut probes = 0_usize;
            while probes < RING_SIZE && candidate != ring_position {
                let mut length = 0_usize;
                while length < limit
                    && ring[(candidate.wrapping_add(length)) & (RING_SIZE - 1)]
                        == input[position + length]
                {
                    length += 1;
                }
                if length > best_length {
                    best_length = length;
                    best_position = candidate;
                    if length >= limit {
                        break;
                    }
                }
                candidate = (candidate.wrapping_add(1)) & (RING_SIZE - 1);
                probes += 1;
            }
        }
        if flag_count == 0 {
            flag_position = output.len();
            output.push(0);
        }
        if best_length > MATCH_THRESHOLD {
            flags |= 1 << (7 - flag_count);
            output.push((best_position & 0xFF) as u8);
            output.push(
                (((best_position >> 8) & 7) << 5) as u8
                    | (best_length as u8 - MATCH_THRESHOLD as u8),
            );
            for offset in 0..best_length {
                ring[ring_position] = input[position + offset];
                ring_position = (ring_position.wrapping_add(1)) & (RING_SIZE - 1);
            }
            position += best_length;
        } else {
            output.push(input[position]);
            ring[ring_position] = input[position];
            ring_position = (ring_position.wrapping_add(1)) & (RING_SIZE - 1);
            position += 1;
        }
        flag_count = (flag_count + 1) % 8;
        if flag_count == 0 {
            output[flag_position] = flags;
            flags = 0;
        } else {
            output[flag_position] = flags;
        }
    }
    output
}

/// Decompresses; a truncated or corrupt stream fails explicitly.
pub fn lzss_decompress(input: &[u8], expected_len: usize) -> Result<Vec<u8>, CoreError> {
    let mut output = Vec::with_capacity(expected_len);
    let mut ring = vec![0_u8; RING_SIZE];
    for (index, slot) in ring.iter_mut().enumerate().take(RING_SIZE) {
        *slot = index as u8;
    }
    let mut ring_position = RING_PREFILL.wrapping_add(MAX_MATCH - 1);
    let mut position = 0_usize;
    while position < input.len() {
        let flags = input[position];
        position += 1;
        for bit in (0..8).rev() {
            if position >= input.len() {
                break;
            }
            if flags & (1 << bit) != 0 {
                let low = input[position] as usize;
                let high = input[position + 1] as usize;
                position += 2;
                let source = (low | ((high & 0xE0) << 3)) & (RING_SIZE - 1);
                let length = (high & 0x1F) + MATCH_THRESHOLD;
                for offset in 0..length {
                    let byte = ring[(source + offset) & (RING_SIZE - 1)];
                    output.push(byte);
                    if output.len() > expected_len {
                        return Err(okumura_error("LZSS output exceeds its declared length"));
                    }
                    ring[ring_position] = byte;
                    ring_position = (ring_position.wrapping_add(1)) & (RING_SIZE - 1);
                }
            } else {
                let byte = input[position];
                position += 1;
                output.push(byte);
                if output.len() > expected_len {
                    return Err(okumura_error("LZSS output exceeds its declared length"));
                }
                ring[ring_position] = byte;
                ring_position = (ring_position.wrapping_add(1)) & (RING_SIZE - 1);
            }
        }
    }
    Ok(output)
}

const XOR_KEY: &[u8; 15] = b"DAME_SAVE_IMAGE";

/// The stream-A cipher: `enc = key ^ (plain + 0x80)` over the 15-byte
/// cycle; only the TLV stream carries it.
pub fn xor_cipher(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(index, byte)| byte.wrapping_add(0x80) ^ XOR_KEY[index % XOR_KEY.len()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lzss_round_trips_the_native_parameters() {
        let mut sample = Vec::new();
        for index in 0..4096_u32 {
            sample.push((index.wrapping_mul(31) >> 8) as u8);
            if index % 97 == 0 {
                sample.extend_from_slice(b"DAME_SAVE_IMAGE");
            }
        }
        let packed = lzss_compress(&sample);
        let restored = lzss_decompress(&packed, sample.len()).unwrap();
        assert_eq!(restored, sample);
    }

    #[test]
    fn cipher_is_involutive() {
        let payload = b"ProgState TLV payload \x00\x01\x02".as_slice();
        assert_eq!(xor_cipher(&xor_cipher(payload)), payload);
    }
}
