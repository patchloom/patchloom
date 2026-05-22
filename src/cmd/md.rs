use crate::cli::global::GlobalFlags;
use crate::diff::{self, DiffResult, unified_diff};
use crate::exit;
use crate::ops::md::{
    dedupe_headings_in, find_section, insert_after_heading_in, insert_before_heading_in,
    parse_headings, replace_section_in, table_append_in, upsert_bullet_in,
};
use crate::write::{atomic_write, policy_from_flags};
use anyhow::Context;
use clap::Args;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashSet;

use std::path::Path;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom md table-append README.md --heading '## API' --row '| /users | List users |'
  patchloom md upsert-bullet AGENTS.md --heading '## Rules' --bullet '- Run make check'
  patchloom md replace-section CHANGELOG.md --heading '## Unreleased' --content '- New feature' --apply")]
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
        file: String,
        #[arg(long)]
        heading: String,
        /// Read replacement content from stdin.
        // ref:md-mode:stdin
        #[arg(long)]
        stdin: bool,
        /// Replacement content as argument.
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content after a heading.
    InsertAfterHeading {
        file: String,
        #[arg(long)]
        heading: String,
        // ref:md-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content before a heading.
    InsertBeforeHeading {
        file: String,
        #[arg(long)]
        heading: String,
        // ref:md-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        content: Option<String>,
    },
    /// Add a bullet under a heading if not already present.
    UpsertBullet {
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        bullet: String,
    },
    /// Remove duplicate headings.
    DedupeHeadings { file: String },
    /// Lint common AGENTS.md problems.
    LintAgents { file: String },
    /// Append a row to a markdown table under a heading.
    TableAppend {
        file: String,
        #[arg(long)]
        heading: String,
        /// The row to append, in markdown table format (e.g., "| col1 | col2 | col3 |").
        #[arg(long)]
        row: String,
    },
}

// ---------------------------------------------------------------------------
// Helpers: stdin, write policy, mutation driver
// ---------------------------------------------------------------------------

fn read_content(use_stdin: bool, content: &Option<String>) -> anyhow::Result<String> {
    if use_stdin {
        Ok(std::io::read_to_string(std::io::stdin())?)
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

    if global.diff || global.confirm {
        let d = unified_diff(file, original, &final_content);
        if d.has_changes {
            let result = DiffResult {
                diffs: vec![d],
                total_files_changed: 1,
            };
            print!(
                "{}",
                diff::format_diff_result_colored(&result, global.should_color())
            );
        }
    }

    if global.check && has_changes {
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply || (global.confirm && has_changes && global.should_apply()) {
        atomic_write(path, new_content, &policy)?;
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// Lint
// ---------------------------------------------------------------------------

/// Remove backtick-delimited inline code spans from a line, returning
/// the remaining prose.  This lets lint checks ignore content inside
/// backticks (e.g. ``Never use `git add .` ``).
fn strip_inline_code(line: &str) -> Cow<'_, str> {
    if !line.contains('`') {
        return Cow::Borrowed(line);
    }
    let mut result = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        result.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('`') {
            rest = &after_open[close + 1..];
        } else {
            // Unmatched backtick — keep the rest as-is.
            rest = after_open;
            break;
        }
    }
    result.push_str(rest);
    Cow::Owned(result)
}

#[derive(Debug, Serialize)]
struct LintIssue {
    issue: &'static str,
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
        if !seen.insert(key) {
            issues.push(LintIssue {
                issue: "duplicate heading",
                line: Some(h.line_start + 1), // 1-based
                heading: Some(format!("{} {}", "#".repeat(h.level), h.text)),
            });
        }
    }

    // 2. Dangerous git add commands (skip fenced code blocks and inline code).
    let mut fence_marker: Option<&str> = None;
    for (idx, line) in content.lines().enumerate() {
        if fence_marker.is_none() {
            if line.starts_with("```") {
                fence_marker = Some("```");
                continue;
            } else if line.starts_with("~~~") {
                fence_marker = Some("~~~");
                continue;
            }
        } else if line.starts_with(fence_marker.expect("checked is_none above")) {
            fence_marker = None;
            continue;
        }
        if fence_marker.is_some() {
            continue;
        }
        // Skip lines where the command only appears inside backtick spans.
        let stripped = strip_inline_code(line);
        if stripped.contains("git add .") || stripped.contains("git add -A") {
            issues.push(LintIssue {
                issue: "dangerous command",
                line: Some(idx + 1),
                heading: None,
            });
        }
    }

    // 3. Missing final newline.
    if !content.is_empty() && !content.ends_with('\n') {
        issues.push(LintIssue {
            issue: "missing final newline",
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
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
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
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
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
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
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
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
            match upsert_bullet_in(&original, &heading, &bullet) {
                Some(new) => apply_mutation(&file, &original, &new, global),
                None => Ok(exit::NO_MATCHES),
            }
        }

        MdAction::DedupeHeadings { file } => {
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
            let (new, removed) = dedupe_headings_in(&original);
            if !removed.is_empty() {
                if global.json {
                    println!("{}", serde_json::to_string_pretty(&removed)?);
                } else if global.jsonl {
                    for h in &removed {
                        println!("{}", serde_json::to_string(h)?);
                    }
                } else if !global.quiet {
                    for h in &removed {
                        eprintln!("md: removed duplicate: {h}");
                    }
                }
            }
            apply_mutation(&file, &original, &new, global)
        }

        MdAction::LintAgents { file } => {
            let content =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
            let issues = lint_agents_content(&content);

            if global.json {
                println!("{}", serde_json::to_string_pretty(&issues)?);
            } else if global.jsonl {
                for issue in &issues {
                    println!("{}", serde_json::to_string(issue)?);
                }
            } else if !global.quiet {
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
            let original =
                std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
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

    #[test]
    fn insert_after_heading_missing_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\ncontent\n").unwrap();

        let args = MdArgs {
            action: MdAction::InsertAfterHeading {
                file: file.to_str().unwrap().to_string(),
                heading: "Missing".into(),
                stdin: false,
                content: Some("inserted".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
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

    #[test]
    fn upsert_bullet_missing_heading_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\ncontent\n").unwrap();

        let args = MdArgs {
            action: MdAction::UpsertBullet {
                file: file.to_str().unwrap().to_string(),
                heading: "Missing".into(),
                bullet: "new item".into(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
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

    #[test]
    fn lint_agents_finds_missing_final_newline() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nNo trailing newline").unwrap();

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
    fn lint_agents_skips_dangerous_cmd_in_code_fence() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(
            &file,
            "# Rules\n\n```bash\n# BAD example\ngit add .\n```\n\nStage explicitly.\n",
        )
        .unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn lint_agents_skips_dangerous_cmd_in_tilde_fence() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(
            &file,
            "# Rules\n\n~~~bash\n# BAD example\ngit add .\n~~~\n\nStage explicitly.\n",
        )
        .unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn lint_agents_skips_dangerous_cmd_in_inline_code() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nNever use `git add .` or `git add -A`.\n").unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn strip_inline_code_removes_backtick_spans() {
        assert_eq!(
            strip_inline_code("use `git add .` carefully"),
            "use  carefully"
        );
        assert_eq!(strip_inline_code("no backticks here"), "no backticks here");
        assert_eq!(strip_inline_code("`all code`"), "");
        assert_eq!(strip_inline_code("a `b` c `d` e"), "a  c  e");
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
