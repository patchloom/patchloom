// size-waiver: co-located MCP unit tests, see #1408. Intentionally co-located with the server module for shared test helpers; not unfinished Phase 4 work.
use super::*;
use rmcp::ServiceExt;

/// Spin up a PatchloomService over a duplex stream and return the
/// connected client handle.
async fn spawn_test_client(
    cwd: std::path::PathBuf,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let service = PatchloomService::new(cwd, None).unwrap();
    tokio::spawn(async move {
        let server = service.serve(server_transport).await.unwrap();
        server.waiting().await.unwrap();
    });
    ().serve(client_transport).await.unwrap()
}

// Path containment unit tests (validate_path_contained, validate_path_resolved)
// have been moved to crate::containment::tests. The MCP-level integration test
// `mcp_path_traversal_rejected_via_protocol` above verifies the end-to-end
// path rejection through the MCP protocol layer.

/// Spawn a test client with JSONL logging enabled.
async fn spawn_test_client_with_log(
    cwd: std::path::PathBuf,
    log_path: std::path::PathBuf,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let service =
        PatchloomService::new(cwd, Some(log_path.to_string_lossy().into_owned())).unwrap();
    tokio::spawn(async move {
        let server = service.serve(server_transport).await.unwrap();
        server.waiting().await.unwrap();
    });
    ().serve(client_transport).await.unwrap()
}

mod basic {
    use super::*;

