use super::*;
use shinki_emu_manager_core::model::{ConfigEntry, ConfigField, ConfigKind, ConfigValue};
use shinki_emu_manager_ui_slint::tr;

struct ConfigPathTarget {
    plugin_id: String,
    game_id: String,
    field_id: String,
}

impl ConfigPathTarget {
    fn matches(&self, game_id: &str, field_id: &str) -> bool {
        self.game_id == game_id && self.field_id == field_id
    }

    fn saved_value(&self, library: &Library) -> Result<Option<ConfigValue>, String> {
        Ok(library
            .family_overrides(&self.plugin_id, &self.game_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.id == self.field_id)
            .map(|entry| entry.value))
    }
}

pub(super) struct PendingConfigPath {
    target: ConfigPathTarget,
    previous: Option<ConfigValue>,
    result: tokio::sync::oneshot::Receiver<Result<String, String>>,
}

pub(super) struct ConfigPathError {
    target: ConfigPathTarget,
    message: String,
}

impl ShinkiEmuManagerController {
    pub(super) fn ensure_no_session(&self) -> Result<(), String> {
        if self.session_running() {
            return Err("SHINKI_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        Ok(())
    }

    fn game_plugin(&self, game_id: &str) -> Result<String, String> {
        self.game(game_id)?;
        self.candidates
            .get(game_id)
            .map(|candidate| candidate.plugin_id.clone())
            .ok_or_else(|| "SHINKI_EMU_EMULATOR_SELECTION_REQUIRED".into())
    }

    fn schema_field(&self, plugin_id: &str, field_id: &str) -> Result<(Vec<ConfigField>, usize), String> {
        let schema = self.registry.configuration(plugin_id)?;
        let index = schema
            .iter()
            .position(|field| field.id == field_id)
            .ok_or_else(|| format!("SHINKI_EMU_FAMILY_CONFIG: {field_id}: unknown field"))?;
        Ok((schema, index))
    }

    fn store_config(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<(), String> {
        let (schema, index) = self.schema_field(plugin_id, field_id)?;
        let entry = ConfigEntry {
            id: field_id.into(),
            value: parse_config_value(&schema[index], raw)?,
        };
        self.library
            .set_family_override(plugin_id, game_id, &schema, entry)
            .map_err(|error| error.to_string())
    }

    pub(super) fn set_game_emulator(
        &mut self,
        game_id: &str,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        if self.native_action.is_some() { return Err("SHINKI_EMU_PREPARE_BUSY".into()); }
        if self.manual_games.contains(game_id) {
            // Saved only after this core's session shows its first frame.
            let candidate = self.probe_choices.get(game_id)
                .and_then(|choices| choices.iter().find(|choice| choice.plugin_id == plugin_id))
                .cloned()
                .ok_or("SHINKI_EMU_EMULATOR_SELECTION_INVALID")?;
            if self.candidates.get(game_id) != Some(&candidate) {
                self.configuration_errors.remove(game_id);
            }
            self.candidates.insert(game_id.to_owned(), candidate);
            return self.model();
        }
        let previous = self.library.game_settings(game_id).map_err(|error| error.to_string())?
            .and_then(|settings| settings.emulator_id);
        self.install_probe_choice(game_id, plugin_id)?;
        if previous.as_deref() != Some(plugin_id) {
            self.configuration_errors.remove(game_id);
        }
        self.model()
    }

    pub(super) fn set_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        self.store_config(&plugin, game_id, field_id, raw)?;
        self.clear_config_path_feedback(game_id, field_id);
        self.model()
    }

    pub(super) fn reset_game_config(
        &mut self,
        game_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        self.library
            .remove_family_override(&plugin, game_id, field_id)
            .map_err(|error| error.to_string())?;
        self.clear_config_path_feedback(game_id, field_id);
        self.model()
    }

    pub(super) fn set_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
        raw: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        self.store_config(plugin_id, "", field_id, raw)?;
        self.model()
    }

    pub(super) fn reset_emulator_default(
        &mut self,
        plugin_id: &str,
        field_id: &str,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        self.registry.configuration(plugin_id)?;
        self.library
            .remove_family_override(plugin_id, "", field_id)
            .map_err(|error| error.to_string())?;
        self.model()
    }

    pub(super) fn select_emulator_defaults(
        &mut self,
        plugin_id: &str,
    ) -> Result<ManagerViewModel, String> {
        if plugin_id.is_empty() {
            self.defaults_plugin = None;
        } else {
            self.request_activation(recognition::NativeAction::Defaults(plugin_id.into()))?;
            self.defaults_plugin = Some(plugin_id.into());
        }
        self.model()
    }

    /// How the host should pick a value for a path field.
    pub(super) fn config_path_pick(
        &self,
        game_id: &str,
        field_id: &str,
    ) -> Result<shinki_emu_manager::ConfigPathPick, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        let (schema, index) = self.schema_field(&plugin, field_id)?;
        let field = &schema[index];
        let directory = if field.flags.directory_path {
            true
        } else if field.flags.file_path {
            false
        } else {
            return Err(format!("SHINKI_EMU_FAMILY_CONFIG: {field_id}: not a path"));
        };
        let filter = self
            .registry
            .config_path_options(&plugin, field_id)
            .and_then(|options| options.filter.clone());
        Ok(shinki_emu_manager::ConfigPathPick {
            directory,
            title: field.label.clone(),
            filter,
        })
    }

