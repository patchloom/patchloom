//! `tidy check`: scan files for newline, EOL, and trailing-whitespace issues.

use crate::cli::global::GlobalFlags;
use serde::Serialize;
use std::path::Path;

/// A single tidy issue found in a file.
#[derive(Debug, Clone, Serialize)]
pub struct TidyIssue {
    pub path: String,
    pub issue: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

pub(super) fn check_file(
    path: &Path,
    quiet: bool,
    eol_target: Option<crate::write::EolMode>,
    check_trailing_ws: bool,
) -> Vec<TidyIssue> {
    let Some(text) = crate::files::read_text_file_logged(path, "tidy", quiet) else {
        return Vec::new();
    };
    let data = text.as_bytes();

    let path_str = path.to_string_lossy().into_owned();
    let mut issues = Vec::new();

    // Check missing final newline.
    if !data.is_empty() && !data.ends_with(b"\n") {
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

    // Check EOL normalization target: if the user specified --normalize-eol,
    // flag files whose line endings don't match the target even when they are
    // internally consistent (i.e. not "mixed").
    if let Some(target) = eol_target {
        let has_cr_only =
            memchr::memchr_iter(b'\r', data).any(|i| i + 1 >= data.len() || data[i + 1] != b'\n');
        match target {
            crate::write::EolMode::Lf => {
                if has_crlf || has_cr_only {
                    issues.push(TidyIssue {
                        path: path_str.clone(),
                        issue: "line endings need normalization to LF",
                        line: None,
                    });
                }
            }
            crate::write::EolMode::Crlf => {
                if has_bare_lf || has_cr_only {
                    issues.push(TidyIssue {
                        path: path_str.clone(),
                        issue: "line endings need normalization to CRLF",
                        line: None,
                    });
                }
            }
            crate::write::EolMode::Cr => {
                if has_crlf || has_bare_lf {
                    issues.push(TidyIssue {
                        path: path_str.clone(),
                        issue: "line endings need normalization to CR",
                        line: None,
                    });
                }
            }
            crate::write::EolMode::Keep => {}
        }
    }

    // Check trailing whitespace per line (skip when editorconfig says
    // trim_trailing_whitespace = false for this file type).
    if !check_trailing_ws {
        return issues;
    }
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

/// Resolve both EOL and trailing-whitespace properties from `.editorconfig`
/// in a single parse pass.
#[cfg(feature = "cli")]
fn editorconfig_check_props(path: &Path) -> (Option<crate::write::EolMode>, bool) {
    let props = match ec4rs::properties_of(path) {
        Ok(p) => p,
        Err(_) => return (None, true),
    };
    let eol = props
        .get::<ec4rs::property::EndOfLine>()
        .ok()
        .map(|val| match val {
            ec4rs::property::EndOfLine::Lf => crate::write::EolMode::Lf,
            ec4rs::property::EndOfLine::CrLf => crate::write::EolMode::Crlf,
            ec4rs::property::EndOfLine::Cr => crate::write::EolMode::Cr,
        });
    let trim = match props.get::<ec4rs::property::TrimTrailingWs>() {
        Ok(ec4rs::property::TrimTrailingWs::Value(v)) => v,
        _ => true,
    };
    (eol, trim)
}

/// Stub for non-CLI builds.
#[cfg(not(feature = "cli"))]
fn editorconfig_check_props(_path: &Path) -> (Option<crate::write::EolMode>, bool) {
    (None, true)
}

/// Collect all issues from the given paths, honouring .gitignore and optional
/// glob filtering.  Uses `collect_file_paths_opts` with `include_hidden=true`
/// so dotfiles are also checked.  File scanning is parallelized.
pub(super) fn collect_issues(
    paths: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<Vec<TidyIssue>> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, paths)?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let file_paths = crate::collect_file_paths_opts(paths, global, true, Some(&cwd))?;
    let glob_roots = crate::collect_glob_roots_from_global(paths, global, Some(&cwd))?;

    let quiet = global.quiet;
    let eol_target = global.normalize_eol;
    let respect_ec = global.respect_editorconfig;
    let file_issues: Vec<Vec<TidyIssue>> =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            // Resolve per-file EOL target: explicit --normalize-eol takes
            // precedence; otherwise consult .editorconfig when
            // --respect-editorconfig is set.
            // Resolve editorconfig properties once per file (avoid
            // double-parsing .editorconfig when both EOL and trailing-WS
            // settings are needed).
            let (file_eol_target, check_trailing_ws) = if respect_ec && eol_target.is_none() {
                editorconfig_check_props(path)
            } else {
                (eol_target, true)
            };

            let issues = check_file(path, quiet, file_eol_target, check_trailing_ws);
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
    /// Paths from `--files-from` that were missing (agent honesty).
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<Vec<String>>,
}

/// Render issues to stdout.
///
/// Structured modes propagate serialize errors (`?`) instead of discarding
/// them (`let _ =` / `if let Ok`), so `--json`/`--jsonl` never looks empty
/// while the exit code still reports issues (#1651 class).
pub(super) fn render_issues(
    issues: &[TidyIssue],
    global: &GlobalFlags,
    skipped: Option<Vec<String>>,
) -> anyhow::Result<()> {
    if global.json {
        let output = TidyCheckOutput {
            ok: issues.is_empty(),
            issue_count: issues.len(),
            issues: issues.to_vec(),
            skipped,
        };
        global.emit_json(&output)?;
    } else if global.jsonl {
        global.emit_json_items(issues)?;
    } else {
        for issue in issues {
            if let Some(line) = issue.line {
                println!("{}:{}: {}", issue.path, line, issue.issue);
            } else {
                println!("{}: {}", issue.path, issue.issue);
            }
        }
    }
    Ok(())
}

/// Run `tidy check` for the given paths.
pub(super) fn run_check(paths: &[String], global: &GlobalFlags) -> anyhow::Result<u8> {
    use crate::exit;
    crate::verbose!("tidy: checking {} path(s)", paths.len());
    let cwd = global.resolve_cwd()?;
    if crate::files::all_scan_targets_missing(global, paths, Some(&cwd))? {
        let msg = format!(
            "no such file or directory: {}",
            global.path_scope_description(paths)
        );
        global.emit_error_json_kind(Some("not_found"), &msg)?;
        return Ok(exit::FAILURE);
    }
    let skipped = crate::files::files_from_missing_entries(global, &cwd)?;
    let issues = collect_issues(paths, global)?;
    if !global.quiet || global.json || global.jsonl {
        render_issues(&issues, global, skipped)?;
    }
    if issues.is_empty() {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::CHANGES_DETECTED)
    }
}
