use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

const MAX_REPORTED_ENTRIES: usize = 500;
const REVIEW_ENTRY_COUNT: usize = 10_000;
const REVIEW_TOTAL_UNCOMPRESSED: u64 = 4 * 1024 * 1024 * 1024;
const BLOCK_TOTAL_UNCOMPRESSED: u64 = 20 * 1024 * 1024 * 1024;
const BLOCK_SINGLE_ENTRY: u64 = 8 * 1024 * 1024 * 1024;
const REVIEW_RATIO: u64 = 200;
const BLOCK_RATIO: u64 = 1_000;
const REVIEW_RATIO_MIN_SIZE: u64 = 10 * 1024 * 1024;
const BLOCK_RATIO_MIN_SIZE: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArchiveDecision {
    AcceptStructure,
    Review,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingSeverity {
    Review,
    Blocker,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ArchiveFinding {
    pub severity: FindingSeverity,
    pub code: String,
    pub entry_index: Option<u64>,
    pub entry_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ZipEntryObservation {
    pub index: u64,
    pub name: String,
    pub enclosed_path: Option<String>,
    pub entry_kind: String,
    pub compression: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub expansion_ratio: Option<u64>,
    pub encrypted: bool,
    pub unix_mode: Option<u32>,
    pub crc32: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ZipInspectionReport {
    pub requested_path: String,
    pub canonical_path: String,
    pub archive_file_size: u64,
    pub archive_entries: u64,
    pub reported_entries: u64,
    pub total_compressed_size: u64,
    pub total_uncompressed_size: u64,
    pub decision: ArchiveDecision,
    pub findings: Vec<ArchiveFinding>,
    pub entries: Vec<ZipEntryObservation>,
    pub privacy_mode: String,
    pub observed_at: DateTime<Utc>,
    pub limitations: Vec<String>,
}

pub fn inspect_zip(path: impl AsRef<Path>) -> Result<ZipInspectionReport, String> {
    let requested = path.as_ref();
    let metadata = fs::metadata(requested).map_err(|error| {
        format!(
            "cannot read archive metadata for {}: {error}",
            requested.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "selected archive path is not a file: {}",
            requested.display()
        ));
    }

    let canonical = requested
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize {}: {error}", requested.display()))?;
    let file = File::open(&canonical)
        .map_err(|error| format!("cannot open {}: {error}", canonical.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        format!(
            "invalid or unsupported ZIP archive {}: {error}",
            canonical.display()
        )
    })?;

    let archive_entries = archive.len();
    let mut findings = Vec::new();
    let mut entries = Vec::with_capacity(archive_entries.min(MAX_REPORTED_ENTRIES));
    let mut total_compressed_size = 0_u64;
    let mut total_uncompressed_size = 0_u64;
    let mut normalized_paths = BTreeMap::<String, (usize, String)>::new();

    if archive
        .has_overlapping_files()
        .map_err(|error| format!("cannot validate overlapping ZIP data: {error}"))?
    {
        findings.push(global_finding(
            FindingSeverity::Blocker,
            "OVERLAPPING_COMPRESSED_DATA",
            "Multiple entries reference overlapping compressed data ranges.",
        ));
    }

    if archive_entries > REVIEW_ENTRY_COUNT {
        findings.push(global_finding(
            FindingSeverity::Review,
            "ENTRY_COUNT_HIGH",
            format!(
                "Archive contains {archive_entries} entries; review extraction resource limits before proceeding."
            ),
        ));
    }

    for index in 0..archive_entries {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect ZIP entry {index}: {error}"))?;
        let name = entry.name().to_owned();
        let enclosed_path = entry
            .enclosed_name()
            .map(|value| value.to_string_lossy().into_owned());
        let compressed_size = entry.compressed_size();
        let uncompressed_size = entry.size();
        let expansion_ratio = expansion_ratio(uncompressed_size, compressed_size);
        let is_directory = entry.is_dir();
        let is_symlink = entry.is_symlink();
        let encrypted = entry.encrypted();
        let entry_kind = if is_directory {
            "DIRECTORY"
        } else if is_symlink {
            "SYMLINK"
        } else {
            "FILE"
        }
        .to_owned();

        total_compressed_size = total_compressed_size.saturating_add(compressed_size);
        total_uncompressed_size = total_uncompressed_size.saturating_add(uncompressed_size);

        if enclosed_path.is_none() {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "PATH_NOT_ENCLOSED",
                index,
                &name,
                "Entry path is absolute, escapes the extraction root, contains a NULL byte, or is otherwise not safely enclosed.",
            ));
        }

        inspect_windows_path(index, &name, &mut findings);

        let normalized = normalize_windows_path(&name);
        if !normalized.is_empty() {
            if let Some((first_index, first_name)) = normalized_paths.get(&normalized) {
                findings.push(entry_finding(
                    FindingSeverity::Blocker,
                    "WINDOWS_PATH_COLLISION",
                    index,
                    &name,
                    format!("Entry collides on Windows with entry {first_index} ({first_name})."),
                ));
            } else {
                normalized_paths.insert(normalized, (index, name.clone()));
            }
        }

        if is_symlink {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "SYMLINK_ENTRY",
                index,
                &name,
                "Archive contains a symbolic-link entry; extraction target validation requires a dedicated guarded extractor.",
            ));
        }

        if encrypted {
            findings.push(entry_finding(
                FindingSeverity::Review,
                "ENCRYPTED_ENTRY",
                index,
                &name,
                "Entry is encrypted; ToolOS did not request a password or inspect its contents.",
            ));
        }

        if suspicious_extension(&name) && !is_directory {
            findings.push(entry_finding(
                FindingSeverity::Review,
                "EXECUTABLE_OR_SCRIPT_CONTENT",
                index,
                &name,
                "Archive contains an executable, installer, shortcut, registry file, or script. Extraction must not imply execution approval.",
            ));
        }

        if uncompressed_size >= BLOCK_SINGLE_ENTRY {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "SINGLE_ENTRY_TOO_LARGE",
                index,
                &name,
                format!(
                    "Entry declares {uncompressed_size} uncompressed bytes, exceeding the structural safety limit."
                ),
            ));
        }

        if let Some(ratio) = expansion_ratio {
            if ratio >= BLOCK_RATIO && uncompressed_size >= BLOCK_RATIO_MIN_SIZE {
                findings.push(entry_finding(
                    FindingSeverity::Blocker,
                    "EXTREME_EXPANSION_RATIO",
                    index,
                    &name,
                    format!("Entry declares an expansion ratio of at least {ratio}:1."),
                ));
            } else if ratio >= REVIEW_RATIO && uncompressed_size >= REVIEW_RATIO_MIN_SIZE {
                findings.push(entry_finding(
                    FindingSeverity::Review,
                    "HIGH_EXPANSION_RATIO",
                    index,
                    &name,
                    format!("Entry declares an expansion ratio of at least {ratio}:1."),
                ));
            }
        }

        if entries.len() < MAX_REPORTED_ENTRIES {
            entries.push(ZipEntryObservation {
                index: u64::try_from(index).unwrap_or(u64::MAX),
                name,
                enclosed_path,
                entry_kind,
                compression: format!("{:?}", entry.compression()),
                compressed_size,
                uncompressed_size,
                expansion_ratio,
                encrypted,
                unix_mode: entry.unix_mode(),
                crc32: entry.crc32(),
            });
        }
    }

    if archive_entries > MAX_REPORTED_ENTRIES {
        findings.push(global_finding(
            FindingSeverity::Review,
            "REPORT_ENTRY_LIMIT_REACHED",
            format!(
                "All {archive_entries} entries were evaluated, but only the first {MAX_REPORTED_ENTRIES} are included in the response."
            ),
        ));
    }

    if total_uncompressed_size >= BLOCK_TOTAL_UNCOMPRESSED {
        findings.push(global_finding(
            FindingSeverity::Blocker,
            "TOTAL_UNCOMPRESSED_SIZE_BLOCKED",
            format!(
                "Archive declares {total_uncompressed_size} total uncompressed bytes, exceeding the structural safety limit."
            ),
        ));
    } else if total_uncompressed_size >= REVIEW_TOTAL_UNCOMPRESSED {
        findings.push(global_finding(
            FindingSeverity::Review,
            "TOTAL_UNCOMPRESSED_SIZE_HIGH",
            format!(
                "Archive declares {total_uncompressed_size} total uncompressed bytes; disk capacity and extraction limits require review."
            ),
        ));
    }

    let decision = if findings
        .iter()
        .any(|finding| finding.severity == FindingSeverity::Blocker)
    {
        ArchiveDecision::Block
    } else if findings.is_empty() {
        ArchiveDecision::AcceptStructure
    } else {
        ArchiveDecision::Review
    };

    Ok(ZipInspectionReport {
        requested_path: requested.to_string_lossy().into_owned(),
        canonical_path: canonical.to_string_lossy().into_owned(),
        archive_file_size: metadata.len(),
        archive_entries: u64::try_from(archive_entries).unwrap_or(u64::MAX),
        reported_entries: u64::try_from(entries.len()).unwrap_or(u64::MAX),
        total_compressed_size,
        total_uncompressed_size,
        decision,
        findings,
        entries,
        privacy_mode: "SELECTED_ARCHIVE_METADATA".to_owned(),
        observed_at: Utc::now(),
        limitations: vec![
            "The archive was not extracted and no entry contents were decompressed or executed."
                .to_owned(),
            "CRC integrity, malware, secrets, licenses, and semantic file safety were not evaluated."
                .to_owned(),
            "An ACCEPT_STRUCTURE decision only covers inspected ZIP structure and path metadata; it is not a trust verdict for archive contents."
                .to_owned(),
        ],
    })
}

