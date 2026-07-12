use std::io::ErrorKind;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use toolos_domain::{RpcRequest, RpcResponse, ADAPTER_PROTOCOL_VERSION};
use toolos_winget::{
    identity_probe, install_preview, installed_probe, normalize_selector, uninstall_preview,
    validate_execution_request, version_probe, InstalledQueryStatus, PackageSelector,
    ProcessEvidence, ResolutionStatus, WingetInstallExecutionRequest, WingetInstalledStateReport,
    WingetResolutionReport,
};

const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const STREAM_CHANNEL_CAPACITY: usize = 16;
const TRUNCATION_MARKER: &str = "\n[ToolOS truncated provider output at 65536 bytes]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderStream {
    Stdout,
    Stderr,
}

impl ProviderStream {
    fn prefix(self) -> &'static [u8] {
        match self {
            Self::Stdout => b"[winget stdout] ",
            Self::Stderr => b"[winget stderr] ",
        }
    }
}

#[derive(Debug)]
struct StreamChunk {
    stream: ProviderStream,
    bytes: Vec<u8>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await.context("read adapter request")? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => handle_request(request).await,
            Err(error) => RpcResponse::failure(
                "unknown",
                -32700,
                format!("invalid JSON-RPC request: {error}"),
            ),
        };
        let mut encoded = serde_json::to_vec(&response).context("serialize adapter response")?;
        encoded.push(b'\n');
        stdout
            .write_all(&encoded)
            .await
            .context("write adapter response")?;
        stdout.flush().await.context("flush adapter response")?;
    }

    Ok(())
}

async fn handle_request(request: RpcRequest) -> RpcResponse {
    let result: Result<Value, String> = match request.method.as_str() {
        "adapter.health" => adapter_health().await,
        "winget.resolve" => resolve_request(request.params.clone()).await,
        "winget.installed" => installed_request(request.params.clone()).await,
        "winget.install.execute" => execute_install_request(request.params.clone()).await,
        _ => Err(format!("unknown adapter method: {}", request.method)),
    };

    match result {
        Ok(value) => RpcResponse::success(request.id, value),
        Err(message) => RpcResponse::failure(request.id, -32601, message),
    }
}

async fn adapter_health() -> Result<Value, String> {
    let command = version_probe();
    match run_command(&command.executable, &command.args, Duration::from_secs(5)).await {
        Ok(evidence) => Ok(json!({
            "adapter_id": "toolos.adapter.winget",
            "adapter_version": env!("CARGO_PKG_VERSION"),
            "protocol": ADAPTER_PROTOCOL_VERSION,
            "status": if evidence.exit_code == Some(0) { "HEALTHY" } else { "DEGRADED" },
            "provider_version": provider_version(&evidence),
            "probe": evidence,
            "capabilities": [
                "package.resolve.winget",
                "package.installed.query.winget",
                "package.preview.install",
                "package.preview.uninstall",
                "package.install.execute.winget.user"
            ]
        })),
        Err(error) => Ok(json!({
            "adapter_id": "toolos.adapter.winget",
            "adapter_version": env!("CARGO_PKG_VERSION"),
            "protocol": ADAPTER_PROTOCOL_VERSION,
            "status": "UNAVAILABLE",
            "provider_version": Value::Null,
            "error": error,
            "capabilities": [
                "package.preview.install",
                "package.preview.uninstall"
            ]
        })),
    }
}

