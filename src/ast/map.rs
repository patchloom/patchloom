//! Repository map with PageRank ranking and token-budget-aware output.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use super::Language;
use super::refs::{RefKind, find_refs_in_source_with_tree};
use super::symbols::{SymbolDef, SymbolKind, extract_symbols};

/// A symbol entry in the repository map.
#[derive(Debug, Clone, Serialize)]
pub struct MapEntry {
    /// File path (relative).
    pub file: String,
    /// Symbol name.
    pub name: String,
    /// Symbol kind.
    pub kind: String,
    /// One-line signature.
    pub signature: String,
    /// 1-based line number.
    pub line: usize,
    /// PageRank score (higher = more important).
    pub score: f64,
}

/// Options for generating a repository map.
pub struct MapOptions<'a> {
    /// Maximum approximate token count for output.
    pub max_tokens: usize,
    /// File paths to boost (their symbols get higher initial rank).
    pub focus: &'a [String],
    /// Symbol names to boost.
    pub boost: &'a [String],
}

impl Default for MapOptions<'_> {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            focus: &[],
            boost: &[],
        }
    }
}

/// Collected file data for graph building.
struct FileData {
    path: String,
    source: String,
    lang: Language,
    symbols: Vec<SymbolDef>,
}

/// Generate a ranked repository map from a directory of source files.
pub fn generate_map(files: &[(impl AsRef<Path>, String)], opts: &MapOptions<'_>) -> Vec<MapEntry> {
    // Phase 1: Parse all files and extract symbols
    let file_data: Vec<FileData> = files
        .iter()
        .filter_map(|(path, display)| {
            let path = path.as_ref();
            let lang = Language::from_path(path);
            if !lang.has_grammar() {
                return None;
            }
            let source = std::fs::read_to_string(path).ok()?;
            let symbols = extract_symbols(&source, lang);
            if symbols.is_empty() {
                return None;
            }
            Some(FileData {
                path: display.clone(),
                source,
                lang,
                symbols,
            })
        })
        .collect();

    // Phase 2: Build reference graph and compute PageRank
    let mut all_symbols: Vec<(String, String, SymbolKind, String, usize)> = Vec::new(); // (file, name, kind, sig, line)
    let mut name_to_idx: HashMap<String, Vec<usize>> = HashMap::new();

    for fd in &file_data {
        collect_flat_symbols(&fd.symbols, &fd.path, &mut all_symbols, &mut name_to_idx);
    }

    let n = all_symbols.len();
    if n == 0 {
        return Vec::new();
    }

    // Build adjacency: edges[i] = set of j where symbol i references symbol j
    let mut edges: Vec<HashSet<usize>> = vec![HashSet::new(); n];

    // Pre-parse all files once; reuse trees for all symbol lookups.
    let tree_cache: HashMap<&str, tree_sitter_lib::Tree> = file_data
        .iter()
        .filter_map(|fd| {
            let (tree, _) = super::parse_source(&fd.source, fd.lang)?;
            Some((fd.path.as_str(), tree))
        })
        .collect();

    for fd in &file_data {
        let Some(tree) = tree_cache.get(fd.path.as_str()) else {
            continue;
        };
        for (idx, (_, name, _, _, _)) in all_symbols.iter().enumerate() {
            if all_symbols[idx].0 != fd.path {
                continue;
            }
            // Find what this symbol references in other symbols
            let refs = find_refs_in_source_with_tree(&fd.source, name, tree, &fd.path);
            for r in &refs {
                if r.kind == RefKind::Reference {
                    // This file references `name`, add edges from referencing symbols
                    // to the definition of `name` in other files
                    if let Some(targets) = name_to_idx.get(name.as_str()) {
                        for &target in targets {
                            if target != idx {
                                edges[idx].insert(target);
                            }
                        }
                    }
                }
            }
        }
    }

    // Also find cross-file references: for each file, scan for identifiers
    // that match symbol names in other files
    for fd in &file_data {
        let Some(tree) = tree_cache.get(fd.path.as_str()) else {
            continue;
        };
        for (name, indices) in &name_to_idx {
            // Skip symbols defined in this file
            if indices.iter().all(|&i| all_symbols[i].0 == fd.path) {
                continue;
            }
            let refs = find_refs_in_source_with_tree(&fd.source, name, tree, &fd.path);
            if refs.iter().any(|r| r.kind == RefKind::Reference) {
                // Find symbols in this file that could be the referencing context
                let file_symbols: Vec<usize> =
                    (0..n).filter(|&i| all_symbols[i].0 == fd.path).collect();
                for &src in &file_symbols {
                    for &dst in indices {
                        if all_symbols[dst].0 != fd.path {
                            edges[src].insert(dst);
                        }
                    }
                }
            }
        }
    }

    // Phase 3: PageRank
    let scores = pagerank(&edges, n, opts, &all_symbols);

    // Phase 4: Build entries sorted by score
    let mut entries: Vec<MapEntry> = all_symbols
        .into_iter()
        .enumerate()
        .map(|(i, (file, name, kind, sig, line))| MapEntry {
            file,
            name,
            kind: kind.to_string(),
            signature: sig,
            line,
            score: scores[i],
        })
        .collect();

    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Phase 5: Token budget selection
    truncate_to_budget(&mut entries, opts.max_tokens);

    entries
}

