//! Atomic write, final newline, EOL normalization, trailing-whitespace trimming.

use std::path::Path;

use anyhow::Context;
use tempfile::NamedTempFile;

/// Line ending normalization mode.
///
/// This type is re-exported at the crate root level of `write` for library use
/// (independent of the optional `cli` feature).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
pub enum EolMode {
    /// Keep existing line endings.
    #[default]
    Keep,
    /// Normalize to LF.
    Lf,
    /// Normalize to CRLF.
    Crlf,
    /// Normalize to CR (classic Mac).
    Cr,
}

/// Controls which transformations are applied before writing a file.
pub struct WritePolicy {
    pub ensure_final_newline: bool,
    pub normalize_eol: EolMode,
    pub trim_trailing_whitespace: bool,
    pub collapse_blanks: bool,
}

impl WritePolicy {
    /// Returns `true` when no transformations are configured, meaning content
    /// can be written (or moved) byte-for-byte without interpretation.
    pub fn is_noop(&self) -> bool {
        !self.ensure_final_newline
            && matches!(self.normalize_eol, EolMode::Keep)
            && !self.trim_trailing_whitespace
            && !self.collapse_blanks
    }

    /// Apply an override, setting only the fields that are `Some`.
    pub fn apply_override(&mut self, ov: &WritePolicyOverride) -> anyhow::Result<()> {
        if let Some(v) = ov.ensure_final_newline {
            self.ensure_final_newline = v;
        }
        if let Some(ref s) = ov.normalize_eol {
            self.normalize_eol = parse_eol_mode(s)?;
        }
        if let Some(v) = ov.trim_trailing_whitespace {
            self.trim_trailing_whitespace = v;
        }
        if let Some(v) = ov.collapse_blanks {
            self.collapse_blanks = v;
        }
        Ok(())
    }
}

impl Default for WritePolicy {
    fn default() -> Self {
        Self {
            ensure_final_newline: false,
            normalize_eol: EolMode::Keep,
            trim_trailing_whitespace: false,
            collapse_blanks: false,
        }
    }
}

/// Partially-specified write policy for config files, plans, and overrides.
///
/// Each field is optional; only `Some` values override the base [`WritePolicy`].
/// Used by `.patchloom.toml` config, tx plan `write_policy`, and any context
/// that needs to partially override write transformations.
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct WritePolicyOverride {
    pub ensure_final_newline: Option<bool>,
    pub normalize_eol: Option<String>,
    pub trim_trailing_whitespace: Option<bool>,
    pub collapse_blanks: Option<bool>,
    pub respect_editorconfig: Option<bool>,
}

/// Parse a string EOL mode value into [`EolMode`].
pub fn parse_eol_mode(mode: &str) -> anyhow::Result<EolMode> {
    match mode {
        "lf" => Ok(EolMode::Lf),
        "crlf" => Ok(EolMode::Crlf),
        "cr" => Ok(EolMode::Cr),
        "keep" => Ok(EolMode::Keep),
        _ => {
            anyhow::bail!(
                "invalid normalize_eol value '{mode}': expected 'lf', 'crlf', 'cr', or 'keep'"
            )
        }
    }
}

/// If `content` is non-empty and does not already end with the appropriate
/// line terminator for `eol`, append one.  Empty content is returned unchanged.
///
/// The terminator depends on `eol`:
/// - `Lf` / `Keep` => `\n`
/// - `Crlf`        => `\r\n`
/// - `Cr`          => `\r`
///
/// Returns `Cow::Borrowed` when no change is needed, avoiding allocation.
pub fn ensure_final_newline(content: &str, eol: EolMode) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    let (suffix, already_ok) = match eol {
        EolMode::Cr => ("\r", content.ends_with('\r')),
        EolMode::Crlf => ("\r\n", content.ends_with("\r\n")),
        EolMode::Lf | EolMode::Keep => ("\n", content.ends_with('\n')),
    };
    if content.is_empty() || already_ok {
        Cow::Borrowed(content)
    } else {
        let mut s = String::with_capacity(content.len() + suffix.len());
        s.push_str(content);
        s.push_str(suffix);
        Cow::Owned(s)
    }
}

