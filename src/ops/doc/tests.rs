// ── doc module tests ──────────────────────────────────────────────
use crate::ops::doc::*;
use crate::selector;
use serde_json::json;

mod basic {
    use super::*;

    #[test]
    fn detect_format_json() {
        assert!(matches!(
            detect_format("config.json").unwrap(),
            FileFormat::Json
        ));
    }

    #[test]
    fn detect_format_yaml() {
        assert!(matches!(
            detect_format("config.yaml").unwrap(),
            FileFormat::Yaml
        ));
        assert!(matches!(
            detect_format("config.yml").unwrap(),
            FileFormat::Yaml
        ));
    }

    #[test]
    fn detect_format_toml() {
        assert!(matches!(
            detect_format("Cargo.toml").unwrap(),
            FileFormat::Toml
        ));
    }

    #[test]
    fn yaml_merge_keys_resolved() {
        let yaml = "defaults: &d\n  timeout: 30\n  retries: 3\nstaging:\n  <<: *d\n";
        let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        assert_eq!(val["staging"]["retries"], json!(3));
        assert_eq!(val["staging"]["timeout"], json!(30));
        // The merge key itself must be removed.
        assert!(val["staging"].get("<<").is_none());
    }

    #[test]
    fn parse_and_serialize_json_roundtrip() {
        let input = "{\n  \"a\": 1\n}\n";
        let val = parse_doc(input, &FileFormat::Json).unwrap();
        assert_eq!(val, json!({"a": 1}));
        let out = serialize_value(&val, &FileFormat::Json).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn parse_empty_json_is_empty_object() {
        assert_eq!(parse_doc("", &FileFormat::Json).unwrap(), json!({}));
        assert_eq!(
            parse_doc("   \n\t  ", &FileFormat::Json).unwrap(),
            json!({})
        );
    }

    #[test]
    fn parse_and_serialize_yaml_roundtrip() {
        let input = "a: 1\n";
        let val = parse_doc(input, &crate::ops::doc::FileFormat::Yaml).unwrap();
        assert_eq!(val, json!({"a": 1}));
        let out = serialize_value(&val, &crate::ops::doc::FileFormat::Yaml).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn parse_and_serialize_toml_roundtrip() {
        let input = "a = 1\n";
        let val = parse_doc(input, &FileFormat::Toml).unwrap();
        assert_eq!(val, json!({"a": 1}));
        // TOML pretty serialization may differ slightly; just ensure it parses back
        let out = serialize_value(&val, &FileFormat::Toml).unwrap();
        let reparsed = parse_doc(&out, &FileFormat::Toml).unwrap();
        assert_eq!(reparsed, json!({"a": 1}));
    }

    #[test]
    fn navigate_mut_existing_key() {
        let mut val = json!({"a": {"b": 42}});
        let seg = crate::selector::parse("a.b").unwrap();
        let found = navigate_mut(&mut val, &seg, false, "doc.set").unwrap();
        assert_eq!(*found, json!(42));
    }

    #[test]
    fn navigate_mut_create_missing_key() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b.c").unwrap();
        let found = navigate_mut(&mut val, &seg, true, "doc.set").unwrap();
        // created as empty object, then descended into "c" which was also created
        assert!(found.is_object());
    }

    #[test]
    fn navigate_mut_array_index() {
        let mut val = json!({"items": [10, 20, 30]});
        let seg = crate::selector::parse("items[1]").unwrap();
        let found = navigate_mut(&mut val, &seg, false, "doc.set").unwrap();
        assert_eq!(*found, json!(20));
    }

    #[test]
    fn deep_merge_objects() {
        let mut base = json!({"a": 1, "b": {"c": 2}});
        let other = json!({"b": {"d": 3}, "e": 4});
        deep_merge(&mut base, &other);
        assert_eq!(base, json!({"a": 1, "b": {"c": 2, "d": 3}, "e": 4}));
    }

    #[test]
    fn set_at_path_simple_key() {
        let mut root = json!({"a": 1});
        let sel = crate::selector::parse("b").unwrap();
        set_at_path(&mut root, &sel, json!(2)).unwrap();
        assert_eq!(root, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn set_at_path_nested_creates_intermediates() {
        let mut root = json!({});
        let sel = crate::selector::parse("a.b.c").unwrap();
        set_at_path(&mut root, &sel, json!("deep")).unwrap();
        assert_eq!(root, json!({"a": {"b": {"c": "deep"}}}));
    }

    #[test]
    fn set_at_path_array_index() {
        let mut root = json!({"items": [10, 20, 30]});
        let sel = crate::selector::parse("items[1]").unwrap();
        set_at_path(&mut root, &sel, json!(99)).unwrap();
        assert_eq!(root, json!({"items": [10, 99, 30]}));
    }

    // Happy-path delete_where / delete_at_selector / update_matching wildcard
    // live in navigate.rs co-located tests (single home; avoid dual names).

    #[test]
    fn delete_at_selector_array_index() {
        let mut root = json!({"items": [10, 20, 30]});
        let sel = crate::selector::parse("items[1]").unwrap();
        assert!(delete_at_selector(&mut root, &sel).unwrap());
        assert_eq!(root, json!({"items": [10, 30]}));
    }

    #[test]
    fn move_at_path_renames_key() {
        let mut root = json!({"old_name": "value", "other": 1});
        let from = crate::selector::parse("old_name").unwrap();
        let to = crate::selector::parse("new_name").unwrap();
        move_at_path(&mut root, &from, &to).unwrap();
        assert_eq!(root, json!({"other": 1, "new_name": "value"}));
    }

    #[test]
    fn move_at_path_to_nested_creates_intermediates() {
        let mut root = json!({"src": 42});
        let from = crate::selector::parse("src").unwrap();
        let to = crate::selector::parse("a.b.dst").unwrap();
        move_at_path(&mut root, &from, &to).unwrap();
        assert_eq!(root, json!({"a": {"b": {"dst": 42}}}));
    }

    #[test]
    fn update_matching_by_key() {
        let mut val = json!({"a": {"b": "old"}});
        let seg = crate::selector::parse("a.b").unwrap();
        let count = update_matching(&mut val, &seg, &json!("new")).unwrap();
        assert_eq!(count, 1);
        assert_eq!(val, json!({"a": {"b": "new"}}));
    }

    // update_matching_wildcard: see navigate.rs co-located tests.

    #[test]
    fn update_matching_predicate() {
        let mut val = json!({"items": [
            {"name": "a", "v": 1},
            {"name": "b", "v": 2}
        ]});
        let seg = crate::selector::parse("items[name=b].v").unwrap();
        let count = update_matching(&mut val, &seg, &json!(42)).unwrap();
        assert_eq!(count, 1);
        assert_eq!(val["items"][1]["v"], json!(42));
        // First item unchanged
        assert_eq!(val["items"][0]["v"], json!(1));
    }

    #[test]
    fn toml_deep_nested_table_creation() {
        let toml = "# header\n[app]\nname = \"x\"\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        let mut newv = old.clone();
        set_at_path(
            &mut newv,
            &crate::selector::parse("app.server.tls.port").unwrap(),
            json!(9443),
        )
        .unwrap();
        let result = serialize_value_preserving(toml, &old, &newv, &FileFormat::Toml).unwrap();
        let reparsed: serde_json::Value = toml_edit::de::from_str(&result).expect("valid");
        assert_eq!(
            reparsed,
            json!({"app":{"name":"x","server":{"tls":{"port":9443}}}})
        );
        assert!(result.contains("# header"));
    }

    #[test]
    fn mutation_set() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Set {
                selector: "b".into(),
                value: json!(2),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn mutation_delete_existing() {
        let mut root = json!({"a": 1, "b": 2});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Delete {
                selector: "b".into(),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Removed(1)));
        assert_eq!(root, json!({"a": 1}));
    }

    #[test]
    fn mutation_merge() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Merge {
                selector: None,
                value: json!({"b": 2}),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn mutation_update_array_root_bare_key_is_type_error() {
        let mut root = json!([{"tags": ["a"]}, {"tags": ["b"]}]);
        let original = root.clone();
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Update {
                selector: "tags".into(),
                value: json!(["z"]),
            },
        )
        .unwrap();
        match result {
            MutationResult::TypeError(msg) => {
                assert!(
                    msg.contains("0.tags") || msg.contains("[0].tags"),
                    "expected multi-doc hint, got: {msg}"
                );
            }
            other => panic!("expected TypeError, got {other:?}"),
        }
        assert_eq!(root, original);
    }

    #[test]
    fn mutation_append_array_root_bare_key_is_type_error() {
        let mut root = json!([{"tags": ["a"]}, {"tags": ["b"]}]);
        let err = apply_doc_mutation(
            &mut root,
            DocMutation::Append {
                selector: "tags".into(),
                value: json!("z"),
            },
        )
        .unwrap_err();
        assert!(
            crate::exit::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("0.tags") || msg.contains("[0].tags"),
            "expected multi-doc hint, got: {msg}"
        );
    }

    #[test]
    fn mutation_merge_array_root_with_object_is_type_error() {
        // Multi-doc YAML is modeled as a top-level array. Merging an object
        // overlay must not replace the entire stream (silent data loss).
        let mut root = json!([{"a": 1}, {"b": 2}]);
        let original = root.clone();
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Merge {
                selector: None,
                value: json!({"c": 3}),
            },
        )
        .unwrap();
        match result {
            MutationResult::TypeError(msg) => {
                assert!(
                    msg.contains("top-level array") || msg.contains("multi-document"),
                    "expected multi-doc guidance, got: {msg}"
                );
            }
            other => panic!("expected TypeError, got {other:?}"),
        }
        assert_eq!(root, original, "root must be unchanged on type error");
    }

    #[test]
    fn mutation_merge_into_multi_doc_element_via_selector() {
        let mut root = json!([{"a": 1}, {"b": 2}]);
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Merge {
                selector: Some("0".into()),
                value: json!({"c": 3}),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!([{"a": 1, "c": 3}, {"b": 2}]));
    }

    #[test]
    fn mutation_merge_array_root_with_array_is_type_error() {
        // Array overlay also fully replaces via deep_merge (sibling of #1872).
        // Found by MPI: object overlay was refused; array overlay still applied.
        let mut root = json!([{"a": 1}, {"b": 2}]);
        let original = root.clone();
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Merge {
                selector: None,
                value: json!([{"a": 9}, {"b": 9}]),
            },
        )
        .unwrap();
        match result {
            MutationResult::TypeError(msg) => {
                assert!(
                    msg.contains("top-level array") || msg.contains("multi-document"),
                    "expected multi-doc guidance, got: {msg}"
                );
            }
            other => panic!("expected TypeError, got {other:?}"),
        }
        assert_eq!(root, original, "root must be unchanged on type error");
    }

    #[test]
    fn mutation_append_to_array() {
        let mut root = json!({"items": [1, 2]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Append {
                selector: "items".into(),
                value: json!(3),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root["items"], json!([1, 2, 3]));
    }

    #[test]
    fn mutation_prepend_to_array() {
        let mut root = json!({"items": [2, 3]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Prepend {
                selector: "items".into(),
                value: json!(1),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root["items"], json!([1, 2, 3]));
    }

    #[test]
    fn mutation_update_matching() {
        let mut root = json!({"items": [{"id": 1, "v": "a"}, {"id": 2, "v": "b"}]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Update {
                selector: "items[id=1]".into(),
                value: json!({"id": 1, "v": "updated"}),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root["items"][0]["v"], "updated");
    }

    #[test]
    fn mutation_move_key() {
        let mut root = json!({"src": 42, "dst": {}});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Move {
                from: "src".into(),
                to: "dst.val".into(),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"dst": {"val": 42}}));
    }

    #[test]
    fn mutation_ensure_missing() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Ensure {
                selector: "b".into(),
                value: json!(2),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn mutation_delete_where_matching() {
        let mut root = json!({"items": [{"k": "a"}, {"k": "b"}]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::DeleteWhere {
                selector: "items".into(),
                predicate: "k=a".into(),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Removed(1)));
        assert_eq!(root["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parse_value_bare_string() {
        assert_eq!(parse_value("hello"), json!("hello"));
    }

    #[test]
    fn parse_value_integer() {
        assert_eq!(parse_value("42"), json!(42));
    }

    #[test]
    fn parse_value_bool() {
        assert_eq!(parse_value("true"), json!(true));
    }

    #[test]
    fn parse_value_null() {
        assert_eq!(parse_value("null"), serde_json::Value::Null);
    }

    #[test]
    fn parse_value_json_object() {
        assert_eq!(parse_value(r#"{"a":1}"#), json!({"a": 1}));
    }

    #[test]
    fn parse_value_false_is_bool() {
        let result = parse_value("false");
        assert_eq!(result, json!(false));
        assert!(result.is_boolean());
    }

    #[test]
    fn flatten_value_nested() {
        let val = json!({"a": {"b": 1}, "c": [2, 3]});
        let mut buf = String::new();
        let mut out = Vec::new();
        flatten_value(&val, &mut buf, &mut out);
        let paths: Vec<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"a.b"));
        assert!(paths.contains(&"c[0]"));
        assert!(paths.contains(&"c[1]"));
    }

    #[test]
    fn flatten_value_quotes_dot_keys() {
        // Regression: keys containing '.' were unquoted, making "a.b" (single
        // key) indistinguishable from nested a -> b.
        let val = json!({"a.b": 1, "normal": {"nested": 2}});
        let mut buf = String::new();
        let mut out = Vec::new();
        flatten_value(&val, &mut buf, &mut out);
        let paths: Vec<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
        // Dot-key should be quoted
        assert!(
            paths.contains(&r#""a.b""#),
            "dot-containing key should be quoted: {paths:?}"
        );
        // Normal nested path should NOT be quoted
        assert!(paths.contains(&"normal.nested"));
    }

    #[test]
    fn diff_values_detects_changes() {
        let a = json!({"x": 1, "y": 2});
        let b = json!({"x": 1, "y": 3, "z": 4});
        let mut buf = String::new();
        let mut out = Vec::new();
        diff_values(&a, &b, &mut buf, &mut out);
        assert_eq!(out.len(), 2); // y changed, z added
        assert!(out.iter().any(|e| e.path == "y" && e.kind == "changed"));
        assert!(out.iter().any(|e| e.path == "z" && e.kind == "added"));
    }

    #[test]
    fn yaml_plain_strings_do_not_need_quoting() {
        assert!(!needs_yaml_quoting("hello"));
        assert!(!needs_yaml_quoting("foo-bar"));
        assert!(!needs_yaml_quoting("some_value_123"));
        assert!(!needs_yaml_quoting("v1.2.3"));
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn yaml_merge_key_existing_wins() {
        let yaml = "base: &b\n  x: 1\nchild:\n  <<: *b\n  x: 99\n";
        let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        assert_eq!(val["child"]["x"], json!(99));
    }

    #[test]
    fn yaml_merge_key_multiple() {
        let yaml = "a: &a\n  x: 1\nb: &b\n  y: 2\nc:\n  <<:\n    - *a\n    - *b\n";
        let val = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        assert_eq!(val["c"]["x"], json!(1));
        assert_eq!(val["c"]["y"], json!(2));
    }

    #[test]
    fn deep_merge_overwrites_non_object() {
        let mut base = json!({"a": "string"});
        let other = json!({"a": {"nested": true}});
        deep_merge(&mut base, &other);
        assert_eq!(base, json!({"a": {"nested": true}}));
    }

    #[test]
    fn delete_where_no_match_returns_zero() {
        let mut root = json!({"items": [{"name": "a"}]});
        let sel = crate::selector::parse("items").unwrap();
        let removed = delete_where(&mut root, &sel, "name=zzz").unwrap();
        assert_eq!(removed, 0);
    }

    /// Agents often emit `value=X` for scalar arrays (LLM prior); treat as
    /// element match when items are not objects with a `value` field.
    #[test]
    fn delete_where_value_alias_matches_scalar_elements() {
        let mut root = json!({"tags": ["a", "b", "a"]});
        let sel = crate::selector::parse("tags").unwrap();
        let removed = delete_where(&mut root, &sel, "value=a").unwrap();
        assert_eq!(removed, 2);
        assert_eq!(root["tags"], json!(["b"]));
    }

    /// Object arrays with a real `value` field still match that field.
    #[test]
    fn delete_where_value_key_matches_object_field() {
        let mut root = json!({"items": [{"value": "a"}, {"value": "b"}]});
        let sel = crate::selector::parse("items").unwrap();
        let removed = delete_where(&mut root, &sel, "value=a").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(root["items"], json!([{"value": "b"}]));
    }

    #[test]
    fn delete_where_trims_whitespace() {
        let mut root = json!({"items": [{"name": "a"}, {"name": "b"}]});
        let sel = crate::selector::parse("items").unwrap();
        // Spaces around key and value should be trimmed.
        let removed = delete_where(&mut root, &sel, " name = a ").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(root["items"].as_array().unwrap().len(), 1);
        assert_eq!(root["items"][0]["name"], "b");
    }

    // delete_at_selector_missing_returns_false: see navigate.rs co-located tests.

    #[test]
    fn move_at_path_to_array_index() {
        let mut root = json!({"src": "x", "arr": [1, 2, 3]});
        let from = crate::selector::parse("src").unwrap();
        let to = crate::selector::parse("arr[1]").unwrap();
        move_at_path(&mut root, &from, &to).unwrap();
        let arr = root["arr"].as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[1], json!("x"));
    }

    #[test]
    fn update_matching_chained_predicate_updates_row() {
        let mut val = json!({
            "data": [
                {"type": "server", "port": 9000},
                {"type": "web", "port": 80}
            ]
        });
        let seg = crate::selector::parse("data[type=server][port>8000].port").unwrap();
        let count = update_matching(&mut val, &seg, &json!(443)).unwrap();
        assert_eq!(count, 1);
        assert_eq!(val["data"][0]["port"], json!(443));
        assert_eq!(val["data"][1]["port"], json!(80));
    }

    #[test]
    fn update_matching_not_on_object_replaces_record() {
        let mut val = json!({"item": {"name": "a"}});
        let seg = crate::selector::parse("item[!deprecated]").unwrap();
        let count = update_matching(&mut val, &seg, &json!({"name": "b"})).unwrap();
        assert_eq!(count, 1);
        assert_eq!(val, json!({"item": {"name": "b"}}));
    }

    #[test]
    fn update_matching_non_numeric_field_is_invalid_input() {
        let mut val = json!({"servers": [{"port": "abc"}]});
        let seg = crate::selector::parse("servers[port>8000]").unwrap();
        let err = update_matching(&mut val, &seg, &json!(1)).unwrap_err();
        assert!(
            crate::api::is_invalid_input(&err),
            "expected invalid_input, got {err}"
        );
    }

    #[test]
    fn update_matching_missing_key_returns_zero() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b.c").unwrap();
        let count = update_matching(&mut val, &seg, &json!("x")).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn yaml_sequence_root_mutation_not_lost() {
        let yaml = "- item1\n- item2\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new.as_array_mut().unwrap().push(json!("item3"));

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("item3"), "appended item missing: {result}");
        assert!(result.contains("item1"), "item1 missing: {result}");
        assert!(result.contains("item2"), "item2 missing: {result}");
    }

    #[test]
    fn yaml_mapping_to_scalar_root_type_change_not_lost() {
        let yaml = "name: app\nversion: 1\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let new = json!("overridden");

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(
            result.contains("overridden"),
            "root type change lost: {result}"
        );
    }

    #[test]
    fn yaml_mapping_to_array_root_type_change_not_lost() {
        let yaml = "name: app\nversion: 1\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let new = json!(["a", "b"]);

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("- a"), "array item '- a' missing: {result}");
        assert!(result.contains("- b"), "array item '- b' missing: {result}");
    }

    #[test]
    fn yaml_single_document_marker_accepted() {
        // A single document with an explicit `---` marker is valid and parses fine.
        let yaml = "---\nname: only\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        assert_eq!(val, json!({"name": "only"}));

        let mut new = val.clone();
        new["name"] = json!("updated");
        let result = serialize_value_preserving(yaml, &val, &new, &FileFormat::Yaml).unwrap();
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(reparsed, json!({"name": "updated"}));
    }

    #[test]
    fn mutation_delete_missing() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Delete {
                selector: "z".into(),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::NoMatch));
    }

    #[test]
    fn mutation_update_no_match() {
        let mut root = json!({"items": [{"id": 1}]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Update {
                selector: "items[id=99]".into(),
                value: json!({"id": 99}),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::NoMatch));
    }

    #[test]
    fn mutation_ensure_existing() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Ensure {
                selector: "a".into(),
                value: json!(99),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::AlreadyExists));
        assert_eq!(root["a"], 1); // unchanged
    }

    #[test]
    fn mutation_delete_where_no_match() {
        let mut root = json!({"items": [{"k": "a"}]});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::DeleteWhere {
                selector: "items".into(),
                predicate: "k=z".into(),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::NoMatch));
    }

    #[test]
    fn yaml_empty_string_needs_quoting() {
        assert!(needs_yaml_quoting(""));
    }

    // -- parse_value edge cases (#978) -----------------------------------

    #[test]
    fn parse_value_float() {
        assert_eq!(parse_value("1.5"), json!(1.5));
    }

    #[test]
    fn parse_value_negative_integer() {
        assert_eq!(parse_value("-42"), json!(-42));
    }

    #[test]
    fn parse_value_json_array() {
        assert_eq!(parse_value("[1,2,3]"), json!([1, 2, 3]));
    }

    #[test]
    fn parse_value_json_object_with_spaces() {
        assert_eq!(parse_value(r#"{"a": 1}"#), json!({"a": 1}));
    }

    #[test]
    fn parse_value_quoted_string_with_escapes() {
        // JSON-escaped inner quotes: the input is a valid JSON string literal.
        let result = parse_value(r#""hello\"world""#);
        assert_eq!(result, json!("hello\"world"));
        assert!(result.is_string());
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn detect_format_unsupported() {
        detect_format("readme.txt").expect_err("expected error");
    }

    #[test]
    fn detect_format_no_extension() {
        detect_format("Makefile").expect_err("expected error");
    }

    #[test]
    fn navigate_mut_missing_key_no_create() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b").unwrap();
        navigate_mut(&mut val, &seg, false, "doc.set").expect_err("expected error");
    }

    #[test]
    fn navigate_mut_index_out_of_bounds() {
        let mut val = json!({"items": [10]});
        let seg = crate::selector::parse("items[5]").unwrap();
        navigate_mut(&mut val, &seg, false, "doc.set").expect_err("expected error");
    }

    #[test]
    fn set_at_path_out_of_bounds_index_fails() {
        let mut root = json!({"items": [1]});
        let sel = crate::selector::parse("items[5]").unwrap();
        set_at_path(&mut root, &sel, json!(99)).expect_err("expected error");
    }

    #[test]
    fn delete_where_non_array_fails() {
        let mut root = json!({"items": "not-an-array"});
        let sel = crate::selector::parse("items").unwrap();
        delete_where(&mut root, &sel, "k=v").expect_err("expected error");
    }

    #[test]
    fn delete_where_double_equals_rejected() {
        let mut root =
            json!({"items": [{"name": "a", "keep": false}, {"name": "b", "keep": true}]});
        let sel = crate::selector::parse("items").unwrap();
        let err = delete_where(&mut root, &sel, "keep == false").unwrap_err();
        assert!(
            err.to_string().contains("'=='"),
            "expected == rejection, got: {err}"
        );
        // Array must be unchanged (no silent removal).
        assert_eq!(root["items"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn delete_where_empty_key_rejected() {
        let mut root = json!({"items": [{"name": "a"}]});
        let sel = crate::selector::parse("items").unwrap();
        let err = delete_where(&mut root, &sel, "=value").unwrap_err();
        assert!(
            err.to_string().contains("empty"),
            "expected empty-key error, got: {err}"
        );
    }

    #[test]
    fn move_at_path_missing_source_fails() {
        let mut root = json!({"a": 1});
        let from = crate::selector::parse("nonexistent").unwrap();
        let to = crate::selector::parse("b").unwrap();
        move_at_path(&mut root, &from, &to).expect_err("expected error");
    }

    #[test]
    fn move_at_path_array_root_bare_key_is_type_error() {
        // Multi-doc YAML is a top-level array. Bare-key move must type_error
        // with index hints (not invalid_input "parent is array").
        let mut root = json!([{"port": 80}, {"port": 443}]);
        let original = root.clone();
        let from = crate::selector::parse("port").unwrap();
        let to = crate::selector::parse("http_port").unwrap();
        let err = move_at_path(&mut root, &from, &to).unwrap_err();
        assert!(
            crate::exit::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("0.port") || msg.contains("[0].port"),
            "expected multi-doc index hint, got: {msg}"
        );
        assert_eq!(root, original, "root must be unchanged on type error");
    }

    #[test]
    fn move_at_path_array_root_bare_target_is_type_error() {
        // Indexed source + bare target at array root also needs type_error.
        let mut root = json!([{"port": 80}, {"port": 443}]);
        let original = root.clone();
        let from = crate::selector::parse("0.port").unwrap();
        let to = crate::selector::parse("http_port").unwrap();
        let err = move_at_path(&mut root, &from, &to).unwrap_err();
        assert!(
            crate::exit::is_type_error(&err),
            "expected type_error, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("0.http_port") || msg.contains("[0].http_port"),
            "expected multi-doc index hint on target, got: {msg}"
        );
        assert_eq!(root, original, "root must be unchanged on type error");
    }

    #[test]
    fn move_at_path_empty_from_selector_fails() {
        let mut root = json!({"a": 1});
        let from: Vec<crate::selector::Segment> = vec![];
        let to = crate::selector::parse("b").unwrap();
        move_at_path(&mut root, &from, &to).expect_err("expected error");
    }

    #[test]
    fn yaml_multi_document_parsed_as_array() {
        // Multi-document YAML (--- separated) is parsed as an array of documents.
        let yaml = "---\nname: first\n---\nname: second\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let arr = val.as_array().expect("multi-doc should be an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], json!("first"));
        assert_eq!(arr[1]["name"], json!("second"));
    }

    #[test]
    fn yaml_single_doc_with_leading_separator_is_not_multi() {
        // A single document with a leading --- is standard YAML, not multi-doc.
        let yaml = "---\nname: only\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        // Should be parsed as a single object, not wrapped in an array.
        assert!(
            val.is_object(),
            "single doc should be an object, got: {val}"
        );
        assert_eq!(val["name"], json!("only"));
    }

    #[test]
    fn yaml_multi_document_get_second_doc() {
        // Read operations can address individual documents by index.
        let yaml = "---\napiVersion: v1\nkind: ConfigMap\n---\napiVersion: v1\nkind: Service\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let arr = val.as_array().expect("multi-doc should be an array");
        assert_eq!(arr[1]["kind"], json!("Service"));
    }

    #[test]
    fn is_multi_document_detects_two_docs() {
        assert!(is_multi_document_yaml("---\nfirst: 1\n---\nsecond: 2\n"));
    }

    #[test]
    fn is_multi_document_single_leading_separator() {
        // A single leading --- is standard YAML preamble, not multi-doc.
        assert!(!is_multi_document_yaml("---\nname: only\n"));
    }

    #[test]
    fn is_multi_document_no_separator() {
        assert!(!is_multi_document_yaml("name: value\ncount: 1\n"));
    }

    #[test]
    fn is_multi_document_dashes_in_value_not_separator() {
        // "---" embedded in a value (not on its own line) is not a separator.
        assert!(!is_multi_document_yaml("name: ---foo\ncount: 1\n"));
    }

    #[test]
    fn is_multi_document_without_leading_separator() {
        // Two documents without a leading ---.
        assert!(is_multi_document_yaml("first: 1\n---\nsecond: 2\n"));
    }

    #[test]
    fn is_multi_document_three_docs() {
        assert!(is_multi_document_yaml("---\na: 1\n---\nb: 2\n---\nc: 3\n"));
    }

    #[test]
    fn is_multi_document_trailing_whitespace_after_separator() {
        // YAML spec allows trailing whitespace after ---.
        assert!(is_multi_document_yaml("---\nfirst: 1\n---  \nsecond: 2\n"));
    }

    #[test]
    fn is_multi_document_comment_after_separator() {
        // YAML spec allows comments after ---.
        assert!(is_multi_document_yaml(
            "--- # doc1\nfirst: 1\n--- # doc2\nsecond: 2\n"
        ));
    }

    #[test]
    fn is_multi_document_consecutive_separators() {
        // Two consecutive --- with no content between them.
        assert!(is_multi_document_yaml("---\n---\nsecond: 2\n"));
    }

    #[test]
    fn yaml_multi_document_with_merge_keys() {
        // Multi-document YAML where at least one document uses <<: merge keys.
        // Merge key resolution should work correctly per document.
        let yaml = "---\ndefaults: &defaults\n  timeout: 30\n  retries: 3\nservice:\n  name: api\n  <<: *defaults\n---\nkind: Service\nport: 8080\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let arr = val.as_array().expect("multi-doc should be an array");
        assert_eq!(arr.len(), 2);
        // First doc: merge key should be resolved.
        assert_eq!(arr[0]["service"]["timeout"], json!(30));
        assert_eq!(arr[0]["service"]["retries"], json!(3));
        assert_eq!(arr[0]["service"]["name"], json!("api"));
        // Second doc: plain mapping, no merge keys.
        assert_eq!(arr[1]["kind"], json!("Service"));
        assert_eq!(arr[1]["port"], json!(8080));
    }

    #[test]
    fn yaml_multi_document_set_preserves_separators() {
        // Writing multi-doc YAML must keep --- separators (not collapse to a
        // single sequence document). kubectl apply -f requires multi-doc form.
        let yaml = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: demo\ndata:\n  key: value\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: demo-svc\nspec:\n  ports:\n    - port: 80\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new[0]["data"]["key"] = json!("newval");
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();

        assert!(
            is_multi_document_yaml(&result),
            "write must preserve multi-doc stream, got:\n{result}"
        );
        assert!(
            !result.trim_start().starts_with("- "),
            "must not serialize multi-doc as a YAML sequence, got:\n{result}"
        );
        assert!(
            result.contains("key: newval") || result.contains("key: \"newval\""),
            "updated value missing:\n{result}"
        );
        assert!(
            result.contains("kind: Service"),
            "second document must be preserved:\n{result}"
        );

        // Round-trip: still addressable by document index.
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed[0]["data"]["key"], json!("newval"));
        assert_eq!(reparsed[1]["kind"], json!("Service"));
        assert_eq!(reparsed[1]["spec"]["ports"][0]["port"], json!(80));
    }

    /// Unrelated field edit must keep anchors, aliases, and merge keys instead of
    /// expanding every map into a full copy (agent configs use defaults via <<).
    #[test]
    fn yaml_unrelated_edit_preserves_anchors_aliases_and_merges() {
        let yaml = "\
defaults: &defaults
  timeout: 30
  retries: 3
  log_level: info
staging:
  <<: *defaults
  host: staging.example.com
production:
  <<: *defaults
  host: prod.example.com
  log_level: warning
app_name: my-service
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["app_name"] = json!("other-service");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();

        assert!(
            result.contains("&defaults"),
            "anchor definition must survive unrelated edit:\n{result}"
        );
        let merge_count = result.matches("<<:").count();
        assert_eq!(
            merge_count, 2,
            "expected two merge keys preserved, got {merge_count}:\n{result}"
        );
        assert!(
            !result.contains("timeout: 30\n  retries: 3\n  log_level: info\nstaging:\n  timeout:"),
            "must not expand defaults into staging as a full copy:\n{result}"
        );
        assert!(
            result.contains("app_name: other-service")
                || result.contains("app_name: \"other-service\""),
            "edited field missing:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["app_name"], json!("other-service"));
        assert_eq!(reparsed["staging"]["timeout"], json!(30));
        assert_eq!(reparsed["staging"]["host"], json!("staging.example.com"));
        assert_eq!(reparsed["production"]["log_level"], json!("warning"));
    }

    /// Local override of a merge-inherited field adds an explicit key and
    /// keeps the `<<` merge (YAML override semantics).
    #[test]
    fn yaml_merge_override_preserves_merge_key() {
        let yaml = "\
defaults: &d
  timeout: 30
  retries: 3
staging:
  <<: *d
  name: api
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["staging"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&d") && result.contains("<<: *d"),
            "anchor and merge must remain when overriding inherited field:\n{result}"
        );
        assert!(
            result.contains("timeout: 60"),
            "local override missing:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["staging"]["timeout"], json!(60));
        assert_eq!(reparsed["staging"]["retries"], json!(3));
        assert_eq!(reparsed["defaults"]["timeout"], json!(30));
    }

    /// Pure alias (`key: *anchor`) must stay an alias when a sibling field changes.
    #[test]
    fn yaml_pure_alias_preserved_on_sibling_edit() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
service_b: *shared
label: keep
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["label"] = json!("changed");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(result.contains("&shared"), "anchor must remain:\n{result}");
        assert_eq!(
            result.matches("*shared").count(),
            2,
            "both aliases must remain:\n{result}"
        );
        assert!(
            !result.contains("service_a:\n  timeout:"),
            "must not expand pure aliases into full maps:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["label"], json!("changed"));
        assert_eq!(reparsed["service_a"]["timeout"], json!(30));
        assert_eq!(reparsed["service_b"]["retries"], json!(3));
    }

    /// Interior override of a pure mapping alias becomes `<<: *name` plus the
    /// local key. Sibling aliases stay aliases.
    #[test]
    fn yaml_pure_alias_interior_override_becomes_merge() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
service_b: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared"),
            "anchor definition must remain:\n{result}"
        );
        assert!(
            result.contains("<<: *shared"),
            "override must keep identity via merge:\n{result}"
        );
        assert!(
            result.contains("timeout: 60"),
            "local override missing:\n{result}"
        );
        assert!(
            result.contains("service_b: *shared"),
            "sibling alias must stay a pure alias:\n{result}"
        );
        assert!(
            !result.contains("service_a:\n  timeout: 60\n  retries: 3"),
            "must not expand the alias into a full concrete map:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["timeout"], json!(60));
        assert_eq!(reparsed["service_a"]["retries"], json!(3));
        assert_eq!(reparsed["service_b"]["timeout"], json!(30));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Two pure aliases edited in one write both become merges.
    #[test]
    fn yaml_two_pure_aliases_become_merges() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
service_b: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]["timeout"] = json!(60);
        new["service_b"]["retries"] = json!(9);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  <<: *shared
  timeout: 60
service_b:
  <<: *shared
  retries: 9
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["timeout"], json!(60));
        assert_eq!(reparsed["service_b"]["retries"], json!(9));
        assert_eq!(reparsed["service_a"]["retries"], json!(3));
        assert_eq!(reparsed["service_b"]["timeout"], json!(30));
    }

    /// Adding a key under a pure mapping alias is a local addition beside `<<`.
    #[test]
    fn yaml_pure_alias_add_key_becomes_merge() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]["region"] = json!("us-east");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared") && result.contains("<<: *shared"),
            "add-key must convert alias to merge:\n{result}"
        );
        assert!(
            result.contains("region: us-east") || result.contains("region: \"us-east\""),
            "new key missing:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["region"], json!("us-east"));
        assert_eq!(reparsed["service_a"]["timeout"], json!(30));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Deleting an inherited key cannot be expressed with `<<`. Expand that site.
    #[test]
    fn yaml_pure_alias_delete_inherited_still_expands() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
service_b: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]
            .as_object_mut()
            .unwrap()
            .shift_remove("retries");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared"),
            "anchor definition must remain:\n{result}"
        );
        assert!(
            result.contains("service_b: *shared"),
            "untouched sibling must stay an alias:\n{result}"
        );
        assert!(
            !result.contains("service_a: *shared") && !result.contains("service_a:\n  <<:"),
            "deleted inherited key cannot stay a merge:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert!(
            reparsed["service_a"].get("retries").is_none(),
            "retries must be gone from service_a:\n{result}"
        );
        assert_eq!(reparsed["service_a"]["timeout"], json!(30));
        assert_eq!(reparsed["service_b"]["retries"], json!(3));
    }

    /// Replacing a pure alias with a map that is not a key-superset expands.
    /// Keeping `<<` would leak inherited keys into the semantic value.
    #[test]
    fn yaml_pure_alias_non_superset_replace_expands() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"] = json!({"name": "api"});

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared"),
            "anchor definition must remain:\n{result}"
        );
        assert!(
            !result.contains("<<:"),
            "non-superset replace must not keep a merge:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"], json!({"name": "api"}));
        assert!(reparsed["service_a"].get("timeout").is_none());
    }

    /// Sequence item that is a mapping alias converts to merge on override.
    #[test]
    fn yaml_sequence_alias_item_override_becomes_merge() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
