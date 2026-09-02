//! Unified execution engine for single-operation and multi-operation execution.
//!
//! size-waiver: single execute_single / stage / commit path for CLI, MCP, and library (policy #1408).
//!
//! This module provides `execute_single()`, a lightweight entry point that
//! wraps one `Operation` into a minimal `Plan` and runs it through the existing
//! `execute_and_collect()` + `commit_changes()` path.
//!
//! CLI commands use this instead of reimplementing the read-compute-diff-write
//! cycle independently. MCP and the library API also route through this path,
//! ensuring a single execution engine for all surfaces.

use crate::cli::global::GlobalFlags;
use crate::plan::{Operation, Plan, SCHEMA_VERSION};
use crate::tx::context::EngineContext;
use crate::tx::output::TxExecResult;

use std::path::Path;

/// Options for staged engine execution (library-first; no clap types).
#[derive(Debug)]
pub struct ExecuteOptions<'a> {
    /// Library-first execution context (cwd, mode, write policy inputs, format).
    pub context: EngineContext,
    /// Optional path guard for containment validation.
    pub guard: Option<&'a crate::containment::PathGuard>,
}

impl<'a> ExecuteOptions<'a> {
    /// Construct options from an owned [`EngineContext`].
    pub fn new(context: EngineContext, guard: Option<&'a crate::containment::PathGuard>) -> Self {
        Self { context, guard }
    }

    /// Construct options from CLI/library global flags (boundary adapter only).
    pub fn from_global(
        cwd: &Path,
        global: &GlobalFlags,
        guard: Option<&'a crate::containment::PathGuard>,
    ) -> Self {
        Self::new(EngineContext::from_global(global, cwd.to_path_buf()), guard)
    }

    pub fn cwd(&self) -> &Path {
        self.context.cwd()
    }
}

/// How changes are staged into an [`ExecutionResult`].
#[derive(Debug)]
pub enum WriteSource {
    /// One or more plan operations (single-op or multi-op batch).
    Operations(Vec<Operation>),
    /// Pre-computed `(rel_path, original, new)` triples from a parallel scan.
    Precomputed(Vec<PrecomputedChange>),
}

/// Unified staging request: one entry shape for engine-backed writes.
#[derive(Debug)]
pub struct WriteRequest<'a> {
    pub source: WriteSource,
    pub options: ExecuteOptions<'a>,
}

/// Unified staging report (alias of [`ExecutionResult`] for the write model).
pub type WriteReport = ExecutionResult;

/// Stage changes in memory without committing.
///
/// **Canonical engine entry** for all surfaces (CLI, MCP, library). Source
/// variants cover multi-op plans and precomputed scan results; mode/exit is
/// owned by the caller (typically `cmd::write_mode::finalize_execution_result`).
pub fn stage(request: WriteRequest<'_>) -> anyhow::Result<WriteReport> {
    match request.source {
        WriteSource::Operations(mut ops) if ops.len() == 1 => {
            let op = ops.pop().expect("len checked");
            execute_single(op, request.options)
        }
        WriteSource::Operations(ops) => execute_operations(ops, request.options),
        WriteSource::Precomputed(changes) => execute_precomputed(changes, request.options),
    }
}

/// Result of a single-operation execution.
///
/// Contains everything a CLI command needs to decide on output:
/// which files changed, what diffs were produced, and the exit code.
///
/// Used by CLI commands directly and by the library API via
/// `crate::api::execute_as_edit_result()` (under the `files` feature).
/// The module-level `allow(dead_code)` in `tx/mod.rs` handles the case
/// where neither `cli` nor `files` is enabled.
pub struct ExecutionResult {
    /// The collected execution result from the engine.
    pub(crate) exec_result: TxExecResult,
    /// Whether any effective changes were produced.
    pub has_changes: bool,
    /// Working directory used.
    pub cwd: std::path::PathBuf,
}

