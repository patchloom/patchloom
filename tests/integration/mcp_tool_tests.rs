use super::*;

#[tokio::test]
async fn test_mcp_doc_set_round_trip() {
    if !has_mcp_support() {
        return; // binary built with --no-default-features (no MCP)
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"old","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_set",
        serde_json::json!({"path": "config.json", "selector": "name", "value": "new"}),
    )
    .await;
    assert!(!is_error, "doc_set should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_set ok field: {val}");
    assert_eq!(val["files_changed"], 1, "doc_set files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], "new", "doc_set did not update file: {content}");
    assert_eq!(
        v["version"], "1.0",
        "doc_set clobbered other keys: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({"path": "hello.txt", "old": "world", "new": "patchloom"}),
    )
    .await;
    assert!(!is_error, "replace should succeed: {val}");
    assert_eq!(val["ok"], true, "replace ok field: {val}");
    assert_eq!(val["files_changed"], 1, "replace files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert_eq!(content, "hello patchloom\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_read_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "line1\nline2\nline3\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "read_file",
        serde_json::json!({"path": "data.txt"}),
    )
    .await;
    assert!(!is_error, "read should succeed");
    // read_file returns structured JSON with reads[0].content holding the file text.
    let content = val["reads"][0]["content"]
        .as_str()
        .expect("reads[0].content should be a string");
    assert_eq!(
        content, "line1\nline2\nline3\n",
        "read should return exact file content"
    );
    assert_eq!(val["reads"][0]["total_lines"], 3, "should have 3 lines");
    assert_eq!(val["reads"][0]["path"], "data.txt");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_set_nonexistent_file_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_set",
        serde_json::json!({"path": "nope.json", "selector": "x", "value": 1}),
    )
    .await;
    assert!(
        is_error,
        "doc_set on nonexistent file should return error: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_finds_pattern() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("haystack.txt"),
        "first line\nsecond needle line\nthird line\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "search_files",
        serde_json::json!({"pattern": "needle", "paths": ["haystack.txt"]}),
    )
    .await;
    assert!(!is_error, "search should succeed: {val}");
    assert_eq!(
        val["match_count"], 1,
        "should find exactly one match: {val}"
    );
    assert_eq!(val["file_count"], 1, "should match in one file: {val}");
    assert_eq!(
        val["matches"][0]["path"], "haystack.txt",
        "match should be in haystack.txt: {val}"
    );
    assert!(
        val["matches"][0]["text"]
            .as_str()
            .unwrap_or("")
            .contains("needle"),
        "match text should contain 'needle': {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_multiline_returns_json_matches() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("multi.txt"), "hello\nworld\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "search_files",
        serde_json::json!({
            "pattern": "hello.*world",
            "paths": ["multi.txt"],
            "multiline": true
        }),
    )
    .await;
    assert!(!is_error, "multiline search should succeed: {val}");
    assert_eq!(val["match_count"], 1);
    assert_eq!(val["file_count"], 1);
    assert_eq!(val["matches"][0]["path"], "multi.txt");
    assert_eq!(val["matches"][0]["line"], 1);
    assert_eq!(val["matches"][0]["column"], 1);
    assert_eq!(val["matches"][0]["text"], "hello\nworld");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_invert_match_returns_non_matching_lines() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lines.txt"), "alpha\nbeta\ngamma\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "search_files",
        serde_json::json!({
            "pattern": "beta",
            "paths": ["lines.txt"],
            "invert_match": true
        }),
    )
    .await;
    assert!(!is_error, "invert_match search should succeed: {val}");
    assert_eq!(val["match_count"], 2);
    assert_eq!(val["matches"][0]["text"], "alpha");
    assert_eq!(val["matches"][1]["text"], "gamma");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_assert_count_match() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rep.txt"), "foo\nfoo\nbar\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "search_files",
        serde_json::json!({
            "pattern": "foo",
            "paths": ["rep.txt"],
            "assert_count": 2
        }),
    )
    .await;
    assert!(!is_error, "assert_count=2 should succeed: {val}");
    assert_eq!(val["assert_count"]["expected"], 2);
    assert_eq!(val["assert_count"]["actual"], 2);
    assert_eq!(val["assert_count"]["matched"], true);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_assert_count_mismatch() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rep.txt"), "foo\nfoo\nbar\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "search_files",
        serde_json::json!({
            "pattern": "foo",
            "paths": ["rep.txt"],
            "assert_count": 5
        }),
    )
    .await;
    // assert_count mismatch is a "changes_detected" result, reported as is_error=true
    assert!(is_error, "assert_count mismatch should signal error: {val}");
    assert_eq!(val["assert_count"]["expected"], 5);
    assert_eq!(val["assert_count"]["actual"], 2);
    assert_eq!(val["assert_count"]["matched"], false);
    assert_eq!(
        val["error_kind"], "changes_detected",
        "MCP assert_count mismatch must set error_kind: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_has_existing_key() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"alice","age":30}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "has", "path": "data.json", "selector": "name"}),
    )
    .await;
    assert!(!is_error, "doc_query has should succeed: {val}");
    assert_eq!(
        val, true,
        "doc_query has should return true for existing key: {val}"
    );
    client.cancel().await.unwrap();
}

/// Regression: doc_query "has" for a missing key must return is_error=false
/// with value "false", not is_error=true. The "has" action's purpose is to
/// check existence; "false" is a valid answer, not an error.
#[tokio::test]
async fn test_mcp_doc_has_missing_key_not_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"alice"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "has", "path": "data.json", "selector": "missing_key"}),
    )
    .await;
    assert!(
        !is_error,
        "doc_query has for missing key should NOT be an error: {val}"
    );
    assert_eq!(
        val, false,
        "doc_query has should return false for missing key: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_get_reads_value() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"version":"2.1.0","debug":false}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_get",
        serde_json::json!({"path": "config.json", "selector": "version"}),
    )
    .await;
    assert!(!is_error, "doc_get should succeed: {val}");
    assert_eq!(val, "2.1.0", "doc_get should return the exact value: {val}");
    client.cancel().await.unwrap();
}

/// #1467: singular `path` is accepted as an alias for `paths: [path]`.
#[tokio::test]
async fn test_mcp_search_files_accepts_path_alias() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("fixtures").join("ast-rename");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("svc.py"), "class OrderService:\n    pass\n").unwrap();
    fs::write(
        dir.path().join("root.py"),
        "class OrderService:\n    pass\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "search_files",
        serde_json::json!({
            "pattern": "OrderService",
            "path": "fixtures/ast-rename",
            "literal": true
        }),
    )
    .await;
    assert!(!is_error, "path alias must deserialize and run: {text}");
    assert!(
        text.contains("OrderService") || text.contains("svc.py"),
        "should find match under path alias root: {text}"
    );
    // Should not search workspace root when path is nested.
    assert!(
        !text.contains("root.py"),
        "search should be scoped to path alias root, not workspace root: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_files_rejects_empty_path_alias() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "search_files",
        serde_json::json!({"pattern": "TODO", "path": "   "}),
    )
    .await;
    assert!(
        is_error,
        "whitespace-only path alias must be rejected: {text}"
    );
    assert!(
        text.contains("empty") || text.contains("whitespace") || text.contains("path"),
        "error should mention empty path: {text}"
    );
    client.cancel().await.unwrap();
}

