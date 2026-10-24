//! Library apply for SEARCH/REPLACE / DiffFenced blocks (#2220).

use std::path::{Component, Path, PathBuf};

use crate::containment::PathGuard;
use crate::ops::search_replace::{SearchReplaceBlock, SearchReplaceParseError};

use super::{ApplyMode, EditResult, ReplaceOptions};

/// Options for applying SEARCH/REPLACE blocks.
///
/// Unique apply is implied when `replace_all` is false. Do not flip
/// [`ReplaceOptions::unique`] on generic `replace_text`.
#[derive(Debug, Clone, Default)]
pub struct ApplySearchReplaceOptions {
    /// When true, update every exact match. Default false (unique, or error).
    pub replace_all: bool,
    /// Remap dest onto this path when dest is that path or a suffix
    /// (same rule as Begin Patch `file_hint`). Empty dest (no `-------`
    /// separator) also uses this path. Used by [`super::apply_patch`].
    pub file_hint: Option<PathBuf>,
}

/// Parse then apply a SEARCH/REPLACE or DiffFenced document.
pub fn apply_search_replace_document(
    input: &str,
    cwd: &Path,
    opts: &ApplySearchReplaceOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<EditResult>> {
    let blocks =
        crate::ops::search_replace::parse_search_replace_document(input).map_err(map_parse_err)?;
    apply_search_replace_blocks(&blocks, cwd, opts, mode, guard)
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
        if block.path.trim().is_empty() && opts.file_hint.is_none() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: crate::ops::search_replace::SEARCH_REPLACE_EMPTY_PATH.into(),
            }));
        }
        let dest = resolve_search_replace_path(cwd, &block.path, opts.file_hint.as_deref(), guard)?;
        let display = if block.path.trim().is_empty() {
            dest.to_string_lossy().into_owned()
        } else {
            block.path.clone()
        };

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

