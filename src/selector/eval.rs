use super::parser::{Segment, Selector};
use crate::exit::InvalidInputError;
use crate::selector::{get_nested, item_matches_predicate};

/// Evaluate a selector against a JSON value tree.
///
/// Returns all matching leaf values. For wildcards and predicates the
/// result may contain more than one entry.
///
/// Comparison type mismatches (a present non-numeric field vs `>` / `>=` /
/// `<` / `<=`) are treated as no match. Use [`eval_result`] when the
/// caller must surface those as `invalid_input`.
pub fn eval<'a>(value: &'a serde_json::Value, selector: &Selector) -> Vec<&'a serde_json::Value> {
    eval_result(value, selector).unwrap_or_default()
}

/// Like [`eval`], but a present non-numeric field vs a numeric compare
/// returns [`InvalidInputError`].
pub fn eval_result<'a>(
    value: &'a serde_json::Value,
    selector: &Selector,
) -> Result<Vec<&'a serde_json::Value>, InvalidInputError> {
    crate::verbose!("selector: evaluating {:?}", selector);
    let mut current = vec![value];

    for segment in selector {
        let mut next = Vec::new();
        for val in current {
            match segment {
                Segment::Key(key) => {
                    if let Some(v) = val.get(key.as_str()) {
                        next.push(v);
                    } else if val.is_array()
                        // Numeric dot-notation: `env.0.value` → `env[0].value` (#1288)
                        && let Ok(idx) = key.parse::<usize>()
                        && let Some(v) = val.get(idx)
                    {
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
                    } else if let Some(obj) = val.as_object() {
                        next.extend(obj.values());
                    }
                }
                Segment::Predicate {
                    key,
                    op,
                    value: pred_val,
                } => {
                    for item in predicate_candidates(val, key) {
                        if item_matches_predicate(item, key, *op, pred_val)? {
                            next.push(item);
                        }
                    }
                }
            }
        }
        current = next;
    }

    Ok(current)
}

