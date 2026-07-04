//! Unit tests for tidy check/fix.

use super::check::{check_file, collect_issues};
use super::fix::eol_mode_to_str;
use super::*;
use crate::cli::global::GlobalFlags;
use crate::exit;
use tempfile::TempDir;

#[test]
fn detects_missing_final_newline() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("no_newline.txt");
    std::fs::write(&file, b"hello").unwrap();

    let issues = check_file(&file, true, None, true);
    assert!(
        issues.iter().any(|i| i.issue == "missing final newline"),
        "expected missing final newline issue, got: {issues:?}"
    );
}

#[test]
fn empty_file_no_missing_newline() {
    // Regression: empty files should not be flagged for "missing final newline"
    // since they have no content at all.
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("empty.txt");
    std::fs::write(&file, b"").unwrap();

    let issues = check_file(&file, true, None, true);
    assert!(
        !issues.iter().any(|i| i.issue == "missing final newline"),
        "empty file should not be flagged for missing final newline: {issues:?}"
    );
}

#[test]
fn detects_mixed_line_endings() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("mixed.txt");
    // First line uses CRLF, second uses bare LF.
    std::fs::write(&file, b"line1\r\nline2\nline3\n").unwrap();

    let issues = check_file(&file, true, None, true);
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

    let issues = check_file(&file, true, None, true);
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

    let issues = check_file(&file, true, None, true);
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

    let issues = check_file(&file, true, None, true);
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

    let issues = check_file(&file, true, None, true);
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

    let issues = check_file(&file, true, None, true);
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

/// #1176: collapse_blanks flag must be forwarded from global flags
/// into TidyFix operations so the tx engine can apply it.
#[test]
fn collapse_blanks_flag_forwarded_to_tidy_fix() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("blank.txt");
    // File with consecutive blank lines.
    std::fs::write(&file, "a\n\n\n\nb\n").unwrap();

    let mut global = GlobalFlags::test_with_cwd(tmp.path());
    global.apply = true;
    global.collapse_blanks = true;

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
    // Consecutive blank lines should be collapsed to at most one.
    assert_eq!(content, "a\n\nb\n");
}

/// Regression: `tidy check --normalize-eol lf` must detect files with
/// consistent CRLF line endings (not just mixed endings). Without this,
/// agents think CRLF files are clean when a LF target is specified.
#[test]
fn check_detects_crlf_when_eol_target_is_lf() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("crlf.txt");
    std::fs::write(&file, b"line1\r\nline2\r\n").unwrap();

    // Without --normalize-eol: no EOL issue (consistent CRLF is fine).
    let issues = check_file(&file, true, None, true);
    assert!(
        !issues.iter().any(|i| i.issue.contains("normalization")),
        "no normalization issue without eol target: {issues:?}"
    );

    // With --normalize-eol lf: should detect CRLF as needing normalization.
    let issues = check_file(&file, true, Some(crate::write::EolMode::Lf), true);
    assert!(
        issues
            .iter()
            .any(|i| i.issue.contains("normalization to LF")),
        "expected LF normalization issue, got: {issues:?}"
    );

    // End-to-end via run(): should return CHANGES_DETECTED.
    let mut global = GlobalFlags::test_with_cwd(tmp.path());
    global.normalize_eol = Some(crate::write::EolMode::Lf);
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
        "tidy check --normalize-eol lf must return CHANGES_DETECTED for CRLF file"
    );
}

/// `tidy check --normalize-eol crlf` must detect files with bare LF.
#[test]
fn check_detects_lf_when_eol_target_is_crlf() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("lf.txt");
    std::fs::write(&file, b"line1\nline2\n").unwrap();

    let issues = check_file(&file, true, Some(crate::write::EolMode::Crlf), true);
    assert!(
        issues
            .iter()
            .any(|i| i.issue.contains("normalization to CRLF")),
        "expected CRLF normalization issue, got: {issues:?}"
    );
}

/// Files that already match the target EOL should not be flagged.
#[test]
fn check_no_eol_issue_when_already_matching() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("lf.txt");
    std::fs::write(&file, b"line1\nline2\n").unwrap();

    let issues = check_file(&file, true, Some(crate::write::EolMode::Lf), true);
    assert!(
        !issues.iter().any(|i| i.issue.contains("normalization")),
        "LF file with LF target should not be flagged: {issues:?}"
    );
}

