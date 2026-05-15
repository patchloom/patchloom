use crate::cli::global::GlobalFlags;
use crate::diff;
use crate::exit;
use crate::selector;
use crate::write;
use crate::write::policy_from_flags;
use clap::Args;
use std::path::Path;

#[derive(Debug, Args)]
pub struct DocArgs {
    #[command(subcommand)]
    pub action: DocAction,
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
        /// Predicate in key=value format.
        #[arg(long)]
        predicate: String,
    },
    /// Merge a partial object from stdin or argument.
    Merge {
        file: String,
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
}

// ---------------------------------------------------------------------------
// File format detection & loading
// ---------------------------------------------------------------------------

enum FileFormat {
    Json,
    Yaml,
    Toml,
}

fn detect_format(path: &str) -> anyhow::Result<FileFormat> {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        Some("toml") => Ok(FileFormat::Toml),
        Some(ext) => anyhow::bail!("unsupported file extension: .{ext}"),
        None => anyhow::bail!("file has no extension"),
    }
}

fn load_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let content = std::fs::read_to_string(path)?;
    let format = detect_format(path)?;
    match format {
        FileFormat::Json => Ok(serde_json::from_str(&content)?),
        FileFormat::Yaml => Ok(serde_yaml_ng::from_str(&content)?),
        FileFormat::Toml => {
            // Parse with DocumentMut, then deserialize to serde_json::Value.
            let _doc: toml_edit::DocumentMut = content
                .parse()
                .map_err(|e: toml_edit::TomlError| anyhow::anyhow!("{e}"))?;
            let value: serde_json::Value = toml_edit::de::from_str(&content)?;
            Ok(value)
        }
    }
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn format_value(value: &serde_json::Value, json_mode: bool) -> String {
    if json_mode {
        serde_json::to_string_pretty(value).unwrap_or_default()
    } else {
        match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".to_string(),
            // Compound values (arrays, objects) always render as JSON.
            _ => serde_json::to_string_pretty(value).unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Value parsing & serialization helpers
// ---------------------------------------------------------------------------

/// Parse a CLI value string into a [`serde_json::Value`].
///
/// Recognition order: JSON-quoted string, JSON object/array, boolean, null,
/// i64, f64, then fallback to bare string.
fn parse_value(s: &str) -> serde_json::Value {
    // JSON-quoted string
    if s.starts_with('"') && s.ends_with('"') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            return v;
        }
    }
    // JSON object or array
    if s.starts_with('{') || s.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            return v;
        }
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
    if let Ok(n) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return serde_json::Value::Number(num);
        }
    }
    // Bare string
    serde_json::Value::String(s.to_string())
}

/// Serialize a [`serde_json::Value`] back to the original file format.
fn serialize_value(value: &serde_json::Value, format: &FileFormat) -> anyhow::Result<String> {
    match format {
        FileFormat::Json => {
            let mut s = serde_json::to_string_pretty(value)?;
            s.push('\n');
            Ok(s)
        }
        FileFormat::Yaml => Ok(serde_yaml_ng::to_string(value)?),
        FileFormat::Toml => {
            let s = toml_edit::ser::to_string_pretty(value)
                .map_err(|e| anyhow::anyhow!("TOML serialization error: {e}"))?;
            Ok(s)
        }
    }
}

/// Load a file, returning original content, parsed value, and detected format.
fn load_file_with_content(path: &str) -> anyhow::Result<(String, serde_json::Value, FileFormat)> {
    let content = std::fs::read_to_string(path)?;
    let format = detect_format(path)?;
    let value = match &format {
        FileFormat::Json => serde_json::from_str(&content)?,
        FileFormat::Yaml => serde_yaml_ng::from_str(&content)?,
        FileFormat::Toml => toml_edit::de::from_str(&content)?,
    };
    Ok((content, value, format))
}

// ---------------------------------------------------------------------------
// Mutable tree navigation
// ---------------------------------------------------------------------------

