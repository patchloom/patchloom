pub mod eval;
pub mod parser;

pub use eval::eval;
pub use parser::{parse, Segment, Selector};

/// Check whether a JSON value matches a predicate string using string comparison.
/// Numbers and booleans are compared via their string representation.
pub fn value_matches_str(field: &serde_json::Value, pred_val: &str) -> bool {
    match field {
        serde_json::Value::String(s) => s == pred_val,
        serde_json::Value::Number(n) => n.to_string() == pred_val,
        serde_json::Value::Bool(b) => b.to_string() == pred_val,
        _ => false,
    }
}
