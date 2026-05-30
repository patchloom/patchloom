use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::{Operation, Plan};
use clap::Args;

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
    let (input, path) = if args.stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        (buf, None)
    } else {
        let p = args
            .path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("path is required when --stdin is not set"))?;
        let cwd = global.resolve_cwd()?;
        let full = cwd.join(p);
        let content = std::fs::read_to_string(&full)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", full.display()))?;
        (content, Some(p.to_string()))
    };

    let plan = crate::plan::parse_plan_auto(&input, path.as_deref(), args.format.as_deref())?;

    if global.json || global.jsonl {
        let summary = build_json_summary(&plan);
        global.emit_json(&summary)?;
    } else if !global.quiet {
        print_human_summary(&plan);
    }

    Ok(exit::SUCCESS)
}

fn print_human_summary(plan: &Plan) {
    let n = plan.operations.len();
    let mode = if plan.strict { "strict" } else { "normal" };
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
                _ => "normalize EOL",
            });
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
}

fn format_timeout(timeout: Option<u64>) -> String {
    match timeout {
        Some(t) => format!(" (timeout: {t}s)"),
        None => String::new(),
    }
}

fn describe_operation(op: &Operation) -> String {
    match op {
        Operation::Replace {
            path,
            glob,
            from,
            to,
            nth,
            mode,
            case_insensitive,
            insert_before,
            insert_after,
            ..
        } => {
            let target = path.as_deref().or(glob.as_deref()).unwrap_or("(all files)");
            let is_regex = mode.as_deref() == Some("regex");
            let mode_str = if is_regex { "regex" } else { "literal" };

            if let Some(before) = insert_before {
                return format!("Insert \"{before}\" before \"{from}\" in {target}");
            }
            if let Some(after) = insert_after {
                return format!("Insert \"{after}\" after \"{from}\" in {target}");
            }

            let to_str = to.as_deref().unwrap_or("(delete)");
            let nth_str = nth
                .map(|n| format!(", occurrence #{n}"))
                .unwrap_or_default();
            let ci_str = if *case_insensitive {
                ", case-insensitive"
            } else {
                ""
            };
            format!(
                "Replace \"{from}\" with \"{to_str}\" in {target} ({mode_str}{nth_str}{ci_str})"
            )
        }
        Operation::DocSet {
            path,
            selector,
            value,
        } => {
            format!("Set {selector} to {value} in {path}")
        }
        Operation::DocDelete { path, selector } => {
            format!("Delete key {selector} from {path}")
        }
        Operation::DocMerge { path, value } => {
            format!("Merge {value} into {path}")
        }
        Operation::DocAppend {
            path,
            selector,
            value,
        } => {
            format!("Append {value} to {selector} in {path}")
        }
        Operation::DocPrepend {
            path,
            selector,
            value,
        } => {
            format!("Prepend {value} to {selector} in {path}")
        }
        Operation::DocUpdate {
            path,
            selector,
            value,
        } => {
            format!("Update {selector} to {value} in {path}")
        }
        Operation::DocMove { path, from, to } => {
            format!("Move {from} to {to} in {path}")
        }
        Operation::DocEnsure {
            path,
            selector,
            value,
        } => {
            format!("Ensure {selector} = {value} in {path}")
        }
        Operation::DocDeleteWhere {
            path,
            selector,
            predicate,
        } => {
            format!("Delete from {selector} where {predicate} in {path}")
        }
        Operation::MdReplaceSection { path, heading, .. } => {
            format!("Replace section \"{heading}\" in {path}")
        }
        Operation::MdInsertAfterHeading { path, heading, .. } => {
            format!("Insert content after \"{heading}\" in {path}")
        }
        Operation::MdInsertBeforeHeading { path, heading, .. } => {
            format!("Insert content before \"{heading}\" in {path}")
        }
        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            format!("Upsert bullet \"{bullet}\" under \"{heading}\" in {path}")
        }
        Operation::MdTableAppend { path, heading, .. } => {
            format!("Append row to table under \"{heading}\" in {path}")
        }
        Operation::MdDedupeHeadings { path } => {
            format!("Deduplicate headings in {path}")
        }
        Operation::TidyFix { path, .. } => {
            format!("Normalize whitespace in {path}")
        }
        Operation::FileCreate { path, force, .. } => {
            let force_str = if *force == Some(true) {
                " (overwrite)"
            } else {
                ""
            };
            format!("Create file {path}{force_str}")
        }
        Operation::FileDelete { path } => {
            format!("Delete file {path}")
        }
        Operation::FileRename { from, to, force } => {
            let force_str = if *force { " (overwrite)" } else { "" };
            format!("Rename {from} to {to}{force_str}")
        }
        Operation::PatchApply { .. } => "Apply unified diff patch".to_string(),
        Operation::Search {
            path,
            pattern,
            regex,
            ..
        } => {
            let mode = if *regex { "regex" } else { "literal" };
            format!("Search for \"{pattern}\" in {path} ({mode})")
        }
        Operation::Read { path, lines } => match lines {
            Some(range) => format!("Read {path} lines {range}"),
            None => format!("Read {path}"),
        },
        Operation::MdLintAgents { path } => {
            format!("Lint {path} for AGENTS.md issues")
        }
    }
}

