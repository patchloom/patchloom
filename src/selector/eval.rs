use super::parser::{Segment, Selector};

/// Evaluate a selector against a JSON value tree.
///
/// Returns all matching leaf values.  For wildcards and predicates the
/// result may contain more than one entry.
pub fn eval<'a>(value: &'a serde_json::Value, selector: &Selector) -> Vec<&'a serde_json::Value> {
    crate::verbose!("selector: evaluating {:?}", selector);
    let mut current = vec![value];

    for segment in selector {
        let mut next = Vec::new();
        for val in current {
            match segment {
                Segment::Key(key) => {
                    if let Some(v) = val.get(key.as_str()) {
                        next.push(v);
                    }
                }
                Segment::Index(idx) => {
                    if let Some(v) = val.get(*idx) {
                        next.push(v);
                    }
                }
                Segment::Wildcard => {
                    if let Some(arr) = val.as_array() {
                        next.extend(arr.iter());
                    }
                }
                Segment::Predicate {
                    key,
                    value: pred_val,
                } => {
                    if let Some(arr) = val.as_array() {
                        for item in arr {
                            if let Some(field) = item.get(key.as_str())
                                && crate::selector::value_matches_str(field, pred_val)
                            {
                                next.push(item);
                            }
                        }
                    }
                }
            }
        }
        current = next;
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::parser::parse;
    use serde_json::json;

    #[test]
    fn eval_simple_key_path() {
        let data = json!({"scripts": {"test": "jest"}});
        let sel = parse("scripts.test").unwrap();
        let results = eval(&data, &sel);
        let expected = json!("jest");
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_array_index() {
        let data = json!({"items": [10, 20, 30]});
        let sel = parse("items[1]").unwrap();
        let results = eval(&data, &sel);
        let expected = json!(20);
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_wildcard_collects_all() {
        let data = json!({"steps": [{"name": "a"}, {"name": "b"}, {"name": "c"}]});
        let sel = parse("steps[*].name").unwrap();
        let results = eval(&data, &sel);
        let a = json!("a");
        let b = json!("b");
        let c = json!("c");
        assert_eq!(results, vec![&a, &b, &c]);
    }

    #[test]
    fn eval_predicate_filters() {
        let data = json!({
            "jobs": [
                {"id": "build", "timeout": 10},
                {"id": "test", "timeout": 30}
            ]
        });
        let sel = parse("jobs[id=test].timeout").unwrap();
        let results = eval(&data, &sel);
        let expected = json!(30);
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_missing_key_returns_empty() {
        let data = json!({"a": 1});
        let sel = parse("b").unwrap();
        let results = eval(&data, &sel);
        assert!(results.is_empty());
    }

    // ── edge cases ─────────────────────────────────────────────────

    #[test]
    fn eval_empty_selector_returns_root() {
        let data = json!({"a": 1});
        let sel = parse("").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results, vec![&data]);
    }

    #[test]
    fn eval_index_out_of_bounds_returns_empty() {
        let data = json!({"items": [10, 20]});
        let sel = parse("items[99]").unwrap();
        let results = eval(&data, &sel);
        assert!(results.is_empty());
    }

    #[test]
    fn eval_wildcard_on_non_array_returns_empty() {
        let data = json!({"name": "hello"});
        let sel = parse("name[*]").unwrap();
        let results = eval(&data, &sel);
        assert!(results.is_empty());
    }

    #[test]
    fn eval_predicate_on_empty_array() {
        let data = json!({"items": []});
        let sel = parse("items[id=x]").unwrap();
        let results = eval(&data, &sel);
        assert!(results.is_empty());
    }
}
