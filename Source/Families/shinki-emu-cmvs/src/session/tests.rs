use super::*;

/// A public, synthetic PS2A program: yield to the presenter, then loop.
pub(crate) fn fixture() -> (tempfile::TempDir, CmvsSession) {
    let (root, mut archive) = crate::archive::test_archive();
    archive.bind_game_root(root.path().to_path_buf()).unwrap();
    let mut bytecode = 0x2000u16.to_le_bytes().to_vec();
    bytecode.extend_from_slice(&0x400u16.to_le_bytes());
    bytecode.extend_from_slice(&0u32.to_le_bytes());
    let mut compressed = Vec::new();
    for chunk in bytecode.chunks(8) {
        compressed.push((1u16.checked_shl(chunk.len() as u32).unwrap() - 1) as u8);
        compressed.extend_from_slice(chunk);
    }
    for byte in &mut compressed {
        *byte = byte.rotate_left(1).wrapping_add(0x7c);
    }
    let mut source = vec![0; 0x30];
    source[..4].copy_from_slice(b"PS2A");
    source[4..8].copy_from_slice(&0x30u32.to_le_bytes());
    source[0x14..0x18].copy_from_slice(&(bytecode.len() as u32).to_le_bytes());
    source[0x24..0x28].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    source[0x28..0x2c].copy_from_slice(&(bytecode.len() as u32).to_le_bytes());
    source.extend_from_slice(&compressed);
    std::fs::write(root.path().join("start.ps3"), source).unwrap();
    let instance = roukaku_platform_common::native_wgpu_instance().unwrap();
    let audio = audio::test_output();
    let session = CmvsSession::new_with_audio(
        Arc::new(archive),
        root.path().to_path_buf(),
        "start.ps3",
        WindowState {
            width: 4,
            height: 4,
            focused: true,
            visible: true,
        },
        Lease::acquire().unwrap(),
        &instance,
        || Ok(audio),
    )
    .unwrap();
    (root, session)
}
