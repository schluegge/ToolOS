use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod execution;
mod recovery;
pub use execution::*;
pub use recovery::*;

const MAX_SELECTOR_LENGTH: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageScope {
    User,
    Machine,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PackageSelector {
    pub package_id: String,
    pub source: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub scope: Option<PackageScope>,
    #[serde(default)]
    pub architecture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandPreview {
    pub executable: String,
    pub args: Vec<String>,
    pub powershell: String,
    pub working_directory: Option<String>,
    pub environment_changes: Vec<String>,
    pub blast_radius: String,
    pub execution_enabled: bool,
    pub expected_side_effects: Vec<String>,
    pub approval_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProcessEvidence {
    pub executable: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResolutionStatus {
    ResolvedExact,
    Blocked,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetResolutionReport {
    pub provider_id: String,
    pub provider_version: Option<String>,
    pub status: ResolutionStatus,
    pub selector: PackageSelector,
    pub identity_probe: CommandPreview,
    pub identity_evidence: Option<ProcessEvidence>,
    pub install_preview: CommandPreview,
    pub uninstall_preview: CommandPreview,
    pub observed_at: DateTime<Utc>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstalledQueryStatus {
    QueryCompleted,
    Blocked,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstalledStateReport {
    pub provider_id: String,
    pub provider_version: Option<String>,
    pub status: InstalledQueryStatus,
    pub selector: PackageSelector,
    pub installed_probe: CommandPreview,
    pub installed_evidence: Option<ProcessEvidence>,
    pub observed_at: DateTime<Utc>,
    pub definitive_installed_match: Option<bool>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallPlanStatus {
    AwaitingApproval,
    Blocked,
    ApprovedExecutionDisabled,
    ApprovedAwaitingExecution,
    Executing,
    ExecutionSucceededUnverified,
    ExecutionFailed,
    ExecutionTimedOut,
    ExecutionCancelled,
    UnknownRequiresRecovery,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalChallenge {
    pub required_phrase: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallPlan {
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub status: InstallPlanStatus,
    pub selector: PackageSelector,
    pub resolution: WingetResolutionReport,
    pub installed_state: WingetInstalledStateReport,
    pub install_preview: CommandPreview,
    pub approval_allowed: bool,
    pub approval_challenge: Option<ApprovalChallenge>,
    pub lock_key: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub execution_enabled: bool,
    pub blockers: Vec<String>,
    pub pre_execution_requirements: Vec<String>,
    pub verification: Vec<String>,
    pub rollback: Vec<String>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallApprovalReceipt {
    pub approval_id: Uuid,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub package_id: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub lock_key: String,
    pub lock_expires_at: DateTime<Utc>,
    pub status: InstallPlanStatus,
    pub execution_enabled: bool,
    pub execution_confirmation: String,
    pub limitations: Vec<String>,
}

pub fn build_install_plan(
    resolution: WingetResolutionReport,
    installed_state: WingetInstalledStateReport,
    now: DateTime<Utc>,
    ttl_seconds: u64,
) -> Result<WingetInstallPlan, String> {
    if resolution.selector != installed_state.selector {
        return Err("resolution and installed-state selectors differ".to_owned());
    }

    let ttl_seconds = ttl_seconds.clamp(60, 900);
    let expires_at = now + Duration::seconds(i64::try_from(ttl_seconds).unwrap_or(900));
    let plan_id = Uuid::new_v4();
    let lock_key = "package-manager:winget".to_owned();
    let install_preview = resolution.install_preview.clone();
    let mut blockers = Vec::new();

    if resolution.status != ResolutionStatus::ResolvedExact {
        blockers.push("Exact WinGet package resolution did not succeed.".to_owned());
    }
    if installed_state.status != InstalledQueryStatus::QueryCompleted {
        blockers.push("The installed-state query did not complete successfully.".to_owned());
    }
    if resolution.provider_version.is_none() {
        blockers.push("The WinGet provider version was not observed.".to_owned());
    }
    if resolution.selector.version.is_none() {
        blockers.push("Execution requires an explicit package version pin.".to_owned());
    }
    if resolution.selector.scope != Some(PackageScope::User) {
        blockers.push("The first execution slice requires explicit user scope.".to_owned());
    }
    if resolution.selector.architecture.is_none() {
        blockers.push("Execution requires an explicit architecture pin.".to_owned());
    }
    if installed_state.definitive_installed_match == Some(true) {
        blockers.push(
            "The exact package is already installed according to definitive evidence.".to_owned(),
        );
    }

    let status = if blockers.is_empty() {
        InstallPlanStatus::AwaitingApproval
    } else {
        InstallPlanStatus::Blocked
    };
    let approval_allowed = blockers.is_empty();
    let plan_hash = plan_hash(
        plan_id,
        &resolution.selector,
        &resolution,
        &installed_state,
        &install_preview,
        now,
        expires_at,
    )?;
    let approval_challenge = if approval_allowed {
        Some(ApprovalChallenge {
            required_phrase: approval_phrase(&resolution.selector.package_id, &plan_hash)?,
            expires_at,
        })
    } else {
        None
    };

    Ok(WingetInstallPlan {
        plan_id,
        plan_hash,
        status,
        selector: resolution.selector.clone(),
        resolution,
        installed_state,
        install_preview,
        approval_allowed,
        approval_challenge,
        lock_key,
        created_at: now,
        expires_at,
        execution_enabled: false,
        blockers,
        pre_execution_requirements: vec![
            "Review the exact command, raw identity and installed-state evidence, then enter the approval phrase before the plan expires. Approval arms only a separate receipt-bound execution step; it does not itself invoke the installer."
                .to_owned(),
            "Keep package ID, source, version, user scope, architecture, plan ID and plan hash unchanged."
                .to_owned(),
            "Do not proceed if agreement prompts, elevation, force, overrides, custom installer arguments, hash bypass, dependency skipping, reboot allowance, or machine scope are required."
                .to_owned(),
        ],
        verification: vec![
            "Re-run the exact read-only installed-state query after execution.".to_owned(),
            "Treat WinGet exit code zero as provider success only; run an application-specific healthcheck before relying on the installed tool."
                .to_owned(),
        ],
        rollback: vec![
            "Use the separately previewed exact uninstall command only after a new governed plan and approval flow exists."
                .to_owned(),
            "Inspect PATH, files, services, registry entries, running processes and WinGet logs for residuals; this plan does not guarantee complete rollback."
                .to_owned(),
        ],
        limitations: vec![
            "The plan is valid only until its expiry and only for the captured provider identity and selector."
                .to_owned(),
            "Approval changes only ToolOS metadata and a local lock; a second receipt-bound execution phrase is required before machine mutation."
                .to_owned(),
            "The local lock coordinates ToolOS only and cannot prevent external package-manager processes."
                .to_owned(),
            "Installed-state output is preserved but not parsed into a locale-independent package verdict."
                .to_owned(),
        ],
        single_safest_next_action: if approval_allowed {
            "Review the exact evidence and enter the displayed approval phrase before expiry."
                .to_owned()
        } else {
            "Resolve every blocker, then create a new plan from fresh WinGet evidence.".to_owned()
        },
    })
}

pub fn build_approval_receipt(
    plan: &WingetInstallPlan,
    confirmation: &str,
    now: DateTime<Utc>,
    ttl_seconds: u64,
) -> Result<WingetInstallApprovalReceipt, String> {
    if plan.status != InstallPlanStatus::AwaitingApproval || !plan.approval_allowed {
        return Err("install plan is not awaiting approval".to_owned());
    }
    if now >= plan.expires_at {
        return Err("install plan expired".to_owned());
    }
    let challenge = plan
        .approval_challenge
        .as_ref()
        .ok_or_else(|| "install plan has no approval challenge".to_owned())?;
    if confirmation.trim() != challenge.required_phrase {
        return Err("approval phrase does not match the immutable plan".to_owned());
    }
    let ttl_seconds = ttl_seconds.clamp(60, 600);
    let expires_at =
        (now + Duration::seconds(i64::try_from(ttl_seconds).unwrap_or(300))).min(plan.expires_at);
    let lock_expires_at = expires_at;
    Ok(WingetInstallApprovalReceipt {
        approval_id: Uuid::new_v4(),
        plan_id: plan.plan_id,
        plan_hash: plan.plan_hash.clone(),
        package_id: plan.selector.package_id.clone(),
        approved_at: now,
        expires_at,
        lock_key: plan.lock_key.clone(),
        lock_expires_at,
        status: InstallPlanStatus::ApprovedAwaitingExecution,
        execution_enabled: true,
        execution_confirmation: execution_confirmation(&plan.selector.package_id, &plan.plan_hash)?,
        limitations: vec![
            "This receipt is short-lived and bound to one immutable plan hash.".to_owned(),
            "Execution remains restricted to the exact version-pinned, architecture-pinned, user-scope command derived from the plan."
                .to_owned(),
            "Package and source agreements are not accepted automatically.".to_owned(),
            "The local lock cannot block WinGet or installers launched outside ToolOS.".to_owned(),
        ],
    })
}

fn plan_hash(
    plan_id: Uuid,
    selector: &PackageSelector,
    resolution: &WingetResolutionReport,
    installed_state: &WingetInstalledStateReport,
    install_preview: &CommandPreview,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct HashPayload<'a> {
        plan_id: Uuid,
        selector: &'a PackageSelector,
        resolution: &'a WingetResolutionReport,
        installed_state: &'a WingetInstalledStateReport,
        install_preview: &'a CommandPreview,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        execution_enabled: bool,
        lock_key: &'static str,
    }
    let payload = HashPayload {
        plan_id,
        selector,
        resolution,
        installed_state,
        install_preview,
        created_at,
        expires_at,
        execution_enabled: false,
        lock_key: "package-manager:winget",
    };
    let bytes = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

fn approval_phrase(package_id: &str, plan_hash: &str) -> Result<String, String> {
    let prefix = plan_hash
        .get(..12)
        .ok_or_else(|| "plan hash is too short for approval phrase".to_owned())?;
    Ok(format!("APPROVE INSTALL {package_id} {prefix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector() -> PackageSelector {
        PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        }
    }

    fn preview() -> CommandPreview {
        install_preview(&selector())
    }

    fn process(exit_code: i32) -> ProcessEvidence {
        ProcessEvidence {
            executable: "winget".to_owned(),
            args: vec!["show".to_owned()],
            exit_code: Some(exit_code),
            stdout: "provider output".to_owned(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 10,
        }
    }

    fn reports() -> (WingetResolutionReport, WingetInstalledStateReport) {
        let selector = selector();
        let resolution = WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1.9.0".to_owned()),
            status: ResolutionStatus::ResolvedExact,
            selector: selector.clone(),
            identity_probe: identity_probe(&selector),
            identity_evidence: Some(process(0)),
            install_preview: preview(),
            uninstall_preview: uninstall_preview(&selector),
            observed_at: Utc::now(),
            limitations: Vec::new(),
            single_safest_next_action: "inspect".to_owned(),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1.9.0".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector,
            installed_probe: installed_probe(&selector()),
            installed_evidence: Some(process(0)),
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: Vec::new(),
            single_safest_next_action: "review".to_owned(),
        };
        (resolution, installed)
    }

    #[test]
    fn selector_normalization_rejects_shell_metacharacters() {
        let invalid = PackageSelector {
            package_id: "Git.Git; Remove-Item C:\\".to_owned(),
            ..selector()
        };
        assert!(normalize_selector(invalid).is_err());
    }

    #[test]
    fn install_preview_is_locked_and_execution_disabled() {
        let preview = install_preview(&selector());
        assert_eq!(preview.executable, "winget");
        assert!(preview
            .args
            .windows(2)
            .any(|pair| pair == ["--id", "Git.Git"]));
        assert!(preview.args.iter().any(|argument| argument == "--exact"));
        assert!(preview
            .args
            .windows(2)
            .any(|pair| pair == ["--source", "winget"]));
        assert!(preview
            .args
            .iter()
            .any(|argument| argument == "--disable-interactivity"));
        assert!(preview
            .args
            .iter()
            .any(|argument| argument == "--no-upgrade"));
        assert!(!preview.execution_enabled);
    }

    #[test]
    fn install_preview_never_adds_high_risk_flags() {
        let preview = install_preview(&selector());
        for forbidden in [
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--allow-reboot",
            "--custom",
            "--force",
            "--header",
            "--ignore-local-archive-malware-scan",
            "--ignore-security-hash",
            "--manifest",
            "--override",
            "--skip-dependencies",
        ] {
            assert!(!preview.args.iter().any(|argument| argument == forbidden));
        }
    }

    #[test]
    fn install_plan_is_hash_bound_and_approval_does_not_execute() {
        let (resolution, installed) = reports();
        let now = Utc::now();
        let plan = build_install_plan(resolution, installed, now, 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::AwaitingApproval);
        assert_eq!(plan.plan_hash.len(), 64);
        assert!(!plan.execution_enabled);
        let challenge = plan
            .approval_challenge
            .as_ref()
            .expect("challenge")
            .required_phrase
            .clone();
        let receipt = build_approval_receipt(&plan, &challenge, now, 300).expect("approval");
        assert_eq!(receipt.plan_hash, plan.plan_hash);
        assert_eq!(receipt.status, InstallPlanStatus::ApprovedAwaitingExecution);
        assert!(receipt.execution_enabled);
    }

    #[test]
    fn approval_rejects_wrong_phrase_and_expired_plan() {
        let (resolution, installed) = reports();
        let now = Utc::now();
        let plan = build_install_plan(resolution, installed, now, 60).expect("plan");
        assert!(build_approval_receipt(&plan, "wrong", now, 300).is_err());
        let phrase = plan
            .approval_challenge
            .as_ref()
            .expect("challenge")
            .required_phrase
            .clone();
        assert!(build_approval_receipt(&plan, &phrase, plan.expires_at, 300).is_err());
    }

    #[test]
    fn machine_scope_and_unpinned_plans_are_blocked() {
        let (mut resolution, mut installed) = reports();
        resolution.selector.scope = Some(PackageScope::Machine);
        installed.selector.scope = Some(PackageScope::Machine);
        resolution.install_preview = install_preview(&resolution.selector);
        installed.installed_probe = installed_probe(&installed.selector);
        let plan = build_install_plan(resolution, installed, Utc::now(), 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert!(!plan.approval_allowed);

        let (mut resolution, mut installed) = reports();
        resolution.selector.version = None;
        installed.selector.version = None;
        resolution.install_preview = install_preview(&resolution.selector);
        installed.installed_probe = installed_probe(&installed.selector);
        let plan = build_install_plan(resolution, installed, Utc::now(), 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert!(!plan.approval_allowed);
    }
}
