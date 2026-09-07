//! Atomic write, final newline, EOL normalization, trailing-whitespace trimming.
//!
//! size-waiver: atomic write + hardlink + symlink + Windows named-stream persist (#1230 / #1733 / #1408 / #2341).

use std::path::Path;

use anyhow::Context;
use tempfile::NamedTempFile;

/// Detect the dominant line ending in `text`.
///
/// Returns `"\r\n"` when CRLF sequences are present and at least as common
/// as bare LF; otherwise returns `"\n"`.
///
/// This is the canonical implementation used by all AST and ops modules
/// to preserve the original file's line ending style during content
/// reconstruction.
pub fn detect_eol(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf_only = text.matches('\n').count().saturating_sub(crlf);
    let cr_only = text.matches('\r').count().saturating_sub(crlf);
    if crlf > 0 && crlf >= lf_only && crlf >= cr_only {
        "\r\n"
    } else if cr_only > 0 && cr_only >= lf_only {
        "\r"
    } else {
        "\n"
    }
}

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

/// EditorConfig `charset` handling for text writes.
///
/// Patchloom is a UTF-8 text tool. `utf-8` / `latin1` never insert a BOM;
/// `utf-8-bom` ensures a leading U+FEFF. UTF-16 variants are refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CharsetMode {
    /// Leave any existing BOM (or lack of one) unchanged.
    #[default]
    Keep,
    /// UTF-8 without BOM: strip a leading U+FEFF if present.
    Utf8,
    /// UTF-8 with BOM: ensure a leading U+FEFF.
    Utf8Bom,
    /// EditorConfig charset this process cannot apply (`utf-16le`, `utf-16be`).
    Unsupported(&'static str),
}

/// Controls which transformations are applied before writing a file.
pub struct WritePolicy {
    pub ensure_final_newline: bool,
    pub normalize_eol: EolMode,
    pub trim_trailing_whitespace: bool,
    pub collapse_blanks: bool,
    pub charset: CharsetMode,
}

impl WritePolicy {
    /// Returns `true` when no transformations are configured, meaning content
    /// can be written (or moved) byte-for-byte without interpretation.
    pub fn is_noop(&self) -> bool {
        !self.ensure_final_newline
            && matches!(self.normalize_eol, EolMode::Keep)
            && !self.trim_trailing_whitespace
            && !self.collapse_blanks
            && matches!(self.charset, CharsetMode::Keep)
    }

    /// EditorConfig charset this write cannot apply, if any.
    pub fn unsupported_charset(&self) -> Option<&'static str> {
        match self.charset {
            CharsetMode::Unsupported(name) => Some(name),
            _ => None,
        }
    }

    /// Fail closed when EditorConfig asks for a charset we cannot write.
    pub fn refuse_unsupported_charset(&self) -> anyhow::Result<()> {
        if let Some(name) = self.unsupported_charset() {
            return Err(crate::exit::InvalidInputError {
                msg: format!(
                    "editorconfig charset '{name}' is not supported; use utf-8 or utf-8-bom"
                ),
            }
            .into());
        }
        Ok(())
    }

    /// Apply an override, setting only the fields that are `Some`.
    /// Validates all fields before mutating to avoid partial state on error.
    pub fn apply_override(&mut self, ov: &WritePolicyOverride) -> anyhow::Result<()> {
        // Validate first: parse normalize_eol before touching any field.
        let parsed_eol = match ov.normalize_eol {
            Some(ref s) => Some(parse_eol_mode(s)?),
            None => None,
        };
        // All validation passed; now mutate.
        if let Some(v) = ov.ensure_final_newline {
            self.ensure_final_newline = v;
        }
        if let Some(eol) = parsed_eol {
            self.normalize_eol = eol;
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
            charset: CharsetMode::Keep,
        }
    }
}

