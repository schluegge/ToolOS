use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use toolos_domain::{RpcRequest, RpcResponse, ADAPTER_PROTOCOL_VERSION};
use toolos_winget::{
    identity_probe, install_preview, installed_probe, normalize_selector, uninstall_preview,
    validate_execution_request, version_probe, InstalledQueryStatus, PackageSelector,
    ProcessEvidence, ResolutionStatus, WingetInstallExecutionRequest, WingetInstalledStateReport,
    WingetResolutionReport,
};

const MAX_CAPTURE_BYTES: usize = 64 * 1024;

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
    let evidence = run_command_streaming(&command.executable, &command.args).await?;
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

#[derive(Debug, Default)]
struct StreamCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn run_command_streaming(
    executable: &str,
    args: &[String],
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
        .ok_or_else(|| format!("{executable} stdout pipe is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{executable} stderr pipe is unavailable"))?;
    let transport = Arc::new(Mutex::new(tokio::io::stderr()));

    let wait = async {
        child
            .wait()
            .await
            .map_err(|error| format!("failed while waiting for {executable}: {error}"))
    };
    let stdout_drain = drain_and_tee(stdout, Arc::clone(&transport), b"[TOOLOS_PROVIDER_STDOUT]");
    let stderr_drain = drain_and_tee(stderr, transport, b"[TOOLOS_PROVIDER_STDERR]");
    let (status, stdout, stderr) = tokio::try_join!(wait, stdout_drain, stderr_drain)?;

    Ok(ProcessEvidence {
        executable: executable.to_owned(),
        args: args.to_vec(),
        exit_code: status.code(),
        stdout: captured_text(&stdout),
        stderr: captured_text(&stderr),
        timed_out: false,
        duration_ms: duration_ms(started.elapsed()),
    })
}

async fn drain_and_tee<R>(
    mut reader: R,
    transport: Arc<Mutex<tokio::io::Stderr>>,
    label: &'static [u8],
) -> Result<StreamCapture, String>
where
    R: AsyncRead + Unpin,
{
    let mut capture = StreamCapture::default();
    let mut buffer = [0u8; 8192];
    let mut header_written = false;
    let mut truncation_written = false;
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| format!("failed to read provider output: {error}"))?;
        if read == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(capture.bytes.len());
        let keep = remaining.min(read);
        if keep > 0 {
            capture.bytes.extend_from_slice(&buffer[..keep]);
            let mut writer = transport.lock().await;
            if !header_written {
                let _ = writer
                    .write_all(
                        b"
",
                    )
                    .await;
                let _ = writer.write_all(label).await;
                let _ = writer
                    .write_all(
                        b"
",
                    )
                    .await;
                header_written = true;
            }
            let _ = writer.write_all(&buffer[..keep]).await;
            let _ = writer.flush().await;
        }
        if keep < read {
            capture.truncated = true;
            if !truncation_written {
                let mut writer = transport.lock().await;
                let _ = writer
                    .write_all(
                        b"
[ToolOS truncated live provider output at 65536 bytes]
",
                    )
                    .await;
                let _ = writer.flush().await;
                truncation_written = true;
            }
        }
    }
    Ok(capture)
}

fn captured_text(capture: &StreamCapture) -> String {
    let mut text = String::from_utf8_lossy(&capture.bytes).into_owned();
    if capture.truncated {
        text.push_str(
            "
[ToolOS truncated provider output at 65536 bytes]",
        );
    }
    text
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
    let limit = bytes.len().min(MAX_CAPTURE_BYTES);
    let mut text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    if bytes.len() > MAX_CAPTURE_BYTES {
        text.push_str("\n[ToolOS truncated provider output at 65536 bytes]");
    }
    text
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
    fn captured_stream_records_truncation_without_exceeding_bound() {
        let capture = StreamCapture {
            bytes: vec![b'a'; MAX_CAPTURE_BYTES],
            truncated: true,
        };
        let text = captured_text(&capture);
        assert!(text.contains("ToolOS truncated provider output"));
        assert!(text.len() < MAX_CAPTURE_BYTES + 100);
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
