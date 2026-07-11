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
    match result {
        Ok(json) => println!("{json}"),
        // TxOutput is always serializable in practice; log if that ever fails
        // so structured mode never looks like a silent empty success.
        Err(err) => eprintln!("tx: failed to serialize structured output: {err}"),
    }
}

fn emit_error_json(error_kind: &str, error: &str, backup_session: Option<&str>, compact: bool) {
    emit_output_json(
        &build_error_output(error_kind, error, backup_session),
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
        emit_error_json(error_kind, &err.message, backup_session, compact);
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
    crate::verbose!(
        "tx: committing {} changes, {} deletions, strict={}",
        result.changes.len(),
        result.deletions.len(),
        ctx.strict
    );
    if let Err(err) = crate::tx::commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        ctx.cwd,
    ) {
        return handle_commit_error(err, ctx.structured, ctx.compact);
    }

    if let Err(e) = run_format_command(global, ctx.cwd) {
        // The commit already succeeded; files are written but unformatted.
        // Warn with a recovery hint instead of a bare error (#1159).
        let hint = "files were written but formatting failed; run `patchloom undo` to restore";
        return Err(e.context(hint));
    }

    if show_diffs && !result.changes.is_empty() {
        print_diffs(&result.changes, ctx.cwd, global.should_color());
    }

    // Snapshot non-tx files before lifecycle steps so we can restore
    // collateral changes on strict rollback (#1111.7).
    let collateral_snapshot = if ctx.strict && ctx.plan.has_lifecycle_steps() {
        let tx_paths: std::collections::HashSet<PathBuf> =
            result.changes.iter().map(|(p, _, _)| p.clone()).collect();
        crate::tx::snapshot_non_tx_files(ctx.cwd, &tx_paths)
    } else {
        std::collections::HashMap::new()
    };

    if let Some(err) = run_lifecycle(ctx.plan, ctx.base_cwd, ctx.cwd) {
        if ctx.strict {
            crate::tx::rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            crate::tx::restore_collateral_files(&collateral_snapshot);
            let rollback_msg = format!(
                "rollback: strict mode -- all changes reverted ({})",
                err.message
            );
            if ctx.structured {
                emit_error_json(err.kind, &rollback_msg, None, ctx.compact);
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
        if ctx.structured {
            let output = build_full_tx_output("no_matches", result, ctx.cwd);
            emit_output_json(&output, ctx.compact);
        }
        return Ok(exit::NO_MATCHES);
    }
    if ctx.structured {
        let output = build_full_tx_output("success", result, ctx.cwd);
        emit_output_json(&output, ctx.compact);
    } else if global.show_status() {
        let extra_deletions = result
            .deletions
            .iter()
            .filter(|p| !result.changes.iter().any(|(c, _, _)| c == *p))
            .count();
        let total = result.changes.len() + extra_deletions;
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
    crate::verbose!(
        "tx: plan={}, apply={}, check={}",
        args.plan,
        global.apply,
        global.check
    );

    // 1. Read plan from file or stdin.
    let plan_path = if args.plan == "-" {
        None
    } else {
        Some(global.resolve_user_path(&args.plan)?)
    };
    let plan_text = if args.plan == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let plan_path = plan_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("internal error: plan path missing"))?;
        std::fs::read_to_string(plan_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("failed to read plan file '{}': {e}", plan_path.display()),
                )
                .into()
            } else {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("failed to read plan file '{}': {e}", plan_path.display()),
                })
            }
        })?
    };

    crate::verbose!("tx: plan text length={}", plan_text.len());

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

    run_parsed_plan(plan, args.no_strict, &args.verify, global)
}

