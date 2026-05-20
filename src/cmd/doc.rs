use crate::cli::global::GlobalFlags;
use crate::diff;
use crate::exit;
use crate::ops::doc::{
    FileFormat, deep_merge, delete_at_selector, delete_where, detect_format, move_at_path,
    navigate_mut, parse_doc, serialize_value_preserving, set_at_path, update_matching,
};
use crate::selector;
use crate::write;
use crate::write::policy_from_flags;
use anyhow::Context;
use clap::Args;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Args)]
pub struct DocArgs {
    #[command(subcommand)]
    pub action: DocAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum DocAction {
    /// Read a value at a key path.
    Get { file: String, selector: String },
    /// Check whether a key path exists.
    Has { file: String, selector: String },
    /// List object keys at a path.
    Keys { file: String, selector: String },
    /// Count items in an array or object.
    Len { file: String, selector: String },
    /// Set or create a value at a key path.
    Set {
        file: String,
        selector: String,
        value: String,
    },
    /// Remove a key path.
    Delete { file: String, selector: String },
    /// Delete array items matching a predicate.
    DeleteWhere {
        file: String,
        selector: String,
        // ref:doc-mode:predicate
        /// Predicate in key=value format.
        #[arg(long)]
        predicate: String,
    },
    /// Merge a partial object from stdin or argument.
    Merge {
        file: String,
        // ref:doc-mode:stdin
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        value: Option<String>,
    },
    /// Append to an array.
    Append {
        file: String,
        selector: String,
        value: String,
    },
    /// Prepend to an array.
    Prepend {
        file: String,
        selector: String,
        value: String,
    },
    /// Filter array items by predicate.
    Select { file: String, selector: String },
    /// Update all matching nodes.
    Update {
        file: String,
        selector: String,
        value: String,
    },
    /// Move or rename a key path.
    Move {
        file: String,
        from: String,
        to: String,
    },
    /// Ensure a value exists (idempotent set).
    Ensure {
        file: String,
        selector: String,
        value: String,
    },
    /// List all leaf key paths and their values.
    Flatten { file: String },
    /// Compare two structured files and show differences.
    Diff { file_a: String, file_b: String },
}

impl DocAction {
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

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

/// Recursively enumerate all leaf key paths in a JSON value.
fn flatten_value<'a>(
    value: &'a serde_json::Value,
    prefix: String,
    out: &mut Vec<(String, &'a serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_value(v, path, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let path = format!("{prefix}[{i}]");
                flatten_value(v, path, out);
            }
        }
        _ => {
            out.push((prefix, value));
        }
    }
}

/// Entry in a structured diff.
#[derive(Debug, Clone, Serialize)]
struct DiffEntry {
    path: String,
    kind: String, // "added", "removed", "changed"
    #[serde(skip_serializing_if = "Option::is_none")]
    old_value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_value: Option<serde_json::Value>,
}

