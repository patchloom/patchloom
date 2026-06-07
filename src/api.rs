//! Public library API for embedding patchloom in Rust applications.
//!
//! This module provides a clean, CLI-independent interface to patchloom's
//! editing operations. Functions accept `&Path`/`&str` parameters and return
//! `Result<EditResult>`, with no dependency on `clap` or process arguments.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use patchloom::api::{self, ApplyMode, EditResult};
//! use std::path::Path;
//!
//! // Replace text in a file (preview only)
//! let result = api::replace_text(
//!     Path::new("src/config.rs"),
//!     "old_value",
//!     "new_value",
//!     &api::ReplaceOptions::default(),
//!     ApplyMode::Preview,
//! ).unwrap();
//! println!("diff:\n{}", result.diff);
//! ```
//!
//! # Apply modes
//!
//! All write operations accept an [`ApplyMode`]:
//! - [`ApplyMode::Preview`] — compute the result without writing to disk.
//! - [`ApplyMode::Apply`] — write changes to disk with backup.
//! - [`ApplyMode::Check`] — report whether changes would occur (for CI).
//!
//! # Thread safety
//!
//! All types in this module are `Send + Sync`. Functions are safe to call
//! concurrently from multiple threads when operating on **different files**.
//! Concurrent edits to the **same file** are the caller's responsibility
//! to serialize (e.g., via a `Mutex` per file path).
//!
//! Backup sessions use unique directory names (nanosecond timestamp +
//! monotonic counter), so concurrent `ApplyMode::Apply` calls never collide
//! on backup directories.

use std::path::Path;

use anyhow::{Context, bail};

use crate::backup::BackupSession;
use crate::diff::{DiffResult, format_diff_result, unified_diff};
use crate::ops;
use crate::selector;
use crate::write::{WritePolicy, atomic_create_new, atomic_write};

/// The result of an editing operation.
#[derive(Debug, Clone)]
pub struct EditResult {
    /// Path to the affected file (as provided by the caller).
    pub path: String,
    /// The original file content before the edit.
    pub original_content: String,
    /// The new content after the edit.
    pub new_content: String,
    /// A unified diff between original and new content.
    pub diff: String,
    /// Whether the file was actually written to disk.
    pub applied: bool,
    /// Whether the content changed.
    pub changed: bool,
}

/// Controls whether an operation writes to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    /// Compute the result without writing. Returns the diff and new content.
    Preview,
    /// Write changes to disk with backup support.
    Apply,
    /// Report whether changes would occur, without writing.
    Check,
}

/// Options for text replacement operations.
#[derive(Debug, Clone, Default)]
pub struct ReplaceOptions {
    /// Use regex mode for the `from` pattern.
    pub regex: bool,
    /// Replace only the Nth match (1-based). `None` means replace all.
    pub nth: Option<usize>,
    /// Case-insensitive matching.
    pub case_insensitive: bool,
    /// Enable multiline matching (dot matches newlines in regex mode).
    pub multiline: bool,
    /// Text to insert before each match instead of replacing.
    /// Mutually exclusive with `to` (the replacement text) and `insert_after`.
    pub insert_before: Option<String>,
    /// Text to insert after each match instead of replacing.
    /// Mutually exclusive with `to` (the replacement text) and `insert_before`.
    pub insert_after: Option<String>,
}

/// Write policy options for controlling file write transformations.
#[derive(Debug, Clone, Default)]
pub struct WritePolicyOptions {
    /// Ensure non-empty files end with a newline.
    pub ensure_final_newline: bool,
    /// Normalize line endings. `None` means keep existing.
    pub normalize_eol: Option<EolNormalization>,
    /// Remove trailing whitespace from each line.
    pub trim_trailing_whitespace: bool,
}

/// Line ending normalization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolNormalization {
    /// Normalize to LF (`\n`).
    Lf,
    /// Normalize to CRLF (`\r\n`).
    Crlf,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert user-facing `WritePolicyOptions` to the internal `WritePolicy`.
