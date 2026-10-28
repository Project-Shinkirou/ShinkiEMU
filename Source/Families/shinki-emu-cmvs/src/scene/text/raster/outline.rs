//! Portable 461840 glyph query: the coverage, black box and advance that
//! `GetGlyphOutlineA(GGO_GRAY4_BITMAP)` returns for a
//! `CreateFontA(-size, weight, charset)` font, computed from the font file.
//!
//! The outline is grid-fitted by the font's TrueType instructions at the
//! pixel size (monochrome target, as GDI hints before antialiasing), scaled
//! by four and sampled at 4x4 subpixel centres with the nonzero rule, giving
//! GDI's 17 coverage levels. The black box is the inked pixel bounds. GDI's
//! other font-mapper rules are reproduced explicitly: fixed-pitch DBCS cells
//! are twice the single-byte cell, and a weight with no matching face is
//! emboldened by one pixel.
use super::*;
use roukaku_core::Hash256;
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, Target},
    raw::TableProvider,
};
use std::sync::Arc;

const OVERSAMPLE: i32 = 4;

/// One registered font face (an sfnt or a collection member).
pub(super) struct Face {
    asset_id: String,
    face_index: u32,
    hash: Hash256,
    bytes: Arc<[u8]>,
}

impl Face {
    pub fn new(font: &PackagedFont) -> Result<Self, CoreError> {
        FontRef::from_index(&font.bytes, font.face_index)
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_FONT_BYTES"))?;
        Ok(Self {
            asset_id: font.asset_id.clone(),
            face_index: font.face_index,
            hash: font.hash,
            bytes: font.bytes.as_slice().into(),
        })
    }

    pub fn matches(&self, font: &PackagedFont) -> bool {
        self.asset_id == font.asset_id
            && self.face_index == font.face_index
            && self.hash == font.hash
    }

    fn font(&self) -> Result<FontRef<'_>, CoreError> {
        FontRef::from_index(&self.bytes, self.face_index)
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_FONT_BYTES"))
    }
}

/// A selected face at one pixel size and weight.
pub(super) struct Scaler {
    face: usize,
    hinting: HintingInstance,
    metrics: cell::FaceMetrics,
    /// The single-byte cell of a fixed-pitch face; DBCS cells are twice it.
    fixed_cell: Option<i32>,
    embolden: bool,
}

fn scaled(units: i32, size: u32, units_per_em: u16) -> i32 {
    // GDI rounds each scaled vertical metric half away from zero.
    let numerator = i64::from(units) * i64::from(size) * 2;
    let denominator = i64::from(units_per_em) * 2;
    ((numerator + numerator.signum() * i64::from(units_per_em)) / denominator) as i32
}

impl Scaler {
    pub fn new(assets: &[Face], key: FontKey) -> Result<Self, CoreError> {
        let bold = key.weight == 600;
        let face = key.assets[usize::from(bold)];
        // A weight with no distinct face of its own is emboldened.
        let embolden = bold && key.assets[1] == key.assets[0];
        let font = assets
            .get(face)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RASTER_FONT"))?
            .font()?;
        let units_per_em = font
            .head()
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_FONT_BYTES"))?
            .units_per_em();
        if units_per_em == 0 {
            return Err(error("SHINKI_EMU_CMVS_RASTER_FONT_BYTES"));
        }
        let (ascent, descent) = match font.os2() {
            Ok(os2) => (
                i32::from(os2.us_win_ascent()),
                i32::from(os2.us_win_descent()),
            ),
            Err(_) => {
                let hhea = font
                    .hhea()
                    .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_FONT_BYTES"))?;
                (
                    i32::from(hhea.ascender().to_i16()),
                    -i32::from(hhea.descender().to_i16()),
                )
            }
        };
        let ascent = scaled(ascent, key.size, units_per_em);
        let descent = scaled(descent, key.size, units_per_em);
        let size = i32::try_from(key.size).map_err(|_| error("SHINKI_EMU_CMVS_RASTER_SIZE"))?;
        let fixed_cell = (key.shift_jis
            && font.post().is_ok_and(|post| post.is_fixed_pitch() != 0))
        .then(|| {
            font.os2()
                .map(|os2| scaled(i32::from(os2.x_avg_char_width()), key.size, units_per_em))
                .ok()
        })
        .flatten();
        let hinting = HintingInstance::new(
            &font.outline_glyphs(),
            Size::new(key.size as f32),
            LocationRef::default(),
            HintingOptions {
                engine: Engine::Interpreter,
                target: Target::Mono,
            },
        )
        .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_HINTING"))?;
        Ok(Self {
            face,
            hinting,
            metrics: cell::FaceMetrics {
                ascent,
                internal_leading: ascent + descent - size,
            },
            fixed_cell,
            embolden,
        })
    }
}

