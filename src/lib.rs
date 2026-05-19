pub mod cli;
pub mod cmd;
pub mod diff;
pub mod exit;
pub(crate) mod files;
pub(crate) mod ops;
pub mod plan;
pub mod selector;
pub mod write;

pub(crate) use files::*;

use clap::Parser;
use cli::Cli;

/// Run the patchloom CLI. Returns the exit code as a u8.
pub fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    let json_mode = cli.global.json;
    match cmd::dispatch(cli) {
        Ok(code) => Ok(code),
        Err(e) if json_mode => {
            let output = serde_json::json!({
                "ok": false,
                "error": format!("{e:#}")
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&output).unwrap_or_default()
            );
            Ok(exit::FAILURE)
        }
        Err(e) => Err(e),
    }
}