/// Partially-specified write policy for config files, plans, and overrides.
///
/// Each field is optional; only `Some` values override the base [`WritePolicy`].
/// Used by `.patchloom.toml` config, tx plan `write_policy`, and any context
/// that needs to partially override write transformations.
#[derive(Debug, Default, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(default)]
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
        _ => Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "invalid normalize_eol value '{mode}': expected 'lf', 'crlf', 'cr', or 'keep'"
            ),
        })),
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
        EolMode::Lf => ("\n", content.ends_with('\n')),
        EolMode::Keep => {
            // Use the file's dominant line ending so we don't introduce
            // mixed endings (#1175).
            let detected = detect_eol(content);
            (detected, content.ends_with(detected))
        }
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

    // Charset before final newline so an empty file with utf-8-bom
    // becomes "\u{feff}" then gets the required trailing EOL in one pass.
    if let Cow::Owned(new) = apply_charset(&s, policy.charset) {
        s = Cow::Owned(new);
    }

    if policy.ensure_final_newline
        && let Cow::Owned(new) = ensure_final_newline(&s, policy.normalize_eol)
    {
        s = Cow::Owned(new);
    }

    s
}

/// Apply EditorConfig `charset` to UTF-8 text. Unsupported modes are a no-op
/// here; callers must [`WritePolicy::refuse_unsupported_charset`] first.
pub fn apply_charset(content: &str, mode: CharsetMode) -> std::borrow::Cow<'_, str> {
    const BOM: char = '\u{feff}';
    match mode {
        CharsetMode::Keep | CharsetMode::Unsupported(_) => std::borrow::Cow::Borrowed(content),
        CharsetMode::Utf8 => {
            if let Some(rest) = content.strip_prefix(BOM) {
                std::borrow::Cow::Owned(rest.to_string())
            } else {
                std::borrow::Cow::Borrowed(content)
            }
        }
        CharsetMode::Utf8Bom => {
            if content.starts_with(BOM) {
                std::borrow::Cow::Borrowed(content)
            } else {
                let mut out = String::with_capacity(content.len() + BOM.len_utf8());
                out.push(BOM);
                out.push_str(content);
                std::borrow::Cow::Owned(out)
            }
        }
    }
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

    let (efn, eol, ttw, charset) = if respect_ec {
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

                let charset = charset_from_editorconfig_props(&props);
                (new_efn, new_eol, new_ttw, charset)
            } else {
                (efn, eol, ttw, CharsetMode::Keep)
            }
        } else {
            (efn, eol, ttw, CharsetMode::Keep)
        }
        #[cfg(not(feature = "cli"))]
        {
            (efn, eol, ttw, CharsetMode::Keep)
        }
    } else {
        (efn, eol, ttw, CharsetMode::Keep)
    };

    WritePolicy {
        ensure_final_newline: efn,
        normalize_eol: eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: ttw,
        collapse_blanks: global.collapse_blanks,
        charset,
    }
}

#[cfg(feature = "cli")]
pub(crate) fn charset_from_editorconfig_props(props: &ec4rs::Properties) -> CharsetMode {
    match props.get::<ec4rs::property::Charset>() {
        Ok(ec4rs::property::Charset::Utf8) | Ok(ec4rs::property::Charset::Latin1) => {
            CharsetMode::Utf8
        }
        Ok(ec4rs::property::Charset::Utf8Bom) => CharsetMode::Utf8Bom,
        Ok(ec4rs::property::Charset::Utf16Le) => CharsetMode::Unsupported("utf-16le"),
        Ok(ec4rs::property::Charset::Utf16Be) => CharsetMode::Unsupported("utf-16be"),
        Err(_) => CharsetMode::Keep,
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
pub(crate) fn atomic_create_new(
    path: &Path,
    content: &str,
    policy: &WritePolicy,
) -> anyhow::Result<()> {
    policy.refuse_unsupported_charset()?;
    let final_content = apply_policy(content, policy);

    let dest = persist_dest(path);
    let parent = dest
        .parent()
        .context("cannot determine parent directory of target path")?;

    let tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;

    std::fs::write(tmp.path(), final_content.as_bytes())
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;

    // NamedTempFile creates files with 0o600; apply standard 0o644 so
    // new files are group/world-readable as users expect (#1161).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(tmp.path(), perms)
            .with_context(|| format!("failed to set permissions on {}", tmp.path().display()))?;
    }

    tmp.persist_noclobber(&dest).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::Error::new(crate::exit::AlreadyExistsError {
                msg: format!("file already exists: {}", path.display()),
            })
        } else {
            anyhow::Error::from(e.error)
                .context(format!("failed to persist tempfile to {}", path.display()))
        }
    })?;

    Ok(())
}

