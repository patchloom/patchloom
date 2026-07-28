//! Library disk apply for Morph-class freeform fragments (#2032).

use std::path::Path;

use crate::containment::PathGuard;
use crate::ops::apply_fragment::{
    FragmentPlacement, build_apply_fragment_spec, desugar_to_replace_fields,
};

use super::{ApplyMode, EditResult, ReplaceOptions, replace::replace_text};

/// Apply a freeform fragment at a required placement anchor on disk (#2032).
///
/// Strips Morph-style lazy marker lines (`// ... existing code ...`), then
/// inserts after/before the unique anchor or replaces `old` via the same
/// path as [`replace_text`]. Fail-closed: missing/ambiguous anchors, empty
/// fragment after strip, PathGuard rejects.
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
    let spec = build_apply_fragment_spec(fragment, None, after, before, old, unique)?;
    let d = desugar_to_replace_fields(&spec);
    let opts = ReplaceOptions {
        insert_after: d.insert_after,
        insert_before: d.insert_before,
        unique: d.unique,
        require_change: true,
        ..ReplaceOptions::default()
    };
    let to = d.new_text.as_deref().unwrap_or("");
    replace_text(path, &d.old, to, &opts, mode, guard)
}
