//! Windows CMVS 461840/462040/4662D0 consumer. The non-Windows renderer
//! deliberately remains separate: Swash does not implement the GDI oracle.
use super::*;
use crate::{CmvsNativeCustomGlyphBitmap, CmvsNativeFontRasterState, CmvsNativeTextGlyph};
use roukaku_media_core::{GlyphBitmap, GlyphBitmapFormat, GlyphInstance, Transform2D};
use std::{
    collections::{BTreeSet, btree_map::Entry},
    sync::Mutex,
};
use windows::Win32::Graphics::Gdi::*;
mod cell;
mod handles;
pub(super) fn code(text: &str) -> Result<u16, CoreError> {
    let mut bytes = [0; 4];
    let (result, read, written) = encoding_rs::SHIFT_JIS
        .new_encoder()
        .encode_from_utf8_without_replacement(text, &mut bytes, true);
    if result != encoding_rs::EncoderResult::InputEmpty
        || read != text.len()
        || text.chars().count() != 1
    {
        return Err(error("SHINKI_EMU_CMVS_GDI_CP932"));
    }
    match &bytes[..written] {
        [byte] => Ok(u16::from(*byte)),
        [high, low] => Ok(u16::from_be_bytes([*high, *low])),
        _ => Err(error("SHINKI_EMU_CMVS_GDI_CP932")),
    }
}

