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
  patchloom md table-append README.md --heading '## API' --row '/users|List users'
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
        /// Markdown file to edit.
        file: String,
        /// Heading of the section to replace (e.g. `## Unreleased`).
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
    /// Insert content immediately after a heading line (before existing body).
    ///
    /// Does not insert after the full section body. For a sibling section after
    /// the section ends, use `insert-after-section` (#1726).
    InsertAfterHeading {
        /// Markdown file to edit.
        file: String,
        /// Heading line to insert under (e.g. `## Config`).
        #[arg(long)]
        heading: String,
        // ref:md-mode:stdin
        /// Read insertion content from stdin instead of `--content`.
        #[arg(long)]
        stdin: bool,
        /// Content to insert (omit when using `--stdin`).
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content after the full section body (sibling placement).
    ///
    /// Use when adding a new `##` section after this section's content. Prefer
    /// `insert-after-heading` only for content under the heading (e.g. intro
    /// before a table). #1726
    InsertAfterSection {
        /// Markdown file to edit.
        file: String,
        /// Heading whose section body ends just before the insertion point.
        #[arg(long)]
        heading: String,
        // ref:md-mode:stdin
        /// Read insertion content from stdin instead of `--content`.
        #[arg(long)]
        stdin: bool,
        /// Content to insert (omit when using `--stdin`).
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content immediately before a heading line.
    InsertBeforeHeading {
        /// Markdown file to edit.
        file: String,
        /// Heading line to insert before (e.g. `## Config`).
        #[arg(long)]
        heading: String,
        // ref:md-mode:stdin
        /// Read insertion content from stdin instead of `--content`.
        #[arg(long)]
        stdin: bool,
        /// Content to insert (omit when using `--stdin`).
        #[arg(long)]
        content: Option<String>,
    },
    /// Add a bullet under a heading if not already present.
    UpsertBullet {
        /// Markdown file to edit.
        file: String,
        /// Heading under which to upsert the bullet (e.g. `## Rules`).
        #[arg(long)]
        heading: String,
        /// Bullet text (leading `- ` is optional).
        #[arg(long, allow_hyphen_values = true)]
        bullet: String,
    },
    /// Remove duplicate headings.
    DedupeHeadings {
        /// Markdown file to scan for duplicate headings.
        file: String,
    },
    /// Lint common AGENTS.md problems.
    #[command(name = "lint-agents", alias = "lint")]
    LintAgents {
        /// AGENTS.md (or similar) file to lint.
        file: String,
    },
    /// Append a row to a markdown table under a heading.
    TableAppend {
        /// Markdown file containing the table.
        file: String,
        /// Heading above the target table (e.g. `## API`).
        #[arg(long)]
        heading: String,
        /// Table row: `| col1 | col2 |` or compact `col1|col2` / `col1 | col2`.
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

/// Read replacement/insertion content. Returns `Ok(None)` after emitting a
/// structured validation error when neither stdin nor content is provided.
fn read_content(
    use_stdin: bool,
    content: &Option<String>,
    global: &GlobalFlags,
) -> anyhow::Result<Option<String>> {
    if use_stdin {
        Ok(Some(std::io::read_to_string(std::io::stdin())?))
    } else if let Some(c) = content {
        Ok(Some(c.clone()))
    } else {
        global.emit_error_json_kind(
            Some("invalid_input"),
            "one of --stdin or --content must be provided",
        )?;
        Ok(None)
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
    let cwd = global.resolve_cwd()?;
    // Shared empty-path + --contain gate (engine also checks under --contain).
    global.check_paths_contained(&cwd, [file])?;
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
            applied: phase.applied_flag(),
        },
        check_msg,
        apply_msg,
    ) {
        Ok(code) => Ok(code),
        Err(e) => {
            if exit::is_no_match(&e) {
                global.emit_error_json_kind(Some("no_matches"), &e.to_string())?;
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
            let Some(replacement) = read_content(stdin, &content, global)? else {
                return Ok(exit::FAILURE);
            };
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
            let Some(insertion) = read_content(stdin, &content, global)? else {
                return Ok(exit::FAILURE);
            };
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

        MdAction::InsertAfterSection {
            file,
            heading,
            stdin,
            content,
        } => {
            crate::verbose!(
                "md: insert-after-section file={}, heading={:?}",
                file,
                heading
            );
            let Some(insertion) = read_content(stdin, &content, global)? else {
                return Ok(exit::FAILURE);
            };
            let op = Operation::MdInsertAfterSection {
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
            let Some(insertion) = read_content(stdin, &content, global)? else {
                return Ok(exit::FAILURE);
            };
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
            global.check_paths_contained(&cwd, [&file])?;
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

            // Side-channel headings already emitted; no second JSON schema body.
            let op = Operation::MdDedupeHeadings { path: file.clone() };
            let (cwd, result) = crate::cmd::output::stage_for_write(
                crate::tx::engine::WriteSource::Operations(vec![op]),
                global,
            )?;
            use crate::cmd::write_mode::{FinalizeCallbacks, finalize_report};
            finalize_report(
                global,
                &cwd,
                result,
                true,
                FinalizeCallbacks {
                    on_check: |_g: &GlobalFlags, _has: bool, _diffs: &[crate::diff::FileDiff]| {
                        Ok(())
                    },
                    on_apply: |_g: &GlobalFlags,
                               _has: bool,
                               _diffs: &[crate::diff::FileDiff],
                               _plain: Option<String>| Ok(()),
                    on_preview: |g: &GlobalFlags,
                                 _has: bool,
                                 diffs: &[crate::diff::FileDiff],
                                 _plain: Option<String>| {
                        if !diffs.is_empty() && !g.json && !g.jsonl {
                            let dr = crate::diff::DiffResult {
                                diffs: diffs.to_vec(),
                            };
                            print!(
                                "{}",
                                crate::diff::format_diff_result_colored(&dr, g.should_color())
                            );
                        }
                        Ok(())
                    },
                    after_preview_emit: |_: &GlobalFlags| {},
                    after_preview_apply: |_: &GlobalFlags| {},
                },
            )
        }

        MdAction::LintAgents { file } => {
            crate::verbose!("md: lint-agents file={}", file);
            let cwd = global.resolve_cwd()?;
            global.check_paths_contained(&cwd, [&file])?;
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
            global.check_paths_contained(&cwd, [&file])?;
            let path = cwd.join(&file);
            let content =
                std::fs::read_to_string(&path).with_context(|| format!("reading {file}"))?;
            match find_section(&content, &heading) {
                None => {
                    let msg = format!("heading {:?} not found in {file}", heading);
                    global.emit_error_json_kind(Some("no_matches"), &msg)?;
                    Ok(exit::NO_MATCHES)
                }
                Some((body_start, body_end)) => {
                    // Verify the table exists and the row is valid.
                    if let Err(e) =
                        crate::ops::md::table_append_in(&content, body_start, body_end, &row)
                    {
                        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                            msg: format!("{e} under heading {:?}", heading),
                        }));
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
                            applied: phase.applied_flag(),
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
                global.emit_error_json_kind(
                    Some("invalid_input"),
                    "exactly one of --before or --after must be provided",
                )?;
                return Ok(exit::FAILURE);
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
