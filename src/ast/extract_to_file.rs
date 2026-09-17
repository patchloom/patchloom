//! Extract a named symbol **into a separate source file** (`ast.extract_to_file`).
//!
//! Not to be confused with [`crate::ast::symbol_extract`] (tree-sitter visitors
//! that build the in-memory symbol list).

use super::Language;
use super::symbols::{
    SymbolKind, extract_symbol_text, find_symbol, full_symbol_span, try_extract_symbols,
};

/// Result of an extract-to-file operation.
#[derive(Debug)]
pub struct ExtractResult {
    /// The source file content after extraction.
    pub source_content: String,
    /// The target file content (extracted code).
    pub target_content: String,
    /// Number of lines extracted.
    pub extracted_lines: usize,
}

/// Extract a named symbol from source and produce content for a new file.
///
/// When `unwrap` is true and the symbol is a module, the module's body
/// content is extracted (un-indented by one level) without the `mod name { }` wrapper.
pub fn extract_to_file(
    source: &str,
    symbol: &str,
    replacement: Option<&str>,
    unwrap: bool,
    prepend: Option<&str>,
    lang: Language,
) -> anyhow::Result<ExtractResult> {
    // Empty/whitespace prepend inserts junk blanks vs omitting the field.
    // Newline-only remains insert-a-blank-line (same as wrap preamble).
    if let Some(pre) = prepend
        && pre.trim().is_empty()
        && !pre.contains('\n')
        && !pre.contains('\r')
    {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "ast extract prepend must not be empty".into(),
        }));
    }

    let eol = crate::write::detect_eol(source);
    let symbols = match try_extract_symbols(source, lang) {
        Ok(s) => s,
        Err(crate::ast::ParseFailure::DeadlineExceeded) => {
            return Err(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {lang}"),
            }
            .into());
        }
        Err(crate::ast::ParseFailure::NoGrammar) => {
            return Err(crate::exit::NoMatchError {
                msg: format!("symbol '{symbol}' not found"),
            }
            .into());
        }
    };
    let sym = find_symbol(&symbols, symbol).ok_or_else(|| {
        anyhow::Error::new(crate::exit::NoMatchError {
            msg: format!("symbol '{symbol}' not found"),
        })
    })?;

    let (full_start, full_end) = full_symbol_span(source, sym, lang);
    let lines: Vec<&str> = crate::ops::file::text_lines(source).collect();
    let start_0 = full_start.saturating_sub(1);
    let end_0 = full_end.min(lines.len());

    // Extract the symbol's content for the target file
    let target_body = if unwrap && matches!(sym.kind, SymbolKind::Module | SymbolKind::Class) {
        unwrap_container_body(
            source,
            lang,
            sym,
            &lines,
            sym.start_line.saturating_sub(1),
            end_0,
            eol,
        )?
    } else {
        let text = extract_symbol_text(source, sym, lang);
        text.to_string()
    };

    let extracted_lines = target_body.lines().count();

    // Build target content
    let mut target_content = String::new();
    if let Some(pre) = prepend {
        target_content.push_str(pre);
        if !pre.ends_with('\n') {
            target_content.push_str(eol);
        }
        target_content.push_str(eol);
    }
    target_content.push_str(&target_body);
    if !target_content.ends_with('\n') {
        target_content.push_str(eol);
    }

    // Build source content with the symbol removed or replaced
    let mut source_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut remove_end = end_0;
    // Also remove trailing blank line
    if remove_end < source_lines.len() && source_lines[remove_end].trim().is_empty() {
        remove_end += 1;
    }
    source_lines.drain(start_0..remove_end.min(source_lines.len()));

    if let Some(repl) = replacement {
        // Insert replacement text at the position
        let repl_lines: Vec<String> = repl.lines().map(String::from).collect();
        for (i, line) in repl_lines.iter().enumerate() {
            source_lines.insert(start_0 + i, line.clone());
        }
    }

    let mut source_content = source_lines.join(eol);
    if source.ends_with('\n') && !source_content.ends_with('\n') {
        source_content.push_str(eol);
    }

    Ok(ExtractResult {
        source_content,
        target_content,
        extracted_lines,
    })
}

