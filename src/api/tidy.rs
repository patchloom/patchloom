//! Tidy (whitespace normalization) operation for the library API.
//!
//! Delegates to the tx engine via `execute_as_edit_result`.

use std::path::Path;

use crate::containment::PathGuard;
use crate::plan::Operation;
use crate::write::EolMode;

use super::{ApplyMode, EditResult, WritePolicyOptions};

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
    // Operation::TidyFix does not support collapse_blanks. When that option
    // is set, use the direct implementation to apply the full policy.
    if policy_opts.collapse_blanks {
        return tidy_direct(path, policy_opts, mode, guard);
    }
    let eol_str = policy_opts.normalize_eol.map(|eol| match eol {
        EolMode::Lf => "lf".to_string(),
        EolMode::Crlf => "crlf".to_string(),
        EolMode::Cr => "cr".to_string(),
        EolMode::Keep => "keep".to_string(),
    });
    let op = Operation::TidyFix {
        path: path.to_string_lossy().into(),
        ensure_final_newline: Some(policy_opts.ensure_final_newline),
        trim_trailing_whitespace: Some(policy_opts.trim_trailing_whitespace),
        normalize_eol: eol_str,
    };
    tidy_write(op, path, mode, guard)
}

/// Direct tidy implementation for cases not supported by Operation::TidyFix
/// (e.g., collapse_blanks).
fn tidy_direct(
    path: &Path,
    policy_opts: &WritePolicyOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    use anyhow::Context;

    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let policy = super::make_write_policy(policy_opts);
    let new_content = crate::write::apply_policy(&original, &policy).into_owned();

    let noop_policy = crate::write::WritePolicy::default();
    let applied = super::write_if_apply(path, &new_content, mode, &noop_policy, guard)?;
    Ok(super::build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
        None,
    ))
}

#[cfg(any(feature = "cli", feature = "files"))]
fn tidy_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "tidy")
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn tidy_write(
    _op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    use anyhow::Context;

    // Fallback: apply policy directly through the write module.
    if let Operation::TidyFix {
        ensure_final_newline,
        trim_trailing_whitespace,
        normalize_eol,
        ..
    } = _op
    {
        let path_str = path.to_string_lossy();
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let eol = normalize_eol
            .as_deref()
            .map(|s| match s {
                "lf" => EolMode::Lf,
                "crlf" => EolMode::Crlf,
                "cr" => EolMode::Cr,
                _ => EolMode::Keep,
            })
            .unwrap_or(EolMode::Keep);
        let policy = crate::write::WritePolicy {
            ensure_final_newline: ensure_final_newline.unwrap_or(false),
            normalize_eol: eol,
            trim_trailing_whitespace: trim_trailing_whitespace.unwrap_or(false),
            collapse_blanks: false,
        };
        let new_content = crate::write::apply_policy(&original, &policy).into_owned();

        let noop_policy = crate::write::WritePolicy::default();
        let applied = super::write_if_apply(path, &new_content, mode, &noop_policy, guard)?;
        Ok(super::build_edit_result(
            &path_str,
            original,
            new_content,
            applied,
            "tidy",
            None,
        ))
    } else {
        anyhow::bail!("expected TidyFix operation")
    }
}
