use super::execute::{TxState, read_and_probe, read_file_content, update_file_content};
use crate::ops::replace::{
    compile_replace_regex, context_filtered_offset, replace_content, replace_whole_lines,
    replacement_text,
};
use crate::plan::Operation;
use globset::Glob;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::Path;

/// Execute a replace operation within a transaction.
pub(crate) fn execute_replace_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    crate::verbose!(
        "replace_op: target={}, old_len={}, regex={}",
        op.declared_paths().first().unwrap_or(&"<glob>"),
        if let Operation::Replace { old, .. } = op {
            old.len()
        } else {
            0
        },
        if let Operation::Replace { regex, .. } = op {
            regex
        } else {
            &false
        }
    );
    let Operation::Replace {
        glob,
        path,
        regex: regex_mode,
        old,
        new_text,
        nth,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        whole_line,
        range,
        before_context,
        after_context,
        unique,
        ..
    } = op
    else {
        anyhow::bail!("execute_replace_op called with non-Replace operation")
    };
    let regex_mode = *regex_mode;
    let word_boundary = matches!(
        op,
        Operation::Replace {
            word_boundary: true,
            ..
        }
    );
    let use_regex = regex_mode || *case_insensitive || word_boundary;
    let replacement = replacement_text(
        old,
        new_text,
        insert_before,
        insert_after,
        use_regex,
        regex_mode,
    );
    let compiled_re = compile_replace_regex(
        old,
        regex_mode,
        *case_insensitive,
        *multiline,
        word_boundary,
    )?;
    if range.is_some() && !*whole_line {
        anyhow::bail!("range requires whole_line mode");
    }
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
                old,
                &replacement,
                compiled_re.as_ref(),
                *nth,
                parsed_range,
            )
        } else {
            replace_content(content, old, &replacement, compiled_re.as_ref(), *nth)
        };
        if *unique && match_count > 1 {
            anyhow::bail!(
                "ambiguous match: pattern {:?} matches {} times in {}; provide more context to disambiguate",
                crate::fallback::truncate_str(old, 60),
                match_count,
                p
            );
        }
        if match_count > 0 {
            // When there are multiple exact matches and context is provided
            // (but no nth), use context to disambiguate instead of replacing all.
            if match_count > 1
                && nth.is_none()
                && !*whole_line
                && !regex_mode
                && (before_context.is_some() || after_context.is_some())
                && let Some(target_offset) = context_filtered_offset(
                    content,
                    old,
                    before_context.as_deref(),
                    after_context.as_deref(),
                )
            {
                let new_content = format!(
                    "{}{}{}",
                    &content[..target_offset],
                    &replacement,
                    &content[target_offset + old.len()..],
                );
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &file_path,
                    new_content,
                );
                return Ok(1);
            }
            let owned = replaced.into_owned();
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &file_path,
                owned,
            );
            Ok(match_count)
        } else if !regex_mode && (before_context.is_some() || after_context.is_some()) {
            // Tier 3: Use context-based fallback when exact match fails.
            match crate::fallback::resolve_with_fallback(
                content,
                old,
                before_context.as_deref(),
                after_context.as_deref(),
            ) {
                Ok(anchor) => {
                    let to_text = if let Some(ib) = insert_before {
                        format!("{}{}", ib, anchor.matched_text)
                    } else if let Some(ia) = insert_after {
                        format!("{}{}", anchor.matched_text, ia)
                    } else {
                        new_text.as_deref().unwrap_or("").to_string()
                    };
                    let new_content = format!(
                        "{}{}{}",
                        &content[..anchor.start_offset],
                        to_text,
                        &content[anchor.start_offset + anchor.matched_text.len()..]
                    );
                    update_file_content(
                        tx.pending,
                        tx.deletions,
                        tx.write_targets,
                        &file_path,
                        new_content,
                    );
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
                let similar = crate::fallback::find_similar_targets(content, old, 3);
                if !similar.is_empty() {
                    tx.replace_hint = Some(format!(
                        "no matches for '{}' in {} (did you mean: {}?)",
                        crate::fallback::truncate_str(old, 60),
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
            match read_and_probe(tx.pending, tx.existed_before, &file_path) {
                Ok(false) => continue, // binary file, skip
                Ok(true) => {}
                Err(e) => {
                    if !tx.structured && !tx.quiet {
                        eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                    }
                    continue;
                }
            }
            let content = tx
                .pending
                .get(&file_path)
                .map(|(_, c)| c.clone())
                .expect("read_and_probe guarantees entry exists in pending");
            let (replaced, match_count) = if *whole_line {
                replace_whole_lines(
                    &content,
                    old,
                    &replacement,
                    compiled_re.as_ref(),
                    *nth,
                    parsed_range,
                )
            } else {
                replace_content(&content, old, &replacement, compiled_re.as_ref(), *nth)
            };
            total_matches += match_count;
            if match_count > 0 {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &file_path,
                    replaced.into_owned(),
                );
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
    use crate::tx::TxStateFixture;
    use tempfile::TempDir;

    #[test]
    fn replace_literal_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "hello".into(),
            new_text: Some("goodbye".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "goodbye world");
    }

    #[test]
    fn replace_no_match_returns_zero() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "nonexistent".into(),
            new_text: Some("replacement".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
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
            regex: true,
            old: r"\d+".into(),
            new_text: Some("NUM".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "fooNUMbar");
    }

    #[test]
    fn replace_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "hello".into(),
            new_text: Some("hi".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        assert_eq!(f.pending[&file].1, "hi World");
    }

    #[test]
    fn replace_missing_path_and_glob_errors() {
        let dir = TempDir::new().unwrap();

        let op = Operation::Replace {
            path: None,
            glob: None,
            regex: false,
            old: "x".into(),
            new_text: Some("y".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
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
            regex: false,
            old: "old".into(),
            new_text: Some("new".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 2); // a.txt and b.txt matched
        // c.rs should not be modified
        assert!(!f.pending.contains_key(&dir.path().join("c.rs")));
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
            regex: false,
            old: "fn process(input: Vec<i32>) {".into(),
            new_text: None,
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        let result = &f.pending[&file].1;
        assert!(
            result.contains("fn process(input: Vec<u8>)"),
            "original function signature must be preserved, got: {result}"
        );
        assert!(
            result.contains("/// Process input."),
            "insert_before text must be present, got: {result}"
        );
    }

    #[test]
    fn replace_context_disambiguates_multiple_exact_matches() {
        // #1244: before_context/after_context should disambiguate when
        // the old text matches multiple times.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.ini");
        std::fs::write(
            &file,
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        )
        .unwrap();

        // Use after_context to target the [database] occurrence (port = 5432 follows it).
        let op = Operation::Replace {
            path: Some("config.ini".into()),
            glob: None,
            regex: false,
            old: "host = localhost".into(),
            new_text: Some("host = db.primary".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: Some("port = 5432".into()),
            if_exists: false,
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1, "should replace exactly one occurrence");
        let result = &f.pending[&file].1;
        assert!(
            result.contains("[database]\nhost = db.primary"),
            "database section should be updated: {result}"
        );
        assert!(
            result.contains("[cache]\nhost = localhost"),
            "cache section should be unchanged: {result}"
        );
    }

    #[test]
    fn replace_before_context_disambiguates() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.ini");
        std::fs::write(
            &file,
            "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        )
        .unwrap();

        let op = Operation::Replace {
            path: Some("config.ini".into()),
            glob: None,
            regex: false,
            old: "host = localhost".into(),
            new_text: Some("host = db.internal".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: Some("[database]".into()),
            after_context: None,
            if_exists: false,
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1);
        let result = &f.pending[&file].1;
        assert!(
            result.contains("[database]\nhost = db.internal"),
            "database section should be updated: {result}"
        );
        assert!(
            result.contains("[cache]\nhost = localhost"),
            "cache section should be unchanged: {result}"
        );
    }

    #[test]
    fn replace_glob_skips_binary_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("text.txt"), "hello world").unwrap();
        std::fs::write(dir.path().join("binary.dat"), b"hello\x00world").unwrap();

        let op = Operation::Replace {
            path: None,
            glob: Some("*".into()),
            regex: false,
            old: "hello".into(),
            new_text: Some("goodbye".into()),
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
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let count = execute_replace_op(&op, &mut tx).unwrap();
        drop(tx);
        assert_eq!(count, 1); // only text.txt matched
        assert_eq!(f.pending[&dir.path().join("text.txt")].1, "goodbye world");
        assert!(
            !f.pending.contains_key(&dir.path().join("binary.dat")),
            "binary file should be skipped, not loaded into pending"
        );
    }

    #[test]
    fn replace_range_without_whole_line_is_rejected() {
        // Regression: range was silently ignored when whole_line was false.
        // Now the tx engine validates this, matching the CLI behavior.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

        let op = Operation::Replace {
            path: Some("test.txt".into()),
            glob: None,
            regex: false,
            old: "line".into(),
            new_text: Some("LINE".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: Some("1:2".into()),
            before_context: None,
            after_context: None,
            if_exists: false,
            unique: false,
        };

        let mut f = TxStateFixture::new();
        let mut tx = f.state(dir.path());
        let err = execute_replace_op(&op, &mut tx).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line"),
            "should reject range without whole_line: {err}"
        );
    }
}