/// Regression test for #939 / #1270: search_files with zero matches must
/// return a descriptive "No matches found." message as a success (isError:
/// false), not an error. Empty results are valid answers, not failures.
#[tokio::test]
async fn test_mcp_search_no_match_returns_descriptive_message() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "nothing relevant here\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "search_files",
        serde_json::json!({"pattern": "zzz_nonexistent_pattern_zzz"}),
    )
    .await;
    assert!(
        !is_error,
        "search with no matches should return isError: false (#1270)"
    );
    assert!(
        text.contains("No matches found"),
        "response should say 'No matches found', got: {text}"
    );
    assert!(
        !text.contains("Operation failed with exit code"),
        "generic message should not appear: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_rejects_absolute_path() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(
                serde_json::json!({"pattern": "secret", "paths": ["/etc/passwd"]}),
            )
            .unwrap(),
        );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "search with absolute path should be rejected as a path containment violation"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_rejects_conflicting_modes() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(serde_json::json!({
                "pattern": "hello",
                "files_with_matches": true,
                "count": true
            }))
            .unwrap(),
        );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "search with both files_with_matches and count should be rejected"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_rejects_empty_pattern() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(serde_json::from_value(serde_json::json!({"pattern": ""})).unwrap());
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "search with empty pattern should be rejected"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_file_rename_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old_name.txt"), "content\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "move_file",
        serde_json::json!({"from": "old_name.txt", "to": "new_name.txt"}),
    )
    .await;
    assert!(!is_error, "rename should succeed: {val}");
    assert_eq!(val["ok"], true, "rename ok field: {val}");
    assert_eq!(val["files_created"], 1, "rename files_created: {val}");
    assert_eq!(val["files_deleted"], 1, "rename files_deleted: {val}");
    assert!(
        !dir.path().join("old_name.txt").exists(),
        "old file should not exist"
    );
    assert!(
        dir.path().join("new_name.txt").exists(),
        "new file should exist"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new_name.txt")).unwrap(),
        "content\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_create_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "create_file",
        serde_json::json!({"path": "new_file.txt", "content": "hello world\n"}),
    )
    .await;
    assert!(!is_error, "create should succeed: {val}");
    assert_eq!(val["ok"], true, "create ok field: {val}");
    assert_eq!(val["files_created"], 1, "create files_created: {val}");
    assert!(
        dir.path().join("new_file.txt").exists(),
        "file should exist after create"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new_file.txt")).unwrap(),
        "hello world\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_create_existing_fails_without_force() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("existing.txt"), "original\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "create_file",
        serde_json::json!({"path": "existing.txt", "content": "new content\n"}),
    )
    .await;
    assert!(
        is_error,
        "create should fail for existing file without force: {val}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("existing.txt")).unwrap(),
        "original\n",
        "original content should be preserved"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_create_force_overwrites_existing() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("existing.txt"), "original\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "create_file",
        serde_json::json!({"path": "existing.txt", "content": "replaced\n", "force": true}),
    )
    .await;
    assert!(!is_error, "create with force should succeed: {val}");
    assert_eq!(val["ok"], true, "create force ok field: {val}");
    assert_eq!(
        fs::read_to_string(dir.path().join("existing.txt")).unwrap(),
        "replaced\n",
        "content should be overwritten"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_delete_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doomed.txt"), "bye\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "delete_file",
        serde_json::json!({"path": "doomed.txt"}),
    )
    .await;
    assert!(!is_error, "delete should succeed: {val}");
    assert_eq!(val["ok"], true, "delete ok field: {val}");
    assert_eq!(val["files_deleted"], 1, "delete files_deleted: {val}");
    assert!(
        !dir.path().join("doomed.txt").exists(),
        "file should not exist after delete"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_delete_nonexistent_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "delete_file",
        serde_json::json!({"path": "nonexistent.txt"}),
    )
    .await;
    assert!(
        is_error,
        "delete should fail when file does not exist: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_move_nonexistent_source_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "move_file",
        serde_json::json!({"from": "nonexistent.txt", "to": "dest.txt"}),
    )
    .await;
    assert!(
        is_error,
        "move should fail when source does not exist: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_patch_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old line\n").unwrap();

    let diff = "--- a/target.txt\n+++ b/target.txt\n@@ -1 +1 @@\n-old line\n+new line\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) =
        call_tool_value(&client, "apply_patch", serde_json::json!({"diff": diff})).await;
    assert!(!is_error, "patch should succeed: {val}");
    assert_eq!(val["ok"], true, "patch ok field: {val}");
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "new line\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_apply_patch_on_stale_merge() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("target.txt"),
        "line1\nold line\nline3\nextra\n",
    )
    .unwrap();

    let diff = "--- a/target.txt\n+++ b/target.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "apply_patch",
        serde_json::json!({"diff": diff, "on_stale": "merge"}),
    )
    .await;
    assert!(!is_error, "patch merge should succeed: {val}");
    assert_eq!(val["ok"], true, "patch merge ok field: {val}");
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_apply_patch_merge_conflict_rejected_without_allow_conflicts() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("target.txt"),
        "line1\ncompletely different\nline3\n",
    )
    .unwrap();

    let diff = "--- a/target.txt\n+++ b/target.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "apply_patch",
        serde_json::json!({"diff": diff, "on_stale": "merge"}),
    )
    .await;
    assert!(
        is_error,
        "conflicting merge without allow_conflicts should fail: {val}"
    );
    assert_eq!(val["ok"], false, "patch conflict ok should be false: {val}");
    let response_str = val.to_string();
    assert!(
        response_str.contains("conflict"),
        "response should mention conflict: {val}"
    );
    assert!(
        !response_str.contains("<<<<<<<"),
        "conflict markers must not appear in response: {val}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "line1\ncompletely different\nline3\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_apply_patch_merge_conflict_with_allow_conflicts() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("target.txt"),
        "line1\ncompletely different\nline3\n",
    )
    .unwrap();

    let diff = "--- a/target.txt\n+++ b/target.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "apply_patch",
        serde_json::json!({"diff": diff, "on_stale": "merge", "allow_conflicts": true}),
    )
    .await;
    assert!(
        !is_error,
        "patch merge with allow_conflicts should succeed: {val}"
    );
    assert_eq!(val["ok"], true, "patch allow_conflicts ok field: {val}");
    let content = fs::read_to_string(dir.path().join("target.txt")).unwrap();
    assert!(
        content.contains("<<<<<<< patchloom (ours)"),
        "should have ours marker: {content}"
    );
    assert!(
        content.contains("======="),
        "should have separator: {content}"
    );
    assert!(
        content.contains(">>>>>>> patch (theirs)"),
        "should have theirs marker: {content}"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_md_insert_after_heading_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doc.md"), "# Title\n\nExisting body.\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_insert_after_heading",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Title",
            "content": "Inserted line.\n"
        }),
    )
    .await;
    assert!(!is_error, "md insert_after_heading should succeed: {val}");
    assert_eq!(val["ok"], true, "md insert_after_heading ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md insert_after_heading files_changed: {val}"
    );
    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("Inserted line."),
        "inserted content should be present: {content}"
    );
    // Existing body should still be there.
    assert!(
        content.contains("Existing body."),
        "existing body should be preserved: {content}"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_md_insert_before_heading_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# First\n\nBody one.\n\n## Second\n\nBody two.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_insert_before_heading",
        serde_json::json!({
            "path": "doc.md",
            "heading": "## Second",
            "content": "Preface text.\n"
        }),
    )
    .await;
    assert!(!is_error, "md insert_before_heading should succeed: {val}");
    assert_eq!(val["ok"], true, "md insert_before_heading ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md insert_before_heading files_changed: {val}"
    );
    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("Preface text."),
        "inserted content should be present: {content}"
    );
    // The preface should appear before ## Second.
    let preface_pos = content.find("Preface text.").unwrap();
    let heading_pos = content.find("## Second").unwrap();
    assert!(
        preface_pos < heading_pos,
        "preface should appear before ## Second"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_tidy_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // File missing final newline and with trailing whitespace.
    fs::write(dir.path().join("messy.txt"), "hello   \nworld").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "fix_whitespace",
        serde_json::json!({"path": "messy.txt"}),
    )
    .await;
    assert!(!is_error, "tidy should succeed: {val}");
    assert_eq!(val["ok"], true, "tidy ok: {val}");
    assert_eq!(val["files_changed"], 1, "tidy files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("messy.txt")).unwrap();
    assert!(content.ends_with('\n'), "tidy should ensure final newline");
    assert!(
        !content.contains("   \n"),
        "tidy should trim trailing whitespace"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_upsert_bullet_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doc.md"), "# Rules\n\n- Existing rule\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_upsert_bullet",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Rules",
            "bullet": "- New rule"
        }),
    )
    .await;
    assert!(!is_error, "md_upsert_bullet should succeed: {val}");
    assert_eq!(val["ok"], true, "md_upsert_bullet ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md_upsert_bullet files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("- New rule"),
        "md_upsert_bullet should add the bullet: {content}"
    );
    assert!(
        content.contains("- Existing rule"),
        "md_upsert_bullet should preserve existing bullets: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_merge_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_merge",
        serde_json::json!({"path": "config.json", "value": {"debug": true}}),
    )
    .await;
    assert!(!is_error, "doc_merge should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_merge ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_merge files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["debug"], true, "doc_merge should add new key: {content}");
    assert_eq!(
        v["name"], "app",
        "doc_merge should preserve existing keys: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_delete_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","debug":true}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_delete",
        serde_json::json!({"path": "config.json", "selector": "debug"}),
    )
    .await;
    assert!(!is_error, "doc_delete should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_delete ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_delete files_changed: {val}");
    assert_eq!(val["changed"], true, "doc_delete changed: {val}");
    assert_eq!(val["removed"], 1, "doc_delete removed: {val}");
    assert_eq!(
        val["mutations"][0]["op"], "doc.delete",
        "mutations[0].op: {val}"
    );
    assert_eq!(
        val["mutations"][0]["removed"], 1,
        "mutations[0].removed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("debug").is_none(), "doc_delete should remove key");
    assert_eq!(v["name"], "app", "doc_delete should preserve other keys");
    client.cancel().await.unwrap();
}

