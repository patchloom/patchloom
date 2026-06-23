#![allow(dead_code, unused_imports)]
use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{DiffResult, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::ops::doc::{
    FileFormat, deep_merge, delete_at_selector, delete_where, detect_format, move_at_path,
    navigate_mut, parse_doc, serialize_value_preserving, set_at_path, update_matching,
};
use crate::ops::md::{
    dedupe_headings_in, insert_after_heading_in, insert_before_heading_in, move_section_in,
    replace_section_in, table_append_for_tx, upsert_bullet_in,
};
use crate::ops::patch::{ApplyHunksOptions, ApplyHunksStatus, apply_patch_with_loader};
use crate::ops::replace::{
    ReplaceModeError, compile_replace_regex, replace_content, replace_whole_lines,
    replacement_text, validate_replace_mode,
};
use crate::plan::{self, Operation, Plan};
use crate::selector;

use crate::write::{WritePolicy, apply_policy, atomic_create_new, atomic_write};
use anyhow::Context;
#[cfg(feature = "cli")]
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use std::path::{Path, PathBuf};

#[allow(dead_code)]
const _LIBRARY_TX_DEAD_CODE_ALLOW: () = ();

/// JSON output for the tx command.
#[derive(Serialize)]
struct TxOutput {
    ok: bool,
    status: &'static str,
    files_changed: usize,
    files_created: usize,
    files_deleted: usize,
    changes: Vec<TxChange>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reads: Vec<TxReadResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    searches: Vec<TxSearchResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lints: Vec<TxLintResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

/// A single file change in the tx output.
#[derive(Serialize)]
struct TxChange {
    path: String,
    action: &'static str,
}

/// A search match in the tx output.
#[derive(Serialize)]
struct TxSearchMatch {
    line: usize,
    column: usize,
    text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    context_after: Vec<String>,
}

/// A search result in the tx output.
#[derive(Serialize)]
struct TxSearchResult {
    path: String,
    pattern: String,
    match_count: usize,
    matches: Vec<TxSearchMatch>,
}

/// A file read result in the tx output.
#[derive(Serialize)]
struct TxReadResult {
    path: String,
    content: String,
    start_line: usize,
    end_line: usize,
    total_lines: usize,
}

/// A lint result in the tx output.
#[derive(Serialize)]
struct TxLintResult {
    path: String,
    issue_count: usize,
    issues: Vec<crate::ops::md::LintIssue>,
}

#[cfg(feature = "cli")]
#[derive(Debug, Args)]
#[non_exhaustive]
#[command(after_help = "\
EXAMPLES:
  patchloom tx plan.json
  patchloom tx plan.json --apply
  patchloom tx plan.yaml --check --json")]
pub struct TxArgs {
    // ref:tx-mode:plan-stdin
    /// Path to a plan file (JSON/YAML/TOML), or `-` for stdin.
    pub plan: String,
    // ref:tx-mode:plan-yaml
    /// Plan format when reading from stdin (json, yaml, toml). Auto-detected from file extension otherwise.
    #[arg(long)]
    pub plan_format: Option<String>,
    /// Disable strict rollback on format/validate failure.
    #[arg(long)]
    pub no_strict: bool,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

const DEFAULT_LIFECYCLE_TIMEOUT_SECS: u64 = 60;
use crate::exec;

/// Short label for an operation, used in error messages.
fn op_label(op: &Operation) -> &'static str {
    match op {
        Operation::Replace { .. } => "replace",
        Operation::DocSet { .. } => "doc.set",
        Operation::DocDelete { .. } => "doc.delete",
        Operation::DocMerge { .. } => "doc.merge",
        Operation::DocAppend { .. } => "doc.append",
        Operation::DocPrepend { .. } => "doc.prepend",
        Operation::DocUpdate { .. } => "doc.update",
        Operation::DocMove { .. } => "doc.move",
        Operation::DocEnsure { .. } => "doc.ensure",
        Operation::DocDeleteWhere { .. } => "doc.delete_where",
        Operation::MdReplaceSection { .. } => "md.replace_section",
        Operation::MdInsertAfterHeading { .. } => "md.insert_after_heading",
        Operation::MdInsertBeforeHeading { .. } => "md.insert_before_heading",
        Operation::MdUpsertBullet { .. } => "md.upsert_bullet",
        Operation::MdTableAppend { .. } => "md.table_append",
        Operation::MdMoveSection { .. } => "md.move_section",
        Operation::MdDedupeHeadings { .. } => "md.dedupe_headings",
        Operation::TidyFix { .. } => "tidy.fix",
        Operation::FileAppend { .. } => "file.append",
        Operation::FileCreate { .. } => "file.create",
        Operation::FileDelete { .. } => "file.delete",
        Operation::FileRename { .. } => "file.rename",
        Operation::PatchApply { .. } => "patch.apply",
        Operation::Read { .. } => "read",
        Operation::Search { .. } => "search",
        Operation::MdLintAgents { .. } => "md.lint_agents",
        #[cfg(feature = "ast")]
        Operation::AstRename { .. } => "ast.rename",
        #[cfg(feature = "ast")]
        Operation::AstReplace { .. } => "ast.replace",
    }
}

fn validate_operation(op: &Operation) -> anyhow::Result<()> {
    match op {
        Operation::Replace {
            from,
            to,
            insert_before,
            insert_after,
            nth,
            ..
        } => {
            if from.is_empty() {
                anyhow::bail!("replace operation requires a non-empty search pattern");
            }
            if *nth == Some(0) {
                anyhow::bail!("replace nth is 1-based; use 1 for the first occurrence");
            }
            match validate_replace_mode(
                to.is_some(),
                insert_before.is_some(),
                insert_after.is_some(),
            ) {
                Ok(()) => Ok(()),
                Err(ReplaceModeError::MissingMode) => {
                    anyhow::bail!(
                        "replace operation requires one of to, insert_before, or insert_after"
                    )
                }
                Err(ReplaceModeError::BothInsertModes) => {
                    anyhow::bail!("insert_before and insert_after cannot both be set")
                }
                Err(ReplaceModeError::ToWithInsert) => {
                    anyhow::bail!("to cannot be combined with insert_before or insert_after")
                }
            }
        }
        // Exhaustive match ensures the compiler flags new variants that may
        // need validation constraints.
        Operation::DocSet { .. }
        | Operation::DocDelete { .. }
        | Operation::DocMerge { .. }
        | Operation::DocAppend { .. }
        | Operation::DocPrepend { .. }
        | Operation::DocUpdate { .. }
        | Operation::DocMove { .. }
        | Operation::DocEnsure { .. }
        | Operation::DocDeleteWhere { .. }
        | Operation::MdReplaceSection { .. }
        | Operation::MdInsertAfterHeading { .. }
        | Operation::MdInsertBeforeHeading { .. }
        | Operation::MdUpsertBullet { .. }
        | Operation::MdTableAppend { .. }
        | Operation::MdDedupeHeadings { .. }
        | Operation::TidyFix { .. }
        | Operation::FileAppend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. }
        | Operation::Read { .. }
        | Operation::MdLintAgents { .. }
        | Operation::PatchApply { .. } => Ok(()),
        #[cfg(feature = "ast")]
        Operation::AstRename { .. } | Operation::AstReplace { .. } => Ok(()),
        Operation::MdMoveSection { before, after, .. } => {
            if before.is_none() && after.is_none() {
                anyhow::bail!("md.move_section requires either 'before' or 'after'");
            }
            if before.is_some() && after.is_some() {
                anyhow::bail!("md.move_section: 'before' and 'after' cannot both be set");
            }
            Ok(())
        }
        Operation::Search {
            invert_match,
            multiline,
            ..
        } => {
            if *invert_match && *multiline {
                anyhow::bail!("search: invert_match and multiline cannot be combined");
            }
            Ok(())
        }
    }
}

fn validate_plan_operations(plan: &Plan) -> anyhow::Result<()> {
    for op in &plan.operations {
        validate_operation(op)?;
    }

    Ok(())
}

fn parse_selector(input: &str) -> anyhow::Result<selector::Selector> {
    selector::parse_anyhow(input)
}

// ---------------------------------------------------------------------------
// Pending file changes
// ---------------------------------------------------------------------------

/// Read file content from the pending map or from disk.
///
/// When a file is first loaded from disk (Vacant entry), it is recorded in
/// `existed_before` so the commit/rollback phase knows it was pre-existing.
fn read_file_content<'a>(
    pending: &'a mut HashMap<PathBuf, (String, String)>,
    existed_before: &mut HashSet<PathBuf>,
    path: &Path,
) -> anyhow::Result<&'a str> {
    match pending.entry(path.to_path_buf()) {
        Entry::Occupied(entry) => Ok(&entry.into_mut().1),
        Entry::Vacant(entry) => {
            // Callers pass already-resolved paths (cwd.join(rel_path)),
            // so read directly without double-joining (#385).
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
            existed_before.insert(path.to_path_buf());
            Ok(&entry.insert((content.clone(), content)).1)
        }
    }
}

