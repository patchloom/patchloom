use crate::selector;

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

pub fn navigate_mut<'a>(
    root: &'a mut serde_json::Value,
    segments: &[selector::Segment],
    create: bool,
) -> anyhow::Result<&'a mut serde_json::Value> {
    let mut current = root;
    for seg in segments {
        current = match seg {
            selector::Segment::Key(k) => {
                // Numeric dot-notation: if the current node is an array and
                // the key is a valid non-negative integer, treat it as an
                // array index (e.g. `env.0.value` → `env[0].value`). (#1288)
                if current.is_array() {
                    if let Ok(idx) = k.parse::<usize>() {
                        let len = current.as_array().map_or(0, |a| a.len());
                        current.get_mut(idx).ok_or_else(|| {
                            anyhow::Error::new(crate::exit::InvalidInputError {
                                msg: format!(
                                    "array index {idx} out of bounds (length {len}) at '{k}'"
                                ),
                            })
                        })?
                    } else {
                        return Err(anyhow::Error::new(crate::exit::TypeErrorError {
                            msg: format!(
                                "expected object at key '{k}', found {}",
                                value_type_name(current)
                            ),
                        }));
                    }
                } else {
                    if create {
                        // Convert null or non-object intermediates to an empty
                        // object so child keys can be created.
                        if current.is_null() {
                            *current = serde_json::Value::Object(serde_json::Map::new());
                        } else if !current.is_object() {
                            anyhow::bail!(
                                "expected object at key '{k}', found {}",
                                value_type_name(current)
                            );
                        }
                        let needs_create = match current.as_object() {
                            Some(obj) => !obj.contains_key(k.as_str()),
                            None => false,
                        };
                        if needs_create {
                            current
                                .as_object_mut()
                                .ok_or_else(|| {
                                    anyhow::Error::new(crate::exit::TypeErrorError {
                                        msg: format!("not an object at key '{k}'"),
                                    })
                                })?
                                .insert(
                                    k.clone(),
                                    serde_json::Value::Object(serde_json::Map::new()),
                                );
                        }
                    }
                    current.get_mut(k.as_str()).ok_or_else(|| {
                        anyhow::Error::new(crate::exit::NoMatchError {
                            msg: format!("key not found: {k}"),
                        })
                    })?
                }
            }
            selector::Segment::Index(i) => current.get_mut(*i).ok_or_else(|| {
                anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("index out of bounds: {i}"),
                })
            })?,
            _ => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "wildcard/predicate not supported in write navigation".into(),
                }));
            }
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
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "empty selector".into(),
        }));
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
            // Numeric dot-notation on arrays (#1288).
            if parent.is_array()
                && let Ok(idx) = k.parse::<usize>()
            {
                let arr = parent.as_array_mut().expect("guarded by is_array()");
                if idx < arr.len() {
                    arr[idx] = value;
                } else {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!(
                            "array index {} out of bounds (length {}) at '{}'",
                            idx,
                            arr.len(),
                            k
                        ),
                    }));
                }
                return Ok(());
            }
            parent
                .as_object_mut()
                .ok_or_else(|| {
                    anyhow::Error::new(crate::exit::TypeErrorError {
                        msg: "parent is not an object".into(),
                    })
                })?
                .insert(k.clone(), value);
        }
        selector::Segment::Index(i) => {
            let arr = parent.as_array_mut().ok_or_else(|| {
                anyhow::Error::new(crate::exit::TypeErrorError {
                    msg: "parent is not an array".into(),
                })
            })?;
            if *i < arr.len() {
                arr[*i] = value;
            } else {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("index {} out of bounds (len {})", i, arr.len()),
                }));
            }
        }
        _ => {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "cannot set at wildcard/predicate".into(),
            }));
        }
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
            // Numeric dot-notation on arrays (#1288).
            if parent.is_array()
                && let Ok(idx) = k.parse::<usize>()
            {
                if let Some(arr) = parent.as_array_mut()
                    && idx < arr.len()
                {
                    arr.remove(idx);
                    return Ok(true);
                }
                return Ok(false);
            }
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
        _ => Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "cannot delete at wildcard/predicate".into(),
        })),
    }
}

