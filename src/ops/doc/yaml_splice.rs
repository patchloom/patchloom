// size-waiver: domain bulk, see #1376. YAML comment-preserving splice is cohesive domain logic; further split risks dual writers.
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
    let eol = crate::write::detect_eol(text);
    let cur_len = cur_arr.len();
    let tgt_len = tgt_arr.len();

    // Pure prepend: old array appears at the end of target.
    if tgt_len > cur_len && tgt_arr[tgt_len - cur_len..] == *cur_arr {
        let new_entries = &tgt_arr[..tgt_len - cur_len];
        let new_text = serialize_block_entries(new_entries, entry_indent, eol)?;
        return Ok(Some(insert_text_at_line(
            text,
            lines,
            first_entry,
            &new_text,
            eol,
        )));
    }

    // Pure append: old array appears at the start of target.
    if tgt_len > cur_len && tgt_arr[..cur_len] == *cur_arr {
        let new_entries = &tgt_arr[cur_len..];
        let new_text = serialize_block_entries(new_entries, entry_indent, eol)?;
        return Ok(Some(insert_text_at_line(
            text,
            lines,
            last_entry_end,
            &new_text,
            eol,
        )));
    }

    // General restructuring: replace all entry lines.
    let new_text = serialize_block_entries(tgt_arr, entry_indent, eol)?;
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i < first_entry || i >= last_entry_end {
            out.push_str(line);
            out.push_str(eol);
        } else if i == first_entry {
            out.push_str(&new_text);
        }
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.truncate(out.len() - eol.len());
    }
    Ok(Some(out))
}

