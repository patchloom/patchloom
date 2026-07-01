use crate::cli::global::GlobalFlags;
use crate::diff::{render_diffs_colored, render_diffs_plain};
use crate::exit;
use crate::ops::replace::{
    compile_replace_regex, replace_content, replace_whole_lines, replacement_text,
};
use crate::tx::engine::{ExecuteOptions, execute_precomputed};
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom replace 'old_name' --new 'new_name' src/
  patchloom replace 'http://' --new 'https://' src/ --apply
  patchloom replace 'v1\\.0' --new 'v2.0' --regex README.md")]
pub struct ReplaceArgs {
    /// Pattern to find.
    pub old: String,
    /// Text to replace with.
    #[arg(long)]
    pub new: Option<String>,
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
    /// Useful for deleting lines (--whole-line --new '') or replacing full lines.
    #[arg(long, short = 'L')]
    pub whole_line: bool,
    // ref:replace-mode:word-boundary
    /// Match only at word boundaries. Prevents 'SetupFile' from matching
    /// inside 'BenchSetupFile'. The pattern is auto-escaped for regex
    /// metacharacters before wrapping with \\b anchors.
    #[arg(long, short = 'w')]
    pub word_boundary: bool,
    // ref:replace-mode:range
    /// Restrict matching to a line range (e.g. '10:50' or '10-50'). 1-based, inclusive.
    #[arg(long, short = 'R')]
    pub range: Option<String>,
    // ref:replace-mode:before-context
    /// Context line(s) before the target for anchor-based disambiguation.
    /// When the pattern matches multiple times, the match nearest to this
    /// context is selected. Falls back to fuzzy anchor matching when the
    /// exact text is not found.
    #[arg(long)]
    pub before_context: Option<String>,
    // ref:replace-mode:after-context
    /// Context line(s) after the target for anchor-based disambiguation.
    /// When the pattern matches multiple times, the match nearest to this
    /// context is selected. Falls back to fuzzy anchor matching when the
    /// exact text is not found.
    #[arg(long)]
    pub after_context: Option<String>,
    // ref:replace-mode:unique
    /// Fail if the pattern matches more than once in any single file.
    /// Enforces unambiguous edits by returning exit code 5 (AMBIGUOUS)
    /// when multiple matches exist.
    #[arg(long, short = 'u')]
    pub unique: bool,
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
        &args.old,
        &args.new,
        &args.insert_before,
        &args.insert_after,
        args.regex || args.case_insensitive || args.word_boundary,
        args.regex,
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
        &args.old,
        args.regex,
        args.case_insensitive,
        args.multiline,
        args.word_boundary,
    )?;

    let from = &args.old;
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

