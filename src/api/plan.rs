//! Plan parsing and execution (transaction) for the library API.

#[cfg(any(feature = "cli", feature = "files"))]
use std::path::Path;

#[cfg(any(feature = "cli", feature = "files"))]
use crate::containment::PathGuard;

#[cfg(any(feature = "cli", feature = "files"))]
use super::PlanReport; // reexport alias from tx

/// Expand `plan.for_each` globs (requires the `files` feature; #2169).
#[cfg(feature = "files")]
pub use crate::plan::expand_for_each;
/// Iterate plan `format` then `validate` command strings (#2168).
pub use crate::plan::lifecycle_cmds;
/// Refuse format/validate cmds that need a shell for redirects or pipes (#2168).
pub use crate::plan::refuse_lifecycle_shell_metas;

/// Parse a transaction plan from a JSON string.
pub fn parse_plan(input: &str) -> anyhow::Result<crate::plan::Plan> {
    crate::plan::parse_plan_auto(input, None, None)
}

/// Execute a transaction plan atomically.
///
/// All operations succeed or all are rolled back. Returns the exit code
/// (For library users this returns `PlanReport` directly; CLI/MCP retain
/// the (code, JSON) form for compatibility.)
///
/// See `PlanReport` fields and embedding docs for typed usage:
///
/// ```ignore
/// let report: PlanReport = execute_plan(plan, cwd, guard)?;
/// assert!(report.ok);
/// // report.changes, report.searches etc are typed
/// ```
///
/// The optional `guard` is threaded through to all operations for
/// PathGuard enforcement (see module docs for PathGuard usage). Pass
/// `None` for no additional containment checks (current default behavior
/// for most callers).
///
/// Available with the `files` feature (for pure library use without the
/// CLI) or the `cli` feature.
///
/// `for_each` expands under `files` before PathGuard (#2169). When `guard` is
/// `Some`, format/validate commands that contain shell metas are refused as
/// `GuardRejected` before commit (#2168). MCP still strips those steps.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn execute_plan(
    plan: crate::plan::Plan,
    cwd: &Path,
    guard: Option<&PathGuard>,
) -> anyhow::Result<PlanReport> {
    crate::tx::execute_plan_direct(plan, cwd, guard)
}
