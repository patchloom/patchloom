use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::ops::doc::{
    deep_merge, detect_format, navigate_mut, parse_doc, serialize_value, update_matching,
};
use crate::ops::md::{
    dedupe_headings_in, insert_after_heading_in, insert_before_heading_in, replace_section_in,
    table_append_for_tx, upsert_bullet_in,
};
use crate::ops::patch::apply_patch_with_loader;
use crate::ops::replace::{
    replace_content, replacement_text, validate_replace_mode, ReplaceModeError,
};
use crate::plan::{self, Operation, Plan};
use crate::selector;
use crate::write::{apply_policy, atomic_write, WritePolicy};
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// A single file change in the tx output.
#[derive(Serialize)]
struct TxChange {
    path: String,
    action: &'static str,
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

#[derive(Debug, Args)]
pub struct TxArgs {
    // ref:tx-mode:plan-stdin
    /// Path to a plan JSON file, or `-` for stdin.
    #[arg(long)]
    pub plan: String,
    // ref:tx-mode:plan-yaml
    /// Plan format when reading from stdin (json, yaml, toml). Auto-detected from file extension otherwise.
    #[arg(long)]
    pub plan_format: Option<String>,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

const DEFAULT_LIFECYCLE_TIMEOUT_SECS: u64 = 60;
const SHELL_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn host_shell_command(cmd: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg(cmd);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd);
        command
    }
}

/// Run a shell command with a timeout. Returns the exit status,
/// or an error if the command times out or fails to start.
fn run_shell_with_timeout(
    cmd: &str,
    timeout_secs: u64,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut child = host_shell_command(cmd)
        .stdout(std::process::Stdio::null())
        // Discard stderr so verbose lifecycle steps cannot block on a full pipe.
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("timed out after {timeout_secs}s");
        }
        std::thread::sleep(SHELL_POLL_INTERVAL);
    }
}

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
        Operation::MdDedupeHeadings { .. } => "md.dedupe_headings",
        Operation::HygieneFix { .. } => "hygiene.fix",
        Operation::FileCreate { .. } => "file.create",
        Operation::FileDelete { .. } => "file.delete",
        Operation::PatchApply { .. } => "patch.apply",
        Operation::Read { .. } => "read",
    }
}

fn validate_operation(op: &Operation) -> anyhow::Result<()> {
    match op {
        Operation::Replace {
            from,
            to,
            insert_before,
            insert_after,
            ..
        } => {
            if from.is_empty() {
                anyhow::bail!("replace operation requires a non-empty from field");
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
        _ => Ok(()),
    }
}

fn validate_plan_operations(plan: &Plan) -> anyhow::Result<()> {
    for op in &plan.operations {
        validate_operation(op)?;
    }

    Ok(())
}

fn parse_selector(input: &str) -> anyhow::Result<selector::Selector> {
    selector::parse(input).map_err(|e| anyhow::anyhow!("selector error: {e}"))
}

// ---------------------------------------------------------------------------
// Pending file changes
// ---------------------------------------------------------------------------

/// Read file content from the pending map or from disk.
fn read_file_content<'a>(
    pending: &'a mut HashMap<PathBuf, (String, String)>,
    path: &Path,
) -> anyhow::Result<&'a str> {
    match pending.entry(path.to_path_buf()) {
        Entry::Occupied(entry) => Ok(&entry.into_mut().1),
        Entry::Vacant(entry) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
            Ok(&entry.insert((content.clone(), content)).1)
        }
    }
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
    if let Some((_, ref mut current)) = pending.get_mut(path) {
        *current = new_content;
    } else {
        pending.insert(path.to_path_buf(), (String::new(), new_content));
    }
}

// ---------------------------------------------------------------------------
// Operation execution
// ---------------------------------------------------------------------------

