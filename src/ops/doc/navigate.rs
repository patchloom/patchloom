use crate::selector;

pub fn navigate_mut<'a>(
    root: &'a mut serde_json::Value,
    segments: &[selector::Segment],
    create: bool,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let mut current = root;
    for seg in segments {
        current = match seg {
            selector::Segment::Key(k) => {
                if create {
                    // Convert null root (e.g. empty YAML) to an empty object
                    // so intermediate keys can be created.
                    if current.is_null() {
                        *current = serde_json::Value::Object(serde_json::Map::new());
                    }
                    let needs_create = match current.as_object() {
                        Some(obj) => !obj.contains_key(k.as_str()),
                        None => false,
                    };
                    if needs_create {
                        current
                            .as_object_mut()
                            .ok_or_else(|| anyhow::anyhow!("not an object at key '{k}'"))?
                            .insert(k.clone(), serde_json::Value::Object(serde_json::Map::new()));
                    }
                }
                current
                    .get_mut(k.as_str())
                    .ok_or_else(|| anyhow::anyhow!("key not found: {k}"))?
            }
            selector::Segment::Index(i) => current
                .get_mut(*i)
                .ok_or_else(|| anyhow::anyhow!("index out of bounds: {i}"))?,
            _ => anyhow::bail!("wildcard/predicate not supported in write navigation"),
        };
    }
    Ok(current)
}

/// Returns the parent path and the final segment (for use by set/delete/move).
fn split_last(segments: &[selector::Segment]) -> (&[selector::Segment], &selector::Segment) {
    let (parent, last) = segments.split_at(segments.len() - 1);
    (parent, &last[0])
}

/// Set a value at the location described by `segments`.  Navigates to the
/// parent (creating intermediate keys when needed) and inserts the value at
/// the final Key or Index segment.
pub fn set_at_path(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
    value: serde_json::Value,
) -> anyhow::Result<()> {
    if segments.is_empty() {
        anyhow::bail!("empty selector");
    }
    let (parent_path, last) = split_last(segments);
    let parent = navigate_mut(root, parent_path, true)?;

    // Convert null parent (e.g. empty YAML parsed to null) to an empty
    // object so `doc set` works on empty documents.
    if parent.is_null() && matches!(last, selector::Segment::Key(_)) {
        *parent = serde_json::Value::Object(serde_json::Map::new());
    }

    match last {
        selector::Segment::Key(k) => {
            parent
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("parent is not an object"))?
                .insert(k.clone(), value);
        }
        selector::Segment::Index(i) => {
            let arr = parent
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("parent is not an array"))?;
            if *i < arr.len() {
                arr[*i] = value;
            } else {
                anyhow::bail!("index {} out of bounds (len {})", i, arr.len());
            }
        }
        _ => anyhow::bail!("cannot set at wildcard/predicate"),
    }
    Ok(())
}

/// Delete the value at the given selector path. Returns `true` if
/// something was removed, `false` if the path did not exist.
pub fn delete_at_selector(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
) -> anyhow::Result<bool> {
    if segments.is_empty() {
        return Ok(false);
    }
    let (parent_path, last) = split_last(segments);
    let parent = match navigate_mut(root, parent_path, false) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    match last {
        selector::Segment::Key(k) => {
            if let Some(obj) = parent.as_object_mut() {
                Ok(obj.remove(k.as_str()).is_some())
            } else {
                Ok(false)
            }
        }
        selector::Segment::Index(i) => {
            if let Some(arr) = parent.as_array_mut() {
                if *i < arr.len() {
                    arr.remove(*i);
                    Ok(true)
                } else {
                    Ok(false)
                }
            } else {
                Ok(false)
            }
        }
        _ => anyhow::bail!("cannot delete at wildcard/predicate"),
    }
}

/// Parse a `key=value` predicate and remove matching items from the array
/// at `segments`. Returns the number of items removed.
pub fn delete_where(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
    predicate: &str,
) -> anyhow::Result<usize> {
    let eq_pos = predicate
        .find('=')
        .ok_or_else(|| anyhow::anyhow!("predicate must be in key=value format"))?;
    let pred_key = predicate[..eq_pos].trim();
    if pred_key.is_empty() {
        anyhow::bail!("predicate key is empty; expected key=value format");
    }
    let raw_val = &predicate[eq_pos + 1..];
    if raw_val.starts_with('=') {
        anyhow::bail!("predicate uses '==' but only '=' is supported; use key=value format");
    }
    let pred_val = raw_val.trim();

    let target = navigate_mut(root, segments, false)?;
    let arr = target
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("selector does not point to an array"))?;

    let before_len = arr.len();
    arr.retain(|item| {
        item.get(pred_key)
            .is_none_or(|field| !selector::value_matches_str(field, pred_val))
    });
    Ok(before_len - arr.len())
}

