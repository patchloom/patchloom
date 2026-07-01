use std::borrow::Cow;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct HeadingInfo {
    pub level: usize,
    pub text: String,
    pub line_start: usize,
    /// First line of the section body (after the heading and any underline).
    /// For ATX headings: `line_start + 1`.
    /// For setext headings: `line_start + 2` (text line + underline).
    pub body_line: usize,
    pub line_end: usize,
}

/// Iterate over lines that are NOT inside fenced code blocks or YAML
/// frontmatter.
/// Yields `(0-based line index, line content)` pairs, skipping lines
/// within ```` ``` ```` or `~~~` fenced regions, and skipping YAML
/// frontmatter (`---` delimited block at the very start of the file).
pub fn non_fenced_lines(content: &str) -> impl Iterator<Item = (usize, &str)> {
    // Track the fence character and minimum length required to close.
    let mut fence: Option<(u8, usize)> = None;
    // Detect YAML frontmatter: first line must be exactly `---` (with
    // optional trailing whitespace). If present, skip until the closing
    // `---` delimiter.
    let mut in_frontmatter = false;
    let mut is_first_line = true;
    // Track HTML block comments (CommonMark type 2): <!-- ... -->
    let mut in_html_comment = false;
    content.lines().enumerate().filter(move |(_, line)| {
        // YAML frontmatter handling: must start on the very first line.
        if is_first_line {
            is_first_line = false;
            let trimmed = line.trim();
            if trimmed == "---" {
                in_frontmatter = true;
                return false;
            }
        } else if in_frontmatter {
            let trimmed = line.trim();
            if trimmed == "---" || trimmed == "..." {
                in_frontmatter = false;
            }
            return false;
        }

        // CommonMark: fences can be indented 0-3 spaces.
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if indent <= 3 {
            if let Some((ch, min_len)) = fence {
                let count = trimmed.bytes().take_while(|&b| b == ch).count();
                if count >= min_len {
                    // CommonMark 4.5: the closing code fence may optionally
                    // be followed by spaces and tabs only. No other characters
                    // may occur on the line. This applies to both backtick and
                    // tilde fences.
                    let after_fence = &trimmed[count..];
                    if after_fence.bytes().all(|b| b == b' ' || b == b'\t') {
                        fence = None;
                        return false;
                    }
                }
            } else {
                let backticks = trimmed.bytes().take_while(|&b| b == b'`').count();
                // CommonMark 4.5: if the info string comes after a backtick
                // fence, it may not contain any backtick characters.
                if backticks >= 3 && !trimmed[backticks..].contains('`') {
                    fence = Some((b'`', backticks));
                    return false;
                }
                let tildes = trimmed.bytes().take_while(|&b| b == b'~').count();
                if tildes >= 3 {
                    fence = Some((b'~', tildes));
                    return false;
                }
            }
        }
        if fence.is_some() {
            return false;
        }

        // HTML block comment handling (CommonMark type 2): skip lines
        // inside <!-- ... -->. Only checked outside fenced code blocks.
        if in_html_comment {
            if line.contains("-->") {
                in_html_comment = false;
            }
            return false;
        }
        let comment_trimmed = trimmed.trim_start_matches(' ');
        let comment_indent = line.len() - comment_trimmed.len();
        if comment_indent <= 3 && comment_trimmed.starts_with("<!--") {
            if !line.contains("-->") {
                in_html_comment = true;
                return false;
            }
            // Single-line comment (<!-- ... --> on one line): skip just this line.
            return false;
        }

        true
    })
}

/// Check if a line is a setext underline (`===` for h1, `---` for h2).
/// Returns `Some(level)` if the line is a valid setext underline.
/// CommonMark: the underline must be at least one `=` or `-` character,
/// optionally preceded by up to 3 spaces and followed by trailing spaces.
fn setext_underline_level(line: &str) -> Option<usize> {
    let stripped = line.trim_start_matches(' ');
    let indent = line.len() - stripped.len();
    if indent > 3 {
        return None;
    }
    let trimmed = stripped.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.bytes().all(|b| b == b'=') {
        Some(1)
    } else if trimmed.bytes().all(|b| b == b'-') {
        Some(2)
    } else {
        None
    }
}

