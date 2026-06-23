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

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::backup::BackupSession;
use crate::containment::PathGuard;
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
    /// Action/kind of the edit (e.g. "append", "create", "replace", "rename", "doc.set").
    /// Helps consumers (like Bline) distinguish cross-file or op type without parsing path.
    pub action: &'static str,
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
    /// Normalize line endings. `None` means keep existing.
    pub normalize_eol: Option<EolNormalization>,
    /// Remove trailing whitespace from each line.
    pub trim_trailing_whitespace: bool,
    /// Collapse consecutive blank lines into a single blank line.
    pub collapse_blanks: bool,
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
    use crate::write::EolMode;
    WritePolicy {
        ensure_final_newline: opts.ensure_final_newline,
        normalize_eol: match opts.normalize_eol {
            None => EolMode::Keep,
            Some(EolNormalization::Lf) => EolMode::Lf,
            Some(EolNormalization::Crlf) => EolMode::Crlf,
        },
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

fn write_if_apply(
    path: &Path,
    new_content: &str,
    mode: ApplyMode,
    policy: &WritePolicy,
    guard: Option<&PathGuard>,
) -> anyhow::Result<bool> {
    if mode != ApplyMode::Apply {
        return Ok(false);
    }
    ensure_contained(guard, path)?;
    // Use the project root (parent of the file) as backup root.
    // For library users, backup is best-effort.
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    backup.save_before_write(path)?;
    atomic_write(path, new_content, policy)?;
    backup.finalize()?;
    Ok(true)
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
    }
}

// ---------------------------------------------------------------------------
// Document operations (JSON/YAML/TOML)
// ---------------------------------------------------------------------------

/// A parsed document loaded from disk, ready for mutation.
struct LoadedDoc {
    path_str: String,
    format: ops::doc::FileFormat,
    original: String,
    value: serde_json::Value,
}

/// Load and parse a JSON/YAML/TOML file.
fn load_doc(path: &Path) -> anyhow::Result<LoadedDoc> {
    let path_str = path.to_string_lossy().into_owned();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value = ops::doc::parse_doc(&original, &format)?;
    Ok(LoadedDoc {
        path_str,
        format,
        original,
        value,
    })
}

/// Serialize a mutated document and build an `EditResult`.
fn finish_doc_edit(
    path: &Path,
    doc: &LoadedDoc,
    new_value: &serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let new_content =
        ops::doc::serialize_value_preserving(&doc.original, &doc.value, new_value, &doc.format)?;
    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &doc.path_str,
        doc.original.clone(),
        new_content,
        applied,
        "doc.set",
    ))
}

