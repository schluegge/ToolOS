use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use chrono::Utc;
use toolos_domain::{EnvironmentKind, MachineSnapshot, ProjectSnapshot, ToolObservation};

const TOOL_CANDIDATES: &[(&str, &str, &[&str], EnvironmentKind)] = &[
    ("git", "Git", &["git"], EnvironmentKind::WindowsNative),
    ("cargo", "Cargo", &["cargo"], EnvironmentKind::WindowsNative),
    ("rustc", "Rust compiler", &["rustc"], EnvironmentKind::WindowsNative),
    ("node", "Node.js", &["node"], EnvironmentKind::WindowsNative),
    ("pnpm", "pnpm", &["pnpm"], EnvironmentKind::WindowsNative),
    ("python", "Python", &["python", "python3"], EnvironmentKind::WindowsNative),
    ("uv", "uv", &["uv"], EnvironmentKind::WindowsNative),
    ("pwsh", "PowerShell 7", &["pwsh"], EnvironmentKind::PowerShell),
    ("powershell", "Windows PowerShell", &["powershell"], EnvironmentKind::PowerShell),
    ("winget", "WinGet", &["winget"], EnvironmentKind::WindowsNative),
    ("wsl", "Windows Subsystem for Linux", &["wsl"], EnvironmentKind::Wsl),
    ("docker", "Docker", &["docker"], EnvironmentKind::Container),
    ("podman", "Podman", &["podman"], EnvironmentKind::Container),
    ("ollama", "Ollama", &["ollama"], EnvironmentKind::WindowsNative),
    ("codex", "Codex CLI", &["codex"], EnvironmentKind::WindowsNative),
    ("claude", "Claude Code", &["claude"], EnvironmentKind::WindowsNative),
    ("gemini", "Gemini CLI", &["gemini"], EnvironmentKind::WindowsNative),
    ("opencode", "OpenCode", &["opencode"], EnvironmentKind::WindowsNative),
    ("pi", "Pi coding agent", &["pi"], EnvironmentKind::WindowsNative),
];

const PROJECT_MARKERS: &[(&str, &str)] = &[
    ("Cargo.toml", "Rust"),
    ("package.json", "Node.js/TypeScript"),
    ("pnpm-workspace.yaml", "pnpm workspace"),
    ("pyproject.toml", "Python"),
    ("requirements.txt", "Python"),
    ("go.mod", "Go"),
    ("pom.xml", "Java/Maven"),
    ("build.gradle", "Java/Gradle"),
    ("build.gradle.kts", "Kotlin/Gradle"),
    ("CMakeLists.txt", "C/C++ CMake"),
    ("global.json", ".NET"),
    ("Dockerfile", "Container"),
    ("compose.yaml", "Container Compose"),
    ("docker-compose.yml", "Container Compose"),
];

const INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".cursorrules",
    "GEMINI.md",
    "CODEX.md",
];

#[must_use]
pub fn inspect_machine() -> MachineSnapshot {
    let tools = TOOL_CANDIDATES
        .iter()
        .map(|(tool_id, display_name, executables, environment)| {
            let resolved = executables.iter().find_map(|name| resolve_executable(name));
            ToolObservation {
                tool_id: (*tool_id).to_owned(),
                display_name: (*display_name).to_owned(),
                executable: executables.join(" | "),
                discovered: resolved.is_some(),
                resolved_path: resolved.map(|path| path.to_string_lossy().into_owned()),
                execution_environment: environment.clone(),
                limitations: vec![
                    "PATH presence only; version, authentication, compatibility, and health are not inferred"
                        .to_owned(),
                ],
            }
        })
        .collect::<Vec<_>>();

    let mut environments = BTreeSet::new();
    environments.insert(if cfg!(windows) {
        "WINDOWS_NATIVE".to_owned()
    } else {
        "UNIX_NATIVE".to_owned()
    });
    for tool in &tools {
        if tool.discovered {
            environments.insert(environment_label(&tool.execution_environment).to_owned());
        }
    }

    MachineSnapshot {
        os: env::consts::OS.to_owned(),
        architecture: env::consts::ARCH.to_owned(),
        hostname_token: hostname_token(),
        environments: environments
            .into_iter()
            .filter_map(|label| environment_from_label(&label))
            .collect(),
        tools,
        privacy_mode: "METADATA_ONLY".to_owned(),
        observed_at: Utc::now(),
    }
}

