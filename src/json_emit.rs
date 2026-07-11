//! Fail-closed serialization for agent-facing structured stdout (`--json` / `--jsonl`).
//!
//! Primary serialization should succeed for normal report types. When it does
//! not, callers still print a minimal `ok: false` envelope and treat the emit
//! as a hard failure (exit 1) so agents never see empty stdout with a soft
//! plan exit code. See issue #1651.
//!
//! Always compiled (library + CLI). Print/exit helpers are mainly used from
//! the CLI entry path; without `cli`, only [`fallback_envelope`] is typically
//! needed (e.g. `api::format_search_results`).

#![cfg_attr(not(feature = "cli"), allow(dead_code))]

use serde::Serialize;
use std::cell::Cell;

/// True when the primary value serialized; false when the printed body is the
/// fallback envelope (callers should map that to [`crate::exit::FAILURE`]).
pub(crate) struct StructuredEmit {
    pub json: String,
    pub primary_ok: bool,
}

thread_local! {
    /// Test-only: force the next [`serialize_structured`] call to take the
    /// fallback path without needing a non-serializable type.
    static FORCE_SERIALIZE_FAIL: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that forces structured serialize to fail while held (`cfg(test)`).
#[cfg(test)]
pub(crate) struct ForceSerializeFailGuard {
    prev: bool,
}

#[cfg(test)]
impl ForceSerializeFailGuard {
    pub(crate) fn engage() -> Self {
        let prev = FORCE_SERIALIZE_FAIL.with(|c| c.replace(true));
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for ForceSerializeFailGuard {
    fn drop(&mut self) {
        FORCE_SERIALIZE_FAIL.with(|c| c.set(self.prev));
    }
}

/// Serialize `value` for structured agent stdout.
///
/// On primary failure (or test inject), returns a minimal fallback JSON body
/// with `primary_ok: false`. The fallback itself uses a hard-coded string if
/// even `serde_json::Value` serialization fails (should not happen).
pub(crate) fn serialize_structured<T: Serialize>(value: &T, compact: bool) -> StructuredEmit {
    #[cfg(test)]
    if FORCE_SERIALIZE_FAIL.with(|c| c.get()) {
        return StructuredEmit {
            json: fallback_envelope("forced serialize failure for tests", compact),
            primary_ok: false,
        };
    }

    let result = if compact {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    };
    match result {
        Ok(json) => StructuredEmit {
            json,
            primary_ok: true,
        },
        Err(err) => StructuredEmit {
            json: fallback_envelope(&err.to_string(), compact),
            primary_ok: false,
        },
    }
}

/// Print structured JSON to stdout. Returns whether primary serialization
/// succeeded. On failure, still prints a non-empty fallback envelope and
/// logs a one-line diagnostic to stderr.
pub(crate) fn print_structured<T: Serialize>(value: &T, compact: bool) -> bool {
    let emit = serialize_structured(value, compact);
    if !emit.primary_ok {
        eprintln!("failed to serialize structured output (printed fallback envelope)");
    }
    println!("{}", emit.json);
    emit.primary_ok
}

/// Minimal agent envelope when primary serialization fails.
pub(crate) fn fallback_envelope(detail: &str, compact: bool) -> String {
    let payload = serde_json::json!({
        "ok": false,
        "error": format!("failed to serialize structured output: {detail}"),
        "error_kind": "operation_failed",
    });
    let rendered = if compact {
        serde_json::to_string(&payload)
    } else {
        serde_json::to_string_pretty(&payload)
    };
    rendered.unwrap_or_else(|_| {
        // Static ASCII only; never empty.
        r#"{"ok":false,"error":"failed to serialize structured output","error_kind":"operation_failed"}"#
            .to_string()
    })
}

/// Map planned exit code to hard failure when structured emit used the fallback.
#[inline]
pub(crate) fn exit_after_emit(primary_ok: bool, planned: u8) -> u8 {
    if primary_ok {
        planned
    } else {
        crate::exit::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Simple {
        ok: bool,
        status: &'static str,
    }

    #[test]
    fn serialize_structured_success_pretty() {
        let v = Simple {
            ok: true,
            status: "success",
        };
        let emit = serialize_structured(&v, false);
        assert!(emit.primary_ok);
        let parsed: serde_json::Value = serde_json::from_str(&emit.json).unwrap();
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["status"], "success");
        assert!(emit.json.contains('\n'), "pretty should be multi-line");
    }

    #[test]
    fn serialize_structured_success_compact() {
        let v = Simple {
            ok: true,
            status: "success",
        };
        let emit = serialize_structured(&v, true);
        assert!(emit.primary_ok);
        assert!(!emit.json.contains('\n'));
        let parsed: serde_json::Value = serde_json::from_str(&emit.json).unwrap();
        assert_eq!(parsed["status"], "success");
    }

    #[test]
    fn force_fail_returns_fallback_envelope() {
        let _g = ForceSerializeFailGuard::engage();
        let v = Simple {
            ok: true,
            status: "success",
        };
        let emit = serialize_structured(&v, true);
        assert!(!emit.primary_ok);
        assert!(!emit.json.is_empty());
        let parsed: serde_json::Value = serde_json::from_str(&emit.json).unwrap();
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error_kind"], "operation_failed");
        let err = parsed["error"].as_str().unwrap_or("");
        assert!(
            err.contains("failed to serialize structured output"),
            "error={err}"
        );
        assert!(err.contains("forced serialize failure"), "error={err}");
    }

    #[test]
    fn force_fail_pretty_is_valid_json() {
        let _g = ForceSerializeFailGuard::engage();
        let emit = serialize_structured(
            &Simple {
                ok: true,
                status: "x",
            },
            false,
        );
        assert!(!emit.primary_ok);
        serde_json::from_str::<serde_json::Value>(&emit.json).expect("valid json");
    }

    #[test]
    fn exit_after_emit_overrides_planned_code() {
        assert_eq!(exit_after_emit(true, 0), 0);
        assert_eq!(exit_after_emit(true, 2), 2);
        assert_eq!(exit_after_emit(true, 3), 3);
        assert_eq!(exit_after_emit(false, 0), crate::exit::FAILURE);
        assert_eq!(exit_after_emit(false, 2), crate::exit::FAILURE);
        assert_eq!(exit_after_emit(false, 3), crate::exit::FAILURE);
    }

    #[test]
    fn fallback_envelope_hardcoded_shape() {
        let s = fallback_envelope("boom", true);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error_kind"], "operation_failed");
        assert!(v["error"].as_str().unwrap().contains("boom"));
    }

    // TxOutput is always available via the internal `tx` module.
    #[test]
    fn force_fail_on_tx_output_still_produces_agent_envelope() {
        use crate::tx::TxOutput;
        let _g = ForceSerializeFailGuard::engage();
        let report = TxOutput {
            ok: true,
            status: "success".into(),
            files_changed: 1,
            files_created: 0,
            files_deleted: 0,
            changes: vec![],
            reads: vec![],
            searches: vec![],
            lints: vec![],
            mutations: vec![],
            changed: None,
            removed: None,
            error_kind: None,
            error: None,
            backup_session: None,
        };
        let emit = serialize_structured(&report, true);
        assert!(!emit.primary_ok);
        let v: serde_json::Value = serde_json::from_str(&emit.json).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error_kind"], "operation_failed");
        // Never the original success status as the only signal.
        assert_ne!(v["status"], "success");
        assert_eq!(
            exit_after_emit(emit.primary_ok, crate::exit::SUCCESS),
            crate::exit::FAILURE
        );
    }
}
