//! size-waiver: accepted single-domain bulk (policy #1408). CLI replace scan,
//! context/fuzzy routing, and agent JSON honesty for matches/refuses is one unit.

use crate::cli::global::GlobalFlags;
use crate::diff::render_diffs_colored;
use crate::exit;
use crate::ops::replace::{
    compile_replace_regex, count_nth_candidates, replace_content, replace_whole_lines,
    replacement_text,
};
use crate::tx::engine::WriteSource;
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
    // ref:replace-mode:require-change
    /// Fail when the pattern matches zero times (same as default CLI no-match
    /// exit, but explicit for plan/MCP parity and agents). Softened by
    /// `--if-exists`.
    #[arg(long)]
    pub require_change: bool,
    // ref:replace-mode:command-position
    /// Only replace tokens in shell command position (invocable names after
    /// `sudo`/`timeout`/`chpst`/`envdir`/etc.; not arguments like `uv pip` or
    /// longer words like `pipenv`). Literal only; cannot combine with
    /// regex/word-boundary/multiline/nth/insert/context.
    #[arg(long)]
    pub command_position: bool,
    // ref:replace-mode:fuzzy
    /// When exact match fails, try fuzzy/similarity fallback (and when
    /// `--before-context` / `--after-context` is set). Literal only.
    /// Default is fail-closed (#1758): without `--allow-absent-old`, reports
    /// the best candidate (`matched_text` / `refused[]`) and does not write.
    /// With `--allow-absent-old`, always check JSON `matched_text`: a high
    /// score can still rewrite a different live span than `--old` (#1736).
    #[arg(long)]
    pub fuzzy: bool,
    // ref:replace-mode:min-fuzzy-score
    /// Reject fuzzy matches below this score (0.0..=1.0). Exact and anchored
    /// matches are unaffected. Does not prove the matched span is the intended
    /// target; inspect JSON `matched_text`. Plan/MCP field name: `min_fuzzy_score`
    /// (#1687, #1736).
    #[arg(long, value_name = "SCORE")]
    pub min_fuzzy_score: Option<f64>,
    // ref:replace-mode:allow-absent-old
    /// Allow fuzzy/similarity to rewrite a nearby live span when exact `--old`
    /// is not in the file. Default is fail-closed (#1758): report the best
    /// candidate without writing. Anchored context matches still apply.
    /// Plan/MCP field: `allow_absent_old`.
    #[arg(long)]
    pub allow_absent_old: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Clone, Serialize)]
