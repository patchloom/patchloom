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
    /// Per-op doc delete / delete-where summaries (#1439). Empty when the plan
    /// had no such ops. Prefer this for multi-op plans; top-level `changed` /
    /// `removed` are aggregates when present.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mutations: Vec<TxDocMutation>,
    /// Aggregate of [`TxDocMutation::changed`] when `mutations` is non-empty.
    /// Mirrors CLI doc write JSON so agents can treat exit 0 + `removed: 0`
    /// as an idempotent no-op without re-reading the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
    /// Sum of [`TxDocMutation::removed`] when `mutations` is non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_session: Option<String>,
}

/// One doc delete / delete-where outcome inside a plan/tx report (#1439).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxDocMutation {
    pub path: String,
    /// Plan op name, e.g. `doc.delete` or `doc.delete_where`.
    pub op: String,
    pub changed: bool,
    pub removed: usize,
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
    pub(crate) tx_mutations: Vec<TxDocMutation>,
    pub(crate) no_effective_changes: bool,
    pub(crate) replace_no_matches: bool,
    /// "Did you mean?" hints when a replace found zero matches.
    pub(crate) replace_hint: Option<String>,
}

/// Apply mutation summaries onto a [`TxOutput`] (aggregates + list).
pub(crate) fn attach_mutations(output: &mut TxOutput, mutations: Vec<TxDocMutation>) {
    if mutations.is_empty() {
        return;
    }
    let changed = mutations.iter().any(|m| m.changed);
    let removed = mutations.iter().map(|m| m.removed).sum();
    output.changed = Some(changed);
    output.removed = Some(removed);
    output.mutations = mutations;
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
        mutations: Vec::new(),
        changed: None,
        removed: None,
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
    attach_mutations(&mut output, std::mem::take(&mut result.tx_mutations));
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
        mutations: Vec::new(),
        changed: None,
        removed: None,
        error_kind: Some(error_kind.to_string()),
        error: Some(format!(
            "{error_kind}: {}",
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
            Some("ambiguous") => exit::AMBIGUOUS,
            Some("rollback") => exit::ROLLBACK,
            Some("rollback_failed") => exit::FAILURE,
            Some("validation_failed") | Some("format_failed") | Some("verification_failed") => {
                exit::VALIDATION_FAILED
            }
            Some("operation_failed") => exit::OPERATION_FAILED,
            _ => exit::FAILURE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ---- describe_exit_status ----

    /// Spawn a process that exits with the given code on both Unix and Windows.
    fn command_for_exit_code(code: i32) -> std::process::Command {
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("cmd");
            cmd.args(["/C", &format!("exit {code}")]);
            cmd
        }
        #[cfg(not(windows))]
        {
            if code == 0 {
                std::process::Command::new("true")
            } else {
                std::process::Command::new("false")
            }
        }
    }

    #[test]
    fn describe_exit_status_code_zero() {
        let status = command_for_exit_code(0).status().unwrap();
        assert_eq!(describe_exit_status(status), "exit code 0");
    }

    #[test]
    fn describe_exit_status_code_nonzero() {
        let status = command_for_exit_code(1).status().unwrap();
        assert_eq!(describe_exit_status(status), "exit code 1");
    }

    // ---- describe_lifecycle_cwd ----

    #[test]
    fn describe_lifecycle_cwd_same() {
        let cwd = Path::new("/tmp/project");
        assert_eq!(describe_lifecycle_cwd(cwd, cwd), ".");
    }

    #[test]
    fn describe_lifecycle_cwd_subdir() {
        let base = Path::new("/tmp/project");
        let sub = Path::new("/tmp/project/src/lib");
        assert_eq!(describe_lifecycle_cwd(base, sub), "src/lib");
    }

    // ---- format_error_with_backup_hint ----

    #[test]
    fn format_error_without_backup() {
        assert_eq!(format_error_with_backup_hint("oops", None), "oops");
    }

    #[test]
    fn format_error_with_backup() {
        let msg = format_error_with_backup_hint("oops", Some("20260101T120000"));
        assert!(msg.contains("backup session 20260101T120000"));
        assert!(msg.contains("patchloom undo"));
    }

    // ---- build_error_output ----

    #[test]
    fn build_error_output_fields() {
        let out = build_error_output("parse_error", "bad plan", None);
        assert!(!out.ok);
        assert_eq!(out.status, "error");
        assert_eq!(out.error_kind.as_deref(), Some("parse_error"));
        assert!(out.error.as_ref().unwrap().contains("bad plan"));
        assert_eq!(out.files_changed, 0);
        assert!(out.backup_session.is_none());
    }

    #[test]
    fn build_error_output_with_backup() {
        let out = build_error_output("rollback", "fail", Some("ts123"));
        assert_eq!(out.backup_session.as_deref(), Some("ts123"));
        assert!(out.error.as_ref().unwrap().contains("patchloom undo"));
    }

    // ---- exit_code_from_tx_output ----

    fn ok_output(status: &str) -> TxOutput {
        TxOutput {
            ok: true,
            status: status.to_string(),
            files_changed: 0,
            files_created: 0,
            files_deleted: 0,
            changes: Vec::new(),
            reads: Vec::new(),
            searches: Vec::new(),
            lints: Vec::new(),
            mutations: Vec::new(),
            changed: None,
            removed: None,
            error_kind: None,
            error: None,
            backup_session: None,
        }
    }

    fn err_output(kind: &str) -> TxOutput {
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
            mutations: Vec::new(),
            changed: None,
            removed: None,
            error_kind: Some(kind.to_string()),
            error: Some("test error".to_string()),
            backup_session: None,
        }
    }

    #[test]
    fn attach_mutations_sets_aggregates() {
        let mut out = ok_output("success");
        attach_mutations(
            &mut out,
            vec![
                TxDocMutation {
                    path: "a.json".into(),
                    op: "doc.delete_where".into(),
                    changed: true,
                    removed: 2,
                },
                TxDocMutation {
                    path: "b.json".into(),
                    op: "doc.delete".into(),
                    changed: false,
                    removed: 0,
                },
            ],
        );
        assert_eq!(out.changed, Some(true));
        assert_eq!(out.removed, Some(2));
        assert_eq!(out.mutations.len(), 2);
        assert_eq!(out.mutations[0].removed, 2);
    }

    #[test]
    fn attach_mutations_empty_is_noop() {
        let mut out = ok_output("success");
        attach_mutations(&mut out, Vec::new());
        assert_eq!(out.changed, None);
        assert_eq!(out.removed, None);
        assert!(out.mutations.is_empty());
    }

    #[test]
    fn exit_code_success() {
        assert_eq!(
            exit_code_from_tx_output(&ok_output("success")),
            exit::SUCCESS
        );
    }

    #[test]
    fn exit_code_ok_no_matches() {
        assert_eq!(
            exit_code_from_tx_output(&ok_output("no_matches")),
            exit::NO_MATCHES
        );
    }

    #[test]
    fn exit_code_error_kinds() {
        assert_eq!(
            exit_code_from_tx_output(&err_output("no_matches")),
            exit::NO_MATCHES
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("parse_error")),
            exit::PARSE_ERROR
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("rollback")),
            exit::ROLLBACK
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("rollback_failed")),
            exit::FAILURE
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("validation_failed")),
            exit::VALIDATION_FAILED
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("format_failed")),
            exit::VALIDATION_FAILED
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("verification_failed")),
            exit::VALIDATION_FAILED
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("operation_failed")),
            exit::OPERATION_FAILED
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("ambiguous")),
            exit::AMBIGUOUS
        );
        assert_eq!(
            exit_code_from_tx_output(&err_output("unknown_kind")),
            exit::FAILURE
        );
    }

    // ---- build_tx_output ----

    #[test]
    fn build_tx_output_classifies_changes() {
        let cwd = Path::new("/project");
        let existed = HashSet::from([PathBuf::from("/project/existing.txt")]);
        let deletions = HashSet::from([PathBuf::from("/project/removed.txt")]);
        let changes = vec![
            (
                PathBuf::from("/project/existing.txt"),
                "old".to_string(),
                "new".to_string(),
            ),
            (
                PathBuf::from("/project/brand_new.txt"),
                String::new(),
                "content".to_string(),
            ),
            (
                PathBuf::from("/project/removed.txt"),
                "was here".to_string(),
                String::new(),
            ),
        ];

        let out = build_tx_output("success", true, &changes, &deletions, &existed, cwd);
        assert!(out.ok);
        assert_eq!(out.files_changed, 1); // existing.txt modified
        assert_eq!(out.files_created, 1); // brand_new.txt
        assert_eq!(out.files_deleted, 1); // removed.txt
        assert_eq!(out.changes.len(), 3);

        let actions: Vec<&str> = out.changes.iter().map(|c| c.action.as_str()).collect();
        assert!(actions.contains(&"modified"));
        assert!(actions.contains(&"created"));
        assert!(actions.contains(&"deleted"));
    }

    #[test]
    fn build_tx_output_empty_changes() {
        let cwd = Path::new("/project");
        let out = build_tx_output("success", true, &[], &HashSet::new(), &HashSet::new(), cwd);
        assert_eq!(out.files_changed, 0);
        assert_eq!(out.files_created, 0);
        assert_eq!(out.files_deleted, 0);
        assert!(out.changes.is_empty());
    }

    // ---- TxOutput serde round-trip ----

    #[test]
    fn tx_output_serde_round_trip() {
        let out = ok_output("success");
        let json = serde_json::to_string(&out).unwrap();
        let parsed: TxOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.ok, out.ok);
        assert_eq!(parsed.status, out.status);
    }

    #[test]
    fn tx_output_skips_empty_optional_fields() {
        let out = ok_output("success");
        let json = serde_json::to_string(&out).unwrap();
        // Empty reads/searches/lints should be omitted
        assert!(!json.contains("\"reads\""));
        assert!(!json.contains("\"searches\""));
        assert!(!json.contains("\"lints\""));
        assert!(!json.contains("\"error\""));
    }
}