/// #1439: MCP doc_delete missing key is ok with removed:0 / changed:false.
#[tokio::test]
async fn test_mcp_doc_delete_missing_key_reports_removed_zero() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"name":"app"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_delete",
        serde_json::json!({"path": "config.json", "selector": "missing"}),
    )
    .await;
    assert!(!is_error, "idempotent delete should not be an error: {val}");
    assert_eq!(val["ok"], true, "payload: {val}");
    assert_eq!(val["files_changed"], 0, "payload: {val}");
    assert_eq!(val["changed"], false, "payload: {val}");
    assert_eq!(val["removed"], 0, "payload: {val}");
    assert_eq!(val["mutations"][0]["path"], "config.json", "payload: {val}");
    assert_eq!(val["mutations"][0]["op"], "doc.delete", "payload: {val}");
    assert_eq!(val["mutations"][0]["changed"], false, "payload: {val}");
    assert_eq!(val["mutations"][0]["removed"], 0, "payload: {val}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_append_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_append",
        serde_json::json!({"path": "config.json", "selector": "tags", "value": "c"}),
    )
    .await;
    assert!(!is_error, "doc_append should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_append ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_append files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
    assert_eq!(v["tags"][2], "c");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_prepend_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_prepend",
        serde_json::json!({"path": "config.json", "selector": "tags", "value": "z"}),
    )
    .await;
    assert!(!is_error, "doc_prepend should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_prepend ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_prepend files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"][0], "z", "doc_prepend should insert at front");
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_ensure_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"name":"app"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Ensure a missing key.
    let (is_error, val) = call_tool_value(
        &client,
        "doc_ensure",
        serde_json::json!({"path": "config.json", "selector": "debug", "value": false}),
    )
    .await;
    assert!(!is_error, "doc_ensure should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_ensure ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_ensure files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["debug"], false, "doc_ensure should set missing key");
    assert_eq!(v["name"], "app", "doc_ensure should preserve existing keys");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_update_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"items":[{"active":false},{"active":false}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_update",
        serde_json::json!({"path": "config.json", "selector": "items[*]", "value": {"active": true}}),
    )
    .await;
    assert!(!is_error, "doc_update should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_update ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_update files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["items"][0]["active"], true);
    assert_eq!(v["items"][1]["active"], true);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_move_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"old_name":"value"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_move",
        serde_json::json!({"path": "config.json", "from": "old_name", "to": "new_name"}),
    )
    .await;
    assert!(!is_error, "doc_move should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_move ok: {val}");
    assert_eq!(val["files_changed"], 1, "doc_move files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("old_name").is_none(), "doc_move should remove source");
    assert_eq!(v["new_name"], "value", "doc_move should set destination");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_delete_where_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"items":[{"name":"keep"},{"name":"drop"},{"name":"keep2"}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_delete_where",
        serde_json::json!({"path": "config.json", "selector": "items", "predicate": "name=drop"}),
    )
    .await;
    assert!(!is_error, "doc_delete_where should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_delete_where ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "doc_delete_where files_changed: {val}"
    );
    assert_eq!(val["changed"], true, "doc_delete_where changed: {val}");
    assert_eq!(val["removed"], 1, "doc_delete_where removed: {val}");
    assert_eq!(
        val["mutations"][0]["op"], "doc.delete_where",
        "mutations op: {val}"
    );

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        2,
        "doc_delete_where should remove matching item"
    );
    assert_eq!(items[0]["name"], "keep");
    assert_eq!(items[1]["name"], "keep2");
    client.cancel().await.unwrap();
}

/// #1439: MCP delete-where zero matches is ok with removed:0.
#[tokio::test]
async fn test_mcp_doc_delete_where_zero_match_reports_removed_zero() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"items":[{"name":"keep"},{"name":"also"}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_delete_where",
        serde_json::json!({
            "path": "config.json",
            "selector": "items",
            "predicate": "name=nobody"
        }),
    )
    .await;
    assert!(!is_error, "idempotent delete-where should succeed: {val}");
    assert_eq!(val["ok"], true, "payload: {val}");
    assert_eq!(val["files_changed"], 0, "payload: {val}");
    assert_eq!(val["changed"], false, "payload: {val}");
    assert_eq!(val["removed"], 0, "payload: {val}");
    assert_eq!(val["mutations"].as_array().unwrap().len(), 1);
    assert_eq!(val["mutations"][0]["removed"], 0, "payload: {val}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(
        content.contains("keep") && content.contains("also"),
        "file unchanged: {content}"
    );
    client.cancel().await.unwrap();
}

