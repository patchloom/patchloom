//! Hand-written MCP tool handler implementations.
//!
//! Contains the `#[tool_router] impl PatchloomService` block with all
//! `#[tool(...)]` handler methods that require custom logic beyond the
//! auto-generated `MCP_TOOL_REGISTRY` dispatch.
//!
//! **Every tool in this module must appear in
//! [`super::surface::custom_mcp_tools`] inventory with a reason.** Prefer the registry
//! for new 1:1 `Operation` writes. See `surface` module docs for the policy.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). Custom MCP tool
//! handlers co-located with surface inventory; do not split for LOC alone.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData as McpError};
use rmcp::{ServerHandler, tool, tool_router};

use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::Operation;

#[cfg(feature = "ast")]
use super::ast_tools;
use super::params::*;
use super::{
    PatchloomService, doc_readonly, execute_plan_validated, exit_code_to_result,
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
    let entry = op.uses_entry_containment();
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
        if entry {
            svc.check_path_entry(&candidate)
        } else {
            svc.check_path(&candidate)
        }
    };
    for declared in op.declared_paths() {
        check(&declared)?;
    }
    if let Operation::PatchApply { diff, .. } = op {
        if crate::ops::begin_patch::looks_like_begin_patch(diff)
            || crate::ops::search_replace::looks_like_search_replace(diff)
        {
            return Ok(());
        }
        let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
            McpError::invalid_params(
                format!("failed to parse diff for path validation: {e}"),
                None,
            )
        })?;
        for pf in &patch_files {
            if pf.uses_entry_containment() {
                // Path-only rename / empty-hunk delete: entry (#2115).
                let candidate = if std::path::Path::new(pf.path.as_str()).is_absolute() {
                    pf.path.clone()
                } else {
                    format!(
                        "{}/{}",
                        prefix.trim_end_matches('/'),
                        pf.path.trim_start_matches('/')
                    )
                };
                svc.check_path_entry(&candidate)?;
                if let Some(from) = &pf.rename_from {
                    let from_c = if std::path::Path::new(from.as_str()).is_absolute() {
                        from.clone()
                    } else {
                        format!(
                            "{}/{}",
                            prefix.trim_end_matches('/'),
                            from.trim_start_matches('/')
                        )
                    };
                    svc.check_path_entry(&from_c)?;
                }
            } else {
                check(&pf.path)?;
            }
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
        description = "Query a JSON, YAML, or TOML file. Actions: \"has\" (exists, true/false), \"keys\" (keys of one object; omit selector for `.`; e.g. database or items[0]), \"len\" (length of one object or array; omit selector for `.`; e.g. items or database), \"select\" (filter via selector predicates, e.g. users[role=admin]; no separate predicate field), \"flatten\" (leaf paths). keys need one object; len needs one object or array; items[*] is fail-closed ambiguous (use items[0] / items[1]). Example: {\"action\": \"has\", \"path\": \"config.json\", \"selector\": \"database.host\"}"
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
                    let selector = p.selector.unwrap_or_else(|| ".".to_string());
                    crate::cmd::doc::DocAction::Keys { file, selector }
                }
                "len" => {
                    let selector = p.selector.unwrap_or_else(|| ".".to_string());
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
        description = "Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for LLM agents: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .agentignore), max_results. Other options: files_with_matches, files_without_match, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Canonical multi-root field is paths (array); singular path is accepted as an alias for one root (same as paths:[path]). Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".agentignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
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
            if p.files_with_matches && p.files_without_match {
                return Err(McpError::invalid_params(
                    "files_with_matches and files_without_match cannot be combined",
                    None,
                ));
            }
            if p.files_without_match && p.count {
                return Err(McpError::invalid_params(
                    "files_without_match and count cannot be combined",
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
            if p.path.as_ref().is_some_and(|s| s.trim().is_empty()) {
                return Err(McpError::invalid_params(
                    "path must not be empty or whitespace-only (use paths for multi-root, or omit for workspace root)",
                    None,
                ));
            }
            validate_param_size("pattern", &p.pattern)?;
            let paths = p.effective_paths();
            for path in &paths {
                svc.check_path(path)?;
            }
            // Validate custom ignore filenames too (new in #821 for layered ignores).
            // Treat them as paths relative to cwd for containment (even if just names like ".agentignore").
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
                files_without_match: p.files_without_match,
                count: p.count,
                invert_match: p.invert_match,
                multiline: p.multiline,
                case_insensitive: p.case_insensitive,
                assert_count: p.assert_count,
                max_results: p.max_results,
                unique: false,
            };
            let mut global = GlobalFlags::with_cwd_and_json(svc.cwd());
            global.glob = p.globs;
            global.exclude = p.exclude_patterns;
            global.ignore_file = p.custom_ignore_filenames;
            let results = crate::cmd::search::collect_matches(&search_args, &global).map_err(
                |e| {
                    // Prefer invalid_params for typed agent failures (bad regex,
                    // invalid_input) so hosts do not treat them as server bugs.
                    let kind = crate::fallback::error_kind_str(&e).unwrap_or("invalid_input");
                    let msg = crate::exit::agent_error_message(&e);
                    if matches!(
                        kind,
                        "invalid_input"
                            | "parse_error"
                            | "guard_rejected"
                            | "not_found"
                            | "binary"
                            | "invalid_encoding"
                    ) {
                        McpError::invalid_params(msg, None)
                    } else {
                        McpError::internal_error(msg, None)
                    }
                },
            )?;

            let cwd = global
                .resolve_cwd()
                .map_err(|e| McpError::internal_error(format!("resolve cwd: {e}"), None))?;

            // Shared CLI honesty for empty scans (missing / sole binary /
            // unreadable-masked). Used by assert_count(actual=0) and no-match.
            let empty_scan_hard_fail = |global: &GlobalFlags,
                                       paths: &[String],
                                       cwd: &std::path::Path|
             -> Result<(), McpError> {
                match crate::files::all_scan_targets_missing(global, paths, Some(cwd)) {
                    Ok(true) => {
                        return Err(McpError::invalid_params(
                            format!(
                                "no such file or directory: {}",
                                global.path_scope_description(paths)
                            ),
                            None,
                        ));
                    }
                    Ok(false) => {}
                    Err(e) => {
                        return Err(McpError::invalid_params(
                            crate::exit::agent_error_message(&e),
                            None,
                        ));
                    }
                }
                if let Some(err) =
                    crate::ops::file::sole_explicit_non_text_for_scan(paths, None, cwd)
                {
                    let kind = crate::fallback::error_kind_str(&err).unwrap_or("invalid_input");
                    let msg = crate::exit::agent_error_message(&err);
                    return Err(McpError::invalid_params(format!("{kind}: {msg}"), None));
                }
                let scanned = crate::files::collect_file_paths_opts(paths, global, false, Some(cwd))
                    .map_err(|e| {
                        McpError::invalid_params(crate::exit::agent_error_message(&e), None)
                    })?;
                if let Some(err) = crate::ops::file::empty_scan_masked_by_unreadable(&scanned, cwd)
                {
                    return Err(McpError::invalid_params(err.msg, None));
                }
                Ok(())
            };

            // --assert-count mode: return count comparison instead of matches.
            if let Some(expected) = p.assert_count {
                let actual: usize = results.file_match_counts.values().sum();
                if actual == 0 {
                    empty_scan_hard_fail(&global, &search_args.paths, &cwd)?;
                }
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
                let mut output = serde_json::json!({
                    "ok": matched,
                    "status": status,
                    "assert_count": {
                        "expected": expected,
                        "actual": actual,
                        "matched": matched,
                    }
                });
                // Match CLI search --assert-count and tx: agents branch on kind.
                if !matched {
                    output["error_kind"] = serde_json::json!("changes_detected");
                }
                return exit_code_to_result(code, &output.to_string(), "");
            }

            let has_matches = if search_args.file_inventory_mode() {
                !results.file_match_counts.is_empty()
            } else {
                results.has_matches()
            };
            // cwd already resolved for empty-scan honesty.
            let refused =
                crate::cmd::search::explicit_binary_refused(&search_args, &global, &cwd);
            let skipped = crate::files::scan_missing_entries(&global, &cwd, &search_args.paths)
                .map_err(|e| {
                    McpError::invalid_params(crate::exit::agent_error_message(&e), None)
                })?;
            if !has_matches {
                empty_scan_hard_fail(&global, &search_args.paths, &cwd)?;
                // True pattern miss: include refused/skipped like CLI so agents
                // know binary co-paths were not searched as text.
                let path_desc = global.path_scope_description(&search_args.paths);
                let mut body = serde_json::json!({
                    "ok": false,
                    "error_kind": "no_matches",
                    "error": search_args.no_match_message(&path_desc),
                    "match_count": 0,
                    "file_count": 0,
                });
                if let Some(ref r) = refused
                    && !r.is_empty()
                {
                    body["refused"] = serde_json::to_value(r).unwrap_or_default();
                }
                if let Some(ref s) = skipped
                    && !s.is_empty()
                {
                    body["skipped"] = serde_json::to_value(s).unwrap_or_default();
                }
                return Ok(CallToolResult::success(vec![ContentBlock::text(
                    body.to_string(),
                )]));
            }

            let output = crate::cmd::search::format_results(
                results,
                &search_args,
                &global,
                skipped,
                refused,
            )
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            exit_code_to_result(exit::SUCCESS, &output, "No results.")
        })
        .await
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary, fuzzy, min_fuzzy_score, allow_absent_old. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with new=\"\" to delete lines). Fuzzy: when exact old is absent, refuse by default even if score ≥ min_fuzzy_score (#1758); set allow_absent_old=true only for deliberate approximate recovery. Prefer ast_rename for identifiers. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"README.md\", \"old\": \"1.0.0\", \"new\": \"2.0.0\"}. Insert after anchor (mutually exclusive with new): {\"path\": \"src/main.rs\", \"old\": \"use std::io;\", \"insert_after\": \"use std::fs;\"}"
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
            // Skip when case_insensitive, word_boundary, fuzzy, or context anchors
            // are set: validate_edit_nth uses literal `content.contains(from)`,
            // which is case-sensitive and exact-only. Fuzzy typo recovery and
            // context anchors would get false "pattern not found" warnings while
            // the engine still applies successfully (#1751).
            // Also skip insert_before/insert_after: validate_edit_nth treats a
            // missing `new` as delete (`to=""`), which false-flags structured
            // files (package.json anchors) while the real insert succeeds.
            let validation_warnings = if !p.regex
                && !p.case_insensitive
                && !p.word_boundary
                && !p.fuzzy
                && p.before_context.is_none()
                && p.after_context.is_none()
                && p.insert_before.is_none()
                && p.insert_after.is_none()
            {
                let abs = svc.cwd().join(&p.path);
                // Soft-or-preflight (#1894): skip non-text; engine still Strict.
                if let Some(content) = crate::files::read_text_file(&abs) {
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

            // Match honesty comes from engine TxOutput (replace_match_meta), not a
            // second replace_in_content pass. Re-deriving with range:None could
            // overwrite correct engine mode after ranged/fuzzy applies.
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
                require_change: p.require_change,
                command_position: p.command_position,
                fuzzy: p.fuzzy,
                min_fuzzy_score: p.min_fuzzy_score,
                allow_absent_old: p.allow_absent_old,
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
        description = "Lint a markdown rules file for duplicate headings, dangerous git commands, and missing final newline. Returns object envelope {ok, path, issue_count, issues} (CLI lint-agents --json parity; not a bare array). isError stays false when issues are present; branch on ok / issue_count. Example: {\"path\": \"AGENTS.md\"}"
    )]
    async fn md_lint(
        &self,
        Parameters(p): Parameters<MdLintAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            let abs = svc.cwd().join(&p.path);
            // Strict sole-path (#1894): binary / invalid UTF-8 → invalid_params.
            let content = crate::files::load_text_strict(&abs, &p.path).map_err(|e| {
                // load_text_strict messages already include path + OS detail;
                // do not prefix "reading {path}:" again (MPI 2026-07-23).
                if crate::exit::is_load_text_strict_fail(&e) || crate::exit::is_io_not_found(&e) {
                    McpError::invalid_params(e.to_string(), None)
                } else {
                    McpError::internal_error(e.to_string(), None)
                }
            })?;
            let issues = crate::ops::md::lint_agents_content(&content);
            // Envelope matches CLI `md lint-agents --json` (#1854 / #1859).
            // Always tool success (isError=false); agents branch on ok / issue_count.
            let mut envelope = serde_json::json!({
                "ok": issues.is_empty(),
                "path": p.path,
                "issue_count": issues.len(),
                "issues": issues,
            });
            // CLI md lint-agents --json sets error_kind when dirty; agents
            // branch the same way across surfaces.
            if !issues.is_empty() {
                envelope["error_kind"] = serde_json::json!("changes_detected");
            }
            let json = serde_json::to_string_pretty(&envelope)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Apply a unified diff, a Codex *** Begin Patch document, or an Aider SEARCH/REPLACE / DiffFenced document. The diff parameter is the full unified diff text, a *** Begin Patch ... *** End Patch envelope (Add/Update/Delete/Move), or <<<<<<< SEARCH / ======= / >>>>>>> REPLACE blocks (path on the first line after SEARCH). SEARCH/REPLACE is unique by default (multi-match is ambiguous, no write); set replace_all=true to update every exact match. Empty-hunk +++ /dev/null (git deleted file mode, no hunks) unlinks. A hunked delete applies minus lines first; leftover bytes rewrite the file (preview --diff). Stale minus lines are ambiguous and the file is not removed; regenerate minus lines or use file.delete for path-only unlink. Use on_stale=merge for three-way merge on stale unified-diff context; allow_conflicts=true writes conflict markers. Never commit files containing conflict markers. IMPORTANT: do NOT issue concurrent patches/writes against the same files; use execute_plan for multi-op atomicity. Example: {\"diff\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+new\", \"on_stale\": \"fail\"}"
    )]
    async fn apply_patch(
        &self,
        Parameters(p): Parameters<PatchParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            validate_content_size("diff", &p.diff)?;
            if crate::ops::begin_patch::looks_like_begin_patch(&p.diff) {
                if p.replace_all {
                    return Err(McpError::invalid_params(
                        crate::ops::search_replace::REPLACE_ALL_ONLY_FOR_SEARCH_REPLACE,
                        None,
                    ));
                }
                if crate::ops::search_replace::has_search_replace_marker(&p.diff) {
                    return Err(McpError::invalid_params(
                        "mixed Begin Patch and SEARCH/REPLACE grammar is not supported",
                        None,
                    ));
                }
                let ops = crate::ops::begin_patch::parse_begin_patch(&p.diff).map_err(|e| {
                    McpError::invalid_params(format!("failed to parse diff: {e}"), None)
                })?;
                for (path, entry) in crate::ops::begin_patch::begin_patch_containment_checks(&ops) {
                    if entry {
                        svc.check_path_entry(&path)?;
                    } else {
                        svc.check_path(&path)?;
                    }
                }
                let op = Operation::PatchApply {
                    diff: p.diff,
                    on_stale: p.on_stale,
                    allow_conflicts: p.allow_conflicts,
                    replace_all: false,
                };
                return svc.run_one_op(op, Some(p.strict));
            }
            if crate::ops::search_replace::looks_like_search_replace(&p.diff) {
                let paths = crate::ops::search_replace::search_replace_declared_paths(&p.diff)
                    .map_err(|e| {
                        McpError::invalid_params(format!("failed to parse diff: {e}"), None)
                    })?;
                for path in &paths {
                    svc.check_path(path)?;
                }
                let op = Operation::PatchApply {
                    diff: p.diff,
                    on_stale: p.on_stale,
                    allow_conflicts: p.allow_conflicts,
                    replace_all: p.replace_all,
                };
                return svc.run_one_op(op, Some(p.strict));
            }
            // Validate paths embedded in the diff.
            let patch_files = crate::ops::patch::parse_patch(&p.diff).map_err(|e| {
                McpError::invalid_params(format!("failed to parse diff: {e}"), None)
            })?;
            for pf in &patch_files {
                // Git rename and empty-hunk delete are path-only (entry
                // mode, #2115); content hunks and leftover hunked-delete
                // rewrites follow. Match execute_plan so a workspace link
                // → outside target can be unlinked, not rewritten.
                if pf.uses_entry_containment() {
                    svc.check_path_entry(&pf.path)?;
                    if let Some(from) = &pf.rename_from {
                        svc.check_path_entry(from)?;
                    }
                } else {
                    svc.check_path(&pf.path)?;
                }
            }

            if p.replace_all {
                return Err(McpError::invalid_params(
                    crate::ops::search_replace::REPLACE_ALL_ONLY_FOR_SEARCH_REPLACE,
                    None,
                ));
            }
            let op = Operation::PatchApply {
                diff: p.diff,
                on_stale: p.on_stale,
                allow_conflicts: p.allow_conflicts,
                replace_all: false,
            };
            svc.run_one_op(op, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Replace the same text across multiple files in one call. Engine staging is atomic for applied writes (all written files succeed or none change). Pattern misses are soft by default: matching files still apply and total misses appear in refused[]; set require_change=true to fail the whole batch if any file has no match. Canonical field is files (array); singular file is accepted as an alias for one path. Optional fuzzy enables similarity fallback; when exact old is absent, refuse by default unless allow_absent_old=true (#1758). JSON reports match_mode (exact/fuzzy/anchored), optional match_score, optional matched_text, match_count per change and aggregate (#1674). IMPORTANT: do NOT issue concurrent write calls targeting the same files; use execute_plan for multi-op atomicity. Example: {\"files\": [\"Cargo.toml\", \"README.md\"], \"old\": \"0.1.0\", \"new\": \"0.2.0\"}"
    )]
    async fn batch_replace(
        &self,
        Parameters(p): Parameters<BatchReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            let files = p.effective_files();
            if files.is_empty() {
                return Err(McpError::invalid_params(
                    "files array must not be empty (or pass singular file)",
                    None,
                ));
            }
            validate_batch_size("files", files.len())?;
            validate_param_size("old", &p.old)?;
            validate_content_size("new", &p.new_text)?;
            for f in &files {
                svc.check_path(f)?;
            }
            let ops: Vec<Operation> = files
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
                    require_change: p.require_change,
                    command_position: p.command_position,
                    fuzzy: p.fuzzy,
                    min_fuzzy_score: p.min_fuzzy_score,
                    allow_absent_old: p.allow_absent_old,
                })
                .collect();
            svc.run_ops(ops, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Fix whitespace in multiple files in one call: trims trailing spaces and ensures final newline. Atomic: all files succeed or none change. Canonical field is files (array); singular file is accepted as an alias for one path. IMPORTANT: do NOT issue concurrent write calls targeting the same files; use execute_plan for multi-op atomicity. Example: {\"files\": [\"src/main.rs\", \"src/lib.rs\"]}"
    )]
    async fn batch_tidy(
        &self,
        Parameters(p): Parameters<BatchTidyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            let files = p.effective_files();
            if files.is_empty() {
                return Err(McpError::invalid_params(
                    "files array must not be empty (or pass singular file)",
                    None,
                ));
            }
            validate_batch_size("files", files.len())?;
            for f in &files {
                svc.check_path(f)?;
            }
            let ops: Vec<Operation> = files
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
        description = "Execute an arbitrary multi-step transaction plan atomically (MCP equivalent of `patchloom tx`). Provide either an inline 'plan' object or a 'plan_path' to a plan file. Supports mixed operations (doc.*, md.*, replace, file create/delete/rename, tidy, patch, etc). Plan field for the op list is `operations` (alias `ops` accepted). Optional plan.cwd must be a relative path under the server workspace (re-roots relative op paths); absolute plan.cwd strings and ../ escapes are rejected. Op path fields may use absolute paths that resolve inside the workspace (AllowIfContained). Do not set both plan.cwd and for_each. plan.format/validate lifecycle shell steps are ignored on MCP (use project config). Strongly recommended for multi-file or multi-op work. See agent-rules --mode mcp or PATCHLOOM.md for plan schema examples. Nested example: {\"plan\": {\"version\": 1, \"cwd\": \"fixtures/svc\", \"operations\": [{\"op\": \"doc.set\", \"path\": \"configs/app.yaml\", \"selector\": \"name\", \"value\": \"x\"}]}}"
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
                // Strict sole-path plan load (#1894).
                // load_text_strict already prefixes "failed to read {path}";
                // do not re-wrap (same class as #1916).
                let content = crate::files::load_text_strict(&abs, path).map_err(|e| {
                    // Display already has path + OS detail (MPI 2026-07-23).
                    if crate::exit::is_load_text_strict_fail(&e) || crate::exit::is_io_not_found(&e) {
                        McpError::invalid_params(e.to_string(), None)
                    } else {
                        McpError::internal_error(e.to_string(), None)
                    }
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
                if plan_cwd.trim().is_empty() {
                    return Err(McpError::invalid_params(
                        "plan.cwd must not be empty or whitespace-only",
                        None,
                    ));
                }
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

            // Top-level `strict` overrides the plan only when the caller sent it.
            // Omitted leaves plan.strict unchanged so {"plan":{"strict":false}}
            // is not overwritten by a default true.
            apply_execute_plan_strict_override(&mut plan, p.strict);

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
        description = "List files under the workspace (or given roots) with the same ignore/exclude/glob rules as search. Use this instead of a generic filesystem MCP list_dir/tree. Caps results (default max_results=500) and reports truncated+total_matched when capped. max_depth prunes the walk at each root (does not enter deeper dirs). max_results still counts all in-depth matches then truncates (total_matched remains honest). Prefer relative paths. Example: {\"path\": \"src/\", \"exclude_patterns\": [\"target/**\"], \"max_results\": 100, \"max_depth\": 3}"
    )]
    async fn list_files(
        &self,
        Parameters(p): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            if p.path.as_ref().is_some_and(|s| s.trim().is_empty()) {
                return Err(McpError::invalid_params(
                    "path must not be empty or whitespace-only (use paths for multi-root, or omit for workspace root)",
                    None,
                ));
            }
            if p.max_depth == Some(0) {
                return Err(McpError::invalid_params(
                    "max_depth must be >= 1 when set (1 = files directly under each root)",
                    None,
                ));
            }
            let roots = p.effective_paths();
            for path in &roots {
                svc.check_path(path)?;
            }
            for f in &p.custom_ignore_filenames {
                svc.check_path(f)?;
            }
            // Same honesty as search: all-missing roots are typos, not empty inventory.
            if crate::files::all_explicit_paths_missing(&roots, Some(svc.cwd())) {
                return Err(McpError::invalid_params(
                    format!(
                        "no such file or directory: {}",
                        roots.join(", ")
                    ),
                    None,
                ));
            }
            let mut global = GlobalFlags::with_cwd_and_json(svc.cwd());
            global.glob = p.globs;
            global.exclude = p.exclude_patterns;
            global.ignore_file = p.custom_ignore_filenames;
            let report = super::list_files::collect_list_files(
                &roots,
                &global,
                svc.cwd(),
                p.max_results,
                p.max_depth,
                p.include_hidden,
            )
            .map_err(|e| {
                let kind = crate::fallback::error_kind_str(&e).unwrap_or("invalid_input");
                let msg = crate::exit::agent_error_message(&e);
                if matches!(
                    kind,
                    "invalid_input" | "guard_rejected" | "not_found" | "binary" | "invalid_encoding"
                ) {
                    McpError::invalid_params(msg, None)
                } else {
                    McpError::internal_error(msg, None)
                }
            })?;
            let json = serde_json::to_string_pretty(&report)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Show uncommitted file changes vs git HEAD. Returns lists of modified, created, and deleted files. Omits .patchloom/ backup paths from --apply undo sessions. No parameters required."
    )]
    async fn git_status(
        &self,
        Parameters(_p): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            let global = GlobalFlags::with_cwd(svc.cwd());
            let status = crate::cmd::status::collect_status(&[], &global).map_err(|e| {
                // Non-git workspace / invalid_input is agent input, not a server bug.
                if crate::exit::is_invalid_input(&e) {
                    McpError::invalid_params(format!("{e}"), None)
                } else {
                    McpError::internal_error(format!("{e}"), None)
                }
            })?;
            let json = serde_json::to_string_pretty(&status)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            // Always tool success (isError=false); agents branch on ok /
            // total_changes / error_kind like md_lint dirty envelopes.
            Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Return server identity and workspace root: cwd, surface (full|core), tool_count, package version, MCP protocol_version from handshake, and optional recommendation (coding agents may prefer core). Prefer relative path parameters under cwd; absolute paths are allowed only when they resolve inside the workspace (AllowIfContained). Outside-workspace and ../ escapes are rejected."
    )]
    async fn server_info(
        &self,
        Parameters(_p): Parameters<EmptyParams>,
    ) -> Result<CallToolResult, McpError> {
        let cwd = self.cwd().to_string_lossy().to_string();
        // Match ServerHandler::get_info so protocol_version cannot drift from
        // the initialize handshake (rmcp 3 defaults ProtocolVersion::LATEST).
        let handshake = ServerHandler::get_info(self);
        let surface = self.surface();
        let tool_count = surface.expected_tool_count();
        let mut info = serde_json::json!({
            "cwd": cwd,
            // Surface active at handshake (PATCHLOOM_MCP_SURFACE).
            "surface": surface.as_str(),
            "tool_count": tool_count,
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": handshake.protocol_version.to_string(),
        });
        // Full inventory is large: nudge coding agents toward core without
        // changing the product default (#2070 / #1994).
        if matches!(surface, super::surface::McpSurface::Full) {
            info["recommendation"] = serde_json::Value::String(
                crate::cmd::agent_packaging::server_info_full_recommendation(tool_count),
            );
        }
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
        description = "Structural search using AST queries. Use S-expression syntax or set pattern=true for code patterns with $VAR meta-variables (pattern must be valid source after substitution, e.g. fn $NAME() {}). Literal tokens match exactly. $$$MULTI is not implemented. Example: {\"query\": \"(function_item name: (identifier) @name)\", \"path\": \"src/\"}"
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
