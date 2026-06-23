#[allow(unused_imports)]
use anyhow::bail;

pub(crate) type LineRange = (usize, Option<usize>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedLines {
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
}

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

/// Parse a line range spec like "10", "10:20", "10-20", ":20", "10:" .
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
        if end_str.is_empty() {
            anyhow::bail!("missing end line in range '{spec}' (expected START:END)");
        }
        let start: usize = start_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid start line: {start_str}"))?;
        let end: usize = end_str
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid end line: {end_str}"))?;
        if start == 0 {
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
