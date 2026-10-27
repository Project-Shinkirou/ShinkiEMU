use crate::family::{Lease, core, error};
use crate::*;
use roukaku_media_core::TextureFrame;
use shinki_emu_sdk::runtime::*;
use std::{collections::BTreeMap, sync::Arc};
mod audio;
mod cmv;
mod diagnostics;
mod input;
mod movie;
mod native_audio;
mod native_save_audio;
mod native_save_globals;
mod native_slot;
mod native_slot_info;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod test_support;
mod window;
const MAX_INSTRUCTIONS: u32 = 1_048_576;
const MAX_SCRIPT_RESIDENT: u64 = 256 * 1024 * 1024;
/// Interpreter word the native loader thread increments as each async
/// resource completes (dword 2948). `intproc`'s load gate polls it through
/// command 718; headless binds are synchronous, so the count is current
/// immediately.
const LOADER_PROGRESS_FIELD: u32 = 11792;

/// Models one completed async resource load.
fn record_completed_load(vm: &mut CmvsPs2aVmState) {
    let progress = vm
        .interpreter_words
        .entry(LOADER_PROGRESS_FIELD)
        .or_insert(0);
    *progress = progress.wrapping_add(1);
}

pub(crate) struct CmvsSession {
    archive: Arc<CmvsArchive>,
    game_root: std::path::PathBuf,
    vm: CmvsPs2aVmState,
    scripts: BTreeMap<u16, Arc<CmvsScript>>,
    scene: CmvsScene,
    frame: TextureFrame,
    native_render_size: [u32; 2],
    selected: Option<CmvsTextureSlot>,
    /// The live channel animation level (`sub_467A50` dword 18/19) scaled
    /// to 0..1; 0 marks a cleared animation and the scene beneath shows.
    fade_level: f32,
    /// A load whose interpreter-state switch waits for the menu render
    /// pump to exit (the native program switch happens after the fade
    /// completes and the menu unwinds, not at the script's cmd 693).
    pending_load: Option<Box<crate::system_save::slot::NativeSlot>>,
    /// Native commands 128/690 capture A/C before the save menu writes a slot.
    captured_native_slot: Option<SessionResult<Box<crate::system_save::slot::NativeSlot>>>,
    /// The most recently loaded parent slot, presented when the animation
    /// clears the current surface.
    last_loaded_parent: Option<u8>,
    phase: u64,
    suspended: bool,
    input: input::InputState,
    script_names: std::collections::BTreeMap<u16, String>,
    movie: Option<movie::MoviePlayer>,
    cmv_players: BTreeMap<u8, cmv::CmvPlayer>,
    audio: Option<audio::AudioBridge>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    native_sound_format: [u32; 2],
    movie_stage: Option<(u64, TextureFrame)>,
    timer_carry_ns: u64,
    failure: Option<SessionError>,
    pending_window: window::WindowCommands,
    _lease: Lease,
}
impl CmvsSession {
    pub(crate) fn new(
        archive: Arc<CmvsArchive>,
        game_root: std::path::PathBuf,
        entry: &str,
        window: WindowState,
        lease: Lease,
        gpu_instance: &wgpu::Instance,
    ) -> SessionResult<Self> {
        Self::new_with_audio(
            archive,
            game_root,
            entry,
            window,
            lease,
            gpu_instance,
            audio::AudioBridge::new,
        )
    }
    fn new_with_audio(
        archive: Arc<CmvsArchive>,
        game_root: std::path::PathBuf,
        entry: &str,
        window: WindowState,
        lease: Lease,
        gpu_instance: &wgpu::Instance,
        open_audio: impl FnOnce() -> SessionResult<audio::AudioBridge>,
    ) -> SessionResult<Self> {
        let mut vm = CmvsPs2aVmState::new(0);
        // Native seeds the value stack with the interpreter's root frame
        // (`this[3306/3305/3304]` = counter, entry pc, frame) before the
        // first dispatch, so coroutine yields that drain the candidate
        // triples land on a valid return address instead of session junk.
        vm.seed_root_frame();
        let default_font = archive.native_default_font().map_err(core)?;
        archive.initialize_native_caption(&mut vm).map_err(core)?;
        archive
            .initialize_native_script_search(&mut vm)
            .map_err(core)?;
        let script = archive
            .load_native_script(entry, 0, &mut vm)
            .map_err(core)?;
        // Frame installation transfers directly to its entry. It must not
        // queue a second execution of the same initialization block.
        vm.program_counter = script.header.initial_pc;
        let mut scene = CmvsScene::new(gpu_instance).map_err(core)?;
        scene.set_default_font(default_font);
        scene
            .resize_raster(window.width, window.height)
            .map_err(core)?;
        let ([logical_width, logical_height], native_render_size) =
            archive.native_display_sizes().map_err(core)?;
        let frame = scene.clear(logical_width, logical_height).map_err(core)?;
        vm.native_stage_size = [logical_width, logical_height];
        vm.native_render_size = native_render_size;
        vm.native_view.fullscreen = !archive.native_windowed().map_err(core)?;
        archive
            .initialize_native_view_policy(&mut vm)
            .map_err(core)?;
        vm.native_view.reset_viewport(native_render_size);
        let (native_sound_format, [voice_enabled, se_enabled]) =
            archive.native_audio_configuration().map_err(core)?;
        vm.write_interpreter_word(1556, se_enabled);
        vm.write_interpreter_word(1552, voice_enabled);
        let mut pending_window = window::WindowCommands::default();
        pending_window.submit(WindowCommand::SetFullscreen(vm.native_view.fullscreen))?;
        let audio = open_audio()?;
        let cancelled = audio.cancellation_flag();
        tracing::info!(
            event = "shinki.emu.cmvs.session.open",
            fullscreen = vm.native_view.fullscreen
        );
        Ok(Self {
            archive,
            game_root,
            vm,
            scripts: BTreeMap::from([(0, Arc::new(script))]),
            scene,
            frame,
            native_render_size,
            selected: None,
            fade_level: 1.0,
            pending_load: None,
            captured_native_slot: None,
            last_loaded_parent: None,
            phase: 0,
            suspended: false,
            input: input::InputState::default(),
            script_names: std::collections::BTreeMap::new(),
            audio: Some(audio),
            cancelled,
            native_sound_format,
            movie: None,
            cmv_players: BTreeMap::new(),
            movie_stage: None,
            timer_carry_ns: 0,
            failure: None,
            pending_window,
            _lease: lease,
        })
    }
    pub(crate) fn frame_info(&self) -> SessionResult<FrameInfo> {
        let logical = self
            .scene
            .logical_extent()
            .ok_or_else(|| error("SHINKI_EMU_CMVS_CANVAS", "native stage is not initialized"))?;
        let info = FrameInfo {
            width: self.frame.width,
            height: self.frame.height,
            logical_width: logical.width,
            logical_height: logical.height,
            stride: self
                .frame
                .width
                .checked_mul(4)
                .ok_or_else(|| error("SHINKI_EMU_CMVS_FRAME", "frame stride overflow"))?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Ignored,
            },
            rows: FrameRowOrder::TopToBottom,
        };
        info.validate()?;
        Ok(info)
    }
    fn load(&mut self, frame: u16, name: String) -> SessionResult<()> {
        tracing::info!(event = "cmvs.session.script_load", frame, name = %name);
        let script = self
            .archive
            .load_native_script(&name, frame, &mut self.vm)
            .map_err(core)?;
        let resident = self
            .scripts
            .iter()
            .filter(|(slot, _)| **slot != frame)
            .map(|(_, script)| u64::from(script.decoded_size))
            .sum::<u64>()
            + u64::from(script.decoded_size);
        if resident > MAX_SCRIPT_RESIDENT {
            return Err(error(
                "SHINKI_EMU_CMVS_SCRIPT_BOUND",
                "resident script byte limit exceeded",
            ));
        }
        self.script_names.insert(frame, name);
        self.scripts.insert(frame, Arc::new(script));
        self.vm.current_frame = frame;
        Ok(())
    }
    fn advance_native_scene_workers(&mut self, delta_ms: u32) -> SessionResult<()> {
        use crate::vm::native_scene_frame::NativeSceneSound;
        let sounds = crate::vm::native_scene_worker::tick(&mut self.vm, delta_ms).map_err(core)?;
        crate::vm::native_scene_worker::prepare_draw(&mut self.vm, delta_ms).map_err(core)?;
        if let Some(audio) = &mut self.audio {
            for sound in sounds {
                match sound {
                    NativeSceneSound::PlayStatic { slot, volume } => {
                        audio.play_static_sound(slot, volume, None)?
                    }
                    NativeSceneSound::PlayStaticMain {
                        slot,
                        volume,
                        looping,
                    } => audio.play_static_sound(slot, volume, Some(looping))?,
                    NativeSceneSound::StopStatic { slot } => audio.stop_static_sound(slot)?,
                    NativeSceneSound::StopStream { slot } => audio.stop_channel(slot)?,
                }
            }
        }
        Ok(())
    }
    fn retire_native_scene_audio(&mut self, static_sounds: u32, streams: u16) -> SessionResult<()> {
        if let Some(audio) = &mut self.audio {
            for slot in 0..24 {
                if static_sounds & (1 << slot) != 0 {
                    audio.clear_static_sound(slot)?;
                }
            }
            for slot in 0..16 {
                if streams & (1 << slot) != 0 {
                    audio.stop_channel(slot)?;
                }
            }
        }
        Ok(())
    }
    fn action(&mut self, action: CmvsPs2aVmAction) -> SessionResult<()> {
        match action {
            CmvsPs2aVmAction::CreateNativeScene53 {
                slot,
                retired_static_sounds,
                stopped_streams,
            } => {
                self.action(CmvsPs2aVmAction::NativeSceneWorkerResourceChanged {
                    slot,
                    retired_static_sounds,
                    stopped_streams,
                })?;
                let owner = self
                    .scene
                    .prepare_native_scene53(&self.vm, &self.archive, slot)
                    .map_err(core)?;
                crate::vm::native_scene_worker::install_scene53(&mut self.vm, slot, owner)
                    .map_err(core)
            }
            CmvsPs2aVmAction::NativeSceneWorkerResourceChanged {
                slot,
                retired_static_sounds,
                stopped_streams,
            } => {
                self.retire_native_scene_audio(retired_static_sounds, stopped_streams)?;
                self.scene.invalidate_native_meshes(slot);
                self.scene.invalidate_plane_particles(slot);
                self.scene
                    .release_owner_surface(CmvsTextureSlot::SceneRoot(slot));
                if self
                    .selected
                    .is_some_and(|selected| selected.scene_worker() == Some(slot))
                {
                    self.selected = None;
                }
                if let (Some(owner), Some(audio)) =
                    (self.vm.native_scene_workers.get(&slot), &mut self.audio)
                {
                    for &(sound_slot, name) in owner.sound_resources() {
                        native_save_audio::configure_static(
                            &self.archive,
                            &self.vm,
                            audio,
                            sound_slot,
                            name,
                        )?;
                    }
                }
                Ok(())
            }
            CmvsPs2aVmAction::TransferTextureParentToSceneWorker {
                parent_slot,
                slot,
                retired_static_sounds,
                stopped_streams,
            } => {
                self.retire_native_scene_audio(retired_static_sounds, stopped_streams)?;
                self.scene.invalidate_native_meshes(slot);
                self.scene.invalidate_plane_particles(slot);
                let destination = CmvsTextureSlot::SceneRoot(slot);
                self.scene.move_parent_surface(parent_slot, destination);
                self.selected = self
                    .selected
                    .map(|selected| selected.relocate(parent_slot, destination));
                if self.last_loaded_parent == Some(parent_slot) {
                    self.last_loaded_parent = None;
                }
                Ok(())
            }
            CmvsPs2aVmAction::SetWindowIcon { segments } => {
                match self.archive.load_window_icon(&segments).map_err(core)? {
                    Some(icon) => {
                        let (width, height) = icon.dimensions();
                        self.pending_window
                            .submit(WindowCommand::SetIcon(WindowIcon {
                                width,
                                height,
                                rgba: icon.into_raw(),
                            }))?;
                    }
                    None => tracing::debug!(event = "cmvs.window.icon_unavailable"),
                }
                Ok(())
            }
            CmvsPs2aVmAction::SetWindowCaption { segments } => {
                let [CmvsStringSegment::OwnedString(title)] = segments.as_slice() else {
                    return Err(error(
                        "SHINKI_EMU_CMVS_WINDOW_CAPTION",
                        "window caption prefix is not bound",
                    ));
                };
                let command = WindowCommand::SetTitle(title.clone());
                self.pending_window.submit(command)
            }
            CmvsPs2aVmAction::SetNativeFullscreen { enabled } => self
                .pending_window
                .submit(WindowCommand::SetFullscreen(enabled)),
            CmvsPs2aVmAction::ScanNativeSaveSlots { count } => self
                .archive
                .refresh_native_save_catalog(&mut self.vm, count)
                .map_err(core),

            CmvsPs2aVmAction::CreateEffectChannel { effect, .. } => self
                .scene
                .bind_text_owner(&self.vm, effect as u8)
                .map_err(core),
            CmvsPs2aVmAction::NativeText(action) => {
                match action {
                    CmvsNativeTextAction::GlyphCreate { slot }
                    | CmvsNativeTextAction::GlyphFont { slot } => {
                        self.scene.bind_glyph_owner(&self.vm, slot).map_err(core)?;
                    }
                    CmvsNativeTextAction::GlyphAppend { slot, source } => {
                        self.scene
                            .append_glyph(&mut self.vm, slot, &source)
                            .map_err(core)?;
                    }
                    CmvsNativeTextAction::GlyphLoadImage { slot, resource } => {
                        self.scene
                            .load_glyph_image(&self.archive, &mut self.vm, slot, &resource)
                            .map_err(core)?;
                    }
                    CmvsNativeTextAction::Create { slot }
                    | CmvsNativeTextAction::SetFont { slot } => {
                        self.scene.bind_text_owner(&self.vm, slot).map_err(core)?;
                    }
                    CmvsNativeTextAction::SetDefaultFont { font } => {
                        self.scene.set_default_font(font)
                    }
                    CmvsNativeTextAction::Measure {
                        slot,
                        source,
                        proportional,
                    } => {
                        self.scene
                            .measure_text(&mut self.vm, slot, &source, proportional)
                            .map_err(core)?;
                        tracing::debug!(
                            event = "cmvs.text.measured",
                            slot,
                            proportional,
                            tick = self.vm.tick,
                            continuation_pc = self.vm.program_counter,
                            max_width = self.vm.interpreter_words.get(&81224).copied(),
                            lines = self.vm.interpreter_words.get(&81228).copied(),
                            last_width = self.vm.interpreter_words.get(&81236).copied()
                        );
                    }
                    CmvsNativeTextAction::DrawDialogue {
                        channel,
                        slot,
                        source,
                    } => {
                        self.scene
                            .append_dialogue(&mut self.vm, channel, slot, &source)
                            .map_err(core)?;
                        let surface = &self.vm.native_text[&slot];
                        tracing::info!(event = "cmvs.text.dialogue", slot, channel,
                            continuation_pc = self.vm.program_counter, tick = self.vm.tick,
                            source_bytes = source.len(), glyph_count = surface.glyphs.len(),
                            position = ?surface.position, rect = ?surface.rect, virtual_size = ?surface.virtual_size,
                            first_glyph = ?surface.glyphs.first().map(|glyph| glyph.position),
                            metrics = ?(surface.size, surface.character_spacing, surface.line_spacing),
                            first_glyph_size = ?surface.glyphs.first().map(|glyph| glyph.size),
                            canvas = ?self.scene.logical_extent());
                    }
                    CmvsNativeTextAction::DrawStatic { slot, source } => {
                        self.scene
                            .append_text(&mut self.vm, slot, &source)
                            .map_err(core)?;
                        tracing::debug!(
                            event = "cmvs.text.static",
                            slot,
                            glyph_count = self.vm.native_text[&slot].glyphs.len()
                        );
                    }
                    CmvsNativeTextAction::Destroy { .. } | CmvsNativeTextAction::Clear { .. } => {}
                }
                Ok(())
            }
            CmvsPs2aVmAction::BuildNativeSaveSnapshot { resume_pc } => {
                self.captured_native_slot =
                    Some(self.capture_native_slot(Some(resume_pc)).map(Box::new));
                Ok(())
            }
            CmvsPs2aVmAction::FlushNativeTimeline => self.advance_native_timeline(true, true),
            CmvsPs2aVmAction::ApplyNativeAudioGainSetting { command_id } => {
                let bridge = self.audio.as_mut().ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_AUDIO_UNBOUND",
                        "native audio setting has no PCM sink",
                    )
                })?;
                native_save_audio::apply_gain_setting(&self.vm, bridge, command_id)
            }
            CmvsPs2aVmAction::NativeAudio(_) | CmvsPs2aVmAction::ConfigureResourceAudio { .. } => {
                let bridge = self.audio.as_mut().ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_AUDIO_UNBOUND",
                        "native audio has no PCM sink",
                    )
                })?;
                native_audio::execute(
                    &self.archive,
                    &mut self.vm,
                    bridge,
                    self.native_sound_format,
                    action,
                )
            }
            CmvsPs2aVmAction::CallScript { frame, name } => self.load(frame, name),
            CmvsPs2aVmAction::ReloadRootScript { name } => {
                if self.vm.texture_parents.is_empty() {
                    self.scene.clear_parent_surfaces();
                    self.selected = None;
                    self.last_loaded_parent = None;
                }
                self.load(0, name)?;
                self.captured_native_slot = Some(self.capture_native_slot(None).map(Box::new));
                Ok(())
            }
            CmvsPs2aVmAction::EnsurePathDirectories { buffer_offset } => self
                .archive
                .ensure_path_directories(&self.vm, buffer_offset)
                .map_err(core),
            CmvsPs2aVmAction::LoadCustomGlyphResource {
                resource,
                cell_size,
            } => self
                .archive
                .load_native_custom_glyph_sheet(&mut self.vm, &resource, cell_size)
                .map(|_| ())
                .map_err(core),
            CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            } => {
                // Native483D70 reports missing resources through the result
                // word and continues without binding a substitute image.
                self.scene.clear_parent_image(parent_slot);
                let uri = match self.archive.resolve_native_resource(
                    &self.vm,
                    CmvsResourceSearchBank::Texture,
                    &resource,
                ) {
                    Ok(uri) => uri,
                    Err(cause) => {
                        tracing::info!(
                            event = "cmvs.session.texture_resolve_failed",
                            slot = parent_slot,
                            len = resource.len(),
                            code = cause.code()
                        );
                        self.vm.write_interpreter_word(81220, 0);
                        return Ok(());
                    }
                };
                tracing::debug!(
                    event = "cmvs.session.texture_parent_load",
                    slot = parent_slot,
                    name_bytes = resource.len()
                );
                self.scene
                    .bind_parent_image(&mut self.vm, parent_slot, &uri, &self.archive)
                    .map_err(core)?;
                if let Some(reload) = self.vm.texture_reloads.get_mut(&parent_slot) {
                    reload.current_uri = Some(uri);
                }
                record_completed_load(&mut self.vm);
                self.last_loaded_parent = Some(parent_slot);
                self.vm.write_interpreter_word(81220, 1);
                Ok(())
            }
            CmvsPs2aVmAction::ReloadTextureParentImage {
                parent_slot,
                resource,
                bank,
                result_field_offset,
            } => {
                let succeeded = self
                    .scene
                    .load_image_reload(
                        &mut self.vm,
                        parent_slot,
                        resource,
                        bank,
                        result_field_offset,
                        &self.archive,
                    )
                    .map_err(core)?;
                if succeeded {
                    record_completed_load(&mut self.vm);
                    self.last_loaded_parent = Some(parent_slot);
                }
                Ok(())
            }
            CmvsPs2aVmAction::ReleaseTextureReload { parent_slot } => {
                self.scene.release_image_reload(parent_slot);
                Ok(())
            }
            CmvsPs2aVmAction::MoveTextureParentSurface {
                source,
                destination,
            } => {
                let target = CmvsTextureSlot::Parent(destination);
                self.scene.move_parent_surface(source, target);
                self.selected = self.selected.map(|slot| slot.relocate(source, target));
                if self.last_loaded_parent == Some(source) {
                    self.last_loaded_parent = Some(destination);
                }
                Ok(())
            }
            CmvsPs2aVmAction::LoadTextureResource {
                parent_slot,
                child_id,
                resource,
            } => {
                let uri = self
                    .archive
                    .resolve_native_resource(&self.vm, CmvsResourceSearchBank::Texture, &resource)
                    .map_err(core)?;
                tracing::info!(
                    event = "cmvs.session.texture_child_bound",
                    parent = parent_slot,
                    child = child_id,
                    resource_bytes = resource.len(),
                    tick = self.vm.tick
                );
                self.scene
                    .bind_child_image(&mut self.vm, parent_slot, child_id, &uri, &self.archive)
                    .map_err(core)?;
                record_completed_load(&mut self.vm);
                Ok(())
            }
            CmvsPs2aVmAction::PrepareTexturePresentation { clear_background } => {
                let presentation = self.vm.texture_presentation.as_ref().ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_PRESENTATION_OWNER",
                        "mask preparation has no presentation owner",
                    )
                })?;
                let parent = presentation.parent;
                let uri = self
                    .archive
                    .resolve_native_resource(
                        &self.vm,
                        CmvsResourceSearchBank::Texture,
                        &presentation.mask,
                    )
                    .map_err(core)?;
                self.scene
                    .prepare_image(&uri, &self.archive)
                    .map_err(core)?;
                if clear_background {
                    self.scene.bind_clear_surface(&self.archive).map_err(core)?;
                }
                let presentation = self.vm.texture_presentation.as_mut().unwrap();
                presentation.mask = uri;
                tracing::info!(
                    event = "cmvs.session.presentation_started",
                    parent,
                    duration_ms = presentation.duration_ms,
                    showing = presentation.showing,
                    tick = self.vm.tick
                );
                self.selected = Some(CmvsTextureSlot::Parent(parent));
                Ok(())
            }
            CmvsPs2aVmAction::ClearTextureParentSurfaces => {
                self.scene.clear_parent_surfaces();
                self.selected = None;
                self.last_loaded_parent = None;
                Ok(())
            }
            CmvsPs2aVmAction::OpenCmvResource {
                controller_slot,
                parent_slot,
                resource,
                start_frame,
            } => self.open_cmv_resource(controller_slot, parent_slot, resource, start_frame),
            CmvsPs2aVmAction::PresentCmvFrame {
                controller_slot,
                frame,
            } => self.present_cmv_frame(controller_slot, frame),
            CmvsPs2aVmAction::ReleaseCmvResource { controller_slot } => {
                self.cmv_players.remove(&controller_slot);
                Ok(())
            }
            CmvsPs2aVmAction::RemoveTextureChildSurface {
                parent_slot,
                child_id,
            } => {
                self.scene.release_child_surface(parent_slot, child_id);
                if self.selected
                    == Some(CmvsTextureSlot::Child {
                        parent: parent_slot,
                        child: child_id,
                    })
                {
                    self.selected = None;
                }
                Ok(())
            }
            CmvsPs2aVmAction::RemoveTextureParentSurface { parent_slot } => {
                self.scene.release_parent_surface(parent_slot);
                if self.selected.is_some_and(|slot| match slot {
                    CmvsTextureSlot::Parent(parent) | CmvsTextureSlot::Child { parent, .. } => {
                        parent == parent_slot
                    }
                    CmvsTextureSlot::SceneRoot(_) | CmvsTextureSlot::SceneChild { .. } => false,
                }) {
                    self.selected = None;
                }
                if self.last_loaded_parent == Some(parent_slot) {
                    self.last_loaded_parent = None;
                }
                Ok(())
            }
            CmvsPs2aVmAction::CommitTextureSurface {
                parent_slot,
                child_id,
            } => {
                tracing::debug!(
                    event = "cmvs.session.texture_surface_commit",
                    parent = parent_slot,
                    child = ?child_id
                );
                self.selected = Some(child_id.map_or(
                    CmvsTextureSlot::Parent(parent_slot),
                    |child| CmvsTextureSlot::Child {
                        parent: parent_slot,
                        child,
                    },
                ));
                Ok(())
            }
            CmvsPs2aVmAction::OpenScenarioStream { resource } => {
                // `sub_479060` opens the named scenario stream through the
                // configured script search domain; the load completes
                // synchronously before the next frame, so the loader
                // progress word counts it immediately.
                let uri = self
                    .archive
                    .resolve_native_resource(&self.vm, CmvsResourceSearchBank::Script, &resource)
                    .map_err(core)?;
                tracing::info!(event = "cmvs.session.stream_opened", uri = %uri);
                record_completed_load(&mut self.vm);
                Ok(())
            }
            CmvsPs2aVmAction::OpenMovieStream {
                resource,
                result_field_offset,
            } => {
                // `sub_412800` resolves the concatenated path against the
                // movie search domain; the open result lands in the named
                // interpreter word and the VM keeps object occupancy.
                let banks = [
                    CmvsResourceSearchBank::Script,
                    CmvsResourceSearchBank::CrossfadeAudio,
                    CmvsResourceSearchBank::Native0d0c,
                    CmvsResourceSearchBank::Native0d14,
                    CmvsResourceSearchBank::Native0d00,
                    CmvsResourceSearchBank::Native0d04,
                    CmvsResourceSearchBank::Native0d18,
                ];
                let mut uri = None;
                for bank in banks {
                    if let Ok(found) = self
                        .archive
                        .resolve_native_resource(&self.vm, bank, &resource)
                    {
                        uri = Some(found);
                        break;
                    }
                }
                if uri.is_none() {
                    tracing::info!(event = "cmvs.session.movie_unresolved", name = %resource);
                }
                let uri = match uri {
                    Some(uri) => Some(uri),
                    // `sub_4891B0` concatenates onto the movie base path;
                    // the game's movie directory is the observed loose
                    // `data/video` tree, with the same leaf names mirrored
                    // inside the mounted video archive.
                    None => self
                        .archive
                        .stat(&format!("cmvs:/native-files/data/video/{resource}"))
                        .ok()
                        .map(|_| format!("cmvs:/native-files/data/video/{resource}")),
                };
                let result = match uri {
                    Some(uri) => {
                        let size = self.archive.stat(&uri).map_err(core)?.size;
                        // One movie exceeds the single-range VFS bound
                        // (eda.mgv is 175 MB); assemble it in bounded
                        // chunks like the native streaming reader.
                        let chunk_bytes = 32_u64 * 1024 * 1024;
                        let mut bytes = Vec::with_capacity(size as usize);
                        let mut offset = 0_u64;
                        while offset < size {
                            let len = (size - offset).min(chunk_bytes);
                            let part = self.archive.read_range(&uri, offset, len).map_err(core)?;
                            bytes.extend_from_slice(part.bytes.as_slice());
                            offset += len;
                        }
                        let player = match &mut self.audio {
                            Some(bridge) => movie::MoviePlayer::open(
                                bytes.as_slice(),
                                bridge,
                                native_save_audio::channel_gain(
                                    &self.vm,
                                    movie::MOVIE_AUDIO_CHANNEL,
                                ),
                                &self.cancelled,
                            ),
                            None => Err(error(
                                "SHINKI_EMU_CMVS_MEDIA_UNBOUND",
                                "native movie audio is not connected",
                            )),
                        };
                        match player {
                            Ok(player) => {
                                tracing::info!(
                                    event = "cmvs.session.movie_opened",
                                    uri = %uri,
                                    frames = player.index_count,
                                    video = player.has_video_stream(),
                                    tick = self.vm.tick
                                );
                                self.vm.movie_object = Some(uri);
                                self.vm.movie_frame = 0;
                                self.movie = Some(player);
                                1
                            }
                            Err(failure) => {
                                tracing::warn!(event = "cmvs.session.movie_open_failed", uri = %uri, error = %failure);
                                0
                            }
                        }
                    }
                    None => 0,
                };
                self.vm.write_interpreter_word(result_field_offset, result);
                Ok(())
            }
            CmvsPs2aVmAction::PlayCrossfadeAudio {
                primary,
                secondary,
                fade_ms,
                playback_flag,
            } => {
                if self.audio.is_none() {
                    return Err(error(
                        "SHINKI_EMU_CMVS_MEDIA_UNBOUND",
                        "native crossfade audio is not connected",
                    ));
                }
                let primary = self.decode_crossfade_clip(&primary)?;
                let secondary = secondary
                    .map(|reference| self.decode_crossfade_clip(&reference))
                    .transpose()?;
                self.audio
                    .as_mut()
                    .expect("audio bridge checked above")
                    .play_crossfade(
                        primary,
                        secondary,
                        fade_ms,
                        playback_flag,
                        native_save_audio::bgm_gain(&self.vm),
                    )
            }
            CmvsPs2aVmAction::FadeOutAudio { fade_ms } => self
                .audio
                .as_mut()
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_MEDIA_UNBOUND",
                        "native crossfade audio is not connected",
                    )
                })?
                .fade_out(fade_ms),
            CmvsPs2aVmAction::StopAudio => self
                .audio
                .as_mut()
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_MEDIA_UNBOUND",
                        "native crossfade audio is not connected",
                    )
                })?
                .stop(),
            CmvsPs2aVmAction::StorageRequest(CmvsPs2aStorageRequest::LoadSystemState {
                path_buffer_offset,
            }) => self
                .archive
                .load_native_system_state(&mut self.vm, path_buffer_offset)
                .map_err(core),
            CmvsPs2aVmAction::LoadChannelImage {
                channel,
                resource,
                load_error_mask,
            } => {
                let object = self.vm.effect_channels.get_mut(&channel).ok_or_else(|| {
                    error(
                        "SHINKI_EMU_CMVS_CHANNEL_IMAGE",
                        "channel image has no live surface",
                    )
                })?;
                object.image_resource = None;
                object.image_has_alpha = false;
                let uri = match self.archive.resolve_native_resource(
                    &self.vm,
                    CmvsResourceSearchBank::Texture,
                    &resource,
                ) {
                    Ok(uri) => uri,
                    Err(cause) if cause.code() == "SHINKI_EMU_CMVS_RESOURCE_NOT_FOUND" => {
                        *self.vm.interpreter_flag_words.entry(10532).or_default() |=
                            load_error_mask;
                        tracing::info!(
                            event = "cmvs.session.channel_image_missing",
                            channel,
                            tick = self.vm.tick
                        );
                        return Ok(());
                    }
                    Err(cause) => return Err(core(cause)),
                };
                let image_info = self.archive.native_image_info(&uri).map_err(core)?;
                self.scene
                    .prepare_image(&uri, &self.archive)
                    .map_err(core)?;
                let object = self.vm.effect_channels.get_mut(&channel).unwrap();
                object.image_has_alpha = image_info[2] != 0;
                object.image_resource = Some(uri);
                record_completed_load(&mut self.vm);
                tracing::info!(
                    event = "cmvs.session.channel_image_bound",
                    channel,
                    tick = self.vm.tick
                );
                Ok(())
            }
            CmvsPs2aVmAction::CaptureSaveSnapshot => {
                // `sub_471030`: box-filter downscale of the presented frame
                // into a bottom-up 24-bpp BMP at the case-688 dimensions.
                // A zero dimension keeps the previous snapshot (the native
                // readback-failure path also returns 0).
                let width = self.vm.interpreter_words.get(&10296).copied().unwrap_or(0);
                let height = self.vm.interpreter_words.get(&10300).copied().unwrap_or(0);
                if width > 0 && height > 0 && width <= 4096 && height <= 4096 {
                    self.vm.save_snapshot = Some(build_save_snapshot_bmp(
                        self.frame.rgba8.as_ref(),
                        self.frame.width,
                        self.frame.height,
                        width,
                        height,
                    ));
                }
                Ok(())
            }
            CmvsPs2aVmAction::ProbeNativeSaveSlot {
                slot,
                result_field_offset,
            } => {
                let result = u32::from(native_slot_info::probe(&self.game_root, slot));
                self.vm.write_interpreter_word(result_field_offset, result);
                Ok(())
            }
            CmvsPs2aVmAction::SetNativeSaveTitle {
                slot,
                title,
                result_field_offset,
            } => {
                let result = native_slot_info::set_title(&self.game_root, slot, &title);
                self.vm
                    .write_interpreter_word(result_field_offset, u32::from(result));
                Ok(())
            }
            CmvsPs2aVmAction::SetNativeSaveMeta {
                slot,
                metadata,
                result_field_offset,
            } => {
                let result = native_slot_info::set_metadata(&self.game_root, slot, metadata);
                self.vm
                    .write_interpreter_word(result_field_offset, u32::from(result));
                Ok(())
            }
            CmvsPs2aVmAction::LoadNativeSaveInfo {
                slot,
                parent,
                child,
                title_target,
                secondary_target,
                result_field_offset,
            } => {
                let result = self.load_native_slot_info(
                    slot,
                    parent,
                    child,
                    title_target,
                    secondary_target,
                )?;
                self.vm
                    .write_interpreter_word(result_field_offset, u32::from(result));
                Ok(())
            }
            CmvsPs2aVmAction::LoadNativeSaveSlot { slot } => {
                // `Prog_loadAndSwitch` (`sub_485A70`): the slot's TLV is
                // unpacked and the interpreter state (suspend triple,
                // value stack, frame bases, globals) restored before the
                // program counter jumps to the saved continuation. A
                // missing or corrupt file leaves the running program
                // untouched with the native failure result.
                let path = self
                    .game_root
                    .join("save")
                    .join(crate::system_save::slot::slot_file_name(slot));
                let loaded = std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| crate::system_save::slot::decode_native_slot(&bytes).ok());
                match loaded {
                    Some(state) => {
                        // The script's cmd 693 only asks: publish the
                        // success result for the menu poll's gate and hold
                        // the snapshot; the interpreter switch itself is
                        // deferred to the menu render pump's exit (the
                        // `tick` hook below) so the menu's teardown and
                        // fade complete first, exactly like the native
                        // program switch.
                        self.pending_load = Some(Box::new(state));
                        self.vm.write_interpreter_word(81220, 1);
                    }
                    None => {
                        self.vm.write_interpreter_word(81220, 0);
                    }
                }
                Ok(())
            }

            CmvsPs2aVmAction::ForwardEffectChannelWord { channel, value } => {
                // `sub_467A50` → `sub_42EA20` stores the forwarded word at
                // dwords 18/19 of the channel's element container; the
                // game drives per-frame fade and animation levels through
                // it, so the word image must observe the live value.
                if let Ok(channel) = u8::try_from(channel) {
                    self.vm
                        .effect_channels
                        .entry(channel)
                        .or_default()
                        .write_dword(18, value);
                }
                self.fade_level = value.min(255) as f32 / 255.0;
                Ok(())
            }
            CmvsPs2aVmAction::SaveSystemState => {
                // `sub_427E80` serializes the whole system state into the
                // ciphered container; failures leave the previous file in
                // place like the native write-through.
                let payload = crate::system_save::build_system_payload(&self.vm);
                let words = &self.vm.interpreter_words;
                let identity = words.get(&1600).copied().unwrap_or(0);
                let stamp = words.get(&10308).copied().unwrap_or(0);
                let integrity = words.get(&10532).copied().unwrap_or(0);
                let lists = [
                    self.vm.native_system_lists[0].len(),
                    self.vm.native_system_lists[1].len(),
                ];
                let bytes =
                    crate::system_save::encode_system_container(&payload, identity, stamp, &lists);
                let dir = self.game_root.join("save");
                std::fs::create_dir_all(&dir).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("save directory unavailable: {cause}"),
                    )
                })?;
                let path = dir.join("system.dat");
                // `sub_427E80` copies the previous file to `system.bak` only
                // while the interpreter carries no load-integrity flags, so
                // a rejected file never replaces the last good backup. The
                // native `CopyFileA` result is unchecked.
                if integrity == 0 && path.is_file() {
                    let _ = std::fs::copy(&path, dir.join("system.bak"));
                }
                std::fs::write(&path, &bytes).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("system state write failed: {cause}"),
                    )
                })?;
                Ok(())
            }
            CmvsPs2aVmAction::SaveNativeGameSlot { slot } => {
                // Write the actual native command capture, not the currently
                // displayed menu's execution or provider state.
                let bytes = self.native_slot_bytes()?;
                let dir = self.game_root.join("save");
                std::fs::create_dir_all(&dir).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("save directory unavailable: {cause}"),
                    )
                })?;
                let path = dir.join(crate::system_save::slot::slot_file_name(slot));
                std::fs::write(&path, &bytes).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("save slot write failed: {cause}"),
                    )
                })?;
                // The settings field 88 (`sub_45C360`'s reader behind the
                // case-864 copy and the case-866 `top==1 && field>=0`
                // predicate) is the most-recent-save slot: the reset
                // helper parks it at -1 and the save transaction records
                // the written slot, which the load-menu poll reads as
                // "a save exists".
                self.vm.settings_words.insert(88, slot);
                tracing::info!(
                    event = "cmvs.session.save_written",
                    slot,
                    bytes = bytes.len()
                );
                crate::vm::native_slot::request_native_record(&mut self.vm, 30).map_err(core)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Resolves one crossfade BGM reference through the native audio search
    /// domain and decodes it once for the audio worker.
    fn decode_crossfade_clip(
        &self,
        reference: &CmvsPs2aPrivateStringReference,
    ) -> SessionResult<audio::DecodedClip> {
        let word = reference.relative_offset;
        let name = crate::vm::resolve_resource_name(&self.vm, word).map_err(core)?;
        let uri = self
            .archive
            .resolve_native_resource(&self.vm, CmvsResourceSearchBank::CrossfadeAudio, &name)
            .or_else(|_| {
                self.archive.resolve_native_resource(
                    &self.vm,
                    CmvsResourceSearchBank::Script,
                    &name,
                )
            })
            .map_err(core)?;
        let data = self
            .archive
            .load_native_audio(&uri, &name, audio::MAX_CLIP_FRAMES, &self.cancelled)
            .map_err(core)?;
        Ok(audio::DecodedClip::from_kira(data.data.map_err(core)?)
            .with_source(name)
            .with_native_channels(data.channels)
            .with_native_repeat(data.repeat_data)
            .with_native_restart_rewinds(data.restart_rewinds)
            .with_native_format(self.native_sound_format[0], self.native_sound_format[1]))
    }

    fn tick(&mut self) -> SessionResult<()> {
        // The deferred load's switch point: the menu render pump
        // (intproc 342684 loop) unwinds through its 349086 exit once the
        // teardown finishes; that is the native program switch moment.
        // The native program switch lands when the load transition's
        // fade-out completes (the outgoing program's last act); the menu
        // pump parks at 349080 through the fade and its resumes stop
        // right after, so the fade reaching full black with a pending
        // load is the switch moment.
        if self.pending_load.is_some() && self.fade_level >= 1.0 {
            tracing::info!(
                event = "cmvs.session.deferred_load_applied",
                tick = self.vm.tick
            );
            let state = self.pending_load.take().unwrap();
            self.apply_native_slot(*state)?;
        }
        if self.vm.menu_pump_exit_seen {
            self.vm.menu_pump_exit_seen = false;
        }
        // The story's steady state rides the fade floor (~0.05) because
        // the channel driver never fully opens; the native present runs
        // at zero. Snap the residue when no transition is pending so the
        // present fallback (last-loaded parent) can run every frame.
        if !self.input.menu_opened
            && self.pending_load.is_none()
            && self.fade_level > 0.0
            && self.fade_level <= 0.06
        {
            self.fade_level = 0.0;
        }
        // The movie object mirrors VM occupancy: command 197 clears the VM
        // side, the session player stops its slot-2 audio here. While the
        // movie runs, `player->this[8]` (the audio-clock frame counter)
        // feeds command 198.
        let mut movie_released = self.movie.is_some() && self.vm.movie_object.is_none();
        if let (Some(player), Some(bridge), true) = (
            self.movie.as_mut(),
            self.audio.as_ref(),
            self.vm.movie_object.is_some(),
        ) {
            let target = player.current_frame(bridge);
            self.vm.movie_frame = target;
            if target >= player.index_count {
                // `sub_41B4C0` destroys the player when `sub_412700`
                // reaches the MGV index boundary. The script then sees
                // -1 from cmd 198 even before its own cmd 197.
                self.vm.movie_object = None;
                movie_released = true;
                tracing::info!(
                    event = "cmvs.session.movie_completed",
                    target,
                    tick = self.vm.tick
                );
            }
            if let Some((sequence, frame)) = player.pump(target)
                && self
                    .movie_stage
                    .as_ref()
                    .is_none_or(|(staged_sequence, _)| *staged_sequence != sequence)
            {
                self.movie_stage = Some((
                    sequence,
                    TextureFrame {
                        width: frame.width,
                        height: frame.height,
                        rgba8: frame.rgba8.clone(),
                    },
                ));
                tracing::info!(
                    event = "cmvs.session.movie_frame_staged",
                    target,
                    sequence,
                    width = frame.width,
                    height = frame.height,
                    tick = self.vm.tick
                );
            }
        }
        if movie_released {
            if let (Some(mut player), Some(bridge)) = (self.movie.take(), self.audio.as_mut()) {
                let _ = player.close(bridge);
            }
            self.movie_stage = None;
        }
        // The interpreter tick opens by advancing the ten wait-timer words
        // (`this[0x33AC..0x33D0]`) by the frame delta in milliseconds
        // (`0x46db10` adds the clock delta to every word before the queue
        // rebuild). The native frame delta is `1000 / fps` ms; the
        // nanosecond carry keeps the 60 Hz sum exact at 50 ms per 3 ticks.
        self.timer_carry_ns += 1_000_000_000;
        let frame_delta_ms = (self.timer_carry_ns / 60_000_000) as u32;
        self.timer_carry_ns %= 60_000_000;
        self.vm.native_frame_delta_ms = frame_delta_ms;
        crate::vm::native_view::apply_channel_origins(&mut self.vm).map_err(core)?;
        self.input.project(&mut self.vm);
        let extent = self.scene.logical_extent().ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_CANVAS",
                "native input stage is not initialized",
            )
        })?;
        self.vm.native_stage_size = [extent.width, extent.height];
        self.cmv_players
            .retain(|slot, _| crate::vm::cmv::bound_target(&self.vm, *slot).is_some());
        for action in crate::vm::cmv::advance(&mut self.vm, frame_delta_ms) {
            self.action(action)?;
        }
        self.vm.native_render_size = self.native_render_size;
        if frame_delta_ms > 0 {
            for word in 0..10_u32 {
                let field = 13_228 + 4 * word;
                let value = self
                    .vm
                    .interpreter_words
                    .get(&field)
                    .copied()
                    .unwrap_or_default();
                self.vm
                    .write_interpreter_word(field, value.wrapping_add(frame_delta_ms));
            }
            self.vm.session_clock_millis =
                self.vm.session_clock_millis.wrapping_add(frame_delta_ms);
        }
        self.advance_native_timeline(false, false)?;
        self.vm.tick = self.vm.tick.wrapping_add(1);
        // The one-entry-per-frame cap already paces the poll-family
        // cascade; the overlay ring must still observe the republished
        // identity so the dialog rebuild (and its geometric confirm
        // queries) runs on the following engine frames.
        rebuild_frame_entry_queue(&mut self.vm).map_err(core)?;
        self.vm.dispatch_stopped = false;
        // While a dialog dispatch is in flight the native engine paces
        // one continuation per frame; cap the tick so the poll family
        // spreads across engine frames instead of interleaving over the
        // dispatch layout words.
        // A live dialog cascade refreshes the entry queue mid-tick the
        // way the native engine rebuilds it every frame, so the cascade
        // never overdraws one rebuild's pushes.
        let cascade_refresh = self.vm.dialog_dispatch_base_depth.is_some()
            || !self.vm.pending_dialog_dispatches.is_empty();
        let mut executed_count: u32 = 0;
        loop {
            if executed_count.is_multiple_of(256)
                && self.cancelled.load(std::sync::atomic::Ordering::Acquire)
            {
                return Err(error(
                    "SHINKI_EMU_CMVS_CANCELLED",
                    "native session was cancelled",
                ));
            }
            if executed_count >= MAX_INSTRUCTIONS {
                break;
            }
            if cascade_refresh && executed_count > 0 && executed_count.is_multiple_of(2048) {
                rebuild_frame_entry_queue(&mut self.vm).map_err(core)?;
                self.vm.dispatch_stopped = false;
            }
            if self.vm.dispatch_stopped {
                crate::vm::native_text::dialogue::advance(&mut self.vm, frame_delta_ms);
                self.scene
                    .advance_native_image_effects(&mut self.vm, frame_delta_ms)
                    .map_err(core)?;
                crate::vm::native_timeline::advance_channel_image_reloads(
                    &mut self.vm,
                    frame_delta_ms,
                )
                .map_err(core)?;
                crate::vm::texture_reload::advance(&mut self.vm, frame_delta_ms);
                self.scene.synchronize_image_reloads(&self.vm);
                // `sub_4262A0` advances and retires the mask owner once per
                // native frame, after dispatch and before composition.
                if let Some(action) =
                    crate::vm::texture_presentation::advance(&mut self.vm, frame_delta_ms)
                {
                    self.action(action)?;
                }
                crate::vm::camera_transition::advance(&mut self.vm, frame_delta_ms)
                    .map_err(core)?;
                crate::vm::independent_view::tick(&mut self.vm, frame_delta_ms).map_err(core)?;
                self.advance_native_scene_workers(frame_delta_ms)?;
                crate::vm::screen_shake::advance(&mut self.vm).map_err(core)?;
                // An active transition targets its original image family.
                // Present every tick; a skipped texture draw must not be
                // replaced by a text-only black frame.
                let effective_selected = self
                    .vm
                    .texture_presentation
                    .as_ref()
                    .map(|presentation| CmvsTextureSlot::Parent(presentation.parent))
                    .or(self.selected)
                    .or_else(|| self.scene.standing_scene_slot())
                    // Native 425D70 also paints solid-only families with no image binding.
                    .or_else(|| {
                        self.vm
                            .texture_parents
                            .keys()
                            .next()
                            .copied()
                            .map(CmvsTextureSlot::Parent)
                    });
                if let Some(selected) = effective_selected {
                    // A cleared channel animation (`sub_467A50` level 0)
                    // removes the animated surface: the scene beneath —
                    // the most recently loaded parent — becomes visible.
                    let selected =
                        if self.vm.texture_presentation.is_none() && self.fade_level <= 0.01 {
                            self.last_loaded_parent
                                .map(CmvsTextureSlot::Parent)
                                .unwrap_or(selected)
                        } else {
                            selected
                        };
                    let logical = self.scene.logical_extent().ok_or_else(|| {
                        error("SHINKI_EMU_CMVS_CANVAS", "native stage is not initialized")
                    })?;
                    let movie = self.movie_stage.as_ref().map(|(_, frame)| frame);
                    if let Some(frame) = self
                        .scene
                        .render(
                            &mut self.vm,
                            selected,
                            &self.archive,
                            (logical.width, logical.height),
                            movie,
                        )
                        .map_err(core)?
                    {
                        self.frame = frame;
                    }
                } else {
                    // Native present clears even an empty scene. Publishing
                    // the last owner's removal must not depend on a font load.
                    let logical = self.scene.logical_extent().ok_or_else(|| {
                        error("SHINKI_EMU_CMVS_CANVAS", "native stage is not initialized")
                    })?;
                    if let Some(frame) = self
                        .scene
                        .render(
                            &mut self.vm,
                            CmvsTextureSlot::Parent(0),
                            &self.archive,
                            (logical.width, logical.height),
                            self.movie_stage.as_ref().map(|(_, frame)| frame),
                        )
                        .map_err(core)?
                    {
                        self.frame = frame;
                    }
                }
                return Ok(());
            }
            let script = self.scripts.get(&self.vm.current_frame).ok_or_else(|| {
                error("SHINKI_EMU_CMVS_SCRIPT_FRAME", "script frame is not loaded")
            })?;
            let instruction =
                frame_ps2a_instruction(script, self.vm.program_counter).map_err(core)?;
            // While an engine request is live, remember the last pc the
            // requested coroutine occupied inside its reachable region —
            // the record table's mutable park slot. The story contexts
            // running between clicks never enter this region, so the
            // remembered pc is exactly the pass's own wait park.
            if self.vm.engine_request_live
                && ((179_768..=193_300).contains(&self.vm.program_counter)
                    || (405_818..=410_400).contains(&self.vm.program_counter))
            {
                self.vm.engine_request_park =
                    Some((self.vm.program_counter, u32::from(self.vm.current_frame)));
            }
            if matches!(
                &instruction,
                CmvsPs2aInstructionFrame::Command {
                    command_id: 339 | 344,
                    ..
                }
            ) {
                self.vm.native_audio_playing = match self.audio.as_ref() {
                    Some(audio) => audio.playing_channels()?,
                    None => 0,
                };
            }
            if matches!(
                &instruction,
                CmvsPs2aInstructionFrame::Command {
                    command_id: 189,
                    ..
                }
            ) {
                self.vm.native_audio_present = 0;
                if let (Some(channel), Some(bridge)) = (
                    self.vm.native_audio_poll_channel().map_err(core)?,
                    self.audio.as_ref(),
                ) && bridge.snapshot(channel, false)?.exists
                {
                    self.vm.native_audio_present = 1 << channel;
                }
            }
            let executed = execute_cmvs390_frame(&mut self.vm, &instruction);
            if let Err(failure) = executed {
                tracing::error!(
                    event = "cmvs.session.execute_failed",
                    pc = self.vm.program_counter,
                    frame = self.vm.current_frame,
                    cursor = self.vm.stack_cursor_bytes,
                    depth = self.vm.call_frame_bases.len(),
                    tick = self.vm.tick
                );
                return Err(diagnostics::instruction(&instruction, core(failure)));
            }
            if let Some(action) = executed.unwrap() {
                self.action(action)
                    .map_err(|failure| diagnostics::instruction(&instruction, failure))?;
            }
            // The overlay ring's gate at 370566 reads the polled word and
            // its own jfc at 370590 branches on it: with the ring alive
            // the script consumes the message itself, so the host must
            // neither clear nor re-queue it here.
            executed_count += 1;
        }
        // A busy post-switch tick (the revived record cycle runs the
        // instruction budget without parking) still presents: the
        // native engine composes every frame regardless of the script
        // scheduler's state.
        if executed_count >= MAX_INSTRUCTIONS {
            crate::vm::screen_shake::advance(&mut self.vm).map_err(core)?;
            self.scene
                .advance_native_image_effects(&mut self.vm, frame_delta_ms)
                .map_err(core)?;
            crate::vm::native_timeline::advance_channel_image_reloads(&mut self.vm, frame_delta_ms)
                .map_err(core)?;
            crate::vm::texture_reload::advance(&mut self.vm, frame_delta_ms);
            self.scene.synchronize_image_reloads(&self.vm);
            crate::vm::camera_transition::advance(&mut self.vm, frame_delta_ms).map_err(core)?;
            crate::vm::independent_view::tick(&mut self.vm, frame_delta_ms).map_err(core)?;
            self.advance_native_scene_workers(frame_delta_ms)?;
            let effective_selected = self
                .selected
                .or_else(|| self.last_loaded_parent.map(CmvsTextureSlot::Parent));
            if let Some(selected) = effective_selected
                && let Some(logical) = self.scene.logical_extent()
            {
                let movie = self.movie_stage.as_ref().map(|(_, frame)| frame);
                if let Some(frame) = self
                    .scene
                    .render(
                        &mut self.vm,
                        selected,
                        &self.archive,
                        (logical.width, logical.height),
                        movie,
                    )
                    .map_err(core)?
                {
                    self.frame = frame;
                }
            }
        }
        Err(error(
            "SHINKI_EMU_CMVS_STEP_BOUND",
            "instruction limit exceeded",
        ))
    }

    fn advance_inner(
        &mut self,
        elapsed_ns: u64,
        events: &[RuntimeEvent],
    ) -> SessionResult<AdvanceResponse> {
        let mut changed = false;
        for event in events {
            self.input.event(&mut self.vm, event);
            match event {
                RuntimeEvent::WindowSuspended { suspended } => {
                    changed |= self.suspended != *suspended;
                    self.suspended = *suspended;
                }
                RuntimeEvent::WindowResized { width, height } => {
                    self.scene.resize_raster(*width, *height).map_err(core)?;
                }
                RuntimeEvent::PointerMove { x, y } => {
                    let [x, y] = self
                        .scene
                        .native_pointer(&self.vm, [*x, *y])
                        .map_err(core)?;
                    self.vm.pointer_x = x;
                    self.vm.pointer_y = y;
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } => {
                    // The native input manager resolves the key-quad id
                    // from the click message itself (field 347 armed at
                    // once); arming at the VM's poll cadence lets the
                    // dialog chain's poll consume the edge first and
                    // leaves a stale id. Resolve the quad here.
                    if self.input.menu_opened
                        && let Some(quad) = self.vm.menu_quad_hit()
                    {
                        self.vm.input_selection_mirror = quad;
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                } if self.input.menu_opened => {
                    // The native engine's Win32 menu handler posts the
                    // record-17 resume through the request dwords while
                    // the script scheduler is off (`sub_46DF10`'s disabled
                    // branch reads gate 3500 / frame 3501 / pc 3502).
                    self.vm.engine_request_armed = true;
                    self.vm.write_interpreter_word(3500 * 4, 1);
                    self.vm.write_interpreter_word(3501 * 4, 1);
                    self.vm.write_interpreter_word(3502 * 4, 179_768);
                    // The engine's message pump reopens the entry-queue
                    // rebuild gate with each posted request — the ring's
                    // cmd137(0) close must not strand the dwords.
                    self.vm.write_interpreter_word(0x29DC, 0);
                    // The same handler publishes the save-request flag
                    // into process word 16 — the intproc save sequence's
                    // write gate (193170) polls it — and the click
                    // indicator into the frame-1 data segment's word
                    // 276, the save-screen poll loop's exit word
                    // (intproc 410138).
                    self.vm.process_indexed_words.insert(16, 1);
                    self.vm
                        .script_data_segment_words
                        .entry(1)
                        .or_default()
                        .insert(276, 1);
                    // Only the active system menu uses this headless callback
                    // dispatcher. Story choices also use parent 60, but their
                    // press/release belongs to the native 535 filter-chain poll.
                    // Do not resume the menu's record 29 for a story choice.
                    let px = i64::from(self.vm.pointer_x);
                    let py = i64::from(self.vm.pointer_y);
                    let vm = &self.vm;
                    let hits: Vec<(u32, i32)> = vm
                        .texture_children
                        .iter()
                        .filter(|(parent, _)| {
                            vm.texture_parents
                                .get(parent)
                                .is_some_and(|image| image.surface_initialized && !image.hidden)
                        })
                        .flat_map(|(parent, children)| {
                            children.iter().filter_map(move |(child, image)| {
                                if !image.surface_initialized || image.hidden {
                                    return None;
                                }
                                let rect = image.rect_words?;
                                let root = vm
                                    .texture_parents
                                    .get(parent)?
                                    .auxiliary_pair_words
                                    .unwrap_or([0; 2]);
                                let local = image.auxiliary_pair_words.unwrap_or([0; 2]);
                                let left = i64::from(root[0].wrapping_add(local[0]) as i32);
                                let top = i64::from(root[1].wrapping_add(local[1]) as i32);
                                (px >= left
                                    && py >= top
                                    && px < left + i64::from(rect[2])
                                    && py < top + i64::from(rect[3]))
                                .then_some((u32::from(*parent), i32::from(*child)))
                            })
                        })
                        .collect();
                    // Native `WindowFromPoint` resolves the window
                    // z-order: the modal confirm dialog (channel 60) sits
                    // above the system-menu quad layer, which in turn sits
                    // above the full-screen presenter overlay. While a
                    // dialog child contains the point it wins; otherwise a
                    // quad-layer selection armed on this press takes the
                    // click; the overlay only absorbs what nothing else
                    // claimed.
                    let dialog_hit = hits
                        .iter()
                        .find(|(ch, child)| *ch == 60 && *child >= 0)
                        .copied();
                    let overlay_hit = hits
                        .iter()
                        .min_by_key(|(ch, child)| {
                            self.vm
                                .texture_children
                                .get(&(*ch as u8))
                                .and_then(|children| children.get(&(*child as u16)))
                                .and_then(|image| image.rect_words)
                                .map_or(i64::MAX, |rect| i64::from(rect[2]) * i64::from(rect[3]))
                        })
                        .copied();
                    // The confirm dialog is filter-chain driven (the はい
                    // is a chain record): while the chain holds an active
                    // record the click belongs to it. Once the save flow
                    // clears the chain, the same point resolves to the
                    // menu window's quad layer — native's arrow quads own
                    // the corner once the dialog child is gone.
                    // A click landing inside an active filter-chain
                    // record rectangle belongs to the chain (the load
                    // page's row list): native's list child window owns
                    // those points above the quads.
                    if let Some((_, child)) = dialog_hit.or(overlay_hit) {
                        let id = (child as u32).wrapping_add(1);
                        // The presenter path never touches the input
                        // manager's selection (native resolves through the
                        // child-window tree); clear the mirrored id so a
                        // presenter hit does not leak into the dispatcher.
                        self.vm.input_selection_mirror = 0;

                        // The polled word carries the identity to the
                        // overlay ring's gate; its message path lays the
                        // button's visual state and stores the sentinel
                        // back itself. The confirm *action* runs through
                        // the handler registered in record slot 29: the
                        // native input callback resumes it as a plain
                        // record context (the handler reads the identity
                        // from the polled word), so arm that slot for the
                        // next rebuild instead of the dispatch layout.
                        self.vm.process_indexed_words.insert(17, id);
                        const DIALOG_HANDLER_SLOT: u32 = 29;
                        const RECORD_TABLE_BASE: u32 = 0x33DC;
                        let slot_base = RECORD_TABLE_BASE + DIALOG_HANDLER_SLOT * 28;
                        if let (Some(frame), Some(handler_pc)) = (
                            self.vm.interpreter_words.get(&slot_base).copied(),
                            self.vm.interpreter_words.get(&(slot_base + 4)).copied(),
                        ) && handler_pc != u32::MAX
                        {
                            self.vm.forced_dialog_record = Some((frame, handler_pc, id));
                        }
                    } else if self.vm.input_selection_mirror != 0 {
                        // Native resolves the click through the child-window
                        // tree: a presenter hit goes to the overlay ring and
                        // never touches the input manager's selection; a
                        // key-quad hit parks the selection id (field 347)
                        // and the input callback re-enters the item
                        // dispatcher (intproc 186920) with the id laid as
                        // its fl4 input slot. Mirror the second path.
                        let id = self.vm.input_selection_mirror;
                        // The item dispatcher derives its ladder input
                        // (fl4) from process word 10 each poll
                        // (`intproc` 186960): the callback publishes the
                        // selected id there.
                        self.vm.process_indexed_words.insert(10, id);
                        self.vm.queue_dialog_dispatch(186_920, 1, id);
                        tracing::info!(
                            event = "cmvs.session.quad_dispatch_queued",
                            tick = self.vm.tick,
                            id
                        );
                    }
                }
                RuntimeEvent::TextInput { .. } => {
                    return Err(error(
                        "SHINKI_EMU_CMVS_INPUT_UNBOUND",
                        "native input handling is not connected",
                    ));
                }
                _ => {}
            }
        }
        if self.suspended || changed {
            self.phase = 0;
            return Ok(AdvanceResponse {
                reset_clock: true,
                ..AdvanceResponse::running()
            });
        }
        self.phase = self
            .phase
            .checked_add(
                elapsed_ns
                    .checked_mul(60)
                    .ok_or_else(|| error("SHINKI_EMU_CMVS_TIME", "clock overflow"))?,
            )
            .ok_or_else(|| error("SHINKI_EMU_CMVS_TIME", "clock overflow"))?;
        if self.phase / 1_000_000_000 > 600 {
            return Err(error(
                "SHINKI_EMU_CMVS_TIME",
                "elapsed time exceeds tick budget",
            ));
        }
        while self.phase >= 1_000_000_000 {
            self.phase -= 1_000_000_000;
            self.tick()?;
        }
        Ok(AdvanceResponse {
            window_command: self.pending_window.take(),
            ..AdvanceResponse::running()
        })
    }
}
impl CmvsSession {
    pub(crate) fn native_frame(
        &self,
    ) -> SessionResult<(FrameInfo, roukaku_media_core::OwnedPixelBuffer)> {
        Ok((self.frame_info()?, self.frame.rgba8.clone()))
    }
    pub(crate) fn native_audio_cancellation(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.cancelled.clone()
    }
    pub(crate) fn native_pause(&mut self, paused: bool) -> SessionResult<()> {
        let changed = self.suspended != paused;
        self.suspended = paused;
        if let Some(audio) = &mut self.audio {
            audio.suspend(paused)?;
        }
        if changed {
            self.phase = 0;
        }
        Ok(())
    }
    pub(crate) fn native_suspended(&self) -> bool {
        self.suspended
    }
    pub(crate) fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[RuntimeEvent],
    ) -> SessionResult<AdvanceResponse> {
        if let Some(audio) = &self.audio {
            audio.check_failure()?;
        }
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let result = self.advance_inner(elapsed_ns, events);
        if let Err(failure) = &result {
            self.failure = Some(failure.clone());
        }
        result
    }
    pub(crate) fn close(self: Box<Self>) -> SessionResult<()> {
        // Cancel outstanding PCM writes and join the audio worker before
        // the scene and lease drop.
        let mut session = *self;
        session
            .cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        let movie = session
            .movie
            .take()
            .map(|mut movie| match session.audio.as_mut() {
                Some(audio) => movie.close(audio),
                None => Ok(()),
            })
            .transpose();
        let audio = session
            .audio
            .take()
            .map(|mut bridge| bridge.close())
            .transpose();
        movie?;
        audio?;
        tracing::info!(event = "shinki.emu.cmvs.session.close");
        Ok(())
    }
}

