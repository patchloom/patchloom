//! Document format ops (JSON/YAML/TOML): detect, parse, serialize, preserve.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). Format detect,
//! multi-document YAML parse/write (#1718), and CST-preserving serialize
//! entrypoints are one document surface; do not split for LOC alone.

use crate::selector;
use serde::Deserialize;
use std::path::Path;

mod navigate;
mod preserve;
pub mod query;
mod toml_preserve;
mod yaml_cst;
mod yaml_splice;

pub use navigate::{
    deep_merge, delete_at_selector, delete_where, move_at_path, navigate_mut, set_at_path,
    update_matching,
};
use toml_preserve::apply_value_diff;
use yaml_cst::{apply_yaml_mapping_diff, apply_yaml_sequence_diff, try_remove_subsequence};

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
        Some(ext) => Err(crate::exit::InvalidInputError {
            msg: format!(
                "unsupported file extension: .{ext} (supported: .json, .yaml, .yml, .toml)"
            ),
        }
        .into()),
        None => Err(crate::exit::InvalidInputError {
            msg: "file has no extension; doc commands require .json, .yaml, .yml, or .toml".into(),
        }
        .into()),
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
            let s = toml_edit::ser::to_string_pretty(value).map_err(|e| {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("TOML serialization error: {e}"),
                })
            })?;
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
            let mut doc: toml_edit::DocumentMut = original_content.parse().map_err(|e| {
                anyhow::Error::new(crate::exit::ParseErrorError {
                    msg: format!("TOML re-parse for comment preservation: {e}"),
                })
            })?;
            apply_value_diff(doc.as_item_mut(), old_value, new_value);
            Ok(doc.to_string())
        }
        FileFormat::Yaml => {
            // Multi-document streams must stay multi-document on write. Falling
            // through to single-doc serialize turns `---` docs into a YAML
            // sequence (`- item:`), which breaks kubectl/kustomize manifests.
            if is_multi_document_yaml(original_content) {
                return serialize_multi_document_yaml(original_content, old_value, new_value);
            }
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
        // JSON has no comments; return the original text when unchanged
        // to avoid spurious formatting diffs (e.g. array compaction changes).
        _ => {
            if old_value == new_value {
                Ok(original_content.to_string())
            } else {
                serialize_value(new_value, format)
            }
        }
    }
}

/// Clean up whitespace artifacts left by `yaml_edit` CST mutations.
///
/// `Mapping::remove()` can leave trailing whitespace on lines and may
/// drop the final newline. This trims each line's trailing spaces and
/// ensures the output ends with exactly one newline.
fn cleanup_yaml_cst_whitespace(text: &str) -> String {
    let mut result: String = text
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Fix broken block-mapping indentation left by `yaml_edit` CST removals.
///
/// When `Mapping::remove()` deletes an entry in a block mapping, the
/// whitespace tokens surrounding the removed key may be absorbed by the
/// adjacent entry, giving it extra leading spaces. For example, removing
/// `name` from:
///
/// ```yaml
/// app:
///   name: "my-app"
///   version: "1.0.0"
///   port: "8080"
/// ```
///
/// can produce:
///
/// ```yaml
/// app:
///     version: "1.0.0"
///   port: "8080"
/// ```
///
/// or (when the middle key is removed):
///
/// ```yaml
/// app:
///   name: "my-app"
///     port: "8080"
/// ```
///
/// This function detects entries whose indentation exceeds their siblings
/// in the same block mapping and normalizes them to match.
fn fix_yaml_block_indentation(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());

    for i in 0..lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();

        // Skip empty lines; comments are handled below.
        if trimmed.is_empty() {
            result.push(line.to_string());
            continue;
        }

        // Fix comment lines: a comment that is indented more than the next
        // mapping entry at the same level has orphaned whitespace from a
        // removed sibling.  Re-indent it to match the next entry.
        if trimmed.starts_with('#') {
            let comment_indent = line.len() - trimmed.len();
            if comment_indent > 0
                && let Some(next_indent) = next_entry_indent(&lines, i)
                && next_indent < comment_indent
            {
                result.push(format!("{}{}", " ".repeat(next_indent), trimmed));
                continue;
            }
            result.push(line.to_string());
            continue;
        }

        let indent = line.len() - trimmed.len();
        let is_mapping_entry = indent > 0 && (trimmed.contains(": ") || trimmed.ends_with(':'));

        if is_mapping_entry
            && let Some(expected) = expected_sibling_indent(&lines, i, indent)
            && expected < indent
        {
            result.push(format!("{}{}", " ".repeat(expected), trimmed));
            continue;
        }

        result.push(line.to_string());
    }

    let mut out = result.join("\n");
    if text.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Find the indentation of the next non-empty, non-comment line after `i`.
