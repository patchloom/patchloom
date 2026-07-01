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
