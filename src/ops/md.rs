use std::borrow::Cow;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct HeadingInfo {
    pub level: usize,
    pub text: String,
    pub line_start: usize,
    pub line_end: usize,
}

/// Iterate over lines that are NOT inside fenced code blocks.
/// Yields `(0-based line index, line content)` pairs, skipping lines
/// within ```` ``` ```` or `~~~` fenced regions.
pub fn non_fenced_lines(content: &str) -> impl Iterator<Item = (usize, &str)> {
    // Track the fence character and minimum length required to close.
    let mut fence: Option<(u8, usize)> = None;
    content.lines().enumerate().filter(move |(_, line)| {
        // CommonMark: fences can be indented 0-3 spaces.
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if indent <= 3 {
            if let Some((ch, min_len)) = fence {
                let count = trimmed.bytes().take_while(|&b| b == ch).count();
                if count >= min_len {
                    fence = None;
                    return false;
                }
            } else {
                let backticks = trimmed.bytes().take_while(|&b| b == b'`').count();
                if backticks >= 3 {
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
        fence.is_none()
    })
}

pub fn parse_headings(content: &str) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let total_lines = content.lines().count();

    for (idx, line) in non_fenced_lines(content) {
        if !line.starts_with('#') {
            continue;
        }
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if hashes > 6 || hashes >= line.len() {
            continue;
        }
        if line.as_bytes()[hashes] != b' ' {
            continue;
        }
        headings.push(HeadingInfo {
            level: hashes,
            text: line[hashes + 1..].to_string(),
            line_start: idx,
            line_end: 0,
        });
    }

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

fn normalize_heading_query(heading: &str) -> &str {
    let t = heading.trim();
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if n > 0 && t.len() > n && t.as_bytes()[n] == b' ' {
        t[n + 1..].trim()
    } else {
        t
    }
}

pub fn find_section(content: &str, heading: &str) -> Option<(usize, usize)> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let query = normalize_heading_query(heading);

    for h in &headings {
        if h.text.trim() == query {
            let body_start = if h.line_start + 1 < offsets.len() {
                offsets[h.line_start + 1]
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
    let query = normalize_heading_query(heading);

    for h in &headings {
        if h.text.trim() == query {
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
        let without_section = format!(
            "{}{}",
            &source_content[..section_start],
            &source_content[section_end..]
        );
        let result = match position.0 {
            "before" => insert_before_heading_in(&without_section, position.1, section_text)?,
            "after" => insert_after_heading_in(&without_section, position.1, section_text)?,
            _ => return None,
        };
        Some((result.clone(), result))
    } else {
        // Cross-file move: remove from source, insert into dest.
        let new_source = format!(
            "{}{}",
            &source_content[..section_start],
            &source_content[section_end..]
        );
        let new_dest = match position.0 {
            "before" => insert_before_heading_in(dest_content, position.1, section_text)?,
            "after" => insert_after_heading_in(dest_content, position.1, section_text)?,
            _ => return None,
        };
        Some((new_source, new_dest))
    }
}

pub fn replace_section_in(content: &str, heading: &str, replacement: &str) -> Option<String> {
    let (body_start, body_end) = find_section(content, heading)?;
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..body_start]);
    if !replacement.is_empty() {
        out.push_str(replacement);
        if !replacement.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&content[body_end..]);
    Some(out)
}

pub fn insert_after_heading_in(content: &str, heading: &str, insertion: &str) -> Option<String> {
    let (body_start, _) = find_section(content, heading)?;
    let mut out = String::with_capacity(content.len() + insertion.len());
    out.push_str(&content[..body_start]);
    out.push_str(insertion);
    if !insertion.is_empty() && !insertion.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&content[body_start..]);
    Some(out)
}

pub fn insert_before_heading_in(content: &str, heading: &str, insertion: &str) -> Option<String> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let query = normalize_heading_query(heading);

    for h in &headings {
        if h.text.trim() == query {
            let heading_start = offsets[h.line_start];
            let mut out = String::with_capacity(content.len() + insertion.len());
            out.push_str(&content[..heading_start]);
            if !insertion.is_empty() {
                out.push_str(insertion);
                if !insertion.ends_with('\n') {
                    out.push('\n');
                }
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
            }
            out.push_str(&content[heading_start..]);
            return Some(out);
        }
    }
    None
}

pub fn upsert_bullet_in(content: &str, heading: &str, bullet: &str) -> Option<String> {
    let (body_start, body_end) = find_section(content, heading)?;
    let body = &content[body_start..body_end];

    let trimmed = bullet.trim();
    let normalized = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        trimmed.to_string()
    } else {
        format!("- {trimmed}")
    };

    for line in body.lines() {
        if line.trim() == normalized {
            return Some(content.to_string());
        }
    }

    let mut out = String::with_capacity(content.len() + normalized.len() + 2);
    out.push_str(&content[..body_end]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&normalized);
    out.push('\n');
    out.push_str(&content[body_end..]);
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
    t[1..t.len() - 1]
        .chars()
        .all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

pub fn table_append_in(
    content: &str,
    body_start: usize,
    body_end: usize,
    row: &str,
) -> Option<String> {
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

    let insert_pos = last_data_end?;

    let mut out = String::with_capacity(content.len() + row.len() + 2);
    out.push_str(&content[..insert_pos]);
    out.push_str(row);
    if !row.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&content[insert_pos..]);
    Some(out)
}

pub fn table_append_for_tx(content: &str, heading: &str, row: &str) -> Option<String> {
    let (body_start, body_end) = find_section(content, heading)?;
    table_append_in(content, body_start, body_end, row)
}

/// Strip inline code spans (between backticks) from a line so that
/// we don't false-positive on code examples.
pub(crate) fn strip_inline_code(line: &str) -> Cow<'_, str> {
    if !line.contains('`') {
        return Cow::Borrowed(line);
    }
    let mut result = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        result.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('`') {
            rest = &after_open[close + 1..];
        } else {
            // Unmatched backtick — keep the rest as-is.
            rest = after_open;
            break;
        }
    }
    result.push_str(rest);
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

