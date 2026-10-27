use super::*;
use crate::CmvsNativeTextOperation;
mod advance_timing;
pub(crate) mod advance_wait;
pub(crate) mod dialogue;
pub(crate) mod dialogue_layout;
pub(crate) mod font_owner;
pub(crate) mod layout;
pub(crate) mod measurement;
pub use font_owner::{CmvsNativeFontOwner, CmvsNativeFontRasterState};

/// Optional proportional-advance measurer shared by layout and metrics.
pub(crate) type Measure<'a> = Option<&'a mut dyn FnMut(&str) -> Result<u32, CoreError>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeTextGlyph {
    pub text: String,
    pub code: u16,
    pub raster: CmvsNativeFontRasterState,
    pub position: [i32; 2],
    pub font: String,
    pub size: u32,
    pub colors: [u32; 2],
    pub style: u32,
    pub color_mode: u32,
}

/// Native text-object parameters. Font names and source text stay inside the
/// Family. CPU/GPU font handles never enter a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeTextSurface {
    pub font: String,
    pub capacity_hint: u32,
    /// Native text-object DWORD 36, propagated by `sub_464CA0`.
    pub visible: bool,
    pub rect: [i32; 4],
    pub cursor: [i32; 2],
    pub size: u32,
    pub character_spacing: u32,
    pub line_spacing: u32,
    pub colors: [u32; 2],
    pub kinsoku: bool,
    /// CText DWORD26, separate from its outline/paint style.
    pub bold: bool,
    pub style: u32,
    pub color_mode: u32,
    pub position: [i32; 2],
    /// Captured MODE dimensions used by 4662D0 before rasterization.
    pub physical_size: [u32; 2],
    /// Captured BASE dimensions, or the explicit constructor349 extent.
    pub virtual_size: [u32; 2],
    /// CText DWORD37: signed basic draw order.
    pub draw_order: i32,
    /// CText DWORD35, multiplied by the live dialogue speed factor.
    pub glyph_interval_ms: u32,
    /// CText DWORD43 reserves a ruby row even without ruby on that line.
    pub reserve_ruby: bool,
    /// CText DWORD33/34 defaults; channel dialogue supplies its own overrides.
    pub reveal_defaults: [u32; 2],
    pub glyphs: Vec<CmvsNativeTextGlyph>,
}

impl CmvsNativeTextSurface {
    fn new(
        font: String,
        capacity_hint: u32,
        physical_size: [u32; 2],
        virtual_size: [u32; 2],
    ) -> Self {
        Self {
            font,
            capacity_hint,
            visible: true,
            rect: [0; 4],
            cursor: [0; 2],
            size: 0,
            character_spacing: 0,
            line_spacing: 0,
            colors: [0x00ff_ffff; 2],
            kinsoku: true,
            bold: false,
            style: 2,
            color_mode: 0,
            position: [0; 2],
            physical_size,
            virtual_size,
            draw_order: 1,
            glyph_interval_ms: 30,
            reserve_ruby: false,
            reveal_defaults: [0; 2],
            glyphs: Vec::new(),
        }
    }
}

/// `47EA90`/`47ED00` rebuild table 750, retaining the separately owned font.
pub(super) fn rebuild_channel(
    state: &mut CmvsPs2aVmState,
    slot: u8,
    position: [i32; 2],
    size: [i32; 2],
    virtual_size: Option<[u32; 2]>,
) -> Result<(), CoreError> {
    font_owner::ensure_owner(state, font_owner::NATIVE_TEXT_FONT_OWNER_BASE, slot)?;
    allocate(state, 750, slot)?;
    let mut surface = CmvsNativeTextSurface::new(
        String::new(),
        0xC4,
        state.native_render_size,
        virtual_size.unwrap_or(state.native_stage_size),
    );
    surface.position = position;
    surface.rect = [0, 0, size[0], size[1]];
    surface.size = 27;
    surface.style = 0;
    let owner = state
        .native_font_owners
        .get_mut(&(font_owner::NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot)))
        .unwrap();
    owner.set_family(&state.native_default_font)?;
    owner.set_bold(state.interpreter_words.get(&1476).copied().unwrap_or(0) != 0)?;
    surface.bold = owner.raster.bold;
    state.native_text_fonts.insert(slot, String::new());
    state.native_text.insert(slot, surface);
    state.native_text_reveals.remove(&slot);
    state.effect_playback.insert(
        slot,
        CmvsEffectPlaybackRecord {
            value: 12,
            ..Default::default()
        },
    );
    Ok(())
}

