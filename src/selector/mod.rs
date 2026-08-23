pub mod eval;
pub mod parser;

pub use eval::{eval, eval_result};
pub use parser::{PredicateOp, Segment, Selector, parse};

/// Parse a selector string, mapping parse errors to `anyhow::Error` with
/// a "selector error:" prefix for consistent error formatting.
pub fn parse_anyhow(input: &str) -> anyhow::Result<Selector> {
    parse(input).map_err(|e| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!("selector error: {e}"),
        })
    })
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
///
/// Equality wrapper around [`value_matches`].
pub fn value_matches_str(field: &serde_json::Value, pred_val: &str) -> bool {
    value_matches(field, PredicateOp::Eq, pred_val).unwrap_or(false)
}

/// Compare `field` against `pred_val` using `op`.
///
/// Numeric compares (`>`, `>=`, `<`, `<=`) accept a JSON number or a string
/// that parses as `f64`. A present non-numeric field is
/// [`InvalidInputError`](crate::exit::InvalidInputError), not a lexicographic
/// compare. [`PredicateOp::Not`] is handled at the item level by
/// [`item_matches_predicate`]; if called directly it matches JSON `false` or
/// `null` only.
pub fn value_matches(
    field: &serde_json::Value,
    op: PredicateOp,
    pred_val: &str,
) -> Result<bool, crate::exit::InvalidInputError> {
    match op {
        PredicateOp::Eq => Ok(eq_field(field, pred_val)),
        PredicateOp::Ne => Ok(!eq_field(field, pred_val)),
        PredicateOp::Not => Ok(field.is_null() || field == &serde_json::Value::Bool(false)),
        PredicateOp::Gt | PredicateOp::Ge | PredicateOp::Lt | PredicateOp::Le => {
            let Some(lhs) = as_f64(field) else {
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "selector comparison requires a numeric field, found {}",
                        value_type_name(field)
                    ),
                });
            };
            let rhs = pred_val
                .parse::<f64>()
                .map_err(|_| crate::exit::InvalidInputError {
                    msg: format!("comparison operand must be numeric (got '{pred_val}')"),
                })?;
            Ok(match op {
                PredicateOp::Gt => lhs > rhs,
                PredicateOp::Ge => lhs >= rhs,
                PredicateOp::Lt => lhs < rhs,
                PredicateOp::Le => lhs <= rhs,
                PredicateOp::Eq | PredicateOp::Ne | PredicateOp::Not => unreachable!(),
            })
        }
    }
}

fn eq_field(field: &serde_json::Value, pred_val: &str) -> bool {
    match field {
        serde_json::Value::String(s) => s == pred_val,
        serde_json::Value::Number(n) => n.to_string() == pred_val,
        serde_json::Value::Bool(b) => b.to_string() == pred_val,
        serde_json::Value::Null => pred_val == "null",
        _ => false,
    }
}

fn as_f64(field: &serde_json::Value) -> Option<f64> {
    match field {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn value_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Whether `item` satisfies `[key op value]` / `[!key]`.
///
/// Missing fields: no match for `=`, `!=`, and numeric compares (same as
/// historical equality). [`PredicateOp::Not`] matches absent, JSON `false`,
/// or `null`.
pub fn item_matches_predicate(
    item: &serde_json::Value,
    key: &str,
    op: PredicateOp,
    pred_val: &str,
) -> Result<bool, crate::exit::InvalidInputError> {
    match get_nested(item, key) {
        None => Ok(op == PredicateOp::Not),
        Some(field) => {
            if op == PredicateOp::Not {
                return Ok(field.is_null() || field == &serde_json::Value::Bool(false));
            }
            value_matches(field, op, pred_val)
        }
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

    #[test]
    fn value_matches_numeric_gt() {
        assert!(value_matches(&json!(10), PredicateOp::Gt, "5").unwrap());
        assert!(!value_matches(&json!(5), PredicateOp::Gt, "5").unwrap());
        assert!(value_matches(&json!("10"), PredicateOp::Gt, "5").unwrap());
    }

    #[test]
    fn value_matches_non_numeric_is_invalid_input() {
        let err = value_matches(&json!("abc"), PredicateOp::Gt, "5").unwrap_err();
        assert!(
            err.msg.contains("numeric"),
            "expected numeric error, got: {}",
            err.msg
        );
    }
}
