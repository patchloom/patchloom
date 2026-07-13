//! AST write helpers for the public library API (feature `ast` + `files`/`cli`).
//!
//! Covers #1459 (signature rewrite), #1493 (rename / replace-in-symbol / in-content
//! signature rewrite), and #1495 (batch rename).

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::ast::rewrite::{FunctionSigEdit, rewrite_function_signature};
use crate::ast::{Language, rename, replace as ast_replace};
use crate::containment::PathGuard;
use crate::fallback::{EditError, EditErrorKind};
use crate::plan::Operation;
use crate::write::WritePolicy;

use super::{
    ApplyMode, ContentEditResult, EditResult, PostWriteHooks, ensure_contained, make_diff,
    maybe_post_write, write_if_apply,
};

/// Rewrite a function signature on disk (PathGuard + ApplyMode like other writers).
///
/// Prefer structured fields on [`FunctionSigEdit`]. Optionally pass `new_signature`
/// to replace the entire signature span (any language with tree-sitter support).
/// At least one of the structured fields or `new_signature` must be set.
///
/// Available with `features = ["ast"]` and `files` or `cli` for the write path.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_rewrite_signature(
    path: &Path,
    name: &str,
    edit: &FunctionSigEdit,
    new_signature: Option<&str>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    if new_signature.is_none()
        && edit.visibility.is_none()
        && edit.parameters.is_none()
        && edit.return_type.is_none()
    {
        return Err(EditError::new(
            EditErrorKind::InvalidInput,
            "ast_rewrite_signature requires new_signature and/or visibility/parameters/return_type",
        )
        .into());
    }
    let op = Operation::AstRewriteSignature {
        path: path.to_string_lossy().into(),
        old: name.into(),
        new_signature: new_signature.map(str::to_string),
        visibility: edit.visibility.clone(),
        parameters: edit.parameters.clone(),
        return_type: edit.return_type.clone(),
        lang: None,
    };
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "ast.rewrite_signature", None)
}

/// In-memory function signature rewrite (preview/apply parity with on-disk API).
///
/// See #1493. When `new_signature` is set, replaces the full signature span via
/// `rewrite_function_signature_full` semantics; otherwise applies structured
/// [`FunctionSigEdit`] fields.
#[cfg(feature = "ast")]
pub fn ast_rewrite_signature_in_content(
    content: &str,
    function_name: &str,
    edit: &FunctionSigEdit,
    new_signature: Option<&str>,
    lang: Language,
) -> anyhow::Result<ContentEditResult> {
    let new_content = if let Some(full) = new_signature {
        // Full-span replace is currently Rust-oriented (tree-sitter function_item).
        let _ = lang;
        crate::ast::rewrite::replace_function_signature(content, function_name, full).ok_or_else(
            || {
                EditError::new(
                    EditErrorKind::NoMatch,
                    format!("function `{function_name}` not found for signature rewrite"),
                )
            },
        )?
    } else {
        rewrite_function_signature(content, function_name, edit, lang).ok_or_else(|| {
            EditError::new(
                EditErrorKind::NoMatch,
                format!("function `{function_name}` not found for signature rewrite"),
            )
        })?
    };
    let changed = new_content != content;
    let diff = if changed {
        make_diff("<content>", content, &new_content)
    } else {
        String::new()
    };
    Ok(ContentEditResult {
        original: content.to_string(),
        new_content,
        diff,
        changed,
        match_count: if changed { 1 } else { 0 },
        match_mode: None,
        match_score: None,
    })
}

