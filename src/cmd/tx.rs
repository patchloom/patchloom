use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::plan::{self, Plan};
use crate::tx::{
    CommitError, TxExecResult, TxOutput, build_error_output, build_full_tx_output,
    format_error_with_backup_hint, run_lifecycle,
};
use crate::write::run_format_command;

use clap::Args;

use std::path::{Path, PathBuf};

#[cfg_attr(not(feature = "cli"), allow(dead_code))]
#[derive(Debug, Args)]
#[non_exhaustive]
#[command(after_help = "\
EXAMPLES:
  patchloom tx plan.json
  patchloom tx plan.json --apply
  patchloom tx plan.yaml --check --json")]
pub struct TxArgs {
    // ref:tx-mode:plan-stdin
    /// Path to a plan file (JSON/YAML/TOML), or `-` for stdin.
    pub plan: String,
    // ref:tx-mode:plan-yaml
    /// Plan format when reading from stdin (json, yaml, toml). Auto-detected from file extension otherwise.
    #[arg(long)]
    pub plan_format: Option<String>,
    /// Disable strict rollback on format/validate failure.
    #[arg(long)]
    pub no_strict: bool,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::replace::replacement_text;
    use crate::tx::{read_and_probe, read_file_content, update_file_content, validate_operation};
    use crate::write::WritePolicy;
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use tempfile::TempDir;

