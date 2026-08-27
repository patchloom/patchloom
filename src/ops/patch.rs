//! Unified-diff parse and apply (git renames, pure renames, hunk fuzz).
//!
//! size-waiver: single-domain patch parse + apply + pure renames (#2101).
//! Co-located tests push line count. Policy #1408 — do not split for LOC alone.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchLine {
    Context(String),
    Remove(String),
    Add(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<PatchLine>,
    /// `\ No newline at end of file` appeared after a remove/context line.
    pub old_no_final_newline: bool,
    /// `\ No newline at end of file` appeared after an add line.
    pub new_no_final_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub path: String,
    pub hunks: Vec<Hunk>,
    /// `true` when the `---` line is `/dev/null` (new file creation).
    pub is_creation: bool,
    /// `true` when the `+++` line is `/dev/null` (file deletion).
    pub is_deletion: bool,
    /// When `--- a/old` and `+++ b/new` differ (git rename), the pre-rename path.
    /// Content is loaded from this path; result is written to [`path`] (#2101).
    pub rename_from: Option<String>,
    /// Git 100% copy source (`copy from`). Dest is [`path`]. Source is kept
    /// (`rename_from` stays `None`). (#2171)
    pub copy_from: Option<String>,
    /// Dest is listed for preflight but apply refuses (`GIT binary patch`,
    /// `Binary files … differ`, mode-only chmod). (#2173)
    pub unsupported: Option<String>,
}

impl PatchFile {
    fn with_path(path: String) -> Self {
        Self {
            path,
            hunks: Vec::new(),
            is_creation: false,
            is_deletion: false,
            rename_from: None,
            copy_from: None,
            unsupported: None,
        }
    }

    /// Dest and source paths this file entry would touch (C-unescaped).
    #[must_use]
    pub fn declared_paths(&self) -> Vec<String> {
        let mut paths = vec![self.path.clone()];
        if let Some(from) = self.rename_from.as_ref() {
            paths.push(from.clone());
        }
        if let Some(from) = self.copy_from.as_ref() {
            paths.push(from.clone());
        }
        paths
    }
}

/// True when a git rename would overwrite an existing destination that is not
/// a case-only rename of the source (parity with `file.rename` without force).
///
/// Callers supply `dest_exists` from disk or tx pending state.
pub fn rename_would_clobber_dest(from: &str, to: &str, dest_exists: bool) -> bool {
    if !dest_exists || from == to {
        return false;
    }
    let from_path = std::path::Path::new(from);
    let to_path = std::path::Path::new(to);
    let case_only = from_path.parent() == to_path.parent()
        && from_path.file_name().map(|n| n.to_ascii_lowercase())
            == to_path.file_name().map(|n| n.to_ascii_lowercase());
    !case_only
}

/// Error message when [`rename_would_clobber_dest`] is true.
pub fn rename_dest_exists_msg(to: &str) -> String {
    format!(
        "destination already exists: {to} (patch rename refuses overwrite; remove dest or use file.rename --force)"
    )
}

/// Error message when a git copy dest already exists.
pub fn copy_dest_exists_msg(to: &str) -> String {
    format!("destination already exists: {to} (patch copy refuses overwrite; remove dest)")
}

/// Error message when a patch-create dest already exists.
pub fn create_dest_exists_msg(to: &str) -> String {
    format!("destination already exists: {to} (patch create refuses overwrite; remove dest)")
}

/// Already-exists message when dest must not be overwritten.
///
/// Covers rename clobber, git copy dest, and create dest (`--- /dev/null`,
/// with or without hunks). Case-only rename returns `None` even when dest
/// exists.
pub(crate) fn dest_clobber_msg(
    path: &str,
    dest_exists: bool,
    rename_from: Option<&str>,
    is_copy: bool,
    is_creation: bool,
) -> Option<String> {
    if let Some(from) = rename_from
        && rename_would_clobber_dest(from, path, dest_exists)
    {
        return Some(rename_dest_exists_msg(path));
    }
    if is_copy && dest_exists {
        return Some(copy_dest_exists_msg(path));
    }
    if is_creation && dest_exists {
        return Some(create_dest_exists_msg(path));
    }
    None
}

impl PatchFile {
    pub(crate) fn dest_clobber_msg(&self, dest_exists: bool) -> Option<String> {
        dest_clobber_msg(
            &self.path,
            dest_exists,
            self.rename_from.as_deref(),
            self.copy_from.is_some(),
            self.is_creation && self.copy_from.is_none(),
        )
    }
}

/// Apply-refuse message for a dest listed from git-meta that we do not write.
pub fn unsupported_git_meta_msg(path: &str, reason: &str) -> String {
    format!(
        "patch apply: {path} -- unsupported git-meta ({reason}); dest is listed for preflight but apply refuses"
    )
}

/// Check whether line `idx` is a real file header ("--- " followed by "+++"),
/// not a removed line whose content happens to start with "-- " (e.g. SQL
/// comments produce `--- comment text` in the diff).
///
/// Requires additional evidence beyond the "--- "/"+++" prefix pair:
/// - `a/` or `b/` path prefixes (git diff format)
/// - `/dev/null` (file creation/deletion)
/// - a tab character (traditional diff timestamps)
/// - a preceding `diff ` line
/// - position at the very start of the input
///
/// This prevents false positives inside hunks (#1185).
fn is_file_header(lines: &[&str], idx: usize) -> bool {
    if !lines[idx].starts_with("--- ")
        || idx + 1 >= lines.len()
        || !lines[idx + 1].starts_with("+++ ")
    {
        return false;
    }
    let minus_rest = &lines[idx][4..];
    let plus_rest = &lines[idx + 1][4..];
    // Standard git diff paths start with a/ or b/.
    if minus_rest.starts_with("a/") || minus_rest.starts_with("/dev/null") {
        return true;
    }
    if plus_rest.starts_with("b/") || plus_rest.starts_with("/dev/null") {
        return true;
    }
    // Traditional diff uses tabs before timestamps.
    if minus_rest.contains('\t') || plus_rest.contains('\t') {
        return true;
    }
    // Preceded by a `diff ` line.
    if idx > 0 && lines[idx - 1].starts_with("diff ") {
        return true;
    }
    // At the very start of the input (no prior context).
    idx == 0
}

