use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use toolos_domain::{
    EvidenceKind, EvidenceRecord, HealthReport, ProjectInspectParams, RpcRequest, RpcResponse,
};
use toolos_storage::{
    ActionExecutionFinish, ActionExecutionStart, ActionPlanApproval, Storage, StoredActionPlan,
    StoredApprovalReceipt, StoredResourceLock,
};
use toolos_windows_job::{CancellationReason, CancellationToken};
use toolos_winget::{
    build_approval_receipt, build_execution_report, build_install_plan, validate_execution_request,
    CommandPreview, InstallPlanStatus, PackageScope, ProcessContainmentEvidence, ProcessEvidence,
    ProcessTerminationReason, WingetExecutionStatus, WingetInstallApprovalReceipt,
    WingetInstallExecutionRequest, WingetInstallPlan, WingetInstalledStateReport,
    WingetResolutionReport,
};
use tracing::{error, info, instrument};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod contained_adapter;

#[derive(Clone)]
struct AppState {
    storage: Storage,
    started_at: DateTime<Utc>,
    system_adapter_path: PathBuf,
    winget_adapter_path: PathBuf,
    active_install_executions: Arc<tokio::sync::Mutex<HashMap<Uuid, CancellationToken>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_tracing();
    let database_path = database_path()?;
    let storage = Storage::initialize(&database_path).context("initialize ToolOS database")?;
    let system_adapter_path = sibling_adapter_path(
        "TOOLOS_SYSTEM_ADAPTER",
        "toolos-system-adapter.exe",
        "toolos-system-adapter",
    )?;
    let winget_adapter_path = sibling_adapter_path(
        "TOOLOS_WINGET_ADAPTER",
        "toolos-winget-adapter.exe",
        "toolos-winget-adapter",
    )?;
    let started_at = Utc::now();
    storage.set_metadata("daemon.started_at", &started_at.to_rfc3339())?;
    let state = Arc::new(AppState {
        storage,
        started_at,
        system_adapter_path,
        winget_adapter_path,
        active_install_executions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    });

    let startup_trace = Uuid::new_v4();
    state.storage.append_event(
        startup_trace,
        "daemon.started",
        &json!({
            "version": env!("CARGO_PKG_VERSION"),
            "database_path": state.storage.path().to_string_lossy(),
            "system_adapter_path": state.system_adapter_path.to_string_lossy(),
            "winget_adapter_path": state.winget_adapter_path.to_string_lossy()
        }),
    )?;

    info!(
        database = %state.storage.path().display(),
        system_adapter = %state.system_adapter_path.display(),
        winget_adapter = %state.winget_adapter_path.display(),
        "ToolOS daemon listening"
    );

    let handler_state = Arc::clone(&state);
    toolos_ipc::serve(move |request| {
        let state = Arc::clone(&handler_state);
        async move { handle_request(state, request).await }
    })
    .await
    .context("serve ToolOS local IPC")
}

#[instrument(skip(state, request), fields(method = %request.method, request_id = %request.id))]
async fn handle_request(state: Arc<AppState>, request: RpcRequest) -> RpcResponse {
    let request_id = request.id.clone();
    let trace_id = Uuid::new_v4();
    if let Err(error) = state.storage.append_event(
        trace_id,
        "request.received",
        &json!({"method": &request.method, "request_id": &request.id}),
    ) {
        error!(%error, "failed to persist request event");
    }

    let result = dispatch(&state, trace_id, &request).await;
    let response = match result {
        Ok(value) => RpcResponse::success(request_id, value),
        Err(error) => {
            error!(%error, "request failed");
            RpcResponse::failure(request_id, -32000, error.to_string())
        }
    };

    if let Err(error) = state.storage.append_event(
        trace_id,
        "request.completed",
        &json!({
            "method": &request.method,
            "request_id": &request.id,
            "succeeded": response.error.is_none()
        }),
    ) {
        error!(%error, "failed to persist completion event");
    }
    response
}