/// Product projection must handle these operations synchronously. A Family
/// without its font/renderer binding rejects the action and closes the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CmvsNativeTextAction {
    GlyphCreate {
        slot: u8,
    },
    GlyphFont {
        slot: u8,
    },
    GlyphAppend {
        slot: u8,
        source: Vec<u8>,
    },
    GlyphLoadImage {
        slot: u8,
        resource: String,
    },
    Create {
        slot: u8,
    },
    Destroy {
        slot: u8,
    },
    Clear {
        slot: u8,
    },
    SetFont {
        slot: u8,
    },
    SetDefaultFont {
        font: String,
    },
    DrawStatic {
        slot: u8,
        source: String,
    },
    DrawDialogue {
        channel: u8,
        slot: u8,
        source: String,
    },
    Measure {
        slot: u8,
        source: String,
        proportional: bool,
    },
}

pub(super) fn font_name(state: &CmvsPs2aVmState, value: u32) -> Result<String, CoreError> {
    let bytes = text::resolve_tagged_string_bytes(state, value)?;
    if bytes.len() >= 64 || bytes.contains(&0) {
        return Err(invalid(
            "SHINKI_EMU_CMVS_TEXT_FONT",
            "native font name exceeds its buffer",
        ));
    }
    text::resolve_string_value(state, value)
}

pub(super) fn allocate(state: &mut CmvsPs2aVmState, table: u32, slot: u8) -> Result<(), CoreError> {
    let id = state.next_slot_object_id;
    state.next_slot_object_id = id.checked_add(1).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_TEXT_OBJECT",
            "native text object identity exhausted",
        )
    })?;
    state.slot_objects.insert(table << 8 | u32::from(slot), id);
    Ok(())
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    dialogue::validate(state)?;
    font_owner::validate(state)?;
    let fail = || {
        invalid(
            "SHINKI_EMU_CMVS_TEXT_OBJECT",
            "native text snapshot has inconsistent owners or fields",
        )
    };
    for record in state.effect_playback.values() {
        if let Some(lists) = &record.text_surface.boundaries {
            for bytes in lists {
                if bytes.len() >= 64 {
                    return Err(fail());
                }
                text::validate_native_string_slot(bytes)?;
            }
        }
    }
    for (slot, font) in &state.native_text_fonts {
        let (bytes, _, invalid_encoding) = encoding_rs::SHIFT_JIS.encode(font);
        if *slot >= 12
            || invalid_encoding
            || bytes.len() >= 64
            || bytes.contains(&0)
            || !state
                .slot_objects
                .contains_key(&(762 << 8 | u32::from(*slot)))
            || !state
                .native_font_owners
                .contains_key(&(762 + u16::from(*slot)))
        {
            return Err(fail());
        }
    }
    for (slot, surface) in &state.native_text {
        if *slot >= 12
            || state.native_text_fonts.get(slot) != Some(&surface.font)
            || !state
                .slot_objects
                .contains_key(&(750 << 8 | u32::from(*slot)))
            || surface.colors.iter().any(|color| color & 0xff000000 != 0)
            || surface.size > i32::MAX as u32
            || surface.character_spacing > i32::MAX as u32
            || surface.line_spacing > i32::MAX as u32
            || surface.glyphs.len() > 65_535
            || surface.virtual_size.contains(&0)
            || surface.physical_size.contains(&0)
        {
            return Err(fail());
        }
        let owner = state
            .native_font_owners
            .get(&(762 + u16::from(*slot)))
            .ok_or_else(fail)?;
        for glyph in &surface.glyphs {
            let (text, _, invalid_text) = encoding_rs::SHIFT_JIS.encode(&glyph.text);
            let (font, _, invalid_font) = encoding_rs::SHIFT_JIS.encode(&glyph.font);
            let code_bytes = glyph.code.to_be_bytes();
            let source = if code_bytes[0] == 0 {
                &code_bytes[1..]
            } else {
                &code_bytes[..]
            };
            let decoded =
                encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(source);
            if glyph.text.chars().count() != 1
                || text.len() > 2
                || text.contains(&0)
                || invalid_text
                || invalid_font
                || font.len() >= 64
                || font.contains(&0)
                || !(1..=4096).contains(&glyph.size)
                || glyph.colors.iter().any(|color| color & 0xff00_0000 != 0)
                || decoded.as_deref() != Some(glyph.text.as_str())
                || glyph.raster.owner != owner.raster.owner
                || glyph.raster.generation > owner.raster.generation
                || glyph.raster.custom_bitmap_identity.is_some()
                    != glyph.raster.custom_advance.is_some()
                || glyph.raster.custom_bitmap_identity.is_some_and(|identity| {
                    state
                        .native_custom_glyphs
                        .bitmap_by_identity(identity)
                        .is_none()
                })
            {
                return Err(fail());
            }
        }
    }
    Ok(())
}

