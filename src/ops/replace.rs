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
            Self::Mode(e) => match e {
                ReplaceModeError::MissingMode => {
                    write!(
                        f,
                        "one of to, insert_before, or insert_after must be provided"
                    )
                }
                ReplaceModeError::BothInsertModes => {
                    write!(f, "insert_before and insert_after cannot be combined")
                }
                ReplaceModeError::ToWithInsert => {
                    write!(
                        f,
                        "to cannot be combined with insert_before or insert_after"
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
    validate_replace_mode(p.has_to, p.has_insert_before, p.has_insert_after)
        .map_err(ReplaceValidationError::Mode)
}

pub fn replacement_text(
    from: &str,
    to: &Option<String>,
    insert_before: &Option<String>,
    insert_after: &Option<String>,
    use_match_anchor: bool,
) -> String {
    let anchor = if use_match_anchor { "${0}" } else { from };

    if let Some(text) = insert_before {
        return format!("{text}{anchor}");
    }

    if let Some(text) = insert_after {
        return format!("{anchor}{text}");
    }

    to.clone().unwrap_or_default()
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

        // Find line boundary.
        let (line_content, line_with_ending, advance) = if let Some(pos) = rest.find('\n') {
            (&rest[..pos], &rest[..=pos], pos + 1)
        } else {
            (rest, rest, rest.len())
        };

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
            if line_with_ending.len() > line_content.len() {
                result.push('\n');
            }
        } else {
            // Literal match: replacement is used as-is.
            result.push_str(to);
            if line_with_ending.len() > line_content.len() {
                result.push('\n');
            }
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

#[cfg(test)]
mod tests {
    // ── replace module tests ──────────────────────────────────────────
    mod replace_tests {
        use crate::ops::replace::*;

        #[test]
        fn validate_mode_missing() {
            assert_eq!(
                validate_replace_mode(false, false, false),
                Err(ReplaceModeError::MissingMode)
            );
        }

        #[test]
        fn validate_mode_both_inserts() {
            assert_eq!(
                validate_replace_mode(false, true, true),
                Err(ReplaceModeError::BothInsertModes)
            );
        }

        #[test]
        fn validate_mode_to_with_insert() {
            assert_eq!(
                validate_replace_mode(true, true, false),
                Err(ReplaceModeError::ToWithInsert)
            );
            assert_eq!(
                validate_replace_mode(true, false, true),
                Err(ReplaceModeError::ToWithInsert)
            );
        }

        #[test]
        fn validate_mode_valid_to_only() {
            validate_replace_mode(true, false, false).unwrap();
        }

        #[test]
        fn validate_mode_valid_insert_before_only() {
            validate_replace_mode(false, true, false).unwrap();
        }

        #[test]
        fn validate_mode_valid_insert_after_only() {
            validate_replace_mode(false, false, true).unwrap();
        }

        // ── validate_replace_args tests ───────────────────────────────

        fn valid_params() -> ReplaceValidationParams<'static> {
            ReplaceValidationParams {
                pattern: "needle",
                has_to: true,
                has_insert_before: false,
                has_insert_after: false,
                nth: None,
                whole_line: false,
                multiline: false,
                has_range: false,
            }
        }

        #[test]
        fn validate_args_valid_basic() {
            validate_replace_args(&valid_params()).unwrap();
        }

        #[test]
        fn validate_args_empty_pattern() {
            let mut p = valid_params();
            p.pattern = "";
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::EmptyPattern)
            );
        }

        #[test]
        fn validate_args_nth_zero() {
            let mut p = valid_params();
            p.nth = Some(0);
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::NthZero)
            );
        }

        #[test]
        fn validate_args_nth_one_ok() {
            let mut p = valid_params();
            p.nth = Some(1);
            validate_replace_args(&p).unwrap();
        }

        #[test]
        fn validate_args_range_requires_whole_line() {
            let mut p = valid_params();
            p.has_range = true;
            p.whole_line = false;
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::RangeRequiresWholeLine)
            );
        }

        #[test]
        fn validate_args_range_with_whole_line_ok() {
            let mut p = valid_params();
            p.has_range = true;
            p.whole_line = true;
            validate_replace_args(&p).unwrap();
        }

        #[test]
        fn validate_args_whole_line_multiline_conflict() {
            let mut p = valid_params();
            p.whole_line = true;
            p.multiline = true;
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::WholeLineMultilineConflict)
            );
        }

        #[test]
        fn validate_args_mode_error_propagated() {
            let p = ReplaceValidationParams {
                pattern: "needle",
                has_to: false,
                has_insert_before: false,
                has_insert_after: false,
                nth: None,
                whole_line: false,
                multiline: false,
                has_range: false,
            };
            assert_eq!(
                validate_replace_args(&p),
                Err(ReplaceValidationError::Mode(ReplaceModeError::MissingMode))
            );
        }

        #[test]
        fn validate_args_display_messages() {
            // Verify Display impl produces expected human-readable messages.
            assert!(
                ReplaceValidationError::EmptyPattern
                    .to_string()
                    .contains("search pattern must not be empty")
            );
            assert!(ReplaceValidationError::NthZero.to_string().contains("nth"));
            assert!(
                ReplaceValidationError::RangeRequiresWholeLine
                    .to_string()
                    .contains("range requires whole_line")
            );
            assert!(
                ReplaceValidationError::WholeLineMultilineConflict
                    .to_string()
                    .contains("whole_line and multiline")
            );
            assert!(
                ReplaceValidationError::Mode(ReplaceModeError::MissingMode)
                    .to_string()
                    .contains("to, insert_before, or insert_after")
            );
        }

        #[test]
        fn replacement_text_with_to() {
            let result = replacement_text("from", &Some("to".into()), &None, &None, false);
            assert_eq!(result, "to");
        }

        #[test]
        fn replacement_text_insert_before_literal() {
            let result =
                replacement_text("original", &None, &Some("PREFIX\n".into()), &None, false);
            assert_eq!(result, "PREFIX\noriginal");
        }

        #[test]
        fn replacement_text_insert_after_literal() {
            let result =
                replacement_text("original", &None, &None, &Some("\nSUFFIX".into()), false);
            assert_eq!(result, "original\nSUFFIX");
        }

        #[test]
        fn replacement_text_insert_before_regex_anchor() {
            let result = replacement_text("ignored", &None, &Some("PREFIX\n".into()), &None, true);
            assert_eq!(result, "PREFIX\n${0}");
        }

        #[test]
        fn replacement_text_insert_after_regex_anchor() {
            let result = replacement_text("ignored", &None, &None, &Some("\nSUFFIX".into()), true);
            assert_eq!(result, "${0}\nSUFFIX");
        }

        #[test]
        fn replace_content_literal_all() {
            let (out, count) = replace_content("aXbXc", "X", "Y", None, None);
            assert_eq!(out, "aYbYc");
            assert_eq!(count, 2);
        }

        #[test]
        fn replace_content_literal_no_match() {
            let (out, count) = replace_content("hello", "zzz", "y", None, None);
            assert_eq!(out, "hello");
            assert_eq!(count, 0);
            // Cow optimization: no-match must return Borrowed, not a cloned String.
            assert!(
                matches!(out, std::borrow::Cow::Borrowed(_)),
                "expected Cow::Borrowed for no-match, got Owned"
            );
        }

        #[test]
        fn replace_content_literal_nth() {
            let (out, count) = replace_content("aXbXcX", "X", "Y", None, Some(2));
            assert_eq!(out, "aXbYcX");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_literal_nth_out_of_range() {
            let (out, count) = replace_content("aXb", "X", "Y", None, Some(5));
            assert_eq!(out, "aXb");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn replace_content_regex_all() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), None);
            assert_eq!(out, "aNbNcN");
            assert_eq!(count, 3);
        }

        #[test]
        fn replace_content_regex_nth() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("a1b22c333", "unused", "N", Some(&re), Some(2));
            assert_eq!(out, "a1bNc333");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_regex_capture_group() {
            let re = regex::Regex::new(r"(\w+)@(\w+)").unwrap();
            let (out, count) = replace_content("user@host", "unused", "$2=$1", Some(&re), None);
            assert_eq!(out, "host=user");
            assert_eq!(count, 1);
        }

        #[test]
        fn replace_content_regex_no_match_returns_borrowed() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("no digits here", "unused", "N", Some(&re), None);
            assert_eq!(out, "no digits here");
            assert_eq!(count, 0);
            assert!(
                matches!(out, std::borrow::Cow::Borrowed(_)),
                "regex no-match should return Cow::Borrowed"
            );
        }

        #[test]
        fn replace_content_empty_from_returns_borrowed() {
            let (out, count) = replace_content("hello", "", "y", None, None);
            assert_eq!(out, "hello");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn replace_content_regex_nth_no_match_returns_borrowed() {
            let re = regex::Regex::new(r"\d+").unwrap();
            let (out, count) = replace_content("no digits here", "unused", "N", Some(&re), Some(1));
            assert_eq!(out, "no digits here");
            assert_eq!(count, 0);
            assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn compile_regex_mode_returns_some() {
            let re = compile_replace_regex(r"\d+", true, false, false, false)
                .unwrap()
                .expect("regex mode should return Some");
            assert!(re.is_match("abc123"));
        }

        #[test]
        fn compile_regex_multiline_dot_matches_newline() {
            let re = compile_replace_regex("a.b", true, false, true, false)
                .unwrap()
                .unwrap();
            assert!(
                re.is_match("a\nb"),
                "multiline should make dot match newline"
            );
        }

        #[test]
        fn compile_invalid_regex_returns_error() {
            let result = compile_replace_regex("(unclosed", true, false, false, false);
            assert!(result.is_err());
        }

        #[test]
        fn compile_literal_case_insensitive_returns_some() {
            let re = compile_replace_regex("hello", false, true, false, false)
                .unwrap()
                .expect("case-insensitive literal should return Some");
            assert!(re.is_match("HELLO"));
        }

        #[test]
        fn compile_plain_literal_returns_none() {
            let re = compile_replace_regex("hello", false, false, false, false).unwrap();
            assert!(re.is_none());
        }

        #[test]
        fn compile_word_boundary_literal() {
            let re = compile_replace_regex("SetupFile", false, false, false, true)
                .unwrap()
                .expect("word_boundary should return Some");
            assert!(re.is_match("SetupFile"), "should match standalone word");
            assert!(
                re.is_match("use crate::SetupFile;"),
                "should match at word boundary"
            );
            assert!(
                !re.is_match("BenchSetupFile"),
                "should NOT match inside a longer word"
            );
            assert!(
                !re.is_match("SetupFileConfig"),
                "should NOT match when followed by more word chars"
            );
        }

        #[test]
        fn compile_word_boundary_escapes_metacharacters() {
            // Test that regex metacharacters in the pattern are properly escaped.
            // Use a pattern with dots and parens that would be regex-special.
            let re = compile_replace_regex("foo.bar()", false, false, false, true)
                .unwrap()
                .expect("word_boundary with metacharacters should return Some");
            // The escaped pattern should NOT match "fooXbar()" (dot is literal)
            assert!(
                !re.is_match("fooXbarX"),
                "dot should be literal, not wildcard"
            );
        }

        #[test]
        fn compile_word_boundary_case_insensitive() {
            let re = compile_replace_regex("setupfile", false, true, false, true)
                .unwrap()
                .expect("word_boundary + case_insensitive should return Some");
            assert!(re.is_match("SetupFile"));
            assert!(!re.is_match("BenchSetupFile"));
        }

        // ── replace_whole_lines tests ─────────────────────────────────

        #[test]
        fn whole_lines_delete_literal() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, None, None);
            assert_eq!(count, 2);
            assert_eq!(&*result, "aaa\nccc\neee\n");
        }

        #[test]
        fn whole_lines_delete_regex() {
            let re = compile_replace_regex(r"let _\w+", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "fn main() {\n    let _x = foo();\n    let y = bar();\n}\n";
            let (result, count) = replace_whole_lines(content, "", "", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "fn main() {\n    let y = bar();\n}\n");
        }

        #[test]
        fn whole_lines_replace_with_text() {
            let content = "alpha\nbeta\ngamma\n";
            let (result, count) =
                replace_whole_lines(content, "beta", "REPLACED", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "alpha\nREPLACED\ngamma\n");
        }

        #[test]
        fn whole_lines_range_restriction() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            // Range 1:3 means lines 1-3 only. Second bbb is on line 4.
            let (result, count) =
                replace_whole_lines(content, "bbb", "", None, None, Some((1, Some(3))));
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nccc\nbbb\neee\n");
        }

        #[test]
        fn whole_lines_nth() {
            let content = "aaa\nbbb\nccc\nbbb\neee\n";
            let (result, count) = replace_whole_lines(content, "bbb", "", None, Some(2), None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nbbb\nccc\neee\n");
        }

        #[test]
        fn whole_lines_no_match_returns_borrowed() {
            let content = "aaa\nbbb\nccc\n";
            let (result, count) = replace_whole_lines(content, "zzz", "", None, None, None);
            assert_eq!(count, 0);
            assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        }

        #[test]
        fn whole_lines_regex_capture_groups() {
            let re = compile_replace_regex(r"version = (\d+)", true, false, false, false)
                .unwrap()
                .unwrap();
            let content = "name = foo\nversion = 3\nrelease = true\n";
            let (result, count) =
                replace_whole_lines(content, "", "version = ${1}00", Some(&re), None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "name = foo\nversion = 300\nrelease = true\n");
        }

        #[test]
        fn whole_lines_last_line_no_newline() {
            let content = "aaa\nbbb\nccc";
            let (result, count) = replace_whole_lines(content, "ccc", "", None, None, None);
            assert_eq!(count, 1);
            assert_eq!(&*result, "aaa\nbbb\n");
        }
    }
}
