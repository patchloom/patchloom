//! Hand-written MCP tool handler implementations.
//!
//! Contains the `#[tool_router] impl PatchloomService` block with all
//! `#[tool(...)]` handler methods that require custom logic beyond the
//! auto-generated `MCP_TOOL_REGISTRY` dispatch.
//!
//! **Every tool in this module must appear in
//! [`super::surface::custom_mcp_tools`] inventory with a reason.** Prefer the registry
//! for new 1:1 `Operation` writes. See `surface` module docs for the policy.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData as McpError};
use rmcp::{tool, tool_router};

use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::Operation;

#[cfg(feature = "ast")]
use super::ast_tools;
use super::params::*;
use super::{
    PatchloomService, doc_readonly, execute_plan_validated, exit_code_to_result, no_results,
    validate_batch_size, validate_content_size, validate_param_size,
};

/// Validate operation paths when an optional `plan.cwd` re-root is active.
///
/// Relative declared paths are checked as `plan_cwd/path` so containment
/// matches how `execute_plan_direct` will resolve them. Absolute paths are
/// checked as-is (PathGuard still enforces the workspace root).
fn validate_op_paths_under_plan_cwd(
    svc: &PatchloomService,
    op: &Operation,
    plan_cwd: Option<&str>,
) -> Result<(), McpError> {
    let Some(prefix) = plan_cwd else {
        return svc.validate_op_paths(op);
    };
    let check = |path: &str| -> Result<(), McpError> {
        let candidate = if std::path::Path::new(path).is_absolute() {
            path.to_string()
        } else {
            format!(
                "{}/{}",
                prefix.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        };
        svc.check_path(&candidate)
    };
    for declared in op.declared_paths() {
        check(&declared)?;
    }
    if let Operation::PatchApply { diff, .. } = op {
        let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
            McpError::invalid_params(
                format!("failed to parse diff for path validation: {e}"),
                None,
            )
        })?;
        for pf in &patch_files {
            check(&pf.path)?;
        }
    }
    Ok(())
}

/// Create a new tool router with all hand-written `#[tool]` handlers registered.
///
/// This wraps the `#[tool_router]`-generated private `tool_router()` method
/// so it can be called from the parent module (`PatchloomService::new`).
pub(super) fn new_tool_router() -> ToolRouter<PatchloomService> {
    #[cfg(feature = "ast")]
    {
        let mut router = PatchloomService::tool_router();
        router.merge(PatchloomService::ast_tool_router());
        router
    }
    #[cfg(not(feature = "ast"))]
    {
        PatchloomService::tool_router()
    }
}