fn inspect_windows_path(index: usize, name: &str, findings: &mut Vec<ArchiveFinding>) {
    if name.starts_with('/') || name.starts_with('\\') || has_drive_prefix(name) {
        findings.push(entry_finding(
            FindingSeverity::Blocker,
            "WINDOWS_ABSOLUTE_PATH",
            index,
            name,
            "Entry uses an absolute, UNC, or drive-qualified path.",
        ));
    }

    for component in name
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
    {
        if component == ".." {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "PARENT_PATH_COMPONENT",
                index,
                name,
                "Entry contains a parent-directory path component.",
            ));
        }
        if component.ends_with(' ') || component.ends_with('.') {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "WINDOWS_TRAILING_DOT_OR_SPACE",
                index,
                name,
                "A Windows path component ends with a dot or space and may alias another path.",
            ));
        }
        if component
            .chars()
            .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "WINDOWS_INVALID_PATH_CHARACTER",
                index,
                name,
                "A Windows path component contains a reserved character.",
            ));
        }
        if is_windows_reserved_name(component) {
            findings.push(entry_finding(
                FindingSeverity::Blocker,
                "WINDOWS_RESERVED_NAME",
                index,
                name,
                "A path component uses a Windows reserved device name.",
            ));
        }
    }
}

fn has_drive_prefix(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_windows_reserved_name(component: &str) -> bool {
    let trimmed = component.trim_end_matches(|character| character == ' ' || character == '.');
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value))
        || stem
            .strip_prefix("LPT")
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|value| (1..=9).contains(&value))
}

