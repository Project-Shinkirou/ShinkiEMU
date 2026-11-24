use crate::sob::word;
use std::{io, ops::Range};

#[derive(Clone, Copy)]
pub struct AnimationFrame {
    pub image: usize,
    pub milliseconds: u32,
    pub x: i16,
    pub y: i16,
}

pub struct Lag {
    data: Vec<u8>,
    sequences: Vec<Vec<AnimationFrame>>,
    images: Vec<Range<usize>>,
}

impl Lag {
    pub fn parse(data: Vec<u8>) -> io::Result<Self> {
        if !data.starts_with(b"LAG\0") {
            return Err(invalid());
        }
        let sequence_count = half(&data, 10).ok_or_else(invalid)? as usize;
        let extra = word(&data, 16).ok_or_else(invalid)? as usize;
        let timeline = 20usize.checked_add(extra).ok_or_else(invalid)?;
        let frames = word(&data, timeline).ok_or_else(invalid)? as usize;
        let timeline = timeline + 4;
        let bytes = frames.checked_mul(16).ok_or_else(invalid)?;
        let (records, image_table) = data
            .get(timeline..)
            .and_then(|data| data.split_at_checked(bytes))
            .ok_or_else(invalid)?;
        let images = word(image_table, 0).ok_or_else(invalid)? as usize;
        let table_size = images.checked_mul(12).ok_or_else(invalid)?;
        let image_records = image_table
            .get(4..)
            .and_then(|table| table.get(..table_size))
            .ok_or_else(invalid)?;
        let images = image_records
            .chunks_exact(12)
            .map(|record| {
                let size = word(record, 0).unwrap() as usize;
                let offset = word(record, 4).unwrap() as usize;
                let limit = offset
                    .checked_add(size)
                    .filter(|&end| end <= data.len())
                    .ok_or_else(invalid)?;
                Ok(offset..limit)
            })
            .collect::<io::Result<Vec<_>>>()?;
        if sequence_count == 0 || sequence_count > frames + 1 {
            return Err(invalid());
        }
        let mut sequences = vec![Vec::new()];
        for record in records.chunks_exact(16) {
            let image = word(record, 0).unwrap();
            if image == u32::MAX {
                if sequences.len() == sequence_count {
                    break;
                }
                sequences.push(Vec::new());
                continue;
            }
            if image as usize >= images.len() {
                return Err(invalid());
            }
            sequences.last_mut().unwrap().push(AnimationFrame {
                image: image as usize,
                milliseconds: word(record, 4).unwrap(),
                x: half(record, 8).unwrap() as i16,
                y: half(record, 10).unwrap() as i16,
            });
        }
        if sequences.len() != sequence_count {
            return Err(invalid());
        }
        Ok(Self {
            data,
            sequences,
            images,
        })
    }

    pub fn sequence(&self, index: usize) -> Option<&[AnimationFrame]> {
        self.sequences.get(index).map(Vec::as_slice)
    }

    pub fn image(&self, index: usize) -> Option<&[u8]> {
        self.images
            .get(index)
            .map(|range| &self.data[range.clone()])
    }
}

fn half(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(*bytes.get(offset..)?.first_chunk()?))
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen LAG animation")
}
