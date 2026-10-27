use super::*;

const MAX_GLYPHS: usize = 65_535;
const MAX_RUBY_BASE: usize = 32;
const MAX_RUBY_LINES: usize = 32;

type Measure<'a> = Option<&'a mut dyn FnMut(&str, u32) -> Result<u32, CoreError>>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct DialogueLayoutOptions {
    pub ruby_enabled: bool,
    pub ruby_size: u32,
    /// Native ruby advance adjustment, in 1/1024-pixel units.
    pub ruby_spacing: u32,
    pub reserve_ruby: bool,
    pub interval_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DialogueReveal {
    pub glyph_index: usize,
    pub delay_ms: u32,
}

fn failure() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_TEXT_LAYOUT",
        "invalid native dialogue text or layout bounds",
    )
}

fn coordinate(value: i64) -> Result<i32, CoreError> {
    i32::try_from(value).map_err(|_| failure())
}

fn encoded_length(byte: u8) -> usize {
    if (0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte) {
        2
    } else {
        1
    }
}

fn code(bytes: &[u8]) -> u16 {
    if bytes.len() == 2 {
        u16::from_be_bytes([bytes[0], bytes[1]])
    } else {
        u16::from(bytes[0])
    }
}

fn prohibited(surface: &CmvsNativeTextSurface, value: u16) -> bool {
    surface.kinsoku && matches!(value, 0x8141 | 0x8142 | 0x8176 | 0x8178)
}

fn threshold(surface: &CmvsNativeTextSurface) -> i64 {
    i64::from(surface.rect[0]) + i64::from(surface.rect[2])
        - i64::from(surface.size)
        - i64::from(surface.character_spacing)
}

enum Token<'a> {
    Glyph(&'a [u8]),
    Literal(u8),
    Newline,
    Clear,
    ResetDelay,
    Suppress(bool),
    Ruby {
        base: &'a [u8],
        annotation: &'a [u8],
    },
}

fn units(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut index = 0;
    std::iter::from_fn(move || {
        let byte = *bytes.get(index)?;
        let start = index;
        index += encoded_length(byte);
        Some(&bytes[start..index])
    })
}

fn ruby_half<'a>(bytes: &'a [u8], index: &mut usize, delimiter: u8) -> Result<&'a [u8], CoreError> {
    let start = *index;
    loop {
        let byte = *bytes.get(*index).ok_or_else(failure)?;
        if byte == delimiter {
            let value = &bytes[start..*index];
            *index += 1;
            return Ok(value);
        }
        let length = encoded_length(byte);
        bytes.get(*index..*index + length).ok_or_else(failure)?;
        *index += length;
    }
}

fn next<'a>(bytes: &'a [u8], index: &mut usize) -> Result<Token<'a>, CoreError> {
    let byte = *bytes.get(*index).ok_or_else(failure)?;
    *index += 1;
    match byte {
        b'\\' => {
            let control = *bytes.get(*index).ok_or_else(failure)?;
            *index += 1;
            match control {
                b'"' | b'%' | b'{' | b'}' => Ok(Token::Literal(control)),
                b'@' => Ok(Token::Literal(b'\\')),
                b'n' => Ok(Token::Newline),
                b'c' => Ok(Token::Clear),
                b't' => Ok(Token::ResetDelay),
                b'w' => {
                    let state = *bytes.get(*index).ok_or_else(failure)?;
                    *index += 1;
                    match state {
                        b'c' => Ok(Token::Suppress(true)),
                        b's' => Ok(Token::Suppress(false)),
                        _ => Err(failure()),
                    }
                }
                _ => Err(failure()),
            }
        }
        b'{' => {
            let base = ruby_half(bytes, index, b'/')?;
            if units(base).filter(|unit| unit.len() == 2).count() > MAX_RUBY_BASE {
                return Err(failure());
            }
            let annotation = ruby_half(bytes, index, b'}')?;
            Ok(Token::Ruby { base, annotation })
        }
        _ => {
            let start = *index - 1;
            *index = start + encoded_length(byte);
            Ok(Token::Glyph(bytes.get(start..*index).ok_or_else(failure)?))
        }
    }
}

