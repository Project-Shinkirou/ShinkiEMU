//! Local metadata records retained by the Manager.

use rusqlite::OptionalExtension;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::library::{
    Library, LibraryError, validate_metadata, validate_provider, validate_remote_id,
};

/// Provider whose works own the release pinned in `game_release`.
const VNDB_PROVIDER: &str = "vndb";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayTitle {
    pub value: String,
    pub source: DisplayTitleSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTitleSource {
    User,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalIdentityRecord {
    pub game_id: String,
    pub provider: String,
    pub remote_id: String,
    pub provenance: String,
    pub linked_at_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSnapshotState {
    Fresh,
    Stale,
    Failed,
}

impl MetadataSnapshotState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, LibraryError> {
        match value {
            "fresh" => Ok(Self::Fresh),
            "stale" => Ok(Self::Stale),
            "failed" => Ok(Self::Failed),
            _ => Err(LibraryError::InvalidMetadata),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MetadataSnapshotRecord {
    pub game_id: String,
    pub provider: String,
    pub remote_id: String,
    pub metadata_json: String,
    pub fetched_at_unix_ms: i64,
    pub state: MetadataSnapshotState,
}

impl Library {
    pub fn display_title(&self, game_id: &str) -> Result<Option<DisplayTitle>, LibraryError> {
        let Some(game) = self.game(game_id)? else {
            return Ok(None);
        };
        let (value, source) = match game.user_title {
            Some(title) => (title, DisplayTitleSource::User),
            None => (game.title, DisplayTitleSource::Local),
        };
        Ok(Some(DisplayTitle { value, source }))
    }

    pub fn set_external_identity(
        &mut self,
        record: &ExternalIdentityRecord,
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(&record.game_id)?;
        validate_provider(&record.provider)?;
        validate_remote_id(&record.remote_id)?;
        validate_provider(&record.provenance)?;
        if self.game(&record.game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO external_identity(
                game_id, provider, remote_id, provenance, linked_at_unix_ms)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(game_id, provider) DO UPDATE SET
                remote_id=excluded.remote_id,
                provenance=excluded.provenance,
                linked_at_unix_ms=excluded.linked_at_unix_ms",
            rusqlite::params![
                record.game_id,
                record.provider,
                record.remote_id,
                record.provenance,
                record.linked_at_unix_ms,
            ],
        )?;
        Ok(())
    }

    pub fn external_identities(
        &self,
        game_id: &str,
    ) -> Result<Vec<ExternalIdentityRecord>, LibraryError> {
        crate::library::validate_id(game_id)?;
        let mut statement = self.connection.prepare(
            "SELECT game_id, provider, remote_id, provenance, linked_at_unix_ms
             FROM external_identity WHERE game_id=?1 ORDER BY provider",
        )?;
        let records = statement
            .query_map([game_id], external_identity_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn remove_external_identity(
        &mut self,
        game_id: &str,
        provider: &str,
    ) -> Result<bool, LibraryError> {
        crate::library::validate_id(game_id)?;
        validate_provider(provider)?;
        Ok(self.connection.execute(
            "DELETE FROM external_identity WHERE game_id=?1 AND provider=?2",
            rusqlite::params![game_id, provider],
        )? > 0)
    }

    /// Link `target` to the work `source` is linked to for `provider`, with
    /// a copy of the cached metadata. Several installed copies may share one
    /// remote work; each keeps its own game id, settings and records. The
    /// target's pinned VNDB release is kept only while its work is unchanged,
    /// and the source's pin is never copied. Returns `false` and changes
    /// nothing when `source` has no identity for `provider`.
    pub fn inherit_external_identity(
        &mut self,
        source_game_id: &str,
        target_game_id: &str,
        provider: &str,
        linked_at_unix_ms: i64,
    ) -> Result<bool, LibraryError> {
        crate::library::validate_id(source_game_id)?;
        crate::library::validate_id(target_game_id)?;
        validate_provider(provider)?;
        if source_game_id == target_game_id {
            return Err(LibraryError::InvalidId);
        }
        if self.game(target_game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        let transaction = self.connection.transaction()?;
        let source: Option<String> = transaction
            .query_row(
                "SELECT remote_id FROM external_identity WHERE game_id=?1 AND provider=?2",
                rusqlite::params![source_game_id, provider],
                |row| row.get(0),
            )
            .optional()?;
        let Some(remote_id) = source else {
            return Ok(false);
        };
        let previous: Option<String> = transaction
            .query_row(
                "SELECT remote_id FROM external_identity WHERE game_id=?1 AND provider=?2",
                rusqlite::params![target_game_id, provider],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute(
            "INSERT INTO external_identity(
                game_id, provider, remote_id, provenance, linked_at_unix_ms)
             VALUES(?1, ?2, ?3, 'inherited', ?4)
             ON CONFLICT(game_id, provider) DO UPDATE SET
                remote_id=excluded.remote_id,
                provenance=excluded.provenance,
                linked_at_unix_ms=excluded.linked_at_unix_ms",
            rusqlite::params![target_game_id, provider, remote_id, linked_at_unix_ms],
        )?;
        transaction.execute(
            "DELETE FROM metadata_snapshot WHERE game_id=?1 AND provider=?2",
            rusqlite::params![target_game_id, provider],
        )?;
        transaction.execute(
            "INSERT INTO metadata_snapshot(
                game_id, provider, remote_id, metadata_json, fetched_at_unix_ms, state)
             SELECT ?1, provider, remote_id, metadata_json, fetched_at_unix_ms, state
             FROM metadata_snapshot
             WHERE game_id=?2 AND provider=?3 AND remote_id=?4",
            rusqlite::params![target_game_id, source_game_id, provider, remote_id],
        )?;
        if provider == VNDB_PROVIDER && previous.as_deref() != Some(remote_id.as_str()) {
            transaction.execute(
                "DELETE FROM game_release WHERE game_id=?1",
                [target_game_id],
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }

    pub fn set_metadata_snapshot(
        &mut self,
        record: &MetadataSnapshotRecord,
    ) -> Result<(), LibraryError> {
        crate::library::validate_id(&record.game_id)?;
        validate_provider(&record.provider)?;
        validate_remote_id(&record.remote_id)?;
        validate_metadata(&record.metadata_json)?;
        if serde_json::from_str::<serde_json::Value>(&record.metadata_json).is_err() {
            return Err(LibraryError::InvalidMetadata);
        }
        if self.game(&record.game_id)?.is_none() {
            return Err(LibraryError::GameNotFound);
        }
        self.connection.execute(
            "INSERT INTO metadata_snapshot(
                game_id, provider, remote_id, metadata_json, fetched_at_unix_ms, state)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(game_id, provider) DO UPDATE SET
                remote_id=excluded.remote_id,
                metadata_json=excluded.metadata_json,
                fetched_at_unix_ms=excluded.fetched_at_unix_ms,
                state=excluded.state",
            rusqlite::params![
                record.game_id,
                record.provider,
                record.remote_id,
                record.metadata_json,
                record.fetched_at_unix_ms,
                record.state.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn metadata_snapshot(
        &self,
        game_id: &str,
        provider: &str,
    ) -> Result<Option<MetadataSnapshotRecord>, LibraryError> {
        crate::library::validate_id(game_id)?;
        validate_provider(provider)?;
        self.connection
            .query_row(
                "SELECT game_id, provider, remote_id, metadata_json,
                        fetched_at_unix_ms, state
                 FROM metadata_snapshot WHERE game_id=?1 AND provider=?2",
                rusqlite::params![game_id, provider],
                metadata_snapshot_from_row,
            )
            .optional()
            .map_err(LibraryError::from)
    }
}

pub(crate) fn external_identity_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ExternalIdentityRecord> {
    Ok(ExternalIdentityRecord {
        game_id: row.get(0)?,
        provider: row.get(1)?,
        remote_id: row.get(2)?,
        provenance: row.get(3)?,
        linked_at_unix_ms: row.get(4)?,
    })
}

pub(crate) fn metadata_snapshot_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MetadataSnapshotRecord> {
    let state: String = row.get(5)?;
    Ok(MetadataSnapshotRecord {
        game_id: row.get(0)?,
        provider: row.get(1)?,
        remote_id: row.get(2)?,
        metadata_json: row.get(3)?,
        fetched_at_unix_ms: row.get(4)?,
        state: MetadataSnapshotState::parse(&state).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "metadata state",
                )),
            )
        })?,
    })
}