items:
  - *shared
  - *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["items"][0]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared") && result.contains("<<: *shared"),
            "sequence alias override must become merge:\n{result}"
        );
        assert!(
            result.contains("- <<: *shared"),
            "first item must become a merge:\n{result}"
        );
        assert!(
            result.contains("  - *shared"),
            "untouched second item must stay a pure alias:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["items"][0]["timeout"], json!(60));
        assert_eq!(reparsed["items"][0]["retries"], json!(3));
        assert_eq!(reparsed["items"][1]["timeout"], json!(30));
    }

    /// Alias as a field inside a sequence item (`items[1].cfg: *shared`).
    /// The walk must recurse mappings in the list and rewrite only that site.
    #[test]
    fn yaml_alias_field_inside_sequence_item_becomes_merge() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
items:
  - name: a
    cfg: *shared
  - name: b
    cfg: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["items"][1]["cfg"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("name: a") && result.contains("cfg: *shared"),
            "first item cfg must stay a pure alias:\n{result}"
        );
        assert!(
            result.contains("<<: *shared") && result.contains("timeout: 60"),
            "second item cfg must become a merge:\n{result}"
        );
        assert_eq!(
            result.matches("cfg: *shared").count(),
            1,
            "only the unedited cfg should remain a pure alias:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["items"][0]["cfg"]["timeout"], json!(30));
        assert_eq!(reparsed["items"][1]["cfg"]["timeout"], json!(60));
        assert_eq!(reparsed["items"][1]["cfg"]["retries"], json!(3));
        assert_eq!(reparsed["items"][1]["name"], json!("b"));
    }

    /// Same mapping key (`cfg: *shared`) under two sibling objects.
    /// File-wide unique-line matching would rewrite the first site.
    #[test]
    fn yaml_sibling_objects_same_alias_key_rewrites_the_edited_site() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  cfg: *shared
