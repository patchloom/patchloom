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
    /// Aggregate replace match honesty when every replace-backed change agrees
    /// (or worst-case: fuzzy > anchored > exact). See #1674.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_mode: Option<String>,
    /// Similarity score when aggregate [`Self::match_mode`] is fuzzy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_score: Option<f64>,
    /// Sum of per-path replace match counts when any replace meta was recorded.
    /// Lets MCP/CLI agents read honesty without a second content pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_count: Option<usize>,
    /// First fuzzy/anchored matched span when a single change path is present.
    /// Compare to requested `old` after fuzzy apply (#1736).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
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
    /// Replace match honesty for this path (`exact` / `fuzzy` / `anchored`).
    /// Omitted for non-replace changes. See #1674 / #1669.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_mode: Option<String>,
    /// Similarity score when [`Self::match_mode`] is fuzzy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_score: Option<f64>,
    /// Number of replace matches for this path (from engine meta). Omitted for
    /// non-replace changes. Prefer this over re-deriving after Apply.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_count: Option<usize>,
    /// Text actually matched for fuzzy/anchored replace on this path (#1736).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matched_text: Option<String>,
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
    /// Per-path replace match honesty recorded during plan execution (#1674).
    pub(crate) replace_match_meta: HashMap<PathBuf, ReplaceMatchMeta>,
}

/// Per-path replace match honesty recorded during plan execution.
#[derive(Debug, Clone)]
pub(crate) struct ReplaceMatchMeta {
    pub mode: crate::api::MatchMode,
    pub score: Option<f64>,
    pub match_count: usize,
    /// Fuzzy/anchored span text actually matched (may differ from plan `old`). #1736
    pub matched_text: Option<String>,
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

/// JSON label for [`crate::api::MatchMode`] (CLI/MCP parity: snake_case strings).
pub(crate) fn match_mode_label(mode: crate::api::MatchMode) -> &'static str {
    match mode {
        crate::api::MatchMode::Exact => "exact",
        crate::api::MatchMode::Fuzzy => "fuzzy",
        crate::api::MatchMode::Anchored => "anchored",
    }
}

/// Worst-case rollup: fuzzy > anchored > exact (#1674 / #1673).
///
/// Thin re-export of [`crate::api::merge_match_modes`] so `tx::replace_op`
/// and callers keep a stable import path.
pub(crate) use crate::api::merge_match_modes;

fn match_meta_for_path(
    path: &Path,
    meta: &HashMap<PathBuf, ReplaceMatchMeta>,
) -> (Option<String>, Option<f64>, Option<usize>, Option<String>) {
    match meta.get(path) {
        Some(m) => (
            Some(match_mode_label(m.mode).to_string()),
            m.score,
            Some(m.match_count),
            m.matched_text.clone(),
        ),
        None => (None, None, None, None),
    }
}

