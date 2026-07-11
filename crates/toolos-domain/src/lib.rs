use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const JSON_RPC_VERSION: &str = "2.0";
pub const ADAPTER_PROTOCOL_VERSION: &str = "toolos-jsonrpc-1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentKind {
    WindowsNative,
    PowerShell,
    Cmd,
    GitBash,
    Wsl,
    Container,
    RemoteApi,
    UnixNative,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlastRadius {
    ReadOnly,
    ProjectWrite,
    UserProfileWrite,
    MachineWrite,
    NetworkWrite,
    Destructive,
    AccountOrPayment,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceKind {
    Healthcheck,
    MachineInventory,
    ProjectIdentity,
    AdapterInvocation,
    EventReplay,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityStateDimension {
    Discovered,
    Installed,
    Configured,
    Reachable,
    Authenticated,
    Permissioned,
    Compatible,
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl RpcRequest {
    #[must_use]
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id: Uuid::new_v4().to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    #[must_use]
    pub fn success(id: impl Into<String>, result: Value) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn failure(id: impl Into<String>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSON_RPC_VERSION.to_owned(),
            id: id.into(),
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ToolObservation {
    pub tool_id: String,
    pub display_name: String,
    pub executable: String,
    pub discovered: bool,
    pub resolved_path: Option<String>,
    pub execution_environment: EnvironmentKind,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct MachineSnapshot {
    pub os: String,
    pub architecture: String,
    pub hostname_token: Option<String>,
    pub environments: Vec<EnvironmentKind>,
    pub tools: Vec<ToolObservation>,
    pub privacy_mode: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub requested_path: String,
    pub canonical_path: Option<String>,
    pub exists: bool,
    pub is_directory: bool,
    pub repository_root: Option<String>,
    pub markers: Vec<String>,
    pub detected_stacks: Vec<String>,
    pub instruction_files: Vec<String>,
    pub limitations: Vec<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HealthReport {
    pub service: String,
    pub version: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub checked_at: DateTime<Utc>,
    pub database_path: String,
    pub adapter_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EvidenceRecord {
    pub id: Uuid,
    pub trace_id: Uuid,
    pub kind: EvidenceKind,
    pub scope: String,
    pub claim: String,
    pub provider: String,
    pub observed_at: DateTime<Utc>,
    pub payload: Value,
    pub limitations: Vec<String>,
    pub content_sha256: String,
}

impl EvidenceRecord {
    pub fn new(
        trace_id: Uuid,
        kind: EvidenceKind,
        scope: impl Into<String>,
        claim: impl Into<String>,
        provider: impl Into<String>,
        payload: Value,
        limitations: Vec<String>,
    ) -> Result<Self, serde_json::Error> {
        let canonical_payload = serde_json::to_vec(&payload)?;
        let content_sha256 = format!("{:x}", Sha256::digest(canonical_payload));
        Ok(Self {
            id: Uuid::new_v4(),
            trace_id,
            kind,
            scope: scope.into(),
            claim: claim.into(),
            provider: provider.into(),
            observed_at: Utc::now(),
            payload,
            limitations,
            content_sha256,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdapterEntrypoint {
    pub environment: EnvironmentKind,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct AdapterPermissions {
    pub filesystem: Vec<String>,
    pub network: Vec<String>,
    pub credentials: Vec<String>,
    pub process_spawn: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdapterHealthcheck {
    pub method: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdapterProvenance {
    pub canonical_source: String,
    pub maintainer: String,
    pub license: String,
    pub verified_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct AdapterManifest {
    pub schema_version: u32,
    pub adapter_id: String,
    pub adapter_version: String,
    pub entrypoint: AdapterEntrypoint,
    pub protocol: String,
    pub permissions: AdapterPermissions,
    pub capabilities: Vec<String>,
    pub healthcheck: AdapterHealthcheck,
    pub provenance: AdapterProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct EventRecord {
    pub sequence: i64,
    pub trace_id: Uuid,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProjectInspectParams {
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_uses_json_rpc_version() {
        let request = RpcRequest::new("daemon.ping", json!({}));
        assert_eq!(request.jsonrpc, JSON_RPC_VERSION);
        assert!(!request.id.is_empty());
    }

    #[test]
    fn evidence_hash_changes_with_payload() {
        let trace_id = Uuid::new_v4();
        let first = EvidenceRecord::new(
            trace_id,
            EvidenceKind::Healthcheck,
            "daemon",
            "healthy",
            "test",
            json!({"value": 1}),
            vec![],
        )
        .expect("first evidence");
        let second = EvidenceRecord::new(
            trace_id,
            EvidenceKind::Healthcheck,
            "daemon",
            "healthy",
            "test",
            json!({"value": 2}),
            vec![],
        )
        .expect("second evidence");
        assert_ne!(first.content_sha256, second.content_sha256);
        assert_eq!(first.content_sha256.len(), 64);
    }
}
