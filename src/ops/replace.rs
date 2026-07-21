use regex::Regex;

/// Build an optional compiled regex for replace operations.
///
/// Returns `Some(Regex)` when regex mode is active or case-insensitive
/// matching is requested (which requires escaping the literal pattern).
/// Returns `None` for plain literal, case-sensitive replacements.
pub fn compile_replace_regex(
    pattern: &str,
    regex_mode: bool,
    case_insensitive: bool,
    multiline: bool,
    word_boundary: bool,
) -> anyhow::Result<Option<Regex>> {
    if word_boundary && !regex_mode {
        // Escape the literal pattern and wrap with \b anchors.
        let escaped = regex::escape(pattern);
        let wb_pattern = format!("\\b{escaped}\\b");
        return Ok(Some(crate::bounded_regex_build(
            crate::bounded_regex_builder(&wb_pattern)
                .case_insensitive(case_insensitive)
                .multi_line(true)
                .dot_matches_new_line(multiline),
        )?));
    }
    if regex_mode {
        let effective = if word_boundary {
            format!("\\b(?:{pattern})\\b")
        } else {
            pattern.to_string()
        };
        Ok(Some(crate::bounded_regex_build(
            crate::bounded_regex_builder(&effective)
                .case_insensitive(case_insensitive)
                .multi_line(true)
                .dot_matches_new_line(multiline),
        )?))
    } else if case_insensitive {
        Ok(Some(crate::bounded_regex_build(
            crate::bounded_regex_builder(&regex::escape(pattern))
                .case_insensitive(true)
                .multi_line(true),
        )?))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceModeError {
    MissingMode,
    BothInsertModes,
    ToWithInsert,
}

pub fn validate_replace_mode(
    has_to: bool,
    has_insert_before: bool,
    has_insert_after: bool,
) -> Result<(), ReplaceModeError> {
    match (has_to, has_insert_before, has_insert_after) {
        (false, false, false) => Err(ReplaceModeError::MissingMode),
        (_, true, true) => Err(ReplaceModeError::BothInsertModes),
        (true, true, false) | (true, false, true) => Err(ReplaceModeError::ToWithInsert),
        _ => Ok(()),
    }
}

/// Errors from [`validate_replace_args`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceValidationError {
    EmptyPattern,
    NthZero,
    RangeRequiresWholeLine,
    WholeLineMultilineConflict,
    WholeLineInsertConflict,
    Mode(ReplaceModeError),
}

impl std::fmt::Display for ReplaceValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPattern => write!(f, "replace pattern must not be empty"),
            Self::NthZero => {
                write!(
                    f,
                    "nth must be >= 1 (1-based); use nth=1 for the first occurrence"
                )
            }
            Self::RangeRequiresWholeLine => write!(f, "range requires whole_line"),
            Self::WholeLineMultilineConflict => {
                write!(f, "whole_line and multiline cannot be combined")
            }
            Self::WholeLineInsertConflict => {
                write!(
                    f,
                    "whole_line cannot be combined with insert_before or insert_after (would drop non-matched line content)"
                )
            }
            Self::Mode(e) => match e {
                // Name CLI flags first so agents do not retry with --to (#1829).
                // Plan/MCP still accept field aliases new/to and insert_*.
                ReplaceModeError::MissingMode => {
                    write!(
                        f,
                        "one of --new, --insert-before, or --insert-after must be provided \
                         (plan fields: new/to, insert_before, insert_after); \
                         replacement text is not positional — use: replace OLD --new NEW path"
                    )
                }
                ReplaceModeError::BothInsertModes => {
                    write!(
                        f,
                        "--insert-before and --insert-after cannot be combined \
                         (plan fields: insert_before, insert_after)"
                    )
                }
                ReplaceModeError::ToWithInsert => {
                    write!(
                        f,
                        "--new cannot be combined with --insert-before or --insert-after \
                         (plan fields: new/to, insert_before, insert_after)"
                    )
                }
            },
        }
    }
}