/// Atomically write `content` to `path` after applying `policy`.
///
/// Default path (single hard link or new file): a temporary file is created in
/// the same directory as `path`, written to, then renamed over `path`. The
/// target is never left half-written for normal files.
///
/// When the resolved target is a regular file with **more than one hard link**
/// (`nlink > 1` / Windows `nNumberOfLinks > 1`), rename would
/// break siblings (they would keep the old inode). In that case the full
/// payload is staged to a same-dir temp first, then written into the
/// **existing** inode so all hardlink paths stay in sync (#1733).
/// Symlinks are resolved first (#1230); the
/// hardlink rule applies to the resolved path.
pub(crate) fn atomic_write(path: &Path, content: &str, policy: &WritePolicy) -> anyhow::Result<()> {
    policy.refuse_unsupported_charset()?;
    let final_content = apply_policy(content, policy);

    // Resolve live symlinks: write to the target file, not the symlink entry
    // (#1230). Without this, persist() (rename) replaces the symlink directory
    // entry with a regular file, destroying the symlink and leaving the target
    // unchanged.
    //
    // Dangling symlinks cannot be resolved. Force-create / overwrite must still
    // materialize a regular file at `path` (agent recreate after bad rename).
    // Unlink the broken link entry, then write to `path` itself.
    let resolved;
    let write_path = if path.is_symlink() {
        match crate::containment::safe_canonicalize(path) {
            Ok(p) => {
                // dunce: strip Windows \\?\ so display and multi-link checks stay clean.
                resolved = p;
                resolved.as_path()
            }
            Err(_) => {
                std::fs::remove_file(path).with_context(|| {
                    format!("failed to replace dangling symlink {}", path.display())
                })?;
                path
            }
        }
    } else if path.is_file() {
        // Windows 8.3 names (LONGFI~1.TXT) alias the long directory entry.
        // persist() rename using the short spelling replaces that entry
        // (LongFileName.txt vanishes; a new LONGFI~1.TXT file appears).
        match crate::containment::safe_canonicalize(path) {
            Ok(p) => {
                resolved = p;
                resolved.as_path()
            }
            Err(_) => path,
        }
    } else {
        path
    };

    // Capture the original file's permissions before overwriting.
    let original_meta = std::fs::metadata(write_path).ok();
    let original_perms = original_meta.as_ref().map(|m| m.permissions());

    // Preserve hardlinks when the resolved target is multi-linked (#1733).
    if let Some(ref meta) = original_meta
        && meta.is_file()
        && hard_link_count(write_path, meta) > 1
    {
        return write_preserving_hardlinks(write_path, final_content.as_bytes(), original_perms);
    }

    let dest = persist_dest(write_path);
    let parent = dest
        .parent()
        .context("cannot determine parent directory of target path")?;

    // Create a named tempfile in the same directory so the rename is atomic.
    let tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;

    std::fs::write(tmp.path(), final_content.as_bytes())
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;

    // Copy NTFS MOTW onto the tempfile before applying dest permissions.
    // A readonly dest would make `path:Zone.Identifier` unwritable if we
    // set_permissions first. Hardlink in-place writes already keep streams.
    #[cfg(windows)]
    copy_windows_named_streams(write_path, tmp.path())?;

    // Restore the original permissions on the temp file before renaming.
    if let Some(perms) = original_perms {
        std::fs::set_permissions(tmp.path(), perms)
            .with_context(|| format!("failed to set permissions on {}", tmp.path().display()))?;
    }

    tmp.persist(&dest)
        .with_context(|| format!("failed to persist tempfile to {}", write_path.display()))?;

    Ok(())
}

