//! Markdown section-aware operations for the public library API.
//!
//! Standard write functions delegate to the tx engine via `execute_as_edit_result`.
//! Special cases (md_move_section cross-file, md_dedupe_headings with extra return
//! data) keep direct implementations with feature-gated tx fallbacks.

use std::path::Path;

use anyhow::Context;

use crate::containment::PathGuard;
use crate::ops;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Derive cwd from a file path (its parent directory).
#[cfg(any(feature = "cli", feature = "files"))]
fn cwd_from_path(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

/// Unified write path for standard md operations.
#[cfg(any(feature = "cli", feature = "files"))]
fn md_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    super::execute_as_edit_result(op, mode, cwd_from_path(path), guard, action)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn md_write(
    _op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    // Fallback: execute the md operation through the tx engine's execute
    // path. Without the tx module, delegate to the ops layer directly.
    use crate::write::WritePolicy;

    // Re-extract the operation fields to call ops directly.
    // This is only used when building without cli/files features.
    let path_str = path.to_string_lossy();
    let original =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {path_str}"))?;

    let new_content = match _op {
        Operation::MdReplaceSection {
            heading, content, ..
        } => ops::md::replace_section_in(&original, &heading, &content),
        Operation::MdInsertAfterHeading {
            heading, content, ..
        } => ops::md::insert_after_heading_in(&original, &heading, &content),
        Operation::MdInsertBeforeHeading {
            heading, content, ..
        } => ops::md::insert_before_heading_in(&original, &heading, &content),
        Operation::MdUpsertBullet {
            heading, bullet, ..
        } => ops::md::upsert_bullet_in(&original, &heading, &bullet),
        Operation::MdTableAppend { heading, row, .. } => {
            let (body_start, body_end) = ops::md::find_section(&original, &heading)
                .ok_or_else(|| anyhow::anyhow!("heading not found in {path_str}"))?;
            return match ops::md::table_append_in(&original, body_start, body_end, &row) {
                Ok(new_content) => {
                    let policy = WritePolicy::default();
                    let applied = super::write_if_apply(path, &new_content, mode, &policy, guard)?;
                    Ok(super::build_edit_result(
                        &path_str,
                        original,
                        new_content,
                        applied,
                        action,
                        None,
                    ))
                }
                Err(e) => Err(anyhow::anyhow!(
                    "{e} under heading {heading:?} in {path_str}"
                )),
            };
        }
        _ => None,
    }
    .ok_or_else(|| anyhow::anyhow!("heading not found in {path_str}"))?;

    let policy = WritePolicy::default();
    let applied = super::write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(super::build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        action,
        None,
    ))
}

/// Replace the body of a markdown section identified by heading.
pub fn md_replace_section(
    path: &Path,
    heading: &str,
    content: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::MdReplaceSection {
        path: path.to_string_lossy().into(),
        heading: heading.into(),
        content: content.into(),
    };
    md_write(op, path, mode, guard, "md.replace_section")
}

/// Insert or update a bullet point under a markdown heading.
///
/// If the bullet already exists (exact match), the file is unchanged.
pub fn md_upsert_bullet(
    path: &Path,
    heading: &str,
    bullet: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::MdUpsertBullet {
        path: path.to_string_lossy().into(),
        heading: heading.into(),
        bullet: bullet.into(),
    };
    md_write(op, path, mode, guard, "md.upsert_bullet")
}

/// Append a row to a markdown table under a heading.
pub fn md_table_append(
    path: &Path,
    heading: &str,
    row: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::MdTableAppend {
        path: path.to_string_lossy().into(),
        heading: heading.into(),
        row: row.into(),
    };
    md_write(op, path, mode, guard, "md.table_append")
}

/// Insert content after a markdown heading.
pub fn md_insert_after_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::MdInsertAfterHeading {
        path: path.to_string_lossy().into(),
        heading: heading.into(),
        content: insertion.into(),
    };
    md_write(op, path, mode, guard, "md.insert_after_heading")
}

/// Move a markdown section to a position relative to another heading.
///
/// For same-file moves, pass `to` as `None`. For cross-file moves, pass the
/// destination file path in `to`.
///
/// Cross-file moves are complex (two files, guard on dest) and retain the
/// direct implementation. Same-file moves route through the tx engine when
/// available.
pub fn md_move_section(
    path: &Path,
    heading: &str,
    position: (&str, &str),
    to: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    // Cross-file moves retain the direct implementation because the tx engine
    // only handles single-file operations. Same-file moves can route through
    // the engine but cross-file needs coordinated writes to two files.
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let dest_content = match to {
        Some(dest_path) => std::fs::read_to_string(dest_path)
            .with_context(|| format!("failed to read {}", dest_path.display()))?,
        None => original.clone(),
    };

    let (new_source, new_dest) =
        ops::md::move_section_in(&original, heading, &dest_content, position, to.is_none())
            .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = crate::write::WritePolicy::default();
    // Write the destination file for cross-file moves.
    if let Some(dest_path) = to {
        if mode == ApplyMode::Apply {
            super::ensure_contained(guard, dest_path)?;
        }
        let _ = super::write_if_apply(dest_path, &new_dest, mode, &policy, guard)?;
    }
    // Use generalized cross helper for source (centralized per #839).
    let applied = super::apply_cross_file_mutation(
        path,
        None,
        mode,
        guard,
        |backup| backup.save_before_write(path),
        || crate::write::atomic_write(path, &new_source, &policy),
    )?;
    let dest = to.map(|p| p.to_string_lossy().to_string());
    Ok(super::build_edit_result(
        &path_str, original, new_source, applied, "md.move", dest,
    ))
}

/// Remove duplicate headings at the same level in a markdown file.
///
/// Returns the `EditResult` and a list of removed duplicate heading texts.
/// Retains direct implementation because the removed-headings list is not
/// available from the tx engine output.
pub fn md_dedupe_headings(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<(EditResult, Vec<String>)> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let (new_content, removed) = ops::md::dedupe_headings_in(&original);

    let policy = crate::write::WritePolicy::default();
    let applied = super::write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok((
        super::build_edit_result(&path_str, original, new_content, applied, "md.dedupe", None),
        removed,
    ))
}

/// A lint issue found in a markdown file.
///
/// Re-exported from `ops::md` so library consumers don't need to import
/// from the internal `cmd` module path.
pub use crate::ops::md::LintIssue;

/// Lint a markdown file for common agent-rules issues (duplicate headings,
/// missing sections, etc.).
///
/// Returns a list of lint issues found. This is a read-only operation.
pub fn md_lint_agents(path: &Path) -> anyhow::Result<Vec<LintIssue>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(crate::ops::md::lint_agents_content(&content))
}

/// Insert content before a markdown heading.
pub fn md_insert_before_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::MdInsertBeforeHeading {
        path: path.to_string_lossy().into(),
        heading: heading.into(),
        content: insertion.into(),
    };
    md_write(op, path, mode, guard, "md.insert_before_heading")
}
