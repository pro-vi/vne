use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub mod cli;
pub mod git;

pub use git::EnvFileGitStatus;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub root: String,
    pub files: Vec<EnvFile>,
    /// Discovered candidates that could NOT be inspected, with a typed
    /// reason — a scan is never "complete" while this is silently empty
    /// (VNE-SEC-010). Each record also surfaces as a warning finding.
    pub incomplete: Vec<IncompleteEnvFile>,
    pub comparison: Option<EnvComparison>,
    pub layer_report: EnvLayerReport,
    pub framework_profiles: Vec<FrameworkEnvProfile>,
    pub findings: Vec<EnvFinding>,
}

/// Terminal non-read outcome for one discovered candidate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IncompleteEnvFile {
    pub name: String,
    /// "unreadable" | "invalid-utf8" | "oversize" | "count-limit" | "scan-timeout"
    pub reason: &'static str,
}

/// Scan bounds (VNE-SEC-014 scan half): explicit, tunable, enforced.
pub const SCAN_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const SCAN_MAX_FILES: usize = 500;
pub const SCAN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvFileCreationDisposition {
    Created,
    AlreadyExists,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnsureEnvFileOutcome {
    pub disposition: EnvFileCreationDisposition,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvKeyCopyDisposition {
    Added,
    Overwritten,
    AlreadyPresent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvKeyCopyOutcome {
    pub disposition: EnvKeyCopyDisposition,
    pub source_path: String,
    pub destination_path: String,
    pub key: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvKeySetDisposition {
    Updated,
    AlreadyPresent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvKeySetOutcome {
    pub disposition: EnvKeySetDisposition,
    pub path: String,
    pub key: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvKeyRemoveDisposition {
    Removed,
    RemovedAll,
    AlreadyAbsent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvKeyRemoveOutcome {
    pub disposition: EnvKeyRemoveDisposition,
    pub path: String,
    pub key: String,
    pub removed_lines: Vec<usize>,
}

/// Which occurrences of a key `vne rm` may delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKeyRemoveSelector {
    /// The one occurrence; a duplicated key is refused.
    Only,
    /// Exactly the occurrence at this line; a line holding anything else fails.
    Line(usize),
    /// Every occurrence, in one pass.
    All,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvKeyRenameDisposition {
    Renamed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvKeyRenamePair {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvKeyRenameOutcome {
    pub disposition: EnvKeyRenameDisposition,
    pub path: String,
    pub keys: EnvKeyRenamePair,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExampleSyncDisposition {
    Created,
    Updated,
    AlreadyCurrent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExampleSyncOutcome {
    pub disposition: ExampleSyncDisposition,
    pub source_path: String,
    pub example_path: String,
    pub added_keys: Vec<String>,
}

/// The caller's belief about the key before the removal runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKeyExpectation {
    Present,
    Absent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvFile {
    pub path: String,
    pub name: String,
    pub discovery_reasons: Vec<String>,
    pub entries: Vec<EnvEntry>,
    pub diagnostics: Vec<String>,
    pub duplicate_keys: Vec<String>,
    /// Exposure of this file to git. The pure parser cannot know it and leaves
    /// `Unknown`; the filesystem-aware loaders fill it in.
    pub git_status: EnvFileGitStatus,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub id: String,
    pub key: String,
    pub value: String,
    pub display_value: String,
    pub line_number: usize,
    pub exported: bool,
    pub quote: Option<String>,
    pub comment: Option<String>,
    pub shape: KeyShape,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyShape {
    pub kind: String,
    pub label: String,
    pub confidence: String,
    pub redacted_by_default: bool,
    pub sensitive: bool,
    pub exposure: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvComparison {
    pub base_path: String,
    pub example_path: String,
    pub missing_keys: Vec<String>,
    pub extra_keys: Vec<String>,
    pub shared_keys: Vec<String>,
    pub duplicate_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerReport {
    pub ordered_files: Vec<EnvLayerFile>,
    pub overrides: Vec<EnvLayerOverride>,
    pub placeholder_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerFile {
    pub path: String,
    pub name: String,
    pub layer_kind: String,
    pub precedence: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvLayerOverride {
    pub key: String,
    pub files: Vec<String>,
    pub effective_file: String,
    pub conflict: bool,
    pub redacted: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkEnvProfile {
    pub framework: String,
    pub mode: String,
    pub evidence: Vec<String>,
    pub ordered_files: Vec<FrameworkEnvFile>,
    pub missing_files: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FrameworkEnvFile {
    pub path: String,
    pub name: String,
    pub layer_kind: String,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvFinding {
    pub severity: String,
    pub action_kind: String,
    pub title: String,
    pub detail: String,
    pub evidence: Vec<String>,
    pub mutation_preview: Option<String>,
    pub file_path: Option<String>,
    pub key: Option<String>,
    pub line_number: Option<usize>,
    pub entry_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLine {
    key: String,
    value: String,
    line_number: usize,
    exported: bool,
    quote: Option<char>,
    comment: Option<String>,
    /// Byte offset where this entry's line begins, after any leading byte-order
    /// mark. Paired with `next_start` it spans the whole line for removal.
    line_start: usize,
    /// Byte offsets of the key token itself, for renaming it in place.
    key_start: usize,
    key_end: usize,
    value_start: usize,
    value_end: usize,
    next_start: usize,
    diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvEntryTarget {
    file_path: String,
    line_number: usize,
    entry_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendEnvKeyError {
    InvalidKey,
    EmptyValue,
    Duplicate { line_numbers: Vec<usize> },
}

impl AppendEnvKeyError {
    pub fn message(&self, key: &str) -> String {
        match self {
            Self::InvalidKey => invalid_key_message(key),
            Self::EmptyValue => format!("`{key}` needs a value before it can be added"),
            Self::Duplicate { line_numbers } => {
                let lines = line_numbers
                    .iter()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let label = if line_numbers.len() == 1 {
                    "line"
                } else {
                    "lines"
                };
                format!("`{key}` already exists at {label} {lines}; edit the existing occurrence instead")
            }
        }
    }
}

impl fmt::Display for AppendEnvKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str("invalid env key"),
            Self::EmptyValue => formatter.write_str("empty env value"),
            Self::Duplicate { .. } => formatter.write_str("duplicate env key"),
        }
    }
}

impl std::error::Error for AppendEnvKeyError {}

#[derive(Debug)]
pub enum EnvKeyCopyError {
    InvalidKey,
    SameFile,
    SourceMissing,
    SourceDuplicate {
        line_numbers: Vec<usize>,
    },
    SourceMalformed {
        line_number: usize,
    },
    DestinationDuplicate {
        line_numbers: Vec<usize>,
    },
    DestinationMalformed {
        line_number: usize,
    },
    DestinationConflict {
        line_number: usize,
    },
    NonRegularFile {
        role: &'static str,
        path: PathBuf,
    },
    SymlinkComponent {
        role: &'static str,
        path: PathBuf,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl EnvKeyCopyError {
    pub fn message(&self, key: &str) -> String {
        match self {
            Self::InvalidKey => invalid_key_message(key),
            Self::SameFile => "source and destination resolve to the same file".to_string(),
            Self::SourceMissing => format!("source key `{key}` was not found"),
            Self::SourceDuplicate { line_numbers } => format!(
                "source key `{key}` is ambiguous at {}; remove duplicates before copying",
                line_number_label(line_numbers)
            ),
            Self::SourceMalformed { line_number } => format!(
                "source key `{key}` has a malformed value at line {line_number}"
            ),
            Self::DestinationDuplicate { line_numbers } => format!(
                "destination key `{key}` is ambiguous at {}; remove duplicates before copying",
                line_number_label(line_numbers)
            ),
            Self::DestinationMalformed { line_number } => format!(
                "destination key `{key}` has a malformed value at line {line_number}"
            ),
            Self::DestinationConflict { line_number } => format!(
                "destination key `{key}` already has a different value at line {line_number}; pass --overwrite to replace it"
            ),
            Self::NonRegularFile { role, path } => {
                format!("{role} env file {} is not a regular file", path.display())
            }
            Self::SymlinkComponent { role, path } => {
                format!("{role} path crosses a symlink: {}; mutations refuse symlinks", path.display())
            }
            Self::Io {
                action,
                path,
                source,
            } => format!("could not {action} env file {}: {source}", path.display()),
        }
    }
}

impl fmt::Display for EnvKeyCopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("env key copy failed")
    }
}

impl std::error::Error for EnvKeyCopyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum EnvFileAccessError {
    NonRegularFile {
        role: &'static str,
        path: PathBuf,
    },
    SymlinkComponent {
        role: &'static str,
        path: PathBuf,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl EnvFileAccessError {
    pub fn message(&self) -> String {
        match self {
            Self::NonRegularFile { role, path } => {
                format!("{role} env file {} is not a regular file", path.display())
            }
            Self::SymlinkComponent { role, path } => {
                format!("{role} path crosses a symlink: {}; mutations refuse symlinks", path.display())
            }
            Self::Io {
                action,
                path,
                source,
            } => format!("could not {action} env file {}: {source}", path.display()),
        }
    }
}

impl From<EnvFileAccessError> for EnvKeyCopyError {
    fn from(error: EnvFileAccessError) -> Self {
        match error {
            EnvFileAccessError::SymlinkComponent { role, path } => {
                EnvKeyCopyError::SymlinkComponent { role, path }
            }
            EnvFileAccessError::NonRegularFile { role, path } => {
                Self::NonRegularFile { role, path }
            }
            EnvFileAccessError::Io {
                action,
                path,
                source,
            } => Self::Io {
                action,
                path,
                source,
            },
        }
    }
}

#[derive(Debug)]
pub enum ExampleSyncError {
    SameFile,
    UnusableExamplePath,
    Access(EnvFileAccessError),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl ExampleSyncError {
    pub fn message(&self) -> String {
        match self {
            Self::SameFile => "source and example resolve to the same file".to_string(),
            Self::UnusableExamplePath => {
                "example path must name a file inside an existing directory".to_string()
            }
            Self::Access(error) => error.message(),
            Self::Io {
                action,
                path,
                source,
            } => format!(
                "could not {action} example file {}: {source}",
                path.display()
            ),
        }
    }
}

impl fmt::Display for ExampleSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("example sync failed")
    }
}

impl std::error::Error for ExampleSyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::Access(EnvFileAccessError::Io { source, .. }) => {
                Some(source)
            }
            _ => None,
        }
    }
}

impl From<EnvFileAccessError> for ExampleSyncError {
    fn from(error: EnvFileAccessError) -> Self {
        Self::Access(error)
    }
}

#[derive(Debug)]
pub enum EnvKeyRenameError {
    InvalidSourceKey,
    InvalidTargetKey,
    SameKey,
    SourceMissing,
    SourceDuplicate {
        line_numbers: Vec<usize>,
    },
    SourceMalformed {
        line_number: usize,
    },
    TargetExists {
        line_numbers: Vec<usize>,
    },
    /// The source is gone and the target is present. A rename cannot be
    /// confirmed from file state alone, so vne refuses to claim one happened.
    Indeterminate {
        line_numbers: Vec<usize>,
    },
    Access(EnvFileAccessError),
}

impl EnvKeyRenameError {
    pub fn message(&self, from: &str, to: &str) -> String {
        match self {
            Self::InvalidSourceKey => invalid_key_message(from),
            Self::InvalidTargetKey => invalid_key_message(to),
            Self::SameKey => format!("`{from}` and `{to}` are the same key"),
            Self::SourceMissing => format!("`{from}` was not found"),
            Self::SourceDuplicate { line_numbers } => format!(
                "`{from}` is ambiguous at {}; remove duplicates before renaming it",
                line_number_label(line_numbers)
            ),
            Self::SourceMalformed { line_number } => {
                format!("`{from}` has a malformed value at line {line_number}")
            }
            Self::TargetExists { line_numbers } => format!(
                "`{to}` already exists at {}; remove it before renaming `{from}` onto it",
                line_number_label(line_numbers)
            ),
            Self::Indeterminate { line_numbers } => format!(
                "`{from}` is absent and `{to}` is present at {}; vne cannot confirm from file state that this rename already happened",
                line_number_label(line_numbers)
            ),
            Self::Access(error) => error.message(),
        }
    }
}

impl fmt::Display for EnvKeyRenameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("env key rename failed")
    }
}

impl std::error::Error for EnvKeyRenameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access(EnvFileAccessError::Io { source, .. }) => Some(source),
            _ => None,
        }
    }
}

impl From<EnvFileAccessError> for EnvKeyRenameError {
    fn from(error: EnvFileAccessError) -> Self {
        Self::Access(error)
    }
}

#[derive(Debug)]
pub enum EnvKeyRemoveError {
    InvalidKey,
    Duplicate {
        line_numbers: Vec<usize>,
    },
    /// The entry parses badly, so its extent is not trustworthy. An unclosed
    /// quote, for one, makes the parser read to end of file.
    Malformed {
        line_number: usize,
    },
    StaleSelector {
        line_number: usize,
        found_key: Option<String>,
    },
    ExpectedPresent,
    ExpectedAbsent {
        line_numbers: Vec<usize>,
    },
    Access(EnvFileAccessError),
}

impl EnvKeyRemoveError {
    pub fn message(&self, key: &str) -> String {
        match self {
            Self::InvalidKey => invalid_key_message(key),
            Self::Duplicate { line_numbers } => format!(
                "`{key}` is ambiguous at {}; pass --line <N> or --all",
                line_number_label(line_numbers)
            ),
            Self::Malformed { line_number } => format!(
                "`{key}` has a malformed value at line {line_number}; its extent is unclear, so vne will not remove it"
            ),
            Self::StaleSelector {
                line_number,
                found_key: Some(found_key),
            } => format!(
                "line {line_number} holds `{found_key}`, not `{key}`; re-read the file before selecting a line"
            ),
            Self::StaleSelector {
                line_number,
                found_key: None,
            } => format!(
                "line {line_number} holds no env assignment; re-read the file before selecting a line"
            ),
            Self::ExpectedPresent => {
                format!("`{key}` is absent but --expect present was requested")
            }
            Self::ExpectedAbsent { line_numbers } => format!(
                "`{key}` is present at {} but --expect absent was requested",
                line_number_label(line_numbers)
            ),
            Self::Access(error) => error.message(),
        }
    }
}

impl fmt::Display for EnvKeyRemoveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("env key removal failed")
    }
}

impl std::error::Error for EnvKeyRemoveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access(EnvFileAccessError::Io { source, .. }) => Some(source),
            _ => None,
        }
    }
}

impl From<EnvFileAccessError> for EnvKeyRemoveError {
    fn from(error: EnvFileAccessError) -> Self {
        Self::Access(error)
    }
}

#[derive(Debug)]
pub enum EnvKeySetError {
    InvalidKey,
    /// The new value is empty or whitespace, which a failed pipeline produces
    /// as readily as a deliberate clear.
    EmptyValue,
    Missing,
    Duplicate {
        line_numbers: Vec<usize>,
    },
    Malformed {
        line_number: usize,
    },
    Access(EnvFileAccessError),
}

impl EnvKeySetError {
    pub fn message(&self, key: &str) -> String {
        match self {
            Self::InvalidKey => invalid_key_message(key),
            Self::EmptyValue => format!(
                "`{key}` was given an empty value; pass --allow-empty to store one, or use `vne rm` to remove the key"
            ),
            Self::Missing => {
                format!("`{key}` was not found; use `vne add` to create it")
            }
            Self::Duplicate { line_numbers } => format!(
                "`{key}` is ambiguous at {}; remove duplicates before setting it",
                line_number_label(line_numbers)
            ),
            Self::Malformed { line_number } => {
                format!("`{key}` has a malformed value at line {line_number}")
            }
            Self::Access(error) => error.message(),
        }
    }
}

impl fmt::Display for EnvKeySetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("env key set failed")
    }
}

impl std::error::Error for EnvKeySetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Access(EnvFileAccessError::Io { source, .. }) => Some(source),
            _ => None,
        }
    }
}

impl From<EnvFileAccessError> for EnvKeySetError {
    fn from(error: EnvFileAccessError) -> Self {
        Self::Access(error)
    }
}

/// Renders a rejected key for an error message without echoing a value.
///
/// A caller who pastes `KEY=secret` where a key name belongs must not have that
/// secret copied into stderr, where shells, CI logs, and agent transcripts keep
/// it. Everything after the first `=` is elided.
pub(crate) fn invalid_key_message(key: &str) -> String {
    match key.split_once('=') {
        Some((name, _)) => format!(
            "`{name}=…` looks like a KEY=VALUE assignment; pass the key name alone (the value is not echoed)"
        ),
        None => format!("`{key}` is not a valid env key"),
    }
}

pub(crate) fn line_number_label(line_numbers: &[usize]) -> String {
    let lines = line_numbers
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if line_numbers.len() == 1 {
        format!("line {lines}")
    } else {
        format!("lines {lines}")
    }
}

const RAW_PREVIEW_WITHHELD: &str =
    "[raw preview withheld because this file contains redacted or secret-like text]";
const INLINE_COMMENT_WITHHELD: &str = "# [secret-like comment withheld]";
const ALL_VALUES_WITHHELD: &str = "[value withheld by output policy]";
const ALL_COMMENTS_WITHHELD: &str = "# [comment withheld by output policy]";
const ALL_CONTENT_WITHHELD: &str = "[raw content withheld by output policy]";
const SECRET_KEY_MARKERS: &[&str] = &[
    "SECRET",
    "PASSWORD",
    "TOKEN",
    "PRIVATE_KEY",
    "API_KEY",
    "ACCESS_KEY",
    "CLIENT_SECRET",
    "WEBHOOK_SECRET",
    "CREDENTIAL",
    "SERVICE_ROLE_KEY",
];
const SECRET_VALUE_PREFIXES: &[&str] = &[
    "sk-", "sk_", "ghp_", "xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-", "AKIA",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownKeyProfile {
    Secret(&'static str),
    Public(&'static str),
}

#[derive(Clone)]
struct InitialProjectPath(Option<String>);

#[tauri::command]
fn initial_project_path(path: tauri::State<'_, InitialProjectPath>) -> Option<String> {
    path.0.clone()
}

#[tauri::command]
fn load_project(path: String) -> Result<ProjectSnapshot, String> {
    snapshot_project(Path::new(&path))
        .map(redact_snapshot)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_env_value(
    root: String,
    path: String,
    key: String,
    line_number: usize,
) -> Result<String, String> {
    let path = resolve_project_file(&root, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;

    parse_env_entries(&content)
        .into_iter()
        .find(|entry| entry.key == key && entry.line_number == line_number)
        .map(|entry| entry.value)
        .ok_or_else(|| format!("Key `{key}` at line {line_number} was not found"))
}

#[tauri::command]
fn save_env_value(
    root: String,
    path: String,
    key: String,
    line_number: usize,
    value: String,
) -> Result<ProjectSnapshot, String> {
    let root_path = Path::new(&root)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = resolve_project_file(&root, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated = replace_env_value_at(&content, &key, line_number, &value)
        .ok_or_else(|| format!("Key `{key}` at line {line_number} was not found"))?;

    atomic_write_metadata_if_unchanged(&path, &updated, Some(&content)).map_err(|error| error.to_string())?;
    snapshot_project(&root_path)
        .map(redact_snapshot)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_env_key(
    root: String,
    path: String,
    key: String,
    value: String,
) -> Result<ProjectSnapshot, String> {
    let root_path = Path::new(&root)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let path = resolve_project_file(&root, &path)?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated =
        append_env_key_result(&content, &key, &value).map_err(|error| error.message(&key))?;

    atomic_write_metadata_if_unchanged(&path, &updated, Some(&content)).map_err(|error| error.to_string())?;
    snapshot_project(&root_path)
        .map(redact_snapshot)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn ensure_env_file(root: String, name: String) -> Result<EnsureEnvFileOutcome, String> {
    ensure_project_env_file(Path::new(&root), &name).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_with_initial_project_path(None);
}

pub fn run_with_initial_project_path(initial_path: Option<String>) {
    tauri::Builder::default()
        .manage(InitialProjectPath(initial_path))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            initial_project_path,
            load_project,
            reveal_env_value,
            save_env_value,
            add_env_key,
            ensure_env_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running vne");
}

pub fn snapshot_project(root: &Path) -> io::Result<ProjectSnapshot> {
    let root = root.canonicalize()?;
    let mut discovered = BTreeMap::<PathBuf, BTreeSet<String>>::new();

    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if is_env_file_name(&name) {
                add_discovered_env_file(
                    &root,
                    entry.path(),
                    format!("direct env filename `{name}`"),
                    &mut discovered,
                );
            }
        }
    }

    discover_package_json_env_files(&root, &mut discovered);
    discover_compose_env_files(&root, &mut discovered);

    let mut env_paths = discovered.into_iter().collect::<Vec<_>>();
    env_paths.sort_by_key(|(path, _)| {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        (
            env_file_sort_key(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            ),
            relative.to_string_lossy().to_string(),
        )
    });

    // Every discovered candidate resolves to exactly one terminal outcome:
    // a parsed file or an explicit incomplete record (VNE-SEC-010).
    let scan_started = std::time::Instant::now();
    let mut files = Vec::new();
    let mut incomplete = Vec::new();
    for (index, (path, reasons)) in env_paths.into_iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        if index >= SCAN_MAX_FILES {
            incomplete.push(IncompleteEnvFile {
                name,
                reason: "count-limit",
            });
            continue;
        }
        if scan_started.elapsed() >= SCAN_DEADLINE {
            incomplete.push(IncompleteEnvFile {
                name,
                reason: "scan-timeout",
            });
            continue;
        }
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                incomplete.push(IncompleteEnvFile {
                    name,
                    reason: "unreadable",
                });
                continue;
            }
        };
        if meta.len() > SCAN_MAX_FILE_BYTES {
            incomplete.push(IncompleteEnvFile {
                name,
                reason: "oversize",
            });
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut file =
                    parse_env_file_with_reasons(&path, content, reasons.into_iter().collect());
                file.git_status = git::env_file_git_status(&path);
                files.push(file);
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                incomplete.push(IncompleteEnvFile {
                    name,
                    reason: "invalid-utf8",
                });
            }
            Err(_) => {
                incomplete.push(IncompleteEnvFile {
                    name,
                    reason: "unreadable",
                });
            }
        }
    }

    let comparison = compare_actual_to_example(&files);
    let layer_report = build_layer_report(&files);
    let framework_profiles = build_framework_profiles(&root, &files);
    let mut findings = build_findings(
        comparison.as_ref(),
        &layer_report,
        &framework_profiles,
        &files,
    );
    for record in &incomplete {
        findings.push(EnvFinding {
            severity: "warning".into(),
            action_kind: "incompleteScan".into(),
            title: format!("`{}` was not inspected ({})", record.name, record.reason),
            detail: "The scan is incomplete: this discovered env file has no inspection result.".into(),
            evidence: vec![record.name.clone()],
            mutation_preview: None,
            file_path: None,
            key: None,
            line_number: None,
            entry_id: None,
        });
    }

    Ok(ProjectSnapshot {
        root: root.to_string_lossy().to_string(),
        files,
        incomplete,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    })
}

pub fn redact_snapshot(mut snapshot: ProjectSnapshot) -> ProjectSnapshot {
    for file in &mut snapshot.files {
        redact_env_file_in_place(file);
    }

    snapshot
}

pub fn redact_env_file(mut file: EnvFile) -> EnvFile {
    redact_env_file_in_place(&mut file);
    file
}

pub fn withhold_all_snapshot_payloads(snapshot: ProjectSnapshot) -> ProjectSnapshot {
    let ProjectSnapshot {
        root,
        files,
        incomplete,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    } = snapshot;

    ProjectSnapshot {
        root,
        files: files
            .into_iter()
            .map(withhold_all_env_file_payloads)
            .collect(),
        incomplete,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    }
}

pub fn redact_snapshot_values_only(snapshot: ProjectSnapshot) -> ProjectSnapshot {
    let ProjectSnapshot {
        root,
        files,
        incomplete,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    } = snapshot;

    ProjectSnapshot {
        root,
        files: files.into_iter().map(redact_env_file_values_only).collect(),
        incomplete,
        comparison,
        layer_report,
        framework_profiles,
        findings,
    }
}

pub fn redact_env_file_values_only(file: EnvFile) -> EnvFile {
    let EnvFile {
        path,
        name,
        discovery_reasons,
        entries,
        diagnostics,
        duplicate_keys,
        git_status,
        content: _,
    } = redact_env_file(file);

    EnvFile {
        path,
        name,
        discovery_reasons,
        entries: entries
            .into_iter()
            .map(withhold_env_entry_context)
            .collect(),
        diagnostics,
        duplicate_keys,
        git_status,
        content: ALL_CONTENT_WITHHELD.to_string(),
    }
}

pub fn withhold_all_env_file_payloads(file: EnvFile) -> EnvFile {
    let EnvFile {
        path,
        name,
        discovery_reasons,
        entries,
        diagnostics,
        duplicate_keys,
        git_status,
        content: _,
    } = file;

    EnvFile {
        path,
        name,
        discovery_reasons,
        entries: entries
            .into_iter()
            .map(withhold_all_env_entry_payloads)
            .collect(),
        diagnostics,
        duplicate_keys,
        git_status,
        content: ALL_CONTENT_WITHHELD.to_string(),
    }
}

fn withhold_env_entry_context(entry: EnvEntry) -> EnvEntry {
    let EnvEntry {
        id,
        key,
        value,
        display_value,
        line_number,
        exported,
        quote,
        comment,
        shape,
        diagnostics,
    } = entry;

    EnvEntry {
        id,
        key,
        value,
        display_value,
        line_number,
        exported,
        quote,
        comment: comment.map(|_| ALL_COMMENTS_WITHHELD.to_string()),
        shape,
        diagnostics,
    }
}

fn withhold_all_env_entry_payloads(entry: EnvEntry) -> EnvEntry {
    let EnvEntry {
        id,
        key,
        value: _,
        display_value: _,
        line_number,
        exported,
        quote,
        comment,
        shape,
        diagnostics,
    } = entry;

    EnvEntry {
        id,
        key,
        value: ALL_VALUES_WITHHELD.to_string(),
        display_value: ALL_VALUES_WITHHELD.to_string(),
        line_number,
        exported,
        quote,
        comment: comment.map(|_| ALL_COMMENTS_WITHHELD.to_string()),
        shape,
        diagnostics,
    }
}

fn redact_env_file_in_place(file: &mut EnvFile) {
    let mut has_redacted_entry = false;
    for entry in &mut file.entries {
        if entry.shape.redacted_by_default {
            entry.value.clear();
            entry.display_value = "********".to_string();
            has_redacted_entry = true;
        }

        let withhold_comment = entry.comment.as_deref().is_some_and(|comment| {
            entry.shape.redacted_by_default || contains_secretish_text(comment)
        });
        if withhold_comment {
            entry.comment = Some(INLINE_COMMENT_WITHHELD.to_string());
        }
    }

    if has_redacted_entry || content_has_unparsed_secretish_text(&file.content) {
        file.content = RAW_PREVIEW_WITHHELD.to_string();
    }
}

fn content_has_unparsed_secretish_text(content: &str) -> bool {
    let entries = parse_env_entries(content);
    if entries.iter().any(|entry| {
        entry
            .comment
            .as_deref()
            .is_some_and(contains_secretish_text)
    }) {
        return true;
    }

    let parsed_lines = entries
        .iter()
        .map(|entry| entry.line_number)
        .collect::<BTreeSet<_>>();
    content.lines().enumerate().any(|(index, line)| {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        (!parsed_lines.contains(&line_number) || trimmed.starts_with('#'))
            && contains_secretish_text(line)
    })
}

fn contains_secretish_text(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    contains_secretish_key_name(&upper)
        || upper.contains("-----BEGIN")
        || value_has_credential_signature(text)
}

pub fn parse_env_file(path: &Path, content: String) -> EnvFile {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let discovery_reasons = if is_env_file_name(&name) {
        vec![format!("direct env filename `{name}`")]
    } else {
        vec!["opened directly".to_string()]
    };

    parse_env_file_with_reasons(path, content, discovery_reasons)
}

fn parse_env_file_with_reasons(
    path: &Path,
    content: String,
    discovery_reasons: Vec<String>,
) -> EnvFile {
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicate_keys = BTreeSet::new();

    for parsed in parse_env_entries(&content) {
        if !seen.insert(parsed.key.clone()) {
            duplicate_keys.insert(parsed.key.clone());
        }
        if let Some(diagnostic) = &parsed.diagnostic {
            diagnostics.push(format!("Line {}: {diagnostic}", parsed.line_number));
        }

        let shape = infer_key_shape(&parsed.key, &parsed.value);
        let display_value = if shape.redacted_by_default {
            "********".to_string()
        } else {
            parsed.value.clone()
        };

        entries.push(EnvEntry {
            id: entry_id(&parsed.key, parsed.line_number),
            key: parsed.key,
            value: parsed.value,
            display_value,
            line_number: parsed.line_number,
            exported: parsed.exported,
            quote: parsed.quote.map(|quote| quote.to_string()),
            comment: parsed.comment,
            shape,
            diagnostics: parsed.diagnostic.into_iter().collect(),
        });
    }

    for key in &duplicate_keys {
        diagnostics.push(format!("Duplicate key `{key}`"));
    }

    EnvFile {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        discovery_reasons,
        entries,
        diagnostics,
        duplicate_keys: duplicate_keys.into_iter().collect(),
        git_status: EnvFileGitStatus::Unknown,
        content,
    }
}

pub fn replace_env_value(content: &str, key: &str, value: &str) -> Option<String> {
    replace_env_entry(content, value, |parsed| parsed.key == key)
}

pub fn replace_env_value_at(
    content: &str,
    key: &str,
    line_number: usize,
    value: &str,
) -> Option<String> {
    replace_env_entry(content, value, |parsed| {
        parsed.key == key && parsed.line_number == line_number
    })
}

fn replace_env_entry<F>(content: &str, value: &str, matches_entry: F) -> Option<String>
where
    F: Fn(&ParsedLine) -> bool,
{
    let mut output = String::with_capacity(content.len() + value.len());
    let mut replaced = false;
    let mut cursor = 0;

    for parsed in parse_env_entries(content) {
        if matches_entry(&parsed) {
            let replacement = match parsed.quote {
                Some('"') => escape_double_quoted_value(value),
                Some('\'') => escape_single_quoted_value(value),
                Some(_) => escape_double_quoted_value(value),
                None if needs_quotes(value) => format!("\"{}\"", escape_double_quoted_value(value)),
                None => value.to_string(),
            };

            output.push_str(&content[cursor..parsed.value_start]);
            output.push_str(&replacement);
            cursor = parsed.value_end;
            replaced = true;
            break;
        }
    }

    output.push_str(&content[cursor..]);
    replaced.then_some(output)
}

pub fn append_env_key(content: &str, key: &str, value: &str) -> Option<String> {
    append_env_key_result(content, key, value).ok()
}

pub fn append_env_key_result(
    content: &str,
    key: &str,
    value: &str,
) -> Result<String, AppendEnvKeyError> {
    if !is_valid_env_key(key) {
        return Err(AppendEnvKeyError::InvalidKey);
    }

    if value.trim().is_empty() {
        return Err(AppendEnvKeyError::EmptyValue);
    }

    let duplicate_lines = parse_env_entries(content)
        .iter()
        .filter(|entry| entry.key == key)
        .map(|entry| entry.line_number)
        .collect::<Vec<_>>();
    if !duplicate_lines.is_empty() {
        return Err(AppendEnvKeyError::Duplicate {
            line_numbers: duplicate_lines,
        });
    }

    let mut output = String::with_capacity(content.len() + key.len() + value.len() + 4);
    output.push_str(content);
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(key);
    output.push('=');
    if !value.is_empty() {
        if needs_quotes(value) {
            output.push('"');
            output.push_str(&escape_double_quoted_value(value));
            output.push('"');
        } else {
            output.push_str(value);
        }
    }
    output.push('\n');

    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnvValueToken<'a> {
    text: &'a str,
}

pub fn copy_env_key(
    source_path: &Path,
    key: &str,
    destination_path: &Path,
    overwrite: bool,
) -> Result<EnvKeyCopyOutcome, EnvKeyCopyError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeyCopyError::InvalidKey);
    }

    let source_path = canonical_regular_file(source_path, "source")?;
    let destination_path = canonical_regular_file(destination_path, "destination")?;
    if source_path == destination_path {
        return Err(EnvKeyCopyError::SameFile);
    }

    let source_content = read_env_file_at(&source_path)?;
    let destination_content = read_env_file_at(&destination_path)?;
    let (updated, disposition) =
        copy_env_key_content(&source_content, &destination_content, key, overwrite)?;

    if disposition != EnvKeyCopyDisposition::AlreadyPresent {
        atomic_write_metadata_if_unchanged(&destination_path, &updated, Some(&destination_content)).map_err(|source| {
            EnvKeyCopyError::Io {
                action: "write",
                path: destination_path.clone(),
                source,
            }
        })?;
    }

    Ok(EnvKeyCopyOutcome {
        disposition,
        source_path: source_path.to_string_lossy().to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        key: key.to_string(),
    })
}

/// O_NOFOLLOW constant by platform (std does not export it; adding libc for
/// one constant is not worth the dependency). Verified against macOS
/// /usr/include/sys/fcntl.h and Linux include/uapi/asm-generic/fcntl.h.
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = 0x0040_0000;
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0o400_000;

/// Mutation-target guard (VNE-SEC-005): refuse a symlinked final
/// component for every mutation, then verify identity with a no-follow
/// open whose descriptor (dev, ino) must match the pre-check stat
/// (VNE-SEC-013's final-component identity check). Directory-component
/// symlink refusal is deliberately NOT implemented here: ambient system
/// symlinks (/var -> /private/var on macOS) sit above any anchor this
/// layer can attribute; that attribution arrives with O5's backend-owned
/// authorized root. The audit's executed damage (final-component link
/// materialization) is fully covered by this guard.
pub fn ensure_mutation_target(path: &Path, role: &'static str) -> Result<(), EnvFileAccessError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let stat = fs::symlink_metadata(path).map_err(|source| EnvFileAccessError::Io {
            action: "inspect",
            path: path.to_path_buf(),
            source,
        })?;
        if stat.file_type().is_symlink() {
            return Err(EnvFileAccessError::SymlinkComponent {
                role,
                path: path.to_path_buf(),
            });
        }
        if !stat.is_file() {
            return Err(EnvFileAccessError::NonRegularFile {
                role,
                path: path.to_path_buf(),
            });
        }
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(|source| EnvFileAccessError::Io {
                action: "open",
                path: path.to_path_buf(),
                source,
            })?;
        let fd_meta = opened.metadata().map_err(|source| EnvFileAccessError::Io {
            action: "inspect",
            path: path.to_path_buf(),
            source,
        })?;
        if fd_meta.dev() != stat.dev() || fd_meta.ino() != stat.ino() {
            return Err(EnvFileAccessError::NonRegularFile {
                role,
                path: path.to_path_buf(),
            });
        }
    }
    #[cfg(not(unix))]
    let _ = (path, role);
    Ok(())
}

