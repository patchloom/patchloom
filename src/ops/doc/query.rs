//! Pure read-only query functions for JSON/YAML/TOML documents.
//!
//! These operate on parsed `serde_json::Value` trees and return
//! structured results with no IO or formatting. Called by CLI,
//! MCP, and the public library API.

use crate::selector;

/// Result of a query that targets a selector path.
///
/// The caller decides how to format or use the values.
#[derive(Debug)]
pub enum QueryResult {
    /// One or more values matched the selector.
    Values(Vec<serde_json::Value>),
    /// The selector matched nothing.
    NoMatch,
}

/// Query values at a selector path.
///
/// Returns cloned values so the caller owns them.
///
/// When the document root is an array (multi-document YAML or a top-level JSON
/// array) and the selector begins with a non-numeric key, returns
/// [`crate::exit::TypeErrorError`] with an index form hint (`0.key` / `[0].key`)
/// instead of soft [`QueryResult::NoMatch`]. Matches write-path honesty for
/// multi-doc bare keys (docs/reference multi-document YAML; fixrealloop).
pub fn query_get(root: &serde_json::Value, selector: &str) -> anyhow::Result<QueryResult> {
    let segments = selector::parse_anyhow(selector)?;
    let results = selector::eval_result(root, &segments)?;
    if results.is_empty() {
        if let Some(hint) = array_root_bare_key_hint(root, &segments) {
            return Err(crate::exit::TypeErrorError { msg: hint }.into());
        }
        return Ok(QueryResult::NoMatch);
    }
    Ok(QueryResult::Values(results.into_iter().cloned().collect()))
}

/// Actionable error when a bare object key is used at an array root.
pub(crate) fn array_root_bare_key_hint(
    root: &serde_json::Value,
    segments: &[selector::Segment],
) -> Option<String> {
    if !root.is_array() {
        return None;
    }
    let selector::Segment::Key(k) = segments.first()? else {
        return None;
    };
    // Numeric keys are array indices via dot notation (#1288).
    if k.parse::<usize>().is_ok() {
        return None;
    }
    Some(format!(
        "parent is an array, not an object (for multi-document YAML or \
         top-level arrays, address a document/element with an index first, \
         e.g. 0.{k} or [0].{k})"
    ))
}

/// Render selector segments in canonical bracket form (`items[0].name`).
fn format_selector(segments: &[selector::Segment]) -> String {
    let mut out = String::new();
    for seg in segments {
        match seg {
            selector::Segment::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            selector::Segment::Index(n) => out.push_str(&format!("[{n}]")),
            selector::Segment::Wildcard => out.push_str("[*]"),
            selector::Segment::Predicate { key, op, value } => {
                out.push('[');
                match op {
                    selector::PredicateOp::Not => {
                        out.push('!');
                        out.push_str(key);
                    }
                    selector::PredicateOp::Eq => {
                        out.push_str(key);
                        out.push('=');
                        out.push_str(value);
                    }
                    selector::PredicateOp::Ne => {
                        out.push_str(key);
                        out.push_str("!=");
                        out.push_str(value);
                    }
                    selector::PredicateOp::Gt => {
                        out.push_str(key);
                        out.push('>');
                        out.push_str(value);
                    }
                    selector::PredicateOp::Ge => {
                        out.push_str(key);
                        out.push_str(">=");
                        out.push_str(value);
                    }
                    selector::PredicateOp::Lt => {
                        out.push_str(key);
                        out.push('<');
                        out.push_str(value);
                    }
                    selector::PredicateOp::Le => {
                        out.push_str(key);
                        out.push_str("<=");
                        out.push_str(value);
                    }
                }
                out.push(']');
            }
        }
    }
    out
}

/// Concrete index paths for a multi-match keys/len selector (`items[*]` → `items[0]`).
fn unique_index_examples(segments: &[selector::Segment]) -> (String, String) {
    let pos = segments.iter().position(|s| {
        matches!(
            s,
            selector::Segment::Wildcard | selector::Segment::Predicate { .. }
        )
    });
    match pos {
        Some(i) => {
            let mut first = segments.to_vec();
            let mut second = segments.to_vec();
            first[i] = selector::Segment::Index(0);
            second[i] = selector::Segment::Index(1);
            (format_selector(&first), format_selector(&second))
        }
        None => ("items[0]".into(), "items[1]".into()),
    }
}