service_b:
  cfg: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_b"]["cfg"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  cfg: *shared
service_b:
  cfg:
    <<: *shared
    timeout: 60
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["cfg"]["timeout"], json!(30));
        assert_eq!(reparsed["service_b"]["cfg"]["timeout"], json!(60));
        assert_eq!(reparsed["service_b"]["cfg"]["retries"], json!(3));
    }

    /// Three `cfg: *shared` sites; edit only the middle occurrence.
    #[test]
    fn yaml_three_sibling_alias_keys_rewrites_middle_site() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  cfg: *shared
service_b:
  cfg: *shared
service_c:
  cfg: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_b"]["cfg"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
  retries: 3
service_a:
  cfg: *shared
service_b:
  cfg:
    <<: *shared
    timeout: 60
service_c:
  cfg: *shared
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["cfg"]["timeout"], json!(30));
        assert_eq!(reparsed["service_b"]["cfg"]["timeout"], json!(60));
        assert_eq!(reparsed["service_c"]["cfg"]["timeout"], json!(30));
    }

    /// Two sibling lists share `*shared`. Edit the second list only.
    #[test]
    fn yaml_sibling_sequence_alias_rewrites_the_edited_list() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
first:
  - *shared
second:
  - *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["second"][0]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
  retries: 3
first:
  - *shared
