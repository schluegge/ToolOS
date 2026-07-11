use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
            "Queries WinGet registration data for an installed package matching the exact ID.".to_owned(),
            "May refresh or contact the configured package source while resolving source metadata.".to_owned(),
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
