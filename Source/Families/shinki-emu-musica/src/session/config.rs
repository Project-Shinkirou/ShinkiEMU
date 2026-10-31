use super::*;
use crate::{MusicaConfigChange, MusicaConfigControl, MusicaSystemPage};
impl MusicaSession {
    /// Persists `sysparams.txt` `quickSaveFileNumber`, then advances the cursor.
    pub(super) fn store_quick_cursor(&mut self, next: u32) -> SessionResult<()> {
        check_cursor(next)?;
        self.native.store_quick_cursor(next)?;
        self.quick_cursor = next;
        Ok(())
    }

    /// Persists `sysparams.txt` `autoSaveFileNumber`, then advances the cursor.
    pub(super) fn store_auto_cursor(&mut self, next: u32) -> SessionResult<()> {
        check_cursor(next)?;
        self.native.store_auto_cursor(next)?;
        self.auto_cursor = next;
        Ok(())
    }

    pub(super) fn config_event(&mut self, event: &RuntimeEvent) -> SessionResult<bool> {
        if self.vm.state().system_ui.page != MusicaSystemPage::Config {
            if matches!(
                event,
                RuntimeEvent::Key {
                    code: KeyCode::F7,
                    state: KeyState::Pressed,
                    ..
                }
            ) && self.vm.state().system_ui.page == MusicaSystemPage::None
                && self.vm.state().launch_mode == crate::MusicaLaunchMode::Title
                && self.vm.state().wait.is_some()
            {
                self.vm.open_config().map_err(vm_error)?;
                self.clear_input();
                return Ok(true);
            }
            return Ok(false);
        }
        let control = match event {
            RuntimeEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => Some(MusicaConfigControl::Apply),
            RuntimeEvent::Key {
                code: KeyCode::Escape,
                state: KeyState::Pressed,
                ..
            } => Some(MusicaConfigControl::Cancel),
            RuntimeEvent::PointerMove { x, y } => {
                self.pointer = Some((*x, *y));
                if self.config_pointer_down {
                    crate::configuration::config_control_at(*x as i32, *y as i32).filter(
                        |control| {
                            matches!(
                                control,
                                MusicaConfigControl::MessageSpeedUnread(_)
                                    | MusicaConfigControl::MessageSpeedRead(_)
                                    | MusicaConfigControl::MessageSpeedAutoPlay(_)
                                    | MusicaConfigControl::BgmVolume(_)
                                    | MusicaConfigControl::VoiceVolume(_)
                                    | MusicaConfigControl::SeVolume(_)
                            )
                        },
                    )
                } else {
                    None
                }
            }
            RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state,
            } => {
                self.config_pointer_down = *state == KeyState::Pressed;
                if self.config_pointer_down {
                    self.pointer.and_then(|(x, y)| {
                        crate::configuration::config_control_at(x as i32, y as i32)
                    })
                } else {
                    None
                }
            }
            RuntimeEvent::WindowFocused { .. }
            | RuntimeEvent::WindowResized { .. }
            | RuntimeEvent::WindowVisibility { .. }
            | RuntimeEvent::WindowSuspended { .. } => return Ok(false),
            _ => None,
        };
        let Some(control) = control else {
            return Ok(true);
        };
        if control == MusicaConfigControl::Apply {
            let config = self.vm.config_for_presentation().map_err(vm_error)?;
            self.native.store_configuration(config)?;
            if config.fullscreen != self.vm.config().fullscreen {
                self.pending_fullscreen = Some(config.fullscreen);
            }
        }
        match self.vm.apply_config_control(control).map_err(vm_error)? {
            MusicaConfigChange::Present => {}
            MusicaConfigChange::AudioParamsChanged => self.audio.set_preferences(
                self.vm
                    .config_for_presentation()
                    .map_err(vm_error)?
                    .audio_preferences(),
            )?,
            MusicaConfigChange::TestAudio(bus) => {
                let commands = self.vm.config_test_audio(bus).map_err(vm_error)?;
                self.audio.apply(commands)?;
            }
            MusicaConfigChange::Applied | MusicaConfigChange::Cancelled => {
                let commands = self.vm.stop_config_tests().map_err(vm_error)?;
                self.audio.apply(commands)?;
                self.audio
                    .set_preferences(self.vm.config().audio_preferences())?;
                self.scene.set_text_shadow(self.vm.config().text_shadow);
                self.progress_in_background = self.vm.config().progress_in_background;
                self.update_pause()?;
                self.title_focus = None;
                self.clear_input();
            }
        }
        tracing::debug!(event="shinki.emu.musica.config.input",page=?self.vm.state().system_ui.page);
        Ok(true)
    }
}

fn check_cursor(next: u32) -> SessionResult<()> {
    if next >= crate::storage::SAVE_PAGE_WIDTH {
        return Err(error(
            "SHINKI_EMU_MUSICA_QUICK_CURSOR",
            "save cursor is outside the save page",
        ));
    }
    Ok(())
}
