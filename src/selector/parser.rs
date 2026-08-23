/// A selector is a sequence of segments that navigate through a JSON value tree.
pub type Selector = Vec<Segment>;

/// Comparison operator inside a [`Segment::Predicate`].
///
/// Equality (`Eq`) is the default and matches historical `key=value`.
/// Numeric compares require an `f64` operand at parse time. Regex is not
/// supported. `[!key]` is [`PredicateOp::Not`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PredicateOp {
    /// `key=value` (default).
    #[default]
    Eq,
    /// `key!=value`. Missing fields do not match.
    Ne,
    /// `key>N` (numeric).
    Gt,
    /// `key>=N` (numeric).
    Ge,
    /// `key<N` (numeric).
    Lt,
    /// `key<=N` (numeric).
    Le,
    /// `[!key]`: field is absent, JSON `false`, or `null`.
    Not,
}

impl PredicateOp {
    /// True for `>`, `>=`, `<`, and `<=`.
    pub fn is_numeric_compare(self) -> bool {
        matches!(self, Self::Gt | Self::Ge | Self::Lt | Self::Le)
    }
}

/// A single segment in a selector path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Object key access, e.g. `scripts`.
    Key(String),
    /// Array index access, e.g. `[0]`.
    Index(usize),
    /// Wildcard – matches all array elements: `[*]`.
    Wildcard,
    /// Predicate filter on array or object-map elements, e.g. `[name=api]`.
    Predicate {
        key: String,
        op: PredicateOp,
        value: String,
    },
}

/// Parse one `[...]` body into a segment.
///
/// Operator scan is left-to-right. At each index, two-character operators
/// (`!=`, `>=`, `<=`) are tried before `=`, then `>` and `<`. Searching `>`
/// before `=` would treat `items[url=a>b]` as a greater-than compare.
fn parse_bracket_content(content: &str) -> Result<Segment, String> {
    if content == "*" {
        return Ok(Segment::Wildcard);
    }

    if let Some((key, op, value)) = split_predicate(content)? {
        return Ok(Segment::Predicate { key, op, value });
    }

    if let Some(key) = content.strip_prefix('!') {
        if key.is_empty() {
            return Err("empty predicate key".to_string());
        }
        reject_question_prefix(key, "")?;
        return Ok(Segment::Predicate {
            key: key.to_string(),
            op: PredicateOp::Not,
            value: String::new(),
        });
    }

    if let Ok(idx) = content.parse::<usize>() {
        return Ok(Segment::Index(idx));
    }
    Err(format!("invalid bracket content: {content}"))
}

/// Split `key<op>value` if a comparison or equality operator is present.
fn split_predicate(content: &str) -> Result<Option<(String, PredicateOp, String)>, String> {
    let Some((key_end, op, value_start)) = find_predicate_op(content) else {
        return Ok(None);
    };
    let key = &content[..key_end];
    let mut value = content[value_start..].to_string();
    if key.is_empty() {
        return Err("empty predicate key".to_string());
    }
    reject_question_prefix(key, &value)?;
    if op.is_numeric_compare() {
        let trimmed = value.trim();
        if trimmed.parse::<f64>().is_err() {
            return Err(format!(
                "comparison operand must be numeric (got '{value}' after {op})"
            ));
        }
        value = trimmed.to_string();
    }
    Ok(Some((key.to_string(), op, value)))
}

/// First operator in `content`. Two-character forms win at the same index.
///
/// Walk bytes and never slice `content[i..]` (a mid-character index is not
/// a UTF-8 boundary). Operators are ASCII, so the returned indices are
/// valid split points.
fn find_predicate_op(content: &str) -> Option<(usize, PredicateOp, usize)> {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'!' && bytes.get(i + 1) == Some(&b'=') {
            return Some((i, PredicateOp::Ne, i + 2));
        }
        if bytes[i] == b'>' && bytes.get(i + 1) == Some(&b'=') {
            return Some((i, PredicateOp::Ge, i + 2));
        }
        if bytes[i] == b'<' && bytes.get(i + 1) == Some(&b'=') {
            return Some((i, PredicateOp::Le, i + 2));
        }
        match bytes[i] {
            b'=' => return Some((i, PredicateOp::Eq, i + 1)),
            b'>' => return Some((i, PredicateOp::Gt, i + 1)),
            b'<' => return Some((i, PredicateOp::Lt, i + 1)),
            _ => i += 1,
        }
    }
    None
}