#[tool_router]
impl PatchloomService {
    #[tool(
        description = "Read a value from a JSON, YAML, or TOML file by selector path. Example: {\"path\": \"package.json\", \"selector\": \"version\"}"
    )]
    async fn doc_get(
        &self,
        Parameters(p): Parameters<DocGetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            validate_param_size("selector", &p.selector)?;
            let abs = svc.cwd().join(&p.path);
            let action = crate::cmd::doc::DocAction::Get {
                file: abs.to_string_lossy().into_owned(),
                selector: p.selector,
            };
            doc_readonly(&action)
        })
        .await
    }

    #[tool(
        description = "Query a JSON, YAML, or TOML file. Actions: \"has\" (check if selector exists, returns true/false), \"keys\" (list object keys at selector path), \"len\" (count items at selector path), \"select\" (filter array by predicate), \"flatten\" (list all leaf paths and values). Example: {\"action\": \"has\", \"path\": \"config.json\", \"selector\": \"database.host\"}"
    )]
    async fn doc_query(
        &self,
        Parameters(p): Parameters<DocQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            if let Some(ref sel) = p.selector {
                validate_param_size("selector", sel)?;
            }
            let abs = svc.cwd().join(&p.path);
            let file = abs.to_string_lossy().into_owned();
            let action = match p.action.as_str() {
                "has" => {
                    let selector = p.selector.ok_or_else(|| {
                        McpError::invalid_params(
                            "'has' action requires a selector".to_string(),
                            None,
                        )
                    })?;
                    crate::cmd::doc::DocAction::Has { file, selector }
                }
                "keys" => {
                    let selector = p.selector.ok_or_else(|| {
                        McpError::invalid_params(
                            "'keys' action requires a selector".to_string(),
                            None,
                        )
                    })?;
                    crate::cmd::doc::DocAction::Keys { file, selector }
                }
                "len" => {
                    let selector = p.selector.ok_or_else(|| {
                        McpError::invalid_params(
                            "'len' action requires a selector".to_string(),
                            None,
                        )
                    })?;
                    crate::cmd::doc::DocAction::Len { file, selector }
                }
                "select" => {
                    let selector = p.selector.ok_or_else(|| {
                        McpError::invalid_params(
                            "'select' action requires a selector".to_string(),
                            None,
                        )
                    })?;
                    crate::cmd::doc::DocAction::Select { file, selector }
                }
                "flatten" => crate::cmd::doc::DocAction::Flatten { file },
                other => {
                    return Err(McpError::invalid_params(
                        format!(
                            "unknown action '{other}'; valid actions: has, keys, len, select, flatten"
                        ),
                        None,
                    ));
                }
            };
            doc_readonly(&action)
        })
        .await
    }

    #[tool(
        description = "Compare two structured files (JSON, YAML, or TOML) and show differences. Example: {\"file_a\": \"old.json\", \"file_b\": \"new.json\"}"
    )]
    async fn doc_diff(
        &self,
        Parameters(p): Parameters<DocDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.file_a)?;
            svc.check_path(&p.file_b)?;
            let abs_a = svc.cwd().join(&p.file_a);
            let abs_b = svc.cwd().join(&p.file_b);
            let action = crate::cmd::doc::DocAction::Diff {
                file_a: abs_a.to_string_lossy().into_owned(),
                file_b: abs_b.to_string_lossy().into_owned(),
            };
            doc_readonly(&action)
        })
        .await
    }

    #[tool(
        description = "Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for Bline parity: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results. Other options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Canonical multi-root field is paths (array); singular path is accepted as an alias for one root (same as paths:[path]). Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".blineignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
    )]
    async fn search_files(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            if p.files_with_matches && p.count {
                return Err(McpError::invalid_params(
                    "files_with_matches and count cannot be combined",
                    None,
                ));
            }
            if p.invert_match && p.multiline {
                return Err(McpError::invalid_params(
                    "invert_match and multiline cannot be combined",
                    None,
                ));
            }
            if p.pattern.is_empty() {
                return Err(McpError::invalid_params("pattern must not be empty", None));
            }
            validate_param_size("pattern", &p.pattern)?;
            let paths = p.effective_paths();
            for path in &paths {
                svc.check_path(path)?;
            }
            // Validate custom ignore filenames too (new in #821 for layered ignores).
            // Treat them as paths relative to cwd for containment (even if just names like ".blineignore").
            for f in &p.custom_ignore_filenames {
                svc.check_path(f)?;
            }
            let search_args = crate::cmd::search::SearchArgs {
                pattern: p.pattern,
                paths,
                literal: p.literal,
                regex: !p.literal,
                context: p.context,
                before_context: p.before_context,
                after_context: p.after_context,
                files_with_matches: p.files_with_matches,
                count: p.count,
                invert_match: p.invert_match,
                multiline: p.multiline,
                case_insensitive: p.case_insensitive,
                assert_count: p.assert_count,
                max_results: p.max_results,
            };
            let mut global = GlobalFlags::with_cwd_and_json(svc.cwd());
            global.glob = p.globs;
            global.exclude = p.exclude_patterns;
            global.ignore_file = p.custom_ignore_filenames;
            let results = crate::cmd::search::collect_matches(&search_args, &global)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

            // --assert-count mode: return count comparison instead of matches.
            if let Some(expected) = p.assert_count {
                let actual: usize = results.file_match_counts.values().sum();
                let matched = actual == expected;
                let status = if matched {
                    "success"
                } else {
                    "changes_detected"
                };
                let code = if matched {
                    exit::SUCCESS
                } else {
                    exit::CHANGES_DETECTED
                };
                let output = serde_json::json!({
                    "ok": matched,
                    "status": status,
                    "assert_count": {
                        "expected": expected,
                        "actual": actual,
                        "matched": matched,
                    }
                });
                return exit_code_to_result(code, &output.to_string(), "");
            }

            let has_matches = if search_args.count || search_args.files_with_matches {
                !results.file_match_counts.is_empty()
            } else {
                results.has_matches()
            };
            if !has_matches {
                return no_results("No matches found.");
            }

            let output = crate::cmd::search::format_results(results, &search_args, &global)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            exit_code_to_result(exit::SUCCESS, &output, "No results.")
        })
        .await
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with new=\"\" to delete lines). IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"README.md\", \"old\": \"1.0.0\", \"new\": \"2.0.0\"}"
    )]
    async fn replace_text(
        &self,
        Parameters(p): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            validate_param_size("old", &p.old)?;
            if let Some(ref new_text) = p.new_text {
                validate_content_size("new", new_text)?;
            }
            if let Some(ref ib) = p.insert_before {
                validate_content_size("insert_before", ib)?;
            }
            if let Some(ref ia) = p.insert_after {
                validate_content_size("insert_after", ia)?;
            }
            if let Some(ref bc) = p.before_context {
                validate_content_size("before_context", bc)?;
            }
            if let Some(ref ac) = p.after_context {
                validate_content_size("after_context", ac)?;
            }

            crate::ops::replace::validate_replace_args(
                &crate::ops::replace::ReplaceValidationParams {
                    pattern: &p.old,
                    has_to: p.new_text.is_some(),
                    has_insert_before: p.insert_before.is_some(),
                    has_insert_after: p.insert_after.is_some(),
                    nth: p.nth,
                    whole_line: p.whole_line,
                    multiline: p.multiline,
                    has_range: p.range.is_some(),
                },
            )
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

            // Tier 2: pre-validate structured file edits and collect warnings.
            // Skip when case_insensitive or word_boundary is set: validate_edit_nth
            // uses literal `content.contains(from)` which is case-sensitive and
            // ignores word boundaries, producing false "pattern not found" errors.
            let validation_warnings = if !p.regex && !p.case_insensitive && !p.word_boundary {
                let abs = svc.cwd().join(&p.path);
                if let Ok(content) = std::fs::read_to_string(&abs) {
                    let to_str = p.new_text.as_deref().unwrap_or("");
                    let result = crate::fallback::validate_edit_nth(
                        &content,
                        &p.old,
                        to_str,
                        Some(&p.path),
                        p.nth,
                    );
                    let mut warnings = result.warnings;
                    if !result.valid {
                        warnings.extend(result.errors);
                    }
                    warnings
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            let replace_op = Operation::Replace {
                glob: None,
                path: Some(p.path),
                regex: p.regex,
                old: p.old,
                new_text: p.new_text,
                nth: p.nth,
                insert_before: p.insert_before,
                insert_after: p.insert_after,
                case_insensitive: p.case_insensitive,
                multiline: p.multiline,
                if_exists: p.if_exists,
                whole_line: p.whole_line,
                range: p.range,
                word_boundary: p.word_boundary,
                before_context: p.before_context,
                after_context: p.after_context,
                unique: p.unique,
            };
            let mut tool_result = svc.run_one_op(replace_op, Some(p.strict))?;

            // Append validation warnings to the response.
            if !validation_warnings.is_empty() {
                let warning_text = format!(
                    "\n\nWarnings:\n{}",
                    validation_warnings
                        .iter()
                        .map(|w| format!("  - {w}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                tool_result.content.push(ContentBlock::text(warning_text));
            }

            Ok(tool_result)
        })
        .await
    }

    #[tool(
        description = "Move a markdown heading section to a new position (same file reorder or cross-file). Exactly one of before or after is required. Omit to for same-file reorder. IMPORTANT: do NOT issue concurrent writes against the same file(s); use execute_plan for multi-op atomicity. Example: {\"path\": \"spec.md\", \"heading\": \"## Appendix\", \"to\": \"notes.md\", \"before\": \"## References\"}"
    )]
    async fn md_move_section(
        &self,
        Parameters(p): Parameters<MdMoveSectionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            if let Some(ref to) = p.to {
                svc.check_path(to)?;
            }
            if p.before.is_none() && p.after.is_none() {
                return Err(McpError::invalid_params(
                    "exactly one of 'before' or 'after' must be provided",
                    None,
                ));
            }
            if p.before.is_some() && p.after.is_some() {
                return Err(McpError::invalid_params(
                    "'before' and 'after' cannot both be set",
                    None,
                ));
            }
            svc.run_ops(
                vec![Operation::MdMoveSection {
                    path: p.path,
                    heading: p.heading,
                    to: p.to,
                    before: p.before,
                    after: p.after,
                }],
                None,
            )
        })
        .await
    }

    #[tool(
        description = "Lint a markdown rules file for duplicate headings, dangerous git commands, and missing final newline. Example: {\"path\": \"AGENTS.md\"}"
    )]
    async fn md_lint(
        &self,
        Parameters(p): Parameters<MdLintAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            let abs = svc.cwd().join(&p.path);
            let content = std::fs::read_to_string(&abs)
                .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
            let issues = crate::ops::md::lint_agents_content(&content);
            let json = serde_json::to_string_pretty(&issues)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Apply a unified diff (patch). The diff parameter is the full unified diff text. Supports multi-file diffs. Use on_stale=merge for three-way merge on stale context; allow_conflicts=true writes conflict markers. Never commit files containing conflict markers. IMPORTANT: do NOT issue concurrent patches/writes against the same files; use execute_plan for multi-op atomicity. Example: {\"diff\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+new\", \"on_stale\": \"fail\"}"
    )]
    async fn apply_patch(
        &self,
        Parameters(p): Parameters<PatchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            validate_content_size("diff", &p.diff)?;
            // Validate paths embedded in the diff.
            let patch_files = crate::ops::patch::parse_patch(&p.diff).map_err(|e| {
                McpError::invalid_params(format!("failed to parse diff: {e}"), None)
            })?;
            for pf in &patch_files {
                svc.check_path(&pf.path)?;
            }

            let op = Operation::PatchApply {
                diff: p.diff,
                on_stale: p.on_stale,
                allow_conflicts: p.allow_conflicts,
            };
            svc.run_one_op(op, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Replace the same text across multiple files in one call. Atomic: all files succeed or none change. IMPORTANT: do NOT issue concurrent write calls targeting the same files; use execute_plan for multi-op atomicity. Example: {\"files\": [\"Cargo.toml\", \"README.md\"], \"old\": \"0.1.0\", \"new\": \"0.2.0\"}"
    )]
    async fn batch_replace(
        &self,
        Parameters(p): Parameters<BatchReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            if p.files.is_empty() {
                return Err(McpError::invalid_params(
                    "files array must not be empty",
                    None,
                ));
            }
            validate_batch_size("files", p.files.len())?;
            validate_param_size("old", &p.old)?;
            validate_content_size("new", &p.new_text)?;
            for f in &p.files {
                svc.check_path(f)?;
            }
            let ops: Vec<Operation> = p
                .files
                .into_iter()
                .map(|file| Operation::Replace {
                    glob: None,
                    path: Some(file),
                    regex: p.regex,
                    old: p.old.clone(),
                    new_text: Some(p.new_text.clone()),
                    nth: None,
                    insert_before: None,
                    insert_after: None,
                    case_insensitive: p.case_insensitive,
                    multiline: p.multiline,
                    if_exists: p.if_exists,
                    whole_line: false,
                    range: None,
                    word_boundary: p.word_boundary,
                    before_context: None,
                    after_context: None,
                    unique: false,
                })
                .collect();
            svc.run_ops(ops, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Fix whitespace in multiple files in one call: trims trailing spaces and ensures final newline. Atomic: all files succeed or none change. IMPORTANT: do NOT issue concurrent write calls targeting the same files; use execute_plan for multi-op atomicity. Example: {\"files\": [\"src/main.rs\", \"src/lib.rs\"]}"
    )]
    async fn batch_tidy(
        &self,
        Parameters(p): Parameters<BatchTidyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            if p.files.is_empty() {
                return Err(McpError::invalid_params(
                    "files array must not be empty",
                    None,
                ));
            }
            validate_batch_size("files", p.files.len())?;
            for f in &p.files {
                svc.check_path(f)?;
            }
            let ops: Vec<Operation> = p
                .files
                .into_iter()
                .map(|file| Operation::TidyFix {
                    path: file,
                    ensure_final_newline: Some(true),
                    trim_trailing_whitespace: Some(true),
                    normalize_eol: None,
                    collapse_blanks: None,
                    dedent: None,
                    indent: None,
                    lines: None,
                })
                .collect();
            svc.run_ops(ops, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Execute an arbitrary multi-step transaction plan atomically (MCP equivalent of `patchloom tx`). Provide either an inline 'plan' object or a 'plan_path' to a plan file. Supports mixed operations (doc.*, md.*, replace, file create/delete/rename, tidy, patch, etc). Optional plan.cwd (relative path under the server workspace) re-roots relative op paths; absolute paths and ../ escapes are rejected. Do not set both plan.cwd and for_each. plan.format/validate lifecycle shell steps are ignored on MCP (use project config). Strongly recommended for multi-file or multi-op work. See agent-rules --mode mcp or PATCHLOOM.md for plan schema examples. Nested example: {\"plan\": {\"version\": 1, \"cwd\": \"fixtures/svc\", \"operations\": [{\"op\": \"doc.set\", \"path\": \"configs/app.yaml\", \"selector\": \"name\", \"value\": \"x\"}]}}"
    )]
    async fn execute_plan(
        &self,
        Parameters(p): Parameters<ExecutePlanParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            let mut plan = if let Some(inline_plan) = p.plan {
                inline_plan
            } else if let Some(path) = &p.plan_path {
                svc.check_path(path)?;
                let abs = svc.cwd().join(path);
                let content = std::fs::read_to_string(&abs).map_err(|e| {
                    McpError::internal_error(format!("failed to read plan_path: {e}"), None)
                })?;
                crate::plan::parse_plan_auto(&content, Some(path), None).map_err(|e| {
                    McpError::invalid_params(format!("failed to parse plan: {e}"), None)
                })?
            } else {
                return Err(McpError::invalid_params(
                    "either 'plan' (inline) or 'plan_path' must be provided",
                    None,
                ));
            };

            // Honor relative plan.cwd inside the MCP workspace. Reject escapes
            // and absolute path strings (MCP AbsolutePathPolicy::Reject) with a
            // hard error rather than silently stripping cwd (#1465). Lifecycle
            // shell steps remain stripped (format/validate); see #1142.
            // for_each expands globs against the server root; combining it with
            // plan.cwd would double-prefix paths, so reject the combination.
            if plan.cwd.is_some() && plan.for_each.is_some() {
                return Err(McpError::invalid_params(
                    "plan.cwd cannot be combined with for_each on MCP; \
                     omit cwd and use workspace-relative paths in for_each templates \
                     (e.g. path \"{path}\"), or omit for_each and set cwd for a nested re-root",
                    None,
                ));
            }

            let op_path_prefix = plan.cwd.clone();
            if let Some(ref plan_cwd) = op_path_prefix {
                if std::path::Path::new(plan_cwd).is_absolute() {
                    return Err(McpError::invalid_params(
                        format!(
                            "plan.cwd '{plan_cwd}' must be a relative path under the MCP workspace \
                             (absolute path strings are rejected on MCP)"
                        ),
                        None,
                    ));
                }
                svc.check_path(plan_cwd).map_err(|e| {
                    McpError::invalid_params(
                        format!(
                            "plan.cwd '{plan_cwd}' rejected (must resolve inside the MCP workspace): {e}"
                        ),
                        None,
                    )
                })?;
            }

            // Expand for_each (glob-driven batch) before path validation.
            // Globs resolve from the server root (cwd is mutually exclusive above).
            if plan.for_each.is_some() {
                crate::plan::expand_for_each(&mut plan, svc.cwd()).map_err(|e| {
                    McpError::invalid_params(format!("for_each expansion failed: {e}"), None)
                })?;
            }

            // Validate every path declared by operations against the PathGuard.
            // When plan.cwd is set, short op paths are relative to that re-root,
            // so check join(plan.cwd, path) (still under the workspace).
            for op in &plan.operations {
                validate_op_paths_under_plan_cwd(svc, op, op_path_prefix.as_deref())?;
            }

            // The `strict` parameter from the MCP invocation always controls the execution
            // (it defaults to true). This provides a simple, predictable experience for agents.
            plan.strict = Some(p.strict);

            // Strip lifecycle steps to prevent arbitrary command execution.
            // Format/validate commands run unrestricted shell processes,
            // bypassing workspace containment. These should only come from
            // project config (.patchloom.toml), not from LLM-submitted plans.
            plan.format = None;
            plan.validate = None;

            execute_plan_validated(plan, svc.cwd(), Some(&svc.path_guard))
        })
        .await
    }

    // doc_*, read_file, md section mutators, file_* mutators, and fix_whitespace
    // are auto-generated from MCP_TOOL_REGISTRY (registered in PatchloomService::new).

    #[tool(
        description = "Show uncommitted file changes vs git HEAD. Returns lists of modified, created, and deleted files. No parameters required."
    )]
    async fn git_status(
        &self,
        Parameters(_p): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            let global = GlobalFlags::with_cwd(svc.cwd());
            let status = crate::cmd::status::collect_status(&[], &global)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            let json = serde_json::to_string_pretty(&status)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Return the server's working directory. Use this to discover the root path before file operations. All path parameters in other tools are relative to this directory."
    )]
    async fn server_info(
        &self,
        Parameters(_p): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        let cwd = self.cwd().to_string_lossy().to_string();
        let info = serde_json::json!({ "cwd": cwd });
        let json = serde_json::to_string_pretty(&info)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    // move_file, append_file, create_file, and delete_file are auto-generated
    // from MCP_TOOL_REGISTRY (registered in PatchloomService::new).
}

