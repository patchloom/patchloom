pub(crate) type LineRange = (usize, Option<usize>);

#[cfg(any(feature = "cli", feature = "files"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedLines {
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
}

#[cfg(any(feature = "cli", feature = "files"))]
impl SelectedLines {
    pub fn empty(total_lines: usize) -> Self {
        Self {
            content: String::new(),
            start_line: 0,
            end_line: 0,
            total_lines,
        }
    }
}

/// Parse a line range spec like "10", "10:20", "10-20".
pub(crate) fn parse_line_range(spec: &str) -> anyhow::Result<LineRange> {
    // Accept both ':' and '-' as range separators so `--lines 1:10` and
    // `--lines 1-10` both work (the help examples show the dash form).
    let sep = if spec.contains(':') {
        Some(':')
    } else if spec.contains('-') && !spec.starts_with('-') {
        Some('-')
    } else {
        None
    };
    if let Some(sep) = sep
        && let Some((start_str, end_str)) = spec.split_once(sep)
    {
        if start_str.is_empty() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("missing start line in range '{spec}' (expected START:END)"),
            }));
        }
        let start: usize = start_str.parse().map_err(|_| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("invalid start line: {start_str}"),
            })
        })?;
        if start == 0 {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "line numbers are 1-based, got 0".into(),
            }));
        }
        if end_str.is_empty() {
            return Ok((start, None));
        }
        let end: usize = end_str.parse().map_err(|_| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("invalid end line: {end_str}"),
            })
        })?;
        if end == 0 {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "line numbers are 1-based, got 0".into(),
            }));
        }
        if end < start {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("end line {end} is before start line {start}"),
            }));
        }
        Ok((start, Some(end)))
    } else {
        let start: usize = spec.parse().map_err(|_| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("invalid line number: {spec}"),
            })
        })?;
        if start == 0 {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "line numbers are 1-based, got 0".into(),
            }));
        }
        Ok((start, Some(start)))
    }
}

/// Select a range of lines (1-based). Normalizes line endings to LF in output.
///
/// Line ends match search / replace / md: LF, CRLF, and a lone CR
/// ([`crate::ops::file::text_lines`]). [`str::lines`] does not split on CR.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn select_lines(content: &str, lines: LineRange) -> SelectedLines {
    let all_lines: Vec<&str> = crate::ops::file::text_lines(content).collect();
    let total_lines = all_lines.len();
    if total_lines == 0 {
        return SelectedLines::empty(total_lines);
    }

    let (start, end) = lines;
    if start == 0 || start > total_lines {
        return SelectedLines::empty(total_lines);
    }
    let start_idx = start - 1;
    let end_idx = match end {
        Some(e) => e.min(total_lines),
        None => total_lines,
    };

    if start_idx >= end_idx {
        return SelectedLines::empty(total_lines);
    }

    let selected: Vec<&str> = all_lines[start_idx..end_idx].to_vec();
    let joined = selected.join("\n");
    // Join-to-LF output. Restore a terminator on the last selected line
    // when the source ended with LF, CRLF, or a lone CR.
    let add_nl = end_idx == total_lines && (content.ends_with('\n') || content.ends_with('\r'));
    let out = if add_nl && !joined.is_empty() {
        joined + "\n"
    } else {
        joined
    };
    SelectedLines {
        content: out,
        start_line: start_idx + 1,
        end_line: end_idx,
        total_lines,
    }
}

/// SHA-256 of the file bytes, lowercase hex. Whole-file, even when a
/// line slice is returned, so a later write can use it as `expected_sha256`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub(crate) fn hashes_match(content: &str, expected: &str) -> bool {
    let want = expected.trim();
    if want.len() != 64 || !want.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    sha256_hex(content.as_bytes()).eq_ignore_ascii_case(want)
}

/// Turn `lines` plus Claude/Cursor-style aliases into one `START:END` spec.
///
/// `offset` is the 1-based first line. `limit` is how many lines. `start_line`
/// and `end_line` are 1-based inclusive, the same as `lines`. No default cap.
/// Passing both `lines` and an alias is fine when they name the same range.
pub(crate) fn resolve_read_lines(
    lines: Option<&str>,
    offset: Option<u64>,
    limit: Option<u64>,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> anyhow::Result<Option<String>> {
    let alias = alias_spec(offset, limit, start_line, end_line)?;
    match (lines.map(str::trim).filter(|s| !s.is_empty()), alias) {
        (None, None) => Ok(None),
        (Some(spec), None) => Ok(Some(spec.to_string())),
        (None, Some(spec)) => Ok(Some(spec)),
        (Some(spec), Some(alias)) => {
            let left = parse_line_range(spec)?;
            let right = parse_line_range(&alias)?;
            if left != right {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!(
                        "lines '{spec}' disagrees with offset/limit or start_line/end_line ({alias})"
                    ),
                }));
            }
            Ok(Some(spec.to_string()))
        }
    }
}

