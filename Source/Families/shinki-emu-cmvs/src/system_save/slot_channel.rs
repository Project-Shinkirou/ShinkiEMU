use shinki_emu_sdk::CoreError;

use super::slot_image::SavedImage;
use super::slot_io::{Reader, invalid};

/// `466B10`: the serialized 0x9c0-byte channel prefix, followed by its CImage.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedChannel {
    pub(crate) id: u16,
    pub(crate) playback: [u32; 2],
    pub(crate) words: [u16; 1248],
    pub(crate) image: SavedImage,
}

impl SavedChannel {
    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut reader = Reader::new(payload);
        let id = reader.u16()?;
        if reader.u16()? != 3 {
            return Err(invalid("unsupported native channel descriptor version"));
        }
        let playback = [reader.u32()?, reader.u32()?];
        let mut words = [0; 1248];
        for word in &mut words {
            *word = reader.u16()?;
        }
        if words[0] != id {
            return Err(invalid(
                "native channel id disagrees with its object prefix",
            ));
        }
        let length = reader.u32()? as usize;
        let image = SavedImage::decode(reader.bytes(length)?)?;
        reader.finish()?;
        Ok(Self {
            id,
            playback,
            words,
            image,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        if self.words[0] != self.id {
            return Err(invalid(
                "native channel id disagrees with its object prefix",
            ));
        }
        let image = self.image.encode()?;
        let length =
            u32::try_from(image.len()).map_err(|_| invalid("native channel image is too large"))?;
        let mut bytes = Vec::with_capacity(2512 + image.len());
        bytes.extend_from_slice(&self.id.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        for word in self.playback {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for word in self.words {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&image);
        Ok(bytes)
    }
}

/// `462E70` explicitly copies these three disjoint native object regions.
/// They are wire DWORDs, not the layout of a Rust text object. Unknown values
/// survive decode/encode; the scene consumer rejects unsupported semantics.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SavedText {
    pub(crate) capacity: u32,
    pub(crate) font: Vec<u8>,
    pub(crate) virtual_size: [u32; 2],
    /// Native object bytes 4..160 (DWORD1..39).
    pub(crate) parameters: [u32; 39],
    /// Native object bytes 164..312 (DWORD41..77).
    pub(crate) layout: [u32; 37],
    /// Native object bytes 316..524 (DWORD79..130).
    pub(crate) extended: [u32; 52],
    /// Saved native node DWORD1 followed by the 40-byte glyph descriptor.
    /// DWORD1 is a process-local basic pointer; `464AB0` ignores it.
    pub(crate) glyphs: Vec<[u32; 11]>,
}

impl SavedText {
    pub(crate) fn decode(payload: &[u8]) -> Result<Self, CoreError> {
        let mut reader = Reader::new(payload);
        if reader.u16()? != 3 {
            return Err(invalid("unsupported native text descriptor version"));
        }
        let capacity = reader.u32()?;
        let font = reader.c_string(64)?.to_vec();
        let virtual_size = [reader.u32()?, reader.u32()?];
        let mut parameters = [0; 39];
        let mut layout = [0; 37];
        let mut extended = [0; 52];
        for word in &mut parameters {
            *word = reader.u32()?;
        }
        for word in &mut layout {
            *word = reader.u32()?;
        }
        for word in &mut extended {
            *word = reader.u32()?;
        }
        let count = usize::from(reader.u16()?);
        if reader.remaining() != count * 44 {
            return Err(invalid(
                "native text glyph count disagrees with its payload",
            ));
        }
        let mut glyphs = Vec::with_capacity(count);
        for _ in 0..count {
            let mut glyph = [0; 11];
            for word in &mut glyph {
                *word = reader.u32()?;
            }
            glyphs.push(glyph);
        }
        reader.finish()?;
        Ok(Self {
            capacity,
            font,
            virtual_size,
            parameters,
            layout,
            extended,
            glyphs,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        if self.font.len() >= 64 || self.font.contains(&0) {
            return Err(invalid("native text font exceeds its buffer"));
        }
        let count = u16::try_from(self.glyphs.len())
            .map_err(|_| invalid("native text has too many glyphs"))?;
        let mut bytes = Vec::with_capacity(529 + self.font.len() + self.glyphs.len() * 44);
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&self.capacity.to_le_bytes());
        bytes.extend_from_slice(&self.font);
        bytes.push(0);
        for word in self.virtual_size {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for word in self
            .parameters
            .iter()
            .chain(&self.layout)
            .chain(&self.extended)
        {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&count.to_le_bytes());
        for glyph in &self.glyphs {
            for word in glyph {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_466b10_channel_prefix_embedded_image_and_length_fixture() {
        // `445160`: version4, empty container, eleven effect words, F0F0.
        let mut bytes = vec![0; 2512 + 70];
        bytes[0..2].copy_from_slice(&3u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&3u16.to_le_bytes());
        bytes[4..8].copy_from_slice(&1280u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&720u32.to_le_bytes());
        bytes[12..14].copy_from_slice(&3u16.to_le_bytes());
        bytes[40..42].copy_from_slice(&2u16.to_le_bytes());
        bytes[2508..2512].copy_from_slice(&70u32.to_le_bytes());
        bytes[2512..2516].copy_from_slice(&0x0004_ffffu32.to_le_bytes());
        bytes[2524..2528].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[2580..2582].copy_from_slice(&0xf0f0u16.to_le_bytes());
        let saved = SavedChannel::decode(&bytes).unwrap();
        assert_eq!(saved.id, 3);
        assert_eq!(saved.words[14], 2);
        assert_eq!(saved.playback, [1280, 720]);
        assert_eq!(saved.encode().unwrap(), bytes);
        bytes[2508..2512].copy_from_slice(&69u32.to_le_bytes());
        assert!(SavedChannel::decode(&bytes).is_err());
    }

    #[test]
    fn native_462e70_disjoint_regions_and_cp932_glyph_fixture() {
        // Hand-assembled from the native writer offsets, independent of encode.
        let mut bytes = vec![0; 531 + 44];
        bytes[0..2].copy_from_slice(&3u16.to_le_bytes());
        bytes[2..6].copy_from_slice(&196u32.to_le_bytes());
        bytes[6..9].copy_from_slice(b"JP\0");
        bytes[9..13].copy_from_slice(&1280u32.to_le_bytes());
        bytes[13..17].copy_from_slice(&720u32.to_le_bytes());
        bytes[17..21].copy_from_slice(&31u32.to_le_bytes());
        bytes[173..177].copy_from_slice(&148u32.to_le_bytes());
        bytes[321..325].copy_from_slice(&1u32.to_le_bytes());
        bytes[529..531].copy_from_slice(&1u16.to_le_bytes());
        bytes[535..539].copy_from_slice(&((0x8abfu32 << 16) | 40).to_le_bytes());
        let saved = SavedText::decode(&bytes).unwrap();
        assert_eq!(saved.parameters[0], 31);
        assert_eq!(saved.layout[0], 148);
        assert_eq!(saved.extended[0], 1);
        assert_eq!(saved.glyphs[0][1] >> 16, 0x8abf);
        assert_eq!(saved.encode().unwrap(), bytes);
        assert!(SavedText::decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