impl ExecutionResult {
    /// Build diff output for all changed files.
    pub fn build_diffs(&self) -> Vec<crate::diff::FileDiff> {
        let mut diffs = Vec::new();
        for (path, original, new_content) in &self.exec_result.changes {
            // Skip files that are also in the deletions set to avoid
            // generating duplicate diffs (one from changes, one from
            // the deletions loop below).
            if self.exec_result.deletions.contains(path) {
                continue;
            }
            let rel = crate::files::relative_display(path, &self.cwd);
            let path_str = rel.to_string_lossy();
            let mut diff = crate::diff::unified_diff(&path_str, original, new_content);
            // Empty-create: original == new == "" so unified_diff has no hunks,
            // but the dest is a new file. Engine already counts this as a
            // change; CLI `files[]` must list it (fixrealloop R9).
            if !diff.has_changes
                && !self.exec_result.existed_before.contains(path)
                && original.is_empty()
                && new_content.is_empty()
            {
                diff.has_changes = true;
            }
            if diff.has_changes {
                diffs.push(diff);
            }
        }
        // Include diffs for deletions (content -> empty). Empty-file delete
        // has no hunks; still list the dest (sibling of empty-create).
        for path in &self.exec_result.deletions {
            if let Some((original, _)) = self.exec_result.pending.get(path) {
                let rel = crate::files::relative_display(path, &self.cwd);
                let path_str = rel.to_string_lossy();
                if original.is_empty() {
                    diffs.push(crate::diff::FileDiff {
                        path: path_str.into_owned(),
                        hunks: String::new(),
                        has_changes: true,
                    });
                    continue;
                }
                let diff = crate::diff::unified_diff(&path_str, original, "");
                if diff.has_changes {
                    diffs.push(diff);
                }
            }
        }
        diffs
    }

    /// Commit the staged changes to disk with backup.
    ///
    /// Returns the backup session timestamp when a session was created.
    /// Failures after backup finalize map to [`crate::exit::MutationAfterBackupError`]
    /// so [`crate::exit::backup_session_from_error`] can peel the session id.
    pub fn commit(self) -> anyhow::Result<Option<String>> {
        if !self.has_changes {
            return Ok(None);
        }
        super::commit_changes(
            &self.exec_result.changes,
            &self.exec_result.deletions,
            &self.exec_result.existed_before,
            &self.cwd,
            &self.exec_result.renames,
        )
        .map_err(commit_error_to_anyhow)
    }
}

/// Map [`super::CommitError`] to a peelable root error.
///
/// CLI `tx` / `plan_exec` call `commit_changes` directly and keep
/// `CommitError`. Library `execute_as_edit_result` and CLI
/// `write_mode::commit_then_format` go through this mapping.
fn commit_error_to_anyhow(err: super::CommitError) -> anyhow::Error {
    match err.backup_session {
        Some(session) if err.rollback_ok => {
            crate::exit::MutationAfterBackupError::restored(session, err.message).into()
        }
        Some(session) => crate::exit::MutationAfterBackupError::restore_failed(
            session,
            "rollback failed",
            err.message,
        )
        .into(),
        None => anyhow::anyhow!("{}", err.message),
    }
}

/// Stage a single operation (source constructor used by [`stage`]).
pub fn execute_single(
    op: Operation,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    execute_operations(vec![op], options)
}

/// Stage one or more operations (implementation used by [`stage`]).
pub fn execute_operations(
    operations: Vec<Operation>,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    execute_plan_inner(operations, options)
}

/// A pre-computed file change: `(relative_path, original_content, new_content)`.
pub type PrecomputedChange = (String, String, String);

/// Stage pre-computed changes (implementation used by [`stage`]).
///
/// When a [`PathGuard`] is present (CLI `--contain`, MCP, library), every
/// precomputed relative path is checked. This path is used by multi-file
/// scan writers such as `replace` and `tidy fix`; without this check those
/// commands could mutate files outside the workspace under `--contain`
/// (MPI cycle 15).
pub fn execute_precomputed(
    changes: Vec<PrecomputedChange>,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    crate::verbose!("engine: execute_precomputed changes={}", changes.len());
    use crate::write::apply_policy;
    use std::collections::{HashMap, HashSet};

    if let Some(g) = options.guard {
        for (rel_path, _, _) in &changes {
            g.check_path(rel_path)
                .map_err(crate::fallback::EditError::guard_rejected)?;
        }
    }
    for (rel_path, _, _) in &changes {
        crate::backup::refuse_user_write_under_backup_dir(&options.cwd().join(rel_path))?;
    }

    let cwd = options.cwd().to_path_buf();
    let ctx = &options.context;
    crate::verbose!(
        "engine: precomputed via EngineContext cwd={}",
        ctx.cwd().display()
    );
    let mut result_changes: Vec<(std::path::PathBuf, String, String)> = Vec::new();
    let mut existed_before: HashSet<std::path::PathBuf> = HashSet::new();
    let mut pending: HashMap<std::path::PathBuf, (String, String)> = HashMap::new();

    for (rel_path, original, new_content) in changes {
        let abs_path = cwd.join(&rel_path);
        existed_before.insert(abs_path.clone());
        let policy = ctx.write_policy(Some(&abs_path));
        let final_content = apply_policy(&new_content, &policy).into_owned();
        if final_content != original {
            pending.insert(abs_path.clone(), (original.clone(), final_content.clone()));
            result_changes.push((abs_path, original, final_content));
        }
    }

    let no_effective_changes = result_changes.is_empty();
    crate::verbose!(
        "engine: precomputed effective_changes={}",
        result_changes.len()
    );
    let exec_result = super::output::TxExecResult {
        changes: result_changes,
        deletions: HashSet::new(),
        existed_before,
        pending,
        tx_reads: Vec::new(),
        tx_searches: Vec::new(),
        tx_lints: Vec::new(),
        tx_mutations: Vec::new(),
        no_effective_changes,
        replace_no_matches: false,
        replace_hint: None,
        replace_match_meta: HashMap::new(),
        renames: Vec::new(),
    };

    Ok(ExecutionResult {
        exec_result,
        has_changes: !no_effective_changes,
        cwd,
    })
}

