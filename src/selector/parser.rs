/// A selector is a sequence of segments that navigate through a JSON value tree.
pub type Selector = Vec<Segment>;

/// A single segment in a selector path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Object key access, e.g. `scripts`.
    Key(String),
    /// Array index access, e.g. `[0]`.
    Index(usize),
    /// Wildcard – matches all array elements: `[*]`.
    Wildcard,
    /// Predicate filter on array elements, e.g. `[name=api]`.
    Predicate { key: String, value: String },
}

/// Parse a selector string into a [`Selector`].
///
/// # Examples
///
/// ```text
/// "scripts.test"                 → [Key("scripts"), Key("test")]
/// "jobs[0].steps[*].name"        → [Key("jobs"), Index(0), Key("steps"), Wildcard, Key("name")]
/// "jobs[id=test].timeout-minutes" → [Key("jobs"), Predicate{…}, Key("timeout-minutes")]
/// ```
pub fn parse(input: &str) -> Result<Selector, String> {
    // JSON Pointer habit: agents pass `/feature_flag` for root keys. A single
    // leading slash means "from root" and is stripped so it does not create a
    // literal key named `/feature_flag` (#1794). Only one slash is removed.
    let input = input.strip_prefix('/').unwrap_or(input);
    let mut segments = Vec::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Skip dots between segments.
        if bytes[i] == b'.' {
            i += 1;
            continue;
        }

        if bytes[i] == b'[' {
            i += 1; // skip '['
            let start = i;
            let mut depth = 1u32;
            while i < len && depth > 0 {
                if bytes[i] == b'[' {
                    depth += 1;
                } else if bytes[i] == b']' {
                    depth -= 1;
                }
                if depth > 0 {
                    i += 1;
                }
            }
            if depth > 0 {
                return Err("unclosed bracket in selector".to_string());
            }
            let content = &input[start..i];
            i += 1; // skip ']'

            if content == "*" {
                segments.push(Segment::Wildcard);
            } else if let Some(eq_pos) = content.find('=') {
                let key = content[..eq_pos].to_string();
                let value = content[eq_pos + 1..].to_string();
                if key.is_empty() {
                    return Err("empty predicate key".to_string());
                }
                if let Some(stripped) = key.strip_prefix('?') {
                    return Err(format!(
                        "predicate key starts with '?'; use [{stripped}={value}] instead of [{key}={value}]"
                    ));
                }
                segments.push(Segment::Predicate { key, value });
            } else if let Ok(idx) = content.parse::<usize>() {
                segments.push(Segment::Index(idx));
            } else {
                return Err(format!("invalid bracket content: {content}"));
            }
        } else {
            // Key segment: read until '.', '[', or end.
            let start = i;
            while i < len && bytes[i] != b'.' && bytes[i] != b'[' {
                i += 1;
            }
            let key = &input[start..i];
            if !key.is_empty() {
                segments.push(Segment::Key(key.to_string()));
            }
        }
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_key_path() {
        let sel = parse("scripts.test").unwrap();
        assert_eq!(
            sel,
            vec![Segment::Key("scripts".into()), Segment::Key("test".into()),]
        );
    }

    #[test]
    fn parse_array_index() {
        let sel = parse("jobs[0].name").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("jobs".into()),
                Segment::Index(0),
                Segment::Key("name".into()),
            ]
        );
    }

    #[test]
    fn parse_predicate() {
        let sel = parse("jobs[id=test].timeout").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("jobs".into()),
                Segment::Predicate {
                    key: "id".into(),
                    value: "test".into(),
                },
                Segment::Key("timeout".into()),
            ]
        );
    }

    #[test]
    fn parse_wildcard() {
        let sel = parse("steps[*].name").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("steps".into()),
                Segment::Wildcard,
                Segment::Key("name".into()),
            ]
        );
    }

    #[test]
    fn parse_unclosed_bracket_returns_error() {
        let err = parse("items[0").unwrap_err();
        assert!(
            err.contains("unclosed bracket"),
            "expected 'unclosed bracket', got: {err}"
        );
    }

    #[test]
    fn parse_empty_predicate_key_returns_error() {
        let err = parse("items[=value]").unwrap_err();
        assert!(
            err.contains("empty predicate key"),
            "expected 'empty predicate key', got: {err}"
        );
    }

    #[test]
    fn parse_question_mark_prefix_in_predicate_returns_error() {
        let err = parse("items[?name=foo]").unwrap_err();
        assert!(
            err.contains("use [name=foo]"),
            "expected helpful suggestion, got: {err}"
        );
    }

    #[test]
    fn parse_invalid_bracket_content_returns_error() {
        let err = parse("items[abc]").unwrap_err();
        assert!(
            err.contains("invalid bracket content"),
            "expected 'invalid bracket content', got: {err}"
        );
    }

    // ── edge cases ─────────────────────────────────────────────────

    #[test]
    fn parse_empty_string_returns_empty_selector() {
        let sel = parse("").unwrap();
        assert!(sel.is_empty());
    }

    #[test]
    fn parse_leading_dot_is_ignored() {
        assert_eq!(parse(".name").unwrap(), vec![Segment::Key("name".into())]);
    }

    #[test]
    fn parse_leading_slash_is_root_and_stripped() {
        // Agents often emit JSON Pointer style `/feature_flag` (#1794).
        assert_eq!(
            parse("/feature_flag").unwrap(),
            vec![Segment::Key("feature_flag".into())]
        );
        assert_eq!(
            parse("/server.port").unwrap(),
            vec![Segment::Key("server".into()), Segment::Key("port".into())]
        );
        // Only one leading slash is special.
        assert_eq!(parse("//a").unwrap(), vec![Segment::Key("/a".into())]);
        assert!(parse("/").unwrap().is_empty());
    }

    #[test]
    fn parse_consecutive_dots_are_ignored() {
        assert_eq!(
            parse("a..b").unwrap(),
            vec![Segment::Key("a".into()), Segment::Key("b".into())]
        );
    }

    #[test]
    fn parse_index_at_start() {
        let sel = parse("[0].name").unwrap();
        assert_eq!(sel, vec![Segment::Index(0), Segment::Key("name".into())]);
    }

    #[test]
    fn parse_adjacent_brackets() {
        let sel = parse("[0][1]").unwrap();
        assert_eq!(sel, vec![Segment::Index(0), Segment::Index(1)]);
    }

    #[test]
    fn parse_predicate_value_with_equals() {
        let sel = parse("items[url=a=b]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                Segment::Predicate {
                    key: "url".into(),
                    value: "a=b".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_predicate_value_with_brackets() {
        // A predicate value containing brackets (e.g. regex character class)
        // should be parsed correctly without truncating at the inner `]`.
        let sel = parse("items[pattern=[0-9]]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                Segment::Predicate {
                    key: "pattern".into(),
                    value: "[0-9]".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_nested_brackets_in_value() {
        // Deeply nested brackets should be handled.
        let sel = parse("data[regex=[a[b]c]]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("data".into()),
                Segment::Predicate {
                    key: "regex".into(),
                    value: "[a[b]c]".into(),
                },
            ]
        );
    }
}
