use crate::{
    Error, Result,
    game::read_bounded,
    native_data,
    render::{Canvas, RasterAxis},
};
use encoding_rs::{Encoding, GBK};
use image::{Pixel, Rgba};
use lru::LruCache;
use roukaku_core::Hash256;
use roukaku_media_core::{GlyphBitmapFormat, MediaError};
use roukaku_text::{
    CosmicTextLayoutProvider, FontBindingContext, GlyphPlacement, LayoutConstraint,
    LogicalTextLayout, OpenTypeFeature, OverflowPolicy, PackagedFont, TextDirection,
    TextLayoutConfig, TextLayoutRequest, TextLayoutResult, TextRun, WrapPolicy,
    font_unicode_coverage,
};
use shinki_emu_sdk::{TextOutline, TextScene, TextSceneLayout};
use std::{collections::BTreeMap, num::NonZeroUsize, path::Path, sync::Arc};

const FONT: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansJP-Variable.ttf");
const FONT_SC: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf");
const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn native_font(root: &Path) -> Result<Option<Arc<PackagedFont>>> {
    let Some(family) = native_data::font_face(root)? else {
        return Ok(None);
    };
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    #[cfg(target_os = "android")]
    database.load_fonts_dir("/system/fonts");
    let family_name = |name: &str| {
        name.chars()
            .map(|c| match c {
                '！'..='～' => char::from_u32(u32::from(c) - 0xfee0).unwrap_or(c),
                '　' => ' ',
                c => c,
            })
            .collect::<String>()
    };
    let family = family_name(&family);
    let face = database
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(name, _)| family_name(name).eq_ignore_ascii_case(&family))
        })
        .min_by_key(|face| {
            (
                face.style != fontdb::Style::Normal,
                face.weight.0.abs_diff(400),
            )
        });
    face.map(|face| font_from_face(&database, face, "system-installed-local-use"))
        .transpose()
}

pub(crate) fn configured_font(root: &Path, relative: &Path) -> Result<Arc<PackagedFont>> {
    if !relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid("ADV32 font path"));
    }
    let root = root.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root) {
        return Err(Error::Invalid("ADV32 font path"));
    }
    let mut database = fontdb::Database::new();
    database.load_font_data(read_bounded(&path, MAX_FONT_BYTES)?);
    let face = database
        .faces()
        .find(|face| face.index == 0)
        .ok_or(Error::Invalid("ADV32 configured font face"))?;
    font_from_face(&database, face, "local-private")
}

fn font_from_face(
    database: &fontdb::Database,
    face: &fontdb::FaceInfo,
    license: &str,
) -> Result<Arc<PackagedFont>> {
    let family = face
        .families
        .first()
        .map(|family| family.0.clone())
        .ok_or(Error::Invalid("ADV32 font family"))?;
    let (bytes, face_index) = database
        .with_face_data(face.id, |bytes, index| {
            (bytes.len() <= MAX_FONT_BYTES).then(|| (bytes.to_vec(), index))
        })
        .flatten()
        .ok_or(Error::Invalid("ADV32 font data"))?;
    Ok(Arc::new(PackagedFont {
        asset_id: "asset:/font/adv32/primary".into(),
        family,
        face_index,
        hash: Hash256::from_sha256(&bytes),
        license_id: license.into(),
        subset: None,
        coverage: font_unicode_coverage(&bytes, face_index).map_err(text_error)?,
        targets: vec!["adv32".into()],
        profiles: vec!["adv32".into()],
        bytes,
    }))
}

pub(crate) fn cell_width(character: char, size: u16, encoding: &'static Encoding) -> u16 {
    if character.is_ascii() {
        return size / 2;
    }
    let mut utf8 = [0; 4];
    let (bytes, _, _) = encoding.encode(character.encode_utf8(&mut utf8));
    if bytes.len() == 1 { size / 2 } else { size }
}

#[derive(Clone)]
pub(crate) struct Text {
    pub(crate) content: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) size: u16,
    pub(crate) color: [u8; 3],
    pub(crate) width: Option<u32>,
    pub(crate) fixed_pitch: bool,
    pub(crate) line_height: Option<u16>,
    pub(crate) outline: bool,
    pub(crate) clip_width: Option<u32>,
}

