pub mod eval;
pub mod parser;

pub use eval::eval;
pub use parser::{Segment, Selector, parse};

/// Parse a selector string, mapping parse errors to `anyhow::Error` with
/// a "selector error:" prefix for consistent error formatting.
pub fn parse_anyhow(input: &str) -> anyhow::Result<Selector> {
    parse(input).map_err(|e| anyhow::anyhow!("selector error: {e}"))
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
    fn value_matches_str_object_returns_false() {
        assert!(!value_matches_str(&json!({"a": 1}), ""));
    }
}