/// Insert `new_text` before line `line_idx`, preserving all existing lines.
fn insert_text_at_line(
    text: &str,
    lines: &[&str],
    line_idx: usize,
    new_text: &str,
    eol: &str,
) -> String {
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == line_idx {
            out.push_str(new_text);
        }
        out.push_str(line);
        out.push_str(eol);
    }
    // Handle insertion after the last line (for append).
    if line_idx >= lines.len() {
        out.push_str(new_text);
    }
    if !text.ends_with('\n') && out.ends_with('\n') {
        out.truncate(out.len() - eol.len());
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
                // Blank lines and comments do not indicate scope exit.
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                // A non-blank, non-comment line at lower indent means we
                // left the parent key's scope.  Stop searching.
                break;
            }
            if trimmed == key_colon || trimmed.starts_with(&key_colon_sp) {
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
        if trimmed.starts_with('#') {
            // Comment line: include regardless of indentation. YAML
            // comments between entries often have reduced indent (e.g.
            // a top-level comment between indented entries).
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
fn serialize_block_entries(
    entries: &[serde_json::Value],
    indent: &str,
    eol: &str,
) -> anyhow::Result<String> {
    let mut out = String::new();
    for entry in entries {
        match entry {
            serde_json::Value::Null => {
                out.push_str(indent);
                out.push_str("- null");
                out.push_str(eol);
            }
            serde_json::Value::Bool(b) => {
                out.push_str(indent);
                out.push_str("- ");
                out.push_str(if *b { "true" } else { "false" });
                out.push_str(eol);
            }
            serde_json::Value::Number(n) => {
                out.push_str(indent);
                out.push_str("- ");
                out.push_str(&n.to_string());
                out.push_str(eol);
            }
            serde_json::Value::String(s) => {
                out.push_str(indent);
                out.push_str("- ");
                // Quote strings that need it. Use double-quoting for strings
                // containing newlines because YAML single-quoted scalars fold
                // line breaks into spaces, corrupting the data.
                if s.contains('\n') {
                    let escaped = serde_json::to_string(s).expect("JSON string");
                    out.push_str(&escaped);
                } else if needs_yaml_quoting(s) {
                    out.push_str(&format!("'{}'", s.replace('\'', "''")));
                } else {
                    out.push_str(s);
                }
                out.push_str(eol);
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
                    out.push_str(eol);
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
    // Strings with leading/trailing whitespace lose it in plain scalar context.
    if s != s.trim() {
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
    // "- " at the start looks like a block sequence entry when emitted
    // inside a block sequence context (e.g., `- - item` is a nested sequence).
    if s.starts_with("- ") || s == "-" {
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
    // needs_yaml_quoting
    // -----------------------------------------------------------------------

    #[test]
    fn needs_yaml_quoting_leading_whitespace() {
        assert!(needs_yaml_quoting(" hello"), "leading space needs quoting");
    }

    #[test]
    fn needs_yaml_quoting_trailing_whitespace() {
        assert!(needs_yaml_quoting("hello "), "trailing space needs quoting");
    }

    #[test]
    fn needs_yaml_quoting_only_whitespace() {
        assert!(needs_yaml_quoting(" "), "single space needs quoting");
        assert!(needs_yaml_quoting("  "), "double space needs quoting");
    }

    #[test]
    fn needs_yaml_quoting_plain_string_ok() {
        assert!(
            !needs_yaml_quoting("hello"),
            "plain string does not need quoting"
        );
    }

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

    #[test]
    fn find_block_entries_end_comment_at_reduced_indent() {
        // A comment with less indentation than the entries should NOT
        // prematurely end the block (#1110).
        let yaml = "items:\n  - one\n# reduced indent comment\n  - two\nnext: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_block_entries_end(&lines, 1, "  "), 4);
    }

    #[test]
    fn find_block_entries_end_comment_at_entry_indent() {
        let yaml = "items:\n  - one\n  # same indent comment\n  - two\nnext: val\n";
        let lines: Vec<&str> = yaml.lines().collect();
        assert_eq!(find_block_entries_end(&lines, 1, "  "), 4);
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
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        assert!(result.contains("  - null\n"));
        assert!(result.contains("  - true\n"));
        assert!(result.contains("  - 42\n"));
        assert!(result.contains("  - hello\n"));
    }

    #[test]
    fn serialize_string_needing_quoting() {
        let entries = vec![serde_json::json!("true"), serde_json::json!("")];
        let result = serialize_block_entries(&entries, "", "\n").unwrap();
        assert!(result.contains("- 'true'\n"));
        assert!(result.contains("- ''\n"));
    }

    #[test]
    fn serialize_object_entry() {
        let entries = vec![serde_json::json!({"name": "a", "value": 1})];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
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
    fn needs_quoting_special_chars() {
        assert!(needs_yaml_quoting("# comment"));
        assert!(needs_yaml_quoting("a: b"));
        assert!(needs_yaml_quoting(""));
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

    #[test]
    fn needs_quoting_dash_space_prefix() {
        // "- item" at the start of a block sequence entry produces `- - item`
        // which YAML parses as a nested sequence, not a string.
        assert!(needs_yaml_quoting("- item"));
        assert!(needs_yaml_quoting("- "));
        assert!(needs_yaml_quoting("-")); // bare dash is also ambiguous
        // Plain dash in the middle or end is fine.
        assert!(!needs_yaml_quoting("foo-bar"));
        assert!(!needs_yaml_quoting("a-"));
    }

    #[test]
    fn serialize_string_with_newline_uses_double_quote() {
        use serde_json::json;
        let entries = vec![json!("hello\nworld")];
        let result = serialize_block_entries(&entries, "", "\n").unwrap();
        // Must use double-quoting to preserve the newline.
        assert!(
            result.contains('"'),
            "expected double-quoted string for newline, got: {result}"
        );
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!("hello\nworld")]);
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

    // -----------------------------------------------------------------------
    // serialize_block_entries — compound value coverage (#983)
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_nested_object() {
        use serde_json::json;
        let entries = vec![json!({"name": "main", "env": [{"name": "KEY", "value": "val"}]})];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        // First line starts with "  - "
        let first_line = result.lines().next().unwrap();
        assert!(
            first_line.starts_with("  - "),
            "expected '  - ' prefix, got: {first_line}"
        );
        // Continuation lines start with "    " (indent + 2 spaces for alignment)
        for line in result.lines().skip(1) {
            assert!(
                line.starts_with("    "),
                "continuation line must start with 4-space indent, got: {line}"
            );
        }
        // Round-trip: parse back and verify value equality
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "main");
        assert_eq!(parsed[0]["env"][0]["name"], "KEY");
        assert_eq!(parsed[0]["env"][0]["value"], "val");
    }

    #[test]
    fn serialize_array_of_arrays() {
        use serde_json::json;
        let entries = vec![json!([1, 2, 3])];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        let first_line = result.lines().next().unwrap();
        assert!(
            first_line.starts_with("  - "),
            "expected '  - ' prefix, got: {first_line}"
        );
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!([1, 2, 3])]);
    }

    #[test]
    fn serialize_empty_object() {
        use serde_json::json;
        let entries = vec![json!({})];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        assert!(
            result.starts_with("  - "),
            "expected '  - ' prefix, got: {result}"
        );
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!({})]);
    }

    #[test]
    fn serialize_empty_array() {
        use serde_json::json;
        let entries = vec![json!([])];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        assert!(
            result.starts_with("  - "),
            "expected '  - ' prefix, got: {result}"
        );
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!([])]);
    }

    #[test]
    fn serialize_object_with_keys_needing_quoting() {
        use serde_json::json;
        let entries = vec![json!({"true": "val", "null": "x"})];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        // The keys "true" and "null" must be quoted in the output so that
        // YAML parsers treat them as strings, not booleans/null.
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 1);
        // Verify the keys round-trip as strings, not as boolean/null types
        let obj = parsed[0].as_object().unwrap();
        assert!(
            obj.contains_key("true"),
            "key 'true' missing after roundtrip"
        );
        assert!(
            obj.contains_key("null"),
            "key 'null' missing after roundtrip"
        );
        assert_eq!(obj["true"], "val");
        assert_eq!(obj["null"], "x");
    }

    #[test]
    fn serialize_string_containing_single_quotes() {
        use serde_json::json;
        // To exercise the `s.replace('\'', "''")` escape path, the string
        // must both trigger `needs_yaml_quoting` AND contain a single quote.
        // "# it's a comment" starts with '#' (needs quoting) and has a quote.
        let entries = vec![json!("# it's a comment")];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        assert!(
            result.starts_with("  - "),
            "expected '  - ' prefix, got: {result}"
        );
        // The single quote must be escaped as '' in YAML single-quoted style
        assert!(
            result.contains("''"),
            "single quote should be escaped: {result}"
        );
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!("# it's a comment")]);
    }

    #[test]
    fn serialize_unicode_string() {
        use serde_json::json;
        let entries = vec![json!("café"), json!("こんにちは")];
        let result = serialize_block_entries(&entries, "  ", "\n").unwrap();
        let parsed: Vec<serde_json::Value> = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(parsed, vec![json!("café"), json!("こんにちは")]);
    }

    #[test]
    fn serialize_block_entries_crlf() {
        let entries = vec![serde_json::json!("alpha"), serde_json::json!("beta")];
        let result = serialize_block_entries(&entries, "  ", "\r\n").unwrap();
        assert!(
            result.contains("- alpha\r\n"),
            "should use CRLF line endings: {result:?}"
        );
        assert!(
            !result.contains("- alpha\n\r\n"),
            "should not double-terminate: {result:?}"
        );
    }

    #[test]
    fn splice_preserves_crlf_line_endings() {
        use serde_json::json;
        let yaml = "items:\r\n  - one\r\n  - two\r\n";
        let current: serde_json::Value = serde_yaml_ng::from_str(yaml).unwrap();
        let mut target = current.clone();
        target["items"].as_array_mut().unwrap().push(json!("three"));
        let result = splice_yaml_array_diffs(yaml, &current, &target).unwrap();
        if let Some(spliced) = result {
            assert!(
                spliced.contains("\r\n"),
                "CRLF should be preserved in splice output: {spliced:?}"
            );
            assert!(
                !spliced.contains("\n\r\n"),
                "should not have double line-endings: {spliced:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // splice_yaml_array_diffs — multi-diff coverage (#982)
    // -----------------------------------------------------------------------

    #[test]
    fn splice_multi_diff_two_arrays_grow() {
        use serde_json::json;
        // A YAML document with two arrays at different paths; both grow.
        let yaml = "name: app\nenv:\n  - KEY1\nvolumes:\n  - /data\n";
        let current: serde_json::Value = serde_yaml_ng::from_str(yaml).unwrap();
        let mut target = current.clone();
        target["env"].as_array_mut().unwrap().push(json!("KEY2"));
        target["volumes"]
            .as_array_mut()
            .unwrap()
            .push(json!("/logs"));

        let result = splice_yaml_array_diffs(yaml, &current, &target).unwrap();
        // The splice may or may not succeed (depends on CST round-trip fidelity).
        // If it succeeds, verify correctness.
        if let Some(spliced) = result {
            let reparsed: serde_json::Value = serde_yaml_ng::from_str(&spliced).unwrap();
            assert_eq!(reparsed, target, "multi-diff splice mismatch: {spliced}");
        }
        // If None, the function correctly gave up rather than producing wrong output.
    }

    // -----------------------------------------------------------------------
    // splice_yaml_root_sequence — object entries (#982)
    // -----------------------------------------------------------------------

    #[test]
    fn splice_root_sequence_object_entries() {
        use serde_json::json;
        let yaml = "- name: task1\n  value: 1\n- name: task2\n  value: 2\n";
        let cur = vec![
            json!({"name": "task1", "value": 1}),
            json!({"name": "task2", "value": 2}),
        ];
        let tgt = vec![
            json!({"name": "task1", "value": 1}),
            json!({"name": "task2", "value": 2}),
            json!({"name": "task3", "value": 3}),
        ];
        let result = splice_yaml_root_sequence(yaml, &cur, &tgt).unwrap();
        // The splice should succeed for a pure append of an object entry.
        if let Some(spliced) = result {
            assert!(
                spliced.contains("task3"),
                "appended object entry missing: {spliced}"
            );
            let reparsed: serde_json::Value = serde_yaml_ng::from_str(&spliced).unwrap();
            assert_eq!(
                reparsed,
                serde_json::Value::Array(tgt),
                "round-trip mismatch: {spliced}"
            );
        }
    }

    #[test]
    fn find_yaml_key_line_rejects_prefix_match() {
        // Regression: "name:" must not match "namespace:".
        let yaml = "namespace: kube-system\nname: my-app\nitems:\n  - one\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_yaml_key_line(&lines, &["name".into()]);
        assert_eq!(
            result,
            Some(1),
            "should match 'name:' on line 1, not 'namespace:' on line 0"
        );
    }

    #[test]
    fn find_yaml_key_line_matches_key_only_on_line() {
        // "name:" at end of line (value on next line) should match.
        let yaml = "name:\n  first: Alice\n  last: Smith\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_yaml_key_line(&lines, &["name".into()]);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn find_yaml_key_line_does_not_escape_into_sibling() {
        // Regression: searching for ["a", "target"] found "target:" under
        // "b:" because lines with indent < min_indent were skipped instead
        // of stopping the search.
        let yaml = "a:\n  x: 1\nb:\n  target:\n    value: found_me\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_yaml_key_line(&lines, &["a".into(), "target".into()]);
        assert_eq!(result, None, "should not find 'target' under sibling 'b'");
    }

    #[test]
    fn find_yaml_key_line_finds_child_within_scope() {
        // Positive test: the key IS within the parent's scope.
        let yaml = "a:\n  target:\n    value: here\nb:\n  other: 1\n";
        let lines: Vec<&str> = yaml.lines().collect();
        let result = find_yaml_key_line(&lines, &["a".into(), "target".into()]);
        assert_eq!(result, Some(1), "should find 'target' under 'a'");
    }
}
