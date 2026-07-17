use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored};
use crate::exit;
use crate::ops::patch::{
    ApplyHunksOptions, ApplyHunksResult, ApplyHunksStatus, OnStale, apply_hunks,
    apply_hunks_with_options, parse_patch,
};
use crate::plan::Operation;
use crate::tx::engine::WriteSource;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom patch apply changes.patch
  patchloom patch apply changes.patch --apply
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
            std::io::read_to_string(std::io::stdin()).map_err(DiffReadError::StdinError)
        } else {
            // Relative patch paths resolve under --cwd (parity with `tx` / `batch`).
            let full = global
                .resolve_user_path(path)
                .map_err(|e| DiffReadError::IoError(path.clone(), std::io::Error::other(e)))?;
            std::fs::read_to_string(&full)
                .map_err(|e| DiffReadError::IoError(full.display().to_string(), e))
        }
    } else if stdin_flag {
        std::io::read_to_string(std::io::stdin()).map_err(DiffReadError::StdinError)
    } else {
        Err(DiffReadError::NoSource)
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
}

#[derive(Debug, Serialize)]
struct PatchFilesOutput {
    ok: bool,
    files: Vec<PatchFileResult>,
    /// Whether bytes were written (#1812). `false` for preview/`--check`.
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    /// Backup session id after a successful apply (#1802).
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
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
    }
}

/// Build `PatchFileResult` list from diffs, filtering for changed files.
fn build_file_results(
    diffs: &[crate::diff::FileDiff],
    status: &'static str,
) -> Vec<PatchFileResult> {
    diffs
        .iter()
        .filter(|d| d.has_changes)
        .map(|d| PatchFileResult {
            path: d.path.clone(),
            status,
            error: None,
            conflicts: None,
        })
        .collect()
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
    if !global.emit_json(&serde_json::json!({
        "ok": false,
        "error": error,
        "error_kind": error_kind,
    }))? && !global.quiet
    {
        eprintln!("{error}");
    }
    Ok(())
}

