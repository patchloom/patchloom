//! AST-aware subcommands: `patchloom ast list|read|rename|validate|search|refs|deps|map|replace|impact|diff`.

mod common;
mod mutate;
mod query;

pub(crate) use common::{
    collect_source_files, display_path, get_git_file_content, lang_from_str, resolve_target_paths,
    symbol_to_json,
};
pub use common::{filter_symbols, parse_kind_filter};

use crate::cli::global::GlobalFlags;
use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum AstCommand {
    /// List symbol definitions in a file or directory.
    List(query::ListArgs),
    /// Read a specific symbol by name.
    Read(query::ReadArgs),
    /// Rename identifiers in source code (AST-aware, skips strings/comments).
    Rename(mutate::RenameArgs),
    /// Validate syntax of source files.
    Validate(query::ValidateArgs),
    /// Structural search using AST queries.
    Search(query::SearchArgs),
    /// Find all references to a symbol across files.
    Refs(query::RefsArgs),
    /// Extract import/dependency statements from files.
    Deps(query::DepsArgs),
    /// Generate a ranked repository map (PageRank).
    Map(query::MapArgs),
    /// Replace text only within a specific symbol's body.
    Replace(mutate::ReplaceArgs),
    /// Transitive impact analysis of changing a symbol.
    Impact(query::ImpactArgs),
    /// Structural diff between two versions of a file.
    Diff(query::DiffArgs),
}

#[derive(Debug, Args)]
pub struct AstArgs {
    #[command(subcommand)]
    pub command: AstCommand,
}

pub fn run(args: AstArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    match args.command {
        AstCommand::List(a) => query::run_list(a, global),
        AstCommand::Read(a) => query::run_read(a, global),
        AstCommand::Rename(a) => mutate::run_rename(a, global),
        AstCommand::Validate(a) => query::run_validate(a, global),
        AstCommand::Search(a) => query::run_search(a, global),
        AstCommand::Refs(a) => query::run_refs(a, global),
        AstCommand::Deps(a) => query::run_deps(a, global),
        AstCommand::Map(a) => query::run_map(a, global),
        AstCommand::Replace(a) => mutate::run_replace(a, global),
        AstCommand::Impact(a) => query::run_impact(a, global),
        AstCommand::Diff(a) => query::run_diff(a, global),
    }
}