/// Parameters for replace argument validation.
pub struct ReplaceValidationParams<'a> {
    pub pattern: &'a str,
    pub has_to: bool,
    pub has_insert_before: bool,
    pub has_insert_after: bool,
    pub nth: Option<usize>,
    pub whole_line: bool,
    pub multiline: bool,
    pub has_range: bool,
}

/// Validate replace arguments. Shared by CLI and MCP entry points.
pub fn validate_replace_args(
    p: &ReplaceValidationParams<'_>,
) -> Result<(), ReplaceValidationError> {
    if p.pattern.is_empty() {
        return Err(ReplaceValidationError::EmptyPattern);
    }
    if p.nth == Some(0) {
        return Err(ReplaceValidationError::NthZero);
    }
    if p.has_range && !p.whole_line {
        return Err(ReplaceValidationError::RangeRequiresWholeLine);
    }
    if p.whole_line && p.multiline {
        return Err(ReplaceValidationError::WholeLineMultilineConflict);
    }
    if p.whole_line && (p.has_insert_before || p.has_insert_after) {
        return Err(ReplaceValidationError::WholeLineInsertConflict);
    }
    validate_replace_mode(p.has_to, p.has_insert_before, p.has_insert_after)
        .map_err(ReplaceValidationError::Mode)
}

/// Which side of the match an insert lands on (for line-oriented default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertSide {
    Before,
    After,
}

/// Normalize an insert payload so agent-style inserts land on their own line.
///
/// Default for all insert_before / insert_after paths (#1885). Byte-exact
/// mid-line inserts (e.g. `"X"` after `"foo"` inside a line) are unchanged.
///
/// Rules (from Bline host glue, now product default):
/// - insert_after: prepend `\\n` when the payload looks like a new line, or
///   every occurrence of `anchor` is alone on its line, unless the payload
///   already starts with `\\n` or the anchor ends with `\\n`.
/// - insert_before: append `\\n` under the same conditions (so the anchor
///   stays on the next line), unless the payload already ends with `\\n`
///   or the anchor starts with `\\n`.
pub fn normalize_line_insert(
    file_content: &str,
    anchor: &str,
    insert_content: &str,
    side: InsertSide,
) -> String {
    match side {
        InsertSide::After => {
            if insert_content.starts_with('\n') || anchor.ends_with('\n') {
                return insert_content.to_string();
            }
            if looks_like_new_line_payload(insert_content)
                || anchor_is_whole_line(file_content, anchor)
            {
                format!("\n{insert_content}")
            } else {
                insert_content.to_string()
            }
        }
        InsertSide::Before => {
            if insert_content.ends_with('\n') || anchor.starts_with('\n') {
                return insert_content.to_string();
            }
            if looks_like_new_line_payload(insert_content)
                || anchor_is_whole_line(file_content, anchor)
            {
                format!("{insert_content}\n")
            } else {
                insert_content.to_string()
            }
        }
    }
}

fn looks_like_new_line_payload(insert_content: &str) -> bool {
    let trimmed = insert_content.trim_start_matches([' ', '\t']);
    insert_content.starts_with([' ', '\t'])
        || trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || insert_content.contains('\n')
}

/// True when every occurrence of `anchor` is alone on its line (bounded by
/// newlines / file edges). Empty content or empty anchor → false.
///
/// Accepts LF, CRLF, and bare CR as line boundaries so whole-line bare
/// inserts on Windows-style files still line-orient (#1885 follow-up).
pub fn anchor_is_whole_line(file_content: &str, anchor: &str) -> bool {
    if anchor.is_empty() || file_content.is_empty() {
        return false;
    }
    let bytes = file_content.as_bytes();
    let mut any = false;
    for (i, _) in file_content.match_indices(anchor) {
        any = true;
        let before_ok = i == 0 || is_line_boundary_byte(bytes[i - 1]);
        let end = i + anchor.len();
        let after_ok =
            end == file_content.len() || bytes.get(end).copied().is_some_and(is_line_boundary_byte);
        if !(before_ok && after_ok) {
            return false;
        }
    }
    any
}

#[inline]
fn is_line_boundary_byte(b: u8) -> bool {
    b == b'\n' || b == b'\r'
}

