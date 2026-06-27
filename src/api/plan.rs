//! Plan parsing and execution (transaction) for the library API.

#[cfg(any(feature = "cli", feature = "files"))]
use std::path::Path;

#[cfg(any(feature = "cli", feature = "files"))]
use crate::containment::PathGuard;

#[cfg(any(feature = "cli", feature = "files"))]
use super::PlanReport; // reexport alias from tx

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
#[cfg(any(feature = "cli", feature = "files"))]
#[allow(clippy::result_large_err)]
pub fn execute_plan(
    plan: crate::plan::Plan,
    cwd: &Path,
    guard: Option<&PathGuard>,
) -> anyhow::Result<PlanReport> {
    crate::tx::execute_plan_direct(plan, cwd, guard)
}
