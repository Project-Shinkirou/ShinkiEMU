use super::*;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[derive(Default)]
struct Bits(Vec<bool>);
impl Bits {
    fn number(&mut self, value: u32, count: u32) {
        self.0
            .extend((0..count).rev().map(|shift| value & (1 << shift) != 0));
    }
    fn amplitude(&mut self, value: i32) {
        let count = 32 - value.unsigned_abs().leading_zeros();
        self.number(count, 4); // Equal Huffman frequencies produce a balanced tree.
        if count != 0 {
            let raw = if value < 0 {
                value + (1 << count) - 1
            } else {
                value
            };
            self.number(raw as u32, count);
        }
    }
    fn bytes(self) -> Vec<u8> {
        let mut bytes = vec![0; self.0.len().div_ceil(8)];
        for (index, bit) in self.0.into_iter().enumerate() {
            bytes[index / 8] |= u8::from(bit) << (index & 7);
        }
        bytes
    }
}

pub(crate) fn frame(alpha: Option<i16>, luma_dc: i16, inherit: bool) -> Vec<u8> {
    let planes = if alpha.is_some() { 10 } else { 6 };
    let quant_count = if alpha.is_some() { 3 } else { 2 };
    let mut bytes = vec![0; 48 + 144 + quant_count * 64];
    bytes[..4].copy_from_slice(if alpha.is_some() { b"JBPE" } else { b"JBPD" });
    put32(&mut bytes, 4, 44);
    put32(&mut bytes, 8, 0x1800_1002);
    bytes[16..18].copy_from_slice(&16_u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&16_u16.to_le_bytes());
    for index in 0..32 {
        put32(&mut bytes, 48 + index * 4, 1);
    }
    bytes[48 + 144..].fill(1);
    if alpha.is_some() {
        bytes.extend_from_slice(&(planes as u32).to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 1, 1, 0, 1, 1, 0, 0]); // Grid, count, extent, padding.
    bytes.extend_from_slice(&[if inherit { 0x80 } else { 0 }, 0, 0, 0]);
    let mut dc = Bits::default();
    let mut previous = 0_i16;
    for plane in 0..planes {
        let value = if plane < 4 {
            luma_dc
        } else if plane < 6 {
            0
        } else {
            alpha.unwrap()
        };
        dc.amplitude(i32::from(value) - i32::from(previous));
        previous = value;
    }
    let dc = dc.bytes();
    let mut ac = Bits::default();
    for _ in 0..planes {
        ac.number(15, 4);
    }
    let ac = ac.bytes();
    put32(&mut bytes, 28, dc.len() as u32);
    put32(&mut bytes, 32, ac.len() as u32);
    bytes.extend(dc);
    bytes.extend(ac);
    bytes
}

pub(crate) fn container(alpha: bool, frames: &[Vec<u8>]) -> Vec<u8> {
    let base = 64 + frames.len() * 20;
    let mut bytes = vec![0; base];
    bytes[..4].copy_from_slice(if alpha { b"CMV8" } else { b"CMV6" });
    put32(&mut bytes, 4, base as u32);
    put32(&mut bytes, 16, frames.len() as u32);
    put32(&mut bytes, 20, 24);
    put32(&mut bytes, 24, 1);
    put32(&mut bytes, 28, 16);
    put32(&mut bytes, 32, 16);
    put32(&mut bytes, 36, if alpha { 32 } else { 24 });
    for (index, frame) in frames.iter().enumerate() {
        let record = 44 + index * 20;
        let offset = bytes.len() - base;
        put32(&mut bytes, record, index as u32);
        put32(&mut bytes, record + 4, frame.len() as u32);
        put32(&mut bytes, record + 8, 16 * 16 * 4);
        put32(&mut bytes, record + 12, 2);
        put32(&mut bytes, record + 16, offset as u32);
        bytes.extend(frame);
    }
    bytes
}

