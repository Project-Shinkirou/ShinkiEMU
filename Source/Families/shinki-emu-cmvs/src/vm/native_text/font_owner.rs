use super::*;

pub const NATIVE_TEXT_FONT_OWNER_BASE: u16 = 762;
pub const NATIVE_GLYPH_FONT_OWNER_BASE: u16 = 783;
pub(crate) const NATIVE_FONT_PROPORTIONAL: u32 = 0x004f_0580;
pub(crate) const NATIVE_FONT_ALTERNATE_FIELD: u32 = 1496;

/// A native uploaded glyph retains these creation-time inputs. Paint style is
/// independent of CreateFont's 400/600 weight.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct CmvsNativeFontRasterState {
    pub owner: u64,
    pub generation: u32,
    pub bold: bool,
    pub proportional: bool,
    pub shift_jis: bool,
    pub alternate_coverage: bool,
    pub custom_bitmap_identity: Option<u64>,
    pub custom_advance: Option<u16>,
}

impl Default for CmvsNativeFontRasterState {
    fn default() -> Self {
        Self {
            owner: 0,
            generation: 0,
            bold: false,
            proportional: false,
            shift_jis: true,
            alternate_coverage: false,
            custom_bitmap_identity: None,
            custom_advance: None,
        }
    }
}

/// 461700's char/physical-size cache belongs to the font manager, not CText.
/// In particular command 940 does not invalidate entries already generated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CmvsNativeFontOwner {
    pub raster: CmvsNativeFontRasterState,
    pub family: String,
    pub cache: BTreeMap<(u16, u32), CmvsNativeFontRasterState>,
}

impl CmvsNativeFontOwner {
    pub fn new(owner: u64, bold: bool) -> Self {
        Self {
            raster: CmvsNativeFontRasterState {
                owner,
                bold,
                ..Default::default()
            },
            family: String::new(),
            cache: BTreeMap::new(),
        }
    }

    pub fn invalidate(&mut self) -> Result<(), CoreError> {
        self.raster.generation = self.raster.generation.checked_add(1).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "native font generation exhausted",
            )
        })?;
        self.cache.clear();
        Ok(())
    }

    pub fn set_bold(&mut self, bold: bool) -> Result<(), CoreError> {
        if self.raster.bold != bold {
            self.invalidate()?;
            self.raster.bold = bold;
        }
        Ok(())
    }

    pub fn set_family(&mut self, family: &str) -> Result<(), CoreError> {
        if !self.family.eq_ignore_ascii_case(family) {
            self.invalidate()?;
            self.family.clear();
            self.family.push_str(family);
        }
        Ok(())
    }

    pub fn glyph(
        &mut self,
        code: u16,
        pixel_size: u32,
        settings: CmvsNativeFontRasterState,
    ) -> CmvsNativeFontRasterState {
        *self.cache.entry((code, pixel_size)).or_insert(settings)
    }
}

/// Custom lookup precedes the native GDI cache, including on cache hits.
pub(crate) fn native_text_raster_from(
    owners: &mut BTreeMap<u16, CmvsNativeFontOwner>,
    custom: &mut super::super::native_custom_glyph::CmvsNativeCustomGlyphManager,
    slot: u8,
    code: u16,
    pixel_size: u32,
    settings: CmvsNativeFontRasterState,
) -> Result<CmvsNativeFontRasterState, CoreError> {
    native_font_raster_from(
        owners,
        custom,
        NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot),
        code,
        pixel_size,
        settings,
    )
}

pub(crate) fn native_font_raster_from(
    owners: &mut BTreeMap<u16, CmvsNativeFontOwner>,
    custom: &mut super::super::native_custom_glyph::CmvsNativeCustomGlyphManager,
    owner_key: u16,
    code: u16,
    pixel_size: u32,
    mut settings: CmvsNativeFontRasterState,
) -> Result<CmvsNativeFontRasterState, CoreError> {
    if let Some(bitmap) = custom.lookup(code, pixel_size)? {
        settings.custom_bitmap_identity = Some(bitmap.identity);
        settings.custom_advance = Some(if settings.proportional {
            bitmap.descriptor
        } else {
            pixel_size as u16
        });
        return Ok(settings);
    }
    let owner = owners.get_mut(&owner_key).ok_or_else(|| {
        invalid(
            "SHINKI_EMU_CMVS_TEXT_OBJECT",
            "native font manager is absent",
        )
    })?;
    Ok(owner.glyph(code, pixel_size, settings))
}

