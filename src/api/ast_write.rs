//! AST write helpers for the public library API (feature `ast` + `files`/`cli`).

use std::path::Path;

use crate::ast::rewrite::FunctionSigEdit;
use crate::containment::PathGuard;
use crate::plan::Operation;

use super::{ApplyMode, EditResult};

/// Rewrite a function signature on disk (PathGuard + ApplyMode like other writers).
///
/// Prefer structured fields on [`FunctionSigEdit`]. Optionally pass `new_signature`
/// to replace the entire signature span (any language with tree-sitter support).
/// At least one of the structured fields or `new_signature` must be set.
///
/// Available with `features = ["ast"]` and `files` or `cli` for the write path.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
pub fn ast_rewrite_signature(
    path: &Path,
    name: &str,
    edit: &FunctionSigEdit,
    new_signature: Option<&str>,
    mode: ApplyMode,
    guard: Option<&PathGuard>,
) -> anyhow::Result<EditResult> {
    if new_signature.is_none()
        && edit.visibility.is_none()
        && edit.parameters.is_none()
        && edit.return_type.is_none()
    {
        anyhow::bail!(
            "ast_rewrite_signature requires new_signature and/or visibility/parameters/return_type"
        );
    }
    let op = Operation::AstRewriteSignature {
        path: path.to_string_lossy().into(),
        old: name.into(),
        new_signature: new_signature.map(str::to_string),
        visibility: edit.visibility.clone(),
        parameters: edit.parameters.clone(),
        return_type: edit.return_type.clone(),
        lang: None,
    };
    let cwd = path.parent().unwrap_or_else(|| Path::new("."));
    super::execute_as_edit_result(op, mode, cwd, guard, "ast.rewrite_signature", None)
}
