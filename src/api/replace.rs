//! Text replacement operations for the public library API.
//!
//! Delegates to the tx engine via `execute_as_edit_result`.

use std::path::Path;

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, ContentEditResult, EditResult, MatchMode, ReplaceOptions};

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
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "empty search pattern".into(),
        }));
    }
    if opts.range.is_some() && !opts.whole_line {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "range requires whole_line to be true".into(),
        }));
    }
    if opts.whole_line && opts.multiline {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "whole_line and multiline cannot be combined".into(),
        }));
    }

    let range_str = opts.range.map(|(start, end)| {
        if let Some(e) = end {
            format!("{start}:{e}")
        } else {
            format!("{start}:")
        }
    });
    // Library-only match options that the plan/tx Operation does not carry
    // (`fuzzy`) or that need honest `match_mode` (#1662): use the same
    // content path as command_position so disk and in-memory parity hold.
    // Pure `fuzzy: true` without context was previously a no-op on the
    // default files/cli path (tx only falls back when context is set).
    if opts.command_position
        || opts.fuzzy
        || opts.before_context.is_some()
        || opts.after_context.is_some()
    {
        let original = std::fs::read_to_string(path).map_err(|e| {
            crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::OperationFailed,
                format!("failed to read {}: {e}", path.display()),
            )
        })?;
        let content_result = replace_in_content(&original, from, to, opts)?;
        let policy = crate::write::WritePolicy::default();
        let (applied, backup_session) =
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
        result.match_mode = content_result.match_mode;
        result.match_score = content_result.match_score;
        result.matched_text = content_result.matched_text;
        result.backup_session = backup_session.clone();
        super::maybe_post_write(
            applied,
            path,
            opts.post_write.as_ref(),
            opts.post_write_cwd.as_deref(),
            backup_session.as_deref(),
        )?;
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
        fuzzy: false,
        min_fuzzy_score: opts.min_fuzzy_score,
        allow_absent_old: opts.allow_absent_old,
    };
    let mut result = replace_write(op, path, mode, guard, opts.fuzzy)?;
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
    // execute_as_edit_result now threads match_mode/count from replace_op meta.
    // Keep a conservative fallback for any path that still omits meta (#1662).
    if result.match_mode.is_none() && (result.match_count > 0 || result.changed) {
        result.match_mode = Some(MatchMode::Exact);
        if result.match_count == 0 && result.changed {
            result.match_count = 1;
        }
    }
    // Post-Apply format/lint hooks (#1690).
    super::maybe_post_write(
        result.applied,
        path,
        opts.post_write.as_ref(),
        opts.post_write_cwd.as_deref(),
        result.backup_session.as_deref(),
    )?;
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
        min_fuzzy_score,
        allow_absent_old,
        ..
    } = op
    {
        let path_str = path.to_string_lossy();
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let is_regex = regex_mode;
        if old.is_empty() && !is_regex {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "empty search pattern".into(),
            }));
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
            &original,
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
        // Fail closed when context does not select a unique match.
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
                let (applied, backup_session) =
                    super::write_if_apply(path, &ctx_content, mode, &policy, guard)?;
                let mut result = super::build_edit_result(
                    &path_str,
                    original,
                    ctx_content,
                    applied,
                    "replace",
                    None,
                );
                result.match_count = 1;
                result.match_mode = Some(MatchMode::Anchored);
                result.backup_session = backup_session;
                return Ok(result);
            }
            return Err(anyhow::Error::new(crate::exit::AmbiguousError {
                msg: format!(
                    "ambiguous match: pattern {:?} matches {} times; context did not select a unique occurrence (use --nth or stronger before/after-context)",
                    crate::fallback::truncate_str(&old, 60),
                    count
                ),
            }));
        }

        let new_content = new_content.into_owned();

        // Note: context disambiguation runs BEFORE unique, so context can
        // resolve ambiguity even with unique=true. This diverges from the tx
        // engine (which bails on unique before context), but is intentional:
        // context-resolved results are unambiguous by definition.
        if unique && count > 1 {
            return Err(anyhow::Error::new(crate::exit::AmbiguousError {
                msg: format!(
                    "ambiguous match: pattern {old:?} matches {count} times; use --nth or add context to disambiguate"
                ),
            }));
        }

        // nth past last match: applied count is 0 but pattern may still match.
        if count == 0
            && let Some(n) = nth
        {
            let total = ops::replace::count_nth_candidates(
                &original,
                &old,
                compiled_re.as_ref(),
                whole_line,
                parsed_range,
            );
            if total > 0 && n > total {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!(
                        "nth {n} is out of range: pattern matches {total} time{}",
                        if total == 1 { "" } else { "s" }
                    ),
                }));
            }
        }

        // Fuzzy/context fallback: when exact match fails and fuzzy or context
        // is enabled, try resolve_with_fallback for anchor/similarity matching
        // (mirrors tx engine fallback in replace_op.rs).
        if count == 0 && !is_regex && (fuzzy || before_context.is_some() || after_context.is_some())
        {
            use crate::fallback;
            match fallback::resolve_with_fallback_skip_exact(
                &original,
                &old,
                before_context.as_deref(),
                after_context.as_deref(),
                // Primary path already applied word_boundary exact; bare find
                // would re-accept substrings rejected by \b (#1755).
                word_boundary,
            ) {
                Ok(anchor) => {
                    let (match_mode, default_score) =
                        super::match_mode_from_strategy(anchor.strategy);
                    let score = anchor.score.or(default_score);
                    if match_mode == MatchMode::Fuzzy
                        && let Some(min) = min_fuzzy_score
                        && crate::fallback::fuzzy_fails_min_floor(score, min)
                    {
                        // Floor reject is a soft miss: honor if_exists like resolve Err.
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
                        let actual = score
                            .map(|s| format!("{s:.3}"))
                            .unwrap_or_else(|| "none".into());
                        return Err(anyhow::Error::new(crate::exit::NoMatchError {
                            msg: format!(
                                "fuzzy match score {actual} below min_fuzzy_score {min} for {old:?}"
                            ),
                        }));
                    }
                    if crate::fallback::should_refuse_fuzzy_absent_old(
                        fuzzy,
                        match_mode == MatchMode::Fuzzy,
                        allow_absent_old,
                    ) {
                        // #1758: exact old absent — do not rewrite a different live span.
                        let msg = crate::fallback::fuzzy_absent_old_refuse_message(
                            &old,
                            &anchor.matched_text,
                            score,
                        );
                        if if_exists {
                            let mut result = super::build_edit_result(
                                &path_str,
                                original.clone(),
                                original,
                                false,
                                "replace",
                                None,
                            );
                            result.match_mode = Some(match_mode);
                            result.match_score = score;
                            result.matched_text = Some(anchor.matched_text.clone());
                            return Ok(result);
                        }
                        return Err(anyhow::Error::new(crate::exit::NoMatchError { msg }));
                    }
                    let to_text = if let Some(ib) = &insert_before {
                        let ib = ops::replace::normalize_line_insert(
                            &original,
                            &anchor.matched_text,
                            ib,
                            ops::replace::InsertSide::Before,
                        );
                        format!("{}{}", ib, anchor.matched_text)
                    } else if let Some(ia) = &insert_after {
                        let ia = ops::replace::normalize_line_insert(
                            &original,
                            &anchor.matched_text,
                            ia,
                            ops::replace::InsertSide::After,
                        );
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
                    let (applied, backup_session) =
                        super::write_if_apply(path, &fb_content, mode, &policy, guard)?;
                    let mut result = super::build_edit_result(
                        &path_str, original, fb_content, applied, "replace", None,
                    );
                    result.match_count = 1;
                    result.match_mode = Some(match_mode);
                    result.match_score = score;
                    result.matched_text = Some(anchor.matched_text.clone());
                    result.backup_session = backup_session;
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
                    let mut msg = format!("no matches for {old:?}");
                    if let Some(suggestion) = &edit_error.suggestion {
                        msg.push_str(&format!(" (suggestion: {suggestion})"));
                    }
                    if !similar.is_empty() {
                        msg.push_str(&format!(" (did you mean: {}?)", similar.join(", ")));
                    }
                    return Err(anyhow::Error::new(crate::exit::NoMatchError { msg }));
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
        let (applied, backup_session) =
            super::write_if_apply(path, &new_content, mode, &policy, guard)?;
        let mut result =
            super::build_edit_result(&path_str, original, new_content, applied, "replace", None);
        result.match_count = count;
        if count > 0 {
            result.match_mode = Some(MatchMode::Exact);
        }
        result.backup_session = backup_session;
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

    let is_regex = opts.regex;
    // Match replace_text: typed InvalidInputError so edit_error_kind peels
    // invalid_input without scraping English.
    if from.is_empty() && !is_regex {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "empty search pattern".into(),
        }));
    }
    if let Some(min) = opts.min_fuzzy_score
        && (!(0.0..=1.0).contains(&min) || min.is_nan())
    {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "min_fuzzy_score must be in 0.0..=1.0 (got {min}); leave None for no floor"
            ),
        }));
    }
    if opts.range.is_some() && !opts.whole_line {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "range requires whole_line to be true".into(),
        }));
    }
    if opts.whole_line && opts.multiline {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "whole_line and multiline cannot be combined".into(),
        }));
    }

    // Shell command-position path (#1494): only rewrite invocable command tokens.
    if opts.command_position {
        if let Some(msg) = crate::ops::shell_token::command_position_combo_error(
            crate::ops::shell_token::CommandPositionIncompat {
                regex: is_regex,
                case_insensitive: opts.case_insensitive,
                word_boundary: opts.word_boundary,
                whole_line: opts.whole_line,
                multiline: opts.multiline,
                nth: opts.nth.is_some(),
                insert_before: opts.insert_before.is_some(),
                insert_after: opts.insert_after.is_some(),
                before_context: opts.before_context.is_some(),
                after_context: opts.after_context.is_some(),
                fuzzy: opts.fuzzy,
            },
        ) {
            return Err(crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::InvalidInput,
                msg,
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
        content,
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

    if count == 0
        && let Some(n) = opts.nth
    {
        let total = ops::replace::count_nth_candidates(
            content,
            from,
            compiled_re.as_ref(),
            opts.whole_line,
            parsed_range,
        );
        if total > 0 && n > total {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!(
                    "nth {n} is out of range: pattern matches {total} time{}",
                    if total == 1 { "" } else { "s" }
                ),
            }));
        }
    }

    // Context disambiguation (#1315): when multiple exact matches and context
    // is provided, select the match nearest to the context instead of
    // replacing all (mirrors replace_write and tx engine logic).
    // Fail closed when context does not select a unique match.
    if count > 1
        && opts.nth.is_none()
        && !opts.whole_line
        && !is_regex
        && (opts.before_context.is_some() || opts.after_context.is_some())
    {
        if let Some(target_offset) = ops::replace::context_filtered_offset(
            content,
            from,
            opts.before_context.as_deref(),
            opts.after_context.as_deref(),
        ) {
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
                match_mode: Some(MatchMode::Anchored),
                match_score: None,
                // Exact span at the context-picked offset (equals `from`; hosts
                // still get a non-null matched_text on Anchored like fuzzy #1736).
                matched_text: Some(from.to_string()),
            });
        }
        return Err(anyhow::Error::new(crate::exit::AmbiguousError {
            msg: format!(
                "ambiguous match: pattern {:?} matches {} times; context did not select a unique occurrence (use --nth or stronger before/after-context)",
                crate::fallback::truncate_str(from, 60),
                count
            ),
        }));
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
        match fallback::resolve_with_fallback_skip_exact(
            content,
            from,
            opts.before_context.as_deref(),
            opts.after_context.as_deref(),
            // Primary path already applied word_boundary exact; bare find
            // would re-accept substrings rejected by \b (#1755).
            opts.word_boundary,
        ) {
            Ok(anchor) => {
                let (mode, default_score) = super::match_mode_from_strategy(anchor.strategy);
                let score = anchor.score.or(default_score);
                if mode == super::MatchMode::Fuzzy
                    && let Some(min) = opts.min_fuzzy_score
                    && crate::fallback::fuzzy_fails_min_floor(score, min)
                {
                    // Floor reject is a soft miss: honor if_exists like resolve Err
                    // (#1750). Without if_exists, still hard NoMatch.
                    if opts.if_exists {
                        return Ok(ContentEditResult {
                            original: content.to_string(),
                            new_content: content.to_string(),
                            diff: String::new(),
                            changed: false,
                            match_count: 0,
                            match_mode: None,
                            match_score: None,
                            matched_text: None,
                        });
                    }
                    let actual = score
                        .map(|s| format!("{s:.3}"))
                        .unwrap_or_else(|| "none".into());
                    return Err(crate::fallback::EditError::new(
                        crate::fallback::EditErrorKind::NoMatch,
                        format!(
                            "fuzzy match score {actual} below min_fuzzy_score {min} for {:?}",
                            from
                        ),
                    )
                    .into());
                }
                if crate::fallback::should_refuse_fuzzy_absent_old(
                    opts.fuzzy,
                    mode == super::MatchMode::Fuzzy,
                    opts.allow_absent_old,
                ) {
                    // #1758: report candidate honesty without rewriting a different live span.
                    let msg = crate::fallback::fuzzy_absent_old_refuse_message(
                        from,
                        &anchor.matched_text,
                        score,
                    );
                    if opts.if_exists {
                        return Ok(ContentEditResult {
                            original: content.to_string(),
                            new_content: content.to_string(),
                            diff: String::new(),
                            changed: false,
                            match_count: 0,
                            match_mode: Some(mode),
                            match_score: score,
                            matched_text: Some(anchor.matched_text.clone()),
                        });
                    }
                    return Err(crate::fallback::EditError::new(
                        crate::fallback::EditErrorKind::NoMatch,
                        msg,
                    )
                    .into());
                }
                let to_text = if let Some(ib) = &opts.insert_before {
                    let ib = ops::replace::normalize_line_insert(
                        content,
                        &anchor.matched_text,
                        ib,
                        ops::replace::InsertSide::Before,
                    );
                    format!("{}{}", ib, anchor.matched_text)
                } else if let Some(ia) = &opts.insert_after {
                    let ia = ops::replace::normalize_line_insert(
                        content,
                        &anchor.matched_text,
                        ia,
                        ops::replace::InsertSide::After,
                    );
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
                    match_mode: Some(mode),
                    match_score: score,
                    matched_text: Some(anchor.matched_text.clone()),
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
                        match_mode: None,
                        match_score: None,

                        matched_text: None,
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
                "ambiguous match: pattern {:?} matches {} times; use --nth or add context to disambiguate",
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
            match_mode: None,
            match_score: None,

            matched_text: None,
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
        match_mode: if count > 0 {
            Some(MatchMode::Exact)
        } else {
            None
        },
        match_score: None,

        matched_text: None,
    })
}
