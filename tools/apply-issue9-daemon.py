from pathlib import Path


def patch(path_text: str, replacements: list[tuple[str, str, str]]) -> None:
    path = Path(path_text)
    source = path.read_text(encoding="utf-8")
    for old, new, label in replacements:
        count = source.count(old)
        if count != 1:
            raise RuntimeError(f"{path_text} {label}: expected one match, found {count}")
        source = source.replace(old, new, 1)
    path.write_text(source, encoding="utf-8", newline="\n")


patch(
    "crates/toolos-winget/src/lib.rs",
    [
        (
            "    ExecutionCancelled,\n    UnknownRequiresRecovery,\n    Expired,\n",
            "    ExecutionCancelled,\n    RecoveredNoProcessStarted,\n    RecoveredFromPersistedProviderResult,\n    UnknownRequiresRecovery,\n    Expired,\n",
            "recovery plan statuses",
        )
    ],
)

patch(
    "crates/toolos-storage/src/recovery.rs",
    [
        (
            "        ) | (\n            ExecutionJournalPhase::Spawned,\n            ExecutionJournalPhase::ProviderFinished\n        )\n",
            "        ) | (\n            ExecutionJournalPhase::SpawnIntent,\n            ExecutionJournalPhase::ProviderFinished\n        ) | (\n            ExecutionJournalPhase::Spawned,\n            ExecutionJournalPhase::ProviderFinished\n        )\n",
            "spawn failure transition",
        )
    ],
)