pub(super) struct NativeGdiRun<'a> {
    pub id: String,
    pub font_name: &'a str,
    pub style: &'a CmvsNativeTextGlyph,
    pub origin: [i32; 2],
    /// [opacity, horizontal reveal scale], with the native origin-2 pivot.
    pub visual: [f32; 2],
    pub pixel_size: u32,
    pub logical_scale: f32,
    pub custom: Option<&'a CmvsNativeCustomGlyphBitmap>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NativeGdiSettings {
    pub proportional: bool,
    pub alternate_coverage: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FontKey {
    assets: [usize; 2],
    size: u32,
    weight: u32,
    shift_jis: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CellKey {
    font: FontKey,
    code: u16,
    raster: CmvsNativeFontRasterState,
}
impl CellKey {
    fn settings(self) -> NativeGdiSettings {
        NativeGdiSettings {
            proportional: self.raster.proportional,
            alternate_coverage: self.raster.alternate_coverage,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PaintKey {
    cell: CellKey,
    style: u32,
    colors: [u32; 2],
    color_mode: u32,
}
struct CachedCell {
    value: cell::Cell,
    live: bool,
}
struct Variant {
    resource_id: String,
    upload_offset: [i32; 2],
    live: bool,
}

#[derive(Default)]
pub(super) struct NativeGdiRenderer {
    state: Mutex<State>,
    variants: BTreeMap<PaintKey, Variant>,
    pending: Vec<SceneCommand>,
    next_resource: u64,
}

// GDI handles never escape State or the renderer mutex. Selection guards
// restore the stock font before every return, including errors. A non-NULL
// compatible DC remains valid when a worker thread exits.
#[derive(Default)]
struct State {
    fonts: BTreeMap<FontKey, handles::Font>,
    cells: BTreeMap<CellKey, CachedCell>,
    dc: Option<handles::Dc>,
    bindings: BTreeMap<String, [usize; 2]>,
    // Declared after fonts/DC so HFONTs are destroyed before registrations.
    assets: Vec<handles::MemoryFont>,
    measurement_misses: u16,
}
unsafe impl Send for State {}

fn win_error(operation: &'static str) -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_CMVS_GDI",
        format!("{operation}: {}", windows::core::Error::from_thread()),
    )
}

impl State {
    fn key(
        &self,
        name: &str,
        code: u16,
        size: u32,
        mut raster: CmvsNativeFontRasterState,
    ) -> Result<CellKey, CoreError> {
        if size == 0 || size > i32::MAX as u32 {
            return Err(error("SHINKI_EMU_CMVS_GDI_SIZE"));
        }
        let assets = self
            .bindings
            .get(name)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?;
        // Descriptor-only custom changes affect advance, not bitmap identity.
        raster.custom_advance = None;
        Ok(CellKey {
            font: FontKey {
                assets: *assets,
                size,
                weight: if raster.bold { 600 } else { 400 },
                shift_jis: raster.shift_jis,
            },
            code,
            raster,
        })
    }

    fn ensure_cell(
        &mut self,
        key: CellKey,
        custom: Option<&CmvsNativeCustomGlyphBitmap>,
        live: bool,
    ) -> Result<&cell::Cell, CoreError> {
        if !self.cells.contains_key(&key) {
            // Measurement-only cells have a small bounded working set; current
            // frame cells are retained regardless of how many glyphs are live.
            if !live && self.measurement_misses == 256 {
                self.cells.retain(|_, cell| cell.live);
                self.prune_fonts();
                self.measurement_misses = 0;
            }
            if !live {
                self.measurement_misses += 1;
            }
            let value = if let Some(identity) = key.raster.custom_bitmap_identity {
                let bitmap = custom
                    .filter(|bitmap| bitmap.identity == identity && bitmap.size == key.font.size)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_CUSTOM_GLYPH_IDENTITY"))?;
                cell::Cell {
                    bitmap: GlyphBitmap::from_vec(
                        bitmap.size,
                        bitmap.size,
                        GlyphBitmapFormat::Alpha8,
                        bitmap.coverage.clone(),
                    )
                    .map_err(|_| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?,
                    offset: [0, 0],
                    advance: 0,
                    empty: bitmap.coverage.iter().all(|value| *value == 0),
                }
            } else {
                if self.dc.is_none() {
                    self.dc = Some(handles::Dc::new()?);
                }
                let dc = self
                    .dc
                    .as_ref()
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_DC"))?;
                if let Entry::Vacant(entry) = self.fonts.entry(key.font) {
                    entry.insert(handles::Font::new(&self.assets, key.font, dc)?);
                }
                let font = self
                    .fonts
                    .get(&key.font)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_FONT"))?;
                handles::raster(dc, font, key)?
            };
            self.cells.insert(key, CachedCell { value, live });
        }
        let cached = self
            .cells
            .get_mut(&key)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_BITMAP"))?;
        cached.live |= live;
        Ok(&cached.value)
    }

    fn prune_fonts(&mut self) {
        let active: BTreeSet<_> = self.cells.keys().map(|key| key.font).collect();
        self.fonts.retain(|key, _| active.contains(key));
    }
}

impl NativeGdiRenderer {
    /// Bind the asset/face expected for the current native family and weight.
    /// If a weight change selects a distinct real face, the caller must resolve
    /// and rebind that face; a synthesized weight retains the original tables.
    /// GetFontData rejects a different selection rather than weakening identity
    /// checks or silently replacing the requested CreateFontA weight.
    pub(super) fn bind_fonts(
        &mut self,
        fonts: &BTreeMap<String, PackagedFont>,
        bold_fonts: &BTreeMap<String, PackagedFont>,
    ) -> Result<(), CoreError> {
        let current = self
            .state
            .get_mut()
            .map_err(|_| error("SHINKI_EMU_CMVS_GDI_LOCK"))?;
        if current.bindings.len() == fonts.len()
            && fonts.iter().all(|(name, font)| {
                current.bindings.get(name).is_some_and(|indices| {
                    current.assets[indices[0]].matches(font)
                        && current.assets[indices[1]].matches(bold_fonts.get(name).unwrap_or(font))
                })
            })
        {
            return Ok(());
        }
        let mut replacement = State::default();
        for (name, font) in fonts {
            let mut indices = [0; 2];
            for (index, font) in [font, bold_fonts.get(name).unwrap_or(font)]
                .into_iter()
                .enumerate()
            {
                indices[index] = if let Some(index) = replacement
                    .assets
                    .iter()
                    .position(|asset| asset.matches(font))
                {
                    index
                } else {
                    let index = replacement.assets.len();
                    replacement.assets.push(handles::MemoryFont::new(font)?);
                    index
                };
            }
            replacement.bindings.insert(name.clone(), indices);
        }
        // Build transactionally: failed registration leaves the old bindings
        // and every uploaded resource intact.
        *current = replacement;
        self.pending.extend(
            std::mem::take(&mut self.variants)
                .into_values()
                .map(|variant| SceneCommand::ReleaseResource {
                    resource_id: variant.resource_id,
                }),
        );
        Ok(())
    }

    pub(super) fn advance(
        &self,
        font_name: &str,
        code: u16,
        pixel_size: u32,
        raster: CmvsNativeFontRasterState,
    ) -> Result<u32, CoreError> {
        if raster.custom_bitmap_identity.is_some() {
            return raster
                .custom_advance
                .map(u32::from)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_CUSTOM_GLYPH_IDENTITY"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| error("SHINKI_EMU_CMVS_GDI_LOCK"))?;
        let key = state.key(font_name, code, pixel_size, raster)?;
        Ok(state.ensure_cell(key, None, false)?.advance)
    }

    pub(super) fn paint_glyph(
        &mut self,
        font_name: &str,
        record: &crate::CmvsNativeGlyphRecord,
        pixel_size: u32,
        cell_size: u32,
        raster: CmvsNativeFontRasterState,
        custom: Option<&CmvsNativeCustomGlyphBitmap>,
    ) -> Result<crate::CmvsNativeGlyphPainted, CoreError> {
        let state = self
            .state
            .get_mut()
            .map_err(|_| error("SHINKI_EMU_CMVS_GDI_LOCK"))?;
        let key = state.key(font_name, record.code, pixel_size, raster)?;
        let cell = state.ensure_cell(key, custom, true)?;
        let bgra = paint::paint_cell(
            &cell.bitmap,
            paint::PaintStyle {
                style: record.style,
                colors: record.colors,
                color_mode: record.color_mode,
                outline_color: record.outline_color,
            },
            pixel_size,
            cell_size,
        )?;
        Ok(crate::CmvsNativeGlyphPainted {
            bgra,
            cell_size,
            offset: cell.offset,
            physical_size: pixel_size,
        })
    }

    #[cfg(test)]
    pub(super) fn frame(
        &mut self,
        runs: &[NativeGdiRun<'_>],
    ) -> Result<Vec<SceneCommand>, CoreError> {
        self.frame_with_owners(runs, &mut Vec::new())
    }

    pub(super) fn frame_with_owners(
        &mut self,
        runs: &[NativeGdiRun<'_>],
        owners: &mut Vec<usize>,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let state = self
            .state
            .get_mut()
            .map_err(|_| error("SHINKI_EMU_CMVS_GDI_LOCK"))?;
        for cell in state.cells.values_mut() {
            cell.live = false;
        }
        for variant in self.variants.values_mut() {
            variant.live = false;
        }
        let mut commands = Vec::new();
        // New uploads are committed only if every run succeeds. Otherwise
        // pending releases survive for the next successfully returned frame.
        let mut uploads = Vec::new();
        let mut new_keys = Vec::new();
        let result = (|| {
            for (index, run) in runs.iter().enumerate() {
                // Raw nonzero styles above two use opaque fill without a halo.
                let key = state.key(
                    run.font_name,
                    run.style.code,
                    run.pixel_size,
                    run.style.raster,
                )?;
                let cell = state.ensure_cell(key, run.custom, true)?;
                if cell.empty {
                    continue;
                }
                let paint_key = PaintKey {
                    cell: key,
                    style: run.style.style,
                    colors: run.style.colors,
                    color_mode: run.style.color_mode & 3,
                };
                let variant = match self.variants.entry(paint_key) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let (bitmap, upload_offset) =
                            paint::paint_upload(&cell.bitmap, run.style, run.pixel_size)?;
                        let sequence = self.next_resource;
                        self.next_resource = sequence
                            .checked_add(1)
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_GDI_RESOURCE"))?;
                        let resource_id = format!("cmvs.gdi.{sequence}");
                        uploads.push(SceneCommand::UploadGlyph {
                            resource_id: resource_id.clone(),
                            glyph: bitmap,
                        });
                        new_keys.push(paint_key);
                        entry.insert(Variant {
                            resource_id,
                            upload_offset,
                            live: false,
                        })
                    }
                };
                variant.live = true;
                let x = run.origin[0]
                    .checked_add(cell.offset[0])
                    .and_then(|n| n.checked_sub(2))
                    .and_then(|n| n.checked_add(variant.upload_offset[0]))
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"))?;
                let y = run.origin[1]
                    .checked_add(cell.offset[1])
                    .and_then(|n| n.checked_sub(2))
                    .and_then(|n| n.checked_add(variant.upload_offset[1]))
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"))?;
                if run.logical_scale != 1.0 {
                    commands.push(SceneCommand::PushTransform {
                        transform: Transform2D {
                            m11: run.logical_scale,
                            m22: run.logical_scale,
                            ..Transform2D::IDENTITY
                        },
                    });
                }
                if run.visual[1] != 1.0 {
                    let pivot = run.origin[0] as f32 - 2.0;
                    commands.push(SceneCommand::PushTransform {
                        transform: Transform2D {
                            m11: run.visual[1],
                            tx: pivot * (1.0 - run.visual[1]),
                            ..Transform2D::IDENTITY
                        },
                    });
                }
                owners.push(index);
                commands.push(SceneCommand::GlyphRun {
                    id: run.id.clone(),
                    glyphs: vec![GlyphInstance {
                        resource_id: variant.resource_id.clone(),
                        x,
                        y,
                        rotation_quadrants: 0,
                    }]
                    .into(),
                    rgba: [255; 4],
                    opacity: run.visual[0],
                    blend: BlendMode::Alpha,
                });
                if run.visual[1] != 1.0 {
                    commands.push(SceneCommand::PopTransform);
                }
                if run.logical_scale != 1.0 {
                    commands.push(SceneCommand::PopTransform);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            for key in new_keys {
                self.variants.remove(&key);
            }
            return Err(error);
        }
        let mut output = std::mem::take(&mut self.pending);
        self.variants.retain(|_, variant| {
            if !variant.live {
                output.push(SceneCommand::ReleaseResource {
                    resource_id: variant.resource_id.clone(),
                });
            }
            variant.live
        });
        let old_count = state.cells.len();
        state.cells.retain(|_, cell| cell.live);
        state.measurement_misses = 0;
        if state.cells.len() != old_count {
            state.prune_fonts();
        }
        output.extend(uploads);
        output.extend(commands);
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
