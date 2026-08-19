//! size-waiver: accepted single-domain bulk (policy #1408). CLI search collect,
//! format (JSON/JSONL/human), assert_count, and agent honesty for truncated /
//! skipped paths is one unit; do not split for LOC alone.
use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::ops::search::{self as ops_search, SearchMatch, SearchResults};
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom search 'TODO' src/
  patchloom search 'fn main' src/ --count
  patchloom search 'error|warn' --regex src/ -C 2
  patchloom search 'password' . --glob '*.yaml'
  patchloom search 'TODO' . --ignore-file .agentignore --exclude 'target/**' --max-results 50")]
pub struct SearchArgs {
    /// Pattern to search for.
    pub pattern: String,
    /// Paths to search in.
    pub paths: Vec<String>,
    /// Treat pattern as a literal string.
    #[arg(long, short = 'F', conflicts_with = "regex")]
    pub literal: bool,
    /// Treat pattern as a regex (default).
    #[arg(long, conflicts_with = "literal")]
    pub regex: bool,
    /// Lines of context around matches (shorthand for -B N -A N).
    #[arg(long, short = 'C')]
    pub context: Option<usize>,
    /// Lines of context before each match.
    #[arg(long, short = 'B')]
    pub before_context: Option<usize>,
    /// Lines of context after each match.
    #[arg(long, short = 'A')]
    pub after_context: Option<usize>,
    // ref:search-mode:files-with-matches
    /// Only print file paths with matches.
    #[arg(
        long,
        short = 'l',
        conflicts_with_all = ["count", "files_without_match"]
    )]
    pub files_with_matches: bool,
    // ref:search-mode:files-without-match
    /// Only print file paths with no matches (grep -L).
    #[arg(
        long,
        short = 'L',
        conflicts_with_all = ["files_with_matches", "count"]
    )]
    pub files_without_match: bool,
    // ref:search-mode:count
    /// Only print match counts per file (occurrence counts, same total as replace).
    #[arg(
        long,
        short = 'c',
        conflicts_with_all = ["files_with_matches", "files_without_match"]
    )]
    pub count: bool,
    // ref:search-mode:invert-match
    /// Show lines that do NOT match the pattern.
    #[arg(long, short = 'v')]
    pub invert_match: bool,
    // ref:search-mode:multiline
    /// Enable multiline matching (dot matches newlines).
    #[arg(long, short = 'U')]
    pub multiline: bool,
    // ref:search-mode:case-insensitive
    /// Case-insensitive matching.
    #[arg(long, short = 'i')]
    pub case_insensitive: bool,
    // ref:search-mode:assert-count
    /// Assert that the total match count equals N. Exits 0 if exact, 2 otherwise.
    #[arg(long)]
    pub assert_count: Option<usize>,

    /// Maximum number of detailed match results to return (0 = unlimited).
    /// Also caps the file list in `--count` / `--files-with-matches` /
    /// `--files-without-match` (#1798).
    /// `match_count` / total file inventory counts stay full; JSON sets
    /// `truncated: true` when a list was cut.
    #[arg(long, default_value_t = 0)]
    pub max_results: usize,
}

impl SearchArgs {
    /// Path-only listing (`-l` / `-L`): emit file paths, not match lines.
    pub(crate) fn file_path_list_mode(&self) -> bool {
        self.files_with_matches || self.files_without_match
    }

    /// Count or path-only modes share the file inventory JSON/JSONL shape.
    pub(crate) fn file_inventory_mode(&self) -> bool {
        self.count || self.file_path_list_mode()
    }
}

