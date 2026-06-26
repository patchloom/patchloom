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
//! use std::path::{Path, PathBuf};
//!
//! // Replace text in a file (preview only)
//! let result = api::replace_text(
//!     Path::new("src/config.rs"),
//!     "old_value",
//!     "new_value",
//!     &api::ReplaceOptions::default(),
//!     ApplyMode::Preview,
//!     None,
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
//! # Using with PathGuard (containment)
//!
//! All write operations accept an optional `guard: Option<&PathGuard>` (added for library users needing relaxed containment).
//! Pass `None` for no additional checks (default for most internal use).
//! Pass `Some(&guard)` to enforce the policy (e.g. allow temp dirs via the builder).
//!
//! Example:
//!
//! ```rust,no_run
//! use patchloom::api::{self, ApplyMode, ReplaceOptions};
//! use patchloom::containment::PathGuard;
//! use std::path::{Path, PathBuf};
//!
//! let guard = PathGuard::builder(std::env::current_dir().unwrap())
//!     .allow_temp_directory()  // includes /tmp + platform temp (macOS symlink safe)
//!     .build()
//!     .unwrap();
//!
//! let _ = api::replace_text(
//!     Path::new("src/main.rs"),
//!     "old",
//!     "new",
//!     &ReplaceOptions::default(),
//!     ApplyMode::Preview,
//!     Some(&guard),
//! );
//! ```
//!
//! **Guard semantics (see also #756):** The guard provides *write-time* enforcement and is only checked for `ApplyMode::Apply` writes (via `ensure_contained` + `write_if_apply`). Reads (e.g. for diff computation, `Preview`/`Check` modes, `doc_get`, search) and pre-write loads may still observe or describe paths outside the guard. This is intentional for trusted library embedding (the host/ caller controls visibility). MCP uses a separate strict pre-check layer on all paths. `execute_plan` now accepts a guard (see its docs; #755) and performs upfront validation on declared paths.
//!
//! ## Guard & WritePolicy contract (#801 exhaustive audit)
//!
//! - Every public write API and plan `Operation` (file.create/delete/rename/append, doc.set/merge/append/..., md.*, patch, replace, tidy writes, etc.) goes through `ensure_contained` (Apply only) + `BackupSession` + `atomic_*` + `WritePolicy`.
//! - Upfront declared paths checked for `execute_plan` under guard.
//! - No gaps found on review (greps for ensure/Backup/atomic in api/ + tx.rs + spot in ops).
//! - Regression: the `write_if_apply` + `ensure_contained` helpers + upfront in execute_plan + existing guard tests under ["files"] matrix.
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

use crate::backup::BackupSession;
use crate::containment::PathGuard;
use crate::diff::{DiffResult, format_diff_result, unified_diff};
use crate::write::{EolMode, WritePolicy, atomic_write};

#[cfg(any(feature = "cli", feature = "files"))]
pub use crate::tx::{
    TxChange, TxLintResult, TxOutput as PlanReport, TxReadResult, TxSearchMatch, TxSearchResult,
};

mod doc;
pub use self::doc::*;

mod replace;
pub use self::replace::*;

mod md;
pub use self::md::*;

mod file;
pub use self::file::*;

mod patch;
pub use self::patch::*;

mod tidy;
pub use self::tidy::*;

mod search;
pub use self::search::*;

mod read;
pub use self::read::*;

mod plan;
pub use self::plan::*;

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
    /// Action/kind of the edit (e.g. "append", "create", "replace", "rename", "doc.set").
    /// Helps consumers (like Bline) distinguish cross-file or op type without parsing path.
    pub action: &'static str,
    /// For cross-file operations (e.g. `file_rename`, `md_move_section` with `to`),
    /// the destination path if different from `path`.
    pub dest_path: Option<String>,
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
    /// Delete/replace entire lines containing the match rather than just the
    /// matched text. When `to` is empty, matching lines are removed.
    pub whole_line: bool,
    /// Restrict matching to a 1-based inclusive line range `(start, end)`.
    /// Requires `whole_line` to be `true`.
    pub range: Option<(usize, Option<usize>)>,
    /// Return success (no error) even when the pattern matches nothing.
    pub if_exists: bool,
    /// When true, match only at word boundaries (`\b` in regex terms).
    /// Prevents `SetupFile` from matching inside `BenchSetupFile`.
    /// The pattern is auto-escaped for regex metacharacters before
    /// wrapping with `\b` anchors.
    pub word_boundary: bool,
}

