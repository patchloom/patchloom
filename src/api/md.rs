//! Markdown section-aware operations for the public library API.

use std::path::Path;

use anyhow::Context;

use crate::containment::PathGuard;
use crate::ops;
use crate::write::{WritePolicy, atomic_write};

use super::{
    ApplyMode, EditResult, apply_cross_file_mutation, build_edit_result, ensure_contained,
    write_if_apply,
};

/// Replace the body of a markdown section identified by heading.
pub fn md_replace_section(
    path: &Path,
    heading: &str,
    content: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::replace_section_in(&original, heading, content)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "md.replace_section",
        None,
    ))
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
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::upsert_bullet_in(&original, heading, bullet)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "md.upsert_bullet",
        None,
    ))
}

/// Append a row to a markdown table under a heading.
pub fn md_table_append(
    path: &Path,
    heading: &str,
    row: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::table_append_for_tx(&original, heading, row)
        .ok_or_else(|| anyhow::anyhow!("no table found under heading '{}'", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "md.table_append",
        None,
    ))
}

/// Insert content after a markdown heading.
pub fn md_insert_after_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_after_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "md.insert_after_heading",
        None,
    ))
}

/// Move a markdown section to a position relative to another heading.
///
/// For same-file moves, pass `to` as `None`. For cross-file moves, pass the
/// destination file path in `to`.
///
/// The returned `EditResult` always describes the source `path`, with `dest_path`
/// set for cross-file moves. When a cross-file move succeeds under Apply, the
/// destination is mutated as a side effect (the result reports the dest via
/// `dest_path`; re-read for its content if needed). See #795 for richer batch
/// result plans.
pub fn md_move_section(
    path: &Path,
    heading: &str,
    position: (&str, &str),
    to: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
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

    let policy = WritePolicy::default();
    // Write the destination file for cross-file moves.
    if let Some(dest_path) = to {
        if mode == ApplyMode::Apply {
            ensure_contained(guard, dest_path)?;
        }
        let _ = write_if_apply(dest_path, &new_dest, mode, &policy, guard)?;
    }
    // Use generalized cross helper for source (centralized per #839).
    let applied = apply_cross_file_mutation(
        path,
        None,
        mode,
        guard,
        |backup| backup.save_before_write(path),
        || atomic_write(path, &new_source, &policy),
    )?;
    let dest = to.map(|p| p.to_string_lossy().to_string());
    Ok(build_edit_result(
        &path_str, original, new_source, applied, "md.move", dest,
    ))
}

/// Remove duplicate headings at the same level in a markdown file.
///
/// Returns the `EditResult` and a list of removed duplicate heading texts.
pub fn md_dedupe_headings(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<(EditResult, Vec<String>)> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let (new_content, removed) = ops::md::dedupe_headings_in(&original);

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok((
        build_edit_result(&path_str, original, new_content, applied, "md.dedupe", None),
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
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_before_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "md.insert_before_heading",
        None,
    ))
}
