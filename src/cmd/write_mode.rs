//! Single owner of write **mode classification** and **exit codes**.
//!
//! Every CLI write path (engine-backed and binary/case-only callbacks) must use
//! the finalize helpers so preview/check/apply/confirm cannot diverge by path
//! (see #1345–#1348 and #1373).
//!
//! **Commands must not `match classify_write_mode` themselves.**
//!
//! - Standard phase JSON → [`finalize_execution_result`]
//! - Custom JSON/text (replace, tidy, patch, md) → [`finalize_report`]
//! - Binary/case-only rename → [`finalize_callback_write`]
//!
//! Stage with [`crate::tx::engine::stage`] or CLI `run_write` / `stage_for_write`.

use crate::cli::global::GlobalFlags;
use crate::diff::{FileDiff, render_diffs_colored, render_diffs_plain};
use crate::exit;
use crate::tx::engine::ExecutionResult;
use serde::Serialize;
use std::path::Path;

/// Commit staged changes, then run `--format`. On format failure after a
/// successful commit, attach the backup session and written paths so agent
/// JSON can undo and re-validate without re-scanning disk (#1795).
fn commit_then_format(
    result: ExecutionResult,
    global: &GlobalFlags,
    cwd: &Path,
) -> anyhow::Result<()> {
    let written: Vec<String> = result
        .exec_result
        .changes
        .iter()
        .map(|(p, _, _)| {
            crate::files::relative_display(p, cwd)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let backup = result.commit()?;
    if let Err(e) = crate::write::run_format_command(global, cwd) {
        return Err(attach_format_backup(e, backup, written));
    }
    Ok(())
}

/// Apply callback write, then run `--format` with the same backup attachment.
fn apply_then_format(
    mut apply_fn: impl FnMut() -> anyhow::Result<()>,
    global: &GlobalFlags,
    cwd: &Path,
) -> anyhow::Result<()> {
    apply_fn()?;
    // Binary/case-only rename paths do not return a session id from the
    // callback; still map format failures to FormatFailedError for JSON.
    if let Err(e) = crate::write::run_format_command(global, cwd) {
        return Err(attach_format_backup(e, None, Vec::new()));
    }
    Ok(())
}

fn attach_format_backup(
    err: anyhow::Error,
    backup: Option<String>,
    written: Vec<String>,
) -> anyhow::Error {
    // Prefer peeling an existing FormatFailedError so we keep its message.
    if exit::is_format_failed(&err) {
        let msg = err
            .chain()
            .find_map(|c| {
                c.downcast_ref::<exit::FormatFailedError>()
                    .map(|f| f.msg.clone())
            })
            .unwrap_or_else(|| err.to_string());
        let existing = exit::format_failed_backup_session(&err).map(str::to_string);
        let existing_files = exit::format_failed_written_files(&err);
        let files = if written.is_empty() {
            existing_files
        } else {
            written
        };
        return exit::FormatFailedError::new(msg)
            .with_backup_session(backup.or(existing))
            .with_written_files(files)
            .into();
    }
    // Plain errors after a commit still need the agent envelope (#1795).
    if !written.is_empty() || backup.is_some() {
        return exit::FormatFailedError::new(err.to_string())
            .with_backup_session(backup)
            .with_written_files(written)
            .into();
    }
    err
}

/// Phase indicator passed to the output constructor so each command can map
/// it to its own `applied` field semantics.
#[derive(Debug, Clone, Copy)]
pub enum WritePhase {
    /// `--check`: no write performed. The `bool` indicates whether changes
    /// were detected (true = content would change on apply).
    Check(bool),
    /// `--apply`: write was performed.
    Applied,
    /// `--confirm` + JSON: conditionally applied (bool = whether user confirmed).
    Confirmed(bool),
    /// Default dry-run preview.
    Preview,
}

impl WritePhase {
    /// Agent-facing `applied` for success JSON: `Some(true)` after `--apply`,
    /// `Some(confirmed)` after confirm+json, `None` for check/preview (omit).
    #[must_use]
    pub fn applied_flag(self) -> Option<bool> {
        match self {
            WritePhase::Applied => Some(true),
            WritePhase::Confirmed(a) => Some(a),
            WritePhase::Check(_) | WritePhase::Preview => None,
        }
    }
}

/// How a write command should behave given the global flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// `--check`: report without writing.
    Check,
    /// `--apply`: always commit when reached.
    Apply,
    /// `--confirm` combined with `--json`/`--jsonl` (prompt decides apply).
    ConfirmJson,
    /// Default dry-run preview; interactive `--confirm` may still apply.
    Preview,
}

/// Classify write mode from global flags.
///
/// Priority: check > apply > confirm+json > preview.
///
/// Do not match on this in command modules; use a finalize helper instead.
#[must_use]
pub fn classify_write_mode(global: &GlobalFlags) -> WriteMode {
    if global.check {
        WriteMode::Check
    } else if global.apply {
        WriteMode::Apply
    } else if global.confirm && (global.json || global.jsonl) {
        WriteMode::ConfirmJson
    } else {
        WriteMode::Preview
    }
}

/// Exit code for a write command after deciding whether changes exist and
/// whether they were applied.
///
/// | has_changes | applied | exit |
/// |-------------|---------|------|
/// | * | true | 0 SUCCESS |
/// | true | false | 2 CHANGES_DETECTED |
/// | false | false | 0 SUCCESS |
#[must_use]
pub fn write_exit_code(has_changes: bool, applied: bool) -> u8 {
    if applied {
        exit::SUCCESS
    } else if has_changes {
        exit::CHANGES_DETECTED
    } else {
        exit::SUCCESS
    }
}

/// Human-readable messages for default text-mode output.
pub struct WriteMessages<'a> {
    /// Message for `--check` and dry-run when no diff is shown.
    pub check: &'a str,
    /// Message for `--apply` when `--diff` is not set.
    pub apply: &'a str,
    /// Optional status line after interactive confirm-and-apply.
    pub post_confirm: Option<&'a str>,
}

