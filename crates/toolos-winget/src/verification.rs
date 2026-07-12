use std::env;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PackageScope, PackageSelector, ProcessEvidence};

pub const OFFICIAL_PROVIDER_CONTRACT: &str = "toolos.microsoft-winget-client.installed/1";
pub const GIT_RECIPE_ID: &str = "toolos.recipe.git-git/1";
pub const GIT_VERSION_PARSER: &str = "GIT_VERSION_V1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationVerdict {
    Verified,
    NotVerified,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicationHealthVerdict {
    Healthy,
    Unhealthy,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OfficialProviderStatus {
    Available,
    ModuleUnavailable,
    UnsupportedContract,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OfficialVersionComparison {
    Unknown,
    Lesser,
    Equal,
    Greater,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct OfficialInstalledPackage {
    pub id: String,
    pub source: Option<String>,
    pub installed_version: String,
    pub version_comparison: OfficialVersionComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct OfficialProviderSnapshot {
    pub contract: String,
    pub module_version: Option<String>,
    pub status: OfficialProviderStatus,
    pub error: Option<String>,
    pub packages: Vec<OfficialInstalledPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct VerificationDimension {
    pub verdict: VerificationVerdict,
    pub expected: Option<String>,
    pub observed: Option<String>,
    pub evidence: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PackageRecipeEvidence {
    pub recipe_id: String,
    pub package_id: String,
    pub executable_path: Option<String>,
    pub version_parser: String,
    pub raw_version_output: Option<String>,
    pub parsed_version: Option<String>,
    pub architecture: Option<String>,
    pub scope: Option<PackageScope>,
    pub process_evidence: Option<ProcessEvidence>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApplicationHealthReport {
    pub verdict: ApplicationHealthVerdict,
    pub recipe_id: Option<String>,
    pub executable_path: Option<String>,
    pub parsed_version: Option<String>,
    pub process_evidence: Option<ProcessEvidence>,
    pub evidence: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct WingetVerificationReport {
    pub provider_id: String,
    pub provider_contract: String,
    pub provider_status: OfficialProviderStatus,
    pub provider_module_version: Option<String>,
    pub selector: PackageSelector,
    pub id: VerificationDimension,
    pub source: VerificationDimension,
    pub version: VerificationDimension,
    pub scope: VerificationDimension,
    pub architecture: VerificationDimension,
    pub package_identity: VerificationVerdict,
    pub application_health: ApplicationHealthReport,
    pub observed_at: DateTime<Utc>,
    pub limitations: Vec<String>,
    pub single_safest_next_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRoots {
    pub user_roots: Vec<PathBuf>,
    pub system_roots: Vec<PathBuf>,
}

impl ScopeRoots {
    #[must_use]
    pub fn from_environment() -> Self {
        Self {
            user_roots: environment_paths(["LOCALAPPDATA", "APPDATA", "USERPROFILE"]),
            system_roots: environment_paths([
                "ProgramFiles",
                "ProgramFiles(x86)",
                "ProgramW6432",
                "SystemRoot",
            ]),
        }
    }
}

pub fn parse_official_provider_json(json: &str) -> Result<OfficialProviderSnapshot, String> {
    let mut snapshot: OfficialProviderSnapshot =
        serde_json::from_str(json).map_err(|error| format!("invalid provider JSON: {error}"))?;
    if snapshot.contract != OFFICIAL_PROVIDER_CONTRACT {
        snapshot.status = OfficialProviderStatus::UnsupportedContract;
        snapshot.error = Some(format!(
            "unsupported provider contract: {}",
            snapshot.contract
        ));
        snapshot.packages.clear();
        return Ok(snapshot);
    }
    if snapshot.status == OfficialProviderStatus::Available {
        for package in &snapshot.packages {
            if package.id.trim().is_empty() || package.installed_version.trim().is_empty() {
                return Err(
                    "available provider package omitted required typed properties".to_owned(),
                );
            }
        }
    }
    Ok(snapshot)
}

#[must_use]
pub fn build_verification_report(
    selector: PackageSelector,
    provider: OfficialProviderSnapshot,
    recipe: Option<PackageRecipeEvidence>,
    observed_at: DateTime<Utc>,
) -> WingetVerificationReport {
    let (id, source, version) = provider_dimensions(&selector, &provider);
    let (scope, architecture, application_health) = recipe_dimensions(&selector, recipe.as_ref());
    let package_identity = aggregate_dimensions([
        id.verdict,
        source.verdict,
        version.verdict,
        scope.verdict,
        architecture.verdict,
    ]);
    let single_safest_next_action = match (package_identity, application_health.verdict) {
        (VerificationVerdict::Verified, ApplicationHealthVerdict::Healthy) => {
            "The exact package identity and application recipe are verified. Preserve this report with the execution evidence."
                .to_owned()
        }
        (VerificationVerdict::NotVerified, _) | (_, ApplicationHealthVerdict::Unhealthy) => {
            "Do not rely on the installation. Review the contradictory dimension and package-specific health evidence before creating another governed action."
                .to_owned()
        }
        _ => "Treat the installation as unverified. Install or repair the official typed provider or add a reviewed package recipe for every unproven dimension."
            .to_owned(),
    };

    WingetVerificationReport {
        provider_id: "Microsoft.WinGet.Client".to_owned(),
        provider_contract: provider.contract,
        provider_status: provider.status,
        provider_module_version: provider.module_version,
        selector,
        id,
        source,
        version,
        scope,
        architecture,
        package_identity,
        application_health,
        observed_at,
        limitations: vec![
            "Provider execution success, package identity, and application health are independent verdicts."
                .to_owned(),
            "The official typed provider exposes ID, source name, and installed version but not installed scope or architecture."
                .to_owned(),
            "Recipe path and PE evidence apply only to the resolved executable, not every installed file or component."
                .to_owned(),
            "ToolOS never parses localized WinGet tables or message substrings for verification."
                .to_owned(),
        ],
        single_safest_next_action,
    }
}

#[must_use]
pub fn parse_git_version_output(output: &str) -> Option<String> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let version = line.strip_prefix("git version ")?;
    let core = version
        .split_once(".windows.")
        .map_or(version, |(core, _)| core);
    let parts: Vec<_> = core.split('.').collect();
    if !(2..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|value| value.is_ascii_digit()))
    {
        return None;
    }
    Some(core.to_owned())
}

pub fn inspect_pe_architecture(path: &Path) -> Result<Option<String>, String> {
    let mut file = File::open(path).map_err(|error| format!("open PE image: {error}"))?;
    let mut dos = [0_u8; 64];
    file.read_exact(&mut dos)
        .map_err(|error| format!("read PE DOS header: {error}"))?;
    if &dos[..2] != b"MZ" {
        return Ok(None);
    }
    let offset = u32::from_le_bytes([dos[0x3c], dos[0x3d], dos[0x3e], dos[0x3f]]);
    file.seek(SeekFrom::Start(u64::from(offset)))
        .map_err(|error| format!("seek PE header: {error}"))?;
    let mut header = [0_u8; 6];
    file.read_exact(&mut header)
        .map_err(|error| format!("read PE signature: {error}"))?;
    if &header[..4] != b"PE\0\0" {
        return Ok(None);
    }
    let machine = u16::from_le_bytes([header[4], header[5]]);
    Ok(match machine {
        0x014c => Some("x86".to_owned()),
        0x8664 => Some("x64".to_owned()),
        0xaa64 => Some("arm64".to_owned()),
        _ => None,
    })
}

#[must_use]
pub fn classify_scope_path(path: &Path, roots: &ScopeRoots) -> Option<PackageScope> {
    let normalized = normalize_path(path);
    let user = roots
        .user_roots
        .iter()
        .any(|root| is_below(&normalized, &normalize_path(root)));
    let system = roots
        .system_roots
        .iter()
        .any(|root| is_below(&normalized, &normalize_path(root)));
    match (user, system) {
        (true, false) => Some(PackageScope::User),
        (false, true) => Some(PackageScope::Machine),
        _ => None,
    }
}

#[must_use]
pub fn git_recipe_evidence(
    executable_path: Option<&Path>,
    process_evidence: Option<ProcessEvidence>,
    roots: &ScopeRoots,
) -> PackageRecipeEvidence {
    let executable_path_text = executable_path.map(|path| path.to_string_lossy().into_owned());
    let raw_version_output = process_evidence.as_ref().map(|process| {
        if process.stdout.trim().is_empty() {
            process.stderr.clone()
        } else {
            process.stdout.clone()
        }
    });
    let parsed_version = raw_version_output
        .as_deref()
        .and_then(parse_git_version_output);
    let architecture =
        executable_path.and_then(|path| inspect_pe_architecture(path).ok().flatten());
    let scope = executable_path.and_then(|path| classify_scope_path(path, roots));
    PackageRecipeEvidence {
        recipe_id: GIT_RECIPE_ID.to_owned(),
        package_id: "Git.Git".to_owned(),
        executable_path: executable_path_text,
        version_parser: GIT_VERSION_PARSER.to_owned(),
        raw_version_output,
        parsed_version,
        architecture,
        scope,
        process_evidence,
        limitations: vec![
            "The recipe invokes only `git --version`.".to_owned(),
            "Scope is verified only when the canonical executable path is below one unambiguous configured user or system root."
                .to_owned(),
            "Architecture is read from the resolved executable PE Machine field.".to_owned(),
        ],
    }
}

fn provider_dimensions(
    selector: &PackageSelector,
    provider: &OfficialProviderSnapshot,
) -> (
    VerificationDimension,
    VerificationDimension,
    VerificationDimension,
) {
    let expected_version = selector.version.clone();
    if provider.status != OfficialProviderStatus::Available {
        let limitation = provider
            .error
            .clone()
            .unwrap_or_else(|| "official provider is unavailable".to_owned());
        return (
            indeterminate(Some(selector.package_id.clone()), limitation.clone()),
            indeterminate(Some(selector.source.clone()), limitation.clone()),
            indeterminate(expected_version, limitation),
        );
    }
    if provider.packages.is_empty() {
        return (
            not_verified(
                Some(selector.package_id.clone()),
                None,
                "The official typed provider returned no exact installed package.".to_owned(),
            ),
            not_verified(
                Some(selector.source.clone()),
                None,
                "No exact installed package source was returned.".to_owned(),
            ),
            not_verified(
                expected_version,
                None,
                "No installed version was returned.".to_owned(),
            ),
        );
    }
    if provider.packages.len() != 1 {
        let limitation = format!(
            "The official typed provider returned {} exact matches; ToolOS requires exactly one.",
            provider.packages.len()
        );
        return (
            indeterminate(Some(selector.package_id.clone()), limitation.clone()),
            indeterminate(Some(selector.source.clone()), limitation.clone()),
            indeterminate(expected_version, limitation),
        );
    }
    let package = &provider.packages[0];
    let id = equality_dimension(
        selector.package_id.clone(),
        package.id.clone(),
        "Official typed package ID",
    );
    let source = match &package.source {
        Some(source) => equality_dimension(
            selector.source.clone(),
            source.clone(),
            "Official typed source name",
        ),
        None => indeterminate(
            Some(selector.source.clone()),
            "The official typed package object did not expose a source name.".to_owned(),
        ),
    };
    let version = match (&selector.version, package.version_comparison) {
        (Some(expected), OfficialVersionComparison::Equal) => verified(
            Some(expected.clone()),
            Some(package.installed_version.clone()),
            "PSInstalledCatalogPackage.CompareToVersion returned Equal.".to_owned(),
        ),
        (
            Some(expected),
            OfficialVersionComparison::Lesser | OfficialVersionComparison::Greater,
        ) => not_verified(
            Some(expected.clone()),
            Some(package.installed_version.clone()),
            format!(
                "PSInstalledCatalogPackage.CompareToVersion returned {:?}.",
                package.version_comparison
            ),
        ),
        (Some(expected), OfficialVersionComparison::Unknown) => indeterminate_observed(
            Some(expected.clone()),
            Some(package.installed_version.clone()),
            "PSInstalledCatalogPackage.CompareToVersion returned Unknown.".to_owned(),
        ),
        (None, _) => indeterminate(
            None,
            "The immutable selector did not contain a requested version.".to_owned(),
        ),
    };
    (id, source, version)
}

fn recipe_dimensions(
    selector: &PackageSelector,
    recipe: Option<&PackageRecipeEvidence>,
) -> (
    VerificationDimension,
    VerificationDimension,
    ApplicationHealthReport,
) {
    let Some(recipe) = recipe else {
        return (
            indeterminate(
                selector.scope.as_ref().map(scope_name),
                "No reviewed package recipe supplied scope evidence.".to_owned(),
            ),
            indeterminate(
                selector.architecture.clone(),
                "No reviewed package recipe supplied architecture evidence.".to_owned(),
            ),
            ApplicationHealthReport {
                verdict: ApplicationHealthVerdict::Indeterminate,
                recipe_id: None,
                executable_path: None,
                parsed_version: None,
                process_evidence: None,
                evidence: Vec::new(),
                limitations: vec!["No reviewed package recipe is registered.".to_owned()],
            },
        );
    };
    if recipe.package_id != selector.package_id {
        let limitation = "The supplied recipe does not match the immutable package ID.".to_owned();
        return (
            indeterminate(selector.scope.as_ref().map(scope_name), limitation.clone()),
            indeterminate(selector.architecture.clone(), limitation.clone()),
            ApplicationHealthReport {
                verdict: ApplicationHealthVerdict::Indeterminate,
                recipe_id: Some(recipe.recipe_id.clone()),
                executable_path: recipe.executable_path.clone(),
                parsed_version: recipe.parsed_version.clone(),
                process_evidence: recipe.process_evidence.clone(),
                evidence: Vec::new(),
                limitations: vec![limitation],
            },
        );
    }
    let executable_missing = recipe.executable_path.is_none();
    let scope = match (&selector.scope, &recipe.scope) {
        (_, _) if executable_missing => not_verified(
            selector.scope.as_ref().map(scope_name),
            None,
            "The recipe could not resolve its required executable.".to_owned(),
        ),
        (Some(expected), Some(observed)) if expected == observed => verified(
            Some(scope_name(expected)),
            Some(scope_name(observed)),
            "The canonical executable path is below the expected explicit scope root.".to_owned(),
        ),
        (Some(expected), Some(observed)) => not_verified(
            Some(scope_name(expected)),
            Some(scope_name(observed)),
            "The canonical executable path is below a conflicting scope root.".to_owned(),
        ),
        (Some(expected), None) => indeterminate(
            Some(scope_name(expected)),
            "The executable path is not below one unambiguous configured scope root.".to_owned(),
        ),
        (None, _) => indeterminate(
            None,
            "The immutable selector did not require a scope.".to_owned(),
        ),
    };
    let architecture = match (&selector.architecture, &recipe.architecture) {
        (_, _) if executable_missing => not_verified(
            selector.architecture.clone(),
            None,
            "The recipe could not resolve its required executable.".to_owned(),
        ),
        (Some(expected), Some(observed)) if expected.eq_ignore_ascii_case(observed) => verified(
            Some(expected.clone()),
            Some(observed.clone()),
            "The executable PE Machine field matches the requested architecture.".to_owned(),
        ),
        (Some(expected), Some(observed)) => not_verified(
            Some(expected.clone()),
            Some(observed.clone()),
            "The executable PE Machine field contradicts the requested architecture.".to_owned(),
        ),
        (Some(expected), None) => indeterminate(
            Some(expected.clone()),
            "The recipe could not prove executable architecture.".to_owned(),
        ),
        (None, _) => indeterminate(
            None,
            "The immutable selector did not require an architecture.".to_owned(),
        ),
    };
    let health = if executable_missing {
        ApplicationHealthReport {
            verdict: ApplicationHealthVerdict::Unhealthy,
            recipe_id: Some(recipe.recipe_id.clone()),
            executable_path: None,
            parsed_version: None,
            process_evidence: None,
            evidence: vec!["The required recipe executable was not found.".to_owned()],
            limitations: recipe.limitations.clone(),
        }
    } else {
        match &recipe.process_evidence {
            Some(process) if process.exit_code == Some(0) && recipe.parsed_version.is_some() => {
                ApplicationHealthReport {
                    verdict: ApplicationHealthVerdict::Healthy,
                    recipe_id: Some(recipe.recipe_id.clone()),
                    executable_path: recipe.executable_path.clone(),
                    parsed_version: recipe.parsed_version.clone(),
                    process_evidence: Some(process.clone()),
                    evidence: vec![
                        "The fixed recipe command exited zero and its version parser accepted the output."
                            .to_owned(),
                    ],
                    limitations: recipe.limitations.clone(),
                }
            }
            Some(process) if process.exit_code != Some(0) => ApplicationHealthReport {
                verdict: ApplicationHealthVerdict::Unhealthy,
                recipe_id: Some(recipe.recipe_id.clone()),
                executable_path: recipe.executable_path.clone(),
                parsed_version: recipe.parsed_version.clone(),
                process_evidence: Some(process.clone()),
                evidence: vec!["The fixed recipe command did not exit successfully.".to_owned()],
                limitations: recipe.limitations.clone(),
            },
            Some(process) => ApplicationHealthReport {
                verdict: ApplicationHealthVerdict::Indeterminate,
                recipe_id: Some(recipe.recipe_id.clone()),
                executable_path: recipe.executable_path.clone(),
                parsed_version: recipe.parsed_version.clone(),
                process_evidence: Some(process.clone()),
                evidence: vec![
                    "The fixed recipe command completed, but the built-in parser could not prove the expected output contract."
                        .to_owned(),
                ],
                limitations: recipe.limitations.clone(),
            },
            None => ApplicationHealthReport {
                verdict: ApplicationHealthVerdict::Indeterminate,
                recipe_id: Some(recipe.recipe_id.clone()),
                executable_path: recipe.executable_path.clone(),
                parsed_version: recipe.parsed_version.clone(),
                process_evidence: None,
                evidence: vec!["No recipe process evidence was captured.".to_owned()],
                limitations: recipe.limitations.clone(),
            },
        }
    };
    (scope, architecture, health)
}

fn aggregate_dimensions<const N: usize>(verdicts: [VerificationVerdict; N]) -> VerificationVerdict {
    if verdicts.contains(&VerificationVerdict::NotVerified) {
        VerificationVerdict::NotVerified
    } else if verdicts
        .iter()
        .all(|verdict| *verdict == VerificationVerdict::Verified)
    {
        VerificationVerdict::Verified
    } else {
        VerificationVerdict::Indeterminate
    }
}

fn equality_dimension(expected: String, observed: String, label: &str) -> VerificationDimension {
    if expected == observed {
        verified(
            Some(expected),
            Some(observed),
            format!("{label} exactly matches the immutable selector."),
        )
    } else {
        not_verified(
            Some(expected),
            Some(observed),
            format!("{label} contradicts the immutable selector."),
        )
    }
}

fn verified(
    expected: Option<String>,
    observed: Option<String>,
    evidence: String,
) -> VerificationDimension {
    VerificationDimension {
        verdict: VerificationVerdict::Verified,
        expected,
        observed,
        evidence: vec![evidence],
        limitations: Vec::new(),
    }
}

fn not_verified(
    expected: Option<String>,
    observed: Option<String>,
    evidence: String,
) -> VerificationDimension {
    VerificationDimension {
        verdict: VerificationVerdict::NotVerified,
        expected,
        observed,
        evidence: vec![evidence],
        limitations: Vec::new(),
    }
}

fn indeterminate(expected: Option<String>, limitation: String) -> VerificationDimension {
    indeterminate_observed(expected, None, limitation)
}

fn indeterminate_observed(
    expected: Option<String>,
    observed: Option<String>,
    limitation: String,
) -> VerificationDimension {
    VerificationDimension {
        verdict: VerificationVerdict::Indeterminate,
        expected,
        observed,
        evidence: Vec::new(),
        limitations: vec![limitation],
    }
}

fn scope_name(scope: &PackageScope) -> String {
    match scope {
        PackageScope::User => "user".to_owned(),
        PackageScope::Machine => "machine".to_owned(),
    }
}

fn environment_paths<const N: usize>(names: [&str; N]) -> Vec<PathBuf> {
    names
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .collect()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn is_below(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn selector() -> PackageSelector {
        PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("2.50.1".to_owned()),
            scope: Some(PackageScope::User),
            architecture: Some("x64".to_owned()),
        }
    }

    fn provider(version_comparison: OfficialVersionComparison) -> OfficialProviderSnapshot {
        OfficialProviderSnapshot {
            contract: OFFICIAL_PROVIDER_CONTRACT.to_owned(),
            module_version: Some("1.12.0".to_owned()),
            status: OfficialProviderStatus::Available,
            error: None,
            packages: vec![OfficialInstalledPackage {
                id: "Git.Git".to_owned(),
                source: Some("winget".to_owned()),
                installed_version: "2.50.1".to_owned(),
                version_comparison,
            }],
        }
    }

    fn healthy_recipe(scope: PackageScope, architecture: &str) -> PackageRecipeEvidence {
        PackageRecipeEvidence {
            recipe_id: GIT_RECIPE_ID.to_owned(),
            package_id: "Git.Git".to_owned(),
            executable_path: Some("C:\\Users\\test\\AppData\\Local\\Git\\cmd\\git.exe".to_owned()),
            version_parser: GIT_VERSION_PARSER.to_owned(),
            raw_version_output: Some("git version 2.50.1.windows.1".to_owned()),
            parsed_version: Some("2.50.1".to_owned()),
            architecture: Some(architecture.to_owned()),
            scope: Some(scope),
            process_evidence: Some(ProcessEvidence {
                executable: "git.exe".to_owned(),
                args: vec!["--version".to_owned()],
                exit_code: Some(0),
                stdout: "git version 2.50.1.windows.1".to_owned(),
                stderr: String::new(),
                timed_out: false,
                duration_ms: 1,
            }),
            limitations: Vec::new(),
        }
    }

    #[test]
    fn official_provider_contract_parses_without_localized_text() {
        let json = r#"{"contract":"toolos.microsoft-winget-client.installed/1","module_version":"1.12.0","status":"AVAILABLE","error":null,"packages":[{"id":"Git.Git","source":"winget","installed_version":"2.50.1","version_comparison":"EQUAL"}]}"#;
        let parsed = parse_official_provider_json(json).expect("provider JSON");
        assert_eq!(parsed.packages[0].id, "Git.Git");
        assert_eq!(
            parsed.packages[0].version_comparison,
            OfficialVersionComparison::Equal
        );
    }

    #[test]
    fn unsupported_contract_is_indeterminate_not_guessed() {
        let json = r#"{"contract":"future/2","module_version":"2.0","status":"AVAILABLE","error":null,"packages":[]}"#;
        let parsed = parse_official_provider_json(json).expect("provider JSON");
        assert_eq!(parsed.status, OfficialProviderStatus::UnsupportedContract);
        let report = build_verification_report(selector(), parsed, None, Utc::now());
        assert_eq!(report.package_identity, VerificationVerdict::Indeterminate);
    }

    #[test]
    fn exact_all_dimensions_and_healthy_recipe_verify_independently() {
        let report = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Equal),
            Some(healthy_recipe(PackageScope::User, "x64")),
            Utc::now(),
        );
        assert_eq!(report.package_identity, VerificationVerdict::Verified);
        assert_eq!(
            report.application_health.verdict,
            ApplicationHealthVerdict::Healthy
        );
    }

    #[test]
    fn wrong_version_architecture_and_scope_are_not_verified() {
        let version = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Lesser),
            Some(healthy_recipe(PackageScope::User, "x64")),
            Utc::now(),
        );
        assert_eq!(version.version.verdict, VerificationVerdict::NotVerified);

        let architecture = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Equal),
            Some(healthy_recipe(PackageScope::User, "x86")),
            Utc::now(),
        );
        assert_eq!(
            architecture.architecture.verdict,
            VerificationVerdict::NotVerified
        );

        let scope = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Equal),
            Some(healthy_recipe(PackageScope::Machine, "x64")),
            Utc::now(),
        );
        assert_eq!(scope.scope.verdict, VerificationVerdict::NotVerified);
    }

    #[test]
    fn missing_executable_and_failing_healthcheck_are_unhealthy() {
        let missing = PackageRecipeEvidence {
            executable_path: None,
            process_evidence: None,
            parsed_version: None,
            architecture: None,
            scope: None,
            ..healthy_recipe(PackageScope::User, "x64")
        };
        let report = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Equal),
            Some(missing),
            Utc::now(),
        );
        assert_eq!(
            report.application_health.verdict,
            ApplicationHealthVerdict::Unhealthy
        );

        let mut failing = healthy_recipe(PackageScope::User, "x64");
        failing
            .process_evidence
            .as_mut()
            .expect("process")
            .exit_code = Some(1);
        let report = build_verification_report(
            selector(),
            provider(OfficialVersionComparison::Equal),
            Some(failing),
            Utc::now(),
        );
        assert_eq!(report.package_identity, VerificationVerdict::Verified);
        assert_eq!(
            report.application_health.verdict,
            ApplicationHealthVerdict::Unhealthy
        );
    }

    #[test]
    fn duplicate_provider_matches_are_indeterminate() {
        let mut provider = provider(OfficialVersionComparison::Equal);
        provider.packages.push(provider.packages[0].clone());
        let report = build_verification_report(
            selector(),
            provider,
            Some(healthy_recipe(PackageScope::User, "x64")),
            Utc::now(),
        );
        assert_eq!(report.id.verdict, VerificationVerdict::Indeterminate);
        assert_eq!(report.package_identity, VerificationVerdict::Indeterminate);
    }

    #[test]
    fn git_version_parser_accepts_only_named_contract() {
        assert_eq!(
            parse_git_version_output("git version 2.50.1.windows.1\n"),
            Some("2.50.1".to_owned())
        );
        assert_eq!(parse_git_version_output("git version unknown"), None);
        assert_eq!(parse_git_version_output("localized 2.50.1"), None);
    }

    #[test]
    fn pe_architecture_uses_machine_field() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("fixture.exe");
        let mut bytes = vec![0_u8; 0x86];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        File::create(&path)
            .expect("fixture")
            .write_all(&bytes)
            .expect("write fixture");
        assert_eq!(
            inspect_pe_architecture(&path).expect("PE architecture"),
            Some("x64".to_owned())
        );
    }

    #[test]
    fn scope_classifier_proves_only_unambiguous_roots() {
        let roots = ScopeRoots {
            user_roots: vec![PathBuf::from(r"C:\Users\test")],
            system_roots: vec![PathBuf::from(r"C:\Program Files")],
        };
        assert_eq!(
            classify_scope_path(
                Path::new(r"C:\Users\test\AppData\Local\Git\git.exe"),
                &roots
            ),
            Some(PackageScope::User)
        );
        assert_eq!(
            classify_scope_path(Path::new(r"C:\Program Files\Git\cmd\git.exe"), &roots),
            Some(PackageScope::Machine)
        );
        assert_eq!(
            classify_scope_path(Path::new(r"D:\Portable\Git\git.exe"), &roots),
            None
        );
    }
}
