//! AST-aware code insertion: insert code at structurally-defined positions.

use super::Language;
use super::symbols::{SymbolDef, extract_symbols, find_symbol, full_symbol_span};

/// Result of an AST insert operation.
#[derive(Debug)]
pub struct InsertResult {
    /// The full file content after insertion.
    pub content: String,
    /// 1-based line where insertion occurred.
    pub inserted_at_line: usize,
}

/// Shared context for insert operations, grouping the common parameters
/// derived from the source file.
struct InsertContext<'a> {
    source: &'a str,
    content: &'a str,
    symbols: &'a [SymbolDef],
    lines: &'a [&'a str],
    lang: Language,
    eol: &'a str,
}

/// Position within a container (module, impl, etc.) to insert code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    Start,
    End,
}

/// Insert code at a structurally-aware position in source code.
///
/// Exactly one of `inside`, `after`, or `before` must be `Some`.
///
/// - `inside` + `position`: insert content at start/end of the named block.
/// - `after`: insert content after the named symbol.
/// - `before`: insert content before the named symbol.
pub fn insert_code(
    source: &str,
    content: &str,
    inside: Option<&str>,
    after: Option<&str>,
    before: Option<&str>,
    position: InsertPosition,
    lang: Language,
) -> anyhow::Result<InsertResult> {
    let mode_count = inside.is_some() as u8 + after.is_some() as u8 + before.is_some() as u8;
    if mode_count != 1 {
        anyhow::bail!("exactly one of 'inside', 'after', or 'before' must be specified");
    }

    let eol = crate::write::detect_eol(source);
    let symbols = extract_symbols(source, lang);
    let lines: Vec<&str> = source.lines().collect();

    let ctx = InsertContext {
        source,
        content,
        symbols: &symbols,
        lines: &lines,
        lang,
        eol,
    };

    if let Some(container_name) = inside {
        insert_inside(&ctx, container_name, position)
    } else if let Some(after_name) = after {
        insert_adjacent(&ctx, after_name, true)
    } else {
        let before_name = before.expect("mode_count==1 guarantees Some");
        insert_adjacent(&ctx, before_name, false)
    }
}

/// Insert content inside a named container (mod, impl, struct, etc.).
fn insert_inside(
    ctx: &InsertContext<'_>,
    container_name: &str,
    position: InsertPosition,
) -> anyhow::Result<InsertResult> {
    let sym = find_symbol(ctx.symbols, container_name)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", container_name))?;

    let start_idx = sym.start_line.saturating_sub(1);
    let end_idx = sym.end_line.min(ctx.lines.len());

    // Single-line container (e.g. `mod foo {}`): the opening `{` and closing
    // `}` are on the same line.  Both Start and End behave the same: split
    // the line at the braces and insert content between them.
    let close_line_idx = end_idx.saturating_sub(1);
    if close_line_idx <= start_idx {
        let container_line = ctx.lines[start_idx];
        if let Some(open) = container_line.find('{')
            && let Some(close_rel) = container_line[open + 1..].rfind('}')
        {
            let close = open + 1 + close_rel;
            let before_brace = &container_line[..=open]; // up to and including `{`
            let after_brace = &container_line[close..]; // from `}` onward

            // Use container line indent + 4 spaces for the body.
            let line_indent = container_line.len() - container_line.trim_start().len();
            let body_indent = format!("{}{}", &container_line[..line_indent], "    ");
            let adjusted = indent_content(ctx.content, &body_indent, ctx.eol);

            let mut result = String::new();
            for line in &ctx.lines[..start_idx] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }
            result.push_str(before_brace);
            result.push_str(ctx.eol);
            result.push_str(&adjusted);
            if !adjusted.ends_with('\n') {
                result.push_str(ctx.eol);
            }
            result.push_str(after_brace);
            result.push_str(ctx.eol);
            for line in &ctx.lines[start_idx + 1..] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }
            if !ctx.source.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - ctx.eol.len());
            }
            return Ok(InsertResult {
                content: result,
                inserted_at_line: start_idx + 2, // 1-based, after the opening brace
            });
        }
    }

    // Detect the indentation level inside the container.
    let container_indent = detect_indent(ctx.lines, start_idx, end_idx);
    let adjusted = indent_content(ctx.content, &container_indent, ctx.eol);

    let mut result = String::new();

    match position {
        InsertPosition::End => {
            // Insert before the closing brace of the container.
            // The closing brace is typically on end_line (1-based), i.e. end_idx - 1 (0-based).

            for line in &ctx.lines[..close_line_idx] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }

            // Add a blank line before if the previous content doesn't end with one
            if close_line_idx > 0 && !ctx.lines[close_line_idx - 1].trim().is_empty() {
                result.push_str(ctx.eol);
            }
            result.push_str(&adjusted);
            if !adjusted.ends_with('\n') {
                result.push_str(ctx.eol);
            }

            for line in &ctx.lines[close_line_idx..] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }

            let inserted_at = close_line_idx + 1; // 1-based

            // Preserve trailing newline behavior
            if !ctx.source.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - ctx.eol.len());
            }

            Ok(InsertResult {
                content: result,
                inserted_at_line: inserted_at,
            })
        }
        InsertPosition::Start => {
            // Insert after the opening brace/line of the container.
            // We look for the first line after start_line that ends with '{' or ':'
            let insert_after_idx = find_opening_line(ctx.lines, start_idx, end_idx);

            for line in &ctx.lines[..=insert_after_idx] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }

            result.push_str(&adjusted);
            if !adjusted.ends_with('\n') {
                result.push_str(ctx.eol);
            }

            // Add blank line if next line is not blank
            if insert_after_idx + 1 < ctx.lines.len()
                && !ctx.lines[insert_after_idx + 1].trim().is_empty()
            {
                result.push_str(ctx.eol);
            }

            for line in &ctx.lines[insert_after_idx + 1..] {
                result.push_str(line);
                result.push_str(ctx.eol);
            }

            let inserted_at = insert_after_idx + 2; // 1-based

            if !ctx.source.ends_with('\n') && result.ends_with('\n') {
                result.truncate(result.len() - ctx.eol.len());
            }

            Ok(InsertResult {
                content: result,
                inserted_at_line: inserted_at,
            })
        }
    }
}

