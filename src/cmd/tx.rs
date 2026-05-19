use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::ops::doc::{
    deep_merge, delete_at_selector, delete_where, detect_format, move_at_path, navigate_mut,
    parse_doc, serialize_value_preserving, set_at_path, update_matching,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    searches: Vec<TxSearchResult>,
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

fn host_shell_command(cmd: &str, cwd: &Path) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("cmd");
        // Use raw_arg so cmd.exe receives the command string without Rust's
        // MSVC-style argument quoting. Rust's .arg() wraps in double quotes
        // and escapes inner quotes with backslash, which cmd.exe does not
        // understand. raw_arg passes the string verbatim, letting cmd's /C
        // handler parse redirects (>), pipes (|), and inner quotes correctly.
        command.arg("/C").raw_arg(cmd).current_dir(cwd);
        command
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd).current_dir(cwd);
        // Spawn the child in its own process group so that
        // kill_process_tree can kill the entire group (#220).
        command.process_group(0);
        command
    }
}

/// Run a shell command with a timeout. Returns the exit status,
/// or an error if the command times out or fails to start.
fn run_shell_with_timeout(
    cmd: &str,
    timeout_secs: u64,
    cwd: &Path,
) -> anyhow::Result<std::process::ExitStatus> {
    let mut child = host_shell_command(cmd, cwd)
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
            kill_process_tree(&mut child);
            anyhow::bail!("timed out after {timeout_secs}s");
        }
        std::thread::sleep(SHELL_POLL_INTERVAL);
    }
}