/// Used to align orphaned comment lines with their associated mapping entry.
fn next_entry_indent(lines: &[&str], i: usize) -> Option<usize> {
    for line in &lines[i + 1..] {
        let t = line.trim();
        if !t.is_empty() && !t.starts_with('#') {
            return Some(line.len() - t.len());
        }
    }
    None
}

/// Determine the expected indentation for a mapping entry by inspecting
/// its neighbors. Returns `Some(indent)` if a sibling at a lower indent
/// is found (indicating the current line has orphaned extra whitespace).
fn expected_sibling_indent(lines: &[&str], i: usize, current_indent: usize) -> Option<usize> {
    let is_significant = |l: &&str| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    };

    // Strategy 1: look DOWN for a sibling with less indent.
    if let Some(next_line) = lines[i + 1..].iter().find(|l| is_significant(l)) {
        let nt = next_line.trim_start();
        let ni = next_line.len() - nt.len();
        let next_is_entry = nt.contains(": ") || nt.ends_with(':');

        if ni < current_indent && ni > 0 && next_is_entry {
            // Verify: prev line must be a parent (indent < ni, ends ':')
            // or another sibling at the correct indent (== ni).
            let prev_ok = lines[..i]
                .iter()
                .rev()
                .find(|l| is_significant(l))
                .is_some_and(|l| {
                    let pi = l.len() - l.trim_start().len();
                    (pi < ni && l.trim_end().ends_with(':')) || pi == ni
                });
            if prev_ok {
                return Some(ni);
            }
        }
    }

    // Strategy 2: look UP for a sibling with less indent (handles the case
    // where the corrupted line is the last entry in the mapping).
    if let Some(prev_line) = lines[..i].iter().rev().find(|l| is_significant(l)) {
        let pt = prev_line.trim_start();
        let pi = prev_line.len() - pt.len();

        // Prev must be a complete mapping entry (has a value after the colon,
        // so it is a sibling rather than a parent). A parent key ends with ':'
        // alone; a complete entry has ': <value>'.
        if pi < current_indent && pi > 0 && pt.contains(": ") && !is_yaml_parent_line(pt) {
            return Some(pi);
        }
    }

    None
}

/// Check if a trimmed YAML line is a parent key (value is on the next line).
///
/// Parent patterns:
///   `key:`
///   `key: # comment`
///   `key:  `
///
/// Complete entry: `key: value`
fn is_yaml_parent_line(trimmed: &str) -> bool {
    if trimmed.ends_with(':') {
        return true;
    }
    if let Some(pos) = trimmed.find(": ") {
        let after = trimmed[pos + 2..].trim();
        return after.is_empty() || after.starts_with('#');
    }
    false
}

/// Attempt CST-preserving update for YAML. Returns Some(result) on success,
/// None if a text-level fallback (or plain serialize) is required.
fn try_preserve_yaml(
    original_content: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    use std::str::FromStr;

    let file = yaml_edit::YamlFile::from_str(original_content).map_err(|e| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("YAML re-parse for comment preservation: {e}"),
        })
    })?;

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
    // yaml_edit's Mapping::remove() can leave trailing whitespace on the
    // line preceding the removed key and may shift the indentation of the
    // next sibling entry. Clean both artifacts so the output is valid YAML
    // that preserves original quote styles and key ordering.
    let result = fix_yaml_block_indentation(&cleanup_yaml_cst_whitespace(&file.to_string()));

    if let Ok(reparsed) = serde_yaml_ng::from_str::<serde_json::Value>(&result)
        && reparsed.is_object()
        && reparsed == *new_value
        && !has_array_growth
        && all_cst_applied
    {
        return Ok(Some(result));
    }

    // Array growth or structure mismatch: try splice.
    // If the CST produced invalid YAML (e.g., duplicated keys from
    // misinterpreted indentation, #972), serde_yaml_ng will fail to parse
    // it. In that case, skip the splice and fall through to the caller's
    // non-preserving fallback instead of propagating the error.
    if let Ok(reparsed) = serde_yaml_ng::from_str::<serde_json::Value>(&result)
        && let Some(spliced) = yaml_splice::splice_yaml_array_diffs(&result, &reparsed, new_value)?
    {
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
        let result = cleanup_yaml_cst_whitespace(&file.to_string());
        if serde_yaml_ng::from_str::<serde_json::Value>(&result).is_ok_and(|v| v == *new_value) {
            return Ok(Some(result));
        }
    }

    // Growth or failure: text splice.
    if new_arr.len() > old_arr.len()
        && let Some(spliced) =
            yaml_splice::splice_yaml_root_sequence(original_content, old_arr, new_arr)?
        && serde_yaml_ng::from_str::<serde_json::Value>(&spliced).is_ok_and(|v| v == *new_value)
        && spliced.parse::<yaml_edit::YamlFile>().is_ok()
    {
        return Ok(Some(spliced));
    }
    Ok(None)
}

