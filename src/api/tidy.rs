//! Tidy (whitespace normalization) operation for the library API.
//!
//! Delegates to the tx engine via `execute_as_edit_result`.

use std::path::Path;

use crate::containment::PathGuard;
use crate::plan::Operation;
use crate::write::EolMode;

use super::{ApplyMode, EditResult, WritePolicyOptions};

/// Options for indentation transforms (separate from write policy).
#[derive(Debug, Clone, Default)]
pub struct TidyIndentOptions {
    /// Dedent specification: `"auto"`, `"tab"`, or a numeric string (e.g. `"4"`).
    pub dedent: Option<String>,
    /// Indent specification: `"tab"` or a numeric string (e.g. `"4"`).
    pub indent: Option<String>,
    /// Line range restriction: `"10:50"` (1-based inclusive).
    pub lines: Option<String>,
}

/// Apply whitespace normalization to a file using the given write policy.
///
/// Normalizes final newlines, line endings, trailing whitespace, and
/// consecutive blank lines according to the policy options. Optionally
/// applies dedent/indent transforms via `indent_opts`.
pub fn tidy(
    path: &Path,
    policy_opts: &WritePolicyOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    tidy_with_indent(
        path,
        policy_opts,
        &TidyIndentOptions::default(),
        mode,
        guard,
    )
}

/// Apply whitespace normalization with optional dedent/indent transforms.
pub fn tidy_with_indent(
    path: &Path,
    policy_opts: &WritePolicyOptions,
    indent_opts: &TidyIndentOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
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
        collapse_blanks: if policy_opts.collapse_blanks {
            Some(true)
        } else {
            None
        },
        dedent: indent_opts.dedent.clone(),
        indent: indent_opts.indent.clone(),
        lines: indent_opts.lines.clone(),
    };
    let result = tidy_write(op, path, mode, guard)?;
    // Optional post-Apply format/lint hooks from WritePolicyOptions (#1690).
    super::maybe_post_write(
        result.applied,
        path,
        policy_opts.post_write.as_ref(),
        policy_opts.post_write_cwd.as_deref(),
        result.backup_session.as_deref(),
    )?;
    Ok(result)
}

#[cfg(any(feature = "cli", feature = "files"))]
fn tidy_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "tidy", None)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn tidy_write(
    _op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    // Fallback: apply policy directly through the write module.
    if let Operation::TidyFix {
        ensure_final_newline,
        trim_trailing_whitespace,
        normalize_eol,
        collapse_blanks,
        dedent,
        indent,
        lines,
        ..
    } = _op
    {
        let path_str = path.to_string_lossy();
        let original = crate::files::load_text_strict(path, &path_str)?;

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
            collapse_blanks: collapse_blanks.unwrap_or(false),
        };
        let mut new_content = crate::write::apply_policy(&original, &policy).into_owned();

        // Apply dedent/indent after policy normalization.
        let line_range = lines
            .as_deref()
            .map(crate::ops::read::parse_line_range)
            .transpose()?;
        if let Some(ref spec) = dedent {
            new_content = crate::write::dedent_content(&new_content, spec, line_range);
        }
        if let Some(ref spec) = indent {
            new_content = crate::write::indent_content(&new_content, spec, line_range);
        }

        let noop_policy = crate::write::WritePolicy::default();
        let (applied, backup_session) =
            super::write_if_apply(path, &new_content, mode, &noop_policy, guard)?;
        {
            let mut __e =
                super::build_edit_result(&path_str, original, new_content, applied, "tidy", None);
            __e.backup_session = backup_session;
            Ok(__e)
        }
    } else {
        anyhow::bail!("expected TidyFix operation")
    }
}