async fn resolve_request(params: Value) -> Result<Value, String> {
    let selector = parse_selector(params, "winget.resolve")?;
    let probe = identity_probe(&selector);
    let install = install_preview(&selector);
    let uninstall = uninstall_preview(&selector);

    let version = run_command("winget", &["--version".to_owned()], Duration::from_secs(5)).await;
    let provider_version = version.as_ref().ok().and_then(provider_version);

    let (status, identity_evidence, single_safest_next_action) = match run_command(
        &probe.executable,
        &probe.args,
        Duration::from_secs(45),
    )
    .await
    {
        Ok(evidence) if evidence.exit_code == Some(0) && !evidence.timed_out => (
            ResolutionStatus::ResolvedExact,
            Some(evidence),
            "Check the exact installed-state evidence before deciding whether an installation plan is needed."
                .to_owned(),
        ),
        Ok(evidence) => (
            ResolutionStatus::Blocked,
            Some(evidence),
            "Review the captured WinGet output and correct the package ID, source, filters, or source agreement state before proceeding."
                .to_owned(),
        ),
        Err(error) => (
            ResolutionStatus::Unavailable,
            Some(unavailable_evidence(&probe, error)),
            "Install or repair Microsoft App Installer / WinGet, then run exact package resolution again."
                .to_owned(),
        ),
    };

    let report = WingetResolutionReport {
        provider_id: "winget".to_owned(),
        provider_version,
        status,
        selector,
        identity_probe: probe,
        identity_evidence,
        install_preview: install,
        uninstall_preview: uninstall,
        observed_at: chrono::Utc::now(),
        limitations: vec![
            "WinGet output is preserved as provider evidence and is not parsed through locale-dependent table columns."
                .to_owned(),
            "Install and uninstall commands are previews only; execution_enabled remains false."
                .to_owned(),
            "Package and source agreements are not accepted automatically.".to_owned(),
            "A successful exact show query proves provider resolution at observation time, not installer runtime health or uninstall completeness."
                .to_owned(),
        ],
        single_safest_next_action,
    };

    serde_json::to_value(report).map_err(|error| error.to_string())
}

async fn installed_request(params: Value) -> Result<Value, String> {
    let selector = parse_selector(params, "winget.installed")?;
    let probe = installed_probe(&selector);
    let version = run_command("winget", &["--version".to_owned()], Duration::from_secs(5)).await;
    let provider_version = version.as_ref().ok().and_then(provider_version);

    let (status, installed_evidence, single_safest_next_action) = match run_command(
        &probe.executable,
        &probe.args,
        Duration::from_secs(45),
    )
    .await
    {
        Ok(evidence) if evidence.exit_code == Some(0) && !evidence.timed_out => (
            InstalledQueryStatus::QueryCompleted,
            Some(evidence),
            "Review the bounded WinGet output. ToolOS does not infer an installed match from localized table text in this slice."
                .to_owned(),
        ),
        Ok(evidence) => (
            InstalledQueryStatus::Blocked,
            Some(evidence),
            "Review the captured WinGet output and repair the package source, selector, or agreement state before relying on installed-state evidence."
                .to_owned(),
        ),
        Err(error) => (
            InstalledQueryStatus::Unavailable,
            Some(unavailable_evidence(&probe, error)),
            "Install or repair Microsoft App Installer / WinGet, then run the installed-state query again."
                .to_owned(),
        ),
    };

    let report = WingetInstalledStateReport {
        provider_id: "winget".to_owned(),
        provider_version,
        status,
        selector,
        installed_probe: probe,
        installed_evidence,
        observed_at: chrono::Utc::now(),
        definitive_installed_match: None,
        limitations: vec![
            "The WinGet CLI list output is locale-dependent and no documented stable JSON output is used here."
                .to_owned(),
            "An exit code of zero proves that the query completed, not that ToolOS parsed one definitive installed match."
                .to_owned(),
            "Version and architecture are intentionally not sent because the documented list filters used here are exact ID, source, and optional scope."
                .to_owned(),
            "No package, source, registry, file, or installer state is modified.".to_owned(),
        ],
        single_safest_next_action,
    };

    serde_json::to_value(report).map_err(|error| error.to_string())
}

async fn execute_install_request(params: Value) -> Result<Value, String> {
    let request = serde_json::from_value::<WingetInstallExecutionRequest>(params)
        .map_err(|error| format!("winget.install.execute requires a valid request: {error}"))?;
    let command = validate_execution_request(&request)?;
    let evidence = run_command_with_live_tee(
        &command.executable,
        &command.args,
        Duration::from_secs(30 * 60),
    )
    .await?;
    serde_json::to_value(evidence).map_err(|error| error.to_string())
}