/// Load a structured document from the pending buffer, apply a mutation closure,
/// and write the result back. Reduces boilerplate across 9 doc.* operations.
fn with_doc<F>(
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    path: &str,
    mutate: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&mut serde_json::Value) -> anyhow::Result<()>,
{
    let file_path = PathBuf::from(path);
    let content = read_file_content(pending, &file_path)?;
    let format = detect_format(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let mut root = parse_doc(content, &format).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    mutate(&mut root).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let new_content =
        serialize_value(&root, &format).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    update_file_content(pending, deletions, &file_path, new_content);
    Ok(())
}

fn execute_operation(
    op: &Operation,
    pending: &mut HashMap<PathBuf, (String, String)>,
    deletions: &mut HashSet<PathBuf>,
    tx_reads: &mut Vec<TxReadResult>,
) -> anyhow::Result<usize> {
    match op {
        Operation::Replace {
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
            ..
        } => {
            let regex_mode = mode.as_deref() == Some("regex");
            let use_regex = regex_mode || *case_insensitive;
            let replacement = replacement_text(from, to, insert_before, insert_after, use_regex);
            let compiled_re = if regex_mode {
                Some(
                    RegexBuilder::new(from)
                        .case_insensitive(*case_insensitive)
                        .dot_matches_new_line(*multiline)
                        .build()?,
                )
            } else if *case_insensitive {
                Some(
                    RegexBuilder::new(&regex::escape(from))
                        .case_insensitive(true)
                        .build()?,
                )
            } else {
                None
            };

            if let Some(p) = path {
                let file_path = PathBuf::from(p);
                let content = read_file_content(pending, &file_path)?;
                let (replaced, match_count) =
                    replace_content(content, from, &replacement, compiled_re.as_ref(), *nth);
                update_file_content(pending, deletions, &file_path, replaced);
                return Ok(match_count);
            } else if let Some(pattern) = glob {
                let matcher = Glob::new(pattern)?.compile_matcher();
                let walker = WalkBuilder::new(".").build();
                let mut total_matches = 0usize;
                for entry in walker {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                        continue;
                    }
                    let file_path = entry.path().to_path_buf();
                    if !matcher.is_match(&file_path)
                        && !file_path.file_name().is_some_and(|n| matcher.is_match(n))
                    {
                        continue;
                    }
                    // If the file is already in pending (modified by an earlier
                    // operation), use that content. Otherwise read from disk
                    // without inserting into pending so non-matching files don't
                    // get buffered.
                    let content = if pending.contains_key(&file_path) {
                        match read_file_content(pending, &file_path) {
                            Ok(c) => c.to_owned(),
                            Err(e) => {
                                eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                                continue;
                            }
                        }
                    } else {
                        match std::fs::read_to_string(&file_path) {
                            Ok(s) => s,
                            Err(e) => {
                                eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                                continue;
                            }
                        }
                    };
                    let (replaced, match_count) =
                        replace_content(&content, from, &replacement, compiled_re.as_ref(), *nth);
                    total_matches += match_count;
                    if match_count > 0 {
                        update_file_content(pending, deletions, &file_path, replaced);
                    }
                }
                return Ok(total_matches);
            } else {
                anyhow::bail!("replace operation requires either 'path' or 'glob'");
            }
        }

        Operation::DocSet { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let last = sel
                    .last()
                    .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
                let parent_path = &sel[..sel.len() - 1];
                let parent = navigate_mut(root, parent_path, true)?;
                match last {
                    selector::Segment::Key(k) => {
                        parent
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                            .insert(k.clone(), value);
                    }
                    selector::Segment::Index(i) => {
                        let arr = parent
                            .as_array_mut()
                            .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                        if *i < arr.len() {
                            arr[*i] = value;
                        } else {
                            anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                        }
                    }
                    _ => anyhow::bail!("cannot set at wildcard/predicate"),
                }
                Ok(())
            })?;
        }

        Operation::DocDelete { path, key } => {
            let key = key.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let Some(last) = sel.last() else {
                    return Ok(());
                };
                let parent_path = &sel[..sel.len() - 1];
                let parent = navigate_mut(root, parent_path, false)?;
                match last {
                    selector::Segment::Key(k) => {
                        if let Some(obj) = parent.as_object_mut() {
                            obj.remove(k.as_str());
                        }
                    }
                    selector::Segment::Index(i) => {
                        if let Some(arr) = parent.as_array_mut() {
                            if *i < arr.len() {
                                arr.remove(*i);
                            }
                        }
                    }
                    _ => anyhow::bail!("cannot delete at wildcard/predicate"),
                }
                Ok(())
            })?;
        }

        Operation::DocMerge { path, value } => {
            let value = value.clone();
            with_doc(pending, deletions, path, |root| {
                deep_merge(root, &value);
                Ok(())
            })?;
        }

        Operation::DocAppend { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let target = navigate_mut(root, &sel, false)?;
                target
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("target is not an array"))?
                    .push(value);
                Ok(())
            })?;
        }

        Operation::DocPrepend { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let target = navigate_mut(root, &sel, false)?;
                target
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("target is not an array"))?
                    .insert(0, value);
                Ok(())
            })?;
        }

        Operation::DocUpdate { path, key, value } => {
            let key = key.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let count = update_matching(root, &sel, value);
                if count == 0 {
                    anyhow::bail!("no matching nodes found for selector '{key}'");
                }
                Ok(())
            })?;
        }

        Operation::DocMove { path, from, to } => {
            let from = from.clone();
            let to = to.clone();
            with_doc(pending, deletions, path, |root| {
                let from_sel = parse_selector(&from)?;
                let to_sel = parse_selector(&to)?;

                // Remove value at source path.
                let removed = {
                    let last = from_sel
                        .last()
                        .ok_or_else(|| anyhow::anyhow!("empty from selector"))?;
                    let parent_path = &from_sel[..from_sel.len() - 1];
                    let parent = navigate_mut(root, parent_path, false)?;
                    match last {
                        selector::Segment::Key(k) => parent
                            .as_object_mut()
                            .and_then(|obj| obj.remove(k.as_str()))
                            .ok_or_else(|| anyhow::anyhow!("source key '{k}' not found"))?,
                        selector::Segment::Index(i) => {
                            let arr = parent
                                .as_array_mut()
                                .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                            if *i < arr.len() {
                                arr.remove(*i)
                            } else {
                                anyhow::bail!("source index {i} out of bounds");
                            }
                        }
                        _ => anyhow::bail!("cannot move from wildcard/predicate"),
                    }
                };

                // Insert at destination path.
                let last = to_sel
                    .last()
                    .ok_or_else(|| anyhow::anyhow!("empty to selector"))?;
                let parent_path = &to_sel[..to_sel.len() - 1];
                let parent = navigate_mut(root, parent_path, true)?;
                match last {
                    selector::Segment::Key(k) => {
                        parent
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("target parent is not an object"))?
                            .insert(k.clone(), removed);
                    }
                    selector::Segment::Index(i) => {
                        let arr = parent
                            .as_array_mut()
                            .ok_or_else(|| anyhow::anyhow!("target parent is not an array"))?;
                        if *i <= arr.len() {
                            arr.insert(*i, removed);
                        } else {
                            anyhow::bail!("target index {i} out of bounds");
                        }
                    }
                    _ => anyhow::bail!("cannot move to wildcard/predicate"),
                }
                Ok(())
            })?;
        }

        Operation::DocEnsure { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;

                // If the path already exists, no-op.
                if !selector::eval(root, &sel).is_empty() {
                    return Ok(());
                }

                let last = sel
                    .last()
                    .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
                let parent_path = &sel[..sel.len() - 1];
                let parent = navigate_mut(root, parent_path, true)?;
                match last {
                    selector::Segment::Key(k) => {
                        parent
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                            .insert(k.clone(), value);
                    }
                    selector::Segment::Index(i) => {
                        let arr = parent
                            .as_array_mut()
                            .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                        if *i < arr.len() {
                            arr[*i] = value;
                        } else {
                            anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                        }
                    }
                    _ => anyhow::bail!("cannot ensure at wildcard/predicate"),
                }
                Ok(())
            })?;
        }

        Operation::DocDeleteWhere {
            path,
            key,
            predicate,
        } => {
            let key = key.clone();
            let predicate = predicate.clone();
            with_doc(pending, deletions, path, |root| {
                let sel = parse_selector(&key)?;
                let eq_pos = predicate
                    .find('=')
                    .ok_or_else(|| anyhow::anyhow!("predicate must be in key=value format"))?;
                let pred_key = &predicate[..eq_pos];
                let pred_val = &predicate[eq_pos + 1..];

                let target = navigate_mut(root, &sel, false)?;
                let arr = target
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;

                arr.retain(|item| {
                    if let Some(field) = item.get(pred_key) {
                        let matches = match field {
                            serde_json::Value::String(s) => s == pred_val,
                            serde_json::Value::Number(n) => n.to_string() == pred_val,
                            serde_json::Value::Bool(b) => b.to_string() == pred_val,
                            _ => false,
                        };
                        !matches
                    } else {
                        true
                    }
                });
                Ok(())
            })?;
        }

        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = replace_section_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::MdInsertAfterHeading {
            path,
            heading,
            content,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = insert_after_heading_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::MdInsertBeforeHeading {
            path,
            heading,
            content,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = insert_before_heading_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = upsert_bullet_in(file_content, heading, bullet)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::MdTableAppend { path, heading, row } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let new_content = table_append_for_tx(file_content, heading, row)
                .ok_or_else(|| anyhow::anyhow!("heading/table not found: {heading}"))?;
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::MdDedupeHeadings { path } => {
            let file_path = PathBuf::from(path);
            let file_content = read_file_content(pending, &file_path)?;
            let (new_content, _removed) = dedupe_headings_in(file_content);
            update_file_content(pending, deletions, &file_path, new_content);
        }

        Operation::HygieneFix {
            path,
            ensure_final_newline,
        } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?;
            let mut new = content.to_string();
            if ensure_final_newline.unwrap_or(true) {
                new = crate::write::ensure_final_newline(&new);
            }
            update_file_content(pending, deletions, &file_path, new);
        }

        Operation::FileCreate {
            path,
            content,
            force,
        } => {
            let file_path = PathBuf::from(path);
            if force.unwrap_or(false) {
                // Force mode: overwrite existing content.
                if pending.contains_key(&file_path) || file_path.exists() {
                    let _ = read_file_content(pending, &file_path)?;
                }
                update_file_content(pending, deletions, &file_path, content.clone());
            } else {
                if pending.contains_key(&file_path) || file_path.exists() {
                    anyhow::bail!("file already exists: {path}");
                }
                update_file_content(pending, deletions, &file_path, content.clone());
            }
        }

        Operation::PatchApply { diff } => {
            let patched_files = apply_patch_with_loader(diff, |path| {
                let file_path = PathBuf::from(path);
                Ok(read_file_content(pending, &file_path)?.to_string())
            })?;
            for (rel_path, patched_content) in patched_files {
                let file_path = PathBuf::from(&rel_path);
                update_file_content(pending, deletions, &file_path, patched_content);
            }
        }

        Operation::Read { path, lines } => {
            let file_path = PathBuf::from(path);
            let content = read_file_content(pending, &file_path)?.to_string();
            let all_lines: Vec<&str> = content.lines().collect();
            let total_lines = all_lines.len();

            let (start, end) = if let Some(spec) = lines {
                let (s, e) = crate::cmd::read::parse_line_range(spec)?;
                let end = e.unwrap_or(total_lines).min(total_lines);
                (s, end)
            } else {
                (1, total_lines)
            };

            let selected = if total_lines == 0 {
                String::new()
            } else {
                let start_idx = (start - 1).min(total_lines);
                let end_idx = end.min(total_lines);
                let mut s = all_lines[start_idx..end_idx].join("\n");
                if content.ends_with('\n') && end >= total_lines {
                    s.push('\n');
                }
                s
            };

            tx_reads.push(TxReadResult {
                path: path.clone(),
                content: selected,
                start_line: start,
                end_line: end,
                total_lines,
            });
            // Read operations don't modify pending state
        }

        Operation::FileDelete { path } => {
            let file_path = PathBuf::from(path);
            let created_in_tx = match pending.get(&file_path) {
                Some((original, _)) => original.is_empty() && !file_path.exists(),
                None => {
                    let _ = read_file_content(pending, &file_path)?;
                    false
                }
            };

            if created_in_tx {
                pending.remove(&file_path);
                deletions.remove(&file_path);
            } else {
                // Mark current content as empty so the diff shows full deletion.
                update_file_content(pending, deletions, &file_path, String::new());
                deletions.insert(file_path);
            }
        }
    }

    Ok(0)
}

