use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tempfile::TempDir;
use tokio::sync::Mutex;
use toolos_process::{ProcessContainmentEvidence, ProcessStopReason};
use toolos_storage::{
    ActionExecutionStart, ActionPlanApproval, Storage, StoredActionPlan, StoredApprovalReceipt,
    StoredResourceLock,
};
use toolos_winget::{
    build_approval_receipt, build_install_plan, build_residual_state_manifest, identity_probe,
    install_preview, installed_probe, uninstall_preview, ExecutionJournalPhase, InstallPlanStatus,
    InstalledQueryStatus, PackageScope, PackageSelector, ProcessEvidence, RecoveryStatus,
    ResolutionStatus, WingetExecutionJournal, WingetInstalledStateReport,
    WingetPersistedProviderResult, WingetProcessIdentity, WingetRecoveryPolicy,
    WingetResolutionReport,
};
use uuid::Uuid;

use crate::recovery::{ensure_mutation_allowed, reconcile_startup};
use crate::{ActiveExecution, AppState};

struct SeededRecovery {
    directory: TempDir,
    database_path: PathBuf,
    execution_id: Uuid,
    plan_id: Uuid,
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

fn process_evidence(exit_code: i32) -> ProcessEvidence {
    ProcessEvidence {
        executable: "winget".to_owned(),
        args: vec!["install".to_owned()],
        exit_code: Some(exit_code),
        stdout: "fixture provider output".to_owned(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 25,
    }
}

fn provider_reports(now: DateTime<Utc>) -> (WingetResolutionReport, WingetInstalledStateReport) {
    let selector = selector();
    let resolution = WingetResolutionReport {
        provider_id: "winget".to_owned(),
        provider_version: Some("v1.12.0".to_owned()),
        status: ResolutionStatus::ResolvedExact,
        selector: selector.clone(),
        identity_probe: identity_probe(&selector),
        identity_evidence: Some(process_evidence(0)),
        install_preview: install_preview(&selector),
        uninstall_preview: uninstall_preview(&selector),
        observed_at: now,
        limitations: Vec::new(),
        single_safest_next_action: "review".to_owned(),
    };
    let installed = WingetInstalledStateReport {
        provider_id: "winget".to_owned(),
        provider_version: Some("v1.12.0".to_owned()),
        status: InstalledQueryStatus::QueryCompleted,
        selector: selector.clone(),
        installed_probe: installed_probe(&selector),
        installed_evidence: Some(process_evidence(0)),
        observed_at: now,
        definitive_installed_match: None,
        limitations: Vec::new(),
        single_safest_next_action: "review".to_owned(),
    };
    (resolution, installed)
}

fn seed_recovery(phase: ExecutionJournalPhase) -> SeededRecovery {
    let directory = tempfile::tempdir().expect("tempdir");
    let database_path = directory.path().join("toolos.db");
    let storage = Storage::initialize(&database_path).expect("storage");
    let now = Utc::now();
    let (resolution, installed) = provider_reports(now);
    let plan = build_install_plan(resolution, installed.clone(), now, 600).expect("plan");
    let confirmation = plan
        .approval_challenge
        .as_ref()
        .expect("approval challenge")
        .required_phrase
        .clone();
    storage
        .store_action_plan(&StoredActionPlan {
            id: plan.plan_id,
            capability: "package.install.plan.winget".to_owned(),
            resource_key: plan.lock_key.clone(),
            status: "AWAITING_APPROVAL".to_owned(),
            plan_hash: plan.plan_hash.clone(),
            created_at: plan.created_at,
            expires_at: plan.expires_at,
            approval_phrase: confirmation.clone(),
            record_json: serde_json::to_string(&plan).expect("plan json"),
        })
        .expect("store plan");

    let receipt = build_approval_receipt(&plan, &confirmation, now, 300).expect("receipt");
    let mut approved_plan = plan.clone();
    approved_plan.status = InstallPlanStatus::ApprovedAwaitingExecution;
    approved_plan.execution_enabled = true;
    approved_plan.approval_challenge = None;
    let stored_receipt = StoredApprovalReceipt {
        id: receipt.approval_id,
        plan_id: receipt.plan_id,
        plan_hash: receipt.plan_hash.clone(),
        approved_at: receipt.approved_at,
        expires_at: receipt.expires_at,
        resource_key: receipt.lock_key.clone(),
        record_json: serde_json::to_string(&receipt).expect("receipt json"),
    };
    let lock = StoredResourceLock {
        resource_key: receipt.lock_key.clone(),
        holder_plan_id: receipt.plan_id,
        acquired_at: receipt.approved_at,
        expires_at: receipt.lock_expires_at,
    };
    storage
        .approve_action_plan(&ActionPlanApproval {
            plan_id: plan.plan_id,
            expected_hash: &plan.plan_hash,
            confirmation: &confirmation,
            updated_plan_json: &serde_json::to_string(&approved_plan).expect("approved json"),
            receipt: &stored_receipt,
            lock: &lock,
            now,
        })
        .expect("approve plan");

    let execution_id = Uuid::new_v4();
    let mut executing_plan = approved_plan;
    executing_plan.status = InstallPlanStatus::Executing;
    executing_plan.execution_enabled = false;
    let pre_state = build_residual_state_manifest(
        executing_plan.selector.clone(),
        executing_plan.resolution.provider_version.clone(),
        installed,
        now,
    );
    let mut journal = WingetExecutionJournal {
        execution_id,
        plan_id: executing_plan.plan_id,
        approval_id: receipt.approval_id,
        plan_hash: executing_plan.plan_hash.clone(),
        resource_key: executing_plan.lock_key.clone(),
        phase: ExecutionJournalPhase::Prepared,
        provider_id: "winget".to_owned(),
        provider_version: Some("v1.12.0".to_owned()),
        command: executing_plan.install_preview.clone(),
        pre_state,
        process_identity: None,
        provider_result: None,
        recovery_policy: WingetRecoveryPolicy::default(),
        created_at: now,
        updated_at: now,
        resolved_at: None,
        recovery_status: None,
    };
    storage
        .begin_action_execution(&ActionExecutionStart {
            plan_id: executing_plan.plan_id,
            approval_id: receipt.approval_id,
            expected_hash: &executing_plan.plan_hash,
            updated_plan_json: &serde_json::to_string(&executing_plan).expect("executing json"),
            now,
            lock_expires_at: now + ChronoDuration::minutes(32),
            journal: &journal,
        })
        .expect("begin execution");

    if phase != ExecutionJournalPhase::Prepared {
        journal.phase = ExecutionJournalPhase::SpawnIntent;
        journal.updated_at = now + ChronoDuration::seconds(1);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Prepared, &journal)
            .expect("spawn intent");
    }
    if matches!(
        phase,
        ExecutionJournalPhase::Spawned | ExecutionJournalPhase::ProviderFinished
    ) {
        journal.phase = ExecutionJournalPhase::Spawned;
        journal.process_identity = Some(WingetProcessIdentity {
            root_pid: 4242,
            containment_method: "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE".to_owned(),
            started_at: now + ChronoDuration::seconds(2),
        });
        journal.updated_at = now + ChronoDuration::seconds(2);
        storage
            .transition_execution_journal(ExecutionJournalPhase::SpawnIntent, &journal)
            .expect("spawned");
    }
    if phase == ExecutionJournalPhase::ProviderFinished {
        journal.phase = ExecutionJournalPhase::ProviderFinished;
        journal.provider_result = Some(WingetPersistedProviderResult {
            completed_at: now + ChronoDuration::seconds(3),
            process_evidence: process_evidence(0),
            containment_evidence: ProcessContainmentEvidence {
                method: "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE".to_owned(),
                root_pid: Some(4242),
                stop_reason: ProcessStopReason::Exited,
                active_processes_after_cleanup: Some(0),
                descendants_terminated: Some(true),
                containment_confirmed: true,
                detail: "fixture process tree exited".to_owned(),
            },
        });
        journal.updated_at = now + ChronoDuration::seconds(3);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Spawned, &journal)
            .expect("provider finished");
    }

