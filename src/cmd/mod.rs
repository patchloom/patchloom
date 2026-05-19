pub mod batch;
pub mod create;
pub mod delete;
pub mod doc;
pub mod hygiene;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod md;
pub mod patch;
pub mod read;
pub mod rename;
pub mod replace;
pub mod search;
pub mod status;
pub mod tx;

use crate::cli::Cli;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new file with specified content.
    Create(create::CreateArgs),
    /// Delete a file.
    Delete(delete::DeleteArgs),
    /// Fast literal or regex search across a repo.
    Search(search::SearchArgs),
    /// Mechanical string replacement with diff preview.
    Replace(replace::ReplaceArgs),
    /// Show which files have uncommitted changes.
    Status(status::StatusArgs),
    /// Preview or apply unified diffs safely.
    Patch(patch::PatchArgs),
    /// Read file contents with optional line range.
    Read(read::ReadArgs),
    /// Rename (move) a file.
    Rename(rename::RenameArgs),
    /// Markdown section-aware operations.
    Md(md::MdArgs),
    /// Parser-backed JSON, YAML, and TOML operations.
    Doc(doc::DocArgs),
    /// Final newline, line ending, and whitespace normalization.
    Hygiene(hygiene::HygieneArgs),
    /// Execute a multi-operation plan atomically.
    Tx(tx::TxArgs),
    /// Execute multiple operations from a simple line-oriented format.
    Batch(batch::BatchArgs),
    /// Print agent rules for using patchloom (AGENTS.md content for end users).
    AgentRules,
    /// Start an MCP (Model Context Protocol) server on stdio.
    #[cfg(feature = "mcp")]
    McpServer,
    /// Generate shell completions for bash, zsh, fish, or elvish.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

pub fn dispatch(cli: Cli) -> anyhow::Result<u8> {
    let mut global = cli.global;
    match cli.command {
        #[cfg(feature = "mcp")]
        Command::McpServer => mcp::run_mcp_server(),
        Command::AgentRules => {
            let version = env!("CARGO_PKG_VERSION");
            let template = include_str!("../agent_rules.md");
            let output = template.replace("{{VERSION}}", version);
            print!("{output}");
            Ok(crate::exit::SUCCESS)
        }
        Command::Completions { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "patchloom", &mut std::io::stdout());
            Ok(crate::exit::SUCCESS)
        }
        Command::Read(args) => read::run(args, &global),
        Command::Search(args) => search::run(args, &global),
        Command::Status(args) => status::run(args, &global),
        Command::Create(args) => {
            global.merge_write(&args.write);
            create::run(args, &global)
        }
        Command::Delete(args) => {
            global.merge_write(&args.write);
            delete::run(args, &global)
        }
        Command::Rename(args) => {
            global.merge_write(&args.write);
            rename::run(args, &global)
        }
        Command::Replace(args) => {
            global.merge_write(&args.write);
            replace::run(args, &global)
        }
        Command::Patch(args) => {
            global.merge_write(&args.write);
            patch::run(args, &global)
        }
        Command::Md(args) => {
            global.merge_write(&args.write);
            md::run(args, &global)
        }
        Command::Doc(args) => {
            global.merge_write(&args.write);
            doc::run(args, &global)
        }
        Command::Hygiene(args) => {
            global.merge_write(&args.write);
            hygiene::run(args, &global)
        }
        Command::Tx(args) => {
            global.merge_write(&args.write);
            tx::run(args, &global)
        }
        Command::Batch(args) => {
            global.merge_write(&args.write);
            batch::run(args, &global)
        }
    }
}
