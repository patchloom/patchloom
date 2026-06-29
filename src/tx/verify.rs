//! Pre/post-operation symbol count verification for structural safety.
//!
//! When `--verify` is used, the tx engine captures a symbol snapshot before
//! executing operations and compares it against a post-execution snapshot.
//! Mismatches trigger rollback with a descriptive error.

#[cfg(feature = "ast")]
use crate::ast::{Language, symbols};
use crate::plan::VerifyCheck;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Snapshot of symbol counts per file, keyed by file path.
#[cfg(feature = "ast")]
#[derive(Debug, Clone)]
pub(crate) struct SymbolSnapshot {
    /// Per-file symbol lists (only files matching the criteria).
    pub(crate) files: HashMap<PathBuf, Vec<SnappedSymbol>>,
    /// Total count of matching symbols.
    pub(crate) total: usize,
}

/// A symbol captured in a snapshot.
#[cfg(feature = "ast")]
#[derive(Debug, Clone)]
pub(crate) struct SnappedSymbol {
    pub(crate) name: String,
    /// Used in `compare_snapshots` diagnostic messages to show which
    /// specific symbols were added or removed (e.g. "fn 'foo' removed").
    pub(crate) kind: symbols::SymbolKind,
}

/// Returns true when a path string contains glob metacharacters (`*`, `?`, `[`).
fn is_glob_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Walk `dir` recursively, adding files whose relative path (from `root`)
/// matches `matcher` to `out`.
#[cfg(feature = "files")]
fn walk_and_match(
    root: &Path,
    dir: &Path,
    matcher: &globset::GlobMatcher,
    out: &mut HashSet<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_and_match(root, &path, matcher, out);
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(root)
            && matcher.is_match(rel)
        {
            out.insert(path);
        }
    }
}

/// Collect all file paths declared by operations in a plan.
///
/// Handles three cases for each declared path:
/// 1. Literal file path: include directly.
/// 2. Directory path: scan for source files (when `ast` + `cli` features).
/// 3. Glob pattern (contains `*`, `?`, `[`): expand against `cwd`.
pub(crate) fn affected_file_paths(plan: &crate::plan::Plan, cwd: &Path) -> Vec<PathBuf> {
    let mut paths = HashSet::new();
    for op in &plan.operations {
        for p in op.declared_paths() {
            let full = cwd.join(p);
            if full.is_file() {
                paths.insert(full);
            } else if full.is_dir() {
                // For directory targets (e.g. glob replace), scan for source files
                #[cfg(all(feature = "ast", feature = "cli"))]
                if let Ok(files) = crate::cmd::ast::collect_source_files(
                    &full,
                    &crate::cli::global::GlobalFlags::default(),
                ) {
                    for f in files {
                        paths.insert(f);
                    }
                }
            } else if is_glob_pattern(p) {
                // Expand glob patterns against cwd so verification
                // covers files targeted by glob-based operations.
                #[cfg(feature = "files")]
                if let Ok(glob) = globset::Glob::new(p) {
                    let matcher = glob.compile_matcher();
                    walk_and_match(cwd, cwd, &matcher, &mut paths);
                }
            }
        }
    }
    paths.into_iter().collect()
}

/// Take a symbol snapshot of the given files for a specific verify check.
#[cfg(feature = "ast")]
pub(crate) fn snapshot_symbols(files: &[PathBuf], check: &VerifyCheck) -> SymbolSnapshot {
    use symbols::{filter_symbols, parse_kind_filter};

    let mut result = SymbolSnapshot {
        files: HashMap::new(),
        total: 0,
    };

    let (kind_filter, attr_filter) = match check {
        VerifyCheck::SymbolCount { kind, attr } => {
            (parse_kind_filter(&Some(kind.clone())), attr.clone())
        }
        VerifyCheck::Named { check } if check == "unique_names" || check == "no_orphans" => {
            // For named checks, capture all symbols
            (Vec::new(), None)
        }
        _ => return result,
    };

    for path in files {
        let lang = Language::from_path(path);
        if !lang.has_grammar() {
            continue;
        }
        let all_symbols = symbols::extract_symbols_from_file(path, Some(lang));
        let filtered = filter_symbols(&all_symbols, &kind_filter);

        // If attr filter is set, further filter by checking the source for the attribute
        let matched: Vec<SnappedSymbol> = if let Some(ref attr) = attr_filter {
            let source = std::fs::read_to_string(path).unwrap_or_default();
            filtered
                .into_iter()
                .filter(|sym| symbol_has_attr(&source, sym, attr))
                .map(|sym| SnappedSymbol {
                    name: sym.name.clone(),
                    kind: sym.kind,
                })
                .collect()
        } else {
            filtered
                .into_iter()
                .map(|sym| SnappedSymbol {
                    name: sym.name.clone(),
                    kind: sym.kind,
                })
                .collect()
        };

        result.total += matched.len();
        if !matched.is_empty() {
            result.files.insert(path.clone(), matched);
        }
    }

    result
}

