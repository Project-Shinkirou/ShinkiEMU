use super::*;

fn failure() -> CoreError {
    invalid(
        "SHINKI_EMU_CMVS_TEXT_METRICS",
        "invalid native text measurement",
    )
}

/// `463CC0` reports virtual-pixel advances; `463750` reports half-em units.
/// Both restore the caller's cursor and count lines without creating glyphs.
pub(crate) fn measure(
    surface: &CmvsNativeTextSurface,
    source: &str,
    boundary: Option<&CmvsEffectTextSurface>,
    mut proportional: Measure<'_>,
) -> Result<[u32; 3], CoreError> {
    let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(source);
    if malformed || bytes.contains(&0) || bytes.len() > 65_535 * 4 {
        return Err(failure());
    }
    let mut cursor = MetricCursor {
        surface,
        boundary: boundary.filter(|value| value.boundaries.is_some()),
        x: surface.cursor[0] as u32,
        lines: 1,
        maximum: 0,
        last: 0,
        boundary_cursor: LineBoundary::default(),
    };
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                let control = *bytes.get(index + 1).ok_or_else(failure)?;
                index += 2;
                match control {
                    b'n' => {
                        cursor.newline();
                        cursor.last = 0;
                    }
                    b'c' | b't' => {}
                    b'w' => {
                        bytes.get(index).ok_or_else(failure)?;
                        index += 1;
                    }
                    b'"' | b'%' | b'@' | b'{' | b'}' => {
                        let literal = if control == b'@' { b'\\' } else { control };
                        cursor.before(u16::from(literal), true);
                        cursor.emit(&[literal], &mut proportional)?;
                    }
                    _ => return Err(failure()),
                }
            }
            b'{' => {
                index += 1;
                cursor.before(0, false);
                loop {
                    let byte = *bytes.get(index).ok_or_else(failure)?;
                    if byte == b'/' {
                        index += 1;
                        break;
                    }
                    let length = encoded_length(byte);
                    let value = bytes.get(index..index + length).ok_or_else(failure)?;
                    if length == 2 {
                        cursor.emit(value, &mut proportional)?;
                    }
                    index += length;
                }
                loop {
                    let byte = *bytes.get(index).ok_or_else(failure)?;
                    if byte == b'}' {
                        index += 1;
                        break;
                    }
                    let length = encoded_length(byte);
                    bytes.get(index..index + length).ok_or_else(failure)?;
                    index += length;
                }
            }
            byte => {
                let length = encoded_length(byte);
                let value = bytes.get(index..index + length).ok_or_else(failure)?;
                let code = if length == 2 {
                    u16::from_be_bytes([value[0], value[1]])
                } else {
                    u16::from(byte)
                };
                cursor.before(code, length == 1);
                cursor.emit(value, &mut proportional)?;
                index += length;
            }
        }
    }
    Ok([cursor.maximum, cursor.lines, cursor.last])
}

fn encoded_length(byte: u8) -> usize {
    if (0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte) {
        2
    } else {
        1
    }
}

struct MetricCursor<'a> {
    surface: &'a CmvsNativeTextSurface,
    boundary: Option<&'a CmvsEffectTextSurface>,
    x: u32,
    lines: u32,
    maximum: u32,
    last: u32,
    boundary_cursor: LineBoundary,
}

impl MetricCursor<'_> {
    fn newline(&mut self) {
        self.x = self.surface.rect[0] as u32;
        self.lines = self.lines.wrapping_add(1);
        self.boundary_cursor = LineBoundary::default();
    }

    fn before(&mut self, code: u16, single_byte: bool) {
        if self
            .boundary_cursor
            .wrap(self.surface, self.boundary, self.x, code, single_byte)
        {
            self.newline();
            if self.boundary.is_some() {
                self.last = 0;
            }
        }
    }

    fn emit(&mut self, bytes: &[u8], proportional: &mut Measure<'_>) -> Result<(), CoreError> {
        let units = bytes.len() as u32;
        let width = if let Some(measure) = proportional {
            let text = encoding_rs::SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(bytes)
                .ok_or_else(failure)?;
            measure(&text)?
        } else if units == 2 {
            self.surface.size
        } else {
            self.surface.size / 2
        };
        let advance = width.wrapping_add(self.surface.character_spacing);
        self.x = self.x.wrapping_add(advance);
        self.last = self.last.wrapping_add(if proportional.is_some() {
            advance
        } else {
            units
        });
        self.maximum = self.maximum.max(self.last);
        Ok(())
    }
}

/// Native 462F80 state is local to each parser call, not each line.
#[derive(Default)]
pub(super) struct LineBoundary {
    previous: u16,
    repetitions: u32,
}

impl LineBoundary {
    pub(super) fn wrap(
        &mut self,
        surface: &CmvsNativeTextSurface,
        boundary: Option<&CmvsEffectTextSurface>,
        x: u32,
        code: u16,
        single_byte: bool,
    ) -> bool {
        let threshold = (surface.rect[0] as u32)
            .wrapping_add(surface.rect[2] as u32)
            .wrapping_sub(surface.character_spacing)
            .wrapping_sub(surface.size);
        let boundary = boundary.filter(|value| value.boundaries.is_some());
        if x < threshold {
            if boundary.is_some() {
                self.previous = code;
            }
            return false;
        }
        if let Some(boundary) = boundary {
            if !single_byte && surface.kinsoku {
                let lists = boundary.boundaries.as_ref().unwrap();
                let contains = |bytes: &[u8]| {
                    bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .any(|pair| u16::from_be_bytes([pair[0], pair[1]]) == code)
                };
                // 462DB0 gives list zero precedence. List one only hangs
                // when it repeats the last code recorded before the edge.
                if contains(&lists[0]) || contains(&lists[1]) && self.previous == code {
                    self.repetitions = self.repetitions.wrapping_add(1);
                    if boundary.value >= self.repetitions {
                        return false;
                    }
                }
            }
        } else if !single_byte
            && surface.kinsoku
            && matches!(code, 0x8141 | 0x8142 | 0x8176 | 0x8178)
        {
            return false;
        }
        *self = Self::default();
        true
    }
}
