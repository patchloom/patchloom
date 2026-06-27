use crate::cli::global::GlobalFlags;
use crate::diff::{render_diffs_colored, render_diffs_plain};
use crate::exit;
use crate::plan::Operation;
use crate::tx::engine::{ExecuteOptions, ExecutionResult, execute_operations};
use crate::write::{apply_policy, policy_from_flags};
use clap::Args;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom tidy src/ --ensure-final-newline
  patchloom tidy . --trim-trailing-whitespace --apply
  patchloom tidy . --normalize-eol lf --check")]
pub struct TidyArgs {
    #[command(subcommand)]
    pub action: TidyAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum TidyAction {
    /// Report newline, EOL, and whitespace issues in text files.
    Check { paths: Vec<String> },
    /// Apply normalization fixes to text files.
    Fix {
        paths: Vec<String>,
        /// Dedent: remove leading whitespace. Values: "auto", "tab", or a number (e.g. "4").
        #[arg(long)]
        dedent: Option<String>,
        /// Indent: add leading whitespace. Values: "tab" or a number (e.g. "4").
        #[arg(long)]
        indent: Option<String>,
        /// Restrict dedent/indent to a line range (1-based inclusive, e.g. "10:50").
        #[arg(long)]
        lines: Option<String>,
    },
}

/// A single tidy issue found in a file.
#[derive(Debug, Clone, Serialize)]
pub struct TidyIssue {
    pub path: String,
    pub issue: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

fn check_file(path: &Path, quiet: bool) -> Vec<TidyIssue> {
    let Some(text) = crate::files::read_text_file_logged(path, "tidy", quiet) else {
        return Vec::new();
    };
    let data = text.as_bytes();

    let path_str = path.to_string_lossy().into_owned();
    let mut issues = Vec::new();

    // Check missing final newline.
    if !data.ends_with(b"\n") {
        issues.push(TidyIssue {
            path: path_str.clone(),
            issue: "missing final newline",
            line: None,
        });
    }

    // Check mixed line endings: file has both \r\n and bare \n.
    let has_crlf = memchr::memmem::find(data, b"\r\n").is_some();
    // A bare \n is any \n not preceded by \r.
    let has_bare_lf = memchr::memchr_iter(b'\n', data).any(|i| i == 0 || data[i - 1] != b'\r');
    if has_crlf && has_bare_lf {
        issues.push(TidyIssue {
            path: path_str.clone(),
            issue: "mixed line endings",
            line: None,
        });
    }

    // Check trailing whitespace per line.
    // We split on \n so each "line" may still contain a trailing \r from CRLF.
    for (line_idx, raw_line) in data.split(|&b| b == b'\n').enumerate() {
        // Strip trailing \r if present (from CRLF).
        let content = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        // Skip completely empty lines and the phantom empty element after a
        // trailing newline.
        if content.is_empty() {
            continue;
        }
        if matches!(content.last(), Some(b' ' | b'\t')) {
            issues.push(TidyIssue {
                path: path_str.clone(),
                issue: "trailing whitespace",
                line: Some(line_idx + 1), // 1-based
            });
        }
    }

    issues
}

/// Collect all issues from the given paths, honouring .gitignore and optional
/// glob filtering.  Uses `collect_file_paths_opts` with `include_hidden=true`
/// so dotfiles are also checked.  File scanning is parallelized.
fn collect_issues(paths: &[String], global: &GlobalFlags) -> anyhow::Result<Vec<TidyIssue>> {
    let cwd = global.resolve_cwd()?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let file_paths = crate::collect_file_paths_opts(paths, global, true, Some(&cwd))?;
    let glob_roots = crate::collect_glob_roots_from_global(paths, global, Some(&cwd))?;

    let quiet = global.quiet;
    let file_issues: Vec<Vec<TidyIssue>> =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let issues = check_file(path, quiet);
            if issues.is_empty() {
                None
            } else {
                Some(issues)
            }
        });

