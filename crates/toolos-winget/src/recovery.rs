use std::env;

use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toolos_process::ProcessContainmentEvidence;
use uuid::Uuid;

use crate::{
    uninstall_preview, CommandPreview, PackageSelector, ProcessEvidence, WingetInstalledStateReport,
};

const HASH_PREFIX_LENGTH: usize = 12;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionJournalPhase {
    Prepared,
    SpawnIntent,
    Spawned,
    ProviderFinished,
    Finalized,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryStatus {
    RecoveredNoProcessStarted,
    RecoveredFromPersistedProviderResult,
    FailedResidualsPresent,
    UnknownRequiresRecovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetProcessIdentity {
    pub root_pid: u32,
    pub containment_method: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetRecoveryPolicy {
    pub retain_lock_on_unknown: bool,
    pub allow_auto_finalize_provider_finished: bool,
    pub cleanup_execution_enabled: bool,
}

impl Default for WingetRecoveryPolicy {
    fn default() -> Self {
        Self {
            retain_lock_on_unknown: true,
            allow_auto_finalize_provider_finished: true,
            cleanup_execution_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetResidualStateManifest {
    pub selector: PackageSelector,
    pub captured_at: DateTime<Utc>,
    pub provider_id: String,
    pub provider_version: Option<String>,
    pub installed_state: WingetInstalledStateReport,
    pub path_environment_sha256: String,
    pub path_entry_count: usize,
    pub observable_surfaces: Vec<String>,
    pub unobserved_surfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetResidualStateDiff {
    pub provider_version_changed: bool,
    pub path_fingerprint_changed: bool,
    pub installed_evidence_changed: bool,
    pub definitive_installed_match_before: Option<bool>,
    pub definitive_installed_match_after: Option<bool>,
    pub observed_changes: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetPersistedProviderResult {
    pub completed_at: DateTime<Utc>,
    pub process_evidence: ProcessEvidence,
    pub containment_evidence: ProcessContainmentEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetExecutionJournal {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub approval_id: Uuid,
    pub plan_hash: String,
    pub resource_key: String,
    pub phase: ExecutionJournalPhase,
    pub provider_id: String,
    pub provider_version: Option<String>,
    pub command: CommandPreview,
    pub pre_state: WingetResidualStateManifest,
    pub process_identity: Option<WingetProcessIdentity>,
    pub provider_result: Option<WingetPersistedProviderResult>,
    pub recovery_policy: WingetRecoveryPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub recovery_status: Option<RecoveryStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetRecoveryReport {
    pub recovery_id: Uuid,
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub journal_phase: ExecutionJournalPhase,
    pub status: RecoveryStatus,
    pub observed_at: DateTime<Utc>,
    pub pre_state: WingetResidualStateManifest,
    pub post_state: Option<WingetResidualStateManifest>,
    pub residual_diff: Option<WingetResidualStateDiff>,
    pub lock_retained: bool,
    pub mutable_operations_blocked: bool,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryCleanupPlanStatus {
    AwaitingApproval,
    ApprovedExecutionDisabled,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RecoveryCleanupApprovalChallenge {
    pub required_phrase: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetRecoveryCleanupPlan {
    pub cleanup_plan_id: Uuid,
    pub recovery_execution_id: Uuid,
    pub plan_hash: String,
    pub status: RecoveryCleanupPlanStatus,
    pub selector: PackageSelector,
    pub uninstall_preview: CommandPreview,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub approval_allowed: bool,
    pub approval_challenge: Option<RecoveryCleanupApprovalChallenge>,
    pub execution_enabled: bool,
    pub inspection_steps: Vec<String>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetRecoveryCleanupApprovalReceipt {
    pub approval_id: Uuid,
    pub cleanup_plan_id: Uuid,
    pub recovery_execution_id: Uuid,
    pub plan_hash: String,
    pub approved_at: DateTime<Utc>,
    pub status: RecoveryCleanupPlanStatus,
    pub execution_enabled: bool,
    pub limitations: Vec<String>,
}

#[must_use]
pub fn build_residual_state_manifest(
    selector: PackageSelector,
    provider_version: Option<String>,
    installed_state: WingetInstalledStateReport,
    captured_at: DateTime<Utc>,
) -> WingetResidualStateManifest {
    let path = env::var_os("PATH").unwrap_or_default();
    let path_text = path.to_string_lossy();
    let path_environment_sha256 = sha256_hex(path_text.as_bytes());
    let path_entry_count = env::split_paths(&path).count();

    WingetResidualStateManifest {
        selector,
        captured_at,
        provider_id: "winget".to_owned(),
        provider_version,
        installed_state,
        path_environment_sha256,
        path_entry_count,
        observable_surfaces: vec![
            "Exact WinGet installed-state query and bounded provider output".to_owned(),
            "SHA-256 and entry count of the daemon PATH environment without storing PATH contents"
                .to_owned(),
            "Persisted process containment and provider exit evidence".to_owned(),
        ],
        unobserved_surfaces: vec![
            "Installer-created files outside provider inventory".to_owned(),
            "Registry values and uninstall metadata not exposed by the current provider query"
                .to_owned(),
            "Windows services, scheduled tasks, drivers, shell integrations, and running processes"
                .to_owned(),
            "Attribution of unrelated PATH or package-manager changes to this execution".to_owned(),
        ],
    }
}

#[must_use]
pub fn diff_residual_state(
    before: &WingetResidualStateManifest,
    after: &WingetResidualStateManifest,
) -> WingetResidualStateDiff {
    let provider_version_changed = before.provider_version != after.provider_version;
    let path_fingerprint_changed = before.path_environment_sha256 != after.path_environment_sha256
        || before.path_entry_count != after.path_entry_count;
    let before_installed_hash = serialized_sha256(&before.installed_state);
    let after_installed_hash = serialized_sha256(&after.installed_state);
    let installed_evidence_changed = before_installed_hash != after_installed_hash;

    let mut observed_changes = Vec::new();
    if provider_version_changed {
        observed_changes.push("The observed WinGet provider version changed.".to_owned());
    }
    if path_fingerprint_changed {
        observed_changes.push(
            "The daemon PATH fingerprint or entry count changed; ToolOS cannot attribute the change to this package."
                .to_owned(),
        );
    }
    if installed_evidence_changed {
        observed_changes.push(
            "The bounded exact installed-state evidence changed between captures.".to_owned(),
        );
    }
    if observed_changes.is_empty() {
        observed_changes.push(
            "No change was detected on the bounded provider-observable surfaces.".to_owned(),
        );
    }

    WingetResidualStateDiff {
        provider_version_changed,
        path_fingerprint_changed,
        installed_evidence_changed,
        definitive_installed_match_before: before.installed_state.definitive_installed_match,
        definitive_installed_match_after: after.installed_state.definitive_installed_match,
        observed_changes,
        limitations: vec![
            "A changed observation is not proof that the interrupted package execution caused it."
                .to_owned(),
            "An unchanged observation does not prove that files, registry entries, services, tasks, drivers, or other residuals are absent."
                .to_owned(),
            "Locale-dependent WinGet output remains evidence rather than a definitive package-health verdict."
                .to_owned(),
        ],
    }
}

pub fn build_recovery_cleanup_plan(
    report: &WingetRecoveryReport,
    now: DateTime<Utc>,
    ttl_seconds: u64,
) -> Result<WingetRecoveryCleanupPlan, String> {
    if !matches!(
        report.status,
        RecoveryStatus::UnknownRequiresRecovery | RecoveryStatus::FailedResidualsPresent
    ) {
        return Err("cleanup planning is allowed only for unresolved or detected-residual recovery reports".to_owned());
    }

    let ttl_seconds = ttl_seconds.clamp(60, 900);
    let expires_at = now + Duration::seconds(i64::try_from(ttl_seconds).unwrap_or(900));
    let cleanup_plan_id = Uuid::new_v4();
    let selector = report.pre_state.selector.clone();
    let uninstall_preview = uninstall_preview(&selector);
    let plan_hash = cleanup_plan_hash(
        cleanup_plan_id,
        report.execution_id,
        &selector,
        &uninstall_preview,
        now,
        expires_at,
    )?;
    let required_phrase = cleanup_approval_phrase(report.execution_id, &plan_hash)?;

    Ok(WingetRecoveryCleanupPlan {
        cleanup_plan_id,
        recovery_execution_id: report.execution_id,
        plan_hash,
        status: RecoveryCleanupPlanStatus::AwaitingApproval,
        selector,
        uninstall_preview,
        created_at: now,
        expires_at,
        approval_allowed: true,
        approval_challenge: Some(RecoveryCleanupApprovalChallenge {
            required_phrase,
            expires_at,
        }),
        execution_enabled: false,
        inspection_steps: vec![
            "Review the pre-state, post-state, residual diff, and provider output.".to_owned(),
            "Inspect WinGet logs and package-specific files, registry entries, services, tasks, and processes."
                .to_owned(),
            "Use the uninstall preview only after independently proving that uninstall is the correct recovery action."
                .to_owned(),
        ],
        limitations: vec![
            "Approval records intent only; ToolOS has no cleanup executor in Issue #9.".to_owned(),
            "The uninstall preview is not proof that uninstall will remove partial or non-provider residuals."
                .to_owned(),
            "No files, registry entries, services, tasks, packages, or processes are modified by this plan."
                .to_owned(),
        ],
        single_safest_next_action:
            "Review the recovery evidence, then approve this exact disabled cleanup plan only if its inspection and uninstall preview are appropriate."
                .to_owned(),
    })
}

pub fn approve_recovery_cleanup_plan(
    plan: &WingetRecoveryCleanupPlan,
    confirmation: &str,
    now: DateTime<Utc>,
) -> Result<(WingetRecoveryCleanupPlan, WingetRecoveryCleanupApprovalReceipt), String> {
    if plan.status != RecoveryCleanupPlanStatus::AwaitingApproval || !plan.approval_allowed {
        return Err("recovery cleanup plan is not awaiting approval".to_owned());
    }
    if now >= plan.expires_at {
        return Err("recovery cleanup plan expired".to_owned());
    }
    let challenge = plan
        .approval_challenge
        .as_ref()
        .ok_or_else(|| "recovery cleanup plan has no approval challenge".to_owned())?;
    if confirmation.trim() != challenge.required_phrase {
        return Err("recovery cleanup approval phrase mismatch".to_owned());
    }

    let mut approved = plan.clone();
    approved.status = RecoveryCleanupPlanStatus::ApprovedExecutionDisabled;
    approved.approval_allowed = false;
    approved.approval_challenge = None;
    approved.execution_enabled = false;
    approved.single_safest_next_action =
        "Approval was recorded. ToolOS will not execute cleanup; perform further package-specific review."
            .to_owned();

    let receipt = WingetRecoveryCleanupApprovalReceipt {
        approval_id: Uuid::new_v4(),
        cleanup_plan_id: plan.cleanup_plan_id,
        recovery_execution_id: plan.recovery_execution_id,
        plan_hash: plan.plan_hash.clone(),
        approved_at: now,
        status: RecoveryCleanupPlanStatus::ApprovedExecutionDisabled,
        execution_enabled: false,
        limitations: approved.limitations.clone(),
    };
    Ok((approved, receipt))
}

pub fn cleanup_approval_phrase(execution_id: Uuid, plan_hash: &str) -> Result<String, String> {
    let prefix = plan_hash
        .get(..HASH_PREFIX_LENGTH)
        .ok_or_else(|| "cleanup plan hash is too short".to_owned())?;
    Ok(format!(
        "APPROVE RECOVERY CLEANUP {execution_id} {prefix}"
    ))
}

fn cleanup_plan_hash(
    cleanup_plan_id: Uuid,
    execution_id: Uuid,
    selector: &PackageSelector,
    preview: &CommandPreview,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<String, String> {
    let payload = serde_json::to_vec(&(
        cleanup_plan_id,
        execution_id,
        selector,
        preview,
        created_at,
        expires_at,
        false,
    ))
    .map_err(|error| error.to_string())?;
    Ok(sha256_hex(&payload))
}

fn serialized_sha256<T: Serialize>(value: &T) -> String {
    match serde_json::to_vec(value) {
        Ok(bytes) => sha256_hex(&bytes),
        Err(error) => sha256_hex(error.to_string().as_bytes()),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        install_preview, InstalledQueryStatus, PackageScope, WingetInstalledStateReport,
    };

    fn selector() -> PackageSelector {
        PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        }
    }

    fn installed_state(definitive: Option<bool>) -> WingetInstalledStateReport {
        let selector = selector();
        WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector.clone(),
            installed_probe: install_preview(&selector),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: definitive,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        }
    }

    fn recovery_report(status: RecoveryStatus) -> WingetRecoveryReport {
        let now = Utc::now();
        let pre_state = build_residual_state_manifest(
            selector(),
            Some("v1".to_owned()),
            installed_state(None),
            now,
        );
        WingetRecoveryReport {
            recovery_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            plan_id: Uuid::new_v4(),
            journal_phase: ExecutionJournalPhase::Spawned,
            status,
            observed_at: now,
            pre_state,
            post_state: None,
            residual_diff: None,
            lock_retained: true,
            mutable_operations_blocked: true,
            limitations: Vec::new(),
            single_safest_next_action: "inspect".to_owned(),
        }
    }

    #[test]
    fn phases_and_statuses_use_stable_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&ExecutionJournalPhase::ProviderFinished).unwrap(),
            "\"PROVIDER_FINISHED\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryStatus::UnknownRequiresRecovery).unwrap(),
            "\"UNKNOWN_REQUIRES_RECOVERY\""
        );
    }

    #[test]
    fn residual_diff_does_not_attribute_changes() {
        let now = Utc::now();
        let before = build_residual_state_manifest(
            selector(),
            Some("v1".to_owned()),
            installed_state(None),
            now,
        );
        let mut after = before.clone();
        after.provider_version = Some("v2".to_owned());
        after.path_environment_sha256 = "changed".to_owned();
        after.installed_state.definitive_installed_match = Some(true);
        let diff = diff_residual_state(&before, &after);
        assert!(diff.provider_version_changed);
        assert!(diff.path_fingerprint_changed);
        assert!(diff.installed_evidence_changed);
        assert!(diff
            .limitations
            .iter()
            .any(|value| value.contains("not proof")));
    }

    #[test]
    fn cleanup_plan_is_hashed_approved_separately_and_never_executable() {
        let report = recovery_report(RecoveryStatus::UnknownRequiresRecovery);
        let now = Utc::now();
        let plan = build_recovery_cleanup_plan(&report, now, 300).unwrap();
        assert_eq!(plan.plan_hash.len(), 64);
        assert!(!plan.execution_enabled);
        assert!(!plan.uninstall_preview.execution_enabled);
        let phrase = plan
            .approval_challenge
            .as_ref()
            .unwrap()
            .required_phrase
            .clone();
        let (approved, receipt) = approve_recovery_cleanup_plan(&plan, &phrase, now).unwrap();
        assert_eq!(
            approved.status,
            RecoveryCleanupPlanStatus::ApprovedExecutionDisabled
        );
        assert!(!approved.execution_enabled);
        assert!(!receipt.execution_enabled);
    }

    #[test]
    fn resolved_recovery_cannot_create_cleanup_plan() {
        let report = recovery_report(RecoveryStatus::RecoveredNoProcessStarted);
        assert!(build_recovery_cleanup_plan(&report, Utc::now(), 300).is_err());
    }
}