fn alias_spec(
    offset: Option<u64>,
    limit: Option<u64>,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> anyhow::Result<Option<String>> {
    fn bad(msg: &str) -> anyhow::Error {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: msg.to_string(),
        })
    }
    fn one_based(n: u64, name: &str) -> anyhow::Result<u64> {
        if n == 0 {
            return Err(bad(&format!("{name} is 1-based, got 0")));
        }
        Ok(n)
    }
    match (offset, limit, start_line, end_line) {
        (None, None, None, None) => Ok(None),
        (Some(off), lim, None, None) => {
            let start = one_based(off, "offset")?;
            if let Some(n) = lim {
                if n == 0 {
                    return Err(bad("limit must be at least 1"));
                }
                let end = start
                    .checked_add(n - 1)
                    .ok_or_else(|| bad("offset+limit overflows"))?;
                Ok(Some(format!("{start}:{end}")))
            } else {
                Ok(Some(format!("{start}:")))
            }
        }
        (None, Some(n), None, None) => {
            if n == 0 {
                return Err(bad("limit must be at least 1"));
            }
            Ok(Some(format!("1:{n}")))
        }
        (None, None, start, end) => match (start, end) {
            (Some(s), Some(e)) => {
                let s = one_based(s, "start_line")?;
                let e = one_based(e, "end_line")?;
                Ok(Some(format!("{s}:{e}")))
            }
            (Some(s), None) => Ok(Some(format!("{}:", one_based(s, "start_line")?))),
            (None, Some(e)) => Ok(Some(format!("1:{}", one_based(e, "end_line")?))),
            (None, None) => Ok(None),
        },
        _ => Err(bad("pass offset/limit or start_line/end_line, not both")),
    }
}

#[cfg(test)]
#[cfg(any(feature = "cli", feature = "files"))]
mod tests {
    use super::*;

    // ---- parse_line_range ----

    #[test]
    fn parse_single_line() {
        assert_eq!(parse_line_range("5").unwrap(), (5, Some(5)));
    }

    #[test]
    fn parse_colon_range() {
        assert_eq!(parse_line_range("3:7").unwrap(), (3, Some(7)));
    }

    #[test]
    fn parse_dash_range() {
        assert_eq!(parse_line_range("10-20").unwrap(), (10, Some(20)));
    }

    #[test]
    fn parse_zero_start_errors() {
        let err = parse_line_range("0").unwrap_err();
        assert!(err.to_string().contains("1-based"), "got: {err}");
        assert!(
            crate::exit::is_invalid_input(&err),
            "zero line should be invalid_input: {err}"
        );
        let err = parse_line_range("0:5").unwrap_err();
        assert!(err.to_string().contains("1-based"), "got: {err}");
        assert!(crate::exit::is_invalid_input(&err));
    }

