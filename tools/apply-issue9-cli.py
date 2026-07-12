from pathlib import Path

path = Path("apps/toolos-cli/src/main.rs")
source = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    source = source.replace(old, new, 1)


replace_once(
    '''    WingetInstallPlanGet {
        #[arg(long)]
        plan_id: String,
    },
    /// List persisted evidence records.
''',
    '''    WingetInstallPlanGet {
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
''',
    "recovery CLI variants",
)

replace_once(
    '''        Command::WingetInstallPlanGet { plan_id } => (
            "winget.install.plan.get".to_owned(),
            json!({"plan_id": plan_id}),
        ),
        Command::Evidence { limit } => ("evidence.list".to_owned(), json!({"limit": limit})),
''',
    '''        Command::WingetInstallPlanGet { plan_id } => (
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
''',
    "recovery CLI dispatch",
)

replace_once(
    '''    #[test]
    fn clap_parses_winget_installed_query() {
''',
    '''    #[test]
    fn clap_parses_recovery_commands() {
        let cli = Cli::try_parse_from(["toolos", "winget-recovery-list"])
            .expect("parse recovery list");
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
''',
    "recovery CLI tests",
)

path.write_text(source, encoding="utf-8", newline="\n")
