//! Multi-op in-memory content edits for agent hosts (#1459 Feature 2).
//!
//! Compose sequential text operations on one buffer with all-or-nothing
//! semantics, then optionally write once via [`apply_content_edits_to_file`].
//! Results include rolled-up [`ContentEditsResult::match_count`] from replace
//! ops (and the file helper surfaces the same on [`EditResult::match_count`]).

use anyhow::Context;

use crate::ops::file::{append_content, prepend_content};

use super::{ContentEditResult, MatchMode, ReplaceOptions, make_diff, replace_in_content};
use crate::fallback::{EditError, EditErrorKind};

#[cfg(any(feature = "cli", feature = "files"))]
use std::path::Path;

#[cfg(any(feature = "cli", feature = "files"))]
use crate::containment::PathGuard;

#[cfg(any(feature = "cli", feature = "files"))]
use super::{
    ApplyMode, EditResult, PostWriteHooks, build_edit_result, maybe_post_write, write_if_apply,
};

#[cfg(any(feature = "cli", feature = "files"))]
use crate::write::WritePolicy;

/// A single ordered edit on an in-memory buffer.
#[derive(Debug, Clone)]
// ReplaceOptions carries post_write hooks (#1690); boxing would break callers.
#[allow(clippy::large_enum_variant)]
pub enum ContentEdit {
    /// Text replacement (reuses [`ReplaceOptions`]).
    Replace {
        old: String,
        new: String,
        options: ReplaceOptions,
    },
    /// Insert `content` immediately before the first exact match of `anchor`.
    InsertBefore { anchor: String, content: String },
    /// Insert `content` immediately after the first exact match of `anchor`.
    InsertAfter { anchor: String, content: String },
    /// Append content to the end of the buffer.
    Append { content: String },
    /// Prepend content to the start of the buffer.
    Prepend { content: String },
}

/// Per-op match honesty for a successful [`ContentEdit::Replace`] (#2006).
///
/// Hosts should call [`super::fuzzy_span_suspicious`] with this entry's `old`,
/// `matched_text`, and `match_score` rather than rollup fields alone (rollup
/// widest span and min score may come from different ops).
///
/// Prefer building this via real [`apply_content_edits`] / replace for
/// integration tests. Downstream crates cannot use struct literals
/// (`#[non_exhaustive]`); use [`Self::exact`] / [`Self::fuzzy`] for host
/// refuse-glue unit tests (#2033).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ContentEditHonesty {
    /// Index into the `edits` slice (0-based).
    pub op_index: usize,
    /// Match strategy for this replace.
    pub match_mode: Option<MatchMode>,
    /// Fuzzy similarity when `match_mode` is fuzzy.
    pub match_score: Option<f64>,
    /// Live span that was replaced (fuzzy/anchored) when available.
    pub matched_text: Option<String>,
    /// The replace `old` string for this op (for refuse with matching old).
    pub old: String,
}

impl ContentEditHonesty {
    /// Construct a minimal exact-match honesty row for host unit tests (#2033).
    ///
    /// Future fields get internal defaults; prefer live edits for integration
    /// coverage.
    #[must_use]
    pub fn exact(op_index: usize, old: impl Into<String>, matched_text: impl Into<String>) -> Self {
        Self {
            op_index,
            match_mode: Some(MatchMode::Exact),
            match_score: None,
            matched_text: Some(matched_text.into()),
            old: old.into(),
        }
    }

    /// Construct a minimal fuzzy-match honesty row for host unit tests (#2033).
    #[must_use]
    pub fn fuzzy(
        op_index: usize,
        old: impl Into<String>,
        score: f64,
        matched_text: impl Into<String>,
    ) -> Self {
        Self {
            op_index,
            match_mode: Some(MatchMode::Fuzzy),
            match_score: Some(score),
            matched_text: Some(matched_text.into()),
            old: old.into(),
        }
    }
}

