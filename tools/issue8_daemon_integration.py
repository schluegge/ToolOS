from pathlib import Path

ROOT = Path.cwd()


def replace_exact(path: str, old: str, new: str) -> None:
    target = ROOT / path
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one match, found {count}: {old[:120]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


def replace_between(path: str, start: str, end: str, replacement: str) -> None:
    target = ROOT / path
    source = target.read_text(encoding="utf-8")
    start_index = source.find(start)
    if start_index < 0:
        raise RuntimeError(f"{path}: start marker not found: {start!r}")
    end_index = source.find(end, start_index)
    if end_index < 0:
        raise RuntimeError(f"{path}: end marker not found: {end!r}")
    target.write_text(
        source[:start_index] + replacement + source[end_index:],
        encoding="utf-8",
        newline="\n",
    )


replace_exact(
    "crates/toolos-winget/src/lib.rs",
    "    ExecutionSucceededUnverified,\n    ExecutionFailed,\n    Expired,\n",
    "    ExecutionSucceededUnverified,\n    ExecutionFailed,\n    ExecutionTimedOut,\n    ExecutionCancelled,\n    UnknownRequiresRecovery,\n    Expired,\n",
)

replace_exact(
    "crates/toolos-storage/src/lib.rs",
    "pub struct ActionExecutionFinish<'a> {\n    pub plan_id: Uuid,\n    pub final_status: &'a str,\n    pub updated_plan_json: &'a str,\n    pub resource_key: &'a str,\n}\n",
    "pub struct ActionExecutionFinish<'a> {\n    pub plan_id: Uuid,\n    pub final_status: &'a str,\n    pub updated_plan_json: &'a str,\n    pub resource_key: &'a str,\n    pub release_resource_lock: bool,\n}\n",
)
replace_exact(
    "crates/toolos-storage/src/lib.rs",
    "        transaction.execute(\n            \"DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2\",\n            params![execution.resource_key, execution.plan_id.to_string()],\n        )?;\n",
    "        if execution.release_resource_lock {\n            transaction.execute(\n                \"DELETE FROM resource_lock WHERE resource_key = ?1 AND holder_plan_id = ?2\",\n                params![execution.resource_key, execution.plan_id.to_string()],\n            )?;\n        } else {\n            let retained = transaction.execute(\n                \"UPDATE resource_lock SET expires_at = '9999-12-31T23:59:59+00:00'\n                 WHERE resource_key = ?1 AND holder_plan_id = ?2\",\n                params![execution.resource_key, execution.plan_id.to_string()],\n            )?;\n            if retained != 1 {\n                return Err(StorageError::ResourceLocked {\n                    resource_key: execution.resource_key.to_owned(),\n                    holder_plan_id: execution.plan_id.to_string(),\n                    expires_at: \"missing recovery lock\".to_owned(),\n                });\n            }\n        }\n",
)
replace_exact(
    "crates/toolos-storage/src/lib.rs",
    "    #[test]\n    fn evidence_and_events_round_trip() {\n",
    "    #[test]\n    fn unknown_execution_retains_resource_lock() {\n        let directory = tempdir().expect(\"temp directory\");\n        let storage = Storage::initialize(directory.path().join(\"toolos.db\")).expect(\"storage\");\n        let now = Utc::now();\n        let plan_id = Uuid::new_v4();\n        let connection = storage.open_connection().expect(\"connection\");\n        connection\n            .execute(\n                \"INSERT INTO action_plan (id, capability, resource_key, status, plan_hash, created_at, expires_at, approval_phrase, record_json)\n                 VALUES (?1, 'package.install.plan.winget', 'package-manager:winget', 'EXECUTING', 'abc', ?2, ?3, '', '{}')\",\n                params![\n                    plan_id.to_string(),\n                    now.to_rfc3339(),\n                    (now + chrono::Duration::minutes(10)).to_rfc3339()\n                ],\n            )\n            .expect(\"insert executing plan\");\n        connection\n            .execute(\n                \"INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)\n                 VALUES ('package-manager:winget', ?1, ?2, ?3)\",\n                params![\n                    plan_id.to_string(),\n                    now.to_rfc3339(),\n                    (now + chrono::Duration::minutes(30)).to_rfc3339()\n                ],\n            )\n            .expect(\"insert lock\");\n        drop(connection);\n\n        storage\n            .finish_action_execution(&ActionExecutionFinish {\n                plan_id,\n                final_status: \"UNKNOWN_REQUIRES_RECOVERY\",\n                updated_plan_json: \"{}\",\n                resource_key: \"package-manager:winget\",\n                release_resource_lock: false,\n            })\n            .expect(\"finish unknown execution\");\n\n        let lock = storage\n            .get_resource_lock(\n                \"package-manager:winget\",\n                now + chrono::Duration::days(3650),\n            )\n            .expect(\"query retained lock\")\n            .expect(\"retained lock\");\n        assert_eq!(lock.holder_plan_id, plan_id);\n    }\n\n    #[test]\n    fn evidence_and_events_round_trip() {\n",
)

replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "use std::path::{Path, PathBuf};\n",
    "mod contained_adapter;\n\nuse std::collections::HashMap;\nuse std::path::{Path, PathBuf};\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "use tokio::process::Command;\n",
    "use tokio::process::Command;\nuse tokio::sync::Mutex;\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "use toolos_storage::{\n",
    "use toolos_process::{ContainedProcessControl, ProcessContainmentEvidence, ProcessStopReason};\nuse toolos_storage::{\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "use uuid::Uuid;\n\n#[derive(Clone)]\nstruct AppState {\n",
    "use uuid::Uuid;\n\nuse contained_adapter::{containment_failure, spawn_mutating_adapter};\n\nstruct ActiveExecution {\n    plan_id: Uuid,\n    control: Option<ContainedProcessControl>,\n}\n\n#[derive(Clone)]\nstruct AppState {\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "    winget_adapter_path: PathBuf,\n}\n",
    "    winget_adapter_path: PathBuf,\n    active_executions: Arc<Mutex<HashMap<Uuid, ActiveExecution>>>,\n}\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "        winget_adapter_path,\n    });\n",
    "        winget_adapter_path,\n        active_executions: Arc::new(Mutex::new(HashMap::new())),\n    });\n",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    '        "winget.install.execute" => winget_install_execute(state, trace_id, &request.params).await,\n        "winget.install.lock" => winget_install_lock(state),\n',
    '        "winget.install.execute" => winget_install_execute(state, trace_id, &request.params).await,\n        "winget.install.cancel" => winget_install_cancel(state, trace_id, &request.params).await,\n        "winget.install.lock" => winget_install_lock(state),\n',
)

