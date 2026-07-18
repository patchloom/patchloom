//! Auto-generated MCP tool registry mapping Operation variants to MCP tools.
//!
//! This is the **default** path for new write tools that map 1:1 to a plan
//! [`crate::plan::Operation`]. Tools that cannot use this path are listed in
//! [`super::surface::custom_mcp_tools`] (MCP surface honesty).

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
    /// Optional MCP-only guidance appended after the schema registry description
    /// (concurrency warnings, predicate syntax hints, etc.). Base prose and
    /// examples come from `schema::mcp_tool_description` (#1383).
    pub(super) extra: Option<&'static str>,
    /// Whether the tool accepts a `strict` parameter (plan-level, not on Operation).
    pub(super) has_strict: bool,
    /// Field-level validation rules applied before deserialization.
    pub(super) validations: &'static [FieldValidation],
}

impl McpToolMeta {
    /// Resolved tool description for MCP clients (schema base + optional extra + example).
    pub(super) fn description(&self) -> String {
        crate::schema::mcp_tool_description(self.op_name, self.extra)
    }
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("selector"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_merge",
        op_name: "doc.merge",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::JsonDepth("value"),
        ],
    },
    McpToolMeta {
        tool_name: "doc_append",
        op_name: "doc.append",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "For object arrays: predicate='role=admin'. For simple arrays: predicate='_=value'. Nested paths: predicate='settings.theme=dark'. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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
        extra: Some("Optional lines range uses start:end (1-based)."),
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
    },
    McpToolMeta {
        tool_name: "md_upsert_bullet",
        op_name: "md.upsert_bullet",
        extra: Some(
            "Idempotent: skipped if the bullet is already present. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("bullet"),
        ],
    },
    McpToolMeta {
        tool_name: "md_table_append",
        op_name: "md.table_append",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ParamSize("row"),
        ],
    },
    McpToolMeta {
        tool_name: "md_replace_section",
        op_name: "md.replace_section",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_insert_after_heading",
        op_name: "md.insert_after_heading",
        extra: Some(
            "Inserts immediately under the heading line (before existing body). For a sibling ## section after the full body, use md_insert_after_section. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_insert_after_section",
        op_name: "md.insert_after_section",
        extra: Some(
            "Inserts after the full section body (sibling placement). Prefer md_insert_after_heading for content under the heading. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_insert_before_heading",
        op_name: "md.insert_before_heading",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "md_dedupe_headings",
        op_name: "md.dedupe_headings",
        extra: Some(
            "Removes later whole sections with the same heading level+text (body under the second heading is discarded, not merged). IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
    },
    McpToolMeta {
        tool_name: "move_file",
        op_name: "file.rename",
        extra: Some(
            "Use force=true to overwrite an existing destination. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[FieldValidation::Path("from"), FieldValidation::Path("to")],
    },
    McpToolMeta {
        tool_name: "append_file",
        op_name: "file.append",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "prepend_file",
        op_name: "file.prepend",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "create_file",
        op_name: "file.create",
        extra: Some(
            "Fails if the file exists unless force=true. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[
            FieldValidation::Path("path"),
            FieldValidation::ContentSize("content"),
        ],
    },
    McpToolMeta {
        tool_name: "delete_file",
        op_name: "file.delete",
        extra: Some(
            "IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
        has_strict: false,
        validations: &[FieldValidation::Path("path")],
    },
    // MCP-friendly alias of tidy.fix with agent-oriented defaults (trim + final
    // newline on when omitted). Defaults applied in handle_simple_op.
    McpToolMeta {
        tool_name: "fix_whitespace",
        op_name: "tidy.fix",
        extra: Some(
            "Defaults: trim trailing whitespace and ensure final newline when those fields are omitted. IMPORTANT: do NOT issue concurrent calls targeting the same file; use execute_plan for multi-op atomicity.",
        ),
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

/// LLM-prior field aliases accepted by plan serde and hand-written MCP params.
///
/// Remap **before** the registry allowlist so schemars-derived schemas (which do
/// not expose `#[serde(alias)]`) still accept the same priors (#1696 / #1435).
/// Only applied when the **canonical** name is a property of this tool's schema
/// (so `doc.move`'s real `from`/`to` fields are not remapped to replace's old/new).
const FIELD_ALIASES: &[(&str, &str)] = &[
    ("key", "selector"),
    ("from", "old"),
    ("to", "new"),
    ("ops", "operations"),
];

/// Rename known alias keys to canonical names when the schema allows the canonical.
fn apply_field_aliases(
    args: &mut serde_json::Map<String, serde_json::Value>,
    allowed_fields: &std::collections::HashSet<String>,
) -> Result<(), McpError> {
    for &(alias, canonical) in FIELD_ALIASES {
        if !args.contains_key(alias) || !allowed_fields.contains(canonical) {
            continue;
        }
        if args.contains_key(canonical) {
            return Err(McpError::invalid_params(
                format!(
                    "conflicting fields {alias:?} and {canonical:?}; use canonical {canonical:?} only"
                ),
                None,
            ));
        }
        if let Some(val) = args.remove(alias) {
            args.insert(canonical.to_string(), val);
        }
    }
    Ok(())
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
        .as_object_mut()
        .ok_or_else(|| McpError::invalid_params("expected JSON object", None))?;

    // Remap LLM-prior aliases first so allowlist sees only canonical names (#1696).
    apply_field_aliases(args_obj, allowed_fields)?;

    // Reject unknown fields (replaces deny_unknown_fields from hand-written params).
    let unknown: Vec<String> = args_obj
        .keys()
        .filter(|k| !allowed_fields.contains(k.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        let hint = unknown
            .iter()
            .filter_map(|u| {
                FIELD_ALIASES
                    .iter()
                    .find(|(a, _)| *a == u.as_str())
                    .map(|(_, c)| format!("{u} (did you mean {c}?)"))
            })
            .collect::<Vec<_>>();
        let msg = if hint.is_empty() {
            format!("unknown field(s): {}", unknown.join(", "))
        } else {
            // Alias present but this op's schema has no canonical (should be rare).
            format!("unknown field(s): {}", hint.join(", "))
        };
        return Err(McpError::invalid_params(msg, None));
    }

    let args_obj = args
        .as_object()
        .ok_or_else(|| McpError::invalid_params("expected JSON object", None))?;

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

    let mut op: Operation = serde_json::from_value(args)
        .map_err(|e| McpError::invalid_params(format!("invalid parameters: {e}"), None))?;

    // Agent-oriented defaults for the fix_whitespace alias of tidy.fix.
    if meta.tool_name == "fix_whitespace"
        && let Operation::TidyFix {
            ensure_final_newline,
            trim_trailing_whitespace,
            ..
        } = &mut op
    {
        if ensure_final_newline.is_none() {
            *ensure_final_newline = Some(true);
        }
        if trim_trailing_whitespace.is_none() {
            *trim_trailing_whitespace = Some(true);
        }
    }

    service.run_one_op(op, strict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn field_aliases_remap_key_to_selector() {
        let allowed: HashSet<String> = ["path", "selector", "value", "strict"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut args = serde_json::json!({
            "path": "c.json",
            "key": "server.port",
            "value": 9,
        })
        .as_object()
        .unwrap()
        .clone();
        apply_field_aliases(&mut args, &allowed).unwrap();
        assert_eq!(
            args.get("selector").and_then(|v| v.as_str()),
            Some("server.port")
        );
        assert!(!args.contains_key("key"));
    }

    #[test]
    fn field_aliases_conflict_when_both_present() {
        // Only selector is in real schema; include both keys in the args map.
        let allowed: HashSet<String> = ["selector"].into_iter().map(str::to_string).collect();
        let mut args = serde_json::json!({
            "key": "a",
            "selector": "b",
        })
        .as_object()
        .unwrap()
        .clone();
        let err = apply_field_aliases(&mut args, &allowed).unwrap_err();
        assert!(err.to_string().contains("conflicting"), "err={err}");
    }

    #[test]
    fn field_aliases_skip_from_when_canonical_not_in_schema() {
        // doc.move has real `from`/`to` properties; do not remap to old/new.
        let allowed: HashSet<String> = ["path", "from", "to"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut args = serde_json::json!({
            "path": "c.json",
            "from": "a.b",
            "to": "c.d",
        })
        .as_object()
        .unwrap()
        .clone();
        apply_field_aliases(&mut args, &allowed).unwrap();
        assert_eq!(args.get("from").and_then(|v| v.as_str()), Some("a.b"));
        assert_eq!(args.get("to").and_then(|v| v.as_str()), Some("c.d"));
        assert!(!args.contains_key("old"));
        assert!(!args.contains_key("new"));
    }
}
