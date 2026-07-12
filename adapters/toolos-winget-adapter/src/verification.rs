use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::process::Command;
use toolos_winget::{
    build_verification_report, git_recipe_evidence, normalize_selector,
    parse_official_provider_json, OfficialProviderSnapshot, OfficialProviderStatus,
    PackageSelector, ProcessEvidence, ScopeRoots, OFFICIAL_PROVIDER_CONTRACT,
};

const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const POWERSHELL_SELECTOR_ENV: &str = "TOOLOS_WINGET_SELECTOR_JSON";
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(45);
const RECIPE_TIMEOUT: Duration = Duration::from_secs(10);

const OFFICIAL_PROVIDER_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$result = [ordered]@{
    contract = 'toolos.microsoft-winget-client.installed/1'
    module_version = $null
    status = 'MODULE_UNAVAILABLE'
    error = $null
    packages = @()
}
try {
    $selectorText = [Environment]::GetEnvironmentVariable('TOOLOS_WINGET_SELECTOR_JSON', 'Process')
    if ([string]::IsNullOrWhiteSpace($selectorText)) {
        throw [InvalidOperationException]::new('ToolOS selector environment variable is absent')
    }
    $selector = $selectorText | ConvertFrom-Json -ErrorAction Stop
    $module = Get-Module -ListAvailable -Name Microsoft.WinGet.Client |
        Sort-Object Version -Descending |
        Select-Object -First 1
    if ($null -ne $module) {
        Import-Module -Name $module.Path -ErrorAction Stop
        $result.module_version = $module.Version.ToString()
        $items = @(Get-WinGetPackage `
            -Id ([string]$selector.package_id) `
            -Source ([string]$selector.source) `
            -MatchOption Equals `
            -ErrorAction Stop)
        $packages = @($items | ForEach-Object {
            $comparison = 'UNKNOWN'
            if ($null -ne $selector.version -and -not [string]::IsNullOrWhiteSpace([string]$selector.version)) {
                $comparison = $_.CompareToVersion([string]$selector.version).ToString().ToUpperInvariant()
            }
            [ordered]@{
                id = [string]$_.Id
                source = if ($null -eq $_.Source) { $null } else { [string]$_.Source }
                installed_version = [string]$_.InstalledVersion
                version_comparison = $comparison
            }
        })
        $result.status = 'AVAILABLE'
        $result.packages = $packages
    }
}
catch {
    $result.status = 'FAILED'
    $result.error = $_.Exception.GetType().FullName + ': ' + $_.Exception.Message
    $result.packages = @()
}
$result | ConvertTo-Json -Compress -Depth 6
"#;

pub async fn verify_request(params: Value) -> Result<Value, String> {
    let selector = serde_json::from_value::<PackageSelector>(params)
        .map_err(|error| format!("winget.verify requires a valid package selector: {error}"))?;
    let selector = normalize_selector(selector)?;
    let (provider, provider_process_evidence) = official_provider_probe(&selector).await;
    let recipe = recipe_probe(&selector).await;
    let report = build_verification_report(selector, provider, recipe, chrono::Utc::now());
    serde_json::to_value(json!({
        "report": report,
        "provider_process_evidence": provider_process_evidence
    }))
    .map_err(|error| error.to_string())
}

async fn official_provider_probe(
    selector: &PackageSelector,
) -> (OfficialProviderSnapshot, ProcessEvidence) {
    let executable = find_executable(&powershell_candidates());
    let selector_json = match serde_json::to_string(selector) {
        Ok(value) => value,
        Err(error) => {
            let evidence = failed_process("powershell", &format!("serialize selector: {error}"));
            return (failed_snapshot(evidence.stderr.clone()), evidence);
        }
    };
    let Some(executable) = executable else {
        let evidence = failed_process(
            "powershell",
            "No PowerShell executable was resolvable for the official typed provider probe.",
        );
        return (
            OfficialProviderSnapshot {
                contract: OFFICIAL_PROVIDER_CONTRACT.to_owned(),
                module_version: None,
                status: OfficialProviderStatus::ModuleUnavailable,
                error: Some(evidence.stderr.clone()),
                packages: Vec::new(),
            },
            evidence,
        );
    };
    let args = vec![
        "-NoLogo".to_owned(),
        "-NoProfile".to_owned(),
        "-NonInteractive".to_owned(),
        "-Command".to_owned(),
        OFFICIAL_PROVIDER_SCRIPT.to_owned(),
    ];
    let evidence = match run_command_with_environment(
        &executable,
        &args,
        [(POWERSHELL_SELECTOR_ENV, selector_json.as_str())],
        PROVIDER_TIMEOUT,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => failed_process(&executable.to_string_lossy(), &error),
    };
    if evidence.exit_code != Some(0) || evidence.timed_out {
        return (failed_snapshot(evidence.stderr.clone()), evidence);
    }
    let snapshot = match parse_official_provider_json(evidence.stdout.trim()) {
        Ok(value) => value,
        Err(error) => failed_snapshot(error),
    };
    (snapshot, evidence)
}

