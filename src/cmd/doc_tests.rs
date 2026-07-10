use super::*;
use std::fs;
use tempfile::TempDir;

/// Helper: write a file into a temp directory and return its path.
fn write_file(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    path.to_str().unwrap().to_string()
}

// -- Helper to run a doc write action through run() -------------------

fn run_doc(action: DocAction, global: &GlobalFlags) -> anyhow::Result<u8> {
    run(
        DocArgs {
            action,
            write: Default::default(),
        },
        global,
    )
}

mod basic {
    use super::*;

    // -- get ----------------------------------------------------------------

    #[test]
    fn get_returns_value_from_json() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "count": 42}"#);
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_yaml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.yaml", "name: hello\ncount: 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    #[test]
    fn get_returns_value_from_toml() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.toml", "name = \"hello\"\ncount = 42\n");
        let action = DocAction::Get {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "hello");
    }

    // -- has ----------------------------------------------------------------

    #[test]
    fn has_returns_true_for_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "name".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "true");
    }

    #[test]
    fn has_returns_no_matches_for_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Has {
            file: path,
            selector: "missing".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(
            code,
            exit::NO_MATCHES,
            "doc has should return NO_MATCHES when selector is absent"
        );
        assert_eq!(output, "false");
    }

    #[test]
    fn has_exit_code_consistent_with_get() {
        // Regression: doc has always returned SUCCESS even for missing keys,
        // while doc get returned NO_MATCHES. Both should use NO_MATCHES.
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "data.json", r#"{"a": 1}"#);

        // Existing key: both return SUCCESS.
        let (_, has_code) = execute_with_mode(
            &DocAction::Has {
                file: path.clone(),
                selector: "a".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        let (_, get_code) = execute_with_mode(
            &DocAction::Get {
                file: path.clone(),
                selector: "a".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        assert_eq!(has_code, exit::SUCCESS);
        assert_eq!(get_code, exit::SUCCESS);

        // Missing key: both return NO_MATCHES.
        let (_, has_code) = execute_with_mode(
            &DocAction::Has {
                file: path.clone(),
                selector: "missing".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        let (_, get_code) = execute_with_mode(
            &DocAction::Get {
                file: path,
                selector: "missing".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        assert_eq!(has_code, exit::NO_MATCHES);
        assert_eq!(get_code, exit::NO_MATCHES);
    }

    // -- keys ---------------------------------------------------------------

    #[test]
    fn keys_lists_object_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"scripts": {"build": "tsc", "lint": "eslint", "test": "jest"}}"#,
        );
        let action = DocAction::Keys {
            file: path,
            selector: "scripts".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let keys: Vec<&str> = output.split('\n').collect();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"build"));
        assert!(keys.contains(&"lint"));
        assert!(keys.contains(&"test"));
    }

    // -- len ----------------------------------------------------------------

    #[test]
    fn len_counts_array_elements() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3, 4, 5]}"#);
        let action = DocAction::Len {
            file: path,
            selector: "items".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "5");
    }

    // -- set ----------------------------------------------------------------

    #[test]
    fn set_creates_new_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "age".into(),
            value: "42".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["age"], serde_json::json!(42));
    }

    #[test]
    fn set_updates_existing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("world"));
    }

    // -- delete -------------------------------------------------------------

    #[test]
    fn delete_removes_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello", "age": 42}"#);
        let action = DocAction::Delete {
            file: path.clone(),
            selector: "age".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(val.get("age").is_none());
    }

    // -- delete-where -------------------------------------------------------

    #[test]
    fn delete_where_removes_matching_items() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"users": [{"name": "alice"}, {"name": "bob"}, {"name": "carol"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path.clone(),
            selector: "users".into(),
            predicate: "name=bob".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let arr = val["users"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], serde_json::json!("alice"));
        assert_eq!(arr[1]["name"], serde_json::json!("carol"));
    }

    #[test]
    fn delete_where_removes_multiple_matches() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"items": [{"status": "done"}, {"status": "pending"}, {"status": "done"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path.clone(),
            selector: "items".into(),
            predicate: "status=done".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let arr = val["items"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["status"], serde_json::json!("pending"));
    }

    // -- append / prepend ---------------------------------------------------

    #[test]
    fn append_adds_to_array() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3]}"#);
        let action = DocAction::Append {
            file: path.clone(),
            selector: "items".into(),
            value: "4".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["items"], serde_json::json!([1, 2, 3, 4]));
    }

    #[test]
    fn prepend_adds_to_beginning_of_array() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"items": [1, 2, 3]}"#);
        let action = DocAction::Prepend {
            file: path.clone(),
            selector: "items".into(),
            value: "0".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["items"], serde_json::json!([0, 1, 2, 3]));
    }

    #[test]
    fn merge_combines_objects() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Merge {
            file: path.clone(),
            stdin: false,
            value: Some(r#"{"age": 42}"#.into()),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("hello"));
        assert_eq!(val["age"], serde_json::json!(42));
    }

    #[test]
    fn merge_rejects_stdin_and_value_together() {
        // Regression: --stdin and --value are mutually exclusive. Previously
        // providing both silently ignored --value.
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a": 1}"#);
        let action = DocAction::Merge {
            file: path,
            stdin: true,
            value: Some(r#"{"b": 2}"#.into()),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let result = run_doc(action, &global);
        assert!(result.is_err(), "merge --stdin --value should fail");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("mutually exclusive"),
            "error should mention mutual exclusivity: {err_msg}"
        );
    }

    #[test]
    fn ensure_creates_missing_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Ensure {
            file: path.clone(),
            selector: "age".into(),
            value: "30".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["age"], serde_json::json!(30));
        assert_eq!(val["name"], serde_json::json!("hello"));
    }

    // -- move ---------------------------------------------------------------

    #[test]
    fn move_renames_key() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"old_name": "value"}"#);
        let action = DocAction::Move {
            file: path.clone(),
            from: "old_name".into(),
            to: "new_name".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["new_name"], serde_json::json!("value"));
        assert!(val.get("old_name").is_none());
    }

    // -- apply writes file --------------------------------------------------

    #[test]
    fn set_with_apply_writes_file() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("world"));
    }

    // -- check mode ---------------------------------------------------------

    #[test]
    fn set_with_check_reports_changes() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Set {
            file: path,
            selector: "name".into(),
            value: "world".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
    }

    #[test]
    fn diff_detects_added_removed_changed() {
        let dir = TempDir::new().unwrap();
        let a = write_file(
            &dir,
            "a.json",
            r#"{"name":"old","version":1,"removed":true}"#,
        );
        let b = write_file(
            &dir,
            "b.json",
            r#"{"name":"new","version":1,"added":"yes"}"#,
        );
        let action = DocAction::Diff {
            file_a: a,
            file_b: b,
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(output.contains("~ name"), "should show changed: {output}");
        assert!(
            output.contains("- removed"),
            "should show removed: {output}"
        );
        assert!(output.contains("+ added"), "should show added: {output}");
        assert!(
            !output.contains("version"),
            "unchanged key should not appear: {output}"
        );
    }

    #[test]
    fn len_counts_object_keys() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a": 1, "b": 2, "c": 3}"#);
        let action = DocAction::Len {
            file: path,
            selector: String::new(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "3");
    }

    // -- flatten ------------------------------------------------------------

    #[test]
    fn set_apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"version": "1.0"}"#);
        let action = DocAction::Set {
            file: path.clone(),
            selector: "version".into(),
            value: "\"2.0\"".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(
            backup_dir.exists(),
            "backup directory should exist after doc set --apply"
        );
        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(
            !sessions.is_empty(),
            "at least one backup session should be created"
        );
    }

    #[test]
    fn flatten_enumerates_leaf_paths() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"a":1,"b":{"c":2,"d":3},"e":[10,20]}"#,
        );
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(output.contains("a = 1"), "missing a: {output}");
        assert!(output.contains("b.c = 2"), "missing b.c: {output}");
        assert!(output.contains("b.d = 3"), "missing b.d: {output}");
        assert!(output.contains("e[0] = 10"), "missing e[0]: {output}");
        assert!(output.contains("e[1] = 20"), "missing e[1]: {output}");
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn delete_where_no_match_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(
            &dir,
            "test.json",
            r#"{"users": [{"name": "alice"}, {"name": "bob"}]}"#,
        );
        let action = DocAction::DeleteWhere {
            file: path,
            selector: "users".into(),
            predicate: "name=nobody".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        // Engine treats delete-where no-match as success (idempotent).
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn delete_missing_key_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Delete {
            file: path,
            selector: "nonexistent".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        // Engine treats delete no-match as success (idempotent).
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    // -- ensure -------------------------------------------------------------

    #[test]
    fn ensure_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "original"}"#);
        // Ensure on an existing key should not overwrite.
        let action = DocAction::Ensure {
            file: path.clone(),
            selector: "name".into(),
            value: "overwritten".into(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        // Value should remain "original" (not overwritten).
        let val: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(val["name"], serde_json::json!("original"));
    }

    #[test]
    fn diff_identical_files() {
        let dir = TempDir::new().unwrap();
        let a = write_file(&dir, "a.json", r#"{"k":1}"#);
        let b = write_file(&dir, "b.json", r#"{"k":1}"#);
        let action = DocAction::Diff {
            file_a: a,
            file_b: b,
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(output, "identical");
    }

    #[test]
    fn diff_json_wraps_differences_in_object() {
        let dir = TempDir::new().unwrap();
        let a = write_file(&dir, "a.json", r#"{"name":"old","version":1}"#);
        let b = write_file(&dir, "b.json", r#"{"name":"new","version":1}"#);
        let action = DocAction::Diff {
            file_a: a,
            file_b: b,
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["identical"], false);
        assert!(v["differences"].is_array());
        assert!(!v["differences"].as_array().unwrap().is_empty());
    }

    #[test]
    fn flatten_includes_empty_arrays() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"tags":[],"name":"foo","items":[1]}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["tags"], serde_json::json!([]));
        assert_eq!(parsed["name"], serde_json::json!("foo"));
        assert_eq!(parsed["items[0]"], serde_json::json!(1));
    }

    #[test]
    fn flatten_includes_empty_objects() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"config":{},"name":"bar"}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["config"], serde_json::json!({}));
        assert_eq!(parsed["name"], serde_json::json!("bar"));
    }

    #[test]
    fn flatten_includes_nested_empty_containers() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a":{"b":[],"c":{}},"d":[1]}"#);
        let action = DocAction::Flatten { file: path };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["a.b"], serde_json::json!([]));
        assert_eq!(parsed["a.c"], serde_json::json!({}));
        assert_eq!(parsed["d[0]"], serde_json::json!(1));
    }
}