#[test]
fn rgb_delta_inherits_and_backwards_seek_replays() {
    let bytes = container(
        false,
        &[
            frame(None, -1024, false),
            frame(None, 0, true),
            frame(None, 0, false),
        ],
    );
    let mut decoder = CmvDecoder::new(bytes.into()).unwrap();
    let black = decoder.decode_frame(0).unwrap();
    assert_eq!(&black.rgba8[..4], &[0, 0, 0, 255]);
    assert_eq!(decoder.decode_frame(1).unwrap(), black);
    let gray = decoder.decode_frame(2).unwrap();
    assert_eq!(&gray.rgba8[..4], &[128, 128, 128, 255]);
    assert_eq!(decoder.decode_frame(1).unwrap(), black);
    assert_eq!(decoder.decode_frame(2).unwrap(), gray);
    // COW updates cannot mutate a texture the host retained.
    assert_eq!(&black.rgba8[..4], &[0, 0, 0, 255]);
}

#[test]
fn alpha_dct_has_no_luma_bias_and_clamps_at_both_boundaries() {
    for (dc, expected) in [(-8, 0), (0, 0), (1024, 128), (2040, 255), (2048, 255)] {
        let mut decoder =
            CmvDecoder::new(container(true, &[frame(Some(dc), 0, false)]).into()).unwrap();
        let decoded = decoder.decode_frame(0).unwrap();
        assert!(
            decoded
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [128, 128, 128, expected])
        );
    }
}

#[test]
fn alpha_delta_consumes_all_ten_blocks_and_keeps_prior_alpha() {
    let mut decoder = CmvDecoder::new(
        container(
            true,
            &[
                frame(Some(1024), 0, false),
                frame(Some(2040), -1024, true),
                frame(Some(2040), 0, false),
            ],
        )
        .into(),
    )
    .unwrap();
    assert_eq!(decoder.decode_frame(1).unwrap().rgba8[3], 128);
    assert_eq!(decoder.decode_frame(2).unwrap().rgba8[3], 255);
    assert_eq!(decoder.decode_frame(1).unwrap().rgba8[3], 128);
}

#[test]
fn initial_inheritance_and_incomplete_tiles_are_rejected() {
    let mut decoder = CmvDecoder::new(container(false, &[frame(None, 0, true)]).into()).unwrap();
    assert!(decoder.decode_frame(0).is_err());
    let mut payload = frame(None, 0, false);
    let list_header = 48 + 144 + 128;
    payload[list_header + 4] = 0;
    let mut decoder = CmvDecoder::new(container(false, &[payload]).into()).unwrap();
    assert!(decoder.decode_frame(0).is_err());
}

#[test]
fn malformed_indexes_rates_and_frame_ranges_are_rejected() {
    let original = container(false, &[frame(None, 0, false)]);
    for (offset, value) in [
        (4, 64),
        (24, 0),
        (28, 0),
        (44, 1),
        (48, u32::MAX),
        (60, u32::MAX),
    ] {
        let mut malformed = original.clone();
        put32(&mut malformed, offset, value);
        assert!(CmvDecoder::new(malformed.into()).is_err());
    }
    let mut decoder = CmvDecoder::new(original.into()).unwrap();
    assert!(decoder.decode_frame(1).is_err());
}

#[test]
fn truncated_bits_and_alpha_dc_count_fail_without_publishing_a_frame() {
    let mut payload = frame(Some(2040), 0, false);
    let count_offset = 48 + 144 + 192;
    put32(&mut payload, count_offset, 6);
    let mut decoder = CmvDecoder::new(container(true, &[payload]).into()).unwrap();
    assert!(decoder.decode_frame(0).is_err());
    let mut payload = frame(None, 0, false);
    put32(&mut payload, 28, u32::MAX);
    let mut decoder = CmvDecoder::new(container(false, &[payload]).into()).unwrap();
    assert!(decoder.decode_frame(0).is_err());
    assert!(decoder.current.is_none());
}

#[test]
fn ac_run_selector_and_run_length_are_bounded() {
    use crate::jbp::{HuffmanTree, JBitStream};
    let mut frequencies = [0; 31];
    frequencies[..16].fill(1);
    let tree = HuffmanTree::new([1; 16], frequencies).unwrap();
    let mut block = [0; 64];
    // Symbol zero followed by sixteen one bits never selects a valid run.
    let mut bits = JBitStream::new(&[0xf0, 0xff, 0x0f], 0, 3, "CMV_TEST").unwrap();
    assert!(jbpd::decode_ac(&tree, &mut bits, &[1; 16], &mut block).is_err());
    let mut bits = JBitStream::new(&[0], 0, 1, "CMV_TEST").unwrap();
    assert!(jbpd::decode_ac(&tree, &mut bits, &[64; 16], &mut block).is_err());
}