/// Take a snapshot from in-memory pending content (post-execution, before commit).
#[cfg(feature = "ast")]
pub(crate) fn snapshot_symbols_from_pending(
    files: &[PathBuf],
    pending: &HashMap<PathBuf, (String, String)>,
    check: &VerifyCheck,
) -> SymbolSnapshot {
    use symbols::{filter_symbols, parse_kind_filter};

    let mut result = SymbolSnapshot {
        files: HashMap::new(),
        total: 0,
    };

    let (kind_filter, attr_filter) = match check {
        VerifyCheck::SymbolCount { kind, attr } => {
            (parse_kind_filter(&Some(kind.clone())), attr.clone())
        }
        VerifyCheck::Named { check } if check == "unique_names" || check == "no_orphans" => {
            (Vec::new(), None)
        }
        _ => return result,
    };

    for path in files {
        let lang = Language::from_path(path);
        if !lang.has_grammar() {
            continue;
        }

        // Use pending content (post-edit) if available, otherwise read from disk
        let source = if let Some((_, current)) = pending.get(path) {
            current.clone()
        } else if let Ok(s) = std::fs::read_to_string(path) {
            s
        } else {
            continue;
        };

        let all_symbols = symbols::extract_symbols(&source, lang);
        let filtered = filter_symbols(&all_symbols, &kind_filter);

        let matched: Vec<SnappedSymbol> = if let Some(ref attr) = attr_filter {
            filtered
                .into_iter()
                .filter(|sym| symbol_has_attr(&source, sym, attr))
                .map(|sym| SnappedSymbol {
                    name: sym.name.clone(),
                    kind: sym.kind,
                })
                .collect()
        } else {
            filtered
                .into_iter()
                .map(|sym| SnappedSymbol {
                    name: sym.name.clone(),
                    kind: sym.kind,
                })
                .collect()
        };

        result.total += matched.len();
        if !matched.is_empty() {
            result.files.insert(path.clone(), matched);
        }
    }

    result
}

/// Check if a symbol has a specific attribute (e.g., `#[test]` for Rust).
#[cfg(feature = "ast")]
fn symbol_has_attr(source: &str, sym: &symbols::SymbolDef, attr: &str) -> bool {
    // Look at lines before the symbol's start line for the attribute
    let lines: Vec<&str> = source.lines().collect();
    if sym.start_line == 0 || sym.start_line > lines.len() {
        return false;
    }
    // Search up to 10 lines before the symbol for the attribute
    let start = sym.start_line.saturating_sub(11); // 0-indexed, start_line is 1-based
    let end = sym.start_line - 1; // line just before the symbol definition
    for i in start..end {
        if i < lines.len() {
            let line = lines[i].trim();
            // Match #[test], #[cfg(test)], #[tokio::test], @test, @Test, etc.
            if line.contains(&format!("#[{attr}]"))
                || line.contains(&format!("#[{attr}("))
                || line.contains(&format!("#[tokio::{attr}]"))
                || line.contains(&format!("@{attr}"))
                || line.contains(&format!("@{}", capitalize(attr)))
            {
                return true;
            }
        }
    }
    false
}

#[cfg(feature = "ast")]
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Result of a verification comparison.
#[derive(Debug)]
pub(crate) struct VerifyResult {
    pub(crate) passed: bool,
    pub(crate) message: String,
}

