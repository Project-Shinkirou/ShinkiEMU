//! Typed startup configuration. Empty game ID denotes core defaults.
use crate::model::{ConfigEntry, ConfigField, ConfigValue, resolve_config};
use crate::{Library, LibraryError};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

/// Retired values pruned from stored layers are returned alongside resolved values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetiredConfigEntry {
    pub plugin_id: String,
    pub game_id: String,
    pub field_id: String,
    pub value: ConfigValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FamilyConfiguration {
    pub values: Vec<ConfigEntry>,
    pub retired: Vec<RetiredConfigEntry>,
}

impl Library {
    pub fn family_configuration(
        &self,
        plugin_id: &str,
        game_id: &str,
        schema: &[ConfigField],
    ) -> Result<FamilyConfiguration, LibraryError> {
        let transaction = self.connection.unchecked_transaction()?;
        let (mut values, mut retired) =
            read_family_configuration(&transaction, plugin_id, "", schema)?;
        if !game_id.is_empty() {
            let (overrides, retired_overrides) =
                read_family_configuration(&transaction, plugin_id, game_id, schema)?;
            retired.extend(retired_overrides);
            for entry in overrides {
                values.retain(|value| value.id != entry.id);
                values.push(entry);
            }
        }
        let values = resolve_config(schema, &values).map_err(LibraryError::FamilyConfiguration)?;
        transaction.commit()?;
        Ok(FamilyConfiguration { values, retired })
    }

    fn read_family_configuration(
        &self,
        plugin_id: &str,
        game_id: &str,
    ) -> Result<Vec<ConfigEntry>, LibraryError> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT values_json FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
                params![plugin_id, game_id],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(raw) if raw.len() <= 4 * 1024 * 1024 => {
                serde_json::from_str(&raw).map_err(|_| LibraryError::Settings)
            }
            Some(_) => Err(LibraryError::Settings),
            None => Ok(Vec::new()),
        }
    }

    pub fn save_family_configuration(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        schema: &[ConfigField],
        values: &[ConfigEntry],
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(plugin_id)?;
        if !game_id.is_empty() && self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        resolve_config(schema, values).map_err(LibraryError::FamilyConfiguration)?;
        let raw = serde_json::to_string(values).map_err(|_| LibraryError::Serialization)?;
        write_configuration(&self.connection, plugin_id, game_id, &raw)?;
        Ok(())
    }

    /// Stored entries of one layer without defaults, marking what is set there.
    pub fn family_overrides(
        &self,
        plugin_id: &str,
        game_id: &str,
    ) -> Result<Vec<ConfigEntry>, LibraryError> {
        crate::library::validate_id(plugin_id)?;
        self.read_family_configuration(plugin_id, game_id)
    }

    /// Validate and store one entry in a layer, replacing a previous value.
    pub fn set_family_override(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        schema: &[ConfigField],
        entry: ConfigEntry,
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(plugin_id)?;
        if !game_id.is_empty() && self.game(game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        let mut values = self.read_family_configuration(plugin_id, game_id)?;
        match values.iter_mut().find(|value| value.id == entry.id) {
            Some(value) => *value = entry,
            None => values.push(entry),
        }
        resolve_config(schema, &values).map_err(LibraryError::FamilyConfiguration)?;
        let raw = serde_json::to_string(&values).map_err(|_| LibraryError::Serialization)?;
        write_configuration(&self.connection, plugin_id, game_id, &raw)
    }

    /// Remove one entry from a layer. Returns whether the entry was stored.
    pub fn remove_family_override(
        &mut self,
        plugin_id: &str,
        game_id: &str,
        field_id: &str,
    ) -> Result<bool, LibraryError> {
        crate::library::validate_id(plugin_id)?;
        let mut values = self.read_family_configuration(plugin_id, game_id)?;
        let before = values.len();
        values.retain(|value| value.id != field_id);
        if values.len() == before {
            return Ok(false);
        }
        if values.is_empty() {
            self.connection.execute(
                "DELETE FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
                params![plugin_id, game_id],
            )?;
        } else {
            let raw = serde_json::to_string(&values).map_err(|_| LibraryError::Serialization)?;
            write_configuration(&self.connection, plugin_id, game_id, &raw)?;
        }
        Ok(true)
    }

    /// Apply a converted output's emulator selection and typed profile as one
    /// transaction. The caller must identify the output game by canonical path.
    pub fn apply_patch_profile(
        &mut self,
        game_id: &str,
        engine_id: &str,
        emulator_id: &str,
        schema: &[ConfigField],
        values: &[ConfigEntry],
    ) -> Result<(), LibraryError> {
        let settings = self.prepare_game_emulator(game_id, engine_id, emulator_id)?;
        resolve_config(schema, values).map_err(LibraryError::FamilyConfiguration)?;
        let configuration =
            serde_json::to_string(values).map_err(|_| LibraryError::Serialization)?;
        let transaction = self.connection.transaction()?;
        crate::library::write_game_emulator(&transaction, game_id, engine_id, &settings)?;
        write_configuration(&transaction, emulator_id, game_id, &configuration)?;
        transaction.commit()?;
        Ok(())
    }
}