second:
  - <<: *shared
    timeout: 60
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["first"][0]["timeout"], json!(30));
        assert_eq!(reparsed["second"][0]["timeout"], json!(60));
        assert_eq!(reparsed["second"][0]["retries"], json!(3));
    }

    /// CRLF alias lines must still convert to merge and keep CRLF.
    #[test]
    fn yaml_pure_alias_override_preserves_crlf() {
        let yaml = "shared: &shared\r\n  timeout: 30\r\n  retries: 3\r\nservice_a: *shared\r\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("\r\n") && result.contains("<<: *shared"),
            "CRLF alias override must become merge:\n{result:?}"
        );
        assert!(
            !result.replace("\r\n", "").contains('\n'),
            "must not mix bare LF into CRLF file: {result:?}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["timeout"], json!(60));
        assert_eq!(reparsed["service_a"]["retries"], json!(3));
    }

    /// Replacing a pure alias with `{}` must stay an object, not null.
    #[test]
    fn yaml_pure_alias_replace_with_empty_object() {
        let yaml = "shared: &shared\n  timeout: 30\nservice_a: *shared\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"] = json!({});

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(result, "shared: &shared\n  timeout: 30\nservice_a:\n  {}\n");
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"], json!({}));
        assert!(reparsed["service_a"].get("timeout").is_none());
    }

    /// Nested `parent.child: *shared` is the same interior-alias case as a
    /// top-level key; the walk must find the alias under the parent mapping.
    #[test]
    fn yaml_nested_mapping_alias_becomes_merge() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
parent:
  child: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["parent"]["child"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("&shared") && result.contains("<<: *shared"),
            "nested alias override must become merge:\n{result}"
        );
        assert!(
            result.contains("child:\n    <<: *shared"),
            "nested alias must become a block merge:\n{result}"
        );
        assert!(
            result.contains("timeout: 60"),
            "local override missing:\n{result}"
        );
        assert!(
            !result.contains("child: *shared"),
            "edited nested alias must not stay a pure alias:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["parent"]["child"]["timeout"], json!(60));
        assert_eq!(reparsed["parent"]["child"]["retries"], json!(3));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// A trailing comment on `key: *alias` stays on the key after the splice.
    #[test]
    fn yaml_pure_alias_keeps_trailing_comment() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared  # inherited
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["service_a"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("service_a:  # inherited") && result.contains("<<: *shared"),
            "comment must stay on the key line after splice:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["service_a"]["timeout"], json!(60));
        assert_eq!(reparsed["service_a"]["retries"], json!(3));
    }

    /// Mixed flow `{cfg: *shared}` plus later block `cfg: *shared`. Editing
    /// the block site must splice and leave the flow sibling (and `&shared`).
    #[test]
    fn yaml_mixed_flow_block_alias_block_edit_keeps_flow_sibling() {
        let yaml = "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["block"]["cfg"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg:
    <<: *shared
    timeout: 60
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["block"]["cfg"]["timeout"], json!(60));
        assert_eq!(reparsed["flow"]["cfg"]["timeout"], json!(30));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Mixed flow `[*shared]` plus later block `- *shared`. Editing the
    /// block item must splice and leave the flow sibling (and `&shared`).
    #[test]
    fn yaml_mixed_flow_sequence_block_alias_block_edit_keeps_flow_sibling() {
        let yaml = "\
shared: &shared
  timeout: 30
flow: [*shared]
block:
  - *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["block"][0]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
flow: [*shared]
block:
  - <<: *shared
    timeout: 60
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["block"][0]["timeout"], json!(60));
        assert_eq!(reparsed["flow"][0]["timeout"], json!(30));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Pure prepend must not zip `new[0]` onto `- *shared`. The inserted
    /// object is spliced in; the alias item and its trailing comment stay.
    #[test]
    fn yaml_sequence_alias_pure_prepend_keeps_alias() {
        let yaml = "\
shared: &shared
  timeout: 30
items:
  - *shared  # inherited
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["items"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"name": "x"}));

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
items:
  - name: x
  - *shared  # inherited
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["items"][0], json!({"name": "x"}));
        assert_eq!(reparsed["items"][1]["timeout"], json!(30));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Shrink-first + edit remaining must not rewrite `*gone` as if it
    /// were `*keep`. Length shifts leave the remaining item to splice.
    #[test]
    fn yaml_sequence_alias_shrink_first_does_not_rewrite_gone() {
        let yaml = "\
gone: &gone
  timeout: 10
keep: &keep
  timeout: 30
items:
  - *gone
  - *keep
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["items"].as_array_mut().unwrap().remove(0);
        new["items"][0]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
gone: &gone
  timeout: 10
keep: &keep
  timeout: 30
items:
  - timeout: 60
"
        );
        assert!(
            !result.contains("*gone") || result.contains("&gone"),
            "must not rewrite the removed - *gone site:\n{result}"
        );
        assert!(
            !result.contains("<<: *gone"),
            "must not treat *gone as the remaining edit:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["items"], json!([{"timeout": 60}]));
        assert_eq!(reparsed["gone"]["timeout"], json!(10));
        assert_eq!(reparsed["keep"]["timeout"], json!(30));
    }

    /// Prepend on sequence A must not steal file-wide nth from sequence B.
    #[test]
    fn yaml_sequence_alias_prepend_does_not_steal_sibling_nth() {
        let yaml = "\
shared: &shared
  timeout: 30
first:
  - *shared
second:
  - *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["first"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"name": "x"}));
        new["second"][0]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