fn selector_has_multi_match_segment(segments: &[selector::Segment]) -> bool {
    segments.iter().any(|s| {
        matches!(
            s,
            selector::Segment::Wildcard | selector::Segment::Predicate { .. }
        )
    })
}

fn unique_match_required_msg(op: &str, selector: &str, segments: &[selector::Segment]) -> String {
    let (a, b) = unique_index_examples(segments);
    let need = if op == "keys" {
        "one object"
    } else {
        "one object or array"
    };
    format!(
        "doc {op}: selector '{selector}' is not a unique path; \
         {op} needs {need} (e.g. {a} or {b})"
    )
}

fn parent_selector_display(segments: &[selector::Segment]) -> String {
    if segments.len() <= 1 {
        return ".".into();
    }
    let formatted = format_selector(&segments[..segments.len() - 1]);
    if formatted.is_empty() {
        ".".into()
    } else {
        formatted
    }
}

/// Unique target for keys/len: parse, refuse wildcard/predicate, eval once.
fn unique_query_target<'a>(
    root: &'a serde_json::Value,
    selector: &str,
    op: &str,
) -> anyhow::Result<Option<&'a serde_json::Value>> {
    let segments = selector::parse_anyhow(selector)?;
    if selector_has_multi_match_segment(&segments) {
        return Err(crate::exit::AmbiguousError {
            msg: unique_match_required_msg(op, selector, &segments),
        }
        .into());
    }
    let results = selector::eval_result(root, &segments)?;
    if results.is_empty() {
        if let Some(hint) = array_root_bare_key_hint(root, &segments) {
            return Err(crate::exit::TypeErrorError { msg: hint }.into());
        }
        return Ok(None);
    }
    if results.len() > 1 {
        return Err(crate::exit::AmbiguousError {
            msg: unique_match_required_msg(op, selector, &segments),
        }
        .into());
    }
    Ok(Some(results[0]))
}

/// Check whether a selector path exists.
///
/// Soft `false` when the path is simply missing. When the document root is an
/// array (multi-document YAML or top-level JSON array) and the selector starts
/// with a bare object key, returns the same [`crate::exit::TypeErrorError`] as
/// [`query_get`] so agents do not treat a shape mistake as "key absent".
pub fn query_has(root: &serde_json::Value, selector: &str) -> anyhow::Result<bool> {
    let segments = selector::parse_anyhow(selector)?;
    let found = !selector::eval_result(root, &segments)?.is_empty();
    if !found && let Some(hint) = array_root_bare_key_hint(root, &segments) {
        return Err(crate::exit::TypeErrorError { msg: hint }.into());
    }
    Ok(found)
}

/// Result of a keys query.
#[derive(Debug)]
pub enum QueryKeysResult {
    Keys(Vec<String>),
    NoMatch,
    /// The value at the selector is not an object.
    NotAnObject {
        /// True when the unique match is an array (hint `keys on items[0]`).
        is_array: bool,
    },
}

/// Get the keys of an object at a selector path.
///
/// A selector with a wildcard or predicate (`items[*]`, `items[name=foo]`)
/// is always [`crate::exit::AmbiguousError`] (0- and 1-match included). When
/// the selector matches more than one value, returns the same error. Bare
/// object keys at an array root (multi-doc YAML) return
/// [`crate::exit::TypeErrorError`] like [`query_get`] / [`query_has`].
pub fn query_keys(root: &serde_json::Value, selector: &str) -> anyhow::Result<QueryKeysResult> {
    let Some(target) = unique_query_target(root, selector, "keys")? else {
        return Ok(QueryKeysResult::NoMatch);
    };
    if let Some(obj) = target.as_object() {
        Ok(QueryKeysResult::Keys(obj.keys().cloned().collect()))
    } else {
        Ok(QueryKeysResult::NotAnObject {
            is_array: target.is_array(),
        })
    }
}

/// Result of a len query.
#[derive(Debug)]
pub enum QueryLenResult {
    Len(usize),
    NoMatch,
    /// The value at the selector is not an array or object.
    NotArrayOrObject,
}