mod error_handling {
    use super::*;

    // -- missing selector ---------------------------------------------------

    #[test]
    fn get_missing_selector_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Get {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Text).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        assert!(output.is_empty());
    }

    #[test]
    fn get_missing_selector_json_emits_error_object() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Get {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("no match"));
    }

    #[test]
    fn keys_missing_selector_json_emits_error_object() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Keys {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("no match"));
    }

    #[test]
    fn len_missing_selector_json_emits_error_object() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Len {
            file: path,
            selector: "nonexistent".into(),
        };
        let (output, code) = execute_with_mode(&action, OutputMode::Json).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        let v: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("no match"));
    }

    // -- error path tests ---------------------------------------------------

    #[test]
    fn keys_on_scalar_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        // Text mode: error goes to stderr, output is empty. Verify exit code.
        let (_output, code) = execute_with_mode(
            &DocAction::Keys {
                file: path.clone(),
                selector: "name".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
        // JSON mode: error is wrapped in a JSON envelope.
        let (json_output, json_code) = execute_with_mode(
            &DocAction::Keys {
                file: path,
                selector: "name".into(),
            },
            OutputMode::Json,
        )
        .unwrap();
        assert_eq!(json_code, exit::FAILURE);
        assert!(json_output.contains("not an object"));
        assert!(
            json_output.contains(r#""error_kind": "type_error""#)
                || json_output.contains(r#""error_kind":"type_error""#),
            "JSON type mismatch should set error_kind type_error: {json_output}"
        );
    }

    #[test]
    fn len_on_scalar_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        // Text mode: error goes to stderr, output is empty. Verify exit code.
        let (_output, code) = execute_with_mode(
            &DocAction::Len {
                file: path.clone(),
                selector: "name".into(),
            },
            OutputMode::Text,
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
        // JSON mode: error is wrapped in a JSON envelope.
        let (json_output, json_code) = execute_with_mode(
            &DocAction::Len {
                file: path,
                selector: "name".into(),
            },
            OutputMode::Json,
        )
        .unwrap();
        assert_eq!(json_code, exit::FAILURE);
        assert!(json_output.contains("not an array or object"));
        assert!(
            json_output.contains(r#""error_kind": "type_error""#)
                || json_output.contains(r#""error_kind":"type_error""#),
            "JSON type mismatch should set error_kind type_error: {json_output}"
        );
    }

    #[test]
    fn append_to_non_array_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Append {
            file: path,
            selector: "name".into(),
            value: "42".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn prepend_to_non_array_returns_failure() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"name": "hello"}"#);
        let action = DocAction::Prepend {
            file: path,
            selector: "name".into(),
            value: "42".into(),
        };
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_doc(action, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}

mod security {
    #[allow(unused_imports)]
    use super::*;

    // -- merge --------------------------------------------------------------

    #[test]
    fn deep_merge_depth_guard_caps_recursion() {
        use crate::ops::doc::deep_merge;
        // Build a JSON tree nested 200 levels deep (exceeds MAX_MERGE_DEPTH of 128).
        // The guard stops recursing at depth 128 and overwrites with the
        // remaining subtree, preventing stack overflow on adversarial input.
        let mut base = serde_json::json!(null);
        let mut other = serde_json::json!({"leaf": true});
        for _ in 0..200 {
            other = serde_json::json!({"nested": other});
        }
        deep_merge(&mut base, &other);
        // Verify the result is a valid object (not a crash) and the
        // top-level structure was preserved.
        assert!(base.is_object());
        assert!(
            base.get("nested").is_some(),
            "top-level 'nested' key must exist"
        );
        // Walk down to verify nesting was preserved (not just top level).
        let mut cursor = &base;
        for _ in 0..10 {
            cursor = cursor
                .get("nested")
                .expect("nesting should be at least 10 levels deep");
        }
        assert!(cursor.is_object());
    }

    #[test]
    fn execute_with_mode_rejects_write_action() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "t.json", r#"{"a":1}"#);
        let action = DocAction::Delete {
            file: path,
            selector: "a".into(),
        };
        let result = execute_with_mode(&action, OutputMode::Text);
        assert!(result.is_err(), "expected error, got Ok: {result:?}");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("write operations require"),
            "should reject write actions routed through execute_with_mode"
        );
    }
}

// -- format_no_match helper tests ------------------------------------------

mod format_no_match_tests {
    use super::*;

    #[test]
    fn json_mode_returns_structured_error() {
        let (output, code) = format_no_match("selector missed", OutputMode::Json, false).unwrap();
        assert_eq!(code, crate::exit::NO_MATCHES);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error"], "selector missed");
    }

    #[test]
    fn jsonl_mode_returns_compact_error() {
        let (output, code) = format_no_match("selector missed", OutputMode::Jsonl, false).unwrap();
        assert_eq!(code, crate::exit::NO_MATCHES);
        // JSONL output should be a single compact line (no internal newlines).
        assert!(!output.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["ok"], false);
    }

    #[test]
    fn text_mode_returns_empty_string_and_emits_stderr() {
        // In text mode, format_no_match emits to stderr and returns empty
        // stdout. We can't capture stderr in a unit test, but we verify
        // the return value is correct.
        let (output, code) = format_no_match("selector missed", OutputMode::Text, false).unwrap();
        assert_eq!(code, crate::exit::NO_MATCHES);
        assert!(output.is_empty());
    }

    #[test]
    fn text_mode_quiet_suppresses_stderr() {
        let (output, code) = format_no_match("selector missed", OutputMode::Text, true).unwrap();
        assert_eq!(code, crate::exit::NO_MATCHES);
        assert!(output.is_empty());
    }
}

// -- error path coverage --------------------------------------------------

mod error_paths {
    use super::*;

    #[test]
    fn merge_requires_stdin_or_value() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a": 1}"#);
        let action = DocAction::Merge {
            file: path,
            stdin: false,
            value: None,
        };
        let err = run_doc(action, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string()
                .contains("merge requires --stdin or --value"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn merge_rejects_stdin_and_value_together() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "test.json", r#"{"a": 1}"#);
        let action = DocAction::Merge {
            file: path,
            stdin: true,
            value: Some(r#"{"b": 2}"#.to_string()),
        };
        let err = run_doc(action, &GlobalFlags::test_default()).unwrap_err();
        assert!(
            err.to_string()
                .contains("--stdin and --value are mutually exclusive"),
            "unexpected error: {err}"
        );
    }
}
