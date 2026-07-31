//! Patch (unified diff) apply operations for the library API.
//!
//! Single-file `apply_patch` delegates to the tx engine via `execute_as_edit_result`.
//! Multi-file `apply_patch_file` retains a direct implementation.

use std::path::Path;

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Apply a unified diff patch to a file.
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(
    path: &Path,
    patch_text: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::PatchApply {
        diff: patch_text.into(),
        on_stale: Default::default(),
        allow_conflicts: false,
    };
    // Resolve cwd so multi-component relative paths (and git-style patch
    // paths that match the caller path) join correctly.
    let abs = super::absolute_for_engine(path).map_err(|e| {
        crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::OperationFailed,
            format!("failed to resolve path {}: {e}", path.display()),
        )
    })?;
    let cwd_owned: std::path::PathBuf;
    let cwd = if path.is_absolute() {
        abs.parent().unwrap_or_else(|| Path::new("."))
    } else {
        // path "src/lib.rs" → strip components so cwd is project root and
        // patch path "src/lib.rs" resolves once.
        cwd_owned = abs
            .ancestors()
            .nth(path.components().count())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        cwd_owned.as_path()
    };
    let display = path.to_string_lossy();
    patch_write(op, cwd, mode, guard, Some(display.as_ref()))
}

#[cfg(any(feature = "cli", feature = "files"))]
fn patch_write(
    op: Operation,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    display_path: Option<&str>,
) -> anyhow::Result<EditResult> {
    super::execute_as_edit_result_with_path(op, mode, cwd, guard, "patch", None, display_path)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn patch_write(
    _op: Operation,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    _display_path: Option<&str>,
) -> anyhow::Result<EditResult> {
    use crate::ops;

    if let Operation::PatchApply { diff, .. } = _op {
        let patch_files = ops::patch::parse_patch(&diff).map_err(|e| {
            anyhow::Error::new(crate::exit::ParseErrorError {
                msg: format!("patch parse error: {e}"),
            })
        })?;

        if patch_files.is_empty() {
            return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                msg: "no files in patch".into(),
            }));
        }

        // Apply to the first file in the patch (git rename: load old path).
        let pf = &patch_files[0];
        let load_rel = pf.rename_from.as_deref().unwrap_or(pf.path.as_str());
        let load_path = cwd.join(load_rel);
        let write_path = cwd.join(&pf.path);
        // Strict sole-path (#1894): binary / invalid UTF-8 → Binary / InvalidEncoding.
        let original = if pf.is_creation {
            String::new()
        } else {
            crate::files::load_text_strict(&load_path, load_rel)?
        };

        let new_content = ops::patch::apply_hunks(&original, &pf.hunks).map_err(|e| {
            if e.contains("stale context") {
                anyhow::Error::new(crate::exit::AmbiguousError {
                    msg: format!("patch apply error: {e}"),
                })
            } else {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("patch apply error: {e}"),
                })
            }
        })?;

        let policy = crate::write::WritePolicy::default();
        let (applied, backup_session) =
            super::write_if_apply(&write_path, &new_content, mode, &policy, guard)?;
        if applied {
            if let Some(ref from) = pf.rename_from {
                let old = cwd.join(from);
                if crate::ops::file::path_entry_exists(&old) {
                    let _ = std::fs::remove_file(&old);
                }
            }
        }
        {
            let mut __e =
                super::build_edit_result(&pf.path, original, new_content, applied, "patch", None);
            __e.backup_session = backup_session;
            Ok(__e)
        }
    } else {
        anyhow::bail!("expected PatchApply operation")
    }
}

/// Apply a multi-file patch. Returns one `EditResult` per affected file.
///
/// Retains direct implementation since it produces multiple `EditResult`s
/// (one per file), which the single-op `execute_as_edit_result` adapter
/// doesn't support.
///
/// **Atomic Apply:** all files are load+hunk preflighted first; on Apply a
/// single backup session covers every path. Any write failure restores the
/// whole batch (no half-applied multi-file patch).
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let patch_files = crate::ops::patch::parse_patch(patch_text).map_err(|e| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("patch parse error: {e}"),
        })
    })?;

    // Phase 1: preflight load + hunk apply for every file (no disk writes).
    // Git rename: load from rename_from, write to path, delete old after apply (#2101).
    let mut staged: Vec<(
        std::path::PathBuf,
        String,
        String,
        String,
        Option<std::path::PathBuf>,
    )> = Vec::new();
    for pf in &patch_files {
        let load_rel = pf.rename_from.as_deref().unwrap_or(pf.path.as_str());
        let load_path = cwd.join(load_rel);
        let write_path = cwd.join(&pf.path);
        // Strict sole-path (#1894). Creation: empty original.
        let original = if pf.is_creation {
            String::new()
        } else {
            crate::files::load_text_strict(&load_path, load_rel)?
        };

        let new_content = crate::ops::patch::apply_hunks(&original, &pf.hunks).map_err(|e| {
            if e.contains("stale context") {
                anyhow::Error::new(crate::exit::AmbiguousError {
                    msg: format!("patch apply error for {}: {e}", pf.path),
                })
            } else {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("patch apply error for {}: {e}", pf.path),
                })
            }
        })?;
        let delete_after = pf.rename_from.as_ref().map(|f| cwd.join(f));
        staged.push((
            write_path,
            pf.path.clone(),
            original,
            new_content,
            delete_after,
        ));
    }

    // Phase 2: one backup session covering write targets + rename sources,
    // then all-or-nothing write; on success remove rename sources.
    let policy = crate::write::WritePolicy::default();
    let (applied, backup_session) = if mode == ApplyMode::Apply {
        for (write_path, _, _, _, delete_after) in &staged {
            super::ensure_contained(guard, write_path)?;
            if let Some(old) = delete_after {
                super::ensure_contained(guard, old)?;
            }
        }
        let mut backup = crate::backup::BackupSession::new(cwd)?;
        for (write_path, _, _, _, delete_after) in &staged {
            if crate::ops::file::path_entry_exists(write_path) {
                backup.save_before_write(write_path)?;
            }
            if let Some(old) = delete_after
                && old != write_path
                && crate::ops::file::path_entry_exists(old)
            {
                backup.save_before_write(old)?;
            }
        }
        let session = backup.finalize()?;
        let write_result = (|| -> anyhow::Result<()> {
            for (write_path, _, _, new_content, _) in &staged {
                crate::write::atomic_write(write_path, new_content, &policy)?;
            }
            for (_, _, _, _, delete_after) in &staged {
                if let Some(old) = delete_after
                    && crate::ops::file::path_entry_exists(old)
                {
                    std::fs::remove_file(old).map_err(|e| {
                        anyhow::anyhow!("patch rename: failed to remove {}: {e}", old.display())
                    })?;
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
    for (_abs, display, original, new_content, _) in staged {
        let mut edit =
            super::build_edit_result(&display, original, new_content, applied, "patch", None);
        // Same session id on every file result so hosts can undo once.
        edit.backup_session = backup_session.clone();
        results.push(edit);
    }
    Ok(results)
}
