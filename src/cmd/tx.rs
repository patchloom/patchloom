use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::plan::{self, Plan};
use crate::tx::{
    CommitError, TxArgs, TxExecResult, TxOutput, build_error_output, build_full_tx_output,
    format_error_with_backup_hint, run_lifecycle,
};
use crate::write::run_format_command;

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Diff output helper
// ---------------------------------------------------------------------------

fn emit_output_json(output: &TxOutput, compact: bool) {
    let result = if compact {
        serde_json::to_string(output)
    } else {
        serde_json::to_string_pretty(output)
    };
    if let Ok(json) = result {
        println!("{json}");
    }
}

fn legacy_error_prefix(error_kind: &str) -> &str {
    if error_kind == "format_failed" {
        "validation_failed"
    } else {
        error_kind
    }
}

fn emit_error_json_with_prefix(
    error_kind: &'static str,
    legacy_error_prefix: &'static str,
    error: &str,
    backup_session: Option<&str>,
    compact: bool,
) {
    emit_output_json(
        &build_error_output(error_kind, legacy_error_prefix, error, backup_session),
        compact,
    );
}

fn emit_error_json(
    error_kind: &'static str,
    error: &str,
    backup_session: Option<&str>,
    compact: bool,
) {
    emit_error_json_with_prefix(
        error_kind,
        legacy_error_prefix(error_kind),
        error,
        backup_session,
        compact,
    );
}

fn print_diffs(changes: &[(PathBuf, String, String)], cwd: &Path, color: bool) {
    let diffs: Vec<_> = changes
        .iter()
        .map(|(p, old, new)| {
            let display = crate::files::relative_display(p, cwd);
            unified_diff(&display.to_string_lossy(), old, new)
        })
        .collect();
    let result = DiffResult { diffs };
    print!("{}", format_diff_result_colored(&result, color));
}

// ---------------------------------------------------------------------------
// Direct execution (MCP / in-process callers)
// ---------------------------------------------------------------------------

pub use crate::tx::RestoreFailGuard;

fn handle_commit_error(err: CommitError, structured: bool, compact: bool) -> anyhow::Result<u8> {
    let error_kind = if err.rollback_ok {
        "rollback"
    } else {
        "rollback_failed"
    };
    let exit_code = if err.rollback_ok {
        exit::ROLLBACK
    } else {
        exit::FAILURE
    };
    let backup_session = err.backup_session.as_deref();
    if structured {
        emit_error_json_with_prefix(
            error_kind,
            error_kind,
            &err.message,
            backup_session,
            compact,
        );
    } else {
        let msg = format_error_with_backup_hint(&err.message, backup_session);
        eprintln!("tx: {msg}");
    }
    Ok(exit_code)
}

/// Execute a parsed [`Plan`] directly and return the exit code and JSON
/// result string. Does **not** write to stdout or stderr.
///
/// This is the in-process equivalent of spawning
/// `patchloom --json tx <plan-file> --apply` and capturing its output.
/// The MCP server and library API call this to avoid subprocess overhead.
pub fn execute_plan_direct(
    plan: Plan,
    cwd: &Path,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<(u8, String)> {
    // Delegate to the extracted library tx module (enables "files" pure-library use without dup).
    // CLI run and formatting remain in this module for command surface.
    // For CLI we serialize the rich report (library users get the typed PlanReport directly via api).
    let report = crate::tx::execute_plan_direct(plan, cwd, guard)?;
    let code = crate::tx::exit_code_from_tx_output(&report);
    let json = serde_json::to_string_pretty(&report)?;
    Ok((code, json))
}

// ---------------------------------------------------------------------------
// Shared commit + lifecycle helper (used by --apply and --confirm paths)
// ---------------------------------------------------------------------------

/// Context for the commit + lifecycle phase, avoiding parameter bloat.
struct CommitContext<'a> {
    plan: &'a Plan,
    base_cwd: &'a Path,
    cwd: &'a Path,
    strict: bool,
    structured: bool,
    compact: bool,
}

