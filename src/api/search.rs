/*! Search operations for the public library API. */

use std::path::{Path, PathBuf};

use crate::ops;

// ---------------------------------------------------------------------------
// Search operations
// ---------------------------------------------------------------------------

/// A single search match.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// 1-based line number.
    pub line_number: usize,
    /// The matched line content.
    pub line: String,
}

/// Search for a pattern in a file, returning all matching lines.
///
/// This is a read-only operation. For richer results with column/context use
/// `search_file` (the one taking SearchOptions) or the low-level `search_one_file`.
pub fn search(
    path: &Path,
    pattern: &str,
    regex: bool,
    case_insensitive: bool,
) -> anyhow::Result<Vec<SearchMatch>> {
    let display = path.to_string_lossy();
    let content = crate::files::load_text_strict(path, &display)?;

    if pattern.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "search pattern must not be empty".into(),
        }));
    }

    let compiled_re = if regex || case_insensitive {
        Some(ops::replace::compile_replace_regex(
            pattern,
            regex,
            case_insensitive,
            false,
            false,
        )?)
    } else {
        None
    };

    let mut matches = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let matched = match &compiled_re {
            Some(Some(re)) => re.is_match(line),
            _ => line.contains(pattern),
        };
        if matched {
            matches.push(SearchMatch {
                line_number: i + 1,
                line: line.to_string(),
            });
        }
    }
    Ok(matches)
}

/// Search a single file with full `SearchOptions` support (context, regex, literal, etc).
///
/// Returns rich `SearchResult` entries (with column + context when requested).
/// This is the recommended per-file primitive for library/agent hosts (#812).
/// For callers that do their own walking (custom ignores, limits, etc.) see
/// [`search_one_file`].
pub fn search_file(
    path: &Path,
    pattern: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    // Delegate to search_directory (handles file root case + full rich logic with correct column/context).
    // This ensures parity and reuses the complete one-file impl (#812/#815).
    search_directory(path, pattern, opts)
}

/// Options for full directory search (for library consumers).
///
/// Supports customization for advanced ignore behavior (e.g. agent/tool ignore files)
/// via `exclude_patterns` and `custom_ignore_filenames`. The underlying
/// `ignore::WalkBuilder` is used when the "files" feature is enabled, so
/// standard .gitignore is respected by default.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Treat pattern as literal string (not regex).
    pub literal: bool,
    /// Treat as regex (default false, use with literal=false).
    pub regex: bool,
    /// Case insensitive match.
    pub case_insensitive: bool,
    /// Context lines around matches (symmetric). Overridden by
    /// `before_context` / `after_context` when those are set.
    pub context: Option<usize>,
    /// Lines of context before each match. Takes precedence over `context`.
    pub before_context: Option<usize>,
    /// Lines of context after each match. Takes precedence over `context`.
    pub after_context: Option<usize>,
    /// Show lines that do NOT match the pattern.
    pub invert_match: bool,
    /// Enable multiline matching (dot matches newlines in regex mode).
    pub multiline: bool,
    /// Glob patterns to include (e.g. "*.rs").
    pub globs: Vec<String>,
    /// Max number of results (0 for unlimited).
    pub max_results: usize,
    /// Additional gitignore-style patterns to *exclude* (e.g. from a custom ignore file).
    /// These are applied in addition to any .gitignore respected by the walker.
    pub exclude_patterns: Vec<String>,
    /// Custom ignore filenames to respect in addition to .gitignore / .ignore
    /// (e.g. `vec![".agentignore".to_string()]`).
    pub custom_ignore_filenames: Vec<String>,
}

/// Result of a search match with optional context.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line_number: usize,
    pub line: String,
    pub column: usize,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Helper to build context slices for a match line. DRY for search impls (library + CLI).
