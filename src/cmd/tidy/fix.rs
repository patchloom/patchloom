//! `tidy fix`: parallel dirty-file scan, stage via engine, finalize_report.

use crate::cli::global::GlobalFlags;
use crate::diff::render_diffs_colored;
use crate::exit;
use crate::plan::Operation;
use crate::tx::engine::{ExecutionResult, WriteSource};
use crate::write::{apply_policy, policy_from_flags};
use serde::Serialize;
use std::path::Path;

/// Per-file result for tidy fix structured output.
#[derive(Debug, Clone, Serialize)]
pub(super) struct TidyFixFileResult {
    pub path: String,
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

/// Convert `EolMode` to the string format expected by `Operation::TidyFix`.
pub(super) fn eol_mode_to_str(mode: crate::cli::global::EolMode) -> &'static str {
    match mode {
        crate::cli::global::EolMode::Lf => "lf",
        crate::cli::global::EolMode::Crlf => "crlf",
        crate::cli::global::EolMode::Cr => "cr",
        crate::cli::global::EolMode::Keep => "keep",
    }
}

/// Handle output rendering and commit/check/preview for tidy fix via the engine.
///
/// Mode/exit owned by [`crate::cmd::write_mode::finalize_report`].
pub(super) fn tidy_fix_output(
    global: &GlobalFlags,
    result: ExecutionResult,
    dirty_rel_paths: &[String],
    cwd: &Path,
) -> anyhow::Result<u8> {
    use crate::cmd::write_mode::{FinalizeCallbacks, finalize_report};

    let fix_files: Vec<TidyFixFileResult> = dirty_rel_paths
        .iter()
        .map(|p| TidyFixFileResult { path: p.clone() })
        .collect();
    let n_files = dirty_rel_paths.len();

    finalize_report(
        global,
        cwd,
        result,
        true,
        FinalizeCallbacks {
            on_check: |g: &GlobalFlags, _has: bool, _diffs: &[crate::diff::FileDiff]| {
                emit_tidy_fix_output(g, &fix_files, None)?;
                if !g.quiet && !g.json && !g.jsonl {
                    for p in dirty_rel_paths {
                        println!("{p}");
                    }
                }
                Ok(())
            },
            on_apply: |g: &GlobalFlags,
                       _has: bool,
                       _diffs: &[crate::diff::FileDiff],
                       diff_text: Option<String>| {
                emit_tidy_fix_output(g, &fix_files, diff_text)?;
                Ok(())
            },
            on_preview: |g: &GlobalFlags,
                         _has: bool,
                         diffs: &[crate::diff::FileDiff],
                         diff_text: Option<String>| {
                emit_tidy_fix_output(g, &fix_files, diff_text)?;
                if !g.json && !g.jsonl && !g.quiet && !diffs.is_empty() {
                    print!("{}", render_diffs_colored(diffs, g.should_color()));
                }
                Ok(())
            },
            after_preview_emit: |g: &GlobalFlags| {
                if g.show_status() {
                    eprintln!("{n_files} file(s) changed");
                }
            },
            after_preview_apply: |_: &GlobalFlags| {},
        },
    )
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

/// Run `tidy fix` for the given paths and optional dedent/indent/lines.
pub(super) fn run_fix(
    paths: Vec<String>,
    dedent: Option<String>,
    indent: Option<String>,
    lines: Option<String>,
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    crate::verbose!("tidy: fixing {} path(s)", paths.len());
    if dedent.is_some() && indent.is_some() {
        anyhow::bail!("--dedent and --indent cannot both be set");
    }

    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, &paths)?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let fix_file_paths = crate::collect_file_paths_opts(&paths, global, true, Some(&cwd))?;
    let glob_roots = crate::collect_glob_roots_from_global(&paths, global, Some(&cwd))?;

    let line_range = lines
        .as_deref()
        .map(crate::ops::read::parse_line_range)
        .transpose()?;

    let quiet = global.quiet;
    let dedent_ref = dedent.as_deref();
    let indent_ref = indent.as_deref();
    let dirty_rel_paths: Vec<String> = crate::par_process_files(
        &fix_file_paths,
        glob_matcher.as_ref(),
        &glob_roots,
        |file_path| {
            let original = match crate::files::read_text_file_logged(file_path, "tidy", quiet) {
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

    crate::verbose!("tidy: {} file(s) need fixing", dirty_rel_paths.len());
    if dirty_rel_paths.is_empty() {
        emit_tidy_fix_output(global, &[], None)?;
        return Ok(exit::SUCCESS);
    }

    let eol_str = global.normalize_eol.map(eol_mode_to_str);
    let collapse = if global.collapse_blanks {
        Some(true)
    } else {
        None
    };
    let ops: Vec<Operation> = dirty_rel_paths
        .iter()
        .map(|rel_path| Operation::TidyFix {
            path: rel_path.clone(),
            ensure_final_newline: Some(global.ensure_final_newline),
            trim_trailing_whitespace: Some(global.trim_trailing_whitespace),
            normalize_eol: eol_str.map(String::from),
            collapse_blanks: collapse,
            dedent: dedent.clone(),
            indent: indent.clone(),
            lines: lines.clone(),
        })
        .collect();

    let (cwd, result) = crate::cmd::output::stage_for_write(WriteSource::Operations(ops), global)?;

    tidy_fix_output(global, result, &dirty_rel_paths, &cwd)
}