/// Commit changes, run format command, optionally show diffs, run lifecycle
/// steps, and emit structured output. Returns the exit code.
fn commit_and_finalize(
    ctx: &CommitContext<'_>,
    result: &mut TxExecResult,
    global: &GlobalFlags,
    show_diffs: bool,
) -> anyhow::Result<u8> {
    if let Err(err) = crate::tx::commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        ctx.cwd,
    ) {
        return handle_commit_error(err, ctx.structured, ctx.compact);
    }

    run_format_command(global, ctx.cwd)?;

    if show_diffs && !result.changes.is_empty() {
        print_diffs(&result.changes, ctx.cwd, global.should_color());
    }

    if let Some(err) = run_lifecycle(ctx.plan, ctx.base_cwd, ctx.cwd) {
        if ctx.strict {
            crate::tx::rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            let rollback_msg = format!("strict mode -- all changes reverted ({})", err.message);
            if ctx.structured {
                emit_error_json_with_prefix(err.kind, "rollback", &rollback_msg, None, ctx.compact);
            } else {
                eprintln!("tx: {rollback_msg}");
            }
            return Ok(exit::ROLLBACK);
        }
        if ctx.structured {
            emit_error_json(err.kind, &err.message, None, ctx.compact);
        }
        return Ok(exit::VALIDATION_FAILED);
    }

    if result.replace_no_matches {
        return Ok(exit::NO_MATCHES);
    }
    if ctx.structured {
        let output = build_full_tx_output("success", result, ctx.cwd);
        emit_output_json(&output, ctx.compact);
    } else if global.show_status() {
        let n_changed = result.changes.len();
        let n_deleted = result.deletions.len();
        let total = n_changed + n_deleted;
        eprintln!("applied: {total} file(s) affected");
    }
    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(args: TxArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let structured = global.json || global.jsonl;
    let compact = global.jsonl;

    // 1. Read plan from file or stdin.
    let plan_path = if args.plan == "-" {
        None
    } else {
        Some(global.resolve_cwd()?.join(&args.plan))
    };
    let plan_text = if args.plan == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let plan_path = plan_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("internal error: plan path missing"))?;
        std::fs::read_to_string(plan_path).map_err(|e| {
            anyhow::anyhow!("failed to read plan file '{}': {e}", plan_path.display())
        })?
    };

    // 2. Parse plan (JSON, YAML, or TOML).
    let plan_path_hint = if args.plan == "-" {
        None
    } else {
        Some(args.plan.as_str())
    };
    let plan = match plan::parse_plan_auto(&plan_text, plan_path_hint, args.plan_format.as_deref())
    {
        Ok(p) => p,
        Err(e) => {
            if structured {
                emit_error_json("parse_error", &e.to_string(), None, compact);
            } else {
                eprintln!("tx: plan parse error: {e}");
            }
            return Ok(exit::PARSE_ERROR);
        }
    };

    // 3. Validate plan and resolve working directory.
    let base_cwd = global.resolve_cwd()?;
    let (cwd, strict, _resolved_global) =
        match crate::tx::validate_and_prepare_plan(&plan, &base_cwd, args.no_strict) {
            Ok(v) => v,
            Err(output) => {
                let code = if output.error_kind.as_deref() == Some("parse_error") {
                    exit::PARSE_ERROR
                } else {
                    exit::FAILURE
                };
                let msg = output
                    .error
                    .clone()
                    .unwrap_or_else(|| "plan validation failed".to_string());
                let bs = output.backup_session.clone();
                if structured {
                    emit_error_json("parse_error", &msg, bs.as_deref(), compact);
                } else {
                    eprintln!("tx: {msg}");
                }
                return Ok(code);
            }
        };

    if plan.cwd.is_some() && !cwd.is_dir() {
        let msg = format!(
            "plan cwd is not a directory: {}",
            plan.cwd.as_deref().expect("plan.cwd checked above")
        );
        if structured {
            emit_error_json("parse_error", &msg, None, compact);
        } else {
            eprintln!("tx: {msg}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    // 4. Execute all operations, collecting changes in memory (no writes).
    let mut result =
        match crate::tx::execute_and_collect(&plan, &cwd, global, global.quiet, structured) {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if structured {
                    emit_error_json("operation_failed", &msg, None, compact);
                } else {
                    eprintln!("tx: {msg}");
                }
                return Ok(exit::OPERATION_FAILED);
            }
        };

    // Deletions of empty files won't appear in `changes` (original == final == ""),
    // but they still count as changes for --check reporting.
    let pending_deletions = result
        .deletions
        .iter()
        .filter(|p| !result.changes.iter().any(|(c, _, _)| c == *p))
        .count();

    // 5. Output based on mode.
    if global.check {
        if result.no_effective_changes {
            if result.replace_no_matches {
                return Ok(exit::NO_MATCHES);
            }
            if structured {
                let output = build_full_tx_output("success", &mut result, &cwd);
                emit_output_json(&output, compact);
            }
            return Ok(exit::SUCCESS);
        }
        if structured {
            let output = build_full_tx_output("changes_detected", &mut result, &cwd);
            emit_output_json(&output, compact);
        } else if !global.quiet {
            println!(
                "{} file(s) would change",
                result.changes.len() + pending_deletions
            );
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    let ctx = CommitContext {
        plan: &plan,
        base_cwd: &base_cwd,
        cwd: &cwd,
        strict,
        structured,
        compact,
    };

    if global.apply {
        return commit_and_finalize(&ctx, &mut result, global, global.diff);
    }

    if result.replace_no_matches {
        return Ok(exit::NO_MATCHES);
    }

    // Default / --diff mode: show unified diffs.
    if structured {
        let output = build_full_tx_output("success", &mut result, &cwd);
        emit_output_json(&output, compact);
    } else if !result.changes.is_empty() {
        print_diffs(&result.changes, &cwd, global.should_color());
    }
    if !result.no_effective_changes && global.show_status() {
        let n = result.changes.len() + result.deletions.len();
        eprintln!("{n} file(s) changed");
    }

    // --confirm: prompt after showing diffs, then apply if confirmed.
    if !result.no_effective_changes && global.should_apply() {
        return commit_and_finalize(&ctx, &mut result, global, false);
    }

    Ok(exit::SUCCESS)
}

#[path = "tx_tests.rs"]
#[cfg(test)]
mod tests;
