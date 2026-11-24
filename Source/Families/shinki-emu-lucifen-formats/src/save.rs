use crate::{lpk::lzss, sob::word};
use std::io;

pub const THUMBNAIL_WIDTH: usize = 100;
pub const THUMBNAIL_HEIGHT: usize = 75;
const THUMBNAIL_BYTES: usize = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT * 3;
const CHAPTER_BYTES: usize = 44;
const SUMMARY_BYTES: usize = 144;
const RECORD_BYTES: usize = 8 + THUMBNAIL_BYTES + CHAPTER_BYTES + SUMMARY_BYTES;
const MAX_STATE: usize = 64 * 1024 * 1024;

pub struct SaveSlotPreview<'a> {
    pub thumbnail_bgr: &'a [u8],
    pub chapter: &'a [u8; CHAPTER_BYTES],
    pub summary: &'a [u8; SUMMARY_BYTES],
}

impl<'a> SaveSlotPreview<'a> {
    pub const SIZE: usize = RECORD_BYTES;

    pub fn decode(bytes: &'a [u8]) -> io::Result<Self> {
        let record = bytes.get(..RECORD_BYTES).ok_or_else(invalid)?;
        let chapter_at = 8 + THUMBNAIL_BYTES;
        let summary_at = chapter_at + CHAPTER_BYTES;
        Ok(Self {
            thumbnail_bgr: &record[8..chapter_at],
            chapter: record[chapter_at..summary_at].try_into().unwrap(),
            summary: record[summary_at..].try_into().unwrap(),
        })
    }
}

/// Native Lucifen slot header and decoded state. The slot signature belongs
/// to the game installation and must come from the original save identity.
pub struct SaveSlot {
    pub signature: [u8; 4],
    pub thumbnail_bgr: Vec<u8>,
    pub chapter: [u8; CHAPTER_BYTES],
    pub summary: [u8; SUMMARY_BYTES],
    pub state: Vec<u8>,
}

impl SaveSlot {
    pub fn decode(bytes: &[u8], signature: [u8; 4]) -> io::Result<Self> {
        let (record, size_and_data) = bytes.split_at_checked(RECORD_BYTES).ok_or_else(invalid)?;
        if record[..4] != signature || record[4..8] != [0; 4] {
            return Err(invalid());
        }
        let stored = word(size_and_data, 0).ok_or_else(invalid)? as usize;
        let expanded = word(size_and_data, 4).ok_or_else(invalid)? as usize;
        let checksum = *size_and_data.get(8).ok_or_else(invalid)?;
        let input = size_and_data
            .get(9..)
            .filter(|data| data.len() == stored)
            .ok_or_else(invalid)?;
        if stored > MAX_STATE || expanded > MAX_STATE || checksum != sum(input) {
            return Err(invalid());
        }
        let state = if expanded == 0 {
            input.to_vec()
        } else {
            let mut state = Vec::with_capacity(expanded);
            lzss(input, expanded, &mut state)?;
            state
        };
        let preview = SaveSlotPreview::decode(record)?;
        Ok(Self {
            signature,
            thumbnail_bgr: preview.thumbnail_bgr.to_vec(),
            chapter: *preview.chapter,
            summary: *preview.summary,
            state,
        })
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.thumbnail_bgr.len() != THUMBNAIL_BYTES || self.state.len() > MAX_STATE {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(RECORD_BYTES + 9 + self.state.len());
        bytes.extend_from_slice(&self.signature);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&self.thumbnail_bgr);
        bytes.extend_from_slice(&self.chapter);
        bytes.extend_from_slice(&self.summary);
        bytes.extend_from_slice(&(self.state.len() as u32).to_le_bytes());
        // The original loader accepts native uncompressed state with a zero
        // expanded size. This avoids a second LZSS algorithm for save writes.
        bytes.extend_from_slice(&[0; 4]);
        bytes.push(sum(&self.state));
        bytes.extend_from_slice(&self.state);
        Ok(bytes)
    }
}

fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().copied().fold(0, u8::wrapping_add)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen save slot")
}