/// Read file bytes from disk, check for binary content, and if text, populate
/// the pending map. Returns `Ok(true)` if the file is text (content stored in
/// pending), `Ok(false)` if binary (skipped). This avoids the double-read that
/// occurs when `is_binary_file` probes 8 KiB and then `read_file_content`
/// re-reads the full file.
fn read_and_probe(
    pending: &mut HashMap<PathBuf, (String, String)>,
    existed_before: &mut HashSet<PathBuf>,
    path: &Path,
) -> anyhow::Result<bool> {
    if pending.contains_key(path) {
        return Ok(true); // already loaded, not binary
    }
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    if crate::files::is_binary(&bytes) {
        return Ok(false);
    }
    // Skip files that pass the binary check but contain invalid UTF-8
    // (e.g., bare continuation bytes without NUL). This matches standalone
    // search/replace behavior which silently skips such files.
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    existed_before.insert(path.to_path_buf());
    pending.insert(path.to_path_buf(), (content.clone(), content));
    Ok(true)
}

/// Update the current content for a file in the pending map.
/// If the file was previously marked for deletion, unmark it (the latest
/// intent is to write, not delete).
fn update_file_content(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    path: &Path,
    new_content: String,
) {
    deletions.remove(path);
    if let Some((_, current)) = pending.get_mut(path) {
        *current = new_content;
    } else {
        pending.insert(path.to_path_buf(), (String::new(), new_content));
    }
}

// ---------------------------------------------------------------------------
// Operation execution
// ---------------------------------------------------------------------------

/// Flush a single cached document back to the pending text map.
fn flush_doc_cache_entry(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    path: PathBuf,
    cached: CachedDoc,
) -> anyhow::Result<()> {
    let new_content = serialize_value_preserving(
        &cached.original_text,
        &cached.old_value,
        &cached.value,
        &cached.format,
    )
    .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    update_file_content(pending, deletions, &path, new_content);
    Ok(())
}

/// Flush all cached documents back to the pending text map. Called before
/// the write phase or when a non-doc operation needs the current text.
fn flush_doc_cache(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    doc_cache: &mut HashMap<PathBuf, CachedDoc>,
) -> anyhow::Result<()> {
    for (path, cached) in doc_cache.drain() {
        flush_doc_cache_entry(pending, deletions, path, cached)?;
    }
    Ok(())
}

/// Apply a markdown heading operation (read file, transform, write back).
///
/// Five of the six md operations follow an identical pattern: resolve path,
/// read content, call a `(&str, &str, &str) -> Option<String>` transform,
/// error on `None`, and update pending state. This helper captures that pattern.
fn apply_md_heading_op(
    tx: &mut TxState<'_>,
    path: &str,
    heading: &str,
    extra: &str,
    op: impl FnOnce(&str, &str, &str) -> Option<String>,
    err_label: &str,
) -> anyhow::Result<()> {
    let file_path = tx.cwd.join(path);
    let file_content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
    let new_content = op(file_content, heading, extra)
        .ok_or_else(|| anyhow::anyhow!("{err_label} not found: {heading}"))?;
    update_file_content(tx.pending, tx.deletions, &file_path, new_content);
    Ok(())
}

/// Wrap an error with the file path for context.
///
/// This replaces 27 identical `.map_err(path_err(path))` calls
/// throughout `execute_operation` and `get_doc_root`.
fn path_err<E: std::fmt::Display>(path: &str) -> impl FnOnce(E) -> anyhow::Error + '_ {
    move |e| anyhow::anyhow!("{path}: {e}")
}

/// Load a structured document from the cache (or parse from pending buffer)
/// and return a mutable reference to the parsed JSON value. Serialization is
/// deferred until the cache is flushed.
///
/// Callers mutate the returned `&mut Value` directly using borrowed references
/// from the `Operation` match arm, eliminating the need to clone `String` and
/// `Value` fields into a closure.
fn get_doc_root<'a>(
    pending: &mut HashMap<PathBuf, (String, String)>,
    existed_before: &mut HashSet<PathBuf>,
    doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    path: &str,
    cwd: &Path,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let file_path = cwd.join(path);

    // Ensure the document is in the cache.
    if !doc_cache.contains_key(&file_path) {
        let content = read_file_content(pending, existed_before, &file_path)?;
        let format = detect_format(path).map_err(path_err(path))?;
        let root = parse_doc(content, &format).map_err(path_err(path))?;
        let old_value = match format {
            FileFormat::Json => serde_json::Value::Null,
            _ => root.clone(),
        };
        let original_text = content.to_owned();
        doc_cache.insert(
            file_path.clone(),
            CachedDoc {
                value: root,
                format,
                original_text,
                old_value,
            },
        );
    }

    Ok(&mut doc_cache
        .get_mut(&file_path)
        .expect("just inserted into doc_cache")
        .value)
}

/// Cached parsed document for avoiding redundant parse-serialize cycles when
/// multiple doc.* operations target the same file in a single transaction.
struct CachedDoc {
    value: serde_json::Value,
    format: FileFormat,
    /// The text content at the time the document was first parsed.
    /// Used as `original_content` for comment-preserving serialization.
    original_text: String,
    /// Snapshot of the value at parse time; needed by YAML/TOML comment
    /// preservation. Null for JSON (#224).
    old_value: serde_json::Value,
}

/// Mutable transaction state passed through operation execution.
struct TxState<'a> {
    pending: &'a mut HashMap<PathBuf, (String, String)>,
    deletions: &'a mut HashSet<PathBuf>,
    existed_before: &'a mut HashSet<PathBuf>,
    doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    tx_reads: &'a mut Vec<TxReadResult>,
    tx_searches: &'a mut Vec<TxSearchResult>,
    tx_lints: &'a mut Vec<TxLintResult>,
    replace_hint: Option<String>,
    cwd: &'a Path,
    quiet: bool,
    structured: bool,
}

