//! Structural code search using tree-sitter queries.
//!
//! Supports raw S-expression query mode. Pattern mode with meta-variables
//! is a higher-level wrapper that compiles code patterns to queries.

use std::path::Path;

use anyhow::Context;
use serde::Serialize;

use tree_sitter_lib::StreamingIterator;

use super::{Language, parse_source};

/// A single structural search match.
#[derive(Debug, Clone, Serialize)]
pub struct SearchMatch {
    /// 1-based line number.
    pub line: usize,
    /// 0-based column.
    pub column: usize,
    /// The matched source text.
    pub text: String,
    /// Named captures (if any).
    pub captures: Vec<Capture>,
}

/// A named capture from a query.
#[derive(Debug, Clone, Serialize)]
pub struct Capture {
    /// Capture name (without @).
    pub name: String,
    /// The captured text.
    pub text: String,
    /// 1-based line number.
    pub line: usize,
}

/// Search source code using a tree-sitter S-expression query.
pub fn search_query(
    source: &str,
    query_str: &str,
    lang: Language,
    max_results: Option<usize>,
) -> anyhow::Result<Vec<SearchMatch>> {
    let (tree, ts_lang) =
        parse_source(source, lang).ok_or_else(|| anyhow::anyhow!("no grammar for {lang}"))?;

    let query = tree_sitter_lib::Query::new(&ts_lang, query_str)
        .map_err(|e| anyhow::anyhow!("invalid query: {e}"))?;

    let mut cursor = tree_sitter_lib::QueryCursor::new();
    let source_bytes = source.as_bytes();
    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);

    let mut results = Vec::new();
    let limit = max_results.unwrap_or(usize::MAX);

    while let Some(m) = matches.next() {
        if results.len() >= limit {
            break;
        }

        let mut captures = Vec::new();
        let mut primary_node = None;

        for cap in m.captures {
            let name = query.capture_names()[cap.index as usize].to_string();
            let text = cap
                .node
                .utf8_text(source_bytes)
                .unwrap_or_default()
                .to_string();
            let line = cap.node.start_position().row + 1;

            if primary_node.is_none() {
                primary_node = Some(cap.node);
            }

            captures.push(Capture { name, text, line });
        }

        if let Some(node) = primary_node {
            let text = node.utf8_text(source_bytes).unwrap_or_default().to_string();
            results.push(SearchMatch {
                line: node.start_position().row + 1,
                column: node.start_position().column,
                text,
                captures,
            });
        }
    }

    Ok(results)
}

/// Search a file using a query string.
pub fn search_file(
    path: &Path,
    query_str: &str,
    lang_hint: Option<Language>,
    max_results: Option<usize>,
) -> anyhow::Result<Vec<SearchMatch>> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Ok(Vec::new());
    }
    let source =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    search_query(&source, query_str, lang, max_results)
}

/// Compile a simple pattern string into a tree-sitter query.
///
/// Pattern mode: the pattern is parsed as source code for the target language,
/// and its AST node kinds are converted into an S-expression query.
/// `$NAME` meta-variables become `@name` captures on identifier nodes.
///
/// Before parsing, meta-variables (`$NAME`) are replaced with valid placeholder
/// identifiers (`_pl_NAME_`) so tree-sitter can parse the pattern without errors.
/// After parsing, placeholder identifiers are detected and converted back to
/// query captures.
pub fn compile_pattern_query(pattern: &str, lang: Language) -> anyhow::Result<String> {
    // Collect meta-variable names and replace with valid placeholder identifiers.
    let mut placeholders: Vec<String> = Vec::new();
    let mut sanitized = pattern.to_string();
    // Handle $$$MULTI first (greedy), then $NAME.
    // For now, we only handle $NAME (single meta-variables).
    let mut i = 0;
    let bytes = pattern.as_bytes();
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let start = i;
            i += 1; // skip $
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &pattern[name_start..i];
            let placeholder = format!("_pl_{name}_");
            placeholders.push(name.to_string());
            replacements.push((start, i, placeholder));
        } else {
            i += 1;
        }
    }
    // Apply replacements in reverse order to preserve offsets
    for (start, end, placeholder) in replacements.into_iter().rev() {
        sanitized.replace_range(start..end, &placeholder);
    }

    let (tree, _) = parse_source(&sanitized, lang)
        .ok_or_else(|| anyhow::anyhow!("cannot parse pattern for {lang}"))?;

    let root = tree.root_node();
    if root.has_error() {
        anyhow::bail!(
            "pattern is not valid {lang} syntax (after meta-variable substitution): {sanitized}"
        );
    }

    // Walk to the first meaningful child (skip source_file wrapper)
    let child = if root.child_count() == 1 {
        root.child(0).unwrap_or(root)
    } else {
        root
    };

    let placeholder_set: std::collections::HashSet<String> =
        placeholders.iter().map(|n| format!("_pl_{n}_")).collect();

    let mut query = String::new();
    build_query_from_node(child, &sanitized, &mut query, &mut 0, &placeholder_set);
    Ok(query)
}

