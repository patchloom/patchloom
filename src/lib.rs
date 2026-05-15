pub mod cli;
pub mod cmd;
pub mod diff;
pub mod error;
pub mod exit;
pub mod plan;
pub mod selector;
pub mod write;

use clap::Parser;
use cli::Cli;

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB, the same heuristic Git uses).
pub(crate) fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Run the patchloom CLI. Returns the exit code as a u8.
pub fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    cmd::dispatch(cli)
}
