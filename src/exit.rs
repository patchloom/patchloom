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
