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
pub fn operation_variant_schema(op_name: &str) -> serde_json::Value {
    let schema = &*OPERATION_FULL_SCHEMA;

    // Navigate: schema.oneOf[i].properties.op.const == op_name
    let one_of = schema
        .get("oneOf")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("Operation schema missing oneOf array"));

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
            }
            return v;
        }
    }

    panic!("Operation variant \'{op_name}\' not found in oneOf schema");
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
        description: "Replace text in a file using literal string matching.",
        tier: Tier::Weak,
        examples: &[(
            "Replace a function name",
            r###"{"op":"replace","path":"src/main.rs","from":"old_name","to":"new_name"}"###,
        )],
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
        description: "Delete a key at a selector path in a JSON, YAML, or TOML file.",
        tier: Tier::Medium,
        examples: &[(
            "Remove a deprecated config key",
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
            "Rename a config key",
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
        description: "Update all array elements matching a predicate with new values.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "doc.delete_where",
        description: "Delete array elements matching a predicate.",
        tier: Tier::Strong,
        examples: &[],
    },
    OpMeta {
        name: "search",
        description: "Search for text across files with optional regex, context, and count assertion. Supports advanced layered ignores (parity with library SearchOptions for Bline): literal (vs regex), globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results, before_context/after_context.",
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
            r###"{"op":"ast.rename","path":"src/lib.rs","old_name":"OldStruct","new_name":"NewStruct"}"###,
        )],
    },
    OpMeta {
        name: "ast.replace",
        description: "Replace text within a specific symbol's body (AST-scoped).",
        tier: Tier::Medium,
        examples: &[(
            "Replace a constant inside a function",
            r###"{"op":"ast.replace","path":"src/config.rs","symbol":"default_timeout","from":"30","to":"60"}"###,
        )],
    },
];

