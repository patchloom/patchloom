//! Disk apply for Codex `*** Begin Patch` (#2219).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::containment::PathGuard;
use crate::ops::begin_patch::{
    BeginPatchOp, apply_codex_hunks, parse_begin_patch, resolve_begin_patch_dest,
};
use crate::ops::file::{is_regular_file_for_backup, path_entry_exists};

use super::{ApplyMode, EditResult};

/// Apply a Codex Begin Patch document under `cwd`.
///
/// `file_hint` remaps dests that are the hint path or a suffix of it
/// (same-basename other directory stays off the hint).
pub fn apply_begin_patch(
    patch: &str,
    cwd: &Path,
    file_hint: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let ops = parse_begin_patch(patch)?;
    apply_begin_patch_ops(&ops, cwd, file_hint, mode, guard)
}

pub(crate) fn apply_begin_patch_ops(
    ops: &[BeginPatchOp],
    cwd: &Path,
    file_hint: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    #[derive(Clone)]
    enum StageOp {
        Write {
            write_path: PathBuf,
            display: String,
            original: String,
            new_content: String,
            is_creation: bool,
        },
        Delete {
            path: PathBuf,
            display: String,
            original: String,
        },
        Rename {
            from: PathBuf,
            to: PathBuf,
            from_display: String,
            to_display: String,
            original: String,
            new_content: String,
        },
    }

    let mut staged: Vec<StageOp> = Vec::new();
    // Same pending/delete view as tx `dest_exists` so Preview (`patch check`)
    // agrees with apply on Delete-then-Add / Add-then-Delete in one document.
    let mut created: HashMap<PathBuf, String> = HashMap::new();
    let mut deleted: HashSet<PathBuf> = HashSet::new();
    let staged_exists =
        |path: &Path, created: &HashMap<PathBuf, String>, deleted: &HashSet<PathBuf>| {
            if created.contains_key(path) {
                return true;
            }
            if deleted.contains(path) {
                return false;
            }
            path_entry_exists(path)
        };
    for op in ops {
        match op {
            BeginPatchOp::Add { path, content } => {
                let dest = resolve_begin_patch_dest(cwd, path, file_hint);
                super::ensure_contained(guard, &dest)?;
                if dest.is_dir() {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!(
                            "{} is a directory; Begin Patch Add File needs a file path",
                            dest.display()
                        ),
                    }));
                }
                if staged_exists(&dest, &created, &deleted) {
                    return Err(anyhow::Error::new(crate::exit::AlreadyExistsError {
                        msg: crate::ops::patch::create_dest_exists_msg(path),
                    }));
                }
                created.insert(dest.clone(), content.clone());
                deleted.remove(&dest);
                staged.push(StageOp::Write {
                    write_path: dest,
                    display: path.clone(),
                    original: String::new(),
                    new_content: content.clone(),
                    is_creation: true,
                });
            }
            BeginPatchOp::Delete { path } => {
                let dest = resolve_begin_patch_dest(cwd, path, file_hint);
                super::ensure_contained_entry(guard, &dest)?;
                if dest.is_dir() {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("{} is a directory, not a file", dest.display()),
                    }));
                }
                if !staged_exists(&dest, &created, &deleted) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file not found: {path}"),
                    )
                    .into());
                }
                let original = created.get(&dest).cloned().unwrap_or_else(|| {
                    // Same snapshot rule as file_delete: do not follow
                    // symlink / FIFO / socket after entry PathGuard.
                    if is_regular_file_for_backup(&dest) {
                        crate::files::load_text_strict(&dest, path).unwrap_or_default()
                    } else {
                        String::new()
                    }
                });
                created.remove(&dest);
                deleted.insert(dest.clone());
                staged.push(StageOp::Delete {
                    path: dest,
                    display: path.clone(),
                    original,
                });
            }
            BeginPatchOp::Update {
                path,
                hunks,
                move_to,
            } => {
                let dest = resolve_begin_patch_dest(cwd, path, file_hint);
                super::ensure_contained(guard, &dest)?;
                if dest.is_dir() {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!(
                            "{} is a directory; Begin Patch Update File needs a file path",
                            dest.display()
                        ),
                    }));
                }
                if !staged_exists(&dest, &created, &deleted) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file not found: {path}"),
                    )
                    .into());
                }
                let original = if let Some(s) = created.get(&dest) {
                    s.clone()
                } else {
                    crate::files::load_text_strict(&dest, path)?
                };
                let updated = apply_codex_hunks(&original, hunks)?;
                if let Some(new_path) = move_to {
                    let new_dest = resolve_begin_patch_dest(cwd, new_path, None);
                    super::ensure_contained_entry(guard, &new_dest)?;
                    if staged_exists(&new_dest, &created, &deleted) {
                        return Err(anyhow::Error::new(crate::exit::AlreadyExistsError {
                            msg: format!(
                                "destination already exists: {} (Begin Patch Move refuses overwrite; remove dest)",
                                new_dest.display()
                            ),
                        }));
                    }
                    created.remove(&dest);
                    deleted.insert(dest.clone());
                    created.insert(new_dest.clone(), updated.clone());
                    deleted.remove(&new_dest);
                    staged.push(StageOp::Rename {
                        from: dest,
                        to: new_dest,
                        from_display: path.clone(),
                        to_display: new_path.clone(),
                        original,
                        new_content: updated,
                    });
                } else {
                    created.insert(dest.clone(), updated.clone());
                    deleted.remove(&dest);
                    staged.push(StageOp::Write {
                        write_path: dest,
                        display: path.clone(),
                        original,
                        new_content: updated,
                        is_creation: false,
                    });
                }
            }
        }
    }

    let policy = crate::write::WritePolicy::default();
    let (applied, backup_session) = if mode == ApplyMode::Apply {
        for op in &staged {
            match op {
                StageOp::Write { write_path, .. } => super::ensure_contained(guard, write_path)?,
                StageOp::Delete { path, .. } => super::ensure_contained_entry(guard, path)?,
                StageOp::Rename { from, to, .. } => {
                    super::ensure_contained_entry(guard, from)?;
                    super::ensure_contained_entry(guard, to)?;
                }
            }
        }
        let mut backup = crate::backup::BackupSession::new(cwd)?;
        for op in &staged {
            match op {
                StageOp::Write { write_path, .. } => {
                    backup.save_before_write(write_path)?;
                }
                StageOp::Delete { path, .. } => {
                    if path_entry_exists(path) {
                        backup.save_before_delete(path)?;
                    }
                }
                StageOp::Rename { from, to, .. } => {
                    if path_entry_exists(from) {
                        backup.save_before_delete(from)?;
                    }
                    if to != from {
                        backup.save_before_write(to)?;
                    }
                }
            }
        }
        let session = backup.finalize()?;
        let write_result = (|| -> anyhow::Result<()> {
            for op in &staged {
                match op {
                    StageOp::Write {
                        write_path,
                        new_content,
                        ..
                    } => {
                        if let Some(parent) = write_path.parent()
                            && !parent.as_os_str().is_empty()
                            && !parent.exists()
                        {
                            std::fs::create_dir_all(parent)?;
                        }
                        crate::write::atomic_write(write_path, new_content, &policy)?;
                    }
                    StageOp::Delete { path, .. } => {
                        if path_entry_exists(path) {
                            std::fs::remove_file(path).map_err(|e| {
                                anyhow::anyhow!(
                                    "Begin Patch delete: failed to remove {}: {e}",
                                    path.display()
                                )
                            })?;
                        }
                    }
                    StageOp::Rename {
                        from,
                        to,
                        original,
                        new_content,
                        ..
                    } => {
                        if let Some(parent) = to.parent()
                            && !parent.as_os_str().is_empty()
                            && !parent.exists()
                        {
                            std::fs::create_dir_all(parent)?;
                        }
                        crate::ops::file::rename_or_copy(from, to)?;
                        if new_content != original {
                            crate::write::atomic_write(to, new_content, &policy)?;
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = write_result {
            return Err(super::mutation_err_after_backup(cwd, session.as_deref(), e));
        }
        (true, session)
    } else {
        (false, None)
    };

    let mut results = Vec::with_capacity(staged.len());
    for op in staged {
        let mut edit = match op {
            StageOp::Write {
                display,
                original,
                new_content,
                is_creation,
                ..
            } => {
                let mut e = super::build_edit_result(
                    &display,
                    original,
                    new_content,
                    applied,
                    "patch",
                    None,
                );
                if is_creation {
                    e.changed = true;
                }
                e
            }
            StageOp::Delete {
                display, original, ..
            } => {
                let mut e = super::build_edit_result(
                    &display,
                    original,
                    String::new(),
                    applied,
                    "patch",
                    None,
                );
                e.changed = true;
                e
            }
            StageOp::Rename {
                from_display,
                to_display,
                original,
                new_content,
                ..
            } => {
                let mut e = super::build_edit_result(
                    &to_display,
                    original,
                    new_content,
                    applied,
                    "patch",
                    Some(to_display.clone()),
                );
                e.changed = true;
                e.path = from_display;
                e.dest_path = Some(to_display);
                e
            }
        };
        edit.backup_session = backup_session.clone();
        results.push(edit);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{is_already_exists, is_ambiguous, is_guard_rejected, is_no_match};
    #[cfg(unix)]
    use crate::containment::AbsolutePathPolicy;
    use crate::containment::PathGuard;
    use crate::ops::begin_patch::looks_like_begin_patch;

    fn update_patch(path: &str, old: &str, new: &str) -> String {
        format!("*** Begin Patch\n*** Update File: {path}\n@@\n-{old}\n+{new}\n*** End Patch\n")
    }

    #[test]
    fn apply_begin_patch_update_unique() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
        let patch = update_patch("code.rs", "fn old() {}", "fn new() {}");
        assert!(looks_like_begin_patch(&patch));
        let results =
            apply_begin_patch(&patch, dir.path(), None, ApplyMode::Apply, None).expect("update");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
            "fn new() {}\n"
        );
        assert!(results[0].backup_session.is_some());
    }

    #[test]
    fn apply_patch_detects_begin_patch_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("code.rs");
        std::fs::write(&dest, "fn old() {}\n").unwrap();
        let patch = update_patch("code.rs", "fn old() {}", "fn new() {}");
        let result = crate::api::apply_patch(&dest, &patch, ApplyMode::Apply, None)
            .expect("apply_patch detects Begin Patch");
        assert!(result.applied);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "fn new() {}\n");
    }

    #[test]
    fn apply_patch_file_detects_begin_patch_envelope() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
        let patch = update_patch("code.rs", "fn old() {}", "fn new() {}");
        let results = crate::api::apply_patch_file(&patch, dir.path(), ApplyMode::Apply, None)
            .expect("apply_patch_file detects Begin Patch");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
            "fn new() {}\n"
        );
    }

    #[test]
    fn apply_begin_patch_update_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "x\nx\n").unwrap();
        let patch = update_patch("code.rs", "x", "y");
        let err = apply_begin_patch(&patch, dir.path(), None, ApplyMode::Apply, None)
            .expect_err("ambiguous");
        assert!(is_ambiguous(&err));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
            "x\nx\n"
        );
    }

    #[test]
    fn apply_begin_patch_add_delete_move() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gone.rs"), "bye\n").unwrap();
        std::fs::write(dir.path().join("src.rs"), "fn a() {}\n").unwrap();
        let patch = "\
*** Begin Patch
*** Add File: new.rs
+hello
*** Update File: src.rs
*** Move to: dest.rs
@@
-fn a() {}
+fn b() {}
*** Delete File: gone.rs
*** End Patch
";
        let results =
            apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, None).expect("mix");
        assert_eq!(results.len(), 3);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.rs")).unwrap(),
            "hello\n"
        );
        assert!(!dir.path().join("gone.rs").exists());
        assert!(!dir.path().join("src.rs").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dest.rs")).unwrap(),
            "fn b() {}\n"
        );
    }

    #[test]
    fn apply_begin_patch_mixed_grammar() {
        let dir = tempfile::tempdir().unwrap();
        let patch = "\
*** Begin Patch
*** Update File: code.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
--- a/other.rs
+++ b/other.rs
";
        let err = apply_begin_patch(patch, dir.path(), None, ApplyMode::Preview, None)
            .expect_err("mixed");
        assert!(crate::exit::is_parse_error(&err));
        assert!(
            err.to_string()
                .contains("mixed Begin Patch and unified diff grammar"),
            "expected mixed-grammar peel, got {err}"
        );
    }

    #[test]
    fn apply_begin_patch_dest_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("from.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.path().join("taken.rs"), "keep\n").unwrap();
        let patch = "\
*** Begin Patch
*** Update File: from.rs
*** Move to: taken.rs
@@
-fn a() {}
+fn b() {}
*** End Patch
";
        let err =
            apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, None).expect_err("exists");
        assert!(is_already_exists(&err));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("taken.rs")).unwrap(),
            "keep\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("from.rs")).unwrap(),
            "fn a() {}\n"
        );
    }

    #[test]
    fn apply_begin_patch_add_dest_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("new.rs"), "keep\n").unwrap();
        let patch = "\
*** Begin Patch
*** Add File: new.rs
+fn added() {}
*** End Patch
";
        let err =
            apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, None).expect_err("exists");
        assert!(is_already_exists(&err));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("new.rs")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    fn apply_begin_patch_delete_then_add_same_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("swap.rs"), "old\n").unwrap();
        let patch = "\
*** Begin Patch
*** Delete File: swap.rs
*** Add File: swap.rs
+new
*** End Patch
";
        apply_begin_patch(patch, dir.path(), None, ApplyMode::Preview, None)
            .expect("preview delete-then-add");
        apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, None)
            .expect("apply delete-then-add");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("swap.rs")).unwrap(),
            "new\n"
        );
    }

    #[test]
    fn apply_begin_patch_add_then_delete_missing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let patch = "\
*** Begin Patch
*** Add File: brand.rs
+hello
*** Delete File: brand.rs
*** End Patch
";
        apply_begin_patch(patch, dir.path(), None, ApplyMode::Preview, None)
            .expect("preview add-then-delete");
        apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, None)
            .expect("apply add-then-delete");
        assert!(!dir.path().join("brand.rs").exists());
    }

    #[test]
    fn apply_begin_patch_path_guard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.rs"), "fn old() {}\n").unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .build()
            .unwrap();
        let patch = update_patch("../escape.rs", "fn old() {}", "fn new() {}");
        let err = apply_begin_patch(
            patch.as_str(),
            dir.path(),
            None,
            ApplyMode::Apply,
            Some(&guard),
        )
        .expect_err("guard");
        assert!(
            is_guard_rejected(&err),
            "escape dest must peel guard_rejected, got {err}"
        );
        // Ensure in.rs unchanged and no write outside.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("in.rs")).unwrap(),
            "fn old() {}\n"
        );
    }

    #[test]
    fn apply_begin_patch_file_hint_remaps_suffix_not_other_dir() {
        let dir = tempfile::tempdir().unwrap();
        let hint_dir = dir.path().join("crates").join("cli").join("src");
        std::fs::create_dir_all(&hint_dir).unwrap();
        let other_dir = dir.path().join("crates").join("tools").join("src");
        std::fs::create_dir_all(&other_dir).unwrap();
        let hint = hint_dir.join("main.rs");
        std::fs::write(&hint, "fn old() {}\n").unwrap();
        std::fs::write(other_dir.join("main.rs"), "fn other() {}\n").unwrap();

        let patch = update_patch("main.rs", "fn old() {}", "fn new() {}");
        apply_begin_patch(&patch, &hint_dir, Some(&hint), ApplyMode::Apply, None).expect("hint");
        assert_eq!(std::fs::read_to_string(&hint).unwrap(), "fn new() {}\n");

        let other_patch = update_patch(
            "crates/tools/src/main.rs",
            "fn other() {}",
            "fn changed() {}",
        );
        apply_begin_patch(
            &other_patch,
            dir.path(),
            Some(&hint),
            ApplyMode::Apply,
            None,
        )
        .expect("other basename");
        assert_eq!(
            std::fs::read_to_string(&hint).unwrap(),
            "fn new() {}\n",
            "hint file must stay when dest is another directory"
        );
        assert_eq!(
            std::fs::read_to_string(other_dir.join("main.rs")).unwrap(),
            "fn changed() {}\n"
        );
    }

    #[test]
    fn apply_begin_patch_no_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn live() {}\n").unwrap();
        let patch = update_patch("code.rs", "fn missing() {}", "fn x() {}");
        let err =
            apply_begin_patch(&patch, dir.path(), None, ApplyMode::Apply, None).expect_err("miss");
        assert!(is_no_match(&err));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
            "fn live() {}\n"
        );
    }

    /// Delete of a workspace symlink must not snapshot the outside target
    /// into `original_content` / diffs (file_delete snapshot rule).
    #[cfg(unix)]
    #[test]
    fn apply_begin_patch_delete_symlink_outside_does_not_leak_target() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.env");
        const PAYLOAD: &str = "SECRET=outside-do-not-leak\n";
        std::fs::write(&secret, PAYLOAD).unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&secret, &link).unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let patch = "*** Begin Patch\n*** Delete File: link.txt\n*** End Patch\n";
        let results = apply_begin_patch(patch, dir.path(), None, ApplyMode::Apply, Some(&guard))
            .expect("entry delete of workspace symlink");
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].original_content.contains(PAYLOAD.trim()),
            "delete snapshot must not follow symlink: {:?}",
            results[0].original_content
        );
        assert!(
            !results[0].diff.contains(PAYLOAD.trim()),
            "diff must not leak outside target: {}",
            results[0].diff
        );
        assert!(
            !crate::ops::file::path_entry_exists(&link),
            "symlink entry must be unlinked"
        );
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), PAYLOAD);
    }
}