/// Strip an optional ATX closing sequence from heading text.
///
/// Per CommonMark, a trailing sequence of `#` characters preceded by a space
/// (or tabs) is removed, along with any trailing whitespace before that space.
/// Example: `" Heading ## "` → `"Heading"`.
fn strip_atx_closing(text: &str) -> String {
    let trimmed = text.trim_end();
    // Count trailing '#' characters.
    let hash_count = trimmed.bytes().rev().take_while(|&b| b == b'#').count();
    if hash_count == 0 {
        return trimmed.to_string();
    }
    // The hashes must be preceded by a space/tab, or the text is entirely hashes.
    let before = &trimmed[..trimmed.len() - hash_count];
    if before.is_empty() {
        // Text is all '#' — the entire content is the closing sequence.
        return String::new();
    }
    if before.ends_with([' ', '\t']) {
        before.trim_end().to_string()
    } else {
        // No space before hashes — they are part of the heading text.
        trimmed.to_string()
    }
}

pub fn parse_headings(content: &str) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let total_lines = content.lines().count();

    // Collect non-fenced lines into a vec so we can look ahead.
    let nf_lines: Vec<(usize, &str)> = non_fenced_lines(content).collect();

    for (pos, &(idx, line)) in nf_lines.iter().enumerate() {
        // ATX heading: optional 0-3 spaces then 1-6 '#' then space/tab/EOL
        let stripped = line.trim_start_matches(' ');
        let indent = line.len() - stripped.len();
        if indent <= 3 && stripped.starts_with('#') {
            let hashes = stripped.bytes().take_while(|&b| b == b'#').count();
            if hashes > 6 {
                continue;
            }
            // After the hashes: must be end-of-line, space, or tab
            if hashes < stripped.len()
                && stripped.as_bytes()[hashes] != b' '
                && stripped.as_bytes()[hashes] != b'\t'
            {
                continue;
            }
            let text = if hashes >= stripped.len() {
                // Empty heading (line is only '#' characters)
                String::new()
            } else {
                strip_atx_closing(stripped[hashes + 1..].trim_start())
            };
            headings.push(HeadingInfo {
                level: hashes,
                text,
                line_start: idx,
                body_line: idx + 1,
                line_end: 0,
            });
            continue;
        }

        // Setext heading: current line is the underline, previous
        // non-fenced line is the heading text.
        if let Some(level) = setext_underline_level(line)
            && pos > 0
        {
            let (prev_idx, prev_line) = nf_lines[pos - 1];
            // The text line must be non-empty and on the line
            // immediately before the underline.
            let prev_trimmed = prev_line.trim();
            if !prev_trimmed.is_empty() && idx == prev_idx + 1 {
                // Avoid treating the underline's text line as a
                // heading if it was already parsed as an ATX heading.
                let already_atx = headings.last().is_some_and(|h| h.line_start == prev_idx);
                if !already_atx {
                    headings.push(HeadingInfo {
                        level,
                        text: prev_trimmed.to_string(),
                        line_start: prev_idx,
                        body_line: idx + 1, // after the underline
                        line_end: 0,
                    });
                }
            }
        }
    }

    // Sort headings by line_start in case setext headings were
    // interleaved with ATX headings (unlikely but safe).
    headings.sort_by_key(|h| h.line_start);

    for i in 0..headings.len() {
        let lvl = headings[i].level;
        let mut end = total_lines;
        for h in headings.iter().skip(i + 1) {
            if h.level <= lvl {
                end = h.line_start;
                break;
            }
        }
        headings[i].line_end = end;
    }

    headings
}

fn line_byte_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Parse a heading query into (optional level, text).
///
/// If the query includes ATX-style `#` markers (e.g. `"## API"`), the
/// level is extracted and used to filter matches. Plain text queries
/// (e.g. `"API"`) match any heading level (#1158).
fn normalize_heading_query(heading: &str) -> (Option<usize>, &str) {
    let t = heading.trim();
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if n > 0 && t.len() > n && t.as_bytes()[n] == b' ' {
        (Some(n), t[n + 1..].trim())
    } else {
        (None, t)
    }
}

/// Check whether a heading matches the parsed query (level + text).
fn heading_matches(h: &HeadingInfo, level: Option<usize>, query: &str) -> bool {
    h.text.trim() == query && level.is_none_or(|lvl| h.level == lvl)
}