/// The native reservation pass uses fixed advances even in proportional mode,
/// counts narrow punctuation at full width, and does not reset x on `\c`.
fn ruby_lines(
    surface: &CmvsNativeTextSurface,
    bytes: &[u8],
) -> Result<[bool; MAX_RUBY_LINES], CoreError> {
    let mut lines = [false; MAX_RUBY_LINES];
    let mut line = 0usize;
    let mut x = i64::from(surface.cursor[0]);
    let mut index = 0;
    while index < bytes.len() {
        let (width, hang) = match next(bytes, &mut index)? {
            Token::Glyph(value) => (
                if value.len() == 2 {
                    surface.size
                } else {
                    surface.size / 2
                },
                prohibited(surface, code(value)),
            ),
            Token::Literal(_) => (surface.size / 2, false),
            Token::Ruby { base, .. } => {
                if x >= threshold(surface) {
                    line = line.checked_add(1).ok_or_else(failure)?;
                    x = i64::from(surface.rect[0]);
                }
                *lines.get_mut(line).ok_or_else(failure)? = true;
                x += units(base).filter(|unit| unit.len() == 2).count() as i64
                    * (i64::from(surface.size) + i64::from(surface.character_spacing));
                continue;
            }
            Token::Newline => {
                line = line.checked_add(1).ok_or_else(failure)?;
                lines.get(line).ok_or_else(failure)?;
                x = i64::from(surface.rect[0]);
                continue;
            }
            Token::Clear | Token::ResetDelay | Token::Suppress(_) => continue,
        };
        if x >= threshold(surface) && !hang {
            line = line.checked_add(1).ok_or_else(failure)?;
            lines.get(line).ok_or_else(failure)?;
            x = i64::from(surface.rect[0]);
        }
        x += i64::from(width) + i64::from(surface.character_spacing);
    }
    Ok(lines)
}

struct Layout<'a> {
    surface: &'a CmvsNativeTextSurface,
    options: DialogueLayoutOptions,
    cursor: [i32; 2],
    glyphs: Vec<CmvsNativeTextGlyph>,
    reveals: Vec<DialogueReveal>,
    offset: usize,
    cleared: bool,
    line: usize,
    ruby_lines: [bool; MAX_RUBY_LINES],
    delay: u32,
    suppressed: bool,
    boundary: Option<&'a CmvsEffectTextSurface>,
    line_boundary: measurement::LineBoundary,
}

