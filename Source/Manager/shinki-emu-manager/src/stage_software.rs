//! OHOS stage presentation: Slint's software renderer draws the stage as a
//! CPU image. Applies the same row order, channel order, linear-to-sRGB and
//! opaque-alpha rules as the WGPU pixel pass in `stage_pixels.rs`; final-frame
//! filters need WGPU compute and are unavailable.
use std::sync::LazyLock;

use roukaku_media_core::Viewport2D;
use shinki_emu_manager::StageImageUpdate;
use shinki_emu_manager_core::model::{FrameAlpha, FrameFormat, FrameIdentity, FrameRowOrder};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use super::FrameMailbox;

/// sRGB encoding of each linear 8-bit channel value.
static LINEAR_TO_SRGB: LazyLock<[u8; 256]> = LazyLock::new(|| {
    std::array::from_fn(|value| {
        let linear = value as f32 / 255.0;
        let encoded = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round().clamp(0.0, 255.0) as u8
    })
});

#[derive(Default)]
pub(super) struct SoftwarePresentation {
    identity: Option<FrameIdentity>,
    shown: Option<SharedPixelBuffer<Rgba8Pixel>>,
    /// The buffer of the previous frame; reused once Slint dropped its image.
    spare: Option<SharedPixelBuffer<Rgba8Pixel>>,
    geometry: Option<((u32, u32), Viewport2D)>,
}

impl SoftwarePresentation {
    pub(super) fn update(&mut self, mailbox: &FrameMailbox) -> Result<Option<StageImageUpdate>, String> {
        let Some(frame) = mailbox.snapshot()? else {
            if self.identity.take().is_none() {
                return Ok(None);
            }
            self.shown = None;
            self.spare = None;
            let ((logical_width, logical_height), content) =
                self.geometry.take().ok_or("SHINKI_EMU_HOST_FRAME_BASELINE")?;
            return Ok(Some(StageImageUpdate {
                image: Image::default(),
                logical_width,
                logical_height,
                raster_width: content.width,
                raster_height: content.height,
                content,
            }));
        };
        if Some(frame.identity) == self.identity {
            return Ok(None);
        }
        let info = frame.info;
        let (width, height) = (info.width as usize, info.height as usize);
        let source = frame.pixels().ok_or("SHINKI_EMU_HOST_FRAME_PIXELS")?;
        let required = info.required_bytes().ok_or("SHINKI_EMU_HOST_FRAME_SIZE")?;
        let source = source.get(..required).ok_or("SHINKI_EMU_HOST_FRAME_SIZE")?;
        let mut buffer = match self.spare.take() {
            Some(buffer) if buffer.width() == info.width && buffer.height() == info.height => buffer,
            _ => SharedPixelBuffer::new(info.width, info.height),
        };
        let bgra = matches!(info.format, FrameFormat::Bgra8Srgb { .. } | FrameFormat::Bgra8Linear { .. });
        let linear = matches!(info.format, FrameFormat::Rgba8Linear { .. } | FrameFormat::Bgra8Linear { .. });
        let opaque = info.format.alpha() == FrameAlpha::Ignored;
        let encode = &*LINEAR_TO_SRGB;
        let stride = info.stride as usize;
        let destination = buffer.make_mut_bytes();
        for (y, row) in destination.chunks_exact_mut(width * 4).take(height).enumerate() {
            let source_y = match info.rows {
                FrameRowOrder::TopToBottom => y,
                FrameRowOrder::BottomToTop => height - 1 - y,
            };
            let source_row = &source[source_y * stride..source_y * stride + width * 4];
            for (output, input) in row.chunks_exact_mut(4).zip(source_row.chunks_exact(4)) {
                let (r, g, b) = if bgra {
                    (input[2], input[1], input[0])
                } else {
                    (input[0], input[1], input[2])
                };
                let (r, g, b) = if linear {
                    (encode[usize::from(r)], encode[usize::from(g)], encode[usize::from(b)])
                } else {
                    (r, g, b)
                };
                output.copy_from_slice(&[r, g, b, if opaque { u8::MAX } else { input[3] }]);
            }
        }
        mailbox.presented(frame.identity, info)?;
        let image = Image::from_rgba8(buffer.clone());
        self.spare = self.shown.replace(buffer);
        self.identity = Some(frame.identity);
        let content = super::content_viewport_for_output(
            info.logical_width,
            info.logical_height,
            info.width,
            info.height,
            info.width,
            info.height,
        )?;
        self.geometry = Some(((info.logical_width, info.logical_height), content));
        Ok(Some(StageImageUpdate {
            image,
            logical_width: info.logical_width,
            logical_height: info.logical_height,
            raster_width: info.width,
            raster_height: info.height,
            content,
        }))
    }
}