new_execute = r'''async fn winget_install_execute(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let execution_id = required_uuid(params, "execution_id", "winget.install.execute")?;
    let plan_id = required_uuid(params, "plan_id", "winget.install.execute")?;
    let approval_id = required_uuid(params, "approval_id", "winget.install.execute")?;
    let confirmation = required_string(params, "confirmation", "winget.install.execute")?;
    let stored_plan = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    let mut plan: WingetInstallPlan = serde_json::from_str(&stored_plan.record_json)?;
    let stored_receipt = state
        .storage
        .get_approval_receipt(approval_id)?
        .with_context(|| format!("approval receipt not found: {approval_id}"))?;
    let receipt: WingetInstallApprovalReceipt = serde_json::from_str(&stored_receipt.record_json)?;
    let now = Utc::now();
    let lock = state
        .storage
        .get_resource_lock(&plan.lock_key, now)?
        .context("the approved WinGet lock is absent or expired")?;

    validate_execution_authorization(
        &plan,
        &receipt,
        &lock,
        &confirmation,
        &stored_plan.plan_hash,
        now,
    )?;

    let selector_json = serde_json::to_value(&plan.selector)?;
    let resolution_result = winget_resolve(state, trace_id, &selector_json).await?;
    let installed_result = winget_installed(state, trace_id, &selector_json).await?;
    let preflight_resolution: WingetResolutionReport = serde_json::from_value(
        resolution_result
            .get("snapshot")
            .cloned()
            .context("preflight resolution result had no snapshot")?,
    )?;
    let preflight_installed_state: WingetInstalledStateReport = serde_json::from_value(
        installed_result
            .get("snapshot")
            .cloned()
            .context("preflight installed-state result had no snapshot")?,
    )?;
    validate_fresh_preflight(&plan, &preflight_resolution, &preflight_installed_state)?;

    let request = WingetInstallExecutionRequest {
        plan_id,
        plan_hash: plan.plan_hash.clone(),
        selector: plan.selector.clone(),
        expected_command: plan.install_preview.clone(),
    };
    let command = validate_execution_request(&request).map_err(anyhow::Error::msg)?;
    let request_json = serde_json::to_value(&request)?;
    let started_at = Utc::now();
    plan.status = InstallPlanStatus::Executing;
    plan.execution_enabled = false;
    plan.single_safest_next_action =
        "WinGet execution is in progress. Do not start another package-manager operation."
            .to_owned();
    let executing_plan_json = serde_json::to_string(&plan)?;

    {
        let mut active = state.active_executions.lock().await;
        if active.contains_key(&execution_id) {
            return Err(anyhow!("execution ID is already active: {execution_id}"));
        }
        active.insert(
            execution_id,
            ActiveExecution {
                plan_id,
                control: None,
            },
        );
    }

    if let Err(error) = state.storage.begin_action_execution(&ActionExecutionStart {
        plan_id,
        approval_id,
        expected_hash: &plan.plan_hash,
        updated_plan_json: &executing_plan_json,
        now: started_at,
        lock_expires_at: started_at + ChronoDuration::minutes(32),
    }) {
        state.active_executions.lock().await.remove(&execution_id);
        return Err(error.into());
    }

    let (process_evidence, containment_evidence) = match spawn_mutating_adapter(
        execution_id,
        &state.winget_adapter_path,
        "winget.install.execute",
        request_json,
        Duration::from_secs(31 * 60),
    ) {
        Ok(contained) => {
            let containment_execution_id = contained.execution_id();
            let root_pid = contained.root_pid();
            let control = contained.control();
            {
                let mut active = state.active_executions.lock().await;
                let slot = active
                    .get_mut(&execution_id)
                    .context("active execution reservation disappeared before spawn")?;
                slot.control = Some(control);
            }
            if let Err(error) = state.storage.append_event(
                trace_id,
                "winget.install.execution.started",
                &json!({
                    "execution_id": execution_id,
                    "containment_execution_id": containment_execution_id,
                    "plan_id": plan_id,
                    "approval_id": approval_id,
                    "plan_hash": plan.plan_hash,
                    "command": command,
                    "containment_method": "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE",
                    "root_pid": root_pid,
                    "agreements_accepted": false,
                    "elevation_requested": false
                }),
            ) {
                error!(%error, "failed to persist contained execution start event");
            }

            let outcome = contained.wait(&command).await;
            state.active_executions.lock().await.remove(&execution_id);
            match outcome {
                Ok(outcome) => (outcome.process_evidence, outcome.containment),
                Err(error) => (
                    failed_process_evidence(&command, &error.to_string(), false),
                    containment_failure(&error),
                ),
            }
        }
        Err(error) => {
            state.active_executions.lock().await.remove(&execution_id);
            (
                failed_process_evidence(&command, &error.to_string(), false),
                containment_failure(&error),
            )
        }
    };

    let containment_confirmed = containment_evidence.containment_confirmed
        && containment_evidence.active_processes_after_cleanup == Some(0);
    let post_install_state = if containment_confirmed {
        match winget_installed(state, trace_id, &selector_json).await {
            Ok(value) => value
                .get("snapshot")
                .cloned()
                .map(serde_json::from_value::<WingetInstalledStateReport>)
                .transpose()?,
            Err(error) => {
                state.storage.append_event(
                    trace_id,
                    "winget.install.post_state.failed",
                    &json!({"execution_id": execution_id, "error": error.to_string()}),
                )?;
                None
            }
        }
    } else {
        state.storage.append_event(
            trace_id,
            "winget.install.post_state.skipped",
            &json!({
                "execution_id": execution_id,
                "reason": "process-tree termination was not confirmed"
            }),
        )?;
        None
    };

    let completed_at = Utc::now();
    let report = build_execution_report(
        execution_id,
        plan_id,
        approval_id,
        plan.plan_hash.clone(),
        plan.selector.clone(),
        command,
        started_at,
        completed_at,
        process_evidence,
        containment_evidence,
        preflight_resolution,
        preflight_installed_state,
        post_install_state,
    );
    plan.status = match report.status {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified => {
            InstallPlanStatus::ExecutionSucceededUnverified
        }
        WingetExecutionStatus::ProviderFailed => InstallPlanStatus::ExecutionFailed,
        WingetExecutionStatus::TimedOutContained => InstallPlanStatus::ExecutionTimedOut,
        WingetExecutionStatus::CancelledContained => InstallPlanStatus::ExecutionCancelled,
        WingetExecutionStatus::UnknownRequiresRecovery => {
            InstallPlanStatus::UnknownRequiresRecovery
        }
    };
    plan.single_safest_next_action = report.single_safest_next_action.clone();
    let final_status = install_plan_status(&plan.status);
    let release_resource_lock =
        report.status != WingetExecutionStatus::UnknownRequiresRecovery;
    let final_plan_json = serde_json::to_string(&plan)?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-execution:{execution_id}"),
        format!(
            "Governed WinGet execution completed with status {}",
            execution_status(&report.status)
        ),
        "toolos.adapter.winget",
        serde_json::to_value(&report)?,
        report.limitations.clone(),
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALL_EXECUTION")?;
    state
        .storage
        .finish_action_execution(&ActionExecutionFinish {
            plan_id,
            final_status,
            updated_plan_json: &final_plan_json,
            resource_key: &plan.lock_key,
            release_resource_lock,
        })?;
    state.storage.append_event(
        trace_id,
        "winget.install.execution.completed",
        &json!({
            "execution_id": execution_id,
            "plan_id": plan_id,
            "status": execution_status(&report.status),
            "exit_code": report.process_evidence.exit_code,
            "timed_out": report.process_evidence.timed_out,
            "containment_confirmed": report.containment_evidence.containment_confirmed,
            "active_processes_after_cleanup": report.containment_evidence.active_processes_after_cleanup,
            "post_state_captured": report.post_install_state.is_some(),
            "resource_lock_released": release_resource_lock
        }),
    )?;
    Ok(json!({"plan": plan, "report": report, "evidence": evidence}))
}

async fn winget_install_cancel(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let execution_id = required_uuid(params, "execution_id", "winget.install.cancel")?;
    let (plan_id, control) = {
        let active = state.active_executions.lock().await;
        let execution = active
            .get(&execution_id)
            .with_context(|| format!("active execution not found: {execution_id}"))?;
        let control = execution
            .control
            .clone()
            .context("contained process has not started yet; retry cancellation")?;
        (execution.plan_id, control)
    };
    control.cancel(ProcessStopReason::Cancelled)?;
    state.storage.append_event(
        trace_id,
        "winget.install.execution.cancel_requested",
        &json!({"execution_id": execution_id, "plan_id": plan_id}),
    )?;
    Ok(json!({
        "execution_id": execution_id,
        "plan_id": plan_id,
        "cancel_requested": true
    }))
}

fn failed_process_evidence(
    command: &toolos_winget::CommandPreview,
    message: &str,
    timed_out: bool,
) -> ProcessEvidence {
    ProcessEvidence {
        executable: command.executable.clone(),
        args: command.args.clone(),
        exit_code: None,
        stdout: String::new(),
        stderr: message.to_owned(),
        timed_out,
        duration_ms: 0,
    }
}

'''
replace_between(
    "apps/toolos-daemon/src/main.rs",
    "async fn winget_install_execute(\n",
    "fn validate_execution_authorization(\n",
    new_execute,
)

