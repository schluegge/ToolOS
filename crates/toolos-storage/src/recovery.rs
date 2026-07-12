use rusqlite::{params, OptionalExtension, TransactionBehavior};
use toolos_winget::{
    ExecutionJournalPhase, RecoveryStatus, WingetExecutionJournal, WingetRecoveryReport,
};
use uuid::Uuid;

use crate::{Storage, StorageError};

#[derive(Debug)]
pub struct RecoveryResolution<'a> {
    pub journal: &'a WingetExecutionJournal,
    pub report: &'a WingetRecoveryReport,
    pub final_plan_status: &'a str,
    pub updated_plan_json: &'a str,
    pub release_resource_lock: bool,
}

impl Storage {
    pub fn get_execution_journal(
        &self,
        execution_id: Uuid,
    ) -> Result<Option<WingetExecutionJournal>, StorageError> {
        let connection = self.open_connection()?;
        let record = connection
            .query_row(
                "SELECT record_json FROM execution_journal WHERE execution_id = ?1",
                [execution_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        record
            .map(|value| serde_json::from_str(&value).map_err(StorageError::from))
            .transpose()
    }

    pub fn list_unresolved_execution_journals(
        &self,
    ) -> Result<Vec<WingetExecutionJournal>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            "SELECT record_json FROM execution_journal
             WHERE phase != 'FINALIZED'
             ORDER BY created_at ASC, execution_id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut journals = Vec::new();
        for row in rows {
            journals.push(serde_json::from_str(&row?)?);
        }
        Ok(journals)
    }

    pub fn list_recovery_reports(&self) -> Result<Vec<WingetRecoveryReport>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            "SELECT recovery_report_json FROM execution_journal
             WHERE recovery_report_json IS NOT NULL
             ORDER BY resolved_at DESC, execution_id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut reports = Vec::new();
        for row in rows {
            reports.push(serde_json::from_str(&row?)?);
        }
        Ok(reports)
    }

