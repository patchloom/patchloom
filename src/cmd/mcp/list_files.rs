//! Bounded file inventory for MCP `list_files` (#2076).
//!
//! Reuses search/walk ignore and exclude rules so agents can drop a second
//! filesystem MCP for directory listing.

use crate::cli::global::GlobalFlags;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Default cap when the caller omits `max_results` (agent context budget).
pub(crate) const DEFAULT_LIST_MAX_RESULTS: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ListFilesReport {
    pub ok: bool,
    /// Paths relative to MCP cwd when under the workspace; otherwise display form.
    pub paths: Vec<String>,
    /// Number of paths returned (after cap).
    pub count: usize,
    /// True when more files matched than `max_results`.
    pub truncated: bool,
    /// Total matches before the cap (when truncated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_matched: Option<usize>,
    /// Effective walk roots (as requested).
    pub roots: Vec<String>,
}

/// Collect a bounded inventory of files under `roots` with the same ignore /
/// exclude / glob rules as search walks.
pub(crate) fn collect_list_files(
    roots: &[String],
    global: &GlobalFlags,
    cwd: &Path,
    max_results: usize,
    max_depth: Option<usize>,
    include_hidden: bool,
) -> anyhow::Result<ListFilesReport> {
    let cap = if max_results == 0 {
        DEFAULT_LIST_MAX_RESULTS
    } else {
        max_results
    };
    let mut paths =
        crate::files::collect_file_paths_opts(roots, global, include_hidden, Some(cwd))?;

    // Include globs are applied by callers of collect_file_paths_opts (search,
    // replace, tidy), not inside the collector — mirror that here (#2076).
    let glob_matcher = crate::files::build_glob_matcher_from_global(global)?;
    if glob_matcher.is_some() {
        let glob_roots = crate::files::collect_glob_roots_from_global(roots, global, Some(cwd))?;
        paths.retain(|p| {
            crate::files::matches_glob_with_roots(p, glob_matcher.as_ref(), &glob_roots)
        });
    }

    // max_depth is relative to each walk root (not always cwd).
    if let Some(depth) = max_depth {
        let walk_roots: Vec<PathBuf> = roots
            .iter()
            .map(|r| {
                if r == "." {
                    cwd.to_path_buf()
                } else {
                    cwd.join(r)
                }
            })
            .collect();
        paths.retain(|p| path_depth_under_any_root(&walk_roots, p) <= depth);
    }

    // Stable order for agents.
    paths.sort();

    let total = paths.len();
    let truncated = total > cap;
    if truncated {
        paths.truncate(cap);
    }

    let rel: Vec<String> = paths.iter().map(|p| display_rel(cwd, p)).collect();

    Ok(ListFilesReport {
        ok: true,
        count: rel.len(),
        paths: rel,
        truncated,
        total_matched: if truncated { Some(total) } else { None },
        roots: roots.to_vec(),
    })
}

/// Depth of `path` under the first walk root that contains it (file component
/// count relative to that root). Falls back to the full path component count
/// when no root prefix matches.
fn path_depth_under_any_root(walk_roots: &[PathBuf], path: &Path) -> usize {
    for root in walk_roots {
        if let Ok(rel) = path.strip_prefix(root) {
            return rel.components().count();
        }
    }
    path.components().count()
}