pub fn find_section(content: &str, heading: &str) -> Option<(usize, usize)> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let (level, query) = normalize_heading_query(heading);

    for h in &headings {
        if heading_matches(h, level, query) {
            let body_start = if h.body_line < offsets.len() {
                offsets[h.body_line]
            } else {
                content.len()
            };
            let body_end = if h.line_end < offsets.len() {
                offsets[h.line_end]
            } else {
                content.len()
            };
            return Some((body_start, body_end));
        }
    }
    None
}

/// Return the full byte range of a section INCLUDING its heading line.
///
/// Unlike `find_section` which returns only the body range (after the
/// heading), this returns `(section_start, section_end)` where
/// `section_start` is the first byte of the heading line itself.
pub fn section_range(content: &str, heading: &str) -> Option<(usize, usize)> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let (level, query) = normalize_heading_query(heading);

    for h in &headings {
        if heading_matches(h, level, query) {
            let section_start = offsets[h.line_start];
            let section_end = if h.line_end < offsets.len() {
                offsets[h.line_end]
            } else {
                content.len()
            };
            return Some((section_start, section_end));
        }
    }
    None
}

/// Move a heading section to a new location, either within the same file
/// or from one file to another.
///
/// Returns `(new_source, new_dest)`. For same-file moves the caller
/// should use only `new_source` (both values are identical).
///
/// `position` is `("before", heading)` or `("after", heading)`.
pub fn move_section_in(
    source_content: &str,
    heading: &str,
    dest_content: &str,
    position: (&str, &str),
    same_file: bool,
) -> Option<(String, String)> {
    let (section_start, section_end) = section_range(source_content, heading)?;
    let section_text = &source_content[section_start..section_end];

    if same_file {
        // Same-file reorder: remove the section first, then insert into
        // the resulting content. This avoids complex offset adjustment.
        let eol = crate::write::detect_eol(source_content);
        let without_section = format!(
            "{}{}",
            &source_content[..section_start],
            &source_content[section_end..]
        );
        let result = match position.0 {
            "before" => insert_before_heading_in(&without_section, position.1, section_text)?,
            "after" => {
                // Insert after the *full* destination section body (so that the
                // destination's table/list/etc stays under its heading).
                if let Some((_, dest_body_end)) = find_section(&without_section, position.1) {
                    let mut out =
                        String::with_capacity(without_section.len() + section_text.len() + 2);
                    out.push_str(&without_section[..dest_body_end]);
                    if !out.ends_with("\n\n") && !out.ends_with("\r\n\r\n") && !out.is_empty() {
                        out.push_str(eol);
                    }
                    out.push_str(section_text);
                    if !section_text.ends_with('\n') {
                        out.push_str(eol);
                    }
                    out.push_str(&without_section[dest_body_end..]);
                    out
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some((result.clone(), result))
    } else {
        // Cross-file move: remove from source, insert into dest.
        let eol = crate::write::detect_eol(dest_content);
        let new_source = format!(
            "{}{}",
            &source_content[..section_start],
            &source_content[section_end..]
        );
        let new_dest = match position.0 {
            "before" => insert_before_heading_in(dest_content, position.1, section_text)?,
            "after" => {
                if let Some((_, dest_body_end)) = find_section(dest_content, position.1) {
                    let mut out =
                        String::with_capacity(dest_content.len() + section_text.len() + 2);
                    out.push_str(&dest_content[..dest_body_end]);
                    if !out.ends_with("\n\n") && !out.ends_with("\r\n\r\n") && !out.is_empty() {
                        out.push_str(eol);
                    }
                    out.push_str(section_text);
                    if !section_text.ends_with('\n') {
                        out.push_str(eol);
                    }
                    out.push_str(&dest_content[dest_body_end..]);
                    out
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some((new_source, new_dest))
    }
}

pub fn replace_section_in(content: &str, heading: &str, replacement: &str) -> Option<String> {
    let eol = crate::write::detect_eol(content);
    let (body_start, body_end) = find_section(content, heading)?;

    // If the replacement starts with the same heading line, strip it so the
    // caller does not have to know whether the heading is included or not.
    // The heading is already preserved in content[..body_start].
    let replacement = strip_leading_heading(replacement, heading);

    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..body_start]);
    if !replacement.is_empty() {
        out.push_str(replacement);
        if !replacement.ends_with('\n') {
            out.push_str(eol);
        }
    }
    out.push_str(&content[body_end..]);
    Some(out)
}

/// Strip a leading heading line from `text` if it matches `heading`.
/// Handles optional trailing whitespace and newlines after the heading line.
fn strip_leading_heading<'a>(text: &'a str, heading: &str) -> &'a str {
    let (level, query) = normalize_heading_query(heading);
    let first_line = text.lines().next().unwrap_or("");
    let (first_level, first_text) = normalize_heading_query(first_line);
    if first_text == query && (level.is_none() || first_level == level) {
        let after_line = &text[first_line.len()..];
        // Skip the newline(s) after the heading line
        after_line
            .strip_prefix("\r\n")
            .unwrap_or_else(|| after_line.strip_prefix('\n').unwrap_or(after_line))
    } else {
        text
    }
}

pub fn insert_after_heading_in(content: &str, heading: &str, insertion: &str) -> Option<String> {
    let eol = crate::write::detect_eol(content);
    let (body_start, _) = find_section(content, heading)?;
    let mut out = String::with_capacity(content.len() + insertion.len());
    out.push_str(&content[..body_start]);
    out.push_str(insertion);
    if !insertion.is_empty() && !insertion.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&content[body_start..]);
    Some(out)
}

pub fn insert_before_heading_in(content: &str, heading: &str, insertion: &str) -> Option<String> {
    let eol = crate::write::detect_eol(content);
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let (level, query) = normalize_heading_query(heading);

    for h in &headings {
        if heading_matches(h, level, query) {
            let heading_start = offsets[h.line_start];
            let mut out = String::with_capacity(content.len() + insertion.len());
            out.push_str(&content[..heading_start]);
            if !insertion.is_empty() {
                out.push_str(insertion);
                if !insertion.ends_with('\n') {
                    out.push_str(eol);
                }
                if !out.ends_with("\n\n") && !out.ends_with("\r\n\r\n") {
                    out.push_str(eol);
                }
            }
            out.push_str(&content[heading_start..]);
            return Some(out);
        }
    }
    None
}

/// Strip a bullet prefix (`- `, `* `, `+ `) from a trimmed line,
/// returning the text content for style-independent comparison.
fn strip_bullet_prefix(s: &str) -> &str {
    if s.starts_with("- ") || s.starts_with("* ") || s.starts_with("+ ") {
        &s[2..]
    } else {
        s
    }
}

pub fn upsert_bullet_in(content: &str, heading: &str, bullet: &str) -> Option<String> {
    let eol = crate::write::detect_eol(content);
    let (body_start, body_end) = find_section(content, heading)?;
    let body = &content[body_start..body_end];

    let trimmed = bullet.trim();
    let normalized =
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            trimmed.to_string()
        } else {
            format!("- {trimmed}")
        };

    // Dedup across bullet styles: compare text content without prefix.
    let new_text = strip_bullet_prefix(&normalized);
    for line in body.lines() {
        // Only dedup against top-level bullets (no leading whitespace).
        // Indented sub-bullets (e.g. "  - deploy") should not block
        // insertion of a top-level bullet with the same text (#1157).
        // Only compare against actual bullet lines, not plain paragraphs
        // that happen to match the bullet text (#1173).
        let trimmed = line.trim_start();
        if trimmed.len() == line.len()
            && (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
        {
            let existing_text = strip_bullet_prefix(trimmed);
            if existing_text == new_text {
                return Some(content.to_string());
            }
        }
    }

    // Find the end of actual content within the body, before any trailing
    // blank lines. This ensures the new bullet is grouped with existing
    // bullets rather than placed after a blank line separator.
    let trimmed_body = body.trim_end_matches(['\n', '\r']);
    let content_end = body_start + trimmed_body.len();
    // Find the next line boundary after content_end (include the trailing \n of
    // the last non-blank line so the bullet goes on a new line, not appended).
    let insert_at = if content_end < body_end {
        // There is at least one trailing newline; include the first one.
        content_end
            + if content.as_bytes().get(content_end) == Some(&b'\r')
                && content.as_bytes().get(content_end + 1) == Some(&b'\n')
            {
                2
            } else if content.as_bytes().get(content_end) == Some(&b'\n') {
                1
            } else {
                0
            }
    } else {
        body_end
    };

    let mut out = String::with_capacity(content.len() + normalized.len() + 4);
    out.push_str(&content[..insert_at]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&normalized);
    out.push_str(eol);
    // Preserve the blank line separator before the next heading.
    let remainder = &content[insert_at..];
    if remainder.starts_with('#') && !out.ends_with("\n\n") && !out.ends_with("\r\n\r\n") {
        out.push_str(eol);
    }
    out.push_str(remainder);
    Some(out)
}

pub fn dedupe_headings_in(content: &str) -> (String, Vec<String>) {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut removed: Vec<String> = Vec::new();

    for h in &headings {
        let key = (h.level, h.text.trim().to_string());
        if !seen.insert(key) {
            let start = offsets[h.line_start];
            let end = if h.line_end < offsets.len() {
                offsets[h.line_end]
            } else {
                content.len()
            };
            ranges.push((start, end));
            removed.push(format!("{} {}", "#".repeat(h.level), h.text));
        }
    }

    let mut out = String::with_capacity(content.len());
    let mut pos = 0;
    for (start, end) in &ranges {
        if *start < pos {
            continue;
        }
        out.push_str(&content[pos..*start]);
        pos = *end;
    }
    out.push_str(&content[pos..]);

    (out, removed)
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.len() > 1 && t.starts_with('|') && t.ends_with('|')
}

fn is_separator_row(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 || !t.starts_with('|') || !t.ends_with('|') {
        return false;
    }
    let inner = &t[1..t.len() - 1];
    if !inner.chars().all(|c| matches!(c, '-' | ':' | '|' | ' ')) {
        return false;
    }
    // Each cell must contain at least one dash per CommonMark spec.
    inner.split('|').all(|cell| cell.contains('-'))
}

/// Error returned by [`table_append_in`] when the append cannot proceed.
#[derive(Debug)]
pub enum TableAppendError {
    /// No markdown table was found under the heading.
    NoTable,
    /// The row has the wrong number of columns.
    ColumnMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for TableAppendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTable => write!(f, "no markdown table found"),
            Self::ColumnMismatch { expected, actual } => {
                write!(f, "row has {actual} column(s), table has {expected}")
            }
        }
    }
}

