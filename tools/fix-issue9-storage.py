from pathlib import Path

recovery_path = Path("crates/toolos-storage/src/recovery.rs")
recovery = recovery_path.read_text(encoding="utf-8")
old = "use chrono::{DateTime, Utc};\n"
if recovery.count(old) != 1:
    raise RuntimeError(f"recovery imports: expected one match, found {recovery.count(old)}")
recovery_path.write_text(recovery.replace(old, "", 1), encoding="utf-8", newline="\n")

path = Path("crates/toolos-storage/src/lib.rs")
source = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    source = source.replace(old, new, 1)


replace_once(
    '''    use serde_json::json;
    use tempfile::tempdir;
    use toolos_domain::EvidenceKind;
''',
    '''    use serde_json::json;
    use tempfile::tempdir;
    use toolos_domain::EvidenceKind;
    use toolos_winget::{
        build_residual_state_manifest, install_preview, installed_probe,
        ExecutionJournalPhase, InstalledQueryStatus, PackageScope, PackageSelector,
        WingetExecutionJournal, WingetInstalledStateReport, WingetRecoveryPolicy,
    };
''',
    "test imports",
)

helper_anchor = '''    #[test]
    fn unknown_execution_retains_resource_lock() {
'''
helper = '''    fn recovery_test_journal(
        plan_id: Uuid,
        approval_id: Uuid,
        now: DateTime<Utc>,
    ) -> WingetExecutionJournal {
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        };
        let installed_state = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector.clone(),
            installed_probe: installed_probe(&selector),
            installed_evidence: None,
            observed_at: now,
            definitive_installed_match: None,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        };
        WingetExecutionJournal {
            execution_id: Uuid::new_v4(),
            plan_id,
            approval_id,
            plan_hash: "abc".to_owned(),
            resource_key: "package-manager:winget".to_owned(),
            phase: ExecutionJournalPhase::Prepared,
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            command: install_preview(&selector),
            pre_state: build_residual_state_manifest(
                selector,
                Some("v1".to_owned()),
                installed_state,
                now,
            ),
            process_identity: None,
            provider_result: None,
            recovery_policy: WingetRecoveryPolicy::default(),
            created_at: now,
            updated_at: now,
            resolved_at: None,
            recovery_status: None,
        }
    }

    #[test]
    fn unknown_execution_retains_resource_lock() {
'''
replace_once(helper_anchor, helper, "journal test helper")

replace_once(
    '''        let plan_id = Uuid::new_v4();
        let connection = storage.open_connection().expect("connection");
''',
    '''        let plan_id = Uuid::new_v4();
        let approval_id = Uuid::new_v4();
        let mut journal = recovery_test_journal(plan_id, approval_id, now);
        let mut connection = storage.open_connection().expect("connection");
''',
    "test journal setup",
)

lock_insert = '''        connection
            .execute(
                "INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)
                 VALUES ('package-manager:winget', ?1, ?2, ?3)",
                params![
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(30)).to_rfc3339()
                ],
            )
            .expect("insert lock");
        drop(connection);

        storage
            .finish_action_execution(&ActionExecutionFinish {
'''
lock_new = '''        connection
            .execute(
                "INSERT INTO resource_lock (resource_key, holder_plan_id, acquired_at, expires_at)
                 VALUES ('package-manager:winget', ?1, ?2, ?3)",
                params![
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(30)).to_rfc3339()
                ],
            )
            .expect("insert lock");
        connection
            .execute(
                "INSERT INTO approval_receipt (id, plan_id, plan_hash, approved_at, expires_at, resource_key, record_json)
                 VALUES (?1, ?2, 'abc', ?3, ?4, 'package-manager:winget', '{}')",
                params![
                    approval_id.to_string(),
                    plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(5)).to_rfc3339()
                ],
            )
            .expect("insert approval");
        let transaction = connection.transaction().expect("journal transaction");
        recovery::insert_execution_journal(&transaction, &journal).expect("insert journal");
        transaction.commit().expect("commit journal");
        drop(connection);

        journal.phase = ExecutionJournalPhase::SpawnIntent;
        journal.updated_at = now + chrono::Duration::seconds(1);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Prepared, &journal)
            .expect("spawn intent");
        journal.phase = ExecutionJournalPhase::Spawned;
        journal.updated_at = now + chrono::Duration::seconds(2);
        storage
            .transition_execution_journal(ExecutionJournalPhase::SpawnIntent, &journal)
            .expect("spawned");
        journal.phase = ExecutionJournalPhase::ProviderFinished;
        journal.updated_at = now + chrono::Duration::seconds(3);
        storage
            .transition_execution_journal(ExecutionJournalPhase::Spawned, &journal)
            .expect("provider finished");
        journal.phase = ExecutionJournalPhase::Finalized;
        journal.updated_at = now + chrono::Duration::seconds(4);
        journal.resolved_at = Some(journal.updated_at);

        storage
            .finish_action_execution(&ActionExecutionFinish {
'''
replace_once(lock_insert, lock_new, "journal test lifecycle")

replace_once(
    '''                resource_key: "package-manager:winget",
                release_resource_lock: false,
            })
''',
    '''                resource_key: "package-manager:winget",
                release_resource_lock: false,
                journal: &journal,
            })
''',
    "journal test finalization",
)

path.write_text(source, encoding="utf-8", newline="\n")
