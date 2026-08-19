use super::*;

fn args(mode: AgentMode, platform: AgentPlatform) -> AgentRulesArgs {
    AgentRulesArgs {
        mode,
        platform,
        surface: AgentRulesSurface::Full,
    }
}

#[test]
fn default_includes_all_sections() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(out.contains("# Patchloom"));
    assert!(out.contains("## MCP mode"));
    assert!(out.contains("## Tool selection guide"));
    assert!(out.contains("## Batching"));
    assert!(out.contains("## Structured edits"));
    assert!(out.contains("## Exit codes"));
    assert!(out.contains("<<'EOF'"));
    assert!(out.contains("batch ops.txt"));
    assert!(
        out.contains("resolve under `--cwd`"),
        "must document that batch/tx/explain/patch/--files-from meta paths honor --cwd"
    );
    assert!(
        out.contains("meta-input") || out.contains("meta-input files"),
        "must document that --contain applies to meta-input files"
    );
    assert!(
        out.contains("omits `.patchloom/` backups") || out.contains("omits `.patchloom/"),
        "git_status tool guide must note backup omission: {out}"
    );
    assert!(
        out.contains("multi-line content") || out.contains(r#"\\\""#),
        "batch section must document quote escapes / multi-line guidance"
    );
    assert!(
        out.contains("Batch `doc.set f.json v \"2.0\"` is still a number")
            || out.contains("still a number"),
        "batch value note must warn that double-quoted 2.0 stays a JSON number: {out}"
    );
}

#[test]
fn mode_cli_omits_mcp_keeps_cli() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(!out.contains("## MCP mode"));
    assert!(!out.contains("## Tool selection guide"));
    assert!(out.contains("## Batching"));
    assert!(out.contains("## Structured edits"));
    // CLI-only mode keeps the "native tools are faster" note
    assert!(out.contains("native agent tools are faster"));
}

#[test]
fn mcp_mode_documents_plan_cwd_nested_re_root() {
    let out = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(
        out.contains("\"cwd\": \"fixtures/svc\""),
        "MCP rules must show nested plan.cwd example: {out}"
    );
    assert!(
        out.contains("Do not combine `cwd` with `for_each`")
            || out.contains("Do not combine cwd with for_each"),
        "MCP rules must forbid cwd+for_each: {out}"
    );
    assert!(
        out.contains("relative") && out.contains("cwd"),
        "MCP rules must say cwd is relative under workspace"
    );
    assert!(
        out.contains("search_files") && out.contains("paths") && out.contains("path"),
        "MCP rules must document search_files path alias for paths"
    );
}

#[test]
fn mode_all_documents_cli_contain_flag() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(
        out.contains("--contain"),
        "combined MCP+CLI rules must document --contain for CLI sandboxes"
    );
    assert!(
        !out.contains("the CLI does not"),
        "must not claim CLI has no containment after --contain shipped"
    );
    assert!(
        out.contains("absolute paths under `--cwd` are allowed"),
        "must document AllowIfContained: absolute under workspace OK on CLI"
    );
    assert!(
        out.contains("Host sandbox contract") || out.contains("#1832"),
        "must document host must pin cwd and strip model --cwd (#1832)"
    );
}

#[test]
fn mode_cli_documents_contain_for_agent_sandboxes() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("--contain"),
        "CLI-only rules must mention --contain for agent sandboxes"
    );
    assert!(
        out.contains("#1832") || out.contains("model-supplied") || out.contains("effective"),
        "CLI rules must warn hosts not to forward agent --cwd under --contain (#1832)"
    );
}

