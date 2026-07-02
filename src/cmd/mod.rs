pub mod agent_rules;
pub mod append;
#[cfg(feature = "ast")]
pub mod ast;
pub mod batch;
pub mod create;
pub mod delete;
pub mod doc;
pub mod explain;
pub mod init;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod md;
pub mod output;
pub mod patch;
pub mod prepend;
pub mod read;
pub mod rename;
pub mod replace;
pub mod schema;
pub mod search;
pub mod status;
pub mod tidy;
pub mod tx;
pub mod undo;
pub mod write_dispatch;

use crate::cli::Cli;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum Command {
    // -- File Operations (display_order 10-19) --
    /// Append content to an existing file.
    #[command(display_order = 10)]
    Append(append::AppendArgs),
    /// Create a new file with specified content.
    #[command(display_order = 11)]
    Create(create::CreateArgs),
    /// Delete a file.
    #[command(display_order = 12)]
    Delete(delete::DeleteArgs),
    /// Prepend content to the beginning of an existing file.
    #[command(display_order = 11)]
    Prepend(prepend::PrependArgs),
    /// Read file contents with optional line range.
    #[command(display_order = 13)]
    Read(read::ReadArgs),
    /// Rename (move) a file.
    #[command(display_order = 14)]
    Rename(rename::RenameArgs),

    // -- Text Operations (display_order 20-29) --
    /// Fast literal or regex search across text files.
    #[command(display_order = 20)]
    Search(search::SearchArgs),
    /// Mechanical string replacement across text files with diff preview.
    #[command(display_order = 21)]
    Replace(replace::ReplaceArgs),
    /// Preview or apply unified diffs safely.
    #[command(display_order = 22)]
    Patch(patch::PatchArgs),
    /// Text-file newline, line ending, and whitespace normalization.
    #[command(display_order = 23)]
    Tidy(tidy::TidyArgs),
    /// Execute multiple operations from a simple line-oriented format.
    #[command(display_order = 24)]
    Batch(batch::BatchArgs),

    // -- Structured Data (display_order 30-39) --
    /// Parser-backed JSON, YAML, and TOML operations.
    #[command(display_order = 30)]
    Doc(doc::DocArgs),
    /// Markdown section-aware operations.
    #[command(display_order = 31)]
    Md(md::MdArgs),

    // -- AST Operations (display_order 40-49) --
    /// AST-aware operations: list, read, rename, validate, search, refs, deps, map, replace, impact, diff.
    #[cfg(feature = "ast")]
    #[command(display_order = 40)]
    Ast(ast::AstArgs),

    // -- Automation (display_order 50-69) --
    /// Execute a multi-operation plan atomically.
    #[command(display_order = 50)]
    Tx(crate::tx::TxArgs),
    /// Explain a tx plan in plain English.
    #[command(display_order = 51)]
    Explain(explain::ExplainArgs),
    /// Restore files from a backup created by --apply.
    #[command(display_order = 52)]
    Undo(undo::UndoArgs),
    /// Show which files have uncommitted changes.
    #[command(display_order = 53)]
    Status(status::StatusArgs),
    /// Export operation schemas, tier-filtered listings, or system prompt fragments.
    #[command(display_order = 54)]
    Schema(schema::SchemaArgs),
    /// Print agent rules for using patchloom (AGENTS.md content for end users).
    #[command(display_order = 55)]
    AgentRules(agent_rules::AgentRulesArgs),
    /// Set up patchloom in the current project.
    #[command(display_order = 56)]
    Init(init::InitArgs),
    /// Start an MCP (Model Context Protocol) server (stdio by default, Streamable HTTP with --http).
    #[cfg(feature = "mcp")]
    #[command(display_order = 57)]
    McpServer {
        /// Log every tool call to a JSONL file (tool name, duration, status).
        /// Also settable via PATCHLOOM_MCP_LOG env var; the flag takes precedence.
        #[arg(long)]
        log: Option<String>,

        /// Use Streamable HTTP transport instead of stdio.
        #[cfg(feature = "mcp-http")]
        #[arg(long)]
        http: bool,

        /// Bind address (requires --http).
        #[cfg(feature = "mcp-http")]
        #[arg(long, default_value = "127.0.0.1", requires = "http")]
        host: String,

        /// Bind port (requires --http).
        #[cfg(feature = "mcp-http")]
        #[arg(long, default_value_t = 8080, requires = "http")]
        port: u16,

        /// TLS certificate PEM file; enables HTTPS (requires --http and --tls-key).
        #[cfg(feature = "mcp-http")]
        #[arg(long, requires_all = ["http", "tls_key"])]
        tls_cert: Option<std::path::PathBuf>,

        /// TLS private key PEM file (requires --http and --tls-cert).
        #[cfg(feature = "mcp-http")]
        #[arg(long, requires_all = ["http", "tls_cert"])]
        tls_key: Option<std::path::PathBuf>,
    },
    /// Generate shell completions for bash, zsh, fish, or elvish.
    #[command(display_order = 58)]
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

/// Load and apply project config from `.patchloom.toml`.
fn load_project_config(global: &mut crate::cli::global::GlobalFlags) {
    let Ok(cwd) = global.resolve_cwd() else {
        return;
    };
    if let Some((config, _)) = crate::config::find_and_load(&cwd) {
        crate::config::apply_config(global, &config);
    }
}

pub fn dispatch(cli: Cli) -> anyhow::Result<u8> {
    let mut global = cli.global;

    // Load config early for read-only commands (no merge_write).
    // Write commands call load_project_config after merge_write.
    match cli.command {
        #[cfg(feature = "mcp")]
        Command::McpServer {
            log,
            #[cfg(feature = "mcp-http")]
            http,
            #[cfg(feature = "mcp-http")]
            host,
            #[cfg(feature = "mcp-http")]
            port,
            #[cfg(feature = "mcp-http")]
            tls_cert,
            #[cfg(feature = "mcp-http")]
            tls_key,
        } => {
            load_project_config(&mut global);
            #[cfg(feature = "mcp-http")]
            if http {
                return mcp::run_mcp_http_server(
                    &global,
                    log,
                    &host,
                    port,
                    tls_cert.as_deref(),
                    tls_key.as_deref(),
                );
            }
            mcp::run_mcp_server(&global, log)
        }
        Command::Schema(args) => schema::run(args, &global),
        Command::AgentRules(args) => agent_rules::run(args, &global),
        Command::Init(args) => init::run(args, &global),
        Command::Completions { shell } => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            clap_complete::generate(shell, &mut cmd, "patchloom", &mut std::io::stdout());
            Ok(crate::exit::SUCCESS)
        }
        Command::Read(args) => {
            load_project_config(&mut global);
            read::run(args, &global)
        }
        Command::Explain(args) => {
            load_project_config(&mut global);
            explain::run(args, &global)
        }
        Command::Undo(args) => {
            load_project_config(&mut global);
            undo::run(args, &global)
        }
        Command::Search(args) => {
            load_project_config(&mut global);
            search::run(args, &global)
        }
        Command::Status(args) => {
            load_project_config(&mut global);
            status::run(args, &global)
        }
        Command::Append(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            append::run(args, &global)
        }
        Command::Prepend(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            prepend::run(args, &global)
        }
        Command::Create(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            create::run(args, &global)
        }
        Command::Delete(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            delete::run(args, &global)
        }
        Command::Rename(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            rename::run(args, &global)
        }
        Command::Replace(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            replace::run(args, &global)
        }
        Command::Patch(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            patch::run(args, &global)
        }
        Command::Md(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            md::run(args, &global)
        }
        Command::Doc(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            doc::run(args, &global)
        }
        Command::Tidy(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            tidy::run(args, &global)
        }
        Command::Tx(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            tx::run(args, &global)
        }
        Command::Batch(args) => {
            global.merge_write(&args.write);
            load_project_config(&mut global);
            batch::run(args, &global)
        }
        #[cfg(feature = "ast")]
        Command::Ast(args) => {
            // ast rename and ast replace have write flags; others are read-only
            match args.command {
                ast::AstCommand::Rename(ref a) => global.merge_write(&a.write),
                ast::AstCommand::Replace(ref a) => global.merge_write(&a.write),
                _ => {}
            }
            load_project_config(&mut global);
            ast::run(args, &global)
        }
    }
}