fn canonical_regular_file(path: &Path, role: &'static str) -> Result<PathBuf, EnvFileAccessError> {
    ensure_mutation_target(path, role)?;
    let canonical = fs::canonicalize(path).map_err(|source| EnvFileAccessError::Io {
        action: "resolve",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|source| EnvFileAccessError::Io {
        action: "inspect",
        path: canonical.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(EnvFileAccessError::NonRegularFile {
            role,
            path: canonical,
        });
    }
    Ok(canonical)
}

fn read_env_file_at(path: &Path) -> Result<String, EnvFileAccessError> {
    fs::read_to_string(path).map_err(|source| EnvFileAccessError::Io {
        action: "read",
        path: path.to_path_buf(),
        source,
    })
}

fn copy_env_key_content(
    source_content: &str,
    destination_content: &str,
    key: &str,
    overwrite: bool,
) -> Result<(String, EnvKeyCopyDisposition), EnvKeyCopyError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeyCopyError::InvalidKey);
    }

    let source_entries = parse_env_entries(source_content)
        .into_iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    let source = match source_entries.as_slice() {
        [] => return Err(EnvKeyCopyError::SourceMissing),
        [source] => source,
        sources => {
            return Err(EnvKeyCopyError::SourceDuplicate {
                line_numbers: sources.iter().map(|entry| entry.line_number).collect(),
            })
        }
    };
    if source.diagnostic.is_some() {
        return Err(EnvKeyCopyError::SourceMalformed {
            line_number: source.line_number,
        });
    }
    let source_token = env_value_token(source_content, source);

    let destination_entries = parse_env_entries(destination_content)
        .into_iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    let Some(destination) = destination_entries.first() else {
        return Ok((
            append_env_key_token(destination_content, key, source_token),
            EnvKeyCopyDisposition::Added,
        ));
    };
    if destination_entries.len() > 1 {
        return Err(EnvKeyCopyError::DestinationDuplicate {
            line_numbers: destination_entries
                .iter()
                .map(|entry| entry.line_number)
                .collect(),
        });
    }
    if destination.diagnostic.is_some() {
        return Err(EnvKeyCopyError::DestinationMalformed {
            line_number: destination.line_number,
        });
    }

    let destination_token = env_value_token(destination_content, destination);
    if source_token == destination_token {
        return Ok((
            destination_content.to_string(),
            EnvKeyCopyDisposition::AlreadyPresent,
        ));
    }
    if !overwrite {
        return Err(EnvKeyCopyError::DestinationConflict {
            line_number: destination.line_number,
        });
    }

    Ok((
        replace_env_value_token(destination_content, destination, source_token),
        EnvKeyCopyDisposition::Overwritten,
    ))
}

pub fn set_env_key(
    path: &Path,
    key: &str,
    value: &str,
    allow_empty: bool,
) -> Result<EnvKeySetOutcome, EnvKeySetError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeySetError::InvalidKey);
    }
    // A pipeline that produced nothing looks exactly like a deliberate clear,
    // and the difference is somebody's stored secret.
    if !allow_empty && value.trim().is_empty() {
        return Err(EnvKeySetError::EmptyValue);
    }

    let path = canonical_regular_file(path, "target")?;
    let content = read_env_file_at(&path)?;
    let (updated, disposition) = set_env_key_content(&content, key, value)?;

    if disposition != EnvKeySetDisposition::AlreadyPresent {
        atomic_write_metadata_if_unchanged(&path, &updated, Some(&content)).map_err(|source| {
            EnvFileAccessError::Io {
                action: "write",
                path: path.clone(),
                source,
            }
        })?;
    }

    Ok(EnvKeySetOutcome {
        disposition,
        path: path.to_string_lossy().to_string(),
        key: key.to_string(),
    })
}

fn set_env_key_content(
    content: &str,
    key: &str,
    value: &str,
) -> Result<(String, EnvKeySetDisposition), EnvKeySetError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeySetError::InvalidKey);
    }

    let occurrences = parse_env_entries(content)
        .into_iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    let entry = match occurrences.as_slice() {
        [] => return Err(EnvKeySetError::Missing),
        [entry] => entry,
        entries => {
            return Err(EnvKeySetError::Duplicate {
                line_numbers: entries.iter().map(|entry| entry.line_number).collect(),
            })
        }
    };
    if entry.diagnostic.is_some() {
        return Err(EnvKeySetError::Malformed {
            line_number: entry.line_number,
        });
    }
    if entry.value == value {
        return Ok((content.to_string(), EnvKeySetDisposition::AlreadyPresent));
    }

    let updated = replace_env_value_at(content, key, entry.line_number, value)
        .expect("the single parsed occurrence is always replaceable");
    Ok((updated, EnvKeySetDisposition::Updated))
}

pub fn remove_env_key(
    path: &Path,
    key: &str,
    selector: EnvKeyRemoveSelector,
    expectation: Option<EnvKeyExpectation>,
) -> Result<EnvKeyRemoveOutcome, EnvKeyRemoveError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeyRemoveError::InvalidKey);
    }

    let path = canonical_regular_file(path, "target")?;
    let content = read_env_file_at(&path)?;
    let removal = remove_env_key_content(&content, key, selector, expectation)?;

    if let Some(updated) = &removal.content {
        atomic_write_metadata_if_unchanged(&path, updated, Some(&content)).map_err(|source| {
            EnvFileAccessError::Io {
                action: "write",
                path: path.clone(),
                source,
            }
        })?;
    }

    Ok(EnvKeyRemoveOutcome {
        disposition: removal.disposition,
        path: path.to_string_lossy().to_string(),
        key: key.to_string(),
        removed_lines: removal.removed_lines,
    })
}

struct EnvKeyRemovePlan {
    /// `Some` only when bytes actually change; a convergent no-op never writes.
    content: Option<String>,
    disposition: EnvKeyRemoveDisposition,
    removed_lines: Vec<usize>,
}

