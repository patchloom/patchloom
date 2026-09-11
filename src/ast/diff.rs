//! Structural diff: compare symbols between two versions of a file.

use serde::Serialize;

use super::Language;
use super::ParseFailure;
use super::symbols::{SymbolDef, try_extract_symbols};

/// A single structural change.
#[derive(Debug, Clone, Serialize)]
pub struct StructuralChange {
    /// Symbol name.
    pub name: String,
    /// Kind of symbol (fn, struct, etc.).
    pub kind: String,
    /// What changed.
    pub change: ChangeKind,
    /// 1-based line number (in the "new" version, or the "old" if removed).
    pub line: usize,
    /// Optional detail (e.g. "parameter added").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Classification of change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    SignatureChanged,
    BodyChanged,
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Added => write!(f, "+"),
            Self::Removed => write!(f, "-"),
            Self::SignatureChanged => write!(f, "~"),
            Self::BodyChanged => write!(f, "~"),
        }
    }
}

/// Compare two versions of source code and return structural changes.
///
/// Returns an empty list if either side has no grammar or the parse
/// deadline fires. Prefer [`structural_diff_or_timeout`] when timeout
/// must not look like "no structural changes".
pub fn structural_diff(
    old_source: &str,
    new_source: &str,
    lang: Language,
) -> Vec<StructuralChange> {
    try_structural_diff(old_source, new_source, lang).unwrap_or_default()
}

/// Compare two versions, mapping a parse deadline to [`crate::exit::ParseTimeoutError`].
///
/// Missing grammar is an empty list (same as [`structural_diff`]). Library
/// hosts that must distinguish a 5s parse deadline from "no structural
/// changes" should call this instead (#2446).
///
/// ```
/// use patchloom::ast::diff::structural_diff_or_timeout;
/// use patchloom::ast::Language;
///
/// let changes = structural_diff_or_timeout("fn a() {}", "fn a() {}", Language::Rust).unwrap();
/// assert!(changes.is_empty());
/// ```
pub fn structural_diff_or_timeout(
    old_source: &str,
    new_source: &str,
    lang: Language,
) -> anyhow::Result<Vec<StructuralChange>> {
    match try_structural_diff(old_source, new_source, lang) {
        Ok(changes) => Ok(changes),
        Err(ParseFailure::DeadlineExceeded) => Err(crate::exit::ParseTimeoutError {
            msg: format!("parse deadline exceeded for {lang}"),
        }
        .into()),
        Err(ParseFailure::NoGrammar) => Ok(Vec::new()),
    }
}

/// Like [`structural_diff`], but a parse deadline on either side is
/// [`ParseFailure::DeadlineExceeded`] instead of an empty change list.
pub(crate) fn try_structural_diff(
    old_source: &str,
    new_source: &str,
    lang: Language,
) -> Result<Vec<StructuralChange>, ParseFailure> {
    let old_symbols = try_extract_symbols(old_source, lang)?;
    let new_symbols = try_extract_symbols(new_source, lang)?;

    let mut changes = Vec::new();
    diff_symbol_lists(
        &old_symbols,
        &new_symbols,
        old_source,
        new_source,
        &mut changes,
    );
    Ok(changes)
}