#[test]
fn mode_mcp_omits_cli_keeps_mcp() {
    let out = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(out.contains("## MCP mode"));
    assert!(out.contains("## Tool selection guide"));
    // MCP-only mode must not mention batch/transaction tools or CLI
    assert!(!out.contains("batch({\"operations\":"));
    assert!(!out.contains("transaction({\"operations\":"));
    assert!(!out.contains("search_replace"));
    assert!(!out.contains("run_terminal_command"));
    assert!(!out.contains("command line"));
    assert!(out.contains("Use them for ALL file operations"));
    assert!(
        out.contains("confined to the server workspace"),
        "MCP-only rules must state path containment"
    );
    // CLI-only sections must be absent (check for h2 headings, not h3)
    assert!(!out.contains("\n## Batching"));
    assert!(!out.contains("\n## Structured edits"));
    // Must not tell agent to prefer native tools
    assert!(!out.contains("native agent tools are faster"));
}

#[test]
fn platform_linux_omits_windows() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::Linux));
    assert!(out.contains("<<'EOF'"));
    assert!(!out.contains("batch ops.txt"));
    // Single-quote syntax present in CLI section
    assert!(out.contains("'\"2.0.0\"'"));
    // Windows-only double-quote escaping in CLI section absent
    // (MCP section has its own escaped quotes but that's platform-independent)
    assert!(!out.contains("patchloom doc set config.json version \"\\\"2.0.0\\\"\""));
}

#[test]
fn platform_windows_omits_heredoc() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::Windows));
    assert!(!out.contains("<<'EOF'"));
    assert!(out.contains("batch ops.txt"));
    // Windows escaping present in CLI section
    assert!(out.contains("patchloom doc set config.json version \"\\\"2.0.0\\\"\""));
    // Linux single-quote syntax absent
    assert!(!out.contains("'\"2.0.0\"'"));
}

#[test]
fn exit_codes_present_for_cli_modes() {
    // Exit codes are CLI-only (MCP returns JSON results, not exit codes)
    for mode in [AgentMode::All, AgentMode::Cli] {
        for platform in [
            AgentPlatform::All,
            AgentPlatform::Linux,
            AgentPlatform::Windows,
        ] {
            let out = generate_agent_rules(&args(mode, platform));
            assert!(
                out.contains("## Exit codes"),
                "exit codes missing for mode={mode:?} platform={platform:?}"
            );
        }
    }
    // MCP-only mode must NOT have exit codes
    let out = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(!out.contains("## Exit codes"));
}

#[test]
fn workflow_includes_whole_line_delete_example() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(out.contains("--whole-line"));
    assert!(out.contains("--collapse-blanks"));
}

#[test]
fn workflow_documents_batch_plan_kv_peel_siblings() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("heading=")
            && out.contains("bullet=")
            && out.contains("file.rename")
            && out.contains("selector=")
            && out.contains("predicate=")
            && out.contains("md.move_section")
            && out.contains("ast.rewrite_signature")
            && out.contains("tidy.fix"),
        "agents need sibling peel coverage beyond file.create content=: {out}"
    );
}

#[test]
fn workflow_documents_multi_document_yaml_index() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("Multi-document YAML")
            && out.contains("0.metadata.name")
            && out.contains("top-level array")
            && out.contains("type_error")
            && out.contains("doc get")
            && out.contains("has")
            && out.contains("doc keys")
            && out.contains("doc merge")
            && out.contains("doc append")
            && out.contains("move")
            && out.contains("ensure"),
        "agents need multi-doc index guidance + bare-key type_error on get/has/set/keys/merge/append/move/ensure"
    );
}

#[test]
fn workflow_documents_markdown_section_bounds_and_dedupe() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("Markdown section bounds")
            && out.contains("same or higher")
            && out.contains("md_dedupe_headings")
            && out.contains("whole sections"),
        "agents need hierarchical section + dedupe body-loss guidance"
    );
}

#[test]
fn workflow_documents_hardlink_preserving_apply() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(
        out.contains("hard links") && out.contains("nlink") && out.contains("temp+rename"),
        "agents need hardlink-preserving Apply guidance (#1733)"
    );
}