/// Normalize line endings according to `mode`.
///
/// - `Keep`  – return content unchanged.
/// - `Lf`    – replace every `\r\n` with `\n`, and every bare `\r` with `\n`.
/// - `Crlf`  – replace every lone `\n` (not preceded by `\r`) with `\r\n`.
/// - `Cr`    – replace every `\r\n` with `\r`, and every bare `\n` with `\r`.
///
/// Returns `Cow::Borrowed` when no change is needed, avoiding allocation.
/// CRLF mode uses a single-pass scan with `memchr` instead of two
/// `.replace()` calls.
pub fn normalize_eol(content: &str, mode: EolMode) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match mode {
        EolMode::Keep => Cow::Borrowed(content),
        EolMode::Lf => {
            let bytes = content.as_bytes();
            let has_cr = memchr::memchr(b'\r', bytes).is_some();
            if !has_cr {
                Cow::Borrowed(content)
            } else {
                // Replace \r\n with \n, then any remaining bare \r with \n.
                let without_crlf = content.replace("\r\n", "\n");
                Cow::Owned(without_crlf.replace('\r', "\n"))
            }
        }
        EolMode::Crlf => {
            let bytes = content.as_bytes();
            // Check for bare \r (not followed by \n) in addition to bare \n.
            let has_bare_cr = memchr::memchr_iter(b'\r', bytes)
                .any(|i| i + 1 >= bytes.len() || bytes[i + 1] != b'\n');
            if has_bare_cr {
                // Normalize all line endings to \n first, then convert to \r\n.
                let lf_first = content.replace("\r\n", "\n").replace('\r', "\n");
                Cow::Owned(lf_first.replace('\n', "\r\n"))
            } else {
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
        EolMode::Cr => {
            let bytes = content.as_bytes();
            let has_lf = memchr::memchr(b'\n', bytes).is_some();
            if !has_lf {
                Cow::Borrowed(content)
            } else {
                // Replace \r\n with \r, then any remaining bare \n with \r.
                let without_crlf = content.replace("\r\n", "\r");
                Cow::Owned(without_crlf.replace('\n', "\r"))
            }
        }
    }
}

/// Remove trailing spaces and tabs from every line, preserving line endings.
///
/// Returns `Cow::Borrowed` when no trailing whitespace exists, avoiding
/// allocation in the common case of clean files.
pub fn trim_trailing_whitespace(content: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    // Fast check: scan for any trailing whitespace before a newline or at EOF.
    let bytes = content.as_bytes();
    let has_trailing = memchr::memchr2_iter(b'\r', b'\n', bytes).any(|i| {
        // Skip the \n of a \r\n pair (already handled at the \r position).
        if bytes[i] == b'\n' && i > 0 && bytes[i - 1] == b'\r' {
            return false;
        }
        let prev = i.wrapping_sub(1);
        prev < bytes.len() && matches!(bytes[prev], b' ' | b'\t')
    }) || (!content.is_empty()
        && !matches!(bytes[bytes.len() - 1], b'\r' | b'\n')
        && matches!(bytes[bytes.len() - 1], b' ' | b'\t'));

    if !has_trailing {
        return Cow::Borrowed(content);
    }

    let mut result = String::with_capacity(content.len());
    let mut rest = content;

    while !rest.is_empty() {
        // Find the next line-ending sequence (\n, \r\n, or bare \r).
        let rest_bytes = rest.as_bytes();
        if let Some(pos) = memchr::memchr2(b'\r', b'\n', rest_bytes) {
            let (line, ending, advance) = if rest_bytes[pos] == b'\n' {
                (&rest[..pos], "\n", pos + 1)
            } else if pos + 1 < rest_bytes.len() && rest_bytes[pos + 1] == b'\n' {
                (&rest[..pos], "\r\n", pos + 2)
            } else {
                (&rest[..pos], "\r", pos + 1)
            };
            result.push_str(line.trim_end_matches([' ', '\t']));
            result.push_str(ending);
            rest = &rest[advance..];
        } else {
            // Last line without a trailing line ending.
            result.push_str(rest.trim_end_matches([' ', '\t']));
            break;
        }
    }

    Cow::Owned(result)
}

/// Collapse consecutive blank lines into a single blank line.
///
/// A blank line is one that contains only whitespace. Two or more consecutive
/// blank lines are reduced to one. Returns `Cow::Borrowed` when no collapsing
/// is needed, avoiding allocation.
pub fn collapse_blanks(content: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    let bytes = content.as_bytes();
    // Quick scan: look for two consecutive line endings with only whitespace between.
    let mut prev_blank = false;
    let mut needs_collapse = false;
    let mut scan = content.as_bytes();
    while let Some(pos) = memchr::memchr2(b'\r', b'\n', scan) {
        let end = if scan[pos] == b'\n' {
            pos + 1
        } else if pos + 1 < scan.len() && scan[pos + 1] == b'\n' {
            pos + 2
        } else {
            pos + 1
        };
        let line = &content[content.len() - scan.len()..content.len() - scan.len() + pos];
        let is_blank = line.trim().is_empty();
        if is_blank && prev_blank {
            needs_collapse = true;
            break;
        }
        prev_blank = is_blank;
        scan = &scan[end..];
    }

    // Check trailing content after the last newline: if the remainder is
    // blank and the previous line was also blank, we still need to collapse.
    if !needs_collapse && prev_blank && !scan.is_empty() {
        let trailing = std::str::from_utf8(scan).unwrap_or("");
        if trailing.trim().is_empty() {
            needs_collapse = true;
        }
    }

    if !needs_collapse {
        return Cow::Borrowed(content);
    }

    let mut result = String::with_capacity(bytes.len());
    let mut prev_blank = false;
    let mut rest = content;

    while !rest.is_empty() {
        let rest_bytes = rest.as_bytes();
        if let Some(pos) = memchr::memchr2(b'\r', b'\n', rest_bytes) {
            let end = if rest_bytes[pos] == b'\n' {
                pos + 1
            } else if pos + 1 < rest_bytes.len() && rest_bytes[pos + 1] == b'\n' {
                pos + 2
            } else {
                pos + 1
            };
            let line_content = &rest[..pos];
            let line_with_ending = &rest[..end];
            let is_blank = line_content.trim().is_empty();
            if is_blank && prev_blank {
                // Skip this consecutive blank line.
            } else {
                result.push_str(line_with_ending);
            }
            prev_blank = is_blank;
            rest = &rest[end..];
        } else {
            // Last line without trailing newline.
            let is_blank = rest.trim().is_empty();
            if !(is_blank && prev_blank) {
                result.push_str(rest);
            }
            break;
        }
    }

    Cow::Owned(result)
}

/// Dedent content by removing leading whitespace.
///
/// `spec` accepts:
/// - A numeric string (e.g. `"4"`) — remove up to N leading spaces per line.
/// - `"tab"` — remove one leading tab per line.
/// - `"auto"` — find the minimum non-zero indentation and remove that much.
///
/// If `line_range` is `Some((start, end))`, only lines in that 1-based inclusive
/// range are affected. Blank lines are never modified.
pub fn dedent_content(
    content: &str,
    spec: &str,
    line_range: Option<(usize, Option<usize>)>,
) -> String {
    let lines: Vec<&str> = content.split('\n').collect();

    let (start, end) = match line_range {
        Some((s, e)) => (s.max(1), e.unwrap_or(lines.len())),
        None => (1, lines.len()),
    };

    let in_range = |i: usize| {
        let line_num = i + 1; // 1-based
        line_num >= start && line_num <= end
    };

    match spec {
        "auto" => {
            // Find minimum non-zero indentation in the range.
            let min_indent = lines
                .iter()
                .enumerate()
                .filter(|&(i, _)| in_range(i))
                .filter(|&(_, line)| !line.trim().is_empty())
                .map(|(_, line)| line.len() - line.trim_start().len())
                .filter(|&n| n > 0)
                .min()
                .unwrap_or(0);

            if min_indent == 0 {
                return content.to_string();
            }

            dedent_by_n(&lines, min_indent, &in_range)
        }
        "tab" => {
            let result: Vec<String> = lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    if !in_range(i) || line.trim().is_empty() {
                        line.to_string()
                    } else if let Some(rest) = line.strip_prefix('\t') {
                        rest.to_string()
                    } else {
                        line.to_string()
                    }
                })
                .collect();
            result.join("\n")
        }
        n => {
            let count: usize = n.parse().unwrap_or(0);
            if count == 0 {
                return content.to_string();
            }
            dedent_by_n(&lines, count, &in_range)
        }
    }
}