pub(super) fn execute(
    state: &mut CmvsPs2aVmState,
    operation: CmvsNativeTextOperation,
    values: &[(CmvsPs2aCommandStackWordKind, u32)],
) -> Result<Option<CmvsPs2aVmAction>, CoreError> {
    use CmvsNativeTextOperation::*;
    if super::native_glyph::is_operation(operation) {
        return super::native_glyph::execute(state, operation, values);
    }
    let expected = match operation {
        Create | Colors | Cursor | Position => 3,
        Rect => 5,
        Metrics => 4,
        Destroy | Clear => 1,
        Font | Bold | AlternateCoverage | DefaultFont | Kinsoku | DrawStatic | ColorMode
        | MeasureProportional | MeasureFixed => 2,
        _ => unreachable!(),
    };
    if values.len() != expected {
        return Err(invalid(
            "SHINKI_EMU_CMVS_VM_COMMAND_CONTRACT",
            "native text argument count differs",
        ));
    }
    if operation == DefaultFont {
        // 470D50 resolves both source and destination before the branch.
        text::resolve_tagged_string_bytes(state, values[1].1)?;
        if values[0].1 == 0 {
            let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(&state.native_default_font);
            let bytes = bytes.into_owned();
            state.write_native_string_reference(values[1].1, &bytes)?;
            state.write_interpreter_word(81220, 1);
            return Ok(None);
        }
        let font = font_name(state, values[1].1)?;
        state.native_default_font.clone_from(&font);
        return Ok(Some(CmvsPs2aVmAction::NativeText(
            CmvsNativeTextAction::SetDefaultFont { font },
        )));
    }
    if operation == AlternateCoverage {
        if values[0].1 != 0 {
            font_owner::set_alternate(state, values[1].1 != 0)?;
        } else {
            let enabled = state
                .interpreter_words
                .get(&font_owner::NATIVE_FONT_ALTERNATE_FIELD)
                .copied()
                .unwrap_or(0)
                != 0;
            state.write_interpreter_word(81220, u32::from(enabled));
        }
        return Ok(None);
    }
    // Native string resolution precedes the slot guard in these handlers.
    let string = match operation {
        Create | Font => Some(font_name(state, values[1].1)?),
        DrawStatic | MeasureProportional | MeasureFixed => {
            Some(text::resolve_string_value(state, values[1].1)?)
        }
        _ => None,
    };
    let index = values[0].1;
    if index >= 12 {
        raise_error_flag(state, 10532, 0x1000)?;
        return Ok(None);
    }
    let slot = index as u8;
    let key = 750 << 8 | index;
    let action = match operation {
        Create => {
            let font = string.unwrap();
            font_owner::ensure_owner(state, font_owner::NATIVE_TEXT_FONT_OWNER_BASE, slot)?;
            let owner = state
                .native_font_owners
                .get_mut(&(font_owner::NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot)))
                .unwrap();
            owner.set_family(if font.is_empty() {
                &state.native_default_font
            } else {
                &font
            })?;
            let bold = owner.raster.bold;
            allocate(state, 750, slot)?;
            state.native_text_fonts.insert(slot, font.clone());
            let mut surface = CmvsNativeTextSurface::new(
                font,
                values[2].1,
                state.native_render_size,
                state.native_stage_size,
            );
            surface.bold = bold;
            state.native_text.insert(slot, surface);
            state.native_text_reveals.remove(&slot);
            state.effect_playback.insert(
                slot,
                CmvsEffectPlaybackRecord {
                    value: 12,
                    ..Default::default()
                },
            );
            state.write_interpreter_word(81220, 1);
            Some(CmvsNativeTextAction::Create { slot })
        }
        Destroy => {
            let existed = state.slot_objects.remove(&key).is_some();
            state.native_text.remove(&slot);
            state.native_text_reveals.remove(&slot);
            state.effect_playback.remove(&slot);
            existed.then_some(CmvsNativeTextAction::Destroy { slot })
        }
        _ => {
            if !state.slot_objects.contains_key(&key) {
                raise_error_flag(state, 10532, 0x1000)?;
                return Ok(None);
            }
            let surface = state.native_text.get_mut(&slot).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_CMVS_TEXT_OBJECT",
                    "native text object is not initialized",
                )
            })?;
            match operation {
                Rect => {
                    surface.rect = [
                        values[1].1 as i32,
                        values[2].1 as i32,
                        values[3].1 as i32,
                        values[4].1 as i32,
                    ];
                    surface.cursor = [surface.rect[0], surface.rect[1]];
                    None
                }
                Clear => {
                    surface.cursor = [surface.rect[0], surface.rect[1]];
                    surface.glyphs.clear();
                    state.native_text_reveals.remove(&slot);
                    Some(CmvsNativeTextAction::Clear { slot })
                }
                Metrics => {
                    if values[1].1 as i32 > 0 {
                        surface.size = values[1].1;
                    }
                    if values[2].1 as i32 >= 0 {
                        surface.character_spacing = values[2].1;
                    }
                    if values[3].1 as i32 >= 0 {
                        surface.line_spacing = values[3].1;
                    }
                    None
                }
                Font => {
                    let font = string.unwrap();
                    state
                        .native_font_owners
                        .get_mut(&(font_owner::NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot)))
                        .ok_or_else(|| {
                            invalid(
                                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                                "native font manager is absent",
                            )
                        })?
                        .set_family(if font.is_empty() {
                            &state.native_default_font
                        } else {
                            &font
                        })?;
                    surface.font.clone_from(&font);
                    state.native_text_fonts.insert(slot, font);
                    Some(CmvsNativeTextAction::SetFont { slot })
                }
                Bold => {
                    let bold = values[1].1 != 0;
                    if surface.bold != bold {
                        let owner = state
                            .native_font_owners
                            .get_mut(&(font_owner::NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot)))
                            .ok_or_else(|| {
                                invalid(
                                    "SHINKI_EMU_CMVS_TEXT_OBJECT",
                                    "native font manager is absent",
                                )
                            })?;
                        owner.invalidate()?;
                        owner.raster.bold = bold;
                        surface.bold = bold;
                    }
                    None
                }
                Colors => {
                    for (color, value) in surface.colors.iter_mut().zip(&values[1..]) {
                        if value.1 & 0xff00_0000 == 0 {
                            *color = value.1;
                        }
                    }
                    None
                }
                Kinsoku => {
                    surface.kinsoku = values[1].1 != 0;
                    None
                }
                Cursor => {
                    surface.cursor = [values[1].1 as i32, values[2].1 as i32];
                    None
                }
                Position => {
                    surface.position = [values[1].1 as i32, values[2].1 as i32];
                    None
                }
                ColorMode => {
                    surface.color_mode = values[1].1;
                    None
                }
                DrawStatic => {
                    let source = string.unwrap();
                    Some(CmvsNativeTextAction::DrawStatic { slot, source })
                }
                MeasureProportional | MeasureFixed => Some(CmvsNativeTextAction::Measure {
                    slot,
                    source: string.unwrap(),
                    proportional: operation == MeasureProportional,
                }),
                _ => unreachable!(),
            }
        }
    };
    Ok(action.map(CmvsPs2aVmAction::NativeText))
}

#[cfg(test)]
mod tests;