/// Rendering policy for staged (engine) writes.
#[derive(Debug, Clone, Copy)]
pub struct RenderPolicy {
    /// When true, build and show unified diffs in preview / confirm+json.
    pub preview_diffs: bool,
}

impl Default for RenderPolicy {
    fn default() -> Self {
        Self {
            preview_diffs: true,
        }
    }
}

/// Emit hooks for [`finalize_report`] (grouped so the entrypoint stays under
/// clippy's argument limit without a suppression).
pub struct FinalizeCallbacks<OnCheck, OnApply, OnPreview, AfterEmit, AfterApply> {
    pub on_check: OnCheck,
    pub on_apply: OnApply,
    pub on_preview: OnPreview,
    pub after_preview_emit: AfterEmit,
    pub after_preview_apply: AfterApply,
}

/// Canonical mode → commit → format → exit for custom-rendered staged writes.
///
/// Callers only supply **emit** behavior. Order for preview/confirm-json:
/// `on_preview` → optional status → `should_apply` → optional commit → optional
/// post-apply status. That matches replace/tidy/patch/md custom output.
///
/// `on_check` receives built diffs (for commands that list changed files in
/// check mode). Prefer [`finalize_execution_result`] for standard
/// `make_output(WritePhase, diff)` schemas (ConfirmJson emits *after* apply).
pub fn finalize_report<OnCheck, OnApply, OnPreview, AfterEmit, AfterApply>(
    global: &GlobalFlags,
    cwd: &Path,
    result: ExecutionResult,
    preview_diffs: bool,
    mut cb: FinalizeCallbacks<OnCheck, OnApply, OnPreview, AfterEmit, AfterApply>,
) -> anyhow::Result<u8>
where
    OnCheck: FnMut(&GlobalFlags, bool, &[FileDiff]) -> anyhow::Result<()>,
    OnApply: FnMut(&GlobalFlags, bool, &[FileDiff], Option<String>) -> anyhow::Result<()>,
    OnPreview: FnMut(&GlobalFlags, bool, &[FileDiff], Option<String>) -> anyhow::Result<()>,
    AfterEmit: FnMut(&GlobalFlags),
    AfterApply: FnMut(&GlobalFlags),
{
    let has_changes = result.has_changes;
    match classify_write_mode(global) {
        WriteMode::Check => {
            let diffs = result.build_diffs();
            (cb.on_check)(global, has_changes, &diffs)?;
            Ok(write_exit_code(has_changes, false))
        }
        WriteMode::Apply => {
            let diffs = result.build_diffs();
            commit_then_format(result, global, cwd)?;
            let diff_text = if global.diff {
                Some(render_diffs_plain(&diffs))
            } else {
                None
            };
            (cb.on_apply)(global, has_changes, &diffs, diff_text)?;
            Ok(write_exit_code(has_changes, true))
        }
        WriteMode::ConfirmJson | WriteMode::Preview => {
            let diffs = if preview_diffs {
                result.build_diffs()
            } else {
                Vec::new()
            };
            let diff_text = plain_diff_opt(&diffs);
            (cb.on_preview)(global, has_changes, &diffs, diff_text)?;
            (cb.after_preview_emit)(global);
            let applied = global.should_apply();
            if applied {
                commit_then_format(result, global, cwd)?;
                (cb.after_preview_apply)(global);
            }
            Ok(write_exit_code(has_changes, applied))
        }
    }
}

