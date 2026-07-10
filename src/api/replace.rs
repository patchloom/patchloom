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

    let range_str = opts.range.map(|(start, end)| {
        if let Some(e) = end {
            format!("{start}:{e}")
        } else {
            format!("{start}:")
        }
    });
    // Shell command-position is content-path only for now; for files, read +
    // replace_in_content so require_change / command_position stay consistent.
    if opts.command_position {
        let original = std::fs::read_to_string(path).map_err(|e| {
            crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::OperationFailed,
                format!("failed to read {}: {e}", path.display()),
            )
        })?;
        let content_result = replace_in_content(&original, from, to, opts)?;
        let policy = crate::write::WritePolicy::default();
        let applied =
            super::write_if_apply(path, &content_result.new_content, mode, &policy, guard)?;
        let path_str = path.to_string_lossy();
        let mut result = super::build_edit_result(
            &path_str,
            content_result.original,
            content_result.new_content,
            applied,
            "replace",
            None,
        );
        result.match_count = content_result.match_count;
        return Ok(result);
    }

    let op = Operation::Replace {
        glob: None,
        path: Some(path.to_string_lossy().into()),
        regex: opts.regex,
        old: from.into(),
        new_text: Some(to.into()),
        nth: opts.nth,
        insert_before: opts.insert_before.clone(),
        insert_after: opts.insert_after.clone(),
        case_insensitive: opts.case_insensitive,
        multiline: opts.multiline,
        if_exists: opts.if_exists,
        whole_line: opts.whole_line,
        range: range_str,
        word_boundary: opts.word_boundary,
        before_context: opts.before_context.clone(),
        after_context: opts.after_context.clone(),
        unique: opts.unique,
        // require_change is applied below as structured EditError (not via tx bail).
        require_change: false,
        command_position: opts.command_position,
    };
    let result = replace_write(op, path, mode, guard, opts.fuzzy)?;
    // if_exists intentionally softens zero-match (Ok unchanged). require_change
    // must not override that: both true means if_exists wins (#1492 docs).
    if opts.require_change && !opts.if_exists && !result.changed && result.match_count == 0 {
        let similar = crate::fallback::find_similar_targets(&result.original_content, from, 3);
        let mut msg = format!("no matches for {:?}", from);
        if !similar.is_empty() {
            msg.push_str(&format!(" (did you mean: {}?)", similar.join(", ")));
        }
        return Err(
            crate::fallback::EditError::new(crate::fallback::EditErrorKind::NoMatch, msg)
                .with_similar(similar)
                .into(),
        );
    }
    Ok(result)
}

