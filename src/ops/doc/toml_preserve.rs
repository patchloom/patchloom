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
