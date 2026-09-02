//! Plan validation/preparation and direct in-process plan execution.
//!
//! size-waiver: plan validate, execute, and format_failed peel stay in one lifecycle module (policy #1408).

use super::commit::commit_changes;
#[cfg(not(any(feature = "cli", feature = "files")))]
use super::steps::rollback_strict;
use super::steps::{resolve_plan_cwd, run_lifecycle};
#[cfg(any(feature = "cli", feature = "files"))]
use super::steps::{revert_strict_lifecycle, snapshot_non_tx_files, tx_paths_for_collateral};
use crate::cli::global::GlobalFlags;
use crate::plan::{self, Plan};
use crate::tx::execute::execute_and_collect;
use crate::tx::output::{TxOutput, build_applied_with_error_output, build_full_tx_output};
use crate::tx::validate::validate_plan_operations;
#[cfg(feature = "ast")]
use crate::tx::verify;
use crate::tx::{build_error_output, build_error_output_with_suggested_op};
#[cfg(any(test, feature = "cli", feature = "files"))]
use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn config_tx_strict(cwd: &Path) -> Option<bool> {
    crate::config::find_and_load(cwd)
        .map(|(config, _)| config.tx.strict)
        .unwrap_or(None)
}

/// Execute a parsed [`Plan`] directly and return the structured `TxOutput` (PlanReport).
/// Does **not** write to stdout or stderr.
///
/// This is the in-process equivalent used by the library API (`api::execute_plan`)
/// and (via serialization) by the CLI tx command and MCP.
/// Library users get a typed `PlanReport` directly (addresses #811).
///
/// Errors are boxed so the `Result` stays under clippy's large-err threshold
/// (TxOutput is intentionally rich for structured CLI/MCP reporting).
pub(crate) fn validate_and_prepare_plan(
    plan: &Plan,
    cwd: &Path,
    no_strict: bool,
    output: Option<&GlobalFlags>,
) -> Result<(PathBuf, bool, GlobalFlags), Box<TxOutput>> {
    if plan.version != crate::plan::SCHEMA_VERSION {
        let msg = format!(
            "unsupported plan version '{}' (this build supports version {})",
            plan.version,
            crate::plan::SCHEMA_VERSION
        );
        crate::verbose!("tx: plan validation failed: {msg}");
        return Err(Box::new(build_error_output("parse_error", &msg, None)));
    }
    if let Err(e) = validate_plan_operations(plan) {
        crate::verbose!("tx: plan operation validation failed: {e}");
        // Flag/option conflicts match CLI invalid_input (exit 1), not plan
        // parse_error (exit 4). Version mismatches stay parse_error above.
        // Prefer shared classifier; default remaining validation failures to
        // parse_error (structural plan issues).
        let kind = crate::exit::classify_typed_error(&e)
            .map(|(k, _)| k)
            .unwrap_or("parse_error");
        return Err(Box::new(build_error_output(kind, &e.to_string(), None)));
    }

    let effective_cwd = resolve_plan_cwd(cwd, plan.cwd.as_deref());
    let config_strict = config_tx_strict(&effective_cwd);
    let strict = plan::effective_strict(plan.strict, config_strict, no_strict);

    let mut global = GlobalFlags::with_cwd(&effective_cwd);
    if let Some(src) = output {
        global.json = src.json;
        global.jsonl = src.jsonl;
        global.quiet = src.quiet;
    }
    if let Some((config, _)) = crate::config::find_and_load(&effective_cwd) {
        crate::config::apply_config(&mut global, &config);
    }

    Ok((effective_cwd, strict, global))
}

