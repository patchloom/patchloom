use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;
use std::fs;

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Path to the file to read.
    pub file: String,
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

fn parse_line_range(spec: &str) -> anyhow::Result<(usize, Option<usize>)> {
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

pub fn run(args: ReadArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    std::env::set_current_dir(global.resolve_cwd()?)?;

    let content = match fs::read_to_string(&args.file) {
        Ok(c) => c,
        Err(e) => {
            if !global.quiet {
                eprintln!("error: {}: {e}", args.file);
            }
            return Ok(exit::FAILURE);
        }
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    let (start, end) = if let Some(ref spec) = args.lines {
        let (s, e) = parse_line_range(spec)?;
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
        // Preserve trailing newline if the original content had one and we
        // include the last line.
        if content.ends_with('\n') && end >= total_lines {
            s.push('\n');
        }
        s
    };

    if global.json {
        let output = ReadOutput {
            ok: true,
            path: args.file.clone(),
            start_line: start,
            end_line: end,
            total_lines,
            content: selected_content,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !global.quiet {
        print!("{selected_content}");
    }

    Ok(exit::SUCCESS)
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
}
