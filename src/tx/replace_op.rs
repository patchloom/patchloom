//! Tx replace operation execution.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). Path + glob replace
//! with fuzzy/context fallback co-located; do not split for LOC alone.

use super::execute::{TxState, read_and_probe, read_file_content};
use crate::api::MatchMode;
use crate::ops::replace::{
    InsertSide, compile_replace_regex, context_filtered_offset, normalize_line_insert,
    replace_content, replace_whole_lines, replacement_text,
};
use crate::plan::Operation;
use crate::tx::output::merge_match_modes;
use globset::Glob;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::Path;

/// Record replace match honesty for a path (worst-case merge if re-visited) (#1674 / #1736).
fn record_replace_match(
    tx: &mut TxState<'_>,
    path: &Path,
    mode: MatchMode,
    score: Option<f64>,
    match_count: usize,
    matched_text: Option<String>,
) {
    record_replace_match_with_reason(tx, path, mode, score, match_count, matched_text, None);
}

/// Soft exact no-match (zero writes) for multi-op honesty. Listed in
/// `refused[]` with reason `no_matches` when other ops still succeed.
fn record_soft_no_match(tx: &mut TxState<'_>, path: &Path) {
    record_replace_match_with_reason(
        tx,
        path,
        MatchMode::Exact,
        None,
        0,
        None,
        Some("no_matches"),
    );
}

/// Like [`record_replace_match`], with optional soft-refuse reason for `refused[]`.
fn record_replace_match_with_reason(
    tx: &mut TxState<'_>,
    path: &Path,
    mode: MatchMode,
    score: Option<f64>,
    match_count: usize,
    matched_text: Option<String>,
    refuse_reason: Option<&'static str>,
) {
    use crate::tx::output::ReplaceMatchMeta;
    let entry = tx.replace_match_meta.entry(path.to_path_buf());
    match entry {
        std::collections::hash_map::Entry::Vacant(e) => {
            e.insert(ReplaceMatchMeta {
                mode,
                score,
                match_count,
                matched_text,
                refuse_reason,
            });
        }
        std::collections::hash_map::Entry::Occupied(mut e) => {
            let prev = e.get().clone();
            let merged = merge_match_modes(Some(prev.mode), mode);
            // Worst-case confidence: keep the lowest fuzzy score when both exist
            // (#1747 aggregate rule; do not first-wins via score.or).
            let merged_score = if matches!(merged, MatchMode::Fuzzy) {
                match (prev.score, score) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }
            } else {
                None
            };
            // Prefer the first non-exact span so multi-op plans keep the fuzzy text.
            let merged_text = prev.matched_text.or(matched_text);
            // Keep first refuse reason when both soft-refuse the path.
            let merged_reason = prev.refuse_reason.or(refuse_reason);
            e.insert(ReplaceMatchMeta {
                mode: merged,
                score: merged_score,
                match_count: prev.match_count.saturating_add(match_count),
                matched_text: merged_text,
                // Successful write clears soft-refuse reason.
                refuse_reason: if prev.match_count.saturating_add(match_count) > 0 {
                    None
                } else {
                    merged_reason
                },
            });
        }
    }
}

