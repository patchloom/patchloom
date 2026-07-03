//! Library-first execution context (#1375).
//!
//! CLI code still builds this from [`crate::cli::global::GlobalFlags`]. Engine
//! paths prefer [`EngineContext`] for write policy so pure-library builds do
//! not need clap types for that step. Full inversion of every engine field
//! continues in follow-ups under #1375.

use crate::cli::global::GlobalFlags;
use crate::write::{EolMode, WritePolicy, policy_from_flags};
use std::path::{Path, PathBuf};

/// How a write should be applied to disk (mirrors `api::ApplyMode` intent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineApplyMode {
    /// Dry-run / preview (default).
    Preview,
    /// Report whether changes would occur (`--check`).
    Check,
    /// Commit changes (`--apply`).
    Apply,
}

/// Execution context for the transaction engine and staged writes.
///
/// Construct from CLI flags via [`EngineContext::from_global`], or directly
/// from library code without clap.
#[derive(Debug, Clone)]
pub struct EngineContext {
    pub cwd: PathBuf,
    pub mode: EngineApplyMode,
    pub confirm: bool,
    pub json: bool,
    pub jsonl: bool,
    pub quiet: bool,
    pub diff: bool,
    pub ensure_final_newline: bool,
    pub trim_trailing_whitespace: bool,
    pub collapse_blanks: bool,
    pub normalize_eol: Option<EolMode>,
    pub respect_editorconfig: bool,
    pub format: Option<String>,
    pub format_timeout: Option<u64>,
}

impl EngineContext {
    /// Build from CLI global flags (transitional adapter for Phase 3).
    pub fn from_global(global: &GlobalFlags, cwd: PathBuf) -> Self {
        let mode = if global.check {
            EngineApplyMode::Check
        } else if global.apply {
            EngineApplyMode::Apply
        } else {
            EngineApplyMode::Preview
        };
        Self {
            cwd,
            mode,
            confirm: global.confirm,
            json: global.json,
            jsonl: global.jsonl,
            quiet: global.quiet,
            diff: global.diff,
            ensure_final_newline: global.ensure_final_newline,
            trim_trailing_whitespace: global.trim_trailing_whitespace,
            collapse_blanks: global.collapse_blanks,
            normalize_eol: global.normalize_eol,
            respect_editorconfig: global.respect_editorconfig,
            format: global.format.clone(),
            format_timeout: global.format_timeout,
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Bridge back to GlobalFlags for helpers that still require it.
    ///
    /// Transitional only: shrink call sites until they take EngineContext.
    pub fn to_global_flags(&self) -> GlobalFlags {
        GlobalFlags {
            confirm: self.confirm,
            json: self.json,
            jsonl: self.jsonl,
            quiet: self.quiet,
            diff: self.diff,
            ensure_final_newline: self.ensure_final_newline,
            trim_trailing_whitespace: self.trim_trailing_whitespace,
            collapse_blanks: self.collapse_blanks,
            normalize_eol: self.normalize_eol,
            respect_editorconfig: self.respect_editorconfig,
            format: self.format.clone(),
            format_timeout: self.format_timeout,
            apply: matches!(self.mode, EngineApplyMode::Apply),
            check: matches!(self.mode, EngineApplyMode::Check),
            ..GlobalFlags::default()
        }
    }

    /// Write policy for a path under this context.
    pub fn write_policy(&self, path: Option<&Path>) -> WritePolicy {
        let g = self.to_global_flags();
        policy_from_flags(&g, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_global_maps_check_mode() {
        let g = GlobalFlags {
            check: true,
            ..GlobalFlags::default()
        };
        let ctx = EngineContext::from_global(&g, PathBuf::from("/tmp"));
        assert_eq!(ctx.mode, EngineApplyMode::Check);
        assert!(ctx.to_global_flags().check);
        assert_eq!(ctx.cwd(), Path::new("/tmp"));
    }

    #[test]
    fn from_global_maps_apply_mode() {
        let g = GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        };
        let ctx = EngineContext::from_global(&g, PathBuf::from("/tmp"));
        assert_eq!(ctx.mode, EngineApplyMode::Apply);
        assert!(ctx.to_global_flags().apply);
    }

    #[test]
    fn default_preview_mode() {
        let g = GlobalFlags::default();
        let ctx = EngineContext::from_global(&g, PathBuf::from("/tmp"));
        assert_eq!(ctx.mode, EngineApplyMode::Preview);
        let _ = ctx.write_policy(None);
    }
}
