use shinki_emu_sdk::runtime::{SessionResult, WindowCommand};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct WindowCommands(VecDeque<WindowCommand>);

impl WindowCommands {
    pub(super) fn submit(&mut self, command: WindowCommand) -> SessionResult<()> {
        command.validate()?;
        // Latest title/icon wins within its own property. The queue contains
        // at most one entry per typed property; an icon never erases a title.
        let kind = std::mem::discriminant(&command);
        self.0
            .retain(|pending| std::mem::discriminant(pending) != kind);
        self.0.push_back(command);
        Ok(())
    }

    pub(super) fn take(&mut self) -> Option<WindowCommand> {
        self.0.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shinki_emu_sdk::runtime::WindowIcon;

    #[test]
    fn titles_and_icons_survive_together_and_invalid_replacement_is_atomic() {
        let mut queue = WindowCommands::default();
        let title = WindowCommand::SetTitle("title".into());
        let icon = WindowCommand::SetIcon(WindowIcon {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        queue.submit(title.clone()).unwrap();
        queue.submit(icon.clone()).unwrap();
        assert!(queue
            .submit(WindowCommand::SetTitle("bad\0title".into()))
            .is_err());
        assert_eq!(queue.take(), Some(title));
        assert_eq!(queue.take(), Some(icon));
        assert!(queue.take().is_none());
        for _ in 0..1000 {
            queue
                .submit(WindowCommand::SetTitle("latest".into()))
                .unwrap();
        }
        assert_eq!(queue.0.len(), 1);
    }
}