fn dedent_by_n(lines: &[&str], n: usize, in_range: &dyn Fn(usize) -> bool) -> String {
    let result: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if !in_range(i) || line.trim().is_empty() {
                line.to_string()
            } else {
                let leading_spaces = line.len() - line.trim_start().len();
                let strip = n.min(leading_spaces);
                line[strip..].to_string()
            }
        })
        .collect();
    result.join("\n")
}

/// Parse a line range specification like `"10:50"` into `(start, Option<end>)`.
///
/// Both start and end are 1-based inclusive. Examples:
/// - `"10:50"` → `(10, Some(50))`
/// - `"5:"` → `(5, None)` (open-ended)
/// - `"3"` → `(3, Some(3))` (single line)
pub fn parse_line_range(spec: &str) -> anyhow::Result<(usize, Option<usize>)> {
    if let Some((left, right)) = spec.split_once(':') {
        let start: usize = left
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid line range start: '{left}'"))?;
        if start == 0 {
            anyhow::bail!("line numbers are 1-based, got start=0");
        }
        if right.is_empty() {
            Ok((start, None))
        } else {
            let end: usize = right
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid line range end: '{right}'"))?;
            if end == 0 {
                anyhow::bail!("line numbers are 1-based, got end=0");
            }
            if end < start {
                anyhow::bail!("inverted line range: start ({start}) > end ({end})");
            }
            Ok((start, Some(end)))
        }
    } else {
        let line: usize = spec
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid line number: '{spec}'"))?;
        if line == 0 {
            anyhow::bail!("line numbers are 1-based, got 0");
        }
        Ok((line, Some(line)))
    }
}

