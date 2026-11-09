use super::*;
use blowfish::{Blowfish, cipher::{BlockCipherEncrypt, KeyInit}};
use std::io::Write;

fn encrypt(bytes: &mut [u8]) {
    let cipher: Blowfish = Blowfish::new_from_slice(&[0x12; 8]).unwrap();
    for block in bytes.as_chunks_mut::<8>().0 {
        block[..4].reverse();
        block[4..].reverse();
        cipher.encrypt_block((&mut *block).into());
        block[..4].reverse();
        block[4..].reverse();
    }
}
fn fixture(root: &Path, version: u8, packed: bool, script: &[u8]) {
    let mut payload = if packed {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(script).unwrap();
        encoder.finish().unwrap()
    } else { script.to_vec() };
    let stored = payload.len();
    payload.resize(payload.len().div_ceil(8) * 8, 0);
    encrypt(&mut payload);
    let mut index = 1u32.to_le_bytes().to_vec();
    index.extend(b"entry.sc\0");
    let offset_field = index.len();
    index.extend(0u64.to_le_bytes());
    for value in [script.len() as u32, stored as u32, payload.len() as u32, u32::from(packed)] {
        index.extend(value.to_le_bytes());
    }
    index.resize(index.len().div_ceil(8) * 8, 0);
    let header_length = if version == 0 { 4 } else { 36 };
    let offset = (header_length + index.len()) as u64;
    index[offset_field..offset_field + 8].copy_from_slice(&offset.to_le_bytes());
    encrypt(&mut index);
    let mut header = vec![0; header_length];
    let xor = if version == 0 { 0 } else { 0x5a };
    let index_size = index.len() as u32 ^ u32::from_le_bytes([xor; 4]);
    header[header_length - 4..].copy_from_slice(&index_size.to_le_bytes());
    for byte in index.iter_mut().chain(payload.iter_mut()) { *byte ^= xor; }
    header.extend(index);
    header.extend(payload);
    fs::write(root.join("scr.paz"), header).unwrap();
    fs::write(root.join("key.toml"), format!(
        "schema = \"shinki.emu.musica.keys.v2\"\npaz_version = {version}\nindex_size_xor = {}\nrc4_skip_crc = false\n[archive_keys.scr]\nindex_key_hex = \"1212121212121212\"\ndata_key_hex = \"1212121212121212\"\ntype_passwords = {{}}\n",
        u32::from_le_bytes([xor; 4])
    )).unwrap();
}
fn detect(root: &Path) -> DetectionResult<Vec<EngineDetection>> {
    detect_game_directory(root, &NativeCancellation::new(1))
}
#[test]
fn recognizes_all_paz_generations_from_decrypted_index_and_script() {
    for version in 0..=2 {
        for packed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path(), version, packed, b".message 1   Public fixture\r\n.end\r\n");
            let found = detect(root.path()).unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].family_id, "musica");
            assert_eq!(found[0].format, "musica.paz");
        }
    }
}
#[test]
fn profile_wrong_keys_non_scripts_and_truncated_indexes_do_not_match() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), 2, false, b"ordinary unrelated plaintext\n");
    assert!(detect(root.path()).unwrap().is_empty());
    fixture(root.path(), 2, false, b".end\r\n");
    let profile = fs::read_to_string(root.path().join("key.toml")).unwrap();
    fs::write(root.path().join("key.toml"), profile.replace("1212121212121212", "3434343434343434")).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
    fixture(root.path(), 2, false, b".end\r\n");
    let path = root.path().join("scr.paz");
    let bytes = fs::read(&path).unwrap();
    for cut in 0..bytes.len() {
        fs::write(&path, &bytes[..cut]).unwrap();
        assert!(detect(root.path()).unwrap().is_empty());
    }
    fs::remove_file(root.path().join("key.toml")).unwrap();
    fs::write(path, bytes).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}

#[test]
fn renamed_and_nested_archives_are_identified_by_decryption_not_basename() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), 2, false, b".end\r\n");
    fs::rename(root.path().join("scr.paz"), root.path().join("unrelated-extension.bin")).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "musica");
    fs::create_dir_all(root.path().join("localized/bundles")).unwrap();
    fs::rename(root.path().join("unrelated-extension.bin"), root.path().join("localized/bundles/archive.custom")).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "musica");
}

#[test]
fn multipart_header_index_and_script_can_cross_volume_boundaries() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), 2, false, b".message 1   Public\r\n.end\r\n");
    let base = root.path().join("scr.paz");
    let bytes = fs::read(&base).unwrap();
    for cut in [2, 20, 40, bytes.len() - 4] {
        fs::write(&base, &bytes[..cut]).unwrap();
        fs::write(root.path().join("scr.pazA"), &bytes[cut..]).unwrap();
        assert_eq!(detect(root.path()).unwrap()[0].family_id, "musica", "split at {cut}");
    }
    fs::rename(root.path().join("scr.pazA"), root.path().join("scr.pazC")).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}

#[test]
fn receipt_configuration_resolves_private_profile_archive_alias_and_loose_script() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), 1, false, b"ordinary non-script bytes\r\n");
    fs::create_dir_all(root.path().join("private")).unwrap();
    fs::create_dir_all(root.path().join("archives")).unwrap();
    fs::create_dir_all(root.path().join("translated")).unwrap();
    fs::rename(root.path().join("key.toml"), root.path().join("private/keys.toml")).unwrap();
    fs::rename(root.path().join("scr.paz"), root.path().join("archives/translated.bundle")).unwrap();
    fs::write(root.path().join("translated/entry.sc"), b".end\r\n").unwrap();
    let receipt = serde_json::json!({
        "schema": "shinki.emu.file_patch.v1",
        "profile": {"configuration": {
            "profile_file": "PRIVATE/keys.toml",
            "scr_archive": "ARCHIVES/translated.bundle",
            "script_directory": "TRANSLATED"
        }}
    });
    fs::write(root.path().join("shinki-patch-result.json"), serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "musica");
    fs::create_dir(root.path().join(".shinki")).unwrap();
    fs::rename(root.path().join("shinki-patch-result.json"), root.path().join(".shinki/patch-result.json")).unwrap();
    assert_eq!(detect(root.path()).unwrap()[0].family_id, "musica");
    let bad = serde_json::json!({
        "schema": "shinki.emu.file_patch.v1",
        "profile": {"configuration": {"profile_file": "../keys.toml", "scr_archive": "../archive.paz"}}
    });
    fs::write(root.path().join(".shinki/patch-result.json"), serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(detect(root.path()).unwrap().is_empty());
}
