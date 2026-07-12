use anyhow::{anyhow, Context};
use chrono::Utc;
use serde_json::{json, Value};
use toolos_domain::{EvidenceKind, EvidenceRecord};
use toolos_storage::RecoveryResolution;
use toolos_winget::{
    build_residual_state_manifest, diff_residual_state, ExecutionJournalPhase, InstallPlanStatus,
    RecoveryStatus, WingetExecutionJournal, WingetInstallPlan, WingetRecoveryReport,
    WingetResidualStateManifest,
};
use uuid::Uuid;

use crate::{record_evidence, winget_installed, AppState};

pub(super) async fn reconcile_startup(
    state: &AppState,
    trace_id: Uuid,
) -> anyhow::Result<Vec<WingetRecoveryReport>> {
    let journals = state.storage.list_unresolved_execution_journals()?;
    let mut reports = Vec::with_capacity(journals.len());
    for journal in journals {
        reports.push(reconcile_one(state, trace_id, journal).await?);
    }
    Ok(reports)
}

pub(super) fn ensure_mutation_allowed(state: &AppState) -> anyhow::Result<()> {
    if state.storage.has_blocking_recovery()? {
        return Err(anyhow!(
            "mutable WinGet operations are blocked by unresolved execution recovery; inspect winget.recovery.list"
        ));
    }
    Ok(())
}

pub(super) fn list(state: &AppState) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(state.storage.list_recovery_reports()?)?)
}

pub(super) fn get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let execution_id = params
        .get("execution_id")
        .and_then(Value::as_str)
        .context("winget.recovery.get requires string execution_id")
        .and_then(|value| Uuid::parse_str(value).map_err(Into::into))?;
    let report = state
        .storage
        .get_recovery_report(execution_id)?
        .with_context(|| format!("recovery report not found: {execution_id}"))?;
    Ok(serde_json::to_value(report)?)
}

async fn reconcile_one(
    state: &AppState,
    trace_id: Uuid,
    mut journal: WingetExecutionJournal,
) -> anyhow::Result<WingetRecoveryReport> {
    let stored_plan = state
        .storage
        .get_action_plan(journal.plan_id)?
        .with_context(|| format!("recovery action plan not found: {}", journal.plan_id))?;
    let mut plan: WingetInstallPlan = serde_json::from_str(&stored_plan.record_json)
        .context("recovery action plan record is malformed")?;
    let now = Utc::now();

    let post_state = capture_post_state(state, trace_id, &journal).await;
    let residual_diff = post_state
        .as_ref()
        .map(|post| diff_residual_state(&journal.pre_state, post));

    let (status, lock_retained, final_plan_status, safest_action) = recovery_decision(&journal);
    let report = WingetRecoveryReport {
        recovery_id: Uuid::new_v4(),
        execution_id: journal.execution_id,
        plan_id: journal.plan_id,
        journal_phase: journal.phase,
        status,
        observed_at: now,
        pre_state: journal.pre_state.clone(),
        post_state,
        residual_diff,
        lock_retained,
        mutable_operations_blocked: lock_retained,
        limitations: vec![
            "Recovery never replays an interrupted installer or cleanup command.".to_owned(),
            "Read-only WinGet evidence is locale-dependent and is not a universal package-health verdict."
                .to_owned(),
            "Files, registry entries, services, scheduled tasks, drivers, PATH changes, and running processes may require package-specific inspection."
                .to_owned(),
        ],
        single_safest_next_action: safest_action.to_owned(),
    };

    plan.status = final_plan_status;
    plan.execution_enabled = false;
    plan.single_safest_next_action = report.single_safest_next_action.clone();
    let final_status = plan_status_name(&plan.status);
    let final_plan_json = serde_json::to_string(&plan)?;

    journal.phase = ExecutionJournalPhase::Finalized;
    journal.updated_at = now;
    journal.resolved_at = Some(now);
    journal.recovery_status = Some(status);
    state
        .storage
        .resolve_execution_recovery(&RecoveryResolution {
            journal: &journal,
            report: &report,
            final_plan_status: final_status,
            updated_plan_json: &final_plan_json,
            release_resource_lock: !lock_retained,
        })?;

    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-recovery:{}", journal.execution_id),
        format!(
            "Interrupted WinGet execution reconciled as {}",
            recovery_status_name(status)
        ),
        "toolos.daemon.recovery",
        serde_json::to_value(&report)?,
        report.limitations.clone(),
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_EXECUTION_RECOVERY")?;
    state.storage.append_event(
        trace_id,
        "winget.install.execution.reconciled",
        &json!({
            "execution_id": journal.execution_id,
            "plan_id": journal.plan_id,
            "previous_phase": phase_name(report.journal_phase),
            "recovery_status": recovery_status_name(status),
            "lock_retained": lock_retained,
            "post_state_captured": report.post_state.is_some()
        }),
    )?;
    Ok(report)
}

