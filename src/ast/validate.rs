//! Syntax validation using tree-sitter parsing.

use std::path::Path;

use serde::Serialize;

use super::{Language, ParseFailure, try_parse_source};

/// A syntax error found during validation.
#[derive(Debug, Clone, Serialize)]
pub struct SyntaxError {
    /// 1-based line number.
    pub line: usize,
    /// 0-based column.
    pub column: usize,
    /// Problematic source slice, or `missing <kind>` / `invalid <kind>`
    /// when the tree-sitter node has a zero-width span.
    pub text: String,
}

/// Result of validating a source file.
#[derive(Debug, Serialize)]
pub struct ValidationResult {
    /// Whether the file parsed without errors.
    pub valid: bool,
    /// Errors found during parsing.
    pub errors: Vec<SyntaxError>,
    /// The language detected or specified.
    pub language: String,
}

fn validation_from_tree(
    tree: &tree_sitter_lib::Tree,
    source: &str,
    lang: Language,
) -> ValidationResult {
    let root = tree.root_node();
    let mut errors = Vec::new();
    if root.has_error() {
        collect_errors(root, source, &mut errors);
    }
    ValidationResult {
        valid: errors.is_empty(),
        errors,
        language: lang.to_string(),
    }
}

/// Validate syntax of source code for a given language.
pub fn validate_source(source: &str, lang: Language) -> Option<ValidationResult> {
    match try_parse_source(source, lang) {
        Ok((tree, _)) => Some(validation_from_tree(&tree, source, lang)),
        Err(ParseFailure::NoGrammar) => None,
        Err(ParseFailure::DeadlineExceeded) => Some(timeout_validation_source(lang)),
    }
}

/// Timed-out parse recorded as invalid (walks and in-memory callers must not drop it).
fn timeout_result(detail: String, lang: Language) -> ValidationResult {
    ValidationResult {
        valid: false,
        errors: vec![SyntaxError {
            line: 1,
            column: 0,
            text: detail,
        }],
        language: lang.to_string(),
    }
}

/// Timed-out parse recorded as an invalid file (walks must not drop it).
fn timeout_validation(path: &Path, lang: Language) -> ValidationResult {
    timeout_result(
        format!("parse deadline exceeded for {}", path.display()),
        lang,
    )
}

/// In-memory timeout result (no path).
fn timeout_validation_source(lang: Language) -> ValidationResult {
    timeout_result(format!("parse deadline exceeded for {lang}"), lang)
}

/// Validate syntax of a file.
pub fn validate_file(path: &Path, lang_hint: Option<Language>) -> anyhow::Result<ValidationResult> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("no grammar available for {lang}"),
        }));
    }
    // Strict sole-path (#1894): binary / invalid UTF-8 → Binary / InvalidEncoding.
    let source = crate::files::load_text_strict(path, &path.display().to_string())?;
    match try_parse_source(&source, lang) {
        Ok((tree, _)) => Ok(validation_from_tree(&tree, &source, lang)),
        Err(ParseFailure::DeadlineExceeded) => {
            Err(anyhow::Error::new(crate::exit::ParseTimeoutError {
                msg: format!("parse deadline exceeded for {}", path.display()),
            }))
        }
        Err(ParseFailure::NoGrammar) => Err(anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("failed to parse {}", path.display()),
        })),
    }
}

/// Walk-oriented validate: keep timed-out files as `valid: false` (#2406).
///
/// Other errors (no grammar after the walk filter, load failures already
/// preflighted) still return `None` so the caller can treat them as skips.
pub fn validate_file_for_walk(
    path: &Path,
    lang_hint: Option<Language>,
) -> Option<ValidationResult> {
    match validate_file(path, lang_hint) {
        Ok(result) => Some(result),
        Err(e) if crate::exit::is_parse_timeout(&e) => {
            let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
            Some(timeout_validation(path, lang))
        }
        Err(_) => None,
    }
}

fn error_node_text(node: tree_sitter_lib::Node, source: &str) -> String {
    let start = node.start_byte();
    let mut end = node.end_byte().min(start + 50);
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    let span = source.get(start..end).unwrap_or("").trim();
    if !span.is_empty() {
        return span.to_string();
    }
    // MISSING / zero-width ERROR nodes have start==end. Use the grammar
    // kind so --json agents get a non-empty text field (fixrealloop R31).
    if node.is_missing() {
        format!("missing {}", node.kind())
    } else {
        format!("invalid {}", node.kind())
    }
}

