//! AST-aware code wrapping: wrap existing code in a block (module, impl, etc.).

use super::Language;
use super::symbols::{extract_symbols_or_timeout, find_symbol, full_symbol_span};

/// Result of a wrap operation.
#[derive(Debug)]
pub struct WrapResult {
    /// The full file content after wrapping.
    pub content: String,
}

/// Wrap code in a structural block.
///
/// Exactly one of `symbols` or `lines` must be specified.
///
/// - `symbols`: names of symbols to wrap.
/// - `lines`: line range (e.g. "10:50") to wrap.
/// - `wrapper`: the wrapping construct (e.g. "mod foo", "impl Bar", "#[cfg(test)]").
/// - `preamble`: optional content to insert at the top of the wrapped block.
pub fn wrap_code(
    source: &str,
    symbols_arg: Option<&[String]>,
    lines_arg: Option<&str>,
    wrapper: &str,
    preamble: Option<&str>,
    lang: Language,
) -> anyhow::Result<WrapResult> {
    let mode_count = symbols_arg.is_some() as u8 + lines_arg.is_some() as u8;
    if mode_count != 1 {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "exactly one of 'symbols' or 'lines' must be specified".into(),
        }));
    }

    if wrapper.trim().is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "ast wrap wrapper must not be empty".into(),
        }));
    }

    if let Some(pre) = preamble
        && pre.trim().is_empty()
        && !pre.contains('\n')
        && !pre.contains('\r')
    {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "ast wrap preamble must not be empty".into(),
        }));
    }

    let eol = crate::write::detect_eol(source);
    let source_lines: Vec<&str> = crate::ops::file::text_lines(source).collect();

    // Determine the range of lines to wrap (0-based indices).
    let (start_idx, end_idx) = if let Some(symbol_names) = symbols_arg {
        find_symbol_range(source, symbol_names, lang)?
    } else {
        parse_line_range_to_indices(
            lines_arg.expect("mode_count==1 guarantees Some"),
            source_lines.len(),
        )?
    };

    // Extract the code to wrap
    let target_lines = &source_lines[start_idx..end_idx];

    // Detect the indentation of the target code
    let base_indent = if start_idx < source_lines.len() {
        let line = source_lines[start_idx];
        let trimmed = line.trim_start();
        &line[..line.len() - trimmed.len()]
    } else {
        ""
    };

    let (wrapper_opening, closer) = wrap_delimiters(wrapper, lang)?;

    // Build the wrapped output
    let mut wrapped = String::new();

    wrapped.push_str(base_indent);
    wrapped.push_str(&wrapper_opening);
    wrapped.push_str(eol);

    // Preamble (if any)
    if let Some(pre) = preamble {
        for pline in pre.lines() {
            if pline.trim().is_empty() {
                wrapped.push_str(eol);
            } else {
                wrapped.push_str(base_indent);
                wrapped.push_str("    ");
                wrapped.push_str(pline.trim());
                wrapped.push_str(eol);
            }
        }
        wrapped.push_str(eol);
    }

    // Indented body
    for line in target_lines {
        if line.trim().is_empty() {
            wrapped.push_str(eol);
        } else {
            wrapped.push_str("    ");
            wrapped.push_str(line);
            wrapped.push_str(eol);
        }
    }

    if let Some(close) = closer {
        wrapped.push_str(base_indent);
        wrapped.push_str(close);
        wrapped.push_str(eol);
    }

    // Reconstruct the file
    let mut result = String::new();
    for line in &source_lines[..start_idx] {
        result.push_str(line);
        result.push_str(eol);
    }
    result.push_str(&wrapped);
    for line in &source_lines[end_idx..] {
        result.push_str(line);
        result.push_str(eol);
    }

    // Preserve trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - eol.len());
    }

    Ok(WrapResult { content: result })
}

/// Opening line and optional closer for `lang`. Brace languages keep
/// `{` / `}`; Python uses `:`; Ruby uses `end`. Other languages refuse.
fn wrap_delimiters(
    wrapper: &str,
    lang: Language,
) -> anyhow::Result<(String, Option<&'static str>)> {
    let trimmed = wrapper.trim();
    match lang {
        Language::Rust
        | Language::TypeScript
        | Language::JavaScript
        | Language::Go
        | Language::Java
        | Language::CSharp
        | Language::Php
        | Language::Swift
        | Language::Kotlin
        | Language::Cpp
        | Language::C
        | Language::Hcl
        | Language::Protobuf => {
            let open = if trimmed.ends_with('{') {
                trimmed.to_string()
            } else {
                format!("{trimmed} {{")
            };
            Ok((open, Some("}")))
        }
        Language::Python => {
            let core = trimmed.trim_end_matches('{').trim_end();
            let open = if core.ends_with(':') {
                core.to_string()
            } else {
                format!("{core}:")
            };
            Ok((open, None))
        }
        Language::Ruby => {
            let open = trimmed.trim_end_matches('{').trim_end().to_string();
            Ok((open, Some("end")))
        }
        other => Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("ast wrap does not support {other} (no known wrapping form)"),
        })),
    }
}

