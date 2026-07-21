//! Transitive impact analysis: what breaks if you change a symbol?

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use super::Language;
use super::refs::{RefKind, find_all_refs_in_source_with_tree};
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
            // SoftSkip multi-path (#1894): binary / invalid UTF-8 → skip.
            let source = crate::files::read_text_file(path)?;
            Some((path, display.as_str(), source, lang))
        })
        .collect();

    let mut visited: HashSet<(String, String)> = HashSet::new();
    // Seed with the target symbol across all files to prevent self-references.
    visited.insert((symbol_name.to_string(), String::new()));

    // Build a global reverse dependency map in a single pass over all files.
    // For each identifier reference found in the AST, record which symbol
    // contains it. This turns the per-depth O(files) scan into O(1) lookups.
    let mut reverse_deps: HashMap<String, Vec<ReverseRef>> = HashMap::new();

    for (_, display, source, lang) in &file_data {
        // Parse tree (needed for ref extraction).
        let Some((tree, _)) = super::parse_source(source, *lang) else {
            continue;
        };

        // Extract symbols for containment lookup.
        let symbols = extract_symbols(source, *lang);

        // Collect all identifier references in this file.
        let all_refs = find_all_refs_in_source_with_tree(source, &tree, display);

        for (ref_name, sym_ref) in all_refs {
            if sym_ref.kind != RefKind::Reference {
                continue;
            }
            let containing = find_containing_in(&symbols, sym_ref.line);
            let dependent_name = containing.unwrap_or_else(|| format!("<{}>", display));
            reverse_deps.entry(ref_name).or_default().push(ReverseRef {
                symbol: dependent_name,
                file: sym_ref.file,
                line: sym_ref.line,
                context: sym_ref.context,
            });
        }
    }

    find_dependents(symbol_name, &reverse_deps, max_depth, 1, &mut visited)
}

/// A reference edge in the reverse dependency map.
struct ReverseRef {
    /// The symbol that contains the reference (the dependent).
    symbol: String,
    /// File where the reference occurs.
    file: String,
    /// 1-based line number.
    line: usize,
    /// The source line (trimmed).
    context: String,
}

fn find_dependents(
    symbol_name: &str,
    reverse_deps: &HashMap<String, Vec<ReverseRef>>,
    max_depth: usize,
    current_depth: usize,
    visited: &mut HashSet<(String, String)>,
) -> Vec<ImpactNode> {
    let mut results = Vec::new();

    if let Some(refs) = reverse_deps.get(symbol_name) {
        for r in refs {
            let key = (r.symbol.clone(), r.file.clone());
            if visited.contains(&key) {
                continue;
            }
            visited.insert(key);

            let dependents = if current_depth < max_depth {
                find_dependents(
                    &r.symbol,
                    reverse_deps,
                    max_depth,
                    current_depth + 1,
                    visited,
                )
            } else {
                Vec::new()
            };

            results.push(ImpactNode {
                symbol: r.symbol.clone(),
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
        let symbols = extract_symbols(source, Language::Rust);
        let result = find_containing_in(&symbols, 2);
        assert_eq!(result, Some("foo".to_string()));
        let result = find_containing_in(&symbols, 6);
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

    #[test]
    fn find_dependents_same_name_different_files() {
        // Two files both have a function named "process" that references "target".
        // Both should appear in the results, not just the first one found.
        let mut reverse_deps: HashMap<String, Vec<ReverseRef>> = HashMap::new();
        reverse_deps.insert(
            "target".into(),
            vec![
                ReverseRef {
                    symbol: "process".into(),
                    file: "handler.rs".into(),
                    line: 5,
                    context: "target()".into(),
                },
                ReverseRef {
                    symbol: "process".into(),
                    file: "worker.rs".into(),
                    line: 10,
                    context: "target()".into(),
                },
            ],
        );
        let mut visited = HashSet::new();
        visited.insert(("target".into(), String::new()));
        let results = find_dependents("target", &reverse_deps, 1, 1, &mut visited);
        assert_eq!(
            results.len(),
            2,
            "both process() callers should be reported: {results:?}"
        );
        let files: Vec<&str> = results.iter().map(|n| n.file.as_str()).collect();
        assert!(files.contains(&"handler.rs"));
        assert!(files.contains(&"worker.rs"));
    }
}