// ---------------------------------------------------------------------------
// Write policy
// ---------------------------------------------------------------------------

fn plan_normalize_eol(mode: &str) -> EolMode {
    match mode {
        "lf" => EolMode::Lf,
        "crlf" => EolMode::Crlf,
        _ => EolMode::Keep,
    }
}

/// Build the effective write policy for a pending file.
///
/// Start from the CLI-derived per-file defaults, including any EditorConfig
/// values resolved by `policy_from_flags()`, then let plan-level `write_policy`
/// entries override only the keys they set.
fn build_write_policy(plan: &Plan, global: &GlobalFlags, path: &Path) -> WritePolicy {
    let mut write_policy = crate::write::policy_from_flags(global, Some(path));
    let Some(plan_write_policy) = &plan.write_policy else {
        return write_policy;
    };

    if let Some(ensure_final_newline) = plan_write_policy.ensure_final_newline {
        write_policy.ensure_final_newline = ensure_final_newline;
    }
    if let Some(normalize_eol) = plan_write_policy.normalize_eol.as_deref() {
        write_policy.normalize_eol = plan_normalize_eol(normalize_eol);
    }
    if let Some(trim_trailing_whitespace) = plan_write_policy.trim_trailing_whitespace {
        write_policy.trim_trailing_whitespace = trim_trailing_whitespace;
    }

    write_policy
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
    reads: Vec<TxReadResult>,
) -> TxOutput {
    let mut tx_changes = Vec::new();
    let mut created = 0usize;
    let mut deleted_count = 0usize;
    let mut modified = 0usize;

    for (path, _original, _) in changes {
        let path_str = path.to_string_lossy().to_string();
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
                path: path.to_string_lossy().to_string(),
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
        reads,
        error_kind: None,
        error: None,
    }
}

