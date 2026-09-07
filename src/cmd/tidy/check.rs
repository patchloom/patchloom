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
    charset: crate::write::CharsetMode,
) -> Vec<TidyIssue> {
    let Some(text) = crate::files::read_text_file_logged(path, "tidy", quiet) else {
        return Vec::new();
    };
    let data = text.as_bytes();

    let path_str = path.to_string_lossy().into_owned();
    let mut issues = Vec::new();

    match charset {
        crate::write::CharsetMode::Unsupported(name) => {
            issues.push(TidyIssue {
                path: path_str.clone(),
                issue: unsupported_charset_issue(name),
                line: None,
            });
        }
        crate::write::CharsetMode::Utf8Bom if !text.starts_with('\u{feff}') => {
            issues.push(TidyIssue {
                path: path_str.clone(),
                issue: "missing UTF-8 BOM",
                line: None,
            });
        }
        crate::write::CharsetMode::Utf8 if text.starts_with('\u{feff}') => {
            issues.push(TidyIssue {
                path: path_str.clone(),
                issue: "unexpected UTF-8 BOM",
                line: None,
            });
        }
        _ => {}
    }

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

struct EditorconfigCheck {
    eol: Option<crate::write::EolMode>,
    trim: bool,
    charset: crate::write::CharsetMode,
}

fn unsupported_charset_issue(name: &'static str) -> &'static str {
    match name {
        "utf-16le" => "editorconfig charset 'utf-16le' is not supported; use utf-8 or utf-8-bom",
        "utf-16be" => "editorconfig charset 'utf-16be' is not supported; use utf-8 or utf-8-bom",
        _ => "editorconfig charset is not supported; use utf-8 or utf-8-bom",
    }
}

/// Resolve EOL, trailing-whitespace, and charset from `.editorconfig`
/// in a single parse pass.
#[cfg(feature = "cli")]
fn editorconfig_check_props(path: &Path) -> EditorconfigCheck {
    let props = match ec4rs::properties_of(path) {
        Ok(p) => p,
        Err(_) => {
            return EditorconfigCheck {
                eol: None,
                trim: true,
                charset: crate::write::CharsetMode::Keep,
            };
        }
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
    let charset = crate::write::charset_from_editorconfig_props(&props);
    EditorconfigCheck { eol, trim, charset }
}

/// First explicit file (not a directory) whose EditorConfig charset we
/// cannot apply. Checked before the binary peel so a UTF-16 dest is
/// `invalid_input`, not a silent skip or sole `binary`.
pub(super) fn first_unsupported_editorconfig_charset(
    paths: &[String],
    global: &GlobalFlags,
    cwd: &Path,
) -> Option<&'static str> {
    if !global.respect_editorconfig {
        return None;
    }
    for p in paths {
        let abs = {
            let raw = Path::new(p);
            if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                cwd.join(raw)
            }
        };
        if abs.is_dir() {
            continue;
        }
        if let crate::write::CharsetMode::Unsupported(name) = editorconfig_check_props(&abs).charset
        {
            return Some(name);
        }
    }
    None
}

/// Stub for non-CLI builds.
#[cfg(not(feature = "cli"))]
fn editorconfig_check_props(_path: &Path) -> EditorconfigCheck {
    EditorconfigCheck {
        eol: None,
        trim: true,
        charset: crate::write::CharsetMode::Keep,
    }
}

/// First walk plus issues. Remask reuses `scanned`; keep this off the
/// public [`TidyIssue`] surface (crate-private, not a library type).
pub(super) struct CollectedIssues {
    pub issues: Vec<TidyIssue>,
    pub scanned: Vec<std::path::PathBuf>,
}

/// Collect all issues from the given paths, honouring .gitignore and optional
/// glob filtering.  Uses `collect_file_paths_opts` with `include_hidden=true`
/// so dotfiles are also checked.  File scanning is parallelized.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn collect_issues(
    paths: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<Vec<TidyIssue>> {
    Ok(collect_issues_with_list(paths, global, None)?.issues)
}

/// Like [`collect_issues`], with a pre-read `--files-from` list (stdin once).
/// Returns the first walk so empty-scan remask can reuse it.
pub(super) fn collect_issues_with_list(
    paths: &[String],
    global: &GlobalFlags,
    files_from_preload: Option<&[String]>,
) -> anyhow::Result<CollectedIssues> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, paths)?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let file_paths = crate::files::collect_file_paths_opts_with_list(
        paths,
        global,
        true,
        Some(&cwd),
        files_from_preload,
        None,
    )?;
    // Empty --files-from must not report ok:true / zero issues (#1796).
    crate::files::ensure_files_from_nonempty(global, &file_paths)?;
    let glob_roots = crate::collect_glob_roots_from_global(paths, global, Some(&cwd))?;

    let quiet = global.quiet || global.json || global.jsonl;
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
            let (file_eol_target, check_trailing_ws, charset) = if respect_ec {
                let ec = editorconfig_check_props(path);
                if eol_target.is_none() {
                    (ec.eol, ec.trim, ec.charset)
                } else {
                    (eol_target, true, ec.charset)
                }
            } else {
                (eol_target, true, crate::write::CharsetMode::Keep)
            };

            if let crate::write::CharsetMode::Unsupported(name) = charset {
                return Some(vec![TidyIssue {
                    path: path.to_string_lossy().into_owned(),
                    issue: unsupported_charset_issue(name),
                    line: None,
                }]);
            }
            let issues = check_file(path, quiet, file_eol_target, check_trailing_ws, charset);
            if issues.is_empty() {
                None
            } else {
                Some(issues)
            }
        });

    Ok(CollectedIssues {
        issues: file_issues.into_iter().flatten().collect(),
        scanned: file_paths,
    })
}