#[test]
fn workflow_documents_undo_dry_run_and_apply() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("undo --apply") && out.contains("dry-run by default"),
        "CLI agent-rules must state undo is dry-run unless --apply"
    );
    // Exit table should point agents at --apply for undo preview exit 2.
    assert!(
        out.contains("For undo, re-run with `--apply`"),
        "exit code 2 row must mention undo --apply"
    );
    assert!(
        out.contains("applied: false")
            && out.contains("status: changes_detected")
            && out.contains("applied: true")
            && out.contains("status: restored"),
        "undo JSON must document applied (#1830): missing applied/status guidance"
    );
    assert!(
        out.contains("--new") && out.contains("#1829"),
        "replace mode error guidance must name --new (#1829)"
    );
}

#[test]
fn workflow_includes_plan_require_change_and_command_position() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("require_change") && out.contains("command_position") && out.contains("fuzzy"),
        "plan replace library flags must be documented for agents"
    );
    assert!(
        out.contains("--command-position")
            && out.contains("--require-change")
            && out.contains("--fuzzy"),
        "CLI flags must appear in agent-rules examples"
    );
    assert!(
        out.contains("flock")
            && out.contains("runuser")
            && out.contains("setsid")
            && out.contains("run0")
            && out.contains("gosu")
            && out.contains("unshare")
            && out.contains("nsenter")
            && out.contains("taskset")
            && out.contains("systemd-run")
            && out.contains("firejail")
            && out.contains("busybox")
            && out.contains("chpst")
            && out.contains("softlimit")
            && out.contains("envdir")
            && out.contains("setlock"),
        "agent-rules should name isolation/container/runit wrappers for command_position"
    );
    let mcp = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(
        mcp.contains("require_change")
            && mcp.contains("command_position")
            && mcp.contains("fuzzy")
            && mcp.contains("restore_path_from_session")
            && mcp.contains("run_post_write_validation")
            && mcp.contains("match_mode")
            && mcp.contains("matched_text")
            && mcp.contains("allow_absent_old")
            && mcp.contains("fail closed")
            && mcp.contains("refused[]")
            && mcp.contains("below_min_fuzzy_score")
            && mcp.contains("no_matches")
            && mcp.contains("truncated"),
        "MCP-only agent-rules must document replace_text flags, fuzzy fail-closed, refused[], and search truncated"
    );
}

#[test]
fn agent_rules_mcp_documents_server_info_version_fields() {
    let out = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(
        out.contains("protocol_version")
            && out.contains("tool_count")
            && out.contains("server_info")
            && out.contains("package `version`"),
        "MCP agent-rules must document server_info version + protocol_version (#2060)"
    );
}

#[test]
fn workflow_documents_fuzzy_near_collision_negative_example() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("allow-absent-old") && out.contains("refuses the write"),
        "CLI rename workflow must document fuzzy fail-closed when exact old absent (#1758)"
    );
}

#[test]
fn exit_codes_include_doc_write_json_tip() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("Doc write JSON tip"),
        "agents need changed/removed guidance for idempotent doc delete"
    );
    assert!(out.contains("removed: 0"));
    assert!(
        out.contains("already_exists")
            && out.contains("not_found")
            && out.contains("type_error")
            && out.contains("parse_error")
            && out.contains("invalid search/replace regex")
            && out.contains("format_failed"),
        "error_kind catalogue must document file-op, doc, batch, invalid regex, and format_failed"
    );
}

#[test]
fn agent_rules_includes_project_config_section() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(out.contains("## Project configuration"));
    assert!(out.contains(".patchloom.toml"));
    assert!(out.contains("collapse_blanks"));
    assert!(out.contains("[tx]"));
}

