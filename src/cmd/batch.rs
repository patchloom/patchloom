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
fn parse_line(line: &str, line_num: usize) -> anyhow::Result<Operation> {
    let tokens = tokenize(line).map_err(|e| anyhow::anyhow!("line {line_num}: {e}"))?;
    if tokens.is_empty() {
        anyhow::bail!("line {line_num}: empty operation");
    }
    let op = tokens[0].as_str();
    let args = &tokens[1..];

    match op {
        "doc.set" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocSet {
                path: args[0].clone(),
                selector: args[1].clone(),
                value,
            })
        }
        "doc.delete" => {
            require_args(op, args, 2, line_num)?;
            Ok(Operation::DocDelete {
                path: args[0].clone(),
                selector: args[1].clone(),
            })
        }
        "doc.merge" => {
            require_args(op, args, 2, line_num)?;
            let value = parse_json_value(&args[1])?;
            Ok(Operation::DocMerge {
                path: args[0].clone(),
                value,
            })
        }
        "doc.ensure" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocEnsure {
                path: args[0].clone(),
                selector: args[1].clone(),
                value,
            })
        }
        "doc.append" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocAppend {
                path: args[0].clone(),
                selector: args[1].clone(),
                value,
            })
        }
        "doc.prepend" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocPrepend {
                path: args[0].clone(),
                selector: args[1].clone(),
                value,
            })
        }
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
        "file.append" => {
            require_args(op, args, 2, line_num)?;
            Ok(Operation::FileAppend {
                path: args[0].clone(),
                content: args[1].clone(),
            })
        }
        "file.create" => {
            require_args(op, args, 2, line_num)?;
            Ok(Operation::FileCreate {
                path: args[0].clone(),
                content: args[1].clone(),
                force: None,
            })
        }
        "file.delete" => {
            require_args(op, args, 1, line_num)?;
            Ok(Operation::FileDelete {
                path: args[0].clone(),
            })
        }
        "file.rename" => {
            require_args(op, args, 2, line_num)?;
            Ok(Operation::FileRename {
                from: args[0].clone(),
                to: args[1].clone(),
                force: false,
            })
        }
        "md.upsert_bullet" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::MdUpsertBullet {
                path: args[0].clone(),
                heading: args[1].clone(),
                bullet: args[2].clone(),
            })
        }
        "md.table_append" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::MdTableAppend {
                path: args[0].clone(),
                heading: args[1].clone(),
                row: args[2].clone(),
            })
        }
        "doc.update" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocUpdate {
                path: args[0].clone(),
                selector: args[1].clone(),
                value,
            })
        }
        "doc.move" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::DocMove {
                path: args[0].clone(),
                from: args[1].clone(),
                to: args[2].clone(),
            })
        }
        "doc.delete_where" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::DocDeleteWhere {
                path: args[0].clone(),
                selector: args[1].clone(),
                predicate: args[2].clone(),
            })
        }
        "md.replace_section" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::MdReplaceSection {
                path: args[0].clone(),
                heading: args[1].clone(),
                content: args[2].clone(),
            })
        }
        "md.insert_after_heading" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::MdInsertAfterHeading {
                path: args[0].clone(),
                heading: args[1].clone(),
                content: args[2].clone(),
            })
        }
        "md.insert_before_heading" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::MdInsertBeforeHeading {
                path: args[0].clone(),
                heading: args[1].clone(),
                content: args[2].clone(),
            })
        }
        "md.move_section" => {
            // 4 args: md.move_section <path> <heading> before|after <target_heading>
            // 5 args: md.move_section <path> <heading> <to> before|after <target_heading>
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
            Ok(Operation::MdDedupeHeadings {
                path: args[0].clone(),
            })
        }
        "md.lint_agents" => {
            require_args(op, args, 1, line_num)?;
            Ok(Operation::MdLintAgents {
                path: args[0].clone(),
            })
        }
        "tidy.fix" => {
            require_args(op, args, 1, line_num)?;
            Ok(Operation::TidyFix {
                path: args[0].clone(),
                ensure_final_newline: None,
                trim_trailing_whitespace: None,
                normalize_eol: None,
            })
        }
        #[cfg(feature = "ast")]
        "ast.rename" => {
            require_args(op, args, 3, line_num)?;
            Ok(Operation::AstRename {
                path: args[0].clone(),
                old_name: args[1].clone(),
                new_name: args[2].clone(),
                lang: None,
            })
        }
        #[cfg(feature = "ast")]
        "ast.replace" => {
            require_args(op, args, 4, line_num)?;
            Ok(Operation::AstReplace {
                path: args[0].clone(),
                symbol: args[1].clone(),
                from: args[2].clone(),
                to: args[3].clone(),
                regex: false,
                lang: None,
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
    Ok(crate::cmd::doc::parse_value(s))
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

    let tx_args = crate::cmd::tx::TxArgs {
        plan: tmp
            .path()
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("temp file path is not valid UTF-8"))?
            .to_string(),
        plan_format: None,
        no_strict: false,
        write: args.write,
    };
    let result = crate::cmd::tx::run(tx_args, global)?;
    if result == crate::exit::SUCCESS && global.apply {
        let cwd = global.resolve_cwd()?;
        crate::write::run_format_command(global, &cwd)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_batch_operations_limit_is_enforced() {
        let dir = tempfile::TempDir::new().unwrap();
        // Build input with MAX+1 lines.
        let lines: String = (0..=MAX_BATCH_OPERATIONS)
            .map(|i| format!("doc.set f.json key{i} \"v\""))
            .collect::<Vec<_>>()
            .join("\n");
        let input_file = dir.path().join("ops.txt");
        std::fs::write(&input_file, &lines).unwrap();

        let args = BatchArgs {
            input: input_file.to_str().unwrap().to_string(),
            write: Default::default(),
        };
        let global = GlobalFlags {
            cwd: Some(dir.path().to_str().unwrap().to_string()),
            ..GlobalFlags::default()
        };
        let err = run(args, &global).unwrap_err();
        assert!(
            err.to_string().contains("too many operations"),
            "expected limit error: {err}"
        );
    }

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("doc.set config.json version 42").unwrap();
        assert_eq!(tokens, vec!["doc.set", "config.json", "version", "42"]);
    }

    #[test]
    fn tokenize_quoted() {
        let tokens = tokenize(r#"doc.set config.json key "hello world""#).unwrap();
        assert_eq!(tokens, vec!["doc.set", "config.json", "key", "hello world"]);
    }

    #[test]
    fn tokenize_escaped_quote() {
        let tokens = tokenize(r#"replace f.txt "say \"hi\"" "say \"bye\"""#).unwrap();
        assert_eq!(
            tokens,
            vec!["replace", "f.txt", r#"say "hi""#, r#"say "bye""#]
        );
    }

    #[test]
    fn tokenize_json_value_unquoted() {
        // Unquoted JSON without internal quotes works fine.
        let tokens = tokenize("doc.set f.json key 42").unwrap();
        assert_eq!(tokens, vec!["doc.set", "f.json", "key", "42"]);
    }

    #[test]
    fn tokenize_empty_quoted_string() {
        let tokens = tokenize(r#"doc.set f.json key """#).unwrap();
        assert_eq!(tokens, vec!["doc.set", "f.json", "key", ""]);
    }

    #[test]
    fn tokenize_empty_quoted_string_mid_line() {
        let tokens = tokenize(r#"replace f.txt "" "new""#).unwrap();
        assert_eq!(tokens, vec!["replace", "f.txt", "", "new"]);
    }

    #[test]
    fn tokenize_json_value_quoted() {
        // JSON objects with internal quotes must be double-quoted.
        let tokens = tokenize(r#"doc.merge f.json "{\"nested\":\"value\",\"num\":42}""#).unwrap();
        assert_eq!(
            tokens,
            vec!["doc.merge", "f.json", r#"{"nested":"value","num":42}"#]
        );
    }

    #[test]
    fn parse_json_value_number() {
        let v = parse_json_value("42").unwrap();
        assert_eq!(v, serde_json::json!(42));
    }

    #[test]
    fn parse_json_value_string_fallback() {
        let v = parse_json_value("hello").unwrap();
        assert_eq!(v, serde_json::json!("hello"));
    }

    #[test]
    fn parse_json_value_object() {
        let v = parse_json_value(r#"{"a":1}"#).unwrap();
        assert_eq!(v, serde_json::json!({"a": 1}));
    }

    #[test]
    fn tokenize_trailing_backslash_error() {
        let err = tokenize(r#"doc.set f.json key "trail\"#).unwrap_err();
        assert!(
            err.to_string()
                .contains("unexpected end of line after backslash"),
            "expected backslash error, got: {err}"
        );
    }

    #[test]
    fn tokenize_unterminated_quote_error() {
        let err = tokenize(r#"doc.set f.json key "no close"#).unwrap_err();
        assert!(
            err.to_string().contains("unterminated double quote"),
            "expected unterminated quote error, got: {err}"
        );
    }

    #[test]
    fn parse_json_value_quoted_string() {
        let v = parse_json_value(r#""2.0.0""#).unwrap();
        assert_eq!(v, serde_json::json!("2.0.0"));
    }

    #[test]
    fn parse_line_doc_set() {
        let op = parse_line("doc.set config.json version 42", 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, selector, value }
            if path == "config.json" && selector == "version" && value == serde_json::json!(42)
        ));
    }

    #[test]
    fn parse_line_doc_set_string_value() {
        let op = parse_line(r#"doc.set config.json name "my app""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, selector, value }
            if path == "config.json" && selector == "name" && value == serde_json::json!("my app")
        ));
    }

    #[test]
    fn parse_line_replace() {
        let op = parse_line(r#"replace src/main.rs "old text" "new text""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::Replace { path: Some(p), from, to: Some(t), .. }
            if p == "src/main.rs" && from == "old text" && t == "new text"
        ));
    }

    #[test]
    fn parse_line_file_create() {
        let op = parse_line(r#"file.create hello.txt "Hello, World!""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::FileCreate { path, content, .. }
            if path == "hello.txt" && content == "Hello, World!"
        ));
    }

    #[test]
    fn parse_line_file_delete() {
        let op = parse_line("file.delete old.txt", 1).unwrap();
        assert!(matches!(op, Operation::FileDelete { path } if path == "old.txt"));
    }

    #[test]
    fn parse_line_tidy_fix() {
        let op = parse_line("tidy.fix src/lib.rs", 1).unwrap();
        assert!(matches!(
            op,
            Operation::TidyFix { path, ensure_final_newline, trim_trailing_whitespace, normalize_eol }
            if path == "src/lib.rs"
                && ensure_final_newline.is_none()
                && trim_trailing_whitespace.is_none()
                && normalize_eol.is_none()
        ));
    }

    #[test]
    fn parse_line_md_upsert_bullet() {
        let input = "md.upsert_bullet AGENTS.md \"## Rules\" \"- New rule\"";
        let op = parse_line(input, 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdUpsertBullet { path, heading, bullet }
            if path == "AGENTS.md" && heading == "## Rules" && bullet == "- New rule"
        ));
    }

    #[test]
    fn parse_line_doc_update() {
        // JSON objects with internal quotes must be escaped inside double quotes
        // in batch format (see tokenize_json_value_quoted test).
        let op = parse_line(r#"doc.update config.json items[*] "{\"active\":true}""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocUpdate { path, selector, value }
            if path == "config.json" && selector == "items[*]" && value == serde_json::json!({"active": true})
        ));
    }

    #[test]
    fn parse_line_doc_move() {
        let op = parse_line(r#"doc.move config.json old_key new_key"#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocMove { path, from, to }
            if path == "config.json" && from == "old_key" && to == "new_key"
        ));
    }

    #[test]
    fn parse_line_doc_delete_where() {
        let op = parse_line(r#"doc.delete_where config.json items "status=obsolete""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocDeleteWhere { path, selector, predicate }
            if path == "config.json" && selector == "items" && predicate == "status=obsolete"
        ));
    }

    #[test]
    fn parse_line_md_replace_section() {
        let op = parse_line("md.replace_section README.md \"## API\" \"New content\"", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdReplaceSection { path, heading, content }
            if path == "README.md" && heading == "## API" && content == "New content"
        ));
    }

    #[test]
    fn parse_line_md_insert_after_heading() {
        let op = parse_line(
            "md.insert_after_heading README.md \"## Rules\" \"New paragraph\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdInsertAfterHeading { path, heading, content }
            if path == "README.md" && heading == "## Rules" && content == "New paragraph"
        ));
    }

    #[test]
    fn parse_line_md_insert_before_heading() {
        let op = parse_line(
            "md.insert_before_heading README.md \"## Rules\" \"Preamble\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdInsertBeforeHeading { path, heading, content }
            if path == "README.md" && heading == "## Rules" && content == "Preamble"
        ));
    }

    #[test]
    fn parse_line_md_dedupe_headings() {
        let op = parse_line("md.dedupe_headings CHANGELOG.md", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdDedupeHeadings { path }
            if path == "CHANGELOG.md"
        ));
    }

    #[test]
    fn parse_line_md_lint_agents() {
        let op = parse_line("md.lint_agents AGENTS.md", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdLintAgents { path }
            if path == "AGENTS.md"
        ));
    }

    #[test]
    fn parse_line_doc_delete() {
        let op = parse_line("doc.delete config.json old_key", 1).unwrap();
        assert!(matches!(op, Operation::DocDelete { ref path, ref selector }
            if path == "config.json" && selector == "old_key"));
    }

    #[test]
    fn parse_line_doc_merge() {
        let op = parse_line(r#"doc.merge config.json "{\"debug\":true}""#, 1).unwrap();
        assert!(matches!(op, Operation::DocMerge { ref path, ref value }
                if path == "config.json" && value == &serde_json::json!({"debug": true})));
    }

    #[test]
    fn parse_line_doc_ensure() {
        let op = parse_line(r#"doc.ensure config.json version "beta""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocEnsure { ref path, ref selector, ref value }
            if path == "config.json" && selector == "version" && value == &serde_json::json!("beta"))
        );
    }

    #[test]
    fn parse_line_doc_append() {
        let op = parse_line(r#"doc.append config.json tags "new""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocAppend { ref path, ref selector, ref value }
            if path == "config.json" && selector == "tags" && value == &serde_json::json!("new"))
        );
    }

    #[test]
    fn parse_line_doc_prepend() {
        let op = parse_line(r#"doc.prepend config.json items "first""#, 1).unwrap();
        assert!(
            matches!(op, Operation::DocPrepend { ref path, ref selector, ref value }
            if path == "config.json" && selector == "items" && value == &serde_json::json!("first"))
        );
    }

    #[test]
    fn parse_line_md_table_append() {
        let input = "md.table_append README.md \"## Commands\" \"| new | desc |\"";
        let op = parse_line(input, 1).unwrap();
        assert!(
            matches!(op, Operation::MdTableAppend { ref path, ref heading, ref row }
            if path == "README.md" && heading == "## Commands" && row == "| new | desc |")
        );
    }

    #[test]
    fn parse_line_file_rename() {
        let op = parse_line("file.rename old.txt new.txt", 1).unwrap();
        assert!(
            matches!(op, Operation::FileRename { ref from, ref to, force }
            if from == "old.txt" && to == "new.txt" && !force)
        );
    }

    #[test]
    fn parse_line_unknown_op() {
        let err = parse_line("unknown.op foo bar", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    // Batch intentionally does not support read, search, and patch.apply.
    // These are tx-only operations. The tests below document this as deliberate.

    #[test]
    fn parse_line_rejects_read() {
        let err = parse_line("read path.txt", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_rejects_search() {
        let err = parse_line("search path.txt hello", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_rejects_patch_apply() {
        let err = parse_line("patch.apply diff-text", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_too_few_args() {
        let err = parse_line("doc.set config.json", 1).unwrap_err();
        assert!(err.to_string().contains("requires exactly 3 arguments"));
    }

    #[test]
    fn parse_line_extra_args_rejected() {
        let err = parse_line(r#"file.delete old.txt extra"#, 1).unwrap_err();
        assert!(err.to_string().contains("requires exactly 1 arguments"));
    }

    #[test]
    fn parse_line_extra_args_rejected_all_operations() {
        // 2-arg operations (require exactly 2)
        let two_arg_ops = [
            r#"doc.delete f.json sel extra"#,
            r#"doc.merge f.json "{}" extra"#,
            r#"file.create f.txt content extra"#,
            r#"file.rename old.txt new.txt extra"#,
        ];
        for line in &two_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 2 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }

        // 3-arg operations (require exactly 3)
        let three_arg_ops = [
            r#"doc.set f.json sel "v" extra"#,
            r#"doc.ensure f.json sel "v" extra"#,
            r#"doc.append f.json sel "v" extra"#,
            r#"doc.prepend f.json sel "v" extra"#,
            r#"doc.update f.json sel "v" extra"#,
            r#"doc.move f.json from to extra"#,
            r#"doc.delete_where f.json sel "k=v" extra"#,
            r#"replace f.txt old new extra"#,
            r##"md.upsert_bullet f.md "# H" "- b" extra"##,
            r##"md.table_append f.md "# H" "| r |" extra"##,
            r##"md.replace_section f.md "# H" body extra"##,
            r##"md.insert_after_heading f.md "# H" text extra"##,
            r##"md.insert_before_heading f.md "# H" text extra"##,
        ];
        for line in &three_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 3 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }

        // 1-arg operations (require exactly 1)
        let one_arg_ops = [
            "md.dedupe_headings f.md extra",
            "md.lint_agents f.md extra",
            "tidy.fix f.txt extra",
        ];
        for line in &one_arg_ops {
            let err = parse_line(line, 1).unwrap_err();
            assert!(
                err.to_string().contains("requires exactly 1 arguments"),
                "expected rejection for '{line}', got: {err}"
            );
        }
    }

    #[test]
    fn tokenize_error_includes_line_number() {
        // Unterminated quote should include the line number from parse_line.
        let err = parse_line(r#"doc.set f.json key "unterminated"#, 7).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("line 7"),
            "expected line number in error: {msg}"
        );
        assert!(
            msg.contains("unterminated double quote"),
            "expected tokenize message in error: {msg}"
        );
    }

    #[test]
    fn full_batch_parse() {
        let input = r#"
# Update versions across the project
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"

# Create a new file
file.create hello.txt "Hello, World!"
"#;
        let mut operations = Vec::new();
        for (i, line) in input.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            operations.push(parse_line(trimmed, i + 1).unwrap());
        }
        assert_eq!(operations.len(), 4);
    }

    #[test]
    fn parse_line_md_move_section_same_file() {
        let op = parse_line("md.move_section README.md \"FAQ\" before \"License\"", 1).unwrap();
        assert!(matches!(
            op,
            Operation::MdMoveSection { path, heading, to, before, after }
            if path == "README.md" && heading == "FAQ" && to.is_none()
               && before.as_deref() == Some("License") && after.is_none()
        ));
    }

    #[test]
    fn parse_line_md_move_section_cross_file() {
        let op = parse_line(
            "md.move_section spec.md \"Appendix\" dest.md after \"Layer 4\"",
            1,
        )
        .unwrap();
        assert!(matches!(
            op,
            Operation::MdMoveSection { path, heading, to, before, after }
            if path == "spec.md" && heading == "Appendix"
               && to.as_deref() == Some("dest.md")
               && before.is_none() && after.as_deref() == Some("Layer 4")
        ));
    }

    #[test]
    fn parse_line_md_move_section_bad_keyword() {
        let err =
            parse_line("md.move_section README.md \"FAQ\" between \"License\"", 1).unwrap_err();
        assert!(err.to_string().contains("expected 'before' or 'after'"));
    }

    #[test]
    fn parse_line_md_move_section_wrong_arg_count() {
        let err = parse_line("md.move_section README.md", 1).unwrap_err();
        assert!(err.to_string().contains("requires 4 args"));
    }
}
