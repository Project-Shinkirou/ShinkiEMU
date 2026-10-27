use super::*;
use crate::jbp::{HuffmanTree, JBitStream, ZIGZAG_ORDER, cmv as transform, read_frequencies};

/// Native 405f20/407f10: tiled DCT traversal, not ordinary PB JBP.
/// The bitmap preserves previous macroblocks but still consumes their complete
/// coefficient records. CMV8 carries four additional full-resolution alpha blocks.
pub(super) fn decode(
    source: &[u8],
    metadata: CmvMetadata,
    has_previous: bool,
    output: &mut TextureFrame,
) -> Result<(), CoreError> {
    range(source, 0, 44)?;
    let expected_magic: &[u8] = if metadata.has_alpha { b"JBPE" } else { b"JBPD" };
    if &source[..4] != expected_magic {
        return Err(invalid("CMV frame has an unsupported DCT signature"));
    }
    let format = u32_at(source, 8)?;
    if (format >> 28) & 3 != 1 || format & 0x0800_0000 == 0 {
        return Err(invalid("CMV frame sampling or quantization is unsupported"));
    }
    if u32::from(u16_at(source, 16)?) != metadata.width
        || u32::from(u16_at(source, 18)?) != metadata.height
    {
        return Err(invalid("CMV frame dimensions disagree with its container"));
    }
    let width = metadata.width as usize;
    let height = metadata.height as usize;
    let aligned_width = width
        .checked_add(15)
        .map(|value| value & !15)
        .ok_or_else(|| invalid("CMV aligned width overflowed"))?;
    let aligned_height = height
        .checked_add(15)
        .map(|value| value & !15)
        .ok_or_else(|| invalid("CMV aligned height overflowed"))?;
    pixel_bytes(aligned_width as u32, aligned_height as u32)?;
    if output.rgba8.len() != pixel_bytes(metadata.width, metadata.height)? {
        return Err(invalid("CMV output surface has the wrong length"));
    }
    let blocks_x = aligned_width / 16;
    let blocks_y = aligned_height / 16;
    let macroblocks = blocks_x * blocks_y;
    let planes = if metadata.has_alpha { 10 } else { 6 };
    let table = (u32_at(source, 4)? as usize)
        .checked_add(4)
        .ok_or_else(|| invalid("CMV table offset overflowed"))?;
    if table < 44 {
        return Err(invalid("CMV tables overlap the frame header"));
    }
    let quant_count = if metadata.has_alpha { 3 } else { 2 };
    range(source, table, 144 + 64 * quant_count)?;
    let mut runs = [0_u8; 16];
    for (run, value) in runs.iter_mut().zip(&source[table + 128..table + 144]) {
        *run = value
            .checked_add(1)
            .ok_or_else(|| invalid("CMV AC run length overflowed"))?;
    }
    let tree_dc = HuffmanTree::new(runs, read_frequencies(source, table)?)?;
    let tree_ac = HuffmanTree::new(runs, read_frequencies(source, table + 64)?)?;
    let mut quant = [[0_i16; 64]; 3];
    for (plane, table_values) in quant.iter_mut().take(quant_count).enumerate() {
        for (value, byte) in table_values
            .iter_mut()
            .zip(&source[table + 144 + plane * 64..][..64])
        {
            *value = i16::from(*byte);
        }
    }
    let mut list_header = table + 144 + 64 * quant_count;
    let dc_count = macroblocks * planes;
    if metadata.has_alpha {
        if u32_at(source, list_header)? as usize != dc_count {
            return Err(invalid("CMV alpha DC count disagrees with its macroblocks"));
        }
        list_header += 4;
    }
    let tile_grid = u16_at(source, list_header)?;
    let tile_rows = usize::from(tile_grid & 255);
    let tile_cols = usize::from(tile_grid >> 8);
    let list_count = usize::from(u16_at(source, list_header + 2)?);
    if tile_rows != aligned_height.div_ceil(256)
        || tile_cols != aligned_width.div_ceil(256)
        || list_count != tile_rows * tile_cols
    {
        return Err(invalid("CMV tile grid disagrees with its dimensions"));
    }
    let list_start = list_header + 4;
    range(source, list_start, list_count * 2)?;
    for tile in 0..list_count {
        let extent = u16_at(source, list_start + tile * 2)?;
        let rows = (blocks_y - tile / tile_cols * 16).min(16);
        let cols = (blocks_x - tile % tile_cols * 16).min(16);
        if usize::from(extent & 255) != rows || usize::from(extent >> 8) != cols {
            return Err(invalid("CMV macroblock list does not cover its tile"));
        }
    }
    let bitmap_start = list_header + align4(4 + list_count * 2)?;
    let has_bitmap = format & 0x1000 != 0;
    let bitmap_bytes = if has_bitmap {
        align4(macroblocks.div_ceil(8))?
    } else {
        0
    };
    range(source, bitmap_start, bitmap_bytes)?;
    let bits_start = bitmap_start + bitmap_bytes;
    let dc_bytes = u32_at(source, 28)? as usize;
    let ac_bytes = u32_at(source, 32)? as usize;
    let ac_start = bits_start
        .checked_add(dc_bytes)
        .ok_or_else(|| invalid("CMV DC range overflowed"))?;
    let mut dc_bits = JBitStream::new(source, bits_start, dc_bytes, "SHINKI_EMU_CMVS_CMV_DC")?;
    let mut ac_bits = JBitStream::new(source, ac_start, ac_bytes, "SHINKI_EMU_CMVS_CMV_AC")?;
    let mut dc = Vec::with_capacity(dc_count);
    let mut previous = 0_i16;
    for _ in 0..dc_count {
        let count = tree_dc.read(&mut dc_bits)?;
        previous = previous.wrapping_add(amplitude(&mut dc_bits, count)?);
        dc.push(previous);
    }
    let mut macroblock = 0;
    for tile in 0..list_count {
        let extent = u16_at(source, list_start + tile * 2)?;
        let tile_x = tile % tile_cols * 256;
        let tile_y = tile / tile_cols * 256;
        for row in 0..usize::from(extent & 255) {
            for col in 0..usize::from(extent >> 8) {
                let inherit = has_bitmap
                    && source[bitmap_start + macroblock / 8] & (128 >> (macroblock & 7)) != 0;
                if inherit && !has_previous {
                    return Err(invalid(
                        "CMV initial frame inherits an uninitialized macroblock",
                    ));
                }
                let mut blocks = [[0_i16; 64]; 10];
                for plane in 0..planes {
                    let block = &mut blocks[plane];
                    block[0] = dc[macroblock * planes + plane];
                    decode_ac(&tree_ac, &mut ac_bits, &runs, block)?;
                    if !inherit {
                        let quant_plane = if plane < 4 {
                            0
                        } else if plane < 6 {
                            1
                        } else {
                            2
                        };
                        transform::inverse_dct(block, &quant[quant_plane]);
                    }
                }
                if !inherit {
                    paint(
                        output.rgba8.make_mut_for_update(),
                        metadata,
                        [tile_x + col * 16, tile_y + row * 16],
                        &blocks,
                    );
                }
                macroblock += 1;
            }
        }
    }
    Ok(())
}