/// Strip optional Git C-quotes from a path token (`"my file.rs"` → `my file.rs`).
fn unquote_git_path_meta(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        unquote_git_c_string(&s[1..s.len() - 1])
    } else {
        s.to_string()
    }
}

/// Parse `diff --git a/old b/new` into (old, new) relative paths.
///
/// Accepts the full line or the path pair after `diff --git `.
/// Handles C-quoted paths and mixed quoting (`"a/my file.rs" b/ok.rs`).
/// Do not split `diff --git` on whitespace in hosts; use this helper. (#2176)
#[must_use]
pub fn parse_diff_git_paths(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ").unwrap_or(line).trim();
    if rest.is_empty() {
        return None;
    }
    let (raw_a, raw_b) = split_two_git_path_tokens(rest)?;
    // Tokenizer already dropped surrounding quotes; still C-unescape the body
    // (`b/\056env` → `b/.env`).
    let a = strip_diff_ab_prefix(&unquote_git_c_string(&raw_a))?;
    let b = strip_diff_ab_prefix(&unquote_git_c_string(&raw_b))?;
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a, b))
}

fn strip_diff_ab_prefix(s: &str) -> Option<String> {
    s.strip_prefix("a/")
        .or_else(|| s.strip_prefix("b/"))
        .map(str::to_string)
}

fn split_two_git_path_tokens(rest: &str) -> Option<(String, String)> {
    let mut chars = rest.chars().peekable();
    let first = next_git_path_token(&mut chars)?;
    let second = next_git_path_token(&mut chars)?;
    Some((first, second))
}

fn next_git_path_token(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
    match chars.peek() {
        None => None,
        Some('"') => {
            chars.next();
            let mut raw = String::new();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    raw.push('\\');
                    if let Some(n) = chars.next() {
                        raw.push(n);
                    }
                } else if c == '"' {
                    break;
                } else {
                    raw.push(c);
                }
            }
            Some(raw)
        }
        Some(_) => {
            let mut raw = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                raw.push(c);
                chars.next();
            }
            if raw.is_empty() { None } else { Some(raw) }
        }
    }
}

