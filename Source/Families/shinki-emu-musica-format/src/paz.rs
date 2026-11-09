//! Pure PAZ header/index and transforms shared by playback and recognition.
use std::{collections::BTreeMap, io::{Cursor, Read}};
use blowfish::{Blowfish, cipher::{BlockCipherDecrypt, KeyInit}};
use encoding_rs::SHIFT_JIS;
use rc4::{Rc4, StreamCipher};
use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct FormatError { pub code: &'static str, pub message: String }
type PazError = FormatError;
fn error(code: &'static str, message: impl Into<String>) -> FormatError {
    FormatError { code, message: message.into() }
}
pub const MAX_INDEX_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntryDescriptor {
    pub archive_role: String,
    pub entry_id: String,
    pub name: String,
    pub offset: u64,
    pub unpacked_size: u64,
    pub stored_size: u64,
    pub aligned_size: u64,
    pub packed: bool,
    pub video_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct PazRoleScheme {
    pub index_key: Vec<u8>,
    pub rc4_skip_crc: bool,
    pub data_key: Vec<u8>,
    pub type_passwords: BTreeMap<String, String>,
    pub archive_xor: Option<u32>,
    pub video_key: Option<[u8; 256]>,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexLayout { pub offset: u64, pub size: u64, pub xor: u8 }
/// Inspect only the version-selected fixed header, before index allocation.
pub fn index_layout(header: &[u8], version: u8, expected_xor: u32, file_length: u64) -> Result<IndexLayout, FormatError> {
    let (offset, raw) = if version == 0 {
        (4, header.get(..4))
    } else {
        (36, header.get(32..36))
    };
    let raw = raw.ok_or_else(|| error("SHINKI_EMU_MUSICA_INDEX_SHORT", "PAZ header is truncated"))?;
    let raw = u32::from_le_bytes(raw.try_into().unwrap());
    let xor = if version == 0 { 0 } else { (raw >> 24) as u8 };
    let derived = u32::from_le_bytes([xor; 4]);
    if version != 0 && expected_xor != 0 && expected_xor != derived {
        return Err(error("SHINKI_EMU_MUSICA_INDEX_XOR", "configured index XOR does not match the archive header"));
    }
    let size = (raw ^ derived) as u64;
    if size == 0 || size > MAX_INDEX_BYTES || !size.is_multiple_of(8) || offset + size > file_length {
        return Err(error("SHINKI_EMU_MUSICA_INDEX_SIZE", "PAZ index size is empty, unaligned, or out of bounds"));
    }
    Ok(IndexLayout { offset, size, xor })
}

pub fn parse_index(decoded: &[u8], role: &str, file_length: u64, cancelled: impl Fn() -> bool) -> Result<Vec<ArchiveEntryDescriptor>, FormatError> {
    let mut cursor = Cursor::new(decoded);
    let count = read_u32(&mut cursor)? as usize;
    if count > 1_000_000 {
        return Err(error(
            "SHINKI_EMU_MUSICA_ENTRY_COUNT",
            "PAZ entry count exceeds the configured limit",
        ));
    }
    let video_key = if role == "mov" {
        let mut key = vec![0u8; 256];
        cursor
            .read_exact(&mut key)
            .map_err(|_| error("SHINKI_EMU_MUSICA_VIDEO_KEY", "PAZ video key is truncated"))?;
        Some(key)
    } else {
        None
    };
    if count > decoded.len() / 25 || cancelled() {
        return Err(error("SHINKI_EMU_MUSICA_INDEX_SHORT", "PAZ index count exceeds available descriptors or parsing was cancelled"));
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        if cancelled() { return Err(error("SHINKI_EMU_MUSICA_CANCELLED", "PAZ parsing cancelled")); }
        let name = read_c_string(&mut cursor)?;
        let offset = read_u64(&mut cursor)?;
        let unpacked_size = read_u32(&mut cursor)? as u64;
        let stored_size = read_u32(&mut cursor)? as u64;
        let aligned_size = read_u32(&mut cursor)? as u64;
        let packed = read_i32(&mut cursor)? != 0;
        if unpacked_size > MAX_ENTRY_BYTES
            || stored_size > aligned_size
            || (role != "mov" && !aligned_size.is_multiple_of(8))
            || offset
                .checked_add(aligned_size)
                .is_none_or(|end| end > file_length)
        {
            tracing::error!(
                event = "shinki_emu_musica_entry_bounds_invalid",
                archive_role = %role,
                entry_index = index,
                offset,
                unpacked_size,
                stored_size,
                aligned_size,
                archive_size = file_length
            );
            return Err(error(
                "SHINKI_EMU_MUSICA_ENTRY_BOUNDS",
                "PAZ entry descriptor is oversized, unaligned, or out of bounds",
            ));
        }
        entries.push(ArchiveEntryDescriptor {
            archive_role: role.to_owned(),
            entry_id: format!("{}:{index}", role),
            name,
            offset,
            unpacked_size,
            stored_size,
            aligned_size,
            packed,
            video_key: video_key.clone(),
        });
    }
    if cursor.position() as usize > decoded.len() {
        return Err(error(
            "SHINKI_EMU_MUSICA_INDEX_SHORT",
            "PAZ index is truncated",
        ));
    }
    Ok(entries)
}

pub fn decrypt_entry_chunk(scheme: &PazRoleScheme, version: u8, entry: &ArchiveEntryDescriptor, absolute_offset: u64, encrypted: &[u8]) -> Result<Vec<u8>, FormatError> {
        let mut bytes = encrypted.to_vec();
        if entry.archive_role == "mov" {
            let video_key = entry.video_key.as_ref().ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_VIDEO_KEY",
                    "video entry is missing its index key",
                )
            })?;
            if version == 0 {
                let mut table = [0u8; 256];
                for (index, value) in video_key.iter().enumerate() {
                    table[*value as usize] = index as u8;
                }
                for byte in &mut bytes {
                    *byte = table[*byte as usize];
                }
                return Ok(bytes);
            }
            let material = format!(
                "{} {:08X} {}",
                entry.name.to_lowercase(),
                entry.unpacked_size,
                password_for_entry(entry, scheme).unwrap_or_default()
            );
            let (entry_key, _, malformed) = SHIFT_JIS.encode(&material);
            if malformed || entry_key.is_empty() {
                return Err(error(
                    "SHINKI_EMU_MUSICA_RC4_KEY",
                    "video RC4 key cannot be encoded as CP932",
                ));
            }
            let key = (0..256)
                .map(|index| video_key[index] ^ entry_key[index % entry_key.len()])
                .collect::<Vec<_>>();
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_RC4_KEY",
                    "video RC4 key length is invalid",
                )
            })?;
            let block_len = usize::try_from(entry.aligned_size.min(0x10000)).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_VIDEO_SIZE",
                    "movie transform size exceeds the platform bound",
                )
            })?;
            if block_len == 0 {
                return Ok(bytes);
            }
            let mut block = vec![0; block_len];
            cipher.apply_keystream(&mut block);
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte ^= block[(absolute_offset as usize + index) % block.len()];
            }
            return Ok(bytes);
        }
        bytes = blowfish_decrypt(&scheme.data_key, &bytes)?;
        if version > 0 && password_for_entry(entry, scheme).is_some() {
            let password = password_for_entry(entry, scheme).unwrap_or_default();
            let material = format!(
                "{} {:08X} {}",
                entry.name.to_lowercase(),
                entry.unpacked_size,
                password
            );
            let (key, _, malformed) = SHIFT_JIS.encode(&material);
            if malformed || key.is_empty() {
                return Err(error(
                    "SHINKI_EMU_MUSICA_RC4_KEY",
                    "entry RC4 key cannot be encoded as CP932",
                ));
            }
            let mut cipher = Rc4::new_from_slice(&key).map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_RC4_KEY",
                    "entry RC4 key length is invalid",
                )
            })?;
            let version_skip = if version >= 2 && scheme.rc4_skip_crc {
                ((crc32fast::hash(&key) >> 12) & 0xff) as u64
            } else {
                0
            };
            let skip = version_skip.checked_add(absolute_offset).ok_or_else(|| {
                error("SHINKI_EMU_MUSICA_RC4_SKIP", "entry RC4 offset overflowed")
            })?;
            if skip > 0 {
                let mut remaining = skip;
                let mut discarded = [0; 4096];
                while remaining > 0 {
                    let length =
                        usize::try_from(remaining.min(discarded.len() as u64)).map_err(|_| {
                            error(
                                "SHINKI_EMU_MUSICA_RC4_SKIP",
                                "entry RC4 offset exceeds the platform bound",
                            )
                        })?;
                    cipher.apply_keystream(&mut discarded[..length]);
                    discarded[..length].fill(0);
                    remaining -= length as u64;
                }
            }
            cipher.apply_keystream(&mut bytes);
        }
        Ok(bytes)
    }

