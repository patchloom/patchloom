//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`

use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt, tool, tool_router};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cli::global::GlobalFlags;
use crate::containment::PathGuard;
use crate::exit;
use crate::plan::{Operation, Plan};

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

fn default_strict_true() -> bool {
    true
}

mod params;

#[cfg(feature = "ast")]
mod ast_tools;

// Re-export so the rest of the file (impls, tests) can use the names without qualification.
use params::*;

use params::ExecutePlanParams;

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Maximum size for content/diff fields (10 MiB).
const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum size for pattern/selector string fields (1 MiB).
const MAX_PARAM_BYTES: usize = 1024 * 1024;

/// Maximum number of files in a batch operation.
const MAX_BATCH_FILES: usize = 1000;

/// Maximum nesting depth for JSON value parameters.
const MAX_JSON_DEPTH: usize = 64;

fn validate_size(field: &str, value: &str, max_bytes: usize) -> Result<(), McpError> {
    if value.len() > max_bytes {
        return Err(McpError::invalid_params(
            format!(
                "{field} exceeds maximum size ({} bytes, limit {max_bytes})",
                value.len(),
            ),
            None,
        ));
    }
    Ok(())
}

fn validate_content_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_CONTENT_BYTES)
}

fn validate_param_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_PARAM_BYTES)
}

fn validate_json_depth(field: &str, value: &serde_json::Value) -> Result<(), McpError> {
    fn measure(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Array(arr) => 1 + arr.iter().map(measure).max().unwrap_or(0),
            serde_json::Value::Object(obj) => 1 + obj.values().map(measure).max().unwrap_or(0),
            _ => 0,
        }
    }
    let depth = measure(value);
    if depth > MAX_JSON_DEPTH {
        return Err(McpError::invalid_params(
            format!("{field} exceeds maximum nesting depth ({depth}, limit {MAX_JSON_DEPTH})"),
            None,
        ));
    }
    Ok(())
}