pub fn table_append_in(
    content: &str,
    body_start: usize,
    body_end: usize,
    row: &str,
) -> Result<String, TableAppendError> {
    let eol = crate::write::detect_eol(content);
    let body = &content[body_start..body_end];
    let mut last_data_end: Option<usize> = None;
    let mut in_table = false;
    let mut pos = body_start;

    for line in body.lines() {
        let line_byte_end = pos + line.len();
        let next_pos = if content.as_bytes().get(line_byte_end) == Some(&b'\r')
            && content.as_bytes().get(line_byte_end + 1) == Some(&b'\n')
        {
            line_byte_end + 2
        } else if content.as_bytes().get(line_byte_end) == Some(&b'\n') {
            line_byte_end + 1
        } else {
            line_byte_end
        };

        if is_table_row(line) {
            in_table = true;
            if is_separator_row(line) {
                // Always update after the separator so new rows go
                // below it, even when no data rows exist yet.
                last_data_end = Some(next_pos);
            } else if last_data_end.is_some() {
                // Only count non-separator rows that appear after the
                // separator (i.e. actual data rows, not the header).
                last_data_end = Some(next_pos);
            }
        } else if in_table {
            break;
        }

        pos = next_pos;
    }

    let insert_pos = last_data_end.ok_or(TableAppendError::NoTable)?;

    // Validate that the new row has the same column count as the existing
    // table to prevent silent corruption of markdown tables (#1172).
    let expected_cols = {
        let section = &content[body_start..body_end];
        section
            .lines()
            .find(|l| is_separator_row(l))
            .map(|sep| sep.trim().matches('|').count().saturating_sub(1))
    };
    if let Some(expected) = expected_cols {
        let actual = if is_table_row(row) {
            row.trim().matches('|').count().saturating_sub(1)
        } else {
            0
        };
        if actual != expected {
            return Err(TableAppendError::ColumnMismatch { expected, actual });
        }
    }

    let mut out = String::with_capacity(content.len() + row.len() + 2);
    out.push_str(&content[..insert_pos]);
    // Ensure there is a newline separator before the new row; without
    // this, files that lack a trailing newline get the new row fused
    // onto the last existing data row.
    if insert_pos > 0 && content.as_bytes().get(insert_pos - 1) != Some(&b'\n') {
        out.push_str(eol);
    }
    out.push_str(row);
    if !row.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&content[insert_pos..]);
    Ok(out)
}