/// #1439: multi-op plan surfaces per-op mutation stats and aggregates.
#[tokio::test]
async fn test_mcp_tx_plan_doc_delete_mutations_summary() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.json"),
        r#"{"items":[{"name":"a"},{"name":"b"},{"name":"a"}]}"#,
    )
    .unwrap();
    fs::write(dir.path().join("b.json"), r#"{"x":1}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "doc.delete_where",
                "path": "a.json",
                "selector": "items",
                "predicate": "name=a"
            },
            {
                "op": "doc.delete",
                "path": "b.json",
                "selector": "missing"
            }
        ]
    });
    let (is_error, val) =
        call_tool_value(&client, "execute_plan", serde_json::json!({"plan": plan})).await;
    assert!(!is_error, "plan should succeed: {val}");
    assert_eq!(val["ok"], true, "payload: {val}");
    // One file modified (a.json), b.json is no-op.
    assert_eq!(val["files_changed"], 1, "payload: {val}");
    assert_eq!(val["changed"], true, "any mutation changed: {val}");
    assert_eq!(val["removed"], 2, "2 from a + 0 from b: {val}");
    let mutations = val["mutations"].as_array().expect("mutations array");
    assert_eq!(mutations.len(), 2, "payload: {val}");
    assert_eq!(mutations[0]["path"], "a.json", "payload: {val}");
    assert_eq!(mutations[0]["op"], "doc.delete_where");
    assert_eq!(mutations[0]["removed"], 2);
    assert_eq!(mutations[0]["changed"], true);
    assert_eq!(mutations[1]["path"], "b.json", "payload: {val}");
    assert_eq!(mutations[1]["op"], "doc.delete");
    assert_eq!(mutations[1]["removed"], 0);
    assert_eq!(mutations[1]["changed"], false);

    let a: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("a.json")).unwrap()).unwrap();
    assert_eq!(a["items"].as_array().unwrap().len(), 1);
    assert_eq!(a["items"][0]["name"], "b");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_keys_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","version":"1.0","debug":true}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "keys", "path": "config.json", "selector": "."}),
    )
    .await;
    assert!(!is_error, "doc_query keys should succeed: {val}");
    let keys = val
        .as_array()
        .expect("doc_query keys should return an array");
    assert!(
        keys.contains(&serde_json::json!("name")),
        "keys should contain 'name': {val}"
    );
    assert!(
        keys.contains(&serde_json::json!("version")),
        "keys should contain 'version': {val}"
    );
    assert!(
        keys.contains(&serde_json::json!("debug")),
        "keys should contain 'debug': {val}"
    );
    assert_eq!(keys.len(), 3, "should have exactly 3 keys: {val}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_len_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b","c"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "len", "path": "config.json", "selector": "tags"}),
    )
    .await;
    assert!(!is_error, "doc_query len should succeed: {val}");
    assert_eq!(val, 3, "doc_query len should return exactly 3: {val}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_select_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"users":[{"role":"admin","name":"alice"},{"role":"user","name":"bob"}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "select", "path": "config.json", "selector": "users[role=admin]"}),
    )
    .await;
    assert!(!is_error, "doc_query select should succeed: {val}");
    assert_eq!(
        val["role"], "admin",
        "selected item should have role=admin: {val}"
    );
    assert_eq!(val["name"], "alice", "selected item should be alice: {val}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_flatten_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"db":{"host":"localhost","port":5432}}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_query",
        serde_json::json!({"action": "flatten", "path": "config.json"}),
    )
    .await;
    assert!(!is_error, "doc_query flatten should succeed: {val}");
    let obj = val
        .as_object()
        .expect("flatten should return a JSON object");
    assert_eq!(
        obj.get("db.host").and_then(|v| v.as_str()),
        Some("localhost"),
        "flatten should map db.host to 'localhost': {val}"
    );
    assert_eq!(
        obj.get("db.port").and_then(|v| v.as_i64()),
        Some(5432),
        "flatten should map db.port to 5432: {val}"
    );
    assert_eq!(obj.len(), 2, "flatten should have exactly 2 entries: {val}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_diff_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.json"),
        r#"{"name":"old","version":"1.0"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("b.json"),
        r#"{"name":"new","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "doc_diff",
        serde_json::json!({"file_a": "a.json", "file_b": "b.json"}),
    )
    .await;
    assert!(!is_error, "doc_diff should succeed: {val}");
    assert_eq!(val["identical"], false, "should not be identical: {val}");
    let diffs = val["differences"]
        .as_array()
        .expect("doc_diff should have a differences array");
    assert_eq!(diffs.len(), 1, "should have exactly one difference: {val}");
    assert_eq!(
        diffs[0]["path"], "name",
        "diff should be on 'name' field: {val}"
    );
    assert_eq!(
        diffs[0]["kind"], "changed",
        "diff kind should be 'changed': {val}"
    );
    assert_eq!(
        diffs[0]["old_value"], "old",
        "old_value should be 'old': {val}"
    );
    assert_eq!(
        diffs[0]["new_value"], "new",
        "new_value should be 'new': {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_status_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Initialize a git repo so status has something to report.
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("tracked.txt"), "hello\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Modify the tracked file to create a diff.
    fs::write(dir.path().join("tracked.txt"), "modified\n").unwrap();

    // New untracked file -> created category in status.
    fs::write(dir.path().join("created.txt"), "new content\n").unwrap();

    // Tracked then deleted -> deleted category.
    fs::write(dir.path().join("to-be-deleted.txt"), "will be gone\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "to-be-deleted.txt"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add for delete"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::remove_file(dir.path().join("to-be-deleted.txt")).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, status) = call_tool_value(&client, "git_status", serde_json::json!({})).await;
    assert!(!is_error, "status should succeed: {status}");
    let modified: Vec<String> = status
        .get("modified")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let created: Vec<String> = status
        .get("created")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let deleted: Vec<String> = status
        .get("deleted")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        modified.iter().any(|f| f.contains("tracked.txt")),
        "status should show modified file: {status}"
    );
    assert!(
        created.iter().any(|f| f.contains("created.txt")),
        "status should show created file: {status}"
    );
    assert!(
        deleted.iter().any(|f| f.contains("to-be-deleted.txt")),
        "status should show deleted file: {status}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_git_status_errors_without_git_repo() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap(); // no git init - expect error from collect_status

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, err_text) = call_tool_text(&client, "git_status", serde_json::json!({})).await;
    assert!(
        is_error,
        "git_status should error outside git repo: {err_text}"
    );
    assert!(
        err_text.contains("git") || err_text.contains("repository") || err_text.contains("failed"),
        "error should mention git/repo: {err_text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_table_append_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Changelog\n\n| Version | Date |\n|---------|------|\n| 0.1.0 | 2024-01-01 |\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_table_append",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Changelog",
            "row": "| 0.2.0 | 2024-06-15 |"
        }),
    )
    .await;
    assert!(!is_error, "md_table_append should succeed: {val}");
    assert_eq!(val["ok"], true, "md_table_append ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md_table_append files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("| 0.2.0 | 2024-06-15 |"),
        "md_table_append should add the row: {content}"
    );
    assert!(
        content.contains("| 0.1.0 | 2024-01-01 |"),
        "md_table_append should preserve existing rows: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_replace_section_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\nIntro text.\n\n## API\n\nOld API docs.\n\n## Usage\n\nUsage text.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_replace_section",
        serde_json::json!({
            "path": "doc.md",
            "heading": "## API",
            "content": "New API documentation.\n"
        }),
    )
    .await;
    assert!(!is_error, "md_replace_section should succeed: {val}");
    assert_eq!(val["ok"], true, "md_replace_section ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md_replace_section files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("New API documentation."),
        "md_replace_section should insert new content: {content}"
    );
    assert!(
        !content.contains("Old API docs."),
        "md_replace_section should remove old content: {content}"
    );
    assert!(
        content.contains("## Usage"),
        "md_replace_section should preserve other sections: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_lint_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Create a file with a duplicate heading (a lint issue).
    fs::write(
        dir.path().join("AGENTS.md"),
        "# Rules\n\nFirst section.\n\n# Rules\n\nDuplicate heading.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_lint",
        serde_json::json!({
            "path": "AGENTS.md"
        }),
    )
    .await;
    assert!(!is_error, "md_lint should succeed: {val}");
    let arr = val.as_array().expect("md_lint should return a JSON array");
    assert!(
        !arr.is_empty(),
        "md_lint should find issues in file with duplicate heading"
    );

    // Also test a clean file returns an empty array.
    fs::write(
        dir.path().join("clean.md"),
        "# Single Heading\n\nContent.\n",
    )
    .unwrap();
    let (is_error2, val2) = call_tool_value(
        &client,
        "md_lint",
        serde_json::json!({
            "path": "clean.md"
        }),
    )
    .await;
    assert!(!is_error2, "md_lint should succeed on clean file: {val2}");
    assert_eq!(
        val2.as_array().unwrap().len(),
        0,
        "md_lint should return empty array for clean file"
    );
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Homogeneous batch MCP integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mcp_batch_replace_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "version = 1.0.0\n").unwrap();
    fs::write(dir.path().join("b.txt"), "version = 1.0.0\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_replace",
        serde_json::json!({
            "files": ["a.txt", "b.txt"],
            "old": "1.0.0",
            "new": "2.0.0"
        }),
    )
    .await;
    assert!(!is_error, "batch_replace should succeed: {val}");
    assert_eq!(val["ok"], true, "batch_replace ok: {val}");
    assert_eq!(
        val["files_changed"], 2,
        "batch_replace files_changed: {val}"
    );
    assert_eq!(
        val["match_mode"], "exact",
        "batch_replace exact match honesty (#1674): {val}"
    );
    for ch in val["changes"].as_array().expect("changes array") {
        assert_eq!(ch["match_mode"], "exact", "per-file match_mode exact: {ch}");
    }

    let a = fs::read_to_string(dir.path().join("a.txt")).unwrap();
    let b = fs::read_to_string(dir.path().join("b.txt")).unwrap();
    assert!(a.contains("2.0.0"), "a.txt should be updated: {a}");
    assert!(b.contains("2.0.0"), "b.txt should be updated: {b}");
    assert!(
        !a.contains("1.0.0"),
        "a.txt should not have old version: {a}"
    );
    assert!(
        !b.contains("1.0.0"),
        "b.txt should not have old version: {b}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_fuzzy_reports_match_mode() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "fn process_data() {}\n").unwrap();
    fs::write(dir.path().join("b.rs"), "fn process_data() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_replace",
        serde_json::json!({
            "files": ["a.rs", "b.rs"],
            "old": "fn proccess_data() {}",
            "new": "fn handle_data() {}",
            "fuzzy": true,
            "require_change": true
        }),
    )
    .await;
    assert!(!is_error, "batch_replace fuzzy should succeed: {val}");
    assert_eq!(val["ok"], true, "batch_replace fuzzy ok: {val}");
    assert_eq!(
        val["match_mode"], "fuzzy",
        "aggregate match_mode fuzzy (#1674): {val}"
    );
    let changes = val["changes"].as_array().expect("changes");
    assert_eq!(changes.len(), 2, "two files changed: {val}");
    for ch in changes {
        assert_eq!(ch["match_mode"], "fuzzy", "per-file fuzzy match_mode: {ch}");
    }
    assert!(
        fs::read_to_string(dir.path().join("a.rs"))
            .unwrap()
            .contains("handle_data"),
        "a.rs rewritten"
    );
    assert!(
        fs::read_to_string(dir.path().join("b.rs"))
            .unwrap()
            .contains("handle_data"),
        "b.rs rewritten"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_command_position() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.sh"),
        "sudo pip install x\nuv pip install\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("b.sh"),
        "timeout 30 pip install y\necho pip\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_replace",
        serde_json::json!({
            "files": ["a.sh", "b.sh"],
            "old": "pip",
            "new": "uv",
            "command_position": true,
            "require_change": true
        }),
    )
    .await;
    assert!(
        !is_error,
        "batch_replace command_position should succeed: {val}"
    );
    assert_eq!(val["ok"], true, "batch_replace command_position ok: {val}");

    assert_eq!(
        fs::read_to_string(dir.path().join("a.sh")).unwrap(),
        "sudo uv install x\nuv pip install\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.sh")).unwrap(),
        "timeout 30 uv install y\necho pip\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_accepts_file_alias() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("solo.txt"), "version = 1.0.0\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_replace",
        serde_json::json!({
            "file": "solo.txt",
            "old": "1.0.0",
            "new": "3.0.0"
        }),
    )
    .await;
    assert!(!is_error, "singular file alias should work: {val}");
    let content = fs::read_to_string(dir.path().join("solo.txt")).unwrap();
    assert!(
        content.contains("3.0.0"),
        "file alias replace should apply: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_regex_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("x.txt"), "foo123bar\n").unwrap();
    fs::write(dir.path().join("y.txt"), "foo456bar\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_replace",
        serde_json::json!({
            "files": ["x.txt", "y.txt"],
            "old": "foo\\d+bar",
            "new": "replaced",
            "regex": true
        }),
    )
    .await;
    assert!(!is_error, "batch_replace regex should succeed: {val}");
    assert_eq!(val["ok"], true, "batch_replace regex ok: {val}");
    assert_eq!(
        val["files_changed"], 2,
        "batch_replace regex files_changed: {val}"
    );

    let x = fs::read_to_string(dir.path().join("x.txt")).unwrap();
    let y = fs::read_to_string(dir.path().join("y.txt")).unwrap();
    assert!(x.contains("replaced"), "x.txt: {x}");
    assert!(y.contains("replaced"), "y.txt: {y}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_empty_files_rejected() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("batch_replace").with_arguments(
        serde_json::from_value(serde_json::json!({
            "files": [],
            "old": "a",
            "new": "b"
        }))
        .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(result.is_err(), "empty files array should be rejected");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_tidy_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Files with trailing whitespace and missing final newline.
    fs::write(dir.path().join("c.txt"), "hello   \nworld").unwrap();
    fs::write(dir.path().join("d.txt"), "foo  \nbar  ").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "batch_tidy",
        serde_json::json!({
            "files": ["c.txt", "d.txt"]
        }),
    )
    .await;
    assert!(!is_error, "batch_tidy should succeed: {val}");
    assert_eq!(val["ok"], true, "batch_tidy ok: {val}");
    assert_eq!(val["files_changed"], 2, "batch_tidy files_changed: {val}");

    let c = fs::read_to_string(dir.path().join("c.txt")).unwrap();
    let d = fs::read_to_string(dir.path().join("d.txt")).unwrap();
    assert!(c.ends_with('\n'), "c.txt should have final newline");
    assert!(d.ends_with('\n'), "d.txt should have final newline");
    assert!(
        !c.contains("   \n"),
        "c.txt trailing whitespace should be trimmed"
    );
    assert!(
        !d.contains("  \n"),
        "d.txt trailing whitespace should be trimmed"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_replace_path_traversal_rejected() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.txt"), "content\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("batch_replace").with_arguments(
        serde_json::from_value(serde_json::json!({
            "files": ["ok.txt", "../../etc/passwd"],
            "old": "a",
            "new": "b"
        }))
        .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "batch_replace with path traversal should be rejected"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_rejects_unknown_fields() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.json"), r#"{"a":1}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Send a valid tool call but with an extra hallucinated parameter.
    // deny_unknown_fields on the params struct must reject this.
    let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
        serde_json::from_value(serde_json::json!({
            "path": "test.json",
            "selector": "a",
            "value": 2,
            "hallucinated_param": true
        }))
        .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    // rmcp 1.8+ returns deny_unknown_fields violations as a tool result with
    // isError (not a protocol-level Err), so accept either shape.
    match result {
        Err(_) => { /* protocol-level rejection: OK */ }
        Ok(r) => {
            assert!(
                r.is_error.unwrap_or(false),
                "unknown fields must be rejected by deny_unknown_fields"
            );
        }
    }
    // File must remain unchanged
    let content = fs::read_to_string(dir.path().join("test.json")).unwrap();
    assert_eq!(content, r#"{"a":1}"#);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_regex_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("ver.txt"),
        "version = 1.2.3\nversion = 4.5.6\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "ver.txt",
            "old": r"version = (\d+)\.(\d+)\.(\d+)",
            "new": "version = $1.$2.99",
            "regex": true
        }),
    )
    .await;
    assert!(!is_error, "regex replace should succeed: {val}");
    assert_eq!(val["ok"], true, "regex replace ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "regex replace files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("ver.txt")).unwrap();
    assert_eq!(content, "version = 1.2.99\nversion = 4.5.99\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_nth_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("rep.txt"), "foo bar foo baz foo\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "rep.txt",
            "old": "foo",
            "new": "qux",
            "nth": 2
        }),
    )
    .await;
    assert!(!is_error, "nth replace should succeed: {val}");
    assert_eq!(val["ok"], true, "nth replace ok: {val}");
    assert_eq!(val["files_changed"], 1, "nth replace files_changed: {val}");

    let content = fs::read_to_string(dir.path().join("rep.txt")).unwrap();
    assert_eq!(content, "foo bar qux baz foo\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_if_exists_no_match_succeeds() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("stable.txt"), "no match here\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "stable.txt",
            "old": "nonexistent",
            "new": "replacement",
            "if_exists": true
        }),
    )
    .await;
    assert!(
        !is_error,
        "if_exists with no match should succeed (not error): {val}"
    );
    assert_eq!(val["ok"], true, "if_exists ok: {val}");
    assert_eq!(
        val["files_changed"], 0,
        "if_exists no match files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("stable.txt")).unwrap();
    assert_eq!(content, "no match here\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_command_position_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("install.sh"),
        "sudo pip install x\nuv pip install\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "install.sh",
            "old": "pip",
            "new": "uv",
            "command_position": true,
            "require_change": true
        }),
    )
    .await;
    assert!(!is_error, "command_position replace should succeed: {val}");
    assert_eq!(val["ok"], true, "command_position ok: {val}");

    let content = fs::read_to_string(dir.path().join("install.sh")).unwrap();
    assert_eq!(content, "sudo uv install x\nuv pip install\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_command_position_rejects_regex() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("install.sh"), "sudo pip install\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "install.sh",
            "old": "pip",
            "new": "uv",
            "command_position": true,
            "regex": true
        }),
    )
    .await;
    assert!(is_error, "command_position+regex must fail: {val}");
    let err = val.to_string();
    assert!(
        err.contains("command_position cannot be combined"),
        "error should name the command_position combo conflict: {val}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("install.sh")).unwrap(),
        "sudo pip install\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_whole_line_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("code.rs"),
        "fn main() {\n    dbg!(x);\n    let y = 1;\n    dbg!(y);\n}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "code.rs",
            "old": "dbg!",
            "new": "",
            "whole_line": true
        }),
    )
    .await;
    assert!(!is_error, "whole_line replace should succeed: {val}");
    assert_eq!(val["ok"], true, "whole_line replace ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "whole_line replace files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("code.rs")).unwrap();
    assert_eq!(content, "fn main() {\n    let y = 1;\n}\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_whole_line_with_range_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "data.txt",
            "old": "bbb",
            "new": "",
            "whole_line": true,
            "range": "1:3"
        }),
    )
    .await;
    assert!(!is_error, "whole_line+range replace should succeed: {val}");
    assert_eq!(val["ok"], true, "whole_line+range replace ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "whole_line+range files_changed: {val}"
    );

    let content = fs::read_to_string(dir.path().join("data.txt")).unwrap();
    // Only the first "bbb" (line 2, within range 1:3) should be deleted.
    assert_eq!(content, "aaa\nccc\nbbb\neee\n");
    client.cancel().await.unwrap();
}