    pub fn get_recovery_report(
        &self,
        execution_id: Uuid,
    ) -> Result<Option<WingetRecoveryReport>, StorageError> {
        let connection = self.open_connection()?;
        let value = connection
            .query_row(
                "SELECT recovery_report_json FROM execution_journal
                 WHERE execution_id = ?1 AND recovery_report_json IS NOT NULL",
                [execution_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        value
            .map(|record| serde_json::from_str(&record).map_err(StorageError::from))
            .transpose()
    }

    pub fn has_blocking_recovery(&self) -> Result<bool, StorageError> {
        let connection = self.open_connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM execution_journal
             WHERE phase != 'FINALIZED'
                OR recovery_status = 'UNKNOWN_REQUIRES_RECOVERY'
                OR recovery_status = 'FAILED_RESIDUALS_PRESENT'",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn transition_execution_journal(
        &self,
        expected_phase: ExecutionJournalPhase,
        journal: &WingetExecutionJournal,
    ) -> Result<(), StorageError> {
        validate_transition(expected_phase, journal.phase)?;
        let connection = self.open_connection()?;
        let updated = connection.execute(
            "UPDATE execution_journal SET
                phase = ?1,
                provider_version = ?2,
                process_identity_json = ?3,
                provider_result_json = ?4,
                record_json = ?5,
                updated_at = ?6
             WHERE execution_id = ?7 AND phase = ?8",
            params![
                phase_name(journal.phase),
                journal.provider_version,
                serialize_optional(&journal.process_identity)?,
                serialize_optional(&journal.provider_result)?,
                serde_json::to_string(journal)?,
                journal.updated_at.to_rfc3339(),
                journal.execution_id.to_string(),
                phase_name(expected_phase),
            ],
        )?;
        if updated != 1 {
            return Err(StorageError::JournalPhaseMismatch {
                execution_id: journal.execution_id.to_string(),
                expected: phase_name(expected_phase).to_owned(),
            });
        }
        Ok(())
    }

    pub fn resolve_execution_recovery(
        &self,
        resolution: &RecoveryResolution<'_>,
    ) -> Result<(), StorageError> {
        let journal = resolution.journal;
        let report = resolution.report;
        if journal.phase != ExecutionJournalPhase::Finalized {
            return Err(StorageError::JournalPhaseMismatch {
                execution_id: journal.execution_id.to_string(),
                expected: "FINALIZED".to_owned(),
            });
        }
        if journal.recovery_status != Some(report.status) {
            return Err(StorageError::JournalRecoveryMismatch(
                journal.execution_id.to_string(),
            ));
        }

        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let plan_updated = transaction.execute(
            "UPDATE action_plan SET status = ?1, record_json = ?2
             WHERE id = ?3 AND status = 'EXECUTING'",
            params![
                resolution.final_plan_status,
                resolution.updated_plan_json,
                journal.plan_id.to_string(),
            ],
        )?;
        if plan_updated != 1 {
            return Err(StorageError::PlanNotApprovable(
                "recovery resolution requires EXECUTING state".to_owned(),
            ));
        }

        let journal_updated = transaction.execute(
            "UPDATE execution_journal SET
                phase = 'FINALIZED',
                record_json = ?1,
                updated_at = ?2,
                resolved_at = ?3,
                recovery_status = ?4,
                recovery_report_json = ?5
             WHERE execution_id = ?6 AND phase != 'FINALIZED'",
            params![
                serde_json::to_string(journal)?,
                journal.updated_at.to_rfc3339(),
                journal
                    .resolved_at
                    .ok_or_else(|| StorageError::JournalRecoveryMismatch(
                        journal.execution_id.to_string()
                    ))?
                    .to_rfc3339(),
                recovery_status_name(report.status),
                serde_json::to_string(report)?,
                journal.execution_id.to_string(),
            ],
        )?;
        if journal_updated != 1 {
            return Err(StorageError::JournalPhaseMismatch {
                execution_id: journal.execution_id.to_string(),
                expected: "non-finalized".to_owned(),
            });
        }

        update_lock(
            &transaction,
            &journal.resource_key,
            journal.plan_id,
            resolution.release_resource_lock,
        )?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn insert_execution_journal(
    transaction: &rusqlite::Transaction<'_>,
    journal: &WingetExecutionJournal,
) -> Result<(), StorageError> {
    if journal.phase != ExecutionJournalPhase::Prepared
        || journal.process_identity.is_some()
        || journal.provider_result.is_some()
        || journal.resolved_at.is_some()
        || journal.recovery_status.is_some()
    {
        return Err(StorageError::JournalPhaseMismatch {
            execution_id: journal.execution_id.to_string(),
            expected: "PREPARED without runtime or recovery result".to_owned(),
        });
    }

    transaction.execute(
        "INSERT INTO execution_journal (
            execution_id, plan_id, approval_id, plan_hash, resource_key, phase,
            provider_id, provider_version, command_json, pre_state_json,
            process_identity_json, provider_result_json, recovery_policy_json,
            created_at, updated_at, resolved_at, recovery_status,
            recovery_report_json, record_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL,
                   ?11, ?12, ?13, NULL, NULL, NULL, ?14)",
        params![
            journal.execution_id.to_string(),
            journal.plan_id.to_string(),
            journal.approval_id.to_string(),
            journal.plan_hash,
            journal.resource_key,
            phase_name(journal.phase),
            journal.provider_id,
            journal.provider_version,
            serde_json::to_string(&journal.command)?,
            serde_json::to_string(&journal.pre_state)?,
            serde_json::to_string(&journal.recovery_policy)?,
            journal.created_at.to_rfc3339(),
            journal.updated_at.to_rfc3339(),
            serde_json::to_string(journal)?,
        ],
    )?;
    Ok(())
}

pub(crate) fn finalize_execution_journal(
    transaction: &rusqlite::Transaction<'_>,
    journal: &WingetExecutionJournal,
) -> Result<(), StorageError> {
    if journal.phase != ExecutionJournalPhase::Finalized || journal.resolved_at.is_none() {
        return Err(StorageError::JournalPhaseMismatch {
            execution_id: journal.execution_id.to_string(),
            expected: "FINALIZED with resolved_at".to_owned(),
        });
    }
    let updated = transaction.execute(
        "UPDATE execution_journal SET
            phase = 'FINALIZED', provider_version = ?1,
            process_identity_json = ?2, provider_result_json = ?3,
            record_json = ?4, updated_at = ?5, resolved_at = ?6,
            recovery_status = ?7
         WHERE execution_id = ?8 AND phase = 'PROVIDER_FINISHED'",
        params![
            journal.provider_version,
            serialize_optional(&journal.process_identity)?,
            serialize_optional(&journal.provider_result)?,
            serde_json::to_string(journal)?,
            journal.updated_at.to_rfc3339(),
            journal.resolved_at.map(|value| value.to_rfc3339()),
            journal.recovery_status.map(recovery_status_name),
            journal.execution_id.to_string(),
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::JournalPhaseMismatch {
            execution_id: journal.execution_id.to_string(),
            expected: "PROVIDER_FINISHED".to_owned(),
        });
    }
    Ok(())
}

fn update_lock(
    transaction: &rusqlite::Transaction<'_>,
    resource_key: &str,
    plan_id: Uuid,
    release: bool,
) -> Result<(), StorageError> {
    if release {
        transaction.execute(
            "DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2",
            params![resource_key, plan_id.to_string()],
        )?;
    } else {
        let retained = transaction.execute(
            "UPDATE resource_lock SET expires_at = '9999-12-31T23:59:59+00:00'
             WHERE resource_key = ?1 AND holder_plan_id = ?2",
            params![resource_key, plan_id.to_string()],
        )?;
        if retained != 1 {
            return Err(StorageError::ResourceLocked {
                resource_key: resource_key.to_owned(),
                holder_plan_id: plan_id.to_string(),
                expires_at: "missing recovery lock".to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_transition(
    expected: ExecutionJournalPhase,
    next: ExecutionJournalPhase,
) -> Result<(), StorageError> {
    let valid = matches!(
        (expected, next),
        (
            ExecutionJournalPhase::Prepared,
            ExecutionJournalPhase::SpawnIntent
        ) | (
            ExecutionJournalPhase::SpawnIntent,
            ExecutionJournalPhase::Spawned
        ) | (
            ExecutionJournalPhase::Spawned,
            ExecutionJournalPhase::ProviderFinished
        )
    );
    if valid {
        Ok(())
    } else {
        Err(StorageError::JournalTransitionInvalid {
            expected: phase_name(expected).to_owned(),
            next: phase_name(next).to_owned(),
        })
    }
}

pub(crate) fn phase_name(phase: ExecutionJournalPhase) -> &'static str {
    match phase {
        ExecutionJournalPhase::Prepared => "PREPARED",
        ExecutionJournalPhase::SpawnIntent => "SPAWN_INTENT",
        ExecutionJournalPhase::Spawned => "SPAWNED",
        ExecutionJournalPhase::ProviderFinished => "PROVIDER_FINISHED",
        ExecutionJournalPhase::Finalized => "FINALIZED",
    }
}

fn recovery_status_name(status: RecoveryStatus) -> &'static str {
    match status {
        RecoveryStatus::RecoveredNoProcessStarted => "RECOVERED_NO_PROCESS_STARTED",
        RecoveryStatus::RecoveredFromPersistedProviderResult => {
            "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT"
        }
        RecoveryStatus::FailedResidualsPresent => "FAILED_RESIDUALS_PRESENT",
        RecoveryStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
    }
}

fn serialize_optional<T: serde::Serialize>(
    value: &Option<T>,
) -> Result<Option<String>, StorageError> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Storage;
    use chrono::Utc;
    use tempfile::tempdir;
    use toolos_winget::{
        build_residual_state_manifest, install_preview, CommandPreview, InstalledQueryStatus,
        PackageScope, PackageSelector, WingetInstalledStateReport, WingetRecoveryPolicy,
    };

    fn storage() -> Storage {
        let directory = tempdir().expect("tempdir");
        let path = directory.keep().join("toolos.db");
        Storage::initialize(path).expect("storage")
    }

    fn selector() -> PackageSelector {
        PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        }
    }

    fn installed_state(selector: &PackageSelector) -> WingetInstalledStateReport {
        WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector.clone(),
            installed_probe: install_preview(selector),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        }
    }

    fn journal(phase: ExecutionJournalPhase) -> WingetExecutionJournal {
        let now = Utc::now();
        let selector = selector();
        WingetExecutionJournal {
            execution_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            approval_id: Uuid::new_v4(),
            plan_hash: "a".repeat(64),
            resource_key: "package-manager:winget".to_owned(),
            phase,
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            command: CommandPreview {
                execution_enabled: false,
                ..install_preview(&selector)
            },
            pre_state: build_residual_state_manifest(
                selector.clone(),
                Some("v1".to_owned()),
                installed_state(&selector),
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
    fn migration_creates_execution_journal_for_existing_databases() {
        let storage = storage();
        let connection = storage.open_connection().expect("open");
        let table: String = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='execution_journal'",
                [],
                |row| row.get(0),
            )
            .expect("journal table");
        assert_eq!(table, "execution_journal");
    }

    #[test]
    fn transition_validation_rejects_skipped_phases() {
        let journal = journal(ExecutionJournalPhase::Spawned);
        assert!(validate_transition(ExecutionJournalPhase::Prepared, journal.phase).is_err());
        assert!(validate_transition(
            ExecutionJournalPhase::SpawnIntent,
            ExecutionJournalPhase::Spawned
        )
        .is_ok());
    }
}