async fn recipe_probe(selector: &PackageSelector) -> Option<toolos_winget::PackageRecipeEvidence> {
    if selector.package_id != "Git.Git" {
        return None;
    }
    let executable = find_executable(&["git.exe", "git"]);
    let Some(executable) = executable else {
        return Some(git_recipe_evidence(
            None,
            None,
            &ScopeRoots::from_environment(),
        ));
    };
    let canonical = std::fs::canonicalize(&executable).unwrap_or(executable);
    let args = vec!["--version".to_owned()];
    let process = match run_command_with_environment(
        &canonical,
        &args,
        std::iter::empty::<(&str, &str)>(),
        RECIPE_TIMEOUT,
    )
    .await
    {
        Ok(value) => Some(value),
        Err(error) => Some(failed_process(&canonical.to_string_lossy(), &error)),
    };
    Some(git_recipe_evidence(
        Some(&canonical),
        process,
        &ScopeRoots::from_environment(),
    ))
}

async fn run_command_with_environment<'a, I>(
    executable: &Path,
    args: &[String],
    environment: I,
    timeout: Duration,
) -> Result<ProcessEvidence, String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let started = Instant::now();
    let child = Command::new(executable)
        .args(args)
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", executable.display()))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(ProcessEvidence {
            executable: executable.to_string_lossy().into_owned(),
            args: args.to_vec(),
            exit_code: output.status.code(),
            stdout: bounded_text(&output.stdout),
            stderr: bounded_text(&output.stderr),
            timed_out: false,
            duration_ms: duration_ms(started.elapsed()),
        }),
        Ok(Err(error)) => Err(format!(
            "failed while waiting for {}: {error}",
            executable.display()
        )),
        Err(_) => Ok(ProcessEvidence {
            executable: executable.to_string_lossy().into_owned(),
            args: args.to_vec(),
            exit_code: None,
            stdout: String::new(),
            stderr: format!("command exceeded {} seconds", timeout.as_secs()),
            timed_out: true,
            duration_ms: duration_ms(started.elapsed()),
        }),
    }
}

fn failed_snapshot(error: String) -> OfficialProviderSnapshot {
    OfficialProviderSnapshot {
        contract: OFFICIAL_PROVIDER_CONTRACT.to_owned(),
        module_version: None,
        status: OfficialProviderStatus::Failed,
        error: Some(error),
        packages: Vec::new(),
    }
}

fn failed_process(executable: &str, error: &str) -> ProcessEvidence {
    ProcessEvidence {
        executable: executable.to_owned(),
        args: Vec::new(),
        exit_code: None,
        stdout: String::new(),
        stderr: error.to_owned(),
        timed_out: false,
        duration_ms: 0,
    }
}

fn powershell_candidates() -> [&'static str; 3] {
    if cfg!(windows) {
        ["powershell.exe", "pwsh.exe", "pwsh"]
    } else {
        ["pwsh", "powershell", "powershell.exe"]
    }
}

fn find_executable(candidates: &[&str]) -> Option<PathBuf> {
    let paths = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect::<Vec<_>>())
        .unwrap_or_default();
    find_executable_in_paths(candidates, &paths)
}

fn find_executable_in_paths(candidates: &[&str], paths: &[PathBuf]) -> Option<PathBuf> {
    for directory in paths {
        for candidate in candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn bounded_text(bytes: &[u8]) -> String {
    let limit = bytes.len().min(MAX_CAPTURE_BYTES);
    let mut text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    if bytes.len() > MAX_CAPTURE_BYTES {
        text.push_str("\n[ToolOS truncated verification output at 65536 bytes]");
    }
    text
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn provider_script_uses_only_copied_typed_properties() {
        for required in [
            "Get-WinGetPackage",
            "-MatchOption Equals",
            "$_.Id",
            "$_.Source",
            "$_.InstalledVersion",
            "$_.CompareToVersion",
            "ConvertTo-Json",
        ] {
            assert!(OFFICIAL_PROVIDER_SCRIPT.contains(required));
        }
        assert!(!OFFICIAL_PROVIDER_SCRIPT.contains("winget list"));
        assert!(!OFFICIAL_PROVIDER_SCRIPT.contains("Format-Table"));
    }

    #[test]
    fn executable_resolution_is_bounded_to_supplied_path_roots() {
        let directory = tempdir().expect("tempdir");
        let executable = directory
            .path()
            .join(if cfg!(windows) { "tool.exe" } else { "tool" });
        std::fs::write(&executable, b"fixture").expect("fixture");
        assert_eq!(
            find_executable_in_paths(
                &[executable.file_name().unwrap().to_str().unwrap()],
                &[directory.path().to_path_buf()]
            ),
            Some(executable)
        );
        assert!(
            find_executable_in_paths(&["missing"], &[directory.path().to_path_buf()]).is_none()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn real_git_recipe_exercises_runner_git() {
        let selector = PackageSelector {
            package_id: "Git.Git".to_owned(),
            source: "winget".to_owned(),
            version: Some("0.0.0".to_owned()),
            scope: Some(toolos_winget::PackageScope::Machine),
            architecture: Some("x64".to_owned()),
        };
        let recipe = recipe_probe(&selector).await.expect("Git recipe");
        assert!(recipe.executable_path.is_some());
        assert_eq!(
            recipe
                .process_evidence
                .as_ref()
                .and_then(|value| value.exit_code),
            Some(0)
        );
        assert!(recipe.parsed_version.is_some());
        assert!(matches!(
            recipe.architecture.as_deref(),
            Some("x86" | "x64" | "arm64")
        ));
    }
}
