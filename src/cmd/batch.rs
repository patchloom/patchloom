use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::{Operation, Plan};
use clap::Args;

/// Maximum number of operations in a single batch. Prevents unbounded
/// memory allocation from accidentally or maliciously large inputs.
pub const MAX_BATCH_OPERATIONS: usize = 10_000;

/// Execute multiple operations from a simple line-oriented format.
///
/// Each line is one operation with positional arguments:
///
/// ```text
/// doc.set <path> <selector> <value>
/// doc.delete <path> <selector>
/// doc.merge <path> <json-value>
/// doc.ensure <path> <selector> <value>
/// doc.append <path> <selector> <value>
/// doc.prepend <path> <selector> <value>
/// doc.update <path> <selector> <value>
/// doc.move <path> <from> <to>
/// doc.delete_where <path> <selector> <predicate>
/// replace <path> <from> <to>
/// file.create <path> <content>
/// file.delete <path>
/// file.rename <from> <to>
/// md.upsert_bullet <path> <heading> <bullet>
/// md.table_append <path> <heading> <row>
/// md.replace_section <path> <heading> <content>
/// md.insert_after_heading <path> <heading> <content>
/// md.insert_before_heading <path> <heading> <content>
/// md.move_section <path> <heading> before|after <target_heading>
/// md.move_section <path> <heading> <to> before|after <target_heading>
/// md.dedupe_headings <path>
/// md.lint_agents <path>
/// tidy.fix <path>
/// ```
///
/// Lines starting with `#` are comments. Empty lines are ignored.
/// Values containing spaces must be quoted with double quotes.
#[derive(Debug, Args)]
#[command(after_help = r#"OPERATIONS:
  doc.set, doc.delete, doc.merge, doc.ensure, doc.append, doc.prepend,
  doc.update, doc.move, doc.delete_where, replace, file.create,
  file.delete, file.rename, md.upsert_bullet, md.table_append,
  md.replace_section, md.insert_after_heading, md.insert_before_heading,
  md.move_section, md.dedupe_headings, md.lint_agents, tidy.fix

EXAMPLES:
  printf 'doc.set config.json version "2.0"\nreplace README.md v1 v2\n' | patchloom batch
  patchloom batch --apply <<'EOF'
  doc.set package.json version "3.0.0"
  replace README.md "v1.0" "v3.0"
  EOF"#)]
pub struct BatchArgs {
    /// Read operations from a file, or stdin if omitted.
    #[arg(default_value = "-")]
    pub input: String,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

/// Parse a single line into an Operation.
/// Small helper macro used inside parse_line arms to cut the repetitive
/// "Ok(Operation::Variant { ... })" boilerplate.
macro_rules! op {
    ($Variant:ident { $($tt:tt)* }) => {
        Ok(Operation::$Variant { $($tt)* })
    };
}

/// Helper macros for batch operation parsing.
///
/// `doc_psv!` handles the common doc pattern: 3 args = path + selector + json_value.
/// `doc_ps!` handles 2-arg doc ops: path + selector.
/// `md_phc!` handles 3-arg markdown ops: path + heading + content (third field varies).
macro_rules! doc_psv {
    ($op_name:expr, $args:expr, $line_num:expr, $Variant:ident) => {{
        require_args($op_name, $args, 3, $line_num)?;
        let value = parse_json_value(&$args[2])?;
        op!($Variant {
            path: $args[0].clone(),
            selector: $args[1].clone(),
            value
        })
    }};
}

macro_rules! md_phc {
    ($op_name:expr, $args:expr, $line_num:expr, $Variant:ident, $field3:ident) => {{
        require_args($op_name, $args, 3, $line_num)?;
        op!($Variant {
            path: $args[0].clone(),
            heading: $args[1].clone(),
            $field3: $args[2].clone()
        })
    }};
}

fn parse_line(line: &str, line_num: usize) -> anyhow::Result<Operation> {
    let tokens = tokenize(line).map_err(|e| anyhow::anyhow!("line {line_num}: {e}"))?;
    if tokens.is_empty() {
        anyhow::bail!("line {line_num}: empty operation");
    }
    let op = tokens[0].as_str();
    let args = &tokens[1..];

    match op {
        // -- doc operations (path + selector + value) --------------------------
        "doc.set" => doc_psv!(op, args, line_num, DocSet),
        "doc.ensure" => doc_psv!(op, args, line_num, DocEnsure),
        "doc.append" => doc_psv!(op, args, line_num, DocAppend),
        "doc.prepend" => doc_psv!(op, args, line_num, DocPrepend),
        "doc.update" => doc_psv!(op, args, line_num, DocUpdate),

        "doc.delete" => {
            require_args(op, args, 2, line_num)?;
            op!(DocDelete {
                path: args[0].clone(),
                selector: args[1].clone()
            })
        }
        "doc.merge" => {
            require_args(op, args, 2, line_num)?;
            let value = parse_json_value(&args[1])?;
            op!(DocMerge {
                path: args[0].clone(),
                value
            })
        }
        "doc.move" => {
            require_args(op, args, 3, line_num)?;
            op!(DocMove {
                path: args[0].clone(),
                from: args[1].clone(),
                to: args[2].clone()
            })
        }
        "doc.delete_where" => {
            require_args(op, args, 3, line_num)?;
            op!(DocDeleteWhere {
                path: args[0].clone(),
                selector: args[1].clone(),
                predicate: args[2].clone()
            })
        }

        // -- replace -----------------------------------------------------------
        "replace" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::Replace {
                glob: None,
                path: Some(args[0].clone()),
                mode: None,
                from: args[1].clone(),
                to: Some(args[2].clone()),
                nth: None,
                insert_before: None,
                insert_after: None,
                case_insensitive: false,
                multiline: false,
                if_exists: false,
                whole_line: false,
                range: None,
                word_boundary: false,
                before_context: None,
                after_context: None,
            })
        }

