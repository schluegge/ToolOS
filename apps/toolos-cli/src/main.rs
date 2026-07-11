use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use toolos_domain::{RpcRequest, RpcResponse};

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
    /// Create an expiring WinGet install plan. No installation is executed.
    WingetPlanInstall {
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
    /// Create an expiring WinGet uninstall plan. No uninstall is executed.
    WingetPlanUninstall {
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
    /// Approve a stored plan after exact phrase and risk acknowledgements.
    ActionApprove {
        #[arg(long)]
        plan_id: String,
        #[arg(long)]
        phrase: String,
        #[arg(long)]
        reviewed_exact_identity: bool,
        #[arg(long)]
        accepts_declared_write_scope: bool,
        #[arg(long)]
        understands_no_automatic_rollback: bool,
    },
    /// Reject a stored plan before execution exists.
    ActionReject {
        #[arg(long)]
        plan_id: String,
    },
    /// List recent durable action plans.
    Actions {
        #[arg(long, default_value_t = 50)]
        limit: usize,
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
            selector_json(id, source, version, scope, architecture),
        ),
        Command::WingetPlanInstall {
            id,
            source,
            version,
            scope,
            architecture,
        } => (
            "winget.plan.install".to_owned(),
            selector_json(id, source, version, scope, architecture),
        ),
        Command::WingetPlanUninstall {
            id,
            source,
            version,
            scope,
            architecture,
        } => (
            "winget.plan.uninstall".to_owned(),
            selector_json(id, source, version, scope, architecture),
        ),
        Command::ActionApprove {
            plan_id,
            phrase,
            reviewed_exact_identity,
            accepts_declared_write_scope,
            understands_no_automatic_rollback,
        } => (
            "actions.approve".to_owned(),
            json!({
                "plan_id": plan_id,
                "confirmation_phrase": phrase,
                "acknowledgements": {
                    "reviewed_exact_identity": reviewed_exact_identity,
                    "accepts_declared_write_scope": accepts_declared_write_scope,
                    "understands_no_automatic_rollback": understands_no_automatic_rollback
                }
            }),
        ),
        Command::ActionReject { plan_id } => {
            ("actions.reject".to_owned(), json!({"plan_id": plan_id}))
        }
        Command::Actions { limit } => ("actions.list".to_owned(), json!({"limit": limit})),
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

fn selector_json(
    id: String,
    source: String,
    version: Option<String>,
    scope: Option<WingetScope>,
    architecture: Option<String>,
) -> Value {
    json!({
        "package_id": id,
        "source": source,
        "version": version,
        "scope": scope.map(|value| value.as_str()),
        "architecture": architecture
    })
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
    fn clap_parses_exact_winget_selector() {
        let cli = Cli::try_parse_from([
            "toolos",
            "winget-plan-install",
            "--id",
            "Git.Git",
            "--source",
            "winget",
            "--scope",
            "user",
            "--architecture",
            "x64",
        ])
        .expect("parse CLI");
        match cli.command {
            Command::WingetPlanInstall {
                id,
                source,
                scope,
                architecture,
                ..
            } => {
                assert_eq!(id, "Git.Git");
                assert_eq!(source, "winget");
                assert!(matches!(scope, Some(WingetScope::User)));
                assert_eq!(architecture.as_deref(), Some("x64"));
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn approval_flags_default_to_false() {
        let cli = Cli::try_parse_from([
            "toolos",
            "action-approve",
            "--plan-id",
            "00000000-0000-0000-0000-000000000001",
            "--phrase",
            "APPROVE INSTALL Git.Git ABCD1234",
        ])
        .expect("parse CLI");
        match cli.command {
            Command::ActionApprove {
                reviewed_exact_identity,
                accepts_declared_write_scope,
                understands_no_automatic_rollback,
                ..
            } => {
                assert!(!reviewed_exact_identity);
                assert!(!accepts_declared_write_scope);
                assert!(!understands_no_automatic_rollback);
            }
            _ => panic!("wrong command"),
        }
    }
}