pub fn execute_plan_direct(
    plan: Plan,
    cwd: &Path,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<TxOutput> {
    // Expand for_each (glob-driven batch) before PathGuard / declared_paths.
    // Requires `files` (cli enables files). Library hosts must not silently
    // skip expansion (#2169).
    let mut plan = plan;
    #[cfg(feature = "files")]
    if plan.for_each.is_some() {
        crate::plan::expand_for_each(&mut plan, cwd)?;
    }

    crate::verbose!(
        "tx: direct plan execution ({} ops, cwd={}, guard={})",
        plan.operations.len(),
        cwd.display(),
        guard.is_some()
    );

    // Structured in-process API: never print human config warnings.
    let output_flags = GlobalFlags::with_cwd_and_json(cwd);
    let (effective_cwd, strict, global) =
        match validate_and_prepare_plan(&plan, cwd, false, Some(&output_flags)) {
            Ok(v) => v,
            Err(output) => return Ok(*output),
        };

    for op in &plan.operations {
        crate::backup::refuse_declared_paths_under_backup_dir(&effective_cwd, op)?;
    }

    // PathGuard enforcement for library callers of execute_plan (addresses #755).
    if let Some(g) = guard {
        // Defense-in-depth: reject plans whose cwd would escape the guard's
        // workspace root (MCP and library callers both honor plan.cwd when
        // contained; escapes must fail closed).
        if plan.cwd.is_some() {
            // Must use the same canonicalize as PathGuard (dunce) so Windows
            // UNC prefixes do not break starts_with against canon_root (#1931).
            let canon_cwd = crate::containment::safe_canonicalize(&effective_cwd)
                .unwrap_or_else(|_| effective_cwd.clone());
            if !canon_cwd.starts_with(g.canon_root()) {
                return Err(crate::fallback::EditError::guard_rejected(format!(
                    "plan cwd '{}' escapes workspace root '{}'",
                    effective_cwd.display(),
                    g.root().display()
                )));
            }
        }
        crate::plan::refuse_lifecycle_if_guarded(&plan, Some(g))?;
        // Upfront check on declared paths; PatchApply renames use entry mode
        // (no-follow last component) like FileRename (#2115 / MPI 2026-08-02).
        for op in &plan.operations {
            super::super::execute::enforce_guard_for_op(g, op)?;
        }
    }

    // Pre-execution verification snapshot.
    #[cfg(feature = "ast")]
    let verify_before = if let Some(ref checks) = plan.verify {
        if !checks.is_empty() {
            let affected = verify::scan_paths_for_checks(&plan, &effective_cwd, checks);
            checks
                .iter()
                .map(|check| {
                    let snap = verify::snapshot_symbols(&affected, check);
                    (check.clone(), snap)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Execute operations and collect changes in memory.
    let engine_ctx = crate::tx::context::EngineContext::from_global(&global, effective_cwd.clone());
    let mut result = match execute_and_collect(&plan, &engine_ctx, true, true, guard) {
        Ok(r) => r,
        Err(e) => {
            // Shared typed-kind table (no_matches, ambiguous, …); unknown →
            // operation_failed (tx default, exit 9). Preserve suggested_op (#2133).
            let suggested = crate::exit::suggested_op_from_error(&e);
            if let Some((kind, _)) = crate::exit::classify_typed_error(&e) {
                return Ok(build_error_output_with_suggested_op(
                    kind,
                    &e.to_string(),
                    None,
                    suggested,
                ));
            }
            return Ok(build_error_output("operation_failed", &e.to_string(), None));
        }
    };

    if result.replace_no_matches {
        // Prefer structured no_matches report so replace_hint / error_kind
        // match CLI --json tx (build_full_tx_output), not an empty error body.
        return Ok(build_full_tx_output(
            "no_matches",
            &mut result,
            &effective_cwd,
        ));
    }

    if result.no_effective_changes {
        let output = build_full_tx_output("success", &mut result, &effective_cwd);
        return Ok(output);
    }

    // Post-execution verification against pending content.
    #[cfg(feature = "ast")]
    if !verify_before.is_empty() {
        let checks: Vec<_> = verify_before.iter().map(|(c, _)| c.clone()).collect();
        let affected = verify::scan_paths_for_checks(&plan, &effective_cwd, &checks);
        let mut messages = Vec::new();
        let mut any_failed = false;
        for (check, before_snap) in &verify_before {
            let after_snap =
                verify::snapshot_symbols_from_pending(&affected, &result.pending, check);
            let vr = verify::compare_snapshots(before_snap, &after_snap, check, &effective_cwd);
            messages.push(vr.message.clone());
            if !vr.passed {
                any_failed = true;
            }
        }
        if any_failed {
            let summary = messages.join("\n");
            let msg = format!("verification failed, changes not applied:\n{summary}");
            return Ok(build_error_output("verification_failed", &msg, None));
        }
    }

    // Apply: back up originals, write files.
    let apply_backup_session = match commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
        &result.renames,
    ) {
        Ok(session) => session,
        Err(err) => {
            let error_kind = if err.rollback_ok {
                "rollback"
            } else {
                "rollback_failed"
            };
            let output =
                build_error_output(error_kind, &err.message, err.backup_session.as_deref());
            return Ok(output);
        }
    };

    // Snapshot non-tx files before format/validate steps so we can restore
    // collateral changes on strict rollback (#1111.7).
    // Include deletions and renames, not only `changes`: empty-file deletes
    // used to omit from `changes` (original == final == "") and must still
    // be treated as tx paths so collateral walk does not re-touch them.
    #[cfg(any(feature = "cli", feature = "files"))]
    let collateral_snapshot = if strict && plan.has_lifecycle_steps() {
        let tx_paths = tx_paths_for_collateral(&result.changes, &result.deletions, &result.renames);
        snapshot_non_tx_files(&effective_cwd, &tx_paths)
    } else {
        HashMap::new()
    };

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd, true) {
        if strict {
            #[cfg(any(feature = "cli", feature = "files"))]
            {
                match revert_strict_lifecycle(
                    &effective_cwd,
                    &result.changes,
                    &result.pending,
                    &result.deletions,
                    &result.existed_before,
                    apply_backup_session.as_deref(),
                    &collateral_snapshot,
                ) {
                    Ok(()) => {
                        let msg = format!("strict mode -- all changes reverted ({})", err.message);
                        // Same kind as CLI tx (`err.kind`): format_failed /
                        // validation_failed. Keep rollback_failed only when
                        // revert itself fails.
                        return Ok(build_error_output(
                            err.kind,
                            &msg,
                            apply_backup_session.as_deref(),
                        ));
                    }
                    Err(detail) => {
                        let msg = format!(
                            "strict mode -- could not fully revert changes ({detail}; {})",
                            err.message
                        );
                        return Ok(build_error_output(
                            "rollback_failed",
                            &msg,
                            apply_backup_session.as_deref(),
                        ));
                    }
                }
            }
            #[cfg(not(any(feature = "cli", feature = "files")))]
            {
                // No collateral snapshot without the files walker. Prefer
                // backup restore; do not run string rollback after a failed
                // restore_session (partial restore must not be overwritten).
                let mut rollback_ok = true;
                if let Some(ts) = apply_backup_session.as_ref() {
                    if crate::backup::restore_session(&effective_cwd, ts).is_err() {
                        rollback_ok = false;
                    }
                } else {
                    rollback_strict(
                        &result.changes,
                        &result.pending,
                        &result.deletions,
                        &result.existed_before,
                        true,
                    );
                }
                if rollback_ok {
                    let msg = format!("strict mode -- all changes reverted ({})", err.message);
                    return Ok(build_error_output(
                        err.kind,
                        &msg,
                        apply_backup_session.as_deref(),
                    ));
                }
                let msg = format!(
                    "strict mode -- could not fully revert changes ({})",
                    err.message
                );
                return Ok(build_error_output(
                    "rollback_failed",
                    &msg,
                    apply_backup_session.as_deref(),
                ));
            }
        }
        // Non-strict: writes already committed. Report applied changes so
        // agents do not see files_changed=0 while the working tree changed.
        return Ok(build_applied_with_error_output(
            err.kind,
            &err.message,
            &mut result,
            &effective_cwd,
            apply_backup_session.as_deref(),
        ));
    }

    let mut output = build_full_tx_output("success", &mut result, &effective_cwd);
    if output.backup_session.is_none() {
        output.backup_session = apply_backup_session;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::super::commit::{
        CommitError, FORCE_RESTORE_FAIL, RestoreFailGuard, commit_changes, commit_error,
    };
    use super::super::steps::{
        COLLATERAL_SNAPSHOT_MAX_SIZE, LifecycleError, lifecycle_failure_msg, resolve_plan_cwd,
        rollback_strict, run_lifecycle, tx_paths_for_collateral,
    };
    #[cfg(any(feature = "cli", feature = "files"))]
    use super::super::steps::{
        restore_collateral_files, revert_strict_lifecycle, snapshot_non_tx_files,
    };
    use super::*;

    // ---- lifecycle_failure_msg ----

    #[test]
    fn lifecycle_failure_msg_no_stderr() {
        assert_eq!(lifecycle_failure_msg("step failed", ""), "step failed");
    }

    #[test]
    fn lifecycle_failure_msg_whitespace_only_stderr() {
        assert_eq!(
            lifecycle_failure_msg("step failed", "  \n  "),
            "step failed"
        );
    }

    #[test]
    fn lifecycle_failure_msg_with_stderr() {
        let msg = lifecycle_failure_msg("step failed", "  error: bad input\n");
        assert_eq!(msg, "step failed: error: bad input");
    }

    // ---- resolve_plan_cwd ----

    #[test]
    fn resolve_plan_cwd_none() {
        let base = Path::new("/base/dir");
        assert_eq!(resolve_plan_cwd(base, None), PathBuf::from("/base/dir"));
    }

    #[test]
    fn resolve_plan_cwd_relative() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_plan_cwd(base, Some("sub/path")),
            PathBuf::from("/base/dir/sub/path")
        );
    }

    #[test]
    fn resolve_plan_cwd_absolute() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_plan_cwd(base, Some("/other/dir")),
            PathBuf::from("/other/dir")
        );
    }

    // ---- validate_and_prepare_plan ----

    fn minimal_plan(version: u32) -> crate::plan::Plan {
        crate::plan::Plan {
            version,
            cwd: None,
            operations: vec![crate::plan::Operation::Read {
                path: "test.txt".into(),
                lines: None,
            }],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        }
    }

    #[test]
    fn validate_and_prepare_plan_rejects_unsupported_version() {
        let dir = tempfile::TempDir::new().unwrap();
        let plan = minimal_plan(999);
        let err = validate_and_prepare_plan(&plan, dir.path(), false, None).unwrap_err();
        assert_eq!(err.error_kind.as_deref(), Some("parse_error"));
        let msg = err.error.as_deref().unwrap_or("");
        assert!(
            msg.contains("unsupported plan version") && msg.contains("999"),
            "expected version error, got: {msg}"
        );
    }

    #[test]
    fn validate_and_prepare_plan_accepts_current_version() {
        let dir = tempfile::TempDir::new().unwrap();
        let plan = minimal_plan(crate::plan::SCHEMA_VERSION);
        let (cwd, _strict, _global) = validate_and_prepare_plan(&plan, dir.path(), false, None)
            .expect("valid plan must prepare");
        assert_eq!(cwd, dir.path());
    }

    #[test]
    fn validate_and_prepare_plan_threads_json_from_caller() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[defaults]\napply = true\n",
        )
        .unwrap();
        let plan = minimal_plan(crate::plan::SCHEMA_VERSION);
        let flags = GlobalFlags::with_cwd_and_json(dir.path());
        let _ = crate::config::take_config_warnings();
        let (_, _, global) = validate_and_prepare_plan(&plan, dir.path(), false, Some(&flags))
            .expect("valid plan must prepare");
        assert!(global.json, "caller --json must survive plan prepare");
        assert!(
            !global.apply,
            "repo [defaults] apply must stay ignored after prepare"
        );
        let warnings = crate::config::take_config_warnings();
        assert!(
            warnings
                .iter()
                .all(|w| !w.contains("apply = true is ignored")),
            "json plan prepare must not emit apply-ignore warning: {warnings:?}"
        );
    }

    #[test]
    fn execute_plan_direct_does_not_print_defaults_apply_warning() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[defaults]\napply = true\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let plan = minimal_plan(crate::plan::SCHEMA_VERSION);
        let _ = crate::config::take_config_warnings();
        execute_plan_direct(plan, dir.path(), None).expect("plan ok");
        let warnings = crate::config::take_config_warnings();
        assert!(
            warnings
                .iter()
                .all(|w| !w.contains("apply = true is ignored")),
            "execute_plan_direct must not print apply-ignore warning: {warnings:?}"
        );
    }

    #[test]
    fn validate_and_prepare_plan_rejects_invalid_operation() {
        let dir = tempfile::TempDir::new().unwrap();
        // whole_line + multiline is rejected by validate_operation.
        let plan = crate::plan::Plan {
            version: crate::plan::SCHEMA_VERSION,
            cwd: None,
            operations: vec![crate::plan::Operation::Replace {
                glob: None,
                path: Some("f.txt".into()),
                regex: false,
                old: "a".into(),
                new_text: Some("b".into()),
                nth: None,
                insert_before: None,
                insert_after: None,
                case_insensitive: false,
                multiline: true,
                if_exists: false,
                whole_line: true,
                range: None,
                word_boundary: false,
                before_context: None,
                after_context: None,
                unique: false,
                require_change: false,
                command_position: false,
                fuzzy: false,
                min_fuzzy_score: None,
                allow_absent_old: false,
            }],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };
        let err = validate_and_prepare_plan(&plan, dir.path(), false, None).unwrap_err();
        assert_eq!(err.error_kind.as_deref(), Some("invalid_input"));
        let msg = err.error.as_deref().unwrap_or("");
        assert!(
            msg.contains("whole_line") && msg.contains("multiline"),
            "expected mutual-exclusion error, got: {msg}"
        );
    }

    #[test]
    fn execute_plan_direct_rejects_escaped_cwd_with_guard() {
        // Regression: a plan with cwd pointing outside the workspace
        // must be rejected when a PathGuard is present.
        use crate::containment::{AbsolutePathPolicy, PathGuard};

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "content").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();

        let plan = crate::plan::Plan {
            version: 1,
            cwd: Some("/tmp".into()),
            operations: vec![crate::plan::Operation::Read {
                path: "test.txt".into(),
                lines: None,
            }],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };

        let result = execute_plan_direct(plan, dir.path(), Some(&guard));
        assert!(
            result.is_err(),
            "should reject plan.cwd that escapes workspace"
        );
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes workspace root"),
            "error should mention escape: {msg}"
        );
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::GuardRejected),
            "plan cwd escape must peel as GuardRejected: {err}"
        );
    }

    // ---- CommitError ----

    #[test]
    fn commit_error_display() {
        let err = CommitError {
            message: "write failed".into(),
            rollback_ok: true,
            backup_session: None,
        };
        assert_eq!(format!("{err}"), "write failed");
    }

    #[test]
    fn commit_error_helper() {
        let err = commit_error("test message");
        assert_eq!(err.message, "test message");
        assert!(err.rollback_ok);
        assert!(err.backup_session.is_none());
    }

    // ---- RestoreFailGuard ----

    #[test]
    fn restore_fail_guard_toggles_flag() {
        // Before engaging, restore should succeed (flag is false).
        assert!(!FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));

        {
            let _guard = RestoreFailGuard::engage();
            assert!(FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));
        }

        // After guard is dropped, flag is reset.
        assert!(!FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));
    }

    // ---- LifecycleError ----

    /// Regression: rollback_strict must create parent directories before
    /// restoring deleted files via atomic_write. If the parent dir was
    /// removed (e.g., the delete was the last file in it), the restore fails.
    #[test]
    fn rollback_strict_creates_parent_dir_for_deleted_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("file.txt");
        std::fs::write(&file, "original").unwrap();

        // Simulate: file was deleted and its parent dir removed.
        let file_pb = file.clone();
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&sub).unwrap();
        assert!(!sub.exists());

        // Build the state as if the tx engine tracked this deletion.
        let mut pending = HashMap::new();
        pending.insert(file_pb.clone(), ("original".to_string(), String::new()));
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let mut existed_before = HashSet::new();
        existed_before.insert(file_pb.clone());

        // rollback_strict should recreate the parent dir and restore the file.
        rollback_strict(&[], &pending, &deletions, &existed_before, true);
        assert!(
            file.exists(),
            "rollback should restore file even when parent dir was removed"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    /// Regression (#1063): a file created and then deleted in the same tx
    /// must not exist after rollback (it did not exist before the tx).
    #[test]
    fn rollback_strict_create_then_delete_leaves_no_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("ephemeral.txt");

        // Simulate: the tx engine created this file and then deleted it.
        // The changes list contains an entry with empty original (did not
        // exist before) and the deletions set also contains it.
        let file_pb = file.clone();
        let changes = vec![(file_pb.clone(), String::new(), "hello".to_string())];
        let pending = HashMap::new();
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let existed_before = HashSet::new(); // did NOT exist before tx

        // Create the file on disk to simulate mid-tx state.
        std::fs::write(&file, "hello").unwrap();

        rollback_strict(&changes, &pending, &deletions, &existed_before, true);

        assert!(
            !file.exists(),
            "create-then-delete file must not exist after rollback"
        );
    }

    /// Ensure rollback_strict still restores modified-then-deleted files
    /// (files that existed before the tx, were modified, then deleted).
    #[test]
    fn rollback_strict_modify_then_delete_restores_original() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "original").unwrap();

        let file_pb = file.clone();
        // File was modified (shows up in changes) and also deleted.
        let changes = vec![(
            file_pb.clone(),
            "original".to_string(),
            "modified".to_string(),
        )];
        let mut pending = HashMap::new();
        pending.insert(
            file_pb.clone(),
            ("original".to_string(), "modified".to_string()),
        );
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let mut existed_before = HashSet::new();
        existed_before.insert(file_pb.clone());

        // Simulate mid-tx state: file was deleted.
        std::fs::remove_file(&file).unwrap();

        rollback_strict(&changes, &pending, &deletions, &existed_before, true);

        // The deletions loop should restore the original.
        assert!(file.exists(), "deleted file should be restored");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn lifecycle_error_fields() {
        let err = LifecycleError {
            message: "validation step 1 failed".into(),
            kind: "validation_failed",
        };
        assert_eq!(err.message, "validation step 1 failed");
        assert_eq!(err.kind, "validation_failed");
    }

    // ---- run_lifecycle ----

    #[test]
    fn run_lifecycle_no_steps() {
        let plan = Plan {
            version: crate::plan::SCHEMA_VERSION,
            operations: Vec::new(),
            format: None,
            validate: None,
            verify: None,
            cwd: None,
            strict: None,
            write_policy: None,
            for_each: None,
        };
        let cwd = Path::new("/tmp");
        assert!(run_lifecycle(&plan, cwd, cwd, true).is_none());
    }

    // ---- snapshot / restore collateral (#1111.7) ----

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn snapshot_non_tx_files_captures_text_skips_binary_and_tx() {
        let dir = tempfile::TempDir::new().unwrap();

        // A text file not in the tx (should be captured).
        let collateral = dir.path().join("other.rs");
        std::fs::write(&collateral, "fn main() {}").unwrap();

        // A binary file (should be skipped).
        let binary = dir.path().join("image.png");
        std::fs::write(&binary, b"\x89PNG\x00\x00").unwrap();

        // A file that IS in the tx (should be skipped).
        let tx_file = dir.path().join("changed.rs");
        std::fs::write(&tx_file, "// changed").unwrap();

        let mut tx_paths = HashSet::new();
        tx_paths.insert(tx_file.clone());

        let snapshot = snapshot_non_tx_files(dir.path(), &tx_paths);

        assert!(
            snapshot.contains_key(&collateral),
            "collateral text file should be in snapshot"
        );
        assert_eq!(snapshot[&collateral], "fn main() {}");
        assert!(
            !snapshot.contains_key(&binary),
            "binary file should not be in snapshot"
        );
        assert!(
            !snapshot.contains_key(&tx_file),
            "tx file should not be in snapshot"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn snapshot_non_tx_files_prunes_git_and_patchloom_dirs() {
        let dir = tempfile::TempDir::new().unwrap();

        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let rust_file = src.join("a.rs");
        std::fs::write(&rust_file, "fn a() {}").unwrap();

        let git_pack = dir.path().join(".git").join("objects").join("pack");
        std::fs::create_dir_all(&git_pack).unwrap();
        let git_obj = git_pack.join("x");
        std::fs::write(&git_obj, "packdata").unwrap();
        let git_file = dir.path().join(".git").join("foo.txt");
        std::fs::write(&git_file, "git metadata").unwrap();

        let backups = dir.path().join(".patchloom").join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let backup = backups.join("x");
        std::fs::write(&backup, "backup session").unwrap();

        let snapshot = snapshot_non_tx_files(dir.path(), &HashSet::new());

        assert!(
            snapshot.contains_key(&rust_file),
            "real source file should be in snapshot"
        );
        assert_eq!(snapshot[&rust_file], "fn a() {}");
        assert!(
            snapshot
                .keys()
                .all(|p| !p.components().any(|c| c.as_os_str() == ".git")),
            "snapshot must not include paths under .git: {:?}",
            snapshot.keys().collect::<Vec<_>>()
        );
        assert!(
            snapshot
                .keys()
                .all(|p| !p.components().any(|c| c.as_os_str() == ".patchloom")),
            "snapshot must not include paths under .patchloom: {:?}",
            snapshot.keys().collect::<Vec<_>>()
        );
        assert!(
            !snapshot.contains_key(&backup),
            ".patchloom/backups/x must not appear in snapshot"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn restore_collateral_files_reverts_changed_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("bystander.rs");
        std::fs::write(&file, "original content").unwrap();

        // Build a snapshot with the original content.
        let mut snapshot = HashMap::new();
        snapshot.insert(file.clone(), "original content".to_string());

        // Simulate a formatter modifying the file.
        std::fs::write(&file, "reformatted content").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "reformatted content"
        );

        // Restore should revert the file.
        restore_collateral_files(&snapshot).expect("collateral restore");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "original content",
            "collateral file should be restored to pre-format content"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn restore_collateral_skips_unchanged_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("untouched.rs");
        std::fs::write(&file, "same content").unwrap();

        let mut snapshot = HashMap::new();
        snapshot.insert(file.clone(), "same content".to_string());

        // File was not modified by the formatter. restore should be a no-op.
        restore_collateral_files(&snapshot).expect("collateral restore");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "same content");
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn snapshot_skips_large_files() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a file just above the 1 MiB threshold.
        let big_file = dir.path().join("huge.txt");
        let big_content = "x".repeat(COLLATERAL_SNAPSHOT_MAX_SIZE as usize + 1);
        std::fs::write(&big_file, &big_content).unwrap();

        // Create a small file that should be captured.
        let small_file = dir.path().join("small.txt");
        std::fs::write(&small_file, "tiny").unwrap();

        let snapshot = snapshot_non_tx_files(dir.path(), &HashSet::new());

        assert!(
            !snapshot.contains_key(&big_file),
            "file above size cap should not be in snapshot"
        );
        assert!(
            snapshot.contains_key(&small_file),
            "small file should be in snapshot"
        );
    }

    #[test]
    fn tx_paths_for_collateral_includes_rename_dest_absent_from_changes() {
        let dest = PathBuf::from("/tmp/renamed.txt");
        let src = PathBuf::from("/tmp/old.txt");
        let deletions = HashSet::from([src.clone()]);
        let renames = vec![(src.clone(), dest.clone())];
        // dest is a rename target but not in `changes` (identical-content
        // force overwrite). It must still be a tx path.
        let paths = tx_paths_for_collateral(&[], &deletions, &renames);
        assert!(paths.contains(&dest), "rename dest must be a tx path");
        assert!(paths.contains(&src), "rename source must be a tx path");
    }

    #[cfg(unix)]
    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn restore_collateral_files_reports_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("bystander.rs");
        let sibling = dir.path().join("link.rs");
        std::fs::write(&file, "original content").unwrap();
        std::fs::hard_link(&file, &sibling).unwrap();

        let mut snapshot = HashMap::new();
        snapshot.insert(file.clone(), "original content".to_string());
        std::fs::write(&file, "reformatted content").unwrap();

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        // Root (common in Docker) can still write mode-444 files. Skip when
        // permissions do not actually block writing.
        if std::fs::OpenOptions::new().write(true).open(&file).is_ok() {
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644));
            return;
        }

        let result = restore_collateral_files(&snapshot);
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644));
        let failed = result.expect_err("readonly hardlink restore must fail");
        assert!(
            failed.iter().any(|p| p == &file),
            "failed paths should include the collateral file: {failed:?}"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn revert_strict_lifecycle_skips_rollback_strict_after_restore_fail() {
        let dir = tempfile::TempDir::new().unwrap();
        let dest = dir.path().join("new.txt");
        std::fs::write(&dest, "post-rename").unwrap();

        let changes = vec![(dest.clone(), String::new(), "post-rename".to_string())];
        let pending = HashMap::new();
        let deletions = HashSet::new();
        let existed_before = HashSet::new();
        let collateral = HashMap::new();

        let _guard = RestoreFailGuard::engage();
        let err = revert_strict_lifecycle(
            dir.path(),
            &changes,
            &pending,
            &deletions,
            &existed_before,
            Some("missing-session"),
            &collateral,
        )
        .expect_err("forced restore fail must not claim full revert");
        assert!(
            err.contains("backup restore failed"),
            "error should name the failed backup restore: {err}"
        );
        assert!(
            dest.exists(),
            "must not run rollback_strict after restore_session Err"
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "post-rename");
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn execute_plan_strict_format_fail_restore_err_is_rollback_failed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("old.txt"), "content\n").unwrap();
        let cmd = if cfg!(windows) { "exit /b 1" } else { "false" };
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "version": 1,
            "strict": true,
            "operations": [
                {"op": "file.rename", "from": "old.txt", "to": "new.txt"}
            ],
            "format": [{"cmd": cmd, "timeout": 5}]
        }))
        .unwrap();

        let _guard = RestoreFailGuard::engage();
        let report = execute_plan_direct(plan, dir.path(), None).expect("plan returns output");
        assert_eq!(report.error_kind.as_deref(), Some("rollback_failed"));
        assert!(!report.ok);
        let err = report.error.as_deref().unwrap_or("");
        assert!(
            err.contains("could not fully revert"),
            "must not claim a full revert: {err}"
        );
        assert!(
            !err.contains("all changes reverted"),
            "must not claim a full revert: {err}"
        );
        assert!(
            dir.path().join("new.txt").exists(),
            "must not run rollback_strict after restore_session Err"
        );
    }

    /// Library/MCP `execute_plan` must peel `format_failed` after a successful
    /// strict revert (CLI tx already locks this in integration tx_tests).
    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn execute_plan_strict_format_fail_revert_ok_is_format_failed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "original\n").unwrap();
        let cmd = if cfg!(windows) { "exit /b 1" } else { "false" };
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "version": 1,
            "strict": true,
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "old": "original",
                "new": "changed"
            }],
            "format": [{"cmd": cmd, "timeout": 5}]
        }))
        .unwrap();

        let report = execute_plan_direct(plan, dir.path(), None).expect("plan returns output");
        assert_eq!(
            report.error_kind.as_deref(),
            Some("format_failed"),
            "revert-ok strict lifecycle must peel format_failed, not rollback: {report:?}"
        );
        assert!(!report.ok);
        let err = report.error.as_deref().unwrap_or("");
        assert!(
            err.contains("all changes reverted"),
            "must claim a full revert: {err}"
        );
        assert!(
            err.contains("format step failed"),
            "must include format failure: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("test.txt")).unwrap(),
            "original\n",
            "strict revert must restore original content"
        );
    }

    /// batch_replace / execute_plan JSON must surface match_mode for fuzzy (#1674).
    #[test]
    fn execute_plan_fuzzy_replace_reports_match_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn process_data() {}\n").unwrap();
        std::fs::write(dir.path().join("b.rs"), "fn process_data() {}\n").unwrap();

        let plan = crate::plan::Plan {
            version: crate::plan::SCHEMA_VERSION,
            cwd: None,
            operations: vec![
                crate::plan::Operation::Replace {
                    glob: None,
                    path: Some("a.rs".into()),
                    regex: false,
                    old: "fn proccess_data() {}".into(),
                    new_text: Some("fn handle() {}".into()),
                    nth: None,
                    insert_before: None,
                    insert_after: None,
                    case_insensitive: false,
                    multiline: false,
                    if_exists: false,
                    whole_line: false,
                    range: None,
                    word_boundary: false,
                    before_context: None,
                    after_context: None,
                    unique: false,
                    require_change: true,
                    command_position: false,
                    fuzzy: true,
                    min_fuzzy_score: None,
                    allow_absent_old: true,
                },
                crate::plan::Operation::Replace {
                    glob: None,
                    path: Some("b.rs".into()),
                    regex: false,
                    old: "fn process_data() {}".into(),
                    new_text: Some("fn handle() {}".into()),
                    nth: None,
                    insert_before: None,
                    insert_after: None,
                    case_insensitive: false,
                    multiline: false,
                    if_exists: false,
                    whole_line: false,
                    range: None,
                    word_boundary: false,
                    before_context: None,
                    after_context: None,
                    unique: false,
                    require_change: true,
                    command_position: false,
                    fuzzy: false,
                    min_fuzzy_score: None,
                    allow_absent_old: false,
                },
            ],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };

        let report = execute_plan_direct(plan, dir.path(), None).expect("plan ok");
        assert!(report.ok, "{report:?}");
        assert_eq!(report.files_changed, 2);
        // Aggregate is worst-case fuzzy when any file used fuzzy.
        assert_eq!(report.match_mode.as_deref(), Some("fuzzy"));
        let a = report
            .changes
            .iter()
            .find(|c| c.path.ends_with("a.rs"))
            .expect("a.rs change");
        assert_eq!(a.match_mode.as_deref(), Some("fuzzy"));
        let b = report
            .changes
            .iter()
            .find(|c| c.path.ends_with("b.rs"))
            .expect("b.rs change");
        assert_eq!(b.match_mode.as_deref(), Some("exact"));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"match_mode\""), "{json}");
    }
}
