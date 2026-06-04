use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{DiffResult, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::ops::doc::{
    FileFormat, deep_merge, delete_at_selector, delete_where, detect_format, move_at_path,
    navigate_mut, parse_doc, serialize_value_preserving, set_at_path, update_matching,
};
use crate::ops::md::{
    dedupe_headings_in, insert_after_heading_in, insert_before_heading_in, replace_section_in,
    table_append_for_tx, upsert_bullet_in,
};
use crate::ops::patch::apply_patch_with_loader;
use crate::ops::replace::{
    ReplaceModeError, compile_replace_regex, replace_content, replacement_text,
    validate_replace_mode,
};
use crate::plan::{self, Operation, Plan};
use crate::selector;
use crate::write::{WritePolicy, apply_policy, atomic_create_new, atomic_write};
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    lints: Vec<TxLintResult>,
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

/// A lint result in the tx output.
#[derive(Serialize)]
struct TxLintResult {
    path: String,
    issue_count: usize,
    issues: Vec<crate::cmd::md::LintIssue>,
}

#[derive(Debug, Args)]
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

/// Result of a shell command execution with captured stderr.
struct ShellResult {
    status: std::process::ExitStatus,
    /// First N bytes of stderr output (truncated for safety).
    stderr_head: String,
}

/// Maximum bytes of stderr to capture from lifecycle commands.
const LIFECYCLE_STDERR_MAX: usize = 512;

/// Run a shell command with a timeout. Returns the exit status and
/// a truncated snippet of stderr, or an error if the command times out
/// or fails to start.
fn run_shell_with_timeout(cmd: &str, timeout_secs: u64, cwd: &Path) -> anyhow::Result<ShellResult> {
    let mut child = host_shell_command(cmd, cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Read stderr in a background thread to avoid blocking if the pipe fills.
    let stderr_handle = child.stderr.take().expect("stderr piped");
    let reader_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = vec![0u8; LIFECYCLE_STDERR_MAX + 1];
        let mut reader = stderr_handle;
        let mut total = 0;
        loop {
            match reader.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if total > LIFECYCLE_STDERR_MAX {
                        // Drain remaining stderr so the child is not blocked.
                        let mut discard = [0u8; 4096];
                        while reader.read(&mut discard).unwrap_or(0) > 0 {}
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let cap = total.min(LIFECYCLE_STDERR_MAX);
        let text = String::from_utf8_lossy(&buf[..cap]).to_string();
        if total > LIFECYCLE_STDERR_MAX {
            format!("{text}... (truncated)")
        } else {
            text
        }
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(status) = child.try_wait()? {
            let stderr_head = reader_thread.join().unwrap_or_default();
            return Ok(ShellResult {
                status,
                stderr_head,
            });
        }
        if std::time::Instant::now() >= deadline {
            kill_process_tree(&mut child);
            let stderr_head = reader_thread.join().unwrap_or_default();
            anyhow::bail!("timed out after {timeout_secs}s: {stderr_head}");
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
        // Guard: if the child has already exited, id() returns 0.
        // killpg(0, SIGKILL) would kill our own process group.
        if pid > 0 {
            // SAFETY: killpg is a POSIX function that sends a signal to a
            // process group. We pass the child's PID (which is also its
            // PGID due to process_group(0)) and SIGKILL (9).
            #[expect(unsafe_code)]
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
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
        Operation::TidyFix { .. } => "tidy.fix",
        Operation::FileCreate { .. } => "file.create",
        Operation::FileDelete { .. } => "file.delete",
        Operation::FileRename { .. } => "file.rename",
        Operation::PatchApply { .. } => "patch.apply",
        Operation::Read { .. } => "read",
        Operation::Search { .. } => "search",
        Operation::MdLintAgents { .. } => "md.lint_agents",
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
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. }
        | Operation::Read { .. }
        | Operation::MdLintAgents { .. }
        | Operation::PatchApply { .. } => Ok(()),
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
fn read_file_content<'a>(
    pending: &'a mut HashMap<PathBuf, (String, String)>,
    path: &Path,
) -> anyhow::Result<&'a str> {
    match pending.entry(path.to_path_buf()) {
        Entry::Occupied(entry) => Ok(&entry.into_mut().1),
        Entry::Vacant(entry) => {
            // Callers pass already-resolved paths (cwd.join(rel_path)),
            // so read directly without double-joining (#385).
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
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

/// Load a structured document from the cache (or parse from pending buffer)
/// and return a mutable reference to the parsed JSON value. Serialization is
/// deferred until the cache is flushed.
///
/// Callers mutate the returned `&mut Value` directly using borrowed references
/// from the `Operation` match arm, eliminating the need to clone `String` and
/// `Value` fields into a closure.
fn get_doc_root<'a>(
    pending: &mut HashMap<PathBuf, (String, String)>,
    doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    path: &str,
    cwd: &Path,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let file_path = cwd.join(path);

    // Ensure the document is in the cache.
    if !doc_cache.contains_key(&file_path) {
        let content = read_file_content(pending, &file_path)?;
        let format = detect_format(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        let root = parse_doc(content, &format).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
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
    doc_cache: &'a mut HashMap<PathBuf, CachedDoc>,
    tx_reads: &'a mut Vec<TxReadResult>,
    tx_searches: &'a mut Vec<TxSearchResult>,
    tx_lints: &'a mut Vec<TxLintResult>,
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
        ..
    } = op
    else {
        unreachable!()
    };
    let regex_mode = mode.as_deref() == Some("regex");
    let use_regex = regex_mode || *case_insensitive;
    let replacement = replacement_text(from, to, insert_before, insert_after, use_regex);
    let compiled_re = compile_replace_regex(from, regex_mode, *case_insensitive, *multiline)?;

    if let Some(p) = path {
        let file_path = tx.cwd.join(p);
        let content = read_file_content(tx.pending, &file_path)?;
        let (replaced, match_count) =
            replace_content(content, from, &replacement, compiled_re.as_ref(), *nth);
        if match_count > 0 {
            let owned = replaced.into_owned();
            update_file_content(tx.pending, tx.deletions, &file_path, owned);
        }
        Ok(match_count)
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
            let content = if tx.pending.contains_key(&file_path) {
                let result = read_file_content(tx.pending, &file_path);
                match result {
                    Ok(c) => c.to_owned(),
                    Err(e) => {
                        if !tx.structured && !tx.quiet {
                            eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                        }
                        continue;
                    }
                }
            } else {
                match std::fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        if !tx.structured && !tx.quiet {
                            eprintln!("tx: replace: skipping {}: {e}", file_path.display());
                        }
                        continue;
                    }
                }
            };
            let (replaced, match_count) =
                replace_content(&content, from, &replacement, compiled_re.as_ref(), *nth);
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
    read_file_content(tx.pending, &file_path)?;
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
        let range = crate::cmd::read::parse_line_range(spec)?;
        crate::cmd::read::select_lines(content, range)
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
            if !read_and_probe(tx.pending, file_path)? {
                continue; // binary file, skip
            }
        } else {
            read_file_content(tx.pending, file_path)?;
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
            | Operation::MdDedupeHeadings { .. }
            | Operation::PatchApply { .. }
            | Operation::Read { .. }
            | Operation::Search { .. }
            | Operation::MdLintAgents { .. }
            | Operation::TidyFix { .. }
    )
}

fn execute_operation(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::Replace { .. } => {
            return execute_replace_op(op, tx);
        }

        Operation::DocSet {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            set_at_path(root, &sel, value.clone()).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        }

        Operation::DocDelete { path, selector } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            delete_at_selector(root, &sel).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        }

        Operation::DocMerge { path, value } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            deep_merge(root, value);
        }

        Operation::DocAppend {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let target =
                navigate_mut(root, &sel, false).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
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
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let target =
                navigate_mut(root, &sel, false).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
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
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let count = update_matching(root, &sel, value);
            if count == 0 {
                anyhow::bail!("{path}: no matching nodes found for selector '{selector}'");
            }
        }

        Operation::DocMove { path, from, to } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let from_sel = parse_selector(from).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let to_sel = parse_selector(to).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            move_at_path(root, &from_sel, &to_sel).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        }

        Operation::DocEnsure {
            path,
            selector,
            value,
        } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            // If the path already exists, no-op.
            if selector::eval(root, &sel).is_empty() {
                set_at_path(root, &sel, value.clone())
                    .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            }
        }

        Operation::DocDeleteWhere {
            path,
            selector,
            predicate,
        } => {
            let root = get_doc_root(tx.pending, tx.doc_cache, path, tx.cwd)
                .map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let sel = parse_selector(selector).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            delete_where(root, &sel, predicate).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
        }

        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path)?;
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
            let file_content = read_file_content(tx.pending, &file_path)?;
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
            let file_content = read_file_content(tx.pending, &file_path)?;
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
            let file_content = read_file_content(tx.pending, &file_path)?;
            let new_content = upsert_bullet_in(file_content, heading, bullet)
                .ok_or_else(|| anyhow::anyhow!("heading not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdTableAppend { path, heading, row } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path)?;
            let new_content = table_append_for_tx(file_content, heading, row)
                .ok_or_else(|| anyhow::anyhow!("heading/table not found: {heading}"))?;
            update_file_content(tx.pending, tx.deletions, &file_path, new_content);
        }

        Operation::MdDedupeHeadings { path } => {
            let file_path = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, &file_path)?;
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
            let content = read_file_content(tx.pending, &file_path)?.to_owned();
            let policy = WritePolicy {
                ensure_final_newline: ensure_final_newline.unwrap_or(true),
                trim_trailing_whitespace: trim_trailing_whitespace.unwrap_or(false),
                normalize_eol: if let Some(eol) = normalize_eol {
                    plan_normalize_eol(eol)?
                } else {
                    EolMode::Keep
                },
            };
            let new = crate::write::apply_policy(&content, &policy);
            if content != *new {
                update_file_content(tx.pending, tx.deletions, &file_path, new.into_owned());
            }
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
                    let _ = read_file_content(tx.pending, &file_path)?;
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

        Operation::PatchApply { diff } => {
            let patched_files = apply_patch_with_loader(diff, |path| {
                let file_path = tx.cwd.join(path);
                Ok(read_file_content(tx.pending, &file_path)?.to_string())
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

        Operation::MdLintAgents { path } => {
            let file_path = tx.cwd.join(path);
            let content = read_file_content(tx.pending, &file_path)?;
            let issues = crate::cmd::md::lint_agents_content(content);
            tx.tx_lints.push(TxLintResult {
                path: path.clone(),
                issue_count: issues.len(),
                issues,
            });
        }

        Operation::FileDelete { path } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                anyhow::bail!("target is not a file: {path}");
            }
            let created_in_tx = match tx.pending.get(&file_path) {
                Some((original, _)) => original.is_empty() && !file_path.exists(),
                None => {
                    let _ = read_file_content(tx.pending, &file_path)?;
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

            // If source and destination are the same path, no-op.
            if src_path == dst_path {
                return Ok(0);
            }

            // Read source content into pending (validates it exists).
            let content = read_file_content(tx.pending, &src_path)?.to_string();

            // Check destination does not already exist (unless force).
            if !force {
                let dst_exists = tx.pending.contains_key(&dst_path) || dst_path.exists();
                if dst_exists {
                    anyhow::bail!("destination already exists: {to}");
                }
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
    }
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

fn build_error_output(
    error_kind: &'static str,
    legacy_error_prefix: &str,
    error: &str,
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
        error: Some(format!("{legacy_error_prefix}: {error}")),
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
    compact: bool,
) {
    emit_output_json(
        &build_error_output(error_kind, legacy_error_prefix, error),
        compact,
    );
}

fn emit_error_json(error_kind: &'static str, error: &str, compact: bool) {
    emit_error_json_with_prefix(error_kind, legacy_error_prefix(error_kind), error, compact);
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
    let total = diffs.iter().filter(|d| d.has_changes).count();
    let result = DiffResult {
        diffs,
        total_files_changed: total,
    };
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
        let result = run_shell_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(ShellResult {
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
        let result = run_shell_with_timeout(&step.cmd, timeout_secs, cwd);
        match result {
            Ok(ShellResult {
                status,
                stderr_head,
            }) if status.success() => {
                let _ = stderr_head; // Discard stderr on success.
            }
            Ok(ShellResult {
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
    let mut has_non_idempotent_replace = false;
    let mut total_replace_matches = 0usize;
    let mut tx_reads: Vec<TxReadResult> = Vec::new();
    let mut tx_searches: Vec<TxSearchResult> = Vec::new();
    let mut tx_lints: Vec<TxLintResult> = Vec::new();
    let mut doc_cache: HashMap<PathBuf, CachedDoc> = HashMap::new();

    for (i, op) in plan.operations.iter().enumerate() {
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
            doc_cache: &mut doc_cache,
            tx_reads: &mut tx_reads,
            tx_searches: &mut tx_searches,
            tx_lints: &mut tx_lints,
            cwd,
            quiet,
            structured,
        };
        match execute_operation(op, &mut tx) {
            Ok(count) => total_replace_matches += count,
            Err(e) => {
                anyhow::bail!("operation {} ({}) failed: {e}", i + 1, op_label(op));
            }
        }
    }

    flush_doc_cache(&mut pending, &mut deletions, &mut doc_cache)?;

    let existed_before: HashSet<PathBuf> = pending
        .keys()
        .filter(|path| path.exists())
        .cloned()
        .collect();

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

/// Apply pending changes to disk: backup originals, write modified files,
/// delete removed files, finalize backup session.
fn commit_changes(
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> anyhow::Result<()> {
    let mut backup = crate::backup::BackupSession::new(cwd)?;
    for (path, _, _) in changes {
        if deletions.contains(path) {
            backup.save_before_delete(path)?;
        } else {
            backup.save_before_write(path)?;
        }
    }
    for path in deletions {
        backup.save_before_delete(path)?;
    }

    let noop_policy = WritePolicy::default();
    for (path, _, new_content) in changes {
        if deletions.contains(path) {
            std::fs::remove_file(path)?;
        } else {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent)?;
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
            std::fs::remove_file(path)?;
        }
    }
    backup.finalize()?;
    Ok(())
}

/// Build a JSON error string without writing to stdout.
#[cfg(feature = "mcp")]
fn make_error_json(error_kind: &'static str, error: &str) -> String {
    make_error_json_with_prefix(error_kind, legacy_error_prefix(error_kind), error)
}

/// Build a JSON error string with an explicit legacy prefix.
#[cfg(feature = "mcp")]
fn make_error_json_with_prefix(
    error_kind: &'static str,
    legacy_error_prefix: &str,
    error: &str,
) -> String {
    serde_json::to_string_pretty(&build_error_output(error_kind, legacy_error_prefix, error))
        .unwrap_or_default()
}

/// Execute a parsed [`Plan`] directly and return the exit code and JSON
/// result string. Does **not** write to stdout or stderr.
///
/// This is the in-process equivalent of spawning
/// `patchloom --json tx <plan-file> --apply` and capturing its output.
/// The MCP server calls this to avoid subprocess overhead.
#[cfg(feature = "mcp")]
pub(crate) fn execute_plan_direct(plan: Plan, cwd: &Path) -> anyhow::Result<(u8, String)> {
    // Validate plan.
    if plan.version != crate::plan::SCHEMA_VERSION {
        let msg = format!(
            "unsupported plan version '{}' (this build supports version {})",
            plan.version,
            crate::plan::SCHEMA_VERSION
        );
        return Ok((exit::PARSE_ERROR, make_error_json("parse_error", &msg)));
    }
    if let Err(e) = validate_plan_operations(&plan) {
        return Ok((
            exit::PARSE_ERROR,
            make_error_json("parse_error", &e.to_string()),
        ));
    }

    // Resolve working directory (plan.cwd overrides argument).
    let effective_cwd = resolve_plan_cwd(cwd, plan.cwd.as_deref());

    // Load project config so write_policy picks up .patchloom.toml settings.
    let mut global = GlobalFlags {
        cwd: Some(effective_cwd.to_string_lossy().into_owned()),
        ..GlobalFlags::default()
    };
    if let Some((config, _)) = crate::config::find_and_load(&effective_cwd) {
        crate::config::apply_config(&mut global, &config);
    }

    // Execute operations and collect changes in memory.
    let result = match execute_and_collect(&plan, &effective_cwd, &global, true, true) {
        Ok(r) => r,
        Err(e) => {
            return Ok((exit::ROLLBACK, make_error_json("rollback", &e.to_string())));
        }
    };

    if result.replace_no_matches {
        return Ok((exit::NO_MATCHES, String::new()));
    }

    if result.no_effective_changes {
        let mut output = build_tx_output(
            "success",
            true,
            &result.changes,
            &result.deletions,
            &result.existed_before,
            &effective_cwd,
        );
        output.reads = result.tx_reads;
        output.searches = result.tx_searches;
        output.lints = result.tx_lints;
        let json = serde_json::to_string_pretty(&output)?;
        return Ok((exit::SUCCESS, json));
    }

    // Apply: back up originals, write files.
    commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
    )?;

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd) {
        if plan.strict {
            rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            let msg = format!("strict mode -- all changes reverted ({})", err.message);
            return Ok((
                exit::ROLLBACK,
                make_error_json_with_prefix(err.kind, "rollback", &msg),
            ));
        }
        return Ok((
            exit::VALIDATION_FAILED,
            make_error_json(err.kind, &err.message),
        ));
    }

    let mut output = build_tx_output(
        "success",
        true,
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
    );
    output.reads = result.tx_reads;
    output.searches = result.tx_searches;
    output.lints = result.tx_lints;
    let json = serde_json::to_string_pretty(&output)?;
    Ok((exit::SUCCESS, json))
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: TxArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let structured = global.json || global.jsonl;
    let compact = global.jsonl;

    // 1. Read plan from file or stdin.
    let plan_path = if args.plan == "-" {
        None
    } else {
        Some(global.resolve_cwd()?.join(&args.plan))
    };
    let plan_text = if args.plan == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let plan_path = plan_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("internal error: plan path missing"))?;
        std::fs::read_to_string(plan_path).map_err(|e| {
            anyhow::anyhow!("failed to read plan file '{}': {e}", plan_path.display())
        })?
    };

    // 2. Parse plan (JSON, YAML, or TOML).
    let plan_path_hint = if args.plan == "-" {
        None
    } else {
        Some(args.plan.as_str())
    };
    let plan = match plan::parse_plan_auto(&plan_text, plan_path_hint, args.plan_format.as_deref())
    {
        Ok(p) => p,
        Err(e) => {
            if structured {
                emit_error_json("parse_error", &e.to_string(), compact);
            } else {
                eprintln!("tx: plan parse error: {e}");
            }
            return Ok(exit::PARSE_ERROR);
        }
    };

    if plan.version != crate::plan::SCHEMA_VERSION {
        let msg = format!(
            "unsupported plan version '{}' (this build supports version {})",
            plan.version,
            crate::plan::SCHEMA_VERSION
        );
        if structured {
            emit_error_json("parse_error", &msg, compact);
        } else {
            eprintln!("tx: {msg}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    if let Err(e) = validate_plan_operations(&plan) {
        if structured {
            emit_error_json("parse_error", &e.to_string(), compact);
        } else {
            eprintln!("tx: plan parse error: {e}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    // 3. Resolve working directory (plan.cwd overrides global --cwd).
    let base_cwd = global.resolve_cwd()?;
    let cwd = resolve_plan_cwd(&base_cwd, plan.cwd.as_deref());

    if plan.cwd.is_some() && !cwd.is_dir() {
        let msg = format!(
            "plan cwd is not a directory: {}",
            plan.cwd.as_deref().expect("plan.cwd checked above")
        );
        if structured {
            emit_error_json("parse_error", &msg, compact);
        } else {
            eprintln!("tx: {msg}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    // 4. Execute all operations, collecting changes in memory (no writes).
    let mut result = match execute_and_collect(&plan, &cwd, global, global.quiet, structured) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            if structured {
                emit_error_json("rollback", &msg, compact);
            } else {
                eprintln!("tx: {msg}");
            }
            return Ok(exit::ROLLBACK);
        }
    };

    // Deletions of empty files won't appear in `changes` (original == final == ""),
    // but they still count as changes for --check reporting.
    let pending_deletions = result
        .deletions
        .iter()
        .filter(|p| !result.changes.iter().any(|(c, _, _)| c == *p))
        .count();

    // 5. Output based on mode.
    if global.check {
        if result.no_effective_changes {
            if result.replace_no_matches {
                return Ok(exit::NO_MATCHES);
            }
            if structured {
                let mut output = build_tx_output(
                    "success",
                    true,
                    &result.changes,
                    &result.deletions,
                    &result.existed_before,
                    &cwd,
                );
                output.reads = std::mem::take(&mut result.tx_reads);
                output.searches = std::mem::take(&mut result.tx_searches);
                output.lints = std::mem::take(&mut result.tx_lints);
                emit_output_json(&output, compact);
            }
            return Ok(exit::SUCCESS);
        }
        if structured {
            let mut output = build_tx_output(
                "changes_detected",
                true,
                &result.changes,
                &result.deletions,
                &result.existed_before,
                &cwd,
            );
            output.reads = std::mem::take(&mut result.tx_reads);
            output.searches = std::mem::take(&mut result.tx_searches);
            output.lints = std::mem::take(&mut result.tx_lints);
            emit_output_json(&output, compact);
        } else if !global.quiet {
            println!(
                "{} file(s) would change",
                result.changes.len() + pending_deletions
            );
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        commit_changes(
            &result.changes,
            &result.deletions,
            &result.existed_before,
            &cwd,
        )?;

        // Show diffs if --diff flag is set.
        if global.diff && !result.changes.is_empty() {
            print_diffs(&result.changes, &cwd, global.should_color());
        }

        // 6. Run format steps, then validation steps.
        if let Some(err) = run_lifecycle(&plan, &base_cwd, &cwd) {
            if plan.strict {
                rollback_strict(
                    &result.changes,
                    &result.pending,
                    &result.deletions,
                    &result.existed_before,
                );
                let rollback_msg = format!("strict mode -- all changes reverted ({})", err.message);
                if structured {
                    emit_error_json_with_prefix(err.kind, "rollback", &rollback_msg, compact);
                } else {
                    eprintln!("tx: {rollback_msg}");
                }
                return Ok(exit::ROLLBACK);
            }
            if structured {
                emit_error_json(err.kind, &err.message, compact);
            }
            return Ok(exit::VALIDATION_FAILED);
        }

        if result.replace_no_matches {
            return Ok(exit::NO_MATCHES);
        }
        if structured {
            let mut output = build_tx_output(
                "success",
                true,
                &result.changes,
                &result.deletions,
                &result.existed_before,
                &cwd,
            );
            output.reads = std::mem::take(&mut result.tx_reads);
            output.searches = std::mem::take(&mut result.tx_searches);
            output.lints = std::mem::take(&mut result.tx_lints);
            emit_output_json(&output, compact);
        } else if global.show_status() {
            let n_changed = result.changes.len();
            let n_deleted = result.deletions.len();
            let total = n_changed + n_deleted;
            eprintln!("applied: {total} file(s) affected");
        }
        return Ok(exit::SUCCESS);
    }

    if result.replace_no_matches {
        return Ok(exit::NO_MATCHES);
    }

    // Default / --diff mode: show unified diffs.
    if structured {
        let mut output = build_tx_output(
            "success",
            true,
            &result.changes,
            &result.deletions,
            &result.existed_before,
            &cwd,
        );
        output.reads = std::mem::take(&mut result.tx_reads);
        output.searches = std::mem::take(&mut result.tx_searches);
        output.lints = std::mem::take(&mut result.tx_lints);
        emit_output_json(&output, compact);
    } else if !result.changes.is_empty() {
        print_diffs(&result.changes, &cwd, global.should_color());
    }
    if !result.no_effective_changes && global.show_status() {
        let n = result.changes.len() + result.deletions.len();
        eprintln!("{n} file(s) changed");
    }

    // --confirm: prompt after showing diffs, then apply if confirmed.
    if !result.no_effective_changes && global.should_apply() {
        commit_changes(
            &result.changes,
            &result.deletions,
            &result.existed_before,
            &cwd,
        )?;
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

    fn portable_path_str(p: &std::path::Path) -> String {
        p.to_str().unwrap().replace('\\', "/")
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
    fn read_and_probe_returns_true_for_text() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("text.rs");
        fs::write(&path, "fn main() {}\n").unwrap();
        let mut pending = HashMap::new();

        assert!(read_and_probe(&mut pending, &path).unwrap());
        assert!(pending.contains_key(&path));
        assert_eq!(pending[&path].1, "fn main() {}\n");
    }

    #[test]
    fn read_and_probe_returns_false_for_binary() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.bin");
        fs::write(&path, b"ELF\x00\x01\x02\x03").unwrap();
        let mut pending = HashMap::new();

        assert!(!read_and_probe(&mut pending, &path).unwrap());
        assert!(!pending.contains_key(&path));
    }

    #[test]
    fn read_and_probe_returns_false_for_invalid_utf8() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.txt");
        // Bare continuation bytes: invalid UTF-8 but no NUL (passes binary check)
        fs::write(&path, [0x80, 0x81, 0x82]).unwrap();
        let mut pending = HashMap::new();

        // Should skip gracefully (false) instead of erroring,
        // matching standalone search/replace behavior.
        assert!(!read_and_probe(&mut pending, &path).unwrap());
        assert!(!pending.contains_key(&path));
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

        // Build plan with replace + doc.set + tidy.fix.
        let plan_json = serde_json::json!({
            "version": "1",
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
                    "selector": "name",
                    "value": "new"
                },
                {
                    "op": "tidy.fix",
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

        // Verify tidy.fix.
        assert!(fs::read_to_string(&no_nl).unwrap().ends_with('\n'));
    }

    #[test]
    fn create_then_delete_in_same_tx_becomes_noop() {
        let dir = TempDir::new().unwrap();
        let new_file = dir.path().join("new.txt");
        let plan_json = serde_json::json!({
            "version": "1",
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
            "version": "1",
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
                    "selector": "name",
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
    fn legacy_error_prefix_maps_format_failed() {
        assert_eq!(legacy_error_prefix("format_failed"), "validation_failed");
        assert_eq!(legacy_error_prefix("rollback"), "rollback");
        assert_eq!(legacy_error_prefix("parse_error"), "parse_error");
    }

    #[test]
    fn build_error_output_produces_expected_shape() {
        let output = build_error_output("rollback", "rollback", "disk full");
        assert!(!output.ok);
        assert_eq!(output.status, "error");
        assert_eq!(output.error_kind, Some("rollback"));
        assert_eq!(output.error, Some("rollback: disk full".to_string()));
        assert_eq!(output.files_changed, 0);
    }

    #[test]
    fn validation_pass() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
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
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn validation_fail_required() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
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
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::VALIDATION_FAILED);
    }

    #[test]
    fn validation_pass_with_large_stderr_output() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
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
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn parse_plan_json_string() {
        let plan_json = r#"{"version": "1", "operations": [{"op": "replace", "path": "test.txt", "from": "a", "to": "b"}]}"#;
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
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

    #[test]
    fn validate_operation_rejects_empty_from() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "",
                "to": "x"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string().contains("non-empty search pattern"),
            "expected 'non-empty search pattern' error, got: {}",
            err
        );
    }

    #[test]
    fn validate_operation_rejects_nth_zero() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "to": "world",
                "nth": 0
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string().contains("1-based"),
            "expected '1-based' error, got: {}",
            err
        );
    }

    #[test]
    fn validate_operation_rejects_invert_match_with_multiline() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "search",
                "path": "test.txt",
                "pattern": "foo",
                "invert_match": true,
                "multiline": true
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string()
                .contains("invert_match and multiline cannot be combined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn tx_file_rename_basic() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        fs::write(&src, "content\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dir.path().join("new.txt"))
            }]
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
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "content\n"
        );
    }

    #[test]
    fn tx_file_rename_same_path_noop() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("same.txt");
        fs::write(&f, "data\n").unwrap();

        let path_str = portable_path_str(&f);
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": path_str,
                "to": path_str
            }]
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
        assert_eq!(fs::read_to_string(&f).unwrap(), "data\n");
    }

    #[test]
    fn tx_file_rename_dst_exists_without_force_rolls_back() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        fs::write(&src, "source\n").unwrap();
        fs::write(&dst, "dest\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dst)
            }]
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
        // Both files unchanged.
        assert_eq!(fs::read_to_string(&src).unwrap(), "source\n");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "dest\n");
    }

    #[test]
    fn tx_file_rename_dst_exists_with_force() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        fs::write(&src, "source\n").unwrap();
        fs::write(&dst, "dest\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dst),
                "force": true
            }]
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
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "source\n");
    }

    #[test]
    fn tx_create_then_rename() {
        let dir = TempDir::new().unwrap();
        let created = dir.path().join("created.txt");
        let renamed = dir.path().join("renamed.txt");

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.create",
                    "path": portable_path_str(&created),
                    "content": "new file\n"
                },
                {
                    "op": "file.rename",
                    "from": portable_path_str(&created),
                    "to": portable_path_str(&renamed)
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
        assert!(!created.exists());
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "new file\n");
    }

    #[test]
    fn tx_unsupported_plan_version() {
        let dir = TempDir::new().unwrap();
        let plan_json = serde_json::json!({
            "version": "99",
            "operations": []
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

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
    fn tx_delete_then_create_without_force_succeeds() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("target.txt");
        fs::write(&f, "original\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.delete",
                    "path": portable_path_str(&f)
                },
                {
                    "op": "file.create",
                    "path": portable_path_str(&f),
                    "content": "recreated\n"
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
        assert_eq!(fs::read_to_string(&f).unwrap(), "recreated\n");
    }

    // -----------------------------------------------------------------------
    // #387: Medium-priority test gaps
    // -----------------------------------------------------------------------

    #[test]
    fn plan_normalize_eol_invalid_value_returns_error() {
        let result = super::plan_normalize_eol("bogus");
        assert!(result.is_err(), "invalid eol value should fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("invalid normalize_eol"),
            "error should name the field: {msg}"
        );
    }

    #[test]
    fn rollback_strict_restores_modified_files() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, "original").unwrap();

        let mut pending = HashMap::new();
        pending.insert(f.clone(), ("original".to_string(), "changed".to_string()));
        let deletions = HashSet::new();
        let existed_before: HashSet<PathBuf> = [f.clone()].into();

        // Simulate the apply phase: write the new content.
        crate::write::atomic_write(&f, "changed", &WritePolicy::default()).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "changed");

        // Rollback should restore to original.
        rollback_strict(
            &[(f.clone(), "original".to_string(), "changed".to_string())],
            &pending,
            &deletions,
            &existed_before,
        );
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            "original",
            "rollback_strict should restore the original content"
        );
    }

    #[test]
    fn tx_doc_append_to_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.append", "path": "{}", "selector": "items", "value": 42}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::ROLLBACK,
            "doc.append to non-array should roll back"
        );
    }

    #[test]
    fn tx_doc_prepend_to_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.prepend", "path": "{}", "selector": "items", "value": 99}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::ROLLBACK,
            "doc.prepend to non-array should roll back"
        );
    }

    #[test]
    fn tx_doc_delete_where_on_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.delete_where", "path": "{}", "selector": "items", "predicate": "x=1"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::ROLLBACK,
            "doc.delete_where on non-array should roll back"
        );
    }

    #[test]
    fn tx_strict_mode_rollback_on_operation_failure() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.txt");
        fs::write(&f, "original content").unwrap();

        // Operation 1 succeeds (replace), operation 2 fails (read nonexistent).
        // Strict mode should revert operation 1's changes.
        let plan = format!(
            r#"{{
  "version": "1",
  "strict": true,
  "operations": [
    {{"op": "replace", "path": "{}", "from": "original", "to": "modified"}},
    {{"op": "read", "path": "nonexistent-file.txt"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::ROLLBACK,
            "strict plan with failing op should roll back"
        );

        // File should remain untouched because execution failed before any writes.
        let content = fs::read_to_string(&f).unwrap();
        assert_eq!(content, "original content", "file should remain unchanged");
    }

    #[test]
    fn tx_invalid_normalize_eol_in_plan() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("file.txt");
        fs::write(&f, "hello\n").unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "tidy.fix", "path": "{}", "normalize_eol": "bogus"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::ROLLBACK,
            "invalid normalize_eol should cause rollback"
        );
    }

    #[test]
    fn undo_list_json_output() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "v1").unwrap();
        let mut session = crate::backup::BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap().unwrap();

        let args = crate::cmd::undo::UndoArgs {
            list: true,
            session: None,
            apply: false,
        };
        let mut global = GlobalFlags {
            json: true,
            quiet: false,
            cwd: Some(dir.path().to_string_lossy().to_string()),
            ..GlobalFlags::default()
        };
        let code = crate::cmd::undo::run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Also test JSONL mode.
        global.json = false;
        global.jsonl = true;
        let args2 = crate::cmd::undo::UndoArgs {
            list: true,
            session: None,
            apply: false,
        };
        let code2 = crate::cmd::undo::run(args2, &global).unwrap();
        assert_eq!(code2, exit::SUCCESS);
    }

    #[test]
    fn undo_dry_run_json_output() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("b.txt");
        fs::write(&file, "v1").unwrap();
        let mut session = crate::backup::BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Modify file so dry-run shows changes.
        fs::write(&file, "v2").unwrap();

        let args = crate::cmd::undo::UndoArgs {
            list: false,
            session: Some(ts),
            apply: false,
        };
        let global = GlobalFlags {
            json: true,
            quiet: false,
            cwd: Some(dir.path().to_string_lossy().to_string()),
            ..GlobalFlags::default()
        };
        let code = crate::cmd::undo::run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }
}