    /// Store a path the host picked for a path field.
    pub(super) fn set_config_path(
        &mut self,
        game_id: &str,
        field_id: &str,
        path: &Path,
    ) -> Result<ManagerViewModel, String> {
        self.ensure_no_session()?;
        let plugin = self.game_plugin(game_id)?;
        let (schema, index) = self.schema_field(&plugin, field_id)?;
        let field = &schema[index];
        if !field.flags.directory_path && !field.flags.file_path {
            return Err(format!("SHINKI_EMU_FAMILY_CONFIG: {field_id}: not a path"));
        }
        let game = self.game(game_id)?;
        let game_directory = self.native_game_directory(&game)?;
        if self
            .registry
            .config_path_options(&plugin, field_id)
            .is_some_and(|options| options.validates_path)
        {
            if self.startup_busy() {
                return Err("SHINKI_EMU_PREPARE_BUSY".into());
            }
            let target = ConfigPathTarget {
                plugin_id: plugin,
                game_id: game_id.into(),
                field_id: field_id.into(),
            };
            let previous = target.saved_value(&self.library)?;
            let result = self.registry.validate_config_path(
                &target.plugin_id,
                field_id,
                path,
                game_directory,
                field.flags.game_relative,
            )?;
            self.clear_config_path_feedback(game_id, field_id);
            self.pending_config_path = Some(PendingConfigPath {
                target,
                previous,
                result,
            });
        } else {
            let path = canonical_config_path(path)?;
            let raw = config_path_value(&path, game_directory, field.flags.game_relative)?;
            self.store_config(&plugin, game_id, field_id, &raw)?;
            self.clear_config_path_feedback(game_id, field_id);
        }
        self.model()
    }

    fn clear_config_path_feedback(&mut self, game_id: &str, field_id: &str) {
        if self
            .pending_config_path
            .as_ref()
            .is_some_and(|pending| pending.target.matches(game_id, field_id))
        {
            self.pending_config_path = None;
        }
        if self
            .config_path_error
            .as_ref()
            .is_some_and(|error| error.target.matches(game_id, field_id))
        {
            self.config_path_error = None;
        }
    }