/// Indent content by adding leading whitespace.
///
/// `spec` accepts:
/// - A numeric string (e.g. `"4"`) — add N leading spaces to each non-blank line.
/// - `"tab"` — add one leading tab to each non-blank line.
///
/// If `line_range` is `Some((start, end))`, only lines in that 1-based inclusive
/// range are affected. Blank lines are never modified.
pub fn indent_content(
    content: &str,
    spec: &str,
    line_range: Option<(usize, Option<usize>)>,
) -> String {
    let lines: Vec<&str> = content.split('\n').collect();

    let (start, end) = match line_range {
        Some((s, e)) => (s.max(1), e.unwrap_or(lines.len())),
        None => (1, lines.len()),
    };

    let prefix = match spec {
        "tab" => "\t".to_string(),
        n => {
            let count: usize = n.parse().unwrap_or(0);
            " ".repeat(count)
        }
    };

    if prefix.is_empty() {
        return content.to_string();
    }

    let result: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_num = i + 1;
            if line_num < start || line_num > end || line.trim().is_empty() {
                line.to_string()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect();
    result.join("\n")
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
        trim_trailing_whitespace(content)
    } else {
        Cow::Borrowed(content)
    };

    if !matches!(policy.normalize_eol, EolMode::Keep)
        && let Cow::Owned(new) = normalize_eol(&s, policy.normalize_eol)
    {
        s = Cow::Owned(new);
    }

    if policy.collapse_blanks
        && let Cow::Owned(new) = collapse_blanks(&s)
    {
        s = Cow::Owned(new);
    }

    if policy.ensure_final_newline
        && let Cow::Owned(new) = ensure_final_newline(&s, policy.normalize_eol)
    {
        s = Cow::Owned(new);
    }

    s
}