first:
  - name: x
  - *shared
second:
  - <<: *shared
    timeout: 60
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["first"][0], json!({"name": "x"}));
        assert_eq!(reparsed["first"][1]["timeout"], json!(30));
        assert_eq!(reparsed["second"][0]["timeout"], json!(60));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Insert-middle + edit later `- *shared` must keep alias identity
    /// off the inserted object (length shift is unaligned).
    #[test]
    fn yaml_sequence_alias_insert_middle_does_not_rewrite_later_alias() {
        let yaml = "\
shared: &shared
  timeout: 30
items:
  - name: a
  - *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["items"]
            .as_array_mut()
            .unwrap()
            .insert(1, json!({"name": "mid"}));
        new["items"][2]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
shared: &shared
  timeout: 30
items:
  - name: a
  - name: mid
  - timeout: 60
"
        );
        assert!(
            result.contains("&shared"),
            "anchor definition must remain:\n{result}"
        );
        assert!(
            !result.contains("- *shared") && !result.contains("<<: *shared"),
            "unaligned insert must dump the later item, not zip - *shared:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(
            reparsed["items"],
            json!([{"name": "a"}, {"name": "mid"}, {"timeout": 60}])
        );
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// Multi-doc streams serialize per document. A pure alias in doc 1 must
    /// become a merge without dumping doc 0.
    #[test]
    fn yaml_multi_doc_second_doc_pure_alias_becomes_merge() {
        let yaml = "\
---
kind: Config
name: keep
---
shared: &shared
  timeout: 30
  retries: 3
service_a: *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new[1]["service_a"]["timeout"] = json!(60);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            is_multi_document_yaml(&result),
            "must stay multi-document:\n{result}"
        );
        assert!(
            result.contains("name: keep") || result.contains("name: \"keep\""),
            "first document must stay intact:\n{result}"
        );
        assert!(
            result.contains("&shared") && result.contains("<<: *shared"),
            "second-doc alias override must become merge:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed[0]["name"], json!("keep"));
        assert_eq!(reparsed[1]["service_a"]["timeout"], json!(60));
        assert_eq!(reparsed[1]["service_a"]["retries"], json!(3));
    }

    /// Keys that are not YAML plain identifiers skip the alias-line splice
    /// (they are interpolated into a regex). Semantic edit still applies.
    #[test]
    fn yaml_non_plain_key_alias_still_edits() {
        let yaml = "\
shared: &shared
  timeout: 30
  retries: 3
\"foo:bar\": *shared
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["foo:bar"]["timeout"] = json!(60);

        let file: yaml_edit::YamlFile = yaml.parse().unwrap();
        let splice =
            super::super::yaml_cst::rewrite_yaml_alias_object_edits(yaml, &file, &old, &new)
                .unwrap();
        assert!(
            splice.is_none(),
            "non-plain key must skip the alias-line splice, got {splice:?}"
        );

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["foo:bar"]["timeout"], json!(60));
        assert_eq!(reparsed["foo:bar"]["retries"], json!(3));
        assert_eq!(reparsed["shared"]["timeout"], json!(30));
    }

    /// New key under a merge map is a local addition; merge/anchor stay.
    #[test]
    fn yaml_add_key_under_merge_map_preserves_merge() {
        let yaml = "\
defaults: &d
  timeout: 30
staging:
  <<: *d
  name: api
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["staging"]["region"] = json!("us-east");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("<<: *d") && result.contains("&d"),
            "merge/anchor must remain after local key add:\n{result}"
        );
        assert!(
            result.contains("region: us-east") || result.contains("region: \"us-east\""),
            "new key missing:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["staging"]["region"], json!("us-east"));
        assert_eq!(reparsed["staging"]["timeout"], json!(30));
    }

    /// Growing an array inherited only via `<<: *anchor` must add a local
    /// `env:` override beside the merge key. Dumping loses `&defaults`.
    #[test]
    fn yaml_inherited_array_growth_keeps_merge_key() {
        let yaml = "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["deployment"]["env"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "B", "value": "2"}));

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // Inserted override uses serde_yaml_ng quote style ('1' / '2').
        // Anchor/merge and the original defaults.env quotes stay.
        assert_eq!(
            result,
            "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
  env:
    - name: A
      value: '1'
    - name: B
      value: '2'
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(
            reparsed["deployment"]["env"],
            json!([{"name": "A", "value": "1"}, {"name": "B", "value": "2"}])
        );
        assert_eq!(
            reparsed["defaults"]["env"],
            json!([{"name": "A", "value": "1"}])
        );
        assert!(
            !presentation_style_changed(yaml, &result, &FileFormat::Yaml),
            "same indent level and &/* /<< counts must not flag style:\n{result}"
        );
    }

    /// Shrinking the same inherited array still keeps `<<` and adds a local
    /// override (does not trip array-growth; indent fixer must not flatten).
    #[test]
    fn yaml_inherited_array_shrink_keeps_merge_key() {
        let yaml = "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
    - name: B
      value: \"2\"
deployment:
  <<: *defaults
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["deployment"]["env"].as_array_mut().unwrap().pop();

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
    - name: B
      value: \"2\"
deployment:
  <<: *defaults
  env:
    - name: A
      value: '1'
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(
            reparsed["deployment"]["env"],
            json!([{"name": "A", "value": "1"}])
        );
        assert_eq!(
            reparsed["defaults"]["env"],
            json!([{"name": "A", "value": "1"}, {"name": "B", "value": "2"}])
        );
    }

    /// Deleting a key inherited only via `<<` must expand that site (drop
    /// `<<`, write the remaining local map). Dumping loses `&defaults`.
    #[test]
    fn yaml_inherited_delete_expands_site_keeps_anchor() {
        let yaml = "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["deployment"]
            .as_object_mut()
            .unwrap()
            .shift_remove("env");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment: {}
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert!(
            reparsed["deployment"].get("env").is_none(),
            "inherited env must be gone from deployment:\n{result}"
        );
        assert_eq!(
            reparsed["defaults"]["env"],
            json!([{"name": "A", "value": "1"}])
        );
    }

    /// Replacing a merge map with a non-superset object must expand that
    /// site only. Keeping `<<` would leak inherited keys.
    #[test]
    fn yaml_inherited_non_superset_replace_expands_site() {
        let yaml = "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["deployment"] = json!({"name": "api"});

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert_eq!(
            result,
            "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  name: api
"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed["deployment"], json!({"name": "api"}));
        assert!(reparsed["deployment"].get("env").is_none());
        assert_eq!(
            reparsed["defaults"]["env"],
            json!([{"name": "A", "value": "1"}])
        );
    }

    /// Multi-document stream: unrelated edit in one doc keeps that doc's merges.
    #[test]
    fn yaml_multi_doc_unrelated_edit_preserves_merges() {
        let yaml = "\
---
defaults: &defaults
  timeout: 30
service:
  <<: *defaults
  name: api
---
kind: Service
port: 8080
";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new[0]["service"]["name"] = json!("api-v2");
        new[1]["port"] = json!(9090);

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            is_multi_document_yaml(&result),
            "must stay multi-document:\n{result}"
        );
        assert!(
            result.contains("&defaults") && result.contains("<<: *defaults"),
            "first doc anchors/merges must survive an edit in that document:\n{result}"
        );
        assert!(
            result.contains("name: api-v2") || result.contains("name: \"api-v2\""),
            "first doc edit missing:\n{result}"
        );
        assert!(
            result.contains("port: 9090"),
            "second doc edit missing:\n{result}"
        );
    }

    #[test]
    fn yaml_multi_document_delete_first_preserves_surviving_comments() {
        // Whole-doc delete must not pair body[0] onto former doc 1.
        let yaml = "# doc A comment\nname: alpha\n---\n# doc B comment\nname: beta\n---\n# doc C comment\nname: gamma\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new.as_array_mut().unwrap().remove(0);
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("# doc B comment") && result.contains("name: beta"),
            "surviving beta must keep its comment, got:\n{result}"
        );
        assert!(
            result.contains("# doc C comment") && result.contains("name: gamma"),
            "surviving gamma must keep its comment, got:\n{result}"
        );
        assert!(
            !result.contains("# doc A comment"),
            "deleted alpha comment must not stick to beta, got:\n{result}"
        );
        assert!(
            !result.contains("name: alpha"),
            "deleted alpha must be gone, got:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed.as_array().map(|a| a.len()), Some(2));
        assert_eq!(reparsed[0]["name"], json!("beta"));
        assert_eq!(reparsed[1]["name"], json!("gamma"));
    }

    #[test]
    fn yaml_multi_document_set_preserves_crlf_separators() {
        // Windows agent editing multi-doc k8s manifests must keep CRLF stream.
        let yaml = "---\r\na: 1\r\n---\r\nb: 2\r\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new[0]["a"] = json!(9);
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("\r\n"),
            "multi-doc write must preserve CRLF separators, got: {result:?}"
        );
        assert!(
            !result.replace("\r\n", "").contains('\n'),
            "must not mix bare LF into CRLF multi-doc stream: {result:?}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed[0]["a"], json!(9));
        assert_eq!(reparsed[1]["b"], json!(2));
    }

    #[test]
    fn yaml_multi_document_set_second_doc_preserves_first() {
        let yaml = "---\nname: alpha\nversion: 1\n---\nname: beta\nversion: 2\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new[1]["version"] = json!(99);
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();

        assert!(is_multi_document_yaml(&result), "got:\n{result}");
        assert!(
            result.starts_with("---"),
            "leading marker preserved:\n{result}"
        );
        let reparsed = parse_doc(&result, &FileFormat::Yaml).unwrap();
        assert_eq!(reparsed[0]["name"], json!("alpha"));
        assert_eq!(reparsed[0]["version"], json!(1));
        assert_eq!(reparsed[1]["name"], json!("beta"));
        assert_eq!(reparsed[1]["version"], json!(99));
    }

    #[test]
    fn yaml_multi_document_unchanged_returns_original() {
        let yaml = "first: 1\n---\nsecond: 2\n";
        let val = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let result = serialize_value_preserving(yaml, &val, &val, &FileFormat::Yaml).unwrap();
        assert_eq!(result, yaml);
    }

    #[test]
    fn split_multi_document_yaml_without_leading_marker() {
        let (leading, bodies) = split_multi_document_yaml("first: 1\n---\nsecond: 2\n");
        assert!(!leading);
        assert_eq!(bodies.len(), 2);
        assert!(bodies[0].contains("first: 1"));
        assert!(bodies[1].contains("second: 2"));
    }

    #[test]
    fn split_multi_document_yaml_with_leading_marker() {
        let (leading, bodies) = split_multi_document_yaml("---\na: 1\n---\nb: 2\n");
        assert!(leading);
        assert_eq!(bodies.len(), 2);
        assert!(bodies[0].contains("a: 1"));
        assert!(bodies[1].contains("b: 2"));
    }

    #[test]
    fn mutation_append_to_non_array() {
        let mut root = json!({"items": "not-array"});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Append {
                selector: "items".into(),
                value: json!(1),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::TypeError(_)));
    }

    #[test]
    fn mutation_move_missing_source() {
        let mut root = json!({"a": 1});
        let err = apply_doc_mutation(
            &mut root,
            DocMutation::Move {
                from: "nonexistent".into(),
                to: "b".into(),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("source key 'nonexistent' not found")
        );
    }

    #[test]
    fn mutation_prepend_to_non_array() {
        let mut root = json!({"items": "not-array"});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Prepend {
                selector: "items".into(),
                value: json!(1),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::TypeError(_)));
    }
}