/// Parse a `key=value` predicate and remove matching items from the array
/// at `segments`. Returns the number of items removed.
///
/// Predicate keys support dotted paths (e.g. `settings.theme=dark`) for
/// nested field matching. For simple (non-object) arrays, use `_=value`,
/// `.=value`, or `value=value` to match against the element value itself.
/// The `value=` form is accepted because agents often emit that name
/// (LLM prior) instead of `.` / `_` (fixrealloop).
pub fn delete_where(
    root: &mut serde_json::Value,
    segments: &[selector::Segment],
    predicate: &str,
) -> anyhow::Result<usize> {
    let eq_pos = predicate.find('=').ok_or_else(|| {
        anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "predicate must be in key=value format".into(),
        })
    })?;
    let pred_key = predicate[..eq_pos].trim();
    if pred_key.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "predicate key is empty; expected key=value format".into(),
        }));
    }
    let raw_val = &predicate[eq_pos + 1..];
    if raw_val.starts_with('=') {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "predicate uses '==' but only '=' is supported; use key=value format".into(),
        }));
    }
    let pred_val = raw_val.trim();

    let target = navigate_mut(root, segments, false)?;
    let arr = target.as_array_mut().ok_or_else(|| {
        anyhow::Error::new(crate::exit::TypeErrorError {
            msg: "selector does not point to an array".into(),
        })
    })?;

    let before_len = arr.len();
    if pred_key == "_" || pred_key == "." {
        // Simple value matching: compare the array item itself.
        arr.retain(|item| !selector::value_matches_str(item, pred_val));
    } else if pred_key == "value" {
        // Agents often write value=X for scalar arrays. Prefer a real field
        // named "value" on objects; fall back to element match for scalars.
        arr.retain(|item| {
            if let Some(field) = item.get("value") {
                !selector::value_matches_str(field, pred_val)
            } else if item.is_object() {
                // Same as missing field: keep the item.
                true
            } else {
                !selector::value_matches_str(item, pred_val)
            }
        });
    } else {
        arr.retain(|item| {
            selector::get_nested(item, pred_key)
                .is_none_or(|field| !selector::value_matches_str(field, pred_val))
        });
    }
    Ok(before_len - arr.len())
}