/// Insert content after or before a named symbol.
fn insert_adjacent(
    ctx: &InsertContext<'_>,
    symbol_name: &str,
    after: bool,
) -> anyhow::Result<InsertResult> {
    let sym = find_symbol(ctx.symbols, symbol_name)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", symbol_name))?;

    // Use the symbol's indentation level for the new content
    let sym_start_idx = sym.start_line.saturating_sub(1);
    let indent = if sym_start_idx < ctx.lines.len() {
        let line = ctx.lines[sym_start_idx];
        let trimmed = line.trim_start();
        &line[..line.len() - trimmed.len()]
    } else {
        ""
    };
    let adjusted = indent_content(ctx.content, indent, ctx.eol);

    let mut result = String::new();
    let inserted_at;

    if after {
        let end_idx = sym.end_line.min(ctx.lines.len());
        for line in &ctx.lines[..end_idx] {
            result.push_str(line);
            result.push_str(ctx.eol);
        }
        result.push_str(ctx.eol);
        result.push_str(&adjusted);
        if !adjusted.ends_with('\n') {
            result.push_str(ctx.eol);
        }
        inserted_at = end_idx + 1; // 1-based
        for line in &ctx.lines[end_idx..] {
            result.push_str(line);
            result.push_str(ctx.eol);
        }
    } else {
        // Before: use full_symbol_span to include attributes and doc comments.
        let (full_start, _) = full_symbol_span(ctx.source, sym, ctx.lang);
        let start_idx = full_start.saturating_sub(1);
        for line in &ctx.lines[..start_idx] {
            result.push_str(line);
            result.push_str(ctx.eol);
        }
        result.push_str(&adjusted);
        if !adjusted.ends_with('\n') {
            result.push_str(ctx.eol);
        }
        result.push_str(ctx.eol);
        inserted_at = start_idx + 1; // 1-based
        for line in &ctx.lines[start_idx..] {
            result.push_str(line);
            result.push_str(ctx.eol);
        }
    }

    if !ctx.source.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - ctx.eol.len());
    }

    Ok(InsertResult {
        content: result,
        inserted_at_line: inserted_at,
    })
}