/// Build the replacement string for replace / insert modes.
///
/// `file_content` is the file (or buffer) being edited; it is used to decide
/// line-oriented insert normalization (#1885). Pass `""` only in pure unit
/// tests that do not exercise whole-line-anchor detection.
pub fn replacement_text(
    from: &str,
    to: &Option<String>,
    insert_before: &Option<String>,
    insert_after: &Option<String>,
    use_match_anchor: bool,
    regex_mode: bool,
    file_content: &str,
) -> String {
    let anchor = if use_match_anchor { "${0}" } else { from };

    // When a regex is compiled internally (case_insensitive / word_boundary)
    // but the user did NOT request regex mode, dollar signs in user-provided
    // replacement text must be escaped so caps.expand() treats them literally.
    let needs_escape = use_match_anchor && !regex_mode;

    if let Some(text) = insert_before {
        // Normalize against the literal `from` pattern (not ${0}) so whole-line
        // detection sees the real anchor text in the file.
        let normalized = normalize_line_insert(file_content, from, text, InsertSide::Before);
        let safe = if needs_escape {
            normalized.replace('$', "$$")
        } else {
            normalized
        };
        return format!("{safe}{anchor}");
    }

    if let Some(text) = insert_after {
        let normalized = normalize_line_insert(file_content, from, text, InsertSide::After);
        let safe = if needs_escape {
            normalized.replace('$', "$$")
        } else {
            normalized
        };
        return format!("{anchor}{safe}");
    }

    let raw = to.clone().unwrap_or_default();
    if needs_escape {
        raw.replace('$', "$$")
    } else {
        raw
    }
}

fn expand_regex_replacement(caps: &regex::Captures<'_>, replacement: &str) -> String {
    let mut expanded = String::new();
    caps.expand(replacement, &mut expanded);
    expanded
}

/// Count non-overlapping matches of `from` / `compiled_re` in `content`.
///
/// Used for agent-honest errors when `--nth` is past the last match (the
/// replace path returns applied count 0, which is otherwise indistinguishable
/// from a true no-match).
pub fn count_content_matches(content: &str, from: &str, compiled_re: Option<&Regex>) -> usize {
    match compiled_re {
        Some(re) => {
            let content_len = content.len();
            re.find_iter(content)
                .filter(|m| !(m.start() == content_len && m.end() == content_len))
                .count()
        }
        None => {
            if from.is_empty() {
                return 0;
            }
            content.match_indices(from).count()
        }
    }
}

/// Count lines that match for whole-line replace (one match per matching line).
///
/// Differs from [`count_content_matches`] when a line contains the pattern more
/// than once: whole-line mode still counts that line once. Optional `range` is
/// 1-based inclusive, matching [`replace_whole_lines`].
pub fn count_whole_line_matches(
    content: &str,
    from: &str,
    compiled_re: Option<&Regex>,
    range: Option<(usize, Option<usize>)>,
) -> usize {
    content
        .lines()
        .enumerate()
        .filter(|(i, line)| {
            let line_num = i + 1;
            let in_range = match range {
                Some((start, Some(end))) => line_num >= start && line_num <= end,
                Some((start, None)) => line_num >= start,
                None => true,
            };
            if !in_range {
                return false;
            }
            if let Some(re) = compiled_re {
                re.is_match(line)
            } else if from.is_empty() {
                false
            } else {
                line.contains(from)
            }
        })
        .count()
}

/// Matches available for `--nth` under the active replace mode.
///
/// `range` only affects whole-line mode (same as replace).
pub fn count_nth_candidates(
    content: &str,
    from: &str,
    compiled_re: Option<&Regex>,
    whole_line: bool,
    range: Option<(usize, Option<usize>)>,
) -> usize {
    if whole_line {
        count_whole_line_matches(content, from, compiled_re, range)
    } else {
        count_content_matches(content, from, compiled_re)
    }
}

