//! In-place upgrades: schema 5 (and 4, after its engine rename) and schema 6
//! to the current schema 7.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Transaction};
use shinki_emu_translation::{OpenAiApi, TranslationProvider};

use super::{state::write_app_state, LibraryError, TranslationProfile, STATE_SCHEMA_SQL};
use crate::{
    model::{ConfigEntry, ConfigValue},
    work_settings::{GameSettings, RenderPreference},
};

pub(super) const SCHEMA_5_TABLES: [&str; 10] = [
    "family_configuration",
    "appearance_settings",
    "library_game",
    "external_identity",
    "metadata_snapshot",
    "play_session",
    "manager_settings",
    "game_settings",
    "translation_profile",
    "plugin_installation",
];

/// Emulators now use their upstream repository name as plugin ID.
const RENAMED_PLUGINS: [(&str, &str); 6] = [
    ("shinki.emu.fvp", "rfvp"),
    ("shinki.emu.siglus", "siglus_rs"),
    ("shinki.emu.artemis", "art3m1s-core"),
    ("sena-native-koikake", "sena-rs"),
    ("shinki-emu-ethornell", "ethornell-rs"),
    ("shinki-emu-krkr", "kirikiroid2-web"),
];
const FVP_PLUGIN: &str = "rfvp";
const FVP_REMOVED_FIELDS: [&str; 2] = ["surface", "render_scale"];
/// Emulators whose render size moved from configuration fields to the typed
/// render preference: `(plugin_id, width field, height field)`.
const RENDER_FIELD_PLUGINS: [(&str, &str, &str); 2] = [
    ("shinki.emu.musica", "render_width", "render_height"),
    ("sena-rs", "raster_width", "raster_height"),
];

pub(super) fn upgrade_schema_5(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    transaction.execute_batch(STATE_SCHEMA_SQL)?;
    transaction.execute_batch("DROP TABLE plugin_installation;")?;
    migrate_translation_profile(transaction)?;
    rename_plugins(transaction)?;
    migrate_family_configuration(transaction)?;
    let has_games: bool =
        transaction.query_row("SELECT EXISTS(SELECT 1 FROM library_game)", [], |row| {
            row.get(0)
        })?;
    if has_games {
        write_app_state(transaction, "onboarding.completed", &true)?;
    }
    Ok(())
}

/// Schema 7: several games may link the same remote work, and a pinned
/// release keeps its title.
pub(super) fn upgrade_schema_6(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    rebuild_external_identity(transaction)?;
    transaction
        .execute_batch("ALTER TABLE game_release ADD COLUMN title TEXT NOT NULL DEFAULT '';")?;
    Ok(())
}

/// Recreate `external_identity` without the schema 4–6 `UNIQUE(provider,
/// remote_id)` constraint, keeping every row.
pub(super) fn rebuild_external_identity(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    transaction.execute_batch(
        "CREATE TABLE external_identity_v7 (
            game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            provenance TEXT NOT NULL,
            linked_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY(game_id, provider)
        );
        INSERT INTO external_identity_v7(
            game_id, provider, remote_id, provenance, linked_at_unix_ms)
        SELECT game_id, provider, remote_id, provenance, linked_at_unix_ms
        FROM external_identity;
        DROP TABLE external_identity;
        ALTER TABLE external_identity_v7 RENAME TO external_identity;",
    )?;
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaFiveTranslationProfile {
    profile_id: String,
    #[serde(rename = "endpoint_kind")]
    _endpoint_kind: String,
    endpoint: String,
    protocol: OpenAiApi,
    model: String,
    target_language: String,
    timeout_ms: u64,
    secret_reference: String,
}

fn convert_translation_profile(raw: &str) -> Option<String> {
    let previous: SchemaFiveTranslationProfile = serde_json::from_str(raw).ok()?;
    let defaults = TranslationProfile::default();
    let profile = TranslationProfile {
        profile_id: previous.profile_id,
        provider: TranslationProvider::OpenAi,
        base_url: previous.endpoint,
        openai_api: previous.protocol,
        model: previous.model,
        target_language: previous.target_language,
        timeout_ms: previous.timeout_ms,
        secret_reference: previous.secret_reference,
        custom_prompt: String::new(),
        context_segments: defaults.context_segments,
        context_chars: defaults.context_chars,
    };
    profile.validate().ok()?;
    serde_json::to_string(&profile).ok()
}