async fn dispatch(state: &AppState, trace_id: Uuid, request: &RpcRequest) -> anyhow::Result<Value> {
    match request.method.as_str() {
        "daemon.ping" => daemon_ping(state).await,
        "machine.inspect" => machine_inspect(state, trace_id).await,
        "project.inspect" => project_inspect(state, trace_id, &request.params).await,
        "archive.inspect" => archive_inspect(state, trace_id, &request.params).await,
        "winget.resolve" => winget_resolve(state, trace_id, &request.params).await,
        "winget.installed" => winget_installed(state, trace_id, &request.params).await,
        "winget.install.plan" => winget_install_plan(state, trace_id, &request.params).await,
        "winget.install.plan.get" => winget_install_plan_get(state, &request.params),
        "winget.install.approve" => winget_install_approve(state, trace_id, &request.params),
        "winget.install.execute" => winget_install_execute(state, trace_id, &request.params).await,
        "winget.install.cancel" => winget_install_cancel(state, trace_id, &request.params).await,
        "winget.install.lock" => winget_install_lock(state),
        "evidence.list" => {
            let limit = bounded_limit(&request.params, 50);
            Ok(serde_json::to_value(state.storage.list_evidence(limit)?)?)
        }
        "events.replay" => {
            let limit = bounded_limit(&request.params, 100);
            Ok(serde_json::to_value(state.storage.replay_events(limit)?)?)
        }
        "capabilities.list" => Ok(capabilities()),
        _ => Err(anyhow!("unknown daemon method: {}", request.method)),
    }
}

async fn daemon_ping(state: &AppState) -> anyhow::Result<Value> {
    let system_status = adapter_health(&state.system_adapter_path).await;
    let winget_status = adapter_health(&state.winget_adapter_path).await;
    let report = HealthReport {
        service: "toolos-daemon".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        status: "HEALTHY".to_owned(),
        started_at: state.started_at,
        checked_at: Utc::now(),
        database_path: state.storage.path().to_string_lossy().into_owned(),
        adapter_status: format!("system={system_status}; winget={winget_status}"),
    };
    Ok(serde_json::to_value(report)?)
}

