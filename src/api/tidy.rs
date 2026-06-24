//! Tidy (whitespace normalization) operation for the library API.

use std::path::Path;

use anyhow::Context;

use crate::containment::PathGuard;
use crate::write::WritePolicy;

use super::{
    ApplyMode, EditResult, WritePolicyOptions, build_edit_result, make_write_policy, write_if_apply,
};

/// Apply whitespace normalization to a file using the given write policy.
///
/// Normalizes final newlines, line endings, trailing whitespace, and
/// consecutive blank lines according to the policy options.
pub fn tidy(
    path: &Path,
    policy_opts: &WritePolicyOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let policy = make_write_policy(policy_opts);
    let new_content = crate::write::apply_policy(&original, &policy).into_owned();

    // Use a default (no-op) policy for writing since we already applied
    // the transformations above.
    let noop_policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &noop_policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
        None,
    ))
}
