use crate::cli::global::GlobalFlags;
use crate::diff::{self, DiffResult, unified_diff};
use crate::exit;
use crate::ops::replace::{
    compile_replace_regex, replace_content, replace_whole_lines, replacement_text,
};
use crate::write::policy_from_flags;
use clap::Args;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom replace 'old_name' --to 'new_name' src/
  patchloom replace 'http://' --to 'https://' src/ --apply
  patchloom replace 'v1\\.0' --to 'v2.0' --regex README.md")]
pub struct ReplaceArgs {
    /// Pattern to find.
    pub from: String,
    /// Text to replace with.
    #[arg(long)]
    pub to: Option<String>,
    // ref:replace-mode:insert-before
    /// Insert text before each match instead of replacing.
    #[arg(long, conflicts_with = "insert_after")]
    pub insert_before: Option<String>,
    // ref:replace-mode:insert-after
    /// Insert text after each match instead of replacing.
    #[arg(long, conflicts_with = "insert_before")]
    pub insert_after: Option<String>,
    /// Paths to operate on.
    pub paths: Vec<String>,
    /// Treat pattern as a literal string (default).
    #[arg(long, short = 'F')]
    pub literal: bool,
    // ref:replace-mode:regex
    /// Treat pattern as a regex.
    #[arg(long)]
    pub regex: bool,
    // ref:replace-mode:if-exists
    /// Return success even if no matches found (idempotent mode).
    #[arg(long)]
    pub if_exists: bool,
    // ref:replace-mode:multiline
    /// Enable multiline matching (dot matches newlines in regex mode).
    #[arg(long, short = 'U')]
    pub multiline: bool,
    // ref:replace-mode:nth
    /// Replace only the Nth occurrence (1-based).
    #[arg(long)]
    pub nth: Option<usize>,
    // ref:replace-mode:case-insensitive
    /// Case-insensitive matching.
    #[arg(long, short = 'i')]
    pub case_insensitive: bool,
    // ref:replace-mode:whole-line
    /// Replace the entire line containing each match, not just the matched span.
    /// Useful for deleting lines (--whole-line --to '') or replacing full lines.
    #[arg(long, short = 'L')]
    pub whole_line: bool,
    // ref:replace-mode:word-boundary
    /// Match only at word boundaries. Prevents 'SetupFile' from matching
    /// inside 'BenchSetupFile'. The pattern is auto-escaped for regex
    /// metacharacters before wrapping with \\b anchors.
    #[arg(long, short = 'w')]
    pub word_boundary: bool,
    // ref:replace-mode:range
    /// Restrict matching to a line range (e.g. '10:50'). 1-based, inclusive.
    #[arg(long, short = 'R')]
    pub range: Option<String>,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Clone, Serialize)]
struct ReplaceFileResult {
    path: String,
    match_count: usize,
}

#[derive(Debug, Serialize)]
struct ReplaceOutput {
    ok: bool,
    match_count: usize,
    file_count: usize,
    files: Vec<ReplaceFileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
}

/// Result of processing a single file.
struct FileReplacement {
    path: String,
    /// Relative path for display in diffs and JSON output.
    display_path: String,
    original: String,
    replaced: String,
    match_count: usize,
}

/// Build policy+write tuples and write atomically with backup.
fn apply_replacements(
    replacements: &[FileReplacement],
    global: &GlobalFlags,
    cwd: &Path,
) -> anyhow::Result<()> {
    let policies: Vec<_> = replacements
        .iter()
        .map(|r| policy_from_flags(global, Some(Path::new(&r.path))))
        .collect();
    let writes: Vec<_> = replacements
        .iter()
        .zip(&policies)
        .map(|(r, p)| (Path::new(r.path.as_str()), r.replaced.as_str(), p))
        .collect();
    crate::backup::backup_write_files(cwd, &writes)
}

/// Parse `--range` argument into (start, optional_end). Reuses the line-range
/// parser from the read command.
fn parse_range_arg(spec: Option<&str>) -> anyhow::Result<Option<(usize, Option<usize>)>> {
    match spec {
        None => Ok(None),
        Some(s) => {
            let (start, end) = crate::cmd::read::parse_line_range(s)?;
            Ok(Some((start, end)))
        }
    }
}

fn build_replacement(args: &ReplaceArgs) -> String {
    replacement_text(
        &args.from,
        &args.to,
        &args.insert_before,
        &args.insert_after,
        args.regex || args.case_insensitive || args.word_boundary,
    )
}

