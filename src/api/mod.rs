//! Public library API for embedding patchloom in Rust applications.
//!
//! This module provides a clean, CLI-independent interface to patchloom's
//! editing operations. Functions accept `&Path`/`&str` parameters and return
//! `Result<EditResult>`, with no dependency on `clap` or process arguments.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use patchloom::api::{self, ApplyMode, EditResult, ReplaceOptions};
//! use std::path::{Path, PathBuf};
//!
//! // Replace text in a file (preview only)
//! let result = api::replace_text(
//!     Path::new("src/config.rs"),
//!     "old_value",
//!     "new_value",
//!     &ReplaceOptions::default(),
//!     ApplyMode::Preview,
//!     None,
//! ).unwrap();
//! println!("diff:\n{}", result.diff);
//!
//! // Agent hosts: share one policy across primary + fallback paths (#1965)
//! let opts = ReplaceOptions::for_agent();
//! let _ = api::replace_in_content("src", "old_value", "new_value", &opts);
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
//! **Guard semantics:** The guard provides *write-time* enforcement and is only checked for `ApplyMode::Apply` writes (via `ensure_contained` + `write_if_apply`). Reads (e.g. for diff computation, `Preview`/`Check` modes, `doc_get`, search) and pre-write loads may still observe or describe paths outside the guard. This is intentional for trusted library embedding (the host/caller controls visibility). MCP uses a separate strict pre-check layer on all paths. `execute_plan` also accepts a guard and performs upfront validation on declared paths.
//!
//! ## Guard & WritePolicy contract
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
//!
//! # Text I/O and multi-doc for embedders (#1894 / #1909 / #1910)
//!
//! Prefer these entry points instead of reimplementing binary / UTF-8 probes:
//!
//! - [`load_text`] / [`crate::files::load_text_strict`]: sole-path text load;
//!   binary → `EditErrorKind::Binary`, invalid UTF-8 → `InvalidEncoding` (#1963)
//! - [`is_binary_file`]: cheap 8 KiB NUL preflight (open fail → `false`; use
//!   `load_text` when you need a typed open error)
//! - [`edit_error_kind`]: peel `TypeError`, `AlreadyExists`, `Binary`,
//!   `InvalidEncoding`, `NotFound`, `Conflicts`, etc. separately from coarse
//!   `InvalidInput` / `OperationFailed`
//! - [`error_kind_str`] / [`peel_error`]: CLI-stable kind strings for host
//!   envelopes (`already_exists`, `binary`, `invalid_encoding`, …) (#1948 / #1964)
//! - Bool peels (same chain as [`edit_error_kind`]): [`is_already_exists`],
//!   [`is_not_found`], [`is_conflicts`], [`is_changes_detected`],
//!   [`is_type_error`], [`is_format_failed`], [`is_guard_rejected`],
//!   [`is_invalid_input`], [`is_binary`], [`is_invalid_encoding`],
//!   [`is_no_match`], [`is_ambiguous`]
//! - [`file_create`] with `force: true` overwrites unreadable/binary priors
//!   without a host remove+recreate loop (#1962); hardlinks stay on the normal
//!   Apply write path
//! - [`doc_merge`]: optional `selector` for multi-doc YAML (`Some("0")`)
//!
//! ## Frozen `error_kind_str` contract for embedders (#1964)
//!
//! For a given failure, [`error_kind_str`] returns the **same** string CLI
//! `--json` puts in `error_kind`. Stable strings through 0.20 (#1963 Binary/InvalidEncoding):
//!
//! | String | Typical cause |
//! |--------|----------------|
//! | `already_exists` | create/rename dest without force |
//! | `not_found` | missing path I/O |
//! | `binary` | NUL binary probe (#1963) |
//! | `invalid_encoding` | non-UTF-8 text (#1963) |
//! | `fuzzy_span_suspicious` | over-wide fuzzy refuse (#2005) |
//! | `invalid_input` | empty path, directory target, empty pattern, unreadable IO |
//! | `guard_rejected` | PathGuard / `--contain` |
//! | `no_matches` | soft zero matches |
//! | `ambiguous` | unique multi-match |
//! | `type_error` | multi-doc / wrong-root doc nav |
//! | `conflicts` | patch merge conflict markers |
//! | `changes_detected` | check/assert-count exit 2 |
//! | `parse_error` | plan/patch/doc parse |
//! | `format_failed` | post-write format/lint |
//! | `operation_failed` | generic op failure |
//!
//! New kinds are **append-only** on [`EditErrorKind`]; hosts must keep a
//! wildcard / default arm. Prefer [`peel_error`] when you need kind + message
//! + suggestion in one call.
//!
//! ```rust,no_run
//! use patchloom::api::{self, edit_error_kind, error_kind_str, EditErrorKind, ApplyMode};
//! use std::path::Path;
//!
//! let path = Path::new("stream.yaml");
//! if api::is_binary_file(path) {
//!     // refuse before replace_text
//! }
//! match api::load_text(path) {
//!     Ok(_text) => { /* safe to treat as agent-editable text */ }
//!     Err(e) if edit_error_kind(&e) == Some(EditErrorKind::Binary) => {
//!         assert_eq!(error_kind_str(&e), Some("binary"));
//!     }
//!     Err(e) if edit_error_kind(&e) == Some(EditErrorKind::InvalidEncoding) => {
//!         assert_eq!(error_kind_str(&e), Some("invalid_encoding"));
//!     }
//!     Err(_) => {}
//! }
//! // Multi-doc merge into document 0 (not root wipe)
//! let _ = api::doc_merge(
//!     path,
//!     serde_json::json!({"c": 3}),
//!     ApplyMode::Apply,
//!     None,
//!     Some("0"),
//! );
//! // Dest-exists → AlreadyExists / "already_exists" (not InvalidInput)
//! match api::file_create(path, "x\n", false, ApplyMode::Apply, None) {
//!     Err(e) if edit_error_kind(&e) == Some(EditErrorKind::AlreadyExists) => {
//!         assert_eq!(error_kind_str(&e), Some("already_exists"));
//!     }
//!     _ => {}
//! }
//! // Force create overwrites binary prior (#1962)
//! let _ = api::file_create(path, "fresh\n", true, ApplyMode::Apply, None);
//! ```
//!
//! `EditErrorKind` is `#[non_exhaustive]`; match with a wildcard arm so new
//! honesty kinds can land in minor releases without breaking hosts.

