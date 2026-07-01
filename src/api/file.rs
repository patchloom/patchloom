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

/// Unified write path for standard file operations.
#[cfg(any(feature = "cli", feature = "files"))]
fn file_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    super::execute_as_edit_result(op, mode, cwd_from_path(path), guard, action, None)
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
            if path.exists() && !path.is_file() {
                bail!("target is not a file: {}", path.display());
            }
            let original = if path.exists() {
                std::fs::read_to_string(path)
                    .with_context(|| format!("failed to read {}", path.display()))?
            } else {
                String::new()
            };
            let force = force.unwrap_or(false);
            if !force && path.exists() && mode != ApplyMode::Preview {
                bail!("file already exists: {}", path.display());
            }
            let policy = crate::write::WritePolicy::default();
            let applied = super::write_if_apply(path, &content, mode, &policy, guard)?;
            Ok(super::build_edit_result(
                &path_str, original, content, applied, action, None,
            ))
        }
        Operation::FileDelete { .. } => {
            let path_str = path.to_string_lossy();
            if !path.exists() {
                bail!("file not found: {}", path.display());
            }
            let original = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let applied = if mode == ApplyMode::Apply {
                super::ensure_contained(guard, path)?;
                std::fs::remove_file(path)
                    .with_context(|| format!("failed to delete {}", path.display()))?;
                true
            } else {
                false
            };
            Ok(super::build_edit_result(
                &path_str,
                original,
                String::new(),
                applied,
                action,
                None,
            ))
        }
        Operation::FileAppend { ref content, .. } | Operation::FilePrepend { ref content, .. } => {
            let is_append = matches!(op, Operation::FileAppend { .. });
            let content = content.clone();
            let path_str = path.to_string_lossy();
            if path.exists() && !path.is_file() {
                bail!("target is not a file: {}", path.display());
            }
            if !path.exists() {
                bail!("file does not exist: {}", path.display());
            }
            let original = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let combined = if is_append {
                crate::ops::file::append_content(&original, &content)
            } else {
                crate::ops::file::prepend_content(&original, &content)
            };
            let policy = crate::write::WritePolicy::default();
            let applied = super::write_if_apply(path, &combined, mode, &policy, guard)?;
            Ok(super::build_edit_result(
                &path_str, original, combined, applied, action, None,
            ))
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
    super::execute_as_edit_result(op, mode, cwd_from_path(src), guard, action, dest_path)
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
            bail!(
                "destination already exists: {} (use force to overwrite)",
                dst.display()
            );
        }
        let original = std::fs::read_to_string(src)
            .with_context(|| format!("failed to read {}", src.display()))?;
        let applied = super::apply_cross_file_mutation(
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
        Ok(super::build_edit_result(
            &src.to_string_lossy(),
            original.clone(),
            original,
            applied,
            action,
            dest_path,
        ))
    } else {
        bail!("unsupported cross-file operation")
    }
}

/// Create a new file with the given content.
///
/// If `force` is false, fails when the file already exists.
pub fn file_create(
    path: &Path,
    content: &str,
    force: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::FileCreate {
        path: path.to_string_lossy().into(),
        content: content.into(),
        force: Some(force),
    };
    file_write(op, path, mode, guard, "create")
}

/// Delete a file.
pub fn file_delete(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::FileDelete {
        path: path.to_string_lossy().into(),
    };
    file_write(op, path, mode, guard, "delete")
}

/// Rename (move) a file.
pub fn file_rename(
    src: &Path,
    dst: &Path,
    force: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
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
    let op = Operation::FilePrepend {
        path: path.to_string_lossy().into(),
        content: content.into(),
    };
    file_write(op, path, mode, guard, "prepend")
}