pub fn replace_content<'a>(
    content: &'a str,
    from: &str,
    to: &str,
    compiled_re: Option<&Regex>,
    nth: Option<usize>,
) -> (std::borrow::Cow<'a, str>, usize) {
    use std::borrow::Cow;
    match (nth, compiled_re) {
        (Some(n), Some(re)) => {
            let content_len = content.len();
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            for caps in re.captures_iter(content) {
                // Skip zero-length matches at the very end of the content.
                // With multi_line(true), patterns like ^$ produce a trailing
                // match after the final newline that search (line-by-line) does
                // not see. Dropping it keeps nth consistent with search.
                if let Some(m) = caps.get(0)
                    && m.start() == content_len
                    && m.end() == content_len
                {
                    continue;
                }
                count += 1;
                if count != n {
                    continue;
                }
                let Some(m) = caps.get(0) else {
                    return (Cow::Borrowed(content), 0);
                };
                result.push_str(&content[..m.start()]);
                result.push_str(&expand_regex_replacement(&caps, to));
                result.push_str(&content[m.end()..]);
                return (Cow::Owned(result), 1);
            }
            (Cow::Borrowed(content), 0)
        }
        (Some(n), None) => {
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            for (start, _) in content.match_indices(from) {
                count += 1;
                if count != n {
                    continue;
                }

                result.push_str(&content[..start]);
                result.push_str(to);
                result.push_str(&content[start + from.len()..]);
                return (Cow::Owned(result), 1);
            }
            (Cow::Borrowed(content), 0)
        }
        (None, Some(re)) => {
            let content_len = content.len();
            let mut count = 0usize;
            let replaced = re.replace_all(content, |caps: &regex::Captures| {
                // Skip zero-length matches at the very end of the content.
                // With multi_line(true), patterns like ^$ produce a trailing
                // match after the final newline that search (line-by-line) does
                // not see. Dropping it keeps replace consistent with search.
                if let Some(m) = caps.get(0)
                    && m.start() == content_len
                    && m.end() == content_len
                {
                    return String::new();
                }
                count += 1;
                expand_regex_replacement(caps, to)
            });
            match replaced {
                Cow::Borrowed(_) => (Cow::Borrowed(content), 0),
                Cow::Owned(s) => (Cow::Owned(s), count),
            }
        }
        (None, None) => {
            // Single-pass using SIMD-accelerated memchr::memmem::Finder.
            // All callers validate `from` is non-empty via validate_replace_args().
            debug_assert!(!from.is_empty(), "replace_content called with empty `from`");
            let finder = memchr::memmem::Finder::new(from.as_bytes());
            let bytes = content.as_bytes();
            let mut result = String::with_capacity(content.len());
            let mut count = 0usize;
            let mut last = 0;
            while let Some(pos) = finder.find(&bytes[last..]) {
                let abs = last + pos;
                result.push_str(&content[last..abs]);
                result.push_str(to);
                last = abs + from.len();
                count += 1;
            }
            if count == 0 {
                return (Cow::Borrowed(content), 0);
            }
            result.push_str(&content[last..]);
            (Cow::Owned(result), count)
        }
    }
}

/// Score how well a content fragment matches a context fragment.
///
/// Uses Jaro-Winkler (full-string similarity) and substring containment.
/// Agents often pass short anchors (`alpha`, `[cache]`) that appear inside
/// longer lines; pure JW of the whole line vs the short token can fall under
/// 0.8 even when the anchor is clearly present (fixrealloop 2026-07-15).
fn context_fragment_score(content_fragment: &str, ctx_fragment: &str) -> f64 {
    let a = content_fragment.trim();
    let b = ctx_fragment.trim();
    if b.is_empty() {
        return 0.0;
    }
    let jw = strsim::jaro_winkler(a, b);
    // Containment for short anchors inside longer lines (min 2 chars avoids
    // matching single-character noise like "a" / "x" everywhere).
    if b.len() >= 2 && a.contains(b) {
        jw.max(1.0)
    } else {
        jw
    }
}

