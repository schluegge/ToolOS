#![cfg(windows)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use toolos_windows_job::{
    CancellationReason, CancellationToken, ContainedChild, ProcessSpec, TerminationReason,
};

#[test]
fn explicit_cancellation_terminates_nested_child() {
    let case = TestCase::new("explicit-cancel");
    let child = spawn_fixture(&case, Duration::from_secs(15));
    wait_for_path(&case.ready);

    let cancellation = CancellationToken::default();
    assert!(cancellation.cancel(CancellationReason::ExplicitCancellation));
    let output = child.wait(&cancellation).expect("wait for contained tree");

    assert_eq!(
        output.containment.termination_reason,
        TerminationReason::ExplicitCancellation
    );
    assert!(output.containment.termination_requested);
    assert!(output.containment.termination_confirmed);
    assert_eq!(output.containment.active_processes_after, Some(0));
    assert!(output.containment.assigned_at_creation);
    assert!(output.containment.inherited_handle_list_restricted);
    assert!(!output.stdout.is_empty());
    assert_descendant_did_not_survive(&case.survivor);
}

#[test]
fn timeout_terminates_nested_child() {
    let case = TestCase::new("timeout");
    let child = spawn_fixture(&case, Duration::from_secs(2));
    let output = child
        .wait(&CancellationToken::default())
        .expect("wait for timeout containment");

    assert_eq!(output.containment.termination_reason, TerminationReason::TimedOut);
    assert!(output.containment.termination_requested);
    assert!(output.containment.termination_confirmed);
    assert_eq!(output.containment.active_processes_after, Some(0));
    assert_descendant_did_not_survive(&case.survivor);
}

#[test]
fn daemon_shutdown_cancellation_is_distinct_and_confirmed() {
    let case = TestCase::new("daemon-shutdown");
    let child = spawn_fixture(&case, Duration::from_secs(15));
    wait_for_path(&case.ready);

    let cancellation = CancellationToken::default();
    assert!(cancellation.cancel(CancellationReason::DaemonShutdown));
    let output = child.wait(&cancellation).expect("wait for daemon shutdown");

    assert_eq!(
        output.containment.termination_reason,
        TerminationReason::DaemonShutdown
    );
    assert!(output.containment.termination_confirmed);
    assert_eq!(output.containment.active_processes_after, Some(0));
    assert_descendant_did_not_survive(&case.survivor);
}

#[test]
fn dropping_job_owner_kills_nested_child() {
    let case = TestCase::new("job-close");
    let child = spawn_fixture(&case, Duration::from_secs(15));
    wait_for_path(&case.ready);

    drop(child);
    assert_descendant_did_not_survive(&case.survivor);
}

fn spawn_fixture(case: &TestCase, timeout: Duration) -> ContainedChild {
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_toolos-job-tree-fixture"));
    let mut spec = ProcessSpec::new(
        executable,
        vec![
            OsString::from("parent"),
            case.ready.as_os_str().to_owned(),
            case.survivor.as_os_str().to_owned(),
        ],
    );
    spec.timeout = timeout;
    spec.descendant_grace = Duration::from_secs(2);
    spec.termination_grace = Duration::from_secs(5);
    ContainedChild::spawn(spec).expect("spawn contained fixture")
}

fn wait_for_path(path: &Path) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("fixture did not create ready marker: {}", path.display());
}

fn assert_descendant_did_not_survive(path: &Path) {
    thread::sleep(Duration::from_secs(2));
    assert!(
        !path.exists(),
        "nested child survived containment and wrote {}",
        path.display()
    );
}

struct TestCase {
    directory: PathBuf,
    ready: PathBuf,
    survivor: PathBuf,
}

impl TestCase {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "toolos-job-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        Self {
            ready: directory.join("ready.txt"),
            survivor: directory.join("survivor.txt"),
            directory,
        }
    }
}

impl Drop for TestCase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
