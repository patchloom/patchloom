//! Bounded file inventory for CLI `list-files` and MCP `list_files` (#2076 / #2541).
//!
//! Reuses search/walk ignore and exclude rules so agents can drop a second
//! filesystem MCP for directory listing. The collector lives here so CLI
//! compiles without the `mcp` feature.

use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;
use std::path::Path;

/// Default cap when the caller omits `max_results` (agent context budget).
pub(crate) const DEFAULT_LIST_MAX_RESULTS: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ListFilesReport {
    pub ok: bool,
    /// Paths relative to cwd when under the workspace; otherwise display form.
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

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom list-files
  patchloom list-files src/ --exclude 'target/**' --max-results 100
  patchloom list-files --glob '**/*.rs' --max-depth 2 --json")]
pub struct ListFilesArgs {
    /// Walk roots (defaults to the working directory).
    pub paths: Vec<String>,
    /// Max paths to return. Default 500 when omitted or 0 (agent context budget).
    #[arg(long, default_value_t = 0)]
    pub max_results: usize,
    /// Max path depth under each root (1 = only files directly under the root).
    /// Applied during the walk: deeper directories are not entered.
    #[arg(long)]
    pub max_depth: Option<usize>,
    /// Include hidden files (still never walks `.git` / `.patchloom`).
    #[arg(long)]
    pub include_hidden: bool,
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
    // Walk-time max_depth pruning via WalkBuilder (#2078). Do not full-walk
    // then filter: deep monorepos stay cheap when agents pass max_depth.
    // max_results still collects all matches in-depth then caps (total_matched
    // honesty when truncated).
    let mut paths = crate::files::collect_file_paths_opts_depth(
        roots,
        global,
        include_hidden,
        Some(cwd),
        max_depth,
    )?;

    // Include globs are applied by callers of collect_file_paths_opts (search,
    // replace, tidy), not inside the collector — mirror that here (#2076).
    let glob_matcher = crate::files::build_glob_matcher_from_global(global)?;
    if glob_matcher.is_some() {
        let glob_roots = crate::files::collect_glob_roots_from_global(roots, global, Some(cwd))?;
        paths.retain(|p| {
            crate::files::matches_glob_with_roots(p, glob_matcher.as_ref(), &glob_roots)
        });
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

fn display_rel(cwd: &Path, path: &Path) -> String {
    match path.strip_prefix(cwd) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().replace('\\', "/"),
        Ok(_) => ".".into(),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

pub fn run(args: ListFilesArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    if args.paths.iter().any(|p| p.trim().is_empty()) {
        global.emit_error_json_kind(
            Some("invalid_input"),
            "path must not be empty or whitespace-only",
        )?;
        return Ok(exit::FAILURE);
    }
    if args.max_depth == Some(0) {
        global.emit_error_json_kind(
            Some("invalid_input"),
            "max_depth must be >= 1 when set (1 = files directly under each root)",
        )?;
        return Ok(exit::FAILURE);
    }
    let roots = if args.paths.is_empty() {
        vec![".".into()]
    } else {
        args.paths.clone()
    };
    global.check_paths_contained(&cwd, &roots)?;
    if crate::files::all_explicit_paths_missing(&roots, Some(&cwd)) {
        let msg = format!("no such file or directory: {}", roots.join(", "));
        global.emit_error_json_kind(Some("not_found"), &msg)?;
        return Ok(exit::FAILURE);
    }
    let report = collect_list_files(
        &roots,
        global,
        &cwd,
        args.max_results,
        args.max_depth,
        args.include_hidden,
    )?;
    if !global.emit_json(&report)? && !global.quiet {
        for path in &report.paths {
            if !crate::json_emit::write_stdout_ignore_epipe(path.as_bytes(), true)? {
                break;
            }
        }
    }
    Ok(exit::SUCCESS)
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

    /// #2078: deep trees with max_depth must not surface deep files (walk prune).
    #[test]
    fn max_depth_prunes_very_deep_tree() {
        let dir = TempDir::new().unwrap();
        let mut deep = dir.path().to_path_buf();
        for i in 0..12 {
            deep.push(format!("d{i}"));
            fs::create_dir_all(&deep).unwrap();
        }
        fs::write(deep.join("leaf.txt"), "leaf\n").unwrap();
        fs::write(dir.path().join("top.txt"), "top\n").unwrap();
        let global = GlobalFlags::test_default();
        let report =
            collect_list_files(&[".".into()], &global, dir.path(), 500, Some(1), false).unwrap();
        assert!(
            report
                .paths
                .iter()
                .any(|p| p == "top.txt" || p.ends_with("top.txt")),
            "top-level file: {:?}",
            report.paths
        );
        assert!(
            !report.paths.iter().any(|p| p.contains("leaf")),
            "depth-12 leaf must be pruned: {:?}",
            report.paths
        );
        // Unlimited depth still finds the leaf.
        let deep_report =
            collect_list_files(&[".".into()], &global, dir.path(), 500, None, false).unwrap();
        assert!(
            deep_report.paths.iter().any(|p| p.contains("leaf")),
            "unlimited must find leaf: {:?}",
            deep_report.paths
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
    fn invalid_glob_pattern_errors() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let mut global = GlobalFlags::test_default();
        // Unclosed bracket is invalid for globset.
        global.glob = vec!["**/*[".into()];
        let err = collect_list_files(&[".".into()], &global, dir.path(), 500, None, false)
            .expect_err("invalid glob must fail");
        let msg = format!("{err:#}");
        assert!(!msg.is_empty(), "error should describe invalid glob: {msg}");
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

    /// #2078: max_depth applies per walk root when multiple roots are set.
    #[test]
    fn multi_root_max_depth_prunes_each_root() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("a/nested")).unwrap();
        fs::create_dir_all(dir.path().join("b/nested")).unwrap();
        fs::write(dir.path().join("a/top_a.txt"), "a\n").unwrap();
        fs::write(dir.path().join("a/nested/deep_a.txt"), "da\n").unwrap();
        fs::write(dir.path().join("b/top_b.txt"), "b\n").unwrap();
        fs::write(dir.path().join("b/nested/deep_b.txt"), "db\n").unwrap();
        fs::write(dir.path().join("root.md"), "r\n").unwrap();
        let global = GlobalFlags::test_default();
        let report = collect_list_files(
            &["a".into(), "b".into()],
            &global,
            dir.path(),
            500,
            Some(1),
            false,
        )
        .unwrap();
        let joined = report.paths.join(" ");
        assert!(joined.contains("top_a"), "a top: {:?}", report.paths);
        assert!(joined.contains("top_b"), "b top: {:?}", report.paths);
        assert!(
            !joined.contains("deep_"),
            "deep pruned per root: {:?}",
            report.paths
        );
        assert!(
            !joined.contains("root.md"),
            "outside multi-root: {:?}",
            report.paths
        );
    }

    #[test]
    fn run_lists_temp_dir_and_honors_exclude() {
        let dir = TempDir::new().unwrap();
        write_tree(dir.path());
        let mut global = GlobalFlags::with_cwd_and_json(dir.path());
        global.exclude = vec!["vendor/**".into()];
        let code = run(
            ListFilesArgs {
                paths: vec![".".into()],
                max_results: 0,
                max_depth: None,
                include_hidden: false,
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn run_missing_root_is_not_found() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::with_cwd_and_json(dir.path());
        let code = run(
            ListFilesArgs {
                paths: vec!["no-such-root".into()],
                max_results: 0,
                max_depth: None,
                include_hidden: false,
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}