fn diff_symbol_lists(
    old: &[SymbolDef],
    new: &[SymbolDef],
    old_source: &str,
    new_source: &str,
    changes: &mut Vec<StructuralChange>,
) {
    // Build multimaps to handle duplicate symbol names (e.g., multiple
    // `impl Foo` blocks or overloaded functions).
    let mut old_map: std::collections::HashMap<&str, Vec<&SymbolDef>> =
        std::collections::HashMap::new();
    for s in old {
        old_map.entry(s.name.as_str()).or_default().push(s);
    }
    let mut new_map: std::collections::HashMap<&str, Vec<&SymbolDef>> =
        std::collections::HashMap::new();
    for s in new {
        new_map.entry(s.name.as_str()).or_default().push(s);
    }

    // Added symbols (in new but not old)
    for sym in new {
        if !old_map.contains_key(sym.name.as_str()) {
            changes.push(StructuralChange {
                name: sym.name.clone(),
                kind: sym.kind.to_string(),
                change: ChangeKind::Added,
                line: sym.start_line,
                detail: Some(format!("added at line {}", sym.start_line)),
            });
        }
    }

    // Removed symbols (in old but not new)
    for sym in old {
        if !new_map.contains_key(sym.name.as_str()) {
            changes.push(StructuralChange {
                name: sym.name.clone(),
                kind: sym.kind.to_string(),
                change: ChangeKind::Removed,
                line: sym.start_line,
                detail: Some(format!("was at line {}", sym.start_line)),
            });
        }
    }

    // Changed symbols: match same-name symbols by position order.
    for (name, new_syms) in &new_map {
        if let Some(old_syms) = old_map.get(name) {
            // Count difference: extra new ones are additions, extra old ones are removals.
            let paired = old_syms.len().min(new_syms.len());
            for i in 0..paired {
                let old_sym = old_syms[i];
                let new_sym = new_syms[i];
                if old_sym.signature != new_sym.signature {
                    changes.push(StructuralChange {
                        name: new_sym.name.clone(),
                        kind: new_sym.kind.to_string(),
                        change: ChangeKind::SignatureChanged,
                        line: new_sym.start_line,
                        detail: Some(format!("was: {}", old_sym.signature)),
                    });
                } else {
                    let old_body = extract_body(old_source, old_sym);
                    let new_body = extract_body(new_source, new_sym);
                    if old_body != new_body {
                        changes.push(StructuralChange {
                            name: new_sym.name.clone(),
                            kind: new_sym.kind.to_string(),
                            change: ChangeKind::BodyChanged,
                            line: new_sym.start_line,
                            detail: Some(format!(
                                "lines {}-{}",
                                new_sym.start_line, new_sym.end_line
                            )),
                        });
                    }
                }
                diff_symbol_lists(
                    &old_sym.children,
                    &new_sym.children,
                    old_source,
                    new_source,
                    changes,
                );
            }
            // Extra new symbols beyond what old had.
            for new_sym in &new_syms[paired..] {
                changes.push(StructuralChange {
                    name: new_sym.name.clone(),
                    kind: new_sym.kind.to_string(),
                    change: ChangeKind::Added,
                    line: new_sym.start_line,
                    detail: Some(format!("added at line {}", new_sym.start_line)),
                });
            }
            // Extra old symbols that were removed.
            for old_sym in &old_syms[paired..] {
                changes.push(StructuralChange {
                    name: old_sym.name.clone(),
                    kind: old_sym.kind.to_string(),
                    change: ChangeKind::Removed,
                    line: old_sym.start_line,
                    detail: Some(format!("was at line {}", old_sym.start_line)),
                });
            }
        }
    }
}

fn extract_body<'a>(source: &'a str, sym: &SymbolDef) -> &'a str {
    let lines: Vec<&str> = crate::ops::file::text_lines(source).collect();
    let start = sym.start_line.saturating_sub(1);
    let end = sym.end_line.min(lines.len());
    if start >= lines.len() || start >= end {
        return "";
    }
    let offsets = crate::ast::symbols::compute_line_byte_offsets(source);
    let start_byte = if start < offsets.len() {
        offsets[start]
    } else {
        source.len()
    };
    let end_byte = if end < offsets.len() {
        offsets[end]
    } else {
        source.len()
    };
    &source[start_byte..end_byte]
}

