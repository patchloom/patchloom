//! Patch (unified diff) apply operations for the library API.

use std::path::Path;

use anyhow::{Context, bail};

use crate::containment::PathGuard;
use crate::ops;
use crate::write::WritePolicy;

use super::{ApplyMode, EditResult, build_edit_result, write_if_apply};

/// Apply a unified diff patch to a file.
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(
    path: &Path,
    patch_text: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let patch_files = ops::patch::parse_patch(patch_text)
        .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

    // Find the hunks for this file. The patch may contain hunks for multiple
    // files; we only apply the ones matching the given path.
    let path_str_ref: &str = &path_str;
    let hunks: Vec<_> = patch_files
        .iter()
        .filter(|pf| {
            pf.path == path_str_ref || std::path::Path::new(path_str_ref).ends_with(&pf.path)
        })
        .flat_map(|pf| pf.hunks.iter())
        .collect();

    if hunks.is_empty() {
        bail!("no hunks found for path '{}'", path_str);
    }

    let owned_hunks: Vec<_> = hunks.into_iter().cloned().collect();
    let new_content = ops::patch::apply_hunks(&original, &owned_hunks)
        .map_err(|e| anyhow::anyhow!("patch apply error: {e}"))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "patch",
        None,
    ))
}

/// Apply a multi-file patch. Returns one `EditResult` per affected file.
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let patch_files = ops::patch::parse_patch(patch_text)
        .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

    let mut results = Vec::new();
    for pf in &patch_files {
        let file_path = cwd.join(&pf.path);
        let original = std::fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?;

        let new_content = ops::patch::apply_hunks(&original, &pf.hunks)
            .map_err(|e| anyhow::anyhow!("patch apply error for {}: {e}", pf.path))?;

        let policy = WritePolicy::default();
        let applied = write_if_apply(&file_path, &new_content, mode, &policy, guard)?;
        results.push(build_edit_result(
            &pf.path,
            original,
            new_content,
            applied,
            "patch",
            None,
        ));
    }
    Ok(results)
}
