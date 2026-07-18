//! Write-policy construction for pending tx files.
use crate::cli::global::GlobalFlags;
use crate::plan::Plan;
use crate::tx::context::EngineContext;
use crate::write::WritePolicy;
use std::path::Path;

// ---------------------------------------------------------------------------
// Write policy
// ---------------------------------------------------------------------------

/// Build the effective write policy for a pending file.
///
/// Start from the CLI-derived per-file defaults, including any EditorConfig
/// values resolved by `policy_from_flags()`, then let plan-level `write_policy`
/// entries override only the keys they set.
pub(crate) fn build_write_policy(
    plan: &Plan,
    ctx: &EngineContext,
    path: &Path,
) -> anyhow::Result<WritePolicy> {
    build_write_policy_with_plan(plan, ctx, path, true)
}

/// Like [`build_write_policy`], but when `apply_plan_fields` is false skip
/// plan `write_policy` field overrides (trim/ensure/eol/collapse).
///
/// Used for paths whose content was already staged by `tidy.fix` with the
/// full defaults→plan→op precedence (#1847). Still resolves EditorConfig via
/// CLI flags (and plan `respect_editorconfig` when set) so CLI
/// `tidy fix --respect-editorconfig` keeps working.
pub(crate) fn build_write_policy_with_plan(
    plan: &Plan,
    ctx: &EngineContext,
    path: &Path,
    apply_plan_fields: bool,
) -> anyhow::Result<WritePolicy> {
    // If the plan explicitly sets respect_editorconfig, build the policy
    // with that flag applied. This must happen before policy_from_flags
    // because EditorConfig properties are resolved during construction,
    // not during apply_override (#1111.3).
    let mut base_flags = ctx.to_global_flags();
    if let Some(ov) = &plan.write_policy
        && let Some(ec) = ov.respect_editorconfig
    {
        base_flags = GlobalFlags::with_editorconfig(&base_flags, ec);
    }
    let mut write_policy = crate::write::policy_from_flags(&base_flags, Some(path));
    if apply_plan_fields && let Some(ov) = &plan.write_policy {
        write_policy.apply_override(ov)?;
    }
    Ok(write_policy)
}