/// Dest path for tempfile persist. On Windows, long dests need the `\\?\`
/// prefix or `MoveFileEx` returns `rollback` (MAX_PATH 260). Short dests
/// stay unchanged so 8.3 and existing tests keep the same spelling.
fn persist_dest(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        windows_extended_persist_path(path)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// `\\?\C:\...` or `\\?\UNC\server\share\...` when the dest is at or past
/// the legacy 260-char limit. Already-prefixed and short dests are unchanged.
#[cfg(windows)]
fn windows_extended_persist_path(path: &Path) -> std::path::PathBuf {
    const MAX_PATH_BUDGET: usize = 248;
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let abs = crate::ops::file::windows_collapse_dest_path(&abs);
    let raw = abs.as_os_str().to_string_lossy();
    if raw.starts_with(r"\\?\") || raw.starts_with(r"\\.\") {
        return abs;
    }
    if raw.len() < MAX_PATH_BUDGET {
        return abs;
    }
    if raw.starts_with(r"\\") {
        let rest = raw.trim_start_matches('\\');
        return std::path::PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    std::path::PathBuf::from(format!(r"\\?\{raw}"))
}

/// Always try these names even when stream listing fails (MOTW).
#[cfg(windows)]
const WINDOWS_PRESERVED_STREAMS: &[&str] = &["Zone.Identifier"];

/// `path:stream` without going through [`crate::ops::file::is_windows_ads_path`]
/// (that helper refuses dests that *look* like ADS, which these are).
#[cfg(windows)]
fn windows_stream_path(path: &Path, stream: &str) -> std::path::PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(":");
    raw.push(stream);
    std::path::PathBuf::from(raw)
}

/// Copy NTFS named streams from `from` onto `to` (the tempfile).
///
/// `Get-Item -Stream *` lists custom streams without `FindFirstStreamW`
/// (this crate denies `unsafe`). MOTW is always attempted even if listing
/// fails.
/// Missing streams are skipped. A stream that exists but cannot be copied
/// fails the write so we do not claim success after dropping it.
#[cfg(windows)]
fn copy_windows_named_streams(from: &Path, to: &Path) -> anyhow::Result<()> {
    if !from.is_file() {
        return Ok(());
    }
    let mut names = list_windows_named_stream_names(from);
    for fallback in WINDOWS_PRESERVED_STREAMS {
        if !names.iter().any(|n| n == fallback) {
            names.push((*fallback).to_string());
        }
    }
    for name in &names {
        let src = windows_stream_path(from, name);
        // CopyFileEx rejects an ADS dest (ERROR_INVALID_PARAMETER). Read
        // then write through the `path:stream` spelling instead.
        let bytes = match std::fs::read(&src) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("failed to read NTFS stream {name} from {}", from.display())
                });
            }
        };
        let dest = windows_stream_path(to, name);
        std::fs::write(&dest, bytes).with_context(|| {
            format!("failed to restore NTFS stream {name} onto {}", to.display())
        })?;
    }
    Ok(())
}

/// List NTFS stream names via `Get-Item -Stream *`. Empty on spawn/parse miss.
///
/// Path goes through `PATCHLOOM_DIR_R` and `-LiteralPath` so dest names
/// with `&` / `|` are not extra commands. `-Force` includes Hidden and
/// System dests that `dir /R` and unforced `Get-Item` skip.
#[cfg(windows)]
fn list_windows_named_stream_names(path: &Path) -> Vec<String> {
    let mut cmd = std::process::Command::new("powershell");
    cmd.env("PATCHLOOM_DIR_R", path).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$ErrorActionPreference='Stop'; Get-Item -LiteralPath $env:PATCHLOOM_DIR_R -Force -Stream * | ForEach-Object { $_.Stream }",
    ]);
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_stream_name_lines(&String::from_utf8_lossy(&output.stdout))
}