fn remove_env_key_content(
    content: &str,
    key: &str,
    selector: EnvKeyRemoveSelector,
    expectation: Option<EnvKeyExpectation>,
) -> Result<EnvKeyRemovePlan, EnvKeyRemoveError> {
    if !is_valid_env_key(key) {
        return Err(EnvKeyRemoveError::InvalidKey);
    }

    let entries = parse_env_entries(content);
    let occurrences = entries
        .iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    let occurrence_lines = occurrences
        .iter()
        .map(|entry| entry.line_number)
        .collect::<Vec<_>>();

    match expectation {
        Some(EnvKeyExpectation::Present) if occurrences.is_empty() => {
            return Err(EnvKeyRemoveError::ExpectedPresent)
        }
        Some(EnvKeyExpectation::Absent) if !occurrences.is_empty() => {
            return Err(EnvKeyRemoveError::ExpectedAbsent {
                line_numbers: occurrence_lines,
            })
        }
        _ => {}
    }

    let doomed = match selector {
        EnvKeyRemoveSelector::Line(line_number) => {
            let selected = entries
                .iter()
                .find(|entry| entry.line_number == line_number)
                .filter(|entry| entry.key == key)
                .ok_or_else(|| EnvKeyRemoveError::StaleSelector {
                    line_number,
                    found_key: entries
                        .iter()
                        .find(|entry| entry.line_number == line_number)
                        .map(|entry| entry.key.clone()),
                })?;
            vec![selected]
        }
        EnvKeyRemoveSelector::All => occurrences,
        EnvKeyRemoveSelector::Only => match occurrences.as_slice() {
            [] => Vec::new(),
            [entry] => vec![*entry],
            _ => {
                return Err(EnvKeyRemoveError::Duplicate {
                    line_numbers: occurrence_lines,
                })
            }
        },
    };

    if let Some(malformed) = doomed.iter().find(|entry| entry.diagnostic.is_some()) {
        return Err(EnvKeyRemoveError::Malformed {
            line_number: malformed.line_number,
        });
    }

    if doomed.is_empty() {
        return Ok(EnvKeyRemovePlan {
            content: None,
            disposition: EnvKeyRemoveDisposition::AlreadyAbsent,
            removed_lines: Vec::new(),
        });
    }

    let disposition = if doomed.len() > 1 {
        EnvKeyRemoveDisposition::RemovedAll
    } else {
        EnvKeyRemoveDisposition::Removed
    };
    let removed_lines = doomed.iter().map(|entry| entry.line_number).collect();

    let mut updated = String::with_capacity(content.len());
    let mut cursor = 0;
    for entry in doomed {
        updated.push_str(&content[cursor..entry.line_start]);
        cursor = entry.next_start;
    }
    updated.push_str(&content[cursor..]);

    Ok(EnvKeyRemovePlan {
        content: Some(updated),
        disposition,
        removed_lines,
    })
}

pub fn rename_env_key(
    path: &Path,
    from: &str,
    to: &str,
) -> Result<EnvKeyRenameOutcome, EnvKeyRenameError> {
    let path = canonical_regular_file(path, "target")?;
    let content = read_env_file_at(&path)?;
    let updated = rename_env_key_content(&content, from, to)?;

    atomic_write_metadata_if_unchanged(&path, &updated, Some(&content)).map_err(|source| {
        EnvFileAccessError::Io {
            action: "write",
            path: path.clone(),
            source,
        }
    })?;

    Ok(EnvKeyRenameOutcome {
        disposition: EnvKeyRenameDisposition::Renamed,
        path: path.to_string_lossy().to_string(),
        keys: EnvKeyRenamePair {
            from: from.to_string(),
            to: to.to_string(),
        },
    })
}

/// Renames the key token in place. The value token, inline comment, `export`
/// prefix, quoting, spacing, and line position are all left byte-identical, so
/// the only change to the file is the key name itself.
fn rename_env_key_content(
    content: &str,
    from: &str,
    to: &str,
) -> Result<String, EnvKeyRenameError> {
    if !is_valid_env_key(from) {
        return Err(EnvKeyRenameError::InvalidSourceKey);
    }
    if !is_valid_env_key(to) {
        return Err(EnvKeyRenameError::InvalidTargetKey);
    }
    if from == to {
        return Err(EnvKeyRenameError::SameKey);
    }

    let entries = parse_env_entries(content);
    let source_lines = entries
        .iter()
        .filter(|entry| entry.key == from)
        .map(|entry| entry.line_number)
        .collect::<Vec<_>>();
    let target_lines = entries
        .iter()
        .filter(|entry| entry.key == to)
        .map(|entry| entry.line_number)
        .collect::<Vec<_>>();

    if !target_lines.is_empty() {
        return Err(if source_lines.is_empty() {
            EnvKeyRenameError::Indeterminate {
                line_numbers: target_lines,
            }
        } else {
            EnvKeyRenameError::TargetExists {
                line_numbers: target_lines,
            }
        });
    }

    let source = match entries
        .iter()
        .filter(|entry| entry.key == from)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Err(EnvKeyRenameError::SourceMissing),
        [source] => *source,
        _ => {
            return Err(EnvKeyRenameError::SourceDuplicate {
                line_numbers: source_lines,
            })
        }
    };
    if source.diagnostic.is_some() {
        return Err(EnvKeyRenameError::SourceMalformed {
            line_number: source.line_number,
        });
    }

    let mut updated = String::with_capacity(content.len() + to.len());
    updated.push_str(&content[..source.key_start]);
    updated.push_str(to);
    updated.push_str(&content[source.key_end..]);
    Ok(updated)
}

/// Adds keys the real env file has and the example file lacks, as valueless
/// placeholders. It never copies a value, never deletes an example entry, and
/// never rewrites a line that is already there.
pub fn sync_example_file(
    source_path: &Path,
    example_path: Option<&Path>,
) -> Result<ExampleSyncOutcome, ExampleSyncError> {
    // Read the source before touching the example file. Creating first would
    // leave an empty example behind when the source turns out to be unreadable.
    let source_path = canonical_regular_file(source_path, "source")?;
    let source_content = read_env_file_at(&source_path)?;

    let example_path = match example_path {
        Some(path) => path.to_path_buf(),
        None => default_example_path(&source_path)?,
    };
    // `created` comes from what the creation primitive actually did, not from an
    // existence check taken before it ran.
    let created = if example_path.exists() {
        false
    } else {
        create_example_file(&example_path)? == EnvFileCreationDisposition::Created
    };

    let example_path = canonical_regular_file(&example_path, "example")?;
    if source_path == example_path {
        return Err(ExampleSyncError::SameFile);
    }

    let example_content = read_env_file_at(&example_path)?;
    let (updated, added_keys) = append_missing_example_keys(&source_content, &example_content);

    if !added_keys.is_empty() {
        atomic_write_metadata_if_unchanged(&example_path, &updated, Some(&example_content)).map_err(|source| {
            ExampleSyncError::Io {
                action: "write",
                path: example_path.clone(),
                source,
            }
        })?;
    }

    let disposition = match (created, added_keys.is_empty()) {
        (true, _) => ExampleSyncDisposition::Created,
        (false, false) => ExampleSyncDisposition::Updated,
        (false, true) => ExampleSyncDisposition::AlreadyCurrent,
    };

    Ok(ExampleSyncOutcome {
        disposition,
        source_path: source_path.to_string_lossy().to_string(),
        example_path: example_path.to_string_lossy().to_string(),
        added_keys,
    })
}

fn default_example_path(source_path: &Path) -> Result<PathBuf, ExampleSyncError> {
    let parent = source_path
        .parent()
        .ok_or(ExampleSyncError::UnusableExamplePath)?;
    Ok(parent.join(".env.example"))
}

fn create_example_file(path: &Path) -> Result<EnvFileCreationDisposition, ExampleSyncError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ExampleSyncError::UnusableExamplePath)?;

    ensure_project_env_file(parent, name)
        .map(|outcome| outcome.disposition)
        .map_err(|source| ExampleSyncError::Io {
            action: "create",
            path: path.to_path_buf(),
            source,
        })
}

/// Returns the example content with the missing keys appended, plus the key
/// names in source order. Additions are key-only placeholders: comment
/// payloads from real env files never travel into example files
/// (VNE-SEC-006 — structural boundary, no classifier involvement).
fn append_missing_example_keys(
    source_content: &str,
    example_content: &str,
) -> (String, Vec<String>) {
    let example_keys = parse_env_entries(example_content)
        .into_iter()
        .map(|entry| entry.key)
        .collect::<BTreeSet<_>>();

    let mut seen = BTreeSet::new();
    let mut additions = Vec::new();
    for entry in parse_env_entries(source_content) {
        if example_keys.contains(&entry.key) || !seen.insert(entry.key.clone()) {
            continue;
        }
        additions.push(entry.key);
    }

    let line_ending = preferred_line_ending(example_content);
    let mut updated = String::with_capacity(example_content.len() + additions.len() * 32);
    updated.push_str(example_content);
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push_str(line_ending);
    }
    for key in &additions {
        updated.push_str(key);
        updated.push('=');
        updated.push_str(line_ending);
    }

    let added_keys = additions;
    (updated, added_keys)
}

fn env_value_token<'a>(content: &'a str, entry: &ParsedLine) -> EnvValueToken<'a> {
    let (start, end) = env_value_token_bounds(entry);
    EnvValueToken {
        text: &content[start..end],
    }
}

fn env_value_token_bounds(entry: &ParsedLine) -> (usize, usize) {
    if entry.quote.is_some() {
        (entry.value_start - 1, entry.value_end + 1)
    } else {
        (entry.value_start, entry.value_end)
    }
}

fn append_env_key_token(content: &str, key: &str, token: EnvValueToken<'_>) -> String {
    let line_ending = preferred_line_ending(content);
    let mut output = String::with_capacity(content.len() + key.len() + token.text.len() + 4);
    output.push_str(content);
    if !output.is_empty() && !output.ends_with('\n') {
        output.push_str(line_ending);
    }
    output.push_str(key);
    output.push('=');
    output.push_str(token.text);
    output.push_str(line_ending);
    output
}

fn replace_env_value_token(
    content: &str,
    destination: &ParsedLine,
    source_token: EnvValueToken<'_>,
) -> String {
    let destination_token = env_value_token(content, destination);
    let (token_start, token_end) = env_value_token_bounds(destination);
    let mut output = String::with_capacity(
        content.len() - destination_token.text.len() + source_token.text.len(),
    );
    output.push_str(&content[..token_start]);
    output.push_str(source_token.text);
    output.push_str(&content[token_end..]);
    output
}

fn preferred_line_ending(content: &str) -> &'static str {
    match content.find('\n') {
        Some(index) if index > 0 && content.as_bytes()[index - 1] == b'\r' => "\r\n",
        _ => "\n",
    }
}

pub fn infer_key_shape(key: &str, value: &str) -> KeyShape {
    let upper = key.to_ascii_uppercase();
    let trimmed = value.trim();
    let mut reasons = Vec::new();

    let public_prefix = upper.starts_with("NEXT_PUBLIC_")
        || upper.starts_with("VITE_")
        || upper.starts_with("PUBLIC_");
    let provider_key = key_without_public_prefix(&upper);
    let provider = provider_reason(provider_key);
    if let Some(provider) = provider {
        reasons.push(provider);
    }
    if public_prefix {
        reasons.push("frontend-exposed prefix".to_string());
    }

    let known_profile = known_key_profile(&upper);
    if let Some(profile) = known_profile {
        let reason = match profile {
            KnownKeyProfile::Secret(reason) | KnownKeyProfile::Public(reason) => reason,
        };
        reasons.push(reason.to_string());
    }

    let known_public_profile = matches!(known_profile, Some(KnownKeyProfile::Public(_)));
    let browser_exposed = public_prefix || known_public_profile;
    let known_public_value = known_public_profile && known_public_credential_value(&upper, trimmed);

    let secretish = matches!(known_profile, Some(KnownKeyProfile::Secret(_)))
        || contains_secretish_key_name(&upper);

    if secretish && browser_exposed {
        reasons.push("secret-like key name".to_string());
        return shape_with_context(
            "public-secret",
            "Browser-exposed secret",
            "high",
            true,
            true,
            Some("browser"),
            reasons,
        );
    }

    if secretish {
        reasons.push("secret-like key name".to_string());
        return shape("secret", "Secret", "high", true, reasons);
    }

    if trimmed.starts_with("-----BEGIN ") {
        reasons.push("PEM block marker".to_string());
        return shape_with_context(
            "pem",
            "PEM / certificate",
            "high",
            true,
            true,
            browser_exposed.then_some("browser"),
            reasons,
        );
    }

    let url_like = upper.ends_with("_URL") || upper.ends_with("_URI") || trimmed.contains("://");
    if upper.contains("DSN") {
        reasons.push("DSN-like key name".to_string());
        if url_like {
            reasons.push("URL-like name or value".to_string());
        }
        return shape_with_context(
            "dsn",
            "Credential URL / DSN",
            "high",
            true,
            true,
            browser_exposed.then_some("browser"),
            reasons,
        );
    }

    if url_like && is_credential_url_key(&upper) {
        reasons.push("credential-bearing URL key name".to_string());
        reasons.push("URL-like name or value".to_string());
        return shape_with_context(
            "credential-url",
            "Credential URL / DSN",
            "high",
            true,
            true,
            browser_exposed.then_some("browser"),
            reasons,
        );
    }

    if url_like && url_value_has_secretish_parts(trimmed) {
        reasons.push("credential-like URL value".to_string());
        reasons.push("URL-like name or value".to_string());
        return shape_with_context(
            "credential-url",
            "Credential URL / DSN",
            "high",
            true,
            true,
            browser_exposed.then_some("browser"),
            reasons,
        );
    }

    if value_has_credential_signature(trimmed) && !known_public_value {
        reasons.push("credential-like value signature".to_string());
        if browser_exposed {
            return shape_with_context(
                "public-secret",
                "Browser-exposed secret",
                "high",
                true,
                true,
                Some("browser"),
                reasons,
            );
        }
        return shape("secret", "Secret", "high", true, reasons);
    }

    if matches!(known_profile, Some(KnownKeyProfile::Public(_))) {
        return shape_with_context(
            "public",
            "Public provider key",
            "high",
            false,
            false,
            Some("browser"),
            reasons,
        );
    }

    if public_prefix {
        return shape_with_context(
            "public",
            "Public frontend variable",
            "high",
            false,
            false,
            Some("browser"),
            reasons,
        );
    }

    if url_like {
        reasons.push("URL-like name or value".to_string());
        return shape("url", "URL / DSN", "high", false, reasons);
    }

    if upper.ends_with("_PORT") || key == "PORT" || trimmed.parse::<u16>().is_ok() {
        reasons.push("port-like key or numeric port value".to_string());
        return shape("port", "Port", "medium", false, reasons);
    }

    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "true" | "false" | "1" | "0" | "yes" | "no"
    ) {
        reasons.push("boolean-like value".to_string());
        return shape("bool", "Boolean", "medium", false, reasons);
    }

    if trimmed.parse::<i64>().is_ok() {
        reasons.push("integer value".to_string());
        return shape("int", "Integer", "medium", false, reasons);
    }

    if upper.ends_with("_TIMEOUT")
        || upper.ends_with("_TTL")
        || upper.ends_with("_INTERVAL")
        || looks_like_duration(trimmed)
    {
        reasons.push("duration-like key or value".to_string());
        return shape("duration", "Duration", "medium", false, reasons);
    }

    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        reasons.push("JSON-looking value".to_string());
        return shape("json", "JSON", "medium", false, reasons);
    }

    if looks_like_uuid(trimmed) {
        reasons.push("UUID-looking value".to_string());
        return shape("uuid", "UUID", "medium", false, reasons);
    }

    if trimmed.contains('@') && trimmed.contains('.') {
        reasons.push("email-looking value".to_string());
        return shape("email", "Email", "medium", false, reasons);
    }

    if upper.ends_with("_PATH")
        || upper.ends_with("_DIR")
        || upper.ends_with("_FILE")
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("~/")
    {
        reasons.push("path-like key or value".to_string());
        return shape("path", "File path", "medium", false, reasons);
    }

    if trimmed.contains(',') {
        reasons.push("comma-separated value".to_string());
        return shape("list", "List", "medium", false, reasons);
    }

    if looks_like_base64(trimmed) {
        reasons.push("base64-looking value".to_string());
        return shape("base64", "Base64", "low", true, reasons);
    }

    if upper.ends_with("_HOST") || upper.ends_with("_HOSTNAME") || key == "HOST" {
        reasons.push("host-like key name".to_string());
        return shape("host", "Host", "medium", false, reasons);
    }

    // Default-deny (VNE-SEC-002 CLI half): a value matching NO known-benign
    // shape is withheld, not exposed. Known-benign shapes (url, port, bool,
    // int, duration, json, uuid, email, path, list, host, public) still show
    // under --values; anything unrecognized is treated as possibly secret.
    // Note: withheld (redacted) WITHOUT being labeled sensitive — the
    // plain `shape()` helper couples the two flags, and lookalike keys must
    // not gain a secret label.
    reasons.push("unrecognized shape; withheld by default".to_string());
    shape_with_context("text", "Text", "low", true, false, None, reasons)
}

fn parse_env_entries(content: &str) -> Vec<ParsedLine> {
    let mut entries = Vec::new();
    let mut offset = 0;
    let mut line_number = 1;

    while offset < content.len() {
        if let Some(parsed) = parse_env_entry_at(content, offset, line_number) {
            line_number += count_newlines(&content[offset..parsed.next_start]);
            offset = parsed.next_start;
            entries.push(parsed);
        } else {
            let next = next_line_start(content, offset);
            line_number += count_newlines(&content[offset..next]);
            offset = next;
        }
    }

    entries
}

fn parse_env_entry_at(content: &str, line_start: usize, line_number: usize) -> Option<ParsedLine> {
    let line_end = current_line_body_end(content, line_start);
    let line_body = &content[line_start..line_end];
    let bytes = line_body.as_bytes();
    let mut index = 0;
    let mut entry_start = line_start;

    if line_start == 0 && bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        index = 3;
        entry_start = line_start + 3;
    }

    skip_spaces(bytes, &mut index);
    if index >= bytes.len() || bytes[index] == b'#' {
        return None;
    }

    let mut exported = false;
    if bytes[index..].starts_with(b"export")
        && bytes.get(index + 6).is_some_and(u8::is_ascii_whitespace)
    {
        exported = true;
        index += 6;
        skip_spaces(bytes, &mut index);
    }

    let key_start = index;
    if index >= bytes.len() || !is_key_start(bytes[index]) {
        return None;
    }
    index += 1;
    while index < bytes.len() && is_key_continue(bytes[index]) {
        index += 1;
    }
    let key_end = index;
    let key = line_body[key_start..key_end].to_string();

    skip_spaces(bytes, &mut index);
    if bytes.get(index) != Some(&b'=') {
        return None;
    }
    index += 1;
    skip_spaces(bytes, &mut index);

    let mut diagnostic = None;
    let mut quote = None;
    let mut comment = None;
    let value_start;
    let value_end;
    let next_start;
    let value;

    match bytes.get(index) {
        Some(b'"') | Some(b'\'') => {
            let quote_byte = bytes[index];
            quote = Some(quote_byte as char);
            index += 1;
            value_start = line_start + index;
            let mut cursor = value_start;
            let mut escaped = false;
            let mut found = false;
            while cursor < content.len() {
                let byte = content.as_bytes()[cursor];
                if byte == b'\\' && !escaped {
                    escaped = true;
                    cursor += 1;
                    continue;
                }
                if byte == quote_byte && !escaped {
                    found = true;
                    break;
                }
                escaped = false;
                cursor += 1;
            }
            value_end = cursor;
            next_start = if found {
                let comment_end = current_line_body_end(content, cursor);
                if cursor < comment_end {
                    comment = extract_inline_comment(&content[cursor + 1..comment_end]);
                }
                next_line_start(content, cursor)
            } else {
                content.len()
            };
            value = content[value_start..value_end].to_string();
            if !found {
                diagnostic = Some("quoted value is not closed".to_string());
            }
        }
        _ => {
            let local_value_start = index;
            value_start = line_start + local_value_start;
            let mut cursor = index;
            let mut comment_start = bytes.len();
            while cursor < bytes.len() {
                if bytes[cursor] == b'#'
                    && (cursor == index || bytes[cursor.saturating_sub(1)].is_ascii_whitespace())
                {
                    comment_start = cursor;
                    break;
                }
                cursor += 1;
            }
            let mut end = comment_start;
            while end > local_value_start && bytes[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            if comment_start < bytes.len() {
                comment = Some(line_body[comment_start..].trim().to_string());
            }
            value_end = line_start + end;
            next_start = next_line_start(content, line_start);
            value = content[value_start..value_end].to_string();
        }
    }

    Some(ParsedLine {
        key,
        value,
        line_number,
        exported,
        quote,
        comment,
        line_start: entry_start,
        key_start: line_start + key_start,
        key_end: line_start + key_end,
        value_start,
        value_end,
        next_start,
        diagnostic,
    })
}

fn compare_actual_to_example(files: &[EnvFile]) -> Option<EnvComparison> {
    let base = files.iter().find(|file| file.name == ".env")?;
    let example = files.iter().find(|file| {
        matches!(
            file.name.as_str(),
            ".env.example" | ".env.sample" | ".env.template" | ".env.defaults"
        )
    })?;

    Some(compare_env_files(base, example))
}

