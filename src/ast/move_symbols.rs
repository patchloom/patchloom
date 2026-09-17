//! AST-aware symbol movement between files.

use super::Language;
use super::symbols::{
    check_no_overlapping_spans, extract_symbol_text, extract_symbols_or_timeout, find_symbol,
    full_symbol_span,
};

/// Position to insert symbols in the target file.
#[derive(Debug, Clone)]
pub enum MovePosition {
    End,
    Start,
    After(String),
    Before(String),
}

/// Result of a move operation.
#[derive(Debug)]
pub struct MoveResult {
    pub source_content: String,
    pub target_content: String,
    pub symbols_moved: usize,
}

/// Parse a position string into a MovePosition.
///
/// Unknown tokens fail closed so agents inventing `top` / `first` do not
/// silently append at end.
pub fn parse_position(s: Option<&str>) -> anyhow::Result<MovePosition> {
    match s {
        None | Some("") | Some("end") => Ok(MovePosition::End),
        Some("start") => Ok(MovePosition::Start),
        Some(raw) if let Some(name) = raw.strip_prefix("after:") => {
            if name.is_empty() {
                return Err(crate::exit::InvalidInputError {
                    msg: "ast.move position after: requires a symbol name".into(),
                }
                .into());
            }
            Ok(MovePosition::After(name.to_string()))
        }
        Some(raw) if let Some(name) = raw.strip_prefix("before:") => {
            if name.is_empty() {
                return Err(crate::exit::InvalidInputError {
                    msg: "ast.move position before: requires a symbol name".into(),
                }
                .into());
            }
            Ok(MovePosition::Before(name.to_string()))
        }
        Some(other) => Err(crate::exit::InvalidInputError {
            msg: format!(
                "unknown ast.move position {other:?}; use start, end, after:NAME, or before:NAME"
            ),
        }
        .into()),
    }
}

/// Move named symbols from source to target file.
pub fn move_symbols(
    source: &str,
    target: &str,
    symbols: &[String],
    position: MovePosition,
    lang: Language,
) -> anyhow::Result<MoveResult> {
    if symbols.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "ast move symbols must not be empty".into(),
        }));
    }

    let src_symbols = extract_symbols_or_timeout(source, lang)?;
    move_symbols_from_symbols(source, target, &src_symbols, symbols, position, lang)
}