/// Extract the body of a module/class, removing the wrapper and un-indenting.
///
/// Prefers the tree-sitter `body` node; falls back to `{`/`}` or Ruby `end`
/// pairing. `unwrap` with no locatable body is `invalid_input` (#2529).
fn unwrap_container_body(
    source: &str,
    lang: Language,
    sym: &super::symbols::SymbolDef,
    lines: &[&str],
    sym_start_0: usize,
    sym_end_0: usize,
    eol: &str,
) -> anyhow::Result<String> {
    if let Some(body) = unwrap_body_from_ast(source, lang, sym, eol) {
        return Ok(body);
    }
    if let Some(body) = unwrap_body_from_braces(lines, sym_start_0, sym_end_0, eol) {
        return Ok(body);
    }
    if let Some(body) = unwrap_body_from_end(lines, sym_start_0, sym_end_0, eol) {
        return Ok(body);
    }
    Err(anyhow::Error::new(crate::exit::InvalidInputError {
        msg: format!(
            "ast extract unwrap requested but no body found for '{}'",
            sym.name
        ),
    }))
}

fn unwrap_body_from_ast(
    source: &str,
    lang: Language,
    sym: &super::symbols::SymbolDef,
    eol: &str,
) -> Option<String> {
    let (tree, _) = super::parse_source(source, lang)?;
    let node = find_node_for_span(tree.root_node(), source, sym.start_line, sym.end_line)?;
    let body = node.child_by_field_name("body").or_else(|| {
        let kinds = [
            "declaration_list",
            "body_statement",
            "block",
            "class_body",
            "statement_block",
        ];
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| kinds.contains(&child.kind()))
    })?;
    let raw = body_inner_text(source, body);
    Some(unindent_body_text(raw, eol))
}

fn find_node_for_span<'a>(
    node: tree_sitter_lib::Node<'a>,
    source: &str,
    start_line: usize,
    end_line: usize,
) -> Option<tree_sitter_lib::Node<'a>> {
    let (ns, ne) = super::symbol_extract::node_source_lines(source, node);
    if ns == start_line && ne == end_line {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_node_for_span(child, source, start_line, end_line) {
            return Some(found);
        }
    }
    None
}

fn body_inner_text<'a>(source: &'a str, body: tree_sitter_lib::Node<'a>) -> &'a str {
    let text = &source[body.start_byte()..body.end_byte()];
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && let (Some(open), Some(close)) = (text.find('{'), text.rfind('}'))
        && close > open
    {
        return &text[open + 1..close];
    }
    text
}

fn unindent_body_text(raw: &str, eol: &str) -> String {
    let body_lines: Vec<&str> = crate::ops::file::text_lines(raw).collect();
    let min_indent = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| crate::write::indent_char_count(l))
        .min()
        .unwrap_or(0);
    let parts: Vec<String> = body_lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line[crate::write::indent_strip_offset(line, min_indent)..].to_string()
            }
        })
        .collect();
    // Drop leading/trailing blank lines introduced by brace inner text.
    let start = parts.iter().position(|l| !l.is_empty()).unwrap_or(0);
    let end = parts
        .iter()
        .rposition(|l| !l.is_empty())
        .map(|i| i + 1)
        .unwrap_or(parts.len());
    if start >= end {
        return String::new();
    }
    parts[start..end].join(eol)
}

fn unwrap_body_from_end(
    lines: &[&str],
    sym_start_0: usize,
    sym_end_0: usize,
    eol: &str,
) -> Option<String> {
    if sym_end_0 <= sym_start_0 + 1 {
        return None;
    }
    let last = lines
        .get(sym_end_0.saturating_sub(1))
        .copied()
        .unwrap_or("");
    let t = last.trim();
    let is_end =
        t == "end" || t.starts_with("end ") || t.starts_with("end;") || t.starts_with("end#");
    if !is_end {
        return None;
    }
    let body_lines = &lines[sym_start_0 + 1..sym_end_0 - 1];
    Some(unindent_body_text(&body_lines.join(eol), eol))
}

