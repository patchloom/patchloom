//! Library disk apply for Morph-class freeform fragments (#2032).

use std::path::Path;

use crate::containment::PathGuard;
use crate::ops::apply_fragment::{FragmentPlacement, plan_apply_fragment_to_replace};

use super::{ApplyMode, EditResult};

/// Apply a freeform fragment at a required placement anchor on disk (#2032).
///
/// Strips Morph-style lazy marker lines (`// ... existing code ...`), then
/// inserts after/before the unique anchor or replaces `old` via the same
/// plan desugar path as CLI/MCP (`plan_apply_fragment_to_replace` → tx).
/// Fail-closed: missing/ambiguous anchors, empty fragment after strip,
/// PathGuard rejects.
///
/// # Example
///
/// ```rust,no_run
/// use patchloom::api::{
///     ApplyMode, FragmentPlacement, apply_fragment_to_file,
/// };
/// use std::path::Path;
///
/// let _ = apply_fragment_to_file(
///     Path::new("src/lib.rs"),
///     "// ... existing code ...\nfn new() {}\n// ... existing code ...\n",
///     FragmentPlacement::After("fn foo() {".into()),
///     true,
///     ApplyMode::Apply,
///     None,
/// )?;
/// # Ok::<(), anyhow::Error>(())
/// ```
#[cfg(any(feature = "cli", feature = "files"))]
pub fn apply_fragment_to_file(
    path: &Path,
    fragment: &str,
    placement: FragmentPlacement,
    unique: bool,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    let (after, before, old) = match &placement {
        FragmentPlacement::After(a) => (Some(a.as_str()), None, None),
        FragmentPlacement::Before(b) => (None, Some(b.as_str()), None),
        FragmentPlacement::Replace(o) => (None, None, Some(o.as_str())),
    };
    let abs = super::library_abs_path(path, guard)?;
    let path_str = super::library_op_path(path, &abs, guard);
    let op = plan_apply_fragment_to_replace(&path_str, fragment, None, after, before, old, unique)?;
    let cwd = super::library_project_root(&abs, guard);
    let display = path.to_string_lossy();
    super::execute_as_edit_result_with_path(
        op,
        mode,
        cwd,
        guard,
        "apply.fragment",
        None,
        Some(display.as_ref()),
    )
}
