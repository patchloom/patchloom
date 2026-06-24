//! File-level operations (create, delete, rename, append, prepend) for the library API.

use std::path::Path;

use anyhow::{Context, bail};

use crate::containment::PathGuard;
use crate::write::{WritePolicy, atomic_create_new, atomic_write};

use super::{
    ApplyMode, EditResult, apply_cross_file_mutation, apply_mutation, build_edit_result,
    write_if_apply,
};

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
    let path_str = path.to_string_lossy().to_string();

    if path.exists() && !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read existing file: {}", path.display()))?
    } else {
        String::new()
    };

    if !force && path.exists() && mode != ApplyMode::Preview {
        bail!("file already exists: {}", path.display());
    }

    let applied = apply_mutation(
        path,
        mode,
        guard,
        |backup| {
            // Ensure parent directories exist for create.
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            backup.save_before_write(path)
        },
        || {
            let policy = WritePolicy::default();
            if force {
                atomic_write(path, content, &policy)
            } else {
                atomic_create_new(path, content, &policy)
            }
        },
    )?;

    Ok(build_edit_result(
        &path_str,
        original,
        content.to_string(),
        applied,
        "create",
        None,
    ))
}

/// Delete a file.
pub fn file_delete(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy().to_string();

    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let applied = apply_mutation(
        path,
        mode,
        guard,
        |backup| backup.save_before_delete(path),
        || {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to delete {}", path.display()))
        },
    )?;

    Ok(build_edit_result(
        &path_str,
        original,
        String::new(),
        applied,
        "delete",
        None,
    ))
}

/// Rename (move) a file.
pub fn file_rename(
    src: &Path,
    dst: &Path,
    force: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let src_str = src.to_string_lossy().to_string();

    if !src.exists() {
        bail!("source not found: {}", src.display());
    }
    if !src.is_file() {
        bail!("source is not a file: {}", src.display());
    }
    if dst.exists() && !force {
        bail!("destination already exists: {}", dst.display());
    }
    if dst.exists() && !dst.is_file() {
        bail!("destination is not a file: {}", dst.display());
    }

    let original = std::fs::read_to_string(src)
        .with_context(|| format!("failed to read {}", src.display()))?;

    let applied = apply_cross_file_mutation(
        src,
        Some(dst),
        mode,
        guard,
        |backup| {
            backup.save_before_write(src)?;
            if dst.exists() && force {
                backup.save_before_write(dst)?;
            }
            // Ensure parent directory of destination exists.
            if let Some(parent) = dst.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            Ok(())
        },
        || {
            std::fs::rename(src, dst)
                .with_context(|| format!("failed to rename {} -> {}", src.display(), dst.display()))
        },
    )?;

    Ok(build_edit_result(
        &src_str,
        original.clone(),
        original,
        applied,
        "rename",
        Some(dst.to_string_lossy().to_string()),
    ))
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
    let path_str = path.to_string_lossy().to_string();

    if !path.exists() {
        bail!("file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let combined = crate::ops::file::append_content(&original, content);

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &combined, mode, &policy, guard)?;

    Ok(build_edit_result(
        &path_str, original, combined, applied, "append", None,
    ))
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
    let path_str = path.to_string_lossy().to_string();

    if !path.exists() {
        bail!("file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let combined = crate::ops::file::prepend_content(&original, content);

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &combined, mode, &policy, guard)?;

    Ok(build_edit_result(
        &path_str, original, combined, applied, "prepend", None,
    ))
}
