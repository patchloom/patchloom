//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`
//!
//! ## Module layout
//!
//! - [`validation`] - Resource limits and input validation helpers.
//! - [`registry`]   - Auto-generated tool registry (1:1 Operation mappings).
//! - [`handlers`]   - Hand-written `#[tool]` handler implementations.
//! - [`transport`]  - `ServerHandler` impl and server startup (stdio/HTTP).
//! - [`params`]     - Parameter structs for hand-written MCP tools.
//! - [`ast_tools`]  - AST MCP handler implementations (feature-gated).

use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData as McpError, JsonObject, Tool};
use std::path::PathBuf;
use std::sync::Arc;

use crate::containment::PathGuard;
use crate::exit;
use crate::plan::{Operation, Plan};

// ---------------------------------------------------------------------------
// Submodules
// ---------------------------------------------------------------------------

mod handlers;
mod params;
mod registry;
mod transport;
mod validation;

#[cfg(feature = "ast")]
mod ast_tools;

// Re-export validation helpers for use by sibling modules (params, handlers, registry).
use validation::*;

// Re-export param types so tests can use them via `super::*`.
#[cfg(test)]
use params::*;

// Re-export transport entry points for use by cmd/mod.rs.
#[cfg(feature = "mcp-http")]
pub(crate) use transport::run_mcp_http_server;
pub(crate) use transport::run_mcp_server;

use registry::{MCP_TOOL_REGISTRY, handle_simple_op, inject_strict_into_schema};

// ---------------------------------------------------------------------------
// Path containment (test-only helper)
// ---------------------------------------------------------------------------

/// Validate all paths in a list of operations.
/// Checks both syntactic containment and symlink resolution.
#[cfg(test)]
fn validate_operation_paths(
    operations: &[Operation],
    cwd: &std::path::Path,
) -> Result<(), McpError> {
    let guard = crate::containment::PathGuard::new(
        cwd.to_path_buf(),
        crate::containment::AbsolutePathPolicy::Reject,
    )
    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
    for op in operations {
        for path in op.declared_paths() {
            guard
                .check_path(path)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        }
        if let Operation::PatchApply { diff, .. } = op {
            let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
                McpError::invalid_params(
                    format!("failed to parse diff for path validation: {e}"),
                    None,
                )
            })?;
            for pf in &patch_files {
                guard
                    .check_path(&pf.path)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchloomService {
    tool_router: ToolRouter<Self>,
    /// Path guard: validates and canonicalizes paths relative to cwd.
    path_guard: crate::containment::PathGuard,
    /// Optional path for logging MCP tool calls as JSONL.
    /// Set via `--log <path>` or `PATCHLOOM_MCP_LOG` env var.
    call_log: Option<PathBuf>,
}

impl PatchloomService {
    pub fn new(cwd: PathBuf, log_flag: Option<String>) -> anyhow::Result<Self> {
        let path_guard = crate::containment::PathGuard::new(
            cwd.clone(),
            crate::containment::AbsolutePathPolicy::Reject,
        )
        .map_err(|e| anyhow::anyhow!("failed to initialize path guard: {e}"))?;
        // --log flag takes precedence over PATCHLOOM_MCP_LOG env var.
        let call_log = log_flag
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("PATCHLOOM_MCP_LOG").map(PathBuf::from));
        let mut tool_router = handlers::new_tool_router();

        // Register auto-generated tools from the registry. Each tool derives
        // its input schema from the corresponding Operation variant and uses
        // the generic `handle_simple_op` dispatcher.
        for meta in MCP_TOOL_REGISTRY {
            let mut schema = crate::schema::operation_variant_schema(meta.op_name)?;
            if meta.has_strict {
                schema = inject_strict_into_schema(schema);
            }
            // Collect the set of allowed field names from the schema properties
            // so handle_simple_op can reject unknown fields (deny_unknown_fields).
            let allowed_fields: Arc<std::collections::HashSet<String>> = Arc::new(
                schema
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .map(|props| props.keys().cloned().collect())
                    .unwrap_or_default(),
            );

            let input_schema: JsonObject = serde_json::from_value(schema).map_err(|e| {
                anyhow::anyhow!("operation_variant_schema must produce a valid JSON object: {e}")
            })?;

            tool_router.add_route(ToolRoute::new_dyn(
                Tool::new(meta.tool_name, meta.description, Arc::new(input_schema)),
                move |ctx: ToolCallContext<'_, PatchloomService>| {
                    let fields = Arc::clone(&allowed_fields);
                    let svc = ctx.service.clone();
                    let args = ctx.arguments.unwrap_or_default();
                    Box::pin(async move {
                        svc.blocking(move |svc| {
                            let args_value = serde_json::Value::Object(args);
                            handle_simple_op(svc, meta, args_value, &fields)
                        })
                        .await
                    })
                },
            ));
        }

        Ok(Self {
            tool_router,
            path_guard,
            call_log,
        })
    }

    /// Validate a path for both syntactic containment and symlink resolution.
    /// Combines the two checks that must always be called together.
    fn check_path(&self, path: &str) -> Result<(), McpError> {
        self.path_guard
            .check_path(path)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(())
    }

    /// The workspace root directory (non-canonicalized).
    fn cwd(&self) -> &std::path::Path {
        self.path_guard.root()
    }

    /// Run a synchronous closure on the blocking thread pool.
    ///
    /// All MCP handlers perform synchronous file I/O. Wrapping them in
    /// `spawn_blocking` prevents blocking the tokio async runtime, which
    /// matters for HTTP transport where concurrent requests would otherwise
    /// serialize on a single async task.
    async fn blocking<F, R>(&self, f: F) -> Result<R, McpError>
    where
        F: FnOnce(&PatchloomService) -> Result<R, McpError> + Send + 'static,
        R: Send + 'static,
    {
        let svc = self.clone();
        tokio::task::spawn_blocking(move || f(&svc))
            .await
            .map_err(|e| McpError::internal_error(format!("task join error: {e}"), None))?
    }

    /// Write a JSONL log entry for a tool call if logging is enabled.
    fn log_tool_call(
        &self,
        tool: &str,
        duration_ms: u64,
        result: &Result<CallToolResult, McpError>,
    ) {
        let Some(ref log_path) = self.call_log else {
            return;
        };
        let (ok, error) = match result {
            Ok(r) => (!r.is_error.unwrap_or(false), None),
            Err(e) => (false, Some(format!("{e}"))),
        };
        let ts = {
            let d = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            d.as_millis()
        };
        let mut entry = serde_json::json!({
            "ts": ts,
            "tool": tool,
            "duration_ms": duration_ms,
            "ok": ok,
        });
        if let Some(err_msg) = error {
            entry["error"] = serde_json::Value::String(err_msg);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{entry}");
        }
    }

    /// Helper to execute one or more operations as a plan.
    /// Reduces repetitive boilerplate across single-op and batch MCP tools.
    fn run_ops(
        &self,
        ops: Vec<Operation>,
        strict: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        execute_plan_validated(
            make_plan_strict(ops, strict),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    /// Helper for single-op case to reduce vec! boilerplate in handlers.
    fn run_one_op(&self, op: Operation, strict: Option<bool>) -> Result<CallToolResult, McpError> {
        self.run_ops(vec![op], strict)
    }

    /// Validate paths for a single operation (including embedded paths for PatchApply).
    fn validate_op_paths(&self, op: &Operation) -> Result<(), McpError> {
        for declared in op.declared_paths() {
            self.check_path(declared)?;
        }
        if let Operation::PatchApply { diff, .. } = op {
            let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
                McpError::invalid_params(
                    format!("failed to parse diff for path validation: {e}"),
                    None,
                )
            })?;
            for pf in &patch_files {
                self.check_path(&pf.path)?;
            }
        }
        Ok(())
    }
}

