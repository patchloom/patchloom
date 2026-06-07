#![deny(unsafe_code)]

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