    #[test]
    fn parse_end_before_start_errors() {
        let err = parse_line_range("10:5").unwrap_err().to_string();
        assert!(
            err.contains("end line 5 is before start line 10"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_missing_start_errors() {
        let err = parse_line_range(":5").unwrap_err().to_string();
        assert!(err.contains("missing start line"), "got: {err}");
    }

    #[test]
    fn parse_open_ended_range() {
        assert_eq!(parse_line_range("5:").unwrap(), (5, None));
        assert_eq!(parse_line_range("1:").unwrap(), (1, None));
    }

    #[test]
    fn parse_open_ended_zero_start_errors() {
        let err = parse_line_range("0:").unwrap_err().to_string();
        assert!(err.contains("1-based"), "got: {err}");
    }

    #[test]
    fn parse_non_numeric_errors() {
        let err = parse_line_range("abc").unwrap_err().to_string();
        assert!(err.contains("invalid line number"), "got: {err}");
        let err = parse_line_range("1:abc").unwrap_err().to_string();
        assert!(err.contains("invalid end line"), "got: {err}");
    }

    #[test]
    fn parse_end_zero_errors() {
        // R4 fix: end=0 in a range like "1:0" must be rejected (1-based).
        let err = parse_line_range("1:0").unwrap_err();
        assert!(
            err.to_string().contains("1-based"),
            "expected 1-based error, got: {err}"
        );
        let err = parse_line_range("5:0").unwrap_err().to_string();
        assert!(err.contains("1-based"), "got: {err}");
    }

    #[test]
    fn parse_same_start_end() {
        assert_eq!(parse_line_range("1:1").unwrap(), (1, Some(1)));
    }

    #[test]
    fn offset_limit_matches_claude_style_window() {
        let spec = resolve_read_lines(None, Some(10), Some(5), None, None)
            .unwrap()
            .unwrap();
        assert_eq!(spec, "10:14");
        assert_eq!(parse_line_range(&spec).unwrap(), (10, Some(14)));
    }

    #[test]
    fn start_line_end_line_is_inclusive() {
        let spec = resolve_read_lines(None, None, None, Some(2), Some(4))
            .unwrap()
            .unwrap();
        assert_eq!(spec, "2:4");
    }

    #[test]
    fn lines_and_alias_must_agree() {
        let err = resolve_read_lines(Some("1:2"), Some(3), Some(1), None, None).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        let ok = resolve_read_lines(Some("10:14"), Some(10), Some(5), None, None).unwrap();
        assert_eq!(ok.as_deref(), Some("10:14"));
    }

    #[test]
    fn sha256_is_stable_lowercase_hex() {
        let hex = sha256_hex(b"hello\n");
        assert_eq!(hex.len(), 64);
        assert!(hashes_match("hello\n", &hex));
        assert!(hashes_match("hello\n", &hex.to_uppercase()));
        assert!(!hashes_match("hello\n", "abc"));
    }

    #[test]
    fn parse_negative_dash_is_not_range() {
        // A leading dash is not a separator, so "-5" should fail as invalid number.
        let err = parse_line_range("-5").unwrap_err().to_string();
        assert!(err.contains("invalid line number"), "got: {err}");
    }

    // ---- select_lines ----

    #[test]
    fn select_single_line() {
        let content = "aaa\nbbb\nccc\n";
        let result = select_lines(content, (2, Some(2)));
        assert_eq!(result.content, "bbb");
        assert_eq!(result.start_line, 2);
        assert_eq!(result.end_line, 2);
        assert_eq!(result.total_lines, 3);
    }

    #[test]
    fn select_range() {
        let content = "line1\nline2\nline3\nline4\n";
        let result = select_lines(content, (2, Some(3)));
        assert_eq!(result.content, "line2\nline3");
        assert_eq!(result.start_line, 2);
        assert_eq!(result.end_line, 3);
    }

    #[test]
    fn select_last_line_preserves_trailing_newline() {
        let content = "aaa\nbbb\nccc\n";
        let result = select_lines(content, (3, Some(3)));
        assert_eq!(result.content, "ccc\n");
        assert_eq!(result.end_line, 3);
    }

    #[test]
    fn select_all_lines() {
        let content = "a\nb\nc\n";
        let result = select_lines(content, (1, None));
        assert_eq!(result.content, "a\nb\nc\n");
        assert_eq!(result.start_line, 1);
        assert_eq!(result.end_line, 3);
    }

    #[test]
    fn select_open_ended_from_middle() {
        let content = "a\nb\nc\nd\n";
        let result = select_lines(content, (3, None));
        assert_eq!(result.content, "c\nd\n");
        assert_eq!(result.start_line, 3);
        assert_eq!(result.end_line, 4);
    }

    #[test]
    fn select_empty_content() {
        let result = select_lines("", (1, Some(1)));
        assert_eq!(result, SelectedLines::empty(0));
    }

    #[test]
    fn select_start_beyond_total() {
        let content = "one\ntwo\n";
        let result = select_lines(content, (10, Some(20)));
        assert_eq!(result, SelectedLines::empty(2));
    }

    #[test]
    fn select_start_zero_returns_empty() {
        let content = "one\ntwo\n";
        let result = select_lines(content, (0, Some(1)));
        assert_eq!(result, SelectedLines::empty(2));
    }

    #[test]
    fn select_end_clamped_to_total() {
        let content = "a\nb\nc\n";
        let result = select_lines(content, (2, Some(100)));
        assert_eq!(result.content, "b\nc\n");
        assert_eq!(result.end_line, 3);
    }

    #[test]
    fn select_no_trailing_newline() {
        let content = "alpha\nbeta";
        let result = select_lines(content, (1, Some(2)));
        // No trailing newline in source, so none added.
        assert_eq!(result.content, "alpha\nbeta");
    }

    #[test]
    fn select_mixed_cr_lf_agrees_with_text_lines() {
        // Search numbers this as a / b / c / d. str::lines() used to
        // treat "b\rc" as one line so --lines 3 returned d (#2334).
        let content = "a\nb\rc\r\nd\n";
        let third = select_lines(content, (3, Some(3)));
        assert_eq!(third.content, "c");
        assert_eq!(third.start_line, 3);
        assert_eq!(third.end_line, 3);
        assert_eq!(third.total_lines, 4);

        let all = select_lines(content, (1, None));
        assert_eq!(all.content, "a\nb\nc\nd\n");
        assert_eq!(all.total_lines, 4);
        assert_eq!(all.end_line, 4);
    }

    #[test]
    fn select_last_line_cr_only_restores_lf_terminator() {
        let content = "end\rnext\r";
        let result = select_lines(content, (2, Some(2)));
        assert_eq!(result.content, "next\n");
        assert_eq!(result.total_lines, 2);
    }
}