fn migrate_translation_profile(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    let raw: Option<String> = transaction
        .query_row(
            "SELECT profile_json FROM translation_profile WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(());
    };
    match convert_translation_profile(&raw) {
        Some(converted) => {
            transaction.execute(
                "UPDATE translation_profile SET profile_json=?1 WHERE singleton=1",
                [converted],
            )?;
        }
        None => {
            transaction.execute("DELETE FROM translation_profile WHERE singleton=1", [])?;
            tracing::warn!(event = "shinki.emu.library.translation_profile_reset");
        }
    }
    Ok(())
}

fn rename_plugins(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    for (old, new) in RENAMED_PLUGINS {
        transaction.execute(
            "UPDATE family_configuration SET plugin_id=?2 WHERE plugin_id=?1",
            [old, new],
        )?;
    }
    let rows = {
        let mut statement =
            transaction.prepare("SELECT game_id, settings_json FROM game_settings")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (game_id, raw) in rows {
        let Ok(mut settings) = serde_json::from_str::<GameSettings>(&raw) else {
            continue;
        };
        let Some((_, new)) = RENAMED_PLUGINS
            .iter()
            .find(|(old, _)| settings.emulator_id.as_deref() == Some(*old))
        else {
            continue;
        };
        settings.emulator_id = Some((*new).to_owned());
        let encoded = serde_json::to_string(&settings).map_err(|_| LibraryError::Serialization)?;
        transaction.execute(
            "UPDATE game_settings SET settings_json=?2 WHERE game_id=?1",
            [&game_id, &encoded],
        )?;
    }
    Ok(())
}

struct ConfigurationRow {
    plugin_id: String,
    game_id: String,
    entries: Vec<ConfigEntry>,
    changed: bool,
}

fn take_render_extent(
    entries: &mut Vec<ConfigEntry>,
    width_id: &str,
    height_id: &str,
) -> Option<RenderPreference> {
    let integer = |id: &str| {
        entries
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| match entry.value {
                ConfigValue::Integer(value) => u32::try_from(value).ok(),
                _ => None,
            })
    };
    let width = integer(width_id);
    let height = integer(height_id);
    entries.retain(|entry| entry.id != width_id && entry.id != height_id);
    let preference = RenderPreference::Custom {
        width: width?,
        height: height?,
    };
    preference.validate().ok()?;
    Some(preference)
}

fn migrate_family_configuration(transaction: &Transaction<'_>) -> Result<(), LibraryError> {
    let mut rows = {
        let mut statement = transaction
            .prepare("SELECT plugin_id, game_id, values_json FROM family_configuration")?;
        let records = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        records
            .into_iter()
            .filter_map(|(plugin_id, game_id, raw)| {
                let entries = serde_json::from_str::<Vec<ConfigEntry>>(&raw).ok()?;
                Some(ConfigurationRow {
                    plugin_id,
                    game_id,
                    entries,
                    changed: false,
                })
            })
            .collect::<Vec<_>>()
    };

    for row in &mut rows {
        let before = row.entries.len();
        if row.plugin_id == FVP_PLUGIN {
            row.entries
                .retain(|entry| !FVP_REMOVED_FIELDS.contains(&entry.id.as_str()));
        }
        if let Some(&(plugin_id, width_id, height_id)) = RENDER_FIELD_PLUGINS
            .iter()
            .find(|(plugin_id, _, _)| *plugin_id == row.plugin_id)
        {
            if let Some(preference) = take_render_extent(&mut row.entries, width_id, height_id) {
                if row.game_id.is_empty() {
                    write_app_state(
                        transaction,
                        &format!("render.default.{plugin_id}"),
                        &preference,
                    )?;
                } else {
                    set_game_render(transaction, &row.game_id, preference)?;
                }
            }
        }
        row.changed = row.entries.len() != before;
    }

    let defaults = rows
        .iter()
        .filter(|row| row.game_id.is_empty())
        .map(|row| (row.plugin_id.clone(), row.entries.clone()))
        .collect::<BTreeMap<_, _>>();
    for row in rows.iter_mut().filter(|row| !row.game_id.is_empty()) {
        let Some(core) = defaults.get(&row.plugin_id) else {
            continue;
        };
        let before = row.entries.len();
        row.entries
            .retain(|entry| !core.iter().any(|default| default == entry));
        row.changed |= row.entries.len() != before;
    }

    for row in rows.iter().filter(|row| row.changed) {
        if row.entries.is_empty() {
            transaction.execute(
                "DELETE FROM family_configuration WHERE plugin_id=?1 AND game_id=?2",
                [&row.plugin_id, &row.game_id],
            )?;
        } else {
            let raw =
                serde_json::to_string(&row.entries).map_err(|_| LibraryError::Serialization)?;
            transaction.execute(
                "UPDATE family_configuration SET values_json=?3 WHERE plugin_id=?1 AND game_id=?2",
                [&row.plugin_id, &row.game_id, &raw],
            )?;
        }
    }
    Ok(())
}

