//! Syntax validation using tree-sitter parsing.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

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

/// Validate syntax of source code for a given language.
pub fn validate_source(source: &str, lang: Language) -> Option<ValidationResult> {
    let (tree, _) = parse_source(source, lang)?;
    let root = tree.root_node();

    let mut errors = Vec::new();
    if root.has_error() {
        collect_errors(root, source, &mut errors);
    }

    Some(ValidationResult {
        valid: errors.is_empty(),
        errors,
        language: lang.to_string(),
    })
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
    validate_source(&source, lang).ok_or_else(|| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("failed to parse {}", path.display()),
        })
    })
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
        errors.push(SyntaxError {
            line: node.start_position().row + 1,
            column: node.start_position().column,
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
}