/// The character `GetGlyphOutlineA` maps from a native code in the font's
/// charset code page.
fn character(code: u16, shift_jis: bool) -> Option<char> {
    let bytes = code.to_be_bytes();
    let bytes = if code <= 255 { &bytes[1..] } else { &bytes[..] };
    let encoding = if shift_jis || code > 255 {
        encoding_rs::SHIFT_JIS
    } else {
        encoding_rs::WINDOWS_1252
    };
    let (text, malformed) = encoding.decode_without_bom_handling(bytes);
    let mut characters = text.chars();
    let character = characters.next()?;
    (!malformed && characters.next().is_none()).then_some(character)
}

pub(super) fn raster(
    assets: &[Face],
    scaler: &Scaler,
    key: CellKey,
) -> Result<cell::Cell, CoreError> {
    let font = assets
        .get(scaler.face)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_RASTER_FONT"))?
        .font()?;
    let glyph = character(key.code, key.font.shift_jis)
        .and_then(|character| font.charmap().map(character))
        .unwrap_or(GlyphId::NOTDEF);
    let outlines = font.outline_glyphs();
    let mut pen = Pen::default();
    let mut advance = 0;
    if let Some(outline) = outlines.get(glyph) {
        let adjusted = outline
            .draw(DrawSettings::hinted(&scaler.hinting, false), &mut pen)
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_OUTLINE"))?;
        advance = adjusted.advance_width.unwrap_or_default().round() as i32;
    }
    if let Some(cell) = scaler.fixed_cell
        && key.code > 255
        && advance > cell
    {
        advance = cell * 2;
    }
    let mut coverage = coverage(&pen.edges);
    if scaler.embolden {
        advance += 1;
        if let Some(coverage) = &mut coverage {
            coverage.embolden();
        }
    }
    let (metrics, levels) = match coverage {
        Some(coverage) => (
            cell::GlyphMetrics {
                black_box: [coverage.width, coverage.height],
                origin: coverage.origin,
                cell_inc_x: advance as i16,
            },
            coverage.levels,
        ),
        // GDI reports a 1x1 black box at the origin for a blank glyph.
        None => (
            cell::GlyphMetrics {
                black_box: [1, 1],
                origin: [0, 0],
                cell_inc_x: advance as i16,
            },
            Vec::new(),
        ),
    };
    cell::compose(
        key.font.size,
        key.code,
        &metrics,
        &scaler.metrics,
        &levels,
        key.settings(),
    )
}

/// One y-monotone edge in oversampled units, y up: a line (`control` is
/// `None`) or a quadratic Bézier.
#[derive(Clone, Copy)]
struct Edge {
    from: [f64; 2],
    control: Option<[f64; 2]>,
    to: [f64; 2],
}

impl Edge {
    fn bounds(&self) -> [f64; 4] {
        let mut bounds = [self.from[0], self.from[1], self.from[0], self.from[1]];
        for point in self.control.iter().chain([&self.to]) {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
        bounds
    }

    /// The crossing at scanline `y` and its winding, half-open at the top.
    fn crossing(&self, y: f64) -> Option<(f64, i32)> {
        let (low, high, winding) = if self.from[1] < self.to[1] {
            (self.from[1], self.to[1], 1)
        } else {
            (self.to[1], self.from[1], -1)
        };
        if !(low <= y && y < high) {
            return None;
        }
        let Some(control) = self.control else {
            let t = (y - self.from[1]) / (self.to[1] - self.from[1]);
            return Some((self.from[0] + t * (self.to[0] - self.from[0]), winding));
        };
        let a = self.from[1] - 2.0 * control[1] + self.to[1];
        let b = 2.0 * (control[1] - self.from[1]);
        let c = self.from[1] - y;
        let t = if a.abs() < 1e-12 {
            -c / b
        } else {
            let discriminant = (b * b - 4.0 * a * c).max(0.0).sqrt();
            let first = (-b + discriminant) / (2.0 * a);
            if (0.0..=1.0).contains(&first) {
                first
            } else {
                (-b - discriminant) / (2.0 * a)
            }
        }
        .clamp(0.0, 1.0);
        let u = 1.0 - t;
        Some((
            u * u * self.from[0] + 2.0 * u * t * control[0] + t * t * self.to[0],
            winding,
        ))
    }
}

#[derive(Default)]
struct Pen {
    edges: Vec<Edge>,
    start: [f64; 2],
    current: [f64; 2],
}

impl Pen {
    fn point(x: f32, y: f32) -> [f64; 2] {
        [
            f64::from(x) * f64::from(OVERSAMPLE),
            f64::from(y) * f64::from(OVERSAMPLE),
        ]
    }