async fn machine_inspect(state: &AppState, trace_id: Uuid) -> anyhow::Result<Value> {
    let payload = invoke_adapter(
        &state.system_adapter_path,
        "machine.inspect",
        json!({}),
        Duration::from_secs(10),
    )
    .await?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::MachineInventory,
        "local-machine",
        "Host metadata and PATH-visible tool candidates were inspected",
        "toolos.adapter.system",
        payload.clone(),
        vec![
            "PATH presence does not prove version, authentication, compatibility, or health"
                .to_owned(),
            "No detected executable was launched".to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "MACHINE_INVENTORY")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn project_inspect(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let params: ProjectInspectParams = serde_json::from_value(params.clone())
        .context("project.inspect requires {\"path\": \"...\"}")?;
    let payload = invoke_adapter(
        &state.system_adapter_path,
        "project.inspect",
        json!({"path": params.path}),
        Duration::from_secs(10),
    )
    .await?;
    let scope = payload
        .get("canonical_path")
        .and_then(Value::as_str)
        .unwrap_or("selected-project")
        .to_owned();
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::ProjectIdentity,
        scope,
        "Selected project identity and top-level markers were inspected",
        "toolos.adapter.system",
        payload.clone(),
        vec![
            "Marker files indicate probable structure, not build health".to_owned(),
            "Repository scripts and package lifecycle hooks were not executed".to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "PROJECT_IDENTITY")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn archive_inspect(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let path = required_path(params, "archive.inspect")?;
    let payload = invoke_adapter(
        &state.system_adapter_path,
        "archive.inspect",
        json!({"path": path}),
        Duration::from_secs(10),
    )
    .await?;
    let scope = payload
        .get("canonical_path")
        .and_then(Value::as_str)
        .unwrap_or("selected-archive")
        .to_owned();
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        scope,
        "Selected ZIP structure and extraction paths were inspected without extraction",
        "toolos.adapter.system",
        payload.clone(),
        vec![
            "The archive was not extracted and no entry contents were executed".to_owned(),
            "Structural acceptance is not a malware, secret, license, or content trust verdict"
                .to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "ARCHIVE_INSPECTION")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn winget_resolve(state: &AppState, trace_id: Uuid, params: &Value) -> anyhow::Result<Value> {
    let payload = invoke_adapter(
        &state.winget_adapter_path,
        "winget.resolve",
        params.clone(),
        Duration::from_secs(55),
    )
    .await?;
    let evidence = winget_evidence(
        trace_id,
        &payload,
        "WinGet exact package resolution",
        vec![
            "Provider output is retained without locale-dependent table parsing".to_owned(),
            "Install and uninstall commands remain disabled previews".to_owned(),
            "Package and source agreements were not accepted automatically".to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_PACKAGE_RESOLUTION")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn winget_installed(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let payload = invoke_adapter(
        &state.winget_adapter_path,
        "winget.installed",
        params.clone(),
        Duration::from_secs(55),
    )
    .await?;
    let evidence = winget_evidence(
        trace_id,
        &payload,
        "WinGet exact installed-state query",
        vec![
            "The query is read-only and does not install, update, repair, or uninstall software"
                .to_owned(),
            "ToolOS preserves localized provider output and does not infer a definitive match from table text"
                .to_owned(),
            "A successful query exit proves completion, not a parsed installed-package verdict"
                .to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALLED_STATE")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn winget_install_plan(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let resolution_result = winget_resolve(state, trace_id, params).await?;
    let installed_result = winget_installed(state, trace_id, params).await?;
    let resolution: WingetResolutionReport = serde_json::from_value(
        resolution_result
            .get("snapshot")
            .cloned()
            .context("winget resolution result had no snapshot")?,
    )?;
    let installed_state: WingetInstalledStateReport = serde_json::from_value(
        installed_result
            .get("snapshot")
            .cloned()
            .context("winget installed-state result had no snapshot")?,
    )?;
    let plan = build_install_plan(resolution, installed_state, Utc::now(), 600)
        .map_err(anyhow::Error::msg)?;
    let approval_phrase = plan
        .approval_challenge
        .as_ref()
        .map(|challenge| challenge.required_phrase.clone())
        .unwrap_or_default();
    let stored = StoredActionPlan {
        id: plan.plan_id,
        capability: "package.install.plan.winget".to_owned(),
        resource_key: plan.lock_key.clone(),
        status: install_plan_status(&plan.status).to_owned(),
        plan_hash: plan.plan_hash.clone(),
        created_at: plan.created_at,
        expires_at: plan.expires_at,
        approval_phrase,
        record_json: serde_json::to_string(&plan)?,
    };
    state.storage.store_action_plan(&stored)?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-plan:{}", plan.plan_id),
        format!(
            "Governed WinGet install plan created with status {}",
            install_plan_status(&plan.status)
        ),
        "toolos.daemon.governance",
        serde_json::to_value(&plan)?,
        plan.limitations.clone(),
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALL_PLAN")?;
    state.storage.append_event(
        trace_id,
        "winget.install.plan.created",
        &json!({
            "plan_id": plan.plan_id,
            "plan_hash": plan.plan_hash,
            "status": install_plan_status(&plan.status),
            "expires_at": plan.expires_at
        }),
    )?;
    Ok(json!({"plan": plan, "evidence": evidence}))
}

fn winget_install_plan_get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.plan.get")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    let mut plan: WingetInstallPlan = serde_json::from_str(&stored.record_json)?;
    if plan.status == InstallPlanStatus::AwaitingApproval && Utc::now() >= plan.expires_at {
        plan.status = InstallPlanStatus::Expired;
        plan.approval_allowed = false;
        plan.approval_challenge = None;
        plan.single_safest_next_action =
            "This plan expired. Create a new plan from fresh identity and installed-state evidence."
                .to_owned();
    }
    Ok(serde_json::to_value(plan)?)
}

fn winget_install_approve(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.approve")?;
    let expected_hash = required_string(params, "plan_hash", "winget.install.approve")?;
    let confirmation = required_string(params, "confirmation", "winget.install.approve")?;
    let stored = state
        .storage
        .get_action_plan(plan_id)?
        .with_context(|| format!("install plan not found: {plan_id}"))?;
    let plan: WingetInstallPlan = serde_json::from_str(&stored.record_json)?;
    let now = Utc::now();
    let receipt =
        build_approval_receipt(&plan, &confirmation, now, 300).map_err(anyhow::Error::msg)?;
    let mut approved_plan = plan.clone();
    approved_plan.status = InstallPlanStatus::ApprovedAwaitingExecution;
    approved_plan.execution_enabled = true;
    approved_plan.approval_challenge = None;
    approved_plan.single_safest_next_action =
        "The plan is armed for one separate receipt-bound execution phrase. Approval itself did not invoke WinGet."
            .to_owned();
    let updated_plan_json = serde_json::to_string(&approved_plan)?;
    let stored_receipt = StoredApprovalReceipt {
        id: receipt.approval_id,
        plan_id: receipt.plan_id,
        plan_hash: receipt.plan_hash.clone(),
        approved_at: receipt.approved_at,
        expires_at: receipt.expires_at,
        resource_key: receipt.lock_key.clone(),
        record_json: serde_json::to_string(&receipt)?,
    };
    let lock = StoredResourceLock {
        resource_key: receipt.lock_key.clone(),
        holder_plan_id: receipt.plan_id,
        acquired_at: receipt.approved_at,
        expires_at: receipt.lock_expires_at,
    };
    state.storage.approve_action_plan(&ActionPlanApproval {
        plan_id,
        expected_hash: &expected_hash,
        confirmation: &confirmation,
        updated_plan_json: &updated_plan_json,
        receipt: &stored_receipt,
        lock: &lock,
        now,
    })?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-approval:{}", receipt.approval_id),
        "Governed WinGet install plan armed for one separate receipt-bound execution step",
        "toolos.daemon.governance",
        json!({"plan": approved_plan, "receipt": receipt, "lock": lock}),
        vec![
            "Approval is short-lived and bound to one immutable plan hash".to_owned(),
            "The local lock cannot block WinGet processes started outside ToolOS".to_owned(),
            "No installer, agreement acceptance, elevation, or machine mutation occurred"
                .to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &evidence, "WINGET_INSTALL_APPROVAL")?;
    state.storage.append_event(
        trace_id,
        "winget.install.plan.approved",
        &json!({
            "plan_id": plan_id,
            "approval_id": stored_receipt.id,
            "lock_key": lock.resource_key,
            "lock_expires_at": lock.expires_at,
            "execution_enabled": true
        }),
    )?;
    Ok(json!({
        "plan": approved_plan,
        "receipt": receipt,
        "lock": lock,
        "evidence": evidence
    }))
}

async fn winget_install_execute(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
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
    let execution_id = Uuid::new_v4();
    let started_at = Utc::now();
    plan.status = InstallPlanStatus::Executing;
    plan.execution_enabled = false;
    plan.single_safest_next_action =
        "WinGet execution is in progress. Do not start another package-manager operation."
            .to_owned();
    let executing_plan_json = serde_json::to_string(&plan)?;
    state
        .storage
        .begin_action_execution(&ActionExecutionStart {
            plan_id,
            approval_id,
            expected_hash: &plan.plan_hash,
            updated_plan_json: &executing_plan_json,
            now: started_at,
            lock_expires_at: started_at + ChronoDuration::minutes(32),
        })?;
    state.storage.append_event(
        trace_id,
        "winget.install.execution.started",
        &json!({
            "execution_id": execution_id,
            "plan_id": plan_id,
            "approval_id": approval_id,
            "plan_hash": plan.plan_hash,
            "command": command,
            "agreements_accepted": false,
            "elevation_requested": false
        }),
    )?;

    let cancellation = CancellationToken::default();
    {
        let mut active = state.active_install_executions.lock().await;
        if active.insert(plan_id, cancellation.clone()).is_some() {
            return Err(anyhow!("an execution is already active for plan {plan_id}"));
        }
    }
    let contained_result = contained_adapter::invoke(
        &state.winget_adapter_path,
        "winget.install.execute",
        serde_json::to_value(&request)?,
        Duration::from_secs(31 * 60),
        cancellation,
    )
    .await;
    state
        .active_install_executions
        .lock()
        .await
        .remove(&plan_id);
    let (process_evidence, containment) = contained_process_evidence(&command, contained_result)?;

    let post_install_state = if containment.process_tree_terminated() {
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
        containment,
        preflight_resolution,
        preflight_installed_state,
        post_install_state,
    );
    plan.status = match report.status {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified => {
            InstallPlanStatus::ExecutionSucceededUnverified
        }
        WingetExecutionStatus::ProviderFailed | WingetExecutionStatus::TimedOut => {
            InstallPlanStatus::ExecutionFailed
        }
        WingetExecutionStatus::Cancelled => InstallPlanStatus::ExecutionCancelled,
        WingetExecutionStatus::UnknownRequiresRecovery => {
            InstallPlanStatus::UnknownRequiresRecovery
        }
    };
    plan.single_safest_next_action = report.single_safest_next_action.clone();
    let final_status = install_plan_status(&plan.status);
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
            release_lock: report.status != WingetExecutionStatus::UnknownRequiresRecovery,
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
            "post_state_captured": report.post_install_state.is_some()
        }),
    )?;
    Ok(json!({"plan": plan, "report": report, "evidence": evidence}))
}

fn contained_process_evidence(
    command: &CommandPreview,
    outcome: anyhow::Result<contained_adapter::ContainedAdapterOutcome>,
) -> anyhow::Result<(ProcessEvidence, ProcessContainmentEvidence)> {
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let containment = ProcessContainmentEvidence::unavailable(error.to_string());
            return Ok((
                ProcessEvidence {
                    executable: command.executable.clone(),
                    args: command.args.clone(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("contained adapter setup failed: {error}"),
                    timed_out: false,
                    duration_ms: 0,
                },
                containment,
            ));
        }
    };

    let timed_out = outcome.containment.termination_reason == ProcessTerminationReason::TimedOut;
    if let Some(response) = outcome.response {
        if let Some(error) = response.error {
            return Ok((
                ProcessEvidence {
                    executable: command.executable.clone(),
                    args: command.args.clone(),
                    exit_code: None,
                    stdout: outcome.transport_stdout,
                    stderr: format!(
                        "adapter error {}: {}; {}",
                        error.code, error.message, outcome.transport_stderr
                    ),
                    timed_out,
                    duration_ms: outcome.duration_ms,
                },
                outcome.containment,
            ));
        }
        if let Some(payload) = response.result {
            if let Ok(evidence) = serde_json::from_value::<ProcessEvidence>(payload) {
                return Ok((evidence, outcome.containment));
            }
        }
    }

    Ok((
        ProcessEvidence {
            executable: command.executable.clone(),
            args: command.args.clone(),
            exit_code: None,
            stdout: outcome.transport_stdout,
            stderr: format!(
                "contained adapter produced no valid execution response; root_exit_code={:?}; {}",
                outcome.root_exit_code, outcome.transport_stderr
            ),
            timed_out,
            duration_ms: outcome.duration_ms,
        },
        outcome.containment,
    ))
}

async fn winget_install_cancel(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let plan_id = required_uuid(params, "plan_id", "winget.install.cancel")?;
    let token = state
        .active_install_executions
        .lock()
        .await
        .get(&plan_id)
        .cloned();
    let active = token.is_some();
    let cancel_requested = token
        .as_ref()
        .is_some_and(|token| token.cancel(CancellationReason::ExplicitCancellation));
    state.storage.append_event(
        trace_id,
        "winget.install.execution.cancel_requested",
        &json!({
            "plan_id": plan_id,
            "active": active,
            "cancel_requested": cancel_requested
        }),
    )?;
    Ok(json!({
        "plan_id": plan_id,
        "active": active,
        "cancel_requested": cancel_requested
    }))
}

fn validate_execution_authorization(
    plan: &WingetInstallPlan,
    receipt: &WingetInstallApprovalReceipt,
    lock: &StoredResourceLock,
    confirmation: &str,
    stored_hash: &str,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    if plan.status != InstallPlanStatus::ApprovedAwaitingExecution
        || receipt.status != InstallPlanStatus::ApprovedAwaitingExecution
        || !plan.execution_enabled
        || !receipt.execution_enabled
    {
        return Err(anyhow!(
            "install plan and receipt are not armed for a separate execution step"
        ));
    }
    if now >= plan.expires_at || now >= receipt.expires_at {
        return Err(anyhow!("install plan or approval receipt expired"));
    }
    if plan.plan_hash != stored_hash
        || receipt.plan_hash != plan.plan_hash
        || receipt.plan_id != plan.plan_id
    {
        return Err(anyhow!(
            "plan, stored hash, and approval receipt do not match"
        ));
    }
    if confirmation.trim() != receipt.execution_confirmation {
        return Err(anyhow!(
            "execution phrase does not match the approval receipt"
        ));
    }
    if lock.holder_plan_id != plan.plan_id || lock.resource_key != plan.lock_key {
        return Err(anyhow!("WinGet lock is not held by this plan"));
    }
    if plan.selector.scope != Some(PackageScope::User) {
        return Err(anyhow!("execution is restricted to explicit user scope"));
    }
    if plan.selector.version.is_none() || plan.selector.architecture.is_none() {
        return Err(anyhow!(
            "execution requires explicit version and architecture pins; create a new plan"
        ));
    }
    Ok(())
}

fn validate_fresh_preflight(
    plan: &WingetInstallPlan,
    resolution: &WingetResolutionReport,
    installed_state: &WingetInstalledStateReport,
) -> anyhow::Result<()> {
    if resolution.status != toolos_winget::ResolutionStatus::ResolvedExact {
        return Err(anyhow!("fresh exact package resolution failed"));
    }
    if installed_state.status != toolos_winget::InstalledQueryStatus::QueryCompleted {
        return Err(anyhow!("fresh installed-state query failed"));
    }
    if resolution.selector != plan.selector || installed_state.selector != plan.selector {
        return Err(anyhow!(
            "fresh provider selectors differ from the immutable plan"
        ));
    }
    if resolution.install_preview != plan.install_preview {
        return Err(anyhow!(
            "fresh derived install command differs from the immutable plan"
        ));
    }
    if resolution.provider_version != plan.resolution.provider_version {
        return Err(anyhow!(
            "WinGet provider version changed after planning; create a new plan"
        ));
    }
    if installed_state.definitive_installed_match == Some(true) {
        return Err(anyhow!(
            "the exact package is already installed according to definitive evidence"
        ));
    }
    Ok(())
}

fn winget_install_lock(state: &AppState) -> anyhow::Result<Value> {
    let lock = state
        .storage
        .get_resource_lock("package-manager:winget", Utc::now())?;
    Ok(json!({
        "resource_key": "package-manager:winget",
        "active": lock.is_some(),
        "lock": lock
    }))
}

fn winget_evidence(
    trace_id: Uuid,
    payload: &Value,
    claim_prefix: &str,
    limitations: Vec<String>,
) -> anyhow::Result<EvidenceRecord> {
    let package_id = payload
        .pointer("/selector/package_id")
        .and_then(Value::as_str)
        .unwrap_or("selected-package");
    let source = payload
        .pointer("/selector/source")
        .and_then(Value::as_str)
        .unwrap_or("unknown-source");
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget:{source}:{package_id}"),
        format!("{claim_prefix} completed with status {status}"),
        "toolos.adapter.winget",
        payload.clone(),
        limitations,
    )
    .map_err(Into::into)
}

