use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;
use std::fs;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom read src/main.rs
  patchloom read src/main.rs --lines 1-10
  patchloom read src/main.rs src/lib.rs --json")]
pub struct ReadArgs {
    /// Paths to the files to read.
    #[arg(required = true, num_args = 1..)]
    pub files: Vec<String>,
    /// Line range to display (1-based inclusive, e.g. "50:120" or "50-120").
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

pub(crate) use crate::ops::read::{LineRange, parse_line_range, select_lines};

/// Per-path read failure with a stable agent `error_kind`.
#[derive(Debug, Clone)]
struct ReadFail {
    kind: &'static str,
    msg: String,
}

impl std::fmt::Display for ReadFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

fn read_one_file(path: &str, lines: Option<LineRange>) -> Result<ReadOutput, ReadFail> {
    let p = std::path::Path::new(path);
    if p.exists() && !p.is_file() {
        return Err(ReadFail {
            kind: "invalid_input",
            msg: format!("{path}: target is not a file"),
        });
    }
    // Refuse binary targets (NUL in probe window) before UTF-8 decode so agents
    // do not get ok:true with embedded \0 (parity with search/replace/tidy;
    // MPI 2026-07-20).
    if let Err(err) = crate::ops::file::ensure_not_binary_file(p, path) {
        return Err(ReadFail {
            kind: "invalid_input",
            msg: err.msg,
        });
    }
    let content = fs::read_to_string(path).map_err(|e| {
        let kind = if e.kind() == std::io::ErrorKind::NotFound {
            "not_found"
        } else if e.kind() == std::io::ErrorKind::IsADirectory
            || e.kind() == std::io::ErrorKind::PermissionDenied
            || e.kind() == std::io::ErrorKind::InvalidData
        {
            // InvalidData covers non-UTF-8 text that passed the binary probe
            // (no early NUL). Surface as invalid_input, not not_found.
            "invalid_input"
        } else {
            "not_found"
        };
        ReadFail {
            kind,
            msg: format!("{path}: {e}"),
        }
    })?;

    // Fast path: no line range requested, skip split/join (#169).
    if lines.is_none() {
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

    let selected = select_lines(&content, lines.expect("checked is_none above"));
    // Empty selection for an explicit --lines range is not success: agents treat
    // ok:true + empty content as "file is empty", not "range past EOF"
    // (fixrealloop 2026-07-15).
    if selected.start_line == 0 {
        let n = selected.total_lines;
        return Err(ReadFail {
            kind: "no_matches",
            msg: format!(
                "{path}: line range outside file ({n} line{})",
                if n == 1 { "" } else { "s" }
            ),
        });
    }

    Ok(ReadOutput {
        ok: true,
        path: path.to_string(),
        start_line: selected.start_line,
        end_line: selected.end_line,
        total_lines: selected.total_lines,
        content: selected.content,
    })
}

/// Pick a single envelope kind when every path failed (or for partial skipped).
fn classify_read_failures(errors: &[ReadFail]) -> (&'static str, u8) {
    if errors.is_empty() {
        return ("not_found", exit::FAILURE);
    }
    if errors.iter().all(|e| e.kind == "no_matches") {
        return ("no_matches", exit::NO_MATCHES);
    }
    if errors.iter().all(|e| e.kind == "invalid_input") {
        return ("invalid_input", exit::FAILURE);
    }
    if errors.iter().all(|e| e.kind == "not_found") {
        return ("not_found", exit::FAILURE);
    }
    // Mixed failures (e.g. missing + directory): prefer invalid_input so agents
    // do not treat a directory path as create-missing.
    if errors.iter().any(|e| e.kind == "invalid_input") {
        return ("invalid_input", exit::FAILURE);
    }
    ("not_found", exit::FAILURE)
}

pub fn run(args: ReadArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("read: files={:?}, lines={:?}", args.files, args.lines);
    let cwd = global.resolve_cwd()?;
    // --contain applies to read paths (agent sandbox; MPI cycle 18).
    global.check_paths_contained(&cwd, &args.files)?;
    let structured = global.json || global.jsonl;

    let parsed_lines = if let Some(spec) = &args.lines {
        match parse_line_range(spec) {
            Ok(range) => Some(range),
            Err(err) => {
                let msg = format!("invalid --lines value '{spec}': {err}");
                global.emit_error_json_kind(Some("invalid_input"), &msg)?;
                return Ok(exit::FAILURE);
            }
        }
    } else {
        None
    };

    let multi = args.files.len() > 1;
    let mut outputs: Vec<ReadOutput> = Vec::new();
    let mut errors: Vec<ReadFail> = Vec::new();

    for (i, path) in args.files.iter().enumerate() {
        let resolved = cwd.join(path);
        let resolved_str = resolved.to_string_lossy();
        match read_one_file(&resolved_str, parsed_lines) {
            Ok(output) => {
                if global.jsonl {
                    global.emit_json(&output)?;
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
                // Always show errors on stderr, even with --quiet (#1179).
                if !structured {
                    eprintln!("read: {e}");
                }
                errors.push(e);
            }
        }
    }

    if errors.len() == args.files.len() {
        let msg = errors
            .iter()
            .map(|e| e.msg.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let (kind, code) = classify_read_failures(&errors);
        global.emit_error_json_kind(Some(kind), &msg)?;
        return Ok(code);
    }

    if global.json {
        if !multi && outputs.len() == 1 && errors.is_empty() {
            global.emit_json(&outputs[0])?;
        } else if multi && !errors.is_empty() {
            // Partial multi-path: bare success array hides failed paths from agents
            // that only parse stdout. Envelope with skipped[] + error_kind.
            #[derive(Serialize)]
            struct PartialReadReport {
                ok: bool,
                files: Vec<ReadOutput>,
                skipped: Vec<String>,
                error_kind: &'static str,
                error: String,
            }
            let (kind, _) = classify_read_failures(&errors);
            let report = PartialReadReport {
                ok: false,
                files: outputs,
                skipped: errors.iter().map(|e| e.msg.clone()).collect(),
                error_kind: kind,
                error: format!("partial read failure: {} path(s) failed", errors.len()),
            };
            global.emit_json(&report)?;
        } else {
            global.emit_json(&outputs)?;
        }
    }

    // Always show errors on stderr, even with --quiet (#1179).
    if structured {
        for error in &errors {
            eprintln!("read: {error}");
        }
    }

    // Partial failure: some files succeeded, some failed (#1166).
    if !errors.is_empty() {
        return Ok(exit::FAILURE);
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
    fn parse_range_start_end_dash() {
        assert_eq!(parse_line_range("5-10").unwrap(), (5, Some(10)));
    }

    #[test]
    fn parse_range_single_line() {
        assert_eq!(parse_line_range("7").unwrap(), (7, Some(7)));
    }

    #[test]
    fn parse_range_zero_fails() {
        parse_line_range("0:5").expect_err("expected error");
    }

    #[test]
    fn parse_range_end_before_start_fails() {
        parse_line_range("10:5").expect_err("expected error");
    }

    #[test]
    fn parse_range_missing_start_fails() {
        let err = parse_line_range(":10").unwrap_err();
        assert!(
            err.to_string().contains("missing start line"),
            "expected 'missing start line', got: {err}"
        );
    }

    #[test]
    fn parse_range_open_ended() {
        assert_eq!(parse_line_range("5:").unwrap(), (5, None));
    }

    #[test]
    fn parse_range_non_numeric_single_fails() {
        let err = parse_line_range("abc").unwrap_err();
        assert!(
            err.to_string().contains("invalid line number"),
            "expected 'invalid line number', got: {err}"
        );
    }

    #[test]
    fn parse_range_single_zero_fails() {
        let err = parse_line_range("0").unwrap_err();
        assert!(
            err.to_string().contains("1-based"),
            "expected '1-based', got: {err}"
        );
    }

    #[test]
    fn parse_range_non_numeric_start_fails() {
        let err = parse_line_range("abc:10").unwrap_err();
        assert!(
            err.to_string().contains("invalid start line"),
            "expected 'invalid start line', got: {err}"
        );
    }

    #[test]
    fn parse_range_non_numeric_end_fails() {
        let err = parse_line_range("5:xyz").unwrap_err();
        assert!(
            err.to_string().contains("invalid end line"),
            "expected 'invalid end line', got: {err}"
        );
    }

    #[test]
    fn read_one_file_empty_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, "").unwrap();
        let result = read_one_file(file.to_str().unwrap(), None).unwrap();
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
        let lines = Some((2, Some(100)));
        let result = read_one_file(file.to_str().unwrap(), lines).unwrap();
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
        let result = read_one_file(file.to_str().unwrap(), None).unwrap();
        assert_eq!(result.content, "hello\n");
        assert_eq!(result.total_lines, 1);
    }

    #[test]
    fn read_one_file_no_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("notrail.txt");
        fs::write(&file, "hello").unwrap();
        let result = read_one_file(file.to_str().unwrap(), None).unwrap();
        assert_eq!(result.content, "hello");
        assert_eq!(result.total_lines, 1);
    }

    #[test]
    fn read_one_file_with_lines_preserves_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("trail.txt");
        fs::write(&file, "a\nb\nc\n").unwrap();
        // Request to end of file: trailing newline should be preserved.
        let lines = Some((2, Some(3)));
        let result = read_one_file(file.to_str().unwrap(), lines).unwrap();
        assert_eq!(result.content, "b\nc\n");
    }

    #[test]
    fn read_one_file_mid_range_no_trailing_newline() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("mid.txt");
        fs::write(&file, "a\nb\nc\nd\n").unwrap();
        // Request lines 2:3 (not end of file): no trailing newline added.
        let lines = Some((2, Some(3)));
        let result = read_one_file(file.to_str().unwrap(), lines).unwrap();
        assert_eq!(result.content, "b\nc");
    }

