use super::*;
impl MusicaSession {
    pub(super) fn tick(&mut self) -> SessionResult<bool> {
        if self.pending.is_some() {
            if self.vm.fast_forward_active() {
                return self.skip_translation();
            }
            let event = self.semantic_event(None)?;
            if event.is_none() {
                let tick =
                    self.vm.state().fixed_tick.checked_add(1).ok_or_else(|| {
                        error("SHINKI_EMU_MUSICA_TICK", "tick counter overflowed")
                    })?;
                self.vm.advance_waiting_tick(tick).map_err(vm_error)?;
            }
            return self.apply_event(event);
        }
        let visual_completion = matches!(
            self.vm.state().wait,
            Some(
                MusicaWaitState::CharacterTransition { .. }
                    | MusicaWaitState::LinearScroll { .. }
                    | MusicaWaitState::AxisScroll { .. }
            )
        );
        let tick = self
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .ok_or_else(|| error("SHINKI_EMU_MUSICA_TICK", "tick counter overflowed"))?;
        self.poll_voice_duration()?;
        if let Some(wait) = self.vm.state().wait.clone() {
            let delta = ((u128::from(tick) * 1_000_000_000 / 60)
                - (u128::from(tick - 1) * 1_000_000_000 / 60)) as u64;
            self.wait_ns = self.wait_ns.saturating_add(delta);
            let (id, ready) = match wait {
                MusicaWaitState::Voice {
                    token_id,
                    milliseconds,
                    ..
                } => (
                    token_id,
                    self.pending.is_none()
                        && (self.vm.fast_forward_active()
                            || milliseconds.is_some_and(|duration| {
                                self.wait_ns >= u64::from(duration) * 1_000_000
                            })),
                ),
                MusicaWaitState::Input { token_id } => (
                    token_id,
                    (self.input_pending
                        || (self.vm.state().choice.is_none()
                            && self.vm.fast_forward_active()
                            && self.wait_ns >= 10_000_000))
                        && self.pending.is_none(),
                ),
                MusicaWaitState::Time {
                    token_id,
                    milliseconds,
                    ..
                } => (
                    token_id,
                    self.pending.is_none()
                        && (self.vm.fast_forward_active()
                            || self.wait_ns >= u64::from(milliseconds) * 1_000_000),
                ),
                MusicaWaitState::CharacterTransition {
                    token_id,
                    slot_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .characters
                            .get(&slot_id)
                            .and_then(|character| character.transition.as_ref())
                            .is_some_and(|transition| transition.completed),
                ),
                MusicaWaitState::LinearScroll {
                    token_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .linear_scroll
                            .as_ref()
                            .is_some_and(|scroll| scroll.completed),
                ),
                MusicaWaitState::AxisScroll {
                    token_id,
                    milliseconds,
                } => (
                    token_id,
                    (!self.vm.config().animation
                        && self.wait_ns >= u64::from(milliseconds) * 1_000_000)
                        || self
                            .vm
                            .state()
                            .axis_scroll
                            .as_ref()
                            .is_some_and(|scroll| scroll.completed),
                ),
                MusicaWaitState::Presentation { token_id, .. } => (token_id, true),
                MusicaWaitState::Media { token_id, .. } => (token_id, false),
                MusicaWaitState::Provider { .. } => {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_WAIT_UNSUPPORTED",
                        "script requires an unimplemented media or provider wait",
                    ));
                }
            };
            if ready {
                if self.vm.state().choice.is_some() {
                    self.vm.commit_choice().map_err(vm_error)?;
                } else {
                    self.vm.resolve_wait(&id).map_err(vm_error)?;
                }
                self.wait_ns = 0;
                self.input_pending = false;
            } else {
                self.vm.advance_waiting_tick(tick).map_err(vm_error)?;
                return Ok(false);
            }
        }
        let event = self.vm.step(tick).map_err(vm_error)?;
        if let Some(checkpoint) = self.vm.take_native_autosave() {
            self.auto_save(checkpoint)?;
        }
        let event = self.semantic_event(event)?;
        let changed = self.apply_event(event)?;
        Ok(changed || visual_completion)
    }

    pub(super) fn apply_event(&mut self, event: Option<MusicaVmEvent>) -> SessionResult<bool> {
        let changed: SessionResult<bool> = match event {
            Some(MusicaVmEvent::Movie(state)) => {
                if (state.width, state.height)
                    != (self.info.logical_width, self.info.logical_height)
                {
                    return Err(error(
                        "SHINKI_EMU_MUSICA_MOVIE_STAGE_IDENTITY",
                        "movie dimensions do not match the stage",
                    ));
                }
                self.movie = Some(crate::movie::Movie::open(&self.archive, &state)?);
                #[cfg(feature = "ffmpeg-vcpkg")]
                self.scene.begin_movie(&state.resource_uri)?;
                Ok(false)
            }
            Some(MusicaVmEvent::Choice) => {
                self.message = None;
                Ok(true)
            }
            Some(MusicaVmEvent::Message {
                text,
                speaker,
                audio_commands,
                ..
            }) => {
                self.voice_duration = None;
                self.audio.apply(audio_commands)?;
                self.poll_voice_duration()?;
                self.wait_ns = 0;
                self.message(text, speaker)?;
                Ok(true)
            }
            Some(MusicaVmEvent::Wait(_)) => {
                self.wait_ns = 0;
                Ok(false)
            }
            Some(MusicaVmEvent::Audio { commands }) => {
                self.audio.apply(commands)?;
                Ok(false)
            }
            Some(MusicaVmEvent::Chain { target }) => {
                let (file, label) =
                    crate::script::chain_target_parts(&target).ok_or_else(|| {
                        error(
                            "SHINKI_EMU_MUSICA_RUNTIME_CHAIN",
                            "invalid chained script location",
                        )
                    })?;
                let uri = format!("musica:/scr/{file}");
                let loaded =
                    crate::script_loader::load_script(&self.archive, &uri, self.primary_encoding)?;
                let encoding = loaded.script.encoding;
                self.vm
                    .replace_script(uri, loaded.hash, loaded.script, label)
                    .map_err(vm_error)?;
                self.scene.set_text_encoding(encoding);
                self.sync_read_flags()?;
                Ok(false)
            }
            Some(MusicaVmEvent::Stage(stage)) => {
                self.record_cg_views(&stage)?;
                self.attach_stage_animations(&stage)?;
                Ok(true)
            }
            Some(
                MusicaVmEvent::Effect(_)
                | MusicaVmEvent::EffectCleared
                | MusicaVmEvent::Character(_)
                | MusicaVmEvent::Firefly(_)
                | MusicaVmEvent::FireflyCleared { .. }
                | MusicaVmEvent::SecondaryEffect(_)
                | MusicaVmEvent::SecondaryEffectCleared { .. }
                | MusicaVmEvent::WScroll2(_)
                | MusicaVmEvent::ScrollXf(_)
                | MusicaVmEvent::LinearScroll(_)
                | MusicaVmEvent::AxisScroll(_)
                | MusicaVmEvent::ScreenShake(_)
                | MusicaVmEvent::Panel { .. },
            ) => Ok(true),
            Some(MusicaVmEvent::Terminal) => {
                self.cancel_text()?;
                if self.vm.state().system_ui.page == crate::MusicaSystemPage::Title {
                    self.audio.restore(Vec::new())?;
                    self.message = None;
                    self.title_focus = None;
                    self.clear_input();
                    Ok(true)
                } else {
                    self.finished = true;
                    Ok(false)
                }
            }
            None => Ok(false),
        };
        changed
    }
}

