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

/// The two original `system.dat` envelopes. eden (English edition) reads a
/// zero u32 followed by RC4("minori") over the property table (eden_en.exe
/// `sub_4C1700`/`sub_4BFE60`); Perseus encrypts `[u32 length][zlib(table + NUL)]`
/// with little-endian Blowfish and zero padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemFormat {
    Rc4,
    Blowfish,
}

const RC4_KEY: &[u8] = b"minori";

fn rc4(bytes: &mut [u8]) -> Result<(), CoreError> {
    use rc4::{Rc4, StreamCipher};
    let mut cipher =
        Rc4::new_from_slice(RC4_KEY).map_err(|_| invalid("original system data key is invalid"))?;
    cipher.apply_keystream(bytes);
    Ok(())
}

/// Decode an original `system.dat` variable table.
pub fn decode_system(bytes: &[u8], format: SystemFormat) -> Result<Properties, CoreError> {
    if bytes.len() > LIMIT {
        return Err(invalid("original system data has an invalid length"));
    }
    if format == SystemFormat::Rc4 {
        let mut plain = bytes
            .strip_prefix(&[0, 0, 0, 0])
            .ok_or_else(|| invalid("original system data header is invalid"))?
            .to_vec();
        rc4(&mut plain)?;
        return parse_properties(&plain, false);
    }
    if bytes.len() < 8 || !bytes.len().is_multiple_of(8) {
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

/// Encode an original `system.dat` variable table in the given envelope.
pub fn encode_system(properties: &Properties, format: SystemFormat) -> Result<Vec<u8>, CoreError> {
    let mut plain = format_properties(properties, false)?;
    if format == SystemFormat::Rc4 {
        rc4(&mut plain)?;
        let mut bytes = Vec::with_capacity(4 + plain.len());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&plain);
        return Ok(bytes);
    }
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

/// The u32 every original compressed file starts with: CRC32 of the title's
/// `SerialCode` registry string, zero when the title has none.
pub fn file_prefix(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn decode_zfile(bytes: &[u8], bound: usize, prefix: u32) -> Result<Vec<u8>, CoreError> {
    let compressed = bytes
        .strip_prefix(&prefix.to_le_bytes())
        .ok_or_else(|| invalid("original compressed file header is invalid"))?;
    decompress(compressed, bound, false)
}

fn encode_zfile(plain: &[u8], prefix: u32) -> Result<Vec<u8>, CoreError> {
    let compressed = compress(plain)?;
    let mut bytes = Vec::with_capacity(4 + compressed.len());
    bytes.extend_from_slice(&prefix.to_le_bytes());
    bytes.extend_from_slice(&compressed);
    Ok(bytes)
}

/// Decode one script's `<script>.tbr` read bitmap.
pub fn decode_read_flags(bytes: &[u8], prefix: u32) -> Result<[u8; READ_FLAG_BYTES], CoreError> {
    decode_zfile(bytes, READ_FLAG_BYTES, prefix)?
        .try_into()
        .map_err(|_| invalid("original read bitmap has an invalid length"))
}

/// Encode one script's `<script>.tbr` read bitmap.
pub fn encode_read_flags(bits: &[u8; READ_FLAG_BYTES], prefix: u32) -> Result<Vec<u8>, CoreError> {
    encode_zfile(bits, prefix)
}

/// Decode the newline-separated original `cgv.dat` image names.
pub fn decode_cg_names(bytes: &[u8], prefix: u32) -> Result<Vec<Vec<u8>>, CoreError> {
    let plain = decode_zfile(bytes, LIMIT, prefix)?;
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
pub fn encode_cg_names(names: &[Vec<u8>], prefix: u32) -> Result<Vec<u8>, CoreError> {
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
    encode_zfile(&plain, prefix)
}

/// Original titles whose save layout is established from their executables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinoriTitle {
    EdenEnglish,
    Perseus,
}

impl MinoriTitle {
    const ALL: [Self; 2] = [Self::EdenEnglish, Self::Perseus];

    /// Folder below `minori/`. Both names are compiled-in literals: eden_en.exe
    /// `sub_4BB980` pushes 0x56AAE8, perseus.exe `sub_4705F0` pushes the CP932
    /// bytes at 0x4FEC8C. The original passes them to ANSI APIs, so a non-CP932
    /// system shows the Perseus folder as mojibake (GBK: 壞嬻偺儁儖僙僂僗).
    pub fn directory(self) -> &'static str {
        match self {
            Self::EdenEnglish => "eden (English edition)",
            Self::Perseus => "夏空のペルセウス",
        }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::EdenEnglish => "eden_en.exe",
            Self::Perseus => "perseus.exe",
        }
    }

    pub fn system_format(self) -> SystemFormat {
        match self {
            Self::EdenEnglish => SystemFormat::Rc4,
            Self::Perseus => SystemFormat::Blowfish,
        }
    }

    /// `HKLM` key whose `SerialCode` feeds the file prefix (perseus.exe
    /// `sub_47EFB0`). eden_en.exe has no serial and always writes zero.
    pub fn serial_key(self) -> Option<&'static str> {
        match self {
            Self::EdenEnglish => None,
            Self::Perseus => Some("Software\\minori\\perseus"),
        }
    }

    /// The title whose original executable sits in the game directory.
    pub fn detect(game_root: &std::path::Path) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|title| game_root.join(title.executable()).is_file())
    }
}