/// Kill a shell process and its descendants.
///
/// On Unix, the child is spawned in its own process group (via
/// `CommandExt::process_group(0)`), so we send SIGKILL to the entire
/// group with `libc::killpg`. This ensures compound commands
/// (pipelines, `&&` chains) have all descendants killed, not just the
/// immediate `sh` process.
///
/// On Windows, `child.kill()` calls `TerminateProcess` which only kills
/// the immediate `cmd.exe` process, leaving grandchildren (ping, powershell,
/// etc.) running as orphans. We use `taskkill /F /T /PID` to kill the
/// entire process tree.
fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        // The child was spawned in its own process group, so its PID
        // equals its PGID. Kill the entire group.
        let pid = child.id() as i32;
        // SAFETY: killpg is a POSIX function that sends a signal to a
        // process group. We pass the child's PID (which is also its
        // PGID due to process_group(0)) and SIGKILL (9).
        #[allow(unsafe_code)]
        unsafe {
            libc::killpg(pid, libc::SIGKILL);
        }
    }
    let _ = child.wait();
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
        Operation::Search { .. } => "search",
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
                anyhow::bail!("replace operation requires a non-empty from field");
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
        | Operation::HygieneFix { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::Read { .. }
        | Operation::Search { .. }
        | Operation::PatchApply { .. } => Ok(()),
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
    cwd: &Path,
) -> anyhow::Result<&'a str> {
    match pending.entry(path.to_path_buf()) {
        Entry::Occupied(entry) => Ok(&entry.into_mut().1),
        Entry::Vacant(entry) => {
            let abs = cwd.join(path);
            let content = std::fs::read_to_string(&abs)
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
    if let Some((_, current)) = pending.get_mut(path) {
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
    cwd: &Path,
    mutate: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&mut serde_json::Value) -> anyhow::Result<()>,
{
    let file_path = cwd.join(path);
    let content = read_file_content(pending, &file_path, cwd)?;
    let format = detect_format(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let mut root = parse_doc(content, &format).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    // Only clone old_value for TOML/YAML (needed for comment-preserving
    // serialization). JSON serialization ignores old_value (#224).
    let old_value = match format {
        crate::ops::doc::FileFormat::Json => serde_json::Value::Null,
        _ => root.clone(),
    };
    mutate(&mut root).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let new_content = serialize_value_preserving(content, &old_value, &root, &format)
        .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    update_file_content(pending, deletions, &file_path, new_content);
    Ok(())
}

/// Mutable transaction state passed through operation execution.
struct TxState<'a> {
    pending: &'a mut HashMap<PathBuf, (String, String)>,
    deletions: &'a mut HashSet<PathBuf>,
    tx_reads: &'a mut Vec<TxReadResult>,
    tx_searches: &'a mut Vec<TxSearchResult>,
    cwd: &'a Path,
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
        ..
    } = op
    else {
        unreachable!()
    };
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
        let file_path = tx.cwd.join(p);
        let content = read_file_content(tx.pending, &file_path, tx.cwd)?;
        let (replaced, match_count) =
            replace_content(content, from, &replacement, compiled_re.as_ref(), *nth);
        update_file_content(tx.pending, tx.deletions, &file_path, replaced);
        Ok(match_count)
    } else if let Some(pattern) = glob {
        let matcher = Glob::new(pattern)?.compile_matcher();
        let walker = WalkBuilder::new(tx.cwd).build();
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
            let content = if tx.pending.contains_key(&file_path) {
                let result = read_file_content(tx.pending, &file_path, tx.cwd);
                match result {
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
                update_file_content(tx.pending, tx.deletions, &file_path, replaced);
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
    let content = read_file_content(tx.pending, &file_path, tx.cwd)?.to_string();
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

    tx.tx_reads.push(TxReadResult {
        path: path.to_string(),
        content: selected,
        start_line: start,
        end_line: end,
        total_lines,
    });
    Ok(())
}

/// Execute a search operation within a transaction.
fn execute_search_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<()> {
    let Operation::Search {
        path,
        pattern,
        regex,
        case_insensitive,
        context,
        before_context,
        after_context,
    } = op
    else {
        unreachable!()
    };
    let file_path = tx.cwd.join(path);
    let content = read_file_content(tx.pending, &file_path, tx.cwd)?.to_string();

    let re = if *regex {
        let mut builder = RegexBuilder::new(pattern);
        if *case_insensitive {
            builder.case_insensitive(true);
        }
        builder.build()?
    } else {
        let escaped = regex::escape(pattern);
        let mut builder = RegexBuilder::new(&escaped);
        if *case_insensitive {
            builder.case_insensitive(true);
        }
        builder.build()?
    };

    let ctx_before = before_context.or(*context).unwrap_or(0);
    let ctx_after = after_context.or(*context).unwrap_or(0);
    let lines: Vec<&str> = content.lines().collect();
    let mut matches = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if let Some(m) = re.find(line) {
            let start = i.saturating_sub(ctx_before);
            let end = (i + 1 + ctx_after).min(lines.len());
            matches.push(TxSearchMatch {
                line: i + 1,
                column: m.start() + 1,
                text: line.to_string(),
                context_before: lines[start..i].iter().map(|s| s.to_string()).collect(),
                context_after: lines[i + 1..end].iter().map(|s| s.to_string()).collect(),
            });
        }
    }

    tx.tx_searches.push(TxSearchResult {
        path: path.to_string(),
        pattern: pattern.to_string(),
        match_count: matches.len(),
        matches,
    });
    Ok(())
}

fn execute_operation(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::Replace { .. } => {
            return execute_replace_op(op, tx);
        }

        Operation::DocSet { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                let sel = parse_selector(&key)?;
                set_at_path(root, &sel, value)?;
                Ok(())
            })?;
        }

        Operation::DocDelete { path, key } => {
            let key = key.clone();
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                let sel = parse_selector(&key)?;
                delete_at_selector(root, &sel)?;
                Ok(())
            })?;
        }

        Operation::DocMerge { path, value } => {
            let value = value.clone();
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                deep_merge(root, &value);
                Ok(())
            })?;
        }

        Operation::DocAppend { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
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
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
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
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
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
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                let from_sel = parse_selector(&from)?;
                let to_sel = parse_selector(&to)?;
                move_at_path(root, &from_sel, &to_sel)?;
                Ok(())
            })?;
        }

        Operation::DocEnsure { path, key, value } => {
            let key = key.clone();
            let value = value.clone();
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                let sel = parse_selector(&key)?;

                // If the path already exists, no-op.
                if !selector::eval(root, &sel).is_empty() {
                    return Ok(());
                }

                set_at_path(root, &sel, value)?;
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
            with_doc(tx.pending, tx.deletions, path, tx.cwd, |root| {
                let sel = parse_selector(&key)?;
                delete_where(root, &sel, &predicate)?;
                Ok(())
            })?;
        }

        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let new_content = replace_section_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdInsertAfterHeading {
            path,
            heading,
            content,
        } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let new_content = insert_after_heading_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdInsertBeforeHeading {
            path,
            heading,
            content,
        } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let new_content = insert_before_heading_in(file_content, heading, content)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let new_content = upsert_bullet_in(file_content, heading, bullet)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdTableAppend { path, heading, row } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let new_content = table_append_for_tx(file_content, heading, row)
                .ok_or_else(|| anyhow::anyhow!("heading/table not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdDedupeHeadings { path } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let (new_content, _removed) = dedupe_headings_in(file_content);
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::HygieneFix {
            path,
            ensure_final_newline,
            trim_trailing_whitespace,
            normalize_eol,
        } => {
            let file_path = tx.cwd.join(path);
            let content = read_file_content(tx.pending, &file_path, tx.cwd)?;
            let mut new = content.to_string();
            if trim_trailing_whitespace.unwrap_or(false) {
                new = crate::write::trim_trailing_whitespace(&new);
            }
            if let Some(eol) = normalize_eol {
                let mode = plan_normalize_eol(eol)?;
                new = crate::write::normalize_eol(&new, mode);
            }
            if ensure_final_newline.unwrap_or(true) {
                new = crate::write::ensure_final_newline(&new);
            }
            update_file_content(tx.pending, tx.deletions, &file_path, new);
        }

        Operation::FileCreate {
            path,
            content,
            force,
        } => {
            let file_path = tx.cwd.join(path);
            if force.unwrap_or(false) {
                if tx.pending.contains_key(&file_path) || file_path.exists() {
                    let _ = read_file_content(tx.pending, &file_path, tx.cwd)?;
                }
                update_file_content(tx.pending, tx.deletions, &file_path, content.clone());
            } else {
                if tx.pending.contains_key(&file_path) || file_path.exists() {
                    anyhow::bail!("file already exists: {path}");
                }
                update_file_content(tx.pending, tx.deletions, &file_path, content.clone());
            }
        }

        Operation::PatchApply { diff } => {
            let patched_files = apply_patch_with_loader(diff, |path| {
                let file_path = tx.cwd.join(path);
                Ok(read_file_content(tx.pending, &file_path, tx.cwd)?.to_string())
            })?;
            for (rel_path, patched_content) in patched_files {
                let file_path = tx.cwd.join(&rel_path);
                update_file_content(tx.pending, tx.deletions, &file_path, patched_content);
            }
        }

        Operation::Read { path, lines } => {
            execute_read_op(path, lines, tx)?;
        }

        Operation::Search { .. } => {
            execute_search_op(op, tx)?;
        }

        Operation::FileDelete { path } => {
            let file_path = tx.cwd.join(path);
            let created_in_tx = match tx.pending.get(&file_path) {
                Some((original, _)) => original.is_empty() && !file_path.exists(),
                None => {
                    let _ = read_file_content(tx.pending, &file_path, tx.cwd)?;
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
        p.strip_prefix(cwd)
            .unwrap_or(p)
            .to_string_lossy()
            .to_string()
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
        searches: Vec::new(),
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

fn print_diffs(changes: &[(PathBuf, String, String)], cwd: &Path) {
    let diffs: Vec<_> = changes
        .iter()
        .map(|(p, old, new)| {
            let display = p.strip_prefix(cwd).unwrap_or(p);
            unified_diff(&display.to_string_lossy(), old, new)
        })
        .collect();
    let total = diffs.iter().filter(|d| d.has_changes).count();
    let result = DiffResult {
        diffs,
        total_files_changed: total,
    };
    print!("{}", format_diff_result(&result));
}

// ---------------------------------------------------------------------------
// Lifecycle helpers (extracted from run() for testability)
// ---------------------------------------------------------------------------

struct LifecycleError {
    message: String,
    kind: &'static str,
}

fn run_format_steps(steps: &[plan::FormatStep], cwd: &Path) -> Result<(), LifecycleError> {
    for (index, step) in steps.iter().enumerate() {
        let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
        let result = run_shell_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(status) if !status.success() => {
                let msg = format!(
                    "format step failed (step {}, {})",
                    index + 1,
                    describe_exit_status(status)
                );
                eprintln!("tx: {msg}");
                return Err(LifecycleError {
                    message: msg,
                    kind: "format_failed",
                });
            }
            Err(e) => {
                let msg = format!("format step error (step {}): {e}", index + 1);
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

fn run_validate_steps(steps: &[plan::ValidationStep], cwd: &Path) -> Result<(), LifecycleError> {
    for (index, step) in steps.iter().enumerate() {
        let timeout_secs = step.timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
        let result = run_shell_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(status) if status.success() => {}
            Ok(status) => {
                let msg = format!(
                    "required validation failed (step {}, {})",
                    index + 1,
                    describe_exit_status(status)
                );
                eprintln!("tx: {msg}");
                if step.required.unwrap_or(false) {
                    return Err(LifecycleError {
                        message: msg,
                        kind: "validation_failed",
                    });
                }
            }
            Err(e) => {
                let msg = format!("validation error (step {}): {e}", index + 1);
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
            let _ = std::fs::remove_file(path);
        } else {
            let _ = atomic_write(path, original, &noop_policy);
        }
    }
    for path in deletions {
        if let Some((orig, _)) = pending.get(path) {
            if existed_before.contains(path) {
                let _ = atomic_write(path, orig, &noop_policy);
            }
        }
    }
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

    if let Some(ref v) = plan.version {
        if v != crate::plan::SCHEMA_VERSION {
            let msg = format!(
                "unsupported plan version '{v}' (this build supports version {})",
                crate::plan::SCHEMA_VERSION
            );
            if global.json {
                emit_error_json("parse_error", &msg);
            } else {
                eprintln!("tx: {msg}");
            }
            return Ok(exit::PARSE_ERROR);
        }
    }

    if let Err(e) = validate_plan_operations(&plan) {
        if global.json {
            emit_error_json("parse_error", &e.to_string());
        } else {
            eprintln!("tx: plan parse error: {e}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    // 3. Resolve working directory (plan.cwd overrides global --cwd).
    let cwd: PathBuf = if let Some(ref c) = plan.cwd {
        PathBuf::from(c)
    } else {
        global.resolve_cwd()?
    };

    // 4. Execute all operations, collecting changes in memory (no writes).
    let mut pending: HashMap<PathBuf, (String, String)> = HashMap::new();
    let mut deletions: HashSet<PathBuf> = HashSet::new();
    let mut has_non_idempotent_replace = false;
    let mut total_replace_matches = 0usize;
    let mut tx_reads: Vec<TxReadResult> = Vec::new();
    let mut tx_searches: Vec<TxSearchResult> = Vec::new();

    for (i, op) in plan.operations.iter().enumerate() {
        if let Operation::Replace { if_exists, .. } = op {
            if !if_exists {
                has_non_idempotent_replace = true;
            }
        }
        let mut tx = TxState {
            pending: &mut pending,
            deletions: &mut deletions,
            tx_reads: &mut tx_reads,
            tx_searches: &mut tx_searches,
            cwd: &cwd,
        };
        let result = execute_operation(op, &mut tx);
        match result {
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
    //    Drain the pending map to move data instead of cloning (#224).
    let drained: Vec<(PathBuf, (String, String))> = pending.drain().collect();
    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, (original, current)) in &drained {
        let write_policy = build_write_policy(&plan, global, path)?;
        let final_content = apply_policy(current, &write_policy);
        if *original != final_content {
            changes.push((path.clone(), original.clone(), final_content));
        }
    }
    // Re-populate pending from drained so rollback_strict still works.
    let pending: HashMap<PathBuf, (String, String)> = drained.into_iter().collect();
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
                let mut output =
                    build_tx_output("success", true, &changes, &deletions, &existed_before, &cwd);
                output.reads = std::mem::take(&mut tx_reads);
                output.searches = std::mem::take(&mut tx_searches);
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
                &cwd,
            );
            output.reads = std::mem::take(&mut tx_reads);
            output.searches = std::mem::take(&mut tx_searches);
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
            print_diffs(&changes, &cwd);
        }

        // 7. Run format steps, then validation steps.
        let lifecycle_err = plan
            .format
            .as_deref()
            .map(|steps| run_format_steps(steps, &cwd))
            .unwrap_or(Ok(()))
            .err()
            .or_else(|| {
                plan.validate
                    .as_deref()
                    .and_then(|steps| run_validate_steps(steps, &cwd).err())
            });

        if let Some(err) = lifecycle_err {
            if plan.strict {
                rollback_strict(&changes, &pending, &deletions, &existed_before);
                let rollback_msg = format!("strict mode -- all changes reverted ({})", err.message);
                if global.json {
                    emit_error_json_with_prefix(err.kind, "rollback", &rollback_msg);
                } else {
                    eprintln!("tx: {rollback_msg}");
                }
                return Ok(exit::ROLLBACK);
            }
            if global.json {
                emit_error_json(err.kind, &err.message);
            }
            return Ok(exit::VALIDATION_FAILED);
        }

        if replace_no_matches {
            return Ok(exit::NO_MATCHES);
        }
        if global.json {
            let mut output =
                build_tx_output("success", true, &changes, &deletions, &existed_before, &cwd);
            output.reads = std::mem::take(&mut tx_reads);
            output.searches = std::mem::take(&mut tx_searches);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        return Ok(exit::SUCCESS);
    }

    if replace_no_matches {
        return Ok(exit::NO_MATCHES);
    }

    // Default / --diff mode: show unified diffs.
    if global.json {
        let mut output =
            build_tx_output("success", true, &changes, &deletions, &existed_before, &cwd);
        output.reads = std::mem::take(&mut tx_reads);
        output.searches = std::mem::take(&mut tx_searches);
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !changes.is_empty() {
        print_diffs(&changes, &cwd);
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
            // -NoProfile skips profile loading which can take 1-2 seconds on CI.
            // Single Write() call instead of 4000 iterations keeps runtime well
            // under the timeout even on slow runners.
            "powershell -NoProfile -Command \"[Console]::Error.Write('x' * 800000); exit 0\""
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

        let content = read_file_content(&mut pending, &path, dir.path()).unwrap();

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
                {"cmd": shell_stderr_spam(), "required": true, "timeout": 10}
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