/// Unified write path for replace operations.
#[cfg(any(feature = "cli", feature = "files"))]
fn replace_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    _fuzzy: bool,
) -> anyhow::Result<EditResult> {
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "replace", None)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn replace_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    fuzzy: bool,
) -> anyhow::Result<EditResult> {
    use crate::ops;
    use anyhow::{Context, bail};

    // Fallback for no-cli/files builds: delegate to ops layer directly.
    if let Operation::Replace {
        old,
        new_text,
        regex: regex_mode,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        if_exists,
        whole_line,
        range,
        word_boundary,
        nth,
        unique,
        before_context,
        after_context,
        ..
    } = op
    {
        let path_str = path.to_string_lossy();
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let is_regex = regex_mode;
        if old.is_empty() && !is_regex {
            bail!("empty search pattern");
        }

        let compiled_re = ops::replace::compile_replace_regex(
            &old,
            is_regex,
            case_insensitive,
            multiline,
            word_boundary,
        )?;

        let direct_to = if insert_before.is_none() && insert_after.is_none() {
            new_text.clone()
        } else {
            None
        };
        let replacement = ops::replace::replacement_text(
            &old,
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
                &old,
                &replacement,
                compiled_re.as_ref(),
                nth,
                parsed_range,
            )
        } else {
            ops::replace::replace_content(&original, &old, &replacement, compiled_re.as_ref(), nth)
        };

        // Context disambiguation: when multiple exact matches and context is
        // provided, select the match nearest to the context instead of
        // replacing all (mirrors tx engine logic in replace_op.rs).
        if count > 1
            && nth.is_none()
            && !whole_line
            && !is_regex
            && (before_context.is_some() || after_context.is_some())
        {
            if let Some(target_offset) = ops::replace::context_filtered_offset(
                &original,
                &old,
                before_context.as_deref(),
                after_context.as_deref(),
            ) {
                let ctx_content = format!(
                    "{}{}{}",
                    &original[..target_offset],
                    &replacement,
                    &original[target_offset + old.len()..],
                );
                let policy = crate::write::WritePolicy::default();
                let applied = super::write_if_apply(path, &ctx_content, mode, &policy, guard)?;
                let mut result = super::build_edit_result(
                    &path_str,
                    original,
                    ctx_content,
                    applied,
                    "replace",
                    None,
                );
                result.match_count = 1;
                return Ok(result);
            }
        }

        let new_content = new_content.into_owned();

        // Note: context disambiguation runs BEFORE unique, so context can
        // resolve ambiguity even with unique=true. This diverges from the tx
        // engine (which bails on unique before context), but is intentional:
        // context-resolved results are unambiguous by definition.
        if unique && count > 1 {
            bail!(
                "ambiguous match: pattern {:?} matches {} times; provide more context to disambiguate",
                old,
                count
            );
        }

        // Fuzzy/context fallback: when exact match fails and fuzzy or context
        // is enabled, try resolve_with_fallback for anchor/similarity matching
        // (mirrors tx engine fallback in replace_op.rs).
        if count == 0 && !is_regex && (fuzzy || before_context.is_some() || after_context.is_some())
        {
            use crate::fallback;
            match fallback::resolve_with_fallback(
                &original,
                &old,
                before_context.as_deref(),
                after_context.as_deref(),
            ) {
                Ok(anchor) => {
                    let to_text = if let Some(ib) = &insert_before {
                        format!("{}{}", ib, anchor.matched_text)
                    } else if let Some(ia) = &insert_after {
                        format!("{}{}", anchor.matched_text, ia)
                    } else {
                        new_text.as_deref().unwrap_or("").to_string()
                    };
                    let fb_content = format!(
                        "{}{}{}",
                        &original[..anchor.start_offset],
                        to_text,
                        &original[anchor.start_offset + anchor.matched_text.len()..],
                    );
                    let policy = crate::write::WritePolicy::default();
                    let applied = super::write_if_apply(path, &fb_content, mode, &policy, guard)?;
                    let mut result = super::build_edit_result(
                        &path_str, original, fb_content, applied, "replace", None,
                    );
                    result.match_count = 1;
                    return Ok(result);
                }
                Err(edit_error) => {
                    if if_exists {
                        return Ok(super::build_edit_result(
                            &path_str,
                            original.clone(),
                            original,
                            false,
                            "replace",
                            None,
                        ));
                    }
                    let similar = fallback::find_similar_targets(&original, &old, 3);
                    let mut msg = format!("no matches for {:?}", old);
                    if let Some(suggestion) = &edit_error.suggestion {
                        msg.push_str(&format!(" (suggestion: {})", suggestion));
                    }
                    if !similar.is_empty() {
                        msg.push_str(&format!(" (did you mean: {}?)", similar.join(", ")));
                    }
                    bail!("{msg}");
                }
            }
        }

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
        let mut result =
            super::build_edit_result(&path_str, original, new_content, applied, "replace", None);
        result.match_count = count;
        Ok(result)
    } else {
        bail!("expected Replace operation")
    }
}

