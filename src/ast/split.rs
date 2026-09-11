//! AST-aware file splitting: distribute symbols across multiple target files.

use super::Language;
use super::symbols::{
    check_no_overlapping_spans, extract_symbol_text, extract_symbols_or_timeout, full_symbol_span,
};

/// Target specification for a split operation.
#[derive(Debug)]
pub struct SplitTarget {
    pub path: String,
    pub symbols: Vec<String>,
    pub prepend: Option<String>,
}

/// Result of a split operation.
#[derive(Debug)]
pub struct SplitResult {
    /// The source file content after splitting.
    pub source_content: String,
    /// Target file contents: (path, content) pairs.
    pub targets: Vec<(String, String)>,
    /// Total number of symbols distributed to targets.
    pub symbols_distributed: usize,
}

/// Split a file by distributing symbols across multiple target files.
///
/// When `require_exhaustive` is true, every top-level symbol must be accounted
/// for in either `targets` or `keep_in_source`.
pub fn split_file(
    source: &str,
    targets: &[SplitTarget],
    keep_in_source: &[String],
    source_suffix: Option<&str>,
    source_prefix: Option<&str>,
    require_exhaustive: bool,
    lang: Language,
) -> anyhow::Result<SplitResult> {
    for target in targets {
        if target.symbols.is_empty() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "ast split target symbols must not be empty".into(),
            }));
        }
        for name in &target.symbols {
            if name.trim().is_empty() {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "ast split symbol must not be empty".into(),
                }));
            }
        }
        // Empty/whitespace prepend inserts junk blanks vs omitting the field.
        // Newline-only remains insert-a-blank-line.
        if let Some(pre) = &target.prepend
            && pre.trim().is_empty()
            && !pre.contains('\n')
            && !pre.contains('\r')
        {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "ast split target prepend must not be empty".into(),
            }));
        }
    }

    // Whitespace-only affix (not empty "") inserts junk; empty is identity with omitted.
    // Newline-only remains insert-a-blank-line.
    for (label, affix) in [
        ("source_suffix", source_suffix),
        ("source_prefix", source_prefix),
    ] {
        if let Some(s) = affix
            && !s.is_empty()
            && s.trim().is_empty()
            && !s.contains('\n')
            && !s.contains('\r')
        {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("ast split {label} must not be empty"),
            }));
        }
    }

    let eol = crate::write::detect_eol(source);
    let all_symbols = extract_symbols_or_timeout(source, lang)?;
    let lines: Vec<&str> = crate::ops::file::text_lines(source).collect();

    // Build a map of symbol name -> target index
    let mut sym_to_target: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (idx, target) in targets.iter().enumerate() {
        for name in &target.symbols {
            if let Some(prev_idx) = sym_to_target.insert(name.as_str(), idx) {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!(
                        "symbol '{}' assigned to multiple targets: '{}' and '{}'",
                        name, targets[prev_idx].path, target.path
                    ),
                }));
            }
        }
    }

    // Validate exhaustiveness
    if require_exhaustive {
        let mut unaccounted: Vec<&str> = Vec::new();
        for sym in &all_symbols {
            if !sym_to_target.contains_key(sym.name.as_str())
                && !keep_in_source.iter().any(|k| k == &sym.name)
            {
                unaccounted.push(&sym.name);
            }
        }
        if !unaccounted.is_empty() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!(
                    "unaccounted symbols (use keep_in_source or add to a target): {}",
                    unaccounted.join(", ")
                ),
            }));
        }
    }

    // Collect spans for symbols being distributed
    let mut spans_to_remove: Vec<(usize, usize)> = Vec::new(); // (start_0, end_0)
    let mut target_contents: Vec<String> = vec![String::new(); targets.len()];

    // Build target contents
    for sym in &all_symbols {
        if let Some(&target_idx) = sym_to_target.get(sym.name.as_str()) {
            let text = extract_symbol_text(source, sym, lang);
            if !target_contents[target_idx].is_empty() {
                target_contents[target_idx].push_str(eol);
            }
            target_contents[target_idx].push_str(text.trim_end_matches(['\r', '\n']));
            target_contents[target_idx].push_str(eol);

            let (full_start, full_end) = full_symbol_span(source, sym, lang);
            spans_to_remove.push((full_start.saturating_sub(1), full_end.min(lines.len())));
        }
    }

    let symbols_distributed = spans_to_remove.len();

    // Detect overlapping spans that would corrupt content during removal
    {
        let span_names: Vec<&str> = all_symbols
            .iter()
            .filter(|s| sym_to_target.contains_key(s.name.as_str()))
            .map(|s| s.name.as_str())
            .collect();
        check_no_overlapping_spans(&spans_to_remove, &span_names)?;
    }

    // Remove distributed symbols from source (in reverse order)
    let mut src_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    spans_to_remove.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (start_0, end_0) in &spans_to_remove {
        let mut remove_end = *end_0;
        if remove_end < src_lines.len() && src_lines[remove_end].trim().is_empty() {
            remove_end += 1;
        }
        src_lines.drain(*start_0..remove_end.min(src_lines.len()));
    }

    // Apply source_prefix and source_suffix
    if let Some(prefix) = source_prefix {
        let prefix_lines: Vec<String> = prefix.lines().map(String::from).collect();
        for (i, line) in prefix_lines.iter().enumerate() {
            src_lines.insert(i, line.clone());
        }
    }
    if let Some(suffix) = source_suffix {
        if src_lines.last().is_some_and(|l| !l.trim().is_empty()) {
            src_lines.push(String::new());
        }
        for line in suffix.lines() {
            src_lines.push(line.to_string());
        }
    }

    let mut source_content = src_lines.join(eol);
    if source.ends_with('\n') && !source_content.ends_with('\n') {
        source_content.push_str(eol);
    }

    // Build final target contents with prepend
    let result_targets: Vec<(String, String)> = targets
        .iter()
        .zip(target_contents.iter())
        .map(|(spec, content)| {
            let mut final_content = String::new();
            if let Some(pre) = &spec.prepend {
                final_content.push_str(pre);
                if !pre.ends_with('\n') {
                    final_content.push_str(eol);
                }
                final_content.push_str(eol);
            }
            final_content.push_str(content);
            if !final_content.ends_with('\n') {
                final_content.push_str(eol);
            }
            (spec.path.clone(), final_content)
        })
        .collect();

    Ok(SplitResult {
        source_content,
        targets: result_targets,
        symbols_distributed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic() {
        let source = "fn alpha() {}\n\nfn beta() {}\n\nfn gamma() {}\n";
        let targets = vec![
            SplitTarget {
                path: "a.rs".into(),
                symbols: vec!["alpha".into()],
                prepend: None,
            },
            SplitTarget {
                path: "b.rs".into(),
                symbols: vec!["beta".into()],
                prepend: None,
            },
        ];
        let result = split_file(
            source,
            &targets,
            &["gamma".into()],
            None,
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        assert!(result.source_content.contains("fn gamma"));
        assert!(!result.source_content.contains("fn alpha"));
        assert!(!result.source_content.contains("fn beta"));
        assert_eq!(result.targets.len(), 2);
        assert!(result.targets[0].1.contains("fn alpha"));
        assert!(result.targets[1].1.contains("fn beta"));
        assert_eq!(result.symbols_distributed, 2);
    }

    #[test]
    fn split_exhaustive_check_fails() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let result = split_file(source, &targets, &[], None, None, true, Language::Rust);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(result.unwrap_err().to_string().contains("beta"));
    }

    #[test]
    fn split_non_exhaustive_ok() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        split_file(source, &targets, &[], None, None, false, Language::Rust).unwrap();
    }

    #[test]
    fn split_with_prepend_and_suffix() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: Some("use super::*;".into()),
        }];
        let result = split_file(
            source,
            &targets,
            &["beta".into()],
            Some("mod a;"),
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        assert!(result.targets[0].1.contains("use super::*;"));
        assert!(result.targets[0].1.contains("fn alpha"));
        assert!(result.source_content.contains("mod a;"));
    }

    #[test]
    fn split_keep_in_source() {
        let source = "struct Config {\n    x: i32,\n}\n\nfn helper() {}\n\nfn process() {}\n";
        let targets = vec![SplitTarget {
            path: "helpers.rs".into(),
            symbols: vec!["helper".into()],
            prepend: None,
        }];
        let result = split_file(
            source,
            &targets,
            &["Config".into(), "process".into()],
            None,
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        assert!(result.source_content.contains("struct Config"));
        assert!(result.source_content.contains("fn process"));
        assert!(!result.source_content.contains("fn helper"));
    }

    #[test]
    fn split_with_source_prefix() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let result = split_file(
            source,
            &targets,
            &["beta".into()],
            None,
            Some("// This file was split."),
            true,
            Language::Rust,
        )
        .unwrap();
        assert!(result.source_content.starts_with("// This file was split."));
        assert!(result.source_content.contains("fn beta"));
    }

    #[test]
    fn split_preserves_attributes() {
        let source = "/// Alpha doc.\n#[test]\nfn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let result = split_file(
            source,
            &targets,
            &["beta".into()],
            None,
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        assert!(result.targets[0].1.contains("/// Alpha doc."));
        assert!(result.targets[0].1.contains("#[test]"));
    }

    #[test]
    fn split_preserves_crlf_line_endings() {
        let source = "fn alpha() {}\r\n\r\nfn beta() {}\r\n\r\nfn gamma() {}\r\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let result = split_file(
            source,
            &targets,
            &["beta".into(), "gamma".into()],
            None,
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        // Source content should preserve CRLF
        let bytes = result.source_content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "bare LF in source at byte {i}: {:?}",
                    result.source_content
                );
            }
        }
        // Target content should preserve CRLF
        let target_bytes = result.targets[0].1.as_bytes();
        for (i, &b) in target_bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && target_bytes[i - 1] == b'\r',
                    "bare LF in target at byte {i}: {:?}",
                    result.targets[0].1
                );
            }
        }
    }

    #[test]
    fn split_non_overlapping_adjacent_symbols() {
        // Adjacent symbols with proper separation should split fine.
        let source = "/// Doc A\nfn alpha() {}\n\n/// Doc B\nfn beta() {}\n\nfn gamma() {}\n";
        let targets = vec![
            SplitTarget {
                path: "a.rs".into(),
                symbols: vec!["alpha".into()],
                prepend: None,
            },
            SplitTarget {
                path: "b.rs".into(),
                symbols: vec!["beta".into()],
                prepend: None,
            },
        ];
        let result = split_file(
            source,
            &targets,
            &["gamma".into()],
            None,
            None,
            true,
            Language::Rust,
        );
        let r = result.expect("split should succeed");
        assert_eq!(r.symbols_distributed, 2);
        assert!(r.targets[0].1.contains("/// Doc A"));
        assert!(r.targets[1].1.contains("/// Doc B"));
    }

    #[test]
    fn split_duplicate_symbol_across_targets_errors() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![
            SplitTarget {
                path: "a.rs".into(),
                symbols: vec!["alpha".into()],
                prepend: None,
            },
            SplitTarget {
                path: "b.rs".into(),
                symbols: vec!["alpha".into()],
                prepend: None,
            },
        ];
        let result = split_file(source, &targets, &[], None, None, false, Language::Rust);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("alpha"));
        assert!(err.contains("a.rs"));
        assert!(err.contains("b.rs"));
    }

    fn nested_rust_source(depth: usize) -> String {
        let mut source = String::from("fn main() { let x = ");
        source.push_str(&"(".repeat(depth));
        source.push('1');
        source.push_str(&")".repeat(depth));
        source.push_str("; }\n");
        source
    }

    #[test]
    fn split_timeout_is_parse_timeout() {
        let source = nested_rust_source(80_000);
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["main".into()],
            prepend: Some("// dest".into()),
        }];
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let err = split_file(&source, &targets, &[], None, None, false, Language::Rust)
            .expect_err("deadline must be Err, not empty dest writes");
        assert!(
            crate::exit::is_parse_timeout(&err),
            "timeout must not write empty dests: {err}"
        );
    }

    #[test]
    fn split_empty_symbol_is_invalid_input() {
        let source = "fn alpha() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec![String::new()],
            prepend: None,
        }];
        let err = split_file(source, &targets, &[], None, None, false, Language::Rust)
            .expect_err("empty split symbol must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty symbol must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn split_whitespace_symbol_is_invalid_input() {
        let source = "fn alpha() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["   ".into()],
            prepend: None,
        }];
        let err = split_file(source, &targets, &[], None, None, false, Language::Rust)
            .expect_err("whitespace-only split symbol must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "whitespace-only symbol must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn split_empty_symbols_vec_is_invalid_input() {
        let source = "fn alpha() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec![],
            prepend: None,
        }];
        let err = split_file(source, &targets, &[], None, None, false, Language::Rust)
            .expect_err("empty split symbols vec must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty symbols vec must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn split_whitespace_source_suffix_is_invalid_input() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let err = split_file(
            source,
            &targets,
            &["beta".into()],
            Some("   "),
            None,
            true,
            Language::Rust,
        )
        .expect_err("whitespace-only source_suffix must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "whitespace suffix must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn split_whitespace_source_prefix_is_invalid_input() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let err = split_file(
            source,
            &targets,
            &["beta".into()],
            None,
            Some("   "),
            true,
            Language::Rust,
        )
        .expect_err("whitespace-only source_prefix must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "whitespace prefix must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn split_empty_source_suffix_is_identity() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let omitted = split_file(
            source,
            &targets,
            &["beta".into()],
            None,
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        let empty = split_file(
            source,
            &targets,
            &["beta".into()],
            Some(""),
            None,
            true,
            Language::Rust,
        )
        .unwrap();
        assert_eq!(
            empty.source_content, omitted.source_content,
            "empty source_suffix must match omitted"
        );
    }

    #[test]
    fn split_newline_source_suffix_is_ok() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: None,
        }];
        let result = split_file(
            source,
            &targets,
            &["beta".into()],
            Some("\n"),
            None,
            true,
            Language::Rust,
        )
        .expect("newline-only source_suffix is insert-a-blank-line");
        assert!(
            result.source_content.contains("fn beta"),
            "beta must remain in source: {:?}",
            result.source_content
        );
        assert!(
            result.source_content.ends_with("\n\n") || result.source_content.contains("\n\n"),
            "newline suffix should insert blank line: {:?}",
            result.source_content
        );
    }

    #[test]
    fn split_empty_target_prepend_is_invalid_input() {
        let source = "fn alpha() {}\n";
        for pre in ["", "   "] {
            let targets = vec![SplitTarget {
                path: "a.rs".into(),
                symbols: vec!["alpha".into()],
                prepend: Some(pre.into()),
            }];
            let err = split_file(source, &targets, &[], None, None, false, Language::Rust)
                .expect_err("empty/whitespace target prepend must be invalid_input");
            assert!(
                crate::exit::is_invalid_input(&err),
                "prepend {pre:?} must classify as invalid_input: {err}"
            );
            assert!(
                err.to_string().contains("must not be empty"),
                "message must say must not be empty: {err}"
            );
        }
    }

    #[test]
    fn split_newline_target_prepend_is_ok() {
        let source = "fn alpha() {}\n";
        let targets = vec![SplitTarget {
            path: "a.rs".into(),
            symbols: vec!["alpha".into()],
            prepend: Some("\n".into()),
        }];
        let result = split_file(source, &targets, &[], None, None, false, Language::Rust)
            .expect("newline-only target prepend is insert-a-blank-line");
        assert!(
            result.targets[0].1.starts_with('\n'),
            "newline prepend should insert blank line: {:?}",
            result.targets[0].1
        );
        assert!(result.targets[0].1.contains("fn alpha"));
    }
}
