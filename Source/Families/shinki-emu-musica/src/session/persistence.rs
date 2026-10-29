use super::*;
impl MusicaSession {
    pub(crate) fn install_imported_message(
        &mut self,
        text: String,
        speaker: Option<String>,
    ) -> SessionResult<()> {
        self.message(text, speaker)
    }
    /// Writes `system.dat` when a `.setglobal` changed the title-wide table.
    pub(super) fn persist_globals(&mut self) -> SessionResult<()> {
        let globals = &self.vm.state().global_variables;
        if *globals != self.persisted_globals {
            self.native.store_globals(globals)?;
            self.persisted_globals = globals.clone();
        }
        Ok(())
    }
    pub(super) fn quick_save(&mut self) -> SessionResult<()> {
        let pc_line = self.vm.state().pc_line;
        if self.last_quick_save_pc_line == Some(pc_line) {
            return Ok(());
        }
        let slot = 10 + self.quick_cursor;
        self.save(slot)?;
        let next = (self.quick_cursor + 1) % crate::storage::SAVE_PAGE_WIDTH;
        self.storage.write_quick_cursor(self.game, next)?;
        self.quick_cursor = next;
        self.last_quick_save_pc_line = Some(pc_line);
        tracing::debug!(event = "shinki.emu.musica.quick_save.rotated", slot, next);
        Ok(())
    }
    pub(super) fn quick_load(&mut self) -> SessionResult<()> {
        self.load(
            10 + (self.quick_cursor + crate::storage::SAVE_PAGE_WIDTH - 1)
                % crate::storage::SAVE_PAGE_WIDTH,
        )
    }

    pub(super) fn save(&mut self, slot: u32) -> SessionResult<()> {
        let sounds = self.audio.snapshot()?;
        self.poll_voice_duration()?;
        let vm = self
            .vm
            .encode_native_save()
            .map_err(|_| error("SHINKI_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        let mut state = MusicaVm::decode_native_save(&vm).map_err(vm_error)?;
        if matches!(
            state.system_ui.page,
            crate::MusicaSystemPage::Save | crate::MusicaSystemPage::Load
        ) {
            state.system_ui.page = crate::MusicaSystemPage::None;
            state.system_ui.focus_index = 0;
        }
        let vm = postcard::to_allocvec(&state)
            .map_err(|_| error("SHINKI_EMU_MUSICA_SAVE_STATE", "VM state cannot be saved"))?;
        let pixels = self.gameplay_frame.as_ref().unwrap_or(&self.scene.pixels);
        let card = crate::storage::SaveCard::capture(
            self.info.logical_width,
            self.info.logical_height,
            self.info.width,
            self.info.height,
            pixels,
        )?;
        self.storage.write(
            slot,
            &Snapshot {
                logical_extent: [self.info.logical_width, self.info.logical_height],
                card,
                game: self.game,
                vm,
                message: self.message.clone(),
                translation_visible: self.translation_visible,
                translation_overlay: self.translation_overlay.clone(),
                wait_ns: self.wait_ns,
                sounds,
            },
        )?;
        tracing::info!(event = "shinki.emu.musica.save.completed", slot);
        Ok(())
    }
    pub(super) fn load(&mut self, slot: u32) -> SessionResult<()> {
        let mut saved = self.storage.read(slot)?;
        if saved.logical_extent != [self.info.logical_width, self.info.logical_height] {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_CANVAS",
                "save logical canvas differs from this session",
            ));
        }
        if saved.game != self.game {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_GAME",
                "save belongs to another archive/profile identity",
            ));
        }
        let state = MusicaVm::decode_native_save(&saved.vm)
            .map_err(|_| error("SHINKI_EMU_MUSICA_SAVE_STATE", "VM save is invalid"))?;
        let loaded = crate::script_loader::load_script(
            &self.archive,
            &state.script_uri,
            self.primary_encoding,
        )?;
        if loaded.hash != state.script_hash {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_SCRIPT",
                "saved script or include has changed",
            ));
        }
        if loaded.script.encoding != state.script_encoding {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_ENCODING",
                "saved script encoding no longer matches",
            ));
        }
        let script = loaded.script;
        let mut vm = MusicaVm::new(
            state.script_uri,
            state.script_hash,
            script,
            state.session_seed,
        )
        .map_err(|_| error("SHINKI_EMU_MUSICA_SAVE_STATE", "saved script is invalid"))?;
        vm.set_config(self.vm.config().clone()).map_err(vm_error)?;
        vm.restore_native_save(&saved.vm, 1)
            .map_err(|_| error("SHINKI_EMU_MUSICA_SAVE_STATE", "saved VM state is invalid"))?;
        vm.set_launch_mode(self.vm.state().launch_mode);
        // Globals are title-wide (`system.dat`), never restored from a slot.
        vm.replace_global_variables(self.vm.state().global_variables.clone());
        let mut scene = Scene::new_scaled(
            self.scene.gpu_instance(),
            self.archive.clone(),
            self.info.logical_width,
            self.info.logical_height,
            self.info.width,
            self.info.height,
            self.scene.texture_overrides(),
            state.script_encoding,
        )?;
        scene.set_text_shadow(self.scene.text_shadow());
        scene.set_configured_font(self.scene.configured_font())?;
        scene.set_translation_overlay(saved.translation_overlay.clone(), saved.translation_visible);
        #[cfg(feature = "ffmpeg-vcpkg")]
        {
            scene.set_subtitle_catalog(self.scene.subtitle_catalog());
            if let Some(movie) = vm.state().movie.as_ref() {
                scene.begin_movie(&movie.resource_uri)?;
            }
        }
        let choices = Self::localized_choices_for(
            &vm,
            self.translation_table.as_deref(),
            self.translation_mode,
        )?;
        scene.render(
            vm.state(),
            saved.message.as_ref(),
            choices
                .as_ref()
                .map(|(labels, index)| (labels.as_slice(), *index)),
        )?;
        self.cancel_text()?;
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        for sound in &mut saved.sounds {
            if sound.id == 4 && !self.vm.voice_preferences().enabled(&sound.uri) {
                sound.playing = false;
                sound.fade = None;
            }
        }
        if let Some(movie) = self.movie.take() {
            movie.close(&self.audio)?;
        }
        self.audio.restore(saved.sounds)?;
        let movie = vm
            .state()
            .movie
            .as_ref()
            .map(|state| crate::movie::Movie::open(&self.archive, state))
            .transpose()?;
        vm.set_control_pressed(self.control_keys != 0);
        self.voice_duration = None;
        self.load_from_title = false;
        self.title_focus = None;
        self.gameplay_frame = None;
        self.save_cards.clear();
        self.vm = vm;
        self.movie = movie;
        self.scene = scene;
        self.message = saved.message;
        self.translation_visible = saved.translation_visible;
        self.translation_overlay = saved.translation_overlay;
        self.wait_ns = saved.wait_ns;
        self.phase = 0;
        self.input_pending = false;
        self.pointer = None;
        self.finished = self.vm.state().terminal;
        self.reset_host_clock = true;
        tracing::info!(event = "shinki.emu.musica.load.completed", slot);
        Ok(())
    }
}
