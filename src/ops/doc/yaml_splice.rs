/// Check if there are any arrays that grew between `old` and `new`.
pub(super) fn has_array_growth_diffs(old: &serde_json::Value, new: &serde_json::Value) -> bool {
    if old == new {
        return false;
    }
    match (old, new) {
        (serde_json::Value::Object(old_map), serde_json::Value::Object(new_map)) => {
            for (key, new_val) in new_map {
                if let Some(old_val) = old_map.get(key)
                    && has_array_growth_diffs(old_val, new_val)
                {
                    return true;
                }
            }
            false
        }
        (serde_json::Value::Array(old_arr), serde_json::Value::Array(new_arr)) => {
            if new_arr.len() > old_arr.len() {
                return true;
            }
            // Recurse into same-length elements to detect nested growth.
            old_arr
                .iter()
                .zip(new_arr.iter())
                .any(|(o, n)| has_array_growth_diffs(o, n))
        }
        _ => false,
    }
}

/// Text-level fallback for array diffs that the CST path could not
/// handle (general restructuring where the array is neither a pure
/// prepend nor a pure append of the original).
///
/// `text` is the CST-serialized document (with non-array diffs already
/// applied).  `current` is the parsed value of `text`.  `target` is the
/// desired final value.  The function finds arrays that differ between
/// `current` and `target` and replaces them in the text, preserving
/// all surrounding comments.
pub(super) fn splice_yaml_array_diffs(
    text: &str,
    current: &serde_json::Value,
    target: &serde_json::Value,
) -> anyhow::Result<Option<String>> {
    let mut diffs: Vec<(Vec<String>, &[serde_json::Value], &[serde_json::Value])> = Vec::new();
    find_array_diffs(current, target, &mut Vec::new(), &mut diffs);
    if diffs.is_empty() {
        return Ok(None);
    }
    let mut result = text.to_string();
    for (key_path, cur_arr, tgt_arr) in &diffs {
        if let Some(spliced) = splice_yaml_array_at_path(&result, key_path, cur_arr, tgt_arr)? {
            result = spliced;
        } else {
            // Could not splice this array; give up.
            return Ok(None);
        }
    }
    if serde_yaml_ng::from_str::<serde_json::Value>(&result).is_ok_and(|v| v == *target) {
        // Verify the result round-trips cleanly through the CST library (yaml_edit).
        // serde_yaml_ng is more lenient with indentation patterns than yaml_edit.
        // A spliced result with altered indentation may parse once but corrupt
        // subsequent CST modifications (e.g., duplicating keys with wrong indent).
        // The CST no-op round-trip catches this: if `file.to_string()` differs from
        // the input, the CST misinterprets the structure and future edits will fail (#972).
        if let Ok(file) = result.parse::<yaml_edit::YamlFile>() {
            let roundtrip = file.to_string();
            if roundtrip == result {
                return Ok(Some(result));
            }
        }
        Ok(None)
    } else {
        Ok(None)
    }
}

/// Recursively find arrays that differ between `current` and `target`.
/// Each result entry includes the key path, the current array, and the
/// target array (needed to detect prepend vs append vs general).
fn find_array_diffs<'a>(
    current: &'a serde_json::Value,
    target: &'a serde_json::Value,
    path: &mut Vec<String>,
    result: &mut Vec<(
        Vec<String>,
        &'a [serde_json::Value],
        &'a [serde_json::Value],
    )>,
) {
    if current == target {
        return;
    }
    match (current, target) {
        (serde_json::Value::Object(cur_map), serde_json::Value::Object(tgt_map)) => {
            for (key, tgt_val) in tgt_map {
                if let Some(cur_val) = cur_map.get(key) {
                    path.push(key.clone());
                    find_array_diffs(cur_val, tgt_val, path, result);
                    path.pop();
                }
            }
        }
        (serde_json::Value::Array(cur_arr), serde_json::Value::Array(tgt_arr))
            if cur_arr.len() != tgt_arr.len() =>
        {
            // Different lengths: this array itself changed (growth/shrink).
            result.push((path.clone(), cur_arr.as_slice(), tgt_arr.as_slice()));
        }
        (serde_json::Value::Array(_), serde_json::Value::Array(_)) => {
            // Same-length arrays: the splice path cannot navigate into array
            // elements by index (it only supports mapping key paths). If the
            // diff is inside an element (e.g., containers[0].env grew), we
            // must NOT push the parent array or the splice will replace all
            // entries via serde_yaml_ng, losing the original indentation (#972).
            // Instead, we skip it and let the caller fall back to the
            // non-preserving serializer which produces correct output.
        }
        _ => {}
    }
}

