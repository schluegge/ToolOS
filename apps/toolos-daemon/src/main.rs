use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use toolos_domain::{
    EvidenceKind, EvidenceRecord, HealthReport, ProjectInspectParams, RpcRequest, RpcResponse,
};
use toolos_storage::Storage;
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
        "daemon.ping" => daemon_ping(state).await,
        "machine.inspect" => machine_inspect(state, trace_id).await,
        "project.inspect" => project_inspect(state, trace_id, &request.params).await,
        "archive.inspect" => archive_inspect(state, trace_id, &request.params).await,
        "winget.resolve" => winget_resolve(state, trace_id, &request.params).await,
        "winget.installed" => winget_installed(state, trace_id, &request.params).await,
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
    fn explicit_data_directory_wins() {
        let directory = std::env::temp_dir().join("toolos-explicit-data-test");
        std::env::set_var("TOOLOS_DATA_DIR", &directory);
        let path = database_path().expect("database path");
        std::env::remove_var("TOOLOS_DATA_DIR");
        assert_eq!(path, directory.join("toolos.db"));
    }
}