        // -- file operations ---------------------------------------------------
        "file.append" => {
            require_args(op, args, 2, line_num)?;
            op!(FileAppend {
                path: args[0].clone(),
                content: args[1].clone()
            })
        }
        "file.create" => {
            require_args(op, args, 2, line_num)?;
            op!(FileCreate {
                path: args[0].clone(),
                content: args[1].clone(),
                force: None
            })
        }
        "file.delete" => {
            require_args(op, args, 1, line_num)?;
            op!(FileDelete {
                path: args[0].clone()
            })
        }
        "file.rename" => {
            require_args(op, args, 2, line_num)?;
            op!(FileRename {
                from: args[0].clone(),
                to: args[1].clone(),
                force: false
            })
        }

        // -- markdown operations -----------------------------------------------
        "md.upsert_bullet" => md_phc!(op, args, line_num, MdUpsertBullet, bullet),
        "md.table_append" => md_phc!(op, args, line_num, MdTableAppend, row),
        "md.replace_section" => md_phc!(op, args, line_num, MdReplaceSection, content),
        "md.insert_after_heading" => md_phc!(op, args, line_num, MdInsertAfterHeading, content),
        "md.insert_before_heading" => md_phc!(op, args, line_num, MdInsertBeforeHeading, content),

        "md.move_section" => {
            if args.len() == 4 {
                let (before, after) = parse_position_keyword(&args[2], line_num)?;
                Ok(Operation::MdMoveSection {
                    path: args[0].clone(),
                    heading: args[1].clone(),
                    to: None,
                    before: before.map(|_| args[3].clone()),
                    after: after.map(|_| args[3].clone()),
                })
            } else if args.len() == 5 {
                let (before, after) = parse_position_keyword(&args[3], line_num)?;
                Ok(Operation::MdMoveSection {
                    path: args[0].clone(),
                    heading: args[1].clone(),
                    to: Some(args[2].clone()),
                    before: before.map(|_| args[4].clone()),
                    after: after.map(|_| args[4].clone()),
                })
            } else {
                anyhow::bail!(
                    "line {line_num}: md.move_section requires 4 args (same-file) or 5 args (cross-file)"
                )
            }
        }
        "md.dedupe_headings" => {
            require_args(op, args, 1, line_num)?;
            op!(MdDedupeHeadings {
                path: args[0].clone()
            })
        }
        "md.lint_agents" => {
            require_args(op, args, 1, line_num)?;
            op!(MdLintAgents {
                path: args[0].clone()
            })
        }

        // -- tidy --------------------------------------------------------------
        "tidy.fix" => {
            require_args(op, args, 1, line_num)?;
            op!(TidyFix {
                path: args[0].clone(),
                ensure_final_newline: None,
                trim_trailing_whitespace: None,
                normalize_eol: None,
            })
        }