pub fn parse_patch(input: &str) -> Result<Vec<PatchFile>, String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut files: Vec<PatchFile> = Vec::new();
    let mut i = 0;
    // When `diff --git` has copy from/to then ---/+++, do not treat path
    // inequality as rename (would delete the copy source on apply).
    let mut suppress_path_rename = false;

    while i < lines.len() {
        // Pure git rename (100% similarity) often has no ---/+++ headers:
        //   diff --git a/old b/new
        //   similarity index 100%
        //   rename from old
        //   rename to new
        if lines[i].starts_with("diff --git ")
            && (i + 1 >= lines.len() || !is_file_header(&lines, i + 1))
        {
            // Look ahead for rename/copy/git-meta before next file/diff.
            let mut rename_from_meta: Option<String> = None;
            let mut rename_to_meta: Option<String> = None;
            let mut copy_from_meta: Option<String> = None;
            let mut copy_to_meta: Option<String> = None;
            let mut saw_copy = false;
            let mut saw_new_file_mode = false;
            let mut saw_deleted_file_mode = false;
            let mut saw_empty_index = false;
            let mut saw_git_binary = false;
            let mut saw_binary_files = false;
            let mut saw_old_mode = false;
            let mut saw_new_mode = false;
            let mut j = i + 1;
            while j < lines.len() && !lines[j].starts_with("diff ") && !is_file_header(&lines, j) {
                if let Some(rest) = lines[j].strip_prefix("rename from ") {
                    // C-quoted paths with spaces: rename from "my file.rs"
                    rename_from_meta = Some(unquote_git_path_meta(rest));
                } else if let Some(rest) = lines[j].strip_prefix("rename to ") {
                    rename_to_meta = Some(unquote_git_path_meta(rest));
                } else if let Some(rest) = lines[j].strip_prefix("copy from ") {
                    copy_from_meta = Some(unquote_git_path_meta(rest));
                    saw_copy = true;
                } else if let Some(rest) = lines[j].strip_prefix("copy to ") {
                    copy_to_meta = Some(unquote_git_path_meta(rest));
                    saw_copy = true;
                } else if lines[j].starts_with("new file mode ") {
                    saw_new_file_mode = true;
                } else if lines[j].starts_with("deleted file mode ") {
                    saw_deleted_file_mode = true;
                } else if lines[j].starts_with("old mode ") {
                    saw_old_mode = true;
                } else if lines[j].starts_with("new mode ") {
                    saw_new_mode = true;
                } else if lines[j].starts_with("GIT binary patch") {
                    saw_git_binary = true;
                } else if lines[j].starts_with("Binary files ") && lines[j].contains(" differ") {
                    saw_binary_files = true;
                } else if let Some(rest) = lines[j].strip_prefix("index ") {
                    // Empty blob create: `index 0000000..e69de29`.
                    if rest.contains("0000000..e69de29") || rest.starts_with("0000000..") {
                        saw_empty_index = true;
                    }
                } else if lines[j].starts_with("@@ ") {
                    // Has hunks under this diff without ---/+++; fall through
                    // to normal scan (should not happen in git format).
                    break;
                }
                j += 1;
            }
            // If the next real header is ---/+++ for this same rename, let the
            // normal path consume it (may include content hunks).
            if j < lines.len() && is_file_header(&lines, j) {
                suppress_path_rename = saw_copy;
                i = j;
                // fall through to ---/+++ handler below (no continue)
            } else if let (Some(from), Some(to)) = (rename_from_meta, rename_to_meta) {
                // Pure rename: require explicit rename from/to (never infer from
                // path inequality alone — that mis-classifies copy as rename
                // and would delete the source on apply).
                let mut pf = PatchFile::with_path(to);
                pf.rename_from = Some(from);
                files.push(pf);
                i = j;
                continue;
            } else if let (Some(from), Some(to)) = (copy_from_meta, copy_to_meta) {
                // 100% git copy: dest is created, source is kept. (#2171)
                let mut pf = PatchFile::with_path(to);
                pf.is_creation = true;
                pf.copy_from = Some(from);
                files.push(pf);
                i = j;
                continue;
            } else if saw_git_binary || saw_binary_files {
                let dest = parse_diff_git_paths(lines[i]).map(|(_, b)| b);
                let Some(dest) = dest else {
                    return Err("binary git-meta dest could not be parsed".to_string());
                };
                let reason = if saw_git_binary {
                    "GIT binary patch"
                } else {
                    "Binary files differ"
                };
                let mut pf = PatchFile::with_path(dest);
                pf.unsupported = Some(reason.to_string());
                files.push(pf);
                i = j;
                continue;
            } else if saw_new_file_mode && saw_empty_index {
                let dest = parse_diff_git_paths(lines[i]).map(|(_, b)| b);
                let Some(dest) = dest else {
                    return Err("empty-create dest could not be parsed".to_string());
                };
                let mut pf = PatchFile::with_path(dest);
                pf.is_creation = true;
                files.push(pf);
                i = j;
                continue;
            } else if saw_deleted_file_mode {
                let dest = parse_diff_git_paths(lines[i]).map(|(a, _)| a);
                let Some(dest) = dest else {
                    return Err("deleted-file dest could not be parsed".to_string());
                };
                let mut pf = PatchFile::with_path(dest);
                pf.is_deletion = true;
                files.push(pf);
                i = j;
                continue;
            } else if saw_old_mode && saw_new_mode {
                let dest = parse_diff_git_paths(lines[i]).map(|(_, b)| b);
                let Some(dest) = dest else {
                    return Err("mode-only dest could not be parsed".to_string());
                };
                let mut pf = PatchFile::with_path(dest);
                pf.unsupported = Some("mode-only chmod".to_string());
                files.push(pf);
                i = j;
                continue;
            } else {
                // Incomplete meta (similarity only, etc.): skip this diff line.
                i += 1;
                continue;
            }
        }

        if !is_file_header(&lines, i) {
            i += 1;
            continue;
        }

        // For deletions the `+++` line is `/dev/null`; take path from `---`.
        // Git renames use different minus/plus paths (neither `/dev/null`).
        let minus_path = parse_file_path(lines[i]);
        let plus_path = parse_file_path(lines[i + 1]);
        let is_creation = minus_path == "/dev/null";
        let is_deletion = plus_path == "/dev/null";
        let rename_from =
            if !is_creation && !is_deletion && minus_path != plus_path && !suppress_path_rename {
                Some(minus_path.clone())
            } else {
                None
            };
        suppress_path_rename = false;
        let path = if is_deletion { minus_path } else { plus_path };
        i += 2;

        let mut hunks: Vec<Hunk> = Vec::new();
        while i < lines.len() && !is_file_header(&lines, i) {
            if lines[i].starts_with("@@ ") {
                let hunk = parse_hunk_header(lines[i])?;
                let mut hunk_lines: Vec<PatchLine> = Vec::new();
                let mut old_no_final_newline = false;
                let mut new_no_final_newline = false;
                i += 1;

                while i < lines.len()
                    && !lines[i].starts_with("@@ ")
                    && !is_file_header(&lines, i)
                    && !lines[i].starts_with("diff ")
                {
                    let line = lines[i];
                    if let Some(rest) = line.strip_prefix('+') {
                        hunk_lines.push(PatchLine::Add(rest.to_string()));
                    } else if let Some(rest) = line.strip_prefix('-') {
                        hunk_lines.push(PatchLine::Remove(rest.to_string()));
                    } else if let Some(rest) = line.strip_prefix(' ') {
                        hunk_lines.push(PatchLine::Context(rest.to_string()));
                    } else if line == "\\ No newline at end of file" {
                        // Marker applies to the previous hunk line (git format).
                        match hunk_lines.last() {
                            Some(PatchLine::Add(_)) => new_no_final_newline = true,
                            Some(PatchLine::Remove(_) | PatchLine::Context(_)) => {
                                old_no_final_newline = true;
                            }
                            None => {}
                        }
                    } else {
                        hunk_lines.push(PatchLine::Context(line.to_string()));
                    }
                    i += 1;
                }

                hunks.push(Hunk {
                    old_start: hunk.old_start,
                    old_count: hunk.old_count,
                    new_start: hunk.new_start,
                    new_count: hunk.new_count,
                    lines: hunk_lines,
                    old_no_final_newline,
                    new_no_final_newline,
                });
            } else if lines[i].starts_with("diff ") {
                break;
            } else {
                i += 1;
            }
        }

        // Empty hunks are valid for pure renames (100% similarity, no content
        // change). Other empty-hunk files remain a parse error.
        if hunks.is_empty() && rename_from.is_none() && !is_creation && !is_deletion {
            return Err(format!("no hunks found for file {path}"));
        }
        if hunks.is_empty() && is_creation {
            return Err(format!("no hunks found for file {path}"));
        }
        // Deletion without hunks is already allowed by apply path.

        files.push(PatchFile {
            path,
            hunks,
            is_creation,
            is_deletion,
            rename_from,
            copy_from: None,
            unsupported: None,
        });
    }

    if files.is_empty() {
        return Err("no files found in patch".to_string());
    }

    Ok(files)
}