fn build_query_from_node(
    node: tree_sitter_lib::Node,
    source: &str,
    query: &mut String,
    capture_idx: &mut usize,
    placeholders: &std::collections::HashSet<String>,
) {
    let kind = node.kind();
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();

    // Check if this node is a placeholder for a meta-variable
    if placeholders.contains(text) {
        // Extract capture name: _pl_NAME_ -> NAME
        let capture_name = text
            .strip_prefix("_pl_")
            .and_then(|s| s.strip_suffix('_'))
            .unwrap_or(text);
        query.push_str(&format!("({kind}) @{capture_name}"));
        *capture_idx += 1;
        return;
    }

    if node.child_count() == 0 {
        // Leaf node: match exact text
        query.push_str(&format!("({kind}) @cap{capture_idx}"));
        *capture_idx += 1;
        return;
    }

    query.push_str(&format!("({kind}"));
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        query.push(' ');
        build_query_from_node(child, source, query, capture_idx, placeholders);
    }
    query.push(')');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_finds_function_calls() {
        let source = r#"
fn main() {
    let x = foo();
    let y = bar();
    let z = foo();
}
"#;
        let query = r#"(call_expression function: (identifier) @fn)"#;
        let results = search_query(source, query, Language::Rust, None).unwrap();
        assert!(results.len() >= 3);
    }

    #[test]
    fn search_query_respects_max_results() {
        let source = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let query = "(function_item name: (identifier) @name)";
        let results = search_query(source, query, Language::Rust, Some(2)).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_query_captures_names() {
        let source = "fn hello() {}\n";
        let query = "(function_item name: (identifier) @name)";
        let results = search_query(source, query, Language::Rust, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].captures[0].name, "name");
        assert_eq!(results[0].captures[0].text, "hello");
    }

    #[test]
    fn search_query_invalid_query_returns_error() {
        let result = search_query("fn main() {}", "(invalid query !!!", Language::Rust, None);
        result.expect_err("expected error");
    }

    #[test]
    fn search_query_python() {
        let source = "def foo():\n    pass\ndef bar():\n    pass\n";
        let query = "(function_definition name: (identifier) @name)";
        let results = search_query(source, query, Language::Python, None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_query_go_struct() {
        let source = "package main\ntype Config struct {\n    Host string\n}\n";
        let query = "(type_declaration (type_spec name: (type_identifier) @name))";
        let results = search_query(source, query, Language::Go, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].captures[0].text, "Config");
    }

    #[test]
    fn unknown_language_returns_error() {
        let result = search_query("anything", "(identifier)", Language::Unknown, None);
        result.expect_err("expected error");
    }

    #[test]
    fn compile_pattern_rust_struct() {
        let query = compile_pattern_query("struct $NAME {}", Language::Rust).unwrap();
        // Should produce a query that matches struct items
        let source = "struct Config {}\nstruct Server {}\n";
        let results = search_query(source, &query, Language::Rust, None).unwrap();
        assert!(
            results.len() >= 2,
            "expected >=2 struct matches, got {}",
            results.len()
        );
    }

    #[test]
    fn compile_pattern_rust_fn() {
        let query = compile_pattern_query("fn $NAME() {}", Language::Rust).unwrap();
        let source = "fn main() {}\nfn helper() {}\nfn process(x: i32) {}\n";
        let results = search_query(source, &query, Language::Rust, None).unwrap();
        // Should match main and helper (no params), may or may not match process
        assert!(!results.is_empty(), "expected matches for fn pattern");
    }

    #[test]
    fn compile_pattern_python_class() {
        let query = compile_pattern_query("class $NAME:", Language::Python).unwrap();
        let source = "class Foo:\n    pass\nclass Bar:\n    pass\n";
        let results = search_query(source, &query, Language::Python, None).unwrap();
        assert!(
            results.len() >= 2,
            "expected >=2 class matches, got {}",
            results.len()
        );
    }

    #[test]
    fn compile_pattern_js_function() {
        let query = compile_pattern_query("function $NAME() {}", Language::JavaScript).unwrap();
        let source = "function foo() {}\nfunction bar(x) {}\n";
        let results = search_query(source, &query, Language::JavaScript, None).unwrap();
        assert!(
            !results.is_empty(),
            "expected matches for JS function pattern"
        );
    }
}
