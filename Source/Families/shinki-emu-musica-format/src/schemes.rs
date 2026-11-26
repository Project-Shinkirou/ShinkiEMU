use crate::PazRoleScheme;
use shinki_emu_musica_keys::{MusicaArchiveKey, MusicaKeyFile};
use std::collections::BTreeMap;

/// Decode validated private configuration once. No resource access or logging.
pub fn into_schemes(key_file: &MusicaKeyFile) -> BTreeMap<String, PazRoleScheme> {
    key_file
        .archive_keys
        .iter()
        .map(|(role, key)| (role.clone(), decode_scheme(key, key_file.rc4_skip_crc)))
        .collect()
}

pub fn decode_scheme(key: &MusicaArchiveKey, rc4_skip_crc: bool) -> PazRoleScheme {
    let video_key = key.video_key_hex.as_ref().map(|hex| {
        std::array::from_fn(|index| decode_hex_byte(&hex.as_bytes()[index * 2..index * 2 + 2]))
    });
    PazRoleScheme {
        index_key: decode_hex(&key.index_key_hex),
        data_key: decode_hex(&key.data_key_hex),
        rc4_skip_crc,
        type_passwords: key.type_passwords.clone(),
        archive_xor: key.archive_xor,
        video_key,
    }
}
fn decode_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| decode_hex_byte(pair))
        .collect()
}
fn decode_hex_byte(pair: &[u8]) -> u8 {
    let nibble = |byte: u8| {
        if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte.to_ascii_lowercase() - b'a' + 10
        }
    };
    nibble(pair[0]) << 4 | nibble(pair[1])
}
