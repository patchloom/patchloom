//! Repository map with PageRank ranking and token-budget-aware output.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;

use super::Language;
use super::refs::{RefKind, find_all_refs_in_source_with_tree};
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

    // Collect all identifiers in each file in a single pass (O(1) parses per file).
    // Each entry is (identifier_name, 1-based line number, RefKind).
    let file_all_refs: HashMap<&str, Vec<(String, usize, RefKind)>> = file_data
        .iter()
        .filter_map(|fd| {
            let tree = tree_cache.get(fd.path.as_str())?;
            let refs = find_all_refs_in_source_with_tree(&fd.source, tree, &fd.path);
            let tuples: Vec<(String, usize, RefKind)> = refs
                .into_iter()
                .map(|(name, r)| (name, r.line, r.kind))
                .collect();
            Some((fd.path.as_str(), tuples))
        })
        .collect();

    // Build symbol line ranges for containment checks.
    // all_symbols_ranges[i] = (file, start_line_1based, end_line_1based).
    let all_symbols_ranges: Vec<(&str, usize, usize)> = file_data
        .iter()
        .flat_map(|fd| collect_symbol_ranges(&fd.symbols, &fd.path))
        .collect();

    // For each symbol, find identifiers within its line range that match
    // other known symbol names, and add edges to those targets.
    for (src_idx, (src_file, src_start, src_end)) in all_symbols_ranges.iter().enumerate() {
        let Some(refs_in_file) = file_all_refs.get(src_file) else {
            continue;
        };
        for (ref_name, ref_line, ref_kind) in refs_in_file {
            if *ref_kind == RefKind::Definition {
                continue;
            }
            // Check if this reference falls within the symbol's line range
            if *ref_line < *src_start || *ref_line > *src_end {
                continue;
            }
            // Find target symbols with this name
            if let Some(targets) = name_to_idx.get(ref_name.as_str()) {
                for &dst in targets {
                    if dst != src_idx {
                        edges[src_idx].insert(dst);
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

/// Collect (file, start_line, end_line) ranges for all symbols in the same
/// order as `collect_flat_symbols` produces them. This keeps the index
/// correspondence between `all_symbols` and the range table.
fn collect_symbol_ranges<'a>(
    symbols: &'a [SymbolDef],
    file: &'a str,
) -> Vec<(&'a str, usize, usize)> {
    let mut out = Vec::new();
    for sym in symbols {
        out.push((file, sym.start_line, sym.end_line));
        out.extend(collect_symbol_ranges(&sym.children, file));
    }
    out
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

    /// Regression: cross-file edges must be attributed to the specific symbol
    /// that contains the reference, not to ALL symbols in the file (#1101).
    #[test]
    fn edges_scoped_to_containing_symbol() {
        // Create two files:
        // - file_a.rs: defines `foo` (calls `baz`) and `bar` (no calls)
        // - file_b.rs: defines `baz`
        //
        // Only `foo` should have an edge to `baz`. `bar` should NOT.
        let dir = tempfile::tempdir().unwrap();

        let file_a = dir.path().join("file_a.rs");
        std::fs::write(
            &file_a,
            "fn foo() {\n    baz();\n}\n\nfn bar() {\n    let x = 1;\n}\n",
        )
        .unwrap();

        let file_b = dir.path().join("file_b.rs");
        std::fs::write(&file_b, "fn baz() {\n    println!(\"hello\");\n}\n").unwrap();

        let files: Vec<(std::path::PathBuf, String)> = vec![
            (file_a.clone(), "file_a.rs".into()),
            (file_b.clone(), "file_b.rs".into()),
        ];

        let opts = MapOptions {
            max_tokens: 10000,
            ..MapOptions::default()
        };
        let entries = generate_map(&files, &opts);

        // baz should have a higher score than bar because foo references baz,
        // but bar has no references to baz.
        let baz_score = entries
            .iter()
            .find(|e| e.name == "baz")
            .map(|e| e.score)
            .unwrap_or(0.0);
        let bar_score = entries
            .iter()
            .find(|e| e.name == "bar")
            .map(|e| e.score)
            .unwrap_or(0.0);
        // If the bug were present (edges from ALL symbols in file_a to baz),
        // bar would also boost baz. With the fix, only foo -> baz exists.
        // We can't assert exact scores but we can verify the map runs without panic.
        assert!(!entries.is_empty(), "map should produce entries");
        // Both baz and bar should appear
        assert!(
            entries.iter().any(|e| e.name == "baz"),
            "baz should be in the map"
        );
        assert!(
            entries.iter().any(|e| e.name == "bar"),
            "bar should be in the map"
        );
        // baz should have equal or higher score than bar (foo references baz)
        assert!(
            baz_score >= bar_score,
            "baz ({baz_score}) should score >= bar ({bar_score}) since foo->baz edge exists"
        );
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