/// Extract the body of a module, removing the wrapper and un-indenting.
fn unwrap_body_from_braces(
    lines: &[&str],
    sym_start_0: usize,
    sym_end_0: usize,
    eol: &str,
) -> Option<String> {
    // Find the opening brace line
    let mut body_start = None;
    let mut brace_line_tail: Option<&str> = None;
    for (i, line) in lines.iter().enumerate().take(sym_end_0).skip(sym_start_0) {
        let trimmed = line.trim();
        if trimmed.contains('{') {
            // Check if there is code after the `{` on the same line.
            if let Some(open) = line.find('{') {
                let after = line[open + 1..].trim();
                // Ignore if the rest is just `}` or empty (handled below).
                if !after.is_empty() && !after.starts_with('}') {
                    brace_line_tail = Some(line[open + 1..].trim_end());
                }
            }
            body_start = Some(i + 1);
            break;
        }
    }
    let body_start = body_start?;

    // The closing brace is on the last line
    let body_end = if sym_end_0 > 0 {
        sym_end_0 - 1
    } else {
        sym_end_0
    };

    if body_start >= body_end {
        // Opening and closing braces may be on the same line (e.g.
        // `mod foo { fn bar() {} }`).  Extract text between braces.
        if body_start > 0 {
            let brace_line = lines[body_start - 1];
            if let Some(open) = brace_line.find('{') {
                let after_open = &brace_line[open + 1..];
                if let Some(close) = after_open.rfind('}') {
                    let inner = after_open[..close].trim();
                    if !inner.is_empty() {
                        return Some(inner.to_string());
                    }
                }
            }
        }
        return None;
    }

    let body_lines = &lines[body_start..body_end];

    // Find common indentation, counted in characters so that multi-byte
    // whitespace indents stay comparable (#2377).
    let min_indent = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| crate::write::indent_char_count(l))
        .min()
        .unwrap_or(0);

    // Un-indent
    let mut parts: Vec<String> = Vec::new();

    // Prepend any code that was on the same line as the opening `{`.
    if let Some(tail) = brace_line_tail {
        let trimmed = tail.trim();
        // Strip trailing `}` if the closing brace is also on this tail
        // (already handled by the single-line branch above, but be safe).
        parts.push(trimmed.to_string());
    }

    parts.extend(body_lines.iter().map(|line| {
        if line.trim().is_empty() {
            String::new()
        } else {
            // `indent_strip_offset` clamps to the line's own indent, so a line
            // shorter than `min_indent` needs no separate guard.
            line[crate::write::indent_strip_offset(line, min_indent)..].to_string()
        }
    }));

    Some(parts.join(eol))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #2377: the un-indent step measured the common indent in bytes, so a
    /// body mixing ASCII and multi-byte whitespace sliced mid-character.
    #[test]
    fn extract_body_with_wide_whitespace_indent_does_not_panic() {
        let source = "mod tests {\n  fn a() {}\n\u{3000}fn b() {}\n}\n";
        let result = extract_to_file(source, "tests", None, false, None, Language::Rust).unwrap();
        assert!(result.target_content.contains("fn a() {}"));
        assert!(result.target_content.contains("fn b() {}"));
    }

    #[test]
    fn extract_body_shorter_than_common_indent_is_kept() {
        // The blank-ish line must not be sliced past its own length.
        let source = "mod tests {\n    fn a() {}\n\n    fn b() {}\n}\n";
        let result = extract_to_file(source, "tests", None, false, None, Language::Rust).unwrap();
        assert!(result.target_content.contains("fn a() {}"));
        assert!(result.target_content.contains("fn b() {}"));
    }

    #[test]
    fn extract_basic_function() {
        let source = "fn foo() {\n    42\n}\n\nfn bar() {}\n";
        let result = extract_to_file(source, "foo", None, false, None, Language::Rust).unwrap();
        assert!(!result.source_content.contains("fn foo"));
        assert!(result.source_content.contains("fn bar"));
        assert!(result.target_content.contains("fn foo()"));
        assert!(
            result.target_content.contains("    42"),
            "target should contain the function body: {}",
            result.target_content
        );
    }

    #[test]
    fn extract_with_replacement() {
        let source = "mod tests {\n    fn test_a() {}\n}\n\nfn other() {}\n";
        let replacement = "#[path = \"tests.rs\"]\nmod tests;";
        let result = extract_to_file(
            source,
            "tests",
            Some(replacement),
            true,
            None,
            Language::Rust,
        )
        .unwrap();
        assert!(result.source_content.contains("#[path = \"tests.rs\"]"));
        assert!(result.source_content.contains("mod tests;"));
        assert!(!result.source_content.contains("fn test_a"));
    }

    #[test]
    fn extract_unwrap_module() {
        let source = "mod tests {\n    fn test_a() {\n        assert!(true);\n    }\n}\n";
        let result = extract_to_file(source, "tests", None, true, None, Language::Rust).unwrap();
        // Content should be un-indented
        assert!(result.target_content.contains("fn test_a()"));
        assert!(result.target_content.contains("    assert!(true);"));
        // Should NOT contain the mod wrapper
        assert!(!result.target_content.contains("mod tests"));
    }

    #[test]
    fn extract_with_prepend() {
        let source = "fn foo() {}\n\nfn bar() {}\n";
        let result = extract_to_file(
            source,
            "foo",
            None,
            false,
            Some("use super::*;"),
            Language::Rust,
        )
        .unwrap();
        assert!(result.target_content.starts_with("use super::*;"));
        assert!(result.target_content.contains("fn foo()"));
    }

    #[test]
    fn extract_no_unwrap() {
        let source = "mod tests {\n    fn test_a() {}\n}\n";
        let result = extract_to_file(source, "tests", None, false, None, Language::Rust).unwrap();
        // Full module text should be preserved
        assert!(result.target_content.contains("mod tests {"));
    }

    #[test]
    fn extract_symbol_not_found() {
        let source = "fn foo() {}\n";
        let result = extract_to_file(source, "nonexistent", None, false, None, Language::Rust);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
    }

    #[test]
    fn extract_preserves_attributes() {
        let source = "/// Doc comment.\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
        let result = extract_to_file(source, "tests", None, false, None, Language::Rust).unwrap();
        assert!(result.target_content.contains("/// Doc comment."));
        assert!(result.target_content.contains("#[cfg(test)]"));
    }

    #[test]
    fn unwrap_module_with_trailing_comment_on_brace_line() {
        // Opening brace line has a trailing comment: `mod tests { // test module`
        // The brace detection must still find it
        let source =
            "mod tests { // test module\n    fn test_a() {\n        assert!(true);\n    }\n}\n";
        let result = extract_to_file(source, "tests", None, true, None, Language::Rust).unwrap();
        // The mod declaration line should NOT leak into the unwrapped body
        assert!(
            !result.target_content.contains("mod tests"),
            "mod declaration should not appear in unwrapped body: {}",
            result.target_content
        );
        assert!(result.target_content.contains("fn test_a()"));
    }

    #[test]
    fn extract_preserves_crlf_line_endings() {
        let source = "fn foo() {\r\n    42\r\n}\r\n\r\nfn bar() {}\r\n";
        let result = extract_to_file(source, "foo", None, false, None, Language::Rust).unwrap();
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
    fn unwrap_single_line_module() {
        // Regression: single-line modules returned empty body because
        // body_start (line after `{`) >= body_end (line before `}`).
        let source = "mod foo { fn bar() {} }\n";
        let result = extract_to_file(source, "foo", None, true, None, Language::Rust).unwrap();
        assert!(
            result.target_content.contains("fn bar()"),
            "single-line module body should be extracted, got: {:?}",
            result.target_content
        );
    }

    #[test]
    fn unwrap_module_preserves_brace_line_content() {
        // Code after the opening `{` on the same line should be preserved.
        let source = "mod foo { use bar::*;\n    fn baz() {}\n}\n";
        let result = extract_to_file(source, "foo", None, true, None, Language::Rust).unwrap();
        assert!(
            result.target_content.contains("use bar::*;"),
            "content on the brace line should be preserved, got: {:?}",
            result.target_content
        );
        assert!(
            result.target_content.contains("fn baz()"),
            "body content should also be present, got: {:?}",
            result.target_content
        );
    }

    /// #2529: unwrap of a Ruby module must drop `module`/`end` and keep the body.
    #[test]
    fn extract_unwrap_ruby_module() {
        let source = "module Foo\n  def bar\n    1\n  end\nend\n";
        let result = extract_to_file(source, "Foo", None, true, None, Language::Ruby).unwrap();
        assert_eq!(result.target_content, "def bar\n  1\nend\n");
        assert!(
            !result.target_content.contains("module Foo"),
            "unwrapped body must not keep the module header: {}",
            result.target_content
        );
        assert!(
            !result.source_content.contains("module Foo"),
            "source must drop the extracted module: {}",
            result.source_content
        );
    }

    #[test]
    fn extract_unwrap_without_body_is_invalid_input() {
        let source = "mod foo;\n";
        let err = extract_to_file(source, "foo", None, true, None, Language::Rust)
            .expect_err("unwrap of a body-less module must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "unwrap without a body must classify as invalid_input: {err}"
        );
    }

    #[test]
    fn extract_empty_prepend_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        for pre in ["", "   "] {
            let err = extract_to_file(source, "foo", None, false, Some(pre), Language::Rust)
                .expect_err("empty/whitespace prepend must be invalid_input");
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
    fn extract_newline_prepend_is_ok() {
        let source = "fn foo() { let x = 1; }\n";
        let result = extract_to_file(source, "foo", None, false, Some("\n"), Language::Rust)
            .expect("newline-only prepend is insert-a-blank-line");
        assert!(
            result.target_content.starts_with('\n'),
            "newline prepend should insert a blank line: {:?}",
            result.target_content
        );
        assert!(result.target_content.contains("fn foo()"));
    }
}
