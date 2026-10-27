use super::*;
use roukaku_text::PackagedFont;
#[cfg(not(windows))]
use roukaku_text::{
    CosmicTextLayoutProvider, FontBindingContext, LayoutConstraint, OverflowPolicy, TextDirection,
    TextLayoutConfig, TextLayoutRequest, TextRun, WrapPolicy,
};
#[cfg(not(windows))]
use shinki_emu_sdk::{TextScene, TextSceneLayout};
mod coordinates;
mod fonts;
#[cfg(windows)]
mod gdi;
mod glyph;
mod glyph_aux;
mod paint;
#[cfg(test)]
pub(super) mod tests;

#[derive(Default)]
pub(super) struct NativeTextRenderer {
    database: Option<fontdb::Database>,
    fonts: BTreeMap<String, PackagedFont>,
    glyph_atlases: BTreeMap<u8, glyph::AtlasUpload>,
    glyph_sequence: u64,
    glyph_pending: Vec<SceneCommand>,
    #[cfg(windows)]
    bold_fonts: BTreeMap<String, PackagedFont>,
    #[cfg(windows)]
    gdi: gdi::NativeGdiRenderer,
    #[cfg(not(windows))]
    scene: Option<TextScene>,
    #[cfg(not(windows))]
    pending: Vec<SceneCommand>,
    #[cfg(not(windows))]
    paint: paint::NativePaint,
    pub(super) default_font: Option<String>,
}

