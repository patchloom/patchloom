use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::{Operation, Plan};
use clap::Args;
use std::io::Read;

/// Execute multiple operations from a simple line-oriented format.
///
/// Each line is one operation with positional arguments:
///
/// ```text
/// doc.set <path> <key> <value>
/// doc.delete <path> <key>
/// doc.merge <path> <json-value>
/// doc.ensure <path> <key> <value>
/// doc.append <path> <key> <value>
/// doc.prepend <path> <key> <value>
/// replace <path> <from> <to>
/// file.create <path> <content>
/// file.delete <path>
/// md.upsert_bullet <path> <heading> <bullet>
/// md.table_append <path> <heading> <row>
/// hygiene.fix <path>
/// ```
///
/// Lines starting with `#` are comments. Empty lines are ignored.
/// Values containing spaces must be quoted with double quotes.
#[derive(Debug, Args)]
pub struct BatchArgs {
    /// Read operations from a file instead of stdin. Use `-` for stdin (default).
    #[arg(long, default_value = "-")]
    pub input: String,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

/// Parse a single line into an Operation.
pub fn parse_line(line: &str, line_num: usize) -> anyhow::Result<Operation> {
    let tokens = tokenize(line)?;
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
                key: args[1].clone(),
                value,
            })
        }
        "doc.delete" => {
            require_args(op, args, 2, line_num)?;
            Ok(Operation::DocDelete {
                path: args[0].clone(),
                key: args[1].clone(),
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
                key: args[1].clone(),
                value,
            })
        }
        "doc.append" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocAppend {
                path: args[0].clone(),
                key: args[1].clone(),
                value,
            })
        }
        "doc.prepend" => {
            require_args(op, args, 3, line_num)?;
            let value = parse_json_value(&args[2])?;
            Ok(Operation::DocPrepend {
                path: args[0].clone(),
                key: args[1].clone(),
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
        "hygiene.fix" => {
            require_args(op, args, 1, line_num)?;
            Ok(Operation::HygieneFix {
                path: args[0].clone(),
                ensure_final_newline: None,
                trim_trailing_whitespace: None,
                normalize_eol: None,
            })
        }
        _ => anyhow::bail!("line {line_num}: unknown operation '{op}'"),
    }
}

/// Check that the right number of arguments were provided.
fn require_args(op: &str, args: &[String], expected: usize, line_num: usize) -> anyhow::Result<()> {
    if args.len() < expected {
        anyhow::bail!(
            "line {line_num}: '{op}' requires {expected} arguments, got {}",
            args.len()
        );
    }
    Ok(())
}

/// Parse a string as a JSON value. If it fails, treat it as a plain string.
fn parse_json_value(s: &str) -> anyhow::Result<serde_json::Value> {
    // Try JSON first (handles objects, arrays, numbers, booleans, null, quoted strings).
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        return Ok(v);
    }
    // Fall back to treating the raw text as a JSON string.
    Ok(serde_json::Value::String(s.to_string()))
}

/// Tokenize a line using shell-like quoting rules.
/// - Whitespace separates tokens
/// - Double-quoted strings preserve spaces and allow escapes (\", \\)
fn tokenize(line: &str) -> anyhow::Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    let mut current = String::new();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            chars.next();
        } else if ch == '"' {
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
            current.push(ch);
            chars.next();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

pub fn run(args: BatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    // Read input.
    let input = if args.input == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
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
        if !global.quiet {
            eprintln!("batch: no operations found in input");
        }
        return Ok(exit::SUCCESS);
    }

    // Build a plan and delegate to tx.
    let plan_json = {
        let plan = Plan {
            cwd: None,
            write_policy: None,
            strict: false,
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
        plan: tmp.path().to_str().unwrap().to_string(),
        plan_format: None,
        write: args.write,
    };
    crate::cmd::tx::run(tx_args, global)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_json_value_quoted_string() {
        let v = parse_json_value(r#""2.0.0""#).unwrap();
        assert_eq!(v, serde_json::json!("2.0.0"));
    }

    #[test]
    fn parse_line_doc_set() {
        let op = parse_line("doc.set config.json version 42", 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, key, value }
            if path == "config.json" && key == "version" && value == serde_json::json!(42)
        ));
    }

    #[test]
    fn parse_line_doc_set_string_value() {
        let op = parse_line(r#"doc.set config.json name "my app""#, 1).unwrap();
        assert!(matches!(
            op,
            Operation::DocSet { path, key, value }
            if path == "config.json" && key == "name" && value == serde_json::json!("my app")
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
    fn parse_line_hygiene_fix() {
        let op = parse_line("hygiene.fix src/lib.rs", 1).unwrap();
        assert!(matches!(op, Operation::HygieneFix { path, .. } if path == "src/lib.rs"));
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
    fn parse_line_unknown_op() {
        let err = parse_line("unknown.op foo bar", 1).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn parse_line_too_few_args() {
        let err = parse_line("doc.set config.json", 1).unwrap_err();
        assert!(err.to_string().contains("requires 3 arguments"));
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
}
