//! Transitive impact analysis: what breaks if you change a symbol?

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use super::Language;
use super::refs::{RefKind, find_refs_in_source};
use super::symbols::extract_symbols;

/// A node in the impact tree.
#[derive(Debug, Clone, Serialize)]
pub struct ImpactNode {
    /// Symbol name.
    pub symbol: String,
    /// File where the reference occurs.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// The source line (trimmed).
    pub context: String,
    /// Transitive dependents (callers of this caller).
    pub dependents: Vec<ImpactNode>,
}

/// Compute the transitive impact of changing a symbol.
pub fn compute_impact(
    symbol_name: &str,
    files: &[(impl AsRef<Path>, String)],
    max_depth: usize,
) -> Vec<ImpactNode> {
    // Collect all file sources
    let file_data: Vec<(&Path, &str, String, Language)> = files
        .iter()
        .filter_map(|(path, display)| {
            let path = path.as_ref();
            let lang = Language::from_path(path);
            if !lang.has_grammar() {
                return None;
            }
            let source = std::fs::read_to_string(path).ok()?;
            Some((path, display.as_str(), source, lang))
        })
        .collect();

    let mut visited = HashSet::new();
    visited.insert(symbol_name.to_string());

    find_dependents(symbol_name, &file_data, max_depth, 1, &mut visited)
}

fn find_dependents(
    symbol_name: &str,
    file_data: &[(&Path, &str, String, Language)],
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<String>,
) -> Vec<ImpactNode> {
    let mut results = Vec::new();

    for (_, display, source, lang) in file_data {
        let refs = find_refs_in_source(source, symbol_name, *lang, display);
        for r in &refs {
            if r.kind != RefKind::Reference {
                continue;
            }

            // Find which symbol contains this reference line
            let containing = find_containing_symbol(source, *lang, r.line);
            let dependent_name = containing.unwrap_or_else(|| format!("<{}>", display));

            if visited.contains(&dependent_name) {
                continue;
            }
            visited.insert(dependent_name.clone());

            let dependents = if current_depth < max_depth {
                find_dependents(
                    &dependent_name,
                    file_data,
                    max_depth,
                    current_depth + 1,
                    visited,
                )
            } else {
                Vec::new()
            };

            results.push(ImpactNode {
                symbol: dependent_name,
                file: r.file.clone(),
                line: r.line,
                context: r.context.clone(),
                dependents,
            });
        }
    }

    // Deduplicate by (symbol, file) keeping the first occurrence
    let mut seen = HashSet::new();
    results.retain(|node| seen.insert((node.symbol.clone(), node.file.clone())));

    results
}

/// Find the symbol that contains a given line number.
fn find_containing_symbol(source: &str, lang: Language, line: usize) -> Option<String> {
    let symbols = extract_symbols(source, lang);
    find_containing_in(&symbols, line)
}

fn find_containing_in(symbols: &[super::symbols::SymbolDef], line: usize) -> Option<String> {
    for sym in symbols {
        if line >= sym.start_line && line <= sym.end_line {
            // Check children first for more specific match
            if let Some(child_name) = find_containing_in(&sym.children, line) {
                return Some(child_name);
            }
            return Some(sym.name.clone());
        }
    }
    None
}

/// Render impact tree as human-readable text.
pub fn render_impact_tree(symbol: &str, nodes: &[ImpactNode], indent: usize) -> String {
    let mut out = String::new();
    if indent == 0 {
        out.push_str(&format!("{symbol}\n"));
    }
    let pad = "   ".repeat(indent);
    for node in nodes {
        out.push_str(&format!(
            "{pad}<- {} ({}:{})\n",
            node.symbol, node.file, node.line
        ));
        if !node.dependents.is_empty() {
            out.push_str(&render_impact_tree(
                &node.symbol,
                &node.dependents,
                indent + 1,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_containing_symbol_works() {
        let source = "fn foo() {\n    let x = 1;\n}\n\nfn bar() {\n    let y = 2;\n}\n";
        let result = find_containing_symbol(source, Language::Rust, 2);
        assert_eq!(result, Some("foo".to_string()));
        let result = find_containing_symbol(source, Language::Rust, 6);
        assert_eq!(result, Some("bar".to_string()));
    }

    #[test]
    fn render_impact_tree_formats_correctly() {
        let nodes = vec![ImpactNode {
            symbol: "caller".into(),
            file: "main.rs".into(),
            line: 10,
            context: "caller()".into(),
            dependents: vec![],
        }];
        let output = render_impact_tree("target", &nodes, 0);
        assert!(output.contains("target\n"));
        assert!(output.contains("<- caller (main.rs:10)"));
    }

    #[test]
    fn empty_impact_returns_empty() {
        let nodes: Vec<ImpactNode> = Vec::new();
        let output = render_impact_tree("target", &nodes, 0);
        assert_eq!(output, "target\n");
    }
}
