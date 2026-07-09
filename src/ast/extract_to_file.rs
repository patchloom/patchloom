//! Extract a named symbol **into a separate source file** (`ast.extract_to_file`).
//!
//! Not to be confused with [`crate::ast::symbol_extract`] (tree-sitter visitors
//! that build the in-memory symbol list).

use super::Language;
use super::symbols::{
    SymbolKind, extract_symbol_text, extract_symbols, find_symbol, full_symbol_span,
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
    let eol = crate::write::detect_eol(source);
    let symbols = extract_symbols(source, lang);
    let sym = find_symbol(&symbols, symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", symbol))?;

    let (full_start, full_end) = full_symbol_span(source, sym, lang);
    let lines: Vec<&str> = source.lines().collect();
    let start_0 = full_start.saturating_sub(1);
    let end_0 = full_end.min(lines.len());

    // Extract the symbol's content for the target file
    let target_body = if unwrap && sym.kind == SymbolKind::Module {
        unwrap_module_body(&lines, sym.start_line.saturating_sub(1), end_0, eol)
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

/// Extract the body of a module, removing the wrapper and un-indenting.
fn unwrap_module_body(lines: &[&str], sym_start_0: usize, sym_end_0: usize, eol: &str) -> String {
    // Find the opening brace line
    let mut body_start = sym_start_0;
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
            body_start = i + 1;
            break;
        }
    }

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
                        return inner.to_string();
                    }
                }
            }
        }
        return String::new();
    }

    let body_lines = &lines[body_start..body_end];

    // Find common indentation
    let min_indent = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
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
        } else if line.len() >= min_indent {
            line[min_indent..].to_string()
        } else {
            line.to_string()
        }
    }));

    parts.join(eol)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
