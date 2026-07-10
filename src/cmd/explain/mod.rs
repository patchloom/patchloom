//! Human-readable transaction plan summaries (`patchloom explain`).
//!
//! Operation prose is built in [`describe`]: rich field formatting with a
//! schema-registry label from [`crate::schema::operation_description`].

use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::Plan;

mod describe;
use clap::Args;
use describe::describe_operation;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom explain plan.json
  cat plan.json | patchloom explain --stdin
  patchloom explain plan.yaml --json")]
pub struct ExplainArgs {
    /// Path to a tx plan file (JSON, YAML, or TOML).
    #[arg(required_unless_present = "stdin")]
    pub path: Option<String>,

    /// Read plan from stdin instead of a file.
    #[arg(long)]
    pub stdin: bool,

    /// Format hint: json, yaml, or toml (auto-detected from extension).
    #[arg(long)]
    pub format: Option<String>,
}

pub fn run(args: ExplainArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "explain: path={:?}, stdin={}, format={:?}",
        args.path,
        args.stdin,
        args.format
    );
    let (input, path) = if args.stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        (buf, None)
    } else {
        let p = match args.path.as_deref() {
            Some(p) => p,
            None => {
                global.emit_error_json_kind(
                    Some("invalid_input"),
                    "path is required when --stdin is not set",
                )?;
                return Ok(exit::FAILURE);
            }
        };
        let full = global.resolve_user_path(p)?;
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("cannot read {}: {e}", full.display());
                global.emit_error_json_kind(Some("not_found"), &msg)?;
                return Ok(exit::FAILURE);
            }
        };
        (content, Some(p.to_string()))
    };

    let mut plan =
        match crate::plan::parse_plan_auto(&input, path.as_deref(), args.format.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                global.emit_error_json_kind(Some("parse_error"), &e.to_string())?;
                return Ok(exit::PARSE_ERROR);
            }
        };
    let cwd = global.resolve_cwd()?;

    // Expand for_each before summarizing so the explain output shows all
    // expanded operations (not the template).
    if plan.for_each.is_some() {
        crate::plan::expand_for_each(&mut plan, &cwd)?;
    }
    let config_strict = crate::config::find_and_load(&cwd)
        .map(|(config, _)| config.tx.strict)
        .unwrap_or(None);
    let strict = crate::plan::effective_strict(plan.strict, config_strict, false);

    if !global.emit_json(&build_json_summary(&plan, strict))? && !global.quiet {
        print_human_summary(&plan, strict);
    }

    Ok(exit::SUCCESS)
}

pub(super) fn print_human_summary(plan: &Plan, strict: bool) {
    let n = plan.operations.len();
    let mode = if strict { "strict" } else { "normal" };
    println!("Plan: {n} operation(s) ({mode} mode)\n");

    for (i, op) in plan.operations.iter().enumerate() {
        println!("  {}. {}", i + 1, describe_operation(op));
    }

    if let Some(ref wp) = plan.write_policy {
        let mut parts = Vec::new();
        if wp.ensure_final_newline == Some(true) {
            parts.push("ensure final newline");
        }
        if wp.trim_trailing_whitespace == Some(true) {
            parts.push("trim trailing whitespace");
        }
        if let Some(ref eol) = wp.normalize_eol {
            parts.push(match eol.as_str() {
                "lf" => "normalize EOL to LF",
                "crlf" => "normalize EOL to CRLF",
                "cr" => "normalize EOL to CR",
                _ => "normalize EOL",
            });
        }
        if wp.collapse_blanks == Some(true) {
            parts.push("collapse blank lines");
        }
        if wp.respect_editorconfig == Some(true) {
            parts.push("respect editorconfig");
        }
        if !parts.is_empty() {
            println!("\nWrite policy: {}", parts.join(", "));
        }
    }

    if let Some(ref steps) = plan.format {
        for step in steps {
            println!("Format: {}{}", step.cmd, format_timeout(step.timeout));
        }
    }

    if let Some(ref steps) = plan.validate {
        for step in steps {
            let req = if step.required == Some(true) {
                "required"
            } else {
                "advisory"
            };
            println!(
                "Validate: {} ({req}){}",
                step.cmd,
                format_timeout(step.timeout)
            );
        }
    }

    if let Some(ref checks) = plan.verify {
        for check in checks {
            match check {
                crate::plan::VerifyCheck::SymbolCount { kind, attr } => {
                    if let Some(a) = attr {
                        println!("Verify: count {kind} symbols with attr={a}");
                    } else {
                        println!("Verify: count {kind} symbols");
                    }
                }
                crate::plan::VerifyCheck::Named { check } => {
                    println!("Verify: {check}");
                }
            }
        }
    }
}

pub(super) fn format_timeout(timeout: Option<u64>) -> String {
    match timeout {
        Some(t) => format!(" (timeout: {t}s)"),
        None => String::new(),
    }
}

/// JSON summary for `--json` / `--jsonl` explain output.
///
/// Each operation includes the serde `op` name, the schema-registry catalog
/// blurb (aligned with `patchloom schema` / MCP), and the rich field summary.
pub(super) fn build_json_summary(plan: &Plan, strict: bool) -> serde_json::Value {
    let ops: Vec<serde_json::Value> = plan
        .operations
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let op_name = describe::operation_op_name(op);
            let catalog = crate::schema::operation_description(&op_name);
            serde_json::json!({
                "index": i + 1,
                "op": op_name,
                "catalog": catalog,
                "description": describe_operation(op),
            })
        })
        .collect();

    serde_json::json!({
        "ok": true,
        "operation_count": plan.operations.len(),
        "strict": strict,
        "operations": ops,
        "has_write_policy": plan.write_policy.is_some(),
        "format_steps": plan.format.as_ref().map(|f| f.len()).unwrap_or(0),
        "validate_steps": plan.validate.as_ref().map(|v| v.len()).unwrap_or(0),
        "verify_checks": plan.verify.as_ref().map(|v| v.len()).unwrap_or(0),
    })
}
