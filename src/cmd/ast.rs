//! AST-aware subcommands: `patchloom ast list|read|rename|validate|search|refs|deps|map|replace|impact|diff`.

use crate::ast::Language;
use crate::ast::symbols::{self, SymbolDef};
use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::cmd::output::execute_via_engine;
use crate::exit;
use crate::plan::Operation;
use crate::tx::engine::ExecuteOptions;
use anyhow::Context;
use clap::{Args, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Shared helpers for AST subcommands
// ---------------------------------------------------------------------------

/// Resolve `--lang` hint to a `Language`, falling back to extension detection.
fn resolve_lang(lang_arg: Option<&str>, path: &Path) -> Language {
    lang_arg
        .map(lang_from_str)
        .unwrap_or_else(|| Language::from_path(path))
}

/// Common preamble for single-file AST commands: resolve cwd, join path, detect
/// language, read source. Returns `(cwd, target, lang, source)`.
fn setup_single_file(
    path_arg: &str,
    lang_arg: Option<&str>,
    global: &GlobalFlags,
) -> anyhow::Result<(PathBuf, PathBuf, Language, String)> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(path_arg);
    let lang = resolve_lang(lang_arg, &target);
    let source = std::fs::read_to_string(&target).with_context(|| format!("reading {path_arg}"))?;
    Ok((cwd, target, lang, source))
}

/// Common preamble for multi-file AST commands: resolve cwd, join path, resolve
/// target paths. Returns `(cwd, paths)`.
fn setup_multi_file(
    path_arg: &str,
    global: &GlobalFlags,
) -> anyhow::Result<(PathBuf, Vec<PathBuf>)> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(path_arg);
    let paths = resolve_target_paths(&target, path_arg, global)?;
    Ok((cwd, paths))
}

#[derive(Debug, Subcommand)]
pub enum AstCommand {
    /// List symbol definitions in a file or directory.
    List(ListArgs),
    /// Read a specific symbol by name.
    Read(ReadArgs),
    /// Rename identifiers in source code (AST-aware, skips strings/comments).
    Rename(RenameArgs),
    /// Validate syntax of source files.
    Validate(ValidateArgs),
    /// Structural search using AST queries.
    Search(SearchArgs),
    /// Find all references to a symbol across files.
    Refs(RefsArgs),
    /// Extract import/dependency statements from files.
    Deps(DepsArgs),
    /// Generate a ranked repository map (PageRank).
    Map(MapArgs),
    /// Replace text only within a specific symbol's body.
    Replace(ReplaceArgs),
    /// Transitive impact analysis of changing a symbol.
    Impact(ImpactArgs),
    /// Structural diff between two versions of a file.
    Diff(DiffArgs),
}

#[derive(Debug, Args)]
pub struct AstArgs {
    #[command(subcommand)]
    pub command: AstCommand,
}

