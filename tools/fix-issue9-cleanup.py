from pathlib import Path

lib_path = Path("crates/toolos-storage/src/lib.rs")
lib = lib_path.read_text(encoding="utf-8")
old = "mod cleanup;\nmod recovery;\npub use cleanup::*;\npub use recovery::*;\n"
new = "mod cleanup;\nmod recovery;\npub use recovery::*;\n"
if lib.count(old) != 1:
    raise RuntimeError(f"cleanup module export: expected one match, found {lib.count(old)}")
lib_path.write_text(lib.replace(old, new, 1), encoding="utf-8", newline="\n")

path = Path("crates/toolos-storage/src/cleanup.rs")
source = path.read_text(encoding="utf-8")
old = '''        let now = Utc::now();
        let plan = build_recovery_cleanup_plan(&report(), now, 300).expect("plan");
        storage
'''
new = '''        let now = Utc::now();
        let recovery = report();
        let approval_id = Uuid::new_v4();
        let connection = storage.open_connection().expect("connection");
        connection
            .execute(
                "INSERT INTO action_plan (id, capability, resource_key, status, plan_hash, created_at, expires_at, approval_phrase, record_json)
                 VALUES (?1, 'package.install.plan.winget', 'package-manager:winget', 'UNKNOWN_REQUIRES_RECOVERY', 'abc', ?2, ?3, '', '{}')",
                params![
                    recovery.plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(10)).to_rfc3339()
                ],
            )
            .expect("insert plan");
        connection
            .execute(
                "INSERT INTO approval_receipt (id, plan_id, plan_hash, approved_at, expires_at, resource_key, record_json)
                 VALUES (?1, ?2, 'abc', ?3, ?4, 'package-manager:winget', '{}')",
                params![
                    approval_id.to_string(),
                    recovery.plan_id.to_string(),
                    now.to_rfc3339(),
                    (now + chrono::Duration::minutes(5)).to_rfc3339()
                ],
            )
            .expect("insert approval");
        connection
            .execute(
                "INSERT INTO execution_journal (
                    execution_id, plan_id, approval_id, plan_hash, resource_key, phase,
                    provider_id, provider_version, command_json, pre_state_json,
                    process_identity_json, provider_result_json, recovery_policy_json,
                    created_at, updated_at, resolved_at, recovery_status,
                    recovery_report_json, record_json
                 ) VALUES (?1, ?2, ?3, 'abc', 'package-manager:winget', 'FINALIZED',
                           'winget', 'v1', '{}', ?4, NULL, NULL, '{}', ?5, ?5, ?5,
                           'UNKNOWN_REQUIRES_RECOVERY', ?6, '{}')",
                params![
                    recovery.execution_id.to_string(),
                    recovery.plan_id.to_string(),
                    approval_id.to_string(),
                    serde_json::to_string(&recovery.pre_state).expect("pre-state"),
                    now.to_rfc3339(),
                    serde_json::to_string(&recovery).expect("recovery report")
                ],
            )
            .expect("insert recovery journal");
        drop(connection);

        let plan = build_recovery_cleanup_plan(&recovery, now, 300).expect("plan");
        storage
'''
if source.count(old) != 1:
    raise RuntimeError(f"cleanup test setup: expected one match, found {source.count(old)}")
path.write_text(source.replace(old, new, 1), encoding="utf-8", newline="\n")
