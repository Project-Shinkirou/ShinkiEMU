use super::{CmvsNativeGlyphGraph, CmvsNativeGlyphRecord};
use shinki_emu_sdk::CoreError;

fn malformed() -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_CMVS_GLYPH_TEXT",
        "native standalone glyph text has a truncated character, unknown escape, or unterminated markup",
    )
}

#[derive(Clone, Copy)]
enum Token {
    Newline,
    Brace,
    Glyph {
        code: u16,
        double: bool,
        brace: bool,
    },
}

fn double(byte: u8) -> bool {
    matches!(byte, 0x81..=0x9f | 0xe0..=0xfc)
}

// 41A3A0 is a byte classifier, not a Unicode/CP932 validity check.
fn character(source: &[u8], index: &mut usize) -> Result<(u16, bool), CoreError> {
    let first = *source.get(*index).ok_or_else(malformed)?;
    *index += 1;
    if double(first) {
        let second = *source.get(*index).ok_or_else(malformed)?;
        *index += 1;
        Ok((u16::from_be_bytes([first, second]), true))
    } else {
        Ok(((first as i8 as i16) as u16, false))
    }
}

fn parse(
    source: &[u8],
    mut consume: impl FnMut(Token) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    let source = &source[..source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len())];
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\\' => {
                let escape = *source.get(index + 1).ok_or_else(malformed)?;
                index += 2;
                match escape {
                    b'n' => consume(Token::Newline)?,
                    b'c' | b't' => {}
                    b'w' => {
                        source.get(index).ok_or_else(malformed)?;
                        index += 1;
                    }
                    b'"' | b'%' | b'{' | b'}' | b'@' => consume(Token::Glyph {
                        code: u16::from(if escape == b'@' { b'\\' } else { escape }),
                        double: false,
                        brace: false,
                    })?,
                    _ => return Err(malformed()),
                }
            }
            b'{' => {
                index += 1;
                consume(Token::Brace)?;
                loop {
                    if *source.get(index).ok_or_else(malformed)? == b'/' {
                        index += 1;
                        break;
                    }
                    let (code, double) = character(source, &mut index)?;
                    if double {
                        consume(Token::Glyph {
                            code,
                            double,
                            brace: true,
                        })?;
                    }
                }
                loop {
                    if *source.get(index).ok_or_else(malformed)? == b'}' {
                        index += 1;
                        break;
                    }
                    character(source, &mut index)?;
                }
            }
            _ => {
                let (code, double) = character(source, &mut index)?;
                consume(Token::Glyph {
                    code,
                    double,
                    brace: false,
                })?;
            }
        }
    }
    Ok(())
}

fn hanging(code: u16) -> bool {
    matches!(code, 0x8141 | 0x8142 | 0x8176 | 0x8178)
}

fn threshold(graph: &CmvsNativeGlyphGraph) -> f32 {
    (f32::from_bits(graph.rect[2]) + f32::from_bits(graph.rect[0]))
        - (graph.size as f32 + f32::from_bits(graph.gaps[0]))
}

fn newline(graph: &mut CmvsNativeGlyphGraph) {
    graph.cursor[0] = graph.rect[0];
    graph.cursor[1] = ((graph.size as f32 + f32::from_bits(graph.gaps[1]))
        + f32::from_bits(graph.cursor[1]))
    .to_bits();
}

fn before(graph: &mut CmvsNativeGlyphGraph, code: u16, double: bool) {
    if f32::from_bits(graph.cursor[0]) >= threshold(graph)
        && !(double && graph.kinsoku && hanging(code))
    {
        newline(graph);
    }
}

