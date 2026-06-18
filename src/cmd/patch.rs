use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::ops::patch::{
    ApplyHunksOptions, ApplyHunksResult, ApplyHunksStatus, OnStale, apply_hunks,
    apply_hunks_with_options, parse_patch,
};
use crate::write::policy_from_flags;
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
        println!(
            "{}",
            serde_json::to_string_pretty(&PatchFilesOutput {
                ok,
                files: results.to_vec(),
            })?
        );
    } else if global.jsonl {
        for r in results {
            println!("{}", serde_json::to_string(r)?);
        }
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

    let root = global.resolve_cwd()?;
    let command_label = if merge_mode {
        "patch merge"
    } else {
        match args.action {
            PatchAction::Check { .. } => "patch check",
            PatchAction::Apply { .. } => "patch apply",
            PatchAction::Merge { .. } => "patch merge",
        }
    };

    if matches!(args.action, PatchAction::Check { .. }) {
        let mut all_clean = true;
        let mut results = Vec::new();
        for pf in &patch_files {
            let file_path = root.join(&pf.path);
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

    if merge_mode && (global.check || (!global.apply && !global.should_apply())) {
        let check_options = ApplyHunksOptions {
            on_stale: OnStale::Merge,
            allow_conflicts: true,
        };
        let mut results = Vec::new();
        let mut all_ok = true;
        for pf in &patch_files {
            let file_path = root.join(&pf.path);
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
        return Ok(if results.iter().any(|r| r.status == "conflict") {
            exit::CONFLICTS
        } else if all_ok {
            exit::SUCCESS
        } else {
            exit::AMBIGUOUS
        });
    }

    let mut diffs = Vec::new();
    let mut file_changes = Vec::new();
    let mut has_conflicts = false;

    for pf in &patch_files {
        let file_path = root.join(&pf.path);
        let original = std::fs::read_to_string(&file_path).unwrap_or_default();
        let applied = match apply_patch_file(&original, &pf.hunks, apply_options) {
            Ok(a) => a,
            Err(msg) => {
                let label = if apply_options.on_stale == OnStale::Merge {
                    "MERGE FAILED"
                } else {
                    "STALE"
                };
                let err = format!("{command_label}: {} -- {label}: {msg}", pf.path);
                emit_error(global, &err)?;
                return Ok(if msg.contains("conflict") {
                    exit::CONFLICTS
                } else {
                    exit::AMBIGUOUS
                });
            }
        };
        if applied.status == ApplyHunksStatus::Conflict {
            has_conflicts = true;
        }
        diffs.push(unified_diff(&pf.path, &original, &applied.content));
        file_changes.push((file_path, applied.content));
    }

    if has_conflicts && !apply_options.allow_conflicts {
        return Ok(exit::CONFLICTS);
    }

    if global.check {
        let changed = diffs.iter().filter(|d| d.has_changes).count();
        if changed > 0 {
            if !global.quiet {
                println!("{changed} file(s) would change");
            }
            return Ok(exit::CHANGES_DETECTED);
        }
        return Ok(exit::SUCCESS);
    }

    if global.apply || global.should_apply() {
        let policies: Vec<_> = file_changes
            .iter()
            .map(|(p, _)| policy_from_flags(global, Some(p.as_path())))
            .collect();
        let writes: Vec<_> = file_changes
            .iter()
            .zip(&policies)
            .map(|((p, c), pol)| (p.as_path(), c.as_str(), pol))
            .collect();
        crate::backup::backup_write_files(&root, &writes)?;
        return Ok(exit::SUCCESS);
    }

    let changed = diffs.iter().filter(|d| d.has_changes).count();
    print!(
        "{}",
        format_diff_result_colored(
            &DiffResult {
                diffs,
                total_files_changed: changed,
            },
            global.should_color()
        )
    );
    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    fn flags_for(dir: &std::path::Path) -> GlobalFlags {
        GlobalFlags {
            cwd: Some(dir.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        }
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
        let mut global = flags_for(tmp.path());
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
}