    pub(super) fn poll_config_path(&mut self) -> bool {
        use tokio::sync::oneshot::error::TryRecvError;

        let Some(pending) = &mut self.pending_config_path else {
            return false;
        };
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Closed) => Err("SHINKI_EMU_OWNER_DISCONNECTED".into()),
        };
        let PendingConfigPath {
            target, previous, ..
        } = self.pending_config_path.take().unwrap();
        if self.game_plugin(&target.game_id).as_deref() != Ok(target.plugin_id.as_str()) {
            return true;
        }
        let result = (|| {
            self.ensure_no_session()?;
            // A preset or other intervening edit takes precedence over this pick.
            if target.saved_value(&self.library)? == previous {
                self.store_config(&target.plugin_id, &target.game_id, &target.field_id, &result?)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(message) = result {
            self.config_path_error = Some(ConfigPathError { target, message });
        }
        true
    }

    pub(super) fn game_config_fields(
        &mut self,
        game_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        let Some(plugin) = self
            .candidates
            .get(game_id)
            .map(|candidate| candidate.plugin_id.clone())
        else {
            return Ok(Vec::new());
        };
        let mut fields = self.scope_fields(&plugin, game_id)?;
        if let Some(error) = &self.config_path_error
            && error.target.game_id == game_id
            && error.target.plugin_id == plugin
            && let Some(field) = fields.iter_mut().find(|field| field.key == error.target.field_id)
        {
            field.error = shinki_emu_manager_ui_slint::describe_error(&error.message).title;
        }
        Ok(fields)
    }

    pub(super) fn emulator_default_fields(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        self.scope_fields(plugin_id, "")
    }

    fn scope_fields(
        &mut self,
        plugin_id: &str,
        game_id: &str,
    ) -> Result<Vec<ConfigFieldViewModel>, String> {
        let Ok(schema) = self.registry.configuration(plugin_id) else {
            return Ok(Vec::new());
        };
        let configuration = self
            .library
            .family_configuration(plugin_id, game_id, &schema)
            .map_err(|error| error.to_string())?;
        self.report_retired_configuration(&configuration.retired);
        let overrides = self
            .library
            .family_overrides(plugin_id, game_id)
            .map_err(|error| error.to_string())?;
        Ok(config_field_views(&schema, &configuration.values, &overrides))
    }

    pub(super) fn report_retired_configuration(
        &mut self,
        retired: &[shinki_emu_manager_core::RetiredConfigEntry],
    ) -> bool {
        if retired.is_empty() {
            return false;
        }
        let mut paths = Vec::new();
        for entry in retired {
            tracing::warn!(
                event = "shinki.emu.config.retired_field",
                plugin_id = %entry.plugin_id,
                game_id = %entry.game_id,
                field_id = %entry.field_id,
                value = ?entry.value
            );
            if let shinki_emu_manager_core::model::ConfigValue::String(value) = &entry.value
                && retired_value_is_path(&entry.field_id, value)
            {
                let slug = core_documentation_slug(&entry.plugin_id);
                paths.push(tr!(
                    "核心设置项已不存在。旧存档文件夹：{value}\n请将其中的存档复制到该核心当前的存档位置。核心文档：https://project-shinkirou.github.io/en/shinkiemu/cores/{slug}/",
                    value = value,
                    slug = slug
                ));
            }
        }
        if paths.is_empty() {
            return false;
        }
        let new_paths = paths
            .into_iter()
            .filter(|message| !self.diagnostic.contains(message))
            .collect::<Vec<_>>();
        if new_paths.is_empty() {
            return true;
        }
        if self.retired_diagnostic.as_deref() == Some(self.diagnostic.as_str()) {
            self.diagnostic.push_str("\n\n");
            self.diagnostic.push_str(&new_paths.join("\n\n"));
        } else {
            self.diagnostic = new_paths.join("\n\n");
        }
        self.retired_diagnostic = Some(self.diagnostic.clone());
        self.diagnostic_severity = "warning";
        true
    }
}

fn retired_value_is_path(field_id: &str, value: &str) -> bool {
    let field_kind = field_id
        .rsplit(['.', '_', '-'])
        .next()
        .unwrap_or(field_id)
        .to_ascii_lowercase();
    Path::new(value).is_absolute()
        || value.contains('/')
        || value.contains('\\')
        || matches!(value, "." | "..")
        || matches!(field_kind.as_str(), "path" | "dir" | "directory" | "root")
}

fn core_documentation_slug(plugin_id: &str) -> String {
    plugin_id
        .chars()
        .map(|character| match character {
            '.' | '_' => '-',
            character => character.to_ascii_lowercase(),
        })
        .collect()
}

pub(super) fn canonical_config_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|_| "SHINKI_EMU_CONFIG_PATH_INVALID".into())
}

/// Make the already canonical path game-relative when the field allows it and
/// the path lies inside that directory.
pub(super) fn config_path_value(
    path: &Path,
    game: &Path,
    game_relative: bool,
) -> Result<String, String> {
    let game = fs::canonicalize(game).map_err(|_| "SHINKI_EMU_GAME_DIRECTORY_INVALID")?;
    let stored = match path.strip_prefix(&game) {
        Ok(relative) if game_relative && !relative.as_os_str().is_empty() => relative,
        _ => path,
    };
    stored
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "SHINKI_EMU_CONFIG_PATH_UTF8".into())
}

