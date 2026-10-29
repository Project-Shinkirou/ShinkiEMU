use super::*;

impl ShinkiEmuManagerController {
    pub(super) fn set_appearance(
        &mut self,
        settings: shinki_emu_manager_core::AppearanceSettings,
    ) -> Result<ManagerViewModel, String> {
        settings.validate()?;
        self.library
            .save_appearance_settings(&settings)
            .map_err(|e| e.to_string())?;
        if settings.language != self.appearance.language {
            shinki_emu_manager_ui_slint::i18n::apply(&settings.language);
        }
        self.appearance = settings;
        self.model()
    }
}
