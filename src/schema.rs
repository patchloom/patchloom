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
//! let ops = schema::operations_for_tier(Tier::Medium).unwrap();
//! assert!(!ops.is_empty());
//!
//! // Generate a system prompt fragment
//! let prompt = schema::system_prompt_for_tier(Tier::Medium).unwrap();
//! assert!(prompt.contains("replace"));
//! ```

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Cached full JSON Schema for `plan::Operation` (tagged enum).
/// Generated once via `LazyLock`; individual variant schemas are extracted from
/// the `oneOf` array by matching on the `op` const value.
static OPERATION_FULL_SCHEMA: std::sync::LazyLock<serde_json::Value> =
    std::sync::LazyLock::new(|| {
        let generator = schemars::generate::SchemaSettings::default().into_generator();
        let root = generator.into_root_schema_for::<crate::plan::Operation>();
        serde_json::to_value(root).expect("Operation schema serialization")
    });

/// Extract a single variant's JSON Schema from the full `Operation` schema.
///
/// The schemars output for `#[serde(tag = "op")]` enums is a `oneOf` array
/// where each element has `properties.op.const` set to the variant's serde
/// rename value (e.g., `"doc.set"`). This function finds the matching variant,
/// clones it, strips the `op` discriminator property (not needed for standalone
/// schema consumers), and removes top-level `$schema`/`title` for compactness.
pub fn operation_variant_schema(op_name: &str) -> anyhow::Result<serde_json::Value> {
    let schema = &*OPERATION_FULL_SCHEMA;

    // Navigate: schema.oneOf[i].properties.op.const == op_name
    let one_of = schema
        .get("oneOf")
        .and_then(|v| v.as_array())
        .context("Operation schema missing oneOf array")?;

    for variant in one_of {
        let op_const = variant
            .pointer("/properties/op/const")
            .and_then(|v| v.as_str());
        if op_const == Some(op_name) {
            let mut v = variant.clone();
            if let Some(obj) = v.as_object_mut() {
                // Remove the "op" discriminator property.
                if let Some(props) = obj.get_mut("properties").and_then(|p| p.as_object_mut()) {
                    props.remove("op");
                }
                // Remove "op" from the required array.
                if let Some(req) = obj.get_mut("required").and_then(|r| r.as_array_mut()) {
                    req.retain(|r| r.as_str() != Some("op"));
                }
                // Strip metadata keys for compactness.
                obj.remove("$schema");
                obj.remove("title");
                // Copy $defs from root schema so $ref pointers resolve
                // (e.g. patch.apply references OnStale, ast.split references
                // SplitTargetSpec).
                if let Some(defs) = schema.get("$defs") {
                    obj.insert("$defs".to_string(), defs.clone());
                }
            }
            return Ok(v);
        }
    }

    anyhow::bail!("Operation variant '{op_name}' not found in oneOf schema");
}

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
///
/// CLI accepts only these names (`weak`, `medium`, `strong`) via clap
/// `ValueEnum` so help lists the enum; no industry-size aliases (`small`/`large`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
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

/// Static metadata for each operation: (name, description, tier, examples).
///
/// The `parameters` field is derived at runtime from `plan::Operation` via
/// `operation_variant_schema()`. This table holds only the human-authored
/// metadata that cannot be derived: description text, capability tier, and
/// example invocations.
///
/// Operations are listed in tier order (weak, medium, strong) to match the
/// output order of `operation_schemas()`.
struct OpMeta {
    name: &'static str,
    description: &'static str,
    tier: Tier,
    examples: &'static [(&'static str, &'static str)],
}

// Example tuples: (description, JSON args as string).
// Using &str avoids runtime serde_json::json!() allocation in the static table;
// parsing happens lazily in operation_schemas().

