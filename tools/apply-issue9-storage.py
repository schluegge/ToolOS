from pathlib import Path

PATH = Path("crates/toolos-storage/src/lib.rs")
source = PATH.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    source = source.replace(old, new, 1)


replace_once(
    "use toolos_domain::{EventRecord, EvidenceRecord};\nuse uuid::Uuid;\n",
    "use toolos_domain::{EventRecord, EvidenceRecord};\nuse toolos_winget::WingetExecutionJournal;\nuse uuid::Uuid;\n\nmod recovery;\npub use recovery::*;\n",
    "module imports",
)

replace_once(
    "    M::up(\n        \"CREATE TABLE action_plan (",
    "    M::up(\n        \"CREATE TABLE action_plan (",
    "action migration anchor",
)

migration_anchor = '''    M::up(
        "CREATE TABLE action_plan (
            id TEXT PRIMARY KEY,
            capability TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            status TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            approval_phrase TEXT NOT NULL,
            record_json TEXT NOT NULL
        );
        CREATE INDEX action_plan_status_idx ON action_plan(status, expires_at);
        CREATE TABLE approval_receipt (
            id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            plan_hash TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            record_json TEXT NOT NULL,
            FOREIGN KEY(plan_id) REFERENCES action_plan(id)
        );
        CREATE TABLE resource_lock (
            resource_key TEXT PRIMARY KEY,
            holder_plan_id TEXT NOT NULL,
            acquired_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            FOREIGN KEY(holder_plan_id) REFERENCES action_plan(id)
        );",
    ),
'''

migration_new = migration_anchor + '''    M::up(
        "CREATE TABLE execution_journal (
            execution_id TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL UNIQUE,
            approval_id TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            resource_key TEXT NOT NULL,
            phase TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            provider_version TEXT,
            command_json TEXT NOT NULL,
            pre_state_json TEXT NOT NULL,
            process_identity_json TEXT,
            provider_result_json TEXT,
            recovery_policy_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            resolved_at TEXT,
            recovery_status TEXT,
            recovery_report_json TEXT,
            record_json TEXT NOT NULL,
            FOREIGN KEY(plan_id) REFERENCES action_plan(id),
            FOREIGN KEY(approval_id) REFERENCES approval_receipt(id)
        );
        CREATE INDEX execution_journal_phase_idx
            ON execution_journal(phase, updated_at);
        CREATE INDEX execution_journal_recovery_idx
            ON execution_journal(recovery_status, resolved_at);",
    ),
'''
replace_once(migration_anchor, migration_new, "journal migration")

replace_once(
    '''    #[error("resource lock {resource_key} is held by plan {holder_plan_id} until {expires_at}")]
    ResourceLocked {
        resource_key: String,
        holder_plan_id: String,
        expires_at: String,
    },
''',
    '''    #[error("resource lock {resource_key} is held by plan {holder_plan_id} until {expires_at}")]
    ResourceLocked {
        resource_key: String,
        holder_plan_id: String,
        expires_at: String,
    },
    #[error("execution journal {execution_id} is not in expected phase {expected}")]
    JournalPhaseMismatch {
        execution_id: String,
        expected: String,
    },
    #[error("invalid execution journal transition from {expected} to {next}")]
    JournalTransitionInvalid { expected: String, next: String },
    #[error("execution journal recovery result does not match report: {0}")]
    JournalRecoveryMismatch(String),
''',
    "journal errors",
)

replace_once(
    '''pub struct ActionExecutionStart<'a> {
    pub plan_id: Uuid,
    pub approval_id: Uuid,
    pub expected_hash: &'a str,
    pub updated_plan_json: &'a str,
    pub now: DateTime<Utc>,
    pub lock_expires_at: DateTime<Utc>,
}
''',
    '''pub struct ActionExecutionStart<'a> {
    pub plan_id: Uuid,
    pub approval_id: Uuid,
    pub expected_hash: &'a str,
    pub updated_plan_json: &'a str,
    pub now: DateTime<Utc>,
    pub lock_expires_at: DateTime<Utc>,
    pub journal: &'a WingetExecutionJournal,
}
''',
    "execution start contract",
)

replace_once(
    '''pub struct ActionExecutionFinish<'a> {
    pub plan_id: Uuid,
    pub final_status: &'a str,
    pub updated_plan_json: &'a str,
    pub resource_key: &'a str,
    pub release_resource_lock: bool,
}
''',
    '''pub struct ActionExecutionFinish<'a> {
    pub plan_id: Uuid,
    pub final_status: &'a str,
    pub updated_plan_json: &'a str,
    pub resource_key: &'a str,
    pub release_resource_lock: bool,
    pub journal: &'a WingetExecutionJournal,
}
''',
    "execution finish contract",
)

begin_anchor = '''        transaction.execute(
            "UPDATE resource_lock SET expires_at = ?1 WHERE resource_key = ?2 AND holder_plan_id = ?3",
            params![
                execution.lock_expires_at.to_rfc3339(),
                receipt_resource_key,
                execution.plan_id.to_string()
            ],
        )?;
        transaction.commit()?;
'''
begin_new = '''        transaction.execute(
            "UPDATE resource_lock SET expires_at = ?1 WHERE resource_key = ?2 AND holder_plan_id = ?3",
            params![
                execution.lock_expires_at.to_rfc3339(),
                receipt_resource_key,
                execution.plan_id.to_string()
            ],
        )?;
        recovery::insert_execution_journal(&transaction, execution.journal)?;
        transaction.commit()?;
'''
replace_once(begin_anchor, begin_new, "atomic journal insert")

finish_anchor = '''        if execution.release_resource_lock {
            transaction.execute(
'''
finish_new = '''        recovery::finalize_execution_journal(&transaction, execution.journal)?;
        if execution.release_resource_lock {
            transaction.execute(
'''
replace_once(finish_anchor, finish_new, "atomic journal finalization")

PATH.write_text(source, encoding="utf-8", newline="\n")
