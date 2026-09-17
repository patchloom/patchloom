pub mod eval;
pub mod parser;

pub use eval::{eval, eval_result};
pub use parser::{PredicateOp, Segment, Selector, parse, split_predicate};

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
/// For flat keys (no dots), this is an object key lookup, or an array
/// index when the value is an array and the key parses as `usize`.
/// For dotted keys, it first tries a direct `get(key)` to handle literal
/// dot-containing keys (e.g. `"my.key"`), then falls back to walking
/// each dot-separated segment. On ties, the first-found result wins
/// (direct lookup takes priority). Object key `"0"` wins over indexing.
pub fn get_nested<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    // Fast path: no dots means plain key lookup (or array index).
    if !key.contains('.') {
        return get_path_segment(value, key);
    }
    // Try direct lookup first (handles literal-dot keys like "my.key").
    if let Some(v) = value.get(key) {
        return Some(v);
    }
    // Fall back to dotted path traversal.
    let mut current = value;
    for segment in key.split('.') {
        current = get_path_segment(current, segment)?;
    }
    Some(current)
}

/// Object key first; numeric segment indexes an array (#2522).
fn get_path_segment<'a>(
    current: &'a serde_json::Value,
    segment: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(obj) = current.as_object() {
        return obj.get(segment);
    }
    if current.is_array()
        && let Ok(idx) = segment.parse::<usize>()
    {
        return current.get(idx);
    }
    None
}

/// Check whether a JSON value matches a predicate string.
/// JSON numbers compare numerically when the operand parses as a number;
/// string fields stay string-equal. Booleans use their string form.
///
/// Equality wrapper around [`value_matches`].
pub fn value_matches_str(field: &serde_json::Value, pred_val: &str) -> bool {
    value_matches(field, PredicateOp::Eq, pred_val).unwrap_or(false)
}

/// Compare `field` against `pred_val` using `op`.
///
/// Numeric compares (`>`, `>=`, `<`, `<=`) accept a JSON number or a string
/// that parses as a finite number. Integer operands compare exactly when
/// both sides are integers; otherwise `f64`. A present non-numeric field is
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
            if let Some(ord) = cmp_numeric(field, pred_val) {
                return Ok(match op {
                    PredicateOp::Gt => ord.is_gt(),
                    PredicateOp::Ge => ord.is_ge(),
                    PredicateOp::Lt => ord.is_lt(),
                    PredicateOp::Le => ord.is_le(),
                    PredicateOp::Eq | PredicateOp::Ne | PredicateOp::Not => unreachable!(),
                });
            }
            if as_f64(field).is_none() && as_i64(field).is_none() && as_u64(field).is_none() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "selector comparison requires a numeric field, found {}",
                        value_type_name(field)
                    ),
                });
            }
            Err(crate::exit::InvalidInputError {
                msg: format!("comparison operand must be numeric (got '{pred_val}')"),
            })
        }
    }
}

fn eq_field(field: &serde_json::Value, pred_val: &str) -> bool {
    match field {
        serde_json::Value::String(s) => s == pred_val,
        serde_json::Value::Number(n) => number_eq(n, pred_val),
        serde_json::Value::Bool(b) => b.to_string() == pred_val,
        serde_json::Value::Null => pred_val == "null",
        _ => false,
    }
}

fn number_eq(n: &serde_json::Number, pred_val: &str) -> bool {
    if let (Some(lhs), Ok(rhs)) = (n.as_i64(), pred_val.parse::<i64>()) {
        return lhs == rhs;
    }
    if let (Some(lhs), Ok(rhs)) = (n.as_u64(), pred_val.parse::<u64>()) {
        return lhs == rhs;
    }
    if let (Some(lhs), Some(rhs)) = (n.as_f64(), parse_finite_f64(pred_val)) {
        return lhs == rhs;
    }
    n.to_string() == pred_val
}

fn cmp_numeric(field: &serde_json::Value, pred_val: &str) -> Option<std::cmp::Ordering> {
    if let Some(lhs) = as_i64(field) {
        if let Ok(rhs) = pred_val.parse::<i64>() {
            return Some(lhs.cmp(&rhs));
        }
    }
    if let Some(lhs) = as_u64(field) {
        if let Ok(rhs) = pred_val.parse::<u64>() {
            return Some(lhs.cmp(&rhs));
        }
    }
    let lhs = as_f64(field)?;
    let rhs = parse_finite_f64(pred_val)?;
    lhs.partial_cmp(&rhs)
}

fn parse_finite_f64(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|f| f.is_finite())
}

fn as_i64(field: &serde_json::Value) -> Option<i64> {
    match field {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn as_u64(field: &serde_json::Value) -> Option<u64> {
    match field {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn as_f64(field: &serde_json::Value) -> Option<f64> {
    match field {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => parse_finite_f64(s),
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

    // ── #2522 get_nested indexes arrays ────────────────────────────

    #[test]
    fn get_nested_numeric_segment_indexes_array() {
        let data = json!({"steps": [{"name": "build"}, {"name": "test"}]});
        assert_eq!(get_nested(&data, "steps.0.name"), Some(&json!("build")));
        assert_eq!(get_nested(&data, "steps.1.name"), Some(&json!("test")));
    }

    #[test]
    fn get_nested_object_key_zero_wins() {
        let data = json!({"items": {"0": "zero-key", "1": "one-key"}});
        assert_eq!(get_nested(&data, "items.0"), Some(&json!("zero-key")));
    }

    #[test]
    fn get_nested_array_index_without_dots() {
        let data = json!([10, 20, 30]);
        assert_eq!(get_nested(&data, "1"), Some(&json!(20)));
    }

    // ── #2523 numeric = / != ───────────────────────────────────────

    #[test]
    fn eq_field_number_matches_integer_and_exponent_spelling() {
        let float_8080 =
            serde_json::Value::Number(serde_json::Number::from_f64(8080.0).expect("finite 8080.0"));
        assert!(value_matches_str(&float_8080, "8080"));
        assert!(value_matches_str(&json!(80), "80.0"));
        assert!(value_matches_str(&json!(1000), "1e3"));
    }

    #[test]
    fn eq_field_number_ne_treats_8080_and_8080_point_zero_equal() {
        let float_8080 =
            serde_json::Value::Number(serde_json::Number::from_f64(8080.0).expect("finite 8080.0"));
        assert!(!value_matches(&float_8080, PredicateOp::Ne, "8080").unwrap());
    }

    #[test]
    fn eq_field_string_stays_string_equal() {
        assert!(value_matches_str(&json!("8080"), "8080"));
        assert!(!value_matches_str(&json!("8080"), "8080.0"));
        assert!(!value_matches_str(&json!("1e3"), "1000"));
    }

    // ── #2524 integer-exact compare ────────────────────────────────

    #[test]
    fn value_matches_large_integer_gt_is_exact() {
        assert!(
            value_matches(
                &json!(9007199254740993i64),
                PredicateOp::Gt,
                "9007199254740992"
            )
            .unwrap()
        );
        assert!(
            !value_matches(
                &json!(9007199254740992i64),
                PredicateOp::Gt,
                "9007199254740992"
            )
            .unwrap()
        );
    }
}
