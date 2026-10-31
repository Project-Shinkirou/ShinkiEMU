use crate::{eden_save::EdenEdition, scene::error};
use roukaku_media_core::{Extent2D, TextureFrame};
use shinki_emu_sdk::{StageCanvas, runtime::SessionResult};
use std::{io::Cursor, time::SystemTime};

const MAX_CAPTURE_BYTES: usize = 128 * 1024 * 1024;

fn dimensions(edition: EdenEdition) -> (u32, u32) {
    match edition {
        EdenEdition::Perseus => (106, 60),
        EdenEdition::Japanese | EdenEdition::English => (128, 80),
    }
}

#[derive(Clone)]
pub(crate) struct SaveCard {
    pub timestamp: String,
    pub comment: String,
    pub thumbnail: TextureFrame,
}
impl SaveCard {
    pub fn capture_thumbnail(
        edition: EdenEdition,
        logical_width: u32,
        logical_height: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> SessionResult<Vec<u8>> {
        let fail = || {
            error(
                "SHINKI_EMU_MUSICA_SAVE_THUMBNAIL",
                "gameplay thumbnail could not be encoded",
            )
        };
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(height as usize))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(fail)?;
        if expected > MAX_CAPTURE_BYTES || rgba.len() != expected {
            return Err(fail());
        }
        let canvas = StageCanvas::new(
            Extent2D::new(logical_width, logical_height),
            Extent2D::new(width, height),
        )
        .map_err(|_| fail())?;
        let viewport = canvas.viewport();
        if viewport
            .x
            .checked_add(viewport.width)
            .is_none_or(|right| right > width)
            || viewport
                .y
                .checked_add(viewport.height)
                .is_none_or(|bottom| bottom > height)
            || viewport.width == 0
            || viewport.height == 0
        {
            return Err(fail());
        }
        let (thumb_width, thumb_height) = dimensions(edition);
        let mut image = image::RgbaImage::new(thumb_width, thumb_height);
        for y in 0..thumb_height {
            let start_y = viewport.y + y * viewport.height / thumb_height;
            let end_y = viewport.y + (y + 1) * viewport.height / thumb_height;
            for x in 0..thumb_width {
                let start_x = viewport.x + x * viewport.width / thumb_width;
                let end_x = viewport.x + (x + 1) * viewport.width / thumb_width;
                let mut channels = [0u64; 3];
                let mut count = 0u64;
                for row in start_y..end_y.max(start_y + 1) {
                    for col in start_x..end_x.max(start_x + 1) {
                        let index = (row as usize * width as usize + col as usize) * 4;
                        for (sum, component) in channels.iter_mut().zip(&rgba[index..index + 3]) {
                            *sum += u64::from(*component);
                        }
                        count += 1;
                    }
                }
                image.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        (channels[0] / count) as u8,
                        (channels[1] / count) as u8,
                        (channels[2] / count) as u8,
                        255,
                    ]),
                );
            }
        }
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| fail())?;
        Ok(png.into_inner())
    }
    pub fn from_native(
        edition: EdenEdition,
        comment: String,
        thumbnail_png: Vec<u8>,
        modified: SystemTime,
    ) -> SessionResult<Self> {
        let time = time::OffsetDateTime::from(modified);
        let offset = time::UtcOffset::current_local_offset().map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_TIMESTAMP",
                "local time is unavailable",
            )
        })?;
        let now = time.to_offset(offset);
        Ok(Self {
            timestamp: format!(
                "{:04}/{:02}/{:02} {:02}:{:02}",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute()
            ),
            comment,
            thumbnail: decode_thumbnail(edition, &thumbnail_png)?,
        })
    }
}

fn decode_thumbnail(edition: EdenEdition, png: &[u8]) -> SessionResult<TextureFrame> {
    let fail = || {
        error(
            "SHINKI_EMU_MUSICA_SAVE_THUMBNAIL",
            "native thumbnail is invalid",
        )
    };
    if png.is_empty() || png.len() > 1024 * 1024 {
        return Err(fail());
    }
    let (width, height) = dimensions(edition);
    let mut reader = image::ImageReader::with_format(Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| fail())?;
    if (image.width(), image.height()) != (width, height) {
        return Err(fail());
    }
    Ok(TextureFrame {
        width,
        height,
        rgba8: image.into_rgba8().into_raw().into(),
    })
}