const OPERATION_REGISTRY: &[OpMeta] = &[
    // --- Weak tier ---
    OpMeta {
        name: "replace",
        description: "Replace text in a file using literal string matching. Optional require_change (fail closed on zero matches) and command_position (shell invocable tokens only; peels sudo/timeout/busybox/flock/runuser/unshare/nsenter/taskset wrappers, not uv pip).",
        tier: Tier::Weak,
        examples: &[
            (
                "Replace a function name",
                r###"{"op":"replace","path":"src/main.rs","old":"old_name","new":"new_name"}"###,
            ),
            (
                "Rewrite shell command tokens only",
                r###"{"op":"replace","path":"install.sh","old":"pip","new":"uv","command_position":true,"require_change":true}"###,
            ),
        ],
    },
    OpMeta {
        name: "file.append",
        description: "Append content to an existing file.",
        tier: Tier::Weak,
        examples: &[(
            "Append a test function to a test file",
            r###"{"op":"file.append","path":"tests/test.rs","content":"#[test]\nfn new_test() {}\n"}"###,
        )],
    },
    OpMeta {
        name: "file.prepend",
        description: "Prepend content to an existing file.",
        tier: Tier::Weak,
        examples: &[(
            "Prepend a license header",
            r###"{"op":"file.prepend","path":"src/main.rs","content":"// Copyright 2026\n"}"###,
        )],
    },
    OpMeta {
        name: "file.create",
        description: "Create a new file with specified content.",
        tier: Tier::Weak,
        examples: &[(
            "Create a new config file",
            r###"{"op":"file.create","path":"config.json","content":"{\"version\": \"1.0\"}"}"###,
        )],
    },
    OpMeta {
        name: "file.delete",
        description: "Delete a file.",
        tier: Tier::Weak,
        examples: &[(
            "Delete a temporary file",
            r###"{"op":"file.delete","path":"tmp/scratch.txt"}"###,
        )],
    },
    OpMeta {
        name: "file.rename",
        description: "Rename (move) a file.",
        tier: Tier::Weak,
        examples: &[(
            "Rename a file",
            r###"{"op":"file.rename","from":"old.txt","to":"new.txt"}"###,
        )],
    },
    OpMeta {
        name: "tidy.fix",
        description: "Normalize whitespace, line endings, and final newline in a file.",
        tier: Tier::Weak,
        examples: &[(
            "Fix whitespace in a file",
            r###"{"op":"tidy.fix","path":"src/main.rs","ensure_final_newline":true,"trim_trailing_whitespace":true}"###,
        )],
    },
    // --- Medium tier ---
    OpMeta {
        name: "doc.set",
        description: "Set a value at a selector path in a JSON, YAML, or TOML file. Parser-backed; output is always valid.",
        tier: Tier::Medium,
        examples: &[(
            "Update a version in package.json",
            r###"{"op":"doc.set","path":"package.json","selector":"version","value":"2.0.0"}"###,
        )],
    },
    OpMeta {
        name: "doc.delete",
        description: "Delete a value at a selector path in a JSON, YAML, or TOML file. CLI --json and MCP/tx success include changed and removed (0 on missing key; exit 0 / ok is idempotent).",
        tier: Tier::Medium,
        examples: &[(
            "Remove a deprecated config entry",
            r###"{"op":"doc.delete","path":"config.json","selector":"deprecated_key"}"###,
        )],
    },
    OpMeta {
        name: "doc.merge",
        description: "Deep-merge a JSON object into the root of a document.",
        tier: Tier::Medium,
        examples: &[(
            "Add database config",
            r###"{"op":"doc.merge","path":"config.yaml","value":{"database":{"host":"localhost","port":5432}}}"###,
        )],
    },
    OpMeta {
        name: "doc.append",
        description: "Append a value to an array at a selector path.",
        tier: Tier::Medium,
        examples: &[(
            "Add an item to a list",
            r###"{"op":"doc.append","path":"data.json","selector":"items","value":"new_item"}"###,
        )],
    },
    OpMeta {
        name: "doc.move",
        description: "Move a value from one selector path to another within the same file.",
        tier: Tier::Medium,
        examples: &[(
            "Rename a config path",
            r###"{"op":"doc.move","path":"config.json","from":"old_key","to":"new_key"}"###,
        )],
    },
    OpMeta {
        name: "doc.ensure",
        description: "Set a value only if the selector path does not already exist.",
        tier: Tier::Medium,
        examples: &[(
            "Ensure a default config value exists",
            r###"{"op":"doc.ensure","path":"config.json","selector":"timeout","value":30}"###,
        )],
    },
    OpMeta {
        name: "md.replace_section",
        description: "Replace the body of a markdown section identified by heading.",
        tier: Tier::Medium,
        examples: &[(
            "Update the install section of README",
            r###"{"op":"md.replace_section","path":"README.md","heading":"## Installation","content":"Run `npm install`.\n"}"###,
        )],
    },
    OpMeta {
        name: "md.upsert_bullet",
        description: "Insert or update a bullet point under a markdown heading.",
        tier: Tier::Medium,
        examples: &[(
            "Add a changelog entry",
            r###"{"op":"md.upsert_bullet","path":"CHANGELOG.md","heading":"## Changes","bullet":"- Added new feature"}"###,
        )],
    },
    OpMeta {
        name: "md.table_append",
        description: "Append a row to a markdown table under a heading.",
        tier: Tier::Medium,
        examples: &[(
            "Add a row to an API table",
            r###"{"op":"md.table_append","path":"README.md","heading":"## API","row":"| GET | /health | Health check |"}"###,
        )],
    },
    OpMeta {
        name: "md.insert_after_heading",
        description: "Insert content immediately after a markdown heading.",
        tier: Tier::Medium,
        examples: &[],
    },
    OpMeta {
        name: "md.insert_before_heading",
        description: "Insert content immediately before a markdown heading.",
        tier: Tier::Medium,
        examples: &[],
    },
    OpMeta {
        name: "md.move_section",
        description: "Move a heading section to a new position (same-file reorder or cross-file move). Exactly one of before or after is required.",
        tier: Tier::Medium,
        examples: &[
            (
                "Reorder within same file",
                r###"{"op":"md.move_section","path":"README.md","heading":"## FAQ","before":"## License"}"###,
            ),
            (
                "Move section to another file",
                r###"{"op":"md.move_section","path":"spec.md","heading":"## Appendix E","to":"investigation.md","after":"## Layer 4"}"###,
            ),
        ],
    },
    OpMeta {
        name: "patch.apply",
        description: "Apply a unified diff patch to one or more files. Supports three-way merge on stale context.",
        tier: Tier::Medium,
        examples: &[],
    },
    // --- Strong tier ---
    OpMeta {
        name: "doc.prepend",
        description: "Prepend a value to the beginning of an array at a selector path.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "doc.update",
        description: "Set a new value at every location matching a selector. Use wildcards (items[*].enabled) or selector predicates (items[name=foo].v). Not a separate --where flag; the filter is part of the selector string.",
        tier: Tier::Strong,
        examples: &[
            (
                "Update one field on matching array objects via a selector predicate",
                r###"{"op":"doc.update","path":"config.toml","selector":"items[name=a].v","value":7}"###,
            ),
            (
                "Set a field on every array element with a wildcard",
                r###"{"op":"doc.update","path":"config.json","selector":"items[*].enabled","value":true}"###,
            ),
        ],
    },
    OpMeta {
        name: "doc.delete_where",
        description: "Delete array elements matching a key=value predicate via --predicate (CLI) or the predicate field (plans). For scalar arrays use .=x, _=x, or value=x. Different from doc.update, which filters inside the selector path. CLI --json and MCP/tx success include changed and removed (0 when no elements match; exit 0 / ok is idempotent).",
        tier: Tier::Strong,
        examples: &[(
            "Remove scalar array elements equal to a (agent-friendly value= form)",
            r###"{"op":"doc.delete_where","path":"config.toml","selector":"tags","predicate":"value=a"}"###,
        )],
    },
    OpMeta {
        name: "search",
        description: "Search for text across files with optional regex, context, and count assertion. Supports advanced layered ignores: literal (vs regex), globs (include), exclude_patterns, custom_ignore_filenames, max_results, before_context/after_context.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "read",
        description: "Read file contents with optional line range.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "md.dedupe_headings",
        description: "Remove duplicate markdown headings in a file.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "md.lint_agents",
        description: "Lint an AGENTS.md file for common issues.",
        tier: Tier::Strong,
        examples: &[],
    },
];

