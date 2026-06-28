use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored};
use crate::exit;
use crate::ops::patch::{
    ApplyHunksOptions, ApplyHunksResult, ApplyHunksStatus, OnStale, apply_hunks,
    apply_hunks_with_options, parse_patch,
};
use crate::plan::Operation;
use crate::tx::engine::{ExecuteOptions, execute_single};
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
#[non_exhaustive]
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

fn read_diff_input(file: &Option<String>, stdin_flag: bool) -> Result<String, DiffReadError> {
    if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|e| DiffReadError::IoError(path.clone(), e))
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

fn emit_error(global: &GlobalFlags, error: &str) -> anyhow::Result<()> {
    if global.emit_json(&serde_json::json!({"ok": false, "error": error}))? {
        return Ok(());
    }
    eprintln!("{error}");
    Ok(())
}

fn emit_patch_files_output(
    global: &GlobalFlags,
    ok: bool,
    results: &[PatchFileResult],
) -> anyhow::Result<()> {
    if global.json {
        let output = PatchFilesOutput {
            ok,
            files: results.to_vec(),
        };
        global.emit_json(&output)?;
    } else if global.jsonl {
        global.emit_json_items(results)?;
    }
    Ok(())
}

pub fn run(args: PatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
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

    let diff_text = match read_diff_input(&file, stdin_flag) {
        Ok(text) => text,
        Err(DiffReadError::NoSource) => {
            emit_error(global, "patch: must specify --file <path> or --stdin")?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::IoError(path, e)) => {
            emit_error(global, &format!("patch: failed to read '{path}': {e}"))?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::StdinError(e)) => {
            emit_error(global, &format!("patch: failed to read stdin: {e}"))?;
            return Ok(exit::PARSE_ERROR);
        }
    };

    let patch_files = match parse_patch(&diff_text) {
        Ok(pf) => pf,
        Err(msg) => {
            emit_error(global, &format!("patch: parse error: {msg}"))?;
            return Ok(exit::PARSE_ERROR);
        }
    };

    let cwd = global.resolve_cwd()?;

    if matches!(args.action, PatchAction::Check { .. }) {
        let mut all_clean = true;
        let mut results = Vec::new();
        for pf in &patch_files {
            let file_path = cwd.join(&pf.path);
            let original = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
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
        emit_patch_files_output(global, all_clean, &results)?;
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
            let original = std::fs::read_to_string(&file_path).unwrap_or_default();
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
        emit_patch_files_output(global, all_ok, &results)?;
        let has_conflicts = results.iter().any(|r| r.status == "conflict");
        return Ok(if has_conflicts && !apply_options.allow_conflicts {
            exit::CONFLICTS
        } else if all_ok || (has_conflicts && apply_options.allow_conflicts) {
            exit::SUCCESS
        } else {
            exit::AMBIGUOUS
        });
    }

    // Build the PatchApply operation and route through the engine.
    let op = Operation::PatchApply {
        diff: diff_text,
        on_stale: apply_options.on_stale,
        allow_conflicts: apply_options.allow_conflicts,
    };

    let options = ExecuteOptions {
        cwd: &cwd,
        global,
        guard: None,
    };
    let result = match execute_single(op, options) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            // Map engine errors to specific exit codes with CLI-style messages.
            // The engine error from apply_patch_with_loader already includes
            // "patch apply: <path> -- <detail>", so we add the STALE/MERGE
            // FAILED label to match the original CLI format.
            let exit_code = if msg.contains("conflict") {
                exit::CONFLICTS
            } else {
                exit::AMBIGUOUS
            };
            // Inject the STALE/MERGE FAILED label between path and error detail.
            let label = if merge_mode { "MERGE FAILED" } else { "STALE" };
            let err = inject_stale_label(&msg, label);
            emit_error(global, &err)?;
            return Ok(exit_code);
        }
    };

    // --check mode: report what would happen, no mutation.
    if global.check {
        let diffs = result.build_diffs();
        let changed = diffs.iter().filter(|d| d.has_changes).count();
        if changed > 0 {
            if !global.quiet {
                println!("{changed} file(s) would change");
            }
            return Ok(exit::CHANGES_DETECTED);
        }
        return Ok(exit::SUCCESS);
    }

    // --apply mode: commit, then show output.
    if global.apply || global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, &cwd)?;
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show diff preview.
    let diffs = result.build_diffs();
    print!(
        "{}",
        format_diff_result_colored(&DiffResult { diffs }, global.should_color())
    );
    Ok(exit::SUCCESS)
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
    fn inject_stale_label_fallback_without_separator() {
        let msg = "some other error";
        let result = inject_stale_label(msg, "STALE");
        assert_eq!(result, "some other error (STALE)");
    }
}