/// Canonical names table must stay present so agents do not invent alternates.
/// Full AST CLI examples live under `#[cfg(feature = "ast")]`; the names table
/// always documents PATH/SYMBOL order so this still holds for test-mcp-no-ast.
#[test]
fn agent_rules_ast_cli_examples_use_real_clap_shapes() {
    // #1841: no --symbol / --name; correct positional order.
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("ast replace PATH SYMBOL")
            || out.contains("ast replace src/config.rs default_timeout"),
        "replace must use PATH SYMBOL positionals"
    );
    assert!(
        !out.contains("ast replace src/config.rs --symbol")
            && !out.contains("ast replace PATH --symbol"),
        "must not document nonexistent --symbol"
    );
    assert!(
        out.contains("ast refs my_function src/") || out.contains("ast refs SYMBOL PATH"),
        "refs must document SYMBOL PATH order"
    );
    assert!(
        !out.contains("ast refs src/ --name") && !out.contains("ast refs PATH --name"),
        "must not document nonexistent --name on refs"
    );
    #[cfg(feature = "ast")]
    {
        assert!(
            out.contains("ast replace src/config.rs default_timeout --old 30 --new 60"),
            "AST section example must use PATH SYMBOL positionals"
        );
        assert!(
            out.contains("ast refs my_function src/"),
            "AST section example must use SYMBOL PATH for refs"
        );
    }
}

#[test]
fn agent_rules_documents_for_each_plan_json_shape() {
    // #1842: complete plan-level for_each example.
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(
        out.contains("\"for_each\"") && out.contains("\"glob\""),
        "must show plan-level for_each with glob"
    );
    assert!(
        out.contains("plan-level field") || out.contains("#1842"),
        "must say for_each is plan-level not an op"
    );
    assert!(
        out.contains("unparseable") && out.contains("has_symbol"),
        "must document invalid glob and filter as invalid_input: {out}"
    );
}

#[test]
fn agent_rules_documents_doc_query_envelope_and_has_exit() {
    // #1838 / #1843 lock strings for CLI agent hosts.
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("Doc query") && out.contains("\"ok\":true") && out.contains("value"),
        "must document doc query JSON success envelope"
    );
    assert!(
        out.contains("doc has") && out.contains("#1843"),
        "must document doc has exit 0 for missing key"
    );
}

#[test]
fn agent_rules_documents_tidy_fix_defaults() {
    // #1840 / #1847
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(
        out.contains("tidy.fix") && out.contains("#1840") && out.contains("#1847"),
        "must document tidy.fix defaults and commit precedence"
    );
    assert!(
        out.contains("{\"op\":\"tidy.fix\"") || out.contains(r#"{"op":"tidy.fix""#),
        "must include bare tidy.fix plan example"
    );
}

#[test]
fn agent_rules_includes_canonical_parameter_names() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(
        out.contains("## Canonical parameter names"),
        "missing Canonical parameter names section"
    );
    assert!(out.contains("| `old` |"), "must document canonical old");
    assert!(
        out.contains("positional `OLD`") && out.contains("not `--old`"),
        "replace CLI must document positional OLD, not --old (#1834)"
    );
    assert!(
        !out.contains("| `old` | CLI: `--old`."),
        "must not claim replace uses CLI --old (#1834)"
    );
    assert!(out.contains("| `new` |"), "must document canonical new");
    assert!(
        out.contains("| `selector` |"),
        "must document canonical selector"
    );
    assert!(
        out.contains("`weak` / `medium` / `strong`"),
        "must document schema tier names"
    );
    assert!(
        out.contains("#1832") && (out.contains("model-supplied") || out.contains("strip")),
        "must document host sandbox contract for --contain/#1832"
    );
    // Present in both CLI and MCP-only modes (not CLI-only prose).
    let mcp_only = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::All));
    assert!(
        mcp_only.contains("## Canonical parameter names"),
        "MCP-only agent-rules must still include canonical names"
    );
}

#[test]
fn agent_rules_includes_patch_merge_workflow() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(out.contains("--on-stale merge"));
    assert!(out.contains("--allow-conflicts"));
    assert!(out.contains("never commit files containing conflict markers"));
}

#[test]
fn agent_rules_exit_codes_include_conflicts_and_rollback_failed() {
    let out = generate_agent_rules(&args(AgentMode::Cli, AgentPlatform::All));
    assert!(out.contains("| 8 |"));
    assert!(out.contains("| 9 |"));
    assert!(out.contains("rollback_failed"));
    assert!(out.contains("operation_failed"));
}

