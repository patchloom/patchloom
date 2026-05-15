//! Transaction plan format parsing.

use serde::Deserialize;

/// A transaction plan containing multiple operations to execute atomically.
#[derive(Debug, Deserialize)]
pub struct Plan {
    pub cwd: Option<String>,
    pub write_policy: Option<PlanWritePolicy>,
    pub operations: Vec<Operation>,
    pub format: Option<Vec<FormatStep>>,
    pub validate: Option<Vec<ValidationStep>>,
}

/// A format step to run after applying operations but before validation.
#[derive(Debug, Deserialize)]
pub struct FormatStep {
    pub cmd: String,
}

/// Write policy settings specified in the plan.
#[derive(Debug, Deserialize)]
pub struct PlanWritePolicy {
    pub ensure_final_newline: Option<bool>,
    pub normalize_eol: Option<String>,
    pub trim_trailing_whitespace: Option<bool>,
}

/// A single operation within a plan.
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum Operation {
    #[serde(rename = "replace")]
    Replace {
        glob: Option<String>,
        path: Option<String>,
        mode: Option<String>,
        from: String,
        to: String,
        nth: Option<usize>,
    },
    #[serde(rename = "doc.set")]
    DocSet {
        path: String,
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete")]
    DocDelete { path: String, key: String },
    #[serde(rename = "doc.merge")]
    DocMerge {
        path: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.append")]
    DocAppend {
        path: String,
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.prepend")]
    DocPrepend {
        path: String,
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.update")]
    DocUpdate {
        path: String,
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
        key: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete_where")]
    DocDeleteWhere {
        path: String,
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
}

/// A validation step to run after applying operations.
#[derive(Debug, Deserialize)]
pub struct ValidationStep {
    pub cmd: String,
    pub required: Option<bool>,
}

/// Parse a plan from a JSON string.
pub fn parse_plan(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = serde_json::from_str(input)?;
    Ok(plan)
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
            {"op": "file.create", "path": "f.txt", "content": "c"},
            {"op": "file.create", "path": "g.txt", "content": "c", "force": true},
            {"op": "file.delete", "path": "f.txt"},
            {"op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b"}
        ]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.operations.len(), 22);
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
}
