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
        let found = navigate_mut(&mut val, &seg, false).unwrap();
        assert_eq!(*found, json!(42));
    }

    #[test]
    fn navigate_mut_create_missing_key() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b.c").unwrap();
        let found = navigate_mut(&mut val, &seg, true).unwrap();
        // created as empty object, then descended into "c" which was also created
        assert!(found.is_object());
    }

    #[test]
    fn navigate_mut_array_index() {
        let mut val = json!({"items": [10, 20, 30]});
        let seg = crate::selector::parse("items[1]").unwrap();
        let found = navigate_mut(&mut val, &seg, false).unwrap();
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

    #[test]
    fn delete_where_removes_matching_items() {
        let mut root = json!({"items": [{"name": "a"}, {"name": "b"}, {"name": "c"}]});
        let sel = crate::selector::parse("items").unwrap();
        let removed = delete_where(&mut root, &sel, "name=b").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(root["items"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn delete_at_selector_removes_key() {
        let mut root = json!({"a": 1, "b": 2});
        let sel = crate::selector::parse("b").unwrap();
        assert!(delete_at_selector(&mut root, &sel).unwrap());
        assert_eq!(root, json!({"a": 1}));
    }

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
        let count = update_matching(&mut val, &seg, &json!("new"));
        assert_eq!(count, 1);
        assert_eq!(val, json!({"a": {"b": "new"}}));
    }

    #[test]
    fn update_matching_wildcard() {
        let mut val = json!({"items": [{"v": 1}, {"v": 2}]});
        let seg = crate::selector::parse("items[*].v").unwrap();
        let count = update_matching(&mut val, &seg, &json!(99));
        assert_eq!(count, 2);
        assert_eq!(val, json!({"items": [{"v": 99}, {"v": 99}]}));
    }

    #[test]
    fn update_matching_predicate() {
        let mut val = json!({"items": [
            {"name": "a", "v": 1},
            {"name": "b", "v": 2}
        ]});
        let seg = crate::selector::parse("items[name=b].v").unwrap();
        let count = update_matching(&mut val, &seg, &json!(42));
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
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"a": 1}));
    }

    #[test]
    fn mutation_merge() {
        let mut root = json!({"a": 1});
        let result = apply_doc_mutation(
            &mut root,
            DocMutation::Merge {
                value: json!({"b": 2}),
            },
        )
        .unwrap();
        assert!(matches!(result, MutationResult::Applied));
        assert_eq!(root, json!({"a": 1, "b": 2}));
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
        assert!(matches!(result, MutationResult::Applied));
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
    fn parse_value_plain_string() {
        let result = parse_value("hello");
        assert_eq!(result, json!("hello"));
        assert!(result.is_string());
    }

    #[test]
    fn parse_value_true_is_bool() {
        // "true" is recognized as boolean, not left as a string.
        let result = parse_value("true");
        assert_eq!(result, json!(true));
        assert!(result.is_boolean());
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

    #[test]
    fn delete_at_selector_missing_returns_false() {
        let mut root = json!({"a": 1});
        let sel = crate::selector::parse("nonexistent").unwrap();
        assert!(!delete_at_selector(&mut root, &sel).unwrap());
    }

    #[test]
    fn delete_at_selector_empty_returns_false() {
        let mut root = json!({"a": 1});
        let sel: Vec<crate::selector::Segment> = vec![];
        assert!(!delete_at_selector(&mut root, &sel).unwrap());
    }

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
    fn update_matching_missing_key_returns_zero() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b.c").unwrap();
        let count = update_matching(&mut val, &seg, &json!("x"));
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
        assert!(detect_format("readme.txt").is_err());
    }

    #[test]
    fn detect_format_no_extension() {
        assert!(detect_format("Makefile").is_err());
    }

    #[test]
    fn navigate_mut_missing_key_no_create() {
        let mut val = json!({"a": 1});
        let seg = crate::selector::parse("b").unwrap();
        assert!(navigate_mut(&mut val, &seg, false).is_err());
    }

    #[test]
    fn navigate_mut_index_out_of_bounds() {
        let mut val = json!({"items": [10]});
        let seg = crate::selector::parse("items[5]").unwrap();
        assert!(navigate_mut(&mut val, &seg, false).is_err());
    }

    #[test]
    fn set_at_path_out_of_bounds_index_fails() {
        let mut root = json!({"items": [1]});
        let sel = crate::selector::parse("items[5]").unwrap();
        assert!(set_at_path(&mut root, &sel, json!(99)).is_err());
    }

    #[test]
    fn set_at_path_empty_selector_fails() {
        let mut root = json!({});
        let sel: Vec<crate::selector::Segment> = vec![];
        assert!(set_at_path(&mut root, &sel, json!(1)).is_err());
    }

    #[test]
    fn delete_where_invalid_predicate_fails() {
        let mut root = json!({"items": [{"name": "a"}]});
        let sel = crate::selector::parse("items").unwrap();
        assert!(delete_where(&mut root, &sel, "no-equals-sign").is_err());
    }

    #[test]
    fn delete_where_non_array_fails() {
        let mut root = json!({"items": "not-an-array"});
        let sel = crate::selector::parse("items").unwrap();
        assert!(delete_where(&mut root, &sel, "k=v").is_err());
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
        assert!(move_at_path(&mut root, &from, &to).is_err());
    }

    #[test]
    fn move_at_path_empty_from_selector_fails() {
        let mut root = json!({"a": 1});
        let from: Vec<crate::selector::Segment> = vec![];
        let to = crate::selector::parse("b").unwrap();
        assert!(move_at_path(&mut root, &from, &to).is_err());
    }

    #[test]
    fn yaml_multi_document_rejected_by_parser() {
        // Multi-document YAML (--- separated) is rejected by serde_yaml_ng.
        // This test documents the current behavior: parse_doc returns an error
        // rather than silently dropping documents.
        let yaml = "---\nname: first\n---\nname: second\n";
        let err = parse_doc(yaml, &FileFormat::Yaml).unwrap_err();
        assert!(
            err.to_string().contains("more than one document"),
            "expected multi-document rejection, got: {err}"
        );
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

    #[test]
    fn deep_merge_depth_limit() {
        // Build a deeply nested structure beyond MAX_MERGE_DEPTH (128)
        let mut deep_val = json!("leaf");
        for _ in 0..130 {
            deep_val = json!({"n": deep_val});
        }
        let mut base = json!({});
        deep_merge(&mut base, &deep_val);
        // Should not panic; at depth 128 it clones instead of recursing
        assert!(base.is_object());
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
        // Regression: has_array_growth_diffs must recurse into same-length
        // array elements to detect nested growth (e.g., adding a port to
        // the first server's ports array while the outer array stays the
        // same length).
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
    fn delete_key_with_preceding_standalone_comment() {
        let yaml = "server:\n  host: localhost\n  port: 8080\n  # Worker count\n  workers: 4\n";
        let old = json!({"server": {"host": "localhost", "port": 8080, "workers": 4}});
        let new = json!({"server": {"host": "localhost", "port": 8080}});
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // The standalone comment should NOT leak onto the previous line.
        for line in result.lines() {
            if line.contains("port") {
                assert!(
                    !line.contains('#'),
                    "standalone comment leaked onto port line: {:?}",
                    line
                );
            }
        }
        assert!(
            !result.contains("Worker count"),
            "orphaned comment should be removed"
        );
        assert!(!result.contains("workers"), "deleted key should be gone");
        assert!(result.contains("port: 8080"));
    }

    #[test]
    fn delete_key_preserves_legitimate_inline_comments() {
        let yaml = "server:\n  host: localhost # primary host\n  port: 8080\n  # Remove this\n  workers: 4\n";
        let old = json!({"server": {"host": "localhost", "port": 8080, "workers": 4}});
        let new = json!({"server": {"host": "localhost", "port": 8080}});
        let result = serialize_value_preserving(yaml, &old, &new, &FileFormat::Yaml).unwrap();
        // Legitimate inline comment on host should be preserved.
        assert!(
            result.contains("# primary host"),
            "legitimate inline comment was incorrectly removed"
        );
        // Orphaned standalone comment should not appear inline.
        assert!(
            !result.contains("# Remove this"),
            "orphaned comment should be removed"
        );
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