pub fn password_for_entry<'a>(
    entry: &ArchiveEntryDescriptor,
    scheme: &'a PazRoleScheme,
) -> Option<&'a str> {
    if entry.packed {
        return None;
    }
    let lower = entry.name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        scheme.type_passwords.get("png").map(String::as_str)
    } else if lower.ends_with(".ogg") || matches!(entry.archive_role.as_str(), "se" | "voice") {
        scheme.type_passwords.get("ogg").map(String::as_str)
    } else if lower.ends_with(".sc") {
        scheme.type_passwords.get("sc").map(String::as_str)
    } else if lower.ends_with(".avi") || lower.ends_with(".mpg") || lower.ends_with(".mpeg") {
        scheme.type_passwords.get("avi").map(String::as_str)
    } else {
        None
    }
}

pub fn validate_blowfish_key(key: &[u8]) -> Result<(), PazError> {
    if !(4..=56).contains(&key.len()) {
        return Err(error(
            "SHINKI_EMU_MUSICA_BLOWFISH_KEY",
            "Blowfish key length must be between 4 and 56 bytes",
        ));
    }
    Ok(())
}

pub fn blowfish_decrypt(key: &[u8], encrypted: &[u8]) -> Result<Vec<u8>, PazError> {
    validate_blowfish_key(key)?;
    if !encrypted.len().is_multiple_of(8) {
        return Err(error(
            "SHINKI_EMU_MUSICA_BLOWFISH_ALIGNMENT",
            "Blowfish input is not block aligned",
        ));
    }
    let cipher: Blowfish = Blowfish::new_from_slice(key)
        .map_err(|_| error("SHINKI_EMU_MUSICA_BLOWFISH_KEY", "Blowfish key is invalid"))?;
    let mut bytes = encrypted.to_vec();
    for chunk in bytes.as_chunks_mut::<8>().0.iter_mut() {
        chunk[..4].reverse();
        chunk[4..].reverse();
        cipher.decrypt_block((&mut *chunk).into());
        chunk[..4].reverse();
        chunk[4..].reverse();
    }
    Ok(bytes)
}

