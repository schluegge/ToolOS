use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toolos_winget::{CommandPreview, ResolutionStatus, WingetResolutionReport};
use uuid::Uuid;

const PLAN_LIFETIME_MINUTES: i64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionKind {
    WingetInstall,
    WingetUninstall,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionStatus {
    WaitingApproval,
    ApprovedAwaitingExecutor,
    Rejected,
    Expired,
}

impl ActionStatus {
    #[must_use]
    pub fn storage_value(&self) -> &'static str {
        match self {
            Self::WaitingApproval => "WAITING_APPROVAL",
            Self::ApprovedAwaitingExecutor => "APPROVED_AWAITING_EXECUTOR",
            Self::Rejected => "REJECTED",
            Self::Expired => "EXPIRED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalAcknowledgements {
    pub reviewed_exact_identity: bool,
    pub accepts_declared_write_scope: bool,
    pub understands_no_automatic_rollback: bool,
}

impl ApprovalAcknowledgements {
    #[must_use]
    pub fn all_confirmed(&self) -> bool {
        self.reviewed_exact_identity
            && self.accepts_declared_write_scope
            && self.understands_no_automatic_rollback
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub plan_id: Uuid,
    pub confirmation_phrase: String,
    pub acknowledgements: ApprovalAcknowledgements,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RollbackManifest {
    pub reversibility: String,
    pub uninstall_preview: CommandPreview,
    pub covered_surfaces: Vec<String>,
    pub uncovered_surfaces: Vec<String>,
    pub single_safest_recovery_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetActionPlan {
    pub id: Uuid,
    pub trace_id: Uuid,
    pub kind: ActionKind,
    pub status: ActionStatus,
    pub package_resolution: WingetResolutionReport,
    pub command: CommandPreview,
    pub command_sha256: String,
    pub confirmation_phrase: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub acknowledgements: Option<ApprovalAcknowledgements>,
    pub execution_available: bool,
    pub blocked_execution_gates: Vec<String>,
    pub rollback: RollbackManifest,
}

pub fn create_install_plan(
    trace_id: Uuid,
    resolution: WingetResolutionReport,
    now: DateTime<Utc>,
) -> Result<WingetActionPlan, String> {
    validate_resolution(&resolution)?;
    create_plan(
        trace_id,
        ActionKind::WingetInstall,
        resolution.install_preview.clone(),
        resolution,
        now,
    )
}

pub fn create_uninstall_plan(
    trace_id: Uuid,
    resolution: WingetResolutionReport,
    now: DateTime<Utc>,
) -> Result<WingetActionPlan, String> {
    validate_resolution(&resolution)?;
    create_plan(
        trace_id,
        ActionKind::WingetUninstall,
        resolution.uninstall_preview.clone(),
        resolution,
        now,
    )
}

pub fn approve_plan(
    mut plan: WingetActionPlan,
    request: &ApprovalRequest,
    now: DateTime<Utc>,
) -> Result<WingetActionPlan, String> {
    if plan.id != request.plan_id {
        return Err("approval plan ID does not match the stored plan".to_owned());
    }
    if plan.status != ActionStatus::WaitingApproval {
        return Err(format!(
            "plan is not waiting for approval; current status is {}",
            plan.status.storage_value()
        ));
    }
    if now >= plan.expires_at {
        plan.status = ActionStatus::Expired;
        return Err("action plan expired; create a fresh plan and resolve the package again".to_owned());
    }
    if request.confirmation_phrase != plan.confirmation_phrase {
        return Err("confirmation phrase does not match the action plan".to_owned());
    }
    if !request.acknowledgements.all_confirmed() {
        return Err("all three approval acknowledgements are required".to_owned());
    }
    if hash_command(&plan.command)? != plan.command_sha256 {
        return Err("stored command hash no longer matches the action plan".to_owned());
    }

    plan.status = ActionStatus::ApprovedAwaitingExecutor;
    plan.approved_at = Some(now);
    plan.acknowledgements = Some(request.acknowledgements.clone());
    Ok(plan)
}

pub fn reject_plan(
    mut plan: WingetActionPlan,
    now: DateTime<Utc>,
) -> Result<WingetActionPlan, String> {
    if plan.status != ActionStatus::WaitingApproval {
        return Err(format!(
            "plan cannot be rejected from status {}",
            plan.status.storage_value()
        ));
    }
    if now >= plan.expires_at {
        plan.status = ActionStatus::Expired;
    } else {
        plan.status = ActionStatus::Rejected;
    }
    Ok(plan)
}

fn create_plan(
    trace_id: Uuid,
    kind: ActionKind,
    command: CommandPreview,
    resolution: WingetResolutionReport,
    now: DateTime<Utc>,
) -> Result<WingetActionPlan, String> {
    if command.execution_enabled {
        return Err("source command preview unexpectedly enabled execution".to_owned());
    }

    let id = Uuid::new_v4();
    let verb = match kind {
        ActionKind::WingetInstall => "INSTALL",
        ActionKind::WingetUninstall => "UNINSTALL",
    };
    let confirmation_phrase = format!(
        "APPROVE {verb} {} {}",
        resolution.selector.package_id,
        short_id(id)
    );
    let rollback = RollbackManifest {
        reversibility: "PARTIAL_UNVERIFIED".to_owned(),
        uninstall_preview: resolution.uninstall_preview.clone(),
        covered_surfaces: vec![
            "Exact package selector and proposed uninstall command are preserved.".to_owned(),
            "Provider command and action evidence remain in local ToolOS state.".to_owned(),
        ],
        uncovered_surfaces: vec![
            "Installer-created user data, caches, services, scheduled tasks, PATH edits, registry values, and shared dependencies are not yet inventoried automatically."
                .to_owned(),
            "The package uninstaller may be missing, incomplete, interactive, or require elevation."
                .to_owned(),
            "ToolOS has not yet proven process-tree cancellation or restoration after partial installation."
                .to_owned(),
        ],
        single_safest_recovery_action:
            "Do not execute until ToolOS has captured a pre-install state manifest and a tested process-tree cancellation boundary."
                .to_owned(),
    };

    Ok(WingetActionPlan {
        id,
        trace_id,
        kind,
        status: ActionStatus::WaitingApproval,
        package_resolution: resolution,
        command_sha256: hash_command(&command)?,
        command,
        confirmation_phrase,
        created_at: now,
        expires_at: now + Duration::minutes(PLAN_LIFETIME_MINUTES),
        approved_at: None,
        acknowledgements: None,
        execution_available: false,
        blocked_execution_gates: vec![
            "Process-tree cancellation is not implemented and proven on Windows.".to_owned(),
            "Pre-install residual-state inventory is not implemented.".to_owned(),
            "Automatic rollback coverage has not been tested for the selected package.".to_owned(),
            "Installer elevation and agreement handling remain explicit human boundaries.".to_owned(),
        ],
        rollback,
    })
}

fn validate_resolution(resolution: &WingetResolutionReport) -> Result<(), String> {
    if resolution.status != ResolutionStatus::ResolvedExact {
        return Err("an exact successful WinGet resolution is required before planning".to_owned());
    }
    let evidence = resolution
        .identity_evidence
        .as_ref()
        .ok_or_else(|| "package resolution has no provider process evidence".to_owned())?;
    if evidence.exit_code != Some(0) || evidence.timed_out {
        return Err("package identity probe did not complete successfully".to_owned());
    }
    Ok(())
}

fn hash_command(command: &CommandPreview) -> Result<String, String> {
    let canonical = serde_json::to_vec(command).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

fn short_id(id: Uuid) -> String {
    id.simple().to_string()[..8].to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolos_winget::{
        identity_probe, install_preview, uninstall_preview, PackageScope, PackageSelector,
        ProcessEvidence,
    };

    fn resolution() -> WingetResolutionReport {
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        };
        WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1.10.0".to_owned()),
            status: ResolutionStatus::ResolvedExact,
            selector: selector.clone(),
            identity_probe: identity_probe(&selector),
            identity_evidence: Some(ProcessEvidence {
                executable: "winget".to_owned(),
                args: vec!["show".to_owned()],
                exit_code: Some(0),
                stdout: "Found Git".to_owned(),
                stderr: String::new(),
                timed_out: false,
                duration_ms: 10,
            }),
            install_preview: install_preview(&selector),
            uninstall_preview: uninstall_preview(&selector),
            observed_at: Utc::now(),
            limitations: Vec::new(),
            single_safest_next_action: "Review".to_owned(),
        }
    }

    fn acknowledgements() -> ApprovalAcknowledgements {
        ApprovalAcknowledgements {
            reviewed_exact_identity: true,
            accepts_declared_write_scope: true,
            understands_no_automatic_rollback: true,
        }
    }

    #[test]
    fn plan_is_time_limited_and_execution_remains_blocked() {
        let now = Utc::now();
        let plan = create_install_plan(Uuid::new_v4(), resolution(), now).expect("plan");
        assert_eq!(plan.status, ActionStatus::WaitingApproval);
        assert_eq!(plan.expires_at, now + Duration::minutes(15));
        assert!(!plan.execution_available);
        assert_eq!(plan.command_sha256.len(), 64);
        assert!(plan.confirmation_phrase.contains("APPROVE INSTALL Git.Git"));
        assert!(!plan.blocked_execution_gates.is_empty());
    }

    #[test]
    fn approval_requires_exact_phrase_and_all_acknowledgements() {
        let now = Utc::now();
        let plan = create_install_plan(Uuid::new_v4(), resolution(), now).expect("plan");
        let wrong = ApprovalRequest {
            plan_id: plan.id,
            confirmation_phrase: "wrong".to_owned(),
            acknowledgements: acknowledgements(),
        };
        assert!(approve_plan(plan.clone(), &wrong, now).is_err());

        let incomplete = ApprovalRequest {
            plan_id: plan.id,
            confirmation_phrase: plan.confirmation_phrase.clone(),
            acknowledgements: ApprovalAcknowledgements {
                reviewed_exact_identity: true,
                accepts_declared_write_scope: false,
                understands_no_automatic_rollback: true,
            },
        };
        assert!(approve_plan(plan.clone(), &incomplete, now).is_err());

        let valid = ApprovalRequest {
            plan_id: plan.id,
            confirmation_phrase: plan.confirmation_phrase.clone(),
            acknowledgements: acknowledgements(),
        };
        let approved = approve_plan(plan, &valid, now).expect("approval");
        assert_eq!(
            approved.status,
            ActionStatus::ApprovedAwaitingExecutor
        );
        assert!(!approved.execution_available);
    }

    #[test]
    fn expired_plan_cannot_be_approved() {
        let now = Utc::now();
        let plan = create_install_plan(Uuid::new_v4(), resolution(), now).expect("plan");
        let request = ApprovalRequest {
            plan_id: plan.id,
            confirmation_phrase: plan.confirmation_phrase.clone(),
            acknowledgements: acknowledgements(),
        };
        assert!(approve_plan(plan, &request, now + Duration::minutes(16)).is_err());
    }

    #[test]
    fn unresolved_package_cannot_create_plan() {
        let mut unresolved = resolution();
        unresolved.status = ResolutionStatus::Blocked;
        assert!(create_install_plan(Uuid::new_v4(), unresolved, Utc::now()).is_err());
    }
}
