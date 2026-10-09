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
            } else if let Some(inline) = item.as_inline_table_mut() {
                // Edit inline table in-place to preserve inline formatting.
                let removed: Vec<String> = old_map
                    .keys()
                    .filter(|k| !new_map.contains_key(k.as_str()))
                    .cloned()
                    .collect();
                for k in &removed {
                    inline.remove(k);
                }
                for (key, new_val) in new_map {
                    if let Some(old_val) = old_map.get(key) {
                        if old_val != new_val {
                            inline.insert(key, json_to_toml_value(new_val));
                        }
                    } else {
                        inline.insert(key, json_to_toml_value(new_val));
                    }
                }
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
            } else if item.as_array_of_tables().is_some() && new_arr.iter().any(|n| !n.is_object())
            {
                // An element that is no longer a table cannot stay in an
                // array-of-tables. Replace the whole value.
                *item = json_to_toml_item(new);
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

/// Marker wrapped around a JSON integer that does not fit in TOML's i64.
/// `toml_edit` rejects that integer and would otherwise store an f64.
/// [`restore_oversize_toml_integers`] writes the digits back as a bare integer.
/// [`prepare_oversize_toml_source`] quotes the same tokens so a later read
/// can parse the file, then [`unwrap_oversize_toml_integers`] removes the marker.
const OVERSIZE_INT_MARK: &str = "__patchloom_oversize_int:";

fn json_number_to_toml(n: &serde_json::Number) -> toml_edit::Value {
    if let Some(i) = n.as_i64() {
        return toml_edit::Value::from(i);
    }
    if n.as_u64().is_some() {
        return toml_edit::Value::from(format!("{OVERSIZE_INT_MARK}{n}"));
    }
    toml_edit::Value::from(n.as_f64().unwrap_or(0.0))
}

/// Quote bare integers `toml_edit` rejects so the document can be parsed.
///
/// Only an `integer number overflowed` span that is all ASCII digits is
/// rewritten, and only once per digit run. Any other parse error is left
/// for the caller.
pub(super) fn prepare_oversize_toml_source(
    content: &str,
) -> Result<std::borrow::Cow<'_, str>, toml_edit::TomlError> {
    let mut rewritten: Option<String> = None;
    let limit = ascii_digit_runs(content).saturating_add(1);
    for _ in 0..limit {
        let current = rewritten.as_deref().unwrap_or(content);
        match current.parse::<toml_edit::DocumentMut>() {
            Ok(_) => {
                return Ok(match rewritten {
                    Some(text) => std::borrow::Cow::Owned(text),
                    None => std::borrow::Cow::Borrowed(content),
                });
            }
            Err(err) => {
                let Some(next) = rewrite_integer_overflow(current, err.message(), err.span())
                else {
                    return Err(err);
                };
                rewritten = Some(next);
            }
        }
    }
    Ok(std::borrow::Cow::Owned(
        rewritten.unwrap_or_else(|| content.to_string()),
    ))
}

fn ascii_digit_runs(content: &str) -> usize {
    let mut runs = 0usize;
    let mut in_run = false;
    for byte in content.bytes() {
        let digit = byte.is_ascii_digit();
        if digit && !in_run {
            runs += 1;
        }
        in_run = digit;
    }
    runs
}

fn rewrite_integer_overflow(
    text: &str,
    message: &str,
    span: Option<std::ops::Range<usize>>,
) -> Option<String> {
    if !message.contains("integer number overflowed") {
        return None;
    }
    let span = span?;
    if span.start > span.end
        || span.end > text.len()
        || !text.is_char_boundary(span.start)
        || !text.is_char_boundary(span.end)
    {
        return None;
    }
    let token = &text[span.start..span.end];
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + OVERSIZE_INT_MARK.len() + 2);
    out.push_str(&text[..span.start]);
    out.push('"');
    out.push_str(OVERSIZE_INT_MARK);
    out.push_str(token);
    out.push('"');
    out.push_str(&text[span.end..]);
    Some(out)
}

/// Replace oversize-integer markers with a `u64` number, or with the decimal
/// text when the digits do not fit in `u64`.
pub(super) fn unwrap_oversize_toml_integers(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::String(text) => {
            let Some(digits) = text.strip_prefix(OVERSIZE_INT_MARK) else {
                return;
            };
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return;
            }
            if let Ok(n) = digits.parse::<u64>() {
                *val = serde_json::Value::Number(n.into());
            } else {
                *val = serde_json::Value::String(digits.to_string());
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                unwrap_oversize_toml_integers(child);
            }
        }
        serde_json::Value::Object(map) => {
            for child in map.values_mut() {
                unwrap_oversize_toml_integers(child);
            }
        }
        _ => {}
    }
}