/// Filter multiple exact matches by `before_context` / `after_context`.
///
/// When the `old` text occurs more than once in `content` and no `nth` is
/// specified, this function uses context lines to pick the right occurrence.
/// It compares up to 3 lines of context (nearest to the match first) using
/// Jaro-Winkler similarity and substring containment (threshold >= 0.8). The
/// occurrence with the highest aggregate score wins. Returns the byte offset
/// of the winning match, or `None` if no context is provided, only one match
/// exists, or all scores are zero.
///
/// For `before_context`, the last N lines are compared against the N lines
/// preceding the match, and (for single-line `old`) the same-line prefix
/// before the match is also scored against the nearest context fragment.
/// For `after_context`, the first N lines following the match are compared,
/// plus the same-line suffix after a single-line match.
/// Tie-breaking: first occurrence (lowest byte offset) wins on equal scores.
pub fn context_filtered_offset(
    content: &str,
    old: &str,
    before_context: Option<&str>,
    after_context: Option<&str>,
) -> Option<usize> {
    if before_context.is_none() && after_context.is_none() {
        return None;
    }

    let offsets: Vec<usize> = content.match_indices(old).map(|(i, _)| i).collect();
    if offsets.len() < 2 {
        return None; // single match: no disambiguation needed
    }

    let lines: Vec<&str> = content.lines().collect();
    // Build a map: byte-offset-of-line-start -> line-index
    let mut line_starts: Vec<usize> = Vec::with_capacity(lines.len());
    let mut off = 0;
    for line in &lines {
        line_starts.push(off);
        off += line.len();
        // skip the newline character(s)
        if content.as_bytes().get(off) == Some(&b'\r') {
            off += 1;
        }
        if content.as_bytes().get(off) == Some(&b'\n') {
            off += 1;
        }
    }

    let line_index_at = |byte_offset: usize| -> usize {
        match line_starts.binary_search(&byte_offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        }
    };

    const MAX_CONTEXT_LINES: usize = 3;
    let old_line_count = old.lines().count().max(1);
    let single_line_old = old_line_count == 1 && !old.contains('\n');

    let mut best: Option<(usize, f64)> = None;
    for &match_off in &offsets {
        let match_line = line_index_at(match_off);
        let mut score = 0.0f64;
        let mut checks = 0u32;

        if let Some(before) = before_context {
            let ctx_lines: Vec<&str> = before.lines().collect();
            // Take up to MAX_CONTEXT_LINES from the end (nearest to match first).
            let start = ctx_lines.len().saturating_sub(MAX_CONTEXT_LINES);
            let ctx_tail = &ctx_lines[start..];
            for (i, ctx_line) in ctx_tail.iter().rev().enumerate() {
                // Nearest before-context fragment also scores the same-line
                // prefix (agents pass anchors that sit on the match line).
                if i == 0 && single_line_old && match_line < lines.len() {
                    let line = lines[match_line];
                    let col = match_off
                        .saturating_sub(line_starts[match_line])
                        .min(line.len());
                    if line.is_char_boundary(col) {
                        checks += 1;
                        let sim = context_fragment_score(&line[..col], ctx_line);
                        if sim >= 0.8 {
                            score += sim;
                        }
                    }
                }
                let content_idx = match_line.checked_sub(i + 1);
                if let Some(ci) = content_idx {
                    checks += 1;
                    let sim = context_fragment_score(lines[ci], ctx_line);
                    if sim >= 0.8 {
                        score += sim;
                    }
                }
            }
        }

        if let Some(after) = after_context {
            let ctx_lines: Vec<&str> = after.lines().collect();
            let n = ctx_lines.len().min(MAX_CONTEXT_LINES);
            let end_line = match_line + old_line_count;
            for (i, ctx_line) in ctx_lines[..n].iter().enumerate() {
                // Same-line suffix for nearest after-context fragment.
                if i == 0 && single_line_old && match_line < lines.len() {
                    let line = lines[match_line];
                    let match_end = match_off.saturating_add(old.len());
                    let col = match_end
                        .saturating_sub(line_starts[match_line])
                        .min(line.len());
                    if line.is_char_boundary(col) {
                        checks += 1;
                        let sim = context_fragment_score(&line[col..], ctx_line);
                        if sim >= 0.8 {
                            score += sim;
                        }
                    }
                }
                let content_idx = end_line + i;
                if content_idx < lines.len() {
                    checks += 1;
                    let sim = context_fragment_score(lines[content_idx], ctx_line);
                    if sim >= 0.8 {
                        score += sim;
                    }
                }
            }
        }

        if checks > 0 && score > 0.0 && best.is_none_or(|(_, s)| score > s) {
            best = Some((match_off, score));
        }
    }

    best.map(|(off, _)| off)
}