/// Rename all identifier occurrences of `old` to `new` in a source file.
///
/// Tree-sitter based (skips strings/comments). Zero replacements → `NoMatch`.
/// See #1493.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_rename(
    path: &Path,
    old: &str,
    new: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    if old.is_empty() || new.is_empty() {
        return Err(EditError::new(
            EditErrorKind::InvalidInput,
            "ast_rename old/new must not be empty",
        )
        .into());
    }
    ensure_contained(guard, path)?;
    let path_str = path.to_string_lossy().into_owned();
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {path_str}"))
        .map_err(|e| EditError::new(EditErrorKind::OperationFailed, e.to_string()))?;
    let lang = Language::from_path(path);
    let Some(renamed) = rename::rename_in_source(&original, old, new, lang) else {
        return Err(EditError::new(
            EditErrorKind::ParseError,
            format!("failed to parse {} as {:?}", path_str, lang),
        )
        .into());
    };
    if renamed.replacements == 0 {
        return Err(EditError::new(
            EditErrorKind::NoMatch,
            format!("no identifier matches for `{old}` in {path_str}"),
        )
        .into());
    }
    let policy = WritePolicy::default();
    let (applied, backup_session) = write_if_apply(path, &renamed.content, mode, &policy, guard)?;
    let mut result = super::build_edit_result(
        &path_str,
        original,
        renamed.content,
        applied,
        "ast.rename",
        None,
    );
    result.backup_session = backup_session;
    result.match_count = renamed.replacements;
    Ok(result)
}

/// Options for [`ast_replace_in_symbol`] (#1658).
#[derive(Debug, Clone, Default)]
pub struct AstReplaceInSymbolOptions {
    /// Use regex mode for `old` (default `false`).
    pub regex: bool,
}

/// Replace text inside a named symbol body on disk.
///
/// Supports literal and regex modes via [`AstReplaceInSymbolOptions`].
/// - Missing symbol → `NoMatch` (message names the symbol)
/// - Symbol found, zero pattern matches → `NoMatch` (message names the pattern)
///
/// Zero replacements → `NoMatch`. See #1493, #1658.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_replace_in_symbol(
    path: &Path,
    symbol: &str,
    old: &str,
    new: &str,
    opts: &AstReplaceInSymbolOptions,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    ensure_contained(guard, path)?;
    let path_str = path.to_string_lossy().into_owned();
    let original =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {path_str}"))?;
    let lang = Language::from_path(path);
    let replaced =
        match ast_replace::replace_in_symbol(&original, symbol, old, new, opts.regex, lang) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Err(EditError::new(
                    EditErrorKind::NoMatch,
                    format!("symbol `{symbol}` not found in {path_str}"),
                )
                .into());
            }
            Err(e) => {
                return Err(EditError::new(EditErrorKind::OperationFailed, e.to_string()).into());
            }
        };
    if replaced.replacements == 0 {
        return Err(EditError::new(
            EditErrorKind::NoMatch,
            format!("no matches for `{old}` in symbol `{symbol}` in {path_str}"),
        )
        .into());
    }
    let policy = WritePolicy::default();
    let (applied, backup_session) = write_if_apply(path, &replaced.content, mode, &policy, guard)?;
    let mut result = super::build_edit_result(
        &path_str,
        original,
        replaced.content,
        applied,
        "ast.replace_in_symbol",
        None,
    );
    result.backup_session = backup_session;
    result.match_count = replaced.replacements;
    result.match_mode = Some(super::MatchMode::Exact);
    Ok(result)
}

/// Options for [`find_files_with_symbol`] (#1664).
#[derive(Debug, Clone, Default)]
pub struct SymbolSearchOptions {
    /// Cap on returned paths (`None` = no limit).
    pub max_files: Option<usize>,
    /// Restrict to these languages (`None` = any language with a known extension).
    pub languages: Option<Vec<Language>>,
    /// Include hidden files/dirs (default false; same as search).
    pub include_hidden: bool,
}