pub fn table_append_for_tx(
    content: &str,
    heading: &str,
    row: &str,
) -> Result<Option<String>, TableAppendError> {
    let Some((body_start, body_end)) = find_section(content, heading) else {
        return Ok(None);
    };
    table_append_in(content, body_start, body_end, row).map(Some)
}

/// Strip inline code spans (between backticks) from a line so that
/// we don't false-positive on code examples. Handles multi-backtick
/// spans (e.g. `` `code` ``, ``` ``code`` ```).
pub(crate) fn strip_inline_code(line: &str) -> Cow<'_, str> {
    if !line.contains('`') {
        return Cow::Borrowed(line);
    }
    let mut result = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // Count the opening backtick run length.
            let run_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let run_len = i - run_start;
            // Find the matching closing run of the same length.
            let mut found = false;
            let mut j = i;
            while j < bytes.len() {
                if bytes[j] == b'`' {
                    let close_start = j;
                    while j < bytes.len() && bytes[j] == b'`' {
                        j += 1;
                    }
                    if j - close_start == run_len {
                        // Matched closing run — skip the entire span.
                        i = j;
                        found = true;
                        break;
                    }
                    // Different run length — not a match, continue searching.
                } else {
                    j += 1;
                }
            }
            if !found {
                // Unmatched backtick run — keep the rest as-is.
                result.push_str(&line[run_start..]);
                return Cow::Owned(result);
            }
        } else {
            // Advance by one UTF-8 character, not one byte.
            // `bytes[i] as char` corrupts multi-byte UTF-8 (e.g. é → Ã©).
            let ch = line[i..].chars().next().expect("i < len guarantees char");
            result.push(ch);
            i += ch.len_utf8();
        }
    }
    Cow::Owned(result)
}