/// Compute structural differences between two JSON values.
fn diff_values(
    a: &serde_json::Value,
    b: &serde_json::Value,
    prefix: &str,
    out: &mut Vec<DiffEntry>,
) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (k, va) in ma {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                if let Some(vb) = mb.get(k) {
                    diff_values(va, vb, &path, out);
                } else {
                    out.push(DiffEntry {
                        path,
                        kind: "removed".to_string(),
                        old_value: Some(va.clone()),
                        new_value: None,
                    });
                }
            }
            for (k, vb) in mb {
                if !ma.contains_key(k) {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    out.push(DiffEntry {
                        path,
                        kind: "added".to_string(),
                        old_value: None,
                        new_value: Some(vb.clone()),
                    });
                }
            }
        }
        (serde_json::Value::Array(aa), serde_json::Value::Array(ab)) => {
            let max_len = aa.len().max(ab.len());
            for i in 0..max_len {
                let path = format!("{prefix}[{i}]");
                match (aa.get(i), ab.get(i)) {
                    (Some(va), Some(vb)) => diff_values(va, vb, &path, out),
                    (Some(va), None) => out.push(DiffEntry {
                        path,
                        kind: "removed".to_string(),
                        old_value: Some(va.clone()),
                        new_value: None,
                    }),
                    (None, Some(vb)) => out.push(DiffEntry {
                        path,
                        kind: "added".to_string(),
                        old_value: None,
                        new_value: Some(vb.clone()),
                    }),
                    (None, None) => {}
                }
            }
        }
        _ => {
            if a != b {
                out.push(DiffEntry {
                    path: prefix.to_string(),
                    kind: "changed".to_string(),
                    old_value: Some(a.clone()),
                    new_value: Some(b.clone()),
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
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
// Value parsing & serialization helpers
// ---------------------------------------------------------------------------

/// Parse a CLI value string into a [`serde_json::Value`].
///
/// Recognition order: JSON-quoted string, JSON object/array, boolean, null,
/// i64, f64, then fallback to bare string.
pub(crate) fn parse_value(s: &str) -> serde_json::Value {
    // JSON-quoted string
    if s.starts_with('"')
        && s.ends_with('"')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
    {
        return v;
    }
    // JSON object or array
    if (s.starts_with('{') || s.starts_with('['))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(s)
    {
        return v;
    }
    // Booleans
    if s == "true" {
        return serde_json::Value::Bool(true);
    }
    if s == "false" {
        return serde_json::Value::Bool(false);
    }
    // Null
    if s == "null" {
        return serde_json::Value::Null;
    }
    // Integer
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    // Float
    if let Ok(n) = s.parse::<f64>()
        && let Some(num) = serde_json::Number::from_f64(n)
    {
        return serde_json::Value::Number(num);
    }
    // Bare string
    serde_json::Value::String(s.to_string())
}

/// Serialize a [`serde_json::Value`] back to the original file format.
/// Load a file, returning original content, parsed value, and detected format.
/// Clone `old_value` only for TOML/YAML (needed for comment-preserving
/// serialization). JSON serialization ignores `old_value` (#224).
fn clone_for_preserve(root: &serde_json::Value, format: &FileFormat) -> serde_json::Value {
    match format {
        FileFormat::Json => serde_json::Value::Null,
        _ => root.clone(),
    }
}

fn load_file_with_content(path: &str) -> anyhow::Result<(String, serde_json::Value, FileFormat)> {
    let content = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let format = detect_format(path)?;
    let value = parse_doc(&content, &format).with_context(|| format!("parsing {path}"))?;
    Ok((content, value, format))
}

// ---------------------------------------------------------------------------
// Write context & result handling
// ---------------------------------------------------------------------------

/// Flags controlling write behaviour, extracted from [`GlobalFlags`].
#[derive(Default)]
struct WriteContext {
    check: bool,
    apply: bool,
    quiet: bool,
    write_policy: write::WritePolicy,
}

/// Diff / check / apply logic shared by all write subcommands.
fn write_result(
    path: &str,
    original: &str,
    new_content: &str,
    ctx: &WriteContext,
) -> anyhow::Result<(String, u8)> {
    if ctx.check {
        if original != new_content {
            return Ok((String::new(), exit::CHANGES_DETECTED));
        }
        return Ok((String::new(), exit::SUCCESS));
    }
    if ctx.apply {
        write::atomic_write(Path::new(path), new_content, &ctx.write_policy)?;
        return Ok((String::new(), exit::SUCCESS));
    }
    // Default or explicit --diff: show unified diff.
    let file_diff = diff::unified_diff(path, original, new_content);
    if file_diff.has_changes {
        let diff_result = diff::DiffResult {
            diffs: vec![file_diff],
            total_files_changed: 1,
        };
        let output = diff::format_diff_result(&diff_result);
        Ok((output, exit::SUCCESS))
    } else {
        Ok((String::new(), exit::SUCCESS))
    }
}

// ---------------------------------------------------------------------------
// Write-mode execution
// ---------------------------------------------------------------------------

fn execute_write(action: &DocAction, ctx: &WriteContext) -> anyhow::Result<(String, u8)> {
    match action {
        DocAction::Set {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            set_at_path(&mut root, &sel, parsed)?;

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Delete { file, selector } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            if !delete_at_selector(&mut root, &sel)? {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::DeleteWhere {
            file,
            selector,
            predicate,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            let removed = delete_where(&mut root, &sel, predicate)?;

            if removed == 0 {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Merge { file, stdin, value } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);

            let merge_str = if *stdin {
                std::io::read_to_string(std::io::stdin())?
            } else if let Some(v) = value {
                v.clone()
            } else {
                anyhow::bail!("merge requires --stdin or --value");
            };

            let merge_val = parse_value(&merge_str);
            deep_merge(&mut root, &merge_val);

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Append {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let target = navigate_mut(&mut root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => arr.push(parsed),
                None => {
                    if !ctx.quiet {
                        eprintln!("doc append: target at '{selector}' is not an array");
                    }
                    return Ok((String::new(), exit::FAILURE));
                }
            }

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Prepend {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let target = navigate_mut(&mut root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => arr.insert(0, parsed),
                None => {
                    if !ctx.quiet {
                        eprintln!("doc prepend: target at '{selector}' is not an array");
                    }
                    return Ok((String::new(), exit::FAILURE));
                }
            }

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Update {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let count = update_matching(&mut root, &sel, &parsed);
            if count == 0 {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Move { file, from, to } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let from_sel =
                selector::parse(from).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let to_sel = selector::parse(to).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            move_at_path(&mut root, &from_sel, &to_sel)?;

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Ensure {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let old_value = clone_for_preserve(&root, &format);
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            // If the path already exists, return immediately – no mutation.
            if !selector::eval(&root, &sel).is_empty() {
                return Ok((String::new(), exit::SUCCESS));
            }

            // Path does not exist – create it (same logic as set).
            let parsed = parse_value(value);
            set_at_path(&mut root, &sel, parsed)?;

            let new_content = serialize_value_preserving(&original, &old_value, &root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        // Read-only actions are handled by execute_with_mode().
        _ => anyhow::bail!("not a write action"),
    }
}

// ---------------------------------------------------------------------------
// Core execution (returns output text + exit code for testability)
// ---------------------------------------------------------------------------

fn execute_with_mode(action: &DocAction, output_mode: OutputMode) -> anyhow::Result<(String, u8)> {
    match action {
        DocAction::Get { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            Ok((format_values(&results, output_mode)?, exit::SUCCESS))
        }

        DocAction::Has { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            let found = !results.is_empty();
            let output = match output_mode {
                OutputMode::Text => found.to_string(),
                OutputMode::Json => serde_json::to_string_pretty(&found)?,
                OutputMode::Jsonl => serde_json::to_string(&found)?,
            };
            Ok((output, exit::SUCCESS))
        }

        DocAction::Keys { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            match results[0].as_object() {
                Some(obj) => {
                    let keys: Vec<&str> = obj.keys().map(std::string::String::as_str).collect();
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
                None => Ok((
                    format!("doc keys: target at '{selector}' is not an object"),
                    exit::FAILURE,
                )),
            }
        }

        DocAction::Len { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            let target = results[0];
            if let Some(arr) = target.as_array() {
                let len = arr.len();
                let output = match output_mode {
                    OutputMode::Text => len.to_string(),
                    OutputMode::Json => serde_json::to_string_pretty(&len)?,
                    OutputMode::Jsonl => serde_json::to_string(&len)?,
                };
                Ok((output, exit::SUCCESS))
            } else if let Some(obj) = target.as_object() {
                let len = obj.len();
                let output = match output_mode {
                    OutputMode::Text => len.to_string(),
                    OutputMode::Json => serde_json::to_string_pretty(&len)?,
                    OutputMode::Jsonl => serde_json::to_string(&len)?,
                };
                Ok((output, exit::SUCCESS))
            } else {
                Ok((
                    format!("doc len: target at '{selector}' is not an array or object"),
                    exit::FAILURE,
                ))
            }
        }

        // Select is read-only: filter and return matching items.
        DocAction::Select { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            Ok((format_values(&results, output_mode)?, exit::SUCCESS))
        }

        DocAction::Flatten { file } => {
            let root = load_file(file)?;
            let mut entries = Vec::new();
            flatten_value(&root, String::new(), &mut entries);
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
            diff_values(&val_a, &val_b, "", &mut entries);
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
                    let mut out = String::new();
                    for e in &entries {
                        match e.kind.as_str() {
                            "added" => {
                                if let Some(v) = e.new_value.as_ref() {
                                    out.push_str(&format!(
                                        "+ {} = {}\n",
                                        e.path,
                                        format_value(v, OutputMode::Text)
                                    ));
                                }
                            }
                            "removed" => {
                                if let Some(v) = e.old_value.as_ref() {
                                    out.push_str(&format!(
                                        "- {} = {}\n",
                                        e.path,
                                        format_value(v, OutputMode::Text)
                                    ));
                                }
                            }
                            "changed" => {
                                if let (Some(old), Some(new)) =
                                    (e.old_value.as_ref(), e.new_value.as_ref())
                                {
                                    out.push_str(&format!(
                                        "~ {} = {} -> {}\n",
                                        e.path,
                                        format_value(old, OutputMode::Text),
                                        format_value(new, OutputMode::Text)
                                    ));
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
        DocAction::Set { .. }
        | DocAction::Delete { .. }
        | DocAction::DeleteWhere { .. }
        | DocAction::Merge { .. }
        | DocAction::Append { .. }
        | DocAction::Prepend { .. }
        | DocAction::Update { .. }
        | DocAction::Move { .. }
        | DocAction::Ensure { .. } => {
            anyhow::bail!("write operations require the run() entry point")
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(mut args: DocArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    args.action.resolve_files(&cwd);

    let is_write = matches!(
        &args.action,
        DocAction::Set { .. }
            | DocAction::Delete { .. }
            | DocAction::DeleteWhere { .. }
            | DocAction::Merge { .. }
            | DocAction::Append { .. }
            | DocAction::Prepend { .. }
            | DocAction::Update { .. }
            | DocAction::Move { .. }
            | DocAction::Ensure { .. }
    );

    if is_write {
        // Extract the file path from the action for EditorConfig lookup.
        let doc_file_path: Option<&str> = match &args.action {
            DocAction::Set { file, .. }
            | DocAction::Delete { file, .. }
            | DocAction::DeleteWhere { file, .. }
            | DocAction::Merge { file, .. }
            | DocAction::Append { file, .. }
            | DocAction::Prepend { file, .. }
            | DocAction::Update { file, .. }
            | DocAction::Ensure { file, .. } => Some(file.as_str()),
            DocAction::Move { file, .. } => Some(file.as_str()),
            _ => None,
        };
        let ctx = WriteContext {
            check: global.check,
            apply: global.apply,
            quiet: global.quiet,
            write_policy: policy_from_flags(global, doc_file_path.map(std::path::Path::new)),
        };
        let (output, code) = execute_write(&args.action, &ctx)?;
        if !output.is_empty() {
            println!("{output}");
        }
        return Ok(code);
    }

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

    // -- set ----------------------------------------------------------------

    #[test]
    fn set_creates_new_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path,
            selector: "age".into(),
            value: "42".into(),
        };
        let ctx = WriteContext::default();
        let (output, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        // Default: shows diff containing the new key.
        assert!(output.contains("+"));
        assert!(output.contains("age"));
    }

    #[test]
    fn set_updates_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path,
            selector: "name".into(),
            value: "world".into(),
        };
        let ctx = WriteContext::default();
        let (output, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.contains("world"));
    }

    // -- delete -------------------------------------------------------------

    #[test]
    fn delete_removes_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "age": 42}"#);
        let action = DocAction::Delete {
            file: path,
            selector: "age".into(),
        };
        let ctx = WriteContext::default();
        let (output, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.contains("-"));
        assert!(output.contains("age"));
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = val["users"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], serde_json::json!("alice"));
        assert_eq!(arr[1]["name"], serde_json::json!("carol"));
    }

    #[test]
    fn delete_where_no_match_returns_exit_3() {
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
        let ctx = WriteContext::default();
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = val["items"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["status"], serde_json::json!("pending"));
    }

    #[test]
    fn delete_missing_key_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Delete {
            file: path,
            selector: "nonexistent".into(),
        };
        let ctx = WriteContext::default();
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(val["items"], serde_json::json!([0, 1, 2, 3]));
    }

    // -- merge --------------------------------------------------------------

    #[test]
    fn deep_merge_depth_guard_caps_recursion() {
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (output, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.is_empty());
        // File should be unchanged.
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, r#"{"name": "original"}"#);
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
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
        let ctx = WriteContext {
            apply: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(&path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();
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
        let ctx = WriteContext {
            check: true,
            ..WriteContext::default()
        };
        let (_, code) = execute_write(&action, &ctx).unwrap();
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
        let ctx = WriteContext::default();
        let (_, code) = execute_write(&action, &ctx).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    // -- flatten ------------------------------------------------------------

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
}
