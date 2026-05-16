use crate::cli::global::GlobalFlags;
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_write, policy_from_flags};
use clap::Args;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Args)]
pub struct MdArgs {
    #[command(subcommand)]
    pub action: MdAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum MdAction {
    /// Replace a heading section.
    ReplaceSection {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        /// Read replacement content from stdin.
        #[arg(long)]
        stdin: bool,
        /// Replacement content as argument.
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content after a heading.
    InsertAfterHeading {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content before a heading.
    InsertBeforeHeading {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        content: Option<String>,
    },
    /// Add a bullet under a heading if not already present.
    UpsertBullet {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        bullet: String,
    },
    /// Remove duplicate headings.
    DedupeHeadings {
        #[arg(long)]
        file: String,
    },
    /// Lint common AGENTS.md problems.
    LintAgents {
        #[arg(long)]
        file: String,
    },
    /// Append a row to a markdown table under a heading.
    TableAppend {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        /// The row to append, in markdown table format (e.g., "| col1 | col2 | col3 |").
        #[arg(long)]
        row: String,
    },
}

// ---------------------------------------------------------------------------
// Markdown section parser
// ---------------------------------------------------------------------------

/// Information about a single heading in a markdown document.
#[derive(Debug, Clone)]
pub(crate) struct HeadingInfo {
    /// Number of `#` characters (1–6).
    pub level: usize,
    /// Heading text after `# `.
    pub text: String,
    /// 0-based line number of the heading line.
    pub line_start: usize,
    /// 0-based line number (exclusive) where the section ends.
    pub line_end: usize,
}

/// Parse all headings from markdown content.
///
/// A heading line matches `^#{1,6} `. Each heading's `line_end` is set to
/// the line number of the next heading at the same or higher level (fewer
/// or equal `#` characters), or the total number of lines.
pub(crate) fn parse_headings(content: &str) -> Vec<HeadingInfo> {
    let lines: Vec<&str> = content.lines().collect();
    let mut headings = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if !line.starts_with('#') {
            continue;
        }
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if hashes > 6 || hashes >= line.len() {
            continue;
        }
        if line.as_bytes()[hashes] != b' ' {
            continue;
        }
        headings.push(HeadingInfo {
            level: hashes,
            text: line[hashes + 1..].to_string(),
            line_start: idx,
            line_end: 0, // filled below
        });
    }

    let total = lines.len();
    for i in 0..headings.len() {
        let lvl = headings[i].level;
        let mut end = total;
        for h in headings.iter().skip(i + 1) {
            if h.level <= lvl {
                end = h.line_start;
                break;
            }
        }
        headings[i].line_end = end;
    }

    headings
}

/// Byte offset of the start of each line. The vector always begins with `0`.
fn line_byte_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Strip leading `#` prefix from a heading query so both `"Title"` and
/// `"## Title"` match a `## Title` heading.
fn normalize_heading_query(heading: &str) -> &str {
    let t = heading.trim();
    let n = t.bytes().take_while(|&b| b == b'#').count();
    if n > 0 && t.len() > n && t.as_bytes()[n] == b' ' {
        t[n + 1..].trim()
    } else {
        t
    }
}

/// Find a section by heading text.
///
/// Returns `(body_start_byte, body_end_byte)` where the body starts
/// immediately after the heading line and ends at the start of the next
/// heading of same-or-higher level, or at EOF.
pub(crate) fn find_section(content: &str, heading: &str) -> Option<(usize, usize)> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let query = normalize_heading_query(heading);

    for h in &headings {
        if h.text.trim() == query {
            let body_start = if h.line_start + 1 < offsets.len() {
                offsets[h.line_start + 1]
            } else {
                content.len()
            };
            let body_end = if h.line_end < offsets.len() {
                offsets[h.line_end]
            } else {
                content.len()
            };
            return Some((body_start, body_end));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers: stdin, write policy, mutation driver
// ---------------------------------------------------------------------------

fn read_content(use_stdin: bool, content: &Option<String>) -> anyhow::Result<String> {
    if use_stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else if let Some(c) = content {
        Ok(c.clone())
    } else {
        anyhow::bail!("one of --stdin or --content must be provided")
    }
}

/// Compare original with policy-applied new content, then diff/check/apply.
fn apply_mutation(
    file: &str,
    original: &str,
    new_content: &str,
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    let path = Path::new(file);
    let policy = policy_from_flags(global, Some(path));
    let final_content = crate::write::apply_policy(new_content, &policy);
    let has_changes = original != final_content;

    if global.diff {
        let d = unified_diff(file, original, &final_content);
        if d.has_changes {
            let result = DiffResult {
                diffs: vec![d],
                total_files_changed: 1,
            };
            print!("{}", format_diff_result(&result));
        }
    }

    if global.check && has_changes {
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        atomic_write(path, new_content, &policy)?;
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// Content mutation functions (pure, no I/O)
// ---------------------------------------------------------------------------

pub(crate) fn replace_section_in(
    content: &str,
    heading: &str,
    replacement: &str,
) -> Option<String> {
    let (body_start, body_end) = find_section(content, heading)?;
    let mut out = String::with_capacity(content.len());
    out.push_str(&content[..body_start]);
    if !replacement.is_empty() {
        out.push_str(replacement);
        if !replacement.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(&content[body_end..]);
    Some(out)
}

pub(crate) fn insert_after_heading_in(
    content: &str,
    heading: &str,
    insertion: &str,
) -> Option<String> {
    let (body_start, _) = find_section(content, heading)?;
    let mut out = String::with_capacity(content.len() + insertion.len());
    out.push_str(&content[..body_start]);
    out.push_str(insertion);
    if !insertion.is_empty() && !insertion.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&content[body_start..]);
    Some(out)
}

pub(crate) fn insert_before_heading_in(
    content: &str,
    heading: &str,
    insertion: &str,
) -> Option<String> {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let query = normalize_heading_query(heading);

    for h in &headings {
        if h.text.trim() == query {
            let heading_start = offsets[h.line_start];
            let mut out = String::with_capacity(content.len() + insertion.len());
            out.push_str(&content[..heading_start]);
            if !insertion.is_empty() {
                out.push_str(insertion);
                if !insertion.ends_with('\n') {
                    out.push('\n');
                }
                // Ensure a blank line between inserted content and the heading.
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
            }
            out.push_str(&content[heading_start..]);
            return Some(out);
        }
    }
    None
}

pub(crate) fn upsert_bullet_in(content: &str, heading: &str, bullet: &str) -> Option<String> {
    let (body_start, body_end) = find_section(content, heading)?;
    let body = &content[body_start..body_end];

    // Normalise: ensure bullet starts with `- `.
    let trimmed = bullet.trim();
    let normalized = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        trimmed.to_string()
    } else {
        format!("- {trimmed}")
    };

    // Already present → idempotent no-op.
    for line in body.lines() {
        if line.trim() == normalized {
            return Some(content.to_string());
        }
    }

    // Append bullet at the end of the section body.
    let mut out = String::with_capacity(content.len() + normalized.len() + 2);
    out.push_str(&content[..body_end]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&normalized);
    out.push('\n');
    out.push_str(&content[body_end..]);
    Some(out)
}

pub(crate) fn dedupe_headings_in(content: &str) -> (String, Vec<String>) {
    let headings = parse_headings(content);
    let offsets = line_byte_starts(content);
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut removed: Vec<String> = Vec::new();

    for h in &headings {
        let key = (h.level, h.text.trim().to_string());
        if seen.contains(&key) {
            let start = offsets[h.line_start];
            let end = if h.line_end < offsets.len() {
                offsets[h.line_end]
            } else {
                content.len()
            };
            ranges.push((start, end));
            removed.push(format!("{} {}", "#".repeat(h.level), h.text));
        } else {
            seen.insert(key);
        }
    }

    let mut out = String::with_capacity(content.len());
    let mut pos = 0;
    for (start, end) in &ranges {
        if *start < pos {
            // Overlapping with a previously removed range – skip.
            continue;
        }
        out.push_str(&content[pos..*start]);
        pos = *end;
    }
    out.push_str(&content[pos..]);

    (out, removed)
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.len() > 1 && t.starts_with('|') && t.ends_with('|')
}

fn is_separator_row(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 || !t.starts_with('|') || !t.ends_with('|') {
        return false;
    }
    t[1..t.len() - 1]
        .chars()
        .all(|c| matches!(c, '-' | ':' | '|' | ' '))
}

fn table_append_in(content: &str, body_start: usize, body_end: usize, row: &str) -> Option<String> {
    let body = &content[body_start..body_end];
    let mut last_data_end: Option<usize> = None;
    let mut in_table = false;
    let mut pos = body_start;

    for line in body.lines() {
        let line_byte_end = pos + line.len();
        let next_pos = if content.as_bytes().get(line_byte_end) == Some(&b'\r')
            && content.as_bytes().get(line_byte_end + 1) == Some(&b'\n')
        {
            line_byte_end + 2
        } else if content.as_bytes().get(line_byte_end) == Some(&b'\n') {
            line_byte_end + 1
        } else {
            line_byte_end
        };

        if is_table_row(line) {
            in_table = true;
            if !is_separator_row(line) {
                last_data_end = Some(next_pos);
            }
        } else if in_table {
            break;
        }

        pos = next_pos;
    }

    let insert_pos = last_data_end?;

    let mut out = String::with_capacity(content.len() + row.len() + 2);
    out.push_str(&content[..insert_pos]);
    out.push_str(row);
    if !row.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&content[insert_pos..]);
    Some(out)
}

// ---------------------------------------------------------------------------
// Lint
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct LintIssue {
    issue: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading: Option<String>,
}

fn lint_agents_content(content: &str) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    // 1. Duplicate headings (same text at same level).
    let headings = parse_headings(content);
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    for h in &headings {
        let key = (h.level, h.text.trim().to_string());
        if seen.contains(&key) {
            issues.push(LintIssue {
                issue: "duplicate heading".to_string(),
                line: Some(h.line_start + 1), // 1-based
                heading: Some(format!("{} {}", "#".repeat(h.level), h.text)),
            });
        } else {
            seen.insert(key);
        }
    }

    // 2. Dangerous git add commands.
    for (idx, line) in content.lines().enumerate() {
        if line.contains("git add .") || line.contains("git add -A") {
            issues.push(LintIssue {
                issue: "dangerous command".to_string(),
                line: Some(idx + 1),
                heading: None,
            });
        }
    }

    // 3. Missing final newline.
    if !content.is_empty() && !content.ends_with('\n') {
        issues.push(LintIssue {
            issue: "missing final newline".to_string(),
            line: None,
            heading: None,
        });
    }

    issues
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: MdArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    match args.action {
        MdAction::ReplaceSection {
            file,
            heading,
            stdin,
            content,
        } => {
            let replacement = read_content(stdin, &content)?;
            let original = std::fs::read_to_string(&file)?;
            match replace_section_in(&original, &heading, &replacement) {
                Some(new) => apply_mutation(&file, &original, &new, global),
                None => Ok(exit::NO_MATCHES),
            }
        }

        MdAction::InsertAfterHeading {
            file,
            heading,
            stdin,
            content,
        } => {
            let insertion = read_content(stdin, &content)?;
            let original = std::fs::read_to_string(&file)?;
            match insert_after_heading_in(&original, &heading, &insertion) {
                Some(new) => apply_mutation(&file, &original, &new, global),
                None => Ok(exit::NO_MATCHES),
            }
        }

        MdAction::InsertBeforeHeading {
            file,
            heading,
            stdin,
            content,
        } => {
            let insertion = read_content(stdin, &content)?;
            let original = std::fs::read_to_string(&file)?;
            match insert_before_heading_in(&original, &heading, &insertion) {
                Some(new) => apply_mutation(&file, &original, &new, global),
                None => Ok(exit::NO_MATCHES),
            }
        }

        MdAction::UpsertBullet {
            file,
            heading,
            bullet,
        } => {
            let original = std::fs::read_to_string(&file)?;
            match upsert_bullet_in(&original, &heading, &bullet) {
                Some(new) => apply_mutation(&file, &original, &new, global),
                None => Ok(exit::NO_MATCHES),
            }
        }

        MdAction::DedupeHeadings { file } => {
            let original = std::fs::read_to_string(&file)?;
            let (new, removed) = dedupe_headings_in(&original);
            if !removed.is_empty() {
                if global.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&removed).expect("serialize removed")
                    );
                } else if !global.quiet {
                    for h in &removed {
                        eprintln!("removed duplicate: {h}");
                    }
                }
            }
            apply_mutation(&file, &original, &new, global)
        }

        MdAction::LintAgents { file } => {
            let content = std::fs::read_to_string(&file)?;
            let issues = lint_agents_content(&content);

            if global.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&issues).expect("serialize lint issues")
                );
            } else if global.jsonl {
                for issue in &issues {
                    println!(
                        "{}",
                        serde_json::to_string(issue).expect("serialize lint issue")
                    );
                }
            } else {
                for issue in &issues {
                    match (issue.line, &issue.heading) {
                        (Some(ln), Some(h)) => {
                            println!("{file}:{ln}: {} {h:?}", issue.issue);
                        }
                        (Some(ln), None) => {
                            println!("{file}:{ln}: {}", issue.issue);
                        }
                        _ => {
                            println!("{file}: {}", issue.issue);
                        }
                    }
                }
            }

            if issues.is_empty() {
                Ok(exit::SUCCESS)
            } else {
                Ok(exit::CHANGES_DETECTED)
            }
        }

        MdAction::TableAppend { file, heading, row } => {
            let original = std::fs::read_to_string(&file)?;
            match find_section(&original, &heading) {
                None => Ok(exit::NO_MATCHES),
                Some((body_start, body_end)) => {
                    match table_append_in(&original, body_start, body_end, &row) {
                        Some(new) => apply_mutation(&file, &original, &new, global),
                        None => {
                            anyhow::bail!("no markdown table found under heading {:?}", heading)
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Default global flags with `--apply` enabled for write tests.
    fn default_global() -> GlobalFlags {
        GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        }
    }

    // -- replace-section ----------------------------------------------------

    #[test]
    fn replace_section_replaces_correct_content() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\nold content\n# Other\nkept\n").unwrap();

        let args = MdArgs {
            action: MdAction::ReplaceSection {
                file: file.to_str().unwrap().to_string(),
                heading: "Title".into(),
                stdin: false,
                content: Some("new content".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(result.contains("new content"), "missing new content");
        assert!(!result.contains("old content"), "old content still present");
        assert!(
            result.contains("# Other\nkept\n"),
            "adjacent section damaged"
        );
    }

    #[test]
    fn replace_section_missing_heading_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\ncontent\n").unwrap();

        let args = MdArgs {
            action: MdAction::ReplaceSection {
                file: file.to_str().unwrap().to_string(),
                heading: "Missing".into(),
                stdin: false,
                content: Some("new".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    // -- insert-after-heading -----------------------------------------------

    #[test]
    fn insert_after_heading_inserts_at_correct_position() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\nexisting\n").unwrap();

        let args = MdArgs {
            action: MdAction::InsertAfterHeading {
                file: file.to_str().unwrap().to_string(),
                heading: "Title".into(),
                stdin: false,
                content: Some("inserted".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(
            result.starts_with("# Title\ninserted\n"),
            "insertion not at correct position: {result}"
        );
        assert!(result.contains("existing"), "existing content lost");
    }

    // -- upsert-bullet ------------------------------------------------------

    #[test]
    fn upsert_bullet_adds_new_bullet() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# List\n- item1\n").unwrap();

        let args = MdArgs {
            action: MdAction::UpsertBullet {
                file: file.to_str().unwrap().to_string(),
                heading: "List".into(),
                bullet: "item2".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(result.contains("- item2"), "new bullet missing");
        assert!(result.contains("- item1"), "existing bullet lost");
    }

    #[test]
    fn upsert_bullet_is_idempotent_when_bullet_exists() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# List\n- item1\n").unwrap();

        let args = MdArgs {
            action: MdAction::UpsertBullet {
                file: file.to_str().unwrap().to_string(),
                heading: "List".into(),
                bullet: "- item1".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert_eq!(
            result.matches("- item1").count(),
            1,
            "bullet duplicated: {result}"
        );
    }

    // -- dedupe-headings ----------------------------------------------------

    #[test]
    fn dedupe_headings_removes_duplicate_headings() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(
            &file,
            "# Section\nfirst\n# Section\nsecond\n# Other\nkept\n",
        )
        .unwrap();

        let args = MdArgs {
            action: MdAction::DedupeHeadings {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert_eq!(
            result.matches("# Section").count(),
            1,
            "duplicate not removed: {result}"
        );
        assert!(result.contains("first"), "first occurrence body lost");
        assert!(!result.contains("second"), "duplicate body remains");
        assert!(
            result.contains("# Other\nkept\n"),
            "unrelated section damaged"
        );
    }

    // -- lint-agents --------------------------------------------------------

    #[test]
    fn lint_agents_finds_duplicate_headings() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nfoo\n# Rules\nbar\n").unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn lint_agents_finds_dangerous_git_add_dot() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nRun git add . to stage\n").unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    // -- final newline ------------------------------------------------------

    #[test]
    fn final_newline_is_preserved_on_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\ncontent\n").unwrap();

        let mut global = default_global();
        global.ensure_final_newline = true;

        let args = MdArgs {
            action: MdAction::ReplaceSection {
                file: file.to_str().unwrap().to_string(),
                heading: "Title".into(),
                stdin: false,
                content: Some("replaced".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(result.ends_with('\n'), "final newline missing: {result:?}");
    }

    // -- parser unit tests --------------------------------------------------

    #[test]
    fn parse_headings_basic() {
        let content = "# A\ntext\n## B\nmore\n# C\n";
        let h = parse_headings(content);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].level, 1);
        assert_eq!(h[0].text, "A");
        assert_eq!(h[1].level, 2);
        assert_eq!(h[1].text, "B");
        assert_eq!(h[2].level, 1);
        assert_eq!(h[2].text, "C");
        // A ends at C (level 1 <= 1).
        assert_eq!(h[0].line_end, 4);
        // B ends at C (level 1 <= 2).
        assert_eq!(h[1].line_end, 4);
        // C ends at EOF.
        assert_eq!(h[2].line_end, 5);
    }

    #[test]
    fn find_section_returns_correct_body() {
        let content = "# A\nbody a\n# B\nbody b\n";
        let (start, end) = find_section(content, "A").unwrap();
        assert_eq!(&content[start..end], "body a\n");

        let (start, end) = find_section(content, "B").unwrap();
        assert_eq!(&content[start..end], "body b\n");
    }

    #[test]
    fn find_section_none_for_missing() {
        assert!(find_section("# X\n", "Y").is_none());
    }

    #[test]
    fn find_section_accepts_hash_prefixed_query() {
        let content = "## Sub\ndata\n";
        let (start, end) = find_section(content, "## Sub").unwrap();
        assert_eq!(&content[start..end], "data\n");
    }

    // -- table-append -------------------------------------------------------

    #[test]
    fn table_append_adds_row() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Table\n| H1 | H2 |\n|---|---|\n| A | B |\n").unwrap();

        let args = MdArgs {
            action: MdAction::TableAppend {
                file: file.to_str().unwrap().to_string(),
                heading: "Table".into(),
                row: "| C | D |".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(result.contains("| C | D |"), "new row missing");
        let a_pos = result.find("| A | B |").unwrap();
        let c_pos = result.find("| C | D |").unwrap();
        assert!(c_pos > a_pos, "new row not after existing data row");
    }

    #[test]
    fn table_append_heading_not_found_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\n| H1 |\n|---|\n| A |\n").unwrap();

        let args = MdArgs {
            action: MdAction::TableAppend {
                file: file.to_str().unwrap().to_string(),
                heading: "Missing".into(),
                row: "| X |".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn table_append_no_table_returns_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\nsome text but no table\n").unwrap();

        let args = MdArgs {
            action: MdAction::TableAppend {
                file: file.to_str().unwrap().to_string(),
                heading: "Title".into(),
                row: "| X |".into(),
            },
            write: Default::default(),
        };
        let result = run(args, &default_global());
        assert!(result.is_err(), "expected error when no table is present");
    }

    #[test]
    fn table_append_preserves_surrounding_content() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(
            &file,
            "# Before\npre-content\n# Table\n| H1 |\n|---|\n| A |\n\nAfter table text.\n# After\npost-content\n",
        )
        .unwrap();

        let args = MdArgs {
            action: MdAction::TableAppend {
                file: file.to_str().unwrap().to_string(),
                heading: "Table".into(),
                row: "| B |".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        assert!(
            result.contains("# Before\npre-content\n"),
            "content before heading damaged"
        );
        assert!(
            result.contains("After table text.\n"),
            "content after table damaged"
        );
        assert!(
            result.contains("# After\npost-content\n"),
            "next section damaged"
        );
        assert!(result.contains("| B |"), "new row missing");
    }
}