    #[tokio::test]
    async fn mcp_lists_expected_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let tools = client.peer().list_all_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        let descriptions = tools
            .iter()
            .map(|t| (t.name.as_ref(), t.description.as_deref().unwrap_or("")))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert!(names.contains(&"doc_set"), "missing doc_set tool");
        assert!(names.contains(&"doc_get"), "missing doc_get tool");
        assert!(names.contains(&"doc_query"), "missing doc_query tool");
        assert!(names.contains(&"doc_diff"), "missing doc_diff tool");
        assert!(names.contains(&"read_file"), "missing read_file tool");
        assert!(names.contains(&"search_files"), "missing search_files tool");
        assert_eq!(
            descriptions.get("search_files"),
            Some(
                &"Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for Bline parity: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results. Other options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".blineignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
            ),
            "search_files description drifted"
        );
        assert!(names.contains(&"git_status"), "missing git_status tool");
        assert!(names.contains(&"replace_text"), "missing replace_text tool");
        assert_eq!(
            descriptions.get("replace_text"),
            Some(
                &"Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with new=\"\" to delete lines). IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"README.md\", \"old\": \"1.0.0\", \"new\": \"2.0.0\"}"
            ),
            "replace_text description drifted"
        );
        assert!(
            names.contains(&"fix_whitespace"),
            "missing fix_whitespace tool"
        );
        let expected_fix_ws = crate::schema::mcp_tool_description(
            "tidy.fix",
            Some(
                "Defaults: trim trailing whitespace and ensure final newline when those fields are omitted.",
            ),
        );
        assert_eq!(
            descriptions.get("fix_whitespace").map(|s| s.to_string()),
            Some(expected_fix_ws),
            "fix_whitespace description drifted from schema registry"
        );
        assert!(names.contains(&"move_file"), "missing move_file tool");
        assert!(names.contains(&"create_file"), "missing create_file tool");
        assert!(names.contains(&"delete_file"), "missing delete_file tool");
        assert!(names.contains(&"apply_patch"), "missing apply_patch tool");
        assert!(
            names.contains(&"batch_replace"),
            "missing batch_replace tool"
        );
        assert!(names.contains(&"batch_tidy"), "missing batch_tidy tool");
        assert!(names.contains(&"execute_plan"), "missing execute_plan tool");
        assert!(
            names.contains(&"md_insert_after_heading"),
            "missing md_insert_after_heading tool"
        );
        assert!(
            names.contains(&"md_insert_before_heading"),
            "missing md_insert_before_heading tool"
        );
        assert!(
            names.contains(&"md_move_section"),
            "missing md_move_section tool"
        );
        assert!(names.contains(&"append_file"), "missing append_file tool");
        assert!(names.contains(&"prepend_file"), "missing prepend_file tool");
        #[cfg(feature = "ast")]
        {
            assert!(names.contains(&"ast_insert"), "missing ast_insert tool");
            assert!(names.contains(&"ast_wrap"), "missing ast_wrap tool");
            assert!(names.contains(&"ast_imports"), "missing ast_imports tool");
            assert!(names.contains(&"ast_reorder"), "missing ast_reorder tool");
            assert!(names.contains(&"ast_group"), "missing ast_group tool");
            assert!(names.contains(&"ast_move"), "missing ast_move tool");
            assert!(
                names.contains(&"ast_extract_to_file"),
                "missing ast_extract_to_file tool"
            );
            assert!(names.contains(&"ast_split"), "missing ast_split tool");
        }
        #[cfg(not(feature = "ast"))]
        {
            assert!(
                !names.iter().any(|n| n.starts_with("ast_")),
                "ast tools must not appear without the ast feature: {names:?}"
            );
        }
        assert!(
            names.contains(&"md_dedupe_headings"),
            "missing md_dedupe_headings tool"
        );
        // Surface honesty: live list_tools must equal registry ∪ custom inventory
        // for the active feature set (AST tools only when `ast` is enabled).
        {
            use super::super::registry::MCP_TOOL_REGISTRY;
            use super::super::surface::{custom_mcp_tools, custom_tool_names};
            use std::collections::BTreeSet;

            let live: BTreeSet<&str> = names.iter().copied().collect();
            let mut expected: BTreeSet<&str> =
                MCP_TOOL_REGISTRY.iter().map(|t| t.tool_name).collect();
            expected.extend(custom_tool_names());
            assert_eq!(
                live,
                expected,
                "list_tools drifted from surface inventory.\nonly live: {:?}\nonly inventory: {:?}",
                live.difference(&expected).collect::<Vec<_>>(),
                expected.difference(&live).collect::<Vec<_>>(),
            );
            assert_eq!(
                custom_mcp_tools().count() + MCP_TOOL_REGISTRY.len(),
                live.len()
            );
        }
        client.cancel().await.unwrap();
    }

    #[test]
    fn mcp_search_params_accept_new_bline_ignore_fields() {
        // Ensures schemars/serde accept the new fields added for #821 parity.
        let json = r#"{
            "pattern": "TODO",
            "paths": ["src/"],
            "globs": ["*.rs"],
            "exclude_patterns": ["target/**"],
            "custom_ignore_filenames": [".blineignore"],
            "max_results": 10,
            "literal": true
        }"#;
        let p: SearchParams =
            serde_json::from_str(json).expect("SearchParams deserial with new ignore/max fields");
        assert_eq!(p.globs.len(), 1);
        assert_eq!(p.max_results, 10);
    }

    #[tokio::test]
    async fn mcp_server_info_has_correct_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let info = client.peer_info().expect("peer info should be set");
        assert_eq!(info.server_info.name, "patchloom");
        client.cancel().await.unwrap();
    }

    /// Verify server instructions contain tool category guide (#1273).
    ///
    /// Models search for tools by keyword (e.g. "replace_text" for YAML edits)
    /// and miss better-fit tools (doc_set) when the instructions don't mention
    /// categories. This test ensures the instructions always include the
    /// category guide that steers models to the right tool class.
    #[tokio::test]
    async fn mcp_instructions_contain_tool_categories() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let info = client.peer_info().expect("peer info should be set");
        let instructions = info
            .instructions
            .as_deref()
            .expect("server should have instructions");
        // Must contain category headers
        assert!(
            instructions.contains("Document ops"),
            "instructions must mention Document ops category"
        );
        assert!(
            instructions.contains("Markdown ops"),
            "instructions must mention Markdown ops category"
        );
        assert!(
            instructions.contains("Text ops"),
            "instructions must mention Text ops category"
        );
        #[cfg(feature = "ast")]
        {
            assert!(
                instructions.contains("AST ops"),
                "instructions must mention AST ops category when ast is enabled"
            );
            // No accidental indent from push_str bodies (cycle 5 honesty).
            assert!(
                instructions.contains("\n- AST ops"),
                "AST category line must not have leading indent spaces"
            );
        }
        #[cfg(not(feature = "ast"))]
        assert!(
            !instructions.contains("AST ops"),
            "instructions must not advertise AST ops without the ast feature"
        );
        assert!(
            instructions.contains("\n- Plan ops"),
            "Plan category line must not have leading indent spaces"
        );
        assert!(
            instructions.contains("File ops"),
            "instructions must mention File ops category"
        );
        // Must contain the key steering hint from #1273
        assert!(
            instructions.contains("doc_set"),
            "instructions must mention doc_set for discoverability"
        );
        assert!(
            instructions.contains("JSON/YAML/TOML"),
            "instructions must associate doc_* with JSON/YAML/TOML"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_example_argument_keys_match_schemas() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let tools = client.peer().list_all_tools().await.unwrap();

        // Build map of tool_name -> set of valid property keys from schemas
        let mut schema_keys: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for tool in &tools {
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                schema_keys.insert(tool.name.to_string(), props.keys().cloned().collect());
            }
        }

        // Read and validate the example file
        let example_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/08-mcp-tool-call.json");
        let example_content = std::fs::read_to_string(&example_path)
            .expect("failed to read examples/08-mcp-tool-call.json");
        let example_json: serde_json::Value = serde_json::from_str(&example_content)
            .expect("failed to parse examples/08-mcp-tool-call.json");

        let examples = example_json["examples"]
            .as_array()
            .expect("examples should be an array");

        let mut errors = Vec::new();
        for entry in examples {
            let tool_name = entry["tool"].as_str().expect("tool should be a string");
            let arguments = entry["arguments"]
                .as_object()
                .expect("arguments should be an object");

            if let Some(valid_keys) = schema_keys.get(tool_name) {
                for key in arguments.keys() {
                    if !valid_keys.contains(key) {
                        errors.push(format!(
                            "tool '{}': unknown argument '{}' (valid: {:?})",
                            tool_name, key, valid_keys
                        ));
                    }
                }
            } else {
                errors.push(format!("tool '{}': not found in MCP tool list", tool_name));
            }
        }

        assert!(
            errors.is_empty(),
            "MCP example validation errors:\n{}",
            errors.join("\n")
        );
        client.cancel().await.unwrap();
    }

    /// Verify field drift between hand-written MCP params structs and Operation variants.
    ///
    /// Auto-generated tools (registered via MCP_TOOL_REGISTRY) are drift-proof by
    /// construction since they use the Operation variant's schema directly. This test
    /// only covers hand-written handlers that maintain their own params structs.
    #[test]
    fn mcp_params_fields_match_operation_variants() {
        use crate::schema::operation_variant_schema;

        fn schema_keys_for<T: schemars::JsonSchema>() -> std::collections::BTreeSet<String> {
            let generator = schemars::generate::SchemaSettings::default().into_generator();
            let root = generator.into_root_schema_for::<T>();
            let v = serde_json::to_value(root).unwrap();
            v.get("properties")
                .and_then(|p| p.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default()
        }

        fn op_schema_keys(op_name: &str) -> std::collections::BTreeSet<String> {
            let schema = operation_variant_schema(op_name).unwrap();
            schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default()
        }

        struct Check {
            op_name: &'static str,
            mcp_keys: std::collections::BTreeSet<String>,
            mcp_only_allowed: &'static [&'static str],
            op_only_allowed: &'static [&'static str],
        }

        // Only hand-written handlers need drift checking.
        let checks = vec![
            Check {
                op_name: "replace",
                mcp_keys: schema_keys_for::<ReplaceParams>(),
                mcp_only_allowed: &["strict", "regex"],
                op_only_allowed: &["glob", "mode"],
            },
            // tidy.fix / fix_whitespace is registry-generated (no hand-written params).
            Check {
                op_name: "patch.apply",
                mcp_keys: schema_keys_for::<PatchParams>(),
                mcp_only_allowed: &["strict"],
                op_only_allowed: &[],
            },
            Check {
                op_name: "md.move_section",
                mcp_keys: schema_keys_for::<MdMoveSectionParams>(),
                mcp_only_allowed: &[],
                op_only_allowed: &[],
            },
            Check {
                op_name: "md.lint_agents",
                mcp_keys: schema_keys_for::<MdLintAgentsParams>(),
                mcp_only_allowed: &[],
                op_only_allowed: &[],
            },
            Check {
                op_name: "search",
                mcp_keys: schema_keys_for::<SearchParams>(),
                mcp_only_allowed: &["paths", "files_with_matches", "count", "literal"],
                op_only_allowed: &["path", "regex"],
            },
        ];

        let mut errors = Vec::new();
        for check in &checks {
            let op_keys = op_schema_keys(check.op_name);
            let mcp_only_allowed: std::collections::BTreeSet<&str> =
                check.mcp_only_allowed.iter().copied().collect();
            let op_only_allowed: std::collections::BTreeSet<&str> =
                check.op_only_allowed.iter().copied().collect();

            for key in &check.mcp_keys {
                if !op_keys.contains(key) && !mcp_only_allowed.contains(key.as_str()) {
                    errors.push(format!(
                        "{}: MCP param '{}' not in Operation (add to op_only_allowed if intentional)",
                        check.op_name, key
                    ));
                }
            }

            for key in &op_keys {
                if !check.mcp_keys.contains(key) && !op_only_allowed.contains(key.as_str()) {
                    errors.push(format!(
                        "{}: Operation field '{}' not in MCP params (add to mcp_only_allowed if intentional)",
                        check.op_name, key
                    ));
                }
            }
        }

        assert!(
            errors.is_empty(),
            "MCP params / Operation field drift detected:\n  {}",
            errors.join("\n  ")
        );
    }

    #[test]
    fn patch_apply_safe_paths_pass_containment() {
        let dir = tempfile::TempDir::new().unwrap();
        let safe_diff = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let ops = vec![Operation::PatchApply {
            diff: safe_diff.to_string(),
            on_stale: crate::ops::patch::OnStale::Fail,
            allow_conflicts: false,
        }];
        let result = validate_operation_paths(&ops, dir.path());
        result.expect("PatchApply with safe paths should pass");
    }

    #[tokio::test]
    async fn mcp_log_writes_jsonl_on_tool_call() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        let client = spawn_test_client_with_log(dir.path().to_path_buf(), log_file.clone()).await;

        // Call read_file to trigger a log entry.
        let params = rmcp::model::CallToolRequestParams::new("read_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "test.txt",
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            result.is_error != Some(true),
            "read_file should succeed: {result:?}"
        );
        client.cancel().await.unwrap();

        // Verify the log file contains a valid JSONL entry.
        let log_content = std::fs::read_to_string(&log_file).expect("log file should exist");
        let lines: Vec<&str> = log_content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly 1 log line");
        let entry: serde_json::Value =
            serde_json::from_str(lines[0]).expect("log line should be valid JSON");
        assert_eq!(entry["tool"], "read_file");
        assert_eq!(entry["ok"], true);
        assert!(entry["ts"].is_number(), "ts should be a number");
        assert!(
            entry["duration_ms"].is_number(),
            "duration_ms should be a number"
        );
    }

    #[tokio::test]
    async fn mcp_log_records_error_on_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        let client = spawn_test_client_with_log(dir.path().to_path_buf(), log_file.clone()).await;

        // Call doc_set on a non-existent file to trigger an error result.
        let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "nonexistent.json",
                "selector": "key",
                "value": "val",
            }))
            .unwrap(),
        );
        let _result = client.peer().call_tool(params).await;
        client.cancel().await.unwrap();

        let log_content = std::fs::read_to_string(&log_file).expect("log file should exist");
        let lines: Vec<&str> = log_content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly 1 log line");
        let entry: serde_json::Value =
            serde_json::from_str(lines[0]).expect("log line should be valid JSON");
        assert_eq!(entry["tool"], "doc_set");
        assert_eq!(entry["ok"], false);
    }

    #[test]
    fn validate_content_size_accepts_small() {
        validate_content_size("field", "hello").unwrap();
    }

    #[test]
    fn validate_param_size_accepts_small() {
        validate_param_size("key", "a.b.c").unwrap();
    }

    #[test]
    fn validate_batch_size_accepts_small() {
        validate_batch_size("files", 5).unwrap();
    }

    #[test]
    fn validate_json_depth_accepts_shallow() {
        let val = serde_json::json!({"a": {"b": "c"}});
        validate_json_depth("value", &val).unwrap();
    }

    #[test]
    fn validate_json_depth_accepts_scalar() {
        let val = serde_json::json!("hello");
        validate_json_depth("value", &val).unwrap();
    }

    #[tokio::test]
    async fn mcp_no_log_without_flag() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        // Use default client (no log flag).
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("read_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "test.txt",
            }))
            .unwrap(),
        );
        let _result = client.peer().call_tool(params).await.unwrap();
        client.cancel().await.unwrap();

        // Log file should not exist since no log path was configured.
        assert!(
            !log_file.exists(),
            "log file should not exist without --log"
        );
    }

    #[tokio::test]
    async fn mcp_execute_plan_mixed_ops_atomic() {
        // Test the new execute_plan tool with a mixed plan (doc + replace + create).
        // This is the core of #827: one call for atomic multi-op instead of many parallel/serial.
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Prepare initial file
        std::fs::write(dir.path().join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();

        let plan_json = serde_json::json!({
            "version": 1,
            "strict": true,
            "operations": [
                {
                    "op": "doc.set",
                    "path": "package.json",
                    "selector": "version",
                    "value": "2.0.0"
                },
                {
                    "op": "replace",
                    "path": "package.json",
                    "old": "2.0.0",
                    "new": "2.1.0"
                },
                {
                    "op": "file.create",
                    "path": "CREATED.md",
                    "content": "# Created via plan\n"
                }
            ]
        });

        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "execute_plan should succeed: {:?}",
            result
        );

        // Verify results
        let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert!(
            pkg.contains("2.1.0"),
            "doc.set + replace should have updated to 2.1.0"
        );

        let created = std::fs::read_to_string(dir.path().join("CREATED.md")).unwrap();
        assert!(created.contains("Created via plan"));

        client.cancel().await.unwrap();
    }
}

