//! Unified-diff check/apply/merge CLI surface.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). One module owns
//! patch check/apply/merge modes, agent JSON/JSONL honesty, and exit/kind
//! mapping for multi-file results; do not split for LOC alone.

use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored};
use crate::exit;
use crate::ops::patch::{
    ApplyHunksOptions, ApplyHunksResult, ApplyHunksStatus, OnStale, PatchFile, apply_hunks,
    apply_hunks_with_options, parse_patch, record_staged_patch_dest, staged_path_exists,
    unsupported_git_meta_msg,
};
use crate::plan::Operation;
use crate::tx::engine::WriteSource;
use clap::Args;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom patch apply changes.patch
  patchloom patch apply changes.patch --apply
  patchloom patch apply sr.txt --apply --replace-all
  patchloom patch check changes.patch
  patchloom patch merge changes.patch --check
  patchloom patch merge changes.patch --apply --allow-conflicts")]
pub struct PatchArgs {
    #[command(subcommand)]
    pub action: PatchAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum PatchAction {
    Check {
        // ref:patch-mode:file
        file: Option<String>,
        // ref:patch-mode:stdin
        #[arg(long)]
        stdin: bool,
    },
    Apply {
        // ref:patch-mode:file
        file: Option<String>,
        // ref:patch-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long, value_enum, default_value_t = OnStaleCli::Fail)]
        on_stale: OnStaleCli,
        /// SEARCH/REPLACE only: update every exact match (default is unique).
        // ref:patch-mode:replace_all
        #[arg(long)]
        replace_all: bool,
    },
    Merge {
        // ref:patch-mode:file
        file: Option<String>,
        // ref:patch-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        allow_conflicts: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum OnStaleCli {
    #[default]
    Fail,
    Merge,
}

impl From<OnStaleCli> for OnStale {
    fn from(value: OnStaleCli) -> Self {
        match value {
            OnStaleCli::Fail => OnStale::Fail,
            OnStaleCli::Merge => OnStale::Merge,
        }
    }
}

enum DiffReadError {
    NoSource,
    IoError(String, std::io::Error),
    StdinError(std::io::Error),
    /// Diff bytes are binary (#1896 / #1963).
    Binary(String),
    /// Diff bytes are not valid UTF-8 (#1896 / #1963).
    InvalidEncoding(String),
    /// Blank / invalid patch *path* (not malformed diff text). Agents branch on
    /// `invalid_input` like other commands (#2152 family).
    InvalidInput(String),
}

fn classify_diff_bytes(bytes: Vec<u8>, display: &str) -> Result<String, DiffReadError> {
    match crate::files::classify_text_bytes(&bytes) {
        crate::files::TextBytesKind::Text(s) => Ok(s),
        crate::files::TextBytesKind::Binary => Err(DiffReadError::Binary(format!(
            "patch input is a binary file: {display}"
        ))),
        crate::files::TextBytesKind::InvalidUtf8 => Err(DiffReadError::InvalidEncoding(format!(
            "patch input is not valid UTF-8 text: {display}"
        ))),
    }
}

fn read_diff_stdin() -> Result<String, DiffReadError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(DiffReadError::StdinError)?;
    classify_diff_bytes(bytes, "stdin")
}

fn read_diff_input(
    file: &Option<String>,
    stdin_flag: bool,
    global: &GlobalFlags,
) -> Result<String, DiffReadError> {
    // A bare "-" path means stdin (common CLI convention); agents often pass
    // this instead of --stdin (fixrealloop).
    if let Some(path) = file {
        if path == "-" {
            read_diff_stdin()
        } else if crate::containment::is_blank_path(path) {
            // Fail closed before resolve/load so blank paths do not become
            // `parse_error` via IoError mapping (parity with create/doc/read).
            Err(DiffReadError::InvalidInput(
                "path must not be empty".to_string(),
            ))
        } else {
            // Relative patch paths resolve under --cwd (parity with `tx` / `batch`).
            let full = global
                .resolve_user_path(path)
                .map_err(|e| DiffReadError::IoError(path.clone(), std::io::Error::other(e)))?;
            let display = full.display().to_string();
            // Strict sole-path for the patch file itself (#1896).
            crate::files::load_text_strict(&full, &display).map_err(|e| {
                if crate::exit::is_binary(&e) {
                    DiffReadError::Binary(e.to_string())
                } else if crate::exit::is_invalid_encoding(&e) {
                    DiffReadError::InvalidEncoding(e.to_string())
                } else if crate::exit::is_invalid_input(&e) {
                    let msg = e.to_string();
                    if msg.contains("path must not be empty") {
                        DiffReadError::InvalidInput(msg)
                    } else {
                        // Directory / non-file / unreadable treated as IO for patch
                        // input (agents still get a fail-closed read error).
                        DiffReadError::IoError(display.clone(), std::io::Error::other(msg))
                    }
                } else if crate::exit::is_io_not_found(&e) {
                    DiffReadError::IoError(
                        display.clone(),
                        std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()),
                    )
                } else {
                    DiffReadError::IoError(display, std::io::Error::other(e.to_string()))
                }
            })
        }
    } else if stdin_flag {
        read_diff_stdin()
    } else {
        Err(DiffReadError::NoSource)
    }
}