fn capabilities() -> Value {
    json!([
        {
            "capability_id": "machine.inspect",
            "provider_id": "toolos.adapter.system",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "project.inspect",
            "provider_id": "toolos.adapter.system",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "archive.inspect",
            "provider_id": "toolos.adapter.system",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.resolve.winget",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.installed.query.winget",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.install.plan.winget",
            "provider_id": "toolos.daemon.governance",
            "blast_radius": "LOCAL_METADATA_WRITE",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.install.approve.winget",
            "provider_id": "toolos.daemon.governance",
            "blast_radius": "LOCAL_METADATA_WRITE",
            "status": "IMPLEMENTED_ARMS_SEPARATE_EXECUTION"
        },
        {
            "capability_id": "package.install.execute.winget",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",
            "status": "IMPLEMENTED_USER_SCOPE_PINNED"
        },
        {
            "capability_id": "package.install.cancel.winget",
            "provider_id": "toolos.daemon.governance",
            "blast_radius": "SAFETY_CONTROL",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "package.preview.install",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",
            "status": "PREVIEW_ONLY"
        },
        {
            "capability_id": "package.preview.uninstall",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",
            "status": "PREVIEW_ONLY"
        }
    ])
}

async fn adapter_health(path: &Path) -> &'static str {
    match invoke_adapter(path, "adapter.health", json!({}), Duration::from_secs(7)).await {
        Ok(payload) => match payload.get("status").and_then(Value::as_str) {
            Some("HEALTHY") => "HEALTHY",
            Some("DEGRADED") => "DEGRADED",
            Some("UNAVAILABLE") => "UNAVAILABLE",
            _ => "REACHABLE",
        },
        Err(_) => "UNREACHABLE",
    }
}