/// Finalize a staged [`ExecutionResult`] under global write flags with a
/// standard phase-based JSON schema (`make_output(phase, diff)`).
pub fn finalize_execution_result<T: Serialize>(
    global: &GlobalFlags,
    cwd: &Path,
    result: ExecutionResult,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    msgs: WriteMessages<'_>,
    render: RenderPolicy,
) -> anyhow::Result<u8> {
    let has_changes = result.has_changes;
    match classify_write_mode(global) {
        WriteMode::Check => {
            let output = make_output(WritePhase::Check(has_changes), None);
            if !global.emit_json(&output)? && !global.quiet && has_changes {
                println!("{}", msgs.check);
            }
            Ok(write_exit_code(has_changes, false))
        }
        WriteMode::Apply => {
            let diffs = result.build_diffs();
            commit_then_format(result, global, cwd)?;
            let diff_text = if global.diff {
                Some(render_diffs_plain(&diffs))
            } else {
                None
            };
            let output = make_output(WritePhase::Applied, diff_text);
            if !global.emit_json(&output)? && has_changes {
                if global.diff {
                    print!("{}", render_diffs_colored(&diffs, global.should_color()));
                } else if !global.quiet {
                    println!("{}", msgs.apply);
                }
            }
            Ok(write_exit_code(has_changes, true))
        }
        WriteMode::ConfirmJson => {
            let diffs = if render.preview_diffs {
                result.build_diffs()
            } else {
                Vec::new()
            };
            let diff_text = plain_diff_opt(&diffs);
            let applied = global.should_apply();
            if applied {
                commit_then_format(result, global, cwd)?;
            }
            let output = make_output(WritePhase::Confirmed(applied), diff_text);
            global.emit_json(&output)?;
            Ok(write_exit_code(has_changes, applied))
        }
        WriteMode::Preview => {
            let diffs = if render.preview_diffs {
                result.build_diffs()
            } else {
                Vec::new()
            };
            let diff_text = plain_diff_opt(&diffs);
            let output = make_output(WritePhase::Preview, diff_text);
            if !global.emit_json(&output)? {
                if !diffs.is_empty() {
                    print!("{}", render_diffs_colored(&diffs, global.should_color()));
                } else if has_changes && !global.quiet {
                    println!("{}", msgs.check);
                }
            }
            let applied = global.should_apply();
            if applied {
                commit_then_format(result, global, cwd)?;
                if let Some(msg) = msgs.post_confirm
                    && global.show_status()
                {
                    eprintln!("{msg}");
                }
            }
            Ok(write_exit_code(has_changes, applied))
        }
    }
}