use std::path::Path;

use crate::backup::BackupSession;
use crate::containment::PathGuard;
use crate::diff::{DiffResult, format_diff_result, unified_diff};
pub use crate::ops::patch::{Hunk, PatchFile, PatchLine};
use crate::write::{EolMode, WritePolicy, atomic_write};

#[cfg(any(feature = "cli", feature = "files"))]
pub use crate::tx::{
    TxChange, TxDocMutation, TxLintResult, TxOutput as PlanReport, TxReadResult, TxRefused,
    TxSearchMatch, TxSearchResult,
};

mod doc;
pub use self::doc::*;

/// Load a path as agent-editable UTF-8 text (sole-path **Strict** policy).
///
/// Thin alias of [`crate::files::load_text_strict`] for hosts that follow
/// `api::*` only (#1910). Binary returns [`EditErrorKind::Binary`]; invalid
/// UTF-8 returns [`EditErrorKind::InvalidEncoding`] (#1963). Directory /
/// unreadable IO stay [`EditErrorKind::InvalidInput`].
///
/// Display string in errors is the path's lossy string form.
pub fn load_text(path: &Path) -> anyhow::Result<String> {
    let display = path.to_string_lossy();
    crate::files::load_text_strict(path, &display)
}

/// Re-export of [`crate::files::is_binary_file`] for `api::*` discoverability (#1910).
pub use crate::files::is_binary_file;

/// Re-export of [`crate::files::load_text_strict`] (display string control) (#1910).
pub use crate::files::load_text_strict;

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

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
mod ast_write;
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub use self::ast_write::*;

mod content_edits;
mod fuzzy_span;
pub use self::content_edits::*;

// Constrained freeform fragment helpers (#2018); pure + plan desugar.
pub use crate::ops::apply_fragment::{
    ApplyFragmentSpec, DesugaredReplace, FragmentPlacement, build_apply_fragment_spec,
    desugar_to_replace_fields, desugar_to_replace_operation, is_lazy_marker_line,
    plan_apply_fragment_to_replace, strip_lazy_markers,
};

mod post_write;
pub use self::post_write::*;

/// How a replace operation resolved its match site (#1662).
///
/// Embedders use this for agent honesty ("applied via fuzzy — verify") and
/// optional policy (reject low-score fuzzy under `unique`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Literal/regex exact match (default path).
    Exact,
    /// Similarity-based fuzzy fallback ([`ReplaceOptions::fuzzy`]).
    Fuzzy,
    /// Disambiguated via before/after context anchors.
    Anchored,
}

