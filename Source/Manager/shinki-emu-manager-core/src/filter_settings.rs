use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub const FILTER_SOURCE_BYTE_LIMIT: usize = 1_048_576;
pub const FILTER_DEFAULT_STRENGTH: f32 = 0.35;

/// One user-visible stage. A preset is either one stage or the empty chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilterStage {
    Scale {
        scale: f32,
    },
    Sharpen {
        strength: f32,
    },
    LegacyAnime4k,
    Lanczos3 {
        scale: f32,
    },
    Scale2x,
    Anime4kOriginalRestore,
    Anime4kOriginalX2,
    Format4 {
        source: String,
        strength: f32,
        parameters: BTreeMap<String, f32>,
        /// Old settings could store a source while the filter was off.
        #[serde(default)]
        disabled: bool,
    },
}

/// Private Manager settings; shader source never enters a Family or save.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct FilterSettings {
    pub stages: Vec<FilterStage>,
}

impl FilterSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.stages.len() > 16
            || self.stages.iter().any(|stage| match stage {
                FilterStage::Scale { scale } | FilterStage::Lanczos3 { scale } => {
                    !(1.0..=4.0).contains(scale)
                }
                FilterStage::Sharpen { strength } => !(0.0..=1.0).contains(strength),
                FilterStage::Format4 {
                    source,
                    strength,
                    parameters,
                    ..
                } => {
                    source.is_empty()
                        || source.len() > FILTER_SOURCE_BYTE_LIMIT
                        || !(0.0..=1.0).contains(strength)
                        || !valid_parameters(parameters)
                }
                _ => false,
            })
        {
            return Err("SHINKI_EMU_FILTER_SETTINGS_INVALID".into());
        }
        Ok(())
    }
}

fn valid_parameters(parameters: &BTreeMap<String, f32>) -> bool {
    parameters.len() <= 64
        && parameters
            .iter()
            .all(|(key, value)| !key.is_empty() && key.len() <= 128 && value.is_finite())
}

impl<'de> Deserialize<'de> for FilterSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ordered {
            stages: Vec<FilterStage>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Legacy {
            configuration: LegacyConfiguration,
            source: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyConfiguration {
            preset: LegacyPreset,
            scale: f32,
            strength: f32,
            parameters: BTreeMap<String, f32>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum LegacyPreset {
            None,
            Scale,
            Sharpen,
            Anime4kRestoreUpscale,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Ordered(Ordered),
            Legacy(Legacy),
        }

        Ok(match Stored::deserialize(deserializer)? {
            Stored::Ordered(ordered) => Self {
                stages: ordered.stages,
            },
            Stored::Legacy(Legacy {
                configuration,
                source,
            }) => {
                let LegacyConfiguration {
                    preset,
                    scale,
                    strength,
                    parameters,
                } = configuration;
                if !(1.0..=4.0).contains(&scale)
                    || !(0.0..=1.0).contains(&strength)
                    || (matches!(preset, LegacyPreset::Anime4kRestoreUpscale) && scale != 2.0)
                    || !valid_parameters(&parameters)
                {
                    return Err(serde::de::Error::custom(
                        "SHINKI_EMU_FILTER_SETTINGS_INVALID",
                    ));
                }
                let stage = match (source, preset) {
                    (Some(source), preset) => Some(FilterStage::Format4 {
                        source,
                        strength,
                        parameters,
                        disabled: matches!(preset, LegacyPreset::None),
                    }),
                    (None, LegacyPreset::None) => None,
                    (None, LegacyPreset::Scale) => Some(FilterStage::Scale { scale }),
                    (None, LegacyPreset::Sharpen) => Some(FilterStage::Sharpen { strength }),
                    (None, LegacyPreset::Anime4kRestoreUpscale) => Some(FilterStage::LegacyAnime4k),
                };
                Self {
                    stages: stage.into_iter().collect(),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_json_migrates_without_activating_an_inactive_source() {
        for (preset, source, expected) in [
            (
                "scale",
                serde_json::Value::Null,
                vec![FilterStage::Scale { scale: 1.5 }],
            ),
            (
                "sharpen",
                serde_json::Value::Null,
                vec![FilterStage::Sharpen { strength: 0.7 }],
            ),
            (
                "anime4k_restore_upscale",
                serde_json::Value::Null,
                vec![FilterStage::LegacyAnime4k],
            ),
            (
                "anime4k_restore_upscale",
                "external".into(),
                vec![FilterStage::Format4 {
                    source: "external".into(),
                    strength: 0.7,
                    parameters: BTreeMap::new(),
                    disabled: false,
                }],
            ),
            (
                "none",
                "external".into(),
                vec![FilterStage::Format4 {
                    source: "external".into(),
                    strength: 0.7,
                    parameters: BTreeMap::new(),
                    disabled: true,
                }],
            ),
        ] {
            let old = serde_json::json!({
                "configuration": {
                    "preset": preset,
                    "scale": if preset == "anime4k_restore_upscale" { 2.0 } else { 1.5 },
                    "strength": 0.7,
                    "parameters": {}
                },
                "source": source
            });
            let settings: FilterSettings = serde_json::from_value(old).unwrap();
            settings.validate().unwrap();
            assert_eq!(settings.stages, expected);
            let new = serde_json::to_value(&settings).unwrap();
            assert!(new.get("configuration").is_none());
            assert_eq!(
                serde_json::from_value::<FilterSettings>(new).unwrap(),
                settings
            );
        }
    }
}
