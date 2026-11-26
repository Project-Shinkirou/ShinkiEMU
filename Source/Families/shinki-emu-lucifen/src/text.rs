use roukaku_core::Hash256;
use roukaku_text::{
    CosmicTextLayoutProvider, FontBindingContext, GlyphPlacement, GlyphResource, LayoutConstraint,
    LogicalTextLayout, OverflowPolicy, PackagedFont, TextDirection, TextLayoutConfig,
    TextLayoutRequest, TextLayoutResult, TextRun, UnicodeRange, WrapPolicy,
};
use shinki_emu_sdk::TextScene;
use std::{io, sync::Arc};

const JP: &str = "Noto Sans JP";
const SC: &str = "Noto Sans SC";

#[derive(Clone)]
pub(crate) struct Font {
    pub family: String,
    pub size: u16,
    pub line_spacing: i32,
    pub foreground: [u8; 3],
    pub shadow: [u8; 3],
    pub effect: (u8, i32, i32),
    pub glyph_capacity: u16,
    pub gradient: Option<[[u8; 3]; 2]>,
    pub bold: bool,
    pub italic: bool,
}

pub(crate) struct Text {
    scene: TextScene,
    chinese: bool,
    coverage: Vec<UnicodeRange>,
}

pub(crate) struct NativeTextRaster {
    layout: Arc<TextLayoutResult>,
    origin_carry: [i32; 2],
    tracking_carry: [i32; 2],
}

impl NativeTextRaster {
    pub(crate) fn resources(&self) -> &[GlyphResource] {
        &self.layout.glyph_resources
    }

    pub(crate) fn glyphs(&self) -> impl Iterator<Item = (&str, [i64; 2])> {
        let mut tracking =
            (self.tracking_carry != [0, 0]).then(|| GlyphTracking::new(self.tracking_carry));
        self.layout
            .shaped_runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(move |glyph| {
                // Track every shaped glyph, including spaces with no bitmap.
                let offsets = tracking.as_mut().map(|tracking| tracking.next(glyph.line));
                let mut x = glyph.render_x?;
                if let Some([first, second]) = offsets {
                    x = x.saturating_add(first).saturating_add(second);
                }
                Some((
                    glyph.resource_id.as_deref()?,
                    [
                        i64::from(x) + i64::from(self.origin_carry[0]),
                        i64::from(glyph.render_y?) + i64::from(self.origin_carry[1]),
                    ],
                ))
            })
    }
}

struct GlyphTracking {
    spacing: [i32; 2],
    line: Option<u32>,
    index: i32,
}

impl GlyphTracking {
    fn new(spacing: [i32; 2]) -> Self {
        Self {
            spacing,
            line: None,
            index: 0,
        }
    }

    fn next(&mut self, line: u32) -> [i32; 2] {
        if self.line != Some(line) {
            self.line = Some(line);
            self.index = 0;
        }
        let offsets = self.spacing.map(|value| value.saturating_mul(self.index));
        self.index += 1;
        offsets
    }
}