/// The result of an editing operation.
///
/// Marked `non_exhaustive` so new honesty fields (for example `matched_text`)
/// can land in minor releases without breaking external struct literals.
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    /// Helps agent hosts distinguish cross-file or op type without parsing path.
    pub action: &'static str,
    /// For cross-file operations (e.g. `file_rename`, `md_move_section` with `to`),
    /// the destination path if different from `path`.
    pub dest_path: Option<String>,
    /// Number of times the search pattern matched in the original content.
    ///
    /// Only meaningful for replace operations; defaults to `0` for other
    /// operation types (doc, md, file, patch, tidy).
    pub match_count: usize,
    /// Number of items removed by the operation (keys, array elements, etc.).
    ///
    /// Meaningful for `doc.delete` and `doc.delete_where` (including
    /// idempotent no-ops where `removed == 0` and `changed == false`).
    /// Defaults to `0` for other operation types. Mirrors MCP/tx JSON
    /// mutation summaries so embedders can report delete outcomes without
    /// re-reading the file or parsing diffs (#1459 / #1439).
    pub removed: usize,
    /// Match strategy used for replace ops (`None` for non-replace ops). See #1662.
    pub match_mode: Option<MatchMode>,
    /// Similarity score when [`MatchMode::Fuzzy`] was used; otherwise `None`.
    pub match_score: Option<f64>,
    /// Text actually matched for fuzzy/anchored replace (may differ from `old`).
    ///
    /// Agents must not treat `match_mode == Fuzzy` + high `match_score` alone as
    /// "correct target": compare this field to the requested pattern and prefer
    /// `ast_rename` for identifier renames (#1736). For over-wide fuzzy spans,
    /// with `for_agent`, auto-refuse via [`ReplaceOptions::refuse_suspicious_fuzzy`];
    /// custom options still call [`fuzzy_span_suspicious`] before trust (#1981 / #2005).
    pub matched_text: Option<String>,
    /// Backup session timestamp created for this Apply (if any).
    ///
    /// Set when a write produced a backup session (see `BackupSession::finalize`).
    /// `None` for Preview/Check, no-ops, or when nothing was backed up.
    /// Use with [`crate::backup::restore_path_from_session`] for surgical undo (#1686).
    pub backup_session: Option<String>,
}

/// Result of an in-memory content edit (no file path, no applied flag).
///
/// Returned by [`replace::replace_in_content`] for callers that work on
/// in-memory buffers rather than files on disk.
///
/// Marked `non_exhaustive` so new honesty fields can land in minor releases
/// without breaking external struct literals.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ContentEditResult {
    /// The original content before the edit.
    pub original: String,
    /// The content after the edit.
    pub new_content: String,
    /// A unified diff between original and new content.
    pub diff: String,
    /// Whether the content changed.
    pub changed: bool,
    /// Number of times the search pattern matched in the original content.
    ///
    /// Populated regardless of whether replacements were applied (e.g. even
    /// when `if_exists` suppresses the error on zero matches, or when `nth`
    /// limits which match is replaced). Embedders can use this to enforce
    /// their own ambiguity policies without pre-scanning the content.
    pub match_count: usize,
    /// Match strategy for this replace (`None` when unchanged/no-match soft path).
    pub match_mode: Option<MatchMode>,
    /// Similarity score when fuzzy matching was used.
    pub match_score: Option<f64>,
    /// Text actually matched for fuzzy/anchored replace (may differ from `from`).
    /// See #1736: hosts should verify this against the requested pattern.
    pub matched_text: Option<String>,
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

/// Recommended fuzzy similarity floor for [`ReplaceOptions::for_agent`] (#1965).
///
/// Exact and anchored matches ignore the floor. Similarity/fuzzy rewrites of a
/// live span must score at least this value when hosts use the agent preset.
pub const AGENT_MIN_FUZZY_SCORE: f64 = 0.90;