// Regression: before_context and after_context were not validated for size
// in the replace_text MCP handler, while from/to/insert_before/insert_after were.
#[tokio::test]
async fn test_mcp_replace_whole_line_insert_before_rejected() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "prefix needle suffix\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "data.txt",
            "old": "needle",
            "insert_before": "BEFORE\n",
            "whole_line": true
        }),
    )
    .await;
    assert!(
        is_error,
        "whole_line + insert_before should be rejected: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_append_file_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "line one\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "append_file",
        serde_json::json!({"path": "data.txt", "content": "line two\n"}),
    )
    .await;
    assert!(!is_error, "append_file should succeed: {val}");
    assert_eq!(val["ok"], true, "append ok field: {val}");
    assert_eq!(val["files_changed"], 1, "append files_changed: {val}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line one\nline two\n",
        "content should be appended"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_append_file_missing_file_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "append_file",
        serde_json::json!({"path": "nonexistent.txt", "content": "data\n"}),
    )
    .await;
    assert!(
        is_error,
        "append_file should fail when file does not exist: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_move_section_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Top\n\nIntro\n\n## Alpha\n\nA content\n\n## Beta\n\nB content\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_move_section",
        serde_json::json!({
            "path": "doc.md",
            "heading": "## Beta",
            "before": "## Alpha"
        }),
    )
    .await;
    assert!(!is_error, "md_move_section should succeed: {val}");
    assert_eq!(val["ok"], true, "md_move_section ok: {val}");
    assert_eq!(
        val["files_changed"], 1,
        "md_move_section files_changed: {val}"
    );

    let content = fs::read_to_string(&file).unwrap();
    let alpha_pos = content.find("## Alpha").expect("Alpha should exist");
    let beta_pos = content.find("## Beta").expect("Beta should exist");
    assert!(
        beta_pos < alpha_pos,
        "Beta should come before Alpha after move"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_query_unknown_action_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"x":1}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("doc_query").with_arguments(
        serde_json::from_value(serde_json::json!({
            "path": "data.json",
            "action": "nonexistent"
        }))
        .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "unknown action should return McpError::invalid_params"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_query_has_without_selector_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"x":1}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("doc_query").with_arguments(
        serde_json::from_value(serde_json::json!({
            "path": "data.json",
            "action": "has"
        }))
        .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "'has' without selector should return McpError::invalid_params"
    );
    client.cancel().await.unwrap();
}