fn set_game_render(
    transaction: &Transaction<'_>,
    game_id: &str,
    preference: RenderPreference,
) -> Result<(), LibraryError> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM library_game WHERE game_id=?1)",
        [game_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let raw: Option<String> = transaction
        .query_row(
            "SELECT settings_json FROM game_settings WHERE game_id=?1",
            [game_id],
            |row| row.get(0),
        )
        .optional()?;
    let mut settings = match raw {
        Some(raw) => match serde_json::from_str::<GameSettings>(&raw) {
            Ok(settings) => settings,
            Err(_) => return Ok(()),
        },
        None => GameSettings::default(),
    };
    settings.render = Some(preference);
    let encoded = serde_json::to_string(&settings).map_err(|_| LibraryError::Serialization)?;
    transaction.execute(
        "INSERT INTO game_settings(game_id, settings_json) VALUES(?1, ?2)
         ON CONFLICT(game_id) DO UPDATE SET settings_json=excluded.settings_json",
        [game_id, &encoded],
    )?;
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use rusqlite::Connection;

    use super::super::{GameRecord, Library, REQUIRED_TABLES, SCHEMA_SQL};
    use super::*;
    use shinki_emu_translation::OpenAiApi;

    const PLUGIN_INSTALLATION_SQL: &str = r#"
CREATE TABLE plugin_installation (
    plugin_id TEXT PRIMARY KEY NOT NULL,
    family_id TEXT NOT NULL,
    location TEXT NOT NULL UNIQUE,
    abi_fingerprint TEXT NOT NULL,
    version TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    supported_formats_json TEXT NOT NULL,
    installed_at_unix_ms INTEGER NOT NULL
);
"#;

    pub(in crate::library) fn schema_5_library(version: i64) -> Library {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection.execute_batch(SCHEMA_SQL).unwrap();
        connection.execute_batch(PLUGIN_INSTALLATION_SQL).unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        Library { connection }
    }

    fn add_game(library: &mut Library, game_id: &str) {
        library
            .add_game(&GameRecord {
                game_id: game_id.into(),
                title: game_id.into(),
                user_title: None,
                location: format!("location-{game_id}"),
                family_id: Some("musica".into()),
                content_fingerprint: None,
                added_at_unix_ms: 1,
            })
            .unwrap();
    }

    fn insert_row(library: &Library, plugin_id: &str, game_id: &str, entries: &[ConfigEntry]) {
        library
            .connection
            .execute(
                "INSERT INTO family_configuration(plugin_id, game_id, values_json) VALUES(?1, ?2, ?3)",
                [plugin_id, game_id, &serde_json::to_string(entries).unwrap()],
            )
            .unwrap();
    }

    fn entry(id: &str, value: ConfigValue) -> ConfigEntry {
        ConfigEntry {
            id: id.into(),
            value,
        }
    }

    fn enumerated(id: &str, value: &str) -> ConfigEntry {
        entry(id, ConfigValue::Enum(value.into()))
    }

    fn upgrade(library: Library) -> Library {
        Library::from_connection(library.connection).unwrap()
    }

    fn user_version(library: &Library) -> i64 {
        library
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn render_fields_become_preferences_and_game_rows_keep_only_individual_values() {
        let mut old = schema_5_library(5);
        add_game(&mut old, "g");
        add_game(&mut old, "plain");
        insert_row(
            &old,
            "shinki.emu.musica",
            "",
            &[
                enumerated("script_encoding", "shift_jis"),
                enumerated("launch_mode", "title"),
                entry("render_width", ConfigValue::Integer(1920)),
                entry("render_height", ConfigValue::Integer(1080)),
            ],
        );
        insert_row(
            &old,
            "shinki.emu.musica",
            "g",
            &[
                entry("render_width", ConfigValue::Integer(2560)),
                entry("render_height", ConfigValue::Integer(1440)),
                enumerated("script_encoding", "gbk"),
                enumerated("launch_mode", "title"),
            ],
        );
        insert_row(
            &old,
            "shinki.emu.musica",
            "plain",
            &[entry("render_width", ConfigValue::Integer(800))],
        );
        insert_row(
            &old,
            "shinki.emu.fvp",
            "",
            &[
                enumerated("surface", "cpu"),
                entry("render_scale", ConfigValue::Number(1.5)),
                enumerated("script_encoding", "gbk"),
            ],
        );
        insert_row(
            &old,
            "shinki.emu.fvp",
            "g",
            &[enumerated("surface", "d3d12")],
        );
        insert_row(
            &old,
            "sena-native-koikake",
            "",
            &[
                entry("raster_width", ConfigValue::Integer(1600)),
                entry("raster_height", ConfigValue::Integer(900)),
            ],
        );
        let settings = GameSettings {
            input_mapping: Some(crate::default_vn_preset()),
            emulator_id: Some("shinki.emu.fvp".into()),
            ..Default::default()
        };
        old.set_game_settings("g", &settings).unwrap();

        let library = upgrade(old);
        let migrated = library.game_settings("g").unwrap().unwrap();
        assert_eq!(
            migrated.render,
            Some(RenderPreference::Custom {
                width: 2560,
                height: 1440
            })
        );
        assert_eq!(migrated.input_mapping, settings.input_mapping);
        assert_eq!(migrated.emulator_id.as_deref(), Some("rfvp"));
        assert_eq!(
            library.family_overrides("shinki.emu.musica", "g").unwrap(),
            [enumerated("script_encoding", "gbk")]
        );
        assert_eq!(library.game_settings("plain").unwrap(), None);
        assert!(library
            .family_overrides("shinki.emu.musica", "plain")
            .unwrap()
            .is_empty());
        assert_eq!(
            library.family_overrides("shinki.emu.musica", "").unwrap(),
            [
                enumerated("script_encoding", "shift_jis"),
                enumerated("launch_mode", "title")
            ]
        );
        assert_eq!(
            library
                .app_state::<RenderPreference>("render.default.shinki.emu.musica")
                .unwrap(),
            Some(RenderPreference::Custom {
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(
            library
                .app_state::<RenderPreference>("render.default.sena-rs")
                .unwrap(),
            Some(RenderPreference::Custom {
                width: 1600,
                height: 900
            })
        );
        assert!(library.family_overrides("sena-rs", "").unwrap().is_empty());
        assert_eq!(
            library.family_overrides("rfvp", "").unwrap(),
            [enumerated("script_encoding", "gbk")]
        );
        assert!(library.family_overrides("rfvp", "g").unwrap().is_empty());
        assert_eq!(user_version(&library), 7);
        assert_eq!(library.user_table_count().unwrap(), REQUIRED_TABLES.len());
        assert_eq!(REQUIRED_TABLES.len(), 13);
        assert_eq!(
            library.app_state::<bool>("onboarding.completed").unwrap(),
            Some(true)
        );
    }

    #[test]
    fn ecnu_translation_profile_becomes_openai_with_base_url_preserved() {
        let old = schema_5_library(5);
        old.connection
            .execute(
                "INSERT INTO translation_profile(singleton, profile_json) VALUES(1, ?1)",
                [r#"{"profile_id":"translation.default","endpoint_kind":"ecnu","endpoint":"https://chat.ecnu.edu.cn/open/api/v1","protocol":"chat_completions","model":"ecnu-plus","target_language":"zh-CN","timeout_ms":15000,"secret_reference":"translation.default"}"#],
            )
            .unwrap();
        let library = upgrade(old);
        let profile = library.translation_profile().unwrap().unwrap();
        assert_eq!(profile.provider, TranslationProvider::OpenAi);
        assert_eq!(profile.base_url, "https://chat.ecnu.edu.cn/open/api/v1");
        assert_eq!(profile.openai_api, OpenAiApi::ChatCompletions);
        assert_eq!(profile.model, "ecnu-plus");
        assert_eq!(profile.secret_reference, "translation.default");
        assert_eq!(profile.custom_prompt, "");
        assert_eq!(profile.context_segments, 8);
        assert_eq!(profile.context_chars, 6_000);
    }

    #[test]
    fn unreadable_translation_profile_is_reset() {
        let old = schema_5_library(5);
        old.connection
            .execute(
                "INSERT INTO translation_profile(singleton, profile_json) VALUES(1, ?1)",
                [r#"{"profile_id":"x","endpoint":"not a url"}"#],
            )
            .unwrap();
        let library = upgrade(old);
        assert_eq!(library.translation_profile().unwrap(), None);
    }

    #[test]
    fn empty_library_upgrade_drops_legacy_table_without_onboarding_flag() {
        let library = upgrade(schema_5_library(5));
        assert_eq!(user_version(&library), 7);
        assert_eq!(library.user_table_count().unwrap(), 13);
        assert!(library.tables_present(&REQUIRED_TABLES).unwrap());
        assert!(!library.tables_present(&["plugin_installation"]).unwrap());
        assert_eq!(
            library.app_state::<bool>("onboarding.completed").unwrap(),
            None
        );
    }

    #[test]
    fn corrupt_schema_5_is_rejected_without_changes() {
        let old = schema_5_library(5);
        old.connection
            .execute_batch("DROP TABLE plugin_installation;")
            .unwrap();
        assert!(matches!(
            Library::from_connection(old.connection),
            Err(LibraryError::SchemaCorrupt)
        ));
    }

    /// Schema 4–6 `external_identity`, with the cross-game uniqueness that
    /// schema 7 drops.
    const LEGACY_IDENTITY_SQL: &str = "
        DROP TABLE external_identity;
        CREATE TABLE external_identity (
            game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            remote_id TEXT NOT NULL,
            provenance TEXT NOT NULL,
            linked_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY(game_id, provider),
            UNIQUE(provider, remote_id)
        );";

    /// A schema 4 or 5 database exactly as those versions created it.
    fn legacy_schema_5(version: i64) -> Library {
        let library = schema_5_library(version);
        library
            .connection
            .execute_batch(LEGACY_IDENTITY_SQL)
            .unwrap();
        library
    }

    /// A schema 6 database exactly as that version created it.
    fn legacy_schema_6() -> Library {
        legacy_schema_6_on(Connection::open_in_memory().unwrap())
    }

    fn legacy_schema_6_on(connection: Connection) -> Library {
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();
        connection.execute_batch(SCHEMA_SQL).unwrap();
        connection.execute_batch(LEGACY_IDENTITY_SQL).unwrap();
        let release = "release_id TEXT NOT NULL,\n    title TEXT NOT NULL DEFAULT ''";
        assert!(STATE_SCHEMA_SQL.contains(release));
        connection
            .execute_batch(&STATE_SCHEMA_SQL.replace(release, "release_id TEXT NOT NULL"))
            .unwrap();
        connection.pragma_update(None, "user_version", 6).unwrap();
        Library { connection }
    }

    /// Every column, index, foreign key and normalised `CREATE` statement.
    fn schema_shape(library: &Library) -> Vec<String> {
        let connection = &library.connection;
        let mut shape = Vec::new();
        let objects: Vec<(String, String, String)> = connection
            .prepare(
                "SELECT type, name, sql FROM sqlite_master
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (kind, name, sql) in objects {
            let normalised: String = sql
                .replace(&format!("\"{name}\""), &name)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace("( ", "(")
                .replace(" )", ")")
                // `ALTER TABLE ... ADD COLUMN` splices "<old> , <new>".
                .replace(" ,", ",");
            shape.push(format!("{kind} {name}: {normalised}"));
            if kind != "table" {
                continue;
            }
            for pragma in ["table_info", "index_list", "foreign_key_list"] {
                let rows: Vec<String> = connection
                    .prepare(&format!("SELECT * FROM pragma_{pragma}(?1)"))
                    .unwrap()
                    .query_map([&name], |row| {
                        let count = row.as_ref().column_count();
                        (0..count)
                            .map(|index| {
                                row.get::<_, rusqlite::types::Value>(index)
                                    .map(|value| format!("{value:?}"))
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map(|values| values.join("|"))
                    })
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap();
                for row in rows {
                    shape.push(format!("{name} {pragma}: {row}"));
                }
            }
        }
        shape
    }

    fn link(library: &mut Library, game_id: &str, remote_id: &str) {
        library
            .set_external_identity(&crate::ExternalIdentityRecord {
                game_id: game_id.into(),
                provider: "vndb".into(),
                remote_id: remote_id.into(),
                provenance: "manual".into(),
                linked_at_unix_ms: 2,
            })
            .unwrap();
        library
            .set_metadata_snapshot(&crate::MetadataSnapshotRecord {
                game_id: game_id.into(),
                provider: "vndb".into(),
                remote_id: remote_id.into(),
                metadata_json: format!(r#"{{"title":"{game_id}"}}"#),
                fetched_at_unix_ms: 3,
                state: crate::MetadataSnapshotState::Fresh,
            })
            .unwrap();
    }

    fn played(library: &mut Library, game_id: &str) {
        let session = library.start_play_session(game_id, 1_000).unwrap();
        library
            .end_play_session(&session, 3_500, crate::PlaySessionEndReason::Leave)
            .unwrap();
    }

    #[test]
    fn every_upgrade_path_matches_a_fresh_schema_7() {
        let fresh = schema_shape(&Library::in_memory().unwrap());
        assert!(fresh
            .iter()
            .any(|line| line.starts_with("table external_identity:") && !line.contains("UNIQUE")));
        for old in [legacy_schema_5(4), legacy_schema_5(5), legacy_schema_6()] {
            assert_ne!(schema_shape(&old), fresh);
            let upgraded = upgrade(old);
            assert_eq!(user_version(&upgraded), 7);
            let shape = schema_shape(&upgraded);
            let missing: Vec<_> = fresh.iter().filter(|line| !shape.contains(line)).collect();
            let extra: Vec<_> = shape.iter().filter(|line| !fresh.contains(line)).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "missing {missing:#?}\nextra {extra:#?}"
            );
            assert_eq!(shape, fresh);
        }
    }

    #[test]
    fn schema_6_upgrade_keeps_identities_metadata_releases_and_records() {
        let mut old = legacy_schema_6();
        add_game(&mut old, "retail");
        add_game(&mut old, "other");
        link(&mut old, "retail", "v1");
        link(&mut old, "other", "v2");
        old.connection
            .execute(
                "INSERT INTO game_release(game_id, release_id) VALUES('retail', 'r10')",
                [],
            )
            .unwrap();
        let settings = GameSettings {
            emulator_id: Some("rfvp".into()),
            ..Default::default()
        };
        old.set_game_settings("retail", &settings).unwrap();
        played(&mut old, "retail");
        let identities = old.external_identities("retail").unwrap();
        let snapshot = old.metadata_snapshot("other", "vndb").unwrap();

        let mut library = upgrade(old);
        assert_eq!(library.external_identities("retail").unwrap(), identities);
        assert_eq!(
            library.metadata_snapshot("other", "vndb").unwrap(),
            snapshot
        );
        let release = library.game_release("retail").unwrap().unwrap();
        assert_eq!(release.release_id, "r10");
        assert_eq!(release.title, "");
        assert_eq!(library.game_settings("retail").unwrap(), Some(settings));
        assert_eq!(
            library.play_stats("retail").unwrap().total_duration_ms,
            2_500
        );
        // The dropped constraint: another copy may now link the same work.
        add_game(&mut library, "download");
        link(&mut library, "download", "v1");
        assert_eq!(library.external_identities("retail").unwrap(), identities);
    }

    #[test]
    fn schema_5_upgrade_keeps_identities_and_records() {
        let mut old = legacy_schema_5(5);
        add_game(&mut old, "g");
        link(&mut old, "g", "v7");
        played(&mut old, "g");
        let identities = old.external_identities("g").unwrap();
        let snapshot = old.metadata_snapshot("g", "vndb").unwrap();
        let library = upgrade(old);
        assert_eq!(library.external_identities("g").unwrap(), identities);
        assert_eq!(library.metadata_snapshot("g", "vndb").unwrap(), snapshot);
        assert_eq!(library.play_stats("g").unwrap().session_count, 1);
    }

    #[test]
    fn failed_schema_6_upgrade_rolls_back_every_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.sqlite3");
        let mut old = legacy_schema_6_on(Connection::open(&path).unwrap());
        add_game(&mut old, "g");
        link(&mut old, "g", "v1");
        // The external_identity rebuild succeeds; the later game_release
        // column addition then fails.
        old.connection
            .execute_batch("ALTER TABLE game_release ADD COLUMN title TEXT;")
            .unwrap();
        let before = schema_shape(&old);
        let identities = old.external_identities("g").unwrap();
        drop(old);

        assert!(matches!(Library::open(&path), Err(LibraryError::Sqlite(_))));
        let unchanged = Library {
            connection: Connection::open(&path).unwrap(),
        };
        assert_eq!(user_version(&unchanged), 6);
        assert_eq!(schema_shape(&unchanged), before);
        assert_eq!(unchanged.external_identities("g").unwrap(), identities);
    }
}