/// Move a value from one path to another within the same document.
/// Removes the value at `from_segments` and inserts it at `to_segments`.
pub fn move_at_path(
    root: &mut serde_json::Value,
    from_segments: &[selector::Segment],
    to_segments: &[selector::Segment],
) -> anyhow::Result<()> {
    if from_segments.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "empty from selector".into(),
        }));
    }
    if to_segments.is_empty() {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "empty to selector".into(),
        }));
    }

    // Same path: no-op.
    if from_segments == to_segments {
        return Ok(());
    }

    // Reject moving a path into its own descendant (would silently destroy data).
    if to_segments.len() > from_segments.len() && to_segments.starts_with(from_segments) {
        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: "cannot move a path into its own descendant: destination is under source".into(),
        }));
    }

    let (from_parent, from_last) = split_last(from_segments);
    let (to_parent, to_last) = split_last(to_segments);

    // Intra-array move: source and destination share the same parent array.
    // Use remove-then-insert so index arithmetic stays correct (#1196).
    if from_parent == to_parent
        && let (selector::Segment::Index(fi), selector::Segment::Index(ti)) = (from_last, to_last)
    {
        let parent = navigate_mut(root, from_parent, false)?;
        let arr = parent.as_array_mut().ok_or_else(|| {
            anyhow::Error::new(crate::exit::TypeErrorError {
                msg: "parent is not an array".into(),
            })
        })?;
        if *fi >= arr.len() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("source index {fi} out of bounds"),
            }));
        }
        if *ti >= arr.len() {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("target index {ti} out of bounds"),
            }));
        }
        let val = arr.remove(*fi);
        let insert_at = (*ti).min(arr.len());
        arr.insert(insert_at, val);
        return Ok(());
    }

    // Cross-container move: clone-insert-remove so the tree is not mutated
    // if destination insertion fails (#1183).
    let cloned = {
        let parent = navigate_mut(root, from_parent, false)?;
        match from_last {
            selector::Segment::Key(k) => {
                // Numeric dot-notation on arrays (#1288).
                if parent.is_array() {
                    if let Ok(idx) = k.parse::<usize>() {
                        let arr = parent.as_array().ok_or_else(|| {
                            anyhow::Error::new(crate::exit::TypeErrorError {
                                msg: "source parent is not an array".into(),
                            })
                        })?;
                        if idx < arr.len() {
                            arr[idx].clone()
                        } else {
                            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                                msg: format!("source index {idx} out of bounds"),
                            }));
                        }
                    } else {
                        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                            msg: format!("source key '{k}' not found (parent is array)"),
                        }));
                    }
                } else {
                    parent
                        .as_object()
                        .and_then(|obj| obj.get(k.as_str()))
                        .ok_or_else(|| {
                            anyhow::Error::new(crate::exit::NoMatchError {
                                msg: format!("source key '{k}' not found"),
                            })
                        })?
                        .clone()
                }
            }
            selector::Segment::Index(i) => {
                let arr = parent.as_array().ok_or_else(|| {
                    anyhow::Error::new(crate::exit::TypeErrorError {
                        msg: "source parent is not an array".into(),
                    })
                })?;
                if *i < arr.len() {
                    arr[*i].clone()
                } else {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("source index {i} out of bounds"),
                    }));
                }
            }
            _ => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "cannot move from wildcard/predicate".into(),
                }));
            }
        }
    };

    // Insert clone at destination path (creates intermediate objects).
    {
        let parent = navigate_mut(root, to_parent, true)?;
        match to_last {
            selector::Segment::Key(k) => {
                // Numeric dot-notation on arrays (#1288).
                if parent.is_array() {
                    if let Ok(idx) = k.parse::<usize>() {
                        let arr = parent.as_array_mut().ok_or_else(|| {
                            anyhow::Error::new(crate::exit::TypeErrorError {
                                msg: "target parent is not an array".into(),
                            })
                        })?;
                        if idx <= arr.len() {
                            arr.insert(idx, cloned);
                        } else {
                            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                                msg: format!("target index {idx} out of bounds"),
                            }));
                        }
                    } else {
                        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                            msg: format!("target key '{k}' not valid (parent is array)"),
                        }));
                    }
                } else {
                    parent
                        .as_object_mut()
                        .ok_or_else(|| {
                            anyhow::Error::new(crate::exit::TypeErrorError {
                                msg: "target parent is not an object".into(),
                            })
                        })?
                        .insert(k.clone(), cloned);
                }
            }
            selector::Segment::Index(i) => {
                let arr = parent.as_array_mut().ok_or_else(|| {
                    anyhow::Error::new(crate::exit::TypeErrorError {
                        msg: "target parent is not an array".into(),
                    })
                })?;
                if *i <= arr.len() {
                    arr.insert(*i, cloned);
                } else {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("target index {i} out of bounds"),
                    }));
                }
            }
            _ => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "cannot move to wildcard/predicate".into(),
                }));
            }
        }
    }

    // Destination insert succeeded; now remove from source.
    {
        let parent = navigate_mut(root, from_parent, false)?;
        match from_last {
            selector::Segment::Key(k) => {
                // Numeric dot-notation on arrays (#1288).
                if parent.is_array() {
                    if let Ok(idx) = k.parse::<usize>()
                        && let Some(arr) = parent.as_array_mut()
                        && idx < arr.len()
                    {
                        arr.remove(idx);
                    }
                } else {
                    parent
                        .as_object_mut()
                        .and_then(|obj| obj.remove(k.as_str()));
                }
            }
            selector::Segment::Index(i) => {
                let arr = parent.as_array_mut().ok_or_else(|| {
                    anyhow::Error::new(crate::exit::TypeErrorError {
                        msg: "source parent is not an array".into(),
                    })
                })?;
                arr.remove(*i);
            }
            _ => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "cannot remove from wildcard/predicate selector".into(),
                }));
            }
        }
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
            } else if value.is_array() {
                // Numeric dot-notation (#1288).
                if let Ok(idx) = k.parse::<usize>()
                    && let Some(child) = value.get_mut(idx)
                {
                    return update_matching(child, rest, new_val);
                }
                0
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
            } else if let Some(obj) = value.as_object_mut() {
                for item in obj.values_mut() {
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
                    let matches = selector::get_nested(item, key)
                        .is_some_and(|field| selector::value_matches_str(field, pred_val));
                    if matches {
                        count += update_matching(item, rest, new_val);
                    }
                }
            } else if let Some(obj) = value.as_object_mut() {
                for item in obj.values_mut() {
                    let matches = selector::get_nested(item, key)
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
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
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
        let err = set_at_path(&mut root, &[], json!(1)).expect_err("expected error");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty selector should be invalid_input: {err}"
        );
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
        result.expect_err("expected error");
    }

    #[test]
    fn delete_where_simple_string_array() {
        // #1247: delete-where with _=value on simple arrays.
        let mut root = json!({"tags": ["alpha", "beta", "gamma", "beta"]});
        let count = delete_where(&mut root, &segs("tags"), "_=beta").unwrap();
        assert_eq!(count, 2);
        assert_eq!(root["tags"], json!(["alpha", "gamma"]));
    }

    #[test]
    fn delete_where_simple_number_array() {
        let mut root = json!({"nums": [1, 2, 3, 2]});
        let count = delete_where(&mut root, &segs("nums"), "_=2").unwrap();
        assert_eq!(count, 2);
        assert_eq!(root["nums"], json!([1, 3]));
    }

    #[test]
    fn delete_where_dot_key_also_works() {
        // Alternative syntax: .=value
        let mut root = json!({"vals": ["a", "b", "c"]});
        let count = delete_where(&mut root, &segs("vals"), ".=b").unwrap();
        assert_eq!(count, 1);
        assert_eq!(root["vals"], json!(["a", "c"]));
    }

    #[test]
    fn delete_where_nested_predicate_path() {
        // #1246: dotted predicate key for delete_where.
        let mut root = json!({
            "users": [
                {"name": "Alice", "prefs": {"notify": "true"}},
                {"name": "Bob", "prefs": {"notify": "false"}}
            ]
        });
        let count = delete_where(&mut root, &segs("users"), "prefs.notify=false").unwrap();
        assert_eq!(count, 1);
        assert_eq!(root["users"].as_array().unwrap().len(), 1);
        assert_eq!(root["users"][0]["name"], json!("Alice"));
    }

    #[test]
    fn delete_where_empty_key_errors() {
        let mut root = json!({"items": []});
        let result = delete_where(&mut root, &segs("items"), "=val");
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(result.unwrap_err().to_string().contains("key is empty"));
    }

    #[test]
    fn delete_where_double_equals_errors() {
        let mut root = json!({"items": []});
        let result = delete_where(&mut root, &segs("items"), "k==v");
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("predicate uses '=='"),
            "error should mention '==' predicate misuse: {err_msg}"
        );
    }

    #[test]
    fn delete_where_mixed_array_objects_and_scalars() {
        // Mixed array containing both objects and scalar values.
        // Using _=value should only remove matching scalars, leaving objects untouched.
        let mut root = json!({"items": [1, {"name": "x"}, 2, "hello", 2]});
        let count = delete_where(&mut root, &segs("items"), "_=2").unwrap();
        assert_eq!(count, 2);
        assert_eq!(root["items"], json!([1, {"name": "x"}, "hello"]));
    }

    #[test]
    fn delete_where_mixed_array_string_match() {
        // Mixed array: string match should not affect objects or numbers.
        let mut root = json!({"items": ["keep", {"v": "drop"}, "drop", 42, "drop"]});
        let count = delete_where(&mut root, &segs("items"), "_=drop").unwrap();
        assert_eq!(count, 2);
        assert_eq!(root["items"], json!(["keep", {"v": "drop"}, 42]));
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
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
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

    // Regression: navigate_mut with create=true should produce a clear error
    // when an intermediate value is a non-object (e.g. string), not a generic
    // "expected object" that omits the actual type.
    #[test]
    fn navigate_mut_error_on_non_object_intermediate() {
        let mut root = json!({"a": "scalar"});
        let path = segs("a.b");
        let err = navigate_mut(&mut root, &path, true).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("string"),
            "error should mention the actual type 'string', got: {msg}"
        );
        assert!(
            msg.contains("'b'"),
            "error should mention the failing key 'b', got: {msg}"
        );
    }

    /// #1196: Moving an element forward within the same array.
    #[test]
    fn move_at_path_intra_array_forward() {
        let mut root = json!({"arr": [10, 20, 30]});
        move_at_path(&mut root, &segs("arr[0]"), &segs("arr[2]")).unwrap();
        assert_eq!(root["arr"], json!([20, 30, 10]));
    }

    /// #1196: Moving an element backward within the same array.
    #[test]
    fn move_at_path_intra_array_backward() {
        let mut root = json!({"arr": [10, 20, 30]});
        move_at_path(&mut root, &segs("arr[2]"), &segs("arr[0]")).unwrap();
        assert_eq!(root["arr"], json!([30, 10, 20]));
    }

    // ── #1288: numeric dot-notation on arrays ──────────────────────

    #[test]
    fn navigate_mut_numeric_dot_notation_on_array() {
        let mut root = json!({"env": [{"value": "A"}, {"value": "B"}]});
        let val = navigate_mut(&mut root, &segs("env.0.value"), false).unwrap();
        assert_eq!(val, &json!("A"));
    }

    #[test]
    fn set_at_path_numeric_dot_notation() {
        let mut root = json!({"items": [10, 20, 30]});
        set_at_path(&mut root, &segs("items.1"), json!(99)).unwrap();
        assert_eq!(root["items"][1], json!(99));
    }

    #[test]
    fn set_at_path_numeric_dot_notation_out_of_bounds() {
        let mut root = json!({"items": [10, 20]});
        let result = set_at_path(&mut root, &segs("items.99"), json!(0));
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(
            result.unwrap_err().to_string().contains("out of bounds"),
            "should report out of bounds"
        );
    }

    #[test]
    fn delete_at_selector_numeric_dot_notation() {
        let mut root = json!({"arr": ["a", "b", "c"]});
        let removed = delete_at_selector(&mut root, &segs("arr.1")).unwrap();
        assert!(removed);
        assert_eq!(root["arr"], json!(["a", "c"]));
    }

    #[test]
    fn update_matching_numeric_dot_notation() {
        let mut root = json!({"env": [{"name": "A"}, {"name": "B"}]});
        let count = update_matching(&mut root, &segs("env.0.name"), &json!("X"));
        assert_eq!(count, 1);
        assert_eq!(root["env"][0]["name"], json!("X"));
    }

    #[test]
    fn move_at_path_numeric_dot_notation() {
        let mut root = json!({"src": ["a", "b"], "dst": {}});
        move_at_path(&mut root, &segs("src.0"), &segs("dst.moved")).unwrap();
        assert_eq!(root["dst"]["moved"], json!("a"));
        assert_eq!(root["src"], json!(["b"]));
    }

    /// #1196: Moving a key to itself must be a no-op, not a deletion.
    #[test]
    fn move_at_path_same_key_is_noop() {
        let mut root = json!({"a": 1, "b": 2});
        move_at_path(&mut root, &segs("a"), &segs("a")).unwrap();
        assert_eq!(root, json!({"a": 1, "b": 2}));
    }

    /// #1196: Moving an array element to its own index is a no-op.
    #[test]
    fn move_at_path_same_index_is_noop() {
        let mut root = json!({"arr": [10, 20, 30]});
        move_at_path(&mut root, &segs("arr[1]"), &segs("arr[1]")).unwrap();
        assert_eq!(root["arr"], json!([10, 20, 30]));
    }

    /// #1183: When destination insertion fails, the source value must be
    /// preserved in the tree (not silently dropped).
    #[test]
    fn move_at_path_preserves_source_on_dest_failure() {
        let mut root = json!({"src": 42, "blocker": "string"});
        let from = segs("src");
        let to = segs("blocker.nested");
        // Destination parent "blocker" is a string, not an object.
        let result = move_at_path(&mut root, &from, &to);
        result.expect_err("expected error");
        // Source must still be present after the failed move.
        assert_eq!(
            root.get("src"),
            Some(&json!(42)),
            "source value must be preserved on destination failure: {root}"
        );
    }
}
