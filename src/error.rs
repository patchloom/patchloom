use crate::exit;
use std::fmt;

/// All error kinds patchloom can produce, each mapped to an exit code.
#[derive(Debug)]
pub enum PatchloomError {
    /// Generic failure (exit 1).
    Generic(String),
    /// Changes detected in --check mode (exit 2).
    ChangesDetected,
    /// No matches found (exit 3).
    NoMatches,
    /// Parse or validation of input failed (exit 4).
    ParseError(String),
    /// Ambiguous target or stale context (exit 5).
    Ambiguous(String),
    /// Validation command failed (exit 6).
    ValidationFailed(String),
    /// Atomic transaction rolled back (exit 7).
    Rollback(String),
}

impl PatchloomError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Generic(_) => exit::FAILURE,
            Self::ChangesDetected => exit::CHANGES_DETECTED,
            Self::NoMatches => exit::NO_MATCHES,
            Self::ParseError(_) => exit::PARSE_ERROR,
            Self::Ambiguous(_) => exit::AMBIGUOUS,
            Self::ValidationFailed(_) => exit::VALIDATION_FAILED,
            Self::Rollback(_) => exit::ROLLBACK,
        }
    }
}

impl fmt::Display for PatchloomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generic(msg) => write!(f, "{msg}"),
            Self::ChangesDetected => write!(f, "changes detected"),
            Self::NoMatches => write!(f, "no matches found"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::Ambiguous(msg) => write!(f, "ambiguous target: {msg}"),
            Self::ValidationFailed(msg) => write!(f, "validation failed: {msg}"),
            Self::Rollback(msg) => write!(f, "transaction rolled back: {msg}"),
        }
    }
}

impl std::error::Error for PatchloomError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_mapping() {
        assert_eq!(PatchloomError::Generic("x".into()).exit_code(), 1);
        assert_eq!(PatchloomError::ChangesDetected.exit_code(), 2);
        assert_eq!(PatchloomError::NoMatches.exit_code(), 3);
        assert_eq!(PatchloomError::ParseError("x".into()).exit_code(), 4);
        assert_eq!(PatchloomError::Ambiguous("x".into()).exit_code(), 5);
        assert_eq!(PatchloomError::ValidationFailed("x".into()).exit_code(), 6);
        assert_eq!(PatchloomError::Rollback("x".into()).exit_code(), 7);
    }

    #[test]
    fn display_messages() {
        assert_eq!(PatchloomError::NoMatches.to_string(), "no matches found");
        assert_eq!(
            PatchloomError::ParseError("bad json".into()).to_string(),
            "parse error: bad json"
        );
    }
}