/// Execute a replace operation within a transaction.
fn execute_replace_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    let Operation::Replace {
        glob,
        path,
        mode,
        from,
        to,
        nth,
        insert_before,
        insert_after,
        case_insensitive,
        multiline,
        whole_line,
        range,
        before_context,
        after_context,
        ..
    } = op
    else {
        unreachable!()
    };
    let regex_mode = mode.as_deref() == Some("regex");
    let word_boundary = matches!(
        op,
        Operation::Replace {
            word_boundary: true,
            ..
        }
    );
    let use_regex = regex_mode || *case_insensitive || word_boundary;
    let replacement = replacement_text(from, to, insert_before, insert_after, use_regex);
    let compiled_re = compile_replace_regex(
        from,
        regex_mode,
        *case_insensitive,
        *multiline,
        word_boundary,
    )?;
    let parsed_range = range
        .as_deref()
        .map(crate::ops::read::parse_line_range)
        .transpose()?;

    if let Some(p) = path {
        let file_path = tx.cwd.join(p);
        let content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
        let (replaced, match_count) = if *whole_line {
            replace_whole_lines(
                content,
                from,
                &replacement,
                compiled_re.as_ref(),
                *nth,
                parsed_range,
            )
        } else {
            replace_content(content, from, &replacement, compiled_re.as_ref(), *nth)
        };
        if match_count > 0 {
            let owned = replaced.into_owned();
            update_file_content(tx.pending, tx.deletions, &file_path, owned);
            Ok(match_count)
        } else if !regex_mode && (before_context.is_some() || after_context.is_some()) {
            // Tier 3: Use context-based fallback when exact match fails.
            match crate::fallback::resolve_with_fallback(
                content,
                from,
                before_context.as_deref(),
                after_context.as_deref(),
            ) {
                Ok(anchor) => {
                    let to_text = to.as_deref().unwrap_or("");
                    let new_content = format!(
                        "{}{}{}",
                        &content[..anchor.start_offset],
                        to_text,
                        &content[anchor.start_offset + anchor.matched_text.len()..]
                    );
                    update_file_content(tx.pending, tx.deletions, &file_path, new_content);
                    tx.replace_hint = Some(format!(
                        "fallback matched via {:?} strategy in {}",
                        anchor.strategy, p,
                    ));
                    Ok(1)
                }
                Err(edit_error) => {
                    tx.replace_hint = Some(edit_error.message.clone());
                    Ok(0)
                }
            }
        } else {
            if !regex_mode {
                // Tier 1: Provide "did you mean?" hints for literal no-match.
                let similar = crate::fallback::find_similar_targets(content, from, 3);
                if !similar.is_empty() {
                    tx.replace_hint = Some(format!(
                        "no matches for '{}' in {} (did you mean: {}?)",
                        crate::fallback::truncate_str(from, 60),
                        p,
                        similar.join(", ")
                    ));
                }
            }
            Ok(0)
        }
    } else if let Some(pattern) = glob {
        let matcher = Glob::new(pattern)?.compile_matcher();
        let matches_pattern = |path: &Path| {
            matcher.is_match(path)
                || path.file_name().is_some_and(|name| matcher.is_match(name))
                || path.strip_prefix(tx.cwd).ok().is_some_and(|relative| {
                    !relative.as_os_str().is_empty()
                        && (matcher.is_match(relative)
                            || relative
                                .file_name()
                                .is_some_and(|name| matcher.is_match(name)))
                })
        };
        let mut total_matches = 0usize;
        let mut candidate_paths = Vec::new();
        let mut seen_paths = HashSet::new();

        let walker = WalkBuilder::new(tx.cwd).build();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let file_path = entry.path().to_path_buf();
            if matches_pattern(&file_path) && seen_paths.insert(file_path.clone()) {
                candidate_paths.push(file_path);
            }
        }

        for pending_path in tx.pending.keys() {
            if !pending_path.starts_with(tx.cwd)
                || pending_path.exists()
                || tx.deletions.contains(pending_path)
                || !matches_pattern(pending_path)
            {
                continue;
            }
            if seen_paths.insert(pending_path.clone()) {
                candidate_paths.push(pending_path.clone());
            }
        }

        for file_path in candidate_paths {
            let content = match read_file_content(tx.pending, tx.existed_before, &file_path) {
                Ok(c) => c.to_owned(),
                Err(e) => {
                    if !tx.structured && !tx.quiet {
                        eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                    }
                    continue;
                }
            };
            let (replaced, match_count) = if *whole_line {
                replace_whole_lines(
                    &content,
                    from,
                    &replacement,
                    compiled_re.as_ref(),
                    *nth,
                    parsed_range,
                )
            } else {
                replace_content(&content, from, &replacement, compiled_re.as_ref(), *nth)
            };
            total_matches += match_count;
            if match_count > 0 {
                update_file_content(tx.pending, tx.deletions, &file_path, replaced.into_owned());
            }
        }
        Ok(total_matches)
    } else {
        anyhow::bail!("replace operation requires either 'path' or 'glob'");
    }
}

/// Execute a read operation within a transaction.
fn execute_read_op(path: &str, lines: &Option<String>, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let file_path = tx.cwd.join(path);
    // Ensure file is loaded into pending (mutable borrow), then release it.
    read_file_content(tx.pending, tx.existed_before, &file_path)?;
    // Re-borrow immutably to avoid cloning the entire file content.
    let content = &tx.pending[&file_path].1;
    // Fast path: no line range requested, preserve raw content exactly and avoid
    // rebuilding lines. This matches the standalone `read` command contract.
    if lines.is_none() {
        let total_lines = content.lines().count();
        let start_line = if total_lines == 0 { 0 } else { 1 };
        tx.tx_reads.push(TxReadResult {
            path: path.to_string(),
            content: content.clone(),
            start_line,
            end_line: total_lines,
            total_lines,
        });
        return Ok(());
    }

    let selected = {
        let spec = lines.as_ref().expect("checked is_none above");
        let range = crate::ops::read::parse_line_range(spec)?;
        crate::ops::read::select_lines(content, range)
    };

    tx.tx_reads.push(TxReadResult {
        path: path.to_string(),
        content: selected.content,
        start_line: selected.start_line,
        end_line: selected.end_line,
        total_lines: selected.total_lines,
    });
    Ok(())
}

/// Execute a search operation within a transaction.
///
/// If `path` is a directory, walks it (respecting `.gitignore`) and searches
/// each non-binary file, mirroring the standalone `search` command behavior.
fn execute_search_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let Operation::Search {
        path,
        pattern,
        regex,
        case_insensitive,
        multiline,
        invert_match,
        context,
        before_context,
        after_context,
        assert_count,
    } = op
    else {
        unreachable!()
    };

    if *invert_match && *multiline {
        anyhow::bail!("invert_match and multiline cannot be combined");
    }

    let re = if *regex {
        let mut builder = RegexBuilder::new(pattern);
        builder.case_insensitive(*case_insensitive);
        builder.dot_matches_new_line(*multiline);
        builder.build()?
    } else {
        let escaped = regex::escape(pattern);
        let mut builder = RegexBuilder::new(&escaped);
        builder.case_insensitive(*case_insensitive);
        builder.dot_matches_new_line(*multiline);
        builder.build()?
    };

    let ctx_before = before_context.or(*context).unwrap_or(0);
    let ctx_after = after_context.or(*context).unwrap_or(0);

    let resolved = tx.cwd.join(path);
    let file_paths: Vec<PathBuf> = if resolved.is_dir() {
        let mut paths = Vec::new();
        for entry in WalkBuilder::new(&resolved).build() {
            let entry = entry?;
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                paths.push(entry.into_path());
            }
        }
        paths.sort();
        paths
    } else {
        vec![resolved]
    };

    let mut all_matches = Vec::new();

    for file_path in &file_paths {
        // For directory walks, read the file once and check for binary content
        // in the same buffer (avoids double-reading text files: 8 KiB probe +
        // full re-read). For single-file paths, skip the binary check.
        if file_paths.len() > 1 {
            if !read_and_probe(tx.pending, tx.existed_before, file_path)? {
                continue; // binary file, skip
            }
        } else {
            read_file_content(tx.pending, tx.existed_before, file_path)?;
        }
        let content = &tx.pending[file_path].1;
        let lines: Vec<&str> = content.lines().collect();

        let is_multi_file = file_paths.len() > 1;
        if *multiline {
            // Multiline mode: search the full content so patterns can span lines.
            for m in re.find_iter(content) {
                let line_idx = content[..m.start()].matches('\n').count();
                let start = line_idx.saturating_sub(ctx_before);
                let end = (line_idx + 1 + ctx_after).min(lines.len());
                let matched_text = m.as_str().to_string();
                let text = if is_multi_file {
                    let display_path = file_path
                        .strip_prefix(tx.cwd)
                        .unwrap_or(file_path)
                        .to_string_lossy();
                    format!("{display_path}:{matched_text}")
                } else {
                    matched_text
                };
                let col = m.start() - content[..m.start()].rfind('\n').map_or(0, |p| p + 1);
                all_matches.push(TxSearchMatch {
                    line: line_idx + 1,
                    column: col + 1,
                    text,
                    context_before: lines[start..line_idx]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                    context_after: lines[line_idx + 1..end]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                });
            }
        } else {
            for (i, line) in lines.iter().enumerate() {
                let found = re.find(line);
                let is_match = if *invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };
                if is_match {
                    let start = i.saturating_sub(ctx_before);
                    let end = (i + 1 + ctx_after).min(lines.len());
                    let text = if is_multi_file {
                        let display_path = file_path
                            .strip_prefix(tx.cwd)
                            .unwrap_or(file_path)
                            .to_string_lossy();
                        format!("{display_path}:{}", line)
                    } else {
                        line.to_string()
                    };
                    let column = found.map_or(1, |m| m.start() + 1);
                    all_matches.push(TxSearchMatch {
                        line: i + 1,
                        column,
                        text,
                        context_before: lines[start..i].iter().map(|s| s.to_string()).collect(),
                        context_after: lines[i + 1..end].iter().map(|s| s.to_string()).collect(),
                    });
                }
            }
        }
    }

    if let Some(expected) = assert_count
        && all_matches.len() != *expected
    {
        anyhow::bail!(
            "search assert_count: expected {} matches for '{}' in {}, found {}",
            expected,
            pattern,
            path,
            all_matches.len()
        );
    }

    tx.tx_searches.push(TxSearchResult {
        path: path.clone(),
        pattern: pattern.clone(),
        match_count: all_matches.len(),
        matches: all_matches,
    });
    Ok(())
}

