//! Post-write format/lint hooks for agent hosts (#1663).
//!
//! After `ApplyMode::Apply`, hosts can run project formatters or linters and
//! optionally revert the written path via the existing backup session.

use std::path::Path;

use crate::backup::restore_path_from_latest_backup;
use crate::exit::FormatFailedError;
use crate::fallback::{EditError, EditErrorKind};

/// What to do when a post-write hook fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostWriteOnFailure {
    /// Leave the written content; return a structured format failure.
    #[default]
    KeepWithError,
    /// Restore the path from the latest backup session that contains it.
    Revert,
}

/// Format and/or lint hooks after an Apply write.
#[derive(Debug, Clone, Default)]
pub struct PostWriteHooks {
    /// Shell command run from `project_root` (e.g. `"rustfmt src/lib.rs"`).
    pub format_cmd: Option<String>,
    /// Optional second shell command (lint). Runs only if format succeeds.
    pub lint_cmd: Option<String>,
    /// Failure policy (default keep with error).
    pub on_failure: PostWriteOnFailure,
    /// Per-command timeout in seconds (default 30).
    pub timeout_secs: Option<u64>,
}

/// Run optional format then lint after a successful Apply.
///
/// Failures map to [`FormatFailedError`] (JSON `error_kind: format_failed`) so
/// they peel via [`crate::api::edit_error_kind`] / [`crate::api::classify_error`]
/// as [`EditErrorKind::OperationFailed`]. With [`PostWriteOnFailure::Revert`],
/// restores only `path` from the latest backup that contains it (see
/// [`restore_path_from_latest_backup`]).
///
/// No-op when both commands are unset.
pub fn run_post_write_validation(
    project_root: &Path,
    path: &Path,
    hooks: &PostWriteHooks,
) -> anyhow::Result<()> {
    let timeout = hooks.timeout_secs.unwrap_or(30);
    for (label, cmd) in [
        ("format", hooks.format_cmd.as_deref()),
        ("lint", hooks.lint_cmd.as_deref()),
    ] {
        let Some(cmd) = cmd else {
            continue;
        };
        if let Err(e) = run_hook_cmd(cmd, timeout, project_root, label) {
            if hooks.on_failure == PostWriteOnFailure::Revert {
                // Surface restore failure: silent Ok(false)/Err left the file
                // mutated while the host only saw the format error.
                match restore_path_from_latest_backup(project_root, path) {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(FormatFailedError {
                            msg: format!(
                                "{e}; also failed to revert {}: no backup session for path",
                                path.display()
                            ),
                        }
                        .into());
                    }
                    Err(restore_err) => {
                        return Err(FormatFailedError {
                            msg: format!(
                                "{e}; also failed to revert {}: {restore_err}",
                                path.display()
                            ),
                        }
                        .into());
                    }
                }
            }
            return Err(e);
        }
    }
    Ok(())
}

fn run_hook_cmd(cmd: &str, timeout_secs: u64, cwd: &Path, label: &str) -> anyhow::Result<()> {
    let result =
        crate::exec::run_with_timeout(cmd, timeout_secs, cwd).map_err(|e| FormatFailedError {
            msg: format!("{label} command failed ({cmd}): {e}"),
        })?;
    if !result.status.success() {
        let stderr = if result.stderr_head.is_empty() {
            String::new()
        } else {
            format!(": {}", result.stderr_head)
        };
        return Err(FormatFailedError {
            msg: format!("{label} command failed ({cmd}){stderr}"),
        }
        .into());
    }
    Ok(())
}

/// Host-supplied post-write validator trait (#1663).
///
/// Prefer [`run_post_write_validation`] for shell format/lint; implement this
/// when validation is pure Rust.
pub trait PostWriteValidator {
    /// Return `Ok(())` if `after` is acceptable; otherwise an [`EditError`].
    fn validate(&self, path: &Path, before: &str, after: &str) -> Result<(), EditError>;
}

/// Apply a [`PostWriteValidator`] after an in-memory or disk edit.
///
/// On failure with `revert=true`, restores `path` from the latest backup under
/// `project_root` (no-op if no backup).
pub fn apply_post_write_validator<V: PostWriteValidator + ?Sized>(
    project_root: &Path,
    path: &Path,
    before: &str,
    after: &str,
    validator: &V,
    revert: bool,
) -> anyhow::Result<()> {
    match validator.validate(path, before, after) {
        Ok(()) => Ok(()),
        Err(e) => {
            if revert {
                match restore_path_from_latest_backup(project_root, path) {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(EditError::new(
                            EditErrorKind::OperationFailed,
                            format!(
                                "{e}; also failed to revert {}: no backup session for path",
                                path.display()
                            ),
                        )
                        .into());
                    }
                    Err(restore_err) => {
                        return Err(EditError::new(
                            EditErrorKind::OperationFailed,
                            format!(
                                "{e}; also failed to revert {}: {restore_err}",
                                path.display()
                            ),
                        )
                        .into());
                    }
                }
            }
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApplyMode, ReplaceOptions, replace_text};
    use crate::fallback::{EditErrorKind, edit_error_kind};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn post_write_format_success_is_ok() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("ok.txt");
        fs::write(&file, "a\n").unwrap();
        let hooks = PostWriteHooks {
            format_cmd: Some("true".into()),
            ..Default::default()
        };
        run_post_write_validation(dir.path(), &file, &hooks).unwrap();
    }

    #[test]
    fn post_write_format_failure_is_format_failed() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("bad.txt");
        fs::write(&file, "a\n").unwrap();
        let hooks = PostWriteHooks {
            format_cmd: Some("false".into()),
            on_failure: PostWriteOnFailure::KeepWithError,
            ..Default::default()
        };
        let err = run_post_write_validation(dir.path(), &file, &hooks).unwrap_err();
        assert!(
            crate::exit::is_format_failed(&err),
            "expected format_failed, got {err}"
        );
        assert_eq!(edit_error_kind(&err), Some(EditErrorKind::OperationFailed));
    }

    #[test]
    fn post_write_revert_restores_path() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.txt");
        fs::write(&file, "before\n").unwrap();
        let opts = ReplaceOptions {
            require_change: true,
            ..Default::default()
        };
        replace_text(&file, "before", "after", &opts, ApplyMode::Apply, None).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");

        let hooks = PostWriteHooks {
            format_cmd: Some("false".into()),
            on_failure: PostWriteOnFailure::Revert,
            ..Default::default()
        };
        let err = run_post_write_validation(dir.path(), &file, &hooks).unwrap_err();
        assert!(crate::exit::is_format_failed(&err));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "before\n",
            "revert must restore pre-Apply bytes"
        );
    }

    struct AlwaysFail;

    impl PostWriteValidator for AlwaysFail {
        fn validate(&self, _path: &Path, _before: &str, _after: &str) -> Result<(), EditError> {
            Err(EditError::new(
                EditErrorKind::OperationFailed,
                "lint regression",
            ))
        }
    }

    #[test]
    fn post_write_validator_trait_reverts() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("t.txt");
        fs::write(&file, "old\n").unwrap();
        replace_text(
            &file,
            "old",
            "new",
            &ReplaceOptions {
                require_change: true,
                ..Default::default()
            },
            ApplyMode::Apply,
            None,
        )
        .unwrap();
        let err =
            apply_post_write_validator(dir.path(), &file, "old\n", "new\n", &AlwaysFail, true)
                .unwrap_err();
        assert_eq!(edit_error_kind(&err), Some(EditErrorKind::OperationFailed));
        assert_eq!(fs::read_to_string(&file).unwrap(), "old\n");
    }
}
