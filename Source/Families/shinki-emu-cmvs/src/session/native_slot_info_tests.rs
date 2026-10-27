use crate::session::test_support::{command, game, open, picture, pixel, render};

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_native_browser_reads_headers_without_mutating_compressed_state_and_loads_preview() {
    let (root, archive) = game();
    let mut session = open(root.path(), archive);
    picture(&mut session, "red.pb3", 1);
    session.vm.interpreter_string_buffers.insert(
        10324,
        vec![crate::CmvsStringSegment::OwnedString(
            "saved chapter".into(),
        )],
    );
    command(&mut session, 688, &[2, 1]);
    command(&mut session, 696, &[]);
    command(&mut session, 690, &[]);
    command(&mut session, 694, &[987]);
    let path = root.path().join("save/save987.dat");
    let original = std::fs::read(&path).unwrap();
    session
        .vm
        .process_string_slots
        .insert(3, b"slot annotation".to_vec());
    command(&mut session, 700, &[987, 0x8000_0003]);
    assert_eq!(session.vm.interpreter_words[&81220], 1);
    command(&mut session, 702, &[987, 0x1122_3344, 0x5566_7788]);
    assert_eq!(session.vm.interpreter_words[&81220], 1);
    let updated = std::fs::read(&path).unwrap();
    assert_eq!(&updated[600..], &original[600..]);
    crate::system_save::slot::decode_native_slot(&updated).unwrap();
    picture(&mut session, "blue.pb3", 2);
    command(
        &mut session,
        701,
        &[987, 7, u32::MAX, 0x8000_0001, 0x8000_0002],
    );
    assert_eq!(session.vm.interpreter_words[&81220], 1);
    assert!(session.vm.process_string_slots[&1].ends_with(b"saved chapter"));
    assert_eq!(session.vm.process_string_slots[&2], b"slot annotation");
    assert_eq!(session.vm.interpreter_words[&81240], 0x1122_3344);
    assert_eq!(session.vm.interpreter_words[&81244], 0x5566_7788);
    command(&mut session, 64, &[7]);
    command(&mut session, 68, &[7, u32::MAX, 0, 0, 2, 1]);
    render(&mut session);
    assert_eq!(pixel(&session, 0, 0), [240, 0, 0, 255]);
    assert_eq!(pixel(&session, 4, 4), [0, 0, 240, 255]);
    command(&mut session, 51, &[7]);
    assert_eq!(session.vm.interpreter_words[&81224], 2);
    assert_eq!(session.vm.interpreter_words[&81228], 1);
    assert_eq!(session.vm.interpreter_words[&81232], 0);
    let mut old_port = updated;
    old_port[..4].copy_from_slice(b"CUS2");
    std::fs::write(root.path().join("save/save986.dat"), old_port).unwrap();
    command(&mut session, 699, &[986]);
    assert_eq!(session.vm.interpreter_words[&81220], 0);
    command(&mut session, 699, &[987]);
    assert_eq!(session.vm.interpreter_words[&81220], 1);
}

#[test]
fn native_32_bit_thumbnail_keeps_the_alpha_byte() {
    let root = tempfile::tempdir().unwrap();
    let mut bmp = vec![0; 58];
    bmp[..2].copy_from_slice(b"BM");
    for (at, word) in [(2, 58u32), (10, 54), (14, 40), (18, 1), (22, 1)] {
        bmp[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    bmp[26..30].copy_from_slice(&[1, 0, 32, 0]);
    bmp[54..58].copy_from_slice(&[10, 20, 30, 64]);
    let packed = crate::system_save::lzss::compress(&bmp);
    let mut header = [0; 600];
    header[564..568].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    header[584..588].copy_from_slice(&(bmp.len() as u32).to_le_bytes());
    let mut file_bytes = header.to_vec();
    file_bytes.extend(packed);
    let path = root.path().join("thumbnail.dat");
    std::fs::write(&path, file_bytes).unwrap();
    let mut file = std::fs::File::open(path).unwrap();
    let (frame, source_alpha) = super::bitmap(&mut file, &header).unwrap();
    assert!(source_alpha);
    assert_eq!(&frame.rgba8[..4], &[30, 20, 10, 64]);
}
