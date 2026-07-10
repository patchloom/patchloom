#![deny(unsafe_code)]
//! Patchloom: agent-grade repo operations as a Rust library.
//!
//! This crate provides both a CLI binary and a library API for structured
//! file editing operations. The [`api`] module is the main entry point for
//! library consumers.
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `cli`   | **yes** | CLI parser (clap) and all subcommand implementations. Disable for pure library use. |
//! | `mcp`   | **yes** | MCP server support (adds `tokio`, `rmcp`, `schemars`) |
//! | `ast`   | **yes** | AST-aware operations using tree-sitter (20 language grammars) |
//! | `files` | no      | File scanning helpers + library plan execution + search_directory + append etc. for pure-library use (no CLI/clap). |
//! | `full`  | no      | Everything: `cli` + `mcp` + `ast` |
//!
//! ## Embedding as a library
//!
//! To use patchloom as a library (no CLI, no MCP):
//!
//! ```toml
//! [dependencies]
//! patchloom = { default-features = false }
//! ```
//!
//! Or with AST support:
//!
//! ```toml
//! patchloom = { default-features = false, features = ["ast"] }
//! ```
//!
//! (Update the version number in these examples when the next release-please
//! PR bumps the crate version. See the release checklist.)
//!
//! This gives you the [`api`] module (primary editing interface), [`ops`],
//! and utility modules:
//!
//! - [`containment`] -- workspace path guard (flexible `AbsolutePathPolicy` via builder for temp dirs/extra roots in library use; strict `Reject` for MCP)
//! - [`exec`] -- shell command execution with process-tree management
//! - [`fallback`] -- multi-strategy edit recovery (exact, anchor, similarity)
//! - [`files`] -- `is_binary`, `read_text_file`, and (with "files" feature) parallel scanning helpers
//! - [`write`] -- atomic file writes with write-policy transformations
//!
//! With "files" feature you also get `api::search_directory`, `api::execute_plan`,
//! `api::file_append`/`file_prepend`, and full plan execution for library use.
//! For advanced search ignore (e.g. .blineignore on top of .gitignore) + custom walkers:
//! Use `SearchOptions::exclude_patterns` and `custom_ignore_filenames` with `search_directory`/`search_file`,
//! or collect paths with `files::collect_file_paths_with_ignores` (or your own `WalkBuilder`) then
//! pair with the low-level `api::search_one_file` inside `par_process_files` + `format_search_results` / `build_context_lines`.
//! See `api::search_one_file` (and its docs for custom `WalkBuilder` use), `api::SearchOptions`, and `files` module.
//!
//! Example (pure library with plans):
//! ```rust,ignore
//! use patchloom::api::{execute_plan, parse_plan, ApplyMode, file_append};
//! use patchloom::containment::PathGuard;
//! use std::path::Path;
//!
//! let guard = PathGuard::builder(std::env::current_dir().unwrap())
//!     .allow_temp_directory()
//!     .build()?;
//!
//! // Simple append via api
//! let _ = file_append(Path::new("log.txt"), "entry\n", ApplyMode::Apply, Some(&guard))?;
//!
//! // Or via plan for atomic multi-op
//! let plan_json = r#"{"version":1,"ops":[{"op":"file.append","path":"log.txt","content":"more\n"}]}"#;
//! let plan = parse_plan(plan_json)?;
//! let report = execute_plan(plan, Path::new("."), Some(&guard))?;
//! assert!(report.ok);
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! For AST signature edits (bline #1459 / #821 follow-through, #1493):
//!
//! - In-memory: `api::ast_rewrite_signature_in_content` or
//!   `ast::rewrite::rewrite_function_signature` with `FunctionSigEdit`
//! - Parse Rust fragments: `FunctionSigEdit::parse_rust("pub fn f(x: i32) -> T")`
//! - Full-string rewrites accept a logical signature without trailing space;
//!   high-level helpers preserve the body gap before `{` (`splice_function_signature`, #1503)
//! - On disk: `api::ast_rewrite_signature`, `api::ast_rename`, `api::ast_replace_in_symbol`
//! - Multi-file rename: `api::ast_rename_batch` (same-file serialization, per-file results; #1495)
//! - Plans / MCP: op `ast.rewrite_signature` / tool `ast_rewrite_signature`
//!
//! CLI `ast rewrite-signature` is still optional; library + plan + MCP cover embedders.
//!
//! Fail-closed text edits for agent hosts (#1492): set `ReplaceOptions.require_change = true`
//! so zero matches become `EditErrorKind::NoMatch` (not `Ok(changed=false)`). Match kinds via
//! `api::edit_error_kind(&err)` without scraping English. Example:
//!
//! ```rust,no_run
//! use patchloom::api::{self, ReplaceOptions, edit_error_kind, EditErrorKind};
//! let opts = ReplaceOptions { require_change: true, ..Default::default() };
//! match api::replace_in_content("a b", "missing", "x", &opts) {
//!     Ok(r) => assert!(r.changed),
//!     Err(e) => assert_eq!(edit_error_kind(&e), Some(EditErrorKind::NoMatch)),
//! }
//! ```
//!
//! Shell command-position matching (#1494): opt-in `ReplaceOptions.command_position`
//! rewrites invocable tokens (`pip install`) without touching arguments (`uv pip`) or
//! longer words (`pipenv`). Not the same as `word_boundary`. Post-Apply validate/revert:
//! host runs a validator, then `backup::restore_path_from_latest_backup(project_root, path)`.
//!
//! For several ordered text edits on **one buffer** then a single write (agent intent engines):
//! use `api::apply_content_edits` / `api::apply_content_edits_to_file` with
//! `ContentEdit::{Replace, InsertBefore, InsertAfter, Append, Prepend}` (all-or-nothing).
//! Multi-file multi-op remains `execute_plan`.
//!
//! **Note on results**: Single-file ops return `EditResult` (with `action`, `dest_path`,
//! `match_count` for replace, and `removed` for `doc.delete` / `doc.delete_where`).
//! `execute_plan` (library) returns `PlanReport` (typed TxOutput) with `ok`, `changes`,
//! `searches`, `reads`, `error`, plus `mutations` / aggregate `changed` / `removed` for
//! deletes (including idempotent `removed: 0` no-ops) (#811, #1439, #1459).
//! See `api::PlanReport`, `api::execute_plan`, and embedding docs. CLI/MCP retain (code, json) for compatibility.
//!
//! For library users needing relaxed containment (e.g. agents like Bline using --yolo or temp files):
//! ```rust,no_run
//! use patchloom::containment::PathGuard;
//! let guard = PathGuard::builder(std::env::current_dir().unwrap())
//!     .allow_temp_directory()  // includes /tmp and handles macOS /tmp -> /private/tmp
//!     .build()
//!     .expect("guard");
//! // pass to high-level api functions, e.g.
//! let _ = patchloom::api::replace_text(
//!     std::path::Path::new("foo.txt"),
//!     "old",
//!     "new",
//!     &patchloom::api::ReplaceOptions::default(),
//!     patchloom::api::ApplyMode::Preview,
//!     Some(&guard),
//! );
//! ```
//!
//! The `files` module (pure helpers like `is_binary`, `read_text_file`, and scanning tools when "files" feature enabled) is always available.
//! The `cli` and `cmd` modules require the `cli` feature.
//!
//! For pure library use with plans and execution (post #792), prefer
//! `features = ["ast", "files"]` (or "files"). `execute_plan` is available
//! under `any(feature = "cli", "files")` and delegates to the `tx` module.
//!
//! ## Migration for high-level api::* signature changes (PathGuard, #758)
//!
//! The addition of the trailing `guard: Option<&PathGuard>` parameter to all mutating
//! functions (replace_text, doc_*, md_*, file_*, tidy, apply_patch, etc.) and to
//! `execute_plan` is a source-breaking change from pre-#749 usage.
//!
//! ```rust,ignore
//! // Before
//! patchloom::api::doc_set(&p, "k", v, ApplyMode::Apply)?;
//!
//! // After (pass None to keep previous strict-root behavior, or a guard for relaxed)
//! patchloom::api::doc_set(&p, "k", v, ApplyMode::Apply, None)?;
//! ```
//!
//! See the "Using with PathGuard" section in the `api` module docs, the builder
//! for relaxed policies, and AGENTS.md "High-level library API signature changes"
//! for the full checklist (doctests, greps, examples, tests). `execute_plan` now
//! also accepts the guard (threaded into tx; #755).
//!
//! With `features = ["ast"]`, the [`ast`] module provides tree-sitter parsing,
//! symbol extraction, structural search, rename, and more for 20 languages.
//!
//! No `clap`, `tokio` or other heavy dependencies are pulled in when `cli` and `mcp` are disabled.
//!
//! ## Thread safety
//!
//! All public API types ([`api::EditResult`], [`api::ApplyMode`], etc.) are
//! `Send + Sync`. Library functions are safe to call concurrently from
//! multiple threads with one constraint:
//!
//! - **Different files**: fully safe. Multiple threads can edit different files
//!   simultaneously with no coordination.
//! - **Same file**: the caller must serialize access. Concurrent writes to the
//!   same file are inherently racy (last writer wins). Use a mutex or other
//!   synchronization if you need to coordinate edits to a single file.
//!
//! Backup sessions use unique directory names (nanosecond timestamp +
//! monotonic counter) so concurrent backup creation never collides.
//!
//! Configuration can be loaded once with [`config::CachedConfig`] and reused
//! across threads, avoiding repeated disk reads.

pub mod api;
#[cfg(feature = "ast")]
pub mod ast;
pub mod backup;
pub mod cli;
#[cfg(feature = "cli")]
pub mod cmd;
pub mod config;
pub mod containment;
pub(crate) mod diff;
pub mod exec;
pub(crate) mod exit;
pub mod fallback;
pub mod files;
pub mod ops;
pub mod plan;
pub mod schema;
pub mod selector;
pub mod write;

// Re-exports for library ergonomics (no need to dig into api/plan when using ["ast","files"]).
#[cfg(any(feature = "cli", feature = "files"))]
pub use api::search_one_file;
pub use api::{
    ApplyMode, ContentEditResult, EditError, EditErrorKind, EditResult, Hunk, PatchFile, PatchLine,
    ReplaceOptions, SearchOptions, SearchResult, WritePolicyOptions, build_context_lines,
    edit_error_kind, edit_error_ref, format_search_results, parse_unified_diff, search_file,
    text_diff,
};
pub use plan::Plan;

#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) mod tx;

#[cfg(feature = "cli")]
pub(crate) use files::*;

// ---------------------------------------------------------------------------
// Verbose logging
// ---------------------------------------------------------------------------

/// Global flag set once at startup; checked by the `verbose!` macro.
static VERBOSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Returns `true` if verbose mode is enabled.
pub fn is_verbose() -> bool {
    VERBOSE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Enable verbose mode globally. Called once at startup.
#[cfg_attr(not(feature = "cli"), allow(dead_code))]
fn enable_verbose() {
    VERBOSE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Print a verbose diagnostic message to stderr.
///
/// Usage: `verbose!("processing {} files", count);`
#[macro_export]
macro_rules! verbose {
    ($($arg:tt)*) => {
        if $crate::is_verbose() {
            eprintln!("[patchloom] {}", format!($($arg)*));
        }
    };
}

// ---------------------------------------------------------------------------
// Bounded regex compilation
// ---------------------------------------------------------------------------

/// Create a [`regex::RegexBuilder`] with bounded compilation limits.
///
/// All user-supplied regex patterns must go through this function so that
/// pathological patterns cannot exhaust memory (important when patchloom
/// runs as an MCP server handling untrusted input).
pub fn bounded_regex_builder(pattern: &str) -> regex::RegexBuilder {
    let mut b = regex::RegexBuilder::new(pattern);
    // 10 MiB compiled program + DFA cache limits.
    b.size_limit(10 * 1024 * 1024);
    b.dfa_size_limit(10 * 1024 * 1024);
    b
}

/// Run the patchloom CLI. Returns the exit code as a u8.
///
/// Requires the `cli` feature (enabled by default).
#[cfg(feature = "cli")]
pub fn run() -> anyhow::Result<u8> {
    use clap::Parser;

    let cli = crate::cli::Cli::parse();

    // Enable verbose mode from --verbose flag or PATCHLOOM_LOG env var.
    if cli.global.verbose || std::env::var_os("PATCHLOOM_LOG").is_some() {
        enable_verbose();
    }

    let structured = cli.global.json || cli.global.jsonl;
    let compact = cli.global.jsonl;
    match cmd::dispatch(cli) {
        Ok(code) => Ok(code),
        Err(e) if structured => {
            let output = serde_json::json!({
                "ok": false,
                "error": format!("{e:#}")
            });
            let serialized = if compact {
                serde_json::to_string(&output)
            } else {
                serde_json::to_string_pretty(&output)
            };
            println!("{}", serialized.unwrap_or_default());
            Ok(exit::FAILURE)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_regex_builder_compiles_valid_pattern() {
        let re = bounded_regex_builder(r"\d+").build().unwrap();
        assert!(re.is_match("abc123"));
    }

    #[test]
    fn bounded_regex_builder_rejects_invalid_pattern() {
        bounded_regex_builder(r"(unclosed")
            .build()
            .expect_err("expected error");
    }

    #[test]
    fn bounded_regex_builder_applies_size_limit() {
        // Verify that our builder applies size limits by comparing
        // with a manually-limited builder. A small pattern with a
        // 1-byte limit must fail, proving the mechanism works.
        let mut tiny = regex::RegexBuilder::new(r"\d+");
        tiny.size_limit(1);
        assert!(tiny.build().is_err(), "1-byte limit should reject any NFA");

        // Our builder should compile normal patterns just fine.
        bounded_regex_builder(r"\d+")
            .build()
            .expect("simple pattern should compile");

        // Verify the builder actually sets size_limit (not just dfa_size_limit)
        // by building a moderately large pattern that fits 10 MiB.
        let medium: String = (0..1000)
            .map(|i| format!("word_{i}"))
            .collect::<Vec<_>>()
            .join("|");
        bounded_regex_builder(&medium)
            .build()
            .expect("1K-alternation pattern should compile within 10 MiB limit");
    }
}