/// Options for text replacement operations.
///
/// # Agent hosts (#1965)
///
/// Prefer [`Self::for_agent`] for primary and fallback replace paths so both
/// call sites share one policy. Override fields with struct update syntax
/// (`ReplaceOptions { unique: false, ..ReplaceOptions::for_agent() }` for
/// replace-all). Do **not** treat [`Default`] as agent-oriented: it keeps
/// historical soft no-match and fuzzy-off library defaults.
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
    ///
    /// **Line-oriented by default (#1885):** when the payload looks like a new
    /// line (indent, `//`/`#` comment, embedded newline) or every match of the
    /// anchor is alone on its line, a separating newline is added so the insert
    /// does not glue onto the anchor. Pure mid-line inserts (e.g. `"X"` after
    /// `"foo"` inside a line) stay byte-exact.
    pub insert_before: Option<String>,
    /// Text to insert after each match instead of replacing.
    /// Mutually exclusive with `to` (the replacement text) and `insert_before`.
    ///
    /// Same line-oriented default as [`Self::insert_before`] (#1885).
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
    /// When true, the operation fails if the pattern matches more than once.
    ///
    /// This enforces unambiguous edits: the caller is guaranteed that exactly
    /// one location was affected, or the operation is rejected with an error.
    /// Useful for AI coding agents that need to ensure each edit targets a
    /// unique location in the file.
    pub unique: bool,
    /// When true and the exact match fails (0 matches), attempt fuzzy
    /// resolution via `resolve_with_fallback` (anchor + similarity matching).
    /// On fuzzy success, the matched text is used for the replacement.
    /// On fuzzy failure, the error includes "did you mean?" suggestions.
    /// Only applies to literal (non-regex) patterns.
    pub fuzzy: bool,
    /// Context line(s) before the target for anchor-based fallback matching.
    /// When the pattern matches multiple times, the match nearest to this
    /// anchor text is selected.
    pub before_context: Option<String>,
    /// Context line(s) after the target for anchor-based fallback matching.
    /// When the pattern matches multiple times, the match nearest to this
    /// anchor text is selected.
    pub after_context: Option<String>,
    /// When true, zero matches is an error ([`EditErrorKind::NoMatch`]) instead
    /// of `Ok(changed=false)`. Agent hosts that fail closed should set this.
    /// Default `false` preserves historical library/CLI soft no-match behavior.
    /// When both this and [`Self::if_exists`] are true, `if_exists` wins
    /// (returns Ok unchanged for zero matches).
    /// See #1492.
    pub require_change: bool,
    /// When true, only replace tokens in **shell command position** (start of
    /// line / after `&&` `|` `;` / newlines, after transparent prefixes like
    /// `sudo`, `env KEY=val`, `timeout 30`, `nice -n 10`, `stdbuf`, `ionice`,
    /// `setsid`, `unshare`/`nsenter`/`taskset`/`prlimit`/`numactl`/`chrt`/
    /// `setpriv`, `busybox`, `runuser`, `flock`/`chroot` + path, and option
    /// flags like `-E` / `-p` or arg-taking `-u USER`). Does not match inside
    /// longer tokens (`pip` vs `pipenv`) or as arguments (`uv pip`). Opt-in;
    /// not the same as `word_boundary`. See #1494.
    pub command_position: bool,
    /// When fuzzy applies, reject if similarity is below this floor (e.g. `0.80`).
    /// `None` = no floor (default). Exact and anchored matches are unaffected (#1687).
    pub min_fuzzy_score: Option<f64>,
    /// When exact `old` is not present, allow Similarity/fuzzy to rewrite a nearby
    /// live span. Default `false` fails closed (#1758): refuse the write and report
    /// the best candidate. Anchored matches (explicit context) still apply.
    /// Opt in for deliberate approximate recovery (former 0.14 fuzzy default).
    pub allow_absent_old: bool,
    /// When true, refuse a successful **fuzzy** match if
    /// [`fuzzy_span_suspicious`] fires under default [`FuzzySpanPolicy`]
    /// (#2005). Returns [`EditErrorKind::FuzzySpanSuspicious`] instead of `Ok`.
    /// Default `false` for back-compat; [`Self::for_agent`] sets `true` so agent
    /// hosts cannot forget a second post-Apply check. Exact/anchored matches are
    /// not refused. Custom policy hosts keep [`fuzzy_span_suspicious_with_policy`].
    pub refuse_suspicious_fuzzy: bool,
    /// Optional post-Apply format/lint hooks (#1690 / #1663).
    ///
    /// When set, runs after a successful disk write. Prefer pairing with
    /// [`EditResult::backup_session`] for targeted revert.
    pub post_write: Option<PostWriteHooks>,
    /// Working directory for [`Self::post_write`] shell commands.
    /// Defaults to the written file's parent when unset.
    pub post_write_cwd: Option<std::path::PathBuf>,
}