pub fn run(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "replace: old={:?} regex={} paths={:?}",
        args.old,
        args.regex,
        args.paths
    );
    use crate::ops::replace::{ReplaceValidationParams, validate_replace_args};
    validate_replace_args(&ReplaceValidationParams {
        pattern: &args.old,
        has_to: args.new.is_some(),
        has_insert_before: args.insert_before.is_some(),
        has_insert_after: args.insert_after.is_some(),
        nth: args.nth,
        whole_line: args.whole_line,
        multiline: args.multiline,
        has_range: args.range.is_some(),
    })
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let cwd = global.resolve_cwd()?;

    // Context-based replace: route through the tx engine where the
    // context_filtered_offset and fallback chain live.
    if args.before_context.is_some() || args.after_context.is_some() {
        return run_context_replace(args, global, &cwd);
    }

    // Phase 1: Parallel file scan to identify files with matches.
    let replacements = collect_replacements(&args, global)?;

    // Unique check: fail if any file has more than one match.
    if args.unique {
        for r in &replacements {
            if r.match_count > 1 {
                if global.json || global.jsonl {
                    let output = ReplaceOutput {
                        ok: false,
                        match_count: r.match_count,
                        file_count: 1,
                        files: vec![ReplaceFileResult {
                            path: r.display_path.clone(),
                            match_count: r.match_count,
                        }],
                        diff: None,
                    };
                    global.emit_json(&output)?;
                }
                if global.show_status() {
                    eprintln!(
                        "ambiguous match: pattern {:?} matches {} times in {}; use --nth or add context to disambiguate",
                        crate::fallback::truncate_str(&args.old, 60),
                        r.match_count,
                        r.display_path,
                    );
                }
                return Ok(exit::AMBIGUOUS);
            }
        }
    }

    if replacements.is_empty() {
        if args.if_exists {
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
            eprintln!("no matches for '{}' in {path_desc}", args.old);
            if !args.regex && crate::files::has_regex_metacharacters(&args.old) {
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

    // Phase 2: Feed pre-computed results to the engine for commit/diff/backup.
    // The scan phase already read files and computed replacements; we pass
    // (display_path, original, replaced) directly to avoid re-reading files.
    let precomputed = replacements
        .iter()
        .map(|r| {
            (
                r.display_path.clone(),
                r.original.clone(),
                r.replaced.clone(),
            )
        })
        .collect();

    let options = ExecuteOptions {
        cwd: &cwd,
        global,
        guard: None,
    };
    let result = execute_precomputed(precomputed, options);

    // Phase 3: Render output based on mode (check/apply/diff/confirm).
    replace_output(global, result, &files, total_matches, file_count, &cwd)
}

/// Handle output rendering and commit/check/preview for replace via the engine.
fn replace_output(
    global: &GlobalFlags,
    result: crate::tx::engine::ExecutionResult,
    files: &[ReplaceFileResult],
    total_matches: usize,
    file_count: usize,
    cwd: &std::path::Path,
) -> anyhow::Result<u8> {
    // --check mode: report summary, exit 2 if changes needed.
    if global.check {
        if global.json {
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files: files.to_vec(),
                diff: None,
            };
            global.emit_json(&output)?;
        } else if !global.emit_json_items(files)? && !global.quiet {
            println!("{total_matches} match(es) in {file_count} file(s)");
            for f in files {
                println!("  {}: {} match(es)", f.path, f.match_count);
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
        if global.json {
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files: files.to_vec(),
                diff: diff_text,
            };
            global.emit_json(&output)?;
        } else if !global.emit_json_items(files)? {
            if global.diff {
                print!("{}", render_diffs_colored(&diffs, global.should_color()));
            } else if !global.quiet {
                println!("replaced {total_matches} match(es) in {file_count} file(s)");
                for f in files {
                    println!("  {}: {} match(es)", f.path, f.match_count);
                }
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: preview diffs.
    let diffs = result.build_diffs();
    let diff_text = if !diffs.is_empty() {
        Some(render_diffs_plain(&diffs))
    } else {
        None
    };

    if global.json {
        let output = ReplaceOutput {
            ok: true,
            match_count: total_matches,
            file_count,
            files: files.to_vec(),
            diff: diff_text,
        };
        global.emit_json(&output)?;
    } else if !global.emit_json_items(files)? && !diffs.is_empty() {
        print!("{}", render_diffs_colored(&diffs, global.should_color()));
    }
    if global.show_status() {
        eprintln!("{file_count} file(s) changed, {total_matches} replacement(s)");
    }

    // --confirm: prompt after showing diff, then apply if confirmed.
    if global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, cwd)?;
        if global.show_status() {
            eprintln!("replaced {total_matches} match(es) in {file_count} file(s)");
        }
        return Ok(exit::SUCCESS);
    }

    Ok(exit::CHANGES_DETECTED)
}

/// Context-based replace: routes through the tx engine where
/// `context_filtered_offset` and the fallback chain live.
fn run_context_replace(
    args: ReplaceArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
) -> anyhow::Result<u8> {
    use crate::plan::Operation;
    use crate::tx::engine::{ExecuteOptions, execute_operations};

    // Context-based replace requires explicit file paths (not directory scan).
    if args.paths.is_empty() {
        anyhow::bail!("--before-context/--after-context requires explicit file paths");
    }

    let ops: Vec<Operation> = args
        .paths
        .iter()
        .map(|p| Operation::Replace {
            glob: None,
            path: Some(p.clone()),
            regex: args.regex,
            old: args.old.clone(),
            new_text: args.new.clone(),
            nth: args.nth,
            insert_before: args.insert_before.clone(),
            insert_after: args.insert_after.clone(),
            case_insensitive: args.case_insensitive,
            multiline: args.multiline,
            if_exists: args.if_exists,
            whole_line: args.whole_line,
            range: args.range.clone(),
            word_boundary: args.word_boundary,
            before_context: args.before_context.clone(),
            after_context: args.after_context.clone(),
            unique: args.unique,
        })
        .collect();

    let options = ExecuteOptions {
        cwd,
        global,
        guard: None,
    };
    let result = execute_operations(ops, options)?;

    if !result.has_changes {
        if args.if_exists {
            return Ok(exit::SUCCESS);
        }
        if global.show_status() {
            eprintln!("no matches for '{}' in context-based replace", args.old);
        }
        return Ok(exit::NO_MATCHES);
    }

    let total_matches = result.exec_result.changes.len();

    // --check mode.
    if global.check {
        if !global.quiet {
            println!("{total_matches} file(s) changed");
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode.
    if global.apply {
        let diffs = result.build_diffs();
        result.commit()?;
        crate::write::run_format_command(global, cwd)?;

        if global.diff {
            print!("{}", render_diffs_colored(&diffs, global.should_color()));
        } else if !global.quiet {
            println!("replaced in {total_matches} file(s) (context-based)");
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff: preview.
    let diffs = result.build_diffs();
    if !diffs.is_empty() {
        print!("{}", render_diffs_colored(&diffs, global.should_color()));
    }
    if global.show_status() {
        eprintln!("{total_matches} file(s) changed");
    }

    if global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, cwd)?;
        return Ok(exit::SUCCESS);
    }

    Ok(exit::CHANGES_DETECTED)
}

#[path = "replace_tests.rs"]
#[cfg(test)]
mod tests;