/// Paths under `root` that likely contain identifier `name` (#1664).
///
/// Uses the same ignore / binary skip rules as library file collection, then
/// a word-boundary heuristic on source files with a known language extension.
/// Compose with [`ast_rename_batch`]:
///
/// ```rust,no_run
/// # #[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
/// # {
/// use patchloom::api::{
///     ApplyMode, AstRenameBatchOptions, SymbolSearchOptions, ast_rename_batch,
///     find_files_with_symbol,
/// };
/// use std::path::Path;
///
/// let root = Path::new(".");
/// let paths = find_files_with_symbol(root, "OldType", &SymbolSearchOptions::default())?;
/// let path_refs: Vec<&Path> = paths.iter().map(Path::new).collect();
/// let _ = ast_rename_batch(
///     &path_refs,
///     "OldType",
///     "NewType",
///     &AstRenameBatchOptions {
///         mode: ApplyMode::Preview,
///         ..Default::default()
///     },
///     None,
/// )?;
/// # }
/// # Ok::<(), anyhow::Error>(())
/// ```
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn find_files_with_symbol(
    root: &Path,
    name: &str,
    opts: &SymbolSearchOptions,
) -> anyhow::Result<Vec<PathBuf>> {
    if name.is_empty() {
        return Err(EditError::new(
            EditErrorKind::InvalidInput,
            "find_files_with_symbol name must not be empty",
        )
        .into());
    }
    // max_files: 0 means empty result (not "one file" via `len >= 0` after push).
    if opts.max_files == Some(0) {
        return Ok(Vec::new());
    }
    let all = crate::files::collect_file_paths(root, opts.include_hidden)?;
    let mut out = Vec::new();
    for path in all {
        let lang = Language::from_path(&path);
        if lang == Language::Unknown {
            continue;
        }
        if let Some(ref allowed) = opts.languages
            && !allowed.contains(&lang)
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if crate::files::is_binary(&bytes) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        if content_has_identifier(&content, name) {
            out.push(path);
            if opts.max_files.is_some_and(|m| out.len() >= m) {
                break;
            }
        }
    }
    Ok(out)
}

/// Options for [`ast_rename_project`] (#1689).
#[derive(Debug, Clone, Default)]
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub struct AstRenameProjectOptions {
    pub search: SymbolSearchOptions,
    pub rename: AstRenameBatchOptions,
}

/// Result of [`ast_rename_project`]: discovery set plus per-file rename outcomes.
#[derive(Debug)]
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub struct AstRenameProjectReport {
    /// Paths discovery considered (even when rename is a no-op).
    pub paths_considered: Vec<PathBuf>,
    /// Per-file rename results (same order as batch processing).
    pub results: Vec<AstRenameFileResult>,
}

/// Discover files containing `old_name` under `root` then batch-rename (#1689).
///
/// Preferred one-shot for agent hosts: “rename OldType project-wide” without a
/// separate path list from the model.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_rename_project(
    root: &Path,
    old_name: &str,
    new_name: &str,
    opts: &AstRenameProjectOptions,
    guard: Option<&PathGuard>,
) -> anyhow::Result<AstRenameProjectReport> {
    let paths = find_files_with_symbol(root, old_name, &opts.search)?;
    if paths.is_empty() {
        return Err(EditError::new(
            EditErrorKind::NoMatch,
            format!(
                "no files under {} contain identifier {old_name:?}",
                root.display()
            ),
        )
        .into());
    }
    let path_refs: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let results = ast_rename_batch(&path_refs, old_name, new_name, &opts.rename, guard)?;
    Ok(AstRenameProjectReport {
        paths_considered: paths,
        results,
    })
}

