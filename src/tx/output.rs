//! size-waiver: accepted single-domain bulk (policy #1408). Tx JSON output
//! assembly and match honesty aggregation for plan/CLI/MCP is one unit; do not
//! split for LOC alone.

use crate::exit;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Structured report from `execute_plan` (and the `tx` command).
///
/// Library users can deserialize the JSON string returned by `execute_plan`
/// into this type for typed access instead of string parsing.
/// See #805 and the embedding docs.
///
/// Marked `non_exhaustive` so new honesty fields can land in minor releases
/// without breaking external struct literals. Serde deserialization is
/// unaffected (`#[serde(default)]` on optional fields).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[non_exhaustive]
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub changed: Option<bool>,
    /// Sum of [`TxDocMutation::removed`] when `mutations` is non-empty.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub removed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub backup_session: Option<String>,
    /// Aggregate replace match honesty when every replace-backed change agrees
    /// (or worst-case: fuzzy > anchored > exact). See #1674.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_mode: Option<String>,
    /// Similarity score when aggregate [`Self::match_mode`] is fuzzy.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_score: Option<f64>,
    /// Sum of per-path replace match counts when any replace meta was recorded.
    /// Lets MCP/CLI agents read honesty without a second content pass.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_count: Option<usize>,
    /// First fuzzy/anchored matched span when a single change path is present.
    /// Compare to requested `old` after fuzzy apply (#1736).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matched_text: Option<String>,
    /// Soft-refuse paths where fuzzy found a candidate but did not write
    /// (exact old absent without `allow_absent_old`). Present on partial
    /// multi-op success so agents do not treat overall ok as full coverage
    /// (parity with CLI `replace` `refused[]`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub refused: Vec<TxRefused>,
}

/// One soft-refuse path in a plan/tx report (fuzzy fail-closed without a write).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TxRefused {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub match_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub matched_text: Option<String>,
    /// Machine-readable reason (`exact_old_absent` or `no_write`).
    pub reason: String,
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
///
/// Marked `non_exhaustive` so new honesty fields can land in minor releases
/// without breaking external struct literals.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[non_exhaustive]
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
    /// Explicit `file.rename` pairs `(from, to)` for hardlink-preserving
    /// commit via `fs::rename` (including rename-then-edit in one plan).
    pub(crate) renames: Vec<(PathBuf, PathBuf)>,
}