/// Result of applying a sequence of [`ContentEdit`]s to a buffer.
///
/// Marked `non_exhaustive` so new honesty fields can land in minor releases
/// without breaking external struct literals.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ContentEditsResult {
    /// Original buffer before any edits.
    pub original: String,
    /// Buffer after all edits (or unchanged when `changed` is false).
    pub modified: String,
    /// Unified diff from original to modified.
    pub diff: String,
    /// Whether any edit changed the buffer.
    pub changed: bool,
    /// Number of ops that successfully applied (equals `edits.len()` on success).
    pub ops_applied: usize,
    /// Sum of replace match counts across all [`ContentEdit::Replace`] ops.
    /// Insert/append/prepend contribute 0. Useful for agent ambiguity policies.
    pub match_count: usize,
    /// Worst-case match strategy across replace ops (`Fuzzy` > `Anchored` > `Exact`).
    /// `None` when no replace ops ran or none matched.
    pub match_mode: Option<MatchMode>,
    /// Lowest fuzzy similarity score across replace ops in the batch (worst-case
    /// confidence when several fuzzy ops ran).
    pub match_score: Option<f64>,
    /// Matched span for host policy: **widest** fuzzy/anchored span in the batch
    /// by Unicode char count (worst-case over-wide for refuse; #1736 / #1981).
    /// Not the first non-empty only (that hid later over-wide fuzzy ops).
    ///
    /// **Pairing note:** `match_score` is the **minimum** fuzzy score across ops;
    /// `matched_text` is the **widest** span. They may come from different ops.
    /// Prefer [`Self::op_honesty`] with the corresponding `old` when possible; do not
    /// treat rolled-up fields as a single joint (old, span, score) triple.
    pub matched_text: Option<String>,
    /// Per successful replace-like op, in apply order (#2006). Empty when no
    /// replace ops reported match honesty.
    pub op_honesty: Vec<ContentEditHonesty>,
}

/// Apply ordered edits to an in-memory buffer.
///
/// **All-or-nothing:** if any op fails, returns `Err` and does not expose a
/// partial buffer. On success, every op has been applied in order; each op
/// sees the result of the previous one.
///
/// Diff headers use `<buffer>`. Prefer [`apply_content_edits_with_label`] when
/// the host knows a real path (#1665).
///
/// Available with default features off as long as the replace path is
/// compiled (always; no `ast` required).
pub fn apply_content_edits(
    content: &str,
    edits: &[ContentEdit],
) -> anyhow::Result<ContentEditsResult> {
    apply_content_edits_with_label(content, edits, None)
}

/// Like [`apply_content_edits`], but labels unified-diff headers with
/// `path_label` when set (e.g. `src/lib.rs`). `None` keeps `<buffer>` (#1665).
pub fn apply_content_edits_with_label(
    content: &str,
    edits: &[ContentEdit],
    path_label: Option<&str>,
) -> anyhow::Result<ContentEditsResult> {
    let original = content.to_string();
    let mut current = content.to_string();
    let mut ops_applied = 0usize;
    let mut match_count = 0usize;
    let mut match_mode: Option<MatchMode> = None;
    let mut match_score: Option<f64> = None;
    let mut matched_text: Option<String> = None;
    let mut op_honesty: Vec<ContentEditHonesty> = Vec::new();

    for (i, edit) in edits.iter().enumerate() {
        let one = apply_one(&current, edit)
            .with_context(|| format!("content edit {} of {} failed", i + 1, edits.len()))?;
        current = one.content;
        match_count = match_count.saturating_add(one.match_count);
        ops_applied += 1;
        if let ContentEdit::Replace { old, .. } = edit
            && (one.match_mode.is_some() || one.matched_text.is_some())
        {
            op_honesty.push(ContentEditHonesty {
                op_index: i,
                match_mode: one.match_mode,
                match_score: one.match_score,
                matched_text: one.matched_text.clone(),
                old: old.clone(),
            });
        }
        if let Some(m) = one.match_mode {
            match_mode = Some(super::merge_match_modes(match_mode, m));
            // Worst-case confidence: lowest fuzzy score across ops.
            if m == MatchMode::Fuzzy
                && let Some(s) = one.match_score
            {
                match_score = Some(match_score.map_or(s, |prev| prev.min(s)));
            }
            // Widest span wins so multi-op hosts can refuse over-wide fuzzy
            // even when an earlier Exact/small Fuzzy left a short matched_text (#1981).
            matched_text = super::prefer_widest_matched_text(matched_text, one.matched_text);
        }
    }

    let changed = current != original;
    let label = path_label.unwrap_or("<buffer>");
    let diff = make_diff(label, &original, &current);
    Ok(ContentEditsResult {
        original,
        modified: current,
        diff,
        changed,
        ops_applied,
        match_count,
        match_mode,
        match_score,
        matched_text,
        op_honesty,
    })
}