///
/// Exposed for downstreams that want consistent context extraction (#815).
///
/// Accepts asymmetric context sizes (`before_ctx`, `after_ctx`). For symmetric
/// context pass the same value for both.
pub fn build_context_lines(
    all_lines: &[&str],
    match_idx: usize,
    before_ctx: usize,
    after_ctx: usize,
) -> (Vec<String>, Vec<String>) {
    let before = if before_ctx == 0 {
        vec![]
    } else {
        let start = match_idx.saturating_sub(before_ctx);
        all_lines[start..match_idx]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    let after = if after_ctx == 0 {
        vec![]
    } else {
        let end = (match_idx + 1 + after_ctx).min(all_lines.len());
        all_lines[match_idx + 1..end]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    (before, after)
}

/// Search a directory (or file) recursively for pattern.
/// Respects globs and custom ignores (via `SearchOptions`) if "files" feature enabled.
/// Uses parallel processing when available.
/// This provides the full power of the CLI search for library use (see #773, #796).
/// See `SearchOptions` for `exclude_patterns` and `custom_ignore_filenames`
/// (agent/tool ignore files, etc.).
///
/// For callers that already have a list of files (custom walker), see [`search_one_file`].
///
/// Example for custom ignore files used by LLM agents:
/// ```ignore
/// let opts = SearchOptions {
///     custom_ignore_filenames: vec![".agentignore".into()],
///     exclude_patterns: vec!["*.tmp".into()],
///     ..Default::default()
/// };
/// let hits = search_directory(Path::new("."), "TODO", &opts)?;
/// ```
pub fn search_directory(
    root: &Path,
    pattern: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    if pattern.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "search pattern must not be empty".into(),
        }));
    }
    // Fail closed on invalid regex before scanning: search_one_file soft-skips
    // bad patterns (returns empty) so a directory walk would look like "no
    // matches" (agent-hostile). Preflight matches CLI/search() erroring on
    // compile failure.
    validate_search_pattern(pattern, opts)?;

    #[cfg(any(feature = "cli", feature = "files"))]
    {
        use crate::files::{build_glob_matcher, par_process_files};
        let glob_matcher = build_glob_matcher(&opts.globs)?;
        let glob_roots = vec![root.to_path_buf()];

        // Delegate to shared helper for identical multi-source ignore precedence (#813).
        let file_paths = crate::files::collect_file_paths_with_ignores(
            root,
            &opts.custom_ignore_filenames,
            &opts.exclude_patterns,
            false, // search typically does not traverse hidden unless requested via other means
        )?;

        let limit = if opts.max_results > 0 {
            opts.max_results
        } else {
            usize::MAX
        };

        let file_result_groups: Vec<Vec<SearchResult>> =
            par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let v = search_one_file(path, pattern, opts, root);
                if v.is_empty() { None } else { Some(v) }
            });

        let mut res: Vec<SearchResult> = file_result_groups.into_iter().flatten().collect();
        if limit < usize::MAX {
            res.truncate(limit);
        }
        Ok(res)
    }

    #[cfg(not(any(feature = "cli", feature = "files")))]
    {
        if opts.multiline {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "multiline search requires the 'cli' or 'files' feature".into(),
            }));
        }
        if opts.invert_match {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "invert_match search requires the 'cli' or 'files' feature".into(),
            }));
        }
        // fallback to single file search (multi-match supported via basic search)
        if root.is_file() {
            let basic = search(root, pattern, opts.regex, opts.case_insensitive)?;
            let path_buf = root.to_path_buf();
            let ctx_b = opts.before_context.or(opts.context).unwrap_or(0);
            let ctx_a = opts.after_context.or(opts.context).unwrap_or(0);
            // read once for both content and context lines (fallback is rare / no "files" feature)
            let label = root.to_string_lossy();
            let content = crate::files::load_text_strict(root, &label)?;
            let all_lines: Vec<&str> = content.lines().collect();
            let results: Vec<SearchResult> = basic
                .into_iter()
                .map(|m| {
                    let i = m.line_number - 1;
                    let (context_before, context_after) =
                        build_context_lines(&all_lines, i, ctx_b, ctx_a);
                    // column not tracked in basic fallback search (always 1; full impl behind "files" feature)
                    let column = 1;
                    SearchResult {
                        path: path_buf.clone(),
                        line_number: m.line_number,
                        line: m.line,
                        column,
                        context_before,
                        context_after,
                    }
                })
                .collect();
            Ok(results)
        } else {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "search_directory requires the 'files' feature to be enabled (for pure-library recursive search with ignores/parallelism)".into(),
            }));
        }
    }
}