fn parse_file_path(line: &str) -> String {
    let raw = line
        .strip_prefix("+++ ")
        .or_else(|| line.strip_prefix("--- "))
        .unwrap_or(line);

    // Strip tab-separated timestamp from `diff -u` output first so C-quoted
    // paths with spaces are not split mid-name.
    let mut path = raw.split('\t').next().unwrap_or(raw);

    // Git C-quotes special paths: `+++ "b/my file.rs"`. Unquote before a/b
    // strip so agents replaying `git diff` resolve the real path.
    let owned_unquoted;
    if path.len() >= 2 && path.starts_with('"') && path.ends_with('"') {
        owned_unquoted = unquote_git_c_string(&path[1..path.len() - 1]);
        path = owned_unquoted.as_str();
    }

    let path = path
        .strip_prefix("b/")
        .or_else(|| path.strip_prefix("a/"))
        .unwrap_or(path);
    path.to_string()
}

/// Dest and source paths a unified diff would touch (C-unescaped).
///
/// Includes `---`/`+++`, rename from/to, copy from/to, empty creates, and
/// git-meta dests that apply refuses (binary / mode-only). Parse errors are
/// `Err` (do not return a partial dest list). (#2172)
pub fn patch_declared_paths(text: &str) -> Result<Vec<String>, String> {
    let files = parse_patch(text)?;
    Ok(files.iter().flat_map(PatchFile::declared_paths).collect())
}

/// Parse one unified-diff path token (`+++ b/foo`, `rename to "\\056env"`).
///
/// Accepts a raw token with or without a `+++ ` / `--- ` / `rename` / `copy`
/// prefix so hosts can feed single lines. (#2170)
#[must_use]
pub fn parse_diff_file_path(token: &str) -> String {
    let t = token.trim();
    let t = t
        .strip_prefix("+++ ")
        .or_else(|| t.strip_prefix("--- "))
        .or_else(|| t.strip_prefix("rename from "))
        .or_else(|| t.strip_prefix("rename to "))
        .or_else(|| t.strip_prefix("copy from "))
        .or_else(|| t.strip_prefix("copy to "))
        .unwrap_or(t);
    parse_file_path(&format!("+++ {t}"))
}

/// Git C-string unescape for a quoted path body (no surrounding quotes).
///
/// Handles `\"`, `\\`, `\a`/`\b`/`\f`/`\n`/`\r`/`\t`/`\v`, and up to
/// three-digit octal byte escapes (`\303\251` → UTF-8 `é`, `\056` → `.`)
/// used by `core.quotePath`. (#2170 / #2175)
#[must_use]
pub fn unquote_git_c_string(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('a') => bytes.push(b'\x07'),
                Some('b') => bytes.push(b'\x08'),
                Some('f') => bytes.push(b'\x0c'),
                Some('n') => bytes.push(b'\n'),
                Some('r') => bytes.push(b'\r'),
                Some('t') => bytes.push(b'\t'),
                Some('v') => bytes.push(b'\x0b'),
                Some('"') => bytes.push(b'"'),
                Some('\\') => bytes.push(b'\\'),
                Some(d) if d.is_digit(8) => {
                    // Up to three octal digits (git C-quoted path bytes).
                    let mut val = d.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        match chars.peek().and_then(|n| n.to_digit(8)) {
                            Some(nd) => {
                                chars.next();
                                val = val * 8 + nd;
                            }
                            None => break,
                        }
                    }
                    bytes.push(val as u8);
                }
                Some(other) => {
                    let mut buf = [0u8; 4];
                    bytes.extend(other.encode_utf8(&mut buf).as_bytes());
                }
                None => bytes.push(b'\\'),
            }
        } else {
            let mut buf = [0u8; 4];
            bytes.extend(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn parse_hunk_header(line: &str) -> Result<Hunk, String> {
    let trimmed = line
        .strip_prefix("@@ ")
        .ok_or_else(|| format!("invalid hunk header: {line}"))?;

    let end = trimmed
        .find(" @@")
        .ok_or_else(|| format!("invalid hunk header (no closing @@): {line}"))?;
    let range_part = &trimmed[..end];

    let parts: Vec<&str> = range_part.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(format!("invalid hunk header ranges: {line}"));
    }

    let (old_start, old_count) = parse_range(parts[0].strip_prefix('-').unwrap_or(parts[0]))?;
    let (new_start, new_count) = parse_range(parts[1].strip_prefix('+').unwrap_or(parts[1]))?;

    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
        old_no_final_newline: false,
        new_no_final_newline: false,
    })
}