/// Read a file, apply multi-op content edits, and write according to `mode`.
///
/// Requires `files` or `cli` for PathGuard / write policy integration.
///
/// For over-wide fuzzy refuse before any disk write, use
/// [`apply_content_edits_to_file_with_span_policy`] (#2008). When each replace
/// uses [`super::ReplaceOptions::for_agent`], refuse already runs in memory
/// via `refuse_suspicious_fuzzy` (#2005).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn apply_content_edits_to_file(
    path: &Path,
    edits: &[ContentEdit],
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    apply_content_edits_to_file_with_span_policy(path, edits, mode, guard, None)
}

/// Like [`apply_content_edits_to_file`], with optional pre-write fuzzy span
/// refuse (#2008).
///
/// When `fuzzy_span_policy` is `Some`, after a successful in-memory batch and
/// **before** `write_if_apply`, each fuzzy replace in `op_honesty` is checked
/// with [`super::fuzzy_span_suspicious_with_policy`]. On refuse:
/// [`EditErrorKind::FuzzySpanSuspicious`], no disk write, no backup session.
///
/// `None` keeps historical behavior (no extra gate). Prefer
/// [`super::FuzzySpanPolicy::default`] for the same thresholds as
/// [`super::ReplaceOptions::for_agent`].
#[cfg(any(feature = "cli", feature = "files"))]
pub fn apply_content_edits_to_file_with_span_policy(
    path: &Path,
    edits: &[ContentEdit],
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    fuzzy_span_policy: Option<&super::FuzzySpanPolicy>,
) -> anyhow::Result<EditResult> {
    if let Some(g) = guard {
        g.check_path(&path.to_string_lossy())
            .map_err(EditError::guard_rejected)?;
    }
    let path_str = path.to_string_lossy().into_owned();
    let original = crate::files::load_text_strict(path, &path_str).map_err(|e| {
        // Preserve content SoftSkip peels (#1963); do not collapse to OperationFailed.
        if crate::exit::is_load_text_strict_fail(&e) {
            return e;
        }
        EditError::new(EditErrorKind::OperationFailed, e.to_string()).into()
    })?;
    // Prefer real path labels in multi-op preview (#1665 / #1500).
    let batch = apply_content_edits_with_label(&original, edits, Some(path_str.as_str()))?;
    if let Some(policy) = fuzzy_span_policy {
        refuse_batch_if_suspicious_fuzzy(&batch, policy)?;
    }
    let policy = WritePolicy::default();
    let (applied, backup_session) = write_if_apply(path, &batch.modified, mode, &policy, guard)?;
    // build_edit_result uses path for headers (#1500) and stays field-complete.
    let mut result = build_edit_result(
        &path_str,
        batch.original,
        batch.modified,
        applied,
        "content.edits",
        None,
    );
    result.backup_session = backup_session.clone();
    result.match_count = batch.match_count;
    result.match_mode = batch.match_mode;
    result.match_score = batch.match_score;
    result.matched_text = batch.matched_text;
    // Honor post_write from the last Replace edit that set hooks (#1690).
    let (hooks, hooks_cwd) = post_write_from_edits(edits);
    maybe_post_write(applied, path, hooks, hooks_cwd, backup_session.as_deref())?;
    Ok(result)
}

