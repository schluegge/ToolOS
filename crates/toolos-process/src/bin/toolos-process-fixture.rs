use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::thread;
use std::time::Duration;

use toolos_process::{spawn_contained, ContainedCommandSpec};

fn main() {
    if let Err(error) = run() {
        eprintln!("fixture error: {error}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let mode = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| "missing fixture mode".to_owned())?;
    let remaining: Vec<_> = arguments.collect();

    match mode.as_str() {
        "grandchild" => grandchild(&remaining),
        "parent" => parent(&remaining),
        "host-exit" => host_exit(&remaining),
        "output" => output(&remaining),
        _ => Err(format!("unknown fixture mode: {mode}")),
    }
}

fn grandchild(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let marker = required_path(arguments, "--marker")?;
    let delay = required_u64(arguments, "--delay-ms")?;
    thread::sleep(Duration::from_millis(delay));
    fs::write(marker, b"grandchild survived")
        .map_err(|error| format!("write grandchild marker: {error}"))
}

fn parent(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let fixture = required_path(arguments, "--fixture")?;
    let marker = required_path(arguments, "--marker")?;
    let delay = required_u64(arguments, "--delay-ms")?;
    let ready = optional_path(arguments, "--ready");

    let mut child = Command::new(&fixture)
        .arg("grandchild")
        .arg("--marker")
        .arg(&marker)
        .arg("--delay-ms")
        .arg(delay.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("spawn grandchild: {error}"))?;

    println!("PARENT_READY grandchild_pid={}", child.id());
    eprintln!("PARENT_STDERR_READY");
    if let Some(path) = ready {
        fs::write(path, b"ready").map_err(|error| format!("write ready marker: {error}"))?;
    }
    child
        .wait()
        .map_err(|error| format!("wait for grandchild: {error}"))?;
    Ok(())
}

fn host_exit(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let fixture = required_path(arguments, "--fixture")?;
    let marker = required_path(arguments, "--marker")?;
    let delay = required_u64(arguments, "--delay-ms")?;
    let ready = required_path(arguments, "--ready")?;

    let mut spec = ContainedCommandSpec::new(fixture.as_os_str());
    spec.args = vec![
        "parent".into(),
        "--fixture".into(),
        fixture.into_os_string(),
        "--marker".into(),
        marker.into_os_string(),
        "--delay-ms".into(),
        delay.to_string().into(),
        "--ready".into(),
        ready.clone().into_os_string(),
    ];
    spec.timeout = Duration::from_secs(60);
    let contained = spawn_contained(spec).map_err(|error| error.to_string())?;

    wait_for_path(&ready, Duration::from_secs(5))?;
    println!(
        "HOST_EXIT_READY execution_id={} root_pid={}",
        contained.execution_id(),
        contained.root_pid()
    );
    process::exit(0);
}

fn output(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    let stdout_bytes = required_usize(arguments, "--stdout-bytes")?;
    let stderr_bytes = required_usize(arguments, "--stderr-bytes")?;
    print!("{}", "o".repeat(stdout_bytes));
    eprint!("{}", "e".repeat(stderr_bytes));
    Ok(())
}

fn wait_for_path(path: &Path, timeout: Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(format!("timed out waiting for {}", path.display()))
}

fn required_path(arguments: &[std::ffi::OsString], flag: &str) -> Result<PathBuf, String> {
    optional_value(arguments, flag)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing {flag}"))
}

fn optional_path(arguments: &[std::ffi::OsString], flag: &str) -> Option<PathBuf> {
    optional_value(arguments, flag).map(PathBuf::from)
}

fn required_u64(arguments: &[std::ffi::OsString], flag: &str) -> Result<u64, String> {
    optional_value(arguments, flag)
        .and_then(|value| value.to_str()?.parse().ok())
        .ok_or_else(|| format!("missing or invalid {flag}"))
}

fn required_usize(arguments: &[std::ffi::OsString], flag: &str) -> Result<usize, String> {
    optional_value(arguments, flag)
        .and_then(|value| value.to_str()?.parse().ok())
        .ok_or_else(|| format!("missing or invalid {flag}"))
}

fn optional_value<'a>(
    arguments: &'a [std::ffi::OsString],
    flag: &str,
) -> Option<&'a std::ffi::OsStr> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_os_str())
}