pub fn make_write_policy(opts: &WritePolicyOptions) -> WritePolicy {
    use crate::cli::global::EolMode;
    WritePolicy {
        ensure_final_newline: opts.ensure_final_newline,
        normalize_eol: match opts.normalize_eol {
            None => EolMode::Keep,
            Some(EolNormalization::Lf) => EolMode::Lf,
            Some(EolNormalization::Crlf) => EolMode::Crlf,
        },
        trim_trailing_whitespace: opts.trim_trailing_whitespace,
    }
}

fn make_diff(path: &str, old: &str, new: &str) -> String {
    let file_diff = unified_diff(path, old, new);
    let changed = file_diff.has_changes;
    if !changed {
        return String::new();
    }
    let result = DiffResult {
        diffs: vec![file_diff],
        total_files_changed: 1,
    };
    format_diff_result(&result)
}

fn write_if_apply(
    path: &Path,
    new_content: &str,
    mode: ApplyMode,
    policy: &WritePolicy,
) -> anyhow::Result<bool> {
    if mode != ApplyMode::Apply {
        return Ok(false);
    }
    // Use the project root (parent of the file) as backup root.
    // For library users, backup is best-effort.
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    backup.save_before_write(path)?;
    atomic_write(path, new_content, policy)?;
    backup.finalize()?;
    Ok(true)
}

fn build_edit_result(
    path_str: &str,
    original: String,
    new_content: String,
    applied: bool,
) -> EditResult {
    let diff = make_diff(path_str, &original, &new_content);
    let changed = original != new_content;
    EditResult {
        path: path_str.to_string(),
        original_content: original,
        new_content,
        diff,
        applied,
        changed,
    }
}

// ---------------------------------------------------------------------------
// Document operations (JSON/YAML/TOML)
// ---------------------------------------------------------------------------

/// Set a value at a selector path in a JSON, YAML, or TOML file.
///
/// The file format is detected from the extension. The selector uses
/// patchloom's selector syntax (e.g., `"database.host"`, `"items[0].name"`).
pub fn doc_set(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut doc = ops::doc::parse_doc(&original, &format)?;

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::set_at_path(&mut doc, &segments, value)?;

    let new_content = ops::doc::serialize_value_preserving(
        &original,
        &{ ops::doc::parse_doc(&original, &format)? },
        &doc,
        &format,
    )?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Delete a value at a selector path in a JSON, YAML, or TOML file.
pub fn doc_delete(path: &Path, selector: &str, mode: ApplyMode) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let old_value = ops::doc::parse_doc(&original, &format)?;
    let mut doc = old_value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::delete_at_selector(&mut doc, &segments)?;

    let new_content = ops::doc::serialize_value_preserving(&original, &old_value, &doc, &format)?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Deep-merge a value into the root of a JSON, YAML, or TOML file.
pub fn doc_merge(
    path: &Path,
    value: serde_json::Value,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let old_value = ops::doc::parse_doc(&original, &format)?;
    let mut doc = old_value.clone();

    ops::doc::deep_merge(&mut doc, &value);

    let new_content = ops::doc::serialize_value_preserving(&original, &old_value, &doc, &format)?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Get a value at a selector path from a JSON, YAML, or TOML file.
///
/// This is a read-only operation; the `mode` parameter is ignored.
pub fn doc_get(path: &Path, selector: &str) -> anyhow::Result<serde_json::Value> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc = ops::doc::parse_doc(&content, &format)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc, &segments);
    match result.len() {
        0 => bail!("selector '{}' matched nothing", selector),
        1 => Ok(result.into_iter().next().unwrap().clone()),
        _ => Ok(serde_json::Value::Array(
            result.into_iter().cloned().collect(),
        )),
    }
}