fn parse_selector(params: Value, method: &str) -> Result<PackageSelector, String> {
    let selector = serde_json::from_value::<PackageSelector>(params)
        .map_err(|error| format!("{method} requires a valid package selector: {error}"))?;
    normalize_selector(selector)
}

fn unavailable_evidence(command: &toolos_winget::CommandPreview, error: String) -> ProcessEvidence {
    ProcessEvidence {
        executable: command.executable.clone(),
        args: command.args.clone(),
        exit_code: None,
        stdout: String::new(),
        stderr: error,
        timed_out: false,
        duration_ms: 0,
    }
}

async fn run_command(
    executable: &str,
    args: &[String],
    timeout: Duration,
) -> Result<ProcessEvidence, String> {
    let started = Instant::now();
    let child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start {executable}: {error}"))?;

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(ProcessEvidence {
            executable: executable.to_owned(),
            args: args.to_vec(),
            exit_code: output.status.code(),
            stdout: bounded_text(&output.stdout),
            stderr: bounded_text(&output.stderr),
            timed_out: false,
            duration_ms: duration_ms(started.elapsed()),
        }),
        Ok(Err(error)) => Err(format!("failed while waiting for {executable}: {error}")),
        Err(_) => Ok(ProcessEvidence {
            executable: executable.to_owned(),
            args: args.to_vec(),
            exit_code: None,
            stdout: String::new(),
            stderr: format!("command exceeded {} seconds", timeout.as_secs()),
            timed_out: true,
            duration_ms: duration_ms(started.elapsed()),
        }),
    }
}

async fn run_command_with_live_tee(
    executable: &str,
    args: &[String],
    timeout: Duration,
) -> Result<ProcessEvidence, String> {
    let started = Instant::now();
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start {executable}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{executable} stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{executable} stderr pipe unavailable"))?;

    let (sender, receiver) = mpsc::channel(STREAM_CHANNEL_CAPACITY);
    let stdout_sender = sender.clone();
    let stdout_task = tokio::spawn(capture_stream(
        stdout,
        ProviderStream::Stdout,
        stdout_sender,
        MAX_CAPTURE_BYTES,
    ));
    let stderr_task = tokio::spawn(capture_stream(
        stderr,
        ProviderStream::Stderr,
        sender,
        MAX_CAPTURE_BYTES,
    ));
    let mirror_task = tokio::spawn(mirror_streams(receiver));

    let (exit_code, timed_out, wait_error) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => (status.code(), false, None),
        Ok(Err(error)) => (None, false, Some(format!("failed while waiting for {executable}: {error}"))),
        Err(_) => {
            let mut errors = Vec::new();
            if let Err(error) = child.kill().await {
                if error.kind() != ErrorKind::InvalidInput {
                    errors.push(format!("failed to kill timed-out {executable}: {error}"));
                }
            }
            if let Err(error) = child.wait().await {
                errors.push(format!("failed to reap timed-out {executable}: {error}"));
            }
            let wait_error = (!errors.is_empty()).then(|| errors.join("; "));
            (None, true, wait_error)
        }
    };

    let stdout_capture = stdout_task
        .await
        .map_err(|error| format!("stdout capture task failed: {error}"))??;
    let stderr_capture = stderr_task
        .await
        .map_err(|error| format!("stderr capture task failed: {error}"))??;
    mirror_task
        .await
        .map_err(|error| format!("provider mirror task failed: {error}"))??;

    let mut stderr_text = captured_text(stderr_capture);
    if let Some(error) = wait_error {
        if !stderr_text.is_empty() {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&error);
    }
    if timed_out {
        if !stderr_text.is_empty() {
            stderr_text.push('\n');
        }
        stderr_text.push_str(&format!("command exceeded {} seconds", timeout.as_secs()));
    }

    Ok(ProcessEvidence {
        executable: executable.to_owned(),
        args: args.to_vec(),
        exit_code,
        stdout: captured_text(stdout_capture),
        stderr: stderr_text,
        timed_out,
        duration_ms: duration_ms(started.elapsed()),
    })
}

