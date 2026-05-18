use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;
use std::fs;

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Paths to the files to read.
    #[arg(required = true, num_args = 1..)]
    pub files: Vec<String>,
    /// Line range to display (1-based inclusive, e.g. "50:120").
    #[arg(long)]
    pub lines: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReadOutput {
    ok: bool,
    path: String,
    start_line: usize,
    end_line: usize,
    total_lines: usize,
    content: String,
}

pub(crate) fn parse_line_range(spec: &str) -> anyhow::Result<(usize, Option<usize>)> {
    if let Some((start_str, end_str)) = spec.split_once(':') {
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

fn read_one_file(path: &str, lines_spec: &Option<String>) -> Result<ReadOutput, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;

    // Fast path: no line range requested, skip split/join (#169).
    if lines_spec.is_none() {
        let total_lines = content.lines().count();
        let start_line = if total_lines == 0 { 0 } else { 1 };
        return Ok(ReadOutput {
            ok: true,
            path: path.to_string(),
            start_line,
            end_line: total_lines,
            total_lines,
            content,
        });
    }

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    let (start, end) = if let Some(ref spec) = lines_spec {
        let (s, e) = parse_line_range(spec).map_err(|e| e.to_string())?;
        let end = e.unwrap_or(total_lines).min(total_lines);
        (s, end)
    } else {
        (1, total_lines)
    };

    let selected: Vec<&str> = if total_lines == 0 {
        Vec::new()
    } else {
        let start_idx = (start - 1).min(total_lines);
        let end_idx = end.min(total_lines);
        all_lines[start_idx..end_idx].to_vec()
    };

    let selected_content = if selected.is_empty() {
        String::new()
    } else {
        let mut s = selected.join("\n");
        if content.ends_with('\n') && end >= total_lines {
            s.push('\n');
        }
        s
    };

    Ok(ReadOutput {
        ok: true,
        path: path.to_string(),
        start_line: start,
        end_line: end,
        total_lines,
        content: selected_content,
    })
}

pub fn run(args: ReadArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;

    let multi = args.files.len() > 1;
    let mut outputs: Vec<ReadOutput> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (i, path) in args.files.iter().enumerate() {
        let resolved = cwd.join(path);
        let resolved_str = resolved.to_string_lossy();
        match read_one_file(&resolved_str, &args.lines) {
            Ok(output) => {
                if global.jsonl {
                    println!("{}", serde_json::to_string(&output)?);
                } else if global.json {
                    outputs.push(output);
                } else if !global.quiet {
                    if multi && i > 0 {
                        println!();
                    }
                    if multi {
                        println!("==> {} <==", path);
                    }
                    print!("{}", output.content);
                }
            }
            Err(e) => {
                if !global.quiet {
                    eprintln!("error: {e}");
                }
                errors.push(e);
            }
        }
    }

    if global.json {
        if outputs.len() == 1 {
            println!("{}", serde_json::to_string_pretty(&outputs[0])?);
        } else {
            println!("{}", serde_json::to_string_pretty(&outputs)?);
        }
    }

    if errors.len() == args.files.len() {
        Ok(exit::FAILURE)
    } else {
        Ok(exit::SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_start_end() {
        assert_eq!(parse_line_range("5:10").unwrap(), (5, Some(10)));
    }

    #[test]
    fn parse_range_single_line() {
        assert_eq!(parse_line_range("7").unwrap(), (7, Some(7)));
    }

    #[test]
    fn parse_range_zero_fails() {
        assert!(parse_line_range("0:5").is_err());
    }

    #[test]
    fn parse_range_end_before_start_fails() {
        assert!(parse_line_range("10:5").is_err());
    }

    #[test]
    fn read_one_file_empty_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, "").unwrap();
        let result = read_one_file(file.to_str().unwrap(), &None).unwrap();
        assert_eq!(result.content, "");
        assert_eq!(result.total_lines, 0);
        assert_eq!(result.start_line, 0);
        assert_eq!(result.end_line, 0);
    }

    #[test]
    fn read_one_file_line_range_clamped_to_file_length() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("short.txt");
        fs::write(&file, "line1\nline2\nline3\n").unwrap();
        let spec = Some("2:100".to_string());
        let result = read_one_file(file.to_str().unwrap(), &spec).unwrap();
        assert_eq!(result.content, "line2\nline3\n");
        assert_eq!(result.start_line, 2);
        assert_eq!(result.end_line, 3);
        assert_eq!(result.total_lines, 3);
    }

    #[test]
    fn read_one_file_no_lines_spec_preserves_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("trail.txt");
        fs::write(&file, "hello\n").unwrap();
        let result = read_one_file(file.to_str().unwrap(), &None).unwrap();
        assert_eq!(result.content, "hello\n");
        assert_eq!(result.total_lines, 1);
    }

    #[test]
    fn read_one_file_no_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("notrail.txt");
        fs::write(&file, "hello").unwrap();
        let result = read_one_file(file.to_str().unwrap(), &None).unwrap();
        assert_eq!(result.content, "hello");
        assert_eq!(result.total_lines, 1);
    }

    #[test]
    fn read_one_file_with_lines_preserves_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("trail.txt");
        fs::write(&file, "a\nb\nc\n").unwrap();
        // Request to end of file: trailing newline should be preserved.
        let spec = Some("2:3".to_string());
        let result = read_one_file(file.to_str().unwrap(), &spec).unwrap();
        assert_eq!(result.content, "b\nc\n");
    }

    #[test]
    fn read_one_file_mid_range_no_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("mid.txt");
        fs::write(&file, "a\nb\nc\nd\n").unwrap();
        // Request lines 2:3 (not end of file): no trailing newline added.
        let spec = Some("2:3".to_string());
        let result = read_one_file(file.to_str().unwrap(), &spec).unwrap();
        assert_eq!(result.content, "b\nc");
    }

    #[test]
    fn read_one_file_empty_file_with_line_range() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, "").unwrap();
        let spec = Some("1:5".to_string());
        let result = read_one_file(file.to_str().unwrap(), &spec).unwrap();
        assert_eq!(result.content, "");
        assert_eq!(result.total_lines, 0);
    }

    #[test]
    fn read_one_file_nonexistent_returns_error() {
        let result = read_one_file("/tmp/does-not-exist-patchloom-test.txt", &None);
        assert!(result.is_err());
    }
}
