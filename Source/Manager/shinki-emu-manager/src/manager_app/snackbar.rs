use super::*;
use shinki_emu_manager::SnackbarOutcome;
use shinki_emu_manager_ui_slint::SnackbarViewModel;
use shinki_emu_manager_ui_slint::tr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SnackbarAction {
    Undo,
    Restart,
    AddUnrecognized(probe_worker::UnrecognizedRoot),
}

impl SnackbarAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Undo => tr!("撤销"),
            Self::Restart => tr!("立即重启"),
            Self::AddUnrecognized(_) => tr!("作为游戏添加"),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Snackbar {
    serial: u32,
    text: String,
    action: Option<SnackbarAction>,
}

impl Snackbar {
    pub(super) fn view(&self) -> SnackbarViewModel {
        SnackbarViewModel {
            serial: self.serial,
            text: self.text.clone(),
            action: self
                .action
                .as_ref()
                .map(SnackbarAction::label)
                .unwrap_or_default()
                .into(),
        }
    }
}

impl ShinkiEmuManagerController {
    pub(super) fn show_snackbar(
        &mut self,
        text: impl Into<String>,
        action: Option<SnackbarAction>,
    ) {
        self.snackbar.serial = self.snackbar.serial.wrapping_add(1);
        self.snackbar.text = text.into();
        self.snackbar.action = action;
    }

    pub(super) fn clear_unrecognized_offer(&mut self, requested: Option<&Path>) {
        if let Some(SnackbarAction::AddUnrecognized(root)) = &self.snackbar.action
            && requested.is_none_or(|path| path == root.requested)
        {
            self.show_snackbar("", None);
        }
    }

    pub(super) fn snackbar_action(&mut self) -> Result<SnackbarOutcome, String> {
        match self.snackbar.action.take() {
            Some(SnackbarAction::Undo) => self
                .undo_remove()
                .map(|model| SnackbarOutcome::Model(Box::new(model))),
            Some(SnackbarAction::Restart) => Ok(SnackbarOutcome::Restart),
            Some(SnackbarAction::AddUnrecognized(root)) => {
                self.show_snackbar("", None);
                self.admit_unrecognized_root(root).map(|model| SnackbarOutcome::Model(Box::new(model)))
            }
            None => Err("SHINKI_EMU_SNACKBAR_ACTION_UNAVAILABLE".into()),
        }
    }
}
