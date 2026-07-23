//! Exit codes for patchloom.

#![cfg_attr(not(feature = "cli"), allow(dead_code))]

/// Command completed successfully.
pub const SUCCESS: u8 = 0;
/// Unrecoverable error (I/O failure, invalid arguments, etc.).
pub const FAILURE: u8 = 1;
/// Write command detected pending changes (`--check` mode).
pub const CHANGES_DETECTED: u8 = 2;
/// Search or replace found zero matches.
pub const NO_MATCHES: u8 = 3;

/// Typed error for no-match conditions in tx engine operations.
///
/// Commands check for this via `anyhow::Error::downcast_ref::<NoMatchError>()`
/// instead of fragile `msg.contains(...)` string matching on error messages.
/// This decouples exit-code classification from error message wording.
#[derive(Debug)]
pub struct NoMatchError {
    pub msg: String,
}

impl std::fmt::Display for NoMatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for NoMatchError {}

/// Check whether an `anyhow::Error` chain contains a `NoMatchError`.
///
/// The tx engine wraps operation errors with context (e.g.
/// "operation 1 (doc.update) failed"), so the `NoMatchError` may not be
/// the top-level error. This walks the full chain via `anyhow::Chain`.
pub fn is_no_match(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<NoMatchError>().is_some())
}

/// Typed error for ambiguous multi-match conditions in tx engine operations.
///
/// Parallel to [`NoMatchError`]: lets `tx` map `unique` multi-match failures
/// to exit code [`AMBIGUOUS`] (5) instead of generic [`OPERATION_FAILED`] (9).
#[derive(Debug)]
pub struct AmbiguousError {
    pub msg: String,
}

impl std::fmt::Display for AmbiguousError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for AmbiguousError {}

/// Check whether an `anyhow::Error` chain contains an [`AmbiguousError`].
pub fn is_ambiguous(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<AmbiguousError>().is_some())
}

/// Typed error for invalid CLI/input conditions that map to exit
/// [`FAILURE`] (1) with JSON `error_kind: "invalid_input"`.
///
/// Used for `--contain` path rejections and empty-path validation so the
/// global `--json` dispatch path does not require English string matching.
#[derive(Debug)]
pub struct InvalidInputError {
    pub msg: String,
}

impl std::fmt::Display for InvalidInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for InvalidInputError {}

/// Check whether an `anyhow::Error` chain contains an [`InvalidInputError`].
pub fn is_invalid_input(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<InvalidInputError>().is_some())
}

/// True when any cause is an IO `NotFound` (missing file/dir).
pub fn is_io_not_found(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    })
}

/// Typed error for create/rename conflicts that map to exit [`FAILURE`] (1)
/// with JSON `error_kind: "already_exists"`.
#[derive(Debug)]
pub struct AlreadyExistsError {
    pub msg: String,
}

impl std::fmt::Display for AlreadyExistsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for AlreadyExistsError {}

/// Check whether an `anyhow::Error` chain contains an [`AlreadyExistsError`].
pub fn is_already_exists(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<AlreadyExistsError>().is_some())
}

/// Typed error for doc type mismatches that map to exit [`FAILURE`] (1)
/// with JSON `error_kind: "type_error"`.
#[derive(Debug)]
pub struct TypeErrorError {
    pub msg: String,
}

impl std::fmt::Display for TypeErrorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for TypeErrorError {}

/// Check whether an `anyhow::Error` chain contains a [`TypeErrorError`].
pub fn is_type_error(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<TypeErrorError>().is_some())
}

/// Typed error for patch merge conflicts that map to exit [`CONFLICTS`] (8)
/// with JSON `error_kind: "conflicts"`.
#[derive(Debug)]
pub struct ConflictsError {
    pub msg: String,
}

impl std::fmt::Display for ConflictsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ConflictsError {}

/// Check whether an `anyhow::Error` chain contains a [`ConflictsError`].
pub fn is_conflicts(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<ConflictsError>().is_some())
}

/// Typed error for plan/patch/document parse failures that map to exit
/// [`PARSE_ERROR`] (4) with JSON `error_kind: "parse_error"`.
#[derive(Debug)]
pub struct ParseErrorError {
    pub msg: String,
}

