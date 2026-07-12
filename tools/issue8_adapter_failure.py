from pathlib import Path


def replace_exact(path: str, old: str, new: str) -> None:
    target = Path(path)
    source = target.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one match, found {count}: {old[:120]!r}")
    target.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")


replace_exact(
    "crates/toolos-process/src/windows.rs",
    '''        if root_exited && active == 0 {
            containment_confirmed = true;
            active_after_cleanup = Some(0);
            break;
        }

        if control.inner.requested_stop.load(Ordering::Acquire) == STOP_NONE
''',
    '''        if root_exited && active == 0 {
            containment_confirmed = true;
            active_after_cleanup = Some(0);
            break;
        }

        if root_exited
            && active > 0
            && control.inner.requested_stop.load(Ordering::Acquire) == STOP_NONE
        {
            control.cancel(ProcessStopReason::ContainmentFailed)?;
            termination_requested_at = Some(Instant::now());
        }

        if control.inner.requested_stop.load(Ordering::Acquire) == STOP_NONE
''',
)

replace_exact(
    "crates/toolos-process/src/bin/toolos-process-fixture.rs",
    '''        "grandchild" => grandchild(&remaining),
        "parent" => parent(&remaining),
        "host-exit" => host_exit(&remaining),
''',
    '''        "grandchild" => grandchild(&remaining),
        "parent" => parent(&remaining),
        "parent-detach" => parent_detach(&remaining),
        "host-exit" => host_exit(&remaining),
''',
)

replace_exact(
    "crates/toolos-process/src/bin/toolos-process-fixture.rs",
    '''fn host_exit(arguments: &[std::ffi::OsString]) -> Result<(), String> {
''',
    '''fn parent_detach(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let fixture = required_path(arguments, "--fixture")?;
    let marker = required_path(arguments, "--marker")?;
    let delay = required_u64(arguments, "--delay-ms")?;
    let ready = required_path(arguments, "--ready")?;

    let child = Command::new(&fixture)
        .arg("grandchild")
        .arg("--marker")
        .arg(&marker)
        .arg("--delay-ms")
        .arg(delay.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn detached grandchild: {error}"))?;

    println!("DETACHED_PARENT_EXITING grandchild_pid={}", child.id());
    eprintln!("DETACHED_PARENT_STDERR_READY");
    fs::write(ready, b"ready").map_err(|error| format!("write ready marker: {error}"))?;
    drop(child);
    Ok(())
}

fn host_exit(arguments: &[std::ffi::OsString]) -> Result<(), String> {
''',
)

replace_exact(
    "crates/toolos-process/tests/windows_job_object.rs",
    '''#[test]
fn closing_last_job_handle_on_host_exit_terminates_grandchild() {
''',
    '''#[tokio::test]
async fn adapter_exit_with_live_descendant_is_terminated_fail_closed() {
    let marker = unique_path("adapter-failure-survived.txt");
    let ready = unique_path("adapter-failure-ready.txt");
    remove_if_present(&marker);
    remove_if_present(&ready);

    let executable = fixture();
    let mut spec = ContainedCommandSpec::new(&executable);
    spec.args = vec![
        "parent-detach".into(),
        "--fixture".into(),
        executable.into_os_string(),
        "--marker".into(),
        marker.as_os_str().to_owned(),
        "--delay-ms".into(),
        GRANDCHILD_DELAY_MS.to_string().into(),
        "--ready".into(),
        ready.as_os_str().to_owned(),
    ];
    spec.timeout = Duration::from_secs(30);

    let started = Instant::now();
    let output = spawn_contained(spec)
        .expect("spawn detached-parent fixture")
        .wait()
        .await
        .expect("wait for fail-closed containment");

    assert!(ready.exists(), "detached grandchild never started");
    assert_eq!(
        output.containment.stop_reason,
        ProcessStopReason::ContainmentFailed
    );
    assert_eq!(output.containment.active_processes_after_cleanup, Some(0));
    assert!(output.containment.containment_confirmed);
    assert!(
        output.stderr.contains("DETACHED_PARENT_STDERR_READY"),
        "adapter stderr emitted before failure was lost"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "adapter failure waited for the full execution timeout"
    );

    tokio::time::sleep(Duration::from_millis(GRANDCHILD_DELAY_MS + 500)).await;
    assert!(
        !marker.exists(),
        "grandchild survived the unexpected root-process exit"
    );
}

#[test]
fn closing_last_job_handle_on_host_exit_terminates_grandchild() {
''',
)
