//! Shared output model for CLI commands (#968).
//!
//! Provides `execute_via_engine`, a generic function that routes CLI write
//! commands through the tx engine while letting each command provide its own
//! JSON output struct for backward compatibility.
//!
//! This replaces per-command boilerplate (backup, write, diff, mode branching)
//! with a single code path shared across all write commands.

use crate::cli::global::GlobalFlags;
use crate::diff::{render_diffs_colored, render_diffs_plain};
use crate::exit;
use crate::tx::engine::{ExecuteOptions, execute_single};
use serde::Serialize;

/// Phase indicator passed to the output constructor so each command can map
/// it to its own `applied` field semantics.
#[derive(Debug, Clone, Copy)]
pub enum WritePhase {
    /// `--check`: no write performed.
    Check,
    /// `--apply`: write was performed.
    Applied,
    /// `--confirm` + JSON: conditionally applied (bool = whether user confirmed).
    Confirmed(bool),
    /// Default dry-run preview.
    Preview,
}

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
    let options = ExecuteOptions { cwd: &cwd, global };

    let result = execute_single(op, options)?;

    // --check mode: report what would happen, no mutation.
    if global.check {
        let output = make_output(WritePhase::Check, None);
        if !global.emit_json(&output)? && !global.quiet {
            println!("{check_msg}");
        }
        return if result.has_changes {
            Ok(exit::CHANGES_DETECTED)
        } else {
            Ok(exit::SUCCESS)
        };
    }

    // --apply mode: commit, then report.
    if global.apply {
        let diffs = result.build_diffs();
        result.commit()?;
        crate::write::run_format_command(global, &cwd)?;

        let diff_text = if global.diff {
            Some(render_diffs_plain(&diffs))
        } else {
            None
        };

        let output = make_output(WritePhase::Applied, diff_text);
        if !global.emit_json(&output)? {
            if global.diff {
                print!("{}", render_diffs_colored(&diffs, global.should_color()));
            } else if !global.quiet {
                println!("{apply_msg}");
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: preview, then optionally confirm.
    let diffs = if preview_diffs {
        result.build_diffs()
    } else {
        Vec::new()
    };
    let diff_text = if !diffs.is_empty() {
        Some(render_diffs_plain(&diffs))
    } else {
        None
    };

    // --confirm + JSON: prompt, conditionally apply, emit JSON.
    if global.confirm && (global.json || global.jsonl) {
        let applied = global.should_apply();
        if applied {
            result.commit()?;
            crate::write::run_format_command(global, &cwd)?;
        }
        let output = make_output(WritePhase::Confirmed(applied), diff_text);
        global.emit_json(&output)?;
        return Ok(exit::SUCCESS);
    }

    // Show preview output.
    let output = make_output(WritePhase::Preview, diff_text);
    if !global.emit_json(&output)? {
        if !diffs.is_empty() {
            print!("{}", render_diffs_colored(&diffs, global.should_color()));
        } else if !global.quiet {
            println!("{check_msg}");
        }
    }

    // --confirm: prompt after showing diff, then apply if confirmed.
    if global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, &cwd)?;
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    WritePhase::Applied => Some(true),
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
            |_phase, _diff| TestOutput {
                ok: true,
                path: "new.txt".to_string(),
                applied: None,
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
        let file = dir.path().join("doomed.txt");
        fs::write(&file, "bye\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let op = Operation::FileDelete {
            path: "doomed.txt".to_string(),
        };

        let code = execute_via_engine(
            op,
            &global,
            |phase, _diff| TestOutput {
                ok: true,
                path: "doomed.txt".to_string(),
                applied: match phase {
                    WritePhase::Applied => Some(true),
                    _ => None,
                },
            },
            "would delete doomed.txt",
            "deleted doomed.txt",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists());
    }

    #[test]
    fn execute_via_engine_append_apply() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("log.txt");
        fs::write(&file, "line1\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let op = Operation::FileAppend {
            path: "log.txt".to_string(),
            content: "line2\n".to_string(),
        };

        let code = execute_via_engine(
            op,
            &global,
            |_phase, _diff| TestOutput {
                ok: true,
                path: "log.txt".to_string(),
                applied: None,
            },
            "would append to log.txt",
            "appended to log.txt",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert_eq!(fs::read_to_string(&file).unwrap(), "line1\nline2\n");
    }

    #[test]
    fn execute_via_engine_preview_mode() {
        let dir = TempDir::new().unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());

        let op = Operation::FileCreate {
            path: "preview.txt".to_string(),
            content: "data\n".to_string(),
            force: None,
        };

        let code = execute_via_engine(
            op,
            &global,
            |_phase, _diff| TestOutput {
                ok: true,
                path: "preview.txt".to_string(),
                applied: None,
            },
            "would create preview.txt",
            "created preview.txt",
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert!(!dir.path().join("preview.txt").exists());
    }
}