/// Explicit path refused for non-pattern reasons (parity with replace `refused[]`).
#[derive(Debug, Serialize, Clone)]
pub(crate) struct SearchRefused {
    path: String,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
struct SearchOutput {
    ok: bool,
    matches: Vec<SearchMatch>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<SearchFileEntry>,
    match_count: usize,
    file_count: usize,
    /// True when `matches` was capped by `--max-results` while `match_count`
    /// is the full total. Agents must not treat `matches.len()` as complete
    /// coverage when this is set (fixrealloop 2026-07-16).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
    /// Machine-readable failure kind for agents (`no_matches`), aligned with
    /// CLI replace / tx JSON. Omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    /// Human/agent diagnostic on soft no-match (path scope). Omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Paths from `--files-from` that were missing (agent JSON; #1756).
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<Vec<String>>,
    /// Explicit multi-path co-targets that were not searched (e.g. binary).
    /// Directory walks are not expanded into mass refused (parity with replace).
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<Vec<SearchRefused>>,
}

#[derive(Debug, Serialize)]
struct SearchFileEntry {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
}

#[derive(Debug, Serialize)]
struct SearchAssertCount {
    expected: usize,
    actual: usize,
    matched: bool,
}

#[derive(Debug, Serialize)]
struct SearchAssertCountOutput {
    ok: bool,
    status: &'static str,
    assert_count: SearchAssertCount,
    /// Set on mismatch so agents can branch like tx (`error_kind: changes_detected`).
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
}

pub(crate) fn collect_matches(
    args: &SearchArgs,
    global: &GlobalFlags,
) -> anyhow::Result<SearchResults> {
    collect_matches_with_list(args, global, None)
}

/// Like [`collect_matches`], with a pre-read `--files-from` list (stdin once).
pub(crate) fn collect_matches_with_list(
    args: &SearchArgs,
    global: &GlobalFlags,
    files_from_preload: Option<&[String]>,
) -> anyhow::Result<SearchResults> {
    crate::verbose!(
        "search: pattern={:?} literal={} paths={:?}",
        args.pattern,
        args.literal,
        args.paths
    );
    let cwd = global.resolve_cwd()?;
    // --contain applies to search paths (agent sandbox must not read secrets
    // outside the workspace; MPI cycle 18).
    global.check_paths_contained(&cwd, &args.paths)?;
    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let matcher = ops_search::build_matcher(
        &args.pattern,
        args.literal,
        args.case_insensitive,
        args.multiline,
    )?;
    let file_paths = crate::files::collect_file_paths_opts_with_list(
        &args.paths,
        global,
        false,
        Some(&cwd),
        files_from_preload,
        None,
    )?;
    // Empty --files-from is invalid_input, not pattern no_matches (#1796).
    crate::files::ensure_files_from_nonempty(global, &file_paths)?;
    let glob_roots = crate::collect_glob_roots_from_global(&args.paths, global, Some(&cwd))?;
    let count_only = args.count || args.files_with_matches || args.files_without_match;

    // Process files in parallel (#167).
    let cwd_ref = &cwd;
    let search_params = ops_search::SearchFileParams {
        multiline: args.multiline,
        invert_match: args.invert_match,
        count_only,
        files_with_matches: args.files_with_matches,
        files_without_match: args.files_without_match,
        assert_count: args.assert_count,
        before_context: args.before_context,
        after_context: args.after_context,
        context: args.context,
        quiet: global.quiet,
    };
    let file_results =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            ops_search::search_one_file(path, &matcher, &search_params, cwd_ref)
        });

    Ok(ops_search::merge_file_results(
        file_results,
        count_only,
        args.max_results,
    ))
}

/// Explicit multi-file lists only: co-listed non-text / unreadable paths.
/// Sole non-text is hard `invalid_input` elsewhere; directory walks omit refused.
pub(crate) fn explicit_binary_refused(
    args: &SearchArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
) -> Option<Vec<SearchRefused>> {
    explicit_binary_refused_with_list(args, global, cwd, None)
}

/// Like [`explicit_binary_refused`], with a pre-read `--files-from` list.
pub(crate) fn explicit_binary_refused_with_list(
    args: &SearchArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
    files_from_preload: Option<&[String]>,
) -> Option<Vec<SearchRefused>> {
    let explicit = if global.files_from.is_some() {
        // Match replace: files-from counts as explicit list.
        true
    } else if args.paths.len() < 2 {
        // Sole path uses sole_explicit_non_text; no refused[] needed.
        false
    } else {
        args.paths.iter().all(|p| !cwd.join(p).is_dir())
    };
    if !explicit {
        return None;
    }
    // Prefer caller-resolved list so `--files-from -` is not re-read (empty).
    let owned;
    let path_strings: &[String] = if let Some(pre) = files_from_preload {
        pre
    } else if global.files_from.is_some() {
        owned = global.read_files_from().ok().flatten().unwrap_or_default();
        &owned
    } else {
        &args.paths
    };
    // Shared helper: binary / invalid_utf8 / unreadable (len < 2 → None;
    // sole is invalid_input via sole_explicit_non_text_for_scan).
    let shared = crate::ops::file::explicit_multi_path_non_text_refused(path_strings, cwd)?;
    let mut refused: Vec<SearchRefused> = shared
        .into_iter()
        .map(|r| SearchRefused {
            path: r.path,
            reason: r.reason,
        })
        .collect();
    refused.sort_by(|a, b| a.path.cmp(&b.path));
    Some(refused)
}

