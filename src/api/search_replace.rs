//! Library apply for SEARCH/REPLACE / DiffFenced blocks (#2220).

use std::path::{Component, Path, PathBuf};

use crate::containment::PathGuard;
use crate::ops::search_replace::{
    SearchReplaceBlock, SearchReplaceParseError, parse_diff_fenced, parse_search_replace,
};

use super::{ApplyMode, EditResult, ReplaceOptions};

/// Options for applying SEARCH/REPLACE blocks.
///
/// Unique apply is implied when `replace_all` is false. Do not flip
/// [`ReplaceOptions::unique`] on generic `replace_text`.
#[derive(Debug, Clone, Default)]
pub struct ApplySearchReplaceOptions {
    /// When true, update every exact match. Default false (unique, or error).
    pub replace_all: bool,
}

/// Parse then apply a SEARCH/REPLACE or DiffFenced document.
pub fn apply_search_replace_document(
    input: &str,
    cwd: &Path,
    opts: &ApplySearchReplaceOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let blocks = parse_search_replace_document(input).map_err(map_parse_err)?;
    apply_search_replace_blocks(&blocks, cwd, opts, mode, guard)
}

/// Parse SEARCH/REPLACE, or DiffFenced (fenced unwrap) when the document
/// wraps blocks in triple backticks.
pub fn parse_search_replace_document(
    input: &str,
) -> Result<Vec<SearchReplaceBlock>, SearchReplaceParseError> {
    let fenced = input.lines().any(|l| {
        let t = l.trim();
        t == "```" || t.starts_with("```")
    });
    if fenced {
        parse_diff_fenced(input)
    } else {
        parse_search_replace(input)
    }
}

/// Apply parsed SEARCH/REPLACE blocks under `cwd`.
///
/// Default: multi-match is [`crate::fallback::EditErrorKind::AmbiguousTarget`]
/// and nothing is written. `replace_all: true` updates every exact match.
/// Empty SEARCH is invalid input. Relative paths only (or absolute when a
/// PathGuard allows the contained path). `..` is rejected.
pub fn apply_search_replace_blocks(
    blocks: &[SearchReplaceBlock],
    cwd: &Path,
    opts: &ApplySearchReplaceOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    if blocks.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "no SEARCH/REPLACE blocks to apply".into(),
        }));
    }

    let replace_opts = ReplaceOptions {
        unique: !opts.replace_all,
        require_change: true,
        ..ReplaceOptions::default()
    };

    // Group sequential blocks by resolved path so one file is rewritten once.
    struct Planned {
        path: PathBuf,
        display: String,
        original: String,
        new_content: String,
        match_count: usize,
    }
    let mut planned: Vec<Planned> = Vec::new();

    for block in blocks {
        if block.old.is_empty() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "SEARCH/REPLACE SEARCH block must not be empty (not a whole-file rewrite)"
                    .into(),
            }));
        }
        if block.path.trim().is_empty() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "SEARCH/REPLACE path must not be empty".into(),
            }));
        }
        let dest = resolve_search_replace_path(cwd, &block.path, guard)?;
        let display = block.path.clone();

        if let Some(idx) = planned.iter().position(|p| p.path == dest) {
            let existing = &mut planned[idx];
            let content_result = super::replace_in_content(
                &existing.new_content,
                &block.old,
                &block.new,
                &replace_opts,
            )?;
            existing.new_content = content_result.new_content;
            existing.match_count = existing
                .match_count
                .saturating_add(content_result.match_count);
            continue;
        }

        let path_str = dest.to_string_lossy();
        let original = crate::files::load_text_strict(&dest, &path_str)?;
        let content_result =
            super::replace_in_content(&original, &block.old, &block.new, &replace_opts)?;
        planned.push(Planned {
            path: dest,
            display,
            original,
            new_content: content_result.new_content,
            match_count: content_result.match_count,
        });
    }

    let writes: Vec<(&Path, &str)> = planned
        .iter()
        .map(|p| (p.path.as_path(), p.new_content.as_str()))
        .collect();
    let policy = crate::write::WritePolicy::default();
    let (applied, backup_session) = super::write_if_apply_many(&writes, mode, &policy, guard, cwd)?;

    let mut results = Vec::with_capacity(planned.len());
    for p in planned {
        let mut e = super::build_edit_result(
            &p.display,
            p.original,
            p.new_content,
            applied,
            "replace",
            None,
        );
        e.match_count = p.match_count;
        if p.match_count > 0 {
            e.match_mode = Some(super::MatchMode::Exact);
        }
        e.backup_session = backup_session.clone();
        results.push(e);
    }
    Ok(results)
}

