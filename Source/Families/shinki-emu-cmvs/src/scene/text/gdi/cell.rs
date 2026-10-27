use super::*;

// 461840's four little-endian immediate DWORDs followed by byte FF.
const ALTERNATE: [u8; 17] = [
    0, 31, 60, 87, 112, 135, 156, 175, 192, 207, 220, 231, 240, 247, 252, 254, 255,
];

pub(super) struct Cell {
    pub bitmap: GlyphBitmap,
    pub offset: [i32; 2],
    pub advance: u32,
    pub empty: bool,
}

// 461840 copies the top-left of the GDI black box, not a bearing-adjusted
// source subrectangle. Negative bearings are carried by the descriptor.
pub(super) fn compose(
    size: u32,
    code: u16,
    gm: &GLYPHMETRICS,
    tm: &TEXTMETRICA,
    source: &[u8],
    settings: NativeGdiSettings,
) -> Result<Cell, CoreError> {
    let size_i = i32::try_from(size).map_err(|_| error("SHINKI_EMU_CMVS_GDI_SIZE"))?;
    let raw_x = gm.gmptGlyphOrigin.x;
    let raw_y = tm.tmAscent - tm.tmInternalLeading / 2 - gm.gmptGlyphOrigin.y;
    let ink_x = raw_x.max(0);
    let mut ink_y = raw_y.max(0);
    let width = i64::from(gm.gmBlackBoxX)
        .min(i64::from(size_i) - i64::from(ink_x))
        .max(0) as u32;
    let mut height = if settings.proportional && code <= 255 {
        gm.gmBlackBoxY.min(size)
    } else {
        i64::from(gm.gmBlackBoxY)
            .min(i64::from(size_i) - i64::from(ink_y))
            .max(0) as u32
    };
    let mut offset = [0, 0];
    let advance = if settings.proportional {
        offset[0] = raw_x.min(0);
        if code > 255 {
            offset[1] = raw_y.min(0);
        } else if i64::from(ink_y) + i64::from(height) > i64::from(size) {
            offset[1] = (i64::from(ink_y) + i64::from(height) - i64::from(size)) as i32;
            ink_y = size_i - height as i32;
        }
        u32::from(gm.gmCellIncX as u16)
    } else if code <= 255 {
        (ink_x as u32 + width) & 0xffff
    } else {
        size & 0xffff
    };
    // A zero-byte space still has real GDI metrics; it never needs an upload.
    if source.is_empty() {
        height = 0;
        offset[1] = 0;
        if settings.proportional {
            offset[0] = 0;
        }
    }
    let count = (size as usize)
        .checked_mul(size as usize)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_SIZE"))?;
    let mut pixels = vec![0; count];
    let stride = gm
        .gmBlackBoxX
        .checked_add(3)
        .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?
        & !3;
    if height != 0 && width != 0 {
        let required = (height as usize - 1)
            .checked_mul(stride as usize)
            .and_then(|n| n.checked_add(width as usize))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?;
        if source.len() < required {
            return Err(error("SHINKI_EMU_CMVS_GDI_BITMAP"));
        }
        for y in 0..height as usize {
            let start = (ink_y as usize + y) * size as usize + ink_x as usize;
            let row = pixels
                .get_mut(start..start + width as usize)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?;
            for (target, value) in row
                .iter_mut()
                .zip(&source[y * stride as usize..][..width as usize])
            {
                *target = if settings.alternate_coverage {
                    ALTERNATE
                        .get(*value as usize)
                        .copied()
                        .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?
                } else if *value == 0 {
                    0
                } else {
                    value.wrapping_mul(16).wrapping_sub(1)
                };
            }
        }
    }
    let empty = pixels.iter().all(|value| *value == 0);
    let bitmap = GlyphBitmap::from_vec(size, size, GlyphBitmapFormat::Alpha8, pixels)
        .map_err(|_| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?;
    // Native descriptor fields are signed WORDs.
    offset = offset.map(|value| i32::from(value as i16));
    Ok(Cell {
        bitmap,
        offset,
        advance,
        empty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(proportional: bool, alternate_coverage: bool) -> NativeGdiSettings {
        NativeGdiSettings {
            proportional,
            alternate_coverage,
        }
    }

    #[test]
    fn dbcs_code_is_big_endian_and_halfwidth_is_one_byte() {
        assert_eq!(code("そ").unwrap(), 0x82bb);
        assert_eq!(code("ｱ").unwrap(), 0xb1);
        assert!(code("AB").is_err());
        assert!(code("🦀").is_err());
    }

    #[test]
    fn dword_stride_and_right_bottom_clip_preserve_source_origin() {
        let gm = GLYPHMETRICS {
            gmBlackBoxX: 3,
            gmBlackBoxY: 3,
            gmptGlyphOrigin: windows::Win32::Foundation::POINT { x: 2, y: 2 },
            gmCellIncX: 7,
            ..Default::default()
        };
        let tm = TEXTMETRICA {
            tmAscent: 4,
            ..Default::default()
        };
        let result = compose(
            4,
            0x82bb,
            &gm,
            &tm,
            &[1, 2, 3, 99, 4, 5, 6, 99, 7, 8, 9, 99],
            settings(false, false),
        )
        .unwrap();
        assert_eq!(
            result.bitmap.pixels.as_slice(),
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15, 31, 0, 0, 63, 79]
        );
        assert_eq!(result.advance, 4);
        assert_eq!(result.offset, [0, 0]);
    }

    #[test]
    fn proportional_ascii_descender_moves_cell_up_and_descriptor_down() {
        let gm = GLYPHMETRICS {
            gmBlackBoxX: 1,
            gmBlackBoxY: 3,
            gmptGlyphOrigin: windows::Win32::Foundation::POINT { x: -1, y: 1 },
            gmCellIncX: 2,
            ..Default::default()
        };
        let tm = TEXTMETRICA {
            tmAscent: 4,
            ..Default::default()
        };
        let result = compose(
            4,
            b'g' as u16,
            &gm,
            &tm,
            &[16, 0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0],
            settings(true, false),
        )
        .unwrap();
        assert_eq!(result.offset, [-1, 2]);
        assert_eq!(result.advance, 2);
        assert_eq!(
            result.bitmap.pixels.as_slice(),
            &[0, 0, 0, 0, 255, 0, 0, 0, 127, 0, 0, 0, 15, 0, 0, 0]
        );
    }

    #[test]
    fn alternate_levels_and_negative_dbcs_bearings_are_exact() {
        let gm = GLYPHMETRICS {
            gmBlackBoxX: 17,
            gmBlackBoxY: 1,
            gmptGlyphOrigin: windows::Win32::Foundation::POINT { x: -2, y: 8 },
            gmCellIncX: 18,
            ..Default::default()
        };
        let tm = TEXTMETRICA {
            tmAscent: 7,
            ..Default::default()
        };
        let source: Vec<u8> = (0..17).chain([90, 90, 90]).collect();
        let result = compose(17, 0x82bb, &gm, &tm, &source, settings(true, true)).unwrap();
        assert_eq!(result.offset, [-2, -1]);
        assert_eq!(&result.bitmap.pixels.as_slice()[..17], &ALTERNATE);
    }

    #[test]
    fn blank_space_keeps_advance_without_uploadable_ink() {
        let gm = GLYPHMETRICS {
            gmCellIncX: 14,
            ..Default::default()
        };
        let result = compose(
            27,
            32,
            &gm,
            &TEXTMETRICA::default(),
            &[],
            settings(true, false),
        )
        .unwrap();
        assert_eq!(result.advance, 14);
        assert!(result.empty);
    }
}
