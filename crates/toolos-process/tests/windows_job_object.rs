#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use toolos_process::{spawn_contained, ContainedCommandSpec, ProcessStopReason};
use uuid::Uuid;

const GRANDCHILD_DELAY_MS: u64 = 2_000;

#[tokio::test]
async fn normal_exit_reports_zero_active_processes_and_captures_streams() {
    let mut spec = ContainedCommandSpec::new(fixture());
    spec.args = vec![
        "output".into(),
        "--stdout-bytes".into(),
        "32".into(),
        "--stderr-bytes".into(),
        "24".into(),
    ];

    let output = spawn_contained(spec)
        .expect("spawn contained fixture")
        .wait()
        .await
        .expect("wait for contained fixture");

    assert_eq!(output.exit_code, Some(0));
    assert_eq!(output.stdout, "o".repeat(32));
    assert_eq!(output.stderr, "e".repeat(24));
    assert_eq!(output.containment.stop_reason, ProcessStopReason::Exited);
    assert_eq!(output.containment.active_processes_after_cleanup, Some(0));
    assert_eq!(output.containment.descendants_terminated, Some(true));
    assert!(output.containment.containment_confirmed);
    assert!(output.root_pid > 0);
}

#[tokio::test]
async fn timeout_terminates_parent_and_grandchild() {
    let marker = unique_path("timeout-survived.txt");
    let ready = unique_path("timeout-ready.txt");
    remove_if_present(&marker);
    remove_if_present(&ready);

    let mut spec = parent_spec(&marker, &ready, GRANDCHILD_DELAY_MS);
    spec.timeout = Duration::from_millis(700);
    let output = spawn_contained(spec)
        .expect("spawn contained parent")
        .wait()
        .await
        .expect("wait for timeout result");

    assert!(ready.exists(), "fixture never proved the grandchild started");
    assert_eq!(
        output.containment.stop_reason,
        ProcessStopReason::TimedOut
    );
    assert_eq!(output.containment.active_processes_after_cleanup, Some(0));
    assert!(output.containment.containment_confirmed);

    tokio::time::sleep(Duration::from_millis(GRANDCHILD_DELAY_MS + 500)).await;
    assert!(
        !marker.exists(),
        "grandchild wrote its survival marker after timeout"
    );
}

#[tokio::test]
async fn explicit_cancel_terminates_parent_and_grandchild() {
    let marker = unique_path("cancel-survived.txt");
    let ready = unique_path("cancel-ready.txt");
    remove_if_present(&marker);
    remove_if_present(&ready);

    let mut spec = parent_spec(&marker, &ready, GRANDCHILD_DELAY_MS);
    spec.timeout = Duration::from_secs(30);
    let contained = spawn_contained(spec).expect("spawn contained parent");
    let control = contained.control();
    wait_for_path(&ready, Duration::from_secs(5)).await;
    control
        .cancel(ProcessStopReason::Cancelled)
        .expect("cancel contained process tree");
    let output = contained.wait().await.expect("wait for cancelled tree");

    assert_eq!(
        output.containment.stop_reason,
        ProcessStopReason::Cancelled
    );
    assert_eq!(output.containment.active_processes_after_cleanup, Some(0));
    assert!(output.containment.containment_confirmed);

    tokio::time::sleep(Duration::from_millis(GRANDCHILD_DELAY_MS + 500)).await;
    assert!(
        !marker.exists(),
        "grandchild wrote its survival marker after cancellation"
    );
}

#[test]
fn closing_last_job_handle_on_host_exit_terminates_grandchild() {
    let marker = unique_path("host-exit-survived.txt");
    let ready = unique_path("host-exit-ready.txt");
    remove_if_present(&marker);
    remove_if_present(&ready);

    let status = Command::new(fixture())
        .arg("host-exit")
        .arg("--fixture")
        .arg(fixture())
        .arg("--marker")
        .arg(&marker)
        .arg("--delay-ms")
        .arg(GRANDCHILD_DELAY_MS.to_string())
        .arg("--ready")
        .arg(&ready)
        .status()
        .expect("run abrupt host-exit fixture");

    assert!(status.success());
    assert!(ready.exists(), "host exited before grandchild readiness");
    std::thread::sleep(Duration::from_millis(GRANDCHILD_DELAY_MS + 500));
    assert!(
        !marker.exists(),
        "grandchild survived closure of the final Job Object handle"
    );
}

#[tokio::test]
async fn output_capture_is_independently_bounded() {
    let mut spec = ContainedCommandSpec::new(fixture());
    spec.args = vec![
        "output".into(),
        "--stdout-bytes".into(),
        "70000".into(),
        "--stderr-bytes".into(),
        "70000".into(),
    ];
    spec.max_stdout_bytes = 4_096;
    spec.max_stderr_bytes = 4_096;

    let output = spawn_contained(spec)
        .expect("spawn large-output fixture")
        .wait()
        .await
        .expect("wait for large-output fixture");

    assert!(output.stdout.len() < 4_200);
    assert!(output.stderr.len() < 4_200);
    assert!(output.stdout.contains("ToolOS truncated contained process output"));
    assert!(output.stderr.contains("ToolOS truncated contained process output"));
    assert!(output.containment.containment_confirmed);
}

fn parent_spec(marker: &Path, ready: &Path, delay_ms: u64) -> ContainedCommandSpec {
    let executable = fixture();
    let mut spec = ContainedCommandSpec::new(&executable);
    spec.args = vec![
        "parent".into(),
        "--fixture".into(),
        executable.into_os_string(),
        "--marker".into(),
        marker.as_os_str().to_owned(),
        "--delay-ms".into(),
        delay_ms.to_string().into(),
        "--ready".into(),
        ready.as_os_str().to_owned(),
    ];
    spec
}

async fn wait_for_path(path: &Path, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {}", path.display());
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_toolos-process-fixture"))
}

fn unique_path(suffix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("toolos-{}-{suffix}", Uuid::new_v4()))
}

fn remove_if_present(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {}: {error}", path.display()),
    }
}