    fn line(&mut self, to: [f64; 2]) {
        if to[1] != self.current[1] {
            self.edges.push(Edge {
                from: self.current,
                control: None,
                to,
            });
        }
        self.current = to;
    }

    fn quad(&mut self, control: [f64; 2], to: [f64; 2]) {
        let from = self.current;
        // Split at the vertical extremum so every piece is y-monotone.
        let denominator = from[1] - 2.0 * control[1] + to[1];
        let t = if denominator.abs() > 1e-12 {
            (from[1] - control[1]) / denominator
        } else {
            -1.0
        };
        if t > 0.0 && t < 1.0 {
            let lerp =
                |a: [f64; 2], b: [f64; 2]| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let first = lerp(from, control);
            let second = lerp(control, to);
            let middle = lerp(first, second);
            self.push_quad(from, first, middle);
            self.push_quad(middle, second, to);
        } else {
            self.push_quad(from, control, to);
        }
        self.current = to;
    }

    fn push_quad(&mut self, from: [f64; 2], control: [f64; 2], to: [f64; 2]) {
        if from[1] != to[1] {
            self.edges.push(Edge {
                from,
                control: Some(control),
                to,
            });
        }
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = Self::point(x, y);
        self.current = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.line(Self::point(x, y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.quad(Self::point(cx0, cy0), Self::point(x, y));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        // CFF outlines: approximate each cubic by quadratic pieces.
        const PIECES: usize = 8;
        let p0 = self.current;
        let [p1, p2, p3] = [
            Self::point(cx0, cy0),
            Self::point(cx1, cy1),
            Self::point(x, y),
        ];
        let at = |t: f64| {
            let u = 1.0 - t;
            [0, 1].map(|i| {
                u * u * u * p0[i]
                    + 3.0 * u * u * t * p1[i]
                    + 3.0 * u * t * t * p2[i]
                    + t * t * t * p3[i]
            })
        };
        let derivative = |t: f64| {
            let u = 1.0 - t;
            [0, 1].map(|i| {
                3.0 * u * u * (p1[i] - p0[i])
                    + 6.0 * u * t * (p2[i] - p1[i])
                    + 3.0 * t * t * (p3[i] - p2[i])
            })
        };
        for piece in 0..PIECES {
            let (t0, t1) = (
                piece as f64 / PIECES as f64,
                (piece + 1) as f64 / PIECES as f64,
            );
            let (a, b) = (at(t0), at(t1));
            let (da, db) = (derivative(t0), derivative(t1));
            let h = (t1 - t0) / 2.0;
            let control = [0, 1].map(|i| ((a[i] + da[i] * h) + (b[i] - db[i] * h)) / 2.0);
            self.quad(control, b);
        }
    }
    fn close(&mut self) {
        let start = self.start;
        self.line(start);
    }
}

struct Coverage {
    width: u32,
    height: u32,
    /// Black-box left column and top row, in pixels, y up from the baseline.
    origin: [i32; 2],
    /// GGO_GRAY4 rows (levels 0..=16), each padded to a DWORD.
    levels: Vec<u8>,
}

impl Coverage {
    fn stride(width: u32) -> usize {
        ((width + 3) & !3) as usize
    }

    /// GDI's synthetic bold: one pixel wider, each level adds its left
    /// neighbour's, saturating at full coverage.
    fn embolden(&mut self) {
        let (old, new) = (Self::stride(self.width), Self::stride(self.width + 1));
        let mut levels = vec![0; new * self.height as usize];
        for y in 0..self.height as usize {
            let row = &self.levels[y * old..y * old + self.width as usize];
            for x in 0..=self.width as usize {
                let here = row.get(x).copied().unwrap_or(0);
                let left = x.checked_sub(1).map_or(0, |x| row[x]);
                levels[y * new + x] = (here + left).min(16);
            }
        }
        self.width += 1;
        self.levels = levels;
    }
}

fn coverage(edges: &[Edge]) -> Option<Coverage> {
    let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for edge in edges {
        let [x0, y0, x1, y1] = edge.bounds();
        bounds = [
            bounds[0].min(x0),
            bounds[1].min(y0),
            bounds[2].max(x1),
            bounds[3].max(y1),
        ];
    }
    if edges.is_empty() || bounds.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let oversample = f64::from(OVERSAMPLE);
    let left = (bounds[0] / oversample).floor() as i32;
    let right = (bounds[2] / oversample).ceil() as i32;
    let bottom = (bounds[1] / oversample).floor() as i32;
    let top = (bounds[3] / oversample).ceil() as i32;
    let width = usize::try_from(right - left).ok()?.max(1);
    let height = usize::try_from(top - bottom).ok()?.max(1);
    if width > 16384 || height > 16384 {
        return None;
    }
    let mut grid = vec![0u8; width * height];
    let mut crossings = Vec::new();
    for sample_row in 0..height * OVERSAMPLE as usize {
        let y = f64::from(bottom * OVERSAMPLE) + sample_row as f64 + 0.5;
        crossings.clear();
        crossings.extend(edges.iter().filter_map(|edge| edge.crossing(y)));
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let row = height - 1 - sample_row / OVERSAMPLE as usize;
        let mut winding = 0;
        let mut span_start = 0.0;
        for &(x, direction) in &crossings {
            let before = winding;
            winding += direction;
            if before == 0 && winding != 0 {
                span_start = x;
            } else if before != 0 && winding == 0 {
                // Subpixel centres at x + 0.5 inside [span_start, x).
                let first = (span_start - 0.5).ceil() as i64;
                let last = (x - 0.5).ceil() as i64;
                let origin = i64::from(left * OVERSAMPLE);
                for sample in first.max(origin)..last.min(i64::from(right * OVERSAMPLE)) {
                    grid[row * width + ((sample - origin) / i64::from(OVERSAMPLE)) as usize] += 1;
                }
            }
        }
    }
    let inked = |x: usize, y: usize| grid[y * width + x] != 0;
    let first_row = (0..height).find(|&y| (0..width).any(|x| inked(x, y)))?;
    let last_row = (0..height)
        .rev()
        .find(|&y| (0..width).any(|x| inked(x, y)))?;
    let first_column = (0..width).find(|&x| (0..height).any(|y| inked(x, y)))?;
    let last_column = (0..width)
        .rev()
        .find(|&x| (0..height).any(|y| inked(x, y)))?;
    let cropped_width = (last_column - first_column + 1) as u32;
    let cropped_height = (last_row - first_row + 1) as u32;
    let stride = Coverage::stride(cropped_width);
    let mut levels = vec![0; stride * cropped_height as usize];
    for y in 0..cropped_height as usize {
        let source = (first_row + y) * width + first_column;
        levels[y * stride..y * stride + cropped_width as usize]
            .copy_from_slice(&grid[source..source + cropped_width as usize]);
    }
    Some(Coverage {
        width: cropped_width,
        height: cropped_height,
        origin: [left + first_column as i32, top - first_row as i32],
        levels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_metrics_round_half_away_from_zero_like_gdi() {
        // MS Gothic: 256 units per em, winAscent 220, winDescent 36.
        assert_eq!(scaled(220, 21, 256), 18);
        assert_eq!(scaled(36, 32, 256), 5);
        assert_eq!(scaled(36, 25, 256), 4);
        assert_eq!(scaled(128, 21, 256), 11);
    }

    #[test]
    fn square_coverage_counts_subpixel_centres_and_crops_to_ink() {
        // A 1.5 x 1.5 pixel square from (0.25, 0.25), y up.
        let mut pen = Pen::default();
        pen.move_to(0.25, 0.25);
        pen.line_to(1.75, 0.25);
        pen.line_to(1.75, 1.75);
        pen.line_to(0.25, 1.75);
        pen.close();
        let coverage = coverage(&pen.edges).unwrap();
        assert_eq!((coverage.width, coverage.height), (2, 2));
        assert_eq!(coverage.origin, [0, 2]);
        assert_eq!(&coverage.levels, &[9, 9, 0, 0, 9, 9, 0, 0]);
    }

    #[test]
    fn synthetic_bold_adds_the_left_neighbour_and_widens_by_one() {
        let mut coverage = Coverage {
            width: 3,
            height: 1,
            origin: [0, 1],
            levels: vec![4, 8, 2, 0],
        };
        coverage.embolden();
        assert_eq!(coverage.width, 4);
        assert_eq!(&coverage.levels[..4], &[4, 12, 10, 2]);
        coverage.levels = vec![16, 16, 1, 0, 0, 0, 0, 0];
        coverage.embolden();
        assert_eq!(&coverage.levels[..5], &[16, 16, 16, 1, 0]);
    }

    #[test]
    fn native_codes_decode_in_the_selected_charset() {
        assert_eq!(character(0x82a0, true), Some('あ'));
        assert_eq!(character(0xb1, true), Some('ｱ'));
        assert_eq!(character(0xe9, false), Some('é'));
        assert_eq!(character(0x41, false), Some('A'));
    }
}
