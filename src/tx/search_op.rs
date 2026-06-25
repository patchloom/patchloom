use super::execute::{TxState, read_and_probe, read_file_content};
use super::output::{TxSearchMatch, TxSearchResult};
use crate::plan::Operation;
use regex::RegexBuilder;
use std::path::PathBuf;

/// Execute a search operation within a transaction.
///
/// If `path` is a directory, walks it (respecting `.gitignore`) and searches
/// each non-binary file, mirroring the standalone `search` command behavior.
pub(crate) fn execute_search_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let Operation::Search {
        path,
        pattern,
        regex,
        case_insensitive,
        multiline,
        invert_match,
        context,
        before_context,
        after_context,
        assert_count,
        literal,
        globs,
        max_results,
        exclude_patterns,
        custom_ignore_filenames,
    } = op
    else {
        unreachable!()
    };

    if *invert_match && *multiline {
        anyhow::bail!("invert_match and multiline cannot be combined");
    }
    if *literal && *regex {
        anyhow::bail!("search: literal and regex cannot be combined");
    }

    // literal means treat pattern as literal text (escape it); !regex also escapes for backward compat in plans.
    let pat = if *literal || !*regex {
        regex::escape(pattern)
    } else {
        pattern.clone()
    };
    let re = {
        let mut builder = RegexBuilder::new(&pat);
        builder.case_insensitive(*case_insensitive);
        builder.dot_matches_new_line(*multiline);
        builder.build()?
    };

    let ctx_before = before_context.or(*context).unwrap_or(0);
    let ctx_after = after_context.or(*context).unwrap_or(0);

    let resolved = tx.cwd.join(path);
    // Use the shared advanced ignore collection when available for parity with
    // api::search_directory / CLI (respects .gitignore + custom_ignore_filenames + exclude_patterns).
    // Then apply include globs.
    #[cfg(any(feature = "cli", feature = "files"))]
    let candidate_paths: Vec<PathBuf> = if resolved.is_dir() {
        crate::files::collect_file_paths_with_ignores(
            &resolved,
            custom_ignore_filenames,
            exclude_patterns,
            false,
        )?
    } else {
        vec![resolved.clone()]
    };
    #[cfg(not(any(feature = "cli", feature = "files")))]
    let candidate_paths: Vec<PathBuf> = if resolved.is_dir() {
        let mut paths = Vec::new();
        for entry in WalkBuilder::new(&resolved).build() {
            let entry = entry?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                paths.push(entry.into_path());
            }
        }
        paths.sort();
        paths
    } else {
        vec![resolved.clone()]
    };

    let glob_matcher = crate::files::build_glob_matcher(globs)?;
    // glob_roots used for relative glob matching; use the search root (dir or parent of file)
    let glob_root = if resolved.is_dir() {
        resolved.clone()
    } else {
        resolved.parent().unwrap_or(&resolved).to_path_buf()
    };
    let glob_roots = vec![glob_root];

    let file_paths: Vec<PathBuf> = if let Some(m) = &glob_matcher {
        candidate_paths
            .into_iter()
            .filter(|p| crate::files::matches_glob_with_roots(p, Some(m), &glob_roots))
            .collect()
    } else {
        candidate_paths
    };

    let mut all_matches = Vec::new();

    for file_path in &file_paths {
        // For directory walks, read the file once and check for binary content
        // in the same buffer (avoids double-reading text files: 8 KiB probe +
        // full re-read). For single-file paths, skip the binary check.
        if file_paths.len() > 1 {
            if !read_and_probe(tx.pending, tx.existed_before, file_path)? {
                continue; // binary file, skip
            }
        } else {
            read_file_content(tx.pending, tx.existed_before, file_path)?;
        }
        let content = &tx.pending[file_path].1;
        let lines: Vec<&str> = content.lines().collect();

        let is_multi_file = file_paths.len() > 1;
        if *multiline {
            // Multiline mode: search the full content so patterns can span lines.
            for m in re.find_iter(content) {
                let line_idx = content[..m.start()].matches('\n').count();
                let start = line_idx.saturating_sub(ctx_before);
                let end = (line_idx + 1 + ctx_after).min(lines.len());
                let matched_text = m.as_str().to_string();
                let text = if is_multi_file {
                    let display_path = file_path
                        .strip_prefix(tx.cwd)
                        .unwrap_or(file_path)
                        .to_string_lossy();
                    format!("{display_path}:{matched_text}")
                } else {
                    matched_text
                };
                let col = m.start() - content[..m.start()].rfind('\n').map_or(0, |p| p + 1);
                all_matches.push(TxSearchMatch {
                    line: line_idx + 1,
                    column: col + 1,
                    text,
                    context_before: lines[start..line_idx]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    context_after: lines[line_idx + 1..end]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                });
            }
        } else {
            for (i, line) in lines.iter().enumerate() {
                let found = re.find(line);
                let is_match = if *invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };
                if is_match {
                    let start = i.saturating_sub(ctx_before);
                    let end = (i + 1 + ctx_after).min(lines.len());
                    let text = if is_multi_file {
                        let display_path = file_path
                            .strip_prefix(tx.cwd)
                            .unwrap_or(file_path)
                            .to_string_lossy();
                        format!("{display_path}:{}", line)
                    } else {
                        line.to_string()
                    };
                    let column = found.map_or(1, |m| m.start() + 1);
                    all_matches.push(TxSearchMatch {
                        line: i + 1,
                        column,
                        text,
                        context_before: lines[start..i].iter().map(|s| s.to_string()).collect(),
                        context_after: lines[i + 1..end].iter().map(|s| s.to_string()).collect(),
                    });
                }
            }
        }
    }

    // Cap after collection. Append order matches walk order.
    if *max_results > 0 {
        all_matches.truncate(*max_results);
    }

    if let Some(expected) = assert_count
        && all_matches.len() != *expected
    {
        anyhow::bail!(
            "search assert_count: expected {} matches for '{}' in {}, found {}",
            expected,
            pattern,
            path,
            all_matches.len()
        );
    }

    tx.tx_searches.push(TxSearchResult {
        path: path.clone(),
        pattern: pattern.clone(),
        match_count: all_matches.len(),
        matches: all_matches,
    });
    Ok(())
}