/// Regression test for #895: doc_set via MCP must create intermediate YAML
/// keys without corrupting the document structure.
#[tokio::test]
async fn test_mcp_doc_set_yaml_deep_nested_path_creation() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.yaml"), "existing: value\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;

    // Set a deeply nested path that requires creating intermediate keys
    let (is_error, val) = call_tool_value(
        &client,
        "doc_set",
        serde_json::json!({
            "path": "config.yaml",
            "selector": "level1.level2.level3",
            "value": "deep_value"
        }),
    )
    .await;
    assert!(!is_error, "doc_set deep nested should succeed: {val}");
    assert_eq!(val["ok"], true, "doc_set ok field: {val}");

    // Verify the file is valid YAML and has the correct structure
    let content = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content)
        .unwrap_or_else(|e| panic!("YAML parse failed after doc_set: {e}\ncontent:\n{content}"));
    assert_eq!(
        parsed["existing"], "value",
        "doc_set clobbered existing key: {content}"
    );
    assert_eq!(
        parsed["level1"]["level2"]["level3"], "deep_value",
        "doc_set did not create nested path: {content}"
    );

    // Round-trip: read back the value via doc_get to verify MCP consistency
    let (is_error2, val2) = call_tool_value(
        &client,
        "doc_get",
        serde_json::json!({
            "path": "config.yaml",
            "selector": "level1.level2.level3"
        }),
    )
    .await;
    assert!(!is_error2, "doc_get should succeed: {val2}");
    assert_eq!(
        val2, "deep_value",
        "doc_get returned wrong value after doc_set: {val2}"
    );

    client.cancel().await.unwrap();
}

/// Regression test for #895: doc_set via MCP on YAML with multiple sequential
/// nested writes must preserve all previous writes.
#[tokio::test]
async fn test_mcp_doc_set_yaml_multiple_nested_writes() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("app.yaml"), "app:\n  name: test\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;

    // First write: set a nested config value
    let (err1, v1) = call_tool_value(
        &client,
        "doc_set",
        serde_json::json!({
            "path": "app.yaml",
            "selector": "app.database.host",
            "value": "localhost"
        }),
    )
    .await;
    assert!(!err1, "first doc_set should succeed: {v1}");

    // Second write: set another nested config value in the same subtree
    let (err2, v2) = call_tool_value(
        &client,
        "doc_set",
        serde_json::json!({
            "path": "app.yaml",
            "selector": "app.database.port",
            "value": 5432
        }),
    )
    .await;
    assert!(!err2, "second doc_set should succeed: {v2}");

    // Verify both writes and the original data are all present
    let content = fs::read_to_string(dir.path().join("app.yaml")).unwrap();
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content)
        .unwrap_or_else(|e| panic!("YAML parse failed: {e}\ncontent:\n{content}"));
    assert_eq!(
        parsed["app"]["name"], "test",
        "original key clobbered: {content}"
    );
    assert_eq!(
        parsed["app"]["database"]["host"], "localhost",
        "first nested write lost: {content}"
    );
    assert_eq!(
        parsed["app"]["database"]["port"], 5432,
        "second nested write lost: {content}"
    );

    client.cancel().await.unwrap();
}

/// Regression test for #892: sequential MCP replace_text calls to the same
/// file must each see the result of the previous call (MCP call ordering,
/// not handler internals).
#[tokio::test]
async fn test_mcp_replace_text_sequential_calls_see_prior_writes() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "line_a\nline_b\nline_c\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;

    // Write 1: replace line_a
    let (err1, v1) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "target.txt",
            "old": "line_a",
            "new": "REPLACED_A"
        }),
    )
    .await;
    assert!(!err1, "replace 1 should succeed: {v1}");

    // Write 2: replace line_b (must see write 1's result)
    let (err2, v2) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "target.txt",
            "old": "line_b",
            "new": "REPLACED_B"
        }),
    )
    .await;
    assert!(!err2, "replace 2 should succeed: {v2}");

    // Write 3: replace line_c (must see writes 1+2's result)
    let (err3, v3) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "target.txt",
            "old": "line_c",
            "new": "REPLACED_C"
        }),
    )
    .await;
    assert!(!err3, "replace 3 should succeed: {v3}");

    // All three writes must be present
    let content = fs::read_to_string(dir.path().join("target.txt")).unwrap();
    assert_eq!(
        content, "REPLACED_A\nREPLACED_B\nREPLACED_C\n",
        "sequential writes lost data: {content}"
    );

    client.cancel().await.unwrap();
}

