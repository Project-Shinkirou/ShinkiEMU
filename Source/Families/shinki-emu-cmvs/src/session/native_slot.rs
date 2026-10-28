use super::*;
use crate::system_save::slot::{NativeSlot, encode_native_slot_with_preview};
use crate::vm::native_slot::{
    apply_native_interpreter, capture_native_interpreter, capture_native_program,
    install_native_program, reconcile_native_records_and_request29,
};

struct PreparedNativeSlot {
    vm: CmvsPs2aVmState,
    scripts: BTreeMap<u16, Arc<CmvsScript>>,
    script_names: BTreeMap<u16, String>,
    scene: CmvsScene,
    cmv_players: BTreeMap<u8, cmv::CmvPlayer>,
    audio: native_save_audio::PreparedNativeAudio,
    frame: TextureFrame,
    selected: Option<CmvsTextureSlot>,
}

fn owned_native_name(bytes: &[u8]) -> SessionResult<String> {
    let (text, malformed) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
    if malformed || bytes.contains(&0) {
        return Err(error(
            "SHINKI_EMU_CMVS_NATIVE_SLOT_NAME",
            "saved script name is invalid",
        ));
    }
    Ok(text.into_owned())
}

fn record_image(vm: &CmvsPs2aVmState) -> Vec<[u32; 7]> {
    (0..64)
        .map(|slot| {
            std::array::from_fn(|field| {
                vm.interpreter_words
                    .get(&(13272 + slot * 28 + field as u32 * 4))
                    .copied()
                    .unwrap_or(if field == 2 { u32::MAX } else { 0 })
            })
        })
        .collect()
}

/// 471F50 destroys these native object owners before rebuilding the saved A.
fn clear_native_objects(vm: &mut CmvsPs2aVmState) {
    let destroyed = |key: &u32| {
        matches!(
            *key >> 8,
            1924 | 2948
                | 742
                | 750
                | 775
                | 3100
                | 3176
                | 3180
                | 3184
                | 3196
                | 3220
                | 3252
                | 3256
                | 3260
                | 3264
                | 3268
        )
    };
    vm.slot_objects.retain(|key, _| !destroyed(key));
    vm.slot_object_seed_words.retain(|key, _| !destroyed(key));
    vm.texture_parents.clear();
    vm.texture_children.clear();
    vm.texture_reloads.clear();
    vm.texture_presentation = None;
    vm.cmv_controllers.clear();
    vm.effect_channels.clear();
    vm.effect_playback.clear();
    vm.effect_elements.clear();
    vm.movie_object = None;
    vm.movie_frame = 0;
    vm.channel_child_presenters.clear();
    vm.native_text.clear();
    vm.native_glyphs.clear();
    vm.native_text_reveals.clear();
    vm.native_timeline = None;
    crate::vm::camera_transition::reset(vm);
    crate::vm::independent_view::reset(vm);
    vm.native_scene_workers.clear();
    for slot in 0..4 {
        vm.write_interpreter_word(2948 + 4 * slot, 0);
    }
    vm.screen_shake = None;
    vm.scene_words.clear();
    vm.filter_chain_records.clear();
    vm.filter_chain_record_order.clear();
    vm.filter_chain_active_records.clear();
    vm.filter_chain_locked_records.clear();
    vm.filter_chain_channels.clear();
    vm.filter_chain_apply_revisions.clear();
    vm.resource_channel_slots.clear();
    vm.interpreter_words
        .retain(|offset, _| !(1608..1920).contains(offset));
    for field in [
        1464, 1500, 10572, 10708, 3176, 3180, 3252, 3256, 3260, 3264, 3268,
    ] {
        vm.write_interpreter_word(field, 0);
    }
    vm.dialog_dispatch_base_depth = None;
    vm.dialog_dispatch_cursor = None;
    vm.forced_dialog_record = None;
    vm.engine_request_park = None;
    vm.engine_request_armed = false;
    vm.dispatch_stopped = false;
    vm.input_selection_mirror = 0;
    vm.load_confirm_selection = None;
    vm.menu_session_live = false;
    vm.menu_pump_exit_seen = false;
    vm.input_advance_press = false;
    vm.input_advance_release = false;
}