/// Returns true if the operation works on raw text and needs any cached
/// doc values flushed (serialized) back to the pending text map first.
fn op_needs_doc_flush(op: &Operation) -> bool {
    matches!(
        op,
        Operation::Replace { .. }
            | Operation::MdReplaceSection { .. }
            | Operation::MdInsertAfterHeading { .. }
            | Operation::MdInsertBeforeHeading { .. }
            | Operation::MdUpsertBullet { .. }
            | Operation::MdTableAppend { .. }
            | Operation::MdMoveSection { .. }
            | Operation::MdDedupeHeadings { .. }
            | Operation::PatchApply { .. }
            | Operation::FileAppend { .. }
            | Operation::Read { .. }
            | Operation::Search { .. }
            | Operation::MdLintAgents { .. }
            | Operation::TidyFix { .. }
    ) || {
        #[cfg(feature = "ast")]
        {
            matches!(
                op,
                Operation::AstRename { .. } | Operation::AstReplace { .. }
            )
        }
        #[cfg(not(feature = "ast"))]
        {
            false
        }
    }
}

fn execute_doc_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    match op {
        Operation::DocSet {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            set_at_path(root, &sel, value.clone()).map_err(path_err(path))?;
        }

        Operation::DocDelete { path, selector } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            delete_at_selector(root, &sel).map_err(path_err(path))?;
        }

        Operation::DocMerge { path, value } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            deep_merge(root, value);
        }

        Operation::DocAppend {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            let target = navigate_mut(root, &sel, false).map_err(path_err(path))?;
            target
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("{path}: target is not an array"))?
                .push(value.clone());
        }

        Operation::DocPrepend {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            let target = navigate_mut(root, &sel, false).map_err(path_err(path))?;
            target
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("{path}: target is not an array"))?
                .insert(0, value.clone());
        }

        Operation::DocUpdate {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            let count = update_matching(root, &sel, value);
            if count == 0 {
                anyhow::bail!("{path}: no matching nodes found for selector '{selector}'");
            }
        }

        Operation::DocMove { path, from, to } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let from_sel = parse_selector(from).map_err(path_err(path))?;
            let to_sel = parse_selector(to).map_err(path_err(path))?;
            move_at_path(root, &from_sel, &to_sel).map_err(path_err(path))?;
        }

        Operation::DocEnsure {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            // If the path already exists, no-op.
            if selector::eval(root, &sel).is_empty() {
                set_at_path(root, &sel, value.clone()).map_err(path_err(path))?;
            }
        }

        Operation::DocDeleteWhere {
            path,
            selector,
            predicate,
        } => {
            let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
                .map_err(path_err(path))?;
            let sel = parse_selector(selector).map_err(path_err(path))?;
            delete_where(root, &sel, predicate).map_err(path_err(path))?;
        }

        _ => unreachable!("execute_doc_op called with non-doc operation"),
    }
    Ok(())
}

fn execute_file_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::FileAppend { path, content } => {
            let file_path = tx.cwd.join(path);
            if !tx.deletions.contains(&file_path)
                && !file_path.exists()
                && !tx.pending.contains_key(&file_path)
            {
                anyhow::bail!("file does not exist: {path}");
            }
            let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let mut combined = existing.to_string();
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(content);
            update_file_content(tx.pending, tx.deletions, &file_path, combined);
        }

        Operation::FileCreate {
            path,
            content,
            force,
        } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                anyhow::bail!("target is not a file: {path}");
            }
            if force.unwrap_or(false) {
                if tx.pending.contains_key(&file_path) || file_path.exists() {
                    let _ = read_file_content(tx.pending, tx.existed_before, &file_path)?;
                }
                update_file_content(tx.pending, tx.deletions, &file_path, content.clone());
            } else {
                let exists_in_tx =
                    tx.pending.contains_key(&file_path) && !tx.deletions.contains(&file_path);
                if exists_in_tx || (!tx.deletions.contains(&file_path) && file_path.exists()) {
                    anyhow::bail!("file already exists: {path}");
                }
                update_file_content(tx.pending, tx.deletions, &file_path, content.clone());
            }
        }

        Operation::FileDelete { path } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                anyhow::bail!("target is not a file: {path}");
            }
            let created_in_tx = match tx.pending.get(&file_path) {
                Some((original, _)) => original.is_empty() && !file_path.exists(),
                None => {
                    let _ = read_file_content(tx.pending, tx.existed_before, &file_path)?;
                    false
                }
            };

            if created_in_tx {
                tx.pending.remove(&file_path);
                tx.deletions.remove(&file_path);
            } else {
                update_file_content(tx.pending, tx.deletions, &file_path, String::new());
                tx.deletions.insert(file_path);
            }
        }

        Operation::FileRename { from, to, force } => {
            let src_path = tx.cwd.join(from);
            let dst_path = tx.cwd.join(to);

            if src_path.exists() && !src_path.is_file() {
                anyhow::bail!("source is not a file: {from}");
            }
            if dst_path.exists() && !dst_path.is_file() {
                anyhow::bail!("destination is not a file: {to}");
            }

            // If source and destination resolve to the same file, no-op.
            if src_path == dst_path
                || matches!(
                    (src_path.canonicalize(), dst_path.canonicalize()),
                    (Ok(ref s), Ok(ref d)) if s == d
                )
            {
                return Ok(0);
            }

            // Read source content into pending (validates it exists).
            let content = read_file_content(tx.pending, tx.existed_before, &src_path)?.to_string();

            // Check destination does not already exist (unless force).
            if !force {
                let dst_exists = (tx.pending.contains_key(&dst_path)
                    && !tx.deletions.contains(&dst_path))
                    || (!tx.deletions.contains(&dst_path) && dst_path.exists());
                if dst_exists {
                    anyhow::bail!("destination already exists: {to}");
                }
            }

            // If destination exists on disk, load it into pending first so
            // existed_before is populated and commit uses atomic_write (not
            // atomic_create_new which would fail on existing files).
            if *force && !tx.pending.contains_key(&dst_path) && dst_path.exists() {
                let _ = read_file_content(tx.pending, tx.existed_before, &dst_path)?;
            }

            // Write content to destination.
            update_file_content(tx.pending, tx.deletions, &dst_path, content);

            // Delete source (same logic as file.delete for tx-created files).
            let created_in_tx = match tx.pending.get(&src_path) {
                Some((original, _)) => original.is_empty() && !src_path.exists(),
                None => false,
            };
            if created_in_tx {
                tx.pending.remove(&src_path);
                tx.deletions.remove(&src_path);
            } else {
                update_file_content(tx.pending, tx.deletions, &src_path, String::new());
                tx.deletions.insert(src_path);
            }
        }

        _ => unreachable!("execute_file_op called with non-file operation"),
    }
    Ok(0)
}

