use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::exit;
use serde::Serialize;
use std::path::Path;

/// Human-readable messages for the write state machine.
pub struct WriteMessages<'a> {
    /// Message for `--check` and dry-run when no diff is available.
    pub check: &'a str,
    /// Message for `--apply` when `--diff` is not set.
    pub apply: &'a str,
    /// Optional message printed via `show_status()` after interactive
    /// confirm-and-apply in default mode.
    pub post_confirm: Option<&'a str>,
}

/// Shared state machine for single-file write commands.
///
/// Handles the check / apply / confirm+json / default(diff) branching that
/// `append`, `create`, `delete`, and `rename` all share.
///
/// # Parameters
///
/// * `make_output` - builds the command-specific output struct from a phase
///   and an optional diff string (uncolored, for JSON).
/// * `diff_fn` - produces a diff string; `None` for commands without diffs
///   (e.g. `delete`). The `bool` argument is whether to emit ANSI color.
/// * `apply_fn` - performs the actual mutation (backup + write). Called at
///   most once.
/// * `msgs` - human-readable messages for check, apply, and post-confirm.
pub fn execute_write<T: Serialize>(
    global: &GlobalFlags,
    cwd: &Path,
    make_output: impl Fn(WritePhase, Option<String>) -> T,
    diff_fn: Option<&dyn Fn(bool) -> String>,
    mut apply_fn: impl FnMut() -> anyhow::Result<()>,
    msgs: WriteMessages<'_>,
) -> anyhow::Result<u8> {
    // --check mode: report what would happen, no mutation.
    if global.check {
        let output = make_output(WritePhase::Check(true), None);
        if !global.emit_json(&output)? && !global.quiet {
            println!("{}", msgs.check);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: mutate, then report.
    if global.apply {
        apply_fn()?;
        crate::write::run_format_command(global, cwd)?;

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
            } else if !global.quiet {
                println!("{}", msgs.apply);
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: preview, then optionally confirm.
    let diff_text = diff_fn.map(|f| f(false));

    // --confirm + JSON: prompt, conditionally apply, emit JSON.
    if global.confirm && (global.json || global.jsonl) {
        let applied = global.should_apply();
        if applied {
            apply_fn()?;
            crate::write::run_format_command(global, cwd)?;
        }
        let output = make_output(WritePhase::Confirmed(applied), diff_text);
        global.emit_json(&output)?;
        return if applied {
            Ok(exit::SUCCESS)
        } else {
            Ok(exit::CHANGES_DETECTED)
        };
    }

    // Show preview output.
    let output = make_output(WritePhase::Preview, diff_text);
    if !global.emit_json(&output)? {
        if let Some(f) = diff_fn {
            print!("{}", f(global.should_color()));
        } else if !global.quiet {
            println!("{}", msgs.check);
        }
    }

    // --confirm: prompt after showing diff, then apply if confirmed.
    if global.should_apply() {
        apply_fn()?;
        crate::write::run_format_command(global, cwd)?;
        if let Some(msg) = msgs.post_confirm
            && global.show_status()
        {
            eprintln!("{msg}");
        }
        return Ok(exit::SUCCESS);
    }

    Ok(exit::CHANGES_DETECTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, Serialize, PartialEq)]
    struct TestOutput {
        ok: bool,
        diff: Option<String>,
        phase: String,
    }

    fn make_test_output(phase: WritePhase, diff: Option<String>) -> TestOutput {
        let phase_str = match phase {
            WritePhase::Check(_) => "check".to_string(),
            WritePhase::Applied => "applied".to_string(),
            WritePhase::Confirmed(b) => format!("confirmed({b})"),
            WritePhase::Preview => "preview".to_string(),
        };
        TestOutput {
            ok: true,
            diff,
            phase: phase_str,
        }
    }

    fn test_msgs() -> WriteMessages<'static> {
        WriteMessages {
            check: "would change",
            apply: "changed",
            post_confirm: None,
        }
    }

    #[test]
    fn check_mode_returns_changes_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;

        let mut applied = false;
        let code = execute_write(
            &global,
            dir.path(),
            make_test_output,
            Some(&|_color| "diff".to_string()),
            || {
                applied = true;
                Ok(())
            },
            test_msgs(),
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!applied, "check mode must not apply");
    }

    #[test]
    fn apply_mode_calls_apply_fn() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let mut applied = false;
        let code = execute_write(
            &global,
            dir.path(),
            make_test_output,
            Some(&|_color| "diff".to_string()),
            || {
                applied = true;
                Ok(())
            },
            test_msgs(),
        )
        .unwrap();

        assert_eq!(code, exit::SUCCESS);
        assert!(applied, "apply mode must call apply_fn");
    }

    #[test]
    fn default_mode_returns_changes_detected_without_apply() {
        let dir = tempfile::TempDir::new().unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());

        let mut applied = false;
        let code = execute_write(
            &global,
            dir.path(),
            make_test_output,
            Some(&|_color| "diff".to_string()),
            || {
                applied = true;
                Ok(())
            },
            test_msgs(),
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!applied, "default mode must not apply");
    }

    #[test]
    fn no_diff_fn_uses_human_check_in_preview() {
        let dir = tempfile::TempDir::new().unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());

        let code = execute_write(
            &global,
            dir.path(),
            make_test_output,
            None::<&dyn Fn(bool) -> String>,
            || Ok(()),
            WriteMessages {
                check: "would delete foo.txt",
                apply: "deleted foo.txt",
                post_confirm: None,
            },
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn confirm_json_decline_returns_changes_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.confirm = true;
        global.json = true;
        // confirm + json in non-TTY -> should_apply() returns false

        let mut applied = false;
        let code = execute_write(
            &global,
            dir.path(),
            make_test_output,
            Some(&|_color| "diff".to_string()),
            || {
                applied = true;
                Ok(())
            },
            test_msgs(),
        )
        .unwrap();

        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(!applied, "decline must not apply");
    }

    #[test]
    fn apply_error_propagates() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let result = execute_write(
            &global,
            dir.path(),
            make_test_output,
            None::<&dyn Fn(bool) -> String>,
            || anyhow::bail!("write failed"),
            test_msgs(),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("write failed"));
    }
}