/// The u32 written before every compressed file and slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilePrefix {
    pub value: u32,
    /// The original rejects a mismatched prefix; Musica does the same only when
    /// the value came from the same input the original reads.
    pub strict: bool,
}

/// Precedence: the title's registry `SerialCode` (CRC32 of the string bytes),
/// then the prefix of an existing original file in the native directory, then
/// zero. Titles without a serial always use a strict zero.
pub fn resolve_prefix(
    title: MinoriTitle,
    registry_serial: impl FnOnce(&str) -> Option<Vec<u8>>,
    existing_file: Option<u32>,
) -> FilePrefix {
    let Some(key) = title.serial_key() else {
        return FilePrefix {
            value: 0,
            strict: true,
        };
    };
    if let Some(serial) = registry_serial(key) {
        let end = serial.iter().position(|byte| *byte == 0).unwrap_or(serial.len());
        return FilePrefix {
            value: crc32fast::hash(&serial[..end]),
            strict: true,
        };
    }
    match existing_file {
        Some(value) => FilePrefix {
            value,
            strict: true,
        },
        None => FilePrefix {
            value: 0,
            strict: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blowfish_system_fixture_keeps_its_envelope() {
        // Synthetic property, encrypted independently with the original
        // little-endian Blowfish block convention and negated "minori" key.
        let fixture = [
            255, 41, 217, 178, 49, 197, 50, 125, 253, 33, 52, 90, 58, 139, 86, 140, 68, 250, 42,
            94, 195, 2, 37, 87, 223, 71, 135, 4, 3, 43, 24, 209,
        ];
        let properties = Properties::from([(b"REN_CLEAR".to_vec(), b"1".to_vec())]);
        assert_eq!(
            decode_system(&fixture, SystemFormat::Blowfish).unwrap(),
            properties
        );
        let encoded = encode_system(&properties, SystemFormat::Blowfish).unwrap();
        assert!(encoded.len().is_multiple_of(8));
        assert_eq!(
            decode_system(&encoded, SystemFormat::Blowfish).unwrap(),
            properties
        );
        assert!(decode_system(&fixture, SystemFormat::Rc4).is_err());
    }

    #[test]
    fn rc4_system_fixture_reencodes_byte_identically() {
        // Zero u32 then RC4("minori") over the table, as eden_en.exe writes it.
        let fixture = [
            0, 0, 0, 0, 119, 189, 225, 184, 158, 226, 131, 214, 249, 31, 157, 81, 58, 20, 42, 147,
            128, 201, 226, 70, 51, 82, 150, 162, 230, 69, 181, 130, 158, 188,
        ];
        let decoded = decode_system(&fixture, SystemFormat::Rc4).unwrap();
        assert_eq!(decoded[b"EYE_SEQ_DEFAULT".as_slice()], b"2");
        assert_eq!(decoded[b"REN_CLEAR".as_slice()], b"1");
        assert_eq!(encode_system(&decoded, SystemFormat::Rc4).unwrap(), fixture);
        let mut foreign = fixture;
        foreign[0] = 1;
        assert!(decode_system(&foreign, SystemFormat::Rc4).is_err());
    }

    #[test]
    fn settings_cg_and_read_flags_keep_plaintext_and_prefix() {
        let settings = b"fontname\t\x82\xa0\r\nvolumeBGM\t75\r\n";
        let decoded = decode_settings(settings).unwrap();
        assert_eq!(encode_settings(&decoded).unwrap(), settings);
        let names = vec![b"bg01.png".to_vec(), b"st01.png".to_vec()];
        let cg_fixture = [
            0, 0, 0, 0, 120, 156, 75, 74, 55, 48, 212, 43, 200, 75, 231, 42, 46, 129, 50, 0, 52,
            194, 5, 109,
        ];
        assert_eq!(decode_cg_names(&cg_fixture, 0).unwrap(), names);
        assert!(decode_cg_names(&cg_fixture, 7).is_err());
        let serial = 0x2299_cf44;
        let encoded = encode_cg_names(&names, serial).unwrap();
        assert_eq!(file_prefix(&encoded), Some(serial));
        assert_eq!(decode_cg_names(&encoded, serial).unwrap(), names);
        assert!(decode_cg_names(&encoded, 0).is_err());
        let mut bits = [0u8; READ_FLAG_BYTES];
        bits[12] = 0x20;
        let flags_fixture = [
            0, 0, 0, 0, 120, 156, 237, 193, 65, 13, 0, 0, 8, 0, 161, 139, 98, 255, 148, 62, 205,
            224, 6, 212, 153, 0, 0, 0, 0, 0, 0, 0, 128, 143, 22, 30, 188, 0, 33,
        ];
        assert_eq!(decode_read_flags(&flags_fixture, 0).unwrap(), bits);
        let encoded = encode_read_flags(&bits, 0).unwrap();
        assert_eq!(&encoded[..4], &[0; 4]);
        assert_eq!(decode_read_flags(&encoded, 0).unwrap(), bits);
    }
}
