//! Text replacement operations for the public library API.
//!
//! Delegates to the tx engine via `execute_as_edit_result`.

use std::path::Path;

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, ContentEditResult, EditResult, ReplaceOptions};

/// Replace text in a file using literal or regex matching.
///
/// When `opts.insert_before` or `opts.insert_after` is set, the matched text
/// is preserved and the insertion is added adjacent to it.
pub fn replace_text(
    path: &Path,
    from: &str,
    to: &str,
    opts: &ReplaceOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    // Pre-validate before building the Operation.
    if from.is_empty() && !opts.regex {
        anyhow::bail!("empty search pattern");
    }
    if opts.range.is_some() && !opts.whole_line {
        anyhow::bail!("range requires whole_line to be true");
    }
    if opts.whole_line && opts.multiline {
        anyhow::bail!("whole_line and multiline cannot be combined");
    }

    let mode_str = if opts.regex {
        Some("regex".to_string())
    } else {
        None
    };
    let range_str = opts.range.map(|(start, end)| {
        if let Some(e) = end {
            format!("{start}:{e}")
        } else {
            format!("{start}:")
        }
    });
    let op = Operation::Replace {
        glob: None,
        path: Some(path.to_string_lossy().into()),
        mode: mode_str,
        from: from.into(),
        to: Some(to.into()),
        nth: opts.nth,
        insert_before: opts.insert_before.clone(),
        insert_after: opts.insert_after.clone(),
        case_insensitive: opts.case_insensitive,
        multiline: opts.multiline,
        if_exists: opts.if_exists,
        whole_line: opts.whole_line,
        range: range_str,
        word_boundary: opts.word_boundary,
        before_context: None,
        after_context: None,
    };
    replace_write(op, path, mode, guard)
}

/// Unified write path for replace operations.
#[cfg(any(feature = "cli", feature = "files"))]
fn replace_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "replace")
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn replace_write(
    _op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    use crate::ops;
    use anyhow::{Context, bail};

    // Fallback for no-cli/files builds: delegate to ops layer directly.
    if let Operation::Replace {
        from,
        to,
        mode: mode_str,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        if_exists,
        whole_line,
        range,
        word_boundary,
        nth,
        ..
    } = _op
    {
        let path_str = path.to_string_lossy();
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let is_regex = mode_str.as_deref() == Some("regex");
        if from.is_empty() && !is_regex {
            bail!("empty search pattern");
        }

        let compiled_re = ops::replace::compile_replace_regex(
            &from,
            is_regex,
            case_insensitive,
            multiline,
            word_boundary,
        )?;

        let direct_to = if insert_before.is_none() && insert_after.is_none() {
            to.clone()
        } else {
            None
        };
        let replacement = ops::replace::replacement_text(
            &from,
            &direct_to,
            &insert_before,
            &insert_after,
            compiled_re.is_some(),
            is_regex,
        );

        let parsed_range = range.as_deref().map(|r| {
            let parts: Vec<&str> = r.splitn(2, ':').collect();
            let start: usize = parts[0].parse().unwrap_or(1);
            let end: Option<usize> = parts
                .get(1)
                .and_then(|s| if s.is_empty() { None } else { s.parse().ok() });
            (start, end)
        });

        let (new_content, count) = if whole_line {
            ops::replace::replace_whole_lines(
                &original,
                &from,
                &replacement,
                compiled_re.as_ref(),
                nth,
                parsed_range,
            )
        } else {
            ops::replace::replace_content(&original, &from, &replacement, compiled_re.as_ref(), nth)
        };
        let new_content = new_content.into_owned();

        if count == 0 && if_exists {
            return Ok(super::build_edit_result(
                &path_str,
                original.clone(),
                original,
                false,
                "replace",
                None,
            ));
        }

        let policy = crate::write::WritePolicy::default();
        let applied = super::write_if_apply(path, &new_content, mode, &policy, guard)?;
        Ok(super::build_edit_result(
            &path_str,
            original,
            new_content,
            applied,
            "replace",
            None,
        ))
    } else {
        bail!("expected Replace operation")
    }
}

/// Replace text in a content string (no disk I/O).
///
/// Applies the same replacement logic as [`replace_text`] but operates on an
/// in-memory string instead of a file path. Supports all [`ReplaceOptions`]
/// features: regex, word boundary, nth, case insensitive, multiline,
/// insert_before/after, whole_line, range, and if_exists.
pub fn replace_in_content(
    content: &str,
    from: &str,
    to: &str,
    opts: &ReplaceOptions,
) -> anyhow::Result<ContentEditResult> {
    use crate::ops;
    use anyhow::bail;

    let is_regex = opts.regex;
    if from.is_empty() && !is_regex {
        bail!("empty search pattern");
    }
    if opts.range.is_some() && !opts.whole_line {
        bail!("range requires whole_line to be true");
    }
    if opts.whole_line && opts.multiline {
        bail!("whole_line and multiline cannot be combined");
    }

    let compiled_re = ops::replace::compile_replace_regex(
        from,
        is_regex,
        opts.case_insensitive,
        opts.multiline,
        opts.word_boundary,
    )?;

    let direct_to = if opts.insert_before.is_none() && opts.insert_after.is_none() {
        Some(to.to_string())
    } else {
        None
    };
    let replacement = ops::replace::replacement_text(
        from,
        &direct_to,
        &opts.insert_before,
        &opts.insert_after,
        compiled_re.is_some(),
        is_regex,
    );

    let parsed_range = opts.range;

    let (new_content, count) = if opts.whole_line {
        ops::replace::replace_whole_lines(
            content,
            from,
            &replacement,
            compiled_re.as_ref(),
            opts.nth,
            parsed_range,
        )
    } else {
        ops::replace::replace_content(content, from, &replacement, compiled_re.as_ref(), opts.nth)
    };
    let new_content = new_content.into_owned();

    if count == 0 && opts.if_exists {
        return Ok(ContentEditResult {
            original: content.to_string(),
            new_content: content.to_string(),
            diff: String::new(),
            changed: false,
        });
    }

    let changed = content != new_content;
    let diff = if changed {
        super::make_diff("<content>", content, &new_content)
    } else {
        String::new()
    };

    Ok(ContentEditResult {
        original: content.to_string(),
        new_content,
        diff,
        changed,
    })
}