replace_exact(
    "apps/toolos-daemon/src/main.rs",
    "        InstallPlanStatus::ExecutionSucceededUnverified => \"EXECUTION_SUCCEEDED_UNVERIFIED\",\n        InstallPlanStatus::ExecutionFailed => \"EXECUTION_FAILED\",\n        InstallPlanStatus::Expired => \"EXPIRED\",\n",
    "        InstallPlanStatus::ExecutionSucceededUnverified => \"EXECUTION_SUCCEEDED_UNVERIFIED\",\n        InstallPlanStatus::ExecutionFailed => \"EXECUTION_FAILED\",\n        InstallPlanStatus::ExecutionTimedOut => \"EXECUTION_TIMED_OUT\",\n        InstallPlanStatus::ExecutionCancelled => \"EXECUTION_CANCELLED\",\n        InstallPlanStatus::UnknownRequiresRecovery => \"UNKNOWN_REQUIRES_RECOVERY\",\n        InstallPlanStatus::Expired => \"EXPIRED\",\n",
)
replace_between(
    "apps/toolos-daemon/src/main.rs",
    "fn execution_status(status: &WingetExecutionStatus) -> &'static str {\n",
    "fn bounded_limit(\n",
    """fn execution_status(status: &WingetExecutionStatus) -> &'static str {
    match status {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified => {
            "PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED"
        }
        WingetExecutionStatus::ProviderFailed => "PROVIDER_FAILED",
        WingetExecutionStatus::TimedOutContained => "TIMED_OUT_CONTAINED",
        WingetExecutionStatus::CancelledContained => "CANCELLED_CONTAINED",
        WingetExecutionStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
    }
}

""",
)
replace_exact(
    "apps/toolos-daemon/src/main.rs",
    '        {\n            "capability_id": "package.install.execute.winget",\n            "provider_id": "toolos.adapter.winget",\n            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",\n            "status": "IMPLEMENTED_USER_SCOPE_PINNED"\n        },\n',
    '        {\n            "capability_id": "package.install.execute.winget",\n            "provider_id": "toolos.adapter.winget",\n            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",\n            "status": "IMPLEMENTED_USER_SCOPE_PINNED_JOB_OBJECT"\n        },\n        {\n            "capability_id": "package.install.cancel.winget",\n            "provider_id": "toolos.daemon.governance",\n            "blast_radius": "PROCESS_TREE_TERMINATION",\n            "status": "IMPLEMENTED_JOB_OBJECT"\n        },\n',
)