pub fn parse_doc(content: &str, format: &FileFormat) -> anyhow::Result<serde_json::Value> {
    match format {
        FileFormat::Json => serde_json::from_str(content)
            .map_err(|e| anyhow::Error::new(crate::exit::ParseErrorError { msg: e.to_string() })),
        FileFormat::Yaml => {
            if is_multi_document_yaml(content) {
                parse_multi_document_yaml(content).map_err(|e| {
                    // Multi-doc path may already be typed; re-wrap plain anyhow.
                    if crate::exit::is_parse_error(&e) {
                        e
                    } else {
                        anyhow::Error::new(crate::exit::ParseErrorError { msg: e.to_string() })
                    }
                })
            } else {
                let mut val: serde_json::Value = serde_yaml_ng::from_str(content).map_err(|e| {
                    anyhow::Error::new(crate::exit::ParseErrorError { msg: e.to_string() })
                })?;
                resolve_yaml_merge_keys(&mut val);
                Ok(val)
            }
        }
        FileFormat::Toml => toml_edit::de::from_str(content)
            .map_err(|e| anyhow::Error::new(crate::exit::ParseErrorError { msg: e.to_string() })),
    }
}

/// Check whether YAML content contains multiple documents.
///
/// A `---` on its own line is the YAML document separator. A single leading
/// `---` is standard YAML preamble, not multi-document. We look for a second
/// `---` separator after stripping the optional leading one.
pub(crate) fn is_multi_document_yaml(content: &str) -> bool {
    // Skip the optional leading document marker.
    let rest = content.strip_prefix("---").map_or(content, |after| {
        // The leading `---` must be followed by whitespace, a comment, newline,
        // or EOF to count as a document marker rather than part of a value.
        if is_after_yaml_marker(after) {
            skip_to_next_line(after)
        } else {
            content
        }
    });

    // Check if rest itself starts with a `---` separator (consecutive markers).
    if rest.starts_with("---") && is_after_yaml_marker(&rest[3..]) {
        return true;
    }

    // Look for `\n---` followed by whitespace, comment, newline, or EOF.
    for (i, _) in rest.match_indices("\n---") {
        let after_marker = &rest[i + 4..];
        if is_after_yaml_marker(after_marker) {
            return true;
        }
    }
    false
}

/// Check whether the text after `---` constitutes a valid document marker
/// ending: empty (EOF), newline, whitespace before newline, or `#` comment.
fn is_after_yaml_marker(after: &str) -> bool {
    if after.is_empty() {
        return true;
    }
    let b = after.as_bytes()[0];
    // Direct newline or carriage return.
    if b == b'\n' || b == b'\r' {
        return true;
    }
    // Trailing whitespace or comment: skip spaces/tabs, then expect newline/comment/EOF.
    if b == b' ' || b == b'\t' || b == b'#' {
        let rest = after
            .as_bytes()
            .iter()
            .skip_while(|&&c| c == b' ' || c == b'\t')
            .copied()
            .next();
        return rest.is_none() || rest == Some(b'\n') || rest == Some(b'\r') || rest == Some(b'#');
    }
    false
}

/// Skip past the current line (after `---`) to the start of the next line.
fn skip_to_next_line(s: &str) -> &str {
    match s.find('\n') {
        Some(pos) => &s[pos + 1..],
        None => "",
    }
}

/// Parse multi-document YAML into a JSON array, resolving merge keys per doc.
fn parse_multi_document_yaml(content: &str) -> anyhow::Result<serde_json::Value> {
    let mut docs = Vec::new();
    for de in serde_yaml_ng::Deserializer::from_str(content) {
        let mut val: serde_json::Value = serde_json::Value::deserialize(de)?;
        resolve_yaml_merge_keys(&mut val);
        docs.push(val);
    }
    // Precondition: is_multi_document_yaml() returned true, so content has
    // ---  separators and the YAML deserializer always yields at least one doc.
    debug_assert!(!docs.is_empty(), "multi-doc YAML produced zero documents");
    Ok(serde_json::Value::Array(docs))
}