pub(crate) fn build_tx_output_with_meta(
    status: &'static str,
    ok: bool,
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
    replace_match_meta: &HashMap<PathBuf, ReplaceMatchMeta>,
) -> TxOutput {
    let mut tx_changes = Vec::new();
    let mut created = 0usize;
    let mut deleted_count = 0usize;
    let mut modified = 0usize;
    let mut agg_mode: Option<crate::api::MatchMode> = None;
    let mut agg_score: Option<f64> = None;
    let mut agg_count: usize = 0;
    let mut any_replace_meta = false;
    let mut top_matched_text: Option<String> = None;

    let display_path = |p: &Path| -> String {
        crate::files::relative_display(p, cwd)
            .to_string_lossy()
            .into_owned()
    };

    for (path, _original, _) in changes {
        let path_str = display_path(path);
        let (match_mode, match_score, match_count, matched_text) =
            match_meta_for_path(path, replace_match_meta);
        if let Some(m) = replace_match_meta.get(path) {
            any_replace_meta = true;
            agg_mode = Some(merge_match_modes(agg_mode, m.mode));
            if matches!(m.mode, crate::api::MatchMode::Fuzzy) {
                agg_score = m.score.or(agg_score);
            }
            agg_count = agg_count.saturating_add(m.match_count);
            if top_matched_text.is_none() {
                top_matched_text = m.matched_text.clone();
            }
        }
        if deletions.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "deleted".to_string(),
                match_mode,
                match_score,
                match_count,
                matched_text,
            });
            deleted_count += 1;
        } else if !existed_before.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "created".to_string(),
                match_mode,
                match_score,
                match_count,
                matched_text,
            });
            created += 1;
        } else {
            tx_changes.push(TxChange {
                path: path_str,
                action: "modified".to_string(),
                match_mode,
                match_score,
                match_count,
                matched_text,
            });
            modified += 1;
        }
    }
    // Deletions not captured in changes (empty files).
    for path in deletions {
        if !changes.iter().any(|(c, _, _)| c == path) {
            let (match_mode, match_score, match_count, matched_text) =
                match_meta_for_path(path, replace_match_meta);
            if replace_match_meta.contains_key(path) {
                any_replace_meta = true;
            }
            tx_changes.push(TxChange {
                path: display_path(path),
                action: "deleted".to_string(),
                match_mode,
                match_score,
                match_count,
                matched_text,
            });
            deleted_count += 1;
        }
    }

    let (top_mode, top_score) = match agg_mode {
        Some(m) => (
            Some(match_mode_label(m).to_string()),
            if matches!(m, crate::api::MatchMode::Fuzzy) {
                agg_score
            } else {
                None
            },
        ),
        None => (None, None),
    };

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
        match_mode: top_mode,
        match_score: top_score,
        match_count: if any_replace_meta {
            Some(agg_count)
        } else {
            None
        },
        // Surface only when a single replace path is present so multi-file
        // rollups do not pick an arbitrary sibling span.
        matched_text: if changes.len() == 1 {
            top_matched_text
        } else {
            None
        },
    }
}

pub(crate) fn build_full_tx_output(
    status: &'static str,
    result: &mut TxExecResult,
    cwd: &Path,
) -> TxOutput {
    let mut output = build_tx_output_with_meta(
        status,
        true,
        &result.changes,
        &result.deletions,
        &result.existed_before,
        cwd,
        &result.replace_match_meta,
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
    error_kind: &str,
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
        match_mode: None,
        match_score: None,
        match_count: None,
        matched_text: None,
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
            Some("conflicts") => exit::CONFLICTS,
            Some("changes_detected") => exit::CHANGES_DETECTED,
            // parse_error already handled above; keep exhaustive for clarity
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
            match_mode: None,
            match_score: None,
            match_count: None,
            matched_text: None,
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
            match_mode: None,
            match_score: None,
            match_count: None,
            matched_text: None,
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

        let out = build_tx_output_with_meta(
            "success",
            true,
            &changes,
            &deletions,
            &existed,
            cwd,
            &HashMap::new(),
        );
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
        let out = build_tx_output_with_meta(
            "success",
            true,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            cwd,
            &HashMap::new(),
        );
        assert_eq!(out.files_changed, 0);
        assert_eq!(out.files_created, 0);
        assert_eq!(out.files_deleted, 0);
        assert!(out.changes.is_empty());
    }

    #[test]
    fn build_tx_output_includes_replace_match_mode() {
        let cwd = Path::new("/project");
        let path = PathBuf::from("/project/a.txt");
        let existed = HashSet::from([path.clone()]);
        let changes = vec![(path.clone(), "old".into(), "new".into())];
        let mut meta = HashMap::new();
        meta.insert(
            path,
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.91),
                match_count: 1,
                matched_text: Some("proccess".into()),
            },
        );
        let out = build_tx_output_with_meta(
            "success",
            true,
            &changes,
            &HashSet::new(),
            &existed,
            cwd,
            &meta,
        );
        assert_eq!(out.match_mode.as_deref(), Some("fuzzy"));
        assert_eq!(out.match_score, Some(0.91));
        assert_eq!(out.match_count, Some(1));
        assert_eq!(out.changes.len(), 1);
        assert_eq!(out.changes[0].match_mode.as_deref(), Some("fuzzy"));
        assert_eq!(out.changes[0].match_score, Some(0.91));
        assert_eq!(out.changes[0].match_count, Some(1));
        assert_eq!(out.matched_text.as_deref(), Some("proccess"));
        assert_eq!(out.changes[0].matched_text.as_deref(), Some("proccess"));
        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"match_mode\":\"fuzzy\""), "{json}");
        assert!(json.contains("\"match_count\":1"), "{json}");
        assert!(json.contains("\"matched_text\":\"proccess\""), "{json}");
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