    fn shell_true() -> &'static str {
        #[cfg(windows)]
        {
            "exit /b 0"
        }
        #[cfg(not(windows))]
        {
            "true"
        }
    }

    fn shell_false() -> &'static str {
        #[cfg(windows)]
        {
            "exit /b 1"
        }
        #[cfg(not(windows))]
        {
            "false"
        }
    }

    fn shell_stderr_spam() -> &'static str {
        #[cfg(windows)]
        {
            // -NoProfile skips profile loading which can take 1-2 seconds on CI.
            // Single Write() call instead of 4000 iterations keeps runtime well
            // under the timeout even on slow runners.
            "powershell -NoProfile -Command \"[Console]::Error.Write('x' * 800000); exit 0\""
        }
        #[cfg(not(windows))]
        {
            "python3 -c \"import sys; sys.stderr.write('x' * (1024 * 1024)); sys.stderr.flush()\""
        }
    }

    fn portable_path_str(p: &std::path::Path) -> String {
        p.to_str().unwrap().replace('\\', "/")
    }

    #[test]
    fn read_file_content_populates_pending_from_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("existing.txt");
        fs::write(&path, "hello from disk\n").unwrap();
        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        let content = read_file_content(&mut pending, &mut existed, &path).unwrap();

        assert_eq!(content, "hello from disk\n");
        assert_eq!(
            pending.get(&path),
            Some(&(
                "hello from disk\n".to_string(),
                "hello from disk\n".to_string()
            ))
        );
    }

    #[test]
    fn read_and_probe_returns_true_for_text() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("text.rs");
        fs::write(&path, "fn main() {}\n").unwrap();
        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        assert!(read_and_probe(&mut pending, &mut existed, &path).unwrap());
        assert!(pending.contains_key(&path));
        assert_eq!(pending[&path].1, "fn main() {}\n");
    }

    #[test]
    fn read_and_probe_returns_false_for_binary() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.bin");
        fs::write(&path, b"ELF\x00\x01\x02\x03").unwrap();
        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        assert!(!read_and_probe(&mut pending, &mut existed, &path).unwrap());
        assert!(!pending.contains_key(&path));
    }

    #[test]
    fn read_and_probe_returns_false_for_invalid_utf8() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.txt");
        // Bare continuation bytes: invalid UTF-8 but no NUL (passes binary check)
        fs::write(&path, [0x80, 0x81, 0x82]).unwrap();
        let mut pending = HashMap::new();
        let mut existed = HashSet::new();

        // Should skip gracefully (false) instead of erroring,
        // matching standalone search/replace behavior.
        assert!(!read_and_probe(&mut pending, &mut existed, &path).unwrap());
        assert!(!pending.contains_key(&path));
    }

    #[test]
    fn update_file_content_inserts_missing_pending_entry() {
        let mut pending = HashMap::new();
        let mut deletions = HashSet::new();
        let path = PathBuf::from("created.txt");

        update_file_content(&mut pending, &mut deletions, &path, "brand new".to_string());

        assert_eq!(
            pending.get(&path),
            Some(&(String::new(), "brand new".to_string()))
        );
    }

    #[test]
    fn multi_op_plan() {
        let dir = TempDir::new().unwrap();

        // Create test files.
        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let json_file = dir.path().join("config.json");
        fs::write(&json_file, r#"{"name": "old"}"#).unwrap();

        let no_nl = dir.path().join("no_nl.txt");
        fs::write(&no_nl, "content").unwrap();

        // Build plan with replace + doc.set + tidy.fix.
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": json_file.to_str().unwrap(),
                    "selector": "name",
                    "value": "new"
                },
                {
                    "op": "tidy.fix",
                    "path": no_nl.to_str().unwrap(),
                    "ensure_final_newline": true
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify replace.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hi world\n");

        // Verify doc.set.
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&json_file).unwrap()).unwrap();
        assert_eq!(config["name"], serde_json::json!("new"));

        // Verify tidy.fix.
        assert!(fs::read_to_string(&no_nl).unwrap().ends_with('\n'));
    }

    #[test]
    fn create_then_delete_in_same_tx_becomes_noop() {
        let dir = TempDir::new().unwrap();
        let new_file = dir.path().join("new.txt");
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {"op": "file.create", "path": new_file.to_str().unwrap(), "content": "hello"},
                {"op": "file.delete", "path": new_file.to_str().unwrap()}
            ]
        });
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!new_file.exists());
    }

    #[test]
    fn rollback_on_failure() {
        let dir = TempDir::new().unwrap();

        let txt = dir.path().join("test.txt");
        fs::write(&txt, "hello world\n").unwrap();

        let nonexistent = dir.path().join("nonexistent.json");

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "replace",
                    "path": txt.to_str().unwrap(),
                    "from": "hello",
                    "to": "hi"
                },
                {
                    "op": "doc.set",
                    "path": nonexistent.to_str().unwrap(),
                    "selector": "name",
                    "value": "test"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::OPERATION_FAILED);

        // Verify no files were modified.
        assert_eq!(fs::read_to_string(&txt).unwrap(), "hello world\n");
    }

    #[test]
    fn legacy_error_prefix_maps_format_failed() {
        assert_eq!(legacy_error_prefix("format_failed"), "validation_failed");
        assert_eq!(legacy_error_prefix("rollback"), "rollback");
        assert_eq!(legacy_error_prefix("parse_error"), "parse_error");
    }

    #[test]
    fn build_error_output_produces_expected_shape() {
        let output = build_error_output("rollback", "rollback", "disk full", None);
        assert!(!output.ok);
        assert_eq!(output.status, "error".to_string());
        assert_eq!(output.error_kind, Some("rollback".to_string()));
        assert_eq!(output.error, Some("rollback: disk full".to_string()));
        assert_eq!(output.files_changed, 0);
    }

    #[test]
    fn validation_pass() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [],
            "validate": [
                {"cmd": shell_true(), "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn validation_fail_required() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "strict": false,
            "operations": [],
            "validate": [
                {"cmd": shell_false(), "required": true}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::VALIDATION_FAILED);
    }

    #[test]
    fn validation_pass_with_large_stderr_output() {
        let dir = TempDir::new().unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [],
            "validate": [
                {"cmd": shell_stderr_spam(), "required": true, "timeout": 10}
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;
        global.cwd = Some(dir.path().to_string_lossy().into_owned());

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn parse_plan_json_string() {
        let plan_json = r#"{"version": "1", "operations": [{"op": "replace", "path": "test.txt", "from": "a", "to": "b"}]}"#;
        let plan = plan::parse_plan(plan_json).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn malformed_plan_returns_parse_error() {
        let dir = TempDir::new().unwrap();

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, "not json at all {{{").unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_default();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn replace_requires_replacement_mode() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "replace operation requires one of to, insert_before, or insert_after"
        );
    }

    #[test]
    fn replace_conflicting_insert_fields_return_parse_error() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "insert_before": "X",
                "insert_after": "Y"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "insert_before and insert_after cannot both be set"
        );
    }

    #[test]
    fn regex_insert_before_uses_match_anchor_in_replacement_text() {
        let text = replacement_text("b+", &None, &Some("X".to_string()), &None, true);

        assert_eq!(text, "X${0}");
    }

    #[test]
    fn regex_insert_after_uses_match_anchor_in_replacement_text() {
        let text = replacement_text("b+", &None, &None, &Some("X".to_string()), true);

        assert_eq!(text, "${0}X");
    }

    #[test]
    fn replace_to_with_insert_returns_parse_error() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "to": "world",
                "insert_before": "X"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "to cannot be combined with insert_before or insert_after"
        );
    }

    #[test]
    fn tx_file_create_in_plan() {
        let dir = TempDir::new().unwrap();

        let new_file = dir.path().join("created.txt");

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.create",
                    "path": new_file.to_str().unwrap(),
                    "content": "brand new file\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Verify the file was created with the correct content.
        assert!(new_file.exists());
        assert_eq!(fs::read_to_string(&new_file).unwrap(), "brand new file\n");
    }

    #[test]
    fn tx_file_create_existing_fails() {
        let dir = TempDir::new().unwrap();

        let existing = dir.path().join("existing.txt");
        fs::write(&existing, "original content\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.create",
                    "path": existing.to_str().unwrap(),
                    "content": "should not overwrite\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::OPERATION_FAILED);

        // Verify the original file was NOT modified.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "original content\n");
    }

    #[test]
    fn validate_operation_rejects_empty_from() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "",
                "to": "x"
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string().contains("non-empty search pattern"),
            "expected 'non-empty search pattern' error, got: {}",
            err
        );
    }

    #[test]
    fn validate_operation_rejects_nth_zero() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello",
                "to": "world",
                "nth": 0
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string().contains("1-based"),
            "expected '1-based' error, got: {}",
            err
        );
    }

    #[test]
    fn validate_operation_rejects_invert_match_with_multiline() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "search",
                "path": "test.txt",
                "pattern": "foo",
                "invert_match": true,
                "multiline": true
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string()
                .contains("invert_match and multiline cannot be combined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_operation_rejects_literal_with_regex() {
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "search",
                "path": "test.txt",
                "pattern": "foo",
                "literal": true,
                "regex": true
            }]
        })
        .to_string();
        let plan = plan::parse_plan(&plan_json).unwrap();

        let err = validate_operation(&plan.operations[0]).unwrap_err();
        assert!(
            err.to_string()
                .contains("literal and regex cannot be combined"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn tx_file_rename_basic() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        fs::write(&src, "content\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dir.path().join("new.txt"))
            }]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "content\n"
        );
    }

    #[test]
    fn tx_file_rename_same_path_noop() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("same.txt");
        fs::write(&f, "data\n").unwrap();

        let path_str = portable_path_str(&f);
        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": path_str,
                "to": path_str
            }]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(fs::read_to_string(&f).unwrap(), "data\n");
    }

    #[test]
    fn tx_file_rename_dst_exists_without_force_rolls_back() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        fs::write(&src, "source\n").unwrap();
        fs::write(&dst, "dest\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dst)
            }]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::OPERATION_FAILED);
        // Both files unchanged.
        assert_eq!(fs::read_to_string(&src).unwrap(), "source\n");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "dest\n");
    }

    #[test]
    fn tx_file_rename_dst_exists_with_force() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        fs::write(&src, "source\n").unwrap();
        fs::write(&dst, "dest\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "file.rename",
                "from": portable_path_str(&src),
                "to": portable_path_str(&dst),
                "force": true
            }]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "source\n");
    }

    #[test]
    fn tx_create_then_rename() {
        let dir = TempDir::new().unwrap();
        let created = dir.path().join("created.txt");
        let renamed = dir.path().join("renamed.txt");

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.create",
                    "path": portable_path_str(&created),
                    "content": "new file\n"
                },
                {
                    "op": "file.rename",
                    "from": portable_path_str(&created),
                    "to": portable_path_str(&renamed)
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!created.exists());
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "new file\n");
    }

    #[test]
    fn tx_unsupported_plan_version() {
        let dir = TempDir::new().unwrap();
        let plan_json = serde_json::json!({
            "version": "99",
            "operations": []
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_default();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn tx_delete_then_create_without_force_succeeds() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("target.txt");
        fs::write(&f, "original\n").unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "operations": [
                {
                    "op": "file.delete",
                    "path": portable_path_str(&f)
                },
                {
                    "op": "file.create",
                    "path": portable_path_str(&f),
                    "content": "recreated\n"
                }
            ]
        });

        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, serde_json::to_string(&plan_json).unwrap()).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(fs::read_to_string(&f).unwrap(), "recreated\n");
    }

    // -----------------------------------------------------------------------
    // #387: Medium-priority test gaps
    // -----------------------------------------------------------------------

    #[test]
    fn plan_normalize_eol_invalid_value_returns_error() {
        let result = crate::tx::plan_normalize_eol("bogus");
        let msg = result
            .expect_err("invalid eol value should fail")
            .to_string();
        assert!(
            msg.contains("invalid normalize_eol"),
            "error should name the field: {msg}"
        );
    }

    #[test]
    fn rollback_strict_restores_modified_files() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("a.txt");
        fs::write(&f, "original").unwrap();

        let mut pending = HashMap::new();
        pending.insert(f.clone(), ("original".to_string(), "changed".to_string()));
        let deletions = HashSet::new();
        let existed_before: HashSet<PathBuf> = [f.clone()].into();

        // Simulate the apply phase: write the new content.
        crate::write::atomic_write(&f, "changed", &WritePolicy::default()).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "changed");

        // Rollback should restore to original.
        crate::tx::rollback_strict(
            &[(f.clone(), "original".to_string(), "changed".to_string())],
            &pending,
            &deletions,
            &existed_before,
        );
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            "original",
            "rollback_strict should restore the original content"
        );
    }

    #[test]
    fn tx_doc_append_to_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.append", "path": "{}", "selector": "items", "value": 42}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::OPERATION_FAILED,
            "doc.append to non-array should fail before commit"
        );
    }

    #[test]
    fn tx_doc_prepend_to_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.prepend", "path": "{}", "selector": "items", "value": 99}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::OPERATION_FAILED,
            "doc.prepend to non-array should fail before commit"
        );
    }

    #[test]
    fn tx_doc_delete_where_on_non_array_rolls_back() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": "not-an-array"}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.delete_where", "path": "{}", "selector": "items", "predicate": "x=1"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::OPERATION_FAILED,
            "doc.delete_where on non-array should fail before commit"
        );
    }

    #[test]
    fn tx_doc_delete_missing_key_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"a": 1}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.delete", "path": "{}", "selector": "nonexistent"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::SUCCESS,
            "doc.delete on missing key should succeed (idempotent)"
        );
    }

    #[test]
    fn tx_doc_delete_where_no_match_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.json");
        fs::write(&f, r#"{"items": [{"name": "a"}]}"#).unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "doc.delete_where", "path": "{}", "selector": "items", "predicate": "name=nonexistent"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::SUCCESS,
            "doc.delete_where with zero matches should succeed (idempotent)"
        );
    }

    #[test]
    fn tx_strict_mode_rollback_on_operation_failure() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("data.txt");
        fs::write(&f, "original content").unwrap();

        // Operation 1 succeeds (replace), operation 2 fails (read nonexistent).
        // Strict mode should revert operation 1's changes.
        let plan = format!(
            r#"{{
  "version": "1",
  "strict": true,
  "operations": [
    {{"op": "replace", "path": "{}", "from": "original", "to": "modified"}},
    {{"op": "read", "path": "nonexistent-file.txt"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::OPERATION_FAILED,
            "strict plan with failing op should fail before commit"
        );

        // File should remain untouched because execution failed before any writes.
        let content = fs::read_to_string(&f).unwrap();
        assert_eq!(content, "original content", "file should remain unchanged");
    }

    #[test]
    fn tx_invalid_normalize_eol_in_plan() {
        let dir = TempDir::new().unwrap();
        let f = dir.path().join("file.txt");
        fs::write(&f, "hello\n").unwrap();

        let plan = format!(
            r#"{{
  "version": "1",
  "operations": [
    {{"op": "tidy.fix", "path": "{}", "normalize_eol": "bogus"}}
  ]
}}"#,
            portable_path_str(&f)
        );
        let plan_file = dir.path().join("plan.json");
        fs::write(&plan_file, &plan).unwrap();

        let args = TxArgs {
            plan: plan_file.to_str().unwrap().to_string(),
            plan_format: None,
            no_strict: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(
            code,
            exit::OPERATION_FAILED,
            "invalid normalize_eol should fail before commit"
        );
    }

    #[test]
    fn handle_commit_error_rollback_failed_exits_failure() {
        let err = CommitError {
            message: "write failed".into(),
            rollback_ok: false,
            backup_session: Some("1234567890".into()),
        };
        let code = handle_commit_error(err, true, false).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn commit_changes_reports_rollback_failed_when_restore_fails() {
        let dir = TempDir::new().unwrap();
        let f1 = dir.path().join("a.txt");
        let blocker = dir.path().join("blocker");
        fs::write(&f1, "original-a").unwrap();
        fs::write(&blocker, "blocker-is-file").unwrap();
        let f2 = blocker.join("b.txt");

        let changes = vec![
            (f1.clone(), "original-a".to_string(), "new-a".to_string()),
            (f2.clone(), String::new(), "new-b".to_string()),
        ];
        let deletions = HashSet::new();
        let existed_before: HashSet<_> = [f1.clone()].into();

        let _guard = RestoreFailGuard::engage();
        let err = crate::tx::commit_changes(&changes, &deletions, &existed_before, dir.path())
            .unwrap_err();

        assert!(!err.rollback_ok, "restore should be reported as failed");
        let _session = err
            .backup_session
            .expect("backup session should be present on failure");
        assert_eq!(fs::read_to_string(&f1).unwrap(), "new-a");
    }

    #[test]
    fn commit_changes_rolls_back_on_mid_write_failure() {
        let dir = TempDir::new().unwrap();
        let f1 = dir.path().join("a.txt");
        let blocker = dir.path().join("blocker");
        fs::write(&f1, "original-a").unwrap();
        fs::write(&blocker, "blocker-is-file").unwrap();
        let f2 = blocker.join("b.txt");

        let changes = vec![
            (f1.clone(), "original-a".to_string(), "new-a".to_string()),
            (f2.clone(), String::new(), "new-b".to_string()),
        ];
        let deletions = HashSet::new();
        let existed_before: HashSet<_> = [f1.clone()].into();

        let err = crate::tx::commit_changes(&changes, &deletions, &existed_before, dir.path())
            .unwrap_err();
        assert!(err.rollback_ok, "rollback should succeed");
        assert!(
            err.backup_session.is_some(),
            "backup session should be recorded"
        );
        assert_eq!(fs::read_to_string(&f1).unwrap(), "original-a");
        assert!(!f2.exists());
    }

    #[test]
    fn undo_list_json_output() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "v1").unwrap();
        let mut session = crate::backup::BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap().unwrap();

        let args = crate::cmd::undo::UndoArgs {
            list: true,
            session: None,
            apply: false,
        };
        let global = GlobalFlags::with_cwd_and_json(dir.path());
        let code = crate::cmd::undo::run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Also test JSONL mode.
        let global = GlobalFlags::with_cwd_and_jsonl(dir.path());
        let args2 = crate::cmd::undo::UndoArgs {
            list: true,
            session: None,
            apply: false,
        };
        let code2 = crate::cmd::undo::run(args2, &global).unwrap();
        assert_eq!(code2, exit::SUCCESS);
    }

    #[test]
    fn undo_dry_run_json_output() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("b.txt");
        fs::write(&file, "v1").unwrap();
        let mut session = crate::backup::BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Modify file so dry-run shows changes.
        fs::write(&file, "v2").unwrap();

        let args = crate::cmd::undo::UndoArgs {
            list: false,
            session: Some(ts),
            apply: false,
        };
        let global = GlobalFlags::with_cwd_and_json(dir.path());
        let code = crate::cmd::undo::run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn path_err_formats_flat_error_with_path_prefix() {
        let err = crate::tx::path_err("config.toml")(anyhow::anyhow!("key not found"));
        let msg = err.to_string();
        assert_eq!(msg, "config.toml: key not found");
    }
}
