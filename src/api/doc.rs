//! Document operations (JSON, YAML, TOML) for the public library API.
//!
//! Contains LoadedDoc, loading, finishing edits, and all doc_* functions.
//! Re-exported from api:: via `mod doc; pub use self::doc::*;`.

use std::path::Path;

use anyhow::{Context, anyhow, bail};

use crate::containment::PathGuard;
use crate::ops;
use crate::selector;
use crate::write::WritePolicy;

use super::{ApplyMode, EditResult, build_edit_result, write_if_apply};

/// A parsed document loaded from disk, ready for mutation.
struct LoadedDoc {
    path_str: String,
    format: ops::doc::FileFormat,
    original: String,
    value: serde_json::Value,
}

/// Load and parse a JSON/YAML/TOML file.
fn load_doc(path: &Path) -> anyhow::Result<LoadedDoc> {
    let path_str = path.to_string_lossy().into_owned();
    let format = ops::doc::detect_format(&path_str)?;
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value = ops::doc::parse_doc(&original, &format)?;
    Ok(LoadedDoc {
        path_str,
        format,
        original,
        value,
    })
}

/// Serialize a mutated document and build an `EditResult`.
fn finish_doc_edit(
    path: &Path,
    doc: &LoadedDoc,
    new_value: &serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
    action: &'static str,
) -> anyhow::Result<EditResult> {
    let new_content =
        ops::doc::serialize_value_preserving(&doc.original, &doc.value, new_value, &doc.format)?;
    let policy = WritePolicy::default();
    let applied = write_if_apply(path, &new_content, mode, &policy, guard)?;
    Ok(build_edit_result(
        &doc.path_str,
        doc.original.clone(),
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
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::set_at_path(&mut new_value, &segments, value)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.set")
}

/// Delete a value at a selector path in a JSON, YAML, or TOML file.
pub fn doc_delete(
    path: &Path,
    selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::delete_at_selector(&mut new_value, &segments)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.delete")
}

/// Deep-merge a value into the root of a JSON, YAML, or TOML file.
pub fn doc_merge(
    path: &Path,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    ops::doc::deep_merge(&mut new_value, &value);

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.merge")
}

/// Get a value at a selector path from a JSON, YAML, or TOML file.
///
/// This is a read-only operation; the `mode` parameter is ignored.
pub fn doc_get(path: &Path, selector: &str) -> anyhow::Result<serde_json::Value> {
    let doc = load_doc(path)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc.value, &segments);
    match result.len() {
        0 => bail!("selector '{}' matched nothing", selector),
        1 => Ok(result
            .into_iter()
            .next()
            .expect("len==1 guarantees element")
            .clone()),
        _ => Ok(serde_json::Value::Array(
            result.into_iter().cloned().collect(),
        )),
    }
}

/// Check whether a selector path exists in a JSON, YAML, or TOML file.
pub fn doc_has(path: &Path, selector: &str) -> anyhow::Result<bool> {
    let doc = load_doc(path)?;

    let segments = selector::parse_anyhow(selector)?;
    let result = selector::eval(&doc.value, &segments);
    Ok(!result.is_empty())
}

/// Append a value to an array at a selector path.
pub fn doc_append(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    let target = ops::doc::navigate_mut(&mut new_value, &segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow!("selector does not point to an array"))?;
    arr.push(value);

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.append")
}

/// Prepend a value to an array at a selector path.
pub fn doc_prepend(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    let target = ops::doc::navigate_mut(&mut new_value, &segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow!("selector does not point to an array"))?;
    arr.insert(0, value);

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.prepend")
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
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::update_matching(&mut new_value, &segments, &value);

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.update")
}

/// Ensure a value exists at a selector path; set it only if missing.
pub fn doc_ensure(
    path: &Path,
    selector: &str,
    value: serde_json::Value,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    // Only set if the path does not already exist.
    let existing = selector::eval(&new_value, &segments);
    if existing.is_empty() {
        ops::doc::set_at_path(&mut new_value, &segments, value)?;
    }

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.ensure")
}

/// Delete array elements matching a predicate (e.g., `"name=old"`).
pub fn doc_delete_where(
    path: &Path,
    selector: &str,
    predicate: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let segments = selector::parse_anyhow(selector)?;
    ops::doc::delete_where(&mut new_value, &segments, predicate)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.delete_where")
}

/// Move a value from one selector path to another within the same file.
pub fn doc_move(
    path: &Path,
    from_selector: &str,
    to_selector: &str,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let doc = load_doc(path)?;
    let mut new_value = doc.value.clone();

    let from_segments = selector::parse_anyhow(from_selector)?;
    let to_segments = selector::parse_anyhow(to_selector)?;
    ops::doc::move_at_path(&mut new_value, &from_segments, &to_segments)?;

    finish_doc_edit(path, &doc, &new_value, mode, guard, "doc.move")
}