/// Per-path replace match honesty recorded during plan execution.
#[derive(Debug, Clone)]
pub(crate) struct ReplaceMatchMeta {
    pub mode: crate::api::MatchMode,
    pub score: Option<f64>,
    pub match_count: usize,
    /// Fuzzy/anchored span text actually matched (may differ from plan `old`). #1736
    pub matched_text: Option<String>,
    /// Soft-no-write reason when `match_count` is 0 (`exact_old_absent`,
    /// `below_min_fuzzy_score`). Used for CLI/tx `refused[]` honesty.
    pub refuse_reason: Option<&'static str>,
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
            if matches!(m.mode, crate::api::MatchMode::Fuzzy)
                && let Some(s) = m.score
            {
                // Worst-case confidence: keep the lowest fuzzy score across paths.
                agg_score = Some(agg_score.map_or(s, |prev| prev.min(s)));
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
            if let Some(m) = replace_match_meta.get(path) {
                any_replace_meta = true;
                agg_mode = Some(merge_match_modes(agg_mode, m.mode));
                if matches!(m.mode, crate::api::MatchMode::Fuzzy)
                    && let Some(s) = m.score
                {
                    agg_score = Some(agg_score.map_or(s, |prev| prev.min(s)));
                }
                agg_count = agg_count.saturating_add(m.match_count);
                if top_matched_text.is_none() {
                    top_matched_text = m.matched_text.clone();
                }
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

    // Soft full refuses (fuzzy fail-closed #1758) store honesty without a write.
    // Fold only when there is no write surface: otherwise refuse meta would poison
    // success aggregates (e.g. exact multi-file apply + one soft refuse → match_mode
    // "fuzzy"). Partial refuses are listed in `refused[]` instead.
    if changes.is_empty() && deletions.is_empty() {
        for m in replace_match_meta.values() {
            any_replace_meta = true;
            agg_mode = Some(merge_match_modes(agg_mode, m.mode));
            if matches!(m.mode, crate::api::MatchMode::Fuzzy)
                && let Some(s) = m.score
            {
                agg_score = Some(agg_score.map_or(s, |prev| prev.min(s)));
            }
            agg_count = agg_count.saturating_add(m.match_count);
            if top_matched_text.is_none() {
                top_matched_text = m.matched_text.clone();
            }
        }
    }

    // Paths with recorded meta but no write (fuzzy refuse / floor skip).
    let mut refused = Vec::new();
    for (path, m) in replace_match_meta {
        if changes.iter().any(|(c, _, _)| c == path) || deletions.contains(path) {
            continue;
        }
        // Only surface candidates that were found but not applied (count 0).
        if m.match_count != 0 || m.matched_text.is_none() {
            continue;
        }
        let reason = m
            .refuse_reason
            .unwrap_or(if m.mode == crate::api::MatchMode::Fuzzy {
                "exact_old_absent"
            } else {
                "no_write"
            });
        refused.push(TxRefused {
            path: display_path(path),
            match_mode: Some(match_mode_label(m.mode).to_string()),
            match_score: m.score,
            matched_text: m.matched_text.clone(),
            reason: reason.to_string(),
        });
    }
    refused.sort_by(|a, b| a.path.cmp(&b.path));

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
        // rollups do not pick an arbitrary sibling span. Gate on replace meta
        // count (not write `changes.len()`), so a lone replace among other
        // ops or a deletion-only replace path still reports the span.
        matched_text: if replace_match_meta.len() == 1 {
            top_matched_text
        } else {
            None
        },
        refused,
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
    // Soft no-match replaces still set status=no_matches with ok=true (exit 3).
    // Surface error_kind + replace_hint so agents see floor / did-you-mean
    // diagnostics that path/glob arms already computed (#1753).
    if status == "no_matches" {
        output.error_kind = Some("no_matches".to_string());
        let detail = result
            .replace_hint
            .as_deref()
            .filter(|h| !h.is_empty())
            .unwrap_or("no matches");
        output.error = Some(detail.to_string());
    }
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
        refused: Vec::new(),
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
            refused: Vec::new(),
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
            refused: Vec::new(),
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
                refuse_reason: None,
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

    /// Multi-path fuzzy aggregate must report the minimum score (worst case).
    #[test]
    fn build_tx_output_fuzzy_agg_score_is_minimum() {
        let cwd = Path::new("/project");
        let a = PathBuf::from("/project/a.txt");
        let b = PathBuf::from("/project/b.txt");
        let existed = HashSet::from([a.clone(), b.clone()]);
        let changes = vec![
            (a.clone(), "old".into(), "new".into()),
            (b.clone(), "old".into(), "new".into()),
        ];
        let mut meta = HashMap::new();
        meta.insert(
            a,
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.95),
                match_count: 1,
                matched_text: Some("aaa".into()),
                refuse_reason: None,
            },
        );
        meta.insert(
            b,
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.80),
                match_count: 1,
                matched_text: Some("bbb".into()),
                refuse_reason: None,
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
        assert_eq!(
            out.match_score,
            Some(0.80),
            "worst-case aggregate score must be the min fuzzy score"
        );
        assert!(
            out.matched_text.is_none(),
            "multi-replace paths must not surface a single arbitrary matched_text"
        );
        assert_eq!(out.match_count, Some(2));
    }