pub(crate) fn format_results(
    results: SearchResults,
    args: &SearchArgs,
    global: &GlobalFlags,
    skipped: Option<Vec<String>>,
    refused: Option<Vec<SearchRefused>>,
) -> anyhow::Result<String> {
    use std::fmt::Write;
    let mut out = String::new();

    if global.json {
        let match_count: usize = results.file_match_counts.values().sum();
        let full_file_count = results.file_match_counts.len();
        // Cap file lists in count / path-only modes when --max-results > 0
        // so agents get one pagination budget across modes (#1798).
        let mut files: Vec<SearchFileEntry> = if args.file_path_list_mode() {
            results
                .file_match_counts
                .keys()
                .map(|p| SearchFileEntry {
                    path: p.to_string(),
                    count: None,
                })
                .collect()
        } else if args.count {
            results
                .file_match_counts
                .iter()
                .map(|(p, c)| SearchFileEntry {
                    path: p.to_string(),
                    count: Some(*c),
                })
                .collect()
        } else {
            Vec::new()
        };
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let files_truncated =
            if args.max_results > 0 && args.file_inventory_mode() && files.len() > args.max_results
            {
                files.truncate(args.max_results);
                true
            } else {
                false
            };
        // match_count is always full; detailed matches may be capped by
        // --max-results. Count modes set truncated when the file list was cut.
        let truncated = if args.file_inventory_mode() {
            files_truncated
        } else {
            args.max_results > 0 && results.matches.len() < match_count
        };
        let payload = SearchOutput {
            ok: true,
            match_count,
            // Full inventory size even when `files` is capped (#1798).
            file_count: full_file_count,
            matches: results.matches,
            files,
            truncated,
            error_kind: None,
            error: None,
            skipped,
            refused,
        };
        out = serde_json::to_string_pretty(&payload)?;
        out.push('\n');
    } else if global.jsonl {
        if args.file_inventory_mode() {
            // Cap file streams under --max-results (#1798) and trailer when cut.
            let mut paths: Vec<(String, usize)> = results
                .file_match_counts
                .iter()
                .map(|(p, c)| (p.to_string(), *c))
                .collect();
            paths.sort_by(|a, b| a.0.cmp(&b.0));
            let full_file_count = paths.len();
            let truncated = args.max_results > 0 && paths.len() > args.max_results;
            if truncated {
                paths.truncate(args.max_results);
            }
            for (path, count) in &paths {
                if args.file_path_list_mode() {
                    out.push_str(&serde_json::to_string(&serde_json::json!({"path": path}))?);
                } else {
                    out.push_str(&serde_json::to_string(
                        &serde_json::json!({"path": path, "count": count}),
                    )?);
                }
                out.push('\n');
            }
            let need_summary = truncated
                || skipped.as_ref().is_some_and(|s| !s.is_empty())
                || refused.as_ref().is_some_and(|r| !r.is_empty());
            if need_summary {
                out.push_str(&serde_json::to_string(&serde_json::json!({
                    "type": "summary",
                    "ok": true,
                    "file_count": full_file_count,
                    "file_emitted": paths.len(),
                    "truncated": truncated,
                    "skipped": skipped,
                    "refused": refused,
                }))?);
                out.push('\n');
            }
        } else {
            let match_count: usize = results.file_match_counts.values().sum();
            let emitted = results.matches.len();
            let truncated = args.max_results > 0 && emitted < match_count;
            for m in &results.matches {
                out.push_str(&serde_json::to_string(m)?);
                out.push('\n');
            }
            // Trailer when --max-results capped the stream, or when skipped/
            // refused would only appear under --json (agent --jsonl parity).
            let need_summary = truncated
                || skipped.as_ref().is_some_and(|s| !s.is_empty())
                || refused.as_ref().is_some_and(|r| !r.is_empty());
            if need_summary {
                out.push_str(&serde_json::to_string(&serde_json::json!({
                    "type": "summary",
                    "ok": true,
                    "match_count": match_count,
                    "match_emitted": emitted,
                    "file_count": results.file_match_counts.len(),
                    "truncated": truncated,
                    "skipped": skipped,
                    "refused": refused,
                }))?);
                out.push('\n');
            }
        }
    } else if args.file_path_list_mode() {
        let color = global.should_color();
        for path in results.file_match_counts.keys() {
            if color {
                let _ = writeln!(out, "\x1b[35m{path}\x1b[0m");
            } else {
                out.push_str(path);
                out.push('\n');
            }
        }
    } else if args.count {
        let color = global.should_color();
        for (path, count) in &results.file_match_counts {
            if color {
                let _ = writeln!(out, "\x1b[35m{path}\x1b[0m:{count}");
            } else {
                let _ = writeln!(out, "{path}:{count}");
            }
        }
    } else {
        let color = global.should_color();
        let (path_c, sep_c, ln_c, reset) = if color {
            ("\x1b[35m", "\x1b[36m", "\x1b[32m", "\x1b[0m")
        } else {
            ("", "", "", "")
        };
        let has_ctx =
            args.context.is_some() || args.before_context.is_some() || args.after_context.is_some();
        let mut prev_end: Option<(usize, usize)> = None; // (match index, last line covered)
        let mut emitted_up_to: usize = 0; // last 1-based line printed for current file
        for (mi, m) in results.matches.iter().enumerate() {
            if has_ctx {
                // Only print separator between non-contiguous groups,
                // matching standard grep/ripgrep behavior (#1111).
                let cur_start = m
                    .line
                    .saturating_sub(m.context_before.as_ref().map_or(0, |b| b.len()));
                if let Some((prev_idx, prev_last_line)) = prev_end {
                    let prev_path = &results.matches[prev_idx].path;
                    if **prev_path != *m.path {
                        emitted_up_to = 0;
                        out.push_str("--\n");
                    } else if cur_start > prev_last_line + 1 {
                        out.push_str("--\n");
                    }
                }
                let after_ctx_len = m.context_after.as_ref().map_or(0, |a| a.len());
                prev_end = Some((mi, m.line + after_ctx_len));
            }
            if let Some(ref before) = m.context_before {
                for (j, ctx) in before.iter().enumerate() {
                    let ln = m.line - before.len() + j;
                    // Skip lines already emitted by the previous match's
                    // context-after window to avoid duplicates (#1160).
                    if has_ctx && ln <= emitted_up_to {
                        continue;
                    }
                    let _ = writeln!(
                        out,
                        "{path_c}{}{reset}{sep_c}-{reset}{ln_c}{ln}{reset}{sep_c}-{reset}{ctx}",
                        m.path
                    );
                }
            }
            let _ = writeln!(
                out,
                "{path_c}{}{reset}{sep_c}:{reset}{ln_c}{}{reset}{sep_c}:{reset}{}{sep_c}:{reset}{}",
                m.path, m.line, m.column, m.text
            );
            if has_ctx {
                emitted_up_to = m.line;
            }
            if let Some(ref after) = m.context_after {
                for (j, ctx) in after.iter().enumerate() {
                    let ln = m.line + 1 + j;
                    // Skip context-after lines that will be printed as a
                    // subsequent match line to avoid duplicates (#1182).
                    let is_future_match = results.matches[mi + 1..]
                        .iter()
                        .take_while(|next| *next.path == *m.path)
                        .any(|next| next.line == ln);
                    if is_future_match {
                        continue;
                    }
                    let _ = writeln!(
                        out,
                        "{path_c}{}{reset}{sep_c}-{reset}{ln_c}{ln}{reset}{sep_c}-{reset}{ctx}",
                        m.path
                    );
                }
                if has_ctx {
                    let last_after_ln = m.line + after.len();
                    if last_after_ln > emitted_up_to {
                        emitted_up_to = last_after_ln;
                    }
                }
            }
        }
    }

    Ok(out)
}