async fn capture_post_state(
    state: &AppState,
    trace_id: Uuid,
    journal: &WingetExecutionJournal,
) -> Option<WingetResidualStateManifest> {
    if journal.phase == ExecutionJournalPhase::Prepared {
        return None;
    }
    let selector_json = serde_json::to_value(&journal.pre_state.selector).ok()?;
    let value = winget_installed(state, trace_id, &selector_json).await.ok()?;
    let installed = value
        .get("snapshot")
        .cloned()
        .and_then(|snapshot| serde_json::from_value(snapshot).ok())?;
    Some(build_residual_state_manifest(
        journal.pre_state.selector.clone(),
        journal.provider_version.clone(),
        installed,
        Utc::now(),
    ))
}

fn recovery_decision(
    journal: &WingetExecutionJournal,
) -> (RecoveryStatus, bool, InstallPlanStatus, &'static str) {
    match journal.phase {
        ExecutionJournalPhase::Prepared => (
            RecoveryStatus::RecoveredNoProcessStarted,
            false,
            InstallPlanStatus::RecoveredNoProcessStarted,
            "No process was eligible to start. Review the recovery report before creating a fresh plan.",
        ),
        ExecutionJournalPhase::ProviderFinished
            if journal.provider_result.as_ref().is_some_and(|result| {
                result.containment_evidence.containment_confirmed
                    && result.containment_evidence.active_processes_after_cleanup == Some(0)
            }) => (
            RecoveryStatus::RecoveredFromPersistedProviderResult,
            false,
            InstallPlanStatus::RecoveredFromPersistedProviderResult,
            "Review the persisted provider result and post-state evidence; create a fresh plan only after the machine state is understood.",
        ),
        ExecutionJournalPhase::SpawnIntent
        | ExecutionJournalPhase::Spawned
        | ExecutionJournalPhase::ProviderFinished
        | ExecutionJournalPhase::Finalized => (
            RecoveryStatus::UnknownRequiresRecovery,
            true,
            InstallPlanStatus::UnknownRequiresRecovery,
            "Do not create another mutable package plan. Review the recovery report and create a separately approved disabled cleanup plan if appropriate.",
        ),
    }
}

fn phase_name(phase: ExecutionJournalPhase) -> &'static str {
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

fn plan_status_name(status: &InstallPlanStatus) -> &'static str {
    match status {
        InstallPlanStatus::RecoveredNoProcessStarted => "RECOVERED_NO_PROCESS_STARTED",
        InstallPlanStatus::RecoveredFromPersistedProviderResult => {
            "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT"
        }
        InstallPlanStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
        _ => "UNKNOWN_REQUIRES_RECOVERY",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use toolos_winget::{
        build_residual_state_manifest, install_preview, InstalledQueryStatus, PackageScope,
        PackageSelector, WingetInstalledStateReport, WingetRecoveryPolicy,
    };

    fn journal(phase: ExecutionJournalPhase) -> WingetExecutionJournal {
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
        WingetExecutionJournal {
            execution_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            approval_id: Uuid::new_v4(),
            plan_hash: "a".repeat(64),
            resource_key: "package-manager:winget".to_owned(),
            phase,
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            command: install_preview(&selector),
            pre_state: build_residual_state_manifest(
                selector,
                Some("v1".to_owned()),
                installed,
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
    fn prepared_is_the_only_auto_release_without_provider_result() {
        let (status, retained, plan_status, _) = recovery_decision(&journal(ExecutionJournalPhase::Prepared));
        assert_eq!(status, RecoveryStatus::RecoveredNoProcessStarted);
        assert!(!retained);
        assert_eq!(plan_status, InstallPlanStatus::RecoveredNoProcessStarted);

        for phase in [
            ExecutionJournalPhase::SpawnIntent,
            ExecutionJournalPhase::Spawned,
            ExecutionJournalPhase::ProviderFinished,
        ] {
            let (status, retained, _, _) = recovery_decision(&journal(phase));
            assert_eq!(status, RecoveryStatus::UnknownRequiresRecovery);
            assert!(retained);
        }
    }
}