/// Walk files and collect all replacements using parallel file processing.
fn collect_replacements(
    args: &ReplaceArgs,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<FileReplacement>> {
    let cwd = global.resolve_cwd()?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let file_paths = crate::collect_file_paths_opts(&args.paths, global, false, Some(&cwd))?;
    let glob_roots = crate::collect_glob_roots_from_global(&args.paths, global, Some(&cwd))?;
    let replacement = build_replacement(args);
    let quiet = global.quiet;

    let compiled_re = compile_replace_regex(
        &args.from,
        args.regex,
        args.case_insensitive,
        args.multiline,
        args.word_boundary,
    )?;

    let from = &args.from;
    let nth = args.nth;
    let whole_line = args.whole_line;
    let range = parse_range_arg(args.range.as_deref())?;

    let cwd_ref = &cwd;
    let mut replacements: Vec<FileReplacement> =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let content = crate::files::read_text_file_logged(path, "replace", quiet)?;
            let (replaced, count) = if whole_line {
                replace_whole_lines(
                    &content,
                    from,
                    &replacement,
                    compiled_re.as_ref(),
                    nth,
                    range,
                )
            } else {
                replace_content(&content, from, &replacement, compiled_re.as_ref(), nth)
            };
            if count > 0 {
                let replaced = replaced.into_owned();
                let display_path = crate::files::relative_display(path, cwd_ref)
                    .to_string_lossy()
                    .into_owned();
                Some(FileReplacement {
                    path: path.to_string_lossy().into_owned(),
                    display_path,
                    original: content,
                    replaced,
                    match_count: count,
                })
            } else {
                None
            }
        });

    // Drop files where the replacement produces identical content (e.g.
    // replacing "X" with "X").  The match count was non-zero, but there is
    // no actual change to write, diff, or report.
    replacements.retain(|r| r.original != r.replaced);

    replacements.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(replacements)
}

fn make_file_results(replacements: &[FileReplacement]) -> Vec<ReplaceFileResult> {
    replacements
        .iter()
        .map(|r| ReplaceFileResult {
            path: r.display_path.clone(),
            match_count: r.match_count,
        })
        .collect()
}

fn make_diff_output(replacements: &[FileReplacement], color: bool) -> String {
    let diffs: Vec<_> = replacements
        .iter()
        .map(|r| unified_diff(&r.display_path, &r.original, &r.replaced))
        .collect();
    let diff_result = DiffResult { diffs };
    diff::format_diff_result_colored(&diff_result, color)
}