/// Navigate through a mutable JSON value tree following `segments`.
///
/// When `create` is true, missing intermediate object keys are created as
/// empty objects.  Returns an error if navigation is impossible (e.g.
/// traversing through a scalar or an out-of-bounds index).
fn navigate_mut<'a>(
    root: &'a mut serde_json::Value,
    segments: &[selector::Segment],
    create: bool,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let mut current = root;
    for seg in segments {
        current = match seg {
            selector::Segment::Key(k) => {
                if create {
                    let needs_create = match current.as_object() {
                        Some(obj) => !obj.contains_key(k.as_str()),
                        None => false,
                    };
                    if needs_create {
                        current
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("not an object at key '{k}'"))?
                            .insert(k.clone(), serde_json::Value::Object(serde_json::Map::new()));
                    }
                }
                current
                    .get_mut(k.as_str())
                    .ok_or_else(|| anyhow::anyhow!("key not found: {k}"))?
            }
            selector::Segment::Index(i) => current
                .get_mut(*i)
                .ok_or_else(|| anyhow::anyhow!("index out of bounds: {i}"))?,
            _ => anyhow::bail!("wildcard/predicate not supported in write navigation"),
        };
    }
    Ok(current)
}

/// Deep-merge `other` into `base`.  Object keys are merged recursively;
/// all other types are overwritten.
fn deep_merge(base: &mut serde_json::Value, other: &serde_json::Value) {
    if base.is_object() && other.is_object() {
        let other_map = other.as_object().unwrap();
        let base_map = base.as_object_mut().unwrap();
        for (key, value) in other_map {
            let entry = base_map
                .entry(key.clone())
                .or_insert(serde_json::Value::Null);
            deep_merge(entry, value);
        }
    } else {
        *base = other.clone();
    }
}

/// Recursively update all values matched by `segments`, replacing each with
/// `new_val`.  Returns the number of values updated.
fn update_matching(
    value: &mut serde_json::Value,
    segments: &[selector::Segment],
    new_val: &serde_json::Value,
) -> usize {
    if segments.is_empty() {
        *value = new_val.clone();
        return 1;
    }
    let first = &segments[0];
    let rest = &segments[1..];
    match first {
        selector::Segment::Key(k) => {
            if let Some(child) = value.get_mut(k.as_str()) {
                update_matching(child, rest, new_val)
            } else {
                0
            }
        }
        selector::Segment::Index(i) => {
            if let Some(child) = value.get_mut(*i) {
                update_matching(child, rest, new_val)
            } else {
                0
            }
        }
        selector::Segment::Wildcard => {
            let mut count = 0;
            if let Some(arr) = value.as_array_mut() {
                for item in arr.iter_mut() {
                    count += update_matching(item, rest, new_val);
                }
            }
            count
        }
        selector::Segment::Predicate {
            key,
            value: pred_val,
        } => {
            let mut count = 0;
            if let Some(arr) = value.as_array_mut() {
                for item in arr.iter_mut() {
                    let matches = {
                        item.get(key.as_str()).is_some_and(|field| match field {
                            serde_json::Value::String(s) => s == pred_val,
                            serde_json::Value::Number(n) => n.to_string() == *pred_val,
                            serde_json::Value::Bool(b) => b.to_string() == *pred_val,
                            _ => false,
                        })
                    };
                    if matches {
                        count += update_matching(item, rest, new_val);
                    }
                }
            }
            count
        }
    }
}

// ---------------------------------------------------------------------------
// Write context & result handling
// ---------------------------------------------------------------------------

/// Flags controlling write behaviour, extracted from [`GlobalFlags`].
#[derive(Default)]
struct WriteContext {
    check: bool,
    apply: bool,
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
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let last = sel
                .last()
                .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
            let parent_path = &sel[..sel.len() - 1];
            let parent = navigate_mut(&mut root, parent_path, true)?;

            match last {
                selector::Segment::Key(k) => {
                    parent
                        .as_object_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                        .insert(k.clone(), parsed);
                }
                selector::Segment::Index(i) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                    if *i < arr.len() {
                        arr[*i] = parsed;
                    } else {
                        anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                    }
                }
                _ => anyhow::bail!("cannot set at wildcard/predicate"),
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Delete { file, selector } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            if sel.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let last = sel.last().unwrap();
            let parent_path = &sel[..sel.len() - 1];

