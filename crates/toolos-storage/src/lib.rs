use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use thiserror::Error;
use toolos_domain::{EventRecord, EvidenceRecord};
use uuid::Uuid;

const MIGRATION_SLICE: &[M<'_>] = &[
    M::up(
        "CREATE TABLE event_log (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT,
            trace_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE INDEX event_log_trace_idx ON event_log(trace_id, sequence);",
    ),
    M::up(
        "CREATE TABLE evidence (
            id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            scope TEXT NOT NULL,
            claim TEXT NOT NULL,
            provider TEXT NOT NULL,
            observed_at TEXT NOT NULL,
            record_json TEXT NOT NULL,
            content_sha256 TEXT NOT NULL
        );
        CREATE INDEX evidence_observed_idx ON evidence(observed_at DESC);
        CREATE INDEX evidence_trace_idx ON evidence(trace_id, observed_at DESC);",
    ),
    M::up(
        "CREATE TABLE daemon_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    ),
];
const MIGRATIONS: Migrations<'_> = Migrations::from_slice(MIGRATION_SLICE);

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid UUID in database: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("invalid timestamp in database: {0}")]
    Timestamp(#[from] chrono::ParseError),
}

#[derive(Debug, Clone)]
pub struct Storage {
    path: PathBuf,
}

impl Storage {
    pub fn initialize(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let storage = Self { path };
        let mut connection = storage.open_connection()?;
        MIGRATIONS.to_latest(&mut connection)?;
        Ok(storage)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record_evidence(&self, record: &EvidenceRecord) -> Result<(), StorageError> {
        let connection = self.open_connection()?;
        let record_json = serde_json::to_string(record)?;
        connection.execute(
            "INSERT INTO evidence (
                id, trace_id, kind, scope, claim, provider, observed_at, record_json, content_sha256
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id.to_string(),
                record.trace_id.to_string(),
                format!("{:?}", record.kind),
                record.scope,
                record.claim,
                record.provider,
                record.observed_at.to_rfc3339(),
                record_json,
                record.content_sha256,
            ],
        )?;
        Ok(())
    }

    pub fn list_evidence(&self, limit: usize) -> Result<Vec<EvidenceRecord>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM evidence ORDER BY observed_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            row.get::<_, String>(0)
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(serde_json::from_str(&row?)?);
        }
        Ok(records)
    }

    pub fn append_event(
        &self,
        trace_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, StorageError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO event_log (trace_id, event_type, occurred_at, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                trace_id.to_string(),
                event_type,
                Utc::now().to_rfc3339(),
                serde_json::to_string(payload)?,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn replay_events(&self, limit: usize) -> Result<Vec<EventRecord>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            "SELECT sequence, trace_id, event_type, occurred_at, payload_json
             FROM event_log ORDER BY sequence DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (sequence, trace_id, event_type, occurred_at, payload_json) = row?;
            events.push(EventRecord {
                sequence,
                trace_id: Uuid::parse_str(&trace_id)?,
                event_type,
                occurred_at: DateTime::parse_from_rfc3339(&occurred_at)?.with_timezone(&Utc),
                payload_json,
            });
        }
        events.reverse();
        Ok(events)
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO daemon_metadata (key, value, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, StorageError> {
        let connection = self.open_connection()?;
        Ok(connection
            .query_row(
                "SELECT value FROM daemon_metadata WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn open_connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }
}

pub fn validate_migrations() -> Result<(), StorageError> {
    MIGRATIONS.validate()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;
    use toolos_domain::EvidenceKind;

    #[test]
    fn migrations_are_valid() {
        validate_migrations().expect("valid migrations");
    }

    #[test]
    fn evidence_and_events_round_trip() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let trace_id = Uuid::new_v4();
        let evidence = EvidenceRecord::new(
            trace_id,
            EvidenceKind::MachineInventory,
            "machine",
            "test observation",
            "test-provider",
            json!({"ok": true}),
            vec!["test limitation".to_owned()],
        )
        .expect("evidence");
        storage.record_evidence(&evidence).expect("record evidence");
        storage
            .append_event(trace_id, "test.event", &json!({"evidence_id": evidence.id}))
            .expect("append event");

        let records = storage.list_evidence(10).expect("list evidence");
        assert_eq!(records, vec![evidence]);
        let events = storage.replay_events(10).expect("replay events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "test.event");
    }
}
