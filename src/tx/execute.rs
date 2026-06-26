use super::output::{TxExecResult, TxLintResult, TxReadResult, TxSearchResult};
use super::validate::op_label;
use crate::cli::global::{EolMode, GlobalFlags};
use crate::ops::doc::{
    FileFormat, MutationResult, apply_doc_mutation, detect_format, parse_doc,
    serialize_value_preserving,
};
use crate::ops::md::{
    dedupe_headings_in, insert_after_heading_in, insert_before_heading_in, move_section_in,
    replace_section_in, table_append_for_tx, upsert_bullet_in,
};
use crate::ops::patch::{ApplyHunksOptions, ApplyHunksStatus, apply_patch_with_loader};

use crate::plan::{Operation, Plan};
use crate::write::{WritePolicy, apply_policy};

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Pending file changes
// ---------------------------------------------------------------------------

/// Read file content from the pending map or from disk.
///
/// When a file is first loaded from disk (Vacant entry), it is recorded in
/// `existed_before` so the commit/rollback phase knows it was pre-existing.
pub(crate) fn read_file_content<'a>(
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
pub(crate) fn read_and_probe(
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
pub(crate) fn update_file_content(
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
pub(crate) fn flush_doc_cache_entry(
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
pub(crate) fn flush_doc_cache(
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
pub(crate) fn apply_md_heading_op(
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
pub(crate) fn path_err<E: std::fmt::Display>(path: &str) -> impl FnOnce(E) -> anyhow::Error + '_ {
    move |e| anyhow::anyhow!("{path}: {e}")
}

/// Load a structured document from the cache (or parse from pending buffer)
/// and return a mutable reference to the parsed JSON value. Serialization is
/// deferred until the cache is flushed.
///
/// Callers mutate the returned `&mut Value` directly using borrowed references
/// from the `Operation` match arm, eliminating the need to clone `String` and
/// `Value` fields into a closure.
pub(crate) fn get_doc_root<'a>(
    pending: &mut HashMap<PathBuf, (String, String)>,
    existed_before: &mut HashSet<PathBuf>,
    doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    path: &str,
    cwd: &Path,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let file_path = cwd.join(path);

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
pub(crate) struct CachedDoc {
    pub(crate) value: serde_json::Value,
    pub(crate) format: FileFormat,
    /// The text content at the time the document was first parsed.
    /// Used as `original_content` for comment-preserving serialization.
    pub(crate) original_text: String,
    /// Snapshot of the value at parse time; needed by YAML/TOML comment
    /// preservation. Null for JSON (#224).
    pub(crate) old_value: serde_json::Value,
}

/// Mutable transaction state passed through operation execution.
pub(crate) struct TxState<'a> {
    pub(crate) pending: &'a mut HashMap<PathBuf, (String, String)>,
    pub(crate) deletions: &'a mut HashSet<PathBuf>,
    pub(crate) existed_before: &'a mut HashSet<PathBuf>,
    pub(crate) doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    pub(crate) tx_reads: &'a mut Vec<TxReadResult>,
    pub(crate) tx_searches: &'a mut Vec<TxSearchResult>,
    pub(crate) tx_lints: &'a mut Vec<TxLintResult>,
    pub(crate) replace_hint: Option<String>,
    pub(crate) cwd: &'a Path,
    pub(crate) quiet: bool,
    pub(crate) structured: bool,
}

// execute_replace_op extracted to tx/replace_op.rs (#906).
pub(crate) use super::replace_op::execute_replace_op;

/// Execute a read operation within a transaction.
pub(crate) fn execute_read_op(
    path: &str,
    lines: &Option<String>,
    tx: &mut TxState<'_>,
) -> anyhow::Result<()> {
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

// execute_search_op extracted to tx/search_op.rs (#906).
pub(crate) use super::search_op::execute_search_op;

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

pub(crate) fn execute_doc_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let (path, mutation) =
        crate::plan::op_to_doc_mutation(op).expect("execute_doc_op called with non-doc operation");
    let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
        .map_err(path_err(path))?;

    // Delete and DeleteWhere are idempotent: a no-match is not an error
    // (the old code silently discarded the bool/count return value).
    // Update is strict: no-match means the selector was wrong.
    let strict_no_match = matches!(op, Operation::DocUpdate { .. });

    match apply_doc_mutation(root, mutation).map_err(path_err(path))? {
        MutationResult::Applied | MutationResult::AlreadyExists => Ok(()),
        MutationResult::NoMatch if strict_no_match => {
            let label = op_label(op);
            anyhow::bail!("{path}: {label} matched nothing");
        }
        MutationResult::NoMatch => Ok(()),
        MutationResult::TypeError(msg) => {
            anyhow::bail!("{path}: {msg}");
        }
    }
}

// op_to_doc_mutation moved to plan.rs as the single source of truth for
// Operation::Doc* -> DocMutation conversion (see #901).

pub(crate) fn execute_file_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
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
            let combined = crate::ops::file::append_content(existing, content);
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

pub(crate) fn execute_operation(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
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
                    crate::write::parse_eol_mode(eol)?
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
    if let Some(ov) = &plan.write_policy {
        write_policy.apply_override(ov)?;
    }
    Ok(write_policy)
}

// ---------------------------------------------------------------------------
// Shared execution core
// ---------------------------------------------------------------------------

/// Execute all plan operations in memory, apply write policy, and return
/// the collected results without touching the filesystem.
///
/// On operation failure the error message is pre-formatted as
/// `"operation N (label) failed: ..."` so callers can emit it directly.
pub(crate) fn execute_and_collect(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ---- read_file_content ----

    #[test]
    fn read_file_content_from_disk() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello world").unwrap();

        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        let content = read_file_content(&mut pending, &mut existed, &file).unwrap();
        assert_eq!(content, "hello world");
        assert!(existed.contains(&file));
        assert!(pending.contains_key(&file));
        // Original and current should both be "hello world".
        let (orig, cur) = &pending[&file];
        assert_eq!(orig, "hello world");
        assert_eq!(cur, "hello world");
    }

    #[test]
    fn read_file_content_from_pending() {
        let path = PathBuf::from("/fake/already_loaded.txt");
        let mut pending = HashMap::new();
        pending.insert(
            path.clone(),
            ("original".to_string(), "modified".to_string()),
        );
        let mut existed = HashSet::new();

        let content = read_file_content(&mut pending, &mut existed, &path).unwrap();
        assert_eq!(content, "modified");
        // Should not add to existed_before since it was already in pending.
        assert!(!existed.contains(&path));
    }

    #[test]
    fn read_file_content_missing_file_errors() {
        let mut pending = HashMap::new();
        let mut existed = HashSet::new();
        let path = PathBuf::from("/nonexistent/file.txt");

        let result = read_file_content(&mut pending, &mut existed, &path);
        assert!(result.is_err());
    }

    // ---- read_and_probe ----

    #[test]
    fn read_and_probe_text_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("text.txt");
        std::fs::write(&file, "text content").unwrap();

        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        assert!(read_and_probe(&mut pending, &mut existed, &file).unwrap());
        assert!(pending.contains_key(&file));
    }

    #[test]
    fn read_and_probe_binary_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("binary.bin");
        // Write bytes with NUL to trigger binary detection.
        std::fs::write(&file, b"\x00\x01\x02\x03").unwrap();

        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        assert!(!read_and_probe(&mut pending, &mut existed, &file).unwrap());
        assert!(!pending.contains_key(&file));
    }

    #[test]
    fn read_and_probe_already_loaded_skips() {
        let path = PathBuf::from("/fake/loaded.txt");
        let mut pending = HashMap::new();
        pending.insert(path.clone(), ("orig".to_string(), "cur".to_string()));
        let mut existed = HashSet::new();

        assert!(read_and_probe(&mut pending, &mut existed, &path).unwrap());
    }

    // ---- update_file_content ----

    #[test]
    fn update_file_content_existing_entry() {
        let path = PathBuf::from("/fake/file.txt");
        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        pending.insert(path.clone(), ("original".to_string(), "old".to_string()));

        update_file_content(&mut pending, &mut deletions, &path, "new content".into());

        let (orig, cur) = &pending[&path];
        assert_eq!(orig, "original"); // original preserved
        assert_eq!(cur, "new content");
    }

    #[test]
    fn update_file_content_new_entry() {
        let path = PathBuf::from("/fake/new_file.txt");
        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();

        update_file_content(&mut pending, &mut deletions, &path, "content".into());

        let (orig, cur) = &pending[&path];
        assert!(orig.is_empty()); // no original for new files
        assert_eq!(cur, "content");
    }

    #[test]
    fn update_file_content_clears_deletion() {
        let path = PathBuf::from("/fake/file.txt");
        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        deletions.insert(path.clone());

        update_file_content(&mut pending, &mut deletions, &path, "revived".into());

        assert!(!deletions.contains(&path));
    }

    // ---- path_err ----

    #[test]
    fn path_err_wraps_message() {
        let wrapper = path_err("config.yaml");
        let err = wrapper("invalid key");
        assert_eq!(format!("{err}"), "config.yaml: invalid key");
    }

    // ---- op_needs_doc_flush ----

    #[test]
    fn op_needs_doc_flush_for_replace() {
        let op = Operation::Replace {
            path: Some("f.txt".into()),
            glob: None,
            mode: None,
            from: "a".into(),
            to: Some("b".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            whole_line: false,
            word_boundary: false,
            range: None,
            before_context: None,
            after_context: None,
            if_exists: false,
        };
        assert!(op_needs_doc_flush(&op));
    }

    #[test]
    fn op_needs_doc_flush_false_for_doc_set() {
        let op = Operation::DocSet {
            path: "f.json".into(),
            selector: "key".into(),
            value: serde_json::json!("val"),
        };
        assert!(!op_needs_doc_flush(&op));
    }

    #[test]
    fn op_needs_doc_flush_for_read() {
        let op = Operation::Read {
            path: "f.txt".into(),
            lines: None,
        };
        assert!(op_needs_doc_flush(&op));
    }

    #[test]
    fn op_needs_doc_flush_for_search() {
        let op = Operation::Search {
            path: "src".into(),
            pattern: "TODO".into(),
            regex: false,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: Vec::new(),
            max_results: 0,
            exclude_patterns: Vec::new(),
            custom_ignore_filenames: Vec::new(),
        };
        assert!(op_needs_doc_flush(&op));
    }
}
