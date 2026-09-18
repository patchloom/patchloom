// size-waiver: co-located MCP unit tests (policy #1408). Intentionally co-located with the server module for shared helpers; not unfinished Phase 4 work.
use super::*;
use rmcp::ServiceExt;

/// Spin up a PatchloomService over a duplex stream and return the
/// connected client handle.
async fn spawn_test_client(
    cwd: std::path::PathBuf,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    spawn_test_client_with_surface(cwd, surface::McpSurface::Full).await
}

/// Spawn with an explicit MCP surface (avoids env races in parallel tests).
async fn spawn_test_client_with_surface(
    cwd: std::path::PathBuf,
    surface: surface::McpSurface,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let service = PatchloomService::new_with_surface(cwd, None, surface).unwrap();
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

/// #2551: clone must share the ToolRouter allocation.
#[test]
fn clone_shares_tool_router() {
    let dir = tempfile::TempDir::new().unwrap();
    let svc = PatchloomService::new_with_surface(
        dir.path().to_path_buf(),
        None,
        surface::McpSurface::Full,
    )
    .unwrap();
    let cloned = svc.clone();
    assert!(
        svc.shares_tool_router_with(&cloned),
        "blocking() clones must not copy the ToolRouter HashMap"
    );
}

#[test]
fn interned_description_is_stable() {
    let meta = super::registry::MCP_TOOL_REGISTRY
        .iter()
        .find(|m| m.tool_name == "doc_set")
        .expect("doc_set registry row");
    let a = meta.interned_description();
    let b = meta.interned_description();
    assert_eq!(
        a.as_ptr(),
        b.as_ptr(),
        "intern must reuse the leaked string"
    );
    assert!(a.contains("doc.set") || a.contains("selector") || !a.is_empty());
}

/// #1838: MCP peels CLI doc-query ok/value envelopes back to bare values.
#[test]
fn peel_doc_query_success_value_extracts_value() {
    let envelope = r#"{"ok":true,"value":{"a":1},"path":"/tmp/x.json","selector":"a"}"#;
    let peeled = peel_doc_query_success_value(envelope);
    let v: serde_json::Value = serde_json::from_str(&peeled).unwrap();
    assert_eq!(v, serde_json::json!({"a": 1}), "peeled: {peeled}");
}

#[test]
fn peel_doc_query_success_value_leaves_errors_and_diff() {
    let err = r#"{"ok":false,"error":"no match","error_kind":"no_matches"}"#;
    assert_eq!(peel_doc_query_success_value(err), err);

    let diff = r#"{"identical":false,"differences":["~ a"]}"#;
    assert_eq!(peel_doc_query_success_value(diff), diff);

    let not_json = "plain text";
    assert_eq!(peel_doc_query_success_value(not_json), not_json);
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
        assert!(
            names.contains(&"list_files"),
            "missing list_files tool (#2076)"
        );
        assert_eq!(
            descriptions.get("search_files"),
            Some(
                &"Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for LLM agents: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .agentignore), max_results. Other options: files_with_matches, files_without_match, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Canonical multi-root field is paths (array); singular path is accepted as an alias for one root (same as paths:[path]). Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".agentignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
            ),
            "search_files description drifted"
        );
        #[cfg(feature = "ast")]
        {
            // All AST mutators must carry concurrent / execute_plan guidance.
            for tool in [
                "ast_rename",
                "ast_replace",
                "ast_replace_symbol",
                "ast_delete_symbol",
                "ast_rewrite_signature",
                "ast_insert",
                "ast_wrap",
                "ast_reorder",
                "ast_group",
                "ast_move",
                "ast_extract_to_file",
                "ast_split",
            ] {
                let desc = descriptions.get(tool).copied().unwrap_or("");
                assert!(
                    desc.contains("execute_plan") && desc.contains("concurrent"),
                    "{tool} must warn about concurrent writes / execute_plan (#1468): {desc}"
                );
            }
        }
        // All registry write tools (and multi-file custom writers) must carry concurrent guidance.
        for tool in [
            "doc_set",
            "doc_delete",
            "doc_merge",
            "doc_append",
            "doc_prepend",
            "doc_ensure",
            "doc_delete_where",
            "doc_update",
            "doc_move",
            "md_upsert_bullet",
            "md_table_append",
            "md_replace_section",
            "md_insert_after_heading",
            "md_insert_after_section",
            "md_insert_before_heading",
            "md_dedupe_headings",
            "move_file",
            "append_file",
            "prepend_file",
            "create_file",
            "delete_file",
            "fix_whitespace",
            "batch_tidy",
            "apply_patch",
            "replace_text",
            "batch_replace",
            "md_move_section",
            "apply_fragment",
            "undo_restore",
        ] {
            let desc = descriptions.get(tool).copied().unwrap_or("");
            assert!(
                desc.contains("concurrent") && desc.contains("execute_plan"),
                "{tool} must warn about concurrent writes / execute_plan: {desc}"
            );
        }
        assert!(names.contains(&"git_status"), "missing git_status tool");
        assert!(
            names.contains(&"undo_list"),
            "missing undo_list tool (#2541)"
        );
        assert!(
            names.contains(&"undo_restore"),
            "missing undo_restore tool (#2541)"
        );
        assert!(
            names.contains(&"explain_plan"),
            "missing explain_plan tool (#2541)"
        );
        assert!(
            names.contains(&"tidy_check"),
            "missing tidy_check tool (#2541)"
        );
        assert!(names.contains(&"replace_text"), "missing replace_text tool");
        assert_eq!(
            descriptions.get("replace_text"),
            Some(
                &"Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary, fuzzy, min_fuzzy_score, allow_absent_old. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with new=\"\" to delete lines). Fuzzy: when exact old is absent, refuse by default even if score ≥ min_fuzzy_score (#1758); set allow_absent_old=true only for deliberate approximate recovery. Prefer ast_rename for identifiers. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"README.md\", \"old\": \"1.0.0\", \"new\": \"2.0.0\"}. Insert after anchor (mutually exclusive with new): {\"path\": \"src/main.rs\", \"old\": \"use std::io;\", \"insert_after\": \"use std::fs;\"}"
            ),
            "replace_text description drifted"
        );
        let batch_desc = descriptions.get("batch_replace").copied().unwrap_or("");
        assert!(
            batch_desc.contains("matched_text") && batch_desc.contains("match_mode"),
            "batch_replace must document matched_text + match_mode: {batch_desc}"
        );
        assert!(
            names.contains(&"fix_whitespace"),
            "missing fix_whitespace tool"
        );
        let fix_ws_extra = crate::cmd::mcp::registry::MCP_TOOL_REGISTRY
            .iter()
            .find(|t| t.tool_name == "fix_whitespace")
            .and_then(|t| t.extra);
        let expected_fix_ws = crate::schema::mcp_tool_description("tidy.fix", fix_ws_extra);
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
            names.contains(&"md_insert_after_section"),
            "missing md_insert_after_section tool"
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
            assert!(
                names.contains(&"ast_replace_symbol"),
                "missing ast_replace_symbol tool"
            );
            assert!(
                names.contains(&"ast_delete_symbol"),
                "missing ast_delete_symbol tool"
            );
            assert!(
                names.contains(&"ast_rewrite_signature"),
                "missing ast_rewrite_signature tool"
            );
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
    fn mcp_search_params_accept_custom_ignore_fields() {
        // Ensures schemars/serde accept the new fields added for #821 parity.
        let json = r#"{
            "pattern": "TODO",
            "paths": ["src/"],
            "globs": ["*.rs"],
            "exclude_patterns": ["target/**"],
            "custom_ignore_filenames": [".agentignore"],
            "max_results": 10,
            "literal": true
        }"#;
        let p: SearchParams =
            serde_json::from_str(json).expect("SearchParams deserial with new ignore/max fields");
        assert_eq!(p.globs.len(), 1);
        assert_eq!(p.max_results, 10);
    }

    #[test]
    fn search_params_path_alias_maps_to_paths() {
        // #1467: singular path is LLM prior (matches other tools).
        let p: SearchParams = serde_json::from_str(r#"{"pattern": "TODO", "path": "src/lib.rs"}"#)
            .expect("path alias should deserialize");
        assert_eq!(p.effective_paths(), vec!["src/lib.rs".to_string()]);
        assert!(p.paths.is_empty());
        assert_eq!(p.path.as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn search_params_paths_wins_over_path() {
        let p: SearchParams = serde_json::from_str(
            r#"{"pattern": "TODO", "paths": ["a/", "b/"], "path": "ignored/"}"#,
        )
        .expect("both path and paths should deserialize");
        assert_eq!(
            p.effective_paths(),
            vec!["a/".to_string(), "b/".to_string()]
        );
    }

    #[test]
    fn search_params_unknown_field_still_rejected() {
        let err = serde_json::from_str::<SearchParams>(r#"{"pattern": "TODO", "root": "src/"}"#)
            .unwrap_err();
        assert!(
            err.to_string().contains("unknown field") || err.to_string().contains("root"),
            "deny_unknown_fields must still reject unrelated keys: {err}"
        );
    }

    #[test]
    fn batch_replace_params_file_alias() {
        let p: BatchReplaceParams =
            serde_json::from_str(r#"{"file": "a.txt", "old": "x", "new": "y"}"#)
                .expect("file alias should deserialize");
        assert_eq!(p.effective_files(), vec!["a.txt".to_string()]);
    }

    #[test]
    fn batch_tidy_params_file_alias() {
        let p: BatchTidyParams =
            serde_json::from_str(r#"{"file": "a.txt"}"#).expect("file alias should deserialize");
        assert_eq!(p.effective_files(), vec!["a.txt".to_string()]);
    }

    #[tokio::test]
    async fn mcp_server_info_has_correct_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let info = client.peer_info().expect("peer info should be set");
        // rmcp 3: peer_info.server_info is Option (discovery may omit identity).
        let server = info
            .server_info
            .as_ref()
            .expect("initialize handshake must provide server_info");
        assert_eq!(server.name, "patchloom");
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
                mcp_only_allowed: &["strict", "apply"],
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
                // path is shared with Operation; paths/files_with_matches/files_without_match/count/literal are MCP-shaped
                mcp_only_allowed: &[
                    "paths",
                    "files_with_matches",
                    "files_without_match",
                    "count",
                    "literal",
                ],
                op_only_allowed: &["regex"],
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
            replace_all: false,
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

    /// A hung `--log` path must not pin the tokio runtime (#2464).
    #[cfg(unix)]
    #[test]
    fn log_tool_call_does_not_block_runtime_on_hung_fifo() {
        let dir = tempfile::TempDir::new().unwrap();
        let fifo = dir.path().join("hung.jsonl");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo available on unix CI");
        assert!(status.success(), "mkfifo {fifo:?}");
        let svc = PatchloomService::new(
            dir.path().to_path_buf(),
            Some(fifo.to_string_lossy().into_owned()),
        )
        .unwrap();
        let ok_result: Result<rmcp::model::CallToolResponse, McpError> = Ok(
            rmcp::model::CallToolResponse::Complete(CallToolResult::success(vec![])),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap();
            let yielded = rt.block_on(async {
                tokio::select! {
                    _ = svc.log_tool_call("server_info", 1, &ok_result) => false,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(80)) => true,
                }
            });
            let _ = tx.send(yielded);
        });
        let yielded = rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("runtime stayed blocked on log write");
        assert!(
            yielded,
            "sleep arm must win; log write must not pin the runtime"
        );
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
            replace_all: false,
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

    #[test]
    fn execute_plan_validated_rejects_oversized_file_create() {
        let dir = tempfile::TempDir::new().unwrap();
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        let plan = make_plan_strict(
            vec![Operation::FileCreate {
                path: "big.txt".into(),
                content: big,
                force: None,
            }],
            Some(true),
        );
        let err = execute_plan_validated(plan, dir.path(), None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("exceeds"),
            "oversized plan file.create must hit the same content limit as create_file, got: {msg}"
        );
        assert!(
            !dir.path().join("big.txt").exists(),
            "oversized plan must not write"
        );
    }

    #[test]
    fn execute_plan_validated_rejects_oversized_operations() {
        let dir = tempfile::TempDir::new().unwrap();
        let ops: Vec<Operation> = (0..MAX_BATCH_FILES + 1)
            .map(|i| Operation::FileCreate {
                path: format!("f{i}.txt"),
                content: "x".into(),
                force: None,
            })
            .collect();
        let plan = make_plan_strict(ops, Some(true));
        let err = execute_plan_validated(plan, dir.path(), None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("exceeds"),
            "oversized plan operations must hit the batch limit, got: {msg}"
        );
    }

    #[test]
    fn execute_plan_validated_rejects_deep_doc_value() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut val = serde_json::json!("leaf");
        for _ in 0..MAX_JSON_DEPTH + 1 {
            val = serde_json::json!([val]);
        }
        let plan = make_plan_strict(
            vec![Operation::DocSet {
                path: "data.json".into(),
                selector: "a".into(),
                value: val,
                if_exists: false,
            }],
            Some(true),
        );
        let err = execute_plan_validated(plan, dir.path(), None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("nesting depth") || msg.contains("exceeds"),
            "deep plan doc.set must hit the json-depth limit, got: {msg}"
        );
    }

    #[test]
    fn execute_plan_validated_rejects_oversized_selector() {
        let dir = tempfile::TempDir::new().unwrap();
        let plan = make_plan_strict(
            vec![Operation::DocSet {
                path: "data.json".into(),
                selector: "x".repeat(MAX_PARAM_BYTES + 1),
                value: serde_json::json!(1),
                if_exists: false,
            }],
            Some(true),
        );
        let err = execute_plan_validated(plan, dir.path(), None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("exceeds"),
            "oversized plan selector must hit the param limit, got: {msg}"
        );
    }

    #[tokio::test]
    async fn mcp_execute_plan_rejects_oversized_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let big_content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let plan_json = serde_json::json!({
            "version": 1,
            "operations": [{
                "op": "file.create",
                "path": "big.txt",
                "content": big_content,
            }]
        });
        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(
            result.is_err(),
            "execute_plan oversized file.create should be rejected like create_file"
        );
        assert!(
            !dir.path().join("big.txt").exists(),
            "oversized execute_plan must not write"
        );
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
    async fn server_info_reports_full_surface_by_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let params = rmcp::model::CallToolRequestParams::new("server_info");
        let result = client.peer().call_tool(params).await.unwrap();
        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text"),
        };
        let info: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(info["surface"], "full");
        assert_eq!(
            info["tool_count"].as_u64().unwrap() as usize,
            surface::McpSurface::Full.expected_tool_count()
        );
        assert_eq!(
            info["version"].as_str().unwrap(),
            env!("CARGO_PKG_VERSION"),
            "server_info must report package version for host diagnostics"
        );
        assert!(
            info["protocol_version"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "server_info must report MCP protocol_version matching handshake"
        );
        assert!(
            info["recommendation"]
                .as_str()
                .is_some_and(|s| s.contains("PATCHLOOM_MCP_SURFACE=core")),
            "full surface must recommend core for coding agents (#2070)"
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

// --- PATCHLOOM_MCP_SURFACE=core (#1994) ---

mod surface_core_tests {
    use super::*;

    #[tokio::test]
    async fn core_surface_lists_only_core_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let client =
            spawn_test_client_with_surface(dir.path().to_path_buf(), surface::McpSurface::Core)
                .await;
        let tools = client.peer().list_all_tools().await.unwrap();
        let names: std::collections::BTreeSet<_> =
            tools.iter().map(|t| t.name.as_ref().to_string()).collect();
        let expected: std::collections::BTreeSet<_> = surface::CORE_MCP_TOOL_NAMES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(
            names, expected,
            "core list_tools must equal CORE_MCP_TOOL_NAMES"
        );
        assert_eq!(names.len(), surface::McpSurface::Core.expected_tool_count());
        // Full-only tools must be absent.
        assert!(!names.contains("create_file"));
        assert!(!names.contains("doc_diff"));
        assert!(!names.contains("ast_list"));
        assert!(!names.contains("batch_tidy"));
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn core_surface_server_info_reports_core() {
        let dir = tempfile::TempDir::new().unwrap();
        let client =
            spawn_test_client_with_surface(dir.path().to_path_buf(), surface::McpSurface::Core)
                .await;
        let params = rmcp::model::CallToolRequestParams::new("server_info");
        let result = client.peer().call_tool(params).await.unwrap();
        let text = match result.content.first().unwrap() {
            rmcp::model::ContentBlock::Text(t) => &t.text,
            _ => panic!("expected text"),
        };
        let info: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(info["surface"], "core");
        assert_eq!(
            info["tool_count"].as_u64().unwrap() as usize,
            surface::CORE_MCP_TOOL_NAMES.len()
        );
        assert!(
            info.get("recommendation").is_none(),
            "core surface should omit full-inventory recommendation"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn core_surface_rejects_full_only_tool_call() {
        let dir = tempfile::TempDir::new().unwrap();
        let client =
            spawn_test_client_with_surface(dir.path().to_path_buf(), surface::McpSurface::Core)
                .await;
        let params = rmcp::model::CallToolRequestParams::new("create_file");
        let err = client
            .peer()
            .call_tool(params)
            .await
            .expect_err("create_file must not be callable on core surface");
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("create_file")
                || msg.to_lowercase().contains("not found")
                || msg.to_lowercase().contains("unknown")
                || msg.to_lowercase().contains("disabled"),
            "unexpected error: {msg}"
        );
        client.cancel().await.unwrap();
    }

    #[test]
    fn invalid_surface_env_is_invalid_input() {
        // Parse path (no env mutation): invalid values fail closed.
        let err = surface::McpSurface::parse("tiny").unwrap_err().to_string();
        assert!(err.contains("PATCHLOOM_MCP_SURFACE"));
    }

    #[test]
    fn mcp_server_help_documents_surface_env() {
        use clap::CommandFactory;
        let cmd = crate::cli::Cli::command();
        let mcp = cmd.find_subcommand("mcp-server").expect("mcp-server");
        let mut buf = Vec::new();
        mcp.clone().write_long_help(&mut buf).unwrap();
        let help = String::from_utf8(buf).unwrap();
        assert!(
            help.contains("PATCHLOOM_MCP_SURFACE"),
            "mcp-server --help must document PATCHLOOM_MCP_SURFACE:\n{help}"
        );
        assert!(
            help.contains("core") && help.contains("full"),
            "mcp-server --help must name core|full:\n{help}"
        );
    }

    /// Core handshake instructions must only name tools in CORE_MCP_TOOL_NAMES.
    #[test]
    fn core_server_instructions_list_only_core_tools() {
        let text = super::super::transport::server_instructions(surface::McpSurface::Core);
        for name in surface::CORE_MCP_TOOL_NAMES {
            assert!(
                text.contains(name),
                "core instructions must mention registered tool {name}"
            );
        }
        // Full-only tools must not appear (agent would try them and fail).
        for banned in [
            "create_file",
            "delete_file",
            "doc_diff",
            "doc_merge",
            "batch_tidy",
            "apply_patch",
            "ast_list",
            "ast_rename", // not registered on core; plan-only via execute_plan
            "git_status",
            "File ops",
            "AST ops",
            "md_upsert_bullet",
        ] {
            assert!(
                !text.contains(banned),
                "core instructions must not advertise full-only {banned:?}"
            );
        }
        assert!(
            text.contains("PATCHLOOM_MCP_SURFACE=core"),
            "core instructions should name the env mode"
        );
    }

    /// Full instructions keep the category guide (#1273) and still list non-core tools.
    #[test]
    fn full_server_instructions_include_full_inventory_hints() {
        let text = super::super::transport::server_instructions(surface::McpSurface::Full);
        assert!(text.contains("Document ops"));
        assert!(text.contains("create_file"));
        assert!(text.contains("doc_set"));
        // Morph-class apply_fragment is a first-class registry tool; hosts that
        // bias on handshake text must see it next to replace_text.
        assert!(
            text.contains("apply_fragment"),
            "full instructions must list apply_fragment: {text}"
        );
        // Full mode may recommend core for coding agents (#2070); still must
        // not claim the server is currently in core mode as its only mode.
        assert!(
            text.contains("Canonical names") && text.contains("search_files"),
            "full handshake must include name map (#2070)"
        );
        assert!(
            text.contains("Explore vs shell") || text.contains("search_files/read_file"),
            "full handshake must include explore guidance (#2070)"
        );
    }

    /// Protocol: peer_info.instructions equals `server_instructions(Core)` (no drift).
    #[tokio::test]
    async fn core_surface_handshake_instructions_are_core_only() {
        let dir = tempfile::TempDir::new().unwrap();
        let client =
            spawn_test_client_with_surface(dir.path().to_path_buf(), surface::McpSurface::Core)
                .await;
        let info = client.peer_info().expect("peer info should be set");
        let instructions = info
            .instructions
            .as_deref()
            .expect("server should have instructions");
        let expected = super::super::transport::server_instructions(surface::McpSurface::Core);
        assert_eq!(
            instructions, expected,
            "handshake instructions must match server_instructions(Core) exactly"
        );
        assert!(
            !instructions.contains("create_file"),
            "core handshake must not list create_file"
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
            text.contains("no matches for") && text.contains("NONEXISTENT_PATTERN_xyz"),
            "should name the missing pattern, got: {text}"
        );
        assert!(
            text.contains("no_matches"),
            "should set error_kind no_matches, got: {text}"
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
                // MCP descriptions strip plan `"op"` (unknown field on tools).
                assert!(
                    !desc.contains("\"op\""),
                    "{}: MCP description must not teach forbidden op field",
                    tool.tool_name
                );
                // Still include example payload fields from the plan example.
                if let Ok(serde_json::Value::Object(map)) =
                    serde_json::from_str::<serde_json::Value>(example)
                {
                    for (k, v) in map.iter().filter(|(k, _)| *k != "op") {
                        let needle = format!("\"{k}\":");
                        assert!(
                            desc.contains(&needle),
                            "{}: description missing example field {k} (from {example}): {desc}",
                            tool.tool_name
                        );
                        if let Some(s) = v.as_str() {
                            assert!(
                                desc.contains(s)
                                    || desc.contains(&serde_json::to_string(v).unwrap()),
                                "{}: description missing example value for {k}",
                                tool.tool_name
                            );
                        }
                    }
                }
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

// --- #2540: MCP honors .patchloom.toml [exclude] globs on walkers ---

fn tool_result_text(result: &rmcp::model::CallToolResult) -> String {
    match result.content.first() {
        Some(rmcp::model::ContentBlock::Text(t)) => t.text.clone(),
        _ => panic!("expected text content"),
    }
}

fn tool_result_json(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    let text = tool_result_text(result);
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw_text": text }))
}

async fn call_named_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    args: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let params = rmcp::model::CallToolRequestParams::new(name.to_string())
        .with_arguments(serde_json::from_value(args).unwrap());
    client.peer().call_tool(params).await.unwrap()
}

mod config_exclude_tests {
    use super::*;

    fn write_exclude_fixture(dir: &tempfile::TempDir) {
        std::fs::write(
            dir.path().join(".patchloom.toml"),
            "[exclude]\nglobs = [\"*.log\"]\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("a.txt"), "needle\n").unwrap();
        std::fs::write(dir.path().join("a.log"), "needle\n").unwrap();
        std::fs::write(dir.path().join("skip.tmp"), "needle\n").unwrap();
    }

    #[tokio::test]
    async fn search_files_honors_config_exclude_globs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_exclude_fixture(&dir);
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let result = call_named_tool(
            &client,
            "search_files",
            serde_json::json!({
                "pattern": "needle",
                "literal": true
            }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "search should succeed: {}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        let blob = val.to_string();
        assert!(
            blob.contains("a.txt"),
            "config exclude must keep a.txt: {val}"
        );
        assert!(
            !blob.contains("a.log"),
            "config exclude *.log must drop a.log: {val}"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn search_files_merges_request_exclude_patterns_with_config() {
        let dir = tempfile::TempDir::new().unwrap();
        write_exclude_fixture(&dir);
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let result = call_named_tool(
            &client,
            "search_files",
            serde_json::json!({
                "pattern": "needle",
                "literal": true,
                "exclude_patterns": ["*.tmp"]
            }),
        )
        .await;
        let val = tool_result_json(&result);
        let blob = val.to_string();
        assert!(blob.contains("a.txt"), "must keep a.txt: {val}");
        assert!(
            !blob.contains("a.log"),
            "config *.log must still apply: {val}"
        );
        assert!(
            !blob.contains("skip.tmp"),
            "request exclude_patterns *.tmp must still apply: {val}"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn list_files_honors_config_exclude_globs() {
        let dir = tempfile::TempDir::new().unwrap();
        write_exclude_fixture(&dir);
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let result =
            call_named_tool(&client, "list_files", serde_json::json!({ "path": "." })).await;
        assert!(
            !result.is_error.unwrap_or(false),
            "list_files should succeed: {}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        let paths = val["paths"]
            .as_array()
            .expect("paths array")
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>();
        assert!(
            paths.iter().any(|p| p.ends_with("a.txt") || *p == "a.txt"),
            "list_files must keep a.txt: {val}"
        );
        assert!(
            !paths.iter().any(|p| p.ends_with("a.log") || *p == "a.log"),
            "list_files must drop a.log via config exclude: {val}"
        );
        client.cancel().await.unwrap();
    }
}

// --- #2541: MCP undo_list / undo_restore ---

mod undo_mcp_tests {
    use super::*;

    fn create_backup(dir: &std::path::Path, filename: &str, content: &str) -> String {
        let file = dir.join(filename);
        std::fs::write(&file, content).unwrap();
        let mut session = crate::backup::BackupSession::new(dir).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap().unwrap()
    }

    #[tokio::test]
    async fn undo_list_empty_is_no_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(&client, "undo_list", serde_json::json!({})).await;
        assert!(
            !result.is_error.unwrap_or(false),
            "empty list is an envelope, not a tool error: {}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], false);
        assert_eq!(val["error_kind"], "no_matches");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn undo_list_shows_session_after_backup() {
        let dir = tempfile::TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "a.txt", "orig");
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(&client, "undo_list", serde_json::json!({})).await;
        let val = tool_result_json(&result);
        let items = val["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "{val}");
        assert_eq!(items[0]["timestamp"], ts);
        assert_eq!(items[0]["file_count"], 1);
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn undo_restore_dry_run_does_not_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "a.txt", "orig");
        std::fs::write(dir.path().join("a.txt"), "changed").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "undo_restore",
            serde_json::json!({ "session": ts }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["applied"], false, "{val}");
        assert_eq!(val["error_kind"], "changes_detected");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "changed"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn undo_restore_apply_restores_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "a.txt", "orig");
        std::fs::write(dir.path().join("a.txt"), "changed").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "undo_restore",
            serde_json::json!({ "session": ts, "apply": true }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["applied"], true, "{val}");
        assert_eq!(val["ok"], true);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "orig"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn undo_restore_unknown_session_is_no_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "undo_restore",
            serde_json::json!({
                "session": "no-such-session",
                "apply": true
            }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "unknown session is an envelope, not a tool error: {}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], false, "{val}");
        assert_eq!(val["error_kind"], "no_matches", "{val}");
        assert_eq!(val["applied"], false, "{val}");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn undo_restore_unknown_path_is_no_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "a.txt", "orig");
        std::fs::write(dir.path().join("a.txt"), "changed").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "undo_restore",
            serde_json::json!({
                "session": ts,
                "apply": true,
                "path": ["missing.txt"]
            }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "unknown path is an envelope, not a tool error: {}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], false, "{val}");
        assert_eq!(val["error_kind"], "no_matches", "{val}");
        assert_eq!(val["applied"], false, "{val}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "changed"
        );
        client.cancel().await.unwrap();
    }
}

mod explain_plan_mcp_tests {
    use super::*;

    #[tokio::test]
    async fn explain_plan_inline_one_op_returns_description() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let plan =
            r#"{"version":1,"operations":[{"op":"file.create","path":"a.txt","content":"x"}]}"#;
        let result =
            call_named_tool(&client, "explain_plan", serde_json::json!({ "plan": plan })).await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], true, "{val}");
        assert_eq!(val["operation_count"], 1, "{val}");
        let desc = val["operations"][0]["description"].as_str().unwrap_or("");
        assert!(
            desc.contains("create") || desc.contains("a.txt"),
            "expected op description, got: {desc}"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn explain_plan_missing_path_is_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let params = rmcp::model::CallToolRequestParams::new("explain_plan").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "no-such-plan.json"
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        match result {
            Ok(ok) => {
                let val = tool_result_json(&ok);
                let kind = val["error_kind"].as_str().unwrap_or("");
                assert!(
                    kind == "not_found" || kind == "invalid_input" || ok.is_error.unwrap_or(false),
                    "missing path must peel not_found/invalid_params: {val}"
                );
            }
            Err(err) => {
                let text = err.to_string();
                assert!(
                    text.contains("no-such-plan")
                        || text.contains("not found")
                        || text.contains("invalid"),
                    "{text}"
                );
            }
        }
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn explain_plan_malformed_is_parse_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "explain_plan",
            serde_json::json!({ "plan": "not a plan {{" }),
        )
        .await;
        let val = tool_result_json(&result);
        assert_eq!(val["error_kind"], "parse_error", "{val}");
        client.cancel().await.unwrap();
    }
}

mod tidy_check_mcp_tests {
    use super::*;

    #[tokio::test]
    async fn tidy_check_reports_missing_final_newline_without_rewrite() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("dirty.txt");
        std::fs::write(&path, "no-nl").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "tidy_check",
            serde_json::json!({ "path": "dirty.txt" }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], false, "{val}");
        assert_eq!(val["error_kind"], "changes_detected", "{val}");
        let issues = val["issues"].as_array().expect("issues");
        assert!(
            issues.iter().any(|i| {
                i["issue"]
                    .as_str()
                    .is_some_and(|s| s.contains("missing final newline"))
            }),
            "{val}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "no-nl");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn tidy_check_clean_file_is_empty_issues() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("clean.txt"), "ok\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "tidy_check",
            serde_json::json!({ "path": "clean.txt" }),
        )
        .await;
        let val = tool_result_json(&result);
        assert_eq!(val["ok"], true, "{val}");
        let issues = val["issues"].as_array().expect("issues");
        assert!(issues.is_empty(), "{val}");
        client.cancel().await.unwrap();
    }
}

mod apply_patch_check_mcp_tests {
    use super::*;

    fn unified_diff() -> String {
        "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n".to_string()
    }

    #[tokio::test]
    async fn apply_patch_false_does_not_change_file() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "apply_patch",
            serde_json::json!({ "diff": unified_diff(), "apply": false }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        let val = tool_result_json(&result);
        assert_eq!(val["applied"], false, "{val}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "old\n"
        );
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn apply_patch_true_still_writes() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let result = call_named_tool(
            &client,
            "apply_patch",
            serde_json::json!({ "diff": unified_diff(), "apply": true }),
        )
        .await;
        assert!(
            !result.is_error.unwrap_or(false),
            "{}",
            tool_result_text(&result)
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "new\n"
        );
        client.cancel().await.unwrap();
    }
}