/// Like [`move_symbols`] using a pre-extracted source symbol list (tx tree cache).
pub(crate) fn move_symbols_from_symbols(
    source: &str,
    target: &str,
    src_symbols: &[super::symbols::SymbolDef],
    symbols: &[String],
    position: MovePosition,
    lang: Language,
) -> anyhow::Result<MoveResult> {
    if symbols.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "ast move symbols must not be empty".into(),
        }));
    }

    let eol = crate::write::detect_eol(source);
    let lines: Vec<&str> = crate::ops::file::text_lines(source).collect();

    // Collect symbols to move
    let mut to_move: Vec<(usize, usize, String)> = Vec::new(); // (start_0, end_0, text)
    let mut missing: Vec<&str> = Vec::new();

    for name in symbols {
        if let Some(sym) = find_symbol(src_symbols, name) {
            let (full_start, full_end) = full_symbol_span(source, sym, lang);
            let text = extract_symbol_text(source, sym, lang).to_string();
            to_move.push((
                full_start.saturating_sub(1),
                full_end.min(lines.len()),
                text,
            ));
        } else {
            missing.push(name);
        }
    }

    if !missing.is_empty() {
        return Err(anyhow::Error::new(crate::exit::NoMatchError {
            msg: format!("symbol(s) not found in source: {}", missing.join(", ")),
        }));
    }

    // symbols non-empty and no missing names implies to_move is non-empty.
    if to_move.is_empty() {
        return Ok(MoveResult {
            source_content: source.to_string(),
            target_content: target.to_string(),
            symbols_moved: 0,
        });
    }

    // Detect overlapping spans that would corrupt content during removal
    let spans: Vec<(usize, usize)> = to_move.iter().map(|(s, e, _)| (*s, *e)).collect();
    let span_names: Vec<&str> = to_move
        .iter()
        .map(|(s, _, _)| {
            src_symbols
                .iter()
                .find(|sym| {
                    let (fs, _) = full_symbol_span(source, sym, lang);
                    fs.saturating_sub(1) == *s
                })
                .map(|sym| sym.name.as_str())
                .unwrap_or("?")
        })
        .collect();
    check_no_overlapping_spans(&spans, &span_names)?;

    let symbols_moved = to_move.len();

    // Sort by start position (ascending) for ordered insertion into target
    to_move.sort_by_key(|(start, _, _)| *start);
    let ordered_texts: Vec<String> = to_move.iter().map(|(_, _, t)| t.clone()).collect();

    // Remove from source in reverse order
    let mut src_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut sorted_desc = to_move.clone();
    sorted_desc.sort_by_key(|b| std::cmp::Reverse(b.0));
    for (start_0, end_0, _) in &sorted_desc {
        let mut remove_end = *end_0;
        // Also remove trailing blank line if present
        if remove_end < src_lines.len() && src_lines[remove_end].trim().is_empty() {
            remove_end += 1;
        }
        let drain_end = remove_end.min(src_lines.len());
        src_lines.drain(*start_0..drain_end);
    }

    let mut source_result = src_lines.join(eol);
    if source.ends_with('\n') && !source_result.ends_with('\n') {
        source_result.push_str(eol);
    }

    // Build text to insert into target
    let mut insert_text = String::new();
    for (i, text) in ordered_texts.iter().enumerate() {
        if i > 0 {
            insert_text.push_str(eol);
        }
        insert_text.push_str(text.trim_end_matches(['\r', '\n']));
        insert_text.push_str(eol);
    }

    // Insert into target (use target's EOL style, not source's)
    let target_eol = crate::write::detect_eol(target);
    let target_result = insert_into_target(target, &insert_text, &position, lang, target_eol)?;

    Ok(MoveResult {
        source_content: source_result,
        target_content: target_result,
        symbols_moved,
    })
}

/// Count leading preamble lines that `position=start` must stay after (#2530).
fn preamble_line_count(source: &str, lang: Language) -> usize {
    let lines: Vec<&str> = crate::ops::file::text_lines(source).collect();
    let mut i = 0usize;
    if lines.first().is_some_and(|l| l.starts_with("#!")) {
        i += 1;
    }
    match lang {
        Language::Python => {
            if i < lines.len() && is_python_encoding_comment(lines[i]) {
                i += 1;
            }
            i = skip_blank_lines(&lines, i);
            if i < lines.len()
                && let Some(after_doc) = skip_python_string_literal(&lines, i)
            {
                i = skip_blank_lines(&lines, after_doc);
            }
            while i < lines.len() && is_python_future_import(lines[i]) {
                i += 1;
                if i < lines.len() && lines[i].trim().is_empty() {
                    let next = i + 1;
                    if next < lines.len() && is_python_future_import(lines[next]) {
                        i = next;
                    }
                }
            }
            i
        }
        Language::Rust => {
            while i < lines.len() {
                let t = lines[i].trim();
                if t.is_empty() {
                    if i + 1 < lines.len() && is_rust_inner_preamble(lines[i + 1]) {
                        i += 1;
                        continue;
                    }
                    break;
                }
                if t.starts_with("//!") {
                    i += 1;
                    continue;
                }
                if t.starts_with("#![") {
                    i = skip_rust_crate_attr(&lines, i);
                    continue;
                }
                break;
            }
            i
        }
        _ => i,
    }
}

fn skip_blank_lines(lines: &[&str], mut i: usize) -> usize {
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    i
}

fn is_python_encoding_comment(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('#') && t.to_ascii_lowercase().contains("coding")
}

fn is_python_future_import(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("from __future__ import")
}