fn execute_operation(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    // Guard is enforced upfront in execute_plan_direct (for library plans) and via MCP pre-checks.
    // Single-op api::* uses ensure_contained inside write paths.
    // Per-op enforcement inside tx collect can be expanded later if needed.
    match op {
        Operation::Replace { .. } => {
            return execute_replace_op(op, tx);
        }

        Operation::DocSet { .. }
        | Operation::DocDelete { .. }
        | Operation::DocMerge { .. }
        | Operation::DocAppend { .. }
        | Operation::DocPrepend { .. }
        | Operation::DocUpdate { .. }
        | Operation::DocMove { .. }
        | Operation::DocEnsure { .. }
        | Operation::DocDeleteWhere { .. } => {
            execute_doc_op(op, tx)?;
        }

        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(tx, path, heading, content, replace_section_in, "heading")?;
        }

        Operation::MdInsertAfterHeading {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(
                tx,
                path,
                heading,
                content,
                insert_after_heading_in,
                "heading",
            )?;
        }

        Operation::MdInsertBeforeHeading {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(
                tx,
                path,
                heading,
                content,
                insert_before_heading_in,
                "heading",
            )?;
        }

        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            apply_md_heading_op(tx, path, heading, bullet, upsert_bullet_in, "heading")?;
        }

        Operation::MdTableAppend { path, heading, row } => {
            apply_md_heading_op(tx, path, heading, row, table_append_for_tx, "heading/table")?;
        }

        Operation::MdMoveSection {
            path,
            heading,
            to,
            before,
            after,
        } => {
            let position = match (before.as_deref(), after.as_deref()) {
                (Some(b), None) => ("before", b),
                (None, Some(a)) => ("after", a),
                _ => anyhow::bail!("md.move_section requires exactly one of 'before' or 'after'"),
            };
            let dest_path_str = to.as_deref().unwrap_or(path.as_str());
            let source_path = tx.cwd.join(path);
            let dest_path = tx.cwd.join(dest_path_str);
            let same_file = to.is_none()
                || matches!(
                    (source_path.canonicalize(), dest_path.canonicalize()),
                    (Ok(ref s), Ok(ref d)) if s == d
                );
            let source_content =
                read_file_content(tx.pending, tx.existed_before, &source_path)?.to_owned();
            let dest_content = if same_file {
                source_content.clone()
            } else {
                read_file_content(tx.pending, tx.existed_before, &dest_path)?.to_owned()
            };
            let (new_source, new_dest) =
                move_section_in(&source_content, heading, &dest_content, position, same_file)
                    .ok_or_else(|| {
                        anyhow::anyhow!("md.move_section: heading or target not found")
                    })?;
            update_file_content(tx.pending, tx.deletions, &source_path, new_source);
            if !same_file {
                update_file_content(tx.pending, tx.deletions, &dest_path, new_dest);
            }
        }

        Operation::MdDedupeHeadings { path } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let (new_content, _removed) = dedupe_headings_in(file_content);
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::TidyFix {
            path,
            ensure_final_newline,
            trim_trailing_whitespace,
            normalize_eol,
        } => {
            let file_path = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &file_path)?.to_owned();
            let policy = WritePolicy {
                ensure_final_newline: ensure_final_newline.unwrap_or(true),
                trim_trailing_whitespace: trim_trailing_whitespace.unwrap_or(false),
                normalize_eol: if let Some(eol) = normalize_eol {
                    plan_normalize_eol(eol)?
                } else {
                    EolMode::Keep
                },
                collapse_blanks: false,
            };
            let new = crate::write::apply_policy(&content, &policy);
            if content != *new {
                update_file_content(tx.pending, tx.deletions, &file_path, new.into_owned());
            }
        }

        Operation::FileAppend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. } => {
            return execute_file_op(op, tx);
        }

        Operation::PatchApply {
            diff,
            on_stale,
            allow_conflicts,
        } => {
            let options = ApplyHunksOptions {
                on_stale: *on_stale,
                allow_conflicts: *allow_conflicts,
            };
            let patched_files = apply_patch_with_loader(
                diff,
                |path| {
                    let file_path = tx.cwd.join(path);
                    Ok(read_file_content(tx.pending, tx.existed_before, &file_path)?.to_string())
                },
                options,
            )?;
            for result in patched_files {
                if result.status == ApplyHunksStatus::Conflict && !allow_conflicts {
                    anyhow::bail!(
                        "patch apply: {} -- merge produced {} conflict(s); set allow_conflicts to write conflict markers",
                        result.path,
                        result.conflicts.len()
                    );
                }
                let file_path = tx.cwd.join(&result.path);
                update_file_content(tx.pending, tx.deletions, &file_path, result.content);
            }
        }

        Operation::Read { path, lines } => {
            execute_read_op(path, lines, tx)?;
        }

        Operation::Search { .. } => {
            execute_search_op(op, tx)?;
        }

        Operation::MdLintAgents { path } => {
            let file_path = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let issues = crate::ops::md::lint_agents_content(content);
            tx.tx_lints.push(TxLintResult {
                path: path.clone(),
                issue_count: issues.len(),
                issues,
            });
        }

        #[cfg(feature = "ast")]
        Operation::AstRename {
            path,
            old_name,
            new_name,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_extension)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let result =
                crate::ast::rename::rename_in_source(content, old_name, new_name, lang_val);
            match result {
                Some(r) if r.replacements > 0 => {
                    update_file_content(tx.pending, tx.deletions, &abs, r.content);
                    return Ok(r.replacements);
                }
                _ => {
                    // Fallback to word-boundary replace
                    let re = crate::ops::replace::compile_replace_regex(
                        old_name, false, false, false, true,
                    )?;
                    if let Some(re) = re {
                        let new_content = re.replace_all(content, new_name.as_str()).to_string();
                        let count = re.find_iter(content).count();
                        if count > 0 {
                            update_file_content(tx.pending, tx.deletions, &abs, new_content);
                            return Ok(count);
                        }
                    }
                    anyhow::bail!("no matches for '{}' in {}", old_name, path);
                }
            }
        }

        #[cfg(feature = "ast")]
        Operation::AstReplace {
            path,
            symbol,
            from,
            to,
            regex,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_extension)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let result = crate::ast::replace::replace_in_symbol(
                content, symbol, from, to, *regex, lang_val,
            )?;
            match result {
                Some(r) if r.replacements > 0 => {
                    update_file_content(tx.pending, tx.deletions, &abs, r.content);
                    return Ok(r.replacements);
                }
                Some(_) => anyhow::bail!(
                    "no matches for '{}' in symbol '{}' in {}",
                    from,
                    symbol,
                    path
                ),
                None => anyhow::bail!("symbol '{}' not found in {}", symbol, path),
            }
        }
    }

    Ok(0)
}

// ---------------------------------------------------------------------------
// Write policy
// ---------------------------------------------------------------------------

fn plan_normalize_eol(mode: &str) -> anyhow::Result<EolMode> {
    match mode {
        "lf" => Ok(EolMode::Lf),
        "crlf" => Ok(EolMode::Crlf),
        "keep" => Ok(EolMode::Keep),
        _ => {
            anyhow::bail!("invalid normalize_eol value '{mode}': expected 'lf', 'crlf', or 'keep'")
        }
    }
}