/// Splice an array at `key_path` in the YAML `text`, handling
/// prepend, append, and general replacement while preserving
/// surrounding comments and formatting.
fn splice_yaml_array_at_path(
    text: &str,
    key_path: &[String],
    cur_arr: &[serde_json::Value],
    tgt_arr: &[serde_json::Value],
) -> anyhow::Result<Option<String>> {
    let lines: Vec<&str> = text.lines().collect();
    // Find the key line that owns the array.
    let key_line_idx = match find_yaml_key_line(&lines, key_path) {
        Some(idx) => idx,
        None => return Ok(None),
    };
    // Find entry boundaries.
    let (first_entry, entry_indent) = match find_first_block_entry(&lines, key_line_idx + 1) {
        Some(v) => v,
        None => return Ok(None),
    };
    let last_entry_end = find_block_entries_end(&lines, first_entry, &entry_indent);
    splice_array_lines(
        text,
        &lines,
        first_entry,
        last_entry_end,
        &entry_indent,
        cur_arr,
        tgt_arr,
    )
}

/// Splice a root-level sequence in YAML text (sequence-rooted docs).
pub(super) fn splice_yaml_root_sequence(
    text: &str,
    cur_arr: &[serde_json::Value],
    tgt_arr: &[serde_json::Value],
) -> anyhow::Result<Option<String>> {
    let lines: Vec<&str> = text.lines().collect();
    let (first_entry, entry_indent) = match find_first_block_entry(&lines, 0) {
        Some(v) => v,
        None => return Ok(None),
    };
    let last_entry_end = find_block_entries_end(&lines, first_entry, &entry_indent);
    splice_array_lines(
        text,
        &lines,
        first_entry,
        last_entry_end,
        &entry_indent,
        cur_arr,
        tgt_arr,
    )
}

/// Core splice logic shared by mapping-rooted and sequence-rooted paths.
/// Detects prepend/append/general and splices accordingly.
fn splice_array_lines(
    text: &str,
    lines: &[&str],
    first_entry: usize,
    last_entry_end: usize,
    entry_indent: &str,
    cur_arr: &[serde_json::Value],
    tgt_arr: &[serde_json::Value],
) -> anyhow::Result<Option<String>> {
    let cur_len = cur_arr.len();
    let tgt_len = tgt_arr.len();

    // Pure prepend: old array appears at the end of target.
    if tgt_len > cur_len && tgt_arr[tgt_len - cur_len..] == *cur_arr {
        let new_entries = &tgt_arr[..tgt_len - cur_len];
        let new_text = serialize_block_entries(new_entries, entry_indent)?;
        return Ok(Some(insert_text_at_line(
            text,
            lines,
            first_entry,
            &new_text,
        )));
    }

    // Pure append: old array appears at the start of target.
    if tgt_len > cur_len && tgt_arr[..cur_len] == *cur_arr {
        let new_entries = &tgt_arr[cur_len..];
        let new_text = serialize_block_entries(new_entries, entry_indent)?;
        return Ok(Some(insert_text_at_line(
            text,
            lines,
            last_entry_end,
            &new_text,
        )));
    }

    // General restructuring: replace all entry lines.
    let new_text = serialize_block_entries(tgt_arr, entry_indent)?;
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i < first_entry || i >= last_entry_end {
            out.push_str(line);
            out.push('\n');
        } else if i == first_entry {
            out.push_str(&new_text);
        }
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    Ok(Some(out))
}

