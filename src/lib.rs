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
//! | `files` | no      | File scanning helpers (`is_binary`, `read_text_file`, `par_process_files`, globs, walker) for library search etc. |
//! | `full`  | no      | Everything: `cli` + `mcp` + `ast` |
//!
//! ## Embedding as a library
//!
//! To use patchloom as a library (no CLI, no MCP):
//!
//! ```toml
//! [dependencies]
//! patchloom = { version = "0.3", default-features = false }
//! ```
//!
//! Or with AST support:
//!
//! ```toml
//! patchloom = { version = "0.3", default-features = false, features = ["ast"] }
//! ```
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
//! With "files" feature you also get `api::search_directory` for content search.
//!
//! For library users needing relaxed containment (e.g. agents like Bline using --yolo or temp files):
//! ```rust,no_run
//! use patchloom::containment::PathGuard;
//! let guard = PathGuard::builder(std::env::current_dir().unwrap())
//!     .allow_temp_directory()
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