/// Compile the pattern the same way [`search_one_file`] does, or error.
///
/// Used by high-level APIs so invalid regex is not mistaken for zero matches.
fn validate_search_pattern(pattern: &str, opts: &SearchOptions) -> anyhow::Result<()> {
    if !(opts.regex || opts.case_insensitive || opts.multiline) {
        return Ok(());
    }
    let pat = if opts.literal || (opts.multiline && !opts.regex) {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    crate::bounded_regex_build(
        crate::bounded_regex_builder(&pat)
            .case_insensitive(opts.case_insensitive)
            .multi_line(true)
            .dot_matches_new_line(opts.multiline),
    )?;
    Ok(())
}

/// Low-level single-file matcher for callers that already selected the files.
///
/// Returns rich [`SearchResult`]s (including `column` and context when requested
/// via [`SearchOptions`]). Intended for advanced library users (e.g. custom
/// `WalkBuilder` with extra ignores, size caps, depth limits, custom truncation)
/// who then want to apply patchloom's matching logic.
///
/// Prefer [`search_file`] for simple per-file use and [`search_directory`] for
/// full directory walking with built-in ignore support.
///
/// **Invalid regex:** this low-level helper returns an empty `Vec` when the
/// pattern fails to compile (so walkers can keep going). High-level
/// [`search_directory`] / [`search_file`] preflight and return
/// `InvalidInputError` instead — prefer those when agents must not treat a
/// bad pattern as "no matches."
#[cfg(any(feature = "cli", feature = "files"))]
pub fn search_one_file(
    path: &Path,
    pattern: &str,
    opts: &SearchOptions,
    root: &Path,
) -> Vec<SearchResult> {
    let content = match crate::files::read_text_file(path) {
        Some(c) => c,
        None => return vec![],
    };
    let display = crate::files::relative_display(path, root);
    let pat = if opts.literal || (opts.multiline && !opts.regex) {
        // Auto-escape when literal is set, or when multiline mode is used
        // without regex (the pattern must go through RegexBuilder but the
        // user expects literal matching).
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let re = if opts.regex || opts.case_insensitive || opts.multiline {
        match crate::bounded_regex_builder(&pat)
            .case_insensitive(opts.case_insensitive)
            .multi_line(true)
            .dot_matches_new_line(opts.multiline)
            .build()
        {
            Ok(r) => Some(r),
            Err(_) => return vec![],
        }
    } else {
        None
    };

    let ctx_before = opts.before_context.or(opts.context).unwrap_or(0);
    let ctx_after = opts.after_context.or(opts.context).unwrap_or(0);

    // Multiline mode: match against full content, report the start line of each match
    if opts.multiline {
        // `re` is always Some here: multiline triggers the regex path above,
        // and invalid patterns return early with an empty vec.
        let re = re.as_ref().expect("multiline always builds regex");
        let mut results = Vec::new();
        let all_lines: Vec<&str> = content.lines().collect();
        for m in re.find_iter(&content) {
            let start_byte = m.start();
            let line_num = content[..start_byte].matches('\n').count();
            let line_text = all_lines.get(line_num).unwrap_or(&"").to_string();
            let (context_before, context_after) =
                build_context_lines(&all_lines, line_num, ctx_before, ctx_after);
            results.push(SearchResult {
                path: display.to_path_buf(),
                line_number: line_num + 1,
                line: line_text,
                column: 1,
                context_before,
                context_after,
            });
        }
        return results;
    }

    let mut results = Vec::new();
    let all_lines: Vec<&str> = content.lines().collect();
    for (i, line) in all_lines.iter().enumerate() {
        let found = if let Some(re) = &re {
            re.is_match(line)
        } else {
            line.contains(pattern)
        };
        let is_match = if opts.invert_match { !found } else { found };
        if is_match {
            let (context_before, context_after) =
                build_context_lines(&all_lines, i, ctx_before, ctx_after);
            let column = if !opts.invert_match {
                if let Some(re) = &re {
                    re.find(line).map_or(1, |m| m.start() + 1)
                } else {
                    line.find(pattern).map_or(1, |p| p + 1)
                }
            } else {
                1
            };
            results.push(SearchResult {
                path: display.to_path_buf(),
                line_number: i + 1,
                line: line.to_string(),
                column,
                context_before,
                context_after,
            });
        }
    }
    results
}

/// Format `SearchResult`s for display (human text or JSON).
///
/// Library / agent hosts can use this for consistent output with CLI search (#812).
pub fn format_search_results(results: &[SearchResult], as_json: bool) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    if as_json {
        let payload: Vec<_> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path,
                    "line": r.line_number,
                    "text": r.line,
                    "column": r.column,
                    "context_before": r.context_before,
                    "context_after": r.context_after,
                })
            })
            .collect();
        // Fail-closed for agent hosts (#1651 class): never empty JSON body.
        match serde_json::to_string_pretty(&payload) {
            Ok(s) => {
                out = s;
                out.push('\n');
            }
            Err(e) => {
                out = crate::json_emit::fallback_envelope(&format!("search results: {e}"), false);
                out.push('\n');
            }
        }
    } else {
        for r in results {
            for ctx in &r.context_before {
                let _ = writeln!(out, "  {}", ctx);
            }
            let _ = writeln!(out, "{}:{}: {}", r.path.display(), r.line_number, r.line);
            for ctx in &r.context_after {
                let _ = writeln!(out, "  {}", ctx);
            }
        }
    }
    out
}