    #[test]
    fn read_one_file_empty_file_with_line_range() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, "").unwrap();
        let lines = Some((1, Some(5)));
        let result = read_one_file(file.to_str().unwrap(), lines);
        assert!(
            result.is_err(),
            "empty file with --lines must not look like success: {result:?}"
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, "no_matches");
        assert!(
            err.msg.contains("line range outside file"),
            "expected outside-file error, got: {err}"
        );
    }

    #[test]
    fn read_one_file_line_range_beyond_eof_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("short.txt");
        fs::write(&file, "line1\nline2\nline3\n").unwrap();
        let result = read_one_file(file.to_str().unwrap(), Some((5, Some(10))));
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        let err = result.unwrap_err();
        assert_eq!(err.kind, "no_matches");
        assert!(
            err.msg.contains("line range outside file") && err.msg.contains("3 lines"),
            "got: {err}"
        );
    }

    #[test]
    fn read_one_file_nonexistent_returns_error() {
        let result = read_one_file("/tmp/does-not-exist-patchloom-test.txt", None);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        let err = result.unwrap_err();
        assert_eq!(err.kind, "not_found");
    }

    #[test]
    fn read_one_file_directory_is_invalid_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = read_one_file(dir.path().to_str().unwrap(), None);
        let err = result.expect_err("directory must fail");
        assert_eq!(err.kind, "invalid_input");
        assert!(err.msg.contains("not a file"), "got: {err}");
    }

    #[test]
    fn select_lines_start_beyond_file_returns_empty() {
        use crate::ops::read::SelectedLines;
        let content = "line1\nline2\n";
        let result = select_lines(content, (100, Some(200)));
        assert_eq!(result, SelectedLines::empty(2));
    }

    #[test]
    fn select_lines_crlf_content_normalizes_to_lf() {
        // .lines() strips both \n and \r\n, then join("\n") always uses LF.
        // This documents the intentional behavior.
        let content = "alpha\r\nbeta\r\ngamma\r\n";
        let result = select_lines(content, (2, Some(3)));
        assert_eq!(result.content, "beta\ngamma\n");
        assert_eq!(result.start_line, 2);
        assert_eq!(result.end_line, 3);
    }

    /// Partial failure (some files exist, some don't) must return FAILURE (#1166).
    #[test]
    fn partial_read_failure_returns_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let exists = dir.path().join("exists.txt");
        std::fs::write(&exists, "hello\n").unwrap();
        let missing = dir.path().join("missing.txt");

        let args = ReadArgs {
            files: vec![
                exists.to_string_lossy().into_owned(),
                missing.to_string_lossy().into_owned(),
            ],
            lines: None,
        };
        let global = GlobalFlags::test_default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    /// #1179: Errors must be reported even with --quiet, so agents can
    /// diagnose failures. The run() function returns FAILURE and the error
    /// goes to stderr unconditionally.
    #[test]
    fn quiet_mode_still_returns_failure_for_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist.txt");

        let args = ReadArgs {
            files: vec![missing.to_string_lossy().into_owned()],
            lines: None,
        };
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    /// #1179: Invalid --lines with --quiet must still return FAILURE.
    #[test]
    fn quiet_mode_still_returns_failure_for_bad_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "hello\n").unwrap();

        let args = ReadArgs {
            files: vec![file.to_string_lossy().into_owned()],
            lines: Some("abc".to_string()),
        };
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}