/// Detects a literal "git add ." (or "git add ." followed by whitespace)
/// that is not inside code.
pub(crate) fn has_dangerous_git_add_dot(line: &str) -> bool {
    let needle = "git add .";
    let mut start = 0;
    while let Some(pos) = line[start..].find(needle) {
        let abs = start + pos;
        let after = abs + needle.len();
        if after >= line.len() || line.as_bytes()[after].is_ascii_whitespace() {
            return true;
        }
        // "git add ./" is equivalent to "git add ." — catch it too.
        if line.as_bytes()[after] == b'/' {
            let after_slash = after + 1;
            if after_slash >= line.len() || line.as_bytes()[after_slash].is_ascii_whitespace() {
                return true;
            }
        }
        start = abs + 1;
    }
    false
}

/// A lint issue found in a markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintIssue {
    /// Description of the issue.
    pub issue: String,
    /// Line number where the issue was found (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Heading text related to the issue (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

pub fn lint_agents_content(content: &str) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    // 1. Duplicate headings (same text at same level).
    let headings = parse_headings(content);
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    for h in &headings {
        let key = (h.level, h.text.trim().to_string());
        if !seen.insert(key) {
            issues.push(LintIssue {
                issue: "duplicate heading".to_string(),
                line: Some(h.line_start + 1), // 1-based
                heading: Some(format!("{} {}", "#".repeat(h.level), h.text)),
            });
        }
    }

    // 2. Dangerous git add commands (skip fenced code blocks and inline code).
    for (idx, line) in non_fenced_lines(content) {
        let stripped = strip_inline_code(line);
        if has_dangerous_git_add_dot(&stripped)
            || stripped.contains("git add -A")
            || stripped.contains("git add --all")
        {
            issues.push(LintIssue {
                issue: "dangerous command".to_string(),
                line: Some(idx + 1),
                heading: None,
            });
        }
    }

    // 3. Missing final newline.
    if !content.is_empty() && !content.ends_with('\n') {
        issues.push(LintIssue {
            issue: "missing final newline".to_string(),
            line: None,
            heading: None,
        });
    }

    issues
}

#[path = "md_tests.rs"]
#[cfg(test)]
mod tests;