/// Load a patch *target* file under Strict content rules with NotFound policy.
///
/// - Text: `Ok(content)`
/// - Missing + `missing_as_empty`: `Ok("")` (creation / merge check;
///   empty-hunk delete still peels `NotFound`)
/// - Missing + not empty: `Err(NotFound)`
/// - Binary / invalid UTF-8: `Err(Binary)` / `Err(InvalidEncoding)` (#1963)
/// - Permission / other non-NotFound IO from `load_text_strict`: `Err(InvalidInput)`
/// - Residual IO: `Err(Io)`
#[derive(Debug)]
enum PatchTargetError {
    NotFound,
    /// Directory or non-file path (per-file check status `error`, exit 5).
    NotAFile(String),
    /// Binary target (fail-closed `binary`, exit 1) (#1963).
    Binary(String),
    /// Invalid UTF-8 target (fail-closed `invalid_encoding`, exit 1) (#1963).
    InvalidEncoding(String),
    /// Unreadable existing path (fail-closed `invalid_input`, exit 1).
    InvalidInput(String),
    Io(String),
}

fn load_patch_target(
    path: &std::path::Path,
    display: &str,
    missing_as_empty: bool,
) -> Result<String, PatchTargetError> {
    match crate::files::load_text_strict(path, display) {
        Ok(s) => Ok(s),
        Err(e) if crate::exit::is_io_not_found(&e) => {
            if missing_as_empty {
                Ok(String::new())
            } else {
                Err(PatchTargetError::NotFound)
            }
        }
        Err(e) if crate::exit::is_binary(&e) => Err(PatchTargetError::Binary(e.to_string())),
        Err(e) if crate::exit::is_invalid_encoding(&e) => {
            Err(PatchTargetError::InvalidEncoding(e.to_string()))
        }
        Err(e) if crate::exit::is_invalid_input(&e) => {
            let msg = e.to_string();
            // Prefix match only: path display can contain "not a file".
            if msg.starts_with("target is not a file:") {
                Err(PatchTargetError::NotAFile(msg))
            } else {
                Err(PatchTargetError::InvalidInput(msg))
            }
        }
        Err(e) => {
            // load_text_strict already prefixes "failed to read {display}";
            // do not double-wrap (same class as #1916 sole-path unreadable).
            // Prefer agent_error_message so embedded OS detail is not doubled.
            Err(PatchTargetError::Io(crate::exit::agent_error_message(&e)))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct PatchFileResult {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflicts: Option<usize>,
    /// Git rename source path (one row per rename; #2106).
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    /// Git rename destination path (same as `path` when set).
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    /// `"renamed"` when this row is a path rename (#2106 / tx parity).
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct PatchFilesOutput {
    ok: bool,
    files: Vec<PatchFileResult>,
    /// Agent branch key when `ok` is false (e.g. stale check → `ambiguous`).
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    /// Human/agent summary when `ok` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Whether bytes were written (#1812). `false` for preview/`--check`.
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    /// Backup session id after a successful apply (#1802).
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

fn dest_clobber_check_result(
    cwd: &Path,
    pf: &PatchFile,
    created: &HashSet<PathBuf>,
    deleted: &HashSet<PathBuf>,
) -> Option<PatchFileResult> {
    let dest_exists = staged_path_exists(&cwd.join(&pf.path), created, deleted);
    let msg = pf.dest_clobber_msg(dest_exists)?;
    let (from, to, action) = if pf.rename_from.is_some() {
        (
            pf.rename_from.clone(),
            Some(pf.path.clone()),
            Some("renamed"),
        )
    } else if pf.copy_from.is_some() {
        (pf.copy_from.clone(), Some(pf.path.clone()), Some("copied"))
    } else {
        (None, None, None)
    };
    Some(PatchFileResult {
        path: pf.path.clone(),
        status: "already_exists",
        error: Some(msg),
        conflicts: None,
        from,
        to,
        action,
    })
}

fn patch_load_rel(pf: &PatchFile) -> &str {
    pf.copy_from
        .as_deref()
        .or(pf.rename_from.as_deref())
        .unwrap_or(pf.path.as_str())
}

fn patch_file_result(path: &str, applied: &ApplyHunksResult) -> PatchFileResult {
    PatchFileResult {
        path: path.to_string(),
        status: applied.status.as_str(),
        error: None,
        conflicts: if applied.conflicts.is_empty() {
            None
        } else {
            Some(applied.conflicts.len())
        },
        from: None,
        to: None,
        action: None,
    }
}

/// Build `PatchFileResult` list from diffs, folding git renames into one row
/// with `from`/`to`/`action: "renamed"` (#2106).
fn build_file_results(
    diffs: &[crate::diff::FileDiff],
    status: &'static str,
    renames: &[(String, String)],
) -> Vec<PatchFileResult> {
    use std::collections::HashSet;
    let rename_from: HashSet<&str> = renames.iter().map(|(f, _)| f.as_str()).collect();
    let rename_to: HashSet<&str> = renames.iter().map(|(_, t)| t.as_str()).collect();

    let mut files = Vec::with_capacity(diffs.len().saturating_add(renames.len()));
    for (from, to) in renames {
        files.push(PatchFileResult {
            path: to.clone(),
            status,
            error: None,
            conflicts: None,
            from: Some(from.clone()),
            to: Some(to.clone()),
            action: Some("renamed"),
        });
    }
    for d in diffs.iter().filter(|d| d.has_changes) {
        if rename_from.contains(d.path.as_str()) || rename_to.contains(d.path.as_str()) {
            continue;
        }
        files.push(PatchFileResult {
            path: d.path.clone(),
            status,
            error: None,
            conflicts: None,
            from: None,
            to: None,
            action: None,
        });
    }
    files
}

fn apply_patch_file(
    original: &str,
    hunks: &[crate::ops::patch::Hunk],
    options: ApplyHunksOptions,
) -> Result<ApplyHunksResult, String> {
    apply_hunks_with_options(original, hunks, options)
}

/// Insert a status label (STALE/MERGE FAILED) into the engine's error message
/// to match the original CLI error format.
///
/// Engine format: `"patch apply: path -- hunk N failed: ..."`
/// CLI format:    `"patch apply: path -- STALE: hunk N failed: ..."`
fn inject_stale_label(msg: &str, label: &str) -> String {
    // The engine error contains " -- " as separator. Insert label after it.
    if let Some(idx) = msg.find(" -- ") {
        let (prefix, rest) = msg.split_at(idx + 4);
        format!("{prefix}{label}: {rest}")
    } else {
        format!("{msg} ({label})")
    }
}

fn emit_error(global: &GlobalFlags, error: &str, error_kind: &str) -> anyhow::Result<()> {
    // Include error_kind so agents can branch (ambiguous=stale, conflicts=merge
    // conflicts) without scraping the English STALE/MERGE FAILED label.
    // applied:false matches replace/create error parity (#1835).
    if !global.emit_json(&serde_json::json!({
        "ok": false,
        "error": error,
        "error_kind": error_kind,
        "applied": false,
    }))? && !global.quiet
    {
        eprintln!("{error}");
    }
    Ok(())
}

/// Top-level `error_kind`, message, and exit code for multi-file patch when `ok` is false.
///
/// Exit codes match the shared agent table (`classify_typed_error`):
/// conflicts → 8, ambiguous/stale → 5, not_found/invalid_input → 1.
fn patch_problem_kind(results: &[PatchFileResult]) -> (&'static str, String, u8) {
    let has_stale = results.iter().any(|r| r.status == "stale");
    let has_missing = results.iter().any(|r| r.status == "missing");
    let has_error = results.iter().any(|r| r.status == "error");
    let has_conflict = results.iter().any(|r| r.status == "conflict");
    let has_already_exists = results.iter().any(|r| r.status == "already_exists");
    if has_conflict {
        (
            "conflicts",
            "one or more patch targets have merge conflicts".into(),
            exit::CONFLICTS,
        )
    } else if has_already_exists {
        (
            "already_exists",
            "one or more patch destinations already exist".into(),
            exit::FAILURE,
        )
    } else if has_stale {
        (
            "ambiguous",
            "one or more patch targets are stale (context no longer matches)".into(),
            exit::AMBIGUOUS,
        )
    } else if has_missing && !has_error {
        (
            "not_found",
            "one or more patch targets are missing".into(),
            exit::FAILURE,
        )
    } else if has_error {
        (
            "invalid_input",
            "one or more patch targets could not be read".into(),
            exit::FAILURE,
        )
    } else {
        (
            "ambiguous",
            "one or more patch targets failed".into(),
            exit::AMBIGUOUS,
        )
    }
}

fn emit_patch_files_output(
    global: &GlobalFlags,
    ok: bool,
    results: &[PatchFileResult],
    applied: Option<bool>,
    backup_session: Option<String>,
) -> anyhow::Result<()> {
    let (error_kind, error) = if ok {
        (None, None)
    } else {
        let (k, e, _) = patch_problem_kind(results);
        (Some(k), Some(e))
    };
    if global.json {
        let output = PatchFilesOutput {
            ok,
            files: results.to_vec(),
            error_kind,
            error: error.clone(),
            applied,
            backup_session: backup_session.clone(),
        };
        global.emit_json(&output)?;
    } else if global.jsonl {
        // Stream per-file rows, then a summary trailer (replace/tidy/search parity)
        // so agents get ok / error_kind / applied without scraping exit alone.
        global.emit_json_items(results)?;
        global.emit_json(&serde_json::json!({
            "type": "summary",
            "ok": ok,
            "error_kind": error_kind,
            "error": error,
            "applied": applied,
            "backup_session": backup_session,
            "file_count": results.len(),
        }))?;
    } else if !global.quiet {
        for r in results {
            let label = match r.status {
                "clean" | "unchanged" => "clean",
                "would_change" => "would change",
                "stale" => "STALE",
                "missing" => "MISSING",
                "error" => "ERROR",
                "conflict" => "CONFLICT",
                "applied" => "applied",
                other => other,
            };
            if let Some(err) = &r.error {
                eprintln!("patch check: {} -- {}: {}", r.path, label, err);
            } else if let Some(n) = r.conflicts {
                eprintln!("patch check: {} -- {} ({} conflicts)", r.path, label, n);
            } else if r.status != "clean" && r.status != "unchanged" && r.status != "applied" {
                eprintln!("patch check: {} -- {}", r.path, label);
            }
        }
    }
    Ok(())
}

pub fn run(args: PatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "patch: action={:?}, apply={}, check={}",
        std::mem::discriminant(&args.action),
        global.apply,
        global.check
    );
    let (file, stdin_flag, merge_mode, apply_options) = match &args.action {
        PatchAction::Check { file, stdin } => {
            (file.clone(), *stdin, false, ApplyHunksOptions::default())
        }
        PatchAction::Apply {
            file,
            stdin,
            on_stale,
            replace_all: _,
        } => (
            file.clone(),
            *stdin,
            false,
            ApplyHunksOptions {
                on_stale: (*on_stale).into(),
                allow_conflicts: false,
            },
        ),
        PatchAction::Merge {
            file,
            stdin,
            allow_conflicts,
        } => (
            file.clone(),
            *stdin,
            true,
            ApplyHunksOptions {
                on_stale: OnStale::Merge,
                allow_conflicts: *allow_conflicts,
            },
        ),
    };

    let cwd = global.resolve_cwd()?;
    let diff_text = match read_diff_input(&file, stdin_flag, global) {
        Ok(text) => text,
        Err(DiffReadError::NoSource) => {
            emit_error(
                global,
                "patch: must specify --file <path> or --stdin",
                "parse_error",
            )?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::IoError(path, e)) => {
            // Missing patch file is not a parse failure; agents branch on
            // error_kind (MPI 2026-07-16: parse_error misclassified NotFound).
            let (kind, code) = if e.kind() == std::io::ErrorKind::NotFound {
                ("not_found", exit::FAILURE)
            } else {
                ("parse_error", exit::PARSE_ERROR)
            };
            // load_text_strict (and stdin map) already include path/context in
            // `e`; do not re-prefix "failed to read" (sibling of #1916).
            let msg = {
                let detail = e.to_string();
                if detail.contains("failed to read") {
                    format!("patch: {detail}")
                } else {
                    format!("patch: failed to read '{path}': {detail}")
                }
            };
            emit_error(global, &msg, kind)?;
            return Ok(code);
        }
        Err(DiffReadError::StdinError(e)) => {
            emit_error(
                global,
                &format!("patch: failed to read stdin: {e}"),
                "parse_error",
            )?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::Binary(msg)) => {
            emit_error(global, &format!("patch: {msg}"), "binary")?;
            return Ok(exit::FAILURE);
        }
        Err(DiffReadError::InvalidEncoding(msg)) => {
            emit_error(global, &format!("patch: {msg}"), "invalid_encoding")?;
            return Ok(exit::FAILURE);
        }
        Err(DiffReadError::InvalidInput(msg)) => {
            // Blank path (and similar): stable agent peel, not parse_error.
            let detail = if msg.contains("path must not be empty") {
                "path must not be empty".to_string()
            } else {
                format!("patch: {msg}")
            };
            emit_error(global, &detail, "invalid_input")?;
            return Ok(exit::FAILURE);
        }
    };

    crate::verbose!("patch: diff text length={}", diff_text.len());
    let replace_all = match &args.action {
        PatchAction::Apply { replace_all, .. } => *replace_all,
        _ => false,
    };
    if replace_all && !crate::ops::search_replace::looks_like_search_replace(&diff_text) {
        emit_error(
            global,
            "patch: --replace-all is only valid for SEARCH/REPLACE documents",
            "invalid_input",
        )?;
        return Ok(exit::FAILURE);
    }
    if crate::ops::begin_patch::looks_like_begin_patch(&diff_text) {
        if crate::ops::search_replace::has_search_replace_marker(&diff_text) {
            emit_error(
                global,
                "patch: mixed Begin Patch and SEARCH/REPLACE grammar is not supported",
                "parse_error",
            )?;
            return Ok(exit::PARSE_ERROR);
        }
        if matches!(args.action, PatchAction::Check { .. }) {
            return run_begin_patch_check(global, &cwd, &diff_text);
        }
        let op = Operation::PatchApply {
            diff: diff_text,
            on_stale: apply_options.on_stale,
            allow_conflicts: apply_options.allow_conflicts,
            replace_all: false,
        };
        return finish_patch_apply(global, op, merge_mode);
    }
    if crate::ops::search_replace::looks_like_search_replace(&diff_text) {
        if matches!(args.action, PatchAction::Check { .. }) {
            return run_search_replace_check(global, &cwd, &diff_text, replace_all);
        }
        let op = Operation::PatchApply {
            diff: diff_text,
            on_stale: apply_options.on_stale,
            allow_conflicts: apply_options.allow_conflicts,
            replace_all,
        };
        return finish_patch_apply(global, op, merge_mode);
    }
    let patch_files = match parse_patch(&diff_text) {
        Ok(pf) => pf,
        Err(msg) => {
            emit_error(global, &format!("patch: parse error: {msg}"), "parse_error")?;
            return Ok(exit::PARSE_ERROR);
        }
    };

    crate::verbose!(
        "patch: parsed {} file(s), merge_mode={}",
        patch_files.len(),
        merge_mode
    );

    if matches!(args.action, PatchAction::Check { .. }) {
        // Agent honesty: "clean" used to mean "patch applies without fuzz"
        // (git apply --check), which agents misread as "nothing to do" while
        // `patch apply` preview correctly reported would_change + exit 2.
        // Align check with apply preview: would_change + CHANGES_DETECTED when
        // content would change; stale/missing/error stay fail-closed.
        let mut any_would_change = false;
        let mut any_problem = false;
        let mut results = Vec::new();
        let mut created = HashSet::new();
        let mut deleted = HashSet::new();
        for pf in &patch_files {
            if let Some(reason) = pf.unsupported.as_deref() {
                results.push(PatchFileResult {
                    path: pf.path.clone(),
                    status: "error",
                    error: Some(unsupported_git_meta_msg(&pf.path, reason)),
                    conflicts: None,
                    from: None,
                    to: None,
                    action: None,
                });
                any_problem = true;
                continue;
            }
            if let Some(row) = dest_clobber_check_result(&cwd, pf, &created, &deleted) {
                results.push(row);
                any_problem = true;
                continue;
            }
            // Git rename/copy: check loads source path (#2101 / #2171).
            let load_rel = patch_load_rel(pf);
            let file_path = cwd.join(load_rel);
            // Strict target load (#1896); creation allows missing → empty.
            let original = match load_patch_target(&file_path, load_rel, pf.is_creation) {
                Ok(s) => s,
                Err(PatchTargetError::NotFound) => {
                    let msg = format!("file not found: {}", file_path.display());
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "missing",
                        error: Some(msg.clone()),
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                    any_problem = true;
                    continue;
                }
                Err(PatchTargetError::Binary(msg)) => {
                    global.emit_error_json_kind(Some("binary"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::InvalidEncoding(msg)) => {
                    global.emit_error_json_kind(Some("invalid_encoding"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::InvalidInput(msg)) => {
                    // Unreadable prior: hard fail-closed for agents.
                    global.emit_error_json_kind(Some("invalid_input"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::NotAFile(msg) | PatchTargetError::Io(msg)) => {
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "error",
                        error: Some(msg.clone()),
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                    if !global.json && !global.jsonl && !global.quiet {
                        eprintln!("patch check: {} -- READ ERROR: {}", pf.path, msg);
                    }
                    any_problem = true;
                    continue;
                }
            };
            // Path-only copy/rename/empty-create/empty-hunk delete: content may
            // match, apply still writes (unlink).
            let is_path_op = pf.rename_from.as_ref().is_some_and(|from| from != &pf.path)
                || pf.copy_from.is_some()
                || (pf.is_creation && pf.hunks.is_empty() && pf.copy_from.is_none())
                || pf.is_deletion;
            match apply_hunks(&original, &pf.hunks) {
                Ok(new_content) if new_content == original && !is_path_op => {
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "unchanged",
                        error: None,
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                }
                Ok(_) => {
                    any_would_change = true;
                    let (from, to, action) = if is_path_op && pf.rename_from.is_some() {
                        (
                            pf.rename_from.clone(),
                            Some(pf.path.clone()),
                            Some("renamed"),
                        )
                    } else if pf.copy_from.is_some() {
                        (pf.copy_from.clone(), Some(pf.path.clone()), Some("copied"))
                    } else {
                        (None, None, None)
                    };
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "would_change",
                        error: None,
                        conflicts: None,
                        from,
                        to,
                        action,
                    });
                    record_staged_patch_dest(&cwd, pf, &mut created, &mut deleted);
                }
                Err(_) => {
                    any_problem = true;
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "stale",
                        error: None,
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                }
            }
        }
        let ok = !any_problem;
        emit_patch_files_output(global, ok, &results, Some(false), None)?;
        if !global.json && !global.jsonl && !global.quiet && any_would_change && !any_problem {
            let n = results
                .iter()
                .filter(|r| r.status == "would_change")
                .count();
            println!("{n} file(s) would change");
        }
        // Exit must match JSON error_kind (not_found/invalid_input → 1, not 5).
        return Ok(if any_problem {
            patch_problem_kind(&results).2
        } else if any_would_change {
            exit::CHANGES_DETECTED
        } else {
            exit::SUCCESS
        });
    }

    if merge_mode && (global.check || (!global.apply && !global.confirm)) {
        let check_options = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: true,
        };
        let mut results = Vec::new();
        let mut all_ok = true;
        let mut any_would_change = false;
        let mut created = HashSet::new();
        let mut deleted = HashSet::new();
        for pf in &patch_files {
            if let Some(reason) = pf.unsupported.as_deref() {
                all_ok = false;
                results.push(PatchFileResult {
                    path: pf.path.clone(),
                    status: "error",
                    error: Some(unsupported_git_meta_msg(&pf.path, reason)),
                    conflicts: None,
                    from: None,
                    to: None,
                    action: None,
                });
                continue;
            }
            if let Some(row) = dest_clobber_check_result(&cwd, pf, &created, &deleted) {
                all_ok = false;
                results.push(row);
                continue;
            }
            // Git rename/copy: load source path (parity with non-merge check).
            let load_rel = patch_load_rel(pf);
            let file_path = cwd.join(load_rel);
            // Merge check: missing target → empty except empty-hunk delete
            // (apply peels not_found; do not report clean).
            let empty_hunk_delete = pf.is_deletion && pf.hunks.is_empty();
            let original = match load_patch_target(&file_path, load_rel, !empty_hunk_delete) {
                Ok(s) => s,
                Err(PatchTargetError::Binary(msg)) => {
                    global.emit_error_json_kind(Some("binary"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::InvalidEncoding(msg)) => {
                    global.emit_error_json_kind(Some("invalid_encoding"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::InvalidInput(msg)) => {
                    global.emit_error_json_kind(Some("invalid_input"), &msg)?;
                    return Ok(exit::FAILURE);
                }
                Err(PatchTargetError::NotFound) => {
                    let msg = format!("file not found: {}", file_path.display());
                    all_ok = false;
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "missing",
                        error: Some(msg),
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                    continue;
                }
                Err(PatchTargetError::NotAFile(msg) | PatchTargetError::Io(msg)) => {
                    global.emit_error_json_kind(
                        Some("invalid_input"),
                        &format!("patch check: cannot read {}: {msg}", pf.path),
                    )?;
                    return Ok(exit::FAILURE);
                }
            };
            match apply_patch_file(&original, &pf.hunks, check_options) {
                Ok(applied) => {
                    // Conflicts are soft when --allow-conflicts (would write markers).
                    if applied.status == ApplyHunksStatus::Conflict
                        && !apply_options.allow_conflicts
                    {
                        all_ok = false;
                    }
                    // Clean means apply without fuzz, not "no content change".
                    // Pure rename: content may equal original but path still moves.
                    let is_path_rename =
                        pf.rename_from.as_ref().is_some_and(|from| from != &pf.path);
                    if applied.content != original || is_path_rename || pf.is_deletion {
                        any_would_change = true;
                    }
                    results.push(patch_file_result(&pf.path, &applied));
                    if applied.content != original
                        || is_path_rename
                        || pf.copy_from.is_some()
                        || pf.is_creation
                        || pf.is_deletion
                    {
                        record_staged_patch_dest(&cwd, pf, &mut created, &mut deleted);
                    }
                }
                Err(msg) => {
                    all_ok = false;
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "error",
                        error: Some(msg),
                        conflicts: None,
                        from: None,
                        to: None,
                        action: None,
                    });
                }
            }
        }
        // With --allow-conflicts, conflict rows are intentional would-change, not
        // top-level ok:false / error_kind:conflicts (agents branch on ok first).
        emit_patch_files_output(global, all_ok, &results, Some(false), None)?;
        let has_errors = results.iter().any(|r| r.status == "error");
        let has_conflicts = results.iter().any(|r| r.status == "conflict");
        let has_already_exists = results.iter().any(|r| r.status == "already_exists");
        let has_missing = results.iter().any(|r| r.status == "missing");
        // Identity / already-applied (content == original): exit 0 so agents
        // do not loop on exit 2 forever (parity with patch check).
        // Errors use shared kind→exit (invalid_input / not_found → 1).
        // When --allow-conflicts, derive kind from non-conflict rows so a
        // read/stale error is not masked as conflicts (exit 8).
        return Ok(if has_errors || has_already_exists || has_missing {
            if apply_options.allow_conflicts {
                let non_conflict: Vec<PatchFileResult> = results
                    .iter()
                    .filter(|r| r.status != "conflict")
                    .cloned()
                    .collect();
                patch_problem_kind(&non_conflict).2
            } else {
                patch_problem_kind(&results).2
            }
        } else if has_conflicts && !apply_options.allow_conflicts {
            exit::CONFLICTS
        } else if any_would_change {
            exit::CHANGES_DETECTED
        } else {
            exit::SUCCESS
        });
    }

    // Build the PatchApply operation and route through the engine.
    let op = Operation::PatchApply {
        diff: diff_text,
        on_stale: apply_options.on_stale,
        allow_conflicts: apply_options.allow_conflicts,
        replace_all: false,
    };
    finish_patch_apply(global, op, merge_mode)
}

fn run_search_replace_check(
    global: &GlobalFlags,
    cwd: &std::path::Path,
    diff_text: &str,
    replace_all: bool,
) -> anyhow::Result<u8> {
    let results = match crate::api::apply_search_replace_document(
        diff_text,
        cwd,
        &crate::api::ApplySearchReplaceOptions {
            replace_all,
            ..Default::default()
        },
        crate::api::ApplyMode::Preview,
        None,
    ) {
        Ok(r) => r,
        Err(e) => {
            let (kind, code) = if let Some((k, c)) = exit::classify_typed_error(&e) {
                (k, c)
            } else {
                ("parse_error", exit::PARSE_ERROR)
            };
            emit_error(global, &e.to_string(), kind)?;
            return Ok(code);
        }
    };
    let files: Vec<PatchFileResult> = results
        .iter()
        .map(|r| PatchFileResult {
            path: r.path.clone(),
            status: if r.changed {
                "would_change"
            } else {
                "unchanged"
            },
            error: None,
            conflicts: None,
            from: None,
            to: None,
            action: None,
        })
        .collect();
    let any_would_change = results.iter().any(|r| r.changed);
    emit_patch_files_output(global, true, &files, Some(false), None)?;
    Ok(if any_would_change {
        exit::CHANGES_DETECTED
    } else {
        exit::SUCCESS
    })
}

fn run_begin_patch_check(
    global: &GlobalFlags,
    cwd: &std::path::Path,
    diff_text: &str,
) -> anyhow::Result<u8> {
    let results = match crate::api::apply_begin_patch(
        diff_text,
        cwd,
        None,
        crate::api::ApplyMode::Preview,
        None,
    ) {
        Ok(r) => r,
        Err(e) => {
            let (kind, code) = if let Some((k, c)) = exit::classify_typed_error(&e) {
                (k, c)
            } else {
                ("parse_error", exit::PARSE_ERROR)
            };
            emit_error(global, &e.to_string(), kind)?;
            return Ok(code);
        }
    };
    let files: Vec<PatchFileResult> = results
        .iter()
        .map(|r| PatchFileResult {
            path: r.path.clone(),
            status: if r.changed {
                "would_change"
            } else {
                "unchanged"
            },
            error: None,
            conflicts: None,
            from: None,
            to: r.dest_path.clone(),
            action: r.dest_path.as_ref().map(|_| "renamed"),
        })
        .collect();
    let any_would_change = results.iter().any(|r| r.changed);
    emit_patch_files_output(global, true, &files, Some(false), None)?;
    Ok(if any_would_change {
        exit::CHANGES_DETECTED
    } else {
        exit::SUCCESS
    })
}

fn finish_patch_apply(global: &GlobalFlags, op: Operation, merge_mode: bool) -> anyhow::Result<u8> {
    let (cwd, result) =
        match crate::cmd::output::stage_for_write(WriteSource::Operations(vec![op]), global) {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                // Map engine errors to specific exit codes with CLI-style messages.
                // The engine error from apply_patch_with_loader already includes
                // "patch apply: <path> -- <detail>", so we add the STALE/MERGE
                // FAILED label to match the original CLI format.
                // Prefer shared classify_typed_error so missing targets peel
                // not_found (exit 1), not STALE/ambiguous (exit 5). Tx already
                // does this; sole-path binary/utf8 stay typed (fixrealloop).
                let (exit_code, kind) = if exit::is_conflicts(&e) || msg.contains("conflict(s)") {
                    (exit::CONFLICTS, "conflicts")
                } else if let Some((k, c)) = exit::classify_typed_error(&e) {
                    (c, k)
                } else {
                    // Ambiguous / stale context and remaining untyped errors.
                    (exit::AMBIGUOUS, "ambiguous")
                };
                let err = if kind == "ambiguous" {
                    // Inject the STALE/MERGE FAILED label between path and detail.
                    let label = if merge_mode { "MERGE FAILED" } else { "STALE" };
                    inject_stale_label(&msg, label)
                } else {
                    msg
                };
                emit_error(global, &err, kind)?;
                return Ok(exit_code);
            }
        };

    use crate::cmd::write_mode::{FinalizeCallbacks, finalize_report};

    // Relative rename pairs for JSON honesty (one row with from/to; #2106).
    let rename_pairs: Vec<(String, String)> = result
        .exec_result
        .renames
        .iter()
        .map(|(from, to)| {
            let from_s = crate::files::relative_display(from, &cwd)
                .to_string_lossy()
                .into_owned();
            let to_s = crate::files::relative_display(to, &cwd)
                .to_string_lossy()
                .into_owned();
            (from_s, to_s)
        })
        .collect();

    finalize_report(
        global,
        &cwd,
        result,
        true,
        FinalizeCallbacks {
            on_check: |g: &GlobalFlags, _has: bool, diffs: &[crate::diff::FileDiff]| {
                let files = build_file_results(diffs, "would_change", &rename_pairs);
                let changed = files.len();
                // Always emit JSON on --check (even empty) so agents parse stdout.
                if g.json || g.jsonl || changed > 0 {
                    emit_patch_files_output(g, true, &files, Some(false), None)?;
                }
                if changed > 0 && !(g.json || g.jsonl || g.quiet) {
                    println!("{changed} file(s) would change");
                }
                Ok(())
            },
            on_apply: |g: &GlobalFlags,
                       has: bool,
                       diffs: &[crate::diff::FileDiff],
                       _plain: Option<String>,
                       backup: Option<String>| {
                let status = if has { "applied" } else { "unchanged" };
                let files = build_file_results(diffs, status, &rename_pairs);
                emit_patch_files_output(g, true, &files, Some(has), backup)?;
                Ok(())
            },
            on_preview: |g: &GlobalFlags,
                         _has: bool,
                         diffs: &[crate::diff::FileDiff],
                         _plain: Option<String>| {
                if g.json || g.jsonl {
                    let files = build_file_results(diffs, "would_change", &rename_pairs);
                    emit_patch_files_output(g, true, &files, Some(false), None)?;
                } else {
                    print!(
                        "{}",
                        format_diff_result_colored(
                            &DiffResult {
                                diffs: diffs.to_vec()
                            },
                            g.should_color()
                        )
                    );
                }
                Ok(())
            },
            after_preview_emit: |_: &GlobalFlags| {},
            after_preview_apply: |_: &GlobalFlags| {},
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    fn file_result(status: &'static str, error: Option<&str>) -> PatchFileResult {
        PatchFileResult {
            path: "dst.rs".into(),
            status,
            error: error.map(str::to_string),
            conflicts: None,
            from: None,
            to: None,
            action: None,
        }
    }

    #[test]
    fn patch_problem_kind_uses_already_exists_status_not_english() {
        let (kind, _, code) = patch_problem_kind(&[file_result("already_exists", None)]);
        assert_eq!(kind, "already_exists");
        assert_eq!(code, exit::FAILURE);

        // Dest-clobber always sets status. Do not scrape "destination already
        // exists" English; a message-only row is not already_exists.
        let (kind, _, _) = patch_problem_kind(&[file_result(
            "error",
            Some("destination already exists: dst.rs (patch copy refuses overwrite; remove dest)"),
        )]);
        assert_ne!(
            kind, "already_exists",
            "must classify from status, not English dest-clobber text"
        );
    }

    #[test]
    fn merge_check_reports_conflict_without_writing() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
        let diff_path = tmp.path().join("stale.patch");
        std::fs::write(
            &diff_path,
            "--- a/hello.txt\n+++ b/hello.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
        )
        .unwrap();
        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.check = true;
        let code = run(
            PatchArgs {
                action: PatchAction::Merge {
                    file: Some(diff_path.to_string_lossy().into_owned()),
                    stdin: false,
                    allow_conflicts: false,
                },
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::CONFLICTS);
    }

    #[cfg(unix)]
    #[test]
    fn load_patch_target_unreadable_does_not_double_wrap() {
        // Sibling of #1916: load_text_strict already prefixes "failed to read".
        // Permission is typed InvalidInput with OS detail in Display (2026-07-23).
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("locked.txt");
        std::fs::write(&file, "secret\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&file).is_ok() {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        let err = load_patch_target(&file, "locked.txt", false).unwrap_err();
        match err {
            PatchTargetError::InvalidInput(msg) => {
                assert_eq!(
                    msg.matches("failed to read").count(),
                    1,
                    "must not double-wrap load_text_strict context: {msg}"
                );
                assert!(
                    msg.contains("locked.txt"),
                    "path should appear in message: {msg}"
                );
                assert!(
                    msg.contains("Permission denied")
                        || msg.contains("PermissionDenied")
                        || msg.contains("os error"),
                    "OS detail missing: {msg}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn merge_check_surfaces_io_error_for_unreadable_file() {
        // R3 fix: I/O errors (non-NotFound) should bail instead of silently
        // returning empty content via unwrap_or_default().
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("secret.txt");
        std::fs::write(&file, "line1\nline2\nline3\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Root (common in Docker) can still read mode-000 files. Skip when
        // permissions do not actually block reading (#1276).
        if std::fs::read_to_string(&file).is_ok() {
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let diff_path = tmp.path().join("fix.patch");
        std::fs::write(
            &diff_path,
            "--- a/secret.txt\n+++ b/secret.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+patched\n line3\n",
        )
        .unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.check = true;
        let result = run(
            PatchArgs {
                action: PatchAction::Merge {
                    file: Some(diff_path.to_string_lossy().into_owned()),
                    stdin: false,
                    allow_conflicts: false,
                },
                write: Default::default(),
            },
            &global,
        );
        // Should surface the I/O error as exit FAILURE, not silently treat as empty.
        let code = result.unwrap();
        assert_eq!(
            code,
            exit::FAILURE,
            "expected I/O error for unreadable file"
        );
        // Cleanup: restore permissions so TempDir can clean up
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn merge_check_treats_not_found_as_empty() {
        // R3 fix: NotFound should be treated as empty (new file creation),
        // not as an error.
        let tmp = TempDir::new().unwrap();
        let diff_path = tmp.path().join("new.patch");
        std::fs::write(
            &diff_path,
            "--- /dev/null\n+++ b/new_file.txt\n@@ -0,0 +1 @@\n+hello\n",
        )
        .unwrap();

        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.check = true;
        let code = run(
            PatchArgs {
                action: PatchAction::Merge {
                    file: Some(diff_path.to_string_lossy().into_owned()),
                    stdin: false,
                    allow_conflicts: false,
                },
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        // Should report changes detected (not error), treating missing file
        // as empty for new file creation.
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn inject_stale_label_inserts_after_separator() {
        let msg = "patch apply: test.txt -- hunk 1 failed: stale context";
        let result = inject_stale_label(msg, "STALE");
        assert_eq!(
            result,
            "patch apply: test.txt -- STALE: hunk 1 failed: stale context"
        );
    }

    #[test]
    fn conflict_matching_uses_precise_marker() {
        // R3 fix: the exit code logic checks for "conflict(s)" (not just
        // "conflict") to avoid false positives on messages that happen to
        // contain the word "conflict" in a different context.
        //
        // "conflict(s)" should map to CONFLICTS exit code.
        let msg_with_conflicts = "patch apply: f.txt -- 2 conflict(s) found";
        let exit_code = if msg_with_conflicts.contains("conflict(s)") {
            exit::CONFLICTS
        } else {
            exit::AMBIGUOUS
        };
        assert_eq!(exit_code, exit::CONFLICTS);

        // A message with "conflict" but NOT "conflict(s)" should NOT
        // trigger the CONFLICTS exit code.
        let msg_generic = "patch apply: f.txt -- conflicting base version";
        let exit_code2 = if msg_generic.contains("conflict(s)") {
            exit::CONFLICTS
        } else {
            exit::AMBIGUOUS
        };
        assert_eq!(exit_code2, exit::AMBIGUOUS);
    }

    #[test]
    fn inject_stale_label_fallback_without_separator() {
        let msg = "some other error";
        let result = inject_stale_label(msg, "STALE");
        assert_eq!(result, "some other error (STALE)");
    }

    #[test]
    fn patch_apply_json_output_on_success() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.txt");
        std::fs::write(&file, "line one\nline two\nline three\n").unwrap();
        let diff_path = tmp.path().join("fix.patch");
        std::fs::write(
            &diff_path,
            "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line one\n-line two\n+line TWO\n line three\n",
        )
        .unwrap();
        let mut global = GlobalFlags::test_with_cwd(tmp.path());
        global.apply = true;
        global.json = true;

        let code = run(
            PatchArgs {
                action: PatchAction::Apply {
                    file: Some(diff_path.to_string_lossy().into_owned()),
                    stdin: false,
                    on_stale: OnStaleCli::Fail,
                    replace_all: false,
                },
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("line TWO"), "patch should be applied");
    }
}
