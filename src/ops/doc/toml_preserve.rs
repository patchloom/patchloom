//! size-waiver: TOML comment-preserving value diff is one writer (policy #1408).

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
            let added: Vec<String> = new_map
                .keys()
                .filter(|k| !old_map.contains_key(k.as_str()))
                .cloned()
                .collect();
            // One removed key and one added key with the same value is a
            // rename (`doc move`). Removing then inserting drops the old
            // key's comments.
            let renamed = if removed.len() == 1 && added.len() == 1 {
                let old_key = removed[0].clone();
                let new_key = added[0].clone();
                if old_map.get(&old_key) == new_map.get(&new_key) {
                    Some((old_key, new_key))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((old_key, new_key)) = &renamed {
                let leaf = table.key_mut(old_key).map(|key| key.leaf_decor().clone());
                let value_decor = table
                    .get(old_key)
                    .and_then(|item| item.as_value())
                    .map(|value| value.decor().clone());
                if let Some(existing) = table.remove(old_key) {
                    table.insert(new_key, existing);
                    if let Some(leaf) = leaf
                        && let Some(mut key) = table.key_mut(new_key)
                    {
                        *key.leaf_decor_mut() = leaf;
                    }
                    if let Some(value_decor) = value_decor
                        && let Some(item) = table.get_mut(new_key)
                        && let Some(value) = item.as_value_mut()
                    {
                        *value.decor_mut() = value_decor;
                    }
                }
            }
            for key in &removed {
                if renamed.as_ref().is_some_and(|(old_key, _)| old_key == key) {
                    continue;
                }
                table.remove(key);
            }

            // Add new keys or recurse into changed values.
            for (key, new_val) in new_map {
                if renamed.as_ref().is_some_and(|(_, new_key)| new_key == key) {
                    continue;
                }
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
                        // Keep the element's prefix (newline and indent) and
                        // its trailing comment. A fresh value starts at `[9,`.
                        let decor = v.decor().clone();
                        let mut new_val = json_to_toml_value(n);
                        *new_val.decor_mut() = decor;
                        *v = new_val;
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

        (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr))
            if old_arr.len() != new_arr.len() =>
        {
            // A pure suffix or prefix keeps decorations on the existing
            // array. Wholesale replacement drops a tail comment.
            if !try_toml_extend_array(item, old_arr, new_arr)
                && !try_toml_remove_subsequence(item, old_arr, new_arr)
            {
                *item = json_to_toml_item(new);
            }
        }

        // Type change or scalar change: wholesale replacement.
        // A different-length array is handled above.
        // A scalar keeps the old value's comment. Replacing the item
        // drops `n = 1 # keep` down to `n = 9`.
        _ => {
            if !new.is_array()
                && !new.is_object()
                && let Some(decor) = item.as_value().map(|value| value.decor().clone())
            {
                let mut new_val = json_to_toml_value(new);
                *new_val.decor_mut() = decor;
                *item = toml_edit::Item::Value(new_val);
            } else {
                *item = json_to_toml_item(new);
            }
        }
    }
}