/// Compare pre and post snapshots for a SymbolCount check.
#[cfg(feature = "ast")]
pub(crate) fn compare_snapshots(
    before: &SymbolSnapshot,
    after: &SymbolSnapshot,
    check: &VerifyCheck,
    cwd: &Path,
) -> VerifyResult {
    match check {
        VerifyCheck::SymbolCount { kind, attr } => {
            let label = if let Some(a) = attr {
                format!("{kind} (attr={a})")
            } else {
                kind.clone()
            };

            if before.total == after.total {
                VerifyResult {
                    passed: true,
                    message: format!(
                        "verify {label}: before={}, after={} \u{2713}",
                        before.total, after.total
                    ),
                }
            } else {
                // Find which files lost or gained symbols
                let mut details = Vec::new();
                let all_files: HashSet<&PathBuf> =
                    before.files.keys().chain(after.files.keys()).collect();

                for path in all_files {
                    let before_syms = before.files.get(path);
                    let after_syms = after.files.get(path);
                    let b = before_syms.map_or(0, |v| v.len());
                    let a = after_syms.map_or(0, |v| v.len());
                    if b != a {
                        let display = path.strip_prefix(cwd).unwrap_or(path).display();
                        let diff = a as isize - b as isize;
                        let mut line = format!("  {display}: {b} -> {a} ({diff:+})");

                        // Show which specific symbols were added or removed
                        let before_names: HashSet<&str> = before_syms
                            .map(|v| v.iter().map(|s| s.name.as_str()).collect())
                            .unwrap_or_default();
                        let after_names: HashSet<&str> = after_syms
                            .map(|v| v.iter().map(|s| s.name.as_str()).collect())
                            .unwrap_or_default();

                        let removed: Vec<_> = before_syms
                            .into_iter()
                            .flat_map(|v| v.iter())
                            .filter(|s| !after_names.contains(s.name.as_str()))
                            .map(|s| format!("{} '{}'", s.kind, s.name))
                            .collect();
                        let added: Vec<_> = after_syms
                            .into_iter()
                            .flat_map(|v| v.iter())
                            .filter(|s| !before_names.contains(s.name.as_str()))
                            .map(|s| format!("{} '{}'", s.kind, s.name))
                            .collect();

                        if !removed.is_empty() {
                            line.push_str(&format!(" removed: {}", removed.join(", ")));
                        }
                        if !added.is_empty() {
                            line.push_str(&format!(" added: {}", added.join(", ")));
                        }

                        details.push(line);
                    }
                }

                let detail_str = if details.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", details.join("\n"))
                };

                let diff = after.total as isize - before.total as isize;
                VerifyResult {
                    passed: false,
                    message: format!(
                        "verify {label}: before={}, after={} ({diff:+}) \u{2717}{detail_str}",
                        before.total, after.total
                    ),
                }
            }
        }
        VerifyCheck::Named { check } if check == "unique_names" => check_unique_names(after, cwd),
        VerifyCheck::Named { check } if check == "no_orphans" => {
            check_no_orphans(before, after, cwd)
        }
        _ => VerifyResult {
            passed: true,
            message: "unknown check (skipped)".to_string(),
        },
    }
}

/// Check that no file has duplicate symbol names.
#[cfg(feature = "ast")]
fn check_unique_names(snapshot: &SymbolSnapshot, cwd: &Path) -> VerifyResult {
    let mut duplicates = Vec::new();
    for (path, syms) in &snapshot.files {
        let mut seen = HashMap::new();
        for sym in syms {
            *seen.entry(sym.name.as_str()).or_insert(0usize) += 1;
        }
        for (name, count) in &seen {
            if *count > 1 {
                let display = path.strip_prefix(cwd).unwrap_or(path).display();
                duplicates.push(format!("  {display}: '{name}' appears {count} times"));
            }
        }
    }

    if duplicates.is_empty() {
        VerifyResult {
            passed: true,
            message: "verify unique_names: no duplicates found \u{2713}".to_string(),
        }
    } else {
        VerifyResult {
            passed: false,
            message: format!(
                "verify unique_names: duplicate symbols found \u{2717}\n{}",
                duplicates.join("\n")
            ),
        }
    }
}