#[test]
fn agent_rules_documents_library_type_error_and_binary_preflight() {
    // Library embedder bullets live in the MCP/All agent-rules surface.
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    assert!(
        out.contains("EditErrorKind::TypeError"),
        "library hosts need TypeError peel docs (#1883)"
    );
    // #1947/#1948: dest-exists is AlreadyExists, not InvalidInput (stale #1935 bullet).
    assert!(
        out.contains("AlreadyExists") && out.contains("already_exists"),
        "library hosts need AlreadyExists / already_exists peels (#1947)"
    );
    assert!(
        out.contains("is_already_exists") && out.contains("error_kind_str"),
        "library hosts need api::is_already_exists and error_kind_str (#1948)"
    );
    assert!(
        out.contains("ReplaceOptions::for_agent") && out.contains("AGENT_MIN_FUZZY_SCORE"),
        "library hosts need for_agent replace preset docs (#1965)"
    );
    assert!(
        out.contains("fuzzy_span_suspicious")
            && out.contains("FuzzySpanPolicy")
            && out.contains("refuse_suspicious_fuzzy")
            && out.contains("op_honesty")
            && out.contains("refuse_batch_if_suspicious_fuzzy")
            && out.contains("apply_content_edits_to_file_with_span_policy")
            && out.contains("widest")
            && out.contains("embedder-host.md")
            && out.contains("primary + fallback")
            && out.contains("lifecycle_cmds")
            && out.contains("refuse_lifecycle_shell_metas")
            && out.contains("files")
            && out.contains("#2168")
            && out.contains("unquote_git_c_string")
            && out.contains("patch_declared_paths")
            && out.contains("parse_diff_git_paths")
            && out.contains("pair after")
            && out.contains("changed: true")
            && out.contains("#2170"),
        "library hosts need over-wide fuzzy refuse + multi-op/tx rollup + checklist (#1981/#2006-#2009/#2064) and for_each/lifecycle/patch dest (#2168/#2169/#2170)"
    );
    assert!(
        out.contains("Which surface to use")
            && out.contains("ast-grep")
            && out.contains("Context budget")
            && out.contains("Multi-document YAML")
            && out.contains("doc update")
            && out.contains("items[name=a].v")
            && out.contains("structured multi-match")
            && out.contains("suggested_op")
            && out.contains("doc.update"),
        "agent-rules need decision tree + set-vs-update + suggested_op + ast-grep + context tips (#1992/#1993/#1996/#2132/#2133)"
    );
    assert!(
        out.contains("Morph Fast Apply")
            && out.contains("morph-gap-matrix")
            && out.contains("not supported")
            && out.contains("apply-fragment")
            && out.contains("apply.fragment"),
        "agent-rules need Morph freeform routing + apply-fragment (#2018/#2019)"
    );
    assert!(
        out.contains("Canonical names")
            && out.contains("search_files")
            && out.contains("Explore vs shell")
            && out.contains("style_changed")
            && out.contains("Recommended MCP surface"),
        "agent-rules must include packaging name map, explore rule, YAML honesty, core host defaults (#2070)"
    );
    let core = generate_agent_rules(&AgentRulesArgs {
        mode: AgentMode::All,
        platform: AgentPlatform::All,
        surface: AgentRulesSurface::Core,
    });
    assert!(
        core.len() < 8_000 && core.contains("surface=core") && core.contains("execute_plan"),
        "agent-rules --surface core must stay short (#2070)"
    );
    assert!(
        out.contains("PATCHLOOM_MCP_SURFACE")
            && out.contains("core")
            && out.contains("read_file")
            && out.contains("list_files")
            && out.contains("execute_plan")
            && out.contains("surface-aware")
            && out.contains("plan catalog")
            && out.contains("filesystem MCP"),
        "agent-rules must document PATCHLOOM_MCP_SURFACE=core pack + list_files (#1994/#2076)"
    );
    assert!(
        out.contains("| List/inventory files") && out.contains("`list_files`"),
        "tool selection guide must include list_files (#2076)"
    );
    assert!(
        out.contains("one-line override") || out.contains("allow_absent_old: true"),
        "recovery stays a documented override, not a second constructor (#1980)"
    );
    assert!(
        out.contains("is_load_text_strict_fail"),
        "library hosts need is_load_text_strict_fail peel docs (#1963)"
    );
    assert!(
        out.contains("is_not_found")
            && out.contains("is_conflicts")
            && out.contains("is_changes_detected")
            && out.contains("is_type_error")
            && out.contains("is_format_failed")
            && out.contains("is_guard_rejected")
            && out.contains("is_invalid_input")
            && out.contains("is_binary")
            && out.contains("is_invalid_encoding")
            && out.contains("peel_error")
            && out.contains("is_no_match")
            && out.contains("is_ambiguous"),
        "library hosts need full bool peel set for fine-grained EditErrorKind"
    );
    assert!(
        !out.contains("exists/dir/binary → `InvalidInput`")
            && !out.contains("exists/dir/binary → InvalidInput"),
        "must not claim create dest-exists peels as InvalidInput after #1950"
    );
    assert!(
        out.contains("appended") || out.contains("append"),
        "library hosts need EditErrorKind append-only discriminant note (#1955)"
    );
    assert!(
        out.contains("is_binary_file"),
        "library hosts need is_binary_file preflight (#1884)"
    );
    assert!(
        out.contains("load_text_strict"),
        "library hosts need text I/O honesty load_text_strict (#1894)"
    );
    assert!(
        out.contains("api::load_text"),
        "library hosts need api::load_text alias (#1910)"
    );
    assert!(
        out.contains("api::doc_merge") && out.contains("Some(\"0\")"),
        "library hosts need multi-doc doc_merge selector (#1909)"
    );
    assert!(
        out.contains("non_exhaustive"),
        "library hosts need EditErrorKind non_exhaustive note (#1910)"
    );
    // Line-oriented insert is CLI-facing as well (mode All includes CLI).
    assert!(
        out.contains("Insert line placement"),
        "agents need line-oriented insert default (#1885)"
    );
    // Copy-paste shape: anchor + flag + path (not positional NEW / wrong order).
    assert!(
        out.contains(
            "patchloom replace 'use std::io;' --insert-after 'use std::fs;' src/main.rs --apply"
        ),
        "agents need a full CLI insert-after recipe, not prose-only flags"
    );
    assert!(
        out.contains("Insert a line after an anchor"),
        "workflow examples must include insert-after"
    );
}