fn validate_batch_size(field: &str, count: usize) -> Result<(), McpError> {
    if count > MAX_BATCH_FILES {
        return Err(McpError::invalid_params(
            format!("{field} exceeds maximum batch size ({count}, limit {MAX_BATCH_FILES})"),
            None,
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Path containment (delegated to crate::containment::PathGuard)
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
        for path in crate::plan::declared_paths(op) {
            guard
                .check_path(path)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        }
        if let Operation::PatchApply { diff, .. } = op {
            // Validate paths embedded in the unified diff text (special case
            // because paths live inside the diff payload, not as top-level
            // declared fields).
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
// Auto-generated tool registry
// ---------------------------------------------------------------------------

/// Validation rule for a single field in an auto-generated MCP tool.
enum FieldValidation {
    /// Validate a string field with `check_path` (path containment).
    Path(&'static str),
    /// Validate a string field with `validate_param_size` (1 MiB limit).
    ParamSize(&'static str),
    /// Validate a string field with `validate_content_size` (10 MiB limit).
    ContentSize(&'static str),
    /// Validate a `serde_json::Value` field with `validate_json_depth` (64 levels).
    JsonDepth(&'static str),
}

/// Metadata for an auto-generated MCP tool that maps 1:1 to an `Operation` variant.
struct McpToolMeta {
    /// MCP tool name (e.g., `"doc_set"`).
    tool_name: &'static str,
    /// `Operation` variant's serde tag value (e.g., `"doc.set"`).
    op_name: &'static str,
    /// Tool description shown to MCP clients.
    description: &'static str,
    /// Whether the tool accepts a `strict` parameter (plan-level, not on Operation).
    has_strict: bool,
    /// Field-level validation rules applied before deserialization.
    validations: &'static [FieldValidation],
}

/// Registry of simple MCP tools that map 1:1 to `Operation` variants.
///
/// Each entry auto-generates an MCP tool at startup: the input schema is derived
/// from the Operation variant via `operation_variant_schema()`, and the handler
/// deserializes the JSON args directly into an `Operation` (injecting the `op`
/// discriminator). This eliminates dedicated params structs and handler methods
/// for these tools.
const MCP_TOOL_REGISTRY: &[McpToolMeta] = &[
    McpToolMeta {
        tool_name: "doc_set",
        op_name: "doc.set",
        description: "Set a value in a JSON, YAML, or TOML file. Parser-backed, preserves comments. Use dot notation for nested paths. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"package.json\", \"selector\": \"version\", \"value\": \"2.0.0\"}",
        has_strict: true,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_delete",
        op_name: "doc.delete",
        description: "Delete a value from a JSON, YAML, or TOML file. Example: {\"path\": \"package.json\", \"selector\": \"scripts.test\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_merge",
        op_name: "doc.merge",
        description: "Deep-merge an object into a JSON, YAML, or TOML document. Example: {\"path\": \"config.yaml\", \"value\": {\"server\": {\"port\": 8080}} }",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_append",
        op_name: "doc.append",
        description: "Append a value to an array in a JSON, YAML, or TOML file. Example: {\"path\": \"package.json\", \"selector\": \"dependencies\", \"value\": \"new-pkg\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_prepend",
        op_name: "doc.prepend",
        description: "Prepend a value to an array in a JSON, YAML, or TOML file. Inserts at position 0.",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_ensure",
        op_name: "doc.ensure",
        description: "Set a value in JSON/YAML/TOML only if it does not already exist. Idempotent: no-op if present. Example: {\"path\": \"config.json\", \"selector\": \"debug\", \"value\": false}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_delete_where",
        op_name: "doc.delete_where",
        description: "Remove array items matching a predicate from JSON/YAML/TOML. Example: {\"path\": \"config.yaml\", \"selector\": \"users\", \"predicate\": \"role=admin\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::ParamSize("predicate"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_update",
        op_name: "doc.update",
        description: "Update all items matching a wildcard selector in a JSON, YAML, or TOML file. Example: {\"path\": \"config.yaml\", \"selector\": \"servers[*].port\", \"value\": 8080}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_move",
        op_name: "doc.move",
        description: "Move/rename a key in a JSON, YAML, or TOML file. Example: {\"path\": \"config.json\", \"from\": \"old_name\", \"to\": \"new_name\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("from"),
            FieldValidation::ParamSize("to"),
        ],
    },
    McpToolMeta {
        tool_name: "read_file",
        op_name: "read",
        description: "Read file contents with optional line range. Example: {\"path\": \"src/main.rs\", \"lines\": \"1:50\"}",
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
    },
    McpToolMeta {
        tool_name: "md_upsert_bullet",
        op_name: "md.upsert_bullet",
        description: "Add a bullet under a markdown heading. Idempotent: skipped if already present. Example: {\"path\": \"CHANGELOG.md\", \"heading\": \"## Changes\", \"bullet\": \"- Added new feature\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("bullet"),
        ],
    },
    McpToolMeta {
        tool_name: "md_table_append",
        op_name: "md.table_append",
        description: "Append a row to a markdown table under a heading. Example: {\"path\": \"docs/api.md\", \"heading\": \"## Endpoints\", \"row\": \"| GET | /health | 200 |\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("row"),
        ],
    },
    McpToolMeta {
        tool_name: "md_replace_section",
        op_name: "md.replace_section",
        description: "Replace the content of a markdown section (from heading to next same-level heading). Example: {\"path\": \"README.md\", \"heading\": \"## Installation\", \"content\": \"Run `cargo install patchloom`.\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_insert_after_heading",
        op_name: "md.insert_after_heading",
        description: "Insert content immediately after a markdown heading (before existing section body). Example: {\"path\": \"AGENTS.md\", \"heading\": \"## Rules\", \"content\": \"Always run tests.\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_insert_before_heading",
        op_name: "md.insert_before_heading",
        description: "Insert content before a markdown heading. Example: {\"path\": \"AGENTS.md\", \"heading\": \"## Rules\", \"content\": \"## Preamble\\nRead this first.\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "move_file",
        op_name: "file.rename",
        description: "Move or rename a file. Use force=true to overwrite existing. Example: {\"from\": \"old.txt\", \"to\": \"new.txt\"}",
        has_strict: false,
        validations: &[FieldValidation::Path("from"), FieldValidation::Path("to")],
    },
    McpToolMeta {
        tool_name: "append_file",
        op_name: "file.append",
        description: "Append content to the end of an existing file. Example: {\"path\": \"log.txt\", \"content\": \"new entry\\n\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "create_file",
        op_name: "file.create",
        description: "Create a new file with content. Fails if file exists unless force=true. Example: {\"path\": \"src/new.rs\", \"content\": \"fn main() {}\"}",
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "delete_file",
        op_name: "file.delete",
        description: "Delete a file. Example: {\"path\": \"obsolete.txt\"}",
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
    },
];

/// Inject a `strict` boolean property (default true) into a JSON Schema object.
fn inject_strict_into_schema(mut schema: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = schema.as_object_mut()
        && let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut())
    {
        props.insert(
            "strict".to_string(),
            serde_json::json!({
                "type": "boolean",
                "description": "Enable strict mode (default true). When true, ambiguous matches cause an error instead of a best-effort resolution.",
                "default": true
            }),
        );
    }
    schema
}

/// Generic handler for auto-generated MCP tools.
///
/// Runs field validations from the registry entry, injects the `op` discriminator,
/// deserializes into an `Operation`, and executes via `run_one_op`.
fn handle_simple_op(
    service: &PatchloomService,
    meta: &McpToolMeta,
    mut args: serde_json::Value,
    allowed_fields: &std::collections::HashSet<String>,
) -> Result<CallToolResult, McpError> {
    let args_obj = args
        .as_object()
        .ok_or_else(|| McpError::invalid_params("expected JSON object", None))?;

    // Reject unknown fields (replaces deny_unknown_fields from hand-written params).
    let unknown: Vec<&str> = args_obj
        .keys()
        .filter(|k| !allowed_fields.contains(k.as_str()))
        .map(|k| k.as_str())
        .collect();
    if !unknown.is_empty() {
        return Err(McpError::invalid_params(
            format!("unknown field(s): {}", unknown.join(", ")),
            None,
        ));
    }

    // Run field validations.
    for v in meta.validations {
        match v {
            FieldValidation::Path(field) => {
                if let Some(val) = args_obj.get(*field).and_then(|v| v.as_str()) {
                    service.check_path(val)?;
                }
            }
            FieldValidation::ParamSize(field) => {
                if let Some(val) = args_obj.get(*field).and_then(|v| v.as_str()) {
                    validate_param_size(field, val)?;
                }
            }
            FieldValidation::ContentSize(field) => {
                if let Some(val) = args_obj.get(*field).and_then(|v| v.as_str()) {
                    validate_content_size(field, val)?;
                }
            }
            FieldValidation::JsonDepth(field) => {
                if let Some(val) = args_obj.get(*field) {
                    validate_json_depth(field, val)?;
                }
            }
        }
    }

    // Extract and remove `strict` before deserializing into Operation.
    let strict = if meta.has_strict {
        let s = args
            .as_object()
            .and_then(|o| o.get("strict"))
            .and_then(|v| v.as_bool());
        if let Some(obj) = args.as_object_mut() {
            obj.remove("strict");
        }
        s
    } else {
        None
    };

    // Inject the `op` discriminator for serde tagged-enum deserialization.
    if let Some(obj) = args.as_object_mut() {
        obj.insert(
            "op".to_string(),
            serde_json::Value::String(meta.op_name.to_string()),
        );
    }

    let op: Operation = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(format!("invalid parameters: {e}"), None))?;

    service.run_one_op(op, strict)
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
        let mut tool_router = Self::tool_router();

        // Register auto-generated tools from the registry. Each tool derives
        // its input schema from the corresponding Operation variant and uses
        // the generic `handle_simple_op` dispatcher.
        for meta in MCP_TOOL_REGISTRY {
            let mut schema = crate::schema::operation_variant_schema(meta.op_name);
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

            let input_schema: JsonObject = serde_json::from_value(schema)
                .expect("operation_variant_schema must produce a valid JSON object");

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
        for declared in crate::plan::declared_paths(op) {
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
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    } else {
        Ok(CallToolResult::error(vec![Content::text(msg)]))
    }
}

/// Execute a read-only doc operation directly (no subprocess).
fn doc_readonly(action: &crate::cmd::doc::DocAction) -> Result<CallToolResult, McpError> {
    let (output, code) =
        crate::cmd::doc::execute_with_mode(action, crate::cmd::doc::OutputMode::Json)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    exit_code_to_result(code, &output, "No results.")
}

fn make_plan_strict(operations: Vec<Operation>, strict: Option<bool>) -> Plan {
    Plan {
        version: crate::plan::SCHEMA_VERSION.to_string(),
        cwd: None,
        write_policy: None,
        strict,
        operations,
        format: None,
        validate: None,
    }
}

#[tool_router]
impl PatchloomService {
    #[tool(
        description = "Read a value from a JSON, YAML, or TOML file by selector. Example: {\"path\": \"package.json\", \"selector\": \"version\"}"
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
        description = "Query a JSON, YAML, or TOML file. Actions: \"has\" (check if selector exists, returns true/false), \"keys\" (list object keys at selector), \"len\" (count items at selector), \"select\" (filter array by predicate selector), \"flatten\" (list all leaf paths and values). Example: {\"action\": \"has\", \"path\": \"config.json\", \"selector\": \"database.host\"}"
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
        description = "Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for Bline parity: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results. Other options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".blineignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
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
            validate_param_size("pattern", &p.pattern)?;
            for path in &p.paths {
                svc.check_path(path)?;
            }
            // Validate custom ignore filenames too (new in #821 for layered ignores).
            // Treat them as paths relative to cwd for containment (even if just names like ".blineignore").
            for f in &p.custom_ignore_filenames {
                svc.check_path(f)?;
            }
            let search_args = crate::cmd::search::SearchArgs {
                pattern: p.pattern,
                paths: if p.paths.is_empty() {
                    vec![".".into()]
                } else {
                    p.paths
                },
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
                    "ok": true,
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
                return exit_code_to_result(exit::NO_MATCHES, "", "No matches found.");
            }

            let output = crate::cmd::search::format_results(results, &search_args, &global)
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
            exit_code_to_result(exit::SUCCESS, &output, "No results.")
        })
        .await
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with to=\"\" to delete lines). IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity. Example: {\"path\": \"README.md\", \"from\": \"1.0.0\", \"to\": \"2.0.0\"}"
    )]
    async fn replace_text(
        &self,
        Parameters(p): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            validate_param_size("from", &p.from)?;
            if let Some(ref to) = p.to {
                validate_content_size("to", to)?;
            }
            if let Some(ref ib) = p.insert_before {
                validate_content_size("insert_before", ib)?;
            }
            if let Some(ref ia) = p.insert_after {
                validate_content_size("insert_after", ia)?;
            }

            crate::ops::replace::validate_replace_args(
                &crate::ops::replace::ReplaceValidationParams {
                    pattern: &p.from,
                    has_to: p.to.is_some(),
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
            let validation_warnings = if !p.regex {
                let abs = svc.cwd().join(&p.path);
                if let Ok(content) = std::fs::read_to_string(&abs) {
                    let to_str = p.to.as_deref().unwrap_or("");
                    let result =
                        crate::fallback::validate_edit(&content, &p.from, to_str, Some(&p.path));
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

            let mode = if p.regex {
                Some("regex".to_string())
            } else {
                None
            };
            let replace_op = Operation::Replace {
                glob: None,
                path: Some(p.path),
                mode,
                from: p.from,
                to: p.to,
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
                tool_result.content.push(Content::text(warning_text));
            }

            Ok(tool_result)
        })
        .await
    }

    #[tool(
        description = "Move a markdown heading section to a new position (same file reorder or cross-file). Exactly one of before or after is required. Omit to for same-file reorder. Example: {\"path\": \"spec.md\", \"heading\": \"## Appendix\", \"to\": \"notes.md\", \"before\": \"## References\"}"
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
            Ok(CallToolResult::success(vec![Content::text(json)]))
        })
        .await
    }

    #[tool(
        description = "Apply a unified diff (patch). The diff parameter is the full unified diff text. Supports multi-file diffs. Use on_stale=merge for three-way merge on stale context; allow_conflicts=true writes conflict markers. Never commit files containing conflict markers. Example: {\"diff\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+new\", \"on_stale\": \"fail\"}"
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
        description = "Replace the same text across multiple files in one call. Atomic: all files succeed or none change. Example: {\"files\": [\"Cargo.toml\", \"README.md\"], \"from\": \"0.1.0\", \"to\": \"0.2.0\"}"
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
            validate_param_size("from", &p.from)?;
            validate_content_size("to", &p.to)?;
            for f in &p.files {
                svc.check_path(f)?;
            }
            let mode = if p.regex {
                Some("regex".to_string())
            } else {
                None
            };
            let ops: Vec<Operation> = p
                .files
                .into_iter()
                .map(|file| Operation::Replace {
                    glob: None,
                    path: Some(file),
                    mode: mode.clone(),
                    from: p.from.clone(),
                    to: Some(p.to.clone()),
                    nth: None,
                    insert_before: None,
                    insert_after: None,
                    case_insensitive: p.case_insensitive,
                    multiline: p.multiline,
                    if_exists: false,
                    whole_line: false,
                    range: None,
                    word_boundary: p.word_boundary,
                    before_context: None,
                    after_context: None,
                })
                .collect();
            svc.run_ops(ops, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Fix whitespace in multiple files in one call: trims trailing spaces and ensures final newline. Atomic: all files succeed or none change. Example: {\"files\": [\"src/main.rs\", \"src/lib.rs\"]}"
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
                })
                .collect();
            svc.run_ops(ops, Some(p.strict))
        })
        .await
    }

    #[tool(
        description = "Execute an arbitrary multi-step transaction plan atomically (MCP equivalent of `patchloom tx`). Provide either an inline 'plan' object or a 'plan_path' to a plan file. Supports mixed operations (doc.*, md.*, replace, file create/delete/rename, tidy, patch, etc). Strongly recommended for any multi-file or multi-op work to prevent races and ensure atomicity/rollback. See agent-rules --mode mcp or PATCHLOOM.md for plan schema examples."
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

            // Validate every path declared by operations against the PathGuard
            // (including special handling for paths embedded in PatchApply diffs).
            for op in &plan.operations {
                svc.validate_op_paths(op)?;
            }

            // The `strict` parameter from the MCP invocation always controls the execution
            // (it defaults to true). This provides a simple, predictable experience for agents.
            plan.strict = Some(p.strict);

            execute_plan_validated(plan, svc.cwd(), Some(&svc.path_guard))
        })
        .await
    }

    // doc_set, doc_delete, doc_merge, doc_append, doc_prepend, doc_ensure,
    // doc_delete_where, doc_update, doc_move, and read_file are auto-generated
    // from MCP_TOOL_REGISTRY (registered in PatchloomService::new).

    #[tool(
        description = "Fix whitespace in a file: trims trailing spaces and ensures final newline. Safe to call on any file (no-op if already clean). Example: {\"path\": \"dirty.txt\"}"
    )]
    async fn fix_whitespace(
        &self,
        Parameters(p): Parameters<TidyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| {
            svc.check_path(&p.path)?;
            svc.run_ops(
                vec![Operation::TidyFix {
                    path: p.path,
                    ensure_final_newline: Some(true),
                    trim_trailing_whitespace: Some(true),
                    normalize_eol: None,
                }],
                None,
            )
        })
        .await
    }

    // md_upsert_bullet, md_table_append, md_replace_section,
    // md_insert_after_heading, and md_insert_before_heading are auto-generated
    // from MCP_TOOL_REGISTRY (registered in PatchloomService::new).

    // -----------------------------------------------------------------
    // AST tools (feature-gated)
    // -----------------------------------------------------------------

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
    #[tool(
        description = "Rename identifiers across files using AST-aware renaming (skips strings and comments). Example: {\"old_name\": \"process_data\", \"new_name\": \"transform_data\", \"path\": \"src/\"}"
    )]
    async fn ast_rename(
        &self,
        Parameters(p): Parameters<AstRenameParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_rename(svc, p))
            .await
    }

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
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

    #[cfg(feature = "ast")]
    #[tool(
        description = "Replace text only within a specific symbol's body using AST scoping. Precise: only changes code inside the named symbol, leaving everything else untouched. Example: {\"path\": \"src/lib.rs\", \"symbol\": \"parse_config\", \"from\": \"unwrap()\", \"to\": \"expect(\\\"parse failed\\\")\"}"
    )]
    async fn ast_replace(
        &self,
        Parameters(p): Parameters<AstReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.blocking(move |svc| ast_tools::handle_ast_replace(svc, p))
            .await
    }

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
            Ok(CallToolResult::success(vec![Content::text(json)]))
        })
        .await
    }

    // move_file, append_file, create_file, and delete_file are auto-generated
    // from MCP_TOOL_REGISTRY (registered in PatchloomService::new).
}

impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use these tools for ALL file operations. Prefer 'execute_plan' (or tx plans) for any multi-op or multi-file work to ensure atomicity and avoid races from parallel calls on the same paths. Use batch_replace/batch_tidy only for uniform ops across files. Per-call success does not guarantee combined success if you issue conflicting parallel writes.",
            );
        info.server_info.name = "patchloom".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.clone();
        crate::verbose!("mcp: tool call -> {tool_name}");
        let start = std::time::Instant::now();
        let tc = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tc).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        crate::verbose!(
            "mcp: {tool_name} completed in {duration_ms}ms (ok={})",
            result.is_ok()
        );
        self.log_tool_call(&tool_name, duration_ms, &result);
        result
    }
}

/// Run the MCP server over Streamable HTTP (optionally with TLS).
#[cfg(feature = "mcp-http")]
pub(crate) fn run_mcp_http_server(
    global: &GlobalFlags,
    log: Option<String>,
    host: &str,
    port: u16,
    tls_cert: Option<&std::path::Path>,
    tls_key: Option<&std::path::Path>,
) -> anyhow::Result<u8> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
    use tokio_util::sync::CancellationToken;

    let cwd = global.resolve_cwd()?;
    let ct = CancellationToken::new();

    let mut config =
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());

    // When binding to non-loopback, allow any Host header
    if host != "127.0.0.1" && host != "::1" && host != "localhost" {
        config = config.disable_allowed_hosts();
    }

    let log_clone = log;
    let service = StreamableHttpService::new(
        move || {
            PatchloomService::new(cwd.clone(), log_clone.clone())
                .map_err(|e| std::io::Error::other(e.to_string()))
        },
        std::sync::Arc::new(LocalSessionManager::default()),
        config,
    );

    let app = axum::Router::new().nest_service("/mcp", service);
    let addr: std::net::SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid bind address: {e}"))?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| anyhow::anyhow!("TLS config error: {e}"))?;

            let handle = axum_server::Handle::new();
            let h = handle.clone();
            let ct2 = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ct2.cancel();
                h.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
            });

            // Print the banner once the server is actually bound so that
            // --port 0 shows the real ephemeral port (fixes #867).
            let h_addr = handle.clone();
            tokio::spawn(async move {
                if let Some(real_addr) = h_addr.listening().await {
                    eprintln!("MCP HTTPS server listening on https://{real_addr}/mcp");
                }
            });

            axum_server::bind_rustls(addr, tls_config)
                .handle(handle)
                .serve(app.into_make_service())
                .await
                .map_err(|e| anyhow::anyhow!("HTTPS server error: {e}"))?;
        } else {
            let ct2 = ct.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ct2.cancel();
            });

            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|e| anyhow::anyhow!("failed to bind {addr}: {e}"))?;
            eprintln!(
                "MCP HTTP server listening on http://{}/mcp",
                listener.local_addr()?
            );

            axum::serve(listener, app)
                .with_graceful_shutdown(ct.cancelled_owned())
                .await
                .map_err(|e| anyhow::anyhow!("HTTP server error: {e}"))?;
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::exit::SUCCESS)
}

/// Run the MCP server on stdio.
pub(crate) fn run_mcp_server(global: &GlobalFlags, log: Option<String>) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let service = PatchloomService::new(cwd, log)?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = service
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        server
            .waiting()
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::exit::SUCCESS)
}

#[cfg(test)]
mod tests;