pub(super) fn restore_oversize_toml_integers(text: &str) -> String {
    let needle = format!("\"{OVERSIZE_INT_MARK}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(&needle) {
        out.push_str(&rest[..start]);
        let after = &rest[start + needle.len()..];
        let Some(end) = after.find('"') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let digits = &after[..end];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            out.push_str(digits);
            rest = &after[end + 1..];
        } else {
            out.push_str(&needle);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Convert a `serde_json::Value` to a `toml_edit::Value` (scalar/array/inline-table).
fn json_to_toml_value(val: &serde_json::Value) -> toml_edit::Value {
    match val {
        serde_json::Value::String(s) => {
            if let Ok(dt) = s.parse::<toml_edit::Datetime>() {
                toml_edit::Value::from(dt)
            } else {
                toml_edit::Value::from(s.as_str())
            }
        }
        serde_json::Value::Bool(b) => toml_edit::Value::from(*b),
        serde_json::Value::Number(n) => json_number_to_toml(n),
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
            // TOML has no null. Empty string is the write mapping; do not
            // eprint (CLI --json / library / MCP must not leak a warning).
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
        assert!(
            !result.contains("b ="),
            "removed key should be gone: {result}"
        );
        assert!(result.contains("a = 1"));
    }

    #[test]
    fn apply_value_diff_adds_new_key() {
        let mut doc = parse_toml("a = 1\n");
        let old = json(r#"{"a": 1}"#);
        let new = json(r#"{"a": 1, "c": 3}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            result.contains("c = 3"),
            "new key 'c = 3' should appear: {result}"
        );
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
    fn json_integer_above_i64_max_stays_a_toml_integer() {
        let old = json(r#"{"n": 1}"#);
        let new = json(r#"{"n": 9223372036854775809}"#);
        let result = crate::ops::doc::serialize_value_preserving(
            "n = 1\n",
            &old,
            &new,
            &crate::ops::doc::FileFormat::Toml,
        )
        .unwrap();
        assert!(
            result.contains("9223372036854775809"),
            "decimal text must survive: {result}"
        );
        assert!(
            !result.contains("9223372036854775808")
                && !result.contains('.')
                && !result.contains(OVERSIZE_INT_MARK),
            "must not round to f64: {result}"
        );
    }

    #[test]
    fn array_of_tables_element_replaced_when_it_stops_being_a_table() {
        let mut doc = parse_toml("[[item]]\nname = \"a\"\n[[item]]\nname = \"b\"\n");
        let old = json(r#"{"item":[{"name":"a"},{"name":"b"}]}"#);
        let new = json(r#"{"item":["gone",{"name":"b"}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("gone"), "{result}");
        assert!(
            !result.contains("name = \"a\""),
            "old table must not stay: {result}"
        );
    }

    #[test]
    fn apply_value_diff_same_length_array() {
        let mut doc = parse_toml("arr = [1, 2, 3]\n");
        let old = json(r#"{"arr": [1, 2, 3]}"#);
        let new = json(r#"{"arr": [1, 99, 3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            result.contains(", 99,") || result.contains("[99,") || result.contains(", 99]"),
            "array element 99 should appear in array context: {result}"
        );
        assert!(
            !result.contains(", 2, 3]"),
            "old array layout should not persist: {result}"
        );
    }

    #[test]
    fn apply_value_diff_different_length_array_replaces() {
        let mut doc = parse_toml("arr = [1, 2]\n");
        let old = json(r#"{"arr": [1, 2]}"#);
        let new = json(r#"{"arr": [1, 2, 3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            result.contains("1, 2, 3"),
            "extended array [1, 2, 3] should appear: {result}"
        );
    }

    #[test]
    fn apply_value_diff_preserves_inline_table_format() {
        let original = "options = { debug = true, verbose = false }\n";
        let mut doc = parse_toml(original);
        let old = json(r#"{"options": {"debug": true, "verbose": false}}"#);
        let new = json(r#"{"options": {"debug": false, "verbose": false}}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        // Must stay as inline table, not expand to [options] section
        assert!(
            !result.contains("[options]"),
            "inline table must not expand to section: {result}"
        );
        assert!(
            result.contains("debug = false"),
            "debug should be updated: {result}"
        );
        assert!(
            result.contains("verbose = false"),
            "verbose should be preserved: {result}"
        );
    }

    #[test]
    fn apply_value_diff_inline_table_add_and_remove_keys() {
        let original = "opts = { a = 1, b = 2 }\n";
        let mut doc = parse_toml(original);
        let old = json(r#"{"opts": {"a": 1, "b": 2}}"#);
        let new = json(r#"{"opts": {"a": 1, "c": 3}}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            !result.contains("[opts]"),
            "inline table must not expand: {result}"
        );
        assert!(
            !result.contains("b ="),
            "removed key 'b' should be gone: {result}"
        );
        assert!(
            result.contains("c = 3"),
            "new key 'c' should appear: {result}"
        );
    }

    #[test]
    fn json_to_toml_value_null_maps_to_empty_string() {
        let val = json_to_toml_value(&serde_json::Value::Null);
        assert_eq!(val.as_str(), Some(""), "null should map to empty string");
    }

    #[test]
    fn json_to_toml_value_datetime_string_emits_datetime() {
        let val = json_to_toml_value(&serde_json::json!("2020-01-01T00:00:00Z"));
        assert!(
            val.is_datetime(),
            "RFC3339/TOML datetime string must emit Datetime, got {val}"
        );
        assert_eq!(
            val.as_datetime().unwrap().to_string(),
            "2020-01-01T00:00:00Z"
        );
    }

    #[test]
    fn json_to_toml_value_plain_string_stays_string() {
        let val = json_to_toml_value(&serde_json::json!("hello"));
        assert_eq!(val.as_str(), Some("hello"));
        assert!(!val.is_datetime());
    }

    #[test]
    fn apply_value_diff_writes_unquoted_datetime() {
        let mut doc = parse_toml("when = 1979-05-27T07:32:00Z\n");
        let old = json(r#"{"when": "1979-05-27T07:32:00Z"}"#);
        let new = json(r#"{"when": "2020-01-01T00:00:00Z"}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(
            result.contains("when = 2020-01-01T00:00:00Z"),
            "datetime must be unquoted TOML: {result}"
        );
        assert!(
            !result.contains("\"2020-01-01T00:00:00Z\""),
            "datetime must not be a quoted string: {result}"
        );
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
