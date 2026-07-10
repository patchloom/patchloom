//! Operation dispatcher and in-memory plan execution.
//!
//! File mutations: [`file_ops`]. Write policy: [`policy`].
//! Other handlers: `tx/{ast,md,patch,replace,search,tidy}_op`.

use super::output::{TxDocMutation, TxExecResult, TxLintResult, TxReadResult, TxSearchResult};
use super::validate::op_label;
use crate::ops::doc::{
    FileFormat, MutationResult, apply_doc_mutation, detect_format, parse_doc,
    serialize_value_preserving,
};
use crate::plan::{Operation, Plan};
use crate::tx::context::EngineContext;
use crate::write::apply_policy;
use anyhow::Context;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod file_ops;
mod policy;
#[cfg(test)]
mod tests;

pub(crate) use file_ops::execute_file_op;
pub(crate) use policy::build_write_policy;

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
                .with_context(|| format!("failed to read {}", path.display()))?;
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
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
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

/// Update the current content for a file in the pending map and mark it
/// as a write target. If the file was previously marked for deletion,
/// unmark it (the latest intent is to write, not delete).
pub(crate) fn update_file_content(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    write_targets: &mut HashSet<PathBuf>,
    path: &Path,
    new_content: String,
) {
    deletions.remove(path);
    write_targets.insert(path.to_path_buf());
    if let Some((_, current)) = pending.get_mut(path) {
        *current = new_content;
    } else {
        pending.insert(path.to_path_buf(), (String::new(), new_content));
    }
}

/// Mark a file as targeted by a write operation. Write policy is only
/// applied to files in this set, not to files loaded solely for reading
/// (#1108).
pub(crate) fn mark_write_target(write_targets: &mut HashSet<PathBuf>, path: &Path) {
    write_targets.insert(path.to_path_buf());
}

// ---------------------------------------------------------------------------
// Operation execution
// ---------------------------------------------------------------------------

/// Flush a single cached document back to the pending text map.
pub(crate) fn flush_doc_cache_entry(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    write_targets: &mut HashSet<PathBuf>,
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
    update_file_content(pending, deletions, write_targets, &path, new_content);
    Ok(())
}

/// Flush all cached documents back to the pending text map. Called before
/// the write phase or when a non-doc operation needs the current text.
pub(crate) fn flush_doc_cache(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    write_targets: &mut HashSet<PathBuf>,
    doc_cache: &mut HashMap<PathBuf, CachedDoc>,
) -> anyhow::Result<()> {
    for (path, cached) in doc_cache.drain() {
        flush_doc_cache_entry(pending, deletions, write_targets, path, cached)?;
    }
    Ok(())
}

/// Wrap an error with the file path for context.
///
/// This replaces 27 identical `.map_err(path_err(path))` calls
/// throughout `execute_operation` and `get_doc_root`. Preserves the
/// source chain so typed kinds (IO NotFound, etc.) remain downcastable.
pub(crate) fn path_err(path: &str) -> impl FnOnce(anyhow::Error) -> anyhow::Error + '_ {
    // Include the prior Display text in the context so stderr stays informative,
    // while keeping the source chain for is_io_not_found / typed kinds.
    move |e| {
        let prior = e.to_string();
        e.context(format!("{path}: {prior}"))
    }
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
        let old_value = root.clone();
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
    /// Doc delete / delete-where summaries for plan/MCP JSON (#1439).
    pub(crate) tx_mutations: &'a mut Vec<TxDocMutation>,
    /// Files targeted by write operations (Replace, TidyFix, Doc mutations,
    /// Md mutations, Patch, AST transforms, etc.). Write policy is only
    /// applied to these files, not to files loaded solely for reading (#1108).
    pub(crate) write_targets: &'a mut HashSet<PathBuf>,
    pub(crate) replace_hint: Option<String>,
    pub(crate) cwd: &'a Path,
    pub(crate) quiet: bool,
    pub(crate) structured: bool,
    /// Optional PathGuard for defense-in-depth containment checks on
    /// dynamically-expanded paths (e.g. glob-matched files in replace). The
    /// upfront declared_paths check covers static paths; this catches paths
    /// discovered at runtime (#1361).
    pub(crate) guard: Option<&'a crate::containment::PathGuard>,
}