/// Insert `new_text` before line `line_idx`, preserving all existing lines.
fn insert_text_at_line(text: &str, lines: &[&str], line_idx: usize, new_text: &str) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == line_idx {
            out.push_str(new_text);
        }
        out.push_str(line);
        out.push('\n');
    }
    // Handle insertion after the last line (for append).
    if line_idx >= lines.len() {
        out.push_str(new_text);
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Find the line index of the YAML key at the given path.
/// For path `["server", "tags"]`, finds `server:` first, then `tags:`
/// at deeper indentation inside it.
fn find_yaml_key_line(lines: &[&str], key_path: &[String]) -> Option<usize> {
    let mut search_start = 0;
    let mut min_indent = 0;
    for (depth, key) in key_path.iter().enumerate() {
        let key_colon = format!("{}:", key);
        let key_colon_sp = format!("{}: ", key);
        let mut found = false;
        for (i, line) in lines.iter().enumerate().skip(search_start) {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            if indent < min_indent {
                continue;
            }
            if trimmed.starts_with(&key_colon) || trimmed.starts_with(&key_colon_sp) {
                search_start = i + 1;
                min_indent = indent + 1;
                if depth == key_path.len() - 1 {
                    return Some(i);
                }
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }
    None
}

/// Find the first block-style entry (`- ...`) after `start_line`.
/// Returns `(line_index, indent_string)`.
fn find_first_block_entry(lines: &[&str], start_line: usize) -> Option<(usize, String)> {
    for i in start_line..lines.len() {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with("- ") || trimmed == "-" {
            let indent = &lines[i][..lines[i].len() - trimmed.len()];
            return Some((i, indent.to_string()));
        }
        // Stop at blank lines or lines with less indentation (end of value).
        if trimmed.is_empty() {
            continue;
        }
        // A non-entry line at the same or lesser indent means the key
        // has no block entries (might be flow-style or empty).
        if !trimmed.starts_with('#') {
            break;
        }
    }
    None
}

/// Find the line index AFTER the last block-style entry.
/// An entry can span multiple lines (for compound values like mappings).
fn find_block_entries_end(lines: &[&str], first_entry: usize, entry_indent: &str) -> usize {
    let indent_len = entry_indent.len();
    let mut end = first_entry;
    for (i, line) in lines.iter().enumerate().skip(first_entry) {
        let trimmed = line.trim_start();
        let cur_indent = line.len() - trimmed.len();
        if trimmed.is_empty() {
            // Blank line: could be inside or after the array.
            // Include it tentatively; the next non-blank line decides.
            end = i + 1;
            continue;
        }
        if trimmed.starts_with('#') && cur_indent >= indent_len {
            // Comment at or deeper than entry indent: part of the array.
            end = i + 1;
            continue;
        }
        if cur_indent > indent_len {
            // Continuation of a multi-line entry.
            end = i + 1;
            continue;
        }
        if cur_indent == indent_len && (trimmed.starts_with("- ") || trimmed == "-") {
            // Another entry at the same indent.
            end = i + 1;
            continue;
        }
        // Line at same or lesser indent that is not an entry: end of array.
        break;
    }
    // Trim trailing blank lines from the range so we don't eat
    // blank lines that separate the array from the next key.
    while end > first_entry && lines.get(end - 1).is_some_and(|l| l.trim().is_empty()) {
        end -= 1;
    }
    end
}

/// Serialize `entries` as block-style YAML array entries at `indent`.
fn serialize_block_entries(entries: &[serde_json::Value], indent: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    for entry in entries {
        match entry {
            serde_json::Value::Null => {
                out.push_str(indent);
                out.push_str("- null\n");
            }
            serde_json::Value::Bool(b) => {
                out.push_str(indent);
                out.push_str("- ");
                out.push_str(if *b { "true" } else { "false" });
                out.push('\n');
            }
            serde_json::Value::Number(n) => {
                out.push_str(indent);
                out.push_str("- ");
                out.push_str(&n.to_string());
                out.push('\n');
            }
            serde_json::Value::String(s) => {
                out.push_str(indent);
                out.push_str("- ");
                // Quote strings that need it.
                if needs_yaml_quoting(s) {
                    out.push_str(&format!("'{}'", s.replace('\'', "''")));
                } else {
                    out.push_str(s);
                }
                out.push('\n');
            }
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                // Serialize compound value, then re-indent.
                let yaml = serde_yaml_ng::to_string(entry)?;
                let yaml_lines: Vec<&str> = yaml.trim_end().lines().collect();
                for (j, line) in yaml_lines.iter().enumerate() {
                    out.push_str(indent);
                    if j == 0 {
                        out.push_str("- ");
                    } else {
                        out.push_str("  ");
                    }
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    Ok(out)
}

/// Check if a string needs YAML quoting.
pub fn needs_yaml_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Values that look like booleans, null, or YAML special floats.
    if s.eq_ignore_ascii_case("true")
        || s.eq_ignore_ascii_case("false")
        || s.eq_ignore_ascii_case("yes")
        || s.eq_ignore_ascii_case("no")
        || s.eq_ignore_ascii_case("on")
        || s.eq_ignore_ascii_case("off")
        || s.eq_ignore_ascii_case("null")
        || s.eq_ignore_ascii_case(".inf")
        || s.eq_ignore_ascii_case("-.inf")
        || s.eq_ignore_ascii_case("+.inf")
        || s.eq_ignore_ascii_case(".nan")
        || s == "~"
    {
        return true;
    }
    if s.parse::<f64>().is_ok() {
        return true;
    }
    // Trailing colon makes the value look like a mapping key (e.g., "host:").
    if s.ends_with(':') {
        return true;
    }
    // YAML tag indicator (e.g., "!important", "!!str").
    // Strings with special YAML characters.
    s.starts_with(|c: char| "#&*?|>{[%@`\"'!".contains(c))
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // find_yaml_key_line
    // -----------------------------------------------------------------------

    #[test]
    fn find_yaml_key_line_single_key() {
        let yaml = "name: foo\nversion: 1\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_yaml_key_line(&lines, &["version".into()]), Some(1));
    }

    #[test]
    fn find_yaml_key_line_nested_path() {
        let yaml = "top:\n  mid:\n    deep: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(
            find_yaml_key_line(&lines, &["top".into(), "mid".into(), "deep".into()]),
            Some(2)
        );
    }

    #[test]
    fn find_yaml_key_line_missing_key() {
        let yaml = "name: foo\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_yaml_key_line(&lines, &["missing".into()]), None);
    }

    // -----------------------------------------------------------------------
    // find_first_block_entry
    // -----------------------------------------------------------------------

    #[test]
    fn find_first_block_entry_simple() {
        let yaml = "items:\n  - one\n  - two\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_first_block_entry(&lines, 1);
        assert_eq!(result, Some((1, "  ".to_string())));
    }

    #[test]
    fn find_first_block_entry_with_comment() {
        let yaml = "items:\n  # a comment\n  - one\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_first_block_entry(&lines, 1);
        assert_eq!(result, Some((2, "  ".to_string())));
    }

    #[test]
    fn find_first_block_entry_no_entries() {
        let yaml = "items: []\nnext: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_first_block_entry(&lines, 1), None);
    }

    // -----------------------------------------------------------------------
    // find_block_entries_end
    // -----------------------------------------------------------------------

    #[test]
    fn find_block_entries_end_simple() {
        let yaml = "items:\n  - one\n  - two\nnext: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_block_entries_end(&lines, 1, "  "), 3);
    }

    #[test]
    fn find_block_entries_end_multiline_values() {
        let yaml = "items:\n  - name: a\n    value: 1\n  - name: b\n    value: 2\nnext: x\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_block_entries_end(&lines, 1, "  "), 5);
    }

    #[test]
    fn find_block_entries_end_trims_trailing_blanks() {
        let yaml = "items:\n  - one\n  - two\n\nnext: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        // Should NOT include the blank line between array and next key.
        assert_eq!(find_block_entries_end(&lines, 1, "  "), 3);
    }

    // -----------------------------------------------------------------------
    // serialize_block_entries
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_scalars() {
        let entries = vec![
            serde_json::json!(null),
            serde_json::json!(true),
            serde_json::json!(42),
            serde_json::json!("hello"),
        ];
        let result = serialize_block_entries(&entries, "  ").unwrap();
        assert!(result.contains("  - null\n"));
        assert!(result.contains("  - true\n"));
        assert!(result.contains("  - 42\n"));
        assert!(result.contains("  - hello\n"));
    }

    #[test]
    fn serialize_string_needing_quoting() {
        let entries = vec![serde_json::json!("true"), serde_json::json!("")];
        let result = serialize_block_entries(&entries, "").unwrap();
        assert!(result.contains("- 'true'\n"));
        assert!(result.contains("- ''\n"));
    }

    #[test]
    fn serialize_object_entry() {
        let entries = vec![serde_json::json!({"name": "a", "value": 1})];
        let result = serialize_block_entries(&entries, "  ").unwrap();
        // First line should be "  - ..." and continuation "    ..."
        let first_line = result.lines().next().unwrap();
        assert!(first_line.starts_with("  - "));
    }

    // -----------------------------------------------------------------------
    // has_array_growth_diffs
    // -----------------------------------------------------------------------

    #[test]
    fn has_array_growth_no_change() {
        let v = serde_json::json!({"a": [1, 2]});
        assert!(!has_array_growth_diffs(&v, &v));
    }

    #[test]
    fn has_array_growth_detects_growth() {
        let old = serde_json::json!({"a": [1]});
        let new = serde_json::json!({"a": [1, 2]});
        assert!(has_array_growth_diffs(&old, &new));
    }

    #[test]
    fn has_array_growth_detects_nested_growth() {
        let old = serde_json::json!({"a": {"b": [1]}});
        let new = serde_json::json!({"a": {"b": [1, 2]}});
        assert!(has_array_growth_diffs(&old, &new));
    }

    #[test]
    fn has_array_growth_shrink_is_not_growth() {
        let old = serde_json::json!({"a": [1, 2, 3]});
        let new = serde_json::json!({"a": [1]});
        assert!(!has_array_growth_diffs(&old, &new));
    }

    // -----------------------------------------------------------------------
    // needs_yaml_quoting
    // -----------------------------------------------------------------------

    #[test]
    fn needs_quoting_booleans_and_null() {
        assert!(needs_yaml_quoting("true"));
        assert!(needs_yaml_quoting("True"));
        assert!(needs_yaml_quoting("yes"));
        assert!(needs_yaml_quoting("null"));
        assert!(needs_yaml_quoting("~"));
    }

    #[test]
    fn needs_quoting_numbers() {
        assert!(needs_yaml_quoting("42"));
        assert!(needs_yaml_quoting("3.14"));
    }

    #[test]
    fn needs_quoting_special_chars() {
        assert!(needs_yaml_quoting("# comment"));
        assert!(needs_yaml_quoting("a: b"));
        assert!(needs_yaml_quoting(""));
    }

    #[test]
    fn needs_quoting_trailing_colon() {
        assert!(needs_yaml_quoting("host:"));
        assert!(needs_yaml_quoting("value:"));
    }

    #[test]
    fn needs_quoting_tag_indicator() {
        assert!(needs_yaml_quoting("!important"));
        assert!(needs_yaml_quoting("!!str"));
    }

    #[test]
    fn needs_quoting_special_floats() {
        assert!(needs_yaml_quoting(".inf"));
        assert!(needs_yaml_quoting("-.inf"));
        assert!(needs_yaml_quoting(".nan"));
        assert!(needs_yaml_quoting(".Inf"));
        assert!(needs_yaml_quoting(".NaN"));
    }

    #[test]
    fn plain_strings_need_no_quoting() {
        assert!(!needs_yaml_quoting("hello"));
        assert!(!needs_yaml_quoting("foo-bar_baz"));
        assert!(!needs_yaml_quoting("http://example.com:8080/path"));
    }

    // -----------------------------------------------------------------------
    // splice_yaml_root_sequence
    // -----------------------------------------------------------------------

    #[test]
    fn splice_root_sequence_replaces_entries() {
        let yaml = "- one\n- two\n";
        let cur = vec![serde_json::json!("one"), serde_json::json!("two")];
        let tgt = vec![
            serde_json::json!("one"),
            serde_json::json!("two"),
            serde_json::json!("three"),
        ];
        let result = splice_yaml_root_sequence(yaml, &cur, &tgt)
            .unwrap()
            .expect("should produce a spliced result");
        assert!(result.contains("- three\n"));
        assert!(result.contains("- one\n"));
    }
}
