use crate::cli::global::GlobalFlags;
use crate::cmd::output::execute_via_engine;
use crate::cmd::write_dispatch::WritePhase;
use crate::exit;
use crate::ops::doc::{detect_format, diff_values, flatten_value, parse_doc, parse_value};
use crate::plan::Operation;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom doc get package.json version
  patchloom doc set config.yaml database.port 5433 --apply
  patchloom doc keys config.toml
  patchloom doc merge config.json '{\"debug\": true}' --apply")]
pub struct DocArgs {
    #[command(subcommand)]
    pub action: DocAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum DocAction {
    /// Read a value at a selector path.
    Get {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
    },
    /// Check whether a selector path exists.
    Has {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
    },
    /// List object keys at a path.
    Keys {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
    },
    /// Count items in an array or object.
    Len {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
    },
    /// Set or create a value at a selector path.
    Set {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
        /// Value (JSON literal or bare string).
        value: String,
    },
    /// Remove a value at a selector path.
    Delete {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
    },
    /// Delete array items matching a predicate.
    DeleteWhere {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
        // ref:doc-mode:predicate
        /// Predicate in key=value format.
        #[arg(long)]
        predicate: String,
    },
    /// Merge a partial object from stdin or argument.
    Merge {
        /// File path (JSON, YAML, or TOML).
        file: String,
        // ref:doc-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        value: Option<String>,
    },
    /// Append to an array.
    Append {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path to an array (e.g. items, dependencies).
        selector: String,
        /// Value to append (JSON literal or bare string).
        value: String,
    },
    /// Prepend to an array.
    Prepend {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path to an array (e.g. items, dependencies).
        selector: String,
        /// Value to prepend (JSON literal or bare string).
        value: String,
    },
    /// Filter array items by predicate.
    Select {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path with a predicate (e.g. items[name=foo]).
        selector: String,
    },
    /// Update all matching nodes.
    Update {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[*].enabled`).
        selector: String,
        /// New value (JSON literal or bare string).
        value: String,
    },
    /// Move or rename a selector path.
    Move {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Source selector path.
        from: String,
        /// Destination selector path.
        to: String,
    },
    /// Ensure a value exists (idempotent set).
    Ensure {
        /// File path (JSON, YAML, or TOML).
        file: String,
        /// Selector path (e.g. `server.port`, `items[0].name`).
        selector: String,
        /// Value to set if missing (JSON literal or bare string).
        value: String,
    },
    /// List all leaf selector paths and their values.
    Flatten {
        /// File path (JSON, YAML, or TOML).
        file: String,
    },
    /// Compare two structured files and show differences.
    Diff {
        /// First file to compare.
        file_a: String,
        /// Second file to compare.
        file_b: String,
    },
}

impl DocAction {
    /// Whether this action mutates a file (vs read-only query).
    fn is_write(&self) -> bool {
        matches!(
            self,
            DocAction::Set { .. }
                | DocAction::Delete { .. }
                | DocAction::DeleteWhere { .. }
                | DocAction::Merge { .. }
                | DocAction::Append { .. }
                | DocAction::Prepend { .. }
                | DocAction::Update { .. }
                | DocAction::Move { .. }
                | DocAction::Ensure { .. }
        )
    }

    /// The primary file path for this action, if any.
    fn file_path(&self) -> Option<&str> {
        match self {
            DocAction::Get { file, .. }
            | DocAction::Has { file, .. }
            | DocAction::Keys { file, .. }
            | DocAction::Len { file, .. }
            | DocAction::Set { file, .. }
            | DocAction::Delete { file, .. }
            | DocAction::DeleteWhere { file, .. }
            | DocAction::Merge { file, .. }
            | DocAction::Append { file, .. }
            | DocAction::Prepend { file, .. }
            | DocAction::Select { file, .. }
            | DocAction::Update { file, .. }
            | DocAction::Move { file, .. }
            | DocAction::Ensure { file, .. }
            | DocAction::Flatten { file } => Some(file.as_str()),
            DocAction::Diff { .. } => None,
        }
    }

    /// Resolve all file paths against `cwd` so the command does not depend
    /// on the process-global current directory.
    fn resolve_files(&mut self, cwd: &std::path::Path) {
        match self {
            DocAction::Get { file, .. }
            | DocAction::Has { file, .. }
            | DocAction::Keys { file, .. }
            | DocAction::Len { file, .. }
            | DocAction::Set { file, .. }
            | DocAction::Delete { file, .. }
            | DocAction::DeleteWhere { file, .. }
            | DocAction::Merge { file, .. }
            | DocAction::Append { file, .. }
            | DocAction::Prepend { file, .. }
            | DocAction::Select { file, .. }
            | DocAction::Update { file, .. }
            | DocAction::Move { file, .. }
            | DocAction::Ensure { file, .. }
            | DocAction::Flatten { file } => {
                *file = cwd.join(&*file).to_string_lossy().into_owned();
            }
            DocAction::Diff { file_a, file_b } => {
                *file_a = cwd.join(&*file_a).to_string_lossy().into_owned();
                *file_b = cwd.join(&*file_b).to_string_lossy().into_owned();
            }
        }
    }
}

fn load_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let format = detect_format(path)?;
    parse_doc(&content, &format).with_context(|| format!("parsing {path}"))
}

use anyhow::Context;

// ---------------------------------------------------------------------------
// Write-mode output & operation builder
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct DocWriteOutput {
    ok: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

/// Convert a write [`DocAction`] into the corresponding [`Operation`] variant.
fn action_to_operation(action: &DocAction) -> anyhow::Result<Operation> {
    match action {
        DocAction::Set {
            file,
            selector,
            value,
        } => Ok(Operation::DocSet {
            path: file.clone(),
            selector: selector.clone(),
            value: parse_value(value),
        }),
        DocAction::Delete { file, selector } => Ok(Operation::DocDelete {
            path: file.clone(),
            selector: selector.clone(),
        }),
        DocAction::DeleteWhere {
            file,
            selector,
            predicate,
        } => Ok(Operation::DocDeleteWhere {
            path: file.clone(),
            selector: selector.clone(),
            predicate: predicate.clone(),
        }),
        DocAction::Merge { file, stdin, value } => {
            let merge_str = if *stdin {
                std::io::read_to_string(std::io::stdin())?
            } else if let Some(v) = value {
                v.clone()
            } else {
                anyhow::bail!("merge requires --stdin or --value");
            };
            Ok(Operation::DocMerge {
                path: file.clone(),
                value: parse_value(&merge_str),
            })
        }
        DocAction::Append {
            file,
            selector,
            value,
        } => Ok(Operation::DocAppend {
            path: file.clone(),
            selector: selector.clone(),
            value: parse_value(value),
        }),
        DocAction::Prepend {
            file,
            selector,
            value,
        } => Ok(Operation::DocPrepend {
            path: file.clone(),
            selector: selector.clone(),
            value: parse_value(value),
        }),
        DocAction::Update {
            file,
            selector,
            value,
        } => Ok(Operation::DocUpdate {
            path: file.clone(),
            selector: selector.clone(),
            value: parse_value(value),
        }),
        DocAction::Move { file, from, to } => Ok(Operation::DocMove {
            path: file.clone(),
            from: from.clone(),
            to: to.clone(),
        }),
        DocAction::Ensure {
            file,
            selector,
            value,
        } => Ok(Operation::DocEnsure {
            path: file.clone(),
            selector: selector.clone(),
            value: parse_value(value),
        }),
        _ => anyhow::bail!("not a write action"),
    }
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Text,
    Json,
    Jsonl,
}

fn format_value(value: &serde_json::Value, mode: OutputMode) -> String {
    match mode {
        OutputMode::Json => serde_json::to_string_pretty(value).unwrap_or_default(),
        OutputMode::Jsonl => serde_json::to_string(value).unwrap_or_default(),
        OutputMode::Text => match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".to_string(),
            // Compound values (arrays, objects) always render as JSON.
            _ => serde_json::to_string_pretty(value).unwrap_or_default(),
        },
    }
}

fn format_values(values: &[&serde_json::Value], mode: OutputMode) -> anyhow::Result<String> {
    match mode {
        OutputMode::Text => Ok(values
            .iter()
            .map(|value| format_value(value, mode))
            .collect::<Vec<_>>()
            .join("\n")),
        OutputMode::Json => {
            if values.len() == 1 {
                Ok(serde_json::to_string_pretty(values[0])?)
            } else {
                Ok(serde_json::to_string_pretty(values)?)
            }
        }
        OutputMode::Jsonl => Ok(values
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")),
    }
}

// ---------------------------------------------------------------------------
// Core execution (returns output text + exit code for testability)
// ---------------------------------------------------------------------------

pub(crate) fn execute_with_mode(
    action: &DocAction,
    output_mode: OutputMode,
) -> anyhow::Result<(String, u8)> {
    match action {
        DocAction::Get { file, selector } | DocAction::Select { file, selector } => {
            let root = load_file(file)?;
            match crate::ops::doc::query::query_get(&root, selector)? {
                crate::ops::doc::query::QueryResult::NoMatch => {
                    Ok((String::new(), exit::NO_MATCHES))
                }
                crate::ops::doc::query::QueryResult::Values(vals) => {
                    let refs: Vec<&serde_json::Value> = vals.iter().collect();
                    Ok((format_values(&refs, output_mode)?, exit::SUCCESS))
                }
            }
        }

        DocAction::Has { file, selector } => {
            let root = load_file(file)?;
            let found = crate::ops::doc::query::query_has(&root, selector)?;
            let output = match output_mode {
                OutputMode::Text => found.to_string(),
                OutputMode::Json => serde_json::to_string_pretty(&found)?,
                OutputMode::Jsonl => serde_json::to_string(&found)?,
            };
            Ok((output, exit::SUCCESS))
        }

        DocAction::Keys { file, selector } => {
            let root = load_file(file)?;
            match crate::ops::doc::query::query_keys(&root, selector)? {
                crate::ops::doc::query::QueryKeysResult::NoMatch => {
                    Ok((String::new(), exit::NO_MATCHES))
                }
                crate::ops::doc::query::QueryKeysResult::NotAnObject => Ok((
                    format!("doc keys: target at '{selector}' is not an object"),
                    exit::FAILURE,
                )),
                crate::ops::doc::query::QueryKeysResult::Keys(keys) => {
                    let output = match output_mode {
                        OutputMode::Text => keys.join("\n"),
                        OutputMode::Json => serde_json::to_string_pretty(&keys)?,
                        OutputMode::Jsonl => keys
                            .iter()
                            .map(serde_json::to_string)
                            .collect::<Result<Vec<_>, _>>()?
                            .join("\n"),
                    };
                    Ok((output, exit::SUCCESS))
                }
            }
        }

        DocAction::Len { file, selector } => {
            let root = load_file(file)?;
            match crate::ops::doc::query::query_len(&root, selector)? {
                crate::ops::doc::query::QueryLenResult::NoMatch => {
                    Ok((String::new(), exit::NO_MATCHES))
                }
                crate::ops::doc::query::QueryLenResult::NotArrayOrObject => Ok((
                    format!("doc len: target at '{selector}' is not an array or object"),
                    exit::FAILURE,
                )),
                crate::ops::doc::query::QueryLenResult::Len(len) => {
                    let output = match output_mode {
                        OutputMode::Text => len.to_string(),
                        OutputMode::Json => serde_json::to_string_pretty(&len)?,
                        OutputMode::Jsonl => serde_json::to_string(&len)?,
                    };
                    Ok((output, exit::SUCCESS))
                }
            }
        }

        DocAction::Flatten { file } => {
            let root = load_file(file)?;
            let mut entries = Vec::new();
            let mut path_buf = String::new();
            flatten_value(&root, &mut path_buf, &mut entries);
            if entries.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            match output_mode {
                OutputMode::Json => {
                    let obj: serde_json::Map<String, serde_json::Value> =
                        entries.into_iter().map(|(k, v)| (k, v.clone())).collect();
                    Ok((serde_json::to_string_pretty(&obj)?, exit::SUCCESS))
                }
                OutputMode::Jsonl => {
                    let lines = entries
                        .iter()
                        .map(|(k, v)| {
                            serde_json::to_string(&serde_json::json!({
                                "path": k,
                                "value": v,
                            }))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n");
                    Ok((lines, exit::SUCCESS))
                }
                OutputMode::Text => {
                    let lines: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{k} = {}", format_value(v, OutputMode::Text)))
                        .collect();
                    Ok((lines.join("\n"), exit::SUCCESS))
                }
            }
        }

        DocAction::Diff { file_a, file_b } => {
            let val_a = load_file(file_a)?;
            let val_b = load_file(file_b)?;
            let mut entries = Vec::new();
            let mut diff_buf = String::new();
            diff_values(&val_a, &val_b, &mut diff_buf, &mut entries);
            if entries.is_empty() {
                return Ok(("identical\n".to_string(), exit::SUCCESS));
            }
            match output_mode {
                OutputMode::Json => Ok((serde_json::to_string_pretty(&entries)?, exit::SUCCESS)),
                OutputMode::Jsonl => Ok((
                    entries
                        .iter()
                        .map(serde_json::to_string)
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n"),
                    exit::SUCCESS,
                )),
                OutputMode::Text => {
                    use std::fmt::Write;
                    let mut out = String::new();
                    for e in &entries {
                        match e.kind {
                            "added" => {
                                if let Some(v) = e.new_value.as_ref() {
                                    let _ = writeln!(
                                        out,
                                        "+ {} = {}",
                                        e.path,
                                        format_value(v, OutputMode::Text)
                                    );
                                }
                            }
                            "removed" => {
                                if let Some(v) = e.old_value.as_ref() {
                                    let _ = writeln!(
                                        out,
                                        "- {} = {}",
                                        e.path,
                                        format_value(v, OutputMode::Text)
                                    );
                                }
                            }
                            "changed" => {
                                if let (Some(old), Some(new)) =
                                    (e.old_value.as_ref(), e.new_value.as_ref())
                                {
                                    let _ = writeln!(
                                        out,
                                        "~ {} = {} -> {}",
                                        e.path,
                                        format_value(old, OutputMode::Text),
                                        format_value(new, OutputMode::Text)
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok((out, exit::SUCCESS))
                }
            }
        }

        // Write-mode subcommands are dispatched through execute_write() via run().
        _ => {
            anyhow::bail!("write operations require the run() entry point")
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(mut args: DocArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("doc: running doc command");

    if args.action.is_write() {
        let display_path = args.action.file_path().unwrap_or("").to_string();
        let op = action_to_operation(&args.action)?;

        let check_msg = format!("would modify {display_path}");
        let apply_msg = format!("updated {display_path}");

        let path_clone = display_path;
        match execute_via_engine(
            op,
            global,
            |phase, diff| DocWriteOutput {
                ok: true,
                path: path_clone.clone(),
                diff,
                applied: match phase {
                    WritePhase::Confirmed(a) => Some(a),
                    _ => None,
                },
            },
            &check_msg,
            &apply_msg,
        ) {
            Ok(code) => return Ok(code),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("matched nothing") {
                    return Ok(exit::NO_MATCHES);
                }
                // TypeError or other engine error → FAILURE
                if global.json || global.jsonl {
                    let err_obj = serde_json::json!({"ok": false, "error": &msg});
                    global.emit_json(&err_obj)?;
                } else if !global.quiet {
                    eprintln!("{msg}");
                }
                return Ok(exit::FAILURE);
            }
        }
    }

    // Read-only operations: resolve file paths for direct filesystem access.
    let cwd = global.resolve_cwd()?;
    args.action.resolve_files(&cwd);

    let output_mode = if global.json {
        OutputMode::Json
    } else if global.jsonl {
        OutputMode::Jsonl
    } else {
        OutputMode::Text
    };

    let (output, code) = execute_with_mode(&args.action, output_mode)?;
    if !output.is_empty() && (global.json || global.jsonl || !global.quiet) {
        println!("{output}");
    }
    Ok(code)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: write a file into a temp directory and return its path.
    fn write_file(dir: &TempDir, name: &str, content: &str) -> String {
        let path = dir.path().join(name);
        fs::write(&path, content).unwrap();
        path.to_str().unwrap().to_string()
    }

    // -- get ----------------------------------------------------------------

    #[test]
    fn get_returns_value_from_json() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "count": 42}"#);
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_yaml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.yaml", "name: hello\ncount: 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_toml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.toml", "name = \"hello\"\ncount = 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    // -- has ----------------------------------------------------------------

    #[test]
    fn has_returns_true_for_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "true");
    }

    #[test]
    fn has_returns_false_for_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "missing".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "false");
    }

    // -- keys ---------------------------------------------------------------

    #[test]
    fn keys_lists_object_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"scripts": {"build": "tsc", "lint": "eslint", "test": "jest"}}"#,
        );
        let action = DocAction::Keys {
            file: path,
            selector: "scripts".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let keys: Vec<&str> = output.split('\n').collect();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"build"));
        assert!(keys.contains(&"lint"));
        assert!(keys.contains(&"test"));
    }

    // -- len ----------------------------------------------------------------

    #[test]
    fn len_counts_array_elements() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3, 4, 5]}"#);
        let action = DocAction::Len {
            file: path,
            selector: "items".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "5");
    }

    // -- missing selector ---------------------------------------------------

    #[test]
    fn get_missing_selector_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Get {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        assert!(output.is_empty());
    }

    // -- parse_value --------------------------------------------------------

    #[test]
    fn parse_value_bare_string() {
        assert_eq!(parse_value("hello"), serde_json::json!("hello"));
    }

    #[test]
    fn parse_value_integer() {
        assert_eq!(parse_value("42"), serde_json::json!(42));
    }

    #[test]
    fn parse_value_bool() {
        assert_eq!(parse_value("true"), serde_json::json!(true));
    }

    #[test]
    fn parse_value_null() {
        assert_eq!(parse_value("null"), serde_json::Value::Null);
    }

    #[test]
    fn parse_value_json_object() {
        let v = parse_value(r#"{"a":1}"#);
        assert_eq!(v, serde_json::json!({"a": 1}));
    }

    // -- Helper to run a doc write action through run() -------------------

    fn run_doc(action: DocAction, global: &GlobalFlags) -> anyhow::Result<u8> {
        run(
            DocArgs {
                action,
                write: Default::default(),
            },
            global,
        )
    }

    // -- set ----------------------------------------------------------------

    #[test]
    fn set_creates_new_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "age".into(),
            value: "42".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["age"], serde_json::json!(42));
    }

    #[test]
    fn set_updates_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("world"));
    }

    // -- delete -------------------------------------------------------------

    #[test]
    fn delete_removes_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "age": 42}"#);
        let action = DocAction::Delete {
            file: path.clone(),
            selector: "age".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(val.get("age").is_none());
    }

    // -- delete-where -------------------------------------------------------

    #[test]
    fn delete_where_removes_matching_items() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"users": [{"name": "alice"}, {"name": "bob"}, {"name": "carol"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path.clone(),
            selector: "users".into(),
            predicate: "name=bob".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let arr = val["users"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], serde_json::json!("alice"));
        assert_eq!(arr[1]["name"], serde_json::json!("carol"));
    }

    #[test]
    fn delete_where_no_match_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"users": [{"name": "alice"}, {"name": "bob"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path,
            selector: "users".into(),
            predicate: "name=nobody".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        // Engine treats delete-where no-match as success (idempotent).
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn delete_where_removes_multiple_matches() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"items": [{"status": "done"}, {"status": "pending"}, {"status": "done"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path.clone(),
            selector: "items".into(),
            predicate: "status=done".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let arr = val["items"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["status"], serde_json::json!("pending"));
    }

    #[test]
    fn delete_missing_key_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Delete {
            file: path,
            selector: "nonexistent".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        // Engine treats delete no-match as success (idempotent).
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    // -- append / prepend ---------------------------------------------------

    #[test]
    fn append_adds_to_array() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3]}"#);
        let action = DocAction::Append {
            file: path.clone(),
            selector: "items".into(),
            value: "4".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["items"], serde_json::json!([1, 2, 3, 4]));
    }

    #[test]
    fn prepend_adds_to_beginning_of_array() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3]}"#);
        let action = DocAction::Prepend {
            file: path.clone(),
            selector: "items".into(),
            value: "0".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["items"], serde_json::json!([0, 1, 2, 3]));
    }

    // -- merge --------------------------------------------------------------

    #[test]
    fn deep_merge_depth_guard_caps_recursion() {
        use crate::ops::doc::deep_merge;
        // Build a JSON tree nested 200 levels deep (exceeds MAX_MERGE_DEPTH of 128).
        // The guard stops recursing at depth 128 and overwrites with the
        // remaining subtree, preventing stack overflow on adversarial input.
        let mut base = serde_json::json!(null);
        let mut other = serde_json::json!({"leaf": true});
        for _ in 0..200 {
            other = serde_json::json!({"nested": other});
        }
        deep_merge(&mut base, &other);
        // Verify the result is a valid object (not a crash) and the
        // top-level structure was preserved.
        assert!(base.is_object());
        assert!(
            base.get("nested").is_some(),
            "top-level 'nested' key must exist"
        );
        // Walk down to verify nesting was preserved (not just top level).
        let mut cursor = &base;
        for _ in 0..10 {
            cursor = cursor
                .get("nested")
                .expect("nesting should be at least 10 levels deep");
        }
        assert!(cursor.is_object());
    }

    #[test]
    fn merge_combines_objects() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Merge {
            file: path.clone(),
            stdin: false,
            value: Some(r#"{"age": 42}"#.into()),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("hello"));
        assert_eq!(val["age"], serde_json::json!(42));
    }

    // -- ensure -------------------------------------------------------------

    #[test]
    fn ensure_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "original"}"#);
        // Ensure on an existing key should not overwrite.
        let action = DocAction::Ensure {
            file: path.clone(),
            selector: "name".into(),
            value: "overwritten".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        // Value should remain "original" (not overwritten).
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("original"));
    }

    #[test]
    fn ensure_creates_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Ensure {
            file: path.clone(),
            selector: "age".into(),
            value: "30".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["age"], serde_json::json!(30));
        assert_eq!(val["name"], serde_json::json!("hello"));
    }

    // -- move ---------------------------------------------------------------

    #[test]
    fn move_renames_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"old_name": "value"}"#);
        let action = DocAction::Move {
            file: path.clone(),
            from: "old_name".into(),
            to: "new_name".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["new_name"], serde_json::json!("value"));
        assert!(val.get("old_name").is_none());
    }

    // -- apply writes file --------------------------------------------------

    #[test]
    fn set_with_apply_writes_file() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("world"));
    }

    // -- check mode ---------------------------------------------------------

    #[test]
    fn set_with_check_reports_changes() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path,
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn diff_detects_added_removed_changed() {
        let dir = TempDir::new().unwrap();
        let a = write_file(
            &dir,
            "a.json",
            r#"{"name":"old","version":1,"removed":true}"#,
        );
        let b = write_file(
            &dir,
            "b.json",
            r#"{"name":"new","version":1,"added":"yes"}"#,
        );
        let action = DocAction::Diff {
            file_a: a,
            file_b: b,
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.contains("~ name"), "should show changed: {output}");
        assert!(
            output.contains("- removed"),
            "should show removed: {output}"
        );
        assert!(output.contains("+ added"), "should show added: {output}");
        assert!(
            !output.contains("version"),
            "unchanged key should not appear: {output}"
        );
    }

    #[test]
    fn diff_identical_files() {
        let dir = TempDir::new().unwrap();
        let a = write_file(&dir, "a.json", r#"{"k":1}"#);
        let b = write_file(&dir, "b.json", r#"{"k":1}"#);
        let action = DocAction::Diff {
            file_a: a,
            file_b: b,
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "identical\n");
    }

    // -- error path tests ---------------------------------------------------

    #[test]
    fn keys_on_scalar_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Keys {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::FAILURE);
        assert!(output.contains("not an object"));
    }

    #[test]
    fn len_on_scalar_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Len {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::FAILURE);
        assert!(output.contains("not an array or object"));
    }

    #[test]
    fn len_counts_object_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a": 1, "b": 2, "c": 3}"#);
        let action = DocAction::Len {
            file: path,
            selector: String::new(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "3");
    }

    #[test]
    fn append_to_non_array_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Append {
            file: path,
            selector: "name".into(),
            value: "42".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn prepend_to_non_array_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Prepend {
            file: path,
            selector: "name".into(),
            value: "42".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    // -- flatten ------------------------------------------------------------

    #[test]
    fn set_apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"version": "1.0"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "version".into(),
            value: "\"2.0\"".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(
            backup_dir.exists(),
            "backup directory should exist after doc set --apply"
        );
        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(
            !sessions.is_empty(),
            "at least one backup session should be created"
        );
    }

    #[test]
    fn flatten_enumerates_leaf_paths() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"a":1,"b":{"c":2,"d":3},"e":[10,20]}"#,
        );
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.contains("a = 1"), "missing a: {output}");
        assert!(output.contains("b.c = 2"), "missing b.c: {output}");
        assert!(output.contains("b.d = 3"), "missing b.d: {output}");
        assert!(output.contains("e[0] = 10"), "missing e[0]: {output}");
        assert!(output.contains("e[1] = 20"), "missing e[1]: {output}");
    }

    #[test]
    fn flatten_includes_empty_arrays() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"tags":[],"name":"foo","items":[1]}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["tags"], serde_json::json!([]));
        assert_eq!(parsed["name"], serde_json::json!("foo"));
        assert_eq!(parsed["items[0]"], serde_json::json!(1));
    }

    #[test]
    fn flatten_includes_empty_objects() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"config":{},"name":"bar"}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["config"], serde_json::json!({}));
        assert_eq!(parsed["name"], serde_json::json!("bar"));
    }

    #[test]
    fn flatten_includes_nested_empty_containers() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a":{"b":[],"c":{}},"d":[1]}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["a.b"], serde_json::json!([]));
        assert_eq!(parsed["a.c"], serde_json::json!({}));
        assert_eq!(parsed["d[0]"], serde_json::json!(1));
    }
}
