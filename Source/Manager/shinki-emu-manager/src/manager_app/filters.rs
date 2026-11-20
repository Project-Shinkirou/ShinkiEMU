use super::*;
use shinki_emu_manager::{FilterApply, FilterApplyState, FilterChainEdit, effects::format4::EffectSource};
use shinki_emu_manager_core::{
    FILTER_DEFAULT_STRENGTH, FILTER_SOURCE_BYTE_LIMIT, FilterSettings, FilterStage,
};
use shinki_emu_manager_ui_slint::tr;

const UNKNOWN_FIELD: &str = "SHINKI_EMU_FILTER_CONFIG_FIELD_UNKNOWN";
const INVALID_STAGE: &str = "SHINKI_EMU_FILTER_STAGE_INDEX";

impl ShinkiEmuManagerController {
    pub(super) fn filter_fields(&self) -> Result<Vec<ConfigFieldViewModel>, String> {
        let Some(stage) = self.filter_settings.stages.get(self.filter_selected_stage) else {
            return Ok(Vec::new());
        };
        let field = |key: &str, label: &str, value: f32, range: (f32, f32, f32)| {
            ConfigFieldViewModel {
                key: key.into(),
                label: label.into(),
                kind: "float".into(),
                value: value.to_string(),
                display_value: value.to_string(),
                min: range.0.into(),
                max: range.1.into(),
                step: range.2.into(),
                ..ConfigFieldViewModel::default()
            }
        };
        let strength_field = |strength| {
            field("filter.strength", tr!("锐化强度"), strength, (0.0, 1.0, 0.05))
        };
        Ok(match stage {
            FilterStage::Scale { scale } | FilterStage::Lanczos3 { scale } => {
                vec![field("filter.scale", tr!("缩放倍率"), *scale, (1.0, 4.0, 0.25))]
            }
            FilterStage::Sharpen { strength } => vec![strength_field(*strength)],
            FilterStage::Format4 { source, strength, parameters, .. } => {
                let effect = EffectSource::parse(source).map_err(|error| error.to_string())?;
                let mut fields = Vec::with_capacity(1 + effect.parameters.len());
                fields.push(strength_field(*strength));
                for parameter in effect.parameters {
                    let value = parameters.get(&parameter.name).copied().unwrap_or(parameter.default);
                    fields.push(field(
                        &format!("parameter.{}", parameter.name),
                        &parameter.label,
                        value,
                        (parameter.min, parameter.max, parameter.step),
                    ));
                }
                fields
            }
            _ => Vec::new(),
        })
    }