#[must_use]
pub fn inspect_project(path: impl AsRef<Path>) -> ProjectSnapshot {
    let requested = path.as_ref();
    let exists = requested.exists();
    let is_directory = requested.is_dir();
    let canonical = requested.canonicalize().ok();
    let scan_root = canonical.as_deref().unwrap_or(requested);

    let repository_root = if is_directory {
        find_repository_root(scan_root)
    } else {
        None
    };
    let identity_root = repository_root.as_deref().unwrap_or(scan_root);

    let mut markers = Vec::new();
    let mut stacks = BTreeSet::new();
    if is_directory {
        for (marker, stack) in PROJECT_MARKERS {
            if identity_root.join(marker).is_file() {
                markers.push((*marker).to_owned());
                stacks.insert((*stack).to_owned());
            }
        }
        for entry in read_directory_names(identity_root) {
            let lower = entry.to_ascii_lowercase();
            if lower.ends_with(".sln") || lower.ends_with(".csproj") || lower.ends_with(".fsproj") {
                markers.push(entry);
                stacks.insert(".NET".to_owned());
            }
        }
    }

    let instruction_files = if is_directory {
        INSTRUCTION_FILES
            .iter()
            .filter(|name| identity_root.join(name).is_file())
            .map(|name| (*name).to_owned())
            .collect()
    } else {
        Vec::new()
    };

    ProjectSnapshot {
        requested_path: requested.to_string_lossy().into_owned(),
        canonical_path: canonical.map(|value| value.to_string_lossy().into_owned()),
        exists,
        is_directory,
        repository_root: repository_root.map(|value| value.to_string_lossy().into_owned()),
        markers,
        detected_stacks: stacks.into_iter().collect(),
        instruction_files,
        limitations: vec![
            "Top-level marker inspection only; repository scripts and package hooks were not executed"
                .to_owned(),
            "A marker indicates probable project structure, not a healthy runtime or build".to_owned(),
        ],
        observed_at: Utc::now(),
    }
}

fn resolve_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 && candidate.is_file() {
        return candidate.canonicalize().ok().or_else(|| Some(candidate.to_path_buf()));
    }

    let path = env::var_os("PATH")?;
    let extensions = executable_extensions();
    for directory in env::split_paths(&path) {
        for extension in &extensions {
            let file_name = if extension.is_empty() {
                name.to_owned()
            } else {
                format!("{name}{extension}")
            };
            let full_path = directory.join(file_name);
            if full_path.is_file() {
                return full_path.canonicalize().ok().or(Some(full_path));
            }
        }
    }
    None
}

fn executable_extensions() -> Vec<String> {
    if cfg!(windows) {
        let configured = env::var_os("PATHEXT")
            .unwrap_or_else(|| OsStr::new(".COM;.EXE;.BAT;.CMD").to_os_string());
        let mut values = configured
            .to_string_lossy()
            .split(';')
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect::<Vec<_>>();
        values.push(String::new());
        values
    } else {
        vec![String::new()]
    }
}

fn find_repository_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn read_directory_names(path: &Path) -> Vec<String> {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

fn hostname_token() -> Option<String> {
    env::var("COMPUTERNAME")
        .ok()
        .or_else(|| env::var("HOSTNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let suffix = value
                .chars()
                .rev()
                .take(4)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            format!("host-…{suffix}")
        })
}

fn environment_label(value: &EnvironmentKind) -> &'static str {
    match value {
        EnvironmentKind::WindowsNative => "WINDOWS_NATIVE",
        EnvironmentKind::PowerShell => "POWERSHELL",
        EnvironmentKind::Cmd => "CMD",
        EnvironmentKind::GitBash => "GIT_BASH",
        EnvironmentKind::Wsl => "WSL",
        EnvironmentKind::Container => "CONTAINER",
        EnvironmentKind::RemoteApi => "REMOTE_API",
        EnvironmentKind::UnixNative => "UNIX_NATIVE",
    }
}

fn environment_from_label(value: &str) -> Option<EnvironmentKind> {
    match value {
        "WINDOWS_NATIVE" => Some(EnvironmentKind::WindowsNative),
        "POWERSHELL" => Some(EnvironmentKind::PowerShell),
        "CMD" => Some(EnvironmentKind::Cmd),
        "GIT_BASH" => Some(EnvironmentKind::GitBash),
        "WSL" => Some(EnvironmentKind::Wsl),
        "CONTAINER" => Some(EnvironmentKind::Container),
        "REMOTE_API" => Some(EnvironmentKind::RemoteApi),
        "UNIX_NATIVE" => Some(EnvironmentKind::UnixNative),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn machine_scan_is_metadata_only() {
        let snapshot = inspect_machine();
        assert_eq!(snapshot.privacy_mode, "METADATA_ONLY");
        assert!(!snapshot.tools.is_empty());
        assert!(snapshot
            .tools
            .iter()
            .all(|tool| !tool.limitations.is_empty()));
    }

    #[test]
    fn project_scan_detects_markers_without_running_them() {
        let root = env::temp_dir().join(format!("toolos-project-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).expect("git directory");
        fs::write(root.join("Cargo.toml"), "[package]\nname='fixture'\n")
            .expect("cargo marker");
        fs::write(root.join("AGENTS.md"), "fixture").expect("instruction marker");

        let snapshot = inspect_project(&root);
        assert!(snapshot.exists);
        assert!(snapshot.markers.contains(&"Cargo.toml".to_owned()));
        assert!(snapshot.instruction_files.contains(&"AGENTS.md".to_owned()));
        assert_eq!(snapshot.detected_stacks, vec!["Rust"]);

        fs::remove_dir_all(root).expect("cleanup");
    }
}
