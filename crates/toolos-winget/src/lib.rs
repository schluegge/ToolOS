use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod containment;
pub use containment::*;
mod execution;
pub use execution::*;

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
    if installed_state.definitive_installed_match == Some(true) {
        blockers.push(
            "The exact package is already installed according to definitive evidence.".to_owned(),
        );
    }

    let fingerprint = serde_json::to_vec(&(
        plan_id,
        &resolution,
        &installed_state,
        &install_preview,
        now,
        expires_at,
        &lock_key,
    ))
    .map_err(|error| format!("cannot serialize install-plan fingerprint: {error}"))?;
    let plan_hash = format!("{:x}", Sha256::digest(fingerprint));
    let approval_allowed = blockers.is_empty();
    let approval_challenge = approval_allowed.then(|| ApprovalChallenge {
        required_phrase: format!(
            "APPROVE INSTALL {} {}",
            resolution.selector.package_id,
            &plan_hash[..12]
        ),
        expires_at,
    });
    let status = if approval_allowed {
        InstallPlanStatus::AwaitingApproval
    } else {
        InstallPlanStatus::Blocked
    };
    let single_safest_next_action = if approval_allowed {
        "Review the exact command, raw identity and installed-state evidence, then enter the approval phrase before the plan expires. Approval arms only a separate receipt-bound execution step; it does not itself invoke the installer."
            .to_owned()
    } else {
        "Resolve every blocker and create a new plan; blocked plans cannot be approved.".to_owned()
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
            "Re-run exact package resolution immediately before any future execution.".to_owned(),
            "Re-run installed-state evidence immediately before any future execution.".to_owned(),
            "Review package and source agreements separately; approval does not accept them.".to_owned(),
            "Acquire the package-manager lock and retain it through verification.".to_owned(),
            "Request elevation separately if machine scope or the installer requires it.".to_owned(),
        ],
        verification: vec![
            "Run the exact installed-state query after installation.".to_owned(),
            "Run a provider-appropriate executable or application healthcheck when one is defined."
                .to_owned(),
            "Persist command, exit code, bounded output, duration, and post-state evidence."
                .to_owned(),
        ],
        rollback: vec![
            "Use the exact uninstall preview only after a separate destructive-action approval."
                .to_owned(),
            "Do not claim full rollback: installer-created files, services, settings, and user data may remain."
                .to_owned(),
        ],
        limitations: vec![
            "WinGet has no documented true no-side-effect install dry-run in the command surface used here; ToolOS dry run means plan generation without invoking `winget install`."
                .to_owned(),
            "Localized WinGet list output is retained as evidence and is not parsed into a definitive installed verdict."
                .to_owned(),
            "The generated command contains no agreement acceptance, hash bypass, dependency skip, force, override, or custom installer arguments."
                .to_owned(),
            "Approval changes only ToolOS metadata and a local lock; a second receipt-bound execution phrase is required before machine mutation."
                .to_owned(),
        ],
        single_safest_next_action,
    })
}

pub fn build_approval_receipt(
    plan: &WingetInstallPlan,
    confirmation: &str,
    now: DateTime<Utc>,
    lock_ttl_seconds: u64,
) -> Result<WingetInstallApprovalReceipt, String> {
    if plan.status != InstallPlanStatus::AwaitingApproval || !plan.approval_allowed {
        return Err("install plan is not awaiting approval".to_owned());
    }
    let challenge = plan
        .approval_challenge
        .as_ref()
        .ok_or_else(|| "install plan has no approval challenge".to_owned())?;
    if now >= challenge.expires_at || now >= plan.expires_at {
        return Err("install plan approval window has expired".to_owned());
    }
    if confirmation.trim() != challenge.required_phrase {
        return Err("approval phrase does not match the immutable install plan".to_owned());
    }

    let lock_ttl_seconds = lock_ttl_seconds.clamp(30, 300);
    let requested_expiry = now + Duration::seconds(i64::try_from(lock_ttl_seconds).unwrap_or(300));
    let expires_at = std::cmp::min(plan.expires_at, requested_expiry);

    Ok(WingetInstallApprovalReceipt {
        approval_id: Uuid::new_v4(),
        plan_id: plan.plan_id,
        plan_hash: plan.plan_hash.clone(),
        package_id: plan.selector.package_id.clone(),
        approved_at: now,
        expires_at,
        lock_key: plan.lock_key.clone(),
        lock_expires_at: expires_at,
        status: InstallPlanStatus::ApprovedAwaitingExecution,
        execution_enabled: true,
        execution_confirmation: execution_confirmation(
            &plan.selector.package_id,
            &plan.plan_hash,
        )?,
        limitations: vec![
            "This receipt authorizes only the immutable plan hash during its short validity window."
                .to_owned(),
            "The package-manager lock is local to ToolOS and cannot prevent external WinGet processes."
                .to_owned(),
            "No package or source agreement was accepted and no installer was executed.".to_owned(),
        ],
    })
}