pub(super) fn config_field_views(
    schema: &[ConfigField],
    entries: &[ConfigEntry],
    overrides: &[ConfigEntry],
) -> Vec<ConfigFieldViewModel> {
    schema
        .iter()
        .zip(entries)
        .map(|(field, entry)| {
            let (kind, min, max, option_values) = match &field.kind {
                ConfigKind::Bool => ("bool", 0.0, 0.0, Vec::new()),
                ConfigKind::Integer { min, max } => ("integer", *min as f64, *max as f64, Vec::new()),
                ConfigKind::Number { min, max } => ("float", *min, *max, Vec::new()),
                ConfigKind::String { .. } if field.flags.file_path => ("file", 0.0, 0.0, Vec::new()),
                ConfigKind::String { .. } if field.flags.directory_path => {
                    ("directory", 0.0, 0.0, Vec::new())
                }
                ConfigKind::String { .. } => ("string", 0.0, 0.0, Vec::new()),
                ConfigKind::Enum { choices } => ("enum", 0.0, 0.0, choices.clone()),
            };
            ConfigFieldViewModel {
                key: field.id.clone(),
                label: field.label.clone(),
                description: field.description.clone(),
                group: field.group.clone(),
                kind: kind.into(),
                value: raw_config_value(&entry.value),
                display_value: display_config_value(field, &entry.value),
                options: (0..option_values.len())
                    .map(|index| choice_label(field, index))
                    .collect(),
                option_values,
                min,
                max,
                step: field.step.unwrap_or(0.0),
                unit: field.unit.clone(),
                advanced: field.flags.advanced,
                error: String::new(),
                overridden: overrides.iter().any(|stored| stored.id == field.id),
            }
        })
        .collect()
}

fn choice_label(field: &ConfigField, index: usize) -> String {
    match &field.kind {
        ConfigKind::Enum { choices } => field
            .choice_labels
            .get(index)
            .or_else(|| choices.get(index))
            .cloned()
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn raw_config_value(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Bool(value) => value.to_string(),
        ConfigValue::Integer(value) => value.to_string(),
        ConfigValue::Number(value) => value.to_string(),
        ConfigValue::String(value) | ConfigValue::Enum(value) => value.clone(),
    }
}

fn display_config_value(field: &ConfigField, value: &ConfigValue) -> String {
    match (value, &field.kind) {
        (ConfigValue::Enum(value), ConfigKind::Enum { choices }) => choices
            .iter()
            .position(|choice| choice == value)
            .map_or_else(|| value.clone(), |index| choice_label(field, index)),
        _ => raw_config_value(value),
    }
}

fn enum_choice(field: &ConfigField, choices: &[String], raw: &str) -> String {
    if choices.iter().any(|choice| choice == raw) {
        return raw.into();
    }
    field
        .choice_labels
        .iter()
        .position(|label| label == raw)
        .and_then(|index| choices.get(index))
        .map_or_else(|| raw.into(), Clone::clone)
}

pub(super) fn parse_config_entries<'a>(
    schema: &[ConfigField],
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<ConfigEntry>, String> {
    values
        .into_iter()
        .map(|(id, raw)| {
            let field = schema
                .iter()
                .find(|field| field.id == id)
                .ok_or_else(|| format!("SHINKI_EMU_FAMILY_CONFIG: {id}: unknown field"))?;
            Ok(ConfigEntry {
                id: field.id.clone(),
                value: parse_config_value(field, raw)?,
            })
        })
        .collect()
}

pub(super) fn parse_config_value(field: &ConfigField, raw: &str) -> Result<ConfigValue, String> {
    let invalid = || format!("SHINKI_EMU_FAMILY_CONFIG: {}: invalid value", field.id);
    let value = match field.kind {
        ConfigKind::Bool => ConfigValue::Bool(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Integer { .. } => ConfigValue::Integer(raw.parse().map_err(|_| invalid())?),
        ConfigKind::Number { .. } => ConfigValue::Number(raw.parse().map_err(|_| invalid())?),
        ConfigKind::String { .. } => ConfigValue::String(raw.into()),
        ConfigKind::Enum { ref choices } => ConfigValue::Enum(enum_choice(field, choices, raw)),
    };
    if !field.kind.accepts(&value) {
        return Err(invalid());
    }
    Ok(value)
}

#[cfg(test)]
#[path = "family_config_tests.rs"]
pub(super) mod tests;
