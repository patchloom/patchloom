//! Text replacement operations for the public library API.

use std::path::Path;

use anyhow::{Context, bail};

use crate::containment::PathGuard;
use crate::ops;
use crate::write::WritePolicy;

use super::{ApplyMode, EditResult, ReplaceOptions, build_edit_result, write_if_apply};

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
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if from.is_empty() && !opts.regex {
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
        opts.regex,
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
    );

    let (new_content, count) = if opts.whole_line {
        ops::replace::replace_whole_lines(
            &original,
            from,
            &replacement,
            compiled_re.as_ref(),
            opts.nth,
            opts.range,
        )
    } else {
        ops::replace::replace_content(
            &original,
            from,
            &replacement,
            compiled_re.as_ref(),
            opts.nth,
        )
    };
    let new_content = new_content.into_owned();

    if count == 0 && opts.if_exists {
        return Ok(build_edit_result(
            &path_str,
            original.clone(),
            original,
            false,
            "replace",
            None,
        ));
    }

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "replace",
        None,
    ))
}
