use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppearanceSettings {
    pub theme_mode: String,
    pub density: String,
    pub cover_size: String,
    pub view_mode: String,
    pub reduce_motion: bool,
    pub cover_badges: bool,
    /// "system" follows the OS locale; otherwise "zh-CN", "en" or "ja".
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "system".into()
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_mode: "system".into(),
            density: "comfortable".into(),
            cover_size: "medium".into(),
            view_mode: "grid".into(),
            reduce_motion: false,
            cover_badges: true,
            language: default_language(),
        }
    }
}

impl AppearanceSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.theme_mode.as_str(), "system" | "light" | "dark")
            || !matches!(self.density.as_str(), "comfortable" | "compact")
            || !matches!(self.cover_size.as_str(), "large" | "medium" | "small")
            || !matches!(self.view_mode.as_str(), "grid" | "list")
            || !matches!(self.language.as_str(), "system" | "zh-CN" | "en" | "ja")
        {
            return Err("SHINKI_EMU_APPEARANCE_INVALID".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppearanceSettings;

    #[test]
    fn settings_saved_before_language_existed_follow_the_system() {
        let saved = r#"{"theme_mode":"dark","density":"compact","cover_size":"small","view_mode":"list","reduce_motion":true,"cover_badges":false}"#;
        let settings: AppearanceSettings = serde_json::from_str(saved).unwrap();
        assert_eq!(settings.language, "system");
        assert_eq!(settings.theme_mode, "dark");
        assert!(settings.validate().is_ok());
        let invalid = AppearanceSettings {
            language: "fr".into(),
            ..settings
        };
        assert!(invalid.validate().is_err());
    }
}
