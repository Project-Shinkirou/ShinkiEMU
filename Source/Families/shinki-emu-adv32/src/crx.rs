//! Circus CRXG images. See the GARbro license notice for the format reference.
use crate::{
    Error, Result,
    binary::{dword, range, word},
};
use flate2::read::ZlibDecoder;
use image::RgbaImage;
#[cfg(not(target_env = "ohos"))]
use roukaku_media_core::{OwnedPixelBuffer, TextureFrame};
use std::{
    io::{Cursor, Read},
    sync::{Arc, OnceLock},
};

pub(crate) const MAX_PIXELS: usize = 8192 * 4096;

pub(crate) struct Crx {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) image: Arc<RgbaImage>,
    opaque: OnceLock<bool>,
    #[cfg(not(target_env = "ohos"))]
    texture: std::sync::OnceLock<TextureFrame>,
}

impl Crx {
    pub(crate) fn from_image(x: i32, y: i32, image: RgbaImage) -> Self {
        Self {
            x,
            y,
            image: Arc::new(image),
            opaque: OnceLock::new(),
            #[cfg(not(target_env = "ohos"))]
            texture: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn image_mut(&mut self) -> &mut RgbaImage {
        self.opaque.take();
        #[cfg(not(target_env = "ohos"))]
        self.texture.take();
        Arc::make_mut(&mut self.image)
    }

    #[cfg(not(target_env = "ohos"))]
    pub(crate) fn texture(&self) -> TextureFrame {
        self.texture
            .get_or_init(|| TextureFrame {
                width: self.image.width(),
                height: self.image.height(),
                rgba8: OwnedPixelBuffer::from_owned(
                    roukaku_byte_source::OwnedByteBuffer::from_owner(self.image.clone(), |image| {
                        image.as_raw().as_slice()
                    }),
                ),
            })
            .clone()
    }

    pub(crate) fn is_opaque(&self) -> bool {
        *self
            .opaque
            .get_or_init(|| self.image.pixels().all(|pixel| pixel[3] == 255))
    }

    pub(crate) fn decode(data: &[u8]) -> Result<Self> {
        if !data.starts_with(b"CRXG") {
            return Err(Error::Invalid("CRXG signature"));
        }
        let x = i32::from(word(data, 4)? as i16);
        let y = i32::from(word(data, 6)? as i16);
        let width = usize::from(word(data, 8)?);
        let height = usize::from(word(data, 10)?);
        let pixels = width * height;
        if pixels == 0 || pixels > MAX_PIXELS {
            return Err(Error::Invalid("CRXG dimensions"));
        }
        let compression = word(data, 12)?;
        let flags = word(data, 14)?;
        let depth = word(data, 16)?;
        let alpha_mode = word(data, 18)?;
        let channels = match depth {
            0 => 3,
            1 => 4,
            _ => 1,
        };
        let mut offset = 20;
        let palette = if channels == 1 {
            let entry_bytes = if depth == 0x102 { 4 } else { 3 };
            let length = usize::from(depth.min(256)) * entry_bytes;
            let palette = range(data, offset, length)?
                .chunks_exact(entry_bytes)
                .map(|entry| {
                    let (r, mut g, b) = (entry[0], entry[1], entry[2]);
                    if (r, g, b) == (255, 0, 255) {
                        g = 255;
                    }
                    [r, g, b, 255]
                })
                .collect::<Vec<_>>();
            offset += length;
            palette
        } else {
            Vec::new()
        };
        if compression == 3 {
            let records = dword(data, offset)? as usize;
            offset += 4;
            let length = records
                .checked_mul(16)
                .ok_or(Error::Invalid("CRXG metadata"))?;
            range(data, offset, length)?;
            offset += length;
        }
        let packed = if flags & 0x10 == 0 {
            data.get(offset..).ok_or(Error::Invalid("CRXG data"))?
        } else {
            let length = dword(data, offset)? as usize;
            let start = offset + 4;
            if length == data.len() {
                // Numbered packs may store the complete member length here.
                data.get(start..).ok_or(Error::Invalid("CRXG data"))?
            } else {
                range(data, start, length)?
            }
        };
        let row_length = width * channels;
        let bytes = match compression {
            1 => unpack_lz(packed, row_length.next_multiple_of(4) * height)?,
            2 | 3 => unpack_rows(packed, width, height, channels)?,
            _ => return Err(Error::Unsupported("CRXG compression")),
        };
        let stride = if compression == 1 {
            row_length.next_multiple_of(4)
        } else {
            row_length
        };
        let mut rgba = Vec::with_capacity(pixels * 4);
        for row in bytes.chunks_exact(stride) {
            for pixel in row[..row_length].chunks_exact(channels) {
                let color = match channels {
                    1 => *palette
                        .get(usize::from(pixel[0]))
                        .ok_or(Error::Invalid("CRXG palette index"))?,
                    3 => [pixel[2], pixel[1], pixel[0], 255],
                    4 if alpha_mode == 1 => [pixel[2], pixel[1], pixel[0], pixel[3]],
                    4 => {
                        let mask = if alpha_mode == 2 { 0 } else { 255 };
                        [pixel[3], pixel[2], pixel[1], pixel[0] ^ mask]
                    }
                    _ => unreachable!(),
                };
                rgba.extend_from_slice(&color);
            }
        }
        let image = RgbaImage::from_raw(width as u32, height as u32, rgba)
            .ok_or(Error::Invalid("CRXG raster"))?;
        Ok(Self::from_image(x, y, image))
    }
}

fn byte(input: &mut impl Read) -> Result<u8> {
    let mut buf = [0];
    input.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn little_word(input: &mut impl Read) -> Result<usize> {
    let mut buf = [0; 2];
    input.read_exact(&mut buf)?;
    Ok(usize::from(u16::from_le_bytes(buf)))
}

fn unpack_lz(packed: &[u8], size: usize) -> Result<Vec<u8>> {
    let mut data = Vec::with_capacity(size);
    let mut input = Cursor::new(packed);
    while data.len() < size {
        let flags = byte(&mut input)?;
        for bit in 0..8 {
            if data.len() == size {
                break;
            }
            if flags & (1 << bit) != 0 {
                data.push(byte(&mut input)?);
                continue;
            }
            let code = usize::from(byte(&mut input)?);
            let (count, distance) = match code {
                0xc0.. => (
                    4 + ((code >> 2) & 0xf),
                    ((code & 3) << 8) | usize::from(byte(&mut input)?),
                ),
                0x80.. => {
                    let distance = code & 31;
                    let distance = if distance == 0 {
                        usize::from(byte(&mut input)?)
                    } else {
                        distance
                    };
                    (2 + ((code >> 5) & 3), distance)
                }
                0x7f => (2 + little_word(&mut input)?, little_word(&mut input)?),
                _ => (code + 4, little_word(&mut input)?),
            };
            for _ in 0..count.min(size - data.len()) {
                // ADV32 starts with a zero-filled 64 KiB ring.
                let value = if distance == 0 {
                    data.len().checked_sub(65536)
                } else {
                    data.len().checked_sub(distance)
                }
                .map_or(0, |index| data[index]);
                data.push(value);
            }
        }
    }
    Ok(data)
}

fn unpack_rows(packed: &[u8], width: usize, height: usize, channels: usize) -> Result<Vec<u8>> {
    let row_length = width * channels;
    let mut input = ZlibDecoder::new(packed);
    let mut data = vec![0u8; height * row_length];
    if channels == 1 {
        input.read_exact(&mut data)?;
        return Ok(data);
    }
    for y in 0..height {
        let filter = byte(&mut input)?;
        let start = y * row_length;
        let (above, remaining) = data.split_at_mut(start);
        let row = &mut remaining[..row_length];
        if filter == 4 {
            decode_runs(&mut input, row, channels)?;
            continue;
        }
        if filter > 3 || (y == 0 && filter != 0) {
            return Err(Error::Invalid("CRXG row filter"));
        }
        input.read_exact(row)?;
        let previous = above
            .get(start.saturating_sub(row_length)..start)
            .unwrap_or_default();
        for x in 0..row_length {
            let prediction = match filter {
                0 if x >= channels => row[x - channels],
                1 => previous[x],
                2 if x >= channels => previous[x - channels],
                3 if x < row_length - channels => previous[x + channels],
                _ => 0,
            };
            row[x] = row[x].wrapping_add(prediction);
        }
    }
    Ok(data)
}

fn decode_runs(input: &mut impl Read, row: &mut [u8], channels: usize) -> Result<()> {
    let width = row.len() / channels;
    for channel in 0..channels {
        let mut x = 0;
        let mut value = byte(input)?;
        while x < width {
            row[x * channels + channel] = value;
            x += 1;
            if x == width {
                break;
            }
            let next = byte(input)?;
            if value != next {
                value = next;
                continue;
            }
            let count = usize::from(byte(input)?);
            if count > width - x {
                return Err(Error::Invalid("CRXG row run"));
            }
            for repeat in x..x + count {
                row[repeat * channels + channel] = value;
            }
            x += count;
            if x < width {
                value = byte(input)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn decodes_a_numbered_pack_member_with_total_length_in_the_header() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&[0, 17, 34, 51]).unwrap();
        let payload = encoder.finish().unwrap();
        let mut crx = b"CRXG".to_vec();
        for word in [0u16, 0, 1, 1, 3, 0x10, 0, 0] {
            crx.extend_from_slice(&word.to_le_bytes());
        }
        crx.extend_from_slice(&0u32.to_le_bytes());
        crx.extend_from_slice(&((28 + payload.len()) as u32).to_le_bytes());
        crx.extend_from_slice(&payload);
        assert_eq!(
            Crx::decode(&crx).unwrap().image.get_pixel(0, 0).0,
            [51, 34, 17, 255]
        );
    }
}