/// Refuse before write when any fuzzy op honesty is over-wide (#2008).
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn refuse_batch_if_suspicious_fuzzy(
    batch: &ContentEditsResult,
    policy: &super::FuzzySpanPolicy,
) -> anyhow::Result<()> {
    use super::{MatchMode, fuzzy_span_suspicious_with_policy};
    if !batch.changed {
        return Ok(());
    }
    for h in &batch.op_honesty {
        if h.match_mode != Some(MatchMode::Fuzzy) {
            continue;
        }
        if fuzzy_span_suspicious_with_policy(
            &h.old,
            h.matched_text.as_deref(),
            h.match_score,
            policy,
        ) {
            let matched = h.matched_text.as_deref().unwrap_or("");
            let score = h
                .match_score
                .map(|s| format!("{s:.3}"))
                .unwrap_or_else(|| "none".into());
            return Err(EditError::new(
                EditErrorKind::FuzzySpanSuspicious,
                format!(
                    "fuzzy span suspicious on content edit {}: old {:?} matched {:?} (score {score}); span policy refuse before write",
                    h.op_index + 1,
                    crate::fallback::truncate_str(&h.old, 60),
                    crate::fallback::truncate_str(matched, 80),
                ),
            )
            .with_suggestion(
                "tighten old, omit span policy, or use refuse_suspicious_fuzzy on each Replace",
            )
            .into());
        }
    }
    Ok(())
}

/// Last replace-side post_write wins (agent hosts set hooks once on the batch).
#[cfg(any(feature = "cli", feature = "files"))]
fn post_write_from_edits(
    edits: &[ContentEdit],
) -> (Option<&PostWriteHooks>, Option<&std::path::Path>) {
    let mut hooks = None;
    let mut cwd = None;
    for e in edits {
        if let ContentEdit::Replace { options, .. } = e
            && options.post_write.is_some()
        {
            hooks = options.post_write.as_ref();
            cwd = options.post_write_cwd.as_deref();
        }
    }
    (hooks, cwd)
}

/// Intermediate apply_one result (keeps the match-honesty fields together).
struct OneEdit {
    content: String,
    match_count: usize,
    match_mode: Option<MatchMode>,
    match_score: Option<f64>,
    matched_text: Option<String>,
}