fn normalize_windows_path(name: &str) -> String {
    name.split(['/', '\\'])
        .filter(|component| !component.is_empty() && *component != ".")
        .map(|component| {
            component
                .trim_end_matches(|character| character == ' ' || character == '.')
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn suspicious_extension(name: &str) -> bool {
    let normalized = name.replace('\\', "/");
    let extension = Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "bat"
            | "cmd"
            | "com"
            | "dll"
            | "exe"
            | "js"
            | "jse"
            | "lnk"
            | "msi"
            | "msp"
            | "ps1"
            | "reg"
            | "scr"
            | "vbs"
            | "vbe"
            | "wsf"
    )
}

fn expansion_ratio(uncompressed_size: u64, compressed_size: u64) -> Option<u64> {
    if uncompressed_size == 0 {
        None
    } else if compressed_size == 0 {
        Some(u64::MAX)
    } else {
        Some(uncompressed_size.saturating_add(compressed_size - 1) / compressed_size)
    }
}

fn global_finding(
    severity: FindingSeverity,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ArchiveFinding {
    ArchiveFinding {
        severity,
        code: code.into(),
        entry_index: None,
        entry_name: None,
        message: message.into(),
    }
}

fn entry_finding(
    severity: FindingSeverity,
    code: impl Into<String>,
    index: usize,
    name: &str,
    message: impl Into<String>,
) -> ArchiveFinding {
    ArchiveFinding {
        severity,
        code: code.into(),
        entry_index: Some(u64::try_from(index).unwrap_or(u64::MAX)),
        entry_name: Some(name.to_owned()),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn archive_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "toolos-archive-{label}-{}-{nonce}.zip",
            std::process::id()
        ))
    }

    fn write_archive(label: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, contents) in files {
            writer
                .start_file(*name, options)
                .expect("start ZIP fixture file");
            writer.write_all(contents).expect("write ZIP fixture file");
        }
        let bytes = writer.finish().expect("finish ZIP fixture").into_inner();
        let path = archive_path(label);
        fs::write(&path, bytes).expect("write ZIP fixture");
        path
    }

    #[test]
    fn safe_archive_structure_is_accepted() {
        let path = write_archive("safe", &[("docs/readme.txt", b"hello")]);
        let report = inspect_zip(&path).expect("inspect ZIP");
        assert_eq!(report.decision, ArchiveDecision::AcceptStructure);
        assert!(report.findings.is_empty());
        assert_eq!(report.archive_entries, 1);
        fs::remove_file(path).expect("remove ZIP fixture");
    }

    #[test]
    fn parent_path_is_blocked_without_extraction() {
        let path = write_archive("traversal", &[("../escape.txt", b"blocked")]);
        let report = inspect_zip(&path).expect("inspect ZIP");
        assert_eq!(report.decision, ArchiveDecision::Block);
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.code == "PATH_NOT_ENCLOSED"));
        fs::remove_file(path).expect("remove ZIP fixture");
    }

    #[test]
    fn case_insensitive_windows_collision_is_blocked() {
        let path = write_archive(
            "collision",
            &[
                ("Folder/Readme.txt", b"first"),
                ("folder/README.TXT", b"second"),
            ],
        );
        let report = inspect_zip(&path).expect("inspect ZIP");
        assert_eq!(report.decision, ArchiveDecision::Block);
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.code == "WINDOWS_PATH_COLLISION"));
        fs::remove_file(path).expect("remove ZIP fixture");
    }
}
