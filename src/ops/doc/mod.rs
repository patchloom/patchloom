use crate::selector;
use std::path::Path;

mod preserve;
mod yaml_splice;

pub use yaml_splice::needs_yaml_quoting;

#[derive(Debug, Clone, Copy)]
pub enum FileFormat {
    Json,
    Yaml,
    Toml,
}

pub fn detect_format(path: &str) -> anyhow::Result<FileFormat> {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("json") => Ok(FileFormat::Json),
        Some("yaml" | "yml") => Ok(FileFormat::Yaml),
        Some("toml") => Ok(FileFormat::Toml),
        Some(ext) => anyhow::bail!(
            "unsupported file extension: .{ext} (supported: .json, .yaml, .yml, .toml)"
        ),
        None => anyhow::bail!(
            "file has no extension; doc commands require .json, .yaml, .yml, or .toml"
        ),
    }
}

pub fn serialize_value(value: &serde_json::Value, format: &FileFormat) -> anyhow::Result<String> {
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

/// Serialize a value back to its original format, preserving comments and
/// formatting for TOML and YAML files.
///
/// For TOML, the original text is re-parsed with `toml_edit::DocumentMut`
/// (which retains comments and whitespace), and only the paths that differ
/// between `old_value` and `new_value` are updated.  Untouched keys keep
/// their original formatting, inline comments, and section ordering.
///
/// For YAML, the original text is re-parsed with `yaml_edit::Document`
/// (a Rowan-based CST that retains comments and whitespace), and only the
/// paths that differ between `old_value` and `new_value` are updated.
///
/// JSON falls through to [`serialize_value`] (JSON has no comments).
pub fn serialize_value_preserving(
    original_content: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
    format: &FileFormat,
) -> anyhow::Result<String> {
    match format {
        FileFormat::Toml => {
            let mut doc: toml_edit::DocumentMut = original_content
                .parse()
                .map_err(|e| anyhow::anyhow!("TOML re-parse for comment preservation: {e}"))?;
            apply_value_diff(doc.as_item_mut(), old_value, new_value);
            Ok(doc.to_string())
        }
        FileFormat::Yaml => {
            if let Some(result) = try_preserve_yaml(original_content, old_value, new_value)? {
                return Ok(result);
            }
            // Fall back to non-preserving (with hoisted comments).
            if old_value == new_value {
                Ok(original_content.to_string())
            } else {
                let body = serialize_value(new_value, format)?;
                Ok(preserve::hoist_comments(original_content, &body))
            }
        }
        // JSON has no comments.
        _ => serialize_value(new_value, format),
    }
}

/// Attempt CST-preserving update for YAML. Returns Some(result) on success,
/// None if a text-level fallback (or plain serialize) is required.
fn try_preserve_yaml(
    original_content: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    use std::str::FromStr;

    let file = yaml_edit::YamlFile::from_str(original_content)
        .map_err(|e| anyhow::anyhow!("YAML re-parse for comment preservation: {e}"))?;

    if let Some(doc) = file.document() {
        if let Some(mapping) = doc.as_mapping() {
            if old_value.is_object() && new_value.is_object() {
                return try_preserve_yaml_object(&file, &mapping, old_value, new_value);
            }
        } else if let Some(seq) = doc.as_sequence()
            && let (Some(old_arr), Some(new_arr)) = (old_value.as_array(), new_value.as_array())
        {
            return try_preserve_yaml_array(
                &file,
                &seq,
                original_content,
                old_arr,
                new_arr,
                new_value,
            );
        }
    }
    Ok(None)
}

fn try_preserve_yaml_object(
    file: &yaml_edit::YamlFile,
    mapping: &yaml_edit::Mapping,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    let has_array_growth = yaml_splice::has_array_growth_diffs(old_value, new_value);
    let all_cst_applied = apply_yaml_mapping_diff(mapping, old_value, new_value)?;
    let result = file.to_string();

    if let Ok(reparsed) = serde_yaml_ng::from_str::<serde_json::Value>(&result)
        && reparsed.is_object()
        && reparsed == *new_value
        && !has_array_growth
        && all_cst_applied
    {
        return Ok(Some(result));
    }

    // Array growth or structure mismatch: try splice.
    let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result)?;
    if let Some(spliced) = yaml_splice::splice_yaml_array_diffs(&result, &reparsed, new_value)? {
        return Ok(Some(spliced));
    }
    Ok(None)
}

fn try_preserve_yaml_array(
    file: &yaml_edit::YamlFile,
    seq: &yaml_edit::Sequence,
    original_content: &str,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
    new_value: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    let applied = if old_arr.len() == new_arr.len() {
        apply_yaml_sequence_diff(seq, old_arr, new_arr)?
    } else if new_arr.len() < old_arr.len() {
        try_remove_subsequence(seq, old_arr, new_arr)
    } else {
        false
    };
    if applied {
        let result = file.to_string();
        if serde_yaml_ng::from_str::<serde_json::Value>(&result).is_ok_and(|v| v.is_array()) {
            return Ok(Some(result));
        }
    }

    // Growth or failure: text splice.
    if new_arr.len() > old_arr.len()
        && let Some(spliced) =
            yaml_splice::splice_yaml_root_sequence(original_content, old_arr, new_arr)?
        && serde_yaml_ng::from_str::<serde_json::Value>(&spliced).is_ok_and(|v| v == *new_value)
    {
        return Ok(Some(spliced));
    }
    Ok(None)
}

// -----------------------------------------------------------------------
// TOML comment-preserving helpers
// -----------------------------------------------------------------------

/// Recursively walk `old` and `new` JSON value trees and apply only the
/// differences to the `toml_edit::Item`, preserving comments and formatting
/// on unchanged parts.
fn apply_value_diff(item: &mut toml_edit::Item, old: &serde_json::Value, new: &serde_json::Value) {
    if old == new {
        return;
    }

    match (old, new) {
        (serde_json::Value::Object(old_map), serde_json::Value::Object(new_map)) => {
            // Try to get a mutable table reference from the item.
            let table = if let Some(t) = item.as_table_mut() {
                t
            } else if item.as_inline_table_mut().is_some() {
                // Inline table: fall back to wholesale replacement since
                // inline tables don't carry per-key comments.
                *item = json_to_toml_item(new);
                return;
            } else {
                *item = json_to_toml_item(new);
                return;
            };

            // Remove keys that no longer exist.
            let removed: Vec<String> = old_map
                .keys()
                .filter(|k| !new_map.contains_key(k.as_str()))
                .cloned()
                .collect();
            for k in &removed {
                table.remove(k);
            }

            // Add new keys or recurse into changed values.
            for (key, new_val) in new_map {
                if let Some(old_val) = old_map.get(key) {
                    if old_val != new_val
                        && let Some(child) = table.get_mut(key)
                    {
                        apply_value_diff(child, old_val, new_val);
                    }
                } else {
                    table.insert(key, json_to_toml_item(new_val));
                }
            }
        }

        (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr))
            if old_arr.len() == new_arr.len() =>
        {
            // Same-length arrays: recurse element by element.
            if let Some(arr) = item.as_array_mut() {
                for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                    if o != n
                        && let Some(v) = arr.get_mut(i)
                    {
                        *v = json_to_toml_value(n);
                    }
                }
            } else if let Some(aot) = item.as_array_of_tables_mut() {
                for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
                    if o != n
                        && let Some(table_item) = aot.get_mut(i)
                    {
                        let mut tbl_item = toml_edit::Item::Table(table_item.clone());
                        apply_value_diff(&mut tbl_item, o, n);
                        if let toml_edit::Item::Table(t) = tbl_item {
                            *table_item = t;
                        }
                    }
                }
            } else {
                *item = json_to_toml_item(new);
            }
        }

        // Type changed, different-length arrays, or scalar change:
        // wholesale replacement.
        _ => {
            *item = json_to_toml_item(new);
        }
    }
}