pub fn run(args: SearchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    if args.pattern.is_empty() {
        let msg = "search pattern must not be empty";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(exit::FAILURE);
    }
    if args.invert_match && args.multiline {
        let msg = "--invert-match and --multiline cannot be combined";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(exit::FAILURE);
    }

    let cwd = global.resolve_cwd()?;
    // Read --files-from once (including stdin `-`). Re-reading stdin is empty
    // and would skip sole-binary / refused honesty and empty-scan masks.
    let files_from_list = global.read_files_from()?;
    if let Some(err) = crate::ops::file::sole_explicit_non_text_for_scan(
        &args.paths,
        files_from_list.as_deref(),
        &cwd,
    ) {
        {
            let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
            let msg = crate::exit::agent_error_message(&err);
            global.emit_error_json_kind(Some(kind), &msg)?;
        }
        return Ok(exit::FAILURE);
    }
    let results = collect_matches_with_list(&args, global, files_from_list.as_deref())?;
    let skipped = if files_from_list.is_some() {
        // Use preloaded list (stdin cannot be re-read).
        Ok(files_from_list
            .as_ref()
            .and_then(|files| crate::files::explicit_paths_missing_entries(&cwd, files)))
    } else {
        crate::files::scan_missing_entries(global, &cwd, &args.paths)
    }?;
    let refused =
        explicit_binary_refused_with_list(&args, global, &cwd, files_from_list.as_deref());

    // --assert-count mode: succeed only if total count equals N.
    // JSON matches MCP search_files: ok == matched; mismatch sets
    // error_kind: changes_detected (agent-rules / tx parity, exit 2).
    if let Some(expected) = args.assert_count {
        let actual: usize = results.file_match_counts.values().sum();
        // actual==0 with unreadable paths must not count as a clean match
        // (assert-count 0 would otherwise pass while the scan was masked).
        if actual == 0 {
            // All targets missing must fail closed (same as normal no-match path).
            // assert-count 0 on a typo path must not report success / "no TODOs".
            let all_missing = if let Some(ref files) = files_from_list {
                crate::files::all_explicit_paths_missing(files, Some(&cwd))
            } else {
                crate::files::all_scan_targets_missing(global, &args.paths, Some(&cwd))?
            };
            if all_missing {
                let msg = format!(
                    "no such file or directory: {}",
                    global.path_scope_description(&args.paths)
                );
                global.emit_error_json_kind(Some("not_found"), &msg)?;
                return Ok(exit::FAILURE);
            }
            if let Some(err) = crate::ops::file::sole_explicit_non_text_for_scan(
                &args.paths,
                files_from_list.as_deref(),
                &cwd,
            ) {
                {
                    let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
                    let msg = crate::exit::agent_error_message(&err);
                    global.emit_error_json_kind(Some(kind), &msg)?;
                }
                return Ok(exit::FAILURE);
            }
            let scanned = crate::files::collect_file_paths_opts_with_list(
                &args.paths,
                global,
                false,
                Some(&cwd),
                files_from_list.as_deref(),
                None,
            )?;
            if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&scanned, &cwd) {
                global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
                return Ok(exit::FAILURE);
            }
        }
        let matches = actual == expected;
        let status = if matches {
            "success"
        } else {
            "changes_detected"
        };
        let code = if matches {
            exit::SUCCESS
        } else {
            exit::CHANGES_DETECTED
        };
        if global.emit_json(&SearchAssertCountOutput {
            ok: matches,
            status,
            assert_count: SearchAssertCount {
                expected,
                actual,
                matched: matches,
            },
            error_kind: if matches {
                None
            } else {
                Some("changes_detected")
            },
        })? {
            return Ok(code);
        }
        if !global.quiet {
            if matches {
                eprintln!("assert-count: {actual} matches (expected {expected})");
            } else {
                eprintln!("assert-count: expected {expected} matches, found {actual}");
            }
        }
        return Ok(code);
    }

    let has_matches = if args.file_inventory_mode() {
        !results.file_match_counts.is_empty()
    } else {
        !results.matches.is_empty()
    };
    if !has_matches {
        let cwd = global.resolve_cwd()?;
        let all_missing = if let Some(ref files) = files_from_list {
            crate::files::all_explicit_paths_missing(files, Some(&cwd))
        } else {
            crate::files::all_scan_targets_missing(global, &args.paths, Some(&cwd))?
        };
        if all_missing {
            let msg = format!(
                "no such file or directory: {}",
                global.path_scope_description(&args.paths)
            );
            global.emit_error_json_kind(Some("not_found"), &msg)?;
            return Ok(exit::FAILURE);
        }
        // Sole explicit non-text soft-skipped by the reader; do not report
        // no_matches (agents retry forever). Includes sole --files-from.
        if let Some(err) = crate::ops::file::sole_explicit_non_text_for_scan(
            &args.paths,
            files_from_list.as_deref(),
            &cwd,
        ) {
            {
                let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
                let msg = crate::exit::agent_error_message(&err);
                global.emit_error_json_kind(Some(kind), &msg)?;
            }
            return Ok(exit::FAILURE);
        }
        // Multi-path / dir walk: unreadable may have masked the scan (#1894).
        let scanned = crate::files::collect_file_paths_opts_with_list(
            &args.paths,
            global,
            false,
            Some(&cwd),
            files_from_list.as_deref(),
            None,
        )?;
        if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&scanned, &cwd) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
        let path_desc = global.path_scope_description(&args.paths);
        let payload = SearchOutput {
            ok: false,
            match_count: 0,
            file_count: 0,
            matches: vec![],
            files: vec![],
            truncated: false,
            error_kind: Some("no_matches"),
            error: Some(format!(
                "no matches for '{}' in {path_desc}",
                crate::fallback::truncate_str(&args.pattern, 60)
            )),
            skipped: skipped.clone(),
            refused: refused.clone(),
        };
        global.emit_json(&payload)?;
        if !global.quiet && !global.json && !global.jsonl {
            eprintln!("no matches for '{}' in {path_desc}", args.pattern);
            if args.literal && crate::files::has_regex_metacharacters(&args.pattern) {
                eprintln!(
                    "hint: --literal is set but the pattern looks like a regex, try removing --literal"
                );
            }
            if !args.case_insensitive {
                eprintln!("hint: try -i for case-insensitive matching");
            }
        }
        return Ok(exit::NO_MATCHES);
    }

    if global.json || global.jsonl || !global.quiet {
        let output = format_results(results, &args, global, skipped, refused)?;
        print!("{output}");
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.txt"),
            "Hello, world!\nThis is a test file.\nHello again!\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("code.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        dir
    }

    fn make_args(pattern: &str, paths: Vec<String>) -> SearchArgs {
        SearchArgs {
            pattern: pattern.to_string(),
            paths,
            literal: false,
            regex: false,
            context: None,
            before_context: None,
            after_context: None,
            files_with_matches: false,
            files_without_match: false,
            count: false,
            invert_match: false,
            multiline: false,
            case_insensitive: false,
            assert_count: None,
            max_results: 0,
        }
    }

    #[test]
    fn empty_pattern_rejected() {
        let dir = make_test_dir();
        let args = make_args("", vec![dir.path().to_string_lossy().into_owned()]);
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn max_results_limits_detailed_matches_but_not_counts() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.max_results = 1;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        // detailed matches capped
        assert_eq!(results.matches.len(), 1);
        // file_match_counts reflect the pre-limit collection (fixture has 2 "Hello" matches total)
        let total: usize = results.file_match_counts.values().sum();
        assert_eq!(
            total, 2,
            "counts should be full (2), not capped by max_results"
        );
    }

    #[test]
    fn max_results_with_context() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.context = Some(1);
        args.max_results = 1;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 1);
        // context fields may be present depending on impl
    }

    #[test]
    fn max_results_does_not_affect_assert_count() {
        // Per design: assert_count uses full file_match_counts even when max_results caps detailed matches.
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.max_results = 1;
        args.assert_count = Some(2); // fixture has 2 "Hello" matches
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(
            code,
            exit::SUCCESS,
            "assert should pass on full count despite max=1"
        );
    }

    #[test]
    fn literal_match_finds_expected() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.literal = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 2);
        assert!(results.matches.iter().all(|m| m.text.contains("Hello")));
    }

    #[test]
    fn regex_match_works() {
        let dir = make_test_dir();
        let args = make_args(r"Hello.*!", vec![dir.path().to_string_lossy().into_owned()]);
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 2);
        for m in &results.matches {
            assert!(m.text.contains("Hello"));
            assert!(m.text.ends_with('!'), "regex match should end with '!'");
        }
    }

    #[test]
    fn no_matches_returns_exit_code_3() {
        let dir = make_test_dir();
        let args = make_args(
            "zzz_no_match_zzz",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn files_with_matches_output() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_with_matches = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        let output =
            format_results(results, &args, &GlobalFlags::test_default(), None, None).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("hello.txt"));
        // Pure paths – no colon-separated match fields after the filename
        assert!(
            lines[0].ends_with("hello.txt"),
            "expected bare path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn count_output() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        let output =
            format_results(results, &args, &GlobalFlags::test_default(), None, None).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(":2"));
        assert!(lines[0].contains("hello.txt:"));
    }

    #[test]
    fn line_and_column_for_offset_maps_multiline_positions() {
        let newline_offsets = vec![2, 5];
        assert_eq!(
            ops_search::line_and_column_for_offset(&newline_offsets, 0),
            (1, 1)
        );
        assert_eq!(
            ops_search::line_and_column_for_offset(&newline_offsets, 2),
            (1, 3)
        );
        assert_eq!(
            ops_search::line_and_column_for_offset(&newline_offsets, 3),
            (2, 1)
        );
        assert_eq!(
            ops_search::line_and_column_for_offset(&newline_offsets, 6),
            (3, 1)
        );
    }

    #[test]
    fn count_only_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        // count_only optimization: matches vec should be empty
        assert!(
            results.matches.is_empty(),
            "count mode should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 2);
    }

    #[test]
    fn files_with_matches_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_with_matches = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            results.matches.is_empty(),
            "files_with_matches should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.len(), 1);
    }

    #[test]
    fn files_without_match_output() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hit.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("miss.txt"), "nothing here\n").unwrap();
        let mut args = make_args("needle", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_without_match = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        let output =
            format_results(results, &args, &GlobalFlags::test_default(), None, None).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].ends_with("miss.txt"),
            "expected miss.txt only, got: {}",
            lines[0]
        );
        assert!(!output.contains("hit.txt"));
    }

    #[test]
    fn files_without_match_skips_match_construction() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hit.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("miss.txt"), "nothing here\n").unwrap();
        let mut args = make_args("needle", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_without_match = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            results.matches.is_empty(),
            "files_without_match should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.len(), 1);
        let path = results.file_match_counts.keys().next().unwrap();
        assert!(path.ends_with("miss.txt"), "expected miss.txt, got: {path}");
        assert_eq!(results.file_match_counts[path], 0);
    }

    #[test]
    fn files_without_match_all_hits_returns_exit_code_3() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hit.txt"), "needle\n").unwrap();
        let mut args = make_args("needle", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_without_match = true;
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn multiline_count_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args(
            r"fn main\(\).*\}",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        args.multiline = true;
        args.count = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            results.matches.is_empty(),
            "multiline count should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 1);
    }

    #[test]
    fn multiline_files_with_matches_stops_after_first_match() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("multi.txt"), "foo\nbar\nfoo\n").unwrap();

        let mut args = make_args("foo", vec![dir.path().to_string_lossy().into_owned()]);
        args.multiline = true;
        args.files_with_matches = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            results.matches.is_empty(),
            "multiline files-with-matches should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 1);
    }

    #[test]
    fn multiline_files_with_matches_with_assert_count_counts_all_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("multi.txt"), "foo\nbar\nfoo\n").unwrap();

        let mut args = make_args("foo", vec![dir.path().to_string_lossy().into_owned()]);
        args.multiline = true;
        args.files_with_matches = true;
        args.assert_count = Some(2);
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(
            results.matches.is_empty(),
            "multiline files-with-matches assert-count should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 2);
    }

    #[test]
    fn invert_match_excludes_matching_lines() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        // hello.txt has 3 lines, 2 match "Hello", so 1 non-matching line.
        // code.rs has 3 lines, 0 match "Hello" (case-sensitive), so 3 non-matching.
        // Total: 4 non-matching lines.
        assert_eq!(results.matches.len(), 4);
        for m in &results.matches {
            assert!(
                !m.text.contains("Hello"),
                "inverted match should not contain 'Hello': {}",
                m.text
            );
        }
    }

    #[test]
    fn multiline_match_spans_lines() {
        let dir = make_test_dir();
        let mut args = make_args(
            r"fn main\(\).*\}",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        args.multiline = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 1);
        let m = &results.matches[0];
        assert!(
            m.path.ends_with("code.rs"),
            "should match code.rs: {}",
            m.path
        );
        assert!(m.text.contains("fn main()"), "text: {}", m.text);
        assert!(
            m.text.contains('}'),
            "text should span to closing brace: {}",
            m.text
        );
        assert_eq!(m.line, 1, "match starts on line 1");
        assert_eq!(m.column, 1, "match starts at column 1");
    }

    #[test]
    fn invert_match_multiline_is_rejected() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.multiline = true;
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn context_lines_included_in_output() {
        let dir = make_test_dir();
        let mut args = make_args("test file", vec![dir.path().to_string_lossy().into_owned()]);
        args.context = Some(1);
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 1);
        let m = &results.matches[0];
        let before = m
            .context_before
            .as_ref()
            .expect("should have context_before");
        assert_eq!(before.len(), 1);
        assert!(
            before[0].contains("Hello, world!"),
            "context before: {:?}",
            before
        );
        let after = m.context_after.as_ref().expect("should have context_after");
        assert_eq!(after.len(), 1);
        assert!(
            after[0].contains("Hello again!"),
            "context after: {:?}",
            after
        );
    }

    #[test]
    fn json_output_values() {
        let dir = make_test_dir();
        let args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        let mut global = GlobalFlags::test_default();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(results, &args, &global, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        // "Hello" appears in two lines of hello.txt, one file.
        assert_eq!(v["match_count"], serde_json::json!(2));
        assert_eq!(v["file_count"], serde_json::json!(1));
        let matches = v["matches"].as_array().expect("matches is array");
        assert_eq!(matches.len(), 2);
        // Matches are ordered by line within the file.
        for m in matches {
            let path = m["path"].as_str().unwrap();
            assert!(path.ends_with("hello.txt"), "unexpected path: {path}");
            let text = m["text"].as_str().unwrap();
            assert!(text.contains("Hello"), "match text missing 'Hello': {text}");
        }
        // "Hello" appears at column 1 on lines 1 ("Hello, world!") and 3 ("Hello again!").
        assert_eq!(matches[0]["line"].as_u64().unwrap(), 1, "first match line");
        assert_eq!(
            matches[0]["column"].as_u64().unwrap(),
            1,
            "first match column"
        );
        assert_eq!(matches[1]["line"].as_u64().unwrap(), 3, "second match line");
        assert_eq!(
            matches[1]["column"].as_u64().unwrap(),
            1,
            "second match column"
        );
    }

    #[test]
    fn json_files_with_matches_lists_file_paths() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_with_matches = true;
        let mut global = GlobalFlags::test_default();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(results, &args, &global, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        assert_eq!(v["file_count"], serde_json::json!(1));
        // matches is empty in files-with-matches mode (by design).
        assert_eq!(v["matches"].as_array().unwrap().len(), 0);
        // files array must contain the matching file paths.
        let files = v["files"].as_array().expect("files array must be present");
        assert_eq!(files.len(), 1);
        assert!(files[0]["path"].as_str().unwrap().ends_with("hello.txt"));
    }

    #[test]
    fn json_files_without_match_lists_file_paths() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hit.txt"), "needle\n").unwrap();
        fs::write(dir.path().join("miss.txt"), "nothing here\n").unwrap();
        let mut args = make_args("needle", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_without_match = true;
        let mut global = GlobalFlags::test_default();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(results, &args, &global, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        assert_eq!(v["file_count"], serde_json::json!(1));
        assert_eq!(v["match_count"], serde_json::json!(0));
        assert_eq!(v["matches"].as_array().unwrap().len(), 0);
        let files = v["files"].as_array().expect("files array must be present");
        assert_eq!(files.len(), 1);
        assert!(files[0]["path"].as_str().unwrap().ends_with("miss.txt"));
        assert!(files[0].get("count").is_none());
    }

    #[test]
    fn json_count_lists_file_counts() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let mut global = GlobalFlags::test_default();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(results, &args, &global, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        // files array must contain the per-file counts.
        let files = v["files"].as_array().expect("files array must be present");
        assert_eq!(files.len(), 1);
        assert!(files[0]["path"].as_str().unwrap().ends_with("hello.txt"));
        assert_eq!(files[0]["count"], serde_json::json!(2));
    }

    #[test]
    fn case_insensitive_finds_all_cases() {
        let dir = make_test_dir();
        // hello.txt has "Hello" (uppercase H); code.rs has "hello" (lowercase).
        let mut args = make_args("hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.case_insensitive = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        // hello.txt: 2 lines with "Hello", code.rs: 1 line with "hello" => 3
        assert_eq!(results.matches.len(), 3);
    }

    #[test]
    fn case_insensitive_literal_escapes_regex_chars() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("special.txt"), "foo(BAR)\nbaz\n").unwrap();

        let mut args = make_args("foo(bar)", vec![dir.path().to_string_lossy().into_owned()]);
        args.literal = true;
        args.case_insensitive = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].text.contains("foo(BAR)"));
    }

    #[test]
    fn invert_match_count_counts_non_matching_lines() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.count = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        // count mode should not build SearchMatch objects
        assert!(results.matches.is_empty());
        // hello.txt: 3 lines, 2 match "Hello", so 1 non-matching.
        // code.rs: 3 lines, 0 match, so 3 non-matching.
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 4);
    }

    #[test]
    fn invert_match_files_with_matches_lists_files_with_non_matching_lines() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.files_with_matches = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(results.matches.is_empty());
        // Both files have non-matching lines.
        assert_eq!(results.file_match_counts.len(), 2);
    }

    #[test]
    fn invert_match_files_without_match_lists_files_with_no_non_matching_lines() {
        // -L is file-level: files with zero inverted hits (every line matches).
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("all_hit.txt"), "Hello\nHello again\n").unwrap();
        fs::write(dir.path().join("mixed.txt"), "Hello\nkeep\n").unwrap();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.files_without_match = true;
        let results = collect_matches(&args, &GlobalFlags::test_default()).unwrap();
        assert!(results.matches.is_empty());
        assert_eq!(results.file_match_counts.len(), 1);
        let path = results.file_match_counts.keys().next().unwrap();
        assert!(
            path.ends_with("all_hit.txt"),
            "expected all_hit.txt, got: {path}"
        );
    }

    #[test]
    fn invert_match_assert_count_exact() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.assert_count = Some(4); // 1 from hello.txt + 3 from code.rs
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn invert_match_assert_count_mismatch() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.assert_count = Some(99);
        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    /// Overlapping context windows must not produce duplicate lines (#1160).
    #[test]
    fn context_no_duplicate_lines() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("close.txt"),
            "line1\nmatch_a\nline3\nmatch_b\nline5\n",
        )
        .unwrap();
        let mut args = make_args("match", vec![dir.path().to_string_lossy().into_owned()]);
        args.context = Some(1);
        let global = GlobalFlags::test_default();
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    /// Verify plain-text context output does not repeat lines between
    /// close matches (regression for #1160).
    #[test]
    fn context_plain_text_no_repeated_lines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("close.txt");
        fs::write(&file, "line1\nmatch_a\nline3\nmatch_b\nline5\n").unwrap();
        let mut args = make_args("match", vec![file.to_string_lossy().into_owned()]);
        args.context = Some(1);
        let global = GlobalFlags::test_default();
        let output = format_results(
            collect_matches(&args, &global).unwrap(),
            &args,
            &global,
            None,
            None,
        )
        .unwrap();
        // "line3" should appear exactly once, not twice.
        let count = output.matches("line3").count();
        assert_eq!(count, 1, "line3 should appear once, got: {output}");
    }

    #[test]
    fn context_after_not_duplicated_for_adjacent_matches() {
        // #1182: When two matches are on adjacent lines, the context-after
        // of match A includes match B's line. That line must NOT appear
        // twice (once as context, once as match).
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("adj.txt");
        fs::write(&file, "line1\nmatch_a\nmatch_b\nline4\n").unwrap();
        let mut args = make_args("match", vec![file.to_string_lossy().into_owned()]);
        args.context = Some(1);
        let global = GlobalFlags::test_default();
        let output = format_results(
            collect_matches(&args, &global).unwrap(),
            &args,
            &global,
            None,
            None,
        )
        .unwrap();
        // "match_b" should appear exactly once (as a match line).
        let count = output.matches("match_b").count();
        assert_eq!(
            count, 1,
            "match_b should appear once, got {count}: {output}"
        );
    }
}
