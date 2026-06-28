//! AST-aware symbol reordering within a file or scope.

use super::Language;
use super::symbols::{SymbolDef, SymbolKind, extract_symbols, find_symbol, full_symbol_span};

/// Strategy for reordering symbols.
#[derive(Debug, Clone)]
pub enum ReorderStrategy {
    /// Sort symbols alphabetically by name.
    Alphabetical,
    /// Sort symbols in reverse alphabetical order.
    Reverse,
    /// Types (structs, enums, traits, interfaces) first, then functions.
    KindFirst,
    /// Explicit ordering: symbols appear in the given order.
    Custom(Vec<String>),
}

/// Result of a reorder operation.
#[derive(Debug)]
pub struct ReorderResult {
    /// The full file content after reordering.
    pub content: String,
    /// Number of symbols that changed position.
    pub symbols_reordered: usize,
}

/// Reorder symbols within a file or a specific scope (module, impl block).
///
/// When `inside` is `Some`, only symbols inside that container are reordered;
/// the container itself and everything outside it remain in place.
pub fn reorder_symbols(
    source: &str,
    inside: Option<&str>,
    strategy: &ReorderStrategy,
    lang: Language,
) -> anyhow::Result<ReorderResult> {
    let all_symbols = extract_symbols(source, lang);
    let lines: Vec<&str> = source.lines().collect();

    let (scope_symbols, scope_start_0, scope_end_0) = if let Some(container) = inside {
        let parent = find_symbol(&all_symbols, container)
            .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found", container))?;
        // Find the opening brace line of the container
        let open_line_0 = find_opening_line(&lines, parent.start_line.saturating_sub(1));
        let close_line_0 = parent.end_line.min(lines.len()).saturating_sub(1);
        (&parent.children, open_line_0 + 1, close_line_0)
    } else {
        (&all_symbols as &Vec<SymbolDef>, 0usize, lines.len())
    };

    if scope_symbols.is_empty() {
        return Ok(ReorderResult {
            content: source.to_string(),
            symbols_reordered: 0,
        });
    }

    // Build spans for each symbol, including attrs/docs
    let mut spans: Vec<SymbolSpan> = scope_symbols
        .iter()
        .map(|sym| {
            let (full_start, full_end) = full_symbol_span(source, sym, lang);
            SymbolSpan {
                name: sym.name.clone(),
                kind: sym.kind,
                start_0: full_start.saturating_sub(1),
                end_0: full_end.min(lines.len()),
            }
        })
        .collect();

    // Sort according to strategy
    let original_order: Vec<String> = spans.iter().map(|s| s.name.clone()).collect();
    sort_spans(&mut spans, strategy)?;
    let new_order: Vec<String> = spans.iter().map(|s| s.name.clone()).collect();

    // Count how many changed position
    let symbols_reordered = original_order
        .iter()
        .zip(new_order.iter())
        .filter(|(a, b)| a != b)
        .count();

    if symbols_reordered == 0 {
        return Ok(ReorderResult {
            content: source.to_string(),
            symbols_reordered: 0,
        });
    }

    // Rebuild the scope section with symbols in the new order.
    // Collect the text of each symbol (spans are already in new order after sort).
    let mut sym_texts: Vec<(&str, String)> = Vec::new();
    for span in &spans {
        let text: String = lines[span.start_0..span.end_0].join("\n");
        sym_texts.push((&span.name, text));
    }

    // Find the positional boundaries of all symbols (by line index).
    let first_sym_start = spans
        .iter()
        .map(|s| s.start_0)
        .min()
        .unwrap_or(scope_start_0);
    let last_sym_end = spans.iter().map(|s| s.end_0).max().unwrap_or(scope_end_0);

    // Now rebuild: before-scope + scope-prefix + reordered symbols + scope-suffix + after-scope
    let mut result = String::new();

    // Lines before the scope (container preamble when using `inside`)
    for line in &lines[..scope_start_0] {
        result.push_str(line);
        result.push('\n');
    }

    // Scope prefix: non-symbol lines inside the scope before the first symbol
    // (use statements, module-level comments, extern crate, etc.)
    for line in &lines[scope_start_0..first_sym_start] {
        result.push_str(line);
        result.push('\n');
    }

    // Emit symbols in new order with blank line separators
    for (i, (_name, text)) in sym_texts.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(text);
        result.push('\n');
    }

    // Scope suffix: non-symbol lines inside the scope after the last symbol
    for line in &lines[last_sym_end..scope_end_0] {
        result.push_str(line);
        result.push('\n');
    }

    // Lines after the scope (closing brace, trailing content)
    for line in &lines[scope_end_0..] {
        result.push_str(line);
        result.push('\n');
    }

    // Preserve trailing newline behavior
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    Ok(ReorderResult {
        content: result,
        symbols_reordered,
    })
}

