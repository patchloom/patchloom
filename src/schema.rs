//! Intent format specification with versioning, tier filtering, and schema export.
//!
//! Provides a machine-readable registry of all patchloom operations, their
//! parameters (as JSON Schemas), capability tiers, and examples. External
//! consumers (AI coding agents) use this to build tool schemas and system
//! prompts programmatically.
//!
//! # Quick start
//!
//! ```rust
//! use patchloom::schema::{self, Tier};
//!
//! // Check compatibility
//! assert!(schema::is_compatible_version("1"));
//!
//! // Get operations for medium-tier models
//! let ops = schema::operations_for_tier(Tier::Medium);
//! assert!(!ops.is_empty());
//!
//! // Generate a system prompt fragment
//! let prompt = schema::system_prompt_for_tier(Tier::Medium);
//! assert!(prompt.contains("replace"));
//! ```

use serde::{Deserialize, Serialize};

/// The current intent format version. Consumers check this at startup to
/// detect breaking changes across patchloom releases.
pub const INTENT_FORMAT_VERSION: &str = "1";

/// Check whether a given version string is compatible with this release.
pub fn is_compatible_version(version: &str) -> bool {
    version == INTENT_FORMAT_VERSION
}

/// Model capability tier for operation filtering.
///
/// Operations are annotated with a minimum tier. Weaker models get fewer,
/// simpler operations; stronger models get the full set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Simple operations: replace_text, file create/delete, fix_whitespace.
    Weak,
    /// Individual structured operations: doc.set, md.replace_section, small batches.
    Medium,
    /// Full power: tx with validation, complex selectors, large batches, search.
    Strong,
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tier::Weak => write!(f, "weak"),
            Tier::Medium => write!(f, "medium"),
            Tier::Strong => write!(f, "strong"),
        }
    }
}

impl std::str::FromStr for Tier {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "weak" => Ok(Tier::Weak),
            "medium" => Ok(Tier::Medium),
            "strong" => Ok(Tier::Strong),
            _ => Err(format!(
                "unknown tier: {s} (expected weak, medium, or strong)"
            )),
        }
    }
}

/// A concrete example of invoking an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationExample {
    /// Human-readable description of what this example does.
    pub description: String,
    /// The operation arguments as a JSON object.
    pub args: serde_json::Value,
}

/// Schema for a single patchloom operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSchema {
    /// Operation name (e.g., "replace", "doc.set", "md.replace_section").
    pub name: String,
    /// Human-readable description of what this operation does.
    pub description: String,
    /// JSON Schema for the operation's parameters.
    pub parameters: serde_json::Value,
    /// Minimum capability tier required to use this operation.
    pub min_tier: Tier,
    /// Example invocations for system prompt inclusion.
    pub examples: Vec<OperationExample>,
}

