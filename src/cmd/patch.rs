use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_write, WritePolicy};
use clap::Args;

#[derive(Debug, Args)]
pub struct PatchArgs {
    #[command(subcommand)]
    pub action: PatchAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum PatchAction {
    /// Check whether a patch applies cleanly.
    Check {
        /// Path to a diff file.
        #[arg(long)]
        file: Option<String>,
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
    /// Apply a unified diff.
    Apply {
        /// Path to a diff file.
        #[arg(long)]
        file: Option<String>,
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
}

// ── Unified diff types ──────────────────────────────────────────────

/// A single line inside a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PatchLine {
    Context(String),
    Remove(String),
    Add(String),
}

/// A contiguous hunk inside a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<PatchLine>,
}

/// All hunks that apply to a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PatchFile {
    path: String,
    hunks: Vec<Hunk>,
}

// ── Parser ──────────────────────────────────────────────────────────

/// Parse a standard unified diff into a list of per-file patches.
fn parse_patch(input: &str) -> Result<Vec<PatchFile>, String> {
    let lines: Vec<&str> = input.lines().collect();
    let mut files: Vec<PatchFile> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        // Skip leading lines that are not `---` headers (e.g. `diff --git` lines).
        if !lines[i].starts_with("--- ") {
            i += 1;
            continue;
        }

        // We found a `--- ` line.  The next line must be `+++ `.
        if i + 1 >= lines.len() || !lines[i + 1].starts_with("+++ ") {
            return Err(format!("expected +++ line after --- at line {}", i + 1));
        }

        let path = parse_file_path(lines[i + 1]);
        i += 2; // advance past --- and +++

        let mut hunks: Vec<Hunk> = Vec::new();

        // Collect hunks until we hit the next file header or EOF.
        while i < lines.len() && !lines[i].starts_with("--- ") {
            if lines[i].starts_with("@@ ") {
                let hunk = parse_hunk_header(lines[i])?;
                let mut hunk_lines: Vec<PatchLine> = Vec::new();
                i += 1;

                while i < lines.len()
                    && !lines[i].starts_with("@@ ")
                    && !lines[i].starts_with("--- ")
                {
                    let line = lines[i];
                    if let Some(rest) = line.strip_prefix('+') {
                        hunk_lines.push(PatchLine::Add(rest.to_string()));
                    } else if let Some(rest) = line.strip_prefix('-') {
                        hunk_lines.push(PatchLine::Remove(rest.to_string()));
                    } else if let Some(rest) = line.strip_prefix(' ') {
                        hunk_lines.push(PatchLine::Context(rest.to_string()));
                    } else if line == "\\ No newline at end of file" {
                        // informational marker – skip
                    } else {
                        // Treat bare line as context (some diffs omit the leading space
                        // for empty context lines).
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

/// Extract the file path from a `+++ b/path` or `+++ path` line.
fn parse_file_path(line: &str) -> String {
    let raw = line
        .strip_prefix("+++ ")
        .or_else(|| line.strip_prefix("--- "))
        .unwrap_or(line);

    // Strip the conventional `a/` or `b/` prefix.
    raw.strip_prefix("b/")
        .or_else(|| raw.strip_prefix("a/"))
        .unwrap_or(raw)
        .to_string()
}

/// Parse a `@@ -old_start,old_count +new_start,new_count @@` header.
fn parse_hunk_header(line: &str) -> Result<Hunk, String> {
    // Format: @@ -<start>,<count> +<start>,<count> @@...
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

/// Parse `start,count` or `start` (count defaults to 1).
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

// ── Hunk application ────────────────────────────────────────────────

/// Maximum number of lines to search above/below the expected position when
/// context does not match exactly.
const FUZZ_RANGE: usize = 3;

/// Apply a sequence of hunks to `original`, returning the patched text.
fn apply_hunks(original: &str, hunks: &[Hunk]) -> Result<String, String> {
    let mut src_lines: Vec<String> = original.lines().map(String::from).collect();

    // Track whether the original ended with a newline so we can preserve it.
    let had_final_newline = original.ends_with('\n') || original.is_empty();

    // We process hunks in order.  `offset` tracks accumulated shift caused by
    // previous hunks (added lines minus removed lines).
    let mut offset: isize = 0;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        // Expected 0-based start in the *current* source.
        let expected: isize = if hunk.old_start == 0 {
            0
        } else {
            hunk.old_start as isize - 1 + offset
        };

        // Build the "old" context lines we expect to find.
        let old_lines: Vec<String> = hunk
            .lines
            .iter()
            .filter_map(|pl| match pl {
                PatchLine::Context(s) => Some(s.clone()),
                PatchLine::Remove(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        // Borrow as &str for matching.
        let src_refs: Vec<&str> = src_lines.iter().map(|s| s.as_str()).collect();
        let old_refs: Vec<&str> = old_lines.iter().map(|s| s.as_str()).collect();

        // Try to find a position where old_lines match.
        let pos = find_match(&src_refs, &old_refs, expected, FUZZ_RANGE).ok_or_else(|| {
            let snippet = old_lines
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "hunk {} failed: stale context near line {} — expected:\n{}",
                hunk_idx + 1,
                hunk.old_start,
                snippet,
            )
        })?;

        // Build replacement lines.
        let new_lines: Vec<String> = hunk
            .lines
            .iter()
            .filter_map(|pl| match pl {
                PatchLine::Context(s) => Some(s.clone()),
                PatchLine::Add(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        let old_len = old_lines.len();
        let new_len = new_lines.len();

        // Splice: remove the old_lines span, insert new_lines.
        src_lines.splice(pos..pos + old_len, new_lines);

        offset += new_len as isize - old_len as isize;
    }

    Ok(join_lines(&src_lines, had_final_newline))
}

/// Join lines back into a single string, optionally appending a final newline.
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

/// Try to find `needle` lines in `haystack` starting at `expected` (0-based),
/// then searching up to `fuzz` lines away.
fn find_match(haystack: &[&str], needle: &[&str], expected: isize, fuzz: usize) -> Option<usize> {
    if needle.is_empty() {
        // A pure-addition hunk: insert at the expected position.
        let pos = expected.max(0) as usize;
        return Some(pos.min(haystack.len()));
    }

    // Try exact position first, then offsets ±1, ±2, …, ±fuzz.
    for delta in 0..=fuzz {
        for &sign in &[1isize, -1isize] {
            let candidate = expected + (delta as isize) * sign;
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

// ── Read diff input ────────────────────────────────────────────────

/// Read diff text from the source indicated by the user.
fn read_diff_input(file: &Option<String>, stdin_flag: bool) -> Result<String, u8> {
    if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|_| exit::PARSE_ERROR)
    } else if stdin_flag {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|_| exit::PARSE_ERROR)?;
        Ok(buf)
    } else {
        Err(exit::PARSE_ERROR)
    }
}

/// Build a [`WritePolicy`] from [`GlobalFlags`].
fn policy_from_global(global: &GlobalFlags) -> WritePolicy {
    WritePolicy {
        ensure_final_newline: global.ensure_final_newline,
        normalize_eol: global.normalize_eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: global.trim_trailing_whitespace,
    }
}

// ── Public entry point ──────────────────────────────────────────────

pub fn run(args: PatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (file, stdin_flag) = match &args.action {
        PatchAction::Check { file, stdin } => (file.clone(), *stdin),
        PatchAction::Apply { file, stdin } => (file.clone(), *stdin),
    };

    // Read diff input.
    let diff_text = match read_diff_input(&file, stdin_flag) {
        Ok(text) => text,
        Err(code) => {
            eprintln!("patch: must specify --file <path> or --stdin");
            return Ok(code);
        }
    };

    // Parse the unified diff.
    let patch_files = match parse_patch(&diff_text) {
        Ok(pf) => pf,
        Err(msg) => {
            eprintln!("patch: parse error: {msg}");
            return Ok(exit::PARSE_ERROR);
        }
    };

    let root = if let Some(ref cwd) = global.cwd {
        std::path::PathBuf::from(cwd)
    } else {
        std::env::current_dir()?
    };

    match args.action {
        PatchAction::Check { .. } => {
            let mut all_clean = true;
            for pf in &patch_files {
                let file_path = root.join(&pf.path);
                let original = std::fs::read_to_string(&file_path).unwrap_or_default();
                match apply_hunks(&original, &pf.hunks) {
                    Ok(_) => {
                        eprintln!("patch check: {} — clean", pf.path);
                    }
                    Err(msg) => {
                        eprintln!("patch check: {} — STALE: {}", pf.path, msg);
                        all_clean = false;
                    }
                }
            }
            if all_clean {
                Ok(exit::SUCCESS)
            } else {
                Ok(exit::AMBIGUOUS)
            }
        }
        PatchAction::Apply { .. } => {
            let policy = policy_from_global(global);
            let mut diffs = Vec::new();

            for pf in &patch_files {
                let file_path = root.join(&pf.path);
                let original = std::fs::read_to_string(&file_path).unwrap_or_default();
                let patched = match apply_hunks(&original, &pf.hunks) {
                    Ok(p) => p,
                    Err(msg) => {
                        eprintln!("patch apply: {} — STALE: {}", pf.path, msg);
                        return Ok(exit::AMBIGUOUS);
                    }
                };

                if global.diff {
                    diffs.push(unified_diff(&pf.path, &original, &patched));
                } else {
                    atomic_write(&file_path, &patched, &policy)?;
                    eprintln!("patch apply: {} — written", pf.path);
                }
            }

            if global.diff {
                let result = DiffResult {
                    diffs,
                    total_files_changed: patch_files.len(),
                };
                print!("{}", format_diff_result(&result));
            }

            Ok(exit::SUCCESS)
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    /// Helper: default `GlobalFlags` pointing at a directory.
    fn flags_for(dir: &std::path::Path) -> GlobalFlags {
        GlobalFlags {
            json: false,
            jsonl: false,
            diff: false,
            apply: false,
            check: false,
            cwd: Some(dir.to_string_lossy().to_string()),
            glob: None,
            atomic: false,
            ensure_final_newline: false,
            normalize_eol: None,
            trim_trailing_whitespace: false,
        }
    }

    // ── parse_patch tests ───────────────────────────────────────────

    #[test]
    fn parse_simple_single_file_diff() {
        let diff = "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "hello.txt");
        assert_eq!(files[0].hunks.len(), 1);

        let h = &files[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 3);
        assert_eq!(h.lines.len(), 4);
        assert_eq!(h.lines[0], PatchLine::Context("line1".into()));
        assert_eq!(h.lines[1], PatchLine::Remove("old line".into()));
        assert_eq!(h.lines[2], PatchLine::Add("new line".into()));
        assert_eq!(h.lines[3], PatchLine::Context("line3".into()));
    }

    #[test]
    fn parse_multi_file_diff() {
        let diff = "\
--- a/file1.txt
+++ b/file1.txt
@@ -1,2 +1,2 @@
 aaa
-bbb
+ccc
--- a/file2.txt
+++ b/file2.txt
@@ -1,2 +1,2 @@
 xxx
-yyy
+zzz
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "file1.txt");
        assert_eq!(files[1].path, "file2.txt");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[1].hunks.len(), 1);
    }

    // ── apply_hunks tests ───────────────────────────────────────────

    #[test]
    fn apply_simple_hunk_correctly() {
        let original = "line1\nold line\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "line1\nnew line\nline3\n");
    }

    #[test]
    fn stale_context_returns_error() {
        let original = "line1\ncompletely different\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stale context"));
    }

    #[test]
    fn apply_with_fuzz_offset() {
        // The hunk says old_start=2, but the matching context is actually at
        // line 4 (0-based: 3).  Within fuzz range of 3, so it should still
        // apply.
        let original = "extra1\nextra2\nline1\nold line\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "extra1\nextra2\nline1\nnew line\nline3\n");
    }

    // ── Integration: check subcommand ───────────────────────────────

    #[test]
    fn check_reports_clean_apply() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\nold line\nline3\n").unwrap();

        let diff_path = tmp.path().join("change.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().to_string()),
                stdin: false,
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn check_reports_stale_context_with_exit_5() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

        let diff_path = tmp.path().join("stale.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().to_string()),
                stdin: false,
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::AMBIGUOUS);
    }

    // ── Integration: apply subcommand ───────────────────────────────

    #[test]
    fn apply_writes_patched_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\nold line\nline3\n").unwrap();

        let diff_path = tmp.path().join("change.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Apply {
                file: Some(diff_path.to_string_lossy().to_string()),
                stdin: false,
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "line1\nnew line\nline3\n");
    }

    // ── Malformed diff ──────────────────────────────────────────────

    #[test]
    fn malformed_diff_returns_parse_error() {
        let tmp = TempDir::new().unwrap();
        let diff_path = tmp.path().join("bad.patch");
        std::fs::write(&diff_path, "this is not a diff at all\n").unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().to_string()),
                stdin: false,
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn no_input_source_returns_parse_error() {
        let tmp = TempDir::new().unwrap();
        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: None,
                stdin: false,
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    // ── Additional parser edge-case tests ───────────────────────────

    #[test]
    fn parse_diff_with_git_prefix_lines() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
index abc1234..def5678 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,2 +1,2 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"world\");
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "foo.rs");
    }

    #[test]
    fn parse_hunk_without_comma_count() {
        // Single-line ranges like `@@ -1 +1 @@` (count defaults to 1).
        let diff = "\
--- a/one.txt
+++ b/one.txt
@@ -1 +1 @@
-old
+new
";
        let files = parse_patch(diff).unwrap();
        let h = &files[0].hunks[0];
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_count, 1);
    }
}