pub fn compare_env_files(base: &EnvFile, example: &EnvFile) -> EnvComparison {
    let base_keys = key_set(base);
    let example_keys = key_set(example);

    let missing_keys = example_keys.difference(&base_keys).cloned().collect();
    let extra_keys = base_keys.difference(&example_keys).cloned().collect();
    let shared_keys = base_keys.intersection(&example_keys).cloned().collect();
    let duplicate_keys = [base, example]
        .into_iter()
        .flat_map(|file| {
            file.duplicate_keys
                .iter()
                .map(|key| format!("{}:{key}", file.name))
        })
        .collect();

    EnvComparison {
        base_path: base.path.clone(),
        example_path: example.path.clone(),
        missing_keys,
        extra_keys,
        shared_keys,
        duplicate_keys,
    }
}

fn build_layer_report(files: &[EnvFile]) -> EnvLayerReport {
    let mut ordered_files = files
        .iter()
        .filter(|file| is_layer_file(file))
        .map(|file| EnvLayerFile {
            path: file.path.clone(),
            name: file.name.clone(),
            layer_kind: layer_kind(&file.name).to_string(),
            precedence: layer_precedence(&file.name),
        })
        .collect::<Vec<_>>();

    ordered_files.sort_by_key(|file| (file.precedence, file.name.clone()));

    let mut by_key = BTreeMap::<String, Vec<(&EnvFile, &EnvEntry)>>::new();
    let mut placeholder_keys = BTreeSet::new();

    for file in files.iter().filter(|file| is_layer_file(file)) {
        for entry in &file.entries {
            by_key
                .entry(entry.key.clone())
                .or_default()
                .push((file, entry));
            if is_placeholder_value(&entry.value) {
                placeholder_keys.insert(format!("{}:{}", file.name, entry.key));
            }
        }
    }

    let mut overrides = Vec::new();
    for (key, mut entries) in by_key {
        if entries.len() < 2 {
            continue;
        }

        entries.sort_by_key(|(file, _)| (layer_precedence(&file.name), file.name.clone()));
        let unique_values = entries
            .iter()
            .map(|(_, entry)| normalized_layer_value(&entry.value))
            .collect::<BTreeSet<_>>();
        let conflict = unique_values.len() > 1;
        let effective_file = entries
            .last()
            .map(|(file, _)| file.name.clone())
            .unwrap_or_default();
        let redacted = entries
            .iter()
            .any(|(_, entry)| entry.shape.redacted_by_default);
        let files = entries
            .iter()
            .map(|(file, _)| file.name.clone())
            .collect::<Vec<_>>();
        let summary = if conflict {
            format!(
                "{} layers set different values; runtime precedence needs framework evidence.",
                files.len()
            )
        } else {
            format!("{} layers repeat the same value across files.", files.len())
        };

        overrides.push(EnvLayerOverride {
            key,
            files,
            effective_file,
            conflict,
            redacted,
            summary,
        });
    }

    EnvLayerReport {
        ordered_files,
        overrides,
        placeholder_keys: placeholder_keys.into_iter().collect(),
    }
}

fn build_framework_profiles(root: &Path, files: &[EnvFile]) -> Vec<FrameworkEnvProfile> {
    let Some(package_json) = read_package_json(root) else {
        return Vec::new();
    };
    let mut profiles = Vec::new();

    if let Some(evidence) = dependency_evidence(&package_json, "next") {
        for mode in ["development", "production", "test"] {
            let desired = next_env_order(mode);
            let (ordered_files, missing_files) = framework_file_order(root, files, &desired);
            profiles.push(FrameworkEnvProfile {
                framework: "Next.js".to_string(),
                mode: mode.to_string(),
                evidence: vec![evidence.clone()],
                ordered_files,
                missing_files,
                notes: vec![
                    "Effective order is highest priority first; process.env is checked before files."
                        .to_string(),
                    "Next.js stops lookup once a key is found.".to_string(),
                    "`NEXT_PUBLIC_` keys are browser-exposed by convention.".to_string(),
                ],
            });
        }
    }

    let vite_evidence = dependency_evidence(&package_json, "vite")
        .or_else(|| dependency_evidence(&package_json, "@sveltejs/kit"));
    if let Some(evidence) = vite_evidence {
        for mode in vite_modes(&package_json) {
            let desired = vite_env_order(&mode);
            let (ordered_files, missing_files) = framework_file_order(root, files, &desired);
            profiles.push(FrameworkEnvProfile {
                framework: "Vite".to_string(),
                mode,
                evidence: vec![evidence.clone()],
                ordered_files,
                missing_files,
                notes: vec![
                    "Effective order is highest priority first; existing process env wins over env files."
                        .to_string(),
                    "Mode-specific env files override generic env files.".to_string(),
                    "`VITE_` keys are browser-exposed by convention.".to_string(),
                ],
            });
        }
    }

    profiles
}

fn read_package_json(root: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&content).ok()
}

fn dependency_evidence(package_json: &serde_json::Value, dependency: &str) -> Option<String> {
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        let Some(version) = package_json
            .get(section)
            .and_then(serde_json::Value::as_object)
            .and_then(|dependencies| dependencies.get(dependency))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };

        return Some(format!("package.json {section}.{dependency} = {version}"));
    }

    None
}

fn next_env_order(mode: &str) -> Vec<String> {
    let mut names = vec![format!(".env.{mode}.local")];
    if mode != "test" {
        names.push(".env.local".to_string());
    }
    names.push(format!(".env.{mode}"));
    names.push(".env".to_string());
    names
}

fn vite_env_order(mode: &str) -> Vec<String> {
    vec![
        format!(".env.{mode}.local"),
        format!(".env.{mode}"),
        ".env.local".to_string(),
        ".env".to_string(),
    ]
}

fn vite_modes(package_json: &serde_json::Value) -> Vec<String> {
    let mut modes = BTreeSet::from(["development".to_string(), "production".to_string()]);

    if let Some(scripts) = package_json
        .get("scripts")
        .and_then(serde_json::Value::as_object)
    {
        for script in scripts.values().filter_map(serde_json::Value::as_str) {
            let tokens = shell_tokens(script);
            for (index, token) in tokens.iter().enumerate() {
                if let Some(mode) = token.strip_prefix("--mode=") {
                    if is_mode_name(mode) {
                        modes.insert(mode.to_string());
                    }
                    continue;
                }

                if token == "--mode" {
                    if let Some(mode) = tokens.get(index + 1).filter(|mode| is_mode_name(mode)) {
                        modes.insert(mode.to_string());
                    }
                }
            }
        }
    }

    modes.into_iter().collect()
}

fn framework_file_order(
    root: &Path,
    files: &[EnvFile],
    desired: &[String],
) -> (Vec<FrameworkEnvFile>, Vec<String>) {
    let mut ordered_files = Vec::new();
    let mut missing_files = Vec::new();

    for (index, name) in desired.iter().enumerate() {
        if let Some(file) = files
            .iter()
            .find(|file| file_has_root_relative_name(root, file, name))
        {
            ordered_files.push(FrameworkEnvFile {
                path: file.path.clone(),
                name: file.name.clone(),
                layer_kind: layer_kind(&file.name).to_string(),
                rank: index + 1,
            });
        } else {
            missing_files.push(name.clone());
        }
    }

    (ordered_files, missing_files)
}

fn file_has_root_relative_name(root: &Path, file: &EnvFile, name: &str) -> bool {
    Path::new(&file.path)
        .strip_prefix(root)
        .ok()
        .is_some_and(|relative| relative == Path::new(name))
}

fn is_mode_name(mode: &str) -> bool {
    !mode.is_empty()
        && mode
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn build_findings(
    comparison: Option<&EnvComparison>,
    layer_report: &EnvLayerReport,
    framework_profiles: &[FrameworkEnvProfile],
    files: &[EnvFile],
) -> Vec<EnvFinding> {
    let mut findings = Vec::new();

    if let Some(comparison) = comparison {
        for key in &comparison.missing_keys {
            findings.push(EnvFinding {
                severity: "warning".to_string(),
                action_kind: "add-missing-key".to_string(),
                title: format!("Add `{key}` to {}", file_label(&comparison.base_path)),
                detail: format!(
                    "`{key}` is documented in {} but missing from {}.",
                    file_label(&comparison.example_path),
                    file_label(&comparison.base_path)
                ),
                evidence: Vec::new(),
                mutation_preview: Some(format!(
                    "Append `{key}=<value>` to {}.",
                    file_label(&comparison.base_path)
                )),
                file_path: Some(comparison.base_path.clone()),
                key: Some(key.clone()),
                line_number: None,
                entry_id: None,
            });
        }

        for key in &comparison.extra_keys {
            let target = find_entry_target_by_path(files, &comparison.base_path, key);
            findings.push(EnvFinding {
                severity: "info".to_string(),
                action_kind: "document-or-remove-key".to_string(),
                title: format!("Document or remove `{key}`"),
                detail: format!(
                    "`{key}` exists in {} but not in {}.",
                    file_label(&comparison.base_path),
                    file_label(&comparison.example_path)
                ),
                evidence: Vec::new(),
                mutation_preview: None,
                file_path: Some(comparison.base_path.clone()),
                key: Some(key.clone()),
                line_number: target.as_ref().map(|target| target.line_number),
                entry_id: target.as_ref().map(|target| target.entry_id.clone()),
            });
        }

        for duplicate in &comparison.duplicate_keys {
            let (file_name, key) = duplicate
                .split_once(':')
                .map(|(file_name, key)| (file_name.to_string(), key.to_string()))
                .unwrap_or_else(|| ("env file".to_string(), duplicate.clone()));
            let target = find_duplicate_entry_target_by_name(files, &file_name, &key);
            findings.push(EnvFinding {
                severity: "warning".to_string(),
                action_kind: "resolve-duplicate-key".to_string(),
                title: format!("Resolve duplicate `{key}`"),
                detail: format!("`{file_name}` defines `{key}` more than once."),
                evidence: Vec::new(),
                mutation_preview: None,
                file_path: target
                    .as_ref()
                    .map(|target| target.file_path.clone())
                    .or_else(|| find_file_path_by_name(files, &file_name)),
                key: Some(key),
                line_number: target.as_ref().map(|target| target.line_number),
                entry_id: target.as_ref().map(|target| target.entry_id.clone()),
            });
        }
    }

    for placeholder in &layer_report.placeholder_keys {
        let (file_name, key) = placeholder
            .split_once(':')
            .map(|(file_name, key)| (file_name.to_string(), key.to_string()))
            .unwrap_or_else(|| ("env file".to_string(), placeholder.clone()));
        let target = find_placeholder_entry_target_by_name(files, &file_name, &key);
        findings.push(EnvFinding {
            severity: "warning".to_string(),
            action_kind: "replace-placeholder".to_string(),
            title: format!("Replace placeholder `{key}`"),
            detail: format!("`{file_name}` has a placeholder-like value for `{key}`."),
            evidence: Vec::new(),
            mutation_preview: None,
            file_path: target
                .as_ref()
                .map(|target| target.file_path.clone())
                .or_else(|| find_file_path_by_name(files, &file_name)),
            key: Some(key),
            line_number: target.as_ref().map(|target| target.line_number),
            entry_id: target.as_ref().map(|target| target.entry_id.clone()),
        });
    }

    for override_row in layer_report
        .overrides
        .iter()
        .filter(|override_row| override_row.conflict)
    {
        let evidence = framework_evidence_for_files(framework_profiles, &override_row.files);
        let detail = if evidence.is_empty() {
            format!(
                "{} set `{}` in multiple env layers; runtime precedence is unknown without framework or tool evidence.",
                override_row.files.join(", "),
                override_row.key
            )
        } else {
            format!(
                "{} set `{}` in multiple env layers; attached framework evidence may identify the likely effective value.",
                override_row.files.join(", "),
                override_row.key
            )
        };
        findings.push(EnvFinding {
            severity: "info".to_string(),
            action_kind: "review-layer-conflict".to_string(),
            title: format!("Review layered `{}`", override_row.key),
            detail,
            evidence,
            mutation_preview: None,
            file_path: None,
            key: Some(override_row.key.clone()),
            line_number: None,
            entry_id: None,
        });
    }

    findings
}

fn framework_evidence_for_files(
    framework_profiles: &[FrameworkEnvProfile],
    file_names: &[String],
) -> Vec<String> {
    let file_names = file_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    framework_profiles
        .iter()
        .filter_map(|profile| {
            let ordered = profile
                .ordered_files
                .iter()
                .filter(|file| file_names.contains(file.name.as_str()))
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>();
            (ordered.len() >= 2).then(|| {
                format!(
                    "{} {} load order: {}",
                    profile.framework,
                    profile.mode,
                    ordered.join(" -> ")
                )
            })
        })
        .take(3)
        .collect()
}

fn find_file_path_by_name(files: &[EnvFile], file_name: &str) -> Option<String> {
    files
        .iter()
        .find(|file| file.name == file_name)
        .map(|file| file.path.clone())
}

fn find_entry_target_by_path(
    files: &[EnvFile],
    file_path: &str,
    key: &str,
) -> Option<EnvEntryTarget> {
    files
        .iter()
        .find(|file| file.path == file_path)
        .and_then(|file| {
            file.entries
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry_target(file, entry))
        })
}

fn find_duplicate_entry_target_by_name(
    files: &[EnvFile],
    file_name: &str,
    key: &str,
) -> Option<EnvEntryTarget> {
    let file = files.iter().find(|file| file.name == file_name)?;
    let matches = file
        .entries
        .iter()
        .filter(|entry| entry.key == key)
        .collect::<Vec<_>>();
    matches
        .get(1)
        .or_else(|| matches.first())
        .map(|entry| entry_target(file, entry))
}

fn find_placeholder_entry_target_by_name(
    files: &[EnvFile],
    file_name: &str,
    key: &str,
) -> Option<EnvEntryTarget> {
    files
        .iter()
        .find(|file| file.name == file_name)
        .and_then(|file| {
            file.entries
                .iter()
                .find(|entry| entry.key == key && is_placeholder_value(&entry.value))
                .map(|entry| entry_target(file, entry))
        })
}

fn entry_target(file: &EnvFile, entry: &EnvEntry) -> EnvEntryTarget {
    EnvEntryTarget {
        file_path: file.path.clone(),
        line_number: entry.line_number,
        entry_id: entry.id.clone(),
    }
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn is_layer_file(file: &EnvFile) -> bool {
    !matches!(
        file.name.as_str(),
        ".env.example" | ".env.sample" | ".env.template" | ".env.defaults"
    )
}

fn layer_kind(name: &str) -> &'static str {
    match name {
        ".env" => "base",
        ".env.local" => "local",
        ".env.development" => "development",
        ".env.production" => "production",
        ".env.test" => "test",
        ".env.development.local" => "development local",
        ".env.production.local" => "production local",
        ".env.test.local" => "test local",
        ".envrc" => "shell",
        ".flaskenv" => "Flask",
        _ if name.ends_with(".env") => "referenced",
        _ => "custom",
    }
}

fn layer_precedence(name: &str) -> usize {
    match name {
        ".env" => 10,
        ".env.development" | ".env.production" | ".env.test" => 20,
        ".env.local" => 30,
        ".env.development.local" | ".env.production.local" | ".env.test.local" => 40,
        ".envrc" | ".flaskenv" => 50,
        _ => 60,
    }
}

fn normalized_layer_value(value: &str) -> String {
    value.trim().trim_matches(['"', '\'']).to_string()
}

fn is_placeholder_value(value: &str) -> bool {
    let normalized = normalized_layer_value(value).to_ascii_lowercase();
    normalized.is_empty()
        || matches!(
            normalized.as_str(),
            "changeme" | "change-me" | "todo" | "tbd" | "replace-me" | "placeholder" | "your-value"
        )
        || normalized.starts_with("your_")
        || normalized.starts_with("your-")
        || (normalized.starts_with('<') && normalized.ends_with('>'))
}

fn key_set(file: &EnvFile) -> BTreeSet<String> {
    file.entries.iter().map(|entry| entry.key.clone()).collect()
}

fn add_discovered_env_file(
    root: &Path,
    candidate: impl AsRef<Path>,
    reason: String,
    discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>,
) {
    let candidate = candidate.as_ref();
    let path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };

    let Ok(metadata) = fs::metadata(&path) else {
        return;
    };
    if !metadata.is_file() {
        return;
    }

    let Ok(canonical) = path.canonicalize() else {
        return;
    };
    if !canonical.starts_with(root) {
        return;
    }

    discovered.entry(canonical).or_default().insert(reason);
}

fn resolve_project_file(root: &str, path: &str) -> Result<PathBuf, String> {
    let root = Path::new(root)
        .canonicalize()
        .map_err(|error| format!("Project root is not accessible: {error}"))?;
    if !root.is_dir() {
        return Err("Project root is not a directory".to_string());
    }

    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("Env file is not accessible: {error}"))?;
    if !path.is_file() {
        return Err("Env path is not a file".to_string());
    }
    if !path.starts_with(&root) {
        return Err("Env file is outside the active project root".to_string());
    }

    Ok(path)
}

fn discover_package_json_env_files(
    root: &Path,
    discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>,
) {
    let path = root.join("package.json");
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };
    let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
        return;
    };

    for (script_name, script_value) in scripts {
        let Some(script) = script_value.as_str() else {
            continue;
        };
        for reference in extract_env_path_references(script) {
            add_discovered_env_file(
                root,
                reference,
                format!("package.json script `{script_name}`"),
                discovered,
            );
        }
    }
}

fn discover_compose_env_files(root: &Path, discovered: &mut BTreeMap<PathBuf, BTreeSet<String>>) {
    for compose_name in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ] {
        let path = root.join(compose_name);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for reference in extract_compose_env_file_references(&content) {
            add_discovered_env_file(
                root,
                reference,
                format!("{compose_name} env_file"),
                discovered,
            );
        }
    }
}

fn extract_env_path_references(command: &str) -> Vec<String> {
    let tokens = shell_tokens(command);
    let mut references = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if let Some(path) = token.strip_prefix("--env-file=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if let Some(path) = token.strip_prefix("DOTENV_CONFIG_PATH=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if let Some(path) = token.strip_prefix("dotenv_config_path=") {
            push_env_reference(&mut references, path);
            continue;
        }

        if token == "--env-file" {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
            continue;
        }

        if token == "-e"
            && tokens[..index]
                .iter()
                .rev()
                .take(3)
                .any(|value| value == "dotenv")
        {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
            continue;
        }

        if matches!(token.as_str(), "-f" | "--file")
            && tokens[..index]
                .iter()
                .rev()
                .take(3)
                .any(|value| value == "env-cmd")
        {
            if let Some(path) = tokens.get(index + 1) {
                push_env_reference(&mut references, path);
            }
        }
    }

    references
}

fn extract_compose_env_file_references(content: &str) -> Vec<String> {
    let mut references = Vec::new();
    let mut in_env_file_block = false;
    let mut block_indent = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("env_file:") {
            in_env_file_block = rest.trim().is_empty();
            block_indent = indent;
            if !in_env_file_block {
                for part in rest.trim().trim_matches(['[', ']']).split(',') {
                    push_env_reference(&mut references, part);
                }
            }
            continue;
        }

        if in_env_file_block {
            if indent <= block_indent && !trimmed.starts_with('-') {
                in_env_file_block = false;
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('-') {
                let candidate = rest.trim();
                if let Some(path) = candidate.strip_prefix("path:") {
                    push_env_reference(&mut references, path);
                } else {
                    push_env_reference(&mut references, candidate);
                }
            }
        }
    }

    references
}

fn push_env_reference(references: &mut Vec<String>, raw: &str) {
    let path = raw
        .trim()
        .trim_matches(['"', '\'', ',', ';'])
        .trim_start_matches("./");
    if path.is_empty() || !path.contains(".env") {
        return;
    }
    if path.contains("://") || path.starts_with("..") {
        return;
    }

    references.push(path.to_string());
}

fn shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;

    for character in input.chars() {
        match (quote, character) {
            (Some(active), value) if value == active => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (None, ',' | '[' | ']' | '{' | '}') => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn is_env_file_name(name: &str) -> bool {
    name == ".env"
        || name.starts_with(".env.")
        || matches!(name, ".envrc" | ".flaskenv")
        || name.ends_with(".env")
}

pub fn ensure_project_env_file(root: &Path, name: &str) -> io::Result<EnsureEnvFileOutcome> {
    validate_new_env_file_name(name)?;

    let canonical_root = root.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("project root {} is not accessible: {error}", root.display()),
        )
    })?;
    if !canonical_root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("project root {} is not a directory", root.display()),
        ));
    }

    let target = canonical_root.join(name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);

    let disposition = match options.open(&target) {
        Ok(file) => {
            drop(file);
            EnvFileCreationDisposition::Created
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&target).map_err(|metadata_error| {
                io::Error::new(
                    metadata_error.kind(),
                    format!(
                        "could not inspect existing env path {}: {metadata_error}",
                        target.display()
                    ),
                )
            })?;
            if !metadata.file_type().is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "refusing to use {} because it is not a regular file",
                        target.display()
                    ),
                ));
            }
            EnvFileCreationDisposition::AlreadyExists
        }
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("could not create env file {}: {error}", target.display()),
            ));
        }
    };

    let resolved_target = target
        .canonicalize()
        .ok()
        .filter(|path| path.parent() == Some(canonical_root.as_path()))
        .unwrap_or(target);

    Ok(EnsureEnvFileOutcome {
        disposition,
        path: resolved_target.to_string_lossy().to_string(),
    })
}

