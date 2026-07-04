//! Mutating `patchloom ast` subcommands (rename, replace).

use super::common::{resolve_lang, setup_multi_file};
use crate::cli::global::GlobalFlags;
use crate::cmd::output::{WritePhase, run_write_op, stage_for_write};
use crate::cmd::write_mode::{RenderPolicy, WriteMessages, finalize_execution_result};
use crate::exit;
use crate::plan::Operation;
use crate::tx::engine::WriteSource;
use anyhow::Context;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// The identifier to rename.
    pub old_name: String,

    /// The new identifier name.
    pub new_name: String,

    /// File or directory to rename in.
    pub path: String,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_rename(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = match setup_multi_file(&args.path, global) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.to_string();
            // fixrealloop: path-first invocation (like many other commands) is a
            // common agent mistake; surface the real order instead of only
            // "path not found: <new_name>".
            if msg.contains("path not found") {
                anyhow::bail!(
                    "{msg}\n\
                     hint: usage is `ast rename <OLD_NAME> <NEW_NAME> <PATH>` \
                     (example: `ast rename alpha gamma mod.rs --apply`). \
                     Path-first order is not accepted."
                );
            }
            return Err(e);
        }
    };
    crate::verbose!(
        "ast rename: '{}' -> '{}' in {}",
        args.old_name,
        args.new_name,
        args.path
    );
    crate::verbose!("ast rename: scanning {} files", paths.len());

    // Pre-filter to files that have matches, then execute as a batch through
    // the tx engine. This gives us backup, rollback, and format lifecycle.
    let mut operations = Vec::new();
    let lang_hint = args.lang.as_deref();
    for path in &paths {
        let lang = resolve_lang(lang_hint, path);
        let source =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

        // Check if this file has any matches (AST or word-boundary fallback).
        let has_match = if lang.has_grammar() {
            crate::ast::rename::rename_in_source(&source, &args.old_name, &args.new_name, lang)
                .is_some_and(|r| r.replacements > 0)
        } else {
            false
        } || {
            // Word-boundary fallback
            crate::ops::replace::compile_replace_regex(&args.old_name, false, false, false, true)
                .ok()
                .flatten()
                .is_some_and(|re| re.is_match(&source))
        };

        if has_match {
            let rel = path
                .strip_prefix(&cwd)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            operations.push(Operation::AstRename {
                path: rel,
                old_name: args.old_name.clone(),
                new_name: args.new_name.clone(),
                lang: args.lang.clone(),
            });
        }
    }

    if operations.is_empty() {
        let msg = format!("symbol '{}' not found in {}", args.old_name, args.path);
        global.emit_error_json(&msg)?;
        return Ok(exit::NO_MATCHES);
    }

    let files_changed = operations.len();
    let (cwd, result) = match stage_for_write(WriteSource::Operations(operations), global) {
        Ok(v) => v,
        Err(e) if exit::is_no_match(&e) => {
            let msg = e.to_string();
            global.emit_error_json(&msg)?;
            return Ok(exit::NO_MATCHES);
        }
        Err(e) => return Err(e),
    };

    #[derive(serde::Serialize)]
    struct RenameOut {
        ok: bool,
        files_changed: usize,
    }

    let check_msg = format!("would rename in {files_changed} file(s)");
    let apply_msg = format!("renamed in {files_changed} file(s)");
    finalize_execution_result(
        global,
        &cwd,
        result,
        |_phase, _diff| RenameOut {
            ok: true,
            files_changed,
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
}

pub(super) fn run_replace(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "ast replace: symbol={}, from={}, regex={}",
        args.symbol,
        args.old,
        args.regex
    );

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
        |phase, diff| AstReplaceOutput {
            ok: true,
            symbol: symbol.clone(),
            replacements: None, // tx engine doesn't expose count
            diff,
            applied: match phase {
                WritePhase::Confirmed(a) => Some(a),
                _ => None,
            },
        },
        &check_msg,
        &apply_msg,
    ) {
        Ok(code) => Ok(code),
        Err(e) => {
            if exit::is_no_match(&e) {
                global.emit_error_json(&e.to_string())?;
                Ok(exit::NO_MATCHES)
            } else {
                Err(e)
            }
        }
    }
}
