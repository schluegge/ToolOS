use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use toolos_domain::{EventRecord, EvidenceRecord};
use toolos_winget::WingetExecutionJournal;
use uuid::Uuid;

mod cleanup;
mod recovery;
pub use cleanup::*;
pub use recovery::*;

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
            capability TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            approval_phrase TEXT NOT NULL,
            record_json TEXT NOT NULL
        );
        CREATE INDEX action_plan_status_idx ON action_plan(status, expires_at);
        CREATE TABLE approval_receipt (
            id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            plan_hash TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            record_json TEXT NOT NULL,
            FOREIGN KEY(plan_id) REFERENCES action_plan(id)
        );
        CREATE TABLE resource_lock (
            resource_key TEXT PRIMARY KEY,
            holder_plan_id TEXT NOT NULL,
            acquired_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            FOREIGN KEY(holder_plan_id) REFERENCES action_plan(id)
        );",
    ),
    M::up(
        "CREATE TABLE execution_journal (
            execution_id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            approval_id TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            phase TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            provider_version TEXT,
            command_json TEXT NOT NULL,
            pre_state_json TEXT NOT NULL,
            process_identity_json TEXT,
            provider_result_json TEXT,
            recovery_policy_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            resolved_at TEXT,
            recovery_status TEXT,
            recovery_report_json TEXT,
            record_json TEXT NOT NULL,
            FOREIGN KEY(plan_id) REFERENCES action_plan(id),
            FOREIGN KEY(approval_id) REFERENCES approval_receipt(id)
        );
        CREATE INDEX execution_journal_phase_idx
            ON execution_journal(phase, updated_at);
        CREATE INDEX execution_journal_recovery_idx
            ON execution_journal(recovery_status, resolved_at);",
    ),
    M::up(
        "CREATE TABLE recovery_cleanup_plan (
            id TEXT PRIMARY KEY,
            recovery_execution_id TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            approval_phrase TEXT NOT NULL,
            execution_enabled INTEGER NOT NULL CHECK(execution_enabled = 0),
            record_json TEXT NOT NULL,
            FOREIGN KEY(recovery_execution_id) REFERENCES execution_journal(execution_id)
        );
        CREATE INDEX recovery_cleanup_execution_idx
            ON recovery_cleanup_plan(recovery_execution_id, created_at DESC);
        CREATE TABLE recovery_cleanup_approval (
            id TEXT PRIMARY KEY,
            cleanup_plan_id TEXT NOT NULL UNIQUE,
            plan_hash TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            execution_enabled INTEGER NOT NULL CHECK(execution_enabled = 0),
            record_json TEXT NOT NULL,
            FOREIGN KEY(cleanup_plan_id) REFERENCES recovery_cleanup_plan(id)
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
    #[error("action plan not found: {0}")]
    PlanNotFound(String),
    #[error("action plan expired: {0}")]
    PlanExpired(String),
    #[error("action plan hash mismatch")]
    PlanHashMismatch,
    #[error("approval phrase mismatch")]
    ApprovalPhraseMismatch,
    #[error("approval receipt not found: {0}")]
    ApprovalNotFound(String),
    #[error("approval receipt expired: {0}")]
    ApprovalExpired(String),
    #[error("action plan is not awaiting approval: {0}")]
    PlanNotApprovable(String),
    #[error("resource lock {resource_key} is held by plan {holder_plan_id} until {expires_at}")]
    ResourceLocked {
        resource_key: String,
        holder_plan_id: String,
        expires_at: String,
    },
    #[error("execution journal {execution_id} is not in expected phase {expected}")]
    JournalPhaseMismatch {
        execution_id: String,
        expected: String,
    },
    #[error("invalid execution journal transition from {expected} to {next}")]
    JournalTransitionInvalid { expected: String, next: String },
    #[error("execution journal recovery result does not match report: {0}")]
    JournalRecoveryMismatch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredActionPlan {
    pub id: Uuid,
    pub capability: String,
    pub resource_key: String,
    pub status: String,
    pub plan_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub approval_phrase: String,
    pub record_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredApprovalReceipt {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub resource_key: String,
    pub record_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredResourceLock {
    pub resource_key: String,
    pub holder_plan_id: Uuid,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ActionPlanApproval<'a> {
    pub plan_id: Uuid,
    pub expected_hash: &'a str,
    pub confirmation: &'a str,
    pub updated_plan_json: &'a str,
    pub receipt: &'a StoredApprovalReceipt,
    pub lock: &'a StoredResourceLock,
    pub now: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ActionExecutionStart<'a> {
    pub plan_id: Uuid,
    pub approval_id: Uuid,
    pub expected_hash: &'a str,
    pub updated_plan_json: &'a str,
    pub now: DateTime<Utc>,
    pub lock_expires_at: DateTime<Utc>,
    pub journal: &'a WingetExecutionJournal,
}

#[derive(Debug)]
pub struct ActionExecutionFinish<'a> {
    pub plan_id: Uuid,
    pub final_status: &'a str,
    pub updated_plan_json: &'a str,
    pub resource_key: &'a str,
    pub release_resource_lock: bool,
    pub journal: &'a WingetExecutionJournal,
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

    pub fn store_action_plan(&self, plan: &StoredActionPlan) -> Result<(), StorageError> {
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO action_plan (
                id, capability, resource_key, status, plan_hash, created_at, expires_at,
                approval_phrase, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                plan.id.to_string(),
                plan.capability,
                plan.resource_key,
                plan.status,
                plan.plan_hash,
                plan.created_at.to_rfc3339(),
                plan.expires_at.to_rfc3339(),
                plan.approval_phrase,
                plan.record_json,
            ],
        )?;
        Ok(())
    }

    pub fn get_action_plan(&self, id: Uuid) -> Result<Option<StoredActionPlan>, StorageError> {
        let connection = self.open_connection()?;
        let row = connection
            .query_row(
                "SELECT capability, resource_key, status, plan_hash, created_at, expires_at,
                        approval_phrase, record_json
                 FROM action_plan WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                capability,
                resource_key,
                status,
                plan_hash,
                created_at,
                expires_at,
                approval_phrase,
                record_json,
            )| {
                Ok(StoredActionPlan {
                    id,
                    capability,
                    resource_key,
                    status,
                    plan_hash,
                    created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
                    expires_at: DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc),
                    approval_phrase,
                    record_json,
                })
            },
        )
        .transpose()
    }

    pub fn approve_action_plan(
        &self,
        approval: &ActionPlanApproval<'_>,
    ) -> Result<(), StorageError> {
        let plan_id = approval.plan_id;
        let expected_hash = approval.expected_hash;
        let confirmation = approval.confirmation;
        let updated_plan_json = approval.updated_plan_json;
        let receipt = approval.receipt;
        let lock = approval.lock;
        let now = approval.now;
        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let plan = transaction
            .query_row(
                "SELECT resource_key, status, plan_hash, expires_at, approval_phrase
                 FROM action_plan WHERE id = ?1",
                [plan_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::PlanNotFound(plan_id.to_string()))?;
        let (resource_key, status, plan_hash, expires_at, approval_phrase) = plan;
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if now >= expires_at {
            return Err(StorageError::PlanExpired(plan_id.to_string()));
        }
        if status != "AWAITING_APPROVAL" {
            return Err(StorageError::PlanNotApprovable(status));
        }
        if plan_hash != expected_hash || receipt.plan_hash != expected_hash {
            return Err(StorageError::PlanHashMismatch);
        }
        if confirmation.trim() != approval_phrase {
            return Err(StorageError::ApprovalPhraseMismatch);
        }
        if resource_key != lock.resource_key || resource_key != receipt.resource_key {
            return Err(StorageError::PlanNotApprovable(
                "approval resource does not match plan".to_owned(),
            ));
        }

        let existing = transaction
            .query_row(
                "SELECT holder_plan_id, acquired_at, expires_at FROM resource_lock
                 WHERE resource_key = ?1",
                [&resource_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((holder_plan_id, _acquired_at, lock_expires_at)) = existing {
            let parsed = DateTime::parse_from_rfc3339(&lock_expires_at)?.with_timezone(&Utc);
            if parsed > now && holder_plan_id != plan_id.to_string() {
                return Err(StorageError::ResourceLocked {
                    resource_key,
                    holder_plan_id,
                    expires_at: lock_expires_at,
                });
            }
            transaction.execute(
                "DELETE FROM resource_lock WHERE resource_key = ?1",
                [&lock.resource_key],
            )?;
        }

        transaction.execute(
            "INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                lock.resource_key,
                lock.holder_plan_id.to_string(),
                lock.acquired_at.to_rfc3339(),
                lock.expires_at.to_rfc3339(),
            ],
        )?;
        transaction.execute(
            "UPDATE action_plan SET status = ?1, record_json = ?2 WHERE id = ?3",
            params![
                "APPROVED_AWAITING_EXECUTION",
                updated_plan_json,
                plan_id.to_string()
            ],
        )?;
        transaction.execute(
            "INSERT INTO approval_receipt (
                id, plan_id, plan_hash, approved_at, expires_at, resource_key, record_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                receipt.id.to_string(),
                receipt.plan_id.to_string(),
                receipt.plan_hash,
                receipt.approved_at.to_rfc3339(),
                receipt.expires_at.to_rfc3339(),
                receipt.resource_key,
                receipt.record_json,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_approval_receipt(
        &self,
        approval_id: Uuid,
    ) -> Result<Option<StoredApprovalReceipt>, StorageError> {
        let connection = self.open_connection()?;
        let row = connection
            .query_row(
                "SELECT plan_id, plan_hash, approved_at, expires_at, resource_key, record_json
                 FROM approval_receipt WHERE id = ?1",
                [approval_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(plan_id, plan_hash, approved_at, expires_at, resource_key, record_json)| {
                Ok(StoredApprovalReceipt {
                    id: approval_id,
                    plan_id: Uuid::parse_str(&plan_id)?,
                    plan_hash,
                    approved_at: DateTime::parse_from_rfc3339(&approved_at)?.with_timezone(&Utc),
                    expires_at: DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc),
                    resource_key,
                    record_json,
                })
            },
        )
        .transpose()
    }

    pub fn begin_action_execution(
        &self,
        execution: &ActionExecutionStart<'_>,
    ) -> Result<(), StorageError> {
        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let plan = transaction
            .query_row(
                "SELECT status, plan_hash, expires_at, resource_key FROM action_plan WHERE id = ?1",
                [execution.plan_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::PlanNotFound(execution.plan_id.to_string()))?;
        let (status, plan_hash, plan_expires_at, resource_key) = plan;
        let plan_expires_at = DateTime::parse_from_rfc3339(&plan_expires_at)?.with_timezone(&Utc);
        if execution.now >= plan_expires_at {
            return Err(StorageError::PlanExpired(execution.plan_id.to_string()));
        }
        if status != "APPROVED_AWAITING_EXECUTION" {
            return Err(StorageError::PlanNotApprovable(status));
        }
        if plan_hash != execution.expected_hash {
            return Err(StorageError::PlanHashMismatch);
        }

        let receipt = transaction
            .query_row(
                "SELECT plan_id, plan_hash, expires_at, resource_key FROM approval_receipt WHERE id = ?1",
                [execution.approval_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| StorageError::ApprovalNotFound(execution.approval_id.to_string()))?;
        let (receipt_plan_id, receipt_hash, receipt_expires_at, receipt_resource_key) = receipt;
        let receipt_expires_at =
            DateTime::parse_from_rfc3339(&receipt_expires_at)?.with_timezone(&Utc);
        if execution.now >= receipt_expires_at {
            return Err(StorageError::ApprovalExpired(
                execution.approval_id.to_string(),
            ));
        }
        if receipt_plan_id != execution.plan_id.to_string()
            || receipt_hash != execution.expected_hash
            || receipt_resource_key != resource_key
        {
            return Err(StorageError::PlanHashMismatch);
        }

        let lock = transaction
            .query_row(
                "SELECT holder_plan_id, expires_at FROM resource_lock WHERE resource_key = ?1",
                [&resource_key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| StorageError::ResourceLocked {
                resource_key: resource_key.clone(),
                holder_plan_id: "none".to_owned(),
                expires_at: execution.now.to_rfc3339(),
            })?;
        let (holder_plan_id, lock_expires_at) = lock;
        let parsed_lock_expiry =
            DateTime::parse_from_rfc3339(&lock_expires_at)?.with_timezone(&Utc);
        if holder_plan_id != execution.plan_id.to_string() || parsed_lock_expiry <= execution.now {
            return Err(StorageError::ResourceLocked {
                resource_key,
                holder_plan_id,
                expires_at: lock_expires_at,
            });
        }

        transaction.execute(
            "UPDATE action_plan SET status = 'EXECUTING', record_json = ?1 WHERE id = ?2",
            params![execution.updated_plan_json, execution.plan_id.to_string()],
        )?;
        transaction.execute(
            "UPDATE approval_receipt SET expires_at = ?1 WHERE id = ?2",
            params![
                execution.now.to_rfc3339(),
                execution.approval_id.to_string()
            ],
        )?;
        transaction.execute(
            "UPDATE resource_lock SET expires_at = ?1 WHERE resource_key = ?2 AND holder_plan_id = ?3",
            params![
                execution.lock_expires_at.to_rfc3339(),
                receipt_resource_key,
                execution.plan_id.to_string()
            ],
        )?;
        recovery::insert_execution_journal(&transaction, execution.journal)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_action_execution(
        &self,
        execution: &ActionExecutionFinish<'_>,
    ) -> Result<(), StorageError> {
        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE action_plan SET status = ?1, record_json = ?2
             WHERE id = ?3 AND status = 'EXECUTING'",
            params![
                execution.final_status,
                execution.updated_plan_json,
                execution.plan_id.to_string()
            ],
        )?;
        if updated != 1 {
            return Err(StorageError::PlanNotApprovable(
                "execution completion requires EXECUTING state".to_owned(),
            ));
        }
        recovery::finalize_execution_journal(&transaction, execution.journal)?;
        if execution.release_resource_lock {
            transaction.execute(
                "DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2",
                params![execution.resource_key, execution.plan_id.to_string()],
            )?;
        } else {
            let retained = transaction.execute(
                "UPDATE resource_lock SET expires_at = '9999-12-31T23:59:59+00:00'
                 WHERE resource_key = ?1 AND holder_plan_id = ?2",
                params![execution.resource_key, execution.plan_id.to_string()],
            )?;
            if retained != 1 {
                return Err(StorageError::ResourceLocked {
                    resource_key: execution.resource_key.to_owned(),
                    holder_plan_id: execution.plan_id.to_string(),
                    expires_at: "missing recovery lock".to_owned(),
                });
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_resource_lock(
        &self,
        resource_key: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<StoredResourceLock>, StorageError> {
        let connection = self.open_connection()?;
        let row = connection
            .query_row(
                "SELECT holder_plan_id, acquired_at, expires_at FROM resource_lock
                 WHERE resource_key = ?1",
                [resource_key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((holder_plan_id, acquired_at, expires_at)) = row else {
            return Ok(None);
        };
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if expires_at <= now {
            connection.execute(
                "DELETE FROM resource_lock WHERE resource_key = ?1",
                [resource_key],
            )?;
            return Ok(None);
        }
        Ok(Some(StoredResourceLock {
            resource_key: resource_key.to_owned(),
            holder_plan_id: Uuid::parse_str(&holder_plan_id)?,
            acquired_at: DateTime::parse_from_rfc3339(&acquired_at)?.with_timezone(&Utc),
            expires_at,
        }))
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
    use toolos_winget::{
        build_residual_state_manifest, install_preview, installed_probe, ExecutionJournalPhase,
        InstalledQueryStatus, PackageScope, PackageSelector, WingetExecutionJournal,
        WingetInstalledStateReport, WingetRecoveryPolicy,
    };

    #[test]
    fn migrations_are_valid() {
        validate_migrations().expect("valid migrations");
    }

    #[test]
    fn plan_approval_is_atomic_and_locks_resource() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan_id = Uuid::new_v4();
        let plan = StoredActionPlan {
            id: plan_id,
            capability: "package.install.plan.winget".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            status: "AWAITING_APPROVAL".to_owned(),
            plan_hash: "abc".to_owned(),
            created_at: now,
            expires_at: now + chrono::Duration::minutes(10),
            approval_phrase: "APPROVE".to_owned(),
            record_json: "{}".to_owned(),
        };
        storage.store_action_plan(&plan).expect("store plan");
        let receipt = StoredApprovalReceipt {
            id: Uuid::new_v4(),
            plan_id,
            plan_hash: "abc".to_owned(),
            approved_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            resource_key: plan.resource_key.clone(),
            record_json: "{}".to_owned(),
        };
        let lock = StoredResourceLock {
            resource_key: plan.resource_key.clone(),
            holder_plan_id: plan_id,
            acquired_at: now,
            expires_at: receipt.expires_at,
        };
        storage
            .approve_action_plan(&ActionPlanApproval {
                plan_id,
                expected_hash: "abc",
                confirmation: "APPROVE",
                updated_plan_json: "{}",
                receipt: &receipt,
                lock: &lock,
                now,
            })
            .expect("approve plan");
        let saved = storage
            .get_action_plan(plan_id)
            .expect("get plan")
            .expect("plan");
        assert_eq!(saved.status, "APPROVED_AWAITING_EXECUTION");
        assert_eq!(
            storage
                .get_resource_lock("package-manager:winget", now)
                .expect("lock")
                .expect("active lock")
                .holder_plan_id,
            plan_id
        );
    }

    #[test]
    fn plan_approval_rejects_wrong_phrase() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan_id = Uuid::new_v4();
        let plan = StoredActionPlan {
            id: plan_id,
            capability: "package.install.plan.winget".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            status: "AWAITING_APPROVAL".to_owned(),
            plan_hash: "abc".to_owned(),
            created_at: now,
            expires_at: now + chrono::Duration::minutes(10),
            approval_phrase: "APPROVE".to_owned(),
            record_json: "{}".to_owned(),
        };
        storage.store_action_plan(&plan).expect("store plan");
        let receipt = StoredApprovalReceipt {
            id: Uuid::new_v4(),
            plan_id,
            plan_hash: "abc".to_owned(),
            approved_at: now,
            expires_at: now + chrono::Duration::minutes(5),
            resource_key: plan.resource_key.clone(),
            record_json: "{}".to_owned(),
        };
        let lock = StoredResourceLock {
            resource_key: plan.resource_key,
            holder_plan_id: plan_id,
            acquired_at: now,
            expires_at: receipt.expires_at,
        };
        assert!(matches!(
            storage.approve_action_plan(&ActionPlanApproval {
                plan_id,
                expected_hash: "abc",
                confirmation: "WRONG",
                updated_plan_json: "{}",
                receipt: &receipt,
                lock: &lock,
                now,
            }),
            Err(StorageError::ApprovalPhraseMismatch)
        ));
    }

    fn recovery_test_journal(
        plan_id: Uuid,
        approval_id: Uuid,
        now: DateTime<Utc>,
    ) -> WingetExecutionJournal {
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        };
        let installed_state = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector.clone(),
            installed_probe: installed_probe(&selector),
            installed_evidence: None,
            observed_at: now,
            definitive_installed_match: None,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        };
        WingetExecutionJournal {
            execution_id: Uuid::new_v4(),
            plan_id,
            approval_id,
            plan_hash: "abc".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            phase: ExecutionJournalPhase::Prepared,
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            command: install_preview(&selector),
            pre_state: build_residual_state_manifest(
                selector,
                Some("v1".to_owned()),
                installed_state,
                now,
            ),
            process_identity: None,
            provider_result: None,
            recovery_policy: WingetRecoveryPolicy::default(),
            created_at: now,
            updated_at: now,
            resolved_at: None,
            recovery_status: None,
        }
    }

    #[test]
    fn unknown_execution_retains_resource_lock() {
        let directory = tempdir().expect("temp directory");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan_id = Uuid::new_v4();
        let approval_id = Uuid::new_v4();
        let mut journal = recovery_test_journal(plan_id, approval_id, now);
        let mut connection = storage.open_connection().expect("connection");
        connection
            .execute(
                "INSERT INTO action_plan (id, capability, resource_key, status, plan_hash, created_at, expires_at, approval_phrase, record_json)
                 VALUES (?1, 'package.install.plan.winget', 'package-manager:winget', 'EXECUTING', 'abc', ?2, ?3, '', '{}')",
                params![
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(10)).to_rfc3339()
                ],
            )
            .expect("insert executing plan");
        connection
            .execute(
                "INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)
                 VALUES ('package-manager:winget', ?1, ?2, ?3)",
                params![
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(30)).to_rfc3339()
                ],
            )
            .expect("insert lock");
        connection
            .execute(
                "INSERT INTO approval_receipt (id, plan_id, plan_hash, approved_at, expires_at, resource_key, record_json)
                 VALUES (?1, ?2, 'abc', ?3, ?4, 'package-manager:winget', '{}')",
                params![
                    approval_id.to_string(),
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(5)).to_rfc3339()
                ],
            )
            .expect("insert approval");
        let transaction = connection.transaction().expect("journal transaction");
        recovery::insert_execution_journal(&transaction, &journal).expect("insert journal");
        transaction.commit().expect("commit journal");
        drop(connection);

        journal.phase = ExecutionJournalPhase::SpawnIntent;
        journal.updated_at = now + chrono::Duration::seconds(1);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Prepared, &journal)
            .expect("spawn intent");
        journal.phase = ExecutionJournalPhase::Spawned;
        journal.updated_at = now + chrono::Duration::seconds(2);
        storage
            .transition_execution_journal(ExecutionJournalPhase::SpawnIntent, &journal)
            .expect("spawned");
        journal.phase = ExecutionJournalPhase::ProviderFinished;
        journal.updated_at = now + chrono::Duration::seconds(3);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Spawned, &journal)
            .expect("provider finished");
        journal.phase = ExecutionJournalPhase::Finalized;
        journal.updated_at = now + chrono::Duration::seconds(4);
        journal.resolved_at = Some(journal.updated_at);

        storage
            .finish_action_execution(&ActionExecutionFinish {
                plan_id,
                final_status: "UNKNOWN_REQUIRES_RECOVERY",
                updated_plan_json: "{}",
                resource_key: "package-manager:winget",
                release_resource_lock: false,
                journal: &journal,
            })
            .expect("finish unknown execution");

        let lock = storage
            .get_resource_lock("package-manager:winget", now + chrono::Duration::days(3650))
            .expect("query retained lock")
            .expect("retained lock");
        assert_eq!(lock.holder_plan_id, plan_id);
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
