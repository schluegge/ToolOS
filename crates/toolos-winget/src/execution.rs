use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    install_preview, normalize_selector, CommandPreview, PackageScope, PackageSelector,
    ProcessEvidence, WingetInstalledStateReport, WingetResolutionReport,
};

const PLAN_HASH_LENGTH: usize = 64;
const CONFIRMATION_HASH_PREFIX_LENGTH: usize = 12;
const FORBIDDEN_INSTALL_ARGUMENTS: &[&str] = &[
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
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallExecutionRequest {
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub selector: PackageSelector,
    pub expected_command: CommandPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WingetExecutionStatus {
    ProviderSucceededPostStateUnverified,
    ProviderFailed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetInstallExecutionReport {
    pub execution_id: Uuid,
    pub plan_id: Uuid,
    pub approval_id: Uuid,
    pub plan_hash: String,
    pub status: WingetExecutionStatus,
    pub selector: PackageSelector,
    pub command: CommandPreview,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub process_evidence: ProcessEvidence,
    pub preflight_resolution: WingetResolutionReport,
    pub preflight_installed_state: WingetInstalledStateReport,
    pub post_install_state: Option<WingetInstalledStateReport>,
    pub execution_attempted: bool,
    pub verification_claim: String,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

pub fn execution_confirmation(package_id: &str, plan_hash: &str) -> Result<String, String> {
    let package_id = package_id.trim();
    if package_id.is_empty() {
        return Err("package ID is empty".to_owned());
    }
    let prefix = plan_hash
        .get(..CONFIRMATION_HASH_PREFIX_LENGTH)
        .ok_or_else(|| "plan hash is too short for an execution confirmation".to_owned())?;
    Ok(format!("EXECUTE INSTALL {package_id} {prefix}"))
}

pub fn validate_execution_request(
    request: &WingetInstallExecutionRequest,
) -> Result<CommandPreview, String> {
    if request.plan_hash.len() != PLAN_HASH_LENGTH
        || !request
            .plan_hash
            .chars()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err(
            "execution requires one lowercase or uppercase 64-character SHA-256 plan hash"
                .to_owned(),
        );
    }

    let normalized = normalize_selector(request.selector.clone())?;
    if normalized != request.selector {
        return Err("execution selector is not normalized".to_owned());
    }
    if request.selector.scope != Some(PackageScope::User) {
        return Err(
            "the first executable WinGet slice is restricted to explicit user scope".to_owned(),
        );
    }
    if request.selector.version.is_none() {
        return Err("execution requires an explicitly pinned package version".to_owned());
    }
    if request.selector.architecture.is_none() {
        return Err("execution requires an explicitly pinned architecture".to_owned());
    }

    let derived = install_preview(&request.selector);
    if request.expected_command != derived {
        return Err(
            "expected command does not equal the command derived from the selector".to_owned(),
        );
    }
    if request.expected_command.execution_enabled {
        return Err("stored command previews must remain execution-disabled artifacts".to_owned());
    }
    if request
        .expected_command
        .args
        .iter()
        .any(|argument| FORBIDDEN_INSTALL_ARGUMENTS.contains(&argument.as_str()))
    {
        return Err("install command contains a forbidden high-risk argument".to_owned());
    }

    for required in [
        "install",
        "--id",
        "--exact",
        "--source",
        "--version",
        "--scope",
        "--architecture",
        "--no-upgrade",
        "--disable-interactivity",
    ] {
        if !request
            .expected_command
            .args
            .iter()
            .any(|argument| argument == required)
        {
            return Err(format!(
                "install command is missing required argument {required}"
            ));
        }
    }

    Ok(derived)
}

#[allow(clippy::too_many_arguments)]
pub fn build_execution_report(
    execution_id: Uuid,
    plan_id: Uuid,
    approval_id: Uuid,
    plan_hash: String,
    selector: PackageSelector,
    command: CommandPreview,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
    process_evidence: ProcessEvidence,
    preflight_resolution: WingetResolutionReport,
    preflight_installed_state: WingetInstalledStateReport,
    post_install_state: Option<WingetInstalledStateReport>,
) -> WingetInstallExecutionReport {
    let status = if process_evidence.timed_out {
        WingetExecutionStatus::TimedOut
    } else if process_evidence.exit_code == Some(0) {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified
    } else {
        WingetExecutionStatus::ProviderFailed
    };
    let (verification_claim, single_safest_next_action) = match status {
        WingetExecutionStatus::ProviderSucceededPostStateUnverified => (
            "WinGet returned exit code zero; ToolOS did not parse a definitive installed-package verdict."
                .to_owned(),
            "Review the post-install WinGet evidence and run an application-specific healthcheck before relying on the package."
                .to_owned(),
        ),
        WingetExecutionStatus::ProviderFailed => (
            "WinGet did not return a successful exit code; installation success is not claimed."
                .to_owned(),
            "Review the bounded provider output. Create a fresh plan only after resolving the reported blocker."
                .to_owned(),
        ),
        WingetExecutionStatus::TimedOut => (
            "The WinGet invocation exceeded the bounded execution window; final installer state is unknown."
                .to_owned(),
            "Inspect WinGet logs and running installer processes before creating another plan."
                .to_owned(),
        ),
    };

    WingetInstallExecutionReport {
        execution_id,
        plan_id,
        approval_id,
        plan_hash,
        status,
        selector,
        command,
        started_at,
        completed_at,
        process_evidence,
        preflight_resolution,
        preflight_installed_state,
        post_install_state,
        execution_attempted: true,
        verification_claim,
        limitations: vec![
            "ToolOS executes only an exact, version-pinned, architecture-pinned, user-scope WinGet command derived from the approved plan."
                .to_owned(),
            "Package and source agreements are never accepted automatically; packages requiring an agreement fail closed in this slice."
                .to_owned(),
            "No force, hash bypass, dependency skip, custom installer arguments, override, manifest, header, or reboot allowance is added."
                .to_owned(),
            "WinGet output and exit status are evidence, not a universal application healthcheck."
                .to_owned(),
            "On timeout, ToolOS cannot prove that every installer child process terminated; inspect the machine before retrying."
                .to_owned(),
        ],
        single_safest_next_action,
    }
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
    fn exact_user_scope_request_is_derived_not_trusted() {
        let selector = selector();
        let request = WingetInstallExecutionRequest {
            plan_id: Uuid::nil(),
            plan_hash: "a".repeat(64),
            expected_command: install_preview(&selector),
            selector,
        };
        let command = validate_execution_request(&request).expect("valid execution request");
        assert!(command
            .args
            .iter()
            .any(|argument| argument == "--no-upgrade"));
        assert!(!command.execution_enabled);
    }

    #[test]
    fn execution_rejects_unpinned_or_machine_scope_selectors() {
        let mut unpinned = selector();
        unpinned.version = None;
        let request = WingetInstallExecutionRequest {
            plan_id: Uuid::nil(),
            plan_hash: "a".repeat(64),
            expected_command: install_preview(&unpinned),
            selector: unpinned,
        };
        assert!(validate_execution_request(&request).is_err());

        let mut machine = selector();
        machine.scope = Some(PackageScope::Machine);
        let request = WingetInstallExecutionRequest {
            plan_id: Uuid::nil(),
            plan_hash: "a".repeat(64),
            expected_command: install_preview(&machine),
            selector: machine,
        };
        assert!(validate_execution_request(&request).is_err());
    }

    #[test]
    fn execution_confirmation_is_plan_bound() {
        assert_eq!(
            execution_confirmation("Git.Git", &"a".repeat(64)).expect("confirmation"),
            "EXECUTE INSTALL Git.Git aaaaaaaaaaaa"
        );
    }
}