/// AST operation metadata, compiled only when the `ast` feature is enabled.
#[cfg(feature = "ast")]
const AST_OPERATION_REGISTRY: &[OpMeta] = &[
    OpMeta {
        name: "ast.rename",
        description: "AST-aware rename: rename identifiers skipping strings and comments.",
        tier: Tier::Medium,
        examples: &[(
            "Rename a struct",
            r###"{"op":"ast.rename","path":"src/lib.rs","old":"OldStruct","new":"NewStruct"}"###,
        )],
    },
    OpMeta {
        name: "ast.replace",
        description: "Replace text within a specific symbol's body (AST-scoped).",
        tier: Tier::Medium,
        examples: &[(
            "Replace a constant inside a function",
            r###"{"op":"ast.replace","path":"src/config.rs","symbol":"default_timeout","old":"30","new":"60"}"###,
        )],
    },
    OpMeta {
        name: "ast.rewrite_signature",
        description: "Rewrite a function signature with structured fields (visibility, parameters, return_type) or a full new_signature string. Multi-language via tree-sitter.",
        tier: Tier::Medium,
        examples: &[(
            "Change parameters and return type",
            r###"{"op":"ast.rewrite_signature","path":"src/lib.rs","old":"process","parameters":"(x: i32)","return_type":"-> String"}"###,
        )],
    },
    OpMeta {
        name: "ast.insert",
        description: "Insert code at a structurally-aware position: inside a container, or after/before a named symbol.",
        tier: Tier::Medium,
        examples: &[(
            "Insert a function after an existing one",
            r###"{"op":"ast.insert","path":"src/lib.rs","content":"fn new_fn() {}","after":"existing_fn"}"###,
        )],
    },
    OpMeta {
        name: "ast.wrap",
        description: "Wrap existing code in a structural block (module, impl, cfg, etc.).",
        tier: Tier::Medium,
        examples: &[(
            "Wrap helpers in a module",
            r###"{"op":"ast.wrap","path":"src/lib.rs","symbols":["helper_a","helper_b"],"wrapper":"mod helpers"}"###,
        )],
    },
    OpMeta {
        name: "ast.imports",
        description: "Manage import/use statements: add (idempotent), remove, deduplicate.",
        tier: Tier::Medium,
        examples: &[(
            "Add an import statement",
            r###"{"op":"ast.imports","path":"src/main.rs","add":["use std::collections::HashMap;"]}"###,
        )],
    },
    OpMeta {
        name: "ast.reorder",
        description: "Reorder symbols within a file or scope by name, kind, or custom order.",
        tier: Tier::Medium,
        examples: &[(
            "Alphabetize functions",
            r###"{"op":"ast.reorder","path":"src/lib.rs","order":"alphabetical"}"###,
        )],
    },
    OpMeta {
        name: "ast.group",
        description: "Group symbols into a named module within a file.",
        tier: Tier::Medium,
        examples: &[(
            "Group test functions into a module",
            r###"{"op":"ast.group","path":"src/tests.rs","module":"line_endings","symbols":["test_crlf","test_lf"],"preamble":"use super::*;"}"###,
        )],
    },
    OpMeta {
        name: "ast.move",
        description: "Move symbols between files with optional target creation.",
        tier: Tier::Medium,
        examples: &[(
            "Move helpers to a utility file",
            r###"{"op":"ast.move","path":"src/lib.rs","target":"src/helpers.rs","symbols":["helper_fn"]}"###,
        )],
    },
    OpMeta {
        name: "ast.extract_to_file",
        description: "Extract a symbol to a separate file with optional module unwrapping.",
        tier: Tier::Medium,
        examples: &[(
            "Extract test module to companion file",
            r###"{"op":"ast.extract_to_file","source":"src/lib.rs","symbol":"tests","target":"src/lib_tests.rs","replacement":"mod tests;","prepend":"use super::*;"}"###,
        )],
    },
    OpMeta {
        name: "ast.split",
        description: "Split a file into multiple target files by distributing symbols.",
        tier: Tier::Strong,
        examples: &[(
            "Split a large module into sub-modules",
            r###"{"op":"ast.split","source":"src/big.rs","targets":[{"path":"src/types.rs","symbols":["Config"],"prepend":"use super::*;"}],"keep_in_source":["main"],"source_suffix":"mod types;"}"###,
        )],
    },
];