/// Return the complete registry of all patchloom operations with schemas.
pub fn operation_schemas() -> Vec<OperationSchema> {
    vec![
        // --- Weak tier: simple, hard-to-misuse operations ---
        OperationSchema {
            name: "replace".into(),
            description: "Replace text in a file using literal string matching.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "from", "to"],
                "properties": {
                    "path": {"type": "string", "description": "File path (relative to working directory)."},
                    "from": {"type": "string", "description": "Text to find."},
                    "to": {"type": "string", "description": "Replacement text."},
                    "mode": {"type": "string", "enum": ["literal", "regex"], "default": "literal"},
                    "nth": {"type": "integer", "minimum": 1, "description": "Replace only the Nth match (1-based)."},
                    "case_insensitive": {"type": "boolean", "default": false},
                    "multiline": {"type": "boolean", "default": false},
                    "if_exists": {"type": "boolean", "default": false, "description": "Do not error if no matches found."},
                    "whole_line": {"type": "boolean", "default": false, "description": "Replace the entire line containing each match."},
                    "range": {"type": "string", "description": "Restrict matching to a line range (e.g. '10:50'). Requires whole_line."}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Replace a function name".into(),
                    args: serde_json::json!({"op": "replace", "path": "src/main.rs", "from": "old_name", "to": "new_name"}),
                },
            ],
        },
        OperationSchema {
            name: "file.append".into(),
            description: "Append content to an existing file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Append a test function to a test file".into(),
                    args: serde_json::json!({"op": "file.append", "path": "tests/test.rs", "content": "#[test]\nfn new_test() {}\n"}),
                },
            ],
        },
        OperationSchema {
            name: "file.create".into(),
            description: "Create a new file with specified content.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "force": {"type": "boolean", "default": false, "description": "Overwrite if file exists."}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Create a new config file".into(),
                    args: serde_json::json!({"op": "file.create", "path": "config.json", "content": "{\"version\": \"1.0\"}"}),
                },
            ],
        },
        OperationSchema {
            name: "file.delete".into(),
            description: "Delete a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Delete a temporary file".into(),
                    args: serde_json::json!({"op": "file.delete", "path": "tmp/scratch.txt"}),
                },
            ],
        },
        OperationSchema {
            name: "file.rename".into(),
            description: "Rename (move) a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["from", "to"],
                "properties": {
                    "from": {"type": "string", "description": "Source file path."},
                    "to": {"type": "string", "description": "Destination file path."},
                    "force": {"type": "boolean", "default": false}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Rename a file".into(),
                    args: serde_json::json!({"op": "file.rename", "from": "old.txt", "to": "new.txt"}),
                },
            ],
        },
        OperationSchema {
            name: "tidy.fix".into(),
            description: "Normalize whitespace, line endings, and final newline in a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "ensure_final_newline": {"type": "boolean"},
                    "trim_trailing_whitespace": {"type": "boolean"},
                    "normalize_eol": {"type": "string", "enum": ["lf", "crlf"]}
                }
            }),
            min_tier: Tier::Weak,
            examples: vec![
                OperationExample {
                    description: "Fix whitespace in a file".into(),
                    args: serde_json::json!({"op": "tidy.fix", "path": "src/main.rs", "ensure_final_newline": true, "trim_trailing_whitespace": true}),
                },
            ],
        },
        // --- Medium tier: structured operations requiring selector/heading knowledge ---
        OperationSchema {
            name: "doc.set".into(),
            description: "Set a value at a selector path in a JSON, YAML, or TOML file. Parser-backed; output is always valid.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string", "description": "Selector path (e.g., 'database.host', 'items[0].name')."},
                    "value": {"description": "The value to set (any JSON type)."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Update a version in package.json".into(),
                    args: serde_json::json!({"op": "doc.set", "path": "package.json", "selector": "version", "value": "2.0.0"}),
                },
            ],
        },
        OperationSchema {
            name: "doc.delete".into(),
            description: "Delete a key at a selector path in a JSON, YAML, or TOML file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string"}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Remove a deprecated config key".into(),
                    args: serde_json::json!({"op": "doc.delete", "path": "config.json", "selector": "deprecated_key"}),
                },
            ],
        },
        OperationSchema {
            name: "doc.merge".into(),
            description: "Deep-merge a JSON object into the root of a document.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "value": {"type": "object", "description": "Object to deep-merge into the document root."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Add database config".into(),
                    args: serde_json::json!({"op": "doc.merge", "path": "config.yaml", "value": {"database": {"host": "localhost", "port": 5432}}}),
                },
            ],
        },
        OperationSchema {
            name: "doc.append".into(),
            description: "Append a value to an array at a selector path.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string"},
                    "value": {"description": "Value to append."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Add an item to a list".into(),
                    args: serde_json::json!({"op": "doc.append", "path": "data.json", "selector": "items", "value": "new_item"}),
                },
            ],
        },
        OperationSchema {
            name: "doc.move".into(),
            description: "Move a value from one selector path to another within the same file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "from", "to"],
                "properties": {
                    "path": {"type": "string"},
                    "from": {"type": "string", "description": "Source selector."},
                    "to": {"type": "string", "description": "Destination selector."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Rename a config key".into(),
                    args: serde_json::json!({"op": "doc.move", "path": "config.json", "from": "old_key", "to": "new_key"}),
                },
            ],
        },
        OperationSchema {
            name: "doc.ensure".into(),
            description: "Set a value only if the selector path does not already exist.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string"},
                    "value": {"description": "Value to set if missing."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Ensure a default config value exists".into(),
                    args: serde_json::json!({"op": "doc.ensure", "path": "config.json", "selector": "timeout", "value": 30}),
                },
            ],
        },
        OperationSchema {
            name: "md.replace_section".into(),
            description: "Replace the body of a markdown section identified by heading.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "heading": {"type": "string", "description": "Heading text (without # prefix)."},
                    "content": {"type": "string", "description": "New section body."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Update the install section of README".into(),
                    args: serde_json::json!({"op": "md.replace_section", "path": "README.md", "heading": "## Installation", "content": "Run `npm install`.\n"}),
                },
            ],
        },
        OperationSchema {
            name: "md.upsert_bullet".into(),
            description: "Insert or update a bullet point under a markdown heading.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading", "bullet"],
                "properties": {
                    "path": {"type": "string"},
                    "heading": {"type": "string"},
                    "bullet": {"type": "string", "description": "Full bullet text (e.g., '- New item')."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Add a changelog entry".into(),
                    args: serde_json::json!({"op": "md.upsert_bullet", "path": "CHANGELOG.md", "heading": "## Changes", "bullet": "- Added new feature"}),
                },
            ],
        },
        OperationSchema {
            name: "md.table_append".into(),
            description: "Append a row to a markdown table under a heading.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading", "row"],
                "properties": {
                    "path": {"type": "string"},
                    "heading": {"type": "string"},
                    "row": {"type": "string", "description": "Table row in markdown format (e.g., '| col1 | col2 |')."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Add a row to an API table".into(),
                    args: serde_json::json!({"op": "md.table_append", "path": "README.md", "heading": "## API", "row": "| GET | /health | Health check |"}),
                },
            ],
        },
        OperationSchema {
            name: "md.insert_after_heading".into(),
            description: "Insert content immediately after a markdown heading.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "heading": {"type": "string"},
                    "content": {"type": "string"}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![],
        },
        OperationSchema {
            name: "md.insert_before_heading".into(),
            description: "Insert content immediately before a markdown heading.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "heading": {"type": "string"},
                    "content": {"type": "string"}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![],
        },
        OperationSchema {
            name: "md.move_section".into(),
            description: "Move a heading section to a new position (same-file reorder or cross-file move). Exactly one of before or after is required.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "heading"],
                "properties": {
                    "path": {"type": "string", "description": "Source file containing the section."},
                    "heading": {"type": "string", "description": "Heading of the section to move."},
                    "to": {"type": "string", "description": "Destination file (omit for same-file reorder)."},
                    "before": {"type": "string", "description": "Insert before this heading."},
                    "after": {"type": "string", "description": "Insert after this heading."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Reorder within same file".into(),
                    args: serde_json::json!({"op": "md.move_section", "path": "README.md", "heading": "## FAQ", "before": "## License"}),
                },
                OperationExample {
                    description: "Move section to another file".into(),
                    args: serde_json::json!({"op": "md.move_section", "path": "spec.md", "heading": "## Appendix E", "to": "investigation.md", "after": "## Layer 4"}),
                },
            ],
        },
        OperationSchema {
            name: "patch.apply".into(),
            description: "Apply a unified diff patch to one or more files. Supports three-way merge on stale context.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["diff"],
                "properties": {
                    "diff": {"type": "string", "description": "Unified diff text."},
                    "on_stale": {"type": "string", "enum": ["fail", "merge"], "default": "fail"},
                    "allow_conflicts": {"type": "boolean", "default": false}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![],
        },
        // --- Strong tier: complex operations requiring orchestration knowledge ---
        OperationSchema {
            name: "doc.prepend".into(),
            description: "Prepend a value to the beginning of an array at a selector path.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string"},
                    "value": {"description": "Value to prepend."}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "doc.update".into(),
            description: "Update all array elements matching a predicate with new values.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "value"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string", "description": "Selector with predicate (e.g., 'items[name=foo]')."},
                    "value": {"type": "object", "description": "Fields to merge into matching elements."}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "doc.delete_where".into(),
            description: "Delete array elements matching a predicate.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "selector", "predicate"],
                "properties": {
                    "path": {"type": "string"},
                    "selector": {"type": "string"},
                    "predicate": {"type": "string", "description": "key=value predicate."}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "search".into(),
            description: "Search for text across files with optional regex, context, and count assertion.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "pattern"],
                "properties": {
                    "path": {"type": "string"},
                    "pattern": {"type": "string"},
                    "regex": {"type": "boolean", "default": false},
                    "case_insensitive": {"type": "boolean", "default": false},
                    "multiline": {"type": "boolean", "default": false},
                    "invert_match": {"type": "boolean", "default": false},
                    "context": {"type": "integer"},
                    "assert_count": {"type": "integer", "description": "Fail if match count differs."}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "read".into(),
            description: "Read file contents with optional line range.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "lines": {"type": "string", "description": "Line range (e.g., '10:25')."}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "md.dedupe_headings".into(),
            description: "Remove duplicate markdown headings in a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        OperationSchema {
            name: "md.lint_agents".into(),
            description: "Lint an AGENTS.md file for common issues.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            }),
            min_tier: Tier::Strong,
            examples: vec![],
        },
        // --- AST operations (feature-gated) ---
        #[cfg(feature = "ast")]
        OperationSchema {
            name: "ast.rename".into(),
            description: "AST-aware rename: rename identifiers skipping strings and comments.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "old_name", "new_name"],
                "properties": {
                    "path": {"type": "string", "description": "File path (relative to working directory)."},
                    "old_name": {"type": "string", "description": "Current identifier name."},
                    "new_name": {"type": "string", "description": "New identifier name."},
                    "lang": {"type": "string", "description": "Language hint (e.g. 'rust', 'python'). Auto-detected from extension if omitted."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Rename a struct".into(),
                    args: serde_json::json!({"op": "ast.rename", "path": "src/lib.rs", "old_name": "OldStruct", "new_name": "NewStruct"}),
                },
            ],
        },
        #[cfg(feature = "ast")]
        OperationSchema {
            name: "ast.replace".into(),
            description: "Replace text within a specific symbol's body (AST-scoped).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["path", "symbol", "from", "to"],
                "properties": {
                    "path": {"type": "string", "description": "File path (relative to working directory)."},
                    "symbol": {"type": "string", "description": "Symbol name to scope the replacement to."},
                    "from": {"type": "string", "description": "Text to find within the symbol body."},
                    "to": {"type": "string", "description": "Replacement text."},
                    "regex": {"type": "boolean", "default": false, "description": "Use regex mode for the from pattern."},
                    "lang": {"type": "string", "description": "Language hint. Auto-detected from extension if omitted."}
                }
            }),
            min_tier: Tier::Medium,
            examples: vec![
                OperationExample {
                    description: "Replace a constant inside a function".into(),
                    args: serde_json::json!({"op": "ast.replace", "path": "src/config.rs", "symbol": "default_timeout", "from": "30", "to": "60"}),
                },
            ],
        },
    ]
}