fn validate_new_env_file_name(name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    let is_single_normal_component = matches!(
        components.next(),
        Some(Component::Normal(component)) if component == OsStr::new(name)
    ) && components.next().is_none();
    let is_exact_basename = !name.is_empty()
        && name.trim() == name
        && is_single_normal_component
        && !name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        && !name.chars().any(char::is_control)
        && !name.ends_with('.')
        && !has_windows_reserved_file_stem(name)
        && name != "."
        && name != "..";

    if !is_exact_basename || !is_env_file_name(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "env file name must be one root-level name such as .env, .env.local, or worker.env",
        ));
    }

    Ok(())
}

fn has_windows_reserved_file_stem(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM¹"
            | "COM²"
            | "COM³"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT¹"
            | "LPT²"
            | "LPT³"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "CONIN$"
            | "CONOUT$"
    )
}

fn env_file_sort_key(name: &str) -> (u8, String) {
    let priority = match name {
        ".env" => 0,
        ".env.local" => 1,
        ".env.development" => 2,
        ".env.production" => 3,
        ".env.test" => 4,
        ".env.example" | ".env.sample" | ".env.template" | ".env.defaults" => 5,
        _ => 9,
    };
    (priority, name.to_string())
}

pub fn atomic_write_preserving_metadata(path: &Path, content: &str) -> io::Result<()> {
    atomic_write_metadata_if_unchanged(path, content, None)
}

/// VNE-SEC-013 overwrite precondition: when `unchanged` carries the bytes
/// the edit was computed from, the file on disk must still hold exactly
/// those bytes at persist time. A concurrent edit or identity swap refuses
/// (exit 2) instead of clobbering or writing through a stale identity.
pub fn atomic_write_metadata_if_unchanged(
    path: &Path,
    content: &str,
    unchanged: Option<&str>,
) -> io::Result<()> {
    // Identity at entry: the persist-time re-check requires the SAME inode
    // that was on the path when the write began (VNE-SEC-013).
    #[cfg(unix)]
    let identity_at_entry = {
        use std::os::unix::fs::MetadataExt;
        let stat = fs::symlink_metadata(path)?;
        (stat.dev(), stat.ino())
    };
    let original = fs::symlink_metadata(path)?;
    if original.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to write through a symlink",
        ));
    }
    let permissions = original.permissions();
    // Security metadata travels with the rewrite (VNE-SEC-012): the full
    // xattr set is copied onto the replacement before it takes the name.
    // Listing failure fails closed — writing metadata-blind is worse.
    let mut xattr_names: Vec<std::ffi::OsString> = Vec::new();
    for name in xattr::list(path)? {
        xattr_names.push(name);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("env");
    let temporary_prefix = format!(".{file_name}.vne-");
    let mut builder = tempfile::Builder::new();
    builder
        .prefix(&temporary_prefix)
        .permissions(permissions.clone());
    let mut temporary = builder.tempfile_in(parent)?;
    if let Some(expected) = unchanged {
        let current = fs::read(path)?;
        if current != expected.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "file changed since it was read; refusing to overwrite (rerun the command)",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let now = fs::symlink_metadata(path)?;
            if (now.dev(), now.ino()) != identity_at_entry {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "file identity changed since the write began; refusing to replace (rerun the command)",
                ));
            }
        }
    }
    temporary.write_all(content.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.as_file().set_permissions(permissions)?;
    for name in &xattr_names {
        let value = xattr::get(path, name)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "xattr set changed during the write; refusing to continue (rerun the command)",
            )
        })?;
        xattr::set(temporary.path(), name, &value)?;
    }
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn shape(
    kind: &str,
    label: &str,
    confidence: &str,
    redacted_by_default: bool,
    reasons: Vec<String>,
) -> KeyShape {
    shape_with_context(
        kind,
        label,
        confidence,
        redacted_by_default,
        redacted_by_default,
        None,
        reasons,
    )
}

fn shape_with_context(
    kind: &str,
    label: &str,
    confidence: &str,
    redacted_by_default: bool,
    sensitive: bool,
    exposure: Option<&str>,
    reasons: Vec<String>,
) -> KeyShape {
    KeyShape {
        kind: kind.to_string(),
        label: label.to_string(),
        confidence: confidence.to_string(),
        redacted_by_default,
        sensitive,
        exposure: exposure.map(str::to_string),
        reasons,
    }
}

fn entry_id(key: &str, line_number: usize) -> String {
    format!("{key}@{line_number}")
}

fn key_without_public_prefix(upper: &str) -> &str {
    upper
        .strip_prefix("NEXT_PUBLIC_")
        .or_else(|| upper.strip_prefix("VITE_"))
        .or_else(|| upper.strip_prefix("PUBLIC_"))
        .unwrap_or(upper)
}

fn known_key_profile(upper: &str) -> Option<KnownKeyProfile> {
    match key_without_public_prefix(upper) {
        "SUPABASE_SERVICE_ROLE_KEY" => {
            Some(KnownKeyProfile::Secret("Supabase service-role credential"))
        }
        "SUPABASE_ANON_KEY" => Some(KnownKeyProfile::Public("Supabase anonymous public key")),
        _ => None,
    }
}

fn contains_secretish_key_name(upper: &str) -> bool {
    contains_any(upper, SECRET_KEY_MARKERS)
}

fn contains_secret_value_token(value: &str) -> bool {
    SECRET_VALUE_PREFIXES.iter().any(|prefix| {
        value.match_indices(prefix).any(|(index, _)| {
            index == 0
                || value[..index]
                    .chars()
                    .next_back()
                    .is_some_and(|character| !is_credential_identifier_character(character))
        })
    })
}

fn is_credential_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn value_has_credential_signature(value: &str) -> bool {
    contains_secret_value_token(value) || contains_jwt_token(value)
}

fn contains_jwt_token(value: &str) -> bool {
    value.match_indices("eyJ").any(|(start, _)| {
        let candidate = &value[start..];
        let end = candidate
            .char_indices()
            .find_map(|(index, character)| {
                (!is_jwt_candidate_character(character)).then_some(index)
            })
            .unwrap_or(candidate.len());
        let segments = candidate[..end].split('.').collect::<Vec<_>>();
        segments.windows(3).any(jwt_segments_are_valid)
    })
}

fn is_jwt_candidate_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '=')
}

fn looks_like_jwt(value: &str) -> bool {
    let segments = value.split('.').collect::<Vec<_>>();
    jwt_segments_are_valid(&segments)
}

fn jwt_segments_are_valid(segments: &[&str]) -> bool {
    segments.len() == 3
        && segments[0].starts_with("eyJ")
        && segments.iter().all(|segment| {
            let unpadded = segment.trim_end_matches('=');
            let padding = segment.len() - unpadded.len();
            !unpadded.is_empty()
                && padding <= 2
                && !unpadded.contains('=')
                && unpadded.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
        })
}

fn known_public_credential_value(upper: &str, value: &str) -> bool {
    match key_without_public_prefix(upper) {
        "SUPABASE_ANON_KEY" => jwt_has_role(value, "anon"),
        _ => false,
    }
}

fn jwt_has_role(value: &str, expected_role: &str) -> bool {
    if !looks_like_jwt(value) {
        return false;
    }

    let Some(payload) = value.split('.').nth(1) else {
        return false;
    };
    let Ok(decoded) = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')) else {
        return false;
    };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&decoded) else {
        return false;
    };

    payload.get("role").and_then(|role| role.as_str()) == Some(expected_role)
}

fn provider_reason(upper: &str) -> Option<String> {
    let provider = if upper.starts_with("AWS_") {
        "AWS"
    } else if upper.starts_with("GCP_") || upper.starts_with("GOOGLE_") {
        "Google Cloud"
    } else if upper.starts_with("AZURE_") {
        "Azure"
    } else if upper.starts_with("STRIPE_") {
        "Stripe"
    } else if upper.starts_with("SUPABASE_") {
        "Supabase"
    } else if upper.starts_with("CLERK_") {
        "Clerk"
    } else if upper.starts_with("SENTRY_") {
        "Sentry"
    } else if upper.starts_with("OPENAI_") {
        "OpenAI"
    } else if upper.starts_with("ANTHROPIC_") {
        "Anthropic"
    } else {
        return None;
    };

    Some(format!("{provider} convention"))
}

fn is_credential_url_key(upper: &str) -> bool {
    let url_key =
        upper.ends_with("_URL") || upper.ends_with("_URI") || upper == "URL" || upper == "URI";
    if !url_key {
        return false;
    }

    upper == "DB_URL"
        || upper == "DB_URI"
        || upper.ends_with("_DB_URL")
        || upper.ends_with("_DB_URI")
        || contains_any(
            upper,
            &[
                "DATABASE",
                "POSTGRES",
                "POSTGRESQL",
                "MYSQL",
                "MARIADB",
                "REDIS",
                "VALKEY",
                "MONGO",
                "MONGODB",
                "SQLSERVER",
                "MSSQL",
                "RABBITMQ",
                "AMQP",
                "SMTP",
                "WEBHOOK",
                "CLICKHOUSE",
                "ELASTICSEARCH",
                "OPENSEARCH",
                "KAFKA",
                "NEON",
                "PLANETSCALE",
            ],
        )
}

fn url_value_has_secretish_parts(value: &str) -> bool {
    let Some((_, after_scheme)) = value.trim().split_once("://") else {
        return false;
    };

    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    if after_scheme[..authority_end].contains('@') {
        return true;
    }

    let Some(query_start) = after_scheme.find('?') else {
        return false;
    };
    let query = after_scheme[query_start + 1..]
        .split('#')
        .next()
        .unwrap_or_default();

    query.split('&').any(|parameter| {
        let name = parameter
            .split_once('=')
            .map(|(name, _)| name)
            .unwrap_or(parameter)
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "api_key"
                | "apikey"
                | "access_key"
                | "access_token"
                | "auth"
                | "authorization"
                | "client_secret"
                | "key"
                | "password"
                | "passwd"
                | "pwd"
                | "secret"
                | "signature"
                | "sig"
                | "token"
        ) || name.ends_with("_key")
            || name.ends_with("_secret")
            || name.ends_with("_token")
    })
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn skip_spaces(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && matches!(bytes[*index], b' ' | b'\t') {
        *index += 1;
    }
}

fn extract_inline_comment(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut index = 0;
    skip_spaces(bytes, &mut index);
    (bytes.get(index) == Some(&b'#')).then(|| segment[index..].trim().to_string())
}

fn current_line_body_end(content: &str, line_start: usize) -> usize {
    let line_end = next_line_start(content, line_start);
    content[line_start..line_end]
        .trim_end_matches(['\r', '\n'])
        .len()
        + line_start
}

fn next_line_start(content: &str, offset: usize) -> usize {
    content[offset..]
        .find('\n')
        .map(|position| offset + position + 1)
        .unwrap_or(content.len())
}

fn count_newlines(value: &str) -> usize {
    value
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

fn is_key_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_key_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

fn is_valid_env_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.first().is_some_and(|first| is_key_start(*first))
        && bytes.iter().skip(1).all(|byte| is_key_continue(*byte))
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value.contains(char::is_whitespace)
        || value.contains('#')
        || value.contains('"')
        || value.contains('\'')
        || value.contains('\n')
}

fn escape_double_quoted_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_single_quoted_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn looks_like_duration(value: &str) -> bool {
    let split_at = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    if split_at == 0 || split_at == value.len() {
        return false;
    }
    matches!(&value[split_at..], "ms" | "s" | "m" | "h" | "d")
}

fn looks_like_uuid(value: &str) -> bool {
    let parts = value.split('-').map(str::len).collect::<Vec<_>>();
    parts == [8, 4, 4, 4, 12]
        && value
            .chars()
            .all(|character| character == '-' || character.is_ascii_hexdigit())
}

