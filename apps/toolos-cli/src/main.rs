use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use toolos_domain::{RpcRequest, RpcResponse};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "toolos", version, about = "ToolOS local control-plane CLI")]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Emit compact JSON instead of formatted JSON"
    )]
    compact: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, ValueEnum)]
enum WingetScope {
    User,
    Machine,
}

impl WingetScope {
    fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Machine => "machine",
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check daemon, database, and adapter reachability.
    Doctor,
    /// Inspect host metadata and PATH-visible tool candidates without launching them.
    Scan,
    /// Inspect project identity and top-level marker files without running project code.
    Inspect { path: String },
    /// Inspect ZIP structure and extraction-path risks without extracting the archive.
    Archive { path: String },
    /// Resolve one exact WinGet package and show disabled install/uninstall previews.
    WingetResolve {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "winget")]
        source: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_enum)]
        scope: Option<WingetScope>,
        #[arg(long)]
        architecture: Option<String>,
    },
    /// Capture read-only WinGet evidence for the installed state of one exact package ID.
    WingetInstalled {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "winget")]
        source: String,
        #[arg(long, value_enum)]
        scope: Option<WingetScope>,
    },
    /// Create a governed WinGet install plan without executing the installer.
    WingetInstallPlan {
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "winget")]
        source: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_enum)]
        scope: Option<WingetScope>,
        #[arg(long)]
        architecture: Option<String>,
    },
    /// Approve one immutable install-plan hash for a short local window.
    WingetInstallApprove {
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        plan_hash: String,
        #[arg(long)]
        confirmation: String,
    },
    /// Execute one approved, pinned user-scope plan inside a Windows Job Object.
    WingetInstallExecute {
        /// Stable ID used by a second CLI process to request cancellation.
        #[arg(long)]
        execution_id: Option<String>,
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        approval_id: String,
        #[arg(long)]
        confirmation: String,
    },
    /// Request cancellation of one active contained WinGet execution.
    WingetInstallCancel {
        #[arg(long)]
        execution_id: String,
    },
    /// Show the current ToolOS WinGet package-manager lock.
    WingetInstallLock,
    /// Get one persisted install plan by UUID.
    WingetInstallPlanGet {
        #[arg(long)]
        plan_id: String,
    },
    /// List persisted WinGet execution-recovery reports.
    WingetRecoveryList,
    /// Get one persisted WinGet execution-recovery report.
    WingetRecoveryGet {
        #[arg(long)]
        execution_id: String,
    },
    /// Create an immutable execution-disabled recovery cleanup plan.
    WingetRecoveryCleanupPlan {
        #[arg(long)]
        execution_id: String,
    },
    /// Approve recovery cleanup intent while execution remains disabled.
    WingetRecoveryCleanupApprove {
        #[arg(long)]
        cleanup_plan_id: String,
        #[arg(long)]
        plan_hash: String,
        #[arg(long)]
        confirmation: String,
    },
    /// List persisted evidence records.
    Evidence {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Replay correlated daemon events.
    Events {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// List implemented capabilities and blast radius.
    Capabilities,
    /// Send a raw method and optional JSON params to the local daemon.
    Raw {
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let (method, params) = match cli.command {
        Command::Doctor => ("daemon.ping".to_owned(), json!({})),
        Command::Scan => ("machine.inspect".to_owned(), json!({})),
        Command::Inspect { path } => ("project.inspect".to_owned(), json!({"path": path})),
        Command::Archive { path } => ("archive.inspect".to_owned(), json!({"path": path})),
        Command::WingetResolve {
            id,
            source,
            version,
            scope,
            architecture,
        } => (
            "winget.resolve".to_owned(),
            json!({
                "package_id": id,
                "source": source,
                "version": version,
                "scope": scope.map(|value| value.as_str()),
                "architecture": architecture
            }),
        ),
        Command::WingetInstalled { id, source, scope } => (
            "winget.installed".to_owned(),
            json!({
                "package_id": id,
                "source": source,
                "version": Value::Null,
                "scope": scope.map(|value| value.as_str()),
                "architecture": Value::Null
            }),
        ),
        Command::WingetInstallPlan {
            id,
            source,
            version,
            scope,
            architecture,
        } => (
            "winget.install.plan".to_owned(),
            json!({
                "package_id": id,
                "source": source,
                "version": version,
                "scope": scope.map(|value| value.as_str()),
                "architecture": architecture
            }),
        ),
        Command::WingetInstallApprove {
            plan_id,
            plan_hash,
            confirmation,
        } => (
            "winget.install.approve".to_owned(),
            json!({
                "plan_id": plan_id,
                "plan_hash": plan_hash,
                "confirmation": confirmation
            }),
        ),
        Command::WingetInstallExecute {
            execution_id,
            plan_id,
            approval_id,
            confirmation,
        } => {
            let execution_id = execution_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            eprintln!("ToolOS execution ID: {execution_id}");
            eprintln!(
                "Cancel from another terminal with: toolos winget-install-cancel --execution-id {execution_id}"
            );
            (
                "winget.install.execute".to_owned(),
                json!({
                    "execution_id": execution_id,
                    "plan_id": plan_id,
                    "approval_id": approval_id,
                    "confirmation": confirmation
                }),
            )
        }
        Command::WingetInstallCancel { execution_id } => (
            "winget.install.cancel".to_owned(),
            json!({"execution_id": execution_id}),
        ),
        Command::WingetInstallLock => ("winget.install.lock".to_owned(), json!({})),
        Command::WingetInstallPlanGet { plan_id } => (
            "winget.install.plan.get".to_owned(),
            json!({"plan_id": plan_id}),
        ),
        Command::WingetRecoveryList => ("winget.recovery.list".to_owned(), json!({})),
        Command::WingetRecoveryGet { execution_id } => (
            "winget.recovery.get".to_owned(),
            json!({"execution_id": execution_id}),
        ),
        Command::WingetRecoveryCleanupPlan { execution_id } => (
            "winget.recovery.cleanup.plan".to_owned(),
            json!({"execution_id": execution_id}),
        ),
        Command::WingetRecoveryCleanupApprove {
            cleanup_plan_id,
            plan_hash,
            confirmation,
        } => (
            "winget.recovery.cleanup.approve".to_owned(),
            json!({
                "cleanup_plan_id": cleanup_plan_id,
                "plan_hash": plan_hash,
                "confirmation": confirmation
            }),
        ),
        Command::Evidence { limit } => ("evidence.list".to_owned(), json!({"limit": limit})),
        Command::Events { limit } => ("events.replay".to_owned(), json!({"limit": limit})),
        Command::Capabilities => ("capabilities.list".to_owned(), json!({})),
        Command::Raw { method, params } => {
            let params =
                serde_json::from_str::<Value>(&params).context("--params must be valid JSON")?;
            (method, params)
        }
    };

    let request = RpcRequest::new(method, params);
    let response = toolos_ipc::request(&request).await.map_err(|error| {
        anyhow!(
            "cannot reach the ToolOS daemon: {error}. Start it with `cargo run -p toolos-daemon` or the packaged ToolOS launcher"
        )
    })?;
    print_response(response, cli.compact)
}

fn print_response(response: RpcResponse, compact: bool) -> anyhow::Result<()> {
    if let Some(error) = response.error {
        return Err(anyhow!("daemon error {}: {}", error.code, error.message));
    }
    let result = response.result.unwrap_or(Value::Null);
    if compact {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_parses_project_path() {
        let cli = Cli::try_parse_from(["toolos", "inspect", "."]).expect("parse CLI");
        match cli.command {
            Command::Inspect { path } => assert_eq!(path, "."),
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn clap_parses_archive_path() {
        let cli = Cli::try_parse_from(["toolos", "archive", "input.zip"]).expect("parse CLI");
        match cli.command {
            Command::Archive { path } => assert_eq!(path, "input.zip"),
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn clap_parses_exact_winget_selector() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-resolve",
            "--id",
            "Git.Git",
            "--source",
            "winget",
            "--version",
            "2.50.1",
            "--scope",
            "user",
            "--architecture",
            "x64",
        ])
        .expect("parse CLI");
        match cli.command {
            Command::WingetResolve {
                id,
                source,
                version,
                scope,
                architecture,
            } => {
                assert_eq!(id, "Git.Git");
                assert_eq!(source, "winget");
                assert_eq!(version.as_deref(), Some("2.50.1"));
                assert!(matches!(scope, Some(WingetScope::User)));
                assert_eq!(architecture.as_deref(), Some("x64"));
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn clap_parses_governed_install_plan_and_approval() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-plan",
            "--id",
            "Git.Git",
            "--scope",
            "user",
        ])
        .expect("parse install plan");
        assert!(matches!(cli.command, Command::WingetInstallPlan { .. }));

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-approve",
            "--plan-id",
            "00000000-0000-0000-0000-000000000001",
            "--plan-hash",
            "abc",
            "--confirmation",
            "APPROVE INSTALL Git.Git abc",
        ])
        .expect("parse approval");
        assert!(matches!(cli.command, Command::WingetInstallApprove { .. }));
    }

    #[test]
    fn clap_parses_governed_install_execution_and_cancel() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-execute",
            "--execution-id",
            "00000000-0000-0000-0000-000000000003",
            "--plan-id",
            "00000000-0000-0000-0000-000000000001",
            "--approval-id",
            "00000000-0000-0000-0000-000000000002",
            "--confirmation",
            "EXECUTE INSTALL Git.Git abcdef123456",
        ])
        .expect("parse execution");
        match cli.command {
            Command::WingetInstallExecute { execution_id, .. } => {
                assert_eq!(
                    execution_id.as_deref(),
                    Some("00000000-0000-0000-0000-000000000003")
                );
            }
            _ => panic!("wrong command"),
        }

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-install-cancel",
            "--execution-id",
            "00000000-0000-0000-0000-000000000003",
        ])
        .expect("parse cancel");
        assert!(matches!(cli.command, Command::WingetInstallCancel { .. }));
    }

    #[test]
    fn clap_parses_recovery_commands() {
        let cli =
            Cli::try_parse_from(["toolos", "winget-recovery-list"]).expect("parse recovery list");
        assert!(matches!(cli.command, Command::WingetRecoveryList));

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-recovery-get",
            "--execution-id",
            "00000000-0000-0000-0000-000000000003",
        ])
        .expect("parse recovery get");
        assert!(matches!(cli.command, Command::WingetRecoveryGet { .. }));

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-recovery-cleanup-plan",
            "--execution-id",
            "00000000-0000-0000-0000-000000000003",
        ])
        .expect("parse cleanup plan");
        assert!(matches!(
            cli.command,
            Command::WingetRecoveryCleanupPlan { .. }
        ));

        let cli = Cli::try_parse_from([
            "toolos",
            "winget-recovery-cleanup-approve",
            "--cleanup-plan-id",
            "00000000-0000-0000-0000-000000000004",
            "--plan-hash",
            "abcdef",
            "--confirmation",
            "APPROVE RECOVERY CLEANUP 00000000-0000-0000-0000-000000000003 abcdef",
        ])
        .expect("parse cleanup approval");
        assert!(matches!(
            cli.command,
            Command::WingetRecoveryCleanupApprove { .. }
        ));
    }

    #[test]
    fn clap_parses_winget_installed_query() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-installed",
            "--id",
            "Git.Git",
            "--source",
            "winget",
            "--scope",
            "user",
        ])
        .expect("parse CLI");
        match cli.command {
            Command::WingetInstalled { id, source, scope } => {
                assert_eq!(id, "Git.Git");
                assert_eq!(source, "winget");
                assert!(matches!(scope, Some(WingetScope::User)));
            }
            _ => panic!("wrong command"),
        }
    }
}
