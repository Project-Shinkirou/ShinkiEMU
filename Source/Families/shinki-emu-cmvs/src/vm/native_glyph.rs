use super::*;
pub(crate) mod layout;
pub(crate) mod motion;

/// Native 775 owner; 783 font managers remain separately owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphGraph {
    pub identity: u64,
    pub font: String,
    pub bold: bool,
    pub size: u32,
    pub capacity_hint: u32,
    pub virtual_size: [u32; 2],
    pub physical_size: [u32; 2],
    pub position: [u32; 2],
    pub rect: [u32; 4],
    pub viewport: [u32; 4],
    pub cursor: [u32; 2],
    pub gaps: [u32; 2],
    pub colors: [u32; 2],
    pub kinsoku: bool,
    pub style: u32,
    pub outline_color: u32,
    pub color_mode: u32,
    pub draw_order: i32,
    pub root: CmvsTextureParentState,
    pub atlas: CmvsNativeGlyphAtlas,
    pub records: Vec<CmvsNativeGlyphRecord>,
    pub provider_generation: u64,
    pub auxiliary: Option<CmvsNativeGlyphAuxiliary>,
    /// Non-owning O+1656 pointer to a481 image, resolved by allocation identity.
    pub secondary_reference: Option<u64>,
    pub bounds: [i32; 4],
    pub motion: Option<CmvsNativeGlyphMotion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphAuxiliary {
    pub resource: String,
    pub uri: Option<String>,
    pub image_info: [u32; 3],
    /// Current decoded provider pixels, including subsequent glyph uploads.
    /// An allocated provider whose lookup failed owns no surfaces or pixels.
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphAtlas {
    pub cell_size: u32,
    pub width: u32,
    pub height: u32,
    /// 1024 native bytes, including reserved slot zero. Clear does not reset it.
    pub occupancy: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphRecord {
    pub code: u16,
    pub position: [i32; 2],
    pub size: u32,
    pub colors: [u32; 2],
    pub style: u32,
    pub outline_color: u32,
    pub color_mode: u32,
    pub painted: Option<CmvsNativeGlyphPainted>,
    pub resident: Option<CmvsNativeGlyphResident>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphPainted {
    /// Native C by C BGRA cell, retained across viewport eviction.
    pub bgra: Vec<u8>,
    pub offset: [i32; 2],
    pub physical_size: u32,
    pub cell_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphResident {
    pub slot: u16,
    pub child: CmvsTextureChildState,
    pub cell_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeGlyphMotion {
    pub start: [i32; 2],
    pub end: [i32; 2],
    pub curve: u32,
    pub extra: u32,
    pub duration: u32,
    pub timestamp: u32,
}

impl CmvsNativeGlyphGraph {
    pub fn scale(&self, value: u32) -> Result<u32, CoreError> {
        let virtual_width = self.virtual_size[0];
        self.physical_size[0]
            .wrapping_mul(value)
            .checked_div(virtual_width)
            .ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_GLYPH_EXTENT",
                    "native glyph virtual width is zero",
                )
            })
    }
}

pub(crate) use motion::clock;

pub(super) fn is_operation(operation: crate::CmvsNativeTextOperation) -> bool {
    use crate::CmvsNativeTextOperation::*;
    matches!(
        operation,
        CreateGlyph
            | CreateGlyphVirtual
            | DestroyGlyph
            | GlyphBold
            | GlyphPosition
            | GlyphPositionFloat
            | GlyphRect
            | GlyphClear
            | GlyphMetrics
            | GlyphFont
            | GlyphColors
            | GlyphKinsoku
            | GlyphAppend
            | GlyphCursor
            | GlyphCursorFloat
            | GlyphColorMode
            | GlyphOrder
            | GlyphMeasure
            | GlyphUnits
            | GlyphStyle
            | GlyphOutline
            | GlyphViewport
            | GlyphBounds
            | GlyphMove
            | GlyphMoveCurve
            | GlyphLoadImage
            | GlyphGetPosition
            | GlyphMotionActive
            | GlyphReference
            | GlyphClamp
            | GlyphStop
    )
}

fn construct(
    state: &mut CmvsPs2aVmState,
    slot: u8,
    font: String,
    size: u32,
    capacity: u32,
    virtual_size: Option<[u32; 2]>,
) -> Result<(), CoreError> {
    use super::native_text::font_owner::{NATIVE_GLYPH_FONT_OWNER_BASE, ensure_owner};
    // 41CA20 sends BASE to429300 (renderer DWORD2/3), MODE to42E870
    // (DWORD6/7). 896 captures both pairs; 897 replaces only virtual extent.
    let physical_size = state.native_render_size;
    let virtual_extent = virtual_size.unwrap_or(state.native_stage_size);
    if physical_size.contains(&0)
        || virtual_extent.contains(&0)
        || state.native_stage_size.contains(&0)
    {
        return Err(invalid(
            "SHINKI_EMU_CMVS_DISPLAY_UNBOUND",
            "native glyph constructor has no renderer extent",
        ));
    }
    let capacity_hint = if capacity == 0 { 512 } else { capacity };
    let cell_size = physical_size[0].wrapping_mul(size) / virtual_extent[0];
    let cell_size = cell_size.wrapping_add(6);
    let width: u32 = if physical_size[0] <= 2048 {
        if capacity_hint <= 64 { 512 } else { 1024 }
    } else if capacity_hint <= 64 {
        1024
    } else {
        2048
    };
    let columns = width
        .checked_div(cell_size)
        .filter(|n| *n != 0)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_GLYPH_ATLAS",
                "native glyph cell has no atlas columns",
            )
        })?;
    let slots = columns.wrapping_mul(columns);
    // Capacity sizes the provider. Residency still searches all slots1..1023.
    let pages = capacity_hint / slots + u32::from(capacity_hint % slots != 0);
    let height = pages.checked_mul(width).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_GLYPH_ATLAS",
            "native glyph atlas allocation overflows",
        )
    })?;
    ensure_owner(state, NATIVE_GLYPH_FONT_OWNER_BASE, slot)?;
    let owner = state
        .native_font_owners
        .get_mut(&(NATIVE_GLYPH_FONT_OWNER_BASE + u16::from(slot)))
        .unwrap();
    owner.set_family(if font.is_empty() {
        &state.native_default_font
    } else {
        &font
    })?;
    let bold = owner.raster.bold;
    super::native_text::allocate(state, 775, slot)?;
    let occupancy = vec![0; 1024];
    let root = CmvsTextureParentState {
        surface_initialized: true,
        basic_kind: 5,
        alpha: Some(255),
        reference_size: Some(physical_size),
        rect_words: Some([0, 0, 4, 4]),
        auxiliary_word: Some(1),
        image_info: [width, height, 1],
        ..Default::default()
    };
    state.native_glyphs.insert(
        slot,
        CmvsNativeGlyphGraph {
            identity: state.slot_objects[&(775 << 8 | u32::from(slot))],
            font,
            bold,
            size,
            capacity_hint,
            virtual_size: virtual_extent,
            physical_size,
            position: [0; 2],
            rect: [0; 4],
            cursor: [0; 2],
            gaps: [0; 2],
            viewport: [
                0,
                0,
                (state.native_stage_size[0] as f32).to_bits(),
                (state.native_stage_size[1] as f32).to_bits(),
            ],
            colors: [0x00ff_ffff; 2],
            kinsoku: true,
            style: 2,
            outline_color: 0,
            color_mode: 0,
            draw_order: 1,
            root,
            atlas: CmvsNativeGlyphAtlas {
                cell_size,
                width,
                height,
                occupancy,
            },
            records: Vec::new(),
            provider_generation: 0,
            auxiliary: None,
            secondary_reference: None,
            bounds: [0; 4],
            motion: None,
        },
    );
    state.write_interpreter_word(81220, 1);
    Ok(())
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    operation: crate::CmvsNativeTextOperation,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    use crate::CmvsNativeTextOperation::*;
    let count = match operation {
        CreateGlyph | GlyphMetrics | GlyphMove => 4,
        CreateGlyphVirtual | GlyphMoveCurve => 6,
        GlyphRect | GlyphViewport | GlyphBounds => 5,
        GlyphPosition | GlyphPositionFloat | GlyphColors | GlyphCursor | GlyphCursorFloat
        | GlyphClamp => 3,
        DestroyGlyph | GlyphClear | GlyphGetPosition | GlyphMotionActive | GlyphStop => 1,
        _ => 2,
    };
    if values.len() != count {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "native glyph argument count differs",
        ));
    }
    // 470D50 runs before the slot guard, including invalid-slot invocations.
    let font = matches!(operation, CreateGlyph | CreateGlyphVirtual | GlyphFont)
        .then(|| super::native_text::font_name(state, values[1].1))
        .transpose()?;
    let source = matches!(operation, GlyphAppend | GlyphMeasure | GlyphUnits)
        .then(|| {
            text::resolve_tagged_string_bytes(state, values[1].1).map(|bytes| bytes.into_owned())
        })
        .transpose()?;
    let resource = (operation == GlyphLoadImage)
        .then(|| text::resolve_string_value(state, values[1].1))
        .transpose()?;
    let index = values[0].1;
    if index >= 8 {
        raise_error_flag(state, 10532, 0x1000)?;
        return Ok(None);
    }
    let slot = index as u8;
    if matches!(operation, CreateGlyph | CreateGlyphVirtual) {
        construct(
            state,
            slot,
            font.unwrap(),
            values[2].1,
            values[3].1,
            (operation == CreateGlyphVirtual).then(|| [values[4].1, values[5].1]),
        )?;
        return Ok(Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::GlyphCreate { slot },
        )));
    }
    if operation == DestroyGlyph {
        state.slot_objects.remove(&(775 << 8 | index));
        state.native_glyphs.remove(&slot);
        return Ok(None);
    }
    if !state.slot_objects.contains_key(&(775 << 8 | index))
        || !state.native_glyphs.contains_key(&slot)
    {
        raise_error_flag(state, 10532, 0x1000)?;
        return Ok(None);
    }
    let now = clock(state);
    let graph = state.native_glyphs.get_mut(&slot).unwrap();
    let integer_pair = || {
        [
            (values[1].1 as i32 as f32).to_bits(),
            (values[2].1 as i32 as f32).to_bits(),
        ]
    };
    let action = match operation {
        GlyphPosition | GlyphPositionFloat => {
            graph.position = if operation == GlyphPosition {
                integer_pair()
            } else {
                [values[1].1, values[2].1]
            };
            let denominator = graph.virtual_size[0] as f32;
            for axis in 0..2 {
                graph.root.float_position_words[axis] = ((graph.physical_size[0] as f32
                    * f32::from_bits(graph.position[axis]))
                    / denominator)
                    .to_bits();
            }
            let reference = graph.secondary_reference;
            let position = graph.position;
            update_secondary(state, reference, position, operation == GlyphPosition)?;
            None
        }
        GlyphRect | GlyphViewport => {
            let rect = std::array::from_fn(|i| (values[i + 1].1 as i32 as f32).to_bits());
            if operation == GlyphRect {
                graph.rect = rect;
                graph.cursor = [rect[0], rect[1]];
            } else {
                graph.viewport = rect;
            }
            None
        }
        GlyphClear => {
            graph.records.clear();
            graph.cursor = [graph.rect[0], graph.rect[1]];
            None
        }
        GlyphMetrics => {
            if values[1].1 as i32 > 0 {
                graph.size = values[1].1;
            }
            for i in 0..2 {
                if values[i + 2].1 as i32 >= 0 {
                    graph.gaps[i] = (values[i + 2].1 as i32 as f32).to_bits();
                }
            }
            None
        }
        GlyphFont => {
            let font = font.unwrap();
            let owner = state
                .native_font_owners
                .get_mut(&(783 + u16::from(slot)))
                .unwrap();
            owner.set_family(if font.is_empty() {
                &state.native_default_font
            } else {
                &font
            })?;
            owner.invalidate()?;
            graph.font = font;
            Some(CmvsNativeTextAction::GlyphFont { slot })
        }
        GlyphBold => {
            let bold = values[1].1 != 0;
            state
                .native_font_owners
                .get_mut(&(783 + u16::from(slot)))
                .unwrap()
                .set_bold(bold)?;
            graph.bold = bold;
            None
        }
        GlyphColors => {
            for i in 0..2 {
                if values[i + 1].1 & 0xff00_0000 == 0 {
                    graph.colors[i] = values[i + 1].1;
                }
            }
            None
        }
        GlyphKinsoku => {
            graph.kinsoku = values[1].1 != 0;
            None
        }
        GlyphAppend => Some(CmvsNativeTextAction::GlyphAppend {
            slot,
            source: source.unwrap(),
        }),
        GlyphCursor | GlyphCursorFloat => {
            graph.cursor = if operation == GlyphCursor {
                integer_pair()
            } else {
                [values[1].1, values[2].1]
            };
            None
        }
        GlyphColorMode => {
            graph.color_mode = values[1].1;
            None
        }
        GlyphOrder => {
            graph.draw_order = values[1].1 as i32;
            graph.root.auxiliary_word = Some(values[1].1);
            None
        }
        GlyphLoadImage => {
            graph.provider_generation =
                graph.provider_generation.checked_add(1).ok_or_else(|| {
                    invalid(
                        "SHINKI_EMU_CMVS_GLYPH_PROVIDER",
                        "glyph provider generation overflow",
                    )
                })?;
            graph.root.blend_mode = Some(4);
            graph.root.image_info = [0; 3];
            let resource = resource.unwrap();
            graph.auxiliary = Some(CmvsNativeGlyphAuxiliary {
                resource: resource.clone(),
                uri: None,
                image_info: [0; 3],
                rgba: Vec::new(),
            });
            Some(CmvsNativeTextAction::GlyphLoadImage { slot, resource })
        }
        GlyphGetPosition => {
            let [x, y] = graph
                .position
                .map(|word| motion::cvtt(f64::from(f32::from_bits(word))) as u32);
            state.write_interpreter_word(81224, x);
            state.write_interpreter_word(81228, y);
            None
        }
        GlyphMotionActive => {
            let active = u32::from(graph.motion.is_some());
            state.write_interpreter_word(81220, active);
            None
        }
        GlyphReference => {
            let reference = values[1].1 as i32;
            if reference < 0 {
                graph.secondary_reference = None;
            } else if reference < 256 {
                if let Some(identity) = state.slot_objects.get(&(1924 << 8 | reference as u32)) {
                    graph.secondary_reference = Some(*identity);
                } else {
                    raise_error_flag(state, 10532, 0x1000)?;
                }
            } else {
                raise_error_flag(state, 10532, 0x1000)?;
            }
            None
        }
        GlyphClamp => {
            let mut position = [values[1].1 as i32, values[2].1 as i32];
            for (axis, value) in position.iter_mut().enumerate() {
                if *value < graph.bounds[axis * 2] {
                    *value = graph.bounds[axis * 2];
                }
                if *value > graph.bounds[axis * 2 + 1] {
                    *value = graph.bounds[axis * 2 + 1];
                }
            }
            state.write_interpreter_word(81224, position[0] as u32);
            state.write_interpreter_word(81228, position[1] as u32);
            None
        }
        GlyphStop => {
            graph.motion = None;
            None
        }
        GlyphMeasure => {
            let [lines, maximum, last] = layout::measure(graph, &source.unwrap())?;
            state.write_interpreter_word(81220, lines);
            state.write_interpreter_word(81224, maximum);
            state.write_interpreter_word(81236, last);
            None
        }
        GlyphUnits => {
            let units = layout::units(&source.unwrap())?;
            state.write_interpreter_word(81220, units);
            None
        }
        GlyphStyle => {
            graph.style = values[1].1;
            None
        }
        GlyphOutline => {
            graph.outline_color = values[1].1;
            None
        }
        GlyphBounds => {
            graph.bounds = std::array::from_fn(|i| values[i + 1].1 as i32);
            None
        }
        GlyphMove | GlyphMoveCurve => {
            let (curve, delta, extra, duration) = if operation == GlyphMove {
                (0, [values[1].1 as i32, values[2].1 as i32], 0, values[3].1)
            } else {
                (
                    values[1].1,
                    [values[2].1 as i32, values[3].1 as i32],
                    values[4].1,
                    values[5].1,
                )
            };
            motion::start(graph, curve, delta, extra, duration, now);
            None
        }
        _ => unreachable!(),
    };
    Ok(action.map(CmvsPs2aVmAction::NativeText))
}

