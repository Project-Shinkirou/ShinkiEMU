//! Native CMV6/CMV8 auxiliary video (CStreamVideo), independent of MGV movies.
mod jbpd;
#[cfg(test)]
pub(crate) mod tests;

use roukaku_byte_source::OwnedByteBuffer;
use roukaku_media_core::TextureFrame;
use shinki_emu_sdk::CoreError;
use std::ops::Range;

const MAX_PIXELS: usize = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
const INDEX_START: usize = 44;
const INDEX_BYTES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmvMetadata {
    pub width: u32,
    pub height: u32,
    pub frame_count: u32,
    pub rate_num: u32,
    pub rate_den: u32,
    pub has_alpha: bool,
}

/// Retains one decoded frame. Returned textures share its pixels until the next
/// real update; backwards seeks rebuild all predecessor deltas from frame zero.
pub struct CmvDecoder {
    source: OwnedByteBuffer,
    metadata: CmvMetadata,
    data_base: usize,
    current: Option<u32>,
    frame: TextureFrame,
}

impl CmvDecoder {
    pub fn new(source: OwnedByteBuffer) -> Result<Self, CoreError> {
        let bytes = source.as_slice();
        range(bytes, 0, 64)?;
        let has_alpha = match &bytes[..4] {
            b"CMV6" => false,
            b"CMV8" => true,
            _ => return Err(invalid("CMV version is unsupported")),
        };
        let metadata = CmvMetadata {
            frame_count: u32_at(bytes, 16)?,
            rate_num: u32_at(bytes, 20)?,
            rate_den: u32_at(bytes, 24)?,
            width: u32_at(bytes, 28)?,
            height: u32_at(bytes, 32)?,
            has_alpha,
        };
        if metadata.frame_count == 0 || metadata.rate_num == 0 || metadata.rate_den == 0 {
            return Err(invalid("CMV frame count or rate is zero"));
        }
        if u32_at(bytes, 36)? != if has_alpha { 32 } else { 24 } {
            return Err(invalid("CMV pixel depth disagrees with its version"));
        }
        let output_bytes = pixel_bytes(metadata.width, metadata.height)?;
        // The native index begins inside the 64-byte first-record header;
        // CStreamVideo reserves a further 20 bytes after the last record.
        let default_base = (metadata.frame_count as usize)
            .checked_mul(INDEX_BYTES)
            .and_then(|size| size.checked_add(64))
            .ok_or_else(|| invalid("CMV frame index overflowed"))?;
        range(bytes, 0, default_base)?;
        let declared_base = u32_at(bytes, 4)? as usize;
        let data_base = if declared_base == 0 {
            default_base
        } else {
            declared_base
        };
        if data_base < default_base || data_base > bytes.len() {
            return Err(invalid("CMV frame data overlaps its index"));
        }
        let mut previous_end = data_base;
        for index in 0..metadata.frame_count {
            let record = INDEX_START + index as usize * INDEX_BYTES;
            if u32_at(bytes, record)? != index {
                return Err(invalid("CMV frame index is not sequential"));
            }
            let decoded = u32_at(bytes, record + 8)? as usize;
            if decoded > MAX_DECODED_BYTES {
                return Err(invalid("CMV declared decoded frame exceeds budget"));
            }
            let payload = frame_range(bytes, data_base, index)?;
            if payload.start < previous_end {
                return Err(invalid("CMV frame payloads overlap or run backwards"));
            }
            previous_end = payload.end;
        }
        Ok(Self {
            source,
            metadata,
            data_base,
            current: None,
            frame: TextureFrame {
                width: metadata.width,
                height: metadata.height,
                rgba8: vec![0; output_bytes].into(),
            },
        })
    }

    pub fn metadata(&self) -> CmvMetadata {
        self.metadata
    }

    pub fn decode_frame(&mut self, index: u32) -> Result<TextureFrame, CoreError> {
        if index >= self.metadata.frame_count {
            return Err(invalid("CMV requested frame is outside its index"));
        }
        if self.current == Some(index) {
            return Ok(self.frame.clone());
        }
        let start = match self.current {
            Some(current) if current < index => current + 1,
            _ => 0,
        };
        // A failed update invalidates the partially written retained surface.
        // The next request must replay a complete keyframe before any delta.
        self.current = None;
        for next in start..=index {
            let payload = frame_range(self.source.as_slice(), self.data_base, next)?;
            jbpd::decode(
                &self.source.as_slice()[payload],
                self.metadata,
                next != 0,
                &mut self.frame,
            )?;
        }
        self.current = Some(index);
        Ok(self.frame.clone())
    }
}

fn frame_range(source: &[u8], base: usize, index: u32) -> Result<Range<usize>, CoreError> {
    let record = INDEX_START + index as usize * INDEX_BYTES;
    let length = u32_at(source, record + 4)? as usize;
    let offset = base
        .checked_add(u32_at(source, record + 16)? as usize)
        .ok_or_else(|| invalid("CMV frame offset overflowed"))?;
    if length == 0 {
        return Err(invalid("CMV frame payload is empty"));
    }
    range(source, offset, length)?;
    Ok(offset..offset + length)
}

fn pixel_bytes(width: u32, height: u32) -> Result<usize, CoreError> {
    if width == 0 || height == 0 {
        return Err(invalid("CMV dimensions are zero"));
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .filter(|pixels| *pixels <= MAX_PIXELS)
        .ok_or_else(|| invalid("CMV pixel count exceeds budget"))?;
    pixels
        .checked_mul(4)
        .filter(|size| *size <= MAX_DECODED_BYTES)
        .ok_or_else(|| invalid("CMV decoded image exceeds budget"))
}

fn range(source: &[u8], start: usize, length: usize) -> Result<(), CoreError> {
    if start
        .checked_add(length)
        .is_none_or(|end| end > source.len())
    {
        return Err(invalid("CMV range exceeds its source"));
    }
    Ok(())
}

fn u16_at(source: &[u8], offset: usize) -> Result<u16, CoreError> {
    range(source, offset, 2)?;
    Ok(u16::from_le_bytes(
        source[offset..offset + 2]
            .try_into()
            .expect("bounded CMV u16"),
    ))
}

fn u32_at(source: &[u8], offset: usize) -> Result<u32, CoreError> {
    range(source, offset, 4)?;
    Ok(u32::from_le_bytes(
        source[offset..offset + 4]
            .try_into()
            .expect("bounded CMV u32"),
    ))
}

fn align4(value: usize) -> Result<usize, CoreError> {
    value
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| invalid("CMV alignment overflowed"))
}

fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_CMV", message)
}