pub fn normalize_selector(selector: PackageSelector) -> Result<PackageSelector, String> {
    Ok(PackageSelector {
        package_id: validate_token("package_id", selector.package_id)?,
        source: validate_token("source", selector.source)?,
        version: selector
            .version
            .map(|value| validate_token("version", value))
            .transpose()?,
        scope: selector.scope,
        architecture: selector
            .architecture
            .map(|value| validate_token("architecture", value))
            .transpose()?,
    })
}

pub fn identity_probe(selector: &PackageSelector) -> CommandPreview {
    let mut args = vec![
        "show".to_owned(),
        "--id".to_owned(),
        selector.package_id.clone(),
        "--exact".to_owned(),
        "--source".to_owned(),
        selector.source.clone(),
        "--disable-interactivity".to_owned(),
    ];
    append_show_filters(&mut args, selector);
    preview(
        args,
        "READ_ONLY",
        vec![
            "Queries the selected WinGet source for one exact package identity.".to_owned(),
            "May contact the configured package source over the network.".to_owned(),
        ],
        Vec::new(),
    )
}

pub fn installed_probe(selector: &PackageSelector) -> CommandPreview {
    let mut args = vec![
        "list".to_owned(),
        "--id".to_owned(),
        selector.package_id.clone(),
        "--exact".to_owned(),
        "--source".to_owned(),
        selector.source.clone(),
        "--disable-interactivity".to_owned(),
    ];
    if let Some(scope) = &selector.scope {
        args.push("--scope".to_owned());
        args.push(scope_text(scope).to_owned());
    }
    preview(
        args,
        "READ_ONLY",
        vec![
            "Queries WinGet registration data for an installed package matching the exact ID."
                .to_owned(),
            "May refresh or contact the configured package source while resolving source metadata."
                .to_owned(),
        ],
        Vec::new(),
    )
}

pub fn install_preview(selector: &PackageSelector) -> CommandPreview {
    let mut args = vec![
        "install".to_owned(),
        "--id".to_owned(),
        selector.package_id.clone(),
        "--exact".to_owned(),
        "--source".to_owned(),
        selector.source.clone(),
        "--no-upgrade".to_owned(),
        "--disable-interactivity".to_owned(),
    ];
    append_install_filters(&mut args, selector);
    preview(
        args,
        if selector.scope == Some(PackageScope::Machine) {
            "MACHINE_WRITE"
        } else {
            "USER_PROFILE_WRITE"
        },
        vec![
            "Would download and run the package installer selected by WinGet.".to_owned(),
            "Could add files, applications, services, PATH entries, registry values, or shortcuts according to the package installer.".to_owned(),
        ],
        vec![
            "Explicit approval of the resolved package identity and command.".to_owned(),
            "Separate review of package and source agreements when WinGet requires them.".to_owned(),
            "Elevation approval when the installer or machine scope requires it.".to_owned(),
        ],
    )
}

