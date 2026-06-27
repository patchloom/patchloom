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
                    context_before: lines[start..line_idx.min(lines.len())]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    context_after: if line_idx + 1 < lines.len() {
                        lines[line_idx + 1..end]
                            .iter()
                            .map(|s| s.to_string())
                            .collect()
                    } else {
                        vec![]
                    },
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

    // Validate assert_count against the true total before truncation.
    let total_match_count = all_matches.len();

    if let Some(expected) = assert_count
        && total_match_count != *expected
    {
        anyhow::bail!(
            "search assert_count: expected {} matches for '{}' in {}, found {}",
            expected,
            pattern,
            path,
            total_match_count
        );
    }

    // Cap after assertion check. Append order matches walk order.
    if *max_results > 0 {
        all_matches.truncate(*max_results);
    }

    tx.tx_searches.push(TxSearchResult {
        path: path.clone(),
        pattern: pattern.clone(),
        match_count: total_match_count,
        matches: all_matches,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::execute::{CachedDoc, TxState};
    use crate::tx::output::{TxLintResult, TxReadResult, TxSearchResult};
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use tempfile::TempDir;

    #[allow(clippy::too_many_arguments)]
    fn make_tx_state<'a>(
        pending: &'a mut HashMap<PathBuf, (String, String)>,
        deletions: &'a mut HashSet<PathBuf>,
        existed_before: &'a mut HashSet<PathBuf>,
        doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
        reads: &'a mut Vec<TxReadResult>,
        searches: &'a mut Vec<TxSearchResult>,
        lints: &'a mut Vec<TxLintResult>,
        cwd: &'a Path,
    ) -> TxState<'a> {
        TxState {
            pending,
            deletions,
            existed_before,
            doc_cache,
            tx_reads: reads,
            tx_searches: searches,
            tx_lints: lints,
            replace_hint: None,
            cwd,
            quiet: true,
            structured: false,
        }
    }

    fn search_op(path: &str, pattern: &str) -> Operation {
        Operation::Search {
            path: path.into(),
            pattern: pattern.into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        }
    }

    #[test]
    fn search_literal_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "line one\nline two\nline three\n").unwrap();

        let op = search_op("test.txt", "two");

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0].match_count, 1);
        assert_eq!(searches[0].matches[0].line, 2);
    }

    #[test]
    fn search_no_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world\n").unwrap();

        let op = search_op("test.txt", "nonexistent");

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches[0].match_count, 0);
    }

    #[test]
    fn search_regex_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "foo123\nbar456\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: r"\d+".into(),
            regex: true,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches[0].match_count, 2);
    }

    #[test]
    fn search_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "hello".into(),
            regex: false,
            case_insensitive: true,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches[0].match_count, 1);
    }

    #[test]
    fn search_invert_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "keep\nskip this\nkeep too\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "skip".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: true,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches[0].match_count, 2); // "keep" and "keep too"
    }

    #[test]
    fn search_assert_count_mismatch_errors() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "one match\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "match".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: Some(5), // expect 5 but only 1 exists
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        let result = execute_search_op(&op, &mut tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("assert_count"));
    }

    #[test]
    fn search_literal_and_regex_combined_errors() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "content").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "x".into(),
            regex: true,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: true,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        let result = execute_search_op(&op, &mut tx);
        assert!(result.is_err());
    }

    #[test]
    fn search_with_context() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "aaa\nbbb\nccc\nddd\neee\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "ccc".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: Some(1),
            after_context: Some(1),
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        let m = &searches[0].matches[0];
        assert_eq!(m.line, 3);
        assert_eq!(m.context_before, vec!["bbb"]);
        assert_eq!(m.context_after, vec!["ddd"]);
    }

    #[test]
    fn search_max_results_cap() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "aaa\naaa\naaa\naaa\naaa\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "aaa".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 2,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        // match_count reports the true total (5 matches), not the truncated count
        assert_eq!(searches[0].match_count, 5);
        // matches vec is truncated to max_results
        assert_eq!(searches[0].matches.len(), 2);
    }

    #[test]
    fn search_assert_count_checks_before_max_results_truncation() {
        // File has 3 matches. assert_count=3 should pass even with max_results=1.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "foo\nfoo\nfoo\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "foo".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: Some(3),
            literal: false,
            globs: Vec::new(),
            max_results: 1,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        execute_search_op(&op, &mut tx).unwrap();
        // match_count reports the true total, not the truncated count
        assert_eq!(searches[0].match_count, 3);
        // matches is truncated to max_results
        assert_eq!(searches[0].matches.len(), 1);
    }

    #[test]
    fn search_multiline_context_at_eof_no_panic() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        // File ending with newline; regex matching at end-of-string
        std::fs::write(&file, "hello\nworld\n").unwrap();

        let op = Operation::Search {
            path: "test.txt".into(),
            pattern: "world".into(),
            regex: true,
            context: Some(2),
            before_context: None,
            after_context: None,
            max_results: 0,
            case_insensitive: false,
            invert_match: false,
            multiline: true,
            assert_count: None,
            literal: false,
            globs: vec![],
            exclude_patterns: vec![],
            custom_ignore_filenames: vec![],
        };

        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let mut existed = HashSet::new();
        let mut doc_cache = HashMap::new();
        let mut reads = Vec::new();
        let mut searches = Vec::new();
        let mut lints = Vec::new();

        let mut tx = make_tx_state(
            &mut pending,
            &mut deletions,
            &mut existed,
            &mut doc_cache,
            &mut reads,
            &mut searches,
            &mut lints,
            dir.path(),
        );

        // Should not panic even when match is near end of file
        execute_search_op(&op, &mut tx).unwrap();
        assert_eq!(searches[0].match_count, 1);
    }
}
