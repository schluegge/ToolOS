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
}