/// Test fixture that owns all the storage behind a `TxState`, avoiding
/// the need to declare 8+ `let mut` bindings in every test.
#[cfg(test)]
pub(crate) struct TxStateFixture {
    pub pending: HashMap<PathBuf, (String, String)>,
    pub deletions: HashSet<PathBuf>,
    pub existed_before: HashSet<PathBuf>,
    pub doc_cache: HashMap<PathBuf, CachedDoc>,
    pub reads: Vec<TxReadResult>,
    pub searches: Vec<TxSearchResult>,
    pub lints: Vec<TxLintResult>,
    pub mutations: Vec<TxDocMutation>,
    pub write_targets: HashSet<PathBuf>,
}

#[cfg(test)]
impl TxStateFixture {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            deletions: HashSet::new(),
            existed_before: HashSet::new(),
            doc_cache: HashMap::new(),
            reads: Vec::new(),
            searches: Vec::new(),
            lints: Vec::new(),
            mutations: Vec::new(),
            write_targets: HashSet::new(),
        }
    }

    pub fn state<'a>(&'a mut self, cwd: &'a Path) -> TxState<'a> {
        TxState {
            pending: &mut self.pending,
            deletions: &mut self.deletions,
            existed_before: &mut self.existed_before,
            doc_cache: &mut self.doc_cache,
            tx_reads: &mut self.reads,
            tx_searches: &mut self.searches,
            tx_lints: &mut self.lints,
            tx_mutations: &mut self.mutations,
            write_targets: &mut self.write_targets,
            replace_hint: None,
            cwd,
            quiet: true,
            structured: false,
            guard: None,
        }
    }
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
/// Delegates to [`Operation::needs_doc_flush()`].
fn op_needs_doc_flush(op: &Operation) -> bool {
    op.needs_doc_flush()
}

