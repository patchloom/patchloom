//! Shared output model for CLI write commands.
//!
//! Engine-backed single-op writes go through
//! [`crate::cmd::write_mode::finalize_execution_result`], the single owner of
//! mode → exit mapping for staged changes (see #1373).

use crate::cli::global::GlobalFlags;
use crate::cmd::write_mode::{RenderPolicy, WriteMessages, finalize_execution_result};
use crate::tx::engine::{ExecuteOptions, execute_single};
use serde::Serialize;

pub use crate::cmd::write_mode::WritePhase;

/// Execute a single operation through the engine and render output based
/// on the global flags (check/apply/diff/confirm modes).
///
/// `make_output` builds the command-specific JSON output struct from
/// a phase indicator and an optional diff string, preserving the exact
/// JSON schema that each command's integration tests expect.
///
/// `check_msg` and `apply_msg` are human-readable messages for text output.
pub fn execute_via_engine<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    execute_via_engine_inner(op, global, make_output, check_msg, apply_msg, true)
}

/// Like `execute_via_engine` but without showing diffs in preview mode.
///
/// Used by commands like `delete` where the old behavior was to show a
/// human-readable message instead of a diff preview.
pub fn execute_via_engine_no_preview_diffs<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    execute_via_engine_inner(op, global, make_output, check_msg, apply_msg, false)
}

fn execute_via_engine_inner<T: Serialize>(
    op: crate::plan::Operation,
    global: &GlobalFlags,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    check_msg: &str,
    apply_msg: &str,
    preview_diffs: bool,
) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let options = ExecuteOptions {
        cwd: &cwd,
        global,
        guard: None,
    };

    let result = execute_single(op, options)?;
    finalize_execution_result(
        global,
        &cwd,
        result,
        make_output,
        WriteMessages {
            check: check_msg,
            apply: apply_msg,
            post_confirm: None,
        },
        RenderPolicy { preview_diffs },
    )
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
        // confirm + json in non-TTY -> should_apply() returns false

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