pub fn xor_byte(bytes: &mut [u8], key: u8) {
    if key != 0 {
        for byte in bytes {
            *byte ^= key;
        }
    }
}

fn read_c_string(cursor: &mut Cursor<&[u8]>) -> Result<String, PazError> {
    let start = cursor.position() as usize;
    let bytes = cursor.get_ref();
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_INDEX_STRING",
                "PAZ entry name is not terminated",
            )
        })?;
    if end - start > 4096 {
        return Err(error(
            "SHINKI_EMU_MUSICA_INDEX_STRING",
            "PAZ entry name exceeds the configured limit",
        ));
    }
    let (text, _, malformed) = SHIFT_JIS.decode(&bytes[start..end]);
    if malformed {
        return Err(error(
            "SHINKI_EMU_MUSICA_INDEX_ENCODING",
            "PAZ entry name is not valid CP932",
        ));
    }
    cursor.set_position((end + 1) as u64);
    Ok(text.into_owned())
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, PazError> {
    let mut bytes = [0; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("SHINKI_EMU_MUSICA_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32, PazError> {
    Ok(read_u32(cursor)? as i32)
}
fn read_u64(cursor: &mut Cursor<&[u8]>) -> Result<u64, PazError> {
    let mut bytes = [0; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| error("SHINKI_EMU_MUSICA_INDEX_SHORT", "PAZ index is truncated"))?;
    Ok(u64::from_le_bytes(bytes))
}

/// Finish a fully decrypted entry with bounded zlib output and native padding rules.
pub fn finish_entry(entry: &ArchiveEntryDescriptor, mut decoded: Vec<u8>) -> Result<Vec<u8>, FormatError> {
    decoded.truncate(entry.stored_size as usize);
    let expected = entry.unpacked_size as usize;
    if entry.packed {
        let mut output = Vec::new();
        ZlibDecoder::new(decoded.as_slice()).take(entry.unpacked_size + 17).read_to_end(&mut output)
            .map_err(|_| error("SHINKI_EMU_MUSICA_ZLIB", "entry zlib stream is invalid"))?;
        decoded = output;
    }
    if decoded.len() > expected && decoded.len() - expected <= 16 && decoded[expected..].iter().all(|b| *b == 0) {
        decoded.truncate(expected);
    }
    if decoded.len() != expected { return Err(error("SHINKI_EMU_MUSICA_ENTRY_SIZE", "decoded entry size does not match its index descriptor")); }
    Ok(decoded)
}