patch(
    "apps/toolos-daemon/src/main.rs",
    [
        (
            "mod contained_adapter;\n",
            "mod contained_adapter;\nmod recovery;\n",
            "recovery module",
        ),
        (
            '''use toolos_winget::{
    build_approval_receipt, build_execution_report, build_install_plan, validate_execution_request,
    InstallPlanStatus, PackageScope, ProcessEvidence, WingetExecutionStatus,
    WingetInstallApprovalReceipt, WingetInstallExecutionRequest, WingetInstallPlan,
    WingetInstalledStateReport, WingetResolutionReport,
};
''',
            '''use toolos_winget::{
    build_approval_receipt, build_execution_report, build_install_plan,
    build_residual_state_manifest, validate_execution_request, ExecutionJournalPhase,
    InstallPlanStatus, PackageScope, ProcessEvidence, WingetExecutionJournal,
    WingetExecutionStatus, WingetInstallApprovalReceipt, WingetInstallExecutionRequest,
    WingetInstallPlan, WingetInstalledStateReport, WingetPersistedProviderResult,
    WingetProcessIdentity, WingetRecoveryPolicy, WingetResolutionReport,
};
''',
            "recovery imports",
        ),
        (
            '''    let startup_trace = Uuid::new_v4();
    state.storage.append_event(
''',
            '''    let startup_trace = Uuid::new_v4();
    let recovery_reports = recovery::reconcile_startup(&state, startup_trace).await?;
    state.storage.append_event(
''',
            "startup reconciliation",
        ),
        (
            '''            "winget_adapter_path": state.winget_adapter_path.to_string_lossy()
''',
            '''            "winget_adapter_path": state.winget_adapter_path.to_string_lossy(),
            "reconciled_executions": recovery_reports.len(),
            "mutable_operations_blocked": state.storage.has_blocking_recovery()?
''',
            "startup recovery evidence",
        ),
        (
            '''        "winget.install.lock" => winget_install_lock(state),
        "evidence.list" => {
''',
            '''        "winget.install.lock" => winget_install_lock(state),
        "winget.recovery.list" => recovery::list(state),
        "winget.recovery.get" => recovery::get(state, &request.params),
        "evidence.list" => {
''',
            "recovery rpc routes",
        ),
        (
            '''fn winget_install_approve(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.approve")?;
''',
            '''fn winget_install_approve(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    recovery::ensure_mutation_allowed(state)?;
    let plan_id = required_uuid(params, "plan_id", "winget.install.approve")?;
''',
            "approval recovery gate",
        ),
        (
            '''async fn winget_install_execute(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let execution_id = required_uuid(params, "execution_id", "winget.install.execute")?;
''',
            '''async fn winget_install_execute(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    recovery::ensure_mutation_allowed(state)?;
    let execution_id = required_uuid(params, "execution_id", "winget.install.execute")?;
''',
            "execution recovery gate",
        ),
        (
            '''    let request_json = serde_json::to_value(&request)?;
    let started_at = Utc::now();
    plan.status = InstallPlanStatus::Executing;
''',
            '''    let request_json = serde_json::to_value(&request)?;
    let started_at = Utc::now();
    let pre_state = build_residual_state_manifest(
        plan.selector.clone(),
        preflight_resolution.provider_version.clone(),
        preflight_installed_state.clone(),
        started_at,
    );
    let mut journal = WingetExecutionJournal {
        execution_id,
        plan_id,
        approval_id,
        plan_hash: plan.plan_hash.clone(),
        resource_key: plan.lock_key.clone(),
        phase: ExecutionJournalPhase::Prepared,
        provider_id: "winget".to_owned(),
        provider_version: preflight_resolution.provider_version.clone(),
        command: command.clone(),
        pre_state,
        process_identity: None,
        provider_result: None,
        recovery_policy: WingetRecoveryPolicy::default(),
        created_at: started_at,
        updated_at: started_at,
        resolved_at: None,
        recovery_status: None,
    };
    plan.status = InstallPlanStatus::Executing;
''',
            "prepared journal",
        ),
        (
            '''        now: started_at,
        lock_expires_at: started_at + ChronoDuration::minutes(32),
    }) {
''',
            '''        now: started_at,
        lock_expires_at: started_at + ChronoDuration::minutes(32),
        journal: &journal,
    }) {
''',
            "atomic prepared journal",
        ),
        (
            '''    let (process_evidence, containment_evidence) = match spawn_mutating_adapter(
''',
            '''    journal.phase = ExecutionJournalPhase::SpawnIntent;
    journal.updated_at = Utc::now();
    state.storage.transition_execution_journal(
        ExecutionJournalPhase::Prepared,
        &journal,
    )?;

    let (process_evidence, containment_evidence, provider_expected_phase) = match spawn_mutating_adapter(
''',
            "spawn intent",
        ),
        (
            '''            let control = contained.control();
            {
''',
            '''            let control = contained.control();
            journal.phase = ExecutionJournalPhase::Spawned;
            journal.process_identity = Some(WingetProcessIdentity {
                root_pid,
                containment_method: "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE".to_owned(),
                started_at: Utc::now(),
            });
            journal.updated_at = Utc::now();
            if let Err(error) = state.storage.transition_execution_journal(
                ExecutionJournalPhase::SpawnIntent,
                &journal,
            ) {
                let _ = control.cancel(ProcessStopReason::ContainmentFailed);
                let _ = contained.wait(&command).await;
                state.active_executions.lock().await.remove(&execution_id);
                return Err(error.into());
            }
            {
''',
            "spawned journal",
        ),
        (
            '''                Ok(outcome) => (outcome.process_evidence, outcome.containment),
                Err(error) => (
                    failed_process_evidence(&command, &error.to_string(), false),
                    containment_failure(&error),
                ),
''',
            '''                Ok(outcome) => (
                    outcome.process_evidence,
                    outcome.containment,
                    ExecutionJournalPhase::Spawned,
                ),
                Err(error) => (
                    failed_process_evidence(&command, &error.to_string(), false),
                    containment_failure(&error),
                    ExecutionJournalPhase::Spawned,
                ),
''',
            "spawned provider outcome",
        ),
        (
            '''            (
                failed_process_evidence(&command, &error.to_string(), false),
                containment_failure(&error),
            )
''',
            '''            (
                failed_process_evidence(&command, &error.to_string(), false),
                containment_failure(&error),
                ExecutionJournalPhase::SpawnIntent,
            )
''',
            "spawn failure provider outcome",
        ),
        (
            '''    let containment_confirmed = containment_evidence.containment_confirmed
''',
            '''    journal.phase = ExecutionJournalPhase::ProviderFinished;
    journal.provider_result = Some(WingetPersistedProviderResult {
        completed_at: Utc::now(),
        process_evidence: process_evidence.clone(),
        containment_evidence: containment_evidence.clone(),
    });
    journal.updated_at = Utc::now();
    state
        .storage
        .transition_execution_journal(provider_expected_phase, &journal)?;

    let containment_confirmed = containment_evidence.containment_confirmed
''',
            "provider finished journal",
        ),
        (
            '''    let final_plan_json = serde_json::to_string(&plan)?;
    let evidence = EvidenceRecord::new(
''',
            '''    let final_plan_json = serde_json::to_string(&plan)?;
    journal.phase = ExecutionJournalPhase::Finalized;
    journal.updated_at = Utc::now();
    journal.resolved_at = Some(journal.updated_at);
    let evidence = EvidenceRecord::new(
''',
            "finalized journal",
        ),
        (
            '''            resource_key: &plan.lock_key,
            release_resource_lock,
        })?;
''',
            '''            resource_key: &plan.lock_key,
            release_resource_lock,
            journal: &journal,
        })?;
''',
            "atomic finalization journal",
        ),
        (
            '''        InstallPlanStatus::ExecutionCancelled => "EXECUTION_CANCELLED",
        InstallPlanStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
''',
            '''        InstallPlanStatus::ExecutionCancelled => "EXECUTION_CANCELLED",
        InstallPlanStatus::RecoveredNoProcessStarted => "RECOVERED_NO_PROCESS_STARTED",
        InstallPlanStatus::RecoveredFromPersistedProviderResult => {
            "RECOVERED_FROM_PERSISTED_PROVIDER_RESULT"
        }
        InstallPlanStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
''',
            "plan status mapping",
        ),
        (
            '''        {
            "capability_id": "package.preview.install",
''',
            '''        {
            "capability_id": "package.execution.recovery.winget",
            "provider_id": "toolos.daemon.recovery",
            "blast_radius": "READ_ONLY_AND_LOCAL_METADATA_WRITE",
            "status": "IMPLEMENTED_FAIL_CLOSED"
        },
        {
            "capability_id": "package.preview.install",
''',
            "recovery capability",
        ),
    ],
)
