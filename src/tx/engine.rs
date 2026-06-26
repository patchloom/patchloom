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
use crate::exit;
use crate::plan::{Operation, Plan, SCHEMA_VERSION};
use crate::tx::output::{TxExecResult, build_full_tx_output};

use std::path::Path;

/// Options for single-operation execution.
#[derive(Debug)]
pub struct ExecuteOptions<'a> {
    /// Working directory for file resolution.
    pub cwd: &'a Path,
    /// Global flags (apply mode, write policy, etc.).
    pub global: &'a GlobalFlags,
}

/// Result of a single-operation execution.
///
/// Contains everything a CLI command needs to decide on output:
/// which files changed, what diffs were produced, and the exit code.
#[allow(dead_code)] // Public API: fields used by library embedders and MCP
pub struct ExecutionResult {
    /// The collected execution result from the engine.
    pub(crate) exec_result: TxExecResult,
    /// Pre-computed exit code.
    pub exit_code: u8,
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

    /// Build a `TxOutput` report from this result.
    #[allow(dead_code)] // Public API for library embedders
    pub fn into_tx_output(mut self) -> crate::tx::TxOutput {
        build_full_tx_output("success", &mut self.exec_result, &self.cwd)
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
    let plan = Plan {
        version: SCHEMA_VERSION.to_string(),
        operations: vec![op],
        format: None,
        validate: None,
        cwd: None,
        strict: None,
        write_policy: None,
    };

    let cwd = options.cwd.to_path_buf();
    let result = super::execute_and_collect(&plan, &cwd, options.global, true, true)?;

    let has_changes = !result.no_effective_changes;
    let exit_code = if result.replace_no_matches {
        exit::NO_MATCHES
    } else {
        exit::SUCCESS
    };

    Ok(ExecutionResult {
        exec_result: result,
        exit_code,
        has_changes,
        cwd,
    })
}

/// Execute multiple operations through the unified engine.
///
/// Similar to `execute_single` but for multi-operation batches that need
/// atomicity (all succeed or none are committed).
#[allow(dead_code)] // Public API for library embedders
pub fn execute_operations(
    operations: Vec<Operation>,
    options: ExecuteOptions<'_>,
) -> anyhow::Result<ExecutionResult> {
    let plan = Plan {
        version: SCHEMA_VERSION.to_string(),
        operations,
        format: None,
        validate: None,
        cwd: None,
        strict: None,
        write_policy: None,
    };

    let cwd = options.cwd.to_path_buf();
    let result = super::execute_and_collect(&plan, &cwd, options.global, true, true)?;

    let has_changes = !result.no_effective_changes;
    let exit_code = if result.replace_no_matches {
        exit::NO_MATCHES
    } else {
        exit::SUCCESS
    };

    Ok(ExecutionResult {
        exec_result: result,
        exit_code,
        has_changes,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
    use std::fs;
    use tempfile::TempDir;

    fn test_options<'a>(cwd: &'a Path, global: &'a GlobalFlags) -> ExecuteOptions<'a> {
        ExecuteOptions { cwd, global }
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
        assert_eq!(result.exit_code, exit::SUCCESS);

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
        assert_eq!(result.exit_code, exit::SUCCESS);

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
        assert_eq!(result.exit_code, exit::SUCCESS);
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
    fn execute_single_into_tx_output() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_default();

        let op = Operation::FileCreate {
            path: "report.txt".to_string(),
            content: "data\n".to_string(),
            force: None,
        };

        let result = execute_single(op, test_options(dir.path(), &global)).unwrap();
        let output = result.into_tx_output();
        assert!(output.ok);
        assert_eq!(output.status, "success");
        assert_eq!(output.files_created, 1);
    }
}