/// Finalize a callback-based write (binary/case-only rename) under the same
/// mode/exit contract as engine-backed writes.
///
/// `has_changes` is typically `true` when the command has already decided the
/// operation would change something (there is no separate "no-op" probe).
pub fn finalize_callback_write<T: Serialize>(
    global: &GlobalFlags,
    cwd: &Path,
    has_changes: bool,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    diff_fn: Option<&dyn Fn(bool) -> String>,
    mut apply_fn: impl FnMut() -> anyhow::Result<()>,
    msgs: WriteMessages<'_>,
) -> anyhow::Result<u8> {
    match classify_write_mode(global) {
        WriteMode::Check => {
            let output = make_output(WritePhase::Check(has_changes), None);
            if !global.emit_json(&output)? && !global.quiet && has_changes {
                println!("{}", msgs.check);
            }
            Ok(write_exit_code(has_changes, false))
        }
        WriteMode::Apply => {
            apply_then_format(&mut apply_fn, global, cwd)?;
            let diff_text = if global.diff {
                diff_fn.map(|f| f(false))
            } else {
                None
            };
            let output = make_output(WritePhase::Applied, diff_text);
            if !global.emit_json(&output)? {
                if global.diff {
                    if let Some(f) = diff_fn {
                        print!("{}", f(global.should_color()));
                    }
                } else if !global.quiet && has_changes {
                    println!("{}", msgs.apply);
                }
            }
            Ok(write_exit_code(has_changes, true))
        }
        WriteMode::ConfirmJson => {
            let diff_text = diff_fn.map(|f| f(false));
            let applied = global.should_apply();
            if applied {
                apply_then_format(&mut apply_fn, global, cwd)?;
            }
            let output = make_output(WritePhase::Confirmed(applied), diff_text);
            global.emit_json(&output)?;
            Ok(write_exit_code(has_changes, applied))
        }
        WriteMode::Preview => {
            let diff_text = diff_fn.map(|f| f(false));
            let output = make_output(WritePhase::Preview, diff_text);
            if !global.emit_json(&output)? {
                if let Some(f) = diff_fn {
                    print!("{}", f(global.should_color()));
                } else if has_changes && !global.quiet {
                    println!("{}", msgs.check);
                }
            }
            let applied = global.should_apply();
            if applied {
                apply_then_format(&mut apply_fn, global, cwd)?;
                if let Some(msg) = msgs.post_confirm
                    && global.show_status()
                {
                    eprintln!("{msg}");
                }
            }
            Ok(write_exit_code(has_changes, applied))
        }
    }
}

