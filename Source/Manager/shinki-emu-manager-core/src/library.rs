//! Durable Manager data for games, play records, metadata, and settings.
//!
//! This database is independent from a running emulator. Schemas 4 to 6
//! upgrade in place; incompatible databases are left untouched.
//! Native game save files are never touched by this code.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, config::DbConfig, params};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use shinki_emu_translation::TranslationProfile;
use thiserror::Error;

use crate::{input_mapping::InputMapping, work_settings::GameSettings};

#[path = "library_settings.rs"]
mod settings;

#[path = "library_snapshot.rs"]
mod snapshot;

#[path = "library_state.rs"]
mod state;

#[path = "library_migration.rs"]
mod migration;

pub use snapshot::LibrarySnapshot;
pub use state::{ExcludedGame, GameRelease, LibraryRoot};

const SCHEMA_VERSION: i64 = 7;
const MAX_TITLE_CHARS: usize = 1_024;
const MAX_LOCATION_BYTES: usize = 4_096;
const MAX_METADATA_BYTES: usize = 1_048_576;

const REQUIRED_TABLES: [&str; 13] = [
    "family_configuration",
    "appearance_settings",
    "library_game",
    "external_identity",
    "metadata_snapshot",
    "play_session",
    "manager_settings",
    "game_settings",
    "translation_profile",
    "library_root",
    "excluded_game",
    "game_release",
    "app_state",
];

const SCHEMA_SQL: &str = r#"
CREATE TABLE family_configuration (
    plugin_id TEXT NOT NULL,
    game_id TEXT NOT NULL,
    values_json TEXT NOT NULL,
    PRIMARY KEY(plugin_id, game_id)
);
CREATE TABLE appearance_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    settings_json TEXT NOT NULL
);
CREATE TABLE library_game (
    game_id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    user_title TEXT,
    location TEXT NOT NULL UNIQUE,
    family_id TEXT,
    content_fingerprint TEXT,
    added_at_unix_ms INTEGER NOT NULL
);
CREATE INDEX library_game_title ON library_game(title, game_id);
CREATE TABLE external_identity (
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    provenance TEXT NOT NULL,
    linked_at_unix_ms INTEGER NOT NULL,
    PRIMARY KEY(game_id, provider)
);
CREATE TABLE metadata_snapshot (
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    fetched_at_unix_ms INTEGER NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY(game_id, provider)
);
CREATE TABLE play_session (
    session_id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    start_unix_ms INTEGER NOT NULL,
    end_unix_ms INTEGER,
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK(duration_ms >= 0),
    ended_by TEXT NOT NULL DEFAULT 'active'
);
CREATE INDEX play_session_game_start ON play_session(game_id, start_unix_ms);
CREATE UNIQUE INDEX one_active_play_session ON play_session((1)
) WHERE end_unix_ms IS NULL;
CREATE TABLE manager_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    input_mapping_json TEXT NOT NULL,
    filter_settings_json TEXT NOT NULL
);
CREATE TABLE game_settings (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    settings_json TEXT NOT NULL
);
CREATE TABLE translation_profile (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    profile_json TEXT NOT NULL
);
"#;

/// Tables introduced by schema 6 in their current form; also applied by the
/// schema-5 upgrade.
const STATE_SCHEMA_SQL: &str = r#"
CREATE TABLE library_root (
    path TEXT PRIMARY KEY NOT NULL,
    added_at_unix_ms INTEGER NOT NULL
);
CREATE TABLE excluded_game (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    excluded_at_unix_ms INTEGER NOT NULL
);
CREATE TABLE game_release (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES library_game(game_id) ON DELETE CASCADE,
    release_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT ''
);
CREATE TABLE app_state (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL
);
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameRecord {
    pub game_id: String,
    pub title: String,
    pub user_title: Option<String>,
    pub location: String,
    pub family_id: Option<String>,
    pub content_fingerprint: Option<String>,
    pub added_at_unix_ms: i64,
}

impl GameRecord {
    pub fn display_title(&self) -> &str {
        self.user_title.as_deref().unwrap_or(&self.title)
    }
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("{0}")]
    FamilyConfiguration(crate::model::FamilyError),
    #[error("SHINKI_EMU_LIBRARY_SQLITE: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("SHINKI_EMU_LIBRARY_SCHEMA_CORRUPT")]
    SchemaCorrupt,
    #[error("SHINKI_EMU_LIBRARY_SCHEMA_INCOMPATIBLE")]
    SchemaIncompatible,
    #[error("SHINKI_EMU_LIBRARY_INVALID_ID")]
    InvalidId,
    #[error("SHINKI_EMU_LIBRARY_INVALID_TITLE")]
    InvalidTitle,
    #[error("SHINKI_EMU_LIBRARY_INVALID_LOCATION")]
    InvalidLocation,
    #[error("SHINKI_EMU_LIBRARY_INVALID_METADATA")]
    InvalidMetadata,
    #[error("SHINKI_EMU_LIBRARY_GAME_NOT_FOUND")]
    GameNotFound,
    #[error("SHINKI_EMU_RELEASE_ID_INVALID")]
    InvalidReleaseId,
    #[error("SHINKI_EMU_LIBRARY_ACTIVE_SESSION")]
    ActiveSession,
    #[error("SHINKI_EMU_LIBRARY_SERIALIZATION")]
    Serialization,
    #[error("SHINKI_EMU_LIBRARY_SETTINGS")]
    Settings,
}

