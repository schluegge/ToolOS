use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context};
use serde_json::Value;
use toolos_domain::{RpcRequest, RpcResponse, JSON_RPC_VERSION};
use toolos_process::{
    spawn_contained, ContainedCommandSpec, ContainedProcess, ContainedProcessControl,
    ProcessContainmentEvidence, ProcessStopReason,
};
use toolos_winget::{CommandPreview, ProcessEvidence};
use uuid::Uuid;

pub struct ContainedAdapter {
    process: ContainedProcess,
}

impl ContainedAdapter {
    #[must_use]
    pub fn execution_id(&self) -> Uuid {
        self.process.execution_id()
    }

    #[must_use]
    pub fn root_pid(&self) -> u32 {
        self.process.root_pid()
    }

    #[must_use]
    pub fn control(&self) -> ContainedProcessControl {
        self.process.control()
    }

    pub async fn wait(
        self,
        expected_command: &CommandPreview,
    ) -> Result<ContainedAdapterOutcome, anyhow::Error> {
        let output = self.process.wait().await?;
        let containment = output.containment;
        let adapter_stderr = output.stderr;
        let provider = parse_provider_response(&output.stdout)
            .and_then(|value| serde_json::from_value::<ProcessEvidence>(value).map_err(Into::into));

        let process_evidence = match provider {
            Ok(mut evidence) => {
                if !adapter_stderr.trim().is_empty() {
                    if !evidence.stderr.is_empty() {
                        evidence.stderr.push('\n');
                    }
                    evidence.stderr.push_str("[ToolOS adapter stderr]\n");
                    evidence.stderr.push_str(&adapter_stderr);
                }
                evidence
            }
            Err(error) => ProcessEvidence {
                executable: expected_command.executable.clone(),
                args: expected_command.args.clone(),
                exit_code: None,
                stdout: String::new(),
                stderr: format!(
                    "ToolOS did not receive a valid final adapter response: {error}\n{adapter_stderr}"
                ),
                timed_out: containment.stop_reason == ProcessStopReason::TimedOut,
                duration_ms: output.duration_ms,
            },
        };

        Ok(ContainedAdapterOutcome {
            process_evidence,
            containment,
        })
    }
}

pub struct ContainedAdapterOutcome {
    pub process_evidence: ProcessEvidence,
    pub containment: ProcessContainmentEvidence,
}

pub fn spawn_mutating_adapter(
    execution_id: Uuid,
    path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
) -> anyhow::Result<ContainedAdapter> {
    let request = RpcRequest {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        id: execution_id.to_string(),
        method: method.to_owned(),
        params,
    };
    let mut stdin = serde_json::to_vec(&request)?;
    stdin.push(b'\n');

    let mut spec = ContainedCommandSpec::new(path.as_os_str());
    spec.stdin = stdin;
    spec.timeout = timeout;
    spec.max_stdout_bytes = 128 * 1024;
    spec.max_stderr_bytes = 128 * 1024;

    let process = spawn_contained(spec)
        .with_context(|| format!("start contained adapter at {}", path.display()))?;
    if process.execution_id() == Uuid::nil() {
        return Err(anyhow!("contained adapter returned an invalid execution ID"));
    }
    Ok(ContainedAdapter { process })
}

pub fn containment_failure(error: &anyhow::Error) -> ProcessContainmentEvidence {
    ProcessContainmentEvidence {
        method: "WINDOWS_JOB_OBJECT_STARTUP_ATTRIBUTE".to_owned(),
        root_pid: None,
        stop_reason: ProcessStopReason::ContainmentFailed,
        active_processes_after_cleanup: None,
        descendants_terminated: None,
        containment_confirmed: false,
        detail: error.to_string(),
    }
}

fn parse_provider_response(stdout: &str) -> anyhow::Result<Value> {
    let line = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("contained adapter returned no JSON-RPC response")?;
    let response: RpcResponse = serde_json::from_str(line.trim())?;
    if let Some(error) = response.error {
        return Err(anyhow!("adapter error {}: {}", error.code, error.message));
    }
    response.result.context("adapter response had no result")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_one_json_rpc_result_line() {
        let response = RpcResponse::success("id", json!({"exit_code": 0}));
        let encoded = serde_json::to_string(&response).expect("encode response");
        assert_eq!(
            parse_provider_response(&format!("{encoded}\n"))
                .expect("parse response")
                .get("exit_code")
                .and_then(Value::as_i64),
            Some(0)
        );
    }

    #[test]
    fn containment_failure_never_claims_termination() {
        let evidence = containment_failure(&anyhow!("spawn failed"));
        assert!(!evidence.containment_confirmed);
        assert_eq!(evidence.active_processes_after_cleanup, None);
        assert_eq!(evidence.stop_reason, ProcessStopReason::ContainmentFailed);
    }
}