/// Build a [`WritePolicy`] from [`GlobalFlags`](crate::cli::global::GlobalFlags), optionally merging
/// EditorConfig properties for the given file path.
///
/// Explicit CLI flags always win.  When `--respect-editorconfig` is set and
/// `file_path` is provided, EditorConfig values fill in any flag that was not
/// explicitly set by the user.
///
/// Build WritePolicy from GlobalFlags (and EditorConfig if respect_editorconfig).
/// Usable from library tx execution paths as well (GlobalFlags can be simulated).
pub fn policy_from_flags(
    global: &crate::cli::global::GlobalFlags,
    #[allow(unused_variables)] file_path: Option<&std::path::Path>,
) -> WritePolicy {
    let efn = global.ensure_final_newline;
    let eol = global.normalize_eol;
    let ttw = global.trim_trailing_whitespace;

    let respect_ec = if cfg!(feature = "cli") {
        global.respect_editorconfig
    } else {
        false
    };

    let (efn, eol, ttw) = if respect_ec {
        #[cfg(feature = "cli")]
        if let Some(p) = file_path {
            #[allow(unused_variables)]
            if let Ok(props) = ec4rs::properties_of(p) {
                let mut new_efn = efn;
                let mut new_eol = eol;
                let mut new_ttw = ttw;

                // insert_final_newline
                if !global.ensure_final_newline
                    && let Ok(ec4rs::property::FinalNewline::Value(true)) =
                        props.get::<ec4rs::property::FinalNewline>()
                {
                    new_efn = true;
                }

                // end_of_line
                if global.normalize_eol.is_none()
                    && let Ok(val) = props.get::<ec4rs::property::EndOfLine>()
                {
                    new_eol = Some(match val {
                        ec4rs::property::EndOfLine::Lf => EolMode::Lf,
                        ec4rs::property::EndOfLine::CrLf => EolMode::Crlf,
                        ec4rs::property::EndOfLine::Cr => EolMode::Cr,
                    });
                }

                // trim_trailing_whitespace
                if !global.trim_trailing_whitespace
                    && let Ok(ec4rs::property::TrimTrailingWs::Value(true)) =
                        props.get::<ec4rs::property::TrimTrailingWs>()
                {
                    new_ttw = true;
                }

                (new_efn, new_eol, new_ttw)
            } else {
                (efn, eol, ttw)
            }
        } else {
            (efn, eol, ttw)
        }
        #[cfg(not(feature = "cli"))]
        {
            (efn, eol, ttw)
        }
    } else {
        (efn, eol, ttw)
    };

    WritePolicy {
        ensure_final_newline: efn,
        normalize_eol: eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: ttw,
        collapse_blanks: global.collapse_blanks,
    }
}

/// Create a new file at `path` after applying `policy`, failing if the file
/// already exists.
///
/// Uses `tempfile::NamedTempFile` + `persist_noclobber` so the write is
/// crash-safe. The target file either has full content or does not exist;
/// partial content is never visible. `persist_noclobber` uses
/// `link` + `unlink` (or platform equivalent) to fail if the target already
/// exists, preserving the exclusive-create semantics.
pub fn atomic_create_new(path: &Path, content: &str, policy: &WritePolicy) -> anyhow::Result<()> {
    let final_content = apply_policy(content, policy);

    let parent = path
        .parent()
        .context("cannot determine parent directory of target path")?;

    let tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;

    std::fs::write(tmp.path(), final_content.as_bytes())
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;

    tmp.persist_noclobber(path).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::anyhow!("file already exists: {}", path.display())
        } else {
            anyhow::Error::from(e.error)
                .context(format!("failed to persist tempfile to {}", path.display()))
        }
    })?;

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
    // Use symlink_metadata so that when `path` is a symlink we get the
    // symlink entry's metadata, not the target's. persist() replaces the
    // path entry (the symlink) with a regular file, so carrying over the
    // target's permissions would be wrong.
    let original_perms = std::fs::symlink_metadata(path)
        .ok()
        .filter(|m| !m.file_type().is_symlink())
        .map(|m| m.permissions());

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

