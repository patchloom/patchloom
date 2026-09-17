//! Additional mutating `patchloom ast` subcommands (insert, wrap, imports,
//! reorder, group, move, extract-to-file, split, rewrite-signature).

use crate::ast::parse_lang_hint;
use crate::cli::global::GlobalFlags;
use crate::cmd::output::{WritePhase, run_write_op};
use crate::exit;
use crate::plan::{Operation, SplitTargetSpec};
use clap::Args;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct AstMutateOutput {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

fn ast_mutate_output(
    phase: WritePhase,
    diff: Option<String>,
    backup: Option<String>,
) -> AstMutateOutput {
    AstMutateOutput {
        ok: true,
        diff,
        applied: phase.applied_flag(),
        backup_session: backup,
    }
}

fn run_single_file_ast_op(
    global: &GlobalFlags,
    contain: &[String],
    load_path: &str,
    lang: Option<&str>,
    op: Operation,
    check_msg: &str,
    apply_msg: &str,
) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, contain.iter().map(String::as_str))?;
    let target = cwd.join(load_path);
    if let Err(e) = crate::files::load_text_strict(&target, load_path) {
        if crate::exit::is_load_text_strict_fail(&e) {
            let kind = crate::fallback::error_kind_str(&e).unwrap_or("invalid_input");
            let msg = crate::exit::agent_error_message(&e);
            global.emit_error_json_kind(Some(kind), &msg)?;
            return Ok(exit::FAILURE);
        }
        return Err(e);
    }
    if let Some(s) = lang {
        let _ = parse_lang_hint(s)?;
    }
    match run_write_op(op, global, ast_mutate_output, check_msg, apply_msg) {
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

fn nonempty_symbols(symbols: Vec<String>) -> Option<Vec<String>> {
    if symbols.is_empty() {
        None
    } else {
        Some(symbols)
    }
}

pub(crate) fn parse_reorder_order(raw: &str) -> anyhow::Result<serde_json::Value> {
    let trimmed = raw.trim();
    match trimmed {
        "alphabetical" | "reverse" | "kind-first" | "kind_first" => {
            Ok(serde_json::Value::String(trimmed.to_string()))
        }
        _ => match serde_json::from_str(trimmed) {
            Ok(v) => Ok(v),
            Err(_) => Ok(serde_json::Value::String(trimmed.to_string())),
        },
    }
}

pub(crate) fn parse_split_targets(raw: &str) -> anyhow::Result<Vec<SplitTargetSpec>> {
    serde_json::from_str(raw).map_err(|e| {
        anyhow::Error::new(exit::InvalidInputError {
            msg: format!("ast split --targets must be a JSON array of SplitTargetSpec: {e}"),
        })
    })
}

#[derive(Debug, Args)]
pub struct InsertArgs {
    /// File to insert code into.
    pub path: String,

    /// Code to insert.
    #[arg(long)]
    pub content: Option<String>,

    /// Module/impl/struct to insert into.
    #[arg(long)]
    pub inside: Option<String>,

    /// Insert after this symbol.
    #[arg(long)]
    pub after: Option<String>,

    /// Insert before this symbol.
    #[arg(long)]
    pub before: Option<String>,

    /// Position within `--inside`: start or end.
    #[arg(long)]
    pub position: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_insert(args: InsertArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let content =
        args.content
            .filter(|s| !s.is_empty())
            .ok_or_else(|| exit::InvalidInputError {
                msg: "ast insert requires --content".into(),
            })?;
    crate::verbose!("ast insert: path={}", args.path);
    let check_msg = format!("would insert into {}", args.path);
    let apply_msg = format!("inserted into {}", args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstInsert {
            path: path.clone(),
            content,
            inside: args.inside,
            after: args.after,
            before: args.before,
            position: args.position,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct WrapArgs {
    /// File containing the symbols or line range to wrap.
    pub path: String,

    /// Wrapping construct (e.g. "mod foo", "impl Bar").
    #[arg(long)]
    pub wrapper: String,

    /// Symbols to wrap (comma-separated or repeatable). Mutually exclusive with `--lines`.
    #[arg(long, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Line range to wrap (e.g. "10-20"). Mutually exclusive with `--symbols`.
    #[arg(long)]
    pub lines: Option<String>,

    /// Content to insert at the top of the wrapped block.
    #[arg(long)]
    pub preamble: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_wrap(args: WrapArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast wrap: path={} wrapper={}", args.path, args.wrapper);
    let check_msg = format!("would wrap in {}", args.path);
    let apply_msg = format!("wrapped in {}", args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstWrap {
            path: path.clone(),
            symbols: nonempty_symbols(args.symbols),
            lines: args.lines,
            wrapper: args.wrapper,
            preamble: args.preamble,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct ImportsArgs {
    /// File whose imports should be updated.
    pub path: String,

    /// Import statements to add (repeatable).
    #[arg(long)]
    pub add: Vec<String>,

    /// Import statements to remove (repeatable).
    #[arg(long)]
    pub remove: Vec<String>,

    /// Deduplicate imports after add/remove.
    #[arg(long)]
    pub dedupe: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_imports(args: ImportsArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast imports: path={}", args.path);
    let check_msg = format!("would update imports in {}", args.path);
    let apply_msg = format!("updated imports in {}", args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstImports {
            path: path.clone(),
            add: nonempty_symbols(args.add),
            remove: nonempty_symbols(args.remove),
            dedupe: args.dedupe,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct ReorderArgs {
    /// File whose symbols should be reordered.
    pub path: String,

    /// Ordering: JSON array of names, or alphabetical, reverse, or kind-first.
    #[arg(long)]
    pub order: String,

    /// Scope to reorder within (module/impl). Default: top-level.
    #[arg(long)]
    pub inside: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_reorder(args: ReorderArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast reorder: path={}", args.path);
    let order = parse_reorder_order(&args.order)?;
    let check_msg = format!("would reorder symbols in {}", args.path);
    let apply_msg = format!("reordered symbols in {}", args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstReorder {
            path: path.clone(),
            inside: args.inside,
            order,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct GroupArgs {
    /// File containing the symbols to group.
    pub path: String,

    /// Module name to create or append to.
    #[arg(long)]
    pub module: String,

    /// Symbols to move into the module (comma-separated or repeatable).
    #[arg(long, value_delimiter = ',', required = true)]
    pub symbols: Vec<String>,

    /// Code to insert at the top of the module.
    #[arg(long)]
    pub preamble: Option<String>,

    /// Where to place a new module: first-symbol, end, or after:<symbol>.
    #[arg(long)]
    pub position: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_group(args: GroupArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast group: path={} module={}", args.path, args.module);
    let check_msg = format!("would group symbols in {}", args.path);
    let apply_msg = format!("grouped symbols in {}", args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstGroup {
            path: path.clone(),
            module: args.module,
            symbols: args.symbols,
            preamble: args.preamble,
            position: args.position,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct MoveArgs {
    /// Source file.
    pub path: String,

    /// Target file.
    #[arg(long)]
    pub target: String,

    /// Symbols to move (comma-separated or repeatable).
    #[arg(long, value_delimiter = ',', required = true)]
    pub symbols: Vec<String>,

    /// Position in target: end, start, after:<symbol>, before:<symbol>.
    #[arg(long)]
    pub position: Option<String>,

    /// Content to prepend to the target file if it is being created.
    #[arg(long)]
    pub target_prepend: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_move(args: MoveArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast move: {} -> {}", args.path, args.target);
    let check_msg = format!("would move symbols from {} to {}", args.path, args.target);
    let apply_msg = format!("moved symbols from {} to {}", args.path, args.target);
    let path = args.path.clone();
    let target = args.target.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        &[path.clone(), target.clone()],
        &path,
        lang.as_deref(),
        Operation::AstMove {
            path: path.clone(),
            target: target.clone(),
            symbols: args.symbols,
            position: args.position,
            target_prepend: args.target_prepend,
            lang: lang.clone(),
            update_imports: false,
            old_module_path: None,
            new_module_path: None,
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct ExtractToFileArgs {
    /// Source file containing the symbol.
    #[arg(long)]
    pub source: String,

    /// Name of the symbol to extract.
    #[arg(long)]
    pub symbol: String,

    /// Destination file path.
    #[arg(long)]
    pub target: String,

    /// Text to leave in place of the extracted block.
    #[arg(long)]
    pub replacement: Option<String>,

    /// Remove the wrapper and un-indent (typical for modules).
    #[arg(long)]
    pub unwrap: bool,

    /// Content to prepend to the target file.
    #[arg(long)]
    pub prepend: Option<String>,

    /// Overwrite the target if it exists.
    #[arg(long)]
    pub force: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_extract_to_file(
    args: ExtractToFileArgs,
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    crate::verbose!(
        "ast extract-to-file: {} {} -> {}",
        args.source,
        args.symbol,
        args.target
    );
    let check_msg = format!(
        "would extract '{}' from {} to {}",
        args.symbol, args.source, args.target
    );
    let apply_msg = format!(
        "extracted '{}' from {} to {}",
        args.symbol, args.source, args.target
    );
    let source = args.source.clone();
    let target = args.target.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        &[source.clone(), target.clone()],
        &source,
        lang.as_deref(),
        Operation::AstExtractToFile {
            source: source.clone(),
            symbol: args.symbol,
            target: target.clone(),
            replacement: args.replacement,
            unwrap: if args.unwrap { Some(true) } else { None },
            prepend: args.prepend,
            force: args.force,
            lang: lang.clone(),
            update_imports: false,
            old_module_path: None,
            new_module_path: None,
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct SplitArgs {
    /// File to split.
    #[arg(long)]
    pub source: String,

    /// JSON array of SplitTargetSpec objects.
    #[arg(long)]
    pub targets: String,

    /// Symbols to keep in the source file (repeatable).
    #[arg(long)]
    pub keep_in_source: Vec<String>,

    /// Text to append to source after split.
    #[arg(long)]
    pub source_suffix: Option<String>,

    /// Text to prepend to source after split.
    #[arg(long)]
    pub source_prefix: Option<String>,

    /// Error if any symbol is unaccounted for.
    #[arg(long)]
    pub require_exhaustive: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_split(args: SplitArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("ast split: source={}", args.source);
    let targets = parse_split_targets(&args.targets)?;
    let source = args.source.clone();
    let mut contain = vec![source.clone()];
    contain.extend(targets.iter().map(|t| t.path.clone()));
    let check_msg = format!("would split {}", args.source);
    let apply_msg = format!("split {}", args.source);
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        &contain,
        &source,
        lang.as_deref(),
        Operation::AstSplit {
            source: source.clone(),
            targets,
            keep_in_source: args.keep_in_source,
            source_suffix: args.source_suffix,
            source_prefix: args.source_prefix,
            require_exhaustive: if args.require_exhaustive {
                Some(true)
            } else {
                None
            },
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[derive(Debug, Args)]
pub struct RewriteSignatureArgs {
    /// File containing the function.
    pub path: String,

    /// Function name to rewrite.
    #[arg(long)]
    pub old: String,

    /// New parameter list including parens (e.g. "(x: i32)").
    #[arg(long)]
    pub parameters: Option<String>,

    /// New return type using language-native syntax (e.g. "-> String").
    #[arg(long, allow_hyphen_values = true)]
    pub return_type: Option<String>,

    /// New visibility (e.g. "pub", "pub(crate)", or "" for private).
    #[arg(long)]
    pub visibility: Option<String>,

    /// Full replacement signature text.
    #[arg(long)]
    pub new_signature: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub(super) fn run_rewrite_signature(
    args: RewriteSignatureArgs,
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    crate::verbose!("ast rewrite-signature: path={} old={}", args.path, args.old);
    let check_msg = format!("would rewrite signature of '{}' in {}", args.old, args.path);
    let apply_msg = format!("rewrote signature of '{}' in {}", args.old, args.path);
    let path = args.path.clone();
    let lang = args.lang.clone();
    run_single_file_ast_op(
        global,
        std::slice::from_ref(&path),
        &path,
        lang.as_deref(),
        Operation::AstRewriteSignature {
            path: path.clone(),
            old: args.old,
            new_signature: args.new_signature,
            visibility: args.visibility,
            parameters: args.parameters,
            return_type: args.return_type,
            lang: lang.clone(),
        },
        &check_msg,
        &apply_msg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use clap::Parser;
    use std::fs;
    use tempfile::TempDir;

    fn apply_global(dir: &std::path::Path) -> GlobalFlags {
        let mut global = GlobalFlags::test_with_cwd(dir);
        global.apply = true;
        global
    }

    #[test]
    fn clap_parses_insert_and_rewrite_signature() {
        let cli = crate::cli::Cli::try_parse_from([
            "patchloom",
            "ast",
            "insert",
            "lib.rs",
            "--content",
            "fn added() {}",
            "--after",
            "existing",
        ])
        .expect("insert clap");
        match cli.command {
            crate::cmd::Command::Ast(args) => match args.command {
                crate::cmd::ast::AstCommand::Insert(a) => {
                    assert_eq!(a.path, "lib.rs");
                    assert_eq!(a.content.as_deref(), Some("fn added() {}"));
                    assert_eq!(a.after.as_deref(), Some("existing"));
                }
                other => panic!("expected Insert, got {other:?}"),
            },
            other => panic!("expected Ast, got {other:?}"),
        }

        let cli = crate::cli::Cli::try_parse_from([
            "patchloom",
            "ast",
            "rewrite-signature",
            "lib.rs",
            "--old",
            "process",
            "--parameters",
            "(x: u64)",
            "--return-type",
            "-> u64",
        ])
        .expect("rewrite-signature clap");
        match cli.command {
            crate::cmd::Command::Ast(args) => match args.command {
                crate::cmd::ast::AstCommand::RewriteSignature(a) => {
                    assert_eq!(a.path, "lib.rs");
                    assert_eq!(a.old, "process");
                    assert_eq!(a.parameters.as_deref(), Some("(x: u64)"));
                    assert_eq!(a.return_type.as_deref(), Some("-> u64"));
                }
                other => panic!("expected RewriteSignature, got {other:?}"),
            },
            other => panic!("expected Ast, got {other:?}"),
        }
    }

    #[test]
    fn insert_preview_exits_changes_detected() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn existing() {}\n").unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_insert(
            InsertArgs {
                path: "lib.rs".into(),
                content: Some("fn added() { 1 }".into()),
                inside: None,
                after: Some("existing".into()),
                before: None,
                position: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert_eq!(content, "fn existing() {}\n");
    }

    #[test]
    fn insert_apply_mutates() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn existing() {}\n").unwrap();
        let global = apply_global(dir.path());
        let code = run_insert(
            InsertArgs {
                path: "lib.rs".into(),
                content: Some("fn added() { 1 }".into()),
                inside: None,
                after: Some("existing".into()),
                before: None,
                position: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert!(content.contains("fn added()"), "got: {content}");
    }

    #[test]
    fn insert_missing_symbol_is_no_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn existing() {}\n").unwrap();
        let global = apply_global(dir.path());
        let code = run_insert(
            InsertArgs {
                path: "lib.rs".into(),
                content: Some("fn added() {}".into()),
                inside: None,
                after: Some("missing".into()),
                before: None,
                position: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert_eq!(content, "fn existing() {}\n");
    }

    #[test]
    fn insert_missing_content_is_invalid_input() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn existing() {}\n").unwrap();
        let global = apply_global(dir.path());
        let err = run_insert(
            InsertArgs {
                path: "lib.rs".into(),
                content: None,
                inside: None,
                after: Some("existing".into()),
                before: None,
                position: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .expect_err("missing --content");
        assert!(
            crate::exit::is_invalid_input(&err),
            "expected invalid_input, got {err}"
        );
    }

    #[test]
    fn rewrite_signature_preview_exits_changes_detected() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn process(x: i32) {}\n").unwrap();
        let global = GlobalFlags::test_with_cwd(dir.path());
        let code = run_rewrite_signature(
            RewriteSignatureArgs {
                path: "lib.rs".into(),
                old: "process".into(),
                parameters: Some("(x: u64)".into()),
                return_type: Some("-> u64".into()),
                visibility: None,
                new_signature: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert_eq!(content, "fn process(x: i32) {}\n");
    }

    #[test]
    fn rewrite_signature_apply_mutates() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn process(x: i32) {}\n").unwrap();
        let global = apply_global(dir.path());
        let code = run_rewrite_signature(
            RewriteSignatureArgs {
                path: "lib.rs".into(),
                old: "process".into(),
                parameters: Some("(x: u64)".into()),
                return_type: Some("-> u64".into()),
                visibility: None,
                new_signature: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert!(content.contains("u64"), "got: {content}");
        assert!(content.contains("-> u64 {"), "got: {content}");
    }

    #[test]
    fn rewrite_signature_missing_is_no_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn keep() {}\n").unwrap();
        let global = apply_global(dir.path());
        let code = run_rewrite_signature(
            RewriteSignatureArgs {
                path: "lib.rs".into(),
                old: "missing_fn".into(),
                parameters: Some("(x: i32)".into()),
                return_type: None,
                visibility: None,
                new_signature: None,
                lang: None,
                write: Default::default(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        let content = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
        assert_eq!(content, "fn keep() {}\n");
    }
}