// --- AST MCP tool tests ---

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_list_returns_symbols() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn greet() {}\nstruct Config;\nenum Color { Red }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) =
        call_tool_value(&client, "ast_list", serde_json::json!({"path": "lib.rs"})).await;
    assert!(!is_error, "ast_list should succeed: {val}");
    let symbols = val.as_array().expect("ast_list should return array");
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"greet"), "should find greet: {names:?}");
    assert!(names.contains(&"Config"), "should find Config: {names:?}");
    assert!(names.contains(&"Color"), "should find Color: {names:?}");
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_list_kind_filter() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "pub fn greet() {}\nstruct Config;\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_list",
        serde_json::json!({"path": "lib.rs", "kind": "function"}),
    )
    .await;
    assert!(!is_error, "ast_list with kind filter should succeed: {val}");
    let symbols = val.as_array().expect("ast_list should return array");
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert!(names.contains(&"greet"), "should find greet: {names:?}");
    assert!(
        !names.contains(&"Config"),
        "should NOT find Config with function filter: {names:?}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_read_returns_source() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "fn target() {\n    let x = 42;\n}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_read",
        serde_json::json!({"path": "main.rs", "symbol": "target"}),
    )
    .await;
    assert!(!is_error, "ast_read should succeed: {val}");
    assert_eq!(val["symbol"], "target", "symbol name: {val}");
    let content = val["content"].as_str().expect("content should be string");
    assert!(
        content.contains("let x = 42"),
        "should contain function body: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_read_not_found() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.rs"), "fn existing() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _val) = call_tool_value(
        &client,
        "ast_read",
        serde_json::json!({"path": "main.rs", "symbol": "nonexistent"}),
    )
    .await;
    assert!(is_error, "ast_read with missing symbol should fail");
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_validate_clean() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.rs"), "fn valid() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_validate",
        serde_json::json!({"path": "ok.rs"}),
    )
    .await;
    assert!(
        !is_error,
        "ast_validate should succeed on valid code: {val}"
    );
    // ast_validate returns an array of file results
    let results = val.as_array().expect("ast_validate should return array");
    assert!(
        results.iter().all(|r| r["valid"] == true),
        "valid code should pass validation: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_validate_syntax_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.rs"), "fn broken( {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_validate",
        serde_json::json!({"path": "bad.rs"}),
    )
    .await;
    // ast_validate returns success with errors in the response body
    assert!(!is_error, "ast_validate should not be a tool error: {val}");
    let results = val.as_array().expect("ast_validate should return array");
    assert!(
        results.iter().any(|r| r["valid"] == false),
        "broken code should fail validation: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_search_finds_matches() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn first() { let x = 1; }\nfn second() { let y = 2; }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Use tree-sitter S-expression query for function_item nodes
    let (is_error, val) = call_tool_value(
        &client,
        "ast_search",
        serde_json::json!({"path": "lib.rs", "query": "(function_item) @fn"}),
    )
    .await;
    assert!(!is_error, "ast_search should succeed: {val}");
    let matches = val.as_array().expect("ast_search should return array");
    assert!(
        matches.len() >= 2,
        "should find at least 2 function_item matches: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_rename_applies() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn old_name() {}\nfn caller() { old_name(); }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_rename",
        serde_json::json!({"path": "lib.rs", "old": "old_name", "new": "new_name"}),
    )
    .await;
    assert!(!is_error, "ast_rename should succeed: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        content.contains("new_name"),
        "file should contain renamed symbol: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "old name should be gone: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_rewrite_signature_applies() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn process(x: i32) {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_rewrite_signature",
        serde_json::json!({
            "path": "lib.rs",
            "old": "process",
            "parameters": "(x: u64)",
            "return_type": "-> u64"
        }),
    )
    .await;
    assert!(!is_error, "ast_rewrite_signature should succeed: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        content.contains("u64"),
        "signature should be rewritten: {content}"
    );
    assert!(
        content.contains("-> u64 {"),
        "MCP rewrite_signature must preserve body gap (#1503): {content}"
    );
    assert!(
        !content.contains("u64{"),
        "must not glue type to brace: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_rewrite_signature_missing_reports_detail() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn keep() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_rewrite_signature",
        serde_json::json!({
            "path": "lib.rs",
            "old": "missing_fn",
            "parameters": "(x: i32)"
        }),
    )
    .await;
    // Write no-match is an agent-visible error (not empty-query success).
    assert!(is_error, "missing function should be tool error: {val}");
    let text = val.to_string();
    assert!(
        text.contains("missing_fn") || text.contains("not found") || text.contains("no_matches"),
        "error should name the missing function or no_matches: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_refs_finds_references() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn helper() {}\nfn main() { helper(); helper(); }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_refs",
        serde_json::json!({"path": "lib.rs", "symbol": "helper"}),
    )
    .await;
    assert!(!is_error, "ast_refs should succeed: {val}");
    let binding = val.to_string();
    let text = val.as_str().unwrap_or(&binding);
    assert!(
        text.contains("helper"),
        "should contain references to helper: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_deps_extracts_imports() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("main.rs"),
        "use std::collections::HashMap;\nuse std::io;\nfn main() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) =
        call_tool_value(&client, "ast_deps", serde_json::json!({"path": "main.rs"})).await;
    assert!(!is_error, "ast_deps should succeed: {val}");
    let binding = val.to_string();
    let text = val.as_str().unwrap_or(&binding);
    assert!(
        text.contains("std::collections::HashMap") || text.contains("HashMap"),
        "should contain HashMap import: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_map_returns_ranked_symbols() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        "pub fn core_fn() {}\npub fn caller() { core_fn(); }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_map",
        serde_json::json!({"path": "src", "max_tokens": 1024}),
    )
    .await;
    assert!(!is_error, "ast_map should succeed: {val}");
    let binding = val.to_string();
    let text = val.as_str().unwrap_or(&binding);
    assert!(
        text.contains("core_fn") || text.contains("caller"),
        "map should contain symbol names: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_diff_detects_changes() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    // Set up a git repo with a committed file, then modify it
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("lib.rs"), "fn original() {}\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "lib.rs"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Now modify the file (add a new function)
    fs::write(
        dir.path().join("lib.rs"),
        "fn original() {}\nfn added() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_diff",
        serde_json::json!({"path": "lib.rs", "from": "HEAD"}),
    )
    .await;
    assert!(!is_error, "ast_diff should succeed: {val}");
    let binding = val.to_string();
    let text = val.as_str().unwrap_or(&binding);
    assert!(
        text.contains("added"),
        "diff should detect the new function: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_impact_traces_dependents() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        "pub fn base() {}\npub fn mid() { base(); }\npub fn top() { mid(); }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_impact",
        serde_json::json!({"path": "src", "symbol": "base", "depth": 2}),
    )
    .await;
    assert!(!is_error, "ast_impact should succeed: {val}");
    let binding = val.to_string();
    let text = val.as_str().unwrap_or(&binding);
    assert!(
        text.contains("mid"),
        "impact should include mid as a dependent of base: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_ast_replace_within_symbol() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn target() { let val = 42; }\nfn other() { let val = 99; }\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_replace",
        serde_json::json!({
            "path": "lib.rs",
            "symbol": "target",
            "old": "42",
            "new": "100"
        }),
    )
    .await;
    assert!(!is_error, "ast_replace should succeed: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        content.contains("let val = 100"),
        "target function should have 'let val = 100': {content}"
    );
    assert!(
        content.contains("let val = 99"),
        "other function should still have 'let val = 99': {content}"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_list_unsupported_file_names_language() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.csv"), "a,b,c\n1,2,3\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) =
        call_tool_text(&client, "ast_list", serde_json::json!({"path": "data.csv"})).await;
    assert!(is_error, "ast_list on unsupported file should return error");
    assert!(
        text.contains("Unsupported language"),
        "response should contain 'Unsupported language': {text}"
    );
    assert!(
        text.contains("Rust"),
        "response should list supported languages: {text}"
    );
    client.cancel().await.unwrap();
}

/// Regression test for #939 / #1270: NO_MATCHES results must return the
/// descriptive fallback message as a success (isError: false), not an error.
/// Empty results are valid answers that should not trigger agent recovery mode.
#[tokio::test]
#[cfg(feature = "ast")]
async fn test_mcp_no_matches_returns_descriptive_message() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn hello() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Search for a symbol that does not exist
    let (is_error, text) = call_tool_text(
        &client,
        "ast_refs",
        serde_json::json!({"path": "lib.rs", "symbol": "nonexistent_symbol_xyz"}),
    )
    .await;
    assert!(
        !is_error,
        "ast_refs on missing symbol should return isError: false (#1270)"
    );
    assert!(
        text.contains("No references found"),
        "response should contain descriptive message, not generic exit code: {text}"
    );
    assert!(
        !text.contains("Operation failed with exit code"),
        "generic message should not appear: {text}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_reorder round-trip ─────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_reorder_alphabetical() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn zebra() {}\nfn alpha() {}\nfn mid() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_reorder",
        serde_json::json!({"path": "lib.rs", "order": "alphabetical"}),
    )
    .await;
    assert!(!is_error, "ast_reorder should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_reorder ok: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    let alpha = content.find("fn alpha").unwrap();
    let mid = content.find("fn mid").unwrap();
    let zebra = content.find("fn zebra").unwrap();
    assert!(
        alpha < mid && mid < zebra,
        "should be alphabetical: {content}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_group round-trip ───────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_group_creates_module() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn public_api() {}\nfn test_a() {}\nfn test_b() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_group",
        serde_json::json!({
            "path": "lib.rs",
            "module": "tests",
            "symbols": ["test_a", "test_b"],
            "preamble": "use super::*;"
        }),
    )
    .await;
    assert!(!is_error, "ast_group should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_group ok: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(content.contains("mod tests"), "module created: {content}");
    assert!(
        content.contains("use super::*;"),
        "preamble present: {content}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_move round-trip ────────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_move_between_files() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("src.rs"),
        "fn keep() {}\nfn moveme() { 42 }\n",
    )
    .unwrap();
    fs::write(dir.path().join("dst.rs"), "fn existing() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_move",
        serde_json::json!({
            "path": "src.rs",
            "target": "dst.rs",
            "symbols": ["moveme"]
        }),
    )
    .await;
    assert!(!is_error, "ast_move should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_move ok: {val}");

    let src = fs::read_to_string(dir.path().join("src.rs")).unwrap();
    let dst = fs::read_to_string(dir.path().join("dst.rs")).unwrap();
    assert!(!src.contains("moveme"), "removed from source: {src}");
    assert!(dst.contains("fn moveme"), "added to target: {dst}");
    assert!(dst.contains("fn existing"), "existing kept: {dst}");
    client.cancel().await.unwrap();
}

// ── MCP ast_extract_to_file round-trip ─────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_extract_to_file() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn main() {}\n\nmod tests {\n    fn test_one() {}\n}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_extract_to_file",
        serde_json::json!({
            "source": "lib.rs",
            "symbol": "tests",
            "target": "tests.rs",
            "replacement": "mod tests;",
            "prepend": "use super::*;\n"
        }),
    )
    .await;
    assert!(!is_error, "ast_extract_to_file should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_extract_to_file ok: {val}");

    let src = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    let tgt = fs::read_to_string(dir.path().join("tests.rs")).unwrap();
    assert!(src.contains("mod tests;"), "replacement in source: {src}");
    assert!(
        tgt.contains("fn test_one"),
        "extracted content in target: {tgt}"
    );
    assert!(tgt.contains("use super::*;"), "prepend in target: {tgt}");
    client.cancel().await.unwrap();
}

// ── MCP ast_split round-trip ───────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_split_distributes_symbols() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("big.rs"),
        "struct Config {}\nfn helper() {}\nfn main() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_split",
        serde_json::json!({
            "source": "big.rs",
            "targets": [
                {"path": "types.rs", "symbols": ["Config"]},
                {"path": "utils.rs", "symbols": ["helper"]}
            ],
            "keep_in_source": ["main"],
            "source_suffix": "mod types;\nmod utils;\n"
        }),
    )
    .await;
    assert!(!is_error, "ast_split should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_split ok: {val}");

    let src = fs::read_to_string(dir.path().join("big.rs")).unwrap();
    let types = fs::read_to_string(dir.path().join("types.rs")).unwrap();
    let utils = fs::read_to_string(dir.path().join("utils.rs")).unwrap();
    assert!(src.contains("fn main"), "main stays: {src}");
    assert!(src.contains("mod types;"), "suffix appended: {src}");
    assert!(types.contains("struct Config"), "Config in types: {types}");
    assert!(utils.contains("fn helper"), "helper in utils: {utils}");
    client.cancel().await.unwrap();
}