impl std::fmt::Display for ParseErrorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ParseErrorError {}

/// Check whether an `anyhow::Error` chain contains a [`ParseErrorError`].
pub fn is_parse_error(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<ParseErrorError>().is_some())
}

/// Typed error for assert-count / soft mismatch that map to exit
/// [`CHANGES_DETECTED`] (2) with JSON `error_kind: "changes_detected"`.
/// Matches CLI `search --assert-count` when the actual count differs.
#[derive(Debug)]
pub struct ChangesDetectedError {
    pub msg: String,
}

impl std::fmt::Display for ChangesDetectedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ChangesDetectedError {}

/// Check whether an `anyhow::Error` chain contains a [`ChangesDetectedError`].
pub fn is_changes_detected(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<ChangesDetectedError>().is_some())
}

/// Typed error for post-write `--format` / format-step failures that map to
/// exit [`FAILURE`] (1) with JSON `error_kind: "format_failed"`.
///
/// Matches plan/tx lifecycle `format_failed` kind so agents can branch without
/// scraping "format command failed" English. Exit stays 1 (not 9): files may
/// already be written; recovery is `undo` or re-run the formatter.
///
/// When writes already committed, [`Self::backup_session`] carries the undo
/// session id so CLI `--json` can expose `backup_session` (same honesty as
/// non-strict tx format/validate failures).
#[derive(Debug)]
pub struct FormatFailedError {
    pub msg: String,
    /// Backup session created for the write that preceded format failure.
    pub backup_session: Option<String>,
    /// Paths already written before format failed (#1795). Agents need this
    /// without re-reading disk or parsing English `error` text.
    pub written_files: Vec<String>,
}

impl FormatFailedError {
    /// Format failure without a known backup session.
    #[must_use]
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            backup_session: None,
            written_files: Vec::new(),
        }
    }

    /// Attach a backup session (files already on disk; agent can `undo`).
    #[must_use]
    pub fn with_backup_session(mut self, session: Option<String>) -> Self {
        self.backup_session = session.filter(|s| !s.is_empty());
        self
    }

    /// Paths that were committed before the format hook failed.
    #[must_use]
    pub fn with_written_files(mut self, files: Vec<String>) -> Self {
        self.written_files = files;
        self
    }
}

impl std::fmt::Display for FormatFailedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.backup_session {
            Some(ts) => write!(
                f,
                "{} (backup session {ts}; run `patchloom undo` to restore)",
                self.msg
            ),
            None => f.write_str(&self.msg),
        }
    }
}

impl std::error::Error for FormatFailedError {}

/// Check whether an `anyhow::Error` chain contains a [`FormatFailedError`].
pub fn is_format_failed(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<FormatFailedError>().is_some())
}

/// First [`FormatFailedError::backup_session`] found in the error chain.
#[must_use]
pub fn format_failed_backup_session(err: &anyhow::Error) -> Option<&str> {
    err.chain().find_map(|cause| {
        cause
            .downcast_ref::<FormatFailedError>()
            .and_then(|f| f.backup_session.as_deref())
    })
}

/// Written file paths from the first [`FormatFailedError`] in the chain (#1795).
#[must_use]
pub fn format_failed_written_files(err: &anyhow::Error) -> Vec<String> {
    err.chain()
        .find_map(|cause| {
            cause
                .downcast_ref::<FormatFailedError>()
                .map(|f| f.written_files.clone())
        })
        .unwrap_or_default()
}

/// True when any cause is an IO `IsADirectory` (path exists but is a directory).
#[must_use]
pub fn is_io_is_a_directory(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::IsADirectory)
    })
}

/// Format an error for agent-facing surfaces (JSON `error`, typed msg fields).
///
/// Prefer [`Display`] when the outer context already embeds every cause message
/// (common after `context(format!("…: {e}"))` on IO errors). In that case
/// `{:#}` only doubles the OS detail (e.g. `…: No such file …: No such file …`).
/// Otherwise use the full chain (`{:#}`) so multi-layer context stays complete.
///
/// [`Display`]: std::fmt::Display
#[must_use]
pub fn agent_error_message(err: &anyhow::Error) -> String {
    let top = err.to_string();
    let mut complete = true;
    for cause in err.chain().skip(1) {
        let c = cause.to_string();
        if !c.is_empty() && !top.contains(&c) {
            complete = false;
            break;
        }
    }
    if complete { top } else { format!("{err:#}") }
}