async fn capture_stream<R>(
    mut reader: R,
    stream: ProviderStream,
    sender: mpsc::Sender<StreamChunk>,
    limit: usize,
) -> Result<CapturedBytes, String>
where
    R: AsyncRead + Unpin,
{
    let mut captured = CapturedBytes::new(limit);
    let mut buffer = vec![0u8; 8 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| format!("read {stream:?}: {error}"))?;
        if read == 0 {
            break;
        }
        captured.append(&buffer[..read]);
        let _ = sender
            .send(StreamChunk {
                stream,
                bytes: buffer[..read].to_vec(),
            })
            .await;
    }
    Ok(captured)
}

async fn mirror_streams(mut receiver: mpsc::Receiver<StreamChunk>) -> Result<(), String> {
    let mut stderr = tokio::io::stderr();
    while let Some(chunk) = receiver.recv().await {
        stderr
            .write_all(chunk.stream.prefix())
            .await
            .map_err(|error| format!("write provider stream prefix: {error}"))?;
        stderr
            .write_all(&chunk.bytes)
            .await
            .map_err(|error| format!("mirror provider stream: {error}"))?;
        if !chunk.bytes.ends_with(b"\n") {
            stderr
                .write_all(b"\n")
                .await
                .map_err(|error| format!("terminate provider mirror line: {error}"))?;
        }
    }
    stderr
        .flush()
        .await
        .map_err(|error| format!("flush provider mirror: {error}"))
}

#[derive(Debug)]
struct CapturedBytes {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl CapturedBytes {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(8 * 1024)),
            limit,
            truncated: false,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        let retained = bytes.len().min(remaining);
        self.bytes.extend_from_slice(&bytes[..retained]);
        self.truncated |= retained < bytes.len();
    }
}

fn captured_text(captured: CapturedBytes) -> String {
    let mut text = String::from_utf8_lossy(&captured.bytes).into_owned();
    if captured.truncated {
        text.push_str(TRUNCATION_MARKER);
    }
    text
}

fn provider_version(evidence: &ProcessEvidence) -> Option<String> {
    evidence
        .stdout
        .lines()
        .chain(evidence.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn bounded_text(bytes: &[u8]) -> String {
    let mut captured = CapturedBytes::new(MAX_CAPTURE_BYTES);
    captured.append(bytes);
    captured_text(captured)
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_capture_is_bounded() {
        let bytes = vec![b'a'; MAX_CAPTURE_BYTES + 100];
        let text = bounded_text(&bytes);
        assert!(text.contains("ToolOS truncated provider output"));
        assert!(text.len() < MAX_CAPTURE_BYTES + 100);
    }

    #[test]
    fn chunked_capture_marks_only_real_overflow() {
        let mut captured = CapturedBytes::new(4);
        captured.append(b"ab");
        captured.append(b"cd");
        assert_eq!(captured_text(captured), "abcd");

        let mut captured = CapturedBytes::new(4);
        captured.append(b"abc");
        captured.append(b"def");
        let text = captured_text(captured);
        assert!(text.starts_with("abcd"));
        assert!(text.contains("ToolOS truncated provider output"));
    }

    #[test]
    fn live_tee_stream_prefixes_are_unambiguous() {
        assert_eq!(ProviderStream::Stdout.prefix(), b"[winget stdout] ");
        assert_eq!(ProviderStream::Stderr.prefix(), b"[winget stderr] ");
        assert_ne!(ProviderStream::Stdout.prefix(), ProviderStream::Stderr.prefix());
    }

    #[test]
    fn provider_version_uses_first_non_empty_line() {
        let evidence = ProcessEvidence {
            executable: "winget".to_owned(),
            args: vec!["--version".to_owned()],
            exit_code: Some(0),
            stdout: "\nv1.9.0\n".to_owned(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 1,
        };
        assert_eq!(provider_version(&evidence).as_deref(), Some("v1.9.0"));
    }
}
