#![deny(unsafe_code)]

pub mod cli;
pub mod cmd;
pub mod diff;
pub mod error;
pub mod exit;
pub mod plan;
pub mod selector;
pub mod write;

use clap::Parser;
use cli::global::GlobalFlags;
use cli::Cli;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB, the same heuristic Git uses).
pub(crate) fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Collect file paths from either `--files-from`, or by walking `paths` with
/// `ignore::WalkBuilder` (respects `.gitignore`).
pub(crate) fn collect_file_paths(
    paths: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(files) = global.read_files_from() {
        return Ok(files.iter().map(PathBuf::from).collect());
    }
    let defaults;
    let roots: &[String] = if paths.is_empty() {
        defaults = [".".to_string()];
        &defaults
    } else {
        paths
    };
    let mut builder = WalkBuilder::new(&roots[0]);
    for p in &roots[1..] {
        builder.add(p);
    }
    Ok(builder
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .map(|e| e.into_path())
        .collect())
}

/// Build a compiled glob matcher from `--glob`, or `None` if unset.
pub(crate) fn build_glob_matcher(global: &GlobalFlags) -> anyhow::Result<Option<GlobMatcher>> {
    global
        .glob
        .as_ref()
        .map(|p| Glob::new(p).map(|g| g.compile_matcher()))
        .transpose()
        .map_err(Into::into)
}

/// Check whether `path` matches the glob (always true if no glob).
pub(crate) fn matches_glob(path: &Path, matcher: Option<&GlobMatcher>) -> bool {
    match matcher {
        None => true,
        Some(m) => m.is_match(path) || path.file_name().is_some_and(|n| m.is_match(n)),
    }
}

/// Run the patchloom CLI. Returns the exit code as a u8.
pub fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    cmd::dispatch(cli)
}