fn amplitude(bits: &mut JBitStream<'_>, count: usize) -> Result<i16, CoreError> {
    if count == 0 {
        return Ok(0);
    }
    let raw = bits.get_bits(count)? as i32;
    Ok(if raw < 1 << (count - 1) {
        raw - (1 << count) + 1
    } else {
        raw
    } as i16)
}

pub(super) fn decode_ac(
    tree: &HuffmanTree,
    bits: &mut JBitStream<'_>,
    runs: &[u8; 16],
    block: &mut [i16; 64],
) -> Result<(), CoreError> {
    let mut coefficient = 0;
    while coefficient < 63 {
        let count = tree.read(bits)?;
        if count == 15 {
            break;
        }
        if count == 0 {
            let mut run = 0;
            while bits.next_bit()? != 0 {
                run += 1;
                if run >= runs.len() {
                    return Err(invalid("CMV AC run selector exceeds its table"));
                }
            }
            coefficient += usize::from(runs[run]);
            if coefficient > 63 {
                return Err(invalid("CMV AC run exceeds its block"));
            }
        } else {
            block[ZIGZAG_ORDER[coefficient]] = amplitude(bits, count)?;
            coefficient += 1;
        }
    }
    Ok(())
}

fn paint(output: &mut [u8], metadata: CmvMetadata, [x, y]: [usize; 2], blocks: &[[i16; 64]; 10]) {
    let width = metadata.width as usize;
    let height = metadata.height as usize;
    for dy in 0..16.min(height.saturating_sub(y)) {
        for dx in 0..16.min(width.saturating_sub(x)) {
            let plane = dy / 8 * 2 + dx / 8;
            let pixel = dy % 8 * 8 + dx % 8;
            let chroma = dy / 2 * 8 + dx / 2;
            let alpha = if metadata.has_alpha {
                blocks[6 + plane][pixel]
            } else {
                255
            };
            let rgba = transform::rgba(
                blocks[plane][pixel],
                blocks[4][chroma],
                blocks[5][chroma],
                alpha,
            );
            let offset = ((y + dy) * width + x + dx) * 4;
            output[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
}