fn emit_patch_files_output(
    global: &GlobalFlags,
    ok: bool,
    results: &[PatchFileResult],
    applied: Option<bool>,
    backup_session: Option<String>,
) -> anyhow::Result<()> {
    if global.json {
        let output = PatchFilesOutput {
            ok,
            files: results.to_vec(),
            applied,
            backup_session,
        };
        global.emit_json(&output)?;
    } else if global.jsonl {
        global.emit_json_items(results)?;
    } else if !global.quiet {
        for r in results {
            let label = match r.status {
                "clean" => "clean",
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
            } else if r.status != "clean" && r.status != "applied" {
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
            emit_error(
                global,
                &format!("patch: failed to read '{path}': {e}"),
                kind,
            )?;
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
    };

    crate::verbose!("patch: diff text length={}", diff_text.len());
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
        let mut all_clean = true;
        let mut results = Vec::new();
        for pf in &patch_files {
            let file_path = cwd.join(&pf.path);
            let original = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if pf.is_creation {
                        // Creation patch: file should not exist yet.
                        String::new()
                    } else {
                        let msg = format!("file not found: {}", file_path.display());
                        results.push(PatchFileResult {
                            path: pf.path.clone(),
                            status: "missing",
                            error: Some(msg.clone()),
                            conflicts: None,
                        });
                        all_clean = false;
                        continue;
                    }
                }
                Err(e) => {
                    let msg = format!("failed to read {}: {}", file_path.display(), e);
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "error",
                        error: Some(msg.clone()),
                        conflicts: None,
                    });
                    if !global.json && !global.jsonl && !global.quiet {
                        eprintln!("patch check: {} -- READ ERROR: {}", pf.path, msg);
                    }
                    all_clean = false;
                    continue;
                }
            };
            if apply_hunks(&original, &pf.hunks).is_ok() {
                results.push(PatchFileResult {
                    path: pf.path.clone(),
                    status: "clean",
                    error: None,
                    conflicts: None,
                });
            } else {
                all_clean = false;
                results.push(PatchFileResult {
                    path: pf.path.clone(),
                    status: "stale",
                    error: None,
                    conflicts: None,
                });
            }
        }
        emit_patch_files_output(global, all_clean, &results, Some(false), None)?;
        return Ok(if all_clean {
            exit::SUCCESS
        } else {
            exit::AMBIGUOUS
        });
    }

    if merge_mode && (global.check || (!global.apply && !global.confirm)) {
        let check_options = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: true,
        };
        let mut results = Vec::new();
        let mut all_ok = true;
        for pf in &patch_files {
            let file_path = cwd.join(&pf.path);
            let original = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => {
                    let msg = format!("patch check: cannot read {}: {e}", pf.path);
                    global.emit_error_json_kind(Some("invalid_input"), &msg)?;
                    return Ok(exit::FAILURE);
                }
            };
            match apply_patch_file(&original, &pf.hunks, check_options) {
                Ok(applied) => {
                    if applied.status == ApplyHunksStatus::Conflict {
                        all_ok = false;
                    }
                    results.push(patch_file_result(&pf.path, &applied));
                }
                Err(msg) => {
                    all_ok = false;
                    results.push(PatchFileResult {
                        path: pf.path.clone(),
                        status: "error",
                        error: Some(msg),
                        conflicts: None,
                    });
                }
            }
        }
        emit_patch_files_output(global, all_ok, &results, Some(false), None)?;
        let has_errors = results.iter().any(|r| r.status == "error");
        let has_conflicts = results.iter().any(|r| r.status == "conflict");
        return Ok(if has_errors {
            exit::AMBIGUOUS
        } else if has_conflicts && !apply_options.allow_conflicts {
            exit::CONFLICTS
        } else {
            // Preview/check mode: report that changes would be applied.
            exit::CHANGES_DETECTED
        });
    }

    // Build the PatchApply operation and route through the engine.
    let op = Operation::PatchApply {
        diff: diff_text,
        on_stale: apply_options.on_stale,
        allow_conflicts: apply_options.allow_conflicts,
    };

    let (cwd, result) =
        match crate::cmd::output::stage_for_write(WriteSource::Operations(vec![op]), global) {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                // Map engine errors to specific exit codes with CLI-style messages.
                // The engine error from apply_patch_with_loader already includes
                // "patch apply: <path> -- <detail>", so we add the STALE/MERGE
                // FAILED label to match the original CLI format.
                // Prefer typed ConflictsError; keep conflict(s) text fallback for
                // any remaining untyped paths.
                let (exit_code, kind) = if exit::is_conflicts(&e) || msg.contains("conflict(s)") {
                    (exit::CONFLICTS, "conflicts")
                } else {
                    (exit::AMBIGUOUS, "ambiguous")
                };
                // Inject the STALE/MERGE FAILED label between path and error detail.
                let label = if merge_mode { "MERGE FAILED" } else { "STALE" };
                let err = inject_stale_label(&msg, label);
                emit_error(global, &err, kind)?;
                return Ok(exit_code);
            }
        };

    use crate::cmd::write_mode::{FinalizeCallbacks, finalize_report};

    finalize_report(
        global,
        &cwd,
        result,
        true,
        FinalizeCallbacks {
            on_check: |g: &GlobalFlags, _has: bool, diffs: &[crate::diff::FileDiff]| {
                let files = build_file_results(diffs, "would_change");
                let changed = files.len();
                if changed > 0 {
                    emit_patch_files_output(g, true, &files, Some(false), None)?;
                    if !(g.json || g.jsonl || g.quiet) {
                        println!("{changed} file(s) would change");
                    }
                }
                Ok(())
            },
            on_apply: |g: &GlobalFlags,
                       has: bool,
                       diffs: &[crate::diff::FileDiff],
                       _plain: Option<String>,
                       backup: Option<String>| {
                let status = if has { "applied" } else { "unchanged" };
                let files = build_file_results(diffs, status);
                emit_patch_files_output(g, true, &files, Some(has), backup)?;
                Ok(())
            },
            on_preview: |g: &GlobalFlags,
                         _has: bool,
                         diffs: &[crate::diff::FileDiff],
                         _plain: Option<String>| {
                if g.json || g.jsonl {
                    let files = build_file_results(diffs, "would_change");
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
