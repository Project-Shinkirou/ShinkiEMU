use crate::family::{core, error, Lease};
use crate::*;
use roukaku_media_core::{RectI, TextureFrame};
use shinki_emu_sdk::runtime::*;
use std::{collections::BTreeMap, sync::Arc};
mod audio;
mod diagnostics;
mod input;
mod movie;
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
    pc_hotspots: std::collections::HashMap<(u16, u32), u32>,
    entered_watch_range: bool,
    last_logged_pc: u32,
    last_logged_frame: u16,
    misaligned_report: std::collections::BTreeSet<(u16, u32)>,
    vm: CmvsPs2aVmState,
    scripts: BTreeMap<u16, CmvsScript>,
    scene: CmvsScene,
    frame: TextureFrame,
    selected: Option<CmvsTextureSlot>,
    last_present_target_logged: Option<u32>,
    /// The live channel animation level (`sub_467A50` dword 18/19) scaled
    /// to 0..1; 0 marks a cleared animation and the scene beneath shows.
    fade_level: f32,
    /// A load whose interpreter-state switch waits for the menu render
    /// pump to exit (the native program switch happens after the fade
    /// completes and the menu unwinds, not at the script's cmd 693).
    pending_load: Option<Box<crate::system_save::slot::CmvsSlotState>>,
    /// The story interpreter context captured when the system menu
    /// opened: the native save serializes the suspended story, not the
    /// menu's own flying state (its transient records would replay the
    /// save chain on load and wreck the scheduler cycle).
    story_snapshot: Option<Box<StorySnapshot>>,
    /// One delayed resume pulse fired after the load's menu-teardown
    /// storm ends: the poll's last park can sit between its scheduler
    /// disable and re-enable, and the single native-style request
    /// finishes the toggle so the scan resumes the cycle.
    load_storm_end_tick: Option<u64>,
    /// The most recently loaded parent slot, presented when the animation
    /// clears the current surface.
    last_loaded_parent: Option<u8>,
    phase: u64,
    suspended: bool,
    input: input::InputState,
    /// Auto-advance click phase: `true` between a synthesized press and
    /// its release. See `pump_auto_advance`.
    auto_click_held: bool,
    /// Reverse-engineering aid: tick counter gating the periodic idle
    /// snapshot diagnostic. Never participates in execution or snapshots.
    idle_probe: u32,
    last_frozen_resume: Option<(u32, u16)>,
    last_tick_resume: Option<(u16, u32)>,
    last_dispatch_frame: Option<u16>,
    last_state_zone_pc: Option<u32>,
    last_fade_level: Option<f32>,
    script_names: std::collections::BTreeMap<u16, String>,
    entry_name: String,
    last_frozen_tail: Vec<(u16, u32)>,
    tick_ring: Vec<(u32, u32, u16, u32)>,
    prev_scheduler_enabled: Option<bool>,
    flip_trace_ticks: u32,
    last_waiter_cursor3: Option<(u16, u32, u32)>,
    /// Ticks remaining of post-dispatch pacing; the poll cascade keeps
    /// interleaving for several frames after the dispatch completes.
    cascade_pace_ticks: u32,
    movie: Option<movie::MoviePlayer>,
    audio: Option<audio::AudioBridge>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
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
        // Host-side auto-advance stand-in for the native auto/skip toggle:
        // the story's text gate (`ReadAutomaticAdvance`, case 751) polls
        // this mode every frame while a line is on screen.
        vm.input_auto_mode = std::env::var("SHINKI_EMU_CMVS_AUTO_ADVANCE").is_ok_and(|v| v == "1");
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
        let frame = scene.clear(window.width, window.height).map_err(core)?;
        let audio = open_audio()?;
        let cancelled = audio.cancellation_flag();
        tracing::info!(event = "shinki.emu.cmvs.session.open");
        Ok(Self {
            archive,
            game_root,
            pc_hotspots: std::collections::HashMap::new(),
            entered_watch_range: false,
            last_logged_pc: 0,
            last_logged_frame: 0,
            misaligned_report: std::collections::BTreeSet::new(),
            vm,
            scripts: BTreeMap::from([(0, script)]),
            scene,
            frame,
            selected: None,
            last_present_target_logged: None,
            fade_level: 1.0,
            pending_load: None,
            story_snapshot: None,
            load_storm_end_tick: None,
            last_loaded_parent: None,
            phase: 0,
            suspended: false,
            input: input::InputState::default(),
            auto_click_held: false,
            idle_probe: 0,
            last_frozen_resume: None,
            last_tick_resume: None,
            last_dispatch_frame: None,
            last_state_zone_pc: None,
            last_fade_level: None,
            script_names: std::collections::BTreeMap::new(),
            entry_name: entry.to_owned(),
            last_frozen_tail: Vec::new(),
            tick_ring: Vec::new(),
            prev_scheduler_enabled: None,
            flip_trace_ticks: 0,
            last_waiter_cursor3: None,
            cascade_pace_ticks: 0,
            audio: Some(audio),
            cancelled,
            movie: None,
            movie_stage: None,
            timer_carry_ns: 0,
            failure: None,
            pending_window: window::WindowCommands::default(),
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
        self.script_names.insert(frame, name.clone());
        let script = self
            .archive
            .load_native_script(&name, frame, &mut self.vm)
            .map_err(core)?;
        if let (Some(out), Ok(want)) = (
            std::env::var_os("DUMP_RUNTIME_SCRIPT"),
            std::env::var("DUMP_RUNTIME_SCRIPT_NAME"),
        ) {
            if name.contains(&want) {
                let _ = std::fs::write(out, script.bytecode.as_slice());
            }
        }
        if frame == 2
            && name.contains("intcode")
            && std::env::var_os("DUMP_RUNTIME_SCRIPT").is_some()
            && std::env::var_os("DUMP_RUNTIME_SCRIPT_NAME").is_none()
        {
            if let Some(out) = std::env::var_os("DUMP_RUNTIME_SCRIPT") {
                let _ = std::fs::write(&out, script.bytecode.as_slice());
            }
        }
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
        self.scripts.insert(frame, script);
        self.vm.current_frame = frame;
        Ok(())
    }
    fn action(&mut self, action: CmvsPs2aVmAction) -> SessionResult<()> {
        match action {
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
            CmvsPs2aVmAction::ScanNativeSaveSlots { count } => self
                .archive
                .refresh_native_save_catalog(&mut self.vm, count)
                .map_err(core),

            CmvsPs2aVmAction::NativeText(action) => {
                match action {
                    CmvsNativeTextAction::Create { slot }
                    | CmvsNativeTextAction::SetFont { slot } => {
                        self.scene
                            .bind_text_font(&self.vm.native_text[&slot].font)
                            .map_err(core)?;
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
            CmvsPs2aVmAction::CallScript { frame, name } => self.load(frame, name),
            CmvsPs2aVmAction::ReloadRootScript { name } => self.load(0, name),
            CmvsPs2aVmAction::EnsurePathDirectories { buffer_offset } => self
                .archive
                .ensure_path_directories(&self.vm, buffer_offset)
                .map_err(core),
            CmvsPs2aVmAction::LoadTextureParentResource {
                parent_slot,
                resource,
            } => {
                // `sub_483D70` reports load failure through the result
                // word and continues; a misaligned pool read producing a
                // directory-like name is native behavior and must not end
                // the session.
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
                            name = %resource,
                            code = cause.code()
                        );
                        self.vm.interpreter_words.insert(81220, 0);
                        return Ok(());
                    }
                };
                tracing::debug!(
                    event = "cmvs.session.texture_parent_load",
                    slot = parent_slot,
                    name = %resource
                );
                self.scene
                    .bind(CmvsTextureSlot::Parent(parent_slot), &uri, &self.archive)
                    .map_err(core)?;
                record_completed_load(&mut self.vm);
                self.last_loaded_parent = Some(parent_slot);
                self.vm.interpreter_words.insert(81220, 1);
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
                self.scene
                    .bind(
                        CmvsTextureSlot::Child {
                            parent: parent_slot,
                            child: child_id,
                        },
                        &uri,
                        &self.archive,
                    )
                    .map_err(core)?;
                record_completed_load(&mut self.vm);
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
                            Some(bridge) => {
                                movie::MoviePlayer::open(bytes.as_slice(), bridge, &self.cancelled)
                            }
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
                                    video = player.has_video_stream()
                                );
                                self.vm.movie_object = Some(uri);
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
                self.vm
                    .interpreter_words
                    .insert(result_field_offset, result);
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
                    .play_crossfade(primary, secondary, fade_ms, playback_flag)
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
            CmvsPs2aVmAction::PlayChannelSound { channel, name } => {
                if self.audio.is_none() {
                    return Err(error(
                        "SHINKI_EMU_CMVS_MEDIA_UNBOUND",
                        "native media channel is not connected",
                    ));
                }
                // A name that resolves through no bank fails natively
                // without stopping the scene: the sound simply never
                // starts, exactly like a texture load failure.
                let clip = match self.decode_channel_clip(&name) {
                    Ok(clip) => clip,
                    Err(cause) => {
                        tracing::info!(
                            event = "cmvs.session.channel_sound_resolve_failed",
                            name = %name,
                            code = cause.code()
                        );
                        return Ok(());
                    }
                };
                let channel = u8::try_from(channel).map_err(|_| {
                    error(
                        "SHINKI_EMU_CMVS_AUDIO_CHANNEL",
                        "native sound channel index is out of range",
                    )
                })?;
                self.audio
                    .as_mut()
                    .expect("audio bridge checked above")
                    .play_channel(channel, clip, false)
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
                let result = self
                    .archive
                    .stat(&format!("cmvs:/native-files/save/save{slot:03}.dat"))
                    .is_ok() as u32;
                self.vm
                    .interpreter_words
                    .insert(result_field_offset, result);
                Ok(())
            }
            CmvsPs2aVmAction::SetNativeSaveTitle {
                slot: _,
                result_field_offset,
            } => {
                // The header rewrite needs the native save serializer's
                // file layout; without a written save the rewrite fails
                // exactly like the missing-file path.
                self.vm.interpreter_words.insert(result_field_offset, 0);
                Ok(())
            }
            CmvsPs2aVmAction::SetNativeSaveMeta {
                slot: _,
                result_field_offset,
            } => {
                self.vm.interpreter_words.insert(result_field_offset, 0);
                Ok(())
            }
            CmvsPs2aVmAction::LoadNativeSaveInfo {
                slot,
                result_field_offset,
            } => {
                // `sub_4855C0`: the save browser reads the header of
                // saveNNN.dat; a file this session wrote unpacks cleanly
                // and reports success, anything else stays the native
                // missing-file failure.
                let path = self
                    .game_root
                    .join("save")
                    .join(crate::system_save::slot::slot_file_name(slot));
                match std::fs::read(&path) {
                    Ok(bytes) => match crate::system_save::slot::unpack_save_container(&bytes) {
                        Ok(_) => {
                            self.vm.interpreter_words.insert(result_field_offset, 1);
                        }
                        Err(_) => {
                            self.vm.interpreter_words.insert(result_field_offset, 0);
                        }
                    },
                    Err(_) => {
                        self.vm.interpreter_words.insert(result_field_offset, 0);
                    }
                }
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
                let loaded = std::fs::read(&path).ok().and_then(|bytes| {
                    crate::system_save::slot::unpack_save_container(&bytes)
                        .ok()
                        .and_then(|tlv| crate::system_save::slot::deserialize_prog_state(&tlv).ok())
                });
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
                        self.vm.interpreter_words.insert(81220, 1);
                    }
                    None => {
                        self.vm.interpreter_words.insert(81220, 0);
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
                let next = value.min(255) as f32 / 255.0;
                if tracing::enabled!(tracing::Level::INFO) && self.last_fade_level != Some(next) {
                    self.last_fade_level = Some(next);
                    tracing::info!(
                        event = "cmvs.session.fade_level",
                        value = next,
                        channel = channel,
                        tick = self.vm.tick
                    );
                }
                self.fade_level = next;
                Ok(())
            }
            CmvsPs2aVmAction::SaveSystemState => {
                // `sub_427E80` serializes the whole system state into the
                // ciphered container; failures leave the previous file in
                // place like the native write-through.
                let payload = crate::system_save::build_system_payload(&self.vm);
                let identity = self.vm.interpreter_words.get(&1600).copied().unwrap_or(0);
                let lists = [
                    self.vm.native_system_lists[0].len(),
                    self.vm.native_system_lists[1].len(),
                ];
                let bytes = crate::system_save::encode_system_container(&payload, identity, &lists);
                let dir = self.game_root.join("save");
                std::fs::create_dir_all(&dir).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("save directory unavailable: {cause}"),
                    )
                })?;
                let path = dir.join("system.dat");
                std::fs::write(&path, &bytes).map_err(|cause| {
                    error(
                        "SHINKI_EMU_CMVS_SAVE_WRITE",
                        &format!("system state write failed: {cause}"),
                    )
                })?;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "cmvs.session.system_state_saved",
                        bytes = bytes.len(),
                        tick = self.vm.tick
                    );
                }
                Ok(())
            }
            CmvsPs2aVmAction::SaveNativeGameSlot { slot } => {
                // `Save_writeFile` (0x4774D0): the ProgState TLV is LZSS
                // packed, XOR'd with the fixed key and stored in the
                // saveNNN.dat container inside the game's save directory.
                // The native save serializes the suspended story, not
                // the menu's flying state; use the menu-open snapshot.
                let story = match self.story_snapshot.as_deref() {
                    Some(snapshot) => snapshot.state.clone(),
                    None => snapshot_story_context(&self.vm),
                };
                let tlv = crate::system_save::slot::serialize_snapshot(&story).map_err(core)?;
                let bytes = crate::system_save::slot::pack_save_container(&tlv, &[], &[]);
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
                // The native save flow destroys the confirm-dialog
                // overlay when it completes; the window hit test then
                // resolves the page-arrow quad instead of the はい
                // button. Tear the channel-64 presenter family down so
                // the follow-up clicks reach the quad layer.
                self.vm
                    .channel_child_presenters
                    .retain(|(ch, _), _| *ch != 64);
                self.input.menu_save_completed = true;
                tracing::info!(
                    event = "cmvs.session.save_written",
                    slot,
                    bytes = bytes.len()
                );
                // `sub_4774D0` ends by resuming the poll family through
                // the engine request dwords (its record-30 push): the
                // save screen redraws and the flow continues natively.
                self.vm.engine_request_armed = true;
                self.vm.interpreter_words.insert(3500 * 4, 1);
                self.vm.interpreter_words.insert(3501 * 4, 1);
                self.vm.interpreter_words.insert(3502 * 4, 179_768);
                self.vm.interpreter_words.insert(0x29DC, 0);
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
            .load_audio(&uri, audio::MAX_CLIP_FRAMES, &self.cancelled)
            .map_err(core)?;
        Ok(audio::DecodedClip::from_kira(data))
    }

    /// Resolves one channel-sound name through the configured native audio
    /// search domains and decodes it once for the audio worker.
    fn decode_channel_clip(&self, name: &str) -> SessionResult<audio::DecodedClip> {
        let banks = [
            CmvsResourceSearchBank::CrossfadeAudio,
            CmvsResourceSearchBank::Script,
            CmvsResourceSearchBank::Native0d0c,
            CmvsResourceSearchBank::Native0d14,
            CmvsResourceSearchBank::Native0d00,
            CmvsResourceSearchBank::Native0d04,
            CmvsResourceSearchBank::Native0d18,
        ];
        let mut uri = None;
        for bank in banks {
            if let Ok(found) = self.archive.resolve_native_resource(&self.vm, bank, name) {
                uri = Some(found);
                break;
            }
        }
        let uri = uri.ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_AUDIO_RESOURCE",
                "channel sound name resolved through no configured native search domain",
            )
        })?;
        let data = self
            .archive
            .load_audio(&uri, audio::MAX_CLIP_FRAMES, &self.cancelled)
            .map_err(core)?;
        Ok(audio::DecodedClip::from_kira(data))
    }
    /// Applies a deferred load's interpreter switch: the menu render pump
    /// has exited (`tick`'s 349086 watch) and the saved program state
    /// replaces the live one, exactly like the native program switch.
    fn apply_pending_load(&mut self) {
        let Some(state) = self.pending_load.take() else {
            return;
        };
        // `sub_475E40` restores the engine's full dword image: every
        // interpreter word, process word and flag the save carried
        // replaces the live tables wholesale, so the menu's words die
        // with its program. Apply the snapshot's tables first.
        if let Some(snapshot) = self.story_snapshot.as_deref() {
            self.vm.interpreter_words = snapshot.interpreter.clone();
            self.vm.process_indexed_words = snapshot.process.clone();
            self.vm.process_flag_bits = snapshot.flags.clone();
            self.scene.restore_bindings(
                snapshot.scene_bindings.clone(),
                snapshot.scene_geometry.clone(),
            );
            // The present target and the fallback parent drive the
            // compositor's requested family; without these the render
            // keeps requesting the menu's torn-down surface.
            self.selected = snapshot.selected;
            self.last_loaded_parent = snapshot.last_loaded_parent;
            self.vm.slot_objects = snapshot.slot_objects.clone();
            self.vm.texture_parents = snapshot.texture_parents.clone();
            self.vm.texture_children = snapshot.texture_children.clone();
            self.vm.retained_slot_placements = snapshot.retained_slot_placements.clone();
        }
        {
            if let Some((counter, resume_pc, resume_frame)) = state.suspend {
                self.vm.interpreter_words.insert(3306, counter);
                self.vm.interpreter_words.insert(3304, resume_frame);
                if let Ok(frame) = u16::try_from(resume_frame) {
                    self.vm.current_frame = frame;
                }
                // The saved continuation must land on a decoded
                // instruction start. A capture taken while the
                // frame sat between an expression's terminator
                // and its successor stores an interior offset;
                // snap forward to the next valid start (native
                // resumes the coroutine at the instruction
                // boundary the record table was built from).
                let valid = self.vm.script_valid_starts.get(&self.vm.current_frame);
                let snapped = match valid {
                    Some(starts) if starts.contains(&resume_pc) => resume_pc,
                    Some(starts) => {
                        let next = starts
                            .iter()
                            .copied()
                            .filter(|pc| *pc > resume_pc)
                            .min()
                            .unwrap_or(resume_pc);
                        tracing::info!(
                            event = "cmvs.session.save_resume_snapped",
                            saved = resume_pc,
                            resume = next,
                            frame = self.vm.current_frame
                        );
                        next
                    }
                    None => resume_pc,
                };
                self.vm.interpreter_words.insert(3305, snapped);
                self.vm.program_counter = snapped;
            }
            // The value stack's words mean nothing without
            // the frame bases and cursor that segment them:
            // the serializer stores both (`frame_bases`,
            // `value_stack_cursor_hint`) and the native
            // `Prog_loadAndSwitch` restores them alongside the
            // words, or the return cascade cannot unwind to
            // the story's suspension.
            self.vm.call_frame_bases = state.frame_bases.clone();
            if let Some(cursor) = state.value_stack_cursor_hint {
                self.vm.stack_cursor_bytes = cursor.min(self.vm.stack_bytes.len() as u32);
            }
            for (index, word) in state.value_stack.iter().enumerate() {
                let offset = (index * 4) as u32;
                if (offset + 4) as usize <= self.vm.stack_bytes.len() {
                    self.vm.stack_bytes[offset as usize..offset as usize + 4]
                        .copy_from_slice(&word.to_le_bytes());
                }
            }
            // The restored image defines the whole stack: the
            // native tail reads unwritten words as stale
            // buffer noise rather than faulting.
            for flag in self.vm.stack_initialized.iter_mut() {
                *flag = true;
            }
            for (index, value) in state.globals.iter().enumerate() {
                if let Some(word) = value {
                    self.vm.interpreter_words.insert(640 + index as u32, *word);
                }
            }
            // `sub_471F50` copies the live record table at
            // load start and restores that copy — the table
            // survives the switch (the save file carries the
            // script bytecode, not the records). Keeping the
            // live records preserves the orchestrator's
            // in-flight markers so the record-29 re-run
            // routes to completion instead of replaying.
            tracing::info!(
                event = "cmvs.session.save_loaded",
                records = state.records.len(),
                tick = self.vm.tick
            );
            self.vm.interpreter_words.insert(81220, 1);
            // `sub_471F50`'s switch tail: reset the raw request bank
            // (3515=0, 3516=-1, 3514=0) and push the orchestrator
            // record (`sub_477F60(29)`) — the record the save flow's
            // confirm uses. The restored table's slot 29 carries the
            // story's entry; the forced-record push resumes it.
            self.vm.interpreter_words.insert(3515 * 4, 0);
            self.vm.interpreter_words.insert(3516 * 4, u32::MAX);
            self.vm.interpreter_words.insert(3514 * 4, 0);
            // The story's dialogue-wait submit flag (process word 9)
            // arms the auto-advance pump; the snapshot carries the
            // interpreter state but not the live process words, and the
            // restored story sits at a dialogue await. Re-arm it so the
            // pump's synthetic confirms keep driving the story.
            self.vm.process_indexed_words.insert(9, 1);
            // The program switch closes the system menu: without this
            // the session's input layer keeps routing confirms (the
            // pump's synthetic Enters included) into the menu logic
            // instead of the restored story.
            self.input.close_menu();
            self.vm.story_dispatch_armed = true;
            self.load_storm_end_tick = Some(self.vm.tick.saturating_add(3000));
            // Native's switch swaps the whole execution context; the
            // menu coroutine's in-flight dispatch state (queues, parked
            // chains, request parks) would keep dragging the vm's
            // program counter away from the restored story. Drop it all
            // — only the story's suspend triple and the record-29
            // restart remain.
            self.vm.pending_dialog_dispatches.clear();
            self.vm.dialog_dispatch_base_depth = None;
            self.vm.dialog_dispatch_cursor = None;
            self.vm.engine_request_park = None;
            self.vm.dispatch_stopped = false;
            let rec = 0x33DCu32 + 29 * 28;
            let rec_pc = self
                .vm
                .interpreter_words
                .get(&(rec + 4))
                .copied()
                .unwrap_or(u32::MAX);
            let rec_frame = self.vm.interpreter_words.get(&rec).copied().unwrap_or(1);
            if rec_pc != u32::MAX {
                tracing::info!(
                    event = "cmvs.session.load_record29_pushed",
                    pc = rec_pc,
                    frame = rec_frame,
                    tick = self.vm.tick
                );
                self.vm.forced_dialog_record = Some((rec_frame, rec_pc, 1));
            }
        }
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
            self.apply_pending_load();
        }
        if self.vm.menu_pump_exit_seen {
            self.vm.menu_pump_exit_seen = false;
        }
        // The revived fade driver ramps the load's black down but its
        // final approach stalls near the floor (~0.05 — 95% black); the
        // native driver reaches fully open. Snap the residue once the
        // story cycle is live so the rebuilt scene shows.
        if self.vm.story_dispatch_armed && self.fade_level > 0.0 && self.fade_level <= 0.06 {
            self.fade_level = 0.0;
            self.last_fade_level = Some(0.0);
        }
        // The storm's last poll park sits between its scheduler disable
        // and re-enable; one engine request after the storm completes
        // the toggle and the scan resumes the render cycle.
        if let Some(end) = self.load_storm_end_tick {
            if self.vm.tick >= end {
                self.load_storm_end_tick = None;
                tracing::info!(event = "cmvs.session.load_cycle_pulse", tick = self.vm.tick);
                self.vm.engine_request_armed = true;
                self.vm.interpreter_words.insert(3500 * 4, 1);
                self.vm.interpreter_words.insert(3501 * 4, 1);
                self.vm.interpreter_words.insert(3502 * 4, 370_466);
                // Leave the round-robin scan armed as well: the native
                // main loop drives the idle poll from the enabled-mode
                // scan, not from one-shot requests.
                self.vm.interpreter_words.insert(0x33D4, 1);
                self.vm.interpreter_words.insert(0x29DC, 0);
            }
        }

        if self.vm.menu_capture_story {
            self.vm.menu_capture_story = false;
            let (scene_bindings, scene_geometry) = self.scene.bindings_snapshot();
            self.story_snapshot = Some(Box::new(StorySnapshot {
                state: snapshot_story_context(&self.vm),
                interpreter: self.vm.interpreter_words.clone(),
                process: self.vm.process_indexed_words.clone(),
                flags: self.vm.process_flag_bits.clone(),
                scene_bindings,
                scene_geometry,
                selected: self.selected,
                last_loaded_parent: self.last_loaded_parent,
                slot_objects: self.vm.slot_objects.clone(),
                texture_parents: self.vm.texture_parents.clone(),
                texture_children: self.vm.texture_children.clone(),
                retained_slot_placements: self.vm.retained_slot_placements.clone(),
            }));
            tracing::info!(
                event = "cmvs.session.story_snapshot_captured",
                tick = self.vm.tick
            );
        }
        // Native channel completion (`sub_48C0E0`): a bounded playback
        // lifecycle publishes the stop transition (+0xD0=0, +0xD4=1) the
        // element waiters poll. Expire armed channels and mark the
        // completion word.
        let now = self.vm.tick;
        let expired: Vec<u8> = self
            .vm
            .channel_completion_at
            .iter()
            .filter(|(_, at)| now >= **at)
            .map(|(ch, _)| *ch)
            .collect();
        for ch in expired {
            self.vm.channel_completion_at.remove(&ch);
            self.vm
                .interpreter_words
                .insert(742 * 4 + 0x100 + u32::from(ch), 1);
        }
        // Periodic idle snapshot (RE aid): every 600 ticks log the state
        // the story gate depends on, so a stall shows exactly which
        // latch stopped moving.
        if self.idle_probe.is_multiple_of(300) {
            tracing::info!(
                event = "cmvs.session.sched_gates",
                rebuild_gate = self.vm.interpreter_words.get(&0x29DC).copied().unwrap_or(0),
                sched_enabled = self.vm.interpreter_words.get(&0x33D4).copied().unwrap_or(0),
                tick = self.vm.tick
            );
        }
        self.idle_probe = self.idle_probe.wrapping_add(1);
        if self.idle_probe.is_multiple_of(600) {
            let channels: Vec<u8> = (0..8u32)
                .filter(|&ch| {
                    self.vm
                        .interpreter_words
                        .get(&(742 * 4 + 4 * ch))
                        .copied()
                        .unwrap_or(0)
                        != 0
                })
                .map(|ch| ch as u8)
                .collect();
            tracing::info!(
                event = "cmvs.session.idle_snapshot",
                frame0_pc = self.vm.program_counter,
                current_frame = self.vm.current_frame,
                word9 = self.vm.process_indexed_words.get(&9).copied().unwrap_or(0),
                rebuild_gate = self.vm.interpreter_words.get(&10716).copied().unwrap_or(0),
                scheduler_enabled = self.vm.interpreter_words.get(&0x33D4).copied().unwrap_or(0),
                word110 = self.vm.process_indexed_words.get(&110).copied().unwrap_or(0),
                word111 = self.vm.process_indexed_words.get(&111).copied().unwrap_or(0),
                pump_gate324 = self
                    .vm
                    .script_data_segment_words
                    .get(&1)
                    .and_then(|words| words.get(&324))
                    .copied()
                    .unwrap_or(0),
                msg81220 = self.vm.interpreter_words.get(&81220).copied().unwrap_or(0),
                live_records = ?(0..32u32)
                    .filter(|&slot| {
                        let base = 13276 + slot * 28;
                        let pc = self.vm.interpreter_words.get(&(base + 4)).copied();
                        matches!(pc, Some(v) if v != 0 && v != u32::MAX)
                    })
                    .map(|slot| {
                        let base = 13276 + slot * 28;
                        (
                            slot,
                            self.vm.interpreter_words.get(&base).copied().unwrap_or(0),
                            self.vm.interpreter_words.get(&(base + 4)).copied().unwrap_or(0),
                        )
                    })
                    .collect::<Vec<_>>(),
                // Every script frame's latest tick-end pc: the full map of
                // where each coroutine parked, so a stall names its waiter.
                frame_pcs = ?self
                    .vm
                    .last_frame_stop_pcs
                    .iter()
                    .collect::<Vec<_>>(),
                script_frames = ?self
                    .scripts
                    .iter()
                    .map(|(frame, script)| (frame, script.header.initial_pc, script.header.bytecode_size))
                    .collect::<Vec<_>>(),
                occupied_channels = ?channels,
            );
        }
        // The movie object mirrors VM occupancy: command 197 clears the VM
        // side, the session player stops its slot-2 audio here. While the
        // movie runs, `player->this[8]` (the audio-clock frame counter)
        // feeds command 198.
        if self.movie.is_some() && self.vm.movie_object.is_none() {
            if let (Some(mut player), Some(bridge)) = (self.movie.take(), self.audio.as_mut()) {
                let _ = player.close(bridge);
            }
            self.movie_stage = None;
        }
        if let (Some(player), Some(bridge)) = (self.movie.as_mut(), self.audio.as_ref()) {
            let target = player.current_frame(bridge);
            self.vm.movie_frame = target;
            if let Some(frame) = player.pump(target) {
                if self
                    .movie_stage
                    .as_ref()
                    .is_none_or(|(sequence, _)| *sequence != u64::from(target))
                {
                    self.movie_stage = Some((
                        u64::from(target),
                        TextureFrame {
                            width: frame.width,
                            height: frame.height,
                            rgba8: frame.rgba8.clone().into(),
                        },
                    ));
                    tracing::info!(
                        event = "cmvs.session.movie_frame_staged",
                        target,
                        width = frame.width,
                        height = frame.height,
                        first_pixel = ?frame.rgba8.get(0..4).map(<[u8]>::to_vec)
                    );
                }
            }
        }
        // The interpreter tick opens by advancing the ten wait-timer words
        // (`this[0x33AC..0x33D0]`) by the frame delta in milliseconds
        // (`0x46db10` adds the clock delta to every word before the queue
        // rebuild). The native frame delta is `1000 / fps` ms; the
        // nanosecond carry keeps the 60 Hz sum exact at 50 ms per 3 ticks.
        self.timer_carry_ns += 1_000_000_000;
        let frame_delta_ms = (self.timer_carry_ns / 60_000_000) as u32;
        self.timer_carry_ns %= 60_000_000;
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
                    .interpreter_words
                    .insert(field, value.wrapping_add(frame_delta_ms));
            }
            self.vm.session_clock_millis =
                self.vm.session_clock_millis.wrapping_add(frame_delta_ms);
        }
        self.vm.tick = self.vm.tick.wrapping_add(1);
        // Scheduler-transition window: keep a small ring of tick starts and
        // dump it when the enable word flips, plus ten following ticks — the
        // exact pc trajectory across the disable.
        {
            let enabled = self.vm.interpreter_words.get(&0x33D4).copied().unwrap_or(0) != 0;
            self.tick_ring.push((
                u32::try_from(self.vm.tick % 4_000_000_000).unwrap_or(0),
                self.vm.program_counter,
                self.vm.current_frame,
                u32::from(enabled),
            ));
            let excess = self.tick_ring.len().saturating_sub(24);
            if excess > 0 {
                self.tick_ring.drain(0..excess);
            }
            let flipped = self.prev_scheduler_enabled == Some(true) && !enabled;
            if flipped && tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    event = "cmvs.session.scheduler_flip_window",
                    ring = ?self.tick_ring
                );
                self.flip_trace_ticks = 12;
            }
            if self.flip_trace_ticks > 0 {
                self.flip_trace_ticks -= 1;
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        event = "cmvs.session.post_flip_tick",
                        tick = self.vm.tick,
                        pc = self.vm.program_counter,
                        frame = self.vm.current_frame
                    );
                }
            }
            self.prev_scheduler_enabled = Some(enabled);
        }
        // Change-gated: while the scheduler word stays off, name the pc the
        // dispatch keeps resuming — the frozen-waiter diagnostic.
        if self.vm.interpreter_words.get(&0x33D4).copied().unwrap_or(0) == 0
            && tracing::enabled!(tracing::Level::INFO)
        {
            let resume = (self.vm.program_counter, self.vm.current_frame);
            if self.last_frozen_resume != Some(resume) {
                self.last_frozen_resume = Some(resume);
                tracing::info!(
                    event = "cmvs.session.frozen_dispatch_resume",
                    pc = self.vm.program_counter,
                    frame = self.vm.current_frame,
                    tick = self.vm.tick
                );
            }
        }
        // The one-entry-per-frame cap already paces the poll-family
        // cascade; the overlay ring must still observe the republished
        // identity so the dialog rebuild (and its geometric confirm
        // queries) runs on the following engine frames.
        rebuild_frame_entry_queue(&mut self.vm).map_err(core)?;
        self.vm.dispatch_stopped = false;
        // Resume trace: the dispatch position this tick opens at. With the
        // scheduler disabled the refill is a no-op and this pc must be the
        // previous tick's wait position — a divergence names the code that
        // hijacked the suspension.
        if tracing::enabled!(tracing::Level::INFO) {
            let key = (self.vm.current_frame, self.vm.program_counter);
            if self.last_tick_resume != Some(key) {
                self.last_tick_resume = Some(key);
                tracing::info!(
                    event = "cmvs.session.tick_resume",
                    frame = self.vm.current_frame,
                    pc = self.vm.program_counter,
                    tick = self.vm.tick
                );
            }
        }
        // Bounded numeric-only failure context; never retain script text.
        let mut recent = [(0_u16, 0_u32, 0_u32, 0_u32); 64];
        let mut recent_index = 0_usize;
        // Frozen-scheduler tail trace: the last pcs of the tick, logged when
        // their content changes — pinpoints a self-restoring dispatch loop.
        let frozen_start = self.vm.interpreter_words.get(&0x33D4).copied().unwrap_or(0) == 0;
        let mut frozen_tail: Vec<(u16, u32)> = Vec::new();
        // While a dialog dispatch is in flight the native engine paces
        // one continuation per frame; cap the tick so the poll family
        // spreads across engine frames instead of interleaving over the
        // dispatch layout words.
        // A live dialog cascade refreshes the entry queue mid-tick the
        // way the native engine rebuilds it every frame, so the cascade
        // never overdraws one rebuild's pushes.
        let cascade_refresh = self.cascade_pace_ticks > 0
            || self.vm.dialog_dispatch_base_depth.is_some()
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
                if frozen_start
                    && !frozen_tail.is_empty()
                    && self.last_frozen_tail != frozen_tail
                    && tracing::enabled!(tracing::Level::INFO)
                {
                    self.last_frozen_tail = frozen_tail.clone();
                    tracing::info!(
                        event = "cmvs.session.frozen_tick_tail",
                        tail = ?frozen_tail,
                        tick = self.vm.tick
                    );
                }
                if tracing::enabled!(tracing::Level::DEBUG) {
                    let target = self.vm.interpreter_words.get(&0x7421_0020).copied();
                    if target != self.last_present_target_logged {
                        tracing::debug!(
                            event = "cmvs.session.present_target",
                            target = ?target,
                            selected = ?self.selected
                        );
                        self.last_present_target_logged = target;
                    }
                }
                tracing::trace!(
                    event = "cmvs.session.present",
                    tick = self.vm.tick,
                    frame = self.vm.current_frame,
                    selected = ?self.selected,
                    parents = self.vm.texture_parents.len(),
                    children = self
                        .vm
                        .texture_children
                        .values()
                        .map(BTreeMap::len)
                        .sum::<usize>(),
                    channels = self.vm.effect_channels.len(),
                    text_surfaces = self.vm.native_text.len(),
                    glyphs = self
                        .vm
                        .native_text
                        .values()
                        .map(|surface| surface.glyphs.len())
                        .sum::<usize>(),
                    movie = ?self.vm.movie_object
                );
                if self.idle_probe.is_multiple_of(600) {
                    tracing::info!(
                        event = "cmvs.session.render_state",
                        selected = ?self.selected,
                        last_parent = ?self.last_loaded_parent,
                        fade = self.fade_level,
                        bindings = self.scene.binding_count(),
                        parents = self.vm.texture_parents.len(),
                        has28 = self.vm.texture_parents.contains_key(&28),
                        tick = self.vm.tick
                    );
                }
                // A present target cleared by the program switch falls
                // back to the most recently loaded parent — the story's
                // standing scene.
                let effective_selected = if self.vm.story_dispatch_armed {
                    self.selected
                        .or(self.last_loaded_parent.map(CmvsTextureSlot::Parent))
                } else {
                    self.selected
                };
                if let Some(selected) = effective_selected {
                    // A cleared channel animation (`sub_467A50` level 0)
                    // removes the animated surface: the scene beneath —
                    // the most recently loaded parent — becomes visible.
                    let selected = if self.fade_level <= 0.01 {
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
                    let rendered = self.scene.render(
                        &self.vm,
                        selected,
                        &self.archive,
                        (logical.width, logical.height),
                        movie,
                    );
                    match rendered {
                        Ok(Some(frame)) => self.frame = frame,
                        Ok(None) => {}
                        Err(cause) => {
                            // The post-switch fallback present can hit a
                            // surface whose slot state died with the menu;
                            // the native present skips the layer instead
                            // of failing the frame.
                            tracing::info!(
                                event = "cmvs.session.render_fallback_skipped",
                                error = %cause,
                                tick = self.vm.tick
                            );
                        }
                    }
                } else if self.scene.has_text_renderer() {
                    let logical = self.scene.logical_extent().ok_or_else(|| {
                        error("SHINKI_EMU_CMVS_CANVAS", "native stage is not initialized")
                    })?;
                    self.frame = self
                        .scene
                        .render_text(&self.vm, logical.width, logical.height)
                        .map_err(core)?;
                }
                if tracing::enabled!(tracing::Level::INFO) && self.vm.tick < 2_000 {
                    tracing::info!(
                        event = "cmvs.session.tick_stop",
                        frame = self.vm.current_frame,
                        pc = self.vm.program_counter,
                        tick = self.vm.tick
                    );
                }
                return Ok(());
            }
            if tracing::enabled!(tracing::Level::INFO)
                && self.vm.tick < 2_000
                && self.last_dispatch_frame != Some(self.vm.current_frame)
            {
                self.last_dispatch_frame = Some(self.vm.current_frame);
                tracing::info!(
                    event = "cmvs.session.frame_switch",
                    frame = self.vm.current_frame,
                    pc = self.vm.program_counter,
                    tick = self.vm.tick
                );
            }
            // Word-324 state-machine watch: every frame-1 pc inside the
            // 422300-424500 dispatch/service corridor, change-gated — the
            // service chain's non-return names itself here.
            if tracing::enabled!(tracing::Level::INFO)
                && self.vm.tick < 2_000
                && self.vm.current_frame == 1
                && ((422_300..=424_500).contains(&self.vm.program_counter)
                    || self.vm.program_counter == 42_2392)
                && self.last_state_zone_pc != Some(self.vm.program_counter)
            {
                self.last_state_zone_pc = Some(self.vm.program_counter);
                tracing::info!(
                    event = "cmvs.session.state_zone_pc",
                    pc = self.vm.program_counter,
                    tick = self.vm.tick
                );
            }
            recent[recent_index] = (
                self.vm.current_frame,
                self.vm.program_counter,
                self.vm.stack_cursor_bytes,
                self.vm.call_frame_bases.last().copied().unwrap_or(0),
            );
            // Waiter-band trace: inside intproc's scenario-waiter region, log
            // the stack cursor per pc (change-gated) — a net-pop violation
            // shows as a persistent 4-byte drift across the pass.
            if tracing::enabled!(tracing::Level::INFO) && self.vm.tick.is_multiple_of(60) {
                if tracing::enabled!(tracing::Level::INFO) {
                    if let Some(starts) = self.vm.script_valid_starts.get(&self.vm.current_frame) {
                        if !starts.contains(&self.vm.program_counter)
                            && self
                                .misaligned_report
                                .insert((self.vm.current_frame, self.vm.program_counter))
                        {
                            tracing::info!(
                                event = "cmvs.session.misaligned_pc",
                                pc = self.vm.program_counter,
                                prev_pc = self.last_logged_pc,
                                prev_frame = self.last_logged_frame,
                                frame = self.vm.current_frame
                            );
                        }
                    }
                }
                let key = (
                    self.vm.current_frame,
                    self.vm.program_counter,
                    self.vm.stack_cursor_bytes,
                );
                if self.last_waiter_cursor3 != Some(key) {
                    self.last_waiter_cursor3 = Some(key);
                    tracing::info!(
                        event = "cmvs.session.waiter_cursor",
                        frame = self.vm.current_frame,
                        pc = self.vm.program_counter,
                        stack = self.vm.stack_cursor_bytes,
                        tick = u32::try_from(self.vm.tick % 4_000_000_000).unwrap_or(0)
                    );
                }
            }
            if frozen_start {
                frozen_tail.push((self.vm.current_frame, self.vm.program_counter));
                let excess = frozen_tail.len().saturating_sub(8);
                if excess > 0 {
                    frozen_tail.drain(0..excess);
                }
            }
            recent_index = (recent_index + 1) % recent.len();
            let script = self.scripts.get(&self.vm.current_frame).ok_or_else(|| {
                error("SHINKI_EMU_CMVS_SCRIPT_FRAME", "script frame is not loaded")
            })?;
            if tracing::enabled!(tracing::Level::INFO) {
                if let Some(starts) = self.vm.script_valid_starts.get(&self.vm.current_frame) {
                    if !starts.contains(&self.vm.program_counter)
                        && self
                            .misaligned_report
                            .insert((self.vm.current_frame, self.vm.program_counter))
                    {
                        tracing::info!(
                            event = "cmvs.session.misaligned_pc",
                            pc = self.vm.program_counter,
                            prev_pc = self.last_logged_pc,
                            prev_frame = self.last_logged_frame,
                            frame = self.vm.current_frame,
                            in_starts = starts.contains(&self.vm.program_counter)
                        );
                    }
                }
            }
            if tracing::enabled!(tracing::Level::INFO)
                && self.vm.current_frame == 2
                && (75_700..=75_980).contains(&self.vm.program_counter)
            {
                let raw = script
                    .bytecode
                    .get(self.vm.program_counter as usize..self.vm.program_counter as usize + 4)
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                    .unwrap_or(0);
                tracing::info!(
                    event = "cmvs.session.frame2_decode",
                    pc = self.vm.program_counter,
                    raw_word = raw,
                    tick = self.vm.tick
                );
            }
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(self.vm.program_counter, 2816 | 2840 | 3608)
            {
                tracing::info!(
                    event = "cmvs.session.write_pc_script",
                    pc = self.vm.program_counter,
                    frame = self.vm.current_frame,
                    script = self
                        .script_names
                        .get(&self.vm.current_frame)
                        .map(String::as_str)
                        .unwrap_or(&self.entry_name),
                    tick = self.vm.tick
                );
            }
            let instruction =
                frame_ps2a_instruction(script, self.vm.program_counter).map_err(|failure| {
                    // Top-of-stack words as raw u32s; no script text.
                    let top: Vec<u32> = (0..8_u32)
                        .filter_map(|index| {
                            let end = self.vm.stack_cursor_bytes.checked_sub(index * 4)?;
                            let start = end.checked_sub(4)?;
                            self.vm
                                .stack_bytes
                                .get(start as usize..end as usize)
                                .and_then(|bytes| Some(u32::from_le_bytes(bytes.try_into().ok()?)))
                        })
                        .collect();
                    tracing::error!(event = "cmvs.session.invalid_instruction",
                        next = recent_index, recent = ?recent, stack_top = ?top,
                        pc = self.vm.program_counter, frame = self.vm.current_frame,
                        cursor = self.vm.stack_cursor_bytes);
                    core(failure)
                })?;
            if tracing::enabled!(tracing::Level::INFO)
                && (370_000..=370_466).contains(&self.vm.program_counter)
            {
                let raw = script
                    .bytecode
                    .get(self.vm.program_counter as usize..self.vm.program_counter as usize + 4)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])))
                    .unwrap_or(0);
                tracing::info!(
                    event = "cmvs.session.zone_raw",
                    pc = self.vm.program_counter,
                    frame = self.vm.current_frame,
                    script = self
                        .script_names
                        .get(&self.vm.current_frame)
                        .map(String::as_str)
                        .unwrap_or("?"),
                    raw
                );
            }
            if self.vm.current_frame == 0
                && (200_400..200_700).contains(&self.vm.program_counter)
                && !self.entered_watch_range
            {
                self.entered_watch_range = true;
                tracing::info!(
                    event = "cmvs.session.range_entry",
                    pc = self.vm.program_counter,
                    prev_pc = self.last_logged_pc,
                    stack = self.vm.stack_cursor_bytes
                );
            }
            {
                let hotspot = self
                    .pc_hotspots
                    .entry((self.vm.current_frame, self.vm.program_counter))
                    .or_insert(0u32);
                *hotspot = hotspot.saturating_add(1);
                self.last_logged_frame = self.vm.current_frame;
                self.last_logged_pc = self.vm.program_counter;
                if *hotspot == 2000 {
                    tracing::error!(
                        event = "cmvs.session.pc_hotspot",
                        frame = self.vm.current_frame,
                        pc = self.vm.program_counter
                    );
                }
            }
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
        Err(error(
            "SHINKI_EMU_CMVS_STEP_BOUND",
            "instruction limit exceeded",
        ))
    }

    /// Native auto mode: when the story line-wait latch (process word 9,
    /// armed at intproc 70620 when the text surface is configured) is set,
    /// the engine's own advance consumes it without a physical click. The
    /// host synthesizes that click through the same input path a real key
    /// takes — press edge this tick, release the next — so the
    /// coroutine-resume chain (advance edge, channel record, cmd 138)
    /// behaves exactly as it does for physical clicks.
    fn pump_auto_advance(&mut self) {
        if !self.vm.input_auto_mode {
            return;
        }
        self.vm.input_synthetic = true;
        self.pump_auto_advance_inner();
        self.vm.input_synthetic = false;
    }
    fn pump_auto_advance_inner(&mut self) {
        let armed = self.vm.process_indexed_words.get(&9).copied().unwrap_or(0) != 0;
        let plain = KeyModifiers {
            shift: false,
            control: false,
            alt: false,
            super_key: false,
        };
        if self.auto_click_held {
            let release = RuntimeEvent::Key {
                code: KeyCode::Enter,
                state: KeyState::Released,
                modifiers: plain,
            };
            self.input.event(&mut self.vm, &release);
            self.auto_click_held = false;
        } else {
            if armed && !self.input.confirm_held() {
                let press = RuntimeEvent::Key {
                    code: KeyCode::Enter,
                    state: KeyState::Pressed,
                    modifiers: plain,
                };
                self.input.event(&mut self.vm, &press);
                self.auto_click_held = true;
            }
        }
    }
    fn advance_inner(
        &mut self,
        elapsed_ns: u64,
        events: &[RuntimeEvent],
    ) -> SessionResult<AdvanceResponse> {
        let mut changed = false;
        for event in events {
            self.input.event(&mut self.vm, event);
            if tracing::enabled!(tracing::Level::INFO)
                && matches!(event, RuntimeEvent::PointerButton { .. })
            {
                tracing::info!(
                    event = "cmvs.session.input_pointer_event",
                    tick = self.vm.tick,
                    confirm_after = self.input.confirm_held(),
                    release_armed = self.vm.input_advance_release
                );
            }
            match event {
                RuntimeEvent::WindowSuspended { suspended } => {
                    changed |= self.suspended != *suspended;
                    self.suspended = *suspended;
                }
                RuntimeEvent::WindowResized { width, height } => {
                    self.scene.resize_raster(*width, *height).map_err(core)?;
                }
                RuntimeEvent::PointerMove { x, y } => {
                    self.vm.pointer_x = *x as i32;
                    self.vm.pointer_y = *y as i32;
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
                    if self.input.menu_opened {
                        if let Some(quad) = self.vm.menu_quad_hit() {
                            self.vm.input_selection_mirror = quad;
                        }
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Released,
                } => {
                    // The native engine's Win32 menu handler posts the
                    // record-17 resume through the request dwords while
                    // the script scheduler is off (`sub_46DF10`'s disabled
                    // branch reads gate 3500 / frame 3501 / pc 3502).
                    if self.input.menu_opened {
                        self.vm.engine_request_armed = true;
                        self.vm.interpreter_words.insert(3500 * 4, 1);
                        self.vm.interpreter_words.insert(3501 * 4, 1);
                        self.vm.interpreter_words.insert(3502 * 4, 179_768);
                        // The engine's message pump reopens the entry-queue
                        // rebuild gate with each posted request — the ring's
                        // cmd137(0) close must not strand the dwords.
                        self.vm.interpreter_words.insert(0x29DC, 0);
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
                    }
                    // The native hit test resolves the release through the
                    // Win32 child-window tree (`WindowFromPoint` at
                    // `sub_411FC0`); the headless equivalent answers from
                    // the channel-child presenter rectangles and posts the
                    // hit identity through the message word the overlay
                    // ring polls.
                    let px = (i64::from(self.vm.pointer_x) * 3) / 2;
                    let py = (i64::from(self.vm.pointer_y) * 3) / 2;
                    let hits: Vec<(u32, i32)> = self
                        .vm
                        .channel_child_presenters
                        .iter()
                        .filter_map(|((ch, child), presenter)| {
                            let rect = presenter.rect?;
                            let left = i64::from(presenter.values[0] as i32 + rect[0]);
                            let top = i64::from(presenter.values[1] as i32 + rect[1]);
                            let right = left + i64::from(rect[2].max(0));
                            let bottom = top + i64::from(rect[3].max(0));
                            (px >= left && py >= top && px < right && py < bottom)
                                .then_some((*ch, *child))
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
                            let rect = self
                                .vm
                                .channel_child_presenters
                                .get(&(*ch, *child))
                                .and_then(|p| p.rect)
                                .unwrap_or([0, 0, i32::MAX, i32::MAX]);
                            (rect[2].max(0) as i64) * (rect[3].max(0) as i64)
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
                    let chain_rect_hit = self
                        .vm
                        .filter_chain_records
                        .values()
                        .flat_map(|records| records.values())
                        .any(|record| {
                            let [x, y, w, h] = record.words;
                            (w > 0 && h > 0)
                                && px >= i64::from(x)
                                && py >= i64::from(y)
                                && px < i64::from(x) + i64::from(w)
                                && py < i64::from(y) + i64::from(h)
                        });
                    if let Some((channel, child)) = dialog_hit.or(
                        if self.input.menu_opened
                            && self.input.menu_save_completed
                            && !chain_rect_hit
                            && self.vm.input_selection_mirror != 0
                        {
                            // A system-menu quad selection armed on this press
                            // owns the click: native's quad windows sit above
                            // the presenter overlay (but below the chain list).
                            None
                        } else {
                            overlay_hit
                        },
                    ) {
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
                        ) {
                            if handler_pc != u32::MAX {
                                self.vm.forced_dialog_record = Some((frame, handler_pc, id));
                            }
                        }
                        if tracing::enabled!(tracing::Level::INFO) {
                            let table_base = 0x33DC_u32;
                            let mut records: Vec<String> = Vec::new();
                            for slot in 0..64_u32 {
                                let base = table_base + slot * 28;
                                let flag = self.vm.interpreter_words.get(&(base - 4)).copied();
                                let frame = self.vm.interpreter_words.get(&base).copied();
                                let pc = self.vm.interpreter_words.get(&(base + 4)).copied();
                                if frame.is_some() || pc.is_some() {
                                    records
                                        .push(format!("{}:{:?}/{:?}/{:?}", slot, flag, frame, pc));
                                }
                            }
                            tracing::info!(
                                event = "cmvs.session.presenter_click",
                                tick = self.vm.tick,
                                channel,
                                child,
                                id
                            );
                        }
                    } else if self.input.menu_opened && self.vm.input_selection_mirror != 0 {
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
            self.pump_auto_advance();
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
            let base = bmp.len();
            bmp.extend_from_slice(&[
                (sums[2] / samples) as u8,
                (sums[1] / samples) as u8,
                (sums[0] / samples) as u8,
            ]);
            let _ = base;
        }
        bmp.resize((row + 1) * row_bytes, 0);
    }
    bmp
}

/// The story context captured at menu open: the save's interpreter
/// subset plus the engine's full word tables (`sub_475E40`'s restore
/// image — the menu's words die with its program at the switch).
struct StorySnapshot {
    state: crate::system_save::slot::CmvsSlotState,
    interpreter: std::collections::BTreeMap<u32, u32>,
    process: std::collections::BTreeMap<u32, u32>,
    flags: std::collections::BTreeSet<u32>,
    scene_bindings: std::collections::BTreeMap<CmvsTextureSlot, String>,
    scene_geometry: std::collections::BTreeMap<CmvsTextureSlot, (RectI, u32)>,
    selected: Option<CmvsTextureSlot>,
    last_loaded_parent: Option<u8>,
    slot_objects: std::collections::BTreeMap<u32, u64>,
    texture_parents: std::collections::BTreeMap<u8, crate::vm::CmvsTextureParentState>,
    texture_children: std::collections::BTreeMap<
        u8,
        std::collections::BTreeMap<u16, crate::vm::CmvsTextureChildState>,
    >,
    retained_slot_placements: std::collections::BTreeMap<u32, crate::vm::CmvsRetainedPlacement>,
}

/// Builds the interpreter state subset a save serializes from the live
/// VM — used for the menu-open story snapshot.
fn snapshot_story_context(vm: &CmvsPs2aVmState) -> crate::system_save::slot::CmvsSlotState {
    use crate::system_save::slot::CmvsSlotState;
    let mut records = Vec::with_capacity(64);
    for slot in 0..64_u32 {
        let mut row = [0_u32; 7];
        for field in 0..7_u32 {
            let default = if field == 2 { u32::MAX } else { 0 };
            row[field as usize] = vm
                .interpreter_words
                .get(&(0x33D8 + slot * 28 + field * 4))
                .copied()
                .unwrap_or(default);
        }
        records.push(row);
    }
    let mut stack = Vec::with_capacity(vm.stack_bytes.len() / 4);
    for chunk in vm.stack_bytes.as_chunks::<4>().0 {
        stack.push(u32::from_le_bytes(*chunk));
    }
    CmvsSlotState {
        suspend: Some((
            vm.interpreter_words.get(&0x33A8).copied().unwrap_or(0),
            vm.program_counter,
            u32::from(vm.current_frame),
        )),
        value_stack_cursor_hint: Some(vm.stack_cursor_bytes),
        frame_depth_hint: Some(vm.call_frame_bases.len() as u32),
        records,
        strings: Vec::new(),
        numbers: Vec::new(),
        value_stack: stack,
        frame_bases: vm.call_frame_bases.clone(),
        globals: [
            vm.interpreter_words.get(&640).copied(),
            vm.interpreter_words.get(&641).copied(),
            vm.interpreter_words.get(&642).copied(),
        ],
    }
}

#[cfg(test)]
pub(crate) mod tests;