fn record_evidence(
    state: &AppState,
    trace_id: Uuid,
    evidence: &EvidenceRecord,
    kind: &str,
) -> anyhow::Result<()> {
    state.storage.record_evidence(evidence)?;
    state.storage.append_event(
        trace_id,
        "evidence.recorded",
        &json!({"evidence_id": evidence.id, "kind": kind}),
    )?;
    Ok(())
}

fn required_path(params: &Value, method: &str) -> anyhow::Result<String> {
    params
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("{method} requires a non-empty string 'path' parameter"))
}

fn required_string(params: &Value, key: &str, method: &str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("{method} requires a non-empty string '{key}' parameter"))
}

fn required_uuid(params: &Value, key: &str, method: &str) -> anyhow::Result<Uuid> {
    let value = required_string(params, key, method)?;
    Uuid::parse_str(&value).with_context(|| format!("{method} requires a UUID '{key}' parameter"))
}

fn install_plan_status(status: &InstallPlanStatus) -> &'static str {
    match status {
        InstallPlanStatus::AwaitingApproval => "AWAITING_APPROVAL",
        InstallPlanStatus::Blocked => "BLOCKED",
        InstallPlanStatus::ApprovedExecutionDisabled => "APPROVED_EXECUTION_DISABLED",
        InstallPlanStatus::ApprovedAwaitingExecution => "APPROVED_AWAITING_EXECUTION",
        InstallPlanStatus::Executing => "EXECUTING",
        InstallPlanStatus::ExecutionSucceededUnverified => "EXECUTION_SUCCEEDED_UNVERIFIED",
        InstallPlanStatus::ExecutionFailed => "EXECUTION_FAILED",
        InstallPlanStatus::ExecutionCancelled => "EXECUTION_CANCELLED",
        InstallPlanStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
        InstallPlanStatus::Expired => "EXPIRED",
    }
}