struct ReplaceFileResult {
    path: String,
    match_count: usize,
    /// How the match was resolved (`exact` / `fuzzy` / `anchored`). #1669
    #[serde(skip_serializing_if = "Option::is_none")]
    match_mode: Option<&'static str>,
    /// Similarity score when match_mode is fuzzy.
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    /// Text actually matched for fuzzy/anchored (may differ from `--old`). #1736
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReplaceOutput {
    ok: bool,
    match_count: usize,
    file_count: usize,
    files: Vec<ReplaceFileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    /// Set when the pattern matched but every replacement was identical (no writes).
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<bool>,
    /// Machine-readable failure kind for agents (`no_matches`, `ambiguous`),
    /// aligned with tx JSON `error_kind`. Omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    /// Human/agent diagnostic on failure (floor reject, did-you-mean prose).
    /// Mirrors tx JSON `error` for soft no-matches (#1754).
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Aggregate match strategy when all files share one mode (#1669).
    #[serde(skip_serializing_if = "Option::is_none")]
    match_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    /// First-file fuzzy/anchored matched span when `file_count == 1` (#1736).
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_text: Option<String>,
    /// Did-you-mean candidates on soft no-match (literal patterns only).
    /// Agents should prefer this over scraping the English error string.
    #[serde(skip_serializing_if = "Option::is_none")]
    similar_targets: Option<Vec<String>>,
    /// Paths from `--files-from` that were missing or unreadable and soft-skipped.
    /// Present under `--json` even when `--quiet` suppresses stderr (#1756).
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<Vec<String>>,
    /// Files where fuzzy/similarity found a candidate but did not write
    /// (exact old absent without `allow_absent_old`). Present on partial multi-file
    /// success so agents do not treat overall ok as full coverage.
    #[serde(skip_serializing_if = "Option::is_none")]
    refused: Option<Vec<RefuseFileResult>>,
}

/// Per-file soft refuse honesty (fuzzy fail-closed without a write).
#[derive(Debug, Clone, Serialize)]
struct RefuseFileResult {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_text: Option<String>,
    /// Machine-readable reason (`exact_old_absent`).
    reason: &'static str,
}

/// Result of processing a single file.
struct FileReplacement {
    path: String,
    /// Relative path for display in diffs and JSON output.
    display_path: String,
    original: String,
    replaced: String,
    match_count: usize,
    match_mode: Option<&'static str>,
    match_score: Option<f64>,
    matched_text: Option<String>,
}

fn match_mode_str(mode: crate::api::MatchMode) -> &'static str {
    crate::tx::match_mode_label(mode)
}

/// Best-effort "did you mean?" candidates for a soft no-match (literal only).
///
/// Samples up to a few readable files under the replace path args so agents
/// can branch on structured `similar_targets` without scraping English.
fn similar_targets_for_no_match(
    args: &ReplaceArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
) -> Option<Vec<String>> {
    if args.regex {
        return None;
    }
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for p in &args.paths {
        let abs = cwd.join(p);
        if abs.is_file() {
            files.push(abs);
        } else if abs.is_dir()
            && let Ok(mut walked) = crate::files::collect_file_paths_opts(
                std::slice::from_ref(p),
                global,
                false,
                Some(cwd),
            )
        {
            walked.truncate(5);
            files.extend(walked);
        }
    }
    files.truncate(5);
    for f in files {
        let Ok(content) = std::fs::read_to_string(&f) else {
            continue;
        };
        let similar = crate::fallback::find_similar_targets(&content, &args.old, 3);
        if !similar.is_empty() {
            return Some(similar);
        }
    }
    None
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

/// Walk files and collect replacements using parallel file processing.
///
/// Includes identity matches (`new == old`) so callers can enforce `--unique`
/// and distinguish "pattern found" from "pattern absent" before filtering.
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
    let command_position = args.command_position;
    let to = args.new.as_deref().unwrap_or("");
    let range = parse_range_arg(args.range.as_deref())?;

    let cwd_ref = &cwd;
    let mut replacements: Vec<FileReplacement> =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let content = crate::files::read_text_file_logged(path, "replace", quiet)?;
            let (replaced, count) = if command_position {
                let (out, n) =
                    crate::ops::shell_token::replace_command_position(&content, from, to);
                (std::borrow::Cow::Owned(out), n)
            } else if whole_line {
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
                    match_mode: Some("exact"),
                    match_score: None,
                    matched_text: None,
                })
            } else {
                None
            }
        });

    replacements.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    // Caller runs unique against this list (including identity matches), then
    // filters identity so writes/diffs only cover real content changes.
    Ok(replacements)
}

fn make_file_results(replacements: &[FileReplacement]) -> Vec<ReplaceFileResult> {
    replacements
        .iter()
        .map(|r| ReplaceFileResult {
            path: r.display_path.clone(),
            match_count: r.match_count,
            match_mode: r.match_mode,
            match_score: r.match_score,
            matched_text: r.matched_text.clone(),
        })
        .collect()
}

