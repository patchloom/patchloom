use super::execute::{TxState, read_file_content, update_file_content};
use crate::ops::replace::{
    compile_replace_regex, replace_content, replace_whole_lines, replacement_text,
};
use crate::plan::Operation;
use globset::Glob;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::Path;

/// Execute a replace operation within a transaction.
pub(crate) fn execute_replace_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    let Operation::Replace {
        glob,
        path,
        mode,
        from,
        to,
        nth,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        whole_line,
        range,
        before_context,
        after_context,
        ..
    } = op
    else {
        unreachable!()
    };
    let regex_mode = mode.as_deref() == Some("regex");
    let word_boundary = matches!(
        op,
        Operation::Replace {
            word_boundary: true,
            ..
        }
    );
    let use_regex = regex_mode || *case_insensitive || word_boundary;
    let replacement = replacement_text(from, to, insert_before, insert_after, use_regex);
    let compiled_re = compile_replace_regex(
        from,
        regex_mode,
        *case_insensitive,
        *multiline,
        word_boundary,
    )?;
    let parsed_range = range
        .as_deref()
        .map(crate::ops::read::parse_line_range)
        .transpose()?;

    if let Some(p) = path {
        let file_path = tx.cwd.join(p);
        let content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
        let (replaced, match_count) = if *whole_line {
            replace_whole_lines(
                content,
                from,
                &replacement,
                compiled_re.as_ref(),
                *nth,
                parsed_range,
            )
        } else {
            replace_content(content, from, &replacement, compiled_re.as_ref(), *nth)
        };
        if match_count > 0 {
            let owned = replaced.into_owned();
            update_file_content(tx.pending, tx.deletions, &file_path, owned);
            Ok(match_count)
        } else if !regex_mode && (before_context.is_some() || after_context.is_some()) {
            // Tier 3: Use context-based fallback when exact match fails.
            match crate::fallback::resolve_with_fallback(
                content,
                from,
                before_context.as_deref(),
                after_context.as_deref(),
            ) {
                Ok(anchor) => {
                    let to_text = if let Some(ib) = insert_before {
                        format!("{}{}", ib, anchor.matched_text)
                    } else if let Some(ia) = insert_after {
                        format!("{}{}", anchor.matched_text, ia)
                    } else {
                        to.as_deref().unwrap_or("").to_string()
                    };
                    let new_content = format!(
                        "{}{}{}",
                        &content[..anchor.start_offset],
                        to_text,
                        &content[anchor.start_offset + anchor.matched_text.len()..]
                    );
                    update_file_content(tx.pending, tx.deletions, &file_path, new_content);
                    tx.replace_hint = Some(format!(
                        "fallback matched via {:?} strategy in {}",
                        anchor.strategy, p,
                    ));
                    Ok(1)
                }
                Err(edit_error) => {
                    tx.replace_hint = Some(edit_error.message.clone());
                    Ok(0)
                }
            }
        } else {
            if !regex_mode {
                // Tier 1: Provide "did you mean?" hints for literal no-match.
                let similar = crate::fallback::find_similar_targets(content, from, 3);
                if !similar.is_empty() {
                    tx.replace_hint = Some(format!(
                        "no matches for '{}' in {} (did you mean: {}?)",
                        crate::fallback::truncate_str(from, 60),
                        p,
                        similar.join(", ")
                    ));
                }
            }
            Ok(0)
        }
    } else if let Some(pattern) = glob {
        let matcher = Glob::new(pattern)?.compile_matcher();
        let matches_pattern = |path: &Path| {
            matcher.is_match(path)
                || path.file_name().is_some_and(|name| matcher.is_match(name))
                || path.strip_prefix(tx.cwd).ok().is_some_and(|relative| {
                    !relative.as_os_str().is_empty()
                        && (matcher.is_match(relative)
                            || relative
                                .file_name()
                                .is_some_and(|name| matcher.is_match(name)))
                })
        };
        let mut total_matches = 0usize;
        let mut candidate_paths = Vec::new();
        let mut seen_paths = HashSet::new();

        let walker = WalkBuilder::new(tx.cwd).build();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let file_path = entry.path().to_path_buf();
            if matches_pattern(&file_path) && seen_paths.insert(file_path.clone()) {
                candidate_paths.push(file_path);
            }
        }

        for pending_path in tx.pending.keys() {
            if !pending_path.starts_with(tx.cwd)
                || pending_path.exists()
                || tx.deletions.contains(pending_path)
                || !matches_pattern(pending_path)
            {
                continue;
            }
            if seen_paths.insert(pending_path.clone()) {
                candidate_paths.push(pending_path.clone());
            }
        }

        for file_path in candidate_paths {
            let content = match read_file_content(tx.pending, tx.existed_before, &file_path) {
                Ok(c) => c.to_owned(),
                Err(e) => {
                    if !tx.structured && !tx.quiet {
                        eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                    }
                    continue;
                }
            };
            let (replaced, match_count) = if *whole_line {
                replace_whole_lines(
                    &content,
                    from,
                    &replacement,
                    compiled_re.as_ref(),
                    *nth,
                    parsed_range,
                )
            } else {
                replace_content(&content, from, &replacement, compiled_re.as_ref(), *nth)
            };
            total_matches += match_count;
            if match_count > 0 {
                update_file_content(tx.pending, tx.deletions, &file_path, replaced.into_owned());
            }
        }
        Ok(total_matches)
    } else {
        anyhow::bail!("replace operation requires either 'path' or 'glob'");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Operation;
    use crate::tx::execute::{CachedDoc, TxState};
    use crate::tx::output::{TxLintResult, TxReadResult, TxSearchResult};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
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

    #[test]
    fn replace_literal_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            mode: None,
            from: "hello".into(),
            to: Some("goodbye".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 1);
        assert_eq!(pending[&file].1, "goodbye world");
    }

    #[test]
    fn replace_no_match_returns_zero() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            mode: None,
            from: "nonexistent".into(),
            to: Some("replacement".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn replace_regex_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "foo123bar").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            mode: Some("regex".into()),
            from: r"\d+".into(),
            to: Some("NUM".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 1);
        assert_eq!(pending[&file].1, "fooNUMbar");
    }

    #[test]
    fn replace_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            mode: None,
            from: "hello".into(),
            to: Some("hi".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: true,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 1);
        assert_eq!(pending[&file].1, "hi World");
    }

    #[test]
    fn replace_missing_path_and_glob_errors() {
        let dir = TempDir::new().unwrap();

        let op = Operation::Replace {
            path: None,
            glob: None,
            mode: None,
            from: "x".into(),
            to: Some("y".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let result = execute_replace_op(&op, &mut tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("'path' or 'glob'"));
    }

    #[test]
    fn replace_glob_matches_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old value").unwrap();
        std::fs::write(dir.path().join("b.txt"), "old value").unwrap();
        std::fs::write(dir.path().join("c.rs"), "old value").unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*.txt".into()),
            mode: None,
            from: "old".into(),
            to: Some("new".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 2); // a.txt and b.txt matched
        // c.rs should not be modified
        assert!(!pending.contains_key(&dir.path().join("c.rs")));
    }

    #[test]
    fn replace_insert_before_with_fallback_preserves_matched_text() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "fn process(input: Vec<u8>) {\n}\n").unwrap();

        // `from` is stale (Vec<i32> instead of Vec<u8>), so exact match fails.
        // Fallback should insert_before the matched text, not delete it.
        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            mode: None,
            from: "fn process(input: Vec<i32>) {".into(),
            to: None,
            nth: None,
            insert_before: Some("/// Process input.\n".into()),
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: Some("fn process".into()),
            after_context: None,
            if_exists: false,
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

        let count = execute_replace_op(&op, &mut tx).unwrap();
        assert_eq!(count, 1);
        let result = &pending[&file].1;
        assert!(
            result.contains("fn process(input: Vec<u8>)"),
            "original function signature must be preserved, got: {result}"
        );
        assert!(
            result.contains("/// Process input."),
            "insert_before text must be present, got: {result}"
        );
    }
}