/// Return the complete registry of all patchloom operations with schemas.
pub fn operation_schemas() -> Vec<OperationSchema> {
    let mut all: Vec<&OpMeta> = OPERATION_REGISTRY.iter().collect();
    #[cfg(feature = "ast")]
    all.extend(AST_OPERATION_REGISTRY.iter());

    all.into_iter()
        .map(|meta| OperationSchema {
            name: meta.name.into(),
            description: meta.description.into(),
            parameters: operation_variant_schema(meta.name),
            min_tier: meta.tier,
            examples: meta
                .examples
                .iter()
                .map(|(desc, json)| OperationExample {
                    description: (*desc).into(),
                    args: serde_json::from_str(json)
                        .unwrap_or_else(|e| panic!("bad example JSON for {}: {e}", meta.name)),
                })
                .collect(),
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

    /// Verify that every field accepted by `plan::Operation` for a given op
    /// name has a corresponding entry in the hand-written schema `properties`.
    /// This catches drift when new fields are added to the serde-based
    /// `Operation` enum but not to the JSON Schema export.
    #[test]
    fn schema_properties_cover_plan_operation_fields() {
        use crate::plan::Operation;

        // Build representative Operation values for each schema op name.
        // Only the field names matter; values are dummies.
        let mut ops: Vec<(&str, Operation)> = vec![
            (
                "replace",
                Operation::Replace {
                    glob: Some("*.rs".into()),
                    path: Some("f.rs".into()),
                    mode: Some("literal".into()),
                    from: "a".into(),
                    to: Some("b".into()),
                    nth: Some(1),
                    insert_before: Some("x".into()),
                    insert_after: Some("y".into()),
                    case_insensitive: false,
                    multiline: false,
                    if_exists: false,
                    whole_line: false,
                    range: Some("1:10".into()),
                    word_boundary: false,
                    before_context: Some("ctx".into()),
                    after_context: Some("ctx".into()),
                },
            ),
            (
                "file.create",
                Operation::FileCreate {
                    path: "f".into(),
                    content: "c".into(),
                    force: Some(true),
                },
            ),
            (
                "file.append",
                Operation::FileAppend {
                    path: "f".into(),
                    content: "c".into(),
                },
            ),
            ("file.delete", Operation::FileDelete { path: "f".into() }),
            (
                "file.rename",
                Operation::FileRename {
                    from: "a".into(),
                    to: "b".into(),
                    force: true,
                },
            ),
            (
                "tidy.fix",
                Operation::TidyFix {
                    path: "f".into(),
                    ensure_final_newline: Some(true),
                    trim_trailing_whitespace: Some(true),
                    normalize_eol: Some("lf".into()),
                },
            ),
            (
                "doc.set",
                Operation::DocSet {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!("v"),
                },
            ),
            (
                "doc.delete",
                Operation::DocDelete {
                    path: "f".into(),
                    selector: "s".into(),
                },
            ),
            (
                "doc.merge",
                Operation::DocMerge {
                    path: "f".into(),
                    value: serde_json::json!({}),
                },
            ),
            (
                "doc.append",
                Operation::DocAppend {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!([]),
                },
            ),
            (
                "doc.prepend",
                Operation::DocPrepend {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!([]),
                },
            ),
            (
                "doc.update",
                Operation::DocUpdate {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!({}),
                },
            ),
            (
                "doc.move",
                Operation::DocMove {
                    path: "f".into(),
                    from: "a".into(),
                    to: "b".into(),
                },
            ),
            (
                "doc.ensure",
                Operation::DocEnsure {
                    path: "f".into(),
                    selector: "s".into(),
                    value: serde_json::json!(1),
                },
            ),
            (
                "doc.delete_where",
                Operation::DocDeleteWhere {
                    path: "f".into(),
                    selector: "s".into(),
                    predicate: "p".into(),
                },
            ),
            (
                "md.replace_section",
                Operation::MdReplaceSection {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.insert_after_heading",
                Operation::MdInsertAfterHeading {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.insert_before_heading",
                Operation::MdInsertBeforeHeading {
                    path: "f".into(),
                    heading: "h".into(),
                    content: "c".into(),
                },
            ),
            (
                "md.upsert_bullet",
                Operation::MdUpsertBullet {
                    path: "f".into(),
                    heading: "h".into(),
                    bullet: "b".into(),
                },
            ),
            (
                "md.table_append",
                Operation::MdTableAppend {
                    path: "f".into(),
                    heading: "h".into(),
                    row: "r".into(),
                },
            ),
            (
                "md.move_section",
                Operation::MdMoveSection {
                    path: "f".into(),
                    heading: "h".into(),
                    to: None,
                    before: None,
                    after: None,
                },
            ),
            (
                "md.dedupe_headings",
                Operation::MdDedupeHeadings { path: "f".into() },
            ),
            (
                "md.lint_agents",
                Operation::MdLintAgents { path: "f".into() },
            ),
            (
                "patch.apply",
                Operation::PatchApply {
                    diff: "d".into(),
                    on_stale: Default::default(),
                    allow_conflicts: false,
                },
            ),
            (
                "search",
                Operation::Search {
                    path: "f".into(),
                    pattern: "p".into(),
                    regex: false,
                    case_insensitive: false,
                    multiline: false,
                    invert_match: false,
                    context: None,
                    before_context: None,
                    after_context: None,
                    assert_count: None,
                    literal: false,
                    globs: vec![],
                    max_results: 0,
                    exclude_patterns: vec![],
                    custom_ignore_filenames: vec![],
                },
            ),
            (
                "read",
                Operation::Read {
                    path: "f".into(),
                    lines: None,
                },
            ),
        ];

        #[cfg(feature = "ast")]
        {
            ops.push((
                "ast.rename",
                Operation::AstRename {
                    path: "f.rs".into(),
                    old_name: "old".into(),
                    new_name: "new".into(),
                    lang: None,
                },
            ));
            ops.push((
                "ast.replace",
                Operation::AstReplace {
                    path: "f.rs".into(),
                    symbol: "s".into(),
                    from: "a".into(),
                    to: "b".into(),
                    regex: false,
                    lang: None,
                },
            ));
        }

        let schemas = operation_schemas();
        let schema_map: std::collections::HashMap<&str, &OperationSchema> =
            schemas.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut missing: Vec<String> = Vec::new();

        for (op_name, op_value) in &ops {
            let schema = match schema_map.get(op_name) {
                Some(s) => s,
                None => {
                    missing.push(format!("{op_name}: no schema found"));
                    continue;
                }
            };
            let props = schema
                .parameters
                .get("properties")
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| {
                    panic!("schema {op_name}: parameters.properties is not an object")
                });

            // Serialize the Operation to get its field names.
            let serialized = serde_json::to_value(op_value).unwrap();
            let obj = serialized.as_object().unwrap();
            for key in obj.keys() {
                if key == "op" {
                    continue; // tag field, not a parameter
                }
                if !props.contains_key(key) {
                    missing.push(format!(
                        "{op_name}: plan.rs field '{key}' missing from schema properties"
                    ));
                }
            }
        }

        assert!(
            missing.is_empty(),
            "schema/plan drift detected:\n  {}",
            missing.join("\n  ")
        );
    }

    // Static assertion: types must be Send + Sync.
    const _: () = {
        fn _assert<T: Send + Sync>() {}
        let _ = _assert::<Tier>;
        let _ = _assert::<OperationSchema>;
        let _ = _assert::<OperationExample>;
    };
}