/// Check that every symbol from `before` appears in `after` (no orphans).
#[cfg(feature = "ast")]
fn check_no_orphans(before: &SymbolSnapshot, after: &SymbolSnapshot, cwd: &Path) -> VerifyResult {
    let after_names: HashSet<&str> = after
        .files
        .values()
        .flat_map(|syms| syms.iter().map(|s| s.name.as_str()))
        .collect();

    let mut orphans = Vec::new();
    for (path, syms) in &before.files {
        for sym in syms {
            if !after_names.contains(sym.name.as_str()) {
                let display = path.strip_prefix(cwd).unwrap_or(path).display();
                orphans.push(format!(
                    "  {display}: '{}' not found in any target",
                    sym.name
                ));
            }
        }
    }

    if orphans.is_empty() {
        VerifyResult {
            passed: true,
            message: "verify no_orphans: all symbols preserved \u{2713}".to_string(),
        }
    } else {
        VerifyResult {
            passed: false,
            message: format!(
                "verify no_orphans: {} symbol(s) lost \u{2717}\n{}",
                orphans.len(),
                orphans.join("\n")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: affected_file_paths must expand glob patterns instead
    /// of silently skipping them (which would cause --verify to miss files).
    #[test]
    fn affected_file_paths_expands_globs() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn lib() {}\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();

        let plan = crate::plan::Plan {
            version: "1".into(),
            cwd: None,
            strict: None,
            operations: vec![crate::plan::Operation::Replace {
                glob: Some("src/*.rs".into()),
                path: None,
                mode: None,
                from: "old".into(),
                to: Some("new".into()),
                nth: None,
                insert_before: None,
                insert_after: None,
                case_insensitive: false,
                multiline: false,
                if_exists: false,
                whole_line: false,
                range: None,
                word_boundary: false,
                before_context: None,
                after_context: None,
            }],
            write_policy: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };

        let affected = affected_file_paths(&plan, dir.path());
        let names: Vec<String> = affected
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(
            names.contains(&"main.rs".to_string()),
            "glob should match main.rs, got: {names:?}"
        );
        assert!(
            names.contains(&"lib.rs".to_string()),
            "glob should match lib.rs, got: {names:?}"
        );
        assert!(
            !names.contains(&"README.md".to_string()),
            "glob should not match README.md"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn parse_verify_kind_only() {
        let check = VerifyCheck::parse("kind=function").unwrap();
        match check {
            VerifyCheck::SymbolCount { kind, attr } => {
                assert_eq!(kind, "function");
                assert!(attr.is_none());
            }
            _ => panic!("expected SymbolCount"),
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn parse_verify_kind_and_attr() {
        let check = VerifyCheck::parse("kind=function,attr=test").unwrap();
        match check {
            VerifyCheck::SymbolCount { kind, attr } => {
                assert_eq!(kind, "function");
                assert_eq!(attr.as_deref(), Some("test"));
            }
            _ => panic!("expected SymbolCount"),
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn parse_verify_named_check() {
        let check = VerifyCheck::parse("unique_names").unwrap();
        match check {
            VerifyCheck::Named { check } => assert_eq!(check, "unique_names"),
            _ => panic!("expected Named"),
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn parse_verify_bare_kind() {
        let check = VerifyCheck::parse("function").unwrap();
        match check {
            VerifyCheck::SymbolCount { kind, attr } => {
                assert_eq!(kind, "function");
                assert!(attr.is_none());
            }
            _ => panic!("expected SymbolCount"),
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn parse_verify_unknown_key_errors() {
        assert!(VerifyCheck::parse("foo=bar").is_err());
    }

    #[test]
    #[cfg(feature = "ast")]
    fn symbol_has_test_attr() {
        let source = r#"
#[test]
fn my_test() {
    assert!(true);
}
"#;
        let syms = symbols::extract_symbols(source, Language::Rust);
        assert!(!syms.is_empty());
        assert!(symbol_has_attr(source, &syms[0], "test"));
    }

    #[test]
    #[cfg(feature = "ast")]
    fn symbol_without_attr() {
        let source = "fn normal() { }\n";
        let syms = symbols::extract_symbols(source, Language::Rust);
        assert!(!syms.is_empty());
        assert!(!symbol_has_attr(source, &syms[0], "test"));
    }

    #[test]
    #[cfg(feature = "ast")]
    fn compare_snapshots_equal() {
        let check = VerifyCheck::SymbolCount {
            kind: "function".to_string(),
            attr: None,
        };
        let before = SymbolSnapshot {
            files: HashMap::new(),
            total: 5,
        };
        let after = SymbolSnapshot {
            files: HashMap::new(),
            total: 5,
        };
        let result = compare_snapshots(&before, &after, &check, Path::new("/tmp"));
        assert!(result.passed);
    }

    #[test]
    #[cfg(feature = "ast")]
    fn compare_snapshots_mismatch() {
        let check = VerifyCheck::SymbolCount {
            kind: "function".to_string(),
            attr: Some("test".to_string()),
        };
        let before = SymbolSnapshot {
            files: HashMap::new(),
            total: 10,
        };
        let after = SymbolSnapshot {
            files: HashMap::new(),
            total: 8,
        };
        let result = compare_snapshots(&before, &after, &check, Path::new("/tmp"));
        assert!(!result.passed);
        assert!(
            result.message.contains("-2"),
            "should show count diff: {}",
            result.message
        );
        assert!(
            result.message.contains("function (attr=test)"),
            "should include kind label with attr: {}",
            result.message
        );
    }

    #[test]
    #[cfg(feature = "ast")]
    fn compare_snapshots_mismatch_shows_symbol_names_and_kinds() {
        let check = VerifyCheck::SymbolCount {
            kind: "function".to_string(),
            attr: None,
        };
        let before = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/lib.rs"),
                vec![
                    SnappedSymbol {
                        name: "alpha".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                    SnappedSymbol {
                        name: "beta".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                ],
            )]),
            total: 2,
        };
        let after = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/lib.rs"),
                vec![SnappedSymbol {
                    name: "alpha".into(),
                    kind: symbols::SymbolKind::Function,
                }],
            )]),
            total: 1,
        };
        let result = compare_snapshots(&before, &after, &check, Path::new("/tmp"));
        assert!(!result.passed);
        assert!(
            result.message.contains("fn 'beta'"),
            "should name the removed symbol with its kind: {}",
            result.message
        );
        assert!(
            result.message.contains("removed:"),
            "should label the removed symbols: {}",
            result.message
        );
    }

    #[test]
    #[cfg(feature = "ast")]
    fn unique_names_no_duplicates() {
        let snapshot = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/test.rs"),
                vec![
                    SnappedSymbol {
                        name: "a".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                    SnappedSymbol {
                        name: "b".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                ],
            )]),
            total: 2,
        };
        let result = check_unique_names(&snapshot, Path::new("/tmp"));
        assert!(result.passed);
    }

    #[test]
    #[cfg(feature = "ast")]
    fn unique_names_with_duplicates() {
        let snapshot = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/test.rs"),
                vec![
                    SnappedSymbol {
                        name: "foo".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                    SnappedSymbol {
                        name: "foo".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                ],
            )]),
            total: 2,
        };
        let result = check_unique_names(&snapshot, Path::new("/tmp"));
        assert!(!result.passed);
        assert!(result.message.contains("'foo' appears 2 times"));
    }

    #[test]
    #[cfg(feature = "ast")]
    fn no_orphans_all_present() {
        let syms = vec![
            SnappedSymbol {
                name: "a".into(),
                kind: symbols::SymbolKind::Function,
            },
            SnappedSymbol {
                name: "b".into(),
                kind: symbols::SymbolKind::Function,
            },
        ];
        let before = SymbolSnapshot {
            files: HashMap::from([(PathBuf::from("/tmp/old.rs"), syms.clone())]),
            total: 2,
        };
        let after = SymbolSnapshot {
            files: HashMap::from([(PathBuf::from("/tmp/new.rs"), syms)]),
            total: 2,
        };
        let result = check_no_orphans(&before, &after, Path::new("/tmp"));
        assert!(result.passed);
    }

    #[test]
    #[cfg(feature = "ast")]
    fn no_orphans_missing_symbol() {
        let before = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/old.rs"),
                vec![
                    SnappedSymbol {
                        name: "a".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                    SnappedSymbol {
                        name: "b".into(),
                        kind: symbols::SymbolKind::Function,
                    },
                ],
            )]),
            total: 2,
        };
        let after = SymbolSnapshot {
            files: HashMap::from([(
                PathBuf::from("/tmp/new.rs"),
                vec![SnappedSymbol {
                    name: "a".into(),
                    kind: symbols::SymbolKind::Function,
                }],
            )]),
            total: 1,
        };
        let result = check_no_orphans(&before, &after, Path::new("/tmp"));
        assert!(!result.passed);
        assert!(result.message.contains("'b' not found"));
    }
}