#[cfg(test)]
mod tests {
    // ── md module tests ───────────────────────────────────────────────
    mod md_tests {
        use crate::ops::md::*;

        #[test]
        fn parse_headings_basic() {
            let content = "# H1\ntext\n## H2\nmore\n# H1b\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 3);
            assert_eq!(headings[0].level, 1);
            assert_eq!(headings[0].text, "H1");
            assert_eq!(headings[1].level, 2);
            assert_eq!(headings[1].text, "H2");
            assert_eq!(headings[2].level, 1);
            assert_eq!(headings[2].text, "H1b");
        }

        #[test]
        fn parse_headings_skips_fenced_code_blocks() {
            let content = "# Real\n```\n# Fake\n```\n## Also Real\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Real");
            assert_eq!(headings[1].text, "Also Real");
        }

        #[test]
        fn parse_headings_skips_indented_fenced_code_blocks() {
            // CommonMark allows up to 3 spaces of indentation before fence markers.
            let content = "# Real\n   ```\n# Fake inside indented fence\n   ```\n## Also Real\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Real");
            assert_eq!(headings[1].text, "Also Real");

            // 4 spaces is NOT a fence opener (indented code block instead)
            let content4 = "# Real\n    ```\n# Still Real\n    ```\n";
            let headings4 = parse_headings(content4);
            assert_eq!(headings4.len(), 2);
            assert_eq!(headings4[0].text, "Real");
            assert_eq!(headings4[1].text, "Still Real");