/// Whether a single line (without trailing newline) is a YAML document marker.
fn is_yaml_document_separator_line(line: &str) -> bool {
    line.strip_prefix("---").is_some_and(is_after_yaml_marker)
}

/// Split multi-document YAML into document body strings.
///
/// Returns `(had_leading_marker, bodies)`. Bodies exclude the `---` separator
/// lines. Caller should only invoke when [`is_multi_document_yaml`] is true.
pub(crate) fn split_multi_document_yaml(content: &str) -> (bool, Vec<String>) {
    let mut bodies: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut leading_marker = false;
    let mut first_line = true;
    let mut saw_body = false;

    for line in content.split_inclusive('\n') {
        let without_nl = line.trim_end_matches(['\n', '\r']);
        if is_yaml_document_separator_line(without_nl) {
            if first_line && !saw_body {
                // Leading stream marker (optional preamble), not a boundary.
                leading_marker = true;
                first_line = false;
                continue;
            }
            bodies.push(std::mem::take(&mut current));
            first_line = false;
            continue;
        }
        first_line = false;
        saw_body = true;
        current.push_str(line);
    }
    bodies.push(current);
    (leading_marker, bodies)
}

/// Reassemble multi-document YAML from body strings.
fn join_multi_document_yaml(leading_marker: bool, docs: &[String]) -> String {
    let mut out = String::new();
    if leading_marker {
        out.push_str("---\n");
    }
    for (i, doc) in docs.iter().enumerate() {
        if i > 0 {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("---\n");
        }
        let body = doc.trim_end_matches(['\n', '\r']);
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

/// Serialize one document body (single-doc preserve path, no multi-doc re-entry).
fn serialize_single_yaml_document(
    original_body: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> anyhow::Result<String> {
    if old_value == new_value && !original_body.is_empty() {
        return Ok(original_body.to_string());
    }
    if !original_body.trim().is_empty()
        && let Some(result) = try_preserve_yaml(original_body, old_value, new_value)?
    {
        return Ok(result);
    }
    let body = serialize_value(new_value, &FileFormat::Yaml)?;
    if original_body.is_empty() {
        Ok(body)
    } else {
        Ok(preserve::hoist_comments(original_body, &body))
    }
}

/// Write multi-document YAML while keeping `---` document separators.
///
/// Parse models multi-doc streams as a JSON array. Naively serializing that
/// array emits a single sequence document (`- kind: ...`), which is not a
/// multi-document stream and breaks tools that expect `---` separators
/// (e.g. `kubectl apply -f`).
fn serialize_multi_document_yaml(
    original_content: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> anyhow::Result<String> {
    if old_value == new_value {
        return Ok(original_content.to_string());
    }

    let (leading_marker, bodies) = split_multi_document_yaml(original_content);

    let Some(new_docs) = new_value.as_array() else {
        // Unexpected: multi-doc parse always yields an array. Fall back.
        let body = serialize_value(new_value, &FileFormat::Yaml)?;
        return Ok(preserve::hoist_comments(original_content, &body));
    };

    let old_docs = old_value.as_array();
    let mut out_docs: Vec<String> = Vec::with_capacity(new_docs.len());

    for (i, new_doc) in new_docs.iter().enumerate() {
        let orig_body = bodies.get(i).map(String::as_str).unwrap_or("");
        let old_doc = old_docs.and_then(|arr| arr.get(i));
        match old_doc {
            Some(old_doc) if old_doc == new_doc && !orig_body.is_empty() => {
                out_docs.push(orig_body.to_string());
            }
            Some(old_doc) => {
                out_docs.push(serialize_single_yaml_document(orig_body, old_doc, new_doc)?);
            }
            None => {
                // Appended document: no original body to preserve.
                out_docs.push(serialize_value(new_doc, &FileFormat::Yaml)?);
            }
        }
    }

    Ok(join_multi_document_yaml(leading_marker, &out_docs))
}

// ---------------------------------------------------------------------------
// Pure data helpers (value parsing, flattening, diffing)
// ---------------------------------------------------------------------------

/// Parse a CLI value string into a [`serde_json::Value`].
///
/// Recognition order: JSON-quoted string, JSON object/array, boolean, null,
/// i64, f64, then fallback to bare string.
pub fn parse_value(s: &str) -> serde_json::Value {
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

/// Recursively enumerate all leaf selector paths in a JSON value.
///
/// Uses a mutable `String` buffer for the path prefix to avoid
/// allocating a new `String` via `format!()` at every recursion level.
/// The buffer is extended and truncated as the recursion descends and
/// ascends, so only leaf paths produce a final `String::clone()`.
/// Append a key to the path buffer, quoting it if it contains separator
/// characters (`.`, `[`, `]`) to prevent ambiguity in flattened paths.
fn push_key_quoted(buf: &mut String, k: &str) {
    if k.contains('.') || k.contains('[') || k.contains(']') || k.contains('"') {
        buf.push('"');
        buf.push_str(&k.replace('"', "\\\""));
        buf.push('"');
    } else {
        buf.push_str(k);
    }
}

pub fn flatten_value<'a>(
    value: &'a serde_json::Value,
    buf: &mut String,
    out: &mut Vec<(String, &'a serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(map) if !map.is_empty() => {
            for (k, v) in map {
                let restore = buf.len();
                if !buf.is_empty() {
                    buf.push('.');
                }
                push_key_quoted(buf, k);
                flatten_value(v, buf, out);
                buf.truncate(restore);
            }
        }
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            for (i, v) in arr.iter().enumerate() {
                let restore = buf.len();
                buf.push('[');
                let _ = std::fmt::Write::write_fmt(buf, format_args!("{i}"));
                buf.push(']');
                flatten_value(v, buf, out);
                buf.truncate(restore);
            }
        }
        _ => {
            out.push((buf.clone(), value));
        }
    }
}

/// Entry in a structured diff.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffEntry {
    pub path: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_value: Option<serde_json::Value>,
}

/// Compute structural differences between two JSON values.
///
/// Uses a mutable `String` buffer for the path prefix to avoid
/// allocating a new `String` via `format!()` at every recursion level.
pub fn diff_values(
    a: &serde_json::Value,
    b: &serde_json::Value,
    buf: &mut String,
    out: &mut Vec<DiffEntry>,
) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (k, va) in ma {
                let restore = buf.len();
                if !buf.is_empty() {
                    buf.push('.');
                }
                push_key_quoted(buf, k);
                if let Some(vb) = mb.get(k) {
                    diff_values(va, vb, buf, out);
                } else {
                    out.push(DiffEntry {
                        path: buf.clone(),
                        kind: "removed",
                        old_value: Some(va.clone()),
                        new_value: None,
                    });
                }
                buf.truncate(restore);
            }
            for (k, vb) in mb {
                if !ma.contains_key(k) {
                    let restore = buf.len();
                    if !buf.is_empty() {
                        buf.push('.');
                    }
                    push_key_quoted(buf, k);
                    out.push(DiffEntry {
                        path: buf.clone(),
                        kind: "added",
                        old_value: None,
                        new_value: Some(vb.clone()),
                    });
                    buf.truncate(restore);
                }
            }
        }
        (serde_json::Value::Array(aa), serde_json::Value::Array(ab)) => {
            let max_len = aa.len().max(ab.len());
            for i in 0..max_len {
                let restore = buf.len();
                buf.push('[');
                let _ = std::fmt::Write::write_fmt(buf, format_args!("{i}"));
                buf.push(']');
                match (aa.get(i), ab.get(i)) {
                    (Some(va), Some(vb)) => diff_values(va, vb, buf, out),
                    (Some(va), None) => out.push(DiffEntry {
                        path: buf.clone(),
                        kind: "removed",
                        old_value: Some(va.clone()),
                        new_value: None,
                    }),
                    (None, Some(vb)) => out.push(DiffEntry {
                        path: buf.clone(),
                        kind: "added",
                        old_value: None,
                        new_value: Some(vb.clone()),
                    }),
                    (None, None) => {}
                }
                buf.truncate(restore);
            }
        }
        _ => {
            if a != b {
                out.push(DiffEntry {
                    path: buf.clone(),
                    kind: "changed",
                    old_value: Some(a.clone()),
                    new_value: Some(b.clone()),
                });
            }
        }
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
    /// A delete / delete-where removed this many items (always >= 1).
    ///
    /// Callers that only care about "did anything change" can treat this like
    /// [`Applied`](Self::Applied). Callers that need counts (CLI/MCP JSON)
    /// use the payload for `removed`.
    Removed(usize),
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
                Ok(MutationResult::Removed(1))
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
                Ok(MutationResult::Removed(removed))
            }
        }
    }
}

#[cfg(test)]
mod tests;
