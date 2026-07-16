use crate::cli::global::GlobalFlags;
use crate::cmd::write_mode::finalize_callback_write;
use serde::Serialize;
use std::path::Path;

pub use crate::cmd::write_mode::{WriteMessages, WritePhase};

/// Shared state machine for callback-based single-file writes (binary /
/// case-only rename). Mode → exit ownership lives in
/// [`crate::cmd::write_mode`].
pub fn execute_write<T: Serialize>(
    global: &GlobalFlags,
    cwd: &Path,
    make_output: impl Fn(WritePhase, Option<String>, Option<String>) -> T,
    diff_fn: Option<&dyn Fn(bool) -> String>,
    apply_fn: impl FnMut() -> anyhow::Result<()>,
    msgs: WriteMessages<'_>,
) -> anyhow::Result<u8> {
    // Callback paths are only entered when a change is known to be needed.
    finalize_callback_write(global, cwd, true, make_output, diff_fn, apply_fn, msgs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
    use serde::Serialize;

    #[derive(Debug, Serialize, PartialEq)]
    struct TestOutput {
        ok: bool,
        diff: Option<String>,
        phase: String,
    }

    fn make_test_output(
        phase: WritePhase,
        diff: Option<String>,
        _backup: Option<String>,
    ) -> TestOutput {
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
        // Force decline: Docker/eval harnesses often allocate a pseudo-TTY,
        // which would otherwise prompt and default to yes on empty input.
        let _decline = crate::cli::global::ConfirmAnswerGuard::force(false);

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

        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(result.unwrap_err().to_string().contains("write failed"));
    }
}