/// Replace text in a content string (no disk I/O).
///
/// Applies the same replacement logic as [`replace_text`] but operates on an
/// in-memory string instead of a file path. Supports all [`ReplaceOptions`]
/// features: regex, word boundary, nth, case insensitive, multiline,
/// insert_before/after, whole_line, range, if_exists, unique, fuzzy,
/// before_context, and after_context.
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

    // Shell command-position path (#1494): only rewrite invocable command tokens.
    if opts.command_position {
        if is_regex
            || opts.whole_line
            || opts.multiline
            || opts.nth.is_some()
            || opts.case_insensitive
            || opts.word_boundary
            || opts.fuzzy
            || opts.before_context.is_some()
            || opts.after_context.is_some()
        {
            return Err(crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::InvalidInput,
                "command_position cannot be combined with regex, whole_line, multiline, nth, \
                 case_insensitive, word_boundary, fuzzy, or context anchors",
            )
            .into());
        }
        if opts.insert_before.is_some() || opts.insert_after.is_some() {
            return Err(crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::InvalidInput,
                "command_position cannot be combined with insert_before/insert_after",
            )
            .into());
        }
        let (new_content, count) =
            crate::ops::shell_token::replace_command_position(content, from, to);
        return finalize_content_replace(content, from, new_content, count, opts);
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

    // Context disambiguation (#1315): when multiple exact matches and context
    // is provided, select the match nearest to the context instead of
    // replacing all (mirrors replace_write and tx engine logic).
    if count > 1
        && opts.nth.is_none()
        && !opts.whole_line
        && !is_regex
        && (opts.before_context.is_some() || opts.after_context.is_some())
        && let Some(target_offset) = ops::replace::context_filtered_offset(
            content,
            from,
            opts.before_context.as_deref(),
            opts.after_context.as_deref(),
        )
    {
        let ctx_content = format!(
            "{}{}{}",
            &content[..target_offset],
            replacement,
            &content[target_offset + from.len()..],
        );
        let diff = super::make_diff("<content>", content, &ctx_content);
        return Ok(ContentEditResult {
            original: content.to_string(),
            new_content: ctx_content,
            diff,
            changed: true,
            match_count: 1,
        });
    }

    let new_content = new_content.into_owned();

    // Fuzzy/context fallback (#1286, #1315): when exact match fails and fuzzy
    // or context is enabled, try resolve_with_fallback for anchor/similarity
    // matching (matches replace_write and tx engine behavior).
    // Ambiguous (unique + count>1) is finalized below so command_position and
    // the normal path share one check in finalize_content_replace.
    if count == 0
        && !is_regex
        && (opts.fuzzy || opts.before_context.is_some() || opts.after_context.is_some())
    {
        use crate::fallback;
        match fallback::resolve_with_fallback(
            content,
            from,
            opts.before_context.as_deref(),
            opts.after_context.as_deref(),
        ) {
            Ok(anchor) => {
                let to_text = if let Some(ib) = &opts.insert_before {
                    format!("{}{}", ib, anchor.matched_text)
                } else if let Some(ia) = &opts.insert_after {
                    format!("{}{}", anchor.matched_text, ia)
                } else {
                    to.to_string()
                };
                let fuzzy_content = format!(
                    "{}{}{}",
                    &content[..anchor.start_offset],
                    to_text,
                    &content[anchor.start_offset + anchor.matched_text.len()..]
                );
                let diff = super::make_diff("<content>", content, &fuzzy_content);
                return Ok(ContentEditResult {
                    original: content.to_string(),
                    new_content: fuzzy_content,
                    diff,
                    changed: true,
                    match_count: 1,
                });
            }
            Err(edit_error) => {
                if opts.if_exists {
                    return Ok(ContentEditResult {
                        original: content.to_string(),
                        new_content: content.to_string(),
                        diff: String::new(),
                        changed: false,
                        match_count: 0,
                    });
                }
                let similar = fallback::find_similar_targets(content, from, 3);
                let mut msg = format!("no matches for {:?}", from);
                if let Some(suggestion) = &edit_error.suggestion {
                    msg.push_str(&format!(" (suggestion: {})", suggestion));
                }
                if !similar.is_empty() {
                    msg.push_str(&format!(" (did you mean: {}?)", similar.join(", ")));
                }
                let mut err =
                    crate::fallback::EditError::new(crate::fallback::EditErrorKind::NoMatch, msg)
                        .with_similar(similar);
                if let Some(s) = edit_error.suggestion.clone() {
                    err = err.with_suggestion(s);
                }
                return Err(err.into());
            }
        }
    }

    finalize_content_replace(content, from, new_content, count, opts)
}

/// Shared zero-match / success finalization for content replace paths (#1492).
fn finalize_content_replace(
    content: &str,
    from: &str,
    new_content: String,
    count: usize,
    opts: &ReplaceOptions,
) -> anyhow::Result<ContentEditResult> {
    if opts.unique && count > 1 {
        return Err(crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::AmbiguousTarget,
            format!(
                "ambiguous match: pattern {:?} matches {} times; provide more context to disambiguate",
                from, count
            ),
        )
        .into());
    }

    if count == 0 && opts.if_exists {
        return Ok(ContentEditResult {
            original: content.to_string(),
            new_content: content.to_string(),
            diff: String::new(),
            changed: false,
            match_count: 0,
        });
    }

    if count == 0 && opts.require_change {
        let similar = crate::fallback::find_similar_targets(content, from, 3);
        let mut msg = format!("no matches for {:?}", from);
        if !similar.is_empty() {
            msg.push_str(&format!(" (did you mean: {}?)", similar.join(", ")));
        }
        return Err(
            crate::fallback::EditError::new(crate::fallback::EditErrorKind::NoMatch, msg)
                .with_similar(similar)
                .into(),
        );
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
        match_count: count,
    })
}