/// Items a predicate should test.
///
/// Arrays: each element. Objects: the object itself when it already has
/// `key` (or has no object-valued children); otherwise each value, so
/// `services[name=api]` still filters a map of objects. This also lets
/// chained `data[type=server][port>8000]` test each selected item.
fn predicate_candidates<'a>(val: &'a serde_json::Value, key: &str) -> Vec<&'a serde_json::Value> {
    if let Some(arr) = val.as_array() {
        return arr.iter().collect();
    }
    let Some(obj) = val.as_object() else {
        return Vec::new();
    };
    if get_nested(val, key).is_some() {
        return vec![val];
    }
    if obj.values().any(|v| v.is_object() || v.is_array()) {
        return obj.values().collect();
    }
    vec![val]
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

    // ── object wildcard/predicate tests (#1111.6) ──────────────

    #[test]
    fn eval_wildcard_on_object_iterates_values() {
        let data = json!({
            "servers": {
                "web": {"port": 80},
                "api": {"port": 8080},
                "db":  {"port": 5432}
            }
        });
        let sel = parse("servers[*].port").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 3, "should match all 3 server ports");
        let ports: Vec<i64> = results.iter().filter_map(|v| v.as_i64()).collect();
        assert!(ports.contains(&80));
        assert!(ports.contains(&8080));
        assert!(ports.contains(&5432));
    }

    #[test]
    fn eval_predicate_on_object_filters_values() {
        let data = json!({
            "services": {
                "web": {"name": "web", "port": 80},
                "api": {"name": "api", "port": 8080}
            }
        });
        let sel = parse("services[name=api].port").unwrap();
        let results = eval(&data, &sel);
        let expected = json!(8080);
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_predicate_nested_path() {
        // #1246: predicates should support dotted paths like settings.theme
        let data = json!({
            "users": [
                {"name": "Alice", "settings": {"theme": "dark"}},
                {"name": "Bob", "settings": {"theme": "light"}},
                {"name": "Charlie", "settings": {"theme": "dark"}}
            ]
        });
        let sel = parse("users[settings.theme=dark].name").unwrap();
        let results = eval(&data, &sel);
        let alice = json!("Alice");
        let charlie = json!("Charlie");
        assert_eq!(results, vec![&alice, &charlie]);
    }

    #[test]
    fn eval_wildcard_on_nested_object() {
        let data = json!({
            "config": {
                "dev":  {"debug": true},
                "prod": {"debug": false}
            }
        });
        let sel = parse("config[*].debug").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 2);
    }

    // ── #1288: numeric dot-notation as array index ─────────────────

    #[test]
    fn eval_numeric_dot_notation_as_array_index() {
        let data = json!({"env": [{"name": "A", "value": "1"}, {"name": "B", "value": "2"}]});
        let sel = parse("env.0.value").unwrap();
        let results = eval(&data, &sel);
        let expected = json!("1");
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_numeric_dot_notation_second_element() {
        let data = json!({"items": ["alpha", "beta", "gamma"]});
        let sel = parse("items.1").unwrap();
        let results = eval(&data, &sel);
        let expected = json!("beta");
        assert_eq!(results, vec![&expected]);
    }

    #[test]
    fn eval_numeric_dot_notation_out_of_bounds_returns_empty() {
        let data = json!({"items": [10, 20]});
        let sel = parse("items.99").unwrap();
        let results = eval(&data, &sel);
        assert!(results.is_empty());
    }

    #[test]
    fn eval_numeric_key_on_object_prefers_object_key() {
        // When an object has a key that is a numeric string, it should
        // be treated as an object key, not an array index.
        let data = json!({"data": {"0": "zero-key", "1": "one-key"}});
        let sel = parse("data.0").unwrap();
        let results = eval(&data, &sel);
        let expected = json!("zero-key");
        assert_eq!(results, vec![&expected]);
    }

    // ── #2230 comparison and negation ──────────────────────────────

    fn servers_doc() -> serde_json::Value {
        json!({
            "servers": [
                {"name": "web", "port": 80},
                {"name": "api", "port": 8080},
                {"name": "edge", "port": 8000}
            ]
        })
    }

    #[test]
    fn eval_numeric_gt() {
        let data = servers_doc();
        let sel = parse("servers[port>8000]").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], json!("api"));
    }

    #[test]
    fn eval_numeric_ge_lt_le() {
        let data = servers_doc();
        assert_eq!(eval(&data, &parse("servers[port>=8000]").unwrap()).len(), 2);
        assert_eq!(eval(&data, &parse("servers[port<8000]").unwrap()).len(), 1);
        assert_eq!(eval(&data, &parse("servers[port<=8000]").unwrap()).len(), 2);
    }

    #[test]
    fn eval_numeric_string_field_still_compares() {
        let data = json!({"items": [{"n": "10"}, {"n": "3"}]});
        let sel = parse("items[n>5]").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["n"], json!("10"));
    }

    #[test]
    fn eval_ne_matches_other_strings_not_missing() {
        let data = json!({
            "items": [
                {"status": "done"},
                {"status": "open"},
                {"name": "no-status"}
            ]
        });
        let sel = parse("items[status!=done]").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["status"], json!("open"));
    }

    #[test]
    fn eval_not_matches_absent_false_null_not_true() {
        let data = json!({
            "flags": [
                {"name": "a"},
                {"name": "b", "deprecated": false},
                {"name": "c", "deprecated": null},
                {"name": "d", "deprecated": true}
            ]
        });
        let sel = parse("flags[!deprecated]").unwrap();
        let results = eval(&data, &sel);
        let names: Vec<&str> = results
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn eval_existing_name_eq_still_works() {
        let data = json!({"items": [{"name": "a"}, {"name": "b"}]});
        let sel = parse("items[name=b]").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], json!("b"));
    }

    #[test]
    fn eval_result_non_numeric_field_is_invalid_input() {
        let data = json!({"items": [{"port": "abc"}]});
        let sel = parse("items[port>8000]").unwrap();
        let err = eval_result(&data, &sel).expect_err("non-numeric field vs >");
        assert!(
            err.msg.contains("numeric"),
            "expected numeric type error, got: {}",
            err.msg
        );
    }

    #[test]
    fn eval_chained_eq_and_gt() {
        let data = json!({
            "data": [
                {"type": "server", "port": 9000},
                {"type": "server", "port": 80},
                {"type": "client", "port": 9000}
            ]
        });
        let sel = parse("data[type=server][port>8000]").unwrap();
        let results = eval(&data, &sel);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["port"], json!(9000));
        assert_eq!(results[0]["type"], json!("server"));
    }
}