        // -- AST operations (feature-gated) ------------------------------------
        #[cfg(feature = "ast")]
        "ast.rename" => {
            require_args(op, args, 3, line_num)?;
            op!(AstRename {
                path: args[0].clone(),
                old_name: args[1].clone(),
                new_name: args[2].clone(),
                lang: None
            })
        }
        #[cfg(feature = "ast")]
        "ast.replace" => {
            require_args(op, args, 4, line_num)?;
            op!(AstReplace {
                path: args[0].clone(),
                symbol: args[1].clone(),
                from: args[2].clone(),
                to: args[3].clone(),
                regex: false,
                lang: None
            })
        }

        _ => anyhow::bail!("line {line_num}: unknown operation '{op}'"),
    }
}

/// Check that the exact number of arguments were provided.
/// Parse a position keyword ("before" or "after") and return which one was given.
fn parse_position_keyword(
    keyword: &str,
    line_num: usize,
) -> anyhow::Result<(Option<()>, Option<()>)> {
    match keyword {
        "before" => Ok((Some(()), None)),
        "after" => Ok((None, Some(()))),
        _ => anyhow::bail!("line {line_num}: expected 'before' or 'after', got '{keyword}'"),
    }
}

fn require_args(op: &str, args: &[String], expected: usize, line_num: usize) -> anyhow::Result<()> {
    if args.len() != expected {
        anyhow::bail!(
            "line {line_num}: '{op}' requires exactly {expected} arguments, got {}",
            args.len()
        );
    }
    Ok(())
}

/// Parse a string as a JSON value. Delegates to `doc::parse_value` which
/// handles JSON literals, quoted strings, booleans, null, numbers, and
/// bare-string fallback.
fn parse_json_value(s: &str) -> anyhow::Result<serde_json::Value> {
    Ok(crate::ops::doc::parse_value(s))
}

/// Tokenize a line using shell-like quoting rules.
/// - Whitespace separates tokens
/// - Double-quoted strings preserve spaces and allow escapes (\", \\)
pub fn tokenize(line: &str) -> anyhow::Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current = String::new();
    // Track whether we've entered a token (via quoting or non-whitespace).
    // This ensures empty quoted strings like "" produce an empty-string token
    // instead of being silently dropped.
    let mut in_token = false;

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            chars.next();
        } else if ch == '"' {
            in_token = true;
            chars.next(); // consume opening quote
            loop {
                match chars.next() {
                    Some('\\') => match chars.next() {
                        Some(escaped) => current.push(escaped),
                        None => anyhow::bail!("unexpected end of line after backslash"),
                    },
                    Some('"') => break,
                    Some(c) => current.push(c),
                    None => anyhow::bail!("unterminated double quote"),
                }
            }
        } else {
            in_token = true;
            current.push(ch);
            chars.next();
        }
    }
    if in_token {
        tokens.push(current);
    }
    Ok(tokens)
}

pub fn run(args: BatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    // Read input.
    let input = if args.input == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(&args.input)
            .map_err(|e| anyhow::anyhow!("failed to read '{}': {e}", args.input))?
    };

    // Parse lines into operations.
    let mut operations = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        operations.push(parse_line(trimmed, i + 1)?);
    }

    if operations.is_empty() {
        if global.json || global.jsonl {
            global.emit_json(&serde_json::json!({
                "ok": true,
                "status": "success",
                "files_changed": 0,
                "files_created": 0,
                "files_deleted": 0,
                "changes": []
            }))?;
        } else if !global.quiet {
            eprintln!("batch: no operations found in input");
        }
        return Ok(exit::SUCCESS);
    }

    if operations.len() > MAX_BATCH_OPERATIONS {
        anyhow::bail!(
            "batch: too many operations ({}, max {MAX_BATCH_OPERATIONS})",
            operations.len()
        );
    }

    // Build a plan and delegate to tx.
    let plan_json = {
        let plan = Plan {
            version: crate::plan::SCHEMA_VERSION.to_string(),
            cwd: None,
            write_policy: None,
            strict: None,
            operations,
            format: None,
            validate: None,
        };
        serde_json::to_string(&plan)?
    };

    // Write the plan to a temp file and invoke tx.
    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::write(tmp.path(), &plan_json)?;

    let tx_args = crate::tx::TxArgs {
        plan: tmp
            .path()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("temp file path is not valid UTF-8"))?
            .to_string(),
        plan_format: None,
        no_strict: false,
        write: args.write,
    };
    // Delegate to tx, which handles --apply / --confirm / --format / plan lifecycle.
    let result = crate::cmd::tx::run(tx_args, global)?;
    Ok(result)
}

#[path = "batch_tests.rs"]
#[cfg(test)]
mod tests;