/// Build the shared JSON error envelope + exit code for agent `--json` paths.
///
/// Includes `error_kind` when classifiable and `backup_session` when a
/// post-write format failure carries one.
#[must_use]
pub fn structured_error_payload(err: &anyhow::Error) -> (serde_json::Value, u8) {
    let (kind, code) = match classify_typed_error(err) {
        Some((k, c)) => (Some(k), c),
        None => (None, FAILURE),
    };
    let mut output = serde_json::json!({
        "ok": false,
        "error": agent_error_message(err)
    });
    if let Some(k) = kind {
        output["error_kind"] = serde_json::Value::String(k.to_string());
    }
    if let Some(bs) = format_failed_backup_session(err) {
        output["backup_session"] = serde_json::Value::String(bs.to_string());
    }
    let written = format_failed_written_files(err);
    if !written.is_empty() {
        // Canonical with other mutators (#1831 / #1788): agents branch on
        // `applied`. Keep `write_applied` as a deprecated alias for one release.
        output["applied"] = serde_json::Value::Bool(true);
        output["write_applied"] = serde_json::Value::Bool(true);
        output["files_changed"] = serde_json::Value::Number(written.len().into());
        output["files"] = serde_json::Value::Array(
            written
                .into_iter()
                .map(|path| serde_json::json!({ "path": path }))
                .collect(),
        );
    } else if kind.is_some_and(error_kind_implies_not_applied)
        || matches!(kind, Some("format_failed"))
    {
        // Pre-write failures never mutate disk (#1835): agents branch on
        // `applied` alone. Also cover not_found / already_exists / parse_error
        // (CLI emit_error_json_kind parity). format_failed with no written
        // paths is applied:false (write did not land).
        output["applied"] = serde_json::Value::Bool(false);
    }
    (output, code)
}

/// Error kinds that mean no successful write landed on disk.
///
/// Used by [`structured_error_payload`] and CLI
/// [`crate::cli::global::GlobalFlags::emit_error_json_kind`] so agents can
/// branch on `applied: false` without enumerating every kind.
#[must_use]
pub fn error_kind_implies_not_applied(kind: &str) -> bool {
    matches!(
        kind,
        "no_matches"
            | "ambiguous"
            | "invalid_input"
            | "not_found"
            | "already_exists"
            | "type_error"
            | "conflicts"
            | "parse_error"
            | "changes_detected"
    )
}

/// Classify a typed error for JSON `error_kind` + exit code.
///
/// Shared by global `--json` dispatch and command remappers (e.g. doc write)
/// so new kinds cannot be dropped in one path and present in another.
/// Returns `None` when the chain has no recognized typed kind.
pub fn classify_typed_error(err: &anyhow::Error) -> Option<(&'static str, u8)> {
    if is_no_match(err) {
        Some(("no_matches", NO_MATCHES))
    } else if is_ambiguous(err) {
        Some(("ambiguous", AMBIGUOUS))
    } else if is_invalid_input(err) {
        Some(("invalid_input", FAILURE))
    } else if is_io_not_found(err) {
        Some(("not_found", FAILURE))
    } else if is_io_is_a_directory(err) {
        // Path exists but is a directory (or non-file). Not not_found: agents
        // must not retry as create; use invalid_input like API file ops.
        Some(("invalid_input", FAILURE))
    } else if is_already_exists(err) {
        Some(("already_exists", FAILURE))
    } else if is_type_error(err) {
        Some(("type_error", FAILURE))
    } else if is_conflicts(err) {
        Some(("conflicts", CONFLICTS))
    } else if is_parse_error(err) {
        Some(("parse_error", PARSE_ERROR))
    } else if is_changes_detected(err) {
        Some(("changes_detected", CHANGES_DETECTED))
    } else if is_format_failed(err) {
        Some(("format_failed", FAILURE))
    } else {
        None
    }
}

