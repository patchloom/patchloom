use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::cmd::output::execute_via_engine;
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
        // All single-file variants share a `file` field; only `Diff` uses two files.
        macro_rules! single_file {
            ($($Variant:ident),+ $(,)?) => {
                match self {
                    $(DocAction::$Variant { file, .. } => Some(file.as_str()),)+
                    DocAction::Diff { .. } => None,
                }
            };
        }
        single_file!(
            Get,
            Has,
            Keys,
            Len,
            Set,
            Delete,
            DeleteWhere,
            Merge,
            Append,
            Prepend,
            Select,
            Update,
            Move,
            Ensure,
            Flatten
        )
    }

    /// Resolve all file paths against `cwd` so the command does not depend
    /// on the process-global current directory.
    fn resolve_files(&mut self, cwd: &std::path::Path) {
        macro_rules! resolve_single {
            ($($Variant:ident),+ $(,)?) => {
                match self {
                    $(DocAction::$Variant { file, .. } => {
                        *file = cwd.join(&*file).to_string_lossy().into_owned();
                    })+
                    DocAction::Diff { file_a, file_b } => {
                        *file_a = cwd.join(&*file_a).to_string_lossy().into_owned();
                        *file_b = cwd.join(&*file_b).to_string_lossy().into_owned();
                    }
                }
            };
        }
        resolve_single!(
            Get,
            Has,
            Keys,
            Len,
            Set,
            Delete,
            DeleteWhere,
            Merge,
            Append,
            Prepend,
            Select,
            Update,
            Move,
            Ensure,
            Flatten
        );
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
        } => {
            crate::verbose!(
                "doc: set file={}, selector={:?}, value={:?}",
                file,
                selector,
                value
            );
            Ok(Operation::DocSet {
                path: file.clone(),
                selector: selector.clone(),
                value: parse_value(value),
            })
        }
        DocAction::Delete { file, selector } => {
            crate::verbose!("doc: delete file={}, selector={:?}", file, selector);
            Ok(Operation::DocDelete {
                path: file.clone(),
                selector: selector.clone(),
            })
        }
        DocAction::DeleteWhere {
            file,
            selector,
            predicate,
        } => {
            crate::verbose!(
                "doc: delete-where file={}, selector={:?}, predicate={:?}",
                file,
                selector,
                predicate
            );
            Ok(Operation::DocDeleteWhere {
                path: file.clone(),
                selector: selector.clone(),
                predicate: predicate.clone(),
            })
        }
        DocAction::Merge { file, stdin, value } => {
            crate::verbose!("doc: merge file={}, stdin={}", file, stdin);
            if *stdin && value.is_some() {
                anyhow::bail!("merge: --stdin and --value are mutually exclusive");
            }
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
        } => {
            crate::verbose!(
                "doc: append file={}, selector={:?}, value={:?}",
                file,
                selector,
                value
            );
            Ok(Operation::DocAppend {
                path: file.clone(),
                selector: selector.clone(),
                value: parse_value(value),
            })
        }
        DocAction::Prepend {
            file,
            selector,
            value,
        } => {
            crate::verbose!(
                "doc: prepend file={}, selector={:?}, value={:?}",
                file,
                selector,
                value
            );
            Ok(Operation::DocPrepend {
                path: file.clone(),
                selector: selector.clone(),
                value: parse_value(value),
            })
        }
        DocAction::Update {
            file,
            selector,
            value,
        } => {
            crate::verbose!(
                "doc: update file={}, selector={:?}, value={:?}",
                file,
                selector,
                value
            );
            Ok(Operation::DocUpdate {
                path: file.clone(),
                selector: selector.clone(),
                value: parse_value(value),
            })
        }
        DocAction::Move { file, from, to } => {
            crate::verbose!("doc: move file={}, from={:?}, to={:?}", file, from, to);
            Ok(Operation::DocMove {
                path: file.clone(),
                from: from.clone(),
                to: to.clone(),
            })
        }
        DocAction::Ensure {
            file,
            selector,
            value,
        } => {
            crate::verbose!(
                "doc: ensure file={}, selector={:?}, value={:?}",
                file,
                selector,
                value
            );
            Ok(Operation::DocEnsure {
                path: file.clone(),
                selector: selector.clone(),
                value: parse_value(value),
            })
        }
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
            crate::verbose!("doc: get/select file={}, selector={:?}", file, selector);
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
            crate::verbose!("doc: has file={}, selector={:?}", file, selector);
            let root = load_file(file)?;
            let found = crate::ops::doc::query::query_has(&root, selector)?;
            let output = match output_mode {
                OutputMode::Text => found.to_string(),
                OutputMode::Json => serde_json::to_string_pretty(&found)?,
                OutputMode::Jsonl => serde_json::to_string(&found)?,
            };
            Ok((
                output,
                if found {
                    exit::SUCCESS
                } else {
                    exit::NO_MATCHES
                },
            ))
        }

        DocAction::Keys { file, selector } => {
            crate::verbose!("doc: keys file={}, selector={:?}", file, selector);
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
            crate::verbose!("doc: len file={}, selector={:?}", file, selector);
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
            crate::verbose!("doc: flatten file={}", file);
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
            crate::verbose!("doc: diff file_a={}, file_b={}", file_a, file_b);
            let val_a = load_file(file_a)?;
            let val_b = load_file(file_b)?;
            let mut entries = Vec::new();
            let mut diff_buf = String::new();
            diff_values(&val_a, &val_b, &mut diff_buf, &mut entries);
            if entries.is_empty() {
                let out = match output_mode {
                    OutputMode::Json => serde_json::to_string_pretty(&serde_json::json!({
                        "identical": true, "differences": []
                    }))?,
                    OutputMode::Jsonl => serde_json::to_string(&serde_json::json!({
                        "identical": true, "differences": []
                    }))?,
                    OutputMode::Text => "identical".to_string(),
                };
                return Ok((out, exit::SUCCESS));
            }
            match output_mode {
                OutputMode::Json => Ok((
                    serde_json::to_string_pretty(&entries)?,
                    exit::CHANGES_DETECTED,
                )),
                OutputMode::Jsonl => Ok((
                    entries
                        .iter()
                        .map(serde_json::to_string)
                        .collect::<Result<Vec<_>, _>>()?
                        .join("\n"),
                    exit::CHANGES_DETECTED,
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
                    Ok((out, exit::CHANGES_DETECTED))
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
    crate::verbose!("doc: action={:?}", std::mem::discriminant(&args.action));

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

#[path = "doc_tests.rs"]
#[cfg(test)]
mod tests;