    Ok(file_issues.into_iter().flatten().collect())
}

/// JSON wrapper for tidy check output.
#[derive(Debug, Serialize)]
struct TidyCheckOutput {
    ok: bool,
    issue_count: usize,
    issues: Vec<TidyIssue>,
}

/// Per-file result for tidy fix structured output.
#[derive(Debug, Clone, Serialize)]
struct TidyFixFileResult {
    path: String,
}

/// JSON wrapper for tidy fix output.
#[derive(Debug, Serialize)]
struct TidyFixOutput {
    ok: bool,
    files_changed: usize,
    files: Vec<TidyFixFileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
}

/// Render issues to stdout.
fn render_issues(issues: &[TidyIssue], global: &GlobalFlags) {
    if global.json {
        let output = TidyCheckOutput {
            ok: issues.is_empty(),
            issue_count: issues.len(),
            issues: issues.to_vec(),
        };
        let _ = global.emit_json(&output);
    } else if let Ok(true) = global.emit_json_items(issues) {
        // emitted per-item JSONL
    } else {
        for issue in issues {
            if let Some(line) = issue.line {
                println!("{}:{}: {}", issue.path, line, issue.issue);
            } else {
                println!("{}: {}", issue.path, issue.issue);
            }
        }
    }
}

pub fn run(args: TidyArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    match args.action {
        TidyAction::Check { paths } => {
            let issues = collect_issues(&paths, global)?;
            if !global.quiet || global.json || global.jsonl {
                render_issues(&issues, global);
            }
            if issues.is_empty() {
                Ok(exit::SUCCESS)
            } else {
                Ok(exit::CHANGES_DETECTED)
            }
        }
        TidyAction::Fix {
            paths,
            dedent,
            indent,
            lines,
        } => {
            if dedent.is_some() && indent.is_some() {
                anyhow::bail!("--dedent and --indent cannot both be set");
            }

            let cwd = global.resolve_cwd()?;
            let glob_matcher = crate::build_glob_matcher_from_global(global)?;
            let fix_file_paths = crate::collect_file_paths_opts(&paths, global, true, Some(&cwd))?;
            let glob_roots = crate::collect_glob_roots_from_global(&paths, global, Some(&cwd))?;

            // Parse line range once (shared across all files).
            let line_range = lines
                .as_deref()
                .map(crate::write::parse_line_range)
                .transpose()?;

            // Parallel read+compute phase: identify which files need fixing.
            // This preserves the performance-critical parallel scan; only
            // files that actually differ after policy application are
            // included in the engine execution.
            let quiet = global.quiet;
            let dedent_ref = dedent.as_deref();
            let indent_ref = indent.as_deref();
            let dirty_rel_paths: Vec<String> = crate::par_process_files(
                &fix_file_paths,
                glob_matcher.as_ref(),
                &glob_roots,
                |file_path| {
                    let original =
                        match crate::files::read_text_file_logged(file_path, "tidy", quiet) {
                            Some(text) => text,
                            None => return None,
                        };
                    let policy = policy_from_flags(global, Some(file_path));
                    let mut fixed = apply_policy(&original, &policy).into_owned();
                    if let Some(spec) = dedent_ref {
                        fixed = crate::write::dedent_content(&fixed, spec, line_range);
                    }
                    if let Some(spec) = indent_ref {
                        fixed = crate::write::indent_content(&fixed, spec, line_range);
                    }
                    if fixed == *original {
                        return None;
                    }
                    let rel_path = file_path
                        .strip_prefix(&cwd)
                        .unwrap_or(file_path)
                        .to_string_lossy()
                        .to_string();
                    Some(rel_path)
                },
            );

            if dirty_rel_paths.is_empty() {
                // No changes: emit structured output (matching old behavior)
                // and exit.
                emit_tidy_fix_output(global, &[], None)?;
                return Ok(exit::SUCCESS);
            }

            // Build one TidyFix operation per dirty file.
            let eol_str = global.normalize_eol.map(eol_mode_to_str);
            let ops: Vec<Operation> = dirty_rel_paths
                .iter()
                .map(|rel_path| Operation::TidyFix {
                    path: rel_path.clone(),
                    ensure_final_newline: Some(global.ensure_final_newline),
                    trim_trailing_whitespace: Some(global.trim_trailing_whitespace),
                    normalize_eol: eol_str.map(String::from),
                    dedent: dedent.clone(),
                    indent: indent.clone(),
                    lines: lines.clone(),
                })
                .collect();

            let options = ExecuteOptions {
                cwd: &cwd,
                global,
                guard: None,
            };
            let result = execute_operations(ops, options)?;

            tidy_fix_output(global, result, &dirty_rel_paths, &cwd)
        }
    }
}