pub fn display_path(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

pub fn run(args: AstArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    match args.command {
        AstCommand::List(a) => run_list(a, global),
        AstCommand::Read(a) => run_read(a, global),
        AstCommand::Rename(a) => run_rename(a, global),
        AstCommand::Validate(a) => run_validate(a, global),
        AstCommand::Search(a) => run_search(a, global),
        AstCommand::Refs(a) => run_refs(a, global),
        AstCommand::Deps(a) => run_deps(a, global),
        AstCommand::Map(a) => run_map(a, global),
        AstCommand::Replace(a) => run_replace(a, global),
        AstCommand::Impact(a) => run_impact(a, global),
        AstCommand::Diff(a) => run_diff(a, global),
    }
}

// ---------------------------------------------------------------------------
// ast list
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ListArgs {
    /// File or directory to list symbols from.
    pub path: String,

    /// Filter by symbol kind (comma-separated: function,struct,enum,...).
    #[arg(long)]
    pub kind: Option<String>,

    /// Compact mode: definition names only (Cline-style, maximum token efficiency).
    #[arg(long)]
    pub compact: bool,

    /// Language hint (overrides extension detection).
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_list(args: ListArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);

    let kind_filter = parse_kind_filter(&args.kind);
    let lang_hint = args.lang.as_deref();
    crate::verbose!(
        "ast list: target={}, kind_filter={:?}",
        args.path,
        kind_filter
    );

    let mut any_output = false;

    if target.is_file() {
        let lang = resolve_lang(lang_hint, &target);
        crate::verbose!("ast list: detected language={lang} for {}", args.path);
        if !lang.has_grammar() {
            eprintln!(
                "Unsupported language: {} (detected from {}). \
                 Supported: Rust, Python, TypeScript, JavaScript, Go, Java, \
                 C#, Ruby, PHP, Swift, Kotlin, C, C++, HCL, XML, Protobuf, \
                 TOML, YAML, JSON, Shell.",
                lang, args.path,
            );
            return Ok(exit::NO_MATCHES);
        }
        let symbols = symbols::extract_symbols_from_file(&target, Some(lang));
        let filtered = filter_symbols(&symbols, &kind_filter);
        if !filtered.is_empty() {
            any_output = true;
            if global.json || global.jsonl {
                print_symbols_json(&args.path, &filtered, global)?;
            } else if args.compact {
                print_symbols_compact(&args.path, &filtered);
            } else {
                print_symbols_human(&args.path, &filtered);
            }
        }
    } else if target.is_dir() {
        let paths = collect_source_files(&target, global)?;
        crate::verbose!("ast list: scanning {} files in {}", paths.len(), args.path);

        struct ListFileResult {
            display: String,
            symbols: Vec<SymbolDef>,
        }

        let results: Vec<ListFileResult> = crate::par_process_files(&paths, None, &[], |path| {
            let lang = resolve_lang(lang_hint, path);
            let symbols = symbols::extract_symbols_from_file(path, Some(lang));
            if symbols.is_empty() {
                return None;
            }
            let display = display_path(path, &cwd);
            Some(ListFileResult { display, symbols })
        });

        for result in &results {
            let filtered = filter_symbols(&result.symbols, &kind_filter);
            if filtered.is_empty() {
                continue;
            }
            any_output = true;
            if global.json || global.jsonl {
                print_symbols_json(&result.display, &filtered, global)?;
            } else if args.compact {
                print_symbols_compact(&result.display, &filtered);
            } else {
                print_symbols_human(&result.display, &filtered);
            }
        }
    } else {
        anyhow::bail!("path not found: {}", args.path);
    }

    if any_output {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::NO_MATCHES)
    }
}

