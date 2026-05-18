//! Transaction plan format parsing.

use serde::{Deserialize, Serialize};

/// A transaction plan containing multiple operations to execute atomically.
#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub cwd: Option<String>,
    pub write_policy: Option<PlanWritePolicy>,
    #[serde(default)]
    pub strict: bool,
    pub operations: Vec<Operation>,
    pub format: Option<Vec<FormatStep>>,
    pub validate: Option<Vec<ValidationStep>>,
}

/// A format step to run after applying operations but before validation.
#[derive(Debug, Deserialize, Serialize)]
pub struct FormatStep {
    pub cmd: String,
    /// Timeout in seconds (default: 60).
    pub timeout: Option<u64>,
}

/// Write policy settings specified in the plan.
#[derive(Debug, Deserialize, Serialize)]
pub struct PlanWritePolicy {
    pub ensure_final_newline: Option<bool>,
    pub normalize_eol: Option<String>,
    pub trim_trailing_whitespace: Option<bool>,
}

/// A single operation within a plan.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op")]
pub enum Operation {
    #[serde(rename = "replace")]
    Replace {
        glob: Option<String>,
        path: Option<String>,
        mode: Option<String>,
        from: String,
        to: Option<String>,
        nth: Option<usize>,
        insert_before: Option<String>,
        insert_after: Option<String>,
        #[serde(default)]
        case_insensitive: bool,
        #[serde(default)]
        multiline: bool,
        #[serde(default)]
        if_exists: bool,
    },
    #[serde(rename = "doc.set")]
    DocSet {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete")]
    DocDelete {
        path: String,
        #[serde(alias = "selector")]
        key: String,
    },
    #[serde(rename = "doc.merge")]
    DocMerge {
        path: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.append")]
    DocAppend {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.prepend")]
    DocPrepend {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.update")]
    DocUpdate {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.move")]
    DocMove {
        path: String,
        from: String,
        to: String,
    },
    #[serde(rename = "doc.ensure")]
    DocEnsure {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete_where")]
    DocDeleteWhere {
        path: String,
        #[serde(alias = "selector")]
        key: String,
        predicate: String,
    },
    #[serde(rename = "md.replace_section")]
    MdReplaceSection {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.insert_after_heading")]
    MdInsertAfterHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.insert_before_heading")]
    MdInsertBeforeHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.upsert_bullet")]
    MdUpsertBullet {
        path: String,
        heading: String,
        bullet: String,
    },
    #[serde(rename = "md.table_append")]
    MdTableAppend {
        path: String,
        heading: String,
        row: String,
    },
    #[serde(rename = "md.dedupe_headings")]
    MdDedupeHeadings { path: String },
    #[serde(rename = "hygiene.fix")]
    HygieneFix {
        path: String,
        ensure_final_newline: Option<bool>,
        trim_trailing_whitespace: Option<bool>,
        normalize_eol: Option<String>,
    },
    #[serde(rename = "file.create")]
    FileCreate {
        path: String,
        content: String,
        force: Option<bool>,
    },
    #[serde(rename = "file.delete")]
    FileDelete { path: String },
    #[serde(rename = "patch.apply")]
    PatchApply {
        /// Inline diff text to apply.
        diff: String,
    },
    #[serde(rename = "search")]
    Search {
        path: String,
        pattern: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        case_insensitive: bool,
        context: Option<usize>,
        before_context: Option<usize>,
        after_context: Option<usize>,
    },
    #[serde(rename = "read")]
    Read {
        path: String,
        /// Optional line range (e.g., "10:25").
        lines: Option<String>,
    },
}

/// A validation step to run after applying operations.
#[derive(Debug, Deserialize, Serialize)]
pub struct ValidationStep {
    pub cmd: String,
    pub required: Option<bool>,
    /// Timeout in seconds (default: 60).
    pub timeout: Option<u64>,
}

/// Parse a plan from a JSON string.
pub fn parse_plan(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = serde_json::from_str(input)?;
    Ok(plan)
}

/// Parse a plan from a YAML string.
pub fn parse_plan_yaml(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = serde_yaml_ng::from_str(input)?;
    Ok(plan)
}

/// Parse a plan from a TOML string.
pub fn parse_plan_toml(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = toml_edit::de::from_str(input)?;
    Ok(plan)
}

/// Detect plan format from a file path extension and parse accordingly.
pub fn parse_plan_auto(
    input: &str,
    path: Option<&str>,
    format_hint: Option<&str>,
) -> anyhow::Result<Plan> {
    let fmt = format_hint.or_else(|| {
        path.and_then(|p| {
            if p.ends_with(".yaml") || p.ends_with(".yml") {
                Some("yaml")
            } else if p.ends_with(".toml") {
                Some("toml")
            } else {
                None
            }
        })
    });
    match fmt {
        Some("yaml" | "yml") => parse_plan_yaml(input),
        Some("toml") => parse_plan_toml(input),
        _ => parse_plan(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_plan() {
        let json = r#"{"operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert!(plan.cwd.is_none());
        assert!(plan.write_policy.is_none());
        assert!(plan.validate.is_none());
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_with_all_fields() {
        let json = r#"{
            "cwd": "/tmp",
            "write_policy": {"ensure_final_newline": true, "normalize_eol": "lf"},
            "operations": [{"op": "file.create", "path": "f.txt", "content": "hi"}],
            "validate": [{"cmd": "echo ok"}]
        }"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.cwd.as_deref(), Some("/tmp"));
        let wp = plan.write_policy.unwrap();
        assert_eq!(wp.ensure_final_newline, Some(true));
        assert_eq!(wp.normalize_eol.as_deref(), Some("lf"));
        assert!(plan.validate.unwrap()[0].required.is_none());
    }

    #[test]
    fn parse_doc_operation_selector_alias_maps_to_key_field() {
        let json = r#"{
            "operations": [
                {"op": "doc.set", "path": "f.json", "selector": "nested.key", "value": 1},
                {"op": "doc.delete", "path": "f.json", "selector": "nested.key"},
                {"op": "doc.append", "path": "f.json", "selector": "items", "value": 1},
                {"op": "doc.prepend", "path": "f.json", "selector": "items", "value": 0},
                {"op": "doc.update", "path": "f.json", "selector": "items[*].status", "value": "done"},
                {"op": "doc.ensure", "path": "f.json", "selector": "defaults.enabled", "value": true},
                {"op": "doc.delete_where", "path": "f.json", "selector": "items", "predicate": "name=x"}
            ]
        }"#;
        let plan = parse_plan(json).unwrap();

        assert!(matches!(
            &plan.operations[0],
            Operation::DocSet { key, .. } if key == "nested.key"
        ));
        assert!(matches!(
            &plan.operations[1],
            Operation::DocDelete { key, .. } if key == "nested.key"
        ));
        assert!(matches!(
            &plan.operations[2],
            Operation::DocAppend { key, .. } if key == "items"
        ));
        assert!(matches!(
            &plan.operations[3],
            Operation::DocPrepend { key, .. } if key == "items"
        ));
        assert!(matches!(
            &plan.operations[4],
            Operation::DocUpdate { key, .. } if key == "items[*].status"
        ));
        assert!(matches!(
            &plan.operations[5],
            Operation::DocEnsure { key, .. } if key == "defaults.enabled"
        ));
        assert!(matches!(
            &plan.operations[6],
            Operation::DocDeleteWhere { key, .. } if key == "items"
        ));
    }

    #[test]
    fn parse_plan_unknown_op_fails() {
        let json = r#"{"operations": [{"op": "unknown", "x": 1}]}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn parse_plan_missing_operations_fails() {
        let json = r#"{"cwd": "/tmp"}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn parse_all_operation_variants() {
        let json = r#"{"operations": [
            {"op": "replace", "from": "a", "to": "b"},
            {"op": "replace", "from": "a", "to": "b", "nth": 2},
            {"op": "doc.set", "path": "f.json", "key": "k", "value": 1},
            {"op": "doc.delete", "path": "f.json", "key": "k"},
            {"op": "doc.merge", "path": "f.json", "value": {}},
            {"op": "doc.append", "path": "f.json", "key": "arr", "value": 1},
            {"op": "doc.prepend", "path": "f.json", "key": "arr", "value": 0},
            {"op": "doc.update", "path": "f.json", "key": "k", "value": 2},
            {"op": "doc.move", "path": "f.json", "from": "a", "to": "b"},
            {"op": "doc.ensure", "path": "f.json", "key": "k", "value": 1},
            {"op": "doc.delete_where", "path": "f.json", "key": "arr", "predicate": "name=x"},
            {"op": "md.replace_section", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_after_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_before_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.upsert_bullet", "path": "f.md", "heading": "H", "bullet": "- item"},
            {"op": "md.table_append", "path": "f.md", "heading": "H", "row": "| a | b |"},
            {"op": "md.dedupe_headings", "path": "f.md"},
            {"op": "hygiene.fix", "path": "f.txt"},
            {"op": "hygiene.fix", "path": "f.txt", "trim_trailing_whitespace": true, "normalize_eol": "lf"},
            {"op": "file.create", "path": "f.txt", "content": "c"},
            {"op": "file.create", "path": "g.txt", "content": "c", "force": true},
            {"op": "file.delete", "path": "f.txt"},
            {"op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b"},
            {"op": "read", "path": "f.txt"},
            {"op": "read", "path": "f.txt", "lines": "1:10"},
            {"op": "search", "path": "f.txt", "pattern": "hello"},
            {"op": "search", "path": "f.txt", "pattern": "he.*o", "regex": true, "case_insensitive": true}
        ]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.operations.len(), 27);
    }

    #[test]
    fn parse_plan_with_format_steps() {
        let json = r#"{
            "operations": [],
            "format": [{"cmd": "cargo fmt"}],
            "validate": [{"cmd": "make check"}]
        }"#;
        let plan = parse_plan(json).unwrap();
        let fmt = plan.format.unwrap();
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].cmd, "cargo fmt");
    }

    // ── YAML / TOML / auto-detect ─────────────────────────────────

    #[test]
    fn parse_plan_yaml_basic() {
        let yaml = "operations:\n  - op: replace\n    from: old\n    to: new\n";
        let plan = parse_plan_yaml(yaml).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            &plan.operations[0],
            Operation::Replace { from, to, .. } if from == "old" && to.as_deref() == Some("new")
        ));
    }

    #[test]
    fn parse_plan_toml_basic() {
        let toml = "[[operations]]\nop = \"replace\"\nfrom = \"old\"\nto = \"new\"\n";
        let plan = parse_plan_toml(toml).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            &plan.operations[0],
            Operation::Replace { from, to, .. } if from == "old" && to.as_deref() == Some("new")
        ));
    }

