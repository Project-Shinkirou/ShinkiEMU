//! Native minori auxiliary files. Text values are kept as bytes so unknown
//! CP932 properties and resource names survive a read/write cycle unchanged.
use crate::CoreError;
use blowfish::{
    Blowfish,
    cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit},
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};

const LIMIT: usize = 64 * 1024 * 1024;
const READ_FLAG_BYTES: usize = 8192;
const SYSTEM_KEY: [u8; 6] = [0x93, 0x97, 0x92, 0x91, 0x8e, 0x97];

/// Property bytes use the game's original encoding; unknown fields are retained.
pub type Properties = BTreeMap<Vec<u8>, Vec<u8>>;

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_MUSICA_ORIGINAL_DATA", message)
}

fn compress(bytes: &[u8]) -> Result<Vec<u8>, CoreError> {
    if bytes.len() > LIMIT {
        return Err(invalid("original data exceeds its size limit"));
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(bytes)
        .map_err(|_| invalid("original data compression failed"))?;
    z.finish()
        .map_err(|_| invalid("original data compression failed"))
}

fn decompress(bytes: &[u8], bound: usize, padding: bool) -> Result<Vec<u8>, CoreError> {
    if bytes.len() > LIMIT {
        return Err(invalid("original compressed data exceeds its size limit"));
    }
    let mut z = ZlibDecoder::new(bytes);
    let mut plain = Vec::new();
    (&mut z)
        .take((bound + 1) as u64)
        .read_to_end(&mut plain)
        .map_err(|_| invalid("original compressed data is invalid"))?;
    if plain.len() > bound {
        return Err(invalid("original data exceeds its size limit"));
    }
    let remaining = bytes.len().saturating_sub(z.total_in() as usize);
    if (!padding && remaining != 0) || (padding && remaining > 7) {
        return Err(invalid(
            "original compressed data has unexpected trailing bytes",
        ));
    }
    Ok(plain)
}

fn cipher_blocks(bytes: &mut [u8], encrypt: bool) -> Result<(), CoreError> {
    if !bytes.len().is_multiple_of(8) {
        return Err(invalid("original system data is not block aligned"));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(&SYSTEM_KEY)
        .map_err(|_| invalid("original system data key is invalid"))?;
    for block in bytes.as_chunks_mut::<8>().0 {
        block[..4].reverse();
        block[4..].reverse();
        if encrypt {
            cipher.encrypt_block((&mut *block).into());
        } else {
            cipher.decrypt_block((&mut *block).into());
        }
        block[..4].reverse();
        block[4..].reverse();
    }
    Ok(())
}

fn parse_properties(bytes: &[u8], terminator: bool) -> Result<Properties, CoreError> {
    if bytes.len() > LIMIT {
        return Err(invalid("original properties exceed their size limit"));
    }
    let bytes = if terminator {
        bytes
            .strip_suffix(&[0])
            .ok_or_else(|| invalid("system properties lack a terminator"))?
    } else {
        bytes
    };
    let mut result = Properties::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let separator = line
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| invalid("original property is missing its separator"))?;
        let (key, value) = line.split_at(separator);
        if key.is_empty()
            || key.contains(&0)
            || value[1..].contains(&0)
            || result.insert(key.to_vec(), value[1..].to_vec()).is_some()
        {
            return Err(invalid("original properties contain an invalid field"));
        }
    }
    Ok(result)
}

fn format_properties(properties: &Properties, crlf: bool) -> Result<Vec<u8>, CoreError> {
    let mut bytes = Vec::new();
    for (key, value) in properties {
        if key.is_empty()
            || key
                .iter()
                .any(|byte| matches!(byte, 0 | b'\t' | b'\r' | b'\n'))
            || value.iter().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
        {
            return Err(invalid("original properties contain an invalid field"));
        }
        bytes.extend_from_slice(key);
        bytes.push(b'\t');
        bytes.extend_from_slice(value);
        if crlf {
            bytes.push(b'\r');
        }
        bytes.push(b'\n');
        if bytes.len() > LIMIT {
            return Err(invalid("original properties exceed their size limit"));
        }
    }
    Ok(bytes)
}

/// Decode the encrypted, zlib-compressed `system.dat` variable table.
pub fn decode_system(bytes: &[u8]) -> Result<Properties, CoreError> {
    if bytes.len() < 8 || bytes.len() > LIMIT || !bytes.len().is_multiple_of(8) {
        return Err(invalid("original system data has an invalid length"));
    }
    let mut decrypted = bytes.to_vec();
    cipher_blocks(&mut decrypted, false)?;
    let expected = u32::from_le_bytes(decrypted[..4].try_into().unwrap()) as usize;
    if expected == 0 || expected > LIMIT {
        return Err(invalid(
            "original system data has an invalid decoded length",
        ));
    }
    let plain = decompress(&decrypted[4..], expected, true)?;
    if plain.len() != expected {
        return Err(invalid("original system data length does not match"));
    }
    parse_properties(&plain, true)
}