mod security {
    use super::*;

    fn nest_n(depth: usize, leaf: serde_json::Value) -> serde_json::Value {
        let mut v = leaf;
        for _ in 0..depth {
            v = json!({"n": v});
        }
        v
    }

    #[test]
    fn deep_merge_depth_limit() {
        // Depth 10 still key-merges. Depth 128 clones the remaining
        // subtree, so the leaf is other's keys only.
        let mut base = nest_n(150, json!({"a": 1}));
        if let Some(mid) = nest_get_mut(&mut base, 10) {
            mid.as_object_mut()
                .unwrap()
                .insert("from_base".into(), json!(true));
        }
        let mut other = nest_n(150, json!({"b": 2}));
        if let Some(mid) = nest_get_mut(&mut other, 10) {
            mid.as_object_mut()
                .unwrap()
                .insert("from_other".into(), json!(true));
        }
        deep_merge(&mut base, &other);

        let mid = nest_get(&base, 10).expect("depth 10");
        assert_eq!(mid.get("from_base"), Some(&json!(true)));
        assert_eq!(mid.get("from_other"), Some(&json!(true)));

        let leaf = nest_get(&base, 150).expect("depth 150");
        assert_eq!(leaf, &json!({"b": 2}));
        assert!(leaf.get("a").is_none());
    }

    fn nest_get(v: &serde_json::Value, depth: usize) -> Option<&serde_json::Value> {
        let mut cursor = v;
        for _ in 0..depth {
            cursor = cursor.get("n")?;
        }
        Some(cursor)
    }

    fn nest_get_mut(v: &mut serde_json::Value, depth: usize) -> Option<&mut serde_json::Value> {
        let mut cursor = v;
        for _ in 0..depth {
            cursor = cursor.get_mut("n")?;
        }
        Some(cursor)
    }

    #[test]
    fn resolve_yaml_merge_keys_depth_guard_caps_recursion() {
        // `<<` at depth 10 must resolve. `<<` at depth 151 must stay.
        // A cap of 2 would leave the shallow key; no cap flattens the leaf.
        let mut inner = json!({"<<": {"deep": 1}});
        for _ in 0..140 {
            inner = json!({"nested": inner});
        }
        inner = json!({"nested": inner, "<<": {"shallow": 1}});
        for _ in 0..10 {
            inner = json!({"nested": inner});
        }
        super::super::resolve_yaml_merge_keys(&mut inner);

        let mut cursor = &inner;
        for _ in 0..10 {
            cursor = cursor
                .get("nested")
                .expect("nesting should be at least 10 levels deep");
        }
        assert_eq!(cursor.get("shallow"), Some(&json!(1)));
        assert!(
            cursor.get("<<").is_none(),
            "merge key at depth 10 must resolve:\n{cursor}"
        );

        cursor = &inner;
        for _ in 0..151 {
            cursor = cursor
                .get("nested")
                .expect("nesting should continue past the merge-key depth cap");
        }
        while let Some(next) = cursor.get("nested") {
            cursor = next;
        }
        assert!(
            cursor.get("<<").is_some(),
            "merge key beyond MAX_MERGE_DEPTH must remain unresolved"
        );
        assert!(cursor.get("deep").is_none());
    }
}

mod regression {
    use super::*;

    // ── gap closing regressions (#824/#825 style fidelity) ───────────
    #[test]
    fn yaml_deep_nested_object_creation() {
        // Deep (2+) new intermediates: in-memory always correct.
        // Preserving serialize may use fallback for some >1-level cases
        // (data correct, comments may be hoisted). We assert in-memory +
        // plain serialize roundtrip.
        let mut v = json!({"root": 1});
        set_at_path(&mut v, &crate::selector::parse("a.b.c").unwrap(), json!(42)).unwrap();
        assert_eq!(v, json!({"root":1, "a":{"b":{"c":42}}}));

        let plain = serialize_value(&v, &FileFormat::Yaml).unwrap();
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&plain).unwrap();
        assert_eq!(reparsed, json!({"root":1, "a":{"b":{"c":42}}}));
    }

    #[test]
    fn yaml_nested_keys_create_correct_structure() {
        // Regression for #824: new nested via set/ensure/merge must produce
        // valid indented mappings, not "key:\nleaf: " (null parent + sibling).
        let yaml = "a: 1\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut newv = old.clone();
        set_at_path(
            &mut newv,
            &crate::selector::parse("server.port").unwrap(),
            json!(9090),
        )
        .unwrap();
        let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(reparsed, json!({"a":1, "server":{"port":9090}}));
        // Also for ensure path (no-op if exists, but create case)
        let mut new2 = old.clone();
        // simulate ensure by set since not exist
        set_at_path(
            &mut new2,
            &crate::selector::parse("settings.debug").unwrap(),
            json!(false),
        )
        .unwrap();
        let r2 = serialize_value_preserving(yaml, &old, &new2, &FileFormat::Yaml).unwrap();
        let p2: serde_json::Value = serde_yaml_ng::from_str(&r2).unwrap();
        assert_eq!(p2, json!({"a":1, "settings":{"debug":false}}));
        // merge case
        let mut new3 = old.clone();
        deep_merge(&mut new3, &json!({"server": {"port": 9090}}));
        let r3 = serialize_value_preserving(yaml, &old, &new3, &FileFormat::Yaml).unwrap();
        let p3: serde_json::Value = serde_yaml_ng::from_str(&r3).unwrap();
        assert_eq!(p3, json!({"a":1, "server":{"port":9090}}));
    }

    #[test]
    fn yaml_array_shrink_with_modification_not_lost() {
        // Regression: complex array shrinkage (shrink + modify remaining
        // elements) must not be silently dropped. The CST path cannot
        // handle this case, so it should fall through to serialize_value.
        let yaml = "# Config\nname: app\ntags:\n  - alpha\n  - beta\n  - gamma\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let new = json!({"name": "app", "tags": ["MODIFIED"]});

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        let parsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(
            parsed, new,
            "serialized YAML must match target value: {result}"
        );
    }

    #[test]
    fn yaml_nested_complex_shrinkage_in_same_length_outer() {
        // Regression: same-length outer array with complex inner array
        // shrinkage (not a subsequence). apply_yaml_sequence_diff must
        // propagate the failure flag from the nested mapping diff.
        let yaml = "# Comment\nitems:\n  - name: a\n    tags:\n      - x\n      - y\n  - name: b\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let new = json!({"items": [{"name": "a", "tags": ["MODIFIED"]}, {"name": "b"}]});

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        let parsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(
            parsed, new,
            "nested complex shrinkage must not be silently dropped: {result}"
        );
    }

    #[test]
    fn yaml_nested_array_growth_detected() {
        // Regression: nested array growth (add a port on the first server
        // while the outer array stays the same length) must not be dropped.
        let yaml = "# config comment\nservers:\n  - name: web\n    ports:\n      - 80\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        new["servers"][0]["ports"]
            .as_array_mut()
            .unwrap()
            .push(json!(443));

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(
            result.contains("443"),
            "nested array growth should not be silently dropped: {result}"
        );
        assert!(
            result.contains("# config comment"),
            "comments should be preserved: {result}"
        );
    }

    #[test]
    fn yaml_nested_array_append_produces_yaml_edit_parseable_output() {
        // Regression for #972: doc_append on a deeply nested array (e.g., K8s
        // env vars inside a container) could produce YAML with altered indentation.
        // serde_yaml_ng accepted the result, but yaml_edit (CST parser) rejected it,
        // causing subsequent doc_set calls to fail with "did not find expected key".
        //
        // Uses realistic K8s-style indentation: containers at 8 spaces, entries
        // at 10 spaces (the exact pattern from the bug report).
        let yaml = concat!(
            "apiVersion: apps/v1\n",
            "kind: Deployment\n",
            "metadata:\n",
            "  name: my-app\n",
            "spec:\n",
            "  replicas: 1\n",
            "  template:\n",
            "    spec:\n",
            "      containers:\n",
            "        - name: main\n",
            "          image: my-app:latest\n",
            "          env:\n",
            "            - name: DB_HOST\n",
            "              value: postgres.default:5432\n",
            "            - name: API_URL\n",
            "              valueFrom:\n",
            "                configMapKeyRef:\n",
            "                  name: config\n",
            "                  key: api-url\n",
            "          resources:\n",
            "            limits:\n",
            "              cpu: \"1\"\n",
            "              memory: 512Mi\n",
        );
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        // Append a new env var with a URL value (the original bug trigger).
        new["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_array_mut()
            .unwrap()
            .push(
                json!({"name": "OTEL_ENDPOINT", "value": "http://otel-collector.monitoring:4317"}),
            );

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // Must round-trip through serde_yaml_ng.
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(reparsed, new, "serialized YAML must match target: {result}");
        // Must also be parseable by yaml_edit (the CST library used by doc_set).
        assert!(
            result.parse::<yaml_edit::YamlFile>().is_ok(),
            "result must be parseable by yaml_edit for subsequent doc_set: {result}"
        );
        // After the append, a subsequent set on a different path must succeed.
        // This simulates the #972 scenario: doc_append then doc_set.
        let mut final_val = new.clone();
        final_val["spec"]["template"]["spec"]["containers"][0]["resources"]["limits"]["cpu"] =
            json!("2");
        let result2 = serialize_value_preserving(&result, &new, &final_val, &FileFormat::Yaml)
            .expect("doc_set after doc_append must not fail");
        let reparsed2: serde_json::Value = serde_yaml_ng::from_str(&result2).unwrap();
        assert_eq!(
            reparsed2, final_val,
            "second operation must produce correct output: {result2}"
        );
    }
}