/// Check whether a selector path exists in a JSON, YAML, or TOML file.
pub fn doc_has(path: &Path, selector: &str) -> anyhow::Result<bool> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc = ops::doc::parse_doc(&content, &format)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc, &segments);
    Ok(!result.is_empty())
}

/// Append a value to an array at a selector path.
pub fn doc_append(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let old_value = ops::doc::parse_doc(&original, &format)?;
    let mut doc = old_value.clone();

    let segments = selector::parse_anyhow(selector)?;
    let target = ops::doc::navigate_mut(&mut doc, &segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;
    arr.push(value);

    let new_content = ops::doc::serialize_value_preserving(&original, &old_value, &doc, &format)?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Move a value from one selector path to another within the same file.
pub fn doc_move(
    path: &Path,
    from_selector: &str,
    to_selector: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let old_value = ops::doc::parse_doc(&original, &format)?;
    let mut doc = old_value.clone();

    let from_segments = selector::parse_anyhow(from_selector)?;
    let to_segments = selector::parse_anyhow(to_selector)?;
    ops::doc::move_at_path(&mut doc, &from_segments, &to_segments)?;

    let new_content = ops::doc::serialize_value_preserving(&original, &old_value, &doc, &format)?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

// ---------------------------------------------------------------------------
// Text replacement
// ---------------------------------------------------------------------------

/// Replace text in a file using literal or regex matching.
///
/// When `opts.insert_before` or `opts.insert_after` is set, the matched text
/// is preserved and the insertion is added adjacent to it.
pub fn replace_text(
    path: &Path,
    from: &str,
    to: &str,
    opts: &ReplaceOptions,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if from.is_empty() && !opts.regex {
        bail!("empty search pattern");
    }

    let compiled_re = ops::replace::compile_replace_regex(
        from,
        opts.regex,
        opts.case_insensitive,
        opts.multiline,
    )?;

    let replacement = ops::replace::replacement_text(
        from,
        &if opts.insert_before.is_none() && opts.insert_after.is_none() {
            Some(to.to_string())
        } else {
            None
        },
        &opts.insert_before,
        &opts.insert_after,
        compiled_re.is_some(),
    );

    let (new_content, _count) = ops::replace::replace_content(
        &original,
        from,
        &replacement,
        compiled_re.as_ref(),
        opts.nth,
    );
    let new_content = new_content.into_owned();

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

// ---------------------------------------------------------------------------
// Markdown operations
// ---------------------------------------------------------------------------

/// Replace the body of a markdown section identified by heading.
pub fn md_replace_section(
    path: &Path,
    heading: &str,
    content: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::replace_section_in(&original, heading, content)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Insert or update a bullet point under a markdown heading.
///
/// If the bullet already exists (exact match), the file is unchanged.
pub fn md_upsert_bullet(
    path: &Path,
    heading: &str,
    bullet: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::upsert_bullet_in(&original, heading, bullet)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Append a row to a markdown table under a heading.
pub fn md_table_append(
    path: &Path,
    heading: &str,
    row: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::table_append_for_tx(&original, heading, row)
        .ok_or_else(|| anyhow::anyhow!("no table found under heading '{}'", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Insert content after a markdown heading.
pub fn md_insert_after_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_after_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Insert content before a markdown heading.
pub fn md_insert_before_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_before_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

/// Create a new file with the given content.
///
/// If `force` is false, fails when the file already exists.
pub fn file_create(
    path: &Path,
    content: &str,
    force: bool,
    mode: ApplyMode,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy().to_string();

    if path.exists() && !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };

    if !force && path.exists() && mode != ApplyMode::Preview {
        bail!("file already exists: {}", path.display());
    }

    let applied = if mode == ApplyMode::Apply {
        // Ensure parent directories exist.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let policy = WritePolicy::default();
        let cwd = path.parent().unwrap_or_else(|| Path::new("."));
        let mut backup = BackupSession::new(cwd)?;
        backup.save_before_write(path)?;
        if force {
            atomic_write(path, content, &policy)?;
        } else {
            atomic_create_new(path, content, &policy)?;
        }
        backup.finalize()?;
        true
    } else {
        false
    };

    Ok(build_edit_result(
        &path_str,
        original,
        content.to_string(),
        applied,
    ))
}

/// Delete a file.
pub fn file_delete(path: &Path, mode: ApplyMode) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy().to_string();

    if !path.exists() {
        bail!("file not found: {}", path.display());
    }
    if !path.is_file() {
        bail!("target is not a file: {}", path.display());
    }

    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let applied = if mode == ApplyMode::Apply {
        let cwd = path.parent().unwrap_or_else(|| Path::new("."));
        let mut backup = BackupSession::new(cwd)?;
        backup.save_before_delete(path)?;
        std::fs::remove_file(path)
            .with_context(|| format!("failed to delete {}", path.display()))?;
        backup.finalize()?;
        true
    } else {
        false
    };

    Ok(build_edit_result(
        &path_str,
        original,
        String::new(),
        applied,
    ))
}

/// Rename (move) a file.
pub fn file_rename(
    src: &Path,
    dst: &Path,
    force: bool,
    mode: ApplyMode,
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

    let applied = if mode == ApplyMode::Apply {
        let cwd = src.parent().unwrap_or_else(|| Path::new("."));
        let mut backup = BackupSession::new(cwd)?;
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
        std::fs::rename(src, dst)
            .with_context(|| format!("failed to rename {} -> {}", src.display(), dst.display()))?;
        backup.finalize()?;
        true
    } else {
        false
    };

    Ok(build_edit_result(
        &src_str,
        original.clone(),
        original,
        applied,
    ))
}

// ---------------------------------------------------------------------------
// Patch operations
// ---------------------------------------------------------------------------

/// Apply a unified diff patch to a file.
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(path: &Path, patch_text: &str, mode: ApplyMode) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let patch_files = ops::patch::parse_patch(patch_text)
        .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

    // Find the hunks for this file. The patch may contain hunks for multiple
    // files; we only apply the ones matching the given path.
    let path_str_ref: &str = &path_str;
    let hunks: Vec<_> = patch_files
        .iter()
        .filter(|pf| pf.path == path_str_ref || path_str_ref.ends_with(&pf.path))
        .flat_map(|pf| pf.hunks.iter())
        .collect();

    if hunks.is_empty() {
        bail!("no hunks found for path '{}'", path_str);
    }

    let owned_hunks: Vec<_> = hunks.into_iter().cloned().collect();
    let new_content = ops::patch::apply_hunks(&original, &owned_hunks)
        .map_err(|e| anyhow::anyhow!("patch apply error: {e}"))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy)?;
    Ok(build_edit_result(&path_str, original, new_content, applied))
}

/// Apply a multi-file patch. Returns one `EditResult` per affected file.
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
) -> anyhow::Result<Vec<EditResult>> {
    let patch_files = ops::patch::parse_patch(patch_text)
        .map_err(|e| anyhow::anyhow!("patch parse error: {e}"))?;

    let mut results = Vec::new();
    for pf in &patch_files {
        let file_path = cwd.join(&pf.path);
        let original = std::fs::read_to_string(&file_path)
            .with_context(|| format!("failed to read {}", file_path.display()))?;

        let new_content = ops::patch::apply_hunks(&original, &pf.hunks)
            .map_err(|e| anyhow::anyhow!("patch apply error for {}: {e}", pf.path))?;

        let policy = WritePolicy::default();
        let applied = write_if_apply(&file_path, &new_content, mode, &policy)?;
        results.push(build_edit_result(&pf.path, original, new_content, applied));
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Plan / Transaction operations
// ---------------------------------------------------------------------------

/// Parse a transaction plan from a JSON string.
pub fn parse_plan(input: &str) -> anyhow::Result<crate::plan::Plan> {
    crate::plan::parse_plan_auto(input, None, None)
}

/// Execute a transaction plan atomically.
///
/// All operations succeed or all are rolled back. Returns the exit code
/// and a JSON string with the operation results.
pub fn execute_plan(plan: crate::plan::Plan, cwd: &Path) -> anyhow::Result<(u8, String)> {
    crate::cmd::tx::execute_plan_direct(plan, cwd)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn doc_set_preview_does_not_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let result = doc_set(
            &file,
            "version",
            serde_json::json!("2.0"),
            ApplyMode::Preview,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.applied);
        assert!(result.new_content.contains("2.0"));
        // File should be unchanged on disk.
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("1.0"));
    }

    #[test]
    fn doc_set_apply_writes_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let result = doc_set(&file, "version", serde_json::json!("2.0"), ApplyMode::Apply).unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("2.0"));
    }

    #[test]
    fn doc_get_reads_value() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"database": {"host": "localhost"}}"#).unwrap();

        let value = doc_get(&file, "database.host").unwrap();
        assert_eq!(value, serde_json::json!("localhost"));
    }

    #[test]
    fn doc_has_returns_true_for_existing() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        assert!(doc_has(&file, "version").unwrap());
        assert!(!doc_has(&file, "nonexistent").unwrap());
    }

    #[test]
    fn doc_delete_removes_key() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"a": 1, "b": 2}"#).unwrap();

        let result = doc_delete(&file, "b", ApplyMode::Apply).unwrap();
        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(!on_disk.contains("\"b\""));
    }

    #[test]
    fn doc_merge_merges_values() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"a": 1}"#).unwrap();

        let result = doc_merge(&file, serde_json::json!({"b": 2}), ApplyMode::Apply).unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("\"b\""));
    }

    #[test]
    fn replace_text_preview() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn old_name() {}\n").unwrap();

        let result = replace_text(
            &file,
            "old_name",
            "new_name",
            &ReplaceOptions::default(),
            ApplyMode::Preview,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.applied);
        assert!(result.new_content.contains("new_name"));
    }

    #[test]
    fn replace_text_apply() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn old_name() {}\n").unwrap();

        let result = replace_text(
            &file,
            "old_name",
            "new_name",
            &ReplaceOptions::default(),
            ApplyMode::Apply,
        )
        .unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("new_name"));
    }

    #[test]
    fn md_replace_section_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("README.md");
        fs::write(
            &file,
            "## Section A\n\nOld body.\n\n## Section B\n\nKeep this.\n",
        )
        .unwrap();

        let result =
            md_replace_section(&file, "Section A", "New body.\n", ApplyMode::Preview).unwrap();

        assert!(result.changed);
        assert!(result.new_content.contains("New body."));
        assert!(result.new_content.contains("Keep this."));
    }

    #[test]
    fn md_upsert_bullet_adds_new() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("CHANGELOG.md");
        fs::write(&file, "# Changes\n\n- Item 1\n").unwrap();

        let result = md_upsert_bullet(&file, "# Changes", "- Item 2", ApplyMode::Preview).unwrap();

        assert!(result.changed);
        assert!(result.new_content.contains("- Item 2"));
    }

    #[test]
    fn file_create_and_delete() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");

        // Create
        let result = file_create(&file, "hello\n", false, ApplyMode::Apply).unwrap();
        assert!(result.applied);
        assert!(file.exists());

        // Delete
        let result = file_delete(&file, ApplyMode::Apply).unwrap();
        assert!(result.applied);
        assert!(!file.exists());
    }

    #[test]
    fn file_rename_works() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "content\n").unwrap();

        let result = file_rename(&src, &dst, false, ApplyMode::Apply).unwrap();
        assert!(result.applied);
        assert!(!src.exists());
        assert!(dst.exists());
    }

    #[test]
    fn apply_mode_check_does_not_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let result = doc_set(&file, "version", serde_json::json!("2.0"), ApplyMode::Check).unwrap();

        assert!(result.changed);
        assert!(!result.applied);
        // File should be unchanged.
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("1.0"));
    }

    #[test]
    fn doc_append_to_array() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(&file, r#"{"items": [1, 2]}"#).unwrap();

        let result = doc_append(&file, "items", serde_json::json!(3), ApplyMode::Apply).unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(parsed["items"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn doc_move_renames_key() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(&file, r#"{"old_key": "value"}"#).unwrap();

        let result = doc_move(&file, "old_key", "new_key", ApplyMode::Apply).unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("new_key"));
        assert!(!on_disk.contains("old_key"));
    }

    #[test]
    fn replace_text_empty_pattern_rejected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "content").unwrap();

        let err = replace_text(
            &file,
            "",
            "x",
            &ReplaceOptions::default(),
            ApplyMode::Preview,
        )
        .unwrap_err();
        assert!(err.to_string().contains("empty search pattern"));
    }

    #[test]
    fn edit_result_unchanged_when_no_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.yaml");
        fs::write(&file, "version: \"1.0\"\n").unwrap();

        // Set to the same value.
        let result = doc_set(
            &file,
            "version",
            serde_json::json!("1.0"),
            ApplyMode::Preview,
        )
        .unwrap();

        assert!(!result.changed);
        assert!(result.diff.is_empty());
    }

    #[test]
    fn yaml_doc_set_preserves_comments() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.yaml");
        fs::write(
            &file,
            "# Main config\nversion: \"1.0\"\n# Keep this comment\nname: test\n",
        )
        .unwrap();

        let result = doc_set(&file, "version", serde_json::json!("2.0"), ApplyMode::Apply).unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("2.0"));
        assert!(on_disk.contains("# Main config"));
        assert!(on_disk.contains("# Keep this comment"));
    }

    #[test]
    fn execute_plan_runs_operations() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let plan_json = r#"{
                "version": "1",
                "operations": [
                    {
                        "op": "replace",
                        "path": "test.txt",
                        "mode": "literal",
                        "from": "hello",
                        "to": "goodbye"
                    }
                ]
            }"#;

        let plan = parse_plan(plan_json).unwrap();
        let (code, _output) = execute_plan(plan, dir.path()).unwrap();
        // execute_plan_direct applies changes and returns SUCCESS.
        assert_eq!(code, crate::exit::SUCCESS);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("goodbye"));
    }

    // Static assertions: all public API types must be Send + Sync.
    const _: () = {
        fn _assert<T: Send + Sync>() {}
        let _ = _assert::<EditResult>;
        let _ = _assert::<ApplyMode>;
        let _ = _assert::<ReplaceOptions>;
        let _ = _assert::<WritePolicyOptions>;
        let _ = _assert::<EolNormalization>;
    };

    #[test]
    fn concurrent_edits_to_different_files() {
        let dir = TempDir::new().unwrap();
        let num_threads = 8;

        // Create files for each thread.
        for i in 0..num_threads {
            let file = dir.path().join(format!("file_{i}.json"));
            fs::write(&file, format!(r#"{{"value": {i}}}"#)).unwrap();
        }

        // Edit all files concurrently.
        std::thread::scope(|s| {
            let dir_path = dir.path();
            for i in 0..num_threads {
                s.spawn(move || {
                    let file = dir_path.join(format!("file_{i}.json"));
                    let result =
                        doc_set(&file, "value", serde_json::json!(i * 100), ApplyMode::Apply)
                            .unwrap();
                    assert!(result.changed);
                    assert!(result.applied);
                });
            }
        });

        // Verify all files were updated correctly.
        for i in 0..num_threads {
            let file = dir.path().join(format!("file_{i}.json"));
            let content = fs::read_to_string(&file).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(parsed["value"], serde_json::json!(i * 100));
        }
    }

    #[test]
    fn concurrent_backup_sessions_no_collision() {
        use crate::backup::{BackupSession, list_sessions};

        let dir = TempDir::new().unwrap();
        let num_threads = 16;

        // Create a file for each thread to back up.
        for i in 0..num_threads {
            let file = dir.path().join(format!("backup_{i}.txt"));
            fs::write(&file, format!("original_{i}")).unwrap();
        }

        // Create backup sessions concurrently.
        let timestamps: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

        std::thread::scope(|s| {
            let dir_path = dir.path();
            let ts_ref = &timestamps;
            for i in 0..num_threads {
                s.spawn(move || {
                    let file = dir_path.join(format!("backup_{i}.txt"));
                    let mut session = BackupSession::new(dir_path).unwrap();
                    session.save_before_write(&file).unwrap();
                    let ts = session.finalize().unwrap().unwrap();
                    ts_ref.lock().unwrap().push(ts);
                });
            }
        });

        let ts = timestamps.into_inner().unwrap();
        // All timestamps must be unique (no collisions).
        let mut sorted = ts.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            num_threads,
            "all backup session timestamps must be unique"
        );

        // All sessions should be listed.
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), num_threads);
    }

    #[test]
    fn concurrent_replace_text_different_files() {
        let dir = TempDir::new().unwrap();
        let num_threads = 8;

        for i in 0..num_threads {
            let file = dir.path().join(format!("code_{i}.rs"));
            fs::write(&file, format!("fn func_{i}() {{}}\n")).unwrap();
        }

        std::thread::scope(|s| {
            let dir_path = dir.path();
            for i in 0..num_threads {
                s.spawn(move || {
                    let file = dir_path.join(format!("code_{i}.rs"));
                    let result = replace_text(
                        &file,
                        &format!("func_{i}"),
                        &format!("renamed_{i}"),
                        &ReplaceOptions::default(),
                        ApplyMode::Apply,
                    )
                    .unwrap();
                    assert!(result.changed);
                    assert!(result.applied);
                });
            }
        });

        for i in 0..num_threads {
            let file = dir.path().join(format!("code_{i}.rs"));
            let content = fs::read_to_string(&file).unwrap();
            assert!(
                content.contains(&format!("renamed_{i}")),
                "file {i} should contain renamed_{i}, got: {content}"
            );
        }
    }

    #[test]
    fn concurrent_md_operations() {
        let dir = TempDir::new().unwrap();
        let num_threads = 4;

        for i in 0..num_threads {
            let file = dir.path().join(format!("doc_{i}.md"));
            fs::write(&file, format!("# Section {i}\n\nOriginal body.\n")).unwrap();
        }

        std::thread::scope(|s| {
            let dir_path = dir.path();
            for i in 0..num_threads {
                s.spawn(move || {
                    let file = dir_path.join(format!("doc_{i}.md"));
                    let result = md_replace_section(
                        &file,
                        &format!("Section {i}"),
                        &format!("Updated body {i}.\n"),
                        ApplyMode::Apply,
                    )
                    .unwrap();
                    assert!(result.changed);
                    assert!(result.applied);
                });
            }
        });

        for i in 0..num_threads {
            let file = dir.path().join(format!("doc_{i}.md"));
            let content = fs::read_to_string(&file).unwrap();
            assert!(content.contains(&format!("Updated body {i}.")));
        }
    }

    #[test]
    fn concurrent_file_create() {
        let dir = TempDir::new().unwrap();
        let num_threads = 8;

        std::thread::scope(|s| {
            let dir_path = dir.path();
            for i in 0..num_threads {
                s.spawn(move || {
                    let file = dir_path.join(format!("new_{i}.txt"));
                    let result =
                        file_create(&file, &format!("content_{i}\n"), false, ApplyMode::Apply)
                            .unwrap();
                    assert!(result.applied);
                });
            }
        });

        for i in 0..num_threads {
            let file = dir.path().join(format!("new_{i}.txt"));
            assert!(file.exists());
            let content = fs::read_to_string(&file).unwrap();
            assert_eq!(content, format!("content_{i}\n"));
        }
    }
}