/// Build the effective write policy for a pending file.
///
/// Start from the CLI-derived per-file defaults, including any EditorConfig
/// values resolved by `policy_from_flags()`, then let plan-level `write_policy`
/// entries override only the keys they set.
fn build_write_policy(
    plan: &Plan,
    global: &GlobalFlags,
    path: &Path,
) -> anyhow::Result<WritePolicy> {
    let mut write_policy = crate::write::policy_from_flags(global, Some(path));
    let Some(plan_write_policy) = &plan.write_policy else {
        return Ok(write_policy);
    };

    if let Some(ensure_final_newline) = plan_write_policy.ensure_final_newline {
        write_policy.ensure_final_newline = ensure_final_newline;
    }
    if let Some(normalize_eol) = plan_write_policy.normalize_eol.as_deref() {
        write_policy.normalize_eol = plan_normalize_eol(normalize_eol)?;
    }
    if let Some(trim_trailing_whitespace) = plan_write_policy.trim_trailing_whitespace {
        write_policy.trim_trailing_whitespace = trim_trailing_whitespace;
    }
    if let Some(collapse_blanks) = plan_write_policy.collapse_blanks {
        write_policy.collapse_blanks = collapse_blanks;
    }

    Ok(write_policy)
}

// ---------------------------------------------------------------------------
// Diff output helper
// ---------------------------------------------------------------------------

fn build_tx_output(
    status: &'static str,
    ok: bool,
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> TxOutput {
    let mut tx_changes = Vec::new();
    let mut created = 0usize;
    let mut deleted_count = 0usize;
    let mut modified = 0usize;

    let display_path = |p: &Path| -> String {
        crate::files::relative_display(p, cwd)
            .to_string_lossy()
            .into_owned()
    };

    for (path, _original, _) in changes {
        let path_str = display_path(path);
        if deletions.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "deleted",
            });
            deleted_count += 1;
        } else if !existed_before.contains(path) {
            tx_changes.push(TxChange {
                path: path_str,
                action: "created",
            });
            created += 1;
        } else {
            tx_changes.push(TxChange {
                path: path_str,
                action: "modified",
            });
            modified += 1;
        }
    }
    // Deletions not captured in changes (empty files).
    for path in deletions {
        if !changes.iter().any(|(c, _, _)| c == path) {
            tx_changes.push(TxChange {
                path: display_path(path),
                action: "deleted",
            });
            deleted_count += 1;
        }
    }

    TxOutput {
        ok,
        status,
        files_changed: modified,
        files_created: created,
        files_deleted: deleted_count,
        changes: tx_changes,
        reads: Vec::new(),
        searches: Vec::new(),
        lints: Vec::new(),
        error_kind: None,
        error: None,
        backup_session: None,
    }
}

fn build_full_tx_output(status: &'static str, result: &mut TxExecResult, cwd: &Path) -> TxOutput {
    let mut output = build_tx_output(
        status,
        true,
        &result.changes,
        &result.deletions,
        &result.existed_before,
        cwd,
    );
    output.reads = std::mem::take(&mut result.tx_reads);
    output.searches = std::mem::take(&mut result.tx_searches);
    output.lints = std::mem::take(&mut result.tx_lints);
    output
}

fn emit_output_json(output: &TxOutput, compact: bool) {
    let result = if compact {
        serde_json::to_string(output)
    } else {
        serde_json::to_string_pretty(output)
    };
    if let Ok(json) = result {
        println!("{json}");
    }
}

fn format_error_with_backup_hint(error: &str, backup_session: Option<&str>) -> String {
    match backup_session {
        Some(ts) => format!("{error} (backup session {ts}; run `patchloom undo` to restore)"),
        None => error.to_string(),
    }
}

fn build_error_output(
    error_kind: &'static str,
    legacy_error_prefix: &str,
    error: &str,
    backup_session: Option<&str>,
) -> TxOutput {
    TxOutput {
        ok: false,
        status: "error",
        files_changed: 0,
        files_created: 0,
        files_deleted: 0,
        changes: Vec::new(),
        reads: Vec::new(),
        searches: Vec::new(),
        lints: Vec::new(),
        error_kind: Some(error_kind),
        error: Some(format!(
            "{legacy_error_prefix}: {}",
            format_error_with_backup_hint(error, backup_session)
        )),
        backup_session: backup_session.map(str::to_string),
    }
}

fn legacy_error_prefix(error_kind: &str) -> &str {
    if error_kind == "format_failed" {
        "validation_failed"
    } else {
        error_kind
    }
}

fn emit_error_json_with_prefix(
    error_kind: &'static str,
    legacy_error_prefix: &'static str,
    error: &str,
    backup_session: Option<&str>,
    compact: bool,
) {
    emit_output_json(
        &build_error_output(error_kind, legacy_error_prefix, error, backup_session),
        compact,
    );
}

fn emit_error_json(
    error_kind: &'static str,
    error: &str,
    backup_session: Option<&str>,
    compact: bool,
) {
    emit_error_json_with_prefix(
        error_kind,
        legacy_error_prefix(error_kind),
        error,
        backup_session,
        compact,
    );
}

fn describe_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".to_string(),
    }
}

fn describe_lifecycle_cwd(base_cwd: &Path, cwd: &Path) -> String {
    if cwd == base_cwd {
        ".".to_string()
    } else {
        crate::files::relative_display(cwd, base_cwd)
            .display()
            .to_string()
    }
}

fn print_diffs(changes: &[(PathBuf, String, String)], cwd: &Path, color: bool) {
    let diffs: Vec<_> = changes
        .iter()
        .map(|(p, old, new)| {
            let display = crate::files::relative_display(p, cwd);
            unified_diff(&display.to_string_lossy(), old, new)
        })
        .collect();
    let result = DiffResult { diffs };
    print!("{}", format_diff_result_colored(&result, color));
}

// ---------------------------------------------------------------------------
// Lifecycle helpers (extracted from run() for testability)
// ---------------------------------------------------------------------------

struct LifecycleError {
    message: String,
    kind: &'static str,
}

/// Format a lifecycle step failure message, appending stderr output if available.
fn lifecycle_failure_msg(header: &str, stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        header.to_string()
    } else {
        format!("{header}: {trimmed}")
    }
}

