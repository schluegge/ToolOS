from pathlib import Path


def patch(path_text: str, replacements: list[tuple[str, str, str]]) -> None:
    path = Path(path_text)
    source = path.read_text(encoding="utf-8")
    for old, new, label in replacements:
        count = source.count(old)
        if count != 1:
            raise RuntimeError(f"{path_text} {label}: expected one match, found {count}")
        source = source.replace(old, new, 1)
    path.write_text(source, encoding="utf-8", newline="\n")


patch(
    "crates/toolos-storage/src/lib.rs",
    [
        (
            "mod recovery;\npub use recovery::*;\n",
            "mod cleanup;\nmod recovery;\npub use cleanup::*;\npub use recovery::*;\n",
            "cleanup module export",
        ),
        (
            '''    M::up(
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
''',
            '''    M::up(
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
    M::up(
        "CREATE TABLE recovery_cleanup_plan (
            id TEXT PRIMARY KEY,
            recovery_execution_id TEXT NOT NULL,
            plan_hash TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            approval_phrase TEXT NOT NULL,
            execution_enabled INTEGER NOT NULL CHECK(execution_enabled = 0),
            record_json TEXT NOT NULL,
            FOREIGN KEY(recovery_execution_id) REFERENCES execution_journal(execution_id)
        );
        CREATE INDEX recovery_cleanup_execution_idx
            ON recovery_cleanup_plan(recovery_execution_id, created_at DESC);
        CREATE TABLE recovery_cleanup_approval (
            id TEXT PRIMARY KEY,
            cleanup_plan_id TEXT NOT NULL UNIQUE,
            plan_hash TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            execution_enabled INTEGER NOT NULL CHECK(execution_enabled = 0),
            record_json TEXT NOT NULL,
            FOREIGN KEY(cleanup_plan_id) REFERENCES recovery_cleanup_plan(id)
        );",
    ),
''',
            "cleanup migration",
        ),
    ],
)

patch(
    "apps/toolos-daemon/src/main.rs",
    [
        (
            '''        "winget.recovery.list" => recovery::list(state),
        "winget.recovery.get" => recovery::get(state, &request.params),
''',
            '''        "winget.recovery.list" => recovery::list(state),
        "winget.recovery.get" => recovery::get(state, &request.params),
        "winget.recovery.cleanup.plan" => {
            recovery::cleanup_plan(state, trace_id, &request.params)
        }
        "winget.recovery.cleanup.approve" => {
            recovery::cleanup_approve(state, trace_id, &request.params)
        }
''',
            "cleanup rpc routes",
        ),
        (
            '''        {
            "capability_id": "package.preview.install",
''',
            '''        {
            "capability_id": "package.recovery.cleanup.plan.winget",
            "provider_id": "toolos.daemon.recovery",
            "blast_radius": "LOCAL_METADATA_WRITE",
            "status": "APPROVAL_ONLY_EXECUTION_DISABLED"
        },
        {
            "capability_id": "package.preview.install",
''',
            "cleanup capability",
        ),
    ],
)

patch(
    "apps/toolos-daemon/src/recovery.rs",
    [
        (
            '''use toolos_winget::{
    build_residual_state_manifest, diff_residual_state, ExecutionJournalPhase, InstallPlanStatus,
    RecoveryStatus, WingetExecutionJournal, WingetInstallPlan, WingetRecoveryReport,
    WingetResidualStateManifest,
};
''',
            '''use toolos_winget::{
    approve_recovery_cleanup_plan, build_recovery_cleanup_plan, build_residual_state_manifest,
    diff_residual_state, ExecutionJournalPhase, InstallPlanStatus, RecoveryStatus,
    WingetExecutionJournal, WingetInstallPlan, WingetRecoveryReport,
    WingetResidualStateManifest,
};
''',
            "cleanup imports",
        ),
        (
            '''pub(super) fn get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
''',
            '''pub(super) fn cleanup_plan(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let execution_id = required_uuid(params, "execution_id", "winget.recovery.cleanup.plan")?;
    let report = state
        .storage
        .get_recovery_report(execution_id)?
        .with_context(|| format!("recovery report not found: {execution_id}"))?;
    let plan = build_recovery_cleanup_plan(&report, Utc::now(), 600)
        .map_err(anyhow::Error::msg)?;
    state.storage.store_recovery_cleanup_plan(&plan)?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-recovery-cleanup-plan:{}", plan.cleanup_plan_id),
        "Execution-disabled recovery cleanup plan created",
        "toolos.daemon.recovery",
        serde_json::to_value(&plan)?,
        plan.limitations.clone(),
    )?;
    record_evidence(
        state,
        trace_id,
        &evidence,
        "WINGET_RECOVERY_CLEANUP_PLAN",
    )?;
    Ok(json!({"plan": plan, "evidence": evidence}))
}

pub(super) fn cleanup_approve(
    state: &AppState,
    trace_id: Uuid,
    params: &Value,
) -> anyhow::Result<Value> {
    let cleanup_plan_id = required_uuid(
        params,
        "cleanup_plan_id",
        "winget.recovery.cleanup.approve",
    )?;
    let plan_hash = required_string(
        params,
        "plan_hash",
        "winget.recovery.cleanup.approve",
    )?;
    let confirmation = required_string(
        params,
        "confirmation",
        "winget.recovery.cleanup.approve",
    )?;
    let plan = state
        .storage
        .get_recovery_cleanup_plan(cleanup_plan_id)?
        .with_context(|| format!("recovery cleanup plan not found: {cleanup_plan_id}"))?;
    if plan.plan_hash != plan_hash {
        return Err(anyhow!("recovery cleanup plan hash mismatch"));
    }
    let now = Utc::now();
    let (approved, receipt) =
        approve_recovery_cleanup_plan(&plan, &confirmation, now).map_err(anyhow::Error::msg)?;
    state.storage.approve_recovery_cleanup_plan(
        cleanup_plan_id,
        &plan_hash,
        &confirmation,
        now,
        &approved,
        &receipt,
    )?;
    let evidence = EvidenceRecord::new(
        trace_id,
        EvidenceKind::AdapterInvocation,
        format!("winget-recovery-cleanup-approval:{}", receipt.approval_id),
        "Recovery cleanup intent approved while execution remained disabled",
        "toolos.daemon.recovery",
        json!({"plan": approved, "receipt": receipt}),
        approved.limitations.clone(),
    )?;
    record_evidence(
        state,
        trace_id,
        &evidence,
        "WINGET_RECOVERY_CLEANUP_APPROVAL",
    )?;
    Ok(json!({"plan": approved, "receipt": receipt, "evidence": evidence}))
}

pub(super) fn get(state: &AppState, params: &Value) -> anyhow::Result<Value> {
''',
            "cleanup handlers",
        ),
        (
            '''fn phase_name(phase: ExecutionJournalPhase) -> &'static str {
''',
            '''fn required_string(params: &Value, key: &str, method: &str) -> anyhow::Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("{method} requires non-empty string {key}"))
}

fn required_uuid(params: &Value, key: &str, method: &str) -> anyhow::Result<Uuid> {
    let value = required_string(params, key, method)?;
    Uuid::parse_str(&value).with_context(|| format!("{method} requires UUID {key}"))
}

fn phase_name(phase: ExecutionJournalPhase) -> &'static str {
''',
            "cleanup parameter helpers",
        ),
    ],
)
