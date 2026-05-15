//! Transaction plan format parsing.

use serde::Deserialize;

/// A transaction plan containing multiple operations to execute atomically.
#[derive(Debug, Deserialize)]
pub struct Plan {
    pub cwd: Option<String>,
    pub write_policy: Option<PlanWritePolicy>,
    pub operations: Vec<Operation>,
    pub validate: Option<Vec<ValidationStep>>,
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
    #[serde(rename = "hygiene.fix")]
    HygieneFix {
        path: String,
        ensure_final_newline: Option<bool>,
    },
    #[serde(rename = "file.create")]
    FileCreate { path: String, content: String },
    #[serde(rename = "file.delete")]
    FileDelete { path: String },
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