impl Layout<'_> {
    fn reserve(&mut self) -> Result<(), CoreError> {
        if self.options.reserve_ruby
            || (self.options.ruby_enabled && *self.ruby_lines.get(self.line).ok_or_else(failure)?)
        {
            self.cursor[1] =
                coordinate(i64::from(self.cursor[1]) + i64::from(self.options.ruby_size))?;
        }
        Ok(())
    }

    fn newline(&mut self) -> Result<(), CoreError> {
        self.line = self.line.checked_add(1).ok_or_else(failure)?;
        self.line_boundary = measurement::LineBoundary::default();
        self.cursor[0] = self.surface.rect[0];
        self.cursor[1] = coordinate(
            i64::from(self.cursor[1])
                + i64::from(self.surface.size)
                + i64::from(self.surface.line_spacing),
        )?;
        self.reserve()
    }

    fn wrap(&mut self, value: u16, single_byte: bool) -> Result<(), CoreError> {
        if self.line_boundary.wrap(
            self.surface,
            self.boundary,
            self.cursor[0] as u32,
            value,
            single_byte,
        ) {
            self.newline()?;
        }
        Ok(())
    }

    fn pending(&mut self, glyph_index: Option<usize>, delay: u32) -> Result<u32, CoreError> {
        if self.suppressed {
            return Ok(0);
        }
        if self.options.interval_ms != 0 {
            let glyph_index = glyph_index.ok_or_else(failure)?;
            if let Some(last) = self
                .reveals
                .last_mut()
                .filter(|last| last.glyph_index == glyph_index)
            {
                last.delay_ms = delay;
            } else {
                self.reveals.push(DialogueReveal {
                    glyph_index,
                    delay_ms: delay,
                });
            }
        }
        delay
            .checked_add(self.options.interval_ms)
            .ok_or_else(failure)
    }

    fn glyph(
        &mut self,
        bytes: &[u8],
        size: u32,
        position: [i32; 2],
        measure: &mut Measure<'_>,
    ) -> Result<(usize, u32), CoreError> {
        if self.offset + self.glyphs.len() >= MAX_GLYPHS {
            return Err(failure());
        }
        let text = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(bytes)
            .ok_or_else(failure)?;
        let width = if let Some(callback) = measure {
            callback(&text, size)?
        } else if bytes.len() == 1 {
            size / 2
        } else {
            size
        };
        let index = self.offset + self.glyphs.len();
        self.glyphs.push(CmvsNativeTextGlyph {
            text: text.into_owned(),
            code: code(bytes),
            raster: CmvsNativeFontRasterState {
                bold: self.surface.bold,
                ..Default::default()
            },
            position,
            font: self.surface.font.clone(),
            size,
            colors: self.surface.colors,
            style: self.surface.style,
            color_mode: self.surface.color_mode,
        });
        Ok((index, width))
    }

    fn ordinary(&mut self, bytes: &[u8], measure: &mut Measure<'_>) -> Result<(), CoreError> {
        let value = code(bytes);
        self.wrap(value, bytes.len() == 1)?;
        let mut position = self.cursor;
        let narrow = value == 0x814a;
        if narrow {
            position[0] = coordinate(i64::from(position[0]) - i64::from(self.surface.size / 5))?;
        }
        let (index, measured) = self.glyph(bytes, self.surface.size, position, measure)?;
        let width = if narrow {
            self.surface.size / 2 - self.surface.size / 5
        } else {
            measured
        };
        self.cursor[0] = coordinate(
            i64::from(self.cursor[0])
                + i64::from(width)
                + i64::from(self.surface.character_spacing),
        )?;
        self.delay = self.pending(Some(index), self.delay)?;
        Ok(())
    }

    fn ruby(
        &mut self,
        base: &[u8],
        annotation: &[u8],
        measure: &mut Measure<'_>,
    ) -> Result<(), CoreError> {
        // A ruby group wraps once before its base, never inside it.
        self.wrap(0, false)?;
        let start = self.cursor[0];
        let initial_delay = self.delay;
        let mut positions = [[0i64; 2]; MAX_RUBY_BASE];
        let mut last = (self.offset + self.glyphs.len()).checked_sub(1);
        let mut base_count = 0;
        for bytes in units(base) {
            if bytes.len() == 2 {
                let (index, width) = self.glyph(bytes, self.surface.size, self.cursor, measure)?;
                positions[base_count] = [i64::from(self.cursor[0]), i64::from(width)];
                base_count += 1;
                self.cursor[0] = coordinate(
                    i64::from(self.cursor[0])
                        + i64::from(width)
                        + i64::from(self.surface.character_spacing),
                )?;
                last = Some(index);
            }
            // Single-byte ruby content is ignored by the native renderer but
            // still runs its reveal helper against the preceding valid glyph.
            self.delay = self.pending(last, self.delay)?;
        }
        // Both native ruby loops call the reveal helper once on their closing
        // delimiter, updating the preceding glyph's timing a second time.
        self.delay = self.pending(last, self.delay)?;
        if !self.options.ruby_enabled {
            return Ok(());
        }
        let annotation_y =
            coordinate(i64::from(self.cursor[1]) - i64::from(self.options.ruby_size))?;
        let first = self.glyphs.len();
        let mut annotation_delay = initial_delay;
        let mut total_width = 0i64;
        let mut all_dots = true;
        let proportional = measure.is_some();
        let mut count = 0;
        for bytes in units(annotation) {
            if bytes.len() == 2 {
                let (index, width) =
                    self.glyph(bytes, self.options.ruby_size, [0, annotation_y], measure)?;
                total_width += i64::from(width);
                // Until placement, x holds the measured advance to avoid a second
                // host measurement or an auxiliary annotation-width allocation.
                self.glyphs.last_mut().ok_or_else(failure)?.position[0] =
                    coordinate(i64::from(width))?;
                last = Some(index);
                count += 1;
                all_dots &= code(bytes) == 0x8145;
            }
            annotation_delay = self.pending(last, annotation_delay)?;
        }
        self.pending(last, annotation_delay)?;
        if count == 0 {
            // The fixed branch divides by the annotation glyph count.
            return if proportional { Ok(()) } else { Err(failure()) };
        }
        let span = i64::from(self.cursor[0]) - i64::from(start);
        if proportional && all_dots && count == base_count {
            // The native equal-count emphasis-dot branch centers one dot over
            // each measured base glyph (the repeated dot has one width).
            let dot_width = self.glyphs.last().ok_or_else(failure)?.position[0];
            for (glyph, base) in self.glyphs[first..].iter_mut().zip(positions) {
                glyph.position[0] = coordinate(base[0] + base[1] / 2 - i64::from(dot_width / 2))?;
            }
        } else if proportional {
            let mut x = i64::from(start) * 1024 + span * 512 - total_width * 512;
            for glyph in &mut self.glyphs[first..] {
                let width = glyph.position[0];
                glyph.position[0] = coordinate(x >> 10)?;
                x += i64::from(width) * 1024;
            }
        } else {
            let annotation_width = count as i64 * i64::from(self.options.ruby_size) * 1024;
            let mut x = i64::from(start) * 1024;

            let step = if annotation_width <= span * 1024 {
                let padding = (span * 1024 - annotation_width) / (2 * count as i64);
                x += padding;
                i64::from(self.options.ruby_spacing)
                    + 2 * (padding + i64::from(self.options.ruby_size) * 512)
            } else {
                x -= (annotation_width - span * 1024) >> 1;
                i64::from(self.options.ruby_size) * 1024
            };
            for glyph in &mut self.glyphs[first..] {
                glyph.position[0] = coordinate(x >> 10)?;
                x += step;
            }
        }
        Ok(())
    }
}