/// Files with recorded match meta but no write (fuzzy refuse / floor skip).
fn collect_refused_files(
    cwd: &std::path::Path,
    changes: &[(std::path::PathBuf, String, String)],
    meta: &std::collections::HashMap<std::path::PathBuf, crate::tx::ReplaceMatchMeta>,
) -> Option<Vec<RefuseFileResult>> {
    let mut out = Vec::new();
    for (path, m) in meta {
        if changes.iter().any(|(c, _, _)| c == path) {
            continue;
        }
        // Only surface candidates that were found but not applied (count 0).
        if m.match_count != 0 || m.matched_text.is_none() {
            continue;
        }
        let reason = m
            .refuse_reason
            .unwrap_or(if m.mode == crate::api::MatchMode::Fuzzy {
                "exact_old_absent"
            } else {
                "no_write"
            });
        out.push(RefuseFileResult {
            path: crate::files::relative_display(path, cwd)
                .to_string_lossy()
                .into_owned(),
            match_mode: Some(match_mode_str(m.mode)),
            match_score: m.score,
            matched_text: m.matched_text.clone(),
            reason,
        });
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Candidate honesty from soft refuses (no write): fuzzy fail-closed (#1758)
/// or floor reject with recorded meta. Single-path meta surfaces mode/score/span.
fn aggregate_refuse_match_meta(
    meta: &std::collections::HashMap<std::path::PathBuf, crate::tx::ReplaceMatchMeta>,
) -> (Option<&'static str>, Option<f64>, Option<String>) {
    use crate::api::{MatchMode, merge_match_modes};

    if meta.is_empty() {
        return (None, None, None);
    }
    let mut agg: Option<MatchMode> = None;
    let mut score: Option<f64> = None;
    let mut matched: Option<String> = None;
    for m in meta.values() {
        agg = Some(merge_match_modes(agg, m.mode));
        if m.mode == MatchMode::Fuzzy
            && let Some(s) = m.score
        {
            score = Some(score.map_or(s, |prev| prev.min(s)));
        }
        if matched.is_none() {
            matched = m.matched_text.clone();
        }
    }
    let mode = match agg {
        Some(MatchMode::Fuzzy) => Some("fuzzy"),
        Some(MatchMode::Anchored) => Some("anchored"),
        Some(MatchMode::Exact) => Some("exact"),
        None => None,
    };
    let score = if matches!(agg, Some(MatchMode::Fuzzy)) {
        score
    } else {
        None
    };
    let matched = if meta.len() == 1 { matched } else { None };
    (mode, score, matched)
}

/// Top-level match honesty for multi-file CLI replace (#1669 / #1674).
///
/// Uses the same worst-case rollup as plan/tx and content_edits
/// (`fuzzy` > `anchored` > `exact`) so agents see a single conservative
/// confidence when files disagree.
fn aggregate_match_meta(files: &[ReplaceFileResult]) -> (Option<&'static str>, Option<f64>) {
    use crate::api::{MatchMode, merge_match_modes};

    let mut agg: Option<MatchMode> = None;
    let mut score: Option<f64> = None;
    for f in files {
        let Some(label) = f.match_mode else {
            continue;
        };
        let mode = match label {
            "fuzzy" => MatchMode::Fuzzy,
            "anchored" => MatchMode::Anchored,
            "exact" => MatchMode::Exact,
            _ => continue,
        };
        agg = Some(merge_match_modes(agg, mode));
        // Worst-case confidence: lowest fuzzy score across files (parity with
        // plan/tx aggregate and content_edits).
        if mode == MatchMode::Fuzzy
            && let Some(s) = f.match_score
        {
            score = Some(score.map_or(s, |prev| prev.min(s)));
        }
    }
    match agg {
        Some(MatchMode::Fuzzy) => (Some("fuzzy"), score),
        Some(MatchMode::Anchored) => (Some("anchored"), None),
        Some(MatchMode::Exact) => (Some("exact"), None),
        None => (None, None),
    }
}

pub fn run(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "replace: old={:?} regex={} paths={:?}",
        args.old,
        args.regex,
        args.paths
    );
    if args.command_position
        && let Some(msg) = crate::ops::shell_token::command_position_combo_error(
            crate::ops::shell_token::CommandPositionIncompat {
                regex: args.regex,
                case_insensitive: args.case_insensitive,
                word_boundary: args.word_boundary,
                whole_line: args.whole_line,
                multiline: args.multiline,
                nth: args.nth.is_some(),
                insert_before: args.insert_before.is_some(),
                insert_after: args.insert_after.is_some(),
                before_context: args.before_context.is_some(),
                after_context: args.after_context.is_some(),
                fuzzy: args.fuzzy,
            },
        )
    {
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(exit::FAILURE);
    }
    use crate::ops::replace::{ReplaceValidationParams, validate_replace_args};
    if let Err(e) = validate_replace_args(&ReplaceValidationParams {
        pattern: &args.old,
        has_to: args.new.is_some(),
        has_insert_before: args.insert_before.is_some(),
        has_insert_after: args.insert_after.is_some(),
        nth: args.nth,
        whole_line: args.whole_line,
        multiline: args.multiline,
        has_range: args.range.is_some(),
    }) {
        global.emit_error_json_kind(Some("invalid_input"), &e.to_string())?;
        return Ok(exit::FAILURE);
    }

    let cwd = global.resolve_cwd()?;
    // Fail closed before the parallel scan so --contain does not read
    // outside the workspace while computing matches (precomputed write-path
    // guard remains defense-in-depth).
    global.check_paths_contained(&cwd, &args.paths)?;
    let skipped = crate::files::files_from_missing_entries(global, &cwd)?;

    // Context / pure fuzzy: route through the tx engine where the
    // context_filtered_offset and fallback chain live (#1668).
    if args.fuzzy || args.before_context.is_some() || args.after_context.is_some() {
        return run_context_replace(args, global, &cwd, skipped);
    }

    // Phase 1: Parallel file scan to identify files with matches (includes identity).
    let mut replacements = collect_replacements(&args, global)?;
    let raw_match_count: usize = replacements.iter().map(|r| r.match_count).sum();

    // Unique check: fail if any file has more than one match (before identity
    // filter so `--unique` is honest when new == old).
    if args.unique {
        for r in &replacements {
            if r.match_count > 1 {
                let output = ReplaceOutput {
                    ok: false,
                    match_count: r.match_count,
                    file_count: 1,
                    files: vec![ReplaceFileResult {
                        path: r.display_path.clone(),
                        match_count: r.match_count,
                        match_mode: r.match_mode,
                        match_score: r.match_score,
                        matched_text: r.matched_text.clone(),
                    }],
                    diff: None,
                    identity: None,
                    error_kind: Some("ambiguous"),
                    error: None,
                    match_mode: None,
                    match_score: None,
                    matched_text: None,
                    similar_targets: None,
                    skipped: skipped.clone(),
                    refused: None,
                };
                global.emit_json(&output)?;
                if !global.quiet {
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

    // Drop files where the replacement produces identical content (e.g.
    // replacing "X" with "X"). Match count was non-zero, but there is no
    // actual change to write, diff, or report.
    replacements.retain(|r| r.original != r.replaced);

    if replacements.is_empty() {
        // Identity-only matches (pattern found, new == old): not a soft no-match.
        // require_change is satisfied; exit success with the raw match count.
        if raw_match_count > 0 {
            let output = ReplaceOutput {
                ok: true,
                match_count: raw_match_count,
                file_count: 0,
                files: vec![],
                diff: None,
                identity: Some(true),
                error_kind: None,
                error: None,
                match_mode: Some("exact"),
                match_score: None,
                matched_text: None,
                similar_targets: None,
                skipped: skipped.clone(),
                refused: None,
            };
            global.emit_json(&output)?;
            if !global.quiet && !global.json && !global.jsonl {
                eprintln!(
                    "matched {raw_match_count} occurrence(s) of '{}' but replacement is identical (no file changes)",
                    args.old
                );
            }
            return Ok(exit::SUCCESS);
        }
        // --nth past the last match returns applied count 0 (looks like no-match).
        // Re-count so agents see "nth out of range" instead of "no matches for pattern".
        if let Some(n) = args.nth {
            let compiled_re = compile_replace_regex(
                &args.old,
                args.regex,
                args.case_insensitive,
                args.multiline,
                args.word_boundary,
            )?;
            let cwd = global.resolve_cwd()?;
            let file_paths =
                crate::collect_file_paths_opts(&args.paths, global, false, Some(&cwd))?;
            let range = parse_range_arg(args.range.as_deref())?;
            for path in &file_paths {
                let Ok(content) = std::fs::read_to_string(path) else {
                    continue;
                };
                let total = count_nth_candidates(
                    &content,
                    &args.old,
                    compiled_re.as_ref(),
                    args.whole_line,
                    range,
                );
                if total > 0 && n > total {
                    let display = crate::files::relative_display(path, &cwd)
                        .to_string_lossy()
                        .into_owned();
                    let msg = format!(
                        "nth {n} is out of range in {display}: pattern matches {total} time{}",
                        if total == 1 { "" } else { "s" }
                    );
                    global.emit_error_json_kind(Some("invalid_input"), &msg)?;
                    return Ok(exit::FAILURE);
                }
            }
        }
        if args.if_exists {
            let output = ReplaceOutput {
                ok: true,
                match_count: 0,
                file_count: 0,
                files: vec![],
                diff: None,
                identity: None,
                error_kind: None,
                error: None,
                match_mode: None,
                match_score: None,
                matched_text: None,
                similar_targets: None,
                skipped: skipped.clone(),
                refused: None,
            };
            global.emit_json(&output)?;
            return Ok(exit::SUCCESS);
        }
        if crate::files::all_scan_targets_missing(global, &args.paths, Some(&cwd))? {
            let msg = format!(
                "no such file or directory: {}",
                global.path_scope_description(&args.paths)
            );
            global.emit_error_json_kind(Some("not_found"), &msg)?;
            return Ok(exit::FAILURE);
        }
        let similar = similar_targets_for_no_match(&args, global, &cwd);
        let path_desc = global.path_scope_description(&args.paths);
        // Agents reading `error` (tx parity) should not need to scrape stderr.
        // Always set a message (even without similar_targets) so pure misses are
        // not error_kind-only empty bodies.
        let error_msg = match &similar {
            Some(s) if !s.is_empty() => format!(
                "no matches for '{}' (did you mean: {}?)",
                crate::fallback::truncate_str(&args.old, 60),
                s.join(", ")
            ),
            _ => format!(
                "no matches for '{}' in {path_desc}",
                crate::fallback::truncate_str(&args.old, 60)
            ),
        };
        let output = ReplaceOutput {
            ok: false,
            match_count: 0,
            file_count: 0,
            files: vec![],
            diff: None,
            identity: None,
            error_kind: Some("no_matches"),
            error: Some(error_msg),
            match_mode: None,
            match_score: None,
            matched_text: None,
            similar_targets: similar.clone(),
            skipped: skipped.clone(),
            refused: None,
        };
        global.emit_json(&output)?;
        if !global.quiet && !global.json && !global.jsonl {
            eprintln!("no matches for '{}' in {path_desc}", args.old);
            if let Some(ref s) = similar {
                eprintln!("did you mean: {}?", s.join(", "));
            }
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

    let (cwd, result) =
        crate::cmd::output::stage_for_write(WriteSource::Precomputed(precomputed), global)?;

    // Phase 3: Render output based on mode (check/apply/diff/confirm).
    // Custom JSON schema (match counts); mode/exit via write_mode helpers.
    replace_output(
        global,
        result,
        &files,
        total_matches,
        file_count,
        &cwd,
        skipped,
    )
}

/// Handle output rendering and commit/check/preview for replace via the engine.
///
/// Mode/exit owned by [`crate::cmd::write_mode::finalize_report`].
fn replace_output(
    global: &GlobalFlags,
    result: crate::tx::engine::ExecutionResult,
    files: &[ReplaceFileResult],
    total_matches: usize,
    file_count: usize,
    cwd: &std::path::Path,
    skipped: Option<Vec<String>>,
) -> anyhow::Result<u8> {
    use crate::cmd::write_mode::{FinalizeCallbacks, finalize_report};

    let refused = collect_refused_files(
        cwd,
        &result.exec_result.changes,
        &result.exec_result.replace_match_meta,
    );
    let (agg_mode, agg_score) = aggregate_match_meta(files);
    let build_output = |diff: Option<String>| ReplaceOutput {
        ok: true,
        match_count: total_matches,
        file_count,
        files: files.to_vec(),
        diff,
        identity: None,
        error_kind: None,
        error: None,
        match_mode: agg_mode,
        match_score: agg_score,
        matched_text: if files.len() == 1 {
            files[0].matched_text.clone()
        } else {
            None
        },
        similar_targets: None,
        skipped: skipped.clone(),
        refused: refused.clone(),
    };

    finalize_report(
        global,
        cwd,
        result,
        true,
        FinalizeCallbacks {
            on_check: |g: &GlobalFlags, _has: bool, _diffs: &[crate::diff::FileDiff]| {
                if g.json {
                    g.emit_json(&build_output(None))?;
                } else if !g.emit_json_items(files)? && !g.quiet {
                    println!("{total_matches} match(es) in {file_count} file(s)");
                    for f in files {
                        println!("  {}: {} match(es)", f.path, f.match_count);
                    }
                }
                if !g.quiet
                    && !g.json
                    && !g.jsonl
                    && let Some(ref refused) = refused
                {
                    for r in refused {
                        eprintln!(
                            "refused {}: {} (use --allow-absent-old to apply fuzzy candidate)",
                            r.path, r.reason
                        );
                    }
                }
                Ok(())
            },
            on_apply: |g: &GlobalFlags,
                       _has: bool,
                       diffs: &[crate::diff::FileDiff],
                       diff_text: Option<String>| {
                if g.json {
                    g.emit_json(&build_output(diff_text))?;
                } else if !g.emit_json_items(files)? {
                    if g.diff {
                        print!("{}", render_diffs_colored(diffs, g.should_color()));
                    } else if !g.quiet {
                        println!("replaced {total_matches} match(es) in {file_count} file(s)");
                        for f in files {
                            println!("  {}: {} match(es)", f.path, f.match_count);
                        }
                    }
                }
                if !g.quiet
                    && !g.json
                    && !g.jsonl
                    && let Some(ref refused) = refused
                {
                    for r in refused {
                        eprintln!(
                            "refused {}: {} (use --allow-absent-old to apply fuzzy candidate)",
                            r.path, r.reason
                        );
                    }
                }
                Ok(())
            },
            on_preview: |g: &GlobalFlags,
                         _has: bool,
                         diffs: &[crate::diff::FileDiff],
                         diff_text: Option<String>| {
                if g.json {
                    g.emit_json(&build_output(diff_text))?;
                } else if !g.emit_json_items(files)? && !diffs.is_empty() {
                    print!("{}", render_diffs_colored(diffs, g.should_color()));
                }
                if !g.quiet
                    && !g.json
                    && !g.jsonl
                    && let Some(ref refused) = refused
                {
                    for r in refused {
                        eprintln!(
                            "refused {}: {} (use --allow-absent-old to apply fuzzy candidate)",
                            r.path, r.reason
                        );
                    }
                }
                Ok(())
            },
            after_preview_emit: |g: &GlobalFlags| {
                if g.show_status() {
                    eprintln!("{file_count} file(s) changed, {total_matches} replacement(s)");
                }
            },
            after_preview_apply: |g: &GlobalFlags| {
                if g.show_status() {
                    eprintln!("replaced {total_matches} match(es) in {file_count} file(s)");
                }
            },
        },
    )
}

/// Context / pure-fuzzy replace: routes through the tx engine where
/// `context_filtered_offset` and the fallback chain live.
fn run_context_replace(
    args: ReplaceArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
    skipped: Option<Vec<String>>,
) -> anyhow::Result<u8> {
    use crate::plan::Operation;

    // Expand directories the same way as the default replace scan so
    // `patchloom replace --fuzzy OLD --new NEW src/` works (not only
    // single-file paths). Empty roots default to `.` via collect.
    let paths = if args.paths.is_empty() {
        vec![".".to_string()]
    } else {
        args.paths.clone()
    };
    let file_paths = crate::collect_file_paths_opts(&paths, global, false, Some(cwd))?;
    if file_paths.is_empty() {
        if crate::files::all_scan_targets_missing(global, &paths, Some(cwd))? {
            let msg = format!(
                "no such file or directory: {}",
                global.path_scope_description(&paths)
            );
            global.emit_error_json_kind(Some("not_found"), &msg)?;
            return Ok(exit::FAILURE);
        }
        let empty = ReplaceOutput {
            ok: args.if_exists,
            match_count: 0,
            file_count: 0,
            files: vec![],
            diff: None,
            identity: None,
            error_kind: if args.if_exists {
                None
            } else {
                Some("no_matches")
            },
            error: None,
            match_mode: None,
            match_score: None,
            matched_text: None,
            similar_targets: None,
            skipped: skipped.clone(),
            refused: None,
        };
        global.emit_json(&empty)?;
        if args.if_exists {
            return Ok(exit::SUCCESS);
        }
        if !global.quiet && !global.json && !global.jsonl {
            eprintln!(
                "no matches for '{}' in fuzzy/context replace ({})",
                args.old,
                global.path_scope_description(&paths)
            );
        }
        return Ok(exit::NO_MATCHES);
    }

    let ops: Vec<Operation> = file_paths
        .iter()
        .map(|p| {
            let rel = crate::files::relative_display(p, cwd)
                .to_string_lossy()
                .into_owned();
            Operation::Replace {
                glob: None,
                path: Some(rel),
                regex: args.regex,
                old: args.old.clone(),
                new_text: args.new.clone(),
                nth: args.nth,
                insert_before: args.insert_before.clone(),
                insert_after: args.insert_after.clone(),
                case_insensitive: args.case_insensitive,
                multiline: args.multiline,
                if_exists: args.if_exists || file_paths.len() > 1,
                // Multi-file fuzzy: soft-miss per file so one miss does not
                // abort the batch; overall require_change checked via has_changes.
                whole_line: args.whole_line,
                range: args.range.clone(),
                word_boundary: args.word_boundary,
                before_context: args.before_context.clone(),
                after_context: args.after_context.clone(),
                unique: args.unique,
                require_change: args.require_change && file_paths.len() == 1,
                command_position: args.command_position,
                fuzzy: args.fuzzy,
                min_fuzzy_score: args.min_fuzzy_score,
                allow_absent_old: args.allow_absent_old,
            }
        })
        .collect();

    let (cwd, result) = crate::cmd::output::stage_for_write(WriteSource::Operations(ops), global)?;

    if !result.has_changes {
        // Engine already computed floor / resolve / did-you-mean into replace_hint.
        let hint = result
            .exec_result
            .replace_hint
            .as_deref()
            .filter(|h| !h.is_empty())
            .map(str::to_string);
        // Soft refuse (#1758) records candidate honesty without a write.
        let (refuse_mode, refuse_score, refuse_matched) =
            aggregate_refuse_match_meta(&result.exec_result.replace_match_meta);
        let refused = collect_refused_files(
            &cwd,
            &result.exec_result.changes,
            &result.exec_result.replace_match_meta,
        );
        let empty = ReplaceOutput {
            ok: args.if_exists,
            match_count: 0,
            file_count: 0,
            files: vec![],
            diff: None,
            identity: None,
            error_kind: if args.if_exists {
                None
            } else {
                Some("no_matches")
            },
            error: if args.if_exists { None } else { hint.clone() },
            match_mode: refuse_mode,
            match_score: refuse_score,
            matched_text: refuse_matched,
            similar_targets: None,
            skipped: skipped.clone(),
            refused,
        };
        global.emit_json(&empty)?;
        if args.if_exists {
            return Ok(exit::SUCCESS);
        }
        if !global.quiet && !global.json && !global.jsonl {
            eprintln!("no matches for '{}' in context-based replace", args.old);
            // Engine floor / did-you-mean prose (also in JSON `error`).
            if let Some(ref h) = hint {
                eprintln!("{h}");
            }
        }
        return Ok(exit::NO_MATCHES);
    }

    // Prefer match honesty recorded during tx replace_op (#1674). Fall back to
    // re-running the library content path only when meta is missing (should not
    // happen for fuzzy/context ops that changed the file).
    let lib_opts = crate::api::ReplaceOptions {
        regex: args.regex,
        nth: args.nth,
        case_insensitive: args.case_insensitive,
        multiline: args.multiline,
        insert_before: args.insert_before.clone(),
        insert_after: args.insert_after.clone(),
        whole_line: args.whole_line,
        range: None,
        if_exists: true,
        word_boundary: args.word_boundary,
        unique: args.unique,
        fuzzy: args.fuzzy,
        before_context: args.before_context.clone(),
        after_context: args.after_context.clone(),
        require_change: false,
        command_position: args.command_position,
        min_fuzzy_score: args.min_fuzzy_score,
        allow_absent_old: args.allow_absent_old,
        post_write: None,
        post_write_cwd: None,
    };
    let to = args.new.as_deref().unwrap_or("");
    let files: Vec<ReplaceFileResult> = result
        .exec_result
        .changes
        .iter()
        .map(|(p, original, _new)| {
            let (mode, score, count, matched_text) =
                if let Some(m) = result.exec_result.replace_match_meta.get(p) {
                    (
                        Some(match_mode_str(m.mode)),
                        m.score,
                        m.match_count.max(1),
                        m.matched_text.clone(),
                    )
                } else {
                    match crate::api::replace_in_content(original, &args.old, to, &lib_opts) {
                        Ok(r) => (
                            r.match_mode.map(match_mode_str),
                            r.match_score,
                            r.match_count.max(1),
                            r.matched_text,
                        ),
                        // Do not invent "exact" when re-derive fails (#1674 honesty).
                        Err(_) => (None, None, 1, None),
                    }
                };
            ReplaceFileResult {
                path: crate::files::relative_display(p, &cwd)
                    .to_string_lossy()
                    .into_owned(),
                match_count: count,
                match_mode: mode,
                match_score: score,
                matched_text,
            }
        })
        .collect();
    let total_matches = files.len();
    let file_count = total_matches;
    // Same mode/exit owner as default replace path (no hand-rolled matrix).
    replace_output(
        global,
        result,
        &files,
        total_matches,
        file_count,
        &cwd,
        skipped,
    )
}

#[path = "replace_tests.rs"]
#[cfg(test)]
mod tests;