/// Find the 0-based start and end line indices that span all named symbols.
fn find_symbol_range(
    source: &str,
    symbol_names: &[String],
    lang: Language,
) -> anyhow::Result<(usize, usize)> {
    if symbol_names.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "symbols list must not be empty".into(),
        }));
    }
    let symbols = extract_symbols_or_timeout(source, lang)?;
    let mut min_start = usize::MAX;
    let mut max_end = 0usize;

    for name in symbol_names {
        let sym = find_symbol(&symbols, name).ok_or_else(|| {
            anyhow::Error::new(crate::exit::NoMatchError {
                msg: format!("symbol '{name}' not found"),
            })
        })?;
        let (full_start, full_end) = full_symbol_span(source, sym, lang);
        let start = full_start.saturating_sub(1);
        let end = full_end;
        if start < min_start {
            min_start = start;
        }
        if end > max_end {
            max_end = end;
        }
    }

    Ok((min_start, max_end))
}

/// Parse a line range like "10:50" into 0-based start/end indices.
fn parse_line_range_to_indices(spec: &str, total_lines: usize) -> anyhow::Result<(usize, usize)> {
    let parts: Vec<&str> = if spec.contains(':') {
        spec.splitn(2, ':').collect()
    } else if spec.contains('-') && !spec.starts_with('-') {
        spec.splitn(2, '-').collect()
    } else {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("invalid line range format: '{spec}' (expected START:END)"),
        }));
    };

    if parts.len() != 2 {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("invalid line range: '{spec}'"),
        }));
    }

    let start: usize = parts[0].parse().map_err(|_| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("invalid start line: {}", parts[0]),
        })
    })?;

    if start == 0 {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "line numbers are 1-based, got 0".into(),
        }));
    }

    let start_idx = start - 1;

    if parts[1].is_empty() {
        return Ok((start_idx, total_lines));
    }

    let end: usize = parts[1].parse().map_err(|_| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("invalid end line: {}", parts[1]),
        })
    })?;
    if end < start {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("end line {end} is before start line {start}"),
        }));
    }

    let end_idx = end.min(total_lines);

    Ok((start_idx, end_idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_symbols_in_module() {
        let source = "fn helper_a() {}\n\nfn helper_b() {}\n";
        let result = wrap_code(
            source,
            Some(&["helper_a".into(), "helper_b".into()]),
            None,
            "pub(crate) mod helpers",
            None,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("pub(crate) mod helpers {"));
        assert!(result.content.contains("    fn helper_a()"));
        assert!(result.content.contains("    fn helper_b()"));
        assert!(
            result.content.ends_with("}\n") || result.content.ends_with('}'),
            "module wrapper should close with }}: {}",
            result.content
        );
    }

    #[test]
    fn wrap_with_preamble() {
        let source = "fn test_a() {}\n\nfn test_b() {}\n";
        let result = wrap_code(
            source,
            Some(&["test_a".into(), "test_b".into()]),
            None,
            "mod tests",
            Some("use super::*;"),
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("mod tests {"));
        assert!(result.content.contains("    use super::*;"));
        let use_pos = result.content.find("use super::*").unwrap();
        let fn_pos = result.content.find("fn test_a").unwrap();
        assert!(use_pos < fn_pos);
    }

    #[test]
    fn wrap_lines_range() {
        let source = "line1\nline2\nline3\nline4\nline5\n";
        let result = wrap_code(
            source,
            None,
            Some("2:4"),
            "#[cfg(test)]",
            None,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("#[cfg(test)] {"));
        assert!(result.content.contains("    line2"));
        assert!(result.content.contains("    line3"));
        assert!(result.content.contains("    line4"));
        // line1 and line5 should not be wrapped
        assert!(result.content.starts_with("line1\n"));
        assert!(result.content.contains("}\nline5"));
    }

    #[test]
    fn wrap_preserves_indentation() {
        let source = "    fn inner_a() {}\n    fn inner_b() {}\n";
        let result = wrap_code(
            source,
            Some(&["inner_a".into(), "inner_b".into()]),
            None,
            "mod nested",
            None,
            Language::Rust,
        )
        .unwrap();
        // The wrapper should use the same base indentation
        assert!(result.content.contains("    mod nested {"));
        assert!(result.content.contains("        fn inner_a()"));
    }

    #[test]
    fn wrap_symbol_not_found() {
        let source = "fn foo() {}\n";
        let result = wrap_code(
            source,
            Some(&["nonexistent".into()]),
            None,
            "mod wrapper",
            None,
            Language::Rust,
        );
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn wrap_requires_exactly_one_mode() {
        let source = "fn foo() {}\n";
        // None specified
        let result = wrap_code(source, None, None, "mod x", None, Language::Rust);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");

        // Both specified
        let result = wrap_code(
            source,
            Some(&["foo".into()]),
            Some("1:1"),
            "mod x",
            None,
            Language::Rust,
        );
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
    }

    #[test]
    fn wrap_includes_doc_comments_and_attributes() {
        // Doc comments and attributes above a symbol must be included in the wrap
        let source = "/// Helper docs\n#[inline]\nfn helper() {\n    42\n}\n";
        let result = wrap_code(
            source,
            Some(&["helper".into()]),
            None,
            "mod utils",
            None,
            Language::Rust,
        )
        .unwrap();
        // The doc comment and attribute must be inside the module wrapper
        let wrapper_pos = result.content.find("mod utils {").unwrap();
        let doc_pos = result.content.find("/// Helper docs").unwrap();
        let attr_pos = result.content.find("#[inline]").unwrap();
        assert!(
            doc_pos > wrapper_pos,
            "doc comment should be inside wrapper: {}",
            result.content
        );
        assert!(
            attr_pos > wrapper_pos,
            "attribute should be inside wrapper: {}",
            result.content
        );
    }

    #[test]
    fn wrap_preserves_crlf_line_endings() {
        let source = "fn helper_a() {}\r\n\r\nfn helper_b() {}\r\n";
        let result = wrap_code(
            source,
            Some(&["helper_a".into(), "helper_b".into()]),
            None,
            "mod helpers",
            None,
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
    fn wrap_cfg_attribute() {
        let source = "fn feature_a() {\n    // experimental\n}\n";
        let result = wrap_code(
            source,
            Some(&["feature_a".into()]),
            None,
            "#[cfg(feature = \"experimental\")]",
            None,
            Language::Rust,
        )
        .unwrap();
        assert!(
            result
                .content
                .contains("#[cfg(feature = \"experimental\")] {")
        );
    }

    #[test]
    fn wrap_empty_wrapper_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        let err = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "",
            None,
            Language::Rust,
        )
        .expect_err("empty wrap wrapper must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty wrapper must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn wrap_whitespace_only_wrapper_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        let err = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "   ",
            None,
            Language::Rust,
        )
        .expect_err("whitespace-only wrap wrapper must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "whitespace-only wrapper must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn wrap_newline_only_wrapper_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        let err = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "\n",
            None,
            Language::Rust,
        )
        .expect_err("newline-only wrap wrapper must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "newline-only wrapper must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn wrap_empty_preamble_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        let err = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "mod tests",
            Some(""),
            Language::Rust,
        )
        .expect_err("empty wrap preamble must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty preamble must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    #[test]
    fn wrap_empty_whitespace_preamble_is_invalid_input() {
        let source = "fn foo() { let x = 1; }\n";
        let err = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "mod tests",
            Some("   "),
            Language::Rust,
        )
        .expect_err("whitespace-only wrap preamble must be invalid_input");
        assert!(
            crate::exit::is_invalid_input(&err),
            "whitespace-only preamble must classify as invalid_input: {err}"
        );
        assert!(
            err.to_string().contains("must not be empty"),
            "message must say must not be empty: {err}"
        );
    }

    /// #2527: Python wrap uses `wrapper:` and no closing brace.
    #[test]
    fn wrap_python_uses_colon_not_braces() {
        let source = "def a():\n    return 1\n\ndef b():\n    return 2\n";
        let result = wrap_code(
            source,
            Some(&["a".into(), "b".into()]),
            None,
            "class Util",
            None,
            Language::Python,
        )
        .unwrap();
        assert_eq!(
            result.content,
            "class Util:\n    def a():\n        return 1\n\n    def b():\n        return 2\n"
        );
        assert!(
            !result.content.contains('{') && !result.content.contains('}'),
            "Python wrap must not emit braces: {}",
            result.content
        );
    }

    /// #2527: Ruby wrap uses `wrapper` / `end`, not braces.
    #[test]
    fn wrap_ruby_uses_end_not_braces() {
        let source = "def a\n  1\nend\n\ndef b\n  2\nend\n";
        let result = wrap_code(
            source,
            Some(&["a".into(), "b".into()]),
            None,
            "module Util",
            None,
            Language::Ruby,
        )
        .unwrap();
        assert!(
            result.content.starts_with("module Util\n"),
            "Ruby wrap must open with the wrapper line: {}",
            result.content
        );
        assert!(
            result.content.contains("    def a") && result.content.contains("    def b"),
            "wrapped methods must be indented: {}",
            result.content
        );
        let trimmed = result.content.trim_end();
        assert!(
            trimmed.ends_with("\nend") || trimmed.ends_with("end"),
            "Ruby wrap must close with end: {}",
            result.content
        );
        assert!(
            !result.content.contains('{') && !result.content.contains('}'),
            "Ruby wrap must not emit braces: {}",
            result.content
        );
    }

    #[test]
    fn wrap_newline_preamble_still_ok() {
        let source = "fn foo() { let x = 1; }\n";
        let result = wrap_code(
            source,
            Some(&["foo".into()]),
            None,
            "mod tests",
            Some("\n"),
            Language::Rust,
        )
        .expect("newline-only wrap preamble is insert-a-blank-line");
        assert!(
            result.content.contains("mod tests {"),
            "wrapper must still apply: {}",
            result.content
        );
        assert!(
            result.content.contains("fn foo()"),
            "wrapped symbol must remain: {}",
            result.content
        );
    }
}