impl ReplaceOptions {
    /// Shared replace policy for coding-agent hosts (primary + fallback) (#1965).
    ///
    /// Use this constructor in **every** host path that applies text replace so
    /// options cannot drift between engine and recovery. Fields:
    ///
    /// | Field | Value | Why |
    /// |-------|-------|-----|
    /// | `unique` | `true` | One unambiguous target (or error) |
    /// | `require_change` | `true` | Zero matches is [`EditErrorKind::NoMatch`], not soft success |
    /// | `fuzzy` | `true` | Enables anchor/similarity machinery (Similarity rewrite of missing `old` still needs `allow_absent_old`) |
    /// | `min_fuzzy_score` | [`AGENT_MIN_FUZZY_SCORE`] (`0.90`) | Reject weak similarity rewrites |
    /// | `allow_absent_old` | `false` | Fail closed when exact `old` is gone (#1758); report candidate |
    /// | `refuse_suspicious_fuzzy` | `true` | Auto-refuse over-wide fuzzy spans (#2005 / #1981) |
    /// | other fields | [`Default`] | Hosts opt into word_boundary, command_position, etc. |
    ///
    /// ## Overrides (struct update)
    ///
    /// ```rust
    /// use patchloom::api::ReplaceOptions;
    ///
    /// // Replace every match (not unique).
    /// let replace_all = ReplaceOptions {
    ///     unique: false,
    ///     ..ReplaceOptions::for_agent()
    /// };
    ///
    /// // Deliberate approximate recovery when exact old is absent.
    /// let recovery = ReplaceOptions {
    ///     allow_absent_old: true,
    ///     ..ReplaceOptions::for_agent()
    /// };
    ///
    /// // Exact word-boundary rename: disable fuzzy so recovery does not override `\b` misses.
    /// let word = ReplaceOptions {
    ///     fuzzy: false,
    ///     min_fuzzy_score: None,
    ///     word_boundary: true,
    ///     ..ReplaceOptions::for_agent()
    /// };
    /// # let _ = (replace_all, recovery, word);
    /// ```
    ///
    /// ## Interaction rules
    ///
    /// - `command_position` cannot combine with `fuzzy`, `word_boundary`, regex,
    ///   whole_line, multiline, nth, case_insensitive, insert_before/after, or
    ///   context anchors (see `COMMAND_POSITION_COMBO_MSG`).
    /// - `require_change` + `if_exists`: `if_exists` wins (soft zero matches).
    /// - Anchored context (`before_context` / `after_context`) still applies when
    ///   `allow_absent_old` is false.
    ///
    /// This is **not** Bline's historical host glue (`allow_absent_old: true`).
    /// That remains an explicit host override so patchloom keeps fail-closed
    /// defaults for approximate rewrites of missing text.
    #[must_use]
    pub fn for_agent() -> Self {
        Self {
            unique: true,
            require_change: true,
            fuzzy: true,
            min_fuzzy_score: Some(AGENT_MIN_FUZZY_SCORE),
            allow_absent_old: false,
            refuse_suspicious_fuzzy: true,
            ..Self::default()
        }
    }
}

pub use fuzzy_span::{FuzzySpanPolicy, fuzzy_span_suspicious, fuzzy_span_suspicious_with_policy};

// Re-export structured edit errors for embedders (#1492, #1659, #1947, #1948, #1963, #1964).
/// Re-export: binary | invalid_encoding | invalid_input from sole-path loads (#1963).
pub use crate::exit::is_load_text_strict_fail;
pub use crate::fallback::{
    EditError, EditErrorKind, PeeledError, classify_error, classify_error_ref, edit_error_kind,
    edit_error_ref, error_kind_str, find_similar_targets, is_already_exists, is_ambiguous,
    is_binary, is_changes_detected, is_conflicts, is_format_failed, is_fuzzy_span_suspicious,
    is_guard_rejected, is_invalid_encoding, is_invalid_input, is_no_match, is_not_found,
    is_type_error, peel_error,
};

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
    /// Optional post-Apply format/lint hooks for high-level writers (#1690).
    pub post_write: Option<PostWriteHooks>,
    /// Cwd for [`Self::post_write`] shell commands (default: file parent).
    pub post_write_cwd: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert user-facing `WritePolicyOptions` to the internal `WritePolicy`.
