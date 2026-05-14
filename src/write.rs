//! Atomic write, final newline, EOL normalization, trailing-whitespace trimming.

use std::path::Path;

use anyhow::Context;
use tempfile::NamedTempFile;

use crate::cli::global::EolMode;

/// Controls which transformations are applied before writing a file.
pub struct WritePolicy {
    pub ensure_final_newline: bool,
    pub normalize_eol: EolMode,
    pub trim_trailing_whitespace: bool,
}

impl Default for WritePolicy {
    fn default() -> Self {
        Self {
            ensure_final_newline: false,
            normalize_eol: EolMode::Keep,
            trim_trailing_whitespace: false,
        }
    }
}

/// If `content` is non-empty and does not already end with `\n`, append one.
/// Empty content is returned unchanged.
pub fn ensure_final_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_owned()
    } else {
        let mut s = content.to_owned();
        s.push('\n');
        s
    }
}

/// Normalize line endings according to `mode`.
///
/// - `Keep`  – return content unchanged.
/// - `Lf`    – replace every `\r\n` with `\n`.
/// - `Crlf`  – replace every lone `\n` (not preceded by `\r`) with `\r\n`.
pub fn normalize_eol(content: &str, mode: EolMode) -> String {
    match mode {
        EolMode::Keep => content.to_owned(),
        EolMode::Lf => content.replace("\r\n", "\n"),
        EolMode::Crlf => {
            // First normalise everything to LF, then expand to CRLF.
            let lf_only = content.replace("\r\n", "\n");
            lf_only.replace('\n', "\r\n")
        }
    }
}

/// Remove trailing spaces and tabs from every line, preserving line endings.
pub fn trim_trailing_whitespace(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;

    while !rest.is_empty() {
        // Find the next line-ending sequence.
        if let Some(pos) = rest.find('\n') {
            // Check for CRLF.
            let (line, ending, advance) = if pos > 0 && rest.as_bytes()[pos - 1] == b'\r' {
                (&rest[..pos - 1], "\r\n", pos + 1)
            } else {
                (&rest[..pos], "\n", pos + 1)
            };
            result.push_str(line.trim_end_matches([' ', '\t']));
            result.push_str(ending);
            rest = &rest[advance..];
        } else {
            // Last line without a trailing newline.
            result.push_str(rest.trim_end_matches([' ', '\t']));
            break;
        }
    }

    result
}

/// Apply a [`WritePolicy`] to `content`: trim, then EOL normalise, then final newline.
pub fn apply_policy(content: &str, policy: &WritePolicy) -> String {
    let mut s = content.to_owned();

    if policy.trim_trailing_whitespace {
        s = trim_trailing_whitespace(&s);
    }

    s = normalize_eol(&s, policy.normalize_eol);

    if policy.ensure_final_newline {
        s = ensure_final_newline(&s);
    }

    s
}

/// Atomically write `content` to `path` after applying `policy`.
///
/// A temporary file is created in the same directory as `path`, written to, then
/// renamed over `path`. This guarantees the target file is never in a
/// partially-written state (assuming the same filesystem).
pub fn atomic_write(path: &Path, content: &str, policy: &WritePolicy) -> anyhow::Result<()> {
    let final_content = apply_policy(content, policy);

    let parent = path
        .parent()
        .context("cannot determine parent directory of target path")?;

    // Create a named tempfile in the same directory so the rename is atomic.
    let tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;

    std::fs::write(tmp.path(), final_content.as_bytes())
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;

    tmp.persist(path)
        .with_context(|| format!("failed to persist tempfile to {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ensure_final_newline_adds_when_missing() {
        assert_eq!(ensure_final_newline("hello"), "hello\n");
    }

    #[test]
    fn ensure_final_newline_empty_stays_empty() {
        assert_eq!(ensure_final_newline(""), "");
    }

    #[test]
    fn ensure_final_newline_no_double_add() {
        assert_eq!(ensure_final_newline("hello\n"), "hello\n");
    }

    #[test]
    fn normalize_eol_lf_converts_crlf() {
        assert_eq!(normalize_eol("a\r\nb\r\n", EolMode::Lf), "a\nb\n");
    }

    #[test]
    fn normalize_eol_crlf_converts_lf() {
        assert_eq!(normalize_eol("a\nb\n", EolMode::Crlf), "a\r\nb\r\n");
    }

    #[test]
    fn normalize_eol_keep_unchanged() {
        let content = "a\r\nb\nc\n";
        assert_eq!(normalize_eol(content, EolMode::Keep), content);
    }

    #[test]
    fn trim_trailing_whitespace_removes_spaces() {
        assert_eq!(
            trim_trailing_whitespace("hello   \nworld\t\n"),
            "hello\nworld\n"
        );
    }

    #[test]
    fn apply_policy_chains_all() {
        let policy = WritePolicy {
            trim_trailing_whitespace: true,
            normalize_eol: EolMode::Lf,
            ensure_final_newline: true,
        };
        // Trailing whitespace, CRLF endings, no final newline.
        let input = "hello  \r\nworld\t\r\n";
        let result = apply_policy(input, &policy);
        // After trim: "hello\r\nworld\r\n"
        // After LF:   "hello\nworld\n"
        // After final newline: already ends with \n → unchanged.
        assert_eq!(result, "hello\nworld\n");
    }

    #[test]
    fn atomic_write_writes_correct_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("output.txt");

        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Lf,
            trim_trailing_whitespace: true,
        };

        atomic_write(&target, "foo  \r\nbar", &policy).unwrap();

        let got = fs::read_to_string(&target).unwrap();
        assert_eq!(got, "foo\nbar\n");
    }
}
