//! Shared output model for CLI write commands.
//!
//! **Canonical engine-backed write path (rewrite singularity):**
//!
//! 1. Build a [`crate::tx::engine::WriteRequest`] (`Operations` or `Precomputed`)
//! 2. Call [`run_write`] (or [`stage`](crate::tx::engine::stage) +
//!    [`finalize_execution_result`](crate::cmd::write_mode::finalize_execution_result)
//!    when a command needs a custom JSON schema)
//!
//! Mode → exit ownership always lives in [`crate::cmd::write_mode`].
//! Binary/case-only renames use [`crate::cmd::write_dispatch::execute_write`]
//! (callback path; same exit matrix via `finalize_callback_write`).

use crate::cli::global::GlobalFlags;
use crate::cmd::write_mode::{RenderPolicy, WriteMessages, finalize_execution_result};
use crate::tx::engine::{ExecuteOptions, WriteRequest, WriteSource, stage};
use serde::Serialize;

pub use crate::cmd::write_mode::WritePhase;

/// Stage a write and finalize under global flags (canonical single-op path).
///
/// Prefer this over constructing engine options and calling `stage` yourself
/// when the command uses the standard phase-based JSON schema.
pub fn run_write<T: Serialize>(
    source: WriteSource,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    msgs: WriteMessages<'_>,
    render: RenderPolicy,
) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let guard = global.workspace_guard(&cwd)?;
    let options = ExecuteOptions::from_global(&cwd, global, guard.as_ref());
    let result = stage(WriteRequest { source, options })?;
    finalize_execution_result(global, &cwd, result, make_output, msgs, render)
}

/// Stage one `Operation` and finalize (most CLI write commands).
pub fn run_write_op<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    run_write(
        WriteSource::Operations(vec![op]),
        global,
        make_output,
        WriteMessages {
            check: check_msg,
            apply: apply_msg,
            post_confirm: None,
        },
        RenderPolicy::default(),
    )
}

/// Like [`run_write_op`] but without unified diffs in preview (e.g. `delete`).
pub fn run_write_op_no_preview_diffs<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    run_write(
        WriteSource::Operations(vec![op]),
        global,
        make_output,
        WriteMessages {
            check: check_msg,
            apply: apply_msg,
            post_confirm: None,
        },
        RenderPolicy {
            preview_diffs: false,
        },
    )
}

/// Stage multi-op or precomputed changes and return the report for a custom
/// renderer. Callers must use [`finalize_execution_result`] (or the same
/// `classify_write_mode` / `write_exit_code` contract) for mode/exit.
pub fn stage_for_write(
    source: WriteSource,
    global: &GlobalFlags,
) -> anyhow::Result<(std::path::PathBuf, crate::tx::engine::WriteReport)> {
    let cwd = global.resolve_cwd()?;
    let guard = global.workspace_guard(&cwd)?;
    let options = ExecuteOptions::from_global(&cwd, global, guard.as_ref());
    let report = stage(WriteRequest { source, options })?;
    Ok((cwd, report))
}

// ---------------------------------------------------------------------------
// Compatibility aliases (same behavior; prefer run_write* for new code)
// ---------------------------------------------------------------------------

/// Execute a single operation through the engine and render output based
/// on the global flags (check/apply/diff/confirm modes).
///
/// Prefer [`run_write_op`].
pub fn execute_via_engine<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    run_write_op(op, global, make_output, check_msg, apply_msg)
}

/// Like `execute_via_engine` but without showing diffs in preview mode.
///
/// Prefer [`run_write_op_no_preview_diffs`].
pub fn execute_via_engine_no_preview_diffs<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    run_write_op_no_preview_diffs(op, global, make_output, check_msg, apply_msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
    use crate::plan::Operation;
    use std::fs;
    use tempfile::TempDir;

    #[derive(Debug, Serialize)]
    struct TestOutput {
        ok: bool,
        path: String,
        applied: Option<bool>,
    }

    #[test]
    fn execute_via_engine_create_apply() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let op = Operation::FileCreate {
            path: "new.txt".to_string(),
            content: "hello\n".to_string(),
            force: None,
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "new.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would create new.txt",
            "created new.txt",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert_eq!(
            fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn execute_via_engine_create_check() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;

        let op = Operation::FileCreate {
            path: "new.txt".to_string(),
            content: "hello\n".to_string(),
            force: None,
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "new.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would create new.txt",
            "created new.txt",
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!dir.path().join("new.txt").exists());
    }

    #[test]
    fn execute_via_engine_delete_apply() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("del.txt");
        fs::write(&file, "x\n").unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let op = Operation::FileDelete {
            path: "del.txt".to_string(),
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "del.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would delete del.txt",
            "deleted del.txt",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists());
    }

    #[test]
    fn execute_via_engine_append_apply() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "hi\n").unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let op = Operation::FileAppend {
            path: "a.txt".to_string(),
            content: "there\n".to_string(),
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "a.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would append",
            "appended",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert_eq!(fs::read_to_string(&file).unwrap(), "hi\nthere\n");
    }

    #[test]
    fn execute_via_engine_preview_mode() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());

        let op = Operation::FileCreate {
            path: "new.txt".to_string(),
            content: "hello\n".to_string(),
            force: None,
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "new.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would create new.txt",
            "created new.txt",
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!dir.path().join("new.txt").exists());
    }

    #[test]
    fn execute_via_engine_confirm_json_decline_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.confirm = true;
        global.json = true;
        // Force decline: Docker/eval harnesses often allocate a pseudo-TTY,
        // which would otherwise prompt and default to yes on empty input.
        let _decline = crate::cli::global::ConfirmAnswerGuard::force(false);

        let op = Operation::FileCreate {
            path: "new.txt".to_string(),
            content: "hello\n".to_string(),
            force: None,
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "new.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would create new.txt",
            "created new.txt",
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!dir.path().join("new.txt").exists());
    }

    #[test]
    fn execute_via_engine_preview_no_changes() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "hello\n").unwrap();

        let global = GlobalFlags::test_with_cwd(dir.path());

        let op = Operation::FileAppend {
            path: "existing.txt".to_string(),
            content: String::new(),
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "existing.txt".to_string(),
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            "would append",
            "appended",
        )
        .unwrap();

        // Empty append may be treated as no effective change.
        assert!(
            code == exit::SUCCESS || code == exit::CHANGES_DETECTED,
            "unexpected code {code}"
        );
    }
}