fn collect_flat_symbols(
    symbols: &[SymbolDef],
    file: &str,
    out: &mut Vec<(String, String, SymbolKind, String, usize)>,
    name_to_idx: &mut HashMap<String, Vec<usize>>,
) {
    for sym in symbols {
        let idx = out.len();
        name_to_idx.entry(sym.name.clone()).or_default().push(idx);
        out.push((
            file.to_string(),
            sym.name.clone(),
            sym.kind,
            sym.signature.clone(),
            sym.start_line,
        ));
        collect_flat_symbols(&sym.children, file, out, name_to_idx);
    }
}

fn pagerank(
    edges: &[HashSet<usize>],
    n: usize,
    opts: &MapOptions<'_>,
    symbols: &[(String, String, SymbolKind, String, usize)],
) -> Vec<f64> {
    let damping = 0.85;
    let max_iterations = 100;
    let convergence_threshold = 1e-6;

    // Initialize with personalization bias
    let mut scores = vec![1.0 / n as f64; n];

    // Apply focus/boost to initial scores
    for (i, (file, name, _, _, _)) in symbols.iter().enumerate() {
        if opts.focus.iter().any(|f| file.contains(f.as_str())) {
            scores[i] *= 3.0;
        }
        if opts.boost.iter().any(|b| b == name) {
            scores[i] *= 5.0;
        }
    }

    // Normalize
    let sum: f64 = scores.iter().sum();
    if sum > 0.0 {
        for s in &mut scores {
            *s /= sum;
        }
    }

    let personalization = scores.clone();

    // Compute out-degree
    let out_degree: Vec<usize> = edges.iter().map(|e| e.len()).collect();

    for _ in 0..max_iterations {
        let mut new_scores = vec![0.0; n];

        for (i, edge_list) in edges.iter().enumerate() {
            if out_degree[i] > 0 {
                let share = scores[i] / out_degree[i] as f64;
                for &j in edge_list {
                    new_scores[j] += share;
                }
            }
        }

        // Apply damping with personalization
        for i in 0..n {
            new_scores[i] = damping * new_scores[i] + (1.0 - damping) * personalization[i];
        }

        // Check convergence: L1 norm of score change
        let delta: f64 = scores
            .iter()
            .zip(new_scores.iter())
            .map(|(old, new)| (old - new).abs())
            .sum();

        scores = new_scores;

        if delta < convergence_threshold {
            break;
        }
    }

    scores
}

/// Rough token estimate: ~4 chars per token.
fn estimate_tokens(entry: &MapEntry) -> usize {
    (entry.signature.len() + entry.file.len() + 10) / 4
}

fn truncate_to_budget(entries: &mut Vec<MapEntry>, max_tokens: usize) {
    let mut total = 0;
    let mut keep = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        total += estimate_tokens(entry);
        if total > max_tokens {
            keep = i;
            break;
        }
    }
    entries.truncate(keep);
}