fn execution_status(status: &WingetExecutionStatus) -> &'static str {
    match status {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified => {
            "PROVIDER_SUCCEEDED_POST_STATE_UNVERIFIED"
        }
        WingetExecutionStatus::ProviderFailed => "PROVIDER_FAILED",
        WingetExecutionStatus::TimedOut => "TIMED_OUT",
        WingetExecutionStatus::Cancelled => "CANCELLED",
        WingetExecutionStatus::UnknownRequiresRecovery => "UNKNOWN_REQUIRES_RECOVERY",
    }
}

fn bounded_limit(params: &Value, default: usize) -> usize {
    params
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
        .clamp(1, 500)
}

async fn invoke_adapter(
    path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
) -> anyhow::Result<Value> {
    let mut child = Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start adapter at {}", path.display()))?;

    let mut stdin = child.stdin.take().context("adapter stdin unavailable")?;
    let stdout = child.stdout.take().context("adapter stdout unavailable")?;
    let request = RpcRequest::new(method, params);
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    stdin.write_all(&encoded).await?;
    stdin.shutdown().await?;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let read = tokio::time::timeout(timeout, reader.read_line(&mut line))
        .await
        .with_context(|| format!("adapter method {method} timed out"))??;
    if read == 0 {
        let output = child.wait_with_output().await?;
        return Err(anyhow!(
            "adapter returned no response: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let response: RpcResponse = serde_json::from_str(line.trim_end())?;
    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!("adapter exited with status {status}"));
    }
    if let Some(error) = response.error {
        return Err(anyhow!("adapter error {}: {}", error.code, error.message));
    }
    response.result.context("adapter response had no result")
}

fn database_path() -> anyhow::Result<PathBuf> {
    if let Some(directory) = std::env::var_os("TOOLOS_DATA_DIR") {
        return Ok(PathBuf::from(directory).join("toolos.db"));
    }
    if cfg!(windows) {
        let directory = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .context("LOCALAPPDATA is unavailable; set TOOLOS_DATA_DIR")?;
        Ok(directory.join("ToolOS").join("toolos.db"))
    } else {
        let directory = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .unwrap_or_else(std::env::temp_dir);
        Ok(directory.join("toolos").join("toolos.db"))
    }
}

fn sibling_adapter_path(
    environment_variable: &str,
    windows_name: &str,
    unix_name: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os(environment_variable) {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe().context("resolve daemon executable path")?;
    let file_name = if cfg!(windows) {
        windows_name
    } else {
        unix_name
    };
    Ok(executable
        .parent()
        .context("daemon executable has no parent directory")?
        .join(file_name))
}

fn initialize_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("toolos=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_is_bounded() {
        assert_eq!(bounded_limit(&json!({}), 50), 50);
        assert_eq!(bounded_limit(&json!({"limit": 0}), 50), 1);
        assert_eq!(bounded_limit(&json!({"limit": 900}), 50), 500);
    }

    #[test]
    fn path_parameter_must_be_non_empty() {
        assert!(required_path(&json!({"path": "  "}), "archive.inspect").is_err());
        assert_eq!(
            required_path(&json!({"path": "fixture.zip"}), "archive.inspect").expect("path"),
            "fixture.zip"
        );
    }

    #[test]
    fn capabilities_include_installed_state_query() {
        let values = capabilities()
            .as_array()
            .expect("capabilities array")
            .clone();
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.installed.query.winget")
        }));
    }

    #[test]
    fn capabilities_include_governed_install_plan_and_pinned_user_execution() {
        let values = capabilities()
            .as_array()
            .expect("capabilities array")
            .clone();
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.plan.winget")
                && value.get("status").and_then(Value::as_str) == Some("IMPLEMENTED")
        }));
        assert!(values.iter().any(|value| {
            value.get("capability_id").and_then(Value::as_str)
                == Some("package.install.execute.winget")
                && value.get("status").and_then(Value::as_str)
                    == Some("IMPLEMENTED_USER_SCOPE_PINNED")
        }));
    }

    #[test]
    fn required_uuid_rejects_invalid_values() {
        assert!(required_uuid(
            &json!({"plan_id": "not-a-uuid"}),
            "plan_id",
            "winget.install.plan.get"
        )
        .is_err());
    }

    #[test]
    fn explicit_data_directory_wins() {
        let directory = std::env::temp_dir().join("toolos-explicit-data-test");
        std::env::set_var("TOOLOS_DATA_DIR", &directory);
        let path = database_path().expect("database path");
        std::env::remove_var("TOOLOS_DATA_DIR");
        assert_eq!(path, directory.join("toolos.db"));
    }
}