/// Render changes as human-readable text.
pub fn render_changes(file: &str, changes: &[StructuralChange]) -> String {
    let mut out = format!("{file}\n");
    for c in changes {
        let detail = c.detail.as_deref().unwrap_or("");
        out.push_str(&format!(
            "  {} {} {}: {} [{}]\n",
            c.change, c.kind, c.name, detail, c.line
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_added_function() {
        let old = "fn foo() {}\n";
        let new = "fn foo() {}\nfn bar() {}\n";
        let changes = structural_diff(old, new, Language::Rust);
        assert!(
            changes
                .iter()
                .any(|c| c.name == "bar" && c.change == ChangeKind::Added)
        );
    }

    #[test]
    fn detects_removed_function() {
        let old = "fn foo() {}\nfn bar() {}\n";
        let new = "fn foo() {}\n";
        let changes = structural_diff(old, new, Language::Rust);
        assert!(
            changes
                .iter()
                .any(|c| c.name == "bar" && c.change == ChangeKind::Removed)
        );
    }

    #[test]
    fn detects_signature_change() {
        let old = "fn foo() {}\n";
        let new = "fn foo(x: i32) {}\n";
        let changes = structural_diff(old, new, Language::Rust);
        assert!(
            changes
                .iter()
                .any(|c| c.name == "foo" && c.change == ChangeKind::SignatureChanged)
        );
    }

    #[test]
    fn detects_body_change() {
        let old = "fn foo() {\n    let x = 1;\n}\n";
        let new = "fn foo() {\n    let x = 2;\n}\n";
        let changes = structural_diff(old, new, Language::Rust);
        assert!(
            changes
                .iter()
                .any(|c| c.name == "foo" && c.change == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn no_changes_returns_empty() {
        let source = "fn foo() {\n    let x = 1;\n}\n";
        let changes = structural_diff(source, source, Language::Rust);
        assert!(changes.is_empty());
    }

    #[test]
    fn extract_body_crlf() {
        let source = "line1\r\nline2\r\nline3\r\n";
        let sym = SymbolDef {
            name: "test".into(),
            kind: crate::ast::symbols::SymbolKind::Function,
            start_line: 2,
            end_line: 3,
            signature: String::new(),
            children: vec![],
            depth: 0,
        };
        let body = extract_body(source, &sym);
        // Lines 2-3 inclusive = "line2\r\nline3\r\n"
        assert_eq!(body, "line2\r\nline3\r\n");
    }

    #[test]
    fn python_diff() {
        let old = "def hello():\n    pass\n";
        let new = "def hello():\n    print('hi')\ndef world():\n    pass\n";
        let changes = structural_diff(old, new, Language::Python);
        assert!(
            changes
                .iter()
                .any(|c| c.name == "world" && c.change == ChangeKind::Added)
        );
        assert!(
            changes
                .iter()
                .any(|c| c.name == "hello" && c.change == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn duplicate_symbol_names_not_lost() {
        // Two functions with the same name; both should be tracked.
        let old = "def run():\n    pass\ndef run():\n    pass\n";
        let new = "def run():\n    pass\ndef run():\n    return 1\ndef run():\n    return 2\n";
        let changes = structural_diff(old, new, Language::Python);
        // The second `run` body changed.
        assert!(
            changes
                .iter()
                .any(|c| c.name == "run" && c.change == ChangeKind::BodyChanged),
            "second run body change should be detected: {changes:?}"
        );
        // The third `run` is new (added).
        assert!(
            changes
                .iter()
                .any(|c| c.name == "run" && c.change == ChangeKind::Added),
            "third run should be detected as added: {changes:?}"
        );
    }

    // Unique: public or_timeout twin peels parse_timeout; empty-vec API stays empty (#2446).
    #[test]
    fn structural_diff_or_timeout_deadline_is_parse_timeout() {
        let source = crate::ast::nested_rust_source_for_timeout(80_000);
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let err = structural_diff_or_timeout(&source, &source, Language::Rust).unwrap_err();
        assert_eq!(
            crate::fallback::error_kind_str(&err),
            Some("parse_timeout"),
            "expected parse_timeout, got {err}"
        );
    }

    #[test]
    fn structural_diff_deadline_stays_empty() {
        let source = crate::ast::nested_rust_source_for_timeout(80_000);
        let _guard = crate::ast::ParseTimeoutGuard::set(std::time::Duration::from_millis(1));
        let changes = structural_diff(&source, &source, Language::Rust);
        assert!(
            changes.is_empty(),
            "empty-vec structural_diff must stay empty on deadline"
        );
    }

    #[test]
    fn structural_diff_or_timeout_unknown_lang_is_empty() {
        let changes = structural_diff_or_timeout("old", "new", Language::Unknown).unwrap();
        assert!(
            changes.is_empty(),
            "missing grammar must stay empty, not parse_timeout"
        );
    }
}