pub fn uninstall_preview(selector: &PackageSelector) -> CommandPreview {
    let mut args = vec![
        "uninstall".to_owned(),
        "--id".to_owned(),
        selector.package_id.clone(),
        "--exact".to_owned(),
        "--source".to_owned(),
        selector.source.clone(),
        "--disable-interactivity".to_owned(),
    ];
    if let Some(version) = &selector.version {
        args.push("--version".to_owned());
        args.push(version.clone());
    }
    if let Some(scope) = &selector.scope {
        args.push("--scope".to_owned());
        args.push(scope_text(scope).to_owned());
    }
    preview(
        args,
        if selector.scope == Some(PackageScope::Machine) {
            "MACHINE_WRITE"
        } else {
            "USER_PROFILE_WRITE"
        },
        vec![
            "Would run the installed package's uninstall command through WinGet.".to_owned(),
            "Residual configuration, caches, services, and user data may remain unless separately inventoried.".to_owned(),
        ],
        vec![
            "Explicit approval of the installed package identity and command.".to_owned(),
            "A residual-state manifest and recovery warning before execution.".to_owned(),
            "Elevation approval when the uninstaller or machine scope requires it.".to_owned(),
        ],
    )
}

pub fn version_probe() -> CommandPreview {
    preview(
        vec!["--version".to_owned()],
        "READ_ONLY",
        vec!["Reads the WinGet client version.".to_owned()],
        Vec::new(),
    )
}

fn append_show_filters(args: &mut Vec<String>, selector: &PackageSelector) {
    if let Some(version) = &selector.version {
        args.push("--version".to_owned());
        args.push(version.clone());
    }
    if let Some(scope) = &selector.scope {
        args.push("--scope".to_owned());
        args.push(scope_text(scope).to_owned());
    }
    if let Some(architecture) = &selector.architecture {
        args.push("--architecture".to_owned());
        args.push(architecture.clone());
    }
}

fn append_install_filters(args: &mut Vec<String>, selector: &PackageSelector) {
    if let Some(version) = &selector.version {
        args.push("--version".to_owned());
        args.push(version.clone());
    }
    if let Some(scope) = &selector.scope {
        args.push("--scope".to_owned());
        args.push(scope_text(scope).to_owned());
    }
    if let Some(architecture) = &selector.architecture {
        args.push("--architecture".to_owned());
        args.push(architecture.clone());
    }
}

fn scope_text(scope: &PackageScope) -> &'static str {
    match scope {
        PackageScope::User => "user",
        PackageScope::Machine => "machine",
    }
}

fn preview(
    args: Vec<String>,
    blast_radius: &str,
    expected_side_effects: Vec<String>,
    approval_requirements: Vec<String>,
) -> CommandPreview {
    CommandPreview {
        powershell: render_powershell("winget", &args),
        executable: "winget".to_owned(),
        args,
        working_directory: None,
        environment_changes: Vec::new(),
        blast_radius: blast_radius.to_owned(),
        execution_enabled: false,
        expected_side_effects,
        approval_requirements,
    }
}

fn validate_token(field: &str, value: String) -> Result<String, String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > MAX_SELECTOR_LENGTH {
        return Err(format!("{field} exceeds {MAX_SELECTOR_LENGTH} bytes"));
    }
    if value.starts_with('-') {
        return Err(format!("{field} must not begin with '-'"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} contains a control character"));
    }
    Ok(value)
}

