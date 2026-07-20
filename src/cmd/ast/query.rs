//! Read-only `patchloom ast` subcommands (list, read, validate, search, refs, deps, map, impact, diff).

use super::common::{
    collect_source_files, display_path, filter_symbols, get_git_file_content, lang_from_str,
    parse_kind_filter, print_symbol_items_json, print_symbols_compact, print_symbols_human,
    print_symbols_json, resolve_lang, resolve_target_paths, setup_multi_file, setup_single_file,
    symbol_to_json,
};
use crate::ast::symbols::{self, SymbolDef};
use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::Context;
use clap::Args;

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

pub(super) fn run_list(args: ListArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    // Match setup_multi_file / read-side commands: --contain must reject ../ escapes
    // before reading source outside the workspace (MPI 2026-07-07).
    global.check_paths_contained(&cwd, [args.path.as_str()])?;
    let target = cwd.join(&args.path);

    let kind_filter = parse_kind_filter(&args.kind);
    let lang_hint = args.lang.as_deref();
    crate::verbose!(
        "ast list: target={}, kind_filter={:?}",
        args.path,
        kind_filter
    );

    let mut any_output = false;
    // Structured multi-file output must be one array (or one JSONL stream), not
    // one pretty document per file/symbol (fixrealloop 2026-07-15).
    let mut structured_items: Vec<serde_json::Value> = Vec::new();
    let structured = global.json || global.jsonl;

    if target.is_file() {
        // Binary before "unsupported language" so agents do not treat NUL files
        // as a soft language miss (parity with replace/tidy/md).
        if let Err(err) = crate::ops::file::ensure_not_binary_file(&target, &args.path) {
            global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
            return Ok(exit::FAILURE);
        }
        let lang = resolve_lang(lang_hint, &target);
        crate::verbose!("ast list: detected language={lang} for {}", args.path);
        if !lang.has_grammar() {
            let msg = format!(
                "Unsupported language: {} (detected from {}). \
                 Supported: Rust, Python, TypeScript, JavaScript, Go, Java, \
                 C#, Ruby, PHP, Swift, Kotlin, C, C++, HCL, XML, Protobuf, \
                 TOML, YAML, JSON, Shell.",
                lang, args.path,
            );
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
        let symbols = symbols::extract_symbols_from_file(&target, Some(lang));
        let filtered = filter_symbols(&symbols, &kind_filter);
        if !filtered.is_empty() {
            any_output = true;
            if structured {
                print_symbols_json(&args.path, &filtered, global)?;
            } else if args.compact {
                print_symbols_compact(&args.path, &filtered);
            } else {
                print_symbols_human(&args.path, &filtered);
            }
        }
    } else if target.is_dir() {
        let paths = collect_source_files(&target, global)?;
        let glob_matcher = crate::build_glob_matcher_from_global(global)?;
        let glob_roots = vec![target.clone()];
        crate::verbose!("ast list: scanning {} files in {}", paths.len(), args.path);

        struct ListFileResult {
            display: String,
            symbols: Vec<SymbolDef>,
        }

        let results: Vec<ListFileResult> =
            crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
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
            if structured {
                for sym in &filtered {
                    structured_items.push(symbol_to_json(sym, &result.display));
                }
            } else if args.compact {
                print_symbols_compact(&result.display, &filtered);
            } else {
                print_symbols_human(&result.display, &filtered);
            }
        }
        if structured && !structured_items.is_empty() {
            print_symbol_items_json(&structured_items, global)?;
        }
    } else {
        let msg = format!("path not found: {}", args.path);
        global.emit_error_json_kind(Some("not_found"), &msg)?;
        return Ok(exit::FAILURE);
    }

    if any_output {
        Ok(exit::SUCCESS)
    } else {
        let msg = format!("no symbols found in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        Ok(exit::NO_MATCHES)
    }
}

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

pub(super) fn run_read(args: ReadArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (_cwd, _target, lang, source) =
        setup_single_file(&args.path, args.lang.as_deref(), global)?;
    crate::verbose!(
        "ast read: file={}, symbol={}, lang={lang}",
        args.path,
        args.symbol
    );
    let all_symbols = symbols::extract_symbols(&source, lang);
    let sym = match symbols::find_symbol(&all_symbols, &args.symbol) {
        Some(s) => s,
        None => {
            let msg = format!("symbol '{}' not found in {}", args.symbol, args.path);
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
    };

    let lines: Vec<&str> = source.lines().collect();
    let start = sym
        .start_line
        .saturating_sub(args.context.saturating_add(1));
    let end = sym.end_line.saturating_add(args.context).min(lines.len());

    let content: String = lines[start..end].iter().map(|l| format!("{l}\n")).collect();
    if !global.emit_json(&serde_json::json!({
        "file": args.path,
        "symbol": sym.name,
        "kind": sym.kind.to_string(),
        "start_line": sym.start_line,
        "end_line": sym.end_line,
        "signature": sym.signature,
        "content": content,
    }))? {
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            println!("{line_num:>4} | {line}");
        }
    }

    Ok(exit::SUCCESS)
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// File or directory to validate.
    pub path: String,

    /// Language hint.
    #[arg(long)]
    pub lang: Option<String>,
}

pub(super) fn run_validate(args: ValidateArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    let lang_hint = args.lang.as_deref();
    crate::verbose!("ast validate: target={}", args.path);

    // Empty directory / no grammar files: fail closed (not vacuous success).
    if paths.is_empty() {
        let msg = format!("no source files to validate in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if let Err(err) = super::common::reject_sole_explicit_binary(&paths, &args.path) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }

    // Single explicit path with no grammar: same honesty as `ast list` so agents
    // do not treat `[]` + exit 0 as "validated OK".
    if paths.len() == 1 {
        let lang = resolve_lang(lang_hint, &paths[0]);
        if !lang.has_grammar() {
            let msg = format!(
                "Unsupported language: {} (detected from {}). \
                 Supported: Rust, Python, TypeScript, JavaScript, Go, Java, \
                 C#, Ruby, PHP, Swift, Kotlin, C, C++, HCL, XML, Protobuf, \
                 TOML, YAML, JSON, Shell.",
                lang, args.path,
            );
            global.emit_error_json_kind(Some("no_matches"), &msg)?;
            return Ok(exit::NO_MATCHES);
        }
    }

    let mut all_valid = true;
    crate::verbose!("ast validate: checking {} files", paths.len());

    struct ValidateFileResult {
        display: String,
        result: crate::ast::validate::ValidationResult,
    }

    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let glob_roots = vec![cwd.join(&args.path)];
    let results: Vec<ValidateFileResult> =
        crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let lang = resolve_lang(lang_hint, path);
            if !lang.has_grammar() {
                return None;
            }
            let result = crate::ast::validate::validate_file(path, Some(lang)).ok()?;
            let display = display_path(path, &cwd);
            Some(ValidateFileResult { display, result })
        });

    // Directory walk can include only non-grammar files filtered inside the
    // loop, or every validate_file call can fail. Do not report success with
    // zero checks performed.
    if results.is_empty() {
        let msg = format!("no source files to validate in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    let structured = global.json || global.jsonl;
    let mut structured_items: Vec<serde_json::Value> = Vec::new();
    for vr in &results {
        if !vr.result.valid {
            all_valid = false;
        }
        if structured {
            structured_items.push(serde_json::json!({
                "file": vr.display,
                "valid": vr.result.valid,
                "language": vr.result.language,
                "errors": vr.result.errors,
            }));
        } else if !global.quiet {
            if !vr.result.valid {
                eprintln!("{}: INVALID ({})", vr.display, vr.result.language);
                for err in &vr.result.errors {
                    eprintln!("  line {}:{}: {}", err.line, err.column, err.text.trim());
                }
            } else {
                eprintln!("{}: OK ({})", vr.display, vr.result.language);
            }
        }
    }
    // One array for --json (agent-parseable); one line per file for --jsonl.
    if structured {
        global.emit_json_items(&structured_items)?;
    }

    if all_valid {
        Ok(exit::SUCCESS)
    } else {
        Ok(exit::FAILURE)
    }
}

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

pub(super) fn run_search(args: SearchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    if let Err(err) = super::common::reject_sole_explicit_binary(&paths, &args.path) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }
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

    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let glob_roots = vec![cwd.join(&args.path)];
    let file_results: Vec<SearchFileResult> =
        crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
            let lang = resolve_lang(lang_hint, path);
            let query_str = if args.pattern {
                match crate::ast::search::compile_pattern_query(&args.query, lang) {
                    Ok(q) => q,
                    Err(e) => {
                        if !global.quiet {
                            eprintln!(
                                "patchloom: pattern compile error for {}: {e}",
                                path.display()
                            );
                        }
                        return None;
                    }
                }
            } else {
                args.query.clone()
            };
            let matches =
                crate::ast::search::search_file(path, &query_str, Some(lang), args.max_results)
                    .ok()?;
            if matches.is_empty() {
                return None;
            }
            let display = display_path(path, &cwd);
            Some(SearchFileResult { display, matches })
        });

    let structured = global.json || global.jsonl;
    let mut structured_items: Vec<serde_json::Value> = Vec::new();
    'outer: for result in &file_results {
        for m in &result.matches {
            total_matches += 1;
            if structured {
                structured_items.push(serde_json::json!({
                    "file": result.display,
                    "line": m.line,
                    "column": m.column,
                    "text": m.text,
                    "captures": m.captures,
                }));
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
            if let Some(max) = args.max_results
                && total_matches >= max
            {
                break 'outer;
            }
        }
    }

    if total_matches == 0 {
        let msg = format!("no matches for pattern in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        Ok(exit::NO_MATCHES)
    } else {
        if structured {
            global.emit_json_items(&structured_items)?;
        }
        Ok(exit::SUCCESS)
    }
}

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

pub(super) fn run_refs(args: RefsArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    if let Err(err) = super::common::reject_sole_explicit_binary(&paths, &args.path) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }
    let lang_hint = args.lang.as_deref();
    crate::verbose!("ast refs: symbol={}, target={}", args.symbol, args.path);
    crate::verbose!("ast refs: scanning {} files", paths.len());

    let glob_matcher = crate::build_glob_matcher_from_global(global)?;
    let glob_roots = vec![cwd.join(&args.path)];
    let per_file_refs: Vec<Vec<crate::ast::refs::SymbolRef>> =
        crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
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
        let msg = format!("no references found for '{}' in {}", args.symbol, args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json(&serde_json::json!({
        "symbol": args.symbol,
        "references": all_refs,
        "count": all_refs.len(),
    }))? {
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

pub(super) fn run_deps(args: DepsArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [args.path.as_str()])?;
    let target = cwd.join(&args.path);
    let lang_hint = args.lang.as_deref().map(lang_from_str);
    crate::verbose!("ast deps: target={}, reverse={}", args.path, args.reverse);

    let paths = resolve_target_paths(&target, &args.path, global)?;
    crate::verbose!("ast deps: scanning {} files", paths.len());

    let mut any_output = false;
    let structured = global.json || global.jsonl;
    let mut structured_items: Vec<serde_json::Value> = Vec::new();

    if args.reverse {
        // For reverse deps, scan all files and find which ones import
        // anything matching the target file's module path
        let target_name = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        // Scan from the project root (cwd), not just the target's parent
        // directory, to find importers anywhere in the project.
        let all_files = collect_source_files(&cwd, global)?;

        struct ReverseHit {
            display: String,
            matching: Vec<crate::ast::deps::Import>,
        }

        let hits: Vec<ReverseHit> = crate::par_process_files(&all_files, None, &[], |path| {
            let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
            // Use segment-boundary matching to avoid substring false positives.
            // Split import paths on common separators (::, /, .) and check if
            // any segment exactly equals the target file stem.
            let matching: Vec<_> = imports
                .into_iter()
                .filter(|i| {
                    i.path
                        .split(['/', '.'])
                        .flat_map(|seg| seg.split("::"))
                        .any(|seg| seg == target_name)
                })
                .collect();
            if matching.is_empty() {
                return None;
            }
            Some(ReverseHit {
                display: display_path(path, &cwd),
                matching,
            })
        });

        for hit in &hits {
            any_output = true;
            if structured {
                for imp in &hit.matching {
                    structured_items.push(serde_json::json!({
                        "file": hit.display,
                        "imports": imp.path,
                        "line": imp.line,
                        "raw": imp.raw,
                    }));
                }
            } else {
                for imp in &hit.matching {
                    println!("{}:{}: {}", hit.display, imp.line, imp.raw);
                }
            }
        }
    } else {
        struct DepsFileResult {
            display: String,
            imports: Vec<crate::ast::deps::Import>,
        }

        let glob_matcher = crate::build_glob_matcher_from_global(global)?;
        let glob_roots = vec![cwd.join(&args.path)];
        let results: Vec<DepsFileResult> =
            crate::par_process_files(&paths, glob_matcher.as_ref(), &glob_roots, |path| {
                let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
                if imports.is_empty() {
                    return None;
                }
                let display = display_path(path, &cwd);
                Some(DepsFileResult { display, imports })
            });

        for result in &results {
            any_output = true;
            if structured {
                structured_items.push(serde_json::json!({
                    "file": result.display,
                    "imports": result.imports,
                }));
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
        if structured {
            global.emit_json_items(&structured_items)?;
        }
        Ok(exit::SUCCESS)
    } else {
        let msg = format!("no imports found in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        Ok(exit::NO_MATCHES)
    }
}

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

pub(super) fn run_map(args: MapArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [args.path.as_str()])?;
    let target = cwd.join(&args.path);
    crate::verbose!(
        "ast map: target={}, max_tokens={}",
        args.path,
        args.max_tokens
    );

    if !target.is_dir() {
        let msg = format!("path must be a directory: {}", args.path);
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(exit::FAILURE);
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
        let msg = format!("no symbols found in {}", args.path);
        global.emit_error_json_kind(Some("no_matches"), &msg)?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json_items(&entries)? {
        print!("{}", crate::ast::map::render_tree(&entries));
    }

    Ok(exit::SUCCESS)
}

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

pub(super) fn run_impact(args: ImpactArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (cwd, paths) = setup_multi_file(&args.path, global)?;
    if let Err(err) = super::common::reject_sole_explicit_binary(&paths, &args.path) {
        global.emit_error_json_kind(Some("invalid_input"), &err.msg)?;
        return Ok(exit::FAILURE);
    }
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
        global.emit_error_json_kind(
            Some("no_matches"),
            &format!("no references found for '{}'", args.symbol),
        )?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json(&serde_json::json!({
        "symbol": args.symbol,
        "depth": args.depth,
        "impact": nodes,
        "direct_count": nodes.len(),
    }))? {
        print!(
            "{}",
            crate::ast::impact::render_impact_tree(&args.symbol, &nodes, 0)
        );
    }

    Ok(exit::SUCCESS)
}

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

pub(super) fn run_diff(args: DiffArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [args.path.as_str()])?;
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
        global.emit_error_json_kind(Some("no_matches"), "no structural changes")?;
        return Ok(exit::NO_MATCHES);
    }

    if !global.emit_json(&serde_json::json!({
        "file": args.path,
        "from": args.from,
        "to": args.to.as_deref().unwrap_or("working tree"),
        "changes": changes,
    }))? {
        print!("{}", crate::ast::diff::render_changes(&args.path, &changes));
    }

    Ok(exit::SUCCESS)
}
