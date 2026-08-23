//! File-level operations (create, delete, rename, append, prepend) for the library API.
//!
//! Standard file operations delegate to the tx engine via `execute_as_edit_result`.
//! All operations, including prepend, route through the tx engine.

use std::path::Path;

#[cfg(not(any(feature = "cli", feature = "files")))]
use anyhow::{Context, bail};

use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Derive cwd from a file path (its parent directory).
#[cfg(any(feature = "cli", feature = "files"))]
fn cwd_from_path(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

/// Absolutize for engine handoff; map IO errors to OperationFailed.
#[cfg(any(feature = "cli", feature = "files"))]
fn abs_path(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    super::absolute_for_engine(path).map_err(|e| {
        crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::OperationFailed,
            format!("failed to resolve path {}: {e}", path.display()),
        )
        .into()
    })
}

/// Unified write path for standard file operations.
#[cfg(any(feature = "cli", feature = "files"))]
fn file_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    let display = path.to_string_lossy();
    super::execute_as_edit_result_with_path(
        op,
        mode,
        cwd_from_path(path),
        guard,
        action,
        None,
        Some(display.as_ref()),
    )
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn file_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    // Fallback for no-cli/files builds: delegate to the ops layer directly.
    match op {
        Operation::FileCreate { content, force, .. } => {
            let path_str = path.to_string_lossy();
            use crate::ops::file::{PathEntryKind, classify_path_entry, path_entry_exists};
            // Match engine: entry presence (dangling is present) + real dirs refuse.
            match classify_path_entry(path) {
                PathEntryKind::RealDirectory => {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("target is not a file: {}", path.display()),
                    }));
                }
                PathEntryKind::Missing | PathEntryKind::RegularFile | PathEntryKind::Special => {}
            }
            crate::ops::file::ensure_parent_components_are_directories(path)?;
            let force = force.unwrap_or(false);
            // Match engine path: refuse existing without force in all modes
            // (Preview/Check/Apply). Preview must not soft-succeed (#MPI dual-path).
            if !force && path_entry_exists(path) {
                return Err(anyhow::Error::new(crate::exit::AlreadyExistsError {
                    msg: format!(
                        "file already exists: {} (use force to overwrite)",
                        path.display()
                    ),
                }));
            }
            // Force: soft-load prior (binary/encoding/unreadable → empty) (#1962).
            // Special nodes (dangling) → empty original; regular text strict load.
            let original = match classify_path_entry(path) {
                PathEntryKind::RegularFile => {
                    match crate::files::load_text_strict(path, &path_str) {
                        Ok(s) => s,
                        Err(e) if force && crate::exit::is_load_text_strict_fail(&e) => {
                            String::new()
                        }
                        Err(e) => return Err(e),
                    }
                }
                PathEntryKind::Missing | PathEntryKind::Special | PathEntryKind::RealDirectory => {
                    String::new()
                }
            };
            let policy = crate::write::WritePolicy::default();
            let (applied, backup_session) =
                super::write_if_apply(path, &content, mode, &policy, guard)?;
            {
                let mut __e =
                    super::build_edit_result(&path_str, original, content, applied, action, None);
                __e.backup_session = backup_session;
                Ok(__e)
            }
        }
        Operation::FileDelete { if_exists, .. } => {
            let path_str = path.to_string_lossy();
            // path_entry_exists includes dangling symlinks (#2087).
            if !crate::ops::file::path_entry_exists(path) {
                if if_exists {
                    return Ok(super::build_edit_result(
                        &path_str,
                        String::new(),
                        String::new(),
                        false,
                        action,
                        None,
                    ));
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {}", path.display()),
                )
                .into());
            }
            // Regular files, symlinks (unlink only), FIFO/socket/device ok;
            // real directories refuse (#2087).
            crate::ops::file::ensure_unlinkable_not_directory(path, path_str.as_ref())?;
            // Delete may remove non-UTF-8 / special nodes; soft snapshot only
            // for regular text files.
            let original = if crate::ops::file::is_regular_file_for_backup(path) {
                crate::files::load_text_strict(path, &path_str).unwrap_or_default()
            } else {
                String::new()
            };
            // Entry containment: do not follow symlink targets (#2115).
            // Preview/Check: report would-delete without unlinking (#2087 DryRun).
            let (applied, backup_session) = if mode == ApplyMode::Apply {
                super::ensure_contained_entry(guard, path)?;
                super::apply_mutation(
                    path,
                    mode,
                    None, // already checked with entry semantics
                    |backup| backup.save_before_delete(path),
                    || {
                        std::fs::remove_file(path)
                            .with_context(|| format!("failed to delete {}", path.display()))
                    },
                )?
            } else {
                super::ensure_contained_entry(guard, path)?;
                (false, None)
            };
            {
                let mut __e = super::build_edit_result(
                    &path_str,
                    original,
                    String::new(),
                    applied,
                    action,
                    None,
                );
                __e.backup_session = backup_session;
                Ok(__e)
            }
        }
        Operation::FileAppend { ref content, .. } | Operation::FilePrepend { ref content, .. } => {
            let is_append = matches!(op, Operation::FileAppend { .. });
            let content = content.clone();
            let path_str = path.to_string_lossy();
            // Match CLI/tx: entry presence (dangling symlink is present) and
            // require a regular file for content inject (#2087 dual-path).
            use crate::ops::file::{PathEntryKind, classify_path_entry, path_entry_exists};
            if !path_entry_exists(path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file does not exist: {}", path.display()),
                )
                .into());
            }
            if classify_path_entry(path) != PathEntryKind::RegularFile {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {}", path.display()),
                }));
            }
            let original = crate::files::load_text_strict(path, &path_str)?;
            let combined = if is_append {
                crate::ops::file::append_content(&original, &content)
            } else {
                crate::ops::file::prepend_content(&original, &content)
            };
            let policy = crate::write::WritePolicy::default();
            let (applied, backup_session) =
                super::write_if_apply(path, &combined, mode, &policy, guard)?;
            {
                let mut __e =
                    super::build_edit_result(&path_str, original, combined, applied, action, None);
                __e.backup_session = backup_session;
                Ok(__e)
            }
        }
        _ => bail!("unsupported file operation"),
    }
}