fn parse_range(s: &str) -> Result<(usize, usize), String> {
    if let Some((a, b)) = s.split_once(',') {
        let start = a
            .parse::<usize>()
            .map_err(|e| format!("bad range start '{a}': {e}"))?;
        let count = b
            .parse::<usize>()
            .map_err(|e| format!("bad range count '{b}': {e}"))?;
        Ok((start, count))
    } else {
        let start = s
            .parse::<usize>()
            .map_err(|e| format!("bad range '{s}': {e}"))?;
        Ok((start, 1))
    }
}

const FUZZ_RANGE: usize = 3;

const CONFLICT_OURS: &str = "<<<<<<< patchloom (ours)";
const CONFLICT_SEP: &str = "=======";
const CONFLICT_THEIRS: &str = ">>>>>>> patch (theirs)";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum OnStale {
    #[default]
    Fail,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyHunksOptions {
    pub on_stale: OnStale,
    pub allow_conflicts: bool,
}
impl Default for ApplyHunksOptions {
    fn default() -> Self {
        Self {
            on_stale: OnStale::Fail,
            allow_conflicts: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRange {
    pub start_line: usize,
    pub end_line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub content: String,
    pub conflicts: Vec<ConflictRange>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeError {
    pub message: String,
}
impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyHunksStatus {
    Clean,
    Merged,
    Conflict,
}
impl ApplyHunksStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ApplyHunksStatus::Clean => "clean",
            ApplyHunksStatus::Merged => "merged",
            ApplyHunksStatus::Conflict => "conflict",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyHunksResult {
    pub content: String,
    pub status: ApplyHunksStatus,
    pub conflicts: Vec<ConflictRange>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchApplyFileResult {
    pub path: String,
    pub content: String,
    pub status: ApplyHunksStatus,
    pub conflicts: Vec<ConflictRange>,
    /// `true` when the patch creates a new file (`--- /dev/null`).
    pub is_creation: bool,
    /// `true` when the patch deletes a file (`+++ /dev/null`).
    pub is_deletion: bool,
    /// Pre-rename path when applying a git rename patch (#2101).
    pub rename_from: Option<String>,
    /// Git copy source when dest is a 100% copy (source is kept). (#2171)
    pub copy_from: Option<String>,
}

pub fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String, String> {
    let eol = detect_eol(original);
    let mut src_lines: Vec<String> = original.lines().map(String::from).collect();
    let mut had_final_newline =
        original.ends_with('\n') || original.ends_with("\r\n") || original.is_empty();
    let mut offset: isize = 0;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        let expected: isize = if hunk.old_start == 0 {
            0
        } else {
            let Some(base) = isize::try_from(hunk.old_start)
                .ok()
                .and_then(|s| s.checked_sub(1))
                .and_then(|s| s.checked_add(offset))
            else {
                return Err(format!(
                    "hunk {} failed: line number {} out of range",
                    hunk_idx + 1,
                    hunk.old_start,
                ));
            };
            base
        };

        // Collect &str refs directly, avoiding N string clones per hunk.
        let old_refs: Vec<&str> = hunk
            .lines
            .iter()
            .filter_map(|pl| match pl {
                PatchLine::Context(s) | PatchLine::Remove(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();

        let src_refs: Vec<&str> = src_lines.iter().map(std::string::String::as_str).collect();

        let pos = find_match(&src_refs, &old_refs, expected, FUZZ_RANGE).ok_or_else(|| {
            let snippet = old_refs
                .iter()
                .take(3)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "hunk {} failed: stale context near line {} — expected:\n{}",
                hunk_idx + 1,
                hunk.old_start,
                snippet,
            )
        })?;

        let new_lines: Vec<String> = hunk
            .lines
            .iter()
            .filter_map(|pl| match pl {
                PatchLine::Context(s) => Some(s.clone()),
                PatchLine::Add(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        let old_len = old_refs.len();
        let new_len = new_lines.len();
        // When the match covers the end of the file, git "\ No newline" markers
        // decide whether the result ends with a newline.
        let touches_eof = pos + old_len >= src_lines.len();
        src_lines.splice(pos..pos + old_len, new_lines);
        if touches_eof {
            let last_new = hunk
                .lines
                .iter()
                .rev()
                .find(|pl| matches!(pl, PatchLine::Add(_) | PatchLine::Context(_)));
            match last_new {
                Some(PatchLine::Add(_)) => {
                    if hunk.new_no_final_newline {
                        // Explicit: new file ends without NL.
                        had_final_newline = false;
                    } else if hunk.old_no_final_newline {
                        // Old last line lacked NL; new last line has no marker → has NL.
                        had_final_newline = true;
                    }
                    // Else keep prior had_final_newline (preserve EOF when only
                    // rewriting the last line without git markers).
                }
                Some(PatchLine::Context(_)) if hunk.old_no_final_newline => {
                    // EOF context line that lacked NL on the old side.
                    had_final_newline = false;
                }
                Some(PatchLine::Context(_)) if hunk.new_no_final_newline => {
                    had_final_newline = false;
                }
                _ => {}
            }
        }
        let delta = isize::try_from(new_len).unwrap_or(isize::MAX)
            - isize::try_from(old_len).unwrap_or(isize::MAX);
        offset = offset.saturating_add(delta);
    }

    Ok(join_lines_with(&src_lines, had_final_newline, eol))
}

pub fn apply_hunks_with_options(
    ours: &str,
    hunks: &[Hunk],
    options: ApplyHunksOptions,
) -> Result<ApplyHunksResult, String> {
    match options.on_stale {
        OnStale::Fail => {
            let content = apply_hunks(ours, hunks)?;
            Ok(ApplyHunksResult {
                content,
                status: ApplyHunksStatus::Clean,
                conflicts: Vec::new(),
            })
        }
        OnStale::Merge => {
            if let Ok(content) = apply_hunks(ours, hunks) {
                return Ok(ApplyHunksResult {
                    content,
                    status: ApplyHunksStatus::Clean,
                    conflicts: Vec::new(),
                });
            }
            let merge_result = merge_hunks(ours, hunks).map_err(|e| e.message)?;
            // apply_hunks already failed above (line 315); since it is pure and
            // deterministic, re-calling it with the same inputs would fail again.
            // The merge path always produces Merged or Conflict status.
            let status = if !merge_result.conflicts.is_empty() {
                ApplyHunksStatus::Conflict
            } else {
                ApplyHunksStatus::Merged
            };
            if status == ApplyHunksStatus::Conflict && !options.allow_conflicts {
                return Err(format!(
                    "patch merge produced {} conflict(s); pass --allow-conflicts to write conflict markers",
                    merge_result.conflicts.len()
                ));
            }
            Ok(ApplyHunksResult {
                content: merge_result.content,
                status,
                conflicts: merge_result.conflicts,
            })
        }
    }
}

pub fn merge_hunks(ours: &str, hunks: &[Hunk]) -> Result<MergeResult, MergeError> {
    let eol = detect_eol(ours);
    let mut src_lines: Vec<String> = ours.lines().map(String::from).collect();
    let had_final_newline = ours.ends_with('\n') || ours.ends_with("\r\n") || ours.is_empty();
    let mut offset: isize = 0;
    let mut conflicts = Vec::new();
    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        let expected = hunk_expected_start(hunk, offset).map_err(|msg| MergeError {
            message: format!("hunk {} failed: {msg}", hunk_idx + 1),
        })?;
        let old_refs = hunk_old_refs(hunk);
        let base_lines = hunk_base_lines(hunk);
        let theirs_lines = hunk_theirs_lines(hunk);
        let src_refs: Vec<&str> = src_lines.iter().map(String::as_str).collect();
        let pos = locate_hunk_region(&src_refs, hunk, expected).ok_or_else(|| MergeError {
            message: format!(
                "hunk {} failed: stale context near line {}",
                hunk_idx + 1,
                hunk.old_start
            ),
        })?;
        let old_len = old_refs.len();
        let ours_region: Vec<String> = src_lines[pos..pos + old_len].to_vec();
        let (replacement, hunk_conflicts) =
            if ours_region.iter().map(String::as_str).collect::<Vec<_>>() == old_refs {
                (theirs_lines, Vec::new())
            } else {
                merge_three_way(&base_lines, &ours_region, &theirs_lines, pos + 1)
            };
        conflicts.extend(hunk_conflicts);
        let new_len = replacement.len();
        src_lines.splice(pos..pos + old_len, replacement);
        offset = offset.saturating_add(
            isize::try_from(new_len).unwrap_or(isize::MAX)
                - isize::try_from(old_len).unwrap_or(isize::MAX),
        );
    }
    Ok(MergeResult {
        content: join_lines_with(&src_lines, had_final_newline, eol),
        conflicts,
    })
}

fn hunk_expected_start(hunk: &Hunk, offset: isize) -> Result<isize, String> {
    if hunk.old_start == 0 {
        Ok(0)
    } else {
        isize::try_from(hunk.old_start)
            .ok()
            .and_then(|s| s.checked_sub(1))
            .and_then(|s| s.checked_add(offset))
            .ok_or_else(|| format!("line number {} out of range", hunk.old_start))
    }
}
fn hunk_old_refs(hunk: &Hunk) -> Vec<&str> {
    hunk.lines
        .iter()
        .filter_map(|pl| match pl {
            PatchLine::Context(s) | PatchLine::Remove(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}
fn hunk_base_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|pl| match pl {
            PatchLine::Context(s) | PatchLine::Remove(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}
fn hunk_theirs_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|pl| match pl {
            PatchLine::Context(s) | PatchLine::Add(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}
fn locate_hunk_region(haystack: &[&str], hunk: &Hunk, expected: isize) -> Option<usize> {
    let old_refs = hunk_old_refs(hunk);
    find_match(haystack, &old_refs, expected, FUZZ_RANGE)
        .or_else(|| find_match_global(haystack, &old_refs))
        .or_else(|| locate_by_context_anchors(haystack, hunk, expected))
}
fn locate_by_context_anchors(haystack: &[&str], hunk: &Hunk, expected: isize) -> Option<usize> {
    let old_refs = hunk_old_refs(hunk);
    let base_len = old_refs.len();
    if base_len == 0 {
        return Some((expected.max(0) as usize).min(haystack.len()));
    }
    let (prefix_ctx, suffix_ctx) = hunk_context_anchors(hunk);
    if prefix_ctx.is_empty() && suffix_ctx.is_empty() {
        return None;
    }
    let prefix_refs: Vec<&str> = prefix_ctx.iter().map(String::as_str).collect();
    let pos = if prefix_ctx.is_empty() {
        None
    } else {
        find_match(haystack, &prefix_refs, expected, FUZZ_RANGE)
            .or_else(|| find_match_global(haystack, &prefix_refs))
    };
    let pos = if let Some(pos) = pos {
        pos
    } else if !suffix_ctx.is_empty() {
        let suffix_refs: Vec<&str> = suffix_ctx.iter().map(String::as_str).collect();
        let suffix_expected = expected
            .saturating_add(isize::try_from(base_len).unwrap_or(isize::MAX))
            .saturating_sub(isize::try_from(suffix_refs.len()).unwrap_or(isize::MAX));
        let suffix_pos = find_match(haystack, &suffix_refs, suffix_expected, FUZZ_RANGE)
            .or_else(|| find_match_global(haystack, &suffix_refs))?;
        suffix_pos.saturating_sub(base_len.saturating_sub(suffix_refs.len()))
    } else {
        return None;
    };
    if !suffix_ctx.is_empty() {
        let suffix_start = pos + base_len.saturating_sub(suffix_ctx.len());
        if suffix_start + suffix_ctx.len() > haystack.len() {
            return None;
        }
        let suffix_refs: Vec<&str> = suffix_ctx.iter().map(String::as_str).collect();
        if haystack[suffix_start..suffix_start + suffix_refs.len()] != *suffix_refs {
            return None;
        }
    }
    if pos + base_len > haystack.len() {
        return None;
    }
    Some(pos)
}
fn hunk_context_anchors(hunk: &Hunk) -> (Vec<String>, Vec<String>) {
    // Prefix: leading context lines before the first change.
    let mut prefix_ctx = Vec::new();
    for pl in &hunk.lines {
        match pl {
            PatchLine::Context(s) => prefix_ctx.push(s.clone()),
            _ => break,
        }
    }
    // Suffix: trailing context lines after the last change.
    // Scan backwards so mid-hunk context between change blocks is excluded.
    let mut suffix_ctx = Vec::new();
    for pl in hunk.lines.iter().rev() {
        match pl {
            PatchLine::Context(s) => suffix_ctx.push(s.clone()),
            _ => break,
        }
    }
    suffix_ctx.reverse();
    (prefix_ctx, suffix_ctx)
}
fn merge_three_way(
    base: &[String],
    ours: &[String],
    theirs: &[String],
    region_start_line: usize,
) -> (Vec<String>, Vec<ConflictRange>) {
    if base.len() == ours.len() && base.len() == theirs.len() {
        merge_three_way_lines(base, ours, theirs, region_start_line)
    } else {
        merge_three_way_block(base, ours, theirs, region_start_line)
    }
}
fn merge_three_way_lines(
    base: &[String],
    ours: &[String],
    theirs: &[String],
    region_start_line: usize,
) -> (Vec<String>, Vec<ConflictRange>) {
    let mut out = Vec::new();
    let mut conflicts = Vec::new();
    let mut line_no = region_start_line;
    for i in 0..base.len() {
        let (b, o, t) = (&base[i], &ours[i], &theirs[i]);
        if o == b && t == b {
            out.push(o.clone());
            line_no += 1;
        } else if o == b {
            out.push(t.clone());
            line_no += 1;
        } else if t == b || o == t {
            out.push(o.clone());
            line_no += 1;
        } else {
            let start = line_no;
            out.extend([
                CONFLICT_OURS.to_string(),
                o.clone(),
                CONFLICT_SEP.to_string(),
                t.clone(),
                CONFLICT_THEIRS.to_string(),
            ]);
            conflicts.push(ConflictRange {
                start_line: start,
                end_line: start + 4,
            });
            line_no += 5;
        }
    }
    (out, conflicts)
}
fn merge_three_way_block(
    base: &[String],
    ours: &[String],
    theirs: &[String],
    region_start_line: usize,
) -> (Vec<String>, Vec<ConflictRange>) {
    if ours == base {
        return (theirs.to_vec(), Vec::new());
    }
    if theirs == base {
        return (ours.to_vec(), Vec::new());
    }
    if ours == theirs {
        return (ours.to_vec(), Vec::new());
    }
    let start = region_start_line;
    let mut out = vec![CONFLICT_OURS.to_string()];
    out.extend(ours.iter().cloned());
    out.push(CONFLICT_SEP.to_string());
    out.extend(theirs.iter().cloned());
    out.push(CONFLICT_THEIRS.to_string());
    let end = start + out.len().saturating_sub(1);
    (
        out,
        vec![ConflictRange {
            start_line: start,
            end_line: end,
        }],
    )
}
fn find_match_global(haystack: &[&str], needle: &[&str]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let max_start = haystack.len() - needle.len();
    for pos in 0..=max_start {
        if haystack[pos..pos + needle.len()] == *needle {
            return Some(pos);
        }
    }
    None
}
fn join_lines_with(lines: &[String], final_newline: bool, eol: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join(eol);
    if final_newline {
        out.push_str(eol);
    }
    out
}

/// Detect dominant line ending in the original text.
fn detect_eol(text: &str) -> &'static str {
    crate::write::detect_eol(text)
}

fn find_match(haystack: &[&str], needle: &[&str], expected: isize, fuzz: usize) -> Option<usize> {
    if needle.is_empty() {
        let pos = expected.max(0) as usize;
        return Some(pos.min(haystack.len()));
    }

    for delta in 0..=fuzz {
        for &sign in &[1isize, -1isize] {
            let Some(offset) = isize::try_from(delta).ok() else {
                continue;
            };
            let Some(candidate) = offset
                .checked_mul(sign)
                .and_then(|o| expected.checked_add(o))
            else {
                continue;
            };
            if candidate < 0 {
                continue;
            }
            let pos = candidate as usize;
            if pos + needle.len() > haystack.len() {
                continue;
            }
            if haystack[pos..pos + needle.len()] == *needle {
                return Some(pos);
            }
        }
    }

    None
}

#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn apply_patch_with_loader<F>(
    diff_text: &str,
    mut load_original: F,
    options: ApplyHunksOptions,
) -> anyhow::Result<Vec<PatchApplyFileResult>>
where
    F: FnMut(&str) -> anyhow::Result<String>,
{
    let patch_files = parse_patch(diff_text).map_err(|msg| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("patch parse error: {msg}"),
        })
    })?;
    let mut results = Vec::new();
    for pf in &patch_files {
        if let Some(reason) = pf.unsupported.as_deref() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: unsupported_git_meta_msg(&pf.path, reason),
            }));
        }
        // New file creation: original is empty, don't try to load from disk.
        // Git rename: load from rename_from (old path), write to path (new).
        // Git copy: load from copy_from, write dest, keep source. (#2171)
        let load_path = pf
            .copy_from
            .as_deref()
            .or(pf.rename_from.as_deref())
            .unwrap_or(pf.path.as_str());
        let pure_rename =
            pf.rename_from.is_some() && pf.hunks.is_empty() && !pf.is_deletion && !pf.is_creation;
        let pure_copy = pf.copy_from.is_some() && pf.hunks.is_empty();
        let original = if pf.copy_from.is_some() {
            load_original(load_path)?
        } else if pf.is_creation {
            String::new()
        } else {
            match load_original(load_path) {
                Ok(s) => s,
                // Pure path rename of binary / invalid UTF-8: soft empty snapshot
                // (commit uses fs::rename so bytes stay intact; #2031 parity).
                Err(e)
                    if pure_rename
                        && (crate::exit::is_binary(&e) || crate::exit::is_invalid_encoding(&e)) =>
                {
                    String::new()
                }
                Err(e) => return Err(e),
            }
        };
        // Pure rename (empty hunks): keep content, stage rename only.
        if pure_rename {
            results.push(PatchApplyFileResult {
                path: pf.path.clone(),
                content: original,
                status: ApplyHunksStatus::Clean,
                conflicts: Vec::new(),
                is_creation: false,
                is_deletion: false,
                rename_from: pf.rename_from.clone(),
                copy_from: None,
            });
            continue;
        }
        // Pure copy (empty hunks): write dest, keep source.
        if pure_copy {
            results.push(PatchApplyFileResult {
                path: pf.path.clone(),
                content: original,
                status: ApplyHunksStatus::Clean,
                conflicts: Vec::new(),
                is_creation: true,
                is_deletion: false,
                rename_from: None,
                copy_from: pf.copy_from.clone(),
            });
            continue;
        }
        // File deletion: still run hunk application so stale context fails
        // closed (check and apply must agree). Skipping hunks always deleted
        // even when the file was edited after the patch was made.
        if pf.is_deletion {
            if pf.hunks.is_empty() {
                // Pure delete marker without hunks: remove as-is.
                results.push(PatchApplyFileResult {
                    path: pf.path.clone(),
                    content: String::new(),
                    status: ApplyHunksStatus::Clean,
                    conflicts: Vec::new(),
                    is_creation: false,
                    is_deletion: true,
                    rename_from: None,
                    copy_from: None,
                });
                continue;
            }
            let applied = match apply_hunks_with_options(&original, &pf.hunks, options) {
                Ok(applied) => applied,
                Err(msg) if msg.contains("conflict(s)") => {
                    return Err(crate::exit::ConflictsError {
                        msg: format!("patch apply: {} -- {msg}", pf.path),
                    }
                    .into());
                }
                Err(msg) if msg.contains("stale context") => {
                    return Err(crate::exit::AmbiguousError {
                        msg: format!("patch apply: {} -- {msg}", pf.path),
                    }
                    .into());
                }
                Err(msg) => {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("patch apply: {} -- {msg}", pf.path),
                    }));
                }
            };
            // Only treat as deletion when context matched (Clean empty result).
            // Stale/conflict already returned Err above under OnStale::Fail.
            let is_clean_delete =
                applied.status == ApplyHunksStatus::Clean && applied.content.is_empty();
            results.push(PatchApplyFileResult {
                path: pf.path.clone(),
                content: applied.content,
                status: applied.status,
                conflicts: applied.conflicts,
                is_creation: false,
                is_deletion: is_clean_delete,
                rename_from: None,
                copy_from: None,
            });
            continue;
        }
        let applied = match apply_hunks_with_options(&original, &pf.hunks, options) {
            Ok(applied) => applied,
            // Merge conflicts without allow_conflicts: typed kind so CLI/tx
            // map to exit CONFLICTS (8) and error_kind "conflicts".
            Err(msg) if msg.contains("conflict(s)") => {
                return Err(crate::exit::ConflictsError {
                    msg: format!("patch apply: {} -- {msg}", pf.path),
                }
                .into());
            }
            Err(msg) if msg.contains("stale context") => {
                return Err(crate::exit::AmbiguousError {
                    msg: format!("patch apply: {} -- {msg}", pf.path),
                }
                .into());
            }
            Err(msg) => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("patch apply: {} -- {msg}", pf.path),
                }));
            }
        };
        results.push(PatchApplyFileResult {
            path: pf.path.clone(),
            content: applied.content,
            status: applied.status,
            conflicts: applied.conflicts,
            is_creation: pf.is_creation,
            is_deletion: false,
            rename_from: pf.rename_from.clone(),
            copy_from: pf.copy_from.clone(),
        });
    }
    Ok(results)
}

#[path = "patch_tests.rs"]
#[cfg(test)]
mod tests;