#[derive(Debug, Clone)]
struct SymbolSpan {
    name: String,
    kind: SymbolKind,
    start_0: usize, // 0-based line index
    end_0: usize,   // 0-based exclusive end
}

fn sort_spans(spans: &mut [SymbolSpan], strategy: &ReorderStrategy) -> anyhow::Result<()> {
    match strategy {
        ReorderStrategy::Alphabetical => {
            spans.sort_by_key(|a| a.name.to_lowercase());
        }
        ReorderStrategy::Reverse => {
            spans.sort_by_key(|b| std::cmp::Reverse(b.name.to_lowercase()));
        }
        ReorderStrategy::KindFirst => {
            spans.sort_by(|a, b| {
                let ka = kind_priority(a.kind);
                let kb = kind_priority(b.kind);
                ka.cmp(&kb)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        }
        ReorderStrategy::Custom(order) => {
            spans.sort_by(|a, b| {
                let ia = order
                    .iter()
                    .position(|n| *n == a.name)
                    .unwrap_or(usize::MAX);
                let ib = order
                    .iter()
                    .position(|n| *n == b.name)
                    .unwrap_or(usize::MAX);
                ia.cmp(&ib)
            });
        }
    }
    Ok(())
}

/// Priority for kind-first ordering: types before functions.
fn kind_priority(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Struct => 0,
        SymbolKind::Enum => 1,
        SymbolKind::Trait | SymbolKind::Interface => 2,
        SymbolKind::Type => 3,
        SymbolKind::Const => 4,
        SymbolKind::Class => 5,
        SymbolKind::Impl => 6,
        SymbolKind::Module => 7,
        SymbolKind::Function => 8,
        SymbolKind::Method => 9,
    }
}

/// Find the line with the opening brace of a container (0-based index).
fn find_opening_line(lines: &[&str], start_0: usize) -> usize {
    for (i, line) in lines.iter().enumerate().skip(start_0) {
        let trimmed = line.trim();
        if trimmed.ends_with('{') || trimmed.ends_with(':') || trimmed.ends_with(":{") {
            return i;
        }
    }
    start_0
}

/// Parse a reorder strategy from a `serde_json::Value`.
///
/// - String `"alphabetical"`, `"reverse"`, `"kind-first"` -> named strategy
/// - Array of strings -> `Custom` order
pub fn parse_strategy(value: &serde_json::Value) -> anyhow::Result<ReorderStrategy> {
    match value {
        serde_json::Value::String(s) => match s.as_str() {
            "alphabetical" => Ok(ReorderStrategy::Alphabetical),
            "reverse" => Ok(ReorderStrategy::Reverse),
            "kind-first" | "kind_first" => Ok(ReorderStrategy::KindFirst),
            other => anyhow::bail!("unknown reorder strategy: '{other}'"),
        },
        serde_json::Value::Array(arr) => {
            let names: Result<Vec<String>, _> = arr
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(String::from)
                        .ok_or_else(|| anyhow::anyhow!("custom order items must be strings"))
                })
                .collect();
            Ok(ReorderStrategy::Custom(names?))
        }
        _ => anyhow::bail!("'order' must be a string or array of strings"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_alphabetical() {
        let source = "fn charlie() {}\n\nfn alpha() {}\n\nfn bravo() {}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::Alphabetical, Language::Rust).unwrap();
        let alpha_pos = result.content.find("fn alpha").unwrap();
        let bravo_pos = result.content.find("fn bravo").unwrap();
        let charlie_pos = result.content.find("fn charlie").unwrap();
        assert!(alpha_pos < bravo_pos);
        assert!(bravo_pos < charlie_pos);
        assert_eq!(result.symbols_reordered, 3);
    }

    #[test]
    fn reorder_reverse() {
        let source = "fn alpha() {}\n\nfn bravo() {}\n\nfn charlie() {}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::Reverse, Language::Rust).unwrap();
        let alpha_pos = result.content.find("fn alpha").unwrap();
        let bravo_pos = result.content.find("fn bravo").unwrap();
        let charlie_pos = result.content.find("fn charlie").unwrap();
        assert!(charlie_pos < bravo_pos);
        assert!(bravo_pos < alpha_pos);
    }

    #[test]
    fn reorder_kind_first() {
        let source =
            "fn do_stuff() {}\n\nstruct Config {\n    x: i32,\n}\n\nenum Mode {\n    A,\n}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::KindFirst, Language::Rust).unwrap();
        let struct_pos = result.content.find("struct Config").unwrap();
        let enum_pos = result.content.find("enum Mode").unwrap();
        let fn_pos = result.content.find("fn do_stuff").unwrap();
        assert!(struct_pos < enum_pos);
        assert!(enum_pos < fn_pos);
    }

    #[test]
    fn reorder_custom() {
        let source = "fn charlie() {}\n\nfn alpha() {}\n\nfn bravo() {}\n";
        let order = vec!["bravo".into(), "charlie".into(), "alpha".into()];
        let result = reorder_symbols(
            source,
            None,
            &ReorderStrategy::Custom(order),
            Language::Rust,
        )
        .unwrap();
        let alpha_pos = result.content.find("fn alpha").unwrap();
        let bravo_pos = result.content.find("fn bravo").unwrap();
        let charlie_pos = result.content.find("fn charlie").unwrap();
        assert!(bravo_pos < charlie_pos);
        assert!(charlie_pos < alpha_pos);
    }

    #[test]
    fn reorder_inside_module() {
        let source = "fn outside() {}\n\nmod tests {\n    fn zebra() {}\n    fn apple() {}\n}\n";
        let result = reorder_symbols(
            source,
            Some("tests"),
            &ReorderStrategy::Alphabetical,
            Language::Rust,
        )
        .unwrap();
        assert!(result.content.contains("fn outside()")); // unchanged
        let apple_pos = result.content.find("fn apple").unwrap();
        let zebra_pos = result.content.find("fn zebra").unwrap();
        assert!(apple_pos < zebra_pos);
    }

    #[test]
    fn reorder_no_op_when_already_sorted() {
        let source = "fn alpha() {}\n\nfn bravo() {}\n\nfn charlie() {}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::Alphabetical, Language::Rust).unwrap();
        assert_eq!(result.symbols_reordered, 0);
        assert_eq!(result.content, source);
    }

    #[test]
    fn reorder_preserves_attributes() {
        let source = "#[test]\nfn zebra() {}\n\n/// Doc for alpha.\n#[cfg(test)]\nfn alpha() {}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::Alphabetical, Language::Rust).unwrap();
        // alpha (with its doc + cfg) should come first
        let alpha_pos = result.content.find("fn alpha").unwrap();
        let zebra_pos = result.content.find("fn zebra").unwrap();
        assert!(alpha_pos < zebra_pos);
        // attributes should still be present
        assert!(result.content.contains("/// Doc for alpha."));
        assert!(result.content.contains("#[cfg(test)]"));
        assert!(result.content.contains("#[test]"));
    }

    #[test]
    fn reorder_symbol_not_found_inside() {
        let source = "fn foo() {}\n";
        let result = reorder_symbols(
            source,
            Some("nonexistent"),
            &ReorderStrategy::Alphabetical,
            Language::Rust,
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_strategy_string() {
        let v = serde_json::json!("alphabetical");
        assert!(matches!(
            parse_strategy(&v).unwrap(),
            ReorderStrategy::Alphabetical
        ));
        let v = serde_json::json!("reverse");
        assert!(matches!(
            parse_strategy(&v).unwrap(),
            ReorderStrategy::Reverse
        ));
        let v = serde_json::json!("kind-first");
        assert!(matches!(
            parse_strategy(&v).unwrap(),
            ReorderStrategy::KindFirst
        ));
    }

    #[test]
    fn parse_strategy_array() {
        let v = serde_json::json!(["b", "a", "c"]);
        let strategy = parse_strategy(&v).unwrap();
        assert!(
            matches!(strategy, ReorderStrategy::Custom(ref names) if names == &["b", "a", "c"])
        );
    }

    #[test]
    fn parse_strategy_kind_first_underscore() {
        let v = serde_json::json!("kind_first");
        assert!(matches!(
            parse_strategy(&v).unwrap(),
            ReorderStrategy::KindFirst
        ));
    }

    #[test]
    fn parse_strategy_unknown_string() {
        let v = serde_json::json!("bogus");
        assert!(parse_strategy(&v).is_err());
    }

    #[test]
    fn parse_strategy_array_non_string_item() {
        let v = serde_json::json!(["a", 42, "c"]);
        assert!(parse_strategy(&v).is_err());
    }

    #[test]
    fn parse_strategy_invalid() {
        let v = serde_json::json!(42);
        assert!(parse_strategy(&v).is_err());
    }

    // Regression: reorder must preserve non-symbol content (use statements,
    // module-level comments) that appears before the first symbol.
    #[test]
    fn reorder_preserves_use_statements() {
        let source =
            "use std::collections::HashMap;\n\n// Module doc\nfn zebra() {}\n\nfn alpha() {}\n";
        let result =
            reorder_symbols(source, None, &ReorderStrategy::Alphabetical, Language::Rust).unwrap();
        assert!(
            result.content.contains("use std::collections::HashMap;"),
            "use statement should be preserved: {}",
            result.content
        );
        assert!(
            result.content.contains("// Module doc"),
            "module comment should be preserved: {}",
            result.content
        );
        let alpha_pos = result.content.find("fn alpha").unwrap();
        let zebra_pos = result.content.find("fn zebra").unwrap();
        assert!(alpha_pos < zebra_pos, "alpha should come before zebra");
        // use statement should come before any function
        let use_pos = result.content.find("use std").unwrap();
        assert!(
            use_pos < alpha_pos,
            "use statement should remain before symbols"
        );
    }
}