    /// Top-level matched_text when only one replace path exists, even if other
    /// non-replace changes are present (gate on replace meta, not changes.len).
    #[test]
    fn build_tx_output_matched_text_when_single_replace_among_other_changes() {
        let cwd = Path::new("/project");
        let replaced = PathBuf::from("/project/a.txt");
        let other = PathBuf::from("/project/b.txt");
        let existed = HashSet::from([replaced.clone(), other.clone()]);
        let changes = vec![
            (replaced.clone(), "old".into(), "new".into()),
            (other, "x".into(), "y".into()),
        ];
        let mut meta = HashMap::new();
        meta.insert(
            replaced,
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.88),
                match_count: 1,
                matched_text: Some("live_span".into()),
                refuse_reason: None,
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
        assert_eq!(
            out.matched_text.as_deref(),
            Some("live_span"),
            "single replace path must surface matched_text even when other files changed"
        );
        assert_eq!(out.match_score, Some(0.88));
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

    /// Soft no_matches reports must carry error_kind + replace_hint for agents.
    #[test]
    fn build_full_tx_output_no_matches_includes_hint() {
        use std::collections::HashMap;
        let cwd = Path::new("/project");
        let mut result = TxExecResult {
            changes: vec![],
            deletions: HashSet::new(),
            existed_before: HashSet::new(),
            pending: HashMap::new(),
            tx_reads: vec![],
            tx_searches: vec![],
            tx_lints: vec![],
            tx_mutations: vec![],
            no_effective_changes: true,
            replace_no_matches: true,
            replace_hint: Some(
                "fuzzy match score 0.900 below min_fuzzy_score 1 for \"proccess\"".into(),
            ),
            replace_match_meta: HashMap::new(),
            renames: vec![],
        };
        let out = build_full_tx_output("no_matches", &mut result, cwd);
        assert_eq!(out.status, "no_matches");
        assert_eq!(out.error_kind.as_deref(), Some("no_matches"));
        assert!(
            out.error
                .as_deref()
                .is_some_and(|e| e.contains("min_fuzzy_score")),
            "hint must appear in error: {:?}",
            out.error
        );
    }

    /// Soft refuse (#1758) records replace_match_meta without a write; no_matches
    /// JSON must still expose match_mode / match_score / matched_text.
    #[test]
    fn build_full_tx_output_no_matches_includes_refuse_match_meta() {
        use std::collections::HashMap;
        let cwd = Path::new("/project");
        let mut meta = HashMap::new();
        meta.insert(
            PathBuf::from("/project/app.py"),
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.987),
                match_count: 0,
                matched_text: Some("compute_checksum".into()),
                refuse_reason: None,
            },
        );
        let mut result = TxExecResult {
            changes: vec![],
            deletions: HashSet::new(),
            existed_before: HashSet::new(),
            pending: HashMap::new(),
            tx_reads: vec![],
            tx_searches: vec![],
            tx_lints: vec![],
            tx_mutations: vec![],
            no_effective_changes: true,
            replace_no_matches: true,
            replace_hint: Some("exact old absent; best fuzzy candidate".into()),
            replace_match_meta: meta,
            renames: vec![],
        };
        let out = build_full_tx_output("no_matches", &mut result, cwd);
        assert_eq!(out.status, "no_matches");
        assert_eq!(out.match_mode.as_deref(), Some("fuzzy"));
        assert_eq!(out.match_score, Some(0.987));
        assert_eq!(out.matched_text.as_deref(), Some("compute_checksum"));
        assert_eq!(out.match_count, Some(0));
    }

    /// Partial apply + soft refuse must not report aggregate match_mode=fuzzy.
    #[test]
    fn build_full_tx_output_partial_success_ignores_refuse_meta_in_aggregate() {
        use std::collections::HashMap;
        let cwd = Path::new("/project");
        let changed = PathBuf::from("/project/a.txt");
        let refused = PathBuf::from("/project/b.txt");
        let mut meta = HashMap::new();
        meta.insert(
            changed.clone(),
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Exact,
                score: None,
                match_count: 1,
                matched_text: None,
                refuse_reason: None,
            },
        );
        meta.insert(
            refused,
            ReplaceMatchMeta {
                mode: crate::api::MatchMode::Fuzzy,
                score: Some(0.97),
                match_count: 0,
                matched_text: Some("helo world".into()),
                refuse_reason: Some("exact_old_absent"),
            },
        );
        let mut result = TxExecResult {
            changes: vec![(changed, "hello world\n".into(), "hi\n".into())],
            deletions: HashSet::new(),
            existed_before: {
                let mut s = HashSet::new();
                s.insert(PathBuf::from("/project/a.txt"));
                s
            },
            pending: HashMap::new(),
            tx_reads: vec![],
            tx_searches: vec![],
            tx_lints: vec![],
            tx_mutations: vec![],
            no_effective_changes: false,
            replace_no_matches: false,
            replace_hint: Some("exact old absent".into()),
            replace_match_meta: meta,
            renames: vec![],
        };
        let out = build_full_tx_output("success", &mut result, cwd);
        assert_eq!(out.status, "success");
        assert_eq!(out.match_mode.as_deref(), Some("exact"));
        assert!(out.match_score.is_none());
        assert_eq!(out.match_count, Some(1));
        assert_eq!(out.refused.len(), 1);
        assert_eq!(out.refused[0].path, "b.txt");
        assert_eq!(out.refused[0].match_mode.as_deref(), Some("fuzzy"));
        assert_eq!(out.refused[0].reason, "exact_old_absent");
        assert_eq!(out.refused[0].matched_text.as_deref(), Some("helo world"));
    }

    /// Hosts may deserialize older plan/tx JSON that never had match honesty
    /// fields. Missing keys must default to None (parity with TxChange).
    #[test]
    fn tx_output_deserializes_minimal_json_without_match_fields() {
        let json = r#"{"ok":true,"status":"success","files_changed":0,"files_created":0,"files_deleted":0,"changes":[]}"#;
        let parsed: TxOutput = serde_json::from_str(json).expect("minimal TxOutput JSON");
        assert!(parsed.ok);
        assert!(parsed.match_mode.is_none());
        assert!(parsed.match_score.is_none());
        assert!(parsed.match_count.is_none());
        assert!(parsed.matched_text.is_none());
        assert!(parsed.backup_session.is_none());
        assert!(parsed.error_kind.is_none());
    }
}
