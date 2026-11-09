use super::*;
use shinki_emu_manager_ui_slint::ChoiceViewModel;

impl ShinkiEmuManagerController {
    pub(super) fn engine_label(&self, family_id: &str) -> String {
        family_id.to_owned()
    }

    pub(super) fn emulator_label(&self, plugin_id: &str) -> String {
        let name = shinki_emu_manager_core::static_core_catalog::lookup(plugin_id)
            .map_or(plugin_id, |entry| entry.display_name);
        let (version, identity) = self.core_release(plugin_id);
        let version = identity.map_or_else(|| version.to_owned(), shinki_emu_metadata::CoreIdentity::display_version);
        if version.is_empty() { name.to_owned() } else { format!("{name} · {version}") }
    }

    pub(super) fn appearance_view(&self) -> AppearanceViewModel {
        AppearanceViewModel {
            theme_mode: self.appearance.theme_mode.clone(),
            density: self.appearance.density.clone(),
            cover_size: self.appearance.cover_size.clone(),
            view_mode: self.appearance.view_mode.clone(),
            reduce_motion: self.appearance.reduce_motion,
            cover_badges: self.appearance.cover_badges,
            language: self.appearance.language.clone(),
        }
    }

    /// Emulator picker for the library detail: empty unless there is a real choice.
    pub(super) fn emulator_choices(&self, game_id: &str) -> Vec<ChoiceViewModel> {
        match self.probe_selection_for_game(game_id) {
            Some(choices) if choices.len() > 1 => choices
                .iter()
                .map(|choice| ChoiceViewModel {
                    id: choice.plugin_id.clone(),
                    label: self.emulator_label(&choice.plugin_id),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

pub(super) fn binding_views(
    mapping: &shinki_emu_manager_core::InputMapping,
) -> Vec<GamepadBindingViewModel> {
    GamepadInput::DISPLAY_ORDER
        .iter()
        .map(|button| GamepadBindingViewModel {
            id: button.as_str().into(),
            label: button.label().into(),
            key: mapping.gamepad.get(button).cloned().unwrap_or_default(),
        })
        .collect()
}
