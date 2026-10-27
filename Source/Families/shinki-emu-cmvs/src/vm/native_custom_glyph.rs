//! Native bitmap-font manager at main+3164 (413FC0..4143F9).
//! Sheets and glyphs are append-only until engine teardown. Native slot saves do
//! not serialize this manager; private VM snapshots retain the live allocations.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeCustomGlyphBitmap {
    pub identity: u64,
    pub size: u32,
    pub descriptor: u16,
    /// Native type-8 coverage, top-down, tightly packed; no GDI coverage LUT.
    pub coverage: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Sheet {
    resource: String,
    cell_size: u32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct Glyph {
    code: u16,
    bitmaps: Vec<CmvsNativeCustomGlyphBitmap>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeCustomGlyphManager {
    sheets: Vec<Sheet>,
    glyphs: Vec<Glyph>,
    next_identity: u64,
}
fn failure(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_CUSTOM_GLYPH_BOUNDS", message)
}

impl CmvsNativeCustomGlyphManager {
    pub(crate) fn load_sheet(
        &mut self,
        resource: String,
        cell_size: u32,
        width: u32,
        height: u32,
        coverage: Vec<u8>,
    ) -> Result<(), CoreError> {
        if (width as u64) * (height as u64) != coverage.len() as u64 {
            return Err(failure(
                "custom glyph sheet dimensions disagree with its coverage",
            ));
        }
        self.sheets.push(Sheet {
            resource,
            cell_size,
            width,
            height,
            coverage,
        });
        Ok(())
    }

    /// 413FC0: cell index is not the character code. Each loaded sheet supplies
    /// the same indexed square at its own cell size. Duplicates remain linked,
    /// and 4142A0 always chooses the first record for a character.
    pub(crate) fn register(&mut self, cell_index: u32, code: u16) -> Result<(), CoreError> {
        if self.sheets.len() > 16 {
            return Err(failure(
                "native custom glyph record has only sixteen bitmap slots",
            ));
        }
        // Validate the complete extraction before committing any allocation.
        for sheet in &self.sheets {
            let cell = sheet.cell_size;
            if cell == 0 || cell > u16::MAX as u32 || sheet.width / cell == 0 {
                return Err(failure("custom glyph sheet has an invalid cell size"));
            }
            let columns = sheet.width / cell;
            let x = (cell_index % columns) as u64 * cell as u64;
            let y = (cell_index / columns) as u64 * cell as u64;
            if x + cell as u64 > sheet.width as u64 || y + cell as u64 > sheet.height as u64 {
                return Err(failure("custom glyph cell index exceeds its source sheet"));
            }
        }
        let mut bitmaps = Vec::with_capacity(self.sheets.len());
        for sheet in &self.sheets {
            let cell = sheet.cell_size;
            let columns = sheet.width / cell;
            let x = cell_index % columns * cell;
            let y = cell_index / columns * cell;
            let mut coverage = Vec::with_capacity((cell as usize) * (cell as usize));
            for row in y..y + cell {
                let start = row as usize * sheet.width as usize + x as usize;
                coverage.extend_from_slice(&sheet.coverage[start..start + cell as usize]);
            }
            self.next_identity += 1;
            bitmaps.push(CmvsNativeCustomGlyphBitmap {
                identity: self.next_identity,
                size: cell,
                descriptor: cell as u16,
                coverage,
            });
        }
        self.glyphs.push(Glyph { code, bitmaps });
        Ok(())
    }

    /// 414160: change only the exact entry, not previously generated sizes.
    pub(crate) fn set_descriptor(&mut self, code: u16, size: u32, descriptor: u16) -> bool {
        let Some(glyph) = self.glyphs.iter_mut().find(|glyph| glyph.code == code) else {
            return false;
        };
        let Some(bitmap) = glyph.bitmaps.iter_mut().find(|bitmap| bitmap.size == size) else {
            return false;
        };
        bitmap.descriptor = descriptor;
        true
    }

    /// 4142A0: exact first; otherwise smallest larger size strictly below 1024.
    /// Derived bitmaps participate in subsequent searches and occupy one of the
    /// same sixteen slots. Custom lookup precedes the font-owner GDI cache.
    pub fn lookup(
        &mut self,
        code: u16,
        size: u32,
    ) -> Result<Option<&CmvsNativeCustomGlyphBitmap>, CoreError> {
        let Some(glyph) = self.glyphs.iter_mut().find(|glyph| glyph.code == code) else {
            return Ok(None);
        };
        if let Some(index) = glyph.bitmaps.iter().position(|bitmap| bitmap.size == size) {
            return Ok(Some(&glyph.bitmaps[index]));
        }
        if glyph.bitmaps.len() >= 16 {
            return Ok(None);
        }
        let Some(source) = glyph
            .bitmaps
            .iter()
            .filter(|bitmap| bitmap.size > size && bitmap.size < 1024)
            .min_by_key(|bitmap| bitmap.size)
        else {
            return Ok(None);
        };
        if size == 0 {
            return Err(failure(
                "native custom glyph scaler divides by a zero destination size",
            ));
        }
        let descriptor = (size * u32::from(source.descriptor) / source.size) as u16;
        let coverage = scale(&source.coverage, source.size, size);
        self.next_identity += 1;
        glyph.bitmaps.push(CmvsNativeCustomGlyphBitmap {
            identity: self.next_identity,
            size,
            descriptor,
            coverage,
        });
        Ok(glyph.bitmaps.last())
    }

    pub fn bitmap_by_identity(&self, identity: u64) -> Option<&CmvsNativeCustomGlyphBitmap> {
        self.glyphs
            .iter()
            .flat_map(|glyph| &glyph.bitmaps)
            .find(|bitmap| bitmap.identity == identity)
    }
}

/// 413E90 uses floor(source_size*1024/destination_size), a top-left origin,
/// and truncates the bilinear sum by 20 bits. Preserve its right-edge sample:
/// lower-right becomes upper-right when x is the final source column.
fn scale(source: &[u8], source_size: u32, size: u32) -> Vec<u8> {
    let step = (source_size << 10) / size;
    let mut output = Vec::with_capacity(size as usize * size as usize);
    for y in 0..size {
        let fy = y * step;
        let sy = fy >> 10;
        let wy = fy & 1023;
        for x in 0..size {
            let fx = x * step;
            let sx = fx >> 10;
            let wx = fx & 1023;
            let top = (sy * source_size + sx) as usize;
            let right = if sx >= source_size - 1 { top } else { top + 1 };
            let bottom = if sy >= source_size - 1 {
                top
            } else {
                top + source_size as usize
            };
            let bottom_right = if sy >= source_size - 1 || sx >= source_size - 1 {
                right
            } else {
                bottom + 1
            };
            let upper = (1024 - wx) * u32::from(source[top]) + wx * u32::from(source[right]);
            let lower =
                (1024 - wx) * u32::from(source[bottom]) + wx * u32::from(source[bottom_right]);
            output.push(((wy * lower + (1024 - wy) * upper) >> 20) as u8);
        }
    }
    output
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    command_id: u16,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    match (command_id, values) {
        (960, [(_, name), (_, size)]) => Ok(Some(CmvsPs2aVmAction::LoadCustomGlyphResource {
            resource: super::resolve_resource_name(state, *name)?,
            cell_size: *size,
        })),
        (961, [(_, index), (_, code)]) => {
            state.native_custom_glyphs.register(*index, *code as u16)?;
            Ok(None)
        }
        (962, [(_, code), (_, size), (_, descriptor)]) => {
            state
                .native_custom_glyphs
                .set_descriptor(*code as u16, *size, *descriptor as u16);
            Ok(None)
        }
        _ => Err(failure(
            "custom glyph command operands disagree with the native handler",
        )),
    }
}

#[cfg(test)]
mod tests;