/// Execute an already-parsed plan (shared by `tx` CLI and in-process `batch`).
///
/// Batch builds a `Plan` in memory and must not re-enter via a temp plan path:
/// under `--contain`, absolute temp paths outside the workspace are rejected by
/// [`GlobalFlags::resolve_user_path`].
pub(crate) fn run_parsed_plan(
    plan: Plan,
    no_strict: bool,
    verify: &[String],
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    let structured = global.json || global.jsonl;
    let compact = global.jsonl;

    // Expand for_each (glob-driven batch) before validation.
    let base_cwd = global.resolve_cwd()?;
    let mut plan = plan;
    if plan.for_each.is_some()
        && let Err(e) = crate::plan::expand_for_each(&mut plan, &base_cwd)
    {
        let msg = e.to_string();
        if structured {
            emit_error_json("parse_error", &msg, None, compact);
        } else {
            eprintln!("tx: {msg}");
        }
        return Ok(exit::PARSE_ERROR);
    }

    crate::verbose!(
        "tx: parsed plan with {} operations, strict={:?}",
        plan.operations.len(),
        plan.strict
    );

    // 3. Validate plan and resolve working directory.
    let (cwd, strict, _resolved_global) =
        match crate::tx::validate_and_prepare_plan(&plan, &base_cwd, no_strict) {
            Ok(v) => v,
            Err(output) => {
                let kind = output
                    .error_kind
                    .clone()
                    .unwrap_or_else(|| "parse_error".into());
                let code = match kind.as_str() {
                    "parse_error" => exit::PARSE_ERROR,
                    "invalid_input" => exit::FAILURE,
                    _ => exit::FAILURE,
                };
                let msg = output.error.as_deref().unwrap_or("plan validation failed");
                let bs = output.backup_session.as_deref();
                if structured {
                    emit_error_json(&kind, msg, bs, compact);
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

    // Merge --verify CLI args with plan's verify field.
    #[cfg(feature = "ast")]
    let verify_checks = {
        use crate::plan::VerifyCheck;
        let mut checks: Vec<VerifyCheck> = plan.verify.clone().unwrap_or_default();
        for raw in verify {
            match VerifyCheck::parse(raw) {
                Ok(c) => checks.push(c),
                Err(e) => {
                    let msg = format!("invalid --verify spec '{raw}': {e}");
                    if structured {
                        emit_error_json("parse_error", &msg, None, compact);
                    } else {
                        eprintln!("tx: {msg}");
                    }
                    return Ok(exit::PARSE_ERROR);
                }
            }
        }
        checks
    };

    // 3c. Take pre-execution symbol snapshot for verification.
    #[cfg(feature = "ast")]
    let verify_before = if !verify_checks.is_empty() {
        let affected = crate::tx::verify::affected_file_paths(&plan, &cwd);
        verify_checks
            .iter()
            .map(|check| {
                let snap = crate::tx::verify::snapshot_symbols(&affected, check);
                (check.clone(), snap)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    crate::verbose!("tx: cwd={}, strict={}", cwd.display(), strict);

    // 4. Execute all operations, collecting changes in memory (no writes).
    let engine_ctx = crate::tx::context::EngineContext::from_global(global, cwd.clone());
    let guard = global.workspace_guard(&cwd)?;
    let mut result = match crate::tx::execute_and_collect(
        &plan,
        &engine_ctx,
        global.quiet,
        structured,
        guard.as_ref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            // Shared typed-kind table (no_matches, ambiguous, format_failed, …).
            // Unknown engine failures stay operation_failed (exit 9) for tx.
            let (kind, code) = match exit::classify_typed_error(&e) {
                Some((k, c)) => (k, c),
                None => ("operation_failed", exit::OPERATION_FAILED),
            };
            if structured {
                emit_error_json(kind, &msg, None, compact);
            } else {
                eprintln!("tx: {msg}");
            }
            return Ok(code);
        }
    };

    // 4b. Post-execution verification against pending content.
    #[cfg(feature = "ast")]
    if !verify_before.is_empty() {
        let affected = crate::tx::verify::affected_file_paths(&plan, &cwd);
        let mut any_failed = false;
        let mut messages = Vec::new();
        for (check, before_snap) in &verify_before {
            let after_snap =
                crate::tx::verify::snapshot_symbols_from_pending(&affected, &result.pending, check);
            let vr = crate::tx::verify::compare_snapshots(before_snap, &after_snap, check, &cwd);
            messages.push(vr.message.clone());
            if !vr.passed {
                any_failed = true;
            }
        }
        let summary = messages.join("\n");
        if any_failed {
            let msg = format!("verification failed, changes not applied:\n{summary}");
            if structured {
                emit_error_json("verification_failed", &msg, None, compact);
            } else {
                eprintln!("tx: {msg}");
            }
            return Ok(exit::VALIDATION_FAILED);
        }
        if !global.quiet {
            for m in &messages {
                eprintln!("tx: {m}");
            }
        }
    }

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
                if structured {
                    let output = build_full_tx_output("no_matches", &mut result, &cwd);
                    emit_output_json(&output, compact);
                }
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
        if structured {
            let output = build_full_tx_output("no_matches", &mut result, &cwd);
            emit_output_json(&output, compact);
        }
        return Ok(exit::NO_MATCHES);
    }

    // Default / --diff mode: show unified diffs.
    // When changes would apply (exit 2), JSON status must be changes_detected
    // (same as --check), not success. Agents branch on status as well as exit.
    let preview_status = if result.no_effective_changes {
        "success"
    } else {
        "changes_detected"
    };
    if structured {
        let output = build_full_tx_output(preview_status, &mut result, &cwd);
        emit_output_json(&output, compact);
    } else if !result.changes.is_empty() {
        print_diffs(&result.changes, &cwd, global.should_color());
    }
    if !result.no_effective_changes && global.show_status() {
        let extra_deletions = result
            .deletions
            .iter()
            .filter(|p| !result.changes.iter().any(|(c, _, _)| c == *p))
            .count();
        let n = result.changes.len() + extra_deletions;
        eprintln!("{n} file(s) changed");
    }

    // --confirm: prompt after showing diffs, then apply if confirmed.
    if !result.no_effective_changes && global.should_apply() {
        return commit_and_finalize(&ctx, &mut result, global, false);
    }

    if !result.no_effective_changes {
        Ok(exit::CHANGES_DETECTED)
    } else {
        Ok(exit::SUCCESS)
    }
}

#[path = "tx_tests.rs"]
#[cfg(test)]
mod tests;