    #[test]
    fn parse_plan_auto_detects_yaml() {
        let yaml = "operations:\n  - op: replace\n    from: a\n    to: b\n";
        let plan = parse_plan_auto(yaml, Some("plan.yaml"), None).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_auto_format_hint_overrides_extension() {
        let yaml = "operations:\n  - op: replace\n    from: a\n    to: b\n";
        // Extension says .json but hint says yaml.
        let plan = parse_plan_auto(yaml, Some("plan.json"), Some("yaml")).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_auto_defaults_to_json() {
        let json = r#"{"operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan_auto(json, Some("plan.txt"), None).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_strict_and_all_policy_fields() {
        let json = r#"{
            "strict": true,
            "write_policy": {
                "ensure_final_newline": true,
                "normalize_eol": "crlf",
                "trim_trailing_whitespace": true
            },
            "operations": [],
            "format": [{"cmd": "fmt", "timeout": 30}],
            "validate": [{"cmd": "check", "required": true, "timeout": 120}]
        }"#;
        let plan = parse_plan(json).unwrap();
        assert!(plan.strict);
        let wp = plan.write_policy.unwrap();
        assert_eq!(wp.ensure_final_newline, Some(true));
        assert_eq!(wp.normalize_eol.as_deref(), Some("crlf"));
        assert_eq!(wp.trim_trailing_whitespace, Some(true));
        let fmt = &plan.format.unwrap()[0];
        assert_eq!(fmt.timeout, Some(30));
        let val = &plan.validate.unwrap()[0];
        assert_eq!(val.required, Some(true));
        assert_eq!(val.timeout, Some(120));
    }
}