fn build_json_summary(plan: &Plan) -> serde_json::Value {
    let ops: Vec<serde_json::Value> = plan
        .operations
        .iter()
        .enumerate()
        .map(|(i, op)| {
            serde_json::json!({
                "index": i + 1,
                "description": describe_operation(op),
            })
        })
        .collect();

    serde_json::json!({
        "operation_count": plan.operations.len(),
        "strict": plan.strict,
        "operations": ops,
        "has_write_policy": plan.write_policy.is_some(),
        "format_steps": plan.format.as_ref().map(|f| f.len()).unwrap_or(0),
        "validate_steps": plan.validate.as_ref().map(|v| v.len()).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{FormatStep, ValidationStep};

    #[test]
    fn describe_replace_literal() {
        let op = Operation::Replace {
            path: Some("README.md".into()),
            glob: None,
            mode: None,
            from: "v1".into(),
            to: Some("v2".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(desc, r#"Replace "v1" with "v2" in README.md (literal)"#);
    }

    #[test]
    fn describe_replace_regex_nth() {
        let op = Operation::Replace {
            path: Some("src/lib.rs".into()),
            glob: None,
            mode: Some("regex".into()),
            from: r"fn\s+main".into(),
            to: Some("fn entry".into()),
            nth: Some(1),
            insert_before: None,
            insert_after: None,
            case_insensitive: true,
            multiline: false,
            if_exists: false,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains("regex"));
        assert!(desc.contains("#1"));
        assert!(desc.contains("case-insensitive"));
    }

    #[test]
    fn describe_doc_set() {
        let op = Operation::DocSet {
            path: "package.json".into(),
            selector: "version".into(),
            value: serde_json::json!("2.0.0"),
        };
        assert_eq!(
            describe_operation(&op),
            r#"Set version to "2.0.0" in package.json"#
        );
    }

    #[test]
    fn describe_file_create() {
        let op = Operation::FileCreate {
            path: "new.txt".into(),
            content: "hello".into(),
            force: Some(true),
        };
        assert!(describe_operation(&op).contains("(overwrite)"));
    }

    #[test]
    fn describe_search_regex() {
        let op = Operation::Search {
            path: "src".into(),
            pattern: "TODO".into(),
            regex: true,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
        };
        assert!(describe_operation(&op).contains("(regex)"));
    }

    #[test]
    fn human_summary_output() {
        let plan = Plan {
            version: "1".into(),
            cwd: None,
            write_policy: None,
            strict: true,
            operations: vec![
                Operation::FileCreate {
                    path: "test.txt".into(),
                    content: "hi".into(),
                    force: None,
                },
                Operation::FileDelete {
                    path: "old.txt".into(),
                },
            ],
            format: None,
            validate: None,
        };
        // Just ensure it doesn't panic.
        print_human_summary(&plan);
    }

    #[test]
    fn json_summary_structure() {
        let plan = Plan {
            version: "1".into(),
            cwd: None,
            write_policy: None,
            strict: false,
            operations: vec![Operation::FileDelete {
                path: "x.txt".into(),
            }],
            format: Some(vec![FormatStep {
                cmd: "fmt".into(),
                timeout: Some(30),
            }]),
            validate: Some(vec![ValidationStep {
                cmd: "test".into(),
                required: Some(true),
                timeout: None,
            }]),
        };
        let json = build_json_summary(&plan);
        assert_eq!(json["operation_count"], 1);
        assert_eq!(json["format_steps"], 1);
        assert_eq!(json["validate_steps"], 1);
        assert!(!json["strict"].as_bool().unwrap());
    }
}
