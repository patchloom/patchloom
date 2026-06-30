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
        return Ok(Some(
            regex::RegexBuilder::new(&wb_pattern)
                .case_insensitive(case_insensitive)
                .dot_matches_new_line(multiline)
                .build()?,
        ));
    }
    if regex_mode {
        let effective = if word_boundary {
            format!("\\b(?:{pattern})\\b")
        } else {
            pattern.to_string()
        };
        Ok(Some(
            regex::RegexBuilder::new(&effective)
                .case_insensitive(case_insensitive)
                .dot_matches_new_line(multiline)
                .build()?,
        ))
    } else if case_insensitive {
        Ok(Some(
            regex::RegexBuilder::new(&regex::escape(pattern))
                .case_insensitive(true)
                .build()?,
        ))
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
            Self::EmptyPattern => write!(f, "search pattern must not be empty"),
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
                ReplaceModeError::MissingMode => {
                    write!(
                        f,
                        "one of 'to', 'insert_before', or 'insert_after' must be provided"
                    )
                }
                ReplaceModeError::BothInsertModes => {
                    write!(f, "'insert_before' and 'insert_after' cannot be combined")
                }
                ReplaceModeError::ToWithInsert => {
                    write!(
                        f,
                        "'to' cannot be combined with 'insert_before' or 'insert_after'"
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

pub fn replacement_text(
    from: &str,
    to: &Option<String>,
    insert_before: &Option<String>,
    insert_after: &Option<String>,
    use_match_anchor: bool,
    regex_mode: bool,
) -> String {
    let anchor = if use_match_anchor { "${0}" } else { from };

    // When a regex is compiled internally (case_insensitive / word_boundary)
    // but the user did NOT request regex mode, dollar signs in user-provided
    // replacement text must be escaped so caps.expand() treats them literally.
    let needs_escape = use_match_anchor && !regex_mode;

    if let Some(text) = insert_before {
        let safe = if needs_escape {
            text.replace('$', "$$")
        } else {
            text.clone()
        };
        return format!("{safe}{anchor}");
    }

    if let Some(text) = insert_after {
        let safe = if needs_escape {
            text.replace('$', "$$")
        } else {
            text.clone()
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
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            for caps in re.captures_iter(content) {
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
            let mut count = 0usize;
            let replaced = re.replace_all(content, |caps: &regex::Captures| {
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
            if from.is_empty() {
                return (Cow::Borrowed(content), 0);
            }
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

/// Filter multiple exact matches by `before_context` / `after_context`.
///
/// When the `old` text occurs more than once in `content` and no `nth` is
/// specified, this function uses context lines to pick the right occurrence.
/// It returns the byte offset of the winning match, or `None` if no
/// occurrence (or all occurrences) match the context equally well.
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

    let mut best: Option<(usize, f64)> = None;
    for &match_off in &offsets {
        let match_line = line_index_at(match_off);
        let mut score = 0.0f64;
        let mut checks = 0u32;

        if let Some(before) = before_context {
            checks += 1;
            let before_line = before.lines().last().unwrap_or(before).trim();
            if match_line > 0 {
                let prev = lines[match_line - 1].trim();
                let sim = strsim::jaro_winkler(prev, before_line);
                if sim >= 0.8 {
                    score += sim;
                }
            }
        }

        if let Some(after) = after_context {
            checks += 1;
            let after_line = after.lines().next().unwrap_or(after).trim();
            let old_lines = old.lines().count();
            let end_line = match_line + old_lines;
            if end_line < lines.len() {
                let next = lines[end_line].trim();
                let sim = strsim::jaro_winkler(next, after_line);
                if sim >= 0.8 {
                    score += sim;
                }
            }
        }

        if checks > 0 && score > 0.0 && (best.is_none() || score > best.unwrap().1) {
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