/// Regression: `tidy check --respect-editorconfig` must detect EOL
/// mismatches defined in `.editorconfig`.  Without this fix,
/// `collect_issues()` only passed `global.normalize_eol` (which is
/// `None` without explicit `--normalize-eol`), so editorconfig's
/// `end_of_line` setting was silently ignored in check mode.
#[test]
fn check_respect_editorconfig_detects_eol_mismatch() {
    let tmp = TempDir::new().unwrap();

    // .editorconfig: *.txt must use CRLF
    std::fs::write(
        tmp.path().join(".editorconfig"),
        "root = true\n\n[*.txt]\nend_of_line = crlf\n",
    )
    .unwrap();

    // File has LF endings (mismatches editorconfig)
    let file = tmp.path().join("test.txt");
    std::fs::write(&file, b"line1\nline2\n").unwrap();

    let mut global = GlobalFlags::test_with_cwd(tmp.path());
    global.respect_editorconfig = true;

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
        "tidy check --respect-editorconfig must detect LF when editorconfig says CRLF"
    );
}

/// Regression: `tidy check --respect-editorconfig` must suppress trailing
/// whitespace diagnostics for file types where `.editorconfig` declares
/// `trim_trailing_whitespace = false` (e.g. `[*.md]`).  Without this fix,
/// `check_file()` always flagged trailing whitespace regardless of the
/// editorconfig setting, causing false positives on Markdown files where
/// trailing spaces are intentional (e.g. hard line breaks).
#[test]
fn check_respect_editorconfig_suppresses_trailing_ws() {
    let tmp = TempDir::new().unwrap();

    // .editorconfig: *.md must NOT trim trailing whitespace
    std::fs::write(
        tmp.path().join(".editorconfig"),
        "root = true\n\n[*.md]\ntrim_trailing_whitespace = false\n",
    )
    .unwrap();

    // Markdown file with intentional trailing whitespace (hard line break)
    let file = tmp.path().join("readme.md");
    std::fs::write(&file, "Hello  \nWorld\n").unwrap();

    let mut global = GlobalFlags::test_with_cwd(tmp.path());
    global.respect_editorconfig = true;

    let issues = collect_issues(&[".".to_string()], &global).unwrap();
    assert!(
        !issues.iter().any(|i| i.issue == "trailing whitespace"),
        "trailing whitespace should not be reported when editorconfig says \
         trim_trailing_whitespace = false, got: {issues:?}"
    );

    // End-to-end: should return SUCCESS (no issues)
    let args = TidyArgs {
        action: TidyAction::Check {
            paths: vec![".".to_string()],
        },
        write: Default::default(),
    };
    let code = run(args, &global).unwrap();
    assert_eq!(
        code,
        exit::SUCCESS,
        "tidy check --respect-editorconfig must not flag trailing ws \
         when editorconfig says trim_trailing_whitespace = false"
    );
}

/// `tidy check --respect-editorconfig` still flags trailing whitespace
/// when editorconfig explicitly sets `trim_trailing_whitespace = true`.
#[test]
fn check_respect_editorconfig_flags_trailing_ws_when_true() {
    let tmp = TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join(".editorconfig"),
        "root = true\n\n[*.rs]\ntrim_trailing_whitespace = true\n",
    )
    .unwrap();

    let file = tmp.path().join("main.rs");
    std::fs::write(&file, "fn main()  \n").unwrap();

    let mut global = GlobalFlags::test_with_cwd(tmp.path());
    global.respect_editorconfig = true;

    let issues = collect_issues(&[".".to_string()], &global).unwrap();
    assert!(
        issues.iter().any(|i| i.issue == "trailing whitespace"),
        "trailing whitespace should still be flagged when editorconfig says \
         trim_trailing_whitespace = true, got: {issues:?}"
    );
}

/// `check_file` with `check_trailing_ws = false` suppresses trailing ws.
#[test]
fn check_file_skips_trailing_ws_when_disabled() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("test.txt");
    std::fs::write(&file, "hello   \nworld\n").unwrap();

    // With check_trailing_ws = true: should find trailing whitespace
    let issues = check_file(&file, true, None, true);
    assert!(
        issues.iter().any(|i| i.issue == "trailing whitespace"),
        "should find trailing ws with check_trailing_ws=true"
    );

    // With check_trailing_ws = false: should NOT find trailing whitespace
    let issues = check_file(&file, true, None, false);
    assert!(
        !issues.iter().any(|i| i.issue == "trailing whitespace"),
        "should not find trailing ws with check_trailing_ws=false"
    );
}
