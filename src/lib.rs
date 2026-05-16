#![deny(unsafe_code)]

pub mod cli;
pub mod cmd;
pub mod diff;
pub mod error;
pub mod exit;
pub(crate) mod ops;
pub mod plan;
pub mod selector;
pub mod write;

use clap::Parser;
use cli::global::GlobalFlags;
use cli::Cli;
use globset::{Glob, GlobSet, GlobSetBuilder};
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
    collect_file_paths_opts(paths, global, false, None)
}

/// Like [`collect_file_paths`] but with options for hidden files and a root
/// directory.  When `root` is `Some`, paths are joined with it before walking.
/// Hygiene commands set `include_hidden = true` so dotfiles are checked.
pub(crate) fn collect_file_paths_opts(
    paths: &[String],
    global: &GlobalFlags,
    include_hidden: bool,
    root: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(files) = global.read_files_from()? {
        return Ok(files
            .iter()
            .map(|f| match root {
                Some(r) => r.join(f),
                None => PathBuf::from(f),
            })
            .collect());
    }
    let defaults;
    let effective: &[String] = if paths.is_empty() {
        defaults = [".".to_string()];
        &defaults
    } else {
        paths
    };
    let resolve = |p: &str| -> PathBuf {
        match root {
            Some(r) => r.join(p),
            None => PathBuf::from(p),
        }
    };
    let first = resolve(&effective[0]);
    let mut builder = WalkBuilder::new(&first);
    for p in &effective[1..] {
        builder.add(resolve(p));
    }
    if include_hidden {
        builder.hidden(false);
    }
    Ok(builder
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .map(|e| e.into_path())
        .collect())
}

/// Build a compiled glob matcher from `--glob`, or `None` if no globs given.
pub(crate) fn build_glob_matcher(global: &GlobalFlags) -> anyhow::Result<Option<GlobSet>> {
    if global.glob.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in &global.glob {
        builder.add(Glob::new(pattern)?);
    }
    Ok(Some(builder.build()?))
}

/// Check whether `path` matches any of the globs (always true if no globs).
pub(crate) fn matches_glob(path: &Path, matcher: Option<&GlobSet>) -> bool {
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
