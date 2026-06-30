pub mod eval;
pub mod parser;

pub use eval::eval;
pub use parser::{Segment, Selector, parse};

/// Parse a selector string, mapping parse errors to `anyhow::Error` with
/// a "selector error:" prefix for consistent error formatting.
pub fn parse_anyhow(input: &str) -> anyhow::Result<Selector> {
    parse(input).map_err(|e| anyhow::anyhow!("selector error: {e}"))
}

/// Navigate a dotted path like `"settings.theme"` into a JSON value.
///
/// For flat keys (no dots), this is equivalent to `value.get(key)`.
/// For dotted keys, it first tries a direct `get(key)` to handle literal
/// dot-containing keys (e.g. `"my.key"`), then falls back to walking
/// each dot-separated segment. On ties, the first-found result wins
/// (direct lookup takes priority).
pub fn get_nested<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    // Fast path: no dots means plain key lookup.
    if !key.contains('.') {
        return value.get(key);
    }
    // Try direct lookup first (handles literal-dot keys like "my.key").
    if let Some(v) = value.get(key) {
        return Some(v);
    }
    // Fall back to dotted path traversal.
    let mut current = value;
    for segment in key.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Check whether a JSON value matches a predicate string using string comparison.
/// Numbers and booleans are compared via their string representation.
pub fn value_matches_str(field: &serde_json::Value, pred_val: &str) -> bool {
    match field {
        serde_json::Value::String(s) => s == pred_val,
        serde_json::Value::Number(n) => n.to_string() == pred_val,
        serde_json::Value::Bool(b) => b.to_string() == pred_val,
        serde_json::Value::Null => pred_val == "null",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_matches_str_string() {
        assert!(value_matches_str(&json!("hello"), "hello"));
        assert!(!value_matches_str(&json!("hello"), "world"));
    }

    #[test]
    fn value_matches_str_number() {
        assert!(value_matches_str(&json!(42), "42"));
        assert!(!value_matches_str(&json!(42), "43"));
    }

    #[test]
    fn value_matches_str_bool() {
        assert!(value_matches_str(&json!(true), "true"));
        assert!(value_matches_str(&json!(false), "false"));
        assert!(!value_matches_str(&json!(true), "false"));
    }

    /// Null values match the string "null" (#1164).
    #[test]
    fn value_matches_str_null_matches_null_string() {
        assert!(value_matches_str(&json!(null), "null"));
        assert!(!value_matches_str(&json!(null), "other"));
    }

    #[test]
    fn get_nested_flat_key() {
        let data = json!({"name": "Alice"});
        assert_eq!(get_nested(&data, "name"), Some(&json!("Alice")));
    }

    #[test]
    fn get_nested_dotted_path() {
        let data = json!({"settings": {"theme": "dark"}});
        assert_eq!(get_nested(&data, "settings.theme"), Some(&json!("dark")));
    }

    #[test]
    fn get_nested_deep_path() {
        let data = json!({"a": {"b": {"c": 42}}});
        assert_eq!(get_nested(&data, "a.b.c"), Some(&json!(42)));
    }

    #[test]
    fn get_nested_literal_dot_key_takes_priority() {
        // A key literally named "a.b" should match before dotted traversal.
        let data = json!({"a.b": "literal", "a": {"b": "nested"}});
        assert_eq!(get_nested(&data, "a.b"), Some(&json!("literal")));
    }

    #[test]
    fn get_nested_missing_returns_none() {
        let data = json!({"a": {"b": 1}});
        assert_eq!(get_nested(&data, "a.c"), None);
    }

    #[test]
    fn value_matches_str_object_returns_false() {
        assert!(!value_matches_str(&json!({"a": 1}), ""));
    }
}