            // Indented tilde fences
            let tilde = "# Top\n  ~~~\n# Fake\n  ~~~\n# Bottom\n";
            let headings_tilde = parse_headings(tilde);
            assert_eq!(headings_tilde.len(), 2);
            assert_eq!(headings_tilde[0].text, "Top");
            assert_eq!(headings_tilde[1].text, "Bottom");
        }

        #[test]
        fn parse_headings_longer_fence_requires_matching_length() {
            // A 4-backtick fence is only closed by 4+ backticks, not 3.
            let content = "# Top\n````\n```\n# Fake\n```\n````\n# Bottom\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Top");
            assert_eq!(headings[1].text, "Bottom");

            // A 5-tilde fence requires 5+ tildes to close
            let tilde5 = "# Top\n~~~~~\n~~~\n# Fake\n~~~\n~~~~~\n# Bottom\n";
            let headings5 = parse_headings(tilde5);
            assert_eq!(headings5.len(), 2);
            assert_eq!(headings5[0].text, "Top");
            assert_eq!(headings5[1].text, "Bottom");
        }

        #[test]
        fn parse_headings_mixed_fence_markers() {
            // ~~~ inside a ``` block is content, not a closer
            let content = "# Top\n```\n~~~\n# Not Real\n~~~\n```\n# Bottom\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Top");
            assert_eq!(headings[1].text, "Bottom");
        }

        #[test]
        fn parse_headings_skips_tilde_fenced_blocks() {
            let content = "# Top\n~~~bash\n# Not a heading\n~~~\n# Bottom\n";
            let headings = parse_headings(content);
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Top");
            assert_eq!(headings[1].text, "Bottom");
        }

        #[test]
        fn parse_headings_section_boundaries() {
            // ## B (level 2) does NOT end # A (level 1); only same-or-higher level ends it
            let content = "# A\nline1\nline2\n## B\nline3\n";
            let headings = parse_headings(content);
            assert_eq!(headings[0].line_start, 0);
            assert_eq!(headings[0].line_end, 5); // # A owns everything (no same-level heading)
            assert_eq!(headings[1].line_start, 3);
            assert_eq!(headings[1].line_end, 5); // ## B to end of content

            // Two same-level headings: second ends first
            let content2 = "# A\nbody\n# B\nmore\n";
            let h2 = parse_headings(content2);
            assert_eq!(h2[0].line_end, 2); // # A ends at # B
            assert_eq!(h2[1].line_end, 4); // # B to end
        }

        #[test]
        fn parse_headings_ignores_invalid() {
            let content = "#nospace\n##also\n# Valid\n###### Six\n####### Seven\n";
            let headings = parse_headings(content);
            // Only "# Valid" and "###### Six" are valid (Seven > 6 levels)
            assert_eq!(headings.len(), 2);
            assert_eq!(headings[0].text, "Valid");
            assert_eq!(headings[1].text, "Six");
        }

        #[test]
        fn find_section_returns_body_bytes() {
            // ## Next is deeper than # Title, so it's part of the section body
            let content = "# Title\nBody line 1\nBody line 2\n## Next\n";
            let (start, end) = find_section(content, "Title").unwrap();
            let body = &content[start..end];
            assert_eq!(body, "Body line 1\nBody line 2\n## Next\n");

            // Same-level heading ends the section
            let content2 = "# Title\nBody\n# Other\nKeep\n";
            let (s2, e2) = find_section(content2, "Title").unwrap();
            assert_eq!(&content2[s2..e2], "Body\n");
        }

        #[test]
        fn find_section_with_hashes_in_query() {
            let content = "## API\nsome text\n";
            let (start, end) = find_section(content, "## API").unwrap();
            assert_eq!(&content[start..end], "some text\n");
        }

        #[test]
        fn find_section_missing() {
            let content = "# Title\nBody\n";
            assert!(find_section(content, "Nonexistent").is_none());
        }

        #[test]
        fn replace_section_basic() {
            // Use same-level heading so section boundary is clear
            let content = "# Title\nOld body\n# Next\nKeep\n";
            let result = replace_section_in(content, "Title", "New body").unwrap();
            assert_eq!(result, "# Title\nNew body\n# Next\nKeep\n");
        }

        #[test]
        fn replace_section_empty_replacement() {
            let content = "# Title\nOld body\n# Next\nKeep\n";
            let result = replace_section_in(content, "Title", "").unwrap();
            assert_eq!(result, "# Title\n# Next\nKeep\n");
        }

        #[test]
        fn replace_section_missing_heading() {
            let content = "# Title\nBody\n";
            assert!(replace_section_in(content, "Missing", "x").is_none());
        }

        #[test]
        fn insert_after_heading() {
            let content = "# Title\nExisting\n";
            let result = insert_after_heading_in(content, "Title", "Inserted\n").unwrap();
            assert_eq!(result, "# Title\nInserted\nExisting\n");
        }

        #[test]
        fn insert_before_heading() {
            let content = "# First\nBody\n## Second\nMore\n";
            let result = insert_before_heading_in(content, "Second", "Inserted").unwrap();
            assert!(result.contains("Inserted\n\n## Second"));
        }

        #[test]
        fn upsert_bullet_adds_new() {
            let content = "# List\n- item1\n";
            let result = upsert_bullet_in(content, "List", "- item2").unwrap();
            assert!(result.contains("- item1\n- item2\n"));
        }

        #[test]
        fn upsert_bullet_dedup_existing() {
            let content = "# List\n- item1\n";
            let result = upsert_bullet_in(content, "List", "- item1").unwrap();
            // Should return content unchanged (no duplicate)
            assert_eq!(result, content);
        }

        #[test]
        fn upsert_bullet_auto_prefix() {
            let content = "# List\n- a\n";
            let result = upsert_bullet_in(content, "List", "new item").unwrap();
            assert!(result.contains("- new item\n"));
        }

        #[test]
        fn dedupe_headings_removes_duplicate() {
            let content = "# Title\nFirst\n# Title\nSecond\n";
            let (result, removed) = dedupe_headings_in(content);
            assert_eq!(removed, vec!["# Title"]);
            // First occurrence kept, second removed
            assert!(result.contains("First"));
            assert!(!result.contains("Second"));
        }

        #[test]
        fn dedupe_headings_no_duplicates() {
            let content = "# A\n## B\n# C\n";
            let (result, removed) = dedupe_headings_in(content);
            assert!(removed.is_empty());
            assert_eq!(result, content);
        }

        #[test]
        fn table_append_basic() {
            let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n## Next\n";
            let (start, end) = find_section(content, "API").unwrap();
            let result = table_append_in(content, start, end, "| b | 2 |").unwrap();
            assert!(result.contains("| a | 1 |\n| b | 2 |\n## Next"));
        }

        #[test]
        fn table_append_header_only() {
            let content = "# API\n| Name | Value |\n|---|---|\n## Next\n";
            let (start, end) = find_section(content, "API").unwrap();
            let result = table_append_in(content, start, end, "| a | 1 |").unwrap();
            assert_eq!(
                result,
                "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n## Next\n"
            );
        }

        #[test]
        fn table_append_no_table() {
            let content = "# API\nJust text\n";
            let (start, end) = find_section(content, "API").unwrap();
            assert!(table_append_in(content, start, end, "| b | 2 |").is_none());
        }

        #[test]
        fn table_append_for_tx_basic() {
            let content = "# API\n| Name | Value |\n|---|---|\n| a | 1 |\n";
            let result = table_append_for_tx(content, "API", "| b | 2 |").unwrap();
            assert!(result.contains("| a | 1 |\n| b | 2 |\n"));
        }

        #[test]
        fn section_range_includes_heading() {
            let content = "# A\nbody a\n# B\nbody b\n";
            let (start, end) = section_range(content, "A").unwrap();
            assert_eq!(&content[start..end], "# A\nbody a\n");
        }

        #[test]
        fn section_range_last_section() {
            let content = "# A\nbody a\n# B\nbody b\n";
            let (start, end) = section_range(content, "B").unwrap();
            assert_eq!(&content[start..end], "# B\nbody b\n");
        }

        #[test]
        fn section_range_missing() {
            let content = "# A\nbody\n";
            assert!(section_range(content, "Missing").is_none());
        }

        #[test]
        fn move_section_same_file_before() {
            let content = "# A\na body\n# B\nb body\n# C\nc body\n";
            let (result, _) =
                move_section_in(content, "C", content, ("before", "B"), true).unwrap();
            let a_pos = result.find("# A").unwrap();
            let c_pos = result.find("# C").unwrap();
            let b_pos = result.find("# B").unwrap();
            assert!(a_pos < c_pos);
            assert!(c_pos < b_pos);
            assert!(result.contains("a body"));
            assert!(result.contains("b body"));
            assert!(result.contains("c body"));
        }

        #[test]
        fn move_section_same_file_after() {
            let content = "# A\na body\n# B\nb body\n# C\nc body\n";
            let (result, _) = move_section_in(content, "A", content, ("after", "B"), true).unwrap();
            let b_pos = result.find("# B").unwrap();
            let a_pos = result.find("# A").unwrap();
            let c_pos = result.find("# C").unwrap();
            assert!(b_pos < a_pos);
            assert!(a_pos < c_pos);
        }

        #[test]
        fn move_section_cross_file() {
            let source = "# Keep\nkept\n# Move\nmoved\n# Stay\nstayed\n";
            let dest = "# Intro\nintro\n# End\nend\n";
            let (new_src, new_dst) =
                move_section_in(source, "Move", dest, ("before", "End"), false).unwrap();
            assert!(!new_src.contains("# Move"));
            assert!(!new_src.contains("moved"));
            assert!(new_src.contains("# Keep"));
            assert!(new_src.contains("# Stay"));
            assert!(new_dst.contains("# Move\nmoved"));
            let move_pos = new_dst.find("# Move").unwrap();
            let end_pos = new_dst.find("# End").unwrap();
            assert!(move_pos < end_pos);
        }

        #[test]
        fn move_section_missing_source_heading() {
            let content = "# A\nbody\n";
            assert!(move_section_in(content, "Missing", content, ("before", "A"), true).is_none());
        }

        #[test]
        fn move_section_missing_target_heading() {
            let content = "# A\nbody\n# B\nbody\n";
            assert!(move_section_in(content, "A", content, ("before", "Missing"), true).is_none());
        }
    }
}