/// Builds the case-696 save snapshot: a bottom-up 24-bpp BMP of the
/// presented frame, box-averaged down to the case-688 dimensions.
fn build_save_snapshot_bmp(
    rgba8: &[u8],
    source_width: u32,
    source_height: u32,
    out_width: u32,
    out_height: u32,
) -> Vec<u8> {
    let row_bytes = (out_width as usize * 3).div_ceil(4) * 4;
    let pixel_bytes = row_bytes * out_height as usize;
    let file_size = 54 + pixel_bytes;
    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&54_u32.to_le_bytes());
    bmp.extend_from_slice(&40_u32.to_le_bytes());
    bmp.extend_from_slice(&(out_width as i32).to_le_bytes());
    bmp.extend_from_slice(&(out_height as i32).to_le_bytes());
    bmp.extend_from_slice(&1_u16.to_le_bytes());
    bmp.extend_from_slice(&24_u16.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    bmp.extend_from_slice(&0_i32.to_le_bytes());
    bmp.extend_from_slice(&0_i32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    bmp.extend_from_slice(&0_u32.to_le_bytes());
    for row in 0..out_height as usize {
        // Bottom-up row order.
        let source_row = out_height as usize - 1 - row;
        let y0 = source_row * source_height as usize / out_height as usize;
        let y1 = ((source_row + 1) * source_height as usize / out_height as usize).max(y0 + 1);
        for column in 0..out_width as usize {
            let x0 = column * source_width as usize / out_width as usize;
            let x1 = ((column + 1) * source_width as usize / out_width as usize).max(x0 + 1);
            let mut sums = [0_u32; 3];
            let mut samples = 0_u32;
            for y in y0..y1.min(source_height as usize) {
                for x in x0..x1.min(source_width as usize) {
                    let index = (y * source_width as usize + x) * 4;
                    sums[0] += rgba8[index] as u32;
                    sums[1] += rgba8[index + 1] as u32;
                    sums[2] += rgba8[index + 2] as u32;
                    samples += 1;
                }
            }
            let samples = samples.max(1);
            bmp.extend_from_slice(&[
                (sums[2] / samples) as u8,
                (sums[1] / samples) as u8,
                (sums[0] / samples) as u8,
            ]);
        }
        bmp.resize(54 + (row + 1) * row_bytes, 0);
    }
    bmp
}

#[cfg(test)]
pub(crate) mod tests;
