//! Lucifen ELG image decoding. Format reference: GARbro ImageELG.cs (morkt, MIT).

use std::io;

pub struct ElgImage {
    pub width: u32,
    pub height: u32,
    pub origin: (i16, i16),
    pub rgba: Vec<u8>,
    pub indices: Option<Vec<u8>>,
}

pub fn decode_elg(bytes: &[u8]) -> io::Result<ElgImage> {
    let mut input = Bytes(bytes.strip_prefix(b"ELG").ok_or_else(invalid)?);
    let variant = input.byte()?;
    let depth = if matches!(variant, 1 | 2) {
        input.byte()?
    } else {
        variant
    };
    let mut origin = (0, 0);
    if variant == 1 {
        origin = input.origin()?;
    }
    let width = usize::from(input.u16()?);
    let height = usize::from(input.u16()?);
    let pixels = width
        .checked_mul(height)
        .filter(|&n| (1..=16_777_216).contains(&n))
        .ok_or_else(invalid)?;
    if variant == 2 {
        origin = input.origin()?;
        while input.byte()? != 0 {
            let length = u32::from_le_bytes(input.take(4)?.try_into().unwrap()) as usize;
            input.take(length.checked_sub(4).ok_or_else(invalid)?)?;
        }
    }
    let mut rgba = Vec::with_capacity(pixels * 4);
    let indices = match depth {
        8 => {
            let palette = decode_plane(&mut input, 1024, 1, 0)?;
            let indices = decode_plane(&mut input, pixels, 1, 0)?;
            for &index in &indices {
                let pixel = &palette[usize::from(index) * 4..][..3];
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
            }
            Some(indices)
        }
        24 | 32 => {
            let rgb = decode_plane(&mut input, pixels, 3, width)?;
            if depth == 32 {
                let alpha = decode_plane(&mut input, pixels, 1, 0)?;
                for (pixel, alpha) in rgb.chunks_exact(3).zip(alpha) {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
                }
            } else {
                for pixel in rgb.chunks_exact(3) {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
            }
            None
        }
        _ => return Err(invalid()),
    };
    Ok(ElgImage {
        width: width as u32,
        height: height as u32,
        origin,
        rgba,
        indices,
    })
}

struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let (prefix, tail) = self.0.split_at_checked(count).ok_or_else(invalid)?;
        self.0 = tail;
        Ok(prefix)
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn origin(&mut self) -> io::Result<(i16, i16)> {
        Ok((self.u16()? as i16, self.u16()? as i16))
    }

    fn extended(&mut self, high: u8) -> io::Result<usize> {
        Ok((usize::from(high) << 8) | usize::from(self.byte()?))
    }

    fn count(&mut self, flags: u8, minimum: usize) -> io::Result<usize> {
        let low = flags & 31;
        Ok(if flags & 32 == 0 {
            usize::from(low) + minimum
        } else {
            self.extended(low)? + 32 + minimum
        })
    }
}

fn decode_plane(
    input: &mut Bytes<'_>,
    pixels: usize,
    stride: usize,
    width: usize,
) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(pixels * stride);
    let color = stride == 3;
    loop {
        let flags = input.byte()?;
        if flags == 255 {
            if out.len() != pixels * stride {
                return Err(invalid());
            }
            return Ok(out);
        }
        let used = out.len() / stride;
        match flags >> 6 {
            0 => {
                let count = input.count(flags, 1)?;
                check_span(used, count, pixels)?;
                out.extend_from_slice(input.take(count * stride)?);
            }
            1 => {
                let count = input.count(flags, if color { 2 } else { 3 })?;
                check_span(used, count, pixels)?;
                let pixel = input.take(stride)?;
                for _ in 0..count {
                    out.extend_from_slice(pixel);
                }
            }
            _ => {
                let (distance, count) = if color {
                    color_copy(input, flags, width)?
                } else {
                    byte_copy(input, flags)?
                };
                check_span(used, count, pixels)?;
                if distance == 0 || used < distance {
                    return Err(invalid());
                }
                let distance = distance * stride;
                for _ in 0..count * stride {
                    out.push(out[out.len() - distance]);
                }
            }
        }
    }
}

fn check_span(start: usize, count: usize, length: usize) -> io::Result<()> {
    if count > length - start {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn byte_copy(input: &mut Bytes<'_>, flags: u8) -> io::Result<(usize, usize)> {
    let short = usize::from(flags & 15);
    Ok(match flags & 0xf0 {
        0x80 => (usize::from(input.byte()?) + 2, short + 2),
        0x90 => (
            input.extended(flags & 15)? + 3,
            usize::from(input.byte()?) + 4,
        ),
        0xa0 => (input.extended(flags & 15)? + 3, 3),
        0xb0 => (input.extended(flags & 15)? + 3, 4),
        _ if flags & 32 != 0 => (usize::from(flags & 31) + 2, 2),
        _ => (usize::from(flags & 31) + 1, 1),
    })
}

fn color_copy(input: &mut Bytes<'_>, flags: u8, width: usize) -> io::Result<(usize, usize)> {
    let short = flags & 15;
    let distance = match flags & 0xf0 {
        0x80 => return Ok((usize::from(input.byte()?) + 2, usize::from(short) + 1)),
        0x90 => return Ok((input.extended(short)? + 2, usize::from(input.byte()?) + 1)),
        0xa0 => {
            let upper = input.extended(short)?;
            return Ok((
                (upper << 8) + usize::from(input.byte()?) + 4098,
                usize::from(input.byte()?) + 1,
            ));
        }
        0xb0 if flags & 8 == 0 => usize::from(flags & 7) + 2,
        0xb0 => input.extended(flags & 7)? + 10,
        0xc0 => {
            let extended = input.extended(flags & 3)?;
            match flags & 12 {
                0 => width * (extended + 16),
                4 => width * (extended + 16) + 1,
                8 => width * (extended + 16) - 1,
                _ => extended + 2058,
            }
        }
        0xd0 => width * (usize::from(short) + 1),
        0xe0 => width * (usize::from(short) + 1) + 1,
        _ => width * (usize::from(short) + 1) - 1,
    };
    Ok((distance, 1))
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen ELG image")
}