fn emit_error_json_with_prefix(
    error_kind: &'static str,
    legacy_error_prefix: &'static str,
    error: &str,
) {
    let output = TxOutput {
        ok: false,
        status: "error",
        files_changed: 0,
        files_created: 0,
        files_deleted: 0,
        changes: Vec::new(),
        reads: Vec::new(),
        error_kind: Some(error_kind),
        error: Some(format!("{legacy_error_prefix}: {error}")),
    };
    // Best-effort: if serialization fails, we can't do much.
    if let Ok(json) = serde_json::to_string_pretty(&output) {
        println!("{json}");
    }
}

fn emit_error_json(error_kind: &'static str, error: &str) {
    let legacy_error_prefix = if error_kind == "format_failed" {
        "validation_failed"
    } else {
        error_kind
    };
    emit_error_json_with_prefix(error_kind, legacy_error_prefix, error);
}

fn describe_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => "terminated by signal".to_string(),
    }
}

fn print_diffs(changes: &[(PathBuf, String, String)]) {
    let diffs: Vec<_> = changes
        .iter()
        .map(|(p, old, new)| unified_diff(&p.to_string_lossy(), old, new))
        .collect();
    let total = diffs.iter().filter(|d| d.has_changes).count();
    let result = DiffResult {
        diffs,
        total_files_changed: total,
    };
    print!("{}", format_diff_result(&result));
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: TxArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    // 1. Read plan from file or stdin.
    let plan_text = if args.plan == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(&args.plan)
            .map_err(|e| anyhow::anyhow!("failed to read plan file '{}': {e}", args.plan))?
    };

    // 2. Parse plan (JSON, YAML, or TOML).
    let plan_path = if args.plan == "-" {
        None
    } else {
        Some(args.plan.as_str())
    };
    let plan = match plan::parse_plan_auto(&plan_text, plan_path, args.plan_format.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            if global.json {
                emit_error_json("parse_error", &e.to_string());
            } else {
                eprintln!("tx: plan parse error: {e}");
            }
            return Ok(exit::PARSE_ERROR);
        }
    };

    if let Err(e) = validate_plan_operations(&plan) {
        if global.json {
            emit_error_json("parse_error", &e.to_string());
        } else {
            eprintln!("tx: plan parse error: {e}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    // 3. Set working directory (plan.cwd overrides global --cwd).
    if let Some(ref cwd) = plan.cwd {
        std::env::set_current_dir(cwd)?;
    } else {
        std::env::set_current_dir(global.resolve_cwd()?)?;
    }

    // 4. Execute all operations, collecting changes in memory (no writes).
    let mut pending: HashMap<PathBuf, (String, String)> = HashMap::new();
    let mut deletions: HashSet<PathBuf> = HashSet::new();
    let mut has_non_idempotent_replace = false;
    let mut total_replace_matches = 0usize;
    let mut tx_reads: Vec<TxReadResult> = Vec::new();

    for (i, op) in plan.operations.iter().enumerate() {
        if let Operation::Replace { if_exists, .. } = op {
            if !if_exists {
                has_non_idempotent_replace = true;
            }
        }
        match execute_operation(op, &mut pending, &mut deletions, &mut tx_reads) {
            Ok(match_count) => {
                total_replace_matches += match_count;
            }
            Err(e) => {
                let msg = format!("operation {} ({}) failed: {e}", i + 1, op_label(op));
                if global.json {
                    emit_error_json("rollback", &msg);
                } else {
                    eprintln!("tx: {msg}");
                }
                return Ok(exit::ROLLBACK);
            }
        }
    }

    let existed_before: HashSet<PathBuf> = pending
        .keys()
        .filter(|path| path.exists())
        .cloned()
        .collect();

    // 5. Apply write policy and collect actual file changes.
    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, (original, current)) in &pending {
        let write_policy = build_write_policy(&plan, global, path);
        let final_content = apply_policy(current, &write_policy);
        if *original != final_content {
            changes.push((path.clone(), original.clone(), final_content));
        }
    }
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    // 6. Output based on mode.
    // Deletions of empty files won't appear in `changes` (original == final == ""),
    // but they still count as changes for --check reporting.
    let pending_deletions = deletions
        .iter()
        .filter(|p| !changes.iter().any(|(c, _, _)| c == *p))
        .count();
    let no_effective_changes = changes.is_empty() && pending_deletions == 0;
    let replace_no_matches =
        has_non_idempotent_replace && total_replace_matches == 0 && no_effective_changes;

    if global.check {
        if no_effective_changes {
            if replace_no_matches {
                return Ok(exit::NO_MATCHES);
            }
            if global.json {
                let mut output = build_tx_output(
                    "success",
                    true,
                    &changes,
                    &deletions,
                    &existed_before,
                    Vec::new(),
                );
                output.reads = std::mem::take(&mut tx_reads);
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            return Ok(exit::SUCCESS);
        }
        if global.json {
            let mut output = build_tx_output(
                "changes_detected",
                true,
                &changes,
                &deletions,
                &existed_before,
                Vec::new(),
            );
            output.reads = std::mem::take(&mut tx_reads);
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !global.quiet {
            println!("{} file(s) would change", changes.len() + pending_deletions);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        // Write all files atomically (policy already applied).
        let noop_policy = WritePolicy::default();
        for (path, _, new_content) in &changes {
            if deletions.contains(path) {
                std::fs::remove_file(path)?;
            } else {
                // Ensure parent directories exist (needed for file.create).
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
                atomic_write(path, new_content, &noop_policy)?;
            }
        }
        // Handle deletions not captured in changes (e.g. empty files whose
        // original == final content == "").
        for path in &deletions {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
        }

        // Show diffs if --diff flag is set.
        if global.diff && !changes.is_empty() {
            print_diffs(&changes);
        }

        // 7. Run format steps (between writes and validation).
        let mut lifecycle_failed = false;
        let mut lifecycle_error = None;
        let mut lifecycle_error_kind = "validation_failed";
        if let Some(ref format_steps) = plan.format {
            for (index, step) in format_steps.iter().enumerate() {
                let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
                match run_shell_with_timeout(&step.cmd, timeout_secs) {
                    Ok(status) if !status.success() => {
                        let msg = format!(
                            "format step failed (step {}, {})",
                            index + 1,
                            describe_exit_status(status)
                        );
                        eprintln!("tx: {msg}");
                        lifecycle_error = Some(msg);
                        lifecycle_error_kind = "format_failed";
                        lifecycle_failed = true;
                        break;
                    }
                    Err(e) => {
                        let msg = format!("format step error (step {}): {e}", index + 1);
                        eprintln!("tx: {msg}");
                        lifecycle_error = Some(msg);
                        lifecycle_error_kind = "format_failed";
                        lifecycle_failed = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        // 8. Run validation steps.
        if !lifecycle_failed {
            if let Some(ref validate) = plan.validate {
                for (index, step) in validate.iter().enumerate() {
                    let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
                    let success = match run_shell_with_timeout(&step.cmd, timeout_secs) {
                        Ok(status) if status.success() => true,
                        Ok(status) => {
                            let msg = format!(
                                "required validation failed (step {}, {})",
                                index + 1,
                                describe_exit_status(status)
                            );
                            eprintln!("tx: {msg}");
                            lifecycle_error = Some(msg);
                            false
                        }
                        Err(e) => {
                            let msg = format!("validation error (step {}): {e}", index + 1);
                            eprintln!("tx: {msg}");
                            lifecycle_error = Some(msg);
                            false
                        }
                    };

                    if !success && step.required.unwrap_or(false) {
                        lifecycle_failed = true;
                        break;
                    }
                }
            }
        }

        if lifecycle_failed {
            if plan.strict {
                // Restore original file contents.
                let noop_policy = WritePolicy::default();
                for (path, original, _) in &changes {
                    if !existed_before.contains(path) && !deletions.contains(path) {
                        // File was created by the tx; remove it.
                        let _ = std::fs::remove_file(path);
                    } else {
                        let _ = atomic_write(path, original, &noop_policy);
                    }
                }
                // Restore files that were deleted.
                for path in &deletions {
                    if let Some((orig, _)) = pending.get(path) {
                        if existed_before.contains(path) {
                            let _ = atomic_write(path, orig, &noop_policy);
                        }
                    }
                }
                let rollback_msg = match lifecycle_error.as_deref() {
                    Some(reason) => format!("strict mode -- all changes reverted ({reason})"),
                    None => "strict mode -- all changes reverted".to_string(),
                };
                if global.json {
                    emit_error_json_with_prefix(lifecycle_error_kind, "rollback", &rollback_msg);
                } else {
                    eprintln!("tx: {rollback_msg}");
                }
                return Ok(exit::ROLLBACK);
            }
            if global.json {
                emit_error_json(
                    lifecycle_error_kind,
                    lifecycle_error
                        .as_deref()
                        .unwrap_or("format or validation step failed"),
                );
            }
            return Ok(exit::VALIDATION_FAILED);
        }

        if replace_no_matches {
            return Ok(exit::NO_MATCHES);
        }
        if global.json {
            let mut output = build_tx_output(
                "success",
                true,
                &changes,
                &deletions,
                &existed_before,
                Vec::new(),
            );
            output.reads = std::mem::take(&mut tx_reads);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        return Ok(exit::SUCCESS);
    }

    if replace_no_matches {
        return Ok(exit::NO_MATCHES);
    }

    // Default / --diff mode: show unified diffs.
    if global.json {
        let mut output = build_tx_output(
            "success",
            true,
            &changes,
            &deletions,
            &existed_before,
            Vec::new(),
        );
        output.reads = std::mem::take(&mut tx_reads);
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !changes.is_empty() {
        print_diffs(&changes);
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn shell_true() -> &'static str {
        #[cfg(windows)]
        {
            "exit /b 0"
        }
        #[cfg(not(windows))]
        {
            "true"
        }
    }

    fn shell_false() -> &'static str {
        #[cfg(windows)]
        {
            "exit /b 1"
        }
        #[cfg(not(windows))]
        {
            "false"
        }
    }

    fn shell_stderr_spam() -> &'static str {
        #[cfg(windows)]
        {
            "powershell -Command \"1..4000 | ForEach-Object { Write-Error ('x' * 200) }; exit 0\""
        }
        #[cfg(not(windows))]
        {
            "python3 -c \"import sys; sys.stderr.write('x' * (1024 * 1024)); sys.stderr.flush()\""
        }
    }

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

    #[test]
    fn read_file_content_populates_pending_from_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.txt");
        fs::write(&path, "hello from disk\n").unwrap();
        let mut pending = HashMap::new();

        let content = read_file_content(&mut pending, &path).unwrap();

        assert_eq!(content, "hello from disk\n");
        assert_eq!(
            pending.get(&path),
            Some(&(
                "hello from disk\n".to_string(),
                "hello from disk\n".to_string()
            ))
        );
    }

    #[test]
    fn update_file_content_inserts_missing_pending_entry() {
        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let path = PathBuf::from("created.txt");

        update_file_content(&mut pending, &mut deletions, &path, "brand new".to_string());

        assert_eq!(
            pending.get(&path),
            Some(&(String::new(), "brand new".to_string()))
        );
    }

    #[test]
    fn multi_op_plan() {
        let dir = TempDir::new().unwrap();

        // Create test files.
        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let json_file = dir.path().join("config.json");
        fs::write(&json_file, r#"{"name": "old"}"#).unwrap();

        let no_nl = dir.path().join("no_nl.txt");
        fs::write(&no_nl, "content").unwrap();

        // Build plan with replace + doc.set + hygiene.fix.
        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": json_file.to_str().unwrap(),
                    "key": "name",
                    "value": "new"
                },
                {
                    "op": "hygiene.fix",
                    "path": no_nl.to_str().unwrap(),
                    "ensure_final_newline": true
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify replace.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hi world\n");

        // Verify doc.set.
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&json_file).unwrap()).unwrap();
        assert_eq!(config["name"], serde_json::json!("new"));

        // Verify hygiene.fix.
        assert!(fs::read_to_string(&no_nl).unwrap().ends_with('\n'));
    }

    #[test]
    fn create_then_delete_in_same_tx_becomes_noop() {
        let dir = TempDir::new().unwrap();
        let new_file = dir.path().join("new.txt");
        let plan_json = serde_json::json!({
            "operations": [
                {"op": "file.create", "path": new_file.to_str().unwrap(), "content": "hello"},
                {"op": "file.delete", "path": new_file.to_str().unwrap()}
            ]
        });
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!new_file.exists());
    }

    #[test]
    fn rollback_on_failure() {
        let dir = TempDir::new().unwrap();

        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let nonexistent = dir.path().join("nonexistent.json");

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": nonexistent.to_str().unwrap(),
                    "key": "name",
                    "value": "test"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::ROLLBACK);

        // Verify no files were modified.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hello world\n");
    }

    #[test]
    fn validation_pass() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "operations": [],
            "validate": [
                {"cmd": shell_true(), "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn validation_fail_required() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "operations": [],
            "validate": [
                {"cmd": shell_false(), "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::VALIDATION_FAILED);
    }

    #[test]
    fn validation_pass_with_large_stderr_output() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "operations": [],
            "validate": [
                {"cmd": shell_stderr_spam(), "required": true, "timeout": 2}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn parse_plan_json_string() {
        let plan_json =
            r#"{"operations": [{"op": "replace", "path": "test.txt", "from": "a", "to": "b"}]}"#;
        let plan = plan::parse_plan(plan_json).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn malformed_plan_returns_parse_error() {
        let dir = TempDir::new().unwrap();

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, "not json at all {{{").unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let global = default_global();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn replace_requires_replacement_mode() {
        let plan_json = serde_json::json!({
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "replace operation requires one of to, insert_before, or insert_after"
        );
    }

    #[test]
    fn replace_conflicting_insert_fields_return_parse_error() {
        let plan_json = serde_json::json!({
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "insert_before": "X",
                "insert_after": "Y"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "insert_before and insert_after cannot both be set"
        );
    }

    #[test]
    fn regex_insert_before_uses_match_anchor_in_replacement_text() {
        let text = replacement_text("b+", &None, &Some("X".to_string()), &None, true);

        assert_eq!(text, "X${0}");
    }

    #[test]
    fn regex_insert_after_uses_match_anchor_in_replacement_text() {
        let text = replacement_text("b+", &None, &None, &Some("X".to_string()), true);

        assert_eq!(text, "${0}X");
    }

    #[test]
    fn replace_to_with_insert_returns_parse_error() {
        let plan_json = serde_json::json!({
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "to": "world",
                "insert_before": "X"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "to cannot be combined with insert_before or insert_after"
        );
    }

    #[test]
    fn tx_file_create_in_plan() {
        let dir = TempDir::new().unwrap();

        let new_file = dir.path().join("created.txt");

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "file.create",
                    "path": new_file.to_str().unwrap(),
                    "content": "brand new file\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify the file was created with the correct content.
        assert!(new_file.exists());
        assert_eq!(fs::read_to_string(&new_file).unwrap(), "brand new file\n");
    }

    #[test]
    fn tx_file_create_existing_fails() {
        let dir = TempDir::new().unwrap();

        let existing = dir.path().join("existing.txt");
        fs::write(&existing, "original content\n").unwrap();

        let plan_json = serde_json::json!({
            "operations": [
                {
                    "op": "file.create",
                    "path": existing.to_str().unwrap(),
                    "content": "should not overwrite\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::ROLLBACK);

        // Verify the original file was NOT modified.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "original content\n");
    }
}
