use roukaku_core::Hash256;
use roukaku_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, PackagedFont,
    TextDirection, TextLayoutConfig, TextLayoutRequest, TextLayoutResult, TextRun, UnicodeRange,
    WrapPolicy,
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

    pub(crate) fn layout(
        &self,
        font: &Font,
        text: String,
        width: u32,
    ) -> io::Result<Arc<TextLayoutResult>> {
        self.layout_with_wrap(font, text, width, WrapPolicy::None)
    }

    pub(crate) fn layout_with_wrap(
        &self,
        font: &Font,
        text: String,
        width: u32,
        wrap: WrapPolicy,
    ) -> io::Result<Arc<TextLayoutResult>> {
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
        self.scene.layout_shared(&request).map_err(layout_error)
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
