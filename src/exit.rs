//! Exit codes for patchloom.

#![cfg_attr(not(feature = "cli"), allow(dead_code))]

/// Command completed successfully.
pub const SUCCESS: u8 = 0;
/// Unrecoverable error (I/O failure, invalid arguments, etc.).
pub const FAILURE: u8 = 1;
/// Write command detected pending changes (`--check` mode).
pub const CHANGES_DETECTED: u8 = 2;
/// Search or replace found zero matches.
pub const NO_MATCHES: u8 = 3;

/// Typed error for no-match conditions in tx engine operations.
///
/// Commands check for this via `anyhow::Error::downcast_ref::<NoMatchError>()`
/// instead of fragile `msg.contains(...)` string matching on error messages.
/// This decouples exit-code classification from error message wording.
#[derive(Debug)]
pub struct NoMatchError {
    pub msg: String,
}

impl std::fmt::Display for NoMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for NoMatchError {}

/// Check whether an `anyhow::Error` chain contains a `NoMatchError`.
///
/// The tx engine wraps operation errors with context (e.g.
/// "operation 1 (doc.update) failed"), so the `NoMatchError` may not be
/// the top-level error. This walks the full chain via `anyhow::Chain`.
pub fn is_no_match(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<NoMatchError>().is_some())
}

/// Plan, patch, or structured document could not be parsed.
pub const PARSE_ERROR: u8 = 4;
/// Multiple candidates matched and the command could not pick one.
pub const AMBIGUOUS: u8 = 5;
/// A `validate` step failed (tx lifecycle).
pub const VALIDATION_FAILED: u8 = 6;
/// Strict-mode rollback triggered by a failed `format` or `validate` step.
pub const ROLLBACK: u8 = 7;
/// Patch merge produced conflict markers.
pub const CONFLICTS: u8 = 8;
/// Tx operation staging failure (`error_kind`: `operation_failed`).
pub const OPERATION_FAILED: u8 = 9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_values() {
        assert_eq!(SUCCESS, 0);
        assert_eq!(FAILURE, 1);
        assert_eq!(CHANGES_DETECTED, 2);
        assert_eq!(NO_MATCHES, 3);
        assert_eq!(PARSE_ERROR, 4);
        assert_eq!(AMBIGUOUS, 5);
        assert_eq!(VALIDATION_FAILED, 6);
        assert_eq!(ROLLBACK, 7);
        assert_eq!(CONFLICTS, 8);
        assert_eq!(OPERATION_FAILED, 9);
    }

    #[test]
    fn no_match_error_downcast() {
        let err: anyhow::Error = NoMatchError {
            msg: "selector 'x' matched nothing".to_string(),
        }
        .into();
        assert!(
            err.downcast_ref::<NoMatchError>().is_some(),
            "NoMatchError should be downcastable from anyhow::Error"
        );
    }

    #[test]
    fn no_match_error_display() {
        let err = NoMatchError {
            msg: "test message".to_string(),
        };
        assert_eq!(err.to_string(), "test message");
    }

    #[test]
    fn non_no_match_error_not_downcast() {
        let err = anyhow::anyhow!("some other error");
        assert!(
            err.downcast_ref::<NoMatchError>().is_none(),
            "plain anyhow error should not downcast to NoMatchError"
        );
    }

    #[test]
    fn is_no_match_finds_wrapped_error() {
        let err: anyhow::Error = NoMatchError {
            msg: "selector matched nothing".to_string(),
        }
        .into();
        let wrapped = err.context("operation 1 (doc.update) failed");
        assert!(
            is_no_match(&wrapped),
            "is_no_match should find NoMatchError through .context() wrapper"
        );
    }

    #[test]
    fn is_no_match_rejects_wrapped_non_no_match() {
        let err = anyhow::anyhow!("some other error").context("operation 1 failed");
        assert!(
            !is_no_match(&err),
            "is_no_match should return false for non-NoMatchError in chain"
        );
    }

    #[test]
    fn exit_codes_are_unique() {
        let codes = [
            SUCCESS,
            FAILURE,
            CHANGES_DETECTED,
            NO_MATCHES,
            PARSE_ERROR,
            AMBIGUOUS,
            VALIDATION_FAILED,
            ROLLBACK,
            CONFLICTS,
            OPERATION_FAILED,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }
}
