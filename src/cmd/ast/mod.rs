//! AST-aware subcommands: `patchloom ast list|read|rename|validate|search|refs|deps|map|replace|replace-symbol|delete-symbol|insert|wrap|imports|reorder|group|move|extract-to-file|split|rewrite-signature|impact|diff`.

mod common;
mod mutate;
mod mutate_extra;
mod query;

pub(crate) use common::{
    collect_source_files, display_path, get_git_file_content, is_sole_explicit_file,
    resolve_target_paths, symbol_to_json,
};
pub use common::{filter_symbols, parse_kind_filter};
pub(crate) use mutate_extra::{parse_reorder_order, parse_split_targets};

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
    /// Replace a whole symbol span (including leading docs and attributes).
    ReplaceSymbol(mutate::ReplaceSymbolArgs),
    /// Delete a whole symbol span (including leading docs and attributes).
    DeleteSymbol(mutate::DeleteSymbolArgs),
    /// Insert source at a structurally-defined position.
    Insert(mutate_extra::InsertArgs),
    /// Wrap symbols or a line range in a block.
    Wrap(mutate_extra::WrapArgs),
    /// Add, remove, or deduplicate import statements.
    Imports(mutate_extra::ImportsArgs),
    /// Reorder symbols in a file or scope.
    Reorder(mutate_extra::ReorderArgs),
    /// Move symbols into a module block in the same file.
    Group(mutate_extra::GroupArgs),
    /// Move symbols from one file to another.
    Move(mutate_extra::MoveArgs),
    /// Extract one symbol into a new file.
    ExtractToFile(mutate_extra::ExtractToFileArgs),
    /// Split a file by distributing symbols across targets.
    Split(mutate_extra::SplitArgs),
    /// Rewrite a function signature with structured fields or a full span.
    RewriteSignature(mutate_extra::RewriteSignatureArgs),
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
        AstCommand::ReplaceSymbol(a) => mutate::run_replace_symbol(a, global),
        AstCommand::DeleteSymbol(a) => mutate::run_delete_symbol(a, global),
        AstCommand::Insert(a) => mutate_extra::run_insert(a, global),
        AstCommand::Wrap(a) => mutate_extra::run_wrap(a, global),
        AstCommand::Imports(a) => mutate_extra::run_imports(a, global),
        AstCommand::Reorder(a) => mutate_extra::run_reorder(a, global),
        AstCommand::Group(a) => mutate_extra::run_group(a, global),
        AstCommand::Move(a) => mutate_extra::run_move(a, global),
        AstCommand::ExtractToFile(a) => mutate_extra::run_extract_to_file(a, global),
        AstCommand::Split(a) => mutate_extra::run_split(a, global),
        AstCommand::RewriteSignature(a) => mutate_extra::run_rewrite_signature(a, global),
        AstCommand::Impact(a) => query::run_impact(a, global),
        AstCommand::Diff(a) => query::run_diff(a, global),
    }
}
