//! Plan validation/preparation and direct in-process plan execution.

use super::commit::commit_changes;
use super::steps::{
    resolve_plan_cwd, restore_collateral_files, rollback_strict, run_lifecycle,
    snapshot_non_tx_files,
};
use crate::cli::global::GlobalFlags;
use crate::plan::{self, Plan};
use crate::tx::execute::execute_and_collect;
use crate::tx::output::{TxOutput, build_error_output, build_full_tx_output};
use crate::tx::validate::validate_plan_operations;
use crate::tx::verify;
use std::collections::{HashMap, HashSet};
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
    // Expand for_each (glob-driven batch) before anything else.
    #[cfg(feature = "cli")]
    let mut plan = plan;
    #[cfg(feature = "cli")]
    if plan.for_each.is_some() {
        crate::plan::expand_for_each(&mut plan, cwd)?;
    }

    crate::verbose!(
        "tx: direct plan execution ({} ops, cwd={}, guard={})",
        plan.operations.len(),
        cwd.display(),
        guard.is_some()
    );

    let (effective_cwd, strict, global) = match validate_and_prepare_plan(&plan, cwd, false) {
        Ok(v) => v,
        Err(output) => return Ok(*output),
    };

    // PathGuard enforcement for library callers of execute_plan (addresses #755).
    if let Some(g) = guard {
        // Defense-in-depth: reject plans whose cwd would escape the guard's
        // workspace root (MCP and library callers both honor plan.cwd when
        // contained; escapes must fail closed).
        if plan.cwd.is_some() {
            let canon_cwd = effective_cwd
                .canonicalize()
                .unwrap_or_else(|_| effective_cwd.clone());
            if !canon_cwd.starts_with(g.canon_root()) {
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "plan cwd '{}' escapes workspace root '{}'",
                        effective_cwd.display(),
                        g.root().display()
                    ),
                }
                .into());
            }
        }
        // Upfront check on declared paths using shared helper; dynamic (globs
        // patterns, patch embedded paths) are best-effort or handled by loaders.
        for op in &plan.operations {
            for p in op.declared_paths() {
                g.check_path(&p).map_err(|e| {
                    anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("path rejected by workspace guard: {e}"),
                    })
                })?;
            }
        }
    }

    // Pre-execution verification snapshot.
    #[cfg(feature = "ast")]
    let verify_before = if let Some(ref checks) = plan.verify {
        if !checks.is_empty() {
            let affected = verify::affected_file_paths(&plan, &effective_cwd);
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
            // operation_failed (tx default, exit 9).
            if let Some((kind, _)) = crate::exit::classify_typed_error(&e) {
                return Ok(build_error_output(kind, &e.to_string(), None));
            }
            return Ok(build_error_output("operation_failed", &e.to_string(), None));
        }
    };

    if result.replace_no_matches {
        let output = build_error_output(
            "no_matches",
            result.replace_hint.as_deref().unwrap_or(""),
            None,
        );
        return Ok(output);
    }

    if result.no_effective_changes {
        let output = build_full_tx_output("success", &mut result, &effective_cwd);
        return Ok(output);
    }

    // Post-execution verification against pending content.
    #[cfg(feature = "ast")]
    if !verify_before.is_empty() {
        let affected = verify::affected_file_paths(&plan, &effective_cwd);
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
    if let Err(err) = commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
    ) {
        let error_kind = if err.rollback_ok {
            "rollback"
        } else {
            "rollback_failed"
        };
        let output = build_error_output(error_kind, &err.message, err.backup_session.as_deref());
        return Ok(output);
    }

    // Snapshot non-tx files before format/validate steps so we can restore
    // collateral changes on strict rollback (#1111.7).
    #[cfg(any(feature = "cli", feature = "files"))]
    let collateral_snapshot = if strict && plan.has_lifecycle_steps() {
        let tx_paths: HashSet<PathBuf> = result.changes.iter().map(|(p, _, _)| p.clone()).collect();
        snapshot_non_tx_files(&effective_cwd, &tx_paths)
    } else {
        HashMap::new()
    };

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd) {
        if strict {
            rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            #[cfg(any(feature = "cli", feature = "files"))]
            restore_collateral_files(&collateral_snapshot);
            let msg = format!("strict mode -- all changes reverted ({})", err.message);
            return Ok(build_error_output("rollback", &msg, None));
        }
        return Ok(build_error_output(err.kind, &err.message, None));
    }

    let output = build_full_tx_output("success", &mut result, &effective_cwd);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::super::commit::{
        CommitError, FORCE_RESTORE_FAIL, RestoreFailGuard, commit_changes, commit_error,
    };
    use super::super::steps::{
        COLLATERAL_SNAPSHOT_MAX_SIZE, LifecycleError, lifecycle_failure_msg, resolve_plan_cwd,
        restore_collateral_files, rollback_strict, run_lifecycle, snapshot_non_tx_files,
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
        let err = validate_and_prepare_plan(&plan, dir.path(), false).unwrap_err();
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
        let (cwd, _strict, _global) =
            validate_and_prepare_plan(&plan, dir.path(), false).expect("valid plan must prepare");
        assert_eq!(cwd, dir.path());
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
            }],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };
        let err = validate_and_prepare_plan(&plan, dir.path(), false).unwrap_err();
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
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("escapes workspace root"),
            "error should mention escape: {msg}"
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
        rollback_strict(&[], &pending, &deletions, &existed_before);
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

        rollback_strict(&changes, &pending, &deletions, &existed_before);

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

        rollback_strict(&changes, &pending, &deletions, &existed_before);

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
        assert!(run_lifecycle(&plan, cwd, cwd).is_none());
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
        restore_collateral_files(&snapshot);
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
        restore_collateral_files(&snapshot);
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
}