/// Return the complete registry of all patchloom operations with schemas.
pub fn operation_schemas() -> anyhow::Result<Vec<OperationSchema>> {
    #[allow(unused_mut)]
    let mut all: Vec<&OpMeta> = OPERATION_REGISTRY.iter().collect();
    #[cfg(feature = "ast")]
    all.extend(AST_OPERATION_REGISTRY.iter());

    all.into_iter()
        .map(|meta| {
            Ok(OperationSchema {
                name: meta.name.into(),
                description: meta.description.into(),
                parameters: operation_variant_schema(meta.name)?,
                min_tier: meta.tier,
                examples: meta
                    .examples
                    .iter()
                    .map(|(desc, json)| {
                        Ok(OperationExample {
                            description: (*desc).into(),
                            args: serde_json::from_str(json)
                                .with_context(|| format!("bad example JSON for {}", meta.name))?,
                        })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?,
            })
        })
        .collect()
}

/// JSON Schema for plan-level `write_policy` envelope fields.
///
/// These fields are set at the plan level (not per-operation) and apply to all
/// writes in a transaction. Agents using `patchloom schema` can discover these
/// to build complete tx plans.
pub fn plan_write_policy_schema() -> serde_json::Value {
    let generator = schemars::generate::SchemaSettings::default().into_generator();
    let root = generator.into_root_schema_for::<crate::write::WritePolicyOverride>();
    let mut v = serde_json::to_value(root).expect("schema serialization");
    if let Some(obj) = v.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
    }
    v
}

/// Get operations available at a given capability tier (includes all lower tiers).
pub fn operations_for_tier(tier: Tier) -> anyhow::Result<Vec<OperationSchema>> {
    Ok(operation_schemas()?
        .into_iter()
        .filter(|op| op.min_tier <= tier)
        .collect())
}

/// Extract a human-readable type string from a JSON Schema `type` field.
///
/// Handles both `"type": "string"` (scalar) and `"type": ["string", "null"]`
/// (nullable, produced by schemars for `Option<T>` fields).
fn extract_type_str(schema: &serde_json::Value) -> String {
    match schema.get("type") {
        Some(t) if t.is_string() => t.as_str().expect("is_string guard").to_string(),
        Some(t) if t.is_array() => t
            .as_array()
            .expect("is_array guard")
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        _ => "any".to_string(),
    }
}

/// Generate a system prompt fragment describing available operations at this tier.
///
/// The output is markdown text suitable for inclusion in an LLM system prompt.
pub fn system_prompt_for_tier(tier: Tier) -> anyhow::Result<String> {
    let ops = operations_for_tier(tier)?;
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
                let type_str = extract_type_str(schema);
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
            let type_str = extract_type_str(schema);
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

    Ok(out)
}

#[path = "schema_tests.rs"]
#[cfg(test)]
mod tests;

/// All registered plan operation names (non-AST + AST when the `ast` feature is on).
pub fn registered_operation_names() -> Vec<&'static str> {
    #[cfg(feature = "ast")]
    {
        let mut names: Vec<&'static str> = OPERATION_REGISTRY.iter().map(|o| o.name).collect();
        names.extend(AST_OPERATION_REGISTRY.iter().map(|o| o.name));
        names
    }
    #[cfg(not(feature = "ast"))]
    {
        OPERATION_REGISTRY.iter().map(|o| o.name).collect()
    }
}

