use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::cmd::output::execute_via_engine;
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
        #[arg(long, allow_hyphen_values = true)]
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
                WritePhase::Check(changed) => Some(changed),
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
            if exit::is_no_match(&e) {
                let msg = e.to_string();
                global.emit_json(&serde_json::json!({
                    "ok": false,
                    "error": &msg,
                }))?;
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
    crate::verbose!("md: action={:?}", std::mem::discriminant(&args.action));
    match args.action {
        MdAction::ReplaceSection {
            file,
            heading,
            stdin,
            content,
        } => {
            crate::verbose!("md: replace-section file={}, heading={:?}", file, heading);
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
            crate::verbose!(
                "md: insert-after-heading file={}, heading={:?}",
                file,
                heading
            );
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
            crate::verbose!(
                "md: insert-before-heading file={}, heading={:?}",
                file,
                heading
            );
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
            crate::verbose!("md: upsert-bullet file={}, heading={:?}", file, heading);
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
            crate::verbose!("md: dedupe-headings file={}", file);
            // Pre-read to compute removed headings for side-channel output,
            // then route the actual write through the engine.
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let original =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            let (_new, removed) = dedupe_headings_in(&original);

            // Emit removed headings as side-channel output.
            if !removed.is_empty() && !global.emit_json_items(&removed)? && !global.quiet {
                for h in &removed {
                    eprintln!("md: removed duplicate: {h}");
                }
            }

            // Route the write through the engine. Use execute_single directly
            // to avoid execute_via_engine's JSON emission (which would conflict
            // with the removed-headings output already emitted above).
            let op = Operation::MdDedupeHeadings { path: file.clone() };
            let options = crate::tx::engine::ExecuteOptions {
                cwd: &cwd,
                global,
                guard: None,
            };
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
            crate::verbose!("md: lint-agents file={}", file);
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            let issues = lint_agents_content(&content);

            if !global.emit_json_items(&issues)? && !global.quiet {
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
            crate::verbose!("md: table-append file={}, heading={:?}", file, heading);
            // Pre-validate: distinguish "heading not found" (NO_MATCHES)
            // from "no table under heading" (error), which the engine
            // conflates into a single None.
            let cwd = global.resolve_cwd()?;
            let path = cwd.join(&file);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            match find_section(&content, &heading) {
                None => {
                    global.emit_json(&serde_json::json!({
                        "ok": false,
                        "error": format!("heading {:?} not found in {file}", heading),
                    }))?;
                    Ok(exit::NO_MATCHES)
                }
                Some((body_start, body_end)) => {
                    // Verify the table exists and the row is valid.
                    if let Err(e) =
                        crate::ops::md::table_append_in(&content, body_start, body_end, &row)
                    {
                        anyhow::bail!("{e} under heading {:?}", heading);
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
                                WritePhase::Check(changed) => Some(changed),
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
            crate::verbose!(
                "md: move-section file={}, heading={:?}, to={:?}",
                file,
                heading,
                to
            );
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

#[path = "md_tests.rs"]
#[cfg(test)]
mod tests;