/// Encode the original little-endian Blowfish and zlib `system.dat` format.
pub fn encode_system(properties: &Properties) -> Result<Vec<u8>, CoreError> {
    let mut plain = format_properties(properties, false)?;
    plain.push(0);
    let compressed = compress(&plain)?;
    let mut bytes = Vec::with_capacity((compressed.len() + 11) & !7);
    bytes.extend_from_slice(&(plain.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&compressed);
    bytes.resize((bytes.len() + 7) & !7, 0);
    cipher_blocks(&mut bytes, true)?;
    Ok(bytes)
}

/// Decode `sysparams.txt` while preserving unmapped native property bytes.
pub fn decode_settings(bytes: &[u8]) -> Result<Properties, CoreError> {
    parse_properties(bytes, false)
}

/// Encode `sysparams.txt` with the original tab-separated CRLF layout.
pub fn encode_settings(properties: &Properties) -> Result<Vec<u8>, CoreError> {
    format_properties(properties, true)
}

fn decode_zfile(bytes: &[u8], bound: usize) -> Result<Vec<u8>, CoreError> {
    let compressed = bytes
        .strip_prefix(&[0, 0, 0, 0])
        .ok_or_else(|| invalid("original compressed file header is invalid"))?;
    decompress(compressed, bound, false)
}

fn encode_zfile(plain: &[u8]) -> Result<Vec<u8>, CoreError> {
    let compressed = compress(plain)?;
    let mut bytes = Vec::with_capacity(4 + compressed.len());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&compressed);
    Ok(bytes)
}

/// Decode one script's `<script>.tbr` read bitmap.
pub fn decode_read_flags(bytes: &[u8]) -> Result<[u8; READ_FLAG_BYTES], CoreError> {
    decode_zfile(bytes, READ_FLAG_BYTES)?
        .try_into()
        .map_err(|_| invalid("original read bitmap has an invalid length"))
}

/// Encode one script's `<script>.tbr` read bitmap.
pub fn encode_read_flags(bits: &[u8; READ_FLAG_BYTES]) -> Result<Vec<u8>, CoreError> {
    encode_zfile(bits)
}

/// Decode the newline-separated original `cgv.dat` image names.
pub fn decode_cg_names(bytes: &[u8]) -> Result<Vec<Vec<u8>>, CoreError> {
    let plain = decode_zfile(bytes, LIMIT)?;
    let Some(body) = plain.strip_suffix(b"\n") else {
        if plain.is_empty() {
            return Ok(Vec::new());
        }
        return Err(invalid("original CG list is not newline terminated"));
    };
    let mut names = Vec::new();
    for line in body.split(|byte| *byte == b'\n') {
        if line.is_empty() || line.iter().any(|byte| matches!(byte, 0 | b'\r')) {
            return Err(invalid("original CG list contains an invalid name"));
        }
        names.push(line.to_vec());
    }
    Ok(names)
}

/// Encode the original `cgv.dat` image-name list.
pub fn encode_cg_names(names: &[Vec<u8>]) -> Result<Vec<u8>, CoreError> {
    let mut plain = Vec::new();
    for name in names {
        if name.is_empty() || name.iter().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
            return Err(invalid("original CG list contains an invalid name"));
        }
        plain.extend_from_slice(name);
        plain.push(b'\n');
        if plain.len() > LIMIT {
            return Err(invalid("original CG list exceeds its size limit"));
        }
    }
    encode_zfile(&plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independently_constructed_system_fixture_round_trips() {
        // Synthetic property, encrypted independently with the original
        // little-endian Blowfish block convention and negated "minori" key.
        let fixture = [
            255, 41, 217, 178, 49, 197, 50, 125, 253, 33, 52, 90, 58, 139, 86, 140, 68, 250, 42,
            94, 195, 2, 37, 87, 223, 71, 135, 4, 3, 43, 24, 209,
        ];
        let properties = Properties::from([(b"REN_CLEAR".to_vec(), b"1".to_vec())]);
        assert_eq!(decode_system(&fixture).unwrap(), properties);
        assert_eq!(
            decode_system(&encode_system(&properties).unwrap()).unwrap(),
            properties
        );
    }

    #[test]
    fn original_settings_cg_and_script_bitmap_round_trip() {
        let settings = b"fontname\t\x82\xa0\r\nvolumeBGM\t75\r\n";
        let decoded = decode_settings(settings).unwrap();
        assert_eq!(encode_settings(&decoded).unwrap(), settings);
        let names = vec![b"bg01.png".to_vec(), b"st01.png".to_vec()];
        let cg_fixture = [
            0, 0, 0, 0, 120, 156, 75, 74, 55, 48, 212, 43, 200, 75, 231, 42, 46, 129, 50, 0, 52,
            194, 5, 109,
        ];
        assert_eq!(decode_cg_names(&cg_fixture).unwrap(), names);
        assert_eq!(
            decode_cg_names(&encode_cg_names(&names).unwrap()).unwrap(),
            names
        );
        let mut bits = [0u8; READ_FLAG_BYTES];
        bits[12] = 0x20;
        let flags_fixture = [
            0, 0, 0, 0, 120, 156, 237, 193, 65, 13, 0, 0, 8, 0, 161, 139, 98, 255, 148, 62, 205,
            224, 6, 212, 153, 0, 0, 0, 0, 0, 0, 0, 128, 143, 22, 30, 188, 0, 33,
        ];
        assert_eq!(decode_read_flags(&flags_fixture).unwrap(), bits);
        assert_eq!(
            decode_read_flags(&encode_read_flags(&bits).unwrap()).unwrap(),
            bits
        );
    }
}