pub struct Library {
    pub(crate) connection: Connection,
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LibraryError> {
        let path = path.as_ref();
        if path.exists() {
            // A rejected database must not have its pending WAL checkpointed.
            let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
            Self { connection }.compatible_schema_version()?;
        }
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self, LibraryError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, LibraryError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let mut library = Self { connection };
        library.ensure_schema()?;
        Ok(library)
    }

    fn compatible_schema_version(&self) -> Result<i64, LibraryError> {
        let application_id: i64 =
            self.connection
                .pragma_query_value(None, "application_id", |row| row.get(0))?;
        if application_id != 0 {
            return Err(LibraryError::SchemaIncompatible);
        }
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        let required = match version {
            SCHEMA_VERSION | 6 => &REQUIRED_TABLES[..],
            4 | 5 => &migration::SCHEMA_5_TABLES[..],
            0 => {
                let empty: bool = self.connection.query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM sqlite_schema)",
                    [],
                    |row| row.get(0),
                )?;
                let pages: i64 = self
                    .connection
                    .pragma_query_value(None, "page_count", |row| row.get(0))?;
                if empty && pages <= 1 {
                    return Ok(0);
                }
                return Err(LibraryError::SchemaIncompatible);
            }
            _ => return Err(LibraryError::SchemaIncompatible),
        };
        if self.user_table_count()? != required.len() || !self.tables_present(required)? {
            return Err(LibraryError::SchemaCorrupt);
        }
        Ok(version)
    }

    fn ensure_schema(&mut self) -> Result<(), LibraryError> {
        let version = self.compatible_schema_version()?;
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        if version == 4 || version == 5 {
            if version == 4 {
                transaction.execute(
                    "UPDATE library_game SET family_id='softpal' WHERE family_id='sena'",
                    [],
                )?;
            }
            migration::upgrade_schema_5(&transaction)?;
            migration::rebuild_external_identity(&transaction)?;
        } else if version == 6 {
            migration::upgrade_schema_6(&transaction)?;
        } else {
            transaction.execute_batch(SCHEMA_SQL)?;
            transaction.execute_batch(STATE_SCHEMA_SQL)?;
        }
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
        Ok(())
    }

    fn user_table_count(&self) -> Result<usize, LibraryError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )?;
        usize::try_from(count).map_err(|_| LibraryError::SchemaCorrupt)
    }

    fn tables_present(&self, tables: &[&str]) -> Result<bool, LibraryError> {
        for table in tables {
            let present: bool = self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )?;
            if !present {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn add_game(&mut self, game: &GameRecord) -> Result<(), LibraryError> {
        validate_game(game)?;
        self.connection.execute(
            "INSERT INTO library_game(
                game_id, title, user_title, location, family_id,
                content_fingerprint, added_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(game_id) DO UPDATE SET
                title=excluded.title,
                user_title=excluded.user_title,
                location=excluded.location,
                family_id=excluded.family_id,
                content_fingerprint=excluded.content_fingerprint",
            params![
                game.game_id,
                game.title,
                game.user_title,
                game.location,
                game.family_id,
                game.content_fingerprint,
                game.added_at_unix_ms,
            ],
        )?;
        Ok(())
    }

    pub fn list_games(&self) -> Result<Vec<GameRecord>, LibraryError> {
        let mut statement = self.connection.prepare(
            "SELECT game_id, title, user_title, location, family_id,
                    content_fingerprint, added_at_unix_ms
             FROM library_game ORDER BY title COLLATE NOCASE, game_id",
        )?;
        let records = statement
            .query_map([], game_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn game(&self, game_id: &str) -> Result<Option<GameRecord>, LibraryError> {
        validate_id(game_id)?;
        self.connection
            .query_row(
                "SELECT game_id, title, user_title, location, family_id,
                        content_fingerprint, added_at_unix_ms
                 FROM library_game WHERE game_id=?1",
                [game_id],
                game_from_row,
            )
            .optional()
            .map_err(LibraryError::from)
    }

    pub fn remove_game(&mut self, game_id: &str) -> Result<bool, LibraryError> {
        validate_id(game_id)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM family_configuration WHERE game_id=?1",
            [game_id],
        )?;
        let removed =
            transaction.execute("DELETE FROM library_game WHERE game_id=?1", [game_id])? > 0;
        transaction.commit()?;
        Ok(removed)
    }

    pub fn set_game_user_title(
        &mut self,
        game_id: &str,
        user_title: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if let Some(title) = user_title {
            validate_title(title)?;
        }
        let changed = self.connection.execute(
            "UPDATE library_game SET user_title=?2 WHERE game_id=?1",
            params![game_id, user_title],
        )?;
        if changed == 0 {
            return Err(LibraryError::GameNotFound);
        }
        Ok(())
    }

    pub fn set_game_family(
        &mut self,
        game_id: &str,
        family_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        validate_id(game_id)?;
        if let Some(family_id) = family_id {
            validate_id(family_id)?;
        }
        let changed = self.connection.execute(
            "UPDATE library_game SET family_id=?2 WHERE game_id=?1",
            params![game_id, family_id],
        )?;
        if changed == 0 {
            return Err(LibraryError::GameNotFound);
        }
        Ok(())
    }

    pub(crate) fn prepare_game_emulator(
        &self,
        game_id: &str,
        engine_id: &str,
        emulator_id: &str,
    ) -> Result<String, LibraryError> {
        validate_id(game_id)?;
        validate_id(engine_id)?;
        validate_id(emulator_id)?;
        let mut settings = self.game_settings(game_id)?.unwrap_or_default();
        settings.emulator_id = Some(emulator_id.to_owned());
        settings.validate().map_err(|_| LibraryError::Settings)?;
        serde_json::to_string(&settings).map_err(|_| LibraryError::Serialization)
    }

    /// Bind the detected engine and selected emulator atomically, retaining other overrides.
    pub fn select_game_emulator(
        &mut self,
        game_id: &str,
        engine_id: &str,
        emulator_id: &str,
    ) -> Result<(), LibraryError> {
        let encoded = self.prepare_game_emulator(game_id, engine_id, emulator_id)?;
        let transaction = self.connection.transaction()?;
        write_game_emulator(&transaction, game_id, engine_id, &encoded)?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn write_game_emulator(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &str,
    engine_id: &str,
    encoded: &str,
) -> Result<(), LibraryError> {
    if transaction.execute(
        "UPDATE library_game SET family_id=?2 WHERE game_id=?1",
        params![game_id, engine_id],
    )? == 0
    {
        return Err(LibraryError::GameNotFound);
    }
    transaction.execute(
        "INSERT INTO game_settings(game_id, settings_json) VALUES(?1, ?2)
         ON CONFLICT(game_id) DO UPDATE SET settings_json=excluded.settings_json",
        params![game_id, encoded],
    )?;
    Ok(())
}

fn game_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameRecord> {
    Ok(GameRecord {
        game_id: row.get(0)?,
        title: row.get(1)?,
        user_title: row.get(2)?,
        location: row.get(3)?,
        family_id: row.get(4)?,
        content_fingerprint: row.get(5)?,
        added_at_unix_ms: row.get(6)?,
    })
}

fn validate_game(game: &GameRecord) -> Result<(), LibraryError> {
    validate_id(&game.game_id)?;
    validate_title(&game.title)?;
    if let Some(user_title) = &game.user_title {
        validate_title(user_title)?;
    }
    validate_location(&game.location)?;
    if let Some(family_id) = &game.family_id {
        validate_id(family_id)?;
    }
    if game
        .content_fingerprint
        .as_deref()
        .is_some_and(|value| value.is_empty() || value.len() > 256 || value.contains('\0'))
    {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

pub(crate) fn validate_id(value: &str) -> Result<(), LibraryError> {
    if !is_safe_identifier(value) {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn validate_title(value: &str) -> Result<(), LibraryError> {
    if value.trim().is_empty() || value.chars().count() > MAX_TITLE_CHARS || value.contains('\0') {
        return Err(LibraryError::InvalidTitle);
    }
    Ok(())
}

fn validate_location(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > MAX_LOCATION_BYTES || value.contains('\0') {
        return Err(LibraryError::InvalidLocation);
    }
    Ok(())
}

pub(crate) fn validate_metadata(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > MAX_METADATA_BYTES || value.contains('\0') {
        return Err(LibraryError::InvalidMetadata);
    }
    Ok(())
}

pub(crate) fn validate_provider(value: &str) -> Result<(), LibraryError> {
    validate_id(value)
}

pub(crate) fn validate_remote_id(value: &str) -> Result<(), LibraryError> {
    if value.is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(LibraryError::InvalidId);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_mapping::default_vn_preset;

    fn game(id: &str, location: &str) -> GameRecord {
        GameRecord {
            game_id: id.into(),
            title: id.into(),
            user_title: None,
            location: location.into(),
            family_id: Some("fvp".into()),
            content_fingerprint: None,
            added_at_unix_ms: 1,
        }
    }

    #[test]
    fn incompatible_database_is_rejected_without_rewriting_it() {
        let root = tempfile::tempdir().unwrap();
        for (name, sql) in [
            (
                "newer",
                "CREATE TABLE library_game(title TEXT);
                 INSERT INTO library_game VALUES ('kept');
                 PRAGMA user_version=11;",
            ),
            ("foreign-view", "CREATE VIEW owner_view AS SELECT 'kept';"),
            ("foreign-id", "PRAGMA application_id=12345;"),
            (
                "foreign-freelist",
                "CREATE TABLE old_data(title TEXT); DROP TABLE old_data;",
            ),
        ] {
            let path = root.path().join(name);
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(sql).unwrap();
            drop(connection);
            let before = std::fs::read(&path).unwrap();
            assert!(matches!(
                Library::open(&path),
                Err(LibraryError::SchemaIncompatible)
            ));
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn incompatible_database_open_does_not_checkpoint_its_wal() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.sqlite3");
        let wal = root.path().join("library.sqlite3-wal");
        let connection = Connection::open(&path).unwrap();
        connection
            .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE newer_schema(title TEXT);
                 INSERT INTO newer_schema VALUES ('kept in WAL');
                 PRAGMA user_version=11;",
            )
            .unwrap();
        drop(connection);
        let before_database = std::fs::read(&path).unwrap();
        let before_wal = std::fs::read(&wal).unwrap();
        assert!(matches!(
            Library::open(&path),
            Err(LibraryError::SchemaIncompatible)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before_database);
        assert_eq!(std::fs::read(&wal).unwrap(), before_wal);
    }

    #[test]
    fn duplicate_locations_are_rejected_and_native_location_is_untouched() {
        let mut library = Library::in_memory().unwrap();
        library.add_game(&game("game-a", "same-location")).unwrap();
        assert!(library.add_game(&game("game-b", "same-location")).is_err());
        assert!(library.remove_game("game-a").unwrap());
        assert!(library.game("game-a").unwrap().is_none());
    }

    #[test]
    fn schema_four_renames_engine_without_losing_library_or_settings() {
        let mut old = migration::tests::schema_5_library(4);
        let mut record = game("softpal-game", "existing-installation");
        record.family_id = Some("sena".into());
        record.user_title = Some("My title".into());
        old.add_game(&record).unwrap();
        let settings = GameSettings {
            input_mapping: Some(default_vn_preset()),
            ..Default::default()
        };
        old.set_game_settings(&record.game_id, &settings).unwrap();
        let upgraded = Library::from_connection(old.connection).unwrap();
        let saved = upgraded.game(&record.game_id).unwrap().unwrap();
        assert_eq!(saved.family_id.as_deref(), Some("softpal"));
        assert_eq!(saved.user_title, record.user_title);
        assert_eq!(saved.location, record.location);
        assert_eq!(
            upgraded.game_settings(&record.game_id).unwrap(),
            Some(settings)
        );
    }

    #[test]
    fn emulator_choices_are_per_game_durable_and_preserve_input_overrides() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("library.sqlite3");
        let settings = GameSettings {
            input_mapping: Some(default_vn_preset()),
            ..Default::default()
        };
        {
            let mut library = Library::open(&path).unwrap();
            library.add_game(&game("one", "location-one")).unwrap();
            library.add_game(&game("two", "location-two")).unwrap();
            library.set_game_settings("one", &settings).unwrap();
            library
                .select_game_emulator("one", "softpal", "shinki.emu.sena")
                .unwrap();
            library
                .select_game_emulator("two", "softpal", "alternate.softpal")
                .unwrap();
            assert!(library.select_game_emulator("one", "softpal", "").is_err());
            assert!(matches!(
                library.select_game_emulator("absent", "softpal", "shinki.emu.sena"),
                Err(LibraryError::GameNotFound)
            ));
        }
        let library = Library::open(path).unwrap();
        let one = library.game_settings("one").unwrap().unwrap();
        assert_eq!(one.emulator_id.as_deref(), Some("shinki.emu.sena"));
        assert_eq!(one.input_mapping, settings.input_mapping);
        assert_eq!(
            library
                .game_settings("two")
                .unwrap()
                .unwrap()
                .emulator_id
                .as_deref(),
            Some("alternate.softpal")
        );
        assert_eq!(
            library.game("one").unwrap().unwrap().family_id.as_deref(),
            Some("softpal")
        );
        assert_eq!(
            library.game("two").unwrap().unwrap().family_id.as_deref(),
            Some("softpal")
        );
    }
}