/// Look up the human description for a plan `op` name (e.g. `"doc.set"`).
pub fn operation_description(op_name: &str) -> Option<&'static str> {
    OPERATION_REGISTRY
        .iter()
        .chain(ast_registry_iter())
        .find(|o| o.name == op_name)
        .map(|o| o.description)
}

/// First example JSON string for an operation, if any.
pub fn operation_example_json(op_name: &str) -> Option<&'static str> {
    OPERATION_REGISTRY
        .iter()
        .chain(ast_registry_iter())
        .find(|o| o.name == op_name)
        .and_then(|o| o.examples.first().map(|(_, json)| *json))
}

fn ast_registry_iter() -> impl Iterator<Item = &'static OpMeta> {
    #[cfg(feature = "ast")]
    {
        AST_OPERATION_REGISTRY.iter()
    }
    #[cfg(not(feature = "ast"))]
    {
        std::iter::empty()
    }
}

/// Build an MCP tool description from the schema registry for a plan op name.
///
/// Format: `<OpMeta.description>[ extra][ Example: <first example JSON>]`
/// MCP-only guidance (concurrency, etc.) belongs in `extra`, not a full
/// duplicate of the registry prose (#1383).
pub fn mcp_tool_description(op_name: &str, extra: Option<&str>) -> String {
    let base = operation_description(op_name).unwrap_or("Patchloom operation.");
    let mut out = base.to_string();
    if let Some(extra) = extra.filter(|s| !s.is_empty()) {
        if !out.ends_with('.') && !out.ends_with(' ') {
            out.push('.');
        }
        out.push(' ');
        out.push_str(extra.trim());
    }
    if let Some(example) = operation_example_json(op_name) {
        out.push_str(" Example: ");
        out.push_str(example);
    }
    out
}

/// Build a compact agent-facing operations catalogue from the registry.
///
/// Used by `agent-rules` so the op list cannot drift from schema export.
pub fn agent_operations_catalogue() -> String {
    let mut out = String::from("## Operations (from schema registry)\n\n");
    for meta in OPERATION_REGISTRY.iter().chain(ast_registry_iter()) {
        out.push_str(&format!("- `{}`: {}\n", meta.name, meta.description));
    }
    out
}
