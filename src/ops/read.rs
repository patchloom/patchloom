#[allow(unused_imports)]
use anyhow::bail;

#[cfg(any(feature = "cli", feature = "files"))]
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
#[cfg(any(feature = "cli", feature = "files"))]
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
            anyhow::bail!("missing start line in range '{spec}' (expected START:END)");
        }
        let start: usize = start_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid start line: {start_str}"))?;
        if start == 0 {
            anyhow::bail!("line numbers are 1-based, got 0");
        }
        if end_str.is_empty() {
            return Ok((start, None));
        }
        let end: usize = end_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid end line: {end_str}"))?;
        if end == 0 {
            anyhow::bail!("line numbers are 1-based, got 0");
        }
        if end < start {
            anyhow::bail!("end line {end} is before start line {start}");
        }
        Ok((start, Some(end)))
    } else {
        let start: usize = spec
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid line number: {spec}"))?;
        if start == 0 {
            anyhow::bail!("line numbers are 1-based, got 0");
        }
        Ok((start, Some(start)))
    }
}

/// Select a range of lines (1-based). Normalizes line endings to LF in output.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn select_lines(content: &str, lines: LineRange) -> SelectedLines {
    let all_lines: Vec<&str> = content.lines().collect();
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
    let add_nl = end_idx == total_lines && content.ends_with('\n');
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
        assert!(parse_line_range("0").is_err());
        assert!(parse_line_range("0:5").is_err());
    }

    #[test]
    fn parse_end_before_start_errors() {
        assert!(parse_line_range("10:5").is_err());
    }

    #[test]
    fn parse_missing_start_errors() {
        assert!(parse_line_range(":5").is_err());
    }

    #[test]
    fn parse_open_ended_range() {
        assert_eq!(parse_line_range("5:").unwrap(), (5, None));
        assert_eq!(parse_line_range("1:").unwrap(), (1, None));
    }

    #[test]
    fn parse_open_ended_zero_start_errors() {
        assert!(parse_line_range("0:").is_err());
    }

    #[test]
    fn parse_non_numeric_errors() {
        assert!(parse_line_range("abc").is_err());
        assert!(parse_line_range("1:abc").is_err());
    }

    #[test]
    fn parse_end_zero_errors() {
        // R4 fix: end=0 in a range like "1:0" must be rejected (1-based).
        let err = parse_line_range("1:0").unwrap_err();
        assert!(
            err.to_string().contains("1-based"),
            "expected 1-based error, got: {err}"
        );
        // Also check "5:0"
        assert!(parse_line_range("5:0").is_err());
    }

    #[test]
    fn parse_same_start_end() {
        assert_eq!(parse_line_range("1:1").unwrap(), (1, Some(1)));
    }

    #[test]
    fn parse_negative_dash_is_not_range() {
        // A leading dash is not a separator, so "-5" should fail as invalid number.
        assert!(parse_line_range("-5").is_err());
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
}
