//! CMVS 461840/462040/4662D0 consumer: native glyph cells, paint variants and
//! their scene resources, on the portable glyph rasterizer in `outline`.
use super::*;
use crate::{CmvsNativeCustomGlyphBitmap, CmvsNativeFontRasterState, CmvsNativeTextGlyph};
use roukaku_media_core::{GlyphBitmap, GlyphBitmapFormat, GlyphInstance};
use std::{
    collections::{BTreeSet, btree_map::Entry},
    sync::Mutex,
};
mod cell;
mod outline;
pub(super) fn code(text: &str) -> Result<u16, CoreError> {
    let mut bytes = [0; 4];
    let (result, read, written) = encoding_rs::SHIFT_JIS
        .new_encoder()
        .encode_from_utf8_without_replacement(text, &mut bytes, true);
    if result != encoding_rs::EncoderResult::InputEmpty
        || read != text.len()
        || text.chars().count() != 1
    {
        return Err(error("SHINKI_EMU_CMVS_RASTER_CP932"));
    }
    match &bytes[..written] {
        [byte] => Ok(u16::from(*byte)),
        [high, low] => Ok(u16::from_be_bytes([*high, *low])),
        _ => Err(error("SHINKI_EMU_CMVS_RASTER_CP932")),
    }
}

pub(super) struct NativeRasterRun<'a> {
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
pub(super) struct NativeRasterSettings {
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
    fn settings(self) -> NativeRasterSettings {
        NativeRasterSettings {
            proportional: self.raster.proportional,
            alternate_coverage: self.raster.alternate_coverage,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PaintKey {
    cell: CellKey,
    grid: PresentationGrid,
    style: u32,
    colors: [u32; 2],
    color_mode: u32,
}

/// Actual texels per native cell pixel and snapped XY sample phase. The
/// native size in `CellKey` and this density together determine raster ppem.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PresentationGrid {
    density: u32,
    pub(super) phase: [u8; 2],
}

impl PresentationGrid {
    pub(super) const NATIVE: Self = Self {
        density: 1.0f32.to_bits(),
        phase: [0; 2],
    };

    pub(super) fn scale(self) -> f32 {
        f32::from_bits(self.density)
    }

    pub(super) fn is_native(self) -> bool {
        self == Self::NATIVE
    }

    fn at(
        run: &NativeRasterRun<'_>,
        camera: Transform2D,
        shake: [f32; 2],
        anchor: [i32; 2],
    ) -> Self {
        if run.style.raster.custom_bitmap_identity.is_some() {
            return Self::NATIVE;
        }
        // Bound the temporary bitmap by the native 4096px cell budget,
        // including the two MODE pixels on each side for the paint kernel.
        let padded = run.pixel_size as f32 + 4.0;
        let scale = (camera.m11 * run.logical_scale).min((4096.0f32 / padded).max(1.0));
        // Reveal scales the final quad, not its mask. Shake follows the
        // reveal transform in the native queue and contributes to phase.
        let phase = std::array::from_fn(|axis| {
            let logical = anchor[axis] as f32 * run.logical_scale + shake[axis];
            let physical = logical * camera.m11 + [camera.tx, camera.ty][axis];
            ((physical.rem_euclid(1.0) * 4.0).round() as u8) & 3
        });
        Self {
            density: scale.to_bits(),
            phase,
        }
    }
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
pub(super) struct NativeRasterRenderer {
    state: Mutex<State>,
    variants: BTreeMap<PaintKey, Variant>,
    pending: Vec<SceneCommand>,
    next_resource: u64,
}

#[derive(Default)]
struct State {
    fonts: BTreeMap<FontKey, outline::Scaler>,
    cells: BTreeMap<CellKey, CachedCell>,
    bindings: BTreeMap<String, [usize; 2]>,
    assets: Vec<outline::Face>,
    measurement_misses: u16,
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
            return Err(error("SHINKI_EMU_CMVS_RASTER_SIZE"));
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
                    .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_BITMAP"))?,
                    offset: [0, 0],
                    advance: 0,
                    empty: bitmap.coverage.iter().all(|value| *value == 0),
                    ink: None,
                }
            } else {
                if let Entry::Vacant(entry) = self.fonts.entry(key.font) {
                    entry.insert(outline::Scaler::new(&self.assets, key.font)?);
                }
                let scaler = self
                    .fonts
                    .get(&key.font)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_RASTER_FONT"))?;
                outline::raster(&self.assets, scaler, key)?
            };
            self.cells.insert(key, CachedCell { value, live });
        }
        let cached = self
            .cells
            .get_mut(&key)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_RASTER_BITMAP"))?;
        cached.live |= live;
        Ok(&cached.value)
    }

    fn prune_fonts(&mut self) {
        let active: BTreeSet<_> = self.cells.keys().map(|key| key.font).collect();
        self.fonts.retain(|key, _| active.contains(key));
    }
}