pub(crate) fn resolve_search_replace_path(
    cwd: &Path,
    dest: &str,
    file_hint: Option<&Path>,
    guard: Option<&PathGuard>,
) -> anyhow::Result<PathBuf> {
    if dest.trim().is_empty() {
        let Some(hint) = file_hint else {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: crate::ops::search_replace::SEARCH_REPLACE_EMPTY_PATH.into(),
            }));
        };
        super::ensure_contained_resolved(guard, hint)?;
        return Ok(hint.to_path_buf());
    }
    let path = Path::new(dest);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("SEARCH/REPLACE path must not contain '..': {dest}"),
        }));
    }
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::ops::begin_patch::resolve_begin_patch_dest(cwd, dest, file_hint)
    };
    if path.is_absolute() && guard.is_none() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "SEARCH/REPLACE path must be relative to the workspace (or pass PathGuard for a contained absolute): {dest}"
            ),
        }));
    }
    super::ensure_contained_resolved(guard, &joined)?;
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{is_ambiguous, is_invalid_input, is_no_match, is_not_found};
    use crate::containment::PathGuard;
    use crate::exit::is_parse_error;

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
            &ApplySearchReplaceOptions {
                replace_all: true,
                ..Default::default()
            },
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
    fn apply_search_replace_reject_guard_allows_in_root_write() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("in.rs"), "old\n").unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .build()
            .unwrap();
        let results = apply_search_replace_blocks(
            &[sr_block("in.rs", "old", "new")],
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Apply,
            Some(&guard),
        )
        .expect("in-root dest under builder-default Reject must apply");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("in.rs")).unwrap(),
            "new\n"
        );
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

    #[test]
    fn apply_patch_detects_search_replace_document() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("code.rs");
        std::fs::write(&dest, "fn old() {}\n").unwrap();
        let input = "\
<<<<<<< SEARCH
code.rs
-------
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
";
        let result = crate::api::apply_patch(&dest, input, ApplyMode::Apply, None)
            .expect("apply_patch detects SEARCH/REPLACE");
        assert!(result.applied);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "fn new() {}\n");
    }

    #[test]
    fn apply_patch_destless_search_replace_uses_file_hint() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let dest = src.join("rafter.rs");
        let original = concat!(
            "            pub fn set(&mut self, deg: u32) -> Result<u64, Error> {\n",
            "        if deg == 0 {\n",
            "            return Err(Error::Zero);\n",
            "        }\n",
            "        Ok(deg as u64)\n",
            "    }\n",
        );
        let updated = concat!(
            "    pub fn set(&mut self, deg: u32) -> Result<u64, Error> {\n",
            "        if deg == 0 {\n",
            "            return Err(Error::Zero);\n",
            "        }\n",
            "        Ok(deg as u64)\n",
            "    }\n",
        );
        std::fs::write(&dest, original).unwrap();
        let input = concat!(
            "<<<<<<< SEARCH\n",
            "            pub fn set(&mut self, deg: u32) -> Result<u64, Error> {\n",
            "        if deg == 0 {\n",
            "=======\n",
            "    pub fn set(&mut self, deg: u32) -> Result<u64, Error> {\n",
            "        if deg == 0 {\n",
            ">>>>>>> REPLACE\n",
        );
        crate::api::apply_patch(&dest, input, ApplyMode::Apply, None)
            .expect("dest-less SEARCH/REPLACE applies to apply_patch path");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), updated);
        let invented = src.join("pub fn set(&mut self, deg: u32) -> Result<u64, Error> {");
        assert!(
            !invented.exists(),
            "must not join the first SEARCH line under path.parent()"
        );
    }

    #[test]
    fn apply_search_replace_destless_without_file_hint_is_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("only.rs"), "only.rs\nthe old text\n").unwrap();
        let input = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        let err = apply_search_replace_document(
            input,
            dir.path(),
            &ApplySearchReplaceOptions::default(),
            ApplyMode::Preview,
            None,
        )
        .expect_err("dest-less needs file_hint or ------- dest");
        assert!(is_invalid_input(&err), "{err}");
        assert!(err.to_string().contains("path must not be empty"), "{err}");
    }

    #[test]
    fn apply_patch_file_destless_search_replace_is_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("only.rs"), "only.rs\nthe old text\n").unwrap();
        let input = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        let err = crate::api::apply_patch_file(input, dir.path(), ApplyMode::Preview, None)
            .expect_err("apply_patch_file still needs ------- dest");
        assert!(is_invalid_input(&err), "{err}");
    }

    #[test]
    fn apply_patch_search_replace_repo_relative_dest_uses_file_hint() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let dest = src.join("foo.rs");
        std::fs::write(&dest, "fn old() {}\n").unwrap();
        let input = "\
<<<<<<< SEARCH
src/foo.rs
-------
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
";
        crate::api::apply_patch(&dest, input, ApplyMode::Apply, None)
            .expect("repo-relative dest remaps onto apply_patch path");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "fn new() {}\n");
        assert!(
            !src.join("src").join("foo.rs").exists(),
            "must not join dest under path.parent() as src/src/foo.rs"
        );
    }

    fn two_dest_doc(first: &str, second: &str) -> String {
        format!(
            "<<<<<<< SEARCH\n{first}\n-------\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n<<<<<<< SEARCH\n{second}\n-------\nbeta\n=======\nBETA\n>>>>>>> REPLACE\n"
        )
    }

    #[test]
    fn apply_patch_refuses_other_dest_before_write() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let other = dir.path().join("g.rs");
        let original_dest = "alpha\nbeta\n";
        let original_other = "alpha\nbeta\n";
        std::fs::write(&dest, original_dest).unwrap();
        std::fs::write(&other, original_other).unwrap();
        let doc = two_dest_doc("f.rs", "g.rs");
        for mode in [ApplyMode::Apply, ApplyMode::Preview] {
            let err = crate::api::apply_patch(&dest, &doc, mode, None)
                .expect_err("second dest is not the caller file");
            assert!(is_invalid_input(&err), "{mode:?} {err}");
            assert!(err.to_string().contains("apply_patch_file"), "{err}");
            assert_eq!(std::fs::read_to_string(&dest).unwrap(), original_dest);
            assert_eq!(std::fs::read_to_string(&other).unwrap(), original_other);
        }
    }

    #[test]
    fn apply_patch_refuses_other_dest_same_file_still_applies() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        std::fs::write(&dest, "alpha\nbeta\n").unwrap();
        let doc = two_dest_doc("f.rs", "f.rs");
        let result = crate::api::apply_patch(&dest, &doc, ApplyMode::Apply, None)
            .expect("both dests are the caller file");
        assert!(result.applied);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "ALPHA\nBETA\n");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != ".patchloom")
            .collect();
        assert_eq!(names, vec!["f.rs".to_string()]);
    }

    #[test]
    fn apply_patch_file_detects_search_replace_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
        let input = "\
<<<<<<< SEARCH
code.rs
-------
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
";
        let results = crate::api::apply_patch_file(input, dir.path(), ApplyMode::Apply, None)
            .expect("apply_patch_file detects SEARCH/REPLACE");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("code.rs")).unwrap(),
            "fn new() {}\n"
        );
    }

    fn two_foo_doc(dest_line: Option<&str>) -> String {
        match dest_line {
            Some(dest) => {
                format!("<<<<<<< SEARCH\n{dest}\n-------\nfoo\n=======\nbar\n>>>>>>> REPLACE\n")
            }
            None => "<<<<<<< SEARCH\nfoo\n=======\nbar\n>>>>>>> REPLACE\n".to_string(),
        }
    }

    #[test]
    fn apply_patch_multi_match_names_library_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "foo\nfoo\n";
        std::fs::write(&dest, original).unwrap();
        let err = crate::api::apply_patch(&dest, &two_foo_doc(None), ApplyMode::Apply, None)
            .expect_err("unique apply_patch");
        assert!(is_ambiguous(&err), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("2 times"), "{msg}");
        assert!(msg.contains("apply_search_replace_document"), "{msg}");
        assert!(msg.contains("apply_patch_with_replace_all"), "{msg}");
        assert!(msg.contains("apply_patch_file_with_replace_all"), "{msg}");
        assert!(msg.contains("replace_all: true (MCP apply_patch)"), "{msg}");
        assert!(msg.contains("unique: false (plan/MCP)"), "{msg}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    #[test]
    fn apply_patch_with_replace_all_updates_every_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        std::fs::write(&dest, "foo\nfoo\n").unwrap();
        let result = crate::api::apply_patch_with_replace_all(
            &dest,
            &two_foo_doc(None),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect("replace_all");
        assert!(result.applied);
        assert_eq!(result.match_count, 2);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "bar\nbar\n");
    }

    #[test]
    fn apply_patch_with_replace_all_unique_match_updates_once() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        std::fs::write(&dest, "foo\n").unwrap();
        let result = crate::api::apply_patch_with_replace_all(
            &dest,
            &two_foo_doc(None),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect("one match");
        assert!(result.applied);
        assert_eq!(result.match_count, 1);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "bar\n");
    }

    #[test]
    fn apply_patch_with_replace_all_preview_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "foo\nfoo\n";
        std::fs::write(&dest, original).unwrap();
        let result = crate::api::apply_patch_with_replace_all(
            &dest,
            &two_foo_doc(None),
            ApplyMode::Preview,
            None,
            true,
        )
        .expect("preview");
        assert!(!result.applied);
        assert!(result.changed);
        assert_eq!(result.match_count, 2);
        assert_eq!(result.new_content, "bar\nbar\n");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    #[test]
    fn apply_patch_file_multi_match_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "foo\nfoo\n";
        std::fs::write(&dest, original).unwrap();
        let err = crate::api::apply_patch_file(
            &two_foo_doc(Some("f.rs")),
            dir.path(),
            ApplyMode::Apply,
            None,
        )
        .expect_err("unique apply_patch_file");
        assert!(is_ambiguous(&err), "{err}");
        assert!(
            err.to_string()
                .contains("apply_patch_file_with_replace_all"),
            "{err}"
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    #[test]
    fn apply_patch_file_with_replace_all_updates_every_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        std::fs::write(&dest, "foo\nfoo\n").unwrap();
        let results = crate::api::apply_patch_file_with_replace_all(
            &two_foo_doc(Some("f.rs")),
            dir.path(),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect("replace_all");
        assert_eq!(results.len(), 1);
        assert!(results[0].applied);
        assert_eq!(results[0].match_count, 2);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "bar\nbar\n");
    }

    #[test]
    fn apply_patch_with_replace_all_rejects_unified_diff() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.txt");
        let original = "old\n";
        std::fs::write(&dest, original).unwrap();
        let diff = "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let err =
            crate::api::apply_patch_with_replace_all(&dest, diff, ApplyMode::Apply, None, true)
                .expect_err("unified");
        assert!(is_invalid_input(&err), "{err}");
        assert!(
            err.to_string()
                .contains("replace_all is only valid for SEARCH/REPLACE"),
            "{err}"
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        crate::api::apply_patch(&dest, diff, ApplyMode::Apply, None).expect("unique unified");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new\n");
    }

    #[test]
    fn apply_patch_file_with_replace_all_rejects_unified_diff() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.txt");
        let original = "old\n";
        std::fs::write(&dest, original).unwrap();
        let diff = "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let err = crate::api::apply_patch_file_with_replace_all(
            diff,
            dir.path(),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect_err("unified file");
        assert!(is_invalid_input(&err), "{err}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    #[test]
    fn apply_patch_with_replace_all_rejects_begin_patch() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "old\n";
        std::fs::write(&dest, original).unwrap();
        let patch = "*** Begin Patch\n*** Update File: f.rs\n@@\n-old\n+new\n*** End Patch\n";
        let err =
            crate::api::apply_patch_with_replace_all(&dest, patch, ApplyMode::Apply, None, true)
                .expect_err("begin");
        assert!(is_invalid_input(&err), "{err}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        let err = crate::api::apply_patch_file_with_replace_all(
            patch,
            dir.path(),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect_err("begin file");
        assert!(is_invalid_input(&err), "{err}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    /// Begin Patch that also contains `<<<<<<< SEARCH`. `replace_all: true`
    /// is invalid_input before the mixed-grammar parse_error.
    fn mixed_begin_and_search_replace() -> &'static str {
        "\
*** Begin Patch
*** Update File: f.rs
@@
-old
+new
<<<<<<< SEARCH
foo
=======
bar
>>>>>>> REPLACE
*** End Patch
"
    }

    fn assert_replace_all_refused(err: &anyhow::Error) {
        assert!(is_invalid_input(err), "{err}");
        assert!(!is_parse_error(err), "{err}");
        assert!(
            err.to_string()
                .contains("replace_all is only valid for SEARCH/REPLACE"),
            "{err}"
        );
    }

    fn assert_mixed_grammar(err: &anyhow::Error) {
        assert!(is_parse_error(err), "{err}");
        assert!(!is_invalid_input(err), "{err}");
        assert!(
            err.to_string()
                .contains("mixed Begin Patch and SEARCH/REPLACE"),
            "{err}"
        );
    }

    #[test]
    fn replace_all_on_mixed_begin_patch_is_invalid_input_before_parse() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "old\n";
        std::fs::write(&dest, original).unwrap();
        let patch = mixed_begin_and_search_replace();
        let err =
            crate::api::apply_patch_with_replace_all(&dest, patch, ApplyMode::Apply, None, true)
                .expect_err("mixed replace_all");
        assert_replace_all_refused(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        let err = crate::api::apply_patch(&dest, patch, ApplyMode::Apply, None)
            .expect_err("mixed unique");
        assert_mixed_grammar(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);

        let err = crate::api::apply_patch_file_with_replace_all(
            patch,
            dir.path(),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect_err("mixed file replace_all");
        assert_replace_all_refused(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        let err = crate::api::apply_patch_file(patch, dir.path(), ApplyMode::Apply, None)
            .expect_err("mixed file unique");
        assert_mixed_grammar(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }

    #[test]
    fn replace_all_on_garbage_is_invalid_input_before_parse() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.rs");
        let original = "old\n";
        std::fs::write(&dest, original).unwrap();
        let patch = "not a patch\n";
        let err =
            crate::api::apply_patch_with_replace_all(&dest, patch, ApplyMode::Apply, None, true)
                .expect_err("garbage replace_all");
        assert_replace_all_refused(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        let err = crate::api::apply_patch(&dest, patch, ApplyMode::Apply, None)
            .expect_err("garbage unique");
        assert!(is_parse_error(&err), "{err}");
        assert!(!is_invalid_input(&err), "{err}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);

        let err = crate::api::apply_patch_file_with_replace_all(
            patch,
            dir.path(),
            ApplyMode::Apply,
            None,
            true,
        )
        .expect_err("garbage file replace_all");
        assert_replace_all_refused(&err);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
        let err = crate::api::apply_patch_file(patch, dir.path(), ApplyMode::Apply, None)
            .expect_err("garbage file unique");
        assert!(is_parse_error(&err), "{err}");
        assert!(!is_invalid_input(&err), "{err}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    }
}
