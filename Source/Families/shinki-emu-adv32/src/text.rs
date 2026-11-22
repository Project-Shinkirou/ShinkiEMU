use crate::render::Canvas;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use image::{Pixel, Rgba};

pub(crate) const FONT: &[u8] = include_bytes!("../../../../Assets/Fonts/NotoSansJP-Variable.ttf");
pub(crate) const FONT_SC: &[u8] =
    include_bytes!("../../../../Assets/Fonts/NotoSansSC-Variable.ttf");

#[derive(Clone)]
pub(crate) struct Text {
    pub(crate) content: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) size: u16,
    pub(crate) color: [u8; 3],
    pub(crate) width: Option<u32>,
}

pub(crate) struct TextPainter {
    raster: Option<RasterText>,
}

struct RasterText {
    fonts: FontSystem,
    cache: SwashCache,
}

impl TextPainter {
    pub(crate) fn new() -> Self {
        Self { raster: None }
    }

    pub(crate) fn draw(&mut self, frame: &mut Canvas, text: &Text) {
        if text.content.is_empty() || text.size == 0 {
            return;
        }
        if frame.queue_text(text) {
            return;
        }
        let raster = self.raster.get_or_insert_with(|| {
            let mut fonts = FontSystem::new();
            fonts.db_mut().load_font_data(FONT.to_vec());
            fonts.db_mut().load_font_data(FONT_SC.to_vec());
            RasterText {
                fonts,
                cache: SwashCache::new(),
            }
        });
        let frame = frame.raster_mut();
        let size = f32::from(text.size.min(256));
        let mut buffer = Buffer::new(&mut raster.fonts, Metrics::new(size, size * 1.25));
        let mut layout = buffer.borrow_with(&mut raster.fonts);
        let available =
            (i64::from(frame.width()) - i64::from(text.x)).clamp(1, i64::from(frame.width()));
        let width = text
            .width
            .map(i64::from)
            .map_or(available, |width| width.min(available));
        layout.set_size(Some(width as f32), None);
        let attrs = Attrs::new().family(Family::Name("Noto Sans JP"));
        layout.set_text(&text.content, &attrs, Shaping::Advanced, None);
        let [r, g, b] = text.color;
        layout.draw(
            &mut raster.cache,
            Color::rgb(r, g, b),
            |x, y, w, h, color| {
                let color = Rgba([color.r(), color.g(), color.b(), color.a()]);
                for dy in 0..h {
                    for dx in 0..w {
                        let x = i64::from(text.x) + i64::from(x) + i64::from(dx);
                        let y = i64::from(text.y) + i64::from(y) + i64::from(dy);
                        if x >= 0
                            && y >= 0
                            && let Some(pixel) = frame.get_pixel_mut_checked(x as u32, y as u32)
                        {
                            pixel.blend(&color);
                        }
                    }
                }
            },
        );
    }
}
