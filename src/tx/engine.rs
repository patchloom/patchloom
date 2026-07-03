//! Unified execution engine for single-operation and multi-operation execution.
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
use crate::tx::output::TxExecResult;

use std::path::Path;

/// Options for single-operation execution.
#[derive(Debug)]
pub struct ExecuteOptions<'a> {
    /// Working directory for file resolution.
    pub cwd: &'a Path,
    /// Global flags (apply mode, write policy, etc.).
    pub global: &'a GlobalFlags,
    /// Optional path guard for containment validation.
    pub guard: Option<&'a crate::containment::PathGuard>,
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
            let diff = crate::diff::unified_diff(&path_str, original, new_content);
            if diff.has_changes {
                diffs.push(diff);
            }
        }
        // Include diffs for deletions (content -> empty).
        for path in &self.exec_result.deletions {
            if let Some((original, _)) = self.exec_result.pending.get(path)
                && !original.is_empty()
            {
                let rel = crate::files::relative_display(path, &self.cwd);
                let path_str = rel.to_string_lossy();
                let diff = crate::diff::unified_diff(&path_str, original, "");
                if diff.has_changes {
                    diffs.push(diff);
                }
            }
        }
        diffs
    }

    /// Commit the staged changes to disk with backup.
    pub fn commit(self) -> anyhow::Result<()> {
        if !self.has_changes {
            return Ok(());
        }
        super::commit_changes(
            &self.exec_result.changes,
            &self.exec_result.deletions,
            &self.exec_result.existed_before,
            &self.cwd,
        )
        .map_err(|e| anyhow::anyhow!("{}", e.message))
    }
}

/// Execute a single `Operation` through the unified engine.
///
/// This is the primary entry point for CLI commands that want to delegate
/// their orchestration to the tx engine. It handles:
/// - File resolution via cwd
/// - Write policy application
/// - Change collection in memory (no disk writes)
///
/// The caller decides whether to commit (via `ExecutionResult::commit()`)
/// based on the apply/check/diff mode.
pub fn execute_single(
    op: Operation,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    execute_plan_inner(vec![op], options)
}

/// Execute multiple operations through the unified engine.
///
/// Similar to `execute_single` but for multi-operation batches that need
/// atomicity (all succeed or none are committed).
///
/// Used by CLI commands (`tidy`, `ast rename`) for multi-file batches.
pub fn execute_operations(
    operations: Vec<Operation>,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    execute_plan_inner(operations, options)
}

/// A pre-computed file change: `(relative_path, original_content, new_content)`.
///
/// Used by commands that perform their own parallel scan/compute phase and
/// want to hand off the results to the engine for commit/diff/backup without
/// re-reading or re-computing.
pub type PrecomputedChange = (String, String, String);

/// Wrap pre-computed file changes into an `ExecutionResult`.
///
/// This bypasses operation execution entirely: the caller has already read
/// files and computed new content (e.g. via a parallel scan phase). The
/// engine provides only the commit/diff/backup lifecycle.
///
/// Write policy is applied to each change if enabled in `global`.
pub fn execute_precomputed(
    changes: Vec<PrecomputedChange>,
    options: ExecuteOptions<'_>,
) -> ExecutionResult {
    crate::verbose!("engine: execute_precomputed changes={}", changes.len());
    use crate::write::{apply_policy, policy_from_flags};
    use std::collections::{HashMap, HashSet};

    let cwd = options.cwd.to_path_buf();
    let mut result_changes: Vec<(std::path::PathBuf, String, String)> = Vec::new();
    let mut existed_before: HashSet<std::path::PathBuf> = HashSet::new();
    let mut pending: HashMap<std::path::PathBuf, (String, String)> = HashMap::new();

    for (rel_path, original, new_content) in changes {
        let abs_path = cwd.join(&rel_path);
        existed_before.insert(abs_path.clone());
        let policy = policy_from_flags(options.global, Some(&abs_path));
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
        no_effective_changes,
        replace_no_matches: false,
        replace_hint: None,
    };

    ExecutionResult {
        exec_result,
        has_changes: !no_effective_changes,
        cwd,
    }
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
    // Validate TidyFix-specific constraints (dedent + indent mutual exclusion).
    for op in &operations {
        if let Operation::TidyFix { dedent, indent, .. } = op
            && dedent.is_some()
            && indent.is_some()
        {
            anyhow::bail!("tidy.fix: 'dedent' and 'indent' cannot both be set");
        }
    }

    // PathGuard enforcement (same pattern as lifecycle.rs execute_plan_direct).
    if let Some(g) = options.guard {
        for op in &operations {
            for p in op.declared_paths() {
                g.check_path(&p)
                    .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {}", e))?;
            }
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

    let cwd = options.cwd.to_path_buf();
    let result =
        super::execute_and_collect(&plan, &cwd, options.global, true, true, options.guard)?;

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
        ExecuteOptions {
            cwd,
            global,
            guard: None,
        }
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
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        assert!(result.has_changes);

        // Still exists until commit
        assert!(file.exists());

        result.commit().unwrap();
        assert!(!file.exists());
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
    fn execute_single_missing_file_errors() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();
        let op = Operation::FileAppend {
            path: "nonexistent.txt".to_string(),
            content: "oops\n".to_string(),
        };

        let result = execute_single(op, test_options(dir.path(), &global));
        assert!(result.is_err());
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

        let result = execute_precomputed(changes, test_options(dir.path(), &global));
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

        let result = execute_precomputed(changes, test_options(dir.path(), &global));
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

        let result = execute_precomputed(changes, test_options(dir.path(), &global));
        let diffs = result.build_diffs();
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].has_changes);
    }
}
