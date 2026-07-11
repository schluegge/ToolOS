use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context};
use serde_json::json;
use tokio::process::Command;
use toolos_domain::RpcRequest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let executable_directory = std::env::current_exe()
        .context("resolve launcher path")?
        .parent()
        .map(Path::to_path_buf)
        .context("launcher has no parent directory")?;

    if toolos_ipc::request(&RpcRequest::new("daemon.ping", json!({})))
        .await
        .is_err()
    {
        start_daemon(&executable_directory).await?;
        wait_for_daemon().await?;
    }

    start_ui(&executable_directory).await
}

async fn start_daemon(directory: &Path) -> anyhow::Result<()> {
    let daemon = sibling_binary(directory, "toolos-daemon");
    Command::new(&daemon)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start daemon at {}", daemon.display()))?;
    Ok(())
}

async fn wait_for_daemon() -> anyhow::Result<()> {
    for _ in 0..40 {
        if toolos_ipc::request(&RpcRequest::new("daemon.ping", json!({})))
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(anyhow!(
        "ToolOS daemon did not become reachable after startup"
    ))
}

async fn start_ui(directory: &Path) -> anyhow::Result<()> {
    let ui = sibling_binary(directory, "toolos-ui");
    Command::new(&ui)
        .spawn()
        .with_context(|| format!("start desktop UI at {}", ui.display()))?;
    Ok(())
}

fn sibling_binary(directory: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        directory.join(format!("{name}.exe"))
    } else {
        directory.join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_binary_names_are_platform_specific() {
        let path = sibling_binary(Path::new("bin"), "toolos-daemon");
        if cfg!(windows) {
            assert!(path.ends_with("toolos-daemon.exe"));
        } else {
            assert!(path.ends_with("toolos-daemon"));
        }
    }
}
