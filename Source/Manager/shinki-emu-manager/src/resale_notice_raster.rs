use super::TEXT;
use roukaku_core::{DiagnosticSeverity, Hash256};
use roukaku_media_core::GlyphBitmapFormat;
use roukaku_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OpenTypeFeature,
    OverflowPolicy, PackagedFont, TextDirection, TextLayoutConfig, TextLayoutRequest, TextRun,
    WrapPolicy,
};
use std::collections::BTreeMap;

const FAMILY: &str = "Noto Sans SC";
const TARGET: &str = "shinki-emu-manager";
const PROFILE: &str = "resale-notice";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoticeTheme {
    pub dark: bool,
    pub surface: [u8; 3],
    pub on_surface: [u8; 3],
}

fn section_request(text: &str, language: &str, width: f32, scale: f32) -> TextLayoutRequest {
    TextLayoutRequest {
        key: format!("manager.resale-notice.{language}"),
        runs: vec![TextRun {
            text: text.into(),
            language: language.into(),
            script: None,
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: width,
            max_height: None,
            max_lines: None,
            font_size: 15.0 * scale,
            line_height: 23.0 * scale,
            wrap: WrapPolicy::WordOrGlyph,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec![FAMILY.into()],
        // Noto Sans SC's default curly quotes occupy a full CJK em. Its packaged
        // proportional alternates give English punctuation normal Latin spacing.
        features: if language == "en" {
            vec![OpenTypeFeature {
                tag: "pwid".into(),
                value: 1,
            }]
        } else {
            Vec::new()
        },
    }
}

/// Opaque RGBA pixels, ready for Slint's image property.
pub struct NoticeImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Uses the same packaged-font shaping/rasterization stack as the SDK.
/// The host caches by physical width, scale factor and UI palette.
pub struct NoticeRasterizer {
    provider: CosmicTextLayoutProvider,
}
impl NoticeRasterizer {
    pub fn new() -> Result<Self, String> {
        let bytes = include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf");
        let font = PackagedFont {
            asset_id: "asset:/font/manager/notice-sc".into(),
            family: FAMILY.into(),
            face_index: 0,
            hash: Hash256::from_sha256(bytes),
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: roukaku_text::font_unicode_coverage(bytes, 0).map_err(|e| e.to_string())?,
            targets: vec![TARGET.into()],
            profiles: vec![PROFILE.into()],
            bytes: bytes.to_vec(),
        };
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: TARGET.into(),
                profile: PROFILE.into(),
                default_locale: "zh-CN".into(),
            },
            vec![font],
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { provider })
    }

    /// Width is physical pixels. Height always includes both complete languages.
    pub fn rasterize(
        &self,
        width: u32,
        scale: f32,
        theme: NoticeTheme,
    ) -> Result<NoticeImage, String> {
        if !scale.is_finite()
            || !(0.5..=4.0).contains(&scale)
            || width < (96.0 * scale) as u32
            || width > 8192
        {
            return Err("SHINKI_EMU_NOTICE_RASTER_SIZE".into());
        }
        let padding = (12.0 * scale).ceil() as u32;
        let (chinese, english) = TEXT.split_once("\n\n").expect("bilingual notice");
        let mut layouts = Vec::with_capacity(2);
        for (text, language) in [(chinese, "zh-CN"), (english, "en")] {
            let layout = self
                .provider
                .layout_shared(&section_request(
                    text,
                    language,
                    (width - 2 * padding) as f32,
                    scale,
                ))
                .map_err(|error| error.to_string())?;
            if layout.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.severity,
                    DiagnosticSeverity::Error | DiagnosticSeverity::Blocking
                )
            }) {
                return Err(format!("SHINKI_EMU_NOTICE_TEXT: {:?}", layout.diagnostics));
            }
            layouts.push(layout);
        }
        let paragraph_gap = (23.0 * scale).ceil() as u32;
        let height = layouts
            .iter()
            .map(|layout| layout.height.ceil() as u32)
            .sum::<u32>()
            + paragraph_gap
            + 2 * padding;
        if height > 8192 {
            return Err("SHINKI_EMU_NOTICE_RASTER_SIZE".into());
        }
        let mut rgba = vec![0; width as usize * height as usize * 4];
        rgba.as_chunks_mut::<4>().0.fill([
            theme.surface[0],
            theme.surface[1],
            theme.surface[2],
            255,
        ]);
        let mut offset_y = padding;
        for layout in layouts {
            let resources: BTreeMap<_, _> = layout
                .glyph_resources
                .iter()
                .map(|r| (r.resource_id.as_str(), &r.bitmap))
                .collect();
            for glyph in layout.shaped_runs.iter().flat_map(|run| &run.glyphs) {
                let Some(id) = glyph.resource_id.as_deref() else {
                    continue;
                };
                let bitmap = resources.get(id).ok_or("SHINKI_EMU_NOTICE_GLYPH_MISSING")?;
                let origin_x =
                    glyph.render_x.ok_or("SHINKI_EMU_NOTICE_GLYPH_POSITION")? + padding as i32;
                let origin_y =
                    glyph.render_y.ok_or("SHINKI_EMU_NOTICE_GLYPH_POSITION")? + offset_y as i32;
                let pixels: &[u8] = bitmap.pixels.as_ref();
                for y in 0..bitmap.height {
                    for x in 0..bitmap.width {
                        let dx = origin_x + x as i32;
                        let dy = origin_y + y as i32;
                        if dx < 0 || dy < 0 || dx >= width as i32 || dy >= height as i32 {
                            continue;
                        }
                        let index = (y * bitmap.width + x) as usize;
                        let alpha = match bitmap.format {
                            GlyphBitmapFormat::Alpha8 => pixels[index],
                            GlyphBitmapFormat::Rgba8 => pixels[index * 4 + 3],
                        } as u32;
                        let dest = ((dy as u32 * width + dx as u32) * 4) as usize;
                        for channel in 0..3 {
                            rgba[dest + channel] = ((u32::from(theme.on_surface[channel]) * alpha
                                + u32::from(rgba[dest + channel]) * (255 - alpha)
                                + 127)
                                / 255) as u8;
                        }
                    }
                }
            }
            offset_y += layout.height.ceil() as u32 + paragraph_gap;
        }
        Ok(NoticeImage {
            width,
            height,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const DARK: NoticeTheme = NoticeTheme {
        dark: true,
        surface: [24, 29, 42],
        on_surface: [238, 238, 238],
    };
    #[test]
    fn bilingual_notice_wraps_on_phone_and_high_dpi_without_losing_text() {
        let rasterizer = NoticeRasterizer::new().unwrap();
        let phone = rasterizer.rasterize(320, 1.0, DARK).unwrap();
        let desktop = rasterizer.rasterize(680, 1.0, DARK).unwrap();
        let hidpi = rasterizer.rasterize(1360, 2.0, DARK).unwrap();
        assert!(phone.height > desktop.height);
        assert!((hidpi.height as i32 - desktop.height as i32 * 2).abs() < 8);
        for image in [&phone, &desktop, &hidpi] {
            assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
            let middle = image.height as usize / 2 * image.width as usize * 4;
            assert!(
                image.rgba[..middle]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 100)
            );
            assert!(
                image.rgba[middle..]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 100)
            );
            assert!(image.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        }
    }

    #[test]
    fn raster_uses_the_supplied_surface_and_text_colors() {
        let rasterizer = NoticeRasterizer::new().unwrap();
        let light = NoticeTheme {
            dark: false,
            surface: [235, 242, 250],
            on_surface: [20, 28, 35],
        };
        let dark = rasterizer.rasterize(680, 1.0, DARK).unwrap();
        let light = rasterizer.rasterize(680, 1.0, light).unwrap();
        assert_eq!((dark.width, dark.height), (light.width, light.height));
        assert_eq!(&dark.rgba[..4], &[24, 29, 42, 255]);
        assert_eq!(&light.rgba[..4], &[235, 242, 250, 255]);
        assert!(
            dark.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 200)
        );
        assert!(
            light
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] < 50)
        );
    }

    #[test]
    fn english_curly_quotes_use_packaged_proportional_glyphs_without_fallback_gaps() {
        let rasterizer = NoticeRasterizer::new().unwrap();
        let text = "“activating”";
        let layout = rasterizer
            .provider
            .layout_shared(&section_request(text, "en", 680.0, 1.0))
            .unwrap();
        let glyphs: Vec<_> = layout
            .shaped_runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .collect();
        assert_eq!(glyphs.len(), text.chars().count());
        assert!(glyphs.iter().all(|glyph| {
            glyph.font_asset_id == "asset:/font/manager/notice-sc"
                && glyph.glyph_id != 0
                && glyph.resource_id.is_some()
        }));
        let opening = glyphs.first().unwrap();
        let closing = glyphs.last().unwrap();
        assert!(opening.advance < 7.5 && closing.advance < 7.5);
        for pair in glyphs.windows(2) {
            assert!((pair[1].x - pair[0].x - pair[0].advance).abs() < 0.01);
            assert_eq!(pair[0].source.end, pair[1].source.start);
        }
    }
}