mod yaml_cst_cleanup {
    use super::*;

    #[test]
    fn fix_yaml_block_indentation_keeps_sequence_item_fields() {
        let yaml = "\
defaults: &defaults
  env:
    - name: A
      value: \"1\"
deployment:
  <<: *defaults
  env:
    - name: A
      value: '1'
    - name: B
      value: '2'
";
        let fixed = super::super::fix_yaml_block_indentation(yaml);
        assert_eq!(fixed, yaml);
    }

    #[test]
    fn delete_last_key_no_trailing_whitespace() {
        let yaml = "top:\n  first: aaa\n  second: bbb\n  third: ccc\n";
        let old = json!({"top": {"first": "aaa", "second": "bbb", "third": "ccc"}});
        let new = json!({"top": {"first": "aaa", "second": "bbb"}});
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // No line should have trailing whitespace
        for line in result.lines() {
            assert_eq!(
                line,
                line.trim_end(),
                "trailing whitespace found in: {:?}",
                line
            );
        }
        // Must end with exactly one newline
        assert!(result.ends_with('\n'), "missing final newline");
        assert!(
            !result.ends_with("\n\n"),
            "double final newline: {:?}",
            &result[result.len().saturating_sub(4)..]
        );
    }

    #[test]
    fn yaml_cst_set_preserves_crlf_line_endings() {
        // Real Windows path: doc set on CRLF YAML with comments uses CST preserve.
        let yaml = "server:\r\n  port: 8080\r\n  # keep me\r\n  host: localhost\r\n";
        let old = json!({"server": {"port": 8080, "host": "localhost"}});
        let new = json!({"server": {"port": 9090, "host": "localhost"}});
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(
            result.contains("\r\n"),
            "CST preserve path must keep CRLF, got: {result:?}"
        );
        assert!(result.contains("9090"), "updated port missing: {result:?}");
        assert!(
            result.contains("# keep me"),
            "comment must be preserved: {result:?}"
        );
        // Dominant EOL should remain CRLF (no mixed bare LF after strip).
        let without_crlf = result.replace("\r\n", "");
        assert!(
            !without_crlf.contains('\n'),
            "must not introduce bare LF into CRLF file: {result:?}"
        );
    }