/// Find the line with the opening brace/colon of a container.
fn find_opening_line(lines: &[&str], start_idx: usize, end_idx: usize) -> usize {
    for (i, line) in lines
        .iter()
        .enumerate()
        .take(end_idx.min(lines.len()))
        .skip(start_idx)
    {
        let trimmed = line.trim();
        if trimmed.ends_with('{') || trimmed.ends_with(':') || trimmed.ends_with(":{") {
            return i;
        }
    }
    // Fallback: use the start line itself
    start_idx
}

/// Detect the indentation used inside a container block.
fn detect_indent(lines: &[&str], start_idx: usize, end_idx: usize) -> String {
    // Look at the first non-empty line inside the block (after the opening)
    let body_start = find_opening_line(lines, start_idx, end_idx) + 1;
    for i in body_start..end_idx.saturating_sub(1) {
        if i < lines.len() && !lines[i].trim().is_empty() {
            let line = lines[i];
            let trimmed = line.trim_start();
            return line[..line.len() - trimmed.len()].to_string();
        }
    }
    // Fallback: base indent of container + 4 spaces
    if start_idx < lines.len() {
        let line = lines[start_idx];
        let trimmed = line.trim_start();
        let base = &line[..line.len() - trimmed.len()];
        return format!("{base}    ");
    }
    "    ".to_string()
}