/// Plan, patch, or structured document could not be parsed.
pub const PARSE_ERROR: u8 = 4;
/// Multiple candidates matched and the command could not pick one.
pub const AMBIGUOUS: u8 = 5;
/// A `validate` step failed (tx lifecycle).
pub const VALIDATION_FAILED: u8 = 6;
/// Strict-mode rollback triggered by a failed `format` or `validate` step.
pub const ROLLBACK: u8 = 7;
/// Patch merge produced conflict markers.
pub const CONFLICTS: u8 = 8;
/// Tx operation staging failure (`error_kind`: `operation_failed`).
pub const OPERATION_FAILED: u8 = 9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_typed_error_maps_format_failed() {
        let err: anyhow::Error = FormatFailedError::new("format command failed").into();
        let wrapped = err.context("files were written but formatting failed");
        assert_eq!(
            classify_typed_error(&wrapped),
            Some(("format_failed", FAILURE))
        );
    }

    #[test]
    fn structured_error_payload_includes_format_backup_session() {
        let err: anyhow::Error = FormatFailedError::new("format command failed (false)")
            .with_backup_session(Some("123_0".into()))
            .into();
        let (payload, code) = structured_error_payload(&err);
        assert_eq!(code, FAILURE);
        assert_eq!(payload["error_kind"], "format_failed");
        assert_eq!(payload["backup_session"], "123_0");
        assert!(
            payload["error"]
                .as_str()
                .unwrap_or("")
                .contains("backup session 123_0"),
            "display should mention undo session: {payload}"
        );
    }

    #[test]
    fn structured_error_payload_includes_format_written_files() {
        let err: anyhow::Error = FormatFailedError::new("format command failed (false)")
            .with_backup_session(Some("123_0".into()))
            .with_written_files(vec!["a.txt".into(), "b.txt".into()])
            .into();
        let (payload, code) = structured_error_payload(&err);
        assert_eq!(code, FAILURE);
        assert_eq!(payload["error_kind"], "format_failed");
        assert_eq!(
            payload["applied"], true,
            "format_failed must set applied when write landed (#1831): {payload}"
        );
        assert_eq!(
            payload["write_applied"], true,
            "write_applied kept as deprecated alias: {payload}"
        );
        assert_eq!(payload["files_changed"], 2);
        assert_eq!(payload["files"][0]["path"], "a.txt");
        assert_eq!(payload["files"][1]["path"], "b.txt");
        assert_eq!(
            format_failed_written_files(&err),
            vec!["a.txt".to_string(), "b.txt".to_string()]
        );
    }

    #[test]
    fn structured_error_payload_prewrite_no_matches_and_ambiguous_set_applied_false() {
        let no_match: anyhow::Error = NoMatchError {
            msg: "no matches".into(),
        }
        .into();
        let (p, code) = structured_error_payload(&no_match);
        assert_eq!(code, NO_MATCHES);
        assert_eq!(p["error_kind"], "no_matches");
        assert_eq!(
            p["applied"], false,
            "pre-write no_matches must set applied:false (#1835): {p}"
        );

        let amb: anyhow::Error = AmbiguousError {
            msg: "many matches".into(),
        }
        .into();
        let (p, code) = structured_error_payload(&amb);
        assert_eq!(code, AMBIGUOUS);
        assert_eq!(p["error_kind"], "ambiguous");
        assert_eq!(
            p["applied"], false,
            "pre-write ambiguous must set applied:false (#1835): {p}"
        );
    }

    #[test]
    fn structured_error_payload_prewrite_already_exists_and_not_found_set_applied_false() {
        let exists: anyhow::Error = AlreadyExistsError {
            msg: "file already exists".into(),
        }
        .into();
        let (p, code) = structured_error_payload(&exists);
        assert_eq!(code, FAILURE);
        assert_eq!(p["error_kind"], "already_exists");
        assert_eq!(
            p["applied"], false,
            "pre-write already_exists must set applied:false: {p}"
        );

        let missing: anyhow::Error =
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such file").into();
        let (p, code) = structured_error_payload(&missing);
        assert_eq!(code, FAILURE);
        assert_eq!(p["error_kind"], "not_found");
        assert_eq!(
            p["applied"], false,
            "pre-write not_found must set applied:false: {p}"
        );
    }

    #[test]
    fn agent_error_message_avoids_double_os_when_context_embeds_cause() {
        // Mirrors load_text_strict NotFound: context already includes `{e}`.
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let msg = format!("failed to read missing.txt: {io}");
        let err = anyhow::Error::new(io).context(msg);
        let agent = agent_error_message(&err);
        assert!(
            agent.contains("failed to read missing.txt"),
            "path context missing: {agent}"
        );
        assert!(agent.contains("No such file"), "OS detail missing: {agent}");
        assert_eq!(
            agent.matches("No such file").count(),
            1,
            "OS detail must not double under agent_error_message: {agent}"
        );
        // {:#} still doubles — that is what we are avoiding.
        let raw = format!("{err:#}");
        assert!(
            raw.matches("No such file").count() >= 2,
            "test premise: raw {{:#}} should double: {raw}"
        );
        let (payload, _) = structured_error_payload(&err);
        let json_err = payload["error"].as_str().unwrap_or("");
        assert_eq!(
            json_err.matches("No such file").count(),
            1,
            "structured_error_payload must not double OS detail: {json_err}"
        );
    }

    #[test]
    fn agent_error_message_keeps_multi_layer_chain_when_causes_add_info() {
        let root = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let err = anyhow::Error::new(root)
            .context("failed to read plan.yaml")
            .context("operation 1 (doc.get) failed");
        let agent = agent_error_message(&err);
        assert!(
            agent.contains("operation 1"),
            "outer context missing: {agent}"
        );
        assert!(
            agent.contains("failed to read plan.yaml"),
            "middle context missing: {agent}"
        );
        assert!(
            agent.contains("No such file"),
            "root OS detail missing: {agent}"
        );
    }

    #[test]
    fn error_kind_implies_not_applied_covers_prewrite_kinds() {
        assert!(error_kind_implies_not_applied("already_exists"));
        assert!(error_kind_implies_not_applied("not_found"));
        assert!(error_kind_implies_not_applied("parse_error"));
        assert!(!error_kind_implies_not_applied("format_failed"));
    }

    #[test]
    fn classify_typed_error_maps_is_a_directory() {
        let err: anyhow::Error =
            std::io::Error::new(std::io::ErrorKind::IsADirectory, "is a directory").into();
        let wrapped = err.context("failed to read /tmp/dir");
        assert_eq!(
            classify_typed_error(&wrapped),
            Some(("invalid_input", FAILURE))
        );
    }

    #[test]
    fn classify_typed_error_maps_all_kinds() {
        let cases: Vec<(anyhow::Error, &str, u8)> = vec![
            (
                NoMatchError { msg: "none".into() }.into(),
                "no_matches",
                NO_MATCHES,
            ),
            (
                AmbiguousError { msg: "many".into() }.into(),
                "ambiguous",
                AMBIGUOUS,
            ),
            (
                InvalidInputError { msg: "bad".into() }.into(),
                "invalid_input",
                FAILURE,
            ),
            (
                std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into(),
                "not_found",
                FAILURE,
            ),
            (
                AlreadyExistsError {
                    msg: "exists".into(),
                }
                .into(),
                "already_exists",
                FAILURE,
            ),
            (
                TypeErrorError { msg: "type".into() }.into(),
                "type_error",
                FAILURE,
            ),
            (
                ConflictsError {
                    msg: "conflict".into(),
                }
                .into(),
                "conflicts",
                CONFLICTS,
            ),
            (
                ParseErrorError {
                    msg: "parse".into(),
                }
                .into(),
                "parse_error",
                PARSE_ERROR,
            ),
            (
                ChangesDetectedError { msg: "diff".into() }.into(),
                "changes_detected",
                CHANGES_DETECTED,
            ),
            (
                FormatFailedError::new("fmt").into(),
                "format_failed",
                FAILURE,
            ),
            (
                std::io::Error::new(std::io::ErrorKind::IsADirectory, "dir").into(),
                "invalid_input",
                FAILURE,
            ),
        ];
        for (err, kind, code) in cases {
            assert_eq!(
                classify_typed_error(&err),
                Some((kind, code)),
                "kind={kind}"
            );
        }
    }

    #[test]
    fn classify_typed_error_none_for_plain() {
        let err = anyhow::anyhow!("plain");
        assert_eq!(classify_typed_error(&err), None);
    }

    #[test]
    fn exit_code_values() {
        assert_eq!(SUCCESS, 0);
        assert_eq!(FAILURE, 1);
        assert_eq!(CHANGES_DETECTED, 2);
        assert_eq!(NO_MATCHES, 3);
        assert_eq!(PARSE_ERROR, 4);
        assert_eq!(AMBIGUOUS, 5);
        assert_eq!(VALIDATION_FAILED, 6);
        assert_eq!(ROLLBACK, 7);
        assert_eq!(CONFLICTS, 8);
        assert_eq!(OPERATION_FAILED, 9);
    }

    #[test]
    fn no_match_error_downcast() {
        let err: anyhow::Error = NoMatchError {
            msg: "selector 'x' matched nothing".to_string(),
        }
        .into();
        assert!(
            err.downcast_ref::<NoMatchError>().is_some(),
            "NoMatchError should be downcastable from anyhow::Error"
        );
    }

    #[test]
    fn no_match_error_display() {
        let err = NoMatchError {
            msg: "test message".to_string(),
        };
        assert_eq!(err.to_string(), "test message");
    }

    #[test]
    fn non_no_match_error_not_downcast() {
        let err = anyhow::anyhow!("some other error");
        assert!(
            err.downcast_ref::<NoMatchError>().is_none(),
            "plain anyhow error should not downcast to NoMatchError"
        );
    }

    #[test]
    fn is_no_match_finds_wrapped_error() {
        let err: anyhow::Error = NoMatchError {
            msg: "selector matched nothing".to_string(),
        }
        .into();
        let wrapped = err.context("operation 1 (doc.update) failed");
        assert!(
            is_no_match(&wrapped),
            "is_no_match should find NoMatchError through .context() wrapper"
        );
    }

    #[test]
    fn is_no_match_rejects_wrapped_non_no_match() {
        let err = anyhow::anyhow!("some other error").context("operation 1 failed");
        assert!(
            !is_no_match(&err),
            "is_no_match should return false for non-NoMatchError in chain"
        );
    }

    #[test]
    fn is_ambiguous_finds_wrapped_error() {
        let err: anyhow::Error = AmbiguousError {
            msg: "ambiguous match: pattern \"a\" matches 2 times".to_string(),
        }
        .into();
        let wrapped = err.context("operation 1 (replace) failed");
        assert!(
            is_ambiguous(&wrapped),
            "is_ambiguous should find AmbiguousError through .context() wrapper"
        );
        assert!(!is_no_match(&wrapped));
    }

    #[test]
    fn is_ambiguous_rejects_plain_error() {
        let err = anyhow::anyhow!("some other error").context("operation 1 failed");
        assert!(!is_ambiguous(&err));
    }

    #[test]
    fn is_invalid_input_finds_wrapped_error() {
        let err: anyhow::Error = InvalidInputError {
            msg: "path rejected by workspace guard: escapes".to_string(),
        }
        .into();
        let wrapped = err.context("create failed");
        assert!(is_invalid_input(&wrapped));
        assert!(!is_no_match(&wrapped));
        assert!(!is_ambiguous(&wrapped));
    }

    #[test]
    fn exit_codes_are_unique() {
        let codes = [
            SUCCESS,
            FAILURE,
            CHANGES_DETECTED,
            NO_MATCHES,
            PARSE_ERROR,
            AMBIGUOUS,
            VALIDATION_FAILED,
            ROLLBACK,
            CONFLICTS,
            OPERATION_FAILED,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    #[test]
    fn is_io_not_found_preserves_with_context_chain() {
        use anyhow::Context;
        let err = std::fs::read_to_string("/tmp/patchloom-definitely-missing-xyz-99999")
            .with_context(|| "failed to read path");
        let err = err.unwrap_err();
        assert!(
            is_io_not_found(&err),
            "with_context should keep NotFound in chain: {err:#}"
        );
    }
}