fn collect_errors(node: tree_sitter_lib::Node, source: &str, errors: &mut Vec<SyntaxError>) {
    if node.is_error() || node.is_missing() {
        let start = node.start_byte();
        let line = crate::ops::file::text_line_index(source, start) + 1;
        // Keep the existing 0-based column, but measure from the last
        // `\n` / `\r` so CR-only files match LF/CRLF (#2344).
        let line_start = source[..start.min(source.len())]
            .rfind(['\n', '\r'])
            .map(|i| i + 1)
            .unwrap_or(0);
        let column = start.saturating_sub(line_start);
        errors.push(SyntaxError {
            line,
            column,
            text: error_node_text(node, source),
        });
        return; // Don't recurse into error nodes
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.has_error() || child.is_error() || child.is_missing() {
                collect_errors(child, source, errors);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_rust_code() {
        let source = "fn main() { println!(\"hello\"); }\n";
        let result = validate_source(source, Language::Rust).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn invalid_rust_code() {
        let source = "fn main( { }\n";
        let result = validate_source(source, Language::Rust).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn valid_python_code() {
        let source = "def hello():\n    pass\n";
        let result = validate_source(source, Language::Python).unwrap();
        assert!(result.valid);
    }

    #[test]
    fn unknown_language_returns_none() {
        let result = validate_source("anything", Language::Unknown);
        assert!(result.is_none());
    }

    #[test]
    fn error_location_is_correct() {
        let source = "fn main(\n";
        let result = validate_source(source, Language::Rust).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
        // Error should be on line 1 or 2
        assert!(result.errors[0].line <= 2);
    }

    #[test]
    fn cr_only_syntax_error_uses_same_line_as_lf() {
        let lf = "fn ok() {}\nfn bad( {}\n";
        let cr = "fn ok() {}\rfn bad( {}\r";
        let crlf = "fn ok() {}\r\nfn bad( {}\r\n";
        let lf_r = validate_source(lf, Language::Rust).unwrap();
        let cr_r = validate_source(cr, Language::Rust).unwrap();
        let crlf_r = validate_source(crlf, Language::Rust).unwrap();
        assert!(!lf_r.valid && !cr_r.valid && !crlf_r.valid);
        assert_eq!(lf_r.errors[0].line, 2);
        assert_eq!(cr_r.errors[0].line, 2, "CR-only must not stay on line 1");
        assert_eq!(crlf_r.errors[0].line, 2);
        assert_eq!(cr_r.errors[0].column, lf_r.errors[0].column);
        assert_eq!(crlf_r.errors[0].column, lf_r.errors[0].column);
    }

    #[test]
    fn empty_span_error_has_nonempty_text() {
        // MISSING / zero-width ERROR nodes have start==end, so a raw
        // source slice is empty. Agents need a non-empty text field.
        let source = "fn bad( {}\n";
        let result = validate_source(source, Language::Rust).unwrap();
        assert!(!result.valid);
        assert!(
            result.errors.iter().all(|e| !e.text.trim().is_empty()),
            "empty error text: {:?}",
            result.errors
        );
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.text.starts_with("missing ") || e.text.starts_with("invalid ")),
            "expected kind fallback text: {:?}",
            result.errors
        );
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
    fn validate_source_timeout_is_invalid() {
        let source = nested_rust_source(80_000);
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = validate_source(&source, Language::Rust).expect("timeout stays Some");
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.text.contains("deadline")),
            "{:?}",
            result.errors
        );
    }

    #[test]
    fn validate_file_timeout_is_parse_timeout() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("deep.rs");
        std::fs::write(&path, nested_rust_source(80_000)).unwrap();
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let err = validate_file(&path, Some(Language::Rust)).unwrap_err();
        assert!(
            crate::exit::is_parse_timeout(&err),
            "expected parse_timeout, got {err}"
        );
        assert_eq!(crate::fallback::error_kind_str(&err), Some("parse_timeout"));
    }

    #[test]
    fn validate_file_for_walk_timeout_is_invalid() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("deep.rs");
        std::fs::write(&path, nested_rust_source(80_000)).unwrap();
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let result = validate_file_for_walk(&path, Some(Language::Rust)).expect("timeout stays");
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.text.contains("deadline")),
            "{:?}",
            result.errors
        );
    }
}
