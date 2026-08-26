pub(crate) fn apply_yaml_mapping_diff(
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
                    // Preserve quote style when updating a scalar string.
                    if let Some(new_str) = new_val.as_str()
                        && let Some(existing) = mapping.get(key.as_str())
                        && let Some(scalar) = existing.as_scalar()
                        && scalar.is_quoted()
                    {
                        let raw = scalar.value();
                        let quote_char = raw.chars().next().unwrap_or('"');
                        let quoted = if quote_char == '\'' {
                            // Single-quoted: escape internal single quotes as ''
                            format!("'{}'", new_str.replace('\'', "''"))
                        } else {
                            // Double-quoted: escape internal backslashes and double quotes
                            format!("\"{}\"", new_str.replace('\\', "\\\\").replace('"', "\\\""))
                        };
                        scalar.set_value(&quoted);
                    } else {
                        mapping.set(key.as_str(), json_to_yaml_node(new_val)?);
                    }
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
pub(super) fn apply_yaml_sequence_diff(
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
pub(super) fn try_remove_subsequence(
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

/// Rewrite `key: *alias` (or `- *alias`) lines so interior object edits keep
/// identity. yaml-edit `Mapping::set` on an inline alias either inlines
/// invalid YAML or drops sibling anchors, so this is a line splice.
///
/// Key-superset edits become `<<: *name` plus local keys. Deleting an
/// inherited key, or a non-superset replace, writes a concrete map.
pub(crate) fn rewrite_yaml_alias_object_edits(
    text: &str,
    file: &yaml_edit::YamlFile,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    let Some(doc) = file.document() else {
        return Ok(None);
    };
    let mut rewrites = Vec::new();
    let mut seq_alias_seen = std::collections::HashMap::new();
    let mut map_alias_seen = std::collections::HashMap::new();
    if let Some(mapping) = doc.as_mapping() {
        collect_mapping_alias_rewrites(
            &mapping,
            old,
            new,
            &mut seq_alias_seen,
            &mut map_alias_seen,
            &mut rewrites,
        )?;
    } else if let Some(seq) = doc.as_sequence()
        && let (Some(old_arr), Some(new_arr)) = (old.as_array(), new.as_array())
    {
        collect_sequence_alias_rewrites(
            &seq,
            old_arr,
            new_arr,
            &mut seq_alias_seen,
            &mut map_alias_seen,
            &mut rewrites,
        )?;
    }
    if rewrites.is_empty() {
        return Ok(None);
    }
    apply_alias_line_rewrites(text, &rewrites)
}

fn collect_mapping_alias_rewrites(
    mapping: &yaml_edit::Mapping,
    old: &serde_json::Value,
    new: &serde_json::Value,
    seq_alias_seen: &mut std::collections::HashMap<String, usize>,
    map_alias_seen: &mut std::collections::HashMap<(String, String), usize>,
    out: &mut Vec<AliasLineRewrite>,
) -> anyhow::Result<()> {
    let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
        return Ok(());
    };
    for (key, new_val) in new_map {
        let Some(old_val) = old_map.get(key) else {
            continue;
        };
        // Walk every child sequence so occurrence indexes match file-wide
        // `- *alias` hits, including lists under unchanged sibling keys.
        if let (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr)) =
            (old_val, new_val)
            && let Some(seq) = mapping.get_sequence(key.as_str())
        {
            collect_sequence_alias_rewrites(
                &seq,
                old_arr,
                new_arr,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
        }
        if let (serde_json::Value::Object(_), serde_json::Value::Object(_)) = (old_val, new_val)
            && let Some(child) = mapping.get_mapping(key.as_str())
        {
            collect_mapping_alias_rewrites(
                &child,
                old_val,
                new_val,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
            continue;
        }
        if let Some(node) = mapping.get(key.as_str())
            && let Some(alias) = node.as_alias()
        {
            // Count every `key: *alias` so nth indexes match file-wide hits,
            // including unchanged siblings (`cfg: *shared` under two list items).
            let name = alias.name();
            let occ = map_alias_seen
                .entry((key.clone(), name.clone()))
                .or_insert(0);
            let this = *occ;
            *occ += 1;
            if old_val != new_val
                && let Some(rewrite) =
                    alias_object_rewrite(Some(node), Some(key), old_val, new_val, Some(this))?
            {
                out.push(rewrite);
            }
            continue;
        }
        if old_val == new_val {
            continue;
        }
        if let (serde_json::Value::Object(_), serde_json::Value::Object(_)) = (old_val, new_val)
            && let Some(rewrite) =
                alias_object_rewrite(mapping.get(key.as_str()), Some(key), old_val, new_val, None)?
        {
            out.push(rewrite);
        }
    }
    Ok(())
}

fn collect_sequence_alias_rewrites(
    seq: &yaml_edit::Sequence,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
    seq_alias_seen: &mut std::collections::HashMap<String, usize>,
    map_alias_seen: &mut std::collections::HashMap<(String, String), usize>,
    out: &mut Vec<AliasLineRewrite>,
) -> anyhow::Result<()> {
    for (i, old_val) in old_arr.iter().enumerate() {
        let Some(node) = seq.get(i) else {
            continue;
        };
        if let Some(alias) = node.as_alias() {
            let name = alias.name();
            let occ = seq_alias_seen.entry(name.clone()).or_insert(0);
            let this = *occ;
            *occ += 1;
            let Some(new_val) = new_arr.get(i) else {
                continue;
            };
            if old_val == new_val {
                continue;
            }
            if let Some(rewrite) =
                alias_object_rewrite(Some(node), None, old_val, new_val, Some(this))?
            {
                out.push(rewrite);
            }
        } else if let Some(child) = node.as_mapping()
            && let (Some(new_val), serde_json::Value::Object(_)) = (new_arr.get(i), old_val)
            && new_val.is_object()
        {
            collect_mapping_alias_rewrites(
                child,
                old_val,
                new_val,
                seq_alias_seen,
                map_alias_seen,
                out,
            )?;
        }
    }
    Ok(())
}

fn alias_object_rewrite(
    node: Option<yaml_edit::YamlNode>,
    key: Option<&str>,
    old: &serde_json::Value,
    new: &serde_json::Value,
    seq_occurrence: Option<usize>,
) -> anyhow::Result<Option<AliasLineRewrite>> {
    let Some(alias) = node.as_ref().and_then(yaml_edit::YamlNode::as_alias) else {
        return Ok(None);
    };
    let name = alias.name();
    if !is_safe_yaml_plain_ident(&name) {
        return Ok(None);
    }
    if let Some(key) = key
        && !is_safe_yaml_plain_ident(key)
    {
        return Ok(None);
    }
    let Some(new_map) = new.as_object() else {
        return Ok(None);
    };
    let merge = old.as_object().is_some_and(|old_map| {
        old_map.keys().all(|k| new_map.contains_key(k))
            && new_map.iter().any(|(k, v)| old_map.get(k) != Some(v))
    });
    let body = if merge {
        let mut overrides = serde_json::Map::new();
        if let Some(old_map) = old.as_object() {
            for (k, v) in new_map {
                if old_map.get(k) != Some(v) {
                    overrides.insert(k.clone(), v.clone());
                }
            }
        }
        format_alias_block_body(Some(&name), &overrides)?
    } else {
        format_alias_block_body(None, new_map)?
    };
    Ok(Some(AliasLineRewrite {
        key: key.map(str::to_string),
        alias: name,
        body,
        seq_occurrence,
    }))
}

struct AliasLineRewrite {
    key: Option<String>,
    alias: String,
    body: String,
    seq_occurrence: Option<usize>,
}

/// Anchor names and mapping keys interpolated into the alias-line regex
/// must be unquoted YAML plain identifiers (no `:`, spaces, or quotes).
fn is_safe_yaml_plain_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn format_alias_block_body(
    merge_alias: Option<&str>,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let mut body = String::new();
    if let Some(name) = merge_alias {
        body.push_str("<<: *");
        body.push_str(name);
        body.push('\n');
    }
    if fields.is_empty() {
        if merge_alias.is_none() {
            body.push_str("{}\n");
        }
        return Ok(body);
    }
    let yaml =
        serde_yaml_ng::to_string(&serde_json::Value::Object(fields.clone())).map_err(|e| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("YAML serialization failed: {e}"),
            })
        })?;
    for line in yaml.lines() {
        let line = line.trim_end();
        if line.is_empty() || line == "---" {
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    Ok(body)
}

fn apply_alias_line_rewrites(
    text: &str,
    rewrites: &[AliasLineRewrite],
) -> anyhow::Result<Option<String>> {
    let mut ordered: Vec<&AliasLineRewrite> = rewrites.iter().collect();
    // Later occurrences first so earlier `key: *alias` / `- *alias` indexes
    // stay valid after a splice.
    ordered.sort_by(|a, b| {
        match (
            a.key.as_deref(),
            b.key.as_deref(),
            a.seq_occurrence,
            b.seq_occurrence,
        ) {
            (Some(ka), Some(kb), Some(x), Some(y)) if ka == kb && a.alias == b.alias => y.cmp(&x),
            (None, None, Some(x), Some(y)) if a.alias == b.alias => y.cmp(&x),
            _ => std::cmp::Ordering::Equal,
        }
    });
    let mut out = text.to_string();
    for rewrite in ordered {
        let Some(next) = replace_unique_alias_line(&out, rewrite) else {
            return Ok(None);
        };
        out = next;
    }
    Ok(Some(out))
}

fn replace_unique_alias_line(text: &str, rewrite: &AliasLineRewrite) -> Option<String> {
    let alias_re = regex::escape(&rewrite.alias);
    if let Some(key) = rewrite.key.as_deref() {
        let key_re = regex::escape(key);
        let mapping_re = regex::Regex::new(&format!(
            r"(?m)^([ \t]*){key_re}:[ \t]*\*{alias_re}([ \t]*(?:#.*)?)?[ \t]*\r?$"
        ))
        .ok()?;
        let hits: Vec<regex::Match<'_>> = mapping_re.find_iter(text).collect();
        let idx = match rewrite.seq_occurrence {
            Some(i) => i,
            None if hits.len() == 1 => 0,
            None => return None,
        };
        let hit = hits.get(idx)?;
        let caps = mapping_re.captures(hit.as_str())?;
        let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let comment = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let replacement = with_file_eol(
            text,
            &format_block_replacement(indent, &format!("{key}:{comment}"), &rewrite.body),
        );
        return Some(splice_match(text, *hit, &replacement));
    }

    let seq_re = regex::Regex::new(&format!(
        r"(?m)^([ \t]*)-[ \t]*\*{alias_re}([ \t]*(?:#.*)?)?[ \t]*\r?$"
    ))
    .ok()?;
    let hits: Vec<regex::Match<'_>> = seq_re.find_iter(text).collect();
    let idx = rewrite.seq_occurrence?;
    let hit = hits.get(idx)?;
    let caps = seq_re.captures(hit.as_str())?;
    let indent = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let comment = caps.get(2).map(|m| m.as_str()).unwrap_or("");
    let first = rewrite.body.lines().next().unwrap_or("");
    let rest = rewrite
        .body
        .lines()
        .skip(1)
        .map(|line| format!("{indent}  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut replacement = format!("{indent}- {first}{comment}");
    if !rest.is_empty() {
        replacement.push('\n');
        replacement.push_str(&rest);
    }
    Some(splice_match(text, *hit, &with_file_eol(text, &replacement)))
}

fn with_file_eol(file: &str, block: &str) -> String {
    if file.contains("\r\n") {
        block.replace('\n', "\r\n")
    } else {
        block.to_string()
    }
}

fn splice_match(text: &str, m: regex::Match<'_>, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..m.start()]);
    out.push_str(replacement);
    out.push_str(&text[m.end()..]);
    out
}

fn format_block_replacement(indent: &str, header: &str, body: &str) -> String {
    let mut out = format!("{indent}{header}\n");
    for line in body.lines() {
        out.push_str(indent);
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    // replace() inserts this in place of one line; drop the trailing newline
    // so we do not double the original line ending.
    out.pop();
    out
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
    let yaml_text = serde_yaml_ng::to_string(&wrapper).map_err(|e| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("YAML serialization failed: {e}"),
        })
    })?;
    let doc = yaml_edit::Document::from_str(&yaml_text).map_err(|e| {
        anyhow::Error::new(crate::exit::ParseErrorError {
            msg: format!("YAML CST re-parse failed: {e}"),
        })
    })?;
    doc.as_mapping()
        .and_then(|m| m.get("__v__"))
        .ok_or_else(|| {
            anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "YAML CST wrapper key missing".into(),
            })
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: parse YAML text into a `yaml_edit::Document`, extract its root mapping.
    fn parse_yaml(text: &str) -> yaml_edit::Document {
        use std::str::FromStr;
        yaml_edit::Document::from_str(text).unwrap()
    }

    /// Round-trip helper: apply a mapping diff and serialize back.
    fn apply_and_serialize(yaml: &str, old: &serde_json::Value, new: &serde_json::Value) -> String {
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        apply_yaml_mapping_diff(&mapping, old, new).unwrap();
        doc.to_string()
    }

    // ---- json_to_yaml_node ----

    /// Helper: insert a json_to_yaml_node result into a pre-existing doc
    /// and return the serialized text.
    fn set_and_render(key: &str, val: &serde_json::Value) -> String {
        let doc = parse_yaml(&format!("{key}: placeholder\n"));
        let mapping = doc.as_mapping().unwrap();
        let node = json_to_yaml_node(val).unwrap();
        mapping.set(key, node);
        doc.to_string()
    }

    #[test]
    fn json_to_yaml_node_scalar() {
        let text = set_and_render("key", &json!("hello"));
        assert!(text.contains("key: hello"), "expected 'key: hello': {text}");
    }

    #[test]
    fn json_to_yaml_node_number() {
        let text = set_and_render("val", &json!(42));
        assert!(text.contains("val: 42"), "expected 'val: 42': {text}");
    }

    #[test]
    fn json_to_yaml_node_boolean() {
        let text = set_and_render("flag", &json!(true));
        assert!(text.contains("flag: true"), "expected 'flag: true': {text}");
    }

    #[test]
    fn json_to_yaml_node_array() {
        let text = set_and_render("list", &json!(["a", "b", "c"]));
        assert!(text.contains("- a"));
        assert!(text.contains("- b"));
        assert!(text.contains("- c"));
    }

    #[test]
    fn json_to_yaml_node_nested_object() {
        let text = set_and_render("outer", &json!({"inner": "value"}));
        assert!(text.contains("inner: value"));
    }

    // ---- json_to_yaml_mapping ----

    #[test]
    fn json_to_yaml_mapping_basic() {
        // json_to_yaml_mapping creates a populated Mapping. Verify by
        // setting it as a nested value and checking the rendered YAML.
        let val = json!({"alpha": 1, "beta": "two"});
        let mapping_node = json_to_yaml_mapping(&val).unwrap();
        let doc = parse_yaml("outer: placeholder\n");
        let root = doc.as_mapping().unwrap();
        root.set("outer", &mapping_node);
        let text = doc.to_string();
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
    }

    #[test]
    fn json_to_yaml_mapping_non_object_is_empty() {
        let val = json!("not an object");
        let mapping = json_to_yaml_mapping(&val).unwrap();
        // Non-object: returns empty mapping (no keys set).
        assert!(mapping.get("anything").is_none());
    }

    // ---- apply_yaml_mapping_diff ----

    #[test]
    fn mapping_diff_no_change() {
        let yaml = "key: value\n";
        let old = json!({"key": "value"});
        let new = json!({"key": "value"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert_eq!(result, yaml);
    }

    /// yaml-edit 0.3 still cannot turn `key: *alias` into `<<: *alias` via
    /// `Mapping::set`. A CST set of the expanded object inlines the map and
    /// can drop the sibling `&shared` definition. Keep
    /// [`rewrite_yaml_alias_object_edits`] (#2250).
    #[test]
    fn yaml_edit_set_on_pure_alias_does_not_become_merge() {
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let doc = parse_yaml(yaml);
        let root = doc.as_mapping().unwrap();
        let replacement = json_to_yaml_mapping(&json!({"timeout": 60, "retries": 3})).unwrap();
        root.set("service_a", &replacement);
        let result = doc.to_string();
        assert!(
            !result.contains("<<: *shared"),
            "0.3 Mapping::set unexpectedly produced a merge:\n{result}"
        );
        assert!(
            !result.contains("service_a: *shared"),
            "0.3 Mapping::set left the alias (would drop the override):\n{result}"
        );
        // Concrete map is fine; identity is not. The line splice is still required.
        assert!(
            result.contains("timeout: 60"),
            "0.3 Mapping::set must still write the override:\n{result}"
        );
    }

    #[test]
    fn mapping_diff_pure_alias_override_to_merge() {
        use std::str::FromStr;
        let yaml = "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\n";
        let old = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 30, "retries": 3}});
        let new = json!({"shared": {"timeout": 30, "retries": 3}, "service_a": {"timeout": 60, "retries": 3}});
        let file = yaml_edit::YamlFile::from_str(yaml).unwrap();
        let result = rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
            .unwrap()
            .expect("alias line should be rewritten");
        assert!(
            result.contains("&shared") && result.contains("<<: *shared"),
            "after alias override:\n{result}"
        );
        assert!(
            result.contains("timeout: 60"),
            "local override missing:\n{result}"
        );
        assert!(
            !result.contains("service_a: <<:"),
            "merge must be a block mapping, not inlined:\n{result}"
        );
    }

    #[test]
    fn mapping_diff_scalar_update() {
        let yaml = "name: old\n";
        let old = json!({"name": "old"});
        let new = json!({"name": "new"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("name: new"));
    }

    #[test]
    fn mapping_diff_add_key() {
        let yaml = "existing: yes\n";
        let old = json!({"existing": "yes"});
        let new = json!({"existing": "yes", "added": "here"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("existing: yes"));
        assert!(result.contains("added: here"));
    }

    #[test]
    fn mapping_diff_remove_key() {
        let yaml = "keep: yes\nremove: me\n";
        let old = json!({"keep": "yes", "remove": "me"});
        let new = json!({"keep": "yes"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("keep: yes"));
        assert!(!result.contains("remove"));
    }

    #[test]
    fn mapping_diff_nested_object_update() {
        let yaml = "parent:\n  child: old\n";
        let old = json!({"parent": {"child": "old"}});
        let new = json!({"parent": {"child": "new"}});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("child: new"));
    }

    #[test]
    fn mapping_diff_type_change() {
        let yaml = "key: scalar\n";
        let old = json!({"key": "scalar"});
        let new = json!({"key": ["array", "now"]});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(result.contains("- array"));
    }

    // ---- try_remove_subsequence ----

    #[test]
    fn try_remove_subsequence_simple_tail() {
        let yaml = "items:\n  - a\n  - b\n  - c\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b"), json!("c")];
        let new = vec![json!("a"), json!("b")];
        assert!(try_remove_subsequence(&seq, &old, &new));

        let result = doc.to_string();
        assert!(result.contains("- a"));
        assert!(result.contains("- b"));
        assert!(!result.contains("- c"));
    }

    #[test]
    fn try_remove_subsequence_middle() {
        let yaml = "items:\n  - x\n  - y\n  - z\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("x"), json!("y"), json!("z")];
        let new = vec![json!("x"), json!("z")];
        assert!(try_remove_subsequence(&seq, &old, &new));

        let result = doc.to_string();
        assert!(result.contains("- x"));
        assert!(!result.contains("- y"));
        assert!(result.contains("- z"));
    }

    #[test]
    fn try_remove_subsequence_not_subsequence() {
        let yaml = "items:\n  - a\n  - b\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b")];
        let new = vec![json!("c")]; // "c" not in old
        assert!(!try_remove_subsequence(&seq, &old, &new));
    }

    // ---- apply_yaml_sequence_diff ----

    #[test]
    fn sequence_diff_scalar_update() {
        let yaml = "list:\n  - one\n  - two\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("list").unwrap();

        let old = vec![json!("one"), json!("two")];
        let new = vec![json!("ONE"), json!("two")];
        assert!(apply_yaml_sequence_diff(&seq, &old, &new).unwrap());

        let result = doc.to_string();
        assert!(result.contains("ONE"));
        assert!(result.contains("two"));
    }

    #[test]
    fn sequence_diff_no_change() {
        let yaml = "list:\n  - a\n  - b\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("list").unwrap();

        let old = vec![json!("a"), json!("b")];
        let new = vec![json!("a"), json!("b")];
        assert!(apply_yaml_sequence_diff(&seq, &old, &new).unwrap());
    }

    // ---- apply_yaml_sequence_resize ----

    #[test]
    fn sequence_resize_shrink() {
        let yaml = "items:\n  - a\n  - b\n  - c\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a"), json!("b"), json!("c")];
        let new = vec![json!("a"), json!("c")];
        let new_val = json!(["a", "c"]);
        assert!(apply_yaml_sequence_resize(
            &seq, &old, &new, &mapping, "items", &new_val
        ));
    }

    #[test]
    fn sequence_resize_grow_returns_false() {
        let yaml = "items:\n  - a\n";
        let doc = parse_yaml(yaml);
        let mapping = doc.as_mapping().unwrap();
        let seq = mapping.get_sequence("items").unwrap();

        let old = vec![json!("a")];
        let new = vec![json!("a"), json!("b")];
        let new_val = json!(["a", "b"]);
        // Growth is not handled by CST path, returns false.
        assert!(!apply_yaml_sequence_resize(
            &seq, &old, &new, &mapping, "items", &new_val
        ));
    }

    #[test]
    fn mapping_diff_remove_first_nested_preserves_indentation() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  enabled: \"true\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "enabled": "true", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("name");

        let result = apply_and_serialize(yaml, &old, &new);
        assert!(!result.contains("name"));
    }

    #[test]
    fn mapping_diff_remove_middle_nested() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("version");

        let result = apply_and_serialize(yaml, &old, &new);
        assert!(!result.contains("version"));
        // CST preserves quotes on untouched values (indentation may be
        // wrong; fixed by fix_yaml_block_indentation in the caller).
        assert!(result.contains("\"my-app\""));
        assert!(result.contains("\"8080\""));
    }

    #[test]
    fn scalar_set_preserves_double_quote_style() {
        let yaml = "name: \"John Doe\"\nage: 30\n";
        let old = json!({"name": "John Doe", "age": 30});
        let new = json!({"name": "Jane Doe", "age": 30});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("\"Jane Doe\""),
            "double quotes not preserved: {result}"
        );
    }

    #[test]
    fn scalar_set_preserves_single_quote_style() {
        let yaml = "path: '/usr/local'\nname: plain\n";
        let old = json!({"path": "/usr/local", "name": "plain"});
        let new = json!({"path": "/opt/bin", "name": "plain"});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("'/opt/bin'"),
            "single quotes not preserved: {result}"
        );
    }

    #[test]
    fn scalar_set_preserves_plain_style() {
        let yaml = "name: Alice\nage: 30\n";
        let old = json!({"name": "Alice", "age": 30});
        let new = json!({"name": "Bob", "age": 30});
        let result = apply_and_serialize(yaml, &old, &new);
        assert!(
            result.contains("name: Bob"),
            "plain style should not add quotes: {result}"
        );
        assert!(
            !result.contains("\"Bob\"") && !result.contains("'Bob'"),
            "plain value should not get quoted: {result}"
        );
    }
}
