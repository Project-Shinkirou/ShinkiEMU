use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};

use super::*;

#[test]
fn ani_decodes_all_observed_pixel_formats() {
    for (bpp, raw, expected) in [
        (32, vec![1, 2, 3, 4], [3, 2, 1, 4]),
        (24, vec![1, 2, 3], [3, 2, 1, 255]),
        (16, 0xf800u16.to_le_bytes().to_vec(), [255, 0, 0, 255]),
        (8, vec![7], [7, 7, 7, 255]),
    ] {
        let mut bytes = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0, b'f', 0];
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&(bpp as u16).to_le_bytes());
        bytes.extend_from_slice(&(-2i16).to_le_bytes());
        bytes.extend_from_slice(&3i16.to_le_bytes());
        bytes.extend_from_slice(&raw);
        let archive = MusicaAniArchive::parse(Arc::<[u8]>::from(bytes)).unwrap();
        assert_eq!(
            (archive.frames()[0].offset_x, archive.frames()[0].offset_y),
            (-2, 3)
        );
        assert_eq!(archive.decode_frame(0).unwrap().as_raw(), &expected);
    }
}

#[test]
fn ani_rejects_truncation_and_trailing_data() {
    let bytes = vec![0x00, 0x01, 0x01, 0x00, 0, 0, 0, 0, b'f', 0];
    assert_eq!(
        MusicaAniArchive::parse(Arc::<[u8]>::from(bytes))
            .unwrap_err()
            .code(),
        "SHINKI_EMU_MUSICA_IMAGE_BOUNDS"
    );
}

#[test]
fn sqz_decodes_bgra_and_enforces_exact_output() {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&[1, 2, 3, 4]).unwrap();
    let frame = encoder.finish().unwrap();
    let index_end = 0x24u32;
    let mut bytes = b"SQZ1".to_vec();
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for _ in 0..2 {
        bytes.extend_from_slice(&index_end.to_le_bytes());
        bytes.extend_from_slice(&(frame.len() as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&frame);
    let archive = MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes)).unwrap();
    assert_eq!(archive.frames().len(), 2);
    assert_eq!(archive.decode_frame(1).unwrap().as_raw(), &[3, 2, 1, 4]);
}

#[test]
fn sqz_rejects_metadata_overlap_and_output_overrun() {
    let mut bytes = vec![0; 0x24];
    bytes[..4].copy_from_slice(b"SQZ1");
    bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&1u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes))
            .unwrap_err()
            .code(),
        "SHINKI_EMU_MUSICA_SQZ_ENTRY"
    );
}

#[test]
fn sqz_header_timing_and_parser_share_the_physical_frame_limit() {
    let pair_count = (MAX_FRAME_COUNT / 2) as u32;
    let count = pair_count * 2;
    let data_offset = 20 + count * 8;
    let mut bytes = b"SQZ1".to_vec();
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&pair_count.to_le_bytes());
    for _ in 0..count {
        bytes.extend_from_slice(&data_offset.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
    }
    bytes.push(0);
    assert_eq!(
        MusicaSqzArchive::header_pair_count(&bytes[..20]).unwrap(),
        pair_count
    );
    assert_eq!(
        MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes.clone()))
            .unwrap()
            .frames()
            .len(),
        MAX_FRAME_COUNT
    );
    bytes[16..20].copy_from_slice(&(pair_count + 1).to_le_bytes());
    assert!(MusicaSqzArchive::header_pair_count(&bytes[..20]).is_err());
    assert!(MusicaSqzArchive::parse(Arc::<[u8]>::from(bytes)).is_err());
}
