//! Mutating `patchloom ast` subcommands (rename, replace).

use super::common::{resolve_lang, setup_multi_file};
use crate::cli::global::GlobalFlags;
use crate::cmd::output::{run_write_op, stage_for_write};
use crate::cmd::write_mode::{RenderPolicy, WriteMessages, finalize_execution_result};
use crate::exit;
use crate::plan::Operation;
use crate::tx::engine::WriteSource;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// File or directory to rename in (path-first, same as `ast replace` / batch / plan).
    pub path: String,

    /// The identifier to rename.
    #[arg(long)]
    pub old: String,

    /// The new identifier name.
    #[arg(long)]
    pub new: String,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_rename(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    crate::verbose!(
        "ast rename: '{}' -> '{}' in {}",
        args.old,
        args.new,
        args.path
    );
    crate::verbose!("ast rename: scanning {} files", paths.len());

    // Sole binary must not look like "symbol not found" (NUL is valid UTF-8).
    if let Err(err) = super::common::reject_sole_explicit_binary(&paths, &args.path) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }

    // Pre-filter to files that have matches (parallel read+probe), then execute
    // as a batch through the tx engine. This gives backup, rollback, and format.
    let lang_hint = args.lang.as_deref();
    let old = args.old.as_str();
    let new = args.new.as_str();
    let lang_cli = args.lang.clone();
    let operations: Vec<Operation> = crate::par_process_files(&paths, None, &[], |path| {
        // SoftSkip walk (#1894): binary / invalid UTF-8 / unreadable → skip.
        let source = crate::files::read_text_file(path)?;
        let lang = resolve_lang(lang_hint, path);

        // Check if this file has any matches (AST or word-boundary fallback).
        let has_match = if lang.has_grammar() {
            crate::ast::rename::rename_in_source(&source, old, new, lang)
                .is_some_and(|r| r.replacements > 0)
        } else {
            false
        } || {
            // Word-boundary fallback
            crate::ops::replace::compile_replace_regex(old, false, false, false, true)
                .ok()
                .flatten()
                .is_some_and(|re| re.is_match(&source))
        };

        if !has_match {
            return None;
        }
        let rel = path
            .strip_prefix(&cwd)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        Some(Operation::AstRename {
            path: rel,
            old: old.to_string(),
            new: new.to_string(),
            lang: lang_cli.clone(),
        })
    });

    if operations.is_empty() {
        let msg = format!("symbol '{}' not found in {}", args.old, args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    let (cwd, result) = match stage_for_write(WriteSource::Operations(operations), global) {
        Ok(v) => v,
        Err(e) if exit::is_no_match(&e) => {
            let msg = e.to_string();
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
        Err(e) => return Err(e),
    };
    // Effective content changes only (identity rename old==new is 0).
    // Do not report operations.len() when commit would write nothing.
    let files_changed = result.build_diffs().len();

    #[derive(serde::Serialize)]
    struct RenameOut {
        ok: bool,
        files_changed: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        applied: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        backup_session: Option<String>,
    }

    let check_msg = format!("would rename in {files_changed} file(s)");
    let apply_msg = format!("renamed in {files_changed} file(s)");
    finalize_execution_result(
        global,
        &cwd,
        result,
        |phase, _diff, backup| RenameOut {
            ok: true,
            // Count of files with real content deltas (not identity renames).
            // Agents pair this with `applied` (#1812).
            files_changed,
            applied: phase.applied_flag(),
            backup_session: backup,
        },
        WriteMessages {
            check: &check_msg,
            apply: &apply_msg,
            post_confirm: None,
        },
        RenderPolicy::default(),
    )
}

#[derive(Debug, Args)]
pub struct ReplaceArgs {
    /// File containing the symbol.
    pub path: String,

    /// Symbol name to scope the replacement to.
    pub symbol: String,

    /// Text or regex pattern to find.
    #[arg(long)]
    pub old: String,

    /// Replacement text.
    #[arg(long = "new")]
    pub new_text: String,

    /// Treat --old as a regex pattern.
    #[arg(long)]
    pub regex: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct AstReplaceOutput {
    ok: bool,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    replacements: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    /// Backup session id after a successful apply (#1802).
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

pub(super) fn run_replace(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "ast replace: symbol={}, from={}, regex={}",
        args.symbol,
        args.old,
        args.regex
    );

    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [&args.path])?;
    let target = cwd.join(&args.path);
    // Strict sole-path preflight (#1894); engine also loads via load_text_strict.
    if let Err(e) = crate::files::load_text_strict(&target, &args.path) {
        if let Some(inv) = e.downcast_ref::<crate::exit::InvalidInputError>() {
            global.emit_error_json_kind(Some("invalid_input"), &inv.msg)?;
            return Ok(exit::FAILURE);
        }
        return Err(e);
    }

    let op = Operation::AstReplace {
        path: args.path.clone(),
        symbol: args.symbol.clone(),
        old: args.old.clone(),
        new_text: args.new_text.clone(),
        regex: args.regex,
        lang: args.lang.clone(),
    };

    let symbol = args.symbol.clone();
    let check_msg = format!("would replace in symbol '{symbol}' in {}", args.path);
    let apply_msg = format!("replaced in symbol '{symbol}' in {}", args.path);

    match run_write_op(
        op,
        global,
        |phase, diff, _backup| AstReplaceOutput {
            ok: true,
            symbol: symbol.clone(),
            replacements: None, // tx engine doesn't expose count
            diff,
            applied: phase.applied_flag(),
            backup_session: _backup,
        },
        &check_msg,
        &apply_msg,
    ) {
        Ok(code) => Ok(code),
        Err(e) => {
            if exit::is_no_match(&e) {
                global.emit_error_json_kind(Some("no_matches"), &e.to_string())?;
                Ok(exit::NO_MATCHES)
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn rename_path_first_with_old_new_flags_applies() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("mod.rs"), "fn alpha() {}\n").unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let code = run_rename(
            RenameArgs {
                path: "mod.rs".into(),
                old: "alpha".into(),
                new: "beta".into(),
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(dir.path().join("mod.rs")).unwrap();
        assert!(
            content.contains("fn beta()"),
            "expected rename, got: {content}"
        );
        assert!(!content.contains("fn alpha()"), "old name should be gone");
    }
}