fn read_family_configuration(
    transaction: &Transaction<'_>,
    plugin_id: &str,
    game_id: &str,
    schema: &[ConfigField],
) -> Result<(Vec<ConfigEntry>, Vec<RetiredConfigEntry>), LibraryError> {
    let raw: Option<String> = transaction
        .query_row(
            "SELECT values_json FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
            params![plugin_id, game_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok((Vec::new(), Vec::new()));
    };
    if raw.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if raw.len() > 4 * 1024 * 1024 {
        return Err(LibraryError::Settings);
    }
    let values: Vec<ConfigEntry> =
        serde_json::from_str(&raw).map_err(|_| LibraryError::Settings)?;
    let mut known = Vec::with_capacity(values.len());
    let mut retired = Vec::new();
    for entry in values {
        if schema.iter().any(|field| field.id == entry.id) {
            known.push(entry);
        } else {
            retired.push(RetiredConfigEntry {
                plugin_id: plugin_id.to_owned(),
                game_id: game_id.to_owned(),
                field_id: entry.id,
                value: entry.value,
            });
        }
    }
    resolve_config(schema, &known).map_err(LibraryError::FamilyConfiguration)?;
    if !retired.is_empty() {
        if known.is_empty() {
            transaction.execute(
                "DELETE FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
                params![plugin_id, game_id],
            )?;
        } else {
            let raw = serde_json::to_string(&known).map_err(|_| LibraryError::Serialization)?;
            write_configuration(transaction, plugin_id, game_id, &raw)?;
        }
    }
    Ok((known, retired))
}

fn write_configuration(
    connection: &rusqlite::Connection,
    plugin_id: &str,
    game_id: &str,
    values_json: &str,
) -> Result<(), LibraryError> {
    connection.execute(
        "INSERT INTO family_configuration(plugin_id, game_id, values_json) VALUES(?1, ?2, ?3)
         ON CONFLICT(plugin_id, game_id) DO UPDATE SET values_json=excluded.values_json",
        params![plugin_id, game_id, values_json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConfigFlags, ConfigKind, ConfigValue};

    #[test]
    fn persistence_defaults_overrides_reset_and_failed_write_preservation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manager.sqlite");
        let mut library = Library::open(&path).unwrap();
        library
            .add_game(&crate::GameRecord {
                game_id: "game".into(),
                title: "Test".into(),
                user_title: None,
                location: "test-game".into(),
                family_id: Some("test".into()),
                content_fingerprint: None,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let schema = vec![ConfigField {
            id: "gain".into(),
            label: "Gain".into(),
            group: "Audio".into(),
            kind: ConfigKind::Number { min: 0.0, max: 1.0 },
            default: ConfigValue::Number(0.5),
            description: String::new(),
            choice_labels: Vec::new(),
            flags: ConfigFlags::default(),
            step: None,
            unit: String::new(),
        }];
        let entry = |value| ConfigEntry {
            id: "gain".into(),
            value: ConfigValue::Number(value),
        };
        library
            .save_family_configuration("test.core", "", &schema, &[entry(0.7)])
            .unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap()
                .values,
            vec![entry(0.7)]
        );
        assert!(
            library
                .save_family_configuration("test.core", "", &schema, &[entry(2.0)])
                .is_err()
        );
        assert_eq!(
            library
                .family_configuration("test.core", "", &schema)
                .unwrap()
                .values,
            vec![entry(0.7)]
        );
        library
            .save_family_configuration("test.core", "game", &schema, &[entry(0.2)])
            .unwrap();
        drop(library);
        let mut library = Library::open(&path).unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap()
                .values,
            vec![entry(0.2)]
        );
        library
            .save_family_configuration("test.core", "", &schema, &[])
            .unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "", &schema)
                .unwrap()
                .values,
            vec![entry(0.5)]
        );
        library
            .connection
            .execute(
                "UPDATE family_configuration SET values_json=?1 WHERE game_id='game'",
                [serde_json::to_string(&vec![entry(0.1), entry(0.2)]).unwrap()],
            )
            .unwrap();
        assert!(
            library
                .family_configuration("test.core", "game", &schema)
                .is_err()
        );
        library.remove_game("game").unwrap();
        assert_eq!(
            library
                .family_configuration("test.core", "game", &schema)
                .unwrap()
                .values,
            vec![entry(0.5)]
        );
    }

    #[test]
    fn loading_prunes_retired_fields_and_returns_known_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("manager.sqlite");
        let mut library = Library::open(&path).unwrap();
        library
            .add_game(&crate::GameRecord {
                game_id: "game".into(),
                title: "Test".into(),
                user_title: None,
                location: "test-game".into(),
                family_id: Some("test".into()),
                content_fingerprint: None,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let schema = vec![ConfigField {
            id: "gain".into(),
            label: "Gain".into(),
            group: "Audio".into(),
            kind: ConfigKind::Number { min: 0.0, max: 1.0 },
            default: ConfigValue::Number(0.5),
            description: String::new(),
            choice_labels: Vec::new(),
            flags: ConfigFlags::default(),
            step: None,
            unit: String::new(),
        }];
        let known = ConfigEntry {
            id: "gain".into(),
            value: ConfigValue::Number(0.7),
        };
        let retired = ConfigEntry {
            id: "save_root".into(),
            value: ConfigValue::String("C:/old-saves".into()),
        };
        library
            .connection
            .execute(
                "INSERT INTO family_configuration(plugin_id, game_id, values_json) VALUES(?1, '', ?2)",
                params!["test.core", serde_json::to_string(&[known.clone(), retired.clone()]).unwrap()],
            )
            .unwrap();

        let loaded = library
            .family_configuration("test.core", "game", &schema)
            .unwrap();

        assert_eq!(loaded.values, std::slice::from_ref(&known));
        assert_eq!(
            loaded.retired,
            [RetiredConfigEntry {
                plugin_id: "test.core".into(),
                game_id: String::new(),
                field_id: "save_root".into(),
                value: retired.value,
            }]
        );
        let rewritten: String = library
            .connection
            .query_row(
                "SELECT values_json FROM family_configuration WHERE plugin_id='test.core' AND game_id=''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<ConfigEntry>>(&rewritten).unwrap(),
            [known]
        );
    }

    #[test]
    fn known_stored_values_remain_strict_and_unknown_writes_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let mut library = Library::open(directory.path().join("manager.sqlite")).unwrap();
        library
            .add_game(&crate::GameRecord {
                game_id: "game".into(),
                title: "Test".into(),
                user_title: None,
                location: "test-game".into(),
                family_id: Some("test".into()),
                content_fingerprint: None,
                added_at_unix_ms: 0,
            })
            .unwrap();
        let schema = vec![ConfigField {
            id: "gain".into(),
            label: "Gain".into(),
            group: "Audio".into(),
            kind: ConfigKind::Number { min: 0.0, max: 1.0 },
            default: ConfigValue::Number(0.5),
            description: String::new(),
            choice_labels: Vec::new(),
            flags: ConfigFlags::default(),
            step: None,
            unit: String::new(),
        }];
        let entry = |value| ConfigEntry {
            id: "gain".into(),
            value,
        };
        for values in [
            vec![
                entry(ConfigValue::Number(0.1)),
                entry(ConfigValue::Number(0.2)),
            ],
            vec![entry(ConfigValue::Bool(true))],
        ] {
            library
                .connection
                .execute(
                    "INSERT INTO family_configuration(plugin_id, game_id, values_json) VALUES('test.core', '', ?1) ON CONFLICT(plugin_id, game_id) DO UPDATE SET values_json=excluded.values_json",
                    [serde_json::to_string(&values).unwrap()],
                )
                .unwrap();
            assert!(
                library
                    .family_configuration("test.core", "", &schema)
                    .is_err()
            );
        }
        let unknown = ConfigEntry {
            id: "retired".into(),
            value: ConfigValue::String("old path".into()),
        };
        assert!(
            library
                .save_family_configuration("test.core", "", &schema, std::slice::from_ref(&unknown))
                .is_err()
        );
        assert!(
            library
                .set_family_override("test.core", "game", &schema, unknown)
                .is_err()
        );
    }

    fn patch_fixture(library: &mut Library) -> Vec<ConfigField> {
        for id in ["source", "output"] {
            library
                .add_game(&crate::GameRecord {
                    game_id: id.into(),
                    title: id.into(),
                    user_title: None,
                    location: format!("public-{id}"),
                    family_id: Some("old.engine".into()),
                    content_fingerprint: None,
                    added_at_unix_ms: 1,
                })
                .unwrap();
            library
                .set_game_settings(
                    id,
                    &crate::GameSettings {
                        emulator_id: Some("old.emulator".into()),
                        input_mapping: Some(crate::default_vn_preset()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        vec![ConfigField {
            id: "script_encoding".into(),
            label: "Encoding".into(),
            group: "Script".into(),
            kind: ConfigKind::Enum {
                choices: vec!["cp932".into(), "gbk".into()],
            },
            default: ConfigValue::Enum("cp932".into()),
            description: String::new(),
            choice_labels: Vec::new(),
            flags: ConfigFlags::default(),
            step: None,
            unit: String::new(),
        }]
    }

    #[test]
    fn patch_profile_changes_only_independent_output_and_survives_reopen() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("patch.sqlite");
        let (source, source_settings, output_mapping, schema);
        {
            let mut library = Library::open(&path).unwrap();
            schema = patch_fixture(&mut library);
            source = library.game("source").unwrap();
            source_settings = library.game_settings("source").unwrap();
            output_mapping = library
                .game_settings("output")
                .unwrap()
                .unwrap()
                .input_mapping;
            library
                .apply_patch_profile(
                    "output",
                    "new.engine",
                    "new.emulator",
                    &schema,
                    &[ConfigEntry {
                        id: "script_encoding".into(),
                        value: ConfigValue::Enum("gbk".into()),
                    }],
                )
                .unwrap();
        }
        let library = Library::open(path).unwrap();
        assert_eq!(library.game("source").unwrap(), source);
        assert_eq!(library.game_settings("source").unwrap(), source_settings);
        assert_eq!(
            library
                .game("output")
                .unwrap()
                .unwrap()
                .family_id
                .as_deref(),
            Some("new.engine")
        );
        let settings = library.game_settings("output").unwrap().unwrap();
        assert_eq!(settings.emulator_id.as_deref(), Some("new.emulator"));
        assert_eq!(settings.input_mapping, output_mapping);
        assert_eq!(
            library
                .family_configuration("new.emulator", "output", &schema)
                .unwrap()
                .values,
            [ConfigEntry {
                id: "script_encoding".into(),
                value: ConfigValue::Enum("gbk".into())
            }]
        );
    }

    #[test]
    fn patch_profile_validation_and_last_sql_write_failure_roll_back_selection() {
        let mut library = Library::in_memory().unwrap();
        let schema = patch_fixture(&mut library);
        let original = library.game("output").unwrap();
        let settings = library.game_settings("output").unwrap();
        for values in [
            vec![ConfigEntry {
                id: "unknown".into(),
                value: ConfigValue::Bool(true),
            }],
            vec![ConfigEntry {
                id: "script_encoding".into(),
                value: ConfigValue::Enum("utf8".into()),
            }],
            vec![ConfigEntry {
                id: "script_encoding".into(),
                value: ConfigValue::Bool(true),
            }],
        ] {
            assert!(
                library
                    .apply_patch_profile("output", "new.engine", "new.emulator", &schema, &values)
                    .is_err()
            );
            assert_eq!(library.game("output").unwrap(), original);
            assert_eq!(library.game_settings("output").unwrap(), settings);
        }
        library
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_patch_config BEFORE INSERT ON family_configuration
             BEGIN SELECT RAISE(ABORT, 'public fixture rejects final write'); END;",
            )
            .unwrap();
        assert!(
            library
                .apply_patch_profile(
                    "output",
                    "new.engine",
                    "new.emulator",
                    &schema,
                    &[ConfigEntry {
                        id: "script_encoding".into(),
                        value: ConfigValue::Enum("gbk".into())
                    }]
                )
                .is_err()
        );
        assert_eq!(library.game("output").unwrap(), original);
        assert_eq!(library.game_settings("output").unwrap(), settings);
        let count: i64 = library
            .connection
            .query_row(
                "SELECT COUNT(*) FROM family_configuration WHERE plugin_id='new.emulator'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn single_field_overrides_layer_and_reject_invalid_values() {
        let mut library = Library::in_memory().unwrap();
        let schema = patch_fixture(&mut library);
        let encoding = |value: &str| ConfigEntry {
            id: "script_encoding".into(),
            value: ConfigValue::Enum(value.into()),
        };
        library
            .set_family_override("new.emulator", "", &schema, encoding("cp932"))
            .unwrap();
        library
            .set_family_override("new.emulator", "output", &schema, encoding("gbk"))
            .unwrap();
        assert_eq!(
            library.family_overrides("new.emulator", "output").unwrap(),
            [encoding("gbk")]
        );
        assert_eq!(
            library
                .family_configuration("new.emulator", "output", &schema)
                .unwrap()
                .values,
            [encoding("gbk")]
        );

        for invalid in [
            encoding("utf16"),
            ConfigEntry {
                id: "unknown".into(),
                value: ConfigValue::Bool(true),
            },
        ] {
            assert!(matches!(
                library.set_family_override("new.emulator", "output", &schema, invalid),
                Err(LibraryError::FamilyConfiguration(_))
            ));
            assert_eq!(
                library.family_overrides("new.emulator", "output").unwrap(),
                [encoding("gbk")]
            );
        }
        assert!(matches!(
            library.set_family_override("new.emulator", "absent", &schema, encoding("gbk")),
            Err(LibraryError::GameNotFound)
        ));

        assert!(
            library
                .remove_family_override("new.emulator", "output", "script_encoding")
                .unwrap()
        );
        assert!(
            !library
                .remove_family_override("new.emulator", "output", "script_encoding")
                .unwrap()
        );
        assert!(
            library
                .family_overrides("new.emulator", "output")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            library
                .family_configuration("new.emulator", "output", &schema)
                .unwrap()
                .values,
            [encoding("cp932")]
        );
    }
}