impl CmvsPs2aVmState {
    pub fn native_text_raster_settings(
        &self,
        slot: u8,
    ) -> Result<CmvsNativeFontRasterState, CoreError> {
        self.native_font_raster_settings(NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot))
    }

    pub fn native_glyph_raster_settings(
        &self,
        slot: u8,
    ) -> Result<CmvsNativeFontRasterState, CoreError> {
        self.native_font_raster_settings(NATIVE_GLYPH_FONT_OWNER_BASE + u16::from(slot))
    }

    fn native_font_raster_settings(
        &self,
        owner_key: u16,
    ) -> Result<CmvsNativeFontRasterState, CoreError> {
        let owner = self.native_font_owners.get(&owner_key).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "native font manager is absent",
            )
        })?;
        let mut settings = owner.raster;
        settings.proportional = self
            .process_global_words
            .get(&NATIVE_FONT_PROPORTIONAL)
            .copied()
            .unwrap_or(0)
            != 0;
        settings.shift_jis = self
            .process_global_words
            .get(&NATIVE_FONT_SHIFT_JIS)
            .copied()
            .unwrap_or(1)
            != 0;
        settings.alternate_coverage = self
            .interpreter_words
            .get(&NATIVE_FONT_ALTERNATE_FIELD)
            .copied()
            .unwrap_or(0)
            != 0;
        Ok(settings)
    }

    pub fn native_text_raster(
        &mut self,
        slot: u8,
        code: u16,
        pixel_size: u32,
    ) -> Result<CmvsNativeFontRasterState, CoreError> {
        let settings = self.native_text_raster_settings(slot)?;
        native_text_raster_from(
            &mut self.native_font_owners,
            &mut self.native_custom_glyphs,
            slot,
            code,
            pixel_size,
            settings,
        )
    }

    pub fn native_glyph_raster(
        &mut self,
        slot: u8,
        code: u16,
        pixel_size: u32,
    ) -> Result<CmvsNativeFontRasterState, CoreError> {
        let settings = self.native_glyph_raster_settings(slot)?;
        native_font_raster_from(
            &mut self.native_font_owners,
            &mut self.native_custom_glyphs,
            NATIVE_GLYPH_FONT_OWNER_BASE + u16::from(slot),
            code,
            pixel_size,
            settings,
        )
    }

    pub fn stamp_native_text_glyphs(
        &mut self,
        slot: u8,
        start: usize,
        pixel_size: impl Fn(u32) -> Result<u32, CoreError>,
    ) -> Result<(), CoreError> {
        let settings = self.native_text_raster_settings(slot)?;
        let surface = self.native_text.get_mut(&slot).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "native text surface is absent",
            )
        })?;
        for glyph in surface.glyphs.get_mut(start..).ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_TEXT_OBJECT",
                "native glyph range exceeds its surface",
            )
        })? {
            glyph.font.clone_from(
                &self.native_font_owners[&(NATIVE_TEXT_FONT_OWNER_BASE + u16::from(slot))].family,
            );
            glyph.raster = native_text_raster_from(
                &mut self.native_font_owners,
                &mut self.native_custom_glyphs,
                slot,
                glyph.code,
                pixel_size(glyph.size)?,
                settings,
            )?;
        }
        Ok(())
    }
}

pub(crate) fn ensure_owner(
    state: &mut CmvsPs2aVmState,
    table: u16,
    slot: u8,
) -> Result<(), CoreError> {
    let key = u32::from(table) << 8 | u32::from(slot);
    if !state.slot_objects.contains_key(&key) {
        allocate(state, u32::from(table), slot)?;
    }
    let identity = state.slot_objects[&key];
    state
        .native_font_owners
        .entry(table + u16::from(slot))
        .or_insert_with(|| {
            let mut owner = CmvsNativeFontOwner::new(identity, false);
            owner.family.clone_from(&state.native_default_font);
            owner
        });
    Ok(())
}

pub(super) fn set_alternate(state: &mut CmvsPs2aVmState, enabled: bool) -> Result<(), CoreError> {
    let previous = state
        .interpreter_words
        .get(&NATIVE_FONT_ALTERNATE_FIELD)
        .copied()
        .unwrap_or(0)
        != 0;
    if previous != enabled {
        for owner in state.native_font_owners.values_mut() {
            owner.invalidate()?;
            owner.raster.alternate_coverage = enabled;
        }
        state.write_interpreter_word(NATIVE_FONT_ALTERNATE_FIELD, u32::from(enabled));
    }
    Ok(())
}

pub(super) fn validate(state: &CmvsPs2aVmState) -> Result<(), CoreError> {
    let fail = || {
        invalid(
            "SHINKI_EMU_CMVS_TEXT_OBJECT",
            "native font owner snapshot is inconsistent",
        )
    };
    let (default, _, malformed) = encoding_rs::SHIFT_JIS.encode(&state.native_default_font);
    if malformed || default.len() >= 64 || default.contains(&0) {
        return Err(fail());
    }
    for (&key, owner) in &state.native_font_owners {
        let (table, slot) = match key {
            762..=773 => (762, key - 762),
            783..=790 => (783, key - 783),
            _ => return Err(fail()),
        };
        let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(&owner.family);
        if malformed
            || bytes.len() >= 64
            || bytes.contains(&0)
            || state.slot_objects.get(&(table << 8 | u32::from(slot))) != Some(&owner.raster.owner)
        {
            return Err(fail());
        }
        for (&(_, size), raster) in &owner.cache {
            if !(1..=4096).contains(&size)
                || raster.owner != owner.raster.owner
                || raster.generation != owner.raster.generation
                || raster.bold != owner.raster.bold
                || raster.custom_bitmap_identity.is_some()
                || raster.custom_advance.is_some()
            {
                return Err(fail());
            }
        }
    }
    for (&slot, binding) in &state.native_glyphs {
        if slot >= 8
            || !state
                .slot_objects
                .contains_key(&(775 << 8 | u32::from(slot)))
            || !state
                .native_font_owners
                .contains_key(&(783 + u16::from(slot)))
        {
            return Err(fail());
        }
        let (bytes, _, malformed) = encoding_rs::SHIFT_JIS.encode(&binding.font);
        if malformed || bytes.len() >= 64 || bytes.contains(&0) {
            return Err(fail());
        }
    }
    super::super::native_glyph::validate(state)?;
    Ok(())
}