fn skip_python_string_literal(lines: &[&str], start: usize) -> Option<usize> {
    let t = lines[start].trim_start();
    let quote = if t.starts_with("\"\"\"") {
        "\"\"\""
    } else if t.starts_with("'''") {
        "'''"
    } else if t.starts_with('"') {
        "\""
    } else if t.starts_with('\'') {
        "'"
    } else {
        return None;
    };
    if quote.len() == 1 {
        // Single-line module docstring only.
        if t[quote.len()..].contains(quote) {
            return Some(start + 1);
        }
        return None;
    }
    if t[quote.len()..].contains(quote) {
        return Some(start + 1);
    }
    let mut i = start + 1;
    while i < lines.len() {
        if lines[i].contains(quote) {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn is_rust_inner_preamble(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("//!") || t.starts_with("#![")
}

fn skip_rust_crate_attr(lines: &[&str], start: usize) -> usize {
    let t = lines[start].trim();
    if t.ends_with(']') {
        return start + 1;
    }
    let mut i = start + 1;
    while i < lines.len() {
        if lines[i].contains(']') {
            return i + 1;
        }
        i += 1;
    }
    start + 1
}

fn insert_into_target(
    target: &str,
    insert_text: &str,
    position: &MovePosition,
    lang: Language,
    eol: &str,
) -> anyhow::Result<String> {
    if target.is_empty() {
        return Ok(insert_text.to_string());
    }

    let lines: Vec<&str> = crate::ops::file::text_lines(target).collect();

    match position {
        MovePosition::End => {
            let mut result = target.to_string();
            if !result.ends_with('\n') {
                result.push_str(eol);
            }
            result.push_str(eol);
            result.push_str(insert_text);
            Ok(result)
        }
        MovePosition::Start => {
            let skip = preamble_line_count(target, lang);
            if skip == 0 {
                let mut result = insert_text.to_string();
                result.push_str(eol);
                result.push_str(target);
                return Ok(result);
            }
            let mut result = String::new();
            for line in &lines[..skip] {
                result.push_str(line);
                result.push_str(eol);
            }
            if skip > 0 && !lines[skip - 1].trim().is_empty() {
                result.push_str(eol);
            }
            result.push_str(insert_text);
            if !insert_text.ends_with('\n') {
                result.push_str(eol);
            }
            if skip < lines.len() && !lines[skip].trim().is_empty() {
                result.push_str(eol);
            }
            for line in &lines[skip..] {
                result.push_str(line);
                result.push_str(eol);
            }
            if !target.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - eol.len());
            }
            Ok(result)
        }
        MovePosition::After(sym_name) => {
            let symbols = extract_symbols_or_timeout(target, lang)?;
            let sym = find_symbol(&symbols, sym_name).ok_or_else(|| {
                anyhow::Error::new(crate::exit::NoMatchError {
                    msg: format!("symbol '{sym_name}' not found in target"),
                })
            })?;
            let end_0 = sym.end_line.min(lines.len());
            let mut result = String::new();
            for line in &lines[..end_0] {
                result.push_str(line);
                result.push_str(eol);
            }
            result.push_str(eol);
            result.push_str(insert_text);
            for line in &lines[end_0..] {
                result.push_str(line);
                result.push_str(eol);
            }
            if !target.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - eol.len());
            }
            Ok(result)
        }
        MovePosition::Before(sym_name) => {
            let symbols = extract_symbols_or_timeout(target, lang)?;
            let sym = find_symbol(&symbols, sym_name).ok_or_else(|| {
                anyhow::Error::new(crate::exit::NoMatchError {
                    msg: format!("symbol '{sym_name}' not found in target"),
                })
            })?;
            let (full_start, _) = full_symbol_span(target, sym, lang);
            let start_0 = full_start.saturating_sub(1);
            let mut result = String::new();
            for line in &lines[..start_0] {
                result.push_str(line);
                result.push_str(eol);
            }
            result.push_str(insert_text);
            result.push_str(eol);
            for line in &lines[start_0..] {
                result.push_str(line);
                result.push_str(eol);
            }
            if !target.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - eol.len());
            }
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_basic() {
        let source = "fn foo() {}\n\nfn bar() {}\n";
        let target = "fn existing() {}\n";
        let result = move_symbols(
            source,
            target,
            &["foo".into()],
            MovePosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(!result.source_content.contains("fn foo"));
        assert!(result.source_content.contains("fn bar"));
        assert!(result.target_content.contains("fn foo"));
        assert!(result.target_content.contains("fn existing"));
        assert_eq!(result.symbols_moved, 1);
    }

    #[test]
    fn move_to_empty_file() {
        let source = "fn foo() {\n    42\n}\n\nfn bar() {}\n";
        let result = move_symbols(
            source,
            "",
            &["foo".into()],
            MovePosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(!result.source_content.contains("fn foo"));
        assert!(result.target_content.contains("fn foo()"));
        assert!(
            result.target_content.contains("    42"),
            "target should contain the function body: {}",
            result.target_content
        );
    }

    #[test]
    fn move_multiple_symbols() {
        let source = "fn alpha() {}\n\nfn beta() {}\n\nfn gamma() {}\n";
        let result = move_symbols(
            source,
            "",
            &["alpha".into(), "gamma".into()],
            MovePosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(!result.source_content.contains("fn alpha"));
        assert!(result.source_content.contains("fn beta"));
        assert!(!result.source_content.contains("fn gamma"));
        assert!(result.target_content.contains("fn alpha"));
        assert!(result.target_content.contains("fn gamma"));
        assert_eq!(result.symbols_moved, 2);
    }

    #[test]
    fn move_symbol_not_found() {
        let source = "fn foo() {}\n";
        let result = move_symbols(
            source,
            "",
            &["nonexistent".into()],
            MovePosition::End,
            Language::Rust,
        );
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
    }

    #[test]
    fn move_preserves_attributes() {
        let source = "#[test]\n#[cfg(unix)]\nfn foo() {}\n\nfn bar() {}\n";
        let result = move_symbols(
            source,
            "",
            &["foo".into()],
            MovePosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(result.target_content.contains("#[test]"));
        assert!(result.target_content.contains("#[cfg(unix)]"));
        assert!(result.target_content.contains("fn foo()"));
    }

    /// #2530: start is after `from __future__`, not at byte 0.
    #[test]
    fn move_position_start_after_python_future() {
        let source = "def moved():\n    return 1\n";
        let target = "from __future__ import annotations\n\ndef existing():\n    pass\n";
        let result = move_symbols(
            source,
            target,
            &["moved".into()],
            MovePosition::Start,
            Language::Python,
        )
        .unwrap();
        assert_eq!(
            result.target_content,
            "from __future__ import annotations\n\ndef moved():\n    return 1\n\ndef existing():\n    pass\n"
        );
        let future_pos = result.target_content.find("from __future__").unwrap();
        let moved_pos = result.target_content.find("def moved").unwrap();
        assert!(
            future_pos < moved_pos,
            "moved symbol must follow the future import: {}",
            result.target_content
        );
    }

    /// #2530: start is after `//!` inner docs and `#![...]` crate attrs.
    #[test]
    fn move_position_start_after_rust_inner_docs_and_crate_attr() {
        let source = "fn moved() {}\n";
        let target = "//! crate docs\n#![deny(unsafe_code)]\n\nfn existing() {}\n";
        let result = move_symbols(
            source,
            target,
            &["moved".into()],
            MovePosition::Start,
            Language::Rust,
        )
        .unwrap();
        assert_eq!(
            result.target_content,
            "//! crate docs\n#![deny(unsafe_code)]\n\nfn moved() {}\n\nfn existing() {}\n"
        );
        let doc_pos = result.target_content.find("//! crate docs").unwrap();
        let attr_pos = result.target_content.find("#![deny").unwrap();
        let moved_pos = result.target_content.find("fn moved").unwrap();
        assert!(doc_pos < moved_pos && attr_pos < moved_pos);
    }

    #[test]
    fn move_position_start() {
        let source = "fn new_fn() {}\n";
        let target = "fn existing() {}\n";
        let result = move_symbols(
            source,
            target,
            &["new_fn".into()],
            MovePosition::Start,
            Language::Rust,
        )
        .unwrap();
        let new_pos = result.target_content.find("fn new_fn").unwrap();
        let existing_pos = result.target_content.find("fn existing").unwrap();
        assert!(new_pos < existing_pos);
    }

    #[test]
    fn move_position_after() {
        let source = "fn moved() {}\n";
        let target = "fn first() {}\n\nfn last() {}\n";
        let result = move_symbols(
            source,
            target,
            &["moved".into()],
            MovePosition::After("first".into()),
            Language::Rust,
        )
        .unwrap();
        let first_pos = result.target_content.find("fn first").unwrap();
        let moved_pos = result.target_content.find("fn moved").unwrap();
        let last_pos = result.target_content.find("fn last").unwrap();
        assert!(first_pos < moved_pos);
        assert!(moved_pos < last_pos);
    }

    #[test]
    fn move_position_before() {
        let source = "fn moved() {}\n";
        let target = "fn first() {}\n\nfn last() {}\n";
        let result = move_symbols(
            source,
            target,
            &["moved".into()],
            MovePosition::Before("last".into()),
            Language::Rust,
        )
        .unwrap();
        let first_pos = result.target_content.find("fn first").unwrap();
        let moved_pos = result.target_content.find("fn moved").unwrap();
        let last_pos = result.target_content.find("fn last").unwrap();
        assert!(first_pos < moved_pos);
        assert!(moved_pos < last_pos);
    }

    #[test]
    fn move_preserves_crlf_line_endings() {
        let source = "fn foo() {}\r\n\r\nfn bar() {}\r\n";
        let target = "fn existing() {}\r\n";
        let result = move_symbols(
            source,
            target,
            &["foo".into()],
            MovePosition::End,
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
        let target_bytes = result.target_content.as_bytes();
        for (i, &b) in target_bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && target_bytes[i - 1] == b'\r',
                    "bare LF in target at byte {i}: {:?}",
                    result.target_content
                );
            }
        }
    }

    #[test]
    fn move_adjacent_decorated_symbols_succeeds() {
        // Simulate two adjacent decorated functions where doc comments could
        // cause span overlap. In practice the spans from tree-sitter should
        // not overlap for well-formed code, but if full_symbol_span extends
        // backwards into the prior symbol's range, we should error instead
        // of silently corrupting content.
        //
        // Create a scenario with a shared doc-comment block that's ambiguous:
        // The test verifies that non-overlapping adjacent symbols work fine.
        let source = "/// Doc for alpha\nfn alpha() {}\n\n/// Doc for beta\nfn beta() {}\n";
        let result = move_symbols(
            source,
            "",
            &["alpha".into(), "beta".into()],
            MovePosition::End,
            Language::Rust,
        );
        // These two should NOT overlap (they have a blank line separator)
        assert_eq!(result.expect("move should succeed").symbols_moved, 2);
    }

    #[test]
    fn move_empty_symbols_is_invalid_input() {
        let source = "fn foo() {}\n";
        let err = move_symbols(source, "", &[], MovePosition::End, Language::Rust)
            .expect_err("empty symbols vec must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty symbols must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn parse_position_variants() {
        assert!(matches!(parse_position(None).unwrap(), MovePosition::End));
        assert!(matches!(
            parse_position(Some("end")).unwrap(),
            MovePosition::End
        ));
        assert!(matches!(
            parse_position(Some("start")).unwrap(),
            MovePosition::Start
        ));
        assert!(matches!(
            parse_position(Some("after:foo")).unwrap(),
            MovePosition::After(ref s) if s == "foo"
        ));
        assert!(matches!(
            parse_position(Some("before:bar")).unwrap(),
            MovePosition::Before(ref s) if s == "bar"
        ));
        let err = parse_position(Some("top")).unwrap_err();
        assert!(
            err.to_string().contains("unknown"),
            "top must fail closed: {err}"
        );
    }
}