impl NativeRasterRenderer {
    /// Bind the regular and, when the family has one, the distinct bold face
    /// for each native family name. A weight without its own face is
    /// emboldened by the rasterizer, as GDI's font mapper does.
    pub(super) fn bind_fonts(
        &mut self,
        fonts: &BTreeMap<String, PackagedFont>,
        bold_fonts: &BTreeMap<String, PackagedFont>,
    ) -> Result<(), CoreError> {
        let current = self
            .state
            .get_mut()
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_LOCK"))?;
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
                    replacement.assets.push(outline::Face::new(font)?);
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
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_LOCK"))?;
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
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_LOCK"))?;
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
        runs: &[NativeRasterRun<'_>],
    ) -> Result<Vec<SceneCommand>, CoreError> {
        self.frame_with_owners(runs, Transform2D::IDENTITY, [0.0; 2], &mut Vec::new())
    }

    pub(super) fn frame_with_owners(
        &mut self,
        runs: &[NativeRasterRun<'_>],
        camera: Transform2D,
        shake: [f32; 2],
        owners: &mut Vec<usize>,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let state = self
            .state
            .get_mut()
            .map_err(|_| error("SHINKI_EMU_CMVS_RASTER_LOCK"))?;
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
                let anchor = [
                    run.origin[0].checked_add(cell.offset[0]),
                    run.origin[1].checked_add(cell.offset[1]),
                ];
                let [Some(x), Some(y)] = anchor else {
                    return Err(error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"));
                };
                let anchor = [x, y];
                let grid = PresentationGrid::at(run, camera, shake, anchor);
                let colors = run.style.colors.map(|color| color & 0x00ff_ffff);
                let color_mode = run.style.color_mode & 3;
                let mask_only = run.style.style == 0 && (color_mode == 0 || colors[0] == colors[1]);
                let paint_key = PaintKey {
                    cell: key,
                    grid,
                    style: run.style.style,
                    colors: if mask_only { [0; 2] } else { colors },
                    color_mode: if mask_only { 0 } else { color_mode },
                };
                let variant = match self.variants.entry(paint_key) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        let style = run.style.into();
                        let (bitmap, upload_offset) = if grid.is_native() {
                            paint::paint_upload(
                                &cell.bitmap,
                                style,
                                run.pixel_size,
                                grid,
                                [0; 2],
                                mask_only,
                            )?
                        } else {
                            let ink = cell
                                .ink
                                .expect("only a scalable cell has a presentation grid");
                            let (source, origin) = outline::presentation(
                                &state.assets,
                                &state.fonts[&key.font],
                                key,
                                &ink,
                                grid,
                            )?;
                            paint::paint_upload(
                                &source,
                                style,
                                run.pixel_size,
                                grid,
                                origin,
                                mask_only,
                            )?
                        };
                        let sequence = self.next_resource;
                        self.next_resource = sequence
                            .checked_add(1)
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_RASTER_RESOURCE"))?;
                        let resource_id = format!("cmvs.text.cell.{sequence}");
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
                let mut point = variant.upload_offset;
                if grid.is_native() {
                    for axis in 0..2 {
                        point[axis] = point[axis]
                            .checked_add(anchor[axis])
                            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_GEOMETRY"))?;
                    }
                } else {
                    let inverse = grid.scale().recip();
                    let translate = |axis: usize| {
                        anchor[axis] as f32 - f32::from(grid.phase[axis]) * 0.25 * inverse
                    };
                    commands.push(SceneCommand::PushTransform {
                        transform: Transform2D {
                            m11: inverse,
                            m22: inverse,
                            tx: translate(0),
                            ty: translate(1),
                            ..Transform2D::IDENTITY
                        },
                    });
                }
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
                commands.push(SceneCommand::LinearGlyphRun {
                    id: run.id.clone(),
                    glyphs: vec![GlyphInstance {
                        resource_id: variant.resource_id.clone(),
                        x: point[0],
                        y: point[1],
                        rotation_quadrants: 0,
                    }]
                    .into(),
                    rgba: if mask_only {
                        [
                            (colors[0] >> 16) as u8,
                            (colors[0] >> 8) as u8,
                            colors[0] as u8,
                            255,
                        ]
                    } else {
                        [255; 4]
                    },
                    opacity: run.visual[0],
                });
                if run.visual[1] != 1.0 {
                    commands.push(SceneCommand::PopTransform);
                }
                if run.logical_scale != 1.0 {
                    commands.push(SceneCommand::PopTransform);
                }
                if !grid.is_native() {
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