fn looks_like_base64(value: &str) -> bool {
    value.len() >= 16
        && value.len() % 4 == 0
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '_' | '-' | '=')
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn atomic_write_refuses_when_precondition_fails() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        std::fs::write(&p, "A=1\n").unwrap();
        let err = atomic_write_metadata_if_unchanged(&p, "A=2\n", Some("A=9\n")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "A=1\n", "refusal leaves content intact");
        atomic_write_metadata_if_unchanged(&p, "A=2\n", Some("A=1\n")).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "A=2\n");
    }

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_entries_without_destroying_file_structure() {
        let content = "# db\nexport DATABASE_URL=\"postgres://localhost/app\" # local\n\nNEXT_PUBLIC_SITE=https://example.com\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());

        assert_eq!(file.content, content);
        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].id, "DATABASE_URL@2");
        assert_eq!(file.entries[0].key, "DATABASE_URL");
        assert_eq!(file.entries[0].value, "postgres://localhost/app");
        assert_eq!(file.entries[0].quote.as_deref(), Some("\""));
        assert_eq!(file.entries[0].comment.as_deref(), Some("# local"));
        assert!(file.entries[0].exported);
        assert_eq!(file.entries[1].shape.kind, "public");
    }

    #[test]
    fn parses_quoted_multiline_values_as_one_entry() {
        let content = "PRIVATE_KEY=\"-----BEGIN KEY-----\nabc123\n-----END KEY-----\"\nNEXT_PUBLIC_SITE=https://example.com\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());

        assert_eq!(file.entries.len(), 2);
        assert_eq!(file.entries[0].key, "PRIVATE_KEY");
        assert_eq!(
            file.entries[0].value,
            "-----BEGIN KEY-----\nabc123\n-----END KEY-----"
        );
        assert_eq!(file.entries[1].key, "NEXT_PUBLIC_SITE");
    }

    #[test]
    fn parses_unquoted_inline_comments_as_source_context() {
        let file = parse_env_file(Path::new(".env"), "PORT=1420 # dev server\n".to_string());

        assert_eq!(file.entries[0].value, "1420");
        assert_eq!(file.entries[0].comment.as_deref(), Some("# dev server"));
    }

    #[test]
    fn parses_dotenv_dialect_matrix_and_preserves_unrelated_bytes() {
        let content = "\u{feff}export EMPTY=\r\nHASHED=\"abc#def\"\r\nUNQUOTED_HASH=abc#def\r\nESCAPED_HASH=abc\\#def\r\nCOMMENTED=value # trailing\r\nSINGLE='can\\'t'\r\nJSON={\"a\":1}\r\nINVALID KEY=no\r\nDUP=one\r\nDUP=two\r\nMULTILINE=\"one\r\ntwo\"\r\nNEEDS_QUOTES=plain\r\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());
        let entry = |key: &str| {
            file.entries
                .iter()
                .find(|entry| entry.key == key)
                .unwrap_or_else(|| panic!("missing {key}"))
        };

        assert_eq!(entry("EMPTY").value, "");
        assert!(entry("EMPTY").exported);
        assert_eq!(entry("HASHED").value, "abc#def");
        assert_eq!(entry("UNQUOTED_HASH").value, "abc#def");
        assert_eq!(entry("ESCAPED_HASH").value, "abc\\#def");
        assert_eq!(entry("COMMENTED").value, "value");
        assert_eq!(entry("COMMENTED").comment.as_deref(), Some("# trailing"));
        assert_eq!(entry("SINGLE").quote, Some("'".to_string()));
        assert_eq!(entry("MULTILINE").value, "one\r\ntwo");
        assert!(file.entries.iter().all(|entry| entry.key != "INVALID"));
        assert_eq!(file.duplicate_keys, vec!["DUP"]);

        let updated = replace_env_value_at(
            content,
            "NEEDS_QUOTES",
            entry("NEEDS_QUOTES").line_number,
            "hello world #1",
        )
        .unwrap();
        assert!(updated.starts_with('\u{feff}'));
        assert!(updated.contains("COMMENTED=value # trailing\r\n"));
        assert!(updated.contains("NEEDS_QUOTES=\"hello world #1\"\r\n"));
    }

    #[test]
    fn loads_public_adversarial_dotenv_corpus() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let corpus = repo_root.join("fixtures/adversarial-dotenv");
        let mut loaded = 0;

        for entry in fs::read_dir(&corpus).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("env") {
                continue;
            }

            let content = fs::read_to_string(&path).unwrap();
            let file = parse_env_file(&path, content.clone());
            assert_eq!(file.content, content, "fixture content must stay intact");
            assert!(
                !file.entries.is_empty(),
                "{} should parse at least one assignment",
                path.display()
            );
            loaded += 1;

            if path.file_name().and_then(|name| name.to_str()) == Some("duplicates-invalid.env") {
                assert_eq!(file.duplicate_keys, vec!["DUP"]);
                assert!(file.entries.iter().all(|entry| entry.key != "INVALID"));
                assert!(file.entries.iter().all(|entry| entry.key != "1INVALID"));

                let updated =
                    replace_env_value_at(&content, "NEEDS_QUOTES", 7, "needs quotes # from corpus")
                        .unwrap();
                assert!(updated.contains("NEEDS_QUOTES=\"needs quotes # from corpus\"\n"));
                assert!(updated.contains("DUP=one\nDUP=two\n"));
            }
        }

        assert_eq!(loaded, 3);
    }

    #[test]
    fn replaces_one_value_while_preserving_comments_order_and_quote_style() {
        let content = "# keep\nDATABASE_URL=\"postgres://old/app\" # local\nPORT=3000\n";
        let updated = replace_env_value(content, "DATABASE_URL", "postgres://new/app").unwrap();

        assert_eq!(
            updated,
            "# keep\nDATABASE_URL=\"postgres://new/app\" # local\nPORT=3000\n"
        );
    }

    #[test]
    fn replaces_duplicate_key_by_selected_line_number() {
        let content = "FEATURE_ENABLED=true\nFEATURE_ENABLED=false\nPORT=1420\n";
        let updated = replace_env_value_at(content, "FEATURE_ENABLED", 2, "maybe").unwrap();

        assert_eq!(
            updated,
            "FEATURE_ENABLED=true\nFEATURE_ENABLED=maybe\nPORT=1420\n"
        );
        assert!(replace_env_value_at(content, "FEATURE_ENABLED", 3, "maybe").is_none());
    }

    #[test]
    fn replaces_multiline_value_without_collapsing_lines() {
        let content = "PRIVATE_KEY=\"-----BEGIN KEY-----\nold\n-----END KEY-----\"\nPORT=1420\n";
        let updated = replace_env_value(
            content,
            "PRIVATE_KEY",
            "-----BEGIN KEY-----\nnew\n-----END KEY-----",
        )
        .unwrap();

        assert_eq!(
            updated,
            "PRIVATE_KEY=\"-----BEGIN KEY-----\nnew\n-----END KEY-----\"\nPORT=1420\n"
        );
        let file = parse_env_file(Path::new(".env"), updated);
        assert_eq!(
            file.entries[0].value,
            "-----BEGIN KEY-----\nnew\n-----END KEY-----"
        );
    }

    #[test]
    fn appends_missing_key_value_without_rewriting_existing_content() {
        let content = "# keep\nPORT=1420";
        let updated = append_env_key(content, "REDIS_URL", "redis://localhost:6379").unwrap();

        assert_eq!(
            updated,
            "# keep\nPORT=1420\nREDIS_URL=redis://localhost:6379\n"
        );
        let file = parse_env_file(Path::new(".env"), updated);
        assert_eq!(file.entries[0].key, "PORT");
        assert_eq!(file.entries[1].key, "REDIS_URL");
        assert_eq!(file.entries[1].value, "redis://localhost:6379");
    }

    #[test]
    fn refuses_to_append_duplicate_invalid_or_empty_keys() {
        assert!(append_env_key("PORT=1420\n", "PORT", "1421").is_none());
        assert!(append_env_key("PORT=1420\n", "1INVALID", "value").is_none());
        assert!(append_env_key("PORT=1420\n", "REDIS_URL", "").is_none());
        assert!(append_env_key("PORT=1420\n", "REDIS_URL", "   ").is_none());
    }

    #[test]
    fn reports_duplicate_lines_when_appending_existing_key() {
        let error = append_env_key_result("PORT=1420\nPORT=3000\n", "PORT", "8080")
            .expect_err("duplicate append should fail");

        assert_eq!(
            error,
            AppendEnvKeyError::Duplicate {
                line_numbers: vec![1, 2]
            }
        );
        assert_eq!(
            error.message("PORT"),
            "`PORT` already exists at lines 1, 2; edit the existing occurrence instead"
        );
    }

    #[test]
    fn copy_preserves_exact_value_tokens_and_destination_structure() {
        let source = "SECRET=\"alpha\\\"beta\\\\path\nline two\"\n";
        let destination = "\u{feff}# keep\r\nPORT=1420\r\n";

        let (updated, disposition) =
            copy_env_key_content(source, destination, "SECRET", false).unwrap();

        assert_eq!(disposition, EnvKeyCopyDisposition::Added);
        assert_eq!(
            updated,
            "\u{feff}# keep\r\nPORT=1420\r\nSECRET=\"alpha\\\"beta\\\\path\nline two\"\r\n"
        );
        assert!(updated.starts_with("\u{feff}# keep\r\nPORT=1420\r\n"));
    }

    #[test]
    fn copy_accepts_empty_source_value() {
        let (updated, disposition) =
            copy_env_key_content("EMPTY=\n", "PORT=1420\n", "EMPTY", false).unwrap();

        assert_eq!(disposition, EnvKeyCopyDisposition::Added);
        assert_eq!(updated, "PORT=1420\nEMPTY=\n");
    }

    #[test]
    fn copy_destination_policy_is_explicit_and_retry_safe() {
        let source = "TOKEN='same'\n";

        let (unchanged, disposition) =
            copy_env_key_content(source, "TOKEN='same' # keep\n", "TOKEN", false).unwrap();
        assert_eq!(disposition, EnvKeyCopyDisposition::AlreadyPresent);
        assert_eq!(unchanged, "TOKEN='same' # keep\n");

        assert!(matches!(
            copy_env_key_content(source, "TOKEN=other\n", "TOKEN", false),
            Err(EnvKeyCopyError::DestinationConflict { line_number: 1 })
        ));

        let (updated, disposition) =
            copy_env_key_content(source, "TOKEN=other # keep\n", "TOKEN", true).unwrap();
        assert_eq!(disposition, EnvKeyCopyDisposition::Overwritten);
        assert_eq!(updated, "TOKEN='same' # keep\n");
    }

    #[test]
    fn copy_refuses_missing_duplicate_and_malformed_occurrences() {
        assert!(matches!(
            copy_env_key_content("PORT=1\n", "", "TOKEN", false),
            Err(EnvKeyCopyError::SourceMissing)
        ));
        assert!(matches!(
            copy_env_key_content("TOKEN=one\nTOKEN=two\n", "", "TOKEN", false),
            Err(EnvKeyCopyError::SourceDuplicate { .. })
        ));
        assert!(matches!(
            copy_env_key_content("TOKEN=\"not closed\n", "", "TOKEN", false),
            Err(EnvKeyCopyError::SourceMalformed { line_number: 1 })
        ));
        assert!(matches!(
            copy_env_key_content("TOKEN=one\n", "TOKEN=a\nTOKEN=b\n", "TOKEN", true),
            Err(EnvKeyCopyError::DestinationDuplicate { .. })
        ));
        assert!(matches!(
            copy_env_key_content("TOKEN=one\n", "TOKEN=\"not closed\n", "TOKEN", true),
            Err(EnvKeyCopyError::DestinationMalformed { line_number: 1 })
        ));
    }

    #[test]
    fn set_updates_one_occurrence_and_preserves_surrounding_structure() {
        let content = "\u{feff}# keep\r\nPORT=1420\r\nTOKEN='old' # local\r\n";

        let (updated, disposition) = set_env_key_content(content, "TOKEN", "new").unwrap();

        assert_eq!(disposition, EnvKeySetDisposition::Updated);
        assert_eq!(
            updated,
            "\u{feff}# keep\r\nPORT=1420\r\nTOKEN='new' # local\r\n"
        );
    }

    #[test]
    fn set_quotes_a_value_that_needs_quoting_without_touching_other_lines() {
        let (updated, disposition) =
            set_env_key_content("PORT=1420\nTOKEN=plain\n", "TOKEN", "has space").unwrap();

        assert_eq!(disposition, EnvKeySetDisposition::Updated);
        assert_eq!(updated, "PORT=1420\nTOKEN=\"has space\"\n");
    }

    #[test]
    fn set_reports_an_identical_parsed_value_without_rewriting_content() {
        let content = "TOKEN='same' # keep\n";

        let (unchanged, disposition) = set_env_key_content(content, "TOKEN", "same").unwrap();

        assert_eq!(disposition, EnvKeySetDisposition::AlreadyPresent);
        assert_eq!(unchanged, content);
    }

    #[test]
    fn set_refuses_an_empty_value_unless_the_caller_asks_for_one() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, "TOKEN=old\n").unwrap();

        for blank in ["", "   ", "\t"] {
            let error = set_env_key(&env_file, "TOKEN", blank, false).unwrap_err();
            assert!(matches!(error, EnvKeySetError::EmptyValue), "{blank:?}");
            assert_eq!(fs::read_to_string(&env_file).unwrap(), "TOKEN=old\n");
        }

        let outcome = set_env_key(&env_file, "TOKEN", "", true).unwrap();

        assert_eq!(outcome.disposition, EnvKeySetDisposition::Updated);
        assert_eq!(fs::read_to_string(&env_file).unwrap(), "TOKEN=\"\"\n");
    }

    #[test]
    fn set_empty_value_message_names_both_ways_out() {
        assert_eq!(
            EnvKeySetError::EmptyValue.message("TOKEN"),
            "`TOKEN` was given an empty value; pass --allow-empty to store one, or use `vne rm` to remove the key"
        );
    }

    #[test]
    fn set_refuses_missing_duplicate_malformed_and_invalid_keys() {
        assert!(matches!(
            set_env_key_content("PORT=1420\n", "TOKEN", "new"),
            Err(EnvKeySetError::Missing)
        ));
        assert!(matches!(
            set_env_key_content("TOKEN=one\nTOKEN=two\n", "TOKEN", "new"),
            Err(EnvKeySetError::Duplicate { ref line_numbers }) if line_numbers == &[1, 2]
        ));
        assert!(matches!(
            set_env_key_content("TOKEN=\"not closed\n", "TOKEN", "new"),
            Err(EnvKeySetError::Malformed { line_number: 1 })
        ));
        assert!(matches!(
            set_env_key_content("TOKEN=one\n", "not a key", "new"),
            Err(EnvKeySetError::InvalidKey)
        ));
    }

    #[test]
    fn set_error_messages_name_keys_and_lines_without_values() {
        assert_eq!(
            EnvKeySetError::Missing.message("TOKEN"),
            "`TOKEN` was not found; use `vne add` to create it"
        );
        assert_eq!(
            EnvKeySetError::Duplicate {
                line_numbers: vec![2, 7]
            }
            .message("TOKEN"),
            "`TOKEN` is ambiguous at lines 2, 7; remove duplicates before setting it"
        );
        assert_eq!(
            EnvKeySetError::Malformed { line_number: 3 }.message("TOKEN"),
            "`TOKEN` has a malformed value at line 3"
        );
    }

    #[test]
    fn set_refuses_a_target_that_is_not_a_regular_file() {
        let dir = tempdir().unwrap();

        let error = set_env_key(dir.path(), "TOKEN", "new", false).unwrap_err();

        assert!(matches!(
            error,
            EnvKeySetError::Access(EnvFileAccessError::NonRegularFile { role: "target", .. })
        ));
        assert!(error.message("TOKEN").contains("regular file"));
    }

    #[test]
    fn set_writes_through_the_shared_atomic_writer() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, "TOKEN=old\n").unwrap();

        let outcome = set_env_key(&env_file, "TOKEN", "new", false).unwrap();

        assert_eq!(outcome.disposition, EnvKeySetDisposition::Updated);
        assert_eq!(outcome.key, "TOKEN");
        assert_eq!(
            outcome.path,
            env_file.canonicalize().unwrap().to_string_lossy()
        );
        assert_eq!(fs::read_to_string(&env_file).unwrap(), "TOKEN=new\n");
    }

    fn removal(
        content: &str,
        key: &str,
        selector: EnvKeyRemoveSelector,
    ) -> Result<EnvKeyRemovePlan, EnvKeyRemoveError> {
        remove_env_key_content(content, key, selector, None)
    }

    const THREE_KEYS: &str = "PORT=1420\nTOKEN=one\nDEBUG=true\n";
    const DUPLICATED: &str = "PORT=1420\nTOKEN=one\nDEBUG=true\nTOKEN=two\n";

    #[test]
    fn rm_removes_a_unique_occurrence_and_leaves_the_other_lines_byte_identical() {
        let removed = removal(THREE_KEYS, "TOKEN", EnvKeyRemoveSelector::Only).unwrap();

        assert_eq!(removed.disposition, EnvKeyRemoveDisposition::Removed);
        assert_eq!(removed.removed_lines, vec![2]);
        assert_eq!(removed.content.unwrap(), "PORT=1420\nDEBUG=true\n");
    }

    #[test]
    fn rm_reports_an_absent_key_without_writing_under_every_unselected_form() {
        for selector in [EnvKeyRemoveSelector::Only, EnvKeyRemoveSelector::All] {
            let removed = removal(THREE_KEYS, "ABSENT", selector).unwrap();

            assert_eq!(
                removed.disposition,
                EnvKeyRemoveDisposition::AlreadyAbsent,
                "{selector:?}"
            );
            assert!(removed.content.is_none(), "{selector:?}");
            assert!(removed.removed_lines.is_empty(), "{selector:?}");
        }
    }

    #[test]
    fn rm_refuses_a_duplicated_key_without_a_selector() {
        assert!(matches!(
            removal(DUPLICATED, "TOKEN", EnvKeyRemoveSelector::Only),
            Err(EnvKeyRemoveError::Duplicate { ref line_numbers }) if line_numbers == &[2, 4]
        ));
    }

    #[test]
    fn rm_line_selector_removes_only_the_named_duplicate() {
        let removed = removal(DUPLICATED, "TOKEN", EnvKeyRemoveSelector::Line(4)).unwrap();

        assert_eq!(removed.disposition, EnvKeyRemoveDisposition::Removed);
        assert_eq!(removed.removed_lines, vec![4]);
        assert_eq!(
            removed.content.unwrap(),
            "PORT=1420\nTOKEN=one\nDEBUG=true\n"
        );
    }

    #[test]
    fn rm_line_selector_fails_closed_and_never_hunts_for_another_occurrence() {
        assert!(matches!(
            removal(THREE_KEYS, "TOKEN", EnvKeyRemoveSelector::Line(1)),
            Err(EnvKeyRemoveError::StaleSelector {
                line_number: 1,
                found_key: Some(ref found),
            }) if found == "PORT"
        ));
        assert!(matches!(
            removal(THREE_KEYS, "TOKEN", EnvKeyRemoveSelector::Line(9)),
            Err(EnvKeyRemoveError::StaleSelector {
                line_number: 9,
                found_key: None,
            })
        ));
        assert!(matches!(
            removal(THREE_KEYS, "ABSENT", EnvKeyRemoveSelector::Line(2)),
            Err(EnvKeyRemoveError::StaleSelector {
                line_number: 2,
                found_key: Some(ref found),
            }) if found == "TOKEN"
        ));
    }

    #[test]
    fn rm_all_splices_every_occurrence_in_one_pass() {
        let removed = removal(DUPLICATED, "TOKEN", EnvKeyRemoveSelector::All).unwrap();

        assert_eq!(removed.disposition, EnvKeyRemoveDisposition::RemovedAll);
        assert_eq!(removed.removed_lines, vec![2, 4]);
        assert_eq!(removed.content.unwrap(), "PORT=1420\nDEBUG=true\n");
    }

    #[test]
    fn rm_all_of_a_unique_key_reports_a_single_removal() {
        let removed = removal(THREE_KEYS, "TOKEN", EnvKeyRemoveSelector::All).unwrap();

        assert_eq!(removed.disposition, EnvKeyRemoveDisposition::Removed);
        assert_eq!(removed.removed_lines, vec![2]);
    }

    #[test]
    fn rm_expectations_gate_the_convergent_no_op_and_the_removal() {
        assert!(matches!(
            remove_env_key_content(
                THREE_KEYS,
                "ABSENT",
                EnvKeyRemoveSelector::Only,
                Some(EnvKeyExpectation::Present)
            ),
            Err(EnvKeyRemoveError::ExpectedPresent)
        ));
        assert!(matches!(
            remove_env_key_content(
                DUPLICATED,
                "TOKEN",
                EnvKeyRemoveSelector::All,
                Some(EnvKeyExpectation::Absent)
            ),
            Err(EnvKeyRemoveError::ExpectedAbsent { ref line_numbers }) if line_numbers == &[2, 4]
        ));

        let absent = remove_env_key_content(
            THREE_KEYS,
            "ABSENT",
            EnvKeyRemoveSelector::Only,
            Some(EnvKeyExpectation::Absent),
        )
        .unwrap();
        assert_eq!(absent.disposition, EnvKeyRemoveDisposition::AlreadyAbsent);

        let present = remove_env_key_content(
            THREE_KEYS,
            "TOKEN",
            EnvKeyRemoveSelector::Only,
            Some(EnvKeyExpectation::Present),
        )
        .unwrap();
        assert_eq!(present.disposition, EnvKeyRemoveDisposition::Removed);
    }

    #[test]
    fn rm_preserves_a_leading_byte_order_mark_and_crlf_endings() {
        let removed = removal(
            "\u{feff}TOKEN=one\r\nPORT=1420\r\n",
            "TOKEN",
            EnvKeyRemoveSelector::Only,
        )
        .unwrap();

        assert_eq!(removed.content.unwrap(), "\u{feff}PORT=1420\r\n");
    }

    #[test]
    fn rm_splices_a_multiline_quoted_entry_completely() {
        let removed = removal(
            "A=1\nPRIVATE_KEY=\"line one\nline two\"\nB=2\n",
            "PRIVATE_KEY",
            EnvKeyRemoveSelector::Only,
        )
        .unwrap();

        assert_eq!(removed.content.unwrap(), "A=1\nB=2\n");
    }

    #[test]
    fn rm_removes_a_final_entry_without_a_trailing_newline() {
        let removed = removal("A=1\nTOKEN=one", "TOKEN", EnvKeyRemoveSelector::Only).unwrap();

        assert_eq!(removed.content.unwrap(), "A=1\n");
    }

    #[test]
    fn rm_refuses_a_malformed_entry_whose_extent_runs_to_end_of_file() {
        // An unclosed quote makes the parser read to the end of the file, so
        // splicing the "line" would delete every following key.
        let content = "TOKEN=\"oops\nPORT=1420\nDATABASE_URL=postgres://x\n";

        for selector in [
            EnvKeyRemoveSelector::Only,
            EnvKeyRemoveSelector::Line(1),
            EnvKeyRemoveSelector::All,
        ] {
            assert!(
                matches!(
                    removal(content, "TOKEN", selector),
                    Err(EnvKeyRemoveError::Malformed { line_number: 1 })
                ),
                "{selector:?} should refuse a malformed occurrence"
            );
        }
    }

    #[test]
    fn rm_error_messages_name_keys_and_lines_without_values() {
        assert_eq!(
            EnvKeyRemoveError::Duplicate {
                line_numbers: vec![2, 4]
            }
            .message("TOKEN"),
            "`TOKEN` is ambiguous at lines 2, 4; pass --line <N> or --all"
        );
        assert_eq!(
            EnvKeyRemoveError::Malformed { line_number: 3 }.message("TOKEN"),
            "`TOKEN` has a malformed value at line 3; its extent is unclear, so vne will not remove it"
        );
        assert_eq!(
            EnvKeyRemoveError::StaleSelector {
                line_number: 8,
                found_key: Some("PORT".to_string()),
            }
            .message("TOKEN"),
            "line 8 holds `PORT`, not `TOKEN`; re-read the file before selecting a line"
        );
        assert_eq!(
            EnvKeyRemoveError::StaleSelector {
                line_number: 8,
                found_key: None,
            }
            .message("TOKEN"),
            "line 8 holds no env assignment; re-read the file before selecting a line"
        );
        assert_eq!(
            EnvKeyRemoveError::ExpectedPresent.message("TOKEN"),
            "`TOKEN` is absent but --expect present was requested"
        );
        assert_eq!(
            EnvKeyRemoveError::ExpectedAbsent {
                line_numbers: vec![3]
            }
            .message("TOKEN"),
            "`TOKEN` is present at line 3 but --expect absent was requested"
        );
    }

    #[test]
    fn rm_writes_through_the_shared_atomic_writer_and_converges_on_retry() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, THREE_KEYS).unwrap();

        let removed = remove_env_key(&env_file, "TOKEN", EnvKeyRemoveSelector::Only, None).unwrap();
        assert_eq!(removed.disposition, EnvKeyRemoveDisposition::Removed);
        assert_eq!(removed.removed_lines, vec![2]);
        assert_eq!(
            fs::read_to_string(&env_file).unwrap(),
            "PORT=1420\nDEBUG=true\n"
        );

        let retried = remove_env_key(&env_file, "TOKEN", EnvKeyRemoveSelector::Only, None).unwrap();
        assert_eq!(retried.disposition, EnvKeyRemoveDisposition::AlreadyAbsent);
        assert!(retried.removed_lines.is_empty());
        assert_eq!(
            fs::read_to_string(&env_file).unwrap(),
            "PORT=1420\nDEBUG=true\n"
        );
    }

    #[test]
    fn rename_changes_only_the_key_token() {
        let content = "\u{feff}# keep\r\nexport TOKEN = \"alpha\\\"beta\" # local\r\nPORT=1420\r\n";

        let updated = rename_env_key_content(content, "TOKEN", "API_TOKEN").unwrap();

        assert_eq!(
            updated,
            "\u{feff}# keep\r\nexport API_TOKEN = \"alpha\\\"beta\" # local\r\nPORT=1420\r\n"
        );
    }

    #[test]
    fn rename_preserves_a_multiline_value_and_a_final_line_without_a_newline() {
        let multiline = rename_env_key_content(
            "A=1\nPRIVATE_KEY=\"line one\nline two\"\nB=2\n",
            "PRIVATE_KEY",
            "SIGNING_KEY",
        )
        .unwrap();
        assert_eq!(multiline, "A=1\nSIGNING_KEY=\"line one\nline two\"\nB=2\n");

        let trailing = rename_env_key_content("A=1\nTOKEN=one", "TOKEN", "API_TOKEN").unwrap();
        assert_eq!(trailing, "A=1\nAPI_TOKEN=one");
    }

    #[test]
    fn rename_refuses_an_existing_target_and_never_merges_two_keys() {
        assert!(matches!(
            rename_env_key_content("TOKEN=one\nAPI_TOKEN=two\n", "TOKEN", "API_TOKEN"),
            Err(EnvKeyRenameError::TargetExists { ref line_numbers }) if line_numbers == &[2]
        ));
    }

    #[test]
    fn rename_refuses_to_claim_a_rename_it_cannot_prove() {
        assert!(matches!(
            rename_env_key_content("API_TOKEN=two\n", "TOKEN", "API_TOKEN"),
            Err(EnvKeyRenameError::Indeterminate { ref line_numbers }) if line_numbers == &[1]
        ));
    }

    #[test]
    fn rename_refuses_missing_duplicate_malformed_invalid_and_identical_keys() {
        assert!(matches!(
            rename_env_key_content("PORT=1420\n", "TOKEN", "API_TOKEN"),
            Err(EnvKeyRenameError::SourceMissing)
        ));
        assert!(matches!(
            rename_env_key_content("TOKEN=one\nTOKEN=two\n", "TOKEN", "API_TOKEN"),
            Err(EnvKeyRenameError::SourceDuplicate { ref line_numbers }) if line_numbers == &[1, 2]
        ));
        assert!(matches!(
            rename_env_key_content("TOKEN=\"not closed\n", "TOKEN", "API_TOKEN"),
            Err(EnvKeyRenameError::SourceMalformed { line_number: 1 })
        ));
        assert!(matches!(
            rename_env_key_content("TOKEN=one\n", "not a key", "API_TOKEN"),
            Err(EnvKeyRenameError::InvalidSourceKey)
        ));
        assert!(matches!(
            rename_env_key_content("TOKEN=one\n", "TOKEN", "not a key"),
            Err(EnvKeyRenameError::InvalidTargetKey)
        ));
        assert!(matches!(
            rename_env_key_content("TOKEN=one\n", "TOKEN", "TOKEN"),
            Err(EnvKeyRenameError::SameKey)
        ));
    }

    #[test]
    fn rename_error_messages_name_keys_and_lines_without_values() {
        assert_eq!(
            EnvKeyRenameError::TargetExists {
                line_numbers: vec![4]
            }
            .message("TOKEN", "API_TOKEN"),
            "`API_TOKEN` already exists at line 4; remove it before renaming `TOKEN` onto it"
        );
        assert_eq!(
            EnvKeyRenameError::Indeterminate {
                line_numbers: vec![4]
            }
            .message("TOKEN", "API_TOKEN"),
            "`TOKEN` is absent and `API_TOKEN` is present at line 4; vne cannot confirm from file state that this rename already happened"
        );
        assert_eq!(
            EnvKeyRenameError::SourceMissing.message("TOKEN", "API_TOKEN"),
            "`TOKEN` was not found"
        );
    }

    #[test]
    fn rename_is_not_retry_safe_and_says_so_instead_of_writing_again() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, "PORT=1420\nTOKEN=one # keep\n").unwrap();

        let outcome = rename_env_key(&env_file, "TOKEN", "API_TOKEN").unwrap();
        assert_eq!(outcome.disposition, EnvKeyRenameDisposition::Renamed);
        assert_eq!(outcome.keys.from, "TOKEN");
        assert_eq!(outcome.keys.to, "API_TOKEN");
        let renamed = "PORT=1420\nAPI_TOKEN=one # keep\n";
        assert_eq!(fs::read_to_string(&env_file).unwrap(), renamed);

        let retried = rename_env_key(&env_file, "TOKEN", "API_TOKEN").unwrap_err();
        assert!(matches!(retried, EnvKeyRenameError::Indeterminate { .. }));
        assert_eq!(fs::read_to_string(&env_file).unwrap(), renamed);
    }

    #[test]
    fn example_appends_missing_keys_without_values_and_keeps_existing_lines() {
        let source =
            "DATABASE_URL=postgres://localhost/vne\nPORT=1420 # dev server\nTOKEN=secret\n";
        let example = "# contract\nDATABASE_URL=\n";

        let (updated, added) = append_missing_example_keys(source, example);

        assert_eq!(added, vec!["PORT".to_string(), "TOKEN".to_string()]);
        assert_eq!(
            updated,
            "# contract\nDATABASE_URL=\nPORT=\nTOKEN=\n"
        );
        assert!(updated.starts_with(example));
    }

    #[test]
    fn example_never_copies_a_secret_looking_inline_comment() {
        let source = "TOKEN=value # OPENAI_API_KEY=sk-live-1234567890abcdef\n";

        let (updated, added) = append_missing_example_keys(source, "");

        assert_eq!(added, vec!["TOKEN".to_string()]);
        assert_eq!(updated, "TOKEN=\n");
    }

    #[test]
    fn example_reports_nothing_to_add_when_every_key_is_present() {
        let (updated, added) = append_missing_example_keys("PORT=1420\n", "PORT=\n");

        assert!(added.is_empty());
        assert_eq!(updated, "PORT=\n");
    }

    #[test]
    fn example_adds_a_duplicated_source_key_once_and_preserves_line_endings() {
        let (updated, added) =
            append_missing_example_keys("PORT=1420\nPORT=1421\n", "DATABASE_URL=\r\n");

        assert_eq!(added, vec!["PORT".to_string()]);
        assert_eq!(updated, "DATABASE_URL=\r\nPORT=\r\n");
    }

    #[test]
    fn example_sync_is_additive_creates_a_missing_file_and_converges() {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".env");
        let example = dir.path().join(".env.example");
        fs::write(&source, "PORT=1420\nTOKEN=secret\n").unwrap();

        let created = sync_example_file(&source, None).unwrap();
        assert_eq!(created.disposition, ExampleSyncDisposition::Created);
        assert_eq!(created.added_keys, vec!["PORT", "TOKEN"]);
        assert_eq!(fs::read_to_string(&example).unwrap(), "PORT=\nTOKEN=\n");

        let converged = sync_example_file(&source, None).unwrap();
        assert_eq!(
            converged.disposition,
            ExampleSyncDisposition::AlreadyCurrent
        );
        assert!(converged.added_keys.is_empty());

        // An example-only key is left alone, and a new source key is appended.
        fs::write(&example, "PORT=\nTOKEN=\nEXAMPLE_ONLY=\n").unwrap();
        fs::write(&source, "PORT=1420\nTOKEN=secret\nQUEUE_URL=redis://x\n").unwrap();

        let updated = sync_example_file(&source, None).unwrap();
        assert_eq!(updated.disposition, ExampleSyncDisposition::Updated);
        assert_eq!(updated.added_keys, vec!["QUEUE_URL"]);
        assert_eq!(
            fs::read_to_string(&example).unwrap(),
            "PORT=\nTOKEN=\nEXAMPLE_ONLY=\nQUEUE_URL=\n"
        );
    }

    #[test]
    fn example_sync_refuses_a_source_that_is_its_own_example() {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".env");
        fs::write(&source, "PORT=1420\n").unwrap();

        let error = sync_example_file(&source, Some(&source)).unwrap_err();

        assert!(matches!(error, ExampleSyncError::SameFile));
        assert_eq!(fs::read_to_string(&source).unwrap(), "PORT=1420\n");
    }

    #[test]
    fn copy_rejects_paths_that_resolve_to_the_same_file() {
        let dir = tempdir().unwrap();
        let env_file = dir.path().join("shared.env");
        fs::write(&env_file, "TOKEN=one\n").unwrap();
        let alias = dir.path().join(".").join("shared.env");

        let error = copy_env_key(&env_file, "TOKEN", &alias, false).unwrap_err();

        assert!(matches!(error, EnvKeyCopyError::SameFile));
        assert_eq!(fs::read_to_string(env_file).unwrap(), "TOKEN=one\n");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_mode_and_ignores_predictable_stale_temp_path() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempdir().unwrap();
        let destination = dir.path().join("secret.env");
        let outside = dir.path().join("outside.env");
        fs::write(&destination, "TOKEN=old\n").unwrap();
        fs::write(&outside, "OUTSIDE=keep\n").unwrap();
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&outside, dir.path().join(".secret.env.vne.tmp")).unwrap();

        atomic_write_preserving_metadata(&destination, "TOKEN=new\n").unwrap();

        assert_eq!(fs::read_to_string(&destination).unwrap(), "TOKEN=new\n");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "OUTSIDE=keep\n");
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let temporary_entries = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".secret.env.vne-")
            })
            .count();
        assert_eq!(temporary_entries, 0);
    }

    #[test]
    fn escapes_replacement_inside_single_quotes() {
        let content = "GREETING='hello'\n";
        let updated = replace_env_value(content, "GREETING", "can't stop").unwrap();

        assert_eq!(updated, "GREETING='can\\'t stop'\n");
    }

    #[test]
    fn infers_common_key_shapes_and_redaction() {
        assert_eq!(infer_key_shape("OPENAI_API_KEY", "sk-test").kind, "secret");
        assert!(infer_key_shape("OPENAI_API_KEY", "sk-test").redacted_by_default);
        let redis = infer_key_shape("REDIS_URL", "redis://localhost:6379");
        assert_eq!(redis.kind, "credential-url");
        assert!(redis.redacted_by_default);
        assert_eq!(infer_key_shape("FEATURE_ENABLED", "true").kind, "bool");
        assert_eq!(infer_key_shape("PORT", "5173").kind, "port");
    }

    #[test]
    fn classifies_supabase_service_role_and_public_siblings() {
        let service_role = infer_key_shape("SUPABASE_SERVICE_ROLE_KEY", "ServiceRoleA1X");
        assert_eq!(service_role.kind, "secret");
        assert!(service_role.sensitive);
        assert!(service_role.redacted_by_default);
        assert!(service_role
            .reasons
            .contains(&"Supabase service-role credential".to_string()));

        let exposed_service_role =
            infer_key_shape("NEXT_PUBLIC_SUPABASE_SERVICE_ROLE_KEY", "ServiceRoleA1X");
        assert_eq!(exposed_service_role.kind, "public-secret");
        assert_eq!(exposed_service_role.exposure.as_deref(), Some("browser"));
        assert!(exposed_service_role.sensitive);

        let anon_jwt = "eyJhbGciOiJIUzI1NiJ9.eyJyb2xlIjoiYW5vbiJ9.signature";
        for key in ["SUPABASE_ANON_KEY", "NEXT_PUBLIC_SUPABASE_ANON_KEY"] {
            let anon = infer_key_shape(key, anon_jwt);
            assert_eq!(anon.kind, "public");
            assert!(!anon.sensitive);
            assert!(!anon.redacted_by_default);
            assert_eq!(anon.exposure.as_deref(), Some("browser"));
        }

        for key in ["SUPABASE_ANON_KEY", "NEXT_PUBLIC_SUPABASE_ANON_KEY"] {
            for value in [
                "sk-live-public-profile-override",
                "ghp_public_profile_override",
                "AKIAPUBLICPROFILEOVERRIDE",
                "-----BEGIN PRIVATE KEY-----",
                "postgres://user:pass@example.test/app",
                "eyJhbGciOiJIUzI1NiJ9.invalid.signature",
                "eyJhbGciOiJIUzI1NiJ9.eyJyb2xlIjoic2VydmljZV9yb2xlIn0.signature",
            ] {
                let misplaced_secret = infer_key_shape(key, value);
                assert!(
                    misplaced_secret.sensitive,
                    "strong secret evidence must override a public key profile: {key}={value}"
                );
                assert_eq!(misplaced_secret.exposure.as_deref(), Some("browser"));
            }
        }

        let padded_anon_payload =
            base64::engine::general_purpose::URL_SAFE.encode(br#"{"role":"anon","padding":"x"}"#);
        let padded_service_payload = base64::engine::general_purpose::URL_SAFE
            .encode(br#"{"role":"service_role","padding":"x"}"#);
        assert!(padded_anon_payload.ends_with('='));
        assert!(padded_service_payload.ends_with('='));

        let padded_anon = infer_key_shape(
            "SUPABASE_ANON_KEY",
            &format!("eyJhbGciOiJIUzI1NiJ9.{padded_anon_payload}.signature"),
        );
        assert!(!padded_anon.sensitive);

        let padded_service_role = infer_key_shape(
            "SUPABASE_ANON_KEY",
            &format!("eyJhbGciOiJIUzI1NiJ9.{padded_service_payload}.signature"),
        );
        assert!(padded_service_role.sensitive);
        assert_eq!(padded_service_role.exposure.as_deref(), Some("browser"));
    }

    #[test]
    fn classifies_known_credential_value_signatures_without_broad_lookalikes() {
        for value in [
            "sk-test",
            "sk_test",
            "ghp_test",
            "xoxb-test",
            "AKIATEST123",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.signature",
            "Bearer ghp_wrapped",
            "token=ghp_wrapped",
            "https://example.test/sk-embedded",
        ] {
            let shape = infer_key_shape("UNRECOGNIZED", value);
            assert_eq!(shape.kind, "secret", "{value} should be credential-like");
            assert!(shape.sensitive);
            assert!(shape.redacted_by_default);
        }

        // Lookalikes are still NOT classified secret (kind discipline), but
        // since the default-deny change they withhold like all unrecognized
        // shapes — absence of a secret label no longer means exposure.
        for (key, value) in [
            ("SK_THEME", "dark"),
            ("SERVICE_ROLE_NAME", "worker"),
            ("UNRECOGNIZED", "flask_config"),
            ("UNRECOGNIZED", "eyJ.not-a-complete-token"),
        ] {
            let shape = infer_key_shape(key, value);
            assert!(!shape.sensitive, "{key}={value}");
            assert!(shape.redacted_by_default, "{key}={value} must withhold (default-deny)");
        }

        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJwdW5jdHVhdGVkIn0.signature";
        for value in [format!("{jwt}."), format!("prefix.{jwt}")] {
            assert!(
                infer_key_shape("UNRECOGNIZED", &value).sensitive,
                "JWT evidence must survive adjacent dot punctuation: {value}"
            );
        }
    }

    #[test]
    fn key_shape_sensitivity_and_default_redaction_stay_in_lockstep() {
        for (key, value) in [
            ("FEATURE_FLAG", "true"),
            ("OPENAI_API_KEY", "sk-test"),
            ("NEXT_PUBLIC_API_KEY", "sk-browser"),
            ("SUPABASE_ANON_KEY", "anon"),
            ("SUPABASE_SERVICE_ROLE_KEY", "service-role"),
            ("DATABASE_URL", "postgres://user:pass@localhost/app"),
        ] {
            let shape = infer_key_shape(key, value);
            assert_eq!(
                shape.sensitive, shape.redacted_by_default,
                "{key} must preserve the classification invariant"
            );
        }
    }

    #[test]
    fn redacts_credential_bearing_urls_and_dsns() {
        let database = infer_key_shape("DATABASE_URL", "postgres://user:pass@localhost/app");
        assert_eq!(database.kind, "credential-url");
        assert!(database.redacted_by_default);

        let webhook = infer_key_shape("WEBHOOK_URL", "https://example.com/hook?token=abc");
        assert_eq!(webhook.kind, "credential-url");
        assert!(webhook.redacted_by_default);

        let sentry = infer_key_shape("SENTRY_DSN", "https://public@sentry.example/1");
        assert_eq!(sentry.kind, "dsn");
        assert!(sentry.redacted_by_default);

        let public_url = infer_key_shape("NEXT_PUBLIC_SITE_URL", "https://example.com");
        assert_eq!(public_url.kind, "public");
        assert!(!public_url.redacted_by_default);
        assert!(!public_url.sensitive);
        assert_eq!(public_url.exposure.as_deref(), Some("browser"));

        let public_with_token =
            infer_key_shape("VITE_CALLBACK_URL", "https://example.com/hook?token=abc");
        assert_eq!(public_with_token.kind, "credential-url");
        assert!(public_with_token.redacted_by_default);
        assert!(public_with_token.sensitive);
        assert_eq!(public_with_token.exposure.as_deref(), Some("browser"));

        let public_secret = infer_key_shape("NEXT_PUBLIC_API_KEY", "sk-browser-leak");
        assert_eq!(public_secret.kind, "public-secret");
        assert!(public_secret.redacted_by_default);
        assert!(public_secret.sensitive);
        assert_eq!(public_secret.exposure.as_deref(), Some("browser"));
        assert!(public_secret
            .reasons
            .contains(&"frontend-exposed prefix".to_string()));
        assert!(public_secret
            .reasons
            .contains(&"secret-like key name".to_string()));

        let api_url = infer_key_shape("API_BASE_URL", "https://api.example.com");
        assert_eq!(api_url.kind, "url");
        assert!(!api_url.redacted_by_default);
    }

    #[test]
    fn compares_env_against_example_and_names_duplicates() {
        let actual = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://local\nEXTRA=yes\nEXTRA=no\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nREDIS_URL=\n".to_string(),
        );
        let comparison = compare_actual_to_example(&[actual, example]).unwrap();

        assert_eq!(comparison.missing_keys, vec!["REDIS_URL"]);
        assert_eq!(comparison.extra_keys, vec!["EXTRA"]);
        assert_eq!(comparison.duplicate_keys, vec![".env:EXTRA"]);
    }

    #[test]
    fn scans_common_env_files_in_a_project() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://local\n").unwrap();
        fs::write(
            dir.path().join(".env.example"),
            "DATABASE_URL=\nREDIS_URL=\n",
        )
        .unwrap();
        fs::write(dir.path().join("notes.txt"), "ignore").unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();

        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(snapshot.comparison.unwrap().missing_keys, vec!["REDIS_URL"]);
    }

    #[test]
    fn discovers_env_files_referenced_by_common_project_config() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://local\n").unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::create_dir_all(dir.path().join("deploy")).unwrap();
        fs::write(
            dir.path().join("config").join("worker.env"),
            "QUEUE_URL=redis://localhost\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("deploy").join("compose.env"),
            "COMPOSE_PROJECT_NAME=vne\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"worker":"node --env-file=config/worker.env worker.js"}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("docker-compose.yml"),
            "services:\n  app:\n    env_file:\n      - deploy/compose.env\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let names = snapshot
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&".env"));
        assert!(names.contains(&"worker.env"));
        assert!(names.contains(&"compose.env"));
        assert!(snapshot
            .files
            .iter()
            .find(|file| file.name == "worker.env")
            .unwrap()
            .discovery_reasons
            .iter()
            .any(|reason| reason.contains("package.json")));
    }

    #[test]
    fn resolves_write_paths_only_inside_project_root() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let inside_file = project.path().join(".env");
        let outside_file = outside.path().join(".env");
        fs::write(&inside_file, "PORT=1420\n").unwrap();
        fs::write(&outside_file, "PORT=1420\n").unwrap();

        assert_eq!(
            resolve_project_file(
                project.path().to_str().unwrap(),
                inside_file.to_str().unwrap()
            )
            .unwrap(),
            inside_file.canonicalize().unwrap()
        );
        assert!(resolve_project_file(
            project.path().to_str().unwrap(),
            outside_file.to_str().unwrap()
        )
        .is_err());
    }

    #[test]
    fn accepts_supported_root_level_env_names() {
        let project = tempdir().unwrap();

        for name in [".env", ".env.local", ".envrc", ".flaskenv", "worker.env"] {
            let outcome = ensure_project_env_file(project.path(), name).unwrap();
            assert_eq!(outcome.disposition, EnvFileCreationDisposition::Created);
            assert_eq!(fs::read(project.path().join(name)).unwrap(), b"");
        }
    }

    #[test]
    fn rejects_non_basename_env_names_without_writing() {
        let project = tempdir().unwrap();

        for name in [
            "",
            ".",
            "..",
            "notes.txt",
            "../.env",
            "config/app.env",
            "config\\app.env",
            " .env",
            ".env ",
            ".env\nlocal",
            "/tmp/.env",
            "C:.env",
            ".env.x:y",
            ".env.local.",
            "CON.env",
            "CON .env",
            "com1.env",
            "com1 .env",
            "COM¹.env",
            "LPT9.env",
            "LPT³.env",
            ".env<local",
            ".env>local",
            ".env\"local",
            ".env|local",
            ".env?local",
            ".env*local",
        ] {
            assert!(
                ensure_project_env_file(project.path(), name).is_err(),
                "{name:?} should be rejected"
            );
        }

        assert!(fs::read_dir(project.path()).unwrap().next().is_none());
    }

    #[test]
    fn accepts_names_that_only_resemble_windows_devices() {
        let project = tempdir().unwrap();

        for name in ["COM10.env", "LPT0.env", "CONNECTION.env", ".env.CON"] {
            let outcome = ensure_project_env_file(project.path(), name).unwrap();
            assert_eq!(outcome.disposition, EnvFileCreationDisposition::Created);
        }
    }

    #[test]
    fn case_variant_outcome_path_matches_the_project_snapshot() {
        let project = tempdir().unwrap();
        let actual_path = project.path().join(".env.local");
        let requested_path = project.path().join(".env.Local");
        fs::write(&actual_path, b"KEEP=original\n").unwrap();
        let names_alias = requested_path.exists();

        let outcome = ensure_project_env_file(project.path(), ".env.Local").unwrap();
        let snapshot = snapshot_project(project.path()).unwrap();

        let expected_disposition = if names_alias {
            EnvFileCreationDisposition::AlreadyExists
        } else {
            EnvFileCreationDisposition::Created
        };
        assert_eq!(outcome.disposition, expected_disposition);
        assert!(snapshot.files.iter().any(|file| file.path == outcome.path));
        assert_eq!(fs::read(actual_path).unwrap(), b"KEEP=original\n");
    }

    #[test]
    fn existing_env_file_is_idempotent_and_byte_preserving() {
        let project = tempdir().unwrap();
        let env_path = project.path().join(".env");
        fs::write(&env_path, b"API_TOKEN=keep-me\n").unwrap();

        let outcome = ensure_project_env_file(project.path(), ".env").unwrap();

        assert_eq!(
            outcome.disposition,
            EnvFileCreationDisposition::AlreadyExists
        );
        assert_eq!(fs::read(env_path).unwrap(), b"API_TOKEN=keep-me\n");
    }

    #[test]
    fn concurrent_ensure_has_one_created_winner() {
        let project = tempdir().unwrap();
        let root = project.path().to_path_buf();
        let first_root = root.clone();
        let second_root = root.clone();
        let first = std::thread::spawn(move || ensure_project_env_file(&first_root, ".env"));
        let second = std::thread::spawn(move || ensure_project_env_file(&second_root, ".env"));

        let outcomes = [
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        ];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| outcome.disposition == EnvFileCreationDisposition::Created)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| {
                    outcome.disposition == EnvFileCreationDisposition::AlreadyExists
                })
                .count(),
            1
        );
        assert_eq!(fs::read(root.join(".env")).unwrap(), b"");
    }

    #[test]
    fn rejects_existing_directory_at_env_target() {
        let project = tempdir().unwrap();
        fs::create_dir(project.path().join("worker.env")).unwrap();

        assert!(ensure_project_env_file(project.path(), "worker.env").is_err());
        assert!(project.path().join("worker.env").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn creates_private_env_file_and_rejects_existing_symlink() {
        use std::os::unix::fs::{symlink, FileTypeExt, PermissionsExt};
        use std::os::unix::net::UnixListener;

        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.env");
        fs::write(&outside_file, b"OUTSIDE=untouched\n").unwrap();
        symlink(&outside_file, project.path().join("linked.env")).unwrap();

        assert!(ensure_project_env_file(project.path(), "linked.env").is_err());
        assert_eq!(fs::read(&outside_file).unwrap(), b"OUTSIDE=untouched\n");

        let socket_path = project.path().join("socket.env");
        let _listener = UnixListener::bind(&socket_path).unwrap();
        assert!(ensure_project_env_file(project.path(), "socket.env").is_err());
        assert!(fs::symlink_metadata(socket_path)
            .unwrap()
            .file_type()
            .is_socket());

        ensure_project_env_file(project.path(), ".env").unwrap();
        let mode = fs::metadata(project.path().join(".env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }

    #[test]
    fn ensure_outcome_serializes_without_env_payload() {
        let project = tempdir().unwrap();
        let outcome = ensure_project_env_file(project.path(), ".env").unwrap();

        let serialized = serde_json::to_value(outcome).unwrap();
        assert_eq!(serialized["disposition"], "created");
        assert_eq!(serialized.as_object().unwrap().len(), 2);
        assert!(serialized["path"].as_str().unwrap().ends_with("/.env"));
    }

    #[test]
    fn ensure_command_creates_a_discoverable_empty_file() {
        let project = tempdir().unwrap();
        let outcome = ensure_env_file(
            project.path().to_string_lossy().to_string(),
            ".env".to_string(),
        )
        .unwrap();

        let snapshot = load_project(project.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(outcome.disposition, EnvFileCreationDisposition::Created);
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, outcome.path);
        assert!(snapshot.files[0].content.is_empty());
        assert!(snapshot.files[0].entries.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_write_path_symlink_escape() {
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join(".env");
        let direct_link = project.path().join("linked.env");
        let nested_link_dir = project.path().join("linked-dir");
        let nested_link_file = nested_link_dir.join(".env");
        fs::write(&outside_file, "PORT=1420\n").unwrap();

        std::os::unix::fs::symlink(&outside_file, &direct_link).unwrap();
        std::os::unix::fs::symlink(outside.path(), &nested_link_dir).unwrap();

        assert!(resolve_project_file(
            project.path().to_str().unwrap(),
            direct_link.to_str().unwrap()
        )
        .is_err());
        assert!(resolve_project_file(
            project.path().to_str().unwrap(),
            nested_link_file.to_str().unwrap()
        )
        .is_err());
    }

    #[test]
    fn ignores_compose_env_file_references_outside_project_root() {
        let parent = tempdir().unwrap();
        let project = parent.path().join("project");
        let outside = parent.path().join("outside");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(project.join(".env"), "PORT=1420\n").unwrap();
        fs::write(outside.join(".env"), "API_TOKEN=outside\n").unwrap();
        fs::write(
            project.join("compose.yaml"),
            "services:\n  app:\n    env_file:\n      - ../outside/.env\n",
        )
        .unwrap();

        let snapshot = snapshot_project(&project).unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].name, ".env");
        assert!(!snapshot.files[0].content.contains("outside"));
    }

    #[test]
    fn load_project_redacts_secret_values_until_per_entry_reveal() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(
            &env_path,
            "API_TOKEN=sk-hidden\nPORT=1420\nDATABASE_URL=postgres://user:pass@localhost/app\n",
        )
        .unwrap();

        let hidden = load_project(dir.path().to_string_lossy().to_string()).unwrap();
        let hidden_file = hidden
            .files
            .iter()
            .find(|file| file.name == ".env")
            .unwrap();
        let token = hidden_file
            .entries
            .iter()
            .find(|entry| entry.key == "API_TOKEN")
            .unwrap();
        let port = hidden_file
            .entries
            .iter()
            .find(|entry| entry.key == "PORT")
            .unwrap();

        assert_eq!(token.value, "");
        assert_eq!(token.display_value, "********");
        assert_eq!(port.value, "1420");
        assert!(!hidden_file.content.contains("sk-hidden"));
        assert!(!hidden_file.content.contains("user:pass"));
        assert_eq!(hidden_file.content, RAW_PREVIEW_WITHHELD);

        let revealed_token = reveal_env_value(
            dir.path().to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
            "API_TOKEN".to_string(),
            1,
        )
        .unwrap();
        let revealed_database = reveal_env_value(
            dir.path().to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
            "DATABASE_URL".to_string(),
            3,
        )
        .unwrap();

        assert_eq!(revealed_token, "sk-hidden");
        assert_eq!(revealed_database, "postgres://user:pass@localhost/app");

        let still_hidden = load_project(dir.path().to_string_lossy().to_string()).unwrap();
        let still_hidden_file = still_hidden
            .files
            .iter()
            .find(|file| file.name == ".env")
            .unwrap();
        assert!(!still_hidden_file.content.contains("sk-hidden"));
        assert!(!still_hidden_file.content.contains("user:pass"));
    }

    #[test]
    fn load_project_withholds_secretish_comments_and_malformed_raw_preview() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".env"),
            "# API_TOKEN=whole-line-secret\nPORT=1420 # API_TOKEN=inline-comment-secret\nBROKEN TOKEN line\n",
        )
        .unwrap();

        let hidden = load_project(dir.path().to_string_lossy().to_string()).unwrap();
        let hidden_file = hidden
            .files
            .iter()
            .find(|file| file.name == ".env")
            .unwrap();
        let port = hidden_file
            .entries
            .iter()
            .find(|entry| entry.key == "PORT")
            .unwrap();

        assert_eq!(port.value, "1420");
        assert_eq!(port.comment.as_deref(), Some(INLINE_COMMENT_WITHHELD));
        assert_eq!(hidden_file.content, RAW_PREVIEW_WITHHELD);
        assert!(!hidden_file.content.contains("whole-line-secret"));
        assert!(!hidden_file.content.contains("inline-comment-secret"));
        assert!(!hidden_file.content.contains("BROKEN TOKEN"));
    }

    #[test]
    fn classified_redaction_scrubs_every_payload_surface() {
        let file = parse_env_file(
            Path::new(".env"),
            "SUPABASE_SERVICE_ROLE_KEY=ServiceRoleA1X # deployment\nPORT=1420 # TOKEN=CommentC3Z\n"
                .to_string(),
        );
        let hidden = redact_env_file(file);
        let service_role = &hidden.entries[0];
        let port = &hidden.entries[1];

        assert_eq!(service_role.value, "");
        assert_eq!(service_role.display_value, "********");
        assert_eq!(
            service_role.comment.as_deref(),
            Some(INLINE_COMMENT_WITHHELD)
        );
        assert_eq!(port.value, "1420");
        assert_eq!(port.display_value, "1420");
        assert_eq!(port.comment.as_deref(), Some(INLINE_COMMENT_WITHHELD));
        assert_eq!(hidden.content, RAW_PREVIEW_WITHHELD);
        assert!(!serde_json::to_string(&hidden)
            .unwrap()
            .contains("ServiceRoleA1X"));
        assert!(!serde_json::to_string(&hidden)
            .unwrap()
            .contains("CommentC3Z"));
    }

    #[test]
    fn classified_redaction_scrubs_jwt_comments_and_raw_content() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJDb21tZW50Snd0In0.signature";
        let file = parse_env_file(Path::new(".env"), format!("PORT=1420 # {jwt}.\n"));
        let hidden = redact_env_file(file);
        let serialized = serde_json::to_string(&hidden).unwrap();

        assert_eq!(
            hidden.entries[0].comment.as_deref(),
            Some(INLINE_COMMENT_WITHHELD)
        );
        assert_eq!(hidden.content, RAW_PREVIEW_WITHHELD);
        assert!(!serialized.contains(jwt));
    }

    #[test]
    fn classified_redaction_scrubs_jwt_in_unparsed_text() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJSYXdKd3QifQ.signature";
        let file = parse_env_file(Path::new(".env"), format!("# {jwt}\nPORT=1420\n"));
        let hidden = redact_env_file(file);

        assert_eq!(hidden.content, RAW_PREVIEW_WITHHELD);
        assert!(!serde_json::to_string(&hidden).unwrap().contains(jwt));
    }

    #[test]
    fn credential_prefix_lookalikes_preserve_comments_and_raw_content() {
        let content = "PORT=1420 # load flask_config before task_queue startup\n";
        let file = parse_env_file(Path::new(".env"), content.to_string());
        let visible = redact_env_file(file);

        assert_eq!(
            visible.entries[0].comment.as_deref(),
            Some("# load flask_config before task_queue startup")
        );
        assert_eq!(visible.content, content);
    }

    #[test]
    fn hide_all_projection_withholds_payloads_without_changing_classification() {
        let file = parse_env_file(
            Path::new(".env"),
            "PLAIN_SETTING=OrdinaryB2Y # ordinary comment\nSUPABASE_SERVICE_ROLE_KEY=ServiceRoleA1X\n"
                .to_string(),
        );
        let original_shapes = file
            .entries
            .iter()
            .map(|entry| entry.shape.clone())
            .collect::<Vec<_>>();
        let hidden = withhold_all_env_file_payloads(file);

        assert!(hidden
            .entries
            .iter()
            .all(|entry| entry.value == ALL_VALUES_WITHHELD));
        assert!(hidden
            .entries
            .iter()
            .all(|entry| entry.display_value == ALL_VALUES_WITHHELD));
        assert_eq!(
            hidden.entries[0].comment.as_deref(),
            Some(ALL_COMMENTS_WITHHELD)
        );
        assert_eq!(hidden.entries[1].comment, None);
        assert_eq!(hidden.content, "[raw content withheld by output policy]");
        assert_eq!(
            hidden
                .entries
                .iter()
                .map(|entry| entry.shape.clone())
                .collect::<Vec<_>>(),
            original_shapes
        );

        let serialized = serde_json::to_string(&hidden).unwrap();
        assert!(!serialized.contains("OrdinaryB2Y"));
        assert!(!serialized.contains("ServiceRoleA1X"));
        assert!(!serialized.contains("ordinary comment"));
    }

    #[test]
    fn values_only_projection_preserves_classified_values_and_withholds_context() {
        let file = parse_env_file(
            Path::new(".env"),
            "PLAIN_SETTING=OrdinaryB2Y # ordinary context\nSUPABASE_SERVICE_ROLE_KEY=ServiceRoleA1X # deployment\n"
                .to_string(),
        );
        let original_shapes = file
            .entries
            .iter()
            .map(|entry| entry.shape.clone())
            .collect::<Vec<_>>();
        let projected = redact_env_file_values_only(file);

        // default-deny: an unrecognized text value withholds under the
        // values-only projection too (VNE-SEC-002 CLI half)
        assert_eq!(projected.entries[0].value, "");
        assert_eq!(projected.entries[0].display_value, "********");
        assert_eq!(projected.entries[1].value, "");
        assert_eq!(projected.entries[1].display_value, "********");
        assert!(projected
            .entries
            .iter()
            .all(|entry| entry.comment.as_deref() == Some(ALL_COMMENTS_WITHHELD)));
        assert_eq!(projected.content, ALL_CONTENT_WITHHELD);
        assert_eq!(
            projected
                .entries
                .iter()
                .map(|entry| entry.shape.clone())
                .collect::<Vec<_>>(),
            original_shapes
        );
    }

    #[test]
    fn save_value_command_returns_rescanned_snapshot() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "API_TOKEN=todo\nPORT=1420\n").unwrap();
        fs::write(dir.path().join(".env.example"), "API_TOKEN=\nPORT=1420\n").unwrap();

        let before = snapshot_project(dir.path()).unwrap();
        assert!(before.findings.iter().any(|finding| {
            finding.action_kind == "replace-placeholder"
                && finding.key.as_deref() == Some("API_TOKEN")
        }));

        let after = save_env_value(
            dir.path().to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
            "API_TOKEN".to_string(),
            1,
            "sk-updated".to_string(),
        )
        .unwrap();
        let refreshed_env = after.files.iter().find(|file| file.name == ".env").unwrap();
        let refreshed_entry = refreshed_env
            .entries
            .iter()
            .find(|entry| entry.key == "API_TOKEN")
            .unwrap();

        assert_eq!(refreshed_entry.value, "");
        assert_eq!(refreshed_entry.display_value, "********");
        assert!(!refreshed_env.content.contains("sk-updated"));
        assert!(!after.findings.iter().any(|finding| {
            finding.action_kind == "replace-placeholder"
                && finding.key.as_deref() == Some("API_TOKEN")
        }));

        let revealed = reveal_env_value(
            dir.path().to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
            "API_TOKEN".to_string(),
            1,
        )
        .unwrap();
        assert_eq!(revealed, "sk-updated");
    }

    #[test]
    fn add_key_command_returns_rescanned_snapshot() {
        let dir = tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "PORT=1420\n").unwrap();
        fs::write(dir.path().join(".env.example"), "PORT=1420\nREDIS_URL=\n").unwrap();

        let before = snapshot_project(dir.path()).unwrap();
        assert!(before.findings.iter().any(|finding| {
            finding.action_kind == "add-missing-key" && finding.key.as_deref() == Some("REDIS_URL")
        }));

        let after = add_env_key(
            dir.path().to_string_lossy().to_string(),
            env_path.to_string_lossy().to_string(),
            "REDIS_URL".to_string(),
            "redis://localhost:6379".to_string(),
        )
        .unwrap();
        let refreshed_env = after.files.iter().find(|file| file.name == ".env").unwrap();

        assert!(refreshed_env
            .entries
            .iter()
            .any(|entry| entry.key == "REDIS_URL"));
        let refreshed_redis = refreshed_env
            .entries
            .iter()
            .find(|entry| entry.key == "REDIS_URL")
            .unwrap();
        assert_eq!(refreshed_redis.value, "");
        assert_eq!(refreshed_redis.display_value, "********");
        assert!(!after.findings.iter().any(|finding| {
            finding.action_kind == "add-missing-key" && finding.key.as_deref() == Some("REDIS_URL")
        }));
    }

    #[test]
    fn reports_layer_overrides_without_exposing_values() {
        let base = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://base\nJWT_SECRET=base-secret\n".to_string(),
        );
        let local = parse_env_file(
            Path::new(".env.local"),
            "DATABASE_URL=postgres://local\nJWT_SECRET=local-secret\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nJWT_SECRET=<secret>\n".to_string(),
        );

        let report = build_layer_report(&[base, local, example]);

        assert_eq!(report.ordered_files.len(), 2);
        assert!(report.placeholder_keys.is_empty());
        let database = report
            .overrides
            .iter()
            .find(|override_row| override_row.key == "DATABASE_URL")
            .unwrap();
        assert_eq!(database.effective_file, ".env.local");
        assert!(database.conflict);
        assert!(database.redacted);
        assert!(database
            .summary
            .contains("runtime precedence needs framework evidence"));
        assert!(!database.summary.contains("postgres://"));

        let secret = report
            .overrides
            .iter()
            .find(|override_row| override_row.key == "JWT_SECRET")
            .unwrap();
        assert!(secret.redacted);
        assert!(!secret.summary.contains("local-secret"));
    }

    #[test]
    fn builds_findings_without_exposing_values() {
        let base = parse_env_file(
            Path::new(".env"),
            "DATABASE_URL=postgres://base\nJWT_SECRET=base-secret\nEXTRA=yes\nEXTRA=no\nAPI_TOKEN=replace-me\n".to_string(),
        );
        let local = parse_env_file(
            Path::new(".env.local"),
            "DATABASE_URL=postgres://local\nJWT_SECRET=local-secret\n".to_string(),
        );
        let example = parse_env_file(
            Path::new(".env.example"),
            "DATABASE_URL=\nJWT_SECRET=\nREDIS_URL=\n".to_string(),
        );
        let files = vec![base, local, example];
        let comparison = compare_actual_to_example(&files);
        let layer_report = build_layer_report(&files);
        let framework_profiles = vec![FrameworkEnvProfile {
            framework: "Next.js".to_string(),
            mode: "development".to_string(),
            evidence: Vec::new(),
            ordered_files: vec![
                FrameworkEnvFile {
                    path: ".env.local".to_string(),
                    name: ".env.local".to_string(),
                    layer_kind: "local".to_string(),
                    rank: 2,
                },
                FrameworkEnvFile {
                    path: ".env".to_string(),
                    name: ".env".to_string(),
                    layer_kind: "base".to_string(),
                    rank: 4,
                },
            ],
            missing_files: Vec::new(),
            notes: Vec::new(),
        }];

        let findings = build_findings(
            comparison.as_ref(),
            &layer_report,
            &framework_profiles,
            &files,
        );

        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "add-missing-key"
                && finding.key.as_deref() == Some("REDIS_URL")));
        let missing_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "add-missing-key"
                    && finding.key.as_deref() == Some("REDIS_URL")
            })
            .unwrap();
        assert_eq!(
            missing_finding.mutation_preview.as_deref(),
            Some("Append `REDIS_URL=<value>` to .env.")
        );
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "resolve-duplicate-key"
                && finding.key.as_deref() == Some("EXTRA")));
        let duplicate_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "resolve-duplicate-key"
                    && finding.key.as_deref() == Some("EXTRA")
            })
            .unwrap();
        assert_eq!(duplicate_finding.file_path.as_deref(), Some(".env"));
        assert_eq!(duplicate_finding.line_number, Some(4));
        assert_eq!(duplicate_finding.entry_id.as_deref(), Some("EXTRA@4"));
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "replace-placeholder"
                && finding.key.as_deref() == Some("API_TOKEN")));
        let placeholder_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "replace-placeholder"
                    && finding.key.as_deref() == Some("API_TOKEN")
            })
            .unwrap();
        assert_eq!(placeholder_finding.file_path.as_deref(), Some(".env"));
        assert_eq!(placeholder_finding.line_number, Some(5));
        assert_eq!(placeholder_finding.entry_id.as_deref(), Some("API_TOKEN@5"));
        assert!(findings
            .iter()
            .any(|finding| finding.action_kind == "review-layer-conflict"
                && finding.key.as_deref() == Some("JWT_SECRET")));
        let layer_finding = findings
            .iter()
            .find(|finding| {
                finding.action_kind == "review-layer-conflict"
                    && finding.key.as_deref() == Some("JWT_SECRET")
            })
            .unwrap();
        assert!(layer_finding
            .evidence
            .iter()
            .any(|evidence| evidence.contains("Next.js development load order")));
        assert!(layer_finding.detail.contains("attached framework evidence"));
        assert!(!layer_finding.detail.contains("currently wins"));

        let rendered = findings
            .iter()
            .map(|finding| {
                format!(
                    "{} {} {}",
                    finding.title,
                    finding.detail,
                    finding.mutation_preview.as_deref().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("base-secret"));
        assert!(!rendered.contains("local-secret"));
        assert!(!rendered.contains("postgres://"));
    }

    #[test]
    fn reports_next_framework_env_load_order() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"next":"14.2.0"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join(".env"), "DATABASE_URL=postgres://base\n").unwrap();
        fs::write(
            dir.path().join(".env.local"),
            "DATABASE_URL=postgres://local\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development"),
            "DATABASE_URL=postgres://development\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development.local"),
            "DATABASE_URL=postgres://development-local\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.test"),
            "DATABASE_URL=postgres://test\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let development = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Next.js" && profile.mode == "development")
            .unwrap();
        let names = development
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ".env.development.local",
                ".env.local",
                ".env.development",
                ".env"
            ]
        );
        assert!(development
            .notes
            .iter()
            .any(|note| note.contains("process.env")));

        let test_profile = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Next.js" && profile.mode == "test")
            .unwrap();
        let test_names = test_profile
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(test_names, vec![".env.test", ".env"]);
        assert!(!test_profile
            .missing_files
            .iter()
            .any(|name| name == ".env.local"));
    }

    #[test]
    fn reports_vite_framework_env_load_order_and_script_modes() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"staging":"vite build --mode staging"},"devDependencies":{"vite":"8.0.0"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join(".env"), "VITE_APP_NAME=base\n").unwrap();
        fs::write(dir.path().join(".env.local"), "VITE_APP_NAME=local\n").unwrap();
        fs::write(
            dir.path().join(".env.development"),
            "VITE_APP_NAME=development\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".env.development.local"),
            "VITE_APP_NAME=development-local\n",
        )
        .unwrap();
        fs::write(dir.path().join(".env.staging"), "VITE_APP_NAME=staging\n").unwrap();
        fs::write(
            dir.path().join(".env.staging.local"),
            "VITE_APP_NAME=staging-local\n",
        )
        .unwrap();

        let snapshot = snapshot_project(dir.path()).unwrap();
        let development = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Vite" && profile.mode == "development")
            .unwrap();
        let names = development
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                ".env.development.local",
                ".env.development",
                ".env.local",
                ".env"
            ]
        );

        let staging = snapshot
            .framework_profiles
            .iter()
            .find(|profile| profile.framework == "Vite" && profile.mode == "staging")
            .unwrap();
        let staging_names = staging
            .ordered_files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            staging_names,
            vec![".env.staging.local", ".env.staging", ".env.local", ".env"]
        );
    }
}