impl MusicaSession {
    /// Probes same-name SQZ/ANI companions for every resource the stage
    /// command references and publishes the native animation state to the VM.
    pub(super) fn attach_stage_animations(
        &mut self,
        stage: &crate::MusicaStageCommand,
    ) -> SessionResult<()> {
        let mut animations = std::collections::BTreeMap::new();
        let candidates = self.stage_resource_candidates(stage);
        for &uri in &candidates {
            let Some((stem, extension)) = uri.rsplit_once('.') else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("png") {
                continue;
            }
            let companion = format!("{stem}.sqz");
            let Ok(stat) = self.archive.stat(&companion) else {
                continue;
            };
            if stat.size < 20 || stat.size > crate::MAX_ENTRY_BYTES {
                continue;
            }
            let header = self
                .archive
                .read_range(&companion, 0, 20)
                .map_err(crate::scene::core_error)?;
            let pair_count = crate::MusicaSqzArchive::header_pair_count(&header.bytes)
                .map_err(crate::scene::core_error)?;
            let interval_ns = 1_000_000_000u64
                .checked_div(u64::from(pair_count))
                .ok_or_else(|| {
                    error(
                        "SHINKI_EMU_MUSICA_SQZ_ANIMATION_TIMING",
                        "SQZ animation timing is invalid",
                    )
                })?;
            let frame_count = pair_count.checked_mul(2).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_SQZ_ANIMATION_TIMING",
                    "SQZ animation frame count overflows",
                )
            })?;
            animations.insert(
                uri.to_owned(),
                crate::MusicaImageAnimationState {
                    frame_index: 0,
                    frame_count,
                    interval_ns,
                    elapsed_ns: 0,
                },
            );
        }
        self.vm
            .attach_image_animations(animations)
            .map_err(vm_error)?;
        let mut patches = std::collections::BTreeMap::new();
        for uri in candidates {
            let Some((stem, extension)) = uri.rsplit_once('.') else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("png") {
                continue;
            }
            let companion = format!("{stem}.ani");
            let Ok(stat) = self.archive.stat(&companion) else {
                continue;
            };
            if stat.size < 12 || stat.size > crate::MAX_ENTRY_BYTES {
                continue;
            }
            let archive = self.scene.ani_source(&companion)?;
            let mut names: Vec<String> = Vec::new();
            for frame in archive.frames() {
                let Some(head) = frame.name.split(' ').next() else {
                    continue;
                };
                if head != frame.name && !names.iter().any(|known| known == head) {
                    names.push(head.to_owned());
                }
            }
            if names.is_empty() {
                continue;
            }
            // The native generator advances modulo 2^64, just like the
            // eye walker; a regular session seed must not fail on overflow.
            let seed = self
                .vm
                .state()
                .random_state
                .wrapping_add(archive.frames().len() as u64)
                .wrapping_mul(6364136223846793005);
            patches.insert(uri.to_owned(), (names, seed));
        }
        self.vm.attach_ani_patches(patches).map_err(vm_error)?;
        Ok(())
    }
    /// Every `.stage` image from the bg archive goes into `cgv.dat` as a
    /// lowercase file name, as in the real eden/Perseus files and ReMinori's
    /// stage hook.
    fn record_cg_views(&mut self, stage: &crate::MusicaStageCommand) -> SessionResult<()> {
        let names = stage
            .foreground
            .uris_in_order()
            .into_iter()
            .chain(
                stage
                    .background
                    .as_ref()
                    .map(|layer| layer.resource_uri.as_str()),
            )
            .filter_map(|uri| uri.strip_prefix("musica:/bg/"))
            .map(str::to_ascii_lowercase);
        for name in names {
            self.native.record_cg(&name)?;
        }
        Ok(())
    }

    fn stage_resource_candidates<'a>(&self, stage: &'a crate::MusicaStageCommand) -> Vec<&'a str> {
        let mut candidates: Vec<&str> = stage.foreground.uris_in_order();
        candidates.extend(
            stage
                .background
                .as_ref()
                .map(|layer| layer.resource_uri.as_str()),
        );
        candidates.extend(stage.stands.iter().map(|stand| stand.resource_uri.as_str()));
        candidates.sort_unstable();
        candidates.dedup();
        candidates
    }
}
