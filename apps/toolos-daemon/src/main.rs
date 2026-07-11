use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

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
    adapter_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_tracing();
    let database_path = database_path()?;
    let storage = Storage::initialize(&database_path).context("initialize ToolOS database")?;
    let adapter_path = adapter_path()?;
    let started_at = Utc::now();
    storage.set_metadata("daemon.started_at", &started_at.to_rfc3339())?;
    let state = Arc::new(AppState {
        storage,
        started_at,
        adapter_path,
    });

    let startup_trace = Uuid::new_v4();
    state.storage.append_event(
        startup_trace,
        "daemon.started",
        &json!({
            "version": env!("CARGO_PKG_VERSION"),
            "database_path": state.storage.path().to_string_lossy(),
            "adapter_path": state.adapter_path.to_string_lossy()
        }),
    )?;

    info!(
        database = %state.storage.path().display(),
        adapter = %state.adapter_path.display(),
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
            let adapter_status =
                match invoke_adapter(&state.adapter_path, "adapter.health", json!({})).await {
                    Ok(_) => "HEALTHY",
                    Err(_) => "UNREACHABLE",
                };
            let report = HealthReport {
                service: "toolos-daemon".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                status: "HEALTHY".to_owned(),
                started_at: state.started_at,
                checked_at: Utc::now(),
                database_path: state.storage.path().to_string_lossy().into_owned(),
                adapter_status: adapter_status.to_owned(),
            };
            Ok(serde_json::to_value(report)?)
        }
        "machine.inspect" => {
            let payload = invoke_adapter(&state.adapter_path, "machine.inspect", json!({})).await?;
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
            state.storage.record_evidence(&evidence)?;
            state.storage.append_event(
                trace_id,
                "evidence.recorded",
                &json!({"evidence_id": evidence.id, "kind": "MACHINE_INVENTORY"}),
            )?;
            Ok(json!({"snapshot": payload, "evidence": evidence}))
        }
        "project.inspect" => {
            let params: ProjectInspectParams = serde_json::from_value(request.params.clone())
                .context("project.inspect requires {\"path\": \"...\"}")?;
            let payload = invoke_adapter(
                &state.adapter_path,
                "project.inspect",
                json!({"path": params.path}),
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
            state.storage.record_evidence(&evidence)?;
            state.storage.append_event(
                trace_id,
                "evidence.recorded",
                &json!({"evidence_id": evidence.id, "kind": "PROJECT_IDENTITY"}),
            )?;
            Ok(json!({"snapshot": payload, "evidence": evidence}))
        }
        "evidence.list" => {
            let limit = bounded_limit(&request.params, 50);
            Ok(serde_json::to_value(state.storage.list_evidence(limit)?)?)
        }
        "events.replay" => {
            let limit = bounded_limit(&request.params, 100);
            Ok(serde_json::to_value(state.storage.replay_events(limit)?)?)
        }
        "capabilities.list" => Ok(json!([
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
            }
        ])),
        _ => Err(anyhow!("unknown daemon method: {}", request.method)),
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

async fn invoke_adapter(path: &Path, method: &str, params: Value) -> anyhow::Result<Value> {
    let mut child = Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start system adapter at {}", path.display()))?;

    let mut stdin = child.stdin.take().context("adapter stdin unavailable")?;
    let stdout = child.stdout.take().context("adapter stdout unavailable")?;
    let request = RpcRequest::new(method, params);
    let mut encoded = serde_json::to_vec(&request)?;
    encoded.push(b'\n');
    stdin.write_all(&encoded).await?;
    stdin.shutdown().await?;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut line),
    )
    .await
    .context("system adapter timed out")??;
    if read == 0 {
        let output = child.wait_with_output().await?;
        return Err(anyhow!(
            "system adapter returned no response: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let response: RpcResponse = serde_json::from_str(line.trim_end())?;
    let status = child.wait().await?;
    if !status.success() {
        return Err(anyhow!("system adapter exited with status {status}"));
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

fn adapter_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("TOOLOS_SYSTEM_ADAPTER") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe().context("resolve daemon executable path")?;
    let file_name = if cfg!(windows) {
        "toolos-system-adapter.exe"
    } else {
        "toolos-system-adapter"
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
    fn explicit_data_directory_wins() {
        let directory = std::env::temp_dir().join("toolos-explicit-data-test");
        std::env::set_var("TOOLOS_DATA_DIR", &directory);
        let path = database_path().expect("database path");
        std::env::remove_var("TOOLOS_DATA_DIR");
        assert_eq!(path, directory.join("toolos.db"));
    }
}