/// Run a post-write format command if configured.
///
/// Checks `global.no_format` first (skips all formatting).
/// If `global.format` is set (via CLI `--format` or `defaults.format`),
/// runs that single command on the cwd.
/// Otherwise, if a `FormatConfig` with `by_extension` entries is provided,
/// runs per-extension formatters on the modified files.
///
/// Formatter errors are reported to stderr but do **not** cause a bail.
/// This matches the design principle that formatting is advisory: the
/// write operation already succeeded and the files are correct, just
/// not yet formatted.
#[cfg(feature = "cli")]
pub fn run_format_command(
    global: &crate::cli::global::GlobalFlags,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    run_format_command_ext(global, cwd, None, None)
}

/// Extended format runner that accepts an optional list of modified paths
/// and an optional format configuration from `.patchloom.toml`.
///
/// `modified_paths` are relative paths (as stored by the tx engine).
/// `format_config` provides per-extension formatter commands.
#[cfg(feature = "cli")]
pub fn run_format_command_ext(
    global: &crate::cli::global::GlobalFlags,
    cwd: &std::path::Path,
    modified_paths: Option<&[&str]>,
    format_config: Option<&crate::config::FormatConfig>,
) -> anyhow::Result<()> {
    if global.no_format {
        return Ok(());
    }

    let timeout_secs = global.format_timeout.unwrap_or(30);

    // Priority 1: explicit --format command (whole-project)
    if let Some(cmd) = global.format.as_deref() {
        let result = crate::exec::run_with_timeout(cmd, timeout_secs, cwd)?;
        if !result.status.success() {
            let stderr = if result.stderr_head.is_empty() {
                String::new()
            } else {
                format!(": {}", result.stderr_head)
            };
            anyhow::bail!("format command failed ({}){stderr}", cmd);
        }
        return Ok(());
    }

    // Priority 2: format config from .patchloom.toml
    let config = match format_config {
        Some(c) => c,
        None => return Ok(()),
    };

    // Check auto flag: if auto is not true and no explicit --format, skip
    if config.auto != Some(true) {
        return Ok(());
    }

    // Priority 2a: catch-all command
    if let Some(ref cmd) = config.command {
        let result = crate::exec::run_with_timeout(cmd, timeout_secs, cwd)?;
        if !result.status.success() {
            eprintln!(
                "warning: format command failed ({}): {}",
                cmd,
                result.stderr_head.trim()
            );
        }
        return Ok(());
    }

    // Priority 2b: per-extension formatters
    if config.by_extension.is_empty() {
        return Ok(());
    }

    let paths = match modified_paths {
        Some(p) => p,
        None => return Ok(()),
    };

    // Group modified files by extension
    let mut by_ext: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for path in paths {
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            && config.by_extension.contains_key(ext)
        {
            by_ext.entry(ext).or_default().push(path);
        }
    }

    // Run formatter for each extension group
    for (ext, files) in &by_ext {
        if let Some(cmd_template) = config.by_extension.get(*ext) {
            for file in files {
                let cmd = format!("{cmd_template} {}", shell_escape(file));
                match crate::exec::run_with_timeout(&cmd, timeout_secs, cwd) {
                    Ok(result) if !result.status.success() => {
                        eprintln!(
                            "warning: formatter for .{ext} failed on {file}: {}",
                            result.stderr_head.trim()
                        );
                    }
                    Err(e) => {
                        eprintln!("warning: formatter for .{ext} error on {file}: {e}");
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// Shell-escape a file path for safe inclusion in a command string.
#[cfg(feature = "cli")]
fn shell_escape(path: &str) -> String {
    // If the path contains no special characters, return as-is
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'.' || b == b'_' || b == b'-')
    {
        return path.to_string();
    }
    // Otherwise, wrap in single quotes (escaping any embedded single quotes)
    format!("'{}'", path.replace('\'', "'\\''"))
}

#[path = "write_tests.rs"]
#[cfg(test)]
mod tests;