impl NativeTextRenderer {
    pub(super) fn native_restore_context(&self) -> Result<Self, CoreError> {
        let mut context = Self {
            database: self.database.clone(),
            default_font: self.default_font.clone(),
            ..Self::default()
        };
        #[cfg(windows)]
        {
            context.bold_fonts = self.bold_fonts.clone();
        }
        if !self.fonts.is_empty() {
            context.install_fonts(self.fonts.clone())?;
        }
        Ok(context)
    }
    fn font<'a>(
        &'a self,
        vm: &CmvsPs2aVmState,
        slot: u8,
    ) -> Result<(&'a str, &'a PackagedFont), CoreError> {
        let owner = vm
            .native_font_owners
            .get(&(762 + u16::from(slot)))
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        self.fonts
            .get_key_value(&owner.family)
            .map(|(name, font)| (name.as_str(), font))
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
        let (font_name, font) = self.font(vm, slot)?;
        let boundary = vm
            .effect_playback
            .get(&slot)
            .map(|record| &record.text_surface);
        let surface = vm
            .native_text
            .get_mut(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        #[cfg(windows)]
        let mut native = NativeMeasure {
            renderer: &self.gdi,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        #[cfg(not(windows))]
        let mut native = NativeMeasure {
            scene: self
                .scene
                .as_ref()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?,
            font,
        };
        #[cfg(windows)]
        let _ = font;
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
        let (font_name, font) = self.font(vm, slot)?;
        let options = crate::vm::native_text::dialogue::options(vm, slot);
        let boundary = vm
            .effect_playback
            .get(&slot)
            .map(|record| &record.text_surface);
        let surface = vm
            .native_text
            .get_mut(&slot)
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))?;
        #[cfg(windows)]
        let mut native = NativeMeasure {
            renderer: &self.gdi,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        #[cfg(not(windows))]
        let mut native = NativeMeasure {
            scene: self
                .scene
                .as_ref()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?,
            font,
        };
        #[cfg(windows)]
        let _ = font;
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
        let (font_name, font) = self.font(vm, slot)?;
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
        #[cfg(windows)]
        let mut native = NativeMeasure {
            renderer: &self.gdi,
            font_name,
            slot,
            settings,
            owners: &mut vm.native_font_owners,
            custom: &mut vm.native_custom_glyphs,
        };
        #[cfg(not(windows))]
        let mut native = NativeMeasure {
            scene: self
                .scene
                .as_ref()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?,
            font,
        };
        #[cfg(windows)]
        let _ = font;
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
        let database = self.database.get_or_insert_with(|| {
            let mut database = fontdb::Database::new();
            database.load_system_fonts();
            database
        });
        let resolved = if name.is_empty() {
            self.default_font
                .as_deref()
                .ok_or_else(|| error("SHINKI_EMU_CMVS_DEFAULT_FONT_UNBOUND"))?
        } else {
            name
        };
        let font = fonts::load(database, resolved, 400)?;
        #[cfg(windows)]
        {
            let bold = if fonts::has_distinct_bold_face(database, resolved)? {
                Some(fonts::load(database, resolved, 600)?)
            } else {
                None
            };
            self.fonts.insert(name.to_owned(), font);
            if let Some(bold) = bold {
                self.bold_fonts.insert(name.to_owned(), bold);
            }
            if let Err(error) = self.gdi.bind_fonts(&self.fonts, &self.bold_fonts) {
                self.fonts.remove(name);
                self.bold_fonts.remove(name);
                return Err(error);
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let mut fonts = self.fonts.clone();
            fonts.insert(name.to_owned(), font);
            self.install_fonts(fonts)
        }
    }

    #[cfg(windows)]
    fn install_fonts(&mut self, fonts: BTreeMap<String, PackagedFont>) -> Result<(), CoreError> {
        self.gdi.bind_fonts(&fonts, &self.bold_fonts)?;
        self.fonts = fonts;
        Ok(())
    }

    #[cfg(not(windows))]
    fn install_fonts(&mut self, fonts: BTreeMap<String, PackagedFont>) -> Result<(), CoreError> {
        let unique = fonts
            .values()
            .map(|font| (font.asset_id.clone(), font.clone()))
            .collect::<BTreeMap<_, _>>();
        let provider = CosmicTextLayoutProvider::new(
            FontBindingContext {
                target: "shinki-emu-cmvs".into(),
                profile: "cmvs.native".into(),
                default_locale: "ja-JP".into(),
            },
            unique.into_values().collect(),
            TextLayoutConfig::production_defaults(),
        )
        .map_err(|_| error("SHINKI_EMU_CMVS_FONT_PROVIDER"))?;
        if let Some(scene) = &mut self.scene {
            self.pending.extend(scene.clear());
        }
        self.scene = Some(TextScene::new(provider));
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

    #[cfg(windows)]
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
                runs.push(gdi::NativeGdiRun {
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
        let commands = self.gdi.frame_with_owners(&runs, &mut drawn)?;
        owners.extend(drawn.into_iter().map(|index| run_owners[index]));
        Ok(commands)
    }

    #[cfg(not(windows))]
    fn frame_with_owners(
        &mut self,
        vm: &CmvsPs2aVmState,
        stage_width: u32,
        owners: &mut Vec<(u8, usize)>,
    ) -> Result<Vec<SceneCommand>, CoreError> {
        let mut regions = Vec::new();
        let mut styles = BTreeMap::new();
        let mut run_owners = BTreeMap::new();
        let mut surfaces = [None; 12];
        let mut count = 0;
        for entry in vm.native_text.iter().filter(|(_, surface)| surface.visible) {
            *surfaces
                .get_mut(count)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_OBJECT"))? = Some(entry);
            count += 1;
        }
        for (slot, surface) in surfaces[..count].iter().flatten().copied() {
            let coordinates = coordinates::NativeCoordinates::new(surface, stage_width)?;
            for (index, glyph) in surface.glyphs.iter().enumerate() {
                let size = coordinates.font_size(glyph.size)?;
                let Some(visual) = crate::vm::native_text::dialogue::visual(vm, *slot, index, size)
                else {
                    continue;
                };
                let id = format!("cmvs.text.{slot}.{index}");
                let font_name = if self.fonts.contains_key(&glyph.font) {
                    glyph.font.clone()
                } else {
                    self.default_font
                        .clone()
                        .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?
                };
                let font = self
                    .fonts
                    .get(&font_name)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_FONT_BINDING"))?;
                let [x, y] = coordinates.position(surface.position, glyph.position)?;
                regions.push(TextSceneLayout {
                    translation: (x, y),
                    rgba: [255; 4],
                    outline: None,
                    request: TextLayoutRequest {
                        key: id.clone(),
                        runs: vec![TextRun {
                            text: glyph.text.clone(),
                            language: "ja-JP".into(),
                            script: Some("Jpan".into()),
                            direction: TextDirection::LeftToRight,
                            ruby: Vec::new(),
                            voice: None,
                        }],
                        constraint: LayoutConstraint {
                            max_width: size as f32 * 4.0,
                            max_height: None,
                            max_lines: None,
                            font_size: size as f32,
                            line_height: size as f32,
                            wrap: WrapPolicy::None,
                            overflow: OverflowPolicy::Visible,
                        },
                        font_families: vec![font.family.clone()],
                        features: Vec::new(),
                    },
                });
                run_owners.insert(id.clone(), (*slot, index));
                styles.insert(id, (glyph, [x, y], visual, size));
            }
        }
        let mut commands = std::mem::take(&mut self.pending);
        if let Some(scene) = &mut self.scene {
            commands.extend(
                scene
                    .frame(&regions)
                    .map_err(|_| error("SHINKI_EMU_CMVS_TEXT_SHAPING"))?,
            );
        } else if !regions.is_empty() {
            return Err(error("SHINKI_EMU_CMVS_FONT_BINDING"));
        }
        let mut drawn = Vec::new();
        let commands = self
            .paint
            .project_with_owners(commands, &styles, &mut drawn)?;
        for id in drawn {
            owners.push(
                *run_owners
                    .get(&id)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TEXT_COMMAND"))?,
            );
        }
        Ok(commands)
    }
}

#[cfg(windows)]
struct NativeMeasure<'a> {
    renderer: &'a gdi::NativeGdiRenderer,
    font_name: &'a str,
    slot: u8,
    settings: crate::CmvsNativeFontRasterState,
    owners: &'a mut BTreeMap<u16, crate::CmvsNativeFontOwner>,
    custom: &'a mut crate::CmvsNativeCustomGlyphManager,
}

#[cfg(windows)]
impl NativeMeasure<'_> {
    fn advance(&mut self, text: &str, size: u32) -> Result<u32, CoreError> {
        let mut total = 0u32;
        let mut utf8 = [0; 4];
        for character in text.chars() {
            let code = gdi::code(character.encode_utf8(&mut utf8))?;
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

#[cfg(not(windows))]
struct NativeMeasure<'a> {
    scene: &'a TextScene,
    font: &'a PackagedFont,
}

#[cfg(not(windows))]
impl NativeMeasure<'_> {
    fn advance(&mut self, text: &str, size: u32) -> Result<u32, CoreError> {
        measure_advance(self.scene, self.font, text, size)
    }
}

#[cfg(not(windows))]
fn measure_advance(
    scene: &TextScene,
    font: &PackagedFont,
    text: &str,
    size: u32,
) -> Result<u32, CoreError> {
    let request = request(
        format!(
            "cmvs.measure.{}.{}.{}",
            font.asset_id,
            size,
            text.chars().next().unwrap_or('\0') as u32
        ),
        text,
        size,
        &font.family,
    );
    let layout = scene
        .layout_shared(&request)
        .map_err(|_| error("SHINKI_EMU_CMVS_TEXT_SHAPING"))?;
    let advance = layout
        .shaped_runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|glyph| glyph.advance)
        .sum::<f32>();
    if !advance.is_finite() || !(0.0..=4096.0).contains(&advance) {
        return Err(error("SHINKI_EMU_CMVS_TEXT_ADVANCE"));
    }
    Ok(advance.round() as u32)
}

#[cfg(not(windows))]
fn request(key: String, text: &str, size: u32, family: &str) -> TextLayoutRequest {
    TextLayoutRequest {
        key,
        runs: vec![TextRun {
            text: text.into(),
            language: "ja-JP".into(),
            script: Some("Jpan".into()),
            direction: TextDirection::LeftToRight,
            ruby: Vec::new(),
            voice: None,
        }],
        constraint: LayoutConstraint {
            max_width: size as f32 * 4.0,
            max_height: None,
            max_lines: None,
            font_size: size as f32,
            line_height: size as f32,
            wrap: WrapPolicy::None,
            overflow: OverflowPolicy::Visible,
        },
        font_families: vec![family.into()],
        features: Vec::new(),
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