/// Word-boundary check for an identifier (ASCII-aware; good enough for discovery).
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
fn content_has_identifier(content: &str, name: &str) -> bool {
    if name.is_empty() || !content.contains(name) {
        return false;
    }
    let bytes = content.as_bytes();
    let name_bytes = name.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = content[start..].find(name) {
        let i = start + rel;
        let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
        let after = i + name_bytes.len();
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
        if start >= content.len() {
            break;
        }
    }
    false
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Options for [`ast_rename_batch`] (#1495).
#[derive(Debug, Clone)]
pub struct AstRenameBatchOptions {
    pub mode: ApplyMode,
    /// When true (default), a per-file `NoMatch` is recorded and remaining
    /// paths still run (agent-friendly best-effort). When false, the batch
    /// stops after the first `NoMatch` (that error is still in the results).
    pub continue_on_no_match: bool,
    /// Stop after the first hard error (IO, parse, guard). Default false.
    /// Independent of [`Self::continue_on_no_match`] (which only covers
    /// `NoMatch`).
    pub fail_fast: bool,
    /// Optional post-Apply format/lint hooks run per successful write (#1690).
    pub post_write: Option<PostWriteHooks>,
    /// Working directory for [`Self::post_write`] shell commands.
    pub post_write_cwd: Option<std::path::PathBuf>,
}

impl Default for AstRenameBatchOptions {
    fn default() -> Self {
        Self {
            mode: ApplyMode::Preview,
            continue_on_no_match: true,
            fail_fast: false,
            post_write: None,
            post_write_cwd: None,
        }
    }
}

/// Per-file outcome from [`ast_rename_batch`].
#[derive(Debug)]
pub struct AstRenameFileResult {
    pub path: PathBuf,
    pub result: Result<EditResult, EditError>,
}

/// Rename identifiers across many files with same-file serialization.
///
/// - Duplicate paths are processed **once** (first occurrence wins order).
/// - Never parallel-writes the same path; return order follows first-seen paths.
/// - Outer `Err` only for empty `old`/`new`. Per-file failures are in each
///   [`AstRenameFileResult::result`].
///
/// See #1495.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_rename_batch(
    paths: &[&Path],
    old: &str,
    new: &str,
    opts: &AstRenameBatchOptions,
    guard: Option<&PathGuard>,
) -> anyhow::Result<Vec<AstRenameFileResult>> {
    if old.is_empty() || new.is_empty() {
        return Err(EditError::new(
            EditErrorKind::InvalidInput,
            "ast_rename_batch old/new must not be empty",
        )
        .into());
    }
    // Dedupe while preserving first-seen order.
    let mut seen = std::collections::HashSet::new();
    let mut unique_paths: Vec<&Path> = Vec::new();
    for p in paths {
        let key = p.to_path_buf();
        if seen.insert(key) {
            unique_paths.push(*p);
        }
    }

    let mut out = Vec::with_capacity(unique_paths.len());
    for path in unique_paths {
        match ast_rename(path, old, new, opts.mode, guard) {
            Ok(r) => {
                // Post-Apply format/lint hooks when configured (#1690).
                if let Err(e) = maybe_post_write(
                    r.applied,
                    path,
                    opts.post_write.as_ref(),
                    opts.post_write_cwd.as_deref(),
                    r.backup_session.as_deref(),
                ) {
                    let edit_err = e.downcast::<EditError>().unwrap_or_else(|e| {
                        EditError::new(EditErrorKind::FormatFailed, e.to_string())
                    });
                    out.push(AstRenameFileResult {
                        path: path.to_path_buf(),
                        result: Err(edit_err),
                    });
                    if opts.fail_fast {
                        break;
                    }
                    continue;
                }
                out.push(AstRenameFileResult {
                    path: path.to_path_buf(),
                    result: Ok(r),
                });
            }
            Err(e) => {
                let edit_err = e.downcast::<EditError>().unwrap_or_else(|e| {
                    EditError::new(EditErrorKind::OperationFailed, e.to_string())
                });
                let is_no_match = edit_err.kind == EditErrorKind::NoMatch;
                out.push(AstRenameFileResult {
                    path: path.to_path_buf(),
                    result: Err(edit_err),
                });
                // NoMatch: stop when continue_on_no_match is false.
                if is_no_match && !opts.continue_on_no_match {
                    break;
                }
                // Hard errors: stop when fail_fast is true.
                if !is_no_match && opts.fail_fast {
                    break;
                }
            }
        }
    }
    Ok(out)
}
