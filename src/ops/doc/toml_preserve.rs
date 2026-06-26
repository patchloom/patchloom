pub(crate) fn apply_value_diff(
    item: &mut toml_edit::Item,
    old: &serde_json::Value,
    new: &serde_json::Value,
) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_toml(s: &str) -> toml_edit::DocumentMut {
        s.parse::<toml_edit::DocumentMut>().unwrap()
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn apply_value_diff_updates_scalar() {
        let mut doc = parse_toml("key = \"old\"\n");
        let old = json(r#"{"key": "old"}"#);
        let new = json(r#"{"key": "new"}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            result.contains("\"new\""),
            "scalar should be updated: {result}"
        );
    }

    #[test]
    fn apply_value_diff_removes_deleted_key() {
        let mut doc = parse_toml("a = 1\nb = 2\n");
        let old = json(r#"{"a": 1, "b": 2}"#);
        let new = json(r#"{"a": 1}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(!result.contains("b ="), "removed key should be gone: {result}");
        assert!(result.contains("a = 1"));
    }

    #[test]
    fn apply_value_diff_adds_new_key() {
        let mut doc = parse_toml("a = 1\n");
        let old = json(r#"{"a": 1}"#);
        let new = json(r#"{"a": 1, "c": 3}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("c"), "new key should appear: {result}");
    }

    #[test]
    fn apply_value_diff_noop_on_equal() {
        let original = "key = \"same\"\n";
        let mut doc = parse_toml(original);
        let val = json(r#"{"key": "same"}"#);
        apply_value_diff(doc.as_item_mut(), &val, &val);
        assert_eq!(doc.to_string(), original);
    }

    #[test]
    fn apply_value_diff_same_length_array() {
        let mut doc = parse_toml("arr = [1, 2, 3]\n");
        let old = json(r#"{"arr": [1, 2, 3]}"#);
        let new = json(r#"{"arr": [1, 99, 3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("99"), "array element should be updated: {result}");
    }

    #[test]
    fn apply_value_diff_different_length_array_replaces() {
        let mut doc = parse_toml("arr = [1, 2]\n");
        let old = json(r#"{"arr": [1, 2]}"#);
        let new = json(r#"{"arr": [1, 2, 3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("3"), "longer array should be applied: {result}");
    }

    #[test]
    fn json_to_toml_value_handles_null() {
        let val = json_to_toml_value(&serde_json::Value::Null);
        assert_eq!(val.as_str(), Some(""), "null should map to empty string");
    }

    #[test]
    fn json_to_toml_item_object_becomes_table() {
        let item = json_to_toml_item(&json(r#"{"k": "v"}"#));
        assert!(item.is_table(), "object should become a table");
    }

    #[test]
    fn json_to_toml_item_array_of_objects_becomes_aot() {
        let item = json_to_toml_item(&json(r#"[{"a": 1}, {"b": 2}]"#));
        assert!(
            item.is_array_of_tables(),
            "array of objects should become array-of-tables"
        );
    }
}
