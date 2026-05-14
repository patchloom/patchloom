// Output rendering: json, jsonl, diff, human modes.

use serde::Serialize;

/// Selects how results are rendered to stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
    Jsonl,
    Diff,
}

/// A successful operation result.
#[derive(Debug, Clone, Serialize)]
pub struct SuccessResult {
    pub ok: bool,
    pub mode: String,
    pub files_touched: usize,
    pub operations_run: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    pub validation: Vec<ValidationResult>,
}

/// Result of a single validation command.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    pub cmd: String,
    pub exit_code: i32,
    pub required: bool,
}

/// An error result.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResult {
    pub ok: bool,
    pub error_code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Render a [`SuccessResult`] in the given [`OutputMode`].
pub fn render_success(result: &SuccessResult, mode: &OutputMode) -> String {
    match mode {
        OutputMode::Human => {
            let mut out = format!(
                "✔ {} complete — {} file(s), {} operation(s)",
                result.mode, result.files_touched, result.operations_run,
            );
            for v in &result.validation {
                let status = if v.exit_code == 0 { "pass" } else { "FAIL" };
                out.push_str(&format!(
                    "\n  validate: {} → {} (exit {})",
                    v.cmd, status, v.exit_code,
                ));
            }
            out
        }
        OutputMode::Json => serde_json::to_string_pretty(result).expect("serialize success"),
        OutputMode::Jsonl => serde_json::to_string(result).expect("serialize success"),
        OutputMode::Diff => result.diff.clone().unwrap_or_default(),
    }
}

/// Render an [`ErrorResult`] in the given [`OutputMode`].
pub fn render_error(result: &ErrorResult, mode: &OutputMode) -> String {
    match mode {
        OutputMode::Human => {
            let mut out = format!("✘ error [{}]: {}", result.error_code, result.message);
            if let Some(details) = &result.details {
                out.push_str(&format!(
                    "\n  details: {}",
                    serde_json::to_string_pretty(details).expect("serialize details"),
                ));
            }
            out
        }
        OutputMode::Json => serde_json::to_string_pretty(result).expect("serialize error"),
        OutputMode::Jsonl => serde_json::to_string(result).expect("serialize error"),
        OutputMode::Diff => format!("error: {}", result.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_success() -> SuccessResult {
        SuccessResult {
            ok: true,
            mode: "apply".to_string(),
            files_touched: 3,
            operations_run: 5,
            diff: Some("--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-old\n+new".to_string()),
            validation: vec![ValidationResult {
                cmd: "cargo test".to_string(),
                exit_code: 0,
                required: true,
            }],
        }
    }

    fn sample_error() -> ErrorResult {
        ErrorResult {
            ok: false,
            error_code: "PARSE_ERROR".to_string(),
            message: "unexpected token".to_string(),
            details: None,
        }
    }

    #[test]
    fn success_json_contains_ok_true() {
        let out = render_success(&sample_success(), &OutputMode::Json);
        assert!(out.contains("\"ok\": true"), "expected ok:true in: {out}");
    }

    #[test]
    fn success_jsonl_is_single_line() {
        let out = render_success(&sample_success(), &OutputMode::Jsonl);
        assert_eq!(out.lines().count(), 1, "JSONL must be a single line");
        // Also verify it is valid JSON.
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
    }

    #[test]
    fn success_human_contains_file_count() {
        let out = render_success(&sample_success(), &OutputMode::Human);
        assert!(out.contains("3 file(s)"), "expected file count in: {out}",);
    }

    #[test]
    fn error_json_contains_ok_false() {
        let out = render_error(&sample_error(), &OutputMode::Json);
        assert!(out.contains("\"ok\": false"), "expected ok:false in: {out}",);
    }

    #[test]
    fn success_diff_mode_prints_diff() {
        let result = sample_success();
        let out = render_success(&result, &OutputMode::Diff);
        assert!(out.contains("--- a/foo"), "expected diff header in: {out}");
        assert!(out.contains("+new"), "expected diff content in: {out}");
    }
}