/// Get the length of an array or object at a selector path.
///
/// A selector with a wildcard or predicate is always
/// [`crate::exit::AmbiguousError`] (0- and 1-match included). When the
/// selector matches more than one value, returns the same error. Bare object
/// keys at an array root return type_error like [`query_get`].
pub fn query_len(root: &serde_json::Value, selector: &str) -> anyhow::Result<QueryLenResult> {
    let Some(target) = unique_query_target(root, selector, "len")? else {
        return Ok(QueryLenResult::NoMatch);
    };
    let len = target
        .as_array()
        .map(|a| a.len())
        .or_else(|| target.as_object().map(|o| o.len()));
    match len {
        Some(n) => Ok(QueryLenResult::Len(n)),
        None => Ok(QueryLenResult::NotArrayOrObject),
    }
}

fn is_root_selector(selector: &str) -> bool {
    selector.is_empty() || selector == "."
}

fn query_selector(selector: &str) -> &str {
    if is_root_selector(selector) {
        ""
    } else {
        selector
    }
}

/// Object keys at a selector, mapped to typed errors for API and CLI.
///
/// Empty / `"."` are the document root. Failures are
/// [`crate::exit::NoMatchError`], [`crate::exit::TypeErrorError`], or
/// [`crate::exit::AmbiguousError`].
pub fn keys_at(root: &serde_json::Value, selector: &str) -> anyhow::Result<Vec<String>> {
    let query = query_selector(selector);
    match query_keys(root, query)? {
        QueryKeysResult::NoMatch => Err(crate::exit::NoMatchError {
            msg: with_similar_object_key_hint(
                format!("no match for selector: {selector}"),
                root,
                query,
            ),
        }
        .into()),
        QueryKeysResult::NotAnObject { is_array } => {
            let msg = if is_root_selector(selector) && is_array {
                "doc keys: target is a top-level array (multi-document YAML or JSON \
                 array); use a document/element index first, e.g. keys on `0` or `[0]`"
                    .to_string()
            } else {
                let display = if is_root_selector(selector) {
                    "."
                } else {
                    selector
                };
                if is_array {
                    format!(
                        "doc keys: target at '{display}' is an array, not an object; \
                         use keys on {display}[0] or len on {display}"
                    )
                } else {
                    let parent = parent_selector_display(&selector::parse_anyhow(selector)?);
                    format!(
                        "doc keys: target at '{display}' is a scalar, not an object; \
                         use keys on {parent}"
                    )
                }
            };
            Err(crate::exit::TypeErrorError { msg }.into())
        }
        QueryKeysResult::Keys(keys) => Ok(keys),
    }
}

/// Array or object length at a selector, mapped to typed errors for API and CLI.
///
/// Empty / `"."` are the document root. Failures are
/// [`crate::exit::NoMatchError`], [`crate::exit::TypeErrorError`], or
/// [`crate::exit::AmbiguousError`].
pub fn len_at(root: &serde_json::Value, selector: &str) -> anyhow::Result<usize> {
    let query = query_selector(selector);
    match query_len(root, query)? {
        QueryLenResult::NoMatch => Err(crate::exit::NoMatchError {
            msg: with_similar_object_key_hint(
                format!("no match for selector: {selector}"),
                root,
                query,
            ),
        }
        .into()),
        QueryLenResult::NotArrayOrObject => {
            let display = if is_root_selector(selector) {
                "."
            } else {
                selector
            };
            let parent = parent_selector_display(&selector::parse_anyhow(selector)?);
            Err(crate::exit::TypeErrorError {
                msg: format!(
                    "doc len: target at '{display}' is a scalar, not an array or object; \
                     use len on {parent}"
                ),
            }
            .into())
        }
        QueryLenResult::Len(n) => Ok(n),
    }
}

