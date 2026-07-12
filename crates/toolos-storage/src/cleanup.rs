use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use toolos_winget::{
    RecoveryCleanupPlanStatus, WingetRecoveryCleanupApprovalReceipt, WingetRecoveryCleanupPlan,
};
use uuid::Uuid;

use crate::{Storage, StorageError};

impl Storage {
    pub fn store_recovery_cleanup_plan(
        &self,
        plan: &WingetRecoveryCleanupPlan,
    ) -> Result<(), StorageError> {
        if plan.status != RecoveryCleanupPlanStatus::AwaitingApproval
            || !plan.approval_allowed
            || plan.execution_enabled
            || plan.uninstall_preview.execution_enabled
        {
            return Err(StorageError::PlanNotApprovable(
                "recovery cleanup plan must be awaiting approval and execution-disabled".to_owned(),
            ));
        }
        let phrase = plan
            .approval_challenge
            .as_ref()
            .ok_or_else(|| {
                StorageError::PlanNotApprovable(
                    "recovery cleanup plan has no approval challenge".to_owned(),
                )
            })?
            .required_phrase
            .clone();
        let connection = self.open_connection()?;
        connection.execute(
            "INSERT INTO recovery_cleanup_plan (
                id, recovery_execution_id, plan_hash, status, created_at, expires_at,
                approval_phrase, execution_enabled, record_json
             ) VALUES (?1, ?2, ?3, 'AWAITING_APPROVAL', ?4, ?5, ?6, 0, ?7)",
            params![
                plan.cleanup_plan_id.to_string(),
                plan.recovery_execution_id.to_string(),
                plan.plan_hash,
                plan.created_at.to_rfc3339(),
                plan.expires_at.to_rfc3339(),
                phrase,
                serde_json::to_string(plan)?,
            ],
        )?;
        Ok(())
    }

    pub fn get_recovery_cleanup_plan(
        &self,
        cleanup_plan_id: Uuid,
    ) -> Result<Option<WingetRecoveryCleanupPlan>, StorageError> {
        let connection = self.open_connection()?;
        let record = connection
            .query_row(
                "SELECT record_json FROM recovery_cleanup_plan WHERE id = ?1",
                [cleanup_plan_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        record
            .map(|value| serde_json::from_str(&value).map_err(StorageError::from))
            .transpose()
    }

    pub fn approve_recovery_cleanup_plan(
        &self,
        cleanup_plan_id: Uuid,
        expected_hash: &str,
        confirmation: &str,
        now: DateTime<Utc>,
        approved_plan: &WingetRecoveryCleanupPlan,
        receipt: &WingetRecoveryCleanupApprovalReceipt,
    ) -> Result<(), StorageError> {
        if approved_plan.execution_enabled || receipt.execution_enabled {
            return Err(StorageError::PlanNotApprovable(
                "recovery cleanup approval cannot enable execution".to_owned(),
            ));
        }
        let mut connection = self.open_connection()?;
        connection.set_transaction_behavior(TransactionBehavior::Immediate);
        let transaction = connection.transaction()?;
        let stored = transaction
            .query_row(
                "SELECT plan_hash, status, expires_at, approval_phrase
                 FROM recovery_cleanup_plan WHERE id = ?1",
                [cleanup_plan_id.to_string()],
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
            .ok_or_else(|| StorageError::PlanNotFound(cleanup_plan_id.to_string()))?;
        let (plan_hash, status, expires_at, approval_phrase) = stored;
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if status != "AWAITING_APPROVAL" {
            return Err(StorageError::PlanNotApprovable(status));
        }
        if now >= expires_at {
            return Err(StorageError::PlanExpired(cleanup_plan_id.to_string()));
        }
        if plan_hash != expected_hash
            || approved_plan.plan_hash != expected_hash
            || receipt.plan_hash != expected_hash
        {
            return Err(StorageError::PlanHashMismatch);
        }
        if confirmation.trim() != approval_phrase {
            return Err(StorageError::ApprovalPhraseMismatch);
        }
        if receipt.cleanup_plan_id != cleanup_plan_id
            || approved_plan.cleanup_plan_id != cleanup_plan_id
        {
            return Err(StorageError::PlanHashMismatch);
        }

        transaction.execute(
            "UPDATE recovery_cleanup_plan SET
                status = 'APPROVED_EXECUTION_DISABLED', execution_enabled = 0,
                record_json = ?1 WHERE id = ?2",
            params![
                serde_json::to_string(approved_plan)?,
                cleanup_plan_id.to_string()
            ],
        )?;
        transaction.execute(
            "INSERT INTO recovery_cleanup_approval (
                id, cleanup_plan_id, plan_hash, approved_at, execution_enabled, record_json
             ) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![
                receipt.approval_id.to_string(),
                cleanup_plan_id.to_string(),
                receipt.plan_hash,
                receipt.approved_at.to_rfc3339(),
                serde_json::to_string(receipt)?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use toolos_winget::{
        approve_recovery_cleanup_plan as approve_domain, build_recovery_cleanup_plan,
        build_residual_state_manifest, install_preview, ExecutionJournalPhase,
        InstalledQueryStatus, PackageScope, PackageSelector, RecoveryStatus,
        WingetInstalledStateReport, WingetRecoveryReport,
    };

    fn report() -> WingetRecoveryReport {
        let now = Utc::now();
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector.clone(),
            installed_probe: install_preview(&selector),
            installed_evidence: None,
            observed_at: now,
            definitive_installed_match: None,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        };
        WingetRecoveryReport {
            recovery_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            journal_phase: ExecutionJournalPhase::Spawned,
            status: RecoveryStatus::UnknownRequiresRecovery,
            observed_at: now,
            pre_state: build_residual_state_manifest(
                selector,
                Some("v1".to_owned()),
                installed,
                now,
            ),
            post_state: None,
            residual_diff: None,
            lock_retained: true,
            mutable_operations_blocked: true,
            limitations: Vec::new(),
            single_safest_next_action: "inspect".to_owned(),
        }
    }

    #[test]
    fn cleanup_approval_is_durable_and_never_enables_execution() {
        let directory = tempdir().expect("tempdir");
        let storage = Storage::initialize(directory.path().join("toolos.db")).expect("storage");
        let now = Utc::now();
        let plan = build_recovery_cleanup_plan(&report(), now, 300).expect("plan");
        storage
            .store_recovery_cleanup_plan(&plan)
            .expect("store cleanup plan");
        let phrase = plan
            .approval_challenge
            .as_ref()
            .expect("challenge")
            .required_phrase
            .clone();
        let (approved, receipt) = approve_domain(&plan, &phrase, now).expect("approve domain");
        storage
            .approve_recovery_cleanup_plan(
                plan.cleanup_plan_id,
                &plan.plan_hash,
                &phrase,
                now,
                &approved,
                &receipt,
            )
            .expect("persist approval");
        let saved = storage
            .get_recovery_cleanup_plan(plan.cleanup_plan_id)
            .expect("load")
            .expect("plan");
        assert_eq!(
            saved.status,
            RecoveryCleanupPlanStatus::ApprovedExecutionDisabled
        );
        assert!(!saved.execution_enabled);
        assert!(!saved.uninstall_preview.execution_enabled);
    }
}