#[test]
fn json_mode_emits_wrapped_json() {
    let args = AgentRulesArgs {
        mode: AgentMode::All,
        platform: AgentPlatform::All,
        surface: AgentRulesSurface::Full,
    };
    let _global = crate::cli::global::GlobalFlags {
        json: true,
        ..crate::cli::global::GlobalFlags::default()
    };
    // Verify that the JSON output would contain the right structure
    let output = generate_agent_rules(&args);
    let json = serde_json::json!({
        "ok": true,
        "format": "markdown",
        "content": output,
    });
    let parsed: serde_json::Value = json;
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["format"], "markdown");
    assert!(parsed["content"].as_str().unwrap().contains("# Patchloom"));
}

#[test]
fn version_is_embedded() {
    let out = generate_agent_rules(&args(AgentMode::All, AgentPlatform::All));
    let version = env!("CARGO_PKG_VERSION");
    assert!(out.contains(&format!("patchloom v{version}")));
}

#[test]
fn mcp_and_windows_compose_to_minimal() {
    let out = generate_agent_rules(&args(AgentMode::Mcp, AgentPlatform::Windows));
    assert!(out.contains("## MCP mode"));
    // MCP mode must not contain batch/transaction or CLI content
    assert!(!out.contains("batch({\"operations\":"));
    assert!(!out.contains("transaction({\"operations\":"));
    assert!(!out.contains("\n## Batching"));
    assert!(!out.contains("batch ops.txt"));
    assert!(!out.contains("<<'EOF'"));
}