impl CmvsSession {
    /// Called by the actual native capture commands, never by opening a menu.
    pub(super) fn capture_native_slot(&self, resume_pc: Option<u32>) -> SessionResult<NativeSlot> {
        let root = self.scripts.get(&0).ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_NATIVE_SLOT_PROGRAM",
                "root program is not loaded",
            )
        })?;
        let program = capture_native_program(root, &self.vm).map_err(core)?;
        let mut interpreter = capture_native_interpreter(&self.vm, &program).map_err(core)?;
        if let Some(resume_pc) = resume_pc {
            interpreter.scalars.insert(263, resume_pc);
            interpreter.scalars.insert(264, 0);
        }
        native_save_audio::capture(self.audio.as_ref(), &self.vm, &mut interpreter.globals)?;
        let mut slot = NativeSlot::new(&self.vm, interpreter, program).map_err(core)?;
        slot.scene_workers = crate::vm::native_scene_worker::capture(&self.vm).map_err(core)?;
        for &parent in self.vm.texture_parents.keys() {
            slot.images.insert(
                u16::from(parent),
                self.scene
                    .capture_native_image(&self.vm, parent, &self.archive)
                    .map_err(core)?,
            );
        }
        for &text in self.vm.native_text.keys() {
            slot.text.insert(
                u16::from(text),
                self.scene
                    .capture_native_text(&self.vm, text)
                    .map_err(core)?,
            );
        }
        for channel in 0_u8..8 {
            if self
                .vm
                .slot_objects
                .contains_key(&(742_u32 << 8 | u32::from(channel)))
            {
                slot.channels.insert(
                    u16::from(channel),
                    self.scene
                        .capture_native_channel(&self.vm, channel, &self.archive)
                        .map_err(core)?,
                );
            }
        }
        for &controller in self.vm.cmv_controllers.keys() {
            slot.cmv.insert(
                u16::from(controller),
                crate::system_save::slot_cmv::SavedCmv::capture(&self.vm, u16::from(controller))
                    .map_err(core)?,
            );
        }
        // These native families have no reconstructed equivalent in this core.
        // Do not silently emit a file which drops a live native object.
        if !self.vm.filter_chain_records.is_empty()
            || !self.vm.scene_words.is_empty()
            || self.vm.slot_objects.keys().any(|key| {
                matches!(*key >> 8, 3196 | 3220)
                    || (*key >> 8 == 2948
                        && !self.vm.native_scene_workers.contains_key(&(*key as u8)))
            })
        {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_SLOT_OBJECT",
                "live optional native object family is unsupported",
            ));
        }
        Ok(slot)
    }

    fn prepare_native_slot(&self, saved: &NativeSlot) -> SessionResult<PreparedNativeSlot> {
        // Native 471F50 never compares the header identity (bytes 8..12):
        // it only checks the file length and the ciphered-body checksum, so
        // a save from another build or update level loads unchanged.
        let mut vm = self.vm.clone();
        clear_native_objects(&mut vm);
        let mut scripts = self.scripts.clone();
        let mut script_names = self.script_names.clone();
        let root = install_native_program(
            &saved.program_image,
            "cmvs:/native-slot/program.ps2a",
            &mut vm,
        )
        .map_err(core)?;
        scripts.insert(0, Arc::new(root));
        script_names.insert(0, owned_native_name(&saved.interpreter.strings[2])?);
        for frame in 1_u16..4 {
            let name = owned_native_name(&saved.interpreter.strings[usize::from(frame) + 2])?;
            if !name.is_empty()
                && (script_names.get(&frame) != Some(&name)
                    || !vm.script_frames.contains_key(&frame))
            {
                let script = self
                    .archive
                    .load_native_script(&name, frame, &mut vm)
                    .map_err(core)?;
                scripts.insert(frame, Arc::new(script));
                script_names.insert(frame, name);
            }
        }
        if scripts
            .values()
            .map(|script| u64::from(script.decoded_size))
            .sum::<u64>()
            > MAX_SCRIPT_RESIDENT
        {
            return Err(error(
                "SHINKI_EMU_CMVS_SCRIPT_BOUND",
                "saved script frames exceed the resident budget",
            ));
        }
        apply_native_interpreter(&mut vm, &saved.interpreter).map_err(core)?;
        for (field, offset) in [(10312, 528), (10320, 536)] {
            vm.write_interpreter_word(
                field,
                u32::from_le_bytes(saved.header[offset..offset + 4].try_into().unwrap()),
            );
        }
        vm.write_interpreter_word(10316, 0);
        let mut scene = self.scene.native_restore_candidate().map_err(core)?;
        for (&parent, image) in &saved.images {
            scene
                .restore_native_image(
                    &mut vm,
                    u8::try_from(parent).map_err(|_| {
                        error(
                            "SHINKI_EMU_CMVS_NATIVE_SLOT_IMAGE",
                            "parent exceeds native table",
                        )
                    })?,
                    image,
                    &self.archive,
                )
                .map_err(core)?;
        }
        // Channels refer to native750 text owners and their virtual extents.
        for (&index, text) in &saved.text {
            scene
                .restore_native_text(
                    &mut vm,
                    u8::try_from(index).map_err(|_| {
                        error(
                            "SHINKI_EMU_CMVS_NATIVE_SLOT_TEXT",
                            "text exceeds native table",
                        )
                    })?,
                    text,
                )
                .map_err(core)?;
        }
        for (&index, channel) in &saved.channels {
            scene
                .restore_native_channel(
                    &mut vm,
                    u8::try_from(index).map_err(|_| {
                        error(
                            "SHINKI_EMU_CMVS_NATIVE_SLOT_CHANNEL",
                            "channel exceeds native table",
                        )
                    })?,
                    channel,
                    &self.archive,
                )
                .map_err(core)?;
        }
        let mut cmv_players = BTreeMap::new();
        for (&index, controller) in &saved.cmv {
            cmv::restore_native_cmv(
                &mut vm,
                &mut scene,
                &self.archive,
                &mut cmv_players,
                index,
                controller,
            )?;
        }
        crate::vm::native_scene_worker::restore(&mut vm, &saved.scene_workers).map_err(core)?;
        let audio = native_save_audio::PreparedNativeAudio::prepare(
            &self.archive,
            &vm,
            &saved.interpreter.globals,
            self.native_sound_format,
        )?;
        let extent = self
            .scene
            .logical_extent()
            .ok_or_else(|| error("SHINKI_EMU_CMVS_CANVAS", "native load has no active stage"))?;
        let selected = vm
            .texture_parents
            .keys()
            .next()
            .copied()
            .map(CmvsTextureSlot::Parent);
        let frame = scene
            .render(
                &mut vm,
                selected.unwrap_or(CmvsTextureSlot::Parent(0)),
                &self.archive,
                (extent.width, extent.height),
                None,
            )
            .map_err(core)?
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_NATIVE_SLOT_FRAME",
                    "restored native scene did not produce a frame",
                )
            })?;
        Ok(PreparedNativeSlot {
            vm,
            scripts,
            script_names,
            scene,
            cmv_players,
            audio,
            frame,
            selected,
        })
    }

    pub(super) fn apply_native_slot(&mut self, mut saved: NativeSlot) -> SessionResult<()> {
        // Prepare at the actual switch, so unsaved live expression/configuration
        // fields and the native record merge use the current menu state.
        let mut prepared = self.prepare_native_slot(&saved)?;
        let prior_records = record_image(&self.vm);
        reconcile_native_records_and_request29(&mut prepared.vm, &prior_records).map_err(core)?;
        if self.movie.is_some() {
            let audio = self.audio.as_mut().ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_AUDIO_UNBOUND",
                    "live movie has no audio owner",
                )
            })?;
            if let Some(mut movie) = self.movie.take() {
                movie.close(audio)?;
            }
        }
        if let Some(audio) = self.audio.as_mut() {
            prepared.audio.commit(audio)?;
        } else if !saved.interpreter.globals.bgm.primary.is_empty()
            || !saved.interpreter.globals.bgm.secondary.is_empty()
            || saved
                .interpreter
                .globals
                .resource_channels
                .slots
                .iter()
                .any(|record| !record.name.is_empty())
            || !saved.interpreter.globals.static_sounds.entries.is_empty()
        {
            return Err(error(
                "SHINKI_EMU_CMVS_AUDIO_UNBOUND",
                "native save requires an audio sink",
            ));
        }
        let records = saved.interpreter.records.len();
        prepared.vm.save_snapshot = if saved.preview_bmp.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut saved.preview_bmp))
        };
        self.vm = prepared.vm;
        self.scripts = prepared.scripts;
        self.script_names = prepared.script_names;
        self.scene = prepared.scene;
        self.cmv_players = prepared.cmv_players;
        self.frame = prepared.frame;
        self.selected = prepared.selected;
        self.last_loaded_parent = self.selected.and_then(|slot| match slot {
            CmvsTextureSlot::Parent(parent) | CmvsTextureSlot::Child { parent, .. } => Some(parent),
            CmvsTextureSlot::SceneRoot(_) | CmvsTextureSlot::SceneChild { .. } => None,
        });
        self.movie = None;
        self.movie_stage = None;
        self.fade_level = 0.0;
        self.input.close_menu();
        self.captured_native_slot = Some(Ok(Box::new(saved)));
        tracing::info!(
            event = "cmvs.session.save_loaded",
            records,
            tick = self.vm.tick
        );
        Ok(())
    }

    pub(super) fn native_slot_bytes(&mut self) -> SessionResult<Vec<u8>> {
        let enabled = self.vm.interpreter_words.get(&10292).copied().unwrap_or(0) as u16;
        let captured = self.captured_native_slot.as_mut().ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_NATIVE_CAPTURE_MISSING",
                "native save command has no captured state",
            )
        })?;
        let slot = match captured {
            Ok(slot) => slot.as_mut(),
            Err(cause) => return Err(cause.clone()),
        };
        slot.header[548..550].copy_from_slice(&enabled.to_le_bytes());
        let preview = if enabled != 0 {
            self.vm.save_snapshot.as_deref().unwrap_or(&[])
        } else {
            &[]
        };
        encode_native_slot_with_preview(slot, preview).map_err(core)
    }
}

#[cfg(test)]
#[path = "native_slot_tests.rs"]
mod tests;