/// One stream name per `Get-Item -Stream *` line. Skips the default `:$DATA`.
#[cfg(any(windows, test))]
fn parse_stream_name_lines(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let name = line.trim();
        if name.is_empty()
            || name.eq_ignore_ascii_case(":$DATA")
            || name.eq_ignore_ascii_case("::$DATA")
        {
            continue;
        }
        if names.iter().any(|n| n == name) {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

/// Directory-entry count for this file. Unix `nlink`; Windows
/// `nNumberOfLinks` via `winapi-util` (stable `MetadataExt::number_of_links`
/// is gated on `windows_by_handle`). Other targets report 1 so
/// [`atomic_write`] keeps temp+rename.
fn hard_link_count(path: &Path, meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        meta.nlink()
    }
    #[cfg(windows)]
    {
        let _ = meta;
        match std::fs::File::open(path).and_then(winapi_util::file::information) {
            Ok(info) => info.number_of_links(),
            Err(_) => 1,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, meta);
        1
    }
}

/// Stage full content on a same-dir temp, then rewrite the existing inode in
/// place so all hardlink paths observe the new bytes (`nlink` stays > 1).
///
/// Used only when [`hard_link_count`] is greater than 1. Single-link files
/// keep the rename path in [`atomic_write`].
fn write_preserving_hardlinks(
    path: &Path,
    bytes: &[u8],
    original_perms: Option<std::fs::Permissions>,
) -> anyhow::Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .context("cannot determine parent directory of target path")?;

    // Stage complete payload first so we do not begin mutating the shared
    // inode until the full content exists on disk (best-effort durability).
    let mut tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create tempfile in {}", parent.display()))?;
    tmp.write_all(bytes)
        .with_context(|| format!("failed to write to tempfile {}", tmp.path().display()))?;
    // Best-effort; some filesystems/OS combinations do not support sync_all.
    let _ = tmp.as_file().sync_all();

    // Prefer write + set_len over truncate-first so a failed mid-write can
    // still recover the shared inode from the staged temp (hosts with
    // package layouts / multi-hardlinked configs).
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| {
            format!(
                "failed to open hardlinked path for write {}",
                path.display()
            )
        })?;
    let write_result = (|| -> anyhow::Result<()> {
        file.write_all(bytes)
            .with_context(|| format!("failed to write hardlinked path {}", path.display()))?;
        file.set_len(bytes.len() as u64).with_context(|| {
            format!("failed to set length on hardlinked path {}", path.display())
        })?;
        let _ = file.sync_all();
        Ok(())
    })();
    drop(file);
    if let Err(e) = write_result {
        // Best-effort restore shared inode from the staged full payload.
        let _ = std::fs::copy(tmp.path(), path);
        return Err(e);
    }

    if let Some(perms) = original_perms {
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("failed to restore permissions on {}", path.display()))?;
    }

    // NamedTempFile Drop removes the staging file.
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
pub(crate) fn run_format_command(
    global: &crate::cli::global::GlobalFlags,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    run_format_command_ext(global, cwd, None, global.format_config.as_ref())
}

