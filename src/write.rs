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

impl WritePolicy {
    /// Returns `true` when no transformations are configured, meaning content
    /// can be written (or moved) byte-for-byte without interpretation.
    pub fn is_noop(&self) -> bool {
        !self.ensure_final_newline
            && matches!(self.normalize_eol, EolMode::Keep)
            && !self.trim_trailing_whitespace
    }
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
///
/// Returns `Cow::Borrowed` when no change is needed, avoiding allocation.
pub fn ensure_final_newline(content: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if content.is_empty() || content.ends_with('\n') {
        Cow::Borrowed(content)
    } else {
        let mut s = String::with_capacity(content.len() + 1);
        s.push_str(content);
        s.push('\n');
        Cow::Owned(s)
    }
}

/// Normalize line endings according to `mode`.
///
/// - `Keep`  – return content unchanged.
/// - `Lf`    – replace every `\r\n` with `\n`.
/// - `Crlf`  – replace every lone `\n` (not preceded by `\r`) with `\r\n`.
///
/// Returns `Cow::Borrowed` when no change is needed, avoiding allocation.
/// CRLF mode uses a single-pass scan with `memchr` instead of two
/// `.replace()` calls.
pub fn normalize_eol(content: &str, mode: EolMode) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match mode {
        EolMode::Keep => Cow::Borrowed(content),
        EolMode::Lf => {
            if memchr::memmem::find(content.as_bytes(), b"\r\n").is_none() {
                Cow::Borrowed(content)
            } else {
                Cow::Owned(content.replace("\r\n", "\n"))
            }
        }
        EolMode::Crlf => {
            let bytes = content.as_bytes();
            let has_bare_lf =
                memchr::memchr_iter(b'\n', bytes).any(|i| i == 0 || bytes[i - 1] != b'\r');
            if !has_bare_lf {
                Cow::Borrowed(content)
            } else {
                // Single-pass: copy slices between \n positions, inserting
                // \r before bare \n characters.
                let mut result = String::with_capacity(content.len() + content.len() / 10);
                let mut last = 0;
                for i in memchr::memchr_iter(b'\n', bytes) {
                    if i == 0 || bytes[i - 1] != b'\r' {
                        result.push_str(&content[last..i]);
                        result.push_str("\r\n");
                    } else {
                        result.push_str(&content[last..=i]);
                    }
                    last = i + 1;
                }
                if last < content.len() {
                    result.push_str(&content[last..]);
                }
                Cow::Owned(result)
            }
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
///
/// Returns `Cow::Borrowed` when the policy is a no-op, avoiding allocation.
pub fn apply_policy<'a>(content: &'a str, policy: &WritePolicy) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;

    if policy.is_noop() {
        return Cow::Borrowed(content);
    }

    let mut s = if policy.trim_trailing_whitespace {
        Cow::Owned(trim_trailing_whitespace(content))
    } else {
        Cow::Borrowed(content)
    };

    if !matches!(policy.normalize_eol, EolMode::Keep)
        && let Cow::Owned(new) = normalize_eol(&s, policy.normalize_eol)
    {
        s = Cow::Owned(new);
    }

    if policy.ensure_final_newline
        && let Cow::Owned(new) = ensure_final_newline(&s)
    {
        s = Cow::Owned(new);
    }

    s
}

/// Build a [`WritePolicy`] from [`GlobalFlags`], optionally merging
/// EditorConfig properties for the given file path.
///
/// Explicit CLI flags always win.  When `--respect-editorconfig` is set and
/// `file_path` is provided, EditorConfig values fill in any flag that was not
/// explicitly set by the user.
pub fn policy_from_flags(
    global: &crate::cli::global::GlobalFlags,
    file_path: Option<&std::path::Path>,
) -> WritePolicy {
    let mut efn = global.ensure_final_newline;
    let mut eol = global.normalize_eol.map(|m| match m {
        EolMode::Keep => EolMode::Keep,
        EolMode::Lf => EolMode::Lf,
        EolMode::Crlf => EolMode::Crlf,
    });
    let mut ttw = global.trim_trailing_whitespace;

    if global.respect_editorconfig
        && let Some(path) = file_path
        && let Ok(props) = ec4rs::properties_of(path)
    {
        // insert_final_newline
        if !global.ensure_final_newline
            && let Ok(ec4rs::property::FinalNewline::Value(true)) =
                props.get::<ec4rs::property::FinalNewline>()
        {
            efn = true;
        }

        // end_of_line
        if global.normalize_eol.is_none()
            && let Ok(val) = props.get::<ec4rs::property::EndOfLine>()
        {
            eol = Some(match val {
                ec4rs::property::EndOfLine::Lf => EolMode::Lf,
                ec4rs::property::EndOfLine::CrLf => EolMode::Crlf,
                ec4rs::property::EndOfLine::Cr => EolMode::Keep,
            });
        }

        // trim_trailing_whitespace
        if !global.trim_trailing_whitespace
            && let Ok(ec4rs::property::TrimTrailingWs::Value(true)) =
                props.get::<ec4rs::property::TrimTrailingWs>()
        {
            ttw = true;
        }
    }

    WritePolicy {
        ensure_final_newline: efn,
        normalize_eol: eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: ttw,
    }
}

/// Create a new file at `path` after applying `policy`, failing if the file
/// already exists.
///
/// Uses `File::create_new` (O_CREAT | O_EXCL) so the existence check and
/// creation happen in a single syscall, eliminating the TOCTOU race inherent
/// in a separate `path.exists()` + write sequence.
pub fn atomic_create_new(path: &Path, content: &str, policy: &WritePolicy) -> anyhow::Result<()> {
    use std::io::Write;
    let final_content = apply_policy(content, policy);
    let mut file = std::fs::File::create_new(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::anyhow!("file already exists: {}", path.display())
        } else {
            anyhow::Error::from(e).context(format!("failed to create {}", path.display()))
        }
    })?;
    file.write_all(final_content.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Atomically write `content` to `path` after applying `policy`.
///
/// A temporary file is created in the same directory as `path`, written to, then
/// renamed over `path`. This guarantees the target file is never in a
/// partially-written state (assuming the same filesystem).
pub fn atomic_write(path: &Path, content: &str, policy: &WritePolicy) -> anyhow::Result<()> {
    let final_content = apply_policy(content, policy);

    // Capture the original file's permissions before overwriting.
    let original_perms = std::fs::metadata(path).ok().map(|m| m.permissions());

    let parent = path
        .parent()
        .context("cannot determine parent directory of target path")?;

    // Create a named tempfile in the same directory so the rename is atomic.
    let tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;

    std::fs::write(tmp.path(), final_content.as_bytes())
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;

    // Restore the original permissions on the temp file before renaming.
    if let Some(perms) = original_perms {
        std::fs::set_permissions(tmp.path(), perms)
            .with_context(|| format!("failed to set permissions on {}", tmp.path().display()))?;
    }

    tmp.persist(path)
        .with_context(|| format!("failed to persist tempfile to {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
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
    fn noop_policy_returns_borrowed() {
        let policy = WritePolicy::default();
        let input = "hello\nworld\n";
        let result = apply_policy(input, &policy);
        assert!(
            matches!(result, std::borrow::Cow::Borrowed(_)),
            "no-op policy should return Cow::Borrowed"
        );
        assert_eq!(&*result, input);
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

    fn test_global_flags() -> GlobalFlags {
        GlobalFlags::default()
    }

    #[test]
    fn policy_from_flags_explicit_flags_win() {
        let dir = tempfile::tempdir().unwrap();
        let ec_path = dir.path().join(".editorconfig");
        fs::write(
            &ec_path,
            "root = true\n\n[*]\ninsert_final_newline = false\nend_of_line = crlf\ntrim_trailing_whitespace = false\n",
        )
        .unwrap();

        let file = dir.path().join("test.txt");
        fs::write(&file, "content\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;
        global.ensure_final_newline = true;
        global.normalize_eol = Some(EolMode::Lf);
        global.trim_trailing_whitespace = true;

        let policy = policy_from_flags(&global, Some(&file));
        // Explicit flags should win over EditorConfig values.
        assert!(policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
        assert!(policy.trim_trailing_whitespace);
    }

    #[test]
    fn policy_from_flags_editorconfig_provides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let ec_path = dir.path().join(".editorconfig");
        fs::write(
            &ec_path,
            "root = true\n\n[*]\ninsert_final_newline = true\nend_of_line = lf\ntrim_trailing_whitespace = true\n",
        )
        .unwrap();

        let file = dir.path().join("test.txt");
        fs::write(&file, "content\n").unwrap();

        let mut global = test_global_flags();
        global.respect_editorconfig = true;

        let policy = policy_from_flags(&global, Some(&file));
        assert!(policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Lf));
        assert!(policy.trim_trailing_whitespace);
    }

    #[test]
    #[cfg(unix)]
    fn atomic_write_preserves_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("script.sh");
        fs::write(&target, "#!/bin/sh\necho old\n").unwrap();

        // Set executable permission (0o755).
        fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();

        let policy = WritePolicy::default();
        atomic_write(&target, "#!/bin/sh\necho new\n", &policy).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o755,
            "permissions should be preserved after atomic_write"
        );
    }

    #[test]
    fn noop_policy_detected() {
        assert!(WritePolicy::default().is_noop());
    }

    #[test]
    fn non_noop_policy_detected() {
        let p = WritePolicy {
            ensure_final_newline: true,
            ..Default::default()
        };
        assert!(!p.is_noop());

        let p2 = WritePolicy {
            normalize_eol: EolMode::Lf,
            ..Default::default()
        };
        assert!(!p2.is_noop());

        let p3 = WritePolicy {
            trim_trailing_whitespace: true,
            ..Default::default()
        };
        assert!(!p3.is_noop());
    }

    #[test]
    fn atomic_create_new_writes_content() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.txt");

        let policy = WritePolicy {
            ensure_final_newline: true,
            normalize_eol: EolMode::Lf,
            trim_trailing_whitespace: false,
        };

        atomic_create_new(&target, "hello", &policy).unwrap();
        let got = fs::read_to_string(&target).unwrap();
        assert_eq!(got, "hello\n");
    }

    #[test]
    fn atomic_create_new_fails_if_exists() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.txt");
        fs::write(&target, "old").unwrap();

        let policy = WritePolicy::default();
        let err = atomic_create_new(&target, "new", &policy).unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "error should mention 'already exists': {err}"
        );
    }

    #[test]
    fn policy_from_flags_no_editorconfig_uses_defaults() {
        let global = test_global_flags();

        let policy = policy_from_flags(&global, None);
        assert!(!policy.ensure_final_newline);
        assert!(matches!(policy.normalize_eol, EolMode::Keep));
        assert!(!policy.trim_trailing_whitespace);
    }
}