fn run_format_steps(
    steps: &[plan::FormatStep],
    base_cwd: &Path,
    cwd: &Path,
) -> Result<(), LifecycleError> {
    let lifecycle_cwd = describe_lifecycle_cwd(base_cwd, cwd);
    for (index, step) in steps.iter().enumerate() {
        let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
        let result = exec::run_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(exec::ShellResult {
                status,
                stderr_head,
            }) if !status.success() => {
                let header = format!(
                    "format step failed (step {}, {}, cwd: {})",
                    index + 1,
                    describe_exit_status(status),
                    lifecycle_cwd
                );
                let msg = lifecycle_failure_msg(&header, &stderr_head);
                eprintln!("tx: {msg}");
                return Err(LifecycleError {
                    message: msg,
                    kind: "format_failed",
                });
            }
            Err(e) => {
                let msg = format!(
                    "format step error (step {}, cwd: {}): {e}",
                    index + 1,
                    lifecycle_cwd
                );
                eprintln!("tx: {msg}");
                return Err(LifecycleError {
                    message: msg,
                    kind: "format_failed",
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn run_validate_steps(
    steps: &[plan::ValidationStep],
    base_cwd: &Path,
    cwd: &Path,
) -> Result<(), LifecycleError> {
    let lifecycle_cwd = describe_lifecycle_cwd(base_cwd, cwd);
    for (index, step) in steps.iter().enumerate() {
        let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
        let result = exec::run_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(exec::ShellResult {
                status,
                stderr_head,
            }) if status.success() => {
                let _ = stderr_head; // Discard stderr on success.
            }
            Ok(exec::ShellResult {
                status,
                stderr_head,
            }) => {
                let header = format!(
                    "required validation failed (step {}, {}, cwd: {})",
                    index + 1,
                    describe_exit_status(status),
                    lifecycle_cwd
                );
                let msg = lifecycle_failure_msg(&header, &stderr_head);
                eprintln!("tx: {msg}");
                if step.required.unwrap_or(false) {
                    return Err(LifecycleError {
                        message: msg,
                        kind: "validation_failed",
                    });
                }
            }
            Err(e) => {
                let msg = format!(
                    "validation error (step {}, cwd: {}): {e}",
                    index + 1,
                    lifecycle_cwd
                );
                eprintln!("tx: {msg}");
                if step.required.unwrap_or(false) {
                    return Err(LifecycleError {
                        message: msg,
                        kind: "validation_failed",
                    });
                }
            }
        }
    }
    Ok(())
}

fn rollback_strict(
    changes: &[(PathBuf, String, String)],
    pending: &HashMap<PathBuf, (String, String)>,
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
) {
    let noop_policy = WritePolicy::default();
    for (path, original, _) in changes {
        if !existed_before.contains(path) && !deletions.contains(path) {
            if let Err(e) = std::fs::remove_file(path) {
                eprintln!("tx: rollback: failed to remove {}: {e}", path.display());
            }
        } else if let Err(e) = atomic_write(path, original, &noop_policy) {
            eprintln!("tx: rollback: failed to restore {}: {e}", path.display());
        }
    }
    for path in deletions {
        if let Some((orig, _)) = pending.get(path)
            && existed_before.contains(path)
            && let Err(e) = atomic_write(path, orig, &noop_policy)
        {
            eprintln!(
                "tx: rollback: failed to restore deleted {}: {e}",
                path.display()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Shared execution core
// ---------------------------------------------------------------------------

/// Intermediate result from executing all operations in a plan and applying
/// write policy. Contains everything needed for callers to decide on output
/// mode, commit changes, and run lifecycle steps.
struct TxExecResult {
    changes: Vec<(PathBuf, String, String)>,
    deletions: HashSet<PathBuf>,
    existed_before: HashSet<PathBuf>,
    /// Original pending map, retained for `rollback_strict`.
    pending: HashMap<PathBuf, (String, String)>,
    tx_reads: Vec<TxReadResult>,
    tx_searches: Vec<TxSearchResult>,
    tx_lints: Vec<TxLintResult>,
    no_effective_changes: bool,
    replace_no_matches: bool,
    /// "Did you mean?" hints when a replace found zero matches.
    replace_hint: Option<String>,
}

/// Execute all plan operations in memory, apply write policy, and return
/// the collected results without touching the filesystem.
///
/// On operation failure the error message is pre-formatted as
/// `"operation N (label) failed: ..."` so callers can emit it directly.
fn execute_and_collect(
    plan: &Plan,
    cwd: &Path,
    global: &GlobalFlags,
    quiet: bool,
    structured: bool,
) -> anyhow::Result<TxExecResult> {
    let mut pending: HashMap<PathBuf, (String, String)> = HashMap::new();
    let mut deletions: HashSet<PathBuf> = HashSet::new();
    let mut existed_before: HashSet<PathBuf> = HashSet::new();
    let mut has_non_idempotent_replace = false;
    let mut total_replace_matches = 0usize;
    let mut tx_reads: Vec<TxReadResult> = Vec::new();
    let mut tx_searches: Vec<TxSearchResult> = Vec::new();
    let mut tx_lints: Vec<TxLintResult> = Vec::new();
    let mut doc_cache: HashMap<PathBuf, CachedDoc> = HashMap::new();
    let mut replace_hint: Option<String> = None;

    crate::verbose!(
        "tx: executing plan with {} operations",
        plan.operations.len()
    );
    for (i, op) in plan.operations.iter().enumerate() {
        crate::verbose!(
            "tx: operation {}/{}: {}",
            i + 1,
            plan.operations.len(),
            op_label(op)
        );
        if let Operation::Replace { if_exists, .. } = op
            && !if_exists
        {
            has_non_idempotent_replace = true;
        }
        if op_needs_doc_flush(op) {
            flush_doc_cache(&mut pending, &mut deletions, &mut doc_cache)?;
        }
        let mut tx = TxState {
            pending: &mut pending,
            deletions: &mut deletions,
            existed_before: &mut existed_before,
            doc_cache: &mut doc_cache,
            tx_reads: &mut tx_reads,
            tx_searches: &mut tx_searches,
            tx_lints: &mut tx_lints,
            replace_hint: None,
            cwd,
            quiet,
            structured,
        };
        match execute_operation(op, &mut tx) {
            Ok(count) => {
                crate::verbose!(
                    "tx: operation {} succeeded (replace_matches: {count})",
                    i + 1
                );
                total_replace_matches += count;
                if replace_hint.is_none() {
                    replace_hint = tx.replace_hint.take();
                }
            }
            Err(e) => {
                crate::verbose!("tx: operation {} failed: {e}", i + 1);
                anyhow::bail!("operation {} ({}) failed: {e}", i + 1, op_label(op));
            }
        }
    }

    flush_doc_cache(&mut pending, &mut deletions, &mut doc_cache)?;

    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, (original, current)) in &pending {
        let write_policy = build_write_policy(plan, global, path)?;
        let final_content = apply_policy(current, &write_policy);
        if *original != *final_content {
            changes.push((path.clone(), original.clone(), final_content.into_owned()));
        }
    }
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    let pending_deletions = deletions
        .iter()
        .filter(|p| !changes.iter().any(|(c, _, _)| c == *p))
        .count();
    let no_effective_changes = changes.is_empty() && pending_deletions == 0;
    let replace_no_matches =
        has_non_idempotent_replace && total_replace_matches == 0 && no_effective_changes;

    Ok(TxExecResult {
        changes,
        deletions,
        existed_before,
        pending,
        tx_reads,
        tx_searches,
        tx_lints,
        no_effective_changes,
        replace_no_matches,
        replace_hint,
    })
}

/// Run format and validation lifecycle steps. Returns `None` on success.
fn run_lifecycle(plan: &Plan, base_cwd: &Path, cwd: &Path) -> Option<LifecycleError> {
    plan.format
        .as_deref()
        .map(|steps| run_format_steps(steps, base_cwd, cwd))
        .unwrap_or(Ok(()))
        .err()
        .or_else(|| {
            plan.validate
                .as_deref()
                .and_then(|steps| run_validate_steps(steps, base_cwd, cwd).err())
        })
}

pub(crate) fn resolve_plan_cwd(base_cwd: &Path, plan_cwd: Option<&str>) -> PathBuf {
    match plan_cwd {
        Some(plan_cwd) => {
            let path = Path::new(plan_cwd);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base_cwd.join(path)
            }
        }
        None => base_cwd.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------
// Direct execution (MCP / in-process callers)
// ---------------------------------------------------------------------------

/// Failure while committing staged changes to disk.
#[derive(Debug)]
pub struct CommitError {
    pub message: String,
    pub rollback_ok: bool,
    pub backup_session: Option<String>,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommitError {}

fn commit_error(message: impl Into<String>) -> CommitError {
    CommitError {
        message: message.into(),
        rollback_ok: true,
        backup_session: None,
    }
}

thread_local! {
    /// Per-thread test hook; never set in production.
    static FORCE_RESTORE_FAIL: std::sync::atomic::AtomicBool =
        const { std::sync::atomic::AtomicBool::new(false) };
}

/// RAII guard that forces restore failure on the current thread only.
#[doc(hidden)]
pub struct RestoreFailGuard;

impl RestoreFailGuard {
    pub fn engage() -> Self {
        FORCE_RESTORE_FAIL.with(|flag| flag.store(true, std::sync::atomic::Ordering::SeqCst));
        Self
    }
}

impl Drop for RestoreFailGuard {
    fn drop(&mut self) {
        FORCE_RESTORE_FAIL.with(|flag| flag.store(false, std::sync::atomic::Ordering::SeqCst));
    }
}

/// Restore files from a backup session after a failed commit. Returns `true`
/// when restore completed successfully.
fn restore_after_failed_commit(cwd: &Path, timestamp: &str) -> bool {
    if FORCE_RESTORE_FAIL.with(|flag| flag.load(std::sync::atomic::Ordering::SeqCst)) {
        return false;
    }
    crate::backup::restore_session(cwd, timestamp).is_ok()
}

/// Apply pending changes to disk: backup originals, write modified files,
/// delete removed files, finalize backup session.
///
/// If any write fails, restores all already-written files from the backup
/// session before returning [`CommitError`].
fn commit_changes(
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> Result<(), CommitError> {
    let mut backup = crate::backup::BackupSession::new(cwd)
        .map_err(|e| commit_error(format!("starting backup session: {e}")))?;
    for (path, _, _) in changes {
        if deletions.contains(path) {
            backup
                .save_before_delete(path)
                .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
        } else {
            backup
                .save_before_write(path)
                .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
        }
    }
    for path in deletions {
        backup
            .save_before_delete(path)
            .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
    }

    // Finalize before writes so undo can recover from a mid-commit failure.
    let backup_session = backup
        .finalize()
        .map_err(|e| commit_error(format!("finalizing backup session: {e}")))?;

    let noop_policy = WritePolicy::default();
    let write_result = (|| -> anyhow::Result<()> {
        for (path, _, new_content) in changes {
            if deletions.contains(path) {
                std::fs::remove_file(path)
                    .with_context(|| format!("deleting {}", path.display()))?;
            } else {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                    && !parent.exists()
                {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating directory {}", parent.display()))?;
                }
                if !existed_before.contains(path) {
                    atomic_create_new(path, new_content, &noop_policy)?;
                } else {
                    atomic_write(path, new_content, &noop_policy)?;
                }
            }
        }
        for path in deletions {
            if path.exists() {
                std::fs::remove_file(path)
                    .with_context(|| format!("deleting {}", path.display()))?;
            }
        }
        Ok(())
    })();

    if let Err(e) = write_result {
        let rollback_ok = if let Some(ref ts) = backup_session {
            restore_after_failed_commit(cwd, ts)
        } else {
            true
        };
        return Err(CommitError {
            message: e.to_string(),
            rollback_ok,
            backup_session,
        });
    }

    Ok(())
}

/// Build a JSON error string without writing to stdout.
fn make_error_json(error_kind: &'static str, error: &str, backup_session: Option<&str>) -> String {
    make_error_json_with_prefix(
        error_kind,
        legacy_error_prefix(error_kind),
        error,
        backup_session,
    )
}

/// Build a JSON error string with an explicit legacy prefix.
fn make_error_json_with_prefix(
    error_kind: &'static str,
    legacy_error_prefix: &str,
    error: &str,
    backup_session: Option<&str>,
) -> String {
    serde_json::to_string_pretty(&build_error_output(
        error_kind,
        legacy_error_prefix,
        error,
        backup_session,
    ))
    .unwrap_or_default()
}

fn config_tx_strict(cwd: &Path) -> Option<bool> {
    crate::config::find_and_load(cwd)
        .map(|(config, _)| config.tx.strict)
        .unwrap_or(None)
}

fn handle_commit_error(err: CommitError, structured: bool, compact: bool) -> anyhow::Result<u8> {
    let error_kind = if err.rollback_ok {
        "rollback"
    } else {
        "rollback_failed"
    };
    let exit_code = if err.rollback_ok {
        exit::ROLLBACK
    } else {
        exit::FAILURE
    };
    let backup_session = err.backup_session.as_deref();
    if structured {
        emit_error_json_with_prefix(
            error_kind,
            error_kind,
            &err.message,
            backup_session,
            compact,
        );
    } else {
        let msg = format_error_with_backup_hint(&err.message, backup_session);
        eprintln!("tx: {msg}");
    }
    Ok(exit_code)
}

/// Execute a parsed [`Plan`] directly and return the exit code and JSON
/// result string. Does **not** write to stdout or stderr.
///
/// This is the in-process equivalent of spawning
/// `patchloom --json tx <plan-file> --apply` and capturing its output.
/// The MCP server and library API call this to avoid subprocess overhead.
/// Validate a plan and resolve its effective working directory, strictness, and
/// config-derived global flags. Returns `Err` with `(exit_code, json)` on
/// validation failure so callers can propagate early.
fn validate_and_prepare_plan(
    plan: &Plan,
    cwd: &Path,
    no_strict: bool,
) -> Result<(PathBuf, bool, GlobalFlags), (u8, String)> {
    if plan.version != crate::plan::SCHEMA_VERSION {
        let msg = format!(
            "unsupported plan version '{}' (this build supports version {})",
            plan.version,
            crate::plan::SCHEMA_VERSION
        );
        return Err((
            exit::PARSE_ERROR,
            make_error_json("parse_error", &msg, None),
        ));
    }
    if let Err(e) = validate_plan_operations(plan) {
        return Err((
            exit::PARSE_ERROR,
            make_error_json("parse_error", &e.to_string(), None),
        ));
    }

    let effective_cwd = resolve_plan_cwd(cwd, plan.cwd.as_deref());
    let config_strict = config_tx_strict(&effective_cwd);
    let strict = plan::effective_strict(plan.strict, config_strict, no_strict);

    let mut global = GlobalFlags {
        cwd: Some(effective_cwd.to_string_lossy().into_owned()),
        ..GlobalFlags::default()
    };
    if let Some((config, _)) = crate::config::find_and_load(&effective_cwd) {
        crate::config::apply_config(&mut global, &config);
    }

    Ok((effective_cwd, strict, global))
}

pub fn execute_plan_direct(
    plan: Plan,
    cwd: &Path,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<(u8, String)> {
    crate::verbose!(
        "tx: direct plan execution ({} ops, cwd={}, guard={})",
        plan.operations.len(),
        cwd.display(),
        guard.is_some()
    );

    let (effective_cwd, strict, global) = match validate_and_prepare_plan(&plan, cwd, false) {
        Ok(v) => v,
        Err((code, json)) => return Ok((code, json)),
    };

    // PathGuard enforcement for library callers of execute_plan (addresses #755).
    // Upfront check on declared paths using shared helper; dynamic (globs
    // patterns, patch embedded paths) are best-effort or handled by loaders.
    if let Some(g) = guard {
        for op in &plan.operations {
            for p in crate::plan::declared_paths(op) {
                g.check_path(p)
                    .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {}", e))?;
            }
        }
    }

    // Execute operations and collect changes in memory.
    let mut result = match execute_and_collect(&plan, &effective_cwd, &global, true, true) {
        Ok(r) => r,
        Err(e) => {
            return Ok((
                exit::OPERATION_FAILED,
                make_error_json("operation_failed", &e.to_string(), None),
            ));
        }
    };

    if result.replace_no_matches {
        let hint_json = result
            .replace_hint
            .as_deref()
            .map(|h| make_error_json("no_matches", h, None))
            .unwrap_or_default();
        return Ok((exit::NO_MATCHES, hint_json));
    }

    if result.no_effective_changes {
        let output = build_full_tx_output("success", &mut result, &effective_cwd);
        let json = serde_json::to_string_pretty(&output)?;
        return Ok((exit::SUCCESS, json));
    }

    // Apply: back up originals, write files.
    if let Err(err) = commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
    ) {
        let error_kind = if err.rollback_ok {
            "rollback"
        } else {
            "rollback_failed"
        };
        let exit_code = if err.rollback_ok {
            exit::ROLLBACK
        } else {
            exit::FAILURE
        };
        return Ok((
            exit_code,
            make_error_json_with_prefix(
                error_kind,
                error_kind,
                &err.message,
                err.backup_session.as_deref(),
            ),
        ));
    }

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd) {
        if strict {
            rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            let msg = format!("strict mode -- all changes reverted ({})", err.message);
            return Ok((
                exit::ROLLBACK,
                make_error_json_with_prefix(err.kind, "rollback", &msg, None),
            ));
        }
        return Ok((
            exit::VALIDATION_FAILED,
            make_error_json(err.kind, &err.message, None),
        ));
    }

    let output = build_full_tx_output("success", &mut result, &effective_cwd);
    let json = serde_json::to_string_pretty(&output)?;
    Ok((exit::SUCCESS, json))
}

// ---------------------------------------------------------------------------