fn plain_diff_opt(diffs: &[FileDiff]) -> Option<String> {
    if diffs.is_empty() {
        None
    } else {
        Some(render_diffs_plain(diffs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_flag_maps_phases() {
        assert_eq!(WritePhase::Applied.applied_flag(), Some(true));
        assert_eq!(WritePhase::Confirmed(true).applied_flag(), Some(true));
        assert_eq!(WritePhase::Confirmed(false).applied_flag(), Some(false));
        assert_eq!(WritePhase::Preview.applied_flag(), None);
        assert_eq!(WritePhase::Check(true).applied_flag(), None);
    }

    #[test]
    fn classify_priority_check_over_apply() {
        let g = GlobalFlags {
            check: true,
            apply: true,
            ..GlobalFlags::default()
        };
        assert_eq!(classify_write_mode(&g), WriteMode::Check);
    }

    #[test]
    fn classify_confirm_json() {
        let g = GlobalFlags {
            confirm: true,
            json: true,
            ..GlobalFlags::default()
        };
        assert_eq!(classify_write_mode(&g), WriteMode::ConfirmJson);
    }

    #[test]
    fn classify_default_preview() {
        assert_eq!(
            classify_write_mode(&GlobalFlags::default()),
            WriteMode::Preview
        );
    }

    #[test]
    fn exit_code_matrix() {
        assert_eq!(write_exit_code(true, true), exit::SUCCESS);
        assert_eq!(write_exit_code(false, true), exit::SUCCESS);
        assert_eq!(write_exit_code(true, false), exit::CHANGES_DETECTED);
        assert_eq!(write_exit_code(false, false), exit::SUCCESS);
    }

    #[test]
    fn finalize_report_check_does_not_commit() {
        use crate::plan::Operation;
        use crate::tx::engine::{ExecuteOptions, WriteRequest, WriteSource, stage};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;
        let options = ExecuteOptions::from_global(dir.path(), &global, None);
        let report = stage(WriteRequest {
            source: WriteSource::Operations(vec![Operation::FileCreate {
                path: "f.txt".to_string(),
                content: "x\n".to_string(),
                force: None,
            }]),
            options,
        })
        .unwrap();

        let checks = AtomicUsize::new(0);
        let code = finalize_report(
            &global,
            dir.path(),
            report,
            true,
            FinalizeCallbacks {
                on_check: |_g: &GlobalFlags, has: bool, _d: &[FileDiff]| {
                    assert!(has);
                    checks.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                on_apply: |_g: &GlobalFlags, _: bool, _: &[FileDiff], _: Option<String>| {
                    panic!("apply must not run")
                },
                on_preview: |_g: &GlobalFlags, _: bool, _: &[FileDiff], _: Option<String>| {
                    panic!("preview must not run")
                },
                after_preview_emit: |_: &GlobalFlags| {},
                after_preview_apply: |_: &GlobalFlags| {},
            },
        )
        .unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert_eq!(checks.load(Ordering::SeqCst), 1);
        assert!(!dir.path().join("f.txt").exists());
    }

    #[test]
    fn finalize_report_apply_commits_and_runs_apply_hook() {
        use crate::plan::Operation;
        use crate::tx::engine::{ExecuteOptions, WriteRequest, WriteSource, stage};
        use std::sync::atomic::{AtomicBool, Ordering};

        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let options = ExecuteOptions::from_global(dir.path(), &global, None);
        let report = stage(WriteRequest {
            source: WriteSource::Operations(vec![Operation::FileCreate {
                path: "applied.txt".to_string(),
                content: "ok\n".to_string(),
                force: None,
            }]),
            options,
        })
        .unwrap();

        let applied_hook = AtomicBool::new(false);
        let code = finalize_report(
            &global,
            dir.path(),
            report,
            true,
            FinalizeCallbacks {
                on_check: |_g: &GlobalFlags, _: bool, _: &[FileDiff]| panic!("check must not run"),
                on_apply: |_g: &GlobalFlags,
                           has: bool,
                           diffs: &[FileDiff],
                           _plain: Option<String>| {
                    assert!(has);
                    assert!(!diffs.is_empty());
                    applied_hook.store(true, Ordering::SeqCst);
                    Ok(())
                },
                on_preview: |_g: &GlobalFlags, _: bool, _: &[FileDiff], _: Option<String>| {
                    panic!("preview must not run")
                },
                after_preview_emit: |_: &GlobalFlags| {},
                after_preview_apply: |_: &GlobalFlags| {},
            },
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(applied_hook.load(Ordering::SeqCst));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("applied.txt")).unwrap(),
            "ok\n"
        );
    }

    #[test]
    fn finalize_report_preview_does_not_commit_without_apply() {
        use crate::plan::Operation;
        use crate::tx::engine::{ExecuteOptions, WriteRequest, WriteSource, stage};

        let dir = tempfile::TempDir::new().unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let options = ExecuteOptions::from_global(dir.path(), &global, None);
        let report = stage(WriteRequest {
            source: WriteSource::Operations(vec![Operation::FileCreate {
                path: "preview.txt".to_string(),
                content: "p\n".to_string(),
                force: None,
            }]),
            options,
        })
        .unwrap();

        let code = finalize_report(
            &global,
            dir.path(),
            report,
            true,
            FinalizeCallbacks {
                on_check: |_g: &GlobalFlags, _: bool, _: &[FileDiff]| panic!("check must not run"),
                on_apply: |_g: &GlobalFlags, _: bool, _: &[FileDiff], _: Option<String>| {
                    panic!("apply must not run")
                },
                on_preview: |_g: &GlobalFlags, has: bool, _d: &[FileDiff], _p: Option<String>| {
                    assert!(has);
                    Ok(())
                },
                after_preview_emit: |_: &GlobalFlags| {},
                after_preview_apply: |_: &GlobalFlags| {},
            },
        )
        .unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!dir.path().join("preview.txt").exists());
    }
}
