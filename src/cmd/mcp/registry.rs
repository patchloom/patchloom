//! Auto-generated MCP tool registry mapping Operation variants to MCP tools.

use rmcp::model::{CallToolResult, ErrorData as McpError};

use super::{PatchloomService, validate_content_size, validate_json_depth, validate_param_size};
use crate::plan::Operation;

// ---------------------------------------------------------------------------
// Auto-generated tool registry
// ---------------------------------------------------------------------------

/// Validation rule for a single field in an auto-generated MCP tool.
pub(super) enum FieldValidation {
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
pub(super) struct McpToolMeta {
    /// MCP tool name (e.g., `"doc_set"`).
    pub(super) tool_name: &'static str,
    /// `Operation` variant's serde tag value (e.g., `"doc.set"`).
    pub(super) op_name: &'static str,
    /// Tool description shown to MCP clients.
    pub(super) description: &'static str,
    /// Whether the tool accepts a `strict` parameter (plan-level, not on Operation).
    pub(super) has_strict: bool,
    /// Field-level validation rules applied before deserialization.
    pub(super) validations: &'static [FieldValidation],
}

/// Registry of simple MCP tools that map 1:1 to `Operation` variants.
///
/// Each entry auto-generates an MCP tool at startup: the input schema is derived
/// from the Operation variant via `operation_variant_schema()`, and the handler
/// deserializes the JSON args directly into an `Operation` (injecting the `op`
/// discriminator). This eliminates dedicated params structs and handler methods
/// for these tools.
pub(super) const MCP_TOOL_REGISTRY: &[McpToolMeta] = &[
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
        tool_name: "md_dedupe_headings",
        op_name: "md.dedupe_headings",
        description: "Remove duplicate markdown headings in a file. Keeps the first occurrence of each heading. Example: {\"path\": \"README.md\"}",
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
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
        tool_name: "prepend_file",
        op_name: "file.prepend",
        description: "Prepend content to the beginning of an existing file. Example: {\"path\": \"src/main.rs\", \"content\": \"// Copyright 2026\\n\"}",
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
pub(super) fn inject_strict_into_schema(mut schema: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = schema.as_object_mut()
        && let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut())
    {
        props.insert(
            "strict".to_string(),
            serde_json::json!({
                "type": "boolean",
                "description": "Enable strict mode (default true). When true, roll back all file writes if format or validate lifecycle steps fail.",
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
pub(super) fn handle_simple_op(
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