/// Write policy options for controlling file write transformations.
#[derive(Debug, Clone, Default)]
pub struct WritePolicyOptions {
    /// Ensure non-empty files end with a newline.
    pub ensure_final_newline: bool,
    /// Normalize line endings. `None` means keep existing (`EolMode::Keep`).
    pub normalize_eol: Option<EolMode>,
    /// Remove trailing whitespace from each line.
    pub trim_trailing_whitespace: bool,
    /// Collapse consecutive blank lines into a single blank line.
    pub collapse_blanks: bool,
}

/// Backward-compatible alias for [`EolMode`](crate::write::EolMode).
#[deprecated(
    since = "0.6.0",
    note = "use crate::write::EolMode instead (Lf, Crlf, Cr, Keep)"
)]
pub type EolNormalization = EolMode;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert user-facing `WritePolicyOptions` to the internal `WritePolicy`.
///
/// Note (for #821): only `tidy` currently accepts `&WritePolicyOptions` at the high-level API.
/// Other mutating functions (`file_append`, `replace_text`, `doc_*`, `md_*`, etc.) default to
/// `WritePolicy::default()` for ergonomics and library backward compat. For full control use a
/// 1-op plan via `execute_plan` (which supports per-step write_policy) or the lower-level
/// `write` + `atomic_write` primitives. Differences from MCP (which uses strict pre-checks + defaults)
/// are intentional.
pub fn make_write_policy(opts: &WritePolicyOptions) -> WritePolicy {
    WritePolicy {
        ensure_final_newline: opts.ensure_final_newline,
        normalize_eol: opts.normalize_eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: opts.trim_trailing_whitespace,
        collapse_blanks: opts.collapse_blanks,
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
    };
    format_diff_result(&result)
}

/// Generalized helper for Apply-mode mutations that need backup + guard.
///
/// Used by write_if_apply and special file ops (create/delete/rename cross-file).
fn apply_mutation(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    prepare_backup: impl FnOnce(&mut BackupSession) -> anyhow::Result<()>,
    perform_mutation: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    if mode != ApplyMode::Apply {
        return Ok(false);
    }
    ensure_contained(guard, path)?;
    // Use the project root (parent of the file) as backup root.
    // For library users, backup is best-effort.
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    prepare_backup(&mut backup)?;
    perform_mutation()?;
    backup.finalize()?;
    Ok(true)
}

/// Generalized cross-file mutation helper (for rename and md cross-file moves).
///
/// Handles guard checks and backup for src (and optional dst).
/// Used to centralize the cross-file logic per code review #839.
fn apply_cross_file_mutation(
    src: &Path,
    dst: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    prepare_backup: impl FnOnce(&mut BackupSession) -> anyhow::Result<()>,
    perform_mutation: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    if mode != ApplyMode::Apply {
        return Ok(false);
    }
    ensure_contained(guard, src)?;
    if let Some(d) = dst {
        ensure_contained(guard, d)?;
    }
    let cwd = src.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    prepare_backup(&mut backup)?;
    perform_mutation()?;
    backup.finalize()?;
    Ok(true)
}

fn write_if_apply(
    path: &Path,
    new_content: &str,
    mode: ApplyMode,
    policy: &WritePolicy,
    guard: Option<&PathGuard>,
) -> anyhow::Result<bool> {
    apply_mutation(
        path,
        mode,
        guard,
        |backup| backup.save_before_write(path),
        || atomic_write(path, new_content, policy),
    )
}

/// Private helper to centralize the guard check and eliminate duplicated
/// inline `if let Some(g) = guard { g.check_path... }` blocks in every
/// write path.
fn ensure_contained(guard: Option<&PathGuard>, path: &Path) -> anyhow::Result<()> {
    if let Some(g) = guard {
        g.check_path(&path.to_string_lossy())
            .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {}", e))?;
    }
    Ok(())
}

fn build_edit_result(
    path_str: &str,
    original: String,
    new_content: String,
    applied: bool,
    action: &'static str,
    dest_path: Option<String>,
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
        action,
        dest_path,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