/// Move a value from one path to another within the same document.
/// Removes the value at `from_segments` and inserts it at `to_segments`.
pub fn move_at_path(
    root: &mut serde_json::Value,
    from_segments: &[selector::Segment],
    to_segments: &[selector::Segment],
) -> anyhow::Result<()> {
    // Remove value at source path.
    let removed = {
        if from_segments.is_empty() {
            anyhow::bail!("empty from selector");
        }
        let (parent_path, last) = split_last(from_segments);
        let parent = navigate_mut(root, parent_path, false)?;
        match last {
            selector::Segment::Key(k) => parent
                .as_object_mut()
                .and_then(|obj| obj.remove(k.as_str()))
                .ok_or_else(|| anyhow::anyhow!("source key '{k}' not found"))?,
            selector::Segment::Index(i) => {
                let arr = parent
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("source parent is not an array"))?;
                if *i < arr.len() {
                    arr.remove(*i)
                } else {
                    anyhow::bail!("source index {i} out of bounds");
                }
            }
            _ => anyhow::bail!("cannot move from wildcard/predicate"),
        }
    };

    // Insert at destination path.
    if to_segments.is_empty() {
        anyhow::bail!("empty to selector");
    }
    let (parent_path, last) = split_last(to_segments);
    let parent = navigate_mut(root, parent_path, true)?;
    match last {
        selector::Segment::Key(k) => {
            parent
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("target parent is not an object"))?
                .insert(k.clone(), removed);
        }
        selector::Segment::Index(i) => {
            let arr = parent
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("target parent is not an array"))?;
            if *i <= arr.len() {
                arr.insert(*i, removed);
            } else {
                anyhow::bail!("target index {i} out of bounds");
            }
        }
        _ => anyhow::bail!("cannot move to wildcard/predicate"),
    }
    Ok(())
}

const MAX_MERGE_DEPTH: usize = 128;

pub fn deep_merge(base: &mut serde_json::Value, other: &serde_json::Value) {
    deep_merge_inner(base, other, 0);
}

fn deep_merge_inner(base: &mut serde_json::Value, other: &serde_json::Value, depth: usize) {
    if depth >= MAX_MERGE_DEPTH {
        *base = other.clone();
        return;
    }
    if let (Some(base_map), Some(other_map)) = (base.as_object_mut(), other.as_object()) {
        for (key, value) in other_map {
            let entry = base_map
                .entry(key.clone())
                .or_insert(serde_json::Value::Null);
            deep_merge_inner(entry, value, depth + 1);
        }
    } else {
        *base = other.clone();
    }
}

