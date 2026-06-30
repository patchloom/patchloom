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
                    } else if let Some(obj) = val.as_object() {
                        next.extend(obj.values());
                    }
                }
                Segment::Predicate {
                    key,
                    value: pred_val,
                } => {
                    if let Some(arr) = val.as_array() {
                        for item in arr {
                            if let Some(field) = crate::selector::get_nested(item, key)
                                && crate::selector::value_matches_str(field, pred_val)
                            {
                                next.push(item);
                            }
                        }
                    } else if let Some(obj) = val.as_object() {
                        for item in obj.values() {
                            if let Some(field) = crate::selector::get_nested(item, key)
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
}