pub(crate) fn execute_doc_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let (path, mutation) = crate::plan::op_to_doc_mutation(op)
        .ok_or_else(|| anyhow::anyhow!("execute_doc_op called with non-doc operation"))?;
    let root = get_doc_root(tx.pending, tx.existed_before, tx.doc_cache, path, tx.cwd)
        .map_err(path_err(path))?;

    // Delete and DeleteWhere are idempotent: a no-match is not an error
    // (the old code silently discarded the bool/count return value).
    // Update is strict: no-match means the selector was wrong.
    let strict_no_match = matches!(op, Operation::DocUpdate { .. });
    let is_delete = matches!(
        op,
        Operation::DocDelete { .. } | Operation::DocDeleteWhere { .. }
    );
    let op_name = match op {
        Operation::DocDelete { .. } => "doc.delete",
        Operation::DocDeleteWhere { .. } => "doc.delete_where",
        _ => "",
    };

    match apply_doc_mutation(root, mutation).map_err(path_err(path))? {
        MutationResult::Applied | MutationResult::AlreadyExists => Ok(()),
        MutationResult::Removed(count) => {
            if is_delete {
                tx.tx_mutations.push(TxDocMutation {
                    path: path.to_string(),
                    op: op_name.to_string(),
                    changed: true,
                    removed: count,
                });
            }
            Ok(())
        }
        MutationResult::NoMatch if strict_no_match => {
            let label = op_label(op);
            Err(crate::exit::NoMatchError {
                msg: format!("{path}: {label} matched nothing"),
            })?
        }
        MutationResult::NoMatch => {
            // Idempotent delete/delete-where: still report removed:0 so MCP/tx
            // JSON can distinguish no-op from real deletes (#1439).
            if is_delete {
                tx.tx_mutations.push(TxDocMutation {
                    path: path.to_string(),
                    op: op_name.to_string(),
                    changed: false,
                    removed: 0,
                });
            }
            Ok(())
        }
        MutationResult::TypeError(msg) => Err(crate::exit::TypeErrorError {
            msg: format!("{path}: {msg}"),
        }
        .into()),
    }
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

        // execute_md_op extracted to tx/md_op.rs (#1359).
        Operation::MdReplaceSection { .. }
        | Operation::MdInsertAfterHeading { .. }
        | Operation::MdInsertBeforeHeading { .. }
        | Operation::MdUpsertBullet { .. }
        | Operation::MdTableAppend { .. }
        | Operation::MdMoveSection { .. }
        | Operation::MdDedupeHeadings { .. }
        | Operation::MdLintAgents { .. } => {
            return super::md_op::execute_md_op(op, tx);
        }

        // execute_tidy_op extracted to tx/tidy_op.rs (#1359).
        Operation::TidyFix { .. } => {
            return super::tidy_op::execute_tidy_op(op, tx);
        }

        Operation::FileAppend { .. }
        | Operation::FilePrepend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. } => {
            return execute_file_op(op, tx);
        }

        // execute_patch_op extracted to tx/patch_op.rs (#1359).
        Operation::PatchApply { .. } => {
            return super::patch_op::execute_patch_op(op, tx);
        }

        Operation::Read { path, lines } => {
            execute_read_op(path, lines, tx)?;
        }

        Operation::Search { .. } => {
            execute_search_op(op, tx)?;
        }

        #[cfg(feature = "ast")]
        Operation::AstRename { .. }
        | Operation::AstReplace { .. }
        | Operation::AstRewriteSignature { .. }
        | Operation::AstInsert { .. }
        | Operation::AstWrap { .. }
        | Operation::AstImports { .. }
        | Operation::AstReorder { .. }
        | Operation::AstGroup { .. }
        | Operation::AstMove { .. }
        | Operation::AstExtractToFile { .. }
        | Operation::AstSplit { .. } => {
            return super::ast_op::execute_ast_op(op, tx);
        }
    }

    Ok(0)
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
    ctx: &EngineContext,
    quiet: bool,
    structured: bool,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<TxExecResult> {
    let cwd = ctx.cwd();
    let mut pending: HashMap<PathBuf, (String, String)> = HashMap::new();
    let mut deletions: HashSet<PathBuf> = HashSet::new();
    let mut existed_before: HashSet<PathBuf> = HashSet::new();
    let mut write_targets: HashSet<PathBuf> = HashSet::new();
    let mut has_non_idempotent_replace = false;
    let mut total_replace_matches = 0usize;
    let mut tx_reads: Vec<TxReadResult> = Vec::new();
    let mut tx_searches: Vec<TxSearchResult> = Vec::new();
    let mut tx_lints: Vec<TxLintResult> = Vec::new();
    let mut tx_mutations: Vec<TxDocMutation> = Vec::new();
    let mut doc_cache: HashMap<PathBuf, CachedDoc> = HashMap::new();
    let mut replace_hint: Option<String> = None;

    // Upfront PathGuard on declared paths (same contract as execute_plan_inner /
    // execute_plan_direct). CLI `tx` and `batch` call this function directly and
    // previously only had guard wiring on replace glob expansion, so
    // `file.create ../escape` under `--contain` could still write outside the
    // workspace (MPI cycle 13).
    if let Some(g) = guard {
        for op in &plan.operations {
            for p in op.declared_paths() {
                g.check_path(&p)
                    .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {e}"))?;
            }
        }
    }

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
            flush_doc_cache(
                &mut pending,
                &mut deletions,
                &mut write_targets,
                &mut doc_cache,
            )?;
        }
        let mut tx = TxState {
            pending: &mut pending,
            deletions: &mut deletions,
            existed_before: &mut existed_before,
            doc_cache: &mut doc_cache,
            tx_reads: &mut tx_reads,
            tx_searches: &mut tx_searches,
            tx_lints: &mut tx_lints,
            tx_mutations: &mut tx_mutations,
            write_targets: &mut write_targets,
            replace_hint: None,
            cwd,
            quiet,
            structured,
            guard,
        };
        match execute_operation(op, &mut tx) {
            Ok(count) => {
                crate::verbose!(
                    "tx: operation {} succeeded (replace_matches: {count})",
                    i + 1
                );
                // Only accumulate Replace matches; other operation types
                // may return non-zero counts (e.g. AST symbol counts) that
                // would mask a genuine Replace no-match condition (#1105).
                if matches!(op, Operation::Replace { .. }) {
                    total_replace_matches += count;
                }
                if replace_hint.is_none() {
                    replace_hint = tx.replace_hint.take();
                }
            }
            Err(e) => {
                crate::verbose!("tx: operation {} failed: {e}", i + 1);
                // Preserve NoMatchError type (exit 3) and include the detail in
                // the message so agents see "function X not found", not just
                // "operation N failed" (#1459 rewrite_signature + all AST ops).
                // Walk the full chain: intermediate .context() must not drop
                // the NoMatch classification.
                if let Some(detail) = e.chain().find_map(|c| {
                    c.downcast_ref::<crate::exit::NoMatchError>()
                        .map(|n| n.msg.clone())
                }) {
                    return Err(crate::exit::NoMatchError {
                        msg: format!("operation {} ({}) failed: {detail}", i + 1, op_label(op)),
                    }
                    .into());
                }
                // Preserve AmbiguousError (exit 5) for replace --unique multi-match.
                if let Some(detail) = e.chain().find_map(|c| {
                    c.downcast_ref::<crate::exit::AmbiguousError>()
                        .map(|n| n.msg.clone())
                }) {
                    return Err(crate::exit::AmbiguousError {
                        msg: format!("operation {} ({}) failed: {detail}", i + 1, op_label(op)),
                    }
                    .into());
                }
                // Keep a readable Display string for stderr/tests, and re-emit
                // typed kinds so CLI/tx --json can map error_kind without
                // scraping English.
                let msg = format!("operation {} ({}) failed: {e}", i + 1, op_label(op));
                if crate::exit::is_io_not_found(&e) {
                    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg).into());
                }
                if crate::exit::is_already_exists(&e) {
                    return Err(crate::exit::AlreadyExistsError { msg }.into());
                }
                if crate::exit::is_invalid_input(&e) {
                    return Err(crate::exit::InvalidInputError { msg }.into());
                }
                if crate::exit::is_type_error(&e) {
                    return Err(crate::exit::TypeErrorError { msg }.into());
                }
                if crate::exit::is_conflicts(&e) {
                    return Err(crate::exit::ConflictsError { msg }.into());
                }
                anyhow::bail!("{msg}");
            }
        }
    }

    flush_doc_cache(
        &mut pending,
        &mut deletions,
        &mut write_targets,
        &mut doc_cache,
    )?;

    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, (original, current)) in &pending {
        // Skip write policy for files that were only loaded for reading
        // (Read/Search operations). Write policy is only applied to files
        // targeted by at least one write operation (#1108).
        if !write_targets.contains(path) {
            continue;
        }
        let write_policy = build_write_policy(plan, ctx, path)?;
        let final_content = apply_policy(current, &write_policy);
        // A file creation with empty content still has original == final == "",
        // but must be treated as an effective change because the file does not
        // exist on disk yet (#create-empty-file).
        let is_new_file = !existed_before.contains(path);
        if *original != *final_content || is_new_file {
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
        tx_mutations,
        no_effective_changes,
        replace_no_matches,
        replace_hint,
    })
}
