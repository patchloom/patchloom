use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::Context;
use clap::Args;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::process;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom status
  patchloom status --json

NOTE:
  Paths under .patchloom/ (undo backups from --apply) are omitted so status
  reflects project files only. Use git status if you need untracked backups.")]
pub struct StatusArgs {
    /// Paths to check (defaults to current directory).
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusOutput {
    /// True only when the tree is clean. Dirty trees set ok:false +
    /// error_kind/status changes_detected so agents that branch on ok match
    /// CLI exit 2 (same pattern as md lint / tx lint).
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    modified: Vec<String>,
    created: Vec<String>,
    deleted: Vec<String>,
    total_changes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileCategory {
    Created,
    Deleted,
    Modified,
}

/// Paths under `.patchloom/` (backup sessions) should not appear in status.
fn is_patchloom_internal_path(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    p == ".patchloom" || p.starts_with(".patchloom/")
}

/// Absolute form of `path`. Prefer canonicalize so `--cwd` `/var/folders`
/// and git toplevel `/private/var/folders` compare equal on macOS.
fn make_absolute(path: &Path) -> PathBuf {
    if let Ok(canon) = path.canonicalize() {
        return canon;
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Join a porcelain path (repo-root relative, `/` even on Windows) onto the
/// work-tree root.
fn join_repo_git_path(repo_root: &Path, git_path: &str) -> PathBuf {
    let mut out = repo_root.to_path_buf();
    for part in git_path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        out.push(part);
    }
    out
}

fn relative_from(from: &Path, to: &Path) -> Option<String> {
    let from_c: Vec<Component<'_>> = from.components().collect();
    let to_c: Vec<Component<'_>> = to.components().collect();
    let mut i = 0;
    while i < from_c.len() && i < to_c.len() && from_c[i] == to_c[i] {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for c in &from_c[i..] {
        match c {
            Component::Normal(_) | Component::ParentDir => parts.push("..".into()),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    for c in &to_c[i..] {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".into()),
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    if parts.is_empty() {
        Some(".".into())
    } else {
        Some(parts.join("/"))
    }
}

/// Emit a porcelain path relative to `--cwd` so agents can read/replace it
/// with the same `--cwd`. Falls back to `../` for siblings, then repo-relative.
fn display_path_for_cwd(cwd: &Path, repo_root: &Path, git_path: &str) -> String {
    let cwd_abs = make_absolute(cwd);
    let repo_abs = make_absolute(repo_root);
    let file_abs = join_repo_git_path(&repo_abs, git_path);

    if let Ok(rel) = file_abs.strip_prefix(&cwd_abs) {
        if rel.as_os_str().is_empty() {
            return ".".to_string();
        }
        return rel.to_string_lossy().replace('\\', "/");
    }

    if cwd_abs.starts_with(&repo_abs)
        && file_abs.starts_with(&repo_abs)
        && let Some(rel) = relative_from(&cwd_abs, &file_abs)
    {
        return rel;
    }

    git_path.to_string()
}

/// Work-tree root for porcelain paths. Same `current_dir` as `git status`.
fn git_work_tree_root(cwd: &Path) -> anyhow::Result<PathBuf> {
    let output = process::Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run `git rev-parse --show-toplevel` -- is git installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("git rev-parse --show-toplevel failed: {stderr}"),
        }));
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(make_absolute(Path::new(&raw)))
}

/// Parse one NUL-delimited `git status --porcelain=v1 -z` record.
fn parse_porcelain_record(record: &[u8]) -> Option<(FileCategory, String)> {
    if record.len() < 4 {
        return None;
    }
    let xy = std::str::from_utf8(&record[..2]).ok()?;
    let file = String::from_utf8_lossy(&record[3..]).into_owned();
    let category = match xy.trim() {
        "??" | "A" | "AM" => FileCategory::Created,
        "D" | "DD" | "AD" | "MD" => FileCategory::Deleted,
        _ => FileCategory::Modified,
    };
    Some((category, file))
}

/// Collect git status without writing to stdout.
pub(crate) fn collect_status(
    paths: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<StatusOutput> {
    collect_status_with_list(paths, global, None)
}

/// Like [`collect_status`], with a pre-read `--files-from` list so stdin `-`
/// is not consumed twice (contain check in [`run`] + collect).
fn collect_status_with_list(
    paths: &[String],
    global: &GlobalFlags,
    files_from_preload: Option<&[String]>,
) -> anyhow::Result<StatusOutput> {
    let cwd = global.resolve_cwd()?;

    let files_owned;
    let files_from: Option<&[String]> = if let Some(pre) = files_from_preload {
        Some(pre)
    } else if global.files_from.is_some() {
        files_owned = global.read_files_from()?;
        files_owned.as_deref()
    } else {
        None
    };
    if let Some(list) = files_from {
        let listed: Vec<PathBuf> = list.iter().map(PathBuf::from).collect();
        crate::files::ensure_files_from_nonempty(global, &listed)?;
        global.check_paths_contained(&cwd, list)?;
    }
    // `--files-from` is the scan list (same as search/tidy), not a later filter.
    let status_paths: &[String] = files_from.unwrap_or(paths);

    let mut cmd = process::Command::new("git");
    cmd.current_dir(&cwd)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("--no-renames")
        .arg("--untracked-files=all")
        .arg("-z");
    if !status_paths.is_empty() {
        cmd.arg("--");
        for path in status_paths {
            cmd.arg(path);
        }
    }
    let output = cmd
        .output()
        .context("failed to run `git status` -- is git installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "git status failed: {stderr}\nhint: run `git init` first, or run patchloom status from inside an existing git repository"
            ),
        }));
    }

    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let glob_roots = crate::collect_glob_roots_from_global(status_paths, global, Some(&cwd))?;
    // Align glob roots with canonical porcelain paths (`/var` vs `/private/var`).
    let glob_roots: Vec<PathBuf> = glob_roots
        .into_iter()
        .map(|p| p.canonicalize().unwrap_or(p))
        .collect();
    // Porcelain paths are work-tree relative even when git runs from a subdir.
    let repo_root = git_work_tree_root(&cwd)?;

    let mut modified = Vec::new();
    let mut created = Vec::new();
    let mut deleted = Vec::new();

    for record in output
        .stdout
        .split(|b| *b == 0)
        .filter(|record| !record.is_empty())
    {
        let (category, file) = match parse_porcelain_record(record) {
            Some(v) => v,
            None => continue,
        };

        // Internal backup store is not a user-facing repo change (#1349 for
        // tidy; same noise on `status` after any --apply). Filter the
        // repo-relative porcelain path before --cwd rewrite.
        if is_patchloom_internal_path(&file) {
            continue;
        }

        let file_path = join_repo_git_path(&repo_root, &file);
        if let Some(ref matcher) = glob_matcher
            && !crate::matches_glob_with_roots(&file_path, Some(matcher), &glob_roots)
        {
            continue;
        }

        let display = display_path_for_cwd(&cwd, &repo_root, &file);
        match category {
            FileCategory::Created => created.push(display),
            FileCategory::Deleted => deleted.push(display),
            FileCategory::Modified => modified.push(display),
        }
    }

    let total_changes = modified.len() + created.len() + deleted.len();
    let dirty = total_changes > 0;

    Ok(StatusOutput {
        ok: !dirty,
        status: if dirty {
            Some("changes_detected")
        } else {
            Some("clean")
        },
        error_kind: if dirty {
            Some("changes_detected")
        } else {
            None
        },
        modified,
        created,
        deleted,
        total_changes,
    })
}

pub fn run(args: StatusArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("status: checking {} path(s)", args.paths.len());
    let cwd = global.resolve_cwd()?;
    // Read once so `--files-from -` is not consumed twice (contain + collect).
    let files_from_list = global.read_files_from()?;
    global.check_paths_contained(&cwd, &args.paths)?;
    if let Some(ref list) = files_from_list {
        global.check_paths_contained(&cwd, list)?;
    }
    let out = match collect_status_with_list(&args.paths, global, files_from_list.as_deref()) {
        Ok(o) => o,
        Err(e) => {
            // Outside a git repo, missing git binary, etc.
            global.emit_error_json_kind(Some("invalid_input"), &format!("{e:#}"))?;
            return Ok(exit::FAILURE);
        }
    };
    crate::verbose!(
        "status: {} modified, {} created, {} deleted",
        out.modified.len(),
        out.created.len(),
        out.deleted.len()
    );

    if !global.emit_json(&out)? && !global.quiet {
        for f in &out.modified {
            println!("M  {f}");
        }
        for f in &out.created {
            println!("A  {f}");
        }
        for f in &out.deleted {
            println!("D  {f}");
        }
        if out.total_changes > 0 {
            println!("{} file(s) changed", out.total_changes);
        }
    }

    if out.total_changes > 0 {
        Ok(exit::CHANGES_DETECTED)
    } else {
        Ok(exit::SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patchloom_internal_paths_are_filtered() {
        assert!(is_patchloom_internal_path(".patchloom"));
        assert!(is_patchloom_internal_path(
            ".patchloom/backups/1/manifest.json"
        ));
        assert!(is_patchloom_internal_path("./.patchloom/backups/x"));
        assert!(!is_patchloom_internal_path("src/main.rs"));
        assert!(!is_patchloom_internal_path("patchloom.toml"));
    }

    #[test]
    fn parse_untracked_file() {
        let (cat, file) = parse_porcelain_record(b"?? new.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "new.txt");
    }

    #[test]
    fn parse_staged_new_file() {
        let (cat, file) = parse_porcelain_record(b"A  staged.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "staged.txt");
    }

    #[test]
    fn parse_staged_and_modified() {
        let (cat, file) = parse_porcelain_record(b"AM both.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "both.txt");
    }

    #[test]
    fn parse_deleted_file() {
        let (cat, file) = parse_porcelain_record(b"D  gone.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
        assert_eq!(file, "gone.txt");
    }

    #[test]
    fn parse_modified_file() {
        let (cat, file) = parse_porcelain_record(b" M changed.txt").unwrap();
        assert_eq!(cat, FileCategory::Modified);
        assert_eq!(file, "changed.txt");
    }

    #[test]
    fn parse_short_line_returns_none() {
        assert!(parse_porcelain_record(b"??").is_none());
        assert!(parse_porcelain_record(b"A").is_none());
        assert!(parse_porcelain_record(b"").is_none());
    }

    #[test]
    fn parse_filename_with_spaces() {
        let (cat, file) = parse_porcelain_record(b"?? file name.txt").unwrap();
        assert_eq!(cat, FileCategory::Created);
        assert_eq!(file, "file name.txt");
    }

    #[test]
    fn parse_compound_deletion_codes() {
        // DD = unmerged, both deleted
        let (cat, _) = parse_porcelain_record(b"DD file.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
        // AD = added in index, deleted in worktree
        let (cat, _) = parse_porcelain_record(b"AD file.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
        // MD = modified in index, deleted in worktree
        let (cat, _) = parse_porcelain_record(b"MD file.txt").unwrap();
        assert_eq!(cat, FileCategory::Deleted);
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) {
        let output = process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:{}\nstderr:{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_git_repo_with_committed_file(dir: &std::path::Path, file: &str, content: &str) {
        git_ok(dir, &["init"]);
        git_ok(dir, &["config", "user.email", "test@test.com"]);
        git_ok(dir, &["config", "user.name", "Test"]);
        if let Some(parent) = std::path::Path::new(file).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir.join(parent)).unwrap();
        }
        std::fs::write(dir.join(file), content).unwrap();
        git_ok(dir, &["add", "--", file]);
        git_ok(dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn display_path_repo_root_cwd_keeps_repo_relative() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let got = display_path_for_cwd(dir.path(), dir.path(), "src/foo.rs");
        assert_eq!(got, "src/foo.rs");
    }

    #[test]
    fn display_path_nested_cwd_strips_prefix() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let got = display_path_for_cwd(&src, dir.path(), "src/foo.rs");
        assert_eq!(got, "foo.rs");
    }

    #[test]
    fn display_path_sibling_uses_parent_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let got = display_path_for_cwd(&src, dir.path(), "other.rs");
        assert_eq!(got, "../other.rs");
    }

    #[test]
    fn collect_status_nested_cwd_emits_cwd_relative_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "src/foo.rs", "old\n");
        std::fs::write(dir.path().join("src/foo.rs"), "new\n").unwrap();

        let src = dir.path().join("src");
        let global = GlobalFlags {
            cwd: Some(src.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let out = collect_status(&[], &global).unwrap();
        assert!(
            out.modified.iter().any(|p| p == "foo.rs"),
            "nested --cwd must emit foo.rs, got {:?}",
            out.modified
        );
        assert!(
            !out.modified
                .iter()
                .any(|p| p == "src/foo.rs" || p.ends_with("/src/foo.rs")),
            "must not emit repo-relative src/foo.rs under --cwd src, got {:?}",
            out.modified
        );
    }

    #[test]
    fn collect_status_repo_root_cwd_keeps_repo_relative_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "src/foo.rs", "old\n");
        std::fs::write(dir.path().join("src/foo.rs"), "new\n").unwrap();

        let global = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let out = collect_status(&[], &global).unwrap();
        assert!(
            out.modified.iter().any(|p| p == "src/foo.rs"),
            "repo-root --cwd must keep src/foo.rs, got {:?}",
            out.modified
        );
    }

    #[test]
    fn collect_status_glob_from_nested_cwd_matches_real_path() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "src/foo.rs", "old\n");
        std::fs::write(dir.path().join("src/foo.rs"), "new\n").unwrap();
        std::fs::write(dir.path().join("src/bar.txt"), "x\n").unwrap();

        let src = dir.path().join("src");
        let global = GlobalFlags {
            cwd: Some(src.to_string_lossy().into_owned()),
            glob: vec!["*.rs".into()],
            ..GlobalFlags::default()
        };
        let out = collect_status(&[], &global).unwrap();
        assert!(
            out.modified.iter().any(|p| p == "foo.rs"),
            "glob *.rs from --cwd src must match real src/foo.rs, got {:?}",
            out.modified
        );
        assert!(
            !out.created.iter().any(|p| p.ends_with("bar.txt")),
            "glob *.rs must not include bar.txt, got created={:?}",
            out.created
        );
    }

    #[test]
    fn collect_status_nested_cwd_still_filters_patchloom() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "src/foo.rs", "old\n");
        std::fs::write(dir.path().join("src/foo.rs"), "new\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".patchloom/backups")).unwrap();
        std::fs::write(dir.path().join(".patchloom/backups/x"), "bak\n").unwrap();

        let src = dir.path().join("src");
        let global = GlobalFlags {
            cwd: Some(src.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let out = collect_status(&[], &global).unwrap();
        let all: Vec<&String> = out
            .modified
            .iter()
            .chain(out.created.iter())
            .chain(out.deleted.iter())
            .collect();
        assert!(
            !all.iter().any(|p| p.contains(".patchloom")),
            "must still omit .patchloom from nested --cwd, got {all:?}"
        );
        assert!(
            out.modified.iter().any(|p| p == "foo.rs"),
            "dirty src/foo.rs should remain, got {:?}",
            out.modified
        );
    }

    #[test]
    fn collect_status_files_from_limits_to_listed_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "a.txt", "old-a\n");
        std::fs::write(dir.path().join("b.txt"), "old-b\n").unwrap();
        git_ok(dir.path(), &["add", "--", "b.txt"]);
        git_ok(dir.path(), &["commit", "-m", "add b"]);
        std::fs::write(dir.path().join("a.txt"), "new-a\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new-b\n").unwrap();
        std::fs::write(dir.path().join("list.txt"), "a.txt\n").unwrap();

        let global = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            files_from: Some("list.txt".into()),
            ..GlobalFlags::default()
        };
        let out = collect_status(&[], &global).unwrap();
        assert!(
            out.modified.iter().any(|p| p == "a.txt"),
            "files-from a.txt must report a.txt, got {:?}",
            out.modified
        );
        assert!(
            !out.modified.iter().any(|p| p == "b.txt")
                && !out.created.iter().any(|p| p == "b.txt")
                && !out.deleted.iter().any(|p| p == "b.txt"),
            "files-from a.txt must omit dirty b.txt, got modified={:?} created={:?} deleted={:?}",
            out.modified,
            out.created,
            out.deleted
        );
        assert_eq!(out.total_changes, 1, "expected only a.txt, got {out:?}");
    }

    #[test]
    fn collect_status_empty_files_from_is_invalid_input() {
        let dir = tempfile::TempDir::new().unwrap();
        init_git_repo_with_committed_file(dir.path(), "a.txt", "old\n");
        std::fs::write(dir.path().join("a.txt"), "new\n").unwrap();
        std::fs::write(dir.path().join("empty.txt"), "").unwrap();

        let global = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            files_from: Some("empty.txt".into()),
            ..GlobalFlags::default()
        };
        let err = collect_status(&[], &global).unwrap_err();
        assert_eq!(
            crate::exit::classify_typed_error(&err).map(|(kind, _)| kind),
            Some("invalid_input"),
            "empty --files-from must be invalid_input, not a clean tree: {err:#}"
        );
        let code = run(StatusArgs { paths: vec![] }, &global).unwrap();
        assert_eq!(
            code,
            exit::FAILURE,
            "empty --files-from must not report clean success"
        );
    }
}
