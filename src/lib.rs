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
//! | `core`  | no      | Marker feature for core library usage (no extra deps) |
//! | `mcp`   | **yes** | MCP server support (adds `tokio`, `rmcp`, `schemars`) |
//! | `full`  | no      | Everything: currently equivalent to `mcp` |
//!
//! ## Embedding as a library
//!
//! To use patchloom as a library without the MCP server overhead:
//!
//! ```toml
//! [dependencies]
//! patchloom = { version = "0.1", default-features = false }
//! ```
//!
//! This gives you the full [`api`] module (doc/replace/md/file/patch operations)
//! and the [`ops`] module for lower-level access, without pulling in `tokio`
//! or other async runtime dependencies.

pub mod api;
pub mod backup;
pub mod cli;
pub mod cmd;
pub mod config;
pub mod diff;
pub mod exit;
pub(crate) mod files;
pub mod ops;
pub mod plan;
pub mod selector;
pub mod write;

pub(crate) use files::*;

use clap::Parser;
use cli::Cli;

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
pub fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();

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