/// JSON Schema for plan-level `write_policy` envelope fields.
///
/// These fields are set at the plan level (not per-operation) and apply to all
/// writes in a transaction. Agents using `patchloom schema` can discover these
/// to build complete tx plans.
pub fn plan_write_policy_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": "Write policy applied to all operations in the plan. Each field is optional; omitted fields inherit from CLI flags or project config.",
        "properties": {
            "ensure_final_newline": {
                "type": "boolean",
                "description": "Ensure every written file ends with a newline."
            },
            "normalize_eol": {
                "type": "string",
                "enum": ["keep", "lf", "crlf"],
                "description": "Line ending normalization mode."
            },
            "trim_trailing_whitespace": {
                "type": "boolean",
                "description": "Remove trailing whitespace from every line."
            },
            "collapse_blanks": {
                "type": "boolean",
                "description": "Collapse consecutive blank lines into a single blank line."
            }
        }
    })
}

/// Get operations available at a given capability tier (includes all lower tiers).
pub fn operations_for_tier(tier: Tier) -> Vec<OperationSchema> {
    operation_schemas()
        .into_iter()
        .filter(|op| op.min_tier <= tier)
        .collect()
}

/// Generate a system prompt fragment describing available operations at this tier.
///
/// The output is markdown text suitable for inclusion in an LLM system prompt.
pub fn system_prompt_for_tier(tier: Tier) -> String {
    let ops = operations_for_tier(tier);
    let mut out = String::new();

    out.push_str(&format!(
        "# Patchloom Operations (tier: {tier}, format version: {INTENT_FORMAT_VERSION})\n\n"
    ));
    out.push_str("The following operations are available. Each operation is expressed as a JSON object with an `\"op\"` field.\n\n");

    for op in &ops {
        out.push_str(&format!("## `{}`\n\n", op.name));
        out.push_str(&format!("{}\n\n", op.description));

        // Parameters
        if let Some(props) = op.parameters.get("properties")
            && let Some(obj) = props.as_object()
        {
            out.push_str("**Parameters:**\n\n");
            let required: Vec<&str> = op
                .parameters
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            for (key, schema) in obj {
                let type_str = schema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                let desc = schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                let req = if required.contains(&key.as_str()) {
                    " (required)"
                } else {
                    ""
                };
                out.push_str(&format!("- `{key}`: {type_str}{req}"));
                if !desc.is_empty() {
                    out.push_str(&format!(" -- {desc}"));
                }
                out.push('\n');
            }
            out.push('\n');
        }

        // Examples
        if !op.examples.is_empty() {
            out.push_str("**Examples:**\n\n");
            for ex in &op.examples {
                out.push_str(&format!("- {}: `{}`\n", ex.description, ex.args));
            }
            out.push('\n');
        }
    }

    // Plan envelope: write_policy
    out.push_str("## Plan Envelope: `write_policy`\n\n");
    out.push_str("Set at the plan level (not per-operation) to apply write transformations to all operations.\n\n");
    let wp_schema = plan_write_policy_schema();
    if let Some(props) = wp_schema.get("properties")
        && let Some(obj) = props.as_object()
    {
        out.push_str("**Fields:**\n\n");
        for (key, schema) in obj {
            let type_str = schema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
            let desc = schema
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            out.push_str(&format!("- `{key}`: {type_str}"));
            if !desc.is_empty() {
                out.push_str(&format!(" -- {desc}"));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compatibility() {
        assert!(is_compatible_version("1"));
        assert!(!is_compatible_version("0"));
        assert!(!is_compatible_version("2"));
    }

    #[test]
    fn tier_ordering() {
        assert!(Tier::Weak < Tier::Medium);
        assert!(Tier::Medium < Tier::Strong);
    }

    #[test]
    fn tier_parse_roundtrip() {
        for tier in [Tier::Weak, Tier::Medium, Tier::Strong] {
            let s = tier.to_string();
            let parsed: Tier = s.parse().unwrap();
            assert_eq!(parsed, tier);
        }
    }

    #[test]
    fn tier_parse_case_insensitive() {
        assert_eq!("WEAK".parse::<Tier>().unwrap(), Tier::Weak);
        assert_eq!("Medium".parse::<Tier>().unwrap(), Tier::Medium);
        assert_eq!("STRONG".parse::<Tier>().unwrap(), Tier::Strong);
    }

    #[test]
    fn tier_parse_invalid() {
        assert!("unknown".parse::<Tier>().is_err());
    }

    #[test]
    fn operation_schemas_non_empty() {
        let schemas = operation_schemas();
        assert!(!schemas.is_empty());
        // Every schema must have a name, description, and valid JSON parameters.
        for schema in &schemas {
            assert!(!schema.name.is_empty(), "schema name is empty");
            assert!(
                !schema.description.is_empty(),
                "schema {}: description is empty",
                schema.name
            );
            assert!(
                schema.parameters.is_object(),
                "schema {}: parameters is not an object",
                schema.name
            );
        }
    }

    #[test]
    fn operation_schemas_have_unique_names() {
        let schemas = operation_schemas();
        let mut names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "duplicate operation names found");
    }

    #[test]
    fn weak_tier_returns_subset() {
        let all = operation_schemas();
        let weak = operations_for_tier(Tier::Weak);
        assert!(
            weak.len() < all.len(),
            "weak should be a proper subset of all"
        );
        for op in &weak {
            assert_eq!(
                op.min_tier,
                Tier::Weak,
                "weak tier should only contain weak ops, got {:?} for {}",
                op.min_tier,
                op.name
            );
        }
    }

    #[test]
    fn medium_tier_includes_weak() {
        let weak = operations_for_tier(Tier::Weak);
        let medium = operations_for_tier(Tier::Medium);
        assert!(medium.len() >= weak.len());
        let weak_names: Vec<&str> = weak.iter().map(|o| o.name.as_str()).collect();
        for name in &weak_names {
            assert!(
                medium.iter().any(|o| o.name == *name),
                "medium tier should include weak op: {name}"
            );
        }
    }

    #[test]
    fn strong_tier_returns_all() {
        let all = operation_schemas();
        let strong = operations_for_tier(Tier::Strong);
        assert_eq!(
            strong.len(),
            all.len(),
            "strong tier should include all operations"
        );
    }

    #[test]
    fn weak_tier_has_expected_ops() {
        let weak = operations_for_tier(Tier::Weak);
        let names: Vec<&str> = weak.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"replace"), "weak tier must include replace");
        assert!(
            names.contains(&"file.create"),
            "weak tier must include file.create"
        );
        assert!(
            names.contains(&"file.delete"),
            "weak tier must include file.delete"
        );
        assert!(
            names.contains(&"tidy.fix"),
            "weak tier must include tidy.fix"
        );
    }

    #[test]
    fn medium_tier_has_expected_ops() {
        let medium = operations_for_tier(Tier::Medium);
        let names: Vec<&str> = medium.iter().map(|o| o.name.as_str()).collect();
        assert!(
            names.contains(&"doc.set"),
            "medium tier must include doc.set"
        );
        assert!(
            names.contains(&"md.replace_section"),
            "medium tier must include md.replace_section"
        );
        assert!(
            names.contains(&"doc.merge"),
            "medium tier must include doc.merge"
        );
    }

    #[test]
    fn strong_tier_has_expected_ops() {
        let strong = operations_for_tier(Tier::Strong);
        let names: Vec<&str> = strong.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"search"), "strong tier must include search");
        assert!(
            names.contains(&"doc.delete_where"),
            "strong tier must include doc.delete_where"
        );
    }

    #[test]
    fn system_prompt_for_weak_tier() {
        let prompt = system_prompt_for_tier(Tier::Weak);
        assert!(prompt.contains("tier: weak"));
        assert!(prompt.contains("replace"));
        assert!(prompt.contains("file.create"));
        // Should NOT contain medium/strong ops.
        assert!(!prompt.contains("## `doc.set`"));
        assert!(!prompt.contains("## `search`"));
    }

    #[test]
    fn system_prompt_for_strong_tier() {
        let prompt = system_prompt_for_tier(Tier::Strong);
        assert!(prompt.contains("tier: strong"));
        assert!(prompt.contains("replace"));
        assert!(prompt.contains("doc.set"));
        assert!(prompt.contains("search"));
    }

    #[test]
    fn plan_write_policy_schema_has_all_fields() {
        let schema = plan_write_policy_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("ensure_final_newline"));
        assert!(props.contains_key("normalize_eol"));
        assert!(props.contains_key("trim_trailing_whitespace"));
        assert!(props.contains_key("collapse_blanks"));
    }

    #[test]
    fn system_prompt_includes_write_policy() {
        let prompt = system_prompt_for_tier(Tier::Strong);
        assert!(prompt.contains("write_policy"));
        assert!(prompt.contains("collapse_blanks"));
        assert!(prompt.contains("ensure_final_newline"));
    }

    #[test]
    fn system_prompt_includes_version() {
        let prompt = system_prompt_for_tier(Tier::Medium);
        assert!(prompt.contains(&format!("format version: {INTENT_FORMAT_VERSION}")));
    }

    #[test]
    fn schemas_have_required_fields() {
        let schemas = operation_schemas();
        for schema in &schemas {
            let required = schema.parameters.get("required");
            assert!(
                required.is_some(),
                "schema {}: must have 'required' field",
                schema.name
            );
        }
    }

    #[test]
    fn operation_examples_include_op_field() {
        for schema in operation_schemas() {
            for ex in &schema.examples {
                let op = ex
                    .args
                    .get("op")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "schema {} example {:?}: missing 'op' field",
                            schema.name, ex.description
                        )
                    });
                assert_eq!(
                    op, schema.name,
                    "schema {} example {:?}: op must match operation name",
                    schema.name, ex.description
                );
            }
        }
    }

    #[test]
    fn operation_schema_serializes_to_json() {
        let schemas = operation_schemas();
        let json = serde_json::to_string_pretty(&schemas).unwrap();
        assert!(!json.is_empty());
        // Roundtrip: deserialize back.
        let deserialized: Vec<OperationSchema> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), schemas.len());
    }

    // Static assertion: types must be Send + Sync.
    const _: () = {
        fn _assert<T: Send + Sync>() {}
        let _ = _assert::<Tier>;
        let _ = _assert::<OperationSchema>;
        let _ = _assert::<OperationExample>;
    };
}