/// CMVS 266/338's dialogue parser. The host callback selects proportional
/// advances; without it the native fixed-width policy applies. Reveal effects
/// themselves belong to the caller; records identify only native pending glyphs.
/// Parsing is transactional, including clear controls and measurement failures.
pub(crate) fn append(
    surface: &mut CmvsNativeTextSurface,
    source: &str,
    options: DialogueLayoutOptions,
    boundary: Option<&CmvsEffectTextSurface>,
    mut measure: Measure<'_>,
) -> Result<Vec<DialogueReveal>, CoreError> {
    if source.is_empty() {
        return Ok(Vec::new());
    }
    if !(1..=4096).contains(&surface.size)
        || surface.rect[2] <= 0
        || ((options.ruby_enabled || options.reserve_ruby)
            && !(1..=4096).contains(&options.ruby_size))
        || surface.glyphs.len() > MAX_GLYPHS
    {
        return Err(failure());
    }
    let (bytes, _, invalid_encoding) = encoding_rs::SHIFT_JIS.encode(source);
    if invalid_encoding || bytes.len() > MAX_GLYPHS * 4 || bytes.contains(&0) {
        return Err(failure());
    }
    let lines = if options.ruby_enabled {
        ruby_lines(surface, &bytes)?
    } else {
        [false; MAX_RUBY_LINES]
    };
    let mut layout = Layout {
        surface,
        options,
        cursor: surface.cursor,
        glyphs: Vec::new(),
        reveals: Vec::new(),
        offset: surface.glyphs.len(),
        cleared: false,
        line: 0,
        ruby_lines: lines,
        delay: 0,
        suppressed: false,
        boundary,
        line_boundary: measurement::LineBoundary::default(),
    };
    if options.ruby_enabled {
        layout.reserve()?;
    }
    let mut index = 0;
    while index < bytes.len() {
        match next(&bytes, &mut index)? {
            Token::Glyph(value) => layout.ordinary(value, &mut measure)?,
            Token::Literal(value) => layout.ordinary(&[value], &mut measure)?,
            Token::Newline => layout.newline()?,
            Token::Clear => {
                layout.glyphs.clear();
                layout.reveals.clear();
                layout.cursor = [surface.rect[0], surface.rect[1]];
                layout.offset = 0;
                layout.cleared = true;
            }
            Token::ResetDelay => layout.delay = 0,
            Token::Suppress(value) => {
                layout.suppressed = value;
                if value {
                    layout.delay = 0;
                }
            }
            Token::Ruby { base, annotation } => layout.ruby(base, annotation, &mut measure)?,
        }
    }
    let Layout {
        cursor,
        glyphs,
        reveals,
        cleared,
        ..
    } = layout;
    if cleared {
        surface.glyphs.clear();
    }
    surface.cursor = cursor;
    surface.glyphs.extend(glyphs);
    Ok(reveals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface() -> CmvsNativeTextSurface {
        CmvsNativeTextSurface {
            font: "Synthetic".into(),
            capacity_hint: 32,
            visible: true,
            rect: [10, 20, 200, 300],
            cursor: [10, 20],
            size: 20,
            character_spacing: 2,
            line_spacing: 4,
            colors: [0x123456, 0xabcdef],
            kinsoku: true,
            bold: false,
            style: 3,
            color_mode: 2,
            position: [0, 0],
            physical_size: [1280, 720],
            virtual_size: [1280, 720],
            draw_order: 1,
            glyph_interval_ms: 30,
            reserve_ruby: false,
            reveal_defaults: [0; 2],
            glyphs: Vec::new(),
        }
    }

    fn options() -> DialogueLayoutOptions {
        DialogueLayoutOptions {
            ruby_enabled: true,
            ruby_size: 10,
            ruby_spacing: 0,
            reserve_ruby: false,
            interval_ms: 30,
        }
    }

    #[test]
    fn cp932_literals_and_glyph_style() {
        let mut s = surface();
        let records = append(&mut s, "漢Aｱ\\@\\{\\}\\%\\\"", options(), None, None).unwrap();
        assert_eq!(
            s.glyphs.iter().map(|g| g.text.as_str()).collect::<String>(),
            "漢Aｱ\\{}%\""
        );
        assert_eq!(s.glyphs[1].position, [32, 20]);
        assert_eq!(s.glyphs[2].position, [44, 20]);
        assert_eq!(
            records[0],
            DialogueReveal {
                glyph_index: 0,
                delay_ms: 0
            }
        );
        assert_eq!(records[7].delay_ms, 210);
        for g in &s.glyphs {
            assert_eq!(
                (&g.font, g.colors, g.style, g.color_mode),
                (&s.font, s.colors, 3, 2)
            );
        }
    }

    #[test]
    fn wrap_hanging_punctuation_narrow_mark_and_newline() {
        let mut s = surface();
        s.rect[2] = 64;
        append(&mut s, "漢字、文゛\\n次", options(), None, None).unwrap();
        assert_eq!(s.glyphs[2].position, [54, 20]);
        assert_eq!(s.glyphs[3].position, [10, 44]);
        assert_eq!(s.glyphs[4].position, [28, 44]);
        assert_eq!(s.glyphs[5].position, [10, 68]);
        assert_eq!(s.cursor, [32, 68]);
        let mut s = surface();
        s.rect[2] = 64;
        s.kinsoku = false;
        append(&mut s, "漢字、", options(), None, None).unwrap();
        assert_eq!(s.glyphs[2].position, [10, 44]);
    }

    #[test]
    fn timing_suppression_reset_and_clear_preserve_native_clock() {
        let mut s = surface();
        append(&mut s, "前", options(), None, None).unwrap();
        let r = append(
            &mut s,
            "甲乙\\t丙\\wc丁戊\\ws己庚\\c辛壬",
            options(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            s.glyphs.iter().map(|g| g.text.as_str()).collect::<String>(),
            "辛壬"
        );
        assert_eq!(s.glyphs[0].position, [10, 20]);
        assert_eq!(
            r,
            vec![
                DialogueReveal {
                    glyph_index: 0,
                    delay_ms: 60
                },
                DialogueReveal {
                    glyph_index: 1,
                    delay_ms: 90
                },
            ]
        );
        let mut s = surface();
        let r = append(&mut s, "甲\\wc乙\\ws丙\\t丁", options(), None, None).unwrap();
        assert_eq!(
            r,
            vec![
                DialogueReveal {
                    glyph_index: 0,
                    delay_ms: 0
                },
                DialogueReveal {
                    glyph_index: 2,
                    delay_ms: 0
                },
                DialogueReveal {
                    glyph_index: 3,
                    delay_ms: 0
                },
            ]
        );
    }

    #[test]
    fn immediate_text_has_no_pending_reveal_records() {
        let mut s = surface();
        let mut o = options();
        o.interval_ms = 0;
        assert!(
            append(&mut s, "甲{漢/かん}乙", o, None, None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(s.glyphs.len(), 5);
    }

    #[test]
    fn fixed_ruby_reserves_line_and_runs_parallel_reveal_clock() {
        let mut s = surface();
        let r = append(&mut s, "前{漢字/かんじ}後", options(), None, None).unwrap();
        assert_eq!(
            s.glyphs.iter().map(|g| g.position).collect::<Vec<_>>(),
            vec![
                [10, 30],
                [32, 30],
                [54, 30],
                [34, 20],
                [48, 20],
                [63, 20],
                [76, 30]
            ]
        );
        assert_eq!(s.glyphs[3].size, 10);
        assert_eq!(
            r.iter().map(|r| r.delay_ms).collect::<Vec<_>>(),
            vec![0, 30, 90, 30, 60, 120, 120]
        );
        assert_eq!(s.cursor, [98, 30]);
    }

    #[test]
    fn fixed_ruby_overhang_spacing_and_disabled_annotations() {
        let mut s = surface();
        let mut o = options();
        o.ruby_spacing = 1024;
        append(&mut s, "{漢字/かん}", o, None, None).unwrap();
        assert_eq!(s.glyphs[2].position, [16, 20]);
        assert_eq!(s.glyphs[3].position, [39, 20]);
        let mut s = surface();
        append(&mut s, "{漢/かんじ}", o, None, None).unwrap();
        assert_eq!(s.glyphs[1].position, [6, 20]);
        assert_eq!(s.glyphs[2].position, [16, 20]);
        assert_eq!(s.glyphs[3].position, [26, 20]);
        let mut s = surface();
        o.ruby_enabled = false;
        let r = append(&mut s, "{漢字/かんじ}後", o, None, None).unwrap();
        assert_eq!(s.glyphs.len(), 3);
        assert_eq!(s.glyphs[0].position, [10, 20]);
        assert_eq!(r[1].delay_ms, 60);
        assert_eq!(r[2].delay_ms, 90);
    }

    #[test]
    fn proportional_ruby_centering_and_emphasis_dots() {
        let mut s = surface();
        let mut measure = |text: &str, size: u32| -> Result<u32, CoreError> {
            Ok(if size == 10 {
                if text == "か" { 7 } else { 5 }
            } else if text == "漢" {
                16
            } else {
                24
            })
        };
        append(&mut s, "{漢字/かん}", options(), None, Some(&mut measure)).unwrap();
        assert_eq!(s.glyphs[0].position, [10, 30]);
        assert_eq!(s.glyphs[1].position, [28, 30]);
        assert_eq!(s.glyphs[2].position, [26, 20]);
        assert_eq!(s.glyphs[3].position, [33, 20]);
        let mut s = surface();
        append(&mut s, "{漢字/・・}", options(), None, Some(&mut measure)).unwrap();
        assert_eq!(s.glyphs[2].position, [16, 20]);
        assert_eq!(s.glyphs[3].position, [38, 20]);
        assert_eq!(s.cursor, [54, 30]);
    }

    #[test]
    fn reservation_census_tracks_wraps_and_explicit_lines() {
        let mut s = surface();
        s.rect[2] = 64;
        append(&mut s, "漢字{文/ぶん}\\n次", options(), None, None).unwrap();
        assert_eq!(s.glyphs[0].position, [10, 20]);
        assert_eq!(s.glyphs[2].position, [10, 54]);
        assert_eq!(s.glyphs[5].position, [10, 78]);
        let mut s = surface();
        let mut o = options();
        o.reserve_ruby = true;
        append(&mut s, "甲\\n乙", o, None, None).unwrap();
        assert_eq!(s.glyphs[0].position, [10, 30]);
        assert_eq!(s.glyphs[1].position, [10, 64]);
        let mut s = surface();
        o.ruby_enabled = false;
        append(&mut s, "甲\\n乙", o, None, None).unwrap();
        // With ruby disabled native reserves on newline, but not at entry.
        assert_eq!(s.glyphs[0].position, [10, 20]);
        assert_eq!(s.glyphs[1].position, [10, 54]);
    }

    #[test]
    fn mixed_ruby_preserves_ignored_byte_timing_and_valid_previous_glyphs() {
        let mut s = surface();
        let r = append(&mut s, "前{A漢B/かxん}後", options(), None, None).unwrap();
        assert_eq!(
            s.glyphs.iter().map(|g| g.text.as_str()).collect::<String>(),
            "前漢かん後"
        );
        assert_eq!(
            r.iter().map(|r| r.delay_ms).collect::<Vec<_>>(),
            vec![30, 120, 60, 120, 150]
        );
        assert_eq!(s.glyphs[2].position, [32, 20]);
        assert_eq!(s.glyphs[3].position, [43, 20]);
        let mut s = surface();
        append(&mut s, "前", options(), None, None).unwrap();
        let r = append(&mut s, "{A/か}", options(), None, None).unwrap();
        assert_eq!(
            r[0],
            DialogueReveal {
                glyph_index: 0,
                delay_ms: 30
            }
        );
        assert_eq!(s.glyphs[1].position, [27, 20]);
        for source in ["{A/かん}", "{/かん}"] {
            assert!(append(&mut surface(), source, options(), None, None).is_err());
        }
        // Suppressed/immediate native helpers do not access a pending glyph.
        let mut o = options();
        o.interval_ms = 0;
        assert!(append(&mut surface(), "{A/かん}", o, None, None).is_ok());
        assert!(append(&mut surface(), "\\wc{A/かん}", options(), None, None).is_ok());
    }

    #[test]
    fn malformed_or_unrepresentable_input_is_transactional() {
        for source in [
            "\\",
            "\\q",
            "\\w",
            "\\wx",
            "{漢}",
            "{漢/}",
            "{漢/a}",
            "{漢/かん",
            "😀",
            "甲\0乙",
        ] {
            let mut s = surface();
            append(&mut s, "前", options(), None, None).unwrap();
            let before = s.clone();
            assert!(
                append(&mut s, source, options(), None, None).is_err(),
                "{source:?}"
            );
            assert_eq!(s, before);
        }
        let mut s = surface();
        let source = format!("{{{}/かん}}", "漢".repeat(33));
        assert!(append(&mut s, &source, options(), None, None).is_err());
        let before = s.clone();
        let mut calls = 0;
        let mut measure = |_: &str, _: u32| -> Result<u32, CoreError> {
            calls += 1;
            if calls == 2 { Err(failure()) } else { Ok(20) }
        };
        assert!(append(&mut s, "甲\\c乙", options(), None, Some(&mut measure)).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn bounds_and_existing_glyph_indices() {
        let mut s = surface();
        append(&mut s, "甲", options(), None, None).unwrap();
        assert_eq!(
            append(&mut s, "乙", options(), None, None).unwrap()[0].glyph_index,
            1
        );
        s.cursor[0] = i32::MAX;
        s.rect[0] = i32::MAX;
        assert!(append(&mut s, "丙", options(), None, None).is_err());
        let mut s = surface();
        assert!(append(&mut s, &"\\n".repeat(32), options(), None, None).is_err());
        let mut o = options();
        o.ruby_enabled = false;
        assert!(append(&mut s, &"\\n".repeat(32), o, None, None).is_ok());
    }
}