/// Set a value at a selector path in a JSON, YAML, or TOML file.
///
/// The file format is detected from the extension. The selector uses
/// patchloom's selector syntax (e.g., `"database.host"`, `"items[0].name"`).
pub fn doc_set(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::set_at_path(&mut new_value, &segments, value)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Delete a value at a selector path in a JSON, YAML, or TOML file.
pub fn doc_delete(
    path: &Path,
    selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::delete_at_selector(&mut new_value, &segments)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Deep-merge a value into the root of a JSON, YAML, or TOML file.
pub fn doc_merge(
    path: &Path,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    ops::doc::deep_merge(&mut new_value, &value);

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Get a value at a selector path from a JSON, YAML, or TOML file.
///
/// This is a read-only operation; the `mode` parameter is ignored.
pub fn doc_get(path: &Path, selector: &str) -> anyhow::Result<serde_json::Value> {
    let doc = load_doc(path)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc.value, &segments);
    match result.len() {
        0 => bail!("selector '{}' matched nothing", selector),
        1 => Ok(result
            .into_iter()
            .next()
            .expect("len==1 guarantees element")
            .clone()),
        _ => Ok(serde_json::Value::Array(
            result.into_iter().cloned().collect(),
        )),
    }
}

/// Check whether a selector path exists in a JSON, YAML, or TOML file.
pub fn doc_has(path: &Path, selector: &str) -> anyhow::Result<bool> {
    let doc = load_doc(path)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc.value, &segments);
    Ok(!result.is_empty())
}

/// Append a value to an array at a selector path.
pub fn doc_append(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    let target = ops::doc::navigate_mut(&mut new_value, &segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;
    arr.push(value);

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Prepend a value to an array at a selector path.
pub fn doc_prepend(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    let target = ops::doc::navigate_mut(&mut new_value, &segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;
    arr.insert(0, value);

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Update all values matching a selector with a new value.
///
/// Returns an `EditResult`. The number of matches updated is reflected in
/// whether the content changed.
pub fn doc_update(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::update_matching(&mut new_value, &segments, &value);

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Ensure a value exists at a selector path; set it only if missing.
pub fn doc_ensure(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    // Only set if the path does not already exist.
    let existing = selector::eval(&new_value, &segments);
    if existing.is_empty() {
        ops::doc::set_at_path(&mut new_value, &segments, value)?;
    }

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Delete array elements matching a predicate (e.g., `"name=old"`).
pub fn doc_delete_where(
    path: &Path,
    selector: &str,
    predicate: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::delete_where(&mut new_value, &segments, predicate)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard)
}

/// Move a value from one selector path to another within the same file.
pub fn doc_move(
    path: &Path,
    from_selector: &str,
    to_selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let from_segments = selector::parse_anyhow(from_selector)?;
    let to_segments = selector::parse_anyhow(to_selector)?;
    ops::doc::move_at_path(&mut new_value, &from_segments, &to_segments)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard)
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
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if from.is_empty() && !opts.regex {
        bail!("empty search pattern");
    }
    if opts.range.is_some() && !opts.whole_line {
        bail!("range requires whole_line to be true");
    }
    if opts.whole_line && opts.multiline {
        bail!("whole_line and multiline cannot be combined");
    }

    let compiled_re = ops::replace::compile_replace_regex(
        from,
        opts.regex,
        opts.case_insensitive,
        opts.multiline,
        opts.word_boundary,
    )?;

    let direct_to = if opts.insert_before.is_none() && opts.insert_after.is_none() {
        Some(to.to_string())
    } else {
        None
    };
    let replacement = ops::replace::replacement_text(
        from,
        &direct_to,
        &opts.insert_before,
        &opts.insert_after,
        compiled_re.is_some(),
    );

    let (new_content, count) = if opts.whole_line {
        ops::replace::replace_whole_lines(
            &original,
            from,
            &replacement,
            compiled_re.as_ref(),
            opts.nth,
            opts.range,
        )
    } else {
        ops::replace::replace_content(
            &original,
            from,
            &replacement,
            compiled_re.as_ref(),
            opts.nth,
        )
    };
    let new_content = new_content.into_owned();

    if count == 0 && opts.if_exists {
        return Ok(build_edit_result(
            &path_str,
            original.clone(),
            original,
            false,
            "edit",
        ));
    }

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
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
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::replace_section_in(&original, heading, content)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

/// Insert or update a bullet point under a markdown heading.
///
/// If the bullet already exists (exact match), the file is unchanged.
pub fn md_upsert_bullet(
    path: &Path,
    heading: &str,
    bullet: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::upsert_bullet_in(&original, heading, bullet)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

/// Append a row to a markdown table under a heading.
pub fn md_table_append(
    path: &Path,
    heading: &str,
    row: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::table_append_for_tx(&original, heading, row)
        .ok_or_else(|| anyhow::anyhow!("no table found under heading '{}'", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

/// Insert content after a markdown heading.
pub fn md_insert_after_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_after_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

/// Move a markdown section to a position relative to another heading.
///
/// For same-file moves, pass `to` as `None`. For cross-file moves, pass the
/// destination file path in `to`.
///
/// The returned `EditResult` always describes the source `path`. When a
/// cross-file move succeeds under Apply, the destination is mutated as a
/// side effect (inspect disk or re-read for its content). This is
/// pre-existing behavior (#757); tx plans report the source result for the op.
pub fn md_move_section(
    path: &Path,
    heading: &str,
    position: (&str, &str),
    to: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let dest_content = match to {
        Some(dest_path) => std::fs::read_to_string(dest_path)
            .with_context(|| format!("failed to read {}", dest_path.display()))?,
        None => original.clone(),
    };

    let (new_source, new_dest) =
        ops::md::move_section_in(&original, heading, &dest_content, position, to.is_none())
            .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    // Write the destination file for cross-file moves.
    if let Some(dest_path) = to {
        if mode == ApplyMode::Apply {
            ensure_contained(guard, dest_path)?;
        }
        let _ = write_if_apply(dest_path, &new_dest, mode, &policy, guard)?;
    }
    if mode == ApplyMode::Apply {
        ensure_contained(guard, path)?;
    }
    let applied = write_if_apply(path, &new_source, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str, original, new_source, applied, "md.move",
    ))
}

/// Remove duplicate headings at the same level in a markdown file.
///
/// Returns the `EditResult` and a list of removed duplicate heading texts.
pub fn md_dedupe_headings(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<(EditResult, Vec<String>)> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let (new_content, removed) = ops::md::dedupe_headings_in(&original);

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok((
        build_edit_result(&path_str, original, new_content, applied, "md.dedupe"),
        removed,
    ))
}

/// A lint issue found in a markdown file.
///
/// Re-exported from `ops::md` so library consumers don't need to import
/// from the internal `cmd` module path.
pub use crate::ops::md::LintIssue;

/// Lint a markdown file for common agent-rules issues (duplicate headings,
/// missing sections, etc.).
///
/// Returns a list of lint issues found. This is a read-only operation.
pub fn md_lint_agents(path: &Path) -> anyhow::Result<Vec<LintIssue>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(crate::ops::md::lint_agents_content(&content))
}

/// Insert content before a markdown heading.
pub fn md_insert_before_heading(
    path: &Path,
    heading: &str,
    insertion: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let new_content = ops::md::insert_before_heading_in(&original, heading, insertion)
        .ok_or_else(|| anyhow::anyhow!("heading '{}' not found", heading))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
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

    let applied = if mode == ApplyMode::Apply {
        ensure_contained(guard, path)?;
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
        "create",
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

    let applied = if mode == ApplyMode::Apply {
        ensure_contained(guard, path)?;
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
        "delete",
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

    let applied = if mode == ApplyMode::Apply {
        ensure_contained(guard, src)?;
        ensure_contained(guard, dst)?;
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
        "rename",
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

    let applied = if mode == ApplyMode::Apply {
        ensure_contained(guard, path)?;
        let cwd = path.parent().unwrap_or_else(|| Path::new("."));
        let mut backup = BackupSession::new(cwd)?;
        backup.save_before_write(path)?;
        let policy = WritePolicy::default();
        atomic_write(path, &combined, &policy)?;
        backup.finalize()?;
        true
    } else {
        false
    };

    Ok(build_edit_result(
        &path_str, original, combined, applied, "append",
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

    let applied = if mode == ApplyMode::Apply {
        ensure_contained(guard, path)?;
        let cwd = path.parent().unwrap_or_else(|| Path::new("."));
        let mut backup = BackupSession::new(cwd)?;
        backup.save_before_write(path)?;
        let policy = WritePolicy::default();
        atomic_write(path, &combined, &policy)?;
        backup.finalize()?;
        true
    } else {
        false
    };

    Ok(build_edit_result(
        &path_str, original, combined, applied, "prepend",
    ))
}

// ---------------------------------------------------------------------------
// Patch operations
// ---------------------------------------------------------------------------

/// Apply a unified diff patch to a file.
///
/// Returns an `EditResult` with the patched content.
pub fn apply_patch(
    path: &Path,
    patch_text: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
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
        .filter(|pf| {
            pf.path == path_str_ref || std::path::Path::new(path_str_ref).ends_with(&pf.path)
        })
        .flat_map(|pf| pf.hunks.iter())
        .collect();

    if hunks.is_empty() {
        bail!("no hunks found for path '{}'", path_str);
    }

    let owned_hunks: Vec<_> = hunks.into_iter().cloned().collect();
    let new_content = ops::patch::apply_hunks(&original, &owned_hunks)
        .map_err(|e| anyhow::anyhow!("patch apply error: {e}"))?;

    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

/// Apply a multi-file patch. Returns one `EditResult` per affected file.
pub fn apply_patch_file(
    patch_text: &str,
    cwd: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
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
        let applied = write_if_apply(&file_path, &new_content, mode, &policy, guard)?;
        results.push(build_edit_result(
            &pf.path,
            original,
            new_content,
            applied,
            "patch",
        ));
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Tidy operations
// ---------------------------------------------------------------------------

/// Apply whitespace normalization to a file using the given write policy.
///
/// Normalizes final newlines, line endings, trailing whitespace, and
/// consecutive blank lines according to the policy options.
pub fn tidy(
    path: &Path,
    policy_opts: &WritePolicyOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let path_str = path.to_string_lossy();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let policy = make_write_policy(policy_opts);
    let new_content = crate::write::apply_policy(&original, &policy).into_owned();

    // Use a default (no-op) policy for writing since we already applied
    // the transformations above.
    let noop_policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &noop_policy, guard)?;
    Ok(build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        "tidy",
    ))
}

// ---------------------------------------------------------------------------
// Search operations
// ---------------------------------------------------------------------------

/// A single search match.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// 1-based line number.
    pub line_number: usize,
    /// The matched line content.
    pub line: String,
}

/// Search for a pattern in a file, returning all matching lines.
///
/// This is a read-only operation.
pub fn search(
    path: &Path,
    pattern: &str,
    regex: bool,
    case_insensitive: bool,
) -> anyhow::Result<Vec<SearchMatch>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if pattern.is_empty() {
        bail!("search pattern must not be empty");
    }

    let compiled_re = if regex || case_insensitive {
        Some(ops::replace::compile_replace_regex(
            pattern,
            regex,
            case_insensitive,
            false,
            false,
        )?)
    } else {
        None
    };

    let mut matches = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let matched = match &compiled_re {
            Some(Some(re)) => re.is_match(line),
            _ => line.contains(pattern),
        };
        if matched {
            matches.push(SearchMatch {
                line_number: i + 1,
                line: line.to_string(),
            });
        }
    }
    Ok(matches)
}

/// Options for full directory search (for library consumers).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Treat pattern as literal string (not regex).
    pub literal: bool,
    /// Treat as regex (default false, use with literal=false).
    pub regex: bool,
    /// Case insensitive match.
    pub case_insensitive: bool,
    /// Context lines around matches.
    pub context: Option<usize>,
    /// Glob patterns to include (e.g. "*.rs").
    pub globs: Vec<String>,
    /// Max number of results (0 for unlimited).
    pub max_results: usize,
}

/// Result of a search match with optional context.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: PathBuf,
    pub line_number: usize,
    pub line: String,
    pub column: usize,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Helper to build context slices for a match line. DRY for search impls.
fn build_context_lines(
    all_lines: &[&str],
    match_idx: usize,
    ctx: usize,
) -> (Vec<String>, Vec<String>) {
    if ctx == 0 {
        return (vec![], vec![]);
    }
    let before_start = match_idx.saturating_sub(ctx);
    let after_end = (match_idx + 1 + ctx).min(all_lines.len());
    let before = all_lines[before_start..match_idx]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let after = all_lines[match_idx + 1..after_end]
        .iter()
        .map(|s| s.to_string())
        .collect();
    (before, after)
}

/// Search a directory (or file) recursively for pattern.
/// Respects globs if "files" feature enabled. Uses parallel processing when available.
/// This provides the full power of the CLI search for library use (see #773).
pub fn search_directory(
    root: &Path,
    pattern: &str,
    opts: &SearchOptions,
) -> anyhow::Result<Vec<SearchResult>> {
    if pattern.is_empty() {
        bail!("search pattern must not be empty");
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    {
        use crate::files::{build_glob_matcher, collect_file_paths, par_process_files};
        let glob_matcher = build_glob_matcher(&opts.globs)?;
        let glob_roots = vec![root.to_path_buf()];
        let file_paths = collect_file_paths(root, false)?;

        let limit = if opts.max_results > 0 {
            opts.max_results
        } else {
            usize::MAX
        };

        let file_result_groups: Vec<Vec<SearchResult>> =
            par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let v = search_one_file_for_api(path, pattern, opts, root);
                if v.is_empty() { None } else { Some(v) }
            });

        let mut res: Vec<SearchResult> = file_result_groups.into_iter().flatten().collect();
        if limit < usize::MAX {
            res.truncate(limit);
        }
        Ok(res)
    }

    #[cfg(not(any(feature = "cli", feature = "files")))]
    {
        // fallback to single file search (multi-match supported via basic search)
        if root.is_file() {
            let basic = search(root, pattern, opts.regex, opts.case_insensitive)?;
            let display = root.to_path_buf();
            // re-read for context lines (fallback is rare / no "files" feature)
            let all_lines: Vec<&str> = std::fs::read_to_string(root)
                .with_context(|| format!("failed to read {}", root.display()))?
                .lines()
                .collect();
            let c = opts.context.unwrap_or(0);
            let content = std::fs::read_to_string(root)
                .with_context(|| format!("failed to read {}", root.display()))?;
            let all_lines: Vec<&str> = content.lines().collect();
            let results: Vec<SearchResult> = basic
                .into_iter()
                .map(|m| {
                    let i = m.line_number - 1;
                    let (context_before, context_after) = build_context_lines(&all_lines, i, c);
                    // column not tracked in basic fallback search (always 1; full impl behind "files" feature)
                    let column = 1;
                    SearchResult {
                        path: display.clone(),
                        line_number: m.line_number,
                        line: m.line,
                        column,
                        context_before,
                        context_after,
                    }
                })
                .collect();
            Ok(results)
        } else {
            bail!("directory search requires the 'files' feature to be enabled");
        }
    }
}

#[cfg(any(feature = "cli", feature = "files"))]
fn search_one_file_for_api(
    path: &Path,
    pattern: &str,
    opts: &SearchOptions,
    root: &Path,
) -> Vec<SearchResult> {
    let content = match crate::files::read_text_file(path) {
        Some(c) => c,
        None => return vec![],
    };
    let display = crate::files::relative_display(path, root);
    let pat = if opts.literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let re = if opts.regex || opts.case_insensitive {
        regex::RegexBuilder::new(&pat)
            .case_insensitive(opts.case_insensitive)
            .build()
            .ok()
    } else {
        None
    };

    let mut results = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let matched = if let Some(re) = &re {
            re.is_match(line)
        } else {
            line.contains(pattern)
        };
        if matched {
            let all_lines: Vec<&str> = content.lines().collect();
            let c = opts.context.unwrap_or(0);
            let (context_before, context_after) = build_context_lines(&all_lines, i, c);
            let column = if opts.regex || opts.case_insensitive {
                if let Some(re) = &re {
                    re.find(line).map_or(1, |m| m.start() + 1)
                } else {
                    1
                }
            } else {
                line.find(pattern).map_or(1, |p| p + 1)
            };
            results.push(SearchResult {
                path: display.to_path_buf(),
                line_number: i + 1,
                line: line.to_string(),
                column,
                context_before,
                context_after,
            });
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Read operations
// ---------------------------------------------------------------------------

/// Read a file's content, optionally restricted to a line range.
///
/// Line numbers are 1-based inclusive. This is a read-only operation.
pub fn read(
    path: &Path,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    match (start_line, end_line) {
        (None, None) => Ok(content),
        (start, end) => {
            let start = start.unwrap_or(1).saturating_sub(1); // convert to 0-based
            let lines: Vec<&str> = content.lines().collect();
            let end = end.unwrap_or(lines.len()).min(lines.len());
            if start >= lines.len() {
                return Ok(String::new());
            }
            let selected: Vec<&str> = lines[start..end].to_vec();
            let mut result = selected.join("\n");
            if !result.is_empty() {
                result.push('\n');
            }
            Ok(result)
        }
    }
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
///
/// The optional `guard` is threaded through to all operations for
/// PathGuard enforcement (see module docs for PathGuard usage). Pass
/// `None` for no additional containment checks (current default behavior
/// for most callers).
///
/// Available with the `files` feature (for pure library use without the
/// CLI) or the `cli` feature.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn execute_plan(
    plan: crate::plan::Plan,
    cwd: &Path,
    guard: Option<&PathGuard>,
) -> anyhow::Result<(u8, String)> {
    crate::tx::execute_plan_direct(plan, cwd, guard)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::containment::{AbsolutePathPolicy, PathGuard};
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
            None,
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

        let result = doc_set(
            &file,
            "version",
            serde_json::json!("2.0"),
            ApplyMode::Apply,
            None,
        )
        .unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("2.0"));
    }

    #[test]
    fn doc_set_respects_guard() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();

        let result = doc_set(
            &file,
            "version",
            serde_json::json!("2.0"),
            ApplyMode::Apply,
            Some(&guard),
        )
        .unwrap();

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

        let result = doc_delete(&file, "b", ApplyMode::Apply, None).unwrap();
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

        let result = doc_merge(&file, serde_json::json!({"b": 2}), ApplyMode::Apply, None).unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(
            parsed["a"],
            serde_json::json!(1),
            "merge must preserve existing keys"
        );
        assert_eq!(parsed["b"], serde_json::json!(2), "merge must add new keys");
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
            None,
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
            None,
        )
        .unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("new_name"));
    }

    #[test]
    fn replace_text_with_relaxed_guard() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn old_name() {}\n").unwrap();

        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let result = replace_text(
            &file,
            "old_name",
            "new_name",
            &ReplaceOptions::default(),
            ApplyMode::Apply,
            Some(&guard),
        )
        .unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("new_name"));
    }

    /// QA coverage: exercise actual write to a conventional temp path using high-level api
    /// under .allow_temp_directory() guard (the main motivation for relaxed policy in
    /// library use by agents). Uses literal /tmp (exercises #781 fix path + ancestor canon).
    #[cfg(unix)]
    #[test]
    fn file_create_writes_to_literal_tmp_under_allow_temp_guard() {
        let dir = TempDir::new().unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let tmp_path = format!("/tmp/patchloom_api_tmp_test_{}.txt", std::process::id());
        let _ = fs::remove_file(&tmp_path);

        let result = file_create(
            Path::new(&tmp_path),
            "temp content via guard\n",
            true, // force ok for test
            ApplyMode::Apply,
            Some(&guard),
        )
        .unwrap();

        assert!(result.applied);
        assert!(result.changed);
        let content = fs::read_to_string(&tmp_path).unwrap();
        assert!(content.contains("temp content via guard"));
        let _ = fs::remove_file(&tmp_path);
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
            md_replace_section(&file, "Section A", "New body.\n", ApplyMode::Preview, None)
                .unwrap();

        assert!(result.changed);
        assert!(result.new_content.contains("New body."));
        assert!(result.new_content.contains("Keep this."));
    }

    #[test]
    fn md_upsert_bullet_adds_new() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("CHANGELOG.md");
        fs::write(&file, "# Changes\n\n- Item 1\n").unwrap();

        let result =
            md_upsert_bullet(&file, "# Changes", "- Item 2", ApplyMode::Preview, None).unwrap();

        assert!(result.changed);
        assert!(result.new_content.contains("- Item 2"));
    }

    #[test]
    fn file_create_and_delete() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");

        // Create
        let result = file_create(&file, "hello\n", false, ApplyMode::Apply, None).unwrap();
        assert!(result.applied);
        assert!(file.exists());

        // Delete
        let result = file_delete(&file, ApplyMode::Apply, None).unwrap();
        assert!(result.applied);
        assert!(!file.exists());
    }

    #[test]
    #[cfg(unix)]
    fn file_create_force_propagates_read_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let file = dir.path().join("unreadable.txt");
        fs::write(&file, "original").unwrap();
        fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Root (common in Docker) can still read mode-000 files. Skip when
        // permissions do not actually block reading.
        if fs::read_to_string(&file).is_ok() {
            fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let err = file_create(&file, "new", true, ApplyMode::Apply, None).unwrap_err();
        // Restore permissions so TempDir cleanup succeeds.
        fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            err.to_string().contains("failed to read existing file"),
            "expected read error, got: {err}"
        );
    }

    #[test]
    fn file_rename_works() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "content\n").unwrap();

        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();
        let result = file_rename(&src, &dst, false, ApplyMode::Apply, Some(&guard)).unwrap();
        assert!(result.applied);
        assert!(!src.exists());
        assert!(dst.exists());
        let dst_content = fs::read_to_string(&dst).unwrap();
        assert_eq!(
            dst_content, "content\n",
            "rename must preserve file content"
        );
    }

    #[test]
    fn file_rename_rejects_guard_on_destination() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "content\n").unwrap();

        // Strict Reject guard: passing an absolute dst triggers rejection on the destination.
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let abs_dst = dst.canonicalize().unwrap_or(dst);

        let err = file_rename(&src, &abs_dst, false, ApplyMode::Apply, Some(&guard)).unwrap_err();
        assert!(err.to_string().contains("path rejected by workspace guard"));
    }

    #[test]
    fn apply_mode_check_does_not_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let result = doc_set(
            &file,
            "version",
            serde_json::json!("2.0"),
            ApplyMode::Check,
            None,
        )
        .unwrap();

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

        let result =
            doc_append(&file, "items", serde_json::json!(3), ApplyMode::Apply, None).unwrap();

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

        let result = doc_move(&file, "old_key", "new_key", ApplyMode::Apply, None).unwrap();

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
            None,
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
            None,
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

        let result = doc_set(
            &file,
            "version",
            serde_json::json!("2.0"),
            ApplyMode::Apply,
            None,
        )
        .unwrap();

        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("2.0"));
        assert!(on_disk.contains("# Main config"));
        assert!(on_disk.contains("# Keep this comment"));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
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
                    },
                    {
                        "op": "file.append",
                        "path": "test.txt",
                        "content": "\n+appended"
                    }
                ]
            }"#;

        let plan = parse_plan(plan_json).unwrap();
        let (code, _output) = execute_plan(plan, dir.path(), None).unwrap();
        // execute_plan_direct applies changes and returns SUCCESS.
        assert_eq!(code, crate::exit::SUCCESS);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("goodbye"));
        assert!(on_disk.contains("+appended"));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn execute_plan_respects_relaxed_guard() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("plan.txt");
        fs::write(&file, "old\n").unwrap();

        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let plan_json = format!(
            r#"{{
                "version": "1",
                "operations": [
                    {{
                        "op": "replace",
                        "path": "{}",
                        "mode": "literal",
                        "from": "old",
                        "to": "new"
                    }}
                ]
            }}"#,
            file.to_string_lossy().replace('\\', "/")
        );

        let plan = parse_plan(&plan_json).unwrap();
        let (code, _output) = execute_plan(plan, dir.path(), Some(&guard)).unwrap();
        assert_eq!(code, crate::exit::SUCCESS);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("new"));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn execute_plan_rejects_on_guard() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("guarded.txt");
        fs::write(&file, "content\n").unwrap();

        // Strict reject on abs path outside
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let abs = file.canonicalize().unwrap_or_else(|_| file.clone());

        let plan_json = format!(
            r#"{{
                "version": "1",
                "operations": [
                    {{
                        "op": "file.delete",
                        "path": "{}"
                    }}
                ]
            }}"#,
            abs.to_string_lossy().replace('\\', "/")
        );

        let plan = parse_plan(&plan_json).unwrap();
        let err = execute_plan(plan, dir.path(), Some(&guard)).unwrap_err();
        assert!(err.to_string().contains("path rejected by workspace guard"));
        // No mutation
        assert!(fs::read_to_string(&file).unwrap().contains("content"));
    }

    #[test]
    fn apply_patch_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

        let patch = format!(
            "--- a/{f}\n+++ b/{f}\n@@ -1,3 +1,3 @@\n fn hello() {{\n-    println!(\"hi\");\n+    println!(\"hello world\");\n }}\n",
            f = "code.rs"
        );

        let result = apply_patch(&file, &patch, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        assert!(!result.applied);
        assert!(result.new_content.contains("hello world"));

        // File should be unchanged on disk.
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("\"hi\""));
    }

    #[test]
    fn md_table_append_adds_row() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("table.md");
        fs::write(
            &file,
            "# Data\n\n| Name | Value |\n|------|-------|\n| a    | 1     |\n",
        )
        .unwrap();

        let result =
            md_table_append(&file, "# Data", "| b    | 2     |", ApplyMode::Apply, None).unwrap();

        assert!(result.changed);
        assert!(result.applied);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("| b    | 2     |"));
    }

    #[test]
    fn md_insert_before_heading_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(&file, "# First\n\nBody 1.\n\n# Second\n\nBody 2.\n").unwrap();

        let result = md_insert_before_heading(
            &file,
            "Second",
            "Inserted before second.\n\n",
            ApplyMode::Preview,
            None,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.applied);
        assert!(result.new_content.contains("Inserted before second."));
    }

    #[test]
    fn md_insert_after_heading_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(&file, "# First\n\nBody 1.\n\n# Second\n\nBody 2.\n").unwrap();

        let result = md_insert_after_heading(
            &file,
            "First",
            "Inserted after first.\n\n",
            ApplyMode::Preview,
            None,
        )
        .unwrap();

        assert!(result.changed);
        assert!(!result.applied);
        assert!(result.new_content.contains("Inserted after first."));
        // Verify insertion is after the heading, not before.
        let pos_heading = result.new_content.find("# First").unwrap();
        let pos_insertion = result.new_content.find("Inserted after first.").unwrap();
        assert!(pos_insertion > pos_heading);
    }

    #[test]
    fn apply_patch_file_applies_multi_file_patch() {
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "aaa\n").unwrap();
        fs::write(&b, "bbb\n").unwrap();

        let patch = "\
--- a/a.txt\n\
+++ b/a.txt\n\
@@ -1 +1 @@\n\
-aaa\n\
+AAA\n\
--- a/b.txt\n\
+++ b/b.txt\n\
@@ -1 +1 @@\n\
-bbb\n\
+BBB\n";

        let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.changed && r.applied));
        assert_eq!(fs::read_to_string(&a).unwrap(), "AAA\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "BBB\n");
    }

    #[test]
    fn apply_patch_path_matching_uses_path_components() {
        // Regression: string ends_with matched "notb/foo.rs" for a patch
        // targeting "b/foo.rs". Path::ends_with does component matching.
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("notb");
        fs::create_dir(&sub).unwrap();
        let file = sub.join("foo.rs");
        fs::write(&file, "original\n").unwrap();

        // Patch targets "b/foo.rs" which should NOT match "notb/foo.rs"
        let patch = "\
--- a/b/foo.rs\n\
+++ b/b/foo.rs\n\
@@ -1 +1 @@\n\
-original\n\
+changed\n";

        let result = apply_patch(&file, patch, ApplyMode::Preview, None);
        assert!(result.is_err() || !result.unwrap().changed);
    }

    #[test]
    fn make_write_policy_maps_options() {
        let opts = WritePolicyOptions {
            ensure_final_newline: true,
            normalize_eol: Some(EolNormalization::Lf),
            trim_trailing_whitespace: true,
            collapse_blanks: true,
        };
        let policy = make_write_policy(&opts);
        assert!(policy.ensure_final_newline);
        assert!(policy.trim_trailing_whitespace);
        assert!(policy.collapse_blanks);
        assert_eq!(
            policy.normalize_eol,
            crate::write::EolMode::Lf,
            "should map EolNormalization::Lf to EolMode::Lf"
        );

        // Default options should produce default policy.
        let default_policy = make_write_policy(&WritePolicyOptions::default());
        assert!(!default_policy.ensure_final_newline);
        assert!(!default_policy.trim_trailing_whitespace);
        assert!(!default_policy.collapse_blanks);
    }

    // --- New API function tests (#573) ---

    #[test]
    fn doc_prepend_adds_to_front() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(&file, r#"{"items": [2, 3]}"#).unwrap();

        let result =
            doc_prepend(&file, "items", serde_json::json!(1), ApplyMode::Apply, None).unwrap();
        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(parsed["items"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn doc_update_changes_matching_values() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(
            &file,
            r#"{"items": [{"name": "a", "val": 1}, {"name": "b", "val": 2}]}"#,
        )
        .unwrap();

        let result = doc_update(
            &file,
            "items[*].val",
            serde_json::json!(99),
            ApplyMode::Apply,
            None,
        )
        .unwrap();
        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(parsed["items"][0]["val"], serde_json::json!(99));
        assert_eq!(parsed["items"][1]["val"], serde_json::json!(99));
    }

    #[test]
    fn doc_ensure_sets_only_if_missing() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"existing": "keep"}"#).unwrap();

        // Ensure a missing key - should be added.
        let result = doc_ensure(
            &file,
            "new_key",
            serde_json::json!("added"),
            ApplyMode::Apply,
            None,
        )
        .unwrap();
        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(on_disk.contains("added"));

        // Ensure an existing key - should NOT change.
        let result = doc_ensure(
            &file,
            "existing",
            serde_json::json!("overwrite"),
            ApplyMode::Preview,
            None,
        )
        .unwrap();
        assert!(!result.changed);
    }

    #[test]
    fn doc_delete_where_removes_matching_elements() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(
            &file,
            r#"{"items": [{"name": "keep"}, {"name": "remove"}, {"name": "keep2"}]}"#,
        )
        .unwrap();

        let result =
            doc_delete_where(&file, "items", "name=remove", ApplyMode::Apply, None).unwrap();
        assert!(result.changed);
        let on_disk = fs::read_to_string(&file).unwrap();
        assert!(!on_disk.contains("remove"));
        assert!(on_disk.contains("keep"));
        assert!(on_disk.contains("keep2"));
    }

    #[test]
    fn md_move_section_reorders() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(
            &file,
            "# A\n\nBody A.\n\n# B\n\nBody B.\n\n# C\n\nBody C.\n",
        )
        .unwrap();

        // Move section A to after section C.
        let result =
            md_move_section(&file, "A", ("after", "C"), None, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        let pos_b = result.new_content.find("# B").unwrap();
        let pos_a = result.new_content.find("# A").unwrap();
        assert!(
            pos_a > pos_b,
            "A should appear after B after moving to after C"
        );
    }

    #[test]
    fn md_move_section_cross_file_writes_dest() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.md");
        let dst = dir.path().join("dst.md");
        fs::write(&src, "# Keep\n\nStay.\n\n# Move\n\nGoing.\n").unwrap();
        fs::write(&dst, "# Target\n\nHere.\n").unwrap();

        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let result = md_move_section(
            &src,
            "Move",
            ("after", "Target"),
            Some(&dst),
            ApplyMode::Apply,
            Some(&guard),
        )
        .unwrap();

        assert_eq!(result.path, src.to_string_lossy());
        let src_content = fs::read_to_string(&src).unwrap();
        let dst_content = fs::read_to_string(&dst).unwrap();
        assert!(
            !src_content.contains("# Move"),
            "section should be removed from source"
        );
        assert!(
            dst_content.contains("# Move"),
            "section should be inserted into destination"
        );
    }

    #[test]
    fn md_move_section_cross_file_rejects_guard_on_destination() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.md");
        let dst = dir.path().join("dst.md");
        fs::write(&src, "# Keep\n\nStay.\n\n# Move\n\nGoing.\n").unwrap();
        fs::write(&dst, "# Target\n\nHere.\n").unwrap();

        // Strict Reject: absolute dst should be rejected by guard before writing dest.
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let abs_dst = dst.canonicalize().unwrap_or(dst);

        let err = md_move_section(
            &src,
            "Move",
            ("after", "Target"),
            Some(&abs_dst),
            ApplyMode::Apply,
            Some(&guard),
        )
        .unwrap_err();

        assert!(err.to_string().contains("path rejected by workspace guard"));
        // src should be untouched since dest was rejected first
        let src_content = fs::read_to_string(&src).unwrap();
        assert!(src_content.contains("# Move"));
    }

    #[test]
    fn md_dedupe_headings_removes_duplicates() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(
            &file,
            "# Title\n\nBody 1.\n\n# Title\n\nBody 2.\n\n## Other\n\nKeep.\n",
        )
        .unwrap();

        let (result, removed) = md_dedupe_headings(&file, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        assert!(
            removed.iter().any(|r| r.contains("Title")),
            "should report removed duplicate heading"
        );
    }

    #[test]
    fn md_lint_agents_finds_issues() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(
            &file,
            "# Rules\n\nSome rules.\n\n# Rules\n\nDuplicate heading.\n",
        )
        .unwrap();

        let issues = md_lint_agents(&file).unwrap();
        assert!(
            !issues.is_empty(),
            "should find duplicate heading lint issue"
        );
    }

    #[test]
    fn tidy_normalizes_whitespace() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("messy.txt");
        fs::write(&file, "line1  \nline2\t \n\n\n\nline3").unwrap();

        let opts = WritePolicyOptions {
            trim_trailing_whitespace: true,
            ensure_final_newline: true,
            collapse_blanks: true,
            ..Default::default()
        };
        let result = tidy(&file, &opts, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        assert!(
            !result.new_content.contains("  \n"),
            "trailing whitespace should be removed"
        );
        assert!(
            result.new_content.ends_with('\n'),
            "should end with newline"
        );
        // Consecutive blank lines should be collapsed.
        assert!(
            !result.new_content.contains("\n\n\n"),
            "should collapse consecutive blanks"
        );
    }

    #[test]
    fn search_finds_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn alpha() {}\nfn beta() {}\nfn alpha_beta() {}\n").unwrap();

        let matches = search(&file, "alpha", false, false).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_number, 1);
        assert_eq!(matches[1].line_number, 3);
    }

    #[test]
    fn search_regex_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "version = 1\nname = test\nversion = 2\n").unwrap();

        let matches = search(&file, r"version = \d+", true, false).unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn read_full_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "line1\nline2\nline3\n").unwrap();

        let content = read(&file, None, None).unwrap();
        assert_eq!(content, "line1\nline2\nline3\n");
    }

    #[test]
    fn read_line_range() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "line1\nline2\nline3\nline4\n").unwrap();

        let content = read(&file, Some(2), Some(3)).unwrap();
        assert_eq!(content, "line2\nline3\n");
    }

    #[test]
    fn search_empty_pattern_returns_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();
        let err = search(&file, "", false, false).unwrap_err();
        assert!(
            err.to_string().contains("empty"),
            "expected empty pattern error, got: {err}"
        );
    }

    #[test]
    fn search_case_insensitive() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "Hello World\nhello world\nHELLO\n").unwrap();
        let matches = search(&file, "hello", false, true).unwrap();
        assert_eq!(
            matches.len(),
            3,
            "case-insensitive should match all 3 lines"
        );
    }

    #[test]
    fn replace_text_whole_line_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "keep this\nremove me\nalso keep\n").unwrap();

        let opts = ReplaceOptions {
            whole_line: true,
            ..Default::default()
        };
        let result = replace_text(&file, "remove", "", &opts, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        assert!(!result.new_content.contains("remove me"));
        assert!(result.new_content.contains("keep this"));
    }

    #[test]
    fn replace_text_whole_line_with_range() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "a\nb\nc\nb\ne\n").unwrap();

        let opts = ReplaceOptions {
            whole_line: true,
            range: Some((1, Some(3))),
            ..Default::default()
        };
        let result = replace_text(&file, "b", "", &opts, ApplyMode::Preview, None).unwrap();
        assert!(result.changed);
        // Only the 'b' on line 2 (within range 1:3) should be removed.
        // The 'b' on line 4 should remain.
        let lines: Vec<&str> = result.new_content.lines().collect();
        assert!(
            lines.contains(&"b"),
            "b outside range should remain: {:?}",
            lines
        );
    }

    #[test]
    fn replace_text_if_exists_no_error_on_miss() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let opts = ReplaceOptions {
            if_exists: true,
            ..Default::default()
        };
        let result = replace_text(&file, "missing", "x", &opts, ApplyMode::Preview, None).unwrap();
        assert!(!result.changed);
        assert!(!result.applied);
    }

    #[test]
    fn replace_text_range_requires_whole_line() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello\n").unwrap();

        let opts = ReplaceOptions {
            range: Some((1, Some(5))),
            ..Default::default()
        };
        let err = replace_text(&file, "hello", "x", &opts, ApplyMode::Preview, None).unwrap_err();
        assert!(err.to_string().contains("range requires whole_line"));
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
                    let result = doc_set(
                        &file,
                        "value",
                        serde_json::json!(i * 100),
                        ApplyMode::Apply,
                        None,
                    )
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
                        None,
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
                        None,
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
    fn doc_get_zero_match_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

        let err = doc_get(&file, "nonexistent").unwrap_err();
        assert!(
            err.to_string().contains("matched nothing"),
            "expected zero-match error, got: {err}"
        );
    }

    #[test]
    fn doc_get_multi_match_returns_array() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.json");
        fs::write(&file, r#"{"items": [{"name": "a"}, {"name": "b"}]}"#).unwrap();

        let value = doc_get(&file, "items[*].name").unwrap();
        assert_eq!(value, serde_json::json!(["a", "b"]));
    }

    #[test]
    fn replace_text_regex_mode() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "version = 1\nversion = 2\n").unwrap();

        let opts = ReplaceOptions {
            regex: true,
            ..ReplaceOptions::default()
        };
        let result = replace_text(
            &file,
            r"version = \d+",
            "version = 99",
            &opts,
            ApplyMode::Preview,
            None,
        )
        .unwrap();

        assert!(result.changed);
        // Regex replaces all matches
        assert_eq!(
            result.new_content.matches("version = 99").count(),
            2,
            "regex should replace all occurrences"
        );
    }

    #[test]
    fn replace_text_nth_match() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "foo bar foo baz foo\n").unwrap();

        let opts = ReplaceOptions {
            nth: Some(2),
            ..ReplaceOptions::default()
        };
        let result = replace_text(&file, "foo", "qux", &opts, ApplyMode::Preview, None).unwrap();

        assert!(result.changed);
        assert_eq!(result.new_content, "foo bar qux baz foo\n");
    }

    #[test]
    fn replace_text_insert_after() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "hello world\n").unwrap();

        let opts = ReplaceOptions {
            insert_after: Some(" beautiful".to_string()),
            ..ReplaceOptions::default()
        };
        let result = replace_text(&file, "hello", "", &opts, ApplyMode::Preview, None).unwrap();

        assert!(result.changed);
        assert!(
            result.new_content.contains("hello beautiful"),
            "insert_after should add text after match: {}",
            result.new_content
        );
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
                    let result = file_create(
                        &file,
                        &format!("content_{i}\n"),
                        false,
                        ApplyMode::Apply,
                        None,
                    )
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

    // --- api::search gap tests ---

    #[test]
    fn search_literal_returns_correct_line_numbers() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn alpha() {}\nfn beta() {}\nfn alpha_2() {}\n").unwrap();

        let matches = search(&file, "alpha", false, false).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_number, 1);
        assert_eq!(matches[1].line_number, 3);
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn search_directory_basic() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("test.rs");
        fs::write(&f, "fn foo() {}\nfn bar() {}\nlet x = foo();\n").unwrap();

        let opts = SearchOptions::default();
        let results = search_directory(dir.path(), "foo", &opts).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.line.contains("foo")));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn search_directory_with_context_and_globs() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            src.join("main.rs"),
            "fn main() { foo(); }\n// comment\nlet x = foo();\n",
        )
        .unwrap();
        std::fs::write(src.join("lib.txt"), "foo in text\n").unwrap(); // should be skipped by glob

        let opts = SearchOptions {
            context: Some(1),
            globs: vec!["*.rs".to_string()],
            ..Default::default()
        };
        let results = search_directory(dir.path(), "foo", &opts).unwrap();
        // multi-match per file now supported (addresses #779)
        assert_eq!(results.len(), 2);
        // first match "foo(); }" on line 1, no before, 1 after
        assert!(results[0].line.contains("foo()"));
        assert_eq!(results[0].context_before.len(), 0);
        assert_eq!(results[0].context_after, vec!["// comment".to_string()]);
        assert_eq!(results[0].column, 13); // position of 'f' in "foo();"
        // second match
        assert!(results[1].line.contains("foo()"));
        assert_eq!(results[1].context_before, vec!["// comment".to_string()]);
        assert_eq!(results[1].context_after.len(), 0);
        assert_eq!(results[1].column, 9);
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn search_directory_max_results_and_case() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a1.txt"), "Foo one\n").unwrap();
        std::fs::write(dir.path().join("a2.txt"), "foo two\n").unwrap();
        std::fs::write(dir.path().join("a3.txt"), "FOO three\n").unwrap();

        let opts = SearchOptions {
            case_insensitive: true,
            max_results: 2,
            ..Default::default()
        };
        let results = search_directory(dir.path(), "foo", &opts).unwrap();
        assert_eq!(results.len(), 2); // capped at 2 results (max_results limits matches)
        assert!(
            results.iter().any(|r| r.line.contains("Foo")
                || r.line.contains("foo")
                || r.line.contains("FOO"))
        );
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn search_directory_empty_pattern_errors() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("f.txt"), "content\n").unwrap();
        let opts = SearchOptions::default();
        let err = search_directory(dir.path(), "", &opts).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn search_directory_invalid_glob_errors() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("f.txt"),
            "content
