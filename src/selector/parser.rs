/// A selector is a sequence of segments that navigate through a JSON value tree.
pub type Selector = Vec<Segment>;

/// Comparison operator inside a [`Segment::Predicate`].
///
/// Equality (`Eq`) is the default and matches historical `key=value`.
/// Numeric compares require a finite operand at parse time. Regex is not
/// supported. `[!key]` is [`PredicateOp::Not`]. `[!key=value]` is
/// [`PredicateOp::Ne`].
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
/// A leading `!` is peeled before the operator scan so `[!key=value]` is
/// [`PredicateOp::Ne`] rather than equality on a key named `!key` (#2520).
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
        reject_question_prefix(key, "", content)?;
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
///
/// Shared with `doc delete-where` so `!=` `>=` `<=` `>` `<` match the
/// selector grammar (#2483). A leading `!` is peeled first: `!key=value`
/// is [`PredicateOp::Ne`]; `!` plus `>` / `>=` / `<` / `<=` is rejected
/// with a hint to `[key!=value]` (#2520).
pub fn split_predicate(content: &str) -> Result<Option<(String, PredicateOp, String)>, String> {
    let (body, bang) = match content.strip_prefix('!') {
        Some(rest) if !rest.starts_with('=') => (rest, true),
        _ => (content, false),
    };
    let Some((key_end, op, value_start)) = find_predicate_op(body) else {
        return Ok(None);
    };
    let op = if bang {
        if op == PredicateOp::Eq {
            PredicateOp::Ne
        } else {
            return Err(format!(
                "'!' cannot combine with {op}; use [key!=value] instead of [{content}]"
            ));
        }
    } else {
        op
    };
    let key = &body[..key_end];
    let mut value = body[value_start..].to_string();
    if key.is_empty() {
        return Err("empty predicate key".to_string());
    }
    reject_question_prefix(key, &value, content)?;
    if op.is_numeric_compare() {
        let trimmed = value.trim();
        if !trimmed.parse::<f64>().is_ok_and(f64::is_finite) {
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

fn reject_question_prefix(key: &str, value: &str, original: &str) -> Result<(), String> {
    if let Some(stripped) = key.strip_prefix('?') {
        let negated = original.starts_with('!');
        let suggestion = if negated && value.is_empty() {
            format!("[!{stripped}]")
        } else if negated {
            format!("[!{stripped}={value}]")
        } else if value.is_empty() {
            format!("[{stripped}]")
        } else {
            format!("[{stripped}={value}]")
        };
        return Err(format!(
            "predicate key starts with '?'; use {suggestion} instead of [{original}]"
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

            match parse_bracket_content(content) {
                Ok(seg) => segments.push(seg),
                Err(e) if e.starts_with("invalid bracket content:") => {
                    let prefix = &input[..start.saturating_sub(1)];
                    let examples = if prefix.is_empty() {
                        "[0], [*], [name=…], [!name]".to_string()
                    } else {
                        format!("{prefix}[0], {prefix}[*], {prefix}[name=…], {prefix}[!name]")
                    };
                    return Err(format!("{e} (try {examples})"));
                }
                Err(e) => return Err(e),
            }
        } else if bytes[i] == b'"' {
            // Quoted key matching flatten (`"a.b"`, `\"` escape). #2482
            i += 1;
            let mut key = String::new();
            let mut escaped = false;
            let mut closed = false;
            while i < len {
                let ch = match input[i..].chars().next() {
                    Some(c) => c,
                    None => break,
                };
                i += ch.len_utf8();
                if escaped {
                    key.push(ch);
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    closed = true;
                    break;
                }
                key.push(ch);
            }
            if escaped || !closed {
                return Err("unclosed quoted key in selector".to_string());
            }
            segments.push(Segment::Key(key));
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

    #[test]
    fn parse_jq_style_bracket_key_suggests_forms() {
        let err = parse("items[name]").unwrap_err();
        assert!(
            err.contains("invalid bracket content: name"),
            "expected invalid bracket content, got: {err}"
        );
        assert!(
            err.contains("items[0]")
                && err.contains("items[*]")
                && err.contains("items[name=…]")
                && err.contains("items[!name]"),
            "expected jq-guess forms, got: {err}"
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

    #[test]
    fn parse_quoted_dot_key() {
        assert_eq!(parse(r#""a.b""#).unwrap(), vec![Segment::Key("a.b".into())]);
    }

    #[test]
    fn parse_quoted_key_nested() {
        assert_eq!(
            parse(r#"outer."a.b""#).unwrap(),
            vec![Segment::Key("outer".into()), Segment::Key("a.b".into())]
        );
        assert_eq!(
            parse(r#""a.b".inner"#).unwrap(),
            vec![Segment::Key("a.b".into()), Segment::Key("inner".into())]
        );
    }

    #[test]
    fn parse_quoted_key_escaped_quote() {
        assert_eq!(
            parse(r#""has\"quote""#).unwrap(),
            vec![Segment::Key(r#"has"quote"#.into())]
        );
    }

    #[test]
    fn parse_quoted_key_with_brackets() {
        assert_eq!(
            parse(r#""a[0]""#).unwrap(),
            vec![Segment::Key("a[0]".into())]
        );
    }

    #[test]
    fn parse_unclosed_quoted_key_errors() {
        let err = parse(r#""a.b"#).unwrap_err();
        assert!(
            err.contains("unclosed") && err.contains("quoted"),
            "expected unclosed quoted key, got: {err}"
        );
    }

    #[test]
    fn parse_unquoted_dot_still_splits() {
        assert_eq!(
            parse("a.b").unwrap(),
            vec![Segment::Key("a".into()), Segment::Key("b".into())]
        );
    }

    // ── #2520 peel leading ! before split_predicate ────────────────

    #[test]
    fn parse_bang_eq_is_ne() {
        let sel = parse("items[!status=done]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("items".into()),
                pred("status", PredicateOp::Ne, "done"),
            ]
        );
    }

    #[test]
    fn split_predicate_bang_eq_is_ne() {
        let (key, op, value) = split_predicate("!status=done").unwrap().unwrap();
        assert_eq!(key, "status");
        assert_eq!(op, PredicateOp::Ne);
        assert_eq!(value, "done");
    }

    #[test]
    fn parse_bang_gt_is_rejected() {
        let err = parse("items[!port>1]").unwrap_err();
        assert!(
            err.contains("[key!=value]"),
            "expected hint to [key!=value], got: {err}"
        );
        assert!(
            err.contains('!') || err.contains("cannot combine"),
            "expected ! / combine diagnostic, got: {err}"
        );
    }

    #[test]
    fn parse_bang_ge_lt_le_are_rejected() {
        for sel in ["items[!n>=1]", "items[!n<1]", "items[!n<=1]"] {
            let err = parse(sel).unwrap_err();
            assert!(
                err.contains("[key!=value]"),
                "expected [key!=value] hint for {sel}, got: {err}"
            );
        }
    }

    #[test]
    fn split_predicate_bang_gt_is_rejected() {
        let err = split_predicate("!port>1").unwrap_err();
        assert!(
            err.contains("[key!=value]"),
            "expected [key!=value] hint for delete-where, got: {err}"
        );
    }

    #[test]
    fn parse_bang_key_still_not() {
        let sel = parse("flags[!deprecated]").unwrap();
        assert_eq!(
            sel,
            vec![
                Segment::Key("flags".into()),
                pred("deprecated", PredicateOp::Not, ""),
            ]
        );
    }

    // ── #2524 reject non-finite compare operands ───────────────────

    #[test]
    fn parse_nan_compare_operand_errors() {
        for sel in [
            "items[port>NaN]",
            "items[port<=inf]",
            "items[port>infinity]",
            "items[n<-inf]",
        ] {
            let err = parse(sel).unwrap_err();
            assert!(
                err.contains("numeric") || err.contains("finite") || err.contains("comparison"),
                "expected non-finite operand error for {sel}, got: {err}"
            );
        }
    }

    #[test]
    fn split_predicate_rejects_nan_and_inf() {
        for pred in ["port>NaN", "port<=inf", "n>infinity"] {
            let err = split_predicate(pred).unwrap_err();
            assert!(
                err.contains("numeric") || err.contains("finite") || err.contains("comparison"),
                "expected non-finite operand error for {pred}, got: {err}"
            );
        }
    }

    // ── #2525 [!?name] hint ────────────────────────────────────────

    #[test]
    fn parse_bang_question_name_suggests_bang_name() {
        let err = parse("items[!?name]").unwrap_err();
        assert!(
            err.contains("[!name]"),
            "expected suggestion [!name], got: {err}"
        );
        assert!(
            err.contains("[!?name]"),
            "expected original input quoted, got: {err}"
        );
        assert!(
            !err.contains("[?name=]"),
            "must not misquote as [?name=], got: {err}"
        );
    }
}
