//! Patch (unified diff) apply operations for the library API.
//!
//! Single-file `apply_patch` delegates to the tx engine via `execute_as_edit_result`.
//! Multi-file `apply_patch_file` retains a direct implementation.

use std::path::Path;

use anyhow::Context;

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Apply a unified diff patch to a file.
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(
    path: &Path,
    patch_text: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::PatchApply {
        diff: patch_text.into(),
        on_stale: Default::default(),
        allow_conflicts: false,
    };
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    patch_write(op, cwd, mode, guard)
}

#[cfg(any(feature = "cli", feature = "files"))]
fn patch_write(
    op: Operation,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    super::execute_as_edit_result(op, mode, cwd, guard, "patch", None)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn patch_write(
    _op: Operation,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    use crate::ops;

    if let Operation::PatchApply { diff, .. } = _op {
        let patch_files = ops::patch::parse_patch(&diff)
            .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

        if patch_files.is_empty() {
            anyhow::bail!("no files in patch");
        }

        // Apply to the first file in the patch.
        let pf = &patch_files[0];
        let file_path = cwd.join(&pf.path);
        let original = std::fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?;

        let new_content = ops::patch::apply_hunks(&original, &pf.hunks)
            .map_err(|e| anyhow::anyhow!("patch apply error: {e}"))?;

        let policy = crate::write::WritePolicy::default();
        let applied = super::write_if_apply(&file_path, &new_content, mode, &policy, guard)?;
        Ok(super::build_edit_result(
            &pf.path,
            original,
            new_content,
            applied,
            "patch",
            None,
        ))
    } else {
        anyhow::bail!("expected PatchApply operation")
    }
}

/// Apply a multi-file patch. Returns one `EditResult` per affected file.
///
/// Retains direct implementation since it produces multiple `EditResult`s
/// (one per file), which the single-op `execute_as_edit_result` adapter
/// doesn't support.
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let patch_files = crate::ops::patch::parse_patch(patch_text)
        .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

    let mut results = Vec::new();
    for pf in &patch_files {
        let file_path = cwd.join(&pf.path);
        let original = std::fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?;

        let new_content = crate::ops::patch::apply_hunks(&original, &pf.hunks)
            .map_err(|e| anyhow::anyhow!("patch apply error for {}: {e}", pf.path))?;

        let policy = crate::write::WritePolicy::default();
        let applied = super::write_if_apply(&file_path, &new_content, mode, &policy, guard)?;
        results.push(super::build_edit_result(
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