/// 899 and motion forward integer virtual coordinates;900 forwards raw floats.
/// A destroyed/replaced481 owner leaves a dangling native pointer, not a new bind.
pub(crate) fn update_secondary(
    state: &mut CmvsPs2aVmState,
    reference: Option<u64>,
    position: [u32; 2],
    integer: bool,
) -> Result<(), CoreError> {
    let Some(reference) = reference else {
        return Ok(());
    };
    let slot = state
        .slot_objects
        .iter()
        .find_map(|(&key, &identity)| {
            (key >> 8 == 1924 && identity == reference).then_some(key as u8)
        })
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_GLYPH_REFERENCE_DANGLING",
                "native glyph secondary image pointer refers to a destroyed owner",
            )
        })?;
    let parent = state.texture_parents.get_mut(&slot).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_GLYPH_REFERENCE_UNBOUND",
            "native glyph secondary image has no basic state",
        )
    })?;
    if integer {
        parent.auxiliary_pair_words =
            Some(position.map(|word| motion::cvtt(f64::from(f32::from_bits(word))) as u32));
    } else {
        parent.float_position_words = [position[0], position[1], 0];
    }
    Ok(())
}
pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    let fail = || {
        invalid(
            "SHINKI_EMU_CMVS_GLYPH_SNAPSHOT",
            "standalone glyph graph snapshot is inconsistent",
        )
    };
    for (&slot, graph) in &state.native_glyphs {
        if state.slot_objects.get(&(775 << 8 | u32::from(slot))) != Some(&graph.identity)
            || graph.physical_size.contains(&0)
            || graph.virtual_size.contains(&0)
            || graph.atlas.cell_size == 0
            || graph.atlas.width == 0
            || graph.atlas.height == 0
            || graph.atlas.occupancy.len() != 1024
            || graph.atlas.occupancy.iter().any(|v| *v > 1)
            || graph.root.basic_kind != 5
        {
            return Err(fail());
        }
        if let Some(provider) = &graph.auxiliary {
            let [width, height, _] = provider.image_info;
            let expected = u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|bytes| bytes.checked_mul(4));
            if (!provider.rgba.is_empty()
                && (width == 0
                    || height == 0
                    || expected != Some(provider.rgba.len() as u64)
                    || provider.uri.is_none()))
                || graph.provider_generation == 0
            {
                return Err(fail());
            }
        } else if graph.provider_generation != 0 {
            return Err(fail());
        }
        let mut residents = BTreeSet::new();
        for record in &graph.records {
            if let Some(painted) = &record.painted {
                let bytes = u64::from(painted.cell_size)
                    .checked_mul(u64::from(painted.cell_size))
                    .and_then(|bytes| bytes.checked_mul(4));
                if painted.cell_size == 0 || bytes != Some(painted.bgra.len() as u64) {
                    return Err(fail());
                }
            }
            if let Some(resident) = &record.resident {
                let index = usize::from(resident.slot);
                if record.painted.is_none()
                    || !(1..1024).contains(&index)
                    || graph.atlas.occupancy[index] != 1
                    || !residents.insert(resident.slot)
                    || resident.child.basic_kind != 0
                    || resident.cell_size == 0
                {
                    return Err(fail());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