// AST tools: separate tool_router so mcp builds without `ast` (closes #1396).
// The rmcp `#[tool_router]` macro does not honor `#[cfg]` on individual
// methods, so feature-gating must be at the impl / router-merge level.
#[cfg(feature = "ast")]
#[tool_router(router = ast_tool_router)]
impl PatchloomService {
    // -----------------------------------------------------------------
    // AST tools (feature-gated)
    // -----------------------------------------------------------------

    #[tool(
        description = "List symbol definitions (functions, classes, structs, enums, methods, etc.) in a file or directory. Supports 20 languages. Example: {\"path\": \"src/\"} or {\"path\": \"main.py\", \"kind\": \"function,class\"}"
    )]
    async fn ast_list(
        &self,
        Parameters(p): Parameters<AstListParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_list(svc, p))
            .await
    }

    #[tool(
        description = "Read a specific symbol's source code by name from a file. Uses AST parsing to find the exact definition. Example: {\"path\": \"src/main.rs\", \"symbol\": \"run\"}"
    )]
    async fn ast_read(
        &self,
        Parameters(p): Parameters<AstReadParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_read(svc, p))
            .await
    }

    #[tool(
        description = "Rename identifiers across files using AST-aware renaming (skips strings and comments). IMPORTANT: do NOT issue concurrent renames (or other writes) against the same file or directory tree; use execute_plan for multi-op atomicity (e.g. multiple renames). Example: {\"path\": \"src/\", \"old\": \"process_data\", \"new\": \"transform_data\"}"
    )]
    async fn ast_rename(
        &self,
        Parameters(p): Parameters<AstRenameParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_rename(svc, p))
            .await
    }

    #[tool(
        description = "Validate syntax of source files. Returns parse errors with line numbers. Supports 20 languages. Example: {\"path\": \"src/main.rs\"}"
    )]
    async fn ast_validate(
        &self,
        Parameters(p): Parameters<AstValidateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_validate(svc, p))
            .await
    }

    #[tool(
        description = "Structural search using AST queries. Use S-expression syntax or set pattern=true for code patterns with meta-variables ($VAR, $$$MULTI). Example: {\"query\": \"(function_item name: (identifier) @name)\", \"path\": \"src/\"}"
    )]
    async fn ast_search(
        &self,
        Parameters(p): Parameters<AstSearchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_search(svc, p))
            .await
    }

    #[tool(
        description = "Find all references to a symbol across files using AST analysis. Distinguishes definitions from references. Example: {\"symbol\": \"process_data\", \"path\": \"src/\"}"
    )]
    async fn ast_refs(
        &self,
        Parameters(p): Parameters<AstRefsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_refs(svc, p))
            .await
    }

    #[tool(
        description = "Extract import/dependency statements from source files. Supports Rust, Python, JS/TS, Go, Java, C/C++, Ruby, PHP. Use reverse=true to find what imports a file. Example: {\"path\": \"src/main.rs\"}"
    )]
    async fn ast_deps(
        &self,
        Parameters(p): Parameters<AstDepsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_deps(svc, p))
            .await
    }

    #[tool(
        description = "Generate a ranked repository map using PageRank over the symbol reference graph. Shows the most important symbols with token-budget-aware output. Example: {\"path\": \"src/\", \"max_tokens\": 2048}"
    )]
    async fn ast_map(
        &self,
        Parameters(p): Parameters<AstMapParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_map(svc, p))
            .await
    }

    #[tool(
        description = "Structural diff between two versions of a file. Shows added, removed, and modified symbols (not line-level diff). Compares against git refs. Example: {\"path\": \"src/lib.rs\", \"from\": \"HEAD~1\"}"
    )]
    async fn ast_diff(
        &self,
        Parameters(p): Parameters<AstDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_diff(svc, p))
            .await
    }

    #[tool(
        description = "Transitive impact analysis: what symbols are affected by changing a given symbol. Traces the reference graph to find all direct and indirect dependents. Example: {\"symbol\": \"parse_config\", \"path\": \"src/\", \"depth\": 3}"
    )]
    async fn ast_impact(
        &self,
        Parameters(p): Parameters<AstImpactParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_impact(svc, p))
            .await
    }

    #[tool(
        description = "Replace text only within a specific symbol's body using AST scoping. Precise: only changes code inside the named symbol, leaving everything else untouched. IMPORTANT: do NOT issue concurrent writes against the same file or directory tree; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/lib.rs\", \"symbol\": \"parse_config\", \"old\": \"unwrap()\", \"new\": \"expect(\\\"parse failed\\\")\"}"
    )]
    async fn ast_replace(
        &self,
        Parameters(p): Parameters<AstReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_replace(svc, p))
            .await
    }

    #[tool(
        description = "Rewrite a function signature with structured fields (visibility, parameters, return_type) or a full new_signature string. Multi-language via tree-sitter. IMPORTANT: do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/lib.rs\", \"old\": \"process\", \"parameters\": \"(x: i32)\", \"return_type\": \"-> String\"}"
    )]
    async fn ast_rewrite_signature(
        &self,
        Parameters(p): Parameters<AstRewriteSignatureParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_rewrite_signature(svc, p))
            .await
    }

    #[tool(
        description = "Insert code at a structurally-aware position: inside a module/impl/struct (at start or end), or after/before a named symbol. Indentation is auto-detected. IMPORTANT: do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/lib.rs\", \"content\": \"fn new_fn() {}\", \"after\": \"existing_fn\"}"
    )]
    async fn ast_insert(
        &self,
        Parameters(p): Parameters<AstInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_insert(svc, p))
            .await
    }

    #[tool(
        description = "Wrap existing code in a structural block (module, impl, cfg, etc.). Specify symbols by name or a line range. IMPORTANT: do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/lib.rs\", \"symbols\": [\"helper_fn\", \"HelperStruct\"], \"wrapper\": \"mod helpers\"}"
    )]
    async fn ast_wrap(
        &self,
        Parameters(p): Parameters<AstWrapParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_wrap(svc, p))
            .await
    }

    #[tool(
        description = "Manage import/use statements: add (idempotent), remove, deduplicate. With no mutation args, lists existing imports. IMPORTANT: when mutating, do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/main.rs\", \"add\": [\"use std::collections::HashMap;\"]}"
    )]
    async fn ast_imports(
        &self,
        Parameters(p): Parameters<AstImportsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_imports(svc, p))
            .await
    }

    #[tool(
        description = "Reorder symbols within a file or scope by name, kind, or custom order. IMPORTANT: do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/lib.rs\", \"order\": \"alphabetical\"} or {\"path\": \"src/lib.rs\", \"order\": [\"Struct\", \"impl Struct\", \"helper\"], \"inside\": \"mod tests\"}"
    )]
    async fn ast_reorder(
        &self,
        Parameters(p): Parameters<AstReorderParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_reorder(svc, p))
            .await
    }

    #[tool(
        description = "Group symbols into a named module within a file. Creates the module if it doesn't exist, or appends to it. IMPORTANT: do NOT issue concurrent writes against the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/tests.rs\", \"module\": \"line_endings\", \"symbols\": [\"test_crlf\", \"test_lf\"], \"preamble\": \"use super::*;\"}"
    )]
    async fn ast_group(
        &self,
        Parameters(p): Parameters<AstGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_group(svc, p))
            .await
    }

    #[tool(
        description = "Move symbols between files. Removes from source, inserts into target (creating it if needed). IMPORTANT: do NOT issue concurrent moves/writes against the same files; use execute_plan for multi-op atomicity. Example: {\"path\": \"src/big.rs\", \"target\": \"src/helpers.rs\", \"symbols\": [\"helper_fn\"], \"target_prepend\": \"use super::*;\"}"
    )]
    async fn ast_move(
        &self,
        Parameters(p): Parameters<AstMoveParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_move(svc, p))
            .await
    }

    #[tool(
        description = "Extract a symbol (module, function, struct) to a separate file. For modules with unwrap=true, content is un-indented. IMPORTANT: do NOT issue concurrent extracts/writes against the same files; use execute_plan for multi-op atomicity. Example: {\"source\": \"src/lib.rs\", \"symbol\": \"tests\", \"target\": \"src/lib_tests.rs\", \"replacement\": \"mod tests;\", \"prepend\": \"use super::*;\"}"
    )]
    async fn ast_extract_to_file(
        &self,
        Parameters(p): Parameters<AstExtractToFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_extract_to_file(svc, p))
            .await
    }

    #[tool(
        description = "Split a file into multiple target files by distributing symbols. Atomic: all targets succeed or all roll back. IMPORTANT: do NOT issue concurrent splits/writes against the same files; use execute_plan for multi-op atomicity. Example: {\"source\": \"src/big.rs\", \"targets\": [{\"path\": \"src/types.rs\", \"symbols\": [\"Config\", \"Mode\"], \"prepend\": \"use super::*;\"}], \"keep_in_source\": [\"main\"], \"source_suffix\": \"mod types;\"}"
    )]
    async fn ast_split(
        &self,
        Parameters(p): Parameters<AstSplitParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_split(svc, p))
            .await
    }
}