/// Convert `EolMode` to the string format expected by `Operation::TidyFix`.
fn eol_mode_to_str(mode: crate::cli::global::EolMode) -> &'static str {
    match mode {
        crate::cli::global::EolMode::Lf => "lf",
        crate::cli::global::EolMode::Crlf => "crlf",
        crate::cli::global::EolMode::Cr => "cr",
        crate::cli::global::EolMode::Keep => "keep",
    }
}

/// Handle output rendering and commit/check/preview for tidy fix via the engine.
fn tidy_fix_output(
    global: &GlobalFlags,
    result: ExecutionResult,
    dirty_rel_paths: &[String],
    cwd: &Path,
) -> anyhow::Result<u8> {
    let fix_files: Vec<TidyFixFileResult> = dirty_rel_paths
        .iter()
        .map(|p| TidyFixFileResult { path: p.clone() })
        .collect();

    // --check mode: report what would happen, no mutation.
    if global.check {
        emit_tidy_fix_output(global, &fix_files, None)?;
        if !global.quiet && !global.json && !global.jsonl {
            for p in dirty_rel_paths {
                println!("{p}");
            }
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: commit, then report.
    if global.apply {
        let diffs = result.build_diffs();
        result.commit()?;
        crate::write::run_format_command(global, cwd)?;

        let diff_text = if global.diff {
            Some(render_diffs_plain(&diffs))
        } else {
            None
        };
        emit_tidy_fix_output(global, &fix_files, diff_text)?;
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: preview diffs.
    let diffs = result.build_diffs();
    let diff_text = if !diffs.is_empty() {
        Some(render_diffs_plain(&diffs))
    } else {
        None
    };

    emit_tidy_fix_output(global, &fix_files, diff_text)?;

    if !global.json && !global.jsonl && !global.quiet && !diffs.is_empty() {
        print!("{}", render_diffs_colored(&diffs, global.should_color()));
    }

    if global.show_status() {
        eprintln!("{} file(s) changed", dirty_rel_paths.len());
    }

    // --confirm: prompt after showing diffs, then apply if confirmed.
    if global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, cwd)?;
        return Ok(exit::SUCCESS);
    }

    Ok(exit::CHANGES_DETECTED)
}

/// Emit structured JSON/JSONL output for tidy fix.
fn emit_tidy_fix_output(
    global: &GlobalFlags,
    fix_files: &[TidyFixFileResult],
    diff_text: Option<String>,
) -> anyhow::Result<()> {
    if global.json {
        let output = TidyFixOutput {
            ok: true,
            files_changed: fix_files.len(),
            files: fix_files.to_vec(),
            diff: diff_text,
        };
        let _ = global.emit_json(&output);
    } else if global.jsonl {
        let _ = global.emit_json_items(fix_files);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    #[test]
    fn detects_missing_final_newline() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_newline.txt");
        std::fs::write(&file, b"hello").unwrap();

        let issues = check_file(&file, true);
        assert!(
            issues.iter().any(|i| i.issue == "missing final newline"),
            "expected missing final newline issue, got: {issues:?}"
        );
    }

    #[test]
    fn detects_mixed_line_endings() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("mixed.txt");
        // First line uses CRLF, second uses bare LF.
        std::fs::write(&file, b"line1\r\nline2\nline3\n").unwrap();

        let issues = check_file(&file, true);
        assert!(
            issues.iter().any(|i| i.issue == "mixed line endings"),
            "expected mixed line endings issue, got: {issues:?}"
        );
    }

    #[test]
    fn detects_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("trailing.txt");
        std::fs::write(&file, b"hello   \nworld\n").unwrap();

        let issues = check_file(&file, true);
        let trailing: Vec<_> = issues
            .iter()
            .filter(|i| i.issue == "trailing whitespace")
            .collect();
        assert_eq!(trailing.len(), 1, "expected 1 trailing whitespace issue");
        assert_eq!(trailing[0].line, Some(1));
    }

    #[test]
    fn clean_file_produces_no_issues_and_exit_zero() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        std::fs::write(&file, b"hello\nworld\n").unwrap();

        let issues = check_file(&file, true);
        assert!(issues.is_empty(), "expected no issues for clean file");

        let global = GlobalFlags::test_with_cwd(tmp.path());
        let args = TidyArgs {
            action: TidyAction::Check {
                paths: vec![".".to_string()],
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn multiple_issues_in_one_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("multi.txt");
        // Missing final newline + trailing whitespace on line 1 + mixed endings.
        std::fs::write(&file, b"hello \r\nworld\nfoo").unwrap();

        let issues = check_file(&file, true);
        let issue_types: Vec<&str> = issues.iter().map(|i| i.issue).collect();
        assert!(
            issue_types.contains(&"missing final newline"),
            "missing final newline not found in {issue_types:?}"
        );
        assert!(
            issue_types.contains(&"mixed line endings"),
            "mixed line endings not found in {issue_types:?}"
        );
        assert!(
            issue_types.contains(&"trailing whitespace"),
            "trailing whitespace not found in {issue_types:?}"
        );
    }

    #[test]
    fn binary_files_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("binary.bin");
        // Binary content with NUL bytes, missing final newline, trailing ws.
        let mut data = b"hello \nworld".to_vec();
        data.insert(3, 0x00);
        std::fs::write(&file, &data).unwrap();

        let issues = check_file(&file, true);
        assert!(
            issues.is_empty(),
            "expected no issues for binary file, got: {issues:?}"
        );
    }

    #[test]
    fn large_binary_files_are_skipped_via_header_probe() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("large.bin");
        let mut data = vec![b'a'; 10_000];
        data[4096] = 0;
        std::fs::write(&file, &data).unwrap();

        let issues = check_file(&file, true);
        assert!(
            issues.is_empty(),
            "expected no issues for large binary file, got: {issues:?}"
        );
    }

    #[test]
    fn glob_filtering_works() {
        let tmp = TempDir::new().unwrap();
        // Create two files: one .rs and one .txt. The .txt has an issue.
        let rs_file = tmp.path().join("clean.rs");
        std::fs::write(&rs_file, b"fn main() {}\n").unwrap();

        let txt_file = tmp.path().join("dirty.txt");
        std::fs::write(&txt_file, b"no newline").unwrap();

        // Filter to only *.rs — should find no issues.
        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.glob = vec!["*.rs".to_string()];

        let issues = collect_issues(&[".".to_string()], &global).unwrap();
        assert!(
            issues.is_empty(),
            "expected no issues when filtering to *.rs, got: {issues:?}"
        );

        // Filter to only *.txt — should find the missing newline.
        global.glob = vec!["*.txt".to_string()];
        let issues = collect_issues(&[".".to_string()], &global).unwrap();
        assert!(
            !issues.is_empty(),
            "expected issues when filtering to *.txt"
        );
        assert!(issues.iter().any(|i| i.issue == "missing final newline"));
    }

    #[test]
    fn check_returns_changes_detected_for_dirty_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_newline.txt");
        std::fs::write(&file, b"hello").unwrap();

        let global = GlobalFlags::test_with_cwd(tmp.path());
        let args = TidyArgs {
            action: TidyAction::Check {
                paths: vec![".".to_string()],
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn fix_adds_missing_newline() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_nl.txt");
        std::fs::write(&file, b"hello").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.ensure_final_newline = true;
        global.apply = true;

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read(&file).unwrap();
        assert!(
            content.ends_with(b"\n"),
            "expected file to end with newline after fix"
        );
    }

    #[test]
    fn fix_normalizes_eol() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("crlf.txt");
        std::fs::write(&file, b"line1\r\nline2\r\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.normalize_eol = Some(crate::cli::global::EolMode::Lf);
        global.apply = true;

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read(&file).unwrap();
        assert!(
            !content.windows(2).any(|w| w == b"\r\n"),
            "expected no CRLF sequences after fix"
        );
    }

    #[test]
    fn fix_trims_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("trailing.txt");
        std::fs::write(&file, b"hello   \nworld\t\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.trim_trailing_whitespace = true;
        global.apply = true;

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn fix_is_idempotent_on_clean_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        std::fs::write(&file, b"hello\nworld\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.ensure_final_newline = true;
        global.trim_trailing_whitespace = true;
        global.normalize_eol = Some(crate::cli::global::EolMode::Lf);
        global.apply = true;

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn fix_dry_run_returns_changes_detected_when_dirty() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_nl.txt");
        std::fs::write(&file, b"hello").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.ensure_final_newline = true;
        // No --apply: default dry-run mode.

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File must not be modified in dry-run mode.
        let content = std::fs::read(&file).unwrap();
        assert_eq!(content, b"hello");
    }

    #[test]
    fn fix_dry_run_returns_success_when_clean() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        std::fs::write(&file, b"hello\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.ensure_final_newline = true;
        // No --apply: default dry-run mode.

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn fix_apply_creates_backup_session() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_nl.txt");
        std::fs::write(&file, b"hello").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.ensure_final_newline = true;
        global.apply = true;

        let args = TidyArgs {
            action: TidyAction::Fix {
                paths: vec![".".to_string()],
                dedent: None,
                indent: None,
                lines: None,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let backup_dir = tmp.path().join(".patchloom").join("backups");
        assert!(
            backup_dir.exists(),
            "backup directory should exist after tidy fix --apply"
        );
        let sessions: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !sessions.is_empty(),
            "at least one backup session should be created"
        );
    }

    #[test]
    fn eol_mode_conversion_round_trips() {
        use crate::cli::global::EolMode;
        assert_eq!(eol_mode_to_str(EolMode::Lf), "lf");
        assert_eq!(eol_mode_to_str(EolMode::Crlf), "crlf");
        assert_eq!(eol_mode_to_str(EolMode::Cr), "cr");
        assert_eq!(eol_mode_to_str(EolMode::Keep), "keep");
    }

    /// Regression: `tidy check --quiet --json` must still emit JSON output.
    /// The old code gated render_issues entirely on `!global.quiet`, which
    /// suppressed JSON/JSONL output. Only human-readable text should be
    /// suppressed by --quiet.
    #[test]
    fn check_quiet_still_emits_json() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_nl.txt");
        std::fs::write(&file, b"hello").unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.quiet = true;
        global.json = true;

        // Collect issues directly (render_issues is called by run()).
        let issues = collect_issues(&[".".to_string()], &global).unwrap();
        assert!(!issues.is_empty(), "should detect issues even when quiet");

        // Verify that run() returns CHANGES_DETECTED (not SUCCESS) with --quiet.
        let args = TidyArgs {
            action: TidyAction::Check {
                paths: vec![".".to_string()],
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::CHANGES_DETECTED,
            "exit code must reflect issues even with --quiet"
        );
    }
}
