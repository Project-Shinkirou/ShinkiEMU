use super::*;
use roukaku_text::PackagedFont;
mod coordinates;
mod fonts;
mod glyph;
mod glyph_aux;
mod paint;
mod raster;
#[cfg(test)]
pub(super) mod tests;

#[derive(Default)]
pub(super) struct NativeTextRenderer {
    database: Option<fontdb::Database>,
    fonts: BTreeMap<String, PackagedFont>,
    glyph_atlases: BTreeMap<u8, glyph::AtlasUpload>,
    glyph_sequence: u64,
    glyph_pending: Vec<SceneCommand>,
    bold_fonts: BTreeMap<String, PackagedFont>,
    raster: raster::NativeRasterRenderer,
    pub(super) default_font: Option<String>,
}

impl NativeTextRenderer {
    pub(super) fn native_restore_context(&self) -> Result<Self, CoreError> {
        let mut context = Self {
            database: self.database.clone(),
            default_font: self.default_font.clone(),
            bold_fonts: self.bold_fonts.clone(),
            ..Self::default()
        };
        if !self.fonts.is_empty() {
            context.install_fonts(self.fonts.clone())?;
        }
        Ok(context)
    }
    fn font<'a>(&'a self, vm: &CmvsPs2aVmState, slot: u8) -> Result<&'a str, CoreError> {
        let owner = vm
            .native_font_owners
            .get(&(762 + u16::from(slot)))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        self.fonts
            .get_key_value(&owner.family)
            .map(|(name, _)| name.as_str())
            .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))
    }

    pub(super) fn stamp_existing(
        &self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        stage_width: u32,
    ) -> Result<(), CoreError> {
        let surface = vm
            .native_text
            .get(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
        vm.stamp_native_text_glyphs(slot, 0, |size| coordinates.font_size(size))
    }

    pub(super) fn append(
        &self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
        stage_width: u32,
    ) -> Result<(), CoreError> {
        let settings = vm.native_text_raster_settings(slot)?;
        let proportional = settings.proportional;
        let font_name = self.font(vm, slot)?;
        let boundary = vm
            .effect_playback
            .get(&slot)
            .map(|record| &record.text_surface);
        let surface = vm
            .native_text
            .get_mut(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        let mut native = NativeMeasure {
            renderer: &self.raster,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
        let size = surface.size;
        let start = surface.glyphs.len();
        let mut measure = |text: &str| {
            coordinates.virtual_advance(native.advance(text, coordinates.font_size(size)?)?)
        };
        crate::vm::append_native_static_text(
            surface,
            source,
            boundary,
            if proportional {
                Some(&mut measure)
            } else {
                None
            },
        )?;
        vm.stamp_native_text_glyphs(slot, start, |size| coordinates.font_size(size))
    }
    pub(super) fn append_dialogue(
        &self,
        vm: &mut CmvsPs2aVmState,
        channel: u8,
        slot: u8,
        source: &str,
        stage_width: u32,
    ) -> Result<(), CoreError> {
        let settings = vm.native_text_raster_settings(slot)?;
        let proportional = settings.proportional;
        let font_name = self.font(vm, slot)?;
        let options = crate::vm::native_text::dialogue::options(vm, slot);
        let boundary = vm
            .effect_playback
            .get(&slot)
            .map(|record| &record.text_surface);
        let surface = vm
            .native_text
            .get_mut(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        let mut native = NativeMeasure {
            renderer: &self.raster,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
        let start = surface.glyphs.len();
        let mut measure = |text: &str, size: u32| {
            coordinates.virtual_advance(native.advance(text, coordinates.font_size(size)?)?)
        };
        let reveals = crate::vm::native_text::dialogue_layout::append(
            surface,
            source,
            options,
            boundary,
            if proportional {
                Some(&mut measure)
            } else {
                None
            },
        )?;
        vm.stamp_native_text_glyphs(slot, start, |size| coordinates.font_size(size))?;
        crate::vm::native_text::dialogue::layout_complete(vm, channel, slot, reveals)
    }

    pub(super) fn measure(
        &self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
        proportional: bool,
        stage_width: u32,
    ) -> Result<[u32; 3], CoreError> {
        let settings = vm.native_text_raster_settings(slot)?;
        let font_name = self.font(vm, slot)?;
        let surface = vm
            .native_text
            .get(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        let boundary = vm
            .effect_playback
            .get(&slot)
            .map(|record| &record.text_surface);
        if source.is_empty() {
            return Ok([0, 1, 0]);
        }
        if !proportional {
            return crate::vm::native_text::measurement::measure(surface, source, boundary, None);
        }
        let mut native = NativeMeasure {
            renderer: &self.raster,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
        let size = surface.size;
        let mut measure = |text: &str| {
            coordinates.virtual_advance(native.advance(text, coordinates.font_size(size)?)?)
        };
        crate::vm::native_text::measurement::measure(surface, source, boundary, Some(&mut measure))
    }
    pub(super) fn bind_font(&mut self, name: &str) -> Result<(), CoreError> {
        if self.fonts.contains_key(name) {
            return Ok(());
        }
        if self.fonts.len() >= 64 {
            return Err(error("SHINKI_EMU_CMVS_FONT_BOUND"));
        }
        let database = self.database.get_or_insert_with(fonts::system_database);
        let resolved = if name.is_empty() {
            self.default_font
                .as_deref()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_DEFAULT_FONT_UNBOUND"))?
        } else {
            name
        };
        let font = fonts::load(database, resolved, 400)?;
        let bold = if fonts::has_distinct_bold_face(database, resolved)? {
            Some(fonts::load(database, resolved, 600)?)
        } else {
            None
        };
        self.fonts.insert(name.to_owned(), font);
        if let Some(bold) = bold {
            self.bold_fonts.insert(name.to_owned(), bold);
        }
        if let Err(error) = self.raster.bind_fonts(&self.fonts, &self.bold_fonts) {
            self.fonts.remove(name);
            self.bold_fonts.remove(name);
            return Err(error);
        }
        Ok(())
    }

    fn install_fonts(&mut self, fonts: BTreeMap<String, PackagedFont>) -> Result<(), CoreError> {
        self.raster.bind_fonts(&fonts, &self.bold_fonts)?;
        self.fonts = fonts;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn frame(
        &mut self,
        vm: &CmvsPs2aVmState,
        stage_width: u32,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        self.frame_with_owners(vm, stage_width, &mut Vec::new())
    }

    fn frame_with_owners(
        &mut self,
        vm: &CmvsPs2aVmState,
        stage_width: u32,
        owners: &mut Vec<(u8, usize)>,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let mut surfaces = [None; 12];
        let mut count = 0;
        for entry in vm.native_text.iter().filter(|(_, surface)| surface.visible) {
            *surfaces
                .get_mut(count)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))? = Some(entry);
            count += 1;
        }
        let mut runs = Vec::new();
        let mut run_owners = Vec::new();
        for (slot, surface) in surfaces[..count].iter().flatten().copied() {
            let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
            for (index, glyph) in surface.glyphs.iter().enumerate() {
                let size = coordinates.font_size(glyph.size)?;
                let Some(visual) = crate::vm::native_text::dialogue::visual(vm, *slot, index, size)
                else {
                    continue;
                };
                let font_name = if self.fonts.contains_key(&glyph.font) {
                    glyph.font.as_str()
                } else {
                    self.default_font
                        .as_deref()
                        .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?
                };
                run_owners.push((*slot, index));
                runs.push(raster::NativeRasterRun {
                    id: format!("cmvs.text.{slot}.{index}"),
                    font_name,
                    style: glyph,
                    origin: coordinates.position(surface.position, glyph.position)?,
                    visual,
                    pixel_size: size,
                    logical_scale: coordinates.logical_scale(),
                    custom: glyph
                        .raster
                        .custom_bitmap_identity
                        .and_then(|identity| vm.native_custom_glyphs.bitmap_by_identity(identity)),
                });
            }
        }
        let mut drawn = Vec::new();
        let commands = self.raster.frame_with_owners(&runs, &mut drawn)?;
        owners.extend(drawn.into_iter().map(|index| run_owners[index]));
        Ok(commands)
    }
}

struct NativeMeasure<'a> {
    renderer: &'a raster::NativeRasterRenderer,
    font_name: &'a str,
    slot: u8,
    settings: crate::CmvsNativeFontRasterState,
    owners: &'a mut BTreeMap<u16, crate::CmvsNativeFontOwner>,
    custom: &'a mut crate::CmvsNativeCustomGlyphManager,
}

impl NativeMeasure<'_> {
    fn advance(&mut self, text: &str, size: u32) -> Result<u32, CoreError> {
        let mut total = 0u32;
        let mut utf8 = [0; 4];
        for character in text.chars() {
            let code = raster::code(character.encode_utf8(&mut utf8))?;
            let raster = crate::vm::native_text::font_owner::native_text_raster_from(
                self.owners,
                self.custom,
                self.slot,
                code,
                size,
                self.settings,
            )?;
            total = total
                .checked_add(self.renderer.advance(self.font_name, code, size, raster)?)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_ADVANCE"))?;
        }
        Ok(total)
    }
}

impl CmvsScene {
    pub(crate) fn measure_text(
        &mut self,
        vm: &mut CmvsPs2aVmState,
        slot: u8,
        source: &str,
        proportional: bool,
    ) -> Result<(), CoreError> {
        let width = self.logical_extent().map_or(0, |extent| extent.width);
        let [maximum, lines, last] = self.text.measure(vm, slot, source, proportional, width)?;
        vm.write_interpreter_word(81224, maximum);
        vm.write_interpreter_word(81228, lines);
        vm.write_interpreter_word(81236, last);
        vm.write_interpreter_word(81220, 1);
        tracing::debug!(
            event = "cmvs.text.measure",
            slot,
            proportional,
            pc = vm.program_counter,
            frame = vm.current_frame,
            tick = vm.tick,
            source_bytes = source.len(),
            maximum,
            lines,
            last
        );
        Ok(())
    }
}