mod security {
    use super::*;

    #[tokio::test]
    async fn mcp_path_traversal_rejected_via_protocol() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "../../etc/passwd",
                "selector": "root",
                "value": "hacked"
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "path traversal should be rejected");
        client.cancel().await.unwrap();
    }

    #[test]
    fn patch_apply_path_containment_validation() {
        let dir = tempfile::TempDir::new().unwrap();
        let evil_diff =
            "--- a/../../etc/passwd\n+++ b/../../etc/passwd\n@@ -1,1 +1,1 @@\n-root\n+hacked\n";
        let ops = vec![Operation::PatchApply {
            diff: evil_diff.to_string(),
            on_stale: crate::ops::patch::OnStale::Fail,
            allow_conflicts: false,
        }];
        let result = validate_operation_paths(&ops, dir.path());
        assert!(
            result.is_err(),
            "PatchApply with escaping paths should be rejected"
        );
    }

    #[test]
    fn validate_content_size_rejects_oversized() {
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        let err = validate_content_size("content", &big).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("content"), "error should name the field");
        assert!(msg.contains("exceeds"), "error should say exceeds");
    }

    #[test]
    fn validate_param_size_rejects_oversized() {
        let big = "x".repeat(MAX_PARAM_BYTES + 1);
        let err = validate_param_size("pattern", &big).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("pattern"), "error should name the field");
    }

    #[test]
    fn validate_batch_size_rejects_oversized() {
        let err = validate_batch_size("files", MAX_BATCH_FILES + 1).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("files"), "error should name the field");
    }

    #[test]
    fn validate_json_depth_rejects_deeply_nested() {
        // Build a value nested deeper than MAX_JSON_DEPTH.
        let mut val = serde_json::json!("leaf");
        for _ in 0..MAX_JSON_DEPTH + 1 {
            val = serde_json::json!([val]);
        }
        let err = validate_json_depth("value", &val).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("nesting depth"), "error should mention depth");
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_content_via_protocol() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let big_content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let params = rmcp::model::CallToolRequestParams::new("create_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "big.txt",
                "content": big_content,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized content should be rejected");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_batch() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let files: Vec<String> = (0..MAX_BATCH_FILES + 1)
            .map(|i| format!("file{i}.txt"))
            .collect();
        let params = rmcp::model::CallToolRequestParams::new("batch_tidy").with_arguments(
            serde_json::from_value(serde_json::json!({
                "files": files,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized batch should be rejected");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_doc_query_selector() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("data.json"), r#"{"a":1}"#).unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let big_selector = "x".repeat(MAX_PARAM_BYTES + 1);
        let params = rmcp::model::CallToolRequestParams::new("doc_query").with_arguments(
            serde_json::from_value(serde_json::json!({
                "action": "has",
                "path": "data.json",
                "selector": big_selector,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized selector should be rejected");
        client.cancel().await.unwrap();
    }
}

mod integrity {
    use super::*;

    #[tokio::test]
    async fn mcp_execute_plan_strict_rollback_on_error() {
        // Verify that strict plan rolls back on failure (e.g. invalid op mid-plan).
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "original").unwrap();

        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Plan that will fail on second op (bad replace or non-existing for safety, but use doc on non structured? Use a replace that requires mode or simply a bad path? Better: use a plan that succeeds first, fails second.
        // For simplicity, use a plan with an op that causes parse/validate fail, but since plan itself is valid, use a mid failure like delete non existing in strict?
        // Simpler: a plan that does create (ok), then a replace that is invalid (no mode).
        // But to trigger runtime fail in tx, easier: use doc.set on a file that will cause later validate fail, but to keep simple use a non-existent for a delete in plan?
        // Actually for this, do two creates, then a doc.set on bad structured that may not rollback file create? File creates are part of tx.
        // Use a plan that the second op fails (e.g. move non-existing source).
        let plan_json = serde_json::json!({
            "version": 1,
            "strict": true,
            "operations": [
                { "op": "file.create", "path": "first.txt", "content": "one" },
                { "op": "file.delete", "path": "does-not-exist.txt" }  // will fail
            ]
        });

        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        // Should report error
        assert!(
            result.is_error.unwrap_or(false),
            "plan with failing op under strict should error"
        );

        // first.txt should NOT exist (rollback)
        assert!(
            !dir.path().join("first.txt").exists(),
            "strict plan should have rolled back the first create"
        );

        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_execute_plan_strips_format_validate() {
        // Verify that format/validate lifecycle steps submitted via MCP are
        // stripped to prevent arbitrary command execution (security fix).
        let dir = tempfile::TempDir::new().unwrap();
        let marker = dir.path().join("pwned.txt");
        std::fs::write(dir.path().join("target.txt"), "hello").unwrap();

        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Submit a plan with a format step that would create a marker file.
        // If the format step is NOT stripped, the marker file will exist.
        let plan_json = serde_json::json!({
            "version": 1,
            "operations": [
                { "op": "replace", "path": "target.txt", "old": "hello", "new": "world" }
            ],
            "format": [{ "cmd": format!("touch {}", marker.display()) }],
            "validate": [{ "cmd": format!("touch {}", marker.display()), "required": false }]
        });

        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "plan should succeed with format/validate stripped"
        );

        // The marker file must NOT exist (format/validate commands were stripped).
        assert!(
            !marker.exists(),
            "format/validate commands should be stripped by MCP handler"
        );

        // The actual replace should have been applied.
        let content = std::fs::read_to_string(dir.path().join("target.txt")).unwrap();
        assert_eq!(content, "world");

        client.cancel().await.unwrap();
    }
}

// --- #1267: server_info tool ---

mod server_info_tests {
    use super::*;

    #[tokio::test]
    async fn server_info_returns_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("server_info");
        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "server_info should succeed"
        );

        let text = result.content.first().unwrap();
        let text = match text {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        let info: serde_json::Value = serde_json::from_str(text).unwrap();
        let cwd = info["cwd"].as_str().unwrap();
        assert_eq!(
            std::path::Path::new(cwd).canonicalize().unwrap(),
            dir.path().canonicalize().unwrap(),
            "server_info cwd should match the server's working directory"
        );
    }

    #[tokio::test]
    async fn server_info_listed_in_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let tools = client.peer().list_all_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(
            names.contains(&"server_info"),
            "server_info should be listed in tools"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn server_info_handles_unicode_path() {
        let base = tempfile::TempDir::new().unwrap();
        let unicode_dir = base.path().join("проект_工作区");
        std::fs::create_dir(&unicode_dir).unwrap();
        let client = spawn_test_client(unicode_dir.clone()).await;

        let params = rmcp::model::CallToolRequestParams::new("server_info");
        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "server_info should succeed with unicode path"
        );

        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        let info: serde_json::Value = serde_json::from_str(text).unwrap();
        let cwd = info["cwd"].as_str().unwrap();
        assert!(
            !cwd.contains('\u{FFFD}'),
            "cwd should not contain replacement character"
        );
        assert!(
            cwd.contains("проект"),
            "cwd should preserve unicode characters"
        );
        client.cancel().await.unwrap();
    }
}

// --- #1270: no_results returns isError: false ---

mod no_results_tests {
    use super::*;

    #[tokio::test]
    async fn search_no_match_returns_success() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("search_files").with_arguments(
            serde_json::from_value(serde_json::json!({
                "pattern": "NONEXISTENT_PATTERN_xyz"
            }))
            .unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        // #1270: no-match should NOT be an error
        assert!(
            !result.is_error.unwrap_or(false),
            "search with no matches should return isError: false, not true"
        );

        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        assert!(
            text.contains("No matches"),
            "should contain 'No matches' message, got: {text}"
        );

        client.cancel().await.unwrap();
    }

    #[cfg(feature = "ast")]
    #[tokio::test]
    async fn ast_list_no_symbols_returns_success() {
        let dir = tempfile::TempDir::new().unwrap();
        // Create a file with no symbol definitions
        std::fs::write(dir.path().join("empty.py"), "# just a comment\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("ast_list").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "empty.py"
            }))
            .unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        // #1270: empty result should NOT be an error
        assert!(
            !result.is_error.unwrap_or(false),
            "ast_list with no symbols should return isError: false, not true"
        );

        client.cancel().await.unwrap();
    }

    #[cfg(feature = "ast")]
    #[tokio::test]
    async fn ast_refs_no_match_returns_success() {
        let dir = tempfile::TempDir::new().unwrap();
        // File has a function definition but nothing references it
        std::fs::write(dir.path().join("lib.rs"), "fn standalone() {}\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("ast_refs").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "lib.rs",
                "symbol": "standalone"
            }))
            .unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        // #1270: no references is a valid answer, not an error
        assert!(
            !result.is_error.unwrap_or(false),
            "ast_refs with no references should return isError: false (#1270)"
        );

        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        assert!(
            text.contains("No references found"),
            "should contain descriptive message, got: {text}"
        );

        client.cancel().await.unwrap();
    }

    #[test]
    fn no_results_helper_returns_success() {
        let result = no_results("No matches found.").unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "no_results should return isError: false"
        );
        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text content"),
        };
        assert_eq!(text, "No matches found.");
    }
}