///
/// Only `tidy` currently accepts `&WritePolicyOptions` at the high-level API.
/// Other mutating functions (`file_append`, `replace_text`, `doc_*`, `md_*`, etc.) default to
/// `WritePolicy::default()`. For full control use a 1-op plan via `execute_plan`
/// (which supports per-step write_policy) or the lower-level `write` + `atomic_write` primitives.
pub fn make_write_policy(opts: &WritePolicyOptions) -> WritePolicy {
    WritePolicy {
        ensure_final_newline: opts.ensure_final_newline,
        normalize_eol: opts.normalize_eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: opts.trim_trailing_whitespace,
        collapse_blanks: opts.collapse_blanks,
    }
}

/// Generate a unified diff between two in-memory strings.
///
/// Returns an empty string when the contents are identical.
/// The `path` parameter is used for the `--- a/` and `+++ b/` diff headers;
/// pass `None` to use a generic `<content>` placeholder.
///
/// Absolute Unix paths (e.g. after `canonicalize()`) are normalized so headers
/// do not contain a double slash (`--- a//tmp/...`): leading `/` characters are
/// stripped for the header only. Relative paths are unchanged
/// (`--- a/src/main.rs`).
///
/// This is the same diff engine used internally by [`replace_in_content`],
/// [`replace_text`], and other editing operations, exposed as a standalone
/// public API for embedders that need to diff arbitrary strings without
/// going through a full edit operation.
pub fn text_diff(original: &str, modified: &str, path: Option<&str>) -> String {
    make_diff(path.unwrap_or("<content>"), original, modified)
}

