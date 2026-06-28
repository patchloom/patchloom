//! AST-aware symbol extraction: extract a symbol to a separate file.

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
    let symbols = extract_symbols(source, lang);
    let sym = find_symbol(&symbols, symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", symbol))?;

    let (full_start, full_end) = full_symbol_span(source, sym, lang);
    let lines: Vec<&str> = source.lines().collect();
    let start_0 = full_start.saturating_sub(1);
    let end_0 = full_end.min(lines.len());

    // Extract the symbol's content for the target file
    let target_body = if unwrap && sym.kind == SymbolKind::Module {
        unwrap_module_body(&lines, sym.start_line.saturating_sub(1), end_0)
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
            target_content.push('\n');
        }
        target_content.push('\n');
    }
    target_content.push_str(&target_body);
    if !target_content.ends_with('\n') {
        target_content.push('\n');
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

    let mut source_content = source_lines.join("\n");
    if source.ends_with('\n') && !source_content.ends_with('\n') {
        source_content.push('\n');
    }

    Ok(ExtractResult {
        source_content,
        target_content,
        extracted_lines,
    })
}

/// Extract the body of a module, removing the wrapper and un-indenting.
fn unwrap_module_body(lines: &[&str], sym_start_0: usize, sym_end_0: usize) -> String {
    // Find the opening brace line
    let mut body_start = sym_start_0;
    for (i, line) in lines.iter().enumerate().take(sym_end_0).skip(sym_start_0) {
        let trimmed = line.trim();
        if trimmed.contains('{') {
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
    body_lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else if line.len() >= min_indent {
                line[min_indent..].to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        assert!(result.is_err());
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
}