    drop(storage);
    SeededRecovery {
        directory,
        database_path,
        execution_id,
        plan_id: executing_plan.plan_id,
    }
}

fn restarted_state(seed: &SeededRecovery) -> AppState {
    AppState {
        storage: Storage::initialize(&seed.database_path).expect("reopened storage"),
        started_at: Utc::now(),
        system_adapter_path: seed.directory.path().join("missing-system-adapter"),
        winget_adapter_path: seed.directory.path().join("missing-winget-adapter"),
        active_executions: Arc::new(Mutex::new(HashMap::<Uuid, ActiveExecution>::new())),
    }
}

async fn reconcile(
    phase: ExecutionJournalPhase,
) -> (
    SeededRecovery,
    AppState,
    toolos_winget::WingetRecoveryReport,
) {
    let seed = seed_recovery(phase);
    let state = restarted_state(&seed);
    let reports = reconcile_startup(&state, Uuid::new_v4())
        .await
        .expect("startup reconciliation");
    assert_eq!(reports.len(), 1);
    let report = reports.into_iter().next().expect("one report");
    assert_eq!(report.execution_id, seed.execution_id);
    (seed, state, report)
}

#[tokio::test]
async fn restart_after_prepared_releases_lock_and_unblocks_writes() {
    let (seed, state, report) = reconcile(ExecutionJournalPhase::Prepared).await;
    assert_eq!(report.status, RecoveryStatus::RecoveredNoProcessStarted);
    assert!(!report.lock_retained);
    assert!(!report.mutable_operations_blocked);
    assert!(state
        .storage
        .get_resource_lock("package-manager:winget", Utc::now())
        .expect("lock query")
        .is_none());
    assert!(!state
        .storage
        .has_blocking_recovery()
        .expect("blocking query"));
    ensure_mutation_allowed(&state).expect("writes unblocked");
    let plan = state
        .storage
        .get_action_plan(seed.plan_id)
        .expect("plan query")
        .expect("plan");
    assert_eq!(plan.status, "RECOVERED_NO_PROCESS_STARTED");
}

