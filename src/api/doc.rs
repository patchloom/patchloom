//! Document operations (JSON, YAML, TOML) for the public library API.
//!
//! Write functions delegate to the tx engine via `execute_as_edit_result`,
//! sharing the same code path as CLI and MCP. Read functions (doc_get,
//! doc_has) load and query directly.
//!
//! Re-exported from api:: via `mod doc; pub use self::doc::*;`.

use std::path::Path;

use anyhow::{Context, bail};

use crate::containment::PathGuard;
use crate::ops;
use crate::ops::doc::query::{QueryResult, query_get, query_has};
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Derive cwd from a file path (its parent directory).
fn cwd_from_path(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

/// Load and parse a JSON/YAML/TOML file for read-only queries.
fn load_doc_value(path: &Path) -> anyhow::Result<serde_json::Value> {
    let path_str = path.to_string_lossy();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    ops::doc::parse_doc(&original, &format)
}

/// Unified write path: delegates to the tx engine when available (cli/files),
/// falls back to direct mutation when the tx module is not compiled in.
#[cfg(any(feature = "cli", feature = "files"))]
fn doc_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    super::execute_as_edit_result(op, mode, cwd_from_path(path), guard, action)
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn doc_write(
    op: Operation,
    path: &Path,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    use crate::ops::doc::{DocMutation, MutationResult};
    use crate::write::WritePolicy;

    // Extract the mutation from the operation.
    let (_, mutation) =
        crate::plan::op_to_doc_mutation(&op).expect("doc_write called with non-doc operation");

    let path_str = path.to_string_lossy().into_owned();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value = ops::doc::parse_doc(&original, &format)?;
    let mut new_value = value.clone();

    let result = ops::doc::apply_doc_mutation(&mut new_value, mutation)?;
    if let MutationResult::TypeError(msg) = result {
        bail!("{msg}");
    }

    let new_content = ops::doc::serialize_value_preserving(&original, &value, &new_value, &format)?;
    let policy = WritePolicy::default();
    let applied = super::write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(super::build_edit_result(
        &path_str,
        original,
        new_content,
        applied,
        action,
        None,
    ))
}

/// Set a value at a selector path in a JSON, YAML, or TOML file.
///
/// The file format is detected from the extension. The selector uses
/// patchloom's selector syntax (e.g., `"database.host"`, `"items[0].name"`).
pub fn doc_set(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocSet {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.set")
}

/// Delete a value at a selector path in a JSON, YAML, or TOML file.
pub fn doc_delete(
    path: &Path,
    selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocDelete {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
    };
    doc_write(op, path, mode, guard, "doc.delete")
}

/// Deep-merge a value into the root of a JSON, YAML, or TOML file.
pub fn doc_merge(
    path: &Path,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocMerge {
        path: path.to_string_lossy().into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.merge")
}

/// Get a value at a selector path from a JSON, YAML, or TOML file.
///
/// This is a read-only operation; the `mode` parameter is ignored.
pub fn doc_get(path: &Path, selector: &str) -> anyhow::Result<serde_json::Value> {
    let value = load_doc_value(path)?;

    match query_get(&value, selector)? {
        QueryResult::NoMatch => bail!("selector '{}' matched nothing", selector),
        QueryResult::Values(vals) if vals.len() == 1 => Ok(vals.into_iter().next().unwrap()),
        QueryResult::Values(vals) => Ok(serde_json::Value::Array(vals)),
    }
}

/// Check whether a selector path exists in a JSON, YAML, or TOML file.
pub fn doc_has(path: &Path, selector: &str) -> anyhow::Result<bool> {
    let value = load_doc_value(path)?;
    query_has(&value, selector)
}

/// Append a value to an array at a selector path.
pub fn doc_append(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocAppend {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.append")
}

/// Prepend a value to an array at a selector path.
pub fn doc_prepend(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocPrepend {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.prepend")
}

/// Update all values matching a selector with a new value.
///
/// Returns an `EditResult`. The number of matches updated is reflected in
/// whether the content changed.
pub fn doc_update(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocUpdate {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.update")
}

/// Ensure a value exists at a selector path; set it only if missing.
pub fn doc_ensure(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocEnsure {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        value,
    };
    doc_write(op, path, mode, guard, "doc.ensure")
}

/// Delete array elements matching a predicate (e.g., `"name=old"`).
pub fn doc_delete_where(
    path: &Path,
    selector: &str,
    predicate: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocDeleteWhere {
        path: path.to_string_lossy().into(),
        selector: selector.into(),
        predicate: predicate.into(),
    };
    doc_write(op, path, mode, guard, "doc.delete_where")
}

/// Move a value from one selector path to another within the same file.
pub fn doc_move(
    path: &Path,
    from_selector: &str,
    to_selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let op = Operation::DocMove {
        path: path.to_string_lossy().into(),
        from: from_selector.into(),
        to: to_selector.into(),
    };
    doc_write(op, path, mode, guard, "doc.move")
}
