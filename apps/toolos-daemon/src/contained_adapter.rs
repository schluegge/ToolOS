use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde_json::Value;
use toolos_domain::{RpcRequest, RpcResponse};
use toolos_windows_job::{
    CancellationToken, ContainedChild, ContainmentReport, ProcessSpec, TerminationReason,
};
use toolos_winget::{ProcessContainmentEvidence, ProcessTerminationReason};

pub struct ContainedAdapterOutcome {
    pub response: Option<RpcResponse>,
    pub root_exit_code: Option<u32>,
    pub transport_stdout: String,
    pub transport_stderr: String,
    pub duration_ms: u64,
    pub containment: ProcessContainmentEvidence,
}

pub async fn invoke(
    path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
    cancellation: CancellationToken,
) -> anyhow::Result<ContainedAdapterOutcome> {
    let path = path.to_path_buf();
    let method = method.to_owned();
    tokio::task::spawn_blocking(move || {
        let request = RpcRequest::new(method, params);
        let mut stdin =
            serde_json::to_vec(&request).context("serialize contained adapter request")?;
        stdin.push(b'\n');

        let mut spec = ProcessSpec::new(path, Vec::<OsString>::new());
        spec.stdin = stdin;
        spec.timeout = timeout;
        spec.descendant_grace = Duration::from_secs(5);
        spec.termination_grace = Duration::from_secs(10);
        spec.max_capture_bytes = 256 * 1024;

        let child = ContainedChild::spawn(spec).context("spawn contained adapter process")?;
        let output = child
            .wait(&cancellation)
            .context("wait for contained adapter process tree")?;
        let transport_stdout = bounded_text(&output.stdout, output.stdout_truncated);
        let transport_stderr = bounded_text(&output.stderr, output.stderr_truncated);
        let response = transport_stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .and_then(|line| serde_json::from_str::<RpcResponse>(line.trim()).ok());

        Ok(ContainedAdapterOutcome {
            response,
            root_exit_code: output.exit_code,
            transport_stdout,
            transport_stderr,
            duration_ms: output.duration_ms,
            containment: map_containment(output.containment),
        })
    })
    .await
    .context("contained adapter task panicked")?
}

fn map_containment(report: ContainmentReport) -> ProcessContainmentEvidence {
    ProcessContainmentEvidence {
        method: report.method,
        root_process_id: Some(report.root_process_id),
        kill_on_job_close: report.kill_on_job_close,
        assigned_at_creation: report.assigned_at_creation,
        inherited_handle_list_restricted: report.inherited_handle_list_restricted,
        termination_reason: match report.termination_reason {
            TerminationReason::ProcessExited => ProcessTerminationReason::ProcessExited,
            TerminationReason::TimedOut => ProcessTerminationReason::TimedOut,
            TerminationReason::ExplicitCancellation => {
                ProcessTerminationReason::ExplicitCancellation
            }
            TerminationReason::DaemonShutdown => ProcessTerminationReason::DaemonShutdown,
            TerminationReason::DescendantsOutlivedRoot => {
                ProcessTerminationReason::DescendantsOutlivedRoot
            }
            TerminationReason::ContainmentFailure => ProcessTerminationReason::ContainmentFailure,
        },
        termination_requested: report.termination_requested,
        termination_confirmed: report.termination_confirmed,
        active_processes_after: report.active_processes_after,
        descendants_outlived_root: report.descendants_outlived_root,
        detail: report.detail,
    }
}

fn bounded_text(bytes: &[u8], truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if truncated {
        text.push_str("\n[ToolOS truncated contained adapter output]");
    }
    text
}