/// Whole-line replacement: when a line matches the pattern, the entire line
/// (including its newline) is replaced with `to`. When `to` is empty, the
/// line is deleted. Supports optional line-range restriction and nth match.
///
/// For regex patterns, capture groups in `to` are expanded using the match
/// found on each line.
pub fn replace_whole_lines<'a>(
    content: &'a str,
    from: &str,
    to: &str,
    compiled_re: Option<&Regex>,
    nth: Option<usize>,
    range: Option<(usize, Option<usize>)>,
) -> (std::borrow::Cow<'a, str>, usize) {
    use std::borrow::Cow;

    let mut result = String::with_capacity(content.len());
    let mut match_count = 0usize;
    let mut rest = content;
    let mut line_num = 0usize; // 1-based

    while !rest.is_empty() {
        line_num += 1;

        // Find line boundary (\n, \r\n, or bare \r).
        let rest_bytes = rest.as_bytes();
        let (line_content, ending, advance) =
            if let Some(pos) = memchr::memchr2(b'\r', b'\n', rest_bytes) {
                if rest_bytes[pos] == b'\n' {
                    (&rest[..pos], "\n", pos + 1)
                } else if pos + 1 < rest_bytes.len() && rest_bytes[pos + 1] == b'\n' {
                    (&rest[..pos], "\r\n", pos + 2)
                } else {
                    (&rest[..pos], "\r", pos + 1)
                }
            } else {
                (rest, "", rest.len())
            };
        let line_with_ending = &rest[..advance];

        // Check range restriction.
        let in_range = match range {
            Some((start, Some(end))) => line_num >= start && line_num <= end,
            Some((start, None)) => line_num >= start,
            None => true,
        };

        if !in_range {
            result.push_str(line_with_ending);
            rest = &rest[advance..];
            continue;
        }

        // Check if this line matches the pattern.
        let line_match = if let Some(re) = compiled_re {
            re.captures(line_content)
        } else if line_content.contains(from) {
            None // Sentinel: literal match found, no captures.
        } else {
            // Use a special marker to distinguish "no match" from
            // "literal match with no captures".
            rest = &rest[advance..];
            result.push_str(line_with_ending);
            continue;
        };

        // For literal matches, we set a flag and handle below.
        let is_literal_match = compiled_re.is_none() && line_content.contains(from);
        let has_match = line_match.is_some() || is_literal_match;

        if !has_match {
            result.push_str(line_with_ending);
            rest = &rest[advance..];
            continue;
        }

        match_count += 1;

        // Handle --nth: only act on the Nth occurrence.
        if let Some(n) = nth
            && match_count != n
        {
            result.push_str(line_with_ending);
            rest = &rest[advance..];
            continue;
        }

        // Replace the line.
        if to.is_empty() {
            // Delete the line entirely (don't append anything).
        } else if let Some(ref caps) = line_match {
            // Regex with captures: expand replacement text.
            let mut expanded = String::new();
            caps.expand(to, &mut expanded);
            result.push_str(&expanded);
            // Preserve the original line ending.
            result.push_str(ending);
        } else {
            // Literal match: replacement is used as-is.
            result.push_str(to);
            result.push_str(ending);
        }

        rest = &rest[advance..];
    }

    // Determine actual match count for nth mode.
    let effective_count = if let Some(n) = nth {
        if match_count >= n { 1 } else { 0 }
    } else {
        match_count
    };

    if effective_count == 0 {
        return (Cow::Borrowed(content), 0);
    }

    (Cow::Owned(result), effective_count)
}

#[path = "replace_tests.rs"]
#[cfg(test)]
mod tests;
