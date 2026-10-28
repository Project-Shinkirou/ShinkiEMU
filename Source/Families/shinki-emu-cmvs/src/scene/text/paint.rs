use super::*;
use crate::CmvsNativeTextGlyph;
use roukaku_media_core::{GlyphBitmap, GlyphBitmapFormat};

#[derive(Clone, Copy)]
pub(super) struct PaintStyle {
    pub style: u32,
    pub colors: [u32; 2],
    pub color_mode: u32,
    pub outline_color: u32,
}

impl From<&CmvsNativeTextGlyph> for PaintStyle {
    fn from(glyph: &CmvsNativeTextGlyph) -> Self {
        Self {
            style: glyph.style,
            colors: glyph.colors,
            color_mode: glyph.color_mode,
            outline_color: 0,
        }
    }
}

// Native styles 1/2 use a maximum-alpha diamond kernel, including attenuated
// corners. Repeated source-over glyph draws do not implement that operation.
fn outline(glyph: &GlyphBitmap, radius: u32, padding: u32, width: u32, pixels: &mut [u8]) {
    for y in 0..glyph.height {
        for x in 0..glyph.width {
            let alpha = glyph.pixels.as_slice()[(y * glyph.width + x) as usize];
            for dy in 0..=radius * 2 {
                for dx in 0..=radius * 2 {
                    let distance = dx.abs_diff(radius) + dy.abs_diff(radius);
                    let value = alpha >> distance.saturating_sub(radius);
                    let target_x = x + dx + padding - radius;
                    let target_y = y + dy + padding - radius;
                    if target_x < width
                        && let Some(target) =
                            pixels.get_mut(((target_y * width + target_x) * 4 + 3) as usize)
                    {
                        *target = (*target).max(value);
                    }
                }
            }
        }
    }
}