/// Apply a single content edit; returns the new buffer plus replace honesty.
fn apply_one(content: &str, edit: &ContentEdit) -> anyhow::Result<OneEdit> {
    match edit {
        ContentEdit::Replace { old, new, options } => {
            let result: ContentEditResult = replace_in_content(content, old, new, options)?;
            Ok(OneEdit {
                content: result.new_content,
                match_count: result.match_count,
                match_mode: result.match_mode,
                match_score: result.match_score,
                matched_text: result.matched_text,
            })
        }
        ContentEdit::InsertBefore {
            anchor,
            content: insert,
        } => {
            if anchor.is_empty() {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "insert_before anchor must not be empty".into(),
                }));
            }
            // First-match only (same as String::find). Callers that need
            // unique anchors should use Replace with unique:true or a longer
            // anchor span.
            match content.find(anchor.as_str()) {
                Some(idx) => {
                    let insert = crate::ops::replace::normalize_line_insert(
                        content,
                        anchor,
                        insert,
                        crate::ops::replace::InsertSide::Before,
                    );
                    let mut out = String::with_capacity(content.len() + insert.len());
                    out.push_str(&content[..idx]);
                    out.push_str(&insert);
                    out.push_str(&content[idx..]);
                    Ok(OneEdit {
                        content: out,
                        match_count: 0,
                        match_mode: None,
                        match_score: None,
                        matched_text: None,
                    })
                }
                None => Err(EditError::new(
                    EditErrorKind::NoMatch,
                    format!("insert_before anchor not found: {anchor:?}"),
                )
                .into()),
            }
        }
        ContentEdit::InsertAfter {
            anchor,
            content: insert,
        } => {
            if anchor.is_empty() {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "insert_after anchor must not be empty".into(),
                }));
            }
            // First-match only (see InsertBefore).
            match content.find(anchor.as_str()) {
                Some(idx) => {
                    let insert = crate::ops::replace::normalize_line_insert(
                        content,
                        anchor,
                        insert,
                        crate::ops::replace::InsertSide::After,
                    );
                    let end = idx + anchor.len();
                    let mut out = String::with_capacity(content.len() + insert.len());
                    out.push_str(&content[..end]);
                    out.push_str(&insert);
                    out.push_str(&content[end..]);
                    Ok(OneEdit {
                        content: out,
                        match_count: 0,
                        match_mode: None,
                        match_score: None,
                        matched_text: None,
                    })
                }
                None => Err(EditError::new(
                    EditErrorKind::NoMatch,
                    format!("insert_after anchor not found: {anchor:?}"),
                )
                .into()),
            }
        }
        ContentEdit::Append { content: inject } => Ok(OneEdit {
            content: append_content(content, inject),
            match_count: 0,
            match_mode: None,
            match_score: None,
            matched_text: None,
        }),
        ContentEdit::Prepend { content: inject } => Ok(OneEdit {
            content: prepend_content(content, inject),
            match_count: 0,
            match_mode: None,
            match_score: None,
            matched_text: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_op_replace_then_append() {
        let edits = [
            ContentEdit::Replace {
                old: "hello".into(),
                new: "hi".into(),
                options: ReplaceOptions::default(),
            },
            ContentEdit::Append {
                content: "\nworld".into(),
            },
        ];
        let r = apply_content_edits("hello\n", &edits).unwrap();
        assert!(r.changed);
        assert_eq!(r.ops_applied, 2);
        assert_eq!(r.match_count, 1, "replace match_count should roll up");
        assert_eq!(r.modified, "hi\n\nworld");
        // Pure buffer helper keeps the `<buffer>` path label (#1500).
        assert!(
            r.diff.contains("--- a/<buffer>") && r.diff.contains("+++ b/<buffer>"),
            "buffer helper must label headers as <buffer>, got:\n{}",
            r.diff
        );
        assert!(
            !r.diff.contains("a//") && !r.diff.contains("b//"),
            "buffer headers must not double-slash: {}",
            r.diff
        );
    }

    #[test]
    fn multi_op_all_or_nothing_on_failure() {
        let edits = [
            ContentEdit::Replace {
                old: "a".into(),
                new: "A".into(),
                options: ReplaceOptions::default(),
            },
            ContentEdit::InsertBefore {
                anchor: "missing".into(),
                content: "x".into(),
            },
        ];
        let err = apply_content_edits("a b\n", &edits).unwrap_err();
        assert!(
            err.to_string().contains("content edit 2"),
            "error should identify failing op: {err}"
        );
    }

    #[test]
    fn multi_op_unique_failure() {
        let edits = [ContentEdit::Replace {
            old: "x".into(),
            new: "y".into(),
            options: ReplaceOptions {
                unique: true,
                ..Default::default()
            },
        }];
        let err = apply_content_edits("x x\n", &edits).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("ambiguous") || msg.contains("matches"),
            "expected unique/ambiguous failure, got: {msg}"
        );
    }

    #[test]
    fn multi_op_insert_before_after() {
        // Mid-line bare inserts stay byte-exact (no leading indent / newline).
        let edits = [
            ContentEdit::InsertBefore {
                anchor: "B".into(),
                content: "A".into(),
            },
            ContentEdit::InsertAfter {
                anchor: "B".into(),
                content: "C".into(),
            },
        ];
        let r = apply_content_edits("xBy", &edits).unwrap();
        assert_eq!(r.modified, "xABCy");
    }

    #[test]
    fn multi_op_insert_line_oriented_on_whole_line_anchor() {
        // Whole-line anchor "B": bare payloads get newlines (#1885).
        let edits = [
            ContentEdit::InsertBefore {
                anchor: "B".into(),
                content: "A".into(),
            },
            ContentEdit::InsertAfter {
                anchor: "B".into(),
                content: "C".into(),
            },
        ];
        let r = apply_content_edits("B", &edits).unwrap();
        // Before: "A\n" + "B" → "A\nB"; After: "B" + "\nC" on that buffer → "A\nB\nC"
        assert_eq!(r.modified, "A\nB\nC");
    }

    #[test]
    fn empty_edits_is_noop() {
        let r = apply_content_edits("same\n", &[]).unwrap();
        assert!(!r.changed);
        assert_eq!(r.ops_applied, 0);
        assert_eq!(r.match_count, 0);
        assert_eq!(r.modified, "same\n");
    }

    #[test]
    fn match_count_sums_multiple_replaces() {
        let edits = [
            ContentEdit::Replace {
                old: "a".into(),
                new: "A".into(),
                options: ReplaceOptions::default(),
            },
            ContentEdit::Replace {
                old: "b".into(),
                new: "B".into(),
                options: ReplaceOptions::default(),
            },
        ];
        let r = apply_content_edits("a a b\n", &edits).unwrap();
        assert_eq!(r.match_count, 3, "two a's + one b: {r:?}");
        assert_eq!(r.modified, "A A B\n");
    }

    #[test]
    fn multi_op_prepend() {
        let edits = [ContentEdit::Prepend {
            content: "pre\n".into(),
        }];
        let r = apply_content_edits("body\n", &edits).unwrap();
        assert_eq!(r.modified, "pre\nbody\n");
        assert_eq!(r.ops_applied, 1);
    }

    #[test]
    fn empty_anchor_is_rejected() {
        let before = apply_content_edits(
            "body\n",
            &[ContentEdit::InsertBefore {
                anchor: String::new(),
                content: "x".into(),
            }],
        )
        .unwrap_err();
        let before_msg = format!("{before:#}");
        assert!(
            before_msg.contains("empty"),
            "insert_before empty anchor: {before_msg}"
        );
        assert_eq!(
            crate::fallback::edit_error_kind(&before),
            Some(EditErrorKind::InvalidInput)
        );
        let after = apply_content_edits(
            "body\n",
            &[ContentEdit::InsertAfter {
                anchor: String::new(),
                content: "x".into(),
            }],
        )
        .unwrap_err();
        let after_msg = format!("{after:#}");
        assert!(
            after_msg.contains("empty"),
            "insert_after empty anchor: {after_msg}"
        );
        assert_eq!(
            crate::fallback::edit_error_kind(&after),
            Some(EditErrorKind::InvalidInput)
        );
    }

    #[test]
    fn require_change_true_missing_is_no_match() {
        let edits = [ContentEdit::Replace {
            old: "missing".into(),
            new: "x".into(),
            options: ReplaceOptions {
                require_change: true,
                ..Default::default()
            },
        }];
        let err = apply_content_edits("hello\n", &edits).unwrap_err();
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(EditErrorKind::NoMatch)
        );
    }

    #[test]
    fn require_change_false_missing_is_ok_unchanged() {
        let edits = [ContentEdit::Replace {
            old: "missing".into(),
            new: "x".into(),
            options: ReplaceOptions::default(),
        }];
        let r = apply_content_edits("hello\n", &edits).unwrap();
        assert!(!r.changed);
        assert_eq!(r.modified, "hello\n");
    }

    #[test]
    fn require_change_unique_multi_is_ambiguous() {
        let edits = [ContentEdit::Replace {
            old: "x".into(),
            new: "y".into(),
            options: ReplaceOptions {
                unique: true,
                require_change: true,
                ..Default::default()
            },
        }];
        let err = apply_content_edits("x x\n", &edits).unwrap_err();
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(EditErrorKind::AmbiguousTarget)
        );
    }

    #[test]
    fn require_change_batch_fails_whole_batch() {
        let edits = [
            ContentEdit::Replace {
                old: "a".into(),
                new: "A".into(),
                options: ReplaceOptions {
                    require_change: true,
                    ..Default::default()
                },
            },
            ContentEdit::Replace {
                old: "missing".into(),
                new: "z".into(),
                options: ReplaceOptions {
                    require_change: true,
                    ..Default::default()
                },
            },
        ];
        let err = apply_content_edits("a b\n", &edits).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("content edit 2"), "got: {msg}");
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(EditErrorKind::NoMatch)
        );
    }
    #[test]
    fn command_position_in_batch_match_count() {
        let edits = [ContentEdit::Replace {
            old: "pip".into(),
            new: "uv".into(),
            options: ReplaceOptions {
                command_position: true,
                require_change: true,
                ..Default::default()
            },
        }];
        let r = apply_content_edits("timeout 5 pip install\nuv pip install\n", &edits).unwrap();
        assert_eq!(r.match_count, 1);
        assert_eq!(r.modified, "timeout 5 uv install\nuv pip install\n");
    }

    #[test]
    fn insert_before_uses_first_anchor_only() {
        let edits = [ContentEdit::InsertBefore {
            anchor: "x".into(),
            content: "IN:".into(),
        }];
        let r = apply_content_edits("a x b x c\n", &edits).unwrap();
        assert_eq!(r.modified, "a IN:x b x c\n");
    }

    #[test]
    fn content_edit_fuzzy_match_mode_rollup() {
        let edits = [ContentEdit::Replace {
            old: "fn proccess_data() {}".into(),
            new: "fn handle_data() {}".into(),
            options: ReplaceOptions {
                fuzzy: true,
                min_fuzzy_score: None,
                allow_absent_old: true,
                require_change: true,
                ..Default::default()
            },
        }];
        let r = apply_content_edits("fn process_data() {}\n", &edits).unwrap();
        assert!(r.changed);
        assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
        assert!(r.match_score.is_some_and(|s| s > 0.85));
        let matched = r
            .matched_text
            .as_deref()
            .expect("content_edits fuzzy must surface matched_text");
        assert!(
            matched.contains("process_data"),
            "matched_text should be live span: {matched:?}"
        );
    }

    #[test]
    fn content_edit_match_mode_rollup_anchored_over_exact() {
        // Exact first, then context-anchored: worst-case is Anchored (#1673).
        let edits = [
            ContentEdit::Replace {
                old: "alpha".into(),
                new: "ALPHA".into(),
                options: ReplaceOptions::default(),
            },
            ContentEdit::Replace {
                old: "TODO: fix".into(),
                new: "TODO: done".into(),
                options: ReplaceOptions {
                    before_context: Some("gamma".into()),
                    ..Default::default()
                },
            },
        ];
        let r = apply_content_edits("alpha\nTODO: fix\nbeta\ngamma\nTODO: fix\n", &edits).unwrap();
        assert!(r.changed);
        assert_eq!(r.match_mode, Some(MatchMode::Anchored));
        assert_eq!(r.match_count, 2);
        // Exact first leaves matched_text None; anchored span is the only honesty text.
        assert_eq!(
            r.matched_text.as_deref(),
            Some("TODO: fix"),
            "batch should report widest anchored/fuzzy matched_text (#1736 / #1981)"
        );
        assert!(r.modified.starts_with("ALPHA\n"));
        assert!(r.modified.contains("gamma\nTODO: done"));
        // Context-anchored replace must not touch the first TODO: fix
        assert!(
            r.modified.contains("ALPHA\nTODO: fix\nbeta"),
            "first TODO must remain: {}",
            r.modified
        );
    }

    #[test]
    fn merge_match_modes_precedence_table() {
        use super::super::merge_match_modes;
        use MatchMode::*;
        assert_eq!(merge_match_modes(None, Exact), Exact);
        assert_eq!(merge_match_modes(Some(Exact), Anchored), Anchored);
        assert_eq!(merge_match_modes(Some(Anchored), Exact), Anchored);
        assert_eq!(merge_match_modes(Some(Exact), Fuzzy), Fuzzy);
        assert_eq!(merge_match_modes(Some(Anchored), Fuzzy), Fuzzy);
        assert_eq!(merge_match_modes(Some(Fuzzy), Anchored), Fuzzy);
        assert_eq!(merge_match_modes(Some(Fuzzy), Exact), Fuzzy);
    }

    /// #1981: multi-op keeps the **widest** matched_text, not the first.
    #[test]
    fn content_edits_matched_text_prefers_widest_span() {
        // First fuzzy: short identifier. Second: much wider live span (simulated
        // by two separate replaces on different content). After first apply the
        // buffer changes; second uses a different old that still fuzzy-matches.
        let edits = [
            ContentEdit::Replace {
                old: "fn small_typo_name() {}".into(),
                new: "fn small_name() {}".into(),
                options: ReplaceOptions {
                    fuzzy: true,
                    allow_absent_old: true,
                    min_fuzzy_score: None,
                    require_change: true,
                    ..Default::default()
                },
            },
            ContentEdit::Replace {
                // Typo of a whole-line-ish target so fuzzy can land a wide span.
                old: "let CONFIGURATION_VALUE_PRIMRY = 1;".into(),
                new: "let CONFIGURATION_VALUE_SECONDARY = 1;".into(),
                options: ReplaceOptions {
                    fuzzy: true,
                    allow_absent_old: true,
                    min_fuzzy_score: None,
                    require_change: true,
                    ..Default::default()
                },
            },
        ];
        let src = "fn small_name() {}\nlet CONFIGURATION_VALUE_PRIMARY = 1;\n";
        let r = apply_content_edits(src, &edits).unwrap();
        assert!(r.changed, "edits should apply: {:?}", r.diff);
        let matched = r
            .matched_text
            .as_deref()
            .expect("batch must surface matched_text");
        // Widest honesty span should reflect the longer identifier line, not
        // only the first short function name match.
        assert!(
            matched.contains("CONFIGURATION_VALUE_PRIMARY"),
            "widest matched_text should come from the longer second-op span, got {matched:?}"
        );
        assert!(
            matched.chars().count() > "small_name".chars().count(),
            "widest span must exceed first-op identifier length, got {matched:?}"
        );

        // Reverse order: wide first, short second still keeps the widest.
        let edits_rev = [
            ContentEdit::Replace {
                old: "let CONFIGURATION_VALUE_PRIMRY = 1;".into(),
                new: "let CONFIGURATION_VALUE_SECONDARY = 1;".into(),
                options: ReplaceOptions {
                    fuzzy: true,
                    allow_absent_old: true,
                    min_fuzzy_score: None,
                    require_change: true,
                    ..Default::default()
                },
            },
            ContentEdit::Replace {
                old: "fn small_typo_name() {}".into(),
                new: "fn small_name() {}".into(),
                options: ReplaceOptions {
                    fuzzy: true,
                    allow_absent_old: true,
                    min_fuzzy_score: None,
                    require_change: true,
                    ..Default::default()
                },
            },
        ];
        let r2 = apply_content_edits(src, &edits_rev).unwrap();
        assert!(r2.changed);
        let matched2 = r2
            .matched_text
            .as_deref()
            .expect("reverse-order batch must surface matched_text");
        assert!(
            matched2.contains("CONFIGURATION_VALUE_PRIMARY"),
            "widest must win when it is the first op, got {matched2:?}"
        );
    }
}
