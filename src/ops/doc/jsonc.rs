//! JSONC: comments and trailing commas stripped for `serde_json`.

/// Strip `//` / `/* */` comments and trailing commas. String contents are kept.
pub fn strip_jsonc(src: &str) -> String {
    strip_trailing_commas(&strip_json_comments(src))
}

/// True when stripping comments or trailing commas changes the text.
pub fn looks_like_jsonc(src: &str) -> bool {
    strip_jsonc(src) != src
}

fn strip_json_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    let mut escape = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    chars.next();
                    for ch in chars.by_ref() {
                        if ch == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for ch in chars.by_ref() {
                        if prev == '*' && ch == '/' {
                            break;
                        }
                        prev = ch;
                    }
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    out
}

fn strip_trailing_commas(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let mut in_string = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Try a single-leaf surgical replace so JSONC comments survive a `doc set`.
pub fn try_preserve_jsonc_leaf(
    original: &str,
    old_value: &serde_json::Value,
    new_value: &serde_json::Value,
) -> Option<String> {
    let (path, old_leaf, new_leaf) = single_leaf_diff(old_value, new_value)?;
    let key = path.last()?;
    let old_txt = serde_json::to_string(old_leaf).ok()?;
    let new_txt = serde_json::to_string(new_leaf).ok()?;
    let needle = format!("\"{}\"", escape_json_key(key));
    let bytes = original.as_bytes();
    let mut search_from = 0usize;
    let mut hit: Option<(usize, usize)> = None;
    while let Some(rel) = original[search_from..].find(&needle) {
        let key_at = search_from + rel;
        let after_key = key_at + needle.len();
        let mut i = after_key;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            search_from = after_key;
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if original[i..].starts_with(&old_txt) {
            if hit.is_some() {
                return None;
            }
            hit = Some((i, i + old_txt.len()));
        }
        search_from = after_key;
    }
    let (start, end) = hit?;
    let mut out = String::with_capacity(original.len() + new_txt.len());
    out.push_str(&original[..start]);
    out.push_str(&new_txt);
    out.push_str(&original[end..]);
    let reparsed = super::parse_doc(&out, &super::FileFormat::Json).ok()?;
    (reparsed == *new_value).then_some(out)
}

fn escape_json_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for c in key.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

fn single_leaf_diff<'a>(
    old: &'a serde_json::Value,
    new: &'a serde_json::Value,
) -> Option<(Vec<String>, &'a serde_json::Value, &'a serde_json::Value)> {
    let mut diffs = Vec::new();
    collect_leaf_diffs(old, new, &mut Vec::new(), &mut diffs);
    if diffs.len() == 1 { diffs.pop() } else { None }
}

fn collect_leaf_diffs<'a>(
    old: &'a serde_json::Value,
    new: &'a serde_json::Value,
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, &'a serde_json::Value, &'a serde_json::Value)>,
) {
    if old == new {
        return;
    }
    match (old, new) {
        (serde_json::Value::Object(om), serde_json::Value::Object(nm)) => {
            for (k, ov) in om {
                path.push(k.clone());
                if let Some(nv) = nm.get(k) {
                    collect_leaf_diffs(ov, nv, path, out);
                } else {
                    out.push((path.clone(), ov, &serde_json::Value::Null));
                }
                path.pop();
            }
            for (k, nv) in nm {
                if !om.contains_key(k) {
                    path.push(k.clone());
                    out.push((path.clone(), &serde_json::Value::Null, nv));
                    path.pop();
                }
            }
        }
        _ => {
            out.push((path.clone(), old, new));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_line_comment_and_trailing_comma() {
        let src = "{\n  // c\n  \"a\": 1,\n}\n";
        let stripped = strip_jsonc(src);
        let val: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(val["a"], json!(1));
    }

    #[test]
    fn strip_block_comment() {
        let src = "{\"a\": /* x */ 1}";
        let val: serde_json::Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(val["a"], json!(1));
    }

    #[test]
    fn does_not_strip_slashes_inside_string() {
        let src = r#"{"u":"http://x"}"#;
        assert_eq!(strip_jsonc(src), src);
    }

    #[test]
    fn preserve_leaf_keeps_comment() {
        let src = "{\n  // keep\n  \"strict\": true\n}\n";
        let old = json!({"strict": true});
        let new = json!({"strict": false});
        let out = try_preserve_jsonc_leaf(src, &old, &new).expect("splice");
        assert!(out.contains("// keep"));
        assert!(out.contains("false"));
    }
}