// 0x462040 paints into one BGRA cell before uploading it. In outlined styles,
// every nonzero fill sample becomes opaque after its RGB is mixed with the
// outline using a 256 denominator. Two source-over glyph draws are different.
pub(super) fn paint_upload(
    glyph: &GlyphBitmap,
    style: impl Into<PaintStyle>,
    pixel_size: u32,
) -> Result<(GlyphBitmap, [i32; 2]), CoreError> {
    let (width, height, mut pixels) = paint_buffer(glyph, style.into(), [0, 0], pixel_size, 2)?;
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] != 0 {
            let (x, y) = (index as u32 % width, index as u32 / width);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    if right == 0 {
        pixels.truncate(4);
        pixels.fill(0);
        return GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Rgba8, pixels)
            .map(|bitmap| (bitmap, [0, 0]))
            .map_err(|_| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"));
    }
    // Keep one transparent texel for the linear sampler. Pixels outside this
    // guard were transparent in the native cell and contribute nothing.
    left = left.saturating_sub(1);
    top = top.saturating_sub(1);
    right = right.saturating_add(1).min(width);
    bottom = bottom.saturating_add(1).min(height);
    let cropped_width = right - left;
    let cropped_height = bottom - top;
    if cropped_width != width || cropped_height != height {
        for row in 0..cropped_height {
            let source = ((top + row) * width + left) as usize * 4;
            let destination = (row * cropped_width) as usize * 4;
            pixels.copy_within(source..source + cropped_width as usize * 4, destination);
        }
        pixels.truncate(cropped_width as usize * cropped_height as usize * 4);
    }
    GlyphBitmap::from_vec(
        cropped_width,
        cropped_height,
        GlyphBitmapFormat::Rgba8,
        pixels,
    )
    .map(|bitmap| (bitmap, [left as i32, top as i32]))
    .map_err(|_| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"))
}

fn paint_buffer(
    glyph: &GlyphBitmap,
    style: PaintStyle,
    offset: [i32; 2],
    pixel_size: u32,
    padding: u32,
) -> Result<(u32, u32, Vec<u8>), CoreError> {
    let radius = if matches!(style.style, 1 | 2) {
        style.style
    } else {
        0
    };
    if glyph.format != GlyphBitmapFormat::Alpha8 || padding < radius || pixel_size == 0 {
        return Err(error("SHINKI_EMU_CMVS_GLYPH_FORMAT"));
    }
    let width = glyph
        .width
        .checked_add(padding * 2)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"))?;
    let height = glyph
        .height
        .checked_add(padding * 2)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"))?;
    let mut pixels = vec![0; width as usize * height as usize * 4];
    paint_pixels(
        glyph,
        style,
        offset,
        pixel_size,
        padding,
        width,
        &mut pixels,
    );
    Ok((width, height, pixels))
}

pub(super) fn paint_cell(
    glyph: &GlyphBitmap,
    style: PaintStyle,
    pixel_size: u32,
    cell_size: u32,
) -> Result<Vec<u8>, CoreError> {
    if glyph.format != GlyphBitmapFormat::Alpha8
        || pixel_size == 0
        || cell_size == 0
        || cell_size > 16384
    {
        return Err(error("SHINKI_EMU_CMVS_GLYPH_FORMAT"));
    }
    let count = (cell_size as usize)
        .checked_mul(cell_size as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"))?;
    let mut pixels = vec![0; count];
    paint_pixels(glyph, style, [0, 0], pixel_size, 2, cell_size, &mut pixels);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok(pixels)
}

fn paint_pixels(
    glyph: &GlyphBitmap,
    style: PaintStyle,
    offset: [i32; 2],
    pixel_size: u32,
    padding: u32,
    width: u32,
    pixels: &mut [u8],
) {
    let radius = if matches!(style.style, 1 | 2) {
        style.style
    } else {
        0
    };
    let outline_rgb = [
        (style.outline_color >> 16) as u8,
        (style.outline_color >> 8) as u8,
        style.outline_color as u8,
    ];
    if radius != 0 {
        outline(glyph, radius, padding, width, pixels);
        for pixel in pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .filter(|pixel| pixel[3] != 0)
        {
            pixel[..3].copy_from_slice(&outline_rgb);
        }
    }
    let height = pixels.len() / 4 / width as usize;
    for y in 0..glyph.height.min((height as u32).saturating_sub(padding)) {
        for x in 0..glyph.width.min(width.saturating_sub(padding)) {
            let alpha = glyph.pixels.as_slice()[(y * glyph.width + x) as usize];
            if alpha == 0 {
                continue;
            }
            let start = (((y + padding) * width + x + padding) * 4) as usize;
            let target = &mut pixels[start..start + 4];
            let rgba = color(
                style,
                [
                    i64::from(offset[0]) + i64::from(x),
                    i64::from(offset[1]) + i64::from(y),
                ],
                pixel_size,
            );
            for ((channel, source), background) in target[..3]
                .iter_mut()
                .zip(rgba[..3].iter())
                .zip(outline_rgb)
            {
                *channel = if style.style == 0 || alpha == 255 {
                    *source
                } else {
                    ((u32::from(*source) * u32::from(alpha)
                        + u32::from(background) * (256 - u32::from(alpha)))
                        >> 8) as u8
                };
            }
            target[3] = if style.style == 0 { alpha } else { 255 };
        }
    }
}

fn color(style: impl Into<PaintStyle>, point: [i64; 2], pixel_size: u32) -> [u8; 4] {
    let style = style.into();
    let mode = style.color_mode & 3;
    let mut rgba = [255; 4];
    for (channel, shift) in rgba[..3].iter_mut().zip([16, 8, 0]) {
        let start = i64::from((style.colors[0] >> shift) & 255) << 10;
        let end = i64::from((style.colors[1] >> shift) & 255) << 10;
        let step = (end - start) / i64::from(pixel_size);
        let distance = match mode {
            1 => step * point[1],
            2 => step * point[0],
            3 => (step / 2) * (point[0] + point[1]),
            _ => 0,
        };
        *channel = ((start + distance) >> 10).clamp(0, 255) as u8;
    }
    rgba
}

/// Paints one glyph into a padded RGBA bitmap, for composition tests.
#[cfg(test)]
pub(super) fn paint(
    glyph: &GlyphBitmap,
    style: impl Into<PaintStyle>,
    offset: [i32; 2],
    pixel_size: u32,
    padding: u32,
) -> Result<GlyphBitmap, CoreError> {
    let (width, height, pixels) = paint_buffer(glyph, style.into(), offset, pixel_size, padding)?;
    GlyphBitmap::from_vec(width, height, GlyphBitmapFormat::Rgba8, pixels)
        .map_err(|_| error("SHINKI_EMU_CMVS_GLYPH_FORMAT"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_outline_uses_maximum_coverage_and_attenuated_corners() {
        let source = GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Alpha8, vec![200]).unwrap();
        let mut pixels = vec![0; 5 * 5 * 4];
        outline(&source, 2, 2, 5, &mut pixels);
        assert_eq!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[3])
                .collect::<Vec<_>>(),
            &[
                50, 100, 200, 100, 50, 100, 200, 200, 200, 100, 200, 200, 200, 200, 200, 100, 200,
                200, 200, 100, 50, 100, 200, 100, 50
            ]
        );
    }

    #[test]
    fn native_color_modes_select_vertical_horizontal_and_diagonal_axes() {
        let mut glyph = CmvsNativeTextGlyph {
            text: "A".into(),
            code: b'A' as u16,
            raster: Default::default(),
            position: [0, 0],
            font: "test".into(),
            size: 32,
            colors: [0xff0000, 0x0000ff],
            style: 0,
            color_mode: 0,
        };
        assert_eq!(color(&glyph, [16, 16], 32), [255, 0, 0, 255]);
        glyph.color_mode = 1;
        assert_eq!(color(&glyph, [32, 0], 32), [255, 0, 0, 255]);
        assert_eq!(color(&glyph, [0, 32], 32), [0, 0, 255, 255]);
        glyph.color_mode = 2;
        assert_eq!(color(&glyph, [32, 0], 32), [0, 0, 255, 255]);
        glyph.color_mode = 3;
        assert_eq!(color(&glyph, [16, 16], 32), [127, 0, 127, 255]);
    }

    fn style(outline: u32, color_mode: u32) -> CmvsNativeTextGlyph {
        CmvsNativeTextGlyph {
            text: "A".into(),
            code: b'A' as u16,
            raster: Default::default(),
            position: [0, 0],
            font: "test".into(),
            size: 4,
            colors: [0xff0000, 0x0000ff],
            style: outline,
            color_mode,
        }
    }

    #[test]
    fn outlined_fill_is_opaque_and_uses_native_integer_rgb_mix() {
        let source =
            GlyphBitmap::from_vec(2, 1, GlyphBitmapFormat::Alpha8, vec![128, 255]).unwrap();
        let bitmap = paint(&source, &style(2, 0), [0, 0], 4, 2).unwrap();
        let pixels = bitmap.pixels.as_slice();
        // A half-covered fill sample is opaque dark red, not translucent red
        // over a separately blended translucent black outline.
        assert_eq!(&pixels[(2 * 6 + 2) * 4..(2 * 6 + 3) * 4], &[127, 0, 0, 255]);
        assert_eq!(&pixels[(2 * 6 + 3) * 4..(2 * 6 + 4) * 4], &[255, 0, 0, 255]);
        assert_eq!(&pixels[..4], &[0, 0, 0, 32]);
        assert_eq!(&pixels[4..8], &[0, 0, 0, 64]);
    }

    #[test]
    fn unoutlined_fill_preserves_coverage_and_colors_each_raster_row() {
        let source =
            GlyphBitmap::from_vec(1, 2, GlyphBitmapFormat::Alpha8, vec![128, 255]).unwrap();
        let bitmap = paint(&source, &style(0, 1), [0, 1], 4, 0).unwrap();
        assert_eq!(
            bitmap.pixels.as_slice(),
            &[191, 0, 63, 128, 127, 0, 127, 255]
        );
    }

    #[test]
    fn diagonal_gradient_truncates_native_fixed_point_step_before_halving() {
        let mut glyph = style(0, 3);
        glyph.colors = [0, 0x010101];
        assert_eq!(color(&glyph, [9, 9], 9), [0, 0, 0, 255]);
        glyph.color_mode = 2;
        assert_eq!(color(&glyph, [9, 0], 9), [0, 0, 0, 255]);
    }

    #[test]
    fn native_cell_padding_is_two_for_every_style() {
        let source = GlyphBitmap::from_vec(1, 1, GlyphBitmapFormat::Alpha8, vec![128]).unwrap();
        for radius in 0..=2 {
            let bitmap = paint(&source, &style(radius, 0), [0, 0], 4, 2).unwrap();
            assert_eq!((bitmap.width, bitmap.height), (5, 5));
            assert_eq!(
                &bitmap.pixels.as_slice()[12 * 4..13 * 4],
                if radius == 0 {
                    &[255, 0, 0, 128]
                } else {
                    &[127, 0, 0, 255]
                }
            );
            if radius < 2 {
                assert_eq!(&bitmap.pixels.as_slice()[..4], &[0; 4]);
            }
        }
    }

    #[test]
    #[ignore = "requires a hardware GPU"]
    fn compact_upload_preserves_native_pixels_under_scale_and_reveal() {
        use roukaku_media_core::{BlendMode, GlyphInstance, SceneCompositing2D, Transform2D};
        let mut coverage = vec![0; 40 * 40];
        for y in 8..32 {
            for x in 7..30 {
                coverage[y * 40 + x] = (40 + (x * 7 + y * 3) % 200) as u8;
            }
        }
        let source = GlyphBitmap::from_vec(40, 40, GlyphBitmapFormat::Alpha8, coverage).unwrap();
        let mut renderer = pollster::block_on(roukaku_platform_common::WgpuOffscreenRenderer::new(
            crate::scene::tests::gpu_instance(),
        ))
        .unwrap()
        .with_default_compositing(SceneCompositing2D::EncodedSrgb);
        let transforms = [
            Transform2D::IDENTITY,
            Transform2D {
                m11: 1.5,
                m22: 1.5,
                ..Transform2D::IDENTITY
            },
            Transform2D {
                m11: 0.65,
                m22: 1.25,
                tx: 11.35,
                ty: 0.2,
                ..Transform2D::IDENTITY
            },
        ];
        let mut sequence = 0;
        for style_id in [0, 2, 3] {
            let style = PaintStyle {
                style: style_id,
                colors: [0xffffff, 0x60a0ff],
                color_mode: 2,
                outline_color: 0,
            };
            let full = paint(&source, style, [0, 0], 40, 2).unwrap();
            let (compact, offset) = paint_upload(&source, style, 40).unwrap();
            assert!(compact.pixels.len() < full.pixels.len());
            let full_id = format!("full.{style_id}");
            let compact_id = format!("compact.{style_id}");
            let uploads = vec![
                SceneCommand::UploadGlyph {
                    resource_id: full_id.clone(),
                    glyph: full,
                },
                SceneCommand::UploadGlyph {
                    resource_id: compact_id.clone(),
                    glyph: compact,
                },
            ];
            for (index, transform) in transforms.into_iter().enumerate() {
                let mut render = |id: &str, offset: [i32; 2], mut commands: Vec<SceneCommand>| {
                    commands.push(SceneCommand::PushTransform { transform });
                    commands.push(SceneCommand::GlyphRun {
                        id: format!("draw.{sequence}"),
                        glyphs: vec![GlyphInstance {
                            resource_id: id.into(),
                            x: 12 + offset[0],
                            y: 10 + offset[1],
                            rotation_quadrants: 0,
                        }]
                        .into(),
                        rgba: [255; 4],
                        opacity: 0.7,
                        blend: BlendMode::Alpha,
                    });
                    commands.push(SceneCommand::PopTransform);
                    sequence += 1;
                    renderer
                        .render(&roukaku_platform::SceneFrame {
                            sequence,
                            width: 96,
                            height: 96,
                            clear_rgba: [17, 60, 90, 255],
                            commands,
                            semantics: None,
                        })
                        .unwrap()
                        .rgba8
                };
                let original = render(
                    &full_id,
                    [0, 0],
                    if index == 0 {
                        uploads.clone()
                    } else {
                        Vec::new()
                    },
                );
                let cropped = render(&compact_id, offset, Vec::new());
                assert!(
                    original
                        .iter()
                        .zip(cropped.iter())
                        .all(|(left, right)| left.abs_diff(*right) <= 1)
                );
            }
        }
    }
}