mod deserialization_tests {
    use super::*;

    #[tokio::test]
    async fn mcp_type_mismatch_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("data.json"), r#"{"key": "val"}"#).unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Send path as a number instead of a string to trigger deserialization error.
        let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": 123,
                "selector": "key",
                "value": "newval"
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(
            result.is_err(),
            "type mismatch should return an MCP error, got: {result:?}"
        );

        client.cancel().await.unwrap();
    }
}

#[cfg(test)]
mod registry_schema_sync {
    use crate::cmd::mcp::registry::MCP_TOOL_REGISTRY;
    use crate::schema::{
        mcp_tool_description, operation_description, operation_example_json,
        registered_operation_names,
    };

    #[test]
    fn mcp_simple_tools_op_names_are_in_schema_registry() {
        let registered: std::collections::HashSet<&str> =
            registered_operation_names().into_iter().collect();
        for tool in MCP_TOOL_REGISTRY {
            assert!(
                registered.contains(tool.op_name),
                "MCP tool {} op_name {} missing from schema registry",
                tool.tool_name,
                tool.op_name,
            );
        }
    }

    /// #1383: simple-tool descriptions are generated from schema meta.
    #[test]
    fn mcp_simple_tool_descriptions_come_from_schema_meta() {
        for tool in MCP_TOOL_REGISTRY {
            let desc = tool.description();
            let base = operation_description(tool.op_name)
                .unwrap_or_else(|| panic!("{}: missing schema description", tool.op_name));
            assert!(
                desc.contains(base),
                "{}: description must include schema base prose\nbase={base}\ndesc={desc}",
                tool.tool_name
            );
            if let Some(example) = operation_example_json(tool.op_name) {
                assert!(
                    desc.contains(example),
                    "{}: description must include schema example JSON",
                    tool.tool_name
                );
            }
            if let Some(extra) = tool.extra {
                assert!(
                    desc.contains(extra),
                    "{}: description must include MCP extra fragment",
                    tool.tool_name
                );
            }
            // Resolved text must match the shared builder (single generation path).
            assert_eq!(
                desc,
                mcp_tool_description(tool.op_name, tool.extra),
                "{}: description() must equal mcp_tool_description()",
                tool.tool_name
            );
        }
    }
}