",
        )
        .unwrap();
        let opts = SearchOptions {
            globs: vec!["[unclosed".to_string()],
            ..Default::default()
        };
        let err = search_directory(dir.path(), "foo", &opts).unwrap_err();
        // exact message from glob parse (exercises build_glob_matcher error path)
        assert!(err.to_string().contains("parsing glob") || err.to_string().contains("unclosed"));
    }

    #[test]
    fn search_no_match_returns_empty() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("code.rs");
        fs::write(&file, "fn hello() {}\n").unwrap();

        let matches = search(&file, "nonexistent", false, false).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn file_append_and_prepend_basic() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "hello").unwrap();

        // append
        let res = file_append(&file, " world", ApplyMode::Apply, None).unwrap();
        assert!(res.changed);
        assert!(res.applied);
        assert_eq!(res.action, "append");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n world");

        // prepend
        let res2 = file_prepend(&file, ">> ", ApplyMode::Apply, None).unwrap();
        assert!(res2.changed);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), ">> hello\n world");
    }

    #[test]
    fn file_append_preview_does_not_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("b.txt");
        std::fs::write(&file, "x").unwrap();

        let res = file_append(&file, "y", ApplyMode::Preview, None).unwrap();
        assert!(res.changed);
        assert!(!res.applied);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "x");
    }

    #[test]
    fn file_append_check_reports_without_writing() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("c.txt");
        std::fs::write(&file, "base").unwrap();

        let res = file_append(&file, " +more", ApplyMode::Check, None).unwrap();
        assert!(res.changed);
        assert!(!res.applied);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "base"); // no write
        assert!(res.new_content.contains("+more"));
    }

    #[test]
    fn file_append_respects_guard_and_relaxed() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("g.txt");
        std::fs::write(&file, "base").unwrap();

        // strict guard rejects outside? but inside ok
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let res = file_append(&file, " +append", ApplyMode::Apply, Some(&guard)).unwrap();
        assert!(res.applied);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "base\n +append");

        // relaxed yolo/temp
        let yolo = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();
        let res2 = file_append(&file, " +yolo", ApplyMode::Apply, Some(&yolo)).unwrap();
        assert!(res2.applied);
    }

    #[test]
    fn search_nonexistent_file_fails() {
        let err = search(
            Path::new("/tmp/nonexistent_patchloom_search.txt"),
            "x",
            false,
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }

    // --- api::read gap tests ---

    #[test]
    fn read_start_only_to_end() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "a\nb\nc\n").unwrap();

        let content = read(&file, Some(2), None).unwrap();
        assert_eq!(content, "b\nc\n");
    }

    #[test]
    fn read_start_beyond_file_returns_empty() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, "a\nb\n").unwrap();

        let content = read(&file, Some(100), None).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn read_nonexistent_file_fails() {
        let err = read(Path::new("/tmp/nonexistent_patchloom_read.txt"), None, None).unwrap_err();
        assert!(err.to_string().contains("failed to read"));
    }

    // --- make_write_policy CRLF variant ---

    #[test]
    fn make_write_policy_maps_crlf() {
        let opts = WritePolicyOptions {
            normalize_eol: Some(EolNormalization::Crlf),
            ..WritePolicyOptions::default()
        };
        let policy = make_write_policy(&opts);
        assert_eq!(
            policy.normalize_eol,
            crate::write::EolMode::Crlf,
            "should map EolNormalization::Crlf to EolMode::Crlf"
        );
    }
}