// ── MCP execute_plan plan.cwd (#1465) ──────────────────────────

#[tokio::test]
async fn test_mcp_execute_plan_honors_nested_cwd() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("fixtures").join("complex");
    fs::create_dir_all(&nested).unwrap();
    // Collision: same relative name at workspace root and under plan.cwd.
    fs::write(dir.path().join("config.json"), r#"{"where":"root"}"#).unwrap();
    fs::write(nested.join("config.json"), r#"{"where":"nested"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "cwd": "fixtures/complex",
                "operations": [{
                    "op": "doc.set",
                    "path": "config.json",
                    "selector": "where",
                    "value": "updated"
                }]
            }
        }),
    )
    .await;
    assert!(
        !is_error,
        "execute_plan with nested cwd should succeed: {val}"
    );
    assert_eq!(val["ok"], true, "plan ok: {val}");

    let nested_content = fs::read_to_string(nested.join("config.json")).unwrap();
    assert!(
        nested_content.contains("updated"),
        "nested file must change: {nested_content}"
    );
    let root_content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(
        root_content.contains("root") && !root_content.contains("updated"),
        "workspace-root collision must not be written: {root_content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_execute_plan_rejects_cwd_escape() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.json"), r#"{}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "cwd": "../outside",
                "operations": [{
                    "op": "doc.set",
                    "path": "ok.json",
                    "selector": "x",
                    "value": 1
                }]
            }
        }),
    )
    .await;
    assert!(
        is_error,
        "escaping plan.cwd must be a tool error, not silent strip: {val}"
    );
    let text = val.to_string();
    assert!(
        text.contains("cwd") || text.contains("rejected") || text.contains("contain"),
        "error should mention cwd rejection: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_execute_plan_rejects_absolute_cwd() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.json"), r#"{}"#).unwrap();
    let abs = dir.path().to_string_lossy().to_string();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "cwd": abs,
                "operations": [{
                    "op": "doc.set",
                    "path": "ok.json",
                    "selector": "x",
                    "value": 1
                }]
            }
        }),
    )
    .await;
    assert!(is_error, "absolute plan.cwd must be rejected on MCP: {val}");
    let text = val.to_string();
    assert!(
        text.contains("relative") || text.contains("absolute") || text.contains("cwd"),
        "error should mention absolute/relative cwd policy: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_execute_plan_rejects_empty_cwd() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.json"), r#"{}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "cwd": "  ",
                "operations": [{
                    "op": "doc.set",
                    "path": "ok.json",
                    "selector": "x",
                    "value": 1
                }]
            }
        }),
    )
    .await;
    assert!(is_error, "whitespace-only plan.cwd must be rejected: {val}");
    let text = val.to_string();
    assert!(
        text.contains("empty") || text.contains("whitespace") || text.contains("cwd"),
        "error should mention empty cwd: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_execute_plan_rejects_cwd_with_for_each() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("a.txt"), "old\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "cwd": "nested",
                "for_each": {"glob": "*.txt"},
                "operations": [{
                    "op": "replace",
                    "path": "{path}",
                    "old": "old",
                    "new": "new"
                }]
            }
        }),
    )
    .await;
    assert!(is_error, "cwd + for_each must be rejected: {val}");
    let text = val.to_string();
    assert!(
        text.contains("for_each") && text.contains("cwd"),
        "error should name both for_each and cwd: {text}"
    );
    // File must be unchanged (plan rejected before apply).
    assert_eq!(fs::read_to_string(nested.join("a.txt")).unwrap(), "old\n");
    client.cancel().await.unwrap();
}

// ── MCP execute_plan with for_each ─────────────────────────────

#[tokio::test]
async fn test_mcp_execute_plan_for_each() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "old\n").unwrap();
    fs::write(src.join("b.txt"), "old\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "execute_plan",
        serde_json::json!({
            "plan": {
                "version": 1,
                "for_each": {"glob": "src/*.txt"},
                "operations": [{
                    "op": "replace",
                    "path": "{path}",
                    "old": "old",
                    "new": "new"
                }]
            }
        }),
    )
    .await;
    assert!(
        !is_error,
        "execute_plan with for_each should succeed: {val}"
    );
    assert_eq!(val["ok"], true, "for_each plan ok: {val}");

    let a = fs::read_to_string(src.join("a.txt")).unwrap();
    let b = fs::read_to_string(src.join("b.txt")).unwrap();
    assert_eq!(a.trim(), "new", "a.txt should be updated: {a}");
    assert_eq!(b.trim(), "new", "b.txt should be updated: {b}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_case_insensitive_no_false_warnings() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // File has uppercase "TODO" but the replace uses lowercase "todo" with
    // case_insensitive=true. Before the fix, validate_edit_nth would report
    // a false "pattern not found" warning because it uses case-sensitive
    // content.contains(from).
    fs::write(dir.path().join("code.rs"), "// TODO: fix this\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "replace_text",
        serde_json::json!({
            "path": "code.rs",
            "old": "todo",
            "new": "DONE",
            "case_insensitive": true
        }),
    )
    .await;
    assert!(!is_error, "case-insensitive replace should succeed: {val}");
    assert_eq!(val["ok"], true, "replace ok field: {val}");
    assert_eq!(val["files_changed"], 1, "should change file: {val}");

    // Verify the replacement actually happened.
    let content = fs::read_to_string(dir.path().join("code.rs")).unwrap();
    assert!(
        content.contains("DONE"),
        "case-insensitive replace should have replaced TODO: {content}"
    );

    // Verify no spurious warning text in the response.
    let response_text = format!("{val}");
    assert!(
        !response_text.contains("not found"),
        "should not produce false 'pattern not found' warning: {val}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_dedupe_headings() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\nFirst content\n\n# Title\n\nDuplicate content\n\n## Sub\n\nBody\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "md_dedupe_headings",
        serde_json::json!({"path": "doc.md"}),
    )
    .await;
    assert!(!is_error, "md_dedupe_headings should succeed: {val}");
    assert_eq!(val["ok"], true, "md_dedupe ok: {val}");

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    // Count occurrences of "# Title" (level-1 heading).
    let title_count =
        content.matches("\n# Title").count() + if content.starts_with("# Title") { 1 } else { 0 };
    assert_eq!(
        title_count, 1,
        "should have removed duplicate heading, got: {content}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_insert round-trip ──────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_insert_after_symbol() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn existing() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_insert",
        serde_json::json!({
            "path": "lib.rs",
            "content": "fn added() { 42 }",
            "after": "existing"
        }),
    )
    .await;
    assert!(!is_error, "ast_insert should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_insert ok: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        content.contains("fn added()"),
        "inserted function should appear: {content}"
    );
    let existing_pos = content.find("fn existing").unwrap();
    let added_pos = content.find("fn added").unwrap();
    assert!(
        added_pos > existing_pos,
        "added should come after existing: {content}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_wrap round-trip ────────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_wrap_symbols_in_module() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn test_a() {}\nfn test_b() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_wrap",
        serde_json::json!({
            "path": "lib.rs",
            "symbols": ["test_a", "test_b"],
            "wrapper": "mod tests"
        }),
    )
    .await;
    assert!(!is_error, "ast_wrap should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_wrap ok: {val}");

    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        content.contains("mod tests"),
        "wrapper module should appear: {content}"
    );
    assert!(
        content.contains("fn test_a"),
        "wrapped function test_a should remain: {content}"
    );
    client.cancel().await.unwrap();
}

// ── MCP ast_imports round-trip ─────────────────────────────────

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_imports_add() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.rs"), "use std::io;\n\nfn main() {}\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_imports",
        serde_json::json!({
            "path": "main.rs",
            "add": ["use std::collections::HashMap;"]
        }),
    )
    .await;
    assert!(!is_error, "ast_imports add should succeed: {val}");
    assert_eq!(val["ok"], true, "ast_imports ok: {val}");

    let content = fs::read_to_string(dir.path().join("main.rs")).unwrap();
    assert!(
        content.contains("use std::collections::HashMap"),
        "added import should appear: {content}"
    );
    assert!(
        content.contains("use std::io"),
        "existing import should remain: {content}"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "ast")]
#[tokio::test]
async fn test_mcp_ast_imports_list_only() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "use std::io;\nuse std::fs;\n\nfn f() {}\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "ast_imports",
        serde_json::json!({"path": "lib.rs"}),
    )
    .await;
    assert!(!is_error, "ast_imports list should succeed: {val}");
    // List-only mode returns the imports array rather than ok:true.
    assert_eq!(val["count"], 2, "should list 2 imports: {val}");

    // File should be unchanged (read-only operation).
    let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert_eq!(
        content, "use std::io;\nuse std::fs;\n\nfn f() {}\n",
        "file should be unchanged"
    );
    client.cancel().await.unwrap();
}
