from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one marker, found {count}: {old[:120]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


path = "adapters/toolos-winget-adapter/src/main.rs"
replace_once(
    path,
    "use std::process::Stdio;\nuse std::time::{Duration, Instant};\n",
    "use std::process::Stdio;\nuse std::sync::Arc;\nuse std::time::{Duration, Instant};\n",
)
replace_once(
    path,
    "use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};\n",
    "use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};\nuse tokio::sync::Mutex;\n",
)
replace_once(
    path,
    '''    let evidence = run_command(
        &command.executable,
        &command.args,
        Duration::from_secs(30 * 60),
    )
    .await?;
''',
    '''    let evidence = run_command_streaming(&command.executable, &command.args).await?;
''',
)
streaming = '''
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
    let stdout_drain = drain_and_tee(
        stdout,
        Arc::clone(&transport),
        b"[TOOLOS_PROVIDER_STDOUT]",
    );
    let stderr_drain = drain_and_tee(
        stderr,
        transport,
        b"[TOOLOS_PROVIDER_STDERR]",
    );
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
                let _ = writer.write_all(b"\n").await;
                let _ = writer.write_all(label).await;
                let _ = writer.write_all(b"\n").await;
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
                    .write_all(b"\n[ToolOS truncated live provider output at 65536 bytes]\n")
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
        text.push_str("\n[ToolOS truncated provider output at 65536 bytes]");
    }
    text
}

'''
replace_once(path, "async fn run_command(\n", streaming + "async fn run_command(\n")
replace_once(
    path,
    '''    #[test]
    fn provider_version_uses_first_non_empty_line() {
''',
    '''    #[test]
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
''',
)

# Document the evidence improvement.
replace_once(
    "docs/provider-decisions/ADR-0005-windows-job-containment.md",
    "6. captures bounded transport output while the process tree runs;\n",
    "6. live-tees bounded WinGet stdout and stderr through the adapter's stderr transport while also retaining them for a normal JSON result;\n",
)
replace_once(
    "docs/provider-decisions/ADR-0005-windows-job-containment.md",
    "The test fails when the nested child survives long enough to write its survivor marker.\n",
    "The test fails when the nested child survives long enough to write its survivor marker. The executing adapter additionally live-tees bounded provider output, so observations emitted before forced termination remain available even when the adapter cannot return its final JSON response.\n",
)
