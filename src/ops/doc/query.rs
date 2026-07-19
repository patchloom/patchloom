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
    let results = selector::eval(root, &segments);
    if results.is_empty() {
        if let Some(hint) = array_root_bare_key_hint(root, &segments) {
            return Err(crate::exit::TypeErrorError { msg: hint }.into());
        }
        return Ok(QueryResult::NoMatch);
    }
    Ok(QueryResult::Values(results.into_iter().cloned().collect()))
}

/// Actionable error when a bare object key is used at an array root.
fn array_root_bare_key_hint(
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

/// Check whether a selector path exists.
///
/// Soft `false` when the path is simply missing. When the document root is an
/// array (multi-document YAML or top-level JSON array) and the selector starts
/// with a bare object key, returns the same [`crate::exit::TypeErrorError`] as
/// [`query_get`] so agents do not treat a shape mistake as "key absent".
pub fn query_has(root: &serde_json::Value, selector: &str) -> anyhow::Result<bool> {
    let segments = selector::parse_anyhow(selector)?;
    let found = !selector::eval(root, &segments).is_empty();
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
    NotAnObject,
}

/// Get the keys of an object at a selector path.
///
/// When the selector matches multiple values, returns keys of the first match.
pub fn query_keys(root: &serde_json::Value, selector: &str) -> anyhow::Result<QueryKeysResult> {
    let segments = selector::parse_anyhow(selector)?;
    let results = selector::eval(root, &segments);
    if results.is_empty() {
        return Ok(QueryKeysResult::NoMatch);
    }
    match results[0].as_object() {
        Some(obj) => Ok(QueryKeysResult::Keys(obj.keys().cloned().collect())),
        None => Ok(QueryKeysResult::NotAnObject),
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
/// When the selector matches multiple values, returns the length of the first match.
pub fn query_len(root: &serde_json::Value, selector: &str) -> anyhow::Result<QueryLenResult> {
    let segments = selector::parse_anyhow(selector)?;
    let results = selector::eval(root, &segments);
    if results.is_empty() {
        return Ok(QueryLenResult::NoMatch);
    }
    let target = results[0];
    let len = target
        .as_array()
        .map(|a| a.len())
        .or_else(|| target.as_object().map(|o| o.len()));
    match len {
        Some(n) => Ok(QueryLenResult::Len(n)),
        None => Ok(QueryLenResult::NotArrayOrObject),
    }
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
            QueryKeysResult::NotAnObject
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
}
