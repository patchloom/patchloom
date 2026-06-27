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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

pub fn parse_patch(input: &str) -> Result<Vec<PatchFile>, String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut files: Vec<PatchFile> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if !lines[i].starts_with("--- ") {
            i += 1;
            continue;
        }

        if i + 1 >= lines.len() || !lines[i + 1].starts_with("+++ ") {
            return Err(format!("expected +++ line after --- at line {}", i + 1));
        }

        let path = parse_file_path(lines[i + 1]);
        i += 2;

        let mut hunks: Vec<Hunk> = Vec::new();
        while i < lines.len() && !lines[i].starts_with("--- ") {
            if lines[i].starts_with("@@ ") {
                let hunk = parse_hunk_header(lines[i])?;
                let mut hunk_lines: Vec<PatchLine> = Vec::new();
                i += 1;

                while i < lines.len()
                    && !lines[i].starts_with("@@ ")
                    && !lines[i].starts_with("--- ")
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
                });
            } else {
                i += 1;
            }
        }

        if hunks.is_empty() {
            return Err(format!("no hunks found for file {path}"));
        }

        files.push(PatchFile { path, hunks });
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

    raw.strip_prefix("b/")
        .or_else(|| raw.strip_prefix("a/"))
        .unwrap_or(raw)
        .to_string()
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
}

pub fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut src_lines: Vec<String> = original.lines().map(String::from).collect();
    let had_final_newline = original.ends_with('\n') || original.is_empty();
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
        src_lines.splice(pos..pos + old_len, new_lines);
        let delta = isize::try_from(new_len).unwrap_or(isize::MAX)
            - isize::try_from(old_len).unwrap_or(isize::MAX);
        offset = offset.saturating_add(delta);
    }

    Ok(join_lines(&src_lines, had_final_newline))
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
    let mut src_lines: Vec<String> = ours.lines().map(String::from).collect();
    let had_final_newline = ours.ends_with('\n') || ours.is_empty();
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
        content: join_lines(&src_lines, had_final_newline),
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
    let mut prefix_ctx = Vec::new();
    let mut suffix_ctx = Vec::new();
    let mut in_change = false;
    for pl in &hunk.lines {
        match pl {
            PatchLine::Context(s) if !in_change => prefix_ctx.push(s.clone()),
            PatchLine::Remove(_) | PatchLine::Add(_) => in_change = true,
            PatchLine::Context(s) if in_change => suffix_ctx.push(s.clone()),
            _ => {}
        }
    }
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
    let max_start = haystack.len().saturating_sub(needle.len());
    for pos in 0..=max_start {
        if haystack[pos..pos + needle.len()] == *needle {
            return Some(pos);
        }
    }
    None
}
fn join_lines(lines: &[String], final_newline: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    if final_newline {
        out.push('\n');
    }
    out
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
    let patch_files =
        parse_patch(diff_text).map_err(|msg| anyhow::anyhow!("patch parse error: {msg}"))?;
    let mut results = Vec::new();
    for pf in &patch_files {
        let original = load_original(&pf.path)?;
        let applied = apply_hunks_with_options(&original, &pf.hunks, options)
            .map_err(|msg| anyhow::anyhow!("patch apply: {} -- {msg}", pf.path))?;
        results.push(PatchApplyFileResult {
            path: pf.path.clone(),
            content: applied.content,
            status: applied.status,
            conflicts: applied.conflicts,
        });
    }
    Ok(results)
}

#[path = "patch_tests.rs"]
#[cfg(test)]
mod tests;