fn reject_question_prefix(key: &str, value: &str) -> Result<(), String> {
    if let Some(stripped) = key.strip_prefix('?') {
        let suggestion = if value.is_empty() {
            format!("[{stripped}]")
        } else {
            format!("[{stripped}={value}]")
        };
        return Err(format!(
            "predicate key starts with '?'; use {suggestion} instead of [{key}={value}]"
        ));
    }
    Ok(())
}

impl std::fmt::Display for PredicateOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Not => "!",
        })
    }
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

            segments.push(parse_bracket_content(content)?);
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
                    op: PredicateOp::Eq,
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
                    op: PredicateOp::Eq,
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
                    op: PredicateOp::Eq,
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
                    op: PredicateOp::Eq,
                    value: "[a[b]c]".into(),
                },
            ]
        );
    }

    // ── #2230 comparison and negation predicates ───────────────────

    fn pred(key: &str, op: PredicateOp, value: &str) -> Segment {
        Segment::Predicate {
            key: key.into(),
            op,
            value: value.into(),
        }
    }

    #[test]
    fn parse_gt_predicate() {
        let sel = parse("servers[port>8000]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("servers".into()),
                pred("port", PredicateOp::Gt, "8000")
            ]
        );
    }

    #[test]
    fn parse_ne_predicate() {
        let sel = parse("items[status!=done]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("status", PredicateOp::Ne, "done")
            ]
        );
    }

    #[test]
    fn parse_ge_lt_le_predicates() {
        assert_eq!(
            parse("servers[port>=8000]").unwrap(),
            vec![
                Segment::Key("servers".into()),
                pred("port", PredicateOp::Ge, "8000")
            ]
        );
        assert_eq!(
            parse("servers[port<8000]").unwrap(),
            vec![
                Segment::Key("servers".into()),
                pred("port", PredicateOp::Lt, "8000")
            ]
        );
        assert_eq!(
            parse("servers[port<=8000]").unwrap(),
            vec![
                Segment::Key("servers".into()),
                pred("port", PredicateOp::Le, "8000")
            ]
        );
    }

    #[test]
    fn parse_equality_value_may_contain_equals() {
        let sel = parse("items[url=a=b]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("url", PredicateOp::Eq, "a=b")
            ]
        );
    }

    #[test]
    fn parse_equality_value_may_contain_gt() {
        // Regression vs scanning `>` before `=`.
        let sel = parse("items[url=a>b]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("url", PredicateOp::Eq, "a>b")
            ]
        );
    }

    #[test]
    fn parse_negation_predicate() {
        let sel = parse("flags[!deprecated]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("flags".into()),
                pred("deprecated", PredicateOp::Not, ""),
            ]
        );
    }

    #[test]
    fn parse_non_numeric_comparison_operand_errors() {
        let err = parse("items[port>abc]").unwrap_err();
        assert!(
            err.contains("numeric") || err.contains("comparison"),
            "expected numeric/comparison parse error, got: {err}"
        );
    }

    #[test]
    fn parse_jobs_id_test_still_eq() {
        let sel = parse("jobs[id=test]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("jobs".into()),
                pred("id", PredicateOp::Eq, "test")
            ]
        );
    }

    #[test]
    fn parse_non_ascii_key_equality_does_not_panic() {
        let sel = parse("items[名前=x]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("名前", PredicateOp::Eq, "x")
            ]
        );
    }

    #[test]
    fn parse_non_ascii_key_gt_does_not_panic() {
        let sel = parse("items[café>1]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("café", PredicateOp::Gt, "1")
            ]
        );
    }

    #[test]
    fn parse_chained_eq_and_gt() {
        let sel = parse("data[type=server][port>8000]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("data".into()),
                pred("type", PredicateOp::Eq, "server"),
                pred("port", PredicateOp::Gt, "8000"),
            ]
        );
    }
}