fn render_powershell(executable: &str, args: &[String]) -> String {
    std::iter::once(executable)
        .chain(args.iter().map(String::as_str))
        .map(quote_powershell)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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

    #[test]
    fn identity_probe_is_exact_and_non_interactive() {
        let command = identity_probe(&selector());
        assert_eq!(command.executable, "winget");
        assert_eq!(command.blast_radius, "READ_ONLY");
        assert!(!command.execution_enabled);
        assert_eq!(
            command.args,
            [
                "show",
                "--id",
                "Git.Git",
                "--exact",
                "--source",
                "winget",
                "--disable-interactivity",
                "--version",
                "2.50.1",
                "--scope",
                "user",
                "--architecture",
                "x64",
            ]
        );
    }

    #[test]
    fn installed_probe_uses_only_supported_identity_and_scope_filters() {
        let command = installed_probe(&selector());
        assert_eq!(command.blast_radius, "READ_ONLY");
        assert!(!command.execution_enabled);
        assert_eq!(
            command.args,
            [
                "list",
                "--id",
                "Git.Git",
                "--exact",
                "--source",
                "winget",
                "--disable-interactivity",
                "--scope",
                "user",
            ]
        );
        assert!(!command.args.iter().any(|arg| arg == "--architecture"));
        assert!(!command.args.iter().any(|arg| arg == "--version"));
    }

    #[test]
    fn governed_plan_binds_hash_phrase_and_expiry() {
        let resolution = WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: ResolutionStatus::ResolvedExact,
            selector: selector(),
            identity_probe: identity_probe(&selector()),
            identity_evidence: None,
            install_preview: install_preview(&selector()),
            uninstall_preview: uninstall_preview(&selector()),
            observed_at: Utc::now(),
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: Some("v1".to_owned()),
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector(),
            installed_probe: installed_probe(&selector()),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let now = Utc::now();
        let plan = build_install_plan(resolution, installed, now, 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::AwaitingApproval);
        assert_eq!(plan.plan_hash.len(), 64);
        assert!(!plan.execution_enabled);
        let phrase = plan
            .approval_challenge
            .as_ref()
            .expect("challenge")
            .required_phrase
            .clone();
        let receipt = build_approval_receipt(&plan, &phrase, now, 300).expect("receipt");
        assert_eq!(receipt.plan_hash, plan.plan_hash);
        assert_eq!(receipt.status, InstallPlanStatus::ApprovedAwaitingExecution);
        assert!(receipt.execution_enabled);
        assert!(build_approval_receipt(&plan, "wrong", now, 300).is_err());
    }

    #[test]
    fn governed_plan_blocks_unresolved_identity() {
        let resolution = WingetResolutionReport {
            provider_id: "winget".to_owned(),
            provider_version: None,
            status: ResolutionStatus::Blocked,
            selector: selector(),
            identity_probe: identity_probe(&selector()),
            identity_evidence: None,
            install_preview: install_preview(&selector()),
            uninstall_preview: uninstall_preview(&selector()),
            observed_at: Utc::now(),
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let installed = WingetInstalledStateReport {
            provider_id: "winget".to_owned(),
            provider_version: None,
            status: InstalledQueryStatus::QueryCompleted,
            selector: selector(),
            installed_probe: installed_probe(&selector()),
            installed_evidence: None,
            observed_at: Utc::now(),
            definitive_installed_match: None,
            limitations: vec![],
            single_safest_next_action: String::new(),
        };
        let plan = build_install_plan(resolution, installed, Utc::now(), 600).expect("plan");
        assert_eq!(plan.status, InstallPlanStatus::Blocked);
        assert!(!plan.approval_allowed);
        assert!(plan.approval_challenge.is_none());
    }

    #[test]
    fn install_preview_does_not_accept_agreements_or_bypass_hashes() {
        let command = install_preview(&selector());
        assert!(!command.execution_enabled);
        assert!(!command.args.iter().any(|arg| arg.contains("agreement")));
        assert!(!command.args.iter().any(|arg| arg == "--force"));
        assert!(!command
            .args
            .iter()
            .any(|arg| arg == "--ignore-security-hash"));
    }

    #[test]
    fn uninstall_preview_does_not_claim_architecture_filter() {
        let command = uninstall_preview(&selector());
        assert!(!command.args.iter().any(|arg| arg == "--architecture"));
        assert!(!command.execution_enabled);
    }

    #[test]
    fn selector_rejects_option_injection_and_control_characters() {
        let mut invalid = selector();
        invalid.package_id = "--help".to_owned();
        assert!(normalize_selector(invalid).is_err());

        let mut invalid = selector();
        invalid.source = "winget\nmalicious".to_owned();
        assert!(normalize_selector(invalid).is_err());
    }

    #[test]
    fn powershell_rendering_escapes_single_quotes() {
        assert_eq!(
            render_powershell("winget", &["show".to_owned(), "A'B".to_owned()]),
            "'winget' 'show' 'A''B'"
        );
    }
}
