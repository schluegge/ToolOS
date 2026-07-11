use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use toolos_actions::{
    approve_plan, create_install_plan, create_uninstall_plan, reject_plan, ActionStatus,
    ApprovalRequest, WingetActionPlan,
};
use toolos_domain::{
    EvidenceKind, EvidenceRecord, HealthReport, ProjectInspectParams, RpcRequest, RpcResponse,
};
use toolos_storage::Storage;
use toolos_winget::WingetResolutionReport;
use tracing::{error, info, instrument};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    storage: Storage,
    started_at: DateTime<Utc>,
    system_adapter_path: PathBuf,
    winget_adapter_path: PathBuf,
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
        "daemon.ping" => {
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
        "machine.inspect" => inspect_machine(state, trace_id).await,
        "project.inspect" => inspect_project(state, trace_id, request).await,
        "archive.inspect" => inspect_archive(state, trace_id, request).await,
        "winget.resolve" => resolve_winget(state, trace_id, request.params.clone()).await,
        "winget.plan.install" => {
            create_winget_plan(state, trace_id, request.params.clone(), true).await
        }
        "winget.plan.uninstall" => {
            create_winget_plan(state, trace_id, request.params.clone(), false).await
        }
        "actions.list" => {
            let limit = bounded_limit(&request.params, 50);
            Ok(serde_json::to_value(
                state.storage.list_action_plans(limit)?,
            )?)
        }
        "actions.get" => {
            let id = required_uuid(&request.params, "plan_id", "actions.get")?;
            let plan = state
                .storage
                .get_action_plan(id)?
                .with_context(|| format!("action plan {id} was not found"))?;
            Ok(serde_json::to_value(plan)?)
        }
        "actions.approve" => approve_action(state, trace_id, request.params.clone()),
        "actions.reject" => reject_action(state, trace_id, request.params.clone()),
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

async fn inspect_machine(state: &AppState, trace_id: Uuid) -> anyhow::Result<Value> {
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

async fn inspect_project(
    state: &AppState,
    trace_id: Uuid,
    request: &RpcRequest,
) -> anyhow::Result<Value> {
    let params: ProjectInspectParams = serde_json::from_value(request.params.clone())
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

async fn inspect_archive(
    state: &AppState,
    trace_id: Uuid,
    request: &RpcRequest,
) -> anyhow::Result<Value> {
    let path = required_path(&request.params, "archive.inspect")?;
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

async fn resolve_winget(state: &AppState, trace_id: Uuid, params: Value) -> anyhow::Result<Value> {
    let payload = invoke_adapter(
        &state.winget_adapter_path,
        "winget.resolve",
        params,
        Duration::from_secs(55),
    )
    .await?;
    let evidence = winget_resolution_evidence(trace_id, &payload)?;
    record_evidence(state, trace_id, &evidence, "WINGET_PACKAGE_RESOLUTION")?;
    Ok(json!({"snapshot": payload, "evidence": evidence}))
}

async fn create_winget_plan(
    state: &AppState,
    trace_id: Uuid,
    params: Value,
    install: bool,
) -> anyhow::Result<Value> {
    let payload = invoke_adapter(
        &state.winget_adapter_path,
        "winget.resolve",
        params,
        Duration::from_secs(55),
    )
    .await?;
    let resolution: WingetResolutionReport = serde_json::from_value(payload.clone())
        .context("WinGet adapter returned an invalid resolution report")?;
    let resolution_evidence = winget_resolution_evidence(trace_id, &payload)?;
    record_evidence(
        state,
        trace_id,
        &resolution_evidence,
        "WINGET_PACKAGE_RESOLUTION",
    )?;

    let plan = if install {
        create_install_plan(trace_id, resolution, Utc::now())
    } else {
        create_uninstall_plan(trace_id, resolution, Utc::now())
    }
    .map_err(anyhow::Error::msg)?;
    state.storage.create_action_plan(&plan)?;
    state.storage.append_event(
        trace_id,
        "action.plan.created",
        &json!({
            "plan_id": plan.id,
            "kind": plan.kind,
            "status": plan.status,
            "command_sha256": plan.command_sha256,
            "expires_at": plan.expires_at,
            "execution_available": plan.execution_available
        }),
    )?;
    let plan_payload = serde_json::to_value(&plan)?;
    let plan_evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("action-plan:{}", plan.id),
        "A time-limited WinGet action plan was created; execution remains blocked",
        "toolos.action-governance",
        plan_payload,
        vec![
            "Approval does not execute the command".to_owned(),
            "Process-tree cancellation and tested rollback are still missing".to_owned(),
            "The plan expires after fifteen minutes and cannot be reused after state transition"
                .to_owned(),
        ],
    )?;
    record_evidence(state, trace_id, &plan_evidence, "ACTION_PLAN_CREATED")?;
    Ok(json!({
        "plan": plan,
        "resolution_evidence": resolution_evidence,
        "plan_evidence": plan_evidence
    }))
}

fn approve_action(state: &AppState, trace_id: Uuid, params: Value) -> anyhow::Result<Value> {
    let request: ApprovalRequest = serde_json::from_value(params)
        .context("actions.approve requires plan_id, confirmation_phrase, and acknowledgements")?;
    let plan = state
        .storage
        .get_action_plan(request.plan_id)?
        .with_context(|| format!("action plan {} was not found", request.plan_id))?;

    if Utc::now() >= plan.expires_at && plan.status == ActionStatus::WaitingApproval {
        let mut expired = plan;
        expired.status = ActionStatus::Expired;
        state
            .storage
            .replace_action_plan(ActionStatus::WaitingApproval, &expired)?;
        state.storage.append_event(
            trace_id,
            "action.plan.expired",
            &json!({"plan_id": expired.id}),
        )?;
        return Err(anyhow!(
            "action plan {} expired; create a fresh plan",
            expired.id
        ));
    }

    let approved = approve_plan(plan, &request, Utc::now()).map_err(anyhow::Error::msg)?;
    state
        .storage
        .replace_action_plan(ActionStatus::WaitingApproval, &approved)?;
    state.storage.append_event(
        trace_id,
        "action.plan.approved",
        &json!({
            "plan_id": approved.id,
            "status": approved.status,
            "execution_available": approved.execution_available,
            "command_sha256": approved.command_sha256
        }),
    )?;
    Ok(json!({
        "plan": approved,
        "outcome": "APPROVED_NOT_EXECUTED",
        "single_safest_next_action": "Implement and prove Windows process-tree cancellation plus pre-install residual-state capture before enabling execution."
    }))
}

fn reject_action(state: &AppState, trace_id: Uuid, params: Value) -> anyhow::Result<Value> {
    let id = required_uuid(&params, "plan_id", "actions.reject")?;
    let plan = state
        .storage
        .get_action_plan(id)?
        .with_context(|| format!("action plan {id} was not found"))?;
    let rejected = reject_plan(plan, Utc::now()).map_err(anyhow::Error::msg)?;
    state
        .storage
        .replace_action_plan(ActionStatus::WaitingApproval, &rejected)?;
    state.storage.append_event(
        trace_id,
        "action.plan.rejected",
        &json!({"plan_id": rejected.id, "status": rejected.status}),
    )?;
    Ok(serde_json::to_value(rejected)?)
}

fn winget_resolution_evidence(trace_id: Uuid, payload: &Value) -> anyhow::Result<EvidenceRecord> {
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
    Ok(EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget:{source}:{package_id}"),
        format!("WinGet exact package resolution completed with status {status}"),
        "toolos.adapter.winget",
        payload.clone(),
        vec![
            "Provider output is retained without locale-dependent table parsing".to_owned(),
            "Install and uninstall commands remain disabled previews".to_owned(),
            "Package and source agreements were not accepted automatically".to_owned(),
        ],
    )?)
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

fn required_uuid(params: &Value, field: &str, method: &str) -> anyhow::Result<Uuid> {
    let value = params
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("{method} requires a string '{field}' parameter"))?;
    Uuid::parse_str(value).with_context(|| format!("{field} is not a valid UUID"))
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
            "capability_id": "action.plan.winget",
            "provider_id": "toolos.action-governance",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED"
        },
        {
            "capability_id": "action.approve.winget",
            "provider_id": "toolos.action-governance",
            "blast_radius": "READ_ONLY",
            "status": "IMPLEMENTED_NO_EXECUTION"
        },
        {
            "capability_id": "package.execute.install",
            "provider_id": "toolos.adapter.winget",
            "blast_radius": "USER_PROFILE_WRITE_OR_MACHINE_WRITE",
            "status": "BLOCKED_MISSING_CANCELLATION_AND_ROLLBACK"
        }
    ])
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
    fn uuid_parameter_is_validated() {
        assert!(required_uuid(&json!({"plan_id": "bad"}), "plan_id", "actions.get").is_err());
        let id = Uuid::new_v4();
        assert_eq!(
            required_uuid(
                &json!({"plan_id": id.to_string()}),
                "plan_id",
                "actions.get"
            )
            .expect("uuid"),
            id
        );
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