    #[test]
    fn delete_last_key_in_nested_section() {
        let yaml = "server:\n  host: localhost\n  port: 8080\n  workers: 4\n\ndb:\n  url: pg\n  pool: 10\n";
        let old = json!({"server": {"host": "localhost", "port": 8080, "workers": 4}, "db": {"url": "pg", "pool": 10}});
        let new = json!({"server": {"host": "localhost", "port": 8080, "workers": 4}, "db": {"url": "pg"}});
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        for line in result.lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace: {:?}", line);
        }
        assert!(result.ends_with('\n'));
        assert!(result.contains("url: pg"));
        assert!(!result.contains("pool"));
    }

    #[test]
    fn delete_first_key_preserves_quotes_and_order() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  enabled: \"true\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "enabled": "true", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("name");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // Quotes must be preserved on untouched values.
        assert!(
            result.contains("\"1.0.0\""),
            "version quotes lost: {result}"
        );
        assert!(result.contains("\"true\""), "enabled quotes lost: {result}");
        assert!(result.contains("\"8080\""), "port quotes lost: {result}");
        // Key ordering must match original (minus deleted key).
        let version_pos = result.find("version").unwrap();
        let enabled_pos = result.find("enabled").unwrap();
        let port_pos = result.find("port").unwrap();
        assert!(
            version_pos < enabled_pos && enabled_pos < port_pos,
            "key order changed: {result}"
        );
        // Deleted key must be gone.
        assert!(!result.contains("name"), "deleted key still present");
        // Indentation must be consistent (2-space).
        for line in result.lines() {
            if line.starts_with(' ') {
                let indent = line.len() - line.trim_start().len();
                assert_eq!(indent, 2, "wrong indentation on: {line:?}");
            }
        }
    }

    #[test]
    fn delete_first_key_quoted_type_sensitive_values() {
        // Values that would change YAML type if unquoted.
        let yaml =
            "config:\n  remove_me: x\n  enabled: \"true\"\n  count: \"42\"\n  ratio: \"1.0\"\n";
        let old =
            json!({"config": {"remove_me": "x", "enabled": "true", "count": "42", "ratio": "1.0"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("config")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("remove_me");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // Re-parse to verify types are still strings (not bool/int/float).
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(
            reparsed["config"]["enabled"],
            json!("true"),
            "enabled became bool"
        );
        assert_eq!(reparsed["config"]["count"], json!("42"), "count became int");
        assert_eq!(
            reparsed["config"]["ratio"],
            json!("1.0"),
            "ratio became float"
        );
    }

    #[test]
    fn delete_middle_key_preserves_formatting() {
        let yaml = "app:\n  name: \"my-app\"\n  version: \"1.0.0\"\n  port: \"8080\"\n";
        let old = json!({"app": {"name": "my-app", "version": "1.0.0", "port": "8080"}});
        let mut new = old.clone();
        new.as_object_mut()
            .unwrap()
            .get_mut("app")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .shift_remove("version");

        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        assert!(result.contains("\"my-app\""), "name quotes lost: {result}");
        assert!(result.contains("\"8080\""), "port quotes lost: {result}");
        assert!(!result.contains("version"));
    }
}

mod format_preservation {
    use super::*;
    use ::proptest::prelude::*;

    #[test]
    fn yaml_new_key_in_existing_nested_preserves_sibling_comment() {
        let yaml = "server:\n  # keep host\n  host: localhost\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut newv = old.clone();
        set_at_path(
            &mut newv,
            &crate::selector::parse("server.port").unwrap(),
            json!(9090),
        )
        .unwrap();
        let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
        assert!(result.contains("# keep host"));
        assert!(
            result.contains("9090") || result.contains("port"),
            "new port value missing"
        );
        // Note: when adding a brand-new key inside a sub, current CST may
        // place it at wrong level (fallback produces correct data). Full
        // attachment for new siblings inside subs is a remaining gap.
    }

    // -- TOML comment preservation ----------------------------------------

    #[test]
    fn toml_comment_preserved_on_set() {
        let toml = "# top\n[server]\nhost = \"localhost\" # hostname\nport = 8080\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[
                selector::Segment::Key("server".into()),
                selector::Segment::Key("port".into()),
            ],
            json!(9090),
        )
        .unwrap();

        let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
        assert!(result.contains("# top"), "top comment missing");
        assert!(result.contains("# hostname"), "inline comment missing");
        assert!(result.contains("9090"), "new value missing");
        assert!(!result.contains("8080"), "old value still present");
    }

    #[test]
    fn toml_comment_preserved_on_delete() {
        let toml = "# keep this\n[section]\na = 1\nb = 2 # inline\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        let mut new = old.clone();
        // Delete key "a" from section.
        new.as_object_mut()
            .unwrap()
            .get_mut("section")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("a");

        let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
        assert!(result.contains("# keep this"), "top comment missing");
        assert!(result.contains("# inline"), "inline comment missing");
        assert!(result.contains("b = 2"), "surviving key missing");
        assert!(!result.contains("a = 1"), "deleted key still present");
    }

    #[test]
    fn toml_section_order_preserved() {
        let toml = "[z_last]\nval = 1\n\n[a_first]\nval = 2\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[
                selector::Segment::Key("a_first".into()),
                selector::Segment::Key("val".into()),
            ],
            json!(99),
        )
        .unwrap();

        let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
        let z_pos = result.find("z_last").unwrap();
        let a_pos = result.find("a_first").unwrap();
        assert!(z_pos < a_pos, "section order changed: z@{z_pos} a@{a_pos}");
    }

    #[test]
    fn toml_new_key_inserted_without_breaking_comments() {
        let toml = "# config\n[pkg]\nname = \"app\"\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[
                selector::Segment::Key("pkg".into()),
                selector::Segment::Key("version".into()),
            ],
            json!("1.0"),
        )
        .unwrap();

        let result = serialize_value_preserving(toml, &old, &new, &FileFormat::Toml).unwrap();
        assert!(result.contains("# config"), "comment missing");
        assert!(result.contains("name = \"app\""), "existing key missing");
        assert!(result.contains("version"), "new key missing");
    }

    #[test]
    fn toml_inline_table_style_preserved() {
        let toml = "[deps]\nserde = { version = 1, features = [\"derive\"] }\n";
        let old = parse_doc(toml, &FileFormat::Toml).unwrap();
        // No change — verify round-trip preserves inline style.
        let result = serialize_value_preserving(toml, &old, &old, &FileFormat::Toml).unwrap();
        assert!(result.contains("serde = {"), "inline table style lost");
    }

    // -- YAML comment preservation ----------------------------------------

    #[test]
    fn yaml_comment_preserved_on_set() {
        let yaml = "# top\nserver:\n  host: localhost # hostname\n  port: 8080\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[
                selector::Segment::Key("server".into()),
                selector::Segment::Key("port".into()),
            ],
            json!(9090),
        )
        .unwrap();

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("# top"), "top comment missing");
        assert!(result.contains("# hostname"), "inline comment missing");
        assert!(result.contains("9090"), "new value missing");
        assert!(!result.contains("8080"), "old value still present");
    }

    #[test]
    fn yaml_comment_preserved_on_delete() {
        let yaml = "# keep this\na: 1\nb: 2 # inline\nc: 3\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        // Delete key "a".
        new.as_object_mut().unwrap().remove("a");

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("# keep this"), "top comment missing");
        assert!(result.contains("# inline"), "inline comment missing");
        assert!(result.contains("b: 2"), "surviving key missing");
        assert!(!result.contains("a: 1"), "deleted key still present");
    }

    #[test]
    fn yaml_key_order_preserved() {
        let yaml = "z_last: 1\na_first: 2\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[selector::Segment::Key("a_first".into())],
            json!(99),
        )
        .unwrap();

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        let z_pos = result.find("z_last").unwrap();
        let a_pos = result.find("a_first").unwrap();
        assert!(z_pos < a_pos, "key order changed: z@{z_pos} a@{a_pos}");
    }

    #[test]
    fn yaml_new_key_inserted_without_breaking_comments() {
        let yaml = "# config\nname: app\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        set_at_path(
            &mut new,
            &[selector::Segment::Key("version".into())],
            json!("1.0"),
        )
        .unwrap();

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("# config"), "comment missing");
        assert!(result.contains("name: app"), "existing key missing");
        assert!(result.contains("version"), "new key missing");
    }

    #[test]
    fn yaml_noop_roundtrip_preserves_comments() {
        let yaml = "# top comment\nname: app\n# section\nserver:\n  port: 8080\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        // No change — verify round-trip preserves everything.
        let result =
            serialize_value_preserving(yaml, &old, &old, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert_eq!(result, yaml, "no-op roundtrip should be identical");
    }

    #[test]
    fn yaml_ensure_nested_in_existing_subobject_preserves_top_comments() {
        // Mirrors the integration test_doc_ensure_yaml_preserves_comments
        // server.port ensure when "server" key already exists as object.
        let yaml = "# Config\nname: my-app\n\n# Server\nserver:\n  host: localhost\n";
        let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
        let mut newv = old.clone();
        set_at_path(
            &mut newv,
            &crate::selector::parse("server.port").unwrap(),
            json!(8080),
        )
        .unwrap();
        let result = serialize_value_preserving(yaml, &old, &newv, &FileFormat::Yaml).unwrap();
        // Must preserve top and section comments (the bug was falling back to plain serialize)
        assert!(
            result.contains("# Config"),
            "top comment lost in ensure nested: {result}"
        );
        assert!(
            result.contains("# Server"),
            "section comment lost: {result}"
        );
        let reparsed: serde_json::Value = serde_yaml_ng::from_str(&result).unwrap();
        assert_eq!(
            reparsed,
            json!({"name":"my-app","server":{"host":"localhost","port":8080}})
        );
    }

    #[test]
    fn yaml_section_comment_between_keys_preserved() {
        let yaml = "a: 1\n\n# Section B\nb: 2\n\n# Section C\nc: 3\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let mut new = old.clone();
        set_at_path(&mut new, &[selector::Segment::Key("b".into())], json!(99)).unwrap();

        let result =
            serialize_value_preserving(yaml, &old, &new, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert!(result.contains("# Section B"), "section B comment missing");
        assert!(result.contains("# Section C"), "section C comment missing");
        assert!(result.contains("b: 99"), "new value missing");
        assert!(!result.contains("b: 2"), "old value still present");
    }

    #[test]
    fn yaml_sequence_root_noop_preserves_content() {
        let yaml = "- item1\n- item2\n";
        let old = parse_doc(yaml, &crate::ops::doc::FileFormat::Yaml).unwrap();
        let result =
            serialize_value_preserving(yaml, &old, &old, &crate::ops::doc::FileFormat::Yaml)
                .unwrap();
        assert_eq!(result, yaml, "no-op roundtrip should be identical");
    }

    #[test]
    fn yaml_booleans_need_quoting() {
        for kw in ["true", "false", "yes", "no", "on", "off"] {
            assert!(needs_yaml_quoting(kw), "{kw} should need quoting");
        }
    }

    #[test]
    fn yaml_booleans_case_insensitive() {
        for kw in ["True", "FALSE", "Yes", "NO", "On", "OFF", "TrUe"] {
            assert!(needs_yaml_quoting(kw), "{kw} should need quoting");
        }
    }

    #[test]
    fn yaml_null_and_tilde_need_quoting() {
        assert!(needs_yaml_quoting("null"));
        assert!(needs_yaml_quoting("Null"));
        assert!(needs_yaml_quoting("NULL"));
        assert!(needs_yaml_quoting("~"));
    }

    #[test]
    fn yaml_numbers_need_quoting() {
        assert!(needs_yaml_quoting("42"));
        assert!(needs_yaml_quoting("3.14"));
        assert!(needs_yaml_quoting("-1"));
        assert!(needs_yaml_quoting("0"));
    }

    #[test]
    fn yaml_special_prefix_chars_need_quoting() {
        assert!(needs_yaml_quoting("#comment"));
        assert!(needs_yaml_quoting("&anchor"));
        assert!(needs_yaml_quoting("*alias"));
        assert!(needs_yaml_quoting("?key"));
        assert!(needs_yaml_quoting("|literal"));
        assert!(needs_yaml_quoting(">folded"));
        assert!(needs_yaml_quoting("{flow}"));
        assert!(needs_yaml_quoting("[list]"));
        assert!(needs_yaml_quoting("%directive"));
        assert!(needs_yaml_quoting("@reserved"));
        assert!(needs_yaml_quoting("`backtick"));
        assert!(needs_yaml_quoting("\"quoted"));
        assert!(needs_yaml_quoting("'squoted"));
        assert!(needs_yaml_quoting("!tag"));
    }

    #[test]
    fn yaml_trailing_colon_needs_quoting() {
        assert!(needs_yaml_quoting("host:"));
        assert!(needs_yaml_quoting("value:"));
    }

    #[test]
    fn yaml_special_floats_need_quoting() {
        assert!(needs_yaml_quoting(".inf"));
        assert!(needs_yaml_quoting("-.inf"));
        assert!(needs_yaml_quoting(".nan"));
    }

    #[test]
    fn yaml_colon_space_and_space_hash_need_quoting() {
        assert!(needs_yaml_quoting("key: value"));
        assert!(needs_yaml_quoting("hello #comment"));
    }

    ::proptest::proptest! {
        /// JSON preserving round-trip: set a key, serialize preserving, reparse.
                #[test]
                fn json_preserving_set_round_trip(
                    key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
                    set_value in prop_oneof![
                        any::<bool>().prop_map(|b| json!(b)),
                        any::<i64>().prop_map(|n| json!(n)),
                        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                    ],
                ) {
                    let original_content = "{\"existing\": 1}";
                    let original_value = parse_doc(original_content, &FileFormat::Json).unwrap();
                    let mut new_value = original_value.clone();
                    let sel = selector::parse(&key).unwrap();
                    set_at_path(&mut new_value, &sel, set_value.clone()).unwrap();

                    let serialized = serialize_value_preserving(
                        original_content,
                        &original_value,
                        &new_value,
                        &FileFormat::Json,
                    ).unwrap();
                    let reparsed = parse_doc(&serialized, &FileFormat::Json).unwrap();
                    prop_assert_eq!(&new_value, &reparsed);
                }
    }
}

mod proptest {
    use super::*;
    use ::proptest::prelude::*;

    /// Generate an arbitrary JSON value (limited depth to keep tests fast).
    fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
        let leaf = prop_oneof![
            Just(json!(null)),
            any::<bool>().prop_map(|b| json!(b)),
            any::<i64>().prop_map(|n| json!(n)),
            "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
        ];
        leaf.prop_recursive(3, 32, 5, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..5).prop_map(serde_json::Value::Array),
                prop::collection::vec(("[a-zA-Z_][a-zA-Z0-9_]{0,10}", inner), 0..5).prop_map(
                    |entries| {
                        let map: serde_json::Map<String, serde_json::Value> =
                            entries.into_iter().collect();
                        serde_json::Value::Object(map)
                    }
                ),
            ]
        })
    }

    proptest! {
        /// JSON round-trip: serialize then reparse must produce the same value.
                #[test]
                fn json_round_trip(value in arb_json_value()) {
                    let serialized = serialize_value(&value, &FileFormat::Json).unwrap();
                    let reparsed = parse_doc(&serialized, &FileFormat::Json).unwrap();
                    prop_assert_eq!(&value, &reparsed);
                }

        /// TOML round-trip for object values (TOML root must be a table).
                #[test]
                fn toml_round_trip(entries in prop::collection::vec(
                    ("[a-zA-Z_][a-zA-Z0-9_]{0,10}", prop_oneof![
                        any::<bool>().prop_map(|b| json!(b)),
                        any::<i64>().prop_map(|n| json!(n)),
                        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                    ]),
                    0..8
                )) {
                    let map: serde_json::Map<String, serde_json::Value> =
                        entries.into_iter().collect();
                    let value = serde_json::Value::Object(map);
                    let serialized = serialize_value(&value, &FileFormat::Toml).unwrap();
                    let reparsed = parse_doc(&serialized, &FileFormat::Toml).unwrap();
                    prop_assert_eq!(&value, &reparsed);
                }

        /// YAML round-trip: serialize then reparse must produce the same value.
                #[test]
                fn yaml_round_trip(value in arb_json_value()) {
                    let serialized = serialize_value(&value, &crate::ops::doc::FileFormat::Yaml).unwrap();
                    let reparsed = parse_doc(&serialized, &crate::ops::doc::FileFormat::Yaml).unwrap();
                    prop_assert_eq!(&value, &reparsed);
                }

        /// JSON set-then-get: setting a key and retrieving it must return the set value.
                #[test]
                fn json_set_then_get(
                    key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
                    set_value in prop_oneof![
                        any::<bool>().prop_map(|b| json!(b)),
                        any::<i64>().prop_map(|n| json!(n)),
                        "[a-zA-Z0-9_]{0,20}".prop_map(|s| json!(s)),
                    ],
                ) {
                    let mut root = json!({});
                    let sel = selector::parse(&key).unwrap();
                    set_at_path(&mut root, &sel, set_value.clone()).unwrap();
                    let results = selector::eval(&root, &sel);
                    prop_assert_eq!(results.len(), 1);
                    prop_assert_eq!(results[0], &set_value);
                }

        /// JSON delete-then-has: deleting a key means it no longer exists.
                #[test]
                fn json_delete_then_has(
                    key in "[a-zA-Z_][a-zA-Z0-9_]{0,10}",
                ) {
                    let mut root = json!({ &key: "value" });
                    let sel = selector::parse(&key).unwrap();
                    let deleted = delete_at_selector(&mut root, &sel).unwrap();
                    prop_assert!(deleted);
                    let results = selector::eval(&root, &sel);
                    prop_assert!(results.is_empty());
                }
    }
}

#[test]
fn presentation_style_changed_flags_yaml_sequence_indent() {
    let before = "env:\n  - name: FEATURE_FLAG\n    value: off\n";
    let after = "env:\n- name: FEATURE_FLAG\n  value: on\n";
    assert!(
        presentation_style_changed(before, after, &FileFormat::Yaml),
        "indented vs collapsed block sequence must set style_changed"
    );
    let after_same = "env:\n  - name: FEATURE_FLAG\n    value: on\n";
    assert!(
        !presentation_style_changed(before, after_same, &FileFormat::Yaml),
        "same sequence indent should not flag style"
    );
    assert!(!presentation_style_changed(
        before,
        before,
        &FileFormat::Yaml
    ));
}

#[test]
fn presentation_style_changed_flags_yaml_alias_identity() {
    let before = "shared: &shared\n  timeout: 30\nservice_a: *shared\n";
    let after = "shared:\n  timeout: 30\nservice_a:\n  timeout: 30\n";
    assert!(
        presentation_style_changed(before, after, &FileFormat::Yaml),
        "dump that drops &/* identity must set style_changed"
    );
    let after_indent = "env:\n  - name: FEATURE_FLAG\n    value: on\n";
    let before_indent = "env:\n  - name: FEATURE_FLAG\n    value: off\n";
    assert!(
        !presentation_style_changed(before_indent, after_indent, &FileFormat::Yaml),
        "indent-only same block sequence must stay false"
    );
}

#[test]
fn presentation_style_changed_flags_yaml_alias_dump() {
    let yaml = "\
shared: &shared
  timeout: 30
flow: {cfg: *shared}
block:
  cfg: *shared
";
    let old = parse_doc(yaml, &FileFormat::Yaml).unwrap();
    let mut new = old.clone();
    new["flow"]["cfg"]["timeout"] = json!(60);
    let dumped = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
    assert!(
        !dumped.contains("&shared") && !dumped.contains("*shared"),
        "fixture must dump without alias identity:\n{dumped}"
    );
    assert!(
        presentation_style_changed(yaml, &dumped, &FileFormat::Yaml),
        "alias-inlining dump must set style_changed:\n{dumped}"
    );
}