/// Indent content to match the target indentation.
///
/// Strips the common leading whitespace from the content, then prepends
/// the target indentation to each line.
fn indent_content(content: &str, target_indent: &str, eol: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // Find the minimum indentation of non-empty lines
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                let stripped = &line[min_indent..];
                format!("{target_indent}{stripped}")
            }
        })
        .collect::<Vec<_>>()
        .join(eol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_inside_mod_end() {
        let source = "mod tests {\n    fn existing() {}\n}\n";
        let content = "#[test]\nfn new_test() {\n    assert!(true);\n}";
        let result = insert_code(
            source,
            content,
            Some("tests"),
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("fn new_test()"));
        assert!(result.content.contains("fn existing()"));
        // The new test should be before the closing brace
        let close_pos = result.content.rfind('}').unwrap();
        let new_fn_pos = result.content.find("fn new_test").unwrap();
        assert!(new_fn_pos < close_pos);
    }

    #[test]
    fn insert_inside_mod_start() {
        let source = "mod tests {\n    fn existing() {}\n}\n";
        let content = "use super::*;";
        let result = insert_code(
            source,
            content,
            Some("tests"),
            None,
            None,
            InsertPosition::Start,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("use super::*;"));
        // use should come before existing()
        let use_pos = result.content.find("use super::*").unwrap();
        let fn_pos = result.content.find("fn existing").unwrap();
        assert!(use_pos < fn_pos);
    }

    #[test]
    fn insert_after_symbol() {
        let source = "fn foo() {\n    let x = 1;\n}\n\nfn bar() {}\n";
        let content = "fn baz() {\n    let y = 2;\n}";
        let result = insert_code(
            source,
            content,
            None,
            Some("foo"),
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("fn baz()"));
        let foo_pos = result.content.find("fn foo").unwrap();
        let baz_pos = result.content.find("fn baz").unwrap();
        let bar_pos = result.content.find("fn bar").unwrap();
        assert!(foo_pos < baz_pos);
        assert!(baz_pos < bar_pos);
    }

    #[test]
    fn insert_before_symbol() {
        let source = "fn foo() {}\n\nfn bar() {}\n";
        let content = "fn baz() {}";
        let result = insert_code(
            source,
            content,
            None,
            None,
            Some("bar"),
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("fn baz()"));
        let baz_pos = result.content.find("fn baz").unwrap();
        let bar_pos = result.content.find("fn bar").unwrap();
        assert!(baz_pos < bar_pos);
    }

    // Regression: "before" insertion must go before attributes/doc comments,
    // not between them and the symbol definition.
    #[test]
    fn insert_before_preserves_attributes() {
        let source = "/// Important docs\n#[test]\nfn target() {}\n";
        let result = insert_code(
            source,
            "fn new_fn() {}",
            None,
            None,
            Some("target"),
            InsertPosition::End, // "End" with adjacent=before inserts before target
            Language::Rust,
        )
        .unwrap();
        let new_pos = result.content.find("fn new_fn").unwrap();
        let doc_pos = result.content.find("/// Important").unwrap();
        assert!(
            new_pos < doc_pos,
            "new_fn should be inserted before the doc comment, not after it: {}",
            result.content
        );
    }

    #[test]
    fn insert_indentation_adjusted() {
        let source = "mod tests {\n    fn test_a() {\n        assert!(true);\n    }\n}\n";
        let content = "fn test_b() {\n    assert!(false);\n}";
        let result = insert_code(
            source,
            content,
            Some("tests"),
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        // Should be indented to match existing content inside the module
        assert!(result.content.contains("    fn test_b()"));
        assert!(result.content.contains("        assert!(false)"));
    }

    #[test]
    fn insert_symbol_not_found() {
        let source = "fn foo() {}\n";
        let result = insert_code(
            source,
            "fn bar() {}",
            Some("nonexistent"),
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn insert_requires_exactly_one_mode() {
        let source = "fn foo() {}\n";
        // None specified
        let result = insert_code(
            source,
            "fn bar() {}",
            None,
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        );
        assert!(result.is_err());

        // Two specified
        let result = insert_code(
            source,
            "fn bar() {}",
            Some("foo"),
            Some("foo"),
            None,
            InsertPosition::End,
            Language::Rust,
        );
        assert!(result.is_err());
    }

    #[test]
    fn insert_python_inside_class() {
        let source = "class MyClass:\n    def existing(self):\n        pass\n";
        let content = "def new_method(self):\n    return 42";
        let result = insert_code(
            source,
            content,
            Some("MyClass"),
            None,
            None,
            InsertPosition::End,
            Language::Python,
        )
        .unwrap();
        assert!(result.content.contains("def new_method"));
    }

    #[test]
    fn insert_preserves_crlf_line_endings() {
        let source = "mod tests {\r\n    fn existing() {}\r\n}\r\n";
        let content = "fn new_fn() {}";
        let result = insert_code(
            source,
            content,
            Some("tests"),
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        let bytes = result.content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "bare LF at byte {i} in: {:?}",
                    result.content
                );
            }
        }
    }

    #[test]
    fn insert_after_preserves_crlf() {
        let source = "fn foo() {\r\n    let x = 1;\r\n}\r\n\r\nfn bar() {}\r\n";
        let content = "fn baz() {}";
        let result = insert_code(
            source,
            content,
            None,
            Some("foo"),
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        let bytes = result.content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "bare LF at byte {i} in: {:?}",
                    result.content
                );
            }
        }
    }

    #[test]
    fn insert_typescript_after_function() {
        let source = "function hello() {\n  console.log('hi');\n}\n\nfunction world() {}\n";
        let content = "function middle() {\n  return 1;\n}";
        let result = insert_code(
            source,
            content,
            None,
            Some("hello"),
            None,
            InsertPosition::End,
            Language::TypeScript,
        )
        .unwrap();
        assert!(result.content.contains("function middle()"));
        let hello_pos = result.content.find("function hello").unwrap();
        let middle_pos = result.content.find("function middle").unwrap();
        let world_pos = result.content.find("function world").unwrap();
        assert!(hello_pos < middle_pos);
        assert!(middle_pos < world_pos);
    }

    #[test]
    fn insert_inside_single_line_container_end() {
        // Single-line container: `mod foo {}` with both braces on one line.
        let source = "mod foo {}\n";
        let content = "fn bar() {}";
        let result = insert_code(
            source,
            content,
            Some("foo"),
            None,
            None,
            InsertPosition::End,
            Language::Rust,
        )
        .unwrap();
        // Content must be INSIDE the module, between { and }.
        let open = result.content.find('{').unwrap();
        let close = result.content.rfind('}').unwrap();
        let bar_pos = result.content.find("fn bar").unwrap();
        assert!(
            bar_pos > open && bar_pos < close,
            "fn bar should be inside braces, got:\n{}",
            result.content
        );
    }

    #[test]
    fn insert_inside_single_line_container_start() {
        let source = "mod foo {}\n";
        let content = "use bar::*;";
        let result = insert_code(
            source,
            content,
            Some("foo"),
            None,
            None,
            InsertPosition::Start,
            Language::Rust,
        )
        .unwrap();
        let open = result.content.find('{').unwrap();
        let close = result.content.rfind('}').unwrap();
        let use_pos = result.content.find("use bar").unwrap();
        assert!(
            use_pos > open && use_pos < close,
            "use statement should be inside braces, got:\n{}",
            result.content
        );
    }
}
