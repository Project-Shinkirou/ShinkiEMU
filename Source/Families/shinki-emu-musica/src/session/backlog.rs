use super::*;
use crate::MusicaSystemPage;

impl MusicaSession {
    pub(super) fn backlog_event(&mut self, event: &RuntimeEvent) -> SessionResult<bool> {
        let active = self.vm.state().system_ui.page == MusicaSystemPage::Backlog;
        if !active {
            if matches!(
                event,
                RuntimeEvent::Key {
                    code: KeyCode::PageUp,
                    state: KeyState::Pressed,
                    ..
                }
            ) && !self.vm.state().backlog.is_empty()
                && !self.finished
            {
                self.cancel_text()?;
                self.vm.open_backlog().map_err(vm_error)?;
                self.input_pending = false;
                tracing::debug!(
                    event = "shinki.emu.musica.backlog.opened",
                    entries = self.vm.state().backlog.len()
                );
                return Ok(true);
            }
            return Ok(false);
        }
        match event {
            RuntimeEvent::Key {
                code: KeyCode::Escape | KeyCode::Backspace,
                state: KeyState::Pressed,
                ..
            } => {
                self.vm.close_backlog().map_err(vm_error)?;
                self.input_pending = false;
                tracing::debug!(event = "shinki.emu.musica.backlog.closed");
            }
            RuntimeEvent::Key {
                code: KeyCode::PageUp | KeyCode::ArrowUp,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_backlog(-1).map_err(vm_error)?,
            RuntimeEvent::Key {
                code: KeyCode::PageDown | KeyCode::ArrowDown,
                state: KeyState::Pressed,
                ..
            } => self.vm.move_backlog(1).map_err(vm_error)?,
            RuntimeEvent::Key {
                code: KeyCode::Enter,
                state: KeyState::Pressed,
                ..
            }
            | RuntimeEvent::PointerButton {
                button: PointerButton::Primary,
                state: KeyState::Pressed,
            } => {
                let commands = self.vm.replay_backlog_voice().map_err(vm_error)?;
                self.audio.apply(commands)?;
                tracing::debug!(event = "shinki.emu.musica.backlog.voice");
            }
            RuntimeEvent::WindowFocused { .. }
            | RuntimeEvent::WindowResized { .. }
            | RuntimeEvent::WindowVisibility { .. }
            | RuntimeEvent::WindowSuspended { .. }
            | RuntimeEvent::Key {
                code: KeyCode::F5 | KeyCode::F9 | KeyCode::ControlLeft | KeyCode::ControlRight,
                ..
            } => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