fn display_rel(cwd: &Path, path: &Path) -> String {
    match path.strip_prefix(cwd) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().replace('\\', "/"),
        Ok(_) => ".".into(),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_tree(dir: &Path) {
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::write(dir.join("src/a.rs"), "a\n").unwrap();
        fs::write(dir.join("src/nested/b.rs"), "b\n").unwrap();
        fs::write(dir.join("README.md"), "r\n").unwrap();
        fs::create_dir_all(dir.join("vendor")).unwrap();
        fs::write(dir.join("vendor/x.js"), "x\n").unwrap();
        fs::write(dir.join("src/lib.ts"), "t\n").unwrap();
    }

    #[test]
    fn lists_files_sorted_relative() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 500, None, false).unwrap();
        assert!(report.ok);
        assert!(
            report
                .paths
                .iter()
                .any(|p| p == "README.md" || p.ends_with("README.md"))
        );
        assert!(report.paths.iter().any(|p| p.contains("a.rs")));
        assert!(!report.truncated);
        // Sorted
        let mut sorted = report.paths.clone();
        sorted.sort();
        assert_eq!(report.paths, sorted);
    }

    #[test]
    fn exclude_and_max_results_truncate() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let mut global = GlobalFlags::test_default();
        global.exclude = vec!["vendor/**".into()];
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 2, None, false).unwrap();
        assert!(!report.paths.iter().any(|p| p.contains("vendor")));
        assert_eq!(report.count, 2);
        assert!(report.truncated);
        assert!(report.total_matched.unwrap() >= 3);
    }

    #[test]
    fn max_depth_limits_nested() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        // depth 1 under cwd: only top-level files
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 500, Some(1), false).unwrap();
        assert!(
            report.paths.iter().all(|p| !p.contains('/')),
            "depth 1 must not include nested paths: {:?}",
            report.paths
        );
        assert!(report.paths.iter().any(|p| p == "README.md"));
    }

    #[test]
    fn max_depth_relative_to_non_dot_root() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        // root=src, depth 1: src/a.rs and src/lib.ts but not src/nested/b.rs
        let report =
            collect_list_files(&["src".into()], &global, dir.path(), 500, Some(1), false).unwrap();
        assert!(
            report.paths.iter().any(|p| p.ends_with("a.rs")),
            "expected a.rs under src depth 1: {:?}",
            report.paths
        );
        assert!(
            report.paths.iter().any(|p| p.ends_with("lib.ts")),
            "expected lib.ts under src depth 1: {:?}",
            report.paths
        );
        assert!(
            !report.paths.iter().any(|p| p.contains("nested")),
            "nested must be depth 2 under src: {:?}",
            report.paths
        );
    }

    #[test]
    fn globs_filter_include() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let mut global = GlobalFlags::test_default();
        global.glob = vec!["**/*.rs".into()];
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 500, None, false).unwrap();
        assert!(
            !report.paths.is_empty(),
            "glob **/*.rs must match: {:?}",
            report.paths
        );
        assert!(
            report.paths.iter().all(|p| p.ends_with(".rs")),
            "only .rs: {:?}",
            report.paths
        );
        assert!(!report.paths.iter().any(|p| p.ends_with(".md")));
        assert!(!report.paths.iter().any(|p| p.ends_with(".js")));
        assert!(!report.paths.iter().any(|p| p.ends_with(".ts")));
    }

    #[test]
    fn zero_max_results_uses_default_cap() {
        assert_eq!(DEFAULT_LIST_MAX_RESULTS, 500);
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 0, None, false).unwrap();
        assert!(report.count <= DEFAULT_LIST_MAX_RESULTS);
        assert!(!report.truncated);
        assert!(report.total_matched.is_none());
    }

    #[test]
    fn total_matched_when_truncated() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 1, None, false).unwrap();
        assert!(report.truncated);
        assert_eq!(report.count, 1);
        let total = report.total_matched.expect("total_matched when truncated");
        assert!(total > 1, "total_matched={total}");
    }

    #[test]
    fn include_hidden_lists_dotfiles() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        fs::write(dir.path().join(".secret"), "s\n").unwrap();
        let global = GlobalFlags::test_default();
        let hidden_off =
            collect_list_files(&[".".into()], &global, dir.path(), 500, None, false).unwrap();
        assert!(
            !hidden_off.paths.iter().any(|p| p.contains(".secret")),
            "hidden off must skip .secret: {:?}",
            hidden_off.paths
        );
        let hidden_on =
            collect_list_files(&[".".into()], &global, dir.path(), 500, None, true).unwrap();
        assert!(
            hidden_on
                .paths
                .iter()
                .any(|p| p.ends_with(".secret") || p == ".secret"),
            "include_hidden must list .secret: {:?}",
            hidden_on.paths
        );
    }

    #[test]
    fn multi_root_union_sorted() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let global = GlobalFlags::test_default();
        let report = collect_list_files(
            &["src".into(), "vendor".into()],
            &global,
            dir.path(),
            500,
            None,
            false,
        )
        .unwrap();
        assert!(
            report.paths.iter().any(|p| p.contains("a.rs")),
            "src root: {:?}",
            report.paths
        );
        assert!(
            report.paths.iter().any(|p| p.contains("x.js")),
            "vendor root: {:?}",
            report.paths
        );
        assert!(
            !report
                .paths
                .iter()
                .any(|p| p == "README.md" || p.ends_with("README.md")),
            "README is outside roots: {:?}",
            report.paths
        );
        let mut sorted = report.paths.clone();
        sorted.sort();
        assert_eq!(report.paths, sorted);
        assert_eq!(report.roots, vec!["src".to_string(), "vendor".to_string()]);
    }
}
