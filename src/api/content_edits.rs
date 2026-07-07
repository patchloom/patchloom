//! Multi-op in-memory content edits for agent hosts (#1459 Feature 2).
//!
//! Compose sequential text operations on one buffer with all-or-nothing
//! semantics, then optionally write once via [`apply_content_edits_to_file`].

use std::path::Path;

use anyhow::Context;

use crate::containment::PathGuard;
use crate::ops::file::{append_content, prepend_content};

use super::{
    ApplyMode, ContentEditResult, EditResult, ReplaceOptions, make_diff, replace_in_content,
    write_if_apply,
};
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

    for (i, edit) in edits.iter().enumerate() {
        current = apply_one(&current, edit)
            .with_context(|| format!("content edit {} of {} failed", i + 1, edits.len()))?;
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
        g.check_path(&path.to_string_lossy())
            .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {e}"))?;
    }
    let path_str = path.to_string_lossy().into_owned();
    let original =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {path_str}"))?;
    let batch = apply_content_edits(&original, edits)?;
    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &batch.modified, mode, &policy, guard)?;
    Ok(EditResult {
        path: path_str,
        original_content: batch.original,
        new_content: batch.modified,
        diff: batch.diff,
        applied,
        changed: batch.changed,
        action: "content.edits",
        dest_path: None,
        match_count: 0,
        removed: 0,
    })
}

fn apply_one(content: &str, edit: &ContentEdit) -> anyhow::Result<String> {
    match edit {
        ContentEdit::Replace { old, new, options } => {
            let result: ContentEditResult = replace_in_content(content, old, new, options)?;
            Ok(result.new_content)
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
                    Ok(out)
                }
                None => anyhow::bail!("insert_before anchor not found: {anchor:?}"),
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
                    Ok(out)
                }
                None => anyhow::bail!("insert_after anchor not found: {anchor:?}"),
            }
        }
        ContentEdit::Append { content: inject } => Ok(append_content(content, inject)),
        ContentEdit::Prepend { content: inject } => Ok(prepend_content(content, inject)),
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
        assert_eq!(r.modified, "hi\n\nworld");
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
    fn multi_op_prepend() {
        let edits = [ContentEdit::Prepend {
            content: "pre\n".into(),
        }];
        let r = apply_content_edits("body\n", &edits).unwrap();
        assert_eq!(r.modified, "pre\nbody\n");
        assert_eq!(r.ops_applied, 1);
    }
}