// ---------------------------------------------------------------------------
// ast read
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// File to read from.
    pub path: String,

    /// Symbol name (e.g. "run" or "Server::start").
    pub symbol: String,

    /// Number of context lines before/after the symbol.
    #[arg(long, short, default_value = "0")]
    pub context: usize,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_read(args: ReadArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (_cwd, _target, lang, source) =
        setup_single_file(&args.path, args.lang.as_deref(), global)?;
    crate::verbose!(
        "ast read: file={}, symbol={}, lang={lang}",
        args.path,
        args.symbol
    );
    let all_symbols = symbols::extract_symbols(&source, lang);
    let sym = symbols::find_symbol(&all_symbols, &args.symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol '{}' not found in {}", args.symbol, args.path))?;

    let lines: Vec<&str> = source.lines().collect();
    let start = sym
        .start_line
        .saturating_sub(args.context.saturating_add(1));
    let end = sym.end_line.saturating_add(args.context).min(lines.len());

    if global.json || global.jsonl {
        let content: String = lines[start..end].iter().map(|l| format!("{l}\n")).collect();
        let obj = serde_json::json!({
            "file": args.path,
            "symbol": sym.name,
            "kind": sym.kind.to_string(),
            "start_line": sym.start_line,
            "end_line": sym.end_line,
            "signature": sym.signature,
            "content": content,
        });
        global.emit_json(&obj)?;
    } else {
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            println!("{line_num:>4} | {line}");
        }
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// ast rename
// ---------------------------------------------------------------------------

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

pub fn resolve_target_paths(
    target: &std::path::Path,
    path_arg: &str,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    if target.is_file() {
        Ok(vec![target.to_path_buf()])
    } else if target.is_dir() {
        collect_source_files(target, global)
    } else {
        anyhow::bail!("path not found: {}", path_arg)
    }
}

fn run_rename(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
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
        return Ok(exit::NO_MATCHES);
    }

    let files_changed = operations.len();
    let options = ExecuteOptions {
        cwd: &cwd,
        global,
        guard: None,
    };
    let result = crate::tx::engine::execute_operations(operations, options)?;

    if global.check {
        if global.json || global.jsonl {
            let obj = serde_json::json!({
                "ok": true,
                "files_changed": files_changed,
            });
            global.emit_json(&obj)?;
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        let diffs = result.build_diffs();
        result.commit()?;
        crate::write::run_format_command(global, &cwd)?;

        if global.json || global.jsonl {
            let obj = serde_json::json!({
                "ok": true,
                "files_changed": diffs.len(),
            });
            global.emit_json(&obj)?;
        } else if !global.quiet {
            for d in &diffs {
                eprintln!("{}: renamed", d.path);
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default preview mode
    let diffs = result.build_diffs();
    if !diffs.is_empty() {
        let colored = crate::diff::render_diffs_colored(&diffs, global.should_color());
        print!("{colored}");
    }

    if global.should_apply() {
        result.commit()?;
        crate::write::run_format_command(global, &cwd)?;
        return Ok(exit::SUCCESS);
    }

    Ok(exit::CHANGES_DETECTED)
}

// ---------------------------------------------------------------------------
// ast validate
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// File or directory to validate.
    pub path: String,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_validate(args: ValidateArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    let lang_hint = args.lang.as_deref();
    crate::verbose!("ast validate: target={}", args.path);

    let mut all_valid = true;
    crate::verbose!("ast validate: checking {} files", paths.len());

    struct ValidateFileResult {
        display: String,
        result: crate::ast::validate::ValidationResult,
    }

    let results: Vec<ValidateFileResult> = crate::par_process_files(&paths, None, &[], |path| {
        let lang = resolve_lang(lang_hint, path);
        if !lang.has_grammar() {
            return None;
        }
        let result = crate::ast::validate::validate_file(path, Some(lang)).ok()?;
        let display = display_path(path, &cwd);
        Some(ValidateFileResult { display, result })
    });

    for vr in &results {
        if !vr.result.valid {
            all_valid = false;
        }
        if global.json || global.jsonl {
            let obj = serde_json::json!({
                "file": vr.display,
                "valid": vr.result.valid,
                "language": vr.result.language,
                "errors": vr.result.errors,
            });
            global.emit_json(&obj)?;
        } else if !vr.result.valid {
            eprintln!("{}: INVALID ({})", vr.display, vr.result.language);
            for err in &vr.result.errors {
                eprintln!("  line {}:{}: {}", err.line, err.column, err.text.trim());
            }
        } else if !global.quiet {
            eprintln!("{}: OK ({})", vr.display, vr.result.language);
        }
    }

    if all_valid {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::FAILURE)
    }
}

// ---------------------------------------------------------------------------
// ast search
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Tree-sitter S-expression query, or a code pattern (with --pattern).
    pub query: String,

    /// File or directory to search.
    pub path: String,

    /// Treat the query as a code pattern with meta-variables ($VAR, $$$MULTI).
    #[arg(long)]
    pub pattern: bool,

    /// Language hint (required for pattern mode; detected from extension otherwise).
    #[arg(long)]
    pub lang: Option<String>,

    /// Maximum number of results.
    #[arg(long)]
    pub max_results: Option<usize>,
}

fn run_search(args: SearchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    let lang_hint = args.lang.as_deref();
    crate::verbose!(
        "ast search: query={}, pattern={}, target={}",
        args.query,
        args.pattern,
        args.path
    );

    let mut total_matches = 0usize;
    crate::verbose!("ast search: scanning {} files", paths.len());

    struct SearchFileResult {
        display: String,
        matches: Vec<crate::ast::search::SearchMatch>,
    }

    let file_results: Vec<SearchFileResult> = crate::par_process_files(&paths, None, &[], |path| {
        let lang = resolve_lang(lang_hint, path);
        let query_str = if args.pattern {
            crate::ast::search::compile_pattern_query(&args.query, lang).ok()?
        } else {
            args.query.clone()
        };
        let matches =
            crate::ast::search::search_file(path, &query_str, Some(lang), args.max_results).ok()?;
        if matches.is_empty() {
            return None;
        }
        let display = display_path(path, &cwd);
        Some(SearchFileResult { display, matches })
    });

    for result in &file_results {
        for m in &result.matches {
            total_matches += 1;
            if global.json || global.jsonl {
                let obj = serde_json::json!({
                    "file": result.display,
                    "line": m.line,
                    "column": m.column,
                    "text": m.text,
                    "captures": m.captures,
                });
                global.emit_json(&obj)?;
            } else {
                println!(
                    "{}:{}:{}: {}",
                    result.display,
                    m.line,
                    m.column,
                    m.text.lines().next().unwrap_or("")
                );
                for cap in &m.captures {
                    println!("  @{} = \"{}\"", cap.name, cap.text);
                }
            }
        }
    }

    if total_matches == 0 {
        Ok(exit::NO_MATCHES)
    } else {
        Ok(exit::SUCCESS)
    }
}

// ---------------------------------------------------------------------------
// ast refs
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct RefsArgs {
    /// Symbol name to find references for.
    pub symbol: String,

    /// File or directory to search.
    pub path: String,

    /// Include the definition site in results.
    #[arg(long)]
    pub include_def: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_refs(args: RefsArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    let lang_hint = args.lang.as_deref();
    crate::verbose!("ast refs: symbol={}, target={}", args.symbol, args.path);
    crate::verbose!("ast refs: scanning {} files", paths.len());

    let per_file_refs: Vec<Vec<crate::ast::refs::SymbolRef>> =
        crate::par_process_files(&paths, None, &[], |path| {
            let display = display_path(path, &cwd);
            let lang = lang_hint.map(lang_from_str);
            let refs = crate::ast::refs::find_refs_in_file(path, &args.symbol, lang, &display);
            if refs.is_empty() { None } else { Some(refs) }
        });

    let mut all_refs: Vec<_> = per_file_refs.into_iter().flatten().collect();

    if !args.include_def {
        all_refs.retain(|r| r.kind != crate::ast::refs::RefKind::Definition);
    }

    if all_refs.is_empty() {
        return Ok(exit::NO_MATCHES);
    }

    if global.json || global.jsonl {
        let obj = serde_json::json!({
            "symbol": args.symbol,
            "references": all_refs,
            "count": all_refs.len(),
        });
        global.emit_json(&obj)?;
    } else {
        for r in &all_refs {
            let kind_label = match r.kind {
                crate::ast::refs::RefKind::Definition => "def",
                crate::ast::refs::RefKind::Reference => "ref",
            };
            println!("{}:{}: [{}] {}", r.file, r.line, kind_label, r.context);
        }
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// ast deps
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct DepsArgs {
    /// File or directory to analyze.
    pub path: String,

    /// Show reverse dependencies (what imports this file).
    #[arg(long)]
    pub reverse: bool,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_deps(args: DepsArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);
    crate::verbose!("ast deps: target={}, reverse={}", args.path, args.reverse);

    let paths = resolve_target_paths(&target, &args.path, global)?;
    crate::verbose!("ast deps: scanning {} files", paths.len());

    let mut any_output = false;

    if args.reverse {
        // For reverse deps, scan all files and find which ones import
        // anything matching the target file's module path
        let target_name = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        let scan_dir = if target.is_file() {
            target.parent().unwrap_or(&cwd).to_path_buf()
        } else {
            target.clone()
        };
        let all_files = collect_source_files(&scan_dir, global)?;

        for path in &all_files {
            let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
            let matching: Vec<_> = imports
                .iter()
                .filter(|i| i.path.contains(target_name))
                .collect();
            if matching.is_empty() {
                continue;
            }
            any_output = true;
            let display = display_path(path, &cwd);

            if global.json || global.jsonl {
                for imp in &matching {
                    let obj = serde_json::json!({
                        "file": display,
                        "imports": imp.path,
                        "line": imp.line,
                        "raw": imp.raw,
                    });
                    global.emit_json(&obj)?;
                }
            } else {
                for imp in &matching {
                    println!("{}:{}: {}", display, imp.line, imp.raw);
                }
            }
        }
    } else {
        struct DepsFileResult {
            display: String,
            imports: Vec<crate::ast::deps::Import>,
        }

        let results: Vec<DepsFileResult> = crate::par_process_files(&paths, None, &[], |path| {
            let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
            if imports.is_empty() {
                return None;
            }
            let display = display_path(path, &cwd);
            Some(DepsFileResult { display, imports })
        });

        for result in &results {
            any_output = true;
            if global.json || global.jsonl {
                let obj = serde_json::json!({
                    "file": result.display,
                    "imports": result.imports,
                });
                global.emit_json(&obj)?;
            } else {
                println!("{}", result.display);
                println!("  imports:");
                for imp in &result.imports {
                    println!("    {}", imp.path);
                }
                println!();
            }
        }
    }

    if any_output {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::NO_MATCHES)
    }
}

// ---------------------------------------------------------------------------
// ast map
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct MapArgs {
    /// Directory to map.
    pub path: String,

    /// Maximum approximate token count for output.
    #[arg(long, default_value = "1024")]
    pub max_tokens: usize,

    /// Boost symbols from these files (comma-separated paths).
    #[arg(long, value_delimiter = ',')]
    pub focus: Vec<String>,

    /// Boost these symbol names (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub boost: Vec<String>,
}

fn run_map(args: MapArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    crate::verbose!(
        "ast map: target={}, max_tokens={}",
        args.path,
        args.max_tokens
    );

    if !target.is_dir() {
        anyhow::bail!("path must be a directory: {}", args.path);
    }

    let paths = collect_source_files(&target, global)?;
    crate::verbose!("ast map: collected {} source files", paths.len());
    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|p| {
            let display = display_path(p, &cwd);
            (p.clone(), display)
        })
        .collect();

    let opts = crate::ast::map::MapOptions {
        max_tokens: args.max_tokens,
        focus: &args.focus,
        boost: &args.boost,
    };

    let entries = crate::ast::map::generate_map(&file_pairs, &opts);

    if entries.is_empty() {
        return Ok(exit::NO_MATCHES);
    }

    if global.jsonl {
        for entry in &entries {
            global.emit_json(entry)?;
        }
    } else if global.json {
        global.emit_json(&entries)?;
    } else {
        print!("{}", crate::ast::map::render_tree(&entries));
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// ast replace
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ReplaceArgs {
    /// File containing the symbol.
    pub path: String,

    /// Symbol name to scope the replacement to.
    pub symbol: String,

    /// Text or regex pattern to find.
    #[arg(long)]
    pub from: String,

    /// Replacement text.
    #[arg(long)]
    pub to: String,

    /// Treat --from as a regex pattern.
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

fn run_replace(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "ast replace: symbol={}, from={}, regex={}",
        args.symbol,
        args.from,
        args.regex
    );

    let op = Operation::AstReplace {
        path: args.path.clone(),
        symbol: args.symbol.clone(),
        from: args.from.clone(),
        to: args.to.clone(),
        regex: args.regex,
        lang: args.lang.clone(),
    };

    let symbol = args.symbol.clone();
    let check_msg = format!("would replace in symbol '{symbol}' in {}", args.path);
    let apply_msg = format!("replaced in symbol '{symbol}' in {}", args.path);

    match execute_via_engine(
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
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("no matches") {
                if !global.quiet {
                    eprintln!("{msg}");
                }
                Ok(exit::NO_MATCHES)
            } else {
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ast impact
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ImpactArgs {
    /// Symbol name to analyze.
    pub symbol: String,

    /// Directory to scan for references.
    pub path: String,

    /// Maximum traversal depth (1 = direct refs only).
    #[arg(long, default_value = "3")]
    pub depth: usize,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_impact(args: ImpactArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    crate::verbose!("ast impact: symbol={}, depth={}", args.symbol, args.depth);
    crate::verbose!("ast impact: scanning {} files", paths.len());

    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|p| {
            let display = display_path(p, &cwd);
            (p.clone(), display)
        })
        .collect();

    let nodes = crate::ast::impact::compute_impact(&args.symbol, &file_pairs, args.depth);

    if nodes.is_empty() {
        if !global.quiet {
            eprintln!("no references found for '{}'", args.symbol);
        }
        return Ok(exit::NO_MATCHES);
    }

    if global.json || global.jsonl {
        let obj = serde_json::json!({
            "symbol": args.symbol,
            "depth": args.depth,
            "impact": nodes,
            "direct_count": nodes.len(),
        });
        global.emit_json(&obj)?;
    } else {
        print!(
            "{}",
            crate::ast::impact::render_impact_tree(&args.symbol, &nodes, 0)
        );
    }

    Ok(exit::SUCCESS)
}

// ---------------------------------------------------------------------------
// ast diff
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct DiffArgs {
    /// File to diff.
    pub path: String,

    /// Git ref for the "old" version (default: HEAD).
    #[arg(long, default_value = "HEAD")]
    pub from: String,

    /// Git ref for the "new" version (default: working tree).
    #[arg(long)]
    pub to: Option<String>,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

fn run_diff(args: DiffArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let target = cwd.join(&args.path);
    let lang = resolve_lang(args.lang.as_deref(), &target);
    crate::verbose!(
        "ast diff: file={}, from={}, lang={lang}",
        args.path,
        args.from
    );

    // Get old version from git
    let old_source = get_git_file_content(&cwd, &args.path, &args.from)?;

    // Get new version
    let new_source = if let Some(ref to_ref) = args.to {
        get_git_file_content(&cwd, &args.path, to_ref)?
    } else {
        std::fs::read_to_string(&target).with_context(|| format!("reading {}", args.path))?
    };

    let changes = crate::ast::diff::structural_diff(&old_source, &new_source, lang);

    if changes.is_empty() {
        if !global.quiet {
            eprintln!("no structural changes");
        }
        return Ok(exit::NO_MATCHES);
    }

    if global.json || global.jsonl {
        let obj = serde_json::json!({
            "file": args.path,
            "from": args.from,
            "to": args.to.as_deref().unwrap_or("working tree"),
            "changes": changes,
        });
        global.emit_json(&obj)?;
    } else {
        print!("{}", crate::ast::diff::render_changes(&args.path, &changes));
    }

    Ok(exit::SUCCESS)
}

pub fn get_git_file_content(
    cwd: &std::path::Path,
    file_path: &str,
    git_ref: &str,
) -> anyhow::Result<String> {
    if git_ref.starts_with('-') {
        anyhow::bail!("invalid git ref: must not start with '-'");
    }
    let output = std::process::Command::new("git")
        .args(["show", &format!("{git_ref}:{file_path}")])
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git show {git_ref}:{file_path} failed: {}", stderr.trim());
    }

    Ok(String::from_utf8(output.stdout)?)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn lang_from_str(s: &str) -> Language {
    Language::from_extension(s)
}

pub use symbols::{filter_symbols, parse_kind_filter};

pub fn collect_source_files(
    dir: &std::path::Path,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let dir_str = dir.to_string_lossy().into_owned();
    let mut all_paths = crate::collect_file_paths_opts(&[dir_str], global, true, None)?;
    // Filter to files with a tree-sitter grammar.
    all_paths.retain(|p| Language::from_path(p).has_grammar());
    all_paths.sort();
    Ok(all_paths)
}

fn print_symbols_human(path: &str, symbols: &[&SymbolDef]) {
    println!("{path}");
    for sym in symbols {
        print_symbol_human(sym, 1);
    }
    println!();
}

fn print_symbol_human(sym: &SymbolDef, indent: usize) {
    let pad = "  ".repeat(indent);
    println!(
        "{pad}{} {} [{}:{}]",
        sym.kind, sym.name, sym.start_line, sym.end_line
    );
    for child in &sym.children {
        print_symbol_human(child, indent + 1);
    }
}

fn print_symbols_compact(path: &str, symbols: &[&SymbolDef]) {
    println!("{path}");
    println!("|----");
    for sym in symbols {
        print_symbol_compact(sym, 0);
    }
    println!("|----");
    println!();
}

fn print_symbol_compact(sym: &SymbolDef, indent: usize) {
    let pad = "  ".repeat(indent);
    println!("|{pad}{} {}", sym.kind, sym.name);
    for child in &sym.children {
        print_symbol_compact(child, indent + 1);
    }
}

fn print_symbols_json(
    path: &str,
    symbols: &[&SymbolDef],
    global: &GlobalFlags,
) -> anyhow::Result<()> {
    for sym in symbols {
        let obj = symbol_to_json(sym, path);
        global.emit_json(&obj)?;
    }
    Ok(())
}

pub fn symbol_to_json(sym: &SymbolDef, path: &str) -> serde_json::Value {
    let children: Vec<serde_json::Value> = sym
        .children
        .iter()
        .map(|c| symbol_to_json(c, path))
        .collect();
    serde_json::json!({
        "file": path,
        "name": sym.name,
        "kind": sym.kind.to_string(),
        "start_line": sym.start_line,
        "end_line": sym.end_line,
        "signature": sym.signature,
        "children": children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::symbols::SymbolKind;

    #[test]
    fn parse_kind_filter_works() {
        let filter = parse_kind_filter(&Some("function,struct".to_string()));
        assert_eq!(filter.len(), 2);
        assert!(filter.contains(&SymbolKind::Function));
        assert!(filter.contains(&SymbolKind::Struct));
    }

    #[test]
    fn parse_kind_filter_empty() {
        let filter = parse_kind_filter(&None);
        assert!(filter.is_empty());
    }

    #[test]
    fn lang_from_str_works() {
        assert_eq!(lang_from_str("rs"), Language::Rust);
        assert_eq!(lang_from_str("py"), Language::Python);
        assert_eq!(lang_from_str("go"), Language::Go);
    }

    // -----------------------------------------------------------------------
    // resolve_target_paths
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_target_paths_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let f = dir.path().join("hello.rs");
        std::fs::write(&f, "fn main() {}\n").unwrap();
        let global = GlobalFlags::default();
        let result = resolve_target_paths(&f, "hello.rs", &global).unwrap();
        assert_eq!(result, vec![f]);
    }

    #[test]
    fn resolve_target_paths_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let f = dir.path().join("lib.rs");
        std::fs::write(&f, "fn foo() {}\n").unwrap();
        // Also create a non-source file that should be skipped.
        std::fs::write(dir.path().join("notes.txt"), "not source").unwrap();
        let global = GlobalFlags::default();
        let result = resolve_target_paths(dir.path(), ".", &global).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("lib.rs"));
    }

    #[test]
    fn resolve_target_paths_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("no_such_file.rs");
        let global = GlobalFlags::default();
        let err = resolve_target_paths(&missing, "no_such_file.rs", &global).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("path not found") && msg.contains("no_such_file.rs"),
            "error should mention path not found and the argument: {msg}"
        );
    }

    #[test]
    fn context_line_calculation_no_overflow() {
        // Regression: 1 + args.context and sym.end_line + args.context
        // used to overflow with large context values. Verify saturating
        // arithmetic prevents panic.
        let start_line: usize = 5;
        let end_line: usize = 10;
        let context: usize = usize::MAX;
        let total_lines: usize = 100;

        let start = start_line.saturating_sub(context.saturating_add(1));
        let end = end_line.saturating_add(context).min(total_lines);
        assert_eq!(start, 0);
        assert_eq!(end, total_lines);
    }

    // Note: apply_or_preview tests removed when AST write commands were
    // migrated to use the tx engine (execute_via_engine / execute_operations).
    // The tx engine has its own comprehensive tests in src/cmd/output.rs
    // and tests/integration.rs.
}