/// Unified cross-file write path (rename).
#[cfg(any(feature = "cli", feature = "files"))]
fn file_write_cross(
    op: Operation,
    src: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
    dest_path: Option<String>,
) -> anyhow::Result<EditResult> {
    let display = src.to_string_lossy();
    super::execute_as_edit_result_with_path(
        op,
        mode,
        cwd_from_path(src),
        guard,
        action,
        dest_path,
        Some(display.as_ref()),
    )
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn file_write_cross(
    _op: Operation,
    src: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
    dest_path: Option<String>,
) -> anyhow::Result<EditResult> {
    // Fallback: rename directly.
    if let Operation::FileRename { to, force, .. } = _op {
        let dst = Path::new(&to);
        if !force && dst.exists() {
            return Err(anyhow::Error::new(crate::exit::AlreadyExistsError {
                msg: format!(
                    "destination already exists: {} (use force to overwrite)",
                    dst.display()
                ),
            }));
        }
        // Soft text load for EditResult body only; binary / unreadable still
        // renames the inode (this no-files fallback path).
        let original = crate::files::try_read_text_file(src).unwrap_or_default();
        let (applied, backup_session) = super::apply_cross_file_mutation(
            src,
            Some(dst),
            mode,
            guard,
            |backup| {
                backup.save_before_write(src)?;
                if dst.exists() && force {
                    backup.save_before_write(dst)?;
                }
                Ok(())
            },
            || {
                std::fs::rename(src, dst).with_context(|| {
                    format!("failed to rename {} -> {}", src.display(), dst.display())
                })
            },
        )?;
        {
            let mut __e = super::build_edit_result(
                &src.to_string_lossy(),
                original.clone(),
                original,
                applied,
                action,
                dest_path,
            );
            __e.backup_session = backup_session;
            Ok(__e)
        }
    } else {
        bail!("unsupported cross-file operation")
    }
}

/// Create a new file with the given content.
///
/// If `force` is false, fails when the file already exists
/// ([`EditErrorKind::AlreadyExists`]).
///
/// When `force` is true and the path already holds binary, invalid UTF-8, or
/// otherwise unreadable prior content, the create **overwrites** with empty
/// original for backup/diff (no host-side remove+recreate). PathGuard still
/// applies. Apply writes use the normal hardlink-preserving commit path (#1962).
pub fn file_create(
    path: &Path,
    content: &str,
    force: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let path_owned = abs_path(path)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let path = path_owned.as_path();
    let op = Operation::FileCreate {
        path: path.to_string_lossy().into(),
        content: content.into(),
        force: Some(force),
    };
    file_write(op, path, mode, guard, "create")
}

/// Delete a file, symlink, FIFO, socket, or device node under PathGuard (#2087).
///
/// **Directories are refused** (use a host-side recursive delete if needed).
/// **Symlinks** are unlinked without following the target. Regular-file content
/// is backed up for undo; special nodes get an empty backup marker (restore
/// recreates an empty regular file, not the original node type).
///
/// **Guard:** uses entry containment ([`PathGuard::check_path_entry`]): the
/// directory entry must sit under the workspace; the symlink **target** is not
/// used for allow/deny (#2115). So `workspace/link → /etc/passwd` can be
/// deleted with a workspace guard without treating the op as touching
/// `/etc/passwd`.
///
/// DryRun / Preview / Check report would-delete without unlinking.
pub fn file_delete(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let path_owned = abs_path(path)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let path = path_owned.as_path();
    let op = Operation::FileDelete {
        path: path.to_string_lossy().into(),
        if_exists: false,
    };
    file_write(op, path, mode, guard, "delete")
}

/// Rename (move) a file, symlink, FIFO, socket, or device node (#2091).
///
/// **Directories are refused.** Symlinks (including dangling and symlink-to-dir)
/// are moved as directory entries without following the target. Soft-loading
/// symlink text and rewriting would mutate the target via `atomic_write`;
/// special nodes use an empty path-only snapshot so write policies never
/// rewrite the link target. Regular-file content is soft-loaded for
/// preview/diff; binary / invalid UTF-8 still path-rename (#2031).
///
/// **Guard:** both `src` and `dst` use entry containment
/// ([`PathGuard::check_path_entry`]) so a link whose target is outside the
/// workspace can still be renamed when the **entry paths** stay inside
/// (#2115).
pub fn file_rename(
    src: &Path,
    dst: &Path,
    force: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let src_owned = abs_path(src)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let dst_owned = abs_path(dst)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let src = src_owned.as_path();
    #[cfg(any(feature = "cli", feature = "files"))]
    let dst = dst_owned.as_path();
    let op = Operation::FileRename {
        from: src.to_string_lossy().into(),
        to: dst.to_string_lossy().into(),
        force,
    };
    let dest_str = Some(dst.to_string_lossy().to_string());
    file_write_cross(op, src, mode, guard, "rename", dest_str)
}

/// Append content to an existing file.
///
/// The file must exist (use file_create for new files). A trailing newline
/// is ensured between existing content and the appended content when needed.
pub fn file_append(
    path: &Path,
    content: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let path_owned = abs_path(path)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let path = path_owned.as_path();
    let op = Operation::FileAppend {
        path: path.to_string_lossy().into(),
        content: content.into(),
    };
    file_write(op, path, mode, guard, "append")
}

/// Prepend content to an existing file.
///
/// The file must exist. Content is inserted at the beginning.
pub fn file_prepend(
    path: &Path,
    content: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let path_owned = abs_path(path)?;
    #[cfg(any(feature = "cli", feature = "files"))]
    let path = path_owned.as_path();
    let op = Operation::FilePrepend {
        path: path.to_string_lossy().into(),
        content: content.into(),
    };
    file_write(op, path, mode, guard, "prepend")
}