/// Shared implementation for single-op and multi-op execution.
fn execute_plan_inner(
    operations: Vec<Operation>,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    crate::verbose!(
        "engine: execute_plan_inner ops={}, guard={}",
        operations.len(),
        options.guard.is_some()
    );
    // Empty path/glob strings join to cwd and produce opaque errors
    // (`: target is not a file: <cwd>`). Reject early on all stage paths
    // (CLI stage_for_write skips validate_plan_operations).
    for op in &operations {
        for p in op.declared_paths() {
            if crate::containment::is_blank_path(&p) {
                return Err(crate::exit::InvalidInputError {
                    msg: "path must not be empty".into(),
                }
                .into());
            }
        }
        crate::backup::refuse_declared_paths_under_backup_dir(options.cwd(), op)?;
    }
    // TidyFix-specific constraints when callers skip validate_plan_operations.
    for op in &operations {
        if let Operation::TidyFix { dedent, indent, .. } = op
            && dedent.is_some()
            && indent.is_some()
        {
            return Err(crate::exit::InvalidInputError {
                msg: "tidy.fix: 'dedent' and 'indent' cannot both be set".into(),
            }
            .into());
        }
    }

    // PathGuard enforcement (same pattern as lifecycle.rs execute_plan_direct).
    // Use GuardRejected (not InvalidInput) so edit_error_kind peels correctly (#1935).
    // FileDelete / FileRename / PatchApply rename + empty-hunk delete
    // use entry mode (#2115). Leftover hunked-delete rewrites follow.
    if let Some(g) = options.guard {
        for op in &operations {
            super::execute::enforce_guard_for_op(g, op)?;
        }
    }

    let plan = Plan {
        version: SCHEMA_VERSION,
        operations,
        format: None,
        validate: None,
        verify: None,
        cwd: None,
        strict: None,
        write_policy: None,
        for_each: None,
    };

    let cwd = options.cwd().to_path_buf();
    let structured = options.context.json || options.context.jsonl;
    let result = super::execute_and_collect(
        &plan,
        &options.context,
        true, // quiet for in-engine collection (CLI prints its own output)
        structured,
        options.guard,
    )?;

    let has_changes = !result.no_effective_changes;

    Ok(ExecutionResult {
        exec_result: result,
        has_changes,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn test_options<'a>(cwd: &'a Path, global: &'a GlobalFlags) -> ExecuteOptions<'a> {
        ExecuteOptions::from_global(cwd, global, None)
    }

    #[test]
    fn execute_single_file_create() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();

        let op = Operation::FileCreate {
            path: "new_file.txt".to_string(),
            content: "hello engine\n".to_string(),
            force: None,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);

        // File should not exist yet (not committed)
        assert!(!dir.path().join("new_file.txt").exists());

        // Commit and verify
        result.commit().unwrap();
        let content = fs::read_to_string(dir.path().join("new_file.txt")).unwrap();
        assert_eq!(content, "hello engine\n");
    }

    #[test]
    fn execute_single_file_delete() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("to_delete.txt");
        fs::write(&file, "doomed\n").unwrap();

        let global = GlobalFlags::test_default();
        let op = Operation::FileDelete {
            path: "to_delete.txt".to_string(),
            if_exists: false,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);

        // Still exists until commit
        assert!(file.exists());

        result.commit().unwrap();
        assert!(!file.exists());
    }

    /// Empty-file delete must be an effective change and restore from the
    /// backup session (same path as non-empty deletes). Regression for the
    /// macOS CI failure where original == final == "" omitted the path from
    /// `changes` and strict validate rollback left the file missing.
    #[test]
    fn execute_empty_file_delete_is_effective_and_restorable() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, "").unwrap();

        let global = GlobalFlags::test_default();
        let op = Operation::FileDelete {
            path: "empty.txt".to_string(),
            if_exists: false,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(
            result.has_changes,
            "deleting an empty file is still a change"
        );
        assert!(
            result
                .exec_result
                .changes
                .iter()
                .any(|(p, _, _)| p == &file),
            "empty delete must appear in changes (not deletions-only)"
        );
        assert!(result.exec_result.deletions.contains(&file));

        let session = result.commit().unwrap();
        let ts = session.expect("empty delete must create a backup session");
        assert!(!file.exists(), "empty file deleted after commit");

        let restored = crate::backup::restore_session(dir.path(), &ts).unwrap();
        assert!(restored >= 1);
        assert!(file.exists(), "empty file restored from backup");
        assert_eq!(fs::read_to_string(&file).unwrap(), "");
    }

    #[test]
    fn execute_single_file_append() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "line one\n").unwrap();

        let global = GlobalFlags::test_default();
        let op = Operation::FileAppend {
            path: "existing.txt".to_string(),
            content: "line two\n".to_string(),
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);
        result.commit().unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "line one\nline two\n");
    }

    #[test]
    fn execute_single_file_rename() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        fs::write(&src, "moved content\n").unwrap();

        let global = GlobalFlags::test_default();
        let op = Operation::FileRename {
            from: "old.txt".to_string(),
            to: "new.txt".to_string(),
            force: false,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);
        result.commit().unwrap();

        assert!(!src.exists());
        let content = fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "moved content\n");
    }

    /// Plan/tx `file.rename` must keep multi-hardlinked inodes (#1739).
    #[cfg(unix)]
    #[test]
    fn execute_file_rename_preserves_hardlinks() {
        use std::os::unix::fs::MetadataExt;
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "shared\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        let before_ino = fs::metadata(&a).unwrap().ino();
        assert_eq!(fs::metadata(&a).unwrap().nlink(), 2);

        let global = GlobalFlags::test_default();
        let op = Operation::FileRename {
            from: "a.txt".to_string(),
            to: "c.txt".to_string(),
            force: false,
        };
        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        result.commit().unwrap();

        assert!(!a.exists());
        let c = dir.path().join("c.txt");
        assert_eq!(fs::read_to_string(&c).unwrap(), "shared\n");
        assert_eq!(fs::read_to_string(&b).unwrap(), "shared\n");
        assert_eq!(fs::metadata(&c).unwrap().ino(), before_ino);
        assert_eq!(fs::metadata(&b).unwrap().ino(), before_ino);
        assert!(
            fs::metadata(&c).unwrap().nlink() > 1,
            "nlink must stay > 1 after tx rename, got {}",
            fs::metadata(&c).unwrap().nlink()
        );
    }

    /// Rename then clear body must still write empty (not skip as soft non-text).
    #[test]
    fn execute_rename_then_clear_writes_empty() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let global = GlobalFlags::test_default();
        let plan = crate::plan::parse_plan(
            r#"{"ops":[
              {"op":"file.rename","from":"a.txt","to":"b.txt"},
              {"op":"replace","path":"b.txt","old":"hello\n","new":""}
            ]}"#,
        )
        .unwrap();
        let result =
            execute_operations(plan.operations, test_options(dir.path(), &global)).unwrap();
        result.commit().unwrap();
        let b = dir.path().join("b.txt");
        assert!(!dir.path().join("a.txt").exists());
        assert!(b.exists());
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "",
            "rename-then-clear must leave empty file, not source body"
        );
    }

    /// Rename then replace in one plan must still share the hardlink inode.
    #[cfg(unix)]
    #[test]
    fn execute_rename_then_replace_preserves_hardlinks() {
        use std::os::unix::fs::MetadataExt;
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "hello world\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        let before_ino = fs::metadata(&a).unwrap().ino();

        let global = GlobalFlags::test_default();
        let plan = crate::plan::parse_plan(
            r#"{"ops":[
              {"op":"file.rename","from":"a.txt","to":"c.txt"},
              {"op":"replace","path":"c.txt","old":"hello","new":"hi"}
            ]}"#,
        )
        .unwrap();
        let result =
            execute_operations(plan.operations, test_options(dir.path(), &global)).unwrap();
        result.commit().unwrap();

        let c = dir.path().join("c.txt");
        assert_eq!(fs::read_to_string(&c).unwrap(), "hi world\n");
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "hi world\n",
            "hardlink sibling must see the replace"
        );
        assert_eq!(fs::metadata(&c).unwrap().ino(), before_ino);
        assert!(fs::metadata(&c).unwrap().nlink() > 1);
    }

    /// Force-overwrite rename must still keep multi-hardlinked source inodes
    /// (fs::rename replaces dest; must not fall back to create+delete).
    #[cfg(unix)]
    #[test]
    fn execute_file_rename_force_preserves_hardlinks() {
        use std::os::unix::fs::MetadataExt;
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let dest = dir.path().join("existing.txt");
        fs::write(&a, "source body\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        fs::write(&dest, "old dest\n").unwrap();
        let before_ino = fs::metadata(&a).unwrap().ino();
        assert_eq!(fs::metadata(&a).unwrap().nlink(), 2);

        let global = GlobalFlags::test_default();
        let op = Operation::FileRename {
            from: "a.txt".to_string(),
            to: "existing.txt".to_string(),
            force: true,
        };
        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert_eq!(
            result.exec_result.renames.len(),
            1,
            "force rename must be recorded for hardlink-preserving commit, got {:?}",
            result.exec_result.renames
        );
        result.commit().unwrap();

        assert!(!a.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "source body\n");
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "source body\n",
            "hardlink sibling of source must still share the moved inode"
        );
        assert_eq!(fs::metadata(&dest).unwrap().ino(), before_ino);
        assert_eq!(fs::metadata(&b).unwrap().ino(), before_ino);
        assert!(
            fs::metadata(&dest).unwrap().nlink() > 1,
            "nlink must stay > 1 after force rename, got {}",
            fs::metadata(&dest).unwrap().nlink()
        );
    }

    /// Double rename then replace in one plan (a->c->d) must keep hardlinks.
    #[cfg(unix)]
    #[test]
    fn execute_rename_chain_then_replace_preserves_hardlinks() {
        use std::os::unix::fs::MetadataExt;
        let dir = TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "hello\n").unwrap();
        fs::hard_link(&a, &b).unwrap();
        let before_ino = fs::metadata(&a).unwrap().ino();

        let global = GlobalFlags::test_default();
        let plan = crate::plan::parse_plan(
            r#"{"ops":[
              {"op":"file.rename","from":"a.txt","to":"c.txt"},
              {"op":"file.rename","from":"c.txt","to":"d.txt"},
              {"op":"replace","path":"d.txt","old":"hello","new":"hi"}
            ]}"#,
        )
        .unwrap();
        let result =
            execute_operations(plan.operations, test_options(dir.path(), &global)).unwrap();
        assert_eq!(
            result.exec_result.renames.len(),
            2,
            "both renames must be recorded for chaining, got {:?}",
            result.exec_result.renames
        );
        result.commit().unwrap();

        let d = dir.path().join("d.txt");
        assert!(!dir.path().join("a.txt").exists());
        assert!(!dir.path().join("c.txt").exists());
        assert_eq!(fs::read_to_string(&d).unwrap(), "hi\n");
        assert_eq!(
            fs::read_to_string(&b).unwrap(),
            "hi\n",
            "hardlink sibling must see chained rename+replace"
        );
        assert_eq!(fs::metadata(&d).unwrap().ino(), before_ino);
        assert!(
            fs::metadata(&d).unwrap().nlink() > 1,
            "nlink must stay > 1, got {}",
            fs::metadata(&d).unwrap().nlink()
        );
    }

    #[test]
    fn execute_single_create_empty_file() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();

        let op = Operation::FileCreate {
            path: "empty.txt".to_string(),
            content: String::new(),
            force: None,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(
            result.has_changes,
            "creating a new file with empty content is still a change"
        );

        result.commit().unwrap();
        assert!(
            dir.path().join("empty.txt").exists(),
            "empty file should exist after commit"
        );
    }

    #[test]
    fn stage_operations_matches_execute_single() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();
        let op = Operation::FileCreate {
            path: "via_stage.txt".to_string(),
            content: "staged\n".to_string(),
            force: None,
        };
        let report = stage(WriteRequest {
            source: WriteSource::Operations(vec![op]),
            options: test_options(dir.path(), &global),
        })
        .unwrap();
        assert!(report.has_changes);
        report.commit().unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("via_stage.txt")).unwrap(),
            "staged\n"
        );
    }

    #[test]
    fn stage_precomputed_writes_on_commit() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("pre.txt");
        fs::write(&path, "old\n").unwrap();
        let global = GlobalFlags::test_default();
        let report = stage(WriteRequest {
            source: WriteSource::Precomputed(vec![(
                "pre.txt".to_string(),
                "old\n".to_string(),
                "new\n".to_string(),
            )]),
            options: test_options(dir.path(), &global),
        })
        .unwrap();
        assert!(report.has_changes);
        assert_eq!(fs::read_to_string(&path).unwrap(), "old\n");
        report.commit().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    }

    /// Engine `commit` must keep `backup_session` on `CommitError` so library
    /// `execute_as_edit_result` and CLI `commit_then_format` can peel it.
    #[test]
    fn engine_commit_fail_preserves_backup_session() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target.txt");
        fs::write(&path, "original\n").unwrap();
        let global = GlobalFlags::test_default();
        let report = stage(WriteRequest {
            source: WriteSource::Precomputed(vec![(
                "target.txt".to_string(),
                "original\n".to_string(),
                "changed\n".to_string(),
            )]),
            options: test_options(dir.path(), &global),
        })
        .unwrap();
        assert!(report.has_changes);

        let _write_fail = crate::tx::WriteFailGuard::fail_paths_containing("target.txt");
        let err = report.commit().expect_err("injected write failure");
        let session = crate::exit::backup_session_from_error(&err)
            .expect("hosts must peel backup_session after engine commit fail");
        assert!(
            !session.is_empty(),
            "peeled session must be the finalized id"
        );
        let typed = err
            .downcast_ref::<crate::exit::MutationAfterBackupError>()
            .expect("CommitError with session maps to MutationAfterBackupError");
        assert!(typed.restored, "rollback should succeed for a single fail");
        assert_eq!(typed.session, session);
        let sessions = crate::backup::list_sessions(dir.path()).unwrap();
        assert!(
            sessions.iter().any(|s| s.timestamp == session),
            "peeled session {session:?} must match list_sessions: {sessions:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
    }

    /// Same peel when restore after the failed write also fails.
    #[test]
    fn engine_commit_fail_restore_failed_preserves_backup_session() {
        let dir = TempDir::new().unwrap();
        let good = dir.path().join("a_good.txt");
        fs::write(&good, "original\n").unwrap();
        let global = GlobalFlags::test_default();
        let report = stage(WriteRequest {
            source: WriteSource::Precomputed(vec![
                (
                    "a_good.txt".to_string(),
                    "original\n".to_string(),
                    "changed\n".to_string(),
                ),
                (
                    "z_fail/child.txt".to_string(),
                    String::new(),
                    "fail\n".to_string(),
                ),
            ]),
            options: test_options(dir.path(), &global),
        })
        .unwrap();

        let _write_fail = crate::tx::WriteFailGuard::fail_paths_containing("z_fail");
        let _restore_fail = crate::tx::RestoreFailGuard::engage();
        let err = report.commit().expect_err("injected write + restore fail");
        let session = crate::exit::backup_session_from_error(&err)
            .expect("hosts must peel backup_session after restore-failed commit");
        assert!(!session.is_empty());
        let typed = err
            .downcast_ref::<crate::exit::MutationAfterBackupError>()
            .expect("restore-failed CommitError maps to MutationAfterBackupError");
        assert!(!typed.restored, "restore was injected to fail");
        assert_eq!(typed.session, session);
    }

    #[test]
    fn execute_options_is_context_only() {
        // Boundary: ExecuteOptions holds EngineContext + guard, not GlobalFlags.
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();
        let opts = ExecuteOptions::from_global(dir.path(), &global, None);
        assert_eq!(opts.cwd(), dir.path());
        assert!(opts.guard.is_none());
        // Construct without GlobalFlags (library-first path).
        let ctx = EngineContext::from_global(&global, dir.path().to_path_buf());
        let opts2 = ExecuteOptions::new(ctx, None);
        assert_eq!(opts2.cwd(), dir.path());
    }

    #[test]
    fn execute_single_no_changes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("stable.txt");
        fs::write(&file, "content\n").unwrap();

        let global = GlobalFlags::test_default();
        // Append empty string = no change
        let op = Operation::FileAppend {
            path: "stable.txt".to_string(),
            content: String::new(),
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(!result.has_changes);
    }

    #[test]
    fn execute_single_build_diffs() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "original\n").unwrap();

        let global = GlobalFlags::test_default();
        let op = Operation::FileAppend {
            path: "test.txt".to_string(),
            content: "appended\n".to_string(),
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        let diffs = result.build_diffs();
        assert!(!diffs.is_empty());
        assert!(diffs[0].has_changes);
    }

    #[test]
    fn execute_single_empty_create_build_diffs_lists_dest() {
        // Empty-to-empty unified_diff has no hunks, but creating a 0-byte
        // dest is still a change (fixrealloop R9: patch apply files[]).
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();
        let op = Operation::FileCreate {
            path: "empty.txt".to_string(),
            content: String::new(),
            force: None,
        };
        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(
            result.has_changes,
            "empty create must be an effective change"
        );
        let diffs = result.build_diffs();
        assert_eq!(
            diffs.len(),
            1,
            "empty create must appear in diffs: {diffs:?}"
        );
        assert!(diffs[0].has_changes, "{diffs:?}");
        assert!(
            diffs[0].path.ends_with("empty.txt"),
            "dest path: {}",
            diffs[0].path
        );
    }

    #[test]
    fn execute_single_missing_file_errors() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();
        let op = Operation::FileAppend {
            path: "nonexistent.txt".to_string(),
            content: "oops\n".to_string(),
        };

        let result = execute_single(op, test_options(dir.path(), &global));
        assert!(result.is_err(), "expected containment rejection");
    }

    #[test]
    fn execute_operations_multi() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();

        let ops = vec![
            Operation::FileCreate {
                path: "a.txt".to_string(),
                content: "file a\n".to_string(),
                force: None,
            },
            Operation::FileCreate {
                path: "b.txt".to_string(),
                content: "file b\n".to_string(),
                force: None,
            },
        ];

        let result = execute_operations(ops, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);
        result.commit().unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "file a\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "file b\n"
        );
    }

    #[test]
    fn execute_precomputed_commits_changes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "original\n").unwrap();

        let global = GlobalFlags::test_default();
        let changes = vec![(
            "test.txt".to_string(),
            "original\n".to_string(),
            "replaced\n".to_string(),
        )];

        let result = execute_precomputed(changes, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);

        // Not committed yet.
        assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");

        // Commit and verify.
        result.commit().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "replaced\n");
    }

    #[test]
    fn execute_precomputed_no_change_when_identical() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "same\n").unwrap();

        let global = GlobalFlags::test_default();
        let changes = vec![(
            "test.txt".to_string(),
            "same\n".to_string(),
            "same\n".to_string(),
        )];

        let result = execute_precomputed(changes, test_options(dir.path(), &global)).unwrap();
        assert!(!result.has_changes);
    }

    #[test]
    fn execute_precomputed_builds_diffs() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "old\n").unwrap();

        let global = GlobalFlags::test_default();
        let changes = vec![(
            "test.txt".to_string(),
            "old\n".to_string(),
            "new\n".to_string(),
        )];

        let result = execute_precomputed(changes, test_options(dir.path(), &global)).unwrap();
        let diffs = result.build_diffs();
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].has_changes);
    }

    #[test]
    fn execute_precomputed_with_guard_rejects_parent_escape() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.contain = true;
        let guard = global.workspace_guard(dir.path()).unwrap().unwrap();
        let options = ExecuteOptions::from_global(dir.path(), &global, Some(&guard));
        let changes = vec![(
            "../escape.txt".to_string(),
            "old\n".to_string(),
            "new\n".to_string(),
        )];

        match execute_precomputed(changes, options) {
            Ok(_) => panic!("expected containment rejection for ../escape.txt"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("escapes")
                        || msg.contains("rejected")
                        || msg.contains("workspace guard"),
                    "expected containment error, got: {msg}"
                );
            }
        }
    }
}
