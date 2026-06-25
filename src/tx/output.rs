use crate::exit;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Structured report from `execute_plan` (and the `tx` command).
///
/// Library users can deserialize the JSON string returned by `execute_plan`
/// into this type for typed access instead of string parsing.
/// See #805 and the embedding docs.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxOutput {
    pub ok: bool,
    pub status: String,
    pub files_changed: usize,
    pub files_created: usize,
    pub files_deleted: usize,
    pub changes: Vec<TxChange>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reads: Vec<TxReadResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub searches: Vec<TxSearchResult>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub lints: Vec<TxLintResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_session: Option<String>,
}

/// A single file change in a plan/tx report.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxChange {
    pub path: String,
    pub action: String,
}

/// A search match in the tx output.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxSearchMatch {
    pub line: usize,
    pub column: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
}

/// A search result in the tx output.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxSearchResult {
    pub path: String,
    pub pattern: String,
    pub match_count: usize,
    pub matches: Vec<TxSearchMatch>,
}

/// A file read result in the tx output.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxReadResult {
    pub path: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
}

/// A lint result in the tx output.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxLintResult {
    pub path: String,
    pub issue_count: usize,
    pub issues: Vec<crate::ops::md::LintIssue>,
}

/// Intermediate result from executing all operations in a plan and applying
/// write policy. Contains everything needed for callers to decide on output
/// mode, commit changes, and run lifecycle steps.
pub(crate) struct TxExecResult {
    pub(crate) changes: Vec<(PathBuf, String, String)>,
    pub(crate) deletions: HashSet<PathBuf>,
    pub(crate) existed_before: HashSet<PathBuf>,
    /// Original pending map, retained for `rollback_strict`.
    pub(crate) pending: HashMap<PathBuf, (String, String)>,
    pub(crate) tx_reads: Vec<TxReadResult>,
    pub(crate) tx_searches: Vec<TxSearchResult>,
    pub(crate) tx_lints: Vec<TxLintResult>,
    pub(crate) no_effective_changes: bool,
    pub(crate) replace_no_matches: bool,
    /// "Did you mean?" hints when a replace found zero matches.
    pub(crate) replace_hint: Option<String>,
}

pub(crate) fn build_tx_output(
    status: &'static str,
    ok: bool,
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> TxOutput {
    let mut tx_changes = Vec::new();
    let mut created = 0usize;
    let mut deleted_count = 0usize;
    let mut modified = 0usize;

    let display_path = |p: &Path| -> String {
        crate::files::relative_display(p, cwd)
            .to_string_lossy()
            .into_owned()
    };

    for (path, _original, _) in changes {
        let path_str = display_path(path);
        if deletions.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "deleted".to_string(),
            });
            deleted_count += 1;
        } else if !existed_before.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "created".to_string(),
            });
            created += 1;
        } else {
            tx_changes.push(TxChange {
                path: path_str,
                action: "modified".to_string(),
            });
            modified += 1;
        }
    }
    // Deletions not captured in changes (empty files).
    for path in deletions {
        if !changes.iter().any(|(c, _, _)| c == path) {
            tx_changes.push(TxChange {
                path: display_path(path),
                action: "deleted".to_string(),
            });
            deleted_count += 1;
        }
    }

    TxOutput {
        ok,
        status: status.to_string(),
        files_changed: modified,
        files_created: created,
        files_deleted: deleted_count,
        changes: tx_changes,
        reads: Vec::new(),
        searches: Vec::new(),
        lints: Vec::new(),
        error_kind: None,
        error: None,
        backup_session: None,
    }
}

pub(crate) fn build_full_tx_output(
    status: &'static str,
    result: &mut TxExecResult,
    cwd: &Path,
) -> TxOutput {
    let mut output = build_tx_output(
        status,
        true,
        &result.changes,
        &result.deletions,
        &result.existed_before,
        cwd,
    );
    output.reads = std::mem::take(&mut result.tx_reads);
    output.searches = std::mem::take(&mut result.tx_searches);
    output.lints = std::mem::take(&mut result.tx_lints);
    output
}

pub(crate) fn describe_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".to_string(),
    }
}

pub(crate) fn describe_lifecycle_cwd(base_cwd: &Path, cwd: &Path) -> String {
    if cwd == base_cwd {
        ".".to_string()
    } else {
        crate::files::relative_display(cwd, base_cwd)
            .display()
            .to_string()
    }
}

pub(crate) fn format_error_with_backup_hint(error: &str, backup_session: Option<&str>) -> String {
    match backup_session {
        Some(ts) => format!("{error} (backup session {ts}; run `patchloom undo` to restore)"),
        None => error.to_string(),
    }
}

pub(crate) fn build_error_output(
    error_kind: &'static str,
    legacy_error_prefix: &str,
    error: &str,
    backup_session: Option<&str>,
) -> TxOutput {
    TxOutput {
        ok: false,
        status: "error".to_string(),
        files_changed: 0,
        files_created: 0,
        files_deleted: 0,
        changes: Vec::new(),
        reads: Vec::new(),
        searches: Vec::new(),
        lints: Vec::new(),
        error_kind: Some(error_kind.to_string()),
        error: Some(format!(
            "{legacy_error_prefix}: {}",
            format_error_with_backup_hint(error, backup_session)
        )),
        backup_session: backup_session.map(str::to_string),
    }
}

/// Map a `TxOutput` (PlanReport) to the traditional exit code for CLI/MCP compat.
pub fn exit_code_from_tx_output(report: &TxOutput) -> u8 {
    if report.ok {
        if report.status == "no_matches" {
            exit::NO_MATCHES
        } else {
            exit::SUCCESS
        }
    } else {
        match report.error_kind.as_deref() {
            Some("no_matches") => exit::NO_MATCHES,
            Some("parse_error") => exit::PARSE_ERROR,
            Some("rollback") => exit::ROLLBACK,
            Some("rollback_failed") => exit::FAILURE, // preserve historical CLI exit for this case
            Some("validation_failed") | Some("format_failed") => exit::VALIDATION_FAILED,
            Some("operation_failed") => exit::OPERATION_FAILED,
            _ => exit::FAILURE,
        }
    }
}
