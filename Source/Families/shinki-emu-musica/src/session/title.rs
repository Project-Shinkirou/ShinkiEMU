use super::*;
use crate::MusicaSystemPage;
impl MusicaSession {
    pub(super) fn start_menu_script(&mut self, uri: String) -> SessionResult<()> {
        let loaded = crate::script_loader::load_script(&self.archive, &uri, self.primary_encoding)?;
        let generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        self.cancel_text()?;
        self.audio.restore(Vec::new())?;
        self.vm
            .start_menu_script(uri, loaded.hash, loaded.script)
            .map_err(vm_error)?;
        self.sync_read_flags()?;
        self.scene
            .set_text_encoding(self.vm.state().script_encoding);
        self.generation = generation;
        self.message = None;
        self.voice_duration = None;
        self.phase = 0;
        self.wait_ns = 0;
        self.title_focus = None;
        self.last_quick_save_pc_line = None;
        self.clear_input();
        tracing::info!(event = "shinki.emu.musica.title.start");
        self.reset_host_clock = true;
        Ok(())
    }
    fn activate_title(&mut self) -> SessionResult<()> {
        use crate::runtime::title::TitleAction;
        let action = self
            .vm
            .title()
            .map_err(vm_error)?
            .buttons
            .get(self.vm.state().system_ui.focus_index as usize)
            .ok_or_else(|| error("SHINKI_EMU_MUSICA_TITLE_FOCUS", "invalid title focus"))?
            .action;
        match action {
            TitleAction::NewGame => self.start_menu_script(self.entry_uri.clone()),
            TitleAction::Load => {
                self.vm.open_title_load().map_err(vm_error)?;
                self.load_from_title = true;
                self.clear_input();
                self.refresh_save_cards()
            }
            TitleAction::Config => {
                self.vm.open_config().map_err(vm_error)?;
                self.clear_input();
                Ok(())
            }
            TitleAction::Memories => self
                .vm
                .set_gallery_page(MusicaSystemPage::Memories, 0)
                .map_err(vm_error),
            TitleAction::Exit => {
                self.finished = true;
                Ok(())
            }
        }
    }
    pub(super) fn title_event(&mut self, event: &RuntimeEvent) -> SessionResult<bool> {
        if self.vm.state().system_ui.page != MusicaSystemPage::Title {
            return Ok(false);
        }
        match event {
            RuntimeEvent::Key {
                code: KeyCode::ArrowUp | KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => {
                let count = self.vm.title().map_err(vm_error)?.buttons.len() as u32;
                let offset = if matches!(
                    event,
                    RuntimeEvent::Key {
                        code: KeyCode::ArrowUp,
                        ..
                    }
                ) {
                    count - 1
                } else {
                    1
                };
                let focus = (self.vm.state().system_ui.focus_index + offset) % count;
                self.vm.set_title_focus(focus).map_err(vm_error)?;
                self.title_focus = Some(focus);
            }
            RuntimeEvent::Key {
                code: KeyCode::Enter | KeyCode::Space,
                state: KeyState::Pressed,
                ..
            } => self.activate_title()?,
            RuntimeEvent::PointerMove { x, y } => {
                self.pointer = Some((*x, *y));
                self.title_focus =
                    crate::runtime::title::focus_at(self.vm.title().map_err(vm_error)?, *x, *y);
                if let Some(focus) = self.title_focus {
                    self.vm.set_title_focus(focus).map_err(vm_error)?;
                }
            }
            RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                if let Some((x, y)) = self.pointer
                    && let Some(focus) =
                        crate::runtime::title::focus_at(self.vm.title().map_err(vm_error)?, x, y)
                {
                    self.vm.set_title_focus(focus).map_err(vm_error)?;
                    self.activate_title()?;
                }
            }
            RuntimeEvent::WindowFocused { .. }
            | RuntimeEvent::WindowResized { .. }
            | RuntimeEvent::WindowVisibility { .. }
            | RuntimeEvent::WindowSuspended { .. } => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
