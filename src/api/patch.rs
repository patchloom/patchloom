//! Patch (unified diff) apply operations for the library API.
//!
//! Single-file `apply_patch` delegates to the tx engine via `execute_as_edit_result`.
//! Multi-file `apply_patch_file` retains a direct implementation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Apply a unified diff patch to a file.
///
/// Also detects Codex Begin Patch and SEARCH/REPLACE / DiffFenced.
/// SEARCH/REPLACE is unique-only here (`replace_all` is CLI / MCP / plan).
/// Dest paths come from the document; `path` only supplies the workspace
/// parent (same as a relative dest under that parent).
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(
    path: &Path,
    patch_text: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    if crate::ops::begin_patch::looks_like_begin_patch(patch_text) {
        if crate::ops::search_replace::has_search_replace_marker(patch_text) {
            return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                msg: "mixed Begin Patch and SEARCH/REPLACE grammar is not supported".into(),
            }));
        }
        let abs = super::absolute_for_engine(path).map_err(|e| {
            crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::OperationFailed,
                format!("failed to resolve path {}: {e}", path.display()),
            )
        })?;
        let cwd = abs.parent().unwrap_or_else(|| Path::new("."));
        let results = super::apply_begin_patch(patch_text, cwd, Some(&abs), mode, guard)?;
        return results.into_iter().next().ok_or_else(|| {
            anyhow::Error::new(crate::exit::ParseErrorError {
                msg: "Begin Patch contained no file operations".into(),
            })
        });
    }
    if crate::ops::search_replace::looks_like_search_replace(patch_text) {
        let abs = super::absolute_for_engine(path).map_err(|e| {
            crate::fallback::EditError::new(
                crate::fallback::EditErrorKind::OperationFailed,
                format!("failed to resolve path {}: {e}", path.display()),
            )
        })?;
        let cwd = abs.parent().unwrap_or_else(|| Path::new("."));
        let results = super::apply_search_replace_document(
            patch_text,
            cwd,
            &super::ApplySearchReplaceOptions {
                file_hint: Some(abs.clone()),
                ..super::ApplySearchReplaceOptions::default()
            },
            mode,
            guard,
        )?;
        return results.into_iter().next().ok_or_else(|| {
            anyhow::Error::new(crate::exit::ParseErrorError {
                msg: "SEARCH/REPLACE contained no blocks".into(),
            })
        });
    }
    let op = Operation::PatchApply {
        diff: patch_text.into(),
        on_stale: Default::default(),
        allow_conflicts: false,
        replace_all: false,
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
        if let Some(reason) = pf.unsupported.as_deref() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: crate::ops::patch::unsupported_git_meta_msg(&pf.path, reason),
            }));
        }
        let load_rel = pf
            .copy_from
            .as_deref()
            .or(pf.rename_from.as_deref())
            .unwrap_or(pf.path.as_str());
        let load_path = cwd.join(load_rel);
        let write_path = cwd.join(&pf.path);
        if let Some(msg) = pf.dest_clobber_msg(crate::ops::file::path_entry_exists(&write_path)) {
            return Err(anyhow::Error::new(crate::exit::AlreadyExistsError { msg }));
        }
        if !pf.is_deletion {
            crate::ops::file::ensure_parent_components_are_directories(&write_path)?;
        }
        if pf.copy_from.is_some() && !crate::ops::file::path_entry_exists(&load_path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {load_rel}"),
            )
            .into());
        }
        // Strict sole-path (#1894): binary / invalid UTF-8 → Binary / InvalidEncoding.
        // 100% copy loads source bytes as text when possible; dest is still written.
        let original = if pf.copy_from.is_some() {
            crate::files::load_text_strict(&load_path, load_rel)?
        } else if pf.is_creation {
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
                    // Hard-fail remove (no silent dual-path leave-behind).
                    std::fs::remove_file(&old).map_err(|e| {
                        anyhow::anyhow!(
                            "patch rename: failed to remove source {}: {e}",
                            old.display()
                        )
                    })?;
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
/// whole batch (no half-applied multi-file patch). Empty-create dests report
/// `changed: true` even when original and new content are both empty.
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    if crate::ops::begin_patch::looks_like_begin_patch(patch_text) {
        if crate::ops::search_replace::has_search_replace_marker(patch_text) {
            return Err(anyhow::Error::new(crate::exit::ParseErrorError {
                msg: "mixed Begin Patch and SEARCH/REPLACE grammar is not supported".into(),
            }));
        }
        return super::apply_begin_patch(patch_text, cwd, None, mode, guard);
    }
    if crate::ops::search_replace::looks_like_search_replace(patch_text) {
        return super::apply_search_replace_document(
            patch_text,
            cwd,
            &super::ApplySearchReplaceOptions::default(),
            mode,
            guard,
        );
    }
    let patch_files = crate::ops::patch::parse_patch(patch_text).map_err(|e| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("patch parse error: {e}"),
        })
    })?;

    // Phase 1: preflight load + hunk apply for every file (no disk writes).
    // Kinds: content write, deletion (unlink), path rename (fs::rename then optional rewrite).
    #[derive(Clone)]
    enum StageOp {
        Write {
            write_path: std::path::PathBuf,
            display: String,
            original: String,
            new_content: String,
            is_creation: bool,
        },
        Delete {
            path: std::path::PathBuf,
            display: String,
            original: String,
        },
        Rename {
            from: std::path::PathBuf,
            to: std::path::PathBuf,
            from_display: String,
            to_display: String,
            original: String,
            new_content: String,
        },
        CopyFile {
            from: std::path::PathBuf,
            to: std::path::PathBuf,
            display: String,
            original: String,
            new_content: String,
        },
    }

    let mut staged: Vec<StageOp> = Vec::new();
    let mut created: HashSet<PathBuf> = HashSet::new();
    let mut deleted: HashSet<PathBuf> = HashSet::new();
    for pf in &patch_files {
        if let Some(reason) = pf.unsupported.as_deref() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: crate::ops::patch::unsupported_git_meta_msg(&pf.path, reason),
            }));
        }
        let load_rel = pf
            .copy_from
            .as_deref()
            .or(pf.rename_from.as_deref())
            .unwrap_or(pf.path.as_str());
        let load_path = cwd.join(load_rel);
        let write_path = cwd.join(&pf.path);
        if let Some(msg) = pf.dest_clobber_msg(crate::ops::patch::staged_path_exists(
            &write_path,
            &created,
            &deleted,
        )) {
            return Err(anyhow::Error::new(crate::exit::AlreadyExistsError { msg }));
        }
        if !pf.is_deletion {
            crate::ops::file::ensure_parent_components_are_directories(&write_path)?;
        }

        if let Some(from) = pf.copy_from.as_ref() {
            let from_path = cwd.join(from);
            if !crate::ops::patch::staged_path_exists(&from_path, &created, &deleted) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {from}"),
                )
                .into());
            }
            let original = match crate::files::load_text_strict(&from_path, from) {
                Ok(s) => s,
                Err(e) if crate::exit::is_binary(&e) || crate::exit::is_invalid_encoding(&e) => {
                    String::new()
                }
                Err(e) => return Err(e),
            };
            staged.push(StageOp::CopyFile {
                from: from_path,
                to: write_path,
                display: pf.path.clone(),
                original: original.clone(),
                new_content: original,
            });
            crate::ops::patch::record_staged_patch_dest(cwd, pf, &mut created, &mut deleted);
            continue;
        }

        if pf.is_deletion {
            // Real unlink, not empty rewrite (tx/CLI parity).
            let original = if crate::ops::file::path_entry_exists(&load_path) {
                crate::files::load_text_strict(&load_path, load_rel).unwrap_or_default()
            } else {
                String::new()
            };
            staged.push(StageOp::Delete {
                path: load_path,
                display: pf.path.clone(),
                original,
            });
            crate::ops::patch::record_staged_patch_dest(cwd, pf, &mut created, &mut deleted);
            continue;
        }

        // Pure path rename of non-text: soft empty snapshot (file_rename #2031).
        let pure_rename = pf.rename_from.is_some() && pf.hunks.is_empty();
        let original = if pf.is_creation {
            String::new()
        } else if pure_rename {
            match crate::files::load_text_strict(&load_path, load_rel) {
                Ok(s) => s,
                Err(e) if crate::exit::is_binary(&e) || crate::exit::is_invalid_encoding(&e) => {
                    String::new()
                }
                Err(e) => return Err(e),
            }
        } else {
            crate::files::load_text_strict(&load_path, load_rel)?
        };

        let new_content = if pure_rename {
            original.clone()
        } else {
            crate::ops::patch::apply_hunks(&original, &pf.hunks).map_err(|e| {
                if e.contains("stale context") {
                    anyhow::Error::new(crate::exit::AmbiguousError {
                        msg: format!("patch apply error for {}: {e}", pf.path),
                    })
                } else {
                    anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("patch apply error for {}: {e}", pf.path),
                    })
                }
            })?
        };

        if let Some(from) = pf.rename_from.as_ref() {
            let from_path = cwd.join(from);
            staged.push(StageOp::Rename {
                from: from_path,
                to: write_path,
                from_display: from.clone(),
                to_display: pf.path.clone(),
                original,
                new_content,
            });
        } else {
            staged.push(StageOp::Write {
                write_path,
                display: pf.path.clone(),
                original,
                new_content,
                is_creation: pf.is_creation,
            });
        }
        crate::ops::patch::record_staged_patch_dest(cwd, pf, &mut created, &mut deleted);
    }

    // Phase 2: one backup session, then all-or-nothing mutate.
    let policy = crate::write::WritePolicy::default();
    let (applied, backup_session) = if mode == ApplyMode::Apply {
        for op in &staged {
            match op {
                StageOp::Write { write_path, .. } => super::ensure_contained(guard, write_path)?,
                // Path-only delete/rename: entry containment (#2115).
                StageOp::Delete { path, .. } => super::ensure_contained_entry(guard, path)?,
                StageOp::Rename { from, to, .. } => {
                    super::ensure_contained_entry(guard, from)?;
                    super::ensure_contained_entry(guard, to)?;
                }
                StageOp::CopyFile { from, to, .. } => {
                    super::ensure_contained(guard, from)?;
                    super::ensure_contained(guard, to)?;
                }
            }
        }
        let mut backup = crate::backup::BackupSession::new(cwd)?;
        // Always record every path (including creates and rename dests as
        // FileAction::Created when missing). Skipping non-existent paths left
        // orphans after mid-batch restore (fixloop 2026-08-02; same class as
        // write_if_apply_many).
        for op in &staged {
            match op {
                StageOp::Write { write_path, .. } => {
                    backup.save_before_write(write_path)?;
                }
                StageOp::Delete { path, .. } => {
                    if crate::ops::file::path_entry_exists(path) {
                        backup.save_before_delete(path)?;
                    }
                }
                StageOp::Rename { from, to, .. } => {
                    if crate::ops::file::path_entry_exists(from) {
                        backup.save_before_delete(from)?;
                    }
                    if to != from {
                        backup.save_before_write(to)?;
                    }
                }
                StageOp::CopyFile { to, .. } => {
                    backup.save_before_write(to)?;
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
                        crate::ops::file::ensure_parent_components_are_directories(write_path)?;
                        if let Some(parent) = write_path.parent()
                            && !parent.as_os_str().is_empty()
                            && !parent.exists()
                        {
                            std::fs::create_dir_all(parent)?;
                        }
                        crate::write::atomic_write(write_path, new_content, &policy)?;
                    }
                    StageOp::Delete { path, .. } => {
                        if crate::ops::file::path_entry_exists(path) {
                            std::fs::remove_file(path).map_err(|e| {
                                anyhow::anyhow!(
                                    "patch delete: failed to remove {}: {e}",
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
                        // fs::rename preserves case-only renames and binary bytes
                        // (write-dest+delete-src would delete the only inode).
                        crate::ops::file::ensure_parent_components_are_directories(to)?;
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
                    StageOp::CopyFile { from, to, .. } => {
                        crate::ops::file::ensure_parent_components_are_directories(to)?;
                        if let Some(parent) = to.parent()
                            && !parent.as_os_str().is_empty()
                            && !parent.exists()
                        {
                            std::fs::create_dir_all(parent)?;
                        }
                        // Byte copy; source stays. (#2171)
                        std::fs::copy(from, to).map_err(|e| {
                            anyhow::anyhow!(
                                "patch copy: failed to copy {} -> {}: {e}",
                                from.display(),
                                to.display()
                            )
                        })?;
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
            StageOp::CopyFile {
                display,
                original,
                new_content,
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
                e.changed = true;
                e
            }
        };
        edit.backup_session = backup_session.clone();
        results.push(edit);
    }
    Ok(results)
}
