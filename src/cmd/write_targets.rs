//! Resolve CLI write dests for `md` / `doc` without dest-glob expand (#2533).
//!
//! `--glob` / `--files-from` is the include walk (same `collect_file_paths`
//! path as search/replace). A dest that looks like `*.md` is `invalid_input`,
//! not a write dest. Directory dests stay `invalid_input` unless a walk flag
//! is set (existing `doc set DIR` contract).

use crate::cli::global::GlobalFlags;
use crate::exit::{InvalidInputError, NoMatchError};
use crate::files::{collect_file_paths_opts, ensure_files_from_nonempty, looks_like_glob_dest};
use std::path::{Path, PathBuf};

/// Kind of files a write walk may return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Markdown,
    StructuredDoc,
}

/// True when `--glob` or `--files-from` requested an include walk.
#[must_use]
pub(crate) fn write_walk_requested(global: &GlobalFlags) -> bool {
    !global.glob.is_empty() || global.files_from.is_some()
}

/// Resolve dests for an `md` / `doc` write. Never dest-glob-expands.
pub(crate) fn resolve_write_targets(
    global: &GlobalFlags,
    cwd: &Path,
    explicit: &[String],
    kind: TargetKind,
) -> anyhow::Result<Vec<String>> {
    for p in explicit {
        if crate::containment::is_blank_path(p) {
            return Err(InvalidInputError {
                msg: "path must not be empty".into(),
            }
            .into());
        }
        if looks_like_glob_dest(p) {
            return Err(InvalidInputError {
                msg: format!(
                    "refusing dest glob `{p}` (not dest-glob expand). Pass --glob '{p}' and omit dest, or list concrete paths"
                ),
            }
            .into());
        }
    }

    if explicit.is_empty() && !write_walk_requested(global) {
        return Err(InvalidInputError {
            msg: "provide at least one file path, or --glob / --files-from".into(),
        }
        .into());
    }

    if !write_walk_requested(global) {
        let mut out = Vec::with_capacity(explicit.len());
        for p in explicit {
            let resolved = cwd.join(p);
            if resolved.is_dir() {
                return Err(InvalidInputError {
                    msg: format!("{} is not a file", resolved.display()),
                }
                .into());
            }
            // Keep the user spelling. Engine PathGuard / load still
            // classify missing dests (`not_found`) and Windows
            // drive-relative paths the same as pre-#2533 single-file writes.
            out.push(p.clone());
        }
        return Ok(out);
    }

    let roots: Vec<String> = if explicit.is_empty() {
        vec![".".into()]
    } else {
        explicit.to_vec()
    };
    global.check_paths_contained(cwd, &roots)?;

    let walked = collect_file_paths_opts(&roots, global, false, Some(cwd))?;
    ensure_files_from_nonempty(global, &walked)?;

    let glob_matcher = crate::files::build_glob_matcher_from_global(global)?;
    let root_paths: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    let glob_roots = crate::files::collect_glob_roots(&root_paths, Some(cwd));

    let mut out: Vec<String> = walked
        .into_iter()
        .filter(|p| crate::files::matches_glob_with_roots(p, glob_matcher.as_ref(), &glob_roots))
        .filter(|p| matches_kind(p, kind))
        .map(|p| display_under_cwd(cwd, &p))
        .collect();
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err(NoMatchError {
            msg: "no matching files".into(),
        }
        .into());
    }
    Ok(out)
}

fn matches_kind(path: &Path, kind: TargetKind) -> bool {
    match kind {
        TargetKind::Markdown => is_markdown_path(path),
        TargetKind::StructuredDoc => {
            crate::ops::doc::detect_format_from_path(&path.to_string_lossy()).is_ok()
        }
    }
}

fn is_markdown_path(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"),
        None => false,
    }
}

fn display_under_cwd(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Shift `doc set FILE SELECTOR VALUE` when `--glob` / `--files-from` replaced dest.
///
/// `doc set --glob '**/package.json' version 2.0.0` parses as
/// file=`version`, selector=`2.0.0`, value=`None`.
pub(crate) fn shift_doc_set_args(
    walk: bool,
    file: String,
    selector: String,
    value: Option<String>,
) -> anyhow::Result<(Vec<String>, String, String)> {
    if walk && value.is_none() {
        return Ok((Vec::new(), file, selector));
    }
    let value = value.ok_or_else(|| InvalidInputError {
        msg: "value is required (or pass --glob / --files-from and omit dest)".into(),
    })?;
    Ok((vec![file], selector, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn flags(dir: &Path) -> GlobalFlags {
        GlobalFlags::test_with_cwd(dir)
    }

    #[test]
    fn dest_glob_without_glob_flag_is_invalid_input() {
        let dir = TempDir::new().unwrap();
        let err = resolve_write_targets(
            &flags(dir.path()),
            dir.path(),
            &["*.md".into()],
            TargetKind::Markdown,
        )
        .unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("dest glob"), "{err}");
    }

    #[test]
    fn directory_dest_without_walk_is_not_a_file() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("docs");
        fs::create_dir(&sub).unwrap();
        let err = resolve_write_targets(
            &flags(dir.path()),
            dir.path(),
            &["docs".into()],
            TargetKind::Markdown,
        )
        .unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("not a file"), "{err}");
    }

    #[test]
    fn glob_walk_returns_matching_markdown() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "# A\n").unwrap();
        fs::write(dir.path().join("b.md"), "# B\n").unwrap();
        fs::write(dir.path().join("skip.txt"), "no\n").unwrap();
        let mut global = flags(dir.path());
        global.glob = vec!["*.md".into()];
        let got = resolve_write_targets(&global, dir.path(), &[], TargetKind::Markdown).unwrap();
        assert_eq!(got, vec!["a.md", "b.md"]);
    }

    #[test]
    fn glob_walk_returns_matching_docs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("pkg")).unwrap();
        fs::write(dir.path().join("pkg/package.json"), r#"{"version":"1"}"#).unwrap();
        fs::write(dir.path().join("notes.md"), "# n\n").unwrap();
        let mut global = flags(dir.path());
        global.glob = vec!["**/package.json".into()];
        fs::write(dir.path().join("other.json"), r#"{"version":"9"}"#).unwrap();
        let got =
            resolve_write_targets(&global, dir.path(), &[], TargetKind::StructuredDoc).unwrap();
        assert_eq!(got, vec!["pkg/package.json"]);
    }

    #[test]
    fn empty_explicit_without_walk_is_invalid_input() {
        let dir = TempDir::new().unwrap();
        let err = resolve_write_targets(&flags(dir.path()), dir.path(), &[], TargetKind::Markdown)
            .unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
    }

    #[test]
    fn shift_set_with_walk_and_missing_value() {
        let (explicit, selector, value) =
            shift_doc_set_args(true, "version".into(), "2.0.0".into(), None).unwrap();
        assert!(explicit.is_empty());
        assert_eq!(selector, "version");
        assert_eq!(value, "2.0.0");
    }

    #[test]
    fn shift_set_without_walk_requires_value() {
        let err = shift_doc_set_args(false, "a.json".into(), "version".into(), None).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err}");
    }

    #[test]
    fn explicit_file_is_rewritten() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "# A\n").unwrap();
        let got = resolve_write_targets(
            &flags(dir.path()),
            dir.path(),
            &["a.md".into()],
            TargetKind::Markdown,
        )
        .unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].ends_with("a.md"), "{got:?}");
    }
}