pub fn update_matching(
    value: &mut serde_json::Value,
    segments: &[selector::Segment],
    new_val: &serde_json::Value,
) -> usize {
    if segments.is_empty() {
        *value = new_val.clone();
        return 1;
    }
    let first = &segments[0];
    let rest = &segments[1..];
    match first {
        selector::Segment::Key(k) => {
            if let Some(child) = value.get_mut(k.as_str()) {
                update_matching(child, rest, new_val)
            } else {
                0
            }
        }
        selector::Segment::Index(i) => {
            if let Some(child) = value.get_mut(*i) {
                update_matching(child, rest, new_val)
            } else {
                0
            }
        }
        selector::Segment::Wildcard => {
            let mut count = 0;
            if let Some(arr) = value.as_array_mut() {
                for item in arr.iter_mut() {
                    count += update_matching(item, rest, new_val);
                }
            }
            count
        }
        selector::Segment::Predicate {
            key,
            value: pred_val,
        } => {
            let mut count = 0;
            if let Some(arr) = value.as_array_mut() {
                for item in arr.iter_mut() {
                    let matches = item
                        .get(key.as_str())
                        .is_some_and(|field| selector::value_matches_str(field, pred_val));
                    if matches {
                        count += update_matching(item, rest, new_val);
                    }
                }
            }
            count
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn segs(path: &str) -> Vec<selector::Segment> {
        selector::parse(path).unwrap()
    }

    #[test]
    fn navigate_mut_key() {
        let mut root = json!({"a": {"b": 1}});
        let val = navigate_mut(&mut root, &segs("a.b"), false).unwrap();
        assert_eq!(val, &json!(1));
    }

    #[test]
    fn navigate_mut_index() {
        let mut root = json!({"items": [10, 20, 30]});
        let val = navigate_mut(&mut root, &segs("items[1]"), false).unwrap();
        assert_eq!(val, &json!(20));
    }

    #[test]
    fn navigate_mut_missing_key_errors() {
        let mut root = json!({"a": 1});
        let result = navigate_mut(&mut root, &segs("b"), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key not found"));
    }

    #[test]
    fn navigate_mut_create_intermediate() {
        let mut root = json!({"a": {}});
        let val = navigate_mut(&mut root, &segs("a.b"), true).unwrap();
        assert!(val.is_object(), "should create intermediate object");
    }

    #[test]
    fn set_at_path_creates_key() {
        let mut root = json!({"x": {}});
        set_at_path(&mut root, &segs("x.y"), json!("hello")).unwrap();
        assert_eq!(root["x"]["y"], json!("hello"));
    }

    #[test]
    fn set_at_path_empty_selector_errors() {
        let mut root = json!({});
        let result = set_at_path(&mut root, &[], json!(1));
        assert!(result.is_err());
    }

    #[test]
    fn set_at_path_index_replaces_element() {
        let mut root = json!({"arr": [1, 2, 3]});
        set_at_path(&mut root, &segs("arr[1]"), json!(99)).unwrap();
        assert_eq!(root["arr"][1], json!(99));
    }

    #[test]
    fn delete_at_selector_removes_key() {
        let mut root = json!({"a": 1, "b": 2});
        let removed = delete_at_selector(&mut root, &segs("b")).unwrap();
        assert!(removed);
        assert!(root.get("b").is_none());
    }

    #[test]
    fn delete_at_selector_missing_returns_false() {
        let mut root = json!({"a": 1});
        let removed = delete_at_selector(&mut root, &segs("z")).unwrap();
        assert!(!removed);
    }

    #[test]
    fn delete_at_selector_empty_segments() {
        let mut root = json!({"a": 1});
        let removed = delete_at_selector(&mut root, &[]).unwrap();
        assert!(!removed);
    }

    #[test]
    fn delete_where_removes_matching_items() {
        let mut root = json!({"items": [{"name": "a"}, {"name": "b"}, {"name": "a"}]});
        let count = delete_where(&mut root, &segs("items"), "name=a").unwrap();
        assert_eq!(count, 2);
        assert_eq!(root["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn delete_where_invalid_predicate() {
        let mut root = json!({"items": []});
        let result = delete_where(&mut root, &segs("items"), "noequalssign");
        assert!(result.is_err());
    }

    #[test]
    fn delete_where_empty_key_errors() {
        let mut root = json!({"items": []});
        let result = delete_where(&mut root, &segs("items"), "=val");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key is empty"));
    }

    #[test]
    fn delete_where_double_equals_errors() {
        let mut root = json!({"items": []});
        let result = delete_where(&mut root, &segs("items"), "k==v");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("=="));
    }

    #[test]
    fn move_at_path_moves_value() {
        let mut root = json!({"src": 42, "dst": {}});
        move_at_path(&mut root, &segs("src"), &segs("dst.moved")).unwrap();
        assert!(root.get("src").is_none());
        assert_eq!(root["dst"]["moved"], json!(42));
    }

    #[test]
    fn move_at_path_empty_to_selector_fails() {
        let mut root = json!({"src": 42});
        let result = move_at_path(&mut root, &segs("src"), &[]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("empty to selector"),
            "should report empty to selector"
        );
    }

    #[test]
    fn set_at_path_on_null_root() {
        // Regression: doc set on an empty YAML file (parsed as null)
        // should convert null to an empty object, not error.
        let mut root = serde_json::Value::Null;
        set_at_path(&mut root, &segs("name"), json!("my-app")).unwrap();
        assert_eq!(root, json!({"name": "my-app"}));
    }

    #[test]
    fn navigate_mut_create_from_null() {
        // Regression: navigate_mut with create=true should handle
        // null nodes by converting them to objects.
        let mut root = serde_json::Value::Null;
        let val = navigate_mut(&mut root, &segs("a.b"), true).unwrap();
        assert!(val.is_object());
        assert!(root.is_object());
    }

    #[test]
    fn deep_merge_overlays_objects() {
        let mut base = json!({"a": 1, "b": {"c": 2}});
        let other = json!({"b": {"d": 3}, "e": 4});
        deep_merge(&mut base, &other);
        assert_eq!(base["a"], json!(1));
        assert_eq!(base["b"]["c"], json!(2));
        assert_eq!(base["b"]["d"], json!(3));
        assert_eq!(base["e"], json!(4));
    }

    #[test]
    fn update_matching_wildcard() {
        let mut root = json!({"items": [{"v": 1}, {"v": 2}]});
        let count = update_matching(&mut root, &segs("items[*].v"), &json!(0));
        assert_eq!(count, 2);
        assert_eq!(root["items"][0]["v"], json!(0));
        assert_eq!(root["items"][1]["v"], json!(0));
    }
}