/// Closest sibling object key for a missing selector path, if any.
///
/// Walks concrete key/index segments until the first miss, then ranks
/// sibling keys as whole strings (no identifier tokenization).
pub(crate) fn similar_object_key_hint(root: &serde_json::Value, selector: &str) -> Option<String> {
    let segments = selector::parse(selector).ok()?;
    let mut current = root;
    for segment in &segments {
        match segment {
            selector::Segment::Key(key) => {
                if let Some(next) = current.get(key.as_str()) {
                    current = next;
                    continue;
                }
                if current.is_array()
                    && let Ok(idx) = key.parse::<usize>()
                    && let Some(next) = current.get(idx)
                {
                    current = next;
                    continue;
                }
                let obj = current.as_object()?;
                return crate::fallback::find_similar_among(obj.keys().map(String::as_str), key, 1)
                    .into_iter()
                    .next();
            }
            selector::Segment::Index(idx) => {
                current = current.get(*idx)?;
            }
            selector::Segment::Wildcard | selector::Segment::Predicate { .. } => {
                return None;
            }
        }
    }
    None
}

/// Append a sibling-key did-you-mean when one is close.
///
/// Shared by CLI `doc get`/`keys`/`len`, library `api::doc_get`/`doc_keys`/`doc_len`, write-nav
/// NoMatch, and plan `doc.update` so whole-key hints (hyphenated names)
/// are not read-only.
pub(crate) fn with_similar_object_key_hint(
    mut msg: String,
    root: &serde_json::Value,
    selector: &str,
) -> String {
    if let Some(hint) = similar_object_key_hint(root, selector) {
        msg.push_str(&format!(" (did you mean: {hint}?)"));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> serde_json::Value {
        serde_json::json!({
            "name": "test",
            "version": "1.0",
            "items": [1, 2, 3],
            "nested": {
                "key": "value"
            }
        })
    }

    // -- query_get --

    #[test]
    fn get_existing_key() {
        let doc = sample_doc();
        match query_get(&doc, "name").unwrap() {
            QueryResult::Values(v) => assert_eq!(v, vec![serde_json::json!("test")]),
            QueryResult::NoMatch => panic!("expected match"),
        }
    }

    #[test]
    fn get_nested_key() {
        let doc = sample_doc();
        match query_get(&doc, "nested.key").unwrap() {
            QueryResult::Values(v) => assert_eq!(v, vec![serde_json::json!("value")]),
            QueryResult::NoMatch => panic!("expected match"),
        }
    }

    #[test]
    fn get_missing_key() {
        let doc = sample_doc();
        assert!(matches!(
            query_get(&doc, "nonexistent").unwrap(),
            QueryResult::NoMatch
        ));
    }

    #[test]
    fn get_array_element() {
        let doc = sample_doc();
        match query_get(&doc, "items[0]").unwrap() {
            QueryResult::Values(v) => assert_eq!(v, vec![serde_json::json!(1)]),
            QueryResult::NoMatch => panic!("expected match"),
        }
    }

    #[test]
    fn get_bare_key_on_array_root_type_error_with_index_hint() {
        // Multi-document YAML / top-level array: bare key must not look like a
        // soft no_matches (agents widen the wrong thing).
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        let err = query_get(&doc, "a").expect_err("bare key at array root");
        let msg = err.to_string();
        assert!(
            crate::exit::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        assert!(
            msg.contains("array")
                && (msg.contains("0.a") || msg.contains("[0].a"))
                && msg.contains("index"),
            "actionable multi-doc hint missing: {msg}"
        );
    }

    #[test]
    fn get_indexed_key_on_array_root_still_matches() {
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        match query_get(&doc, "0.a").unwrap() {
            QueryResult::Values(v) => assert_eq!(v, vec![serde_json::json!(1)]),
            QueryResult::NoMatch => panic!("expected match"),
        }
        match query_get(&doc, "[1].b").unwrap() {
            QueryResult::Values(v) => assert_eq!(v, vec![serde_json::json!(2)]),
            QueryResult::NoMatch => panic!("expected match"),
        }
    }

    #[test]
    fn get_missing_key_inside_doc_still_no_match() {
        // Document 0 is an object; missing nested key stays soft no_matches.
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        assert!(matches!(
            query_get(&doc, "0.missing").unwrap(),
            QueryResult::NoMatch
        ));
    }

    // -- query_has --

    #[test]
    fn has_existing() {
        assert!(query_has(&sample_doc(), "name").unwrap());
    }

    #[test]
    fn has_missing() {
        assert!(!query_has(&sample_doc(), "nonexistent").unwrap());
    }

    #[test]
    fn has_nested() {
        assert!(query_has(&sample_doc(), "nested.key").unwrap());
    }

    #[test]
    fn has_bare_key_on_array_root_type_error_with_index_hint() {
        // Multi-doc / top-level array: bare key must not soft-return false
        // (agents treat that as "absent" and never try 0.key).
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        let err = query_has(&doc, "a").expect_err("bare key at array root");
        assert!(
            crate::exit::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("0.a") || msg.contains("[0].a"),
            "hint missing index form: {msg}"
        );
    }

    #[test]
    fn has_indexed_key_on_array_root_ok() {
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        assert!(query_has(&doc, "0.a").unwrap());
        assert!(!query_has(&doc, "0.missing").unwrap());
    }

    // -- query_keys --

    #[test]
    fn keys_of_object() {
        let doc = sample_doc();
        match query_keys(&doc, "nested").unwrap() {
            QueryKeysResult::Keys(k) => assert_eq!(k, vec!["key"]),
            other => panic!("expected Keys, got {other:?}"),
        }
    }

    #[test]
    fn keys_of_root() {
        let doc = sample_doc();
        match query_keys(&doc, "").unwrap() {
            QueryKeysResult::Keys(k) => {
                assert!(k.contains(&"name".to_string()));
                assert!(k.contains(&"items".to_string()));
            }
            other => panic!("expected Keys, got {other:?}"),
        }
    }

    #[test]
    fn keys_of_array_returns_not_object() {
        let doc = sample_doc();
        assert!(matches!(
            query_keys(&doc, "items").unwrap(),
            QueryKeysResult::NotAnObject { is_array: true }
        ));
    }

    #[test]
    fn keys_missing_returns_no_match() {
        let doc = sample_doc();
        assert!(matches!(
            query_keys(&doc, "nonexistent").unwrap(),
            QueryKeysResult::NoMatch
        ));
    }

    #[test]
    fn keys_wildcard_multi_match_is_ambiguous() {
        let doc = serde_json::json!({"items":[{"a":1},{"b":2}]});
        let err = query_keys(&doc, "items[*]").expect_err("multi-match keys");
        assert!(
            crate::exit::is_ambiguous(&err),
            "expected AmbiguousError, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            (msg.contains("items[0]") || msg.contains("[0]"))
                && (msg.contains("items[1]") || msg.contains("[1]"))
                && msg.contains("one object")
                && !msg.contains("or array"),
            "ambiguous keys must name a concrete index and one object: {msg}"
        );
        let mapped = keys_at(&doc, "items[*]").expect_err("mapper");
        assert!(
            crate::exit::is_ambiguous(&mapped),
            "keys_at must propagate AmbiguousError, got: {mapped}"
        );
        let mapped_msg = mapped.to_string();
        assert!(
            mapped_msg.contains("items[0]") || mapped_msg.contains("[0]"),
            "keys_at ambiguous must name items[0]: {mapped_msg}"
        );
    }

    #[test]
    fn len_wildcard_multi_match_is_ambiguous() {
        let doc = serde_json::json!({"items":[{"a":1},{"b":2}]});
        let err = query_len(&doc, "items[*]").expect_err("multi-match len");
        assert!(
            crate::exit::is_ambiguous(&err),
            "expected AmbiguousError, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            (msg.contains("items[0]") || msg.contains("[0]"))
                && msg.contains("one object or array"),
            "ambiguous len must name a concrete index: {msg}"
        );
        let mapped = len_at(&doc, "items[*]").expect_err("mapper");
        assert!(
            crate::exit::is_ambiguous(&mapped),
            "len_at must propagate AmbiguousError, got: {mapped}"
        );
        let mapped_msg = mapped.to_string();
        assert!(
            mapped_msg.contains("items[0]") || mapped_msg.contains("[0]"),
            "len_at ambiguous must name items[0]: {mapped_msg}"
        );
    }

    #[test]
    fn keys_wildcard_one_or_zero_match_is_ambiguous() {
        for items in [serde_json::json!([{"a": 1}]), serde_json::json!([])] {
            let doc = serde_json::json!({"items": items});
            let err = query_keys(&doc, "items[*]").expect_err("wildcard keys");
            assert!(
                crate::exit::is_ambiguous(&err),
                "keys on items[*] must be Ambiguous even with 0 or 1 hit, got: {err}"
            );
            let msg = err.to_string();
            assert!(
                msg.contains("items[0]") || msg.contains("[0]"),
                "ambiguous keys must name a concrete index: {msg}"
            );
            let mapped = keys_at(&doc, "items[*]").expect_err("mapper");
            assert!(
                crate::exit::is_ambiguous(&mapped),
                "keys_at must be Ambiguous not NoMatch, got: {mapped}"
            );
        }
    }

    #[test]
    fn len_wildcard_one_or_zero_match_is_ambiguous() {
        for items in [serde_json::json!([{"a": 1}]), serde_json::json!([])] {
            let doc = serde_json::json!({"items": items});
            let err = query_len(&doc, "items[*]").expect_err("wildcard len");
            assert!(
                crate::exit::is_ambiguous(&err),
                "len on items[*] must be Ambiguous even with 0 or 1 hit, got: {err}"
            );
            let msg = err.to_string();
            assert!(
                msg.contains("items[0]") || msg.contains("[0]"),
                "ambiguous len must name a concrete index: {msg}"
            );
            let mapped = len_at(&doc, "items[*]").expect_err("mapper");
            assert!(
                crate::exit::is_ambiguous(&mapped),
                "len_at must be Ambiguous not NoMatch, got: {mapped}"
            );
        }
    }

    #[test]
    fn keys_and_len_predicate_one_or_zero_match_is_ambiguous() {
        for items in [serde_json::json!([{"name": "a"}]), serde_json::json!([])] {
            let doc = serde_json::json!({"items": items});
            let keys_err = query_keys(&doc, "items[name=a]").expect_err("predicate keys");
            assert!(
                crate::exit::is_ambiguous(&keys_err),
                "keys on items[name=a] must be Ambiguous even with 0 or 1 hit, got: {keys_err}"
            );
            let len_err = query_len(&doc, "items[name=a]").expect_err("predicate len");
            assert!(
                crate::exit::is_ambiguous(&len_err),
                "len on items[name=a] must be Ambiguous even with 0 or 1 hit, got: {len_err}"
            );
        }
    }

    #[test]
    fn keys_at_maps_no_match_and_type_error() {
        let doc = sample_doc();
        let miss = keys_at(&doc, "nonexistent").expect_err("missing");
        assert!(
            crate::exit::is_no_match(&miss),
            "keys_at missing must be NoMatchError, got: {miss}"
        );
        let typed = keys_at(&doc, "items").expect_err("array");
        assert!(
            crate::exit::is_type_error(&typed),
            "keys_at on array must be TypeErrorError, got: {typed}"
        );
        let msg = typed.to_string();
        assert!(
            msg.contains("items[0]") && msg.contains("len on items"),
            "keys on nested array must hint keys on items[0] / len on items: {msg}"
        );
        let scalar = keys_at(&doc, "name").expect_err("scalar");
        assert!(crate::exit::is_type_error(&scalar), "got: {scalar}");
        let scalar_msg = scalar.to_string();
        assert!(
            scalar_msg.contains("scalar") && scalar_msg.contains("keys on ."),
            "keys on scalar must say scalar and point at parent: {scalar_msg}"
        );
    }

    #[test]
    fn len_at_maps_no_match_and_type_error() {
        let doc = sample_doc();
        let miss = len_at(&doc, "nonexistent").expect_err("missing");
        assert!(
            crate::exit::is_no_match(&miss),
            "len_at missing must be NoMatchError, got: {miss}"
        );
        let typed = len_at(&doc, "name").expect_err("scalar");
        assert!(
            crate::exit::is_type_error(&typed),
            "len_at on scalar must be TypeErrorError, got: {typed}"
        );
        let msg = typed.to_string();
        assert!(
            msg.contains("scalar") && msg.contains("len on ."),
            "len on scalar must say scalar and point at parent: {msg}"
        );
    }

    #[test]
    fn keys_bare_key_at_array_root_is_type_error() {
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        let err = query_keys(&doc, "a").expect_err("bare key at array root");
        assert!(
            crate::fallback::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("0.a") || msg.contains("[0].a"),
            "hint missing index form: {msg}"
        );
    }

    #[test]
    fn len_bare_key_at_array_root_is_type_error() {
        let doc = serde_json::json!([{"a": 1}, {"b": 2}]);
        let err = query_len(&doc, "a").expect_err("bare key at array root");
        assert!(
            crate::fallback::is_type_error(&err),
            "expected type_error, got: {err}"
        );
    }

    // -- query_len --

    #[test]
    fn len_of_array() {
        let doc = sample_doc();
        match query_len(&doc, "items").unwrap() {
            QueryLenResult::Len(n) => assert_eq!(n, 3),
            other => panic!("expected Len, got {other:?}"),
        }
    }

    #[test]
    fn len_of_object() {
        let doc = sample_doc();
        match query_len(&doc, "nested").unwrap() {
            QueryLenResult::Len(n) => assert_eq!(n, 1),
            other => panic!("expected Len, got {other:?}"),
        }
    }

    #[test]
    fn len_of_scalar_returns_not_array_or_object() {
        let doc = sample_doc();
        assert!(matches!(
            query_len(&doc, "name").unwrap(),
            QueryLenResult::NotArrayOrObject
        ));
    }

    #[test]
    fn len_missing_returns_no_match() {
        let doc = sample_doc();
        assert!(matches!(
            query_len(&doc, "nonexistent").unwrap(),
            QueryLenResult::NoMatch
        ));
    }

    #[test]
    fn get_port_gt_returns_matching_items() {
        let doc = serde_json::json!({
            "servers": [
                {"name": "web", "port": 80},
                {"name": "api", "port": 9000}
            ]
        });
        match query_get(&doc, "servers[port>8000]").unwrap() {
            QueryResult::Values(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0]["name"], "api");
            }
            QueryResult::NoMatch => panic!("expected match"),
        }
    }

    #[test]
    fn similar_object_key_hint_finds_typo() {
        let doc = serde_json::json!({"database": {"port": 5432}, "name": "app"});
        assert_eq!(
            similar_object_key_hint(&doc, "databse.port").as_deref(),
            Some("database")
        );
        assert_eq!(
            similar_object_key_hint(&doc, "database.prt").as_deref(),
            Some("port")
        );
    }

    #[test]
    fn similar_object_key_hint_skips_unrelated() {
        let doc = serde_json::json!({"database": {"port": 5432}});
        assert_eq!(similar_object_key_hint(&doc, "xyzzy"), None);
    }

    #[test]
    fn similar_object_key_hint_hyphenated_key_is_whole_string() {
        // Tokenizing identifiers would split `database-url` and hint the
        // absent token `database`. Rank sibling keys as whole strings.
        let doc = serde_json::json!({"database-url": 1});
        let hint = similar_object_key_hint(&doc, "databse-url");
        assert_ne!(
            hint.as_deref(),
            Some("database"),
            "must not hint a hyphen-split token that is not a key: {hint:?}"
        );
        assert!(
            hint.as_deref() == Some("database-url") || hint.is_none(),
            "expected whole key `database-url` or no hint, got {hint:?}"
        );
    }

    #[test]
    fn with_similar_object_key_hint_hyphenated_key_is_whole_string() {
        let doc = serde_json::json!({"database-url": 1});
        let msg = with_similar_object_key_hint("selector missed".into(), &doc, "databse-url");
        assert!(
            msg.contains("database-url"),
            "expected whole-key hint, got: {msg}"
        );
        assert!(
            !msg.contains("did you mean: database?"),
            "must not hint hyphen-split token `database`: {msg}"
        );
    }

    #[test]
    fn get_non_numeric_comparison_operand_is_invalid_input() {
        let doc = serde_json::json!({"servers": [{"port": 80}]});
        let err = query_get(&doc, "servers[port>abc]").expect_err("bad operand");
        assert!(
            crate::exit::is_invalid_input(&err),
            "expected invalid_input, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("numeric") || msg.contains("comparison"),
            "expected numeric/comparison message, got: {msg}"
        );
    }
}