/// Execute a plan whose paths have already been validated by the caller.
///
/// Each individual MCP tool validates paths via `check_path` before calling
/// this function, so no redundant validation is needed here.
fn execute_plan_validated(
    plan: Plan,
    cwd: &std::path::Path,
    guard: Option<&PathGuard>,
) -> Result<CallToolResult, McpError> {
    let (code, json) = crate::cmd::tx::execute_plan_direct(plan, cwd, guard)
        .map_err(|e| McpError::internal_error(format!("plan execution failed: {e}"), None))?;

    exit_code_to_result(code, &json, "Operation completed successfully.")
}

/// Convert an exit code + output string into a `CallToolResult`.
///
/// When `output` is non-empty it is used as-is. Otherwise `fallback` is used
/// for both success and error results so callers can provide a descriptive
/// message regardless of exit code. If both are empty and the code indicates
/// failure, a generic "Operation failed with exit code N." message is returned.
fn exit_code_to_result(code: u8, output: &str, fallback: &str) -> Result<CallToolResult, McpError> {
    let msg = if output.trim().is_empty() {
        if fallback.is_empty() && code != exit::SUCCESS {
            format!("Operation failed with exit code {code}.")
        } else {
            fallback.to_string()
        }
    } else {
        output.trim().to_string()
    };
    if code == exit::SUCCESS {
        Ok(CallToolResult::success(vec![ContentBlock::text(msg)]))
    } else {
        Ok(CallToolResult::error(vec![ContentBlock::text(msg)]))
    }
}

/// Execute a read-only doc operation directly (no subprocess).
fn doc_readonly(action: &crate::cmd::doc::DocAction) -> Result<CallToolResult, McpError> {
    let (output, code) =
        crate::cmd::doc::execute_with_mode(action, crate::cmd::doc::OutputMode::Json)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    // CHANGES_DETECTED (2) is a valid success for doc diff (differences found).
    // NO_MATCHES (3) with non-empty output is a valid answer (e.g. "has"
    // returning "false", "len" returning "0"). Only treat it as an error
    // when the output is empty (genuinely no result).
    let effective = if code == exit::CHANGES_DETECTED
        || (code == exit::NO_MATCHES && !output.trim().is_empty())
    {
        exit::SUCCESS
    } else {
        code
    };
    exit_code_to_result(effective, &output, "No results.")
}

fn make_plan_strict(operations: Vec<Operation>, strict: Option<bool>) -> Plan {
    Plan {
        version: crate::plan::SCHEMA_VERSION,
        cwd: None,
        write_policy: None,
        strict,
        operations,
        format: None,
        validate: None,
        verify: None,
        for_each: None,
    }
}

#[cfg(test)]
mod tests;