pub fn run(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "replace: from={:?} regex={} paths={:?}",
        args.from,
        args.regex,
        args.paths
    );
    use crate::ops::replace::{ReplaceValidationParams, validate_replace_args};
    validate_replace_args(&ReplaceValidationParams {
        pattern: &args.from,
        has_to: args.to.is_some(),
        has_insert_before: args.insert_before.is_some(),
        has_insert_after: args.insert_after.is_some(),
        nth: args.nth,
        whole_line: args.whole_line,
        multiline: args.multiline,
        has_range: args.range.is_some(),
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let cwd = global.resolve_cwd()?;
    let replacements = collect_replacements(&args, global)?;

    if replacements.is_empty() {
        if args.if_exists {
            return Ok(exit::SUCCESS);
        }
        if global.json || global.jsonl {
            let output = ReplaceOutput {
                ok: true,
                match_count: 0,
                file_count: 0,
                files: vec![],
                diff: None,
            };
            global.emit_json(&output)?;
        }
        if global.show_status() {
            let path_desc = if args.paths.is_empty() {
                ".".to_string()
            } else {
                args.paths.join(", ")
            };
            eprintln!("no matches for '{}' in {path_desc}", args.from);
            if !args.regex && crate::files::has_regex_metacharacters(&args.from) {
                eprintln!("hint: pattern contains regex characters, try --regex");
            }
            if !args.case_insensitive {
                eprintln!("hint: try -i for case-insensitive matching");
            }
        }
        return Ok(exit::NO_MATCHES);
    }

    let total_matches: usize = replacements.iter().map(|r| r.match_count).sum();
    let file_count = replacements.len();
    let files = make_file_results(&replacements);

    // --check mode: report summary, exit 2 if changes needed.
    if global.check {
        if global.json {
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files,
                diff: None,
            };
            global.emit_json(&output)?;
        } else if !global.emit_json_items(&files)? && !global.quiet {
            println!("{total_matches} match(es) in {file_count} file(s)");
            for f in &files {
                println!("  {}: {} match(es)", f.path, f.match_count);
            }
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: write changes using atomic_write with write policy.
    if global.apply {
        apply_replacements(&replacements, global, &cwd)?;
        crate::write::run_format_command(global, &cwd)?;

        let color = global.should_color();
        if global.json {
            let diff_text = if global.diff {
                Some(make_diff_output(&replacements, false))
            } else {
                None
            };
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files,
                diff: diff_text,
            };
            global.emit_json(&output)?;
        } else if !global.emit_json_items(&files)? {
            if global.diff {
                print!("{}", make_diff_output(&replacements, color));
            } else if !global.quiet {
                println!("replaced {total_matches} match(es) in {file_count} file(s)");
                for f in &files {
                    println!("  {}: {} match(es)", f.path, f.match_count);
                }
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show unified diff of changes.
    let color = global.should_color();
    if global.json {
        let output = ReplaceOutput {
            ok: true,
            match_count: total_matches,
            file_count,
            files,
            diff: Some(make_diff_output(&replacements, false)),
        };
        global.emit_json(&output)?;
    } else if !global.emit_json_items(&files)? {
        print!("{}", make_diff_output(&replacements, color));
    }
    if global.show_status() {
        eprintln!("{file_count} file(s) changed, {total_matches} replacement(s)");
    }

    // --confirm: prompt after showing diff, then apply if confirmed.
    if global.should_apply() {
        apply_replacements(&replacements, global, &cwd)?;
        crate::write::run_format_command(global, &cwd)?;
        if global.show_status() {
            eprintln!("replaced {total_matches} match(es) in {file_count} file(s)");
        }
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_args(from: &str, to: &str, paths: Vec<String>) -> ReplaceArgs {
        ReplaceArgs {
            from: from.to_string(),
            to: Some(to.to_string()),
            insert_before: None,
            insert_after: None,
            paths,
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        }
    }

    #[test]
    fn literal_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\nhello again\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "hi world\nhi again\n");
    }

    #[test]
    fn regex_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "foo123bar\nfoo456baz\n").unwrap();

        let args = ReplaceArgs {
            from: r"foo\d+".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "replacedbar\nreplacedbaz\n");
    }

    #[test]
    fn regex_capture_groups_in_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "version = \"1.2.3\"\n").unwrap();

        let args = ReplaceArgs {
            from: r#"version = "(\d+)\.(\d+)\.(\d+)""#.to_string(),
            to: Some(r#"version = "$1.$2.99""#.to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(replacements.len(), 1);
        assert_eq!(
            replacements[0].replaced, "version = \"1.2.99\"\n",
            "capture groups $1/$2 should work in replacement text"
        );
    }

    #[test]
    fn no_matches_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "zzz_no_match_zzz",
            "replacement",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn diff_mode_produces_unified_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "old line\n").unwrap();

        let args = make_args(
            "old",
            "new",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        let diff_output = make_diff_output(&replacements, false);

        assert!(diff_output.contains("--- a/"));
        assert!(diff_output.contains("+++ b/"));
        assert!(diff_output.contains("-old line"));
        assert!(diff_output.contains("+new line"));
    }

    #[test]
    fn apply_mode_writes_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multi_file_replace() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "hello from a\n").unwrap();
        fs::write(dir.path().join("b.txt"), "hello from b\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 2);
        let total: usize = replacements.iter().map(|r| r.match_count).sum();
        assert_eq!(total, 2);
    }

    #[test]
    fn if_exists_returns_success_on_no_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "zzz_no_match_zzz".to_string(),
            to: Some("replacement".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn if_exists_still_replaces_when_found() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn binary_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let bin_file = dir.path().join("data.bin");
        // Write a file with NUL bytes (binary content).
        fs::write(&bin_file, b"hello\x00world").unwrap();

        let args = make_args(
            "hello",
            "replaced",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            replacements.is_empty(),
            "binary files should be skipped, got {} matches",
            replacements.len()
        );
    }

    #[test]
    fn write_policy_ensure_final_newline_applied() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.apply = true;
        global.ensure_final_newline = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multiline_regex_spans_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: true,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 1);
        assert_eq!(replacements[0].replaced, "replaced\n");
    }

    #[test]
    fn multiline_false_does_not_span_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: Some("replaced".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert!(
            replacements.is_empty(),
            "without multiline, dot should not match newlines"
        );
    }

    #[test]
    fn check_mode_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File must not be modified in check mode.
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn identity_replacement_treated_as_no_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        // Replacing "hello" with "hello" should produce no change.
        let args = make_args(
            "hello",
            "hello",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            replacements.is_empty(),
            "identity replacement must be filtered out"
        );
    }

    #[test]
    fn identity_replacement_check_returns_no_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hello",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::NO_MATCHES,
            "--check with identity replacement must not report changes"
        );
    }

    #[test]
    fn nth_zero_is_rejected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: Some(0),
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(err.to_string().contains("1-based"), "{err}");
    }

    #[test]
    fn whole_line_deletes_matching_lines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "fn main() {\n    let _x = foo();\n    let y = bar();\n    let _z = baz();\n}\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "let _".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(
            replacements[0].replaced,
            "fn main() {\n    let y = bar();\n}\n"
        );
    }

    #[test]
    fn whole_line_regex_deletes_matching_lines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "use std::io;\nuse std::fmt;\nuse crate::foo;\nuse crate::bar;\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: r"use crate::".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "use std::io;\nuse std::fmt;\n");
    }

    #[test]
    fn whole_line_replaces_entire_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "alpha\nbeta match\ngamma\n").unwrap();

        let args = ReplaceArgs {
            from: "match".to_string(),
            to: Some("replaced line".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].replaced, "alpha\nreplaced line\ngamma\n");
    }

    #[test]
    fn whole_line_with_range_restricts_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

        let args = ReplaceArgs {
            from: "bbb".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: Some("1:3".to_string()),
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        // Only the bbb on line 2 should be deleted; the one on line 4 is outside range.
        assert_eq!(replacements[0].match_count, 1);
        assert_eq!(replacements[0].replaced, "aaa\nccc\nbbb\neee\n");
    }

    #[test]
    fn whole_line_nth_only_removes_nth_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

        let args = ReplaceArgs {
            from: "bbb".to_string(),
            to: Some(String::new()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: Some(2),
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 1);
        // Second occurrence of bbb (line 4) is deleted.
        assert_eq!(replacements[0].replaced, "aaa\nbbb\nccc\neee\n");
    }

    #[test]
    fn range_requires_whole_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: false,
            range: Some("1:5".to_string()),
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line"),
            "{err}"
        );
    }

    #[test]
    fn whole_line_and_multiline_conflict() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: Some("hi".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: true,
            nth: None,
            case_insensitive: false,
            word_boundary: false,
            whole_line: true,
            range: None,
            write: Default::default(),
        };
        let err = run(args, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string()
                .contains("whole_line and multiline cannot be combined"),
            "{err}"
        );
    }

    #[test]
    fn word_boundary_prevents_partial_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(
            &file,
            "struct SetupFile {}\nstruct BenchSetupFile {}\nlet x: SetupFile = todo!();\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(
            replacements[0].replaced,
            "struct NewFile {}\nstruct BenchSetupFile {}\nlet x: NewFile = todo!();\n",
            "word_boundary should rename standalone SetupFile but not inside BenchSetupFile"
        );
    }

    #[test]
    fn word_boundary_with_metacharacters() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        fs::write(&file, "type A = Option<SetupFile>;\n").unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(
            replacements[0].replaced, "type A = Option<NewFile>;\n",
            "word_boundary should match at angle bracket boundaries"
        );
    }

    #[test]
    fn word_boundary_does_not_match_in_string() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        // In a string, SetupFile is still at word boundaries (quotes are non-word chars)
        // so word_boundary alone won't skip strings -- that needs AST (#647).
        // But it WILL prevent matching inside compound identifiers.
        fs::write(
            &file,
            "let name = \"SetupFile\";\nlet x: SetupFileConfig = todo!();\n",
        )
        .unwrap();

        let args = ReplaceArgs {
            from: "SetupFile".to_string(),
            to: Some("NewFile".to_string()),
            insert_before: None,
            insert_after: None,
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            word_boundary: true,
            whole_line: false,
            range: None,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &GlobalFlags::test_default()).unwrap();

        assert_eq!(replacements.len(), 1);
        // SetupFile in the string IS matched (word boundaries don't know about strings)
        // SetupFileConfig is NOT matched (no word boundary between SetupFile and Config)
        assert_eq!(
            replacements[0].replaced,
            "let name = \"NewFile\";\nlet x: SetupFileConfig = todo!();\n",
        );
    }
}