#[tokio::test]
async fn restart_after_spawn_intent_retains_lock_and_blocks_writes() {
    let (_, state, report) = reconcile(ExecutionJournalPhase::SpawnIntent).await;
    assert_eq!(report.status, RecoveryStatus::UnknownRequiresRecovery);
    assert!(report.lock_retained);
    assert!(report.mutable_operations_blocked);
    assert!(state
        .storage
        .get_resource_lock("package-manager:winget", Utc::now())
        .expect("lock query")
        .is_some());
    assert!(state
        .storage
        .has_blocking_recovery()
        .expect("blocking query"));
    assert!(ensure_mutation_allowed(&state).is_err());
}

#[tokio::test]
async fn restart_after_spawned_retains_lock_and_blocks_writes() {
    let (_, state, report) = reconcile(ExecutionJournalPhase::Spawned).await;
    assert_eq!(report.status, RecoveryStatus::UnknownRequiresRecovery);
    assert!(report.lock_retained);
    assert!(state
        .storage
        .has_blocking_recovery()
        .expect("blocking query"));
    assert!(ensure_mutation_allowed(&state).is_err());
}

#[tokio::test]
async fn restart_after_provider_finished_finalizes_without_replay() {
    let (seed, state, report) = reconcile(ExecutionJournalPhase::ProviderFinished).await;
    assert_eq!(
        report.status,
        RecoveryStatus::RecoveredFromPersistedProviderResult
    );
    assert!(!report.lock_retained);
    assert!(!state
        .storage
        .has_blocking_recovery()
        .expect("blocking query"));
    ensure_mutation_allowed(&state).expect("writes unblocked");
    let journal = state
        .storage
        .get_execution_journal(seed.execution_id)
        .expect("journal query")
        .expect("journal");
    assert_eq!(journal.phase, ExecutionJournalPhase::Finalized);
    assert_eq!(
        journal.recovery_status,
        Some(RecoveryStatus::RecoveredFromPersistedProviderResult)
    );
    let plan = state
        .storage
        .get_action_plan(seed.plan_id)
        .expect("plan query")
        .expect("plan");
    assert_eq!(plan.status, "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT");
}