/// Extended format runner that accepts an optional list of modified paths
/// and an optional format configuration from `.patchloom.toml`.
///
/// `modified_paths` are relative paths (as stored by the tx engine).
/// `format_config` provides per-extension formatter commands.
#[cfg(feature = "cli")]
pub(crate) fn run_format_command_ext(
    global: &crate::cli::global::GlobalFlags,
    cwd: &std::path::Path,
    modified_paths: Option<&[&str]>,
    format_config: Option<&crate::config::FormatConfig>,
) -> anyhow::Result<()> {
    if global.no_format {
        return Ok(());
    }

    let timeout_secs = global.format_timeout.unwrap_or(30);
    let show_format_warn = !global.quiet && !global.json && !global.jsonl;

    // Priority 1: explicit --format command (whole-project)
    if let Some(cmd) = global.format.as_deref() {
        refuse_contained_format_cmd(global, cmd)?;
        let result = crate::exec::run_with_timeout(cmd, timeout_secs, cwd).map_err(|e| {
            crate::exit::FormatFailedError::new(format!("format command failed ({cmd}): {e}"))
        })?;
        if !result.status.success() {
            let stderr = if result.stderr_head.is_empty() {
                String::new()
            } else {
                format!(": {}", result.stderr_head)
            };
            return Err(crate::exit::FormatFailedError::new(format!(
                "format command failed ({cmd}){stderr}"
            ))
            .into());
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
        refuse_contained_format_cmd(global, cmd)?;
        let result = crate::exec::run_with_timeout(cmd, timeout_secs, cwd)?;
        if !result.status.success() && show_format_warn {
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

    // When modified_paths are not passed explicitly, discover them via
    // `git diff --name-only HEAD`. This covers CLI commands (replace, md,
    // ast replace, etc.) that call run_format_command without tracking
    // which files they touched.
    let discovered: Vec<String>;
    let discovered_refs: Vec<&str>;
    let paths: &[&str] = match modified_paths {
        Some(p) => p,
        None => {
            discovered = discover_modified_files(cwd);
            if discovered.is_empty() {
                return Ok(());
            }
            discovered_refs = discovered.iter().map(|s| s.as_str()).collect();
            &discovered_refs
        }
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
                refuse_contained_format_cmd(global, cmd_template)?;
                match crate::exec::run_with_timeout(&cmd, timeout_secs, cwd) {
                    Ok(result) if !result.status.success() && show_format_warn => {
                        eprintln!(
                            "warning: formatter for .{ext} failed on {file}: {}",
                            result.stderr_head.trim()
                        );
                    }
                    Err(e) if show_format_warn => {
                        eprintln!("warning: formatter for .{ext} error on {file}: {e}");
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// When `--contain` is on, refuse a format command that needs a shell for
/// redirects, pipelines, or substitutions. Same scanner as plan lifecycle
/// ([`crate::plan::refuse_lifecycle_shell_metas`]).
#[cfg(feature = "cli")]
pub(crate) fn refuse_contained_format_cmd(
    global: &crate::cli::global::GlobalFlags,
    cmd: &str,
) -> anyhow::Result<()> {
    if !global.contain {
        return Ok(());
    }
    if let Err(e) = crate::plan::refuse_lifecycle_shell_metas(cmd) {
        return Err(crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::GuardRejected,
            format!("format command refused under --contain: {cmd:?} ({e})"),
        )
        .into());
    }
    Ok(())
}

/// Preflight configured format commands before a contained write commits.
#[cfg(feature = "cli")]
pub(crate) fn refuse_contained_format_cmds(
    global: &crate::cli::global::GlobalFlags,
) -> anyhow::Result<()> {
    if !global.contain || global.no_format {
        return Ok(());
    }
    if let Some(cmd) = global.format.as_deref() {
        return refuse_contained_format_cmd(global, cmd);
    }
    let Some(config) = global.format_config.as_ref() else {
        return Ok(());
    };
    if config.auto != Some(true) {
        return Ok(());
    }
    if let Some(cmd) = config.command.as_deref() {
        return refuse_contained_format_cmd(global, cmd);
    }
    for cmd in config.by_extension.values() {
        refuse_contained_format_cmd(global, cmd)?;
    }
    Ok(())
}

/// Discover files modified since the last commit by running
/// `git diff --name-only HEAD` in `cwd`. Returns relative paths.
/// Falls back to an empty list on any error (not a git repo, etc.).
#[cfg(feature = "cli")]
fn discover_modified_files(cwd: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Shell-escape a file path for safe inclusion in a command string.
///
/// - **Unix** (`sh -c`): wraps in single quotes, escaping embedded `'`.
/// - **Windows** (`cmd /C`): wraps in double quotes, doubling embedded `"`.
#[cfg(feature = "cli")]
fn shell_escape(path: &str) -> String {
    // If the path contains no special characters, return as-is.
    // On Windows, backslash is a path separator, so treat it as safe.
    if path.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || b == b'/'
            || b == b'.'
            || b == b'_'
            || b == b'-'
            || (cfg!(windows) && b == b'\\')
    }) {
        return path.to_string();
    }

    #[cfg(windows)]
    {
        // cmd.exe uses double quotes; escape embedded double quotes by doubling.
        format!("\"{}\"", path.replace('"', "\"\""))
    }
    #[cfg(not(windows))]
    {
        // POSIX sh uses single quotes; escape embedded single quotes.
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

#[path = "write_tests.rs"]
#[cfg(test)]
mod tests;