impl Text {
    pub(crate) fn open(chinese: bool) -> io::Result<Self> {
        let context = FontBindingContext {
            target: "shinki-emu-lucifen".into(),
            profile: "lucifen.native".into(),
            default_locale: if chinese { "zh-CN" } else { "ja-JP" }.into(),
        };
        let fonts = [
            (
                JP,
                "asset:/font/emu/noto-sans-jp",
                include_bytes!("../../../../Assets/Fonts/NotoSansJP-Variable.ttf").as_slice(),
            ),
            (
                SC,
                "asset:/font/emu/noto-sans-sc",
                include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf").as_slice(),
            ),
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
                coverage: roukaku_text::font_unicode_coverage(bytes, 0).map_err(layout_error)?,
                targets: vec![context.target.clone()],
                profiles: vec![context.profile.clone()],
                bytes: bytes.to_vec(),
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
        let coverage = fonts
            .iter()
            .flat_map(|font| font.coverage.iter().cloned())
            .collect();
        let provider =
            CosmicTextLayoutProvider::new(context, fonts, TextLayoutConfig::production_defaults())
                .map_err(layout_error)?;
        Ok(Self {
            scene: TextScene::new(provider),
            chinese,
            coverage,
        })
    }

    pub(crate) fn shape(
        &self,
        font: &Font,
        text: String,
        width: u32,
    ) -> io::Result<Arc<LogicalTextLayout>> {
        self.shape_with_wrap(font, text, width, WrapPolicy::None)
    }

    pub(crate) fn shape_with_wrap(
        &self,
        font: &Font,
        text: String,
        width: u32,
        wrap: WrapPolicy,
    ) -> io::Result<Arc<LogicalTextLayout>> {
        let families = if self.chinese { [SC, JP] } else { [JP, SC] };
        let covered = |value: char| {
            value.is_control()
                || self
                    .coverage
                    .iter()
                    .any(|range| (range.start..=range.end).contains(&(value as u32)))
        };
        let text = if text.chars().all(covered) {
            text
        } else {
            let default_glyph = if covered('\u{fffd}') {
                '\u{fffd}'
            } else {
                '\u{25a1}'
            };
            text.chars()
                .map(|value| if covered(value) { value } else { default_glyph })
                .collect()
        };
        let request = TextLayoutRequest {
            key: "lucifen.native".into(),
            runs: vec![TextRun {
                text,
                language: if self.chinese { "zh-CN" } else { "ja-JP" }.into(),
                script: None,
                direction: TextDirection::LeftToRight,
                ruby: Vec::new(),
                voice: None,
            }],
            constraint: LayoutConstraint {
                max_width: width.max(1) as f32,
                max_height: None,
                max_lines: None,
                font_size: f32::from(font.size),
                line_height: (i32::from(font.size).saturating_add(font.line_spacing)).max(1) as f32,
                wrap,
                overflow: OverflowPolicy::Clip,
            },
            font_families: families.into_iter().map(str::to_owned).collect(),
            features: Vec::new(),
        };
        self.scene.shape_shared(&request).map_err(layout_error)
    }

    pub(crate) fn rasterize(
        &self,
        mut layout: Arc<LogicalTextLayout>,
        origin: [i32; 2],
        spacing: [i32; 2],
    ) -> io::Result<NativeTextRaster> {
        // Native offsets are integer pixels at 1×, so carrying whole pixels
        // outside the f32 rasterizer does not change either sampling phase.
        // Keep extreme script tracking out of the rasterizer's i32 positions,
        // where it can overflow even if an integer offset later cancels it.
        const RASTER_WINDOW: i32 = 65_536;
        let mut raster_origin = origin.map(|value| value % RASTER_WINDOW);
        let mut tracking_carry = [0, 0];
        if spacing != [0, 0] {
            let gaps = layout
                .shaped_runs
                .iter()
                .map(|run| run.glyphs.len())
                .sum::<usize>()
                .saturating_sub(1) as i64;
            let max_shift = gaps
                * spacing
                    .into_iter()
                    .map(|value| i64::from(value).abs())
                    .sum::<i64>();
            if max_shift < i64::from(RASTER_WINDOW) {
                let mut tracking = GlyphTracking::new(spacing);
                Arc::make_mut(&mut layout)
                    .place_glyphs(|_, _, glyph| {
                        let mut placement = GlyphPlacement::from(glyph);
                        for offset in tracking.next(glyph.line) {
                            placement.x += offset as f32;
                        }
                        Some(placement)
                    })
                    .map_err(layout_error)?;
            } else {
                raster_origin = [0, 0];
                tracking_carry = spacing;
            }
        }
        let [x, y] = raster_origin;
        let layout = self
            .scene
            .rasterize_shared(&layout, 1.0, (x as f32, y as f32))
            .map_err(layout_error)?;
        Ok(NativeTextRaster {
            layout,
            origin_carry: [origin[0] - x, origin[1] - y],
            tracking_carry,
        })
    }
}

fn layout_error(error: roukaku_media_core::MediaError) -> io::Error {
    let message = match error {
        roukaku_media_core::MediaError::Diagnostics(diagnostics) => {
            diagnostics.into_iter().next().map_or_else(
                || "Lucifen font layout failed".to_owned(),
                |first| format!("{}: {}", first.code, first.message),
            )
        }
        roukaku_media_core::MediaError::Message(message) => message,
    };
    io::Error::new(io::ErrorKind::InvalidData, message)
}