fn cvtt(value: f32) -> i32 {
    if !value.is_finite() || !(-2147483648.0..2147483648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

// 43B360 stores the manager's advance and the scaled result in signed WORDs.
fn virtual_advance(graph: &CmvsNativeGlyphGraph, physical: u32) -> Result<i16, CoreError> {
    let physical = physical as i16;
    let m = graph.physical_size[0];
    let v = graph.virtual_size[0];
    if m == v {
        return Ok(physical);
    }
    let product = v.wrapping_mul((physical as i32) as u32);
    let quotient = product.checked_div(m).ok_or_else(|| {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_GLYPH_EXTENT",
            "native glyph physical width is zero",
        )
    })?;
    Ok((quotient as u16).wrapping_add(u16::from(!product.is_multiple_of(m))) as i16)
}

/// 907/43A790 retains descriptors and queries metrics; rasterization is deferred.
pub(crate) fn append(
    graph: &mut CmvsNativeGlyphGraph,
    source: &[u8],
    proportional: bool,
    mut measure: impl FnMut(u16, u32) -> Result<u32, CoreError>,
) -> Result<(), CoreError> {
    parse(source, |token| {
        match token {
            Token::Newline => newline(graph),
            Token::Brace => {
                if proportional {
                    before(graph, 0, true);
                }
            }
            Token::Glyph {
                code,
                double,
                brace,
            } => {
                if !brace || !proportional {
                    before(graph, code, double);
                }
                let mut position = graph.cursor.map(|word| cvtt(f32::from_bits(word)));
                let special = code == 0x814a && !brace;
                if special {
                    position[0] = position[0].wrapping_sub((graph.size / 5) as i32);
                }
                graph.records.push(CmvsNativeGlyphRecord {
                    code,
                    position,
                    size: graph.size,
                    colors: graph.colors,
                    style: graph.style,
                    outline_color: graph.outline_color,
                    color_mode: if matches!(graph.color_mode, 1..=3) {
                        graph.color_mode
                    } else {
                        0
                    },
                    painted: None,
                    resident: None,
                });
                let measured = measure(code, graph.scale(graph.size)?)?;
                let measured = virtual_advance(graph, measured)?;
                let advance = if special {
                    graph.size as f32 * 0.5 - graph.size as f32 / 5.0
                } else if proportional {
                    measured as f32
                } else if double {
                    graph.size as f32
                } else {
                    graph.size as f32 * 0.5
                };
                graph.cursor[0] = ((advance + f32::from_bits(graph.gaps[0]))
                    + f32::from_bits(graph.cursor[0]))
                .to_bits();
            }
        }
        Ok(())
    })
}

/// 912/439D90 always measures fixed halfwidth units and leaves the live cursor alone.
pub(crate) fn measure(graph: &CmvsNativeGlyphGraph, source: &[u8]) -> Result<[u32; 3], CoreError> {
    let mut x = f32::from_bits(graph.cursor[0]);
    let mut lines = 1u32;
    let mut maximum = 0u32;
    let mut last = 0u32;
    parse(source, |token| {
        match token {
            Token::Newline => {
                lines = lines.wrapping_add(1);
                x = f32::from_bits(graph.rect[0]);
                last = 0;
            }
            Token::Brace => {
                if x >= threshold(graph) {
                    lines = lines.wrapping_add(1);
                    x = f32::from_bits(graph.rect[0]);
                    last = 0;
                }
            }
            Token::Glyph {
                code,
                double,
                brace,
            } => {
                if !brace && x >= threshold(graph) && !(double && graph.kinsoku && hanging(code)) {
                    lines = lines.wrapping_add(1);
                    x = f32::from_bits(graph.rect[0]);
                    last = 0;
                }
                x += (graph.size as f32 * if double { 1.0 } else { 0.5 })
                    + f32::from_bits(graph.gaps[0]);
                last = last.wrapping_add(if double { 2 } else { 1 });
                maximum = maximum.max(last);
            }
        }
        Ok(())
    })?;
    Ok([lines, maximum, last])
}

/// 913/439C40 ignores ruby annotations and all control escapes.
pub(crate) fn units(source: &[u8]) -> Result<u32, CoreError> {
    let mut count = 0u32;
    parse(source, |token| {
        if let Token::Glyph { double, .. } = token {
            count = count.wrapping_add(if double { 2 } else { 1 });
        }
        Ok(())
    })?;
    Ok(count)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::vm::native_glyph::CmvsNativeGlyphAtlas;

    pub(crate) fn graph() -> CmvsNativeGlyphGraph {
        CmvsNativeGlyphGraph {
            identity: 1,
            font: "native".into(),
            bold: false,
            size: 21,
            capacity_hint: 1,
            virtual_size: [100, 60],
            physical_size: [200, 120],
            position: [0; 2],
            rect: [0.0, 0.0, 100.0, 60.0].map(f32::to_bits),
            viewport: [0.0, 0.0, 100.0, 60.0].map(f32::to_bits),
            cursor: [0; 2],
            gaps: [0; 2],
            colors: [0x123456, 0xabcdef],
            kinsoku: true,
            style: 9,
            outline_color: 0x112233,
            color_mode: 3,
            draw_order: 1,
            root: Default::default(),
            atlas: CmvsNativeGlyphAtlas {
                cell_size: 48,
                width: 512,
                height: 512,
                occupancy: vec![0; 1024],
            },
            records: Vec::new(),
            provider_generation: 0,
            auxiliary: None,
            secondary_reference: None,
            bounds: [-1000, 1000, -1000, 1000],
            motion: None,
        }
    }

    #[test]
    fn fixed_retains_style_and_queries_every_descriptor_including_signed_kana() {
        let mut graph = graph();
        let mut calls = Vec::new();
        append(&mut graph, b"A\xa6\x82\xa0", false, |code, size| {
            calls.push((code, size));
            Ok(17)
        })
        .unwrap();
        assert_eq!(calls, [(0x41, 42), (0xffa6, 42), (0x82a0, 42)]);
        assert_eq!(
            graph.records.iter().map(|r| r.position).collect::<Vec<_>>(),
            [[0, 0], [10, 0], [21, 0]]
        );
        assert_eq!(f32::from_bits(graph.cursor[0]), 42.0);
        let record = &graph.records[0];
        assert_eq!(
            (
                record.colors,
                record.style,
                record.outline_color,
                record.color_mode
            ),
            ([0x123456, 0xabcdef], 9, 0x112233, 3)
        );
        assert!(
            graph
                .records
                .iter()
                .all(|r| r.painted.is_none() && r.resident.is_none())
        );
    }

    #[test]
    fn markup_literals_controls_and_cstring_termination_count_native_halfwidths() {
        let text = b"A\\c\\t\\w3\\n\\\"\\%\\@\\{\\}{a\x82\xa0b\x82\xa2/ruby\x82\xa4}B\0ignored";
        assert_eq!(units(text).unwrap(), 11);
        let mut graph = graph();
        graph.rect[2] = 1000.0f32.to_bits();
        append(&mut graph, text, false, |_, _| Ok(9)).unwrap();
        assert_eq!(
            graph.records.iter().map(|r| r.code).collect::<Vec<_>>(),
            [65, 34, 37, 92, 123, 125, 0x82a0, 0x82a2, 66]
        );
        assert_eq!(measure(&graph, b"").unwrap(), [1, 0, 0]);
    }

    #[test]
    fn malformed_wire_reports_native_parser_boundary() {
        for source in [
            b"\\q".as_slice(),
            b"\\",
            b"\\w",
            b"\x82",
            b"{base}",
            b"{\x82\xa0/ruby",
            b"{\x82/ok}",
        ] {
            assert!(units(source).is_err(), "{source:?}");
        }
    }

    #[test]
    fn wrapping_hangs_only_four_kinsoku_codes_and_counts_lines_independently() {
        let mut graph = graph();
        graph.rect[2] = 42.0f32.to_bits();
        append(&mut graph, b"\x82\xa0\x81\x41\x82\xa2", false, |_, _| {
            Ok(42)
        })
        .unwrap();
        assert_eq!(
            graph.records.iter().map(|r| r.position).collect::<Vec<_>>(),
            [[0, 0], [21, 0], [0, 21]]
        );
        graph.cursor = [0; 2];
        assert_eq!(
            measure(&graph, b"\x82\xa0\x81\x41\x82\xa2").unwrap(),
            [2, 4, 2]
        );
        graph.kinsoku = false;
        assert_eq!(
            measure(&graph, b"\x82\xa0\x81\x41\x82\xa2").unwrap(),
            [3, 2, 2]
        );
        assert_eq!(graph.cursor, [0; 2]);
    }

    #[test]
    fn braces_wrap_each_fixed_base_but_only_once_for_measurement_and_proportional() {
        let mut fixed = graph();
        fixed.rect[2] = 42.0f32.to_bits();
        let text = b"{ignored\x82\xa0\x82\xa2\x82\xa4/ruby}";
        assert_eq!(measure(&fixed, text).unwrap(), [1, 6, 6]);
        let mut proportional = fixed.clone();
        append(&mut fixed, text, false, |_, _| Ok(42)).unwrap();
        append(&mut proportional, text, true, |_, _| Ok(42)).unwrap();
        assert_eq!(fixed.records[2].position, [0, 42]);
        assert_eq!(proportional.records[2].position, [42, 0]);
    }

    #[test]
    fn voiced_mark_and_proportional_ceil_keep_distinct_native_arithmetic() {
        let mut graph = graph();
        graph.rect[2] = 1000.0f32.to_bits();
        append(&mut graph, b"\x81\x4aA{\x81\x4a/r}", true, |_, _| Ok(17)).unwrap();
        assert_eq!(graph.records[0].position, [-4, 0]);
        assert_eq!(graph.records[1].position, [6, 0]);
        assert_eq!(graph.records[2].position, [15, 0]);
        assert_eq!(f32::from_bits(graph.cursor[0]), 24.3);
        graph.cursor = [0; 2];
        assert_eq!(measure(&graph, b"\x81\x4a").unwrap(), [1, 2, 2]);
    }

    #[test]
    fn append_is_not_limited_by_atlas_capacity_and_private_data_retains_descriptors() {
        let mut graph = graph();
        append(&mut graph, b"ABCD", false, |_, _| Ok(21)).unwrap();
        graph.style = 0;
        graph.colors = [0; 2];
        let bytes = postcard::to_allocvec(&graph).unwrap();
        let restored: CmvsNativeGlyphGraph = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored, graph);
        assert_eq!(restored.records.len(), 4);
        assert_eq!(restored.records[0].style, 9);
        assert_eq!(restored.records[0].colors, [0x123456, 0xabcdef]);
    }

    #[test]
    fn measurement_starts_at_live_cursor_counts_trailing_newline_and_ignores_annotation() {
        let mut graph = graph();
        graph.rect[0] = 5.0f32.to_bits();
        graph.rect[2] = 42.0f32.to_bits();
        graph.cursor = [26.0, 9.0].map(f32::to_bits);
        let cursor = graph.cursor;
        assert_eq!(
            measure(&graph, b"A\\n{a\x82\xa0b\x82\xa2/read}\\n").unwrap(),
            [4, 4, 0]
        );
        assert_eq!(graph.cursor, cursor);
        assert_eq!(units(b"\\n{ASCIIonly/ignored}\\n").unwrap(), 0);
    }

    #[test]
    fn appends_keep_cursor_and_old_descriptors_when_current_style_and_size_change() {
        let mut graph = graph();
        append(&mut graph, b"A", false, |_, _| Ok(21)).unwrap();
        graph.style = 0;
        graph.size = 10;
        graph.colors = [0, 0];
        graph.color_mode = 0x100;
        graph.gaps = [1.5, 2.5].map(f32::to_bits);
        append(&mut graph, b"B\\nC", false, |_, size| {
            assert_eq!(size, 20);
            Ok(10)
        })
        .unwrap();
        assert_eq!(graph.records[0].size, 21);
        assert_eq!(graph.records[0].style, 9);
        assert_eq!(graph.records[1].position, [10, 0]);
        assert_eq!(graph.records[1].colors, [0, 0]);
        assert_eq!(graph.records[1].color_mode, 0);
        assert_eq!(graph.records[2].position, [0, 12]);
        assert_eq!(graph.cursor.map(f32::from_bits), [6.5, 12.5]);
    }

    #[test]
    fn virtual_metrics_narrow_signed_words_and_use_wrapping_unsigned_products() {
        let mut graph = graph();
        assert_eq!(virtual_advance(&graph, 17).unwrap(), 9);
        assert_eq!(virtual_advance(&graph, u32::MAX).unwrap(), -20972);
        graph.virtual_size = graph.physical_size;
        assert_eq!(virtual_advance(&graph, 0x8001).unwrap(), -32767);
        graph.virtual_size = [100, 60];
        graph.physical_size[0] = 0;
        assert!(virtual_advance(&graph, 17).is_err());
    }
}