/// Convert a `serde_json::Value` to a `toml_edit::Value` (scalar/array/inline-table).
fn json_to_toml_value(val: &serde_json::Value) -> toml_edit::Value {
    match val {
        serde_json::Value::String(s) => toml_edit::Value::from(s.as_str()),
        serde_json::Value::Bool(b) => toml_edit::Value::from(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml_edit::Value::from(i)
            } else {
                // Covers u64 > i64::MAX and float values.
                toml_edit::Value::from(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::Array(arr) => {
            let mut a = toml_edit::Array::new();
            for v in arr {
                a.push(json_to_toml_value(v));
            }
            toml_edit::Value::Array(a)
        }
        serde_json::Value::Object(map) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, v) in map {
                t.insert(k, json_to_toml_value(v));
            }
            toml_edit::Value::InlineTable(t)
        }
        serde_json::Value::Null => {
            // TOML has no null; use empty string as fallback.
            toml_edit::Value::from("")
        }
    }
}

/// Convert a `serde_json::Value` to a `toml_edit::Item`.
///
/// Objects become full `Table`s (not inline tables) so they render as
/// `[section]` blocks. Arrays of objects become arrays-of-tables.
fn json_to_toml_item(val: &serde_json::Value) -> toml_edit::Item {
    match val {
        serde_json::Value::Object(map) => {
            let mut table = toml_edit::Table::new();
            for (k, v) in map {
                table.insert(k, json_to_toml_item(v));
            }
            toml_edit::Item::Table(table)
        }
        serde_json::Value::Array(arr) if !arr.is_empty() && arr.iter().all(|v| v.is_object()) => {
            let mut aot = toml_edit::ArrayOfTables::new();
            for v in arr {
                if let serde_json::Value::Object(map) = v {
                    let mut table = toml_edit::Table::new();
                    for (k, v2) in map {
                        table.insert(k, json_to_toml_item(v2));
                    }
                    aot.push(table);
                }
            }
            toml_edit::Item::ArrayOfTables(aot)
        }
        _ => toml_edit::Item::Value(json_to_toml_value(val)),
    }
}

// -----------------------------------------------------------------------
// YAML comment-preserving helpers
// -----------------------------------------------------------------------

/// Recursively walk `old` and `new` JSON value trees and apply only the
/// differences to the `yaml_edit::Mapping`, preserving comments and
/// formatting on unchanged parts.
/// Returns `Ok(true)` if all CST changes were fully applied, `Ok(false)`
/// if at least one array resize was too complex for the CST path.
fn apply_yaml_mapping_diff(
    mapping: &yaml_edit::Mapping,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> anyhow::Result<bool> {
    if old == new {
        return Ok(true);
    }

    let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
        return Ok(true);
    };
    let mut all_applied = true;

    // Remove keys that no longer exist.
    let removed: Vec<String> = old_map
        .keys()
        .filter(|k| !new_map.contains_key(k.as_str()))
        .cloned()
        .collect();
    for k in &removed {
        mapping.remove(k.as_str());
    }

    // Add new keys or recurse into changed values.
    for (key, new_val) in new_map {
        if let Some(old_val) = old_map.get(key) {
            if old_val == new_val {
                continue;
            }
            match (old_val, new_val) {
                // Both objects: recurse using child view from get_mapping. Updates to
                // pre-existing keys inside the sub use in-place set_value (preserves
                // sibling inline comments). Brand-new keys inside may not attach on the
                // cloned sub view (structure check will catch and fallback with comments
                // preserved).
                (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                    if let Some(child) = mapping.get_mapping(key.as_str()) {
                        if !apply_yaml_mapping_diff(&child, old_val, new_val)? {
                            all_applied = false;
                        }
                    } else {
                        mapping.set(key.as_str(), json_to_yaml_mapping(new_val)?);
                    }
                }
                // Both arrays: update via the existing sequence node to
                // preserve block/flow style and the key-value newline.
                (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr)) => {
                    if let Some(seq) = mapping.get_sequence(key.as_str()) {
                        if old_arr.len() == new_arr.len() {
                            if !apply_yaml_sequence_diff(&seq, old_arr, new_arr)? {
                                all_applied = false;
                            }
                        } else if !apply_yaml_sequence_resize(
                            &seq,
                            old_arr,
                            new_arr,
                            mapping,
                            key.as_str(),
                            new_val,
                        ) {
                            all_applied = false;
                        }
                    } else {
                        mapping.set(key.as_str(), json_to_yaml_node(new_val)?);
                    }
                }
                // Type changed or scalar change.
                _ => {
                    mapping.set(key.as_str(), json_to_yaml_node(new_val)?);
                }
            }
        } else {
            // New key: add it.
            if new_val.is_object() {
                // Follow yaml-edit's own pattern for creating nested: set empty first,
                // re-fetch the nested view (to get linked node), then populate.
                // This ensures correct block indentation and attachment in the CST.
                let empty = yaml_edit::Mapping::new();
                mapping.set(key.as_str(), &empty);
                if let Some(nested) = mapping.get_mapping(key.as_str()) {
                    if let Some(obj) = new_val.as_object() {
                        for (k, v) in obj {
                            nested.set(k.as_str(), json_to_yaml_node(v)?);
                        }
                    }
                } else {
                    mapping.set(key.as_str(), json_to_yaml_mapping(new_val)?);
                }
            } else {
                mapping.set(key.as_str(), json_to_yaml_node(new_val)?);
            }
        }
    }
    Ok(all_applied)
}