/// JSON wrapper for tidy check output.
#[derive(Debug, Serialize)]
struct TidyCheckOutput {
    ok: bool,
    issue_count: usize,
    issues: Vec<TidyIssue>,
    /// When issues exist: `changes_detected` so agents can branch without
    /// scraping `issue_count` (parity with search `--assert-count` JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    /// Set with [`Self::status`] when `ok` is false due to tidy issues.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    /// Paths from `--files-from` that were missing (agent honesty).
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<Vec<String>>,
    /// Explicit multi-path co-targets soft-skipped (e.g. binary).
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<Vec<crate::ops::file::PathRefused>>,
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
    refused: Option<Vec<crate::ops::file::PathRefused>>,
) -> anyhow::Result<()> {
    if global.json {
        let dirty = !issues.is_empty();
        let output = TidyCheckOutput {
            ok: !dirty,
            issue_count: issues.len(),
            issues: issues.to_vec(),
            status: dirty.then_some("changes_detected"),
            error_kind: dirty.then_some("changes_detected"),
            skipped,
            refused,
        };
        global.emit_json(&output)?;
    } else if global.jsonl {
        // Stream issues, then dirty summary so agents that only parse stdout
        // still see error_kind / skipped / refused (parity with --json).
        global.emit_json_items(issues)?;
        let dirty = !issues.is_empty();
        global.emit_json(&serde_json::json!({
            "type": "summary",
            "ok": !dirty,
            "issue_count": issues.len(),
            "status": if dirty { Some("changes_detected") } else { None::<&str> },
            "error_kind": if dirty { Some("changes_detected") } else { None::<&str> },
            "skipped": skipped,
            "refused": refused,
        }))?;
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
    // Read --files-from once (including stdin `-`); do not re-read empty stdin.
    let files_from_list = global.read_files_from()?;
    let charset_paths = files_from_list.as_deref().unwrap_or(paths);
    if let Some(name) = first_unsupported_editorconfig_charset(charset_paths, global, &cwd) {
        let msg = format!("editorconfig charset '{name}' is not supported; use utf-8 or utf-8-bom");
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(exit::FAILURE);
    }
    if let Some(err) =
        crate::ops::file::sole_explicit_non_text_for_scan(paths, files_from_list.as_deref(), &cwd)
    {
        {
            let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
            let msg = crate::exit::agent_error_message(&err);
            global.emit_error_json_kind(Some(kind), &msg)?;
        }
        return Ok(exit::FAILURE);
    }
    let skipped = if files_from_list.is_some() {
        files_from_list
            .as_ref()
            .and_then(|files| crate::files::explicit_paths_missing_entries(&cwd, files))
    } else {
        crate::files::scan_missing_entries(global, &cwd, paths)?
    };
    let refuse_paths: &[String] = files_from_list.as_deref().unwrap_or(paths);
    let refused = crate::ops::file::explicit_multi_path_non_text_refused(refuse_paths, &cwd);
    let CollectedIssues { issues, scanned } =
        collect_issues_with_list(paths, global, files_from_list.as_deref())?;
    if let Some(issue) = issues.iter().find(|i| {
        i.issue.starts_with("editorconfig charset") && i.issue.contains("is not supported")
    }) {
        global.emit_error_json_kind(Some("invalid_input"), issue.issue)?;
        return Ok(exit::FAILURE);
    }
    if issues.is_empty() {
        // Unreadable paths soft-skipped as "clean" would mask permission failures.
        // Reuse the first walk; do not collect_file_paths again on a clean tree.
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&scanned, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
    }
    if !global.quiet || global.json || global.jsonl {
        render_issues(&issues, global, skipped, refused)?;
    }
    if issues.is_empty() {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::CHANGES_DETECTED)
    }
}