/// Execute a replace operation within a transaction.
pub(crate) fn execute_replace_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    crate::verbose!(
        "replace_op: target={}, old_len={}, regex={}",
        op.declared_paths()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("<glob>"),
        if let Operation::Replace { old, .. } = op {
            old.len()
        } else {
            0
        },
        if let Operation::Replace { regex, .. } = op {
            regex
        } else {
            &false
        }
    );
    let Operation::Replace {
        glob,
        path,
        regex: regex_mode,
        old,
        new_text,
        nth,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        whole_line,
        range,
        before_context,
        after_context,
        unique,
        require_change,
        command_position,
        fuzzy,
        min_fuzzy_score,
        allow_absent_old,
        ..
    } = op
    else {
        anyhow::bail!("execute_replace_op called with non-Replace operation")
    };
    let allow_absent_old = *allow_absent_old;
    let regex_mode = *regex_mode;
    if let Some(min) = min_fuzzy_score
        && (!(0.0..=1.0).contains(min) || min.is_nan())
    {
        return Err(crate::exit::InvalidInputError {
            msg: format!(
                "min_fuzzy_score must be in 0.0..=1.0 (got {min}); leave None for no floor"
            ),
        }
        .into());
    }
    let word_boundary = matches!(
        op,
        Operation::Replace {
            word_boundary: true,
            ..
        }
    );
    let if_exists = matches!(
        op,
        Operation::Replace {
            if_exists: true,
            ..
        }
    );
    if *command_position
        && let Some(msg) = crate::ops::shell_token::command_position_combo_error(
            crate::ops::shell_token::CommandPositionIncompat {
                regex: regex_mode,
                case_insensitive: *case_insensitive,
                word_boundary,
                whole_line: *whole_line,
                multiline: *multiline,
                nth: nth.is_some(),
                insert_before: insert_before.is_some(),
                insert_after: insert_after.is_some(),
                before_context: before_context.is_some(),
                after_context: after_context.is_some(),
                fuzzy: *fuzzy,
            },
        )
    {
        return Err(crate::exit::InvalidInputError { msg: msg.into() }.into());
    }
    let use_regex = regex_mode || *case_insensitive || word_boundary;
    let compiled_re = compile_replace_regex(
        old,
        regex_mode,
        *case_insensitive,
        *multiline,
        word_boundary,
    )?;
    if range.is_some() && !*whole_line {
        return Err(crate::exit::InvalidInputError {
            msg: "range requires whole_line mode".into(),
        }
        .into());
    }
    let parsed_range = range
        .as_deref()
        .map(crate::ops::read::parse_line_range)
        .transpose()?;

    if let Some(p) = path {
        let file_path = tx.cwd.join(p);
        // CLI `replace --if-exists` soft-skips missing paths (`skipped[]`).
        // Plan/batch must match: without if_exists, missing paths stay hard
        // `not_found` (#1793). With if_exists, missing is a soft success so
        // multi-op batches do not abort on optional files (fixrealloop R12).
        let content = match read_file_content(tx.pending, tx.existed_before, &file_path) {
            Ok(c) => c,
            Err(e) if if_exists && crate::exit::is_io_not_found(&e) => return Ok(0),
            Err(e) => return Err(e),
        };
        // Per-file content so whole-line anchor detection is accurate (#1885).
        let replacement = replacement_text(
            old,
            new_text,
            insert_before,
            insert_after,
            use_regex,
            regex_mode,
            content,
        );
        let (replaced, match_count) = if *command_position {
            let to = new_text.as_deref().unwrap_or("");
            let (out, n) = crate::ops::shell_token::replace_command_position(content, old, to);
            (std::borrow::Cow::Owned(out), n)
        } else if *whole_line {
            replace_whole_lines(
                content,
                old,
                &replacement,
                compiled_re.as_ref(),
                *nth,
                parsed_range,
            )
        } else {
            replace_content(content, old, &replacement, compiled_re.as_ref(), *nth)
        };
        if *unique && match_count > 1 {
            return Err(crate::exit::AmbiguousError {
                msg: format!(
                    "ambiguous match: pattern {:?} matches {} times in {}; use --nth or add context to disambiguate",
                    crate::fallback::truncate_str(old, 60),
                    match_count,
                    p
                ),
            }
            .into());
        }
        if match_count > 0 {
            // When there are multiple exact matches and context is provided
            // (but no nth), use context to disambiguate instead of replacing all.
            // If context does not select a unique match, fail closed as ambiguous
            // (do not fall through to replace-all; fixrealloop 2026-07-15).
            if match_count > 1
                && nth.is_none()
                && !*whole_line
                && !regex_mode
                && (before_context.is_some() || after_context.is_some())
            {
                if let Some(target_offset) = context_filtered_offset(
                    content,
                    old,
                    before_context.as_deref(),
                    after_context.as_deref(),
                ) {
                    let new_content = format!(
                        "{}{}{}",
                        &content[..target_offset],
                        replacement,
                        &content[target_offset + old.len()..],
                    );
                    tx.write_file(&file_path, new_content);
                    record_replace_match(tx, &file_path, MatchMode::Anchored, None, 1, None);
                    return Ok(1);
                }
                return Err(crate::exit::AmbiguousError {
                    msg: format!(
                        "ambiguous match: pattern {:?} matches {} times in {}; context did not select a unique occurrence (use --nth or stronger before/after-context)",
                        crate::fallback::truncate_str(old, 60),
                        match_count,
                        p
                    ),
                }
                .into());
            }
            let owned = replaced.into_owned();
            tx.write_file(&file_path, owned);
            record_replace_match(tx, &file_path, MatchMode::Exact, None, match_count, None);
            Ok(match_count)
        } else if let Some((n, total)) = (*nth).and_then(|n| {
            // nth past last match: applied count is 0 but pattern may still match.
            // Do not report soft no_matches (agents confuse this with missing pattern).
            let total = crate::ops::replace::count_nth_candidates(
                content,
                old,
                compiled_re.as_ref(),
                *whole_line,
                parsed_range,
            );
            if total > 0 && n > total {
                Some((n, total))
            } else {
                None
            }
        }) {
            Err(crate::exit::InvalidInputError {
                msg: format!(
                    "nth {n} is out of range in {p}: pattern matches {total} time{}",
                    if total == 1 { "" } else { "s" }
                ),
            }
            .into())
        } else if !regex_mode && (*fuzzy || before_context.is_some() || after_context.is_some()) {
            // Tier 3: fuzzy and/or context fallback when exact match fails (#1668).
            match crate::fallback::resolve_with_fallback_skip_exact(
                content,
                old,
                before_context.as_deref(),
                after_context.as_deref(),
                // Primary path already applied word_boundary exact; bare find
                // would re-accept substrings rejected by \b (#1755).
                word_boundary,
            ) {
                Ok(anchor) => {
                    let (mode, default_score) =
                        crate::api::match_mode_from_strategy(anchor.strategy);
                    let score = anchor.score.or(default_score);
                    // Reject weak fuzzy matches when host set a floor (#1687).
                    if mode == MatchMode::Fuzzy
                        && let Some(min) = min_fuzzy_score
                        && crate::fallback::fuzzy_fails_min_floor(score, *min)
                    {
                        let actual = score
                            .map(|s| format!("{s:.3}"))
                            .unwrap_or_else(|| "none".into());
                        tx.replace_hint = Some(format!(
                            "fuzzy match score {actual} below min_fuzzy_score {min} for {:?}",
                            crate::fallback::truncate_str(old, 60),
                        ));
                        // Soft zero: still honor require_change below (#1748).
                        // Record candidate so multi-op partial success can list refused[].
                        record_replace_match_with_reason(
                            tx,
                            &file_path,
                            mode,
                            score,
                            0,
                            Some(anchor.matched_text.clone()),
                            Some("below_min_fuzzy_score"),
                        );
                    } else if crate::fallback::should_refuse_fuzzy_absent_old(
                        *fuzzy,
                        mode == MatchMode::Fuzzy,
                        allow_absent_old,
                    ) {
                        // #1758: exact old was absent (primary miss); do not
                        // rewrite a different live span without opt-in.
                        tx.replace_hint = Some(crate::fallback::fuzzy_absent_old_refuse_message(
                            old,
                            &anchor.matched_text,
                            score,
                        ));
                        // Surface candidate honesty without counting a write match.
                        record_replace_match_with_reason(
                            tx,
                            &file_path,
                            mode,
                            score,
                            0,
                            Some(anchor.matched_text.clone()),
                            Some("exact_old_absent"),
                        );
                    } else {
                        let to_text = if let Some(ib) = insert_before {
                            let ib = normalize_line_insert(
                                content,
                                &anchor.matched_text,
                                ib,
                                InsertSide::Before,
                            );
                            format!("{}{}", ib, anchor.matched_text)
                        } else if let Some(ia) = insert_after {
                            let ia = normalize_line_insert(
                                content,
                                &anchor.matched_text,
                                ia,
                                InsertSide::After,
                            );
                            format!("{}{}", anchor.matched_text, ia)
                        } else {
                            new_text.as_deref().unwrap_or("").to_string()
                        };
                        let new_content = format!(
                            "{}{}{}",
                            &content[..anchor.start_offset],
                            to_text,
                            &content[anchor.start_offset + anchor.matched_text.len()..]
                        );
                        tx.write_file(&file_path, new_content);
                        record_replace_match(
                            tx,
                            &file_path,
                            mode,
                            score,
                            1,
                            Some(anchor.matched_text.clone()),
                        );
                        tx.replace_hint = Some(format!(
                            "fallback matched via {:?} strategy in {}",
                            anchor.strategy, p,
                        ));
                        return Ok(1);
                    }
                }
                Err(edit_error) => {
                    tx.replace_hint = Some(edit_error.message.clone());
                }
            }
            if *require_change && !if_exists {
                let msg = tx
                    .replace_hint
                    .clone()
                    .unwrap_or_else(|| format!("no matches for {old:?} in {p}"));
                return Err(crate::exit::NoMatchError { msg }.into());
            }
            // Soft miss: keep plan success for other ops, but surface path in
            // refused[] so agents do not treat multi-op ok as full coverage.
            if !if_exists {
                record_soft_no_match(tx, &file_path);
            }
            Ok(0)
        } else {
            if !regex_mode {
                // Tier 1: Provide "did you mean?" hints for literal no-match.
                let similar = crate::fallback::find_similar_targets(content, old, 3);
                if !similar.is_empty() {
                    tx.replace_hint = Some(format!(
                        "no matches for '{}' in {} (did you mean: {}?)",
                        crate::fallback::truncate_str(old, 60),
                        p,
                        similar.join(", ")
                    ));
                }
            }
            if *require_change && !if_exists {
                let msg = tx
                    .replace_hint
                    .clone()
                    .unwrap_or_else(|| format!("no matches for {old:?} in {p}"));
                return Err(crate::exit::NoMatchError { msg }.into());
            }
            if !if_exists {
                record_soft_no_match(tx, &file_path);
            }
            Ok(0)
        }
    } else if let Some(pattern) = glob {
        let matcher = Glob::new(pattern)?.compile_matcher();
        let matches_pattern = |path: &Path| {
            matcher.is_match(path)
                || path.file_name().is_some_and(|name| matcher.is_match(name))
                || path.strip_prefix(tx.cwd).ok().is_some_and(|relative| {
                    !relative.as_os_str().is_empty()
                        && (matcher.is_match(relative)
                            || relative
                                .file_name()
                                .is_some_and(|name| matcher.is_match(name)))
                })
        };
        let mut total_matches = 0usize;
        let mut candidate_paths = Vec::new();
        let mut seen_paths = HashSet::new();

        let walker = WalkBuilder::new(tx.cwd).build();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let file_path = entry.path().to_path_buf();
            if matches_pattern(&file_path) && seen_paths.insert(file_path.clone()) {
                candidate_paths.push(file_path);
            }
        }

        for pending_path in tx.pending.keys() {
            if !pending_path.starts_with(tx.cwd)
                || pending_path.exists()
                || tx.deletions.contains(pending_path)
                || !matches_pattern(pending_path)
            {
                continue;
            }
            if seen_paths.insert(pending_path.clone()) {
                candidate_paths.push(pending_path.clone());
            }
        }

        // Defense-in-depth: validate glob-expanded paths against PathGuard.
        // WalkBuilder doesn't follow symlinks by default, but this catches edge
        // cases where a path might escape the containment boundary (#1361).
        if let Some(guard) = tx.guard {
            for path in &candidate_paths {
                let rel = path.strip_prefix(tx.cwd).unwrap_or(path).to_string_lossy();
                guard.check_path(&rel)?;
            }
        }

        for file_path in candidate_paths {
            match read_and_probe(tx.pending, tx.existed_before, &file_path) {
                Ok(false) => continue, // binary file, skip
                Ok(true) => {}
                Err(e) => {
                    if !tx.structured && !tx.quiet {
                        eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                    }
                    continue;
                }
            }
            let content = tx
                .pending
                .get(&file_path)
                .map(|(_, c)| c.clone())
                .expect("read_and_probe guarantees entry exists in pending");
            let replacement = replacement_text(
                old,
                new_text,
                insert_before,
                insert_after,
                use_regex,
                regex_mode,
                &content,
            );
            let (replaced, match_count) = if *command_position {
                let to = new_text.as_deref().unwrap_or("");
                let (out, n) = crate::ops::shell_token::replace_command_position(&content, old, to);
                (std::borrow::Cow::Owned(out), n)
            } else if *whole_line {
                replace_whole_lines(
                    &content,
                    old,
                    &replacement,
                    compiled_re.as_ref(),
                    *nth,
                    parsed_range,
                )
            } else {
                replace_content(&content, old, &replacement, compiled_re.as_ref(), *nth)
            };
            if *unique && match_count > 1 {
                let rel = file_path
                    .strip_prefix(tx.cwd)
                    .unwrap_or(&file_path)
                    .display();
                return Err(crate::exit::AmbiguousError {
                    msg: format!(
                        "ambiguous match: pattern {:?} matches {} times in {}; use --nth or add context to disambiguate",
                        crate::fallback::truncate_str(old, 60),
                        match_count,
                        rel
                    ),
                }
                .into());
            }
            if match_count > 0 {
                // Multi-match context disambiguation (parity with single-path).
                if match_count > 1
                    && nth.is_none()
                    && !*whole_line
                    && !regex_mode
                    && (before_context.is_some() || after_context.is_some())
                {
                    if let Some(target_offset) = context_filtered_offset(
                        &content,
                        old,
                        before_context.as_deref(),
                        after_context.as_deref(),
                    ) {
                        let new_content = format!(
                            "{}{}{}",
                            &content[..target_offset],
                            replacement,
                            &content[target_offset + old.len()..],
                        );
                        tx.write_file(&file_path, new_content);
                        record_replace_match(tx, &file_path, MatchMode::Anchored, None, 1, None);
                        total_matches += 1;
                        continue;
                    }
                    let rel = file_path
                        .strip_prefix(tx.cwd)
                        .unwrap_or(&file_path)
                        .display();
                    return Err(crate::exit::AmbiguousError {
                        msg: format!(
                            "ambiguous match: pattern {:?} matches {} times in {}; context did not select a unique occurrence (use --nth or stronger before/after-context)",
                            crate::fallback::truncate_str(old, 60),
                            match_count,
                            rel
                        ),
                    }
                    .into());
                }
                total_matches += match_count;
                tx.write_file(&file_path, replaced.into_owned());
                record_replace_match(tx, &file_path, MatchMode::Exact, None, match_count, None);
            } else if let Some((n, total)) = (*nth).and_then(|n| {
                let total = crate::ops::replace::count_nth_candidates(
                    &content,
                    old,
                    compiled_re.as_ref(),
                    *whole_line,
                    parsed_range,
                );
                if total > 0 && n > total {
                    Some((n, total))
                } else {
                    None
                }
            }) {
                let rel = file_path
                    .strip_prefix(tx.cwd)
                    .unwrap_or(&file_path)
                    .display();
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "nth {n} is out of range in {rel}: pattern matches {total} time{}",
                        if total == 1 { "" } else { "s" }
                    ),
                }
                .into());
            } else if !regex_mode && (*fuzzy || before_context.is_some() || after_context.is_some())
            {
                // Fuzzy/context fallback for glob paths (parity with #1668 path arm).
                match crate::fallback::resolve_with_fallback_skip_exact(
                    &content,
                    old,
                    before_context.as_deref(),
                    after_context.as_deref(),
                    // Primary path already applied word_boundary exact; bare find
                    // would re-accept substrings rejected by \b (#1755).
                    word_boundary,
                ) {
                    Ok(anchor) => {
                        let (mode, default_score) =
                            crate::api::match_mode_from_strategy(anchor.strategy);
                        let score = anchor.score.or(default_score);
                        if mode == MatchMode::Fuzzy
                            && let Some(min) = min_fuzzy_score
                            && crate::fallback::fuzzy_fails_min_floor(score, *min)
                        {
                            // Parity with single-path arm (#1745 / AI findings):
                            // surface the floor rejection so callers can diagnose
                            // why no matches applied under a multi-file glob.
                            let actual = score
                                .map(|s| format!("{s:.3}"))
                                .unwrap_or_else(|| "none".into());
                            tx.replace_hint = Some(format!(
                                "fuzzy match score {actual} below min_fuzzy_score {min} for {:?}",
                                crate::fallback::truncate_str(old, 60),
                            ));
                            record_replace_match_with_reason(
                                tx,
                                &file_path,
                                mode,
                                score,
                                0,
                                Some(anchor.matched_text.clone()),
                                Some("below_min_fuzzy_score"),
                            );
                            // Skip this file; keep scanning (require_change after loop).
                            continue;
                        }
                        if crate::fallback::should_refuse_fuzzy_absent_old(
                            *fuzzy,
                            mode == MatchMode::Fuzzy,
                            allow_absent_old,
                        ) {
                            // #1758: fail closed when exact old absent.
                            tx.replace_hint =
                                Some(crate::fallback::fuzzy_absent_old_refuse_message(
                                    old,
                                    &anchor.matched_text,
                                    score,
                                ));
                            record_replace_match_with_reason(
                                tx,
                                &file_path,
                                mode,
                                score,
                                0,
                                Some(anchor.matched_text.clone()),
                                Some("exact_old_absent"),
                            );
                            continue;
                        }
                        let to_text = if let Some(ib) = insert_before {
                            format!("{}{}", ib, anchor.matched_text)
                        } else if let Some(ia) = insert_after {
                            format!("{}{}", anchor.matched_text, ia)
                        } else {
                            new_text.as_deref().unwrap_or("").to_string()
                        };
                        let new_content = format!(
                            "{}{}{}",
                            &content[..anchor.start_offset],
                            to_text,
                            &content[anchor.start_offset + anchor.matched_text.len()..]
                        );
                        tx.write_file(&file_path, new_content);
                        record_replace_match(
                            tx,
                            &file_path,
                            mode,
                            score,
                            1,
                            Some(anchor.matched_text.clone()),
                        );
                        // Parity with single-path arm: surface which strategy applied.
                        tx.replace_hint = Some(format!(
                            "fallback matched via {:?} strategy in {}",
                            anchor.strategy,
                            crate::files::relative_display(&file_path, tx.cwd).to_string_lossy()
                        ));
                        total_matches += 1;
                    }
                    Err(edit_error) => {
                        // Parity with single-path arm: keep diagnostics for
                        // require_change / replace_no_matches surfaces.
                        tx.replace_hint = Some(edit_error.message.clone());
                    }
                }
            } else if !regex_mode {
                // Exact miss, no fuzzy/context: did-you-mean for this file
                // (last non-empty similar list wins; used if no file matched).
                let similar = crate::fallback::find_similar_targets(&content, old, 3);
                if !similar.is_empty() {
                    let rel = crate::files::relative_display(&file_path, tx.cwd).to_string_lossy();
                    tx.replace_hint = Some(format!(
                        "no matches for '{}' in {} (did you mean: {}?)",
                        crate::fallback::truncate_str(old, 60),
                        rel,
                        similar.join(", ")
                    ));
                }
            }
        }
        if total_matches == 0 && *require_change && !if_exists {
            let msg = tx
                .replace_hint
                .clone()
                .unwrap_or_else(|| format!("no matches for {old:?} (glob {pattern})"));
            return Err(crate::exit::NoMatchError { msg }.into());
        }
        Ok(total_matches)
    } else {
        Err(crate::exit::InvalidInputError {
            msg: "replace operation requires either 'path' or 'glob'".into(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Operation;
    use crate::tx::TxStateFixture;
    use tempfile::TempDir;

    #[test]
    fn replace_literal_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "hello".into(),
            new_text: Some("goodbye".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "goodbye world");
    }

    #[test]
    fn replace_no_match_returns_zero() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "nonexistent".into(),
            new_text: Some("replacement".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn replace_missing_path_if_exists_soft_skips() {
        // CLI --if-exists soft-skips missing paths; plan/batch must match
        // (fixrealloop R12: batch `replace missing.toml … --if-exists` aborted).
        let dir = TempDir::new().unwrap();
        let op = Operation::Replace {
            path: Some("missing.txt".into()),
            glob: None,
            regex: false,
            old: "a".into(),
            new_text: Some("b".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: true,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).expect("if_exists soft-skip");
        assert_eq!(count, 0);
    }

    #[test]
    fn replace_missing_path_without_if_exists_errors() {
        let dir = TempDir::new().unwrap();
        let op = Operation::Replace {
            path: Some("missing.txt".into()),
            glob: None,
            regex: false,
            old: "a".into(),
            new_text: Some("b".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).expect_err("missing without if_exists");
        assert!(
            crate::exit::is_io_not_found(&err),
            "expected NotFound, got: {err:#}"
        );
    }

    #[test]
    fn replace_regex_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "foo123bar").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: true,
            old: r"\d+".into(),
            new_text: Some("NUM".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "fooNUMbar");
    }

    #[test]
    fn replace_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "hello".into(),
            new_text: Some("hi".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: true,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "hi World");
    }

    #[test]
    fn replace_missing_path_and_glob_errors() {
        let dir = TempDir::new().unwrap();

        let op = Operation::Replace {
            path: None,
            glob: None,
            regex: false,
            old: "x".into(),
            new_text: Some("y".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let result = execute_replace_op(&op, &mut tx);
        let err = result.expect_err("expected error when path and glob are both None");
        assert!(
            err.to_string().contains("'path' or 'glob'"),
            "unexpected error: {err}"
        );
        assert!(
            crate::exit::is_invalid_input(&err),
            "missing path/glob should be InvalidInputError: {err}"
        );
    }

    #[test]
    fn replace_glob_matches_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old value").unwrap();
        std::fs::write(dir.path().join("b.txt"), "old value").unwrap();
        std::fs::write(dir.path().join("c.rs"), "old value").unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*.txt".into()),
            regex: false,
            old: "old".into(),
            new_text: Some("new".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 2); // a.txt and b.txt matched
        // c.rs should not be modified
        assert!(!f.pending.contains_key(&dir.path().join("c.rs")));
    }

    #[test]
    fn replace_insert_before_with_fallback_preserves_matched_text() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "fn process(input: Vec<u8>) {\n}\n").unwrap();

        // `from` is stale (Vec<i32> instead of Vec<u8>), so exact match fails.
        // Fallback should insert_before the matched text, not delete it.
        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "fn process(input: Vec<i32>) {".into(),
            new_text: None,
            nth: None,
            insert_before: Some("/// Process input.\n".into()),
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: Some("fn process".into()),
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        let result = &f.pending[&file].1;
        assert!(
            result.contains("fn process(input: Vec<u8>)"),
            "original function signature must be preserved, got: {result}"
        );
        assert!(
            result.contains("/// Process input."),
            "insert_before text must be present, got: {result}"
        );
    }

    #[test]
    fn replace_context_disambiguates_multiple_exact_matches() {
        // #1244: before_context/after_context should disambiguate when
        // the old text matches multiple times.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.ini");
        std::fs::write(
            &file,
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        )
        .unwrap();

        // Use after_context to target the [database] occurrence (port = 5432 follows it).
        let op = Operation::Replace {
            path: Some("config.ini".into()),
            glob: None,
            regex: false,
            old: "host = localhost".into(),
            new_text: Some("host = db.primary".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: Some("port = 5432".into()),
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1, "should replace exactly one occurrence");
        let result = &f.pending[&file].1;
        assert!(
            result.contains("[database]\nhost = db.primary"),
            "database section should be updated: {result}"
        );
        assert!(
            result.contains("[cache]\nhost = localhost"),
            "cache section should be unchanged: {result}"
        );
    }

    /// Context that scores no match must not fall through to replace-all.
    #[test]
    fn replace_context_fail_closed_when_no_score() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "alpha foo\nbeta foo\n").unwrap();
        let op = Operation::Replace {
            path: Some("a.txt".into()),
            glob: None,
            regex: false,
            old: "foo".into(),
            new_text: Some("X".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            // "zzz" is nowhere near either match → score 0 → None.
            before_context: Some("zzz".into()),
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        drop(tx);
        let msg = err.to_string();
        assert!(
            msg.contains("context did not select") || msg.contains("ambiguous"),
            "want fail-closed ambiguous, got: {msg}"
        );
        if let Some((_, content)) = f.pending.get(&file) {
            assert_eq!(
                content, "alpha foo\nbeta foo\n",
                "must not rewrite either match when context fails: {content:?}"
            );
        }
    }

    #[test]
    fn replace_before_context_disambiguates() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.ini");
        std::fs::write(
            &file,
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: Some("config.ini".into()),
            glob: None,
            regex: false,
            old: "host = localhost".into(),
            new_text: Some("host = db.internal".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: Some("[database]".into()),
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        let result = &f.pending[&file].1;
        assert!(
            result.contains("[database]\nhost = db.internal"),
            "database section should be updated: {result}"
        );
        assert!(
            result.contains("[cache]\nhost = localhost"),
            "cache section should be unchanged: {result}"
        );
    }

    #[test]
    fn replace_glob_skips_binary_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("text.txt"), "hello world").unwrap();
        std::fs::write(dir.path().join("binary.dat"), b"hello\x00world").unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*".into()),
            regex: false,
            old: "hello".into(),
            new_text: Some("goodbye".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1); // only text.txt matched
        assert_eq!(f.pending[&dir.path().join("text.txt")].1, "goodbye world");
        assert!(
            !f.pending.contains_key(&dir.path().join("binary.dat")),
            "binary file should be skipped, not loaded into pending"
        );
    }

    #[test]
    fn replace_range_without_whole_line_is_rejected() {
        // Regression: range was silently ignored when whole_line was false.
        // Now the tx engine validates this, matching the CLI behavior.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "line".into(),
            new_text: Some("LINE".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: Some("1:2".into()),
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line"),
            "should reject range without whole_line: {err}"
        );
    }
    #[test]
    fn command_position_rewrites_sudo_pip() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("install.sh");
        std::fs::write(&file, "sudo pip install x\nuv pip install\n").unwrap();
        let op = Operation::Replace {
            path: Some("install.sh".into()),
            glob: None,
            regex: false,
            old: "pip".into(),
            new_text: Some("uv".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: true,
            command_position: true,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "sudo uv install x\nuv pip install\n");
    }

    #[test]
    fn require_change_bails_on_zero_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hello\n").unwrap();
        let op = Operation::Replace {
            path: Some("f.txt".into()),
            glob: None,
            regex: false,
            old: "missing".into(),
            new_text: Some("x".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        assert!(err.to_string().contains("no matches"), "{err}");
    }

    #[test]
    fn replace_min_fuzzy_score_rejects_weak_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("src.rs");
        std::fs::write(
            &file,
            "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n",
        )
        .unwrap();

        // Floor above any real similarity score so the match is always rejected.
        let op = Operation::Replace {
            path: Some("src.rs".into()),
            glob: None,
            regex: false,
            old: "fn process_requets(data: &str) -> Result<()> {".into(),
            new_text: Some("REPLACED".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: Some(1.0),
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let hint = {
            let mut tx = f.state(dir.path());
            let count = execute_replace_op(&op, &mut tx).unwrap();
            assert_eq!(count, 0, "score floor must reject weak fuzzy");
            tx.replace_hint.clone()
        };
        assert!(
            f.write_targets.is_empty(),
            "rejected fuzzy must not stage a write"
        );
        assert!(
            hint.as_deref()
                .is_some_and(|h| h.contains("min_fuzzy_score")),
            "hint: {hint:?}"
        );
    }

    /// Path arm: floor reject with require_change must hard-fail (NoMatch), not
    /// soft Ok(0). Previously returned Ok(0) and skipped the require_change check.
    #[test]
    fn replace_min_fuzzy_score_require_change_bails() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("src.rs");
        std::fs::write(
            &file,
            "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: Some("src.rs".into()),
            glob: None,
            regex: false,
            old: "fn process_requets(data: &str) -> Result<()> {".into(),
            new_text: Some("REPLACED".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: Some(1.0),
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("min_fuzzy_score") || msg.contains("no matches"),
            "require_change after floor reject must surface NoMatch with hint: {msg}"
        );
        assert!(
            f.write_targets.is_empty(),
            "rejected fuzzy must not stage a write"
        );
    }

    /// Glob + require_change after floor reject must put the floor message in
    /// the NoMatch error (not a generic "no matches for … (glob …)").
    #[test]
    fn replace_glob_min_fuzzy_require_change_includes_hint() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("src.rs"),
            "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*.rs".into()),
            regex: false,
            old: "fn process_requets(data: &str) -> Result<()> {".into(),
            new_text: Some("REPLACED".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: Some(1.0),
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("min_fuzzy_score"),
            "glob require_change must surface floor hint, got: {msg}"
        );
    }

    /// Glob arm must set `replace_hint` when min_fuzzy_score rejects, same as
    /// the single-path arm (#1745 / GitHub AI findings parity).
    #[test]
    fn replace_glob_min_fuzzy_score_sets_replace_hint() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("src.rs");
        std::fs::write(
            &file,
            "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*.rs".into()),
            regex: false,
            old: "fn process_requets(data: &str) -> Result<()> {".into(),
            new_text: Some("REPLACED".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: Some(1.0),
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let hint = {
            let mut tx = f.state(dir.path());
            let count = execute_replace_op(&op, &mut tx).unwrap();
            assert_eq!(count, 0, "score floor must reject weak fuzzy on glob");
            tx.replace_hint.clone()
        };
        assert!(
            f.write_targets.is_empty(),
            "rejected fuzzy glob must not stage a write"
        );
        assert!(
            hint.as_deref()
                .is_some_and(|h| h.contains("min_fuzzy_score")),
            "glob floor rejection must set replace_hint: {hint:?}"
        );
    }

    #[test]
    fn replace_pure_fuzzy_without_context() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        std::fs::write(&file, "fn process_data() {}\n").unwrap();

        let op = Operation::Replace {
            path: Some("code.rs".into()),
            glob: None,
            regex: false,
            old: "fn proccess_data() {}".into(),
            new_text: Some("fn handle_data() {}".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: None,
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert!(
            f.pending[&file].1.contains("handle_data"),
            "pure fuzzy plan op must rewrite: {}",
            f.pending[&file].1
        );
        let meta = f.replace_match_meta.get(&file).expect("fuzzy meta");
        assert_eq!(meta.mode, crate::api::MatchMode::Fuzzy);
        assert!(meta.score.is_some(), "similarity fallback should set score");
        assert_eq!(meta.match_count, 1);
    }

    /// Plan/tx path: identifier typo must not nuke surrounding syntax (#1694).
    #[test]
    fn replace_fuzzy_identifier_typo_preserves_syntax() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("cfg.rs");
        std::fs::write(
            &file,
            "const CONFIGURATION_VALUE_PRIMARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: Some("cfg.rs".into()),
            glob: None,
            regex: false,
            old: "CONFIGURATION_VALUE_PRIMRY".into(),
            new_text: Some("CONFIGURATION_VALUE_SECONDARY".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: true,
            require_change: true,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: Some(0.80),
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        let out = &f.pending[&file].1;
        assert!(
            out.starts_with("const CONFIGURATION_VALUE_SECONDARY: i32 = 1;"),
            "tx fuzzy must keep syntax: {out}"
        );
        assert!(
            out.contains("CONFIGURATION_VALUE_PRIMARY"),
            "second occurrence left: {out}"
        );
        assert!(
            !out.lines().any(|l| l == "CONFIGURATION_VALUE_SECONDARY"),
            "must not bare-line replace: {out}"
        );
        let meta = f.replace_match_meta.get(&file).expect("fuzzy meta");
        assert_eq!(meta.mode, crate::api::MatchMode::Fuzzy);
    }

    #[test]
    fn replace_exact_records_match_mode_exact() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("t.txt");
        std::fs::write(&file, "hello world\n").unwrap();

        let op = Operation::Replace {
            path: Some("t.txt".into()),
            glob: None,
            regex: false,
            old: "hello".into(),
            new_text: Some("hi".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        assert_eq!(execute_replace_op(&op, &mut tx).unwrap(), 1);
        drop(tx);
        let meta = f.replace_match_meta.get(&file).expect("exact meta");
        assert_eq!(meta.mode, crate::api::MatchMode::Exact);
        assert!(meta.score.is_none());
        assert_eq!(meta.match_count, 1);
    }

    #[test]
    fn replace_glob_pure_fuzzy_without_context() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn process_data() {}\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "unrelated\n").unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("**/*.rs".into()),
            regex: false,
            old: "fn proccess_data() {}".into(),
            new_text: Some("fn handle_data() {}".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            unique: false,
            require_change: true,
            command_position: false,
            fuzzy: true,
            min_fuzzy_score: None,
            allow_absent_old: true,
        };
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        let hint = tx.replace_hint.clone();
        drop(tx);
        assert_eq!(count, 1, "glob fuzzy should rewrite one .rs file");
        let a = dir.path().join("a.rs");
        assert!(
            f.pending[&a].1.contains("handle_data"),
            "glob pure fuzzy must rewrite: {}",
            f.pending[&a].1
        );
        assert!(
            hint.as_deref()
                .is_some_and(|h| h.contains("fallback matched")),
            "glob success must set replace_hint like path arm: {hint:?}"
        );
    }

    /// Multi-op same path: fuzzy scores merge with min (worst confidence).
    #[test]
    fn record_replace_match_merges_min_fuzzy_score() {
        use crate::api::MatchMode;
        let dir = TempDir::new().unwrap();
        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let path = dir.path().join("a.txt");
        record_replace_match(
            &mut tx,
            &path,
            MatchMode::Fuzzy,
            Some(0.95),
            1,
            Some("first".into()),
        );
        record_replace_match(
            &mut tx,
            &path,
            MatchMode::Fuzzy,
            Some(0.72),
            1,
            Some("second".into()),
        );
        let meta = tx.replace_match_meta.get(&path).expect("merged meta");
        assert_eq!(meta.mode, MatchMode::Fuzzy);
        assert_eq!(
            meta.score,
            Some(0.72),
            "merged fuzzy score must be min, not first-wins"
        );
        assert_eq!(meta.match_count, 2);
        assert_eq!(
            meta.matched_text.as_deref(),
            Some("first"),
            "first non-null matched_text is kept"
        );
    }
}