/// Parse unified diff text into structured patch files and hunks.
///
/// Handles standard unified diff format (`--- a/` / `+++ b/` / `@@`).
/// Tolerant of embedded diffs in prose (only recognizes headers with
/// `a/`/`b/` prefixes, `/dev/null`, tab timestamps, or `diff ` context).
///
/// Returns one [`PatchFile`] per file in the diff, each containing
/// [`Hunk`]s with [`PatchLine`]s for context, added, and removed lines.
///
/// This complements [`text_diff`] (which generates diffs) and
/// `apply_patch` (which applies diffs to files) by providing a
/// parse-only step for embedders that need structured diff data
/// without applying it.
///
/// # Errors
///
/// Returns an error if the diff text contains malformed hunk headers
/// or is otherwise unparseable.
pub fn parse_unified_diff(text: &str) -> Result<Vec<PatchFile>, String> {
    crate::ops::patch::parse_patch(text)
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
/// Returns `(applied, backup_session)`.
pub(crate) fn apply_mutation(
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    prepare_backup: impl FnOnce(&mut BackupSession) -> anyhow::Result<()>,
    perform_mutation: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<(bool, Option<String>)> {
    if mode != ApplyMode::Apply {
        return Ok((false, None));
    }
    ensure_contained(guard, path)?;
    // Use the project root (parent of the file) as backup root.
    // For library users, backup is best-effort.
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    prepare_backup(&mut backup)?;
    perform_mutation()?;
    let session = backup.finalize()?;
    Ok((true, session))
}

/// Generalized cross-file mutation helper (for rename and md cross-file moves).
///
/// Handles guard checks and backup for src (and optional dst).
/// Centralizes the cross-file guard and backup logic.
/// Returns `(applied, backup_session)`.
fn apply_cross_file_mutation(
    src: &Path,
    dst: Option<&Path>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    prepare_backup: impl FnOnce(&mut BackupSession) -> anyhow::Result<()>,
    perform_mutation: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<(bool, Option<String>)> {
    if mode != ApplyMode::Apply {
        return Ok((false, None));
    }
    ensure_contained(guard, src)?;
    if let Some(d) = dst {
        ensure_contained(guard, d)?;
    }
    let cwd = src.parent().unwrap_or_else(|| Path::new("."));
    let mut backup = BackupSession::new(cwd)?;
    prepare_backup(&mut backup)?;
    perform_mutation()?;
    let session = backup.finalize()?;
    Ok((true, session))
}

/// Write content on Apply. Returns `(applied, backup_session)`.
pub(crate) fn write_if_apply(
    path: &Path,
    new_content: &str,
    mode: ApplyMode,
    policy: &WritePolicy,
    guard: Option<&PathGuard>,
) -> anyhow::Result<(bool, Option<String>)> {
    apply_mutation(
        path,
        mode,
        guard,
        |backup| backup.save_before_write(path),
        || atomic_write(path, new_content, policy),
    )
}

/// Run optional post-write hooks after a successful Apply (#1690).
pub(crate) fn maybe_post_write(
    applied: bool,
    path: &Path,
    hooks: Option<&PostWriteHooks>,
    hooks_cwd: Option<&Path>,
    backup_session: Option<&str>,
) -> anyhow::Result<()> {
    if !applied {
        return Ok(());
    }
    let Some(hooks) = hooks else {
        return Ok(());
    };
    let root = hooks_cwd.unwrap_or_else(|| path.parent().unwrap_or_else(|| Path::new(".")));
    if let Some(ts) = backup_session {
        run_post_write_validation_with_session(root, path, hooks, Some(ts))
    } else {
        run_post_write_validation(root, path, hooks)
    }
}

/// Private helper to centralize the guard check and eliminate duplicated
/// inline `if let Some(g) = guard { g.check_path... }` blocks in every
/// write path.
pub(crate) fn ensure_contained(guard: Option<&PathGuard>, path: &Path) -> anyhow::Result<()> {
    if let Some(g) = guard {
        g.check_path(&path.to_string_lossy())
            .map_err(crate::fallback::EditError::guard_rejected)?;
    }
    Ok(())
}

pub(crate) fn build_edit_result(
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
        match_count: 0,
        removed: 0,
        match_mode: None,
        match_score: None,
        matched_text: None,
        backup_session: None,
    }
}

/// Map fallback match strategy to public [`MatchMode`] (#1662).
pub(crate) fn match_mode_from_strategy(
    strategy: crate::fallback::MatchStrategy,
) -> (MatchMode, Option<f64>) {
    use crate::fallback::MatchStrategy;
    match strategy {
        MatchStrategy::Exact => (MatchMode::Exact, None),
        MatchStrategy::Anchor => (MatchMode::Anchored, None),
        MatchStrategy::Similarity => (MatchMode::Fuzzy, None),
    }
}

/// Worst-case match honesty rollup: fuzzy > anchored > exact (#1673 / #1674).
///
/// Shared by multi-op content edits, plan/tx aggregates, and replace_op path
/// re-visits so every surface reports the same confidence ordering.
///
/// Public for embedder hosts that combine multiple [`MatchMode`] values from
/// multi-op content edits (#1844).
pub fn merge_match_modes(prev: Option<MatchMode>, next: MatchMode) -> MatchMode {
    match (prev, next) {
        (Some(MatchMode::Fuzzy), _) | (_, MatchMode::Fuzzy) => MatchMode::Fuzzy,
        (Some(MatchMode::Anchored), _) | (_, MatchMode::Anchored) => MatchMode::Anchored,
        _ => MatchMode::Exact,
    }
}

/// Prefer the wider matched span by Unicode scalar count (#1981 / #2007).
///
/// Used for content_edits and plan/tx multi-op rollups so hosts see the
/// worst-case over-wide span (not first-non-null).
#[must_use]
pub fn prefer_widest_matched_text(prev: Option<String>, next: Option<String>) -> Option<String> {
    match (prev, next) {
        (None, n) => n,
        (p, None) => p,
        (Some(p), Some(n)) => {
            if n.chars().count() > p.chars().count() {
                Some(n)
            } else {
                Some(p)
            }
        }
    }
}

#[cfg(test)]
mod prefer_widest_tests {
    use super::prefer_widest_matched_text;

    #[test]
    fn prefer_widest_matched_text_picks_longer_unicode_span() {
        assert_eq!(
            prefer_widest_matched_text(None, Some("ab".into())).as_deref(),
            Some("ab")
        );
        assert_eq!(
            prefer_widest_matched_text(Some("short".into()), Some("much_longer".into())).as_deref(),
            Some("much_longer")
        );
        assert_eq!(
            prefer_widest_matched_text(Some("keep".into()), Some("x".into())).as_deref(),
            Some("keep")
        );
        // Equal length keeps previous.
        assert_eq!(
            prefer_widest_matched_text(Some("abc".into()), Some("xyz".into())).as_deref(),
            Some("abc")
        );
    }
}

// ---------------------------------------------------------------------------
// TX engine adapter (requires tx module: cli or files feature)
// ---------------------------------------------------------------------------

/// Execute a single `Operation` through the tx engine and return an `EditResult`.
///
/// This is the bridge between the library API (which uses `ApplyMode` and returns
/// `EditResult`) and the tx engine (which uses `GlobalFlags` and returns
/// `ExecutionResult`). All API write functions can delegate to this adapter
/// instead of reimplementing read-transform-write-backup independently.
///
/// For cross-file operations (rename, move) pass `dest_path` to report the
/// destination; single-file operations pass `None`.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn execute_as_edit_result(
    op: crate::plan::Operation,
    mode: ApplyMode,
    cwd: &Path,
    guard: Option<&PathGuard>,
    action: &'static str,
    dest_path: Option<String>,
) -> anyhow::Result<EditResult> {
    let global = mode_to_global_flags(mode);
    let options = crate::tx::engine::ExecuteOptions::from_global(cwd, &global, guard);
    let result = crate::tx::engine::stage(crate::tx::engine::WriteRequest {
        source: crate::tx::engine::WriteSource::Operations(vec![op]),
        options,
    })?;
    execution_result_to_edit_result(result, mode, cwd, action, dest_path)
}

/// Map `ApplyMode` to `GlobalFlags` with the appropriate apply/check settings.
#[cfg(any(feature = "cli", feature = "files"))]
fn mode_to_global_flags(mode: ApplyMode) -> crate::cli::global::GlobalFlags {
    let mut flags = crate::cli::global::GlobalFlags::default();
    match mode {
        ApplyMode::Apply => flags.apply = true,
        ApplyMode::Check => flags.check = true,
        ApplyMode::Preview => {} // default: no apply, no check
    }
    flags
}

/// Convert an `ExecutionResult` into an `EditResult`.
///
/// Handles commit for Apply mode, extracts per-file data from the engine result.
#[cfg(any(feature = "cli", feature = "files"))]
fn execution_result_to_edit_result(
    result: crate::tx::engine::ExecutionResult,
    mode: ApplyMode,
    cwd: &Path,
    action: &'static str,
    dest_path: Option<String>,
) -> anyhow::Result<EditResult> {
    let has_changes = result.has_changes;
    let removed: usize = result
        .exec_result
        .tx_mutations
        .iter()
        .map(|m| m.removed)
        .sum();
    // Prefer mutation path when the engine recorded a doc delete/delete-where
    // outcome (including idempotent no-ops with empty `changes`).
    let mutation_path = result
        .exec_result
        .tx_mutations
        .first()
        .map(|m| m.path.clone());

    // Extract path + content + replace meta before potentially consuming `result`.
    let replace_meta = result
        .exec_result
        .changes
        .first()
        .and_then(|(abs_path, _, _)| result.exec_result.replace_match_meta.get(abs_path).cloned());
    let (path_str, original, new_content) =
        if let Some((abs_path, orig, new)) = result.exec_result.changes.first() {
            let rel = crate::files::relative_display(abs_path, cwd);
            (rel.to_string_lossy().to_string(), orig.clone(), new.clone())
        } else if let Some(abs_path) = result.exec_result.deletions.iter().next() {
            // File deletion: content is in the pending map.
            let rel = crate::files::relative_display(abs_path, cwd);
            let original = result
                .exec_result
                .pending
                .get(abs_path)
                .map(|(orig, _)| orig.clone())
                .unwrap_or_default();
            (rel.to_string_lossy().to_string(), original, String::new())
        } else if let Some(p) = mutation_path {
            // No file content change (e.g. idempotent doc.delete) but mutations
            // still carry path + removed counts for embedders (#1459).
            (p, String::new(), String::new())
        } else {
            // No changes at all (e.g., replace with no matches + if_exists).
            (String::new(), String::new(), String::new())
        };

    // Commit for Apply mode; capture backup session for embedder undo (#1686).
    let (applied, backup_session) = if mode == ApplyMode::Apply && has_changes {
        let session = result.commit()?;
        (true, session)
    } else {
        (false, None)
    };

    let mut edit = build_edit_result(&path_str, original, new_content, applied, action, dest_path);
    edit.removed = removed;
    edit.backup_session = backup_session;
    // Thread replace honesty from the engine (#1674 / #1662 / #1736).
    if let Some(meta) = replace_meta {
        edit.match_mode = Some(meta.mode);
        edit.match_score = meta.score;
        edit.match_count = meta.match_count;
        edit.matched_text = meta.matched_text;
    } else if has_changes && action == "replace" {
        // Legacy fallback if meta was not recorded (should be rare).
        edit.match_mode = Some(MatchMode::Exact);
        if edit.match_count == 0 {
            edit.match_count = 1;
        }
    }
    Ok(edit)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