/// Render the map as a human-readable tree string.
pub fn render_tree(entries: &[MapEntry]) -> String {
    let mut out = String::new();
    let mut current_file = "";

    for entry in entries {
        if entry.file != current_file {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&entry.file);
            out.push('\n');
            current_file = &entry.file;
        }
        out.push_str(&format!(
            "  {} {} [line {}]\n",
            entry.kind, entry.signature, entry.line
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagerank_trivial() {
        // Two nodes pointing at each other
        let edges = vec![HashSet::from([1]), HashSet::from([0])];
        let symbols = vec![
            (
                "a.rs".into(),
                "foo".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
            (
                "b.rs".into(),
                "bar".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
        ];
        let opts = MapOptions::default();
        let scores = pagerank(&edges, 2, &opts, &symbols);
        // Both should have equal score
        assert!((scores[0] - scores[1]).abs() < 0.01);
    }

    #[test]
    fn truncate_respects_budget() {
        let mut entries: Vec<MapEntry> = (0..100)
            .map(|i| MapEntry {
                file: format!("f{i}.rs"),
                name: format!("sym{i}"),
                kind: "fn".into(),
                signature: format!("fn sym{i}()"),
                line: i + 1,
                score: 1.0 / (i + 1) as f64,
            })
            .collect();
        truncate_to_budget(&mut entries, 50);
        assert!(entries.len() < 100);
    }

    #[test]
    fn render_tree_groups_by_file() {
        let entries = vec![
            MapEntry {
                file: "a.rs".into(),
                name: "foo".into(),
                kind: "fn".into(),
                signature: "fn foo()".into(),
                line: 1,
                score: 1.0,
            },
            MapEntry {
                file: "a.rs".into(),
                name: "bar".into(),
                kind: "fn".into(),
                signature: "fn bar()".into(),
                line: 5,
                score: 0.5,
            },
            MapEntry {
                file: "b.rs".into(),
                name: "baz".into(),
                kind: "fn".into(),
                signature: "fn baz()".into(),
                line: 1,
                score: 0.3,
            },
        ];
        let tree = render_tree(&entries);
        assert!(tree.contains("a.rs\n"));
        assert!(tree.contains("b.rs\n"));
        assert!(tree.contains("fn foo()"));
    }

    #[test]
    fn pagerank_converges_early_for_small_graph() {
        // A small graph should converge well before max_iterations (100).
        // Two nodes pointing at each other: convergence should be very fast.
        let edges = vec![HashSet::from([1]), HashSet::from([0])];
        let symbols = vec![
            (
                "a.rs".into(),
                "foo".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
            (
                "b.rs".into(),
                "bar".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
        ];
        let opts = MapOptions::default();
        let scores = pagerank(&edges, 2, &opts, &symbols);
        // Both should have approximately equal non-zero scores that sum to ~1.0
        let total: f64 = scores.iter().sum();
        assert!(
            (total - 1.0).abs() < 0.01,
            "PageRank scores should sum to ~1.0, got {total}"
        );
        assert!(
            scores[0] > 0.0 && scores[1] > 0.0,
            "all scores should be positive"
        );
    }

    #[test]
    fn pagerank_disconnected_graph_converges_immediately() {
        // A graph with no edges should converge in 1 iteration:
        // all scores remain at 1/N through personalization.
        let edges = vec![HashSet::new(); 4];
        let symbols: Vec<_> = (0..4)
            .map(|i| {
                (
                    format!("f{i}.rs"),
                    format!("sym{i}"),
                    SymbolKind::Function,
                    String::new(),
                    1usize,
                )
            })
            .collect();
        let opts = MapOptions::default();
        let scores = pagerank(&edges, 4, &opts, &symbols);
        // All scores should be equal (1/4 * (1-d) + d * 0 = 0.0375)
        let expected = (1.0 - 0.85) / 4.0; // personalization component
        for (i, &s) in scores.iter().enumerate() {
            assert!(
                (s - expected).abs() < 1e-6,
                "score[{i}] = {s}, expected ~{expected}"
            );
        }
    }

    #[test]
    fn boost_increases_score() {
        let edges = vec![HashSet::new(), HashSet::new()];
        let symbols = vec![
            (
                "a.rs".into(),
                "target".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
            (
                "b.rs".into(),
                "other".into(),
                SymbolKind::Function,
                String::new(),
                1,
            ),
        ];
        let opts = MapOptions {
            boost: &["target".into()],
            ..MapOptions::default()
        };
        let scores = pagerank(&edges, 2, &opts, &symbols);
        assert!(scores[0] > scores[1]);
    }
}