fn map_parse_err(e: SearchReplaceParseError) -> anyhow::Error {
    anyhow::Error::new(crate::exit::ParseErrorError { msg: e.message })
}

fn resolve_search_replace_path(
    cwd: &Path,
    dest: &str,
    guard: Option<&PathGuard>,
) -> anyhow::Result<PathBuf> {
    let path = Path::new(dest);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("SEARCH/REPLACE path must not contain '..': {dest}"),
        }));
    }
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    if path.is_absolute() && guard.is_none() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "SEARCH/REPLACE path must be relative to the workspace (or pass PathGuard for a contained absolute): {dest}"
            ),
        }));
    }
    super::ensure_contained(guard, &joined)?;
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{is_ambiguous, is_invalid_input, is_no_match, is_not_found};
    use crate::containment::PathGuard;

    fn sr_block(path: &str, old: &str, new: &str) -> SearchReplaceBlock {
        SearchReplaceBlock {
            path: path.into(),
            old: old.into(),
            new: new.into(),
        }
    }

    #[test]
    fn apply_search_replace_unique_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.rs");
        std::fs::write(&path, "fn foo() {}\nfn bar() {}\n").unwrap();
        let blocks = [sr_block("f.rs", "fn foo() {}", "fn baz() {}")];
        let results = apply_search_replace_blocks(
            &blocks,
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            None,
        )
        .expect("unique apply");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "fn baz() {}\nfn bar() {}\n"
        );
        assert!(results[0].backup_session.is_some());
    }

    #[test]
    fn apply_search_replace_interleaved_same_file_composes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.rs");
        let b = dir.path().join("b.rs");
        std::fs::write(&a, "hello world\n").unwrap();
        std::fs::write(&b, "keep\n").unwrap();
        let blocks = [
            sr_block("a.rs", "hello", "hi"),
            sr_block("b.rs", "keep", "kept"),
            sr_block("a.rs", "world", "earth"),
        ];
        apply_search_replace_blocks(
            &blocks,
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            None,
        )
        .expect("interleaved");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "hi earth\n");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "kept\n");
    }

    #[test]
    fn apply_search_replace_multi_match_no_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.rs");
        let original = "foo\nfoo\nfoo\n";
        std::fs::write(&path, original).unwrap();
        let blocks = [sr_block("f.rs", "foo", "bar")];
        let err = apply_search_replace_blocks(
            &blocks,
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            None,
        )
        .expect_err("ambiguous");
        assert!(is_ambiguous(&err));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn apply_search_replace_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.rs");
        std::fs::write(&path, "foo\nfoo\nfoo\n").unwrap();
        let blocks = [sr_block("f.rs", "foo", "bar")];
        let results = apply_search_replace_blocks(
            &blocks,
            dir.path(),
            &ApplySearchReplaceOptions { replace_all: true },
            ApplyMode::Apply,
            None,
        )
        .expect("replace_all");
        assert_eq!(results[0].match_count, 3);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "bar\nbar\nbar\n");
    }

    #[test]
    fn apply_search_replace_empty_search() {
        let dir = tempfile::tempdir().unwrap();
        let err = apply_search_replace_blocks(
            &[sr_block("f.rs", "", "x")],
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Preview,
            None,
        )
        .expect_err("empty");
        assert!(is_invalid_input(&err));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn apply_search_replace_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let err = apply_search_replace_blocks(
            &[sr_block("missing.rs", "old", "new")],
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Preview,
            None,
        )
        .expect_err("missing");
        assert!(is_not_found(&err));
    }

    #[test]
    fn apply_search_replace_path_guard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.rs"), "old\n").unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .build()
            .unwrap();
        let err = apply_search_replace_blocks(
            &[sr_block("../escape.rs", "old", "new")],
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            Some(&guard),
        )
        .expect_err("parent");
        assert!(is_invalid_input(&err));
        assert!(err.to_string().contains(".."));
    }

    #[test]
    fn apply_search_replace_zero_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.rs"), "live\n").unwrap();
        let err = apply_search_replace_blocks(
            &[sr_block("f.rs", "missing", "x")],
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            None,
        )
        .expect_err("no match");
        assert!(is_no_match(&err));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.rs")).unwrap(),
            "live\n"
        );
    }

    #[test]
    fn apply_search_replace_document_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "old\n").unwrap();
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>> REPLACE
";
        let results = apply_search_replace_document(
            input,
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            None,
        )
        .expect("document");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
            "new\n"
        );
        assert_eq!(results[0].match_count, 1);
    }
}