    pub(super) fn filter_preset_id(&self) -> &'static str {
        match self.filter_settings.stages.as_slice() {
            [] => "none",
            [stage] => stage_id(stage),
            _ => "chain",
        }
    }

    /// Prepare a candidate before saving it. Keep the last accepted settings
    /// until the renderer confirms the pending chain's first dispatch.
    fn update_filter(
        &mut self,
        change: impl FnOnce(&mut FilterSettings, &mut usize) -> Result<(), String>,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        let mut candidate = self.filter_settings.clone();
        let mut selected = self.filter_selected_stage;
        change(&mut candidate, &mut selected)?;
        if candidate != self.filter_settings {
            candidate.validate()?;
            let state = apply(&candidate)?;
            if let Err(error) = self.library.save_filter_settings(&candidate) {
                return Err(match apply(&self.filter_settings) {
                    Ok(_) => error.to_string(),
                    Err(rollback) => format!("{error}; {rollback}"),
                });
            }
            let previous = std::mem::replace(&mut self.filter_settings, candidate);
            match state {
                FilterApplyState::Applied => self.filter_rollback = None,
                FilterApplyState::Deferred => {
                    self.filter_rollback.get_or_insert((previous, self.filter_selected_stage));
                }
            }
        }
        self.filter_selected_stage = selected;
        self.model()
    }

    pub(super) fn finish_filter_activation(
        &mut self,
        accepted: bool,
    ) -> Result<Option<ManagerViewModel>, String> {
        let Some((previous, selected)) = self.filter_rollback.take() else {
            return Ok(None);
        };
        if accepted {
            return Ok(None);
        }
        self.filter_settings = previous;
        self.filter_selected_stage = selected;
        self.library.save_filter_settings(&self.filter_settings).map_err(|error| error.to_string())?;
        self.model().map(Some)
    }

    pub(super) fn set_filter_preset(
        &mut self,
        id: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        if self.filter_preset_id() == id && !matches!(id, "chain" | "external") {
            return self.model();
        }
        let stage = if id == "none" { None } else { Some(builtin_stage(id)?) };
        self.update_filter(
            |settings, selected| {
                settings.stages.clear();
                settings.stages.extend(stage);
                *selected = 0;
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn edit_filter_chain(
        &mut self,
        edit: FilterChainEdit<'_>,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        self.update_filter(
            |settings, selected| {
                let checked = |index| {
                    usize::try_from(index).ok().filter(|&i| i < settings.stages.len()).ok_or(INVALID_STAGE)
                };
                match edit {
                    FilterChainEdit::Select(index) => *selected = checked(index)?,
                    FilterChainEdit::Add(preset) => {
                        *selected = settings.stages.len();
                        settings.stages.push(builtin_stage(preset)?);
                    }
                    FilterChainEdit::Remove(index) => {
                        let index = checked(index)?;
                        settings.stages.remove(index);
                        if index < *selected {
                            *selected -= 1;
                        } else {
                            *selected = (*selected).min(settings.stages.len().saturating_sub(1));
                        }
                    }
                    FilterChainEdit::Move { from, to } => {
                        let (from, to) = (checked(from)?, checked(to)?);
                        if from != to {
                            let stage = settings.stages.remove(from);
                            settings.stages.insert(to, stage);
                            *selected = if *selected == from {
                                to
                            } else if from < *selected && *selected <= to {
                                *selected - 1
                            } else if to <= *selected && *selected < from {
                                *selected + 1
                            } else {
                                *selected
                            };
                        }
                    }
                }
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn set_filter_parameter(
        &mut self,
        key: &str,
        value: &str,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        let value: f32 = value.trim().parse().map_err(|_| "SHINKI_EMU_FILTER_PARAMETER")?;
        self.update_filter(
            |settings, selected| {
                match (key, settings.stages.get_mut(*selected).ok_or(INVALID_STAGE)?) {
                    ("filter.scale", FilterStage::Scale { scale } | FilterStage::Lanczos3 { scale }) => {
                        *scale = value;
                    }
                    ("filter.strength", FilterStage::Sharpen { strength } | FilterStage::Format4 { strength, .. }) => {
                        *strength = value;
                    }
                    (key, FilterStage::Format4 { parameters, source, .. }) => {
                        let name = key.strip_prefix("parameter.").ok_or(UNKNOWN_FIELD)?;
                        let effect = EffectSource::parse(source).map_err(|error| error.to_string())?;
                        let parameter = effect.parameters.iter().find(|parameter| parameter.name == name)
                            .ok_or(UNKNOWN_FIELD)?;
                        if !(parameter.min..=parameter.max).contains(&value) {
                            return Err("SHINKI_EMU_FILTER_PARAMETER_RANGE".into());
                        }
                        parameters.insert(name.into(), value);
                    }
                    _ => return Err(UNKNOWN_FIELD.into()),
                }
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn reset_filter_parameters(
        &mut self,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        self.update_filter(
            |settings, selected| {
                if let Some(stage) = settings.stages.get_mut(*selected) {
                    if let FilterStage::Format4 { strength, parameters, .. } = stage {
                        *strength = FILTER_DEFAULT_STRENGTH;
                        parameters.clear();
                    } else {
                        *stage = builtin_stage(stage_id(stage))?;
                    }
                }
                Ok(())
            },
            apply,
        )
    }

    pub(super) fn load_filter_source(
        &mut self,
        path: &Path,
        apply: FilterApply<'_>,
    ) -> Result<ManagerViewModel, String> {
        use std::io::Read;
        let file = fs::File::open(path).map_err(|_| "SHINKI_EMU_FILTER_SOURCE_OPEN")?;
        let mut bytes = Vec::new();
        file.take(FILTER_SOURCE_BYTE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "SHINKI_EMU_FILTER_SOURCE_READ")?;
        if bytes.len() > FILTER_SOURCE_BYTE_LIMIT {
            return Err("SHINKI_EMU_FILTER_SOURCE_SIZE".into());
        }
        let source = String::from_utf8(bytes).map_err(|_| "SHINKI_EMU_FILTER_SOURCE_UTF8")?;
        self.update_filter(
            |settings, selected| {
                *selected = settings.stages.len();
                settings.stages.push(FilterStage::Format4 {
                    source,
                    strength: FILTER_DEFAULT_STRENGTH,
                    parameters: BTreeMap::new(),
                    disabled: false,
                });
                Ok(())
            },
            apply,
        )
    }
}

fn builtin_stage(id: &str) -> Result<FilterStage, String> {
    Ok(match id {
        "scale" => FilterStage::Scale { scale: 1.0 },
        "sharpen" => FilterStage::Sharpen { strength: FILTER_DEFAULT_STRENGTH },
        "anime4k" => FilterStage::LegacyAnime4k,
        "lanczos3" => FilterStage::Lanczos3 { scale: 2.0 },
        "scale2x" => FilterStage::Scale2x,
        "anime4k_original_restore" => FilterStage::Anime4kOriginalRestore,
        "anime4k_original_x2" => FilterStage::Anime4kOriginalX2,
        _ => return Err("SHINKI_EMU_FILTER_PRESET_UNSUPPORTED".into()),
    })
}

fn stage_id(stage: &FilterStage) -> &'static str {
    match stage {
        FilterStage::Scale { .. } => "scale",
        FilterStage::Sharpen { .. } => "sharpen",
        FilterStage::LegacyAnime4k => "anime4k",
        FilterStage::Lanczos3 { .. } => "lanczos3",
        FilterStage::Scale2x => "scale2x",
        FilterStage::Anime4kOriginalRestore => "anime4k_original_restore",
        FilterStage::Anime4kOriginalX2 => "anime4k_original_x2",
        FilterStage::Format4 { .. } => "external",
    }
}
