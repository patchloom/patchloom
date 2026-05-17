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
        return Ok(ReadOutput {
            ok: true,
            path: path.to_string(),
            start_line: 1,
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
    std::env::set_current_dir(global.resolve_cwd()?)?;

    let multi = args.files.len() > 1;
    let mut outputs: Vec<ReadOutput> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (i, path) in args.files.iter().enumerate() {
        match read_one_file(path, &args.lines) {
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
}
