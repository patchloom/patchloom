/// Exit codes for patchloom.
pub const SUCCESS: u8 = 0;
pub const FAILURE: u8 = 1;
pub const CHANGES_DETECTED: u8 = 2;
pub const NO_MATCHES: u8 = 3;
pub const PARSE_ERROR: u8 = 4;
pub const AMBIGUOUS: u8 = 5;
pub const VALIDATION_FAILED: u8 = 6;
pub const ROLLBACK: u8 = 7;

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
    }
}