            let parent = match navigate_mut(&mut root, parent_path, false) {
                Ok(p) => p,
                Err(_) => return Ok((String::new(), exit::NO_MATCHES)),
            };

            match last {
                selector::Segment::Key(k) => {
                    if let Some(obj) = parent.as_object_mut() {
                        if obj.remove(k.as_str()).is_none() {
                            return Ok((String::new(), exit::NO_MATCHES));
                        }
                    } else {
                        return Ok((String::new(), exit::NO_MATCHES));
                    }
                }
                selector::Segment::Index(i) => {
                    if let Some(arr) = parent.as_array_mut() {
                        if *i < arr.len() {
                            arr.remove(*i);
                        } else {
                            return Ok((String::new(), exit::NO_MATCHES));
                        }
                    } else {
                        return Ok((String::new(), exit::NO_MATCHES));
                    }
                }
                _ => anyhow::bail!("cannot delete at wildcard/predicate"),
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::DeleteWhere {
            file,
            selector,
            predicate,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            let eq_pos = predicate
                .find('=')
                .ok_or_else(|| anyhow::anyhow!("predicate must be in key=value format"))?;
            let pred_key = &predicate[..eq_pos];
            let pred_val = &predicate[eq_pos + 1..];

            let target = navigate_mut(&mut root, &sel, false)?;
            let arr = target
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;

            let before_len = arr.len();
            arr.retain(|item| {
                if let Some(field) = item.get(pred_key) {
                    let matches = match field {
                        serde_json::Value::String(s) => s == pred_val,
                        serde_json::Value::Number(n) => n.to_string() == pred_val,
                        serde_json::Value::Bool(b) => b.to_string() == pred_val,
                        _ => false,
                    };
                    !matches
                } else {
                    true
                }
            });

            if arr.len() == before_len {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Merge { file, stdin, value } => {
            let (original, mut root, format) = load_file_with_content(file)?;

            let merge_str = if *stdin {
                std::io::read_to_string(std::io::stdin())?
            } else if let Some(v) = value {
                v.clone()
            } else {
                anyhow::bail!("merge requires --stdin or --value");
            };

            let merge_val = parse_value(&merge_str);
            deep_merge(&mut root, &merge_val);

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Append {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let target = navigate_mut(&mut root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => arr.push(parsed),
                None => return Ok((String::new(), exit::PARSE_ERROR)),
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Prepend {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let target = navigate_mut(&mut root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => arr.insert(0, parsed),
                None => return Ok((String::new(), exit::PARSE_ERROR)),
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Update {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let parsed = parse_value(value);

            let count = update_matching(&mut root, &sel, &parsed);
            if count == 0 {
                return Ok((String::new(), exit::NO_MATCHES));
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Move { file, from, to } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let from_sel =
                selector::parse(from).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let to_sel = selector::parse(to).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            // Remove value at source path.
            let removed = {
                let last = from_sel
                    .last()
                    .ok_or_else(|| anyhow::anyhow!("empty from selector"))?;
                let parent_path = &from_sel[..from_sel.len() - 1];
                let parent = navigate_mut(&mut root, parent_path, false)?;
                match last {
                    selector::Segment::Key(k) => parent
                        .as_object_mut()
                        .and_then(|obj| obj.remove(k.as_str()))
                        .ok_or_else(|| anyhow::anyhow!("source key '{k}' not found"))?,
                    selector::Segment::Index(i) => {
                        let arr = parent
                            .as_array_mut()
                            .ok_or_else(|| anyhow::anyhow!("source parent is not an array"))?;
                        if *i < arr.len() {
                            arr.remove(*i)
                        } else {
                            anyhow::bail!("source index {i} out of bounds");
                        }
                    }
                    _ => anyhow::bail!("cannot move from wildcard/predicate"),
                }
            };

            // Insert at destination path.
            {
                let last = to_sel
                    .last()
                    .ok_or_else(|| anyhow::anyhow!("empty to selector"))?;
                let parent_path = &to_sel[..to_sel.len() - 1];
                let parent = navigate_mut(&mut root, parent_path, true)?;
                match last {
                    selector::Segment::Key(k) => {
                        parent
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("target parent is not an object"))?
                            .insert(k.clone(), removed);
                    }
                    selector::Segment::Index(i) => {
                        let arr = parent
                            .as_array_mut()
                            .ok_or_else(|| anyhow::anyhow!("target parent is not an array"))?;
                        if *i <= arr.len() {
                            arr.insert(*i, removed);
                        } else {
                            anyhow::bail!("target index {i} out of bounds");
                        }
                    }
                    _ => anyhow::bail!("cannot move to wildcard/predicate"),
                }
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        DocAction::Ensure {
            file,
            selector,
            value,
        } => {
            let (original, mut root, format) = load_file_with_content(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;

            // If the path already exists, return immediately – no mutation.
            if !selector::eval(&root, &sel).is_empty() {
                return Ok((String::new(), exit::SUCCESS));
            }

            // Path does not exist – create it (same logic as set).
            let parsed = parse_value(value);
            let last = sel
                .last()
                .ok_or_else(|| anyhow::anyhow!("empty selector"))?;
            let parent_path = &sel[..sel.len() - 1];
            let parent = navigate_mut(&mut root, parent_path, true)?;

            match last {
                selector::Segment::Key(k) => {
                    parent
                        .as_object_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                        .insert(k.clone(), parsed);
                }
                selector::Segment::Index(i) => {
                    let arr = parent
                        .as_array_mut()
                        .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
                    if *i < arr.len() {
                        arr[*i] = parsed;
                    } else {
                        anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
                    }
                }
                _ => anyhow::bail!("cannot ensure at wildcard/predicate"),
            }

            let new_content = serialize_value(&root, &format)?;
            write_result(file, &original, &new_content, ctx)
        }

        // Read-only actions are handled by execute().
        _ => anyhow::bail!("not a write action"),
    }
}

// ---------------------------------------------------------------------------
// Core execution (returns output text + exit code for testability)
// ---------------------------------------------------------------------------

fn execute(action: &DocAction, json_mode: bool) -> anyhow::Result<(String, u8)> {
    match action {
        DocAction::Get { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            if results.is_empty() {
                return Ok((String::new(), exit::NO_MATCHES));
            }
            let lines: Vec<String> = results.iter().map(|v| format_value(v, json_mode)).collect();
            Ok((lines.join("\n"), exit::SUCCESS))
        }

        DocAction::Has { file, selector } => {
            let root = load_file(file)?;
            let sel =
                selector::parse(selector).map_err(|e| anyhow::anyhow!("selector error: {e}"))?;
            let results = selector::eval(&root, &sel);
            let found = !results.is_empty();
            Ok((found.to_string(), exit::SUCCESS))
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
                    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    Ok((keys.join("\n"), exit::SUCCESS))
                }
                None => Ok((String::new(), exit::PARSE_ERROR)),
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
                Ok((arr.len().to_string(), exit::SUCCESS))
            } else if let Some(obj) = target.as_object() {
                Ok((obj.len().to_string(), exit::SUCCESS))
            } else {
                Ok((String::new(), exit::PARSE_ERROR))
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
            let lines: Vec<String> = results.iter().map(|v| format_value(v, json_mode)).collect();
            Ok((lines.join("\n"), exit::SUCCESS))
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

pub fn run(args: DocArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
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
            write_policy: policy_from_flags(global, doc_file_path.map(std::path::Path::new)),
        };
        let (output, code) = execute_write(&args.action, &ctx)?;
        if !output.is_empty() {
            println!("{output}");
        }
        return Ok(code);
    }

    let (output, code) = execute(&args.action, global.json)?;
    if !output.is_empty() {
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
        let (output, code) = execute(&action, false).unwrap();
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
}