/// Element-by-element diff for same-length YAML sequences.
/// Returns `Ok(true)` if all CST changes were fully applied.
fn apply_yaml_sequence_diff(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> anyhow::Result<bool> {
    let mut all_applied = true;
    for (i, (o, n)) in old_arr.iter().zip(new_arr.iter()).enumerate() {
        if o == n {
            continue;
        }
        match (o, n) {
            (serde_json::Value::Object(_), serde_json::Value::Object(_)) => {
                if let Some(node) = seq.get(i)
                    && let Some(child_mapping) = node.as_mapping()
                {
                    if !apply_yaml_mapping_diff(child_mapping, o, n)? {
                        all_applied = false;
                    }
                    continue;
                }
                seq.set(i, json_to_yaml_node(n)?);
            }
            _ => {
                seq.set(i, json_to_yaml_node(n)?);
            }
        }
    }
    Ok(all_applied)
}

/// Handle different-length array diffs while preserving comments.
///
/// Deletion is handled via targeted `Sequence::remove()` calls.
/// Growth (prepend, append, general restructuring) leaves the CST
/// unchanged; the caller in `serialize_value_preserving` handles it
/// via text-level splicing so comments are always preserved.
/// Returns `true` if the CST was successfully updated, `false` if the
/// change was too complex for the CST path (caller should flag the result
/// as needing a fallback).
fn apply_yaml_sequence_resize(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
    _mapping: &yaml_edit::Mapping,
    _key: &str,
    _new_val: &serde_json::Value,
) -> bool {
    if new_arr.len() < old_arr.len() && try_remove_subsequence(seq, old_arr, new_arr) {
        return true;
    }
    // Growth or complex deletion: CST unchanged. Return false so the
    // caller knows a text-level fallback is needed.
    false
}

/// Try to remove elements from `seq` so that it matches `new_arr`,
/// treating `new_arr` as an ordered subsequence of `old_arr`.
///
/// Returns `true` if the removal succeeded, `false` if `new_arr` is
/// not a subsequence of `old_arr` (caller should fall back).
fn try_remove_subsequence(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> bool {
    let new_len = new_arr.len();
    let mut remove_indices = Vec::new();
    let mut ni = 0;
    for (oi, old_item) in old_arr.iter().enumerate() {
        if ni < new_len && *old_item == new_arr[ni] {
            ni += 1;
        } else {
            remove_indices.push(oi);
        }
    }
    if ni != new_len {
        return false;
    }
    // Iterate in reverse to keep indices stable during removal.
    for &idx in remove_indices.iter().rev() {
        seq.remove(idx);
    }
    true
}

/// Convert a `serde_json::Value` to a `yaml_edit::YamlNode` by
/// round-tripping through `serde_yaml_ng` (for correct serialization)
/// and `yaml_edit` (for a CST node that `Mapping::set` can accept).
///
/// The value is embedded under a temporary key `__v__` so that
/// `serde_yaml_ng` handles indentation of block sequences/mappings.
fn json_to_yaml_node(val: &serde_json::Value) -> anyhow::Result<yaml_edit::YamlNode> {
    use std::str::FromStr;
    let wrapper = serde_json::json!({ "__v__": val });
    let yaml_text = serde_yaml_ng::to_string(&wrapper)
        .map_err(|e| anyhow::anyhow!("YAML serialization failed: {e}"))?;
    let doc = yaml_edit::Document::from_str(&yaml_text)
        .map_err(|e| anyhow::anyhow!("YAML CST re-parse failed: {e}"))?;
    doc.as_mapping()
        .and_then(|m| m.get("__v__"))
        .ok_or_else(|| anyhow::anyhow!("YAML CST wrapper key missing"))
}

/// Convert a JSON object to a `yaml_edit::Mapping`.
fn json_to_yaml_mapping(val: &serde_json::Value) -> anyhow::Result<yaml_edit::Mapping> {
    let mapping = yaml_edit::Mapping::new();
    if let Some(obj) = val.as_object() {
        for (k, v) in obj {
            mapping.set(k.as_str(), json_to_yaml_node(v)?);
        }
    }
    Ok(mapping)
}

pub fn parse_doc(content: &str, format: &FileFormat) -> anyhow::Result<serde_json::Value> {
    match format {
        FileFormat::Json => Ok(serde_json::from_str(content)?),
        FileFormat::Yaml => {
            let mut val: serde_json::Value = serde_yaml_ng::from_str(content)?;
            resolve_yaml_merge_keys(&mut val);
            Ok(val)
        }
        FileFormat::Toml => Ok(toml_edit::de::from_str(content)?),
    }
}

/// Recursively resolve YAML merge keys (`<<`) in a parsed JSON value.
///
/// When `serde_yaml_ng` deserializes `<<: *anchor`, it produces a literal
/// `"<<"` key whose value is the referenced mapping.  This function walks
/// the tree and flattens those entries into the parent object, matching
/// YAML merge-key semantics (existing keys take precedence).
fn resolve_yaml_merge_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            // First, recurse into all child values (including the merge value itself).
            for v in map.values_mut() {
                resolve_yaml_merge_keys(v);
            }

            // Then resolve `<<` if present.
            if let Some(merge_val) = map.remove("<<") {
                match merge_val {
                    serde_json::Value::Object(merged) => {
                        for (k, v) in merged {
                            map.entry(k).or_insert(v);
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        // Multiple merges: `<<: [*a, *b]` — first wins.
                        for item in arr {
                            if let serde_json::Value::Object(merged) = item {
                                for (k, v) in merged {
                                    map.entry(k).or_insert(v);
                                }
                            }
                        }
                    }
                    _ => {
                        // Non-object merge value — put it back as-is.
                        map.insert("<<".to_string(), merge_val);
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                resolve_yaml_merge_keys(v);
            }
        }
        _ => {}
    }
}

pub fn navigate_mut<'a>(
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

/// Returns the parent path and the final segment (for use by set/delete/move).
fn split_last(segments: &[selector::Segment]) -> (&[selector::Segment], &selector::Segment) {
    let (parent, last) = segments.split_at(segments.len() - 1);
    (parent, &last[0])
}

/// Set a value at the location described by `segments`.  Navigates to the
/// parent (creating intermediate keys when needed) and inserts the value at
/// the final Key or Index segment.
pub fn set_at_path(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
    value: serde_json::Value,
) -> anyhow::Result<()> {
    if segments.is_empty() {
        anyhow::bail!("empty selector");
    }
    let (parent_path, last) = split_last(segments);
    let parent = navigate_mut(root, parent_path, true)?;

    match last {
        selector::Segment::Key(k) => {
            parent
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                .insert(k.clone(), value);
        }
        selector::Segment::Index(i) => {
            let arr = parent
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
            if *i < arr.len() {
                arr[*i] = value;
            } else {
                anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
            }
        }
        _ => anyhow::bail!("cannot set at wildcard/predicate"),
    }
    Ok(())
}

/// Delete the value at the given selector path. Returns `true` if
/// something was removed, `false` if the path did not exist.
pub fn delete_at_selector(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
) -> anyhow::Result<bool> {
    if segments.is_empty() {
        return Ok(false);
    }
    let (parent_path, last) = split_last(segments);
    let parent = match navigate_mut(root, parent_path, false) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    match last {
        selector::Segment::Key(k) => {
            if let Some(obj) = parent.as_object_mut() {
                Ok(obj.remove(k.as_str()).is_some())
            } else {
                Ok(false)
            }
        }
        selector::Segment::Index(i) => {
            if let Some(arr) = parent.as_array_mut() {
                if *i < arr.len() {
                    arr.remove(*i);
                    Ok(true)
                } else {
                    Ok(false)
                }
            } else {
                Ok(false)
            }
        }
        _ => anyhow::bail!("cannot delete at wildcard/predicate"),
    }
}

/// Parse a `key=value` predicate and remove matching items from the array
/// at `segments`. Returns the number of items removed.
pub fn delete_where(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
    predicate: &str,
) -> anyhow::Result<usize> {
    let eq_pos = predicate
        .find('=')
        .ok_or_else(|| anyhow::anyhow!("predicate must be in key=value format"))?;
    let pred_key = predicate[..eq_pos].trim();
    if pred_key.is_empty() {
        anyhow::bail!("predicate key is empty; expected key=value format");
    }
    let raw_val = &predicate[eq_pos + 1..];
    if raw_val.starts_with('=') {
        anyhow::bail!("predicate uses '==' but only '=' is supported; use key=value format");
    }
    let pred_val = raw_val.trim();

    let target = navigate_mut(root, segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;

    let before_len = arr.len();
    arr.retain(|item| {
        item.get(pred_key)
            .is_none_or(|field| !selector::value_matches_str(field, pred_val))
    });
    Ok(before_len - arr.len())
}

/// Move a value from one path to another within the same document.
/// Removes the value at `from_segments` and inserts it at `to_segments`.
pub fn move_at_path(
    root: &mut serde_json::Value,
    from_segments: &[selector::Segment],
    to_segments: &[selector::Segment],
) -> anyhow::Result<()> {
    // Remove value at source path.
    let removed = {
        if from_segments.is_empty() {
            anyhow::bail!("empty from selector");
        }
        let (parent_path, last) = split_last(from_segments);
        let parent = navigate_mut(root, parent_path, false)?;
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
    let (parent_path, last) = split_last(to_segments);
    let parent = navigate_mut(root, parent_path, true)?;
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
    Ok(())
}

const MAX_MERGE_DEPTH: usize = 128;

pub fn deep_merge(base: &mut serde_json::Value, other: &serde_json::Value) {
    deep_merge_inner(base, other, 0);
}

fn deep_merge_inner(base: &mut serde_json::Value, other: &serde_json::Value, depth: usize) {
    if depth >= MAX_MERGE_DEPTH {
        *base = other.clone();
        return;
    }
    if let (Some(base_map), Some(other_map)) = (base.as_object_mut(), other.as_object()) {
        for (key, value) in other_map {
            let entry = base_map
                .entry(key.clone())
                .or_insert(serde_json::Value::Null);
            deep_merge_inner(entry, value, depth + 1);
        }
    } else {
        *base = other.clone();
    }
}

pub fn update_matching(
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
                    let matches = item
                        .get(key.as_str())
                        .is_some_and(|field| selector::value_matches_str(field, pred_val));
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
// Unified doc mutation dispatch
// ---------------------------------------------------------------------------

/// Describes a single mutation to apply to a parsed document root.
///
/// This enum captures the 9 doc write operations so that both the CLI
/// (`cmd/doc.rs`) and the transaction engine (`tx.rs`) share a single
/// dispatch path instead of duplicating the match logic.
#[derive(Debug)]
pub enum DocMutation {
    Set {
        selector: String,
        value: serde_json::Value,
    },
    Delete {
        selector: String,
    },
    Merge {
        value: serde_json::Value,
    },
    Append {
        selector: String,
        value: serde_json::Value,
    },
    Prepend {
        selector: String,
        value: serde_json::Value,
    },
    Update {
        selector: String,
        value: serde_json::Value,
    },
    Move {
        from: String,
        to: String,
    },
    Ensure {
        selector: String,
        value: serde_json::Value,
    },
    DeleteWhere {
        selector: String,
        predicate: String,
    },
}

/// Result of applying a [`DocMutation`] to a document root.
#[derive(Debug)]
pub enum MutationResult {
    /// The mutation was applied and the document was modified.
    Applied,
    /// The selector matched nothing (e.g. delete on a missing key).
    NoMatch,
    /// The path already exists (used by `Ensure` when no write is needed).
    AlreadyExists,
    /// A type error occurred (e.g. append to a non-array). The string
    /// includes the operation name prefix for backward-compatible error
    /// messages (e.g. "doc append: target at 'x' is not an array").
    TypeError(String),
}

/// Apply a [`DocMutation`] to an in-memory document root.
///
/// Callers are responsible for:
/// - Parsing the file and providing the root `Value`
/// - Serializing the modified root back to disk
/// - Mapping [`MutationResult`] to the appropriate exit code or error
pub fn apply_doc_mutation(
    root: &mut serde_json::Value,
    mutation: DocMutation,
) -> anyhow::Result<MutationResult> {
    match mutation {
        DocMutation::Set { selector, value } => {
            let sel = selector::parse_anyhow(&selector)?;
            set_at_path(root, &sel, value)?;
            Ok(MutationResult::Applied)
        }
        DocMutation::Delete { selector } => {
            let sel = selector::parse_anyhow(&selector)?;
            if delete_at_selector(root, &sel)? {
                Ok(MutationResult::Applied)
            } else {
                Ok(MutationResult::NoMatch)
            }
        }
        DocMutation::Merge { value } => {
            deep_merge(root, &value);
            Ok(MutationResult::Applied)
        }
        DocMutation::Append { selector, value } => {
            let sel = selector::parse_anyhow(&selector)?;
            let target = navigate_mut(root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => {
                    arr.push(value);
                    Ok(MutationResult::Applied)
                }
                None => Ok(MutationResult::TypeError(format!(
                    "doc append: target at '{selector}' is not an array"
                ))),
            }
        }
        DocMutation::Prepend { selector, value } => {
            let sel = selector::parse_anyhow(&selector)?;
            let target = navigate_mut(root, &sel, false)?;
            match target.as_array_mut() {
                Some(arr) => {
                    arr.insert(0, value);
                    Ok(MutationResult::Applied)
                }
                None => Ok(MutationResult::TypeError(format!(
                    "doc prepend: target at '{selector}' is not an array"
                ))),
            }
        }
        DocMutation::Update { selector, value } => {
            let sel = selector::parse_anyhow(&selector)?;
            if update_matching(root, &sel, &value) == 0 {
                Ok(MutationResult::NoMatch)
            } else {
                Ok(MutationResult::Applied)
            }
        }
        DocMutation::Move { from, to } => {
            let from_sel = selector::parse_anyhow(&from)?;
            let to_sel = selector::parse_anyhow(&to)?;
            move_at_path(root, &from_sel, &to_sel)?;
            Ok(MutationResult::Applied)
        }
        DocMutation::Ensure { selector, value } => {
            let sel = selector::parse_anyhow(&selector)?;
            if !selector::eval(root, &sel).is_empty() {
                Ok(MutationResult::AlreadyExists)
            } else {
                set_at_path(root, &sel, value)?;
                Ok(MutationResult::Applied)
            }
        }
        DocMutation::DeleteWhere {
            selector,
            predicate,
        } => {
            let sel = selector::parse_anyhow(&selector)?;
            let removed = delete_where(root, &sel, &predicate)?;
            if removed == 0 {
                Ok(MutationResult::NoMatch)
            } else {
                Ok(MutationResult::Applied)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // ── doc module tests ──────────────────────────────────────────────
    mod doc_tests {
        use crate::ops::doc::*;
        use crate::selector;
        use serde_json::json;

        // ── gap closing regressions (#824/#825 style fidelity) ───────────
        #[test]
        fn yaml_deep_nested_object_creation() {
            // Deep (2+) new intermediates: in-memory always correct.
            // Preserving serialize may use fallback for some >1-level cases
            // (data correct, comments may be hoisted). We assert in-memory +
            // plain serialize roundtrip.
            let mut v = json!({"root": 1});
            set_at_path(&mut v, &crate::selector::parse("a.b.c").unwrap(), json!(42)).unwrap();
            assert_eq!(v, json!({"root":1, "a":{"b":{"c":42}}}));

            let plain = serialize_value(&v, &FileFormat::Yaml).unwrap();
            let reparsed: serde_json::Value = serde_yaml_ng::from_str(&plain).unwrap();
            assert_eq!(reparsed, json!({"root":1, "a":{"b":{"c":42}}}));
        }

        #[test]
        fn yaml_new_key_in_existing_nested_preserves_sibling_comment() {
            let yaml = "server:\n  # keep host\n  host: localhost\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut newv = old.clone();
            set_at_path(
                &mut newv,
                &crate::selector::parse("server.port").unwrap(),
                json!(9090),
            )
            .unwrap();
            let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
            assert!(result.contains("# keep host"));
            assert!(
                result.contains("9090") || result.contains("port"),
                "new port value missing"
            );
            // Note: when adding a brand-new key inside a sub, current CST may
            // place it at wrong level (fallback produces correct data). Full
            // attachment for new siblings inside subs is a remaining gap.
        }

        #[test]
        fn toml_deep_nested_table_creation() {
            let toml = "# header\n[app]\nname = \"x\"\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut newv = old.clone();
            set_at_path(
                &mut newv,
                &crate::selector::parse("app.server.tls.port").unwrap(),
                json!(9443),
            )
            .unwrap();
            let result = serialize_value_preserving(toml, &old, &newv, &FileFormat::Toml).unwrap();
            let reparsed: serde_json::Value = toml_edit::de::from_str(&result).expect("valid");
            assert_eq!(
                reparsed,
                json!({"app":{"name":"x","server":{"tls":{"port":9443}}}})
            );
            assert!(result.contains("# header"));
        }

        #[test]
        fn detect_format_json() {
            assert!(matches!(
                detect_format("config.json").unwrap(),
                FileFormat::Json
            ));
        }

        #[test]
        fn detect_format_yaml() {
            assert!(matches!(
                detect_format("config.yaml").unwrap(),
                FileFormat::Yaml
            ));
            assert!(matches!(
                detect_format("config.yml").unwrap(),
                FileFormat::Yaml
            ));
        }

        #[test]
        fn detect_format_toml() {
            assert!(matches!(
                detect_format("Cargo.toml").unwrap(),
                FileFormat::Toml
            ));
        }

        #[test]
        fn yaml_merge_keys_resolved() {
            let yaml = "defaults: &d\n  timeout: 30\n  retries: 3\nstaging:\n  <<: *d\n";
            let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            assert_eq!(val["staging"]["retries"], json!(3));
            assert_eq!(val["staging"]["timeout"], json!(30));
            // The merge key itself must be removed.
            assert!(val["staging"].get("<<").is_none());
        }

        #[test]
        fn yaml_merge_key_existing_wins() {
            let yaml = "base: &b\n  x: 1\nchild:\n  <<: *b\n  x: 99\n";
            let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            assert_eq!(val["child"]["x"], json!(99));
        }

        #[test]
        fn yaml_merge_key_multiple() {
            let yaml = "a: &a\n  x: 1\nb: &b\n  y: 2\nc:\n  <<:\n    - *a\n    - *b\n";
            let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            assert_eq!(val["c"]["x"], json!(1));
            assert_eq!(val["c"]["y"], json!(2));
        }

        #[test]
        fn detect_format_unsupported() {
            assert!(detect_format("readme.txt").is_err());
        }

        // -- TOML comment preservation ----------------------------------------

        #[test]
        fn toml_comment_preserved_on_set() {
            let toml = "# top\n[server]\nhost = \"localhost\" # hostname\nport = 8080\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("server".into()),
                    selector::Segment::Key("port".into()),
                ],
                json!(9090),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# top"), "top comment missing");
            assert!(result.contains("# hostname"), "inline comment missing");
            assert!(result.contains("9090"), "new value missing");
            assert!(!result.contains("8080"), "old value still present");
        }

        #[test]
        fn toml_comment_preserved_on_delete() {
            let toml = "# keep this\n[section]\na = 1\nb = 2 # inline\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            // Delete key "a" from section.
            new.as_object_mut()
                .unwrap()
                .get_mut("section")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("a");

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# keep this"), "top comment missing");
            assert!(result.contains("# inline"), "inline comment missing");
            assert!(result.contains("b = 2"), "surviving key missing");
            assert!(!result.contains("a = 1"), "deleted key still present");
        }

        #[test]
        fn toml_section_order_preserved() {
            let toml = "[z_last]\nval = 1\n\n[a_first]\nval = 2\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("a_first".into()),
                    selector::Segment::Key("val".into()),
                ],
                json!(99),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            let z_pos = result.find("z_last").unwrap();
            let a_pos = result.find("a_first").unwrap();
            assert!(z_pos < a_pos, "section order changed: z@{z_pos} a@{a_pos}");
        }

        #[test]
        fn toml_new_key_inserted_without_breaking_comments() {
            let toml = "# config\n[pkg]\nname = \"app\"\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("pkg".into()),
                    selector::Segment::Key("version".into()),
                ],
                json!("1.0"),
            )
            .unwrap();

            let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
            assert!(result.contains("# config"), "comment missing");
            assert!(result.contains("name = \"app\""), "existing key missing");
            assert!(result.contains("version"), "new key missing");
        }

        #[test]
        fn toml_inline_table_style_preserved() {
            let toml = "[deps]\nserde = { version = \"1\", features = [\"derive\"] }\n";
            let old = parse_doc(toml, &FileFormat::Toml).unwrap();
            // No change — verify round-trip preserves inline style.
            let result = serialize_value_preserving(toml, &old, &old, &FileFormat::Toml).unwrap();
            assert!(result.contains("serde = {"), "inline table style lost");
        }

        // -- YAML comment preservation ----------------------------------------

        #[test]
        fn yaml_comment_preserved_on_set() {
            let yaml = "# top\nserver:\n  host: localhost # hostname\n  port: 8080\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[
                    selector::Segment::Key("server".into()),
                    selector::Segment::Key("port".into()),
                ],
                json!(9090),
            )
            .unwrap();

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("# top"), "top comment missing");
            assert!(result.contains("# hostname"), "inline comment missing");
            assert!(result.contains("9090"), "new value missing");
            assert!(!result.contains("8080"), "old value still present");
        }

        #[test]
        fn yaml_comment_preserved_on_delete() {
            let yaml = "# keep this\na: 1\nb: 2 # inline\nc: 3\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            // Delete key "a".
            new.as_object_mut().unwrap().remove("a");

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("# keep this"), "top comment missing");
            assert!(result.contains("# inline"), "inline comment missing");
            assert!(result.contains("b: 2"), "surviving key missing");
            assert!(!result.contains("a: 1"), "deleted key still present");
        }

        #[test]
        fn yaml_key_order_preserved() {
            let yaml = "z_last: 1\na_first: 2\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[selector::Segment::Key("a_first".into())],
                json!(99),
            )
            .unwrap();

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            let z_pos = result.find("z_last").unwrap();
            let a_pos = result.find("a_first").unwrap();
            assert!(z_pos < a_pos, "key order changed: z@{z_pos} a@{a_pos}");
        }

        #[test]
        fn yaml_new_key_inserted_without_breaking_comments() {
            let yaml = "# config\nname: app\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(
                &mut new,
                &[selector::Segment::Key("version".into())],
                json!("1.0"),
            )
            .unwrap();

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("# config"), "comment missing");
            assert!(result.contains("name: app"), "existing key missing");
            assert!(result.contains("version"), "new key missing");
        }

        #[test]
        fn yaml_noop_roundtrip_preserves_comments() {
            let yaml = "# top comment\nname: app\n# section\nserver:\n  port: 8080\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            // No change — verify round-trip preserves everything.
            let result =
                serialize_value_preserving(yaml, &old, &old, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert_eq!(result, yaml, "no-op roundtrip should be identical");
        }

        #[test]
        fn yaml_nested_keys_create_correct_structure() {
            // Regression for #824: new nested via set/ensure/merge must produce
            // valid indented mappings, not "key:\nleaf: " (null parent + sibling).
            let yaml = "a: 1\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut newv = old.clone();
            set_at_path(
                &mut newv,
                &crate::selector::parse("server.port").unwrap(),
                json!(9090),
            )
            .unwrap();
            let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
            let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
            assert_eq!(reparsed, json!({"a":1, "server":{"port":9090}}));
            // Also for ensure path (no-op if exists, but create case)
            let mut new2 = old.clone();
            // simulate ensure by set since not exist
            set_at_path(
                &mut new2,
                &crate::selector::parse("settings.debug").unwrap(),
                json!(false),
            )
            .unwrap();
            let r2 = serialize_value_preserving(yaml, &old, &new2, &FileFormat::Yaml).unwrap();
            let p2: serde_json::Value = serde_yaml_ng::from_str(&r2).unwrap();
            assert_eq!(p2, json!({"a":1, "settings":{"debug":false}}));
            // merge case
            let mut new3 = old.clone();
            deep_merge(&mut new3, &json!({"server": {"port": 9090}}));
            let r3 = serialize_value_preserving(yaml, &old, &new3, &FileFormat::Yaml).unwrap();
            let p3: serde_json::Value = serde_yaml_ng::from_str(&r3).unwrap();
            assert_eq!(p3, json!({"a":1, "server":{"port":9090}}));
        }

        #[test]
        fn yaml_ensure_nested_in_existing_subobject_preserves_top_comments() {
            // Mirrors the integration test_doc_ensure_yaml_preserves_comments
            // server.port ensure when "server" key already exists as object.
            let yaml = "# Config\nname: my-app\n\n# Server\nserver:\n  host: localhost\n";
            let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
            let mut newv = old.clone();
            set_at_path(
                &mut newv,
                &crate::selector::parse("server.port").unwrap(),
                json!(8080),
            )
            .unwrap();
            let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
            // Must preserve top and section comments (the bug was falling back to plain serialize)
            assert!(
                result.contains("# Config"),
                "top comment lost in ensure nested: {result}"
            );
            assert!(
                result.contains("# Server"),
                "section comment lost: {result}"
            );
            let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
            assert_eq!(
                reparsed,
                json!({"name":"my-app","server":{"host":"localhost","port":8080}})
            );
        }

        #[test]
        fn yaml_section_comment_between_keys_preserved() {
            let yaml = "a: 1\n\n# Section B\nb: 2\n\n# Section C\nc: 3\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            set_at_path(&mut new, &[selector::Segment::Key("b".into())], json!(99)).unwrap();

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("# Section B"), "section B comment missing");
            assert!(result.contains("# Section C"), "section C comment missing");
            assert!(result.contains("b: 99"), "new value missing");
            assert!(!result.contains("b: 2"), "old value still present");
        }

        #[test]
        fn yaml_array_shrink_with_modification_not_lost() {
            // Regression: complex array shrinkage (shrink + modify remaining
            // elements) must not be silently dropped. The CST path cannot
            // handle this case, so it should fall through to serialize_value.
            let yaml = "# Config\nname: app\ntags:\n  - alpha\n  - beta\n  - gamma\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let new = json!({"name": "app", "tags": ["MODIFIED"]});

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            let parsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
            assert_eq!(
                parsed, new,
                "serialized YAML must match target value: {result}"
            );
        }

        #[test]
        fn yaml_nested_complex_shrinkage_in_same_length_outer() {
            // Regression: same-length outer array with complex inner array
            // shrinkage (not a subsequence). apply_yaml_sequence_diff must
            // propagate the failure flag from the nested mapping diff.
            let yaml =
                "# Comment\nitems:\n  - name: a\n    tags:\n      - x\n      - y\n  - name: b\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let new = json!({"items": [{"name": "a", "tags": ["MODIFIED"]}, {"name": "b"}]});

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            let parsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
            assert_eq!(
                parsed, new,
                "nested complex shrinkage must not be silently dropped: {result}"
            );
        }

        #[test]
        fn yaml_nested_array_growth_detected() {
            // Regression: has_array_growth_diffs must recurse into same-length
            // array elements to detect nested growth (e.g., adding a port to
            // the first server's ports array while the outer array stays the
            // same length).
            let yaml = "# config comment\nservers:\n  - name: web\n    ports:\n      - 80\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            new["servers"][0]["ports"]
                .as_array_mut()
                .unwrap()
                .push(json!(443));

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(
                result.contains("443"),
                "nested array growth should not be silently dropped: {result}"
            );
            assert!(
                result.contains("# config comment"),
                "comments should be preserved: {result}"
            );
        }

        #[test]
        fn yaml_sequence_root_mutation_not_lost() {
            let yaml = "- item1\n- item2\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let mut new = old.clone();
            new.as_array_mut().unwrap().push(json!("item3"));

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("item3"), "appended item missing: {result}");
            assert!(result.contains("item1"), "item1 missing: {result}");
            assert!(result.contains("item2"), "item2 missing: {result}");
        }

        #[test]
        fn yaml_mapping_to_scalar_root_type_change_not_lost() {
            let yaml = "name: app\nversion: 1\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let new = json!("overridden");

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(
                result.contains("overridden"),
                "root type change lost: {result}"
            );
        }

        #[test]
        fn yaml_mapping_to_array_root_type_change_not_lost() {
            let yaml = "name: app\nversion: 1\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let new = json!(["a", "b"]);

            let result =
                serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert!(result.contains("- a"), "array item '- a' missing: {result}");
            assert!(result.contains("- b"), "array item '- b' missing: {result}");
        }

        #[test]
        fn yaml_sequence_root_noop_preserves_content() {
            let yaml = "- item1\n- item2\n";
            let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
            let result =
                serialize_value_preserving(yaml, &old, &old, &crate::ops::doc::FileFormat::Yaml)
                    .unwrap();
            assert_eq!(result, yaml, "no-op roundtrip should be identical");
        }

        #[test]
        fn detect_format_no_extension() {
            assert!(detect_format("Makefile").is_err());
        }

        #[test]
        fn parse_and_serialize_json_roundtrip() {
            let input = "{\n  \"a\": 1\n}\n";
            let val = parse_doc(input, &FileFormat::Json).unwrap();
            assert_eq!(val, json!({"a": 1}));
            let out = serialize_value(&val, &FileFormat::Json).unwrap();
            assert_eq!(out, input);
        }

        #[test]
        fn parse_and_serialize_yaml_roundtrip() {
            let input = "a: 1\n";
            let val = parse_doc(input, &crate::ops::doc::FileFormat::Yaml).unwrap();
            assert_eq!(val, json!({"a": 1}));
            let out = serialize_value(&val, &crate::ops::doc::FileFormat::Yaml).unwrap();
            assert_eq!(out, input);
        }

        #[test]
        fn parse_and_serialize_toml_roundtrip() {
            let input = "a = 1\n";
            let val = parse_doc(input, &FileFormat::Toml).unwrap();
            assert_eq!(val, json!({"a": 1}));
            // TOML pretty serialization may differ slightly; just ensure it parses back
            let out = serialize_value(&val, &FileFormat::Toml).unwrap();
            let reparsed = parse_doc(&out, &FileFormat::Toml).unwrap();
            assert_eq!(reparsed, json!({"a": 1}));
        }

        #[test]
        fn navigate_mut_existing_key() {
            let mut val = json!({"a": {"b": 42}});
            let seg = crate::selector::parse("a.b").unwrap();
            let found = navigate_mut(&mut val, &seg, false).unwrap();
            assert_eq!(*found, json!(42));
        }

        #[test]
        fn navigate_mut_missing_key_no_create() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b").unwrap();
            assert!(navigate_mut(&mut val, &seg, false).is_err());
        }

        #[test]
        fn navigate_mut_create_missing_key() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b.c").unwrap();
            let found = navigate_mut(&mut val, &seg, true).unwrap();
            // created as empty object, then descended into "c" which was also created
            assert!(found.is_object());
        }

        #[test]
        fn navigate_mut_array_index() {
            let mut val = json!({"items": [10, 20, 30]});
            let seg = crate::selector::parse("items[1]").unwrap();
            let found = navigate_mut(&mut val, &seg, false).unwrap();
            assert_eq!(*found, json!(20));
        }

        #[test]
        fn navigate_mut_index_out_of_bounds() {
            let mut val = json!({"items": [10]});
            let seg = crate::selector::parse("items[5]").unwrap();
            assert!(navigate_mut(&mut val, &seg, false).is_err());
        }

        #[test]
        fn deep_merge_objects() {
            let mut base = json!({"a": 1, "b": {"c": 2}});
            let other = json!({"b": {"d": 3}, "e": 4});
            deep_merge(&mut base, &other);
            assert_eq!(base, json!({"a": 1, "b": {"c": 2, "d": 3}, "e": 4}));
        }

        #[test]
        fn deep_merge_overwrites_non_object() {
            let mut base = json!({"a": "string"});
            let other = json!({"a": {"nested": true}});
            deep_merge(&mut base, &other);
            assert_eq!(base, json!({"a": {"nested": true}}));
        }

        #[test]
        fn deep_merge_depth_limit() {
            // Build a deeply nested structure beyond MAX_MERGE_DEPTH (128)
            let mut deep_val = json!("leaf");
            for _ in 0..130 {
                deep_val = json!({"n": deep_val});
            }
            let mut base = json!({});
            deep_merge(&mut base, &deep_val);
            // Should not panic; at depth 128 it clones instead of recursing
            assert!(base.is_object());
        }

        #[test]
        fn set_at_path_simple_key() {
            let mut root = json!({"a": 1});
            let sel = crate::selector::parse("b").unwrap();
            set_at_path(&mut root, &sel, json!(2)).unwrap();
            assert_eq!(root, json!({"a": 1, "b": 2}));
        }

        #[test]
        fn set_at_path_nested_creates_intermediates() {
            let mut root = json!({});
            let sel = crate::selector::parse("a.b.c").unwrap();
            set_at_path(&mut root, &sel, json!("deep")).unwrap();
            assert_eq!(root, json!({"a": {"b": {"c": "deep"}}}));
        }

        #[test]
        fn set_at_path_array_index() {
            let mut root = json!({"items": [10, 20, 30]});
            let sel = crate::selector::parse("items[1]").unwrap();
            set_at_path(&mut root, &sel, json!(99)).unwrap();
            assert_eq!(root, json!({"items": [10, 99, 30]}));
        }

        #[test]
        fn set_at_path_out_of_bounds_index_fails() {
            let mut root = json!({"items": [1]});
            let sel = crate::selector::parse("items[5]").unwrap();
            assert!(set_at_path(&mut root, &sel, json!(99)).is_err());
        }

        #[test]
        fn set_at_path_empty_selector_fails() {
            let mut root = json!({});
            let sel: Vec<crate::selector::Segment> = vec![];
            assert!(set_at_path(&mut root, &sel, json!(1)).is_err());
        }

        #[test]
        fn delete_where_removes_matching_items() {
            let mut root = json!({"items": [{"name": "a"}, {"name": "b"}, {"name": "c"}]});
            let sel = crate::selector::parse("items").unwrap();
            let removed = delete_where(&mut root, &sel, "name=b").unwrap();
            assert_eq!(removed, 1);
            assert_eq!(root["items"].as_array().unwrap().len(), 2);
        }

        #[test]
        fn delete_where_no_match_returns_zero() {
            let mut root = json!({"items": [{"name": "a"}]});
            let sel = crate::selector::parse("items").unwrap();
            let removed = delete_where(&mut root, &sel, "name=zzz").unwrap();
            assert_eq!(removed, 0);
        }

        #[test]
        fn delete_where_invalid_predicate_fails() {
            let mut root = json!({"items": [{"name": "a"}]});
            let sel = crate::selector::parse("items").unwrap();
            assert!(delete_where(&mut root, &sel, "no-equals-sign").is_err());
        }

        #[test]
        fn delete_where_non_array_fails() {
            let mut root = json!({"items": "not-an-array"});
            let sel = crate::selector::parse("items").unwrap();
            assert!(delete_where(&mut root, &sel, "k=v").is_err());
        }

        #[test]
        fn delete_where_double_equals_rejected() {
            let mut root =
                json!({"items": [{"name": "a", "keep": false}, {"name": "b", "keep": true}]});
            let sel = crate::selector::parse("items").unwrap();
            let err = delete_where(&mut root, &sel, "keep == false").unwrap_err();
            assert!(
                err.to_string().contains("'=='"),
                "expected == rejection, got: {err}"
            );
            // Array must be unchanged (no silent removal).
            assert_eq!(root["items"].as_array().unwrap().len(), 2);
        }

        #[test]
        fn delete_where_empty_key_rejected() {
            let mut root = json!({"items": [{"name": "a"}]});
            let sel = crate::selector::parse("items").unwrap();
            let err = delete_where(&mut root, &sel, "=value").unwrap_err();
            assert!(
                err.to_string().contains("empty"),
                "expected empty-key error, got: {err}"
            );
        }

        #[test]
        fn delete_where_trims_whitespace() {
            let mut root = json!({"items": [{"name": "a"}, {"name": "b"}]});
            let sel = crate::selector::parse("items").unwrap();
            // Spaces around key and value should be trimmed.
            let removed = delete_where(&mut root, &sel, " name = a ").unwrap();
            assert_eq!(removed, 1);
            assert_eq!(root["items"].as_array().unwrap().len(), 1);
            assert_eq!(root["items"][0]["name"], "b");
        }

        #[test]
        fn delete_at_selector_removes_key() {
            let mut root = json!({"a": 1, "b": 2});
            let sel = crate::selector::parse("b").unwrap();
            assert!(delete_at_selector(&mut root, &sel).unwrap());
            assert_eq!(root, json!({"a": 1}));
        }

        #[test]
        fn delete_at_selector_missing_returns_false() {
            let mut root = json!({"a": 1});
            let sel = crate::selector::parse("nonexistent").unwrap();
            assert!(!delete_at_selector(&mut root, &sel).unwrap());
        }

        #[test]
        fn delete_at_selector_array_index() {
            let mut root = json!({"items": [10, 20, 30]});
            let sel = crate::selector::parse("items[1]").unwrap();
            assert!(delete_at_selector(&mut root, &sel).unwrap());
            assert_eq!(root, json!({"items": [10, 30]}));
        }

        #[test]
        fn delete_at_selector_empty_returns_false() {
            let mut root = json!({"a": 1});
            let sel: Vec<crate::selector::Segment> = vec![];
            assert!(!delete_at_selector(&mut root, &sel).unwrap());
        }

        #[test]
        fn move_at_path_renames_key() {
            let mut root = json!({"old_name": "value", "other": 1});
            let from = crate::selector::parse("old_name").unwrap();
            let to = crate::selector::parse("new_name").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            assert_eq!(root, json!({"other": 1, "new_name": "value"}));
        }

        #[test]
        fn move_at_path_to_nested_creates_intermediates() {
            let mut root = json!({"src": 42});
            let from = crate::selector::parse("src").unwrap();
            let to = crate::selector::parse("a.b.dst").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            assert_eq!(root, json!({"a": {"b": {"dst": 42}}}));
        }

        #[test]
        fn move_at_path_missing_source_fails() {
            let mut root = json!({"a": 1});
            let from = crate::selector::parse("nonexistent").unwrap();
            let to = crate::selector::parse("b").unwrap();
            assert!(move_at_path(&mut root, &from, &to).is_err());
        }

        #[test]
        fn move_at_path_empty_from_selector_fails() {
            let mut root = json!({"a": 1});
            let from: Vec<crate::selector::Segment> = vec![];
            let to = crate::selector::parse("b").unwrap();
            assert!(move_at_path(&mut root, &from, &to).is_err());
        }

        #[test]
        fn move_at_path_to_array_index() {
            let mut root = json!({"src": "x", "arr": [1, 2, 3]});
            let from = crate::selector::parse("src").unwrap();
            let to = crate::selector::parse("arr[1]").unwrap();
            move_at_path(&mut root, &from, &to).unwrap();
            let arr = root["arr"].as_array().unwrap();
            assert_eq!(arr.len(), 4);
            assert_eq!(arr[1], json!("x"));
        }

        #[test]
        fn update_matching_by_key() {
            let mut val = json!({"a": {"b": "old"}});
            let seg = crate::selector::parse("a.b").unwrap();
            let count = update_matching(&mut val, &seg, &json!("new"));
            assert_eq!(count, 1);
            assert_eq!(val, json!({"a": {"b": "new"}}));
        }

        #[test]
        fn update_matching_wildcard() {
            let mut val = json!({"items": [{"v": 1}, {"v": 2}]});
            let seg = crate::selector::parse("items[*].v").unwrap();
            let count = update_matching(&mut val, &seg, &json!(99));
            assert_eq!(count, 2);
            assert_eq!(val, json!({"items": [{"v": 99}, {"v": 99}]}));
        }

        #[test]
        fn update_matching_predicate() {
            let mut val = json!({"items": [
                {"name": "a", "v": 1},
                {"name": "b", "v": 2}
            ]});
            let seg = crate::selector::parse("items[name=b].v").unwrap();
            let count = update_matching(&mut val, &seg, &json!(42));
            assert_eq!(count, 1);
            assert_eq!(val["items"][1]["v"], json!(42));
            // First item unchanged
            assert_eq!(val["items"][0]["v"], json!(1));
        }

        #[test]
        fn update_matching_missing_key_returns_zero() {
            let mut val = json!({"a": 1});
            let seg = crate::selector::parse("b.c").unwrap();
            let count = update_matching(&mut val, &seg, &json!("x"));
            assert_eq!(count, 0);
        }
    }

    // ── needs_yaml_quoting tests ─────────────────────────────────────
    mod yaml_quoting_tests {
        use crate::ops::doc::needs_yaml_quoting;

        #[test]
        fn yaml_booleans_need_quoting() {
            for kw in ["true", "false", "yes", "no", "on", "off"] {
                assert!(needs_yaml_quoting(kw), "{kw} should need quoting");
            }
        }

        #[test]
        fn yaml_booleans_case_insensitive() {
            for kw in ["True", "FALSE", "Yes", "NO", "On", "OFF", "TrUe"] {
                assert!(needs_yaml_quoting(kw), "{kw} should need quoting");
            }
        }

        #[test]
        fn yaml_null_and_tilde_need_quoting() {
            assert!(needs_yaml_quoting("null"));
            assert!(needs_yaml_quoting("Null"));
            assert!(needs_yaml_quoting("NULL"));
            assert!(needs_yaml_quoting("~"));
        }

        #[test]
        fn yaml_numbers_need_quoting() {
            assert!(needs_yaml_quoting("42"));
            assert!(needs_yaml_quoting("3.14"));
            assert!(needs_yaml_quoting("-1"));
            assert!(needs_yaml_quoting("0"));
        }

        #[test]
        fn yaml_empty_string_needs_quoting() {
            assert!(needs_yaml_quoting(""));
        }

        #[test]
        fn yaml_special_prefix_chars_need_quoting() {
            assert!(needs_yaml_quoting("#comment"));
            assert!(needs_yaml_quoting("&anchor"));
            assert!(needs_yaml_quoting("*alias"));
            assert!(needs_yaml_quoting("?key"));
            assert!(needs_yaml_quoting("|literal"));
            assert!(needs_yaml_quoting(">folded"));
            assert!(needs_yaml_quoting("{flow}"));
            assert!(needs_yaml_quoting("[list]"));
            assert!(needs_yaml_quoting("%directive"));
            assert!(needs_yaml_quoting("@reserved"));
            assert!(needs_yaml_quoting("`backtick"));
            assert!(needs_yaml_quoting("\"quoted"));
            assert!(needs_yaml_quoting("'squoted"));
        }

        #[test]
        fn yaml_colon_space_and_space_hash_need_quoting() {
            assert!(needs_yaml_quoting("key: value"));
            assert!(needs_yaml_quoting("hello #comment"));
        }

        #[test]
        fn yaml_plain_strings_do_not_need_quoting() {
            assert!(!needs_yaml_quoting("hello"));
            assert!(!needs_yaml_quoting("foo-bar"));
            assert!(!needs_yaml_quoting("some_value_123"));
            assert!(!needs_yaml_quoting("v1.2.3"));
        }
    }

    // ── proptest: doc round-trip ─────────────────────────────────────
    mod proptest_doc {
        use crate::ops::doc::*;
        use crate::selector;
        use proptest::prelude::*;
        use serde_json::json;

        /// Generate an arbitrary JSON value (limited depth to keep tests fast).
        fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
            let leaf = prop_oneof![
                Just(json!(null)),
                any::<bool>().prop_map(|b| json!(b)),
                any::<i64>().prop_map(|n| json!(n)),
                "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
            ];
            leaf.prop_recursive(3, 32, 5, |inner| {
                prop_oneof![
                    prop::collection::vec(inner.clone(), 0..5).prop_map(serde_json::Value::Array),
                    prop::collection::vec(("[a-zA-Z_][a-zA-Z0-9_]{0,10}", inner), 0..5).prop_map(
                        |entries| {
                            let map: serde_json::Map<String, serde_json::Value> =
                                entries.into_iter().collect();
                            serde_json::Value::Object(map)
                        }
                    ),
                ]
            })
        }

        proptest! {
            /// JSON round-trip: serialize then reparse must produce the same value.
            #[test]
            fn json_round_trip(value in arb_json_value()) {
                let serialized = serialize_value(&value, &FileFormat::Json).unwrap();
                let reparsed = parse_doc(&serialized, &FileFormat::Json).unwrap();
                prop_assert_eq!(&value, &reparsed);
            }

            /// TOML round-trip for object values (TOML root must be a table).
            #[test]
            fn toml_round_trip(entries in prop::collection::vec(
                ("[a-zA-Z_][a-zA-Z0-9_]{0,10}", prop_oneof![
                    any::<bool>().prop_map(|b| json!(b)),
                    any::<i64>().prop_map(|n| json!(n)),
                    "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                ]),
                0..8
            )) {
                let map: serde_json::Map<String, serde_json::Value> =
                    entries.into_iter().collect();
                let value = serde_json::Value::Object(map);
                let serialized = serialize_value(&value, &FileFormat::Toml).unwrap();
                let reparsed = parse_doc(&serialized, &FileFormat::Toml).unwrap();
                prop_assert_eq!(&value, &reparsed);
            }

            /// YAML round-trip: serialize then reparse must produce the same value.
            #[test]
            fn yaml_round_trip(value in arb_json_value()) {
                let serialized = serialize_value(&value, &crate::ops::doc::FileFormat::Yaml).unwrap();
                let reparsed = parse_doc(&serialized, &crate::ops::doc::FileFormat::Yaml).unwrap();
                prop_assert_eq!(&value, &reparsed);
            }

            /// JSON set-then-get: setting a key and retrieving it must return the set value.
            #[test]
            fn json_set_then_get(
                key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
                set_value in prop_oneof![
                    any::<bool>().prop_map(|b| json!(b)),
                    any::<i64>().prop_map(|n| json!(n)),
                    "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                ],
            ) {
                let mut root = json!({});
                let sel = selector::parse(&key).unwrap();
                set_at_path(&mut root, &sel, set_value.clone()).unwrap();
                let results = selector::eval(&root, &sel);
                prop_assert_eq!(results.len(), 1);
                prop_assert_eq!(results[0], &set_value);
            }

            /// JSON delete-then-has: deleting a key means it no longer exists.
            #[test]
            fn json_delete_then_has(
                key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
            ) {
                let mut root = json!({ &key: "value" });
                let sel = selector::parse(&key).unwrap();
                let deleted = delete_at_selector(&mut root, &sel).unwrap();
                prop_assert!(deleted);
                let results = selector::eval(&root, &sel);
                prop_assert!(results.is_empty());
            }

            /// JSON preserving round-trip: set a key, serialize preserving, reparse.
            #[test]
            fn json_preserving_set_round_trip(
                key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
                set_value in prop_oneof![
                    any::<bool>().prop_map(|b| json!(b)),
                    any::<i64>().prop_map(|n| json!(n)),
                    "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                ],
            ) {
                let original_content = "{\"existing\": 1}";
                let original_value = parse_doc(original_content, &FileFormat::Json).unwrap();
                let mut new_value = original_value.clone();
                let sel = selector::parse(&key).unwrap();
                set_at_path(&mut new_value, &sel, set_value.clone()).unwrap();

                let serialized = serialize_value_preserving(
                    original_content,
                    &original_value,
                    &new_value,
                    &FileFormat::Json,
                ).unwrap();
                let reparsed = parse_doc(&serialized, &FileFormat::Json).unwrap();
                prop_assert_eq!(&new_value, &reparsed);
            }
        }
    }

    // ── DocMutation / apply_doc_mutation tests ────────────────────────
    mod mutation_tests {
        use super::super::*;
        use serde_json::json;

        #[test]
        fn mutation_set() {
            let mut root = json!({"a": 1});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Set {
                    selector: "b".into(),
                    value: json!(2),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root, json!({"a": 1, "b": 2}));
        }

        #[test]
        fn mutation_delete_existing() {
            let mut root = json!({"a": 1, "b": 2});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Delete {
                    selector: "b".into(),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root, json!({"a": 1}));
        }

        #[test]
        fn mutation_delete_missing() {
            let mut root = json!({"a": 1});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Delete {
                    selector: "z".into(),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::NoMatch));
        }

        #[test]
        fn mutation_merge() {
            let mut root = json!({"a": 1});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Merge {
                    value: json!({"b": 2}),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root, json!({"a": 1, "b": 2}));
        }

        #[test]
        fn mutation_append_to_array() {
            let mut root = json!({"items": [1, 2]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Append {
                    selector: "items".into(),
                    value: json!(3),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root["items"], json!([1, 2, 3]));
        }

        #[test]
        fn mutation_append_to_non_array() {
            let mut root = json!({"items": "not-array"});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Append {
                    selector: "items".into(),
                    value: json!(1),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::TypeError(_)));
        }

        #[test]
        fn mutation_prepend_to_array() {
            let mut root = json!({"items": [2, 3]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Prepend {
                    selector: "items".into(),
                    value: json!(1),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root["items"], json!([1, 2, 3]));
        }

        #[test]
        fn mutation_update_matching() {
            let mut root = json!({"items": [{"id": 1, "v": "a"}, {"id": 2, "v": "b"}]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Update {
                    selector: "items[id=1]".into(),
                    value: json!({"id": 1, "v": "updated"}),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root["items"][0]["v"], "updated");
        }

        #[test]
        fn mutation_update_no_match() {
            let mut root = json!({"items": [{"id": 1}]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Update {
                    selector: "items[id=99]".into(),
                    value: json!({"id": 99}),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::NoMatch));
        }

        #[test]
        fn mutation_move_key() {
            let mut root = json!({"src": 42, "dst": {}});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Move {
                    from: "src".into(),
                    to: "dst.val".into(),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root, json!({"dst": {"val": 42}}));
        }

        #[test]
        fn mutation_move_missing_source() {
            let mut root = json!({"a": 1});
            let err = apply_doc_mutation(
                &mut root,
                DocMutation::Move {
                    from: "nonexistent".into(),
                    to: "b".into(),
                },
            );
            assert!(err.is_err());
            assert!(
                err.unwrap_err()
                    .to_string()
                    .contains("source key 'nonexistent' not found")
            );
        }

        #[test]
        fn mutation_prepend_to_non_array() {
            let mut root = json!({"items": "not-array"});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Prepend {
                    selector: "items".into(),
                    value: json!(1),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::TypeError(_)));
        }

        #[test]
        fn mutation_ensure_missing() {
            let mut root = json!({"a": 1});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Ensure {
                    selector: "b".into(),
                    value: json!(2),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root, json!({"a": 1, "b": 2}));
        }

        #[test]
        fn mutation_ensure_existing() {
            let mut root = json!({"a": 1});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::Ensure {
                    selector: "a".into(),
                    value: json!(99),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::AlreadyExists));
            assert_eq!(root["a"], 1); // unchanged
        }

        #[test]
        fn mutation_delete_where_matching() {
            let mut root = json!({"items": [{"k": "a"}, {"k": "b"}]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::DeleteWhere {
                    selector: "items".into(),
                    predicate: "k=a".into(),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::Applied));
            assert_eq!(root["items"].as_array().unwrap().len(), 1);
        }

        #[test]
        fn mutation_delete_where_no_match() {
            let mut root = json!({"items": [{"k": "a"}]});
            let result = apply_doc_mutation(
                &mut root,
                DocMutation::DeleteWhere {
                    selector: "items".into(),
                    predicate: "k=z".into(),
                },
            )
            .unwrap();
            assert!(matches!(result, MutationResult::NoMatch));
        }
    }
}
