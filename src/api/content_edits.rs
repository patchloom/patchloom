//! Multi-op in-memory content edits for agent hosts (#1459 Feature 2).
//!
//! Compose sequential text operations on one buffer with all-or-nothing
//! semantics, then optionally write once via [`apply_content_edits_to_file`].

use anyhow::Context;

use crate::ops::file::{append_content, prepend_content};

use super::{ContentEditResult, ReplaceOptions, make_diff, replace_in_content};
use crate::fallback::{EditError, EditErrorKind};

#[cfg(any(feature = "cli", feature = "files"))]
use std::path::Path;

#[cfg(any(feature = "cli", feature = "files"))]
use crate::containment::PathGuard;

#[cfg(any(feature = "cli", feature = "files"))]
use super::{ApplyMode, EditResult, build_edit_result, write_if_apply};

#[cfg(any(feature = "cli", feature = "files"))]
use crate::write::WritePolicy;

/// A single ordered edit on an in-memory buffer.
#[derive(Debug, Clone)]
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

/// Result of applying a sequence of [`ContentEdit`]s to a buffer.
#[derive(Debug, Clone)]
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
}

/// Apply ordered edits to an in-memory buffer.
///
/// **All-or-nothing:** if any op fails, returns `Err` and does not expose a
/// partial buffer. On success, every op has been applied in order; each op
/// sees the result of the previous one.
///
/// Available with default features off as long as the replace path is
/// compiled (always; no `ast` required).
pub fn apply_content_edits(
    content: &str,
    edits: &[ContentEdit],
) -> anyhow::Result<ContentEditsResult> {
    let original = content.to_string();
    let mut current = content.to_string();
    let mut ops_applied = 0usize;
    let mut match_count = 0usize;

    for (i, edit) in edits.iter().enumerate() {
        let (next, n) = apply_one(&current, edit)
            .with_context(|| format!("content edit {} of {} failed", i + 1, edits.len()))?;
        current = next;
        match_count = match_count.saturating_add(n);
        ops_applied += 1;
    }

    let changed = current != original;
    let diff = make_diff("<buffer>", &original, &current);
    Ok(ContentEditsResult {
        original,
        modified: current,
        diff,
        changed,
        ops_applied,
        match_count,
    })
}

/// Read a file, apply multi-op content edits, and write according to `mode`.
///
/// Requires `files` or `cli` for PathGuard / write policy integration.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn apply_content_edits_to_file(
    path: &Path,
    edits: &[ContentEdit],
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    if let Some(g) = guard {
        g.check_path(&path.to_string_lossy()).map_err(|e| {
            EditError::new(
                EditErrorKind::GuardRejected,
                format!("path rejected by workspace guard: {e}"),
            )
        })?;
    }
    let path_str = path.to_string_lossy().into_owned();
    let original =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {path_str}"))?;
    let batch = apply_content_edits(&original, edits)?;
    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &batch.modified, mode, &policy, guard)?;
    // build_edit_result uses path for headers (#1500) and stays field-complete.
    let mut result = build_edit_result(
        &path_str,
        batch.original,
        batch.modified,
        applied,
        "content.edits",
        None,
    );
    result.match_count = batch.match_count;
    Ok(result)
}

/// Returns (new content, replace match_count for this op).
fn apply_one(content: &str, edit: &ContentEdit) -> anyhow::Result<(String, usize)> {
    match edit {
        ContentEdit::Replace { old, new, options } => {
            let result: ContentEditResult = replace_in_content(content, old, new, options)?;
            Ok((result.new_content, result.match_count))
        }
        ContentEdit::InsertBefore {
            anchor,
            content: insert,
        } => {
            if anchor.is_empty() {
                anyhow::bail!("insert_before anchor must not be empty");
            }
            match content.find(anchor.as_str()) {
                Some(idx) => {
                    let mut out = String::with_capacity(content.len() + insert.len());
                    out.push_str(&content[..idx]);
                    out.push_str(insert);
                    out.push_str(&content[idx..]);
                    Ok((out, 0))
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
                anyhow::bail!("insert_after anchor must not be empty");
            }
            match content.find(anchor.as_str()) {
                Some(idx) => {
                    let end = idx + anchor.len();
                    let mut out = String::with_capacity(content.len() + insert.len());
                    out.push_str(&content[..end]);
                    out.push_str(insert);
                    out.push_str(&content[end..]);
                    Ok((out, 0))
                }
                None => Err(EditError::new(
                    EditErrorKind::NoMatch,
                    format!("insert_after anchor not found: {anchor:?}"),
                )
                .into()),
            }
        }
        ContentEdit::Append { content: inject } => Ok((append_content(content, inject), 0)),
        ContentEdit::Prepend { content: inject } => Ok((prepend_content(content, inject), 0)),
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
        assert_eq!(r.modified, "ABC");
    }

    #[test]
    fn empty_edits_is_noop() {
        let r = apply_content_edits("same\n", &[]).unwrap();
        assert!(!r.changed);
        assert_eq!(r.ops_applied, 0);
        assert_eq!(r.modified, "same\n");
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
}
