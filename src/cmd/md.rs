use crate::cli::global::GlobalFlags;
use crate::cmd::output::execute_via_engine;
use crate::cmd::write_dispatch::WritePhase;
use crate::exit;
use crate::ops::md::{dedupe_headings_in, find_section, lint_agents_content};
use crate::plan::Operation;
use anyhow::Context;
use clap::Args;
use serde::Serialize;

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
    /// Move a heading section to a new location (same file or different file).
    MoveSection {
        /// Source file containing the section to move.
        file: String,
        /// Heading of the section to move (e.g., "## FAQ").
        #[arg(long)]
        heading: String,
        /// Destination file. Omit for same-file reorder.
        #[arg(long)]
        to: Option<String>,
        /// Insert before this heading at the destination.
        #[arg(long, conflicts_with = "after")]
        before: Option<String>,
        /// Insert after this heading at the destination.
        #[arg(long, conflicts_with = "before")]
        after: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Helpers
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

/// JSON output struct for single-file md write operations.
#[derive(Debug, Serialize)]
struct MdOutput {
    ok: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_changes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

/// Execute a single md operation through the engine, mapping "not found"
/// errors to `exit::NO_MATCHES`.
fn execute_md_op(
    op: Operation,
    global: &GlobalFlags,
    file: &str,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    let file_owned = file.to_string();
    match execute_via_engine(
        op,
        global,
        |phase, diff| MdOutput {
            ok: true,
            path: file_owned.clone(),
            has_changes: match phase {
                WritePhase::Check => Some(true),
                _ => None,
            },
            diff,
            applied: match phase {
                WritePhase::Confirmed(a) => Some(a),
                _ => None,
            },
        },
        check_msg,
        apply_msg,
    ) {
        Ok(code) => Ok(code),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                Ok(exit::NO_MATCHES)
            } else {
                Err(e)
            }
        }
    }
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
            let op = Operation::MdReplaceSection {
                path: file.clone(),
                heading: heading.clone(),
                content: replacement,
            };
            execute_md_op(
                op,
                global,
                &file,
                &format!("would modify {file}"),
                &format!("modified {file}"),
            )
        }

        MdAction::InsertAfterHeading {
            file,
            heading,
            stdin,
            content,
        } => {
            let insertion = read_content(stdin, &content)?;
            let op = Operation::MdInsertAfterHeading {
                path: file.clone(),
                heading: heading.clone(),
                content: insertion,
            };
            execute_md_op(
                op,
                global,
                &file,
                &format!("would modify {file}"),
                &format!("modified {file}"),
            )
        }

        MdAction::InsertBeforeHeading {
            file,
            heading,
            stdin,
            content,
        } => {
            let insertion = read_content(stdin, &content)?;
            let op = Operation::MdInsertBeforeHeading {
                path: file.clone(),
                heading: heading.clone(),
                content: insertion,
            };
            execute_md_op(
                op,
                global,
                &file,
                &format!("would modify {file}"),
                &format!("modified {file}"),
            )
        }

        MdAction::UpsertBullet {
            file,
            heading,
            bullet,
        } => {
            let op = Operation::MdUpsertBullet {
                path: file.clone(),
                heading: heading.clone(),
                bullet,
            };
            execute_md_op(
                op,
                global,
                &file,
                &format!("would modify {file}"),
                &format!("modified {file}"),
            )
        }

        MdAction::DedupeHeadings { file } => {
            // Pre-read to compute removed headings for side-channel output,
            // then route the actual write through the engine.
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let original =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            let (_new, removed) = dedupe_headings_in(&original);

            // Emit removed headings as side-channel output.
            if !removed.is_empty() {
                if global.json || global.jsonl {
                    global.emit_json_items(&removed)?;
                } else if !global.quiet {
                    for h in &removed {
                        eprintln!("md: removed duplicate: {h}");
                    }
                }
            }

            // Route the write through the engine. Use execute_single directly
            // to avoid execute_via_engine's JSON emission (which would conflict
            // with the removed-headings output already emitted above).
            let op = Operation::MdDedupeHeadings { path: file.clone() };
            let options = crate::tx::engine::ExecuteOptions { cwd: &cwd, global };
            let result = crate::tx::engine::execute_single(op, options)?;

            if global.check {
                return if result.has_changes {
                    Ok(exit::CHANGES_DETECTED)
                } else {
                    Ok(exit::SUCCESS)
                };
            }

            if global.apply {
                result.commit()?;
                crate::write::run_format_command(global, &cwd)?;
                return Ok(exit::SUCCESS);
            }

            // Default / --diff mode: preview diffs.
            let diffs = result.build_diffs();
            if !diffs.is_empty() && !global.json && !global.jsonl {
                let dr = crate::diff::DiffResult {
                    diffs: diffs.clone(),
                };
                print!(
                    "{}",
                    crate::diff::format_diff_result_colored(&dr, global.should_color())
                );
            }

            // --confirm: apply if user confirms.
            if global.should_apply() {
                result.commit()?;
                crate::write::run_format_command(global, &cwd)?;
            }

            Ok(exit::SUCCESS)
        }

        MdAction::LintAgents { file } => {
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            let issues = lint_agents_content(&content);

            if global.json || global.jsonl {
                global.emit_json_items(&issues)?;
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
            // Pre-validate: distinguish "heading not found" (NO_MATCHES)
            // from "no table under heading" (error), which the engine
            // conflates into a single None.
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            match find_section(&content, &heading) {
                None => Ok(exit::NO_MATCHES),
                Some((body_start, body_end)) => {
                    // Verify a table actually exists under the heading.
                    if crate::ops::md::table_append_in(&content, body_start, body_end, &row)
                        .is_none()
                    {
                        anyhow::bail!("no markdown table found under heading {:?}", heading);
                    }
                    let op = Operation::MdTableAppend {
                        path: file.clone(),
                        heading: heading.clone(),
                        row,
                    };
                    let file_owned = file.clone();
                    execute_via_engine(
                        op,
                        global,
                        |phase, diff| MdOutput {
                            ok: true,
                            path: file_owned.clone(),
                            has_changes: match phase {
                                WritePhase::Check => Some(true),
                                _ => None,
                            },
                            diff,
                            applied: match phase {
                                WritePhase::Confirmed(a) => Some(a),
                                _ => None,
                            },
                        },
                        &format!("would modify {file}"),
                        &format!("modified {file}"),
                    )
                }
            }
        }

        MdAction::MoveSection {
            file,
            heading,
            to,
            before,
            after,
        } => {
            // Validate exactly one of --before or --after.
            if before.is_none() && after.is_none() {
                anyhow::bail!("exactly one of --before or --after must be provided");
            }

            let dest_file = to.as_deref().unwrap_or(&file);
            let (check_msg, apply_msg) = if dest_file != file {
                (
                    format!("would modify {file}\nwould modify {dest_file}"),
                    format!("modified {file}\nmodified {dest_file}"),
                )
            } else {
                (format!("would modify {file}"), format!("modified {file}"))
            };

            let op = Operation::MdMoveSection {
                path: file.clone(),
                heading,
                to,
                before,
                after,
            };
            execute_md_op(op, global, &file, &check_msg, &apply_msg)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::md::{find_section, has_dangerous_git_add_dot, strip_inline_code};
    use std::fs;
    use tempfile::TempDir;

    // -- backup ------------------------------------------------------------

    #[test]
    fn replace_section_apply_creates_backup_session() {
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
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(
            backup_dir.exists(),
            "backup directory should exist after md replace-section --apply"
        );
        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(
            !sessions.is_empty(),
            "at least one backup session should be created"
        );
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn lint_agents_finds_dangerous_git_add_all_long_form() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nRun git add --all to stage\n").unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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

    #[test]
    fn has_dangerous_git_add_dot_true_cases() {
        assert!(has_dangerous_git_add_dot("git add ."));
        assert!(has_dangerous_git_add_dot("run git add . first"));
        assert!(has_dangerous_git_add_dot("git add . && git commit"));
    }

    #[test]
    fn has_dangerous_git_add_dot_false_for_dotfiles() {
        assert!(!has_dangerous_git_add_dot("git add .gitignore"));
        assert!(!has_dangerous_git_add_dot("git add .editorconfig"));
        assert!(!has_dangerous_git_add_dot(
            "git add .github/workflows/ci.yml"
        ));
    }

    #[test]
    fn lint_agents_allows_git_add_dotfile() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("AGENTS.md");
        fs::write(&file, "# Rules\nRun git add .gitignore to stage\n").unwrap();

        let args = MdArgs {
            action: MdAction::LintAgents {
                file: file.to_str().unwrap().to_string(),
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    // -- final newline ------------------------------------------------------

    #[test]
    fn final_newline_is_preserved_on_write() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# Title\ncontent\n").unwrap();

        let mut global = GlobalFlags::test_apply();
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
        let h = crate::ops::md::parse_headings(content);
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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
        let result = run(args, &GlobalFlags::test_apply());
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
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
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

    // -- move-section -------------------------------------------------------

    #[test]
    fn move_section_same_file_reorder_before() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# A\na-content\n# B\nb-content\n# C\nc-content\n").unwrap();

        let args = MdArgs {
            action: MdAction::MoveSection {
                file: file.to_str().unwrap().to_string(),
                heading: "C".into(),
                to: None,
                before: Some("B".into()),
                after: None,
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        let a_pos = result.find("# A").unwrap();
        let c_pos = result.find("# C").unwrap();
        let b_pos = result.find("# B").unwrap();
        assert!(a_pos < c_pos, "A should come before C");
        assert!(c_pos < b_pos, "C should come before B after move");
        assert!(result.contains("a-content"), "A content lost");
        assert!(result.contains("b-content"), "B content lost");
        assert!(result.contains("c-content"), "C content lost");
    }

    #[test]
    fn move_section_same_file_reorder_after() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# A\na-content\n# B\nb-content\n# C\nc-content\n").unwrap();

        let args = MdArgs {
            action: MdAction::MoveSection {
                file: file.to_str().unwrap().to_string(),
                heading: "A".into(),
                to: None,
                before: None,
                after: Some("B".into()),
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        let b_pos = result.find("# B").unwrap();
        let a_pos = result.find("# A").unwrap();
        let c_pos = result.find("# C").unwrap();
        assert!(b_pos < a_pos, "B should come before A after move");
        assert!(a_pos < c_pos, "A should come before C");
    }

    #[test]
    fn move_section_explicit_to_same_file_reorders_not_duplicates() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# A\na-content\n# B\nb-content\n# C\nc-content\n").unwrap();

        let path_str = file.to_str().unwrap().to_string();
        let args = MdArgs {
            action: MdAction::MoveSection {
                file: path_str.clone(),
                heading: "C".into(),
                to: Some(path_str),
                before: Some("B".into()),
                after: None,
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let result = fs::read_to_string(&file).unwrap();
        let a_pos = result.find("# A").unwrap();
        let c_pos = result.find("# C").unwrap();
        let b_pos = result.find("# B").unwrap();
        assert!(a_pos < c_pos, "A should come before C");
        assert!(c_pos < b_pos, "C should come before B after move");
        assert_eq!(
            result.matches("# C").count(),
            1,
            "section C must appear exactly once (not duplicated)"
        );
        assert_eq!(
            result.matches("c-content").count(),
            1,
            "content must not be duplicated"
        );
    }

    #[test]
    fn move_section_cross_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("source.md");
        let dst = dir.path().join("dest.md");
        fs::write(
            &src,
            "# Keep\nkept\n# Move Me\nmoved content\n# Also Keep\nalso\n",
        )
        .unwrap();
        fs::write(&dst, "# Intro\nintro text\n# End\nend text\n").unwrap();

        let args = MdArgs {
            action: MdAction::MoveSection {
                file: src.to_str().unwrap().to_string(),
                heading: "Move Me".into(),
                to: Some(dst.to_str().unwrap().to_string()),
                before: Some("End".into()),
                after: None,
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let src_result = fs::read_to_string(&src).unwrap();
        assert!(
            !src_result.contains("# Move Me"),
            "section not removed from source"
        );
        assert!(
            !src_result.contains("moved content"),
            "section body not removed from source"
        );
        assert!(src_result.contains("# Keep"), "kept section lost");
        assert!(src_result.contains("# Also Keep"), "other section lost");

        let dst_result = fs::read_to_string(&dst).unwrap();
        assert!(
            dst_result.contains("# Move Me"),
            "section not added to dest"
        );
        assert!(
            dst_result.contains("moved content"),
            "section body not in dest"
        );
        let move_pos = dst_result.find("# Move Me").unwrap();
        let end_pos = dst_result.find("# End").unwrap();
        assert!(move_pos < end_pos, "section not inserted before End");
    }

    #[test]
    fn move_section_missing_heading_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# A\ncontent\n").unwrap();

        let args = MdArgs {
            action: MdAction::MoveSection {
                file: file.to_str().unwrap().to_string(),
                heading: "Missing".into(),
                to: None,
                before: Some("A".into()),
                after: None,
            },
            write: Default::default(),
        };
        let code = run(args, &GlobalFlags::test_apply()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn move_section_no_before_or_after_returns_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.md");
        fs::write(&file, "# A\ncontent\n").unwrap();

        let args = MdArgs {
            action: MdAction::MoveSection {
                file: file.to_str().unwrap().to_string(),
                heading: "A".into(),
                to: None,
                before: None,
                after: None,
            },
            write: Default::default(),
        };
        let result = run(args, &GlobalFlags::test_apply());
        assert!(
            result.is_err(),
            "expected error when neither --before nor --after is provided"
        );
    }
}
