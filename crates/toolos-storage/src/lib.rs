use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use thiserror::Error;
use toolos_actions::{ActionStatus, WingetActionPlan};
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
    M::up(
        "CREATE TABLE action_plan (
            id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            action_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            plan_json TEXT NOT NULL
        );
        CREATE INDEX action_plan_updated_idx ON action_plan(updated_at DESC);
        CREATE INDEX action_plan_status_idx ON action_plan(status, expires_at);",
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
    #[error(
        "action plan state conflict: expected {expected}, actual state changed or plan missing"
    )]
    ActionPlanConflict { expected: String },
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
        let mut statement = connection
            .prepare("SELECT record_json FROM evidence ORDER BY observed_at DESC LIMIT ?1")?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            row.get::<_, String>(0)
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(serde_json::from_str(&row?)?);
        }
        Ok(records)
    }

    pub fn create_action_plan(&self, plan: &WingetActionPlan) -> Result<(), StorageError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO action_plan (
                id, trace_id, action_kind, status, created_at, expires_at, updated_at, plan_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                plan.id.to_string(),
                plan.trace_id.to_string(),
                format!("{:?}", plan.kind),
                plan.status.storage_value(),
                plan.created_at.to_rfc3339(),
                plan.expires_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
                serde_json::to_string(plan)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_action_plan(&self, id: Uuid) -> Result<Option<WingetActionPlan>, StorageError> {
        let connection = self.open_connection()?;
        let json = connection
            .query_row(
                "SELECT plan_json FROM action_plan WHERE id = ?1",
                [id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(StorageError::from)
    }

    pub fn list_action_plans(&self, limit: usize) -> Result<Vec<WingetActionPlan>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare("SELECT plan_json FROM action_plan ORDER BY updated_at DESC LIMIT ?1")?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            row.get::<_, String>(0)
        })?;
        let mut plans = Vec::new();
        for row in rows {
            plans.push(serde_json::from_str(&row?)?);
        }
        Ok(plans)
    }

    pub fn replace_action_plan(
        &self,
        expected_status: ActionStatus,
        plan: &WingetActionPlan,
    ) -> Result<(), StorageError> {
        let mut connection = self.open_connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE action_plan
             SET status = ?1, updated_at = ?2, expires_at = ?3, plan_json = ?4
             WHERE id = ?5 AND status = ?6",
            params![
                plan.status.storage_value(),
                Utc::now().to_rfc3339(),
                plan.expires_at.to_rfc3339(),
                serde_json::to_string(plan)?,
                plan.id.to_string(),
                expected_status.storage_value(),
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::ActionPlanConflict {
                expected: expected_status.storage_value().to_owned(),
            });
        }
        transaction.commit()?;
        Ok(())
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
    use toolos_actions::{
        approve_plan, create_install_plan, ApprovalAcknowledgements, ApprovalRequest,
    };
    use toolos_domain::EvidenceKind;
    use toolos_winget::{
        identity_probe, install_preview, uninstall_preview, PackageScope, PackageSelector,
        ProcessEvidence, ResolutionStatus, WingetResolutionReport,
    };

    fn resolution() -> WingetResolutionReport {
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: None,
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        };
        WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: ResolutionStatus::ResolvedExact,
            selector: selector.clone(),
            identity_probe: identity_probe(&selector),
            identity_evidence: Some(ProcessEvidence {
                executable: "winget".to_owned(),
                args: vec!["show".to_owned()],
                exit_code: Some(0),
                stdout: "resolved".to_owned(),
                stderr: String::new(),
                timed_out: false,
                duration_ms: 1,
            }),
            install_preview: install_preview(&selector),
            uninstall_preview: uninstall_preview(&selector),
            observed_at: Utc::now(),
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        }
    }

    #[test]
    fn migrations_are_valid() {
        validate_migrations().expect("valid migrations");
    }

    #[test]
    fn evidence_events_and_action_plans_round_trip() {
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

        let plan = create_install_plan(trace_id, resolution(), Utc::now()).expect("plan");
        storage.create_action_plan(&plan).expect("create plan");
        assert_eq!(
            storage.get_action_plan(plan.id).expect("get plan"),
            Some(plan.clone())
        );

        let approved = approve_plan(
            plan.clone(),
            &ApprovalRequest {
                plan_id: plan.id,
                confirmation_phrase: plan.confirmation_phrase.clone(),
                acknowledgements: ApprovalAcknowledgements {
                    reviewed_exact_identity: true,
                    accepts_declared_write_scope: true,
                    understands_no_automatic_rollback: true,
                },
            },
            Utc::now(),
        )
        .expect("approve");
        storage
            .replace_action_plan(ActionStatus::WaitingApproval, &approved)
            .expect("replace plan");
        assert_eq!(
            storage
                .get_action_plan(plan.id)
                .expect("get approved plan")
                .expect("stored plan")
                .status,
            ActionStatus::ApprovedAwaitingExecutor
        );
        assert!(storage
            .replace_action_plan(ActionStatus::WaitingApproval, &approved)
            .is_err());

        let records = storage.list_evidence(10).expect("list evidence");
        assert_eq!(records, vec![evidence]);
        let events = storage.replay_events(10).expect("replay events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "test.event");
        assert_eq!(storage.list_action_plans(10).expect("list plans").len(), 1);
    }
}