/// Grow a TOML array when `new_arr` is `old_arr` plus a suffix (append)
/// or a prefix (prepend). Other length changes return false.
fn try_toml_extend_array(
    item: &mut toml_edit::Item,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> bool {
    if new_arr.len() <= old_arr.len() {
        return false;
    }
    let extra = new_arr.len() - old_arr.len();
    let append = new_arr.get(..old_arr.len()) == Some(old_arr);
    let prepend = !append && new_arr.get(extra..) == Some(old_arr);
    if !append && !prepend {
        return false;
    }
    let extras = if append {
        &new_arr[old_arr.len()..]
    } else {
        &new_arr[..extra]
    };
    if let Some(arr) = item.as_array_mut() {
        if arr.len() != old_arr.len() {
            return false;
        }
        if append {
            for value in extras {
                push_toml_array_keeping_layout(arr, json_to_toml_value(value));
            }
        } else {
            for (i, value) in extras.iter().enumerate() {
                insert_toml_array_keeping_layout(arr, i, json_to_toml_value(value));
            }
        }
        return true;
    }
    if item.as_array_of_tables().is_some() {
        if item
            .as_array_of_tables()
            .is_none_or(|aot| aot.len() != old_arr.len())
            || extras.iter().any(|value| !value.is_object())
        {
            return false;
        }
        let mut tables = Vec::with_capacity(extras.len());
        for value in extras {
            match json_to_toml_item(value) {
                toml_edit::Item::Table(table) => tables.push(table),
                _ => return false,
            }
        }
        let Some(aot) = item.as_array_of_tables_mut() else {
            return false;
        };
        if append {
            for table in tables {
                aot.push(table);
            }
        } else {
            for (i, table) in tables.into_iter().enumerate() {
                aot.insert(i, table);
            }
        }
        return true;
    }
    false
}

/// Prefix to copy onto a new element of a multiline array.
///
/// A one-line array (`nums = [1] # tail`) has no newline inside the
/// brackets, so this returns `None` and the tail comment stays put.
fn multiline_array_item_prefix(arr: &toml_edit::Array) -> Option<String> {
    let mut clean: Option<String> = None;
    let mut saw_newline = false;
    let mut indent: Option<String> = None;
    for value in arr.iter() {
        let Some(prefix) = value.decor().prefix().and_then(|raw| raw.as_str()) else {
            continue;
        };
        if let Some(line) = prefix.rsplit(['\n', '\r']).next()
            && !line.is_empty()
            && line.chars().all(|c| c == ' ' || c == '\t')
        {
            indent = Some(line.to_string());
        }
        if prefix.contains('\n') {
            saw_newline = true;
            if !prefix.contains('#') {
                clean = Some(prefix.to_string());
            }
        }
    }
    if let Some(prefix) = clean {
        return Some(prefix);
    }
    let trailing_has_nl = arr
        .trailing()
        .as_str()
        .is_some_and(|trailing| trailing.contains('\n'));
    if saw_newline || trailing_has_nl {
        let indent = indent.unwrap_or_else(|| "  ".to_string());
        return Some(format!("\n{indent}"));
    }
    None
}

/// `}, # note\n` stores `# note` in the array trailing, on the last
/// element's line. A trailing that starts with a newline is a comment
/// above `]`, not a note on that element.
fn split_same_line_array_trailing(trailing: &str) -> Option<(&str, &str)> {
    if trailing.starts_with(['\n', '\r']) {
        return None;
    }
    let hash = trailing.find('#')?;
    let before = &trailing[..hash];
    if before.contains('\n') || before.contains('\r') {
        return None;
    }
    if let Some(rel) = trailing[hash..].find(['\n', '\r']) {
        let cut = hash + rel;
        Some((&trailing[..cut], &trailing[cut..]))
    } else {
        Some((trailing, ""))
    }
}

/// A comment with no comma (`} # primary`) lives on the value suffix.
/// Leaving it there puts the next element's comma on the following line.
fn take_same_line_suffix_comment(arr: &mut toml_edit::Array) -> Option<String> {
    let last = arr.len().checked_sub(1)?;
    let value = arr.get_mut(last)?;
    let suffix = value
        .decor()
        .suffix()
        .and_then(|raw| raw.as_str())
        .unwrap_or("")
        .to_string();
    let (comment, rest) = split_same_line_array_trailing(&suffix)?;
    let comment = comment.to_string();
    let new_suffix = if rest.trim_matches(['\n', '\r', ' ', '\t']).is_empty() {
        String::new()
    } else {
        rest.to_string()
    };
    value.decor_mut().set_suffix(new_suffix);
    Some(comment)
}

fn push_toml_array_keeping_layout(arr: &mut toml_edit::Array, mut value: toml_edit::Value) {
    let Some(item_prefix) = multiline_array_item_prefix(arr) else {
        arr.push_formatted(value);
        return;
    };
    let suffix_comment = take_same_line_suffix_comment(arr);
    let trailing = arr.trailing().as_str().unwrap_or("").to_string();
    let mut prefix = String::new();
    if let Some(comment) = suffix_comment {
        prefix.push_str(&comment);
    }
    if let Some((comment, rest)) = split_same_line_array_trailing(&trailing) {
        prefix.push_str(comment);
        let new_trailing = if rest.is_empty() {
            "\n".to_string()
        } else {
            rest.to_string()
        };
        arr.set_trailing(new_trailing);
    } else if !trailing.contains('\n') {
        arr.set_trailing("\n");
    }
    prefix.push_str(&item_prefix);
    value.decor_mut().set_prefix(prefix);
    arr.push_formatted(value);
}

fn insert_toml_array_keeping_layout(
    arr: &mut toml_edit::Array,
    index: usize,
    mut value: toml_edit::Value,
) {
    if let Some(item_prefix) = multiline_array_item_prefix(arr) {
        value.decor_mut().set_prefix(item_prefix);
    }
    arr.insert_formatted(index, value);
}

fn array_prefix_str(arr: &toml_edit::Array, index: usize) -> String {
    arr.get(index)
        .and_then(|value| value.decor().prefix())
        .and_then(|raw| raw.as_str())
        .unwrap_or("")
        .to_string()
}

/// Before `arr.remove(idx)`, move a same-line comment onto the item that
/// still owns it. The comment after a comma is the next value's prefix,
/// and a last item's comment is the array trailing.
fn rehome_comment_before_array_remove(arr: &mut toml_edit::Array, idx: usize) {
    if idx >= arr.len() {
        return;
    }
    let inherited = split_same_line_array_trailing(&array_prefix_str(arr, idx))
        .map(|(comment, _rest)| comment.to_string());
    if idx + 1 == arr.len() {
        let trailing = arr.trailing().as_str().unwrap_or("").to_string();
        let kept_tail = if let Some((_comment, rest)) = split_same_line_array_trailing(&trailing) {
            if rest.is_empty() {
                "\n".to_string()
            } else {
                rest.to_string()
            }
        } else {
            trailing
        };
        let new_trailing = if let Some(comment) = inherited {
            if kept_tail.starts_with(['\n', '\r']) {
                format!("{comment}{kept_tail}")
            } else {
                format!("{comment}\n{kept_tail}")
            }
        } else {
            kept_tail
        };
        arr.set_trailing(new_trailing);
        return;
    }
    let next_prefix = array_prefix_str(arr, idx + 1);
    let next_rest = match split_same_line_array_trailing(&next_prefix) {
        Some((_comment, rest)) => rest.to_string(),
        None => next_prefix,
    };
    let mut new_prefix = String::new();
    if let Some(comment) = inherited {
        new_prefix.push_str(&comment);
    }
    new_prefix.push_str(&next_rest);
    if let Some(next) = arr.get_mut(idx + 1) {
        next.decor_mut().set_prefix(new_prefix);
    }
}

fn try_toml_remove_subsequence(
    item: &mut toml_edit::Item,
    old_arr: &[serde_json::Value],
    new_arr: &[serde_json::Value],
) -> bool {
    // An empty array of tables serializes as no key. Fall through so
    // wholesale replacement writes `key = []`, which round-trips.
    if new_arr.is_empty() && item.as_array_of_tables().is_some() {
        return false;
    }
    let new_len = new_arr.len();
    let mut remove_indices = Vec::new();
    let mut ni = 0;
    for (oi, old_item) in old_arr.iter().enumerate() {
        if ni < new_len && old_item == &new_arr[ni] {
            ni += 1;
        } else {
            remove_indices.push(oi);
        }
    }
    if ni != new_len {
        return false;
    }
    if let Some(arr) = item.as_array_mut() {
        if arr.len() != old_arr.len() {
            return false;
        }
        for &idx in remove_indices.iter().rev() {
            // A same-line note after a comma is stored on the next item.
            // Removing that item would otherwise leave the deleted note
            // on the survivor and drop the survivor's own note.
            rehome_comment_before_array_remove(arr, idx);
            arr.remove(idx);
        }
        return true;
    }
    if let Some(aot) = item.as_array_of_tables_mut() {
        if aot.len() != old_arr.len() {
            return false;
        }
        for &idx in remove_indices.iter().rev() {
            aot.remove(idx);
        }
        return true;
    }
    false
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
    fn apply_value_diff_scalar_change_keeps_inline_comment() {
        let mut doc = parse_toml("n = 1 # keep\n");
        let old = json(r#"{"n":1}"#);
        let new = json(r#"{"n":9}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["n"], json("9"), "{result}");
        assert!(result.contains("# keep"), "{result}");
    }

    #[test]
    fn apply_value_diff_key_rename_keeps_comments() {
        let src = "# header\nn = 1 # keep\nm = 2 # stay\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"n":1,"m":2}"#);
        let new = json(r#"{"p":1,"m":2}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["p"], json("1"), "{result}");
        assert_eq!(reparsed["m"], json("2"), "{result}");
        assert!(reparsed.get("n").is_none(), "{result}");
        assert!(result.contains("# header"), "{result}");
        assert!(result.contains("# keep"), "{result}");
        assert!(result.contains("# stay"), "{result}");
    }

    #[test]
    fn apply_value_diff_table_field_change_keeps_inline_comment() {
        let src = "\
[[items]]
name = \"a\"
n = 1 # keep

[[items]]
name = \"b\"
n = 2 # other
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a","n":1},{"name":"b","n":2}]}"#);
        let new = json(r#"{"items":[{"name":"a","n":9},{"name":"b","n":2}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["items"][0]["n"], json("9"), "{result}");
        assert_eq!(reparsed["items"][1]["n"], json("2"), "{result}");
        assert!(result.contains("# keep"), "{result}");
        assert!(result.contains("# other"), "{result}");
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
    fn apply_value_diff_same_length_array_keeps_element_prefix() {
        let src = "nums = [\n  1, # keep\n  2, # other\n]\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1,2]}"#);
        let new = json(r#"{"nums":[9,2]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[9,2]"), "{result}");
        assert!(
            result.contains("[\n  9, # keep"),
            "multiline element prefix and comment must stay: {result}"
        );
        assert!(result.contains("# other"), "{result}");
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
    fn apply_value_diff_array_of_tables_shrink_keeps_survivor_comments() {
        let src = "\
# header
[[items]]
# first
name = \"react\"
ver = 1

[[items]]
# second
name = \"vue\"
ver = 2 # keep

# footer
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"react","ver":1},{"name":"vue","ver":2}]}"#);
        let new = json(r#"{"items":[{"name":"vue","ver":2}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("# second"), "{result}");
        assert!(result.contains("ver = 2 # keep"), "{result}");
        assert!(result.contains("name = \"vue\""), "{result}");
        assert!(result.contains("# footer"), "{result}");
        assert!(!result.contains("react"), "{result}");
    }

    #[test]
    fn apply_value_diff_array_of_tables_shrink_to_empty_keeps_key() {
        let src = "\
title = \"x\"
[[items]]
name = \"a\"
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"title":"x","items":[{"name":"a"}]}"#);
        let new = json(r#"{"title":"x","items":[]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["items"], json("[]"), "{result}");
        assert_eq!(reparsed["title"], json(r#""x""#), "{result}");
    }

    #[test]
    fn apply_value_diff_plain_array_shrink_to_empty_keeps_key_and_comment() {
        let src = "nums = [1, 2] # tail\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1,2]}"#);
        let new = json(r#"{"nums":[]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[]"), "{result}");
        assert!(result.contains("# tail"), "{result}");
    }

    #[test]
    fn apply_value_diff_array_shrink_not_subsequence_replaces() {
        let src = "\
[[items]]
name = \"a\"
n = 1 # keep

[[items]]
name = \"b\"
n = 2
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a","n":1},{"name":"b","n":2}]}"#);
        let new = json(r#"{"items":[{"name":"a","n":9}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        assert!(result.contains("n = 9"), "{result}");
        assert!(!result.contains("# keep"), "{result}");
    }

    #[test]
    fn apply_value_diff_plain_array_shrink_keeps_tail_comment() {
        let src = "nums = [1, 2, 3] # tail\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1,2,3]}"#);
        let new = json(r#"{"nums":[1,3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[1,3]"));
        assert!(result.contains("# tail"), "{result}");
    }

    #[test]
    fn apply_value_diff_plain_array_grow_keeps_tail_comment() {
        let src = "nums = [1] # tail\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1]}"#);
        let new = json(r#"{"nums":[1,2]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[1,2]"), "{result}");
        assert!(result.contains("# tail"), "{result}");
        assert!(
            result
                .lines()
                .any(|line| line.contains("[1,") && line.contains("# tail")),
            "one-line tail comment stays outside the brackets: {result}"
        );
    }

    #[test]
    fn apply_value_diff_multiline_array_append_without_trailing_comma() {
        let src = "\
servers = [
  { name = \"a\", port = 1 } # primary
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"servers":[{"name":"a","port":1}]}"#);
        let new = json(r#"{"servers":[{"name":"a","port":1},{"name":"b","port":2}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["servers"][1]["name"], "b", "{result}");
        let a_line = result.lines().find(|line| line.contains("\"a\"")).unwrap();
        let b_line = result.lines().find(|line| line.contains("\"b\"")).unwrap();
        assert_ne!(a_line, b_line, "{result}");
        assert!(
            a_line.contains("# primary"),
            "comment stays on the original item: {result}"
        );
        assert!(
            a_line.contains(','),
            "comma stays on the original item line: {result}"
        );
        assert!(
            !result.lines().any(|line| line.trim() == ","),
            "comma must not sit alone under the comment: {result}"
        );
        assert!(
            !b_line.contains(']'),
            "closing bracket stays on its own line: {result}"
        );
    }

    #[test]
    fn apply_value_diff_delete_where_keeps_survivor_comment() {
        let src = "\
# fleet
items = [
  { name = \"a\", port = 1 }, # keep
  { name = \"b\", port = 2 }, # drop
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a","port":1},{"name":"b","port":2}]}"#);
        let new = json(r#"{"items":[{"name":"a","port":1}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(
            reparsed["items"].as_array().map(|a| a.len()),
            Some(1),
            "{result}"
        );
        let a_line = result.lines().find(|line| line.contains("\"a\"")).unwrap();
        assert!(a_line.contains("# keep"), "{result}");
        assert!(!result.contains("# drop"), "{result}");
        assert!(!result.contains("\"b\""), "{result}");
        assert!(result.contains("# fleet"), "{result}");
    }

    #[test]
    fn apply_value_diff_delete_first_array_item_keeps_second_comment() {
        let src = "\
items = [
  { name = \"a\" }, # gone
  { name = \"b\" }, # stay
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a"},{"name":"b"}]}"#);
        let new = json(r#"{"items":[{"name":"b"}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["items"][0]["name"], "b", "{result}");
        let b_line = result.lines().find(|line| line.contains("\"b\"")).unwrap();
        assert!(b_line.contains("# stay"), "{result}");
        assert!(!result.contains("# gone"), "{result}");
    }

    #[test]
    fn apply_value_diff_delete_middle_array_item_keeps_neighbor_comments() {
        let src = "\
items = [
  { name = \"a\" }, # a
  { name = \"b\" }, # b
  { name = \"c\" }, # c
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a"},{"name":"b"},{"name":"c"}]}"#);
        let new = json(r#"{"items":[{"name":"a"},{"name":"c"}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(
            reparsed["items"].as_array().map(|a| a.len()),
            Some(2),
            "{result}"
        );
        let a_line = result.lines().find(|line| line.contains("\"a\"")).unwrap();
        let c_line = result.lines().find(|line| line.contains("\"c\"")).unwrap();
        assert!(a_line.contains("# a"), "{result}");
        assert!(c_line.contains("# c"), "{result}");
        assert!(!result.contains("# b"), "{result}");
        assert!(!result.contains("\"b\""), "{result}");
    }

    #[test]
    fn apply_value_diff_multiline_array_append_keeps_item_comment() {
        let src = "\
# fleet
servers = [
  { name = \"a\", port = 1 }, # primary
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"servers":[{"name":"a","port":1}]}"#);
        let new = json(r#"{"servers":[{"name":"a","port":1},{"name":"b","port":2}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["servers"][1]["name"], "b", "{result}");
        let a_line = result.lines().find(|line| line.contains("\"a\"")).unwrap();
        let b_line = result.lines().find(|line| line.contains("\"b\"")).unwrap();
        assert_ne!(a_line, b_line, "{result}");
        assert!(a_line.contains("# primary"), "{result}");
        assert!(!b_line.contains('#'), "{result}");
        assert!(result.contains("# fleet"), "{result}");
    }

    #[test]
    fn apply_value_diff_multiline_array_prepend_keeps_item_comment() {
        let src = "\
servers = [
  { name = \"a\", port = 1 }, # primary
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"servers":[{"name":"a","port":1}]}"#);
        let new = json(r#"{"servers":[{"name":"b","port":2},{"name":"a","port":1}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["servers"][0]["name"], "b", "{result}");
        let a_line = result.lines().find(|line| line.contains("\"a\"")).unwrap();
        let b_line = result.lines().find(|line| line.contains("\"b\"")).unwrap();
        assert_ne!(a_line, b_line, "{result}");
        assert!(a_line.contains("# primary"), "{result}");
        assert!(!b_line.contains('#'), "{result}");
    }

    #[test]
    fn apply_value_diff_multiline_array_append_keeps_comment_above_bracket() {
        let src = "\
nums = [
  1,
  # keep
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1]}"#);
        let new = json(r#"{"nums":[1,2]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[1,2]"), "{result}");
        let keep_at = result.find("# keep").expect(&result);
        let two_at = result.find('2').expect(&result);
        assert!(two_at < keep_at, "{result}");
        assert!(
            !result
                .lines()
                .any(|line| line.contains('2') && line.contains("# keep")),
            "{result}"
        );
    }

    #[test]
    fn apply_value_diff_multiline_plain_array_append_stays_one_item_per_line() {
        let src = "\
nums = [
  1,
  2,
]
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1,2]}"#);
        let new = json(r#"{"nums":[1,2,3]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[1,2,3]"), "{result}");
        assert!(
            !result
                .lines()
                .any(|line| line.contains('1') && line.contains('2') && line.contains('3')),
            "{result}"
        );
    }

    #[test]
    fn apply_value_diff_plain_array_prepend_keeps_tail_comment() {
        let src = "nums = [1] # tail\n";
        let mut doc = parse_toml(src);
        let old = json(r#"{"nums":[1]}"#);
        let new = json(r#"{"nums":[0,1]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["nums"], json("[0,1]"), "{result}");
        assert!(result.contains("# tail"), "{result}");
    }

    #[test]
    fn apply_value_diff_array_of_tables_append_keeps_existing_comments() {
        let src = "\
# header
[[items]]
name = \"a\" # keep
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a"}]}"#);
        let new = json(r#"{"items":[{"name":"a"},{"name":"b"}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["items"][0]["name"], "a", "{result}");
        assert_eq!(reparsed["items"][1]["name"], "b", "{result}");
        assert!(result.contains("# header"), "{result}");
        assert!(result.contains("# keep"), "{result}");
    }

    #[test]
    fn apply_value_diff_array_of_tables_prepend_keeps_existing_comments() {
        let src = "\
# header
[[items]]
name = \"a\" # keep
";
        let mut doc = parse_toml(src);
        let old = json(r#"{"items":[{"name":"a"}]}"#);
        let new = json(r#"{"items":[{"name":"b"},{"name":"a"}]}"#);
        apply_value_diff(doc.as_item_mut(), &old, &new);
        let result = doc.to_string();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).unwrap();
        assert_eq!(reparsed["items"][0]["name"], "b", "{result}");
        assert_eq!(reparsed["items"][1]["name"], "a", "{result}");
        assert!(result.contains("# header"), "{result}");
        assert!(result.contains("# keep"), "{result}");
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