impl Text {
    pub(crate) fn request(
        &self,
        key: String,
        frame_width: u32,
        encoding: &'static Encoding,
        font: Option<&PackagedFont>,
    ) -> TextSceneLayout {
        let (language, families) = if encoding == GBK {
            ("zh-Hans", ["Noto Sans SC", "Noto Sans JP"])
        } else {
            ("ja-JP", ["Noto Sans JP", "Noto Sans SC"])
        };
        let size = f32::from(self.size.min(256));
        let available =
            (i64::from(frame_width) - i64::from(self.x)).clamp(1, i64::from(frame_width)) as u32;
        let [r, g, b] = self.color;
        let features = if self.fixed_pitch {
            ["kern", "liga", "clig"]
                .into_iter()
                .map(|tag| OpenTypeFeature {
                    tag: tag.into(),
                    value: 0,
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut font_families = families.map(str::to_owned).to_vec();
        if let Some(font) = font {
            font_families.retain(|family| !family.eq_ignore_ascii_case(&font.family));
            font_families.insert(0, font.family.clone());
        }
        TextSceneLayout {
            request: TextLayoutRequest {
                key,
                runs: vec![TextRun {
                    text: self.content.clone(),
                    language: language.into(),
                    script: None,
                    direction: TextDirection::LeftToRight,
                    ruby: Vec::new(),
                    voice: None,
                }],
                constraint: LayoutConstraint {
                    max_width: self.width.unwrap_or(available).max(1) as f32,
                    max_height: None,
                    max_lines: None,
                    font_size: size,
                    line_height: self.line_height.map_or_else(
                        || {
                            if self.fixed_pitch {
                                size + 12.0
                            } else {
                                size * 1.25
                            }
                        },
                        f32::from,
                    ),
                    wrap: if self.fixed_pitch {
                        WrapPolicy::None
                    } else {
                        WrapPolicy::WordOrGlyph
                    },
                    overflow: OverflowPolicy::Visible,
                },
                font_families,
                features,
            },
            translation: (self.x, self.y),
            rgba: [r, g, b, 255],
            outline: self.outline.then_some(TextOutline {
                radius: 1,
                rgba: [0, 0, 0, 255],
            }),
        }
    }

    fn position_glyphs(
        &self,
        mut layout: Arc<LogicalTextLayout>,
        encoding: &'static Encoding,
    ) -> std::result::Result<Arc<LogicalTextLayout>, MediaError> {
        // ADV32's GDI text renderer uses a half cell for a single-byte
        // character and a full cell for a double-byte character. The glyph's
        // advance is only used to center it inside that cell.
        let mut cursor = 2;
        let cells: BTreeMap<_, _> = self
            .content
            .char_indices()
            .filter_map(|(index, character)| {
                if character == '\n' {
                    cursor = 2;
                    return None;
                }
                let cell = cell_width(character, self.size, encoding);
                let x = cursor;
                cursor += i32::from(cell);
                Some((index, (x, f32::from(cell))))
            })
            .collect();
        Arc::make_mut(&mut layout).place_glyphs(|_, _, glyph| {
            let mut placement = GlyphPlacement::from(glyph);
            if let Some(&(x, advance)) = cells.get(&glyph.source.start) {
                placement.x = x as f32 + (advance - glyph.advance) / 2.0;
                placement.advance = advance;
            }
            Some(placement)
        })?;
        Ok(layout)
    }
}

pub(crate) fn new_scene(
    encoding: &'static Encoding,
    font: Option<&PackagedFont>,
) -> Result<TextScene> {
    let mut fonts = [
        ("Noto Sans JP", "asset:/font/adv32/noto-jp", FONT),
        ("Noto Sans SC", "asset:/font/adv32/noto-sc", FONT_SC),
    ]
    .into_iter()
    .map(|(family, id, bytes)| {
        Ok(PackagedFont {
            asset_id: id.into(),
            family: family.into(),
            face_index: 0,
            hash: Hash256::from_sha256(bytes),
            license_id: "OFL-1.1".into(),
            subset: None,
            coverage: font_unicode_coverage(bytes, 0).map_err(text_error)?,
            targets: vec!["adv32".into()],
            profiles: vec!["adv32".into()],
            bytes: bytes.to_vec(),
        })
    })
    .collect::<Result<Vec<_>>>()?;
    if let Some(font) = font {
        fonts.retain(|fallback| !fallback.family.eq_ignore_ascii_case(&font.family));
        fonts.insert(0, font.clone());
    }
    let provider = CosmicTextLayoutProvider::new(
        FontBindingContext {
            target: "adv32".into(),
            profile: "adv32".into(),
            default_locale: if encoding == GBK { "zh-Hans" } else { "ja-JP" }.into(),
        },
        fonts,
        TextLayoutConfig::production_defaults(),
    )
    .map_err(text_error)?;
    Ok(TextScene::new(provider))
}

type NativeLayoutKey = (u64, u16, &'static str);

pub(crate) struct NativeTextLayouts(LruCache<NativeLayoutKey, Arc<LogicalTextLayout>>);

impl NativeTextLayouts {
    pub(crate) fn new() -> Self {
        Self(LruCache::new(NonZeroUsize::new(128).unwrap()))
    }

    pub(crate) fn position(
        &mut self,
        text: &Text,
        layout: Arc<LogicalTextLayout>,
        encoding: &'static Encoding,
    ) -> std::result::Result<Arc<LogicalTextLayout>, MediaError> {
        if !text.fixed_pitch {
            return Ok(layout);
        }
        // Raster density and region origin do not affect logical cell placement.
        let key = (layout.revision, text.size, encoding.name());
        self.0
            .try_get_or_insert(key, || text.position_glyphs(layout, encoding))
            .map(Arc::clone)
    }
}

pub(crate) struct TextPainter {
    encoding: &'static Encoding,
    font: Option<Arc<PackagedFont>>,
    raster: Option<TextScene>,
    positioned: NativeTextLayouts,
}

impl TextPainter {
    pub(crate) fn new(encoding: &'static Encoding, font: Option<Arc<PackagedFont>>) -> Self {
        Self {
            encoding,
            font,
            raster: None,
            positioned: NativeTextLayouts::new(),
        }
    }

    #[cfg(not(target_env = "ohos"))]
    pub(crate) fn font(&self) -> Option<Arc<PackagedFont>> {
        self.font.clone()
    }

    pub(crate) fn draw(&mut self, frame: &mut Canvas, text: &Text) -> Result<()> {
        if text.content.is_empty() || text.size == 0 || frame.queue_text(text) {
            return Ok(());
        }
        if self.raster.is_none() {
            self.raster = Some(new_scene(self.encoding, self.font.as_deref())?);
        }
        let encoding = self.encoding;
        let request = text.request(
            "adv32:raster".into(),
            frame.width(),
            encoding,
            self.font.as_deref(),
        );
        let raster = self.raster.as_ref().expect("text scene initialized");
        let logical = raster.shape_shared(&request.request).map_err(text_error)?;
        let positioned = self
            .positioned
            .position(text, logical, encoding)
            .map_err(text_error)?;
        let logical_width = frame.width();
        let (frame, mapping) = frame.text_raster();
        let density = mapping.map_or(1.0, |mapping| mapping.scale());
        let offset = (text.x as f32 * density, text.y as f32 * density);
        let layout = raster
            .rasterize_shared(&positioned, density, offset)
            .map_err(text_error)?;
        let bitmaps: BTreeMap<_, _> = layout
            .glyph_resources
            .iter()
            .map(|resource| (resource.resource_id.as_str(), &resource.bitmap))
            .collect();
        let (left, right) = text.clip_width.map_or((0, i64::from(logical_width)), |width| {
            let left = i64::from(text.x);
            (left, left + i64::from(width))
        });
        let (origin, clip_x, clip_y) = if let Some(mapping) = mapping {
            let viewport = mapping.viewport();
            (
                (viewport.x as i32, viewport.y as i32),
                RasterAxis::x(mapping).span(left, right),
                viewport.y..viewport.y + viewport.height,
            )
        } else {
            (
                (0, 0),
                left.clamp(0, i64::from(logical_width)) as u32
                    ..right.clamp(0, i64::from(logical_width)) as u32,
                0..frame.height(),
            )
        };
        let clip = [&clip_x, &clip_y];
        if text.outline {
            let radius = density.round().max(1.0) as i32;
            for (dx, dy) in [(-radius, 0), (radius, 0), (0, -radius), (0, radius)] {
                draw_glyphs(
                    frame,
                    &layout,
                    &bitmaps,
                    (origin.0 + dx, origin.1 + dy),
                    [0; 3],
                    clip,
                );
            }
        }
        draw_glyphs(frame, &layout, &bitmaps, origin, text.color, clip);
        Ok(())
    }
}

fn draw_glyphs(
    frame: &mut image::RgbaImage,
    layout: &TextLayoutResult,
    bitmaps: &BTreeMap<&str, &roukaku_media_core::GlyphBitmap>,
    origin: (i32, i32),
    [r, g, b]: [u8; 3],
    [clip_x, clip_y]: [&std::ops::Range<u32>; 2],
) {
    for glyph in layout.shaped_runs.iter().flat_map(|run| &run.glyphs) {
        let (Some(resource), Some(x), Some(y)) =
            (glyph.resource_id.as_deref(), glyph.render_x, glyph.render_y)
        else {
            continue;
        };
        let bitmap = bitmaps[resource];
        let mask: &[u8] = bitmap.pixels.as_ref();
        for dy in 0..bitmap.height {
            for dx in 0..bitmap.width {
                let x = origin.0 + x + dx as i32;
                let y = origin.1 + y + dy as i32;
                if clip_x.contains(&(x as u32))
                    && clip_y.contains(&(y as u32))
                    && let Some(pixel) = frame.get_pixel_mut_checked(x as u32, y as u32)
                {
                    let index = (dy * bitmap.width + dx) as usize;
                    let alpha = match bitmap.format {
                        GlyphBitmapFormat::Alpha8 => mask[index],
                        GlyphBitmapFormat::Rgba8 => mask[index * 4 + 3],
                    };
                    pixel.blend(&Rgba([r, g, b, alpha]));
                }
            }
        }
    }
}

fn text_error(error: impl std::fmt::Debug) -> Error {
    Error::Render(format!("ADV32 text: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::SHIFT_JIS;

    #[test]
    fn engine_cells_center_single_and_double_byte_glyphs_and_reset_each_line() {
        let text = Text {
            content: "Wiあ\nZ".into(),
            x: 0,
            y: 0,
            size: 22,
            color: [255; 3],
            width: None,
            fixed_pitch: true,
            line_height: None,
            outline: false,
            clip_width: None,
        };
        let painter = new_scene(SHIFT_JIS, None).unwrap();
        let request = text.request("cells".into(), 1280, SHIFT_JIS, None);
        let original = painter.shape_shared(&request.request).unwrap();
        let mut layouts = NativeTextLayouts::new();
        let positioned = layouts
            .position(&text, Arc::clone(&original), SHIFT_JIS)
            .unwrap();
        assert!(Arc::ptr_eq(
            &positioned,
            &layouts
                .position(&text, Arc::clone(&original), SHIFT_JIS)
                .unwrap()
        ));
        let glyphs = |layout: &TextLayoutResult| {
            layout
                .shaped_runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .map(|glyph| (glyph.x, glyph.advance, glyph.line))
                .collect::<Vec<_>>()
        };
        let original = glyphs(&original);
        let positioned = glyphs(&positioned);
        assert_eq!(positioned.len(), 4);
        let cells = [
            (2.0, 11.0, 0),
            (13.0, 11.0, 0),
            (24.0, 22.0, 0),
            (2.0, 11.0, 1),
        ];
        for ((glyph, (_, original_advance, _)), (cell_x, advance, line)) in
            positioned.into_iter().zip(original).zip(cells)
        {
            let x = cell_x + (advance - original_advance) / 2.0;
            assert_eq!(glyph, (x, advance, line));
        }
    }

    #[test]
    fn raster_request_keeps_logical_cells_and_clips_at_fractional_density() {
        use roukaku_media_core::Extent2D;
        use shinki_emu_sdk::StageCanvas;

        let mapping = StageCanvas::new(Extent2D::new(128, 64), Extent2D::new(192, 120)).unwrap();
        assert_eq!(
            mapping.logical_to_raster_point([13.0, 7.0]).unwrap(),
            [19.5, 22.5]
        );
        assert_eq!(
            mapping.raster_to_logical_point([19.5, 22.5]).unwrap(),
            [13.0, 7.0]
        );

        let mut text = Text {
            content: "I字".into(),
            x: 13,
            y: 7,
            size: 24,
            color: [255; 3],
            width: None,
            fixed_pitch: true,
            line_height: None,
            outline: false,
            clip_width: None,
        };
        let mut painter = TextPainter::new(SHIFT_JIS, None);
        let finish = |canvas| match canvas {
            Canvas::Raster(raster) => raster.into_image(),
            #[cfg(not(target_env = "ohos"))]
            Canvas::Scene(_) => unreachable!(),
        };
        let mut native = Canvas::new(128, 64);
        painter.draw(&mut native, &text).unwrap();
        let upscaled = finish(Canvas::from_image(finish(native), Some(mapping)));

        let mut full = Canvas::for_output(mapping, false);
        painter.draw(&mut full, &text).unwrap();
        let full = finish(full);
        assert_ne!(full, upscaled);

        let mut clipped = Canvas::for_output(mapping, false);
        text.clip_width = Some(23); // Logical right edge 36 maps to raster x=54.
        painter.draw(&mut clipped, &text).unwrap();
        let chip = crate::crx::Crx::from_image(
            0,
            0,
            image::RgbaImage::from_pixel(2, 2, Rgba([0, 90, 0, 255])),
        );
        // A later native sprite still samples on the logical pixel grid.
        crate::graphics::blit(&mut clipped, &chip, 34, 0, None);
        let clipped = finish(clipped);
        assert_eq!(clipped.dimensions(), (192, 120));
        let lit = |x: std::ops::Range<u32>| {
            x.into_iter().any(|x| (23..85).any(|y| full.get_pixel(x, y)[0] != 0))
        };
        // The ASCII glyph is centered in its half cell, and the CJK glyph
        // reaches beyond the right edge of the visible region.
        assert!(lit(28..36));
        assert!(lit(54..76));
        for (x, y, pixel) in clipped.enumerate_pixels() {
            let expected = match (x, y) {
                (51..=53, 12..=14) => Rgba([0, 90, 0, 255]),
                (19..=53, 12..=107) => *full.get_pixel(x, y),
                _ => Rgba([0, 0, 0, 255]),
            };
            assert_eq!(*pixel, expected, "raster pixel ({x}, {y})");
        }
    }
}
